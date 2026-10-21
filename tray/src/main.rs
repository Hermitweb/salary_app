//! 员工工资查询系统 —— 托盘控制面板。
//!
//! 启动后系统托盘出现一个图标，右键菜单可以显示面板、启停服务、退出程序。
//! 控制面板是两个标签页：
//! - 「服务」：运行状态、访问地址、端口设置、启停、开机自启；
//! - 「字段显示」：工资条字段的三态显示策略。
//!
//! 服务直接跑在本进程内（独立线程），因此启停都不需要拉起子进程。

use salary_tray::{panel, service, tray_icon};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;

fn main() {
    native_windows_gui::init().expect("无法初始化 Windows 图形界面");
    let _ = native_windows_gui::Font::set_global_family("Microsoft YaHei UI");

    // 托盘命令通道
    let (sender, receiver) = mpsc::channel::<tray_icon::TrayCommand>();

    // 托盘图标在独立线程里常驻
    std::thread::spawn(move || {
        if let Err(error) = tray_icon::spawn(sender) {
            eprintln!("托盘图标创建失败: {error}");
        }
    });

    // 服务管理器由面板与托盘命令共用
    let service = Rc::new(RefCell::new(service::ServiceManager::new()));

    let panel = panel::build(service, receiver).expect("无法创建控制面板");

    native_windows_gui::dispatch_thread_events();

    // 面板资源要一直存活
    drop(panel);
}
