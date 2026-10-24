//! 服务管理：启动、停止、状态探测与端口配置。
//!
//! 服务直接跑在托盘进程内（独立线程 + 独立 actix 运行时），
//! 因此启动/停止都不需要拉起子进程。
//!
//! 状态探测用 TCP 连接判断端口是否在监听，**不发起任何 HTTP 请求**。
//! 端口探测最多会阻塞数百毫秒，因此放在后台线程做，
//! 界面线程只读缓存结果——否则窗口会被判定为「未响应」。

use salary_app_backend::port;
use salary_app_backend::server::{self, ServerRuntime};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// 默认端口（与后端一致）。
pub const DEFAULT_PORT: u16 = port::DEFAULT_PORT;

/// 端口号上限：需要给 HTTPS 预留 +1。
const MAX_PORT: u16 = port::MAX_PORT;

/// 后台端口探测的间隔。
const PROBE_INTERVAL: Duration = Duration::from_secs(2);

/// 自动重启的计数窗口。
///
/// 窗口内重启次数超过上限就不再自动拉起：
/// 反复起不来说明是配置或数据的问题，继续重启只会刷屏。
const RESTART_WINDOW: Duration = Duration::from_secs(600);

/// 一个窗口内最多自动重启几次。
const MAX_RESTART_COUNT: u32 = 5;

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
    /// 服务「本该在跑」——用户点了启动才是真，停掉就清掉。
    expected_running: bool,
    /// 窗口内自动重启的次数。
    restart_count: u32,
    /// 上次自动重启的时间。
    last_restart: Option<Instant>,
}

impl ServiceManager {
    /// 创建管理器并读取已配置的端口。
    pub fn new() -> Self {
        let port = load_port();
        Self {
            runtime: None,
            port,
            probe: PortProbe::spawn(port),
            expected_running: false,
            restart_count: 0,
            last_restart: None,
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
        self.expected_running = true;
        // 手动启动算「一切正常」，把自动重启的计数清零
        self.restart_count = 0;
        self.last_restart = None;

        Ok(format!("服务已启动（端口 {}）", self.port))
    }

    /// 停止服务。
    pub fn stop(&mut self) -> Result<String, String> {
        self.expected_running = false;

        // 是本程序启动的服务，直接停
        if let Some(runtime) = self.runtime.take() {
            runtime.stop();
            return Ok("服务已停止".to_string());
        }

        // 独立启动的（例如直接运行过后端 exe），按端口找到进程再结束
        if let Some(pid) = find_pid_on_port(self.port) {
            // 端口上可能是别的程序，先确认身份再结束
            let Some(name) = process_name(pid) else {
                return Err(format!(
                    "无法确认端口 {} 上进程的身份，未执行结束操作。",
                    self.port
                ));
            };

            if !is_our_process(&name) {
                return Err(format!(
                    "端口 {} 上是「{name}」，不是本系统的服务，没有结束它。\n\
                     如果确实需要结束该进程，请手动处理。",
                    self.port
                ));
            }

            kill_pid(pid)?;
            return Ok(format!("服务已停止（PID {pid}）"));
        }

        Err("服务未运行".to_string())
    }

    /// 本机访问地址。
    pub fn local_url(&self) -> String {
        format!("http://localhost:{}", self.port)
    }
    /// 窗口内自动重启过几次。
    pub fn restart_count(&self) -> u32 {
        self.restart_count
    }

    /// 服务是不是「本该在跑却掉线了」。
    ///
    /// 判据：用户开着服务、运行时已经不在、端口也没人在听。
    /// 独立启动的服务（直接运行后端 exe 起的）端口还在听，不算掉线。
    pub fn dropped(&self) -> bool {
        if !self.expected_running {
            return false;
        }
        if self.runtime_running() {
            return false;
        }
        !self.probe.is_listening()
    }

    /// 把掉线的服务重新拉起来。
    pub fn restart(&mut self) -> Result<String, String> {
        let now = Instant::now();
        if let Some(last) = self.last_restart {
            if now.duration_since(last) > RESTART_WINDOW {
                // 新的故障周期，计数从头来
                self.restart_count = 0;
            }
        }
        self.restart_count += 1;
        self.last_restart = Some(now);

        if self.restart_count > MAX_RESTART_COUNT {
            self.expected_running = false;
            return Err(format!(
                "服务连续掉线 {} 次，已停止自动重启，请检查端口占用或数据目录权限",
                self.restart_count
            ));
        }

        self.runtime = None;
        let runtime = server::spawn_server(self.port).map_err(|e| format!("自动重启失败: {e}"))?;
        self.runtime = Some(runtime);
        self.expected_running = true;

        Ok(format!(
            "服务异常退出，已自动恢复（第 {} 次）",
            self.restart_count
        ))
    }
    /// 局域网访问地址。
    pub fn lan_url(&self) -> Option<String> {
        self.lan_ip().map(|ip| format!("http://{ip}:{}", self.port))
    }

    /// 局域网 HTTPS 访问地址（端口 +1）。
    pub fn lan_https_url(&self) -> Option<String> {
        self.lan_ip()
            .map(|ip| format!("https://{ip}:{}", self.port.saturating_add(1)))
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
    port::read_from_file().unwrap_or(DEFAULT_PORT)
}

/// 写入 `data/port.txt`。
pub fn save_port(port: u16) -> Result<(), String> {
    port::write_to_file(port).map_err(|e| format!("无法写入端口配置: {e}"))
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

/// netstat 里表示「正在监听」的状态取值。
///
/// 状态列是本地化的：英文系统是 `LISTENING`，简体中文系统是「侦听」。
/// 两种都认，否则在中文 Windows 上永远找不到占用端口的进程。
const LISTENING_STATES: &[&str] = &["listening", "侦听", "侦听中"];

/// 判断 netstat 的状态列是否表示「正在监听」。
fn is_listening_state(state: &str) -> bool {
    let state = state.trim().to_lowercase();
    LISTENING_STATES.iter().any(|known| state == *known)
}

/// 用 netstat 找到监听指定端口的进程 PID。
pub fn find_pid_on_port(port: u16) -> Option<u32> {
    let output = Command::new("netstat")
        .args(["-ano", "-p", "tcp"])
        .output()
        .ok()?;

    let text = String::from_utf8_lossy(&output.stdout);

    for line in text.lines() {
        let columns: Vec<&str> = line.split_whitespace().collect();
        if columns.len() < 5 {
            continue;
        }

        // 形如 "TCP  0.0.0.0:5001  0.0.0.0:0  LISTENING  1234"
        if !is_listening_state(columns[3]) {
            continue;
        }

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

/// 查询进程名（可执行文件名）。
pub fn process_name(pid: u32) -> Option<String> {
    let output = Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
        .output()
        .ok()?;

    let text = String::from_utf8_lossy(&output.stdout);

    // CSV 形如 "salary-app-backend.exe","1234","Console","1","12,345 K"
    let line = text.lines().find(|line| !line.trim().is_empty())?;
    let name = line.split(',').next()?.trim().trim_matches('"');

    // 进程不存在时 tasklist 输出一行 INFO 提示
    if name.is_empty() || name.starts_with("INFO") {
        return None;
    }

    Some(name.to_string())
}

/// 进程是否属于本系统。
///
/// 托盘停止服务时会结束占用端口的进程——但那个端口上完全可能是别的程序。
/// 只结束名字里带 salary 的进程，避免误杀无关程序。
pub fn is_our_process(image_name: &str) -> bool {
    image_name.to_lowercase().contains("salary")
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

    #[test]
    fn listening_state_is_recognized_in_both_languages() {
        // 英文系统
        assert!(is_listening_state("LISTENING"));
        assert!(is_listening_state("listening"));
        // 简体中文系统
        assert!(is_listening_state("侦听"));

        assert!(!is_listening_state("ESTABLISHED"));
        assert!(!is_listening_state("TIME_WAIT"));
        assert!(!is_listening_state(""));
    }

    #[test]
    fn only_our_own_processes_may_be_killed() {
        assert!(is_our_process("salary-app-backend.exe"));
        assert!(is_our_process("Salary-Tray.exe"));
        assert!(is_our_process("SALARY-APP-BACKEND.EXE"));

        assert!(!is_our_process("nginx.exe"));
        assert!(!is_our_process("chrome.exe"));
        assert!(!is_our_process("System"));
    }

    #[test]
    fn finds_the_pid_of_a_listening_socket() {
        use std::net::TcpListener;

        // 自己监听一个端口，netstat 应当能查到本进程的 PID
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("应当能监听");
        let port = listener.local_addr().expect("应当能取到端口").port();

        // netstat 有极小的延迟，重试几次
        let mut found = None;
        for _ in 0..10 {
            found = find_pid_on_port(port);
            if found.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        assert_eq!(found, Some(std::process::id()));
    }

    #[test]
    fn process_name_of_the_current_process() {
        let name = process_name(std::process::id()).expect("应当能查到本进程名");
        assert!(!name.is_empty());
        assert!(!name.starts_with("INFO"));
    }
}
