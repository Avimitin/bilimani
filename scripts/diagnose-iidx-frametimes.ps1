# Windows PowerShell 5.1+, x64. Read-only inventory and optional PresentMon trace.
# See docs/iidx-frame-pacing.md. No driver, registry, timer or game settings change.
[CmdletBinding()]
param(
    [ValidatePattern('^[A-Za-z0-9_-]+$')][string]$Label = 'baseline',
    [string]$OutputRoot = (Join-Path $PSScriptRoot '../analysis/frame-pacing'),
    [string]$PresentMonPath,
    [int]$TargetProcessId = 0,
    [ValidateRange(5, 300)][int]$Seconds = 60,
    [ValidateRange(0, 120)][int]$DelaySeconds = 10
)
$ErrorActionPreference = 'Stop'
if (-not [Environment]::Is64BitProcess) { throw 'Run 64-bit PowerShell.' }
$run = Join-Path $OutputRoot ("{0}-{1}-{2}" -f (Get-Date -Format 'yyyyMMdd-HHmmss'), $Label, [guid]::NewGuid().ToString('N').Substring(0, 6))
$null = New-Item -ItemType Directory -Path $run -Force
$run = (Resolve-Path -LiteralPath $run).Path
$issues = [Collections.Generic.List[string]]::new()
function Save-Section([string]$Name, [scriptblock]$Read) {
    try { ConvertTo-Json -InputObject (& $Read) -Depth 8 | Set-Content -LiteralPath (Join-Path $run "$Name.json") -Encoding UTF8 }
    catch { $issues.Add("${Name}: $($_.Exception.Message)") }
}

# Native CCD layouts: paths=72, mode union=64 bytes. Query active paths only;
# do not round 60000/1001 to 60 or confuse desktop pixels with signal pixels.
if (-not ('BilimaniDisplayProbe' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Runtime.InteropServices;
public static class BilimaniDisplayProbe {
    [StructLayout(LayoutKind.Sequential)] public struct Luid { public uint Low; public int High; }
    [StructLayout(LayoutKind.Sequential)] public struct Rational { public uint Numerator, Denominator; }
    [StructLayout(LayoutKind.Sequential)] public struct Region { public uint Width, Height; }
    [StructLayout(LayoutKind.Sequential)] public struct Source { public Luid Adapter; public uint Id, Mode, Flags; }
    [StructLayout(LayoutKind.Sequential)] public struct Target {
        public Luid Adapter; public uint Id, Mode, Technology, Rotation, Scaling;
        public Rational Refresh; public uint Scanline; public int Available; public uint Flags;
    }
    [StructLayout(LayoutKind.Sequential)] public struct Path { public Source Source; public Target Target; public uint Flags; }
    [StructLayout(LayoutKind.Sequential)] public struct Signal {
        public ulong PixelRate; public Rational HSync, VSync; public Region Active, Total;
        public uint Standard, Scanline;
    }
    [StructLayout(LayoutKind.Sequential)] public struct SourceMode { public uint Width, Height, Format; public int X, Y; }
    [StructLayout(LayoutKind.Explicit, Size=64)] public struct Mode {
        [FieldOffset(0)] public uint Type; [FieldOffset(4)] public uint Id;
        [FieldOffset(8)] public Luid Adapter; [FieldOffset(16)] public Signal Signal;
        [FieldOffset(16)] public SourceMode Source;
    }
    [StructLayout(LayoutKind.Sequential)] public struct Header { public uint Type, Size; public Luid Adapter; public uint Id; }
    [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)] public struct SourceName {
        public Header Header; [MarshalAs(UnmanagedType.ByValTStr, SizeConst=32)] public string Name;
    }
    [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)] public struct TargetName {
        public Header Header; public uint Flags, Technology; public ushort Manufacturer, Product; public uint Connector;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst=64)] public string Name;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst=128)] public string DevicePath;
    }
    [DllImport("user32.dll")] static extern int GetDisplayConfigBufferSizes(uint flags, out uint paths, out uint modes);
    [DllImport("user32.dll")] static extern int QueryDisplayConfig(uint flags, ref uint count, [Out] Path[] paths, ref uint modeCount, [Out] Mode[] modes, IntPtr topology);
    [DllImport("user32.dll", EntryPoint="DisplayConfigGetDeviceInfo")] static extern int GetSourceName(ref SourceName name);
    [DllImport("user32.dll", EntryPoint="DisplayConfigGetDeviceInfo")] static extern int GetTargetName(ref TargetName name);
    static string Adapter(Luid id) { return String.Format("{0:X8}:{1:X8}", id.High, id.Low); }
    static double? Hz(Rational r) { return r.Denominator == 0 ? (double?)null : (double)r.Numerator/r.Denominator; }
    public static object[] Read() {
        if (Marshal.SizeOf(typeof(Path)) != 72 || Marshal.SizeOf(typeof(Mode)) != 64)
            throw new Exception("Unexpected CCD ABI sizes");
        for (int retry=0; retry<5; retry++) {
            uint pc, mc; int error=GetDisplayConfigBufferSizes(2, out pc, out mc);
            if (error != 0) throw new Win32Exception(error);
            Path[] paths=new Path[pc]; Mode[] modes=new Mode[mc];
            error=QueryDisplayConfig(2, ref pc, paths, ref mc, modes, IntPtr.Zero);
            if (error == 122) continue;
            if (error != 0) throw new Win32Exception(error);
            var result=new List<object>();
            for (int i=0; i<pc; i++) {
                Path p=paths[i];
                var sn=new SourceName { Header=new Header { Type=1, Size=(uint)Marshal.SizeOf(typeof(SourceName)), Adapter=p.Source.Adapter, Id=p.Source.Id } };
                var tn=new TargetName { Header=new Header { Type=2, Size=(uint)Marshal.SizeOf(typeof(TargetName)), Adapter=p.Target.Adapter, Id=p.Target.Id } };
                int se=GetSourceName(ref sn), te=GetTargetName(ref tn);
                SourceMode? sm=p.Source.Mode<mc && modes[p.Source.Mode].Type==1 ? modes[p.Source.Mode].Source : (SourceMode?)null;
                Signal? signal=p.Target.Mode<mc && modes[p.Target.Mode].Type==2 ? modes[p.Target.Mode].Signal : (Signal?)null;
                result.Add(new {
                    Display=se==0 ? sn.Name : null, Monitor=te==0 ? tn.Name : null,
                    SourceAdapter=Adapter(p.Source.Adapter), TargetAdapter=Adapter(p.Target.Adapter),
                    SourceId=p.Source.Id, TargetId=p.Target.Id, OutputTechnology=p.Target.Technology,
                    DesktopWidth=sm.HasValue ? (uint?)sm.Value.Width : null,
                    DesktopHeight=sm.HasValue ? (uint?)sm.Value.Height : null,
                    SignalWidth=signal.HasValue ? (uint?)signal.Value.Active.Width : null,
                    SignalHeight=signal.HasValue ? (uint?)signal.Value.Active.Height : null,
                    PathHz=Hz(p.Target.Refresh), PathRate=p.Target.Refresh,
                    SignalHz=signal.HasValue ? Hz(signal.Value.VSync) : null,
                    SignalRate=signal.HasValue ? (Rational?)signal.Value.VSync : null,
                    ScanlineOrdering=signal.HasValue ? signal.Value.Scanline : p.Target.Scanline
                });
            }
            return result.ToArray();
        }
        throw new Exception("Display topology kept changing; repeat while stable");
    }
}
'@
}

Write-Host "Collecting inventory before capture: $run"
Save-Section 'displays' { [BilimaniDisplayProbe]::Read() }
Save-Section 'windows' {
    Get-ItemProperty 'HKLM:/SOFTWARE/Microsoft/Windows NT/CurrentVersion' |
        Select-Object ProductName, DisplayVersion, CurrentBuild, UBR, BuildLabEx
}
Save-Section 'updates' { Get-HotFix | Sort-Object InstalledOn -Descending | Select-Object -First 20 HotFixID, InstalledOn, Description }
Save-Section 'hardware' {
    [ordered]@{
        CPU = @(Get-CimInstance Win32_Processor | Select-Object Name, NumberOfCores, NumberOfLogicalProcessors, MaxClockSpeed)
        GPU = @(Get-CimInstance Win32_VideoController | Select-Object Name, DriverVersion, DriverDate, VideoProcessor)
        MemoryBytes = (Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory
    }
}
Save-Section 'settings' {
    $settings = @(
        @('HKLM:/SYSTEM/CurrentControlSet/Control/GraphicsDrivers', 'HwSchMode'),
        @('HKLM:/SOFTWARE/Microsoft/Windows/Dwm', 'OverlayTestMode'),
        @('HKCU:/System/GameConfigStore', 'GameDVR_Enabled'),
        @('HKCU:/SOFTWARE/Microsoft/Windows/CurrentVersion/GameDVR', 'AppCaptureEnabled'),
        @('HKCU:/SOFTWARE/Microsoft/GameBar', 'AutoGameModeEnabled')
    )
    foreach ($setting in $settings) {
        $value = Get-ItemProperty -LiteralPath $setting[0] -Name $setting[1] -ErrorAction SilentlyContinue
        [pscustomobject]@{ Key=$setting[0]; Name=$setting[1]; Value=if ($null -eq $value) { $null } else { $value.($setting[1]) } }
    }
}
Save-Section 'processes' { Get-Process | Select-Object ProcessName, Id, CPU, WorkingSet64 }
Save-Section 'display-events' {
    Get-WinEvent -FilterHashtable @{LogName='System'; StartTime=(Get-Date).AddDays(-3)} -MaxEvents 5000 |
        Where-Object ProviderName -match '^(Display|nvlddmkm|Microsoft-Windows-WHEA-Logger)$' |
        Select-Object -First 50 TimeCreated, ProviderName, Id, LevelDisplayName, Message
}
try { & powercfg.exe /getactivescheme | Set-Content -LiteralPath (Join-Path $run 'power-plan.txt') -Encoding UTF8 }
catch { $issues.Add("power-plan: $($_.Exception.Message)") }

$smi = Get-Command nvidia-smi.exe -ErrorAction SilentlyContinue
if ($smi) {
    try {
        & $smi.Source '--query-gpu=name,driver_version,pstate,clocks.current.graphics,clocks.current.memory,utilization.gpu,temperature.gpu,power.draw' '--format=csv' 2>&1 |
            Out-File -LiteralPath (Join-Path $run 'nvidia-before.csv') -Encoding UTF8
    } catch { $issues.Add("nvidia-smi: $($_.Exception.Message)") }
}

if ($TargetProcessId -eq 0) {
    $games = @(Get-Process -Name spice64,spice -ErrorAction SilentlyContinue)
    if ($games.Count -eq 1) { $TargetProcessId = $games[0].Id }
}
if ($TargetProcessId -gt 0) {
    Save-Section 'game-modules' {
        (Get-Process -Id $TargetProcessId).Modules | ForEach-Object {
            $hash = if ($_.ModuleName -match '^(bm2dx|bilimani_iidx)\.dll$|^spice(64)?\.exe$') { (Get-FileHash -LiteralPath $_.FileName -Algorithm SHA256).Hash } else { $null }
            [pscustomobject]@{ Name=$_.ModuleName; Version=$_.FileVersionInfo.FileVersion; SHA256=$hash }
        }
    }
}
[ordered]@{ Label=$Label; UTC=[DateTime]::UtcNow.ToString('o'); ProcessId=$TargetProcessId; Seconds=$Seconds;
    Note='Inventory is before capture. Registry null means unset/unknown, not disabled. No command lines or configuration secrets collected.' } |
    ConvertTo-Json | Set-Content -LiteralPath (Join-Path $run 'run.json') -Encoding UTF8
($issues -join [Environment]::NewLine) | Set-Content -LiteralPath (Join-Path $run 'collection-issues.txt') -Encoding UTF8

if (-not $PresentMonPath) {
    Write-Host 'Inventory complete. Supply -PresentMonPath and -TargetProcessId for a frame trace.'
    Write-Host $run
    return
}
if ($TargetProcessId -le 0) { throw 'No unique spice process. Use Get-Process spice* and pass -TargetProcessId explicitly.' }
$null = Get-Process -Id $TargetProcessId
$presentMon = (Resolve-Path -LiteralPath $PresentMonPath).Path
[ordered]@{ File=(Split-Path -Leaf $presentMon); SHA256=(Get-FileHash -LiteralPath $presentMon -Algorithm SHA256).Hash } |
    ConvertTo-Json | Set-Content -LiteralPath (Join-Path $run 'presentmon-tool.json') -Encoding UTF8
$csv = Join-Path $run 'frames.csv'
$captureArgs = @('--process_id', "$TargetProcessId", '--output_file', $csv,
    '--v1_metrics', '--no_console_stats', '--no_track_input',
    '--session_name', "bilimani-$([guid]::NewGuid().ToString('N'))",
    '--delay', "$DelaySeconds", '--timed', "$Seconds", '--terminate_after_timed')
$captureArgs | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $run 'presentmon-arguments.json') -Encoding UTF8
Write-Host "Return focus to the game now. Capture starts in $DelaySeconds seconds and lasts $Seconds seconds."
# Foreground console invocation preserves arguments containing spaces. No injected
# overlay, process priority changes or registry modifications. Use an elevated
# console for ETW permissions; the script does not elevate or stop other sessions.
$ErrorActionPreference = 'Continue'
& $presentMon @captureArgs 2>&1 | Tee-Object -FilePath (Join-Path $run 'presentmon-console.txt')
$ErrorActionPreference = 'Stop'
if ($LASTEXITCODE -ne 0) { throw "PresentMon failed ($LASTEXITCODE). See presentmon-console.txt; use the documented console version and ETW permissions." }
if (-not (Test-Path -LiteralPath $csv)) { throw 'PresentMon produced no frames.csv.' }
$summaryScript = Join-Path $PSScriptRoot 'summarize-iidx-frametimes.ps1'
if (Test-Path -LiteralPath $summaryScript) {
    & $summaryScript -CsvPath $csv | Tee-Object -FilePath (Join-Path $run 'summary.txt')
}
Write-Host "Done: $run"
