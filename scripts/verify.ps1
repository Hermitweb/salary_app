# 一键跑完全部检查：格式、lint、测试、前端构建。
#
# 用途：
# - 提交前自检（等价于 CI 里跑的那些步骤）；
# - 换机器、改工具链之后确认环境是好的。
#
# 用法：
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\verify.ps1
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\verify.ps1 -SkipFrontend
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\verify.ps1 -Bench
#
# 注意：脚本必须带 UTF-8 BOM，Windows PowerShell 5.1 才能正确解码中文。

[CmdletBinding()]
param(
    [switch]$SkipFrontend,
    [switch]$SkipTests,
    [switch]$Bench
)

$ErrorActionPreference = "Stop"

$RootDir = Split-Path -Parent $PSScriptRoot
$results = New-Object System.Collections.Generic.List[object]

# ---------------------------------------------------------------------------
# MSVC 链接器修正
# ---------------------------------------------------------------------------

# 避免 Git Bash 自带的 /usr/bin/link.exe 抢在 MSVC link.exe 前面。
# 只要有一个构建脚本（build.rs）需要重新链接就会踩到：
# 链接器从 PATH 里找，而 Git Bash 的 PATH 前排是 GNU coreutils 的 link。
# scripts/build.ps1 里有同一套逻辑，这里复制一份，
# 保证「一键校验」从 Git Bash 启动也能跑通。
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

if ($env:OS -eq "Windows_NT") {
    $linker = Get-Command link.exe -ErrorAction SilentlyContinue
    if (-not ($linker -and $linker.Source -like "*Visual Studio*")) {
        $msvcBin = Get-MsvcBinPath
        if ($msvcBin) {
            $env:PATH = "$msvcBin;$env:PATH"
            Write-Host "==> 使用 MSVC 链接器: $msvcBin" -ForegroundColor Cyan
        } else {
            Write-Warning "未找到 MSVC 链接器；若链接失败，请在 Visual Studio 开发者命令行中运行。"
        }
    }
}

function Invoke-Step {
    param(
        [string]$Name,
        [string]$WorkDir,
        [scriptblock]$Action
    )

    Write-Host ""
    Write-Host "==> $Name" -ForegroundColor Cyan
    $sw = [System.Diagnostics.Stopwatch]::StartNew()

    Push-Location $WorkDir
    try {
        & $Action
        if ($LASTEXITCODE -ne 0) {
            throw "$Name 失败（退出码 $LASTEXITCODE）"
        }
        $sw.Stop()
        $results.Add([pscustomobject]@{ Step = $Name; Result = "通过"; Seconds = [math]::Round($sw.Elapsed.TotalSeconds, 1) })
    } catch {
        $sw.Stop()
        $results.Add([pscustomobject]@{ Step = $Name; Result = "失败"; Seconds = [math]::Round($sw.Elapsed.TotalSeconds, 1) })
        Write-Host ""
        Write-Host "--------------------------------------------------------------" -ForegroundColor Red
        Write-Host "$Name 失败：$_" -ForegroundColor Red
        Write-Host "--------------------------------------------------------------" -ForegroundColor Red
        break
    } finally {
        Pop-Location
    }
}

Write-Host "项目根目录：$RootDir"

Invoke-Step -Name "Rust 格式检查" -WorkDir $RootDir -Action {
    cargo fmt --all -- --check
}

Invoke-Step -Name "文档检查" -WorkDir $RootDir -Action {
    & (Join-Path $RootDir "scripts\check-docs.ps1")
}

Invoke-Step -Name "Rust Lint（整仓）" -WorkDir $RootDir -Action {
    cargo clippy --workspace --all-targets -- -D warnings
}

if (-not $SkipTests) {
    Invoke-Step -Name "Rust 测试（整仓）" -WorkDir $RootDir -Action {
        cargo test --workspace
    }
}

if ($Bench) {
    Invoke-Step -Name "性能基准" -WorkDir $RootDir -Action {
        cargo bench --workspace
    }
}

if (-not $SkipFrontend) {
    Invoke-Step -Name "前端类型检查" -WorkDir (Join-Path $RootDir "src-tauri") -Action {
        npm run typecheck
    }

    Invoke-Step -Name "前端构建" -WorkDir (Join-Path $RootDir "src-tauri") -Action {
        npm run build
    }
}

Write-Host ""
Write-Host "==================== 汇总 ====================" -ForegroundColor Cyan
$results | ForEach-Object {
    $color = if ($_.Result -eq "通过") { "Green" } else { "Red" }
    Write-Host ("{0,-18} {1,-6} {2,6}s" -f $_.Step, $_.Result, $_.Seconds) -ForegroundColor $color
}

$failed = @($results | Where-Object { $_.Result -ne "通过" })
if ($failed.Count -gt 0) {
    Write-Host ""
    Write-Host "有 $($failed.Count) 项未通过。" -ForegroundColor Red
    exit 1
}

Write-Host ""
Write-Host "全部检查通过。" -ForegroundColor Green
