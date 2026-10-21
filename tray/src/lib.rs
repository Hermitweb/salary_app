//! 托盘控制面板的可复用部分。
//!
//! - [`tray_icon`]：系统托盘图标与菜单；
//! - [`panel`]：控制面板窗口；
//! - [`service`]：服务启停、端口与状态探测；
//! - [`fields`]：动态字段发现与 `fields.txt` 读写。

pub mod fields;
pub mod panel;
pub mod service;
pub mod solid;
pub mod text;
pub mod theme;
pub mod tray_icon;
