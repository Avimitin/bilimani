# Agreed behavior

- One Rust DLL loaded by Spice `-k`; no installed companion or playlister dependency.
- SQLite beside the DLL, with built-in defaults and GUI-only configuration;
  paths relative to that directory unless absolute. Import legacy TOML only when
  initializing a database. JSON backups are explicit GUI import/export operations.
- `点歌 <song name> [SPB|SPN|SPH|SPA|SPL|DPB|DPN|DPH|DPA|DPL]`.
- Without difficulty, retain current SP/DP mode and use the game's song-only jump.
- Reject requests for the opposite mode or a nonexistent chart.
- Fuzzy subsequence search, configurable candidate count (5), GUI alias editor.
- Multiple matches require a numbered reply from the same sender, with a configurable
  60-second deadline. One pending selection per sender; a new request replaces it.
- Everyone may request. Optional per-user cooldown starts on successful enqueue;
  disabled by default. A full queue rejects new requests.
- FIFO waiting queue, configurable capacity. Pop only after a successful jump;
  retain the item separately as the current request.
- Automatically jump once on entering song select, or upon a request when idle there.
- Playing any song consumes/skips the current request; advance on return.
- In single-player SP song select, double-tap the opposite Start to toggle the egui
  control panel, including with an empty queue. Read-only Spice SDK input. Default 400 ms
  between presses, configurable in the controls page, enabled by default. Requires a
  release between presses; unavailable SDK, DP, two joined players and noninteractive
  screens disable this shortcut. Each gesture is tied to the active side and scene.
- Navigate on the logged-in side: B1 next item, B2 previous item, B6 confirm,
  B7 back, scratch left/right. While open, that side's seven keys and turntable
  are captured after the native input poll, preventing simultaneous song-select
  actions. Start, the other side and service keys remain unchanged. Held menu
  keys stay suppressed until release when closing. Free text uses keyboard/paste.
- The panel has live chat (last 500 text messages) and an editable queue, a fuzzy
  alias table, and categorized settings. Delete waiting entries by token; reject
  deletion of an in-flight jump. Pick any waiting song by mouse or controller;
  validate its token, selection epoch, mode and absence of an in-flight jump.
  The queue and event log sit left of live chat. Opening the panel returns to the
  live page and focuses the first selectable queued song for one B6 confirmation;
  an empty/unavailable queue falls back to its sidebar entry. Queue buttons
  receive controller focus and scroll into view. On successful
  acknowledgement, close the panel and remove only that request, replacing the
  current song. Failed/cancelled manual picks preserve the queue and current song.
  Automatic jumps wait while the panel is open; gameplay closes the panel.
- Apply/save validates aliases, output paths and the new HTTP listener before an
  SQLite transaction. Existing requests/deadlines survive within the same profile. Connection changes
  stop the old transport before starting the new one. Native module/catalog settings
  can be edited in the GUI but take effect after restart. Concurrent database
  updates require an explicit refresh before saving. JSON import only stages a
  draft; apply uses the same validation. Export includes credentials, excludes
  unsaved edits/chat/queue, and refuses to overwrite an existing file.
- A global stream profile serves unbound cards, guests and ambiguous two-player
  contexts. A logged-in unbound player can create a room from the panel; saving
  binds its settings to that card. Profiles support names, multiple cards, removal
  and deletion. Duplicate card assignments are rejected. Edits are drafts until
  saved; login-bound saves reject a changed login. Selecting a profile to edit
  does not change the active connection. Profile switches cancel pending native
  selections and clear queue/current/pending/cooldowns/chat/processing history;
  monotonic tokens reject late acknowledgements. Cards sharing a profile do not
  clear state or reconnect. SQLite v1 migrates to v2; JSON backups include all
  profiles/cards and accept the previous version. Card IDs remain outside OBS.
- The control panel header displays the active room's anchor, title and connection
  state on one line. Long titles truncate with a full-text tooltip. Metadata refreshes
  every minute alongside the socket; failures preserve the last known information
  and do not block chat. Source changes clear old metadata and discard late events.
- Current request expires 600 seconds after a successful jump, configurable.
  Expiration can advance while still in song select; never jump during gameplay.
- Separate UTF-8 OBS files for current/waiting requests and interactions.
- Optional loopback HTTP service with a compact queue browser overlay and bottom
  interaction popup, enabled by default on port 32133. HTML/CSS/JS ship next to the
  DLL in `chart_request_static/` inside the release ZIP. `overlay.static_dir` can
  select another public asset directory, relative to the DLL or absolute. Read
  files per request so replacements take effect on refresh; `/api/state` retains
  live snapshots. Directory changes validate before save and reuse the listener.
  Existing text files remain available; web bind failures do not stop requests.
- Queue and pending selections are not persisted across game restarts.
- Game share is read-only. Analysis uses local copies, excluded from git and releases.
