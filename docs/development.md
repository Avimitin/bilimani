# 开发、验证与发布

先读 [架构](architecture.md)、[API](api.md) 和根目录 [AGENTS.md](../AGENTS.md)。bilimani-iidx 支持 IIDX 33 / 20260812、34 / 010-0916、34 / 010-0917；34 为实验支持，尚未实机验证。bilimani-ddr 提供独立 DDR 实现；bilimani-api 不依赖具体游戏。根 Cargo.toml 是虚拟 workspace。

## 环境与构建

Windows x64、Rust stable（edition 2024）、Visual Studio C++ Build Tools 和 Windows SDK。前端使用 Node 22.12+ / npm；package-lock.json 和 Cargo.lock 应提交。

```powershell
npm ci
npm run typecheck
npm test
npm run build
./scripts/build.ps1 -CargoArgs @('test', '--workspace', '--all-targets', '--locked')
./scripts/build.ps1 -CargoArgs @('clippy', '--workspace', '--all-targets', '--locked', '--', '-D', 'warnings')
cargo fmt --all --check
./scripts/build.ps1
```

若系统 PowerShell 策略限制本地脚本，在当前终端设置 `Set-ExecutionPolicy -Scope Process Bypass`。build.ps1 设置 MSVC 环境后调用 Cargo，默认构建整个 workspace 的 release 产物。IIDX 的 Cargo package 为 bilimani-iidx，lib target 为 bilimani_iidx，游戏 DLL 为 target/release/bilimani_iidx.dll。

npm run build 独立打包 Card、Mecha，产物位于 dist/web/iidx/<style>/，每页自带资源与 config.json，不依赖另一布局。首次构建从 web/apps/<game>/config.json 写入默认配置，已有产物目录的 config.json 会保留。发布 ZIP 始终从源码复制公开默认值，不使用本地调试配置。dist/web/index.html 为入口索引。

## spice2x 版本依据

最低要求为 [spice2x 26-09-12](https://github.com/spice2x/spice2x.github.io/releases/tag/26-09-12)，发布于 2026-09-12（UTC），上游标记为 Pre-release。对应下载文件为 [spice2x-26-09-12.zip](https://github.com/spice2x/spice2x.github.io/releases/download/26-09-12/spice2x-26-09-12.zip)。

该版本收录 [PR #908](https://github.com/spice2x/spice2x.github.io/pull/908)，引入本项目游戏内菜单需要的 D3D9 绘制回调，对应 SDK v0.4。此最低版本依据上游发布记录和接口要求确定，不代表已完成该版本的实机回归；实机验证项见下文。面向玩家的 README 只列版本与下载入口。

## 不启动游戏的预览

安装前端依赖后，Windows 可双击 `scripts/preview.bat` 构建预览页面并启动 Rust 预览。它会切换到仓库目录，并为这次 PowerShell 调用设置执行策略；构建失败时保留窗口以查看错误。命令行等价操作：

```powershell
npm run build:preview
./scripts/build.ps1 -CargoArgs @('run', '-p', 'bilimani-iidx', '--example', 'api_preview')
```

打开 http://127.0.0.1:32134/iidx/card/ 或 /iidx/mecha/。这是使用真实 HTTP/选曲服务的**假游戏**：9 首 Preview Song，自动确认选曲，并提供合成曲目信息、密度/雷达/键位和演奏进度，不执行任何游戏函数。`build:preview` 将预览页面写入 `dist/preview/`，在构建时接入本地模拟弹幕源，不连接真实直播间，不需要 authCode；相邻配置保留显示/点歌参数，真实平台开关固定关闭。正式 `build` 仍写入 `dist/web/`，发布包不会包含预览代码。例程不随发行包交付，不需要真实游戏文件或单独的 Node mock server。Windows 使用上述脚本，其他支持 Rust 的开发环境在构建预览页面后可直接 `cargo run -p bilimani-iidx --example api_preview`；只有 DLL 原生接入依赖 Windows。

预览还提供 `/api/v1/iidx/user` 的合成玩家 PREVIEW、SP/DP 段位与周统计。`play` 增加合成计数，`other` 清空玩家，`select` 恢复，便于验证左下角轮换和退卡后的清空。

在预览终端输入命令并回车：

| 命令 | 效果 |
|---|---|
| help | 列出终端命令 |
| logout | 清空登录状态与模式，Mecha 约 1.2 秒后回到全屏等待登录遮罩 |
| select | 恢复选曲；从等待登录进入时显示紫色成功提示，再渐隐遮罩 |
| decide | 模拟原生歌曲确认动画；Mecha 转盘旋转并显示紫色扩散光迹，other/select/play/logout 结束此状态 |
| other | 暂时清空曲目信息；进入过选曲后只显示底栏蜂巢动画，不弹遮罩 |
| menu / menu Preview | 模拟点歌，在网页展开候选屏；先打开网页并进入选曲 |
| reply n / reply p / reply 2 | 候选翻页或用编号确认；确认后通过真实 API 入队并收回机械臂 |
| chat 任意文字 | 向页面发送预览观众的模拟弹幕 |
| 1p / 2p | 切换当前玩家侧，观察机械臂的伸出方向 |
| add 2 | 模拟观众将歌曲 ID 2 加入真实队列，打印响应及请求 ID |
| queue | 列出当前点歌、等待项的请求 ID 和错误 |
| pause / resume | 模拟打开/关闭菜单，暂停/继续自动出队 |
| priority 3 / remove 3 | 优先选或删除队列请求 ID 3（不是歌曲 ID） |
| play | 开始游玩 |
| sp / dp | 切换模式；对应模式的队列才会执行 |
| fail | 下一次定位失败，验证保留行及错误状态 |
| reload | 重读当前目录 bilimani_iidx_aliases.csv，可用 BILIMANI_PREVIEW_ALIASES 指定测试文件 |

例如先打开网页，输入 `logout` 观察等待遮罩，再输入 `select` 观察成功过渡；输入 `menu` 展开候选，`reply n` 翻页，`reply 6` 确认并收回。不回复则按页面配置超时。`other` 用于观察无遮挡的底部加载动画。每条命令后可停留观察效果；刷新页面清空候选，不重放之前的终端命令。

也可依次输入 pause、add 1、add 2，然后用打印出的 ID 执行 priority 观察队列替换；play/select 验证当前项结束和下一首。`add` 直接入队，不产生候选。预览始终显示合成数据，只用于页面和协议调试，不证明原生游戏调用正确；没有伪造真实直播平台身份。

验证正式页面时，先运行 `npm run build`，再用 `./scripts/build.ps1 -CargoArgs @('run', '-p', 'bilimani-iidx', '--example', 'api_preview', '--', 'dist/web')` 启动服务，明确使用正式静态目录。在另一终端运行：

```powershell
npx playwright install chromium
npm run test:browser
```

也可设置 PLAYWRIGHT_CHROMIUM_EXECUTABLE 为本机 Chrome 完整路径。BILIMANI_TEST_URL 可替换默认 32134 地址。测试使用生产 bundle 和真实 Rust API，模拟外部平台 HTTP/WebSocket，以及玩家接口的数据缺失/故障；验证 URL 身份码、弹幕搜索确认、队列增删、OBS 停用/激活、刷新保留队列、配置错误、玩家轮换、页面动效设置及图表 DOM 不随轮换重建，截图在忽略目录 analysis/browser/。浏览器测试会清理预览队列，应在专用预览进程运行。

机械臂候选屏可单独运行 `npm run test:browser:arm`，使用同一预览地址和 Chrome 设置。它从 Rust 预览读取合成曲库/遥测并加载生产页面，在浏览器内拦截所有后续游戏及平台请求，不修改预览队列；检查弹幕区域尺寸、1P/2P/DP 方向、分段收放、翻页、多观众切换、超时、中途取消/重新展开和关闭动效。截图保存为 `analysis/browser/arm-*.png`。纯动画状态和方向判断另由 `npm test` 验证。

要直接观看动画，运行 `npm run test:browser:arm -- --headed`：测试会打开可见的浏览器窗口，自动演示并检查上述场景，结束后关闭窗口。只需预览服务，无需游戏或真实直播间；无需在预览终端输入 `add`，该命令直接入队，不会产生前端候选。

登录遮罩和底栏蜂巢动画可用 `npm run test:browser:startup -- --headed` 单独演示；省略 `--headed` 则在后台验证并将截图保存到 `analysis/browser/startup-*.png`。同样使用上述正式静态目录。

终端与网页的端到端检查使用 `npm run test:browser:preview`：先构建 Rust 的 `api_preview` 和 `npm run build:preview`，关闭占用 32134 的预览进程。测试自行启动 Rust 例程，向真实 stdin 发送 logout/select/other/menu/reply/2p/dp，验证网页动效、真实搜索与入队、刷新不重放事件，结束后自动关闭进程。此测试不拦截 HTTP，不访问直播平台。

开发热更新使用 npm run dev。默认连接 32133 的 DLL；假游戏可设置 BILIMANI_API_URL=http://127.0.0.1:32134。从 Vite 打开的 /card/、/mecha/ 使用对应 dist/web/iidx/<style>/config.json，与 DLL 反代读取同一份配置。部署目录为其他路径时设置 BILIMANI_WEB_ROOT。Vite 转发重写 Origin/Host 以匹配本地 API，生产部署无需此开发代理。

转盘可用 `npm run test:browser:turntable` 检查：先构建正式页面和 Rust `api_preview`，释放 32134 端口。测试自行启动 Rust 例程并驱动真实终端命令，检查决定动画、旋转/光迹、重复轮询、刷新、退场先于开曲、取消、退卡及关闭动效；结束后关闭例程。截图在 `analysis/browser/turntable-*.png`。正式原生触发目前仅适用于已分析的 34 / 010-0917 路径，详见 [决定动画证据](iidx34-0100917-decision-analysis.md)。

## 原生边界验证

Rust 单元测试覆盖结构化错误、纯解析、HTTP 约束、选曲状态迁移和游戏快照。scripts/smoke-dll.py --reject 验证真实 DLL 拒绝不支持的模块及 SDK 退出；正向模式需要开发者自己的、忽略目录下的 analysis/bm2dx.dll：

```powershell
py scripts/smoke-dll.py
py scripts/smoke-dll.py --song-info
py scripts/smoke-dll.py --queue
py scripts/smoke-dll.py --user
py scripts/smoke-dll.py --bad-user
py scripts/smoke-dll.py --song-info --lane-generator-patch
py scripts/smoke-dll.py --song-info --bad-lane-layout
py scripts/smoke-dll.py --song-info --bad-lane-update
py scripts/smoke-dll.py --song-info --bad-playback
py scripts/smoke-dll.py --occupied-port
```

脚本只映射二进制以校验布局；回调测试用合成数据和替代函数，不启动游戏。它检查真实 DLL、HTTP 快照、hook 转发及关闭释放端口。它不能证明游戏内行为正确，也不包含平台网络连接。

发布前仍需在各支持构建（**IIDX 33 / 20260812、34 / 010-0916、34 / 010-0917**）实机检查：运行时曲库、原生搜索词、单人 SP 与 DP、自动定位、难度切换、场景变化、开曲/重试/退出、遥测、单人 SP 菜单手势/输入捕获、删除/优先、CSV 重读、OBS 刷新保留队列和实际平台连接。新增构建需独立 ABI/指纹依据，不能仅复制版本名。

IIDX 34 的逐项分析、RVA/布局依据、复现命令与实机待验证项见 [iidx34-analysis.md](iidx34-analysis.md) 和 [iidx34-0100917-analysis.md](iidx34-0100917-analysis.md)。使用 `py scripts/verify-iidx34.py <bm2dx.dll路径>` 做只读文件验证，`py scripts/smoke-iidx34.py --game-dll <010-0916 bm2dx.dll路径> --queue --user` 检查 010-0916 构建的合成回调和 HTTP，`py scripts/smoke-iidx34-0100917.py --game-dll <010-0917 bm2dx.dll路径>` 检查 010-0917 构建的指纹、全部 native guard、vtable hook 和 worker 启动。两者都不启动游戏。33 的回归继续使用 `scripts/smoke-dll.py`。新增分析应同步记录证据与未知项，不只保留本地 IDA 会话。

## 打包

```powershell
./scripts/package.ps1
py scripts/package-frame.py
```

package.ps1 安装锁定的 npm 依赖、构建前端和 workspace，再将各游戏的 DLL、bilimani_web 与可直接使用的默认配置分别放入 dist/package-iidx/ 与 dist/package-ddr/，输出 dist/bilimani-iidx-<version>.zip 与 dist/bilimani-ddr-<version>.zip。DLL 配置及空别名 CSV 来自 config/；各页面 config.json 来自 web/apps/<game>/config.json。只覆盖受控暂存目录，不打包本地调试配置、日志、数据库、Node 或游戏文件。README、docs、AGENTS.md、LICENSE 和 THIRD-PARTY-NOTICES.md 仅保留在仓库，不进入发布包。

package-frame.py 在前端构建后产生独立 Mecha 静态 ZIP，目标是 bilimani_web/iidx/mecha/，只包含该页面的 index.html、assets/ 和源码中的默认 config.json。

GitHub Release workflow 在标签触发时执行 workspace Rust 检查及 TS 检查、构建和打包。带真实游戏文件的原生测试仅在开发者本地运行，游戏文件不得上传 CI。

## 调试与边界

DLL 日志为 bilimani_iidx.log，记录版本、hook/采样启用状态、曲库载入和周期性原生诊断；不会记录弹幕或身份码。平台错误展示在页面，调试时可查看浏览器网络请求。分享日志或截图前不要附带实际配置或含 authCode 的来源 URL。

纯函数负责解析、校验和状态转换，IO 留在网络/文件/native 边界。普通底层错误使用 anyhow 增补上下文，领域错误使用 enum 并按类型映射 HTTP code；不得按错误文字分支。详见 [Rust 约定](architecture.md#rust-实现约定)。

## DDR WORLD development

`crates/bilimani-ddr` produces `bilimani_ddr.dll`; `versions/world` contains the 2026022400 x64 adapter. The local analysis target is copied from the supplied Samba location before IDA opens it. Supported SHA-256: `89756f03a0ca98c2d42776d08569364514472a97ff89f1b09b0b68f40082d62e`. The earlier 32-bit DLL is not supported.

Selection scene vtable RVA 0x355158: update slot 6, exit slot 5. Gameplay actor vtable RVA 0x348438: update slot 6, exit slot 5. Catalog pointer RVA 0x6c9928, 600-byte Info records, mcode +8, MSVC strings +24/+64/+104 (title/yomi/artist), difficulty bytes +272 and signed overrides +476. Selection scene +136 is the model, +144 view; view +1088 is MusicPanel, panel +192 the list. Normal song items use secondary vtable RVA 0x35beb0 and metadata at +328. Native absolute list selection RVA 0x48370 updates the cursor and invokes the normal selected-item callback/preview. Difficulty setter RVA 0xf3460 and availability RVA 0x19fdc0 are checked before execution. All offsets are module-relative and fingerprint gated; native entry prefixes and scene slots are also checked.

Only scene callbacks read game data and call selection functions. The worker reads owned snapshots and submits an epoch-tagged mailbox. Catalog capture waits for selection initialization. Player names and score data are intentionally not inferred. Selection uses the active visible folder/filter; it does not construct metadata or bypass unlock checks. Native hooks remain pass-through after shutdown because the DLL is pinned.

Run `cargo test -p bilimani-ddr`, `cargo run -p bilimani-ddr --example api_preview -- dist/web` (port 32134), and `npm run typecheck && npm test && npm run build`. The Rust preview uses production HTTP/search/queue and synthetic telemetry, with terminal commands printed at startup; it is excluded from release archives. Actual DDR execution has not been validated.

Build the independent DDR archive with `python scripts/package-ddr.py` after the Rust release and frontend builds; output is `dist/bilimani-ddr.zip`. `scripts/package.ps1` builds separate versioned IIDX and DDR archives; each contains only its own DLL, defaults and pages. Tag pushes publish both archives to the same GitHub Release; manual workflow runs only upload build artifacts. Archive versions come from the respective Rust packages. Run `python scripts/smoke-ddr.py`, plus `--wrong-build` and `--bad-entry`, to test the release DLL using synthetic memory. Run `node web/tests/ddr.browser.mjs` while the DDR Rust preview is running; optionally set PLAYWRIGHT_CHROMIUM_EXECUTABLE to a local Chromium executable. These checks do not run the supported game.
