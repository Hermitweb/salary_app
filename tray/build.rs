//! 构建脚本：把清单文件内嵌进可执行文件。
//!
//! 清单里声明了 comctl32 v6 —— native-windows-gui 依赖的
//! `GetWindowSubclass` / `SetWindowSubclass` 只有 v6 版本才导出，
//! 缺少清单时程序会报「无法定位程序入口点」。

fn main() {
    println!("cargo:rerun-if-changed=salary-tray.rc");
    println!("cargo:rerun-if-changed=salary-tray.manifest");
    println!("cargo:rerun-if-changed=resources/app_icon.ico");

    embed_resource::compile("salary-tray.rc", embed_resource::NONE)
        .manifest_required()
        .expect("无法内嵌应用清单");
}
