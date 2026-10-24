//! 剪贴板：把访问地址复制出去。
//!
//! 原版 Tkinter 面板有「复制 URL」和「一键复制所有信息」两个按钮，
//! 运维经常要把地址发到群里，Rust 版也补上。
//!
//! 自己实现而不是引依赖：Windows 剪贴板的最小可用流程就三步
//! （打开、清空、写入全局内存）。

use winapi::shared::minwindef::HGLOBAL;
use winapi::um::winbase::{GMEM_MOVEABLE, GlobalAlloc, GlobalFree, GlobalLock};
use winapi::um::winuser::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};

/// 剪贴板上的 Unicode 文本格式。
const CF_UNICODETEXT: u32 = 13;

/// 把一段文字放进剪贴板。
pub fn copy_text(text: &str) -> bool {
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return false;
        }

        // UTF-16 + 结尾的 0
        let mut wide: Vec<u16> = text.encode_utf16().collect();
        wide.push(0);
        let bytes = wide.len() * std::mem::size_of::<u16>();

        let handle: HGLOBAL = GlobalAlloc(GMEM_MOVEABLE, bytes);
        if handle.is_null() {
            CloseClipboard();
            return false;
        }

        let target = GlobalLock(handle) as *mut u16;
        if target.is_null() {
            GlobalFree(handle);
            CloseClipboard();
            return false;
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), target, wide.len());

        EmptyClipboard();
        // 交给系统接管这块内存；失败时我们自己释放
        let result = SetClipboardData(CF_UNICODETEXT, handle as *mut _);
        let ok = !result.is_null();
        if !ok {
            GlobalFree(handle);
        }

        CloseClipboard();
        ok
    }
}
