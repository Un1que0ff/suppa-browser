@echo off
setlocal EnableExtensions EnableDelayedExpansion
title Suppa Browser - build
cd /d "%~dp0"

echo ============================================
echo   Suppa Browser - install tools and build
echo ============================================
echo.

rem ---------------------------------------------------------------
rem 1. Rust (rustup + stable MSVC toolchain)
rem ---------------------------------------------------------------
if exist "%USERPROFILE%\.cargo\bin\cargo.exe" set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
where cargo >nul 2>nul
if not errorlevel 1 goto :rust_ok

echo [1/4] Rust not found - installing...
powershell -NoProfile -ExecutionPolicy Bypass -Command "$ProgressPreference='SilentlyContinue'; Invoke-WebRequest -UseBasicParsing 'https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe' -OutFile \"$env:TEMP\rustup-init.exe\""
if errorlevel 1 goto :fail_rust
"%TEMP%\rustup-init.exe" -y --default-toolchain stable --profile minimal --default-host x86_64-pc-windows-msvc
if errorlevel 1 goto :fail_rust
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
:rust_ok
echo [1/4] Rust OK
rustup default stable >nul 2>nul

rem ---------------------------------------------------------------
rem 2. Microsoft C++ Build Tools (linker + Windows SDK)
rem ---------------------------------------------------------------
set "VSWHERE=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
set "VSPATH="
if exist "%VSWHERE%" (
  for /f "usebackq delims=" %%i in (`"%VSWHERE%" -products * -latest -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`) do set "VSPATH=%%i"
)
if defined VSPATH goto :msvc_ok

echo [2/4] C++ Build Tools not found - installing (several GB, takes a while)...
echo       Windows will ask for administrator permission.
powershell -NoProfile -ExecutionPolicy Bypass -Command "$ProgressPreference='SilentlyContinue'; Invoke-WebRequest -UseBasicParsing 'https://aka.ms/vs/17/release/vs_BuildTools.exe' -OutFile \"$env:TEMP\vs_BuildTools.exe\""
if errorlevel 1 goto :fail_msvc
"%TEMP%\vs_BuildTools.exe" --quiet --wait --norestart --nocache --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended
rem 3010 = success, reboot recommended
if errorlevel 1 if not errorlevel 3010 goto :fail_msvc
:msvc_ok
echo [2/4] C++ Build Tools OK

rem ---------------------------------------------------------------
rem 3. WebView2 runtime (already part of Windows 10/11, checked anyway)
rem ---------------------------------------------------------------
set "WV2="
reg query "HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" /v pv >nul 2>nul && set "WV2=1"
reg query "HKCU\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" /v pv >nul 2>nul && set "WV2=1"
if defined WV2 goto :wv2_ok
echo [3/4] WebView2 runtime not found - installing...
powershell -NoProfile -ExecutionPolicy Bypass -Command "$ProgressPreference='SilentlyContinue'; Invoke-WebRequest -UseBasicParsing 'https://go.microsoft.com/fwlink/p/?LinkId=2124703' -OutFile \"$env:TEMP\MicrosoftEdgeWebview2Setup.exe\""
"%TEMP%\MicrosoftEdgeWebview2Setup.exe" /silent /install
:wv2_ok
echo [3/4] WebView2 runtime OK

rem ---------------------------------------------------------------
rem 4. Build
rem ---------------------------------------------------------------
echo [4/4] Building Suppa.exe (release)...
echo.
rem Your user folder and this folder are written as "~" and "." inside the exe
rem instead of their real paths (they would show your Windows user name).
rem The flags are joined with the 0x1F separator so paths with spaces work.
powershell -NoProfile -ExecutionPolicy Bypass -Command "$f = @('-Ctarget-feature=+crt-static', ('--remap-path-prefix=' + $env:USERPROFILE + '=~'), ('--remap-path-prefix=' + (Get-Location).Path + '=.')); $env:CARGO_ENCODED_RUSTFLAGS = $f -join [char]0x1f; cargo build --release; exit $LASTEXITCODE"
if errorlevel 1 goto :fail_build

if not exist "dist" mkdir "dist"
copy /y "target\release\Suppa.exe" "dist\Suppa.exe" >nul
if errorlevel 1 (
  echo ERROR: could not replace dist\Suppa.exe - close Suppa and run build.bat again.
  goto :end_fail
)
rem Scripts go next to the exe. /D = only copy files that are newer, so your
rem edits made directly in dist\scripts are kept.
xcopy /E /I /Y /D /Q "scripts" "dist\scripts" >nul

rem Setup for other people: one SuppaSetup.exe with Suppa.exe + scripts inside.
echo.
echo Building SuppaSetup.exe...
pushd installer
powershell -NoProfile -ExecutionPolicy Bypass -Command "$f = @('-Ctarget-feature=+crt-static', ('--remap-path-prefix=' + $env:USERPROFILE + '=~'), ('--remap-path-prefix=' + (Split-Path (Get-Location).Path) + '=.')); $env:CARGO_ENCODED_RUSTFLAGS = $f -join [char]0x1f; cargo build --release; exit $LASTEXITCODE"
set "SETUP_ERR=%errorlevel%"
popd
if not "%SETUP_ERR%"=="0" goto :fail_build
copy /y "installer\target\release\SuppaSetup.exe" "dist\SuppaSetup.exe" >nul

echo.
echo ============================================
for %%F in ("dist\Suppa.exe") do echo   Done: %%~fF  (%%~zF bytes)
for %%F in ("dist\SuppaSetup.exe") do echo   Setup: %%~fF  (%%~zF bytes)
echo   Scripts: %~dp0dist\scripts  (edit them there, then press Update)
echo ============================================
echo.
if /i not "%~1"=="/nopause" pause
exit /b 0

:fail_rust
echo ERROR: Rust installation failed. Check your internet connection.
goto :end_fail
:fail_msvc
echo ERROR: C++ Build Tools installation failed.
echo Install manually: https://visualstudio.microsoft.com/visual-cpp-build-tools/
echo (select "Desktop development with C++"), then run build.bat again.
goto :end_fail
:fail_build
echo ERROR: Build failed - see messages above.
:end_fail
echo.
if /i not "%~1"=="/nopause" pause
exit /b 1
