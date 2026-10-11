# 点歌与队列行为

DLL 拥有公共内存队列，网页管理平台会话、候选和冷却；游戏内菜单操作同一份 DLL 状态。职责约束见 [architecture.md](architecture.md)。

## 生命周期与页面隔离

每套静态页面独立部署、读取自身旁边的 config.json。多个主播可以选择不同页面；选择复用同一页面时，通过 URL 的 `?authCode=身份码` 区分 Bilibili 连接，不建立直播间 mapping。身份码不从配置文件或 OBS 推流设置读取，其他平台可在自己的页面定义接入方式。

页面刷新重新读取配置并连接平台，保留 DLL 的等待队列和当前请求，只清空页面候选、冷却与消息记录。首次读取队列不重播旧事件。DLL 队列在游戏退出时清空，不持久化、不恢复、不自动重试结果不明的 POST。

一个直播会话只运行一个活动点歌来源。OBS 多场景复用同一来源；隐藏/停用后停止弹幕、清除未确认候选，重新激活后新建平台会话。首次加载仅能以文档可见性判断，收到 active 事件后再跟随 active；OBS Studio 预览可能保持来源可见，不保证只凭页面识别唯一节目输出。不同主播来源应配合“不可见时关闭源”和场景切换，避免同时运行。普通浏览器副本也会接收弹幕；项目不做多来源选主。

这些事件的依据见 [obs-browser 接口](https://github.com/obsproject/obs-browser/blob/master/README.md) 和 [来源事件实现](https://github.com/obsproject/obs-browser/blob/master/obs-browser-source.cpp)。不接入 obs-websocket，不读取推流密钥。

## 命令与执行

命令前缀后需要空白和搜索词。IIDX 页面解析可选 SP/DP 谱面后缀，共享 TS 控制器只处理模式/谱面标识。

同一观众重复搜索替换尚未确认的候选，旧异步结果不能覆盖新请求。单个候选直接尝试入队；多个候选只接受发起者回复编号。使用平台稳定用户 ID 关联候选，重名互不干扰。编号为整个列表编号，n/p 翻页，每页 5 首，翻页不延长超时。

页面检查模式、谱面和冷却，再 POST /api/v1/queue。DLL 再验证曲库、谱面、名称和容量，成功入队才开始观众冷却。平台身份 ID 不传 DLL，requester 仅作展示。HTTP 201 表示已进入权威队列，不代表已经定位；页面不直接调度 selections。

DLL 在可选曲、无当前请求且菜单关闭时选择第一首匹配模式的等待项；不匹配模式的项保留。菜单中的“优先选这首”可替换当前点歌，打开菜单也可执行。只有原生定位成功才移出等待队列并更新当前项；失败保留原行和先前当前项，显示错误，等待手动重试或删除，不无限自动重试。

已定位不代表已开曲。开始游玩任意歌曲、超时或菜单跳过后结束当前请求。开曲计数处理轮询间的短暂状态。正在执行的队列项不能删除/替换；其他等待项可删。网页通过同一 /state 中的 queue 快照和最近 64 个事件显示删除、定位、失败等结果。

传输失败时页面提示结果不明，不重发 POST，后续 GET 显示 DLL 实际状态。原生操作可在 30 秒期限内等待适合场景，以新 epoch 重新提交；不涉及跨重启恢复。

## 配置

页面 config.json：

| 字段 | 默认值 | 范围/含义 |
|---|---|---|
| platform.type | bilibili | 当前平台实现 |
| platform.enabled | true | false 只展示游戏信息，无需 authCode |
| platform.relayUrl | ./_proxy/relay | 本页代理路径，或支持 CORS 的 HTTPS relay |
| proxy.relay | https://api1.blive.chat | 固定上游 |
| requests.prefix | 点歌 | 非空字符串 |
| requests.candidateLimit | 20 | 1–100 |
| requests.selectionTimeoutSeconds | 60 | 1–3600，候选确认期限 |
| requests.cooldownSeconds | 0 | 0–86400，0 关闭 |
| requests.maxPendingUsers | 100 | 1–1000，同时搜索/待选人数 |
| display.feedLimit | 10 | 1–100 |

启用 Bilibili 时 URL 必须含单个非空 authCode 参数。需要编码特殊字符；不允许重复参数、控制字符或超过 512 字符。各页面配置互不覆盖。配置只在页面启动时读取，缺失/无效则停止启动并显示错误。反代每次请求只读 proxy 映射。

DLL 的 bilimani_iidx.json：queue.capacity 为 1–1000（默认 20）；queue.current_timeout_seconds 为 1–86400（默认 600）；controls.enabled 默认 true，double_tap_ms 为 100–2000（默认 400）。aliases_path 默认 bilimani_iidx_aliases.csv，路径以 DLL 目录为基准。

CSV 表头 alias,target；目标是歌曲 ID 或唯一完整曲名。UTF-8/BOM/引号受支持，按 NFKC/小写检查重复别名，精确别名命中优先，其他情况参与模糊匹配。全量校验成功才替换表，失败保留旧表。启动时文件不存在表示无别名，主动重读时缺失会报错。文件上限 1 MiB，无自动监控或游戏内编辑。

Bilibili JS 创建 Open Live 会话、协商 zlib、心跳重连并按消息 ID 去重。关闭/停用时尽力结束上游会话；异常关闭后的最终清理由上游心跳机制决定。DLL 不代管这些状态。
