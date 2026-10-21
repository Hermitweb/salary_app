//! 控制面板的配色与字体。
//!
//! 与网页端（`src-tauri/src/styles.css`）使用同一套视觉语言，
//! 让原生窗口和浏览器里的页面看起来是同一个产品。
//!
//! 颜色一律用 `[r, g, b]` 表示，方便直接交给 GDI 画刷。

use native_windows_gui as nwg;
use winapi::shared::windef::HFONT;

/// 浅蓝底：横幅、强调块。
pub const PRIMARY_SOFT: [u8; 3] = [0xef, 0xf6, 0xff];
/// 主色：强调色块、进度条。
pub const PRIMARY: [u8; 3] = [0x3b, 0x82, 0xf6];
/// 成功底色：服务运行中。
pub const SUCCESS_SOFT: [u8; 3] = [0xdc, 0xfa, 0xe5];
/// 成功文字色。
pub const SUCCESS: [u8; 3] = [0x16, 0x7a, 0x43];
/// 危险底色：服务已停止。
pub const DANGER_SOFT: [u8; 3] = [0xfe, 0xe2, 0xe2];
/// 危险文字色。
pub const DANGER: [u8; 3] = [0xb9, 0x1c, 0x1c];
/// 灰底：卡片。
pub const MUTED_SOFT: [u8; 3] = [0xf1, 0xf5, 0xf9];
/// 白色：列表底色、卡片上的文字。
pub const SURFACE: [u8; 3] = [0xff, 0xff, 0xff];
/// 标题文字色。
pub const INK: [u8; 3] = [0x1a, 0x23, 0x32];
/// 次要文字色。
pub const INK_MUTED: [u8; 3] = [0x5b, 0x6b, 0x7d];
/// 分隔线颜色。
pub const HAIRLINE: [u8; 3] = [0xe2, 0xe8, 0xf0];

/// 界面字体。
pub const FONT_FAMILY: &str = "Microsoft YaHei UI";
/// 等宽字体：地址、端口等。
pub const FONT_MONO: &str = "Consolas";

/// 粗体字重。
pub const WEIGHT_BOLD: u32 = 600;

/// 面板用到的字体。
///
/// 字体句柄要一直有效，所以整个面板生命周期内都由 [`crate::panel`] 持有。
pub struct Fonts {
    title: nwg::Font,
    section: nwg::Font,
    body: nwg::Font,
    small: nwg::Font,
    pill: nwg::Font,
    mono: nwg::Font,
}

impl Fonts {
    /// 一次性创建全部字体。
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            title: ui_font(21, true),
            section: ui_font(16, true),
            body: ui_font(15, false),
            small: ui_font(14, false),
            pill: ui_font(15, true),
            mono: mono_font(15),
        }
    }

    /// 大标题的字体句柄。
    pub fn title(&self) -> HFONT {
        self.title.handle
    }

    /// 小节标题的字体句柄。
    pub fn section(&self) -> HFONT {
        self.section.handle
    }

    /// 正文的字体句柄。
    pub fn body(&self) -> HFONT {
        self.body.handle
    }

    /// 辅助说明的字体句柄。
    pub fn small(&self) -> HFONT {
        self.small.handle
    }

    /// 状态标签的字体句柄。
    pub fn pill(&self) -> HFONT {
        self.pill.handle
    }

    /// 等宽字体的句柄。
    pub fn mono(&self) -> HFONT {
        self.mono.handle
    }

    /// 等宽字体（供 NWG 控件直接使用）。
    pub fn mono_ref(&self) -> &nwg::Font {
        &self.mono
    }
}

/// 创建一个字体。
///
/// `size` 的单位是**逻辑像素**，NWG 会按系统 DPI 缩放。
/// 注意 NWG 把它直接交给 `CreateFontW` 当作字符高度（`nHeight`），
/// 所以这里给的是「字身框高」而不是磅值。
pub fn font(family: &str, size: u32, weight: u32) -> nwg::Font {
    let mut font = nwg::Font::default();
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

/// 界面字体。
pub fn ui_font(size: u32, bold: bool) -> nwg::Font {
    font(FONT_FAMILY, size, if bold { WEIGHT_BOLD } else { 400 })
}

/// 等宽字体。
pub fn mono_font(size: u32) -> nwg::Font {
    font(FONT_MONO, size, 400)
}
