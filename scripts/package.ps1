$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath((Split-Path $PSScriptRoot))
Push-Location $root
try {
    & npm.cmd ci
    if ($LASTEXITCODE -ne 0) { throw 'npm ci failed' }
    & npm.cmd run build
    if ($LASTEXITCODE -ne 0) { throw 'Frontend build failed' }
    & (Join-Path $PSScriptRoot 'build.ps1')
    $metadata = (& cargo metadata --locked --offline --no-deps --format-version 1 | ConvertFrom-Json)
    if ($LASTEXITCODE -ne 0) { throw 'Cannot collect workspace metadata' }
    foreach ($game in @('iidx', 'ddr')) {
        $stage = [IO.Path]::GetFullPath((Join-Path $root "dist\package-$game"))
        if ($stage -ne "$root\dist\package-$game") { throw 'Unexpected staging path' }
        # Only this generated directory is replaced. Never delete user installations.
        foreach ($path in @((Join-Path $root 'dist'), $stage)) {
            if ((Test-Path -LiteralPath $path) -and ((Get-Item -LiteralPath $path).Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Staging paths must not be reparse points' }
        }
        if (Test-Path -LiteralPath $stage) { Remove-Item -LiteralPath $stage -Recurse -Force }
        New-Item -ItemType Directory -Path $stage | Out-Null
        foreach ($name in @("bilimani_$game.json", "bilimani_${game}_aliases.csv")) {
            Copy-Item -LiteralPath (Join-Path $root "config\$name") -Destination $stage
        }
        Copy-Item -LiteralPath (Join-Path $root "target\release\bilimani_$game.dll") -Destination $stage
        $static = Join-Path $stage 'bilimani_web'
        New-Item -ItemType Directory -Path $static | Out-Null
        if ($game -eq 'iidx') {
            Copy-Item -LiteralPath (Join-Path $root 'dist\web\index.html') -Destination $static
            $styles = @('card', 'mecha')
        } else {
            $styles = @('street')
        }
        foreach ($style in $styles) {
            $source = Join-Path $root "dist\web\$game\$style"
            $destination = Join-Path $static "$game\$style"
            New-Item -ItemType Directory -Path $destination -Force | Out-Null
            # Take page assets from the build, but configuration only from public defaults.
            foreach ($name in @('index.html', 'assets')) {
                Copy-Item -LiteralPath (Join-Path $source $name) -Destination $destination -Recurse
            }
            Copy-Item -LiteralPath (Join-Path $root "web\apps\$game\config.json") -Destination $destination
        }
        $project = $metadata.packages | Where-Object { $_.name -eq "bilimani-$game" }
        if (-not $project) { throw "Cannot find $game package version" }
        $archive = Join-Path $root "dist\bilimani-$game-$($project.version).zip"
        $files = @(Get-ChildItem -LiteralPath $stage | ForEach-Object { $_.FullName })
        Compress-Archive -LiteralPath $files -DestinationPath $archive -Force
        if ($env:GITHUB_OUTPUT) { "${game}_archive=$archive" | Out-File -FilePath $env:GITHUB_OUTPUT -Encoding utf8 -Append }
        Write-Output "Packaged in $archive"
    }
} finally { Pop-Location }
