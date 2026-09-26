$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot
& (Join-Path $PSScriptRoot 'build.ps1')
$destination = Join-Path $root 'dist'
New-Item -ItemType Directory -Force -Path $destination | Out-Null
foreach ($file in @('README.md','LICENSE','THIRD-PARTY-NOTICES.md','chart-requester.example.toml')) {
    Copy-Item -LiteralPath (Join-Path $root $file) -Destination $destination
}
Copy-Item -LiteralPath (Join-Path $root 'target\release\chart_requester.dll') -Destination $destination
New-Item -ItemType Directory -Force -Path (Join-Path $destination 'docs') | Out-Null
Copy-Item -Path (Join-Path $root 'docs\*.md') -Destination (Join-Path $destination 'docs')
$cargo = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
Push-Location $root
try {
    $metadata = (& $cargo metadata --locked --offline --format-version 1 | ConvertFrom-Json)
    if ($LASTEXITCODE -ne 0) { throw 'Cannot collect dependency license metadata' }
} finally { Pop-Location }
$licenseRoot = Join-Path $destination 'licenses'
New-Item -ItemType Directory -Force -Path $licenseRoot | Out-Null
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
}
$archiveFiles = @('chart_requester.dll','README.md','LICENSE','THIRD-PARTY-NOTICES.md','chart-requester.example.toml','docs','licenses') | ForEach-Object { Join-Path $destination $_ }
$archive = Join-Path $destination 'chart-requester-0.1.0.zip'
# Registry archives can carry Unix-epoch timestamps; ZIP starts at 1980.
Get-ChildItem -LiteralPath $licenseRoot -Recurse -File | Where-Object { $_.LastWriteTime.Year -lt 1980 } | ForEach-Object {
    $_.LastWriteTime = [datetime]'1980-01-01T00:00:00'
}
Compress-Archive -LiteralPath $archiveFiles -DestinationPath $archive -Force
Write-Output "Packaged in $archive"
