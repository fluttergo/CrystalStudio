//! 印章放置：把一枚 `.X` 印章的格贴进地图（**逐格越界裁剪** + **格式能力闸门** + **一步撤销**）。
//!
//! 印章文件本身的读写不在这里 —— 那是 `mir2-stamp` 的事（本 crate 自 2026-11-05 起在
//! `[dependencies]` 里依赖它，但**只**经 [`crate::cells_from_stamp`] 用它：`.X` → 格表 的
//! 掩码/报错口径钉在 `stamp_convert` 一处，见那个模块的文档 §一）。
//! 本模块本身只吃「一枚印章的格表」这个**纯数据**，
//! 因此它既能被 `.X` 文件喂，也能被「从地图上框一块另存」的路径喂。
//!
//! # 一、语义逐条与来源
//!
//! | # | 语义 | 判据落点 | 来源 |
//! |---|---|---|---|
//! | 1 | **锚点**：`(x,y)` 是原点，格落在 `(x+dx, y+dy)`，`dx/dy` 可为负 | [`plan_stamp`] 的 `sx/sy` | `mir2-stamp/src/stamp.rs:22-32`（`StampCell.x/y` 是**相对坐标、可负**）；`CrystalStudio/PlacedStamps.cs:119`（`int sx=x+s.X, sy=y+s.Y`） |
//! | 2 | **越界裁剪**：出界的格**逐格跳过**并计入 `skipped`；**全部出界** ⇒ 拒绝且一格不写 | [`plan_stamp`] 的 `skipped` / `PlaceOutcome::Refused` | `PlacedStamps.cs:80`（`if(sx<0\|\|sy<0\|\|sx>=Width\|\|sy>=Height){blocked.Add(…MapBoundary);continue;}` —— 逐格标 `MapBoundary` 后 `continue`） |
//! | 3 | **写哪些字段**：三族图号 + 该层库号字节；**图号 0 = 该族没有内容 ⇒ 整族不写** | [`写一族`] | `PlacedStamps.cs:90-92`（`MergeStamp`：`if(a.BackImage!=0){…}` / `if(a.MiddleImage!=0){…}` / `if (a.Foreground is { } front) …`）；`Document.cs:31`（`Cell::Empty` 把「全 0」定义为空） |
//! | 4 | **`bit15` 保留矩阵**：地表 `bk_img` 保留、物体 `fr_img` 保留、中层 `mid_img` 不保留 | **复用** [`crate::paint`]（不另写一份） | `miredit-core/src/paint.rs` 的 `Tool::Brush` 分支（`let new_img = if layer.img_of_has_collision_bit() { (cur & COLLISION_BIT) \| low15 } else { low15 }`），即 `miredit-gui/src/edit.rs:1462-1468` 的公式；矩阵定性见 `layer.rs` 模块文档第二节 |
//! | 5 | **撤销**：整次放置 = **一步** | [`crate::UndoStack::commit`]（内部就是 `begin(); stroke = changes; end();`） | `undo.rs` 的 `commit` 文档：与 GUI `begin(); stroke = changes; end();` 等价（`edit.rs:1197-1199`）；填充同款 |
//! | 6 | **坐标重复 ⇒ 整枚拒绝**：印章自身 `(dx,dy)` 出现两次即拒（**不是**"后者胜"）；`待定` / `位置` 两张表退化为**防御**，只有粘贴路径（矩形展开，坐标天然唯一）才可能用到"后者胜" | [`plan_stamp`] 的重复闸门 + `待定` / `位置` | `PlacedStamps.cs:65-66`（`var visited=new HashSet<(int,int)>(); foreach(var s in cells)if(!visited.Add((s.X,s.Y)))return false;`） |
//! | 7 | **格式能力闸门**：`ground_paint_supported=false`（Type5）且印章里有 `bk != 0` ⇒ 拒绝 | [`plan_stamp`] 的 Type5 分支 | `mir2-format/src/model.rs:430-440`（Type5 背图在块级，`bk_img` 恒 0 且写回不参与编码） |
//! | 8 | **库号字节写不进去**（`convert::writes_layer_lib_byte` 为假）⇒ 该字节忽略并在原因里说明 | [`写一族`] 的 `丢库号` + [`plan_stamp`] 的 `notes` | `convert.rs` 模块文档第六节的对照表 |
//! | 9 | **占用闸门**：落点格**已有物件**（`fr_img & 0x7FFF != 0` = C# 的 `HasForeground`）⇒ **整枚拒绝**（不是逐格跳过） | [`plan_stamp`] 的占用闸门 | `PlacedStamps.cs:62-63`（`var c=Get((int)sx,(int)sy); if(c.HasForeground)return false;`）；`Document.cs:26`（`internal bool HasForeground => Foreground is { } front && (front.Image & 0x7FFF) != 0;`） |
//! | 10 | **三族之外的字段也搬**，但**两条路径各按自己那份 C# 字段表**：印章走 `MergeStamp`（门/门偏移 `if(!=0)`、光照 `if(!=0)`、前景动画 `fr!=0`）；粘贴走 `RegionCopy.Paste`（门/门偏移**有前景就无条件赋值**、前景动画同、**完全不碰光照**） | [`合并附加字段`] / [`合并粘贴附加字段`] + [`plan_stamp`] 的 `丢动画` | `PlacedStamps.cs:87-98`；`RegionCopy.cs:84-86` |
//! | 11 | 视图里装不下的 5 个动画字段（中层动画帧/节拍、地表动画图号/偏移/帧数）**如实说明**、绝不静默丢 | [`记丢动画`] | `PlacedStamps.cs:95-96`；`RegionCopy.cs:84-85`；`convert.rs` 的 `LOST_IN_VIEW` |
//!
//! # 二、与 CrystalStudio 的**有意差异**（已记档，不是悄悄改）
//!
//! 官方 `CanPlaceStamp`（`PlacedStamps.cs:52-68`）是**整枚拒绝**，它有三道判据：
//!
//! ```csharp
//! foreach(var s in cells) {
//!     long sx=(long)x+s.X, sy=(long)y+s.Y;
//!     if(sx<0||sy<0||sx>=Width||sy>=Height) return false;   // ① 出界
//!     var c=Get((int)sx,(int)sy);
//!     if(c.HasForeground) return false;                     // ② 落点已有物件
//! }
//! var visited=new HashSet<(int,int)>();
//! foreach(var s in cells) if(!visited.Add((s.X,s.Y))) return false;   // ③ 坐标重复
//! ```
//!
//! **②③ 本模块逐字照搬**（见上表第 6/9 条）；**① 有意保留逐格跳过**
//! （任务书 §三.2："超出地图边界的格逐格跳过并计入 `skipped`"），
//! 并在 [`PlaceOutcome::Placed`] 里把 `skipped` 如实报出来。
//!
//! ⚠ 之所以敢在①上这么改：官方自己就有一条**逐格**的判据 —— `CheckStampPlacement`（`:80`）
//! 对出界格是 `blocked.Add(…MapBoundary); continue;`（继续检查别的格、并给出逐格诊断），
//! 只有 `CanPlaceStamp` 那条快速通道才是整枚拒绝。两者在官方代码里**同时存在**。
//!
//! ⚠ **②③ 只作用于印章路径**：C# 的 `RegionCopy.Paste`（`RegionCopy.cs:71-91`）前置只有
//! `CanPaste`（`document.Contains(x,y) && document.Contains(x+Width-1, y+Height-1)`，`:71`），
//! **没有**占用闸门、也没有坐标重复概念 ⇒ 粘贴路径按 [`PlanMode::Paste`] 走，不套这两道闸门。
//!
//! # 三、关于 `CropStamp`（任务书里点名要读的那个）
//!
//! 任务书写的是「与 `mir2_format` 的 `CropStamp` 口径一致」，但**`mir2-format` 里没有
//! `CropStamp`**（全 crate grep 命中 0 次）。真正的 `CropStamp` 在 C# 侧：
//! `CrystalStudio/CrystalStudio/Document.cs:126-136`。它做的是**造印章时**的居中裁剪
//! （`occupied = cells.Where(s => !s.Cell.Empty)` → 取包围盒中心 → 整体平移），
//! **不是**"放置时按地图边界裁剪"。⇒ 本模块只在两处借它的口径：
//! ① 「**空格不算内容**」（`!s.Cell.Empty`，对应本模块的「图号 0 ⇒ 该族不写」）；
//! ② 地表有内容时按偶偶格对齐的**存在性**（`if (occupied.Any(s => (s.Cell.BackImage & 0x1FFFFFFF) != 0)) { centerX &= ~1; … }`）
//! —— 放置路径不重算锚点（锚点由调用方给），故本模块**不做**这步平移。
//!
//! # 四、`cells` 与 `skipped` 到底数什么（口径必须写死）
//!
//! - `cells` = **真正改动**的格数（去重后 = 那一步撤销里的 `changes.len()`）；
//! - `skipped` = **越界被跳过**的格数；
//! - ⇒ 二者之和**可能小于** `cells.len()`：差额是「落在图内、但三族图号都是 0
//!   或与目标格内容相同」的格（**没改**，所以既不算写、也不算跳过）。
//!   这种情况 [`plan_stamp`] 会额外给一句中文说明，绝不让调用方自己猜。

use std::collections::{HashMap, HashSet};

use mir2_format::{format_by_id, FormatId};
use mir2_map::{Cell, CellFormat, Map};

use crate::brush::Brush;
use crate::convert::{ground_paint_supported, restore_unwritten_lib_byte, writes_layer_lib_byte};
use crate::document::cell_at;
use crate::layer::{EditLayer, SPRITE_MAX};
use crate::paint::{paint, PaintOutcome, Tool};
use crate::undo::Change;

/// 一枚待放置的印章格：**相对锚点**的偏移 + 三族图号/库号 + **三族之外的附加字段**。
///
/// 与 `mir2_stamp::StampCellData` 的对应关系（`.X` 的 `StampCell` 就是 C# 的 `Cell` 子集）：
///
/// | 本结构 | `.X` 侧 | C# 侧（`Document.cs:7-32`） | 本 crate 落点 |
/// |---|---|---|---|
/// | `dx` / `dy` | `StampCell.x` / `.y`（`i32`） | `StampCell.X/.Y` | 相对坐标，**可负** |
/// | `bk` | `back_image as u16`（`i32` 截低 16 位） | `BackImage` | 地表图号 → `bk_img` |
/// | `mid` | `middle_image`（`u16`） | `MiddleImage` | 中层图号 → `mid_img` |
/// | `fr` | `foreground.image as u16` | `Foreground.Image` | 物体图号 → `fr_img` |
/// | `bk_lib` | `back_library as u8` | `BackLibrary` | 地表库号 → `tiles`（`btTiles`） |
/// | `mid_lib` | `middle_library as u8` | `MiddleLibrary` | 中层库号 → `sm_tiles`（`btsmTiles`） |
/// | `fr_lib` | `foreground.library as u8` | `Foreground.Library` | 物体库号 → `area`（`btArea`） |
/// | `door` | `door` | `Door` | 门索引 → `door_index` |
/// | `door_offset` | `door_offset` | `DoorOffset` | 门偏移 → `door_offset` |
/// | `light` | `light` | `Light` | 光照 → `light` |
/// | `ani_frame` | `foreground.frames` | `Foreground.Frames` | 前景动画帧 → `ani_frame` |
/// | `ani_tick` | `foreground.tick` | `Foreground.Tick` | 前景动画节拍 → `ani_tick` |
/// | `mid_ani_frame` | `middle_frames` | `MiddleFrames` | **视图无字段**（`convert.rs` §五） |
/// | `mid_ani_tick` | `middle_tick` | `MiddleTick` | **视图无字段** |
/// | `tile_animation` | `tile_animation` | `TileAnimation` | **视图无字段**（视图的 `tiles` 是 `btTiles`） |
/// | `tile_offset` | `tile_offset` | `TileOffset` | **视图无字段**（视图的 `sm_tiles` 是 `btsmTiles`） |
/// | `tile_frames` | `tile_frames` | `TileFrames` | **视图无字段** |
///
/// 「视图无字段」的五个字段**不是静默丢弃**：有非 0 值时 [`plan_stamp`] 会把它们折成一句中文说明
/// （`禁假数据`：不许让用户以为搬过去了）。它们之所以搬不动，是因为
/// `mir2_map::Cell`（`mir2-map/src/cell.rs:176-202`）只建模 12/14/36 三档的字节，
/// 而这三档里**没有**中层动画与地表动画的位置（`convert.rs` 的 `LOST_IN_VIEW` 逐条列了原因）。
///
/// ⚠ **本 crate 不解释** `back_image` 的高位（C# 的 `0x60000000` 阻挡位）：
/// `bk` 只承载**低 16 位**，与 `mir2_map::Cell::bk_img` 同宽；`bit15` 的语义由
/// [`crate::paint`] 的保留矩阵决定（见模块文档 §一 第 4 条）。
///
/// ⚠ **加字段会打断外部构造方**（`miredit-gui/src/stamp_place.rs`、
/// `miredit-web/src/routes/stamps.rs` 逐字段列出）。本结构 `#[derive(Default)]`，
/// 那些地方补一个 `..Default::default()` 即可继续编译（新增字段一律缺省 0 = "该字段没有内容"，
/// 与 C# `MergeStamp` 的 `if(a.Door!=0)` 判据同口径 ⇒ 缺省值不会写坏任何格）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StampCellInput {
    /// 相对锚点的横向偏移（可负）。
    pub dx: i32,
    /// 相对锚点的纵向偏移（可负）。
    pub dy: i32,
    /// 地表图号（0 = 该族没有内容 ⇒ 地表不写）。
    pub bk: u16,
    /// 中层图号（0 = 该族没有内容 ⇒ 中层不写）。
    pub mid: u16,
    /// 物体图号（0 = 该族没有内容 ⇒ 物体不写）。
    pub fr: u16,
    /// 地表库号字节（`btTiles`）。
    pub bk_lib: u8,
    /// 中层库号字节（`btsmTiles`）。
    pub mid_lib: u8,
    /// 物体库号字节（`btArea`）。
    pub fr_lib: u8,
    /// 门索引（C# `Door`）。**非 0 才搬**，且搬时连 [`Self::door_offset`] 一起搬
    /// —— `PlacedStamps.cs:93` 的 `if(a.Door!=0){c.Door=a.Door;c.DoorOffset=a.DoorOffset;}`。
    pub door: u8,
    /// 门偏移（C# `DoorOffset`）。**只在 [`Self::door`] 非 0 时**跟着搬（同上）。
    pub door_offset: u8,
    /// 光照（C# `Light`）。非 0 才搬 —— `PlacedStamps.cs:94` 的 `if(a.Light!=0)c.Light=a.Light;`。
    pub light: u8,
    /// 前景动画帧（C# `Foreground.Frames`）。物体图号非 0 时才搬 —— `PlacedStamps.cs:92`
    /// 是 `c.Foreground = front` 整体赋值，`Frames`/`Tick` 是 `Front` 的成员。
    pub ani_frame: u8,
    /// 前景动画节拍（C# `Foreground.Tick`）。同 [`Self::ani_frame`]。
    pub ani_tick: u8,
    /// 中层动画帧（C# `MiddleFrames`，`PlacedStamps.cs:96`）。**视图装不下** ⇒ 只报不写。
    pub mid_ani_frame: u8,
    /// 中层动画节拍（C# `MiddleTick`，`PlacedStamps.cs:96`）。**视图装不下** ⇒ 只报不写。
    pub mid_ani_tick: u8,
    /// 地表动画图号（C# `TileAnimation`，`PlacedStamps.cs:95`）。**视图装不下** ⇒ 只报不写。
    ///
    /// ⚠ 别与 [`Self::bk_lib`] 混：`bk_lib` 落的是 `btTiles`（地表**库号**）；
    /// `mir2_format::Cell::tiles` 这个名字在 14/36 档里指 `btTiles`、在 `CrystalCs` 里才指
    /// `TileAnimationImage`（`mir2-format/src/model.rs:276-280` 的双重身份）。
    pub tile_animation: u16,
    /// 地表动画偏移（C# `TileOffset`，`PlacedStamps.cs:95`）。**视图装不下** ⇒ 只报不写。
    pub tile_offset: u16,
    /// 地表动画帧数（C# `TileFrames`，`PlacedStamps.cs:95`）。**视图装不下** ⇒ 只报不写。
    pub tile_frames: u8,
}

/// 单枚印章的**格数上限 1,000,000**。
///
/// 来源：`CrystalStudio/CrystalStudio/Document.cs:140`
/// （`if (list.Count > 1_000_000) throw new InvalidDataException("Invalid object cell count.");`），
/// 与 `mir2-stamp` 的 `MAX_CELLS` 同口径（那边是同一个数：`mir2-stamp/src/cell.rs:169`）。
///
/// 用途：注册印章时拒绝超大格表（一格里 16 字节字段 ⇒ 1e6 格 ≈ 20 MB，
/// 没有上限时"每次点素材面板都注册一遍"会把内存吃穿）。
pub const STAMP_MAX_CELLS: usize = 1_000_000;

/// 印章**注册表**的 id 上限 10,000（本仓护栏，**不是**原版限制）。
///
/// 依据：本机印章目录实有 9,170 个 `.X`（`mir2-stamp` 的 catalogue 口径），
/// 10,000 足以装下整套目录；再多就说明调用方在拿"每次注册一个新 id"当缓存用，
/// 那会无界吃内存（见 [`STAMP_MAX_CELLS`] 的算例）。
pub const STAMP_REGISTRY_MAX: usize = 10_000;

/// 一次印章放置的结果。
///
/// 拒绝**必须**带中文原因（任务书 §五「禁假数据：拒绝要带明确中文原因，不许静默跳过」）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlaceOutcome {
    /// 放成了：`cells` 格真的改了（= 一步撤销里的格数），`skipped` 格越界被跳过。
    Placed {
        /// 真正改动的格数（去重后）。
        cells: usize,
        /// 越界被跳过的格数。
        skipped: usize,
    },
    /// 拒绝：**一格都没写**，原因在中文字符串里（格式不支持 / 未开图 / 全部越界 / 印章为空）。
    Refused {
        /// 中文原因（会同时写进 [`crate::EditSession::note`]，状态栏能直接显示）。
        reason: String,
    },
}

impl PlaceOutcome {
    /// 真正改了几格（`Refused` ⇒ 0）。
    pub const fn cells(&self) -> usize {
        match self {
            PlaceOutcome::Placed { cells, .. } => *cells,
            PlaceOutcome::Refused { .. } => 0,
        }
    }

    /// 越界跳过了几格（`Refused` ⇒ 0）。
    pub const fn skipped(&self) -> usize {
        match self {
            PlaceOutcome::Placed { skipped, .. } => *skipped,
            PlaceOutcome::Refused { .. } => 0,
        }
    }

    /// 拒绝原因（放成 ⇒ `None`）。
    pub fn refused(&self) -> Option<&str> {
        match self {
            PlaceOutcome::Placed { .. } => None,
            PlaceOutcome::Refused { reason } => Some(reason.as_str()),
        }
    }
}

/// 一次放置的**计算结果**（纯函数产物；会话据此写盘 + 记一步撤销）。
///
/// 为什么要"先算后写"：与 [`crate::fill`] 的 `collect_region` 同一个理由 ——
/// 拒绝（全越界 / 格式不支持）必须发生在**动笔之前**，这样"一格不写"是类型层面
/// 保证的，不靠调用方记得先检查。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StampPlan {
    /// 去重后的变更（同一格只出现一次：`before` = 最初值、`after` = 最终值）。
    pub(crate) changes: Vec<Change>,
    /// 越界被跳过的格数。
    pub(crate) skipped: usize,
    /// 需要说给用户听的中文说明（库号字节被忽略 / 三族全 0 没改）。
    pub(crate) notes: Vec<String>,
}

/// 计算口径：**印章放置** vs **剪贴板粘贴**（C# 里是两个不同函数，前置条件也不同）。
///
/// | 口径 | C# 出处 | 占用闸门 | 坐标重复闸门 |
/// |---|---|---|---|
/// | [`PlanMode::Stamp`] | `PlacedStamps.cs:52-68`（`CanPlaceStamp`）+ `:114-121`（`PlaceStamp`） | **有** | **有** |
/// | [`PlanMode::Paste`] | `RegionCopy.cs:71-91`（`CanPaste` + `Paste`） | 无（`CanPaste` 只判边界） | 无（矩形展开，坐标天然唯一） |
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PlanMode {
    /// 印章放置：套 `CanPlaceStamp` 的**占用闸门**与**坐标重复闸门**（`PlacedStamps.cs:58-67`）。
    Stamp,
    /// 剪贴板粘贴：**不套**这两道闸门（`RegionCopy.Paste` 没有它们）。
    Paste,
}

/// 把「一枚印章 + 锚点」算成一份可执行计划；`Err(中文原因)` = 拒绝（一格不写）。
///
/// 判据顺序（早退顺序，也是原因文案的优先级）：
/// ① 未开图（宽或高为 0）→ ② 印章为空 → ③ 视图形状与头部不符 →
/// ④ Type5 且印章有 `bk != 0` → ⑤ **坐标重复**（仅 [`PlanMode::Stamp`]）→
/// ⑥ 逐格算（越界计入 `skipped`；**落点已有物件 ⇒ 整枚拒绝**，仅 [`PlanMode::Stamp`]）→
/// ⑦ 全部越界 ⇒ 拒绝。
///
/// # 参数
///
/// - `view`：编辑/渲染视图（`mir2_map::Map`）。**只读** —— 本函数不改它；
/// - `id`：字节层格式（决定地表闸门与库号字节闸门，见 `convert.rs`）；
/// - `x` / `y`：锚点（`u16`，世界格）；
/// - `cells`：印章格表（相对偏移 + 三族 + 附加字段）；
/// - `mode`：计算口径（见 [`PlanMode`]）。
///
/// ⚠ **拒绝时一格不写**是类型层面保证的：本函数只往 `changes` 里攒，绝不碰 `view`。
pub(crate) fn plan_stamp(
    view: &Map,
    id: FormatId,
    x: u16,
    y: u16,
    cells: &[StampCellInput],
    mode: PlanMode,
) -> Result<StampPlan, String> {
    let (w, h) = (view.width, view.height);
    // ① 未开图：正常路径下 Document 一定有宽高（解析成功即由头部给出），
    //    这是防御性闸门 —— 但一旦成立就必须明确拒绝，绝不能按 0 尺寸去算下标。
    if w == 0 || h == 0 {
        return Err(format!(
            "未开图：当前文档的宽×高 = {w}×{h}，没有可落笔的格 —— 印章未放置，一个字节都没动"
        ));
    }
    // ② 空印章：没有格就没有"全部越界"可言，单独说清楚
    if cells.is_empty() {
        return Err("印章没有任何格（cells 为空），没有可放置的内容 —— 地图一个字节都没动".to_string());
    }
    // ③ 形状自洽：视图格数必须正好 w×h（否则 `cell_at` 会给 None，越界/缺格会被混为一谈）
    let 应格数 = w as usize * h as usize;
    if view.cells.len() != 应格数 {
        return Err(format!(
            "文档视图与头部声明不一致：{w}×{h} 应为 {应格数} 格，实际 {} 格 —— 拒绝放置",
            view.cells.len()
        ));
    }
    // ④ 格式能力闸门（Type5）：背图在块级，逐格写 bk_img 会被写回静默丢弃
    //    ⇒ 只要印章里有 bk≠0 的格就拒绝（bk=0 的格本来就不写地表，无害）。
    if !ground_paint_supported(id) {
        let 有地表 = cells.iter().filter(|c| c.bk != 0).count();
        if 有地表 > 0 {
            return Err(format!(
                "{} 不支持逐格写地表背图（Type5 的背图在 4 格块里：半分辨率 3 B / 2×2，\
                 Cell::bk_img 恒 0 且写回时不参与编码）—— 这枚印章有 {有地表} 格 bk≠0，\
                 已拒绝放置，地图一个字节都没动",
                format_by_id(id).name()
            ));
        }
    }

    // ⑤ 【印章专用】坐标重复闸门（`PlacedStamps.cs:65-66`）：
    //    `var visited=new HashSet<(int,int)>(); foreach(var s in cells)if(!visited.Add((s.X,s.Y)))return false;`
    //    ⇒ 印章**自身**的 `(dx,dy)` 出现两次 ⇒ 整枚拒绝（**不是**"后者胜"）。
    //    粘贴路径不走这条：`RegionCopy.Paste` 是矩形展开（`RegionCopy.cs:75-76` 的双层循环），
    //    `(dx,dy)` 天然唯一，C# 那边根本没有这个概念。
    if mode == PlanMode::Stamp {
        let mut 见过: HashSet<(i32, i32)> = HashSet::with_capacity(cells.len());
        for c in cells {
            if !见过.insert((c.dx, c.dy)) {
                return Err(format!(
                    "印章自身的坐标重复：相对偏移 ({},{}) 在格表里出现了不止一次\
                     （同 C# PlacedStamps.cs:65-66 的 visited 判据）—— 整枚拒绝，地图一个字节都没动",
                    c.dx, c.dy
                ));
            }
        }
    }

    let (w64, h64) = (i64::from(w), i64::from(h));
    let fmt = view.format;
    // 同一格被写多次时：`待定` 保存"当前有效值"（后者覆盖前者），
    // `位置` 记住它在 `changes` 里的下标（只更新 `after`，`before` 永远是最初值）。
    // ⚠ 印章路径已由上面的重复闸门保证"一格只出现一次" ⇒ 这两张表在那里只作防御；
    //    真正用到"后者胜"的是粘贴路径（调用方可以拿 `Clipboard::from_cells` 传重复格）。
    let mut 待定: HashMap<(u16, u16), Cell> = HashMap::new();
    let mut 位置: HashMap<(u16, u16), usize> = HashMap::new();
    let mut changes: Vec<Change> = Vec::new();
    let mut skipped = 0usize;
    let mut 丢库号: Vec<&'static str> = Vec::new();
    let mut 丢动画: Vec<&'static str> = Vec::new();

    for c in cells {
        let sx = i64::from(x) + i64::from(c.dx);
        let sy = i64::from(y) + i64::from(c.dy);
        // 越界裁剪：逐格跳过（`PlacedStamps.cs:80` 的 `MapBoundary` + `continue`）
        if sx < 0 || sy < 0 || sx >= w64 || sy >= h64 {
            skipped += 1;
            continue;
        }
        let (cx, cy) = (sx as u16, sy as u16);
        let Some(地图原格) = cell_at(view, cx, cy).copied() else {
            // ③ 的形状校验已保证不可达；真到了这里也只能算"跳过"（绝不静默写别的格）
            skipped += 1;
            continue;
        };
        // ⑤b 【印章专用】占用闸门（`PlacedStamps.cs:62-63`）：
        //     `var c=Get((int)sx,(int)sy); if(c.HasForeground)return false;`
        //     `HasForeground`（`Document.cs:26`）= `Foreground is {} front && (front.Image & 0x7FFF) != 0`
        //     ⇒ 本 crate 的等价判据 = `fr_img & 0x7FFF != 0`（图号 0 = 没有前景）。
        //     ⚠ 判据必须取**地图原值**（C# 的 `Get`），不是本函数攒的 `待定`：
        //        C# 的 `CanPlaceStamp` 在**任何 Set 之前**跑完。
        if mode == PlanMode::Stamp && (地图原格.fr_img & SPRITE_MAX) != 0 {
            return Err(format!(
                "落点 ({cx},{cy}) 已有物件（物体图号 = {}），印章未放置 —— 请先清除该格的物件，\
                 或换一个落点；地图一个字节都没动",
                地图原格.fr_img & SPRITE_MAX
            ));
        }
        let before = 待定.get(&(cx, cy)).copied().unwrap_or(地图原格);
        let mut after = before;
        // 三族：图号 0 = 该族没有内容 ⇒ 整族不写（含库号字节），
        // 否则"贴一枚没有地表的印章"会顺手把目标图的地表清成 0（`Document.cs:31` 的 Empty 口径）。
        for (layer, img, lib) in [
            (EditLayer::Ground, c.bk, c.bk_lib),
            (EditLayer::Middle, c.mid, c.mid_lib),
            (EditLayer::Front, c.fr, c.fr_lib),
        ] {
            if img == 0 {
                continue;
            }
            after = 写一族(fmt, id, after, layer, img, lib, &mut 丢库号);
        }
        // 三族之外的附加字段：**两条路径各有自己的一份 C# 字段表**，不能共用。
        //
        // | 路径 | C# 出处 | 搬什么 |
        // |---|---|---|
        // | 印章 | `PlacedStamps.cs:87-98`（`MergeStamp`） | 门/门偏移（`if(a.Door!=0)`）、光照（`if(a.Light!=0)`）、前景动画（`fr!=0`） |
        // | 粘贴 | `RegionCopy.cs:84-86`（`Paste`） | 门/门偏移 + 前景动画（**有前景就无条件赋值**）、**完全不碰光照** |
        //
        // ⚠ 之所以不能共用：`MergeStamp` 对门是"非 0 才搬"，而 `Paste` 是
        // `target.Door = source.Door` **无条件赋值**（源 0 会把目标的门清 0）；
        // 反过来 `Paste` 里**没有** `Light` 这一行。共用一份实现必然有一边是错的。
        match mode {
            PlanMode::Stamp => 合并附加字段(&mut after, c, &mut 丢动画),
            PlanMode::Paste => 合并粘贴附加字段(&mut after, c, &mut 丢动画),
        }
        待定.insert((cx, cy), after);
        if after == before {
            continue; // 这一格的有效值没变（值本来就相同）
        }
        match 位置.get(&(cx, cy)) {
            // 同一格第 2 次出现：只把 `after` 换成后者的结果（`before` 保持最初值）
            Some(&i) => changes[i].after = after,
            None => {
                位置.insert((cx, cy), changes.len());
                changes.push(Change {
                    x: cx,
                    y: cy,
                    before,
                    after,
                });
            }
        }
    }

    // 同一格被写多次、最终值回到原值 ⇒ 这一格在这一步里根本没变，不许进撤销步
    changes.retain(|c| c.after != c.before);

    let 图内 = cells.len() - skipped;
    // ⑦ 全部越界 ⇒ 拒绝（一格不写）
    if 图内 == 0 {
        return Err(format!(
            "全部 {} 格都在地图外（锚点 ({x},{y})，地图 {w}×{h}），一格未写",
            cells.len()
        ));
    }

    let mut notes: Vec<String> = Vec::new();
    if !丢库号.is_empty() {
        notes.push(format!(
            "{} 里 {} 没有独立的库号字节（该格式的格布局里这一层不带库号）—— \
             该字节已忽略，只写图号",
            format_by_id(id).name(),
            丢库号.join(" / ")
        ));
    }
    if !丢动画.is_empty() {
        notes.push(format!(
            "{} 的编辑视图里没有 {} 的独立字段（`convert.rs` 的 LOST_IN_VIEW）—— \
             这些字段未随印章搬运，目标格原值保留",
            丢动画.join(" / "),
            format_by_id(id).name()
        ));
    }
    if changes.is_empty() {
        notes.push(format!(
            "印章在图内的 {图内} 格里三族图号都是 0（或与目标格内容相同），没有需要改的字段"
        ));
    }
    Ok(StampPlan {
        changes,
        skipped,
        notes,
    })
}

/// 把印章格里**三族图号之外**的字段按 C# `MergeStamp`（`PlacedStamps.cs:87-98`）搬过去。
///
/// 逐条判据（与 C# 一字对应）：
///
/// | C# | 源码 | 本函数 |
/// |---|---|---|
/// | `if(a.Door!=0){c.Door=a.Door;c.DoorOffset=a.DoorOffset;}` | `:93` | `door != 0` ⇒ 门索引 + 门偏移**一起**搬（门偏移单独非 0 不搬） |
/// | `if(a.Light!=0)c.Light=a.Light;` | `:94` | `light != 0` ⇒ 搬 |
/// | `if (a.Foreground is { } front) c.Foreground = front;` | `:92` | `fr != 0` ⇒ 前景图号（由 [`写一族`] 搬）+ **`Frames`/`Tick`**（本函数搬） |
///
/// # 搬不动的 5 个字段（**不是静默丢弃**）
///
/// `MiddleFrames`（`:96`）、`MiddleTick`（`:96`）、`TileAnimation`（`:95`）、
/// `TileOffset`（`:95`）、`TileFrames`（`:95`）在 `mir2_map::Cell`（`mir2-map/src/cell.rs:176-202`）
/// 里**没有对应字段** —— 它只建模 12/14/36 三档的字节，而这三档里没有中层动画与地表动画的位置
/// （`convert.rs` 的 `LOST_IN_VIEW` 逐条列了原因；`mir2-format/src/formats/shanda2012.rs:71`
/// 也明说 `tile_frames` 在该格式**恒 0**）。⇒ 有非 0 值时记进 `丢动画`，
/// 由 [`plan_stamp`] 折成一句中文说明交给用户，**绝不假装搬了**。
fn 合并附加字段(after: &mut Cell, c: &StampCellInput, 丢动画: &mut Vec<&'static str>) {
    // `PlacedStamps.cs:93`
    if c.door != 0 {
        after.door_index = c.door;
        after.door_offset = c.door_offset;
    }
    // `PlacedStamps.cs:94`
    if c.light != 0 {
        after.light = c.light;
    }
    // `PlacedStamps.cs:92`：`c.Foreground = front` 是**整体赋值**，`Frames`/`Tick` 是 `Front` 的成员
    // ⇒ 只在"有前景"（本 crate 的等价判据 `fr != 0`）时才搬这两个字节。
    if c.fr != 0 {
        after.ani_frame = c.ani_frame;
        after.ani_tick = c.ani_tick;
    }
    // 视图装不下的 5 个：只报不写
    记丢动画(c, 丢动画);
}

/// 把剪贴板格里**三族图号之外**的字段按 C# `RegionCopy.Paste`（`RegionCopy.cs:84-86`）搬过去。
///
/// 与 [`合并附加字段`]（印章的 `MergeStamp`）的**两处不同**，逐条对应源码：
///
/// | 字段 | `MergeStamp`（印章） | `RegionCopy.Paste`（粘贴） |
/// |---|---|---|
/// | 门索引 / 门偏移 | `if(a.Door!=0)` 才搬（`PlacedStamps.cs:93`） | **有前景就无条件赋值**（`RegionCopy.cs:86` 的 `target.Door = source.Door`）⇒ 源门为 0 会把目标的门清成 0 |
/// | 光照 | `if(a.Light!=0)` 搬（`PlacedStamps.cs:94`） | **完全不碰**（`RegionCopy.cs:84-86` 里没有 `Light`） |
/// | 前景动画帧/节拍 | `fr != 0` 时搬（`:92`） | 同（`target.Foreground = source.Foreground` 整体赋值） |
///
/// 视图装不下的 5 个字段与印章路径**同一口径**：只报不写（`Cell::tail` 语义未知，
/// 不按猜测切三个字节出来）。
fn 合并粘贴附加字段(after: &mut Cell, c: &StampCellInput, 丢动画: &mut Vec<&'static str>) {
    if c.fr != 0 {
        // `RegionCopy.cs:86`：`if (pasteObjects) { target.Foreground = source.Foreground;
        // target.Door = source.Door; target.DoorOffset = source.DoorOffset; }`
        after.door_index = c.door;
        after.door_offset = c.door_offset;
        after.ani_frame = c.ani_frame;
        after.ani_tick = c.ani_tick;
    }
    记丢动画(c, 丢动画);
}

/// 把「视图里装不下的 5 个动画字段」按**有无值**记进 `丢动画`（去重，供 [`plan_stamp`] 折成中文说明）。
///
/// 两条路径（`MergeStamp` 的 `:95-96`、`RegionCopy.Paste` 的 `:84-85`）都会搬这 5 个，
/// 但本 crate 的编辑视图里**没有它们的字节**（`convert.rs` 的 `LOST_IN_VIEW`）⇒ 只能如实报。
fn 记丢动画(c: &StampCellInput, 丢动画: &mut Vec<&'static str>) {
    for (name, 有值) in [
        ("中层动画帧（C# MiddleFrames）", c.mid_ani_frame != 0),
        ("中层动画节拍（C# MiddleTick）", c.mid_ani_tick != 0),
        ("地表动画图号（C# TileAnimation）", c.tile_animation != 0),
        ("地表动画偏移（C# TileOffset）", c.tile_offset != 0),
        ("地表动画帧数（C# TileFrames）", c.tile_frames != 0),
    ] {
        if 有值 && !丢动画.contains(&name) {
            丢动画.push(name);
        }
    }
}

/// 写**一族**（图号 + 该层库号字节），其余字段原样带过去。
///
/// 复用 [`paint`]：`bit15` 保留矩阵（地表/物体保留、中层不保留）都是**同一份实现**，
/// 本函数绝不另写一份位运算（任务书 §三.3 的硬要求）。
///
/// # 库号字节：判据是 **writer 写不写**，不是 `has_lib_bytes`
///
/// `paint` 用 `CellFormat::has_lib_bytes()`（`paint.rs:296`）当闸门，那问的是
/// 「12/14/36 三档里有没有 `btTiles`/`btsmTiles`」。但 `btArea`（物体库号）**在 12 B 档里是有的**
/// —— `mir2-format/src/formats/old12.rs:158` 的 `out.push(cell.area)` 就是它。
/// ⇒ 本函数按 [`writes_layer_lib_byte`]（writer 实际写不写）**显式补写**一次，
/// 否则 Old12 的物体库号会出现"视图里看着改了、文件里没动"。
///
/// 写不进去的层记进 `dropped`，由 [`plan_stamp`] 折成一句中文说明（禁静默）。
fn 写一族(
    fmt: CellFormat,
    id: FormatId,
    cell: Cell,
    layer: EditLayer,
    img: u16,
    lib: u8,
    dropped: &mut Vec<&'static str>,
) -> Cell {
    // 印章只写三族图号；阻挡层不在本函数职责内（它们只改 bit15）
    let Some(family) = layer.family() else {
        return cell;
    };
    let brush = Brush {
        family,
        img,
        lib_byte: lib,
        lib: None,
        from: None,
    };
    let mut after = match paint(fmt, cell, Tool::Brush, Some(brush), layer) {
        PaintOutcome::Write(after) => after,
        // 族取自本层、层又是图号层 ⇒ `LayerMismatch` / `PaletteBrushOnCollision` / `NoOp`
        // 三条分支都不可达；真到了这里就保守地不改这一格（宁可少写，也不乱写）
        _ => return cell,
    };
    // ① writer 真的写这个字节 ⇒ **显式写**（含"写成 0"，同 `PlacedStamps.cs:90` 的
    //    `c.BackLibrary=a.BackLibrary` 无条件赋值口径）。不能只靠 `paint`：它在
    //    `has_lib_bytes=false` 的档位里压根不写（`paint.rs:296`），而 `area` 在 12 B 档是有位置的。
    if writes_layer_lib_byte(id, layer) {
        layer.set_lib_byte(&mut after, lib);
    } else if lib != 0 && !dropped.contains(&layer.name()) {
        // ② writer 不写这个字节 ⇒ 记下来告诉用户（该层在本格式里没有独立库号字节）
        dropped.push(layer.name());
    }
    // ③ 兜底层：`paint` 按 `CellFormat::has_lib_bytes()` 写过，但该格式 writer 不写
    //    ⇒ 还原成改前值（否则用户会看到"库号改了、文件没变"）
    restore_unwritten_lib_byte(id, layer, &cell, &mut after);
    after
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::造图;
    use mir2_map::CellFormat;

    /// 造一张 8×8 的合成图（每格内容可区分：图号 = 格序 + 1、中层 +100、物体 +200）。
    ///
    /// ⚠ **物体层被清空**（2026-11-05 订正）：`plan_stamp` 的占用闸门
    /// （`PlacedStamps.cs:62-63`，见模块文档 §一 第 9 条）要求落点格**没有物件**
    /// （`fr_img & 0x7FFF == 0`），而 [`造图`] 夹具每格都带物体图号（`testutil.rs:27`）
    /// ⇒ 不清空的话，本模块**每一条**放置用例都会被整枚拒绝，
    /// 测到的就不再是"三族/库号/附加字段怎么搬"，而是"闸门怎么拒"。
    ///
    /// 只清 `fr_img`：地表/中层/门/光照/库号保持夹具原值，`bit15` 由各用例自己摆。
    fn 图() -> Map {
        let mut map = 造图(8, 8, CellFormat::New14);
        for c in &mut map.cells {
            c.fr_img = 0;
        }
        map
    }

    /// 造一格**只带三族内容**的印章格（门 / 光照 / 前景动画 / 地表动画一律 0）。
    ///
    /// 为什么把 10 个附加字段逐个写 0 而不用 `..Default::default()`：本模块的用例要能
    /// 一眼看出「这些字段在夹具里确实是 0」——`MergeStamp`（`PlacedStamps.cs:93-96`）的
    /// 判据是 `!= 0` 才搬，0 表示**该格没有这些内容**，不是"忘了填"。
    #[allow(clippy::too_many_arguments)]
    fn 格(
        dx: i32,
        dy: i32,
        bk: u16,
        mid: u16,
        fr: u16,
        bk_lib: u8,
        mid_lib: u8,
        fr_lib: u8,
    ) -> StampCellInput {
        StampCellInput {
            dx,
            dy,
            bk,
            mid,
            fr,
            bk_lib,
            mid_lib,
            fr_lib,
            door: 0,
            door_offset: 0,
            light: 0,
            ani_frame: 0,
            ani_tick: 0,
            mid_ani_frame: 0,
            mid_ani_tick: 0,
            tile_animation: 0,
            tile_offset: 0,
            tile_frames: 0,
        }
    }

    /// 一枚 2×2 的小印章：三族都给非 0 值，库号也非 0。
    fn 小印章() -> Vec<StampCellInput> {
        vec![
            格(0, 0, 11, 12, 13, 21, 22, 23),
            格(1, 0, 14, 15, 16, 24, 25, 26),
            格(0, 1, 17, 18, 19, 27, 28, 29),
            格(1, 1, 20, 21, 22, 30, 31, 32),
        ]
    }

    /// 正常放置：4 格都进 `changes`，三族图号与三个库号字节都写进去了。
    #[test]
    fn 三族图号与库号都写() {
        let map = 图();
        let plan = plan_stamp(&map, FormatId::Shanda14, 3, 3, &小印章(), PlanMode::Stamp).expect("应能放置");
        assert_eq!(plan.changes.len(), 4, "2×2 印章 = 4 格");
        assert_eq!(plan.skipped, 0);
        assert!(plan.notes.is_empty(), "Shanda14 三个库号字节都写 ⇒ 不该有告警：{:?}", plan.notes);
        let c = plan.changes.iter().find(|c| (c.x, c.y) == (3, 3)).expect("(3,3)");
        assert_eq!(c.after.bk_img, 11);
        assert_eq!(c.after.mid_img, 12);
        assert_eq!(c.after.fr_img, 13);
        assert_eq!(c.after.tiles, 21, "地表库号 → btTiles");
        assert_eq!(c.after.sm_tiles, 22, "中层库号 → btsmTiles");
        assert_eq!(c.after.area, 23, "物体库号 → btArea");
        // 其余字段一律不动（门索引 / 光照 / 尾区）
        assert_eq!(c.after.door_index, c.before.door_index);
        assert_eq!(c.after.light, c.before.light);
        assert_eq!(c.after.tail, c.before.tail);
    }

    /// **`bit15` 保留矩阵**（任务书 §三.3）：地表/物体保留、中层不保留 —— 与 `paint()` 同一套。
    #[test]
    fn 三族bit15保留矩阵() {
        let mut map = 图();
        // 目标格三族都带 bit15
        let i = crate::document::index_of(&map, 2, 2).unwrap();
        map.cells[i].bk_img = 0x8000 | 0x1234;
        map.cells[i].mid_img = 0x8000 | 0x5678;
        // ⚠ 物体层**只留 bit15、低 15 位必须为 0**：占用闸门（`PlacedStamps.cs:62-63`）按
        // `fr_img & 0x7FFF != 0` 判"落点已有物件"，带 `0x9ABC` 会被整枚拒绝，
        // 那就测不到"贴上去之后 bit15 还在不在"了。
        map.cells[i].fr_img = 0x8000;
        let 一 = vec![StampCellInput { dx: 0, dy: 0, bk: 7, mid: 8, fr: 9, ..Default::default() }];
        let plan = plan_stamp(&map, FormatId::Shanda14, 2, 2, &一, PlanMode::Stamp).expect("应能放置");
        let c = &plan.changes[0];
        assert_eq!(c.after.bk_img, 0x8007, "地表保留 bit15（低阻挡位不许被贴掉）");
        assert_eq!(c.after.mid_img, 0x0008, "中层不保留 bit15（纯图号）");
        assert_eq!(c.after.fr_img, 0x8009, "物体保留 bit15（高阻挡位不许被贴掉）");
    }

    /// 图号 0 = 该族没有内容 ⇒ 整族不写（含库号字节），目标格原值一点不动。
    #[test]
    fn 图号零的族不写() {
        let mut map = 图();
        let i = crate::document::index_of(&map, 4, 4).unwrap();
        // 目标格物体层只留 bit15（低 15 位为 0）⇒ 既过占用闸门，又能证明"物体图号 0 时
        // 目标格物体层一个位都不许动"（`原.fr_img = 0x8000` 非 0，不是"两边都是 0"的空断言）。
        map.cells[i].fr_img = 0x8000;
        let 原 = map.cells[i];
        // 只给中层：地表/物体的图号与库号都是 0
        let 一 = vec![StampCellInput { dx: 0, dy: 0, mid: 42, mid_lib: 9, ..Default::default() }];
        let plan = plan_stamp(&map, FormatId::Shanda14, 4, 4, &一, PlanMode::Stamp).expect("应能放置");
        let c = &plan.changes[0];
        assert_eq!(c.after.mid_img, 42);
        assert_eq!(c.after.sm_tiles, 9);
        assert_eq!(c.after.bk_img, 原.bk_img, "地表图号 0 ⇒ 不清目标格地表");
        assert_eq!(c.after.tiles, 原.tiles, "地表库号 0 ⇒ 不动 btTiles");
        assert_eq!(c.after.fr_img, 原.fr_img, "物体图号 0 ⇒ 不清目标格物体");
        assert_eq!(c.after.area, 原.area);
        // 三族全 0 ⇒ 一格都不改，并给一句中文说明
        let 零 = vec![StampCellInput { dx: 0, dy: 0, ..Default::default() }];
        let plan = plan_stamp(&map, FormatId::Shanda14, 5, 5, &零, PlanMode::Stamp).expect("不该拒绝");
        assert!(plan.changes.is_empty(), "三族全 0 ⇒ 没有要改的字段");
        assert_eq!(plan.skipped, 0);
        assert_eq!(plan.notes.len(), 1);
        assert!(plan.notes[0].contains("三族图号都是 0"), "{:?}", plan.notes);
    }

    /// 越界裁剪：逐格跳过并计数；图内的照写。
    #[test]
    fn 越界逐格跳过并计数() {
        let map = 图();
        // 锚点 (0,0) + 2×2 印章 ⇒ (-1,-1)/(-1,0)/(0,-1) 出界，(0,0) 在图内
        let 一 = vec![
            StampCellInput { dx: -1, dy: -1, bk: 1, ..Default::default() },
            StampCellInput { dx: -1, dy: 0, bk: 2, ..Default::default() },
            StampCellInput { dx: 0, dy: -1, bk: 3, ..Default::default() },
            StampCellInput { dx: 0, dy: 0, bk: 4, ..Default::default() },
        ];
        let plan = plan_stamp(&map, FormatId::Shanda14, 0, 0, &一, PlanMode::Stamp).expect("应能放置");
        assert_eq!(plan.skipped, 3, "3 格出界");
        assert_eq!(plan.changes.len(), 1, "只有 (0,0) 在图内");
        assert_eq!((plan.changes[0].x, plan.changes[0].y), (0, 0));
        assert_eq!(plan.changes[0].after.bk_img, 4);
    }

    /// 全部越界 ⇒ 拒绝，且**一格不写**（`changes` 都不该存在）。
    #[test]
    fn 全越界拒绝() {
        let map = 图();
        let 一 = vec![
            StampCellInput { dx: 0, dy: 0, bk: 1, ..Default::default() },
            StampCellInput { dx: 1, dy: 1, bk: 2, ..Default::default() },
        ];
        let err = plan_stamp(&map, FormatId::Shanda14, 100, 100, &一, PlanMode::Stamp).expect_err("必须拒绝");
        assert!(err.contains("全部 2 格都在地图外"), "{err}");
        assert!(err.contains("一格未写"), "{err}");
    }

    /// 空印章 / 未开图 / 视图形状不符 —— 三种都必须拒绝且原因明确。
    #[test]
    fn 三种拒绝() {
        let map = 图();
        let e = plan_stamp(&map, FormatId::Shanda14, 0, 0, &[], PlanMode::Stamp).expect_err("空印章必须拒绝");
        assert!(e.contains("没有任何格"), "{e}");
        let mut 零尺寸 = map.clone();
        零尺寸.width = 0;
        let e = plan_stamp(&零尺寸, FormatId::Shanda14, 0, 0, &小印章(), PlanMode::Stamp).expect_err("未开图必须拒绝");
        assert!(e.contains("未开图"), "{e}");
        let mut 形状坏 = map.clone();
        形状坏.cells.pop();
        let e = plan_stamp(&形状坏, FormatId::Shanda14, 0, 0, &小印章(), PlanMode::Stamp).expect_err("形状不符必须拒绝");
        assert!(e.contains("不一致"), "{e}");
    }

    /// **Type5 闸门**：有 `bk != 0` ⇒ 拒绝（中文原因点明块级背图）；只有 `bk == 0` ⇒ 放行。
    #[test]
    fn type5有地表就拒绝() {
        let map = 图();
        let e = plan_stamp(&map, FormatId::Mir3Wemade, 1, 1, &小印章(), PlanMode::Stamp).expect_err("Type5 有地表必须拒绝");
        assert!(e.contains("不支持逐格写地表背图"), "{e}");
        assert!(e.contains("bk≠0"), "{e}");
        assert!(e.contains("一个字节都没动"), "{e}");
        // 只有中层/物体（bk 全 0）⇒ Type5 也放行
        let 无地表: Vec<StampCellInput> = 小印章()
            .into_iter()
            .map(|c| StampCellInput { bk: 0, bk_lib: 0, ..c })
            .collect();
        let plan = plan_stamp(&map, FormatId::Mir3Wemade, 1, 1, &无地表, PlanMode::Stamp).expect("无地表应放行");
        assert_eq!(plan.changes.len(), 4);
        assert!(plan.changes.iter().all(|c| c.after.bk_img == c.before.bk_img), "地表一格都不许动");
    }

    /// **库号字节写不进去**：`Mir3Wemade` 只写 `area`，中层的库号必须被忽略并说明。
    ///
    /// ⚠ 这枚印章**不能带地表**（`bk != 0`）：Type5 会被 [`plan_stamp`] 的格式闸门直接拒绝，
    /// 根本走不到库号字节那一步 —— 那正是"两个闸门各管一段"的证据。
    #[test]
    fn 库号写不进去要说明() {
        let map = 图();
        let 无地表: Vec<StampCellInput> = 小印章()
            .into_iter()
            .map(|c| StampCellInput { bk: 0, bk_lib: 0, ..c })
            .collect();
        let plan = plan_stamp(&map, FormatId::Mir3Wemade, 1, 1, &无地表, PlanMode::Stamp).expect("应能放置");
        assert_eq!(plan.notes.len(), 1, "应有一条库号告警：{:?}", plan.notes);
        let n = &plan.notes[0];
        // ⚠ 文案订正（2026-11-05）：旧断言找的是「库号字节没有对应字节」，
        // 但旧文案本身是**假陈述** —— `Old12`/`AntiHack` 的物体层（`area`）是写的
        // （`mir2-format/src/formats/old12.rs:158` 的 `out.push(cell.area)`）。
        // 现在的文案只说准确的那部分：「该层在本格式里没有独立的库号字节」。
        assert!(n.contains("没有独立的库号字节"), "{n}");
        assert!(n.contains("中层图层"), "要点名哪一层：{n}");
        assert!(!n.contains("物体"), "物体层的库号（btArea）该格式是写的，不该点名：{n}");
        // 物体层库号真的写进去了，中层库号保持原值
        let c = &plan.changes[0];
        assert_eq!(c.after.area, 23, "Mir3Wemade 写 btArea");
        assert_eq!(c.after.sm_tiles, c.before.sm_tiles, "中层库号必须还原成改前值");
    }

    /// `Old12` 视图档（无库号字节）⇒ 库号一律忽略并说明；图号照写。
    #[test]
    fn 十二字节档库号忽略() {
        let mut map = 造图(4, 4, CellFormat::Old12);
        // 落点物体层清空 ⇒ 过占用闸门（`PlacedStamps.cs:62-63`）
        let i = crate::document::index_of(&map, 1, 1).unwrap();
        map.cells[i].fr_img = 0;
        let 一 = vec![StampCellInput { dx: 0, dy: 0, bk: 5, bk_lib: 9, ..Default::default() }];
        let plan = plan_stamp(&map, FormatId::Old12, 1, 1, &一, PlanMode::Stamp).expect("应能放置");
        assert_eq!(plan.changes[0].after.bk_img, 5);
        assert_eq!(plan.changes[0].after.tiles, 0, "Old12 档没有库号字节");
        assert_eq!(plan.notes.len(), 1);
        assert!(plan.notes[0].contains("地表图层"), "{:?}", plan.notes);
    }

    /// **粘贴路径的重叠语义**：同一格出现两次 ⇒ 后者胜；撤销里只记**一条**（`before` = 最初值）。
    ///
    /// ⚠ **为什么走 [`PlanMode::Paste`] 而不是 `Stamp`**（2026-11-05 订正）：
    /// 印章路径有坐标重复闸门（`PlacedStamps.cs:65-66`，见模块文档 §一 第 6 条），
    /// `(dx,dy)` 出现两次是**整枚拒绝**，走不到"后者胜"那一步；
    /// 只有粘贴路径（`RegionCopy.cs:75-76` 的矩形展开）才可能喂进重复格
    /// —— 那正是 `待定` / `位置` 两张表存在的理由（`plan_stamp` 里的注释）。
    #[test]
    fn 同格两次后者胜且只记一条() {
        let map = 图();
        let 一 = vec![
            StampCellInput { dx: 0, dy: 0, bk: 100, mid: 0, fr: 0, ..Default::default() },
            StampCellInput { dx: 0, dy: 0, bk: 200, mid: 0, fr: 0, ..Default::default() },
        ];
        let plan =
            plan_stamp(&map, FormatId::Shanda14, 6, 6, &一, PlanMode::Paste).expect("应能放置");
        assert_eq!(plan.changes.len(), 1, "同一格只许一条变更");
        let c = &plan.changes[0];
        assert_eq!(c.after.bk_img, 200, "后者胜");
        let i = crate::document::index_of(&map, 6, 6).unwrap();
        assert_eq!(c.before, map.cells[i], "before 必须是最初值（撤销要能退回原样）");
    }

    /// 同一格写两次又回到原值 ⇒ 这一格**不算改**（不进撤销步）。
    ///
    /// 注意库号字节：`paint` 在写图号时**一定会**把该层库号写成笔刷的 `lib_byte`
    /// （`StampCellInput` 的 `bk_lib = 0` 会把它清成 0，与 C# `MergeStamp` 的
    /// `if(a.BackImage!=0){ c.BackLibrary=a.BackLibrary; … }` 同口径）
    /// ⇒ 本用例把 `bk_lib` 设成目标格原本的 3，否则"抵消"之后仍会剩下库号这一处差异。
    ///
    /// ⚠ **同 [`同格两次后者胜且只记一条`] 的理由**：重复坐标只有粘贴路径才可能，
    /// 印章路径会被坐标重复闸门整枚拒绝。
    #[test]
    fn 两次抵消不算改() {
        let map = 图();
        let i = crate::document::index_of(&map, 1, 1).unwrap();
        let 原图号 = map.cells[i].bk_img & crate::SPRITE_MAX;
        let 原库号 = map.cells[i].tiles;
        let 一 = vec![
            StampCellInput { dx: 0, dy: 0, bk: 999, bk_lib: 原库号, ..Default::default() },
            StampCellInput { dx: 0, dy: 0, bk: 原图号, bk_lib: 原库号, ..Default::default() },
        ];
        let plan =
            plan_stamp(&map, FormatId::Shanda14, 1, 1, &一, PlanMode::Paste).expect("应能放置");
        assert!(plan.changes.is_empty(), "抵消后不该留变更：{:?}", plan.changes.len());
        assert_eq!(plan.skipped, 0);
    }

    /// 负偏移：`dx/dy` 为负时锚点右移 —— 与 `(x+dx, y+dy)` 逐格同口径。
    #[test]
    fn 负偏移按锚点相加() {
        let map = 图();
        let 一 = vec![StampCellInput { dx: -2, dy: -3, bk: 33, ..Default::default() }];
        let plan = plan_stamp(&map, FormatId::Shanda14, 5, 5, &一, PlanMode::Stamp).expect("应能放置");
        assert_eq!((plan.changes[0].x, plan.changes[0].y), (3, 2));
    }
}
