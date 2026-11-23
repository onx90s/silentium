@echo off
setlocal EnableExtensions EnableDelayedExpansion
chcp 65001 >nul
title Silentium
cd /d "%~dp0"

set "EXE=%~dp0target\release\silentium.exe"
if not exist "%EXE%" set "EXE=%~dp0silentium.exe"
set "CFG=%APPDATA%\Silentium\config.json"

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

if not exist "%EXE%" (
    where cargo >nul 2>&1
    if errorlevel 1 (
        echo   [x] silentium.exe nao encontrado.
        echo       rode: cargo build --release
        echo.
        pause
        exit /b 1
    )
    echo   [*] compilando pela primeira vez, ~1 minuto...
    cargo build --release
    if errorlevel 1 (
        echo.
        pause
        exit /b 1
    )
)

if exist "%CFG%" (
    for /f "tokens=2 delims=:" %%A in ('findstr /c:"\"token\"" "%CFG%"') do set "SALVO=%%A"
    set "SALVO=!SALVO: =!"
    set "SALVO=!SALVO:\"=!"
    set "SALVO=!SALVO:,=!"
    if defined SALVO (
        echo   token salvo encontrado ^(...!SALVO:~-6!^)
        echo.
    )
)

set "TOKEN="
set /p TOKEN=  cole seu token (Enter usa o salvo): 
if not defined TOKEN goto :run

set "DISCORD_TOKEN=!TOKEN!"
echo.

:run
"%EXE%" %*
echo.
pause
