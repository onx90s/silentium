@echo off
setlocal EnableExtensions EnableDelayedExpansion
chcp 65001 >nul
title Silentium
cd /d "%~dp0"

set "EXE=%~dp0target\release\silentium.exe"
if not exist "%EXE%" set "EXE=%~dp0silentium.exe"
set "TOKDIR=%LOCALAPPDATA%\Silentium"
set "TOKFILE=%TOKDIR%\token.txt"
echo.
echo ██████╗  ██╗ ██╗      ██████╗ ██╗  ██╗  ████████╗ ██╗ ██╗   ██╗ ███╗   ███╗
echo ██╔═══╝  ██║ ██║      ██╔═══╝ ███╗ ██║  ╚══██╔══╝ ██║ ██║   ██║ ████╗ ████║
echo ██████╗  ██║ ██║      █████╗  ██╔██╗██║    ██║    ██║ ██║   ██║ ██╔████╔██║
echo ╚═══██║  ██║ ██║      ██╔══╝  ██║╚████║    ██║    ██║ ██║   ██║ ██║╚██╔╝██║
echo ██████║  ██║ ██████╗  ██████╗ ██║ ╚███║    ██║    ██║ ╚██████╔╝ ██║ ╚═╝ ██║
echo ╚═════╝  ╚═╝ ╚═════╝  ╚═════╝ ╚═╝  ╚══╝    ╚═╝    ╚═╝  ╚═════╝  ╚═╝     ╚═╝
echo.
echo  silentium - kill switch de voz
echo.

if not exist "%EXE%" (
    where cargo >nul 2>&1
    if errorlevel 1 (
        echo [x] silentium.exe nao encontrado e cargo nao esta no PATH.
        echo     rode: cargo build --release
        goto :fail
    )
    echo [*] compilando ^(primeira vez, ~1 min^)...
    cargo build --release
    if errorlevel 1 goto :fail
)

net session >nul 2>&1
if errorlevel 1 (
    echo [*] pedindo permissao de Administrador...
    powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -Verb RunAs" >nul 2>&1
    exit /b
)

echo   [1] derrubar a call do Discord
echo   [2] restaurar
echo.
set "MODE=1"
set /p MODE=Escolha [1]: 
if "%MODE%"=="" set "MODE=1"

if "%MODE%"=="2" (
    "%EXE%" net unblock
    echo.
    pause
    exit /b
)

echo.
echo [*] token de USUARIO ^(opcional; Enter pula^)
echo     usado apenas para ler o convite. Detectar e derrubar nao usam token.
set "SAVED="
if exist "%TOKFILE%" (
    set /p SAVED=<"%TOKFILE%"
    if defined SAVED (
        echo [*] token salvo: ...!SAVED:~-6!
        set "USE=!"
        set /p USE=Usar esse token? [S/n]: 
        if /i "!USE!"=="n" set "SAVED="
    )
)
if not defined SAVED (
    set "SAVED="
    set /p SAVED=Token: 
    if defined SAVED (
        if not exist "%TOKDIR%" mkdir "%TOKDIR%" >nul 2>&1
        >"%TOKFILE%" echo !SAVED!
        echo [*] salvo em %TOKFILE%
    )
)

echo.
set "LINK="
set /p LINK=Link da call ^(Enter pula^): 
if defined LINK (
    if defined SAVED (
        echo.
        for /f "delims=" %%R in ('"%EXE%" --user-token --token "!SAVED!" resolve "!LINK!" 2^>^&1') do echo     %%R
    ) else (
        echo     ^(sem token, link ignorado^)
    )
)

echo.
echo [*] procurando call ativa...
"%EXE%" detect

echo.
set "SECS=30"
set /p SECS=Segundos com o Discord cortado [30]: 
if "!SECS!"=="" set "SECS=30"

echo.
"%EXE%" net auto --seconds !SECS!
echo.
echo [*] pronto. O Discord voltou a funcionar.
pause
exit /b 0

:fail
echo.
echo [x] falhou. Veja a mensagem acima.
pause
exit /b 1
