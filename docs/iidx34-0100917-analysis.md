# IIDX 34 / 010-0917 binary analysis and adapter evidence

Analysis date: 2026-10-22. Target supplied by the user:
`C:\Users\BeatMania IIDX LM\Downloads\010-0917bm2dx.dll`
(same bytes as `010-0917bm2dx (1).dll`).
This is a distinct exact-build profile from
[IIDX 34 / 010-0916](iidx34-analysis.md), not support inferred from the version
label.

The [song-decision follow-up](iidx34-0100917-decision-analysis.md) records the
confirmed-song animation interval used by Mecha's turntable, its optional guards,
and the supplied `010-20260916.dll` filename/hash mismatch.

| Property | Value |
|---|---|
| SHA-256 | `c98e183ec81ecb9e1cb13c237f9c9cb0b3f65b6fdaff6fd5e6a526008c546534` |
| File size | 18,200,216 bytes |
| Architecture / preferred image base | PE32+ AMD64 / `0x180000000` |
| Image size | `0xc5ff000` |
| PE timestamp / entry RVA | 2026-09-17 02:34:47 UTC / `0xb02b7c` |
| sp2xpatcher identity | `LDJ-6aab51c7_b02b7c`, version **2026-09-17 (010)** |
| Implementation | `crates/bilimani-iidx/src/versions/v34/b010_0917/` |
| API build string | `010-0917` |

The earlier profile `3755587f...fe83` is sp2xpatcher
`LDJ-6aa23d57_b02b3c`, version **2026-09-16 (010)**, PE timestamp
2026-09-10. Both images have the same image size and 57,024 exception-table
regions. Data-section virtual addresses are unchanged; code after `0x5afa9b`
is shifted by `+0x20`, `+0x30` or `+0x40` because five regions outside this
adapter's hook set changed size. The five changed regions are:

| Old RVA | New RVA | IDA evidence |
|---|---|---|
| `0x5afa9b` | `0x5afa9b` | cold block of `sub_1805af8e0`, a boot/state machine |
| `0x5c9f00` | `0x5c9f20` | `dlbg` debug-layer close helper |
| `0x5c9f80` | `0x5c9fa0` | `dlbg` debug-layer creation |
| `0x5ca0c0` | `0x5ca0f0` | debug-layer open helper |
| `0x94fb10` | `0x94fb50` | common scene helper called from selection/result initialization; exact role not proven |

The other 57 code RVAs used by the adapter were matched by exception-table
function index and compared instruction-by-instruction after masking rel32 and
RIP-relative displacements. Their bodies match the earlier build; guard bytes
were regenerated from the new image. This is static equivalence evidence, not a
claim that the changed common scene helper cannot affect runtime behavior.

## Scene lifecycle, selection and input

| RVA | Evidence |
|---|---|
| `0xd52508` | `.?AVCMusicSelectScene@@` vtable; slots 16/17/18 = `0x8d7bb0` / `0x8d7fa0` / `0x8d8290` |
| `0x8da0a0` | normal interactive state member target stored at controller +8 |
| `0x8d7bb0` | selection initialize |
| `0x8d7fa0` | selection cleanup |
| `0x8d8290` | selection update |
| `0x5fa240` / `0x5fa3b0` / `0x5fa410` | selected-bar / record / difficulty getters |
| `0x7bf5a0` / `0x7bf370` / `0x7bf610` | reservation singleton / availability / setter |
| `0x81a530` | current layout/mode thunk; jumps to `0x92fe50` |
| `0x92fb50` / `0x92fd00` | active player / joined-side query |
| `0x92fcc0` / `0x92fd50` | SP/DP chart style / per-side difficulty |
| `0x7f08a0` | modal/input-blocked query |
| `0xd9ff20`, slot 3 → `0xa67db0` | input poll vtable and original entry |

Selection/controller/participation field offsets are unchanged from the
010-0916 profile: state +88, timer +136, controller +152, selection widget
+416, controller member pointer +8, participation flags at `0xb60b3b0`.
The input prologue is the same 16 bytes; the existing MinHook-relay recognizer
is retained.

## Stage families and telemetry

| RVA | Evidence |
|---|---|
| `0xd73238` … `0xd7bb48` | 13 stage RTTI/vtables, addresses unchanged |
| `0x91a300` | normal stage initialize |
| `0x91a370` | stage cleanup |
| `0x91a3d0` | stage update |
| `0x8bea80` / `0x8beb00` | `CFactoryCheckStageScene` initialize / update |
| `0x8fa8e0` | stage-context getter, returns `0xb4e0080` |
| `0x8fa28a` / `0x8f9180` | stage song write / metadata consumer, both context **+0x18** |

The stage initialize/cleanup/update target checks in `native.rs` use the new
RVAs above. Optional lane/playback samplers remain gated by their own entry
guards; a mismatch disables only that display area.

## Gameplay song correction (2026-10-24)

The installed `modules/bm2dx.dll` supplied on the user's IIDX34 share was copied
to ignored `analysis/iidx34-gameplay/bm2dx.dll`. Its SHA-256 is the exact
`c98e183ec81ecb9e1cb13c237f9c9cb0b3f65b6fdaff6fd5e6a526008c546534`
listed above. IDA autoanalysis and Hex-Rays were run on that copy; no game
binary was modified. The local database is `bm2dx.dll.i64` alongside it.

The previous adapter incorrectly read `0xb4e0088` (context +8) for the song.
The correct field is `0xb4e0098` (context **+0x18**):

| RVA | ABI evidence |
|---|---|
| `0x8fa260` | Preparation calls `0x92fcd0` for the selected record; `0x8fa28f` stores it with `mov [rbp+0x18], rax`. The sequencer loader `0x810b00` receives that same field. |
| `0x91a300` | Stage initialization gets the context through `0x8fa8e0`, then calls `0x8f8f80`; the plugin samples after this original callback returns. |
| `0x8f9180` | Initialization reads `mov rdx, [r15+0x18]` for the song metadata consumer `0x8ecd80`. Other consumers use the same field for chart lookup and music ID +0x67c. |
| `0x8f9d04` | Initialization stores the contained `MusicDetailDataJobEnqueuer` object at context +8, with its shared ownership allocation at +0x10. Hex-Rays identifies the allocation's RTTI/vtable; `0x8f9daf` loads +8 to call job enqueue `0x622e20`. This is not a song record. |

Runtime guards now verify the record write and consumer instruction windows,
in addition to the context getter and lifecycle tables. The matching 010-0916
instructions are 0x40 bytes earlier and use the same fields, so both 34 adapters
are corrected. The 33 adapter is unchanged.

A read-only observation of the **old installed DLL** during real gameplay
returned `phase: playing`, `now_playing.song: null`, no chart players, a valid
playback clock, and one valid player profile from `/api/v1/iidx/user`.
Selection returned both a song and a player. Mecha's existing startup display
hides the whole bottom information panel when the song is absent, explaining
why both song and personal information disappeared. No player identity cache
or frontend workaround is needed.

The installed log also revealed a separate 010-0917 catalog failure. Its getter
at `0x939140` is `48 8d 05 29 37 cd 0a c3`, returning `0xb60c870`.
The adapter still compared the 010-0916 displacement beginning `69`, rejecting
even the unmodified getter. The check now accepts this build's correct LEA and
equivalent relocated-buffer MOV forms, and rejects the earlier displacement.

The updated smoke tests deliberately keep an unrelated object at context +8
and the actual song at +0x18. The 010-0917 test additionally exercises the
unaltered native catalog getter, all 13 stage families, repeated gameplay
updates with song and player data, cleanup, missing-song clearing and logout.
All scene originals it invokes are synthetic stubs installed after guard
verification. These are offline regressions, not real-game validation of the
fixed DLL. The corrected artifact still needs an in-game selection → play →
results/retry check, including Mecha's song/profile rotation.

## Catalog, dictionary, density, lane order and playback

| RVA | Evidence |
|---|---|
| `0x939140` | native database accessor; `lea rax, [rip+...]`, returns `0xb60c870` |
| `0x7dc5d0` | title dictionary loader; vtable `0xcbe910` slot 1 |
| `0x622100` / `0x622ba0` / `0x622cf0` | analyzer constructor / note accumulation / vector finalization |
| `0x622e20` / `0x623250` | asynchronous enqueue / publication |
| `0x871910` | selection model singleton getter |
| `0xad7210` / `0xad7220` | publication lock acquire/release |
| `0x80f970` / `0x80efe0` / `0x80fba0` | lane-transform singleton / per-note consumer / RANDOM generator |
| `0x871950` / `0x8846c0` / `0x884660` / `0x884630` | options singleton / RANDOM getter / option accessor / selector |
| `0x80f900` / `0x884ba0` | lane consumer / battle-reflection getter |
| `0x811030` / `0x810b00` / `0x811040` | sequencer getter / load-reset / advance |
| `0x812e40` / `0x815210` / `0x815d80` | chart clock consumer / frame conversion / timeline getter |

All data addresses and object layouts are unchanged from the 010-0916 profile:
database header `IIDX` + version 34, `0x7f8`-byte records, analyzer size 72,
lane table `0xb1259a0`, options `0xb3e9f28`, playback sequence `0xb1281c0`,
timeline `0xb3e3b90`, and the user/profile globals. The pure decoders are
shared with the earlier 34 module.

## Reproduce

```powershell
python scripts/verify-iidx34.py 'C:\path\to\010-0917bm2dx.dll'
./scripts/build.ps1 -CargoArgs @('test', '-p', 'bilimani-iidx')
./scripts/build.ps1 -CargoArgs @('build', '-p', 'bilimani-iidx', '--release', '--locked')
python scripts/smoke-iidx34-0100917.py --game-dll 'C:\path\to\010-0917bm2dx.dll'
```

`verify-iidx34.py` checks the exact SHA-256, all 49 unique 16-byte guard
prefixes extracted from the module, database/input entry bytes, 14 scene
RTTI/lifecycle tables, dictionary and input vtable slots.

`smoke-iidx34-0100917.py` maps the game DLL with
`DONT_RESOLVE_DLL_REFERENCES`, loads the release `bilimani_iidx.dll`, and checks
that the new fingerprint is selected, every native guard and vtable check in
`install()` passes, all hook slots are redirected into the plugin, diagnostics
report `select_hooks_intact=true`, then runs the synthetic catalog/gameplay
regressions described above. Shutdown releases the worker. It does not run
the game entry point or the original scene implementations.

## Validation status and unknowns

Initial 2026-10-22 offline static and private-mapping checks passed: 46 library unit tests plus
the bilimani-iidx integration suites (75 tests total), Clippy with warnings
denied, release build, the new attach smoke check above, and the existing
`scripts/smoke-iidx34.py --queue --user` suite against the 010-0916 build as a
regression.

For the 2026-10-24 correction, the IIDX Rust tests (including example targets),
Clippy with warnings denied, formatting, TypeScript typecheck, 29 frontend
tests, production page builds and IIDX release build passed. Static verification
passed for both 34 fingerprints with 49 guards each. The expanded 010-0917
smoke and 010-0916 `--queue --user` regression both passed. The old release
artifact failed the new native-catalog smoke assertion before the rebuild.

Tested corrected DLL: `target/release/bilimani_iidx.dll`, SHA-256
`71f534a30dfe0466291ac835f975455b7d186aa6607705a5a313533a7fd91ca3`.
The gameplay observations above are of the old installed DLL; **the corrected
DLL has not been installed or validated in the real game**. Still require
in-game checks for runtime catalog/search, SP/DP auto-selection, difficulty
changes, scene transitions, play/retry/exit telemetry, menu input capture,
single-player SP double-tap, queue delete/priority, alias CSV reload, OBS
refresh, and real platform connections. The changed `sub_18094fb50` common
scene helper is called from selection and result initialization; its exact
runtime effect is not established.
