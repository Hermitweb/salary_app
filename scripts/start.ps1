# 一键启动：准备数据 -> 构建前端 -> 构建后端 -> 启动服务。
#
# 用法：
#   scripts/start.ps1              # 开发模式（cargo run）
#   scripts/start.ps1 -Release     # 发布模式
#   scripts/start.ps1 -NoBrowser   # 不自动打开浏览器
#
# 注意：本文件必须保存为「带 BOM 的 UTF-8」。
# Windows PowerShell 5.1 默认按系统 ANSI 代码页读取无 BOM 的脚本，
# 中文注释会导致解析器错位（报「意外的标记 }」）。
# 用记事本「另存为 UTF-8」或 VS Code 的 "Save with encoding: UTF-8 with BOM" 保存。

[CmdletBinding()]
param(
    [switch]$Release,
    [switch]$NoBrowser
)

$ErrorActionPreference = "Stop"
$RootDir = Split-Path -Parent $PSScriptRoot
$TauriDir = Join-Path $RootDir "src-tauri"
$DataDir = Join-Path $RootDir "data"

# Windows 控制台默认是 GBK（代码页 936），Rust 进程输出的是 UTF-8 字节，
# 不切换的话日志里的中文会变成乱码。等价于 chcp 65001。
if ($env:OS -eq "Windows_NT") {
    try {
        [Console]::OutputEncoding = [System.Text.Encoding]::UTF8
    } catch {
        # 某些宿主环境不支持切换编码，忽略即可
    }
}

# ---------------------------------------------------------------------------
# 工具
# ---------------------------------------------------------------------------

function Write-Step($message) {
    Write-Host "==> $message" -ForegroundColor Cyan
}

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
      Git Bash 的 /usr/bin/link.exe 会抢占 MSVC 的 link.exe 导致链接失败。
    #>
    if (-not (Test-IsWindows)) { return }

    $linker = Get-Command link.exe -ErrorAction SilentlyContinue
    if ($linker -and $linker.Source -like "*Visual Studio*") { return }

    $msvcBin = Get-MsvcBinPath
    if ($msvcBin) {
        $env:PATH = "$msvcBin;$env:PATH"
        Write-Host "==> 使用 MSVC 链接器: $msvcBin" -ForegroundColor DarkGray
    }
}

function Test-CommandExists($name) {
    return [bool](Get-Command $name -ErrorAction SilentlyContinue)
}

# ---------------------------------------------------------------------------
# 步骤
# ---------------------------------------------------------------------------

Write-Step "检查依赖"

if (-not (Test-CommandExists "cargo")) {
    throw "未找到 cargo，请先安装 Rust 工具链：https://rustup.rs/"
}

$hasNode = Test-CommandExists "node"
$hasNpm = Test-CommandExists "npm"
if (-not ($hasNode -and $hasNpm)) {
    Write-Warning "未找到 Node.js，前端无法构建。若已有构建产物 src-tauri/dist，将直接使用。"
}

Initialize-MsvcLinker

# 数据目录
Write-Step "检查数据目录"
if (-not (Test-Path $DataDir)) {
    New-Item -ItemType Directory -Path $DataDir | Out-Null
}

$employeeFile = Join-Path $DataDir "员工列表.xlsx"
if (-not (Test-Path $employeeFile)) {
    $hasExcel = Get-ChildItem -Path $DataDir -Filter "*.xls*" -ErrorAction SilentlyContinue
    if (-not $hasExcel) {
        Write-Step "数据目录为空，生成示例数据"
        if (Test-CommandExists "python") {
            Push-Location $RootDir
            try {
                python (Join-Path $PSScriptRoot "generate_sample_data.py") --data-dir $DataDir
            } finally {
                Pop-Location
            }
        } else {
            Write-Warning "未找到 Python，无法生成示例数据。请手动把 员工列表.xlsx 放到 $DataDir"
        }
    } else {
        Write-Warning "未找到 员工列表.xlsx，请确认数据文件是否正确。"
    }
}

# 前端
$distDir = Join-Path $TauriDir "dist"
$needsFrontend = -not (Test-Path (Join-Path $distDir "index.html"))
if (-not $needsFrontend -and $hasNode) {
    $newestSource = Get-ChildItem -Path (Join-Path $TauriDir "src") -Recurse -File |
        Sort-Object LastWriteTime -Descending | Select-Object -First 1
    $oldestDist = Get-ChildItem -Path $distDir -Recurse -File |
        Sort-Object LastWriteTime | Select-Object -First 1
    if ($newestSource -and $oldestDist -and $newestSource.LastWriteTime -gt $oldestDist.LastWriteTime) {
        $needsFrontend = $true
    }
}

if ($needsFrontend) {
    if (-not ($hasNode -and $hasNpm)) {
        Write-Warning "缺少前端构建产物且没有 Node.js，服务将只提供 API。"
    } else {
        Write-Step "构建前端"
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
}

# 后端
Write-Step "构建后端"
Push-Location $RootDir
try {
    if ($Release) {
        cargo build --release --bin salary-app-backend
    } else {
        cargo build --bin salary-app-backend
    }
    if ($LASTEXITCODE -ne 0) { throw "后端构建失败" }

    Write-Step "启动服务"
    if ($NoBrowser) {
        $env:SALARY_NO_BROWSER = "1"
    }

    if ($Release) {
        cargo run --release --bin salary-app-backend
    } else {
        cargo run --bin salary-app-backend
    }
} finally {
    Pop-Location
}
