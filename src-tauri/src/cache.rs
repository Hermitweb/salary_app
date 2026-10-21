//! 数据缓存：以文件修改时间为依据的 Moka 缓存。

use crate::error::AppError;
use crate::excel_parser::EmployeesData;
use crate::models::{CacheStats, SalaryRow};
use calamine::Reader;
use moka::sync::Cache as MokaCache;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::UNIX_EPOCH;

/// 读取文件的修改时间（纳秒）。读不到时返回 0。
fn modified_ns(path: &Path) -> u64 {
    std::fs::metadata(path)
        .ok()
        .and_then(|meta| meta.modified().ok())
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0)
}

/// 工资表解析结果：表头 + 数据行。
type SalaryTable = (Vec<String>, Vec<SalaryRow>);

/// 缓存条目：值 + 写入时的文件修改时间。
#[derive(Clone)]
struct Entry<T> {
    value: T,
    mtime_ns: u64,
}

/// 数据缓存：员工表与工资表解析结果。
///
/// 缓存以「文件路径 + 修改时间」为键，Excel 文件被替换后会自动重新解析，
/// 不需要重启服务。
pub struct DataCache {
    employees: MokaCache<String, Entry<Arc<EmployeesData>>>,
    salaries: MokaCache<String, Entry<SalaryTable>>,
    hits: AtomicUsize,
    misses: AtomicUsize,
}

impl DataCache {
    /// 创建缓存实例。
    pub fn new() -> Self {
        Self {
            // 员工表 ~500 人
            employees: MokaCache::new(500),
            // 工资表 ~36 个月
            salaries: MokaCache::new(36),
            hits: AtomicUsize::new(0),
            misses: AtomicUsize::new(0),
        }
    }

    /// 加载员工表（按修改时间缓存）。
    pub fn get_employees(&self, path: &Path) -> Result<Arc<EmployeesData>, AppError> {
        let key = path.display().to_string();
        let mtime_ns = modified_ns(path);

        if let Some(entry) = self.employees.get(&key) {
            if entry.mtime_ns == mtime_ns {
                self.hits.fetch_add(1, Ordering::Relaxed);
                return Ok(entry.value.clone());
            }
            // 文件已更新，丢弃旧条目
            self.employees.invalidate(&key);
        }

        self.misses.fetch_add(1, Ordering::Relaxed);

        let data = Arc::new(crate::excel_parser::ExcelParser::load_employees(path)?);
        self.employees.insert(
            key,
            Entry {
                value: data.clone(),
                mtime_ns,
            },
        );

        Ok(data)
    }

    /// 加载工资表（按修改时间缓存）。
    pub fn get_salary(&self, path: &Path) -> Result<(Vec<String>, Vec<SalaryRow>), AppError> {
        let key = path.display().to_string();
        let mtime_ns = modified_ns(path);

        if let Some(entry) = self.salaries.get(&key) {
            if entry.mtime_ns == mtime_ns {
                self.hits.fetch_add(1, Ordering::Relaxed);
                return Ok((entry.value.0.clone(), entry.value.1.clone()));
            }
            self.salaries.invalidate(&key);
        }

        self.misses.fetch_add(1, Ordering::Relaxed);

        let value = load_salary_from_disk(path)?;
        self.salaries.insert(
            key,
            Entry {
                value: value.clone(),
                mtime_ns,
            },
        );

        Ok(value)
    }

    /// 清空所有缓存。
    pub fn invalidate_all(&self) {
        self.employees.invalidate_all();
        self.salaries.invalidate_all();
        self.employees.run_pending_tasks();
        self.salaries.run_pending_tasks();
    }

    /// 缓存统计。
    pub fn stats(&self) -> CacheStats {
        self.employees.run_pending_tasks();
        self.salaries.run_pending_tasks();

        CacheStats {
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            salary_files_cached: self.salaries.entry_count() as usize,
        }
    }
}

impl Default for DataCache {
    fn default() -> Self {
        Self::new()
    }
}

/// 从磁盘解析工资表：首行为表头，其余为数据行。
fn load_salary_from_disk(path: &Path) -> Result<(Vec<String>, Vec<SalaryRow>), AppError> {
    if !path.exists() {
        return Err(AppError::ExcelFileNotFound(path.display().to_string()));
    }

    let mut workbook = calamine::open_workbook_auto(path)
        .map_err(|e| AppError::IoError(std::io::Error::new(std::io::ErrorKind::InvalidData, e)))?;

    let worksheet = workbook
        .worksheet_range_at(0)
        .ok_or_else(|| AppError::InternalError(format!("工作簿中没有工作表: {}", path.display())))?
        .map_err(|e| {
            AppError::IoError(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                e.to_string(),
            ))
        })?;

    let mut headers: Vec<String> = Vec::new();
    let mut rows: Vec<SalaryRow> = Vec::new();

    let mut row_iter = worksheet.rows();

    let Some(first_row) = row_iter.next() else {
        return Ok((headers, rows));
    };

    headers = first_row
        .iter()
        .map(crate::excel_parser::cell_to_string)
        .collect();

    for row in row_iter {
        let mut map = std::collections::HashMap::new();

        for (idx, cell) in row.iter().enumerate() {
            if let Some(header) = headers.get(idx) {
                if !header.trim().is_empty() {
                    map.insert(header.clone(), crate::excel_parser::cell_to_string(cell));
                }
            }
        }

        if !map.is_empty() {
            rows.push(SalaryRow { data: map });
        }
    }

    Ok((headers, rows))
}
