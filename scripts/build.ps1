param([string[]]$CargoArgs = @('build', '--release'))
$ErrorActionPreference = 'Stop'
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
$vs = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $vs) { throw 'Install Visual Studio C++ build tools and Windows SDK.' }
$vcvars = Join-Path $vs 'VC\Auxiliary\Build\vcvars64.bat'
$environment = & $env:ComSpec /c "`"$vcvars`" >nul && set"
if ($LASTEXITCODE -ne 0) { throw 'vcvars64 failed' }
foreach ($line in $environment) {
    $pair = $line -split '=', 2
    if ($pair.Length -eq 2 -and $pair[0] -notmatch '^=') {
        [Environment]::SetEnvironmentVariable($pair[0], $pair[1], 'Process')
    }
}
$sdkRoot = Join-Path (Split-Path $PSScriptRoot) 'reference\sdk'
if (Test-Path "$sdkRoot\microsoft.windows.sdk.cpp\c\Include") {
    $sdkInclude = Get-ChildItem "$sdkRoot\microsoft.windows.sdk.cpp\c\Include" -Directory | Sort-Object Name -Descending | Select-Object -First 1
    $sdkLib = Get-ChildItem "$sdkRoot\microsoft.windows.sdk.cpp.x64" -Filter kernel32.lib -Recurse | Select-Object -First 1
    $ucrtLib = Get-ChildItem "$sdkRoot\microsoft.windows.sdk.cpp.x64\c\ucrt\x64" -Filter ucrt.lib | Select-Object -First 1
    if ($sdkLib -and $ucrtLib) {
        $env:LIB = "$($sdkLib.DirectoryName);$($ucrtLib.DirectoryName);$env:LIB"
        $env:INCLUDE = "$($sdkInclude.FullName)\ucrt;$($sdkInclude.FullName)\shared;$($sdkInclude.FullName)\um;$env:INCLUDE"
    }
}
Push-Location (Split-Path $PSScriptRoot)
try { & cargo @CargoArgs; if ($LASTEXITCODE -ne 0) { throw "cargo failed: $LASTEXITCODE" } }
finally { Pop-Location }
