@echo off
setlocal EnableExtensions EnableDelayedExpansion
chcp 65001 >nul
title Silentium
cd /d "%~dp0"

set "DEST=%LOCALAPPDATA%\Silentium"
set "EXE=%DEST%\silentium.exe"
set "URL=https://github.com/onx90s/silentium/releases/latest/download/silentium-windows-x64.zip"

echo.
echo ██████╗  ██╗ ██╗     ██████╗ ███╗   ██╗ ████████╗ ██╗ ██╗   ██╗ ███╗   ███╗
echo ██╔═══╝  ██║ ██║     ██╔═══╝ ████╗  ██║ ╚══██╔══╝ ██║ ██║   ██║ ████╗ ████║
echo ██████╗  ██║ ██║     █████╗  ██╔██╗ ██║    ██║    ██║ ██║   ██║ ██╔████╔██║
echo ╚═══██║  ██║ ██║     ██╔══╝  ██║╚██╗██║    ██║    ██║ ██║   ██║ ██║╚██╔╝██║
echo ██████║  ██║ ██████╗ ██████╗ ██║ ╚████║    ██║    ██║ ╚██████╔╝ ██║ ╚═╝ ██║
echo ╚═════╝  ╚═╝ ╚═════╝ ╚═════╝ ╚═╝  ╚═══╝    ╚═╝    ╚═╝  ╚═════╝  ╚═╝     ╚═╝
echo.
echo   kill switch de voz para Discord
echo.

if exist "%~dp0silentium.exe" set "EXE=%~dp0silentium.exe"
if exist "%EXE%" goto :ask

echo   [*] primeira execucao: baixando o binario (~4 MB)...
powershell -NoProfile -ExecutionPolicy Bypass -Command ^
  "$ErrorActionPreference='Stop'; $d='%DEST%'; New-Item -ItemType Directory -Force -Path $d | Out-Null; $z=Join-Path $d 'silentium.zip'; Invoke-WebRequest -UseBasicParsing -Uri '%URL%' -OutFile $z; Expand-Archive -Force -Path $z -DestinationPath $d; Remove-Item $z -Force; Write-Host ('   [ok] instalado em ' + $d)"
if not exist "%EXE%" (
    echo   [x] download falhou. Se tiver Rust instalado, compile com: cargo build --release
    echo.
    pause
    exit /b 1
)

:ask
if exist "%APPDATA%\Silentium\config.json" (
    echo   [*] token ja salvo em %APPDATA%\Silentium\config.json
    echo.
)
set "TOKEN="
set /p TOKEN=  seu token (Enter usa o salvo): 
if not defined TOKEN goto :run
set "DISCORD_TOKEN=!TOKEN!"
echo.

:run
"%EXE%" %*
echo.
pause
