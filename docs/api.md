# 游戏 HTTP API v1

本协议包含游戏能力与公用队列，不包含直播平台协议/鉴权。队列请求可附带展示用 requester 名称。当前实现：IIDX 33 / 20260812。游戏 ID、模式和谱面标识由游戏定义。

| 请求 | 返回 |
|---|---|
| GET /api/v1/meta | api_version、game、major_version、build、ready |
| GET /api/v1/songs?q=...&limit=20 | songs：id、title、charts（chart.mode/id、level） |
| POST /api/v1/selections | 202，id、status、song_id；接收 song_id、可选 mode/chart |
| GET /api/v1/selections/{id} | 当前操作或有限保留的结果 |
| GET /api/v1/queue | revision、capacity、current、items、events；权威队列快照 |
| POST /api/v1/queue | 201，加入队列；song_id、可选 mode/chart/requester |
| DELETE /api/v1/queue/{id} | 删除等待项或结束当前点歌；执行中返回 409 |
| POST /api/v1/queue/{id}/priority | 优先定位指定等待项，执行中返回 409 |
| POST /api/v1/aliases/reload | 从配置路径重新加载 CSV；失败保留旧别名 |
| GET /api/v1/state | ready、phase（other/select/playing）、mode、epoch、plays、now_playing、queue |
| GET /api/v1/iidx/now-playing | IIDX 实际选曲/演奏信息 |
| GET /api/v1/iidx/lane-counts | IIDX 当前曲目原谱数量 |
| GET /api/v1/iidx/lane-order | IIDX 当前物理键位 |

搜索未就绪返回 503；空/过长查询、非法 limit 或请求字段返回 400。歌曲不在曲库返回 404，已存在未完成选曲返回 409。错误统一为 `{ "error": { "code": "...", "message": "..." } }`，调用方按 code/status 判断，不匹配 message。

POST 示例：`{"song_id":33001,"mode":"SP","chart":"SPA"}`。chart 可省略，沿用游戏难度；mode 可省略，从指定 chart 或当前游戏模式确定。无有效模式时返回 409。ID 为本次 DLL 运行内的操作标识，不跨游戏重启使用。

操作状态：waiting → executing → selected / failed。暂时场景变化可回到 waiting；总等待期限 30 秒，超时为 failed。selected 只表示完成定位，不表示开曲。HTTP 接收成功不直接调用游戏函数。刷新不会撤销已接收操作，也不恢复旧操作；前端不自动重试 POST。新增点歌应使用队列 API，由 DLL 调度；直接 selections 接口仅供游戏能力调用，队列/原生已有操作时返回冲突。

state 是同一次 worker 采样的独立数据，不包含内存地址。plays 是本次运行的累计开曲计数。IIDX 的 now_playing 保留歌曲、谱面、BPM、密度、雷达、原谱数量、随机键位与演奏进度；这些扩展不作为其他游戏必须实现的模型。

GET 支持 HEAD，API 和配置文件禁用缓存。服务默认只监听 127.0.0.1:32133；页面与 API 同源，校验 Host/Origin。带业务正文的写接口只接收有大小上限的 application/json；DELETE、priority 和 aliases/reload 不带正文。静态资源限制在配置根目录内，拒绝路径穿越及越界符号链接。

访问静态目录时，缺少结尾 `/` 会返回 308 并补齐斜杠，原样保留查询参数，使相对资源和 config.json 始终从正确目录加载。例如 `/iidx/mecha?authCode=...` 跳转到 `/iidx/mecha/?authCode=...`。

旧 /api/state 不属于新协议；新页面将当前 DLL 队列与页面本地弹幕/候选组合成展示状态。刷新不会清空 DLL 队列。旧 /api/now-playing 等接口不作为新页面依赖。

## 队列与别名响应

入队正文示例：`{"song_id":33001,"mode":"SP","chart":"SPA","requester":"观众昵称"}`。requester 可省略，最大 512 UTF-8 字节，不接受控制字符，不携带平台用户 ID。队列 ID 与 selections 操作 ID 属于不同资源。

queue 中的等待项为 `{id,song_id,title,mode,chart,requester,error,executing}`，current 为相同基础字段加 `remaining_seconds` / `duration_seconds`。error 是给人看的文本或 null，不是客户端决策依据。队列满返回 queue_full/409；找不到 ID 返回 not_found/404；执行中不能删除/优先替换，返回 busy/409。priority 只接受当前可选场景和匹配模式，否则 mode_unavailable/409。删除当前项相当于跳过本次请求，不强制中断已经开始的游戏。

revision 在队列状态改变时递增；events 保留最近 64 条 `{id,kind,entry}`，kind 为 added/removed/prioritized/selected/failed/played/expired。用它们显示近期通知，不能将其当持久日志；以 current/items 为准。首次 GET 只显示现状、不重播事件。DELETE/priority 返回完整队列快照；priority 响应只表示已登记优先意图，成功定位仍需等 executing 消失并转为 current。

POST /api/v1/aliases/reload 成功返回 `{"count":条数}`；无曲库为 not_ready/503，CSV 或文件错误为 alias_configuration/422，失败不替换旧表。不接受任意文件路径，始终使用 DLL 配置路径。

## 页面反向代理

`GET/HEAD/POST /<页面目录>/_proxy/<名称>/<路径>` 读取页面相邻 config.json 的 `proxy` 映射。例如 `/iidx/card/_proxy/relay/api/open_live/start_game` 转发到 `proxy.relay` 下的相同相对路径，查询参数和正文原样保留。Rust 不解释平台字段或响应正文，不保存上游会话。

只转发 Content-Type、Accept、Authorization 请求头；不转发浏览器 Cookie、Origin、Host，不存 Cookie，不跟随上游重定向，不自动重试。只允许固定配置的 HTTPS 上游（回环 HTTP 供调试）。请求上限 1 MiB、响应上限 4 MiB、上游超时 12 秒。配置无效返回 proxy_configuration/503，上游请求失败返回 upstream/502；正常上游状态码保持，JSON 保留 JSON 类型，其他内容作为二进制返回。
