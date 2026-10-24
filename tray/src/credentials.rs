//! 网页登录的字段映射：账号与密码分别取自哪一列。
//!
//! 控制面板不再保管某个人的具体口令，它只负责一个设置：
//! 从工资表的表头里选出「账号列」和「密码列」，写进
//! `data/login_fields.json`。之后网页登录时，服务端就按这份映射
//! 到员工表里找列校验——员工填的账号是账号列的内容，
//! 口令是密码列的内容（明文与 PBKDF2 哈希都认）。

use salary_app_backend::excel_parser::{self, LoginFieldMapping};
use salary_app_backend::paths;

/// 登录字段映射（转发给后端，读写同一个文件）。
pub type LoginFields = LoginFieldMapping;

/// 读取当前映射；没配置过时返回空映射。
pub fn load() -> LoginFields {
    excel_parser::read_login_mapping()
}

/// 保存映射；保存后网页登录立即按新映射校验。
pub fn save(fields: &LoginFields) -> Result<(), String> {
    excel_parser::write_login_mapping(fields)
}

/// 登录字段的候选列表：员工列表的表头。
///
/// 登录校验对着员工表做，所以候选也应该从员工表来；
/// 员工列表缺失或解析不出来时，退回工资表的表头
/// （两张表的「工号 / 紧急联系人号码」这类列名是同一套）。
pub fn discover_field_names() -> Vec<String> {
    let employee_file = paths::employee_file();

    if employee_file.exists() {
        if let Ok(grid) = excel_parser::read_grid(&employee_file) {
            let names: Vec<String> = grid
                .first()
                .map(|row| {
                    row.iter()
                        .map(|cell| excel_parser::normalize_field_name(cell))
                        .filter(|name| !name.is_empty())
                        .collect()
                })
                .unwrap_or_default();

            if !names.is_empty() {
                return names;
            }
        }
    }

    crate::fields::discover_fields()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapping_defaults_to_empty() {
        // 没配置过时应当是空映射：登录沿用关键字探测
        let fields = load();
        if login_fields_exist() {
            return; // 真实环境里已经配置过，跳过
        }
        assert!(fields.is_empty(), "未配置时映射应当为空");
    }

    #[test]
    fn mapping_round_trips_through_backend() {
        // 直接走后端的读写：写进去再读出来应当一致。
        // 用带进程号的临时内容避免与真实配置互相覆盖。
        let original = load();

        let fields = LoginFields {
            account_field: format!("工号测试-{}", std::process::id()),
            password_field: format!("密码测试-{}", std::process::id()),
        };

        save(&fields).expect("应当能保存映射");
        let loaded = load();
        assert_eq!(loaded.account_field, fields.account_field);
        assert_eq!(loaded.password_field, fields.password_field);
        assert!(!loaded.is_empty());

        // 还原现场
        save(&original).expect("应当能还原映射");
    }

    #[test]
    fn field_candidates_are_not_empty() {
        let names = discover_field_names();
        if names.is_empty() {
            eprintln!("员工表与工资表都缺失，跳过");
            return;
        }

        // 候选列表应当包含工号列（两张表都有这一列）
        assert!(
            names.iter().any(|name| name.contains("工号")),
            "候选字段里应当有工号：{names:?}"
        );
    }

    /// 登录字段配置文件是否已经存在。
    fn login_fields_exist() -> bool {
        paths::login_fields_file().exists()
    }
}
