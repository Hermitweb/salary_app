# 托盘图标自检：模拟左键点击托盘图标，看面板会不会被唤起
#
# 托盘图标的隐藏窗口类名是 tray_icon_app；
# 给它发一条 WM_USER_TRAYICON(6002) + WM_LBUTTONUP，
# 就等价于用户左键点了托盘图标。
#
# 真实点击是系统投递（Post）过来的，所以投递路径必须通；
# SendMessage 那一路只用来判断目标线程有没有在泵消息。
$ErrorActionPreference = 'Stop'

Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public class TT {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
}
'@

$panelTitle = '员工工资查询系统 - 控制面板'
$panel = [IntPtr]::Zero
$icon = [IntPtr]::Zero

$cb = [TT+EnumProc]{
  param($h, $l)
  $sb = New-Object System.Text.StringBuilder 256
  $cls = New-Object System.Text.StringBuilder 256
  [TT]::GetWindowText($h, $sb, 256) | Out-Null
  [TT]::GetClassName($h, $cls, 256) | Out-Null
  if ($sb.ToString() -eq $panelTitle) { $script:panel = $h }
  if ($cls.ToString() -eq 'tray_icon_app') {
    # 只认当前托盘进程的那一个，别被残留窗口骗了
    $procId = 0
    [TT]::GetWindowThreadProcessId($h, [ref]$procId) | Out-Null
    if ($procId -eq (Get-Process -Name 'salary-tray' -ErrorAction SilentlyContinue | Select-Object -First 1).Id) {
      $script:icon = $h
    }
  }
  return $true
}
[TT]::EnumWindows($cb, [IntPtr]::Zero) | Out-Null

if ($icon -eq [IntPtr]::Zero) { Write-Host 'NO TRAY ICON WINDOW'; exit 1 }
if ($panel -eq [IntPtr]::Zero) { Write-Host 'NO PANEL'; exit 1 }
Write-Host ('tray icon hwnd = ' + $icon + '  panel hwnd = ' + $panel)

function Test-TrayClick {
    param([string]$Label, [bool]$UseSend)

    # 先把面板收起来：托盘点击应该能把它重新唤起
    [TT]::ShowWindow($panel, 0) | Out-Null
    Start-Sleep -Milliseconds 400

    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    if ($UseSend) {
        [TT]::SendMessage($icon, 6002, [IntPtr]0, [IntPtr]0x0202) | Out-Null
    } else {
        [TT]::PostMessage($icon, 6002, [IntPtr]0, [IntPtr]0x0202) | Out-Null
    }
    $sw.Stop()

    Start-Sleep -Seconds 2
    $visible = [TT]::IsWindowVisible($panel)
    Write-Host ("{0,-6} round trip = {1,4}ms   panel shown = {2}" -f $Label, $sw.ElapsedMilliseconds, $visible)
    return $visible
}

$post = Test-TrayClick -Label 'POST' -UseSend $false
$send = Test-TrayClick -Label 'SEND' -UseSend $true

if ($post -and $send) { Write-Host 'TRAY CLICK OK' }
elseif ($send) { Write-Host 'TRAY CLICK: 投递路径不通，同步路径通' }
else { Write-Host 'TRAY CLICK FAILED' }
