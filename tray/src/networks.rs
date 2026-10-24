//! 「支持访问的网络」的配置。
//!
//! 原版把这几条网络写死在代码里（办公区域网络 / JTOFFICEWIFI / 宿舍网络），
//! 换个办公地点就得改代码重编。这里改成读 `data/networks.txt`：
//!
//! ```text
//! # 每行：类型 | 名称或说明 | yes/no（是否支持访问）
//! 有线 | 办公区域网络 | yes
//! WiFi | JTOFFICEWIFI、JTOFFICEWIFI_2.4G | yes
//! 宿舍网络 | 暂不支持访问 | no
//! ```
//!
//! 以 `#` 开头的行是注释，空行忽略。

use salary_app_backend::paths;
use std::path::Path;

/// 一条网络记录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkRow {
    /// 类型（有线 / WiFi …）
    pub kind: String,
    /// 名称或说明
    pub description: String,
    /// 是否支持访问
    pub supported: bool,
}

/// 读取支持访问的网络配置。
pub fn load() -> Vec<NetworkRow> {
    load_from(&paths::network_config_file())
}

/// 从指定文件读取支持访问的网络配置。
pub fn load_from(path: &Path) -> Vec<NetworkRow> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };

    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return None;
            }

            // 类型 | 说明 | 是否支持
            let mut parts = trimmed.split('|').map(str::trim);
            let kind = parts.next()?.to_string();
            let description = parts.next().unwrap_or_default().to_string();
            let supported = parts
                .next()
                .map(|value| {
                    let value = value.to_lowercase();
                    matches!(value.as_str(), "yes" | "true" | "1" | "y" | "是" | "支持")
                })
                .unwrap_or(false);

            if kind.is_empty() {
                return None;
            }

            Some(NetworkRow {
                kind,
                description,
                supported,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_network_rows() {
        let path = std::env::temp_dir().join("salary-tray-networks-test.txt");
        std::fs::write(
            &path,
            "# 注释\n\n有线 | 办公区域网络 | yes\nWiFi | JTOFFICEWIFI | no\n",
        )
        .unwrap();

        let rows = load_from(&path);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].kind, "有线");
        assert_eq!(rows[0].description, "办公区域网络");
        assert!(rows[0].supported);
        assert_eq!(rows[1].kind, "WiFi");
        assert!(!rows[1].supported);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn missing_file_yields_no_rows() {
        let path = std::env::temp_dir().join("salary-tray-networks-missing.txt");
        let _ = std::fs::remove_file(&path);
        assert!(load_from(&path).is_empty());
    }
}
