//! 审计日志：记录登录、查询等关键动作。
//!
//! 事件同时写入 `data/audit.log`（JSON Lines）与 tracing 日志。
//!
//! 追加日志时用一把进程内的锁，并把「内容 + 换行」拼成一次写入：
//! 分多次写会让并发的写入互相穿插，导致同一行里出现两条记录。

use crate::models::AuditEvent;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// 审计日志体积上限（字节）。
///
/// 审计日志只增不减，长期运行会慢慢涨到几十上百 MB。
/// 超过上限就把当前日志改名成 `.1` 备份、从头开始写新日志。
const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;

/// 追加审计日志用的进程内锁。
fn append_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// 轮转后的备份文件路径：`data/audit.log` -> `data/audit.log.1`。
fn backup_path(path: &Path) -> PathBuf {
    let mut backup = path.as_os_str().to_os_string();
    backup.push(".1");
    PathBuf::from(backup)
}

/// 日志超过上限时轮转。
///
/// 只保留最近一份备份：旧的 `.1` 直接覆盖。
/// 改名失败时退回到截断当前文件，至少保证日志不会无限膨胀。
fn rotate_if_needed(path: &Path, max_bytes: u64) {
    let Ok(metadata) = std::fs::metadata(path) else {
        // 文件还不存在，无需轮转
        return;
    };

    if metadata.len() < max_bytes {
        return;
    }

    let backup = backup_path(path);
    let _ = std::fs::remove_file(&backup);

    if std::fs::rename(path, &backup).is_err() {
        // 改名失败（比如目标被占用）就截断当前文件
        if let Ok(file) = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(path)
        {
            let _ = file.sync_all();
        }
    }
}

/// 记录一条审计事件。
pub fn record(ip: &str, action: &str, employee_id: &str, employee_name: &str, detail: &str) {
    let event = AuditEvent {
        timestamp: chrono::Utc::now().to_rfc3339(),
        ip: ip.to_string(),
        action: action.to_string(),
        employee_id: employee_id.to_string(),
        employee_name: employee_name.to_string(),
        detail: detail.to_string(),
    };

    tracing::info!(
        action = %event.action,
        employee = %event.employee_id,
        ip = %event.ip,
        detail = %event.detail,
        "审计事件"
    );

    let Ok(line) = serde_json::to_string(&event) else {
        return;
    };

    // 一次写入整行，避免并发时两条记录挤到同一行
    let mut record = String::with_capacity(line.len() + 1);
    record.push_str(&line);
    record.push('\n');

    let _guard = append_lock().lock();

    let path = crate::paths::audit_log_file();
    rotate_if_needed(&path, MAX_LOG_BYTES);

    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = file.write_all(record.as_bytes());
    }
}

/// 读取审计日志的最后若干条记录。
///
/// 按写入顺序返回，最新的在最后。解析不了的行会被跳过——
/// 日志文件可能被外部工具改过，不能因为一行坏掉就整个读不出来。
pub fn read_recent(path: &Path, limit: usize) -> Vec<AuditEvent> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };

    let mut events: Vec<AuditEvent> = content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<AuditEvent>(line).ok())
        .collect();

    if events.len() > limit {
        events.drain(..events.len() - limit);
    }

    events
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join("salary-app-audit-tests");
        std::fs::create_dir_all(&dir).expect("应当能创建临时目录");
        dir
    }

    #[test]
    fn rotates_log_when_it_grows_past_the_limit() {
        let path = temp_dir().join("rotate.log");
        let _ = std::fs::remove_file(&path);
        std::fs::write(&path, vec![b'x'; 100]).expect("应当能写入");

        // 未超限时不动它
        rotate_if_needed(&path, 1000);
        assert!(path.exists(), "未超限时不应轮转");

        // 超限后轮转
        rotate_if_needed(&path, 50);
        assert!(!path.exists(), "超限后当前日志应当被改名");
        assert!(backup_path(&path).exists(), "应当留下 .1 备份");
        assert_eq!(
            std::fs::metadata(backup_path(&path)).unwrap().len(),
            100,
            "备份内容应当完整"
        );

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(backup_path(&path));
    }

    #[test]
    fn rotation_keeps_only_the_latest_backup() {
        let path = temp_dir().join("rotate2.log");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(backup_path(&path));

        std::fs::write(&path, vec![b'a'; 100]).unwrap();
        rotate_if_needed(&path, 50);
        assert_eq!(std::fs::metadata(backup_path(&path)).unwrap().len(), 100);

        // 第二轮轮转：旧备份被覆盖，不会出现 .2
        std::fs::write(&path, vec![b'b'; 200]).unwrap();
        rotate_if_needed(&path, 50);
        assert_eq!(std::fs::metadata(backup_path(&path)).unwrap().len(), 200);

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(backup_path(&path));
    }

    #[test]
    fn rotation_is_skipped_when_the_file_does_not_exist() {
        let path = temp_dir().join("missing.log");
        let _ = std::fs::remove_file(&path);
        // 不应 panic
        rotate_if_needed(&path, 1);
        assert!(!path.exists());
    }

    #[test]
    fn reads_back_the_latest_entries() {
        let path = temp_dir().join("read.log");

        let mut content = String::new();
        for index in 0..5 {
            let event = AuditEvent {
                timestamp: format!("2026-10-21T00:00:0{index}Z"),
                ip: "127.0.0.1".to_string(),
                action: "VIEW".to_string(),
                employee_id: format!("{index:03}"),
                employee_name: "张三".to_string(),
                detail: String::new(),
            };
            content.push_str(&serde_json::to_string(&event).unwrap());
            content.push('\n');
        }
        // 混一行坏数据，读取时应当跳过而不是整体失败
        content.push_str("not json\n");

        std::fs::write(&path, content).unwrap();

        let events = read_recent(&path, 3);
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].employee_id, "002");
        assert_eq!(events[2].employee_id, "004");

        let _ = std::fs::remove_file(&path);
    }
}
