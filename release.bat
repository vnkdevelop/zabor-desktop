@echo off
setlocal EnableExtensions

:: Determine project directory robustly
set "PROJECT_DIR=%~dp0"
if not exist "%PROJECT_DIR%package.json" exit /b 1
cd /d "%PROJECT_DIR%"

:: Check Git availability
where git >nul 2>nul
if errorlevel 1 (
    echo ERROR: Git is required.
    exit /b 1
)

:: Check GitHub CLI availability
where gh >nul 2>nul
if errorlevel 1 (
    echo ERROR: GitHub CLI gh is required.
    exit /b 1
)

:: Read version from package.json
:: Build
:: Find installer executable
:: Commit and Push
:: Create GitHub Release
node --disable-warning=MODULE_TYPELESS_PACKAGE_JSON scripts/release.mjs auto %*
exit /b %errorlevel%
