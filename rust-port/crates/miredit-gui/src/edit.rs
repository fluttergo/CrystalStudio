//! 编辑闭环：吸管 / 笔刷 / 橡皮擦 / **填充** / **笔刷尺寸** / 撤销 / 重做 / 脏标记
//! （**纯数据层，不依赖窗口**）。
//!
//! # 〇、本模块现在是一层**薄适配层**（2026-11-05 迁移收口）
//!
//! 编辑逻辑**全部**在 [`miredit_core`] 里：图层 / 笔刷 / 落笔 / 填充 / 撤销 / 原子保存 /
//! 可序列化命令。本文件只做三件事：
//!
//! 1. **类型全部 `pub use` 回来**（[`EditLayer`] / [`Tool`] / [`Brush`] / [`PickedCell`] /
//!    [`PaintOutcome`] / [`FillOutcome`] / [`FillKey`] / [`Change`] / [`Step`] / [`ByteDiff`] /
//!    [`COLLISION_BIT`] / [`SPRITE_MAX`] / [`UNDO_LIMIT`] / [`TOOL_BRUSH_IDX`] 等）——
//!    本文件里**不再有第二份定义**；
//! 2. [`EditSession`] 是一层**包装**：内核会话自己持有文档，而宿主（`app.rs` /
//!    `editrun.rs`）把 `DocState.map` 放在自己手里 ⇒ 每次调用前后用 `std::mem::swap`
//!    把宿主地图换进/换出（O(1)，只换 `Vec` 指针，不拷 5.9 MB）；
//! 3. **保留宿主层的东西**：界面文案（[`lib_text`] / [`brush_lib_text`] / [`dirty_text`] /
//!    [`family_name`]）、脚本驱动（[`EditCmd`] / [`ScriptReport`] / [`apply_script`]）、
//!    诊断打印（[`print_check`]）。
//!
//! 为什么保留 `&mut Map` 参数：`app.rs` / `cli.rs` / `editrun.rs` **一行都不许改**
//! （迁移硬要求）⇒ 本模块的 public 签名**逐字不动**，只在内部换了一条实现。
//!
//! 判据（`bit15` 矩阵 / 填充 BFS / 撤销逐字节）的**真值文档**在 core 各模块的 `//!` 里
//! （`miredit_core::layer` / `fill` / `paint` / `undo` / `save`），本文件不再复述；
//! 下面保留的是"为什么这样切层 / 为什么橡皮保留 `0x8000`"这类**宿主也要知道**的结论。
//!
//! # 一、五个层（地表 / 中层 / 物体 / 低位阻挡 / 高位阻挡）都能编辑
//!
//! **写哪个字段由「当前编辑层」决定，不由笔刷决定**（这是本版最重要的一条）：
//!
//! | 当前编辑层 | 图号字段 | 库号字段（仅 14/36 档） | 笔刷族必须等于 |
//! |---|---|---|---|
//! | 地表图层 [`EditLayer::Ground`] | `bk_img` | `btTiles` | [`Family::Ground`] |
//! | 中层图层 [`EditLayer::Middle`] | `mid_img` | `btsmTiles` | [`Family::Middle`] |
//! | 物体 [`EditLayer::Front`] | `fr_img` | `btArea` | [`Family::Surface`] |
//! | 低位阻挡 [`EditLayer::LowCollision`] | `bk_img` 的 **`bit15`** | **不写** | 不接受图库笔刷 |
//! | 高位阻挡 [`EditLayer::HighCollision`] | `fr_img` 的 **`bit15`** | **不写** | 同上 |
//!
//! | 工具 | 动哪些字段 | 依据 |
//! |---|---|---|
//! | 吸管 [`Tool::Pickup`] | 读**整格三族**（`bk_img`/`mid_img`/`fr_img`），笔刷只取**当前层**那一个字段 | 任务书 B |
//! | 笔刷 [`Tool::Brush`] | 图号层：只写当前层图号（+ 14/36 档该层库号字节）；**族不匹配 ⇒ 拒绝且一字节不动**<br>阻挡层：**只置 `bit15`**（= 不可走），低 15 位原样；**图库笔刷 ⇒ 拒绝**<br>落笔范围 = 以鼠标格为中心的 **N×N**（`N` = 笔刷尺寸 3..=31 奇数，边缘裁剪） | 同上 / 审计 P0-9 / 审计 P0-5 |
//! | 橡皮擦 [`Tool::Eraser`] | 图号层：**只清当前层图号**：`img &= 0x8000`（保留阻挡位）<br>阻挡层：**只清 `bit15`**（= 可走）：`img &= 0x7FFF`（保留图号）<br>落笔范围同样是 N×N | 既有约定 + 审计 P0-9 |
//! | 填充 [`Tool::Bucket`] | 从点击格 BFS 到「**同层且该层图号相同**」的连通块，用当前笔刷逐格 `paint`；**整次填充 = 一步撤销**；地表按 C# 的 `step=2`（只走偶偶格）<br>判据逐条对 `CrystalStudio/MapViewport.cs:485-539` | 审计 P0-5 |
//!
//! # 二、笔刷尺寸（3..=31 奇数）
//!
//! 只影响**落笔范围**（[`EditSession::footprint`]：居中 N×N、越界格跳过），
//! **不影响任何已写进地图的字节**，因此改尺寸不重渲、不进撤销栈
//! （[`EditSession::set_brush_size`]）。范围与步进来自
//! `CrystalStudio/MainWindow.axaml:6` 的滑块（`Minimum=3 Maximum=31 TickFrequency=2`），
//! 快捷键 `[` / `]` 各 ±2 来自 `MainWindow.axaml.cs:871`。
//!
//! **绝不跨字段乱写**：地表笔刷配「物体」层时，`paint` 直接返回 [`PaintOutcome::LayerMismatch`]，
//! 由 [`EditSession::note`] 把原因交给状态栏，地图一个字节都不动。
//!
//! # 三、阻挡层（`bit15`）的口径
//!
//! 语义**以客户端为准**：`wBkImg & 0x8000` = 低阻挡、`wFrImg & 0x8000` = 高位阻挡
//! （`client/reference_full/Classes/map/TMap.cpp:339` 的 `CanWalk`；
//! Rust 侧同口径 `mir2-map/src/cell.rs` 的 `Cell::is_walkable`）。
//!
//! ⚠ **绝不用 C# `CrystalStudio` 的 `0x20000000` / `0x40000000`**（`MapViewport.cs:23`）——
//! 那是它内存模型的私有位，写进 `.map` 我们客户端读不懂。
//!
//! 三条硬口径：
//! 1. **只动 `bit15`**：置位 = `img | 0x8000`、清位 = `img & 0x7FFF`，
//!    **低 15 位的图号一个 bit 都不许动**（有逐字节测试钉住）；
//! 2. **图库笔刷一律拒绝**（[`PaintOutcome::PaletteBrushOnCollision`]）：图库笔刷的意图是
//!    "盖一张图"，而阻挡层不写图号 —— 静默把位设上会让用户以为图贴上去了；
//!    从地图上吸来的笔刷（[`Brush::from`] 为 `Some`）与"手上没有笔刷"都允许落笔；
//! 3. **不写库号字节**：`btTiles` / `btArea` 在阻挡层原样保留（[`EditLayer::lib_field`] 为 `None`）。
//!
//! **`Old12` 档没有库号字节**：三层的库号字段在该档根本没有对应字节，于是只写图号；
//! `Cell::tiles`/`sm_tiles` 保持原值、`to_bytes()` 长度不变（不凭空补第 13 个字节）。
//!
//! 橡皮为什么在图号层保留 `0x8000`：`.map` 里低阻挡位就存在 `bk_img` 最高位（见
//! `mir2_map::Cell::is_walkable`），若连阻挡位一起清，橡皮就变成了"顺手把墙拆了"——
//! 那是另一个语义（碰撞层编辑）。现在那个语义有了**自己的层**（低位/高位阻挡），
//! 所以图号层的橡皮继续保留最高位，两层各管各的位，互不越界。
//! **画笔 / 填充写图号同样不许清掉碰撞位**（2026-11-04 数据正确性修复，判据见
//! [`EditLayer::img_of_has_collision_bit`] 与 `miredit_core::paint` 的模块文档）。
//!
//! # 四、一次拖动 = 一步
//!
//! [`EditSession::begin`] → 若干次 [`EditSession::touch`] → [`EditSession::end`]，
//! 中间经过的格全部并进**同一个** [`Step`]，于是一次拖动只占一层撤销。
//! 同一格在一次拖动里只记第一次的 `before`（否则撤销会退到中间态）。
//!
//! # 五、怎么改（**本模块不再直接改 `Map::cells`**）
//!
//! 就地改格的那套代码已经搬进 `miredit_core`（`document::cell_mut` 走 `Map::cells`
//! 这个公开字段，不需要给 `mir2-map` 加 `cell_mut` —— 那个 crate 明令禁止改）。
//! 本模块只通过 [`cell_at`] / [`cell_mut`] / [`EditSession`] 的包装方法访问地图；
//! **不许**在宿主里另开一条"直接写 `cells[idx]`"的路（那会让"内存里的地图"与
//! "内核记账的撤销栈"变成两套真值）。

use std::path::Path;

use mir2_map::{CellFormat, Map};

use crate::ui_text as tx;

// ───────────────────── 从内核 re-export（本文件不再有第二份定义） ─────────────────────

pub use miredit_core::{
    byte_diff, cell_at, cell_mut, fill_key, index_of, lib_ref_of, line_cells,
    palette_index_to_sprite, paint, Brush, ByteDiff, Change, Clipboard, EditLayer, Family, FillKey,
    FillOutcome, FillRecord, LibRef, PaintOutcome, PickedCell, PlaceOutcome, Selection, Step,
    StampCellInput, Tool, COLLISION_BIT, SPRITE_MAX, TOOL_BRUSH_IDX, TOOL_BUCKET_IDX,
    TOOL_ERASER_IDX, TOOL_PICKUP_IDX, UNDO_LIMIT,
};

use miredit_core::{footprint_of, pick_cell_of};

// ───────────────────────── 宿主层的文案（不迁：core 不许依赖 gui 文案表） ─────────────────────────

/// 层名（族 → 中文层名）；`Family::Surface` 显示为「物件层」。
///
/// 与 [`crate::palette::family_label`] 同一口径（那边按族、这边按层，值必须相等，
/// 见测试 [`层名与族名同源`](tests::层名与族名同源)）—— 两处各写一套中文的话，
/// 状态栏与素材面板迟早会不同名。
///
/// ⚠ 与 `miredit_core::Family::label()` 的**值相同但来源不同**：core 的那份是
/// 自带常量（不依赖 gui 文案表），这份是 gui 文案表的入口。两边的值由
/// `miredit-core` 的 `层名与界面文案同源` 与 gui 的 `层名与族名同源` 两条测试钉住。
pub fn family_name(f: Family) -> &'static str {
    // 显式三臂：**不**用 `Family::label()`（那会把"gui 文案表 ↔ core 常量"的一致性
    // 变成一条不被任何测试覆盖的隐式假设）。这样测试 `层名与族名同源` 才真的在比
    // `palette::family_label` 与 core 的 `EditLayer::name()` 两张表。
    match f {
        Family::Ground => crate::palette::family_label(mir2_assets::Family::Ground),
        Family::Middle => crate::palette::family_label(mir2_assets::Family::Middle),
        Family::Surface => crate::palette::family_label(mir2_assets::Family::Surface),
    }
}

/// 库号的显示文案：`Old12` 档没有 `btTiles` 字节 ⇒ 如实 `—`（不是 `0`）。
pub fn lib_text(fmt: Option<CellFormat>, tiles: u8) -> String {
    match fmt {
        Some(f) if f.has_lib_bytes() => tiles.to_string(),
        _ => tx::VAL_UNKNOWN.to_owned(),
    }
}

/// **笔刷**库号的显示文案：笔刷自带真实库号就优先用它，否则回落到地图格式口径。
///
/// 为什么不是直接 [`lib_text`]：`Old12` 档的格子里没有库号字节，但"从素材面板选的这一帧"
/// 本来就知道库 —— 那时显示 `—` 是把已知信息丢掉（任务书 §四.4 要的就是真实库号）。
/// 从 12B 地图上吸管取的笔刷仍然显示 `—`（`lib == None`），因为那时**真的不知道**库号。
pub fn brush_lib_text(fmt: Option<CellFormat>, b: &Brush) -> String {
    match b.lib {
        Some(lib) => lib.index.to_string(),
        None => lib_text(fmt, b.lib_byte),
    }
}

/// 布尔 → 界面用的「有/无」。
pub fn dirty_text(v: bool) -> &'static str {
    if v {
        tx::VAL_YES
    } else {
        tx::VAL_NO
    }
}

/// **工具图标下标 → 界面显示的工具名**（含内核 `Tool` 没有的两支：选择 / 印章）。
///
/// 为什么需要它：内核 `Tool::from_index(4)` / `(5)` 都返回 `Unwired`（它的契约只覆盖
/// 4 个编辑工具 + `Unwired`），直接把 `Tool::name()` 显示出来会写成「未接线」——
/// 那是**假信息**（用户明明点了"选择"）。两支宿主层工具的语义分别在
/// [`crate::selection_ops`] 与 [`crate::stamp_place`] 里。
pub fn slot_label(i: usize) -> &'static str {
    match i {
        crate::selection_ops::TOOL_SELECT_IDX => tx::TOOL_SELECT,
        crate::selection_ops::TOOL_STAMP_IDX => tx::TOOL_STAMP,
        _ => Tool::from_index(i).name(),
    }
}

// ───────────────────────── 宿主 ↔ 内核的类型桥（只搬字段，不猜） ─────────────────────────

/// `mir2_assets::LibRef`（宿主素材面板用的定位键）→ 内核的 [`LibRef`]。
///
/// 两个类型**语义一一对应、按序号一一对应**（`Ground`↔`Tiles` / `Middle`↔`SmTiles` /
/// `Surface`↔`Objects`），但**词表不同**（见 `miredit_core::brush` 里 `Family` 的文档：
/// core 的 `label()` 是界面词「地表图层/中层图层/物体」，`mir2-assets` 的是日志词
/// 「地面/中层/表层」）。所以这里只做字段搬运 —— 不查表、不 fallback、不猜。
pub fn core_lib(l: mir2_assets::LibRef) -> LibRef {
    let family = match l.family {
        mir2_assets::Family::Ground => Family::Ground,
        mir2_assets::Family::Middle => Family::Middle,
        mir2_assets::Family::Surface => Family::Surface,
    };
    LibRef {
        family,
        index: l.index,
    }
}

/// [`core_lib`] 的逆：内核的 [`LibRef`] → `mir2_assets::LibRef`。
///
/// 为什么要有它：`EditSession::set_brush_from_palette` 的签名收的是
/// `mir2_assets::LibRef`（素材面板与 `app.rs` 都用那一套），而内核的
/// [`lib_ref_of`] 返回的是内核的 [`LibRef`] —— 两边要能互换，但**判据（层→族）
/// 仍然只有 `lib_ref_of` 一处**，这里只搬字段。
pub fn assets_lib(l: LibRef) -> mir2_assets::LibRef {
    let family = match l.family {
        Family::Ground => mir2_assets::Family::Ground,
        Family::Middle => mir2_assets::Family::Middle,
        Family::Surface => mir2_assets::Family::Surface,
    };
    mir2_assets::LibRef {
        family,
        index: l.index,
    }
}

// ───────────────────────── 会话（薄包装） ─────────────────────────

/// 未绑定地图时的**占位文档**路径（只出现在诊断里，不落盘）。
const 占位路径: &str = "<未绑定地图>";

/// 未绑定地图时的**占位文档**（1×1 `Old12`，`52 + 12 = 64` 字节）。
///
/// 为什么需要它：宿主（`statusbar.rs` / `text_fit.rs` / `editrun.rs`）用
/// [`EditSession::default`] 造一个"还没有地图"的会话；而内核会话**必须**持有文档
/// （它的每个改格方法都吃自己的地图）。1×1 是最小的合法地图
/// （`mir2-map` 的 `Map::parse` 拒绝 0 尺寸，见 `MapError::ZeroDimension`），
/// 解析它不需要任何 I/O，也不会被写回磁盘。
fn 占位文档() -> miredit_core::Document {
    let mut bytes = vec![0u8; mir2_map::HEADER_LEN + 12];
    bytes[0..2].copy_from_slice(&1u16.to_le_bytes());
    bytes[2..4].copy_from_slice(&1u16.to_le_bytes());
    miredit_core::Document::from_bytes(Path::new(占位路径), bytes)
        .expect("1×1 Old12 是合法地图（52+12=64 字节），占位文档必然可解析")
}

/// 编辑会话：工具 + 笔刷 + **当前编辑层** + **笔刷尺寸** + 撤销/重做栈 + 脏标记。
///
/// 内部**只有一份状态**：`miredit_core::EditSession`。本类型不另存任何字段
/// （不镜像 `tool` / `layer` / `brush_size` / `undo` —— 那正是"两份真值"的来源）。
pub struct EditSession {
    /// 内核会话（持有文档；文档里的 `Map` 在每次调用前后与宿主地图交换）。
    core: miredit_core::EditSession,
}

impl Default for EditSession {
    fn default() -> Self {
        Self {
            core: miredit_core::EditSession::new(占位文档()),
        }
    }
}

/// 手写 `Debug`：内核会话不派生 `Debug`（它持有整张地图，打出来没有意义），
/// 而宿主侧的 `#[derive(Debug)]` 是既有 API ⇒ 只打"工具 / 层 / 尺寸"这三样。
impl std::fmt::Debug for EditSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EditSession")
            .field("tool", &self.core.tool())
            .field("layer", &self.core.layer())
            .field("brush_size", &self.core.brush_size())
            .field("undo_len", &self.core.undo_len())
            .field("dirty", &self.core.dirty())
            .finish()
    }
}

impl EditSession {
    /// 把**真文档**（九格式的字节层真值）装进会话 —— 打开地图后**立刻调一次**。
    ///
    /// # 为什么需要它（2026-11-05 九格式收口）
    ///
    /// 内核有三处判定读的是 **`raw`（字节层）**，而不是编辑视图：
    ///
    /// | 判定 | 位置 | 读什么 |
    /// |---|---|---|
    /// | 库号字节还原 | `convert::restore_unwritten_lib_byte` | `raw.format` |
    /// | 地表闸门（Type5 拒绝） | `session.rs` 的 `ground_refusal` | `raw.format`（`ground_paint_supported`）|
    /// | 选区入口裁剪 | `EditSession::set_selection` | `raw.width/height`（`Document::size`）|
    ///
    /// 默认会话里那份文档是 1×1 `Old12` 占位（[`占位文档`]），三处都会按**错误的格式**判定：
    /// 最典型的是 **Type5 的地表闸门在 GUI 里根本不可达**（占位是 `Old12`，
    /// `ground_paint_supported(Old12) == true`）。
    ///
    /// 绑定之后 `raw` 从第一刻起就是磁盘那份字节 ⇒ [`Self::sync_document_format`] 永远早退。
    ///
    /// # 绑定失败也不许静默
    ///
    /// 返回 `false` ⇒ 保持占位文档。此时**保存仍然正确**（保存走
    /// [`crate::mapdoc::MapDoc::to_bytes`]，与本文档无关），受影响的只有"按格式判定的编辑语义"；
    /// 调用方必须把这件事报出来（`app.rs` / `editrun.rs` 打到 stderr）。
    pub fn bind_document(&mut self, path: &Path, bytes: &[u8]) -> bool {
        match miredit_core::Document::from_bytes(path, bytes.to_vec()) {
            Ok(doc) => {
                *self.core.document_mut() = doc;
                true
            }
            Err(_) => false,
        }
    }

    /// 把**宿主的地图**换进内核会话 → 执行 `f` → 换回。
    ///
    /// `std::mem::swap` 只交换 `Map` 的 `Vec` 指针与宽高档位，**不拷格数据**
    /// （`0.map` 5.88 MB，逐次拷贝会直接把交互拖垮）。
    ///
    /// 为什么必须这样：内核会话的每个改格方法都吃**自己的**地图，而宿主的
    /// `DocState.map` 才是"会被保存/被渲染的那一份"。两个对象都要有地图，
    /// 又不能变成两套真值 ⇒ 同一时刻只让一处持有它，中间用 O(1) 的交换。
    fn with_map<R>(
        &mut self,
        map: &mut Map,
        f: impl FnOnce(&mut miredit_core::EditSession) -> R,
    ) -> R {
        // ① 先把内核文档的**字节层档位**对齐到宿主地图（见 [`Self::sync_document_format`]）
        self.sync_document_format(map);
        // ② 再把宿主地图换进内核的**编辑视图**
        //    ⚠ 2026-11-05：内核 `Document` 的地图改成了私有的 `view`，对外经 `view_mut()`
        //    暴露 —— 交换目标跟着改成它（`std::mem::swap` 仍然只换 `Vec` 指针与宽高，**不拷格**）。
        std::mem::swap(self.core.document_mut().view_mut(), map);
        let r = f(&mut self.core);
        std::mem::swap(self.core.document_mut().view_mut(), map);
        r
    }

    /// 让内核文档的**字节层**（档位 `raw.format` + 尺寸 `raw.width/height`）与宿主地图一致。
    ///
    /// # 结论先说（2026-11-05 九格式收口后**保留**，但生产路径上一次都不会重建）
    ///
    /// 打开地图时 [`Self::bind_document`] 已经把**真文档**装进会话 ⇒ `raw` 从第一刻起就是
    /// 磁盘那份字节的真值，档位投影与尺寸都与宿主地图一致 ⇒ 本方法**第一句就 `return`**。
    /// 所以它不是"九格式之前的历史包袱"，而是**未绑定会话的兜底**：
    ///
    /// | 场景 | `raw` 是什么 | 本方法做什么 |
    /// |---|---|---|
    /// | 生产（`DocState::load` + `bind_document`） | 真文档（九种之一） | 早退，**零开销** |
    /// | 单元测试 / `EditSession::default()` | 1×1 `Old12` 占位 | 用宿主地图字节重建 |
    ///
    /// 为什么不能直接删：`edit.rs` 里 80 多条单元测试直接拿裸 `Map` 调改格方法
    /// （不经过 `DocState`），会话里只有占位文档；删掉之后 `raw.format` 恒为 `Old12`，
    /// 于是 ① `restore_unwritten_lib_byte` 会认为"这个档位没有库号字节"，把刚写进去的
    /// `btTiles` 还原成改前值（实测：New14 图上 `tiles` 从 5 变回 3，
    /// `tests::画笔只改地表层` 当场红）；② `set_selection` 用 `Document::size()` 裁剪，
    /// 1×1 会把**任何真实选区**判成图外（症状是"框选没反应"，而地图一个字节都没错）。
    ///
    /// # 什么场景会"不一致"
    ///
    /// 只有**会话里的文档不是这张地图的文档**时：未绑定的默认会话、或有人把另一个
    /// `Map` 塞进已绑定的会话。判据是**档位投影**（`view_cell_format`）+ **尺寸**，
    /// 两条都相同才早退 —— 上一单修的"档位**且**尺寸都一致才早退"原样保留。
    ///
    /// # 修法
    ///
    /// 不一致时用宿主地图的字节**重建文档对象**，然后**就地替换**
    /// （`*document_mut() = doc`）—— 工具 / 当前层 / 笔刷 / 笔刷尺寸 / 撤销栈 / 选区 / 剪贴板
    /// 全部原样保留，因为它们都不在 `Document` 里。
    ///
    /// # 代价
    ///
    /// 只在**换图（档位或尺寸变化）**时发生一次 `to_bytes()` + 再解析（本机最大图 5.88 MB），
    /// **不是每次落笔**：同图连续编辑时第一句就 `return` 了。⚠ 这条重建路径**只对 12/14/36
    /// 三档保真**（`map.to_bytes()` 的能力边界）—— 九格式的会话必须走 [`Self::bind_document`]。
    fn sync_document_format(&mut self, map: &Map) {
        let (rw, rh) = self.core.document().size();
        if miredit_core::view_cell_format(self.core.document().format_id()) == map.format
            && (rw, rh) == (map.width, map.height)
        {
            return;
        }
        let Ok(doc) = miredit_core::Document::from_bytes(Path::new(占位路径), map.to_bytes()) else {
            // 宿主地图自己都写不出字节（理论上不会发生）⇒ 保持原样：
            // 落笔仍会走内核的"库号字节还原"闸门，语义不静默改变。
            return;
        };
        *self.core.document_mut() = doc;
    }

    /// 把一条可序列化命令作用在**宿主的地图**上（换图 → `core::apply` → 换回）。
    ///
    /// 失败（目前只有"层名不认识"这一种）折成"改了 0 格 + 原因"，**绝不静默**。
    fn apply_cmd(
        &mut self,
        map: &mut Map,
        cmd: miredit_core::Command,
    ) -> miredit_core::Outcome {
        self.with_map(map, |core| match miredit_core::apply(core, cmd) {
            Ok(out) => out,
            Err(err) => core.outcome(0, Some(err.to_string())),
        })
    }

    // ── 只读 ──

    /// 当前工具。
    pub fn tool(&self) -> Tool {
        self.core.tool()
    }

    /// 当前笔刷内容。
    pub fn brush(&self) -> Option<Brush> {
        self.core.brush()
    }

    /// **当前编辑层**（落笔写哪一层）。
    pub fn layer(&self) -> EditLayer {
        self.core.layer()
    }

    /// 最近一次「拒绝落笔」的原因（`None` = 没被拒）。
    ///
    /// 只装**拒绝**原因（族不匹配 / 图库笔刷落到阻挡层 / 填充被拒），不装"落笔字段提示" ——
    /// 后者由 [`crate::screen_text`] 按当前层与档位现算，两边不混在一个字段里。
    pub fn note(&self) -> Option<&str> {
        self.core.note()
    }

    /// **笔刷尺寸**（3..=31 的奇数）——落笔范围 = 以鼠标格为中心的 N×N。
    pub fn brush_size(&self) -> usize {
        self.core.brush_size()
    }

    /// 最近一次填充的回执（`None` = 还没填过，或填完被撤销/重做/新笔画清掉了）。
    pub fn last_fill(&self) -> Option<FillRecord> {
        self.core.last_fill()
    }

    /// 清掉「拒绝落笔」提示（用户切层/换笔刷后由内部自动调用）。
    pub fn clear_note(&mut self) {
        self.core.clear_note();
    }

    /// 撤销栈深度。
    pub fn undo_len(&self) -> usize {
        self.core.undo_len()
    }

    /// 重做栈深度。
    pub fn redo_len(&self) -> usize {
        self.core.redo_len()
    }

    /// 当前所处步号（`0` = 文件刚打开时）。
    pub fn pos(&self) -> u64 {
        self.core.pos()
    }

    /// 保存点步号。
    pub fn saved_pos(&self) -> u64 {
        self.core.saved_pos()
    }

    /// 是否有未保存的修改。
    ///
    /// 判据不是"栈非空"，而是**当前位置 ≠ 保存点**：撤销回到保存点后
    /// `dirty` 必须自动变回 `false`，否则用户明明已经退回原样，标题栏还挂着 `*`。
    ///
    /// ⚠ 还有一条：**笔画没结算时也算脏**。窗口失焦/拖动被打断时可能收不到"松开"事件，
    /// 此时字节已经写进内存却没有对应的撤销步 —— 若 `dirty` 仍是假，关窗就不会提示，
    /// 那些字节会静默丢掉。
    pub fn dirty(&self) -> bool {
        self.core.dirty()
    }

    /// 最近一次保存的结果。
    pub fn last_save(&self) -> Option<&Result<(), String>> {
        self.core.last_save()
    }

    /// 最近一次保存的失败原因（成功/没按过 = `None`）。
    pub fn save_error(&self) -> Option<&str> {
        self.core.save_error()
    }

    /// 是否正在一次拖动中。
    pub fn stroke_active(&self) -> bool {
        self.core.stroke_active()
    }

    /// 当前拖动最近落笔的格（插值起点）。
    pub fn last_cell(&self) -> Option<(u16, u16)> {
        self.core.last_cell()
    }

    // ── 状态变更 ──

    /// 选中第 `i` 个工具图标；返回该工具是否已接线。
    pub fn select(&mut self, i: usize) -> bool {
        self.core.set_tool(Tool::from_index(i))
    }

    /// 选**当前编辑层**（写哪一层由它决定）。
    ///
    /// 切层会立刻重算「族不匹配」提示：上一层的笔刷很可能配不上新层，
    /// 那种情况下状态栏应当**马上**说出来，而不是等用户点下去才发现没反应。
    pub fn set_layer(&mut self, layer: EditLayer) {
        self.core.set_layer(layer);
    }

    /// 设**笔刷尺寸**；返回真正生效的尺寸（夹进 3..=31 的奇数）。
    ///
    /// ## ⚠ 改尺寸**不重渲**（也不改地图、不进撤销栈、不动脏标记）
    ///
    /// 尺寸只决定"下一次落笔覆盖多大一块"，已写进地图的字节一个都不变，
    /// 因此**没有理由**让画布丢掉纹理缓存（[`crate::canvas_view::CanvasView::ensure`] 的键
    /// 里也没有尺寸）。官方 `MainWindow.axaml.cs:609` 的 `BrushChanged` 只做了两件事：
    /// `Viewport.BrushSize = …` 与 `BrushSizeLabel.Text = …`（+ 让视口重画笔刷预览），
    /// 同样是"不碰地图数据"。
    pub fn set_brush_size(&mut self, n: usize) -> usize {
        self.core.set_brush_size(n)
    }

    /// 笔刷尺寸按 `delta` 走（官方 `[` / `]` = ∓2）；返回真正生效的尺寸。
    pub fn grow_brush_size(&mut self, delta: isize) -> usize {
        self.core.grow_brush_size(delta)
    }

    /// 以 `(x, y)` 为中心的 **N×N 落笔格**（已按地图边界裁剪）。
    ///
    /// 形状/裁剪判据在 `miredit_core::footprint_of`（单一实现）；本方法只把
    /// "当前笔刷尺寸 + 调用方给的宽高"喂进去。
    ///
    /// 形状 = **居中方形**（官方 `MapViewport.cs:476-478` 的双层循环
    /// `for iy in -radius..=radius { for ix in -radius..=radius { … if (!Document.Contains(sx, sy)) continue; } }`；
    /// 官方工具提示 `CentredBrushFootprintAndtoResize_55DDE2E3` 也写的是「以中心定位笔刷范围」）。
    ///
    /// 边缘裁剪 = **逐格跳过越界格**（与官方 `continue` 同一判据），不是"把方块整体挪回界内"。
    ///
    /// ⚠ **与官方的一处差异（有意，已记档）**：官方地表层的 `step = 2`
    /// （`MapViewport.cs:474-475`：`int step = Layer == MapLayer.Ground ? 2 : 1; if (step == 2) { x &= ~1; y &= ~1; }`，
    /// 即它的地表按 2×2 对齐落笔）；本仓 `paint()` 对三层都是**逐格**写，
    /// 所以这里 `step = 1`。理由：填充/笔刷必须与既有落笔语义（以及 `mir2-map` 的逐格字节）一致，
    /// 否则同一格会出现"笔刷写得到、填充写不到"的两套网格。
    pub fn footprint(&self, x: u16, y: u16, w: u16, h: u16) -> Vec<(u16, u16)> {
        footprint_of(self.core.brush_size(), x, y, w, h)
    }

    /// 标记「已保存到磁盘」：清脏标记。
    pub fn mark_saved(&mut self) {
        self.core.mark_saved();
    }

    /// 记一次保存成功。
    pub fn note_save_ok(&mut self) {
        self.core.note_save_ok();
    }

    /// 记一次保存失败（原因原样保留，状态栏要显示它）。
    pub fn note_save_err(&mut self, err: String) {
        self.core.note_save_err(err);
    }

    /// 开始一次拖动。
    ///
    /// 上一笔若还没结算（窗口失焦、系统吞了"松开"事件），**先把它结算成一步**再开新的一笔 ——
    /// 否则那批字节既撤销不了、也不会出现在撤销栈里，成了孤儿。
    pub fn begin(&mut self) {
        self.core.begin();
    }

    // ── 核心：改格 ──

    /// 在当前拖动里把工具作用到 `(x, y)`；返回是否真的改了这一格（0 或 1）。
    ///
    /// 同一格在一次拖动里重复经过只记第一次：返回 0（值已经写过了）。
    ///
    /// **族不匹配时返回 0 且一字节不动**，并把原因记进 [`Self::note`]（状态栏显示）。
    pub fn touch(&mut self, map: &mut Map, x: u16, y: u16) -> usize {
        self.with_map(map, |core| core.touch(x, y))
    }

    /// 在当前拖动里把工具作用到**以 `(x, y)` 为中心的 N×N**（N = [`Self::brush_size`]）；
    /// 返回真的改了几格（0..=N²，边缘会被裁剪）。
    ///
    /// 这是**画笔 / 橡皮**的落笔入口（官方 `MapViewport.cs:473-482`）：
    /// 尺寸只对这两个工具生效，铅笔与填充恒为 1 格（[`Tool::uses_brush_size`]）。
    /// 吸管/未接线工具**不吃尺寸**，直接返回 0（它们本来就不改地图）。
    pub fn touch_footprint(&mut self, map: &mut Map, x: u16, y: u16) -> usize {
        self.with_map(map, |core| core.touch_footprint(x, y))
    }

    /// 结束一次拖动：把累积变更压成**一步**；返回本步改了几格。
    pub fn end(&mut self) -> usize {
        self.core.end()
    }

    /// 单格一步（按下即松开、或脚本驱动时用）：`begin + touch + end`。
    ///
    /// ⚠ **恒为 1 格**（不吃笔刷尺寸）：这是"精确落一格"的底层原语，
    /// 鼠标落笔走的是 [`Self::paint_stroke_once`]（吃尺寸）。
    pub fn apply_once(&mut self, map: &mut Map, x: u16, y: u16) -> usize {
        self.with_map(map, |core| core.apply_once(x, y))
    }

    /// 以 `(x, y)` 为中心按**当前笔刷尺寸**落一笔（= `begin + touch_footprint + end`）。
    ///
    /// 鼠标"按下即松开"与脚本 `--edit-brush` 走的都是这条（吃尺寸）；
    /// 它与 [`Self::apply_once`] 的差别只有"1 格 vs N×N"，记账方式完全一样。
    pub fn paint_stroke_once(&mut self, map: &mut Map, x: u16, y: u16) -> usize {
        self.with_map(map, |core| core.paint_stroke_once(x, y))
    }

    /// 从 `from` 拖到 `to`（Bresenham 插值，**整条轨迹都按 N×N 落**），结算成**一步**。
    ///
    /// 这就是鼠标拖动的完整语义（[`crate::app`] 的 `paint_to` 与脚本 `--edit-stroke` 共用）：
    /// ① 插值补格（快速拖动时指针会跳格）；② 每个插值点落 N×N；③ 全部并进同一步撤销。
    pub fn paint_stroke(
        &mut self,
        map: &mut Map,
        from: Option<(u16, u16)>,
        to: (u16, u16),
    ) -> usize {
        self.with_map(map, |core| core.paint_stroke(from, to))
    }

    // ── 填充（Bucket） ──

    /// 填充：从 `(x, y)` 出发，BFS 扩散到「**同层且该层图号相同**」的连通块，用当前笔刷填满。
    ///
    /// 判据表见 `miredit_core::fill` 模块文档（逐条对
    /// `CrystalStudio/MapViewport.cs:485-539` 的 `BucketFill`）。
    ///
    /// # 与官方的两处**有意差异**（都已写进 core 注释，不是悄悄改）
    ///
    /// 1. **上限保护**：官方没有上限，本仓单次最多 `FILL_MAX_CELLS`（200,000）格，
    ///    超过 ⇒ [`FillOutcome::TooLarge`] 且**一格不写**（先把连通块收齐再动笔）；
    /// 2. **计的是"真改了几格"**：官方 `:530` 的 `count++` 数的是**出队格数**；
    ///    本仓数的是 `Step.changes.len()`（值真的变了的格）。
    ///
    /// # 阻挡层只**置位**，不清位
    ///
    /// 官方 `:516` 是 `c.BackImage |= ActiveCollisionMask`（只有 `|=`，没有 `&=`），
    /// 且 `:501` 的早退保证"点已置位的块"直接返回 0。要清位请用橡皮
    /// （[`Tool::Eraser`] 在阻挡层是 `& !0x8000`）。
    pub fn fill(&mut self, map: &mut Map, x: u16, y: u16) -> FillOutcome {
        self.with_map(map, |core| core.fill(x, y))
    }

    /// 吸管读**整格三族**（三个图号字段原样）；越界 = `None`。
    ///
    /// 只读不写：它不改地图、不进撤销栈。
    pub fn pick_cell(&self, map: &Map, x: u16, y: u16) -> Option<PickedCell> {
        pick_cell_of(map, x, y)
    }

    /// 吸管：把 `(x, y)` **当前编辑层**的内容取成笔刷；越界 = `None`（笔刷不变）。
    ///
    /// 「按当前层取」的含义：当前层是中间层，取的就是 `mid_img` + `btsmTiles`，
    /// 笔刷族 = [`Family::Middle`]；吸完立刻能往中间层画（族自然匹配）。
    /// 整格的另外两族**不会被塞进笔刷** —— 那正是"跨族乱写"的来源。
    pub fn pick_at(&mut self, map: &Map, x: u16, y: u16) -> Option<Brush> {
        self.core.pick_at_from(map, x, y)
    }

    /// 素材面板点选一帧 → 设成笔刷（**三族都允许**，笔刷带族）。
    ///
    /// 不受限的是"从哪个库选"，**受限的是落笔**：写哪个字段由当前编辑层决定，
    /// 族与层不一致时 [`Self::touch`] 会拒绝并把原因写进 [`Self::note`]。
    ///
    /// `img` 必须是**0 基帧索引**且换算后的图号 ≤ [`SPRITE_MAX`]，否则返回 `None`
    /// 且**不动**现有笔刷（图号 0x8000 会撞上最高位，那是错数据，宁可拒绝）。
    ///
    /// ## ⚠ `img` 是 0 基帧索引，落格写的是 `img + 1`（审计 E1，2026-11-04）
    ///
    /// 三个图号字段都是**1 基图号**：渲染侧取图是 `(img & 0x7FFF) - 1`
    /// （`miredit-render/src/compose.rs` 三层都是 `cache.get(lib, u32::from(n - 1))`），
    /// 客户端真值同样 1 基且 `0` = 该层没有图
    /// （`GodotClient/game/world/map_loader.gd:51-52`「帧号是 1-based，取图前先 -1」、
    /// `GameScene.cpp:3567-3577`）。
    ///
    /// 换算只在 `miredit_core::palette_index_to_sprite` 里做一次，别处不要再 ±1。
    pub fn set_brush_from_palette(&mut self, lib: mir2_assets::LibRef, img: u32) -> Option<Brush> {
        self.core.set_brush_from_palette(core_lib(lib), img)
    }

    /// 撤销一步；返回还原了几格（0 = 没有可撤销的步骤）。
    pub fn undo(&mut self, map: &mut Map) -> usize {
        self.with_map(map, |core| core.undo())
    }

    /// 重做一步；返回重放了几格（0 = 没有可重做的步骤）。
    pub fn redo(&mut self, map: &mut Map) -> usize {
        self.with_map(map, |core| core.redo())
    }

    // ── 印章放置（内核 `register_stamp` / `place_stamp_by_id` 的宿主入口） ──

    /// **注册**一枚印章的格表（同一个 id 再注册 = 覆盖）。
    ///
    /// 内核**不读盘**（它只存 `id → 格表`），所以 `.X → 格表` 的解析在
    /// [`crate::stamp_place::read`] 里做，本方法只负责转交。
    /// 三道护栏（空格表 / 超过 100 万格 / 注册表满）由内核给中文原因。
    pub fn register_stamp(
        &mut self,
        id: impl Into<String>,
        cells: Vec<StampCellInput>,
    ) -> Result<usize, String> {
        self.core.register_stamp(id, cells)
    }

    /// 已注册的印章 id（字典序）。
    pub fn stamp_ids(&self) -> Vec<&str> {
        self.core.stamp_ids()
    }

    /// 取某个 id 的格表（未注册 ⇒ `None`）。
    pub fn stamp_cells(&self, id: &str) -> Option<&[StampCellInput]> {
        self.core.stamp_cells(id)
    }

    /// 注销一枚印章（注册表满时腾位用）。
    pub fn remove_stamp(&mut self, id: &str) -> bool {
        self.core.remove_stamp(id)
    }

    /// 按 **id** 放置一枚**已注册**的印章；`(x, y)` 是锚点。
    ///
    /// 语义全在内核（逐格越界跳过 / 三族图号 / `bit15` 保留 / Type5 闸门 /
    /// 整次放置 = 一步撤销）。拒绝时内核同时把中文原因写进 [`Self::note`]。
    pub fn place_stamp_by_id(&mut self, map: &mut Map, x: u16, y: u16, id: &str) -> PlaceOutcome {
        self.with_map(map, |core| core.place_stamp_by_id(x, y, id))
    }

    // ── 矩形选区 / 剪贴板（内核 `session.rs` 选区段的宿主入口） ──

    /// 当前矩形选区（`None` = 没有选区）。**只读会话状态，不吃地图**。
    pub fn selection(&self) -> Option<Selection> {
        self.core.selection()
    }

    /// 设选区（内核**入口裁剪一次**；返回裁剪后是否非空）。
    ///
    /// 为什么要吃 `map`：内核按 `Document::size()` 裁剪，而宿主的地图在
    /// [`Self::with_map`] 里才换进内核 —— 所以这里必须走一次交换（**不改地图**）。
    pub fn set_selection(&mut self, map: &mut Map, sel: Selection) -> bool {
        self.with_map(map, |core| core.set_selection(sel))
    }

    /// 取消选区（**不清剪贴板**：剪贴板是独立快照，内核的 `Deselect` 也不清）。
    pub fn clear_selection(&mut self) {
        self.core.clear_selection();
    }

    /// 当前剪贴板（`None` = 还没复制过）。**只读会话状态，不吃地图**。
    pub fn clipboard(&self) -> Option<&Clipboard> {
        self.core.clipboard()
    }

    /// 复制选区进剪贴板；返回 `(宽, 高)`。**不置脏、不进撤销栈**（只读地图）。
    pub fn copy_selection(&mut self, map: &mut Map) -> Option<(u16, u16)> {
        self.with_map(map, |core| core.copy_selection())
    }

    /// 剪切 = 复制 + 清空（**整次 = 一步撤销**）；返回 `(宽, 高)`。
    pub fn cut_selection(&mut self, map: &mut Map) -> Option<(u16, u16)> {
        self.with_map(map, |core| core.cut_selection())
    }

    /// 以 `(x, y)` 为**左上角**粘贴剪贴板（语义与印章放置同一份实现）。
    pub fn paste_clipboard(&mut self, map: &mut Map, x: u16, y: u16) -> PlaceOutcome {
        self.with_map(map, |core| core.paste_clipboard(x, y))
    }

    /// 删除选区内容（三族按层清，**整次 = 一步撤销**）；返回真改了几格。
    pub fn delete_selection(&mut self, map: &mut Map) -> usize {
        self.with_map(map, |core| core.delete_selection())
    }

    /// 用当前笔刷填满选区（**整次 = 一步撤销**）；返回真改了几格。
    pub fn fill_selection(&mut self, map: &mut Map) -> usize {
        self.with_map(map, |core| core.fill_selection())
    }

    /// 最近一次「清空/剪切选区」真清掉了几格（内核记账，与回执里的数同源）。
    pub fn last_clear(&self) -> Option<usize> {
        self.core.last_clear()
    }

    /// **仅测试用**：把内核文档换成给定的字节（返回是否解析成功）。
    ///
    /// # 为什么还需要它（2026-11-05 九格式收口后）
    ///
    /// ⚠ 上一版这里写着"GUI 打不开 Type5 文件，只能注入"。**那句话已经不成立**：
    /// 装载走 `miredit_core::Document::load`，九种格式都能开（真装载路径的验收见
    /// `acceptance_tests/formats.rs` 的门 4）。
    ///
    /// 保留它的理由只剩一条：`selection.rs` 的门 6 验的是**内核 `plan_stamp` 的 Type5 闸门**
    /// （贴带地表的内容要被拒）。那一门要的是"会话档位是 Type5"这一个输入，
    /// 不需要真的落一个 Type5 文件，也不需要走宿主装载路径 —— 用注入把两件事解耦。
    ///
    /// # 为什么注入能存活
    ///
    /// [`Self::sync_document_format`] 的早退条件是
    /// `view_cell_format(doc.format_id()) == map.format`，而
    /// `view_cell_format(Mir3Wemade) == New14`（core `convert.rs` 的档位映射表）
    /// ⇒ 宿主地图只要是 `New14`，注入的 `Mir3Wemade` 就不会被重建掉。
    #[cfg(test)]
    pub(crate) fn inject_document(&mut self, bytes: Vec<u8>) -> bool {
        match miredit_core::Document::from_bytes(Path::new(占位路径), bytes) {
            Ok(doc) => {
                *self.core.document_mut() = doc;
                true
            }
            Err(_) => false,
        }
    }
}

// ───────────────────────── 脚本驱动（GUI 与无窗取证共用） ─────────────────────────

/// 一条编辑脚本指令（命令行 `--edit-*` 解析出来的就是它）。
///
/// 与 `miredit_core::cmd::Command` 的关系：能一一对上的（`Layer` / `Pickup` / `Brush` /
/// `Erase` / `Stroke` / `Size` / `Undo` / `Redo`）在 [`apply_script`] 里**翻译成 `Command`
/// 后调 `miredit_core::apply`**；`Fill` 因为要拿到 [`FillOutcome`] 才能区分
/// 「已是目标值」与「被拒绝」（`Outcome` 把两者都折成 `changed == 0`），
/// 保留直调会话；`Check` 是宿主诊断（要磁盘原文件字节），core 里没有对应命令。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditCmd {
    /// 切换**当前编辑层**（脚本中途换层，验收要在一个脚本里跑完三层）。
    Layer(EditLayer),
    /// 吸管：`(x, y)`。
    Pickup(u16, u16),
    /// 画笔：`(x, y)`（**按当前笔刷尺寸**一步；尺寸 = 3 时就是 3×3 = 9 格）。
    Brush(u16, u16),
    /// 橡皮：`(x, y)`（**按当前笔刷尺寸**一步）。
    Erase(u16, u16),
    /// 拖动：`(x0, y0) → (x1, y1)`（Bresenham 插值，整条轨迹按 N×N 落，**一步撤销**）。
    Stroke((u16, u16), (u16, u16)),
    /// 填充：`(x, y)`（BFS 连通块，**整次填充 = 一步撤销**）。
    Fill(u16, u16),
    /// 设笔刷尺寸（`[` / `]` 的脚本版；夹进 3..=31 的奇数）。
    Size(usize),
    /// 撤销 `n` 步。
    Undo(u32),
    /// 重做 `n` 步。
    Redo(u32),
    /// 打印一次「当前内存地图 vs 磁盘原文件」的逐字节比对。
    Check,
}

/// 脚本跑完后的摘要。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScriptReport {
    /// 一共改了几格。
    pub edited_cells: usize,
    /// 一共产生了多少步。
    pub steps: usize,
    /// 结束时撤销栈深度。
    pub undo: usize,
    /// 结束时重做栈深度。
    pub redo: usize,
    /// 结束时是否有未保存修改。
    pub dirty: bool,
}

/// 逐条执行编辑脚本，并把每条的结果写到 stderr（文案全部来自 [`crate::ui_text`]）。
///
/// `doc` 里没有地图时立刻停止（并把原因如实打出来），绝不假装改了。
pub fn apply_script(
    doc: &mut crate::mapdoc::DocState,
    ed: &mut EditSession,
    cmds: &[EditCmd],
) -> ScriptReport {
    use miredit_core::Command;

    let Some(path) = doc.doc.as_ref().map(|d| d.path.clone()) else {
        eprintln!("{}", tx::DIAG_EDIT_NO_MAP);
        return ScriptReport::default();
    };
    let orig = std::fs::read(&path).ok();
    let mut rep = ScriptReport::default();

    for cmd in cmds {
        match *cmd {
            EditCmd::Layer(layer) => {
                // → `Command::SetLayer`（层名走 `EditLayer::from_key`，不认识就报错、不猜）
                ed.apply_cmd_in(doc, Command::SetLayer {
                    layer: layer.key().to_string(),
                });
                // 阻挡层没有库号字节可写 ⇒ 单独一句，不套「落笔 {img} + {lib}」
                if layer.is_collision() {
                    eprintln!(
                        "{}",
                        tx::fill(
                            tx::DIAG_EDIT_LAYER_COLLISION,
                            &[("layer", layer.name()), ("img", layer.img_field())]
                        )
                    );
                } else {
                    eprintln!(
                        "{}",
                        tx::fill(
                            tx::DIAG_EDIT_LAYER,
                            &[
                                ("layer", layer.name()),
                                ("img", layer.img_field()),
                                ("lib", layer.lib_field().unwrap_or(tx::VAL_UNKNOWN)),
                            ]
                        )
                    );
                }
            }
            EditCmd::Pickup(x, y) => {
                ed.select(TOOL_PICKUP_IDX);
                // 整格三族（诊断用）用只读路径；笔刷走 `Command::Pick`
                let picked_cell = match doc.doc.as_ref() {
                    Some(d) => ed.pick_cell(&d.map, x, y),
                    None => None,
                };
                ed.apply_cmd_in(doc, Command::Pick { x, y });
                let picked = ed.brush();
                match picked {
                    Some(b) => {
                        let lib = brush_lib_text(doc.doc.as_ref().map(|d| d.map.format), &b);
                        eprintln!(
                            "{}",
                            tx::fill(
                                tx::DIAG_EDIT_PICK,
                                &[
                                    ("x", &x.to_string()),
                                    ("y", &y.to_string()),
                                    ("fam", b.family_name()),
                                    ("img", &b.sprite().to_string()),
                                    ("lib", &lib),
                                ]
                            )
                        );
                        // 整格三族也打一行：吸管读整格、但笔刷只取当前层那一个字段
                        if let Some(pc) = picked_cell {
                            eprintln!(
                                "{}",
                                tx::fill(
                                    tx::DIAG_EDIT_PICK_CELL,
                                    &[
                                        ("x", &x.to_string()),
                                        ("y", &y.to_string()),
                                        ("bk", &pc.bk_img.to_string()),
                                        ("mid", &pc.mid_img.to_string()),
                                        ("fr", &pc.fr_img.to_string()),
                                    ]
                                )
                            );
                        }
                    }
                    None => eprintln!(
                        "{}",
                        tx::fill(
                            tx::DIAG_EDIT_PICK_NONE,
                            &[("x", &x.to_string()), ("y", &y.to_string())]
                        )
                    ),
                }
            }
            EditCmd::Brush(x, y) => {
                ed.select(TOOL_BRUSH_IDX);
                // → `Command::Paint`（= `paint_stroke_once`，与鼠标落笔同一条路径）
                let out = ed.apply_cmd_in(doc, Command::Paint { x, y });
                let n = out.changed;
                rep.edited_cells += n;
                if n > 0 {
                    rep.steps += 1;
                }
                eprintln!(
                    "{}",
                    tx::fill(
                        tx::DIAG_EDIT_BRUSH,
                        &[
                            ("x", &x.to_string()),
                            ("y", &y.to_string()),
                            ("n", &n.to_string()),
                        ]
                    )
                );
            }
            EditCmd::Erase(x, y) => {
                // → `Command::Erase`（内核里先切橡皮、再按当前尺寸落一笔）
                let out = ed.apply_cmd_in(doc, Command::Erase { x, y });
                let n = out.changed;
                rep.edited_cells += n;
                if n > 0 {
                    rep.steps += 1;
                }
                eprintln!(
                    "{}",
                    tx::fill(
                        tx::DIAG_EDIT_ERASE,
                        &[
                            ("x", &x.to_string()),
                            ("y", &y.to_string()),
                            ("n", &n.to_string()),
                        ]
                    )
                );
            }
            EditCmd::Stroke((x0, y0), (x1, y1)) => {
                ed.select(TOOL_BRUSH_IDX);
                // → `Command::Stroke`（Bresenham 插值，整条轨迹一步）
                let out = ed.apply_cmd_in(
                    doc,
                    Command::Stroke {
                        from: (x0, y0),
                        to: (x1, y1),
                    },
                );
                let n = out.changed;
                rep.edited_cells += n;
                if n > 0 {
                    rep.steps += 1;
                }
                eprintln!(
                    "{}",
                    tx::fill(
                        tx::DIAG_EDIT_STROKE,
                        &[
                            ("x0", &x0.to_string()),
                            ("y0", &y0.to_string()),
                            ("x1", &x1.to_string()),
                            ("y1", &y1.to_string()),
                            ("n", &ed.brush_size().to_string()),
                            ("cells", &n.to_string()),
                        ]
                    )
                );
            }
            EditCmd::Fill(x, y) => {
                ed.select(TOOL_BUCKET_IDX);
                // 直调会话（不翻 `Command::Fill`）：stderr 要区分「已是目标值」与「被拒绝」，
                // 而 `Outcome` 把两者都折成 `changed == 0`（见 [`EditCmd`] 的文档）。
                let out = match doc.doc.as_mut() {
                    Some(d) => ed.fill(&mut d.map, x, y),
                    None => FillOutcome::NoBrush,
                };
                let n = out.cells();
                rep.edited_cells += n;
                if n > 0 {
                    rep.steps += 1;
                }
                if out.is_refusal() {
                    eprintln!(
                        "{}",
                        tx::fill(
                            tx::DIAG_EDIT_FILL_REFUSED,
                            &[("why", ed.note().unwrap_or(tx::VAL_UNKNOWN))]
                        )
                    );
                } else {
                    eprintln!(
                        "{}",
                        tx::fill(
                            tx::DIAG_EDIT_FILL,
                            &[
                                ("x", &x.to_string()),
                                ("y", &y.to_string()),
                                ("n", &n.to_string()),
                                ("layer", ed.layer().name()),
                            ]
                        )
                    );
                }
            }
            EditCmd::Size(n) => {
                // → `Command::SetBrushSize`（尺寸 > u16::MAX 时钳到 u16::MAX，规范化后仍是 31）
                let size = u16::try_from(n).unwrap_or(u16::MAX);
                ed.apply_cmd_in(doc, Command::SetBrushSize { size });
                let got = ed.brush_size();
                eprintln!(
                    "{}",
                    tx::fill(tx::DIAG_EDIT_SIZE, &[("n", &got.to_string())])
                );
            }
            EditCmd::Undo(n) => {
                let mut cells = 0;
                let mut steps = 0;
                for _ in 0..n {
                    // → `Command::Undo`（一次一步，与 `--edit-undo N` 的循环等价）
                    let out = ed.apply_cmd_in(doc, Command::Undo);
                    if out.changed == 0 {
                        break;
                    }
                    cells += out.changed;
                    steps += 1;
                }
                eprintln!(
                    "{}",
                    tx::fill(
                        tx::DIAG_EDIT_UNDO,
                        &[
                            ("n", &steps.to_string()),
                            ("cells", &cells.to_string()),
                            ("undo", &ed.undo_len().to_string()),
                            ("redo", &ed.redo_len().to_string()),
                            ("dirty", dirty_text(ed.dirty())),
                        ]
                    )
                );
            }
            EditCmd::Redo(n) => {
                let mut cells = 0;
                let mut steps = 0;
                for _ in 0..n {
                    let out = ed.apply_cmd_in(doc, Command::Redo);
                    if out.changed == 0 {
                        break;
                    }
                    cells += out.changed;
                    steps += 1;
                }
                eprintln!(
                    "{}",
                    tx::fill(
                        tx::DIAG_EDIT_REDO,
                        &[
                            ("n", &steps.to_string()),
                            ("cells", &cells.to_string()),
                            ("undo", &ed.undo_len().to_string()),
                            ("redo", &ed.redo_len().to_string()),
                            ("dirty", dirty_text(ed.dirty())),
                        ]
                    )
                );
            }
            EditCmd::Check => print_check(doc, orig.as_deref(), &path),
        }
    }

    rep.undo = ed.undo_len();
    rep.redo = ed.redo_len();
    rep.dirty = ed.dirty();
    rep
}

impl EditSession {
    /// 脚本专用：把一条命令作用在 `doc.doc` 的地图上（没有地图 ⇒ 返回 0 格回执）。
    fn apply_cmd_in(
        &mut self,
        doc: &mut crate::mapdoc::DocState,
        cmd: miredit_core::Command,
    ) -> miredit_core::Outcome {
        match doc.doc.as_mut() {
            Some(d) => self.apply_cmd(&mut d.map, cmd),
            // 没有地图：什么都不做，但也要给一份"改了 0 格 + 原因"的回执（不静默）
            None => miredit_core::Outcome {
                changed: 0,
                note: Some(tx::DIAG_EDIT_NO_MAP.to_owned()),
                dirty: self.dirty(),
                can_undo: self.undo_len() > 0,
                can_redo: self.redo_len() > 0,
            },
        }
    }
}

/// 打印一次「内存地图 vs 磁盘原文件」比对（差异按格归并）。
///
/// ⚠ **2026-11-05 九格式收口**：折格口径从 `mir2_map` 的三档（`byte_diff(&Map, &[u8])`，
/// 头部固定 52、格长取 `map.format.cell_size()`）换成 [`crate::mapdoc::MapDoc::diff`]
/// （= `Document::diff`，按**真格式的头部长度与磁盘格长**折格）。
/// 不改的话：Type5 / CrystalCs 上这条诊断会把差异报成"落在第几个 12/14/36 字节格"，
/// 那是**假数据** —— 而 `--edit-check` 存在的意义就是给验收一个可信的数字。
pub fn print_check(doc: &mut crate::mapdoc::DocState, orig: Option<&[u8]>, path: &std::path::Path) {
    let Some(d) = doc.doc.as_mut() else {
        eprintln!("{}", tx::DIAG_EDIT_NO_MAP);
        return;
    };
    let Some(orig) = orig else {
        eprintln!(
            "{}",
            tx::fill(
                tx::DIAG_EDIT_CMP_ERR,
                &[
                    ("path", &path.display().to_string()),
                    ("err", tx::VAL_UNKNOWN)
                ]
            )
        );
        return;
    };
    let cell_len = d.disk_cell_len().unwrap_or(1).max(1);
    let diff = d.diff();
    if diff.same() {
        eprintln!(
            "{}",
            tx::fill(tx::DIAG_EDIT_CMP_SAME, &[("len", &orig.len().to_string())])
        );
    } else {
        eprintln!(
            "{}",
            tx::fill(
                tx::DIAG_EDIT_CMP_DIFF,
                &[
                    ("bytes", &diff.bytes.to_string()),
                    ("cells", &diff.cells.to_string()),
                    ("size", &cell_len.to_string()),
                    ("off", &diff.first.unwrap_or(0).to_string()),
                    ("head", &diff.header.to_string()),
                ]
            )
        );
    }
}

// ───────────────────────── 测试 ─────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use mir2_map::HEADER_LEN;

    /// 资产根缺省值：本机真实根（**整个测试模块里只出现这一处**）。换机器用 `MIREDIT_ASSET_ROOT`。
    const 缺省资产根: &str = r"E:\GodM2";

    /// 资产根：`MIREDIT_ASSET_ROOT` → 缺省 [`缺省资产根`]。
    ///
    /// 根目录不存在（公开仓/PR 的机器上没有游戏素材）⇒ `None`，调用方打印 `[skip]` 并返回。
    /// 只做 `var_os` + `PathBuf`，不引入任何第三方库。
    fn 资产根() -> Option<std::path::PathBuf> {
        let root = std::env::var_os("MIREDIT_ASSET_ROOT")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(缺省资产根));
        root.is_dir().then_some(root)
    }

    /// `<资产根>/GodotClient/maps`（12/14/36 三档的现役语料）。
    fn 语料_a(root: &Path) -> std::path::PathBuf {
        root.join("GodotClient").join("maps")
    }

    /// `<资产根>/client/热血传奇23周年客户端/Map`（1,097 张全量语料）。
    fn 语料_b(root: &Path) -> std::path::PathBuf {
        root.join("client").join("热血传奇23周年客户端").join("Map")
    }

    /// 测试专用：`EditLayer + 族内索引` → 宿主素材面板的图库定位键。
    ///
    /// 判据（层 → 族）仍然只有内核的 `lib_ref_of` 一处；这里只把结果换成
    /// `mir2_assets::LibRef`，因为 `EditSession::set_brush_from_palette` 的签名
    /// 收的是它（素材面板与 `app.rs` 都用那一套）。
    fn 图库(layer: EditLayer, index: u16) -> mir2_assets::LibRef {
        assets_lib(lib_ref_of(layer, index))
    }

    fn 造图(w: u16, h: u16, fmt: CellFormat) -> Map {
        let size = fmt.cell_size();
        let mut bytes = vec![0u8; HEADER_LEN + size * (w as usize * h as usize)];
        bytes[0..2].copy_from_slice(&w.to_le_bytes());
        bytes[2..4].copy_from_slice(&h.to_le_bytes());
        // 每格给一个可区分的初始内容（图号 = 格序 + 1，中层/物件层也带值，便于断言"只改地表"）
        for i in 0..(w as usize * h as usize) {
            let base = HEADER_LEN + i * size;
            let img = (i as u16) + 1;
            bytes[base..base + 2].copy_from_slice(&img.to_le_bytes());
            bytes[base + 2..base + 4].copy_from_slice(&(img + 100).to_le_bytes());
            bytes[base + 4..base + 6].copy_from_slice(&(img + 200).to_le_bytes());
            bytes[base + 6] = 7; // 门索引
            bytes[base + 11] = 9; // 光照
            if fmt.has_lib_bytes() {
                bytes[base + 12] = 3; // btTiles
                bytes[base + 13] = 4; // btsmTiles
            }
            if fmt.has_tail() {
                for (j, b) in bytes[base + 14..base + 36].iter_mut().enumerate() {
                    *b = 0xA0 + j as u8;
                }
            }
        }
        Map::parse(&bytes).expect("合成地图应能解析")
    }

    /// 列优先下标必须与 `Map::cell` 同口径（否则改格会改到别的格上）。
    #[test]
    fn 下标与取格同源() {
        let map = 造图(4, 3, CellFormat::New14);
        for y in 0..3u16 {
            for x in 0..4u16 {
                let idx = index_of(&map, x, y).expect("图内");
                assert_eq!(idx, x as usize * 3 + y as usize);
                assert_eq!(&map.cells[idx], map.cell(x, y).unwrap());
            }
        }
        assert_eq!(index_of(&map, 4, 0), None);
        assert_eq!(index_of(&map, 0, 3), None);
    }

    /// **吸管取值**：把该格 `bk_img`（含阻挡位）与 `btTiles` 原样取下。
    #[test]
    fn 吸管取到图号与库号() {
        let mut map = 造图(3, 3, CellFormat::New14);
        let idx = index_of(&map, 1, 2).unwrap();
        map.cells[idx].bk_img = 0x8000 | 901;
        map.cells[idx].tiles = 4;
        let mut ed = EditSession::default();
        let b = ed.pick_at(&map, 1, 2).expect("图内格应能取");
        assert_eq!(b.family, Family::Ground, "缺省当前层是地表层 ⇒ 笔刷族=地面");
        assert_eq!(b.img, 0x8000 | 901);
        assert_eq!(b.sprite(), 901, "图号要去掉阻挡位");
        assert_eq!(b.lib_byte, 4);
        assert!(b.blocked(), "阻挡位必须一起取下来");
        assert_eq!(b.from, Some((1, 2)));
        assert_eq!(ed.brush(), Some(b));
        // 吸管不改地图、不进撤销栈
        assert_eq!(ed.undo_len(), 0);
        assert!(!ed.dirty());
    }

    /// 越界吸管：拿不到笔刷，也不改已有笔刷（不静默清空）。
    #[test]
    fn 越界吸管不动笔刷() {
        let map = 造图(2, 2, CellFormat::Old12);
        let mut ed = EditSession::default();
        assert_eq!(ed.pick_at(&map, 9, 9), None);
        assert_eq!(ed.brush(), None);
        ed.pick_at(&map, 0, 0);
        let before = ed.brush();
        assert_eq!(ed.pick_at(&map, 5, 5), None);
        assert_eq!(ed.brush(), before);
    }

    /// **改前必红用例**：在 `bk_img=0x815F`（带低阻挡位）的格上写纯图号 10，
    /// 结果必须是 `0x800A`（只换低 15 位、bit15 原样），不是 `0x000A`。
    ///
    /// 真值：`CrystalStudio/CrystalStudio/MapViewport.cs:480`
    /// `c.BackImage = (c.BackImage & 0x60000000) | image;`（画地表保留碰撞位）；
    /// `:520` 填充同款；`:546` 清除选区只留碰撞位。C# 内存模型里 `0x60000000`
    /// 就是低/高两位阻挡（`:23` `ActiveCollisionMask = 0x20000000 / 0x40000000`），
    /// 我们文件级等价位是 `bk_img` / `fr_img` 的 `bit15`（`0x8000`，
    /// `mir2-map/src/cell.rs:205-212` `Cell::is_walkable`、
    /// `client/reference_full/Classes/map/TMap.cpp:339` `CanWalk`）。
    /// 旧实现 `set_img(..., b.img)` 把整 16 位覆盖 ⇒ `0x815F → 0x000A`，
    /// 等于「改个地表顺手拆墙」；本用例在旧逻辑下必须 FAILED。
    #[test]
    fn 画笔写图号保留地表bit15必红() {
        let mut map = 造图(3, 3, CellFormat::New14);
        let idx = index_of(&map, 1, 1).unwrap();
        map.cells[idx].bk_img = 0x815F;
        assert!(!map.walkable(1, 1), "带 bit15 的格本来不可走");
        let mut ed = EditSession::default();
        ed.set_layer(EditLayer::Ground);
        // 帧索引 9 ⇒ 图号 10（纯图号，bit15 = 0）
        ed.set_brush_from_palette(mir2_assets::LibRef::ground(3), 9)
            .expect("合法帧应能设为笔刷");
        assert_eq!(ed.apply_once(&mut map, 1, 1), 1, "应真的改一格");
        assert_eq!(
            map.cells[idx].bk_img, 0x800A,
            "bit15 必须保留：0x815F 写图号 10 应得 0x800A"
        );
        assert_eq!(map.cells[idx].bk_img & SPRITE_MAX, 10, "低 15 位应换成 10");
        assert!(!map.walkable(1, 1), "改图号不许改变可走性");
    }

    /// **画笔改格**：只改地表（`bk_img` / 14 档的 `btTiles`），其它字段逐字段不变。
    ///
    /// ⚠ **2026-11-04 按正确语义订正**（不是放宽断言）：旧断言要求把
    /// `0x8001` 笔刷（吸自带阻挡的源格）盖到可走目标格后 `bk_img == 0x8001`，
    /// 等于把可走格静默改成不可走 —— 那是本单 bug 的**反向表现**。
    /// 现判据改为：目标格 `bit15` 前后不变（仍可走）、只把低 15 位换成笔刷图号 1，
    /// 并额外钉住 `walkable` 前后一致。依据同 [`画笔写图号保留地表bit15必红`]。
    #[test]
    fn 画笔只改地表层() {
        let mut map = 造图(3, 3, CellFormat::New14);
        let mut ed = EditSession::default();
        ed.select(TOOL_BRUSH_IDX);
        let src = index_of(&map, 0, 0).unwrap();
        map.cells[src].bk_img = 0x8001;
        map.cells[src].tiles = 5;
        let b = ed.pick_at(&map, 0, 0).unwrap();
        assert_eq!(b.img, 0x8001, "吸管确实取到了带阻挡位的源格内容");

        let idx = index_of(&map, 2, 1).unwrap();
        let before = map.cells[idx];
        assert!(before.bk_img & COLLISION_BIT == 0, "目标格本来可走");
        assert!(map.walkable(2, 1), "目标格本来可走");
        assert_eq!(ed.apply_once(&mut map, 2, 1), 1, "应真的改了一格");
        let after = map.cells[idx];

        assert_eq!(
            after.bk_img, 1,
            "只写图号 1：笔刷自带的 bit15 不许贴到目标格上"
        );
        assert_eq!(
            after.bk_img & COLLISION_BIT,
            before.bk_img & COLLISION_BIT,
            "目标格 bit15 前后必须相同"
        );
        assert_eq!(after.bk_img & SPRITE_MAX, b.img & SPRITE_MAX, "低 15 位换成笔刷图号");
        assert!(map.walkable(2, 1), "改图号不许改变可走性");
        assert_eq!(after.tiles, b.lib_byte, "14 档应一并写 btTiles");
        // 其余字段逐条比对
        assert_eq!(after.mid_img, before.mid_img, "中层不许动");
        assert_eq!(after.fr_img, before.fr_img, "物件层不许动");
        assert_eq!(after.door_index, before.door_index);
        assert_eq!(after.door_offset, before.door_offset);
        assert_eq!(after.ani_frame, before.ani_frame);
        assert_eq!(after.ani_tick, before.ani_tick);
        assert_eq!(after.area, before.area);
        assert_eq!(after.light, before.light);
        assert_eq!(after.sm_tiles, before.sm_tiles, "btsmTiles 不许动");
        assert_eq!(after.tail, before.tail);
    }

    /// `Old12` 档：`tiles` 在这个档位根本没有字节，画笔不能"顺手"写出第 13 个字节
    /// （`to_bytes` 长度必须不变）。
    #[test]
    fn 画笔在old12档不动库号字节() {
        let mut map = 造图(2, 2, CellFormat::Old12);
        let len_before = map.to_bytes().len();
        let mut ed = EditSession::default();
        map.cells[0].bk_img = 7;
        map.cells[0].tiles = 9; // 人为塞一个值，确认它不会被写出去
        ed.pick_at(&map, 0, 0);
        ed.apply_once(&mut map, 1, 1);
        assert_eq!(map.to_bytes().len(), len_before, "Old12 档文件长度不许变");
        assert_eq!(map.cell(1, 1).unwrap().bk_img, 7);
    }

    /// **橡皮清零**：只把图号清成 0，**保留 0x8000 阻挡位**。
    #[test]
    fn 橡皮只清图号保留阻挡位() {
        let mut map = 造图(2, 2, CellFormat::Old12);
        let idx = index_of(&map, 0, 1).unwrap();
        map.cells[idx].bk_img = 0x8000 | 1234;
        let mut ed = EditSession::default();
        ed.select(TOOL_ERASER_IDX);
        assert_eq!(ed.apply_once(&mut map, 0, 1), 1);
        assert_eq!(map.cells[idx].bk_img, 0x8000, "阻挡位必须留着");
        assert_eq!(map.cells[idx].bk_img & 0x7FFF, 0, "图号必须清零");
        assert!(!map.walkable(0, 1), "擦图号不等于拆墙");
    }

    /// 画笔还没取内容时落笔：一格都不改（不静默写成 0 号图）。
    #[test]
    fn 没取笔刷时画笔不落格() {
        let mut map = 造图(2, 2, CellFormat::Old12);
        let before = map.to_bytes();
        let mut ed = EditSession::default();
        ed.select(TOOL_BRUSH_IDX);
        assert_eq!(ed.apply_once(&mut map, 0, 0), 0);
        assert_eq!(map.to_bytes(), before);
        assert_eq!(ed.undo_len(), 0);
        assert!(!ed.dirty());
    }

    /// 未接线工具：落到画布上什么都不做（"点了没反应"必须是真的没反应）。
    ///
    /// ⚠ 下标 2 已**不再是**未接线工具（本版接了填充，见 [`TOOL_BUCKET_IDX`]），
    /// 所以这里的清单里没有 2 —— 它现在会返回 `true`。
    #[test]
    fn 未接线工具不改地图() {
        let mut map = 造图(2, 2, CellFormat::New14);
        let before = map.to_bytes();
        let mut ed = EditSession::default();
        for i in [1usize, 4, 5, 7, 8, 9] {
            assert!(!ed.select(i), "下标 {i} 本单未接线");
            ed.begin();
            assert_eq!(ed.touch(&mut map, 0, 0), 0);
            assert_eq!(ed.end(), 0);
        }
        assert_eq!(map.to_bytes(), before);
        assert!(!ed.dirty());
        // 已接线的四个：0 画笔 / 2 填充 / 3 橡皮 / 6 吸管
        for i in [0usize, 2, 3, 6] {
            assert!(ed.select(i), "下标 {i} 必须已接线");
        }
    }

    /// **一次拖动只算一步**：经过 3 格 ⇒ 撤销栈 1 步、含 3 条变更；撤销一次全还原。
    #[test]
    fn 一次拖动算一步() {
        let mut map = 造图(4, 4, CellFormat::New14);
        // ⚠ 先把"笔刷来源格"的内容定好，再拍原样快照 —— 否则快照里是旧值，
        //   撤销回到的"原样"与快照本来就不是一回事（这条测试曾经因此假红）
        map.cells[0].bk_img = 42;
        let orig = map.to_bytes();
        let mut ed = EditSession::default();
        ed.pick_at(&map, 0, 0);

        ed.begin();
        assert_eq!(ed.touch(&mut map, 1, 1), 1);
        assert_eq!(ed.touch(&mut map, 2, 1), 1);
        assert_eq!(ed.touch(&mut map, 3, 1), 1);
        // 同一格重复经过：不再记第二次
        assert_eq!(ed.touch(&mut map, 3, 1), 0);
        assert_eq!(ed.end(), 3, "本步应记 3 格");

        assert_eq!(ed.undo_len(), 1, "一次拖动只占一层撤销");
        assert!(ed.dirty());
        assert_eq!(ed.undo(&mut map), 3);
        assert_eq!(map.to_bytes(), orig, "撤销一次应逐字节回到原样");
        assert_eq!(ed.redo_len(), 1);
    }

    /// **撤销 / 重做逐字节相等**：改 3 格 → 撤销 ×3 → 与原文件逐字节相等；
    /// 再重做 ×3 → 与"保存后的文件"逐字节相等。
    #[test]
    fn 撤销重做逐字节还原() {
        let mut map = 造图(5, 5, CellFormat::New14);
        // 同上：先定好笔刷来源格，再拍原样快照
        map.cells[0].bk_img = 0x8000 | 901;
        map.cells[0].tiles = 4;
        let orig = map.to_bytes();
        let mut ed = EditSession::default();
        ed.pick_at(&map, 0, 0);

        for (x, y) in [(1u16, 1u16), (2, 2), (3, 3)] {
            assert_eq!(ed.apply_once(&mut map, x, y), 1);
        }
        let edited = map.to_bytes();
        assert_ne!(edited, orig, "改完必须与原文件不同");

        for _ in 0..3 {
            assert_eq!(ed.undo(&mut map), 1);
        }
        assert_eq!(map.to_bytes(), orig, "撤销 3 步后必须与原文件逐字节相等");
        assert!(!ed.dirty(), "撤销回保存点后脏标记必须自动清零");

        for _ in 0..3 {
            assert_eq!(ed.redo(&mut map), 1);
        }
        assert_eq!(map.to_bytes(), edited, "重做 3 步后必须与保存后的文件逐字节相等");
        assert!(ed.dirty(), "重做回改动点后又是未保存状态");
    }

    /// **脏标记**：新建会话不脏；改一格变脏；保存后变干净；再撤销又变脏。
    #[test]
    fn 脏标记随保存与撤销变化() {
        let mut map = 造图(2, 2, CellFormat::Old12);
        let mut ed = EditSession::default();
        assert!(!ed.dirty(), "刚打开时不该脏");
        map.cells[0].bk_img = 3;
        ed.pick_at(&map, 0, 0);
        ed.apply_once(&mut map, 1, 1);
        assert!(ed.dirty(), "改过就该脏");

        ed.mark_saved();
        ed.note_save_ok();
        assert!(!ed.dirty(), "保存后必须变干净");
        assert_eq!(ed.save_error(), None);

        ed.undo(&mut map);
        assert!(ed.dirty(), "撤销到保存点之前又该脏");
        ed.redo(&mut map);
        assert!(!ed.dirty(), "重做回保存点应重新变干净");
    }

    /// 保存失败的原因要能原样带出来（状态栏要显示它）。
    #[test]
    fn 保存失败原因保留() {
        let mut ed = EditSession::default();
        assert_eq!(ed.save_error(), None, "没按过保存不该有错误");
        ed.note_save_err("磁盘满".to_string());
        assert_eq!(ed.save_error(), Some("磁盘满"));
        ed.mark_saved();
        assert!(!ed.dirty());
        assert_eq!(ed.save_error(), Some("磁盘满"), "失败原因不会因为保存成功就消失");
    }

    /// 撤销栈上限 100 步：第 101 步把最老的一步挤掉。
    #[test]
    fn 撤销栈上限一百步() {
        let mut map = 造图(2, 2, CellFormat::Old12);
        let mut ed = EditSession::default();
        map.cells[0].bk_img = 1;
        ed.pick_at(&map, 0, 0);
        // 交替改两格，保证每次都真的产生变化
        for i in 0..(UNDO_LIMIT + 20) {
            let (x, y) = if i % 2 == 0 { (0u16, 1u16) } else { (1, 1) };
            let idx = index_of(&map, x, y).unwrap();
            map.cells[idx].bk_img = (i as u16 % 7) + 2;
            ed.apply_once(&mut map, x, y);
        }
        assert_eq!(ed.undo_len(), UNDO_LIMIT, "撤销栈只保留最近 100 步");
    }

    /// 新的一步会清空"可重做"（否则重做会把另一条分支的内容贴回来）。
    #[test]
    fn 新的一步清空重做栈() {
        let mut map = 造图(3, 3, CellFormat::Old12);
        let mut ed = EditSession::default();
        map.cells[0].bk_img = 11;
        ed.pick_at(&map, 0, 0);
        ed.apply_once(&mut map, 1, 0);
        ed.undo(&mut map);
        assert_eq!(ed.redo_len(), 1);
        map.cells[0].bk_img = 22;
        ed.pick_at(&map, 0, 0);
        ed.apply_once(&mut map, 2, 0);
        assert_eq!(ed.redo_len(), 0, "新的一步应让可重做作废");
    }

    /// 值没变的一笔不算一步（避免"空刷一遍"污染撤销栈）。
    #[test]
    fn 值没变不记步() {
        let mut map = 造图(2, 2, CellFormat::New14);
        let mut ed = EditSession::default();
        ed.pick_at(&map, 0, 0); // 笔刷 = (0,0) 的内容
        // 把 (1,1) 先改成与 (0,0) 相同
        let idx = index_of(&map, 1, 1).unwrap();
        map.cells[idx] = map.cells[index_of(&map, 0, 0).unwrap()];
        assert_eq!(ed.apply_once(&mut map, 1, 1), 0);
        assert_eq!(ed.undo_len(), 0);
        assert!(!ed.dirty());
    }

    /// **笔画没结算时也算脏**：窗口失焦漏了"松开"事件，字节已写进内存却没有撤销步，
    /// 此时 `dirty` 必须为真 —— 否则关窗不提示，改动静默丢失。
    #[test]
    fn 未结算的笔画也算脏() {
        let mut map = 造图(3, 3, CellFormat::New14);
        let mut ed = EditSession::default();
        map.cells[0].bk_img = 77;
        ed.pick_at(&map, 0, 0);
        assert!(!ed.dirty());
        ed.begin();
        assert_eq!(ed.touch(&mut map, 2, 2), 1);
        assert!(ed.dirty(), "笔画还没结算也必须算脏");
        assert_eq!(ed.undo_len(), 0, "还没结算就没有撤销步");
        assert_eq!(ed.end(), 1, "结算后才算一步");
        assert_eq!(ed.undo_len(), 1);
    }

    /// 上一笔没结算就开新的一笔：**先把上一笔结算掉**，那批字节不会变成孤儿。
    #[test]
    fn 重开一笔会先结算上一笔() {
        let mut map = 造图(3, 3, CellFormat::New14);
        let mut ed = EditSession::default();
        map.cells[0].bk_img = 88;
        ed.pick_at(&map, 0, 0);
        ed.begin();
        assert_eq!(ed.touch(&mut map, 1, 1), 1);
        // 没调 end 就开新的一笔
        ed.begin();
        assert_eq!(ed.undo_len(), 1, "上一笔必须被自动结算");
        assert_eq!(ed.touch(&mut map, 2, 2), 1);
        assert_eq!(ed.end(), 1);
        assert_eq!(ed.undo_len(), 2);
        // 两步都能撤销：撤销两次应回到原样
        assert_eq!(ed.undo(&mut map), 1);
        assert_eq!(ed.undo(&mut map), 1);
        assert!(!ed.dirty(), "撤销回原样后不该脏");
        // (1,1) 的原始图号：列优先下标 1*3+1 = 4 ⇒ 5（造图时 img = idx + 1）
        assert_eq!(map.cell(1, 1).unwrap().bk_img, 5);
    }

    /// Bresenham 插值：含首尾、不重复、跨度正确（斜线也要连起来）。
    #[test]
    fn 划线插值覆盖路径() {
        assert_eq!(line_cells((0, 0), (2, 0)), vec![(0, 0), (1, 0), (2, 0)]);
        assert_eq!(line_cells((3, 3), (3, 3)), vec![(3, 3)]);
        let diag = line_cells((0, 0), (3, 3));
        assert_eq!(diag.len(), 4);
        assert_eq!(diag.first(), Some(&(0, 0)));
        assert_eq!(diag.last(), Some(&(3, 3)));
        let rev = line_cells((5, 5), (2, 5));
        assert_eq!(rev.first(), Some(&(5, 5)));
        assert_eq!(rev.last(), Some(&(2, 5)));
    }

    /// 逐字节差异统计：只改 2 格 ⇒ 差异落在 2 个格区间内，头部 0 字节差异。
    #[test]
    fn 差异按格统计() {
        let mut map = 造图(4, 4, CellFormat::Ext36);
        let orig = map.to_bytes();
        assert!(byte_diff(&map, &orig).same());

        let a = index_of(&map, 1, 2).unwrap();
        let b = index_of(&map, 3, 0).unwrap();
        map.cells[a].bk_img = 0x1234;
        map.cells[b].bk_img = 0x5678;
        let d = byte_diff(&map, &orig);
        assert_eq!(d.cells, 2, "应只涉及 2 个格");
        assert_eq!(d.header, 0, "52 字节头不许变");
        assert_eq!(d.bytes, 4, "每格只改 bk_img 的 2 字节");
        let first = d.first.unwrap();
        assert!(first >= HEADER_LEN);
    }

    /// 库号显示：`Old12` 档如实 `—`，14/36 档给数字。
    #[test]
    fn 库号显示按档位() {
        assert_eq!(lib_text(Some(CellFormat::Old12), 7), tx::VAL_UNKNOWN);
        assert_eq!(lib_text(Some(CellFormat::New14), 7), "7");
        assert_eq!(lib_text(Some(CellFormat::Ext36), 0), "0");
        assert_eq!(lib_text(None, 3), tx::VAL_UNKNOWN);
    }

    /// 工具下标 ↔ 工具名的映射必须与 `icons::TOOLS` 的下标一致（防错位）。
    #[test]
    fn 工具下标与图标表一致() {
        assert_eq!(crate::icons::TOOLS[TOOL_BRUSH_IDX].tool, "Brush");
        assert_eq!(crate::icons::TOOLS[TOOL_ERASER_IDX].tool, "Eraser");
        assert_eq!(crate::icons::TOOLS[TOOL_PICKUP_IDX].tool, "Pickup");
        assert_eq!(Tool::from_index(TOOL_BRUSH_IDX), Tool::Brush);
        assert_eq!(Tool::from_index(TOOL_ERASER_IDX), Tool::Eraser);
        assert_eq!(Tool::from_index(TOOL_PICKUP_IDX), Tool::Pickup);
        for i in 0..crate::icons::TOOLS.len() {
            let t = Tool::from_index(i);
            if t.is_wired() {
                assert_eq!(t.index(), i, "接线工具的 index() 必须回到自己");
            }
        }
        assert_eq!(Tool::Brush.name(), tx::TOOL_BRUSH);
        assert_eq!(Tool::Eraser.name(), tx::TOOL_ERASER);
        assert_eq!(Tool::Pickup.name(), tx::TOOL_PICKUP);
        assert_eq!(Tool::Unwired.name(), tx::TOOL_UNWIRED);
    }

    /// 脚本驱动：吸管 → 画笔 ×3 → 撤销 ×3 → 重做 ×3，末态与"改 3 格"一致。
    #[test]
    fn 脚本驱动跑通闭环() {
        let dir = std::env::temp_dir().join("miredit_gui_edit_script");
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("script-2x2.map");
        let mut bytes = vec![0u8; HEADER_LEN + 12 * 4];
        bytes[0..2].copy_from_slice(&2u16.to_le_bytes());
        bytes[2..4].copy_from_slice(&2u16.to_le_bytes());
        for i in 0..4 {
            bytes[HEADER_LEN + i * 12] = (i as u8) + 1;
        }
        std::fs::write(&p, &bytes).unwrap();

        let mut doc = crate::mapdoc::DocState::load(Some(&p));
        let mut ed = EditSession::default();
        let rep = apply_script(
            &mut doc,
            &mut ed,
            &[
                EditCmd::Pickup(0, 0),
                EditCmd::Brush(1, 0),
                EditCmd::Brush(1, 1),
                EditCmd::Undo(2),
                EditCmd::Check,
                EditCmd::Redo(2),
                EditCmd::Check,
            ],
        );
        assert_eq!(rep.edited_cells, 3);
        assert_eq!(rep.steps, 1);
        assert!(rep.dirty);
        assert_eq!(rep.undo, 1);
        assert_eq!(ed.brush().unwrap().sprite(), 1);
        // 末态 = 三格被刷成 0 号格的内容（图号 1）
        //
        // ⚠ **为什么是 3 格而不是 2 格**（本版语义变化，已如实记录）：
        // 笔刷现在按当前尺寸落（缺省 3 ⇒ 3×3，见 [`EditSession::touch_footprint`]）。
        // 这张 2×2 的图上，(1,0) 的 3×3 覆盖全部 4 格，其中 (0,0) 本来就是图号 1 ⇒ 真改 3 格；
        // 紧接着的 (1,1) 已经全是 1 ⇒ 0 格 ⇒ 不产生第二步。
        assert_eq!(doc.doc.as_ref().unwrap().map.cell(1, 0).unwrap().bk_img, 1);
        assert_eq!(doc.doc.as_ref().unwrap().map.cell(1, 1).unwrap().bk_img, 1);
        let _ = std::fs::remove_file(&p);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 脚本在"没有地图"时必须停手（不假装改过）。
    #[test]
    fn 无地图脚本不做任何事() {
        let mut doc = crate::mapdoc::DocState::load(Some(std::path::Path::new(
            r"E:\__no_such_map__.map",
        )));
        let mut ed = EditSession::default();
        let rep = apply_script(&mut doc, &mut ed, &[EditCmd::Brush(0, 0), EditCmd::Check]);
        assert_eq!(rep.edited_cells, 0);
        assert_eq!(rep.steps, 0);
        assert!(!rep.dirty);
    }

    // ───────────────── 审计 E1：素材帧索引(0 基) → 图号(1 基) ─────────────────
    //
    // 这三条是「必红回归」：把 `set_brush_from_palette` 改回 `bk_img: img as u16`
    // （旧逻辑）时它们全部失败 —— 见交付汇报里的两次 `test result:` 行。

    /// 记帧请求的假源：`compose` 真实要了哪些 `(lib, index)` 全记下来。
    ///
    /// 用它而不是真图库：验收要断言的是"取帧请求的 index 是 N"，
    /// 与图库里第 N 帧长什么样无关（本机没有图库时也必须能跑）。
    struct 记帧源(std::cell::RefCell<Vec<(mir2_assets::LibRef, u32)>>);

    impl 记帧源 {
        fn 新() -> Self {
            记帧源(std::cell::RefCell::new(Vec::new()))
        }
        fn 请求(&self) -> Vec<(mir2_assets::LibRef, u32)> {
            self.0.borrow().clone()
        }
    }

    impl mir2_assets::SpriteSource for 记帧源 {
        fn frame(
            &self,
            lib: mir2_assets::LibRef,
            index: u32,
        ) -> Option<mir2_assets::Frame> {
            self.0.borrow_mut().push((lib, index));
            // 96×64 = 一格地表帧的实测尺寸（`metrics::CELL_W*2 × CELL_H*2`）
            Some(mir2_assets::Frame {
                width: 96,
                height: 64,
                rgba: vec![255u8; 96 * 64 * 4],
                // 本测试只验"取帧请求的帧号"，与帧锚点无关 ⇒ 其余字段（含 anchor）走 Default，
                // 将来 Frame 再加字段时这里不必再改（见 mir2-assets/src/frame.rs 的 Default 文档）。
                ..mir2_assets::Frame::default()
            })
        }
    }

    /// 造一张 2×2 的 New14 空图（有 `btTiles` 字节），并把 (0,0) 之外保持无地表。
    fn 空图2x2() -> Map {
        let mut bytes = vec![0u8; HEADER_LEN + 14 * 4];
        bytes[0..2].copy_from_slice(&2u16.to_le_bytes());
        bytes[2..4].copy_from_slice(&2u16.to_le_bytes());
        Map::parse(&bytes).expect("2×2 New14 合成图应能解析")
    }

    /// **选 palette 第 904 帧 → `bk_img == 905`**（旧逻辑写成 904 ⇒ 必红）。
    #[test]
    fn 素材帧索引落格写图号加一() {
        assert_eq!(palette_index_to_sprite(904), Some(905));
        let lib = mir2_assets::LibRef::ground(4);
        let mut ed = EditSession::default();
        let b = ed
            .set_brush_from_palette(lib, 904)
            .expect("地面库的合法帧应能设为笔刷");
        assert_eq!(b.img, 905, "0 基索引 904 落格必须写成图号 905");
        assert_eq!(b.sprite(), 905, "笔刷显示的图号也必须是 905");
        assert_eq!(b.lib_byte, 4, "地表库号字节照旧");
        assert_eq!(b.from, None);

        // 真的落到地图格上
        let mut map = 空图2x2();
        assert_eq!(ed.apply_once(&mut map, 0, 0), 1);
        assert_eq!(map.cell(0, 0).unwrap().bk_img, 905);
    }

    /// **第 0 帧 ⇒ `bk_img == 1`**（旧逻辑写成 0 = 「该格无地表」，静默丢地表 ⇒ 必红）。
    #[test]
    fn 零号帧落格写成图号一() {
        assert_eq!(palette_index_to_sprite(0), Some(1));
        let lib = mir2_assets::LibRef::ground(0);
        let mut ed = EditSession::default();
        let b = ed.set_brush_from_palette(lib, 0).expect("0 号帧合法");
        assert_eq!(b.img, 1, "0 号帧必须写成图号 1，绝不能写 0");

        let mut map = 空图2x2();
        ed.apply_once(&mut map, 0, 0);
        let cell = map.cell(0, 0).unwrap();
        assert_eq!(cell.bk_img, 1);
        assert_ne!(cell.bk_img & 0x7FFF, 0, "格子不许变成「无地表」");
    }

    /// 上限闸门按**图号**判：索引 0x7FFE ⇒ 0x7FFF 合法；0x7FFF ⇒ 0x8000 越界被拒。
    #[test]
    fn 图号上限按图号判而非按索引() {
        assert_eq!(palette_index_to_sprite(0x7FFE), Some(SPRITE_MAX));
        assert_eq!(palette_index_to_sprite(0x7FFF), None, "0x7FFF+1 = 0x8000 会撞阻挡位");
        assert_eq!(palette_index_to_sprite(u32::MAX), None, "溢出不许 panic/wrap");

        let lib = mir2_assets::LibRef::ground(0);
        let mut ed = EditSession::default();
        assert!(ed.set_brush_from_palette(lib, 0x7FFF).is_none());
        assert_eq!(ed.brush(), None, "被拒时不许动既有笔刷");
        assert!(ed.set_brush_from_palette(lib, 0x7FFE).is_some());
        assert_eq!(ed.brush().unwrap().img, SPRITE_MAX);
    }

    /// **端到端：落格后 compose 取到的就是第 904 帧**（帧源请求 index == 904）。
    ///
    /// 这是 E1 的"看得见后果"那一半：`bk_img` 对不上时，渲染器会去要 903（或干脆不要，
    /// 因为 0 = 无地表），两种错法在这条断言下都必红。
    #[test]
    fn 落格后渲染取到的正是对应帧() {
        let lib = mir2_assets::LibRef::ground(0);
        let mut ed = EditSession::default();
        ed.set_brush_from_palette(lib, 904).unwrap();
        let mut map = 空图2x2();
        ed.apply_once(&mut map, 0, 0);

        let src = 记帧源::新();
        let vp = miredit_render::Viewport::new(0, 0, 2, 2);
        let out = miredit_render::try_render_region(&map, &src, &vp, &miredit_render::LayerMask::all())
            .expect("2×2 视口应能渲染");
        assert_eq!(out.stats.frames_decoded, 1, "只该解 1 帧（只有 (0,0) 有地表）");
        assert_eq!(
            src.请求(),
            vec![(lib, 904)],
            "渲染器必须去要 0 基的 904 号帧（= 图号 905）"
        );

        // 0 号帧那一半：旧逻辑写 bk_img=0 ⇒ 渲染器一帧都不要（格子变"无地表"）
        let mut ed0 = EditSession::default();
        ed0.set_brush_from_palette(lib, 0).unwrap();
        let mut map0 = 空图2x2();
        ed0.apply_once(&mut map0, 0, 0);
        let src0 = 记帧源::新();
        let out0 = miredit_render::try_render_region(
            &map0,
            &src0,
            &vp,
            &miredit_render::LayerMask::all(),
        )
        .expect("应能渲染");
        assert_eq!(out0.stats.frames_decoded, 1, "0 号帧也必须真的画出图来");
        assert_eq!(src0.请求(), vec![(lib, 0)], "0 号帧要向帧源要 index 0");
    }

    /// **吸管反显不再 -1**：地图格 `bk_img=905`（1 基）→ 笔刷图号就是 905。
    ///
    /// 审计 E1 第 4 条：`pick_at` 取的是地图里的**1 基**字节，显示成「图号」时
    /// **不能**再减 1（减了就与面板/状态栏的口径又差 1）。
    #[test]
    fn 吸管反显图号不再减一() {
        let mut map = 空图2x2();
        let idx = index_of(&map, 1, 1).unwrap();
        map.cells[idx].bk_img = 905;
        let mut ed = EditSession::default();
        let b = ed.pick_at(&map, 1, 1).expect("图内格");
        assert_eq!(b.img, 905, "吸管必须原样取 1 基图号");
        assert_eq!(b.sprite(), 905, "反显图号 = 905（不是 904）");
        // 状态栏那一行的真实文案
        assert_eq!(
            crate::ui_text::fill(
                crate::ui_text::STATUS_BRUSH_TMPL,
                &[
                    ("fam", b.family_name()),
                    ("img", &b.sprite().to_string()),
                    ("lib", &brush_lib_text(Some(CellFormat::New14), &b)),
                ]
            ),
            "笔刷：地表图层 图号 905 · 资源库 0"
        );
    }
    /// **真图库 + 真渲染的像素级证明**：落格后渲出来的那块 96×64 就是**第 904 帧**的像素。
    ///
    /// 与 [`落格后渲染取到的正是对应帧`] 互补：那条用假源断言"请求的 index 是 904"，
    /// 这条用本机真图库断言"渲出来的像素就是 `frame_of(ground(0), 904)`"——
    /// 即 palette 缩略图显示的那一帧。差一帧（904 vs 903）会被第二条断言抓住。
    ///
    /// 本机没有图库时**如实跳过**（打印原因，不假装通过）。
    #[test]
    fn 真图库下渲染出的是第九零四帧像素() {
        let Some(root) = mir2_assets::client_root() else {
            eprintln!("[跳过] 未设置 {}（图库扫描根）", mir2_assets::CLIENT_ROOT_ENV);
            return;
        };
        let Ok(paths) = mir2_assets::LibPaths::discover(&root) else {
            eprintln!("[跳过] 本机没有图库 {}", root.display());
            return;
        };
        let Ok(src) = mir2_assets::WilSource::open(&paths) else {
            eprintln!("[跳过] 本机图库打不开");
            return;
        };
        let lib = mir2_assets::LibRef::ground(0);
        let Some(f904) = src.frame_of(lib, 904) else {
            eprintln!("[跳过] 本机 ground(0) 没有第 904 帧");
            return;
        };
        if f904.width == 0 || f904.height == 0 {
            eprintln!("[跳过] 第 904 帧是空帧");
            return;
        }

        // 落格：选 palette 第 904 帧 → 画到 (0,0)（偶偶格，地表才铺）
        let mut ed = EditSession::default();
        ed.set_brush_from_palette(lib, 904).expect("应能设成笔刷");
        let mut map = 空图2x2();
        assert_eq!(ed.apply_once(&mut map, 0, 0), 1);
        assert_eq!(map.cell(0, 0).unwrap().bk_img, 905);

        let vp = miredit_render::Viewport::new(0, 0, 2, 2);
        let out = miredit_render::try_render_region(&map, &src, &vp, &miredit_render::LayerMask::all())
            .expect("2×2 视口应能渲染");
        assert_eq!(out.width, 96, "2 格宽 = 96 px");
        assert_eq!(out.height, 64, "2 格高 = 64 px");
        assert_eq!(out.stats.frames_decoded, 1, "只该解出 1 帧");

        // 「over 到全透明底」逐像素期望值：alpha==0 的像素画布保持 (0,0,0,0)
        let expect = |f: &mir2_assets::Frame| -> Vec<u8> {
            let mut v = vec![0u8; 96 * 64 * 4];
            for i in 0..(96 * 64) {
                if f.rgba[i * 4 + 3] == 0 {
                    continue;
                }
                v[i * 4..i * 4 + 4].copy_from_slice(&f.rgba[i * 4..i * 4 + 4]);
            }
            v
        };
        assert_eq!(
            out.rgba,
            expect(&f904),
            "渲出来的 96×64 必须逐像素等于 palette 第 904 帧"
        );

        // 差一帧必须看得出来（相邻帧若恰好像素相同就如实跳过这一条）
        for other in [903u32, 905] {
            if let Some(fo) = src.frame_of(lib, other) {
                if fo.width == f904.width && fo.height == f904.height && fo.rgba != f904.rgba {
                    assert_ne!(
                        out.rgba,
                        expect(&fo),
                        "渲出来的像素不许是第 {other} 帧（差一帧）"
                    );
                }
            }
        }
    }

    // ───────────────── 三层编辑（按层分派 / 族不匹配 / 橡皮按层 / 12B） ─────────────────
    //
    // 这一组是「三层里只有地表能写」这条审计 P0 的正向判据：
    // ① 写哪个字段由**当前编辑层**决定；② 族不匹配必须拒绝且一字节不动；
    // ③ 橡皮只清当前层；④ 12B 档三族都只写图号（没有库号字节，不许凭空补）。

    /// 层 → 该层库号字节在格内的偏移（`Old12` 档没有这些字节；**阻挡层不写库号** ⇒ `None`）。
    const fn lib_off_of(layer: EditLayer) -> Option<usize> {
        match layer {
            EditLayer::Ground => Some(12), // btTiles
            EditLayer::Middle => Some(13), // btsmTiles
            EditLayer::Front => Some(10),  // btArea
            EditLayer::LowCollision | EditLayer::HighCollision => None,
        }
    }

    /// `to_bytes()` 与原文件的**差异偏移**（升序），用于断言"只动了这几个字节"。
    fn 差异偏移(map: &Map, orig: &[u8]) -> Vec<usize> {
        let now = map.to_bytes();
        assert_eq!(now.len(), orig.len(), "改格不许改文件长度");
        now.iter()
            .zip(orig.iter())
            .enumerate()
            .filter_map(|(i, (a, b))| if a == b { None } else { Some(i) })
            .collect()
    }

    /// **三族 × 三档：改一格只动本层那两个字节（14/36 档再加本层库号字节）。**
    ///
    /// 这是验收门 1 的程序化版本（真文件那一份由 `--edit-*` 无窗驱动跑出来）：
    /// - 地表 ⇒ `+0..+1`（14/36 档再 `+12 btTiles`）
    /// - 中层 ⇒ `+2..+3`（14/36 档再 `+13 btsmTiles`）
    /// - 物件 ⇒ `+4..+5`（14/36 档再 `+10 btArea`）
    ///
    /// 只要有人让画笔"顺手"写给别的层（跨字段写），差异集合里就会多出偏移 ⇒ 当场红。
    ///
    /// ⚠ 循环用 [`EditLayer::IMAGE`]（三族）而不是 `ALL`（五层）：阻挡层不写图号，
    /// 把它套进"图号"口径是错的口径（它的字节断言在 [`阻挡层置位只动bit15`] 里）。
    #[test]
    fn 三族落笔只动本层字节() {
        for fmt in [CellFormat::Old12, CellFormat::New14, CellFormat::Ext36] {
            for layer in EditLayer::IMAGE {
                let mut map = 造图(3, 3, fmt);
                let orig = map.to_bytes();
                // 库里给一个与格内初值都不同的库号字节（否则 +13/+10 那两个字节"没变化"，
                // 断言就退化成了"没写"，测不出跨层乱写）
                let lib = 图库(layer, 5 + layer.row() as u16);
                let mut ed = EditSession::default();
                ed.set_layer(layer);
                // 帧索引 300 ⇒ 图号 301（0x012D）：与原格单字节图号相比**两个字节都变**，
                // 这样"只动 +0..+1"才有两个偏移可查（若两个图号高字节相同，差异只会剩 1 个偏移，
                // 断言就退化成"高字节没写"，测不出跨层乱写）
                ed.set_brush_from_palette(lib, 300).expect("三族都应能设为笔刷");

                let (x, y) = (1u16, 1u16);
                let before = *map.cell(x, y).unwrap();
                assert_eq!(ed.apply_once(&mut map, x, y), 1, "{fmt:?}/{layer:?} 应真的改一格");
                let base = HEADER_LEN + index_of(&map, x, y).unwrap() * fmt.cell_size();
                let mut want = vec![base + layer.row() * 2, base + layer.row() * 2 + 1];
                if fmt.has_lib_bytes() {
                    want.push(base + lib_off_of(layer).expect("图号层必有库号字节偏移"));
                }
                want.sort_unstable();
                assert_eq!(
                    差异偏移(&map, &orig),
                    want,
                    "{fmt:?} 的{layer:?}改一格只许动本层字节（多出来就是跨层乱写）"
                );

                // 逐字段复核：只有本层变了
                let after = *map.cell(x, y).unwrap();
                for other in EditLayer::IMAGE {
                    if other == layer {
                        continue;
                    }
                    assert_eq!(
                        other.img_of(&after),
                        other.img_of(&before),
                        "{fmt:?}/{layer:?} 落笔不许动 {other:?} 的图号"
                    );
                }
            }
        }
    }

    /// **族不匹配拒绝**：当前层 = 物件层、笔刷来自地面库 ⇒ 一格都不改，并给出原因。
    #[test]
    fn 族不匹配拒绝且一字节不动() {
        let mut map = 造图(3, 3, CellFormat::New14);
        let orig = map.to_bytes();
        let mut ed = EditSession::default();
        ed.set_layer(EditLayer::Front);
        ed.set_brush_from_palette(mir2_assets::LibRef::ground(2), 5)
            .expect("地面库合法帧应能设为笔刷");
        // 设笔刷时就已经能判出不一致 ⇒ 提示立刻出现（不等用户点）
        assert_eq!(
            ed.note(),
            Some(
                tx::fill(
                    tx::EDIT_LAYER_MISMATCH_TMPL,
                    &[("brush", tx::LAYER_GROUND), ("layer", tx::LAYER_OBJECTS)]
                )
                .as_str()
            ),
            "族不匹配必须当场说清是哪两族"
        );

        assert_eq!(ed.apply_once(&mut map, 1, 1), 0, "族不匹配 ⇒ 不许改格");
        assert_eq!(map.to_bytes(), orig, "族不匹配时文件必须逐字节不变");
        assert_eq!(ed.undo_len(), 0, "拒绝的一笔不产生撤销步");
        assert!(!ed.dirty(), "拒绝的一笔不许把文档弄脏");

        // 换回同族笔刷 ⇒ 提示自动消失（提示必须跟状态一致）
        ed.set_brush_from_palette(mir2_assets::LibRef::surface(1), 5)
            .unwrap();
        assert_eq!(ed.note(), None);
        assert_eq!(ed.apply_once(&mut map, 1, 1), 1, "同族应当立刻能落笔");
    }

    /// **橡皮按层**：物件层擦 → 只有 `fr_img` 变（保留最高位），`bk_img`/`mid_img` 不变。
    #[test]
    fn 橡皮按层只清当前层() {
        let mut map = 造图(3, 3, CellFormat::New14);
        let idx = index_of(&map, 2, 0).unwrap();
        map.cells[idx].bk_img = 0x8000 | 111;
        map.cells[idx].mid_img = 0x8000 | 222;
        map.cells[idx].fr_img = 0x8000 | 333;
        let orig = map.to_bytes();

        let mut ed = EditSession::default();
        ed.select(TOOL_ERASER_IDX);
        ed.set_layer(EditLayer::Front);
        assert_eq!(ed.apply_once(&mut map, 2, 0), 1);
        let after = *map.cell(2, 0).unwrap();
        assert_eq!(after.fr_img, 0x8000, "物件层图号清零、最高位保留");
        assert_eq!(after.bk_img, 0x8000 | 111, "地表层不许动");
        assert_eq!(after.mid_img, 0x8000 | 222, "中层不许动");

        let base = HEADER_LEN + idx * 14;
        assert_eq!(
            差异偏移(&map, &orig),
            vec![base + 4, base + 5],
            "擦物件层只许动 +4..+5 两个字节"
        );

        // 三层各擦一次，最终三族的最高位都在、图号都清零
        let mut ed2 = EditSession::default();
        ed2.select(TOOL_ERASER_IDX);
        for layer in EditLayer::IMAGE {
            ed2.set_layer(layer);
            ed2.apply_once(&mut map, 2, 0);
        }
        let c = *map.cell(2, 0).unwrap();
        assert_eq!((c.bk_img, c.mid_img, c.fr_img), (0x8000, 0x8000, 0x8000));
    }

    /// **12B 档：三族都只写图号**（没有库号字节 ⇒ 长度不变，且不假装写了库号）。
    #[test]
    fn 十二字节档三族只写图号() {
        for layer in EditLayer::IMAGE {
            let mut map = 造图(2, 2, CellFormat::Old12);
            let orig = map.to_bytes();
            let mut ed = EditSession::default();
            ed.set_layer(layer);
            // 库号字节在 12B 档不存在：笔刷仍记着 7，但落格时不许写出去
            ed.set_brush_from_palette(图库(layer, 7), 300).unwrap();
            assert_eq!(ed.apply_once(&mut map, 1, 1), 1);
            assert_eq!(map.to_bytes().len(), orig.len(), "12B 档文件长度不许变");
            let base = HEADER_LEN + index_of(&map, 1, 1).unwrap() * 12;
            assert_eq!(
                差异偏移(&map, &orig),
                vec![base + layer.row() * 2, base + layer.row() * 2 + 1],
                "12B 档{layer:?}只许动图号两个字节（没有库号字节可写）"
            );
            let c = *map.cell(1, 1).unwrap();
            assert_eq!(layer.img_of(&c), 301, "图号 = 帧索引 300 + 1");
        }
    }

    /// **吸管按当前层取笔刷**：整格三族都读得到，但笔刷只拿当前层那一个字段。
    #[test]
    fn 吸管按当前层取笔刷() {
        let mut map = 造图(3, 3, CellFormat::New14);
        let idx = index_of(&map, 0, 2).unwrap();
        map.cells[idx].bk_img = 11;
        map.cells[idx].tiles = 21;
        map.cells[idx].mid_img = 12;
        map.cells[idx].sm_tiles = 22;
        map.cells[idx].fr_img = 13;
        map.cells[idx].area = 23;

        let mut ed = EditSession::default();
        // 整格三族都能读到（诊断用）
        let pc = ed.pick_cell(&map, 0, 2).expect("图内格");
        assert_eq!((pc.bk_img, pc.mid_img, pc.fr_img), (11, 12, 13));

        // 当前层 = 中层 ⇒ 笔刷族 = 中层、内容 = mid_img/btsmTiles
        ed.set_layer(EditLayer::Middle);
        let b = ed.pick_at(&map, 0, 2).expect("图内格");
        assert_eq!(b.family, Family::Middle);
        assert_eq!(b.img, 12);
        assert_eq!(b.lib_byte, 22);
        assert_eq!(b.lib, Some(LibRef::middle(22)));

        // 画到别格：只写 mid_img + btsmTiles
        let before = *map.cell(1, 1).unwrap();
        assert_eq!(ed.apply_once(&mut map, 1, 1), 1);
        let after = *map.cell(1, 1).unwrap();
        assert_eq!(after.mid_img, 12);
        assert_eq!(after.sm_tiles, 22);
        assert_eq!(after.bk_img, before.bk_img, "地表不许被吸管笔刷写到");
        assert_eq!(after.fr_img, before.fr_img, "物件层不许被吸管笔刷写到");

        // 当前层 = 物件层 ⇒ 笔刷换成 fr_img/btArea
        ed.set_layer(EditLayer::Front);
        let b = ed.pick_at(&map, 0, 2).unwrap();
        assert_eq!(b.family, Family::Surface);
        assert_eq!(b.img, 13);
        assert_eq!(b.lib_byte, 23);
    }

    /// 层名与族名必须同源（状态栏、素材面板、图层行用的是同一批中文）。
    ///
    /// 五行都查：图号层的层名 = 它那一族的中文名；阻挡层**没有族**（`family()` 为 `None`），
    /// 但层名照样来自同一张 [`tx::LAYER_NAMES`] 表。
    #[test]
    fn 层名与族名同源() {
        for layer in EditLayer::ALL {
            assert_eq!(layer.name(), tx::LAYER_NAMES[layer.row()]);
            assert_eq!(EditLayer::from_row(layer.row()), Some(layer));
            match layer.family() {
                Some(fam) => {
                    assert!(!layer.is_collision());
                    assert_eq!(family_name(fam), layer.name(), "{layer:?} 的族名应与层名同源");
                }
                None => {
                    assert!(layer.is_collision(), "{layer:?} 没有族 ⇒ 必须是阻挡层");
                }
            }
            // 图号字段：图号层 = 自己那一列；阻挡层 = 它那个位所在的那一列
            match layer {
                EditLayer::Ground => assert_eq!(layer.img_field(), tx::LAYER_IMG_FIELDS[0]),
                EditLayer::Middle => assert_eq!(layer.img_field(), tx::LAYER_IMG_FIELDS[1]),
                EditLayer::Front => assert_eq!(layer.img_field(), tx::LAYER_IMG_FIELDS[2]),
                EditLayer::LowCollision => {
                    assert_eq!(layer.img_field(), tx::LAYER_IMG_FIELDS[0], "低位阻挡存在 bk_img")
                }
                EditLayer::HighCollision => {
                    assert_eq!(layer.img_field(), tx::LAYER_IMG_FIELDS[2], "高位阻挡存在 fr_img")
                }
            }
            // 库号字段：阻挡层**不写库号** ⇒ None
            if layer.is_collision() {
                assert_eq!(layer.lib_field(), None);
                assert!(!layer.writes_lib_byte());
            } else {
                assert_eq!(
                    layer.lib_field(),
                    Some(tx::LAYER_LIB_FIELDS[layer.row()]),
                    "{layer:?} 的库号字段名应与 LAYER_LIB_FIELDS 同源"
                );
                assert!(layer.writes_lib_byte());
            }
        }
        assert_eq!(EditLayer::from_row(5), None, "越界行不是编辑层");
        assert_eq!(EditLayer::default(), EditLayer::Ground, "缺省当前层=地表层");
    }

    /// **撤销/重做：三族各一条，撤销后与原文件逐字节相等**（三档各跑一遍）。
    #[test]
    fn 三族撤销重做逐字节还原() {
        for fmt in [CellFormat::Old12, CellFormat::New14, CellFormat::Ext36] {
            for layer in EditLayer::IMAGE {
                let mut map = 造图(4, 4, fmt);
                let orig = map.to_bytes();
                let mut ed = EditSession::default();
                ed.set_layer(layer);
                ed.set_brush_from_palette(图库(layer, 6), 7).unwrap();

                assert_eq!(ed.apply_once(&mut map, 2, 2), 1);
                let edited = map.to_bytes();
                assert_ne!(edited, orig, "{fmt:?}/{layer:?} 改完必须与原文件不同");

                assert_eq!(ed.undo(&mut map), 1);
                assert_eq!(
                    map.to_bytes(),
                    orig,
                    "{fmt:?}/{layer:?} 撤销后必须与磁盘原文件逐字节相等"
                );
                assert!(!ed.dirty(), "撤销回原样后不该脏");

                assert_eq!(ed.redo(&mut map), 1);
                assert_eq!(map.to_bytes(), edited, "{fmt:?}/{layer:?} 重做后应回到改动态");
            }
        }
    }

    // ───────────────── 阻挡层（bit15）：审计 P0-9「画完不知道能不能走人」 ─────────────────
    //
    // 这一组是**碰撞层可编辑**的正向判据（三档 × 两位）：
    // ① 画笔 = 置位（`| 0x8000`）、橡皮 = 清位（`& 0x7FFF`）；
    // ② **只动 bit15**：低 15 位的图号一个 bit 都不许变，库号字节也不许变；
    // ③ 与 `mir2_map::Map::walkable` 一致（置位后 true→false、清位后回来）；
    // ④ 图库笔刷必须被拒绝（阻挡层不写图号）。

    /// 阻挡位所在字节在格内的偏移：低位阻挡 = `bk_img` 高字节 `+1`，高位阻挡 = `fr_img` 高字节 `+5`。
    const fn bit15_off_of(layer: EditLayer) -> usize {
        match layer {
            EditLayer::LowCollision => 1,
            EditLayer::HighCollision => 5,
            _ => panic!("只有阻挡层有 bit15 偏移"),
        }
    }

    /// **置位：只动 bit15，图号低 15 位原样**（三档 × 两位）。
    ///
    /// 断言三层：
    /// 1. 差异偏移**只有一个字节**（`+1` 低位 / `+5` 高位），且该字节正好 `| 0x80`；
    /// 2. 该字段低 15 位不变（图号没被顺手改）；
    /// 3. 另外两个图号字段、库号字节、元数据字节全部原样。
    #[test]
    fn 阻挡层置位只动bit15() {
        for fmt in [CellFormat::Old12, CellFormat::New14, CellFormat::Ext36] {
            for layer in [EditLayer::LowCollision, EditLayer::HighCollision] {
                let mut map = 造图(3, 3, fmt);
                let orig = map.to_bytes();
                let (x, y) = (1u16, 2u16);
                let before = *map.cell(x, y).unwrap();
                let mut ed = EditSession::default();
                ed.select(TOOL_BRUSH_IDX);
                ed.set_layer(layer);
                // 手上**没有笔刷**也能置位：阻挡层的落笔载荷是"位"，不是"图号"
                assert_eq!(ed.brush(), None);
                assert_eq!(ed.apply_once(&mut map, x, y), 1, "{fmt:?}/{layer:?} 应真的改一格");

                let base = HEADER_LEN + index_of(&map, x, y).unwrap() * fmt.cell_size();
                assert_eq!(
                    差异偏移(&map, &orig),
                    vec![base + bit15_off_of(layer)],
                    "{fmt:?}/{layer:?} 置位只许动 bit15 那一个字节"
                );

                let after = *map.cell(x, y).unwrap();
                assert_eq!(
                    after.bk_img & 0x7FFF,
                    before.bk_img & 0x7FFF,
                    "{fmt:?}/{layer:?} 地表图号低 15 位不许变"
                );
                assert_eq!(
                    after.fr_img & 0x7FFF,
                    before.fr_img & 0x7FFF,
                    "{fmt:?}/{layer:?} 物件图号低 15 位不许变"
                );
                assert_eq!(after.mid_img, before.mid_img, "中层不许动");
                assert_eq!(after.tiles, before.tiles, "btTiles 不许动（阻挡层不写库号）");
                assert_eq!(after.sm_tiles, before.sm_tiles);
                assert_eq!(after.area, before.area, "btArea 不许动");
                assert_eq!(after.light, before.light);
                assert_eq!(after.door_index, before.door_index);
                assert_eq!(after.tail, before.tail);

                // 目标位确实置上了，另一层不受影响
                let (want_bk, want_fr) = match layer {
                    EditLayer::LowCollision => (before.bk_img | 0x8000, before.fr_img),
                    _ => (before.bk_img, before.fr_img | 0x8000),
                };
                assert_eq!((after.bk_img, after.fr_img), (want_bk, want_fr));
            }
        }
    }

    /// **清位：只动 bit15，图号低 15 位原样**（三档 × 两位；先置位再擦）。
    #[test]
    fn 阻挡层清位只动bit15() {
        for fmt in [CellFormat::Old12, CellFormat::New14, CellFormat::Ext36] {
            for layer in [EditLayer::LowCollision, EditLayer::HighCollision] {
                let mut map = 造图(3, 3, fmt);
                // 先把两位都置上（清位才有东西可清）
                let idx = index_of(&map, 2, 1).unwrap();
                map.cells[idx].bk_img |= 0x8000;
                map.cells[idx].fr_img |= 0x8000;
                let orig = map.to_bytes();
                let before = *map.cell(2, 1).unwrap();
                assert_eq!(before.bk_img & 0x8000, 0x8000);
                assert_eq!(before.fr_img & 0x8000, 0x8000);

                let mut ed = EditSession::default();
                ed.select(TOOL_ERASER_IDX);
                ed.set_layer(layer);
                assert_eq!(ed.apply_once(&mut map, 2, 1), 1, "{fmt:?}/{layer:?} 应真的改一格");

                let base = HEADER_LEN + idx * fmt.cell_size();
                assert_eq!(
                    差异偏移(&map, &orig),
                    vec![base + bit15_off_of(layer)],
                    "{fmt:?}/{layer:?} 清位只许动 bit15 那一个字节"
                );

                let after = *map.cell(2, 1).unwrap();
                // 目标层的位清了、**图号还在**；另一层原样
                let (want_bk, want_fr) = match layer {
                    EditLayer::LowCollision => (before.bk_img & 0x7FFF, before.fr_img),
                    _ => (before.bk_img, before.fr_img & 0x7FFF),
                };
                assert_eq!((after.bk_img, after.fr_img), (want_bk, want_fr));
                assert_eq!(after.bk_img & 0x7FFF, before.bk_img & 0x7FFF);
                assert_eq!(after.fr_img & 0x7FFF, before.fr_img & 0x7FFF);
            }
        }
    }

    /// **与 `mir2_map::Map::walkable` 一致**：置位 true→false、清位 false→true。
    ///
    /// 这是验收门 §四.2 的程序化版本：阻挡位不是"看着像"，客户端 `CanWalk`
    /// （`TMap.cpp:339`）等价于 `Cell::is_walkable`，而 `Map::walkable` 就是它的入口。
    #[test]
    fn 阻挡位与walkable一致() {
        for layer in [EditLayer::LowCollision, EditLayer::HighCollision] {
            let mut map = 造图(3, 3, CellFormat::New14);
            assert!(map.walkable(0, 0), "初始格可走");
            let mut ed = EditSession::default();
            ed.set_layer(layer);

            // 置位 ⇒ 不可走
            ed.select(TOOL_BRUSH_IDX);
            assert_eq!(ed.apply_once(&mut map, 0, 0), 1);
            assert!(!map.walkable(0, 0), "{layer:?} 置位后必须不可走");

            // 清位 ⇒ 又能走
            ed.select(TOOL_ERASER_IDX);
            assert_eq!(ed.apply_once(&mut map, 0, 0), 1);
            assert!(map.walkable(0, 0), "{layer:?} 清位后必须恢复可走");

            // 位与图号解耦：置位后 `Cell::is_walkable` 与 `walkable` 同口径
            ed.select(TOOL_BRUSH_IDX);
            ed.apply_once(&mut map, 0, 0);
            assert!(!map.cell(0, 0).unwrap().is_walkable());
        }
    }

    /// **阻挡层拒绝图库笔刷**（验收门 §四.5）：一格都不改，并把原因写进 `note`。
    ///
    /// 为什么要拒绝：图库笔刷的意图是"盖一张图"，而阻挡层只动 `0x8000` 一个位 ——
    /// 静默把位设上会让人以为图贴上去了（其实图号一个字节都没变）。
    #[test]
    fn 阻挡层拒绝图库笔刷() {
        for layer in [EditLayer::LowCollision, EditLayer::HighCollision] {
            let mut map = 造图(3, 3, CellFormat::New14);
            let orig = map.to_bytes();
            let mut ed = EditSession::default();
            ed.set_layer(layer);
            // 三族各试一次：只要笔刷来自图库，一律拒绝
            for fam in [
                mir2_assets::LibRef::ground(2),
                mir2_assets::LibRef::middle(3),
                mir2_assets::LibRef::surface(4),
            ] {
                ed.set_brush_from_palette(fam, 5).expect("合法帧应能设为笔刷");
                // 设笔刷那一刻提示就该出现（不等用户点下去）
                let want = tx::fill(
                    tx::EDIT_COLLISION_BRUSH_TMPL,
                    &[("brush", crate::palette::family_label(fam.family)), ("layer", layer.name())],
                );
                assert_eq!(ed.note(), Some(want.as_str()), "{layer:?} 必须当场说清原因");

                assert_eq!(ed.apply_once(&mut map, 1, 1), 0, "图库笔刷不许在阻挡层落笔");
                assert_eq!(map.to_bytes(), orig, "拒绝时文件必须逐字节不变");
                assert_eq!(ed.undo_len(), 0, "拒绝的一笔不产生撤销步");
                assert!(!ed.dirty(), "拒绝的一笔不许把文档弄脏");
            }
        }
    }

    /// 阻挡层**接受**"从地图上吸来的笔刷"与"手上没有笔刷"两种情况。
    ///
    /// 判据是 [`Brush::from`]：`Some` = 吸管从地图取的（没有"盖图号"的意图）；
    /// `None` = 从图库选的（上面那条测试已证明会被拒）。
    #[test]
    fn 阻挡层接受地图吸来的笔刷() {
        let mut map = 造图(3, 3, CellFormat::Old12);
        let mut ed = EditSession::default();
        ed.set_layer(EditLayer::LowCollision);
        // 先在别的格上吸一格（吸管按当前层取内容 ⇒ 取的是 bk_img）
        ed.pick_at(&map, 2, 2);
        assert!(ed.brush().is_some());
        assert_eq!(ed.note(), None, "地图吸来的笔刷不该被拒");
        assert_eq!(ed.apply_once(&mut map, 0, 0), 1, "吸来的笔刷应能在阻挡层置位");
        assert_eq!(map.cell(0, 0).unwrap().bk_img & 0x8000, 0x8000);
    }

    /// **吸管在阻挡层取到同一格的内容**：低位阻挡 ⇒ `bk_img`/`btTiles`（地面族），
    /// 高位阻挡 ⇒ `fr_img`/`btArea`（表层族）。取的是"格子内容"，不是"这一层能写什么"。
    #[test]
    fn 吸管在阻挡层取同一格内容() {
        let mut map = 造图(3, 3, CellFormat::New14);
        let idx = index_of(&map, 1, 1).unwrap();
        map.cells[idx].bk_img = 0x8000 | 111;
        map.cells[idx].tiles = 21;
        map.cells[idx].fr_img = 0x8000 | 333;
        map.cells[idx].area = 23;

        let mut ground = EditSession::default();
        ground.set_layer(EditLayer::Ground);
        let g = ground.pick_at(&map, 1, 1).expect("图内格");

        let mut low = EditSession::default();
        low.set_layer(EditLayer::LowCollision);
        let l = low.pick_at(&map, 1, 1).expect("图内格");
        assert_eq!(l, g, "低位阻挡吸到的笔刷必须与地表层一致");

        let mut high = EditSession::default();
        high.set_layer(EditLayer::HighCollision);
        let h = high.pick_at(&map, 1, 1).expect("图内格");
        assert_eq!(h.family, Family::Surface);
        assert_eq!(h.img, 0x8000 | 333);
        assert_eq!(h.lib_byte, 23);
    }

    /// **阻挡层撤销/重做逐字节还原**（三档 × 两位）。
    #[test]
    fn 阻挡层撤销重做逐字节还原() {
        for fmt in [CellFormat::Old12, CellFormat::New14, CellFormat::Ext36] {
            for layer in [EditLayer::LowCollision, EditLayer::HighCollision] {
                let mut map = 造图(4, 4, fmt);
                let orig = map.to_bytes();
                let mut ed = EditSession::default();
                ed.set_layer(layer);

                assert_eq!(ed.apply_once(&mut map, 2, 2), 1);
                let edited = map.to_bytes();
                assert_ne!(edited, orig, "{fmt:?}/{layer:?} 置位后必须与原文件不同");

                assert_eq!(ed.undo(&mut map), 1);
                assert_eq!(map.to_bytes(), orig, "{fmt:?}/{layer:?} 撤销后必须逐字节回原样");
                assert!(!ed.dirty());

                assert_eq!(ed.redo(&mut map), 1);
                assert_eq!(map.to_bytes(), edited, "{fmt:?}/{layer:?} 重做后应回到置位态");
            }
        }
    }


    #[test]
    fn 脚本可切当前编辑层() {
        let dir = std::env::temp_dir().join("miredit_gui_edit_layer_cmd");
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("layer-cmd.map");
        let mut bytes = vec![0u8; HEADER_LEN + 14 * 4];
        bytes[0..2].copy_from_slice(&2u16.to_le_bytes());
        bytes[2..4].copy_from_slice(&2u16.to_le_bytes());
        std::fs::write(&p, &bytes).unwrap();

        let mut doc = crate::mapdoc::DocState::load(Some(&p));
        let mut ed = EditSession::default();
        assert_eq!(ed.layer(), EditLayer::Ground);
        let _ = apply_script(
            &mut doc,
            &mut ed,
            &[EditCmd::Layer(EditLayer::Front), EditCmd::Layer(EditLayer::Middle)],
        );
        assert_eq!(ed.layer(), EditLayer::Middle);
        let _ = std::fs::remove_file(&p);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ───────────────── 本版新增：笔刷尺寸（3..=31 奇数，N×N 居中落笔） ─────────────────

    /// 造一张 `w×h` 的图，并把 `bk_img` 全设成 `img`（用于"同图号连通块"的填充测试）。
    ///
    /// ⚠ 直接拼字节而**不复用 [`造图`]**：那个帮手把"格序+1"写进图号，500×500 时
    /// 会撞 `u16` 上限（本文件有一条 500×500 的上限保护测试）。这里只写 `bk_img` 一个字段。
    fn 同号图(w: u16, h: u16, img: u16) -> Map {
        let n = usize::from(w) * usize::from(h);
        let mut bytes = vec![0u8; HEADER_LEN + 12 * n];
        bytes[0..2].copy_from_slice(&w.to_le_bytes());
        bytes[2..4].copy_from_slice(&h.to_le_bytes());
        for i in 0..n {
            let base = HEADER_LEN + i * 12;
            bytes[base..base + 2].copy_from_slice(&img.to_le_bytes());
        }
        Map::parse(&bytes).expect("合成图应能解析")
    }

    /// 尺寸夹进 3..=31 的奇数（`--brush-size` / `[` `]` / 官方滑块同一判据）。
    #[test]
    fn 尺寸夹到三到三十一的奇数() {
        let mut ed = EditSession::default();
        assert_eq!(ed.brush_size(), 3, "缺省 3（官方 Value=\"3\"）");
        for (ask, want) in [
            (0usize, 3usize),
            (1, 3),
            (2, 3),
            (3, 3),
            (4, 5),
            (7, 7),
            (15, 15),
            (31, 31),
            (32, 31),
            (99, 31),
        ] {
            assert_eq!(ed.set_brush_size(ask), want, "set_brush_size({ask})");
            assert_eq!(ed.brush_size(), want);
        }
        // `[` / `]` 一次走 2
        ed.set_brush_size(7);
        assert_eq!(ed.grow_brush_size(-2), 5);
        assert_eq!(ed.grow_brush_size(2), 7);
        // 撞边界只夹不溢出
        ed.set_brush_size(3);
        assert_eq!(ed.grow_brush_size(-2), 3, "已在下限，`[` 不再减");
        ed.set_brush_size(31);
        assert_eq!(ed.grow_brush_size(2), 31, "已在上限，`]` 不再加");
        // 改尺寸**不产生步骤、不改地图、不动脏标记**
        assert_eq!(ed.undo_len(), 0);
        assert!(!ed.dirty());
    }

    /// **尺寸 3 / 7 / 15 单点落笔 = N×N 格**（地图够大，不触发裁剪）。
    #[test]
    fn 尺寸三档单点覆盖格数() {
        for (size, want) in [(3usize, 9usize), (7, 49), (15, 225)] {
            let mut map = 造图(40, 40, CellFormat::New14);
            let mut ed = EditSession::default();
            ed.select(TOOL_BRUSH_IDX);
            ed.pick_at(&map, 0, 0);
            ed.set_brush_size(size);
            // 中心 (20,20) 的 N×N 完全在图内
            assert_eq!(
                ed.paint_stroke_once(&mut map, 20, 20),
                want,
                "尺寸 {size} 单点应改 {want} 格"
            );
            assert_eq!(ed.undo_len(), 1, "一次落笔 = 一步");
            // 撤销后逐字节回原样
            let orig = 造图(40, 40, CellFormat::New14).to_bytes();
            assert_eq!(ed.undo(&mut map), want);
            assert_eq!(map.to_bytes(), orig, "尺寸 {size} 撤销后必须逐字节还原");
        }
    }

    /// **边缘裁剪**：中心落在角上时，只落图内那几格（官方 `if (!Document.Contains) continue`）。
    #[test]
    fn 尺寸在边缘正确裁剪() {
        // 笔刷从远处 (20,20) 取，保证被画的角格**值会变**（否则"没变"会少算一格）
        let 造 = |w: u16, h: u16| {
            let map = 造图(w, h, CellFormat::New14);
            let mut ed = EditSession::default();
            ed.pick_at(&map, 20, 20);
            (map, ed)
        };
        // 尺寸 7（半径 3）在 (0,0) ⇒ 只剩 4×4 = 16 格
        let (mut map, mut ed) = 造(40, 40);
        ed.set_brush_size(7);
        assert_eq!(ed.paint_stroke_once(&mut map, 0, 0), 16, "左上角应只剩 4×4");
        // 尺寸 15（半径 7）在 (0,0) ⇒ 8×8 = 64 格
        let (mut map2, mut ed2) = 造(40, 40);
        ed2.set_brush_size(15);
        assert_eq!(ed2.paint_stroke_once(&mut map2, 0, 0), 64, "左上角应只剩 8×8");
        // 右下角同理
        let (mut map3, mut ed3) = 造(40, 40);
        ed3.set_brush_size(7);
        assert_eq!(ed3.paint_stroke_once(&mut map3, 39, 39), 16, "右下角应只剩 4×4");
        // 尺寸 31（半径 15）在 (0,0) ⇒ 16×16 = 256 格
        let (mut map4, mut ed4) = 造(40, 40);
        ed4.set_brush_size(31);
        assert_eq!(ed4.paint_stroke_once(&mut map4, 0, 0), 256, "左上角应只剩 16×16");
    }

    /// **拖动画笔时整条轨迹都按 N×N 落**：一条 3 格的横线 + 尺寸 3 ⇒ 3×5 = 15 格（并集）。
    #[test]
    fn 拖动整条轨迹按尺寸落() {
        let mut map = 造图(40, 40, CellFormat::New14);
        let mut ed = EditSession::default();
        ed.pick_at(&map, 0, 0);
        ed.set_brush_size(3);
        // (10,10) → (12,10)：三个中心，每个 3×3，横向并集 = 5 列 × 3 行
        let n = ed.paint_stroke(&mut map, Some((10, 10)), (12, 10));
        assert_eq!(n, 15, "3 格轨迹 × 3×3 笔刷 ⇒ 5×3 = 15 格");
        assert_eq!(ed.undo_len(), 1, "整条轨迹 = 一步");
    }

    // ───────────────── 本版新增：填充（Bucket，判据照 MapViewport.cs:485-539） ─────────────────

    /// **填充只吃"同层同图号"的连通块**：块外一格不动、块内格数对得上。
    ///
    /// ⚠ 地表按 C# 的 `step = 2` 走（只填**偶偶格**，见 [`EditSession::fill`] ①b）：
    /// 这张 10×10 图里 4×4 的块（图号 7）只有 `(0,0)/(0,2)/(2,0)/(2,2)` 四个偶偶格会被填，
    /// 奇偶填充位**一格都不许动**。
    #[test]
    fn 填充只填同号连通块() {
        // 10×10，左上 4×4 = 图号 7，其余 = 图号 9
        let mut map = 造图(10, 10, CellFormat::New14);
        for c in map.cells.iter_mut() {
            c.bk_img = 9;
            c.tiles = 3;
        }
        let mut block = 0usize;
        for y in 0..4u16 {
            for x in 0..4u16 {
                let i = index_of(&map, x, y).unwrap();
                map.cells[i].bk_img = 7;
                block += 1;
            }
        }
        assert_eq!(block, 16);

        // 造一支"图号 1、库号 3"的地表笔刷：临时改一格吸走，再改回去
        let i99 = index_of(&map, 9, 9).unwrap();
        map.cells[i99].bk_img = 1;
        let mut ed = EditSession::default();
        ed.set_layer(EditLayer::Ground);
        ed.pick_at(&map, 9, 9);
        map.cells[i99].bk_img = 9;
        let before = map.to_bytes();
        let snapshot = Map::parse(&before).unwrap();

        let out = ed.fill(&mut map, 0, 0);
        assert_eq!(out, FillOutcome::Filled(4), "4×4 块里只有 4 个偶偶格");
        assert_eq!(ed.undo_len(), 1, "整次填充 = 一步");
        assert_eq!(
            ed.last_fill(),
            Some(FillRecord {
                cells: 4,
                layer: EditLayer::Ground
            })
        );

        // 块内的偶偶格全变成图号 1；奇偶填充位原样（这是 step=2 的判据）
        for y in 0..10u16 {
            for x in 0..10u16 {
                let i = index_of(&map, x, y).unwrap();
                if x < 4 && y < 4 {
                    if x % 2 == 0 && y % 2 == 0 {
                        assert_eq!(map.cells[i].bk_img, 1, "({x},{y}) 偶偶格应被填成 1");
                    } else {
                        assert_eq!(map.cells[i].bk_img, 7, "({x},{y}) 奇偶填充位不许动");
                    }
                } else {
                    assert_eq!(map.cells[i], snapshot.cells[i], "({x},{y}) 块外不许动");
                }
            }
        }
        // 差异只落在块内的偶偶格：4 格 × (bk_img + btTiles) = 8 字节；头部 0
        let d = byte_diff(&map, &before);
        assert_eq!(d.cells, 4, "字节差异必须正好是块内 4 个偶偶格");
        assert_eq!(d.header, 0, "头部不许动");
    }

    /// 造一张 `w×h` 的图，把 `mid_img` 全设成 `img`（中层 step=1 的上限测试用）。
    ///
    /// 与 [`同号图`] 一样直接拼字节：`造图` 会把"格序+1"写进图号，500×500 时撞 `u16`。
    fn 同号中图(w: u16, h: u16, img: u16) -> Map {
        let n = usize::from(w) * usize::from(h);
        let mut bytes = vec![0u8; HEADER_LEN + 14 * n];
        bytes[0..2].copy_from_slice(&w.to_le_bytes());
        bytes[2..4].copy_from_slice(&h.to_le_bytes());
        for i in 0..n {
            let base = HEADER_LEN + i * 14;
            bytes[base + 2..base + 4].copy_from_slice(&img.to_le_bytes()); // mid_img
        }
        Map::parse(&bytes).expect("合成图应能解析")
    }

    /// **整次填充 = 撤销一步**：`undo(1)` 后逐字节回原样。
    #[test]
    fn 填充撤销一步逐字节还原() {
        let mut map = 同号图(8, 8, 5);
        let mut ed = EditSession::default();
        ed.set_layer(EditLayer::Ground);
        // 造一个图号 1 的笔刷
        let i = index_of(&map, 0, 0).unwrap();
        map.cells[i].bk_img = 1;
        ed.pick_at(&map, 0, 0);
        map.cells[i].bk_img = 5; // 改回，让"全图同号 5"成立
        let orig = map.to_bytes();

        let out = ed.fill(&mut map, 3, 3);
        // 8×8 全同号，地表按 step=2 ⇒ 只填偶偶格 = 4×4 = 16
        assert_eq!(out, FillOutcome::Filled(16), "8×8 的偶偶格共 16 个");
        assert_ne!(map.to_bytes(), orig, "填完必须与原文件不同");
        assert_eq!(ed.undo_len(), 1, "整次填充只占一步");

        assert_eq!(ed.undo(&mut map), 16);
        assert_eq!(map.to_bytes(), orig, "撤销一步后必须逐字节相等");
        assert!(!ed.dirty(), "撤销回保存点后不脏");
    }

    /// **上限保护**：连通块超过 200,000 格 ⇒ 报错且**一格不写**（文件不变、栈不变）。
    ///
    /// 用**中层**（step=1）做这件事：地表是 step=2，要触发同一个上限得造 1000×1000
    /// （250,000 个偶偶格）的图，白白多 12 MB；上限判定本身与层无关
    /// （在 [`EditSession::fill`] 的 BFS 循环里，任何层都一样）。
    #[test]
    fn 填充超上限一格不写() {
        // 500×500 = 250,000 格，中层全同号 ⇒ 必然超上限
        let mut map = 同号中图(500, 500, 777);
        let i = index_of(&map, 0, 0).unwrap();
        map.cells[i].mid_img = 1;
        let mut ed = EditSession::default();
        ed.set_layer(EditLayer::Middle);
        ed.pick_at(&map, 0, 0);
        map.cells[i].mid_img = 777;
        let orig = map.to_bytes();

        let out = ed.fill(&mut map, 250, 250);
        match out {
            FillOutcome::TooLarge { cells } => {
                assert_eq!(cells, crate::metrics::FILL_MAX_CELLS + 1, "在上限处截断");
            }
            other => panic!("500×500 全同号必须被上限拦住，实得 {other:?}"),
        }
        assert!(out.is_refusal());
        assert_eq!(map.to_bytes(), orig, "被拒时地图一个字节都不许动");
        assert_eq!(ed.undo_len(), 0, "被拒时不许进撤销栈");
        assert!(!ed.dirty());
        assert!(ed.last_fill().is_none());
        // 原因要能被状态栏/stderr 看到，且带上限数字
        let note = ed.note().expect("必须有拒绝原因");
        assert!(note.contains("200000"), "原因里要写上限：{note}");
    }

    /// **不越界**：填充不跨图边界（角落起填、BFS 不会跑出 `[0,w)×[0,h)`）。
    #[test]
    fn 填充不跨图边界() {
        // 3×3 全同号 ⇒ 从角上填，地表 step=2 只拿得到 4 个偶偶格
        let mut map = 同号图(3, 3, 5);
        let i = index_of(&map, 0, 0).unwrap();
        map.cells[i].bk_img = 1;
        let mut ed = EditSession::default();
        ed.set_layer(EditLayer::Ground);
        ed.pick_at(&map, 0, 0);
        map.cells[i].bk_img = 5;
        assert_eq!(ed.fill(&mut map, 0, 0), FillOutcome::Filled(4));
        assert_eq!(ed.fill(&mut map, 9, 9), FillOutcome::OutOfMap, "图外点不许填");
        assert_eq!(ed.undo_len(), 1, "越界那次没产生步骤");
        // 换成中层（step=1）就是 9 格 —— 两个步长的差别要看得见
        let mut map2 = 同号图(3, 3, 5);
        let j = index_of(&map2, 0, 0).unwrap();
        map2.cells[j].bk_img = 1;
        let mut ed2 = EditSession::default();
        ed2.set_layer(EditLayer::Ground);
        ed2.pick_at(&map2, 0, 0);
        map2.cells[j].bk_img = 5;
        for c in map2.cells.iter_mut() {
            c.mid_img = 777;
        }
        ed2.set_layer(EditLayer::Middle);
        map2.cells[j].mid_img = 1;
        ed2.pick_at(&map2, 0, 0);
        map2.cells[j].mid_img = 777;
        assert_eq!(ed2.fill(&mut map2, 0, 0), FillOutcome::Filled(9), "中层逐格 = 9");
    }

    /// **阻挡层填充只置位**：只改 `bit15`，低 15 位一个 bit 不动。
    #[test]
    fn 填充阻挡层只置位() {
        let mut map = 造图(6, 6, CellFormat::New14);
        for (i, c) in map.cells.iter_mut().enumerate() {
            c.bk_img = ((i as u16) * 37) & 0x7FFF; // 低 15 位各不相同
            c.tiles = 3;
        }
        let orig = map.to_bytes();
        let mut ed = EditSession::default();
        ed.set_layer(EditLayer::LowCollision);
        // 阻挡层不需要笔刷（官方 :494 `image = collision ? mask : …`）
        let out = ed.fill(&mut map, 2, 2);
        assert_eq!(out, FillOutcome::Filled(36), "整图 36 格都要置位");
        for (i, c) in map.cells.iter().enumerate() {
            let old = Map::parse(&orig).unwrap().cells[i];
            assert_eq!(c.bk_img & 0x8000, 0x8000, "第 {i} 格 bit15 必须置上");
            assert_eq!(c.bk_img & 0x7FFF, old.bk_img & 0x7FFF, "第 {i} 格低 15 位不许动");
            assert_eq!(c.fr_img, old.fr_img, "高位阻挡位不许动");
        }
        // 再点一次：已是目标值 ⇒ 无事可做（官方 :501 早退）
        let after_first = map.to_bytes();
        assert_eq!(ed.fill(&mut map, 2, 2), FillOutcome::AlreadyFilled);
        assert_eq!(map.to_bytes(), after_first);
        assert_eq!(ed.undo_len(), 1, "第二次没有产生步骤");
    }

    /// 物体层不支持填充（官方 `:488-492`）；一格都不写。
    #[test]
    fn 填充物体层被拒() {
        let mut map = 造图(4, 4, CellFormat::New14);
        let orig = map.to_bytes();
        let mut ed = EditSession::default();
        ed.set_layer(EditLayer::Front);
        ed.pick_at(&map, 0, 0);
        assert_eq!(ed.fill(&mut map, 1, 1), FillOutcome::UnsupportedLayer);
        assert_eq!(map.to_bytes(), orig);
        assert_eq!(ed.undo_len(), 0);
        assert!(ed.note().is_some());
    }

    /// 没有笔刷时图号层拒绝填充（阻挡层则不需要笔刷）。
    #[test]
    fn 填充没笔刷被拒() {
        let mut map = 造图(4, 4, CellFormat::New14);
        let orig = map.to_bytes();
        let mut ed = EditSession::default();
        ed.set_layer(EditLayer::Ground);
        assert_eq!(ed.brush(), None);
        assert_eq!(ed.fill(&mut map, 1, 1), FillOutcome::NoBrush);
        assert_eq!(map.to_bytes(), orig);
        // 换成阻挡层：同样没有笔刷，但**允许**填（这一层不用笔刷）
        ed.set_layer(EditLayer::LowCollision);
        assert!(matches!(ed.fill(&mut map, 1, 1), FillOutcome::Filled(_)));
    }

    /// **碰撞位不参与地表匹配**（官方 `& 0x1FFFFFFF` 把碰撞位掩掉）：
    /// 只差 `bit15` 的两格属于**同一个**连通块。
    #[test]
    fn 填充匹配键不看重阻挡位() {
        let mut map = 造图(4, 1, CellFormat::New14);
        for c in map.cells.iter_mut() {
            c.bk_img = 100; // 同图号
            c.tiles = 3;
        }
        // 第 0 格置上低阻挡位：图号仍是 100，只有 bit15 不同
        let i0 = index_of(&map, 0, 0).unwrap();
        map.cells[i0].bk_img = 0x8000 | 100;
        assert_eq!(
            fill_key(EditLayer::Ground, map.cell(0, 0).unwrap()),
            fill_key(EditLayer::Ground, map.cell(1, 0).unwrap()),
            "只差 bit15 的格必须同键"
        );
        // 图号不同 ⇒ 不同键
        let i3 = index_of(&map, 3, 0).unwrap();
        map.cells[i3].bk_img = 101;
        assert_ne!(
            fill_key(EditLayer::Ground, map.cell(0, 0).unwrap()),
            fill_key(EditLayer::Ground, map.cell(3, 0).unwrap())
        );
        // 库号不同 ⇒ 不同键
        let i2 = index_of(&map, 2, 0).unwrap();
        map.cells[i2].tiles = 9;
        assert_ne!(
            fill_key(EditLayer::Ground, map.cell(0, 0).unwrap()),
            fill_key(EditLayer::Ground, map.cell(2, 0).unwrap())
        );
        // 中层看的是 mid_img（不做掩码），阻挡层看的是"该位是否已置"
        assert_eq!(
            fill_key(EditLayer::LowCollision, map.cell(0, 0).unwrap()),
            FillKey::Collision(true)
        );
        assert_eq!(
            fill_key(EditLayer::LowCollision, map.cell(1, 0).unwrap()),
            FillKey::Collision(false)
        );
        assert_eq!(
            fill_key(EditLayer::Front, map.cell(1, 0).unwrap()),
            FillKey::Unsupported
        );
    }

    // ───── 2026-11-04 数据正确性：写图号不许清掉 bit15（验收门 §四.2~§四.5） ─────
    //
    // 真值链见 [`EditLayer::img_of_has_collision_bit`] 与 `paint()` 的 ③：
    // C# `MapViewport.cs:480 / :520 / :546` 保留 `0x60000000` 两个碰撞位
    // ⇔ 文件级 `bk_img` / `fr_img` 各自的 `bit15`；`mid_img` 是纯图号。

    /// 三档**真实地图**的只读来源（验收门 §四.2 / §四.3 的 hex 证据）。
    ///
    /// ⚠ 只用 `std::fs::read` 读进内存、`Map::parse` 后在内存里改，**绝不写回磁盘**：
    /// 这些文件在 `client/` 与 `GodotClient/` 下，本单只允许改 `miredit-gui`。
    /// 路径相对资产根（`MIREDIT_ASSET_ROOT`）；**缺根/缺文件时打印 `[skip]` 并跳过**
    /// （公开仓/PR 的机器上没有游戏素材，不算代码错）。
    fn 三档真图() -> Option<[(&'static str, std::path::PathBuf, CellFormat); 3]> {
        let root = 资产根()?;
        Some([
            ("Old12", 语料_b(&root).join("0.map"), CellFormat::Old12),
            ("New14", 语料_a(&root).join("41.map"), CellFormat::New14),
            ("Ext36", 语料_b(&root).join("n0104.map"), CellFormat::Ext36),
        ])
    }

    /// 只读载入一张真图，并核档位 + 往返逐字节相等（不靠猜）。
    ///
    /// 调用方须先确认文件存在；**文件在但读不出来 ⇒ panic**（那是真故障，不许静默跳过）。
    fn 载入真图(tag: &str, path: &Path, want: CellFormat) -> Map {
        let bytes = std::fs::read(path)
            .unwrap_or_else(|e| panic!("真值地图读不到：{tag} {} ⇒ {e}", path.display()));
        let map = Map::parse(&bytes)
            .unwrap_or_else(|e| panic!("真值地图解析失败：{tag} {} ⇒ {e}", path.display()));
        assert_eq!(map.format, want, "{tag} 档位与派单口径不符");
        assert_eq!(map.to_bytes(), bytes, "{tag} 往返必须逐字节相等");
        map
    }

    /// 找第一格 `bk_img` 带 `bit15` 且低 15 位不是 10 的格（保证落笔会变）。
    fn 找带bit15格(map: &Map) -> (u16, u16, usize) {
        let h = map.height as usize;
        for i in 0..map.cells.len() {
            let c = &map.cells[i];
            if c.bk_img & COLLISION_BIT != 0 && c.bk_img & SPRITE_MAX != 10 {
                return ((i / h) as u16, (i % h) as u16, i);
            }
        }
        panic!("真值地图里找不到带 bit15 的格");
    }

    /// **三档合成图 × 画笔**：写图号只换低 15 位，`bit15` 前后相同、其余格 0 差异、
    /// `walkable` 前后一致。自包含版本（不依赖外部文件），作稳定回归。
    #[test]
    fn 三档合成图画笔写图号保留bit15() {
        for fmt in [CellFormat::Old12, CellFormat::New14, CellFormat::Ext36] {
            let mut map = 造图(4, 4, fmt);
            let idx = index_of(&map, 2, 1).unwrap();
            map.cells[idx].bk_img = 0x815F;
            let orig = map.to_bytes();
            let lib_byte = EditLayer::Ground.lib_byte_of(&map.cells[idx]);
            let mut ed = EditSession::default();
            ed.set_layer(EditLayer::Ground);
            // 库号对齐目标格 ⇒ 库号字节写回同值，差异只可能落在图号 2 字节
            ed.set_brush_from_palette(图库(EditLayer::Ground, u16::from(lib_byte)), 9)
                .expect("合法帧应能设为笔刷");
            assert_eq!(ed.apply_once(&mut map, 2, 1), 1, "{fmt:?} 应真的改一格");
            let d = byte_diff(&map, &orig);
            assert_eq!(d.header, 0, "{fmt:?} 头部不许动");
            assert_eq!(d.cells, 1, "{fmt:?} 只许涉及目标格");
            assert_eq!(d.bytes, 2, "{fmt:?} 只许动 bk_img 的 2 字节");
            let orig_map = Map::parse(&orig).unwrap();
            assert_eq!(map.cells[..idx], orig_map.cells[..idx], "{fmt:?} 之前 0 差异");
            assert_eq!(map.cells[idx + 1..], orig_map.cells[idx + 1..], "{fmt:?} 之后 0 差异");
            let after = map.cells[idx];
            assert_eq!(after.bk_img, 0x800A, "{fmt:?} 0x815F 写 10 应得 0x800A");
            assert_eq!(
                after.bk_img & COLLISION_BIT,
                orig_map.cells[idx].bk_img & COLLISION_BIT,
                "{fmt:?} bit15 前后相同"
            );
            assert_eq!(
                map.walkable(2, 1),
                orig_map.walkable(2, 1),
                "{fmt:?} 改图号不许改变可走性"
            );
            eprintln!(
                "BIT15_SYNTH fmt={fmt:?} before=0x815F after=0x{:04X} bit15=1→{} diff_bytes={} diff_cells={} other_cells=0 walkable=false→{}",
                after.bk_img,
                u16::from(after.bk_img & COLLISION_BIT != 0),
                d.bytes,
                d.cells,
                map.walkable(2, 1),
            );
        }
    }

    /// **三档真图 × 画笔**（验收门 §四.2）：`0.map` / `41.map` / `n0104.map`
    /// 副本各找一格带 `bit15` 的写图号 10，逐字节核对「只有低 15 位变、
    /// bit15 前后相同、其余格 0 差异」，并打印 hex。
    #[test]
    fn 三档真实地图画笔写图号保留bit15() {
        let Some(三档) = 三档真图() else {
            eprintln!("[skip] 无资产根 MIREDIT_ASSET_ROOT");
            return;
        };
        for (tag, path, fmt) in 三档 {
            if !path.is_file() {
                eprintln!("[skip] 缺文件 {}", path.display());
                continue;
            }
            let mut map = 载入真图(tag, &path, fmt);
            let (x, y, idx) = 找带bit15格(&map);
            let orig = map.to_bytes();
            let before = map.cells[idx];
            let before_walk = map.walkable(x, y);
            let lib_byte = EditLayer::Ground.lib_byte_of(&before);
            let mut ed = EditSession::default();
            ed.set_layer(EditLayer::Ground);
            ed.set_brush_from_palette(图库(EditLayer::Ground, u16::from(lib_byte)), 9)
                .expect("合法帧应能设为笔刷");
            assert_eq!(ed.apply_once(&mut map, x, y), 1, "{tag} 应真的改一格");

            let after = map.cells[idx];
            let d = byte_diff(&map, &orig);
            let base = HEADER_LEN + idx * fmt.cell_size();
            let offsets = 差异偏移(&map, &orig);
            assert_eq!(d.header, 0, "{tag} 头部不许动");
            assert_eq!(d.cells, 1, "{tag} 只许涉及目标格");
            assert!(!offsets.is_empty(), "{tag} 图号确实要变");
            assert!(
                offsets.iter().all(|o| *o == base || *o == base + 1),
                "{tag} 差异只许落在 bk_img 的两个字节内，实得 {offsets:?}"
            );
            let orig_map = Map::parse(&orig).unwrap();
            assert_eq!(map.cells[..idx], orig_map.cells[..idx], "{tag} 目标格之前 0 差异");
            assert_eq!(map.cells[idx + 1..], orig_map.cells[idx + 1..], "{tag} 目标格之后 0 差异");
            assert_eq!(
                after.bk_img & COLLISION_BIT,
                before.bk_img & COLLISION_BIT,
                "{tag} bit15 前后相同"
            );
            assert_eq!(
                after.bk_img,
                (before.bk_img & COLLISION_BIT) | 10,
                "{tag} 只把低 15 位换成 10"
            );
            assert_eq!(map.walkable(x, y), before_walk, "{tag} 改图号不许改变可走性");
            eprintln!(
                "BIT15_HEX tag={tag} file={} xy=({x},{y}) before=0x{:04X} after=0x{:04X} bit15={}→{} diff_bytes={} diff_cells={} other_cells=0 walkable={}→{} fmt={fmt:?}",
                path.display(),
                before.bk_img,
                after.bk_img,
                u16::from(before.bk_img & COLLISION_BIT != 0),
                u16::from(after.bk_img & COLLISION_BIT != 0),
                d.bytes,
                d.cells,
                before_walk,
                map.walkable(x, y),
            );
        }
    }

    /// **三族各自保留 `bit15` 的裁定矩阵**（父代理追加要求）：
    /// 地表 `bk_img` 保留、物体 `fr_img` 保留、中层 `mid_img` 不保留。
    /// 防止以后有人「统一保留」或「统一不保留」把某一层改错。
    #[test]
    fn 三族写图号bit15保留矩阵() {
        for (layer, keep) in [
            (EditLayer::Ground, true),
            (EditLayer::Middle, false),
            (EditLayer::Front, true),
        ] {
            assert_eq!(
                layer.img_of_has_collision_bit(),
                keep,
                "{layer:?} 的字段语义裁定不许漂"
            );
            let mut map = 造图(3, 3, CellFormat::New14);
            let idx = index_of(&map, 1, 1).unwrap();
            // 三个图号字段都塞上 bit15；中层那个是「异常数据」，按纯图号处理
            map.cells[idx].bk_img = 0x8000 | 111;
            map.cells[idx].mid_img = 0x8000 | 222;
            map.cells[idx].fr_img = 0x8000 | 333;
            let before = map.cells[idx];
            let mut ed = EditSession::default();
            ed.set_layer(layer);
            ed.set_brush_from_palette(图库(layer, 3), 9)
                .expect("合法帧应能设为笔刷");
            assert_eq!(ed.apply_once(&mut map, 1, 1), 1, "{layer:?} 应真的改一格");
            let after = map.cells[idx];
            let want = if keep { 0x800A } else { 0x000A };
            assert_eq!(
                layer.img_of(&after),
                want,
                "{layer:?} 写图号 10 后 bit15 裁定：keep={keep}"
            );
            for other in EditLayer::IMAGE {
                if other != layer {
                    assert_eq!(
                        other.img_of(&after),
                        other.img_of(&before),
                        "{layer:?} 落笔不许动 {other:?}"
                    );
                }
            }
            assert_eq!(
                map.walkable(1, 1),
                before.is_walkable(),
                "{layer:?} 写图号不许改变可走性"
            );
            eprintln!(
                "BIT15_MATRIX layer={layer:?} field={} keep={keep} before=0x{:04X} after=0x{:04X}",
                layer.img_field(),
                layer.img_of(&before),
                layer.img_of(&after),
            );
        }
    }

    /// **填充 912 格**（验收门 §四.3）：复跑主线程用例 `0.map` 点 `(358,0)`。
    /// 旧逻辑 912 格全变 `0x0001`；修复后必须全变 `0x8001`（保留 bit15），
    /// 「bit15 变化格数 = 0」且全图 `walkable` 一格不变。
    #[test]
    fn 填充九百一十二格全部保留bit15() {
        let Some(三档) = 三档真图() else {
            eprintln!("[skip] 无资产根 MIREDIT_ASSET_ROOT");
            return;
        };
        let (tag, path, fmt) = &三档[0];
        assert_eq!(*fmt, CellFormat::Old12, "主线程用例用的是 0.map / Old12");
        if !path.is_file() {
            eprintln!("[skip] 缺文件 {}", path.display());
            return;
        }
        let (tag, fmt) = (*tag, *fmt);
        let mut map = 载入真图(tag, path, fmt);
        let clicked = *map.cell(358, 0).unwrap();
        assert_eq!(clicked.bk_img, 0x815F, "真值：(358,0) 带 bit15、低 15 位 0x015F");
        let orig = map.to_bytes();
        let orig_map = Map::parse(&orig).unwrap();
        let mut ed = EditSession::default();
        ed.set_layer(EditLayer::Ground);
        // 帧索引 0 ⇒ 图号 1（主线程实测旧结果 0x0001）
        ed.set_brush_from_palette(mir2_assets::LibRef::ground(0), 0)
            .expect("合法帧应能设为笔刷");
        let out = ed.fill(&mut map, 358, 0);
        assert_eq!(out, FillOutcome::Filled(912), "主线程复现：912 格");

        let mut bit15_changed = 0usize;
        let mut walk_changed = 0usize;
        let mut changed = 0usize;
        let mut before_set = std::collections::BTreeSet::new();
        let mut after_set = std::collections::BTreeSet::new();
        for i in 0..map.cells.len() {
            let b = orig_map.cells[i];
            let a = map.cells[i];
            if (a.bk_img ^ b.bk_img) & COLLISION_BIT != 0 {
                bit15_changed += 1;
            }
            if a.is_walkable() != b.is_walkable() {
                walk_changed += 1;
            }
            if a != b {
                changed += 1;
                before_set.insert(b.bk_img);
                after_set.insert(a.bk_img);
            }
        }
        let before_hex: Vec<String> = before_set.iter().map(|v| format!("0x{v:04X}")).collect();
        let after_hex: Vec<String> = after_set.iter().map(|v| format!("0x{v:04X}")).collect();
        assert_eq!(bit15_changed, 0, "bit15 变化格数必须为 0");
        assert_eq!(walk_changed, 0, "改图号不得改变任何格的可走性");
        assert_eq!(changed, 912, "真改的格数 = 912");
        assert_eq!(before_hex, vec!["0x815F"], "块内改前取值集合");
        assert_eq!(after_hex, vec!["0x8001"], "块内改后取值集合（不再是 0x0001）");
        eprintln!(
            "BIT15_FILL tag={tag} clicked=(358,0) cells=912 before_set=[{}] after_set=[{}] bit15_changed=0 walk_changed=0 old_logic_after=0x0001",
            before_hex.join(","),
            after_hex.join(","),
        );
    }

    /// **`walkable` 前后对比**（验收门 §四.4）：画笔 / 填充改图号都不得改变可走性。
    /// 覆盖地表与物体两个「带碰撞位字段」，以及中层（本就不参与可走性）。
    #[test]
    fn 画笔与填充改图号不改walkable() {
        let mut map = 造图(4, 4, CellFormat::New14);
        for c in map.cells.iter_mut() {
            c.bk_img = 0;
            c.mid_img = 0;
            c.fr_img = 0;
        }
        let i00 = index_of(&map, 0, 0).unwrap();
        let i10 = index_of(&map, 1, 0).unwrap();
        let i20 = index_of(&map, 2, 0).unwrap();
        let i30 = index_of(&map, 3, 0).unwrap();
        map.cells[i00].bk_img = 0x0050; // 可走
        map.cells[i10].bk_img = 0x8050; // 低位阻挡 ⇒ 不可走
        map.cells[i20].fr_img = 0x8060; // 高位阻挡 ⇒ 不可走
        // (3,0) 两层皆 0 ⇒ 可走
        let before = [
            map.walkable(0, 0),
            map.walkable(1, 0),
            map.walkable(2, 0),
            map.walkable(3, 0),
        ];
        assert_eq!(before, [true, false, false, true]);

        // ① 地表画笔：写 (0,0) 与 (1,0)
        let mut ed = EditSession::default();
        ed.set_layer(EditLayer::Ground);
        ed.set_brush_from_palette(mir2_assets::LibRef::ground(3), 9).unwrap();
        assert_eq!(ed.apply_once(&mut map, 0, 0), 1);
        assert_eq!(ed.apply_once(&mut map, 1, 0), 1);
        // ② 物体画笔：写 (2,0) 与 (3,0)
        ed.set_layer(EditLayer::Front);
        ed.set_brush_from_palette(mir2_assets::LibRef::surface(3), 9).unwrap();
        assert_eq!(ed.apply_once(&mut map, 2, 0), 1);
        assert_eq!(ed.apply_once(&mut map, 3, 0), 1);
        // ③ 中层画笔（纯图号，不参与可走性）
        ed.set_layer(EditLayer::Middle);
        ed.set_brush_from_palette(mir2_assets::LibRef::middle(3), 9).unwrap();
        assert_eq!(ed.apply_once(&mut map, 0, 0), 1);
        let after_brush = [
            map.walkable(0, 0),
            map.walkable(1, 0),
            map.walkable(2, 0),
            map.walkable(3, 0),
        ];
        assert_eq!(after_brush, before, "三族画笔改图号都不许改变可走性");
        assert_eq!((map.cells[i00].bk_img, map.cells[i10].bk_img), (0x000A, 0x800A));
        assert_eq!((map.cells[i20].fr_img, map.cells[i30].fr_img), (0x800A, 0x000A));
        eprintln!("BIT15_WALK brush before={before:?} after={after_brush:?}");

        // ④ 填充：地表一块「低 15 位同图号、bit15 混合」的连通块
        //    （`fill_key` 掩掉 bit15 ⇒ 带/不带阻挡的偶偶格算同一块）
        let mut m2 = 同号图(6, 6, 7);
        for x in 0..6u16 {
            for y in 0..6u16 {
                if x % 2 == 0 && y % 2 == 0 && (x / 2 + y / 2) % 2 == 0 {
                    let i = index_of(&m2, x, y).unwrap();
                    m2.cells[i].bk_img |= COLLISION_BIT;
                }
            }
        }
        let mut walk_before = Vec::new();
        for x in 0..6u16 {
            for y in 0..6u16 {
                walk_before.push(m2.walkable(x, y));
            }
        }
        let bit_before: Vec<u16> = m2.cells.iter().map(|c| c.bk_img & COLLISION_BIT).collect();
        let mut ed2 = EditSession::default();
        ed2.set_layer(EditLayer::Ground);
        ed2.set_brush_from_palette(mir2_assets::LibRef::ground(0), 0).unwrap();
        assert_eq!(ed2.fill(&mut m2, 0, 0), FillOutcome::Filled(9), "6×6 偶偶格 = 9");
        for (i, c) in m2.cells.iter().enumerate() {
            assert_eq!(c.bk_img & COLLISION_BIT, bit_before[i], "第 {i} 格 bit15 不许变");
        }
        let mut walk_after = Vec::new();
        for x in 0..6u16 {
            for y in 0..6u16 {
                walk_after.push(m2.walkable(x, y));
            }
        }
        assert_eq!(walk_after, walk_before, "填充不许改变任何格的可走性");
        assert!(walk_before.iter().any(|v| *v) && walk_before.iter().any(|v| !*v), "块内确实混合可走/不可走");
        eprintln!(
            "BIT15_WALK fill cells=9 walk_before={}true/{}false walk_after={}true/{}false",
            walk_before.iter().filter(|v| **v).count(),
            walk_before.iter().filter(|v| !**v).count(),
            walk_after.iter().filter(|v| **v).count(),
            walk_after.iter().filter(|v| !**v).count(),
        );
    }

    /// **拖动画笔 / `--edit-brush` 脚本**（验收门 §四.2 路径覆盖）：
    /// `paint_stroke_once`（尺寸 3）、`paint_stroke`（拖动）、`apply_script`
    /// 的 Brush / Stroke / Fill 全部经 `touch_footprint` → `touch` → `paint`，
    /// 逐格断言 `bit15` 前后相同。
    #[test]
    fn 拖动画笔与脚本落笔保留bit15() {
        // ① 尺寸 3 落笔 + 一次拖动（内存图）
        let mut map = 造图(8, 8, CellFormat::New14);
        for c in map.cells.iter_mut() {
            c.bk_img = 0x8000 | 7;
        }
        let bit_before: Vec<u16> = map.cells.iter().map(|c| c.bk_img & COLLISION_BIT).collect();
        let mut ed = EditSession::default();
        ed.set_brush_from_palette(mir2_assets::LibRef::ground(0), 0).unwrap();
        ed.set_brush_size(3);
        assert!(ed.paint_stroke_once(&mut map, 3, 3) > 0, "尺寸 3 单点应落 9 格");
        assert!(ed.paint_stroke(&mut map, Some((6, 6)), (6, 6)) > 0, "拖动应落格");
        for (i, c) in map.cells.iter().enumerate() {
            assert_eq!(c.bk_img & COLLISION_BIT, bit_before[i], "第 {i} 格 bit15 不许变");
        }

        // ② 真正的 `--edit-*` 脚本（与 GUI 共用 `apply_script`）
        let dir = std::env::temp_dir().join("miredit_gui_bit15_script");
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("bit15-8x8.map");
        let mut bytes = vec![0u8; HEADER_LEN + 12 * 64];
        bytes[0..2].copy_from_slice(&8u16.to_le_bytes());
        bytes[2..4].copy_from_slice(&8u16.to_le_bytes());
        for i in 0..64usize {
            bytes[HEADER_LEN + i * 12..HEADER_LEN + i * 12 + 2]
                .copy_from_slice(&(0x8007u16).to_le_bytes());
        }
        // (7,7) 作笔刷源：纯图号 1、不带 bit15（列优先下标 = 7*8+7 = 63）
        bytes[HEADER_LEN + 63 * 12..HEADER_LEN + 63 * 12 + 2]
            .copy_from_slice(&1u16.to_le_bytes());
        std::fs::write(&p, &bytes).unwrap();

        let mut doc = crate::mapdoc::DocState::load(Some(&p));
        let mut ed2 = EditSession::default();
        let rep = apply_script(
            &mut doc,
            &mut ed2,
            &[
                EditCmd::Pickup(7, 7),
                EditCmd::Brush(2, 2),
                EditCmd::Stroke((4, 4), (4, 4)),
                EditCmd::Fill(0, 0),
            ],
        );
        assert!(rep.edited_cells > 0, "脚本必须真的改了图号");
        let m = &doc.doc.as_ref().expect("临时地图应能打开").map;
        for x in 0..8u16 {
            for y in 0..8u16 {
                let want = if (x, y) == (7, 7) { 0 } else { COLLISION_BIT };
                assert_eq!(
                    m.cell(x, y).unwrap().bk_img & COLLISION_BIT,
                    want,
                    "脚本 ({x},{y}) 的 bit15 不许变"
                );
            }
        }
        // 图号确实被改过（不是 bit15 没变就当作啥也没干）
        let changed_low15 = m
            .cells
            .iter()
            .filter(|c| c.bk_img & SPRITE_MAX != 7)
            .count();
        assert!(changed_low15 > 0, "脚本至少改了一些格的低 15 位图号");
        eprintln!(
            "BIT15_SCRIPT edited_cells={} changed_low15={changed_low15} all_bit15_unchanged=true",
            rep.edited_cells
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **阻挡层回归**（验收门 §四.5）：图号层写完图号后，低位/高位阻挡的
    /// 置位（`| 0x8000`）与清位（`& 0x7FFF`）仍只动各自那一个 bit。
    #[test]
    fn 图号层写图号不影响阻挡层置位清位() {
        for coll in [EditLayer::LowCollision, EditLayer::HighCollision] {
            let mut map = 造图(3, 3, CellFormat::New14);
            // 先用**地表笔刷会话**写一格图号（本单修的分支）；
            // 阻挡层会话一律没有图库笔刷（图库笔刷在阻挡层会被拒），
            // 这与既有「阻挡层置位/清位只动 bit15」用例的口径一致。
            let mut img_ed = EditSession::default();
            img_ed.set_layer(EditLayer::Ground);
            img_ed
                .set_brush_from_palette(mir2_assets::LibRef::ground(2), 9)
                .unwrap();
            assert_eq!(img_ed.apply_once(&mut map, 1, 1), 1);
            let mut ed = EditSession::default();
            let after_img = *map.cell(1, 1).unwrap();
            assert_eq!(after_img.bk_img, 0x000A, "目标格本来可走 ⇒ 写图号 10 仍可走");
            assert!(map.walkable(1, 1), "写图号后可走");

            // 阻挡层置位：只动 bit15，低 15 位图号原样
            ed.set_layer(coll);
            ed.select(TOOL_BRUSH_IDX);
            assert_eq!(ed.apply_once(&mut map, 1, 1), 1, "{coll:?} 置位应改一格");
            let set = *map.cell(1, 1).unwrap();
            assert!(!map.walkable(1, 1), "{coll:?} 置位后不可走");
            match coll {
                EditLayer::LowCollision => {
                    assert_eq!(set.bk_img, 0x800A, "低位阻挡只置 bk_img 的 bit15");
                    assert_eq!(set.fr_img, after_img.fr_img, "fr_img 不许动");
                }
                _ => {
                    assert_eq!(set.fr_img, after_img.fr_img | COLLISION_BIT, "高位阻挡只置 fr_img 的 bit15");
                    assert_eq!(set.bk_img, 0x000A, "bk_img 不许动");
                }
            }

            // 阻挡层清位：只清 bit15，低 15 位图号原样
            ed.select(TOOL_ERASER_IDX);
            assert_eq!(ed.apply_once(&mut map, 1, 1), 1, "{coll:?} 清位应改一格");
            let cleared = *map.cell(1, 1).unwrap();
            assert!(map.walkable(1, 1), "{coll:?} 清位后恢复可走");
            assert_eq!(cleared.bk_img, 0x000A, "清位后地表图号还是 10");
            assert_eq!(cleared.fr_img & SPRITE_MAX, after_img.fr_img & SPRITE_MAX, "物件图号不许动");
            eprintln!(
                "BIT15_COLLISION layer={coll:?} after_img=0x{:04X} set=0x{:04X}/0x{:04X} cleared=0x{:04X} walk=true→false→true",
                after_img.bk_img, set.bk_img, set.fr_img, cleared.bk_img,
            );
        }
    }
}