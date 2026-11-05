@echo off
setlocal EnableExtensions

:: Determine project directory robustly
set "PROJECT_DIR=%~dp0"

if not exist "%PROJECT_DIR%package.json" (
    if exist "%USERPROFILE%\Documents\zabor\Frontend\zabor-desktop\package.json" (
        set "PROJECT_DIR=%USERPROFILE%\Documents\zabor\Frontend\zabor-desktop\"
    ) else (
        echo ERROR: Could not find project directory with package.json!
        echo Current script directory: %~dp0
        pause
        exit /b 1
    )
)

cd /d "%PROJECT_DIR%"

echo.
echo ========== ZABOR BETA RELEASE (PRE-RELEASE) ==========
echo.

:: Check Git availability
where git >nul 2>nul
if %errorlevel% neq 0 (
    echo [!] ERROR: Git is not installed or not in PATH!
    echo Please install Git from https://git-scm.com/ to commit and push changes.
    pause
    exit /b 1
)

:: Check GitHub CLI availability
set HAS_GH=true
where gh >nul 2>nul
if %errorlevel% neq 0 (
    echo [!] ERROR: GitHub CLI gh is required for beta publishing.
    exit /b 1
)

:: Read version from package.json

:: Build
echo Verification, packaging and publishing are handled by the beta-only pipeline.

:: Find installer executable

:: Commit and Push

:: Create GitHub Release
node --disable-warning=MODULE_TYPELESS_PACKAGE_JSON scripts/release.mjs beta %*
exit /b %errorlevel%
