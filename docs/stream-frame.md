# Mecha 直播框架

1920 × 1080 的 IIDX 布局，保留游戏画面区域、当前曲目信息、难度、BPM 范围、雷达、密度图、随机键位以及点歌/弹幕区域。数据来自游戏 API，队列由 DLL 管理，本页共享 TypeScript 包负责平台连接、候选确认和读取队列。

完整包中的位置为 bilimani_web/iidx/mecha/。独立静态 ZIP 也使用该路径，附带 Mecha 的资源和默认 config.json，不包含 DLL 或 Card。

解压后可直接使用默认配置。OBS 浏览器来源 URL 为 http://127.0.0.1:32133/iidx/mecha/?authCode=你的身份码，尺寸 1920 × 1080，不选本地文件。需要调整设置时，编辑同目录的 config.json 并刷新来源。页面刷新保留等待项和当前点歌。

上方歌曲面板展示实际游戏选曲/演奏信息，观众点歌提示与等待队列展示在侧边。没有游戏信息时展示等待状态，不用点歌请求伪造游戏遥测。缺失密度、BPM 或随机键位不会以零或默认排列伪装成有效值。指标定义见 [IIDX 遥测](iidx-telemetry.md)。

源码位于 web/apps/iidx/mecha/。运行 npm run build 后产物在 dist/web/iidx/mecha/；py scripts/package-frame.py 打包。该页面可独立部署自己的全部资源，平台配置仍需经 DLL 的静态服务/无状态反代访问。开发预览和浏览器检查见 [development.md](development.md)。
