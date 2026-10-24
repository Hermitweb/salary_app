//! 员工工资查询系统 —— 托盘控制面板。
//!
//! 启动后系统托盘出现一个图标，右键菜单可以显示面板、启停服务、退出程序。
//! 控制面板有四个页面：
//! - 「服务控制」：运行状态、访问地址、账号密码设置、端口、启停、开机自启；
//! - 「字段显示」：工资条字段的三态显示策略；
//! - 「使用手册」：面板内自带的使用说明；
//! - 「关于应用」：项目介绍、运行环境、审计日志入口与项目地址。
//!
//! 服务直接跑在本进程内（独立线程），因此启停都不需要拉起子进程。

// 托盘程序是纯图形程序：不要弹出黑色控制台窗口。
// 出错时用消息框告诉用户，而不是把错误写进没人看的标准错误。
#![windows_subsystem = "windows"]

use salary_tray::{panel, service, tray_icon};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;

fn main() {
    if let Err(error) = run() {
        show_error(&error);
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    native_windows_gui::init().map_err(|e| format!("无法初始化 Windows 图形界面: {e}"))?;

    // 托盘命令通道
    let (sender, receiver) = mpsc::channel::<tray_icon::TrayCommand>();

    // 主题通道：界面换主题时通知托盘图标换色
    let (theme_sender, theme_receiver) = mpsc::channel::<salary_tray::theme::Theme>();

    // 托盘图标在独立线程里常驻
    std::thread::spawn(move || {
        if let Err(error) = tray_icon::spawn(sender, theme_receiver) {
            show_error(&format!("托盘图标创建失败: {error}"));
        }
    });

    // 服务管理器由面板与托盘命令共用
    let service = Rc::new(RefCell::new(service::ServiceManager::new()));

    let panel = panel::build(service, receiver, theme_sender)
        .map_err(|e| format!("无法创建控制面板: {e}"))?;

    native_windows_gui::dispatch_thread_events();

    // 面板资源要一直存活
    drop(panel);

    Ok(())
}

/// 弹出错误提示。
///
/// 程序是图形子系统的，标准错误没有出口，
/// 所以启动失败必须让用户看得见。
fn show_error(message: &str) {
    use winapi::um::winuser::{MB_ICONERROR, MB_OK, MessageBoxW};

    let title: Vec<u16> = "员工工资查询系统".encode_utf16().chain(Some(0)).collect();
    let text: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();

    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}
