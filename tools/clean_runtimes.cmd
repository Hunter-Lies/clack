@echo off
rem ============================================================
rem  Clack - wipe the runtimes on this machine (test helper)
rem
rem  Removes everything Clack would install, so the clean-machine
rem  path can be tested:
rem    1. uninstall every Microsoft Visual C++ 2005 - 2022 (x86/x64)
rem    2. delete the loose VB5 / VB6 / CRT 70-71 dlls Clack copies
rem       into System32 / SysWOW64
rem    3. remove C:\ProgramData\Click (payload, install sources, logs)
rem
rem  Usage:
rem    double-click            = do it (asks for admin)
rem    clean_runtimes.cmd list = list only, change nothing
rem
rem  NOTE: keep this file ASCII-only. A .cmd containing non-ASCII text
rem  breaks under some console code pages (a GBK trail byte can be '|'
rem  or '\', which splits lines mid-command).
rem ============================================================
setlocal EnableExtensions EnableDelayedExpansion
set "SELF=%~f0"
set "MODE=%~1"
set "SEEN=%TEMP%\clack_clean_runtimes.seen"

if /i "%MODE%"=="list" goto :LIST_ONLY

rem ---- needs admin: relaunch ourselves elevated ----
net session >nul 2>&1
if errorlevel 1 (
    echo.
    echo Asking for administrator rights...
    powershell -NoProfile -Command "Start-Process -FilePath '%SELF%' -Verb RunAs" >nul 2>&1
    if errorlevel 1 echo Elevation was cancelled, nothing was changed.
    exit /b
)

title Clack - wipe runtimes (test helper)
set "LOG=%~dp0clean_runtimes.log"
set "FAILED=0"

echo ============================================================ > "%LOG%"
echo Clack wipe runtimes  %DATE% %TIME% >> "%LOG%"
echo ============================================================ >> "%LOG%"

echo.
echo ============================================================
echo   Clack - wipe runtimes (test helper)
echo ============================================================
echo.

rem ============ 1. uninstall products ============
echo [1/4] Uninstalling Visual C++ products...
type nul > "%SEEN%" 2>nul

call :SCAN "HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"
call :SCAN "HKLM\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"

rem ============ 2. drop loose dlls ============
echo.
echo [2/4] Deleting the loose dlls Clack copied into the system dirs...
set "DLLS=MSVBVM50.DLL MSVBVM60.DLL MSVCR70.DLL MSVCP70.DLL atl70.dll mfc70.dll mfc70u.dll mfc70chs.dll MSVCR71.DLL MSVCP71.DLL atl71.dll MFC71.DLL mfc71u.dll mfc71chs.dll"
for %%D in ("%SystemRoot%\System32" "%SystemRoot%\SysWOW64") do (
    if exist "%%~D" for %%F in (%DLLS%) do call :DROP_ONE "%%~D\%%~F"
)

rem ============ 3. work dir ============
echo.
echo [3/4] Removing C:\ProgramData\Click...
if exist "%ProgramData%\Click" (
    rd /s /q "%ProgramData%\Click" >nul 2>&1
    if exist "%ProgramData%\Click" (
        echo     failed - something still holds a file, retry after a reboot
        echo     FAILED to remove C:\ProgramData\Click >> "%LOG%"
    ) else (
        echo     done
        echo     removed C:\ProgramData\Click >> "%LOG%"
    )
) else (
    echo     nothing there
)

rem ============ 4. verify ============
echo.
echo [4/4] Checking what is left...
for /f "delims=" %%K in ('reg query "HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall" /s /d /f "Visual C++" 2^>nul ^| findstr /i /r /c:"^HKEY.*{[0-9A-Fa-f-]*}$"') do call :REMAIN_ONE "%%K"
for /f "delims=" %%K in ('reg query "HKLM\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall" /s /d /f "Visual C++" 2^>nul ^| findstr /i /r /c:"^HKEY.*{[0-9A-Fa-f-]*}$"') do call :REMAIN_ONE "%%K"

echo.
echo ============================================================
if "%FAILED%"=="0" (
    echo   Done - everything that could be removed is gone
) else (
    echo   Done - %FAILED% item(s) could not be removed, see the log
)
echo   Log: %LOG%
echo.
echo   Reboot once before testing Clack on the clean state.
echo.
pause
exit /b


rem ================= helpers =================

:SCAN
rem walk one Uninstall root, uninstall / list every Visual C++ product
for /f "delims=" %%K in ('reg query "%~1" /s /d /f "Visual C++" 2^>nul ^| findstr /i /r /c:"^HKEY.*{[0-9A-Fa-f-]*}$"') do (
    call :ONE_PRODUCT "%%K"
)
exit /b


:ONE_PRODUCT
set "KEY=%~1"
set "CODE=%~nx1"
rem the same product shows up more than once (several values match), dedupe
if not exist "%SEEN%" type nul > "%SEEN%"
findstr /i /x /c:"%CODE%" "%SEEN%" >nul 2>&1
if not errorlevel 1 exit /b
echo %CODE%>> "%SEEN%"

set "NAME="
for /f "tokens=2,*" %%A in ('reg query "%KEY%" /v DisplayName 2^>nul ^| findstr /i /c:"DisplayName"') do set "NAME=%%B"
set "US="
for /f "tokens=2,*" %%A in ('reg query "%KEY%" /v UninstallString 2^>nul ^| findstr /i /c:"UninstallString"') do set "US=%%B"

if /i "%MODE%"=="list" (
    echo    !CODE!  !NAME!
    exit /b
)

echo    uninstall: %NAME%
echo ---- %CODE% %NAME% ---- >> "%LOG%"

rem burn bundle (vc_redist) registers an uninstall command, MSI products do not
echo !US! | findstr /i "msiexec" >nul 2>&1 && goto :KILL_MSI
if not "!US!"=="" (
    %US% /quiet /norestart >> "%LOG%" 2>&1
    if not errorlevel 1 goto :KILLED
    echo     uninstall command failed, retrying by product code >> "%LOG%"
)

:KILL_MSI
msiexec /X%CODE% /qn /norestart >> "%LOG%" 2>&1
if errorlevel 1 (
    rem 1605 = not installed at all, that is fine
    if !ERRORLEVEL!==1605 goto :KILLED
    echo     failed, msiexec returned !ERRORLEVEL!
    echo     msiexec returned !ERRORLEVEL! >> "%LOG%"
    set /a FAILED+=1
    exit /b
)

:KILLED
echo     removed
exit /b


:DROP_ONE
if not exist "%~1" exit /b
if /i "%MODE%"=="list" (
    echo    %~1
    exit /b
)
del /f /q "%~1" >nul 2>&1
if exist "%~1" (
    echo     cannot delete %~1 (loaded by some process?)
    echo     cannot delete %~1 >> "%LOG%"
    set /a FAILED+=1
) else (
    echo     deleted %~1
    echo     deleted %~1 >> "%LOG%"
)
rem clean up the .old left behind when a locked file was renamed aside
del /f /q "%~1.old" >nul 2>&1
exit /b


:REMAIN_ONE
set "NAME="
for /f "tokens=2,*" %%A in ('reg query "%~1" /v DisplayName 2^>nul ^| findstr /i /c:"DisplayName"') do set "NAME=%%B"
if not "!NAME!"=="" echo    still here: !NAME!
if not "!NAME!"=="" echo    still here: !NAME! >> "%LOG%"
exit /b


:LIST_ONLY
echo.
echo Visual C++ products found on this machine (list mode, nothing is changed):
echo.
call :SCAN "HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"
call :SCAN "HKLM\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"
echo.
echo Loose dlls Clack copies into the system dirs:
set "DLLS=MSVBVM50.DLL MSVBVM60.DLL MSVCR70.DLL MSVCP70.DLL atl70.dll mfc70.dll mfc70u.dll mfc70chs.dll MSVCR71.DLL MSVCP71.DLL atl71.dll MFC71.DLL mfc71u.dll mfc71chs.dll"
for %%D in ("%SystemRoot%\System32" "%SystemRoot%\SysWOW64") do (
    if exist "%%~D" for %%F in (%DLLS%) do call :DROP_ONE "%%~D\%%~F"
)
echo.
exit /b
