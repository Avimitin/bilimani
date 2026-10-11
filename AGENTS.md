# Repository architecture rules

Read docs/architecture.md and docs/api.md before changing runtime boundaries. They describe the accepted architecture, not an optional roadmap.

- One game per Rust crate/DLL; versions/builds live inside that game crate. Only IIDX 33 / 20260812 is currently supported. Use bilimani_<game>.dll for DLL names, configured through [lib].name so build and release artifacts match.
- Use the bilimani- prefix for Rust packages/directories and bilimani_ for game DLL configuration, logs and alias files. TypeScript packages use the @bilimani/ scope; each page keeps its adjacent config.json.
- bilimani-api must never import a game crate or platform protocol. Queue payloads use game-neutral song/mode/chart identifiers and an optional display name, never platform credentials.
- DLLs own runtime catalogs, CSV aliases, fuzzy search, the shared in-memory bilimani-queue, game snapshots, and native selection execution. HTTP and the in-game menu operate on the same worker-owned queue.
- Keep the in-game menu limited to removing requests, prioritizing a queued song, and reloading the alias CSV. No platform settings, text editors, IME, or standalone configuration application.
- Platform connections, chat parsing, candidate confirmation and viewer cooldowns run in TypeScript inside the page. OBS pages display the authoritative DLL queue and do not schedule selections or provide management controls.
- Shared frontend controller and platform packages must not import application/DOM code. Game vocabulary belongs to the game application.
- Page refresh reconnects and reads the existing DLL queue. Only page-local candidates/cooldowns/chat reset. The queue is in-memory and is cleared on game exit; do not add persistence, replay, cache recovery, leader election, or multi-game coordination.
- Aliases are user-maintained UTF-8 CSV. Reload from the configured file on demand, validate the entire replacement against the live catalog, and retain the last valid table if reload fails. Never edit aliases in the game menu.
- Read the page's adjacent config.json once at startup with caching disabled. Do not import it into a bundle or fall back to localStorage. Ship ready-to-use default JSON/CSV files without .example names or first-run copying. Package defaults from repository sources, never developer runtime configurations; preserve local page configuration when rebuilding.
- Keep README focused on what the plugin does and how a non-developer installs and uses it. Use concrete spice2x releases and download links; put configuration details in docs/usage.md and implementation details in developer docs. Do not add migration instructions.
- Release ZIPs contain only game DLLs, runtime JSON/CSV configuration and static web assets. README, docs, AGENTS.md, LICENSE and THIRD-PARTY-NOTICES.md stay in the repository and are not packaged, including in standalone page ZIPs.
- Static frontends remain independently deployable with their own layout, configuration and platform adapters. When streamers choose to reuse a page, use the URL authCode parameter to identify the Bilibili connection; do not add streamer/profile mappings or require a shared layout. Identity codes must not appear in logs or Referer headers.
- Keep api_preview as the Rust development simulator: reuse real HTTP/queue/search code, provide synthetic telemetry and terminal controls, and keep it out of production DLLs/packages. Do not add a separate Node mock service.
- Use typed Rust error enums and explicit state transitions. Do not use strings as error categories or branch on error messages. Keep parsing/validation pure and effects at IO/native boundaries.
- HTTP handlers never access game memory or call native game functions. Keep worker ownership, game-thread mailboxes, and scene epoch validation.
- Keep docs and package/build scripts consistent with implementation. Historical release notes may describe old behavior but must not be presented as current instructions.
- Verify changes with relevant Rust/TypeScript tests and production builds. Do not claim real-game validation without actually running the supported game.
