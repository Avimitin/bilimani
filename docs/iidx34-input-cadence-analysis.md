# IIDX 34 TDJ input cadence and responsiveness

Analysis date: 2026-10-20. Supplied IIDX 34 / 010-0916 DLL:
SHA-256 `3755587f44785effe5008f74cfe7dccbdf423ad09feead48e00a90f78a42fe83`.
This follows the [HID and native input-history analysis](iidx34-input-analysis.md).
All code addresses below are RVAs; object offsets are decimal.

## Conclusion

The regular gameplay loop refreshes I/O and updates the key-state masks once
per iteration, then performs game processing, rendering and frame pacing.
The TDJ native configuration selects a nominal 120 Hz rate. At that rate,
the intended input-update cadence is approximately **8.333 ms**.

This is code evidence for input processing tied to the game frame. It is not
a measurement or a guarantee that every two polls are at most 8.333 ms apart.
All keys are handled in the same update; the game does not spend 8.333 ms
processing each key in turn. Device scanning, USB reports and host HID
processing can run more frequently than the game consumes their result.

The rate comes from the selected display configuration, and runtime graphics
hooks, frame-rate patches, scheduling and long frames can change actual
intervals. No live game or controller timing was measured.

## Caller chain

The entry wrapper `0x934ff0` initializes the application, runs startup loop
`0x922830`, then enters regular main loop `0x923ed0`.

Within each regular iteration:

| Call site | Action |
|---|---|
| `0x92403c` | Get `IO::IODeviceManagerIIDX` through `0xa6a600` |
| `0x924047` | Dispatch manager vtable slot 3 to `0xa6a7a0` |
| `0x92404a` | Get `IO::InputManagerIIDX` through `0xa675d0` |
| `0x924055` | Dispatch input vtable slot 3 to `0xa67d70` |
| `0x9244b2` onward | Process the active game/scene |
| `0x9246b8` | Render through `0x9248b0` |
| `0x9247c0` | Call frame-pacing/presentation routine `0x95f4f0`, under the native `VSYNC` label |
| `0x924811` | Loop back to `0x923f35` if exit has not been requested |

There is one input-manager update in this main-loop body. The device manager
iterates its device collection and calls each device's slot 3.
For the TDJ implementation, vtable `0xd9bcc8` slot 3 is `0xa54980`;
that decoder refreshes its controller using `0xa52240`, which calls the
imported `GetDeviceStatus` at IAT RVA `0xc675d0`.
The next manager call converts the resulting per-key counters into the
game's pressed/held/released masks.

```text
regular main-loop iteration
  board status read and history decoding
  key-mask update
  game processing
  render
  frame pacing / presentation
next iteration
```

A bounded instruction search for calls to the input singleton getter followed
by slot-3 dispatch finds three sites: `0x92404a`, `0x92818c` and `0x9283fe`.
The last two belong to startup: `0x927fc0` performs initial I/O checks and
`0x9283e0` is called by startup loop `0x922830`. That startup loop also
calls `0x95f4f0` once per iteration. These extra calls must not be mistaken for
a separate high-frequency gameplay input thread.

The startup thread `0x925c50` → `0x926100` checks device readiness through
slot 8 and yields via an AVS call; it does not call the gameplay input poll.
The bounded call search is corroborating evidence, not a claim that a static
analysis can enumerate every possible indirect call after runtime hooks.

## Where 120 Hz and 8.333 ms come from

The configuration parser `0xa3d230` maps `TDJ-JA` and `TDJ-JB` to definition
type 3. Configuration setup `0xa3cf90` builds a type table:

| Definition type | Configured display count | Requested rate |
|---|---|---|
| 1 | 1 | 60 |
| 2 | 1 | 60 |
| 3, TDJ | 2 | 120 |
| 4 | 1 | 120 |

The type-3 entry is assembled at `0xa3d034`–`0xa3d04d`.
It is copied to configuration +16/+20 at `0xa3d132`/`0xa3d138`.
The configuration begins at application root +1992, so requested rate is
root +2012.

Graphics initialization `0x922b60` uses that rate with its display-mode
selection. The selected mode's refresh rate is written at `0x9234e6` to
root +2016, i.e. configuration +24.

Getter `0xa3d200` returns configuration +24 except for definition type 2,
where it returns 60. Consequently, TDJ is **not hardcoded to return 120
regardless of runtime display state**.

Pacing routine `0x95f4f0` calls this getter at `0x95f57b`. Its actual
floating-point instructions at `0x95f580`–`0x95f5c3` calculate:

```text
remaining_ms =
    1000.0 / selected_rate
    - elapsed_since_previous_primary_present_ms
    - 1.0
```

If positive, this value is truncated to integer milliseconds and passed to
`SleepEx` at `0x95f5db`. The routine then calls primary swap-chain presentation
at `0x95f608`; its reference performance-counter value is updated after
primary presentation at `0x95f87a`.

Fullscreen presentation parameters use the selected refresh rate and interval
1 (`0x923223` / `0x923229`). The field layout and interval interpretation agree
with Microsoft's [presentation-parameter definition](https://learn.microsoft.com/en-us/windows/win32/direct3d9/d3dpresent-parameters)
and [presentation-interval semantics](https://learn.microsoft.com/en-us/windows/win32/direct3d9/d3dpresent).
The remaining 1 ms in the software sleep formula is therefore not evidence
of a 1 kHz input loop: presentation is still part of the same frame cycle.

For selected rate 120, the nominal period is `1000 / 120 = 8.333333... ms`.
The sleep calculation, integer truncation and presentation do not create a
hard upper bound on host scheduling or input-observation intervals.

## What this means for responsiveness

For an ideal, evenly spaced poll with period `T`, an already-delivered input
waits from approximately zero to just under `T` for its next observation.
If arrival phase is uniformly distributed, the mean wait is `T/2`.

| Assumed game observation rate | Period | Mean phase wait under that assumption |
|---|---|---|
| 120 Hz | 8.333 ms | 4.167 ms |
| 1000 Hz | 1.000 ms | 0.500 ms |

These are mathematical examples, not measured end-to-end latency or
controller performance. An input arriving immediately before a poll and one
arriving immediately after it can be assigned to adjacent game updates
despite their small physical-time difference.

The inspected judgment path uses integer chart-frame differences. Moving
detection by one game update can therefore affect a grade near a boundary;
it does not imply that every press changes grade, or establish how large the
effect is on this user's scores. Physical responsiveness and conscious
perception cannot be concluded from the frame period alone.

The comparison with native board history remains narrower:

- Both status formats enter the same game update.
- Populated native-format history can rescue activity that ended before the
  snapshot was read.
- This decoder ORs history into key states; it does not use sample position
  to backdate the judgment.
- Thus the 8.333 ms frame cadence is not, by itself, an extra delay caused by
  spice2x's HID hook or evidence that BIO2 would remove that delay.

With a press that remains down until the next game observation, both paths
can detect it at that observation. A complete pulse between observations
is the demonstrated case where retained history can change detection.

## Reproducible verification

```powershell
analysis/iidx34-tools/Scripts/python.exe scripts/verify-iidx34-input-cadence.py `
  'C:\Users\BeatMania IIDX LM\Downloads\bm2dx.dll'
```

Dependencies: `pefile`, `capstone`, `unicorn`; verified with Unicorn 2.1.4.
The script checks the file hash, vtable targets, main-loop call order and
back edge, identified startup dispatch sites, configuration-table entry
and selected-rate store.

It also executes the actual native rate getter and pacing arithmetic in
an isolated emulator. Controlled cases confirm:

- Type 3 with selected rate 120 returns 120.
- Type 3 with selected rate 144 returns 144.
- Type 2 returns 60 even when the stored selected rate is 120.
- The pacing expression matches `1000/rate - elapsed_ms - 1` at synthetic
  120/60 Hz rates, including an already-late frame.

All checks passed. No Windows sleep, Direct3D call, hardware access or DLL
entrypoint is executed by the harness. It validates the interpretation of
the native instructions; a live timestamp trace at `0xa54980` and
`0xa67d70` is still required to establish the user's actual interval
distribution and maximum poll gap.
