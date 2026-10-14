# bilimani

IIDX / DDR 直播点歌插件。观众在直播间发弹幕点歌，游戏自动定位歌曲，OBS 页面展示当前曲目信息和点歌队列。主播可以在游戏内删除点歌、跳过当前点歌或优先选择某一首。

目前适用于 **Bilibili 直播、IIDX 33 / 20260812**，以及 **DDR WORLD / MDX-003-2026022400（64 位，实验支持，尚未实机验证）**。

## 安装

准备好 OBS，并使用 **spice2x 26-09-12（2026 年 9 月 12 日版）或更新版本**启动游戏：[下载 26-09-12](https://github.com/spice2x/spice2x.github.io/releases/download/26-09-12/spice2x-26-09-12.zip)。

以下步骤适用于游戏和 OBS 在同一台电脑上运行。

1. 从[下载页面](https://github.com/Avimitin/bilimani/releases)下载对应游戏的 `bilimani-iidx-版本号.zip` 或 `bilimani-ddr-版本号.zip`，完整解压到一个固定文件夹。包内已带默认配置。
2. 让启动器加载解压出的 `bilimani_iidx.dll`。如果平时用 `.bat` 启动游戏，右键编辑它，在 `spice64.exe` 那一行末尾追加 `-k "D:\你的安装目录\bilimani_iidx.dll"`，将引号内换成文件的实际路径。
3. 获取自己的 Bilibili 直播身份码，操作方法见[身份码获取帮助](https://blive.chat/help)。
4. 启动游戏，在 OBS 的“来源”中点击“＋”，添加“浏览器”。选择下表的一种布局，粘贴地址，把“你的身份码”替换为自己的身份码，并填写宽度和高度。“本地文件”保持不勾选。

| 布局 | OBS 地址 | 宽度 × 高度 |
|---|---|---|
| Mecha：完整直播画面 | `http://127.0.0.1:32133/iidx/mecha/?authCode=你的身份码` | 1920 × 1080 |
| Card：竖版点歌卡片 | `http://127.0.0.1:32133/iidx/card/?authCode=你的身份码` | 480 × 800 |
| DDR Street：街舞直播框 | `http://127.0.0.1:32133/ddr/street/?authCode=你的身份码` | 1920 × 1080 |

DDR 使用 `bilimani_ddr.dll`，启动参数改为 `-k "D:\你的安装目录\bilimani_ddr.dll"`。只加载当前游戏对应的 DLL。选曲时按 F8 打开队列菜单，方向键移动、Enter 确认、Esc 返回。跳歌要求目标歌曲在当前文件夹和筛选结果中。

## 开始点歌

进入游戏的普通选曲界面后，让观众发送 `点歌 曲名`。如果出现多首候选歌曲，观众回复对应数字即可确认。游戏选中歌曲后，由主播自行开始游玩。

IIDX 单人 SP 选曲时，**双击另一侧的 Start** 打开点歌菜单。B1 / B2 移动，转盘切换操作，B6 确认，B7 关闭。

以后直播时，打开游戏和 OBS 即可。

## 更多帮助

- [调整设置、添加歌曲别名、常见问题](docs/usage.md)
- [开发者文档](docs/development.md)
