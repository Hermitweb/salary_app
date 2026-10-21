//! 纯色块控件：GDI 自绘的矩形底色窗口。
//!
//! NWG 的 `Label` 带底色时靠 NCCALCSIZE 技巧，只把中间一条客户区涂成底色，
//! 做横幅、卡片背景时会露出一大片白底。这里注册一个独立窗口类，
//! 在 `WM_ERASEBKGND` 里整体填充，就能得到一块完整的纯色矩形。
//! 文字标签直接叠在上面（标签背景透明，露出底色）。

use native_windows_gui as nwg;
use std::collections::HashMap;
use std::mem::size_of;
use std::sync::Mutex;
use winapi::shared::basetsd::LONG_PTR;
use winapi::shared::minwindef::{HINSTANCE, HMODULE, LPARAM, LRESULT, UINT, WPARAM};
use winapi::shared::windef::{HBRUSH, HWND};
use winapi::um::errhandlingapi::GetLastError;
use winapi::um::libloaderapi::GetModuleHandleW;
use winapi::um::wingdi::{CreateSolidBrush, RGB};
use winapi::um::winuser::{
    COLOR_WINDOW, CreateWindowExW, DefWindowProcW, DestroyWindow, FillRect, GetClientRect,
    GetWindowLongPtrW, RegisterClassExW, SW_HIDE, SW_SHOWNOACTIVATE, SetWindowLongPtrW, ShowWindow,
    WM_DESTROY, WM_ERASEBKGND, WM_NCCREATE, WNDCLASSEXW, WS_CHILD, WS_EX_CONTROLPARENT, WS_VISIBLE,
};

/// 窗口类名。
const CLASS_NAME: &str = "SalarySolidWindow";

/// 模块句柄（窗口类注册与创建需要）。
fn module() -> HMODULE {
    unsafe { GetModuleHandleW(std::ptr::null_mut()) }
}

/// 把颜色打包进 `GWL_USERDATA`。
fn pack(rgb: [u8; 3]) -> usize {
    ((rgb[0] as u32) << 16 | (rgb[1] as u32) << 8 | rgb[2] as u32) as usize
}

/// 解包 `GWL_USERDATA` 里的颜色。
fn unpack(value: LONG_PTR) -> [u8; 3] {
    [
        (((value as u32) >> 16) & 0xff) as u8,
        (((value as u32) >> 8) & 0xff) as u8,
        ((value as u32) & 0xff) as u8,
    ]
}

/// 画刷包一层，让它能放进跨线程 `static`。
struct Brush(HBRUSH);

// 画刷句柄只在 UI 线程里创建和使用
unsafe impl Send for Brush {}
unsafe impl Sync for Brush {}

/// 按颜色缓存画刷，避免重复创建。
fn solid_brush(rgb: [u8; 3]) -> HBRUSH {
    static BRUSHES: Mutex<Option<HashMap<[u8; 3], Brush>>> = Mutex::new(None);

    let mut guard = BRUSHES.lock().unwrap();
    let map = guard.get_or_insert_with(HashMap::new);
    if let Some(brush) = map.get(&rgb) {
        return brush.0;
    }

    let brush = unsafe { CreateSolidBrush(RGB(rgb[0], rgb[1], rgb[2])) };
    map.insert(rgb, Brush(brush));
    brush
}

/// 窗口过程：整体填充底色。
#[allow(clippy::missing_safety_doc)]
unsafe extern "system" fn solid_proc(
    hwnd: HWND,
    msg: UINT,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    use winapi::shared::windef::RECT;
    use winapi::um::winuser::{CREATESTRUCTW, GWL_USERDATA};

    match msg {
        WM_NCCREATE => {
            // 创建时传入的颜色在 CREATESTRUCT.lpCreateParams 里，
            // 这里存进 GWL_USERDATA，绘制时再取出来。
            // （CreateWindowEx 的 lpParam 不会自动写进 USERDATA）
            unsafe {
                let create = lparam as *const CREATESTRUCTW;
                let packed = (*create).lpCreateParams as isize;
                SetWindowLongPtrW(hwnd, GWL_USERDATA, packed);
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
        }
        WM_ERASEBKGND => {
            unsafe {
                let rgb = unpack(GetWindowLongPtrW(hwnd, GWL_USERDATA));
                let mut rect: RECT = core::mem::zeroed();
                GetClientRect(hwnd, &mut rect);
                FillRect(wparam as _, &rect, solid_brush(rgb));
            }
            1
        }
        WM_DESTROY => 0,
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// 进程内注册一次窗口类。
fn register_class_once() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let mut class_name = CLASS_NAME.encode_utf16().collect::<Vec<u16>>();
        class_name.push(0);

        let mut cls: WNDCLASSEXW = unsafe { core::mem::zeroed() };
        cls.cbSize = size_of::<WNDCLASSEXW>() as _;
        cls.hInstance = module() as HINSTANCE;
        cls.lpfnWndProc = Some(solid_proc);
        cls.lpszClassName = class_name.as_ptr();
        cls.hbrBackground = COLOR_WINDOW as _;

        let atom = unsafe { RegisterClassExW(&cls) };
        if atom == 0 {
            let code = unsafe { GetLastError() };
            eprintln!("纯色块窗口类注册失败（错误码 {code}）");
        }
    });
}

/// 一个纯色块窗口。
pub struct SolidBlock {
    handle: HWND,
}

impl SolidBlock {
    /// 在 `parent` 里创建一个 `w × h`、位于 `(x, y)` 的纯色块。
    pub fn create(
        parent: nwg::ControlHandle,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        rgb: [u8; 3],
    ) -> Result<Self, String> {
        register_class_once();

        let parent_handle = parent
            .hwnd()
            .ok_or_else(|| "纯色块父窗口句柄获取失败".to_string())?;

        let mut class_name = CLASS_NAME.encode_utf16().collect::<Vec<u16>>();
        class_name.push(0);
        let window_text: Vec<u16> = vec![0];

        let handle = unsafe {
            CreateWindowExW(
                WS_EX_CONTROLPARENT,
                class_name.as_ptr(),
                window_text.as_ptr(),
                WS_CHILD | WS_VISIBLE,
                x,
                y,
                w,
                h,
                parent_handle,
                0 as _,
                module(),
                pack(rgb) as _,
            )
        };

        if handle.is_null() {
            let code = unsafe { GetLastError() };
            Err(format!("纯色块创建失败（错误码 {code}）"))
        } else {
            Ok(Self { handle })
        }
    }

    /// 显示 / 隐藏。
    pub fn set_visible(&self, visible: bool) {
        unsafe {
            ShowWindow(
                self.handle,
                if visible { SW_SHOWNOACTIVATE } else { SW_HIDE },
            );
        }
    }

    /// 底层句柄。
    pub fn handle(&self) -> HWND {
        self.handle
    }
}

impl Drop for SolidBlock {
    fn drop(&mut self) {
        unsafe {
            DestroyWindow(self.handle);
        }
    }
}
