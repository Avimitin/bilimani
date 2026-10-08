$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot
& (Join-Path $PSScriptRoot 'build.ps1')
$destination = Join-Path $root 'dist'
New-Item -ItemType Directory -Force -Path $destination | Out-Null
Copy-Item -LiteralPath (Join-Path $root 'README.md') -Destination $destination
Copy-Item -LiteralPath (Join-Path $root 'target\release\bilimani.dll') -Destination $destination
Copy-Item -LiteralPath (Join-Path $root 'target\release\bilimani-config.exe') -Destination $destination
# Stage current assets afresh so deleted source files cannot linger in the ZIP.
$staticRoot = [IO.Path]::GetFullPath((Join-Path $destination 'bilimani_web'))
$expectedStaticRoot = [IO.Path]::GetFullPath((Join-Path $root 'dist\bilimani_web'))
if ($staticRoot -ne $expectedStaticRoot) { throw 'Unexpected static staging path' }
if (Test-Path -LiteralPath $staticRoot) {
    if ((Get-Item -LiteralPath $staticRoot).Attributes -band [IO.FileAttributes]::ReparsePoint) {
        throw 'Static staging directory must not be a reparse point'
    }
    Remove-Item -LiteralPath $staticRoot -Recurse -Force
}
New-Item -ItemType Directory -Path $staticRoot | Out-Null
Get-ChildItem -LiteralPath (Join-Path $root 'web') -Directory | ForEach-Object {
    Copy-Item -LiteralPath $_.FullName -Destination $staticRoot -Recurse
}
$cargo = (Get-Command cargo -ErrorAction Stop).Source
Push-Location $root
try {
    $metadata = (& $cargo metadata --locked --offline --no-deps --format-version 1 | ConvertFrom-Json)
    if ($LASTEXITCODE -ne 0) { throw 'Cannot collect package metadata' }
} finally { Pop-Location }
$project = $metadata.packages | Where-Object { $_.manifest_path -eq (Join-Path $root 'Cargo.toml') }
if (-not $project) { throw 'Cannot find root package version' }
$archiveFiles = @('bilimani.dll','bilimani-config.exe','bilimani_web','README.md') | ForEach-Object { Join-Path $destination $_ }
$archive = Join-Path $destination "bilimani-$($project.version).zip"
Compress-Archive -LiteralPath $archiveFiles -DestinationPath $archive -Force
if ($env:GITHUB_OUTPUT) {
    "archive=$archive" | Out-File -FilePath $env:GITHUB_OUTPUT -Encoding utf8 -Append
}
Write-Output "Packaged in $archive"
