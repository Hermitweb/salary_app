//! 端口解析与校验。
//!
//! 端口有三个来源：环境变量 `SALARY_PORT`、`data/port.txt`、默认值。
//! 后端进程与托盘面板都要用同一套规则——两处各写一份时，
//! 端口上限这种细节很容易漏掉一处（曾经就漏过：`SALARY_PORT=65535`
//! 会让 HTTPS 端口和 HTTP 端口撞车，服务整个起不来）。

use crate::paths;
use std::io;

/// 默认端口。
pub const DEFAULT_PORT: u16 = 5001;

/// 端口号上限。
///
/// 需要给 HTTPS 预留 +1，所以最大只能用 65534。
pub const MAX_PORT: u16 = 65534;

/// 端口号是否可用。
pub fn is_valid(port: u16) -> bool {
    (1..=MAX_PORT).contains(&port)
}

/// 解析端口号字符串。
///
/// 越界与非法格式一律返回 `None`，由调用方决定回退还是报错。
pub fn parse(text: &str) -> Option<u16> {
    let port = text.trim().parse::<u16>().ok()?;
    is_valid(port).then_some(port)
}

/// 读取 `data/port.txt` 里的端口。
///
/// 文件缺失、内容非法或端口越界时都返回 `None`。
pub fn read_from_file() -> Option<u16> {
    let text = std::fs::read_to_string(paths::port_file()).ok()?;
    parse(&text)
}

/// 把端口写进 `data/port.txt`。
pub fn write_to_file(port: u16) -> io::Result<()> {
    if !is_valid(port) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("端口号必须在 1-{MAX_PORT} 之间（需要预留 HTTPS 端口 +1）"),
        ));
    }

    let path = paths::port_file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    std::fs::write(&path, port.to_string())
}

/// 解析本次要监听的端口。
///
/// 优先级：`SALARY_PORT` 环境变量 > `data/port.txt` > 默认端口。
/// 三处的值都会经过范围校验。
pub fn resolve() -> u16 {
    if let Some(port) = std::env::var("SALARY_PORT")
        .ok()
        .and_then(|value| parse(&value))
    {
        return port;
    }

    read_from_file().unwrap_or(DEFAULT_PORT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_out_of_range_ports() {
        // 0 会让系统随机分配端口，65535 会和 HTTPS 端口撞车
        assert!(!is_valid(0));
        assert!(!is_valid(65535));
        assert!(is_valid(1));
        assert!(is_valid(5001));
        assert!(is_valid(MAX_PORT));
    }

    #[test]
    fn parses_valid_port_text() {
        assert_eq!(parse("5001"), Some(5001));
        assert_eq!(parse("  8080  "), Some(8080));
        assert_eq!(parse("0"), None);
        assert_eq!(parse("65535"), None);
        assert_eq!(parse("abc"), None);
        assert_eq!(parse(""), None);
        assert_eq!(parse("-1"), None);
    }
}
