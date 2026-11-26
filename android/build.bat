@echo off
setlocal EnableExtensions EnableDelayedExpansion
title Suppa Browser for Android - build
cd /d "%~dp0"

echo ============================================
echo   Suppa Browser for Android - install tools and build
echo ============================================
echo.
rem Everything this script installs goes to one folder (no administrator rights):
rem   %LOCALAPPDATA%\SuppaAndroidTools   (Java 17, Android SDK, Gradle)
rem Override with SUPPA_ANDROID_TOOLS. Pass "tools" to only install the tools.

if not defined SUPPA_ANDROID_TOOLS set "SUPPA_ANDROID_TOOLS=%LOCALAPPDATA%\SuppaAndroidTools"
set "T=%SUPPA_ANDROID_TOOLS%"
if not exist "%T%" mkdir "%T%"
set "GRADLE_VER=8.11.1"

rem ---------------------------------------------------------------
rem 1. Java 17 (Eclipse Temurin)
rem ---------------------------------------------------------------
set "JAVA_HOME="
for /d %%d in ("%T%\jdk-17*") do set "JAVA_HOME=%%d"
if defined JAVA_HOME goto :java_ok
echo [1/4] Java 17 not found - downloading...
curl.exe -L --fail -o "%T%\jdk.zip" "https://api.adoptium.net/v3/binary/latest/17/ga/windows/x64/jdk/hotspot/normal/eclipse?project=jdk"
if errorlevel 1 goto :fail
tar.exe -xf "%T%\jdk.zip" -C "%T%"
if errorlevel 1 goto :fail
del "%T%\jdk.zip"
for /d %%d in ("%T%\jdk-17*") do set "JAVA_HOME=%%d"
if not defined JAVA_HOME goto :fail
:java_ok
set "PATH=%JAVA_HOME%\bin;%PATH%"
echo [1/4] Java OK (%JAVA_HOME%)

rem ---------------------------------------------------------------
rem 2. Android SDK (command-line tools, platform 35, build tools)
rem ---------------------------------------------------------------
set "ANDROID_HOME=%T%\sdk"
set "SDKM=%ANDROID_HOME%\cmdline-tools\latest\bin\sdkmanager.bat"
if exist "%SDKM%" goto :sdk_tools_ok
echo [2/4] Android SDK not found - downloading...
curl.exe -L --fail -o "%T%\cmdtools.zip" "https://dl.google.com/android/repository/commandlinetools-win-11076708_latest.zip"
if errorlevel 1 goto :fail
if not exist "%ANDROID_HOME%\cmdline-tools" mkdir "%ANDROID_HOME%\cmdline-tools"
tar.exe -xf "%T%\cmdtools.zip" -C "%ANDROID_HOME%\cmdline-tools"
if errorlevel 1 goto :fail
move "%ANDROID_HOME%\cmdline-tools\cmdline-tools" "%ANDROID_HOME%\cmdline-tools\latest" >nul
del "%T%\cmdtools.zip"
:sdk_tools_ok
if exist "%ANDROID_HOME%\platforms\android-35" if exist "%ANDROID_HOME%\build-tools\35.0.0" goto :sdk_ok
echo [2/4] Installing Android platform 35 and build tools (accepting the SDK licences)...
rem The Android SDK licence (the same answer "sdkmanager --licenses" writes).
if not exist "%ANDROID_HOME%\licenses" mkdir "%ANDROID_HOME%\licenses"
powershell -NoProfile -Command "[IO.File]::WriteAllText($env:ANDROID_HOME + '\licenses\android-sdk-license', [char]10 + '24333f8a63b6825ea9c5514f83c2829b004d1fee' + [char]10 + 'd56f5187479451eabf01fb78af6dfcb131a6481e' + [char]10 + '8933bad161af4178b1185d1a37fbf41ea5269c55')"
powershell -NoProfile -ExecutionPolicy Bypass -Command "& $env:SDKM ('--sdk_root=' + $env:ANDROID_HOME) 'platforms;android-35' 'build-tools;35.0.0' 'platform-tools'"
if not exist "%ANDROID_HOME%\platforms\android-35" goto :fail
if not exist "%ANDROID_HOME%\build-tools\35.0.0" goto :fail
:sdk_ok
echo [2/4] Android SDK OK

rem ---------------------------------------------------------------
rem 3. Gradle
rem ---------------------------------------------------------------
set "GRADLE=%T%\gradle-%GRADLE_VER%\bin\gradle.bat"
if exist "%GRADLE%" goto :gradle_ok
echo [3/4] Gradle not found - downloading...
curl.exe -L --fail -o "%T%\gradle.zip" "https://services.gradle.org/distributions/gradle-%GRADLE_VER%-bin.zip"
if errorlevel 1 goto :fail
tar.exe -xf "%T%\gradle.zip" -C "%T%"
if errorlevel 1 goto :fail
del "%T%\gradle.zip"
:gradle_ok
echo [3/4] Gradle OK
if /i "%~1"=="tools" goto :done_tools

rem ---------------------------------------------------------------
rem 4. Signing key + build
rem ---------------------------------------------------------------
rem The APK is signed with your own key. Keep it: Android only installs an
rem update over Suppa when it is signed with the same key. Kept outside the
rem project (never commit it). Override with SUPPA_ANDROID_KEYSTORE.
if not defined SUPPA_ANDROID_KEYSTORE set "SUPPA_ANDROID_KEYSTORE=%LOCALAPPDATA%\SuppaBrowser-android\suppa-release.jks"
for %%f in ("%SUPPA_ANDROID_KEYSTORE%") do set "KDIR=%%~dpf"
if not exist "%KDIR%" mkdir "%KDIR%"
set "KPROPS=%SUPPA_ANDROID_KEYSTORE%.properties"
if exist "%SUPPA_ANDROID_KEYSTORE%" goto :key_ok
echo [4/4] Creating your signing key (once): %SUPPA_ANDROID_KEYSTORE%
for /f %%p in ('powershell -NoProfile -Command "[guid]::NewGuid().ToString('N')"') do set "KPASS=%%p"
keytool -genkeypair -v -keystore "%SUPPA_ANDROID_KEYSTORE%" -alias suppa -keyalg RSA -keysize 3072 -validity 10000 -storepass !KPASS! -keypass !KPASS! -dname "CN=Suppa Browser" >nul
if errorlevel 1 goto :fail
> "%KPROPS%" echo password=!KPASS!
:key_ok
for /f "usebackq tokens=1,* delims==" %%a in ("%KPROPS%") do if "%%a"=="password" set "SUPPA_KEY_PASSWORD=%%b"
set "SUPPA_KEYSTORE=%SUPPA_ANDROID_KEYSTORE%"

echo [4/4] Building the APK...
> local.properties echo sdk.dir=%ANDROID_HOME:\=/%
call "%GRADLE%" --no-daemon -q assembleRelease
if errorlevel 1 goto :fail

for /f "usebackq tokens=2 delims== " %%v in (`findstr /b /c:"version = " ..\Cargo.toml`) do if not defined VER set "VER=%%~v"
if not exist "..\dist" mkdir "..\dist"
copy /y "app\build\outputs\apk\release\app-release.apk" "..\dist\Suppa-%VER%-android.apk" >nul
if errorlevel 1 goto :fail
echo.
echo Done: dist\Suppa-%VER%-android.apk
echo Install it on a phone: copy it there and open it, or run
echo   "%ANDROID_HOME%\platform-tools\adb.exe" install -r ..\dist\Suppa-%VER%-android.apk
goto :end

:done_tools
echo Tools are ready in %T%
goto :end

:fail
echo.
echo Build FAILED - see the messages above.
if not defined SUPPA_NO_PAUSE pause
exit /b 1

:end
if not defined SUPPA_NO_PAUSE if /i not "%~1"=="tools" pause
exit /b 0
