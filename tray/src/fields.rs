//! 动态字段发现与 `fields.txt` 读写。
//!
//! 字段列表来自所有工资表的首行（与 Python 原版一致：只统计文件名含「工资」的表），
//! 显示策略保存在 `data/fields.txt`，写回时保留用户手写的注释与无法识别的行。

use calamine::{Data, Reader, open_workbook_auto};
use salary_app_backend::models::FieldState;
use salary_app_backend::paths;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// 读取 Excel 首行作为表头。
fn read_headers(path: &Path) -> Vec<String> {
    let Ok(mut workbook) = open_workbook_auto(path) else {
        return Vec::new();
    };
    let Some(Ok(sheet)) = workbook.worksheet_range_at(0) else {
        return Vec::new();
    };

    sheet
        .rows()
        .next()
        .map(|row| row.iter().map(cell_to_text).collect())
        .unwrap_or_default()
}

/// 单元格转文本（与后端保持一致）。
fn cell_to_text(value: &Data) -> String {
    match value {
        Data::String(s) => s.clone(),
        Data::Int(i) => i.to_string(),
        Data::Float(n) => {
            let formatted = format!("{n:.2}");
            let trimmed = formatted.trim_end_matches('0');
            trimmed.strip_suffix('.').unwrap_or(trimmed).to_string()
        }
        Data::Bool(b) => if *b { "是" } else { "否" }.to_string(),
        Data::DateTime(dt) => dt.to_string(),
        Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
        Data::Error(_) | Data::Empty => String::new(),
    }
}

/// 是否是工资表文件。
fn is_salary_file(path: &Path) -> bool {
    let Some(extension) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    let extension = extension.to_lowercase();
    if extension != "xlsx" && extension != "xls" {
        return false;
    }

    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.contains("工资"))
        .unwrap_or(false)
}

/// 收集所有工资表里出现过的字段（去重，保持出现顺序）。
pub fn discover_fields() -> Vec<String> {
    let mut fields = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for dir in [paths::salary_dir(), paths::data_dir()] {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };

        let mut files: Vec<PathBuf> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        files.sort();

        for path in files {
            if !is_salary_file(&path) {
                continue;
            }

            for header in read_headers(&path) {
                let header = header.trim().to_string();
                if header.is_empty() {
                    continue;
                }
                if seen.insert(header.clone()) {
                    fields.push(header);
                }
            }
        }
    }

    fields
}

/// 读取当前显示策略。
pub fn load_config() -> HashMap<String, FieldState> {
    salary_app_backend::excel_parser::ExcelParser::load_field_config(&paths::field_config_file())
        .map(|config| config.config)
        .unwrap_or_default()
}

/// 策略对应的文本。
fn state_to_text(state: FieldState) -> &'static str {
    match state {
        FieldState::Show => "显示",
        FieldState::Hide => "隐藏",
        FieldState::Auto => "跟随",
    }
}

/// 写回 `fields.txt`，保留注释、空行与未识别的行。
pub fn save_config(config: &HashMap<String, FieldState>) -> std::io::Result<()> {
    let path = paths::field_config_file();
    let existing = std::fs::read_to_string(&path).unwrap_or_default();

    let mut output = String::new();
    let mut written: HashSet<String> = HashSet::new();

    for line in existing.lines() {
        let trimmed = line.trim();

        // 注释与空行原样保留
        if trimmed.is_empty() || trimmed.starts_with('#') {
            output.push_str(line);
            output.push('\n');
            continue;
        }

        let Some((key, _)) = trimmed.split_once('=') else {
            output.push_str(line);
            output.push('\n');
            continue;
        };

        let key = key.trim().to_string();
        match config.get(&key) {
            Some(state) => {
                output.push_str(&format!("{} = {}\n", key, state_to_text(*state)));
                written.insert(key);
            }
            // 不在配置里的行保持原样
            None => {
                output.push_str(line);
                output.push('\n');
            }
        }
    }

    // 新出现的字段追加到末尾
    for (key, state) in config {
        if !written.contains(key) {
            output.push_str(&format!("{} = {}\n", key, state_to_text(*state)));
        }
    }

    std::fs::write(path, output)
}

/// 下一个状态的循环：显示 -> 跟随 -> 隐藏 -> 显示。
pub fn next_state(state: FieldState) -> FieldState {
    match state {
        FieldState::Show => FieldState::Auto,
        FieldState::Auto => FieldState::Hide,
        FieldState::Hide => FieldState::Show,
    }
}

/// 状态的显示文本。
pub fn state_label(state: FieldState) -> &'static str {
    state_to_text(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_cycles() {
        assert_eq!(next_state(FieldState::Show), FieldState::Auto);
        assert_eq!(next_state(FieldState::Auto), FieldState::Hide);
        assert_eq!(next_state(FieldState::Hide), FieldState::Show);
    }
}
