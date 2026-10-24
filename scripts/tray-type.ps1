# 托盘面板自动化：向当前正在编辑的输入框里敲字
$ErrorActionPreference = 'Stop'

Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public class TT {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr p, EnumProc cb, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
}
'@

$panel = [IntPtr]::Zero
$title = '员工工资查询系统 - 控制面板'
$cb = [TT+EnumProc]{
  param($h, $l)
  $sb = New-Object System.Text.StringBuilder 256
  [TT]::GetWindowText($h, $sb, 256) | Out-Null
  if ($sb.ToString() -eq $title) { $script:panel = $h; return $false }
  return $true
}
[TT]::EnumWindows($cb, [IntPtr]::Zero) | Out-Null
if ($panel -eq [IntPtr]::Zero) { Write-Host 'NO PANEL'; exit 1 }

$canvas = [IntPtr]::Zero
$ccb = [TT+EnumProc]{
  param($h, $l)
  $sb = New-Object System.Text.StringBuilder 256
  [TT]::GetClassName($h, $sb, 256) | Out-Null
  if ($sb.ToString() -eq 'SalaryPanelCanvas') { $script:canvas = $h; return $false }
  return $true
}
[TT]::EnumChildWindows($panel, $ccb, [IntPtr]::Zero) | Out-Null
if ($canvas -eq [IntPtr]::Zero) { Write-Host 'NO CANVAS'; exit 1 }

$text = $args[0]

# 可选：先退格若干次，用来清掉输入框里已有的内容
if ($args.Count -ge 2) {
  for ($i = 0; $i -lt [int]$args[1]; $i++) {
    [TT]::SendMessage($canvas, 0x0100, [IntPtr]0x08, [IntPtr]0) | Out-Null  # WM_KEYDOWN VK_BACK
    Start-Sleep -Milliseconds 20
  }
}

foreach ($ch in $text.ToCharArray()) {
  # 用 SendMessage：跨进程投递的 WM_CHAR 不一定会被消息循环取到
  [TT]::SendMessage($canvas, 0x0102, [IntPtr][int][char]$ch, [IntPtr]0) | Out-Null  # WM_CHAR
  Start-Sleep -Milliseconds 20
}
Write-Host ('typed ' + $text.Length + ' chars')
