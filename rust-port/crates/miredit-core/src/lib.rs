//! `miredit-core` —— 地图编辑器的**编辑内核**（无 UI 依赖）。
//!
//! 从 `miredit-gui` 里把「编辑内核」抽出来做成共享层，让 **PC 原生版**与**将来的网页版**
//! 复用同一套逻辑：图层 / 笔刷 / 落笔 / 填充 / 撤销 / 原子保存 / 可序列化命令。
//! 这是"模块化重构"的第二步（第一步是 `mir2-format`，另路进行）。
//!
//! # 一、依赖白名单（任务书 §七；**2026-11-05 扩容一条**）
//!
//! `std` + [`mir2_map`] + [`mir2_format`] + [`mir2_stamp`]。
//!
//! | 依赖 | 角色 | 为什么不能换掉它 |
//! |---|---|---|
//! | [`mir2_format`] | **字节层真值**：九种格式的判别 / 读写 / 未解尾区 | 只有它知道九种格式的头部长度、格长、尾区 |
//! | [`mir2_map`] | **编辑/渲染视图**：三档格长 + 逐格字节 | 渲染层（`miredit-render`）的入口签名就是 `&mir2_map::Map`，编辑逻辑与 103 条既有测试都钉在它上面 |
//! | [`mir2_stamp`] | **`.X` 印章字节层**：解析 / 往返 / 目录索引 | [`stamp_convert`] 要吃 `.X` 的真字段（含 `middle_frames`/`tile_*` 这些视图装不下的），自己再解析一遍就等于把格式知识抄第二份 |
//!
//! ⚠ **`mir2_stamp` 是 2026-11-05 从 `[dev-dependencies]` 上移的**，代价要记账：
//! 它带 `serde` + `serde_json`（只为读 `catalogue/palette.json`），上移后这两个 crate 进入
//! **库本体**依赖树。它们本来就在本 workspace 的 `Cargo.lock` 里（2026-11-06 起由
//! `mir2-stamp` 与 `miredit-web` 引入；vendored 的 `wil2atlas` 子集已裁掉 serde）
//! ⇒ 不新增锁文件版本、不新增传递依赖树；但「内核只依赖 std + mir2-map + mir2-format」
//! 这条**原始白名单从此作废**，换来的是「印章 id → 格表」只有一个入口（[`stamp_convert`]）。
//!
//! **不依赖** `miredit-gui`（那是被抽取方）、`mir2-assets`（`Family` / `LibRef` 在本 crate
//! 按同口径重新定义，见 [`brush`] 模块文档）、`serde`（[`Command`] 只依赖 `std`，
//! 网页层自己做序列化；`serde` 只作为 `mir2_stamp` 的传递依赖存在，本 crate 一行不写）。
//!
//! # 二、双表示（2026-11-05 接入 `mir2-format`）
//!
//! [`Document`] 同时持有两份表示：`raw: mir2_format::RawMap`（字节真值）与
//! `view: mir2_map::Map`（编辑/渲染视图）。**打开**时 `read_any_tolerant` → `RawMap` →
//! 投影成 `Map`；**编辑**只改 `view`；**保存**把 `view` 的格按位宽写回 `raw`，
//! 再由 `raw.format` 的 writer 出字节（`header_raw` + `trailer` 原样带上）。
//!
//! ⇒ 九种格式**全部**能打开、能编辑、能按原格式写回；未改即存 = 逐字节相同。
//! 细节（字段映射表 / 位宽处理 / 信息丢失清单）见 [`convert`] 模块文档。
//!
//! # 三、必须保持的语义（**逐条照搬，不许"优化"**）
//!
//! | # | 语义 | 判据落点 | 来源 |
//! |---|---|---|---|
//! | 1 | **`bit15` 保留矩阵**：地表 `bk_img` 保留、物体 `fr_img` 保留、中层 `mid_img` 不保留；笔刷自身也掩掉 `bit15` | [`EditLayer::img_of_has_collision_bit`] / [`paint`] | `edit.rs:1462-1468`；`MapViewport.cs:480`/`:520`/`:546`；`mir2-map/src/cell.rs:205-212`；`TMap.cpp:339` |
//! | 2 | **橡皮**：图号层 `cur & 0x8000`（保留阻挡位）；阻挡层 `cur & !0x8000` | [`paint`] | `edit.rs:1475-1483` |
//! | 3 | **阻挡层画笔**：`cur \| 0x8000`，**只动 `bit15`** | [`paint`] | `edit.rs:1413-1416` |
//! | 4 | **族 / 层一致性**：笔刷族与当前层不匹配 ⇒ 拒绝（不写）；图库笔刷在阻挡层 ⇒ 拒绝 | [`paint`] → [`PaintOutcome`] | `edit.rs:1404-1432` |
//! | 5 | **填充**：地表 `step=2` 并对齐偶偶格；匹配键 `(库号, 图号 & 0x7FFF)`；四邻 BFS；上限 `200_000` 格且**超限一格不写**；整次填充 = 撤销一步 | [`fill`] / [`EditSession::fill`] | `edit.rs:1036-1206`；`MapViewport.cs:485-539` |
//! | 6 | **撤销**：整格快照；一次拖动/填充 = 一步；撤销后 `to_bytes()` 与操作前**逐字节相等** | [`undo`] / [`UndoStack`] | `edit.rs:554-579`、`:1288-1323` |
//! | 7 | **保存**：`tmp → write_all → sync_all → 读回逐字节核对 → rename → 尽力 fsync 父目录`；目录 fsync 失败**不改结论**，只报告 | [`save`] | `save.rs:1-330` |
//! | 8 | **格式能力闸门**（新增）：`ground_paint_supported=false`（Type5）⇒ 地表层落笔**明确拒绝**并给中文原因；writer 不写的库号字节 ⇒ 还原成改前值并说明 | [`EditSession::touch`] / [`EditSession::fill`] | `mir2-format/src/model.rs:430-440`、`:419-461` |
//! | 9 | **印章放置**（新增）：锚点 + 相对偏移（可负）；出界**逐格跳过**并计数、全出界 ⇒ 拒绝；三族图号 + 库号字节；`bit15` 矩阵复用 [`paint`]；整次放置 = **一步撤销** | [`EditSession::place_stamp`] / [`stamp`] | `CrystalStudio/PlacedStamps.cs:80`、`:87-98`、`:114-121`；`mir2-stamp/src/stamp.rs:22-32` |
//! | 10 | **矩形选区 + 剪贴板**（新增）：选区是**闭区间**（C# `Rect` 半开 + 构造时 `+1`，两者等价）；越界**入口裁剪一次**；复制**整块都抓**（含空格）；删除/剪切**三族按层清 + 库号字节写 0**（取 `CutSource` 口径）；粘贴**复用 `plan_stamp`**（逐格越界跳过 + Type5 闸门 + 保留目标 `bit15`）；**整次操作 = 一步撤销**；选区/剪贴板**不进撤销栈、不置脏** | [`EditSession::copy_selection`] / [`selection`] | `MapViewport.cs:102`、`:115`、`:390`、`:399-400`、`:540`、`:546`；`RegionCopy.cs:17`、`:21-27`、`:45-69`、`:70-91`、`:101-110` |
//! | 11 | **`.X` → 格表**（新增）：三族图号一律 `& 0x7FFF`；三族库号超出 `0..=255` **报错不截断**（消息含「第 N 格 / 地表\|中层\|物体 / 值」）；`door`/`door_offset`/`light`/`ani_frame`/`ani_tick` 与 5 个「视图装不下」的动画字段**全部照搬真值** | [`cells_from_stamp`] / [`stamp_convert`] | `mir2-stamp/src/cell.rs`（v2 格体偏移表）；`CrystalStudio/README.md:205-207` |
//! | 12 | **五层计数**（新增）：官方掩码 = C# **归一化内存表示**（legacy `bit15`→29/30、Mir3 `flag` bit0/1、u32 族磁盘 bit29/30）；单格 **O(1) 增量**、整块换视图后**必须** `rebuild_layer_counts()`；**照抄磁盘 bit29/30 会得到几乎全 0 的假计数**（40 张实测 39 张全 0） | [`layer_counts`] / [`Document::write_view_cell`] | `Document.LayerCounts.cs:5-25@acfbbf8a`；`LegacyMapReader.cs:149-150`、`:160-161`、`:438-439`、`:496-497`、`:545-546@acfbbf8a` |
//!
//! # 四、模块地图
//!
//! | 模块 | 负责 | 抽取源 |
//! |---|---|---|
//! | [`document`] | `Document`：双表示（`raw` + `view`）+ 路径 + 脏标记 + `to_bytes` + 逐字节差异 | `mapdoc.rs:22-44`、`edit.rs:1326-1346`、`:1669-1720` |
//! | [`convert`] | `raw ⇄ view` 的**字段映射表**、位宽处理、信息丢失清单、格式能力闸门 | 新增（接入 `mir2-format`） |
//! | [`layer`] | `EditLayer`（五层）+ 字段映射 + **`bit15` 保留矩阵** | `edit.rs:127-360` |
//! | [`layer_counts`] | **五层计数**：官方掩码的按格式归一化（legacy `bit15` / Mir3 `flag` / u32 `bit29/30`）+ 单格增量 + 整块重建 | `Document.LayerCounts.cs:5-25@acfbbf8a`（2026-11-06 新增） |
//! | [`brush`] | `Brush`（族/图号/库号/来源）+ `PickedCell` + 笔刷尺寸规范化 | `edit.rs:459-552`、`metrics.rs:159-199` |
//! | [`paint`] | `paint()`：单格落笔（按层分派；N×N 由会话展开） | `edit.rs:1360-1488` |
//! | [`fill`] | 填充匹配键 + 四邻 BFS + `step=2` + 上限 + **先收块后动笔** | `edit.rs:1490-1610`、`:1036-1206` |
//! | [`stamp`] | 印章放置：逐格越界裁剪 + 三族写字段 + Type5 闸门 + **先算后写** | `CrystalStudio/PlacedStamps.cs:52-121`（新增） |
//! | [`stamp_convert`] | `.X` 印章 → [`StampCellInput`]：三族图号掩 `0x7FFF` + 库号越界即错 + 12 个附加字段照搬 | `mir2-stamp/src/cell.rs`（2026-11-05 新增） |
//! | [`selection`] | 矩形选区 + 剪贴板：闭区间语义 + 越界裁剪 + 清空计划 + 复制快照 | `MapViewport.cs:102-546`、`RegionCopy.cs:1-133`（新增） |
//! | [`undo`] | 撤销栈（整格快照、一次拖动/填充 = 一步、上限 100） | `edit.rs:554-579`、`:851-960`、`:1288-1323` |
//! | [`session`] | `EditSession`：文档 + 工具 + 笔刷 + 层 + 尺寸 + 撤销栈（N×N 在这里展开） | `edit.rs:581-1324` |
//! | [`save`] | 原子保存 + 目录 fsync 分类 | `save.rs:1-330` |
//! | [`cmd`] | `Command` / `Outcome` / `apply()`（**给网页版**：可序列化） | 新增（任务书 §五） |
//! | [`error`] | `CoreError` / `SaveStage` | 新增（GUI 侧用 `String`，本 crate 结构化） |
//!
//! # 五、最短用法
//!
//! ```no_run
//! use miredit_core::{apply, Command, EditSession};
//!
//! // ① 打开一张图（会话持有文档，见 session.rs 模块文档第一节）
//! let mut ed = EditSession::load(std::path::Path::new("<maps>/0.map"))?;
//! // ② 切层 → 吸管 → 落笔 → 撤销 → 保存，全部走可序列化命令
//! apply(&mut ed, Command::SetLayer { layer: "ground".into() })?;
//! apply(&mut ed, Command::Pick { x: 0, y: 0 })?;
//! let out = apply(&mut ed, Command::Paint { x: 10, y: 10 })?;
//! println!("改了 {} 格，脏={}，可撤销={}", out.changed, out.dirty, out.can_undo);
//! apply(&mut ed, Command::Undo)?;
//! apply(&mut ed, Command::Save)?;
//! // ③ 常驻告警（未解尾块 / 本格式不承诺编辑语义）——GUI/网页应常驻显示
//! if let Some(w) = ed.edit_warning() {
//!     eprintln!("[告警] {w}");
//! }
//! # Ok::<(), miredit_core::CoreError>(())
//! ```
//!
//! # 六、`bit15` 的位运算只有四处（都在 [`paint`] 里，别处不许再写）
//!
//! ```text
//! 写图号        new = if keep { (cur & 0x8000) | (v & 0x7FFF) } else { v & 0x7FFF }
//! 橡皮(图号层)   new = cur & 0x8000
//! 橡皮(阻挡层)   new = cur & !0x8000
//! 阻挡层画笔     new = cur | 0x8000
//! ```
//!
//! 理由见 [`layer`] 模块文档第二节（2026-11-04 裁定 + 三条文件级真值）。

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod brush;
mod cmd;
mod convert;
mod document;
mod error;
mod fill;
mod layer;
mod layer_counts;
mod paint;
mod save;
mod selection;
mod session;
mod stamp;
mod stamp_convert;
mod undo;

#[cfg(test)]
mod testutil;

pub use brush::{
    clamp_brush_size, footprint_of, lib_ref_of, palette_index_to_sprite, pick_cell_of, Brush,
    Family, LibRef, PickedCell, BRUSH_DEFAULT, BRUSH_MAX, BRUSH_MIN, BRUSH_STEP,
};
pub use cmd::{apply, set_brush_from_palette, set_tool_by_key, Command, Outcome};
pub use convert::{
    ground_paint_supported, restore_unwritten_lib_byte, sync_view_into_raw, unmodeled_fields,
    view_cell_format, view_from_raw, writes_layer_lib_byte, ConvertError, FieldMap, MapKind,
    FIELD_MAP, LOST_IN_VIEW,
};
pub use document::{
    byte_diff, byte_diff_bytes, cell_at, cell_mut, index_of, ByteDiff, Document,
};
pub use error::{CoreError, SaveStage};
pub use fill::{
    align_start, collect_region, fill_key, step_of, FillKey, FillOutcome, RegionOutcome,
    FILL_MAX_CELLS,
};
pub use layer::{
    EditLayer, COLLISION_BIT, LAYER_IMG_FIELDS, LAYER_LIB_FIELDS, NAME_FRONT, NAME_GROUND,
    NAME_HIGH_COLLISION, NAME_LOW_COLLISION, NAME_MIDDLE, SPRITE_MAX,
};
pub use layer_counts::{
    cell_mask, cell_mask_ext, LayerCounts, MaskContext, MASK_FRONT, MASK_GROUND,
    MASK_HIGH_COLLISION, MASK_LOW_COLLISION, MASK_MIDDLE,
};
pub use paint::{
    paint, PaintOutcome, Tool, TOOL_BRUSH, TOOL_BRUSH_IDX, TOOL_BUCKET, TOOL_BUCKET_IDX,
    TOOL_ERASER, TOOL_ERASER_IDX, TOOL_PICKUP, TOOL_PICKUP_IDX, TOOL_UNWIRED,
};
pub use save::{
    fsync_parent, save_atomic, save_session, temp_path_for, DirSync, SaveOutcome, SavedFile,
    TEMP_SUFFIX,
};
pub use selection::{Clipboard, Selection};
pub use session::{line_cells, EditSession, FillRecord};
pub use stamp::{PlaceOutcome, StampCellInput, STAMP_MAX_CELLS, STAMP_REGISTRY_MAX};
pub use stamp_convert::{cells_from_stamp, StampConvertError, StampLibLayer};
pub use undo::{Change, Step, UndoStack, UNDO_LIMIT};
