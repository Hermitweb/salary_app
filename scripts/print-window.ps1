# 用 PrintWindow 把窗口内容画到位图（不受其他窗口遮挡影响）
$ErrorActionPreference = 'Stop'

Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public class PW {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, int flags);
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
'@

Add-Type -AssemblyName System.Drawing

# 按标题找窗口：托盘程序是图形子系统进程，MainWindowHandle 常常拿不到
$found = [IntPtr]::Zero
$title = '员工工资查询系统 - 控制面板'
$cb = [PW+EnumProc]{
  param($h, $l)
  $sb = New-Object System.Text.StringBuilder 256
  [PW]::GetWindowText($h, $sb, 256) | Out-Null
  if ($sb.ToString() -eq $title) {
    $script:found = $h
    return $false
  }
  return $true
}
[PW]::EnumWindows($cb, [IntPtr]::Zero) | Out-Null
$h = $found
if ($h -eq [IntPtr]::Zero) { Write-Host 'NO WINDOW'; exit 1 }
# 用整窗尺寸：PW_RENDERFULLCONTENT 会把标题栏一起画进来，
# 位图若按客户区尺寸建，内容会被边框顶偏。
$r = New-Object PW+RECT
[PW]::GetWindowRect($h, [ref]$r) | Out-Null
$w = $r.R - $r.L
$hgt = $r.B - $r.T
Write-Host ('window rect = ' + $r.L + ',' + $r.T + ' - ' + $r.R + ',' + $r.B + ' hwnd=' + $h)

$bmp = New-Object System.Drawing.Bitmap $w, $hgt
$g = [System.Drawing.Graphics]::FromImage($bmp)
$hdc = $g.GetHdc()
# flag 2 = PW_RENDERFULLCONTENT
$ok = [PW]::PrintWindow($h, $hdc, 2)
$g.ReleaseHdc($hdc)
$g.Dispose()
$bmp.Save((Join-Path $PSScriptRoot 'tray-panel.png'), [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
Write-Host ('PrintWindow ok=' + $ok + ' size=' + $w + 'x' + $hgt)
