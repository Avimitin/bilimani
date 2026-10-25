# Regression fixtures for the offline summarizer; does not run PresentMon.
$ErrorActionPreference = 'Stop'
$testRoot = Join-Path $PSScriptRoot ("../analysis/frame-pacing-test-" + [guid]::NewGuid().ToString('N'))
$null = New-Item -ItemType Directory -Path $testRoot
$summarizer = Join-Path $PSScriptRoot 'summarize-iidx-frametimes.ps1'
$fixture = Join-Path $testRoot 'two-streams.csv'
$rows = foreach ($chain in @('0x1','0x2')) {
    for ($i=0; $i -lt 100; $i++) {
        $interval = if ($chain -eq '0x1') { '8.333' } else { '16.667' }
        [pscustomobject]@{
            Application='spice64.exe'; ProcessID=42; SwapChainAddress=$chain
            TimeInSeconds=($i * .1).ToString([Globalization.CultureInfo]::InvariantCulture)
            msBetweenPresents=$interval
            # Main has one dropped frame and one long display interval. The
            # dropped row's bogus display interval must not enter its histogram.
            Dropped=if ($chain -eq '0x1' -and $i -eq 40) { 1 } else { 0 }
            msBetweenDisplayChange=if ($chain -eq '0x1' -and $i -eq 40) { '999' } elseif ($chain -eq '0x1' -and $i -eq 50) { '25.000' } else { $interval }
            msInPresentAPI='0.500'; msUntilRenderComplete='1.000'; msUntilDisplayed='2.000'
            PresentMode=if ($chain -eq '0x1' -and $i -ge 50) { 'Composed: Flip' } else { 'Hardware: Independent Flip' }
            SyncInterval=1
        }
    }
}
$rows | Export-Csv -LiteralPath $fixture -NoTypeInformation -Encoding UTF8
$report = (& $summarizer -CsvPath $fixture) -join "`n"
$main = ($report -split 'spice64.exe, 42, 0x1:')[1] -split 'spice64.exe, 42, 0x2:' | Select-Object -First 1
$sub = ($report -split 'spice64.exe, 42, 0x2:')[1]
foreach ($expected in @('60 retained / 100 rows','p50=8.333','max=25.000 ms','Dropped presents: 1/60','PresentMode transitions: 1','msGPUActive: unavailable', 'Displayed intervals >12.500 ms: 1/59', 't=5s present=8.333ms display=25.000ms')) {
    if (-not $main.Contains($expected)) { throw "Main stream missing '$expected': $report" }
}
if (-not $sub.Contains('p50=16.667') -or -not $sub.Contains('PresentMode transitions: 0')) { throw 'Subscreen was merged or incorrectly counted.' }
if ($main.Contains('max=999.000')) { throw 'Dropped display data entered histogram.' }

$missing = Join-Path $testRoot 'missing-display.csv'
$rows | Select-Object Application,ProcessID,SwapChainAddress,TimeInSeconds,msBetweenPresents |
    Export-Csv -LiteralPath $missing -NoTypeInformation -Encoding UTF8
$noDisplay = (& $summarizer -CsvPath $missing) -join "`n"
if (-not $noDisplay.Contains('Displayed interval: unavailable (not zero)')) { throw 'Missing display data was not identified.' }
$invalid = Join-Path $testRoot 'wrong-schema.csv'
'Application,CPUFrameTime', 'spice64.exe,8.333' | Set-Content -LiteralPath $invalid -Encoding UTF8
$rejected = $false
try { $null = & $summarizer -CsvPath $invalid } catch { $rejected = $true }
if (-not $rejected) { throw 'Wrong CSV schema was accepted.' }
'PASS: separate 120/60 Hz streams, trimmed boundaries, display stalls, dropped frames, mode transitions, missing data and wrong schema.'
