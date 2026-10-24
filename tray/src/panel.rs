//! 控制面板窗口。
//!
//! 窗口本身只做三件事：把画布挂上去、把画布发来的动作转成业务操作、
//! 以及定时刷新状态。界面长什么样全在 [`crate::canvas`] 里，
//! 配色与字体在 [`crate::theme`] 里。
//!
//! 画布发来的动作不在窗口过程里直接执行——那会卡住界面线程，
//! 统一丢进通道，由定时器在事件循环里消费。

use crate::canvas::{Action, Canvas, FieldRow, Hit, Notice, Page, Status};
use crate::fields;
use crate::service::{self, ServiceManager};
use crate::theme::{self, Theme};
use native_windows_gui as nwg;
use salary_app_backend::models::FieldState;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender};
use winapi::um::winuser::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

/// 定时器间隔：既是动作的响应延迟，也是状态刷新的节拍。
const TICK: u64 = 120;
/// 每隔几个 tick 刷新一次服务状态（端口探测本身是 2 秒一次）。
const STATUS_EVERY: u32 = 8;
/// 通知条停留时间（毫秒）。
const NOTICE_MS: i64 = 8000;

/// 面板资源。
///
/// 窗口、画布、输入框与定时器都必须与控件同寿命——
/// 任何被提前 drop 的控件都会销毁底层窗口。
pub struct Panel {
    /// 窗口需要一直存活，字段名加下划线表示「只为了持有」。
    _window: Rc<nwg::Window>,
    _canvas: Rc<RefCell<Canvas>>,
    _timer: nwg::AnimationTimer,
}

/// 托盘图标与侧边栏 logo 用的是同一份品牌图形（原版的 app_icon.ico），
/// 因此不随主题换色——换色的是面板本身，图标是应用的身份。
fn module_handle() -> winapi::shared::minwindef::HINSTANCE {
    unsafe { winapi::um::libloaderapi::GetModuleHandleW(std::ptr::null_mut()) }
}

/// 启动控制面板窗口。
pub fn build(
    service: Rc<RefCell<ServiceManager>>,
    commands: Receiver<crate::tray_icon::TrayCommand>,
    theme_sender: Sender<Theme>,
) -> Result<Panel, String> {
    let mut window = Default::default();
    let mut timer = Default::default();

    let (action_sender, action_receiver): (Sender<Action>, Receiver<Action>) = mpsc::channel();

    // 居中显示
    let screen_w = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let screen_h = unsafe { GetSystemMetrics(SM_CYSCREEN) };

    // NWG 的 size() 设的就是客户区尺寸，画布按同样尺寸铺满
    let position = (
        ((screen_w - crate::theme::metrics::WINDOW_W) / 2).max(0),
        ((screen_h - crate::theme::metrics::WINDOW_H) / 3).max(0),
    );

    // 先建好但不显示：图标要在窗口露脸之前设好，
    // 否则任务栏会先显示默认图标再跳一次。
    // MAIN_WINDOW 带 THICKFRAME 与最大化/最小化框：
    // 窗口可以拖边缩放、可以最大化，画布布局按当前尺寸自适应。
    nwg::Window::builder()
        .size((
            crate::theme::metrics::WINDOW_W,
            crate::theme::metrics::WINDOW_H,
        ))
        .position(position)
        .title("员工工资查询系统 - 控制面板")
        .flags(nwg::WindowFlags::MAIN_WINDOW)
        .build(&mut window)
        .map_err(|e| format!("无法创建窗口: {e}"))?;

    let window_handle = window.handle;

    // 标题栏由系统绘制：图标与底色都要跟主题走
    if let Some(hwnd) = window_handle.hwnd() {
        crate::brand::apply_window_icon(hwnd, module_handle());
        theme::apply_title_bar(hwnd, Theme::Dark);
    }

    // ---------------- 画布 ----------------

    let canvas = Rc::new(RefCell::new(Canvas::create(
        window_handle.hwnd().unwrap(),
        crate::theme::metrics::WINDOW_W,
        crate::theme::metrics::WINDOW_H,
        action_sender,
    )?));

    // 画布跟着窗口边框缩放：父窗口收到 WM_SIZE 时把画布拉成同样大小
    crate::canvas::attach_to_parent(window_handle.hwnd().unwrap(), canvas.borrow().handle());

    // ---------------- 初始状态 ----------------

    let canvas_ref = Rc::clone(&canvas);
    let service_ref = Rc::clone(&service);
    refresh_status(&canvas_ref, &service_ref);
    load_fields(&canvas_ref);
    load_networks(&canvas_ref);
    load_credentials(&canvas_ref);

    // ---------------- 定时刷新 ----------------

    nwg::AnimationTimer::builder()
        .interval(std::time::Duration::from_millis(TICK))
        .parent(&window)
        .build(&mut timer)
        .map_err(|e| format!("无法创建定时器: {e}"))?;
    timer.start();

    // ---------------- 事件 ----------------

    let window = Rc::new(window);
    let event_window = window.clone();
    let event_canvas = Rc::clone(&canvas);
    let event_service = Rc::clone(&service);
    let event_theme_sender = theme_sender.clone();
    // 工资表集合的指纹，用来发现工资表的增删改
    let event_field_signature = Rc::new(RefCell::new(fields::source_signature()));
    let tick_count = std::cell::Cell::new(0u32);

    let handler = nwg::full_bind_event_handler(&window.handle, move |evt, _evt_data, _handle| {
        use nwg::Event as E;

        match evt {
            E::OnTimerTick => {
                tick_count.set(tick_count.get() + 1);

                // 托盘菜单发来的命令
                while let Ok(command) = commands.try_recv() {
                    match command {
                        crate::tray_icon::TrayCommand::ShowPanel => {
                            event_window.set_visible(true);
                            event_window.set_focus();
                        }
                        crate::tray_icon::TrayCommand::StartService => {
                            run_service_action(&event_service, &event_canvas, Action::Start);
                        }
                        crate::tray_icon::TrayCommand::StopService => {
                            run_service_action(&event_service, &event_canvas, Action::Stop);
                        }
                        crate::tray_icon::TrayCommand::Quit => {
                            // 服务在跑时直接退出会让所有人查不了工资，
                            // 先确认一下（原版 Python 也是这么做的）。
                            if event_service.borrow().is_running() && !confirm_quit(&event_window) {
                                return;
                            }
                            let _ = event_service.borrow_mut().stop();
                            nwg::stop_thread_dispatch();
                            return;
                        }
                    }
                }

                // 画布发来的动作
                while let Ok(action) = action_receiver.try_recv() {
                    handle_action(
                        action,
                        &event_service,
                        &event_canvas,
                        &event_window,
                        &event_theme_sender,
                    );
                }

                // 通知条倒计时：倒计时本身看不见，不需要重绘，
                // 只有通知消失的那一刻才要刷新界面。
                let expired = {
                    let mut guard = event_canvas.borrow_mut();
                    match guard.model_mut().notice.as_mut() {
                        Some(notice) => {
                            notice.remaining -= TICK as i64;
                            notice.remaining <= 0
                        }
                        None => false,
                    }
                };
                if expired {
                    let mut guard = event_canvas.borrow_mut();
                    guard.update(|model| model.notice = None);
                    let region = guard.model().layout.notice;
                    guard.repaint(&[region]);
                }

                // 状态刷新：端口探测本身是 2 秒一次，没必要更频繁
                if tick_count.get() % STATUS_EVERY == 0 {
                    refresh_status(&event_canvas, &event_service);

                    // 看门狗：服务本该在跑却掉线了，把它拉起来。
                    // 原版 Python 有这个保活机制，Rust 版不能丢。
                    let dropped = event_service.borrow().dropped();
                    if dropped {
                        let outcome = event_service.borrow_mut().restart();
                        match outcome {
                            Ok(message) => set_notice(&event_canvas, &message, false),
                            Err(message) => set_notice(&event_canvas, &message, true),
                        }
                        refresh_status(&event_canvas, &event_service);
                    }

                    // 工资表有增删改时，字段列表按新的表重新生成。
                    // 面板上的「月份数」是自动更新的，字段列表也该跟上。
                    let signature = fields::source_signature();
                    if *event_field_signature.borrow() != signature {
                        *event_field_signature.borrow_mut() = signature;
                        load_fields(&event_canvas);
                        refresh_credential_fields(&event_canvas);
                    }
                }
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

    // 图标与主题都设好了，这时再让窗口露脸
    window.set_visible(true);

    // 焦点留在窗口上：端口输入框的聚焦框只在真的要改端口时才出现
    window.set_focus();

    Ok(Panel {
        _canvas: canvas,
        _window: window,
        _timer: timer,
    })
}

/// 处理画布发来的动作。
fn handle_action(
    action: Action,
    service: &Rc<RefCell<ServiceManager>>,
    canvas: &Rc<RefCell<Canvas>>,
    window: &Rc<nwg::Window>,
    theme_sender: &Sender<Theme>,
) {
    match action {
        Action::Navigate(page) => {
            let mut guard = canvas.borrow_mut();
            guard.update(|model| {
                model.page = page;
                model.scroll = 0;
                // 滚动动画只属于手册页；换页后让位（WM_TIMER 下一帧会自行清理定时器）
                model.scroll_anim = None;
            });
            // 换页只影响顶栏标题、左侧导航的选中态和内容区
            let layout = &guard.model().layout;
            let regions = [layout.title_area, layout.nav_area, layout.content];
            guard.repaint(&regions);
        }

        Action::ToggleTheme => {
            let mut guard = canvas.borrow_mut();
            guard.update(|model| {
                model.theme = match model.theme {
                    Theme::Dark => Theme::Light,
                    Theme::Light => Theme::Dark,
                };
            });
            // 换主题等于换整套配色，只能整块重绘
            guard.invalidate();
            let current = guard.model().theme;
            drop(guard);

            // 标题栏与托盘图标不归画布管，要单独通知
            if let Some(hwnd) = window.handle.hwnd() {
                theme::apply_title_bar(hwnd, current);
            }
            let _ = theme_sender.send(current);
        }

        Action::Start | Action::Stop => {
            run_service_action(service, canvas, action);
        }

        Action::OpenBrowser => {
            let url = service.borrow().local_url();
            let _ = webbrowser::open(&url);
        }

        Action::OpenData => {
            open_data_folder();
        }

        Action::Firewall => {
            let port = service.borrow().port();
            match service::allow_firewall(port) {
                Ok(message) => {
                    set_notice(canvas, &message, false);
                }
                Err(error) => {
                    // 防火墙的失败原因比较长，弹窗比挤在顶栏看得清楚
                    nwg::modal_info_message(window.handle, "放行防火墙端口", &error);
                }
            }
        }

        Action::ToggleAutostart => {
            let enabled = toggle_autostart();
            let message = format!("开机自启已{}", if enabled { "开启" } else { "关闭" });
            set_notice(canvas, &message, false);
            refresh_status(canvas, service);
        }

        Action::ApplyPort => {
            let text = canvas.borrow().model().port_text.clone();
            apply_port(canvas, service, &text);
        }

        Action::ApplyPortText(text) => {
            apply_port(canvas, service, &text);
        }

        Action::SelectField(index) => {
            let mut guard = canvas.borrow_mut();
            let previous = guard.model().selected;
            guard.update(|model| model.selected = Some(index));

            // 旧选中行与新选中行都要重绘
            let layout = &guard.model().layout;
            let scroll = guard.model().scroll as usize;
            let mut regions = Vec::new();
            for candidate in [previous, Some(index)] {
                if let Some(row) = candidate.and_then(|value| value.checked_sub(scroll)) {
                    if let Some(rect) = layout.field_rows.get(row) {
                        regions.push(*rect);
                    }
                }
            }
            guard.repaint(&regions);
        }

        Action::CycleField => {
            let mut guard = canvas.borrow_mut();
            let selected = guard.model().selected;

            // 没选中字段时不要静默失败：用户点了按钮却什么都没发生，
            // 最容易误以为是界面坏了。
            let Some(index) = selected else {
                drop(guard);
                set_notice(canvas, "请先在列表里选中一个字段", false);
                return;
            };

            guard.update(|model| {
                let Some(field) = model.fields.get_mut(index) else {
                    return;
                };
                field.state = fields::next_state(field.state);
            });

            // 只有那一行的状态 pill 变了
            let layout = &guard.model().layout;
            let scroll = guard.model().scroll as usize;
            if let Some(row) = index.checked_sub(scroll) {
                if let Some(rect) = layout.field_rows.get(row) {
                    guard.repaint(&[*rect]);
                }
            }
        }

        Action::SaveFields => {
            // 按面板里的字段顺序保存：新字段追加到文件末尾时顺序稳定
            let config: Vec<(String, FieldState)> = canvas
                .borrow()
                .model()
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.state))
                .collect();

            match fields::save_config(&config) {
                Ok(()) => {
                    let message = format!("已保存 {} 个字段的显示配置", config.len());
                    set_notice(canvas, &message, false);
                }
                Err(error) => set_notice(canvas, &format!("保存失败: {error}"), true),
            }
        }

        Action::CopyUrl(index) => {
            let url = match index {
                0 => service.borrow().lan_url(),
                _ => service.borrow().lan_https_url(),
            };
            match url {
                Some(url) if crate::clipboard::copy_text(&url) => {
                    set_notice(canvas, &format!("已复制：{url}"), false);
                }
                Some(_) => set_notice(canvas, "复制失败，请手动选中地址复制", true),
                None => set_notice(canvas, "未检测到局域网地址", true),
            }
        }

        Action::CopyAll => {
            let text = copy_all_text(service);
            if crate::clipboard::copy_text(&text) {
                set_notice(canvas, "已复制访问地址与登录说明", false);
            } else {
                set_notice(canvas, "复制失败，请手动复制", true);
            }
        }

        Action::ViewLog => {
            let path = salary_app_backend::paths::audit_log_file();
            if !path.exists() {
                set_notice(
                    canvas,
                    "还没有审计日志（有人查过工资条之后才会生成）",
                    false,
                );
            } else if open_file(&path) {
                set_notice(canvas, "已在记事本里打开审计日志", false);
            } else {
                set_notice(canvas, "打开日志失败，可以在数据目录里手动打开", true);
            }
        }

        Action::CopyRepo => {
            let url = crate::canvas::REPO_URL;
            if url.is_empty() {
                set_notice(canvas, "尚未配置项目地址", true);
            } else if crate::clipboard::copy_text(url) {
                set_notice(canvas, &format!("已复制：{url}"), false);
            } else {
                set_notice(canvas, "复制失败，请手动选中地址复制", true);
            }
        }

        Action::OpenRepo => {
            let url = crate::canvas::REPO_URL;
            if url.is_empty() {
                set_notice(canvas, "尚未配置项目地址", true);
            } else {
                let _ = webbrowser::open(url);
            }
        }

        Action::RescanFields => {
            load_fields(canvas);
            set_notice(canvas, "已重新扫描工资表字段", false);
        }

        Action::CredentialSelectAccount(index) => {
            let mut guard = canvas.borrow_mut();
            guard.update(|model| {
                model.credential.account = Some(index);
            });
            let region = guard.model().layout.credential_card;
            guard.repaint(&[region]);
        }

        Action::CredentialSelectField(index) => {
            let mut guard = canvas.borrow_mut();
            guard.update(|model| {
                model.credential.field = Some(index);
            });
            let region = guard.model().layout.credential_card;
            guard.repaint(&[region]);
        }

        Action::CredentialReadFields => {
            load_credentials(canvas);
            set_notice(canvas, "已重新读取工资表字段", false);
        }

        Action::CredentialSave => {
            save_credential(canvas);
        }
    }
}

/// 保存登录字段映射：账号列与密码列的表头名落盘。
///
/// 服务端每次登录都会重新读这份映射（缓存键里带着它的修改时间），
/// 保存后网页登录立即按新的列校验，不用重启服务。
fn save_credential(canvas: &Rc<RefCell<Canvas>>) {
    let (account, password) = {
        let guard = canvas.borrow();
        let state = &guard.model().credential;

        let account = state
            .account
            .and_then(|index| state.fields.get(index))
            .cloned()
            .unwrap_or_default();
        let password = state
            .field
            .and_then(|index| state.fields.get(index))
            .cloned()
            .unwrap_or_default();

        (account, password)
    };

    if account.is_empty() {
        set_notice(canvas, "请先选择账号字段", false);
        return;
    }
    if password.is_empty() {
        set_notice(canvas, "请先选择密码字段", false);
        return;
    }

    let fields = crate::credentials::LoginFields {
        account_field: account.clone(),
        password_field: password.clone(),
    };

    match crate::credentials::save(&fields) {
        Ok(()) => {
            // 底部提示条与一键复制引用这份映射，保存后立即同步
            let mut guard = canvas.borrow_mut();
            guard.update(|model| {
                model.credential.saved_account = account;
                model.credential.saved_password = password;
            });
            drop(guard);
            set_notice(canvas, "登录字段已保存，网页登录立即生效", false);
        }
        Err(error) => set_notice(canvas, &error, true),
    }
}

/// 应用端口设置。
fn apply_port(canvas: &Rc<RefCell<Canvas>>, service: &Rc<RefCell<ServiceManager>>, text: &str) {
    let parsed = text.trim().parse::<u16>();
    let result = match parsed {
        Ok(port) => service.borrow_mut().set_port(port),
        Err(_) => Err("端口号必须是数字".to_string()),
    };

    match result {
        Ok(message) => set_notice(canvas, &message, false),
        Err(error) => set_notice(canvas, &error, true),
    }

    refresh_status(canvas, service);
}

/// 启停服务并给出反馈。
fn run_service_action(
    service: &Rc<RefCell<ServiceManager>>,
    canvas: &Rc<RefCell<Canvas>>,
    action: Action,
) {
    let result = match action {
        Action::Start => service.borrow_mut().start(),
        Action::Stop => service.borrow_mut().stop(),
        _ => Ok(String::new()),
    };

    match result {
        Ok(message) => set_notice(canvas, &message, false),
        Err(error) => set_notice(canvas, &error, true),
    }

    refresh_status(canvas, service);
}

/// 扫描工资表并加载字段列表。
///
/// 重新扫描时保留用户已经调过的状态：正在编辑的（内存里的）优先，
/// 其次是 `fields.txt` 里保存过的。
fn load_fields(canvas: &Rc<RefCell<Canvas>>) {
    let saved = fields::load_config();
    let names = fields::discover_fields();

    let mut guard = canvas.borrow_mut();
    guard.update(|model| {
        let previous: HashMap<String, FieldState> = model
            .fields
            .iter()
            .map(|field| (field.name.clone(), field.state))
            .collect();

        model.fields = names
            .into_iter()
            .map(|name| {
                let state = previous
                    .get(&name)
                    .copied()
                    .or_else(|| saved.get(&name).copied())
                    .unwrap_or(FieldState::Auto);
                FieldRow { name, state }
            })
            .collect();
        model.selected = None;
        model.scroll = 0;
    });

    // 字段列表与底部的数量说明都变了
    let layout = &guard.model().layout;
    let regions = [layout.field_list, layout.hint];
    guard.repaint(&regions);
}

/// 载入凭据页的状态：账号列表、字段列表与已保存的凭据。
/// 载入凭据页的状态：登录字段候选与已保存的映射。
///
/// 候选列表来自员工表的表头（登录校验对着这张表做），
/// 已保存的映射能让下拉直接停在当前生效的字段上。
fn load_credentials(canvas: &Rc<RefCell<Canvas>>) {
    let fields = crate::credentials::discover_field_names();
    let saved = crate::credentials::load();

    let account = if saved.account_field.is_empty() {
        None
    } else {
        fields.iter().position(|name| *name == saved.account_field)
    };
    let field = if saved.password_field.is_empty() {
        None
    } else {
        fields.iter().position(|name| *name == saved.password_field)
    };

    let mut guard = canvas.borrow_mut();
    guard.update(|model| {
        model.credential.fields = fields;
        model.credential.account = account;
        model.credential.field = field;
        // 底部提示条与一键复制引用已保存的映射，而不是硬编码的列名
        model.credential.saved_account = saved.account_field.clone();
        model.credential.saved_password = saved.password_field.clone();
        model.credential.dropdown = None;
    });
}

/// 工资表有变动时刷新凭据页的字段列表（尽量保留已选项）。
fn refresh_credential_fields(canvas: &Rc<RefCell<Canvas>>) {
    let fields = crate::credentials::discover_field_names();

    let mut guard = canvas.borrow_mut();
    guard.update(|model| {
        let previous_account = model
            .credential
            .account
            .and_then(|index| model.credential.fields.get(index))
            .cloned();
        let previous_field = model
            .credential
            .field
            .and_then(|index| model.credential.fields.get(index))
            .cloned();

        model.credential.fields = fields;
        model.credential.account = previous_account.and_then(|name| {
            model
                .credential
                .fields
                .iter()
                .position(|field| *field == name)
        });
        model.credential.field = previous_field.and_then(|name| {
            model
                .credential
                .fields
                .iter()
                .position(|field| *field == name)
        });
    });
}

/// 一键复制的内容：访问地址、端口与登录说明。
fn copy_all_text(service: &Rc<RefCell<ServiceManager>>) -> String {
    let service = service.borrow();
    let mut lines = Vec::new();
    if let Some(url) = service.lan_url() {
        lines.push(format!("局域网访问（HTTP）：{url}"));
    }
    if let Some(url) = service.lan_https_url() {
        lines.push(format!("局域网访问（HTTPS）：{url}"));
    }
    lines.push(format!("端口：{}", service.port()));

    // 登录说明跟着字段映射走：控制面板里选了哪列当账号/密码，这里就写哪列。
    // 读画布状态里已同步的映射，避免每次复制都去读文件。
    let mapping = crate::credentials::load();
    let account = if mapping.account_field.is_empty() {
        "工号".to_string()
    } else {
        mapping.account_field.clone()
    };
    let password = if mapping.password_field.is_empty() {
        "紧急联系人号码".to_string()
    } else {
        mapping.password_field.clone()
    };
    lines.push(format!("员工登录：账号 = {account}，密码 = {password}"));
    lines.join("\n")
}

/// 用记事本打开一个文件。
///
/// 直接用「open」动词依赖文件关联，`.log` 在很多机器上根本没有关联，
/// 这里点名记事本，稳一点。
fn open_file(path: &std::path::Path) -> bool {
    use winapi::um::shellapi::ShellExecuteW;

    let file: Vec<u16> = path
        .as_os_str()
        .to_string_lossy()
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let verb: Vec<u16> = "open".encode_utf16().chain(Some(0)).collect();
    let exe: Vec<u16> = "notepad.exe".encode_utf16().chain(Some(0)).collect();

    unsafe {
        let result = ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            exe.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            1,
        );
        // ShellExecute 成功时返回值大于 32
        result as isize > 32
    }
}

/// 退出前确认。
///
/// 服务在跑时退出会停掉所有人的查询，误操作的代价太大。
fn confirm_quit(window: &Rc<nwg::Window>) -> bool {
    let params = nwg::MessageParams {
        title: "退出确认",
        content: "工资查询服务正在运行，退出后员工将无法查询。\n\n确定要退出吗？",
        buttons: nwg::MessageButtons::YesNo,
        icons: nwg::MessageIcons::Warning,
    };
    nwg::modal_message(window.handle, &params) == nwg::MessageChoice::Yes
}

/// 读取支持访问的网络配置。
fn load_networks(canvas: &Rc<RefCell<Canvas>>) {
    let rows = crate::networks::load();
    canvas.borrow_mut().update_and_repaint(|model| {
        model.networks = rows;
    });
}

/// 刷新状态快照。
fn refresh_status(canvas: &Rc<RefCell<Canvas>>, service: &Rc<RefCell<ServiceManager>>) {
    let status = {
        let service = service.borrow();
        Status {
            running: service.is_running(),
            port: service.port(),
            local_url: service.local_url(),
            lan_url: service
                .lan_url()
                .unwrap_or_else(|| "未检测到局域网地址".to_string()),
            lan_https_url: service
                .lan_https_url()
                .unwrap_or_else(|| "未检测到局域网地址".to_string()),
            autostart: autostart_enabled(),
            months: count_months(),
            restarts: service.restart_count(),
        }
    };

    let mut guard = canvas.borrow_mut();

    // 状态没变就不重绘：定时器每 120ms 问一次，
    // 无条件重绘会让整个面板不停闪烁。
    if guard.model().status == status {
        return;
    }

    guard.update(move |model| {
        model.status = status;
        // 正在编辑端口时不要覆盖用户输入的内容
        if !model.port_editing {
            model.port_text = model.status.port.to_string();
        }
    });

    // 只重绘受影响的几块：状态 pill 一直在，
    // KPI 行与访问地址卡只有服务页才有。
    let layout = &guard.model().layout;
    let mut regions = vec![layout.status_pill];
    if guard.model().page == Page::Service {
        regions.push(layout.kpi_row);
        regions.push(layout.access_card);
        // 开机自启按钮上的文字也写着开关状态
        if let Some(rect) = layout.action_button(Hit::Autostart) {
            regions.push(rect);
        }
    }
    guard.repaint(&regions);
}

/// 在顶栏显示一条通知。
fn set_notice(canvas: &Rc<RefCell<Canvas>>, text: &str, error: bool) {
    let text = text.replace('\n', " ");
    let mut guard = canvas.borrow_mut();
    guard.update(move |model| {
        model.notice = Some(Notice {
            text,
            error,
            remaining: NOTICE_MS,
        });
    });
    // 通知条只占顶栏里的一小条
    let region = guard.model().layout.notice;
    guard.repaint(&[region]);
}

/// 统计可查询的工资月份数。
///
/// 用后端同一套月份口径，这样面板上的数字和网页端的月份列表永远一致。
fn count_months() -> usize {
    salary_app_backend::handlers::list_months().len()
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
