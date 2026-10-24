//! 性能基准：后端各条热路径的耗时。
//!
//! 用 `harness = false` 跑，不引入 criterion 一类的外部依赖：
//! 基准只需要计时，自己写一套更省事，也不给构建增加网络依赖。
//!
//! 运行：
//! ```text
//! cargo bench -p salary-app-backend
//! ```
//!
//! 输出每项的平均耗时与吞吐。登录链路上最贵的是 PBKDF2
//! （210000 轮），它是口令哈希强度换来的，不能为了快而降低。

use salary_app_backend::auth::{AuthContext, verify_session_token};
use salary_app_backend::cache::DataCache;
use salary_app_backend::excel_parser::ExcelParser;
use salary_app_backend::models::Employee;
use salary_app_backend::password;
use salary_app_backend::paths;
use std::time::Instant;

/// 跑若干轮，报告平均耗时与吞吐。
fn bench(name: &str, iterations: u32, mut run: impl FnMut()) {
    // 预热一轮：首次调用可能包含惰性初始化（正则编译、连接建立等）
    run();

    let start = Instant::now();
    for _ in 0..iterations {
        run();
    }
    let elapsed = start.elapsed();
    let per_ms = elapsed.as_secs_f64() * 1000.0 / iterations as f64;
    let per_second = iterations as f64 / elapsed.as_secs_f64();

    println!("{name:<30} {per_ms:>10.3} ms/次   {per_second:>12.1} 次/秒");
}

/// 找到一份工资表用于解析基准。
fn sample_salary_file() -> Option<std::path::PathBuf> {
    salary_app_backend::handlers::queryable_salary_files()
        .into_iter()
        .next()
}

fn main() {
    println!("=== 后端热路径性能基准 ===");
    println!();

    // ---- 口令哈希：登录链路上最贵的一步 ----
    // 210000 轮 PBKDF2 是安全强度的一部分，
    // 这里的数字是「安全换来的时间」，不要照着调低。
    let hashed = password::hash("salary2026");
    bench("口令哈希 PBKDF2-210000", 20, || {
        let _ = password::hash("salary2026");
    });
    bench("口令校验 PBKDF2-210000", 20, || {
        assert!(password::verify(&hashed, "salary2026"));
    });
    bench("口令校验（明文兼容路径）", 2000, || {
        assert!(password::verify("salary2026", "salary2026"));
    });

    // ---- 会话令牌 ----
    let auth = AuthContext::new(b"benchmark-secret-key-32-bytes-long!!".to_vec());
    let employee = Employee {
        employee_id: "001".to_string(),
        name: Some("张三".to_string()),
        dept: Some("工程部".to_string()),
    };
    let token = auth.issue_token(&employee).expect("签发令牌应当成功");
    bench("会话令牌签发", 2000, || {
        let _ = auth.issue_token(&employee).unwrap();
    });
    bench("会话令牌校验", 2000, || {
        verify_session_token(&token, b"benchmark-secret-key-32-bytes-long!!").unwrap();
    });

    // ---- Excel 解析 ----
    if let Some(file) = sample_salary_file() {
        bench("工资表冷解析（无缓存）", 50, || {
            let cache = DataCache::new();
            let _ = cache.get_salary(&file).expect("工资表应当可解析");
        });

        let cache = DataCache::new();
        let _ = cache.get_salary(&file);
        bench("工资表热解析（命中缓存）", 2000, || {
            let _ = cache.get_salary(&file).expect("工资表应当可解析");
        });
    } else {
        println!("（没有示例工资表，跳过 Excel 解析基准）");
    }

    // ---- 字段显示配置 ----
    // 每次查询都会重新读这个文件：配置要「保存即生效」，
    // 就不能缓存住。这项耗时直接进每个请求的路径。
    let config_path = paths::field_config_file();
    bench("字段配置读取", 2000, || {
        let _ = ExcelParser::load_field_config(&config_path);
    });

    // ---- 员工表解析 ----
    let employee_file = paths::employee_file();
    if employee_file.exists() {
        bench("员工表解析", 200, || {
            let _ = ExcelParser::load_employees(&employee_file);
        });
    }

    // ---- HTTP 处理耗时（进程内，不含网络与序列化开销之外的等待）----
    measure_http();

    println!();
}

/// 测 HTTP 接口的服务端处理耗时。
///
/// 用 actix 的测试服务直接调用处理器：量到的是「服务端真正花掉的时间」，
/// 不掺杂 socket 往返。真实网络延迟另由 `scripts/bench.ps1` 测。
fn measure_http() {
    use actix_web::{App, http::StatusCode, test, web};
    use salary_app_backend::configure_routes;

    let secret = b"benchmark-secret-key-32-bytes-long!!".to_vec();
    let runtime = actix_web::rt::System::new();

    runtime.block_on(async {
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(DataCache::new()))
                .app_data(web::Data::new(AuthContext::new(secret.clone())))
                .configure(configure_routes),
        )
        .await;

        // 登录：口令不对也要跑完整的 PBKDF2，耗时与成功登录同量级
        {
            let start = Instant::now();
            let iterations = 20;
            for _ in 0..iterations {
                let request = test::TestRequest::post()
                    .uri("/api/auth/login")
                    .set_json(serde_json::json!({
                        "username": "001",
                        "password": "salary2026"
                    }))
                    .to_request();
                let _ = test::call_service(&app, request).await;
            }
            let elapsed = start.elapsed();
            println!(
                "{:<30} {:>10.3} ms/次   {:>12.1} 次/秒",
                "HTTP 登录处理",
                elapsed.as_secs_f64() * 1000.0 / iterations as f64,
                iterations as f64 / elapsed.as_secs_f64()
            );
        }

        // 查工资：令牌直接签发，省掉登录那一步
        {
            let employee = Employee {
                employee_id: "001".to_string(),
                name: Some("张三".to_string()),
                dept: Some("工程部".to_string()),
            };
            let token = salary_app_backend::auth::issue_session_token(&employee, &secret, 30)
                .expect("应当能签发令牌");

            let start = Instant::now();
            let iterations = 500;
            for _ in 0..iterations {
                let request = test::TestRequest::get()
                    .uri("/api/salary/2026/10")
                    .insert_header(("Authorization", format!("Bearer {token}")))
                    .to_request();
                let response = test::call_service(&app, request).await;
                assert_eq!(response.status(), StatusCode::OK);
            }
            let elapsed = start.elapsed();
            println!(
                "{:<30} {:>10.3} ms/次   {:>12.1} 次/秒",
                "HTTP 查工资处理",
                elapsed.as_secs_f64() * 1000.0 / iterations as f64,
                iterations as f64 / elapsed.as_secs_f64()
            );
        }
    });
}
