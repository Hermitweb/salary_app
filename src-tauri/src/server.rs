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

/// 是否允许跨域访问。
///
/// 默认同源即可访问；设置 `SALARY_ALLOW_ORIGIN=*` 可放开
/// （仅在确实需要被其它系统调用时使用）。
pub fn cors_enabled() -> bool {
    std::env::var("SALARY_ALLOW_ORIGIN")
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

/// 构造跨域中间件。
pub fn cors_middleware() -> Cors {
    Cors::default()
        .allow_any_origin()
        .allow_any_method()
        .allow_any_header()
        .max_age(3600)
}

/// 启动 HTTP 与 HTTPS 服务。
///
/// 服务跑在独立线程里，调用方拿到 [`ServerRuntime`] 后可以随时停止。
pub fn spawn_server(port: u16) -> std::io::Result<ServerRuntime> {
    let running = Arc::new(AtomicBool::new(false));

    let thread_running = running.clone();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();

    let join = std::thread::Builder::new()
        .name("salary-server".to_string())
        .spawn(move || {
            let system = actix_web::rt::System::new();
            system.block_on(async move {
                match build_servers(port).await {
                    Ok((http_server, https_server)) => {
                        thread_running.store(true, Ordering::Relaxed);

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
                        tracing::error!("服务启动失败: {}", error);
                    }
                }
            });
        })
        .map_err(|e| std::io::Error::other(e.to_string()))?;

    // 不等待服务就绪：证书生成与监听都要时间，阻塞会把调用方（界面线程）卡住。
    // 调用方用 `is_running()` 轮询状态即可。
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

    let cache = web::Data::new(DataCache::new());
    let auth_context = web::Data::new(AuthContext::new(load_or_create_secret_key()));
    let allow_cors = cors_enabled();

    let http_server = {
        let cache = cache.clone();
        let auth_context = auth_context.clone();

        HttpServer::new(move || {
            // 未显式放开跨域时不添加任何 CORS 响应头，浏览器只能同源访问
            let cors = if allow_cors {
                cors_middleware()
            } else {
                Cors::default()
            };

            App::new()
                .wrap(actix_web::middleware::Logger::default())
                .wrap(cors)
                .app_data(cache.clone())
                .app_data(auth_context.clone())
                .configure(crate::configure_routes)
        })
        .bind(("0.0.0.0", port))?
        .run()
    };

    let https_server = {
        let cache = cache.clone();
        let auth_context = auth_context.clone();
        let tls_config = tls_config.clone();

        HttpServer::new(move || {
            let cors = if allow_cors {
                cors_middleware()
            } else {
                Cors::default()
            };

            App::new()
                .wrap(actix_web::middleware::Logger::default())
                .wrap(cors)
                .app_data(cache.clone())
                .app_data(auth_context.clone())
                .configure(crate::configure_routes)
        })
        .bind_rustls_0_23(("0.0.0.0", port.saturating_add(1)), (*tls_config).clone())?
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
