# IIDX 34 monitor validation patch

Analysis date: 2026-10-20. This creates a spice2x JSON option for the supplied
DLL without changing the DLL file or enabling anything in the running game.

## Exact target and existing upstream option

| Property | Value |
|---|---|
| Supplied file | `C:\Users\BeatMania IIDX LM\Downloads\bm2dx.dll` |
| SHA-256 | `3755587f44785effe5008f74cfe7dccbdf423ad09feead48e00a90f78a42fe83` |
| PE timestamp | `0x6aa23d57` |
| Entry-point RVA | `0xb02b3c` |
| spice2x identifier | `LDJ-6aa23d57_b02b3c` |
| Build profile | IIDX 34 / 010-0916 |
| Upstream label for this PE identifier | 2026-09-16 (010) |

The identifier is taken from the actual binary. Upstream's September 17
entry has a different identifier, `LDJ-6aab51c7_b02b7c`; its offsets must not
be substituted merely because the date label matches.

For the newer 010-0917 profile (`LDJ-6aab51c7_b02b7c`), upstream already defines
**Bypass Lightning Monitor Error** at file offset 9544872: the same `0F85` →
`90E9` transformation, shifted by `+0x40`. This repository option is specific to
010-0916 and must not be applied to the newer build.

The examined [eamuse/sp2xpatcher source](https://github.com/eamuse/sp2xpatcher/tree/d407a55bc36d43b66aaf46aff8d93fad800ff1bb)
already contains **Bypass Lightning Monitor Error** for this file.
In its IIDX 33 definitions, **TDJ 60Hz - Disable Monitor Check** uses the
same branch transformation as the older monitor-error bypass. The transformation
itself does not select 60 Hz.

This custom option is named **Disable Monitor Check (TDJ validation)**.
It implements that existing bypass with a full six-byte instruction guard
and an explicit PE identifier. It is not a new frame-rate optimization.

## What the patch does

At RVA `0x91b068` (file offset `0x91a468`, decimal **9544808**):

```text
Original: 0F 85 40 01 00 00    jne 0x91b1ae
Enabled:  90 E9 40 01 00 00   nop; jmp 0x91b1ae
```

The immediately preceding comparison tests whether definition type is 3
(TDJ). Originally, TDJ falls through into validation while other definitions
jump to completion. The replacement makes completion unconditional here.
The original instruction and replacement sequence both end at `0x91b06e`,
so the preserved displacement reaches the same destination.

The monitor-check routine `0x91af40`:

1. Increments its update count and calculates measured FPS using `0x92bc30`.
2. Waits until count +160 reaches threshold +164. Reset `0x91b310` initializes
   that threshold to approximately `20 * selected_rate`, nominally 20 seconds.
3. At the patched branch, normally validates TDJ's measured FPS against narrow
   bounds, permits up to three retries, then handles the broader monitor error.
4. Completion at `0x91b1ae` retains normal bookkeeping. At `0x91b1dc`, it
   writes measured FPS to configuration +28, then processes monitor information,
   applies the normal adjust result and advances the scene.

Thus the option bypasses **validation, retries and the TDJ monitor error**.
It keeps the initial measurement interval and measured timing result.
It does not skip the wait, substitute a fixed 120 FPS timing value, change the
monitor-adjust formula, select 60 Hz, or ensure stable 120 Hz performance.
Skipping measurement entirely is a separate change and is not included.

Related findings: [monitor adjust](iidx34-monitor-adjust-analysis.md),
[input cadence](iidx34-input-cadence-analysis.md).

## JSON format and generated files

The [official JSON specification](https://github.com/spice2x/spice2x.github.io/wiki/patches.json-specification)
uses an array of patch objects. This option has `type: "memory"`, `gameCode`,
`peIdentifier`, and a `patches` array with `dllName`, integer **file** `offset`,
`dataDisabled` and `dataEnabled`. The offset is not an RVA.

The repository contains the minimal option at
[patches/iidx34/LDJ-6aa23d57_b02b3c.json](../patches/iidx34/LDJ-6aa23d57_b02b3c.json).
The optional header's SHA-256 records verification provenance; spice2x does
not enforce that header hash. Runtime matching uses the PE identifier and
the expected instruction bytes.

A ready-to-use combined file was generated at:

```text
C:\Users\BeatMania IIDX LM\Downloads\iidx34-monitor-check\LDJ-6aa23d57_b02b3c.json
```

It preserves the upstream file's 40 options, replacing the equivalent
**Bypass Lightning Monitor Error** entry with the custom name and full guard.
Other options are unchanged. The accompanying `LICENSE-sp2xpatcher.txt`
preserves the upstream MIT notice. The source is pinned to commit
`d407a55bc36d43b66aaf46aff8d93fad800ff1bb`.

To use the combined file, put it in `patches` beside the active `spicecfg.exe`
with its exact filename, preserving a backup of any existing same-name JSON.
Reopen the Patches tab and enable **Disable Monitor Check (TDJ validation)**.
Renaming the option changes its saved-setting identity, so an older enabled
monitor-error option should not be assumed to carry over.
Importing the upstream URL again may replace the custom JSON.
These placement rules follow the [patcher's local JSON instructions](https://github.com/eamuse/sp2xpatcher/blob/d407a55bc36d43b66aaf46aff8d93fad800ff1bb/README.md).

The minimal repository file contains only this option; use the combined file
to retain the other upstream choices.

## Reproduction and validation

Requires Python, `pefile`, `capstone`, and `unicorn`:

```powershell
analysis/iidx34-tools/Scripts/python.exe scripts/prepare-iidx34-monitor-check-patch.py `
  'C:\Users\BeatMania IIDX LM\Downloads\bm2dx.dll'
```

To generate a combined file in a new directory:

```powershell
analysis/iidx34-tools/Scripts/python.exe scripts/prepare-iidx34-monitor-check-patch.py `
  'C:\Users\BeatMania IIDX LM\Downloads\bm2dx.dll' `
  --base-json reference/sp2xpatcher/patches/LDJ-6aa23d57_b02b3c.json `
  --output analysis/monitor-check-export/LDJ-6aa23d57_b02b3c.json
```

The script refuses mismatched DLL hashes, PE identifiers, instruction bytes,
incompatible existing bypass definitions, and pre-existing output files.
It disassembles both sequences and executes them in an isolated CPU emulator:
original TDJ falls through, original non-TDJ jumps, and both patched cases
jump to the original completion target. All four cases passed.

It reads the DLL again after preparation to confirm its hash is unchanged.
The combined JSON was parsed and compared against upstream to confirm only
the intended option and provenance metadata changed. This is offline
instruction and file validation, not real-game validation.
