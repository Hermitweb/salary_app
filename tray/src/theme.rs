//! 控制面板的配色、字体与尺寸。
//!
//! 视觉规范来自仓库根目录的《设计风格规范.md》：一套信息架构、两套主题
//! （深色 Linear 风 / 浅色 Meta 风）。规范强调「结构零改动、主题只换皮」，
//! 因此这里把所有颜色收敛进 [`Palette`]——切换主题时只换这一个结构体，
//! 布局与绘制代码完全不用动。
//!
//! 颜色一律用 `[r, g, b]` 表示，方便直接交给 GDI 画刷。

use native_windows_gui as nwg;
use winapi::shared::windef::{HFONT, HWND};
use winapi::um::dwmapi::DwmSetWindowAttribute;
use winapi::um::wingdi::{GetTextFaceW, SelectObject};
use winapi::um::winuser::{GetDC, ReleaseDC};

/// 主题。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    /// 深色 Linear 风：近黑画布 + surface 阶梯 + 薰草蓝强调色。
    Dark,
    /// 浅色 Meta 风：浅灰画布 + 白卡 + 钴蓝强调色 + 胶囊圆角。
    Light,
}

/// 一套主题用到的全部颜色。
///
/// 字段与规范里的 token 一一对应，命名也保持一致，方便对照规范检查。
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    /// 页面底色。
    pub canvas: [u8; 3],
    /// 卡片 / 面板底色（深色主题的 Surface-1，浅色主题的白卡）。
    pub surface: [u8; 3],
    /// 浮起一层：hover 态、输入框底色（深色 Surface-2）。
    pub surface_raised: [u8; 3],
    /// 选中行 / 激活导航的底色（深色 Surface-3，浅色 Active-Nav-Bg）。
    pub surface_row: [u8; 3],
    /// 1px 描边与分割线。
    pub hairline: [u8; 3],
    /// 需要强调的描边（输入框聚焦）。
    pub hairline_strong: [u8; 3],
    /// 标题与强调正文。
    pub ink: [u8; 3],
    /// 次级文字。
    pub ink_muted: [u8; 3],
    /// 辅助文字 / 占位。
    pub ink_subtle: [u8; 3],
    /// 脚注与禁用态。
    pub ink_tertiary: [u8; 3],
    /// 唯一强调色：品牌标、主按钮、激活态。
    pub primary: [u8; 3],
    /// 主按钮 hover 态。
    pub primary_hover: [u8; 3],
    /// 运行中 / 成功。
    pub success: [u8; 3],
    /// 异常 / 失败。
    pub error: [u8; 3],
    /// 状态 pill 的底色。
    pub pill_bg: [u8; 3],
    /// 浅色主题的卡片投影替身（深色主题不用投影，靠 surface 阶梯分层）。
    pub shadow: Option<[u8; 3]>,
    /// 按钮圆角：深色 8px 直角，浅色胶囊。
    pub radius_button: i32,
    /// 卡片圆角。
    pub radius_card: i32,
    /// 状态 pill 圆角。
    pub radius_pill: i32,
    /// 是否是深色主题。
    ///
    /// 标题栏由系统绘制，只能通过 DWM 的「沉浸式深色模式」切换，
    /// 所以需要一个布尔量来表达这件事。
    pub dark: bool,
}

impl Palette {
    /// 取指定主题的色板。
    pub fn of(theme: Theme) -> Palette {
        match theme {
            Theme::Dark => Palette {
                canvas: [0x01, 0x01, 0x02],
                surface: [0x0f, 0x10, 0x11],
                surface_raised: [0x14, 0x15, 0x16],
                surface_row: [0x18, 0x19, 0x1a],
                hairline: [0x23, 0x25, 0x2a],
                hairline_strong: [0x34, 0x34, 0x3a],
                ink: [0xf7, 0xf8, 0xf8],
                ink_muted: [0xd0, 0xd6, 0xe0],
                // 深底上的灰字要守住可读下限：subtle ≈ 7:1，tertiary ≈ 4.7:1，
                // 再暗就低于小字号的 AA 标准（正文看起来「发虚、费眼」）。
                ink_subtle: [0x9b, 0xa1, 0xac],
                ink_tertiary: [0x77, 0x7d, 0x87],
                primary: [0x5e, 0x6a, 0xd2],
                primary_hover: [0x82, 0x8f, 0xff],
                success: [0x27, 0xa6, 0x44],
                error: [0xeb, 0x57, 0x57],
                pill_bg: [0x14, 0x15, 0x16],
                shadow: None,
                radius_button: 8,
                radius_card: 12,
                radius_pill: 8,
                dark: true,
            },
            Theme::Light => Palette {
                canvas: [0xf1, 0xf4, 0xf7],
                surface: [0xff, 0xff, 0xff],
                surface_raised: [0xf1, 0xf4, 0xf7],
                surface_row: [0xe7, 0xf0, 0xfe],
                hairline: [0xce, 0xd0, 0xd4],
                hairline_strong: [0x85, 0x95, 0xa4],
                ink: [0x0a, 0x13, 0x17],
                ink_muted: [0x44, 0x49, 0x50],
                ink_subtle: [0x5d, 0x6c, 0x7b],
                ink_tertiary: [0x85, 0x95, 0xa4],
                primary: [0x00, 0x64, 0xe0],
                primary_hover: [0x18, 0x76, 0xf2],
                success: [0x31, 0xa2, 0x4c],
                error: [0xe4, 0x1e, 0x3f],
                pill_bg: [0xe4, 0xe6, 0xeb],
                // rgba(0,0,0,0.08) 叠在浅灰画布上的等效实色
                shadow: Some([0xde, 0xe0, 0xe3]),
                radius_button: 100,
                radius_card: 12,
                radius_pill: 100,
                dark: false,
            },
        }
    }
}

/// 沉浸式深色模式的属性号。
///
/// Windows 11 把这个属性从 19 挪到了 20，旧版本仍然认 19，
/// 所以两个都试一遍，先成功的算生效。
const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
/// 旧版 Windows 的沉浸式深色模式属性号。
const DWMWA_USE_IMMERSIVE_DARK_MODE_LEGACY: u32 = 19;
/// 窗口边框颜色（Windows 11 22H2+）。
const DWMWA_BORDER_COLOR: u32 = 34;
/// 标题栏底色（Windows 11 22H2+）。
const DWMWA_CAPTION_COLOR: u32 = 35;
/// 标题栏文字颜色（Windows 11 22H2+）。
const DWMWA_TEXT_COLOR: u32 = 36;

/// 把系统标题栏也纳入主题。
///
/// 标题栏是系统画的，不在自己的画布里，只能通过 DWM 指定颜色。
/// Windows 11 才能精确指定底色与文字色；Windows 10 最多只能开
/// 「沉浸式深色模式」，失败就忽略——旧系统上标题栏保持系统默认色。
pub fn apply_title_bar(hwnd: HWND, theme: Theme) {
    let palette = Palette::of(theme);

    let dark: i32 = i32::from(palette.dark);
    if dwm_set(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &dark, 4) != 0 {
        dwm_set(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE_LEGACY, &dark, 4);
    }

    let caption = colorref(palette.canvas);
    let text = colorref(palette.ink);
    let border = colorref(palette.hairline);
    dwm_set(hwnd, DWMWA_CAPTION_COLOR, &caption, 4);
    dwm_set(hwnd, DWMWA_TEXT_COLOR, &text, 4);
    dwm_set(hwnd, DWMWA_BORDER_COLOR, &border, 4);
}

/// 设置一个 DWM 窗口属性，返回是否成功（0 表示成功）。
fn dwm_set<T>(hwnd: HWND, attribute: u32, value: &T, size: u32) -> i32 {
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            attribute,
            value as *const T as *const winapi::ctypes::c_void,
            size,
        )
    }
}

/// RGB 转 COLORREF。
///
/// DWM 用的是 0x00BBGGRR，和 Windows 里常见的 0x00RRGGBB 正好相反，
/// 写反了标题栏会变成一片蓝。
fn colorref(rgb: [u8; 3]) -> u32 {
    ((rgb[2] as u32) << 16) | ((rgb[1] as u32) << 8) | (rgb[0] as u32)
}

/// 界面字体族候选（按优先级回退）。
///
/// 规范指定 Noto Sans SC，但目标机器上不一定装了；GDI 找不到字体会静默替换
/// 成别的字体，所以这里逐个创建并用 `GetTextFaceW` 核对实际选中的字族。
const SANS_CANDIDATES: &[&str] = &["Noto Sans SC", "Microsoft YaHei UI", "Segoe UI"];

/// 等宽字体族候选（端口、地址、编号用）。
const MONO_CANDIDATES: &[&str] = &["Geist Mono", "Cascadia Mono", "Consolas"];

/// 找一个系统里确实存在的字族。
fn resolve_family(candidates: &'static [&'static str]) -> &'static str {
    let fallback = candidates[candidates.len() - 1];

    let dc = unsafe { GetDC(std::ptr::null_mut()) };
    if dc.is_null() {
        return fallback;
    }

    let mut chosen = fallback;
    for candidate in candidates {
        let mut probe = Default::default();
        if nwg::Font::builder()
            .family(candidate)
            .size(14)
            .weight(400)
            .build(&mut probe)
            .is_err()
        {
            continue;
        }

        let previous = unsafe { SelectObject(dc, probe.handle as _) };
        let mut buffer = [0u16; 64];
        let written = unsafe { GetTextFaceW(dc, buffer.len() as i32, buffer.as_mut_ptr()) };
        unsafe { SelectObject(dc, previous) };

        if written > 0 {
            let face = String::from_utf16_lossy(&buffer[..written as usize]);
            if face.eq_ignore_ascii_case(candidate) {
                chosen = candidate;
                break;
            }
        }
    }

    unsafe { ReleaseDC(std::ptr::null_mut(), dc) };
    chosen
}

/// 面板用到的字体。
///
/// 字号阶梯对齐规范的排版表并在其上整体加大，保证小窗口可读
/// （正文 17 / 次级 16 / KPI 32 / 标号 16 等宽 / 小标签 13）；
/// 面板比规范里的 1440 桌面小，KPI 数字保持 32。
///
/// 字体句柄要一直有效，所以整个面板生命周期内都由 [`crate::canvas`] 持有。
pub struct Fonts {
    page_title: nwg::Font,
    section: nwg::Font,
    kpi: nwg::Font,
    kpi_mono: nwg::Font,
    body: nwg::Font,
    body_medium: nwg::Font,
    small: nwg::Font,
    eyebrow: nwg::Font,
    /// 按钮标签：深色 500（更轻，贴 Linear 语气）。
    button_dark: nwg::Font,
    /// 按钮标签：浅色 700（更重，贴 Meta 语气）。
    button_light: nwg::Font,
    mono: nwg::Font,
}

impl Fonts {
    /// 一次性创建全部字体。
    ///
    /// `scale` 是字号缩放系数：1.0 对应规范的排版表，
    /// 窗口放大时画布会按宽度比例传入更大的值（封顶 1.4），
    /// 让文字跟卡片一起长大，而不是文字不变、留白越来越大。
    #[allow(clippy::new_without_default)]
    pub fn new(scale: f32) -> Self {
        let sans = resolve_family(SANS_CANDIDATES);
        let mono = resolve_family(MONO_CANDIDATES);
        let px = |size: u32| ((size as f32 * scale).round() as u32).max(1);

        Self {
            page_title: sans_font(sans, px(26), 600),
            section: sans_font(sans, px(20), 600),
            kpi: sans_font(sans, px(32), 700),
            kpi_mono: mono_font(mono, px(32)),
            body: sans_font(sans, px(17), 400),
            body_medium: sans_font(sans, px(17), 500),
            small: sans_font(sans, px(16), 400),
            eyebrow: sans_font(sans, px(13), 500),
            button_dark: sans_font(sans, px(16), 500),
            button_light: sans_font(sans, px(16), 700),
            mono: mono_font(mono, px(16)),
        }
    }

    /// 页面标题（中文大字）。
    pub fn page_title(&self) -> HFONT {
        self.page_title.handle
    }

    /// 区块标题。
    pub fn section(&self) -> HFONT {
        self.section.handle
    }

    /// KPI 数值。
    pub fn kpi(&self) -> HFONT {
        self.kpi.handle
    }

    /// KPI 数值（等宽，端口与编号用）。
    pub fn kpi_mono(&self) -> HFONT {
        self.kpi_mono.handle
    }

    /// 正文。
    pub fn body(&self) -> HFONT {
        self.body.handle
    }

    /// 强调正文。
    pub fn body_medium(&self) -> HFONT {
        self.body_medium.handle
    }

    /// 次级文字。
    pub fn small(&self) -> HFONT {
        self.small.handle
    }

    /// 英文小标签（大字距）。
    pub fn eyebrow(&self) -> HFONT {
        self.eyebrow.handle
    }

    /// 按钮标签。
    ///
    /// 规范的排版表：按钮标签 700，但深色主题更轻（500）、浅色更重（700），
    /// 与两套品牌的语气一致。
    pub fn button(&self, dark: bool) -> HFONT {
        if dark {
            self.button_dark.handle
        } else {
            self.button_light.handle
        }
    }

    /// 等宽文字。
    pub fn mono(&self) -> HFONT {
        self.mono.handle
    }

    /// 等宽字体（供 NWG 控件直接使用）。
    pub fn mono_ref(&self) -> &nwg::Font {
        &self.mono
    }
}

/// 创建一个界面字体。
///
/// `size` 的单位是**逻辑像素**，NWG 会按系统 DPI 缩放。
/// 注意 NWG 把它直接交给 `CreateFontW` 当作字符高度（`nHeight`），
/// 所以这里给的是「字身框高」而不是磅值。
fn sans_font(family: &str, size: u32, weight: u32) -> nwg::Font {
    let mut font = Default::default();
    if let Err(error) = nwg::Font::builder()
        .family(family)
        .size(size.max(1))
        .weight(weight)
        .build(&mut font)
    {
        eprintln!("字体创建失败（{family} {size}px）: {error}");
    }
    font
}

/// 创建一个等宽字体。
fn mono_font(family: &str, size: u32) -> nwg::Font {
    let mut font = Default::default();
    if let Err(error) = nwg::Font::builder()
        .family(family)
        .size(size.max(1))
        .weight(400)
        .build(&mut font)
    {
        eprintln!("等宽字体创建失败（{family} {size}px）: {error}");
    }
    font
}

/// 窗口与布局的固定尺寸。
///
/// 规范里的骨架是 1440 桌面，控制面板是常驻的小窗口，
/// 因此按同样比例收窄：侧边栏仍固定 240，内容区约 750。
///
/// 面板支持缩放：下面的宽高是**默认**尺寸，
/// 实际尺寸由窗口决定，布局按当前尺寸重排。
/// 侧边栏宽度与内边距是固定的（规范要求导航固定），
/// 其余部分随窗口伸缩。
pub mod metrics {
    /// 默认窗口宽（客户区）。
    pub const WINDOW_W: i32 = 1200;
    /// 默认窗口高（客户区）。
    pub const WINDOW_H: i32 = 860;
    /// 最小窗口宽：再窄的话 KPI 卡与按钮就挤不下了。
    pub const MIN_W: i32 = 1080;
    /// 最小窗口高：再矮的话服务页的三张卡片会重叠。
    pub const MIN_H: i32 = 780;
    /// 侧边栏宽（规范要求固定导航）。
    pub const SIDEBAR_W: i32 = 240;
    /// 顶栏高。
    pub const TOPBAR_H: i32 = 84;
    /// 内容区内边距。
    pub const PAD: i32 = 24;
    /// 卡片之间的间距。
    ///
    /// 所有纵向留白都用这一个值，间距才统一。
    pub const GAP: i32 = 24;
    /// 字段列表一行的高度。
    pub const ROW_H: i32 = 40;
    /// 字段列表区顶部的标题高度（卡片标题 + 留白）。
    pub const LIST_HEADER_H: i32 = 76;
    /// 字段卡片底部留白。
    pub const LIST_FOOTER_H: i32 = 24;
}
