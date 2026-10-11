# IIDX 33 / 20260812 adapter evidence

Implementation: `crates/bilimani-iidx/src/versions/v33/b20260812/`. Game/version selection lives in
`crates/bilimani-iidx/src/versions/mod.rs`; upper layers use the contracts in `crates/bilimani-iidx/src/game.rs`.

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

The worker also reads the two participation u32s at `0xacd79b0` when publishing
snapshots. Selection/stage exit hooks retain the previous mode, and no further
scene callbacks may run after logout. An `Other` snapshot with an exact eight-byte
all-zero participation sample publishes `mode: null`; active selection/play,
nonzero participation and failed/short reads preserve the sampled mode. This
does not depend on card/profile readiness and does not call a game function from
the worker or write back into the scene mailbox.

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

`crates/bilimani-iidx/tests/density.rs` covers layout, all ten indices, corruption and song identity.
`smoke-dll.py --song-info` uses a private mapped image and synthetic native
objects to test the actual request lock/unlock ABI, busy publication lock,
asynchronous publication, cached stage data, and stale-song rejection. It does
not execute the game's file loader or validate a live gameplay session.

### Original per-lane note totals

Rechecked with IDA MCP on the same supported image. The lane-count interface is
the already-published `MusicDetailData` object, not a DLL export or an input poll.
No additional hooks, native calls, or memory writes are needed.

`0x632d20` chooses detail +48 for note event type 0 and +80 for type 1. Each is
eight `i32` values: keys 1–7 followed by scratch. It increments index
`event.byte_at_5 % 10` by `1 + (event.u16_at_6 != 0)`, exactly the histogram's
weight. These are original chart lanes before RANDOM/MIRROR; CN/HCN and charge
scratches count twice. The chart parser `0x632730` zeroes both arrays and builds
all ten optional difficulty entries independently.

`0x650030(panel, widget_name, counts)` renders eight consecutive values using
the name table at `0x105e9a0`: `notes_graph_key1` through `notes_graph_key7`, then
`notes_graph_scratch`. Its bar-height limits of 175/350 are only visual caps.
`0x64f440` passes detail +48 to the selected physical player's widget for SP,
regardless of whether that player is 1P or 2P. DP passes +48/+80 to the two
widgets. Thus SP API counts always have original chart `side: 1`; DP has both
chart sides. Physical player side belongs to the separate lane-order API.

The adapter copies both arrays from the same 116-byte header already pinned
by the analyzer publication lock. It rejects negative/implausible counts,
totals above 1,000,000, empty charts, or a nonempty second SP array. Where the
histogram is valid, total and scratch sums must agree. Invalid lane totals do
not suppress a valid density graph; invalid histogram pointers do not suppress
independently valid lane totals. Song identity and the existing owned cache
prevent reuse across songs and retain counts after the selection UI is gone.

`/api/lane-counts` projects every available difficulty of the current song from
that owned snapshot. `/api/lane-order` independently projects physical side,
random mode, status and permutation. No request follows a game pointer.
Tests cover all ten chart slots, SP/DP layouts, corruption, histogram agreement,
cache lifetime, HTTP idle clearing and frontend source-to-destination lookup.

## Native RANDOM lane order

Implementation: `lane_order.rs`. All addresses below belong to the same guarded
IIDX 33 image described above. IDA/Hex-Rays connects the gameplay transform and
the game's result-screen lane string; no random function is invoked by the DLL.

| RVA | Finding |
| --- | --- |
| `0x8234b0` | Getter for the lane-transform singleton at `0xa7ef570` |
| `0xa7ef580` | Singleton +16: two `u32[8]` source-to-destination maps, stride 32 bytes |
| `0x823230` | Resets both maps to identity and initializes RNG state |
| `0x8236e0` | Generates RANDOM/R-RANDOM and applies ordinary MIRROR to the maps |
| `0x822b20` | Per-note lookup; fixed table except S-RANDOM / BEGINNER helper branches |
| `0x8229f0` | S-RANDOM consumes RNG and updates per-lane deadlines; has no fixed permutation |
| `0x822670` | BEGINNER helper's stateful per-note transform |
| `0x8854a0` | Getter for `COptionGameData`, pointer at `0xaab3aa8`, vtable `0xd54e60` |
| `0x897800` / `0x897830` | Option block index and address: object +8 +180*index; SP side 0/1, DP index 2 |
| `0x897890` | Random enum at block +48 +4*side: 0 OFF, 1 RANDOM, 2 R-RANDOM, 3 S-RANDOM |
| `0x8960b0` | MIRROR boolean at block +56 +4*side |
| `0x823440` / `0x897d30` | Extra post-table reflections used by special/battle modes |
| `0x92b590` | Result UI inverts the transform by writing each source digit at its destination |
| `0x90f310` / `0x8245c0` | Stage prepare loads the chart and generates maps before stage initialization returns |
| `0x90f420` | Retry regenerates maps through the same loader without necessarily leaving the scene |

The table contains zero-based **source -> destination**, including scratch index 7.
The API instead reports one-based source digits at each physical destination,
left to right, matching `0x92b590`. Thus `[4,3,0,1,2,5,6,7]` becomes `3452167`.
All seven keys must be a permutation of 0..6 and scratch must remain 7. Ordinary
MIRROR is already included in the generated table and must not be applied again.

The post-table reflections in `0x822b20` both use the `6,5,4,3,2,1,0,7` lookup.
`0x823440` requires game kind 7/8, participant count/layout equal to 1, side 2,
SP and random enum nonzero. `0x897d30` requires option block +156 equal to 1,
participant count/layout not equal to 1, SP and random enum nonzero. The sampler
reproduces their XOR without invoking the consuming lookup. Context at `0xacd79a0`
contains six u32 fields: game kind, DP flag, two difficulties, two joined flags.
`0x949480` uses joined count, with DP normalized to 1.

At selection, only OFF/MIRROR has a known arrangement; random options return
`pending` until stage initialization finishes. Playing snapshots validate the
current table throughout the scene's lifetime. Stage vtable slot 15 is sampled
after its original callback every 250ms, covering retry. The 13 supported stage
tables use `0x9336a0`, except table `0xdae728`, which uses `0x8d2350`; both
entry points and all slot targets are guarded. Cleanup clears the snapshot.

Do not gate lane sampling on `0xaab19ae`. A live regression showed the layout
only during initial loading, followed by `unavailable` during normal play.
Although `0x90f310` sets this byte on preparation failure, `0x90ffc0` also sets
it on **successful** sound loading before transitioning to `0x910350`, and
`0x9103a0` sets it in normal stage flow. Other transitions clear it again.
It is a shared stage-control flag, not a persistent load-error indicator.
The sampler ignores it; invalid/unreadable maps still clear the displayed keys,
retry still reads the newly generated map, and stage cleanup clears all data.

The RANDOM generator itself (`0x8236e0`) may be patched by an installed plugin.
The live installation reported this on 2026-10-10; treating it as a fatal adapter
error also prevented the HTTP listener from starting. The sampler does not call
or hook that generator, so its original bytes remain file-analysis evidence only.
A runtime difference is logged and its final table is still sampled, provided
the consuming lookup, layout/option guards and stage callbacks match. Every
sample still validates all seven destinations and scratch. Other lane guard or
stage-update target failures disable only lane sampling and skip the new stage
update hooks; existing song metadata, requests, chat and HTTP remain available.
Diagnostics include observed bytes/targets to distinguish future incompatibilities.

Only bounded `ReadProcessMemory` reads are performed: option bytes, context and
64 bytes of mapping. The HTTP API receives owned arrays. S-RANDOM returns
`dynamic` with null keys; calling its transform merely to display it would alter
gameplay RNG. SP BEGINNER checks the optional helper flag at
`*[0xaab3c20] +8 +side`; a set or unreadable flag conservatively makes the fixed
map unavailable. This can hide a map when native helper eligibility would have
rejected another condition, but avoids claiming a fixed map for a dynamic chart.
Unknown options, invalid tables and read errors never produce a guessed mapping.

`crates/bilimani-iidx/tests/lane_order.rs` exercises inverse direction, options, SP/DP, reflections,
dynamic and invalid states. `smoke-dll.py --song-info` runs the actual scene hooks
against synthetic data, including retry and API clearing. The historical v0.3 check-frame.py covered the seven-digit display, option labels, DP, missing data and text escaping; current browser coverage is described in development.md. These
checks do not replace testing the new DLL during a real gameplay session.
Additional DLL regressions use `--song-info --lane-generator-patch`
to exercise existing generator patches and `--song-info --bad-lane-layout`
or `--song-info --bad-lane-update` to verify web/metadata availability when lane
sampling is disabled. No test overwrites or executes the live game's code.
The unit regression toggles `0xaab19ae` through normal loading/play states and
failed against the old gate. The DLL smoke test keeps it set across repeated
stage updates and a changed retry map, verifying published API keys stay valid.

## Opposite-Start input eligibility

The v0.4 DLL retains this verified gesture for the limited queue menu. It does
not use card identity or stream profiles. The worker toggles visibility only
when the menu renderer is available and the selection context is eligible.


The shortcut reads `0x9493e0(0/1)` on selection frames and requires exactly one
participating side plus SP mode. Unlike `0x949230`, which can return zero for
ambiguous states, these two flags allow both absent and both present states to
disable the shortcut. The function's first 16 bytes are guarded at installation.
No card identifiers or authentication state are inferred from it.

## Logged-in card snapshot (historical)

These findings describe v0.3.x only. The v0.4 DLL no longer reads card identities
or routes stream profiles. Retained as reverse-engineering evidence only.


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

The v0.4 DLL retains this input hook and compatible MinHook-chain validation.
The UI now sends only queue deletion, priority and alias-reload commands to
the worker; there are no platform settings, text fields or IME integration.


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

## Native chart playback clock

Implementation: `playback.rs`, same supported image/hash as above. RVAs were
verified in IDA; no timing function is called or hooked directly.

| RVA / offset | Evidence |
| --- | --- |
| `0x824af0` → `0xa7f1da0` | Sequence singleton accessor |
| Sequence `+0x2a29f8` | Current consumed chart frame (`i32`) |
| Sequence `+0x2a2a10` | Conversion FPS captured at chart load (`f32`) |
| `0x8297e0` → `0xaaad6c0` | Timeline singleton accessor |
| Timeline `+96` | End frame from chart event type 6 (`i32`) |
| `0x8245c0` | Prepare/load: clears current frame, captures FPS, resets timeline |
| `0x8268b0` / `0x8286a0` | Converts .1 milliseconds to frames with `int(ms / (1000/FPS) + 0.40000001)` |
| `0x828cd0` | Publishes end frame and builds BPM/scroll-position timeline |
| `0x824b00` | Sequencer tick: consumes frames toward timeline `+92`, schedules the next frame |

The .1 conversion does not subtract an origin. `frame * 1000 / captured_fps`
therefore aligns with the existing density bins to within frame quantization.
Do not use timeline `+32` (BPM), `0x8291a0` (scroll position), the scene state
machine frame counter, or subtract `0x81c9e0`'s BGM-start frame. The live FPS
configuration can be recalibrated at song end; use the rate captured for this
chart. `0x90f2d0 → 0x90f270 → 0x90de60 → 0x824b00` drives normal playback;
`0x8fbc20` also invokes the sequencer. Sampling is not gated on one normal-stage
state handler, so the shared clock can support other guarded stage classes.

The existing stage callback reads 28 bytes around current frame/FPS and 4 bytes
for end frame after the original update, at most 4 Hz. All published data is
owned. No HTTP or worker thread dereferences these addresses. Invalid/unloaded
clocks return `null`: FPS must be finite in 30..1000, end positive, duration at
most one hour, and current within 0..end+1 (the end boundary is clamped).
Outside `playing`, no clock reads occur. Stage exit clears the snapshot.

The sequencer's scheduling gate at sequence `+2763261` can stop time advancing;
same-stage retry (`0x90f420` calling prepare) resets it. Do not extrapolate with
wall time or preserve a maximum across snapshots. A normal final snapshot can
be a frame short of end; failure is not treated as full completion. The UI lights
each 5/10-second density column when playback enters its interval, keeps the
future histogram dim, and resets on zero/unavailable progress. At position zero
all columns remain off. Clock-only updates preserve the existing song DOM.

Playback guards and lane guards fail independently. Both require intact stage
update vtables; only if both features are disabled is slot 15 left untouched.
An optional failure never prevents the core adapter or web server from starting.
`[playback]` logs the enabled/disabled result and incompatible bytes/slot.

Unit tests cover frame conversion, fractional FPS and corrupt/short snapshots.
DLL smoke tests exercise actual callback-to-HTTP publication with synthetic
native clock values, including frozen time, retry, invalid-clock clearing,
recovery, cleanup and independent guard failures. Historical v0.3 browser checks covered exact 5/10-second boundaries, absent data, backwards progress, two-player plots and stable DOM identity. Current automated checks are documented in development.md. These verify the integration with a private
mapped image; synchronization against audible music still needs live gameplay.

## Player profile and native weekly activity

Implementation: `versions/v33/b20260812/user.rs`; all addresses below are RVAs
for the same verified IIDX 33 / 20260812 image. The sampler only copies memory
through ReadProcessMemory on the worker and publishes owned data. No getter is
called, no additional hook is installed, and HTTP never touches game memory.

Profile loader `0xabedf0` copies `pcdata.name` into player base `0x3101488 +
side*0x3b103a0 + 0x20` (8 Shift-JIS bytes). It stores `sach`/`dach` at `+0x34`/
`+0x38`, and `grade.sgid`/`dgid` at `+0x3c`/`+0x40`. `0x5a64a0` returns the
name and the two point values; `0x8c5cb0` consumes those points, `0x8c4ec0`
sums SP and DP, and `0x8aed20` renders the `1p_djpoint`/`2p_djpoint` widgets.
`0x5a6800` reads the grades; `0x8772b0` and `0x876270` select labels from RVA
`0x1077550`: 0..6 = 7..1 KYU, 7..16 = 1..10 DAN, 17 = CHUDEN, 18 = KAIDEN.
Negative/unknown grade IDs remain absent.

The sampler requires both initialization flags at `0x10b90e8/+4`, a joined
side at `0xacd79b0 + side*4`, normal play type at `0x6c11820 + side*stride`,
and profile-ready byte `0x312771e + side*stride`. Private session identity at
`0x6c11808 + side*stride` is checked for presence and compared before/after
sampling, but never serialized or logged. The loader clears the player block
in `0x5abed0`. A second profile copy rejects torn updates; guests, logout and
failed reads clear the corresponding player rather than retaining cached data.

Weekly network data is `week_data[play_style]/week[week_index]` in the same
loader. With `root = 0x10b90f0 + side*stride`:

| Field | Offset from root | Native getter |
| --- | --- | --- |
| Week ID / date | `0x206bc94/+index*4`, `0x206bca8/+index*4` | `0x5a5530` |
| Per-style, per-week block | `0x206bcbc + style*620 + index*124` | below |
| music_num | block `+0` | `0x5a5650` |
| keyboard_num | block `+112` | `0x5a55c0` |
| scratch_num (not keys) | block `+116` | `0x5a5700` |
| Block present | block `+120` | same getters |
| Activity/week ready | `0x206df8c`, `0x206c194` | same getters |

The current week is **index 4**, verified in `0x6f80c0`, the explicit `r9d=4`
argument in `0x6f8000 → 0x661e30`, and graph `0x723eb0`.
These are game week buckets, not a rolling seven-day history. Date encoding
and reset timezone are not inferred. Native display also adds the current
session counters: singleton pointer at `0xaab3cb0`, side block `+128+244*side`,
style block `+120*style`, songs `+4`, keys `+116` (`0x661ca0`, `0x661c50`).
`0x662320` clears this side block; `0x6620e0` saves counters through `0x59cc80`
and `0x59cc10`. These deltas must not be replaced with the plugin's own `plays`
counter or original chart note counts. Double reads reject changing counters,
week IDs and singleton pointers. An unreadable/missing week remains null.

Six getter guards independently disable player sampling on incompatible code;
core song/queue functions remain available. Typed parsing tests use synthetic
memory, including a heap below the image base, 2P, absent statistics, guests,
logout and a card change during sampling. `smoke-dll.py --user` exercises the
actual DLL's worker-to-HTTP path with synthetic 2P memory; `--bad-user` verifies
that changing a getter guard only disables this optional feature.
Real values, weekly rollover, point
updates and logout timing still require validation in the supported game.

## Arena availability in Battle Mode

Static IDA/Hex-Rays analysis of the same binary establishes both a server gate
and local restrictions. This is evidence only; no Arena patch, hook or API is
installed. A separate server-response probe is recorded below; a running game's
effective values were not sampled.

`0xae0f00` registers `IIDX33gameSystem`, with the `systemInfo` method descriptor.
Its response parser at `0xae0f40` reads `arena_schedule`. The schema at
`0x1094330` describes `season`, `phase`, `rule_type`, `start` and `end`: the first
three occupy bytes 0/1/2, and the last two occupy 32-bit slots at bytes 4/8.
The parser copies this twelve-byte record to response-buffer offset 3324.
Successful system-info completion at `0x5bdcc0` copies its phase byte from
`0x1e7ce3c + 1` to `0xa7af969`, and sets the system-info-ready byte at
`0xa7b0f28`. The getter at `0x59e9e0` returns zero unless that ready byte is set.

The effective-phase getter at `0x8675d0` then applies a local override:

```text
phase = system_info_ready ? cached_arena_schedule.phase : 0
if event_mode_enabled && phase >= 2:
    phase = 2
```

This setting is specifically **EVENT MODE**: getter `0x5d2ef0` reads
`CGameSettings + 16`; `0x5d3400` tests it against zero. Test-menu renderer
`0x98c560` pairs that getter's display values (`NO`, `YES[4 MIN]`,
`YES[6 MIN]`, `YES[8 MIN]`) with the `EVENT MODE` entry in table `0xdb4bd0`.
Do not confuse it with the separate free-play setting.

Mode-selection initialization at `0x8e2c00` inserts Arena (ID 0) into its
allowed Battle-mode choices only if the participant count is one and
`0x867dc0` accepts the effective phase. That predicate is an unsigned
`phase - 2 <= 2`, accepting exactly 2, 3 and 4. Participant getter `0x949480`
counts the two flags at `0xacd79b0/+4`, but treats joined DP as one player;
two simultaneous SP players do not pass. An earlier `CFreeGameData + 9`
branch restricts the entire mode list to Free (ID 3); its business meaning
has not been established and does not explain an otherwise visible Battle
submenu. BPL uses its own predicate, so BPL availability does not prove that
Arena's phase is valid.

| Effective phase | Arena entry (one participant) | Arena Online choice |
| --- | --- | --- |
| 0, 1 | Disabled | Unavailable |
| 2, 3 | Enabled | Disabled; Local remains selectable |
| 4 | Enabled | Enabled |
| Other values | Disabled | Unavailable |

The renderer at `0x8e4790` shows `bn_not_select_arena` when Arena is absent
from the allowed-choice vector. It independently masks the Online choice
unless effective phase equals 4. Input handling at `0x8e7cd0` enforces the
same restriction, using choice 0 for Online and 1 for Local. Thus enabling
EVENT MODE can disable Online while preserving the Arena entry; it does not
by itself explain the entire Arena entry being disabled.

The entry predicates consume the server-provided phase directly, rather than
calculating it from the local date and the schedule's start/end fields.
`0x867520` converts those timestamps for the schedule display. The phase
table above describes observed menu behavior, not guessed official names for
the protocol's enum values. It also does not establish that matchmaking will
succeed after the menu becomes selectable.

For a disabled Arena entry with Battle still visible, inspect the actual
`IIDX33gameSystem.systemInfo` response's `arena_schedule/phase`, the ready
byte, cached phase and participant count. For a disabled Online choice,
also inspect EVENT MODE. These checks distinguish server configuration or
response-loading failures from local menu restrictions without changing code.

## Server-controlled FPS and synchronization correction

Static analysis of this same build confirms that `IIDX33pc.common` supplies
switches for client-side FPS/synchronization correction. The server does not
provide a numeric target FPS through these fields. The static investigation did
not modify the game. A separate server-response probe and optional local patcher
overrides are recorded below; effective flags in a running game were not sampled.

`0xacd5d0` registers the `IIDX33pc` method table, whose `common` entry uses
response parser `0xaba780`. That parser checks for the **presence** of child
nodes, writing byte 1 when present; it does not parse their text or a boolean
attribute. A node whose contents say zero therefore still takes the enable
branch. Absence does not explicitly clear a flag in this parser; do not assume
that removing a node during a running session disables a previously set flag.

| Child node | Common request pointer offset | Effective byte RVA | Getter RVA |
| --- | --- | --- | --- |
| `fps_fix` | 2000 | `0xa78e1cf` | `0x5ab970` |
| `save_unsync_log` | 2008 | `0xa78e1d0` | `0x5a7a70` |
| `fix_framerate` | 2032 | `0xa78e1d3` | `0x5a2900` |
| `fix_real` | 2040 | `0xa78e1d4` | `0x5a2910` |

Request setup `0x5b7740` builds the pointer table for common data. Successful
completion `0x59adf0` copies the staging block into the effective common block,
which explains why direct cross-references to the effective flag bytes only
show their readers. The table, parser and completion copy establish the mapping
above without relying on the names alone.

`fps_fix` enables the write in `0x90de80`. Its observed callers require native
definition type 3. With a nonzero frame count and more than 40,000 ms elapsed,
the function compares elapsed time with `frame_count * 1000 / current_fps`.
An integer absolute difference above 16 ms increments a consecutive-error
counter; a valid check without that error resets the counter. At count >= 2,
and only with `fps_fix` enabled, it calculates `frame_count * 1000 / elapsed_ms`,
clamps that value to **[118.5, 121.5]**, and writes it to both the runtime FPS
field and `CLocalInfo + 12` through `0x5d4c50`. `save_unsync_log` also permits
the measurement path via `0x90efc0`, but does not satisfy the FPS-write gate.
The write and floating-point argument were checked against the assembly.

`fix_framerate` controls another write in sequencer tick `0x824b00`, in its
end-of-playback branch. It requires a recorded synchronization error, more than
20 nominal seconds of ticks, an additional native counter check below 2, and
an absolute error greater than 17 ms. With `F` the FPS captured at chart load,
`E` the accumulated error in milliseconds and `N` the tick count, it computes:

```text
corrected_fps = F / (1 + F * E / (1000 * N))
```

This path writes runtime FPS only when the result is in **[118.5, 121.5)**.
The extra counter's complete semantics have not been established. The sampled
sound position is obtained through `0x5c5da0(1)` and compared with chart time.

`fix_real` selects how that error is accumulated in `0x824b00`: when sound
position leads chart time by over 17 ms, the enabled branch adds one frame's
duration to the accumulator, while the disabled branch assigns the measured
position difference. The extra timeline-frame advance occurs in either branch;
this flag must not be described as enabling all frame catch-up. A chart-time
lead above 34 ms has a separate error-recording branch.

The corrected runtime field is read by `0xa59d50`; chart preparation
`0x8245c0` captures it and chart timing conversions consume it. Thus these are
functional timing changes, not display-only FPS labels. The analysis establishes
internal timing calibration around 120 FPS, not a change to the monitor's
refresh rate or a guarantee of higher rendering performance.

The `IIDX33music.reg` request serializer at `0xadc8b0` separately emits
`fix_sound_unsync` telemetry (`is_fix`, `fps_before`, `fps_after`, `unsync_ms`,
`unsync_cnt`) and `fix_frame_unsync` telemetry (`fix_framerate`, `fix_realtime`,
`unsync_ms`) when their enclosing guards allow it. These are outgoing logs;
the incoming switch is named `fix_real`, not `fix_realtime`.

## Read-only server-response probe (2026-10-11)

Using the user-supplied server and PCBID, a standalone XML client queried
`services.get`, then the advertised `local` endpoint for `IIDX33pc.common` and
`IIDX33gameSystem.systemInfo`. The request model was the repository-recorded
`LDJ:J:D:A:2026081900`; it was not captured from a running game. Both game
responses returned HTTP 200 and module `status="0"`.

The system-info request contained `ver` as `s32` with value **101**, following
request setup `0x5b45a0`, getter `0x95c4d0`, and serializer schema `0x1093ec0`.
The common request has no method-specific fields (serializer `0xabbf10`).

- The common response omitted all four nodes: `fps_fix`, `save_unsync_log`,
  `fix_framerate` and `fix_real`. This response therefore does not enable those
  flags; it does not establish the values already cached in a running game.
- The system-info response supplied `arena_schedule` with `phase=4`,
  `rule_type=0`, `season=1`, `start=0`, and `end=0`. Phase 4 passes the analyzed
  Arena/Online phase gates, subject to local conditions and successful loading
  of the response. Zero schedule timestamps do not fail those menu predicates.

These are observations of one server at one time, not protocol defaults or proof
of working matchmaking. No game process was available for checking cached flags,
participant count or EVENT MODE. Server identifiers and PCBID are intentionally
excluded from the repository; redacted response artifacts remain in ignored
`analysis/server-config-check/`. No score/profile write or registration request
was made.

## Optional Spice patcher overrides for timing switches

For the exact image identified above, Spice's per-PE patch file is
`patches/LDJ-6a713632_b1cccc.json`: the identifier combines the PE timestamp
and entry-point RVA. Three `type: "union"` entries provide `Server controlled
(Default)`, `Force ON`, and `Force OFF`. Each replaces only the corresponding
eight-byte leaf getter. Spice JSON `offset` is a **file offset**, not an RVA.

| Switch | Getter RVA | File offset | Original eight bytes |
| --- | --- | --- | --- |
| `fps_fix` | `0x5ab970` | `0x5aad70` (5942640) | `0FB60558281E0AC3` |
| `fix_framerate` | `0x5a2900` | `0x5a1d00` (5905664) | `0FB605CCB81E0AC3` |
| `fix_real` | `0x5a2910` | `0x5a1d10` (5905680) | `0FB605BDB81E0AC3` |

All three getters originally perform `movzx eax, byte ptr [cached_flag]; ret`.
`Force ON` uses `B8010000009090C3` (`mov eax, 1; nop; nop; ret`), while
`Force OFF` uses `B8000000009090C3`. The default option restores each getter's
original bytes. No cached data is overwritten, and later common responses do
not override a forced getter. The original calibration thresholds, cabinet
checks and accepted FPS ranges remain in place. `fix_real` still selects only
the error-accounting branch described above; it does not gate all catch-up.

These are intended for the analyzed 120 Hz timing paths. Change selections before
playing and restart the game for a clean comparison: stopping future correction
does not restore an FPS value already modified by an earlier correction.
`save_unsync_log` is not overridden. The score-request preparation code at
`0x5aff40` reads the cached `fix_framerate`/`fix_real` flags directly, so outgoing
telemetry can still describe the server settings instead of the forced getter
values. These patches do not make server logs a reliable override-state check.

Validation checked the installed DLL's full SHA-256, original bytes, PE section
translation, non-overlap with all existing patch entries, and JSON union layout.
In an isolated process, `LoadLibraryExW(DONT_RESOLVE_DLL_REFERENCES)` mapped the
image without invoking its entry point or resolving imports. Executing only
these three getters passed 24 combinations of server flag, override selection
and restoration; cached flags were unchanged. The original 41 patch-file entries
were preserved, a backup was saved, and the DLL on disk remained byte-identical.
Artifacts are under ignored `analysis/fps-patcher/`. Actual Spice UI selection
and gameplay timing have not been tested.

## Validation boundary

The runtime installer and scripts/smoke-dll.py check the file hash, architecture, function guards and
selection/search vtable targets and the database accessor. Automated tests cover
request state, transport, expanded native snapshots, dictionary layouts and fuzzy
matching of native keywords;
the DLL smoke test maps the game image without resolving imports or running its
entry point, then tests hook installation. This is not a live gameplay test.
development.md lists the remaining interactive checks, especially frame timing,
song-only jumps, chart availability, and coexistence with installed hooks.
