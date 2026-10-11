$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath((Split-Path $PSScriptRoot))
Push-Location $root
try {
    & npm.cmd ci
    if ($LASTEXITCODE -ne 0) { throw 'npm ci failed' }
    & npm.cmd run build
    if ($LASTEXITCODE -ne 0) { throw 'Frontend build failed' }
    & (Join-Path $PSScriptRoot 'build.ps1')
    $stage = [IO.Path]::GetFullPath((Join-Path $root 'dist\package'))
    if ($stage -ne "$root\dist\package") { throw 'Unexpected staging path' }
    # Only this generated directory is replaced. Never delete user installations.
    foreach ($path in @((Join-Path $root 'dist'), $stage)) {
        if ((Test-Path -LiteralPath $path) -and ((Get-Item -LiteralPath $path).Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Staging paths must not be reparse points' }
    }
    if (Test-Path -LiteralPath $stage) { Remove-Item -LiteralPath $stage -Recurse -Force }
    New-Item -ItemType Directory -Path $stage | Out-Null
    foreach ($name in @('bilimani_iidx.json', 'bilimani_iidx_aliases.csv')) {
        Copy-Item -LiteralPath (Join-Path $root "config\$name") -Destination $stage
    }
    Copy-Item -LiteralPath (Join-Path $root 'target\release\bilimani_iidx.dll') -Destination $stage
    $static = Join-Path $stage 'bilimani_web'
    New-Item -ItemType Directory -Path $static | Out-Null
    Copy-Item -LiteralPath (Join-Path $root 'dist\web\index.html') -Destination $static
    foreach ($style in @('card', 'mecha')) {
        $source = Join-Path $root "dist\web\iidx\$style"
        $destination = Join-Path $static "iidx\$style"
        New-Item -ItemType Directory -Path $destination -Force | Out-Null
        # Take page assets from the build, but configuration only from public defaults.
        foreach ($name in @('index.html', 'assets')) {
            Copy-Item -LiteralPath (Join-Path $source $name) -Destination $destination -Recurse
        }
        Copy-Item -LiteralPath (Join-Path $root 'web\apps\iidx\config.json') -Destination $destination
    }
    $metadata = (& cargo metadata --locked --offline --no-deps --format-version 1 | ConvertFrom-Json)
    if ($LASTEXITCODE -ne 0) { throw 'Cannot collect workspace metadata' }
    $project = $metadata.packages | Where-Object { $_.name -eq 'bilimani-iidx' }
    if (-not $project) { throw 'Cannot find IIDX package version' }
    $archive = Join-Path $root "dist\bilimani-$($project.version).zip"
    $files = @(Get-ChildItem -LiteralPath $stage | ForEach-Object { $_.FullName })
    Compress-Archive -LiteralPath $files -DestinationPath $archive -Force
    if ($env:GITHUB_OUTPUT) { "archive=$archive" | Out-File -FilePath $env:GITHUB_OUTPUT -Encoding utf8 -Append }
    Write-Output "Packaged in $archive"
} finally { Pop-Location }
