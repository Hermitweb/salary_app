//! HTTP / HTTPS 服务的构建与运行。
//!
//! 服务在独立线程里运行（自带 actix 运行时），调用方通过 [`ServerRuntime`]
//! 停止它。这样托盘面板的界面线程不会被 actix 运行时阻塞。

use crate::auth::AuthContext;
use crate::cache::DataCache;
use crate::paths;
use actix_cors::Cors;
use actix_web::{App, HttpServer, web};
use rand::RngCore;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// 等待服务完成监听的超时时间。
///
/// 绑定端口一般在毫秒级完成，生成自签名证书稍慢一些。
/// 超时后按「已启动」继续，不把调用方（界面线程）卡太久。
const READY_TIMEOUT: Duration = Duration::from_secs(10);

/// 服务运行句柄：可用于停止服务或等待其退出。
pub struct ServerRuntime {
    /// 停止信号：发给服务线程，由它在运行时里优雅关闭。
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    running: Arc<AtomicBool>,
    join: Option<std::thread::JoinHandle<()>>,
}

impl ServerRuntime {
    /// 停止服务并等待线程退出。
    pub fn stop(mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }

    /// 等待服务自行结束。
    pub fn wait(mut self) {
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }

    /// 服务是否还在运行。
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }
}

/// 确保自签名证书存在，必要时生成。
pub fn ensure_certificate(cert_path: &Path, key_path: &Path) -> std::io::Result<()> {
    if cert_path.exists() && key_path.exists() {
        return Ok(());
    }

    if let Some(parent) = cert_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    tracing::info!("正在生成自签名证书: {}", cert_path.display());

    let certified =
        rcgen::generate_simple_self_signed(vec!["localhost".to_string(), "127.0.0.1".to_string()])
            .map_err(std::io::Error::other)?;

    std::fs::write(cert_path, certified.cert.pem())?;
    std::fs::write(key_path, certified.key_pair.serialize_pem())?;

    Ok(())
}

/// 加载证书并构造 rustls 服务端配置。
pub fn build_tls_config(
    cert_path: &Path,
    key_path: &Path,
) -> std::io::Result<rustls::ServerConfig> {
    let cert_file = std::fs::File::open(cert_path)?;
    let cert_chain = rustls_pemfile::certs(&mut std::io::BufReader::new(cert_file))
        .collect::<Result<Vec<_>, _>>()?;

    if cert_chain.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("证书文件中没有找到证书: {}", cert_path.display()),
        ));
    }

    let key_file = std::fs::File::open(key_path)?;
    let private_key = rustls_pemfile::private_key(&mut std::io::BufReader::new(key_file))
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("私钥文件中没有找到私钥: {}", key_path.display()),
            )
        })?;

    rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, private_key)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))
}

/// 跨域策略。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CorsPolicy {
    /// 放行任意来源。
    Any,
    /// 只放行指定来源。
    Exact(String),
}

/// 解析 `SALARY_ALLOW_ORIGIN`。
///
/// - 未设置或为空：只允许同源（默认）；
/// - 形如 `https://hr.example.com`：只放行这一个来源；
/// - `*` 或不含 `://` 的旧式取值（如 `1`）：放行任意来源，保持旧行为。
///
/// 旧版本这个变量只是个开关，写什么都不区分来源。收紧之后，
/// 跨域调用方可以明确写出自己那个来源，而不是把整个服务暴露给任意网站。
pub fn cors_policy() -> Option<CorsPolicy> {
    let value = std::env::var("SALARY_ALLOW_ORIGIN").ok()?;
    parse_cors_policy(&value)
}

/// 解析跨域策略的取值。
pub fn parse_cors_policy(value: &str) -> Option<CorsPolicy> {
    let value = value.trim();

    if value.is_empty() {
        return None;
    }

    if value.contains("://") {
        Some(CorsPolicy::Exact(value.to_string()))
    } else {
        Some(CorsPolicy::Any)
    }
}

/// 构造跨域中间件。
pub fn cors_middleware(policy: &CorsPolicy) -> Cors {
    match policy {
        CorsPolicy::Any => Cors::default()
            .allow_any_origin()
            .allow_any_method()
            .allow_any_header()
            .max_age(3600),
        CorsPolicy::Exact(origin) => Cors::default()
            .allowed_origin(origin)
            .allow_any_method()
            .allow_any_header()
            .max_age(3600),
    }
}

/// 启动 HTTP 与 HTTPS 服务。
///
/// 服务跑在独立线程里，调用方拿到 [`ServerRuntime`] 后可以随时停止。
///
/// 端口被占用之类的绑定失败会在这里直接返回错误。绑定发生在服务线程里，
/// 不等它把结果传回来，调用方只能看到「启动成功」而服务其实没跑起来。
pub fn spawn_server(port: u16) -> std::io::Result<ServerRuntime> {
    // 端口上限要给 HTTPS 预留 +1，越界会让两个服务抢同一个端口
    if !crate::port::is_valid(port) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "端口号必须在 1-{} 之间（需要预留 HTTPS 端口 +1）",
                crate::port::MAX_PORT
            ),
        ));
    }

    let running = Arc::new(AtomicBool::new(false));

    let thread_running = running.clone();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<std::io::Result<()>>();

    let join = std::thread::Builder::new()
        .name("salary-server".to_string())
        .spawn(move || {
            let system = actix_web::rt::System::new();
            system.block_on(async move {
                match build_servers(port).await {
                    Ok((http_server, https_server)) => {
                        thread_running.store(true, Ordering::Relaxed);
                        let _ = ready_tx.send(Ok(()));

                        // 任一服务结束或收到停止信号，另一个也随之退出
                        let http_handle = http_server.handle();
                        let https_handle = https_server.handle();
                        tokio::select! {
                            _ = http_server => { https_handle.stop(false).await; }
                            _ = https_server => { http_handle.stop(false).await; }
                            _ = stop_rx => {
                                http_handle.stop(false).await;
                                https_handle.stop(false).await;
                            }
                        }

                        thread_running.store(false, Ordering::Relaxed);
                    }
                    Err(error) => {
                        let _ = ready_tx.send(Err(std::io::Error::other(error.to_string())));
                        tracing::error!("服务启动失败: {}", error);
                    }
                }
            });
        })
        .map_err(|e| std::io::Error::other(e.to_string()))?;

    // 等绑定结果：端口被占用时立刻报错，别让界面显示「服务已启动」
    match ready_rx.recv_timeout(READY_TIMEOUT) {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            let _ = join.join();
            return Err(error);
        }
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            tracing::warn!("等待服务就绪超时，按已启动继续");
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            let _ = join.join();
            return Err(std::io::Error::other("服务线程意外退出"));
        }
    }

    Ok(ServerRuntime {
        stop: Some(stop_tx),
        running,
        join: Some(join),
    })
}

/// 构建 HTTP 与 HTTPS 两个服务。
async fn build_servers(
    port: u16,
) -> std::io::Result<(actix_web::dev::Server, actix_web::dev::Server)> {
    let cert_path = paths::cert_dir().join("server.crt");
    let key_path = paths::cert_dir().join("server.key");
    ensure_certificate(&cert_path, &key_path)?;

    let tls_config = Arc::new(build_tls_config(&cert_path, &key_path)?);

    // HTTPS 端口固定是 HTTP 端口 +1
    let https_port = port + 1;

    let cache = web::Data::new(DataCache::new());
    let auth_context = web::Data::new(AuthContext::new(load_or_create_secret_key()));
    let cors_policy = cors_policy();

    let http_server = {
        let cache = cache.clone();
        let auth_context = auth_context.clone();
        let cors_policy = cors_policy.clone();

        HttpServer::new(move || {
            // 未显式放开跨域时不添加任何 CORS 响应头，浏览器只能同源访问
            let cors = match &cors_policy {
                Some(policy) => cors_middleware(policy),
                None => Cors::default(),
            };

            App::new()
                .wrap(actix_web::middleware::Logger::default())
                .wrap(cors)
                .app_data(cache.clone())
                .app_data(auth_context.clone())
                .configure(crate::configure_routes)
        })
        .bind(("0.0.0.0", port))
        .map_err(|e| std::io::Error::new(e.kind(), format!("HTTP 端口 {port} 无法监听: {e}")))?
        .run()
    };

    let https_server = {
        let cache = cache.clone();
        let auth_context = auth_context.clone();
        let cors_policy = cors_policy.clone();
        let tls_config = tls_config.clone();

        HttpServer::new(move || {
            let cors = match &cors_policy {
                Some(policy) => cors_middleware(policy),
                None => Cors::default(),
            };

            App::new()
                .wrap(actix_web::middleware::Logger::default())
                .wrap(cors)
                .app_data(cache.clone())
                .app_data(auth_context.clone())
                .configure(crate::configure_routes)
        })
        .bind_rustls_0_23(("0.0.0.0", https_port), (*tls_config).clone())
        .map_err(|e| {
            std::io::Error::new(e.kind(), format!("HTTPS 端口 {https_port} 无法监听: {e}"))
        })?
        .run()
    };

    Ok((http_server, https_server))
}

/// 读取或生成令牌签名密钥。
pub fn load_or_create_secret_key() -> Vec<u8> {
    let path = paths::secret_key_file();

    if let Ok(text) = std::fs::read_to_string(&path) {
        let trimmed = text.trim();
        if let Ok(bytes) = hex::decode(trimmed) {
            if bytes.len() >= 32 {
                return bytes;
            }
        }
        tracing::warn!("{} 内容格式异常，已重新生成密钥", path.display());
    }

    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let bytes = bytes.to_vec();

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(error) = std::fs::write(&path, hex::encode(&bytes)) {
        tracing::warn!("无法写入密钥文件 {}: {}", path.display(), error);
    }

    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cors_policy_defaults_to_same_origin_only() {
        assert_eq!(parse_cors_policy(""), None);
        assert_eq!(parse_cors_policy("   "), None);
    }

    #[test]
    fn cors_policy_accepts_a_specific_origin() {
        assert_eq!(
            parse_cors_policy("https://hr.example.com"),
            Some(CorsPolicy::Exact("https://hr.example.com".to_string()))
        );
    }

    #[test]
    fn cors_policy_keeps_legacy_boolean_behavior() {
        // 旧版本只区分「空 / 非空」，这里保持兼容
        assert_eq!(parse_cors_policy("1"), Some(CorsPolicy::Any));
        assert_eq!(parse_cors_policy("*"), Some(CorsPolicy::Any));
        assert_eq!(parse_cors_policy("yes"), Some(CorsPolicy::Any));
    }
}
