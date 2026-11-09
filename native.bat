@echo off
setlocal EnableExtensions DisableDelayedExpansion
cd /d "%~dp0"
if errorlevel 1 exit /b %errorlevel%
set "NATIVE_CSC=%WINDIR%\Microsoft.NET\Framework64\v4.0.30319\csc.exe"
if not exist "%NATIVE_CSC%" goto missing_compiler
if not exist "rust\target\native-developer-tools" mkdir "rust\target\native-developer-tools"
if errorlevel 1 exit /b %errorlevel%
set "RANDOM="
set "NATIVE_DEVELOPER_INVOCATION=%~dp0rust\target\native-developer-tools\invocation-%RANDOM%-%RANDOM%-%RANDOM%"
mkdir "%NATIVE_DEVELOPER_INVOCATION%" 2>nul
if errorlevel 1 goto reservation_failed
set "NATIVE_DEVELOPER_EXE=%NATIVE_DEVELOPER_INVOCATION%\launcher.exe"
cmd.exe /d /v:on /s /c ""!NATIVE_CSC!" /nologo /target:exe /r:System.Web.Extensions.dll /r:System.Management.dll /out:"!NATIVE_DEVELOPER_EXE!" scripts\native\developer-launcher.cs scripts\native\developer-cache.cs"
if errorlevel 1 goto compiler_failed
set "NATIVE_DEVELOPER_ROOT=%~dp0."
cmd.exe /d /v:on /s /c ""!NATIVE_DEVELOPER_EXE!" "!NATIVE_DEVELOPER_ROOT!" --batch"
set "NATIVE_DEVELOPER_EXIT=%ERRORLEVEL%"
goto cleanup
:compiler_failed
set "NATIVE_DEVELOPER_EXIT=%ERRORLEVEL%"
:cleanup
if exist "%NATIVE_DEVELOPER_EXE%" del /q "%NATIVE_DEVELOPER_EXE%"
rmdir "%NATIVE_DEVELOPER_INVOCATION%" 2>nul
if errorlevel 1 echo Owned developer tool cleanup incomplete: "%NATIVE_DEVELOPER_INVOCATION%" 1>&2
exit /b %NATIVE_DEVELOPER_EXIT%
:reservation_failed
echo Cannot reserve a fresh developer tool directory; no existing artifacts were changed. 1>&2
exit /b 2
:missing_compiler
echo Native developer commands require Windows x64 .NET Framework 4 compiler tools. 1>&2
exit /b 2
