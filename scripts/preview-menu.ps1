param(
    [switch]$Check,
    [string]$ConfigPath,
    [string]$DatabasePath,
    [ValidateSet('SP', 'DP')][string]$Mode = 'SP'
)
$ErrorActionPreference = 'Stop'
if ($Check) {
    & "$PSScriptRoot/build.ps1" -CargoArgs @('run', '--locked', '--example', 'menu_check')
} else {
    $previewArgs = @('run', '--locked', '--example', 'menu_preview', '--', '--mode', $Mode)
    # build.ps1 switches to the repository root; resolve user paths beforehand.
    if ($ConfigPath) { $previewArgs += @('--config', (Resolve-Path -LiteralPath $ConfigPath).Path) }
    if ($DatabasePath) { $previewArgs += @('--database', (Resolve-Path -LiteralPath $DatabasePath).Path) }
    & "$PSScriptRoot/build.ps1" -CargoArgs $previewArgs
}
