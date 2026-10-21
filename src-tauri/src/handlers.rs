//! HTTP 处理器。

use crate::audit;
use crate::auth::{AdminUser, AuthContext, AuthUser};
use crate::cache::DataCache;
use crate::error::AppError;
use crate::excel_parser::{self, ExcelParser};
use crate::models::{LoginRequest, LoginResponse, MonthOption};
use crate::paths;
use actix_web::{HttpRequest, HttpResponse, web};
use regex::Regex;
use std::path::PathBuf;
use std::sync::OnceLock;

/// 客户端 IP。
///
/// 本系统是局域网直连部署（没有反向代理），因此只取 TCP 对端地址：
/// `X-Forwarded-For` / `X-Real-IP` 可以被客户端任意伪造，
/// 拿它们做审计依据会让日志失去意义（与 Python 原版一致）。
fn client_ip(req: &HttpRequest) -> String {
    req.peer_addr()
        .map(|addr| addr.ip().to_string())
        .unwrap_or_else(|| "-".to_string())
}

/// 年月匹配规则：优先匹配「年/月」写法，其次是分隔符写法，最后是紧凑写法。
fn year_month_regexes() -> &'static [Regex; 3] {
    static REGEXES: OnceLock<[Regex; 3]> = OnceLock::new();
    REGEXES.get_or_init(|| {
        [
            Regex::new(r"(\d{4})\s*年\s*(\d{1,2})\s*月").expect("年/月 正则"),
            Regex::new(r"(\d{4})[-_/\.](\d{1,2})").expect("分隔符正则"),
            Regex::new(r"(\d{4})(\d{2})").expect("紧凑正则"),
        ]
    })
}

/// 从文件名中解析年份与月份。
///
/// 支持 `2026 年 05 月员工工资.xlsx`、`2026-05 工资.xlsx`、`202605 工资.xlsx`。
pub fn extract_year_month(filename: &str) -> Option<(i32, u32)> {
    let regexes = year_month_regexes();

    for (index, regex) in regexes.iter().enumerate() {
        let Some(captures) = regex.captures(filename) else {
            continue;
        };

        let year: i32 = captures.get(1)?.as_str().parse().ok()?;
        let month: u32 = captures.get(2)?.as_str().parse().ok()?;

        if !(2000..=2100).contains(&year) || !(1..=12).contains(&month) {
            continue;
        }

        // 紧凑写法容易误命中普通数字，需要额外确认没有更长的数字串
        if index == 2 {
            let digits: String = filename.chars().filter(|c| c.is_ascii_digit()).collect();
            if digits.len() != 6 {
                continue;
            }
        }

        return Some((year, month));
    }

    None
}

/// 工资表目录下的所有 Excel 文件。
fn salary_files() -> Vec<PathBuf> {
    let dir = paths::salary_dir();

    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };

    let mut files: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| {
                    let lower = name.to_lowercase();
                    lower.ends_with(".xlsx") || lower.ends_with(".xls")
                })
                .unwrap_or(false)
        })
        .collect();

    files.sort();
    files
}

/// 列出所有可查询的月份（倒序）。
fn list_months() -> Vec<MonthOption> {
    let mut months: Vec<MonthOption> = salary_files()
        .iter()
        .filter_map(|path| {
            let name = path.file_name()?.to_str()?;
            extract_year_month(name).map(|(year, month)| MonthOption { year, month })
        })
        .collect();

    // 倒序排列：最新月份在最前
    months.sort_by_key(|option| std::cmp::Reverse((option.year, option.month)));
    months.dedup_by(|a, b| a.year == b.year && a.month == b.month);
    months
}

/// 查找指定年月的工资表。
fn find_salary_file(year: i32, month: u32) -> Option<PathBuf> {
    salary_files().into_iter().find(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .and_then(extract_year_month)
            == Some((year, month))
    })
}

/// 在表头中定位工号列。
///
/// 统一走 `excel_parser` 的关键字表，避免工资表与员工表对工号列的判断分叉。
fn find_employee_id_column(headers: &[String]) -> Option<usize> {
    excel_parser::find_employee_id_column(headers)
}

/// 在工资表里找出某名员工的记录。
///
/// 工号在员工表与工资表里可能一个是数字、一个是文本（例如「1」与「001」），
/// 所以这里必须与登录校验共用 [`excel_parser::ids_match`]：
/// 口径一旦分叉，员工就会遇到「能登录却查不到工资记录」。
fn find_employee_row<'a>(
    rows: &'a [crate::models::SalaryRow],
    id_header: &str,
    employee_id: &str,
) -> Option<&'a crate::models::SalaryRow> {
    rows.iter().find(|row| {
        row.data
            .get(id_header)
            .map(|value| excel_parser::ids_match(value, employee_id))
            .unwrap_or(false)
    })
}

/// `GET /healthz` —— 健康检查。
pub async fn healthcheck() -> HttpResponse {
    HttpResponse::Ok().body("OK")
}

/// `POST /api/auth/login` —— 登录。
pub async fn login(
    req: HttpRequest,
    body: web::Json<LoginRequest>,
    auth: web::Data<AuthContext>,
    cache: web::Data<DataCache>,
) -> Result<HttpResponse, AppError> {
    let ip = client_ip(&req);
    let username = body.username.trim().to_string();
    let ip_key = format!("ip:{ip}");
    let user_key = format!("user:{username}");

    // 锁定检查：IP 与账号两个维度
    for key in [&ip_key, &user_key] {
        if let Some(wait) = auth.rate_limiter.remaining_lock(key) {
            audit::record(&ip, "LOGIN_BLOCKED", &username, "", "触发登录限流");
            return Err(AppError::LoginLocked(wait));
        }
    }

    let employee_file = paths::employee_file();
    if !employee_file.exists() {
        return Err(AppError::EmployeeFileNotFound(
            employee_file.display().to_string(),
        ));
    }

    let employees = cache.get_employees(&employee_file)?;

    match ExcelParser::authenticate(&employees, &username, body.password.trim()) {
        Ok(employee) => {
            auth.rate_limiter.clear_failures(&ip_key);
            auth.rate_limiter.clear_failures(&user_key);

            let token = auth
                .issue_token(&employee)
                .map_err(AppError::InternalError)?;
            let name = employee.name.clone().unwrap_or_default();

            audit::record(&ip, "LOGIN", &employee.employee_id, &name, "登录成功");

            Ok(HttpResponse::Ok().json(LoginResponse {
                ok: true,
                token: Some(token),
                employee: Some(employee),
                error: None,
            }))
        }
        Err(error) => {
            let mut locked = false;
            for key in [&ip_key, &user_key] {
                if auth.rate_limiter.record_failure(key) {
                    locked = true;
                }
            }

            audit::record(&ip, "LOGIN_FAIL", &username, "", "登录失败");

            if locked {
                return Err(AppError::LoginLocked(auth.rate_limiter.lock_seconds as u64));
            }

            Err(error)
        }
    }
}

/// `POST /api/logout` —— 退出登录。
///
/// 令牌是无状态的，服务端只记录审计事件，由客户端清除本地令牌。
pub async fn logout(req: HttpRequest, user: AuthUser) -> HttpResponse {
    audit::record(
        &client_ip(&req),
        "LOGOUT",
        &user.0.employee_id,
        user.0.name.as_deref().unwrap_or(""),
        "退出登录",
    );

    HttpResponse::Ok().json(serde_json::json!({ "ok": true }))
}

/// `GET /api/available_months` —— 可查询月份列表。
pub async fn available_months(_user: AuthUser) -> HttpResponse {
    HttpResponse::Ok().json(serde_json::json!({
        "ok": true,
        "months": list_months(),
    }))
}

/// `GET /api/salary/{year}/{month}` —— 查询工资。
pub async fn salary_query(
    req: HttpRequest,
    user: AuthUser,
    cache: web::Data<DataCache>,
    path: web::Path<(i32, u32)>,
) -> Result<HttpResponse, AppError> {
    let (year, month) = path.into_inner();
    salary_response(req, user, cache, year, month).await
}

/// 查询参数形式的工资查询（与 Python 原版 `/api/salary?year=&month=` 兼容）。
#[derive(serde::Deserialize)]
pub struct SalaryQuery {
    pub year: Option<i32>,
    pub month: Option<u32>,
}

/// `GET /api/salary?year=&month=` —— 查询工资。
pub async fn salary_query_params(
    req: HttpRequest,
    user: AuthUser,
    cache: web::Data<DataCache>,
    query: web::Query<SalaryQuery>,
) -> Result<HttpResponse, AppError> {
    let year = query
        .year
        .ok_or_else(|| AppError::BadRequest("缺少年月参数".to_string()))?;
    let month = query
        .month
        .ok_or_else(|| AppError::BadRequest("缺少年月参数".to_string()))?;

    salary_response(req, user, cache, year, month).await
}

/// 查询指定年月的工资条。
async fn salary_response(
    req: HttpRequest,
    user: AuthUser,
    cache: web::Data<DataCache>,
    year: i32,
    month: u32,
) -> Result<HttpResponse, AppError> {
    if !(1..=12).contains(&month) {
        return Err(AppError::BadRequest(format!("月份 {month} 超出范围")));
    }

    let file = find_salary_file(year, month).ok_or(AppError::SalaryFileNotFound { year, month })?;
    let (headers, rows) = cache.get_salary(&file)?;

    let id_index = find_employee_id_column(&headers)
        .ok_or_else(|| AppError::InternalError("工资表中找不到工号列".to_string()))?;
    let id_header = headers[id_index].clone();

    let row = find_employee_row(&rows, &id_header, &user.0.employee_id).ok_or_else(|| {
        AppError::EmployeeNotFound(format!(
            "{} 在 {year} 年 {month} 月没有工资记录",
            user.0.employee_id
        ))
    })?;

    // 按 `data/fields.txt` 的字段显示配置过滤列：
    // 默认隐藏空值与 0，可通过配置强制显示或隐藏某个字段。
    let field_config =
        ExcelParser::load_field_config(&paths::field_config_file()).unwrap_or_default();

    let visible: Vec<usize> = headers
        .iter()
        .enumerate()
        .filter(|(_, header)| {
            let value = row.data.get(*header).map(|v| v.as_str()).unwrap_or("");
            field_config.should_show(header, value)
        })
        .map(|(idx, _)| idx)
        .collect();

    let visible_headers: Vec<String> = visible.iter().map(|idx| headers[*idx].clone()).collect();
    let visible_values: Vec<String> = visible
        .iter()
        .map(|idx| row.data.get(&headers[*idx]).cloned().unwrap_or_default())
        .collect();

    let visible_data: std::collections::BTreeMap<String, String> = visible
        .iter()
        .map(|idx| {
            let header = headers[*idx].clone();
            let value = row.data.get(&header).cloned().unwrap_or_default();
            (header, value)
        })
        .collect();

    // 员工号始终保留在数据里（即便为空），与 Python 原版一致
    let mut data_map = visible_data;
    if !data_map.contains_key(&id_header) {
        if let Some(value) = row.data.get(&id_header) {
            data_map.insert(id_header.clone(), value.clone());
        }
    }

    audit::record(
        &client_ip(&req),
        "VIEW",
        &user.0.employee_id,
        user.0.name.as_deref().unwrap_or(""),
        &format!("{year}-{month:02}"),
    );

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "ok": true,
        "year": year,
        "month": month,
        "headers": visible_headers,
        "values": visible_values,
        "data": data_map,
    })))
}

/// `GET /api/cache/stats` —— 缓存统计（管理员）。
pub async fn cache_stats(
    req: HttpRequest,
    admin: AdminUser,
    cache: web::Data<DataCache>,
) -> HttpResponse {
    audit::record(
        &client_ip(&req),
        "CACHE_STATS",
        &admin.0.employee_id,
        admin.0.name.as_deref().unwrap_or(""),
        "查看缓存统计",
    );

    HttpResponse::Ok().json(serde_json::json!({
        "ok": true,
        "stats": cache.stats(),
    }))
}

/// `POST /api/cache/invalidate` —— 清空缓存（管理员）。
pub async fn cache_invalidate(
    req: HttpRequest,
    admin: AdminUser,
    cache: web::Data<DataCache>,
) -> HttpResponse {
    cache.invalidate_all();

    audit::record(
        &client_ip(&req),
        "CACHE_INVALIDATE",
        &admin.0.employee_id,
        admin.0.name.as_deref().unwrap_or(""),
        "清空缓存",
    );

    HttpResponse::Ok().json(serde_json::json!({
        "ok": true,
        "message": "缓存已清空",
    }))
}

/// `POST /api/audit` —— 前端上报操作事件。
#[derive(serde::Deserialize)]
pub struct AuditRequest {
    pub action: String,
    #[serde(default)]
    pub detail: String,
}

/// `GET /api/audit/log` —— 查看审计日志的查询参数。
#[derive(serde::Deserialize)]
pub struct AuditLogQuery {
    /// 返回最近多少条，默认 100，上限 1000。
    pub limit: Option<usize>,
}

/// `GET /api/audit/log` —— 查看审计日志（管理员）。
///
/// 审计日志只写进 `data/audit.log`，直接翻文件不方便；
/// 这个接口把最近若干条解析出来，管理员可以在网页上直接核对谁查看了工资。
pub async fn audit_log(
    req: HttpRequest,
    admin: AdminUser,
    query: web::Query<AuditLogQuery>,
) -> HttpResponse {
    let limit = query.limit.unwrap_or(100).clamp(1, 1000);
    let events = audit::read_recent(&paths::audit_log_file(), limit);

    audit::record(
        &client_ip(&req),
        "AUDIT_VIEW",
        &admin.0.employee_id,
        admin.0.name.as_deref().unwrap_or(""),
        &format!("查看最近 {limit} 条审计记录"),
    );

    HttpResponse::Ok().json(serde_json::json!({
        "ok": true,
        "events": events,
    }))
}

/// `POST /api/audit` —— 记录审计事件。
pub async fn audit_record(
    req: HttpRequest,
    user: AuthUser,
    body: web::Json<AuditRequest>,
) -> HttpResponse {
    audit::record(
        &client_ip(&req),
        &body.action,
        &user.0.employee_id,
        user.0.name.as_deref().unwrap_or(""),
        &body.detail,
    );

    HttpResponse::Ok().json(serde_json::json!({ "ok": true }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_chinese_year_month() {
        assert_eq!(
            extract_year_month("2026 年 05 月员工工资.xlsx"),
            Some((2026, 5))
        );
        assert_eq!(extract_year_month("2026年5月工资表.xlsx"), Some((2026, 5)));
    }

    #[test]
    fn extracts_separated_year_month() {
        assert_eq!(extract_year_month("2026-05 工资.xlsx"), Some((2026, 5)));
        assert_eq!(extract_year_month("2026_10 工资.xlsx"), Some((2026, 10)));
        assert_eq!(extract_year_month("2026/3 工资.xlsx"), Some((2026, 3)));
    }

    #[test]
    fn extracts_compact_year_month() {
        assert_eq!(extract_year_month("202605 工资.xlsx"), Some((2026, 5)));
    }

    #[test]
    fn rejects_unrelated_filenames() {
        assert_eq!(extract_year_month("random.txt"), None);
        assert_eq!(extract_year_month("员工工资.xlsx"), None);
        // 月份越界
        assert_eq!(extract_year_month("2026-13 工资.xlsx"), None);
    }

    /// 构造一张只有工号列的工资表。
    fn salary_rows(ids: &[&str]) -> Vec<crate::models::SalaryRow> {
        ids.iter()
            .map(|id| crate::models::SalaryRow {
                data: std::collections::HashMap::from([("员工号".to_string(), id.to_string())]),
            })
            .collect()
    }

    #[test]
    fn employee_row_matches_padded_and_numeric_ids() {
        // 工资表里存的是文本「001」，令牌里是数字「1」——必须能查到
        let rows = salary_rows(&["001", "002"]);
        let found = find_employee_row(&rows, "员工号", "1");
        assert!(found.is_some(), "001 与 1 应当视为同一名员工");

        // 反过来：工资表存数字，登录用的是带前导零的文本
        let rows = salary_rows(&["1", "2"]);
        let found = find_employee_row(&rows, "员工号", "001");
        assert!(found.is_some(), "1 与 001 应当视为同一名员工");
    }

    #[test]
    fn employee_row_does_not_match_other_employees() {
        let rows = salary_rows(&["001", "002"]);

        assert!(find_employee_row(&rows, "员工号", "003").is_none());
        assert!(find_employee_row(&rows, "员工号", "abc").is_none());
    }
}
