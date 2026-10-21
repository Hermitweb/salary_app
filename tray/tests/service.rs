//! 托盘面板服务管理的集成测试。
//!
//! 验证面板上的「启动服务 / 停止服务」走的是同一条代码路径：
//! 启停服务、端口探测、以及服务真的能响应 HTTP 请求。

use salary_tray::service::{self, ServiceManager};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

/// 选一个不常用的高位端口，避免和真实服务冲突。
const TEST_PORT: u16 = 5099;

/// 读取某个端口上的 HTTP 响应。
fn http_get(port: u16, path: &str) -> Option<String> {
    let address = ("127.0.0.1", port).to_socket_addrs().ok()?.next()?;
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(3))).ok()?;

    let request = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).ok()?;

    let mut response = String::new();
    stream.read_to_string(&mut response).ok()?;
    Some(response)
}

/// 等待服务在端口上就绪。
fn wait_until_ready(port: u16, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if http_get(port, "/healthz").is_some() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    false
}

#[test]
fn panel_starts_and_stops_the_service() {
    // 保护现有端口配置，测试结束后还原
    let original_port = service::load_port();
    let port_file = salary_app_backend::paths::port_file();
    let had_port_file = port_file.exists();

    let mut manager = ServiceManager::new();

    // 服务没在跑时才能改端口
    if manager.is_running() {
        eprintln!("检测到服务已在运行，跳过该测试");
        return;
    }

    manager.set_port(TEST_PORT).expect("应当能设置端口");
    assert_eq!(manager.port(), TEST_PORT);

    let started = manager.start();
    assert!(started.is_ok(), "启动服务失败: {:?}", started);

    // 服务是异步起来的，稍等它就绪
    assert!(
        wait_until_ready(TEST_PORT, Duration::from_secs(30)),
        "服务在超时前没有就绪"
    );
    assert!(manager.is_running(), "服务应当处于运行状态");

    // 端口能被探测到
    assert!(service::is_port_listening(TEST_PORT));

    // 健康检查返回 OK
    let response = http_get(TEST_PORT, "/healthz").expect("应当能访问 /healthz");
    assert!(
        response.contains("200 OK"),
        "健康检查响应异常: {}",
        response.lines().next().unwrap_or("")
    );

    // 前端页面也能访问
    let index = http_get(TEST_PORT, "/").expect("应当能访问首页");
    assert!(
        index.contains("<!DOCTYPE html>"),
        "首页响应异常: {}",
        index.lines().next().unwrap_or("")
    );

    // 停止服务
    let stopped = manager.stop();
    assert!(stopped.is_ok(), "停止服务失败: {:?}", stopped);

    // 等待端口释放
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline && service::is_port_listening(TEST_PORT) {
        std::thread::sleep(Duration::from_millis(200));
    }
    assert!(!service::is_port_listening(TEST_PORT), "端口应当已释放");

    // 还原端口配置
    if had_port_file {
        let _ = std::fs::write(&port_file, original_port.to_string());
    } else {
        let _ = std::fs::remove_file(&port_file);
    }
}
