# 构建脚本（PowerShell 5.1+ / Windows）。
#
# 自动把 MSVC 的链接器目录插到 PATH 最前面，
# 避免 Git Bash 自带的 /usr/bin/link.exe 抢占 MSVC link.exe。
#
# 用法：
#   scripts/build.ps1                # 构建 debug 版后端
#   scripts/build.ps1 -Release       # 构建 release 版后端
#   scripts/build.ps1 -Test          # 运行测试
#   scripts/build.ps1 -Frontend      # 只构建前端
#   scripts/build.ps1 -Tray          # 只构建托盘控制面板
#   scripts/build.ps1 -All           # 前端 + 后端 + 托盘
#
# 注意：本文件必须保存为「带 BOM 的 UTF-8」。
# Windows PowerShell 5.1 默认按系统 ANSI 代码页读取无 BOM 的脚本，
# 中文注释会导致解析器错位（报「意外的标记 }」）。

[CmdletBinding()]
param(
    [switch]$Release,
    [switch]$Test,
    [switch]$Frontend,
    [switch]$Tray,
    [switch]$All
)

$ErrorActionPreference = "Stop"
$RootDir = Split-Path -Parent $PSScriptRoot
$TauriDir = Join-Path $RootDir "src-tauri"

# Windows 控制台默认是 GBK（代码页 936），Rust 进程输出的是 UTF-8 字节，
# 不切换的话构建日志里的中文会变成乱码。等价于 chcp 65001。
if ($env:OS -eq "Windows_NT") {
    try {
        [Console]::OutputEncoding = [System.Text.Encoding]::UTF8
    } catch {
        # 某些宿主环境不支持切换编码，忽略即可
    }
}

# ---------------------------------------------------------------------------
# MSVC 链接器检测
# ---------------------------------------------------------------------------

function Test-IsWindows {
    # $IsWindows 是 PowerShell 7+ 的内置变量，5.1 下要用 $env:OS 判断
    return $env:OS -eq "Windows_NT"
}

function Get-MsvcBinPath {
    <#
      在常见的 Visual Studio 安装位置里查找 MSVC 链接器目录。
    #>
    $candidates = @()

    $vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
    if (Test-Path $vswhere) {
        $installPath = & $vswhere -latest -products * `
            -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
            -property installationPath 2>$null
        if ($installPath) {
            $candidates += Join-Path $installPath "VC\Tools\MSVC"
        }
    }

    $roots = @(
        "${env:ProgramFiles}\Microsoft Visual Studio\2022",
        "${env:ProgramFiles(x86)}\Microsoft Visual Studio\2022",
        "D:\Microsoft Visual Studio\2022",
        "E:\Microsoft Visual Studio\2022",
        "F:\Microsoft Visual Studio\2022"
    )

    foreach ($root in $roots) {
        if (Test-Path $root) {
            $candidates += (Get-ChildItem -Path $root -Directory -ErrorAction SilentlyContinue |
                ForEach-Object { Join-Path $_.FullName "VC\Tools\MSVC" })
        }
    }

    foreach ($candidate in $candidates) {
        if (-not (Test-Path $candidate)) { continue }
        $link = Get-ChildItem -Path $candidate -Directory -ErrorAction SilentlyContinue |
            Sort-Object Name -Descending |
            ForEach-Object { Join-Path $_.FullName "bin\HostX64\x64" } |
            Where-Object { Test-Path (Join-Path $_ "link.exe") } |
            Select-Object -First 1
        if ($link) { return $link }
    }

    return $null
}

function Initialize-MsvcLinker {
    <#
      把 MSVC 链接器目录放到 PATH 最前面。
    #>
    if (-not (Test-IsWindows)) { return }

    $linker = Get-Command link.exe -ErrorAction SilentlyContinue
    if ($linker -and $linker.Source -like "*Visual Studio*") { return }

    $msvcBin = Get-MsvcBinPath
    if ($msvcBin) {
        $env:PATH = "$msvcBin;$env:PATH"
        Write-Host "==> 使用 MSVC 链接器: $msvcBin" -ForegroundColor Cyan
    } else {
        Write-Warning "未找到 MSVC 链接器；若链接失败，请在 Visual Studio 开发者命令行中运行。"
    }
}

# ---------------------------------------------------------------------------
# 任务
# ---------------------------------------------------------------------------

function Build-Frontend {
    Write-Host "==> 构建前端" -ForegroundColor Cyan
    Push-Location $TauriDir
    try {
        if (-not (Test-Path "node_modules")) {
            npm install --no-audit --no-fund
            if ($LASTEXITCODE -ne 0) { throw "npm install 失败" }
        }
        npm run build
        if ($LASTEXITCODE -ne 0) { throw "前端构建失败" }
    } finally {
        Pop-Location
    }
}

function Build-Backend {
    Write-Host "==> 构建后端" -ForegroundColor Cyan
    Push-Location $RootDir
    try {
        if ($Release) {
            cargo build --release --bin salary-app-backend
        } else {
            cargo build --bin salary-app-backend
        }
        if ($LASTEXITCODE -ne 0) { throw "后端构建失败" }
    } finally {
        Pop-Location
    }
}

function Build-Tray {
    Write-Host "==> 构建托盘控制面板" -ForegroundColor Cyan
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
}

function Invoke-Tests {
    Write-Host "==> 运行测试" -ForegroundColor Cyan
    Push-Location $RootDir
    try {
        cargo test
        if ($LASTEXITCODE -ne 0) { throw "测试未通过" }
    } finally {
        Pop-Location
    }
}

Initialize-MsvcLinker

if ($Frontend) {
    Build-Frontend
}
elseif ($Test) {
    Invoke-Tests
}
elseif ($Tray) {
    Build-Tray
}
elseif ($All) {
    Build-Frontend
    Build-Backend
    Build-Tray
}
else {
    Build-Backend
}

Write-Host "==> 完成" -ForegroundColor Green
