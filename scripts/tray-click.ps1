# 托盘面板自动化：点击侧边栏的某一项并截图
$ErrorActionPreference = 'Stop'

Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public class TW {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr p, EnumProc cb, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
}
'@

$panel = [IntPtr]::Zero
$title = '员工工资查询系统 - 控制面板'
$cb = [TW+EnumProc]{
  param($h, $l)
  $sb = New-Object System.Text.StringBuilder 256
  [TW]::GetWindowText($h, $sb, 256) | Out-Null
  if ($sb.ToString() -eq $title) { $script:panel = $h; return $false }
  return $true
}
[TW]::EnumWindows($cb, [IntPtr]::Zero) | Out-Null
if ($panel -eq [IntPtr]::Zero) { Write-Host 'NO PANEL'; exit 1 }

$canvas = [IntPtr]::Zero
$ccb = [TW+EnumProc]{
  param($h, $l)
  $sb = New-Object System.Text.StringBuilder 256
  [TW]::GetClassName($h, $sb, 256) | Out-Null
  if ($sb.ToString() -eq 'SalaryPanelCanvas') { $script:canvas = $h; return $false }
  return $true
}
[TW]::EnumChildWindows($panel, $ccb, [IntPtr]::Zero) | Out-Null
if ($canvas -eq [IntPtr]::Zero) { Write-Host 'NO CANVAS'; exit 1 }

Write-Host ('panel=' + $panel + ' canvas=' + $canvas)

$x = [int]$args[0]
$y = [int]$args[1]
$lParam = [IntPtr](($y -shl 16) -bor ($x -band 0xFFFF))

[TW]::PostMessage($canvas, 0x0201, [IntPtr]1, $lParam) | Out-Null   # WM_LBUTTONDOWN
Start-Sleep -Milliseconds 120
[TW]::PostMessage($canvas, 0x0202, [IntPtr]0, $lParam) | Out-Null   # WM_LBUTTONUP
Write-Host ('clicked ' + $x + ',' + $y)
