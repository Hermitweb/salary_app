//! 品牌标记：侧边栏 logo、标题栏图标、任务栏图标、托盘图标全部用同一个图形。
//!
//! 图形直接沿用 Python 原版的 `app_icon.ico`（蓝色圆角方块 + 白色 ¥），
//! 不再自己画一套——同一个应用换语言、换实现之后，图标应当一眼认得出是它。
//!
//! 做法是把这份 .ico 作为 Windows 资源内嵌进 exe，然后：
//! - 标题栏与任务栏：直接从资源里 `LoadImage` 出来；
//! - 托盘图标与侧边栏 logo：把系统解码好的图标位图像素取出来（`GetIconInfo`），
//!   转成 RGBA 复用。PNG 解码交给系统，不在 Rust 里另写一套。
//!
//! 四处用的是同一份资源，因此永远是同一个图标。

use crate::theme::{Palette, Theme};
use std::mem::size_of;
use winapi::shared::minwindef::HINSTANCE;
use winapi::shared::windef::{HBITMAP, HDC, HICON};
use winapi::um::wingdi::DeleteObject;
use winapi::um::winuser::{
    GetIconInfo, ICONINFO, IMAGE_ICON, LR_DEFAULTSIZE, LR_LOADFROMFILE, LoadImageW,
};

/// 图标资源 ID（与 `salary-tray.rc` 里的定义一致）。
const ICON_RESOURCE_ID: usize = 1;

/// 侧边栏 logo 的边长。
pub const LOGO_SIZE: i32 = 28;

/// 当前主题的品牌底色。
pub fn color_of(theme: Theme) -> [u8; 3] {
    Palette::of(theme).primary
}

/// 从 exe 资源里加载应用图标。
fn load_icon(instance: HINSTANCE, size: i32) -> Option<HICON> {
    unsafe {
        let handle = LoadImageW(
            instance,
            ICON_RESOURCE_ID as *const u16 as *const _,
            IMAGE_ICON,
            size,
            size,
            // 指定尺寸就不要 LR_DEFAULTSIZE，否则拿的是系统默认尺寸
            0,
        );
        if handle.is_null() {
            None
        } else {
            Some(handle as HICON)
        }
    }
}

/// 从 exe 资源里加载应用图标（标题栏与任务栏）。
pub fn window_icon(instance: HINSTANCE) -> Option<HICON> {
    unsafe {
        let handle = LoadImageW(
            instance,
            ICON_RESOURCE_ID as *const u16 as *const _,
            IMAGE_ICON,
            0,
            0,
            LR_DEFAULTSIZE,
        );
        if handle.is_null() {
            None
        } else {
            Some(handle as HICON)
        }
    }
}

/// 从 .ico 文件里加载应用图标（备用路径：资源不可用时用文件）。
pub fn window_icon_from_file(path: &std::path::Path) -> Option<HICON> {
    let wide: Vec<u16> = path
        .as_os_str()
        .to_string_lossy()
        .encode_utf16()
        .chain(Some(0))
        .collect();

    unsafe {
        let handle = LoadImageW(
            core::ptr::null_mut(),
            wide.as_ptr(),
            IMAGE_ICON,
            0,
            0,
            LR_LOADFROMFILE | LR_DEFAULTSIZE,
        );
        if handle.is_null() {
            None
        } else {
            Some(handle as HICON)
        }
    }
}

/// 取出图标的像素（RGBA，每像素 4 字节）。
///
/// 图标是系统解码过的，取它的位图像素即可，
/// 不用在 Rust 里实现一遍 PNG 解码。
pub fn icon_rgba(instance: HINSTANCE, size: i32) -> Option<Vec<u8>> {
    let hicon = load_icon(instance, size)?;

    unsafe {
        let mut info: ICONINFO = core::mem::zeroed();
        if GetIconInfo(hicon, &mut info) == 0 {
            return None;
        }

        let hbm = info.hbmColor;
        if hbm.is_null() {
            return None;
        }

        // 图标位图是设备相关的（bmBits 为空），要用 GetDIBits 才能取到像素
        let width = size as usize;
        let height = size as usize;

        let dc = winapi::um::wingdi::CreateCompatibleDC(core::ptr::null_mut());
        if dc.is_null() {
            DeleteObject(hbm as _);
            if !info.hbmMask.is_null() {
                DeleteObject(info.hbmMask as _);
            }
            return None;
        }

        let mut header: winapi::um::wingdi::BITMAPINFO = core::mem::zeroed();
        header.bmiHeader.biSize = size_of::<winapi::um::wingdi::BITMAPINFOHEADER>() as u32;
        header.bmiHeader.biWidth = size;
        header.bmiHeader.biHeight = -size;
        header.bmiHeader.biPlanes = 1;
        header.bmiHeader.biBitCount = 32;
        header.bmiHeader.biCompression = winapi::um::wingdi::BI_RGB;

        let mut pixels = vec![0u8; width * height * 4];
        let lines = winapi::um::wingdi::GetDIBits(
            dc,
            hbm as *mut _,
            0,
            height as u32,
            pixels.as_mut_ptr() as *mut _,
            &mut header as *mut _,
            winapi::um::wingdi::DIB_RGB_COLORS,
        );
        if lines == 0 {
            winapi::um::wingdi::DeleteDC(dc);
            DeleteObject(hbm as _);
            if !info.hbmMask.is_null() {
                DeleteObject(info.hbmMask as _);
            }
            return None;
        }

        // 透明信息：优先用颜色位图自带的 alpha，没有就用 AND 掩码
        let has_alpha = pixels
            .chunks(4)
            .any(|pixel| pixel[3] != 0 && pixel[3] != 0xff);
        if !has_alpha && !info.hbmMask.is_null() {
            let mut mask_header: winapi::um::wingdi::BITMAPINFO = core::mem::zeroed();
            mask_header.bmiHeader.biSize = size_of::<winapi::um::wingdi::BITMAPINFOHEADER>() as u32;
            mask_header.bmiHeader.biWidth = size;
            mask_header.bmiHeader.biHeight = -size;
            mask_header.bmiHeader.biPlanes = 1;
            mask_header.bmiHeader.biBitCount = 1;
            mask_header.bmiHeader.biCompression = winapi::um::wingdi::BI_RGB;

            let stride = (width).div_ceil(32) * 4;
            let mut mask = vec![0u8; stride * height];
            let mask_lines = winapi::um::wingdi::GetDIBits(
                dc,
                info.hbmMask as *mut _,
                0,
                height as u32,
                mask.as_mut_ptr() as *mut _,
                &mut mask_header as *mut _,
                winapi::um::wingdi::DIB_RGB_COLORS,
            );
            if mask_lines > 0 {
                for y in 0..height {
                    for x in 0..width {
                        let byte = mask[y * stride + x / 8];
                        let transparent = (byte >> (7 - x % 8)) & 1 == 1;
                        let index = (y * width + x) * 4;
                        pixels[index + 3] = if transparent { 0 } else { 0xff };
                    }
                }
            }
        }

        DeleteObject(hbm as _);
        if !info.hbmMask.is_null() {
            DeleteObject(info.hbmMask as _);
        }
        winapi::um::wingdi::DeleteDC(dc);
        DeleteObject(hicon as _);

        Some(pixels)
    }
}

/// 托盘图标用的 RGBA（32×32）。
pub fn tray_rgba(instance: HINSTANCE) -> Option<(Vec<u8>, u32)> {
    icon_rgba(instance, 32).map(|pixels| (pixels, 32))
}

/// 本进程模块句柄（资源里的图标要用它来取）。
pub fn module_handle() -> HINSTANCE {
    unsafe { winapi::um::libloaderapi::GetModuleHandleW(core::ptr::null_mut()) }
}

/// 兜底图标：资源解不出来时画一个纯色方块。
pub fn fallback_rgba(size: u32, color: [u8; 3]) -> Vec<u8> {
    let mut pixels = vec![0u8; (size * size * 4) as usize];
    for index in 0..(size * size) as usize {
        let offset = index * 4;
        pixels[offset] = color[0];
        pixels[offset + 1] = color[1];
        pixels[offset + 2] = color[2];
        pixels[offset + 3] = 0xff;
    }
    pixels
}

/// 把图标贴到窗口上（标题栏 + 任务栏）。
pub fn apply_window_icon(hwnd: winapi::shared::windef::HWND, instance: HINSTANCE) {
    use winapi::um::winuser::{ICON_BIG, ICON_SMALL, SendMessageW, WM_SETICON};

    let big = window_icon(instance);
    let small = window_icon(instance);
    unsafe {
        if let Some(big) = big {
            SendMessageW(hwnd, WM_SETICON, ICON_BIG as usize, big as isize);
        }
        if let Some(small) = small {
            SendMessageW(hwnd, WM_SETICON, ICON_SMALL as usize, small as isize);
        }
    }
}

/// 侧边栏 logo：把图标像素按主题底色预合成成一张不透明的位图。
///
/// 合成一次、之后每次重绘只是一次贴图，
/// 不会因为逐像素写而拖慢每一次重绘。
pub struct Logo {
    /// 内存 DC。
    dc: HDC,
    /// 位图句柄。
    bitmap: HBITMAP,
    /// 之前选中的对象。
    previous: HBITMAP,
    /// DIB 的像素地址。
    bits: *mut u8,
    /// 底图尺寸。
    size: i32,
    /// 当前合成时用的底色。
    background: [u8; 3],
    /// 原始 RGBA 像素。
    source: Vec<u8>,
    /// 原始边长。
    source_size: u32,
}

impl Logo {
    /// 创建 logo（从 exe 资源里的应用图标取像素）。
    pub fn new(instance: HINSTANCE) -> Option<Self> {
        let pixels = icon_rgba(instance, 32)?;
        let size = 32i32;

        unsafe {
            let mut info: winapi::um::wingdi::BITMAPINFO = core::mem::zeroed();
            info.bmiHeader.biSize = size_of::<winapi::um::wingdi::BITMAPINFOHEADER>() as u32;
            info.bmiHeader.biWidth = size;
            // 负高度表示自上而下，和画布坐标一致
            info.bmiHeader.biHeight = -size;
            info.bmiHeader.biPlanes = 1;
            info.bmiHeader.biBitCount = 32;
            info.bmiHeader.biCompression = winapi::um::wingdi::BI_RGB;

            let mut bits: *mut winapi::ctypes::c_void = core::ptr::null_mut();
            let bitmap = winapi::um::wingdi::CreateDIBSection(
                core::ptr::null_mut(),
                &info,
                winapi::um::wingdi::DIB_RGB_COLORS,
                &mut bits,
                core::ptr::null_mut(),
                0,
            );
            if bitmap.is_null() {
                return None;
            }

            let dc = winapi::um::wingdi::CreateCompatibleDC(core::ptr::null_mut());
            if dc.is_null() {
                winapi::um::wingdi::DeleteObject(bitmap as _);
                return None;
            }

            let previous = winapi::um::wingdi::SelectObject(dc, bitmap as _);

            Some(Self {
                dc,
                bitmap,
                previous: previous as _,
                bits: bits as *mut u8,
                size,
                background: [0, 0, 0],
                source: pixels,
                source_size: 32,
            })
        }
    }

    /// 主题变了就按新底色重新合成。
    ///
    /// 合成结果直接写进 DIB 的像素里，之后每次重绘只是一次贴图。
    pub fn update(&mut self, background: [u8; 3]) {
        if self.background == background {
            return;
        }
        self.background = background;

        let side = self.source_size as usize;
        let stride = self.size as usize * 4;

        for y in 0..side {
            for x in 0..side {
                let source_index = (y * side + x) * 4;
                if source_index + 3 >= self.source.len() {
                    continue;
                }
                let alpha = self.source[source_index + 3] as u32;
                let index = y * stride + x * 4;

                // DIB 是 BGRA 顺序
                for (channel, base) in background.iter().enumerate() {
                    let foreground = self.source[source_index + channel] as u32;
                    let base = *base as u32;
                    let mixed = (foreground * alpha + base * (255 - alpha)) / 255;
                    unsafe {
                        *self.bits.add(index + (2 - channel)) = mixed as u8;
                    }
                }
                unsafe {
                    *self.bits.add(index + 3) = 0xff;
                }
            }
        }
    }

    /// 把 logo 画到目标矩形。
    pub fn draw(&self, dc: HDC, dest: crate::paint::Rect) {
        unsafe {
            winapi::um::wingdi::StretchBlt(
                dc,
                dest.x,
                dest.y,
                dest.w,
                dest.h,
                self.dc,
                0,
                0,
                self.size,
                self.size,
                winapi::um::wingdi::SRCCOPY,
            );
        }
    }
}

impl Drop for Logo {
    fn drop(&mut self) {
        unsafe {
            winapi::um::wingdi::SelectObject(self.dc, self.previous as _);
            winapi::um::wingdi::DeleteDC(self.dc);
            winapi::um::wingdi::DeleteObject(self.bitmap as _);
        }
    }
}
