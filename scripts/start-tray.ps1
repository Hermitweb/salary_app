# 启动托盘控制面板 —— 全项目唯一的启动入口。
#
# 做完所有首次准备（数据目录、示例数据、前端构建、MSVC 链接器修正），
# 再编译并运行托盘。服务在面板里点「启动服务」拉起（跑在托盘进程内），
# 不自动开浏览器；面板的「打开网页」按钮随时可达网页端。
# 服务器上不跑面板的部署形态见 docs/RELEASE.md（直接运行 release exe）。
#
# 注意：本文件必须保存为「带 BOM 的 UTF-8」。
# Windows PowerShell 5.1 默认按系统 ANSI 代码页读取无 BOM 的脚本，
# 中文注释会导致解析器错位（报「意外的标记 }」）。
# 用记事本「另存为 UTF-8」或 VS Code 的 "Save with encoding: UTF-8 with BOM" 保存。

[CmdletBinding()]
param(
    [switch]$Release,
    # 内部参数：隐藏重拉出来的子进程带着它，避免无限重拉
    [switch]$Hidden
)

$ErrorActionPreference = "Stop"
$RootDir = Split-Path -Parent $PSScriptRoot
$TauriDir = Join-Path $RootDir "src-tauri"
$DataDir = Join-Path $RootDir "data"

# Windows 控制台默认是 GBK（代码页 936），Rust 进程输出的是 UTF-8 字节，
# 不切换的话日志里的中文会变成乱码。等价于 chcp 65001。
try {
    [Console]::OutputEncoding = [System.Text.Encoding]::UTF8
} catch {
    # 某些宿主环境不支持切换编码，忽略即可
}

# 双击 start-tray.bat 进来的（SALARY_TRAY_NOCONSOLE=1）：把整个
# 准备/构建/启动过程挪到隐藏窗口里跑，双击带出的命令行窗口随即消失。
# 开发者在终端里直接跑本脚本没有这个标记，输出照常可见。
if ($env:SALARY_TRAY_NOCONSOLE -eq "1" -and -not $Hidden) {
    $childArgs = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "`"$PSCommandPath`"")
    if ($Release) { $childArgs += '-Release' }
    $childArgs += '-Hidden'
    Start-Process powershell.exe -WindowStyle Hidden -ArgumentList $childArgs | Out-Null
    exit 0
}

# 隐藏窗口里出错没人看得见：弹窗告知，别让启动失败无声无息。
# 终端里运行时保持原来的行为（红字输出，退出码 1）。
trap {
    if ($Hidden) {
        try {
            Add-Type -AssemblyName System.Windows.Forms | Out-Null
            [System.Windows.Forms.MessageBox]::Show(
                "启动失败：`n$($_.Exception.Message)",
                "员工工资查询系统",
                [System.Windows.Forms.MessageBoxButtons]::OK,
                [System.Windows.Forms.MessageBoxIcon]::Error) | Out-Null
        } catch {
            # 连弹窗都出不来就只能静默了
        }
    }
    exit 1
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

# 数据目录：首次启动生成示例数据，正式使用前替换成真实数据
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

# 前端：面板的「打开网页」按钮与网页端都吃 src-tauri/dist
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
        Write-Warning "缺少前端构建产物且没有 Node.js，面板的「打开网页」将不可用。"
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

# 托盘控制面板
Write-Step "编译托盘控制面板"
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

Write-Step "启动托盘控制面板（服务在面板里点「启动服务」拉起）"
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
