//! GDI 绘制原语：圆角矩形、发丝线、圆点与文字。
//!
//! 规范里的卡片是 12px 圆角、按钮是胶囊、分割线是 1px 发丝线，
//! 这些都不是 Win32 的系统控件能直接表达的，所以自己画。
//!
//! 画刷与画笔按颜色缓存，避免每帧重复创建 GDI 对象。

use std::collections::HashMap;
use std::sync::Mutex;
use winapi::shared::windef::{HBRUSH, HDC, RECT, SIZE};
use winapi::um::wingdi::{
    CreatePen, CreateRoundRectRgn, CreateSolidBrush, DeleteObject, FillRgn, FrameRgn,
    GetTextExtentPoint32W, LineTo, MoveToEx, PS_SOLID, RGB, SelectObject, SetBkMode,
    SetTextCharacterExtra, SetTextColor, TRANSPARENT,
};
use winapi::um::winuser::{
    DT_CENTER, DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX, DT_RIGHT, DT_SINGLELINE, DT_VCENTER,
    DrawTextW, FillRect,
};

/// 一个矩形（整数像素）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    /// 新建矩形。
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    /// 右边界。
    pub fn right(&self) -> i32 {
        self.x + self.w
    }

    /// 下边界。
    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }

    /// 点是否落在矩形内。
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.right() && py >= self.y && py < self.bottom()
    }

    /// 向内缩进。
    pub fn inset(&self, dx: i32, dy: i32) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.w - dx * 2, self.h - dy * 2)
    }

    /// 整体平移。
    pub fn offset(&self, dx: i32, dy: i32) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.w, self.h)
    }
}

/// 文字对齐方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// 画刷包装：GDI 对象只在界面线程里用，但缓存是全局的，
/// 所以手动声明它是 Send（画刷句柄本身可以跨线程传递，只是不能并发使用）。
struct Brush(HBRUSH);

// 画刷句柄只在 UI 线程里创建和使用
unsafe impl Send for Brush {}
unsafe impl Sync for Brush {}

/// 画刷缓存：同色只创建一次。
pub fn solid_brush(color: [u8; 3]) -> HBRUSH {
    brush(color)
}

/// 画刷缓存：同色只创建一次。
fn brush(color: [u8; 3]) -> HBRUSH {
    static BRUSHES: Mutex<Option<HashMap<[u8; 3], Brush>>> = Mutex::new(None);

    let mut guard = BRUSHES.lock().unwrap();
    let map = guard.get_or_insert_with(HashMap::new);
    if let Some(brush) = map.get(&color) {
        return brush.0;
    }

    let brush = unsafe { CreateSolidBrush(RGB(color[0], color[1], color[2])) };
    map.insert(color, Brush(brush));
    brush
}

/// 填充实心矩形。
pub fn fill_rect(dc: HDC, rect: Rect, color: [u8; 3]) {
    let area = RECT {
        left: rect.x,
        top: rect.y,
        right: rect.right(),
        bottom: rect.bottom(),
    };

    unsafe {
        FillRect(dc, &area, brush(color));
    }
}

/// 填充圆角矩形。
pub fn fill_round(dc: HDC, rect: Rect, radius: i32, color: [u8; 3]) {
    if rect.w <= 0 || rect.h <= 0 {
        return;
    }
    if radius <= 0 {
        fill_rect(dc, rect, color);
        return;
    }

    // 圆角太大时 GDI 会自己夹紧，这里再兜一层，避免变成「药丸」
    let max_radius = rect.w.min(rect.h) / 2;
    let radius = radius.min(max_radius);

    let region = unsafe {
        CreateRoundRectRgn(
            rect.x,
            rect.y,
            rect.right() + 1,
            rect.bottom() + 1,
            radius * 2,
            radius * 2,
        )
    };
    if region.is_null() {
        return;
    }

    unsafe {
        FillRgn(dc, region, brush(color));
        DeleteObject(region as _);
    }
}

/// 给圆角矩形描 1px 边。
pub fn stroke_round(dc: HDC, rect: Rect, radius: i32, color: [u8; 3]) {
    if rect.w <= 0 || rect.h <= 0 {
        return;
    }
    if radius <= 0 {
        fill_rect(dc, Rect::new(rect.x, rect.y, rect.w, 1), color);
        fill_rect(dc, Rect::new(rect.x, rect.bottom() - 1, rect.w, 1), color);
        return;
    }

    let max_radius = rect.w.min(rect.h) / 2;
    let radius = radius.min(max_radius);

    let region = unsafe {
        CreateRoundRectRgn(
            rect.x,
            rect.y,
            rect.right() + 1,
            rect.bottom() + 1,
            radius * 2,
            radius * 2,
        )
    };
    if region.is_null() {
        return;
    }

    unsafe {
        FrameRgn(dc, region, brush(color), 1, 1);
        DeleteObject(region as _);
    }
}

/// 填充圆形（状态点）。
///
/// 用「圆角半径等于半径的圆角矩形」画圆——GDI 没有单独的画圆原语，
/// 而圆角矩形的角椭圆直径等于边长时就是正圆。
pub fn fill_circle(dc: HDC, cx: i32, cy: i32, radius: i32, color: [u8; 3]) {
    let region = unsafe {
        CreateRoundRectRgn(
            cx - radius,
            cy - radius,
            cx + radius + 1,
            cy + radius + 1,
            radius * 2,
            radius * 2,
        )
    };
    if region.is_null() {
        return;
    }

    unsafe {
        FillRgn(dc, region, brush(color));
        DeleteObject(region as _);
    }
}

/// 画一条直线。
///
/// 品牌标记的 ¥ 是几笔直线拼出来的，GDI 的画线原语直接画最省事。
pub fn line(dc: HDC, x1: i32, y1: i32, x2: i32, y2: i32, color: [u8; 3]) {
    let pen = unsafe { CreatePen(PS_SOLID as i32, 1, RGB(color[0], color[1], color[2])) };
    if pen.is_null() {
        return;
    }

    unsafe {
        let previous = SelectObject(dc, pen as _);
        MoveToEx(dc, x1, y1, std::ptr::null_mut());
        LineTo(dc, x2, y2);
        SelectObject(dc, previous);
        DeleteObject(pen as _);
    }
}

/// 水平发丝线。
pub fn hline(dc: HDC, x: i32, y: i32, w: i32, color: [u8; 3]) {
    fill_rect(dc, Rect::new(x, y, w, 1), color);
}

/// 垂直发丝线。
pub fn vline(dc: HDC, x: i32, y: i32, h: i32, color: [u8; 3]) {
    fill_rect(dc, Rect::new(x, y, 1, h), color);
}

/// 画一段文字。
///
/// `tracking` 是字间距（逻辑像素），英文小标签用它拉开字距。
pub fn text(
    dc: HDC,
    rect: Rect,
    content: &str,
    font: winapi::shared::windef::HFONT,
    color: [u8; 3],
    align: Align,
    tracking: i32,
) {
    if content.is_empty() {
        return;
    }

    let utf16: Vec<u16> = content.encode_utf16().collect();
    let mut area = RECT {
        left: rect.x,
        top: rect.y,
        right: rect.right(),
        bottom: rect.bottom(),
    };

    let flags = DT_VCENTER
        | DT_SINGLELINE
        | DT_NOPREFIX
        | DT_END_ELLIPSIS
        | match align {
            Align::Left => DT_LEFT,
            Align::Center => DT_CENTER,
            Align::Right => DT_RIGHT,
        };

    unsafe {
        let previous = SelectObject(dc, font as _);
        SetBkMode(dc, TRANSPARENT as i32);
        SetTextColor(dc, RGB(color[0], color[1], color[2]));
        SetTextCharacterExtra(dc, tracking);
        DrawTextW(dc, utf16.as_ptr(), utf16.len() as i32, &mut area, flags);
        SetTextCharacterExtra(dc, 0);
        SelectObject(dc, previous);
    }
}

/// 量一段文字的宽度。
pub fn text_width(dc: HDC, content: &str, font: winapi::shared::windef::HFONT) -> i32 {
    if content.is_empty() {
        return 0;
    }

    let utf16: Vec<u16> = content.encode_utf16().collect();
    let mut size: SIZE = unsafe { core::mem::zeroed() };

    unsafe {
        let previous = SelectObject(dc, font as _);
        GetTextExtentPoint32W(dc, utf16.as_ptr(), utf16.len() as i32, &mut size);
        SelectObject(dc, previous);
    }

    size.cx
}
