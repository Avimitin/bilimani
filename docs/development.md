# 开发、验证与发布

先读 [架构](architecture.md)、[API](api.md) 和根目录 [AGENTS.md](../AGENTS.md)。只有 bilimani-iidx 提供游戏实现，支持 IIDX 33 / 20260812；bilimani-api 不依赖具体游戏。根 Cargo.toml 是虚拟 workspace。

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

npm run build 独立打包 Card、Mecha，产物位于 dist/web/iidx/<style>/，每页自带资源与 config.json，不依赖另一布局。首次构建从 web/apps/iidx/config.json 写入默认配置，已有产物目录的 config.json 会保留。发布 ZIP 始终从源码复制公开默认值，不使用本地调试配置。dist/web/index.html 为入口索引。

## spice2x 版本依据

最低要求为 [spice2x 26-09-12](https://github.com/spice2x/spice2x.github.io/releases/tag/26-09-12)，发布于 2026-09-12（UTC），上游标记为 Pre-release。对应下载文件为 [spice2x-26-09-12.zip](https://github.com/spice2x/spice2x.github.io/releases/download/26-09-12/spice2x-26-09-12.zip)。

该版本收录 [PR #908](https://github.com/spice2x/spice2x.github.io/pull/908)，引入本项目游戏内菜单需要的 D3D9 绘制回调，对应 SDK v0.4。此最低版本依据上游发布记录和接口要求确定，不代表已完成该版本的实机回归；实机验证项见下文。面向玩家的 README 只列版本与下载入口。

## 不启动游戏的预览

前端已构建后，Windows 可双击 `scripts/preview.bat` 启动 Rust 预览。它会切换到仓库目录，并为这次 PowerShell 调用设置执行策略；构建失败时保留窗口以查看错误。

```powershell
npm run build
./scripts/build.ps1 -CargoArgs @('run', '-p', 'bilimani-iidx', '--example', 'api_preview')
```

打开 http://127.0.0.1:32134/iidx/card/ 或 /iidx/mecha/。这是使用真实 HTTP/选曲服务的**假游戏**：9 首 Preview Song，自动确认选曲，并提供合成曲目信息、密度/雷达/键位和演奏进度，不执行任何游戏函数。构建后页面已带默认配置；仅看布局可将产物目录 config.json 的 platform.enabled 设为 false。例程不随发行包交付，不需要真实游戏文件或单独的 Node mock server。Windows 使用上述脚本，其他支持 Rust 的开发环境可直接 `cargo run -p bilimani-iidx --example api_preview`；只有 DLL 原生接入依赖 Windows。

预览还提供 `/api/v1/iidx/user` 的合成玩家 PREVIEW、SP/DP 段位与周统计。`play` 增加合成计数，`other` 清空玩家，`select` 恢复，便于验证左下角轮换和退卡后的清空。

在预览终端输入命令并回车：

| 命令 | 效果 |
|---|---|
| add 2 | 模拟观众将歌曲 ID 2 加入真实队列，打印响应及请求 ID |
| queue | 列出当前点歌、等待项的请求 ID 和错误 |
| pause / resume | 模拟打开/关闭菜单，暂停/继续自动出队 |
| priority 3 / remove 3 | 优先选或删除队列请求 ID 3（不是歌曲 ID） |
| play / select / other | 开始游玩、回选曲、进入无曲目信息场景 |
| sp / dp | 切换模式；对应模式的队列才会执行 |
| fail | 下一次定位失败，验证保留行及错误状态 |
| reload | 重读当前目录 bilimani_iidx_aliases.csv，可用 BILIMANI_PREVIEW_ALIASES 指定测试文件 |

例如依次输入 pause、add 1、add 2，然后用打印出的 ID 执行 priority，即可观察队列替换；play/ select 验证当前项结束和下一首。也可以直接调用文档中的 HTTP API。预览始终显示合成数据，只用于页面和协议调试，不证明原生游戏调用正确；没有伪造真实直播平台身份。需要验证候选/弹幕流程时，浏览器集成测试会模拟外部 HTTP/WebSocket。

运行浏览器集成检查（保留预览进程，在另一终端）：

```powershell
npx playwright install chromium
npm run test:browser
```

也可设置 PLAYWRIGHT_CHROMIUM_EXECUTABLE 为本机 Chrome 完整路径。BILIMANI_TEST_URL 可替换默认 32134 地址。测试使用生产 bundle 和真实 Rust API，模拟外部平台 HTTP/WebSocket，以及玩家接口的数据缺失/故障；验证 URL 身份码、弹幕搜索确认、队列增删、OBS 停用/激活、刷新保留队列、配置错误、玩家轮换、页面动效设置及图表 DOM 不随轮换重建，截图在忽略目录 analysis/browser/。浏览器测试会清理预览队列，应在专用预览进程运行。

开发热更新使用 npm run dev。默认连接 32133 的 DLL；假游戏可设置 BILIMANI_API_URL=http://127.0.0.1:32134。从 Vite 打开的 /card/、/mecha/ 使用对应 dist/web/iidx/<style>/config.json，与 DLL 反代读取同一份配置。部署目录为其他路径时设置 BILIMANI_WEB_ROOT。Vite 转发重写 Origin/Host 以匹配本地 API，生产部署无需此开发代理。

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

发布前仍需在 **IIDX 33 / 20260812** 实机检查：运行时曲库、原生搜索词、单人 SP 与 DP、自动定位、难度切换、场景变化、开曲/重试/退出、遥测、单人 SP 菜单手势/输入捕获、删除/优先、CSV 重读、OBS 刷新保留队列和实际平台连接。新增构建需独立 ABI/指纹依据，不能仅复制版本名。

## 打包

```powershell
./scripts/package.ps1
py scripts/package-frame.py
```

package.ps1 安装锁定的 npm 依赖、构建前端和 workspace，再将 DLL、bilimani_web 与可直接使用的默认配置放入 dist/package/，输出 dist/bilimani-<version>.zip。DLL 配置及空别名 CSV 来自 config/；各页面 config.json 来自 web/apps/iidx/config.json。只覆盖受控暂存目录，不打包本地调试配置、日志、数据库、Node 或游戏文件。README、docs、AGENTS.md、LICENSE 和 THIRD-PARTY-NOTICES.md 仅保留在仓库，不进入发布包。

package-frame.py 在前端构建后产生独立 Mecha 静态 ZIP，目标是 bilimani_web/iidx/mecha/，只包含该页面的 index.html、assets/ 和源码中的默认 config.json。

GitHub Release workflow 在标签触发时执行 workspace Rust 检查及 TS 检查、构建和打包。带真实游戏文件的原生测试仅在开发者本地运行，游戏文件不得上传 CI。

## 调试与边界

DLL 日志为 bilimani_iidx.log，记录版本、hook/采样启用状态、曲库载入和周期性原生诊断；不会记录弹幕或身份码。平台错误展示在页面，调试时可查看浏览器网络请求。分享日志或截图前不要附带实际配置或含 authCode 的来源 URL。

纯函数负责解析、校验和状态转换，IO 留在网络/文件/native 边界。普通底层错误使用 anyhow 增补上下文，领域错误使用 enum 并按类型映射 HTTP code；不得按错误文字分支。详见 [Rust 约定](architecture.md#rust-实现约定)。
