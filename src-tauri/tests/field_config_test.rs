//! 字段显示配置（`data/fields.txt`）的端到端测试。
//!
//! 单独一个测试二进制：它会改写数据目录里的 `fields.txt`，
//! 跟其它集成测试放在一起会互相干扰。
//!
//! 覆盖三态语义：
//! - 「跟随」：值为空或 0 时不展示，其余展示；
//! - 「显示」：强制展示；
//! - 「隐藏」：强制不展示。

use actix_web::{App, http::StatusCode, test, web};
use salary_app_backend::auth::AuthContext;
use salary_app_backend::cache::DataCache;
use salary_app_backend::configure_routes;
use salary_app_backend::excel_parser::{EmployeesData, ExcelParser};
use salary_app_backend::models::is_empty_or_zero;
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
                    b"field-config-test-secret-key".to_vec(),
                )))
                .configure(configure_routes),
        )
        .await
    };
}

/// 让测试使用临时数据目录。
fn setup() {
    static INIT: std::sync::Once = std::sync::Once::new();

    INIT.call_once(|| {
        // `CARGO_MANIFEST_DIR` 是 src-tauri/，仓库根目录在它上面一级
        let source = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data");
        let temp = std::env::temp_dir().join("salary-app-field-config-test");

        let _ = std::fs::remove_dir_all(&temp);
        let _ = std::fs::create_dir_all(&temp);

        // 员工表用仓库里固定的明文样本：登录测试需要口令明文
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("员工列表-测试用.xlsx");
        let _ = std::fs::copy(&fixture, temp.join("员工列表.xlsx"));

        // 工资表用 data/ 下的示例数据
        if let Ok(entries) = std::fs::read_dir(source.join("salary")) {
            let salary = temp.join("salary");
            let _ = std::fs::create_dir_all(&salary);
            for entry in entries.flatten() {
                let _ = std::fs::copy(entry.path(), salary.join(entry.file_name()));
            }
        }

        // SAFETY：本进程内只有这一个测试会写该环境变量，
        // 且它先于任何一次路径解析。
        unsafe { std::env::set_var("SALARY_DATA_DIR", &temp) };
    });
}

/// 示例数据是否就绪。
fn sample_data_ready() -> bool {
    setup();
    paths::employee_file().exists() && paths::salary_dir().is_dir()
}

/// 从员工表里找一个普通员工的登录凭据。
fn staff_credentials() -> Option<(String, String)> {
    let employees = ExcelParser::load_employees(&paths::employee_file()).ok()?;

    for row in &employees.rows {
        let is_resigned = row.values().any(|value| value.trim() == "离职");
        if is_resigned {
            continue;
        }
        if let Some(credentials) = row_credentials(&employees, row) {
            return Some(credentials);
        }
    }

    None
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

    if password.starts_with("$pbkdf2-sha256$") {
        return None;
    }

    Some((employee_id, password))
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

/// 找到一份工资表及其第一行数据。
fn sample_salary() -> Option<(std::path::PathBuf, Vec<String>, HashMap<String, String>)> {
    let mut files: Vec<_> = std::fs::read_dir(paths::salary_dir())
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.contains("工资"))
                .unwrap_or(false)
        })
        .collect();
    files.sort();

    let file = files.into_iter().next()?;
    let cache = DataCache::new();
    let (headers, rows) = cache.get_salary(&file).ok()?;
    let row = rows.into_iter().next()?;
    Some((file, headers, row.data))
}

/// 写一份字段配置。
fn write_config(lines: &[String]) {
    let mut content = String::new();
    for line in lines {
        content.push_str(line);
        content.push('\n');
    }
    std::fs::write(paths::field_config_file(), content).unwrap();
}

/// 查询一次工资条，返回返回的表头。
macro_rules! query_headers {
    ($app:expr, $token:expr) => {{
        let months_request = test::TestRequest::get()
            .uri("/api/available_months")
            .insert_header(("Authorization", format!("Bearer {}", $token)))
            .to_request();
        let months_response = test::call_service($app, months_request).await;
        let months_body: Value = test::read_body_json(months_response).await;
        let months = months_body["months"].as_array().expect("months 应当是数组");
        let latest = months.first().expect("应当有可用月份");
        let year = latest["year"].as_i64().expect("year 应当是数字");
        let month = latest["month"].as_i64().expect("month 应当是数字");

        let request = test::TestRequest::get()
            .uri(&format!("/api/salary/{year}/{month}"))
            .insert_header(("Authorization", format!("Bearer {}", $token)))
            .to_request();
        let response = test::call_service($app, request).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body: Value = test::read_body_json(response).await;

        body["headers"]
            .as_array()
            .expect("headers 应当是数组")
            .iter()
            .map(|value| value.as_str().expect("表头应当是字符串").to_string())
            .collect::<Vec<String>>()
    }};
}

#[actix_web::test]
async fn field_config_controls_the_returned_headers() {
    if !sample_data_ready() {
        eprintln!("示例数据缺失，跳过");
        return;
    }

    let Some((employee_id, password)) = staff_credentials() else {
        eprintln!("员工表中没有可用账号，跳过");
        return;
    };

    let Some((_file, all_headers, first_row)) = sample_salary() else {
        eprintln!("没有可用的工资表，跳过");
        return;
    };

    let app = init_app!();
    let token = login_token!(&app, &employee_id, &password).expect("登录应当成功");

    // 找一个值为空或 0 的字段：默认（跟随）不展示
    let empty_field = all_headers
        .iter()
        .find(|header| {
            first_row
                .get(*header)
                .map(|value| is_empty_or_zero(value))
                .unwrap_or(false)
        })
        .cloned();
    // 找一个有值的字段：默认（跟随）展示
    let filled_field = all_headers
        .iter()
        .find(|header| {
            first_row
                .get(*header)
                .map(|value| !is_empty_or_zero(value))
                .unwrap_or(false)
        })
        .cloned();

    let Some(filled_field) = filled_field else {
        eprintln!("工资表里没有有值的字段，跳过");
        return;
    };

    // 表头不该带首尾空白：面板写进配置的键是去空白的，
    // 两边不一致会让配置「改了不生效」。
    for header in &all_headers {
        assert_eq!(header.trim(), header, "表头带首尾空白: {header:?}");
    }

    // ---- 跟随：空值字段不展示，有值字段展示 ----
    write_config(&[]);
    let headers = query_headers!(&app, &token);
    assert!(
        headers.contains(&filled_field),
        "有值的字段默认应当展示: {headers:?}"
    );
    if let Some(empty_field) = &empty_field {
        assert!(
            !headers.contains(empty_field),
            "空值字段默认不应当展示: {headers:?}"
        );
    }

    // ---- 隐藏：有值字段也不再展示 ----
    write_config(&[format!("{filled_field} = 隐藏")]);
    let headers = query_headers!(&app, &token);
    assert!(
        !headers.contains(&filled_field),
        "隐藏之后不应当出现在表头里: {headers:?}"
    );

    // ---- 显示：空值字段强制展示 ----
    if let Some(empty_field) = &empty_field {
        write_config(&[format!("{empty_field} = 显示")]);
        let headers = query_headers!(&app, &token);
        assert!(
            headers.contains(empty_field),
            "强制显示之后应当出现在表头里: {headers:?}"
        );
    }

    // ---- 配置里的字段名按去空白匹配 ----
    write_config(&[format!("  {filled_field}  =  隐藏 ")]);
    let headers = query_headers!(&app, &token);
    assert!(
        !headers.contains(&filled_field),
        "键前后的空白不该影响匹配: {headers:?}"
    );

    // 收尾：清掉配置，别影响同目录下的其它用例
    let _ = std::fs::remove_file(paths::field_config_file());
}
