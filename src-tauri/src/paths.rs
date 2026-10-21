//! 运行时路径解析。
//!
//! 数据目录（员工表、工资表、日志、证书）与前端静态资源目录在整个进程内
//! 必须保持一致，因此集中在这里解析，避免各模块各写一份导致互相错位。

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// 读取非空环境变量并转成路径。
fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// 编译期的工程目录。仅在它确实存在时才用于定位数据目录，
/// 这样发布到其它机器后不会误指向构建机的路径。
const BUILD_MANIFEST_DIR: &str = env!("CARGO_MANIFEST_DIR");

/// 数据目录。
///
/// 优先级：
/// 1. `SALARY_DATA_DIR` 环境变量；
/// 2. `cargo run`（开发模式）：仓库根目录下的 `data/`；
/// 3. 直接运行可执行文件（发布模式）：优先使用可执行文件上两级目录的 `data/`，
///    否则使用可执行文件同级的 `data/`（便携模式）。
///
/// 结果只解析一次并缓存，避免运行中环境变量变化导致路径漂移。
pub fn data_dir() -> PathBuf {
    static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();
    DATA_DIR.get_or_init(resolve_data_dir).clone()
}

/// 解析数据目录。
fn resolve_data_dir() -> PathBuf {
    if let Some(dir) = env_path("SALARY_DATA_DIR") {
        return dir;
    }

    // 运行时环境变量（cargo run 时由 cargo 注入）
    if let Some(manifest) = env_path("CARGO_MANIFEST_DIR") {
        return manifest.join("..").join("data");
    }

    // 编译期路径：目录仍存在时说明就在这台机器上开发
    let build_manifest = Path::new(BUILD_MANIFEST_DIR);
    if build_manifest.is_dir() {
        return build_manifest.join("..").join("data");
    }

    if let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    {
        // target/release/ -> 仓库根
        let workspace_data = exe_dir.join("..").join("..").join("data");
        if workspace_data.is_dir() {
            return workspace_data;
        }
        return exe_dir.join("data");
    }

    PathBuf::from("data")
}

/// 员工列表 Excel 文件路径。
pub fn employee_file() -> PathBuf {
    data_dir().join("员工列表.xlsx")
}

/// 工资表所在目录。
pub fn salary_dir() -> PathBuf {
    data_dir().join("salary")
}

/// 自签名证书目录。
pub fn cert_dir() -> PathBuf {
    data_dir().join("certs")
}

/// 令牌签名密钥文件。
pub fn secret_key_file() -> PathBuf {
    data_dir().join("secret.key")
}

/// 审计日志文件。
pub fn audit_log_file() -> PathBuf {
    data_dir().join("audit.log")
}

/// 字段显示配置文件。
pub fn field_config_file() -> PathBuf {
    data_dir().join("fields.txt")
}

/// 端口配置文件。
///
/// 与 Python 版一致：控制面板把端口写在这里，服务启动时读取。
pub fn port_file() -> PathBuf {
    data_dir().join("port.txt")
}

/// 用于日志展示的数据目录路径。
///
/// `data_dir()` 为了避免符号链接问题不做规范化，这里给出一份整洁的路径，
/// 避免日志里出现 `src-tauri\..\data` 这样的中间形式。
pub fn display_data_dir() -> String {
    let dir = data_dir();

    let canonical = std::fs::canonicalize(&dir)
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| dir.display().to_string());

    // Windows 下 canonicalize 会带上 `\\?\` 前缀
    canonical
        .strip_prefix(r"\\?\")
        .unwrap_or(&canonical)
        .to_string()
}

// ---------------------------------------------------------------------------
// 前端静态资源
// ---------------------------------------------------------------------------

/// 前端构建产物目录。
///
/// 优先级：
/// 1. `SALARY_WEB_DIR` 环境变量；
/// 2. `cargo run`（开发模式）：`src-tauri/dist/`；
/// 3. 发布模式：可执行文件上两级的 `src-tauri/dist/`（workspace 构建），
///    或可执行文件同级的 `web/`（便携分发）。
pub fn web_dir() -> Option<PathBuf> {
    if let Some(dir) = env_path("SALARY_WEB_DIR") {
        return Some(dir);
    }

    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Some(manifest) = env_path("CARGO_MANIFEST_DIR") {
        candidates.push(manifest.join("dist"));
    }

    if let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    {
        candidates.push(exe_dir.join("..").join("..").join("src-tauri").join("dist"));
        candidates.push(exe_dir.join("web"));
    }

    candidates
        .into_iter()
        .find(|dir| dir.join("index.html").is_file())
}

/// 根据文件扩展名推断 Content-Type。
pub fn content_type_for(path: &str) -> &'static str {
    let lower = path.to_lowercase();

    if lower.ends_with(".html") || lower.ends_with(".htm") {
        "text/html; charset=utf-8"
    } else if lower.ends_with(".js") || lower.ends_with(".mjs") {
        "application/javascript; charset=utf-8"
    } else if lower.ends_with(".css") {
        "text/css; charset=utf-8"
    } else if lower.ends_with(".json") {
        "application/json; charset=utf-8"
    } else if lower.ends_with(".svg") {
        "image/svg+xml"
    } else if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else if lower.ends_with(".ico") {
        "image/x-icon"
    } else if lower.ends_with(".woff2") {
        "font/woff2"
    } else if lower.ends_with(".woff") {
        "font/woff"
    } else if lower.ends_with(".ttf") {
        "font/ttf"
    } else if lower.ends_with(".map") {
        "application/json; charset=utf-8"
    } else {
        "application/octet-stream"
    }
}
