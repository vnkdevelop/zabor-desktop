@echo off
setlocal EnableExtensions
set "APP=%~dp0..\ZABOR-Beta.exe"
if not exist "%APP%" set "APP=%~dp0..\ZABOR.exe"
if not exist "%APP%" exit /b 1
echo Exit the current zabor session before starting this comparison mode.
start "" "%APP%" --legacy-dsp
exit /b 0
