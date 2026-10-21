//! 集成测试：哈希口令的登录路径。
//!
//! 单独成文件是因为 `paths::data_dir()` 在进程内只解析一次，
//! 同一个测试二进制没法同时指向两个数据目录。
//!
//! 用例里的口令是测试样本自带的合成口令（`tests/fixtures/员工列表-哈希口令.xlsx`），
//! 不涉及任何真实凭据。

use actix_web::{App, http::StatusCode, test, web};
use salary_app_backend::auth::AuthContext;
use salary_app_backend::cache::DataCache;
use salary_app_backend::configure_routes;
use salary_app_backend::paths;
use serde_json::Value;
use std::path::PathBuf;

/// 把哈希口令样本放到临时数据目录。
///
/// 同一文件里的用例会并行执行，因此这里用 `Once` 保证只初始化一次——
/// 否则多个线程会同时删同一个目录、拷贝同一个文件，撞上文件锁。
fn setup() {
    static INIT: std::sync::Once = std::sync::Once::new();

    INIT.call_once(|| {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("员工列表-哈希口令.xlsx");
        let temp = std::env::temp_dir().join("salary-app-hashed-password-test");

        let _ = std::fs::remove_dir_all(&temp);
        let _ = std::fs::create_dir_all(&temp);
        let _ = std::fs::copy(&fixture, temp.join("员工列表.xlsx"));

        // SAFETY：本进程内只有这一个线程会写该环境变量；
        // `paths::data_dir()` 解析一次后即缓存，之后不再读取环境变量。
        unsafe { std::env::set_var("SALARY_DATA_DIR", &temp) };
    });
}

/// 初始化被测服务。
macro_rules! init_app {
    () => {
        test::init_service(
            App::new()
                .app_data(web::Data::new(DataCache::new()))
                .app_data(web::Data::new(AuthContext::new(
                    b"hashed-password-test-secret".to_vec(),
                )))
                .configure(configure_routes),
        )
        .await
    };
}

/// 尝试登录，返回（状态码, 响应体）。
///
/// 用宏而不是函数：被测服务的类型是 `actix_web::dev::Service`，
/// 写进函数签名要牵出 `actix_http`，而它不是本 crate 的直接依赖。
macro_rules! try_login {
    ($app:expr, $username:expr, $password:expr) => {{
        let request = test::TestRequest::post()
            .uri("/api/auth/login")
            .set_json(serde_json::json!({ "username": $username, "password": $password }))
            .to_request();

        let response = test::call_service(&$app, request).await;
        let status = response.status();
        let body: Value = test::read_body_json(response).await;

        (status, body)
    }};
}

#[actix_web::test]
async fn hashed_password_can_log_in() {
    setup();
    let app = init_app!();

    let (status, body) = try_login!(&app, "001", "salary2026");

    assert_eq!(status, StatusCode::OK, "哈希口令应当能登录：{body}");
    assert_eq!(body["ok"], serde_json::json!(true));
    assert_eq!(body["employee"]["employee_id"], serde_json::json!("001"));
    assert!(
        body["token"]
            .as_str()
            .is_some_and(|token| !token.is_empty()),
        "应当签发令牌"
    );
}

#[actix_web::test]
async fn hashed_password_rejects_the_wrong_one() {
    setup();
    let app = init_app!();

    let (status, body) = try_login!(&app, "001", "not-the-password");

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], serde_json::json!("认证失败: 工号或密码错误"));
}

#[actix_web::test]
async fn hashed_passwords_are_independent_per_employee() {
    setup();
    let app = init_app!();

    // 002 的口令与 001 不同
    let (status, _) = try_login!(&app, "002", "another-secret");
    assert_eq!(status, StatusCode::OK);

    let (status, _) = try_login!(&app, "002", "salary2026");
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[actix_web::test]
async fn plaintext_and_hashed_rows_coexist() {
    setup();
    let app = init_app!();

    // 同一张表里既有哈希口令也有明文口令，两种都能登录
    let (status, body) = try_login!(&app, "004", "plaintext-2026");
    assert_eq!(status, StatusCode::OK, "明文口令应当照常登录：{body}");

    let (status, _) = try_login!(&app, "004", "salary2026");
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[actix_web::test]
async fn unknown_employee_is_rejected() {
    setup();
    let app = init_app!();

    let (status, body) = try_login!(&app, "999", "salary2026");

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    // 提示不能泄露工号是否存在
    assert_eq!(body["error"], serde_json::json!("认证失败: 工号或密码错误"));
}

#[actix_web::test]
async fn employee_file_is_the_one_under_test() {
    // 确认这个测试进程真的指向了哈希口令样本，而不是仓库里的 data/
    setup();

    assert!(paths::employee_file().exists());
    let content = std::fs::read(paths::employee_file()).expect("应当能读取员工表");
    // XLSX 是 zip 容器，明文口令会以 deflate 后的形式存在；
    // 这里只确认文件非空且是本进程临时目录下的那份
    assert!(!content.is_empty());
    assert!(
        paths::data_dir()
            .to_string_lossy()
            .contains("hashed-password-test"),
        "数据目录应当指向哈希口令测试的临时目录，实际是 {}",
        paths::display_data_dir()
    );
}
