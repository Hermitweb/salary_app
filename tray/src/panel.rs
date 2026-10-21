//! 控制面板窗口。
//!
//! 顶部是标题横幅，下面是两个标签页：
//! - 「服务」：状态、访问地址、端口、启停、开机自启、防火墙；
//! - 「字段显示」：工资条字段的三态显示策略（显示 / 跟随 / 隐藏）。
//!
//! 配色与字体集中在 [`crate::theme`]，与网页端保持同一套视觉语言。
//!
//! 绘制方式：底色用 [`crate::solid`] 的自绘色块，文字用
//! [`crate::text`] 的自绘文字控件——两者都是自己画的，
//! 不会出现系统 STATIC 控件那种「文字压在一块白底上」的割裂感。

use crate::fields;
use crate::service::ServiceManager;
use crate::solid::SolidBlock;
use crate::text::TextBlock;
use crate::theme::{self, Fonts};
use crate::tray_icon::TrayCommand;
use native_windows_gui as nwg;
use salary_app_backend::models::FieldState;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::mpsc::Receiver;

/// 面板状态。
struct PanelState {
    service: Rc<RefCell<ServiceManager>>,
    /// 字段名 -> 显示策略
    config: HashMap<String, FieldState>,
    /// 字段列表（按发现顺序）
    names: Vec<String>,
    /// 字段列表里选中的行
    selected: Option<usize>,
}

/// 面板资源。
///
/// 窗口、标签页、字体与所有自绘控件都必须与控件同寿命——
/// 任何被提前 drop 的控件都会销毁底层窗口。
pub struct Panel {
    /// 窗口需要一直存活，字段名加下划线表示「只为了持有」。
    _window: Rc<nwg::Window>,
    _tabs: nwg::TabsContainer,
    _service_tab: nwg::Tab,
    _fields_tab: nwg::Tab,
    _fonts: Fonts,
    _banner: SolidBlock,
    _running_block: Rc<SolidBlock>,
    _stopped_block: Rc<SolidBlock>,
    _service_card: SolidBlock,
    _fields_card: SolidBlock,
    _banner_text: TextBlock,
    _subtitle: TextBlock,
    _running_pill: Rc<TextBlock>,
    _stopped_pill: Rc<TextBlock>,
    _service_title: TextBlock,
    _local_title: TextBlock,
    _local_label: Rc<TextBlock>,
    _lan_title: TextBlock,
    _lan_label: Rc<TextBlock>,
    _port_title: TextBlock,
    _service_divider: SolidBlock,
    _service_actions: TextBlock,
    _fields_title: TextBlock,
    _fields_hint: TextBlock,
    _selected_label: Rc<TextBlock>,
    _status_bar: nwg::StatusBar,
    _timer: nwg::AnimationTimer,
}

/// 启动控制面板窗口。
pub fn build(
    service: Rc<RefCell<ServiceManager>>,
    commands: Receiver<TrayCommand>,
) -> Result<Panel, String> {
    let mut window = Default::default();
    let mut tabs = Default::default();
    let mut service_tab = Default::default();
    let mut fields_tab = Default::default();
    let mut status_bar = Default::default();
    let mut timer = Default::default();

    let fonts = Fonts::new();

    let state = Rc::new(RefCell::new(PanelState {
        service: service.clone(),
        config: fields::load_config(),
        names: Vec::new(),
        selected: None,
    }));

    nwg::Window::builder()
        .size((800, 560))
        .position((460, 220))
        .title("员工工资查询系统 - 控制面板")
        .build(&mut window)
        .map_err(|e| format!("无法创建窗口: {e}"))?;

    let window_handle = window.handle;

    // ---------------- 标题横幅 ----------------

    let banner = SolidBlock::create(window_handle, 0, 0, 800, 68, theme::PRIMARY_SOFT)
        .map_err(|e| format!("无法创建横幅: {e}"))?;

    let banner_text = TextBlock::create(
        window_handle,
        22,
        8,
        460,
        30,
        theme::PRIMARY_SOFT,
        theme::INK,
        "员工工资查询系统",
        fonts.title(),
        false,
    )
    .map_err(|e| format!("无法创建标题: {e}"))?;

    let subtitle = TextBlock::create(
        window_handle,
        22,
        40,
        460,
        22,
        theme::PRIMARY_SOFT,
        theme::INK_MUTED,
        "托盘控制面板 · 管理工资查询服务",
        fonts.small(),
        false,
    )
    .map_err(|e| format!("无法创建副标题: {e}"))?;

    // 运行状态：两套「色块 + 文字」一起切换
    let running_block = Rc::new(
        SolidBlock::create(window_handle, 636, 16, 140, 36, theme::SUCCESS_SOFT)
            .map_err(|e| format!("无法创建状态色块: {e}"))?,
    );

    let running_pill = Rc::new(
        TextBlock::create(
            window_handle,
            636,
            16,
            140,
            36,
            theme::SUCCESS_SOFT,
            theme::SUCCESS,
            "● 运行中",
            fonts.pill(),
            true,
        )
        .map_err(|e| format!("无法创建状态文字: {e}"))?,
    );

    let stopped_block = Rc::new(
        SolidBlock::create(window_handle, 636, 16, 140, 36, theme::DANGER_SOFT)
            .map_err(|e| format!("无法创建状态色块: {e}"))?,
    );

    let stopped_pill = Rc::new(
        TextBlock::create(
            window_handle,
            636,
            16,
            140,
            36,
            theme::DANGER_SOFT,
            theme::DANGER,
            "● 已停止",
            fonts.pill(),
            true,
        )
        .map_err(|e| format!("无法创建状态文字: {e}"))?,
    );

    // ---------------- 标签页 ----------------

    nwg::TabsContainer::builder()
        .parent(&window)
        .position((0, 68))
        .size((800, 468))
        .build(&mut tabs)
        .map_err(|e| format!("无法创建标签页: {e}"))?;

    nwg::Tab::builder()
        .text("服务")
        .parent(&tabs)
        .build(&mut service_tab)
        .map_err(|e| format!("无法创建服务标签页: {e}"))?;

    nwg::Tab::builder()
        .text("字段显示")
        .parent(&tabs)
        .build(&mut fields_tab)
        .map_err(|e| format!("无法创建字段标签页: {e}"))?;

    let service_tab_handle = service_tab.handle;
    let fields_tab_handle = fields_tab.handle;

    // ---------------- 服务标签页 ----------------

    let service_card = SolidBlock::create(service_tab_handle, 10, 10, 770, 330, theme::MUTED_SOFT)
        .map_err(|e| format!("无法创建卡片: {e}"))?;

    let service_title = TextBlock::create(
        service_tab_handle,
        30,
        24,
        200,
        28,
        theme::MUTED_SOFT,
        theme::INK,
        "访问地址",
        fonts.section(),
        false,
    )
    .map_err(|e| format!("无法创建标题: {e}"))?;

    let local_title = TextBlock::create(
        service_tab_handle,
        30,
        70,
        100,
        26,
        theme::MUTED_SOFT,
        theme::INK_MUTED,
        "本机访问",
        fonts.body(),
        false,
    )
    .map_err(|e| format!("无法创建标签: {e}"))?;

    let local_label = Rc::new(
        TextBlock::create(
            service_tab_handle,
            140,
            70,
            460,
            26,
            theme::MUTED_SOFT,
            theme::INK,
            "",
            fonts.mono(),
            false,
        )
        .map_err(|e| format!("无法创建标签: {e}"))?,
    );

    let lan_title = TextBlock::create(
        service_tab_handle,
        30,
        108,
        100,
        26,
        theme::MUTED_SOFT,
        theme::INK_MUTED,
        "局域网访问",
        fonts.body(),
        false,
    )
    .map_err(|e| format!("无法创建标签: {e}"))?;

    let lan_label = Rc::new(
        TextBlock::create(
            service_tab_handle,
            140,
            108,
            460,
            26,
            theme::MUTED_SOFT,
            theme::INK,
            "",
            fonts.mono(),
            false,
        )
        .map_err(|e| format!("无法创建标签: {e}"))?,
    );

    let port_title = TextBlock::create(
        service_tab_handle,
        30,
        150,
        100,
        28,
        theme::MUTED_SOFT,
        theme::INK_MUTED,
        "端口",
        fonts.body(),
        false,
    )
    .map_err(|e| format!("无法创建标签: {e}"))?;

    let mut port_input = Default::default();
    nwg::TextInput::builder()
        .text(&service.borrow().port().to_string())
        .size((96, 28))
        .position((140, 148))
        .font(Some(fonts.mono_ref()))
        .parent(&service_tab)
        .build(&mut port_input)
        .map_err(|e| format!("无法创建端口输入框: {e}"))?;

    let mut apply_button = Default::default();
    nwg::Button::builder()
        .text("应用端口")
        .size((104, 30))
        .position((246, 147))
        .parent(&service_tab)
        .build(&mut apply_button)
        .map_err(|e| format!("无法创建按钮: {e}"))?;

    // 分隔线
    let service_divider = SolidBlock::create(service_tab_handle, 30, 196, 730, 1, theme::HAIRLINE)
        .map_err(|e| format!("无法创建分隔线: {e}"))?;

    let service_actions = TextBlock::create(
        service_tab_handle,
        30,
        208,
        200,
        28,
        theme::MUTED_SOFT,
        theme::INK,
        "快捷操作",
        fonts.section(),
        false,
    )
    .map_err(|e| format!("无法创建标题: {e}"))?;

    let mut start_button = Default::default();
    nwg::Button::builder()
        .text("启动服务")
        .size((118, 34))
        .position((30, 248))
        .parent(&service_tab)
        .build(&mut start_button)
        .map_err(|e| format!("无法创建按钮: {e}"))?;

    let mut stop_button = Default::default();
    nwg::Button::builder()
        .text("停止服务")
        .size((118, 34))
        .position((160, 248))
        .parent(&service_tab)
        .build(&mut stop_button)
        .map_err(|e| format!("无法创建按钮: {e}"))?;

    let mut open_button = Default::default();
    nwg::Button::builder()
        .text("打开面板")
        .size((118, 34))
        .position((290, 248))
        .parent(&service_tab)
        .build(&mut open_button)
        .map_err(|e| format!("无法创建按钮: {e}"))?;

    let mut data_button = Default::default();
    nwg::Button::builder()
        .text("打开数据目录")
        .size((130, 34))
        .position((420, 248))
        .parent(&service_tab)
        .build(&mut data_button)
        .map_err(|e| format!("无法创建按钮: {e}"))?;

    let mut autostart_button = Default::default();
    nwg::Button::builder()
        .text("开机自启：关")
        .size((148, 34))
        .position((30, 292))
        .parent(&service_tab)
        .build(&mut autostart_button)
        .map_err(|e| format!("无法创建按钮: {e}"))?;

    let mut firewall_button = Default::default();
    nwg::Button::builder()
        .text("允许防火墙")
        .size((130, 34))
        .position((190, 292))
        .parent(&service_tab)
        .build(&mut firewall_button)
        .map_err(|e| format!("无法创建按钮: {e}"))?;

    // ---------------- 字段标签页 ----------------

    let fields_card = SolidBlock::create(fields_tab_handle, 10, 10, 770, 448, theme::MUTED_SOFT)
        .map_err(|e| format!("无法创建卡片: {e}"))?;

    let fields_title = TextBlock::create(
        fields_tab_handle,
        30,
        26,
        200,
        30,
        theme::MUTED_SOFT,
        theme::INK,
        "字段显示",
        fonts.section(),
        false,
    )
    .map_err(|e| format!("无法创建标题: {e}"))?;

    let fields_hint = TextBlock::create(
        fields_tab_handle,
        30,
        66,
        720,
        24,
        theme::MUTED_SOFT,
        theme::INK_MUTED,
        "选择字段后点击「切换状态」在 显示 → 跟随 → 隐藏 之间循环，保存后立即对工资条生效",
        fonts.small(),
        false,
    )
    .map_err(|e| format!("无法创建提示: {e}"))?;

    let mut fields_list = Default::default();
    nwg::ListView::builder()
        .parent(&fields_tab)
        .position((30, 102))
        .size((730, 258))
        .list_style(nwg::ListViewStyle::Detailed)
        .flags(
            nwg::ListViewFlags::VISIBLE
                | nwg::ListViewFlags::SINGLE_SELECTION
                | nwg::ListViewFlags::ALWAYS_SHOW_SELECTION,
        )
        .background_color(theme::SURFACE)
        .text_color(theme::INK)
        .build(&mut fields_list)
        .map_err(|e| format!("无法创建字段列表: {e}"))?;

    let mut cycle_button = Default::default();
    nwg::Button::builder()
        .text("切换状态")
        .size((118, 34))
        .position((30, 378))
        .parent(&fields_tab)
        .build(&mut cycle_button)
        .map_err(|e| format!("无法创建按钮: {e}"))?;

    let mut save_button = Default::default();
    nwg::Button::builder()
        .text("保存")
        .size((98, 34))
        .position((160, 378))
        .parent(&fields_tab)
        .build(&mut save_button)
        .map_err(|e| format!("无法创建按钮: {e}"))?;

    let mut refresh_button = Default::default();
    nwg::Button::builder()
        .text("重新扫描")
        .size((118, 34))
        .position((270, 378))
        .parent(&fields_tab)
        .build(&mut refresh_button)
        .map_err(|e| format!("无法创建按钮: {e}"))?;

    let selected_label = Rc::new(
        TextBlock::create(
            fields_tab_handle,
            402,
            380,
            380,
            30,
            theme::MUTED_SOFT,
            theme::INK_MUTED,
            "未选择字段",
            fonts.small(),
            false,
        )
        .map_err(|e| format!("无法创建状态文字: {e}"))?,
    );

    // ---------------- 状态栏与定时刷新 ----------------

    nwg::StatusBar::builder()
        .text("就绪")
        .parent(&window)
        .build(&mut status_bar)
        .map_err(|e| format!("无法创建状态栏: {e}"))?;

    nwg::AnimationTimer::builder()
        .interval(std::time::Duration::from_millis(500))
        .parent(&window)
        .build(&mut timer)
        .map_err(|e| format!("无法创建定时器: {e}"))?;

    // 定时器要显式启动，用于刷新状态与处理托盘命令
    timer.start();

    // ---------------- 初始填充 ----------------

    refresh_fields(&state, &fields_list, &selected_label);
    // 状态控件：初次填充与事件循环共用同一份
    let status_widgets = StatusWidgets {
        running_block: Rc::clone(&running_block),
        stopped_block: Rc::clone(&stopped_block),
        running_pill: Rc::clone(&running_pill),
        stopped_pill: Rc::clone(&stopped_pill),
        local_label: Rc::clone(&local_label),
        lan_label: Rc::clone(&lan_label),
    };

    refresh_status(&state, &status_widgets);

    // 开机自启按钮的初始文字
    autostart_button.set_text(&format!(
        "开机自启：{}",
        if autostart_enabled() { "开" } else { "关" }
    ));

    // ---------------- 事件 ----------------

    // 窗口需要共享给事件处理
    let window = Rc::new(window);

    let event_state = state.clone();
    let event_window = window.clone();
    let event_commands = commands;

    // 事件回调是 Fn（不可变），且与 Panel 结构体共用这些控件，
    // 所以先克隆一份 Rc 交给回调。
    let event_status = StatusWidgets {
        running_block: Rc::clone(&running_block),
        stopped_block: Rc::clone(&stopped_block),
        running_pill: Rc::clone(&running_pill),
        stopped_pill: Rc::clone(&stopped_pill),
        local_label: Rc::clone(&local_label),
        lan_label: Rc::clone(&lan_label),
    };
    let event_selected_label = Rc::clone(&selected_label);

    let handler = nwg::full_bind_event_handler(&window.handle, move |evt, evt_data, handle| {
        use nwg::Event as E;

        match evt {
            E::OnButtonClick => {
                let state = event_state.clone();
                let window = event_window.clone();

                if handle == start_button {
                    let message = state.borrow().service.borrow_mut().start();
                    nwg::modal_info_message(
                        &window.handle,
                        "启动服务",
                        &message.unwrap_or_else(|e| e),
                    );
                } else if handle == stop_button {
                    let message = state.borrow().service.borrow_mut().stop();
                    nwg::modal_info_message(
                        &window.handle,
                        "停止服务",
                        &message.unwrap_or_else(|e| e),
                    );
                } else if handle == open_button {
                    let url = state.borrow().service.borrow().local_url();
                    let _ = webbrowser::open(&url);
                } else if handle == data_button {
                    open_data_folder();
                } else if handle == autostart_button {
                    let enabled = toggle_autostart();
                    autostart_button
                        .set_text(&format!("开机自启：{}", if enabled { "开" } else { "关" }));
                } else if handle == firewall_button {
                    let port = state.borrow().service.borrow().port();
                    let message = crate::service::allow_firewall(port);
                    nwg::modal_info_message(
                        &window.handle,
                        "放行防火墙端口",
                        &message.unwrap_or_else(|e| e),
                    );
                } else if handle == apply_button {
                    let text = port_input.text();
                    let parsed = text.trim().parse::<u16>();
                    let message = match parsed {
                        Ok(port) => state.borrow().service.borrow_mut().set_port(port),
                        Err(_) => Err("端口号必须是数字".to_string()),
                    };
                    nwg::modal_info_message(
                        &window.handle,
                        "端口设置",
                        &message.unwrap_or_else(|e| e),
                    );
                } else if handle == cycle_button {
                    cycle_selected_field(&state, &fields_list, &event_selected_label);
                } else if handle == save_button {
                    let message = save_fields(&state);
                    nwg::modal_info_message(
                        &window.handle,
                        "保存字段配置",
                        &message.unwrap_or_else(|e| e),
                    );
                } else if handle == refresh_button {
                    refresh_fields(&state, &fields_list, &event_selected_label);
                }
            }

            E::OnListViewClick => {
                // 点击列表时记录选中的行
                if let nwg::EventData::OnListViewItemIndex { row_index, .. } = evt_data {
                    event_state.borrow_mut().selected = Some(row_index);
                }
                refresh_selection(&event_state, &event_selected_label);
            }

            E::OnTimerTick => {
                // 处理托盘菜单发来的命令
                while let Ok(command) = event_commands.try_recv() {
                    match command {
                        TrayCommand::ShowPanel => {
                            event_window.set_visible(true);
                            event_window.set_focus();
                        }
                        TrayCommand::StartService => {
                            let message = event_state.borrow().service.borrow_mut().start();
                            nwg::modal_info_message(
                                &event_window.handle,
                                "启动服务",
                                &message.unwrap_or_else(|e| e),
                            );
                        }
                        TrayCommand::StopService => {
                            let message = event_state.borrow().service.borrow_mut().stop();
                            nwg::modal_info_message(
                                &event_window.handle,
                                "停止服务",
                                &message.unwrap_or_else(|e| e),
                            );
                        }
                        TrayCommand::Quit => {
                            let _ = event_state.borrow().service.borrow_mut().stop();
                            nwg::stop_thread_dispatch();
                            return;
                        }
                    }
                }

                refresh_status(&event_state, &event_status);
            }

            E::OnWindowClose => {
                // 关闭按钮只是收起到托盘，服务继续运行
                event_window.set_visible(false);
            }

            _ => {}
        }
    });

    // 事件处理器要一直活着
    std::mem::forget(handler);

    Ok(Panel {
        _window: window,
        _tabs: tabs,
        _service_tab: service_tab,
        _fields_tab: fields_tab,
        _fonts: fonts,
        _banner: banner,
        _running_block: running_block,
        _stopped_block: stopped_block,
        _service_card: service_card,
        _fields_card: fields_card,
        _banner_text: banner_text,
        _subtitle: subtitle,
        _running_pill: running_pill,
        _stopped_pill: stopped_pill,
        _service_title: service_title,
        _local_title: local_title,
        _local_label: local_label,
        _lan_title: lan_title,
        _lan_label: lan_label,
        _port_title: port_title,
        _service_divider: service_divider,
        _service_actions: service_actions,
        _fields_title: fields_title,
        _fields_hint: fields_hint,
        _selected_label: selected_label,
        _status_bar: status_bar,
        _timer: timer,
    })
}

/// 服务状态相关的控件，刷新时一起更新。
struct StatusWidgets {
    running_block: Rc<SolidBlock>,
    stopped_block: Rc<SolidBlock>,
    running_pill: Rc<TextBlock>,
    stopped_pill: Rc<TextBlock>,
    local_label: Rc<TextBlock>,
    lan_label: Rc<TextBlock>,
}

/// 刷新服务状态显示。
fn refresh_status(state: &Rc<RefCell<PanelState>>, widgets: &StatusWidgets) {
    let state = state.borrow();
    let service = state.service.borrow();

    let running = service.is_running();
    widgets.running_block.set_visible(running);
    widgets.stopped_block.set_visible(!running);
    widgets.running_pill.set_visible(running);
    widgets.stopped_pill.set_visible(!running);

    widgets.local_label.set_text(&service.local_url());
    widgets.lan_label.set_text(&match service.lan_url() {
        Some(url) => url,
        None => "未检测到局域网地址".to_string(),
    });
}

/// 扫描工资表并填充字段列表。
fn refresh_fields(
    state: &Rc<RefCell<PanelState>>,
    list: &nwg::ListView,
    selected_label: &Rc<TextBlock>,
) {
    let names = fields::discover_fields();

    {
        let mut state = state.borrow_mut();
        state.names = names.clone();
        state.selected = None;
        for name in &names {
            state.config.entry(name.clone()).or_insert(FieldState::Auto);
        }
    }

    rebuild_field_list(state, list);
    refresh_selection(state, selected_label);
}

/// 按当前配置重建字段列表。
fn rebuild_field_list(state: &Rc<RefCell<PanelState>>, list: &nwg::ListView) {
    list.clear();

    list.insert_column(nwg::InsertListViewColumn {
        index: Some(0),
        fmt: None,
        width: Some(300),
        text: Some("字段名".to_string()),
    });
    list.insert_column(nwg::InsertListViewColumn {
        index: Some(1),
        fmt: None,
        width: Some(140),
        text: Some("显示状态".to_string()),
    });

    let state = state.borrow();
    for (index, name) in state.names.iter().enumerate() {
        list.insert_item(nwg::InsertListViewItem {
            index: Some(index as i32),
            column_index: 0,
            text: Some(name.clone()),
        });

        let field_state = state.config.get(name).copied().unwrap_or(FieldState::Auto);
        list.insert_item(nwg::InsertListViewItem {
            index: Some(index as i32),
            column_index: 1,
            text: Some(fields::state_label(field_state).to_string()),
        });
    }
}

/// 刷新所选字段的说明。
fn refresh_selection(state: &Rc<RefCell<PanelState>>, selected_label: &Rc<TextBlock>) {
    let state = state.borrow();

    let Some(index) = state.selected else {
        selected_label.set_text("未选择字段");
        return;
    };

    let Some(name) = state.names.get(index) else {
        selected_label.set_text("未选择字段");
        return;
    };

    let field_state = state.config.get(name).copied().unwrap_or(FieldState::Auto);
    selected_label.set_text(&format!(
        "「{name}」当前状态：{}",
        fields::state_label(field_state)
    ));
}

/// 切换所选字段的显示状态。
fn cycle_selected_field(
    state: &Rc<RefCell<PanelState>>,
    list: &nwg::ListView,
    selected_label: &Rc<TextBlock>,
) {
    {
        let mut guard = state.borrow_mut();

        let Some(index) = guard.selected else {
            drop(guard);
            refresh_selection(state, selected_label);
            return;
        };

        let Some(name) = guard.names.get(index).cloned() else {
            return;
        };

        let entry = guard.config.entry(name).or_insert(FieldState::Auto);
        *entry = fields::next_state(*entry);
    }

    rebuild_field_list(state, list);
    refresh_selection(state, selected_label);
}

/// 保存字段配置。
fn save_fields(state: &Rc<RefCell<PanelState>>) -> Result<String, String> {
    let config = state.borrow().config.clone();
    fields::save_config(&config).map_err(|e| format!("保存失败: {e}"))?;
    Ok(format!("已保存 {} 个字段的显示配置", config.len()))
}

/// 打开数据目录。
fn open_data_folder() {
    let dir = salary_app_backend::paths::data_dir();
    let _ = std::process::Command::new("explorer.exe").arg(dir).spawn();
}

/// 开机自启是否已开启。
pub fn autostart_enabled() -> bool {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    let Ok(key) = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run")
    else {
        return false;
    };

    key.get_value::<String, _>("SalaryApp").is_ok()
}

/// 切换开机自启。
fn toggle_autostart() -> bool {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};

    const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
    const VALUE_NAME: &str = "SalaryApp";

    if autostart_enabled() {
        let _ = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(RUN_KEY, KEY_SET_VALUE)
            .and_then(|key| key.delete_value(VALUE_NAME));
        return false;
    }

    let Ok(exe) = std::env::current_exe() else {
        return false;
    };

    let result = RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey(RUN_KEY)
        .and_then(|(key, _)| key.set_value(VALUE_NAME, &exe.to_string_lossy().to_string()));

    result.is_ok()
}
