# Diagnosing IIDX dual-display frame pacing

This guide concerns a 120 Hz main display and a roughly 60 Hz native IIDX touch
display. A reported 120 FPS does not establish evenly spaced displayed frames.
The main display's nominal interval is 8.333 ms; the touch display's is 16.667 ms.
Removing the touch display changes presentation, synchronization and possibly
device topology as well as workload. It does not by itself prove GPU overload.

The first suspects are a changed graphics-driver profile, fullscreen/compositor
behavior, or native secondary-swap-chain pacing after the Windows/driver update.
These are hypotheses until measured. The repository's
[IIDX34 cadence analysis](iidx34-input-cadence-analysis.md) establishes a native
main-loop presentation/pacing path, not the timing of the current installation.

## A/B checklist

Use the same chart, options, camera/streaming state and capture duration. Prefer
three runs of each condition; change one variable, restart the game when changing
display topology or graphics settings, and record the setting with each label.
Keep the measurement method identical in both runs.

1. **Record the baseline first.** Capture both screens attached, then main-only
   with the same main-screen resolution/rate. Stop the game before changing
   topology; select a mode the main display actually supports. Check Windows
   Advanced Display for both desktop and active-signal modes. The script records
   rational refresh rates, so 120/60 can be distinguished from 119.88/59.94.
   A fractional-rate mismatch is evidence to investigate, not proof of the bug.
   Both video cables should normally connect to the GTX 1660, not one to an
   integrated GPU or USB display adapter. Do not force the touch panel to 120 Hz.
2. **Separate the native touch rendering from monitor attachment.** With both
   monitors attached, use a supported spice2x option to disable the native
   subscreen for one run, leaving Windows displaying a static desktop there.
   Record the exact option/version rather than assuming an option's name.
   If that fixes it, investigate the native secondary present path. If a blank
   second desktop still triggers it, the Windows/driver display path becomes
   more likely. If disconnecting the monitor also removed its USB touch device,
   separately test USB disconnection while retaining the video connection.
3. **Check the NVIDIA profile for the actual game host executable**, normally
   `spice64.exe`, including a renamed executable. Record the current settings
   before changes. A useful controlled baseline is VSync application-controlled,
   Max Frame Rate off, fixed refresh/G-SYNC off for this application, Low Latency
   Mode off, and Prefer maximum performance for this application. Set the main
   display's intended 120 Hz mode explicitly. Compare each change independently.
   Check spice2x startup logs too: some versions write NVIDIA profile settings
   on launch, so a manually changed setting may not be the effective setting.
   Do not globally disable VSync or stack NVIDIA/RTSS/game frame limiters.
4. **Compare fullscreen paths.** If the installation supports both, compare its
   normal fullscreen mode against windowed/borderless, then separately test the
   executable's Disable fullscreen optimizations compatibility setting. Record
   PresentMon's `PresentMode`, not just the game's menu label. Restore the
   original setting if it does not help. Do not treat a composed mode as
   automatically broken: the measured cadence is what matters.
5. **Remove optional hooks and captures for one run.** Close OBS, browser video,
   NVIDIA recording/overlay, RTSS and remote streaming if present. Separately
   test without optional game plugins, including `bilimani_iidx.dll`, using the
   normal loader configuration while the game is stopped. The latest plugin
   changes must be tested as a possible regression too. Keep a baseline with
   the same tools running so closing an overlay does not get confused with
   changing the monitor configuration.
6. **Compare the previously smooth NVIDIA driver.** Use the known-good installer
   from NVIDIA, reboot, and restore the recorded per-application settings before
   comparing. Do this separately from changing Windows updates. Do not start
   with broad registry tweaks, DDU, timer/HPET changes, CPU affinity or realtime
   priority. If driver rollback fixes the A/B comparison, retain both captures
   and exact driver/Windows builds.
7. **Then test scheduling/composition settings individually.** If HAGS is
   available, compare on/off with a reboot between runs. Record Game Mode and
   background capture settings. Investigate MPO only if the presentation trace
   points toward composition and earlier checks fail; preserve/restore the prior
   state and use vendor guidance applicable to the actual OS/driver. NVIDIA's
   current MPO article is specifically titled for Windows 11 and is not proof
   that a Windows 10/GTX 1660 issue is MPO-related.

## Read-only collection

Copy these two scripts into the same folder on the **IIDX machine**:

- `scripts/diagnose-iidx-frametimes.ps1`
- `scripts/summarize-iidx-frametimes.ps1`

Download the official signed
[PresentMon 2.3.1 x64 console executable](https://github.com/GameTechDev/PresentMon/releases/download/v2.3.1/PresentMon-2.3.1-x64.exe)
into that folder. This pins the documented CLI and CSV schema; it is not a
claim that this is the latest release. These scripts require only 64-bit Windows
PowerShell 5.1+; no Python, Node or installation into the game is needed.

Start an **Administrator PowerShell** on the game machine, change to that folder,
and run this after the game has started:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\diagnose-iidx-frametimes.ps1 -Label dual -PresentMonPath .\PresentMon-2.3.1-x64.exe -Seconds 60 -DelaySeconds 15
```

The script first collects inventory, then announces the delay before recording.
Return focus to the game and keep the trace within active gameplay. Avoid
loading/results/alt-tab transitions during the measured interval. If the
gameplay excerpt is shorter, use `-Seconds 30`. For the second configuration,
restart the game, then run the same command with `-Label main-only`.

One running `spice.exe` or `spice64.exe` is selected automatically. If the host
has another name or multiple copies are running, find its PID in Task Manager
or `Get-Process spice*` and append `-TargetProcessId 1234` with the real PID.
Do not pass the PID of a launcher that has already exited. ETW capture needs
administrator or Performance Log Users permission; the script does not elevate
itself or change group membership.

For inventory only, omit `-PresentMonPath`. The collector changes no game,
driver, registry, power-plan or timer settings; it writes reports and optionally
starts a uniquely named, timed ETW session. It does not stop other trace sessions.
Collection and process/module enumeration occur **before** the trace, with a
delay to let the game resume. Even ETW has overhead: confirm the symptom exists
with and without measurement, and keep capture conditions identical.

Output defaults to `../analysis/frame-pacing/<timestamp>-<label>-<id>/`, relative
to the scripts. `-OutputRoot D:\FrameTests` chooses another directory. Files:

| File | Purpose |
|---|---|
| `displays.json` | Active paths, desktop/signal resolution, rational refresh rates, adapter IDs; scanline 1 means progressive |
| `windows.json`, `updates.json`, `hardware.json` | OS build/UBR, recent hotfix inventory, CPU/GPU and driver details |
| `settings.json`, `power-plan.txt` | Selected scheduling/capture/composition registry values and power plan; null means unset/unknown, not disabled |
| `game-modules.json` | Loaded module names/versions; SHA-256 for game, bilimani plugin and spice host when readable |
| `display-events.json` | Recent Display, NVIDIA and WHEA events within the last 5,000 System events/three days; absence does not rule out stalls |
| `nvidia-before.csv` | Optional NVIDIA clocks/utilization/temperature snapshot before capture; not a continuous performance trace |
| `frames.csv`, `summary.txt` | Raw per-frame trace and per-swap-chain summary |
| `presentmon-console.txt`, `collection-issues.txt` | Capture errors, ETW loss warnings and incomplete inventory; inspect these before drawing conclusions |

The report excludes process command lines, game configuration, card IDs and
platform credentials. Process/module names and system event messages are still
machine information; review reports before sharing publicly. Do not run inventory
through Remote Desktop if it changes the active display path being investigated.

## Reading the result

The summarizer uses `--v1_metrics` output, trims two seconds from each end of
each stream and groups by **application + process ID + swap-chain address**.
Do not add both screen streams together to compute an FPS figure. Swap-chain
addresses can change on restart and do not directly identify a physical display;
use cadence, the main-only comparison, or an ETW/GPUView trace to identify it.
If mapping remains ambiguous, do not assign a monitor based on row count alone.

| Evidence on the main-screen stream | Next investigation |
|---|---|
| Presents stay near 8.33 ms, but displayed intervals spike to 16.67/25 ms | Driver/compositor/presentation scheduling or dropped frames; compare modes and dual/main-only |
| Present intervals also spike | Application pacing, CPU scheduling, blocking presentation, optional hooks, GPU backpressure or driver stalls; not necessarily CPU saturation |
| Large `msInPresentAPI` changes accompany stutter | Synchronization/backpressure; VSync waiting can be normal, so compare against the smooth baseline |
| `msGPUActive` rises and frames miss budget | GPU work may contribute; confirm with utilization/clocks and a GPU trace, because overlapping frames and multiple engines complicate this metric |
| PresentMode changes align with bad frames | Overlays, focus changes or transitions between presentation paths are candidates |
| Present and display cadence are both regular, notes still judder | Game animation/timebase, monitor behavior, or measurement visibility; ETW is not a camera/scanout measurement |

The `>12.5 ms` threshold is only a useful 120 Hz screening threshold, not a
physical missed-refresh counter. It will flag every ordinary 60 Hz subscreen
frame. Run the summarizer with `-TargetHz 60` when inspecting that stream.
Missing display/GPU metrics remain unavailable; do not read them as zero work.
`msUntilRenderComplete` measures latency after Present, **not GPU execution time**.
An isolated large maximum may be a transition; note the trace-relative times
of perceived stutters and compare several repetitions, not only an average FPS.

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\summarize-iidx-frametimes.ps1 -CsvPath D:\FrameTests\your-run\frames.csv
```

If the frame trace shows application/scheduler stalls, collect a short WPR trace
as a second step. Run `wpr -status` first and do not cancel somebody else's
recording. In an elevated console, use `wpr -profiles` to check available profiles,
then `wpr -start GeneralProfile`, reproduce for roughly 20–30 seconds, and
`wpr -stop D:\FrameTests\iidx-stutter.etl`. Analyze CPU Usage/CPU Usage Precise,
DPC/ISR, disk/page faults and GPU activity (when captured) in Windows Performance
Analyzer; add the available GPU Activity profile through WPR UI if more graphics
detail is needed. Check whether driver/DPC events coincide with the actual bad
frames. A high isolated DPC maximum or a named NVIDIA module alone does not
establish causation. Run this separately from the baseline PresentMon comparison.

## Sources and validation

- [PresentMon console 2.3.1 options](https://github.com/GameTechDev/PresentMon/blob/v2.3.1/README-ConsoleApplication.md)
- [PresentMon v1 CSV metric definitions](https://github.com/GameTechDev/PresentMon/blob/v1.9.2/README.md#csv-columns)
- [Microsoft QueryDisplayConfig](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-querydisplayconfig)
- [NVIDIA per-application 3D settings](https://www.nvidia.com/content/Control-Panel-Help/vLatest/en-us/mergedProjects/3D%20Settings/Manage_3D_Settings_%28reference%29.htm)
- [Microsoft WPR commands](https://learn.microsoft.com/en-us/windows-hardware/test/wpt/wpr-command-line-options)
- [NVIDIA MPO guidance, currently Windows 11-specific](https://nvidia.custhelp.com/app/answers/detail/a_id/5157)

Local validation: the inventory collector ran on a Windows workstation and
returned its actual 3840×2160/120 Hz display path. The pinned PresentMon binary
has a valid Intel signature; its help matches the arguments. ETW capture in this
non-elevated development shell was denied, and the script reported that failure.
The offline summarizer is checked by `scripts/test-iidx-frametimes.ps1` using
separate 120/60 Hz streams, a dropped frame, a display stall, presentation-mode
changes and missing/invalid data. No trace of the affected IIDX machine has
been collected by this work, and no cause is declared proven.
