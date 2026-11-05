@echo off
setlocal EnableExtensions
cd /d "%~dp0"
node --disable-warning=MODULE_TYPELESS_PACKAGE_JSON scripts/promote-beta.mjs %*
exit /b %errorlevel%
