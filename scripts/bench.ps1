# 性能基准：一键跑完 CPU 侧基准 + 真实网络延迟。
#
# 用法：
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\bench.ps1
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\bench.ps1 -Port 5001 -User 001 -Password salary2026
#
# 两部分：
#   1. cargo bench：各条热路径的耗时（口令哈希、Excel 解析、接口处理）；
#   2. HTTP 延迟：对真实跑着的服务发请求，给出 p50 / p95 / p99。
#
# 注意：脚本必须带 UTF-8 BOM，Windows PowerShell 5.1 才能正确解码中文。

[CmdletBinding()]
param(
    [int]$Port = 5001,
    [string]$User = "001",
    [string]$Password = "salary2026",
    [int]$Requests = 200,
    [switch]$SkipBench
)

$ErrorActionPreference = "Stop"
$RootDir = Split-Path -Parent $PSScriptRoot

function Write-Table {
    param(
        [string]$Title,
        [object[]]$Rows
    )
    Write-Host ""
    Write-Host "== $Title" -ForegroundColor Cyan
    $Rows | ForEach-Object {
        Write-Host ("  {0,-26} {1,10} {2,10} {3,10}" -f $_.Name, $_.P50, $_.P95, $_.P99)
    }
}

# ---------------------------------------------------------------- CPU 基准
if (-not $SkipBench) {
    Write-Host "==> CPU 热路径基准（cargo bench）" -ForegroundColor Cyan
    Push-Location $RootDir
    try {
        cargo bench --workspace
        if ($LASTEXITCODE -ne 0) { throw "cargo bench 失败（退出码 $LASTEXITCODE）" }
    } finally {
        Pop-Location
    }
}

# ---------------------------------------------------------------- 网络延迟
Write-Host ""
Write-Host "==> HTTP 真实网络延迟" -ForegroundColor Cyan

$base = "http://127.0.0.1:$Port"

# HttpClient 要先加载程序集（Windows PowerShell 5.1 不会自动加载）
Add-Type -AssemblyName System.Net.Http
$client = New-Object System.Net.Http.HttpClient
$client.Timeout = [TimeSpan]::FromSeconds(30)

try {
    # 登录
    $loginBody = @{ username = $User; password = $Password } | ConvertTo-Json -Compress
    $content = New-Object System.Net.Http.StringContent($loginBody, [System.Text.Encoding]::UTF8, "application/json")
    $response = $client.PostAsync("$base/api/auth/login", $content).GetAwaiter().GetResult()
    if (-not $response.IsSuccessStatusCode) {
        throw "登录失败：HTTP $([int]$response.StatusCode)"
    }
    $token = ($response.Content.ReadAsStringAsync().GetAwaiter().GetResult() | ConvertFrom-Json).token
    if (-not $token) { throw "登录没有返回令牌" }
    $client.DefaultRequestHeaders.Authorization = New-Object System.Net.Http.Headers.AuthenticationHeaderValue("Bearer", $token)

    # 可查询月份
    $months = ($client.GetStringAsync("$base/api/available_months").GetAwaiter().GetResult() | ConvertFrom-Json).months
    if (-not $months -or $months.Count -eq 0) { throw "没有可查询的月份" }
    $latest = $months[0]
    $salaryUri = "$base/api/salary/$($latest.year)/$($latest.month)"

    function Measure-Uri {
        param([string]$Uri, [int]$Count)
        $samples = New-Object System.Collections.Generic.List[double]
        for ($i = 0; $i -lt $Count; $i++) {
            $sw = [System.Diagnostics.Stopwatch]::StartNew()
            $null = $client.GetStringAsync($Uri).GetAwaiter().GetResult()
            $sw.Stop()
            $samples.Add($sw.Elapsed.TotalMilliseconds)
        }
        $sorted = $samples | Sort-Object
        [pscustomobject]@{
            P50 = [math]::Round($sorted[[Math]::Min($sorted.Count - 1, [int]($sorted.Count * 0.50))], 2)
            P95 = [math]::Round($sorted[[Math]::Min($sorted.Count - 1, [int]($sorted.Count * 0.95))], 2)
            P99 = [math]::Round($sorted[[Math]::Min($sorted.Count - 1, [int]($sorted.Count * 0.99))], 2)
        }
    }

    $loginSamples = New-Object System.Collections.Generic.List[double]
    for ($i = 0; $i -lt [Math]::Min($Requests, 50); $i++) {
        $body = @{ username = $User; password = $Password } | ConvertTo-Json -Compress
        $content = New-Object System.Net.Http.StringContent($body, [System.Text.Encoding]::UTF8, "application/json")
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        $null = $client.PostAsync("$base/api/auth/login", $content).GetAwaiter().GetResult()
        $sw.Stop()
        $loginSamples.Add($sw.Elapsed.TotalMilliseconds)
    }
    $sorted = $loginSamples | Sort-Object
    $loginRow = [pscustomobject]@{
        Name = "登录"
        P50 = [math]::Round($sorted[[Math]::Min($sorted.Count - 1, [int]($sorted.Count * 0.50))], 2)
        P95 = [math]::Round($sorted[[Math]::Min($sorted.Count - 1, [int]($sorted.Count * 0.95))], 2)
        P99 = [math]::Round($sorted[[Math]::Min($sorted.Count - 1, [int]($sorted.Count * 0.99))], 2)
    }

    $salaryRow = Measure-Uri -Uri $salaryUri -Count $Requests
    $salaryRow | Add-Member -NotePropertyName Name -NotePropertyValue "查工资" -Force

    $monthsRow = Measure-Uri -Uri "$base/api/available_months" -Count $Requests
    $monthsRow | Add-Member -NotePropertyName Name -NotePropertyValue "可查询月份" -Force

    Write-Table -Title "端到端延迟（毫秒）" -Rows @($loginRow, $salaryRow, $monthsRow)
} finally {
    $client.Dispose()
}

Write-Host ""
Write-Host "说明：登录的耗时几乎全是 PBKDF2（21 万轮），是安全强度换来的，不要为了快而调低。" -ForegroundColor DarkGray
