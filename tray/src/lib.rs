//! 托盘控制面板的可复用部分。
//!
//! - [`tray_icon`]：系统托盘图标与菜单；
//! - [`panel`]：控制面板窗口；
//! - [`canvas`]：自绘的界面画布（卡片、按钮、列表行都在这里画）；
//! - [`brand`]：品牌标记（logo、标题栏、任务栏、托盘共用同一个图形）；
//! - [`clipboard`]：剪贴板（复制访问地址）；
//! - [`networks`]：支持访问的网络配置；
//! - [`paint`]：GDI 绘制原语；
//! - [`theme`]：配色、字体与尺寸；
//! - [`service`]：服务启停、端口与状态探测；
//! - [`fields`]：动态字段发现与 `fields.txt` 读写；
//! - [`credentials`]：网页登录的字段映射（账号列 / 密码列）。

// 绘制函数拿到的 DC 全部来自自己窗口过程里的 BeginPaint，
// 不会把裸指针交给外部调用方，因此不用按 unsafe API 暴露。
#![allow(clippy::not_unsafe_ptr_arg_deref)]

pub mod brand;
pub mod canvas;
pub mod clipboard;
pub mod credentials;
pub mod fields;
pub mod networks;
pub mod paint;
pub mod panel;
pub mod service;
pub mod theme;
pub mod tray_icon;
