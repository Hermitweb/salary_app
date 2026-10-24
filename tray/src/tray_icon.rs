//! 系统托盘图标。
//!
//! 菜单项：显示面板 / 启动服务 / 停止服务 / 退出（与 Python 原版一致）。
//! 事件通过通道回传给界面线程处理。
//!
//! 图标本身来自 [`crate::brand`]，和侧边栏 logo、标题栏图标是同一个图形；
//! 主题切换时界面会发来新颜色，托盘图标跟着换。

use crate::brand;
use crate::theme::Theme;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::Duration;
use tray_icon::menu::{Menu, MenuItem};
use tray_icon::{Icon, TrayIconBuilder, TrayIconEvent};
use winapi::um::winuser::{DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage};

/// 托盘图标边长（像素）。
const ICON_SIZE: u32 = 32;

/// 等待主题消息的间隔。
///
/// 这段时间里要把线程消息队列泵一遍：tray-icon 的隐藏窗口就建在这个线程上，
/// 它的窗口过程也跑在这个线程上，不泵消息的话图标点了没反应、右键菜单也弹不出来。
const POLL: Duration = Duration::from_millis(30);

/// 把线程消息队列里的窗口消息取出来分发掉。
fn pump_messages() {
    let mut message: MSG = unsafe { std::mem::zeroed() };

    while unsafe { PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) } != 0 {
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

/// 托盘菜单触发的命令。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    /// 显示控制面板
    ShowPanel,
    /// 启动服务
    StartService,
    /// 停止服务
    StopService,
    /// 退出程序
    Quit,
}

/// 生成托盘图标：与标题栏、任务栏、侧边栏 logo 同一张图。
///
/// 直接用原版的 `app_icon.ico`（内嵌在 exe 里），不再自己画一个，
/// 免得同一个应用出现两套图标。
fn build_icon(theme: Theme) -> Vec<u8> {
    brand::tray_rgba(brand::module_handle())
        .map(|(pixels, _)| pixels)
        .unwrap_or_else(|| brand::fallback_rgba(ICON_SIZE, brand::color_of(theme)))
}

/// 创建托盘图标与菜单。
///
/// 菜单点击通过 `sender` 回传；函数不返回，托盘图标会一直存活。
pub fn spawn(sender: Sender<TrayCommand>, theme_receiver: Receiver<Theme>) -> Result<(), String> {
    let menu = Menu::new();

    let show_item = MenuItem::new("显示面板", true, None);
    let start_item = MenuItem::new("启动服务", true, None);
    let stop_item = MenuItem::new("停止服务", true, None);
    let quit_item = MenuItem::new("退出", true, None);

    menu.append_items(&[&show_item, &start_item, &stop_item, &quit_item])
        .map_err(|e| format!("无法创建托盘菜单: {e}"))?;

    let show_id = show_item.id().clone();
    let start_id = start_item.id().clone();
    let stop_id = stop_item.id().clone();
    let quit_id = quit_item.id().clone();

    let menu_sender = sender.clone();
    tray_icon::menu::MenuEvent::set_event_handler(Some(
        move |event: tray_icon::menu::MenuEvent| {
            let command = if event.id() == &show_id {
                Some(TrayCommand::ShowPanel)
            } else if event.id() == &start_id {
                Some(TrayCommand::StartService)
            } else if event.id() == &stop_id {
                Some(TrayCommand::StopService)
            } else if event.id() == &quit_id {
                Some(TrayCommand::Quit)
            } else {
                None
            };

            if let Some(command) = command {
                let _ = menu_sender.send(command);
            }
        },
    ));

    // 左键点击托盘图标等同于「显示面板」
    let click_sender = sender.clone();
    TrayIconEvent::set_event_handler(Some(move |event| {
        if let TrayIconEvent::Click {
            button: tray_icon::MouseButton::Left,
            button_state: tray_icon::MouseButtonState::Up,
            ..
        } = event
        {
            let _ = click_sender.send(TrayCommand::ShowPanel);
        }
    }));

    let icon = Icon::from_rgba(build_icon(Theme::Dark), ICON_SIZE, ICON_SIZE)
        .map_err(|e| format!("无法生成托盘图标: {e}"))?;

    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_icon(icon)
        .with_tooltip("员工工资查询系统")
        .with_menu_on_left_click(false)
        .build()
        .map_err(|e| format!("无法创建托盘图标: {e}"))?;

    // 主题切换时界面会发来新颜色，托盘图标跟着换，
    // 这样托盘、标题栏、任务栏、侧边栏 logo 始终是同一个图形。
    //
    // 托盘图标只能在创建它的线程里用，所以这里就守在当前线程上等消息，
    // 顺带保证图标一直活着。等待期间必须继续泵窗口消息：
    // 图标的隐藏窗口与它的窗口过程都挂在当前线程上，
    // 只阻塞等主题消息的话，点击与右键菜单都不会有人处理。
    loop {
        match theme_receiver.recv_timeout(POLL) {
            Ok(theme) => {
                let icon = Icon::from_rgba(build_icon(theme), ICON_SIZE, ICON_SIZE);
                let _ = tray.set_icon(icon.ok());
            }
            Err(RecvTimeoutError::Timeout) => {}
            // 界面那边退出了，托盘图标跟着收工
            Err(RecvTimeoutError::Disconnected) => break,
        }

        pump_messages();
    }

    Ok(())
}
