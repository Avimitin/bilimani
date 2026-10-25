# PresentMon 2.3.1 console --v1_metrics CSV. No external dependencies.
[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$CsvPath,
    [ValidateRange(1, 1000)][double]$TargetHz = 120,
    [ValidateRange(0, 120)][double]$TrimSeconds = 2
)
$ErrorActionPreference = 'Stop'
$rows = @(Import-Csv -LiteralPath $CsvPath)
if ($rows.Count -eq 0) { throw 'No frame rows; check the process ID and PresentMon console output.' }
$fields = $rows[0].PSObject.Properties.Name
foreach ($required in @('Application', 'ProcessID', 'SwapChainAddress', 'TimeInSeconds', 'msBetweenPresents')) {
    if ($fields -notcontains $required) { throw "Missing $required. Capture with PresentMon console --v1_metrics." }
}
function Number($value) {
    $parsed = 0.0
    if ([double]::TryParse([string]$value, [Globalization.NumberStyles]::Float,
        [Globalization.CultureInfo]::InvariantCulture, [ref]$parsed) -and -not [double]::IsNaN($parsed) -and -not [double]::IsInfinity($parsed)) { return $parsed }
    return $null
}
function Stats([string]$Name, $Values) {
    $sorted = @($Values | Where-Object { $null -ne $_ -and $_ -ge 0 } | Sort-Object)
    if ($sorted.Count -eq 0) { return "  ${Name}: unavailable (not zero)" }
    $mean = ($sorted | Measure-Object -Average).Average
    $p50 = $sorted[[Math]::Max(0, [int][Math]::Ceiling($sorted.Count * .50) - 1)]
    $p95 = $sorted[[Math]::Max(0, [int][Math]::Ceiling($sorted.Count * .95) - 1)]
    $p99 = $sorted[[Math]::Max(0, [int][Math]::Ceiling($sorted.Count * .99) - 1)]
    return ('  {0}: n={1} mean={2:F3} p50={3:F3} p95={4:F3} p99={5:F3} max={6:F3} ms' -f $Name,$sorted.Count,$mean,$p50,$p95,$p99,$sorted[-1])
}
"Target reference: $TargetHz Hz ($('{0:F3}' -f (1000/$TargetHz)) ms); trim $TrimSeconds seconds at BOTH ends of each stream."
'Streams remain separate. Identify the main-screen stream before interpreting thresholds; 16.67 ms is normal for a 60 Hz screen.'
'Present API wait includes intentional VSync waiting. Display ETW values are not a physical measurement of the panel.'
foreach ($group in ($rows | Group-Object Application,ProcessID,SwapChainAddress)) {
    $timed = @($group.Group | Where-Object { $null -ne (Number $_.TimeInSeconds) } | Sort-Object { Number $_.TimeInSeconds })
    if ($timed.Count -eq 0) { continue }
    $first = Number $timed[0].TimeInSeconds
    $last = Number $timed[-1].TimeInSeconds
    $sample = @($timed | Where-Object { (Number $_.TimeInSeconds) -ge ($first+$TrimSeconds) -and (Number $_.TimeInSeconds) -le ($last-$TrimSeconds) })
    "`n$($group.Name): $($sample.Count) retained / $($timed.Count) rows"
    if ($sample.Count -lt 2) { '  Too little data after trimming.'; continue }
    $present = @($sample | ForEach-Object { Number $_.msBetweenPresents } | Where-Object { $null -ne $_ -and $_ -gt 0 })
    Stats 'Present interval' $present
    $display = @($sample | Where-Object { (Number $_.Dropped) -ne 1 } |
        ForEach-Object { Number $_.msBetweenDisplayChange } | Where-Object { $null -ne $_ -and $_ -gt 0 })
    Stats 'Displayed interval' $display
    foreach ($metric in @('msInPresentAPI','msUntilRenderComplete','msGPUActive','msUntilDisplayed')) {
        Stats $metric @($sample | Where-Object { $metric -ne 'msUntilDisplayed' -or (Number $_.Dropped) -ne 1 } | ForEach-Object { Number $_.$metric })
    }
    foreach ($metric in @(@('Present',$present), @('Displayed',$display))) {
        $values = @($metric[1])
        if ($values.Count -eq 0) { continue }
        $threshold = 1.5 * 1000 / $TargetHz
        $long = @($values | Where-Object { $_ -gt $threshold }).Count
        '  {0} intervals >{1:F3} ms: {2}/{3} ({4:F2}%); >20 ms: {5}' -f $metric[0],$threshold,$long,$values.Count,(100*$long/$values.Count),@($values | Where-Object { $_ -gt 20 }).Count
    }
    if ($fields -contains 'Dropped') {
        $known = @($sample | Where-Object { (Number $_.Dropped) -in @(0,1) })
        $dropped = @($known | Where-Object { (Number $_.Dropped) -eq 1 }).Count
        if ($known.Count) { '  Dropped presents: {0}/{1} ({2:F2}%)' -f $dropped,$known.Count,(100*$dropped/$known.Count) }
    }
    $transitions = 0
    for ($i=1; $i -lt $sample.Count; $i++) { if ($sample[$i].PresentMode -ne $sample[$i-1].PresentMode) { $transitions++ } }
    "  PresentMode transitions: $transitions"
    foreach ($field in @('PresentMode','SyncInterval')) {
        $counts = @($sample | Group-Object $field | ForEach-Object { "$($_.Name)=$($_.Count)" })
        "  ${field}: $($counts -join '; ')"
    }
    foreach ($gapMetric in @('msBetweenPresents', 'msBetweenDisplayChange')) {
        "  Largest $gapMetric gaps (time relative to capture):"
        $sample | Where-Object { $null -ne (Number $_.$gapMetric) -and (Number $_.$gapMetric) -gt 0 -and ($gapMetric -ne 'msBetweenDisplayChange' -or (Number $_.Dropped) -ne 1) } |
            Sort-Object { Number $_.$gapMetric } -Descending | Select-Object -First 8 |
            ForEach-Object { '    t={0}s present={1}ms display={2}ms inPresent={3}ms mode={4}' -f $_.TimeInSeconds,$_.msBetweenPresents,$_.msBetweenDisplayChange,$_.msInPresentAPI,$_.PresentMode }
    }
}
