@echo off
setlocal
pushd "%~dp0.." || exit /b 1
if exist "reference\node\node-v22.23.3-win-x64\node.exe" set "PATH=%CD%\reference\node\node-v22.23.3-win-x64;%PATH%"
call npm.cmd run build:preview
if errorlevel 1 (
  popd
  pause
  exit /b 1
)
powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "& '.\scripts\build.ps1' -CargoArgs @('run', '-p', 'bilimani-iidx', '--example', 'api_preview', '--locked')"
set "preview_exit=%errorlevel%"
popd
if not "%preview_exit%"=="0" pause
exit /b %preview_exit%
