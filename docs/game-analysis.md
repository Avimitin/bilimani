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

Stage vtables whose slots 13/14 are observed: `0xda50a8`, `0xda5188`, `0xda5268`,
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

## Live song metadata and lifecycle

The same verified IIDX 33 image was inspected again with IDA/Hex-Rays for the
display API. `song_info.rs` decodes a fresh, bounded `0x7f8`-byte copy of the
selected/stage record; it deliberately does not use the once-captured request
catalog. The copied disk databases leave chart-derived fields zero. These are
populated by the running game, so zeros/sentinels must not become fabricated
metrics. All offsets below are **record offsets**, not image RVAs.

| Record offset | Layout / evidence |
| --- | --- |
| `0x000` | UTF-16LE title, 256 bytes (existing catalog parser) |
| `0x140` | UTF-16LE genre, 128 bytes; `0x601610` / `0x830250` pass `record + 320` to text rendering |
| `0x1c0` | UTF-16LE artist, 256 bytes; `0x601610` / `0x8322e0` render `record + 448` |
| `0x3dc` | u16 introduction-version index; `0x82d980` reads word index 494 |
| `0x3ec + chart` | u8 level; `0x9522b0` checks nonzero and at most 12 |
| `0x3fc + chart*8` | u32 maximum BPM |
| `0x400 + chart*8` | u32 minimum BPM; zero means use maximum |
| `0x47c + chart*4` | u32 total notes; score-rate consumers `0x82d1d0` / `0x6fd040` read dword index `287 + chart` |
| `0x4fc + chart*24` | Six signed radar values in NOTES, PEAK, SCRATCH, SOFLAN, CHARGE, CHORD order |

`0x637fc0` copies BPM into the widget's +28/+32 fields. `0x637e10` renders +32
through `bpm_low*` and +28 through `bpm_high*`; minimum <= 0 hides the range.
`0x7fefc0` copies the six radar axes from `record + 1276 + 24*chart`.
`0x7fded0` and `0x830de0` associate that order with `100_notes`, `100_peak`,
`100_scratch`, `100_sof_lan`, `100_charge`, `100_chord`, multiplying by 0.0001
relative to the 100% reference vertices. JSON therefore divides raw values by
100: 10000 becomes 100.0. No graph smoothing or visual minimum is applied to
the exported values. All-zero/missing or negative sentinel radar becomes null.

`0x82dfa0(difficulty, mode)` establishes SP indices 0–4, DP 5–9. For display,
the mode is resolved using the play-style flag at `0xacd79a4` (`0x9493a0`, then
booleanized by `0x949570` / `0x82e0f0`) and the existing `0x949530` layout query.
The latter also reports a double layout with two SP participants, so it must
not alone select DP metadata. A DP battle layout uses SP chart data, matching
the adjustment in `0x7fefc0`. Per-side selection difficulties come from the
existing guarded `0x607030(widget, side)` and participation from `0x9493e0`.

On eligible normal selection frames, after original update returns, the adapter
checks scene identity and song bar type, then samples at most once per 250ms.
Folder bars clear `song` despite the native record getter's fallback to ID 1000.
Selection cleanup clears all display state, and callbacks retain their original
arguments and return values.

`0x90f990` returns `CStageMain` at `0xabac020`. Prepare (`0x90f310`) stores the
actual music record at +8, from `0x9493b0` / `0xacd79d0`. Stage init
(`0x9335d0`, and the wrapper at `0x8d22d0`) calls `0x90e080`, which uses that
record. After the original init, the adapter reads `0xabac028`, the per-side
difficulties at `0xacd79a8` (`0x949430`), and copies the record. This handles a
stage whose song differs from the last ordinary selection. It never assumes a
request-engine song is the played song. The listed stage vtables' slot 14 all
resolve to cleanup `0x933640`, which invokes `0x90d200`; these slots are now
guarded and wrapped to clear display state on exit. No stale song is kept through
results/logout. New read-layout evidence functions receive entry-byte guards.

`scripts/smoke-dll.py --song-info` validates installed callback plumbing using
synthetic records and stubbed original functions in a private mapped image,
including SP/DP index selection, a directory, a different stage record, cleanup,
and original return values. Real gameplay has not been exercised by that test.

## Native note density histogram

Rechecked through IDA MCP against the same SHA-256. This is an internal native
interface, not a DLL export. `MusicDetailDataAnalyzer` drives the assistant
panel's time histogram; the separate `notes_graph_key*` widgets are lane totals.

| RVA | Finding |
| --- | --- |
| `0x6320d0` | Analyzer constructor: `(this, music_id)`; ten optional chart entries |
| `0x632730` | Loads `%05d/%05d.1` on the native detail worker; parses each available chart |
| `0x632d20` | Note events 0/1: resize both vectors to `timestamp_ms / 1000 + 1`, increment total; lane `% 10 == 7` also increments scratch |
| `0x632e70` | End event 6 stores `timestamp_ms` at detail +112 |
| `0x632f20` | Worker consumes pending ID under AVS mutex and publishes an immutable analyzer under the shared_ptr lock |
| `0x650550` | Native panel copies published analyzer; requests its selected ID through the worker mutex |
| `0x64f920` | Renders total and scratch vectors; native height caps at 30 notes/bin |
| `0xa7d33e0` | `MusicDetailDataThread*` singleton, owned by the assistant UI |
| `0xcb5cd0` / `0xcb5cf0` | Expected thread mutex / analyzer vtables |
| `0xaf158c` / `0xaf159c` | MSVC shared_ptr lock/unlock, bit 0 at `0xbaac324` |
| `0xc91fe0` / `0xc91fe8` | AVS mutex lock/unlock import slots (`avs2_core_16/17`) |

The 56-byte thread has mutex handle +8, optional pending ID +16 (u32 ID, presence
byte +20), and published shared_ptr at +40/+48. The 256-byte analyzer has music
ID +8 and ten entries at +16, stride 24: shared_ptr +0/+8, presence byte +16.
Chart order is SP B/N/H/A/L then DP B/N/H/A/L, matching the live music record.
Each `MusicDetailData` starts with two 24-byte `vector<i32>` objects, total at +0
and scratch at +24 (begin/end/capacity pointers). Per-lane totals occupy +48/+80;
duration is u32 milliseconds at +112. A BPM map at +120 is not exported here.

The increment is `1 + (event.u16_at_6 != 0)`: charge notes contribute **2 in the
onset second**, including charge scratches, rather than one count at each end
or counts while held. Both DP sides contribute to the same chart histogram.
Scratch is a subset of total. These are the game's native weighted note counts,
not a claim about literal button presses per second. The vectors end at the last
note's bucket; the chart's silent tail is represented by `duration_ms`.

`native_density.rs` runs in the existing scene callbacks. At the existing 4 Hz
selection sampling point it reads the singleton, uses a nonblocking atomic
try-lock of the verified shared_ptr lock, and copies bounded immutable data
while the lock pins the analyzer. All error paths release the lock. Missing or
different song results trigger the same pending-ID write as `0x650550`, under
the native AVS mutex. Thus opening the assistant tab is unnecessary when its
worker exists. No native analyzer constructor, file parser, or shared scratch
buffer is invoked from our thread. The thread singleton is destroyed by native
UI cleanup on the same scene thread; HTTP and our worker never follow pointers.

One owned song snapshot is cached, matched by canonical music ID. Stage entry
can reuse it after the native UI is destroyed, but a different song never gets
the old graph. No analysis is requested during stage entry. Missing UI worker,
unavailable chart, unfinished analysis, invalid headers/vectors or unsupported
data produce `density: null`; entering a special stage without prior selection
may therefore have metadata but no density. The whole existing adapter remains
restricted to the exact supported DLL hash; request, publication and counting
functions also have entry-byte guards. Bounds are 1 hour, at most 3601 buckets,
10000 weighted notes per bucket, and 1000000 per chart. The API retains full
native 1s counts, including values above 30 and the silent tail. Mecha's VFD
display averages 5s windows (10s for songs over 3 minutes), normalizes a partial
last window by its duration, and quantizes to 12 illuminated segments. The
displayed peak remains the original 1s peak; empty windows retain unlit segments.

`tests/density.rs` covers layout, all ten indices, corruption and song identity.
`smoke-dll.py --song-info` uses a private mapped image and synthetic native
objects to test the actual request lock/unlock ABI, busy publication lock,
asynchronous publication, cached stage data, and stale-song rejection. It does
not execute the game's file loader or validate a live gameplay session.

## Opposite-Start input eligibility

The shortcut reads `0x9493e0(0/1)` on selection frames and requires exactly one
participating side plus SP mode. Unlike `0x949230`, which can return zero for
ambiguous states, these two flags allow both absent and both present states to
disable the shortcut. The function's first 16 bytes are guarded at installation.
No card identifiers or authentication state are inferred from it.

## Logged-in card snapshot

The IIDX 33 adapter reads static player state with `ReadProcessMemory`, without
calling authentication or reader functions. Both subsystem flags at `0x10b90e8`
and `0x10b90ec` must be nonzero, matching the native side-validation function at
`0x5c4480`. Participation flags at `0xacd79b0` select exactly one side. No joined
side or two joined sides produce the global stream fallback.

Per-player data has stride `0x3b103a0`. Side zero's card UID is the u64 at
`0x6c11808`; native `0x5ad900` copies the reader's eight bytes in reverse order
into this field. Native formatter `0xab7060` prints this numeric value as sixteen
uppercase hexadecimal digits. Card entry `0x8badb0` passes reader output through
`0x5ad900`, and `0x5b4190` formats the same field for the card lookup request.
The adapter uses that representation, not an IIDX player ID or display name.

The play type at `0x6c11820` must differ from 1 (guest, whose card is also zeroed
by `0x5adb20`), and the player-data flag at `0x312771e` must be nonzero. Native
player-data operations including `0x5ad8a0` and `0x59e090` check both conditions;
`0x5abed0` clears the per-player data before a new card is processed. Null and
all-ones UIDs are rejected. The adapter takes two matching snapshots before
publishing an owned `CardId`; read errors or changing data produce no identity.
Entry-point guards cover `0x5c4480`, `0x5ad900`, and `0x5ad8a0` in addition to the
existing exact file fingerprint. Card IDs are masked in the menu and excluded
from diagnostics and OBS snapshots.

Unit fixtures cover both sides, guest/unready/ambiguous states. The DLL smoke
test with `--profiles` changes synthetic flags/cards in its private mapped copy,
verifying actual worker routing, same-profile stability and logout fallback.
This does not authenticate a real card or run a live game session.

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

## Controller navigation capture

The supported image's `IO::InputManagerIIDX` vtable is at RVA `0xdd05c0`.
Slot 3 points to the input poll at RVA `0xa7a2f0`; the guarded first 16 bytes are
`48 89 4c 24 08 55 53 56 57 41 54 41 55 41 56 41`.
IDA cross-references tie this table to the singleton accessor at RVA `0xa79b50`.
The poll is called normally before reading or masking its output.

Decompilation verifies four button words at object offsets `+8`, `+12`, `+16`,
`+20`. Bits 0–6 are P1 keys and 7–13 are P2 keys. Turntable position and delta
are at `+0x58 + side*8` and `+0x5c + side*8`, with zero-based side.
The intervening `+0x18..+0x57` region contains C++ containers and must not be
copied, cleared, or treated as an array of button state.

While the panel is open in eligible single-player SP song select, the adapter
emits navigation from the logged-in side, masks only that side's seven key bits
in all four button words, restores its pre-poll turntable position and clears
its delta. It does not mask Start, service keys or the other side. Closing keeps
held menu keys masked until release. Layout masking and edge/repeat behavior
have independent tests; native timing and scratch direction require live testing.

## Validation boundary

`scripts/check-profile.py` checks the file hash, architecture, function guards and
selection/search vtable targets and the database accessor. Automated tests cover
request state, transport, expanded native snapshots, dictionary layouts and fuzzy
matching of native keywords;
the DLL smoke test maps the game image without resolving imports or running its
entry point, then tests hook installation. This is not a live gameplay test.
The README lists the remaining interactive checks, especially frame timing,
song-only jumps, chart availability, and coexistence with installed hooks.
