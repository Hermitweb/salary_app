//! 集成测试：通过真实的 HTTP 接口验证登录、鉴权与工资查询。
//!
//! 依赖 `data/` 下的示例数据（`python scripts/generate_sample_data.py` 生成）。
//! 数据缺失时相关用例会跳过而不是失败。
//!
//! 登录凭据直接从员工表里读取，测试代码中不存放任何口令。

use actix_web::{App, http::StatusCode, test, web};
use salary_app_backend::auth::AuthContext;
use salary_app_backend::cache::DataCache;
use salary_app_backend::configure_routes;
use salary_app_backend::excel_parser::{EmployeesData, ExcelParser};
use salary_app_backend::paths;
use serde_json::{Value, json};
use std::collections::HashMap;

/// 初始化被测服务。
macro_rules! init_app {
    () => {
        test::init_service(
            App::new()
                .app_data(web::Data::new(DataCache::new()))
                .app_data(web::Data::new(AuthContext::new(
                    b"integration-test-secret-key".to_vec(),
                )))
                .configure(configure_routes),
        )
        .await
    };
}

/// 让测试使用临时数据目录。
///
/// 真实数据会被复制到临时目录，测试产生的审计日志等副产物不会写进
/// 仓库的 `data/` 目录。每次进程只解析一次路径，因此这里设置环境变量
/// 之后不会再有并发读取。
fn setup() {
    static INIT: std::sync::Once = std::sync::Once::new();

    INIT.call_once(|| {
        let source = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let temp = std::env::temp_dir().join("salary-app-integration-test");

        let _ = std::fs::remove_dir_all(&temp);
        let _ = std::fs::create_dir_all(&temp);

        // 复制员工表与工资表
        let _ = std::fs::copy(source.join("员工列表.xlsx"), temp.join("员工列表.xlsx"));
        if let Ok(entries) = std::fs::read_dir(source.join("salary")) {
            let salary = temp.join("salary");
            let _ = std::fs::create_dir_all(&salary);
            for entry in entries.flatten() {
                let _ = std::fs::copy(entry.path(), salary.join(entry.file_name()));
            }
        }

        // SAFETY：本进程内只有这一个线程会写该环境变量；
        // `paths::data_dir()` 解析一次后即缓存，之后不再读取环境变量。
        unsafe { std::env::set_var("SALARY_DATA_DIR", &temp) };
    });
}

/// 示例数据是否就绪。
fn sample_data_ready() -> bool {
    setup();
    paths::employee_file().exists() && paths::salary_dir().is_dir()
}

/// 读取员工表中某一行对应的工号与口令。
fn row_credentials(
    employees: &EmployeesData,
    row: &HashMap<String, String>,
) -> Option<(String, String)> {
    let id_header = employees.headers.get(employees.employee_id_idx?)?;
    let password_header = employees.headers.get(employees.password_idx?)?;

    let employee_id = row.get(id_header)?.trim().to_string();
    let password = row.get(password_header)?.clone();

    Some((employee_id, password))
}

/// 从员工表里找一个普通员工的登录凭据。
fn staff_credentials() -> Option<(String, String)> {
    let employees = ExcelParser::load_employees(&paths::employee_file()).ok()?;

    for row in &employees.rows {
        let is_admin = row.values().any(|value| value.trim() == "管理员");
        let is_resigned = row.values().any(|value| value.trim() == "离职");
        if is_admin || is_resigned {
            continue;
        }
        if let Some(credentials) = row_credentials(&employees, row) {
            return Some(credentials);
        }
    }

    None
}

/// 从员工表里找一个管理员的登录凭据。
fn admin_credentials() -> Option<(String, String)> {
    let employees = ExcelParser::load_employees(&paths::employee_file()).ok()?;

    for row in &employees.rows {
        let is_admin = row.values().any(|value| value.trim() == "管理员");
        if !is_admin {
            continue;
        }
        if let Some(credentials) = row_credentials(&employees, row) {
            return Some(credentials);
        }
    }

    None
}

/// 登录并取回令牌。
macro_rules! login_token {
    ($app:expr, $username:expr, $password:expr) => {{
        let request = test::TestRequest::post()
            .uri("/api/auth/login")
            .set_json(json!({ "username": $username, "password": $password }))
            .to_request();

        let response = test::call_service($app, request).await;
        let body: Value = test::read_body_json(response).await;

        body.get("token")
            .and_then(|token| token.as_str())
            .map(str::to_string)
    }};
}

#[actix_web::test]
async fn healthz_reports_ok() {
    setup();
    let app = init_app!();

    let request = test::TestRequest::get().uri("/healthz").to_request();
    let response = test::call_service(&app, request).await;

    assert_eq!(response.status(), StatusCode::OK);
}

#[actix_web::test]
async fn login_succeeds_with_valid_credentials() {
    if !sample_data_ready() {
        eprintln!("示例数据缺失，跳过");
        return;
    }

    let Some((employee_id, password)) = staff_credentials() else {
        eprintln!("员工表中没有可用账号，跳过");
        return;
    };

    let app = init_app!();

    let request = test::TestRequest::post()
        .uri("/api/auth/login")
        .set_json(json!({ "username": employee_id, "password": password }))
        .to_request();

    let response = test::call_service(&app, request).await;
    assert_eq!(response.status(), StatusCode::OK);

    let body: Value = test::read_body_json(response).await;
    assert_eq!(body["ok"], json!(true));
    assert!(
        body["token"]
            .as_str()
            .is_some_and(|token| !token.is_empty())
    );
    assert_eq!(body["employee"]["employee_id"], json!(employee_id));
}

#[actix_web::test]
async fn login_rejects_wrong_password() {
    if !sample_data_ready() {
        eprintln!("示例数据缺失，跳过");
        return;
    }

    let app = init_app!();

    let request = test::TestRequest::post()
        .uri("/api/auth/login")
        .set_json(json!({ "username": "001", "password": "definitely-not-the-password" }))
        .to_request();

    let response = test::call_service(&app, request).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let body: Value = test::read_body_json(response).await;
    assert_eq!(body["ok"], json!(false));
    // 错误提示不能泄露工号是否存在
    assert_eq!(body["error"], json!("认证失败: 工号或密码错误"));
}

#[actix_web::test]
async fn login_rejects_resigned_employee() {
    if !sample_data_ready() {
        eprintln!("示例数据缺失，跳过");
        return;
    }

    let employees = ExcelParser::load_employees(&paths::employee_file()).expect("员工表应当可读");
    let Some((employee_id, password)) = employees
        .rows
        .iter()
        .filter(|row| row.values().any(|value| value.trim() == "离职"))
        .find_map(|row| row_credentials(&employees, row))
    else {
        eprintln!("员工表中没有离职账号，跳过");
        return;
    };

    let app = init_app!();

    let request = test::TestRequest::post()
        .uri("/api/auth/login")
        .set_json(json!({ "username": employee_id, "password": password }))
        .to_request();

    let response = test::call_service(&app, request).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[actix_web::test]
async fn protected_endpoints_require_a_token() {
    setup();
    let app = init_app!();

    for uri in ["/api/available_months", "/api/salary/2026/10"] {
        let request = test::TestRequest::get().uri(uri).to_request();
        let response = test::call_service(&app, request).await;
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "{uri} 应当要求认证"
        );
    }
}

#[actix_web::test]
async fn available_months_lists_salary_files() {
    if !sample_data_ready() {
        eprintln!("示例数据缺失，跳过");
        return;
    }

    let Some((employee_id, password)) = staff_credentials() else {
        eprintln!("员工表中没有可用账号，跳过");
        return;
    };

    let app = init_app!();
    let token = login_token!(&app, &employee_id, &password).expect("登录应当成功");

    let request = test::TestRequest::get()
        .uri("/api/available_months")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();

    let response = test::call_service(&app, request).await;
    assert_eq!(response.status(), StatusCode::OK);

    let body: Value = test::read_body_json(response).await;
    let months = body["months"].as_array().expect("months 应当是数组");
    assert!(!months.is_empty(), "应当至少有一个可用月份");

    // 倒序排列
    let first = &months[0];
    let second = &months[1];
    assert!(
        (first["year"].as_i64(), first["month"].as_i64())
            >= (second["year"].as_i64(), second["month"].as_i64())
    );
}

#[actix_web::test]
async fn salary_query_returns_the_employees_own_row() {
    if !sample_data_ready() {
        eprintln!("示例数据缺失，跳过");
        return;
    }

    let Some((employee_id, password)) = staff_credentials() else {
        eprintln!("员工表中没有可用账号，跳过");
        return;
    };

    let app = init_app!();
    let token = login_token!(&app, &employee_id, &password).expect("登录应当成功");

    // 取一个实际存在的月份
    let months_request = test::TestRequest::get()
        .uri("/api/available_months")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let months_response = test::call_service(&app, months_request).await;
    let months_body: Value = test::read_body_json(months_response).await;
    let months = months_body["months"].as_array().expect("months 应当是数组");
    let Some(latest) = months.first() else {
        eprintln!("没有可用月份，跳过");
        return;
    };
    let year = latest["year"].as_i64().expect("year 应当是数字");
    let month = latest["month"].as_i64().expect("month 应当是数字");

    let request = test::TestRequest::get()
        .uri(&format!("/api/salary/{year}/{month}"))
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();

    let response = test::call_service(&app, request).await;
    assert_eq!(response.status(), StatusCode::OK);

    let body: Value = test::read_body_json(response).await;
    assert_eq!(body["ok"], json!(true));

    let headers = body["headers"].as_array().expect("headers 应当是数组");
    let values = body["values"].as_array().expect("values 应当是数组");
    assert_eq!(headers.len(), values.len());
    assert!(!headers.is_empty());

    // 返回的必须是登录员工本人的工号
    let id_index = headers
        .iter()
        .position(|header| header.as_str() == Some("员工号"))
        .expect("工资表应当包含员工号列");
    assert_eq!(values[id_index], json!(employee_id));
}

#[actix_web::test]
async fn salary_query_rejects_invalid_month() {
    if !sample_data_ready() {
        eprintln!("示例数据缺失，跳过");
        return;
    }

    let Some((employee_id, password)) = staff_credentials() else {
        eprintln!("员工表中没有可用账号，跳过");
        return;
    };

    let app = init_app!();
    let token = login_token!(&app, &employee_id, &password).expect("登录应当成功");

    let request = test::TestRequest::get()
        .uri("/api/salary/2026/13")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();

    let response = test::call_service(&app, request).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[actix_web::test]
async fn salary_query_accepts_query_parameters() {
    // GET /api/salary?year=&month= —— 与 Python 原版兼容的调用形式
    if !sample_data_ready() {
        eprintln!("示例数据缺失，跳过");
        return;
    }

    let Some((employee_id, password)) = staff_credentials() else {
        eprintln!("员工表中没有可用账号，跳过");
        return;
    };

    let app = init_app!();
    let token = login_token!(&app, &employee_id, &password).expect("登录应当成功");

    let request = test::TestRequest::get()
        .uri("/api/salary?year=2026&month=10")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();

    let response = test::call_service(&app, request).await;
    assert_eq!(response.status(), StatusCode::OK);

    let body: Value = test::read_body_json(response).await;
    assert_eq!(body["ok"], json!(true));
    assert_eq!(body["year"], json!(2026));
    assert_eq!(body["month"], json!(10));
}

#[actix_web::test]
async fn salary_query_params_require_year_and_month() {
    setup();
    let app = init_app!();

    let request = test::TestRequest::get().uri("/api/salary").to_request();
    let response = test::call_service(&app, request).await;

    // 缺少令牌时先返回 401
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[actix_web::test]
async fn salary_query_reports_missing_month() {
    if !sample_data_ready() {
        eprintln!("示例数据缺失，跳过");
        return;
    }

    let Some((employee_id, password)) = staff_credentials() else {
        eprintln!("员工表中没有可用账号，跳过");
        return;
    };

    let app = init_app!();
    let token = login_token!(&app, &employee_id, &password).expect("登录应当成功");

    let request = test::TestRequest::get()
        .uri("/api/salary/1999/1")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();

    let response = test::call_service(&app, request).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[actix_web::test]
async fn cache_endpoints_require_admin() {
    if !sample_data_ready() {
        eprintln!("示例数据缺失，跳过");
        return;
    }

    let Some((employee_id, password)) = staff_credentials() else {
        eprintln!("员工表中没有可用账号，跳过");
        return;
    };

    let app = init_app!();
    let staff_token = login_token!(&app, &employee_id, &password).expect("登录应当成功");

    let request = test::TestRequest::get()
        .uri("/api/cache/stats")
        .insert_header(("Authorization", format!("Bearer {staff_token}")))
        .to_request();

    let response = test::call_service(&app, request).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[actix_web::test]
async fn admin_can_read_cache_stats() {
    if !sample_data_ready() {
        eprintln!("示例数据缺失，跳过");
        return;
    }

    let Some((employee_id, password)) = admin_credentials() else {
        eprintln!("员工表中没有管理员账号，跳过");
        return;
    };

    let app = init_app!();
    let admin_token = login_token!(&app, &employee_id, &password).expect("管理员登录应当成功");

    let request = test::TestRequest::get()
        .uri("/api/cache/stats")
        .insert_header(("Authorization", format!("Bearer {admin_token}")))
        .to_request();

    let response = test::call_service(&app, request).await;
    assert_eq!(response.status(), StatusCode::OK);

    let body: Value = test::read_body_json(response).await;
    assert!(body["stats"]["hits"].is_number());
    assert!(body["stats"]["misses"].is_number());
}

#[actix_web::test]
async fn repeated_failures_lock_the_login() {
    if !sample_data_ready() {
        eprintln!("示例数据缺失，跳过");
        return;
    }

    let app = init_app!();

    // 阈值内的失败返回 401
    for _ in 0..4 {
        let request = test::TestRequest::post()
            .uri("/api/auth/login")
            .set_json(json!({ "username": "999", "password": "not-the-password" }))
            .to_request();

        let response = test::call_service(&app, request).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    // 第 5 次失败触发锁定
    let request = test::TestRequest::post()
        .uri("/api/auth/login")
        .set_json(json!({ "username": "999", "password": "not-the-password" }))
        .to_request();

    let response = test::call_service(&app, request).await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);

    // 锁定期间即使密码正确也会被拒绝
    let Some((employee_id, password)) = staff_credentials() else {
        eprintln!("员工表中没有可用账号，跳过");
        return;
    };

    let request = test::TestRequest::post()
        .uri("/api/auth/login")
        .set_json(json!({ "username": employee_id, "password": password }))
        .to_request();

    let response = test::call_service(&app, request).await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[actix_web::test]
async fn unknown_api_route_returns_json_404() {
    setup();
    let app = init_app!();

    let request = test::TestRequest::get()
        .uri("/api/does-not-exist")
        .to_request();
    let response = test::call_service(&app, request).await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[actix_web::test]
async fn audit_log_endpoint_requires_admin() {
    if !sample_data_ready() {
        eprintln!("示例数据缺失，跳过");
        return;
    }

    let Some((employee_id, password)) = staff_credentials() else {
        eprintln!("员工表中没有可用账号，跳过");
        return;
    };

    let app = init_app!();
    let staff_token = login_token!(&app, &employee_id, &password).expect("登录应当成功");

    let request = test::TestRequest::get()
        .uri("/api/audit/log")
        .insert_header(("Authorization", format!("Bearer {staff_token}")))
        .to_request();

    let response = test::call_service(&app, request).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[actix_web::test]
async fn admin_can_read_the_audit_log() {
    if !sample_data_ready() {
        eprintln!("示例数据缺失，跳过");
        return;
    }

    let Some((employee_id, password)) = admin_credentials() else {
        eprintln!("员工表中没有管理员账号，跳过");
        return;
    };

    let app = init_app!();
    let admin_token = login_token!(&app, &employee_id, &password).expect("管理员登录应当成功");

    let request = test::TestRequest::get()
        .uri("/api/audit/log?limit=50")
        .insert_header(("Authorization", format!("Bearer {admin_token}")))
        .to_request();

    let response = test::call_service(&app, request).await;
    assert_eq!(response.status(), StatusCode::OK);

    let body: Value = test::read_body_json(response).await;
    assert_eq!(body["ok"], json!(true));

    let events = body["events"].as_array().expect("events 应当是数组");
    assert!(!events.is_empty(), "刚刚那次登录应当已经写进审计日志");

    // 审计事件要能看出是谁、在什么时间、从哪个 IP 做了什么
    let first = &events[0];
    assert!(first["timestamp"].is_string());
    assert!(first["ip"].is_string());
    assert!(first["action"].is_string());
    assert!(first["employee_id"].is_string());
}

#[actix_web::test]
async fn frontend_assets_send_cache_headers() {
    setup();

    if paths::web_dir().is_none() {
        eprintln!("前端未构建，跳过");
        return;
    }

    let app = init_app!();

    let request = test::TestRequest::get().uri("/").to_request();
    let response = test::call_service(&app, request).await;

    assert_eq!(response.status(), StatusCode::OK);

    // 首页必须每次回源校验，否则前端更新后员工会一直看到旧版本
    let cache_control = response
        .headers()
        .get("cache-control")
        .expect("应当带 Cache-Control")
        .to_str()
        .expect("Cache-Control 应当是可读的 ASCII");
    assert_eq!(cache_control, "no-cache");

    assert!(
        response.headers().contains_key("etag"),
        "应当带 ETag 以支持条件请求"
    );
}

#[actix_web::test]
async fn frontend_assets_answer_conditional_requests() {
    setup();

    if paths::web_dir().is_none() {
        eprintln!("前端未构建，跳过");
        return;
    }

    let app = init_app!();

    let request = test::TestRequest::get().uri("/").to_request();
    let response = test::call_service(&app, request).await;
    let etag = response
        .headers()
        .get("etag")
        .expect("应当带 ETag")
        .to_str()
        .expect("ETag 应当是可读的 ASCII")
        .to_string();

    // 带上相同的 ETag 再请求一次，内容没变就只回 304
    let request = test::TestRequest::get()
        .uri("/")
        .insert_header(("If-None-Match", etag))
        .to_request();
    let response = test::call_service(&app, request).await;

    assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
}
