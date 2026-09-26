# Agreed behavior

- One Rust DLL loaded by Spice `-k`; no installed companion or playlister dependency.
- TOML beside the DLL; paths relative to that directory unless absolute.
- `点歌 <song name> [SPB|SPN|SPH|SPA|SPL|DPB|DPN|DPH|DPA|DPL]`.
- Without difficulty, retain current SP/DP mode and use the game's song-only jump.
- Reject requests for the opposite mode or a nonexistent chart.
- Fuzzy subsequence search, configurable candidate count (5), TOML aliases.
- Multiple matches require a numbered reply from the same sender, with a configurable
  60-second deadline. One pending selection per sender; a new request replaces it.
- Everyone may request. Optional per-user cooldown starts on successful enqueue;
  disabled by default. A full queue rejects new requests.
- FIFO waiting queue, configurable capacity. Pop only after a successful jump;
  retain the item separately as the current request.
- Automatically jump once on entering song select, or upon a request when idle there.
- No hotkey. Playing any song consumes/skips the current request; advance on return.
- Current request expires 600 seconds after a successful jump, configurable.
  Expiration can advance while still in song select; never jump during gameplay.
- Separate UTF-8 OBS files for current/waiting requests and interactions.
- Queue and pending selections are not persisted across game restarts.
- Game share is read-only. Analysis uses local copies, excluded from git and releases.
