# miredit-web —— 网页版地图编辑器的 HTTP 后端

把两个库暴露成 API，**本 crate 自己不实现编辑逻辑，也不实现渲染逻辑**：

| 能力 | 来自 | 本 crate 做了什么 |
|---|---|---|
| 打开 / 编辑 / 撤销 / 原子保存 | `miredit-core`（`EditSession` / `Command` / `apply` / `save_session`） | 把 HTTP JSON 翻成 `Command`，串行化写命令 |
| 出图（三层合成 + 阻挡标记 + PNG） | `miredit-render`（`try_render_region_with` / `png_io`） | 把查询参数翻成 `Viewport` / `LayerMask` / `CollisionOverlay` |
| 图库定位与帧解码 | `mir2-assets`（`LibPaths` / `WilSource`） | 启动时扫一次，进程内共享 |
| 多格式判别 | `mir2-format`（`read_any_tolerant`） | 算 `tolerant_trailer_bytes`；**开图本身**走 `miredit-core`（它也吃九种格式 + 宽松尾块） |
| 印章目录 | `mir2-stamp`（`StampCatalog` / `kind_label_zh` / `Stamp::parse`） | 懒加载 + 分页 + 按页解析 `.X`；`PlaceStamp` 时把 `.X` 转成内核格表 |

---

## 一、跑起来

### 1.1 本地

```bash
cargo build -p miredit-web --release

export MIREDIT_BIND=127.0.0.1:8080
export MIREDIT_MAP_ROOT=/path/to/maps          # /api/map/open 的白名单根
export MIREDIT_CLIENT_ROOT=/path/to/client-root
export MIREDIT_PALETTE_JSON=/path/to/crystalstudio/catalogue/palette.json
export MIREDIT_WEB_ROOT=/path/to/repo/web
cargo run -p miredit-web --release
```

```bash
curl -s http://127.0.0.1:8080/api/health
# {"ok":true,"version":"0.1.0"}
```

### 1.2 环境变量

| 变量 | 缺省 | 说明 |
|---|---|---|
| `MIREDIT_BIND` | `0.0.0.0:8080` | 监听地址（Docker 里必须 `0.0.0.0` 才能被 `-p` 映射） |
| `MIREDIT_MAP_ROOT` | **无（必须显式设置）** | **`/api/map/open` 的白名单根**（相对路径按进程 cwd 绝对化）；未设 ⇒ 地图端点 503 |
| `MIREDIT_CLIENT_ROOT` | **无（`mir2_assets::client_root`）** | 图库根（`.wil/.wzl`）；未设/扫不到 ⇒ 出图端点 503 |
| `MIREDIT_PALETTE_JSON` | **无（必须显式设置）** | 印章目录索引（须落在 `<dataRoot>/catalogue/palette.json`）；未设/不存在 ⇒ `/api/stamps` 503 |
| `MIREDIT_WEB_ROOT` | 本仓 `<仓库根>/web`（中立回退） | 静态前端（本仓的 `web/`）；不存在 ⇒ `GET /` 404（**不伪造页面**） |

> ⚠ 2026-11-06 起**没有任何内置路径指向开发机**：四个路径变量的缺省值要么不存在（地图/图库/印章），
> 要么是本仓自己的 `web/`。独立/公开部署时请按 §1.1 显式设置；不设就是"如实不可用"，不是"猜一个"。

> ⚠ `MIREDIT_MAP_ROOT` 下发给 `/api/map/open` 的 `path` 必须在白名单根之内：前端一律传**相对路径**
> （如 `maps/0.map`），绝对路径同样会被那四道闸校验一次。

### 1.3 Docker

> ⚠ **Docker 构建请用仓库根的 `docker/Dockerfile`，本 crate 目录下的 `Dockerfile` 已废弃。**
>
> 本目录的 `Dockerfile` 自 2026-11-05 起**只剩注释、故意不含 `FROM`** ⇒ **不可构建**：
> `docker build` 会直接报「没有构建阶段 / Dockerfile 为空」，一眼就懂「这不是拿来构建的」，
> 且**不可能产出任何镜像**。
> 废弃原因：它把 `MIREDIT_PALETTE_JSON` 写成 `/data/client/catalogue/palette.json`，
> 而 `mir2-stamp` 按 `dataRoot + objectRoot` 定位 `.X`（`dataRoot` = 该文件的父目录的父目录，
> `objectRoot="Objects"`）⇒ 它会去 `/data/client/Objects` 找印章，永远找不到，
> 于是 `/api/stamps` **返回 200 但每条 `cells`/`bounds` 全是 `null`**（静默降级，不报错）。
> 正确值是 `/data/crystalstudio/catalogue/palette.json`。
>
> ⚠ 上面的报错文案依 docker 行为推断，本机无 Docker **未实跑**；也**别**改回 `FROM scratch` ——
> 那是合法空镜像，构建会「成功」出一个不可运行的镜像。

```bash
# 推荐直接用仓库根的 compose（上下文与挂载都已按容器路径写好）
cd docker && docker compose up -d --build

# 或手工构建（构建上下文必须是仓库根，原因见 docker/Dockerfile 头注释）
docker build -f docker/Dockerfile -t miredit-web:0.1.0 .
docker run --rm -p 8080:8080 \
  -v /path/to/maps:/data/maps:ro \
  -v /path/to/client-root:/data/client:ro \
  -v /path/to/crystalstudio/catalogue:/data/crystalstudio/catalogue:ro \
  -v /path/to/crystalstudio/Objects:/data/crystalstudio/Objects:ro \
  miredit-web:0.1.0
curl http://127.0.0.1:8080/api/health
```

> 该镜像**自带**静态前端（`/app/web`），所以不再挂 `miredit/web`；
> `/data/crystalstudio` 必须挂，否则 `/api/stamps` 的 `cells`/`bounds` 全为 `null`。

> ⚠ **本机没有安装 Docker**（`docker` 不在 PATH，也没有 Docker Desktop / WSL），
> 所以 `docker build` / `docker run` **未实跑**。见 §六「未验证项」。

---

## 二、端点

| 方法 | 路径 | 成功响应 |
|---|---|---|
| GET | `/api/health` | `{"ok":true,"version":"0.1.0"}` |
| GET | `/api/map` | `{"path","width","height","format","dirty","tolerant_trailer_bytes","format_label","editable","ground_paint_supported","edit_warning","disk_cell_len"}`；没开图时除 `width/height` 外全为 `null`（两个能力布尔也是 `null`，**不写 `false`**）。`editable` 与 `edit_warning` 的口径见 §2.0.1 |
| POST | `/api/map/open` `{"path":"…"}` | 同上 |
| GET | `/api/tiles?x&y&w&h&scale&layers&collision&doors&lighting` | `image/png` |
| POST | `/api/edit` `{"cmd":…}` | `{"changed","dirty","note","can_undo","can_redo"}` |
| POST | `/api/save` | `{"saved":true,"bytes":N,"note":null\|"…"}` |
| GET | `/api/palette?kind=&page=&size=` | `{"items":[{"lib","name","frames"}],"total":N}` |
| GET | `/api/stamps?kind=&q=&page=&size=` | `{"items":[{"id","name","kind","kind_zh","cells","bounds"}],"total":N}` |
| WS | `/api/events` | 推送 `{"type":"dirty\|rerender\|error","payload":{…}}` |
| GET | `/` 与 `/*` | 静态文件（`MIREDIT_WEB_ROOT`），找不到就 404 JSON |

### 2.0 `/api/map` 的十一个字段（2026-11-05 新增后五项）

| 字段 | 来源 | 说明 |
|---|---|---|
| `path` / `width` / `height` / `dirty` | `Document` / `EditSession` | 同旧版 |
| `format` | `Document::format_name()` | **视图档位** `Old12`/`New14`/`Ext36`，不是磁盘格长 |
| `tolerant_trailer_bytes` | `mir2_format::read_any_tolerant` 的 `trailer_len()` | 只有宽松判别才非 0 |
| `format_label` | `Document::format_label()` | 九种格式真名（带格长与出处） |
| `editable` | `Document::caps().editable` | 是否承诺「改格后语义自洽」。**只有 `Old12` / `Shanda14` 为 `true`**（2026-11-05 订正；`Old12` 旧值为 `false`）；其余 7 种为 `false`，各带一句具体原因（`FormatCaps::editable_caveat`）。⚠ `false` **不等于不能编辑** |
| `ground_paint_supported` | `Document::ground_paint_supported()` | 地表能否逐格落笔（只有 Type5 为 `false`） |
| `edit_warning` | `EditSession::edit_warning()` | **常驻告警原文**（`null` = 没事；前端整句上屏，本 crate 不改写） |
| `disk_cell_len` | `Document::disk_cell_len()` | 磁盘单格字节数；变长格式 ⇒ `null` |

### 2.0.1 `editable` 与 `edit_warning` 的口径（2026-11-05 订正）

**契约**（真源 `mir2-format/src/registry.rs` 的 `editable与caveat必须一致`）：

> **`editable == false` ⟺ `FormatCaps::editable_caveat` 非空。**

本 crate 只**转发**（`editable` ← `caps().editable`，`edit_warning` ← `EditSession::edit_warning()`），
不推断、不改写、不截断。由此有两条可断言的推论（`src/routes/map.rs` 的
`editable与告警互为反相` 钉住）：

1. `editable == false` ⇒ `edit_warning` **必然非空**（至少含那句 caveat；若同时有未解尾块，
   两段用「；」连接，尾块那句在前）；
2. `editable == true` ⇒ 格式段**一个字都不出**，`edit_warning` 只可能来自未解尾块
   ⇒ **尾块 0 时恒为 `null`**。

九种格式的新取值（真实样本数 = `mir2-format` 的 `real_samples.rs` 全量扫描口径，合计 1,158 张）：

| 格式 | `editable` | caveat 摘要 | 真实样本 |
|---|---|---|---|
| `Old12` | **`true`**（旧值 `false`，已订正） | —（12 字节全部有名字、无未解区） | 856 |
| `Shanda14` | `true` | — | 107 |
| `Shanda2012` | `false` | 每格 22 字节未建模尾区（格内 `rel 14-35`） | 195 |
| `AntiHack` | `false` | 12 字节全有名字，但本机 0 张真实样本，布局仅据 C# 源码 | 0 |
| `Wemade2010` | `false` | 每格 1 字节未建模（`rel 14`）+ 背景图号高 16 位改不到 | 0 |
| `Heroes34` | `false` | 每格 1 字节未建模（`rel 14`）+ 背景图号高 16 位改不到 | 0 |
| `Mir3Wemade` | `false` | 每格 4 字节未建模 + 碰撞/可走 flag 与中层库号/动画帧不在编辑视图里 | 0 |
| `Mir3Shanda` | `false` | 每格 7 字节未建模 + 碰撞/可走 flag 与背景/中层库号、中层动画帧不在编辑视图里 | 0 |
| `CrystalCs` | `false` | 格长变长（26/38/35/33/39/40）+ `ver>=2` 起有未建模字节 + `tiles`/`sm_tiles` 高 8 位改不到 | 0 |

**告警覆盖实测**（`mir2-format` 的 `告警覆盖实测`，`MIR2_GATE_CORPUS=1` 才跑）：
**1,051 / 1,158 张（90.8%）→ 262 张**。明细：`Old12` 856 张里只剩 **67 张**因真尾块告警，
`Shanda2012` 195 张全部因格式告警，`Shanda14` 107 张 **0 告警**；
缺省打开的 `GodotClient/maps/0.map`（Old12，700×700，尾块 0）现在 `edit_warning = null`。

⚠ 旧口径的两处事实错误（已随本次订正消失）：
① `Old12` 被标成 `editable = false` —— 它没有"未解区"；
② `edit_warning` 对**任何**格式都追加一句「本格式（X）未解区语义未知」——
   于是 90.8% 的真实图一开图就挂常驻黄条，用户学会无视它，真正要看的那条（尾块可能整图错位）被淹没。

### 2.0.2 错误体统一（任何非 2xx，**绝不 200 带 error**）

```json
{"error":{"code":"forbidden_path","message":"路径不允许包含 `..`：../../../etc/passwd"}}
```

| `code` | HTTP | 何时 |
|---|---|---|
| `bad_request` | 400 | 参数缺失 / 越界 / 枚举不认识（含 `lighting` 不是三档之一、空值）/ `cmd` 结构不对 |
| `forbidden_path` | 403 | 路径逃出白名单根（含 `..`、根外绝对路径） |
| `not_found` | 404 | 文件 / 静态资源 / 未实现的端点 |
| `no_map` | 409 | 尚未打开地图 |
| `too_large` | 413 | 出图超限（目标图 > 64 MiB / 1× 中间图 > 128 MiB，见 §2.1） |
| `parse_failed` | 422 | 文件在，但 `.map` 三档判别都不命中 |
| `io_error` | 500 | 读写磁盘失败（含保存的四个阶段） |
| `assets_unavailable` | 503 | 图库 / 印章目录不可用 |
| `internal` | 500 | 其它内部错误（如 PNG 编码失败） |

### 2.1 `/api/tiles` 的参数

| 参数 | 取值 | 缺省 |
|---|---|---|
| `x` / `y` | 世界格，`0..=2147483647` | `0` |
| `w` / `h` | `1..=64` 格 | `16` |
| `scale` | `0.25` / `0.5` / `1` / `2`（也收 `quarter` / `half` / `one` / `two`，**大小写不敏感**）；别的值 ⇒ 400 | `1` |
| `layers` | `ground,middle,front` **或** 位掩码 `0..=7`（1=地表 2=中层 4=物体） | 三层全开 |
| `collision` | `low,high` **或** 位掩码 `0..=3`（1=低位 2=高位） | `0` |
| `doors` | `0`（闭门）/ `1`（门全开）；别的值（含空值 `doors=`、`2`、`true`、`01`）⇒ **400** | `0`（= 改动前的产物，**逐字节相同**） |
| `lighting` | `day`（白天）/ `dawn_dusk`（黎明·黄昏）/ `night`（夜晚），**大小写不敏感**；别的值（含空值 `lighting=`、`xyz`、`dawn-dusk`）⇒ **400 + 中文原因**，不静默当成白天 | `day`（不画遮罩，= 改动前的产物，**逐字节相同**） |

输出像素 = `round(w*48*s) × round(h*32*s)`（格 48×32 来自 `miredit-render`；`s=1` 时即 `w*48 × h*32`）。
越界部分透明；缺帧不画、不占位。响应头三条：`X-Render-Scale` = **实际出图档位**（取自
`Rendered.scale`）；`X-Render-Doors` = 本次门口径 `0|1`；`X-Render-Lighting` = 本次光照档位
`day|dawn_dusk|night`（后两条是**规范化回显**，取自真正传给渲染库的 `RenderOptions`，非法值在进
渲染前已 400，故回显值恒等于生效值）。三条都在 `server::EXPOSED_HEADERS` 里 —— 跨源时页面 JS 才读得到。

| 档位 | 走哪条路 | 滤镜 |
|---|---|---|
| `1` | `try_render_region_with_options`（`options=RenderOptions::default()` 时与旧 `try_render_region_with` **同一次调用**） | — |
| `0.25` / `0.5` | `try_render_region_scaled_with` | 面积加权 `Box`（流式） |
| `2` | 同上 | `Nearest`（放大不插值） |

**`doors=1` 改什么**：只改**前景层帧号** —— `(door_index & 0x7F) != 0` 的格再加
`door_offset & 0x7F`（官方客户端 `GameScene.cpp:3328-3388`；语义与出处在
`miredit_render::RenderOptions` 的类型文档里，本 crate **只传参**）。
`doors` 缺省 / `=0` 时帧号仍只有 `(fr_img & 0x7FFF) - 1` ⇒ 与引入该选项前**逐字节相同**
（实测见 `web/README.md` §9）。

**`lighting` 改什么**：三档遮罩由 `miredit_render::lighting` 实现，本 crate 只把协议值显式映射到
`MapLighting` 并与 `doors` **一次**组合进 `RenderOptions::with_doors_and_lighting(open, lighting)`，
两个开关互不覆盖。`day` 完全不画遮罩；`dawn_dusk` 遮罩 alpha 205；`night` 遮罩 alpha 255（逐格光源
另按官方光罩贴图减 alpha）。⚠ 渲染库日志短名是 `dawn-dusk`（连字符），接口契约是 `dawn_dusk`
（下划线）：两套写法不复用、不互相推断。`doors` / `lighting` 都缺省时等于
`RenderOptions::default()`，与改动前**逐字节相同**。

**超限 ⇒ 413 `too_large`**（不是 400，更不是 500）：目标图 > 64 MiB（`TooLarge`）或
1× 中间图 > 128 MiB（`IntermediateTooLarge`），文案带 `want`/`limit` 数字与「缩小视口格数或降低倍率」。
典型：`64×48 @2` 目标图 `6144×3072×4 = 72 MiB`。

### 2.2 WebSocket 报文

```json
{"type":"dirty","payload":{"dirty":true}}
{"type":"rerender","payload":{"x":1,"y":2,"w":1,"h":1}}   // w/h = 0 表示"整屏"
{"type":"error","payload":{"code":"io_error","message":"…"}}
```

心跳 `Ping` 每 30s 一次（同时用来发现对端已断开）。客户端发来的消息**一律忽略**
（本通道单向；编辑走 `POST /api/edit`）。追不上时跳过过期事件，不断连接。

---

## 三、`cmd` 的 JSON 形态（**照 `miredit-core/src/cmd.rs` 的 `enum Command` 直译，十二个变体**）

内核的 `Command` **没有** serde derive（那个 crate 刻意不引 serde），所以这一层是手写的
（`src/json.rs`），字段名逐字对齐 `cmd.rs`：

| JSON | `Command` 变体 | 说明 |
|---|---|---|
| `{"SetLayer":{"layer":"ground"}}` | `SetLayer { layer: String }` | 只认 `ground` / `middle` / `front` / `low_collision` / `high_collision`；别的 ⇒ 400（**不猜**） |
| `{"SetBrushSize":{"size":7}}` | `SetBrushSize { size: u16 }` | 会被夹进 `3..=31` 的奇数，回执里给真正生效的值 |
| `{"SetTool":{"tool":"eraser"}}` | `SetTool { tool: String }` | 只认 `brush`/`bucket`/`eraser`/`pickup`（严格小写，**不收别名**；键校验在 core 的 `set_tool_by_key`）；非法键 ⇒ `200 + changed=0 + 中文原因`，**不改会话状态** |
| `{"Paint":{"x":100,"y":200}}` | `Paint { x: u16, y: u16 }` | 按**当前笔刷尺寸**落一笔（默认 3×3 ⇒ 最多 9 格） |
| `{"Stroke":{"from":[10,10],"to":[20,20]}}` | `Stroke { from: (u16,u16), to: (u16,u16) }` | 元组 ⇒ JSON 两元素数组；整条轨迹 = 一步撤销 |
| `{"Erase":{"x":10,"y":10}}` | `Erase { x: u16, y: u16 }` | 按当前笔刷尺寸擦一笔；**执行后内核工具停在橡皮** ⇒ 切回画笔必须显式发 `SetTool` |
| `{"Fill":{"x":358,"y":0}}` | `Fill { x: u16, y: u16 }` | BFS 连通填充，整次 = 一步撤销 |
| `{"Pick":{"x":10,"y":10}}` | `Pick { x: u16, y: u16 }` | 吸管：把该格**当前层**的内容取成笔刷（不改地图） |
| `{"SetSelection":{"x0":1,"y0":2,"x1":5,"y1":6}}` | `SetSelection { x0,y0,x1,y1: u16 }` | 矩形选区（**闭区间**，越界裁剪）；纯 UI 状态，不改图/不置脏/不产生撤销步 |
| `"ClearSelection"` | `ClearSelection` | 取消选区（**不清剪贴板**） |
| `"CopySelection"` | `CopySelection` | 复制选区进剪贴板（`changed=0`，不改图） |
| `"CutSelection"` | `CutSelection` | 剪切 = 复制 + 清空（一步撤销） |
| `{"PasteSelection":{"x":1,"y":2}}` | `PasteSelection { x, y: u16 }` | 以 `(x,y)` 为左上角粘贴；越界逐格跳过、`bit15` 保留、Type5 闸门 |
| `"DeleteSelection"` | `DeleteSelection` | 清空选区内容（一步撤销） |
| `"FillSelection"` | `FillSelection` | 用当前笔刷填满选区（一步撤销） |
| `{"PlaceStamp":{"x":1,"y":2,"stamp":"mir/mir_00000"}}` | `PlaceStamp { x, y, stamp: String }` | 放印章；**格表由本 crate 先读盘注册**（见 §3.1） |
| `"Undo"` / `"Redo"` / `"Save"` | `Undo` / `Redo` / `Save` | 无参变体 ⇒ JSON 字符串；也接受 `{"Undo":null}` |

> **七条选区/剪贴板命令全部开放**（含 `Copy`/`Cut`/`Clear`）。理由：`PasteSelection`
> 的剪贴板只能由 `CopySelection`/`CutSelection` 填 —— 只开 Paste 等于给一个永远
> `changed=0 + "剪贴板为空"` 的假功能。前端**本轮没有选区 UI**（见 §六）。

**网页侧额外一条**（不是 `Command` 的变体，直译 `cmd::set_brush_from_palette`）：

| JSON | 说明 |
|---|---|
| `{"SetBrushFromPalette":{"family":"ground","lib":0,"frame":3}}` | `family` = `ground`/`middle`/`surface`（`object`/`front` 是 surface 的别名）；`lib` = 族内库号；`frame` = **0 基**帧索引（内核换算成 1 基图号） |

> 为什么要有它：新会话**没有笔刷**，此时 `Paint` 会如实返回 `changed=0` +
> `"没有笔刷：先用吸管取一格，或在资源面板选一帧"`。资源面板选了帧就得有办法把笔刷设进去。

**`changed == 0` 不是错误**：族不匹配 / 没有笔刷 / 值没变 / 没有可撤销的步骤 / 工具名不合法
都是 `200 + changed=0 + note=原因`（内核的口径，见 `miredit-core/src/error.rs` 模块文档）。

### 3.1 `PlaceStamp` 的注册分工（web 读盘，core 零 IO）

```text
web:  StampCatalog::resolve(id) → 路径 → 读 .X → Stamp::parse
      → miredit_core::cells_from_stamp（全仓唯一换算口）
      → session.register_stamp(id, cells) → apply(Command::PlaceStamp{ x, y, stamp: id })
core: cells_from_stamp / register_stamp / place_stamp_by_id / Command::PlaceStamp（一格 IO 都没有）
```

- 读盘在**锁外**（`routes::edit::prepare_stamp`），锁内只 `register_stamp` + `apply`；
- 同一会话同一 id **只读一次盘**（已注册就直接放）；
- **字段换算不在本 crate**（2026-11-05 收口）：`miredit_core::cells_from_stamp`
  （`miredit-core/src/stamp_convert.rs:154`）是**全仓唯一**的 `.X → StampCellInput` 适配，
  本 crate 的 `routes::stamps::to_stamp_inputs` 只剩一行 `map_err`（翻 HTTP 错误）。
  口径以 core 的模块文档为准：三族图号一律 `& 0x7FFF`（`bit15` 是阻挡位不是图号）、
  三族库号必须 `0..=255`、`door` / `door_offset` / `light` / 动画五字段照搬 `.X` 真值；
- 改前本 crate 自己逐字段填，库号写的是 `back_library as u8` —— **`i16 → u8` 静默截断**：
  库号 `300` 变 `44`，贴出来的印章指向**另一个图库**、外观错还不报错。
  现在越界一律 **422**，`message` 是 core 原文
  （「第 1 格 / 地表 / 值 300 超出库号字节范围 0..=255（拒绝截断：截断会静默指向另一个图库）」）；
- 失败口径：id 不存在 ⇒ **404**；`.X` 坏了 ⇒ **422**；**库号越界 ⇒ 422**（同码 `parse_failed`）；
  读不了 ⇒ **500**；目录不可用 ⇒ **503**；注册被拒（0 格 / 超 1,000,000 格 / 注册表满）⇒ **422**（原因用内核中文串）。

---

## 四、与前端 `web/README.md` 的契约差异（如实登记）

任务书 §三 的端点表与 `miredit-core::Command` 优先；前端 README 是**更早的一版草稿**，
下面这些地方**对不上**，接的时候以前者为准：

| # | 项 | 前端 README | 本实现 | 影响 |
|---|---|---|---|---|
| 1 | `cmd` 形态 | `{"op":"paint","layer":"ground","x":12,"y":34,"value":1234}` | `{"Paint":{"x":12,"y":34}}`（外部标记枚举） | **前端要改**：没有 `op`、没有 `value`；切层是独立命令 `SetLayer` |
| 2 | `/api/edit` 响应 | `{changed,dirty,note}` | 多 `can_undo` / `can_redo` | 只多不少，前端可忽略 |
| 3 | `scale` | `0.25 / 0.5 / 1 / 2` | **只 1**（其它 ⇒ 400） | 前端 4 档缩放**暂不可用**：`miredit-render` 只实现 1× 合成，插值出来的像素是假数据 |
| 4 | `layers` / `collision` | 位掩码 | 位掩码 **或** 名字列表 | 兼容，无影响 |
| 5 | `/api/palette` item | `{lib,image,kind,label}`（图块级） | `{lib,name,frames}`（**库级**） | 前端要改成"先选库、再按帧取图" |
| 6 | `/api/stamps` item | `{id,label,w,h,preview}` | `{id,name,kind,kind_zh,cells,bounds}` | 无 `preview`：资源里预览 PNG 未生成（`mir2-stamp` 模块文档已登记），不伪造 URL |
| 7 | `/api/map` | 无 `tolerant_trailer_bytes` | 有（另加 `format_label`/`editable`/`ground_paint_supported`/`edit_warning`/`disk_cell_len`） | 只多不少 |
| 8 | `/api/health` | 图库没就绪时 503 `ok:false` | **恒 200 `ok:true`** | 进程活着就算健康；"图库没配好"由 `/api/tiles` 的 503 表达，两个状态不互相污染 |
| 9 | 错误码表 | 无 `forbidden_path` | 有（403） | 前端对未知 code 会显示原文 `message`，不会崩 |
| 10 | `POST /api/map/open` | 任意绝对路径 | **绝对路径也必须在 `MIREDIT_MAP_ROOT` 内** | 安全要求（防目录穿越）；根设错会 403 |

---

## 五、并发与安全

- **会话状态**：`Arc<RwLock<Option<EditSession>>>`（`src/state.rs`）。
  出图取**读锁**（可并发），编辑 / 打开 / 保存取**写锁** ⇒ 同一时刻只有一个写命令。
- **锁的边界**：写锁只覆盖 `apply()` 那一次调用；**任何 `await` 都不在持锁期间发生**。
  几十毫秒级的渲染 / PNG 编码 / 5 MB 读盘用 `tokio::task::block_in_place` 让出 worker。
- **路径白名单**：`resolve_map_path` 是唯一入口，四道闸 ——
  ① 非空；② **任何 `..` 分量一律拒绝**；③ `canonicalize` 后必须仍在根内（顺带挡符号链接）；
  ④ 必须是普通文件。任一条不过 ⇒ 400/403/404。
- **CORS**：只回显 `http://localhost:<任意端口>` / `http://127.0.0.1:<端口>` / `http://[::1]:<端口>`，
  **不放 `*`**（否则任意网页都能调 `/api/save` 改你的地图）。
- **静态文件**：`..` 拒绝 + `canonicalize` 后仍在 `web_root` 内；未实现的 `/api/*` 走 JSON 404。

---

## 六、未做 / 未验证（诚实清单）

| 项 | 状态 | 原因 |
|---|---|---|
| `docker build` / `docker run` | **未实跑** | 本机没有 Docker（无 CLI、无 Desktop、无 WSL）。Dockerfile 的上下文关系与路径已逐条核对，但"实跑成功"这条证据缺席 |
| ~~`scale != 1` 的瓦片~~ | ✅ **已收口（2026-11-05）** | `/api/tiles` 现接受 `0.25 / 0.5 / 1 / 2`（含 `quarter|half|one|two`，大小写不敏感），走 `miredit_render::try_render_region_scaled`；`1` 仍走旧路径 `try_render_region_with`（**逐字节相同**）。超限（目标图 > 64 MiB / 1× 中间图 > 128 MiB）⇒ **413 `too_large`**，不硬渲也不降级 |
| 印章预览图 `preview` | **不给** | `mir2-stamp` 已登记「预览 PNG 未生成」；本 crate 不伪造 URL |
| 单元格检查器（某格的图号 / 阻挡位） | **没有端点** | 任务书 §三 的端点表里没有；`/api/*` 未列出的路径一律 404（**不编造**） |
| **印章放置的前端交互** | 后端**已通**，前端未接 | `{"PlaceStamp":…}` 可直接用；`web/js/panels.js` 只做目录浏览，并已如实标注「前端放置交互本轮未接」 |
| `tolerant_trailer_bytes` 的**修复** | 只报告，不修 | 该字段是"宽松口径下有多少字节没被解释"，`!= 0` 时本 crate 会打告警；**不会**替你截断或补齐。保存时尾块**逐字节原样写回**（实测 8040 字节全等） |
| WS 断线重连 | 不做 | 与前端约定一致（前端也不自动重连，只记日志） |

> ✅ 已收口（2026-11-05）：**多格式地图（九种）现在能开**（`Document::load` 走
> `mir2_format::read_any_tolerant`）；**橡皮擦**（`SetTool` + `Erase`）；**印章放置**
> （`Command::PlaceStamp` + 本 crate 的 `.X` 读盘注册）。

---

## 七、文件与行数

| 文件 | 行数 | 作用 |
|---|---|---|
| `src/main.rs` | 140 | 入口：env → 状态 → `axum::serve` + 优雅关闭 |
| `src/server.rs` | 385 | 路由表 + CORS 中间件 + `/api/health`（**三条自定义头统一暴露**） |
| `src/state.rs` | 421 | 共享状态、会话锁、路径白名单、事件通道 |
| `src/json.rs` | 503 | `Command` ↔ JSON（手写直译）+ 族/库换算 |
| `src/json_tests.rs` | 209 | `json.rs` 的测试段（`#[cfg(test)] #[path]` 挂回，两边同属一个模块） |
| `src/error.rs` | 178 | `ApiError` + `IntoResponse` + `CoreError` 映射（含新增的 413 `too_large`）+ `StampConvertError` 映射（422） |
| `src/static_files.rs` | 140 | `../web/` 静态文件 + MIME + 404 |
| `src/routes/mod.rs` | 274 | 请求体/查询参数工具 + PNG 编码 + 两条路径的等价自证 + 多倍率尺寸/体积自证 |
| `src/routes/map.rs` | 452 | `/api/map`、`/api/map/open` + 十一字段组装 + `editable`/`edit_warning` 契约测试 |
| `src/routes/tiles.rs` | 446 | `/api/tiles`（出图，四档 `scale` + `doors` + 三档 `lighting`）+ 参数解析 + 413/400 映射；原有测试在 `src/routes/tiles/tests.rs`（524），本轮光照测试在 `src/routes/tiles/lighting_tests.rs`（90），拆出以守住「单文件 ≤600 行」 |
| `src/routes/edit.rs` | 210 | `/api/edit` + `PlaceStamp` 的锁外注册 + 事件 |
| `src/routes/save.rs` | 73 | `/api/save` |
| `src/routes/palette.rs` | 88 | `/api/palette` |
| `src/routes/stamps.rs` | 369 | `/api/stamps` + `load_stamp_cells`（`.X` 读盘 → 调 core `cells_from_stamp`，本文件不再搬字段） |
| `src/routes/events.rs` | 91 | `/api/events`（WS） |
| `Dockerfile` | 12 | **已废弃（2026-11-05）**：只剩注释、故意不含 `FROM`，不可构建。有效文件是仓库根的 `docker/Dockerfile`（多阶段 + 非 root + `EXPOSE 8080`） |
| `Dockerfile.dockerignore` | 22 | 构建上下文排除（BuildKit 的 `<Dockerfile>.dockerignore`） |

单文件均 ≤ 600 行（最大 `json.rs` 503 行；测试段拆到 `json_tests.rs` / `tiles/lighting_tests.rs` 才压下来的）。
⚠ 行数为 **2026-11-05 实测**（`Get-Content | .Count`）。旧表 17 行里有 14 行已经漂移
（含 `src/main.rs` 127→140；`Dockerfile` 63→77 后又于 2026-11-05 废弃成 12 行指针），**漂移原因未逐条追溯**；本次按实测值整表订正。

---

## 八、依赖与理由

| 依赖 | 版本 | 为什么允许 |
|---|---|---|
| `axum` | 0.8（`ws`） | 一套 `Router` 同时覆盖 REST 与 WebSocket；与 hyper 1.x / tower 同源，不必再引 `hyper` / `tower-http`（CORS 与静态文件本 crate 自己写） |
| `tokio` | 1（`rt-multi-thread`/`macros`/`net`/`signal`/`sync`/`time`/`fs`） | 异步运行时；`signal` 接 Ctrl-C 与容器 SIGTERM，`sync` 给 WS 广播 |
| `serde` / `serde_json` | 1 | 请求/响应序列化。`Command` 没 derive，转换手写在 `json.rs` |

除以上四条，其余全是同 workspace 的本地 crate（`miredit-core` / `miredit-render` /
`mir2-assets` / `mir2-format` / `mir2-stamp` / `mir2-map`）—— 它们是"复用"，不是新增第三方。
