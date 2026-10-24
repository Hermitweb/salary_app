//! 自绘控制面板画布。
//!
//! 系统的 Tab / Button / ListView 都带着自己的主题，深色主题下会明显割裂，
//! 所以整块面板改成一张自绘画布：卡片、按钮、状态 pill、字段列表行
//! 全部由 GDI 画出来，鼠标交互靠命中测试完成。
//!
//! 画布只负责「画」和「报动作」，真正的业务动作（启停服务、改字段配置）
//! 由 [`crate::panel`] 处理——画布通过通道把 [`Action`] 发出去，
//! 避免在窗口过程里做耗时操作（那会让窗口「未响应」）。

use crate::paint::{self, Align, Rect};
use crate::theme::{self, Fonts, Palette, Theme};
use salary_app_backend::models::FieldState;
use std::mem::size_of;
use std::sync::mpsc::Sender;
use winapi::shared::minwindef::{DWORD, LPARAM, LRESULT, UINT, WPARAM};
use winapi::shared::windef::{HBITMAP, HDC, HFONT, HWND};
use winapi::um::libloaderapi::GetModuleHandleW;
use winapi::um::wingdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, IntersectClipRect, SRCCOPY, SelectClipRgn,
    SelectObject,
};
use winapi::um::winuser::{
    BeginPaint, CreateWindowExW, DefWindowProcW, DestroyWindow, EndPaint, GetDC, GetWindowLongPtrW,
    IDC_HAND, InvalidateRect, LoadCursorW, RegisterClassExW, ReleaseCapture, ReleaseDC, SetCapture,
    SetCursor, SetFocus, SetWindowLongPtrW, TRACKMOUSEEVENT, TrackMouseEvent, WM_CHAR, WM_DESTROY,
    WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSELEAVE, WM_MOUSEMOVE,
    WM_MOUSEWHEEL, WM_NCCREATE, WM_NCDESTROY, WM_PAINT, WM_SETCURSOR, WM_SIZE, WNDCLASSEXW,
    WS_CHILD, WS_CLIPCHILDREN, WS_VISIBLE,
};

/// 窗口类名。
const CLASS_NAME: &str = "SalaryPanelCanvas";

/// 父窗口的挂接：让画布跟着窗口边框缩放。
///
/// 父窗口是 NWG 建的，没法直接改它的窗口过程，
/// 这里用 comctl32 的窗口子类化挂一个子类上去：
/// 收到 WM_SIZE 就把画布拉成同样大小，收到 WM_GETMINMAXINFO 就给出最小尺寸。
mod host {
    use winapi::shared::minwindef::{BOOL, LPARAM, LRESULT, UINT, WPARAM};
    use winapi::shared::windef::HWND;
    use winapi::um::winuser::{
        MINMAXINFO, SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos, WM_GETMINMAXINFO, WM_NCDESTROY,
        WM_SIZE,
    };

    /// 子类回调的签名。
    pub type SubclassProc =
        unsafe extern "system" fn(HWND, UINT, WPARAM, LPARAM, usize, usize) -> LRESULT;

    #[link(name = "comctl32")]
    unsafe extern "system" {
        fn SetWindowSubclass(hwnd: HWND, pfn: SubclassProc, id: usize, ref_data: usize) -> BOOL;
        fn RemoveWindowSubclass(hwnd: HWND, pfn: SubclassProc, id: usize) -> BOOL;
        fn DefSubclassProc(
            hwnd: HWND,
            msg: UINT,
            wparam: WPARAM,
            lparam: LPARAM,
            id: usize,
            ref_data: usize,
        ) -> LRESULT;
    }

    /// 子类标识：一个窗口只挂一个，用固定值即可。
    const SUBCLASS_ID: usize = 0x5A17;

    /// 父窗口的窗口过程：处理尺寸变化与最小尺寸。
    unsafe extern "system" fn parent_proc(
        hwnd: HWND,
        msg: UINT,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        ref_data: usize,
    ) -> LRESULT {
        match msg {
            WM_SIZE => {
                // 把画布拉成和客户区一样大
                let canvas = ref_data as HWND;
                let width = winapi::shared::minwindef::LOWORD(lparam as u32) as i32;
                let height = winapi::shared::minwindef::HIWORD(lparam as u32) as i32;
                if !canvas.is_null() && width > 0 && height > 0 {
                    unsafe {
                        SetWindowPos(
                            canvas,
                            std::ptr::null_mut(),
                            0,
                            0,
                            width,
                            height,
                            SWP_NOZORDER | SWP_NOACTIVATE,
                        );
                    }
                }
            }
            WM_GETMINMAXINFO => {
                // 再小就挤不下了
                let info = lparam as *mut MINMAXINFO;
                if !info.is_null() {
                    unsafe {
                        (*info).ptMinTrackSize.x = crate::theme::metrics::MIN_W;
                        (*info).ptMinTrackSize.y = crate::theme::metrics::MIN_H;
                    }
                }
            }
            WM_NCDESTROY => {
                // 窗口要没了，摘掉子类，免得野指针留在 comctl32 里
                unsafe {
                    RemoveWindowSubclass(hwnd, parent_proc, SUBCLASS_ID);
                }
            }
            _ => {}
        }

        unsafe { DefSubclassProc(hwnd, msg, wparam, lparam, SUBCLASS_ID, ref_data) }
    }

    /// 给父窗口挂上子类，把画布句柄传进去。
    pub fn attach(parent: HWND, canvas: HWND) {
        unsafe {
            SetWindowSubclass(parent, parent_proc, SUBCLASS_ID, canvas as usize);
        }
    }
}

/// 让画布跟着父窗口一起变尺寸，并限制窗口的最小尺寸。
pub fn attach_to_parent(parent: HWND, canvas: HWND) {
    host::attach(parent, canvas);
}

/// 面板的页面。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    /// 服务控制
    Service,
    /// 字段显示
    Fields,
    /// 使用手册
    Manual,
}

/// 可点击区域。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    /// 左侧导航：服务控制
    NavService,
    /// 左侧导航：字段显示
    NavFields,
    /// 左侧导航：使用手册
    NavManual,
    /// 复制第几条访问地址
    CopyUrl(usize),
    /// 手册目录的第几项
    ManualToc(usize),
    /// 一键复制所有信息
    CopyAll,
    /// 查看日志
    ViewLog,
    /// 主题切换
    ThemeToggle,
    /// 启动服务
    Start,
    /// 停止服务
    Stop,
    /// 在浏览器里打开
    OpenBrowser,
    /// 打开数据目录
    OpenData,
    /// 放行防火墙端口
    Firewall,
    /// 切换开机自启
    Autostart,
    /// 应用端口
    ApplyPort,
    /// 点击端口输入框
    PortField,
    /// 切换所选字段的显示状态
    Cycle,
    /// 保存字段配置
    Save,
    /// 重新扫描字段
    Rescan,
    /// 选中某个字段
    FieldRow(usize),
    /// 账号字段下拉
    CredentialAccount,
    /// 密码字段下拉
    CredentialField,
    /// 重新读取字段列表
    CredentialReadFields,
    /// 保存登录字段映射
    CredentialSave,
    /// 下拉弹层里的第几项
    DropdownItem(usize),
}

/// 画布发出去的动作。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// 切换页面
    Navigate(Page),
    /// 切换深色 / 浅色主题
    ToggleTheme,
    /// 启动服务
    Start,
    /// 停止服务
    Stop,
    /// 在浏览器里打开
    OpenBrowser,
    /// 打开数据目录
    OpenData,
    /// 放行防火墙端口
    Firewall,
    /// 切换开机自启
    ToggleAutostart,
    /// 应用端口（文本由调用方从输入框读取）
    ApplyPort,
    /// 应用端口（带上输入框里的文字）
    ApplyPortText(String),
    /// 切换所选字段的显示状态
    CycleField,
    /// 选中某个字段
    SelectField(usize),
    /// 保存字段配置
    SaveFields,
    /// 重新扫描字段
    RescanFields,
    /// 复制第几条访问地址
    CopyUrl(usize),
    /// 一键复制所有信息（地址 + 端口 + 登录说明）
    CopyAll,
    /// 查看审计日志
    ViewLog,
    /// 选中某个账号字段
    CredentialSelectAccount(usize),
    /// 选中某个密码字段
    CredentialSelectField(usize),
    /// 重新读取字段列表
    CredentialReadFields,
    /// 保存登录字段映射
    CredentialSave,
}

/// 展开的下拉框属于哪个控件。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropdownKind {
    /// 账号下拉。
    Account,
    /// 密码来源字段下拉。
    Field,
}

/// 展开着的下拉框。
#[derive(Debug, Clone)]
pub struct Dropdown {
    /// 属于哪个控件。
    pub kind: DropdownKind,
    /// 弹层里的条目。
    pub items: Vec<String>,
    /// 当前选中的下标。
    pub selected: Option<usize>,
    /// 滚动偏移（按行）。
    pub scroll: i32,
}

/// 凭据页的状态。
#[derive(Debug, Clone, Default)]
pub struct CredentialState {
    /// 可选的登录字段（表头名）。
    pub fields: Vec<String>,
    /// 选中的账号字段下标。
    pub account: Option<usize>,
    /// 选中的密码字段下标。
    pub field: Option<usize>,
    /// 已保存的映射：账号列与密码列的表头名。
    ///
    /// 底部提示条与一键复制都引用这两份文案，
    /// 让「登录用哪两列」在界面上只有同一个出处。
    pub saved_account: String,
    pub saved_password: String,
    /// 展开着的下拉框。
    pub dropdown: Option<Dropdown>,
}

/// 通知条。
#[derive(Debug, Clone)]
pub struct Notice {
    /// 文案
    pub text: String,
    /// 是否是错误
    pub error: bool,
    /// 剩余显示时间（毫秒）
    pub remaining: i64,
}

/// 服务状态快照。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    /// 服务是否在运行
    pub running: bool,
    /// HTTP 端口
    pub port: u16,
    /// 本机访问地址
    pub local_url: String,
    /// 局域网访问地址
    pub lan_url: String,
    /// 局域网 HTTPS 访问地址（端口 +1）
    pub lan_https_url: String,
    /// 开机自启是否开启
    pub autostart: bool,
    /// 可查询的工资月份数
    pub months: usize,
    /// 服务自动恢复过几次（保活）
    pub restarts: u32,
}

/// 字段行。
#[derive(Debug, Clone)]
pub struct FieldRow {
    /// 字段名
    pub name: String,
    /// 显示策略
    pub state: FieldState,
}

/// 画布的数据。
pub struct Model {
    /// 当前主题
    pub theme: Theme,
    /// 当前页面
    pub page: Page,
    /// 服务状态
    pub status: Status,
    /// 字段列表
    pub fields: Vec<FieldRow>,
    /// 登录凭据页的状态
    pub credential: CredentialState,
    /// 选中的字段
    pub selected: Option<usize>,
    /// 列表滚动偏移
    pub scroll: i32,
    /// 使用手册的滚动偏移（像素）。
    pub manual_scroll: i32,
    /// 手册的每一行（已折行）
    pub manual_lines: Vec<ManualLine>,
    /// 手册的目录（标题与起始位置）
    pub manual_sections: Vec<ManualSection>,
    /// 支持访问的网络（来自 `data/networks.txt`）
    pub networks: Vec<crate::networks::NetworkRow>,
    /// 鼠标悬停的区域
    pub hover: Option<Hit>,
    /// 按下的区域
    pub pressed: Option<Hit>,
    /// 顶部通知条
    pub notice: Option<Notice>,
    /// 端口输入框里的文字
    pub port_text: String,
    /// 端口输入框是否正在编辑
    pub port_editing: bool,
    /// 开始编辑前的端口号（按 Esc 可以退回）
    pub port_original: String,
    /// 输入框里的光标位置
    pub caret: usize,
    /// 画布尺寸（客户区像素）。窗口缩放后由 WM_SIZE 更新。
    pub size: (i32, i32),
    /// 字号缩放系数：窗口宽度相对默认宽度的比例（1.0–1.4）。
    /// 由 WM_SIZE 随窗口宽度更新，绘制端用它等比放大卡片内部留白。
    pub font_scale: f32,
    /// 当前布局
    pub layout: Layout,
}

impl Default for Model {
    fn default() -> Self {
        Self {
            theme: Theme::Dark,
            page: Page::Service,
            status: Status::default(),
            fields: Vec::new(),
            credential: CredentialState::default(),
            selected: None,
            scroll: 0,
            manual_scroll: 0,
            manual_lines: Vec::new(),
            manual_sections: Vec::new(),
            networks: Vec::new(),
            hover: None,
            pressed: None,
            notice: None,
            port_text: String::new(),
            port_editing: false,
            port_original: String::new(),
            caret: 0,
            size: (theme::metrics::WINDOW_W, theme::metrics::WINDOW_H),
            font_scale: 1.0,
            layout: Layout::default(),
        }
    }
}

/// 布局：所有控件的位置与命中区域。
///
/// 规范要求「结构零改动」，所以布局只算一次，主题切换时完全不动。
#[derive(Debug, Clone, Default)]
pub struct Layout {
    /// 命中区域（后画的在上，命中测试从后往前找）。
    pub hits: Vec<(Hit, Rect)>,
    /// 导航项
    pub nav: [Rect; 3],
    /// 主题切换按钮
    pub theme_button: Rect,
    /// 状态 pill
    pub status_pill: Rect,
    /// 通知条
    pub notice: Rect,
    /// 内容区（侧边栏右侧、顶栏以下）：换页时整块换掉。
    pub content: Rect,
    /// 顶栏标题（中文标题 + 英文小标签）：换页时跟着换。
    pub title_area: Rect,
    /// 左侧导航区：换页时选中态要跟着换。
    pub nav_area: Rect,
    /// KPI 行：服务状态变化时只重绘这一行。
    pub kpi_row: Rect,
    /// 字段列表区（含滚动条轨道）：滚动时只重绘这一块。
    pub field_list: Rect,
    /// 列表能显示几行（按可用高度算，窗口越高行数越多）。
    pub visible_rows: usize,
    /// KPI 卡片
    pub kpi: [Rect; 3],
    /// 访问地址卡片
    pub access_card: Rect,
    /// 账号密码卡片
    pub credential_card: Rect,
    /// 地址行的标签列
    pub url_labels: [Rect; 2],
    /// 地址行的值列
    pub url_values: [Rect; 2],
    /// 每条地址右侧的「复制」按钮
    pub copy_buttons: [Rect; 2],
    /// 「一键复制所有信息」按钮
    pub copy_all_button: Rect,
    /// 「支持访问的网络」标题
    pub network_label: Rect,
    /// 网络条目
    pub network_rows: Vec<Rect>,
    /// 「查看日志」按钮
    pub view_log_button: Rect,
    /// 端口输入框
    pub port_edit: Rect,
    /// 应用端口按钮
    pub apply_button: Rect,
    /// 快捷操作卡片
    pub actions_card: Rect,
    /// 快捷操作按钮
    pub action_buttons: Vec<(Hit, Rect)>,
    /// 字段卡片
    pub fields_card: Rect,
    /// 字段行
    pub field_rows: Vec<Rect>,
    /// 字段页按钮
    pub field_buttons: Vec<(Hit, Rect)>,
    /// 使用手册卡片
    pub manual_card: Rect,
    /// 使用手册正文区（可滚动）
    pub manual_list: Rect,
    /// 手册目录的标题
    pub manual_toc_title: Rect,
    /// 手册目录条目
    pub manual_toc: Vec<Rect>,
    /// 目录与正文之间的分隔线
    pub manual_divider: Rect,
    /// 账号下拉
    pub credential_account: Rect,
    /// 密码字段下拉
    pub credential_field: Rect,
    /// 凭据页按钮
    pub credential_buttons: Vec<(Hit, Rect)>,
    /// 凭据页的状态提示
    pub credential_status: Rect,
    /// 下拉弹层
    pub dropdown: Rect,
    /// 下拉弹层里的条目
    pub dropdown_items: Vec<Rect>,
    /// 底部提示
    pub hint: Rect,
}

impl Layout {
    /// 命中测试：返回最上层包含该点的区域。
    pub fn hit_test(&self, x: i32, y: i32) -> Option<Hit> {
        self.hits
            .iter()
            .rev()
            .find(|(_, rect)| rect.contains(x, y))
            .map(|(hit, _)| *hit)
    }

    /// 某个命中区域对应的矩形。
    pub fn rect_of(&self, hit: Hit) -> Option<Rect> {
        self.hits
            .iter()
            .find(|(candidate, _)| *candidate == hit)
            .map(|(_, rect)| *rect)
    }

    /// 某个快捷操作按钮的位置。
    pub fn action_button(&self, hit: Hit) -> Option<Rect> {
        self.action_buttons
            .iter()
            .find(|(candidate, _)| *candidate == hit)
            .map(|(_, rect)| *rect)
    }
}

/// 两个矩形的并集（用于重绘「旧区域 + 新区域」）。
fn union(a: Rect, b: Rect) -> Rect {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    let right = a.right().max(b.right());
    let bottom = a.bottom().max(b.bottom());
    Rect::new(x, y, right - x, bottom - y)
}

/// 后备缓冲：整块面板先画在这里，再一次性贴到屏幕上。
///
/// 直接往屏幕 DC 画的话，一大片区域会被分很多笔慢慢刷出来，
/// 肉眼看到的就是「闪一下」。先画到内存位图，再用一次 `BitBlt` 贴上去，
/// 每次重绘都是原子的，不会闪。
struct BackBuffer {
    /// 内存 DC。
    dc: HDC,
    /// 位图句柄。
    bitmap: HBITMAP,
    /// 之前选中的对象（释放时还原）。
    previous: *mut winapi::ctypes::c_void,
    /// 位图尺寸。
    size: (i32, i32),
}

impl BackBuffer {
    /// 建一块 `w × h` 的 32 位后备缓冲。
    fn new(w: i32, h: i32) -> Result<Self, String> {
        let mut buffer = Self {
            dc: core::ptr::null_mut(),
            bitmap: core::ptr::null_mut(),
            previous: core::ptr::null_mut(),
            size: (0, 0),
        };

        unsafe {
            let dc = CreateCompatibleDC(core::ptr::null_mut());
            if dc.is_null() {
                return Err("后备缓冲 DC 创建失败".to_string());
            }
            buffer.dc = dc;
            buffer.previous = SelectObject(dc, core::ptr::null_mut());
        }

        buffer.resize(w, h)?;
        Ok(buffer)
    }

    /// 按新尺寸重建位图（窗口缩放时调用）。
    fn resize(&mut self, w: i32, h: i32) -> Result<(), String> {
        if w <= 0 || h <= 0 || self.size == (w, h) {
            return Ok(());
        }

        unsafe {
            let mut info: BITMAPINFO = core::mem::zeroed();
            info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
            info.bmiHeader.biWidth = w;
            // 负高度表示自上而下，和画布坐标一致
            info.bmiHeader.biHeight = -h;
            info.bmiHeader.biPlanes = 1;
            info.bmiHeader.biBitCount = 32;
            info.bmiHeader.biCompression = BI_RGB;

            let mut bits: *mut winapi::ctypes::c_void = core::ptr::null_mut();
            let bitmap = CreateDIBSection(
                core::ptr::null_mut(),
                &info,
                DIB_RGB_COLORS,
                &mut bits,
                core::ptr::null_mut(),
                0,
            );
            if bitmap.is_null() {
                return Err("后备缓冲位图创建失败".to_string());
            }

            // 换掉旧位图
            SelectObject(self.dc, self.previous);
            if !self.bitmap.is_null() {
                DeleteObject(self.bitmap as _);
            }

            self.previous = SelectObject(self.dc, bitmap as _);
            self.bitmap = bitmap;
            self.size = (w, h);
        }

        Ok(())
    }
}

impl Drop for BackBuffer {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.previous as _);
            if !self.bitmap.is_null() {
                DeleteObject(self.bitmap as _);
            }
            DeleteDC(self.dc);
        }
    }
}

/// 画布内部状态。
struct State {
    model: Model,
    fonts: Fonts,
    /// 字体缩放系数：窗口宽度 / 默认宽度，随 WM_SIZE 更新。
    font_scale: f32,
    sender: Sender<Action>,
    /// 后备缓冲。
    back: BackBuffer,
    /// 品牌 logo（与标题栏、任务栏、托盘同一张图）。
    logo: Option<crate::brand::Logo>,
}

/// 自绘画布。
pub struct Canvas {
    handle: HWND,
    state: Box<State>,
}

impl Canvas {
    /// 在 `parent` 里创建一块 `w × h` 的画布。
    pub fn create(parent: HWND, w: i32, h: i32, sender: Sender<Action>) -> Result<Self, String> {
        register_class_once();

        let mut state = Box::new(State {
            model: Model::default(),
            fonts: Fonts::new(1.0),
            font_scale: 1.0,
            sender,
            back: BackBuffer::new(w, h)?,
            logo: crate::brand::Logo::new(crate::brand::module_handle()),
        });

        // 手册的折行需要量文字，用屏幕 DC 量一次即可
        {
            let content_w = crate::theme::metrics::WINDOW_W
                - crate::theme::metrics::SIDEBAR_W
                - crate::theme::metrics::PAD
                - crate::theme::metrics::PAD;
            let dc = unsafe { GetDC(std::ptr::null_mut()) };
            if !dc.is_null() {
                let (lines, sections) =
                    build_manual(dc, &state.fonts, manual_text_width(content_w));
                state.model.manual_lines = lines;
                state.model.manual_sections = sections;
                unsafe { ReleaseDC(std::ptr::null_mut(), dc) };
            }
        }
        state.model.layout = build_layout(&state.model);

        let mut class_name = CLASS_NAME.encode_utf16().collect::<Vec<u16>>();
        class_name.push(0);
        let window_text: Vec<u16> = vec![0];

        let handle = unsafe {
            CreateWindowExW(
                0,
                class_name.as_ptr(),
                window_text.as_ptr(),
                WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN,
                0,
                0,
                w,
                h,
                parent,
                0 as _,
                module(),
                &mut *state as *mut State as _,
            )
        };

        if handle.is_null() {
            return Err("控制面板画布创建失败".to_string());
        }

        Ok(Self { handle, state })
    }

    /// 底层句柄。
    pub fn handle(&self) -> HWND {
        self.handle
    }

    /// 只读数据。
    pub fn model(&self) -> &Model {
        &self.state.model
    }

    /// 可写数据（不会自动重绘，也不会重算布局）。
    pub fn model_mut(&mut self) -> &mut Model {
        &mut self.state.model
    }

    /// 修改数据并按需重绘。
    ///
    /// **不自动重绘**：面板每 120ms 就会来问一次状态，
    /// 无条件重绘会让整个窗口不停闪烁。这里只改数据，
    /// 需要看到变化的调用方才调 [`Canvas::repaint`]。
    pub fn update<R>(&mut self, apply: impl FnOnce(&mut Model) -> R) -> R {
        let result = apply(&mut self.state.model);
        self.state.model.layout = build_layout(&self.state.model);
        result
    }

    /// 修改数据并重绘。
    pub fn update_and_repaint<R>(&mut self, apply: impl FnOnce(&mut Model) -> R) -> R {
        let result = self.update(apply);
        self.invalidate();
        result
    }

    /// 重绘指定的几块区域。
    ///
    /// 整块重绘会让窗口在鼠标移动时看起来在闪，
    /// 所以能定位到具体区域的改动都走这里。
    pub fn repaint(&self, regions: &[Rect]) {
        invalidate_regions(self.handle, regions);
    }

    /// 重绘。
    pub fn invalidate(&self) {
        unsafe {
            InvalidateRect(self.handle, std::ptr::null(), 0);
        }
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        unsafe {
            DestroyWindow(self.handle);
        }
    }
}

/// 模块句柄。
fn module() -> winapi::shared::minwindef::HINSTANCE {
    unsafe { GetModuleHandleW(std::ptr::null_mut()) }
}

/// 进程内注册一次窗口类。
fn register_class_once() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let mut class_name = CLASS_NAME.encode_utf16().collect::<Vec<u16>>();
        class_name.push(0);

        let mut cls: WNDCLASSEXW = unsafe { core::mem::zeroed() };
        cls.cbSize = size_of::<WNDCLASSEXW>() as _;
        cls.hInstance = module() as _;
        cls.lpfnWndProc = Some(canvas_proc);
        cls.lpszClassName = class_name.as_ptr();

        let atom = unsafe { RegisterClassExW(&cls) };
        if atom == 0 {
            eprintln!("画布窗口类注册失败");
        }
    });
}

/// 只重绘指定区域；`None` 表示整块重绘。
///
/// 局部重绘是关键：整块重绘会让窗口在鼠标移动时看起来在闪。
fn invalidate_region(hwnd: HWND, region: Option<Rect>) {
    unsafe {
        match region {
            Some(rect) => {
                let area = winapi::shared::windef::RECT {
                    left: rect.x,
                    top: rect.y,
                    right: rect.right(),
                    bottom: rect.bottom(),
                };
                InvalidateRect(hwnd, &area, 0);
            }
            None => {
                InvalidateRect(hwnd, std::ptr::null(), 0);
            }
        }
    }
}

/// 只重绘指定的几块区域，空矩形直接跳过。
fn invalidate_regions(hwnd: HWND, regions: &[Rect]) {
    for region in regions {
        if region.w > 0 && region.h > 0 {
            invalidate_region(hwnd, Some(*region));
        }
    }
}

/// 取出窗口上的状态指针。
unsafe fn state_of(hwnd: HWND) -> *mut State {
    unsafe { GetWindowLongPtrW(hwnd, winapi::um::winuser::GWL_USERDATA) as *mut State }
}

/// 窗口过程：绘制 + 鼠标交互。
#[allow(clippy::missing_safety_doc)]
unsafe extern "system" fn canvas_proc(
    hwnd: HWND,
    msg: UINT,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        match msg {
            WM_NCCREATE => {
                // 创建参数里带着状态指针
                let create = lparam as *const winapi::um::winuser::CREATESTRUCTW;
                let state = (*create).lpCreateParams as *mut State;
                SetWindowLongPtrW(hwnd, winapi::um::winuser::GWL_USERDATA, state as isize);
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }

            WM_ERASEBKGND => {
                // 整个背景都由 WM_PAINT 画，交给系统擦会闪
                1
            }

            WM_PAINT => {
                let state = state_of(hwnd);
                if !state.is_null() {
                    // 更新区域必须在 BeginPaint 之前取：
                    // BeginPaint 会把区域标记为已处理，之后再取就是空的了。
                    let mut area: winapi::shared::windef::RECT = core::mem::zeroed();
                    winapi::um::winuser::GetUpdateRect(hwnd, &mut area, 0);
                    let width = area.right - area.left;
                    let height = area.bottom - area.top;

                    let mut paint_struct: winapi::um::winuser::PAINTSTRUCT = core::mem::zeroed();
                    let dc = BeginPaint(hwnd, &mut paint_struct);

                    if width > 0 && height > 0 {
                        let back = (*state).back.dc;

                        // 剪到更新区域：既省了画多余部分的力气，
                        // 也保证贴回屏幕时只有这一块是新的。
                        SelectClipRgn(back, core::ptr::null_mut());
                        IntersectClipRect(back, area.left, area.top, area.right, area.bottom);

                        render(
                            back,
                            &(*state).model,
                            &(*state).fonts,
                            (*state).logo.as_mut(),
                        );

                        // 一次贴图，中间没有半成品，所以不会闪
                        BitBlt(
                            dc, area.left, area.top, width, height, back, area.left, area.top,
                            SRCCOPY,
                        );
                    }

                    EndPaint(hwnd, &paint_struct);
                }
                0
            }

            WM_SIZE => {
                let state = state_of(hwnd);
                if !state.is_null() {
                    // 窗口尺寸变了：布局要重排，整块重画一次。
                    // 后备缓冲也要跟着放大，否则新露出来的区域是旧像素。
                    let width = winapi::shared::minwindef::LOWORD(lparam as DWORD) as i32;
                    let height = winapi::shared::minwindef::HIWORD(lparam as DWORD) as i32;
                    if width > 0 && height > 0 {
                        (*state).model.size = (width, height);

                        // 字号跟着窗口宽度等比缩放：窗口放大文字一起长大，
                        // 封顶 1.4 倍；变化不足 5% 不重建，避免拖拽时频繁抖动。
                        let target =
                            (width as f32 / crate::theme::metrics::WINDOW_W as f32).clamp(1.0, 1.4);
                        if (target - (*state).font_scale).abs() >= 0.05 {
                            (*state).font_scale = target;
                            (*state).model.font_scale = target;
                            (*state).fonts = Fonts::new(target);
                        }

                        // 正文列宽随卡片变，手册的折行要跟着重算
                        let content_w = width
                            - crate::theme::metrics::SIDEBAR_W
                            - crate::theme::metrics::PAD
                            - crate::theme::metrics::PAD;
                        let dc = GetDC(std::ptr::null_mut());
                        if !dc.is_null() {
                            let (lines, sections) =
                                build_manual(dc, &(*state).fonts, manual_text_width(content_w));
                            (*state).model.manual_lines = lines;
                            (*state).model.manual_sections = sections;
                            ReleaseDC(std::ptr::null_mut(), dc);
                        }
                        (*state).model.layout = build_layout(&(*state).model);
                        let _ = (*state).back.resize(width, height);
                        invalidate_region(hwnd, None);
                    }
                }
                0
            }

            WM_MOUSEMOVE => {
                let state = state_of(hwnd);
                if !state.is_null() {
                    let x = winapi::shared::minwindef::LOWORD(lparam as DWORD) as i32;
                    let y = winapi::shared::minwindef::HIWORD(lparam as DWORD) as i32;

                    let previous = (*state).model.hover;
                    let hit = (*state).model.layout.hit_test(x, y);
                    if previous != hit {
                        (*state).model.hover = hit;

                        // 只重绘受影响的那一小块：整块重绘会让面板看起来在闪。
                        // 鼠标在按钮之间来回时，全画布重绘的频率足以造成「疯狂刷新」。
                        let layout = &(*state).model.layout;
                        let mut region = hit.and_then(|hit| layout.rect_of(hit));
                        if let Some(previous) = previous {
                            let old = layout.rect_of(previous);
                            region = match (region, old) {
                                (Some(new), Some(old)) => Some(union(new, old)),
                                (Some(new), None) => Some(new),
                                (None, Some(old)) => Some(old),
                                (None, None) => None,
                            };
                        }
                        invalidate_region(hwnd, region);
                    }

                    // 订阅鼠标离开，离开时清掉悬停态
                    let mut track: TRACKMOUSEEVENT = core::mem::zeroed();
                    track.cbSize = size_of::<TRACKMOUSEEVENT>() as _;
                    track.dwFlags = winapi::um::winuser::TME_LEAVE;
                    track.hwndTrack = hwnd;
                    TrackMouseEvent(&mut track);
                }
                0
            }

            WM_MOUSELEAVE => {
                let state = state_of(hwnd);
                if !state.is_null() {
                    let previous = (*state).model.hover.take();
                    if previous.is_some() {
                        let layout = &(*state).model.layout;
                        invalidate_region(hwnd, previous.and_then(|hit| layout.rect_of(hit)));
                    }
                }
                0
            }

            WM_LBUTTONDOWN => {
                let state = state_of(hwnd);
                if !state.is_null() {
                    let x = winapi::shared::minwindef::LOWORD(lparam as DWORD) as i32;
                    let y = winapi::shared::minwindef::HIWORD(lparam as DWORD) as i32;
                    let hit = (*state).model.layout.hit_test(x, y);
                    (*state).model.pressed = hit;
                    // 焦点挪到画布上：输入框的聚焦框只在真的编辑端口时才出现
                    SetFocus(hwnd);
                    SetCapture(hwnd);
                    let layout = &(*state).model.layout;
                    invalidate_region(hwnd, hit.and_then(|hit| layout.rect_of(hit)));
                }
                0
            }

            WM_LBUTTONUP => {
                let state = state_of(hwnd);
                if !state.is_null() {
                    let x = winapi::shared::minwindef::LOWORD(lparam as DWORD) as i32;
                    let y = winapi::shared::minwindef::HIWORD(lparam as DWORD) as i32;
                    let hit = (*state).model.layout.hit_test(x, y);
                    let pressed = (*state).model.pressed.take();

                    if let Some(Hit::ManualToc(index)) = hit {
                        // 目录：跳到对应小节，纯界面状态，不走动作通道。
                        // 高亮项随滚动位置变化，整个目录都要重绘，
                        // 否则原来那一项的高亮会一直挂着。
                        let model = &mut (*state).model;
                        if let Some(section) = model.manual_sections.get(index) {
                            model.manual_scroll = section.start;
                            let mut regions = model.layout.manual_toc.clone();
                            regions.push(model.layout.manual_list);
                            invalidate_regions(hwnd, &regions);
                        }
                    } else if pressed == Some(Hit::PortField) && hit == Some(Hit::PortField) {
                        // 点端口输入框是进入编辑，不是立刻应用
                        let model = &mut (*state).model;
                        model.port_editing = true;
                        model.port_original = model.port_text.clone();
                        model.caret = model.port_text.chars().count();
                    } else if pressed.is_some() && pressed == hit {
                        // 点到别处就算结束编辑，但这次点击本身照常生效
                        if (*state).model.port_editing && hit != Some(Hit::PortField) {
                            (*state).model.port_editing = false;
                        }
                        if let Some(action) = hit.and_then(|hit| action_for(hit, &(*state).model)) {
                            let _ = (*state).sender.send(action);
                        }
                    }

                    // 下拉弹层：展开、选择、点外面收起
                    if dropdown_click(&mut (*state).model, hit, &(*state).sender) {
                        (*state).model.layout = build_layout(&(*state).model);
                        invalidate_region(hwnd, None);
                    }

                    ReleaseCapture();

                    // 只重绘受影响的几块：按下态消失、可能进入编辑态
                    let layout = &(*state).model.layout;
                    let mut regions: Vec<Rect> = Vec::new();
                    for candidate in [pressed, hit] {
                        if let Some(rect) = candidate.and_then(|hit| layout.rect_of(hit)) {
                            regions.push(rect);
                        }
                    }
                    // 端口输入框的下划线颜色随编辑态变化，离开时也要重绘
                    regions.push(layout.port_edit);
                    invalidate_regions(hwnd, &regions);
                }
                0
            }

            WM_CHAR => {
                let state = state_of(hwnd);
                if !state.is_null() {
                    let model = &mut (*state).model;
                    if model.port_editing {
                        let ch = char::from_u32(wparam as u32);
                        if let Some(ch) = ch {
                            insert_digit(model, ch);
                        }
                        // 只有输入框里的文字和光标变了
                        let region = model.layout.port_edit;
                        invalidate_regions(hwnd, &[region]);
                    }
                }
                0
            }

            WM_KEYDOWN => {
                let state = state_of(hwnd);
                if !state.is_null() {
                    let key = wparam as i32;
                    let model = &mut (*state).model;

                    if model.port_editing {
                        match key {
                            winapi::um::winuser::VK_BACK => backspace(model),
                            winapi::um::winuser::VK_RETURN => {
                                let text = model.port_text.clone();
                                model.port_editing = false;
                                let _ = (*state).sender.send(Action::ApplyPortText(text));
                            }
                            winapi::um::winuser::VK_ESCAPE => {
                                // 取消编辑：回到原来的端口号
                                model.port_text = model.port_original.clone();
                                model.port_editing = false;
                            }
                            _ => {}
                        }
                        let region = model.layout.port_edit;
                        invalidate_regions(hwnd, &[region]);
                    } else if key == winapi::um::winuser::VK_ESCAPE
                        && model.credential.dropdown.is_some()
                    {
                        // Esc 收起下拉
                        model.credential.dropdown = None;
                        model.layout = build_layout(model);
                        invalidate_region(hwnd, None);
                    }
                }
                0
            }

            WM_MOUSEWHEEL => {
                let state = state_of(hwnd);
                if !state.is_null() {
                    // 滚轮增量在高字里，是有符号的 16 位数
                    let delta = winapi::shared::minwindef::HIWORD(wparam as DWORD) as i16 as i32;
                    let step = -delta / 120;

                    match (*state).model.page {
                        Page::Fields => {
                            if step != 0 {
                                let model = &mut (*state).model;
                                // 能滚多远取决于当前能看到几行（窗口高度会变）
                                let max =
                                    model.fields.len().saturating_sub(model.layout.visible_rows)
                                        as i32;
                                let next = (model.scroll + step).clamp(0, max.max(0));
                                if next != model.scroll {
                                    model.scroll = next;
                                    // 布局随滚动位置变化，重算后再重绘列表区
                                    let layout = build_layout(model);
                                    model.layout = layout;
                                    let region = model.layout.field_list;
                                    invalidate_regions(hwnd, &[region]);
                                }
                            }
                        }
                        Page::Manual => {
                            if step != 0 {
                                let model = &mut (*state).model;
                                // 滚动以像素计，一格滚轮约两行
                                let next = (model.manual_scroll + step * 60).max(0);
                                if next != model.manual_scroll {
                                    model.manual_scroll = next;
                                    // 目录高亮跟随正文位置，目录行也要重绘
                                    let mut regions = model.layout.manual_toc.clone();
                                    regions.push(model.layout.manual_list);
                                    invalidate_regions(hwnd, &regions);
                                }
                            }
                        }
                        Page::Service => {
                            // 弹层展开时，滚轮滚的是弹层里的条目
                            let mut changed = false;
                            if let Some(dropdown) = (*state).model.credential.dropdown.as_mut() {
                                if step != 0 {
                                    let max = (dropdown.items.len() as i32 - 8).max(0);
                                    let next = (dropdown.scroll + step).clamp(0, max);
                                    changed = next != dropdown.scroll;
                                    dropdown.scroll = next;
                                }
                            }
                            if changed {
                                let model = &mut (*state).model;
                                model.layout = build_layout(model);
                                invalidate_region(hwnd, None);
                            }
                        }
                    }
                }
                0
            }

            WM_SETCURSOR => {
                let state = state_of(hwnd);
                if !state.is_null() && (*state).model.hover.is_some() {
                    let cursor = LoadCursorW(std::ptr::null_mut(), IDC_HAND);
                    SetCursor(cursor);
                    return 1;
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }

            WM_NCDESTROY => {
                SetWindowLongPtrW(hwnd, winapi::um::winuser::GWL_USERDATA, 0);
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }

            WM_DESTROY => 0,

            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// 命中区域对应的动作。
fn action_for(hit: Hit, model: &Model) -> Option<Action> {
    Some(match hit {
        Hit::NavService => Action::Navigate(Page::Service),
        Hit::NavFields => Action::Navigate(Page::Fields),
        Hit::NavManual => Action::Navigate(Page::Manual),
        Hit::ThemeToggle => Action::ToggleTheme,
        Hit::Start => Action::Start,
        Hit::Stop => Action::Stop,
        Hit::OpenBrowser => Action::OpenBrowser,
        Hit::OpenData => Action::OpenData,
        Hit::Firewall => Action::Firewall,
        Hit::Autostart => Action::ToggleAutostart,
        Hit::ApplyPort => Action::ApplyPort,
        // 端口输入框的点击在画布内部处理（进入编辑），不走动作通道
        Hit::PortField => Action::ApplyPort,
        Hit::Cycle => Action::CycleField,
        Hit::Save => Action::SaveFields,
        Hit::Rescan => Action::RescanFields,
        Hit::CopyUrl(index) => Action::CopyUrl(index),
        Hit::CopyAll => Action::CopyAll,
        Hit::ViewLog => Action::ViewLog,
        // 手册目录的跳转由画布自己处理，不走动作通道
        Hit::ManualToc(_) => return None,
        // 下拉控件的展开收起在画布内部完成
        Hit::CredentialAccount | Hit::CredentialField => return None,
        Hit::DropdownItem(_) => return None,
        Hit::CredentialReadFields => Action::CredentialReadFields,
        Hit::CredentialSave => Action::CredentialSave,
        // 命中的是「可视行号」，选中的却是字段的绝对序号，滚动后两者不相等
        Hit::FieldRow(row) => Action::SelectField(model.scroll as usize + row),
    })
}

/// 在端口输入框里插入一个数字。
///
/// 端口号只能是数字，且最多五位——非法字符直接忽略，
/// 比弹一个提示框更顺手。
fn insert_digit(model: &mut Model, ch: char) {
    if !ch.is_ascii_digit() {
        return;
    }

    let mut chars: Vec<char> = model.port_text.chars().collect();
    if chars.len() >= 5 {
        return;
    }

    let caret = model.caret.min(chars.len());
    chars.insert(caret, ch);
    model.port_text = chars.into_iter().collect();
    model.caret = caret + 1;
}

/// 删掉端口输入框里光标前面的一位。
fn backspace(model: &mut Model) {
    let mut chars: Vec<char> = model.port_text.chars().collect();
    let caret = model.caret.min(chars.len());
    if caret == 0 {
        return;
    }

    chars.remove(caret - 1);
    model.port_text = chars.into_iter().collect();
    model.caret = caret - 1;
}

/// 处理下拉弹层的点击：展开、选择、点外面收起。
///
/// 返回 true 表示界面状态变了，需要重排布局并重绘。
fn dropdown_click(model: &mut Model, hit: Option<Hit>, sender: &Sender<Action>) -> bool {
    match hit {
        Some(Hit::DropdownItem(index)) => {
            let kind = model
                .credential
                .dropdown
                .as_ref()
                .map(|dropdown| dropdown.kind);

            model.credential.dropdown = None;

            let action = match kind {
                Some(DropdownKind::Account) => Action::CredentialSelectAccount(index),
                Some(DropdownKind::Field) => Action::CredentialSelectField(index),
                None => return false,
            };
            let _ = sender.send(action);
            true
        }
        Some(Hit::CredentialAccount) => {
            toggle_dropdown(model, DropdownKind::Account);
            true
        }
        Some(Hit::CredentialField) => {
            toggle_dropdown(model, DropdownKind::Field);
            true
        }
        _ => {
            if model.credential.dropdown.is_some() {
                model.credential.dropdown = None;
                true
            } else {
                false
            }
        }
    }
}

/// 展开或收起下拉框。
fn toggle_dropdown(model: &mut Model, kind: DropdownKind) {
    if model
        .credential
        .dropdown
        .as_ref()
        .map(|dropdown| dropdown.kind)
        == Some(kind)
    {
        model.credential.dropdown = None;
        return;
    }

    let (items, selected) = match kind {
        // 账号与密码两个下拉共用同一份表头候选，只是各自记住选中项
        DropdownKind::Account => (model.credential.fields.clone(), model.credential.account),
        DropdownKind::Field => (model.credential.fields.clone(), model.credential.field),
    };

    // 展开时把选中的那一项露出来
    let scroll = selected
        .map(|index| (index + 3).saturating_sub(8))
        .unwrap_or(0) as i32;

    model.credential.dropdown = Some(Dropdown {
        kind,
        items,
        selected,
        scroll,
    });
}

/// 按钮的视觉层级。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ButtonStyle {
    /// 主按钮：强调色实底
    Primary,
    /// 次按钮：底色 + 描边
    Secondary,
}

/// 计算整块面板的布局。
///
/// 布局跟着窗口尺寸走：侧边栏与内边距是固定的（规范要求导航固定），
/// 内容区随窗口伸缩，字段列表能显示几行按可用高度算。
fn build_layout(model: &Model) -> Layout {
    use theme::metrics;

    let mut layout = Layout::default();

    let (width, height) = model.size;
    let content_x = metrics::SIDEBAR_W + metrics::PAD;
    let content_w = (width - content_x - metrics::PAD).max(240);

    // 内容区与顶栏标题：换页时只有这两块会变
    layout.content = Rect::new(
        metrics::SIDEBAR_W,
        metrics::TOPBAR_H,
        width - metrics::SIDEBAR_W,
        height - metrics::TOPBAR_H,
    );
    layout.title_area = Rect::new(content_x, 14, 240, 48);

    // 左侧导航
    layout.nav[0] = Rect::new(20, 112, metrics::SIDEBAR_W - 40, 46);
    layout.nav[1] = Rect::new(20, 168, metrics::SIDEBAR_W - 40, 46);
    layout.nav[2] = Rect::new(20, 224, metrics::SIDEBAR_W - 40, 46);
    layout.nav_area = Rect::new(0, layout.nav[0].y, metrics::SIDEBAR_W, 162);

    // 顶栏右侧：主题按钮与状态 pill
    layout.theme_button = Rect::new(width - metrics::PAD - 68, 24, 68, 36);
    layout.status_pill = Rect::new(width - metrics::PAD - 68 - 12 - 148, 24, 148, 36);
    layout.notice = Rect::new(
        content_x + 260,
        28,
        (layout.status_pill.x - 16 - (content_x + 260)).max(0),
        26,
    );

    match model.page {
        Page::Service => {
            // KPI 行：服务状态 / HTTP 端口 / 工资月份
            // 卡高跟字号缩放走：字放大了卡片内容才不会挤在一起
            let kpi_gap = metrics::GAP;
            let kpi_w = (content_w - kpi_gap * 2) / 3;
            let kpi_h = (124.0 * model.font_scale).round() as i32;
            for index in 0..3 {
                layout.kpi[index] = Rect::new(
                    content_x + index as i32 * (kpi_w + kpi_gap),
                    metrics::TOPBAR_H + metrics::PAD,
                    kpi_w,
                    kpi_h,
                );
            }
            layout.kpi_row = Rect::new(
                content_x,
                metrics::TOPBAR_H + metrics::PAD,
                content_w,
                kpi_h,
            );

            // 两栏：左边是账号密码设置，右边是访问地址。
            // 账号密码与访问地址是一对——一个管「怎么进去」，一个管「从哪进」，
            // 放在一起比各占一整行省地方。
            let row_y = metrics::TOPBAR_H + metrics::PAD + kpi_h + metrics::GAP;
            let actions_h = (190.0 * model.font_scale).round() as i32;
            // 窗口更高时这一行跟着长高，多出来的空间留在最下面
            let row_h =
                (height - row_y - metrics::GAP - actions_h - metrics::GAP - 22 - metrics::PAD)
                    .clamp(232, 420);

            // 左栏只需要「标签 + 240 宽下拉」，右栏要放完整的局域网地址——
            // 地址被截断就失去意义了，所以右栏拿 56%。
            let left_w = content_w * 44 / 100;
            let right_x = content_x + left_w + metrics::GAP;
            let right_w = content_w - left_w - metrics::GAP;

            layout.credential_card = Rect::new(content_x, row_y, left_w, row_h);
            layout.access_card = Rect::new(right_x, row_y, right_w, row_h);

            // ---- 账号密码卡 ----
            // 账号与密码各是一个下拉，列出表头字段；
            // 选好后点「保存」写进登录字段映射，网页登录立即按它校验。
            let inner_x = content_x + 24;
            let inner_w = left_w - 48;

            layout.credential_account = Rect::new(inner_x + 64, row_y + 68, 240, 34);
            layout.credential_field = Rect::new(inner_x + 64, row_y + 116, 240, 34);

            layout.credential_status = Rect::new(inner_x, row_y + 162, inner_w, 16);

            layout.credential_buttons.push((
                Hit::CredentialSave,
                Rect::new(inner_x, row_y + 188, 120, 38),
            ));
            layout.credential_buttons.push((
                Hit::CredentialReadFields,
                Rect::new(inner_x + 136, row_y + 188, 120, 38),
            ));

            // ---- 访问地址卡 ----
            // URL 值的宽度跟着卡片走：窗口放大后完整地址要能显示出来，
            // 不能在「...」里截断（缩放后等宽字体也变宽，固定宽会截断）。
            let access_inner_x = right_x + 24;
            let row_top = row_y + 76;

            let url_label_w = 64;
            let url_w = (right_w - 48 - url_label_w - 46 - 36).max(120);
            layout.url_labels[0] = Rect::new(access_inner_x, row_top, url_label_w, 30);
            layout.url_values[0] = Rect::new(access_inner_x + url_label_w + 4, row_top, url_w, 30);
            layout.copy_buttons[0] = Rect::new(
                access_inner_x + url_label_w + 4 + url_w + 16,
                row_top + 1,
                46,
                28,
            );
            layout.url_labels[1] = Rect::new(access_inner_x, row_top + 38, url_label_w, 30);
            layout.url_values[1] =
                Rect::new(access_inner_x + url_label_w + 4, row_top + 38, url_w, 30);
            layout.copy_buttons[1] = Rect::new(
                access_inner_x + url_label_w + 4 + url_w + 16,
                row_top + 39,
                46,
                28,
            );
            layout.port_edit = Rect::new(access_inner_x + 110, row_top + 74, 104, 30);
            layout.apply_button = Rect::new(access_inner_x + 226, row_top + 72, 88, 32);
            layout.copy_all_button = Rect::new(right_x + right_w - 120, row_y + 14, 96, 28);

            // ---- 快捷操作卡片 ----
            let actions_y = row_y + row_h + metrics::GAP;
            layout.actions_card = Rect::new(content_x, actions_y, content_w, actions_h);

            // 右侧一栏留给「支持访问的网络」（来自 data/networks.txt）
            let network_x = content_x + content_w - 244;
            layout.network_label = Rect::new(network_x, actions_y + 20, 204, 18);
            for (index, _) in model.networks.iter().enumerate().take(3) {
                layout.network_rows.push(Rect::new(
                    network_x,
                    actions_y + 48 + index as i32 * 24,
                    204,
                    20,
                ));
            }

            let button_gap = metrics::GAP;
            let button_w = (network_x - content_x - 48 - button_gap * 2) / 3;
            let actions: [(Hit, bool); 6] = [
                (Hit::Start, true),
                (Hit::Stop, false),
                (Hit::OpenBrowser, false),
                (Hit::OpenData, false),
                (Hit::Firewall, false),
                (Hit::Autostart, false),
            ];
            for (index, (hit, _)) in actions.iter().enumerate() {
                let column = index % 3;
                let row = index / 3;
                let rect = Rect::new(
                    content_x + 24 + column as i32 * (button_w + button_gap),
                    actions_y + 68 + row as i32 * 52,
                    button_w,
                    42,
                );
                layout.action_buttons.push((*hit, rect));
            }

            // 底部提示紧跟卡片，间距与其它留白一致；
            // 窗口更高时多出来的空间留在最下面，不把提示和卡片拆开。
            // 提示行与「查看日志」的高度跟字号缩放走，缩放后文字才不会被裁。
            let hint_h = (22.0 * model.font_scale).round() as i32;
            let log_h = (28.0 * model.font_scale).round() as i32;
            layout.hint = Rect::new(
                content_x,
                actions_y + actions_h + metrics::GAP,
                content_w - 100,
                hint_h,
            );
            // 「查看日志」放在提示行右侧，不占快捷操作的格子
            layout.view_log_button = Rect::new(
                content_x + content_w - 84,
                actions_y + actions_h + metrics::GAP - 2,
                84,
                log_h,
            );

            // 下拉弹层：贴着控件往下弹，下面放不下就往上弹
            if let Some(dropdown) = &model.credential.dropdown {
                let anchor = match dropdown.kind {
                    DropdownKind::Account => layout.credential_account,
                    DropdownKind::Field => layout.credential_field,
                };

                let item_h = 34;
                let visible = dropdown.items.len().clamp(1, 8);
                let popup_h = visible as i32 * item_h + 8;
                let mut popup_y = anchor.bottom() + 4;
                if popup_y + popup_h > height - 8 {
                    popup_y = (anchor.y - popup_h - 4).max(metrics::TOPBAR_H);
                }

                layout.dropdown = Rect::new(anchor.x, popup_y, anchor.w, popup_h);

                let scroll = dropdown.scroll.max(0) as usize;
                for offset in 0..visible {
                    let index = scroll + offset;
                    if index >= dropdown.items.len() {
                        break;
                    }
                    layout.dropdown_items.push(Rect::new(
                        anchor.x,
                        popup_y + 4 + offset as i32 * item_h,
                        anchor.w,
                        item_h,
                    ));
                }
            }
        }

        Page::Fields => {
            let card_y = metrics::TOPBAR_H + metrics::PAD;

            // 行数按可用高度算：窗口越高，能看到的字段越多。
            // 下方要给按钮行留出位置。
            let button_h = 44;
            let available = height - card_y - metrics::PAD - button_h - metrics::GAP;
            let rows = ((available - metrics::LIST_HEADER_H - metrics::LIST_FOOTER_H)
                / metrics::ROW_H)
                .clamp(3, 20);
            layout.visible_rows = rows as usize;

            let card_h = metrics::LIST_HEADER_H + rows * metrics::ROW_H + metrics::LIST_FOOTER_H;
            layout.fields_card = Rect::new(content_x, card_y, content_w, card_h);

            // 列表区连同右侧滚动条轨道：滚动时只重绘这一块
            layout.field_list = Rect::new(
                content_x + 24,
                card_y + metrics::LIST_HEADER_H,
                content_w - 48 + 16,
                rows * metrics::ROW_H,
            );

            for index in 0..layout.visible_rows {
                let row_index = model.scroll as usize + index;
                if row_index >= model.fields.len() {
                    break;
                }
                let rect = Rect::new(
                    content_x + 24,
                    card_y + metrics::LIST_HEADER_H + index as i32 * metrics::ROW_H,
                    content_w - 48,
                    metrics::ROW_H - 4,
                );
                layout.field_rows.push(rect);
            }

            let button_y = card_y + card_h + metrics::GAP;
            layout
                .field_buttons
                .push((Hit::Cycle, Rect::new(content_x, button_y, 148, 42)));
            layout
                .field_buttons
                .push((Hit::Save, Rect::new(content_x + 164, button_y, 116, 42)));
            layout
                .field_buttons
                .push((Hit::Rescan, Rect::new(content_x + 296, button_y, 148, 42)));
            layout.hint = Rect::new(content_x + 460, button_y, content_w - 460, 42);
        }

        Page::Manual => {
            // 手册整块铺满内容区
            let card_y = metrics::TOPBAR_H + metrics::PAD;
            let card_h = height - card_y - metrics::PAD;
            layout.manual_card = Rect::new(content_x, card_y, content_w, card_h);

            // 左侧目录 + 右侧正文
            let toc_x = content_x + 20;
            layout.manual_toc_title = Rect::new(toc_x, card_y + 72, MANUAL_TOC_W, 16);
            for (index, _) in model.manual_sections.iter().enumerate() {
                layout.manual_toc.push(Rect::new(
                    toc_x,
                    card_y + 96 + index as i32 * 26,
                    MANUAL_TOC_W,
                    22,
                ));
            }
            layout.manual_divider =
                Rect::new(toc_x + MANUAL_TOC_W + 16, card_y + 72, 1, card_h - 92);
            layout.manual_list = Rect::new(
                toc_x + MANUAL_TOC_W + 36,
                card_y + 68,
                manual_text_width(content_w),
                card_h - 88,
            );
            for (index, rect) in layout.manual_toc.iter().enumerate() {
                layout.hits.push((Hit::ManualToc(index), *rect));
            }
        }
    }

    // 命中区域：后加入的在上层
    layout.hits.push((Hit::NavService, layout.nav[0]));
    layout.hits.push((Hit::NavFields, layout.nav[1]));
    layout.hits.push((Hit::NavManual, layout.nav[2]));
    layout
        .hits
        .push((Hit::CredentialAccount, layout.credential_account));
    layout
        .hits
        .push((Hit::CredentialField, layout.credential_field));
    for (hit, rect) in &layout.credential_buttons {
        layout.hits.push((*hit, *rect));
    }
    layout.hits.push((Hit::CopyAll, layout.copy_all_button));
    layout.hits.push((Hit::CopyUrl(0), layout.copy_buttons[0]));
    layout.hits.push((Hit::CopyUrl(1), layout.copy_buttons[1]));
    layout.hits.push((Hit::ViewLog, layout.view_log_button));
    layout.hits.push((Hit::ThemeToggle, layout.theme_button));
    for (hit, rect) in &layout.action_buttons {
        layout.hits.push((*hit, *rect));
    }
    layout.hits.push((Hit::ApplyPort, layout.apply_button));
    layout.hits.push((Hit::PortField, layout.port_edit));
    for (hit, rect) in &layout.field_buttons {
        layout.hits.push((*hit, *rect));
    }
    for (index, rect) in layout.field_rows.iter().enumerate() {
        layout.hits.push((Hit::FieldRow(index), *rect));
    }

    // 下拉弹层里的条目压在最上层：弹层可能盖住卡片上的其它控件
    if let Some(dropdown) = &model.credential.dropdown {
        let scroll = dropdown.scroll.max(0) as usize;
        for (offset, rect) in layout.dropdown_items.iter().enumerate() {
            layout
                .hits
                .push((Hit::DropdownItem(scroll + offset), *rect));
        }
    }

    layout
}

/// 手册里一行文字的样式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManualKind {
    /// 小节标题
    Title,
    /// 正文
    Body,
    /// 条目
    Item,
}

/// 手册的一行（已经按宽度折好）。
#[derive(Debug, Clone)]
pub struct ManualLine {
    /// 行的样式
    pub kind: ManualKind,
    /// 文字
    pub text: String,
    /// 属于第几个小节
    pub section: usize,
    /// 这一行在手册里的纵向偏移（像素）
    pub y: i32,
}

/// 手册的一个小节。
#[derive(Debug, Clone)]
pub struct ManualSection {
    /// 小节标题
    pub title: String,
    /// 这一节第一行的纵向偏移（像素）
    pub start: i32,
}

/// 使用手册的内容。
///
/// 写死在这里而不是读文件：面板是常驻的小程序，
/// 手册要离线可看、随手可查，不能依赖某个文件存在。
/// 每行控制在 34 个汉字以内，最小窗口宽度下也不会被截断。
const MANUAL: &[(&str, &[(&str, ManualKind)])] = &[
    (
        "这是什么",
        &[
            (
                "员工工资查询系统。员工用浏览器或手机打开网页，",
                ManualKind::Body,
            ),
            ("用工号和口令查自己的工资条。", ManualKind::Body),
            (
                "本系统只在本机或局域网内使用，不面向公网。",
                ManualKind::Body,
            ),
        ],
    ),
    (
        "运维先看这几个区块",
        &[
            (
                "服务控制：服务在不在跑、端口是多少、常用操作。",
                ManualKind::Body,
            ),
            (
                "字段显示：决定工资条上显示哪些字段，保存即生效。",
                ManualKind::Body,
            ),
            (
                "登录凭据：就在服务控制页，保管一组网页登录账号口令。",
                ManualKind::Body,
            ),
            ("使用手册：就是这一页。", ManualKind::Body),
        ],
    ),
    (
        "员工怎么查工资",
        &[
            ("浏览器打开面板上的「局域网访问」地址。", ManualKind::Item),
            ("用工号和口令登录。口令忘了找管理员重置。", ManualKind::Item),
            ("顶部选月份，下面就是工资条。", ManualKind::Item),
            ("要留底可以「导出图片」或直接打印。", ManualKind::Item),
        ],
    ),
    (
        "字段显示的三种策略",
        &[
            (
                "跟随：值为空或为 0 时不显示，其余显示（默认）。",
                ManualKind::Body,
            ),
            ("显示：不管有没有值都显示。", ManualKind::Body),
            ("隐藏：永远不显示。", ManualKind::Body),
            (
                "改法：字段显示页选中一行 → 切换状态 → 保存。",
                ManualKind::Item,
            ),
            ("保存后立刻生效，不用重启服务。", ManualKind::Item),
        ],
    ),
    (
        "账号密码怎么设置",
        &[
            (
                "服务控制页左上那张卡：选择网页登录用哪些字段。",
                ManualKind::Body,
            ),
            (
                "账号：从表头里选一个字段，登录时账号就是这一列的内容。",
                ManualKind::Body,
            ),
            (
                "密码：同样从表头里选，登录口令就是这一列的内容，明文和加密口令都认。",
                ManualKind::Body,
            ),
            (
                "字段读取：工资表换了列名之后点一下，重新读一遍表头。",
                ManualKind::Item,
            ),
            (
                "保存：写进 data/login_fields.json，对网页登录立即生效。",
                ManualKind::Item,
            ),
        ],
    ),
    (
        "开机自启与防火墙",
        &[
            (
                "开机自启：写注册表 Run 项，机器重启后自动拉起。",
                ManualKind::Body,
            ),
            ("放行防火墙：给当前端口加一条入站规则，", ManualKind::Body),
            ("同事才能从局域网访问。", ManualKind::Body),
            (
                "改端口前先停服务：端口写在 data/port.txt，",
                ManualKind::Body,
            ),
            ("HTTP 端口 +1 是 HTTPS，两个都要放行。", ManualKind::Body),
        ],
    ),
    (
        "常见问题",
        &[
            ("登录提示「认证失败」：工号或口令不对；", ManualKind::Body),
            ("连续错多次会被锁几分钟，等一会儿再试。", ManualKind::Body),
            (
                "查不到工资：确认选对了月份、姓名在工资表里、",
                ManualKind::Body,
            ),
            ("而且该字段没有被设成「隐藏」。", ManualKind::Body),
            (
                "网页打不开：先看面板的服务状态，必要时启动服务。",
                ManualKind::Body,
            ),
            (
                "局域网访问不了：防火墙没放行，或不在同一网段。",
                ManualKind::Body,
            ),
        ],
    ),
    (
        "数据与备份",
        &[
            (
                "data/ 目录里都是运行时数据：员工列表、工资表、",
                ManualKind::Body,
            ),
            ("字段配置、证书、审计日志。", ManualKind::Body),
            ("备份直接复制整个 data 目录。", ManualKind::Body),
            (
                "审计日志记录了每个人的查询动作，注意保密。",
                ManualKind::Body,
            ),
        ],
    ),
];

/// 手册正文的排版参数。
const MANUAL_TOC_W: i32 = 168;
const MANUAL_LINE_H: i32 = 26;
const MANUAL_TITLE_H: i32 = 42;

/// 手册正文列的宽度。
///
/// 左侧目录 + 右侧正文，正文宽度随卡片一起变；
/// 右边还要给滚动条和内边距留位置。
fn manual_text_width(content_w: i32) -> i32 {
    (content_w - 268).max(240)
}

/// 把手册折行，并算出每一行的纵向偏移与每个小节的起点。
fn build_manual(dc: HDC, fonts: &Fonts, width: i32) -> (Vec<ManualLine>, Vec<ManualSection>) {
    let mut lines: Vec<ManualLine> = Vec::new();
    let mut sections: Vec<ManualSection> = Vec::new();
    let mut y = 0;

    for (index, (title, body)) in MANUAL.iter().enumerate() {
        sections.push(ManualSection {
            title: (*title).to_string(),
            start: y,
        });

        lines.push(ManualLine {
            kind: ManualKind::Title,
            text: (*title).to_string(),
            section: index,
            y,
        });
        y += MANUAL_TITLE_H;

        for (text, kind) in body.iter() {
            for wrapped in wrap_text(dc, text, fonts.body(), width) {
                lines.push(ManualLine {
                    kind: *kind,
                    text: wrapped,
                    section: index,
                    y,
                });
                y += MANUAL_LINE_H;
            }
        }
        // 小节之间留一点空
        y += 8;
    }

    (lines, sections)
}

/// 按宽度把一段文字折成若干行。
///
/// 中文逐字断行，英文尽量在空格处断开——这里只做排版，
/// 不追求出版级的断词质量。
fn wrap_text(dc: HDC, text: &str, font: HFONT, max_width: i32) -> Vec<String> {
    if paint::text_width(dc, text, font) <= max_width {
        return vec![text.to_string()];
    }

    let mut lines = Vec::new();
    let mut current = String::new();

    for ch in text.chars() {
        let candidate = format!("{current}{ch}");
        if paint::text_width(dc, &candidate, font) > max_width && !current.is_empty() {
            // 英文单词尽量不拆开
            let cut = current
                .rfind(' ')
                .filter(|index| *index > current.len() / 2);
            match cut {
                Some(index) => {
                    lines.push(current[..index].to_string());
                    current = format!("{}{ch}", &current[index + 1..]);
                }
                None => {
                    lines.push(std::mem::take(&mut current));
                    current.push(ch);
                }
            }
        } else {
            current = candidate;
        }
    }

    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// 画一整块面板。
fn render(dc: HDC, model: &Model, fonts: &Fonts, logo: Option<&mut crate::brand::Logo>) {
    let palette = Palette::of(model.theme);
    let layout = &model.layout;

    // 画布底色
    paint::fill_rect(
        dc,
        Rect::new(0, 0, model.size.0, model.size.1),
        palette.canvas,
    );

    render_sidebar(dc, model, fonts, &palette, logo);
    render_topbar(dc, model, fonts, &palette, layout);

    match model.page {
        Page::Service => {
            render_service(dc, model, fonts, &palette, layout);
            // 账号密码设置与服务同属一个页面：都在「服务控制」里
            render_credential_card(dc, model, fonts, &palette, layout);
        }
        Page::Fields => render_fields(dc, model, fonts, &palette, layout),
        Page::Manual => render_manual(dc, model, fonts, &palette, layout),
    }

    // 下拉弹层最后画：它要盖在页面内容上面
    render_dropdown(dc, model, fonts, &palette, layout);
}

/// 使用手册页面。
fn render_manual(dc: HDC, model: &Model, fonts: &Fonts, palette: &Palette, layout: &Layout) {
    let card = layout.manual_card;
    paint_card(dc, card, palette);
    paint_card_title(dc, card, "使用说明", "HOW TO USE", fonts, palette);

    let area = layout.manual_list;
    let total = model.manual_lines.len();
    let content_h = model
        .manual_lines
        .last()
        .map(|line| line.y + MANUAL_LINE_H)
        .unwrap_or(0);
    let max_scroll = (content_h - area.h).max(0);
    let scroll = model.manual_scroll.clamp(0, max_scroll);

    // ---- 左侧目录 ----
    paint::text(
        dc,
        layout.manual_toc_title,
        "目录 CONTENTS",
        fonts.eyebrow(),
        palette.ink_tertiary,
        Align::Left,
        1,
    );
    let current = model
        .manual_sections
        .iter()
        .rposition(|section| section.start <= scroll)
        .unwrap_or(0);
    for (index, section) in model.manual_sections.iter().enumerate() {
        let Some(rect) = layout.manual_toc.get(index) else {
            break;
        };
        let active = index == current;
        let hovered = model.hover == Some(Hit::ManualToc(index));
        let pressed = model.pressed == Some(Hit::ManualToc(index));

        // 与侧边栏导航同一套语言：选中给底色 + 左侧强调条，
        // 悬停给浮层底，按住时按选中画——点下去看得见反馈。
        if active || pressed {
            paint::fill_round(dc, *rect, palette.radius_button, palette.surface_row);
            paint::fill_rect(
                dc,
                Rect::new(rect.x, rect.y + 5, 3, rect.h - 10),
                palette.primary,
            );
        } else if hovered {
            paint::fill_round(dc, *rect, palette.radius_button, palette.surface_raised);
        }

        paint::text(
            dc,
            Rect::new(rect.x + 10, rect.y, 26, rect.h),
            &format!("{:02}", index + 1),
            fonts.small(),
            palette.primary,
            Align::Left,
            0,
        );
        paint::text(
            dc,
            Rect::new(rect.x + 38, rect.y, rect.w - 38, rect.h),
            &section.title,
            fonts.small(),
            if active || pressed {
                palette.ink
            } else if hovered {
                palette.ink_muted
            } else {
                palette.ink_tertiary
            },
            Align::Left,
            0,
        );
    }

    // 目录与正文之间的竖线
    paint::vline(
        dc,
        layout.manual_divider.x,
        layout.manual_divider.y,
        layout.manual_divider.h,
        palette.hairline,
    );

    // ---- 右侧正文 ----
    for line in &model.manual_lines {
        let y = area.y + line.y - scroll;
        if y + MANUAL_LINE_H < area.y || y > area.bottom() {
            continue;
        }

        match line.kind {
            ManualKind::Title => {
                // 小节之间一条分隔线，标题才不会混在正文里
                if line.y > 0 {
                    paint::hline(dc, area.x, y - 16, area.w, palette.hairline);
                }
                // 编号用强调色，标题用主色，比正文重一档
                paint::text(
                    dc,
                    Rect::new(area.x, y + 2, 30, 22),
                    &format!("{:02}", line.section + 1),
                    fonts.small(),
                    palette.primary,
                    Align::Left,
                    0,
                );
                paint::text(
                    dc,
                    Rect::new(area.x + 32, y, area.w - 32, 24),
                    &line.text,
                    fonts.section(),
                    palette.ink,
                    Align::Left,
                    0,
                );
            }
            ManualKind::Body => {
                paint::text(
                    dc,
                    Rect::new(area.x, y, area.w, MANUAL_LINE_H),
                    &line.text,
                    fonts.body(),
                    palette.ink_muted,
                    Align::Left,
                    0,
                );
            }
            ManualKind::Item => {
                // 条目：一个小圆点 + 缩进的文字
                paint::fill_circle(dc, area.x + 5, y + 13, 2, palette.primary);
                paint::text(
                    dc,
                    Rect::new(area.x + 18, y, area.w - 18, MANUAL_LINE_H),
                    &line.text,
                    fonts.body(),
                    palette.ink_muted,
                    Align::Left,
                    0,
                );
            }
        }
    }

    // 右边一条细滚动条，提示还有多少内容
    if max_scroll > 0 && total > 0 {
        let track = Rect::new(area.right() + 14, area.y, 4, area.h);
        paint::fill_round(dc, track, 2, palette.hairline);
        let ratio = area.h as f32 / content_h as f32;
        let thumb_h = ((track.h as f32) * ratio).max(28.0) as i32;
        let offset = (scroll as f32 / max_scroll as f32) * (track.h - thumb_h) as f32;
        let thumb = Rect::new(track.x, track.y + offset as i32, track.w, thumb_h);
        paint::fill_round(dc, thumb, 2, palette.ink_tertiary);
    }
}

/// 左侧导航栏。
fn render_sidebar(
    dc: HDC,
    model: &Model,
    fonts: &Fonts,
    palette: &Palette,
    logo: Option<&mut crate::brand::Logo>,
) {
    use theme::metrics;

    paint::fill_rect(
        dc,
        Rect::new(0, 0, metrics::SIDEBAR_W, model.size.1),
        palette.surface,
    );
    paint::vline(
        dc,
        metrics::SIDEBAR_W - 1,
        0,
        model.size.1,
        palette.hairline,
    );

    // 品牌标：直接用 Python 原版那份 app_icon.ico，
    // 和标题栏、任务栏、托盘图标是同一张图。
    let mark = Rect::new(24, 24, crate::brand::LOGO_SIZE, crate::brand::LOGO_SIZE);
    match logo {
        Some(logo) => {
            logo.update(palette.surface);
            logo.draw(dc, mark);
        }
        None => {
            // 兜底：图标解不出来时画一个强调色方块，别让侧边栏空着
            paint::fill_round(dc, mark, 7, palette.primary);
        }
    }

    paint::text(
        dc,
        Rect::new(62, 22, 160, 22),
        "工资自助系统",
        fonts.section(),
        palette.ink,
        Align::Left,
        0,
    );
    paint::text(
        dc,
        Rect::new(62, 44, 170, 14),
        "SALARY SELF-SERVICE",
        fonts.eyebrow(),
        palette.ink_tertiary,
        Align::Left,
        1,
    );

    // 导航项
    let items = [
        (
            model.layout.nav[0],
            Page::Service,
            "服务控制",
            "SERVICE",
            Hit::NavService,
        ),
        (
            model.layout.nav[1],
            Page::Fields,
            "字段显示",
            "FIELDS",
            Hit::NavFields,
        ),
        (
            model.layout.nav[2],
            Page::Manual,
            "使用手册",
            "MANUAL",
            Hit::NavManual,
        ),
    ];
    for (rect, page, title, eyebrow, nav_hit) in items {
        let active = model.page == page;
        let hovered = model.hover == Some(nav_hit);

        if active {
            paint::fill_round(dc, rect, palette.radius_button, palette.surface_row);
            // 激活项左边的强调色竖条
            paint::fill_rect(
                dc,
                Rect::new(rect.x, rect.y + 8, 3, rect.h - 16),
                palette.primary,
            );
        } else if hovered {
            paint::fill_round(dc, rect, palette.radius_button, palette.surface_raised);
        }

        let text_x = rect.x + 16;
        paint::text(
            dc,
            Rect::new(text_x, rect.y + 5, rect.w - 20, 18),
            title,
            fonts.body_medium(),
            if active {
                palette.ink
            } else {
                palette.ink_subtle
            },
            Align::Left,
            0,
        );
        paint::text(
            dc,
            Rect::new(text_x, rect.y + 24, rect.w - 20, 13),
            eyebrow,
            fonts.eyebrow(),
            if active {
                palette.primary
            } else {
                palette.ink_tertiary
            },
            Align::Left,
            1,
        );
    }

    // 底部用户卡
    let card = Rect::new(20, model.size.1 - 96, metrics::SIDEBAR_W - 40, 72);
    paint::fill_round(dc, card, palette.radius_card, palette.surface_raised);

    let avatar = Rect::new(card.x + 14, card.y + 18, 36, 36);
    paint::fill_round(dc, avatar, 10, palette.surface_row);
    let initial = std::env::var("USERNAME")
        .ok()
        .and_then(|name| name.chars().next())
        .map(|c| c.to_string())
        .unwrap_or_else(|| "·".to_string());
    paint::text(
        dc,
        avatar,
        &initial,
        fonts.body_medium(),
        palette.ink_muted,
        Align::Center,
        0,
    );

    let user = std::env::var("USERNAME").unwrap_or_else(|_| "本机用户".to_string());
    paint::text(
        dc,
        Rect::new(card.x + 60, card.y + 16, card.w - 72, 20),
        &user,
        fonts.body_medium(),
        palette.ink,
        Align::Left,
        0,
    );
    paint::text(
        dc,
        Rect::new(card.x + 60, card.y + 38, card.w - 72, 14),
        "本机运维 LOCAL OPERATOR",
        fonts.eyebrow(),
        palette.ink_tertiary,
        Align::Left,
        1,
    );
}

/// 顶栏：页面标题、通知条、状态 pill 与主题切换。
fn render_topbar(dc: HDC, model: &Model, fonts: &Fonts, palette: &Palette, layout: &Layout) {
    use theme::metrics;
    let content_x = metrics::SIDEBAR_W + metrics::PAD;

    paint::hline(
        dc,
        metrics::SIDEBAR_W,
        metrics::TOPBAR_H - 1,
        model.size.0 - metrics::SIDEBAR_W,
        palette.hairline,
    );

    let (title, eyebrow) = match model.page {
        Page::Service => ("服务控制", "SERVICE CONTROL"),
        Page::Fields => ("字段显示", "FIELD DISPLAY"),
        Page::Manual => ("使用手册", "USER MANUAL"),
    };
    paint::text(
        dc,
        Rect::new(content_x, 18, 240, 24),
        title,
        fonts.page_title(),
        palette.ink,
        Align::Left,
        0,
    );
    paint::text(
        dc,
        Rect::new(content_x, 44, 240, 14),
        eyebrow,
        fonts.eyebrow(),
        palette.ink_tertiary,
        Align::Left,
        1,
    );

    // 通知条
    if let Some(notice) = &model.notice {
        let color = if notice.error {
            palette.error
        } else {
            palette.ink_muted
        };
        paint::fill_circle(dc, layout.notice.x + 4, layout.notice.y + 12, 3, color);
        paint::text(
            dc,
            Rect::new(
                layout.notice.x + 16,
                layout.notice.y,
                layout.notice.w - 16,
                layout.notice.h,
            ),
            &notice.text,
            fonts.small(),
            color,
            Align::Right,
            0,
        );
    }

    // 状态 pill
    let running = model.status.running;
    let status_color = if running {
        palette.success
    } else {
        palette.error
    };
    paint::fill_round(dc, layout.status_pill, palette.radius_pill, palette.pill_bg);
    paint::fill_circle(
        dc,
        layout.status_pill.x + 16,
        layout.status_pill.y + layout.status_pill.h / 2,
        4,
        status_color,
    );
    paint::text(
        dc,
        Rect::new(
            layout.status_pill.x + 28,
            layout.status_pill.y,
            layout.status_pill.w - 36,
            layout.status_pill.h,
        ),
        if running {
            "运行中 RUNNING"
        } else {
            "已停止 STOPPED"
        },
        fonts.body_medium(),
        status_color,
        Align::Left,
        0,
    );

    // 主题切换
    let theme_hovered = model.hover == Some(Hit::ThemeToggle);
    paint::fill_round(
        dc,
        layout.theme_button,
        palette.radius_button,
        if theme_hovered {
            palette.surface_row
        } else {
            palette.surface_raised
        },
    );
    paint::text(
        dc,
        layout.theme_button,
        match model.theme {
            Theme::Dark => "浅色",
            Theme::Light => "深色",
        },
        fonts.body_medium(),
        palette.ink_muted,
        Align::Center,
        0,
    );
}

/// 服务控制页。
fn render_service(dc: HDC, model: &Model, fonts: &Fonts, palette: &Palette, layout: &Layout) {
    let status = &model.status;

    // KPI 卡片
    let running = status.running;
    let kpi_values: [(String, [u8; 3]); 3] = [
        (
            if running {
                "运行中".to_string()
            } else {
                "已停止".to_string()
            },
            if running {
                palette.success
            } else {
                palette.error
            },
        ),
        (status.port.to_string(), palette.ink),
        (status.months.to_string(), palette.ink),
    ];
    let kpi_labels = [
        ("SERVICE STATUS", "服务状态"),
        ("PORT", "HTTP 端口"),
        ("MONTHS", "工资月份"),
    ];
    let kpi_subs = [
        if status.restarts > 0 {
            // 服务掉过线、被自动拉起来了
            format!("异常退出后已自动恢复 {} 次", status.restarts)
        } else if running {
            "服务对外提供查询".to_string()
        } else {
            "服务当前未启动".to_string()
        },
        format!("HTTPS {}", status.port.saturating_add(1)),
        "可查询的历史月份".to_string(),
    ];

    for index in 0..3 {
        let rect = layout.kpi[index];
        paint_card(dc, rect, palette);

        // 卡片内部的留白跟字号缩放走：字大了间距也等比放大
        let s = model.font_scale;
        let off = |value: i32| (value as f32 * s).round() as i32;

        paint::text(
            dc,
            Rect::new(rect.x + 24, rect.y + off(18), rect.w - 48, 14),
            kpi_labels[index].0,
            fonts.eyebrow(),
            palette.ink_tertiary,
            Align::Left,
            1,
        );
        paint::text(
            dc,
            Rect::new(rect.x + 24, rect.y + off(36), rect.w - 48, 18),
            kpi_labels[index].1,
            fonts.body_medium(),
            palette.ink_subtle,
            Align::Left,
            0,
        );

        let is_mono = index == 1 || index == 2;
        paint::text(
            dc,
            Rect::new(rect.x + 24, rect.y + off(58), rect.w - 48, 38),
            &kpi_values[index].0,
            if is_mono {
                fonts.kpi_mono()
            } else {
                fonts.kpi()
            },
            kpi_values[index].1,
            Align::Left,
            0,
        );

        paint::text(
            dc,
            Rect::new(rect.x + 24, rect.y + off(98), rect.w - 48, 14),
            kpi_subs[index].as_str(),
            fonts.eyebrow(),
            palette.ink_tertiary,
            Align::Left,
            1,
        );
    }

    // 访问地址卡片：只列局域网地址。
    // 面板就跑在这台机器上，localhost 对使用者没有意义。
    let card = layout.access_card;
    paint_card(dc, card, palette);
    paint_card_title(dc, card, "访问地址", "ACCESS", fonts, palette);

    for index in 0..2 {
        let (label, url) = if index == 0 {
            ("HTTP", status.lan_url.as_str())
        } else {
            ("HTTPS", status.lan_https_url.as_str())
        };
        paint::text(
            dc,
            layout.url_labels[index],
            label,
            fonts.eyebrow(),
            palette.ink_tertiary,
            Align::Left,
            1,
        );
        paint::text(
            dc,
            layout.url_values[index],
            url,
            fonts.mono(),
            palette.ink,
            Align::Left,
            0,
        );
        render_small_button(dc, layout.copy_buttons[index], "复制", fonts, palette);
    }

    // 一键复制所有信息
    render_small_button(dc, layout.copy_all_button, "复制全部信息", fonts, palette);

    paint::text(
        dc,
        Rect::new(card.x + 24, layout.port_edit.y - 2, 100, 26),
        "端口 PORT",
        fonts.body(),
        palette.ink_muted,
        Align::Left,
        0,
    );

    // 登录提示（账号 = 工号，密码 = 紧急联系人号码）放在页面底部的提示行里，
    // 这里不再重复一遍。

    // 端口输入框：整块都是画布自绘的，只用一条发丝线表示「可以输入」
    let port_text = if model.port_text.is_empty() {
        status.port.to_string()
    } else {
        model.port_text.clone()
    };
    paint::text(
        dc,
        layout.port_edit,
        &port_text,
        fonts.mono(),
        if model.port_editing {
            palette.ink
        } else {
            palette.ink_muted
        },
        Align::Left,
        0,
    );

    if model.port_editing {
        // 光标：画在文字后面
        let before: String = port_text.chars().take(model.caret).collect();
        let offset = paint::text_width(dc, &before, fonts.mono());
        paint::fill_rect(
            dc,
            Rect::new(
                layout.port_edit.x + offset + 1,
                layout.port_edit.y + 4,
                1,
                layout.port_edit.h - 8,
            ),
            palette.primary,
        );
    }

    paint::hline(
        dc,
        layout.port_edit.x,
        layout.port_edit.y + layout.port_edit.h,
        layout.port_edit.w,
        if model.port_editing {
            palette.primary
        } else {
            palette.hairline_strong
        },
    );

    let apply_hovered = model.hover == Some(Hit::ApplyPort);
    paint::fill_round(
        dc,
        layout.apply_button,
        palette.radius_button,
        if apply_hovered {
            palette.surface_row
        } else {
            palette.surface_raised
        },
    );
    paint::stroke_round(
        dc,
        layout.apply_button,
        palette.radius_button,
        palette.hairline,
    );
    paint::text(
        dc,
        layout.apply_button,
        "应用 APPLY",
        fonts.body_medium(),
        palette.ink,
        Align::Center,
        0,
    );

    // 快捷操作卡片
    let card = layout.actions_card;
    paint_card(dc, card, palette);
    paint_card_title(dc, card, "快捷操作", "QUICK ACTIONS", fonts, palette);

    let labels = [
        (Hit::Start, "启动服务", "START", ButtonStyle::Primary),
        (Hit::Stop, "停止服务", "STOP", ButtonStyle::Secondary),
        (Hit::OpenBrowser, "打开面板", "OPEN", ButtonStyle::Secondary),
        (
            Hit::OpenData,
            "打开数据目录",
            "DATA",
            ButtonStyle::Secondary,
        ),
        (
            Hit::Firewall,
            "放行防火墙",
            "FIREWALL",
            ButtonStyle::Secondary,
        ),
        (
            Hit::Autostart,
            "开机自启",
            "AUTOSTART",
            ButtonStyle::Secondary,
        ),
    ];
    for (hit, title, eyebrow, style) in labels {
        let Some(rect) = layout
            .action_buttons
            .iter()
            .find(|(candidate, _)| *candidate == hit)
            .map(|(_, rect)| *rect)
        else {
            continue;
        };

        let title = if hit == Hit::Autostart {
            format!("开机自启：{}", if status.autostart { "开" } else { "关" })
        } else {
            title.to_string()
        };

        render_button(
            dc,
            rect,
            &title,
            eyebrow,
            style,
            palette,
            fonts,
            model.hover == Some(hit),
        );
    }

    // 支持访问的网络（配置在 data/networks.txt）
    paint::text(
        dc,
        layout.network_label,
        "支持访问的网络",
        fonts.eyebrow(),
        palette.ink_tertiary,
        Align::Left,
        1,
    );
    for (index, row) in model.networks.iter().enumerate().take(3) {
        let Some(rect) = layout.network_rows.get(index) else {
            break;
        };
        let (mark, color) = if row.supported {
            ("✓", palette.success)
        } else {
            ("✗", palette.error)
        };
        paint::text(
            dc,
            Rect::new(rect.x, rect.y, 16, rect.h),
            mark,
            fonts.body_medium(),
            color,
            Align::Left,
            0,
        );
        paint::text(
            dc,
            Rect::new(rect.x + 18, rect.y, rect.w - 18, rect.h),
            &format!("{} · {}", row.kind, row.description),
            fonts.small(),
            palette.ink_muted,
            Align::Left,
            0,
        );
    }

    // 底部提示：登录字段映射跟着控制面板的设置走，不硬编码列名
    let saved_account = if model.credential.saved_account.is_empty() {
        "工号".to_string()
    } else {
        model.credential.saved_account.clone()
    };
    let saved_password = if model.credential.saved_password.is_empty() {
        "紧急联系人号码".to_string()
    } else {
        model.credential.saved_password.clone()
    };

    paint::text(
        dc,
        layout.hint,
        &format!(
            "开机自启：{} ｜ 员工登录：账号 = {}，密码 = {}；改端口前请先停服务。",
            if status.autostart { "开" } else { "关" },
            saved_account,
            saved_password
        ),
        fonts.small(),
        palette.ink_tertiary,
        Align::Left,
        0,
    );

    // 查看日志
    render_small_button(dc, layout.view_log_button, "查看日志", fonts, palette);
}

/// 字段显示页。
fn render_fields(dc: HDC, model: &Model, fonts: &Fonts, palette: &Palette, layout: &Layout) {
    let card = layout.fields_card;
    paint_card(dc, card, palette);
    paint_card_title(dc, card, "字段显示策略", "FIELD VISIBILITY", fonts, palette);

    if model.fields.is_empty() {
        paint::text(
            dc,
            Rect::new(card.x + 20, card.y + 80, card.w - 40, 20),
            "还没有找到工资表，先把工资表放进数据目录再点「重新扫描」。",
            fonts.body(),
            palette.ink_subtle,
            Align::Left,
            0,
        );
    }

    for (index, rect) in layout.field_rows.iter().enumerate() {
        let Some(field) = model.fields.get(model.scroll as usize + index) else {
            break;
        };

        let selected = model.selected == Some(model.scroll as usize + index);
        let hovered = model.hover == Some(Hit::FieldRow(index));

        if selected {
            paint::fill_round(dc, *rect, palette.radius_button, palette.surface_row);
            paint::fill_rect(
                dc,
                Rect::new(rect.x, rect.y + 6, 3, rect.h - 12),
                palette.primary,
            );
        } else if hovered {
            paint::fill_round(dc, *rect, palette.radius_button, palette.surface_raised);
        }

        paint::text(
            dc,
            Rect::new(rect.x + 16, rect.y, rect.w - 140, rect.h),
            &field.name,
            fonts.body(),
            if selected {
                palette.ink
            } else {
                palette.ink_muted
            },
            Align::Left,
            0,
        );

        // 右侧状态 pill
        let (label, color) = match field.state {
            FieldState::Show => ("显示 SHOW", palette.success),
            FieldState::Hide => ("隐藏 HIDE", palette.error),
            FieldState::Auto => ("跟随 AUTO", palette.ink_subtle),
        };
        let pill = Rect::new(rect.right() - 116, rect.y + 3, 108, rect.h - 6);
        paint::fill_round(dc, pill, palette.radius_pill, palette.pill_bg);
        paint::text(
            dc,
            pill,
            label,
            fonts.body_medium(),
            color,
            Align::Center,
            0,
        );
    }

    // 滚动条提示：字段比可视行数多时画一条细轨道
    let visible_rows = layout.visible_rows.max(1);
    if model.fields.len() > visible_rows {
        let track = Rect::new(
            card.right() - 12,
            card.y + theme::metrics::LIST_HEADER_H,
            4,
            visible_rows as i32 * theme::metrics::ROW_H - 8,
        );
        paint::fill_round(dc, track, 2, palette.hairline);
        let ratio = visible_rows as f32 / model.fields.len() as f32;
        let thumb_h = ((track.h as f32) * ratio).max(24.0) as i32;
        let max_scroll = (model.fields.len() - visible_rows) as f32;
        let offset = (model.scroll as f32 / max_scroll.max(1.0)) * (track.h - thumb_h) as f32;
        let thumb = Rect::new(track.x, track.y + offset as i32, track.w, thumb_h);
        paint::fill_round(dc, thumb, 2, palette.ink_tertiary);
    }

    let buttons = [
        (Hit::Cycle, "切换状态", "CYCLE", ButtonStyle::Primary),
        (Hit::Save, "保存", "SAVE", ButtonStyle::Secondary),
        (Hit::Rescan, "重新扫描", "RESCAN", ButtonStyle::Secondary),
    ];
    for (hit, title, eyebrow, style) in buttons {
        let Some(rect) = layout
            .field_buttons
            .iter()
            .find(|(candidate, _)| *candidate == hit)
            .map(|(_, rect)| *rect)
        else {
            continue;
        };
        render_button(
            dc,
            rect,
            title,
            eyebrow,
            style,
            palette,
            fonts,
            model.hover == Some(hit),
        );
    }

    paint::text(
        dc,
        layout.hint,
        &format!(
            "共 {} 个字段，来自 {} 张工资表；选中一行后点「切换状态」在 显示 → 跟随 → 隐藏 之间循环，保存后立即生效。",
            model.fields.len(),
            model.status.months
        ),
        fonts.small(),
        palette.ink_tertiary,
        Align::Left,
        0,
    );
}

/// 账号密码卡片（服务控制页左栏）。
///
/// 控制面板不查工资：这块只保管一组能登录网页的凭据——
/// 账号从员工列表里选，密码要么手输，要么指定一个工资表字段由面板读出来。
fn render_credential_card(
    dc: HDC,
    model: &Model,
    fonts: &Fonts,
    palette: &Palette,
    layout: &Layout,
) {
    let card = layout.credential_card;
    paint_card(dc, card, palette);
    paint_card_title(dc, card, "账号密码", "ACCOUNT & PASSWORD", fonts, palette);

    let state = &model.credential;
    let label_x = card.x + 24;

    // ---- 账号 ----
    paint::text(
        dc,
        Rect::new(label_x, card.y + 72, 52, 16),
        "账号",
        fonts.body_medium(),
        palette.ink_muted,
        Align::Left,
        0,
    );
    paint::text(
        dc,
        Rect::new(label_x, card.y + 90, 80, 12),
        "ACCOUNT",
        fonts.eyebrow(),
        palette.ink_tertiary,
        Align::Left,
        1,
    );

    let account_text = state
        .account
        .and_then(|index| state.fields.get(index))
        .cloned()
        .unwrap_or_else(|| {
            if state.fields.is_empty() {
                "没有可选字段（未找到员工列表）".to_string()
            } else {
                "请选择账号字段".to_string()
            }
        });

    render_select(
        dc,
        layout.credential_account,
        &account_text,
        state
            .dropdown
            .as_ref()
            .map(|dropdown| dropdown.kind == DropdownKind::Account)
            .unwrap_or(false),
        model.hover == Some(Hit::CredentialAccount),
        fonts,
        palette,
    );

    // ---- 密码 ----
    paint::text(
        dc,
        Rect::new(label_x, card.y + 122, 52, 16),
        "密码",
        fonts.body_medium(),
        palette.ink_muted,
        Align::Left,
        0,
    );
    paint::text(
        dc,
        Rect::new(label_x, card.y + 140, 80, 12),
        "PASSWORD",
        fonts.eyebrow(),
        palette.ink_tertiary,
        Align::Left,
        1,
    );

    let field_text = state
        .field
        .and_then(|index| state.fields.get(index))
        .cloned()
        .unwrap_or_else(|| {
            if state.fields.is_empty() {
                "没有可选字段（未找到工资表）".to_string()
            } else {
                "请选择密码字段".to_string()
            }
        });

    render_select(
        dc,
        layout.credential_field,
        &field_text,
        state
            .dropdown
            .as_ref()
            .map(|dropdown| dropdown.kind == DropdownKind::Field)
            .unwrap_or(false),
        model.hover == Some(Hit::CredentialField),
        fonts,
        palette,
    );

    // ---- 提示 ----
    paint::text(
        dc,
        layout.credential_status,
        "登录时账号取「账号」列的内容，口令取「密码」列的内容，保存后立即生效。",
        fonts.small(),
        palette.ink_tertiary,
        Align::Left,
        0,
    );

    // ---- 按钮 ----
    let buttons = [
        (
            Hit::CredentialSave,
            "保存设置",
            "SAVE",
            ButtonStyle::Primary,
        ),
        (
            Hit::CredentialReadFields,
            "字段读取",
            "SCAN",
            ButtonStyle::Secondary,
        ),
    ];

    for (hit, title, eyebrow, style) in buttons {
        let Some(rect) = layout
            .credential_buttons
            .iter()
            .find(|(candidate, _)| *candidate == hit)
            .map(|(_, rect)| *rect)
        else {
            continue;
        };

        render_button(
            dc,
            rect,
            title,
            eyebrow,
            style,
            palette,
            fonts,
            model.hover == Some(hit),
        );
    }
}

/// 画一个下拉控件。
fn render_select(
    dc: HDC,
    rect: Rect,
    text: &str,
    expanded: bool,
    hovered: bool,
    fonts: &Fonts,
    palette: &Palette,
) {
    paint::fill_round(
        dc,
        rect,
        palette.radius_button,
        if hovered || expanded {
            palette.surface_row
        } else {
            palette.surface_raised
        },
    );
    paint::stroke_round(
        dc,
        rect,
        palette.radius_button,
        if expanded {
            palette.primary
        } else {
            palette.hairline
        },
    );

    paint::text(
        dc,
        Rect::new(rect.x + 12, rect.y, rect.w - 40, rect.h),
        text,
        fonts.small(),
        palette.ink,
        Align::Left,
        0,
    );

    // 右侧的小箭头：展开时翻过来
    paint::text(
        dc,
        Rect::new(rect.right() - 26, rect.y, 20, rect.h),
        if expanded { "▴" } else { "▾" },
        fonts.small(),
        palette.ink_tertiary,
        Align::Center,
        0,
    );
}

/// 画下拉弹层。
///
/// 弹层画在页面内容之上：它可能盖住卡片里的其它控件。
fn render_dropdown(dc: HDC, model: &Model, fonts: &Fonts, palette: &Palette, layout: &Layout) {
    let Some(dropdown) = &model.credential.dropdown else {
        return;
    };
    if layout.dropdown_items.is_empty() {
        return;
    }

    // 弹层是浮出菜单，不是胶囊：浅色主题的 radius_button 是 100（胶囊），
    // 用在两三百像素高的弹层上会把整个面板圆成一颗蛋。这里固定用卡片圆角。
    paint::fill_round(dc, layout.dropdown, palette.radius_card, palette.surface);
    paint::stroke_round(
        dc,
        layout.dropdown,
        palette.radius_card,
        palette.hairline_strong,
    );

    let scroll = dropdown.scroll.max(0) as usize;
    for (offset, rect) in layout.dropdown_items.iter().enumerate() {
        let index = scroll + offset;
        let selected = dropdown.selected == Some(index);
        let hovered = model.hover == Some(Hit::DropdownItem(index));

        if selected {
            paint::fill_round(
                dc,
                rect.inset(4, 2),
                palette.radius_button,
                palette.surface_row,
            );
            paint::fill_rect(
                dc,
                Rect::new(rect.x + 6, rect.y + 7, 3, rect.h - 14),
                palette.primary,
            );
        } else if hovered {
            paint::fill_round(
                dc,
                rect.inset(4, 2),
                palette.radius_button,
                palette.surface_raised,
            );
        }

        let text = dropdown.items.get(index).cloned().unwrap_or_default();
        paint::text(
            dc,
            Rect::new(rect.x + 18, rect.y, rect.w - 36, rect.h),
            &text,
            fonts.small(),
            if selected {
                palette.ink
            } else {
                palette.ink_muted
            },
            Align::Left,
            0,
        );
    }
}

/// 画一个次级小按钮（复制、查看日志这类）。
fn render_small_button(dc: HDC, rect: Rect, label: &str, fonts: &Fonts, palette: &Palette) {
    paint::fill_round(dc, rect, palette.radius_button, palette.surface_raised);
    paint::stroke_round(dc, rect, palette.radius_button, palette.hairline);
    paint::text(
        dc,
        rect,
        label,
        fonts.small(),
        palette.ink_muted,
        Align::Center,
        0,
    );
}

/// 画一张卡片：浅色主题先画一层投影，再画底色与描边。
fn paint_card(dc: HDC, rect: Rect, palette: &Palette) {
    if let Some(shadow) = palette.shadow {
        paint::fill_round(dc, rect.offset(0, 1), palette.radius_card, shadow);
    }
    paint::fill_round(dc, rect, palette.radius_card, palette.surface);
    paint::stroke_round(dc, rect, palette.radius_card, palette.hairline);
}

/// 画卡片标题：中文大字 + 英文小标签（规范要求的中英双语）。
fn paint_card_title(
    dc: HDC,
    card: Rect,
    title: &str,
    eyebrow: &str,
    fonts: &Fonts,
    palette: &Palette,
) {
    paint::text(
        dc,
        Rect::new(card.x + 24, card.y + 18, card.w - 48, 26),
        title,
        fonts.section(),
        palette.ink,
        Align::Left,
        0,
    );
    paint::text(
        dc,
        Rect::new(card.x + 24, card.y + 46, card.w - 48, 14),
        eyebrow,
        fonts.eyebrow(),
        palette.ink_tertiary,
        Align::Left,
        1,
    );
}

/// 画一个按钮。
#[allow(clippy::too_many_arguments)]
fn render_button(
    dc: HDC,
    rect: Rect,
    title: &str,
    eyebrow: &str,
    style: ButtonStyle,
    palette: &Palette,
    fonts: &Fonts,
    hovered: bool,
) {
    match style {
        ButtonStyle::Primary => {
            paint::fill_round(
                dc,
                rect,
                palette.radius_button,
                if hovered {
                    palette.primary_hover
                } else {
                    palette.primary
                },
            );
            paint::text(
                dc,
                Rect::new(rect.x, rect.y + 5, rect.w, 16),
                title,
                fonts.button(palette.dark),
                [0xff, 0xff, 0xff],
                Align::Center,
                0,
            );
            paint::text(
                dc,
                Rect::new(rect.x, rect.y + 22, rect.w, 12),
                eyebrow,
                fonts.eyebrow(),
                [0xdc, 0xdf, 0xf6],
                Align::Center,
                1,
            );
        }
        ButtonStyle::Secondary => {
            paint::fill_round(
                dc,
                rect,
                palette.radius_button,
                if hovered {
                    palette.surface_row
                } else {
                    palette.surface_raised
                },
            );
            paint::stroke_round(dc, rect, palette.radius_button, palette.hairline);
            paint::text(
                dc,
                Rect::new(rect.x, rect.y + 5, rect.w, 16),
                title,
                fonts.button(palette.dark),
                palette.ink,
                Align::Center,
                0,
            );
            paint::text(
                dc,
                Rect::new(rect.x, rect.y + 22, rect.w, 12),
                eyebrow,
                fonts.eyebrow(),
                palette.ink_tertiary,
                Align::Center,
                1,
            );
        }
    }
}
