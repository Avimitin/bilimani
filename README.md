# chart-requester

A self-contained Windows x64 Rust DLL that turns Bilibili danmu into beatmania
IIDX song requests. Load it with Spice's `-k` option. No Python, browser,
blivechat executable, playlister, or companion process is needed at runtime.

**Supported game: IIDX 33, supplied installation configured as `2026081900`
(2026-08-19), restricted to the exact DLL listed under [Game compatibility](#game-compatibility).**

## Install

1. Put `chart_requester.dll` in a **local writable directory**.
2. Copy `chart-requester.example.toml` beside it as `chart-requester.toml`.
3. Set `[bilibili].auth_code` to the broadcaster identity code used by blivechat.
4. Add `-k "C:\path\to\chart_requester.dll"` to your existing Spice launch command.
5. In OBS, add two **Text (GDI+)** sources with **Read from file** enabled:
   `obs/queue.txt` and `obs/interaction.txt` beside the DLL, or the paths you configured.

The DLL creates a default config if missing. Restart the game after editing it.
Keep your existing game launch arguments. Do not replace the game DLL.
The build output and ready-to-copy files are placed in `dist/` by the packaging script.

## Requests

```text
点歌 AA SPA
点歌 冥
点歌 AA -rebuild- DPA
```

Difficulty is optional. Valid tokens are **SPB, SPN, SPH, SPA, SPL, DPB, DPN,
DPH, DPA, DPL**; lowercase is accepted. An omitted difficulty retains the game's
current SP/DP mode and uses the game's native song-only jump. Opposite-mode,
nonexistent, locked, or unavailable charts are rejected, with feedback in the
interaction file. Requests can start after entering the first supported song-select
screen; this supplies the current mode and the live song database.

Search uses case-insensitive, Unicode-normalized, fzf-style fuzzy subsequence
matching. Titles and the game's title readings are searched separately. Exact
titles rank first, but still require a numbered choice if other candidates match.
This is subsequence matching, not edit-distance spelling correction. Chinese
nicknames or additional romanizations can be added as aliases:

```toml
[aliases]
"my nickname" = "AA -rebuild-"
"another nickname" = "25009" # canonical music ID, quoted
```

Multiple candidates appear as a numbered list under the requester's name. Only
that same user's numeric reply selects a candidate. Each user has one pending
selection; a new `点歌` command replaces it. Separate users can choose concurrently.
The default candidate limit is 5 and the deadline is 60 seconds. Open Live provides
an opaque user ID rather than the viewer's public numeric UID; selection uses that
stable ID, never the display name.

The waiting queue holds 20 requests by default and rejects new requests when full.
After a successful jump, the request leaves the waiting queue and appears as
**current**. It remains there until gameplay starts or its 600-second timeout
expires. Playing any song consumes/skips the current request. The next request
jumps when you return to song select. A timeout can advance immediately if you
remain at song select. The DLL never starts a chart or requires a hotkey.

Optional per-user cooldown starts only on successful enqueue; `cooldown_seconds = 0`
disables it, and `300` permits one accepted request per five minutes. Queue state
resets when the game restarts. The queue capacity counts waiting requests, excluding
the separately displayed current request.

## Bilibili connection

The default **Open Live** mode follows blivechat: it sends the broadcaster identity
code to the configured `https://blive.chat` public API to start a session, then
receives messages directly from Bilibili's secure WebSocket servers. This uses
blivechat's hosted service; it does not install or launch that application.
Its availability and Bilibili's live-session limits still apply.

For direct Open Live access, set `relay_url = ""` and supply your own `app_id`,
`access_key_id`, `access_key_secret`, and `auth_code`. Requests are signed with
HMAC-SHA256. The DLL sends both WebSocket and Open Live session heartbeats,
reconnects with backoff, handles zlib/Brotli batches, and deduplicates messages.

Alternative **web** mode uses `mode = "web"` and `room_id`. Set `sessdata` and
`buvid3` if required by your account/room's authentication. This mode ports
blivedm's room lookup, WBI signing and socket authentication. Anonymous access is
not guaranteed, and messages without a usable sender ID cannot enter the queue.
Credentials stay in the local TOML and are never included in OBS output or logs.

## Game compatibility

The current adapter supports only the following supplied game build:

| Item | Supported value |
|---|---|
| Game | beatmania IIDX 33 (Windows x64) |
| Configured software version | `LDJ:J:D:A:2026081900` (2026-08-19) |
| Module | `bm2dx.dll` |

The software version above comes from the supplied installation's
`prop/ea3-config.xml`; it is an informational label, not a binary compatibility
check. The exact supported `bm2dx.dll` is identified by this SHA-256:

```text
c61b6dcb8894062e56d60da8ca90053b27f129e1a8e8da5e54457aa42602397d
```

**The song-jump function addresses, byte signatures, vtable slots and structure
offsets are version-dependent.** Other updates of IIDX 33 and other major versions
are unsupported unless separately analyzed and given a matching adapter. The DLL
checks both the game file hash and native entry-point bytes before installing
hooks; changing a version label or bypassing the hash check does not make a new
build compatible.

It supports the main `CMusicSelectScene` used by normal song selection, including
SP/DP. Special selection interfaces (such as Life/STEP UP, Arena/BPL and course
selection) are not adapters in this release; requests wait until the supported
screen is available. No unlocks or availability checks are bypassed. Unknown game
builds and changed native entry points disable the hook and report an error.

The adapter uses the live database. `game.database_path` is an optional override
for a matching IIDX 33 database, resolved relative to the DLL. It does not make a
different game binary compatible. Other hooks may coexist, but patches to the
guarded entry points cause this DLL to refuse installation.

The game-thread bridge, reservation ABI and validation evidence are documented in
[docs/game-analysis.md](docs/game-analysis.md). **Static analysis, automated tests,
and DLL loading checks do not replace a live game test.** Verify the following
with your game and stream before relying on it:

- A song-only request and SPA/DPA requests select the intended chart; opposite-mode
  and locked charts report errors.
- `点歌 AA` produces candidates, and only the requesting viewer can select them.
- One request jumps at a time; playing either it or a different song advances on return.
- With a temporarily short timeout, an ignored request advances while at song select.
- Opening a menu, starting gameplay, or leaving song select prevents jumps.
- Both OBS sources update, and a connection interruption recovers.

## Build and test

Tested with Rust 1.98.1 (edition 2024), Visual Studio C++ build tools and
a Windows SDK. These are build-time requirements only; the release uses a static CRT.

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build.ps1
powershell -NoProfile -ExecutionPolicy Bypass -Command "& ./scripts/build.ps1 -CargoArgs @('test','--all-targets')"
powershell -NoProfile -ExecutionPolicy Bypass -Command "& ./scripts/build.ps1 -CargoArgs @('clippy','--all-targets','--','-D','warnings')"
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/package.ps1
```

The build script discovers MSVC and also supports workspace-local Microsoft SDK
NuGet packages under `reference/sdk/`. It does not install tools automatically.
Developers can run `scripts/fetch-sdk.ps1` to populate that fallback.
`scripts/check-profile.py` verifies a local game copy without loading it; the
`catalog_check` Cargo example validates and searches a local music database.
Game binaries, databases, IDA files, reference repositories, credentials and build
artifacts are excluded from git. None are included in the release bundle.

Startup and file-output errors are written to `chart-requester.log` beside the DLL.
Modern Spice SDK shutdown callbacks close the chat session. On older loaders or
forced process termination, Bilibili expires the session through its heartbeat TTL.
The DLL remains mapped until process exit; runtime unloading is not supported.
