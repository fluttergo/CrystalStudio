# Crystal Studio 网页版地图编辑器（前端 + 接口契约）

原生 HTML/CSS/JS，**不引任何框架、不引 CDN、不引 npm 包**，字体走系统栈。
后端 `crates/miredit-web`（axum）**已实现**，本文档按**实际代码**写，不是草案
（对照文件：`crates/miredit-web/src/{json.rs,routes/*.rs,state.rs,error.rs}`）。

- 目录：`web/`
- 入口：`index.html`（后端 `GET /` 直接 serve 本目录，目录由 `MIREDIT_WEB_ROOT` 指定；找不到就 404，不伪造页面）

---

## 1. 目录结构

| 路径 | 作用 |
|---|---|
| `index.html` | 骨架：顶栏 / 三栏工作区 / 状态栏 / 抽屉 / 浮层 / **光照三选** / **打开地图对话框（列表 + 手打路径退路）** / 常驻告警条 / 模式提示条 |
| `runtime-config.js` | 容器运行时注入点（默认空，`entrypoint.sh` 会覆盖） |
| `css/app.css` | 浅色金主题 + 响应式两档 + 刘海屏安全区（**599/600，新样式别再往里加**） |
| `css/brush-size.css` | 笔刷尺寸控件（拆出来守住「单文件 ≤600 行」） |
| `css/doors.css` | 「开启门」复选框 |
| `css/lighting.css` | 「地图光照」三选按钮（三档互斥；移动端按钮 ≥44px） |
| `css/maplist.css` | **地图列表**（`GET /api/maps`）：搜索行 / 计数行 / 可滚列表 / 手机底部抽屉 |
| `js/config.js` | 可覆盖配置、契约常量（含光照三档 / 归一化函数）、工具/图层表、事件总线、调试钩子 |
| `js/api.js` | HTTP / WebSocket 客户端 + 错误码中文映射 + **光照响应头真值** + **WS 帧解码** + `listMaps` |
| `js/maplist.js` | **地图列表**：按 500/页拉全量（`ceil(total/500)` 次请求）、**纯客户端过滤**、点一项走 `map:open-path` |
| `js/tiles.js` | 瓦片取图 + LRU 缓存（**键 = 图身份 + 出图参数，含 lighting**）+ 在途请求作废 + 像素采样 |
| `js/canvas.js` | canvas 2D 渲染、**前端四档缩放**、平移、适应地图、**选区框绘制**、光照参数透传 |
| `js/touch.js` | 指针手势：工具落点 / 平移 / 捏合 / 滚轮（**触摸单指延迟 120 ms 落笔，第二指落下即转捏合**） |
| `js/edit.js` | 编辑命令队列（串行 + **同一条命令**去重；含印章与七条选区命令） |
| `js/selection.js` | **矩形选区 + 剪贴板**：拖动框选、Ctrl+C/X/V/Delete、锚点、状态文本 |
| `js/stampmode.js` | **印章待放置态**：进入/取消、点画布放置、失败原文上顶部错误条 |
| `js/panels.js` | 资源库列表 / 印章列表（**行内「放置」+ 长按 + Enter**）/ 单元格检查器 |
| `js/ui.js` | 顶栏、工具、图层、**光照三选**、状态栏真值、横幅、告警条、模式提示条、抽屉、打开地图对话框、快捷键 |
| `js/app.js` | 启动流程、开图、编辑派发、保存/撤销/重做、WS 事件（未知类型计数报警）、画布事件路由 |
| `tiles/**` | **历史静态夹具（2026-11-04 生成，当前前端已不再引用）**，保留备查，可安全删除 |

---

## 2. 运行方式

### 2.1 起后端（**环境变量，不是命令行参数**）

```bash
export MIREDIT_BIND=127.0.0.1:18080
export MIREDIT_MAP_ROOT=/path/to/maps        # /api/map/open 的白名单根
export MIREDIT_CLIENT_ROOT=/path/to/client-root
export MIREDIT_PALETTE_JSON=/path/to/client-root/catalogue/palette.json
export MIREDIT_WEB_ROOT=/path/to/repo/web
cargo run -p miredit-web --release
```

| 变量 | 缺省 | 未设置时 |
|---|---|---|
| `MIREDIT_BIND` | `0.0.0.0:8080` | 用缺省值 |
| `MIREDIT_MAP_ROOT` | **无（必须显式设置）** | 开图/列表端点 503 `assets_unavailable`，文案点名该变量 |
| `MIREDIT_CLIENT_ROOT` | **无（必须显式设置）** | 图库为空 ⇒ 出图 / 素材端点 503「图库未就绪」 |
| `MIREDIT_PALETTE_JSON` | **无（必须显式设置）** | `/api/stamps` 503（**不猜** `catalogue/palette.json`） |
| `MIREDIT_WEB_ROOT` | 本仓 `<仓库根>/web`（中立回退） | 目录不存在 ⇒ `GET /` 404（**不伪造页面**） |

> ⚠ 四个路径变量都**没有**内置缺省值（2026-11-06 起）：公开仓不绑任何开发机路径。
> 不设就是「如实不可用」——503/404 + 启动横幅点名缺哪个变量，**不会**去猜迁移前的目录布局。

浏览器打开 `http://127.0.0.1:18080/` 即可（前端与 API 同源，无 CORS 问题）。

### 2.2 前端可覆盖配置

| 全局变量 | 默认 | 说明 |
|---|---|---|
| `window.__API_BASE__` | `''` | 后端根，空 = 同源 |
| `window.__MIREDIT_CONFIG__` | `{}` | 对象形式覆盖 `apiBase` / `healthTimeout` / `tileCacheSize` / `lastMapKey` |

### 2.3 Docker

见 [`../docker/README.md`](../docker/README.md)（静态前端镜像，`docker compose up` 后访问 8080）。

---

## 3. 接口契约（**按后端实际实现**）

### 3.0 通用约定

- **编码**：JSON 一律 `application/json; charset=utf-8`；`/api/tiles` 返回 `image/png`。
- **路径**：`/api/map/open` 的 `path` 必须在 `MIREDIT_MAP_ROOT` 之内；**前端一律传相对路径**
  （如 `maps/0.map`）。含 `..` 或指向根外 ⇒ **403 `forbidden_path`**。
- **坐标**：世界格整数，左上角 `(0,0)`；`x` 向右、`y` 向下。
- **错误体**：所有非 2xx 一律
  ```json
  { "error": { "code": "no_map", "message": "尚未打开地图" } }
  ```
- **健康判断**：只看 `GET /api/health` 是否 200 + `ok:true`；**不再用它判断图库是否就绪**
  （图库缺失由 `/api/tiles` 的 503 体现）。

---

### 3.1 `GET /api/health`

**200（恒）**
```json
{ "ok": true, "version": "0.1.0" }
```
> 进程能应答就返回 200。图库/印章目录不可用**不算**进程不健康。

---

### 3.2 `GET /api/map`

**200（已打开）** —— 示例（`EM001.map` 一类带 8,040 字节未解尾块的地图；`path` 已换成占位）：
```json
{
  "path": "/path/to/maps/EM001.map",
  "width": 30, "height": 35,
  "format": "Old12",
  "dirty": false,
  "tolerant_trailer_bytes": 8040,
  "format_label": "Old12（12B/格，TMapInfo 旧版）",
  "editable": true,
  "ground_paint_supported": true,
  "edit_warning": "本图有 8040 字节未解释尾块（按头部 30×35 解释）——若真实尺寸不是头部声明值，列优先展开会让整图错位",
  "disk_cell_len": 12
}
```

**200（未打开）** —— `path` / `format` / `format_label` / `tolerant_trailer_bytes` / `edit_warning` /
`disk_cell_len` / `editable` / `ground_paint_supported` 全为 `null`，`width/height` 为 0。

| 字段 | 类型 | 说明 |
|---|---|---|
| `path` | string \| null | 绝对路径（去掉 Windows `\\?\` 前缀） |
| `width` / `height` | int | 世界格 |
| `format` | string \| null | `Old12` / `New14` / `Ext36` —— **视图档位**（三档语义投影），不是磁盘格长 |
| `dirty` | bool | 有未保存修改 |
| `tolerant_trailer_bytes` | int \| null | **宽松口径下未解释的尾块字节数**；`null` = 算不出来（不写 0 冒充干净） |
| `format_label` | string \| null | **九种格式的真名**（带格长与出处），如 `Shanda14（14B/格，TNewMapInfo）` |
| `editable` | bool \| null | 该格式是否承诺「改格后语义自洽」（九种里 `Old12` 与 `Shanda14` 为 `true`）。⚠ `false` **不等于不能编辑** |
| `ground_paint_supported` | bool \| null | 地表能否**逐格**落笔（九种里只有 `Mir3Wemade`/Type5 为 `false`） |
| `edit_warning` | string \| null | **常驻告警原文**（`null` = 没事）；前端**整句原样上屏**，一个字符都不改写 |
| `disk_cell_len` | int \| null | **磁盘单格字节数**；`null` = 变长格式（`CrystalCs`）；`Wemade2010` 定长 15 B/格 ⇒ `15` |

> ⚠ 未开图时 `editable` / `ground_paint_supported` 是 **`null` 而不是 `false`**：
> `false` 表示「这个格式不支持」，`null` 才是「没有图，谈不上」。混用会让前端把
> "没开图"误报成"该格式不可落笔"（假数据）。

> **`edit_warning` 非空 ⇒ 顶部常驻黄条**（不是 toast，用户不点 × 就一直挂着）：
> 文案 = 后端原文；另加一个**格式标记**（`format_label` 短名，`editable=false` 时再追加
> 「未解区语义未知」，如 `Shanda2012 · 未解区语义未知`；`Old12` / `Shanda14` 只说格式名）。
> 告警的两种来源（未解尾块 / 该格式不承诺编辑语义）可能**同时**成立，后端用「；」连好。

> **`ground_paint_supported === false`（Type5）⇒ 地表图层行加「不可落笔」标记**，
> 且前端在**点画布时就先拦**（不等到后端拒绝）：提示里带 `format_label` 原文，
> 并说明「背景图号按 2×2 块编码，逐格写会被写回时丢弃」。当前编辑层是 `ground` 或
> `low_collision` 时都拦（与内核 `ground_refusal` 同口径）；吸管是只读的，不拦。

> `Document::load` 走 `mir2_format::read_any_tolerant` 的**宽松口径** ⇒ 带未解尾块的真实地图
> 也能打开（不会再一律 422），未解字节数如实报在 `tolerant_trailer_bytes`。

---

### 3.3 `POST /api/map/open`

```json
{ "path": "maps/0.map" }
```

**200** —— 同 §3.2（`dirty` 必为 `false`，撤销栈清空）

| 错误 | 何时 |
|---|---|
| **400 `bad_request`** | `path` 缺失 / 空 |
| **403 `forbidden_path`** | 含 `..` 或指向 `MIREDIT_MAP_ROOT` 之外 |
| **404 `not_found`** | 文件不存在 / 不是普通文件 |
| **422 `parse_failed`** | 文件在，但九种格式**宽松判别**都不命中（带尾块的文件现在能开，不再走这条） |

---

### 3.3.1 `GET /api/maps`（地图列表，**只读元数据**）

```json
{"total": 61,
 "items": [{"name":"0.map","path":"0.map","bytes":5880052,"mtime":1793810990}]}
```

| 参数 | 取值 | 缺省 | 说明 |
|---|---|---|---|
| `offset` | `0..=4294967295` | 0 | 跳过多少条 |
| `limit` | `0..=500` | **100** | 每页条数。**超 500 ⇒ 400**（不静默截断）；`0` 合法（只要 `total`） |

| 字段 | 含义 |
|---|---|
| `total` | **总数**（不是本页条数）——前端靠它显示「共 N 张」并决定翻页 |
| `items[].name` | 文件名（含 `.map` 后缀，不含目录） |
| `items[].path` | **相对 `MIREDIT_MAP_ROOT`** 的路径（`/` 分隔）—— **可直接喂 `POST /api/map/open`**（与 §3.3 同一套白名单四道闸） |
| `items[].bytes` | 文件字节数（**只 stat，不读内容**） |
| `items[].mtime` | 修改时间，**Unix 秒**；取不到 ⇒ `null`（不写 0 冒充 1970） |

**扫描口径（硬约束）**

| 项 | 取值 | 理由 |
|---|---|---|
| 只收 `*.map` | 扩展名**大小写不敏感**（`A.MAP` 也算） | Windows 上两种写法都可能出现 |
| 递归深度 | **上限 3 层子目录**（`map_root` 自身算第 0 层） | 常见目录是平的，但换版本可能出现分组目录；不限深度会失控 |
| 符号链接 / 目录联接 | **一律不跟** | 防环，也防白名单根之外的东西混进列表 |
| 只读元数据 | `read_dir` + `file_type` + `metadata`，**零 `read` / 零 `open` / 零解析** | 上千张 / 数百 MB 逐张解析会把服务卡死；列表只做 stat，与解析一张的耗时差三个数量级 |
| 排序 | **文件名自然序**（数字段按数值：`n2.map` 在 `n10.map` 前），同名再按 `path` 字节序兜底 | 结果对同一份目录**恒定**，前端本地过滤与断言才有意义 |
| 条目上限 | 50,000 张；超了 ⇒ **503** | 防病态目录吃干内存，**不返回半截列表冒充完整** |

| 错误 | 何时 |
|---|---|
| **400 `bad_request`** | `limit` > 500、`offset`/`limit` 不是非负整数 |
| **503 `assets_unavailable`** | 根不存在 / 不是目录 / 子目录或条目元数据读不了 —— **绝不返回 200 空列表**（那会让人以为「这个根下真的没有图」） |

**前端怎么用**（`js/maplist.js`）：按 `limit=500` 翻页把 `total` 全拉完（`ceil(total/500)` 次请求），
之后**搜索框只在本地过滤、一次请求都不发**；一次最多渲染 300 行，超出时提示「输入关键词缩小范围」。

---

### 3.4 `GET /api/tiles`（**返回 PNG**）

| 参数 | 取值 | 缺省 | 说明 |
|---|---|---|---|
| `x` / `y` | `0..=2147483647` | 0 | 左上角世界格 |
| `w` / `h` | `1..=64` | 16 | 宽/高（格）。前端固定 16 |
| `scale` | `0.25` / `0.5` / `1` / `2`（也收 `quarter` / `half` / `one` / `two`，**大小写不敏感**） | 1 | 别的值 ⇒ **400**（不四舍五入、不静默降级成 1×） |
| `layers` | 名字列表 `ground,middle,front` **或** 位掩码 `0..=7` | 全开 | `1`=地表 `2`=中层 `4`=物体 |
| `collision` | 名字列表 `low,high` **或** 位掩码 `0..=3` | `0` | `1`=低位 `2`=高位 |
| `doors` | `0` / `1` | `0` | **开启门**：`1` = 门格前景帧号加 `door_offset`（门全开帧）；`0` = 闭门帧，**与改动前逐字节相同**。别的值（含空值 `doors=`、`2`、`true`、`01`）⇒ **400**，不静默当 0 |
| `lighting` | `day` / `dawn_dusk` / `night`（**大小写不敏感**） | `day` | **地图光照**：`day` 不画遮罩、**与改动前逐字节相同**；`dawn_dusk` 遮罩 alpha 205；`night` 遮罩 alpha 255。别的值（含空值、`xyz`、`dawn-dusk`）⇒ **400 + 中文原因**，不静默当白天 |

```
GET /api/tiles?x=0&y=0&w=16&h=16&scale=0.25&layers=ground,middle,front&collision=none&doors=0&lighting=day
```

**200** `image/png`，像素尺寸 **严格等于** `round(w×48×s) × round(h×32×s)`。
越界部分透明；缺帧不画（不画占位块）。响应头三条：
**`X-Render-Scale`** = 实际出图档位（取自 `Rendered.scale`，不是回显请求参数）；
**`X-Render-Doors`** = 本次门口径 `0|1`；
**`X-Render-Lighting`** = 本次光照档位 `day|dawn_dusk|night`（后两条取自真正传给渲染库的
`RenderOptions`，非法值在进渲染前已 400，故回显恒等于生效值）。
三条都列在 `server::EXPOSED_HEADERS`（`x-render-scale, x-render-doors, x-render-lighting`）里，
**跨源时页面 JS 才读得到**。光照状态栏只认 `X-Render-Lighting` 真值，读不到显示「未知（响应头不可读）」，
绝不用请求值冒充。

四档输出尺寸（同一块 `16×16` 格 / `64×48` 格）：

| `scale` | `16×16` 格 | `64×48` 格 | 说明 |
|---|---|---|---|
| `0.25` | 192×128 | 768×384 | 面积加权 `Box` |
| `0.5` | 384×256 | 1536×768 | 面积加权 `Box` |
| `1` | 768×512 | 3072×1536 | **与此前实现逐字节相同** |
| `2` | 1536×1024 | 6144×3072 ⇒ **413** | 最近邻；`64×48@2` 是 72 MiB > 64 MiB 上限 |

| 错误 | 何时 |
|---|---|
| **400 `bad_request`** | `w/h` 越界、`scale` 不在四档内、图层名不认识、`doors` 不是 `0`/`1`、`lighting` 不是三档之一（含空值） |
| **409 `no_map`** | 尚未打开地图 |
| **413 `too_large`** | 出图超限：目标图 > **64 MiB**（`RenderError::TooLarge`）或 1× 中间图 > **128 MiB**（`IntermediateTooLarge`）。**文案含 `want`/`limit` 数字 + 「缩小视口格数或降低倍率」**，且**不硬渲、不降级成 1×** |
| **503 `assets_unavailable`** | 图库不可用（没扫到 Tiles/SmTiles/Objects） |

> **缩小档由服务端出图**（2026-11-05 起）：前端按缩放档请求 `scale=0.25 / 0.5 / 1`，
> 画布对 `scale<1` 的瓦片 **1:1 贴图**（不再 `drawImage` 缩）。理由：25% 档一块 `16×16` 格
> 的 1× PNG 是 768×512（整屏 `64×48` 达 **4.5 MB**），而 `scale=0.25` 只要 **~12 KB**
> —— 体积差一个数量级以上，且 Box 面积加权比浏览器缩放的画质更准。
> **200% 仍请求 `1`**：`2×` 在 `64×48` 会 413，放大由画布最近邻完成（像素画放大不插值）。
> **缓存键 = `<mapToken>|<x,y,w,h,scale,layers,collision,doors,lighting>`**（`js/tiles.js` 的 `keyOf`）：
> 含 `scale` ⇒ 切档会重新取图，切回原档命中缓存（0 新请求）；含 `mapToken` ⇒ **换图后上一张图的
> 瓦片不可能按同一个键落进新图**（`clear()` 自增它，同时在途请求的票一并作废）；
> 含 `doors` / `lighting` ⇒ 勾门、切光照各走一批键（漏任一维就是「切了没反应」，而地图字节全对、极难查）。

---

### 3.5 `POST /api/edit`

请求体固定一层 `cmd`（`miredit_core::Command` 的 **JSON 直译**）：

```json
{ "cmd": { "Paint": { "x": 12, "y": 34 } } }
{ "cmd": "Undo" }
```

**200**
```json
{ "changed": 9, "dirty": true, "note": "落笔 9 格（地表图层）",
  "can_undo": true, "can_redo": false }
```

| 字段 | 说明 |
|---|---|
| `changed` | 真正改了几格（`0` = 没改，原因在 `note`） |
| `dirty` | 操作后是否有未保存修改 |
| `note` | 中文回执/拒绝原因；`null` = 无需说明 |
| `can_undo` / `can_redo` | **按钮禁用态用这两个字段** |

#### `cmd` 变体（**十九条内核命令** + 一条网页侧命令）

| 形态 | 语义 | 典型 `changed` |
|---|---|---|
| `{"SetLayer":{"layer":"middle"}}` | 切当前编辑层；只认 `ground`/`middle`/`front`/`low_collision`/`high_collision` | 0 |
| `{"SetBrushSize":{"size":5}}` | 笔刷边长（夹进 `3..=31` 的奇数） | 0 |
| `{"SetTool":{"tool":"eraser"}}` | **换工具**；`tool` 只认 `brush`/`bucket`/`eraser`/`pickup`（严格小写，**不收别名**） | 0 |
| `{"Paint":{"x":12,"y":34}}` | 按当前笔刷尺寸落一笔（默认 3×3 ⇒ 最多 9 格） | 0..N |
| `{"Stroke":{"from":[10,10],"to":[20,20]}}` | Bresenham 拖动，整条轨迹 = 一步撤销 | 0..N |
| `{"Erase":{"x":12,"y":34}}` | 按当前笔刷尺寸擦一笔（图号层清图号、保留 `bit15`）；**执行后工具留在橡皮** | 0..N |
| `{"Fill":{"x":12,"y":34}}` | BFS 连通填充（不支持物体层） | 0..N |
| `{"Pick":{"x":12,"y":34}}` | 吸管：把该格**当前层**取成笔刷（不改地图） | 0 |
| `{"SetSelection":{"x0":1,"y0":2,"x1":5,"y1":6}}` | 矩形选区（闭区间）；纯 UI 状态，不改图不置脏 | 0 |
| `"ClearSelection"` / `"CopySelection"` / `"CutSelection"` | 取消选区 / 复制 / 剪切（`Copy` 不改图；`Cut` 一步撤销） | 0 / 0 / 0..N |
| `{"PasteSelection":{"x":1,"y":2}}` | 以 `(x,y)` 为左上角粘贴（越界逐格跳过） | 0..N |
| `"DeleteSelection"` / `"FillSelection"` | 清空选区 / 用当前笔刷填满选区（各 = 一步撤销） | 0..N |
| `{"PlaceStamp":{"x":1,"y":2,"stamp":"mir/mir_00000"}}` | 放印章；后端先按 id 读 `.X` 注册格表再逐格贴（整次 = 一步撤销） | 0..N |
| `"Undo"` / `"Redo"` | 撤销/重做**一步**（也接受 `{"Undo":null}`） | 0..N |
| `"Save"` | 等价于 `/api/save` | 0 |
| `{"SetBrushFromPalette":{"family":"object","lib":0,"frame":3}}` | **网页侧命令**（不是 `Command` 变体）；`frame` 是 **0 基**帧索引（图号 = frame+1） | 0 |

> `Paint`/`Erase`/`Fill`/`Pick`/`PlaceStamp` 只吃坐标，**没有** `value` 字段——笔刷内容由
> `Pick` 或 `SetBrushFromPalette` 决定，不是每笔带一个值。

> **`SetTool` 必须显式发**（含"切回画笔"）：`Erase` 执行后内核工具**停在橡皮**
> （与 GUI 的 `--edit-erase` 一致），而 `Paint` 是**按当前工具分派**的 ⇒ 不切回去，
> 擦完再落笔会**继续擦**。前端 `ME.TOOLS[*].toolKey` 已把 `fill`/`pick` 翻成
> `bucket`/`pickup`，**线上词汇表只有一套**。

> `{"SetTool":{"tool":"laser"}}` 这类**非法键**不是 400：内核回 `200 + changed=0 + 中文原因`
> 且不改会话状态（"没认出来"不是"这次调用没完成"）。`PlaceStamp` 的 `stamp` 不存在则是
> **404**（见 §3.11）。

> **`touched_cell()`（内部，仅影响 WS `rerender` 区域）**：`Paint`/`Erase`/`Fill`/`Pick`/
> `PlaceStamp`/`PasteSelection` 报坐标，`Stroke` 报终点；`SetSelection`（矩形装不下一个点）、
> `Clear`/`Copy`/`Cut`/`Delete`/`Fill`（作用在"当前选区"、命令里没有坐标）、
> `SetLayer`/`SetBrushSize`/`SetTool`/`Undo`/`Redo`/`Save` 一律 `None`（= 整屏）。
> 2026-11-05 补齐 `Erase`（此前落进 `_ => None`，改格却只发整屏提示），并把兜底改成**逐条显式**。

| 错误 | 何时 |
|---|---|
| **400 `bad_request`** | `cmd` 结构不对 / 未知变体 / 字段越界 / 层名不认识 |
| **409 `no_map`** | 尚未打开地图 |

---

### 3.6 `POST /api/save`

无请求体（或 `{}`）。原子保存：`tmp → write → fsync → 读回核对 → rename`。

**200**
```json
{ "saved": true, "bytes": 5880052, "note": "[保存] 已写入 …" }
```
> `bytes` 是**保存后磁盘上的真实长度**。没有未保存修改时一个字节都不写（`note` 会说明）。
> **前端必须校验回执**（`app.js` 的 `saveMap`）：`saved === true` 且 `bytes` 是数字才算成功，
> 否则显示「保存未确认 · <原因>」并**不清脏标记** —— 只看 HTTP 200 会被「代理回 200 空体」
> 或将来新增的 `saved:false` 分支骗过去（旧实现就报过「已保存 undefined 字节」）。

| 错误 | 何时 |
|---|---|
| **409 `no_map`** | 尚未打开地图 |
| **500 `io_error`** | 写盘/读回/替换失败（`message` 带阶段名） |

---

### 3.7 `GET /api/palette`

| 参数 | 说明 |
|---|---|
| `kind` | `ground` / `middle` / `object`（`front`、`surface` 是 `object` 的别名），缺省 `ground` |
| `page` | 0 起，缺省每页 200 条（`size` 1..1000） |

**200**
```json
{ "items": [ { "lib": 0, "name": "Tiles", "frames": 31775 } ], "total": 22 }
```

| 字段 | 说明 |
|---|---|
| `lib` | 族内库号（= 地图格里的库号字节） |
| `name` | 磁盘上的库名（`Tiles` / `tiles3` / `Objects21` …） |
| `frames` | 索引表长度；**库打不开 ⇒ `null`**（不猜 0） |

> **item 是「库」不是「图块」**。后端**没有**按库取帧的端点 ⇒ 资源面板只列库，
> 点开写明「图块级待接入」，**不画假图块、不猜图号**。

| 错误 | 何时 |
|---|---|
| **400 `bad_request`** | `kind` 不认识 / `size` 越界 |
| **503 `assets_unavailable`** | 图库未就绪 |

---

### 3.8 `GET /api/stamps`

| 参数 | 说明 |
|---|---|
| `kind` | 分类（英文，精确、大小写不敏感），空 = 全部 |
| `q` | 关键词（只匹配 `id` / `name` 子串），空 = 不过滤 |
| `page` | 0 起，缺省每页 100 条（`size` 1..200） |

**200**
```json
{ "items": [ { "id": "mir/mir_00000", "name": "Market stall - mir_00000",
               "kind": "Building", "kind_zh": "建筑",
               "cells": 25, "bounds": [-1,-2,4,3] } ], "total": 25 }
```

| 字段 | 说明 |
|---|---|
| `cells` | `.X` 解析出的格数；读盘失败 ⇒ `null` |
| `bounds` | `[min_x,min_y,max_x,max_y]` 闭区间；同上 |

> **没有 `preview` 字段** ⇒ 印章列表只用「文字 + 格数 + 范围」展示，**不画缩略图**。
> 放置印章走 §3.5 的 `PlaceStamp`（后端先读 `.X` 注册格表，再逐格贴），失败口径见 §3.11。

---

### 3.9 `WS /api/events`（服务端单向推送）

```json
{ "type": "dirty",    "payload": { "dirty": true } }
{ "type": "rerender", "payload": { "x": 0, "y": 0, "w": 1, "h": 1 } }
{ "type": "error",    "payload": { "code": "io_error", "message": "保存失败：磁盘只读" } }
```

| `type` | 前端行为 |
|---|---|
| `dirty` | 更新顶栏星号 + 状态栏 |
| `rerender` | 丢弃相交区域的瓦片缓存并重绘（`w/h = 0` 表示整屏） |
| `error` | 右下角轻提示 5 秒 |

> 断线**不自动重连**，顶部显示「事件通道断开（不影响编辑）」——
> 编辑仍走 HTTP `/api/edit`，只是收不到推送。
> **连接还在、但协议对不上**是另一回事，同样必须报警：JSON 解析不了 / 缺 `type` 的帧会
> 累计 `state.ws.badFrames`；**连续 3 条**未知 `type` 会累计 `state.ws.unknownStreak`；
> 任一触发即显示「事件通道异常」横幅（状态栏同步），文案里带条数与最近一条 `type`/原文。
> 旧实现是 `catch { return; }` + 未知类型只写调试日志 ⇒ 后端改协议时前端完全静默。

---

### 3.10 错误码总表

| `code` | HTTP | 含义 |
|---|---|---|
| `bad_request` | 400 | 参数缺失/越界/枚举不认识（含 `cmd` 结构不对） |
| `forbidden_path` | 403 | 路径逃出 `MIREDIT_MAP_ROOT`（含 `..`） |
| `not_found` | 404 | 文件 / 资源 / 端点不存在 |
| `no_map` | 409 | 尚未打开地图 |
| `parse_failed` | 422 | 文件在，但九种格式宽松判别都不命中；印章 `.X` 解析失败；印章注册被拒（空印章 / 超单枚上限 / 注册表满） |
| `io_error` | 500 | 读写磁盘失败 |
| `assets_unavailable` | 503 | 图库 / 印章目录 / **地图列表（`MIREDIT_MAP_ROOT` 不可用）** |
| `internal` | 500 | 其它内部错误 |

前端对未列出的 `code` 一律展示后端原文 `message`，不猜语义。

### 3.11 `PlaceStamp` 的失败口径（分工：**web 读盘，core 零 IO**）

`{"PlaceStamp":{"x","y","stamp":"<id>"}}` 进来后，后端先按 id 把印章格表**读盘并注册**，
再调内核逐格贴。四类失败各自不同码：

| 情况 | 状态码 | 说明 |
|---|---|---|
| 印章目录不可用（`palette.json` 读不了） | **503** `assets_unavailable` | 资源没就绪，不是调用方的错 |
| id 不在目录里 | **404** `not_found` | 提示用 `GET /api/stamps?q=…` 检索 |
| `.X` 读不了 / 解析失败 | **500** `io_error` / **422** `parse_failed` | 目录说它在、但文件坏了 |
| 印章注册被拒（0 格 / 超 1,000,000 格 / 注册表满） | **422** `parse_failed` | 原因用内核的中文串原文 |
| 放成了但整枚越界 | **200** + `changed=0` + 中文原因 | 内核语义拒绝，不是错误 |

> 同一会话里同一 id 只读一次盘（已注册就直接放）。

---

## 4. 前端行为约定

| 常量 | 值 | 位置 |
|---|---|---|
| 格尺寸 | `48 × 32` 像素（scale=1） | `js/config.js` `ME.CELL` |
| 一次请求的块 | `16 × 16` 世界格 | `ME.CHUNK` |
| 缩放档 | `0.25 / 0.5 / 1 / 2` | `ME.ZOOMS` |
| 取图 `scale` | **随档位算**：25% ⇒ `0.25`、50% ⇒ `0.5`、100% ⇒ `1`、**200% ⇒ `1`**（2× 会 413） | `ME.tileScaleFor(zoom)` |

**工具 → 命令**

| 工具 | 切工具时先发 | 落笔发 | 说明 |
|---|---|---|---|
| 笔刷 `B` | `{"SetTool":{"tool":"brush"}}` | `{"Paint":{"x","y"}}` | 拖动时**按格发**，同一条命令在队列里去重 |
| 填充 `F` | `{"SetTool":{"tool":"bucket"}}` | `{"Fill":{"x","y"}}` | 只在按下那一下发 |
| 吸管 `I` | `{"SetTool":{"tool":"pickup"}}` | `{"Pick":{"x","y"}}` | 回执 `note` 里的图号显示在左栏「笔刷」与状态栏 |
| 橡皮擦 `E` | `{"SetTool":{"tool":"eraser"}}` | `{"Erase":{"x","y"}}` | 拖动时按格发；**切回画笔也必须发 `SetTool`**（否则继续擦） |
| **选择 `S`** | **一条都不发**（`toolKey = null`） | 拖动松手发 `{"SetSelection":{"x0","y0","x1","y1"}}` | 选区**不是内核工具**：`SetTool` 只收 `brush/bucket/eraser/pickup` ⇒ 纯前端工具态 |

> **视觉态**：当前工具 = `aria-checked="true"`（CSS 高亮金边）+ 状态栏 `stTool` + 轻提示
> 「当前工具：橡皮擦（eraser）」（后端回执原文）。
> `ME.TOOLS[*].usable === false` 的工具才会置灰 + 「待接入」角标（当前五个全为 `usable`）。

### 4.1 印章放置（2026-11-05 接入）

| 步骤 | 交互 | 发给后端 |
|---|---|---|
| 进入待放置态 | 印章列表每行「放置」按钮 / 单击行体后按 `Enter` / 手机**长按该行**（550 ms） | 无（纯前端态） |
| 待放置态视觉 | 画布光标 `crosshair`（`#mapCanvas.is-stamping`）+ 顶部提示条<br>`点击画布放置：<印章名>，Esc 取消` + 该行高亮 `.is-armed` | 无 |
| 落位 | 点画布某格 | `{"PlaceStamp":{"x","y","stamp":"<id>"}}` |
| 成功 | 重取瓦片（整屏作废缓存）+ 脏标记亮 + toast 后端 `note` 原文；**仍留在待放置态**（可连放） | 200 |
| 失败 | 顶部**错误条**（`data-tone="error"`）显示后端原文：<br>404 `印章 id 不存在：…` / 409 `尚未打开地图` / 422 `.X` 坏了 / 503 目录不可用；<br>**200 + `changed=0`**（整枚越界 / 闸门）也按原文上条 | — |
| 取消 | `Esc`，或点错误条右侧 ×（若仍在待放置态会把提示条接回来） | 无 |

> 点在地图外 ⇒ 前端提示「点在地图外：请点地图范围内的格子（本次未发送）」，**不发请求**。
> 印章列表**没有缩略图**（`/api/stamps` 无 `preview` 字段）⇒ 只显示文字 + 格数 + 范围，不画假图。

### 4.2 矩形选区与剪贴板（2026-11-05 接入）

| 操作 | PC | 手机（底部抽屉里的按钮） | 命令 |
|---|---|---|---|
| 画选区 | 选「选择」工具后拖动（端点夹在图内） | 同左（抽屉里点「选择」） | `{"SetSelection":{…}}` |
| 复制 | `Ctrl+C` | 抽屉「复制」 | `"CopySelection"` |
| 剪切 | `Ctrl+X` | 抽屉「剪切」 | `"CutSelection"` |
| 粘贴 | `Ctrl+V` | 抽屉「粘贴」 | `{"PasteSelection":{"x","y"}}` |
| 清空选区内容 | `Delete` | 抽屉「删除」 | `"DeleteSelection"` |
| 用笔刷填满选区 | 抽屉「填充」 | 抽屉「填充」 | `"FillSelection"` |
| 取消选区 | `Esc` | 抽屉「取消选区」 | `"ClearSelection"` |

- **选区真值以后端回执为准**：`SetSelection` 会越界裁剪，`note` 形如
  `选区：3×3 格（(216,5)..(218,7)，闭区间）` ⇒ 前端解析出矩形再上屏；
  后端说「已取消选区」（整块在图外）就清空；解析不出来则保留本地矩形（不猜）。
- **画布可见**：金色虚线（`#a8842b`，`setLineDash([7,4])`）+ 半透明填充（`rgba(200,160,60,.22)`）；
  拖动中的预览更淡（`.14`）。
- **状态栏**：`选区 W×H` / `剪贴板 W×H`（剪贴板尺寸取自 `CopySelection`/`CutSelection` 回执的
  `W×H 格`），左栏脚注另显「粘贴锚点：指针所在格（当前 x, y）」。
- **粘贴锚点 = 当前指针（鼠标/触摸）所在格**：取 `ME.canvas.hover`，指针不在画布上时退回
  最近一次所在格；两者都没有就提示「把指针移到目标格再按 Ctrl+V」，**不发请求**。
- **触摸点击没有 `pointermove`** ⇒ `pointerdown` 里也会更新悬停格，否则手机上锚点会停在上一次位置。
- 换图/重开图 ⇒ 后端重建会话（选区与剪贴板随之清空），前端 `ME.selection.reset()` 同步清空本地值。

> **整块类命令要整屏作废瓦片**：WS `rerender` 只带**锚点一格**（`touched_cell` 的 `w/h` 恒为 1），
> 而 `Fill`/`PlaceStamp`/`PasteSelection`/`DeleteSelection`/`CutSelection`/`FillSelection` 一次改一片
> ⇒ `app.js` 的 `REGION_CMDS` 在 `edit:result` 里对这些命令做 `ME.tiles.invalidate(null)`（清空缓存重取）。
> ⚠ `Fill` 是 **BFS 连通填充**，一次可能改满整张图，**必须在表里**（漏了它 ⇒ 点一下填充，
> 除锚点那一块外全屏都是填充前的旧图）。

**告警条（黄条）**：`/api/map` 的 `edit_warning` 非空 ⇒ 常驻显示（不是 toast），
左侧加格式标记（`Old12 · 未解区语义未知`）。点 × 可收起，但**再打开地图会重新出现**
（它不是"一次性通知"，是"这张图的属性"）。

**地表闸门**：`ground_paint_supported === false` 且当前编辑层是 `ground`/`low_collision`
⇒ 地表行显示「不可落笔」，点画布**前端先拦**（不请求后端），提示带 `format_label` 原文。

**图层眼睛** → 重新拼 `/api/tiles` 的 `layers` / `collision` **名字列表**；
缓存键含这两个参数 ⇒ 关掉再打开是**零请求**（命中缓存）。

**「开启门」复选框**（图层组**组头**，`#doorsToggle`）→ 换 `/api/tiles` 的 `doors=0|1`，
**不发任何后端命令**（纯视图选项，不改地图数据）。缓存键含 `doors` ⇒ 勾上换一批键、重新取图，
取消勾选**命中旧块**（0 新请求，但画面确实变回闭门帧）。归一化只有 `ME.doorsParam` 一处真源
（`config.js`）：`1`/`'1'`/`true` ⇒ `'1'`，其余 ⇒ `'0'`，URL 与缓存键同源，不会两套口径。

**快捷键**：`Ctrl+Z` 撤销、`Ctrl+Y`（或 `Ctrl+Shift+Z`）重做、`Ctrl+S` 保存、
`B/F/E/I/S/H` 切工具、`Ctrl+C/X/V` 复制/剪切/粘贴选区、`Delete` 清空选区内容、
`Enter` 把选中的印章推进待放置态、`+/-` 缩放、`0` 适应地图、
`Esc`（优先级：撤待放置态 → 撤选区 → 关抽屉/浮层）。

**鼠标/触摸**：左键（单指）= 当前工具；中键 / `Alt`+左键 / **「平移」工具（`H`，单指拖动）** = 平移；
滚轮 / 双指捏合 = 切缩放档（双指**只**缩放，不负责平移）。
⚠ 触摸单指落笔**延迟 120 ms**（`touch.js` 的 `TOOL_DELAY_MS`）：第二根手指在窗口内落下 ⇒
转捏合、**一条命令都不发**；窗口内位移 >10 px ⇒ 判定为拖动，立刻补发；快速点按在抬手时补发。

### 4.3 「打开地图」对话框：列表为主，手打路径为退路（2026-11-05 接入）

| 步骤 | 交互 | 发给后端 |
|---|---|---|
| 打开对话框 | 顶栏「打开地图…」 | `GET /api/maps?offset=0&limit=500`，若 `total > 500` 继续翻页直到拉完（`ceil(total/500)` 次请求） |
| 列表项 | `文件名` + `大小 · 修改时间`（KB/MB 1024 进制；`mtime` 是 Unix 秒，转本地时间） | 无 |
| 搜索 | 输入框 ⇒ **纯客户端子串过滤**（大小写不敏感，空格分词取交集；同时匹配 `name` 与 `path`） | **无请求**（纯客户端过滤） |
| 点击一项 | 关对话框 → 走与手打路径**同一条**通路 | `POST /api/map/open {"path": <列表里的 path 原样>}` |
| 搜索框回车 | 打开**第一条**匹配项（无匹配 ⇒ 轻提示，不发空路径） | 同上 |
| 「刷新」 | 丢掉本地缓存重新拉全量 | `GET /api/maps` ×N |
| 手打路径（退路） | 展开 `<details>` 填相对路径 → 「按此路径打开」 | 同上（含 `..` / 根外 ⇒ 403 `forbidden_path`） |
| 失败 | 列表位置显示后端原文（如 503「地图列表不可用：地图根读不到…」），**不显示空列表冒充成功** | — |

> **为什么不在输入时请求后端**：`total` 可能上千，每敲一个字符发一次请求等于
> 每秒钟几十次全目录 stat；而且输入抖动会让结果乱序。拉全量后本地过滤是**零延迟**的。
> **为什么渲染上限 300 行**：上千个 `<button>` 在移动端会明显拖慢首次渲染；超出时计数行
> 明说「仅渲染前 300 条，输入关键词缩小范围」，不假装全都在。
> **触控目标**：桌面 44 px、手机 52 px（`css/maplist.css`；手机断点里 `#mapList` 是
> 底部抽屉内的滚动容器）。

---

## 5. 仍未接的缺口（前端已显式标注）

| 缺口 | 前端现状 | 依据 |
|---|---|---|
| 图块级列表（按库取帧/取图） | 资源库只列到库级；点开写「图块级待接入」 | `/api/palette` 只有 `{lib,name,frames}` |
| 印章缩略图 | 只显示文字 + 格数 + 范围 | `/api/stamps` 无 `preview` |
| 单元格图号 / 阻挡位查询 | 检查器写「待接入（后端无单元格查询端点）」，只显示真实像素色 | 无该端点 |

> ✅ 已收口（2026-11-05）：**橡皮擦**（`SetTool` + `Erase` 全通）、**多格式开图**
> （九种格式 + 宽松尾块，67 张以前 422 的图现在能开）、**印章放置**
> （行内「放置」/ Enter / 手机长按 ⇒ 点画布落位，失败原文上错误条）、
> **选区 / 剪贴板**（拖动框选 + 金色虚线可见 + Ctrl+C/X/V/Delete/Esc + 手机抽屉按钮 +
> 状态栏 `选区 W×H` / `剪贴板 W×H` + 粘贴锚点 = 指针所在格）。


---

## 6. 验证方式

- 后端：`cargo test -p miredit-web`（路径白名单、参数校验、分页、错误码、真 TCP 往返都有单测钉住）；`cargo clippy -p miredit-web --all-targets -- -D warnings`。
- 前端：起后端后浏览器打开 `http://<bind>/` 手工验收；无人值守可用无头 Chromium 经 CDP 派发真实鼠标 / 触摸 / 键盘事件。
- **本文件不锁定会过期的实测数字**（响应字节数、请求计数、画布哈希等），一律以实际运行结果为准。

### 6.1 浏览器支持

| 项 | 要求 / 状态 |
|---|---|
| 必需 Web 平台能力 | Canvas 2D、`PointerEvent`（鼠标 / 触摸 / 捏合）、`WebSocket`、`imageSmoothingEnabled`、CSS 自定义属性 |
| 已验收 | Chromium 内核（Edge）桌面 1440×900 与 375×812 移动视口 |
| 未验收 | Firefox / Safari —— 无专有 API 依赖，理论可用，但**未实测**，按「未验证」对待 |
| 触控目标 | 桌面 ≥44 px、手机 ≥52 px（44 px 是硬底线） |

---

## 7. 边缘口径（便于复查）

- **非法参数一律 400，不静默降级**：`scale=3` / `doors=2` / `lighting=xyz` / `limit=501` 都回 `bad_request` + 中文原因，不四舍五入、不当 0、不截断。
- **`?doors=`（空值）与「没给 `doors`」是两件事**：前者走校验 ⇒ 400，后者才是缺省 `0`。
- **`too_large` 不硬渲**：目标图 > 64 MiB 或 1× 中间图 > 128 MiB ⇒ 413，文案带 `want` / `limit` 数字与处置建议。
- **`IntermediateTooLarge` 在 HTTP 层打不到**：本端点 `w/h ≤ 64`，1× 中间图最多约 25 MB，够不到 128 MiB 阈值；该映射由单测直接构造 `RenderError` 覆盖。
- **跨源头**：`x-render-scale` / `x-render-doors` / `x-render-lighting` 列在 `server::EXPOSED_HEADERS` 里；无 `Origin` 时不发任何 CORS 头。
- **缺图/缺库如实报错**：未开图 409 `no_map`、图库不可用 503 `assets_unavailable`、`GET /` 找不到静态目录 404 —— 都不伪造数据。
