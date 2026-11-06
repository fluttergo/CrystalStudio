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
//! 三处细节，逐条给理由：
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
//! | 6 | 地表粘贴会**按源的奇偶重算锚点** | `RegionCopy.cs:70`（`Anchor`：`x - ((x - sourceX) & 1)`） |
//! | 7 | 粘贴**整块必须图内**，否则整次失败 | `RegionCopy.cs:71`（`CanPaste`）、`:74`（`if (!CanPaste(document, x, y)) return false;`）、`:120`（`"Region extends outside the map"`） |
//! | 8 | 复制**按图层可见性过滤**：隐藏层的族不抓（字段填 0）；三层全关 ⇒ 拒绝 | `RegionCopy.cs:106`（`if (!VisibleLayers.Any(v => v) && !CollisionVisible) { …"Show at least one layer to copy"… return; }`）、`:107`（`VisibleLayers[0..2]` 传进快照） |
//! | 9 | 粘贴**不看当前可见性**，只用复制时定格的层选择 | `RegionCopy.cs:3`（`A detached snapshot: layer choices are captured when copying, not when pasting.`）、`:78-82`（`pasteGround = ground && …`） |
//!
//! **与官方的三处有意差异**（都已登记，不是悄悄改）：
//!
//! - **差异 A（对应第 7 条）**：官方粘贴是**整块拒绝**，本 crate 按任务书 §三.3 要求
//!   **逐格跳过**并在 [`crate::PlaceOutcome::Placed`] 的 `skipped` 里如实报数。
//!   依据是官方自己就有一条逐格判据（`PlacedStamps.cs:80` 的 `MapBoundary + continue`），
//!   与 `stamp.rs` 模块文档 §二 同一条理由、同一份实现。
//! - **差异 B（对应第 6 条）**：官方按源的奇偶重算锚点（`RegionCopy.cs:70`），本 crate 直接以
//!   调用方给的 `(x, y)` 为左上角。理由：本 crate 的地表落笔是**逐格**的
//!   （`session.rs:22-26` 已记档的差异：`paint()` 三层都逐格写，没有 2×2 网格概念），
//!   重算锚点只会让粘贴位置与用户点的位置错开 1 格。
//! - **差异 C（2026-11-05 订正）**：官方 `RegionCopy` 抓的是**整格**（`RegionCopy.cs:24` 的
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
//! - **差异 D（2026-11-05 新增）**：官方「三层全关就拒绝复制」的判据里还带一个
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
/// 不是只抓三族 —— 详见 [`Self::capture`] 与模块文档 §三 差异 C。
///
/// 字段私有：格表只能由 [`crate::EditSession::copy_selection`] 产生，
/// 或由上层用 [`Self::from_cells`] 显式造（网页版要能"把上次复制的区域带回来"）。
///
/// ⚠ 图号只存**低 15 位**（`& 0x7FFF`），理由见 [`Self::capture`]。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Clipboard {
    cells: Vec<StampCellInput>,
    w: u16,
    h: u16,
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

    /// 用一份现成的格表造剪贴板（上层把"上次复制的区域"带回来时用）。
    ///
    /// `w × h` 必须等于 `cells.len()`，否则 `None` —— 宽高与格数对不上时，
    /// 粘贴出来的矩形会是歪的，宁可拒绝。
    pub fn from_cells(w: u16, h: u16, cells: Vec<StampCellInput>) -> Option<Self> {
        if w == 0 || h == 0 || cells.len() != usize::from(w) * usize::from(h) {
            return None;
        }
        Some(Self { cells, w, h })
    }

    /// 从地图上抓一块矩形（**整块都抓，空格也抓**）。来源：`RegionCopy.cs:21-27`。
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
    /// 快照 flag。本 crate 没有这份 flag（[`StampCellInput`] 里没有布尔位），
    /// 于是等价做法 = **复制时就把隐藏层抹成 0** ⇒ 粘贴侧（`stamp::合并粘贴附加字段`）
    /// 的判据仍是"该族有值才写"，结果与 C# 逐字节一致。
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
        Self { cells, w, h }
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

    /// `from_cells` 的宽高/格数自洽闸门。
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
        assert!(Clipboard::default().is_empty());
    }
}
