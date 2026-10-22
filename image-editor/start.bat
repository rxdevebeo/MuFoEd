@echo off
setlocal
title Image Studio

where pwsh.exe >nul 2>&1
if errorlevel 1 (
    powershell.exe -NoLogo -NoProfile -File "%~dp0start.ps1" %*
) else (
    pwsh.exe -NoLogo -NoProfile -File "%~dp0start.ps1" %*
)

set "launch_exit=%errorlevel%"
if not "%launch_exit%"=="0" (
    echo.
    echo Image Studio could not start. See the error above.
    pause
)
exit /b %launch_exit%
