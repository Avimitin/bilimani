# Optional workspace-local Windows SDK fallback. Does not modify VS or registry.
$ErrorActionPreference = 'Stop'
$version = '10.0.28000.2705'
$root = Join-Path (Split-Path $PSScriptRoot) 'reference\sdk'
Add-Type -AssemblyName System.IO.Compression.FileSystem
foreach ($package in @('microsoft.windows.sdk.cpp', 'microsoft.windows.sdk.cpp.x64')) {
    $destination = Join-Path $root $package
    if (Test-Path -LiteralPath $destination) { Write-Output "Already present: $destination"; continue }
    New-Item -ItemType Directory -Force -Path $root | Out-Null
    $archive = "$destination.zip"
    Invoke-WebRequest -UseBasicParsing "https://api.nuget.org/v3-flatcontainer/$package/$version/$package.$version.nupkg" -OutFile $archive
    [IO.Compression.ZipFile]::ExtractToDirectory($archive, $destination)
}
