# IIDX 33 adapter evidence

Analyzed with IDA/Hex-Rays using a local copy of `modules/bm2dx.dll` from the
user-supplied share. The share was only read. IDA database and game files remain
under ignored `analysis/`; no game assets are shipped or checked into git.

SHA-256: `c61b6dcb8894062e56d60da8ca90053b27f129e1a8e8da5e54457aa42602397d`

Image base in IDA: `0x180000000`. All values below are **RVAs**, relocated at runtime.
The profile is deliberately restricted to this exact binary, with additional
in-memory guards for native entry points. An update needs a new analysis/profile.

| RVA | Finding |
|---|---|
| `0xd84788` | `CMusicSelectScene` vtable; slots 13/14/15 = initialize/cleanup/update |
| `0x8eb820` | Selection initialize; owns the selection widget at `this + 408` |
| `0x8ebeb0` | Selection cleanup; calls widget cleanup which disables reservations |
| `0x8ec1f0` | Per-frame selection update; runs state controller then base-scene update |
| `0x8ee190` | Normal interactive selection state, dispatches `0x8ec450` |
| `0x8ec450` | Calls selection widget update with modal/transition checks |
| `0x6077d0` | Widget update; processes native reservations only when input is enabled |
| `0x6094a0` | Reservation consumer; uses requested difficulty or `-1`, opens a suitable category, selects song, refreshes UI |
| `0x7d60e0` | `MusicReserveImpl` singleton getter |
| `0x7d6150` | Reserve song: `(this, music_id, mode, difficulty)`; called via vtable slot 0 by native UI |
| `0x7d5eb0` | Check reservability, mode, native chart availability and category membership |
| `0x7d6090` | Consume request; returns `[difficulty, mode, music_id]`, marks it consumed |
| `0x82ded0` | Current mode thunk, 0 = SP, 1 = DP |
| `0x949230` | Active player side, 0/1 (handles SP on the right side) |
| `0x806f60` | Modal overlay/input-blocked query |
| `0x606fd0` | Get selected music record from selection widget; fallback exists, so bar type is also checked |
| `0x606e60` | Selected bar type; 1 denotes a song/chart |
| `0x607030` | Get selected difficulty for a player |
| `0x951e80` | Canonical music ID lookup; rejects records whose embedded ID does not match |
| `0x951e40` | Record lookup by index; stride `0x7f8` |
| `0x952c20` | Loads `/data/info/1/music_data.bin` into the live buffer |
| `0xacd8900` | Live music database buffer, maximum `0x400000` bytes |

`MusicReserveImpl` fields: +8 enabled, +9 consumed flag, +16 music ID, +20 mode,
+24 difficulty. Passing difficulty `-1` follows the native song-only path and
uses the current player difficulty. The reservability check's fourth argument is
an optional integer encoded with difficulty in the low DWORD and the presence
byte at bit 32. Native callers and the decompiled check establish this layout.

The selection frame wrapper submits only in the normal interactive state: base
scene state +80 equals 3, timer +128 is positive, controller pointer +144 exists,
controller +8 equals `0x8ee190`, member-function adjustment +16 is zero, no queued
state transition at +56, no modal overlay, and reservations enabled. It does not
overwrite a pending touchscreen reservation. After the original update it checks
consumption and the selected canonical music ID/difficulty before acknowledging
success. An unconsumed reservation is withdrawn and retried after the UI is ready.

Only vtable pointers are replaced, with aligned atomic stores and restored page
protection. No instruction relocation or inline trampoline is needed. Original
callbacks are preserved. Stage scene initialization reports gameplay start for
queue advancement; selection cleanup prevents submissions outside that screen.
Network, search, configuration, and OBS writes run on a worker thread.

Stage vtables whose slot 13 is observed: `0xda50a8`, `0xda5188`, `0xda5268`,
`0xda5348`, `0xda5428`, `0xda5508`, `0xda55e8`, `0xda56c8`, `0xdae1e8`,
`0xdae2c8`, `0xdae3a8`, `0xdae488`, `0xdae728`. These identify stage entry;
special song-selection scenes themselves are not supported by this adapter.

## Database

16-byte header: `IIDX`, u32 version (33), u32 record count, u32 lookup-table length.
Then a u32 ID-to-index table, followed by `count` records of `0x7f8` bytes.
Each record contains UTF-16LE title at +0 (256 bytes), Shift-JIS title reading
at +`0x100` (64 bytes), ten chart levels at +`0x3ec` (SP B/N/H/A/L, DP B/N/H/A/L),
and canonical music ID at +`0x67c`. A level of zero means that chart does not exist.

Some unused lookup entries map to record zero rather than `0xffffffff`; following
every non-sentinel entry would create bogus IDs. The parser iterates records and
validates each record's canonical ID against the lookup table, matching the native
getter's identity check. The copied active database parses as 1,932 canonical songs.

## Validation boundary

`scripts/check-profile.py` checks the file hash, architecture, function guards and
selection vtable targets. Automated tests cover request state and transport;
the DLL smoke test maps the game image without resolving imports or running its
entry point, then tests hook installation. This is not a live gameplay test.
The README lists the remaining interactive checks, especially frame timing,
song-only jumps, chart availability, and coexistence with installed hooks.
