# 技术参考与开发指南

日常安装、点歌和常见问题请看 [使用手册](../README.md)。本文保留连接方式、版本校验、构建发布和详细日志说明，供开发与深入排错使用。

新增游戏、游戏版本或直播平台前，请先阅读 [适配层与扩展方式](architecture.md)。核心只依赖统一契约，版本选择集中在注册入口。

## 本地网页界面

`src/overlay.rs` 使用 Hyper 提供只读 HTTP 服务，默认只监听 `127.0.0.1:32133`。
HTML、CSS 和 JavaScript 按样式存放于 `web/card/` 和 `web/mecha/`，每个目录都有完整的 `index.html` 及独立资源。
`scripts/package.ps1` 将样式目录复制到 Release ZIP 的 `bilimani_web/`，其中仅包含 `card/` 和 `mecha/`，不生成根目录页面副本或许可证目录。页面不再嵌入 DLL，无前端构建步骤、CDN 或外部字体依赖。
`overlay.static_dir` 默认为 `bilimani_web/card`；相对路径以 DLL 所在目录为准，也支持绝对路径。旧配置若仍指向 `bilimani_web`，请在设置中选择具体样式子目录。
可在「OBS 显示」页修改并保存。只切换目录时复用已有监听端口，验证目录及 `index.html`
可读取后再提交配置；验证或保存失败保留原服务。旧数据库和 JSON/TOML 配置自动补入默认值。
`/queue` 和 `/index.html` 读取该目录的 `index.html`；`/` 重定向到 `/queue`。
默认页面背景透明，包含点歌区与弹幕／事件区域，画布高度固定为 OBS 浏览器来源高度，未使用的空间透明。其他 URL 读取目录内对应资源，
支持子目录及常见网页、图片、字体 MIME 类型；不提供目录列表。文件修改在下一次请求时生效，
缺失文件返回 404，不回退到内嵌页面。启动时缺少目录或入口会记录错误，点歌和文本输出继续工作。
页面始终读取 `/api/state` 的实时数据，不包含示例模式或预览控件。

工作线程从 Engine 发布独立 JSON 快照，`/api/state` 只读该快照，不访问游戏内存、
配置文件或游戏线程。快照包含当前点歌、队列、候选、剩余秒数、最近提示与公开弹幕记录，不含身份码、
Cookie、应用凭据或观众平台 ID。网页每 500 毫秒读取一次，曲名、昵称和内容通过 `textContent`
写入 DOM，倒计时更新不重建整张卡片。连接持续失败时清空旧队列、显示连接提示，并保留已有弹幕记录；成功后采用服务端最新快照。

`/api/state.room` 返回当前已连接直播间的公开信息：`room_id`、`name`（UP 主名字）、`title`（直播标题）。
DLL 复用直播连接后台通过 B 站直播 API 获取并每 60 秒刷新的房间信息，网页不需要凭据或额外请求 B 站。
断开连接、切换档案尚未取得新信息时返回 `null`；曲库未就绪不会阻止房间信息显示。旧 DLL 没有此字段时，前端隐藏主播名字。

卡片样式的点歌区按可用高度显示最多 6 首等待歌曲，多余数量另行提示。有候选时隐藏当前点歌及队列，
在同一区域完整展示当前观众的 1–20 个候选；超过 8 个使用两列，按列从上到下编号。
选歌期间上方整块区域反转为主题色背景、原背景色文字，编号及倒计时强调在弹幕中回复编号；候选本身不分页。
多人同时待选时每 6 秒轮换整份列表，快照顺序变化保持正在显示的观众，候选全部消失后恢复最新队列与原配色。
空队列且无当前点歌／候选时，上方只保留 37px 标题；只有当前点歌时保留 132px 卡片。
队列或候选存在时恢复完整区域，高度以 320ms 动画过渡，正文同时淡入淡出；减少动态效果时取消动画。
下方将弹幕和事件按到达顺序混排，无记录时只保留 36px 标题；连接提示额外占用一行。
背景按实际弹幕数 × 46px、事件数 × 24px 和 3px 间隔计算高度，以 320ms 动画增减，最多占用剩余空间。
弹幕字号 16px、上下各 5px 内边距，事件保持 12px 字号与无上下内边距的窄卡片；空间不足时按比例压缩卡片。
达到保留上限后，新记录替换最早记录，背景随两种记录的数量调整。每张卡片根据可用高度显示一到两行文字；
ResizeObserver 在区域动画和来源尺寸变化时分别调整行数，避免半行裁切。画布总高度始终等于来源高度。

`overlay.history_limit` 默认 10，可设 1–100，GUI、数据库与 JSON 备份均支持。worker 持有
独立 `overlay::History`，收到弹幕立即写入，每次引擎动作产生的通知按发生顺序取出并记录；
即使曲库尚未就绪也接收普通弹幕。`/api/state` 的 `feed` 包含 `id/at/kind/name/text`，
其中 `kind` 为 `chat` 或 `event`，`id` 为本次运行的递增序号；`feed_limit` 为当前容量。
相同时间、相同内容仍是不同记录。超出容量或调小容量时从头移除；没有时间过期。
刷新网页从服务端恢复最近记录，切换直播档案时清空，不写入配置备份或持久存储。
旧 `notices` 字段与文本文件仍遵循原有时间／条数规则，便于已有自定义页面继续使用。

Mecha 的候选使用单列分页，每页 5 首，背景显示隔行扫描动画。`pending[]` 增加
`page`（从 0 开始）、`page_size`（5）和 `page_count`；`candidates` 仍返回完整有序列表，
数字选择仍为全局编号，兼容卡片样式与已有前端。引擎只接受点歌本人发送的 `n` / `p`
来切换其页码，不区分大小写，首尾不循环，不延长截止时间；新点歌重置为首页。
Mecha 优先展示刚翻页的观众，然后恢复每 6 秒轮换。刷新页面从快照恢复页码。
旧 DLL 缺少分页字段时按首页显示，但无法响应翻页命令；使用分页应同时升级 DLL。

服务器仅允许 GET/HEAD，请求 Host/Origin 限制为本地地址，禁用缓存。
URL 解码后拒绝路径穿越、隐藏文件及 Windows 特殊路径；解析符号链接和目录联接后仍需位于
静态目录内。单个静态文件最大 32 MiB；每次连接只处理一个请求，上限 32 个并发连接、5 秒超时。
启动失败会记录日志并保留文本
输出；关闭时中止监听及连接任务。`[overlay]` 设置兼容旧配置，默认开启。

### 循环时间轴预览（前端开发）

只需 Python 3.10 或更新版本，无第三方依赖、Rust、DLL 或游戏。在仓库根目录运行：

```powershell
py scripts/preview-overlay.py --style mecha
# 卡片版：py scripts/preview-overlay.py --style card
```

打开 **http://127.0.0.1:32134/queue**。预览服务同时提供实际页面与模拟 `/api/state`、
`/api/now-playing`，现有前端无需添加演示逻辑。Mecha 按 1920×1080 设计，Card 建议使用
480×800 视口；也可在 OBS 浏览器来源中填写此地址，查看合成效果。

默认时间表位于 `scripts/fixtures/overlay-loop.json`，每 **42 秒**循环一次：等待连接 →
弹幕出现 → 实际选曲与 BPM／难度／雷达 → 点歌与队列 → 9 个候选与 n/p 翻页 → 确认入队 →
开始演奏 → 历史超过 10 条 → 清空收起 → 再次出现消息 → 断开与下一轮。
歌曲参数全部为模拟值。每轮从初始状态重新构建，消息 ID 跨轮递增；多个浏览器标签共享
同一时间轴，刷新页面不会重置播放，也不会重复插入消息。

```powershell
# 两倍速；实际每 21 秒循环一次
py scripts/preview-overlay.py --style mecha --speed 2
# 选择另一份时间表及端口；也兼容原有录制时间表
py scripts/preview-overlay.py --style card --scenario scripts/fixtures/overlay-demo.json --port 32135
```

修改 `web/<样式>/` 的 HTML、CSS、JS 后刷新浏览器即可生效。新主题也可放在
`web/my-theme/`，然后传入 `--style my-theme`。修改时间表后重启脚本；Ctrl+C 停止。

时间表沿用录制脚本的格式，`at` 是每轮开始后的秒数：

```json
{
  "duration": 8,
  "feed_limit": 10,
  "steps": [
    {"at": 1, "feed": [{"kind": "chat", "name": "观众甲", "text": "晚上好！"}]},
    {"at": 3, "feed": [{"kind": "event", "text": "收到一条测试事件"}]},
    {"at": 6, "clear_feed": true, "set": {"current": null, "queue": [], "pending": []}}
  ]
}
```

`feed` 追加消息，`clear_feed` 清空消息，`set` 替换指定的顶层 API 字段（例如 `queue`、
`pending`、`now_playing`，不做嵌套合并）。`current` 和 `pending` 的 `remaining` 从该字段
最近一次设置时开始倒数；状态何时结束由后续步骤决定。步骤需按时间排序，且 `at < duration`。
完整歌曲信息结构见下方 API 文档。开发服务不写入游戏或配置数据库，也不随 Release 打包。

运行 `py scripts/test-preview-overlay.py` 检查循环边界、倒计时、消息上限、API 和静态文件刷新。

### Rust HTTP 服务预览与浏览器检查

需要验证 DLL 所用的实际 HTTP 服务时，可启动 Rust 预览例程（默认读取 `web/card/`，
也可在 Cargo 的 `--` 后传入其他静态目录）：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -Command "& ./scripts/build.ps1 -CargoArgs @('run','--example','overlay_preview')"
```

机甲框架预览可运行 `cargo run --example overlay_preview -- web/mecha`，然后运行 `python scripts/check-frame.py`（需要 Playwright 和 Pillow）。两种样式统一通过 `/queue` 访问，切换预览目录前先停止当前服务。

此工具仅用于开发，不随 DLL 打包；打开输出的网址可查看未连接游戏的状态。
浏览器检查脚本通过拦截 `/api/state` 提供测试数据，模拟数据不进入 DLL。
完成后按 Ctrl+C 关闭本地服务，释放端口。自动 HTTP/快照测试位于
`tests/overlay.rs`。可选的浏览器检查需要 Python Playwright：启动预览后运行
`py scripts/check-overlay.py --browser "C:/Program Files/Google/Chrome/Application/chrome.exe"`。
不提供 `--browser` 时使用 Playwright 安装的 Chromium；截图保存在忽略的 `analysis/overlay-history/`。
`scripts/smoke-dll.py` 也会检查真实 DLL 启动 HTTP 服务、提供页面/状态，以及关闭后释放端口。

### 当前选曲／游玩歌曲 API

`GET /api/now-playing` 返回实际游戏歌曲快照，无需有观众点歌。同一对象也在
`/api/state.now_playing` 中，页面一次轮询即可获得全部内容。已有的 `current` 字段继续表示
观众的当前点歌请求。两个接口均支持 HEAD，沿用本地 Host/Origin 检查及 `no-store`。

以下为字段示意，数值使用模拟数据；`song.charts` 实际包含该曲存在的全部谱面：

```json
{
  "phase": "selecting",
  "song": {
    "id": 33001,
    "title": "Example Song",
    "artist": "Example Artist",
    "genre": "TEST GENRE",
    "game_version": 33,
    "charts": [{
      "chart": {"mode": "SP", "id": "SPA"},
      "difficulty": "ANOTHER",
      "level": "12",
      "style": "red",
      "bpm": {"min": 100, "max": 200},
      "note_count": 2000,
      "radar": {"notes": 180.25, "peak": 145.5, "scratch": 65.75,
                "soflan": 120.0, "charge": 0.0, "chord": 150.01}
    }]
  },
  "players": [{"side": 2, "chart": {"mode": "SP", "id": "SPA"}}]
}
```

| 字段 | 含义 |
| --- | --- |
| `phase` | `idle`、`selecting`、`playing`；演奏结束清为 `idle` |
| `song` | 当前歌曲；启动、场景离开、选中目录或读取失败时为 `null` |
| `game_version` | 初出版本编号，异常值为 `null` |
| `charts` | 存在的 SP/DP B/N/H/A/L 谱面；等级为字符串，未解锁谱面也可能有元数据 |
| `bpm` | 谱面最小／最大 BPM；恒定 BPM 两值相同，尚未加载时为 `null` |
| `note_count` | 该谱面总音符数，尚未加载或异常时为 `null` |
| `radar` | 六项数值，`100.0` 对应原生雷达的 100% 参考环；缺失时为 `null`，允许超过 100 |
| `players` | 每个参与侧及其选中谱面；`side` 为 1 或 2，通过 `chart.id` 关联 `charts` |

普通选曲在原生更新完成后每 250ms 采集一次；切换难度和手动选歌都更新。弹窗期间保留最近
一次有效选曲，离开选曲清空。进入 stage 后从实际演奏上下文重新采集，不沿用上一首选曲；
stage 清理时立即清空。特殊选曲画面暂不采集选曲信息，其支持的 stage 仍可报告演奏曲目。
本版提供谱面 BPM 范围，尚未采集演奏进度、瞬时 BPM 或实时判定分数。

游戏回调只产生拥有独立内存的 Rust 数据；worker 每 100ms 发布快照，HTTP 请求不调用游戏。
没有可用元数据时仍返回 200 和空状态；网页应正常处理 `null`、空 `players` 与空文本。
Mecha 底栏展示曲名、曲风／作者、参与侧难度、BPM、音符数及雷达；两侧都参与时显示两份难度，
雷达注明其对应的首个参与侧。断线超过三秒清除歌曲，重连恢复。卡片样式维持点歌队列展示。

`tests/song_info.rs` 覆盖记录索引与缺失数据，`tests/overlay.rs` 覆盖 API 发布及清空。
`py scripts/smoke-dll.py --song-info` 使用私有映射与模拟原函数，验证真实 DLL 回调到 JSON 的
链路、SP/DP、目录、stage 切换与清理。`scripts/check-frame.py` 验证手动选曲、双侧难度、雷达、
缺失值、超长文本转义、断线恢复和画面边界。上述模拟测试不等于真实游戏运行验证。

### 按时间表录制网页演示

`scripts/record-overlay.py` 直接读取 `web/card/`，在独立浏览器中拦截所有请求，用 JSON 时间表提供模拟快照，
无需运行游戏或预览服务。安装开发依赖后运行：

```powershell
py -m pip install "playwright>=1.63" imageio-ffmpeg
py -m playwright install ffmpeg
py scripts/record-overlay.py --browser "C:/Program Files/Google/Chrome/Application/chrome.exe"
```

默认生成 35 秒、480×800 的 `analysis/overlay-demo.mp4`，涵盖空列表、弹幕与事件逐条增加、队列展开、
候选反色、超过十条后的替换，以及清空后重新展开。使用 H.264 编码；透明部分合成为深灰色，
可用 `--background "#000000"` 更换。也可通过 `--ffmpeg` 指定支持 libx264 的编码器。

复制并修改 `scripts/fixtures/overlay-demo.json`，再通过 `--scenario 路径 --output analysis/自定义.mp4` 录制：

- `duration` 为录制秒数，`width` / `height` 为画面尺寸，需为正偶数。
- `steps` 按 `at` 秒数排序；`feed` 追加弹幕或事件，`clear_feed: true` 清空历史。
- `set` 替换快照中的指定字段，例如 `current`、`queue`、`pending`；字段结构与 `/api/state` 一致。
- 当前歌曲和候选倒计时会随时间递减，实际展示仍由页面每 500ms 轮询触发；最后一步应预留至少 0.5 秒。

同名 `.timeline.json` 记录每一步实际应用时间。模拟内容只在录制脚本中存在，不会发送到直播间或游戏。

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

The Win32 input bridge preserves the window's ANSI/Unicode procedure type when
installing, forwarding and restoring its subclass. Spice2x's legacy touch
emulator directly calls the result of `GetWindowLongPtrA(GWLP_WNDPROC)` to send
`WM_TOUCH`. Installing a Unicode procedure on its ANSI window can make that
getter return a conversion token instead of a callable address. This caused the
2026-09-27 login-touch crash with Spice2x 2026-09-12: execute access violation at
`0xffff00fd`, called from `spice64.exe+0x320959` (`call rax` after the A getter).
The input bridge now matches the existing procedure type, and ANSI `WM_CHAR`
is decoded using the process code page, including split DBCS and UTF-8 sequences.
The separate Unicode callback retains UTF-16 handling.

`host::menu::input::tests` creates hidden ANSI and Unicode windows, exercises the
same direct touch dispatch, verifies forwarding and restoration, and tests text
decoding. The touch test reproduces the invalid procedure on the old bridge and
passes with the matching subclass. In callers that own touch dispatch, the
general Windows API requirement is to use
[`CallWindowProc`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-callwindowproca)
for values returned by `GetWindowLongPtr`; conversion tokens are not functions.

`src/gui/design.rs` holds the shared theme. Ouroboros provides buttons, switches,
search/input fields, cards, badges and key hints. Its git revision is pinned;
Iosevka/Phosphor families also receive Windows CJK font fallbacks. Password fields
retain egui masking and integer controls retain their original integer types.

Run `menu_check` with the build script's `-CargoArgs @('run','--example','menu_check')`
to render all nine real pages in a hidden D3D9 window. It writes BMP screenshots
under ignored `analysis/menu-preview/` and exercises device Reset between pages.
This does not replace an in-game check of SDK callback ordering, keyboard/IME,
mouse input, Start gestures and compatibility with other overlays.

### 独立配置 EXE

`cargo build --release --locked` 同时构建 DLL 和 `bilimani-config.exe`，发布 ZIP 包含两者。
EXE 复用游戏内 egui 页面、Win32 输入和 D3D9 渲染，默认打开自身目录下的
`bilimani.db`，不依赖当前工作目录，不建立直播连接或 OBS 服务。启动失败通过
Windows 对话框显示错误。关闭控制台或按 Esc 会退出 EXE。

独立配置使用自有 Win32 窗口类承载无边框 egui `CentralPanel`。`WM_NCHITTEST` 根据 egui
发布的标题区与按钮矩形区分拖动、按钮和八个缩放方向；最大化范围为当前屏幕工作区。
原生拖动／缩放消息循环通过定时器持续绘制，D3D9 Reset 与正常绘制共用同一状态并拒绝重入。
游戏内仍使用居中的 egui 浮窗及原有缩放策略。

```powershell
./target/release/bilimani-config.exe --config 'D:/IIDX/modules/bilimani.db'
```

离线别名校验读取明确指定的 `game.database_path` 或 DLL 在 `song_catalog` 表中缓存的
歌曲标题/ID。缓存按游戏模块与曲库路径分组，不增加设置 revision，也不写入 JSON 备份。
首次没有曲库仍可编辑直播档案和其他设置；可在「游戏适配」填写曲库文件再保存别名。
`tests/desktop.rs` 检查独立配置、手动卡号绑定、缓存、文件曲库、别名校验、并发更新和备份草稿。

无窗口检查：`bilimani-config.exe --config <测试路径.db> --hidden --frames 60`。
使用单独的测试数据库；此模式同样不连接直播间。
`py -3 scripts/check-desktop.py` 会启动隐藏窗口，检查标题／按钮命中、八个缩放方向、
负坐标、多次改变大小后的绘制与关闭。`--exe` 可指定 Debug 等其他构建。

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
./scripts/preview-menu.ps1 -ConfigPath 'D:/IIDX/bilimani.db' -DatabasePath 'D:/IIDX/data/info/music_data.bin' -Mode SP
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
使用正式逻辑。日志位于 `analysis/menu-preview/bilimani.log`，凭据会脱敏。

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

Stream profiles use SQLite schema 2 and JSON backup version 2. `tests/stream_profiles.rs`
covers schema 1 migration, atomic profile/card saves, conflicting revisions, backup
compatibility and global fallback. Engine tests cover queue/candidate/cooldown clearing
and late native acknowledgements when switching profiles. GUI tests exercise creating
a card-bound draft with controller input and a busy command channel. The standalone
live preview has no logged-in game card and therefore uses the global connection.

Render the personal-profile page with `cargo run --example menu_check -- analysis/menu-profiles --profiles`;
add `--unbound` to show the creation entry point. These fixtures use synthetic cards and
do not connect to live rooms. `py scripts/smoke-dll.py --profiles` tests actual DLL card
polling and profile switching in its private mapped game image, without executing game
code. Real card login/logout timing still needs an interactive game check.

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

The input poll at RVA `0xa7a2f0` also accepts the x64 MinHook relay used by
2dxtra: an `E9 rel32` replacing only the first five-byte instruction, followed
by unchanged signature bytes, with an executable `FF 25 00 00 00 00` relay
pointing into a loaded external executable module. The input vtable must still
point to the verified game entry. Our vtable wrapper calls that entry, preserving
the existing detour and its trampoline. All other function signatures and the
on-disk game hash remain checked. Unknown patches still reject startup and log
the observed bytes. `[input] Input poll chain:` reports `native` or
`MinHook -> <module>` as seen during installation.

After building the release DLL, `py scripts/smoke-dll.py --input-detour` checks
startup with a synthetic MinHook relay and calls through the installed input
wrapper to a harmless Windows function. `--bad-input-detour` checks that a
modified signature is rejected before any vtable is changed. Both use a private
mapped image from `analysis/bm2dx.dll`, with no game code or live plugins running.
Actual game startup and controller interaction with 2dxtra require an in-game check.

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

The release ZIP contains only `README.md`, `bilimani.dll`,
`bilimani-config.exe`, and `bilimani_web/` at its root.
The static directory contains only the `card/` and `mecha/` frontend styles.
Licenses, developer docs, showcase images, and recording tools are not packaged.

## Automated releases

Pushing a tag runs [Build and release](../.github/workflows/release.yml) on a Windows
x64 runner: formatting, tests and Clippy must pass before building the release ZIP.
The ZIP is kept as an Actions artifact and uploaded to the GitHub Release for that
tag. An existing release receives the rebuilt asset when the workflow is rerun.

The archive name follows the package version in `Cargo.toml`, for example
`bilimani-0.2.0.zip`. Update the package version and `Cargo.lock` before
tagging a new version, then push the tag:

```powershell
git tag v0.2.0
git push origin v0.2.0
```

You can also run the workflow manually from the Actions tab to build and download
the ZIP without publishing a release. Publication uses GitHub's built-in
`GITHUB_TOKEN`; no personal token or extra repository secret is required.
See GitHub's [tag push trigger documentation](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#push)
and [release CLI documentation](https://cli.github.com/manual/gh_release_create).

## Diagnostic logging

`bilimani.log` beside the DLL now records timestamps in local time. Existing
configs automatically get the new defaults: `level = "debug"`, `danmu = true`,
10 MiB per file, three rotated backups (`.log.1` through `.log.3`), and a status
summary every 30 seconds. Adjust them on the GUI logging page.

- `[connection]` and `[transport]`: API attempts, session creation, socket
  authentication, heartbeats, reconnect reasons and session cleanup.
- `[danmu]`: received message sequence number, sender ID/name and actual text.
- `[request]`: the matching sequence number and processing outcome, including
  `ignored_not_a_request`, `ignored_catalog_not_ready`, `awaiting_selection`,
  `selection_page_changed`, `ignored_selection_page_boundary`, `enqueued`,
  rejection reasons, candidates and timeout/queue notices.
- `[jump]`: submission token, song/chart and the game's acknowledgement.
- `[input]`: enabled/window settings, eligible player side, SDK read status and
  double-tap token/epoch/acceptance. Card IDs are not logged.
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
