# 技术参考与开发指南

日常安装、点歌和常见问题请看 [使用手册](../README.md)。本文保留连接方式、版本校验、构建发布和详细日志说明，供开发与深入排错使用。

新增游戏、游戏版本或直播平台前，请先阅读 [适配层与扩展方式](architecture.md)。核心只依赖统一契约，版本选择集中在注册入口。

## 本地网页界面

`src/overlay.rs` 使用 Hyper 提供只读 HTTP 服务，默认只监听 `127.0.0.1:32133`。
HTML、CSS 和 JavaScript 源文件位于 `web/`，由 `scripts/package.ps1` 复制到 Release ZIP
中的 `chart_request_static/`，与 DLL 并列。页面不再嵌入 DLL，无前端构建步骤、CDN 或外部字体依赖。
`overlay.static_dir` 默认为 `chart_request_static`；相对路径以 DLL 所在目录为准，也支持绝对路径。
可在「OBS 显示」页修改并保存。只切换目录时复用已有监听端口，验证目录及 `index.html`
可读取后再提交配置；验证或保存失败保留原服务。旧数据库和 JSON/TOML 配置自动补入默认值。
`/queue` 和 `/index.html` 读取该目录的 `index.html`；`/` 重定向到 `/queue`。
默认页面背景透明，包含队列及底部按需弹出的交互区域。其他 URL 读取目录内对应资源，
支持子目录及常见网页、图片、字体 MIME 类型；不提供目录列表。文件修改在下一次请求时生效，
缺失文件返回 404，不回退到内嵌页面。启动时缺少目录或入口会记录错误，点歌和文本输出继续工作。
页面始终读取 `/api/state` 的实时数据，不包含示例模式或预览控件。

工作线程从 Engine 发布独立 JSON 快照，`/api/state` 只读该快照，不访问游戏内存、
配置文件或游戏线程。快照包含当前点歌、队列、候选、剩余秒数和最近提示，不含身份码、
Cookie、应用凭据或观众平台 ID。网页每 500 毫秒读取一次，曲名和昵称通过 `textContent`
写入 DOM，倒计时更新不重建整张卡片。连接持续失败时清空旧数据，成功后自动恢复。

队列最多显示前 6 首；底部候选按每页 1 位观众、6 秒一页轮换。普通通知只显示最新
1 条，以通知的时间与内容识别新事件，出现 6 秒后隐藏（Engine 提前移除时同步隐藏），
轮询不会重复弹出同一事件。无候选、通知或连接警告时，整个弹出区隐藏并释放占用高度。
长文本或较多候选仍可能需要用户增加 OBS 来源高度。展示层不修改排队、选择期限或跳转行为。

服务器仅允许 GET/HEAD，请求 Host/Origin 限制为本地地址，禁用缓存。
URL 解码后拒绝路径穿越、隐藏文件及 Windows 特殊路径；解析符号链接和目录联接后仍需位于
静态目录内。单个静态文件最大 32 MiB；每次连接只处理一个请求，上限 32 个并发连接、5 秒超时。
启动失败会记录日志并保留文本
输出；关闭时中止监听及连接任务。`[overlay]` 设置兼容旧配置，默认开启。

无需游戏即可预览源码中的实际页面（默认读取 `web/`，也可在 Cargo 的 `--` 后传入其他静态目录）：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -Command "& ./scripts/build.ps1 -CargoArgs @('run','--example','overlay_preview')"
```

此工具仅用于开发，不随 DLL 打包；打开输出的网址可查看未连接游戏的状态。
浏览器检查脚本通过拦截 `/api/state` 提供测试数据，模拟数据不进入 DLL。
完成后按 Ctrl+C 关闭本地服务，释放端口。自动 HTTP/快照测试位于
`tests/overlay.rs`。可选的浏览器检查需要 Python Playwright：启动预览后运行
`py scripts/check-overlay.py --browser "C:/Program Files/Google/Chrome/Application/chrome.exe"`。
不提供 `--browser` 时使用 Playwright 安装的 Chromium；截图保存在忽略的 `analysis/overlay-compact/`。
`scripts/smoke-dll.py` 也会检查真实 DLL 启动 HTTP 服务、提供页面/状态，以及关闭后释放端口。

## Controller menu and live configuration

`src/games/iidx/controls.rs` samples the opposite Start through the host bridge
in `src/host/spice.rs`, using SDK v0.1 `get_button` (table
slot 3, IIDX Start IDs 14/26). The SDK getter is kept behind an `RwLock`; shutdown
disables hooks and clears it while waiting for any active read to finish. Missing
SDK support or nonzero status disables gesture recognition without stopping chat.

Native selection updates sample input every frame, only when the normal selection
gate is open, SP is active and exactly one side is participating. `DoubleTap`
requires two rising edges within the configured window and keys its state to the
selection epoch and active side. Holding on entry, switching sides, leaving
selection, opening a modal or a failed read resets the gesture. A release and two
new presses can toggle again; it works without any current request.
The original game update always runs; Start is not overridden.

Panel navigation uses the logged-in side's native input snapshot, captured after
the IIDX input manager's original poll. B1/B2 have edge/repeat navigation;
B6/B7 are edge-only; scratch generates rate-limited left/right events. While
visible, only that side's seven button bits and turntable state are suppressed
for the game. The version module owns all offsets and emits generic `Navigation`
events; egui never reads game memory. See [input evidence](game-analysis.md).

The adapter validates epoch/side and emits `toggle_menu`. The worker toggles an
egui panel; picking and deleting queued requests are explicit panel commands. Game
hooks/offsets do not cross into the UI or renderer.

Manual picks retain the original queue order until the adapter acknowledges the
selected token. Success replaces the current request and closes the panel;
failure leaves the request in place and reports the result. The worker keeps the
panel pending during selection. Song and delete buttons both accept controller
focus, scroll into view and support B6 confirmation, including long queues.

`src/host/menu.rs` uses SDK v0.4 `register_d3d9` (slot 17, 152-byte x64 table).
Old SDKs leave that pointer null and retain chat functionality. The renderer uses
egui 0.34 with Ouroboros UI components and a small fixed-function D3D9 painter; no shader compiler DLL or
separate process is required. Fonts load once from Windows (Microsoft YaHei and
Japanese fallbacks), without redistributing them. Rendering/input failures are
reported under `[gui]` and do not execute game commands.

`src/gui/design.rs` holds the shared theme. Ouroboros provides buttons, switches,
search/input fields, cards, badges and key hints. Its git revision is pinned;
Iosevka/Phosphor families also receive Windows CJK font fallbacks. Password fields
retain egui masking and integer controls retain their original integer types.

Run `menu_check` with the build script's `-CargoArgs @('run','--example','menu_check')`
to render all nine real pages in a hidden D3D9 window. It writes BMP screenshots
under ignored `analysis/menu-preview/` and exercises device Reset between pages.
This does not replace an in-game check of SDK callback ordering, keyboard/IME,
mouse input, Start gestures and compatibility with other overlays.

### 不启动游戏，交互调试面板

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/preview-menu.ps1
```

这会编译并打开独立窗口，复用 DLL 的真实 egui 页面、D3D9 回调和 Win32 输入桥，
连接真实直播间，使用真实曲库进行模糊匹配、候选选择和入队。无需启动游戏、复制 DLL
或刷卡登录。鼠标、键盘、中文输入、别名过滤、队列删除、设置保存均可操作。

首次运行会创建 `analysis/menu-preview/preview.db`，在「直播连接」页填写身份码并
点击「应用并保存」即可连接。后续运行沿用该配置。也可以直接指定已有配置和曲库：

```powershell
./scripts/preview-menu.ps1 -ConfigPath 'D:/IIDX/chart-requester.db' -DatabasePath 'D:/IIDX/data/info/music_data.bin' -Mode SP
```

保存会写入实际使用的 SQLite 数据库，启动终端会打印其路径。相对路径以数据库目录为准。
首次创建时会导入旁边同名的旧 `.toml`；旧文件保留不动，之后不再读取。
可以用「备份与恢复」页导入、导出 JSON，不必编辑文件。与游戏共享数据库时，保存
采用版本检查；另一窗口保存后先「刷新已保存设置」，不会自动覆盖该窗口的运行状态。
曲库优先使用 `-DatabasePath`，其次 GUI 的曲库路径；本工作区未指定时使用
`analysis/music_data_1.bin`，不存在则使用 `analysis/music_data.bin`。仅支持 IIDX 33
的曲库格式。`-Mode DP` 可检查 DP 匹配，默认 SP。

| 按键 | 预览操作 |
|---|---|
| F1 | 打开 / 关闭面板 |
| F2 / F3 | 模拟 B1 / B2：下一项 / 上一项 |
| F4 / F5 | 模拟转盘左 / 右 |
| F6 / F7 | 模拟 B6 / B7：确认 / 返回 |

在直播间发送 `点歌 <曲名> [难度]`，可在「弹幕与队列」页查看原文和处理日志：
入队、等待观众选择、拒绝原因或忽略原因。主播控制台不显示观众侧的候选列表。
独立模式不会执行跳歌，歌曲保留在等待队列，可
手动删除；不启动 OBS 服务或写入 OBS 文本。连接、匹配、别名校验、冷却和候选超时
使用正式逻辑。日志位于 `analysis/menu-preview/chart-requester.log`，凭据会脱敏。

修改 Rust 页面后关闭窗口，重新运行同一命令即可增量编译，无需重启游戏。退出时会
关闭直播会话。游戏内的原生搜索词典、解锁状态、按键 Hook 和跳歌只能在游戏中验证；
独立模式搜索曲库中的曲名、读音和配置别名。预览程序不随 DLL 打包。

仅生成九页截图并检查 D3D9 Reset：`scripts/preview-menu.ps1 -Check`。
自动检查完整绘制回调可运行 `menu_preview --hidden --frames 60`（通过 Cargo 的 `--`
传递参数）；这个模式不显示窗口，达到指定帧数后退出，仍按配置决定是否连接直播。
自动检查应使用单独的数据库：在 GUI 中关闭直播连接，或指定一个尚不存在、旁边也没有旧 TOML 的 `.db` 路径，使用未填写身份码的默认设置，避免占用真实直播会话。

Live saves validate before committing a database transaction and preserve existing queue state.
`tests/live_config.rs` checks migration, transactional rollback, concurrent revisions,
JSON round trips, rejected backups, deferred game settings, path validation, fuzzy
alias filtering, chat bounds and rendering every page. Engine tests cover stale
deletes, in-flight protection and preserving native search terms during alias updates.

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
Credentials stay in the local SQLite database and explicit JSON backups; they are
never included in OBS output or logs. SQLite is bundled into the DLL via rusqlite;
users do not install a database runtime. `user_version` controls schema upgrades,
and unknown/newer databases are rejected without resetting their contents.

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
summary every 30 seconds. Adjust them on the GUI logging page.

- `[connection]` and `[transport]`: API attempts, session creation, socket
  authentication, heartbeats, reconnect reasons and session cleanup.
- `[danmu]`: received message sequence number, sender ID/name and actual text.
- `[request]`: the matching sequence number and processing outcome, including
  `ignored_not_a_request`, `ignored_catalog_not_ready`, `awaiting_selection`,
  `enqueued`, rejection reasons, candidates and timeout/queue notices.
- `[jump]`: submission token, song/chart and the game's acknowledgement.
- `[input]`: enabled/window settings, eligible player side, SDK read status and
  double-tap token/epoch/acceptance. No card IDs are read or logged.
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
on one line. Apply logging changes in the panel.

Modern Spice SDK shutdown callbacks close the chat session. On older loaders or
forced process termination, Bilibili expires the session through its heartbeat TTL.
The DLL remains mapped until process exit; runtime unloading is not supported.
