@echo off
setlocal
pushd "%~dp0.." || exit /b 1
if exist "reference\node\node-v22.23.3-win-x64\node.exe" set "PATH=%CD%\reference\node\node-v22.23.3-win-x64;%PATH%"
call node web/apps/ddr/build.mjs --preview
if errorlevel 1 goto failed
echo Open http://127.0.0.1:32134/ddr/street/
powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "& '.\scripts\build.ps1' -CargoArgs @('run', '-p', 'bilimani-ddr', '--example', 'api_preview', '--locked', '--', 'dist/ddr-preview')"
if errorlevel 1 goto failed
popd
exit /b 0
:failed
popd
pause
exit /b 1
