# 用 PrintWindow 把窗口内容画到位图（不受其他窗口遮挡影响）
$ErrorActionPreference = 'Stop'

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class PW {
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, int flags);
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
}
'@

Add-Type -AssemblyName System.Drawing

$p = Get-Process -Name salary-tray -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $p) { Write-Host 'NO PROCESS'; exit 1 }

$h = $p.MainWindowHandle
$r = New-Object PW+RECT
[PW]::GetClientRect($h, [ref]$r) | Out-Null

$bmp = New-Object System.Drawing.Bitmap $r.R, $r.B
$g = [System.Drawing.Graphics]::FromImage($bmp)
$hdc = $g.GetHdc()
# flag 2 = PW_RENDERFULLCONTENT
$ok = [PW]::PrintWindow($h, $hdc, 2)
$g.ReleaseHdc($hdc)
$g.Dispose()
$bmp.Save((Join-Path $PSScriptRoot 'tray-panel.png'), [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
Write-Host ('PrintWindow ok=' + $ok + ' size=' + $r.R + 'x' + $r.B)
