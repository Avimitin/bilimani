# IIDX 33 adapter evidence

Implementation: `src/games/iidx/v33/`. Game/version selection lives in
`src/games/mod.rs`; upper layers use the contracts in `src/game.rs`.

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
| `0x9493e0` | Per-side participation query `(u32 side) -> u8`; checks two flags at `0xacd79b0`, used by opposite-Start gating |
| `0x806f60` | Modal overlay/input-blocked query |
| `0x606fd0` | Get selected music record from selection widget; fallback exists, so bar type is also checked |
| `0x606e60` | Selected bar type; 1 denotes a song/chart |
| `0x607030` | Get selected difficulty for a player |
| `0x951e80` | Canonical music ID lookup; rejects records whose embedded ID does not match |
| `0x951e40` | Record lookup by index; stride `0x7f8` |
| `0x951fd0` | Native database accessor; original LEA returns static buffer, verified Omnifix MOV returns relocated allocation |
| `0x952c20` | Loads `/data/info/1/music_data.bin` into the live buffer |
| `0xacd8900` | Original static database buffer (`0x400000` bytes); becomes a pointer slot under Omnifix |
| `0xce9f40` | `CMusicSearchStateInput::CMusicTitleDictionary` vtable; slot 1 is the XML loader |
| `0x7f0250` | Search input initialization, loads title/artist dictionaries through AVS |
| `0x7f2fd0` | Dictionary XML loader, iterates `data_list/data` and invokes supplied callback |
| `0x7ef460` | Title entry parser: `index` is music ID, `yomi` produces keyword keys |
| `0x7f4990` | Rebuilds active title map from full map according to native chart availability |
| `0x7f2b60` / `0x7f2910` | Native prefix / substring search over title map |
| `0x7f35d0` | Search update calls substring search, resolves IDs using `0x951e80`, deduplicates results |

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

The DLL calls `0x951fd0` on the selection thread and copies the returned buffer.
It verifies the accessor's eight bytes, accepting only the original
`48 8d 05 29 69 38 0a c3` or the verified relocated-buffer variant with byte 1
changed to `8b`. The supplied installation's log records Omnifix patching precisely
that byte, along with the loader limits and record accessors. Reading the old
buffer directly would instead see a heap pointer and fail the `IIDX` header check.
Snapshot size now comes from validated header dimensions (up to 10,000 records
and 100,000 lookup entries), supporting databases larger than the original 4 MiB.
Other accessor patches are rejected with a diagnostic, not executed.

## Opposite-Start input eligibility

The shortcut reads `0x9493e0(0/1)` on selection frames and requires exactly one
participating side plus SP mode. Unlike `0x949230`, which can return zero for
ambiguous states, these two flags allow both absent and both present states to
disable the shortcut. The function's first 16 bytes are guarded at installation.
No card identifiers or authentication state are inferred from it.

## Native search index

Search input initialization loads `/data/info/1//music_title_yomi.xml` through AVS.
Installed resource overrides can change the actual XML, so the DLL observes the
populated in-memory dictionary rather than reopening a hardcoded filesystem path.
Title-dictionary vtable slot 1 is wrapped with its original three-argument, byte
return ABI. After the original succeeds, the wrapper copies its map at `this+8`
on the same thread. The loader destroys the path and callback arguments; the
wrapper never accesses them afterwards. The caller then copies that map to the
master map at `this+24`, so capturing the master map inside the loader is too early.

The map contains a head pointer and count. MSVC tree nodes store left/parent/right
at +0/+8/+16, sentinel flag at +25, `std::wstring` key at +32 (length +48,
capacity +56; capacities below 8 use inline UTF-16), and a shared item pointer at
+64. The item's canonical music ID is at +8. Snapshot reads use ReadProcessMemory,
bounded dimensions/string lengths, cycle detection and node-count validation.
Only owned Rust strings and IDs cross to the worker, never borrowed C++ objects.

The worker adds those native keywords to the fuzzy matcher's canonical title and
reading terms, resolves them by music ID, and emits each song only once. Unknown
IDs are skipped. A later dictionary load replaces previous native keywords.
The full index is captured before mode/availability filtering; the existing chart
and native reservation checks still reject unavailable requests at selection.
If the search dictionary has not initialized, canonical titles/readings remain
usable. No extra install, manual XML copy, or search-window prerequisite is needed
for basic requests. The touchscreen's own matching and result ownership are unchanged.

## Validation boundary

`scripts/check-profile.py` checks the file hash, architecture, function guards and
selection/search vtable targets and the database accessor. Automated tests cover
request state, transport, expanded native snapshots, dictionary layouts and fuzzy
matching of native keywords;
the DLL smoke test maps the game image without resolving imports or running its
entry point, then tests hook installation. This is not a live gameplay test.
The README lists the remaining interactive checks, especially frame timing,
song-only jumps, chart availability, and coexistence with installed hooks.
