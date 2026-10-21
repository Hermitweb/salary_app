# 启动托盘控制面板。
#
# 会先编译托盘程序（如果还没编译），再运行它。
# 托盘程序本身不含服务器进程：服务由面板里的「启动服务」按钮按需拉起。

[CmdletBinding()]
param(
    [switch]$Release
)

$ErrorActionPreference = "Stop"
$RootDir = Split-Path -Parent $PSScriptRoot

# Windows 控制台默认是 GBK（代码页 936），Rust 进程输出的是 UTF-8 字节，
# 不切换的话日志里的中文会变成乱码。等价于 chcp 65001。
try {
    [Console]::OutputEncoding = [System.Text.Encoding]::UTF8
} catch {
    # 某些宿主环境不支持切换编码，忽略即可
}

Write-Host "==> 编译托盘控制面板" -ForegroundColor Cyan
Push-Location $RootDir
try {
    if ($Release) {
        cargo build --release -p salary-tray
    } else {
        cargo build -p salary-tray
    }
    if ($LASTEXITCODE -ne 0) { throw "托盘控制面板构建失败" }
} finally {
    Pop-Location
}

Write-Host "==> 启动托盘控制面板" -ForegroundColor Cyan
Push-Location $RootDir
try {
    if ($Release) {
        cargo run --release -p salary-tray
    } else {
        cargo run -p salary-tray
    }
} finally {
    Pop-Location
}
