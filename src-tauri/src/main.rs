//! 员工工资查询系统 —— 服务进程入口。
//!
//! 同时监听 HTTP（`SALARY_PORT`，默认 5001）与 HTTPS（端口 +1，默认 5002），
//! 并把前端构建产物一并托管，实现单进程部署。

use salary_app_backend::{paths, server};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// 默认监听端口。
const DEFAULT_PORT: u16 = 5001;

/// 解析监听端口。
///
/// 优先级与 Python 原版一致：环境变量 `SALARY_PORT` > `data/port.txt` > 默认值。
fn resolve_port() -> u16 {
    if let Ok(value) = std::env::var("SALARY_PORT") {
        if let Ok(port) = value.trim().parse::<u16>() {
            return port;
        }
        tracing::warn!("SALARY_PORT 不是合法端口号，改用配置文件");
    }

    std::fs::read_to_string(paths::port_file())
        .ok()
        .and_then(|text| text.trim().parse::<u16>().ok())
        .filter(|port| (1..=65534).contains(port))
        .unwrap_or(DEFAULT_PORT)
}

/// 获取本机局域网 IP。
///
/// 用 UDP socket 的「连接但不发送」技巧拿到出口网卡地址：
/// UDP 的 `connect()` 只设置默认对端，不会发出任何数据包。
fn local_ip() -> Option<String> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;

    let ip = socket.local_addr().ok()?.ip();
    if ip.is_loopback() || ip.is_unspecified() {
        return None;
    }

    Some(ip.to_string())
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "salary_app=debug,actix_web=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // 显式安装 rustls 加密提供方，避免多 provider 冲突
    if rustls::crypto::ring::default_provider()
        .install_default()
        .is_err()
    {
        tracing::debug!("rustls 加密提供方已安装，跳过");
    }

    let data_dir = paths::data_dir();
    std::fs::create_dir_all(&data_dir)?;
    std::fs::create_dir_all(paths::salary_dir())?;

    tracing::info!("数据目录: {}", paths::display_data_dir());

    let port = resolve_port();
    let runtime = server::spawn_server(port)?;

    tracing::info!("==================================================");
    tracing::info!("  员工工资查询系统");
    tracing::info!("==================================================");
    tracing::info!("  本机访问: http://localhost:{}", port);
    if let Some(ip) = local_ip() {
        tracing::info!("  局域网访问: http://{}:{}", ip, port);
    }
    tracing::info!("  HTTPS: https://localhost:{}", port.saturating_add(1));
    tracing::info!("  监听地址: 0.0.0.0:{}", port);
    tracing::info!("==================================================");

    open_browser(port);

    runtime.wait();

    Ok(())
}

/// 启动后自动打开浏览器（设置 `SALARY_NO_BROWSER=1` 可关闭）。
fn open_browser(port: u16) {
    if std::env::var("SALARY_NO_BROWSER")
        .map(|value| value.trim() == "1")
        .unwrap_or(false)
    {
        return;
    }

    let url = format!("http://localhost:{port}");

    std::thread::spawn(move || {
        // 等待服务器完成监听
        std::thread::sleep(std::time::Duration::from_millis(800));
        let _ = webbrowser::open(&url);
    });
}
