//! 端到端测试：拉起真实的 `salary-app-backend` 进程，走完整业务链路。
//!
//! 与 `integration_test.rs` 的区别：那边把服务挂在测试进程内（actix 的
//! `test::init_service`），这边**真的起一个进程**，用 TCP 说 HTTP。
//! 这样能覆盖到只有独立进程才会暴露的问题：端口解析、证书自签、
//! 数据目录初始化、进程退出时的清理。
//!
//! HTTP 客户端是自己写的极简实现：这个仓库不引入额外的 HTTP 依赖，
//! 免得为了跑个测试多一份供应链。

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// 极简 HTTP 客户端：发一个请求，把状态码与响应体拿回来。
fn request(
    port: u16,
    method: &str,
    path: &str,
    body: Option<&str>,
    token: Option<&str>,
) -> (u16, String) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("应当能连上服务");
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .expect("应当能设置超时");

    let mut request = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nAccept: application/json\r\n"
    );
    if let Some(token) = token {
        request.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    match body {
        Some(body) => {
            request.push_str("Content-Type: application/json\r\n");
            request.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));
            request.push_str(body);
        }
        None => request.push_str("\r\n"),
    }

    stream
        .write_all(request.as_bytes())
        .expect("应当能发出请求");
    stream.flush().expect("应当能发出请求");

    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).expect("应当能读完响应");
    let text = String::from_utf8_lossy(&raw).to_string();

    let (head, body) = text.split_once("\r\n\r\n").expect("响应应当有头部");
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .unwrap_or(0);

    let body = if head.to_lowercase().contains("transfer-encoding: chunked") {
        decode_chunked(body)
    } else {
        body.to_string()
    };

    (status, body)
}

/// 解 chunked 编码的响应体。
fn decode_chunked(body: &str) -> String {
    let mut out = String::new();
    let mut rest = body;

    while let Some((size_line, tail)) = rest.split_once("\r\n") {
        let size = usize::from_str_radix(size_line.trim().split(';').next().unwrap_or("0"), 16)
            .unwrap_or(0);
        if size == 0 || tail.len() < size {
            break;
        }
        out.push_str(&tail[..size]);
        rest = tail[size..].trim_start_matches("\r\n");
    }

    out
}

/// 从 JSON 里取一个字符串字段（够用就好，不引 JSON 解析库）。
fn json_string(body: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let start = body.find(&needle)? + needle.len();
    let tail = &body[start..];
    let colon = tail.find(':')? + 1;
    let tail = tail[colon..].trim_start();
    if !tail.starts_with('"') {
        return None;
    }
    let tail = &tail[1..];
    let end = tail.find('"')?;
    Some(tail[..end].to_string())
}

/// 从 JSON 数组里取出所有字符串元素（`headers` 就是字符串数组）。
fn json_string_array(body: &str, key: &str) -> Vec<String> {
    let Some(needle) = body.find(&format!("\"{key}\"")) else {
        return Vec::new();
    };
    let Some(open) = body[needle..].find('[') else {
        return Vec::new();
    };
    let tail = &body[needle + open + 1..];
    let Some(close) = tail.find(']') else {
        return Vec::new();
    };

    tail[..close]
        .split(',')
        .filter_map(|part| {
            let part = part.trim();
            part.strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
                .map(str::to_string)
        })
        .collect()
}

/// 从 JSON 里取一个数字字段（够用就好，不引 JSON 解析库）。
fn json_number(body: &str, key: &str) -> Option<i32> {
    let needle = format!("\"{key}\"");
    let start = body.find(&needle)? + needle.len();
    let tail = &body[start..];
    let colon = tail.find(':')? + 1;
    tail[colon..]
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '-')
        .collect::<String>()
        .parse()
        .ok()
}

/// 取响应里第一个可查询的月份。
///
/// `months` 是对象数组（`{"year":2026,"month":10}`），不是字符串数组。
fn first_month(body: &str) -> Option<(i32, u32)> {
    let anchor = body.find("\"months\"")?;
    let tail = &body[anchor..];
    let open = tail.find('[')?;
    let items = &tail[open + 1..];
    let close = items.find(']')?;
    let first = &items[..close];
    Some((
        json_number(first, "year")?,
        json_number(first, "month")? as u32,
    ))
}

/// 守护子进程：测试无论怎么结束（断言失败、panic），都把它收掉。
///
/// 不收的话，后端会变成孤儿进程一直占着端口，
/// 也会让调用方（cargo）的输出管道迟迟不关闭。
struct ServerGuard(Child);

impl Drop for ServerGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// 准备测试数据目录。
fn setup_data_dir() -> PathBuf {
    // `CARGO_MANIFEST_DIR` 是 src-tauri/，仓库根目录在它上面一级
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data");
    let temp = std::env::temp_dir().join("salary-app-e2e-test");

    let _ = std::fs::remove_dir_all(&temp);
    std::fs::create_dir_all(&temp).expect("应当能建出临时数据目录");

    // 员工表用明文样本，登录测试需要口令
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("员工列表-测试用.xlsx");
    std::fs::copy(&fixture, temp.join("员工列表.xlsx")).expect("应当能复制员工表");

    let salary = temp.join("salary");
    std::fs::create_dir_all(&salary).expect("应当能建出工资表目录");
    if let Ok(entries) = std::fs::read_dir(source.join("salary")) {
        for entry in entries.flatten() {
            let _ = std::fs::copy(entry.path(), salary.join(entry.file_name()));
        }
    }

    temp
}

/// 找一个没人占用的端口。
///
/// 连续取两个：HTTP 用第一个，HTTPS 用第一个 +1，两个都得空着。
fn free_port_pair() -> u16 {
    for candidate in 15300..16000u16 {
        let http_ok = std::net::TcpListener::bind(("127.0.0.1", candidate)).is_ok();
        let https_ok = std::net::TcpListener::bind(("127.0.0.1", candidate + 1)).is_ok();
        if http_ok && https_ok {
            return candidate;
        }
    }
    panic!("找不到可用端口");
}

/// 等服务起来。
fn wait_until_ready(port: u16, child: &mut Child) {
    let deadline = Instant::now() + Duration::from_secs(30);

    while Instant::now() < deadline {
        if let Ok(Some(status)) = child.try_wait() {
            panic!("服务进程提前退出，状态码 {status}");
        }
        if let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) {
            stream.set_read_timeout(Some(Duration::from_secs(2))).ok();
            let request = format!(
                "GET /healthz HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
            );
            if stream.write_all(request.as_bytes()).is_ok() {
                let mut raw = Vec::new();
                if stream.read_to_end(&mut raw).is_ok() && raw.starts_with(b"HTTP/1.1 200") {
                    return;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }

    panic!("服务在 30 秒内没有就绪");
}

#[test]
fn end_to_end_login_and_salary_query() {
    let data_dir = setup_data_dir();
    let port = free_port_pair();

    let child = Command::new(env!("CARGO_BIN_EXE_salary-app-backend"))
        .env("SALARY_DATA_DIR", &data_dir)
        .env("SALARY_PORT", port.to_string())
        // 测试里不要弹浏览器
        .env("SALARY_NO_BROWSER", "1")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("应当能启动后端进程");

    let mut server = ServerGuard(child);
    wait_until_ready(port, &mut server.0);

    // ---- 健康检查 ----
    let (status, _) = request(port, "GET", "/healthz", None, None);
    assert_eq!(status, 200, "健康检查应当返回 200");

    // ---- 未登录不能查工资 ----
    let (status, _) = request(port, "GET", "/api/available_months", None, None);
    assert_eq!(status, 401, "未登录访问应当被拒绝");

    // ---- 登录 ----
    let (status, body) = request(
        port,
        "POST",
        "/api/auth/login",
        Some(r#"{"username":"001","password":"salary2026"}"#),
        None,
    );
    assert_eq!(status, 200, "登录应当成功：{body}");
    let token = json_string(&body, "token").expect("登录应当返回令牌");

    // ---- 可查询月份 ----
    let (status, body) = request(port, "GET", "/api/available_months", None, Some(&token));
    assert_eq!(status, 200, "应当能查到月份：{body}");
    let (year, month) = first_month(&body).expect("应当至少有一个月份");

    // ---- 查工资 ----
    let (status, body) = request(
        port,
        "GET",
        &format!("/api/salary/{year}/{month}"),
        None,
        Some(&token),
    );
    assert_eq!(status, 200, "应当能查到工资条：{body}");
    let headers = json_string_array(&body, "headers");
    assert!(
        headers.contains(&"姓名".to_string()),
        "表头里应当有姓名：{headers:?}"
    );
    assert!(
        headers.contains(&"本月实发工资".to_string()),
        "表头里应当有本月实发工资：{headers:?}"
    );

    // ---- 字段显示配置立刻生效 ----
    std::fs::write(data_dir.join("fields.txt"), "本月实发工资 = 隐藏\n").expect("应当能写字段配置");

    let (status, body) = request(
        port,
        "GET",
        &format!("/api/salary/{year}/{month}"),
        None,
        Some(&token),
    );
    assert_eq!(status, 200, "改配置之后仍然应当能查到工资条：{body}");
    let headers = json_string_array(&body, "headers");
    assert!(
        !headers.contains(&"本月实发工资".to_string()),
        "隐藏之后不应当再返回该字段：{headers:?}"
    );

    // ---- 口令错误不通过 ----
    let (status, _) = request(
        port,
        "POST",
        "/api/auth/login",
        Some(r#"{"username":"001","password":"wrong-password"}"#),
        None,
    );
    assert_eq!(status, 401, "错误口令应当被拒绝");

    // ---- 收尾 ----
    drop(server);
    let _ = std::fs::remove_dir_all(&data_dir);
}
