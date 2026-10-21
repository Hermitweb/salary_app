@echo off
REM ============================================================
REM   员工工资查询系统 - 启动托盘控制面板
REM   托盘图标右键可以显示面板、启停服务、退出程序
REM ============================================================
chcp 65001 >nul
setlocal
cd /d "%~dp0"

powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\start-tray.ps1" %*

if %errorlevel% neq 0 (
    echo.
    echo 启动失败，请查看上方错误信息。
    pause
)

exit /b %errorlevel%
