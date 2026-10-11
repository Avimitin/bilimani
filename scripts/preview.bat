@echo off
setlocal
pushd "%~dp0.." || exit /b 1
powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "& '.\scripts\build.ps1' -CargoArgs @('run', '-p', 'bilimani-iidx', '--example', 'api_preview', '--locked')"
set "preview_exit=%errorlevel%"
popd
if not "%preview_exit%"=="0" pause
exit /b %preview_exit%
