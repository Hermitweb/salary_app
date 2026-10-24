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
