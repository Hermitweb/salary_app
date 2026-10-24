//! 动态字段发现与 `fields.txt` 读写。
//!
//! 字段列表来自所有工资表的首行（与 Python 原版一致：只统计文件名含「工资」的表），
//! 显示策略保存在 `data/fields.txt`，写回时保留用户手写的注释与无法识别的行。

use calamine::{Reader, open_workbook_auto};
use salary_app_backend::excel_parser::{cell_to_string, normalize_field_name};
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
        .map(|row| {
            row.iter()
                .map(|cell| normalize_field_name(&cell_to_string(cell)))
                .collect()
        })
        .unwrap_or_default()
}

/// 收集所有工资表里出现过的字段（去重，保持出现顺序）。
///
/// 「哪些算工资表」用后端的同一套口径（`handlers::queryable_salary_files`），
/// 否则面板列出的字段会和接口实际返回的对不上。
pub fn discover_fields() -> Vec<String> {
    let mut fields = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for path in salary_app_backend::handlers::queryable_salary_files() {
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

    fields
}

/// 工资表集合的指纹：文件路径 + 修改时间。
///
/// 指纹变了说明工资表有增删改，字段列表就要按新的表重新生成——
/// 否则新放进去一个月的工资表，面板上的字段列表还是老的。
pub fn source_signature() -> Vec<(PathBuf, u64)> {
    salary_app_backend::handlers::queryable_salary_files()
        .into_iter()
        .map(|path| {
            let mtime = std::fs::metadata(&path)
                .and_then(|meta| meta.modified())
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_nanos() as u64)
                .unwrap_or(0);
            (path, mtime)
        })
        .collect()
}

/// 读取当前显示策略。
pub fn load_config() -> HashMap<String, FieldState> {
    load_config_from(&paths::field_config_file())
}
/// 从指定文件读取显示策略。
pub fn load_config_from(path: &Path) -> HashMap<String, FieldState> {
    salary_app_backend::excel_parser::ExcelParser::load_field_config(path)
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
pub fn save_config(config: &[(String, FieldState)]) -> std::io::Result<()> {
    save_config_to(&paths::field_config_file(), config)
}

/// 写回指定的配置文件，保留注释、空行与未识别的行。
///
/// `config` 按面板里的字段顺序传入：新字段追加到末尾时顺序稳定，
/// 不会因为 `HashMap` 的遍历顺序而让文件每次保存都换行序。
pub fn save_config_to(path: &Path, config: &[(String, FieldState)]) -> std::io::Result<()> {
    let existing = std::fs::read_to_string(path).unwrap_or_default();

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
        match config.iter().find(|(name, _)| *name == key) {
            Some((_, state)) => {
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

    // 新出现的字段按面板顺序追加到末尾
    for (key, state) in config {
        if !written.contains(key) {
            output.push_str(&format!("{} = {}\n", key, state_to_text(*state)));
        }
    }

    // 原子写：后端每次查询都会读这个文件，
    // 先截断再写的话正好被读到就会拿到半截内容。
    //
    // 临时文件名带上进程号与计数：并发保存时各写各的，
    // 不会把对方的临时文件提前搬走。
    static SAVE_COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let unique = SAVE_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "fields.txt".to_string());
    let temp = path.with_file_name(format!("{file_name}.{}.{unique}.tmp", std::process::id()));

    std::fs::write(&temp, output)?;
    std::fs::rename(&temp, path)
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
    use std::path::PathBuf;

    /// 串行化会写同一个配置文件的用例。
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// 临时配置文件路径。
    ///
    /// 不走全局的数据目录：`cargo test` 会注入 `CARGO_MANIFEST_DIR`，
    /// 全局路径在进程内只解析一次，测试里改不动。
    fn temp_config_file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("salary-tray-fields-test");
        let _ = std::fs::create_dir_all(&dir);
        dir.join(name)
    }

    #[test]
    fn state_cycles() {
        assert_eq!(next_state(FieldState::Show), FieldState::Auto);
        assert_eq!(next_state(FieldState::Auto), FieldState::Hide);
        assert_eq!(next_state(FieldState::Hide), FieldState::Show);
    }

    #[test]
    fn saved_config_round_trips_and_keeps_manual_edits() {
        let _guard = LOCK.lock().unwrap_or_else(|error| error.into_inner());

        let path = temp_config_file("round-trip.txt");
        std::fs::write(&path, "# 手写注释\n\n绩效 = 隐藏\n这是一行无法识别的内容\n").unwrap();

        let config = vec![
            ("绩效".to_string(), FieldState::Show),
            ("津贴".to_string(), FieldState::Hide),
            ("社保".to_string(), FieldState::Auto),
        ];
        save_config_to(&path, &config).unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        // 注释、空行与无法识别的行都要原样留下
        assert!(text.contains("# 手写注释"), "{text}");
        assert!(text.contains("这是一行无法识别的内容"), "{text}");
        // 已有字段就地更新
        assert!(text.contains("绩效 = 显示"), "{text}");
        // 新字段追加到末尾，且顺序跟面板一致
        assert!(text.contains("津贴 = 隐藏"), "{text}");
        assert!(text.contains("社保 = 跟随"), "{text}");
        assert!(
            text.find("津贴").unwrap() < text.find("社保").unwrap(),
            "{text}"
        );

        // 再读回来，状态必须一致
        let loaded = load_config_from(&path);
        assert_eq!(loaded.get("绩效"), Some(&FieldState::Show));
        assert_eq!(loaded.get("津贴"), Some(&FieldState::Hide));
        assert_eq!(loaded.get("社保"), Some(&FieldState::Auto));

        // 临时文件不能留在数据目录里
        let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .filter(|name| name.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "临时文件残留: {leftovers:?}");
    }

    #[test]
    fn saving_twice_keeps_the_same_line_order() {
        let _guard = LOCK.lock().unwrap_or_else(|error| error.into_inner());

        let path = temp_config_file("stable-order.txt");
        let _ = std::fs::remove_file(&path);

        let config = vec![
            ("员工号".to_string(), FieldState::Auto),
            ("姓名".to_string(), FieldState::Hide),
            ("部门".to_string(), FieldState::Show),
        ];

        save_config_to(&path, &config).unwrap();
        let first = std::fs::read_to_string(&path).unwrap();
        save_config_to(&path, &config).unwrap();
        let second = std::fs::read_to_string(&path).unwrap();

        assert_eq!(first, second);
    }
}
