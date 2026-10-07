$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot
& (Join-Path $PSScriptRoot 'build.ps1')
$destination = Join-Path $root 'dist'
New-Item -ItemType Directory -Force -Path $destination | Out-Null
Copy-Item -LiteralPath (Join-Path $root 'README.md') -Destination $destination
Copy-Item -LiteralPath (Join-Path $root 'target\release\chart_requester.dll') -Destination $destination
# Stage current assets afresh so deleted source files cannot linger in the ZIP.
$staticRoot = [IO.Path]::GetFullPath((Join-Path $destination 'chart_request_static'))
$expectedStaticRoot = [IO.Path]::GetFullPath((Join-Path $root 'dist\chart_request_static'))
if ($staticRoot -ne $expectedStaticRoot) { throw 'Unexpected static staging path' }
if (Test-Path -LiteralPath $staticRoot) {
    if ((Get-Item -LiteralPath $staticRoot).Attributes -band [IO.FileAttributes]::ReparsePoint) {
        throw 'Static staging directory must not be a reparse point'
    }
    Remove-Item -LiteralPath $staticRoot -Recurse -Force
}
Copy-Item -LiteralPath (Join-Path $root 'web') -Destination $staticRoot -Recurse
# Preserve the default static_dir for existing installs; each style is also selectable.
Get-ChildItem -LiteralPath (Join-Path $root 'web\card') -File | ForEach-Object {
    Copy-Item -LiteralPath $_.FullName -Destination $staticRoot
}
$cargo = (Get-Command cargo -ErrorAction Stop).Source
Push-Location $root
try {
    # Include licenses for all locked dependencies, including other target platforms.
    & $cargo fetch --locked
    if ($LASTEXITCODE -ne 0) { throw 'Cannot fetch dependency license sources' }
    $metadata = (& $cargo metadata --locked --offline --format-version 1 | ConvertFrom-Json)
    if ($LASTEXITCODE -ne 0) { throw 'Cannot collect dependency license metadata' }
} finally { Pop-Location }
$project = $metadata.packages | Where-Object { $_.id -eq $metadata.resolve.root }
if (-not $project) { throw 'Cannot find root package version' }
$licenseRoot = Join-Path $staticRoot 'licenses'
New-Item -ItemType Directory -Force -Path $licenseRoot | Out-Null
foreach ($file in @('LICENSE','THIRD-PARTY-NOTICES.md')) {
    Copy-Item -LiteralPath (Join-Path $root $file) -Destination $licenseRoot
}
foreach ($package in $metadata.packages) {
    if (-not $package.source) { continue }
    $crate = Split-Path $package.manifest_path
    $out = Join-Path $licenseRoot "$($package.name)-$($package.version)"
    New-Item -ItemType Directory -Force -Path $out | Out-Null
    @("Name: $($package.name)", "Version: $($package.version)", "License: $($package.license)", "Repository: $($package.repository)") | Set-Content -Encoding UTF8 -LiteralPath (Join-Path $out 'metadata.txt')
    Get-ChildItem -LiteralPath $crate -File | Where-Object { $_.Name -match '^(LICENSE|LICENCE|COPYING|COPYRIGHT|NOTICE)' } | ForEach-Object {
        Copy-Item -LiteralPath $_.FullName -Destination $out
    }
    if ($package.license_file) {
        $licenseFile = Join-Path $crate $package.license_file
        if (Test-Path -LiteralPath $licenseFile) { Copy-Item -LiteralPath $licenseFile -Destination $out }
    }
    if ($package.name -eq 'ouroboros-ui') {
        Copy-Item -LiteralPath (Join-Path $crate 'assets/fonts/OFL-Iosevka.txt') -Destination $out
        Copy-Item -LiteralPath (Join-Path $crate 'CREDITS.md') -Destination $out
    }
}
$archiveFiles = @('chart_requester.dll','chart_request_static','README.md') | ForEach-Object { Join-Path $destination $_ }
$archive = Join-Path $destination "chart-requester-$($project.version).zip"
# Registry archives can carry Unix-epoch timestamps; ZIP starts at 1980.
Get-ChildItem -LiteralPath $licenseRoot -Recurse -File | Where-Object { $_.LastWriteTime.Year -lt 1980 } | ForEach-Object {
    $_.LastWriteTime = [datetime]'1980-01-01T00:00:00'
}
Compress-Archive -LiteralPath $archiveFiles -DestinationPath $archive -Force
if ($env:GITHUB_OUTPUT) {
    "archive=$archive" | Out-File -FilePath $env:GITHUB_OUTPUT -Encoding utf8 -Append
}
Write-Output "Packaged in $archive"
