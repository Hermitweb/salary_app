//! 构建脚本：把应用图标内嵌进后端可执行文件。
//!
//! 后端进程常驻后台，任务管理器里显示的就是这个进程；
//! 不内嵌图标的话，进程列表里就是一个默认的白色图标。
//! 图标与托盘是同一份资源文件（`tray/resources/app_icon.ico`），
//! 保证「一个应用一个图标」。

fn main() {
    // 资源编译依赖 Windows 的 rc.exe / windres，其它平台直接跳过
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=salary-app.rc");
        println!("cargo:rerun-if-changed=../tray/resources/app_icon.ico");
        embed_resource::compile("salary-app.rc", embed_resource::NONE)
            .manifest_required()
            .expect("无法内嵌应用图标");
    }
}
