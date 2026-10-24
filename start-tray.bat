@echo off
REM ============================================================
REM   Salary Query System - start the tray control panel
REM   No console window stays open: this launcher tells the
REM   PowerShell script (SALARY_TRAY_NOCONSOLE=1) to respawn
REM   itself in a hidden window, so prepare/build/run all
REM   happen invisibly and this console closes at once.
REM   Run scripts\start-tray.ps1 directly to see the output.
REM
REM   Keep this file ASCII-only: cmd.exe reads .bat files using the
REM   active OEM code page, and non-ASCII bytes can swallow line
REM   endings and break the batch. The Chinese messages live in
REM   scripts\start-tray.ps1, which is UTF-8 with BOM.
REM ============================================================
chcp 65001 >nul
setlocal
cd /d "%~dp0"

set SALARY_TRAY_NOCONSOLE=1
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\start-tray.ps1" %*

exit /b %errorlevel%
