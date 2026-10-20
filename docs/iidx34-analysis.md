# IIDX 34 / 20260917 binary analysis and adapter evidence

Analysis date: 2026-10-20. Target supplied by the user:
`C:\Users\BeatMania IIDX LM\Downloads\bm2dx.dll`.
The build label **20260917** is user-supplied; the native music database validator
independently requires major version **34**.

The later [audio-offset and signature investigation](iidx34-audio-offset-analysis.md)
records the file-integrity checks and native synchronization findings.
The [monitor-adjust follow-up](iidx34-monitor-adjust-analysis.md) compares the
offset calculation, defaults and calibration getter with IIDX 33.

| Property | Value |
|---|---|
| SHA-256 | `3755587f44785effe5008f74cfe7dccbdf423ad09feead48e00a90f78a42fe83` |
| File size | 18,200,216 bytes |
| Architecture / preferred image base | PE32+ AMD64 / `0x180000000` |
| Image size | `0xc5ff000` |
| Implementation | `crates/bilimani-iidx/src/versions/v34/b20260917/` |
| Output | `bilimani_iidx.dll`, also retaining the 33/20260812 adapter |

All addresses below are **RVAs**, not file offsets or absolute addresses.
Object field offsets are decimal unless prefixed by `0x`. IDA/Hex-Rays analysis,
PE exception-function boundaries, MSVC RTTI, cross-references, and instruction
comparison against the [33 evidence](game-analysis.md) were used. Matching
functions supplied candidates; callers, consumers and structure accesses were
checked before accepting a candidate. No game binary was patched on disk.

This document records each analysis area so later work can start from its
evidence and known limitations. The full game binary and IDA database are local
analysis inputs, not repository or release artifacts. The adapter has offline
validation; **actual IIDX 34 gameplay has not been run for this change**.

## Reproduce the file checks

From the repository root, with Python 3:

```powershell
py scripts/verify-iidx34.py 'C:\Users\BeatMania IIDX LM\Downloads\bm2dx.dll'
```

This stdlib-only, read-only script checks the exact SHA-256, PE architecture,
every literal 16-byte native guard in this profile, database/input entry bytes,
scene RTTI and lifecycle pointers, dictionary loader and input vtable slot.
It does not execute the game. The Rust source is the maintained guard manifest;
the script compares it directly with the file, independently of runtime fixtures.
A prefix match establishes identity at a guarded entry, not proof that an
arbitrary patched function remains semantically compatible.

## 1. Scene lifecycle, native selection, and menu input

| RVA | Evidence / use |
|---|---|
| `0xd52508` | RTTI `CMusicSelectScene` vtable |
| `0x8d7b70` / `0x8d7f60` / `0x8d8250` | initialize / cleanup / update, slots **16/17/18** |
| `0x8da060` → `0x8d8920` | normal interactive state member and its selection update |
| `0x94fcb0` / `0x8da0f0` | base scene/controller updates; establish state/timer/controller fields |
| `0x5fa9e0` / `0x5fc6f0` | selection widget update / native reservation consumer |
| `0x5fa200` / `0x5fa370` / `0x5fa3d0` | selected bar type / record / difficulty getters |
| `0x7bf560` / `0x7bf330` / `0x7bf5d0` | reservation singleton / availability / setter |
| `0x81a4f0` → `0x92fe10` | current layout/mode thunk and implementation |
| `0x92fb10` / `0x92fcc0` | active player / joined-side query |
| `0x92fc80` / `0x92fd10` | SP/DP chart style / per-side difficulty |
| `0x7f0860` | modal/input-blocked query |
| `0xd9ff20`, slot 3 → `0xa67d70` | input poll vtable and original entry |

The extra base-scene virtual methods move lifecycle slots from 13/14/15 in 33
to **16/17/18** in 34. Old slots are real but unrelated functions; a range check
alone would accept the wrong callback. The adapter checks each expected target
before installing hooks.

Selection scene fields:

| Field | IIDX 33 | IIDX 34 |
|---|---:|---:|
| state | 80 | **88** |
| timer | 128 | **136** |
| controller pointer | 144 | **152** |
| selection widget | 408 | **416** |

The controller still contains its member-function pointer at +8, adjustment at
+16, pending transition at +56. State 3, a positive timer, the normal member target,
and no pending transition/modal gate native submission. The widget's internal
list moved, but the adapter calls the verified getters instead of reading it.

Reservation layout remains enabled +8, consumed +9, music ID +16, mode +20,
difficulty +24. The availability query receives an optional difficulty with
bit 32 for presence; the setter receives `(this, music_id, mode, difficulty)`
with -1 meaning unspecified. Submission and acknowledgement
remain in the selection callback with scene epoch checks. No HTTP handler calls
game functions or reads game pointers.

Participation flags are two u32 values at `0xb60b3b0`. The context begins at
`0xb60b3a0`, chart style is +4, difficulties +8/+12, participation +16/+20.
Two-player SP has a double layout but **SP chart IDs**; use the style getter for
metadata, not just the layout thunk. Participation sampling also clears stale
SP/DP mode after logout when callbacks stop.

Input poll begins `48 89 4c 24 08 55 53 56 57 41 54 41 55 41 56 41`.
Button words remain +8/+12/+16/+20; scratch fields +0x58/+0x5c, stride 8.
The existing native-entry/MinHook-relay inspection is retained with this build's
entry RVA. The wrapper calls the original entry so an accepted external hook
chain still runs. Actual coexistence with installed game plugins needs live
validation.

## 2. Stage families and actual playing record

The RTTI table establishes the following lifecycle tables:

| RVA | Class |
|---|---|
| `0xd73238` | `CDanStageScene` |
| `0xd73338` | `CExpertStageScene` |
| `0xd73438` | `CFreeStageScene` |
| `0xd73538` | `CHazardStageScene` |
| `0xd73638` | **`CStepUpStageScene`** |
| `0xd73738` | `CPremiumFreeStageScene` |
| `0xd73838` | `CStandardStageScene` |
| `0xd73938` | `CTutorialStageScene` |
| `0xd7b470` | `CFreePlusStageScene` |
| `0xd7b570` | `CHellStageScene` |
| `0xd7b670` | `CArenaStageScene` |
| `0xd7b870` | `CBPLBattleStageScene` |
| `0xd7bb48` | `CFactoryCheckStageScene` |

Slots 16/17/18 normally point to `0x91a2c0` / `0x91a330` / `0x91a390`.
FactoryCheck uses `0x8bea40` / `0x91a330` / `0x8beac0`.
The StepUp entry replaces the old Life family in this set; it was identified
from RTTI rather than assigning the old label by table position.

`0x8fa8a0` returns stage context `0xb4e0080`; the actual stage record is +8,
written by preparation at `0x8fa220`. Initialize calls `0x8f8f40` to prepare
the stage. Capture after the original callback and clear after cleanup;
do not carry the last selection record into a different stage.
Stage initialize/cleanup target checks are mandatory. Stage update checks gate
optional lane/playback samplers; an incompatible update disables those displays.

## 3. Catalog, search dictionary, and song metadata

| RVA | Evidence |
|---|---|
| `0x939100` | native database accessor: `48 8d 05 69 37 cd 0a c3`, returns `0xb60c870` |
| `0x939d50` | database loading; native static capacity is 5 MiB |
| `0x939e90` / `0x939e30` | version-34 database validation / record validation |
| `0x938f70` / `0x938fb0` | record index getter (2040 stride) / canonical ID lookup |
| `0x628180` / `0x627fd0` | BPM access / display consumers |
| `0x7e88c0`, `0x7e77d0`, `0x81d510` | radar copy, ordering and display |
| `0x819780` / `0x819f30` | note count / introduction version |
| `0xcbe910`, slot 1 → `0x7dc590` | title dictionary loader |

Database header: `IIDX`, u32 version **34**, record count, ID-table length,
then u32 ID→record indices and `0x7f8`-byte records. Canonical ID at record
+0x67c must map back to that record; historical aliases in the lookup table are
not extra songs. Version 33 databases are not accepted by the 34 parser.

The live record layout is unchanged:

| Offset | Field |
|---|---|
| `0x000`, 256 bytes | UTF-16 title |
| `0x100`, 64 bytes | Shift-JIS reading |
| `0x140`, 128 bytes / `0x1c0`, 256 bytes | UTF-16 genre / artist |
| `0x3dc`, u16 | introduction version, bounded by **34** |
| `0x3ec`, ten bytes | chart levels, 0 absent, max 12 |
| `0x3fc + 8*i` / `0x400 + 8*i` | maximum / minimum BPM |
| `0x47c + 4*i` | note count |
| `0x4fc + 24*i` | six radar values |
| `0x67c`, u32 | canonical music ID |

Chart indices 0..4 are SP B/N/H/A/L; 5..9 are DP B/N/H/A/L.
The pure song decoder is shared with 33 with an explicit version bound; native
memory access and profile selection are separate. Missing BPM/notes/radar are
not synthesized.

The normalized dictionary loader matches the earlier loader. Its ordered-tree
layout also remains: object +8 head, +16 count; node links +0/+8/+16, sentinel
byte +25, MSVC string +32 (length +48, capacity +56), music item pointer +64,
item music ID +8. The pure bounded tree decoder is shared with 33. Dictionary
data is copied after native loading, then consumed as owned search terms.
The accessor accepts the reviewed LEA form and equivalent MOV pointer-indirection
form; compatibility with a particular installed Omnifix build is not established
by this offline analysis.

## 4. Density analyzer: changed ownership and chart indexing

This area is **not an RVA-only port**. The old ten-chart/background-thread request
ABI no longer applies. Do not write the old pending-song field or use its AVS
mutex protocol in 34.

| RVA | Evidence |
|---|---|
| `0x6220c0` | analyzer constructor; object is 72 bytes |
| `0x622540` | loads only the two selected charts |
| `0x622b60` / `0x622cb0` | note accumulation / duration and vector finalization |
| `0x622de0` | asynchronous job enqueuer |
| `0x6222a0` | job lambda; constructs and publishes analyzer |
| `0x623210` | publication into model +144/+152 |
| `0x8718d0` → pointer at `0xb3e9fa0` | selection model singleton |
| `0xad71d0` / `0xad71e0` | acquire/release global shared_ptr lock, bit 0 at `0xc51b324` |
| `0xc8b2d8` | analyzer vtable |
| `0x63f3b0` / `0x640010` | consumer / lane-count widgets |

Native jobs are already enqueued by the selection widget (`0x5fac4b`,
`0x5fac62`) and stage preparation (`0x8f9d73`). The plugin samples their published
result; it does not submit duplicate jobs.

Analyzer layout: vtable +0, song ID +8, **explicit chart indices +12/+16**,
optional shared_ptr entries +24/+48 (24-byte stride, presence byte entry+16).
Index 11 is the native absent-chart sentinel; reject all indices ≥10.
These entries identify charts, not necessarily SP side 1 / DP side 2.
The shared allocation has a refcount header; the analyzer is the contained object,
not the allocation base.

Detail allocation is 136 bytes. The used first 116 bytes retain two vector
headers at +0/+24, eight lane counts per side at +48/+80, duration_ms +112.
The note callback preserves native long-note weighting. Histogram bin width is
1000 ms; scratch is a subset of total notes. Vectors and duration are bounded,
scratch cannot exceed total, and lane totals must agree with a valid histogram.
Invalid density and independently valid lane counts can fail separately.

The scene callback tries the native publication lock once; if busy, it skips.
While held, it copies bounded immutable analyzer/vector data, then releases the
lock. The cache contains owned arrays only. Repeated publications for the same
song merge by chart index, so changing difficulty can populate another chart;
changing song invalidates the cache. Unanalyzed charts legitimately remain null.
Verified stage updates also resample publication: a course/special stage's job
may finish after initialization returns.
This still requires live checks of job timing, difficulty changes, and stage
transitions. Offline lock/callback fixtures do not establish game scheduling.

## 5. Lane mapping and option stride

| RVA | Evidence |
|---|---|
| `0x80f930` → `0xb125990` | lane transform singleton, output table +16 (`0xb1259a0`) |
| `0x80fb60` | RANDOM generator |
| `0x80efa0` / `0x80ebd0` | per-note consumer / reflected transform |
| `0x871910` → pointer `0xb3e9f28` | options singleton, vtable `0xd23de0` |
| `0x8845f0` / `0x884620` | option block selector / accessor |
| `0x884680` / `0x882ea0` | RANDOM / MIRROR getters |
| `0x884b60` | battle reflection getter |
| `0x884c70` / `0x81a960` | S-RANDOM / special mode predicates |
| `0x86f860` → pointer `0xb3ea090`, `0x65af80` | beginner assist state / consumer |

Option block stride changes **180 → 188**; the full sampled region is
8 + 3*188 = **572 bytes**. DP uses block 2, SP its physical side.
Random is block+48+4*side, mirror +56+4*side, battle reflection
**+152 (was +156)**. These offsets come from the native getters.

Table values map source→destination; display needs the inverse, with scratch
fixed at index 7. Ordinary MIRROR is baked into the generated table. Special
mode 7/8 and battle reflections happen after it. S-RANDOM is dynamic; beginner
assist can also transform notes and must not be displayed as a fixed permutation.
Never call the consuming lookup to obtain a table: it mutates RNG/state.
A patched generator is tolerated because the plugin only reads its validated
output; changed layout/consumer guards disable lane sampling.

The byte at `0xb3e7e2e` is set during normal sound loading (`0x8faeb0`) and
transitions (`0x8fb290`), so it is not an invalid-lane flag.

## 6. Playback clock

`0x810ff0` returns sequencer `0xb1281c0`. The current frame is +0x2a29f8;
captured FPS is 24 bytes later. `0x810ac0` loads/resets, `0x811000` advances,
`0x8151d0` converts frames, and `0x815d40` returns timeline `0xb3e3b90`
whose end frame is +96. Existing bounded conversion and pause/retry semantics
are retained. Sampling runs after verified stage updates, never on a wall-clock
estimate. Clock/layout guard failure leaves the rest of the plugin available.

## 7. Player identity and weekly statistics

| Item | IIDX 34 value |
|---|---|
| profile root / initialization flags | `0x107c0f0` / `0x107c0e8`, +4 |
| per-side stride | **`0x3e66db8`** |
| player profile base / ready flag | `0x328c068` / `0x32b34d8` |
| session identity block | `0x70f2e00`, 28 bytes; kind +24 |
| activity singleton pointer | `0xb3ea128` |
| week IDs / dates / counts, relative to root | `0x2233e54` / `0x2233e68` / `0x2233e7c` |
| activity ready / week ready, relative to root | `0x223614c` / `0x2234354` |

Name remains player+0x20, eight Shift-JIS bytes. DJ points are
**+0x34/+0x38**, grades +0x3c/+0x40. `0x597660` emits several numeric fields:
its earlier +0x2c/+0x30 outputs are **not** the displayed DJ POINT fields.
The UI consumer `0x8b2800` takes outputs 5/6 derived from +0x34/+0x38.
Grade getter is `0x597a20`; validation/session evidence is `0x5b6770`,
`0x59ec80`, `0x59ebc0`.

Weekly evidence: `0x596710` accesses IDs/dates, `0x596830` songs,
`0x5967a0` keys, `0x5968c0` scratch. Each week block is 124 bytes; style stride
620. Consumers `0x6e0060` / `0x6dffa0` pass index **4** for the game's current
week bucket. This is the native bucket, not a rolling seven-day calculation.
Current-session delta begins activity+128+244*side: songs +4, keys +116,
with a 120-byte stride between SP/DP counters; `0x650730` / `0x6506e0`
establish these counters.

Sample identity before and after copying to reject a card/session change.
Private identity bytes are validation tokens only and are neither logged nor
published. Missing/guest/invalid data stays absent rather than becoming zero.
Profile guard mismatch disables player telemetry independently.

## Validation and remaining live checks

Completed offline validation on 2026-10-20:

| Check | Result |
|---|---|
| Workspace Rust tests, all targets | Passed |
| Workspace Clippy, all targets, warnings denied | Passed |
| Workspace release build | Passed |
| TypeScript typecheck and 29 tests | Passed |
| Production Card, Mecha and DDR page builds | Passed |
| Read-only file verification | Passed: 47 prefixes, 14 scene tables, ancillary slots |
| IIDX 34 `--queue --user` smoke | Passed, including same-song analyzer merge and late stage publication |
| IIDX 34 `--bad-core`, `--bad-scene`, `--reject`, `--occupied-port` | Passed: startup failure handled |
| IIDX 34 `--song-info` with each of `--bad-lane-layout`, `--bad-lane-update`, `--bad-playback`, `--lane-generator-patch` | Passed: expected optional-feature behavior |
| IIDX 34 `--bad-user` | Passed: player telemetry disabled |
| IIDX 33 `--queue --user` and `--reject` regression | Passed |
| Real-game execution | **Not performed** |

Tested artifact: `target/release/bilimani_iidx.dll`, SHA-256
`f3cbd82e249ae0913ba5d541049b643fa9bda6cb083db9bd449515618bf32c30`.
This identifies this local build, not a promise of reproducible compiler output.

Relevant commands (Windows MSVC toolchain, Node dependencies installed):

```powershell
./scripts/build.ps1 -CargoArgs @('test', '--workspace', '--all-targets', '--locked')
./scripts/build.ps1 -CargoArgs @('clippy', '--workspace', '--all-targets', '--locked', '--', '-D', 'warnings')
./scripts/build.ps1
npm run typecheck
npm test
npm run build
py scripts/verify-iidx34.py 'C:\Users\BeatMania IIDX LM\Downloads\bm2dx.dll'
py scripts/smoke-iidx34.py --game-dll 'C:\Users\BeatMania IIDX LM\Downloads\bm2dx.dll' --queue --user
```

`smoke-iidx34.py` maps the supplied DLL without imports/entrypoint execution,
loads the production plugin, then invokes its actual hooks with synthetic
objects and stubbed original functions. It checks actual HTTP snapshots,
queue acknowledgements, alias rollback, input forwarding, density ownership,
lane/playback data, stage cleanup, profile sampling, and SDK shutdown.
Extra modes: `--bad-core`, `--bad-scene`, `--reject`, `--occupied-port`,
`--song-info --bad-lane-layout`, `--song-info --bad-lane-update`,
`--song-info --bad-playback`, `--song-info --lane-generator-patch`, `--bad-user`.
Each run uses a separate process and ignored `analysis/smoke-iidx34-*` directory.
The 33 regression uses `scripts/smoke-dll.py --queue --user`.

Rust tests cover profile resolution/API metadata, version-34 catalog validation,
shared record decoding, explicit analyzer chart indices, sentinels/stale data,
option stride and post-reflections, plus existing queue/input/player behavior.
See the implementation tests for bounded-read and malformed-data cases.

Before declaring real-game validation, check: live catalog/dictionary loading;
SP left/right, two-player SP, DP; normal reservation and difficulty changes;
menu gesture/capture and queue actions; every relevant stage family;
start/retry/end/logout; density publication across difficulty changes; lane
layouts with installed RANDOM plugins; chart clock; profiles/weekly totals;
OBS reconnect and actual platform traffic. These have not been inferred from
passing offline fixtures.
