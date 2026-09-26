# 技术参考与开发指南

日常安装、点歌和常见问题请看 [使用手册](../README.md)。本文保留连接方式、版本校验、构建发布和详细日志说明，供开发与深入排错使用。

## 本地网页界面

`src/overlay.rs` 使用 Hyper 提供只读 HTTP 服务，默认只监听 `127.0.0.1:32133`。
HTML、CSS 和 JavaScript 位于 `web/`，编译时嵌入 DLL，无前端构建步骤、CDN 或外部字体依赖。
`/` 是可切换示例/实时数据的预览页；`/queue`、`/interaction` 是透明背景的 OBS 来源。
两种来源默认读取实时数据，只有显式指定 `?demo=1` 才使用示例。

工作线程从 Engine 发布独立 JSON 快照，`/api/state` 只读该快照，不访问游戏内存、
配置文件或游戏线程。快照包含当前点歌、队列、候选、剩余秒数和最近提示，不含身份码、
Cookie、应用凭据或观众平台 ID。网页每 500 毫秒读取一次，曲名和昵称通过 `textContent`
写入 DOM，倒计时更新不重建整张卡片。连接持续失败时清空旧数据，成功后自动恢复。

队列最多显示前 6 首；候选按每页 2 位观众、6 秒一页轮换。超过 6 个候选时每页显示
1 位观众并使用紧凑排版。最近提示显示最后 2 条，其过期由 Engine 控制。长文本仍可能
需要用户增加 OBS 来源高度。展示层不修改排队、选择期限或跳转行为。

服务器仅允许明确的资源路径与 GET/HEAD，请求 Host/Origin 限制为本地地址，禁用缓存，
每次连接只处理一个请求，上限 32 个并发连接、5 秒超时。启动失败会记录日志并保留文本
输出；关闭时中止监听及连接任务。`[overlay]` 设置兼容旧配置，默认开启。

无需游戏即可预览实际嵌入的页面：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -Command "& ./scripts/build.ps1 -CargoArgs @('run','--example','overlay_preview')"
```

打开输出的网址，完成后按 Ctrl+C 关闭预览服务，释放端口。自动 HTTP/快照测试位于
`tests/overlay.rs`。可选的浏览器检查需要 Python Playwright：启动预览后运行
`py scripts/check-overlay.py --browser "C:/Program Files/Google/Chrome/Application/chrome.exe"`。
不提供 `--browser` 时使用 Playwright 安装的 Chromium；截图保存在忽略的 `analysis/overlay/`。
`scripts/smoke-dll.py` 也会检查真实 DLL 启动 HTTP 服务、提供页面/状态，以及关闭后释放端口。

## Bilibili connection

The default **Open Live** mode follows blivechat: it sends the broadcaster identity
code to the configured `https://api1.blive.chat` public API to start a session, then
receives messages directly from Bilibili's secure WebSocket servers. This uses
blivechat's hosted service; it does not install or launch that application.
Its availability and Bilibili's live-session limits still apply.
The default service alternates between `api1.blive.chat` and `api2.blive.chat`
on reconnect. The old `https://blive.chat` setting is automatically mapped to
these API endpoints; the website itself does not serve this API. Custom relay
URLs and direct Open Live access are kept as configured.

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
[游戏适配分析](game-analysis.md). **Static analysis, automated tests,
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

## Automated releases

Pushing a tag runs [Build and release](../.github/workflows/release.yml) on a Windows
x64 runner: formatting, tests and Clippy must pass before building the release ZIP.
The ZIP is kept as an Actions artifact and uploaded to the GitHub Release for that
tag. An existing release receives the rebuilt asset when the workflow is rerun.

The archive name follows the package version in `Cargo.toml`, for example
`chart-requester-0.1.0.zip`. Update the package version and `Cargo.lock` before
tagging a new version, then push the tag:

```powershell
git tag v0.1.0
git push origin v0.1.0
```

You can also run the workflow manually from the Actions tab to build and download
the ZIP without publishing a release. Publication uses GitHub's built-in
`GITHUB_TOKEN`; no personal token or extra repository secret is required.
See GitHub's [tag push trigger documentation](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#push)
and [release CLI documentation](https://cli.github.com/manual/gh_release_create).

## Diagnostic logging

`chart-requester.log` beside the DLL now records timestamps in local time. Existing
configs automatically get the new defaults: `level = "debug"`, `danmu = true`,
10 MiB per file, three rotated backups (`.log.1` through `.log.3`), and a status
summary every 30 seconds. See `[logging]` in the Chinese example config.

- `[connection]` and `[transport]`: API attempts, session creation, socket
  authentication, heartbeats, reconnect reasons and session cleanup.
- `[danmu]`: received message sequence number, sender ID/name and actual text.
- `[request]`: the matching sequence number and processing outcome, including
  `ignored_not_a_request`, `ignored_catalog_not_ready`, `awaiting_selection`,
  `enqueued`, rejection reasons, candidates and timeout/queue notices.
- `[jump]`: submission token, song/chart and the game's acknowledgement.
- `[status]` and `[game]`: connection status, received/handled counts, queue,
  pending/current requests, waiting reason, selection phase and callback counts.
  `handled` counts messages recognized as requests or pending selections,
  including rejected requests; it is not the number of successfully played songs.

If OBS says the song database is not ready, inspect `select_entries`,
`select_updates`, `database_attempts` and `database_error`. Zero callbacks mean
the supported selection scene has not been observed. A nonempty database error
identifies the failed read/header/size check; `select_hooks_intact=false` means
the installed selection callbacks have been replaced. These diagnostics help
distinguish waiting for the selection screen from a hook/database failure.

The live database now comes from the game's native accessor, including the
verified Omnifix relocated-buffer patch, rather than a fixed buffer address.
`index_loads`, `index_entries`, and `index_error` track the native search dictionary.
`[catalog] Native search index captured` confirms its keywords have reached the
fuzzy matcher. Zero index loads only means the dictionary callback has not run;
canonical titles/readings still work once the song database is ready.

Set `danmu = false` to omit individual chat bodies and sender details; request
notices still contain song titles and requester names. Set `level = "info"` to
omit detailed transport/chat traces, or `"off"` to disable routine logging.
Startup failures are still logged. Authentication packets and raw API responses
are never logged; configured identity codes, access keys and cookies are redacted,
even if they appear in chat. Control characters are escaped to keep each event
on one line. Restart the game after changing logging settings.

Modern Spice SDK shutdown callbacks close the chat session. On older loaders or
forced process termination, Bilibili expires the session through its heartbeat TTL.
The DLL remains mapped until process exit; runtime unloading is not supported.
