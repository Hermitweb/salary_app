@echo off
REM ============================================================
REM   Salary Query System - one-click start
REM   Prepares data, builds frontend + backend, then runs the server.
REM
REM   Keep this file ASCII-only: cmd.exe reads .bat files using the
REM   active OEM code page, and non-ASCII bytes can swallow line
REM   endings and break the batch. The Chinese messages live in
REM   scripts\start.ps1, which is UTF-8 with BOM.
REM ============================================================
chcp 65001 >nul
setlocal
cd /d "%~dp0"

powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\start.ps1" %*

if %errorlevel% neq 0 (
    echo.
    echo Startup failed. Please check the error message above.
    pause
)

exit /b %errorlevel%
