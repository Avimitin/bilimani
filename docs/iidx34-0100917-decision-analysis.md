# IIDX 34 / 010-0917 song-decision interval

Analysis date: 2026-10-25. Input: `D:\Share\tmp\010-20260916.dll`.
Despite its filename, this is the existing **010-0917** profile.

| Property | Value |
|---|---|
| SHA-256 | `c98e183ec81ecb9e1cb13c237f9c9cb0b3f65b6fdaff6fd5e6a526008c546534` |
| File size | 18,200,216 bytes |
| Architecture / image base | AMD64 / `0x180000000` |
| Image size | `0xc5ff000` |
| Implementation | `crates/bilimani-iidx/src/versions/v34/b010_0917/decision.rs` |

The hash was checked with `Get-FileHash` and IDA MCP. Auto-analysis completed
before Hex-Rays and cross-reference inspection. No game file was patched.
All addresses below are RVAs; field offsets are decimal.

## Evidence and ABI

| RVA | Observed role |
|---|---|
| `0x8d8290` | Existing selection update: vtable `0xd52508`, slot 18; calls the controller at scene +152. |
| `0x8da130` | Dispatcher: promotes pending member +56 into active member +8, applies signed this-adjustment +16 to controller +0, calls the member, then increments frame count +88. |
| `0x8d8960` | Interactive update: a widget confirmation schedules `0x8d98f0` (or a special-bar path). |
| `0x8d98f0` | Preparation; may show a dialog at `0x8d90c0` before scheduling `0x8d9330`. |
| `0x8d9330` | Confirmed-song animation. First invocation commits song/difficulties and starts the decision presentation; later invocations wait for completion. |
| `0x8d8eb0` | Commit helper: selected-record getter `0x5fa3b0` writes scene +8448, publishes it through `0x92ff10`, and publishes difficulties through `0x92ff70`. |
| `0x5f3d70` / `0x5f3160` | Configure/start decision presentation. The latter references `music_decide_chara_01`, `_02`, `_03`. |
| `0x8d9684` | Decision-complete branch schedules `0x8d9f70`. |
| `0x8d9f70` | Shared scene-exit wait; eventually invokes scene virtual slot 11. Also reachable from cancel/exit, so it cannot independently prove a decision. |
| `0x91a300` | Existing normal stage initialization hook publishes `playing`. |

The decision member uses the Windows x64 member-call convention with the scene
pointer in RCX; no additional argument is consumed in the inspected body. It is
not directly called or detoured. The existing selection-update wrapper forwards
the original arguments and return value, then reads the controller on the game
thread. HTTP and the worker never read these pointers.

The classifier requires active member = image base + `0x8d9330`, zero
this-adjustment, a positive completed-invocation count, and a non-null committed
record at scene +8448. A pending target alone does not qualify. The record is
decoded afresh, avoiding the older, rate-limited selection sample. Owned
telemetry reports `now_playing.phase: deciding`; scheduling phase remains
`other`, menu access stays unavailable, and `plays` does not increase.

The next member (`0x8d9f70`), cancellation, unreadable state, and selection cleanup
clear the decision phase. Stage initialization also replaces it. The native
interval thus ends before gameplay initialization. Browser visibility follows
the existing worker publication and 500 ms page polling; delivery is not a hard
real-time guarantee.

Both active-scene identity and scene epoch are checked after the original
callback, preventing publication from an old scene after synchronous cleanup or
reinitialization. Six 16-byte guards cover update, dispatcher, decision entry,
commit call, exit scheduling, and committed-record write. A guard mismatch
disables only this optional sampler and is reported in the DLL log.

## Reproducible verification

```powershell
Get-FileHash -Algorithm SHA256 'D:\Share\tmp\010-20260916.dll'
py scripts/verify-iidx34.py 'D:\Share\tmp\010-20260916.dll'
./scripts/build.ps1 -CargoArgs @('test', '-p', 'bilimani-iidx', '--all-targets', '--locked')
./scripts/build.ps1 -CargoArgs @('build', '-p', 'bilimani-iidx', '--release', '--locked')
py scripts/smoke-iidx34-0100917.py --game-dll 'D:\Share\tmp\010-20260916.dll'
./scripts/build.ps1 -CargoArgs @('build', '-p', 'bilimani-iidx', '--example', 'api_preview', '--locked')
npm run typecheck
npm test
npm run build
npm run test:browser:turntable
```

The read-only verifier derives guards from Rust sources. The smoke test maps the
exact image without executing its entry point, validates guards, then replaces
original callbacks with stubs and supplies synthetic scene/controller/song
records. It checks pending/unexecuted decision rejection, committed-song
publication, pre-game exit, cleanup and the existing stage families. It does not
execute native decision logic.

The browser test uses production assets and the real Rust simulator's `decide`,
`other`, `select`, `play` and `logout` commands. It checks rotation, purple trails,
uninterrupted animation across polls, refresh during decision, stopping before
gameplay, cancellation, and `display.vfdEffects: false`. Screenshots are local
artifacts under `analysis/browser/turntable-*.png`.

Offline results on 2026-10-25: all 55 extracted native guards passed; the
compiled release DLL passed the mapped-image smoke test; IIDX Rust tests,
TypeScript checking and all 30 TS tests passed; release DLL and production web
builds completed; the production-page turntable browser test passed.

## Remaining live checks

- No real game was run for this change. Confirm timing with SP 1P/2P, DP,
  timer-driven confirmation, optional dialogs, and cancellation on this build.
- Course, arena, retry and special selectors bypassing this member do not
  currently produce a decision animation. Do not infer one from `other`.
- IIDX 33 and 34/010-0916 have no decision sampler yet; this new effect is
  limited to the analyzed 010-0917 path.
- Check the rotating SVG and soft purple glow in OBS/CEF on the
  target machine, including the existing display-effects setting.
