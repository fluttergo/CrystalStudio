//! 单格落笔：[`paint`] 按「工具 × 当前层」算出这一格改完应该长什么样。
//!
//! 抽取自 `miredit-gui/src/edit.rs:1360-1488`（`PaintOutcome` / `paint`）与
//! `edit.rs:394-455`（`Tool`）。**N×N 展开不在这里**：那是会话的事
//! （[`crate::EditSession::footprint`]），本模块只吃**一格**、只吐**一格**。
//!
//! # 一、工具 × 层的动作表（`edit.rs:21-26`）
//!
//! | 工具 | 动哪些字段 |
//! |---|---|
//! | 画笔 [`Tool::Brush`] | 图号层：只写当前层图号（+ 14/36 档该层库号字节）；**族不匹配 ⇒ 拒绝且一字节不动**<br>阻挡层：**只置 `bit15`**（= 不可走），低 15 位原样；**图库笔刷 ⇒ 拒绝** |
//! | 橡皮 [`Tool::Eraser`] | 图号层：**只清当前层图号**、保留阻挡位（`cur & 0x8000`）<br>阻挡层：**只清 `bit15`**（= 可走，`cur & !0x8000`） |
//! | 吸管 / 填充 / 未接线 | 不改地图（[`PaintOutcome::NoOp`]） |
//!
//! # 二、三条必须逐字照搬的位运算（任务书 §四.1 / §四.2 / §四.3）
//!
//! ```text
//! 写图号   new = if keep { (cur & 0x8000) | (v & 0x7FFF) } else { v & 0x7FFF }   // edit.rs:1462-1468
//! 橡皮(图号层)  new = cur & 0x8000        // 保留阻挡位（否则橡皮变成"顺手把墙拆了"）  // edit.rs:1482
//! 橡皮(阻挡层)  new = cur & !0x8000       // 保留图号                                   // edit.rs:1479
//! 阻挡层画笔    new = cur | 0x8000        // 只动 bit15                                 // edit.rs:1415
//! ```
//!
//! `keep` 就是 [`EditLayer::img_of_has_collision_bit`]（`bit15` 保留矩阵，见 [`crate::layer`]）。
//!
//! # 三、为什么要"先算后写"
//!
//! [`paint`] 是**纯函数**：吃一个 `Cell` 的副本、吐一个 `Cell` 的副本，被拒绝时返回
//! [`PaintOutcome::LayerMismatch`] / [`PaintOutcome::PaletteBrushOnCollision`] 而不是 panic 或
//! 静默返回原值。调用方（会话）据 `Write` 与 `before` 比较才决定"要不要记一步撤销"——
//! 于是"拒绝"和"值没变"是两件可区分的事（`edit.rs:903-905`：值没变不记步，
//! 否则"空刷一遍也算一步"）。

use mir2_map::{Cell, CellFormat};

use crate::brush::{Brush, Family};
use crate::layer::{COLLISION_BIT, EditLayer};

// ── 工具名（中文，与 `miredit-gui/src/ui_text.rs:69-90` 逐字相同） ──
//
// 为什么不从 GUI 引：本 crate 不许依赖 `miredit-gui`（那是被抽取方）。
// 文案只此一份副本，改文案时两处必须同步（GUI 迁到本 crate 后这里就是唯一真源）。
// 与 `layer.rs` 的 `NAME_*` 同一套做法。

/// 画笔的工具名。来源：`miredit-gui/src/ui_text.rs:69`（官方 `Brush_8624CA0F`，旧称「画笔」）。
pub const TOOL_BRUSH: &str = "笔刷";
/// 填充的工具名。来源：`miredit-gui/src/ui_text.rs:88`（官方 `Fill_033955D4`）。
pub const TOOL_BUCKET: &str = "填充";
/// 橡皮的工具名。来源：`miredit-gui/src/ui_text.rs:73`（官方 `Eraser_686EEE3C`，旧称「橡皮」）。
pub const TOOL_ERASER: &str = "橡皮擦";
/// 吸管的工具名。来源：`miredit-gui/src/ui_text.rs:81`（官方 `Pickup_B685076A`）。
pub const TOOL_PICKUP: &str = "吸管";
/// 未接线工具的工具名。来源：`miredit-gui/src/ui_text.rs:90`。
pub const TOOL_UNWIRED: &str = "未接线";

// ── 工具在图标表里的下标（**唯一真源**，GUI 侧 `pub use` 回去） ──
//
// 来源：`miredit-gui/src/edit.rs:373-382`；真值是 `miredit-gui/src/icons.rs` 的 `TOOLS`
// 数组下标（JSON `tag_value`：`Brush` / `Bucket` / `Eraser` / `Pickup`）。
// 放进本 crate 是因为 [`Tool::from_index`] / [`Tool::index`] 必须用同一组数字 ——
// 两边各写一份的话，"图标点下去选中了别的工具"这类错位只会靠测试才发现。

/// 画笔在工具图标表里的下标（JSON `tag_value = "Brush"`）。
pub const TOOL_BRUSH_IDX: usize = 0;
/// 填充在工具图标表里的下标（JSON `tag_value = "Bucket"`，官方快捷键 `F`）。
///
/// 真值：`CrystalStudio/MainWindow.axaml:11` 的第三个工具按钮 `Tag="Bucket"`；
/// 快捷键 `MainWindow.axaml.cs:861` `Key.F => EditorTool.Bucket`。
pub const TOOL_BUCKET_IDX: usize = 2;
/// 橡皮在工具图标表里的下标（MDL2 码位 U+E75C 的等效自绘）。
pub const TOOL_ERASER_IDX: usize = 3;
/// 吸管在工具图标表里的下标（MDL2 码位 U+E7C2 的等效自绘）。
pub const TOOL_PICKUP_IDX: usize = 6;

/// 本 crate 接线的四个工具（其余下标一律 [`Tool::Unwired`]，如实报"未接线"）。
/// 来源：`edit.rs:394-407`。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tool {
    /// 画笔：把当前笔刷写到经过的格（**当前编辑层**），落笔范围 = 以鼠标格为中心的 N×N。
    #[default]
    Brush,
    /// 填充：从点击格出发 BFS 到「同层同图号」的连通块，用当前笔刷一次填满
    /// （**一次填充 = 一步撤销**）。
    Bucket,
    /// 橡皮：把经过格**当前编辑层**的图号清零（**保留最高位**），落笔范围同样是 N×N。
    Eraser,
    /// 吸管：把某格**当前编辑层**的内容取成笔刷（整格三族都会读出来用于诊断）。
    Pickup,
    /// 未接线的工具（图标仍然可选，但落到画布上什么都不做）。
    Unwired,
}

impl Tool {
    /// 是否在本单接线范围内（决定画布上点下去有没有反应）。来源：`edit.rs:444-446`。
    pub const fn is_wired(self) -> bool {
        !matches!(self, Tool::Unwired)
    }

    /// 工具的 **ASCII 键**（`brush` / `bucket` / `eraser` / `pickup` / `unwired`）。
    ///
    /// 与层名的 [`crate::EditLayer::key`] 同一个理由（`edit.rs:217-222`）：给脚本 grep 的行里
    /// 放中文会让"按空格/等号切字段"的脚本在不同代码页下出错。
    /// 网页版的命令/回执里用这一套键。
    pub const fn key(self) -> &'static str {
        match self {
            Tool::Brush => "brush",
            Tool::Bucket => "bucket",
            Tool::Eraser => "eraser",
            Tool::Pickup => "pickup",
            Tool::Unwired => "unwired",
        }
    }

    /// [`Self::key`] 的**逆映射**；不认识就 `None`（绝不猜成某个工具）。
    ///
    /// 这是「工具名（字符串）→ 工具」的**唯一真源**：`Command::SetTool` 与
    /// [`crate::cmd::set_tool_by_key`] 都走它，GUI/网页层不许再各抄一份表。
    ///
    /// # 词汇表只有 **4 个**：`brush` / `bucket` / `eraser` / `pickup`
    ///
    /// 三条硬口径（2026-11-05 契约裁定，GUI 与网页层共用同一套）：
    ///
    /// 1. **`unwired` 拒收**：它是内部态（"本版未接线"的占位），外部把它设进来等于
    ///    "把工具关掉"，没有任何编辑语义 ⇒ 与"非法键"同等对待；
    /// 2. **不收别名**（`fill` / `pick` / `pencil` 之类）：别名由前端翻译，
    ///    线上词汇表只此一套 —— 两边各认一套别名就是"同一个名字在两个端上有两种结果"；
    /// 3. **严格小写精确匹配**：`Brush` / `brush ` 一律 `None`（不猜、不去空白）。
    ///
    /// # 为什么不是 `const fn`
    ///
    /// `&str` 的字面量匹配要用 `PartialEq for str`，它在 const 上下文里不可调用
    /// （[`EditLayer::from_key`](crate::EditLayer::from_key) 同样是普通 `fn`）。
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "brush" => Some(Tool::Brush),
            "bucket" => Some(Tool::Bucket),
            "eraser" => Some(Tool::Eraser),
            "pickup" => Some(Tool::Pickup),
            // `unwired` 与其余一律 None：见方法文档第 1/2 条
            _ => None,
        }
    }

    /// 由**工具图标下标**得到工具（未接线的下标一律 [`Tool::Unwired`]）。来源：`edit.rs:411-419`。
    ///
    /// 这是 GUI 的"点第 i 个图标"与网页版"选了第 i 个工具"的共同入口；
    /// 下标常量就是本模块的 [`TOOL_BRUSH_IDX`] 等四个（**唯一真源**）。
    pub const fn from_index(i: usize) -> Self {
        match i {
            TOOL_BRUSH_IDX => Tool::Brush,
            TOOL_BUCKET_IDX => Tool::Bucket,
            TOOL_ERASER_IDX => Tool::Eraser,
            TOOL_PICKUP_IDX => Tool::Pickup,
            _ => Tool::Unwired,
        }
    }

    /// 本工具对应的图标下标（[`Tool::Unwired`] 给 0，调用方须先判 `is_wired`）。
    /// 来源：`edit.rs:422-430`。
    ///
    /// 与 [`Self::from_index`] 必须往返一致（有单元测试钉住）。
    pub const fn index(self) -> usize {
        match self {
            Tool::Brush => TOOL_BRUSH_IDX,
            Tool::Bucket => TOOL_BUCKET_IDX,
            Tool::Eraser => TOOL_ERASER_IDX,
            Tool::Pickup => TOOL_PICKUP_IDX,
            Tool::Unwired => 0,
        }
    }

    /// 工具名（界面显示用；文案见本模块 `TOOL_*` 常量）。来源：`edit.rs:433-441`。
    ///
    /// 与 [`Self::key`] 的分工：`name` 给人看（中文），`key` 给脚本 grep（ASCII）。
    pub const fn name(self) -> &'static str {
        match self {
            Tool::Brush => TOOL_BRUSH,
            Tool::Bucket => TOOL_BUCKET,
            Tool::Eraser => TOOL_ERASER,
            Tool::Pickup => TOOL_PICKUP,
            Tool::Unwired => TOOL_UNWIRED,
        }
    }

    /// 本工具**吃不吃笔刷尺寸**（官方口径：铅笔与填充恒为单格，其余用 `BrushSize`）。
    /// 来源：`edit.rs:452-454`。
    ///
    /// 真值 `MapViewport.cs:473`：
    /// `int size = Tool is EditorTool.Pencil or EditorTool.Bucket ? 1 : BrushSize;`
    /// —— 注意这是**落笔**的尺寸；填充工具的点选永远是 1 格（它填的是连通块，不是方块）。
    pub const fn uses_brush_size(self) -> bool {
        matches!(self, Tool::Brush | Tool::Eraser)
    }
}

/// 一次落笔的判定结果。来源：`edit.rs:1360-1381`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaintOutcome {
    /// 应把该格改成这个新内容。
    Write(Cell),
    /// 这一笔不改（未接线工具 / 画笔还没取内容）。
    NoOp,
    /// **拒绝**：笔刷族与当前编辑层不一致 —— 绝不跨字段写，调用方须把原因说出去。
    LayerMismatch {
        /// 笔刷那一族。
        brush: Family,
        /// 当前编辑层。
        layer: EditLayer,
    },
    /// **拒绝**：当前层是阻挡层，而笔刷**来自图库**（意图是盖图号，阻挡层不写图号）。
    PaletteBrushOnCollision {
        /// 笔刷那一族。
        brush: Family,
        /// 当前编辑层（必定是低位/高位阻挡）。
        layer: EditLayer,
    },
}

/// 按工具算出"这一格改完应该长什么样"（**只动当前编辑层**，其余字段原样带过去）。
/// 来源：`edit.rs:1393-1488`（逐分支搬，未合并、未重排）。
///
/// - [`Tool::Brush`]：
///   - 图号层：族不一致 ⇒ [`PaintOutcome::LayerMismatch`]（一个字段都不写）；
///     一致 ⇒ 只写该层图号（14/36 档另写该层库号字节）；
///   - 阻挡层：**只置 `bit15`**（低 15 位图号原样）；图库笔刷 ⇒
///     [`PaintOutcome::PaletteBrushOnCollision`]（一位都不写）；
/// - [`Tool::Eraser`]：图号层只清**当前层**图号、保留最高位（`img &= 0x8000`）；
///   阻挡层只清 **`bit15`**、保留图号（`img &= 0x7FFF`）；
/// - 吸管/填充/未接线：不改地图（[`PaintOutcome::NoOp`]）。
///
/// # `Old12` 档
///
/// `fmt.has_lib_bytes() == false` ⇒ **只写图号**：`Cell::tiles`/`sm_tiles` 保持原值，
/// `to_bytes()` 长度不变（不凭空补第 13 个字节，`edit.rs:56-57`）。
pub fn paint(
    fmt: CellFormat,
    cell: Cell,
    tool: Tool,
    brush: Option<Brush>,
    layer: EditLayer,
) -> PaintOutcome {
    let mut out = cell;
    match tool {
        Tool::Brush => {
            if layer.is_collision() {
                // ① 阻挡层：图库笔刷一律拒绝（它想盖图号，而这一层只动一个位）。`edit.rs:1404-1412`
                if let Some(b) = brush {
                    if b.from.is_none() {
                        return PaintOutcome::PaletteBrushOnCollision {
                            brush: b.family,
                            layer,
                        };
                    }
                }
                // ② 置位：**只动 bit15**，低 15 位的图号原样带过去。`edit.rs:1413-1416`
                let cur = layer.img_of(&cell);
                layer.set_img(&mut out, cur | COLLISION_BIT);
                return PaintOutcome::Write(out);
            }
            // 还没取过笔刷 ⇒ 画笔无从落笔（不是错误，不改地图）。`edit.rs:1418-1421`
            let Some(b) = brush else {
                return PaintOutcome::NoOp;
            };
            // 图号层必定有族（`is_collision` 已在上面的分支里返回）。`edit.rs:1422-1425`
            let Some(fam) = layer.family() else {
                return PaintOutcome::NoOp;
            };
            if b.family != fam {
                // 族不匹配：拒绝，绝不把地表图号写进 mid_img 这类跨字段乱写。`edit.rs:1426-1432`
                return PaintOutcome::LayerMismatch {
                    brush: b.family,
                    layer,
                };
            }
            // ③ 写图号：**只替换低 15 位，最高位按字段语义裁定**。
            //
            //    真值：`CrystalStudio/CrystalStudio/MapViewport.cs:480`
            //    `c.BackImage = (c.BackImage & 0x60000000) | image`（画地表保留碰撞位）；
            //    `:520` 填充同款；`:546` 清除选区 `c.BackImage &= 0x60000000`（只留碰撞位）。
            //    C# 内存模型里 `0x60000000` = 低/高两位阻挡（`:23`），
            //    `LegacyMapReader.cs:147-159` 把它们从 `.map` 的
            //    `bk_img & 0x8000` / `frimage & 0x8000` 翻上来
            //    ⇒ 我们文件级等价位 = `bk_img` / `fr_img` 的 `bit15`。
            //
            //    笔刷自己也只取低 15 位：吸管从带阻挡的格取下来的 `b.img` 含 `bit15`，
            //    若原样 `|` 进去，就等于把源格的阻挡"贴"到目标格上（反向拆墙/筑墙）。
            //    （`edit.rs:1433-1461` 原注释逐条保留）
            let cur = layer.img_of(&cell);
            let low15 = b.img & !COLLISION_BIT;
            let new_img = if layer.img_of_has_collision_bit() {
                (cur & COLLISION_BIT) | low15
            } else {
                low15
            };
            layer.set_img(&mut out, new_img);
            if fmt.has_lib_bytes() && layer.writes_lib_byte() {
                // `Old12` 档没有库号字节 ⇒ 不写（`to_bytes` 不会凭空长出一个字节）。`edit.rs:1470-1473`
                layer.set_lib_byte(&mut out, b.lib_byte);
            }
            // ④ **中层动画（C# `MiddleFrames`/`MiddleTick`）不在这里清**，两个理由都有实证：
            //
            //    ① 本 crate 的 `Cell.ani_frame`/`ani_tick` 是**前景**动画，不是中层动画：
            //       磁盘上是 14 B 的 off 8/9（`mir2-map/src/cell.rs:352-353`），C# 读进
            //       `franimationframe`/`franimationtick`（`LegacyMapReader.cs:245-246`，
            //       只在有前景时赋给 `Foreground`，`:259-260`）⇒ 清它等于把前景动画抹掉，
            //       而上游 `MapViewport.cs:592`（另一副本 `:597`）的中层分支一个字节都不碰 `Foreground`。
            //    ② `Mir3Wemade`/`Mir3Shanda`/`CrystalCs` 里真正的中层动画是**另外两个字节**
            //       （`LegacyMapReader.cs:418`/`:486`/`:641` ⇒ `mir2-format::Cell.mid_ani_frame`/
            //       `mid_ani_tick`），**编辑视图不承载**（`convert.rs` 的 `LOST_IN_VIEW` 第 6/7 条）
            //       ⇒ 这里也无从清起。完整论证 + 语料统计见 `selection.rs` 模块文档 §二 第 4/5 条。
        }
        Tool::Eraser => {
            let cur = layer.img_of(&cell);
            if layer.is_collision() {
                // 阻挡层：清 bit15 = 恢复可走；**图号低 15 位原样保留**。`edit.rs:1477-1479`
                layer.set_img(&mut out, cur & !COLLISION_BIT);
            } else {
                // 图号层：只清图号，**保留最高位**（否则橡皮变成"顺手把墙拆了"）。`edit.rs:1480-1483`
                //
                // ⚠ 中层动画同样**不在这里清**：上游 `MapViewport.cs:592` 的 erase 与画图走同一个
                // `case MapLayer.Middle`（`MiddleImage = 0` 且 `MiddleFrames/Tick = 0`），但本 crate 的
                // `ani_frame`/`ani_tick` 是**前景**动画字节（`LegacyMapReader.cs:245-246`/`:259-260`），
                // 中层动画另在不承载的 `mid_ani_*` 里 —— 见 `Tool::Brush` 分支 ④ 与
                // `selection.rs` 模块文档 §二 第 4/5 条。
                layer.set_img(&mut out, cur & COLLISION_BIT);
            }
        }
        Tool::Pickup | Tool::Bucket | Tool::Unwired => return PaintOutcome::NoOp,
    }
    PaintOutcome::Write(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brush::{palette_index_to_sprite, LibRef};
    use crate::testutil::造图;
    use mir2_map::CellFormat;

    /// 工具下标 ↔ 工具必须**往返一致**，且四个下标互不相同（防错位）。
    ///
    /// 来源：`edit.rs:2539-2556`（GUI 侧还额外核对 `icons::TOOLS` 的下标与名字，
    /// 那条留在 gui —— `icons` 是 UI 资产，本 crate 不依赖）。
    #[test]
    fn 工具下标与名字往返一致() {
        // 四个接线工具：下标 → 工具 → 下标
        for (i, t) in [
            (TOOL_BRUSH_IDX, Tool::Brush),
            (TOOL_BUCKET_IDX, Tool::Bucket),
            (TOOL_ERASER_IDX, Tool::Eraser),
            (TOOL_PICKUP_IDX, Tool::Pickup),
        ] {
            assert!(t.is_wired(), "{t:?} 必须已接线");
            assert_eq!(Tool::from_index(i), t, "下标 {i} 应得到 {t:?}");
            assert_eq!(t.index(), i, "{t:?}.index() 必须回到 {i}");
        }
        // 四个下标互不相同（否则两个图标会选中同一个工具）
        let mut idx = [
            TOOL_BRUSH_IDX,
            TOOL_BUCKET_IDX,
            TOOL_ERASER_IDX,
            TOOL_PICKUP_IDX,
        ]
        .to_vec();
        idx.sort_unstable();
        let mut uniq = idx.clone();
        uniq.dedup();
        assert_eq!(uniq.len(), 4, "四个工具下标必须互不相同：{idx:?}");
        // 未接线的下标一律 Unwired，且 Unwired.index() 给 0（调用方须先判 is_wired）
        for i in [1usize, 4, 5, 7, 8, 9, 100] {
            assert_eq!(Tool::from_index(i), Tool::Unwired, "下标 {i} 本版未接线");
        }
        assert!(!Tool::Unwired.is_wired());
        assert_eq!(Tool::Unwired.index(), 0);
        // 工具名（中文，给界面看）与 key（ASCII，给脚本 grep）是两套
        assert_eq!(Tool::Brush.name(), "笔刷");
        assert_eq!(Tool::Bucket.name(), "填充");
        assert_eq!(Tool::Eraser.name(), "橡皮擦");
        assert_eq!(Tool::Pickup.name(), "吸管");
        assert_eq!(Tool::Unwired.name(), "未接线");
        for t in [
            Tool::Brush,
            Tool::Bucket,
            Tool::Eraser,
            Tool::Pickup,
            Tool::Unwired,
        ] {
            assert_ne!(t.name(), t.key(), "{t:?} 的中文名与 ASCII 键不该相同");
        }
    }

    /// 工具名（`key`）与 [`Tool::from_key`] 必须**往返一致**，且不认识的一律 `None`。
    ///
    /// 这是 `Command::SetTool` 的判据：认错一个名字就是"点了橡皮却拿到画笔"。
    ///
    /// ⚠ **往返只对 4 个已接线工具成立**：`unwired` 是内部态，外部词汇表不收它
    /// （2026-11-05 契约裁定，见 [`Tool::from_key`] 的方法文档）。
    #[test]
    fn 工具键与工具往返一致() {
        for t in [Tool::Brush, Tool::Bucket, Tool::Eraser, Tool::Pickup] {
            assert_eq!(Tool::from_key(t.key()), Some(t), "{t:?} 往返失败");
            assert!(t.is_wired(), "{t:?} 必须已接线");
        }
        // 内部态 `unwired` 与别名、大小写、空白一律拒收
        for bad in [
            "",
            "unwired",
            "Brush",
            "BRUSH",
            "brush ",
            " fill",
            "fill",
            "pick",
            "pencil",
            "0",
        ] {
            assert_eq!(Tool::from_key(bad), None, "{bad:?} 不该被认成某个工具");
        }
        // 四个 key 互不相同（否则两个名字会选中同一个工具）
        let mut keys: Vec<&str> = [Tool::Brush, Tool::Bucket, Tool::Eraser, Tool::Pickup]
            .iter()
            .map(|t| t.key())
            .collect();
        keys.sort_unstable();
        let mut uniq = keys.clone();
        uniq.dedup();
        assert_eq!(uniq.len(), 4, "工具键必须互不相同：{keys:?}");
        assert_eq!(Tool::Unwired.key(), "unwired", "内部态仍有 key（给日志用）");
    }

    /// 造一支"从图库选的"笔刷（`from == None`）与一支"从地图吸的"（`from == Some`）。
    fn 图库笔刷(fam: Family, 帧: u32) -> Brush {
        Brush {
            family: fam,
            img: palette_index_to_sprite(帧).unwrap(),
            lib_byte: 3,
            lib: Some(LibRef::ground(3)),
            from: None,
        }
    }
    fn 吸来笔刷(fam: Family, img: u16) -> Brush {
        Brush {
            family: fam,
            img,
            lib_byte: 3,
            lib: None,
            from: Some((0, 0)),
        }
    }

    /// **`bit15` 保留矩阵**（任务书 §四.1 的公式）：三族各一条。
    ///
    /// 输入都是"原格带 `bit15`、笔刷是纯图号 10"：
    /// - 地表 `bk_img=0x815F` ⇒ `0x800A`
    /// - 物体 `fr_img=0x814D` ⇒ `0x800A`
    /// - 中层 `mid_img=0x80DE` ⇒ `0x000A`（不保留）
    #[test]
    fn 写图号bit15保留矩阵() {
        let 帧9 = 9; // 帧索引 9 ⇒ 图号 10
        for (layer, 原值, 期望) in [
            (EditLayer::Ground, 0x815F_u16, 0x800A_u16),
            (EditLayer::Front, 0x814D, 0x800A),
            (EditLayer::Middle, 0x80DE, 0x000A),
        ] {
            let mut c = Cell::default();
            layer.set_img(&mut c, 原值);
            let b = 图库笔刷(layer.pick_family(), 帧9);
            match paint(CellFormat::New14, c, Tool::Brush, Some(b), layer) {
                PaintOutcome::Write(after) => {
                    assert_eq!(
                        layer.img_of(&after),
                        期望,
                        "{layer:?} 写图号 10：0x{原值:04X} ⇒ 应 0x{期望:04X}"
                    );
                }
                other => panic!("{layer:?} 应 Write，实际 {other:?}"),
            }
        }
    }

    /// 族不匹配 ⇒ 拒绝，且**一个字段都不写**。来源：`edit.rs:1426-1432`。
    #[test]
    fn 族不匹配拒绝() {
        let c = Cell {
            bk_img: 0x1111,
            mid_img: 0x2222,
            fr_img: 0x3333,
            ..Cell::default()
        };
        // 地表笔刷配物体层
        let b = 图库笔刷(Family::Ground, 9);
        assert_eq!(
            paint(CellFormat::New14, c, Tool::Brush, Some(b), EditLayer::Front),
            PaintOutcome::LayerMismatch {
                brush: Family::Ground,
                layer: EditLayer::Front
            }
        );
        // 三层两两交叉都不许过
        for layer in EditLayer::IMAGE {
            for fam in Family::ALL {
                if Some(fam) == layer.family() {
                    continue;
                }
                let b = 图库笔刷(fam, 9);
                assert!(
                    matches!(
                        paint(CellFormat::New14, c, Tool::Brush, Some(b), layer),
                        PaintOutcome::LayerMismatch { .. }
                    ),
                    "{fam:?} 笔刷配 {layer:?} 必须被拒"
                );
            }
        }
    }

    /// 阻挡层：图库笔刷拒绝、地图吸来的笔刷只置位。来源：`edit.rs:1404-1416`。
    #[test]
    fn 阻挡层画笔只置位() {
        let c = Cell {
            bk_img: 0x005F,
            ..Cell::default()
        };
        // 图库笔刷 ⇒ 拒绝
        assert_eq!(
            paint(
                CellFormat::New14,
                c,
                Tool::Brush,
                Some(图库笔刷(Family::Ground, 9)),
                EditLayer::LowCollision
            ),
            PaintOutcome::PaletteBrushOnCollision {
                brush: Family::Ground,
                layer: EditLayer::LowCollision
            }
        );
        // 地图吸来的笔刷 ⇒ 只置 bit15，低 15 位原样
        match paint(
            CellFormat::New14,
            c,
            Tool::Brush,
            Some(吸来笔刷(Family::Surface, 0x7FFF)),
            EditLayer::LowCollision,
        ) {
            PaintOutcome::Write(after) => {
                assert_eq!(after.bk_img, 0x805F, "只置 bit15，图号低 15 位原样");
                assert_eq!(after.mid_img, c.mid_img);
                assert_eq!(after.fr_img, c.fr_img);
            }
            other => panic!("应 Write，实际 {other:?}"),
        }
        // 手上没有笔刷也允许（阻挡层不需要笔刷内容）
        match paint(CellFormat::New14, c, Tool::Brush, None, EditLayer::HighCollision) {
            PaintOutcome::Write(after) => assert_eq!(after.fr_img, COLLISION_BIT),
            other => panic!("应 Write，实际 {other:?}"),
        }
    }

    /// 橡皮：图号层 `cur & 0x8000`；阻挡层 `cur & !0x8000`。来源：`edit.rs:1475-1483`。
    ///
    /// ⚠ **图号层的三个层走同一条公式**（`cur & 0x8000`），中层也不例外 ——
    /// 橡皮分支**不看** `bit15` 保留矩阵（`edit.rs:1480-1483` 里就是无条件 `cur & COLLISION_BIT`）。
    /// 矩阵只约束"写图号"那一条路径（[`super::paint`] 的 `Tool::Brush` 分支）。
    /// 照搬，不"顺手统一"。
    #[test]
    fn 橡皮两个分支() {
        let c = Cell {
            bk_img: 0x815F,
            mid_img: 0x8000 | 0x0DE,
            fr_img: 0x814D,
            ..Cell::default()
        };
        // 图号层：保留 bit15、清掉图号
        match paint(CellFormat::New14, c, Tool::Eraser, None, EditLayer::Ground) {
            PaintOutcome::Write(after) => assert_eq!(after.bk_img, 0x8000, "只留阻挡位"),
            other => panic!("{other:?}"),
        }
        // 中层：同样走 `cur & 0x8000`（保留最高位，清掉低 15 位）
        match paint(CellFormat::New14, c, Tool::Eraser, None, EditLayer::Middle) {
            PaintOutcome::Write(after) => {
                assert_eq!(after.mid_img, 0x8000, "橡皮分支不看保留矩阵，三层统一 cur & 0x8000")
            }
            other => panic!("{other:?}"),
        }
        // 阻挡层：清 bit15、保留图号
        match paint(
            CellFormat::New14,
            c,
            Tool::Eraser,
            None,
            EditLayer::LowCollision,
        ) {
            PaintOutcome::Write(after) => assert_eq!(after.bk_img, 0x015F, "图号低 15 位必须留着"),
            other => panic!("{other:?}"),
        }
    }

    /// 吸管 / 填充 / 未接线一律 `NoOp`（它们不改地图）。来源：`edit.rs:1485`。
    #[test]
    fn 其它工具不改地图() {
        let c = Cell {
            bk_img: 0x815F,
            ..Cell::default()
        };
        for t in [Tool::Pickup, Tool::Bucket, Tool::Unwired] {
            assert_eq!(
                paint(CellFormat::New14, c, t, Some(图库笔刷(Family::Ground, 9)), EditLayer::Ground),
                PaintOutcome::NoOp,
                "{t:?} 不许改地图"
            );
        }
    }

    /// `Old12` 档不写库号字节：`tiles`/`sm_tiles` 原值不动。来源：`edit.rs:56-57`、`:1470-1473`。
    #[test]
    fn 十二字节档不动库号() {
        let c = Cell {
            bk_img: 0x8000,
            tiles: 7,
            sm_tiles: 8,
            area: 9,
            ..Cell::default()
        };
        let mut b = 图库笔刷(Family::Ground, 9);
        b.lib_byte = 0xFF;
        match paint(CellFormat::Old12, c, Tool::Brush, Some(b), EditLayer::Ground) {
            PaintOutcome::Write(after) => {
                assert_eq!(after.bk_img, 0x800A);
                assert_eq!(after.tiles, 7, "Old12 档不许写 btTiles");
            }
            other => panic!("{other:?}"),
        }
        // 14/36 档会写
        for fmt in [CellFormat::New14, CellFormat::Ext36] {
            match paint(fmt, c, Tool::Brush, Some(b), EditLayer::Ground) {
                PaintOutcome::Write(after) => {
                    assert_eq!(after.tiles, 0xFF, "{fmt:?} 应写 btTiles");
                    assert_eq!(after.sm_tiles, 8, "别的层的库号不许动");
                    assert_eq!(after.area, 9, "别的层的库号不许动");
                }
                other => panic!("{other:?}"),
            }
        }
    }

    /// 笔刷自带 `bit15` 不许贴到目标格上（反向拆墙/筑墙）。来源：`edit.rs:1458-1461`。
    #[test]
    fn 笔刷自带bit15不传播() {
        let 目标 = Cell {
            bk_img: 0x0000,
            ..Cell::default()
        };
        let b = 吸来笔刷(Family::Ground, 0x8001);
        assert!(b.blocked());
        match paint(CellFormat::New14, 目标, Tool::Brush, Some(b), EditLayer::Ground) {
            PaintOutcome::Write(after) => {
                assert_eq!(after.bk_img, 0x0001, "目标格本来可走，改完还得可走");
                assert!(after.is_walkable());
            }
            other => panic!("{other:?}"),
        }
    }

    /// 值没变时 `paint` 仍然给 `Write`，由调用方判"等于 before ⇒ 不记步"。
    /// 这样"拒绝"与"值没变"是两件可区分的事。来源：`edit.rs:903-905`。
    #[test]
    fn 值没变也给write() {
        let c = Cell {
            bk_img: 10,
            tiles: 3, // 与笔刷的 lib_byte 相同 ⇒ 落笔后整格与原来相等
            ..Cell::default()
        };
        let b = 图库笔刷(Family::Ground, 9); // 图号 10、库号 3
        assert_eq!(
            paint(CellFormat::New14, c, Tool::Brush, Some(b), EditLayer::Ground),
            PaintOutcome::Write(c),
            "同值也返回 Write，由会话比较 before/after 决定不记步"
        );
        // 而未接线工具是 NoOp（连比较都不做）
        assert_eq!(
            paint(CellFormat::New14, c, Tool::Unwired, Some(b), EditLayer::Ground),
            PaintOutcome::NoOp
        );
    }

    /// 合成图冒烟：地表 3×3 里只改一格，其余字段原样（口径同 `edit.rs:2210-2250`）。
    #[test]
    fn 只改当前层字段() {
        let map = 造图(3, 3, CellFormat::New14);
        let before = map.cells[4]; // (1,1)
        let b = 图库笔刷(Family::Ground, 9);
        let after = match paint(CellFormat::New14, before, Tool::Brush, Some(b), EditLayer::Ground) {
            PaintOutcome::Write(a) => a,
            other => panic!("{other:?}"),
        };
        assert_eq!(after.bk_img, 10);
        assert_eq!(after.mid_img, before.mid_img);
        assert_eq!(after.fr_img, before.fr_img);
        assert_eq!(after.door_index, before.door_index);
        assert_eq!(after.light, before.light);
        assert_eq!(after.tiles, b.lib_byte, "14 档应一并写 btTiles");
        assert_eq!(after.sm_tiles, before.sm_tiles);
        assert_eq!(after.area, before.area);
        assert_eq!(after.tail, before.tail);
    }
}
