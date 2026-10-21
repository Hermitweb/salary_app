//! 服务管理：启动、停止、状态探测与端口配置。
//!
//! 服务直接跑在托盘进程内（独立线程 + 独立 actix 运行时），
//! 因此启动/停止都不需要拉起子进程。
//!
//! 状态探测用 TCP 连接判断端口是否在监听，**不发起任何 HTTP 请求**。
//! 端口探测最多会阻塞数百毫秒，因此放在后台线程做，
//! 界面线程只读缓存结果——否则窗口会被判定为「未响应」。

use salary_app_backend::paths;
use salary_app_backend::server::{self, ServerRuntime};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// 默认端口（与后端一致）。
pub const DEFAULT_PORT: u16 = 5001;

/// 端口号上限：需要给 HTTPS 预留 +1。
const MAX_PORT: u16 = 65534;

/// 后台端口探测的间隔。
const PROBE_INTERVAL: Duration = Duration::from_secs(2);

/// 后台端口探测结果缓存。
///
/// 界面线程只读 `listening`，探测本身在后台线程完成。
#[derive(Debug)]
pub struct PortProbe {
    /// 要探测的端口。
    port: AtomicU16,
    /// 端口上是否有进程在监听。
    listening: AtomicBool,
}

impl PortProbe {
    /// 创建缓存并启动后台探测线程。
    fn spawn(port: u16) -> Arc<Self> {
        let probe = Arc::new(Self {
            port: AtomicU16::new(port),
            listening: AtomicBool::new(false),
        });

        let worker = Arc::clone(&probe);
        std::thread::Builder::new()
            .name("salary-port-probe".to_string())
            .spawn(move || {
                while worker.port.load(Ordering::Relaxed) != 0 {
                    let port = worker.port.load(Ordering::Relaxed);
                    worker
                        .listening
                        .store(is_port_listening(port), Ordering::Relaxed);
                    std::thread::sleep(PROBE_INTERVAL);
                }
            })
            .ok();

        probe
    }

    /// 探测指定端口。
    fn set_port(&self, port: u16) {
        self.port.store(port, Ordering::Relaxed);
        self.listening.store(false, Ordering::Relaxed);
    }

    /// 端口上是否有进程在监听（读缓存，不阻塞）。
    fn is_listening(&self) -> bool {
        self.listening.load(Ordering::Relaxed)
    }
}

/// 服务管理器。
pub struct ServiceManager {
    /// 由本程序启动的服务。
    runtime: Option<ServerRuntime>,
    port: u16,
    /// 后台端口探测缓存。
    probe: Arc<PortProbe>,
}

impl ServiceManager {
    /// 创建管理器并读取已配置的端口。
    pub fn new() -> Self {
        let port = load_port();
        Self {
            runtime: None,
            port,
            probe: PortProbe::spawn(port),
        }
    }

    /// 当前端口。
    pub fn port(&self) -> u16 {
        self.port
    }

    /// 修改端口（写入 `data/port.txt`）。
    pub fn set_port(&mut self, port: u16) -> Result<String, String> {
        if !(1..=MAX_PORT).contains(&port) {
            return Err("端口号必须在 1-65534 之间（需要预留 HTTPS 端口 +1）".to_string());
        }
        if self.runtime_running() || self.probe.is_listening() {
            return Err("请先停止服务再修改端口".to_string());
        }

        self.port = port;
        self.probe.set_port(port);
        save_port(port)?;
        Ok(format!("端口已设置为 {port}"))
    }

    /// 服务是否在运行。
    ///
    /// 只读后台探测缓存，不会阻塞界面线程。
    pub fn is_running(&self) -> bool {
        self.runtime_running() || self.probe.is_listening()
    }

    /// 仅判断本进程启动的服务是否在运行（不查询端口）。
    pub fn runtime_running(&self) -> bool {
        self.runtime.as_ref().is_some_and(|r| r.is_running())
    }

    /// 阻塞式端口探测，仅在用户主动操作时使用。
    fn port_blocking_probe(&self) -> bool {
        is_port_listening(self.port)
    }

    /// 启动服务。
    pub fn start(&mut self) -> Result<String, String> {
        // 用户主动点击，这里做一次阻塞式探测，确保判断准确
        if self.runtime_running() || self.port_blocking_probe() {
            return Err("服务已在运行".to_string());
        }

        // 端口写进配置文件，后端启动时读取
        save_port(self.port)?;

        let runtime = server::spawn_server(self.port).map_err(|e| format!("启动失败: {e}"))?;
        self.runtime = Some(runtime);

        Ok(format!("服务已启动（端口 {}）", self.port))
    }

    /// 停止服务。
    pub fn stop(&mut self) -> Result<String, String> {
        // 是本程序启动的服务，直接停
        if let Some(runtime) = self.runtime.take() {
            runtime.stop();
            return Ok("服务已停止".to_string());
        }

        // 独立启动的（例如用 start.bat 启动的），按端口找到进程再结束
        if let Some(pid) = find_pid_on_port(self.port) {
            kill_pid(pid)?;
            return Ok(format!("服务已停止（PID {pid}）"));
        }

        Err("服务未运行".to_string())
    }

    /// 本机访问地址。
    pub fn local_url(&self) -> String {
        format!("http://localhost:{}", self.port)
    }

    /// 局域网访问地址。
    pub fn lan_url(&self) -> Option<String> {
        self.lan_ip().map(|ip| format!("http://{ip}:{}", self.port))
    }

    /// 本机局域网 IP（结果缓存，避免每次刷新都重新探测）。
    fn lan_ip(&self) -> Option<String> {
        /// 缓存项：(上次结果, 探测时间)
        type Cache = Option<(Option<String>, Instant)>;

        static CACHE: std::sync::OnceLock<Mutex<Cache>> = std::sync::OnceLock::new();
        let cache = CACHE.get_or_init(|| Mutex::new(None));

        let mut guard = cache.lock().ok()?;
        let now = Instant::now();

        // 10 分钟内复用上次结果
        if let Some((value, at)) = guard.as_ref() {
            if now.duration_since(*at) < Duration::from_secs(600) {
                return value.clone();
            }
        }

        let value = local_ip();
        *guard = Some((value.clone(), now));
        value
    }
}

impl Default for ServiceManager {
    fn default() -> Self {
        Self::new()
    }
}

/// 读取 `data/port.txt`。
pub fn load_port() -> u16 {
    std::fs::read_to_string(paths::port_file())
        .ok()
        .and_then(|text| text.trim().parse::<u16>().ok())
        .filter(|port| (1..=MAX_PORT).contains(port))
        .unwrap_or(DEFAULT_PORT)
}

/// 写入 `data/port.txt`。
pub fn save_port(port: u16) -> Result<(), String> {
    let path = paths::port_file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("无法创建数据目录: {e}"))?;
    }
    std::fs::write(&path, port.to_string()).map_err(|e| format!("无法写入端口配置: {e}"))
}

/// 端口上是否有进程在监听。
///
/// 连接本机未开放的端口会立刻被拒绝（ECONNREFUSED），
/// 只有被防火墙静默丢弃时才会等到超时，所以实际耗时很短。
pub fn is_port_listening(port: u16) -> bool {
    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    TcpStream::connect_timeout(&address, Duration::from_millis(300)).is_ok()
}

/// 本机局域网 IP。
pub fn local_ip() -> Option<String> {
    // UDP 的 connect 只设置默认对端，不会发出任何数据包
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;

    let ip = socket.local_addr().ok()?.ip();
    if ip.is_loopback() || ip.is_unspecified() {
        return None;
    }
    Some(ip.to_string())
}

/// 用 netstat 找到监听指定端口的进程 PID。
pub fn find_pid_on_port(port: u16) -> Option<u32> {
    let output = Command::new("netstat")
        .args(["-ano", "-p", "tcp"])
        .output()
        .ok()?;

    let text = String::from_utf8_lossy(&output.stdout);

    for line in text.lines() {
        if !line.contains("LISTENING") {
            continue;
        }

        let columns: Vec<&str> = line.split_whitespace().collect();
        if columns.len() < 5 {
            continue;
        }

        // 形如 "TCP  0.0.0.0:5001  0.0.0.0:0  LISTENING  1234"
        let local = columns[1];
        let Some(listening_port) = local.rsplit(':').next() else {
            continue;
        };
        if listening_port != port.to_string() {
            continue;
        }

        if let Ok(pid) = columns[4].parse::<u32>() {
            return Some(pid);
        }
    }

    None
}

/// 结束指定进程。
pub fn kill_pid(pid: u32) -> Result<(), String> {
    let output = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .output()
        .map_err(|e| format!("无法结束进程: {e}"))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// 在 Windows 防火墙中放行 HTTP / HTTPS 端口。
///
/// 需要管理员权限；行为与 Python 原版一致。
pub fn allow_firewall(port: u16) -> Result<String, String> {
    let https_port = port.saturating_add(1);

    for target in [port, https_port] {
        let name = format!("工资查询系统 {target}");

        // 先删掉同名旧规则，避免重复点击堆积规则
        let _ = Command::new("netsh")
            .args([
                "advfirewall",
                "firewall",
                "delete",
                "rule",
                &format!("name={name}"),
            ])
            .output();

        let output = Command::new("netsh")
            .args([
                "advfirewall",
                "firewall",
                "add",
                "rule",
                &format!("name={name}"),
                "dir=in",
                "action=allow",
                "protocol=TCP",
                &format!("localport={target}"),
            ])
            .output()
            .map_err(|e| format!("无法执行 netsh: {e}"))?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(format!(
                "netsh 返回错误（端口 {target}）：\n{error}\n\n\
                 请以管理员身份运行本程序后重试，\n\
                 或手动在防火墙中放行端口 {port} 和 {https_port}。"
            ));
        }
    }

    Ok(format!(
        "已在 Windows 防火墙放行端口 {port}（HTTP）和 {https_port}（HTTPS）。\n局域网内其他电脑现在可以访问本系统。"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlistened_port_is_not_running() {
        // 1 号端口一般不会被占用
        assert!(!is_port_listening(1));
    }

    #[test]
    fn port_range_is_validated() {
        let mut manager = ServiceManager::new();
        assert!(manager.set_port(0).is_err());
        assert!(manager.set_port(65535).is_err());
    }
}
