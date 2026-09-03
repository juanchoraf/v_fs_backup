@echo off
setlocal

set DOTNET_CLI_TELEMETRY_OPTOUT=1
set DOTNET_NOLOGO=true
set POWERSHELL_TELEMETRY_OPTOUT=1
set POWERSHELL_UPDATECHECK=Off
set POWERSHELL_DIAGNOSTICS_OPTOUT=1
set VSCMD_SKIP_SENDTELEMETRY=1
set RUSTUP_NO_UPDATE_CHECK=1
set DOTNET_CLI_WORKLOAD_UPDATE_NOTIFY_DISABLE=1
set DOTNET_SKIP_FIRST_TIME_EXPERIENCE=1

where pwsh.exe >nul 2>nul
if %ERRORLEVEL%==0 (
    pwsh.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0build_binaries_windows.ps1" %*
    exit /b %ERRORLEVEL%
)

powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0build_binaries_windows.ps1" %*
exit /b %ERRORLEVEL%
