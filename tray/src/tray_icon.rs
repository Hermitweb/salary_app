//! 系统托盘图标。
//!
//! 菜单项：显示面板 / 启动服务 / 停止服务 / 退出（与 Python 原版一致）。
//! 事件通过通道回传给界面线程处理。

use std::sync::mpsc::Sender;
use tray_icon::menu::{Menu, MenuItem};
use tray_icon::{Icon, TrayIconBuilder, TrayIconEvent};

/// 托盘图标边长（像素）。
const ICON_SIZE: u32 = 32;

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

/// 距离点到线段的距离（用于画笔画）。
fn distance_to_segment(px: f32, py: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let length_squared = dx * dx + dy * dy;

    if length_squared == 0.0 {
        return ((px - x1).powi(2) + (py - y1).powi(2)).sqrt();
    }

    let t = (((px - x1) * dx + (py - y1) * dy) / length_squared).clamp(0.0, 1.0);
    let closest_x = x1 + t * dx;
    let closest_y = y1 + t * dy;

    ((px - closest_x).powi(2) + (py - closest_y).powi(2)).sqrt()
}

/// 生成托盘图标：蓝色圆底 + 白色 ¥（与 Python 原版一致）。
fn build_icon() -> Vec<u8> {
    let size = ICON_SIZE as i32;
    let center = (size as f32 - 1.0) / 2.0;
    let radius = size as f32 * 0.46;

    // ¥ 的笔画：两条斜线 + 竖线 + 两条横线
    let strokes: [((f32, f32), (f32, f32)); 5] = [
        ((11.0, 8.0), (16.0, 14.5)),
        ((21.0, 8.0), (16.0, 14.5)),
        ((16.0, 14.5), (16.0, 24.0)),
        ((11.5, 17.5), (20.5, 17.5)),
        ((12.5, 20.5), (19.5, 20.5)),
    ];

    let mut rgba = vec![0u8; (size * size * 4) as usize];

    for y in 0..size {
        for x in 0..size {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;

            let dx = px - center;
            let dy = py - center;
            if dx * dx + dy * dy > radius * radius {
                continue;
            }

            let on_stroke = strokes
                .iter()
                .any(|((x1, y1), (x2, y2))| distance_to_segment(px, py, *x1, *y1, *x2, *y2) <= 1.1);

            let index = ((y * size + x) * 4) as usize;

            if on_stroke {
                rgba[index] = 0xff;
                rgba[index + 1] = 0xff;
                rgba[index + 2] = 0xff;
            } else {
                rgba[index] = 0x3b;
                rgba[index + 1] = 0x82;
                rgba[index + 2] = 0xf6;
            }
            rgba[index + 3] = 0xff;
        }
    }

    rgba
}

/// 创建托盘图标与菜单。
///
/// 菜单点击通过 `sender` 回传；函数不返回，托盘图标会一直存活。
pub fn spawn(sender: Sender<TrayCommand>) -> Result<(), String> {
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

    let icon = Icon::from_rgba(build_icon(), ICON_SIZE, ICON_SIZE)
        .map_err(|e| format!("无法生成托盘图标: {e}"))?;

    let _tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_icon(icon)
        .with_tooltip("员工工资查询系统")
        .with_menu_on_left_click(false)
        .build()
        .map_err(|e| format!("无法创建托盘图标: {e}"))?;

    // 托盘图标必须一直存活
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}
