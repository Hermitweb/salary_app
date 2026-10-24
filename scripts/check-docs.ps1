# 检查文档：相对链接是否都指向真实存在的文件、是否留了占位符。
#
# 文档最常见的坏法是「链接指向的文件被改名/删掉了」，
# 静态检查比人工点一遍可靠。
#
# 用法：
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\check-docs.ps1
#
# 注意：脚本必须带 UTF-8 BOM，Windows PowerShell 5.1 才能正确解码中文。

[CmdletBinding()]
param(
    [string]$Root = ""
)

$ErrorActionPreference = "Stop"

if ($Root -eq "") {
    $Root = Split-Path -Parent $PSScriptRoot
}

$problems = New-Object System.Collections.Generic.List[string]

# ---- 收集要检查的 Markdown 文件 ----
$files = New-Object System.Collections.Generic.List[string]
$files.Add((Join-Path $Root "README.md"))
$files.Add((Join-Path $Root "CONTRIBUTING.md"))
$files.Add((Join-Path $Root "CHANGELOG.md"))
$docsDir = Join-Path $Root "docs"
if (Test-Path $docsDir) {
    Get-ChildItem $docsDir -Filter *.md -File | ForEach-Object { $files.Add($_.FullName) }
}
$design = Join-Path $Root "设计风格规范.md"
if (Test-Path $design) { $files.Add($design) }

Write-Host "检查 $($files.Count) 个 Markdown 文件……" -ForegroundColor Cyan

$linkPattern = '\[[^\]]*\]\(([^)\s]+)(?:\s+"[^"]*")?\)'
$placeholderPattern = 'TODO|FIXME|待补充|待完善|XXX（请|此处填写'

foreach ($file in $files) {
    if (-not (Test-Path $file)) { continue }
    $name = Split-Path -Leaf $file
    $lines = [System.IO.File]::ReadAllLines($file, [System.Text.Encoding]::UTF8)

    for ($i = 0; $i -lt $lines.Count; $i++) {
        $line = $lines[$i]

        # 围栏里的代码块不查链接
        if ($line -match '^\s*```') { continue }

        foreach ($match in [regex]::Matches($line, $linkPattern)) {
            $target = $match.Groups[1].Value
            if ($target -match '^(https?:|mailto:|#)') { continue }

            # 去掉锚点
            $path = $target.Split('#')[0]
            if ($path -eq "") { continue }

            $resolved = Join-Path (Split-Path -Parent $file) $path
            if (-not (Test-Path $resolved)) {
                $problems.Add("$name 第 $($i + 1) 行：链接指向不存在的文件 -> $target")
            }
        }

        if ($line -match $placeholderPattern) {
            $problems.Add("$name 第 $($i + 1) 行：留了占位内容 -> $($line.Trim())")
        }
    }
}

# ---- 代码围栏是否成对 ----
foreach ($file in $files) {
    if (-not (Test-Path $file)) { continue }
    $name = Split-Path -Leaf $file
    $lines = [System.IO.File]::ReadAllLines($file, [System.Text.Encoding]::UTF8)
    $fences = 0
    foreach ($line in $lines) {
        if ($line -match '^\s*```') { $fences++ }
    }
    if ($fences % 2 -ne 0) {
        # 消息里不能出现反引号：PowerShell 把它当转义符
        $problems.Add($name + '：代码围栏数量为奇数（' + $fences + '），有没闭合的代码块')
    }
}

if ($problems.Count -gt 0) {
    Write-Host ""
    Write-Host "发现 $($problems.Count) 个问题：" -ForegroundColor Red
    $problems | ForEach-Object { Write-Host "  - $_" -ForegroundColor Red }
    exit 1
}

Write-Host "文档检查通过：链接有效、无占位符、代码围栏成对。" -ForegroundColor Green
