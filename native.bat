@echo off
setlocal EnableExtensions
cd /d "%~dp0"
node scripts\native\prepare-onnx.cjs
if errorlevel 1 exit /b %errorlevel%
node scripts\native\prepare-libclang.cjs
if errorlevel 1 exit /b %errorlevel%
node scripts\native\prepare-rnnoise.cjs
if errorlevel 1 exit /b %errorlevel%
if "%~1"=="check" (
  node scripts\native\verify.cjs
  exit /b
)
set "CARGO=%USERPROFILE%\.cargo\bin\cargo.exe"
if defined CARGO_HOME set "CARGO=%CARGO_HOME%\bin\cargo.exe"
if not exist "%CARGO%" set "CARGO=cargo"
if "%~1"=="" (
  "%CARGO%" run --manifest-path rust\Cargo.toml --locked -p zabor-native
) else (
  "%CARGO%" run --manifest-path rust\Cargo.toml --locked -p zabor-native -- %*
)
exit /b %errorlevel%
