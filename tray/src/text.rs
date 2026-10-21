//! 自绘文字控件：底色与文字一次画完。
//!
//! NWG 的 `Label` 走的是系统 STATIC 控件，底色由父窗口的
//! `WM_CTLCOLORSTATIC` 决定——放在卡片上时会画成一块不透明的白底，
//! 文字像是贴在白色小方块上。这里自己画一遍：
//! `WM_PAINT` 里先铺底色，再用 `TRANSPARENT` 背景模式写文字，
//! 文字与底色严丝合缝。
//!
//! 每个控件的样式（底色、文字色、字体、内容）存在窗口的
//! `GWL_USERDATA` 里，随窗口一起销毁，不留全局状态。

use native_windows_gui as nwg;
use winapi::shared::minwindef::{LPARAM, LRESULT, UINT, WPARAM};
use winapi::shared::windef::{HFONT, HWND, RECT};
use winapi::um::wingdi::{
    CreateSolidBrush, DeleteObject, RGB, SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
};
use winapi::um::winuser::{
    BeginPaint, CREATESTRUCTW, CreateWindowExW, DT_CENTER, DT_LEFT, DT_NOPREFIX, DT_SINGLELINE,
    DT_VCENTER, DefWindowProcW, DestroyWindow, DrawTextW, EndPaint, FillRect, GWL_USERDATA,
    GetClientRect, GetWindowLongPtrW, InvalidateRect, PAINTSTRUCT, RegisterClassExW,
    SetWindowLongPtrW, WM_ERASEBKGND, WM_NCCREATE, WM_NCDESTROY, WM_PAINT, WS_CHILD, WS_VISIBLE,
};

/// 窗口类名。
const CLASS_NAME: &str = "SalaryTextWindow";

/// 每个文字控件的绘制参数。
struct TextStyle {
    /// 底色画刷。
    brush: winapi::shared::windef::HBRUSH,
    /// 文字颜色。
    foreground: [u8; 3],
    /// 文字内容（UTF-16）。
    text: Vec<u16>,
    /// 字体。
    font: HFONT,
    /// 是否水平居中。
    centered: bool,
}

/// 窗口过程：铺底色 + 写文字。
///
/// 指针都来自 Win32 传进来的 `CREATESTRUCT`，有效性由调用方保证
/// （创建时传入的样式指针在这里被接管，窗口销毁时释放）。
unsafe extern "system" fn text_proc(
    hwnd: HWND,
    msg: UINT,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        match msg {
            WM_NCCREATE => {
                // 创建参数里带着样式的指针
                let create = lparam as *const CREATESTRUCTW;
                let style = (*create).lpCreateParams as *mut TextStyle;
                SetWindowLongPtrW(hwnd, GWL_USERDATA, style as isize);
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            WM_ERASEBKGND => {
                // 底色在 WM_PAINT 里画，这里直接说「已擦除」
                1
            }
            WM_PAINT => {
                let mut paint: PAINTSTRUCT = core::mem::zeroed();
                let dc = BeginPaint(hwnd, &mut paint);
                let style = GetWindowLongPtrW(hwnd, GWL_USERDATA) as *mut TextStyle;

                if !style.is_null() {
                    let style = &*style;
                    let mut rect: RECT = core::mem::zeroed();
                    GetClientRect(hwnd, &mut rect);

                    // 铺底色
                    FillRect(dc, &rect, style.brush);

                    // 写文字：透明背景，避免再盖一层方块
                    SetBkMode(dc, TRANSPARENT as i32);
                    SetTextColor(
                        dc,
                        RGB(
                            style.foreground[0],
                            style.foreground[1],
                            style.foreground[2],
                        ),
                    );
                    let old_font = SelectObject(dc, style.font as _);

                    let flags = DT_VCENTER
                        | DT_SINGLELINE
                        | DT_NOPREFIX
                        | if style.centered { DT_CENTER } else { DT_LEFT };
                    DrawTextW(
                        dc,
                        style.text.as_ptr(),
                        style.text.len() as i32,
                        &mut rect,
                        flags,
                    );

                    SelectObject(dc, old_font);
                }

                EndPaint(hwnd, &paint);
                0
            }
            WM_NCDESTROY => {
                // 窗口没了，样式和画刷一起回收
                let style = GetWindowLongPtrW(hwnd, GWL_USERDATA) as *mut TextStyle;
                if !style.is_null() {
                    DeleteObject((*style).brush as _);
                    drop(Box::from_raw(style));
                }
                SetWindowLongPtrW(hwnd, GWL_USERDATA, 0);
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// 进程内注册一次窗口类。
fn register_class_once() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let mut class_name = CLASS_NAME.encode_utf16().collect::<Vec<u16>>();
        class_name.push(0);

        let mut cls: winapi::um::winuser::WNDCLASSEXW = unsafe { core::mem::zeroed() };
        cls.cbSize = std::mem::size_of::<winapi::um::winuser::WNDCLASSEXW>() as _;
        cls.hInstance =
            unsafe { winapi::um::libloaderapi::GetModuleHandleW(std::ptr::null_mut()) } as _;
        cls.lpfnWndProc = Some(text_proc);
        cls.lpszClassName = class_name.as_ptr();

        let atom = unsafe { RegisterClassExW(&cls) };
        if atom == 0 {
            eprintln!("文字窗口类注册失败");
        }
    });
}

/// 一个自绘文字控件。
pub struct TextBlock {
    handle: HWND,
}

impl TextBlock {
    /// 在 `parent` 里创建一个带底色的文字控件。
    ///
    /// `font` 由调用方持有（见 [`crate::theme`]）。
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        parent: nwg::ControlHandle,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        background: [u8; 3],
        foreground: [u8; 3],
        text: &str,
        font: HFONT,
        centered: bool,
    ) -> Result<Self, String> {
        register_class_once();

        let parent_handle = parent
            .hwnd()
            .ok_or_else(|| "文字控件父窗口句柄获取失败".to_string())?;

        let style = Box::into_raw(Box::new(TextStyle {
            brush: unsafe { CreateSolidBrush(RGB(background[0], background[1], background[2])) },
            foreground,
            text: text.encode_utf16().collect(),
            font,
            centered,
        }));

        let mut class_name = CLASS_NAME.encode_utf16().collect::<Vec<u16>>();
        class_name.push(0);
        let window_text: Vec<u16> = vec![0];

        let handle = unsafe {
            CreateWindowExW(
                0,
                class_name.as_ptr(),
                window_text.as_ptr(),
                WS_CHILD | WS_VISIBLE,
                x,
                y,
                w,
                h,
                parent_handle,
                0 as _,
                std::ptr::null_mut(),
                style as _,
            )
        };

        if handle.is_null() {
            unsafe {
                DeleteObject((*style).brush as _);
                drop(Box::from_raw(style));
            }
            return Err("文字控件创建失败".to_string());
        }

        Ok(Self { handle })
    }

    /// 改文字并重绘。
    pub fn set_text(&self, text: &str) {
        unsafe {
            let style = GetWindowLongPtrW(self.handle, GWL_USERDATA) as *mut TextStyle;
            if !style.is_null() {
                (*style).text = text.encode_utf16().collect();
                InvalidateRect(self.handle, std::ptr::null(), 1);
            }
        }
    }

    /// 显示 / 隐藏。
    pub fn set_visible(&self, visible: bool) {
        unsafe {
            winapi::um::winuser::ShowWindow(
                self.handle,
                if visible {
                    winapi::um::winuser::SW_SHOWNOACTIVATE
                } else {
                    winapi::um::winuser::SW_HIDE
                },
            );
        }
    }
}

impl Drop for TextBlock {
    fn drop(&mut self) {
        unsafe {
            DestroyWindow(self.handle);
        }
    }
}
