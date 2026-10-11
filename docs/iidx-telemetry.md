# IIDX 33 / 20260812 遥测

### 当前选曲／游玩歌曲 API

`GET /api/v1/iidx/now-playing` 返回实际游戏歌曲快照，无需有观众点歌。同一对象也在
`/api/v1/state.now_playing` 中，页面一次轮询即可获得全部内容。观众当前点歌保存在页面控制器中，不属于游戏 API。两个接口均支持 HEAD，沿用监听地址对应的 Host/Origin 检查及 `no-store`。

以下为字段示意，数值使用模拟数据；`song.charts` 实际包含该曲存在的全部谱面：

```json
{
  "phase": "playing",
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
      "note_count": 45,
      "radar": {"notes": 180.25, "peak": 145.5, "scratch": 65.75,
                "soflan": 120.0, "charge": 0.0, "chord": 150.01},
      "density": {"bin_ms": 1000, "duration_ms": 6500,
                  "notes": [0, 2, 42, 1], "scratch": [0, 0, 2, 1]}
    }]
  },
  "players": [{"side": 2, "chart": {"mode": "SP", "id": "SPA"}}],
  "lane_order": [{"side": 2, "random": "random", "mirror": false,
                  "status": "ready", "keys": [3, 4, 5, 2, 1, 6, 7]}],
  "playback": {"position_ms": 3200, "duration_ms": 6500}
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
| `density` | 原生每秒 note 密度；未加载、不可用或异常时为 `null`，字段见下文 |
| `lane_counts` | 每个难度的原谱七键／转盘数量；尚未分析或异常时为 `null`，不受随机选项影响 |
| `players` | 每个参与侧及其选中谱面；`side` 为 1 或 2，通过 `chart.id` 关联 `charts` |
| `lane_order` | 每个物理侧的随机选项和实际七键排列；空闲或选项不可读时为空数组，详见下文 |
| `playback` | 原生谱面时钟，包含 `position_ms` 和 `duration_ms`；非演奏或不可用时为 `null` |

普通选曲在原生更新完成后每 250ms 采集一次；切换难度和手动选歌都更新。弹窗期间保留最近
一次有效选曲，离开选曲清空。进入 stage 后从实际演奏上下文重新采集，不沿用上一首选曲；
stage 清理时立即清空。特殊选曲画面暂不采集选曲信息，其支持的 stage 仍可报告演奏曲目。
本版提供谱面 BPM 范围和演奏进度，尚未采集瞬时 BPM 或实时判定分数。

`playback` 直接读取游戏当前谱面帧和结束帧，按加载谱面时的帧率转换成毫秒，
与 `density` 的时间原点相同，精度约为一帧。演奏回调后最多每 250ms 采样一次，
通过 `/api/v1/iidx/now-playing` 和 `/api/v1/state.now_playing` 发布；HTTP 不访问原生指针。
时钟停止时数值不增加，同一 stage 重试时可以回退到 0，网页不使用本地计时补间。
未加载、字段异常或进度接口不兼容时返回 `null`，不会使用上次的进度。
原生结束前最后一次采样可能略小于总时长，不据此推断通关／失败。
`bilimani_iidx.log` 的 `[playback]` 项记录接口启用情况；进度接口单独失败不影响键位和网页，
共用的演奏更新回调不兼容时同时停用进度与键位采样，核心点歌和网页仍可运行。

`lane_order[].side` 为物理侧 1 或 2，DP 同时返回两侧。`random` 为 `off`、`random`、
`r_random` 或 `s_random`，`mirror` 单独表示 MIRROR 开关。`keys` 从实际左至右七键排列，
每个数字为该位置接收的原谱键号，不含转盘。例如 `[3,4,5,2,1,6,7]` 在网页显示为 `3452167`。
未开随机／镜像为 `1234567`，单独 MIRROR 为 `7654321`。

`status` 为 `ready` 时 `keys` 才是有效排列；选曲时 RANDOM／R-RANDOM 尚未生成本次排列，
返回 `pending` 和 `null`。进入演奏后读取游戏实际生成的映射，每 250ms 在原生更新之后采样，
覆盖不退出 stage 的重试。S-RANDOM 逐音符改变落点，没有固定七键排列，返回 `dynamic` 和
`null`。读取失败、非法映射或开启 BEGINNER 动态辅助时返回 `unavailable` 和 `null`；
无法读取选项时省略该侧，不能据此推断为 RANDOM OFF。切歌／退出场景时清除旧排列。
采集仅复制已有状态，不调用会消耗随机数的游戏变换函数。
已有插件对随机生成函数的修改可以保留；采样读取其最终键位表并检查排列是否合法。
其他键位布局／回调不兼容时只停用 `lane_order`，网页及点歌功能继续运行；
`bilimani_iidx.log` 的 `[lane]` 项记录是否启用、修改位置及实际字节。

### 按键数量与按键排列 API

两份独立的只读接口均支持 `GET` / `HEAD`，与 `/api/v1/state` 使用同一批已发布快照，HTTP
线程不访问游戏内存。它们描述当前选曲／演奏的歌曲，不接受任意歌曲加载请求。

`GET /api/v1/iidx/lane-counts` 返回当前歌曲各个可用难度的**原谱**数量：

```json
{
  "phase": "playing", "song_id": 33001,
  "basis": "original_chart", "counting": "native_weighted",
  "charts": [{
    "chart": {"mode": "SP", "id": "SPA"},
    "lane_counts": {"sides": [{
      "side": 1, "keys": [210, 248, 233, 269, 221, 246, 254], "scratch": 119
    }]}
  }]
}
```

`keys` 固定为原谱 1–7 键顺序，`scratch` 单列；SP 只有谱面侧 `1`，即使玩家坐在 2P。
DP 有谱面侧 `1` 和 `2`。数量沿用游戏原生统计：普通 note 计 1，CN／HCN 及长转盘计 2，
不是实际按下次数。未完成分析的难度返回 `lane_counts: null`，真实的空轨道返回 `0`。
相同数据也包含在 `/api/v1/iidx/now-playing` 的 `song.charts[].lane_counts` 内。

`GET /api/v1/iidx/lane-order` 返回独立的当前物理排列：

```json
{
  "phase": "playing", "song_id": 33001,
  "players": [{"side": 2, "chart": {"mode": "SP", "id": "SPA"}}],
  "lane_order": [{"side": 2, "random": "random", "mirror": false,
                  "status": "ready", "keys": [3, 4, 5, 2, 1, 6, 7]}]
}
```

`lane_order[].side` 是物理玩家侧。排列为 `3452167` 时，第一个物理按钮的下标为 `3`，
内部数量取原谱 `keys[2]`，即例中的 `233`。RANDOM／R-RANDOM／MIRROR 都按这一规则显示。
S-RANDOM 没有固定映射，七键数量和下标显示破折号，转盘数量仍可展示；原谱数量 API 保持可用。
空闲／离开歌曲后 `song_id` 为 `null`，数量接口的 `charts` 及排列接口的 `players`、
`lane_order` 均为空数组。独立请求可能跨过一次状态更新；需要严格同步时读一次
`/api/v1/iidx/now-playing` 或 `/api/v1/state`，Mecha 前端采用这一方式。

`density.bin_ms` 固定为 1000；`notes[i]` 是 `[i*1000, (i+1)*1000)` 毫秒内的原生
加权 note 总数，`scratch[i]` 是其中的转盘数量，不能再加到总数上。长押沿用原生统计：
在起点所在秒计 2 个 note，不按持续时长计数。DP 合并两侧。`duration_ms` 是原生谱面结束
时间，数组可能在最后一个 note 所在秒结束；绘制时应按曲长保留结尾空白。不对大于 30 的
数值截断，也不把最后不足一秒的桶放大折算。

普通选曲复用游戏已有后台分析线程请求当前歌曲，无需打开原生密度面板。结果可能晚于歌曲
元数据出现；原生线程不存在、谱面不可用或数据异常时保持 `null`。已取得的当前歌曲密度会
保留到该曲演奏，特殊 stage 若未取得对应结果则为空。仅缓存一首，按歌曲 ID 匹配，不串曲。

游戏回调只产生拥有独立内存的 Rust 数据；worker 每 50ms 发布快照，HTTP 请求不调用游戏。
没有可用元数据时仍返回 200 和空状态；网页应正常处理 `null`、空 `players` 与空文本。
Mecha 底栏展示曲名、曲风／作者、BPM、各轨数量与随机键位、密度及雷达。BPM 在键盘上方，
七个错位排列的矩形 IIDX 按钮内显示数量，荧光底板上的深色数字呈现镂空效果，下标显示随机后的原谱键号。
圆点显示转盘数量，下标为 SC；1P 转盘在左、2P 在右。左上方小字显示随机选项；BPM 与键盘作为一组在底部容器内垂直居中，DP 两侧键位左右并排。不再展示难度牌或总音符数。
每个参与侧有独立密度图、原生每秒峰值和时间轴。密度图宽 380px，
按 5 秒窗口求平均（超过 3 分钟用 10 秒），以 12 段 VFD 灯条近似显示，末窗按实际时长求均值。
低段青绿、高段暗橙，底部亮青绿表示总量中的转盘，空窗仍保留暗段；API 仍返回原生 1 秒数组。
缺失时显示「暂无密度数据」。两侧都参与时显示各自键位和密度图，
雷达注明其对应的首个参与侧。游戏轮询失败后清除歌曲，重连恢复。卡片样式维持点歌队列展示。

Mecha 顶部提示为 20px、主播名为 22px，名字旁为内联 bilibili SVG；向下突出的梯形装甲
占用扩大的提示区。互动栏下移到 y=152、高度 760px，正文／昵称分别为 20px／18px。
队列存在时让出固定 370px，队列隐藏时归还给弹幕；按实际行高显示最近消息，不裁切半行。

自动验证包括 bilimani-iidx/tests 中的 song_info、density、lane_order 和 service 测试，以及 bilimani-api/tests/http.rs 的接口边界测试。
`py scripts/smoke-dll.py --song-info` 使用私有映射与模拟原函数，验证真实 DLL 回调到 JSON 的链路、SP/DP、目录、stage 切换、重试键位更新与清理。web/tests/browser.mjs 验证打包页面的配置、点歌与刷新流程。模拟测试不等于真实游戏运行验证。
