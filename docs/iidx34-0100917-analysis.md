# IIDX 34 / 010-0917 binary analysis and adapter evidence

Analysis date: 2026-10-22. Target supplied by the user:
`C:\Users\BeatMania IIDX LM\Downloads\010-0917bm2dx.dll`
(same bytes as `010-0917bm2dx (1).dll`).
This is a distinct exact-build profile from
[IIDX 34 / 010-0916](iidx34-analysis.md), not support inferred from the version
label.

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

The stage initialize/cleanup/update target checks in `native.rs` use the new
RVAs above. Optional lane/playback samplers remain gated by their own entry
guards; a mismatch disables only that display area.

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
python scripts/smoke-iidx34-0100917.py --game-dll 'C:\path\to\010-0917bm2dx.dll'
```

`verify-iidx34.py` checks the exact SHA-256, all 47 unique 16-byte guard
prefixes extracted from the module, database/input entry bytes, 14 scene
RTTI/lifecycle tables, dictionary and input vtable slots.

`smoke-iidx34-0100917.py` maps the game DLL with
`DONT_RESOLVE_DLL_REFERENCES`, loads the release `bilimani_iidx.dll`, and checks
that the new fingerprint is selected, every native guard and vtable check in
`install()` passes, all hook slots are redirected into the plugin, diagnostics
report `select_hooks_intact=true`, and shutdown releases the worker. It does
not execute any game callback.

## Validation status and unknowns

Offline static and private-mapping checks passed: 46 library unit tests plus
the bilimani-iidx integration suites (75 tests total), Clippy with warnings
denied, release build, the new attach smoke check above, and the existing
`scripts/smoke-iidx34.py --queue --user` suite against the 010-0916 build as a
regression.

**Actual IIDX 34 gameplay has not been run for this build.** Still require
in-game checks for runtime catalog/search, SP/DP auto-selection, difficulty
changes, scene transitions, play/retry/exit telemetry, menu input capture,
single-player SP double-tap, queue delete/priority, alias CSV reload, OBS
refresh, and real platform connections. The changed `sub_18094fb50` common
scene helper is called from selection and result initialization; its exact
runtime effect is not established.
