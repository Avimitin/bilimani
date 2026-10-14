# DDR WORLD API

DDR uses the shared search, selection and queue endpoints in [api.md](api.md). `meta.game` is `ddr`, `major_version` is `WORLD`, and `build` is `2026022400`. Song IDs are DDR mcode values. Modes are SP/DP; charts are `bSP` (BEGINNER, case-sensitive), `BSP`, `DSP`, `ESP`, `CSP`, `BDP`, `DDP`, `EDP`, `CDP`.

`GET /api/v1/ddr/now-playing` returns `{song: {id,title,artist,charts} | null, selected_chart: {chart: {mode,id}, level} | null}` for selection/gameplay. `selected_chart` is the active side's actual native selection (model +4/+8, active side from session +8), captured on the game thread and retained during gameplay. Invalid or unavailable difficulty is null, never inferred from a queue request. The frame displays this one chart; the full song chart catalog remains available to search clients. `GET /api/v1/ddr/user` returns `{players: [{side,name}]}`; side is 1 or 2. Only participation is currently verified, so name is empty. No unverified player names, ranks, scores or statistics are exposed.

The catalog can include locked songs. Native selection requires a selectable song in the active folder/filter and checks requested chart availability. Failure is reported asynchronously as `unavailable`.
