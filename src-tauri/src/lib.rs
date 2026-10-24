//! 员工工资查询系统 —— 后端库。
//!
//! 对外提供 REST API 与前端静态资源托管：
//! - `/api/auth/login` 登录并签发会话令牌；
//! - `/api/auth/me` 用令牌换取当前员工信息；
//! - `/api/available_months` 可查询月份；
//! - `/api/salary/{year}/{month}` 工资查询（需认证）；
//! - `/api/audit` 前端上报操作事件；
//! - 其它路径返回前端 SPA。
//!
//! 接口细节见 `docs/API.md`。

use actix_web::{HttpResponse, http::header, web};

/// 后端版本号（Cargo 包版本，托盘面板「关于应用」页展示）。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod audit;
pub mod auth;
pub mod cache;
pub mod error;
pub mod excel_parser;
pub mod handlers;
pub mod models;
pub mod password;
pub mod paths;
pub mod port;
pub mod server;

/// 前端未构建时返回的提示页。
pub const FRONTEND_MISSING_HTML: &str = r#"<!DOCTYPE html>
<html lang="zh-CN">
<head><meta charset="utf-8"><title>前端未构建</title></head>
<body style="font-family:system-ui,sans-serif;padding:3rem;line-height:1.8">
<h1>前端尚未构建</h1>
<p>后端服务已启动，但没有找到前端构建产物。</p>
<p>请在 <code>src-tauri</code> 目录执行：</p>
<pre style="background:#f4f4f5;padding:1rem;border-radius:8px">npm install
npm run build</pre>
<p>构建完成后刷新本页即可。</p>
</body>
</html>"#;

/// 静态资源的缓存策略。
///
/// Vite 产物的 `assets/` 下文件名带内容哈希——内容变了文件名就跟着变，
/// 因此可以放心让浏览器长期缓存。`index.html` 必须每次回源校验，
/// 否则前端更新后员工会一直看到旧版本。
fn cache_control_for(relative_path: &str) -> &'static str {
    if relative_path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    }
}

/// 依据文件元数据生成 ETag。
///
/// 只用「修改时间 + 长度」，不做内容哈希：静态资源每次请求本来就要读盘，
/// 再算一遍 SHA-256 是白花开销，而这两个字段已经足以区分「文件换了没有」。
fn etag_for(metadata: &std::fs::Metadata) -> String {
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
        .unwrap_or(0);

    format!("\"{:x}-{:x}\"", metadata.len(), modified)
}

/// 读取静态文件并带上缓存相关响应头。
fn build_static_response(relative_path: &str, body: Vec<u8>, etag: &str) -> HttpResponse {
    HttpResponse::Ok()
        .content_type(paths::content_type_for(relative_path))
        .insert_header((header::CACHE_CONTROL, cache_control_for(relative_path)))
        .insert_header((header::ETAG, etag))
        .body(body)
}

/// 从前端目录读取静态文件。
pub fn serve_static(req: &actix_web::HttpRequest, path: &str) -> Option<HttpResponse> {
    let web_dir = paths::web_dir()?;
    let relative = path.trim_start_matches('/');

    // 阻止路径穿越
    if relative
        .split('/')
        .any(|part| part == ".." || part == "." || part.contains('\\') || part.contains(':'))
    {
        return None;
    }

    let file = web_dir.join(relative);
    let metadata = std::fs::metadata(&file).ok()?;
    if !metadata.is_file() {
        return None;
    }

    let etag = etag_for(&metadata);

    // 条件请求：客户端手上已有同一份内容，直接回 304
    let client_has_current = req
        .headers()
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.split(',').any(|tag| tag.trim() == etag));

    if client_has_current {
        return Some(
            HttpResponse::NotModified()
                .insert_header((header::CACHE_CONTROL, cache_control_for(relative)))
                .insert_header((header::ETAG, etag))
                .finish(),
        );
    }

    let body = std::fs::read(&file).ok()?;

    Some(build_static_response(relative, body, &etag))
}

/// 前端资源与 SPA 兜底。
pub async fn serve_frontend(req: actix_web::HttpRequest) -> HttpResponse {
    let path = req.path();

    if path.starts_with("/api/") {
        return HttpResponse::NotFound().json(serde_json::json!({
            "ok": false,
            "error": "接口不存在",
        }));
    }

    if paths::web_dir().is_none() {
        return HttpResponse::ServiceUnavailable()
            .content_type("text/html; charset=utf-8")
            .body(FRONTEND_MISSING_HTML);
    }

    if let Some(response) = serve_static(&req, path) {
        return response;
    }

    // SPA 兜底：没有扩展名的路径一律返回首页
    if std::path::Path::new(path).extension().is_none() {
        if let Some(response) = serve_static(&req, "/index.html") {
            return response;
        }
    }

    HttpResponse::NotFound().finish()
}

/// 图标：前端未使用图标资源，直接返回 204。
pub async fn favicon() -> HttpResponse {
    HttpResponse::NoContent().finish()
}

/// 注册全部路由。
pub fn configure_routes(cfg: &mut web::ServiceConfig) {
    cfg.route("/healthz", web::get().to(handlers::healthcheck))
        .route("/api/auth/login", web::post().to(handlers::login))
        .route("/api/auth/me", web::get().to(handlers::whoami))
        .route("/api/logout", web::post().to(handlers::logout))
        .route(
            "/api/available_months",
            web::get().to(handlers::available_months),
        )
        .route(
            "/api/salary/{year}/{month}",
            web::get().to(handlers::salary_query),
        )
        // 查询参数形式，兼容 Python 原版的 /api/salary?year=&month=
        .route("/api/salary", web::get().to(handlers::salary_query_params))
        .route("/api/audit", web::post().to(handlers::audit_record))
        .route("/favicon.ico", web::get().to(favicon))
        .default_service(web::route().to(serve_frontend));
}
