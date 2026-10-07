# GREEN ROOM 直播框架

1920 × 1080 的极简精密机甲直播框架。石墨灰与低饱和深绿的连续装甲结构，采用叠层肩甲、侧向导轨与锁定接头、连接底部横梁的角部护甲，以及内嵌的信息模块；哑光金属表面带有倒角高光和凹槽阴影，仅在少量关节处嵌入柔和绿光。左上为真正透明的 16:9 游戏区域；底部显示游戏实际选曲／演奏的曲名、曲风、作者、谱面难度与等级、BPM 范围、音符数及六项雷达；右侧依次显示队列、弹幕和点歌事件。底部为连续的 VFD 风格信息显示窗，右侧为一体式内嵌互动显示窗。文字位于深绿烟色玻璃内，带轻微荧光与显示纹理。连接状态并入右侧标题。由本地 CSS / SVG 绘制，无外部字体、图片或网络依赖。

## 安装独立静态包

1. 解压 `chart-requester-green-room.zip`，将包内的 **`chart_request_static/mecha` 子目录**复制到 DLL 旁的 `chart_request_static` 中。实际选曲信息需要支持 `/api/now-playing` 的新版 DLL；旧 DLL 下页面继续显示当前点歌。升级歌曲信息功能时同时更新 DLL 和 Mecha 静态文件。
2. 在游戏控制台「OBS 显示」中，将「网页静态目录」设为 **`chart_request_static/mecha`**，点击应用并保存。
3. 在 OBS 添加浏览器来源，地址填 `http://127.0.0.1:32133/queue`，宽 **1920**、高 **1080**。游戏和插件须先启动；自定义端口时替换 `32133`。
4. 浏览器来源位置设为 **(0, 0)**，放在游戏采集来源的上方。移除以前为卡片添加的自定义 CSS；OBS 默认透明背景 CSS 可以保留。
5. 游戏采集来源位置设为 **X=24、Y=40**，大小设为 **1536 × 864**。以 1920 × 1080 为画布，源的缩放为 80%。16:9 游戏画面不需要裁剪。
6. 刷新浏览器来源。金属边框和关节灯光只在游戏区域之外绘制，四角也保持透明。

切回卡片版时，将「网页静态目录」恢复为 **`chart_request_static`**（兼容原有安装），或选择完整新版发布包中的 **`chart_request_static/card`**。浏览器来源改回 **480 × 800** 并刷新。两种样式的 OBS URL 都是 `/queue`，不需要覆盖任何样式文件。

## 从源码或完整发布包使用

每种样式拥有独立的入口、CSS 和 JavaScript，没有对其他样式目录的资源依赖：

```text
web/
  card/       # 原始卡片样式（index.html、overlay.css、overlay.js）
  mecha/      # 机甲框架（index.html、overlay.css、frame.css、overlay.js）
```

完整 Windows 发布包包含 `chart_request_static/card` 和 `chart_request_static/mecha`；为兼容旧配置，打包时还会在 `chart_request_static` 根目录生成默认卡片文件。源码仅维护两个样式目录，根目录副本由打包脚本生成。独立机甲 ZIP 只包含 `mecha` 子目录，可添加到已有安装。

选择样式目录后统一访问 `http://127.0.0.1:32133/queue`，由服务提供所选目录的 `index.html`。页面通过 `/api/state` 获取实时数据。

源码打包：`python scripts/package-frame.py`，生成 `dist/chart-requester-green-room.zip`。

## 显示行为

- 歌曲信息来自 `/api/state.now_playing`，也可独立请求 `/api/now-playing`。手动选歌、切换难度及进入演奏都会更新，未收到点歌也可使用。每个参与侧显示其难度，雷达标明对应侧；缺失的 BPM／音符数／雷达显示破折号。选中目录、离开场景或演奏结束时清空旧歌。本版提供谱面 BPM 范围，未采集演奏进度和瞬时 BPM。字段说明见 [开发指南](development.md#当前选曲游玩歌曲-api)。
- 右侧最多显示 4 首等待歌曲、最近 4 条弹幕和最近 3 条事件；等待歌曲超出时显示剩余数量。弹幕与事件标题的数字表示「显示条数 / 当前服务端保留的该类条数」。仍使用服务端的共同历史上限（默认 10，可在控制台调整）。
- 搜索到多首歌曲时，候选列表临时替换右侧队列；支持最多 20 首候选，多位观众每 6 秒轮换。底部游戏歌曲信息保持可见。
- 断线后保留弹幕和事件，清除过时的当前点歌及队列，并显示等待连接提示；连接恢复后自动刷新。
- 框架尺寸固定为 1920 × 1080。较小浏览器窗口等比缩放，OBS 请按原始尺寸设置。
- `frame.css` 可调整 `--accent`、`--text`、`--muted` 等颜色和底部 `--title-size`；修改文件后刷新浏览器来源即可。

## 验证

运行 `cargo run --example overlay_preview -- web/mecha` 启动机甲样式预览服务，再运行：

```sh
python -m pip install playwright pillow
python -m playwright install chromium
python scripts/check-frame.py
```

验证原始卡片样式时，先停止机甲预览服务，运行 `cargo run --example overlay_preview -- web/card`，再运行 `python scripts/check-overlay.py`。

也可通过 `--browser` 指定本机 Chromium 路径。框架检查使用本地模拟数据，覆盖游戏区域逐像素透明度、布局边界、候选列表、长文字、离线恢复及缩放；截图输出到 `analysis/stream-frame/`。
