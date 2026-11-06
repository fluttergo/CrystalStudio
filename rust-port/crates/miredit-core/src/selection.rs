//! 矩形选区 + 剪贴板：**复制 / 剪切 / 粘贴 / 删除 / 填充选区**（9 个功能点共用一套选区结构）。
//!
//! 真值全部来自 CrystalStudio 的两个文件：
//! `CrystalStudio/CrystalStudio/MapViewport.cs`（选区本体 + 清空 + 填充选区）与
//! `CrystalStudio/CrystalStudio/RegionCopy.cs`（复制 / 剪切 / 粘贴）。
//!
//! # 一、选区的坐标系与边界语义（**逐条带行号**）
//!
//! | # | 事实 | 源码 | 本 crate 的落地 |
//! |---|---|---|---|
//! | 1 | 选区是**可空的** `Rect?`；`null` = 没有选区 | `MapViewport.cs:102`（`public Rect? Selection { get; private set; }`）、`:107`（`Selection = null`） | `Option<Selection>`；[`crate::EditSession::selection`] |
//! | 2 | 选区**只在拖拽时产生**：起点必须图内，终点**夹到图内** | `:390`（`if (!Document.Contains(x, y)) return false;`）、`:399`（`Math.Clamp(x, 0, Document.Width - 1)`） | [`Selection::clamp_to`] 复刻这条不变量 ⇒ 存下来的选区**永远整块在图内** |
//! | 3 | 存的是 `(左, 上, 宽, 高)`，且宽高 = 端点差 **+1** | `:400`（`new(Math.Min(ax,bx), Math.Min(ay,by), Math.Abs(bx-ax)+1, Math.Abs(by-ay)+1)`） | `x1 = x0 + 宽 - 1` ⇒ 格集合是**闭区间** |
//! | 4 | **`Right` / `Bottom` 不含端点**（半开） | `:115`（`x >= area.Left && x < area.Right && y >= area.Top && y < area.Bottom`）；`:540`/`:543`/`:546` 三个循环全是 `y < r.Bottom` / `x < r.Right` | 闭区间 `[x0..x1]` 与它**等价**：`x1 = Right - 1`；[`Selection::iter`] 用 `x0..=x1` |
//! | 5 | 选区**不进撤销栈**、**不置脏** | `:147`/`:152`（选区随 `MapEditorState` 一起存取，与 `History` **并列**而不是被它管） | 选区/剪贴板是 UI 状态：`SetSelection` 等命令 `changed = 0` 且不动 `dirty` |
//! | 6 | 取消选区 = 置 `null` | `:103-108`（`Deselect()`） | [`crate::EditSession::clear_selection`] |
//! | 7 | 选区工具是 `M` 键；**选区外按下 = 先取消选区** | `MainWindow.axaml.cs:861`（`Key.M => EditorTool.Select`）、`MapViewport.cs:389`（`if (!SelectionContains(x, y)) Deselect();`） | UI 层的事，本 crate 只提供 `set_selection` / `clear_selection` |
//!
//! ⚠ **与派单描述的一处措辞差异（以源码为准）**：派单写「闭区间；与 C# 的 `Rect` 语义对齐」。
//! 更精确的说法是：C# 的 `Rect` 是**半开**（`Right`/`Bottom` 不含端点，第 4 条），
//! 但它在**构造时**把「端点差 +1」写进宽高（第 3 条）⇒ 落到**格**上就是闭区间。
//! 本 crate 直接存闭区间 `{x0,y0,x1,y1}`，两种写法在**格集合**上完全等价，不存在语义差。
//!
//! # 二、清空选中格（删除 / 剪切）的按层语义
//!
//! 官方有**两份**清空实现，语义不同（这是 C# 的历史包袱，不是我们挑的）：
//!
//! | 函数 | 清哪几层 | 清库号字节 | 源码 |
//! |---|---|---|---|
//! | `ClearSelection`（`Delete` 键走它） | **只清当前层** | **不清** | `MapViewport.cs:546`（`case MapLayer.Ground: c.BackImage &= 0x60000000; break;` …）、`MainWindow.axaml.cs:873` |
//! | `CutSource`（剪切） | **三族一起清** | **清**（`BackLibrary = 0` / `MiddleLibrary = 0`） | `RegionCopy.cs:45-69`，`:54`、`:59`、`:63` |
//!
//! **本仓统一取 `CutSource` 口径**（删除与剪切行为一致）—— 2026-11-05 裁定：
//! `img == 0` 时库号字节没有任何意义（渲染只看 `img`），留着就是脏数据；
//! 两个命令语义相同（都是"清掉选中格的内容"）却行为不同，是历史包袱不是设计。
//! ⇒ **这是有意偏离 `MapViewport.cs:546`**（不清库号），已登记，不是漏改。
//!
//! ⚠ **一处例外（2026-11-05 新增）：可见性闸门**。`CutSource` 的三个 `if` 都带层开关
//! （`RegionCopy.cs:52` 的 `if (ground && …)`、`:57` 的 `if (middle && …)`、`:61` 的 `if (objects && …)`）
//! ⇒ **剪切只清可见层**（[`crate::EditSession::cut_selection`] 传当前可见性）；
//! 而删除键的 C# 对照是 `MapViewport.cs:546` 的 `ClearSelection`（不判可见性）
//! ⇒ [`crate::EditSession::delete_selection`] 传 `[true; 3]`，仍三族全清。
//!
//! 逐族落到本 crate 的 16 位字段（`bk_img`/`mid_img`/`fr_img` 的字段归属见 `layer.rs:240-246`）：
//!
//! ```text
//! 地表  if (可见[0] && (bk_img  & 0x7FFF) != 0) { bk_img  &= 0x8000; tiles    = 0; }   // RegionCopy.cs:52-56
//! 中层  if (可见[1] && (mid_img & 0x7FFF) != 0) { mid_img  = 0;      sm_tiles = 0; }   // RegionCopy.cs:57-60
//! 物体  if (可见[2] && (fr_img  & 0x7FFF) != 0) { fr_img  &= 0x8000; area     = 0; }   // RegionCopy.cs:61-64
//! ```
//!
//! 细节逐条给理由（第 1~3 条是原判据；第 4~6 条是 **2026-11-06 复核订正**新增）：
//!
//! 1. **判据掩 `0x7FFF` 而不是整 16 位**：C# 的判据是 `(s.Cell.BackImage & 0x1FFFFFFF) != 0`
//!    （`RegionCopy.cs:52`），它把 `0x60000000` 的两个阻挡位掩掉了 ⇒
//!    **「被挡住但没图号」的格不算有内容**，不该被清。本 crate 的阻挡位是 `bk_img`/`fr_img` 的
//!    `bit15`（等价推导见 `paint.rs:275-283`：C# `0x60000000` ↔ 文件级 `bit15`）
//!    ⇒ 等价判据是 `& 0x7FFF`。
//! 2. **保留 `bit15`**：`bk_img &= 0x8000` / `fr_img &= 0x8000` 就是 C# 的 `& 0x60000000`
//!    （`MapViewport.cs:546` 的地表分支、`RegionCopy.cs:54`）—— 清内容不清阻挡位。
//!    中层没有阻挡位（`layer.rs:257-265`）⇒ 直接置 0。
//! 3. **不动 `ani_frame` / `ani_tick` / `door_index` / `door_offset` / `light` / `tail`**：
//!    - `RegionCopy.cs:59` 确实清了 `MiddleFrames`/`MiddleTick`，但那两个字节在 **14 B 档**里
//!      CrystalStudio 读进了 `FrontAnimationFrame`/`FrontAnimationTick`
//!      （`LegacyMapReader.cs:243-244` 读、`:257-258` 只在有前景时赋值），
//!      `MiddleAnimationFrame` 恒 0（`Document.cs:19-20` 定义、`:63` 从 `s.MiddleAnimationFrame` 抄）
//!      ⇒ **C# 在 14 B 图上没有可清的中层动画**；本 crate 的 `ani_frame`/`ani_tick` 就是那两个字节
//!      （`mir2-map/src/cell.rs:32-33` 的 14 B 布局偏移 8/9、`:232-233` 的解析），照 C# 的实际行为**不清**。
//!    - `RegionCopy.cs:55` 清了地表的 `TileAnimation`/`TileOffset`/`TileFrames`，那是 **36 B 档**的扩展字节
//!      （`LegacyMapReader.cs:300-303`），在本 crate 里落在**不透明的 22 字节尾区** `Cell::tail`
//!      （`convert.rs` 的 `FIELD_MAP` 第 12 条）⇒ 按猜测清尾区会写坏未知字节，故**不清**。
//! 4. ⚠ **订正（2026-11-06，I F1 复核）：视图 `ani_frame`/`ani_tick` 是「前景」动画，不是中层动画。**
//!    第 3 条的**结论不变**（清中层时不动这两个字节），但它只对 14 B 档给了理由，且**行号已过期**；
//!    另有一条「视图 `ani_frame`/`ani_tick` 就是中层动画字节、清中层时该清 0」的说法在本仓流传过 ——
//!    **该说法错**，逐条证否（三源：上游原文 / 本 crate 字段表 / 真实语料）：
//!    - **源头**：`LegacyMapReader.cs:418`/`:486`/`:641` 读的**确实是** `MiddleAnimationFrame`，但它们
//!      分别在 `LoadMapType5`（`Mir3Wemade`）、`LoadMapType6`（`Mir3Shanda`）、`LoadMapType100`
//!      （`CrystalCs`）三支里，且读的是**格内其它偏移**（`mir3_wemade.rs:260-261` 的 `c[1]`/`c[2]`、
//!      `mir3_shanda.rs:149-150` 的 `c[10]`/`c[11]`）——**不是 14 B 档的 off 8/9**。
//!    - **反面**：全文件给 `MiddleAnimation*` 赋值的只有 6 处（`:418`/`:422`、`:486`/`:491`、`:641`/`:642`）
//!      ⇒ `LoadMapType0`/`1`/`2`/`3`/`4`/`7`（`Old12`/`Wemade2010`/`Shanda14`/`Shanda2012`/`AntiHack`/
//!      `Heroes34`）里这两个字段**恒 0**，C# 清它们等于「清 0 到 0」。
//!    - **本 crate 的归属**：视图 `ani_frame`/`ani_tick` ↔ `mir2-format::Cell.ani_frame/ani_tick`
//!      （`convert.rs` 的 `FIELD_MAP` 第 6/7 条）= 磁盘 off 8/9（`mir2-map/src/cell.rs:309-311`、
//!      `:352-353`）= C# 的 `franimationframe`/`franimationtick`（`LegacyMapReader.cs:245-246` 读、
//!      `:259-260` 只在有前景时赋给 `Foreground`）⇒ **前景动画**；`stamp.rs:99-100` 是同一口径
//!      （`.X` 的 `foreground.frames/tick` → `StampCellInput.ani_frame`，与本字段同名不同义）。
//!      上游 `MapViewport.cs:592` 的中层分支**一个字节都不碰** `Foreground`，只清
//!      `MiddleFrames`/`MiddleTick`。
//!    - **语料佐证**（2026-11-06 实测 `E:\GodM2\GodotClient\maps` 的 53 张 14 B 图 / 2,219,118 格）：
//!      `ani_*` 非 0 的 997 格里 **978 格（98.1%）同时有前景**；「前景为 0、`mid_img != 0`、
//!      且 `ani_* != 0`」的只有 7 格（0.0003%）——这是随机杂字节量级，不是「中层动画」。
//!    - **结论**：清中层时清 `ani_frame`/`ani_tick` = **抹掉前景动画**，与上游只清
//!      `MiddleFrames`/`MiddleTick` 的行为相反，故**不清**。
//!    - **行号订正**（文件增长漂移，本单实测两份上游副本）：`LegacyMapReader.cs:243-244` → `:245-246`、
//!      `:257-258` → `:259-260`；`Document.cs:63` → `:71`；`mir2-map/src/cell.rs:32-33` → `:309-311`
//!      （字段）／`:352-353`（解析）；本模块与 `paint.rs` 多处引的 `MapViewport.cs:546`（地表分支，
//!      实为 `ClearSelection` 而不是 `BucketFill`）→ **`:592`**（598 行取证副本）／`:597`
//!      （`_upstream` 的 603 行副本，审计单用的就是这套新编号）。
//! 5. **已知缺口（未修，单列）**：`Mir3Wemade`/`Mir3Shanda`/`CrystalCs` 的中层动画在 C# 里**有真值**，
//!    但落在 `mir2-format::Cell.mid_ani_frame`/`mid_ani_tick`，**编辑视图不承载**
//!    （`convert.rs:261-262` 的 `LOST_IN_VIEW` 第 6/7 条）⇒ 清中层时**清不到它**。要清得先给视图加
//!    这两个字段（`mir2-map` 的 `Cell` + `convert.rs` 的 `FIELD_MAP`），属**跨 crate 变更**。
//!    对唯二 `editable` 的 `Shanda14`/`Old12` **没有可观测差异**（上游那边恒 0）。
//! 6. **`ClearSelection` 的「无条件清」 vs `CutSource` 的「有内容才清」**（2026-11-06 补记）：
//!    - 上游 `MapViewport.cs:592` 的中层分支是 `c.MiddleImage = 0; c.MiddleFrames = 0;
//!      c.MiddleTick = 0;`，**不看** `MiddleImage != 0`，而且逐格 `History.Record`
//!      ⇒ **空选区删一次也留下一步「什么都没变」的撤销**；
//!    - `RegionCopy.cs:57` 则是 `if (middle && s.Cell.MiddleImage != 0)`（**有内容才清**）。
//!    - 本 crate 两条路径共用 [`clear_plan`]，统一取 `CutSource` 的**条件**口径
//!      （`after == before` 就跳过）⇒ 「删除空选区」在这里 `changes` 为空：不记撤销步、回执 0 格。
//!      这是**有意偏离**（保「值没变不记步」的全局口径，`edit.rs:903-905`），不是漏改；
//!      也没有为此新增公共 API（`vis` 一个参数已经区分了层，两种口径的差别只剩「空选区要不要记步」）。
//!
//! # 三、剪贴板
//!
//! | # | 事实 | 源码 |
//! |---|---|---|
//! | 1 | **整块矩形都抓**（空格也抓）；另有一份"非空格"清单**只给预览用** | `RegionCopy.cs:21-27`（`cells = new Cell[w*h]` 逐格全填；`previewCells` 才过滤） |
//! | 2 | 抓的是**整格**：三族图号 + 三个库号 **+ 门索引/门偏移/光照/前景动画帧+节拍**（`.Copy()` 复制全部字段） | `RegionCopy.cs:24`（`document.Get(x+dx, y+dy).Copy()`）+ `Document.cs:9-14`（`BackLibrary/BackImage`、`MiddleLibrary/MiddleImage`、`Foreground`）+ `:16-20`（`Door`/`DoorOffset`/`Light`/动画） |
//! | 3 | 抓取时**整块必须在图内**，否则抛异常 | `RegionCopy.cs:17`（`if (… !document.Contains(x + width - 1, y + height - 1)) throw`）⇒ 本 crate 的选区恒在图内（§一 第 2 条），这条**不可达** |
//! | 4 | 粘贴**逐族判断**：某族图号为 0 ⇒ 该族不写（不把目标格的该族清成 0） | `RegionCopy.cs:78-82`（`pasteGround = ground && (source.BackImage & 0x1FFFFFFF) != 0` … `if (!pasteGround && !pasteMiddle && !pasteObjects && !pasteCollision) continue;`） |
//! | 5 | 粘贴**保留目标格自己的阻挡位** | `RegionCopy.cs:84`（`target.BackImage = (target.BackImage & 0x60000000) \| (source.BackImage & ~0x60000000)`） |
//! | 6 | 地表粘贴会**按源的奇偶重算锚点** | `RegionCopy.cs:70`（`Anchor`：`x - ((x - sourceX) & 1)`）⇒ 本 crate **2026-11-06 已逐字对齐**（[`Clipboard::anchor`]，见本节末「差异 B 已撤销」） |
//! | 7 | 粘贴**整块必须图内**，否则整次失败 | `RegionCopy.cs:71`（`CanPaste`）、`:74`（`if (!CanPaste(document, x, y)) return false;`）、`:120`（`"Region extends outside the map"`） |
//! | 8 | 复制**按图层可见性过滤**：隐藏层的族不抓（字段填 0）；三层全关 ⇒ 拒绝 | `RegionCopy.cs:106`（`if (!VisibleLayers.Any(v => v) && !CollisionVisible) { …"Show at least one layer to copy"… return; }`）、`:107`（`VisibleLayers[0..2]` 传进快照） |
//! | 9 | 粘贴**不看当前可见性**，只用复制时定格的层选择 | `RegionCopy.cs:3`（`A detached snapshot: layer choices are captured when copying, not when pasting.`）、`:78-82`（`pasteGround = ground && …`） |
//!
//! **与官方的三处有意差异**（都已登记，不是悄悄改）：
//!
//! ⚠ **2026-11-06 订正：原「差异 B（不按源奇偶重算锚点）」已撤销**，改为**逐字对齐**上游
//! `RegionCopy.cs:70` 的 `Anchor`（[`Clipboard::anchor`]）：
//!
//! ```text
//! Anchor(x, y) = ground ? (x - ((x - sourceX) & 1), y - ((y - sourceY) & 1)) : (x, y)
//!   sourceX/sourceY = 复制时选区左上角（`RegionCopy.cs:18`）
//!   ground          = 复制时**地表层是否可见**（`RegionCopy.cs:107` 传给构造函数、`:19` 存下），
//!                     不是"抓到的内容里有没有地表"
//! ```
//!
//! 上游 `PlaceRegion` 是**先** `Anchor(hover.X, hover.Y)` **再** `CanPaste`（`RegionCopy.cs:121-122`）
//! ⇒ 重算出来的锚点若为负，`CanPaste` 的 `document.Contains(x, y)` 直接 false ⇒ **整块拒绝**
//! （只弹 "Region extends outside the map"，一格不写）。本 crate 等价地
//! [`crate::PlaceOutcome::Refused`]（见 [`crate::EditSession::paste_clipboard`]）。
//!
//! 为什么之前那条理由（"本 crate 地表逐格落笔、没有 2×2 网格概念"）不成立：粘贴块的**对齐**
//! 是上游的**用户可见行为**（同一块地皮再贴一次必须落在原格上，否则同名图块会错位一格），
//! 与渲染网格、落笔粒度是两件事；渲染层"只铺偶偶格"（`miredit-render/src/compose.rs`）
//! 恰恰说明偶数对齐才是能看见的位置。
//!
//! 剩下的三处差异：
//!
//! - **差异 A（对应第 7 条）**：官方粘贴是**整块拒绝**，本 crate 按任务书 §三.3 要求
//!   **逐格跳过**并在 [`crate::PlaceOutcome::Placed`] 的 `skipped` 里如实报数。
//!   依据是官方自己就有一条逐格判据（`PlacedStamps.cs:80` 的 `MapBoundary + continue`），
//!   与 `stamp.rs` 模块文档 §二 同一条理由、同一份实现。
//! - **差异 B（2026-11-05 订正；原「差异 C」）**：官方 `RegionCopy` 抓的是**整格**（`RegionCopy.cs:24` 的
//!   `document.Get(x+dx, y+dy).Copy()`），本 crate 现在也**整格抓** —— [`Clipboard::capture`]
//!   把 `StampCellInput` 的 10 个附加字段一并填上：门索引/门偏移/光照/前景动画帧+节拍在
//!   12/14 B 布局里**有字节**（`mir2-map/src/cell.rs:184-195` 的 off6/7/11/8/9）⇒ 照抄；
//!   中层动画帧+节拍、地表动画图号/偏移/帧数这 5 个**没有字节**（36 B 档的扩展字段落在
//!   不透明的 `Cell::tail` 里）⇒ 留 0 并由 `stamp::记丢动画` 如实说明。
//!
//!   **粘贴**按 `RegionCopy.cs:84-86` 搬门/门偏移 + 前景动画（有前景就**无条件赋值**），
//!   但**光照不搬** —— C# 的 `Paste` 里没有 `Light` 这一行（`:84-86`），本 crate 不擅自加。
//!   即：整格快照里有 `light`，粘贴路径**不用**它（印章路径 `MergeStamp` 才用，见
//!   `stamp.rs` 模块文档 §一 第 10 条）。仍搬不动的只剩 5 个"视图无字节"的动画字段与 36 B 尾区。
//! - **差异 C（2026-11-05 新增；原「差异 D」）**：官方「三层全关就拒绝复制」的判据里还带一个
//!   **碰撞层可见**的条件（`RegionCopy.cs:106` 的 `&& !CollisionVisible`）；本 crate 的
//!   剪贴板**不抓阻挡位**（[`Clipboard::capture`] 只取低 15 位，`bit15` 由粘贴时的
//!   目标格保留矩阵决定）⇒ 三层全关就等于"没有任何可抓的东西"，**不需要**碰撞层这个第二条件。
//!   即：本 crate 的拒绝判据比官方**更宽**（官方"三层全关但碰撞可见"仍可复制，本 crate 拒绝），
//!   但那种情况下官方复制到的也只有阻挡位，本 crate 本来就不搬它。
//!
//! # 四、粘贴复用 [`crate::stamp`]，不另写位运算
//!
//! 粘贴把剪贴板原样当成"一枚印章的格表"交给 [`crate::stamp::plan_stamp`] ⇒
//! 三族写字段、`bit15` 保留矩阵、逐格越界裁剪、Type5 闸门、整次一步撤销，
//! 全部与 [`crate::EditSession::place_stamp`] **同一份实现**（派单 §三.3 的硬要求）。
//!
//! ⚠ **两处口径差异由 `PlanMode` 分开**（不是各写一份实现）：
//! ① 闸门 —— 粘贴**不套**占用闸门与坐标重复闸门（`RegionCopy.cs:71` 的 `CanPaste` 只判边界）；
//! ② 附加字段 —— 粘贴按 `RegionCopy.cs:84-86` 搬门/门偏移 + 前景动画、**不碰光照**，
//! 印章按 `PlacedStamps.cs:93-94` 搬（门/光照都是"非 0 才搬"）。逐条见 `stamp.rs` §一 第 10 条。
//!
//! ⚠ **锚点在进 `plan_stamp` 之前算**（`RegionCopy.cs:121-122` 的顺序）：
//! [`crate::EditSession::paste_clipboard`] 先 `clip.anchor(x, y)`，再把**锚点**当左上角交给
//! `plan_stamp`；负锚点 ⇒ 整块拒绝（连 `plan_stamp` 都不进）。**不许**在 `stamp.rs` 里再算一次
//! —— 印章路径没有"源坐标"这个概念，锚点只属于剪贴板。
//!
//! # 五、`FillSelection` 的地表步长（**有意与 C# 不同**）
//!
//! C# `MapViewport.cs:540` 的 `FillSelection` 是"对矩形里每一格调一次 `Apply`"，
//! 而 `Apply` 的地表分支带 `int step = Layer == MapLayer.Ground ? 2 : 1; if (step == 2) { x &= ~1; y &= ~1; }`
//! （`:474-475`）⇒ **3×3 选区在奇数原点会被对齐到偶偶格、覆盖 4×4，溢出到选区外**。
//!
//! 本 crate 取 **step=1（严格填满选区）**，理由两条：
//! 1. 溢出选区违背用户预期（选 3×3 却改了 4×4，撤销里也是 16 格）；
//! 2. 与本 crate 既有笔刷口径一致（`session.rs:22-26`：`paint()` 三层都逐格）。
//!
//! 代价：地表**奇格**写进去也**不显示**（渲染只铺偶偶格，`miredit-render/src/compose.rs:107-116`
//! 的 `if x & 1 != 0 { continue }`）—— 属**无害**，且与"笔刷能画到、填充也能填到"同一套网格。

use mir2_format::FormatId;
use mir2_map::Map;

use crate::convert::writes_layer_lib_byte;
use crate::document::cell_at;
use crate::layer::{EditLayer, COLLISION_BIT, SPRITE_MAX};
use crate::stamp::StampCellInput;
use crate::undo::Change;

/// 一个**矩形选区**：闭区间 `[x0..=x1] × [y0..=y1]`（格坐标）。
///
/// 边界语义见模块文档 §一：C# 存的是半开的 `Rect`（`Right`/`Bottom` 不含端点），
/// 但构造时已把「端点差 +1」写进宽高（`MapViewport.cs:400`）⇒ 格集合就是闭区间，
/// 本结构直接存两端点，两种写法等价。
///
/// ⚠ 字段是 `pub` 的（网页层要能直接拼一个发过来），所以**不保证** `x0 <= x1`：
/// [`Self::width`] / [`Self::iter`] / [`Self::clamp_to`] 都会先规范化（取 min/max），
/// 反向坐标（从右下往左上拖）不会被静默丢掉。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    /// 左端列（含）。
    pub x0: u16,
    /// 上端行（含）。
    pub y0: u16,
    /// 右端列（**含**，= C# `Rect.Right - 1`）。
    pub x1: u16,
    /// 下端行（**含**，= C# `Rect.Bottom - 1`）。
    pub y1: u16,
}

impl Selection {
    /// 造一个选区（**不裁剪**；裁剪请用 [`Self::clamp_to`]）。反向端点会被规范化。
    pub const fn new(x0: u16, y0: u16, x1: u16, y1: u16) -> Self {
        Self { x0, y0, x1, y1 }
    }

    /// 规范化后的列区间 `(左, 右)`（两端都含）。
    const fn xs(self) -> (u16, u16) {
        if self.x0 <= self.x1 {
            (self.x0, self.x1)
        } else {
            (self.x1, self.x0)
        }
    }

    /// 规范化后的行区间 `(上, 下)`（两端都含）。
    const fn ys(self) -> (u16, u16) {
        if self.y0 <= self.y1 {
            (self.y0, self.y1)
        } else {
            (self.y1, self.y0)
        }
    }

    /// 宽（列数，≥ 1）。C# 侧等价于 `Rect.Width`（`MapViewport.cs:400` 的 `|bx-ax|+1`）。
    pub const fn width(self) -> u16 {
        let (a, b) = self.xs();
        // `b - a + 1` 最大 65536（a=0, b=65535）⇒ 用 u32 算再收窄，避免 u16 溢出
        (b as u32 - a as u32 + 1) as u16
    }

    /// 高（行数，≥ 1）。C# 侧等价于 `Rect.Height`。
    pub const fn height(self) -> u16 {
        let (a, b) = self.ys();
        (b as u32 - a as u32 + 1) as u16
    }

    /// 格数 = 宽 × 高（`u64`，防 65535² 溢出）。C# 侧对应 `width * height`（`RegionCopy.cs:21`）。
    pub const fn count(self) -> u64 {
        self.width() as u64 * self.height() as u64
    }

    /// 该格在不在选区内。判据逐字对齐 `MapViewport.cs:115` 的 `SelectionContains`
    /// （那边是半开：`x >= Left && x < Right`；这边闭区间：`x0 <= x <= x1`，等价）。
    pub const fn contains(self, x: u16, y: u16) -> bool {
        let (ax, bx) = self.xs();
        let (ay, by) = self.ys();
        x >= ax && x <= bx && y >= ay && y <= by
    }

    /// **裁剪到地图内**（`w`/`h` = 地图宽高）；整块都在图外 ⇒ `None`。
    ///
    /// 复刻 C# 的两条不变量（模块文档 §一 第 2 条）：`MapViewport.cs:390` 要求起点在图内、
    /// `:399` 把终点 `Math.Clamp` 到 `0..=Width-1` ⇒ 官方存下来的选区**永远整块在图内**。
    /// 本 crate 在**入口处**裁剪一次，后续所有迭代都不必再判越界。
    ///
    /// 由于 `u16` 没有负数，"整块在图外"只可能发生在右/下侧（`x0 >= w` 或 `y0 >= h`）。
    pub const fn clamp_to(self, w: u16, h: u16) -> Option<Self> {
        if w == 0 || h == 0 {
            return None;
        }
        let (x0, x1) = self.xs();
        let (y0, y1) = self.ys();
        if x0 >= w || y0 >= h {
            return None;
        }
        // 到这里 x0 < w 且 y0 < h ⇒ 至少有一格在图内，裁剪后必定非空
        Some(Self {
            x0,
            y0,
            x1: if x1 < w { x1 } else { w - 1 },
            y1: if y1 < h { y1 } else { h - 1 },
        })
    }

    /// 按**行优先**遍历选区里的每一格（`y` 外层、`x` 内层）。
    ///
    /// 顺序对齐 C# 的两处双层循环：`MapViewport.cs:540`/`:546` 的
    /// `for (int y = …; y < r.Bottom; y++) for (int x = …; x < r.Right; x++)`
    /// 与 `RegionCopy.cs:22` 的 `for dy … for dx`。撤销步里的格序也因此稳定可复现。
    pub fn iter(self) -> impl Iterator<Item = (u16, u16)> {
        let (x0, x1) = self.xs();
        let (y0, y1) = self.ys();
        (y0..=y1).flat_map(move |y| (x0..=x1).map(move |x| (x, y)))
    }
}

/// **剪贴板**：一块矩形区域的**整格快照**（相对选区左上角）。
///
/// 复用 [`StampCellInput`]（`dx`/`dy` + 三族图号/库号 + 门/光照/动画），因此粘贴可以直接走
/// [`crate::stamp::plan_stamp`]（模块文档 §四）。抓取口径是**整格**（`RegionCopy.cs:24`），
/// 不是只抓三族 —— 详见 [`Self::capture`] 与模块文档 §三 差异 B。
///
/// 字段私有：格表只能由 [`crate::EditSession::copy_selection`] 产生，
/// 或由上层用 [`Self::from_cells`] 显式造（网页版要能"把上次复制的区域带回来"）。
///
/// ⚠ 图号只存**低 15 位**（`& 0x7FFF`），理由见 [`Self::capture`]。
///
/// # 为什么还要记 `source` / `ground`（2026-11-06 新增）
///
/// 它们只为了一件事：复刻上游 `RegionCopy.Anchor`（`RegionCopy.cs:70`）的**奇偶重算**——
/// 那个公式要吃「复制时选区左上角 (`sourceX`/`sourceY`，`:18`)」与「复制时地表层可见 (`ground`，`:19`)
/// 两个量。**不记就无法在粘贴时重算**（用别的东西凑都是猜测）。
/// 二者都只是**元数据**，不参与格表；[`Self::from_cells`] 造的无源剪贴板两者都缺 ⇒
/// [`Self::anchor`] 原样返回 `(x, y)`（= 旧行为，见该方法的文档）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Clipboard {
    cells: Vec<StampCellInput>,
    w: u16,
    h: u16,
    /// 复制时选区**左上角**（`RegionCopy.cs:18` 的 `sourceX = x; sourceY = y;`）。
    /// `None` = 无源剪贴板（[`Self::from_cells`] 造的）。
    source: Option<(u16, u16)>,
    /// 复制时**地表层是否可见**（`RegionCopy.cs:107` 传给构造函数的 `ground`、`:19` 存下）。
    ///
    /// ⚠ 是"复制那一刻的地表可见性"，**不是**"抓到的格表里有没有地表内容" ——
    /// 上游 `Anchor`（`:70`）判的就是这个 flag，两者混淆会让锚点在对空地表块上算错。
    ground: bool,
}

impl Clipboard {
    /// 宽（列数）。
    pub const fn width(&self) -> u16 {
        self.w
    }

    /// 高（行数）。
    pub const fn height(&self) -> u16 {
        self.h
    }

    /// 格表（**行优先**，含空格；`cells.len() == width * height`）。
    pub fn cells(&self) -> &[StampCellInput] {
        &self.cells
    }

    /// 是不是空剪贴板（没有格）。
    pub const fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// 复制时选区**左上角**（`RegionCopy.cs:18` 的 `sourceX/sourceY`）。
    ///
    /// `None` = 无源剪贴板（[`Self::from_cells`] 造的，没有"从哪来"这回事）。
    pub const fn source(&self) -> Option<(u16, u16)> {
        self.source
    }

    /// 复制时**地表层是否可见**（`RegionCopy.cs:107` 的 `VisibleLayers[0]`）。
    ///
    /// `false` ⇒ 粘贴**不重算锚点**（上游 `Anchor` 的 `ground ? … : (x, y)` 后半支）。
    /// 注意它**不是**"剪贴板里有没有地表内容"：地表隐藏时抓到的 `bk` 固然全是 0，
    /// 但地表可见而选区内恰好没有地表图号时，本方法仍是 `true`（上游同样如此）。
    pub const fn ground(&self) -> bool {
        self.ground
    }

    /// **粘贴锚点**：把"鼠标/点击格"换算成真正的粘贴左上角。
    ///
    /// 逐字复刻上游 `RegionCopy.cs:70`：
    ///
    /// ```text
    /// C#:  public (int X, int Y) Anchor(int x, int y)
    ///          => ground ? (x - ((x - sourceX) & 1), y - ((y - sourceY) & 1)) : (x, y);
    /// 本仓: ground && source.is_some() ? (x - ((x - sourceX) & 1), y - ((y - sourceY) & 1)) : (x, y)
    /// ```
    ///
    /// - `ground` 为假、或没有 `source`（[`Self::from_cells`] 造的无源剪贴板）⇒ **原样**返回
    ///   `Some((x, y))`：无源剪贴板**不做奇偶重算**（没有 `sourceX` 就没法算，猜一个就是编数据）。
    /// - **负锚点**（点在地图最左/最上列且源原点奇偶对不上）⇒ `None`。
    ///   对应上游 `PlaceRegion`（`:121-122`）先 `Anchor` 再 `CanPaste`：负坐标必然
    ///   `Contains` 为 false ⇒ `Paste` 返回 false ⇒ **整块拒绝、一格不写**。
    ///   ⇒ 调用方拿到 `None` 必须**整块拒绝**，不许把它当成 `(0, y)` 之类夹到图内。
    ///
    /// ⚠ 用 `i32` 算再收窄：`u16` 直接减会下溢（`0u16 - 1u16` 是 panic/回绕，不是 `-1`）。
    /// 负数的 `& 1` 语义与 C# 一致（补码：`(-1) & 1 == 1`），所以右边不必再加分支。
    ///
    /// 注意本方法只**计算**，不改任何状态：粘贴时会话拿结果当 `plan_stamp` 的左上角
    /// （见 [`crate::EditSession::paste_clipboard`]）。
    pub fn anchor(&self, x: u16, y: u16) -> Option<(u16, u16)> {
        let (Some((sx, sy)), true) = (self.source, self.ground) else {
            return Some((x, y));
        };
        let (x, y) = (i32::from(x), i32::from(y));
        let ax = x - ((x - i32::from(sx)) & 1);
        let ay = y - ((y - i32::from(sy)) & 1);
        if ax < 0 || ay < 0 {
            return None;
        }
        // 到这里必然在 0..=u16::MAX 内：ax = x 或 x-1，x ≤ 65535
        Some((ax as u16, ay as u16))
    }

    /// 用一份现成的格表造剪贴板（上层把"上次复制的区域"带回来时用）。
    ///
    /// `w × h` 必须等于 `cells.len()`，否则 `None` —— 宽高与格数对不上时，
    /// 粘贴出来的矩形会是歪的，宁可拒绝。
    ///
    /// ⚠ **无源剪贴板**：`source = None`、`ground = false` ⇒ [`Self::anchor`] **原样**返回
    /// `(x, y)`，即**不做奇偶重算**（保持本方法在 2026-11-06 之前的行为）。
    /// 理由：奇偶重算需要 `sourceX/sourceY`，而这条路根本不知道源在哪；
    /// 按猜测填一个源坐标会让"网页带回来的剪贴板"莫名其妙错位一格。
    /// 要吃到上游对齐语义，只能走 [`crate::EditSession::copy_selection`] 那条真实复制路径。
    pub fn from_cells(w: u16, h: u16, cells: Vec<StampCellInput>) -> Option<Self> {
        if w == 0 || h == 0 || cells.len() != usize::from(w) * usize::from(h) {
            return None;
        }
        Some(Self {
            cells,
            w,
            h,
            source: None,
            ground: false,
        })
    }

    /// 从地图上抓一块矩形（**整块都抓，空格也抓**）。来源：`RegionCopy.cs:21-27`。
    ///
    /// 除了格表，还记两样**元数据**（2026-11-06，供 [`Self::anchor`] 复刻 `RegionCopy.cs:70`）：
    /// `source = Some((选区左上角 x, y))`（`RegionCopy.cs:18`）与 `ground = vis[0]`
    /// （`RegionCopy.cs:19`/`:107` 的"复制那一刻地表可见"）。
    ///
    /// # 为什么只存低 15 位
    ///
    /// `bit15` 是**阻挡位**（`layer.rs:248-265` 的保留矩阵），而粘贴时永远保留
    /// **目标格**自己的 `bit15`（`RegionCopy.cs:84` 的 `target.BackImage & 0x60000000`，
    /// 本 crate 由 `paint()` 的 `(cur & 0x8000) | low15` 实现）⇒ 源格的 `bit15`
    /// **搬过去也没用**。更要紧的是：`plan_stamp` 的 Type5 闸门按 `bk != 0` 判"有没有地表"
    /// （`stamp.rs:217-227`），若把 `bk_img == 0x8000`（被挡住、无图号）原样带进剪贴板，
    /// 会被误判成"有地表"而在 Type5 上**过度拒绝**。⇒ 存 `& 0x7FFF` 后闸门才是精确的。
    ///
    /// 中层同理（`mid_img` 没有阻挡位，`layer.rs:255-256`），掩 `0x7FFF` 只是统一口径。
    ///
    /// ⚠ 选区必须**整块在图内**（[`Selection::clamp_to`] 已保证）；图外的格直接跳过
    /// （不 panic、不静默改尺寸 —— 真发生了说明调用方绕过了 `clamp_to`）。
    ///
    /// # 可见性过滤（2026-11-05 新增，对齐 `RegionCopy.cs:106-107`）
    ///
    /// `vis = [地表, 中层, 物体]` 与 C# 的 `VisibleLayers[0..2]` 同序（`RegionCopy.cs:107`
    /// 就是这么传的）。**隐藏层的族一律不抓，对应字段填 0**：
    ///
    /// | 隐藏的层 | 置 0 的字段 |
    /// |---|---|
    /// | 地表 | `bk`、`bk_lib`（+ 恒 0 的 `tile_animation`/`tile_offset`/`tile_frames`） |
    /// | 中层 | `mid`、`mid_lib`（+ 恒 0 的 `mid_ani_frame`/`mid_ani_tick`） |
    /// | 物体 | `fr`、`fr_lib`、`door`、`door_offset`、`ani_frame`、`ani_tick` |
    ///
    /// 为什么在**复制**时就滤（而不是粘贴时查当前可见性）：C# 的层选择是
    /// **复制那一刻定格**的 —— `RegionCopy.cs:3` 的原注释
    /// 「A detached snapshot: layer choices are captured when copying, not when pasting.」，
    /// 构造函数的 `ground/middle/objects` 参数由 `:107` 传入、`Paste`（`:78-82`）只读这三个
    /// 快照 flag。本 crate 没有逐族的布尔位（[`StampCellInput`] 里没有），
    /// 于是等价做法 = **复制时就把隐藏层抹成 0** ⇒ 粘贴侧（`stamp::合并粘贴附加字段`）
    /// 的判据仍是"该族有值才写"，结果与 C# 逐字节一致。
    ///
    /// ⚠ 只有**地表那一格 flag** 被额外存进 [`Self::ground`]（2026-11-06），但它**不参与**
    /// 逐族过滤 —— 它唯一的用途是 [`Self::anchor`] 的奇偶重算（`RegionCopy.cs:70`）。
    /// 中层/物体的可见性依旧只体现在"字段被抹成 0"上（本 crate 不需要它们的 flag：
    /// `Paste` 里 `middle`/`objects` 只影响是否写该族，而"该族有没有值"已经等价表达）。
    ///
    /// ⚠ `light` **不随层过滤**：它不是任何一族的字段（C# `Cell.Light` 是整格属性），
    /// 且粘贴路径**根本不写它**（`RegionCopy.cs:84-86` 里没有 `Light`，见 `stamp.rs` 的
    /// `合并粘贴附加字段`）⇒ 留着与 C# 的整格快照同构，也不会写坏目标格。
    pub(crate) fn capture(view: &Map, sel: Selection, vis: [bool; 3]) -> Self {
        // 宽高直接取选区自己的（`RegionCopy.cs:18` 的 `Width = width; Height = height;` 同口径）
        let (w, h) = (sel.width(), sel.height());
        let (抓地表, 抓中层, 抓物体) = (vis[0], vis[1], vis[2]);
        let mut cells = Vec::with_capacity(sel.count() as usize);
        let (x0, _) = sel.xs();
        let (y0, _) = sel.ys();
        for (x, y) in sel.iter() {
            let Some(c) = cell_at(view, x, y) else {
                continue;
            };
            // 行优先的相对偏移：`dx = x - x0`、`dy = y - y0`（`RegionCopy.cs:22-24` 的 `dy*width+dx`）
            //
            // ⚠ **整格搬运**：C# 抓的是**整个 `Cell`**（`RegionCopy.cs:24` 的
            // `var cell = document.Get(x + dx, y + dy).Copy();` —— `Copy()` 复制全部字段，
            // 不是只复制三族），所以这里除三族图号/库号外，门索引、门偏移、光照、
            // 前景动画帧/节拍也一并填上（这 5 个字段在 `mir2_map::Cell` 里都有独立字节：
            // `mir2-map/src/cell.rs:184-195` 的 off6/off7/off11/off8/off9）。
            cells.push(StampCellInput {
                dx: i32::from(x) - i32::from(x0),
                dy: i32::from(y) - i32::from(y0),
                // ── 三族图号 / 三个库号：**隐藏的层填 0**（`RegionCopy.cs:106-107`）──
                bk: if 抓地表 { c.bk_img & SPRITE_MAX } else { 0 },
                mid: if 抓中层 { c.mid_img & SPRITE_MAX } else { 0 },
                fr: if 抓物体 { c.fr_img & SPRITE_MAX } else { 0 },
                bk_lib: if 抓地表 { c.tiles } else { 0 },
                mid_lib: if 抓中层 { c.sm_tiles } else { 0 },
                fr_lib: if 抓物体 { c.area } else { 0 },
                // ── 以下 5 个字段在 12/14 B 格布局里**有**对应字节，逐字节照抄 ──
                // 门索引 / 门偏移：off6 / off7（`mir2-map/src/cell.rs:184-187`）
                // ⚠ 门属于**物体**层（C# `Paste` 的 `if (pasteObjects) { target.Door = … }`，
                //    `RegionCopy.cs:86`）⇒ 物体层隐藏时一并填 0。
                door: if 抓物体 { c.door_index } else { 0 },
                door_offset: if 抓物体 { c.door_offset } else { 0 },
                // 光照：off11（`mir2-map/src/cell.rs:195`）。**不随层过滤**：
                // 它不是任何一族的字段，且粘贴路径根本不写它（`RegionCopy.cs:84-86` 无 `Light`）。
                light: c.light,
                // 前景动画帧 / 节拍：off8 / off9（`mir2-map/src/cell.rs:188-191`）。
                // ⚠ C# 的 `Front.Frames/Tick` 就是这两个字节（`LegacyMapReader.cs:243-244` 读、
                // `:257-258` 只在有前景时赋值），所以它们是**前景**动画，不是中层动画。
                // ⇒ 物体层隐藏时填 0（同 `RegionCopy.cs:86` 的 `target.Foreground = source.Foreground`）。
                ani_frame: if 抓物体 { c.ani_frame } else { 0 },
                ani_tick: if 抓物体 { c.ani_tick } else { 0 },
                // ── 以下 5 个字段在 12 B 布局里没有这个字节 ⇒ 一律留 0（不猜、不编）──
                // 地表动画图号 / 偏移 / 帧数（C# `TileAnimation`/`TileOffset`/`TileFrames`，
                // `RegionCopy.cs:84` 会搬）在 12/14 B 档**没有对应字节**：它们是 36 B 档的
                // 扩展字段（`LegacyMapReader.cs:300-303`），在本 crate 里落在**不透明的 22 字节
                // 尾区** `Cell::tail`（`mir2-map/src/cell.rs:200-201`，语义未知、只许原样往返；
                // `convert.rs` 的 `FIELD_MAP` 第 12 条）。按猜测从尾区切三个字节出来会写坏未知数据，
                // 故留 0 并在 `stamp.rs` 的 `丢动画` 里如实告诉用户"这些字段没搬"。
                tile_animation: 0,
                tile_offset: 0,
                tile_frames: 0,
                // 中层动画帧 / 节拍（C# `MiddleFrames`/`MiddleTick`）同样**12 B 布局里没有这个字节**：
                // 12/14 B 档只有一组动画字节（off8/off9，已按前景动画填进 `ani_frame`/`ani_tick`），
                // 没有第二组给中层用（`mir2-map/src/cell.rs:32-33` 的字节表里只有一行 `btAniFrame`）。
                // C# 自己也承认这点：`Document.cs:19-20` 定义 `MiddleAnimationFrame` 恒 0，
                // `:63` 从 `s.MiddleAnimationFrame` 抄 ⇒ 14 B 图上没有可搬的中层动画。
                mid_ani_frame: 0,
                mid_ani_tick: 0,
            });
        }
        debug_assert_eq!(
            cells.len(),
            usize::from(w) * usize::from(h),
            "选区已被 clamp_to 裁到图内 ⇒ 抓到的格数必须正好是 宽×高"
        );
        Self {
            cells,
            w,
            h,
            // 源左上角：`RegionCopy.cs:18` 的 `sourceX = x; sourceY = y;`（`Anchor` 要用，`:70`）
            source: Some((x0, y0)),
            // 「复制那一刻地表层可见」：`RegionCopy.cs:107` 传给构造函数的 `VisibleLayers[0]`
            // ⚠ 不是"抓到的格表里有地表内容"，见 [`Self::ground`] 的文档
            ground: 抓地表,
        }
    }
}

/// 「清空选中格」的计算结果（**纯函数产物**；会话据此写盘 + 记一步撤销）。
///
/// 与 [`crate::stamp::StampPlan`] 同一个理由："先算后写" ⇒
/// "拒绝时一格不写"是**类型层面**保证的，不靠调用方记得先检查。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ClearPlan {
    /// 去重后的变更（一格里三族可能同时被清 ⇒ 合并成**一条** `Change`）。
    pub(crate) changes: Vec<Change>,
    /// 需要说给用户听的中文说明（库号字节写不进去）。
    pub(crate) notes: Vec<String>,
}

/// 算出「清空这一块选区」要改哪些格（模块文档 §二 的三行规则）。
///
/// 判据全部来自 `RegionCopy.cs:45-69` 的 `CutSource`：
/// - 每族**只在"该族有图号"时才清**（`:52`/`:57`/`:61` 的三个 `if`）；
/// - 每族还要求**该层可见**（`vis`）—— `:52` 是 `if (ground && …)`、`:57` 是 `if (middle && …)`、
///   `:61` 是 `if (objects && …)`，三个 `ground/middle/objects` 就是复制时定格的可见性
///   （`RegionCopy.cs:107`）⇒ **隐藏的层不清**；
/// - 清图号时**保留 `bit15`**（`:54` 的 `& 0x60000000`）；
/// - 库号字节**写 0**（`:54` 的 `target.BackLibrary = 0`、`:59` 的 `MiddleLibrary = 0`、
///   `:63` 的 `Foreground = null` 连带物体库号）。
///
/// ⚠ `vis` 的两个调用口径不同（都不是随手给的）：
/// - [`crate::EditSession::cut_selection`] 传**当前可见性** —— 它对应 `CutSource`，按层清；
/// - [`crate::EditSession::delete_selection`] 传 `[true; 3]` —— 它的 C# 对照是
///   `MapViewport.cs:546` 的 `ClearSelection`（**不按可见性**），本 crate 已登记为"三族一起清"。
///
/// 库号字节的**格式闸门**：writer 不写该字节的格式（`writes_layer_lib_byte` 为假）⇒
/// 视图里也不许置 0，否则会出现"视图显示 0、文件还是旧值、重开又变回来"的假象
/// （同一套判据见 `convert.rs:515-536` 的 `restore_unwritten_lib_byte`）。
pub(crate) fn clear_plan(view: &Map, id: FormatId, sel: Selection, vis: [bool; 3]) -> ClearPlan {
    let (清地表, 清中层, 清物体) = (vis[0], vis[1], vis[2]);
    let mut changes: Vec<Change> = Vec::with_capacity(sel.count() as usize);
    let mut 丢库号: Vec<&'static str> = Vec::new();
    for (x, y) in sel.iter() {
        let Some(before) = cell_at(view, x, y).copied() else {
            continue; // 选区已被 `clamp_to` 裁到图内，这条只是防御
        };
        let mut after = before;
        // 地表：`RegionCopy.cs:52-56`（`if (ground && (s.Cell.BackImage & 0x1FFFFFFF) != 0)`）
        if 清地表 && before.bk_img & SPRITE_MAX != 0 {
            after.bk_img = before.bk_img & COLLISION_BIT;
            if before.tiles != 0 {
                if writes_layer_lib_byte(id, EditLayer::Ground) {
                    after.tiles = 0;
                } else if !丢库号.contains(&EditLayer::Ground.name()) {
                    丢库号.push(EditLayer::Ground.name());
                }
            }
        }
        // 中层：`RegionCopy.cs:57-60`（中层没有阻挡位 ⇒ 整字段置 0）
        //
        // ⚠ 动画两字节**不在这里清**（模块文档 §二 第 4/5 条）：视图的 `ani_frame`/`ani_tick` 是
        // **前景**动画（清它会抹掉前景），真正的中层动画 `mid_ani_frame`/`mid_ani_tick` 又不在视图里
        // （`convert.rs` 的 `LOST_IN_VIEW`）⇒ 这一层现在既不该清 `ani_*`、也清不到 `mid_ani_*`。
        if 清中层 && before.mid_img & SPRITE_MAX != 0 {
            after.mid_img = 0;
            if before.sm_tiles != 0 {
                if writes_layer_lib_byte(id, EditLayer::Middle) {
                    after.sm_tiles = 0;
                } else if !丢库号.contains(&EditLayer::Middle.name()) {
                    丢库号.push(EditLayer::Middle.name());
                }
            }
        }
        // 物体：`RegionCopy.cs:61-64`（保留 `bit15` = 高位阻挡）
        if 清物体 && before.fr_img & SPRITE_MAX != 0 {
            after.fr_img = before.fr_img & COLLISION_BIT;
            if before.area != 0 {
                if writes_layer_lib_byte(id, EditLayer::Front) {
                    after.area = 0;
                } else if !丢库号.contains(&EditLayer::Front.name()) {
                    丢库号.push(EditLayer::Front.name());
                }
            }
        }
        if after == before {
            continue; // 这一格三族都没内容（或只有阻挡位）⇒ 不算改
        }
        changes.push(Change {
            x,
            y,
            before,
            after,
        });
    }
    let mut notes = Vec::new();
    if !丢库号.is_empty() {
        // ⚠ **旧文案是假陈述，已订正（2026-11-05）**：原文写「writer 不写它」，
        // 但 `Old12`/`AntiHack` 的物体层（`area`）**是写的** ——
        // `mir2-format/src/formats/old12.rs:158` 的 `out.push(cell.area)`。
        // 判据本来就在 `writes_layer_lib_byte` 里（按格式 × 层查表），文案只说准确的那部分。
        notes.push(format!(
            "{} 里 {} 没有独立的库号字节（该格式的格布局里这一层不带库号）—— \
             该字节未清零，只清图号",
            mir2_format::format_by_id(id).name(),
            丢库号.join(" / ")
        ));
    }
    ClearPlan { changes, notes }
}

/// 会话里「选区 + 剪贴板」这一对 UI 状态（见模块文档 §一 第 5 条：不进撤销栈、不置脏）。
///
/// 单独抽成一个结构而不是两个字段：`EditSession::new` 里一行 `Default::default()` 就装完，
/// 也方便将来整体快照/恢复（C# 的 `MapEditorState` 就是把它和 `History` 并列存的，`:147`/`:152`）。
#[derive(Debug, Default)]
pub(crate) struct SelectionState {
    /// 当前选区（`None` = 没有选区，= C# 的 `Selection == null`）。
    pub(crate) sel: Option<Selection>,
    /// 剪贴板（`None` = 还没复制过，= C# 的 `copiedRegion == null`，`RegionCopy.cs:96`）。
    pub(crate) clip: Option<Clipboard>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::造图;
    use mir2_map::{CellFormat, Map};

    /// 3×3 选区的宽/高/格数/包含判定/遍历顺序 —— 逐条对 `MapViewport.cs:115`、`:400`。
    #[test]
    fn 选区闭区间语义() {
        let s = Selection::new(2, 3, 4, 5);
        assert_eq!(s.width(), 3);
        assert_eq!(s.height(), 3);
        assert_eq!(s.count(), 9);
        // 四个角与中心都在内
        for (x, y) in [(2, 3), (4, 3), (2, 5), (4, 5), (3, 4)] {
            assert!(s.contains(x, y), "({x},{y}) 应在选区内");
        }
        // 边界外一格就不在（闭区间的两端都含，外扩一格不含）
        for (x, y) in [(1, 3), (5, 3), (2, 2), (2, 6)] {
            assert!(!s.contains(x, y), "({x},{y}) 不该在选区内");
        }
        // 行优先：y 外层、x 内层（`MapViewport.cs:540` 的循环顺序）
        let 格: Vec<(u16, u16)> = s.iter().collect();
        assert_eq!(格.len(), 9);
        assert_eq!(格[0], (2, 3), "第一格是左上角");
        assert_eq!(格[2], (4, 3), "第 3 格是右上角");
        assert_eq!(格[8], (4, 5), "最后一格是右下角");
        assert_eq!(
            格,
            vec![
                (2, 3),
                (3, 3),
                (4, 3),
                (2, 4),
                (3, 4),
                (4, 4),
                (2, 5),
                (3, 5),
                (4, 5)
            ]
        );
    }

    /// 反向坐标（从右下往左上拖）必须被规范化，而不是被静默丢掉。
    #[test]
    fn 反向坐标规范化() {
        let s = Selection::new(5, 5, 2, 3);
        assert_eq!(s.width(), 4);
        assert_eq!(s.height(), 3);
        assert_eq!(s.clamp_to(9, 9), Some(Selection::new(2, 3, 5, 5)));
        assert!(s.contains(2, 3) && s.contains(5, 5));
    }

    /// **越界裁剪**：复刻 `MapViewport.cs:399` 的 `Math.Clamp`；整块在图外 ⇒ `None`。
    #[test]
    fn 越界裁剪到图内() {
        // 右下越界 ⇒ 裁到 (2,3)..(4,4)
        let s = Selection::new(2, 3, 99, 99);
        assert_eq!(s.clamp_to(5, 5), Some(Selection::new(2, 3, 4, 4)));
        // 起点就越界 ⇒ 空（u16 没有负数，只可能发生在右/下侧）
        assert_eq!(Selection::new(5, 0, 9, 9).clamp_to(5, 5), None);
        assert_eq!(Selection::new(0, 5, 9, 9).clamp_to(5, 5), None);
        // 未开图
        assert_eq!(Selection::new(0, 0, 0, 0).clamp_to(0, 0), None);
        // 整块在图内 ⇒ 原样
        let 内 = Selection::new(1, 1, 3, 3);
        assert_eq!(内.clamp_to(9, 9), Some(内));
        // 一格选区（单击不拖）
        let 一 = Selection::new(4, 4, 4, 4);
        assert_eq!(一.count(), 1);
        assert_eq!(一.clamp_to(5, 5), Some(一));
    }

    /// **整块都抓**（空格也抓），行优先、`dx/dy` 相对左上角、三族只存低 15 位。
    ///
    /// 来源：`RegionCopy.cs:21-27`（`cells = new Cell[w*h]` 逐格全填）。
    #[test]
    fn 剪贴板整块抓取含空格() {
        let mut map: Map = 造图(8, 8, CellFormat::New14);
        // (2,3) 三族都给值、库号也给；(3,3) 只留一个阻挡位（bit15，无图号）⇒ 算空格
        let i = crate::document::index_of(&map, 2, 3).unwrap();
        map.cells[i].bk_img = 0x8000 | 11;
        map.cells[i].mid_img = 0x8000 | 12; // 中层的 bit15 属异常数据，抓取时被掩掉
        map.cells[i].fr_img = 0x8000 | 13;
        map.cells[i].tiles = 21;
        map.cells[i].sm_tiles = 22;
        map.cells[i].area = 23;
        let j = crate::document::index_of(&map, 3, 3).unwrap();
        // 夹具每格都有三族图号 ⇒ 这一格先清成"只有阻挡位、没有图号"
        map.cells[j].bk_img = 0x8000;
        map.cells[j].mid_img = 0;
        map.cells[j].fr_img = 0;

        let clip = Clipboard::capture(&map, Selection::new(2, 3, 4, 5), [true; 3]);
        assert_eq!((clip.width(), clip.height()), (3, 3));
        assert_eq!(clip.cells().len(), 9, "整块都抓（含空格）");
        assert!(!clip.is_empty());
        // 元数据（2026-11-06）：源左上角 = 选区左上角（`RegionCopy.cs:18`）、地表可见（`:107`）
        assert_eq!(clip.source(), Some((2, 3)), "source 必须是复制时选区左上角");
        assert!(clip.ground(), "三层默认全开 ⇒ ground 为真");
        // 行优先 + 相对左上角
        let 首 = clip.cells()[0];
        assert_eq!((首.dx, 首.dy), (0, 0));
        assert_eq!(clip.cells()[8].dx, 2);
        assert_eq!(clip.cells()[8].dy, 2);
        // 三族低 15 位 + 三个库号
        assert_eq!(首.bk, 11, "bit15 被掩掉（阻挡位不随剪贴板搬）");
        assert_eq!(首.mid, 12);
        assert_eq!(首.fr, 13);
        assert_eq!((首.bk_lib, 首.mid_lib, 首.fr_lib), (21, 22, 23));
        // (3,3) 只有阻挡位 ⇒ 三族图号都是 0（Type5 闸门据此判"没有地表"）
        let 空 = clip.cells()[1];
        assert_eq!((空.dx, 空.dy), (1, 0));
        assert_eq!((空.bk, 空.mid, 空.fr), (0, 0, 0));
    }

    /// 清空：三族各按"有图号才清、保留 `bit15`、库号写 0"，且不动其它字段。
    ///
    /// 来源：`RegionCopy.cs:45-69`；差异（不清库号的 `ClearSelection`）见模块文档 §二。
    #[test]
    fn 清空按层语义() {
        let mut map: Map = 造图(8, 8, CellFormat::New14);
        let i = crate::document::index_of(&map, 4, 4).unwrap();
        map.cells[i].bk_img = 0x8000 | 111;
        map.cells[i].mid_img = 222;
        map.cells[i].fr_img = 0x8000 | 333;
        map.cells[i].tiles = 7;
        map.cells[i].sm_tiles = 8;
        map.cells[i].area = 9;
        let 原 = map.cells[i];
        let plan = clear_plan(&map, FormatId::Shanda14, Selection::new(4, 4, 4, 4), [true; 3]);
        assert_eq!(plan.changes.len(), 1);
        let c = plan.changes[0].after;
        assert_eq!(c.bk_img, 0x8000, "地表只留阻挡位");
        assert_eq!(c.mid_img, 0, "中层整字段置 0");
        assert_eq!(c.fr_img, 0x8000, "物体只留阻挡位");
        assert_eq!((c.tiles, c.sm_tiles, c.area), (0, 0, 0), "三个库号字节写 0");
        // 其余字段原样（模块文档 §二 第 3 条：动画/门/光照/尾区一律不碰）
        assert_eq!(c.ani_frame, 原.ani_frame);
        assert_eq!(c.ani_tick, 原.ani_tick);
        assert_eq!(c.door_index, 原.door_index);
        assert_eq!(c.door_offset, 原.door_offset);
        assert_eq!(c.light, 原.light);
        assert_eq!(c.tail, 原.tail);
        assert!(plan.notes.is_empty(), "Shanda14 三个库号字节都写 ⇒ 不该有告警");
    }

    /// 只有阻挡位（`bit15`）没有图号的格**不算有内容**，不该被清、也不该进撤销步。
    ///
    /// 来源：`RegionCopy.cs:52` 的 `(s.Cell.BackImage & 0x1FFFFFFF) != 0` —— 它把阻挡位掩掉了。
    #[test]
    fn 只有阻挡位的格不算内容() {
        let mut map: Map = 造图(4, 4, CellFormat::New14);
        let i = crate::document::index_of(&map, 1, 1).unwrap();
        // 夹具每格都有三族图号 ⇒ 先清成"只有阻挡位、没有图号"
        map.cells[i].bk_img = 0x8000;
        map.cells[i].mid_img = 0;
        map.cells[i].fr_img = 0x8000;
        map.cells[i].tiles = 5; // 有库号但没图号 ⇒ 按 C# 判据也不清
        let plan = clear_plan(&map, FormatId::Shanda14, Selection::new(1, 1, 1, 1), [true; 3]);
        assert!(plan.changes.is_empty(), "没有可清的内容");
    }

    /// 库号字节写不进去的格式（`Mir3Wemade` 只写 `area`）⇒ 视图里也不许置 0，并给一句说明。
    ///
    /// 判据同 `convert.rs:515-536`：否则会出现"视图显示 0、文件还是旧值"的假象。
    #[test]
    fn 库号写不进去要说明() {
        let mut map: Map = 造图(4, 4, CellFormat::New14);
        let i = crate::document::index_of(&map, 1, 1).unwrap();
        map.cells[i].mid_img = 222;
        map.cells[i].sm_tiles = 8;
        map.cells[i].fr_img = 333;
        map.cells[i].area = 9;
        let plan = clear_plan(&map, FormatId::Mir3Wemade, Selection::new(1, 1, 1, 1), [true; 3]);
        assert_eq!(plan.changes.len(), 1);
        let c = plan.changes[0].after;
        assert_eq!(c.mid_img, 0, "图号照清");
        assert_eq!(c.sm_tiles, 8, "Mir3Wemade 不写 btsmTiles ⇒ 视图也不许改");
        assert_eq!(c.area, 0, "Mir3Wemade 写 btArea ⇒ 库号照清");
        assert_eq!(plan.notes.len(), 1, "应有一条库号告警：{:?}", plan.notes);
        // ⚠ 文案订正（2026-11-05）：旧断言找的是「库号字节没有对应字节」，
        // 但旧文案本身是假陈述 —— `Old12`/`AntiHack` 的物体层（`area`）是写的
        // （`mir2-format/src/formats/old12.rs:158`）。现文案只说准确的那部分。
        assert!(plan.notes[0].contains("没有独立的库号字节"), "{:?}", plan.notes);
        assert!(plan.notes[0].contains("中层图层"), "{:?}", plan.notes);
    }

    /// 多格选区：只把**真的变了**的格记进变更（空格不进撤销步）。
    #[test]
    fn 空格不进撤销步() {
        let mut map: Map = 造图(6, 6, CellFormat::New14);
        // 只给 (2,2) 一个有内容的格
        let i = crate::document::index_of(&map, 2, 2).unwrap();
        map.cells[i].bk_img = 42;
        // 其余格先清成"空"（造图夹具每格都有图号）
        for k in 0..map.cells.len() {
            let (x, y) = ((k / 6) as u16, (k % 6) as u16);
            if (x, y) != (2, 2) {
                map.cells[k].bk_img = 0;
                map.cells[k].mid_img = 0;
                map.cells[k].fr_img = 0;
            }
        }
        let plan = clear_plan(&map, FormatId::Shanda14, Selection::new(1, 1, 3, 3), [true; 3]);
        assert_eq!(plan.changes.len(), 1, "9 格里只有 (2,2) 有内容");
        assert_eq!((plan.changes[0].x, plan.changes[0].y), (2, 2));
    }

    /// `from_cells` 的宽高/格数自洽闸门 + **无源剪贴板不做奇偶重算**（2026-11-06）。
    #[test]
    fn 造剪贴板校验宽高() {
        let cells = vec![
            StampCellInput::default(),
            StampCellInput::default(),
            StampCellInput::default(),
            StampCellInput::default(),
        ];
        assert!(Clipboard::from_cells(2, 2, cells.clone()).is_some());
        assert!(Clipboard::from_cells(3, 2, cells.clone()).is_none(), "4 格装不进 3×2");
        assert!(Clipboard::from_cells(0, 2, cells.clone()).is_none());
        let c = Clipboard::from_cells(2, 2, cells).expect("2×2");
        assert_eq!((c.width(), c.height()), (2, 2));
        // 无源剪贴板：`source`/`ground` 都缺 ⇒ 锚点原样返回（= 2026-11-06 之前的旧行为）
        assert_eq!(c.source(), None, "from_cells 不知道源在哪儿");
        assert!(!c.ground(), "from_cells 没有地表可见性这回事");
        for (x, y) in [(0u16, 1u16), (1, 1), (7, 9), (65535, 65535)] {
            assert_eq!(c.anchor(x, y), Some((x, y)), "无源剪贴板不许重算锚点：({x},{y})");
        }
        assert!(Clipboard::default().is_empty());
        assert_eq!(Clipboard::default().source(), None);
        assert!(!Clipboard::default().ground());
    }

    /// **粘贴锚点按源奇偶重算** —— 逐字对齐上游 `RegionCopy.cs:70` 的 `Anchor`。
    ///
    /// 单测只钉公式与边界（负锚点 / 无源 / 地表不可见）；会话级的"真的写到哪个格"由
    /// `tests/selection_clipboard.rs::A016_粘贴锚点按源奇偶` 验。
    #[test]
    fn 锚点按源奇偶() {
        let map: Map = 造图(8, 8, CellFormat::New14);
        // 源 = (1,3)..(3,5)：左边奇、上边奇（这样两种奇偶都能验到）
        let clip = Clipboard::capture(&map, Selection::new(1, 3, 3, 5), [true; 3]);
        assert_eq!(clip.source(), Some((1, 3)));
        assert!(clip.ground());
        // 点击点与源同奇偶 ⇒ 不挪（`(x-sourceX)&1 == 0`）
        assert_eq!(clip.anchor(1, 3), Some((1, 3)), "同奇偶：锚点 = 点击点");
        assert_eq!(clip.anchor(5, 9), Some((5, 9)));
        // 点击点与源异奇偶 ⇒ 往左上各挪一格
        assert_eq!(clip.anchor(2, 4), Some((1, 3)), "异奇偶：x/y 都 -1");
        assert_eq!(clip.anchor(0, 0), None, "源原点奇 ⇒ 点 (0,0) 重算成负数 ⇒ 拒绝");
        assert_eq!(clip.anchor(0, 4), None, "只有 x 为负也必须整块拒绝");
        assert_eq!(clip.anchor(2, 0), None, "只有 y 为负也必须整块拒绝");
        assert_eq!(clip.anchor(1, 5), Some((1, 5)), "(1,5) 与源 (1,3) 同奇偶 ⇒ 合法");
        // 边界：源在 (0,0) ⇒ 偶数对齐，永远不会为负
        let 零源 = Clipboard::capture(&map, Selection::new(0, 0, 1, 1), [true; 3]);
        assert_eq!(零源.anchor(0, 0), Some((0, 0)));
        assert_eq!(零源.anchor(1, 1), Some((0, 0)));
        assert_eq!(零源.anchor(65535, 65535), Some((65534, 65534)));
        // 地表**不可见** ⇒ 不重算（上游 `ground ? … : (x, y)` 的后半支）
        let 关地表 = Clipboard::capture(&map, Selection::new(1, 3, 3, 5), [false, true, true]);
        assert!(!关地表.ground());
        assert_eq!(关地表.anchor(2, 4), Some((2, 4)), "地表不可见 ⇒ 原样");
        assert_eq!(关地表.anchor(0, 0), Some((0, 0)), "不重算就永远不会出现负锚点");
    }
}
