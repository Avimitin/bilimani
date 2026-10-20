# IIDX 34 / 20260917 monitor-adjust follow-up

Date: 2026-10-20. Read-only analysis of the same supplied binary as
[the adapter](iidx34-analysis.md) and [signature/audio investigation](iidx34-audio-offset-analysis.md).
No binary, configuration or live setting was changed.

## Result

The inspected monitor-adjust calculation has not changed relative to the local
IIDX 33 / 20260812 reference: its entire 54-instruction sequence matches after
normalizing relocated code targets and RIP-relative references. The divisor,
arithmetic bytes, getter/setter and initial value also match.

This establishes the behavior of these paths in the two supplied files. It does
not establish the running game's current adjustment, or prove that every
monitor-detection path and table is identical across versions.
The signed-file integrity evidence independently argues against a post-signing
monitor-offset edit to the supplied 34 file.

## Addresses and data flow

All addresses are RVAs. Object field offsets are decimal.

| Role | IIDX 33 | IIDX 34 |
|---|---|---|
| Timing adjustment conversion | `0x8285b0` | `0x814ab0` |
| `CLocalInfo` adjust getter, reads +16 | `0x5d4aa0` | `0x5c6c90` |
| Adjust setter, writes +16 | `0x5d4b90` | `0x5c6d80` |
| Local-info reset | `0x5d4bd0` | `0x5c6dc0` |
| Reset instruction supplying raw value 10 | `0x5d4bf0` | `0x5c6de0` |
| Calibration result getter | `0x946b10` | `0x92d3f0` |

In 34, the local-info constructor `0x5c6610` identifies the object as
`CLocalInfo`, then calls reset. The settings singleton getter `0x5c0c30`
returns `0xb0ea630`; local info is singleton+584, adjust is another +16.
The hidden system-info display at `0x980520` uses this getter for `%d Adjust`.

Reset supplies **10 as an internal integer**, not a stated 10 ms delay.
The loaded-data importer `0x5c6e70` can replace it from input structure +8.
Monitor-check completion at `0x91af40` runs the monitor-data processing and
writes the result of `0x92d3f0` through the same setter.
That result getter returns **0 for definition types 2 and 3**; other types use
a monitor-dependent table. The return-zero branches match the 33 reference.
The non-type-2/3 table contents were not compared in this follow-up.

Consequently, observing the reset value in the DLL does not establish the
post-boot value. Saved data, monitor-check execution and in-memory hooks are
outside this file-only observation.

## Calculation and units

At `0x814ab0`, define:

- `M`: local-info adjust if the runtime predicate `0xa3d1e0` is true, otherwise 10.
- `P`: per-player/style timing option read through `0x882ca0`
  (options block +108).
- `F`: runtime FPS from `0xa3d1f0`.
- `C`: 200 for definition type 3, -80 for type 2, otherwise 0.

The returned signed integer is:

```text
Q = trunc((C + 5 * (P - (2*M + 20))) * F / 60)
```

The constants, operations and 60.0f divisor match 33. The arithmetic span at
34 RVA `0x814b4f` is byte-identical to 33 RVA `0x82864f`.

`Q` is not milliseconds. Note/timeline constructors including `0x807150`,
`0x807590`, `0x807b50` and `0x807be0` pass it to `0x815690`.
That consumer divides it by 100 for timeline-index displacement and uses the
signed remainder for interpolation between adjacent entries, with boundary
clamping. Avoid interpreting the raw constants 10, 20 or 200 as added audio
delay, or assigning an absolute wall-clock offset without the timeline context.

This is separate from the native audio-synchronization thresholds documented in
the audio investigation. The `bilimani_iidx.dll` adapter does not write this
adjust field or replace these timing functions.

## Reproduce the comparison

Dependencies: Python, `pefile`, `capstone`. The existing local analysis environment
contains them:

```powershell
analysis/iidx34-tools/Scripts/python.exe scripts/compare-iidx-monitor-adjust.py `
  analysis/bm2dx.dll 'C:\Users\BeatMania IIDX LM\Downloads\bm2dx.dll'
```

The script checks both full-file hashes, disassembles the complete conversion,
compares the normalized sequence, verifies the getter/setter and default bytes,
checks the actual referenced divisor, and compares calibration-result branches.
It does not execute either game. The old reference hash is
`c61b6dcb8894062e56d60da8ca90053b27f129e1a8e8da5e54457aa42602397d`;
the new hash is
`3755587f44785effe5008f74cfe7dccbdf423ad09feead48e00a90f78a42fe83`.
