@echo off
setlocal EnableDelayedExpansion

set "PROJECT_DIR=%~dp0"
cd /d "%PROJECT_DIR%"

echo ============================================================
echo   ZABOR Desktop - Auto-Setup, Sync and Launch
echo ============================================================
echo.

:: ----------------------------------------------------------------
:: 1. Auto-discover or install Git
:: ----------------------------------------------------------------
:CHECK_GIT
where git >nul 2>nul
if not errorlevel 1 goto :GIT_READY

if exist "C:\Program Files\Git\cmd\git.exe" (
    set "PATH=C:\Program Files\Git\cmd;C:\Program Files\Git\bin;!PATH!"
    goto :GIT_READY
)
if exist "%LOCALAPPDATA%\Programs\Git\cmd\git.exe" (
    set "PATH=%LOCALAPPDATA%\Programs\Git\cmd;!PATH!"
    goto :GIT_READY
)

echo [!] Git not found. Installing Git automatically...
where winget >nul 2>nul
if not errorlevel 1 (
    echo [i] Installing Git via winget...
    winget install --id Git.Git -e --source winget --accept-package-agreements --accept-source-agreements --silent
) else (
    echo [i] Downloading Git installer via curl...
    curl -Lo "%TEMP%\git_setup.exe" "https://github.com/git-for-windows/git/releases/latest/download/Git-64-bit.exe"
    echo [i] Running Git installer...
    start /wait "" "%TEMP%\git_setup.exe" /VERYSILENT /NORESTART
    del /f /q "%TEMP%\git_setup.exe" >nul 2>nul
)

if exist "C:\Program Files\Git\cmd\git.exe" set "PATH=C:\Program Files\Git\cmd;C:\Program Files\Git\bin;!PATH!"
if exist "%LOCALAPPDATA%\Programs\Git\cmd\git.exe" set "PATH=%LOCALAPPDATA%\Programs\Git\cmd;!PATH!"

where git >nul 2>nul
if errorlevel 1 (
    echo [!] ERROR: Failed to install or detect Git. Please install manually: https://git-scm.com/
    pause
    exit /b 1
)

:GIT_READY
echo [+] Git is available.

:: ----------------------------------------------------------------
:: 2. Auto-discover or install Node.js & npm
:: ----------------------------------------------------------------
:CHECK_NODE
where node >nul 2>nul
if not errorlevel 1 goto :CHECK_NPM

if exist "C:\Program Files\nodejs\node.exe" (
    set "PATH=C:\Program Files\nodejs;%APPDATA%\npm;!PATH!"
    goto :CHECK_NPM
)
if exist "%LOCALAPPDATA%\Programs\nodejs\node.exe" (
    set "PATH=%LOCALAPPDATA%\Programs\nodejs;%APPDATA%\npm;!PATH!"
    goto :CHECK_NPM
)

echo [!] Node.js not found. Installing Node.js LTS automatically...
where winget >nul 2>nul
if not errorlevel 1 (
    echo [i] Installing Node.js LTS via winget...
    winget install --id OpenJS.NodeJS.LTS -e --source winget --accept-package-agreements --accept-source-agreements --silent
) else (
    echo [i] Downloading Node.js LTS installer via curl...
    curl -Lo "%TEMP%\node_setup.msi" "https://nodejs.org/dist/v20.18.0/node-v20.18.0-x64.msi"
    echo [i] Installing Node.js...
    msiexec /i "%TEMP%\node_setup.msi" /qn /norestart
    del /f /q "%TEMP%\node_setup.msi" >nul 2>nul
)

if exist "C:\Program Files\nodejs\node.exe" set "PATH=C:\Program Files\nodejs;%APPDATA%\npm;!PATH!"
where node >nul 2>nul
if errorlevel 1 (
    echo [!] ERROR: Failed to install or detect Node.js. Please install manually: https://nodejs.org/
    pause
    exit /b 1
)

:CHECK_NPM
where npm >nul 2>nul
if errorlevel 1 (
    if exist "C:\Program Files\nodejs\npm.cmd" set "PATH=C:\Program Files\nodejs;%APPDATA%\npm;!PATH!"
)
echo [+] Node.js and npm are available.

:: ----------------------------------------------------------------
:: 3. Auto-discover or install VS Code
:: ----------------------------------------------------------------
:CHECK_VSCODE
where code >nul 2>nul
if not errorlevel 1 goto :VSCODE_READY

if exist "%LOCALAPPDATA%\Programs\Microsoft VS Code\bin\code.cmd" (
    set "PATH=%LOCALAPPDATA%\Programs\Microsoft VS Code\bin;!PATH!"
    goto :VSCODE_READY
)
if exist "C:\Program Files\Microsoft VS Code\bin\code.cmd" (
    set "PATH=C:\Program Files\Microsoft VS Code\bin;!PATH!"
    goto :VSCODE_READY
)

echo [i] VS Code not found in PATH. Checking winget...
where winget >nul 2>nul
if not errorlevel 1 (
    echo [i] Installing VS Code via winget...
    winget install --id Microsoft.VisualStudioCode -e --source winget --accept-package-agreements --accept-source-agreements --silent
    if exist "%LOCALAPPDATA%\Programs\Microsoft VS Code\bin\code.cmd" set "PATH=%LOCALAPPDATA%\Programs\Microsoft VS Code\bin;!PATH!"
    if exist "C:\Program Files\Microsoft VS Code\bin\code.cmd" set "PATH=C:\Program Files\Microsoft VS Code\bin;!PATH!"
)

:VSCODE_READY

:: ----------------------------------------------------------------
:: 4. Git Repository Setup / Pull
:: ----------------------------------------------------------------
set "REPO_URL=https://github.com/vnkdevelop/zabor-desktop.git"

if exist ".git" goto :REPO_EXISTS

:CLONE_REPO
echo.
echo [i] Git repository not initialized here.
echo [i] Initializing and fetching from %REPO_URL%...
git init
git remote add origin %REPO_URL%
git fetch --all --prune
if errorlevel 1 (
    echo [!] ERROR: Could not fetch from GitHub. Check your network.
    pause
    exit /b 1
)
git checkout -f -B main origin/main
echo [+] Project files checked out successfully!
goto :BRANCH_CHECK

:REPO_EXISTS
echo.
echo [i] Fetching latest changes and all branches from GitHub...
git fetch --all --prune
if errorlevel 1 (
    echo [!] Warning: git fetch failed. Check your network connection.
)

:BRANCH_CHECK
set "CURR_BRANCH=main"
for /f "tokens=*" %%b in ('git rev-parse --abbrev-ref HEAD 2^>nul') do set "CURR_BRANCH=%%b"
if not defined CURR_BRANCH set "CURR_BRANCH=main"

echo.
echo ------------------------------------------------------------
echo [i] Current branch: !CURR_BRANCH!
echo [i] Available branches on GitHub:
git branch -r 2>nul
echo ------------------------------------------------------------
echo.
set "TARGET_BRANCH="
set /p TARGET_BRANCH="Enter branch to switch to [or press Enter to keep '!CURR_BRANCH!']: "
if not defined TARGET_BRANCH set "TARGET_BRANCH=!CURR_BRANCH!"

:: Clean origin/ prefix if user typed it
set "TARGET_BRANCH=!TARGET_BRANCH:origin/=!"
set "TARGET_BRANCH=!TARGET_BRANCH: =!"

if not "!TARGET_BRANCH!"=="!CURR_BRANCH!" (
    echo [i] Switching to branch !TARGET_BRANCH!...
    git checkout !TARGET_BRANCH! 2>nul
    if errorlevel 1 (
        echo [i] Creating local tracking branch for origin/!TARGET_BRANCH!...
        git checkout -B !TARGET_BRANCH! origin/!TARGET_BRANCH!
        if errorlevel 1 (
            echo [!] Could not checkout !TARGET_BRANCH!. Staying on !CURR_BRANCH!.
            set "TARGET_BRANCH=!CURR_BRANCH!"
        )
    )
)

:PULL_CHANGES
echo.
echo [i] Pulling latest code for !TARGET_BRANCH!...
git pull origin !TARGET_BRANCH!
echo.

:: ----------------------------------------------------------------
:: 5. Auto-install all project dependencies (Zero Failure)
:: ----------------------------------------------------------------
:INSTALL_DEPENDENCIES
echo ============================================================
echo   Installing & verifying all project dependencies
echo ============================================================
echo.

echo [1/5] Installing npm packages (safe mode: --ignore-scripts)...
call npm install --ignore-scripts
if errorlevel 1 (
    echo [!] npm install reported issues, retrying with --force...
    call npm install --ignore-scripts --force
)

echo.
echo [2/5] Downloading Electron runtime binary...
if exist "node_modules\electron\install.js" (
    call node node_modules\electron\install.js
)

echo.
echo [3/5] Downloading DeepFilterNet3 audio assets...
if exist "scripts\fetch-deepfilter-assets.cjs" (
    call node scripts\fetch-deepfilter-assets.cjs
)

echo.
echo [4/5] Setting up native audio screenshare modules...
if exist "scripts\rebuild-native-audio.cjs" (
    call node scripts\rebuild-native-audio.cjs
)

echo.
echo [5/5] Building splash screen...
if exist "scripts\build-splash.cjs" (
    call node scripts\build-splash.cjs
)

:: Verify critical runtime modules
if not exist "node_modules\cross-env\src\bin\cross-env.js" (
    echo [!] cross-env missing, installing directly...
    call npm install cross-env --no-save
)

echo.
echo [+] All dependencies are successfully installed and verified!
echo.

:: ----------------------------------------------------------------
:: 6. Launch in VS Code
:: ----------------------------------------------------------------
:OPEN_VSCODE
where code >nul 2>nul
if not errorlevel 1 (
    echo [i] Opening project in VS Code: code .
    call code .
) else (
    echo [i] Note: VS Code command line tool not in PATH.
)

:: ----------------------------------------------------------------
:: 7. Development Launch Menu
:: ----------------------------------------------------------------
:ACTION_MENU
echo.
echo ============================================================
echo  Everything is ready! Choose an action:
echo   [1] Start application immediately: npm run dev [DEFAULT]
echo   [2] Start application in 4K mode: npm run dev:4k
echo   [3] Exit (work in VS Code)
echo ============================================================
set "RUN_ACTION=1"
set /p RUN_ACTION="Choice [1-3, default: 1]: "
if not defined RUN_ACTION set "RUN_ACTION=1"

if "!RUN_ACTION!"=="1" (
    echo.
    echo [i] Starting ZABOR Desktop: npm run dev...
    call npm run dev
)
if "!RUN_ACTION!"=="2" (
    echo.
    echo [i] Starting ZABOR Desktop in 4K: npm run dev:4k...
    call npm run dev:4k
)

echo.
echo Done.
pause
