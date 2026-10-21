//! 领域模型：与 Excel 表结构对应的数据结构。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 员工信息（由员工列表表解析得到）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Employee {
    pub employee_id: String,
    pub name: Option<String>,
    pub dept: Option<String>,
    pub is_admin: bool,
}

/// 登录请求体。
#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

/// 登录响应体。
#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub employee: Option<Employee>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 会话令牌载荷。
///
/// 令牌格式：`base64url(payload).hex(HMAC-SHA256(payload))`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionToken {
    pub employee_id: String,
    pub name: Option<String>,
    pub dept: Option<String>,
    pub is_admin: bool,
    /// 过期时间（Unix 秒）
    pub exp: i64,
}

/// 可查询的工资月份。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonthOption {
    pub year: i32,
    pub month: u32,
}

/// 缓存统计。
#[derive(Debug, Clone, Serialize)]
pub struct CacheStats {
    pub hits: usize,
    pub misses: usize,
    pub salary_files_cached: usize,
}

/// 审计事件。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    /// RFC3339 时间戳
    pub timestamp: String,
    pub ip: String,
    pub action: String,
    pub employee_id: String,
    pub employee_name: String,
    pub detail: String,
}

/// 工资表原始行记录（列名 -> 单元格文本）。
#[derive(Debug, Clone, Default)]
pub struct SalaryRow {
    pub data: HashMap<String, String>,
}

/// 字段显示策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FieldState {
    /// 值为空或 0 时隐藏
    #[default]
    Auto,
    /// 强制显示
    Show,
    /// 强制隐藏
    Hide,
}

/// 判断字段值是否为空或为零。
///
/// 与 Python 版的 `_is_empty_or_zero` 判定一致：
/// 空串、纯空格算空；数值 0（含 `0`、`0.0`、`0.00`）以及带千分位或货币符号的
/// 零（`¥0`、`￥0.00`）都算零。无法解析成数字的文本按「非零」处理。
pub fn is_empty_or_zero(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return true;
    }

    let cleaned: String = trimmed
        .chars()
        .filter(|c| *c != ',' && *c != '¥' && *c != '￥')
        .collect();

    match cleaned.trim().parse::<f64>() {
        Ok(number) => number == 0.0,
        Err(_) => false,
    }
}

/// 字段显示配置（来自 `data/fields.txt`）。
#[derive(Debug, Clone, Default)]
pub struct FieldConfig {
    pub config: HashMap<String, FieldState>,
}

impl FieldConfig {
    /// 判断某个字段是否应当展示。
    pub fn should_show(&self, field_name: &str, value: &str) -> bool {
        match self.config.get(field_name).unwrap_or(&FieldState::Auto) {
            FieldState::Show => true,
            FieldState::Hide => false,
            FieldState::Auto => !is_empty_or_zero(value),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_config_auto_hides_zero_and_empty() {
        let config = FieldConfig::default();
        assert!(!config.should_show("绩效", ""));
        assert!(!config.should_show("绩效", "0"));
        assert!(!config.should_show("绩效", "0.00"));
        assert!(config.should_show("绩效", "1200"));
    }

    #[test]
    fn field_config_explicit_show_overrides_auto() {
        let mut config = FieldConfig::default();
        config.config.insert("绩效".to_string(), FieldState::Show);
        assert!(config.should_show("绩效", "0"));
    }

    #[test]
    fn empty_or_zero_matches_python_semantics() {
        assert!(is_empty_or_zero(""));
        assert!(is_empty_or_zero("   "));
        assert!(is_empty_or_zero("0"));
        assert!(is_empty_or_zero("0.0"));
        assert!(is_empty_or_zero("0.00"));
        assert!(is_empty_or_zero("¥0"));
        assert!(is_empty_or_zero("￥0.00"));
        assert!(!is_empty_or_zero("1,000"));
        assert!(!is_empty_or_zero("0.01"));
        assert!(!is_empty_or_zero("待定"));
    }
}
