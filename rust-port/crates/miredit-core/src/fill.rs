//! 填充（Bucket）：匹配键、四邻 BFS、地表 `step=2`、20 万格上限、**先收块后动笔**。
//!
//! 抽取自 `miredit-gui/src/edit.rs:1004-1213`（`EditSession::fill` 的算法部分）与
//! `edit.rs:1490-1610`（`FillOutcome` / `fill_key` / `neighbours`）。
//!
//! # 一、判据全部照官方 `CrystalStudio/MapViewport.cs:485-539` 的 `BucketFill`
//!
//! | 环节 | 官方源码 | 本 crate |
//! |---|---|---|
//! | 支持哪些层 | `:488` `if (!collision && Layer is not (Ground or Middle))` ⇒ 报 `"Fill works on ground tiles, middle tiles (smtiles), and low/high collisions"` 并 `return 0` | 物体层 ⇒ [`FillOutcome::UnsupportedLayer`]（一格不写） |
//! | 越界 | `:493` `!Document.Contains(x, y)` ⇒ `return 0` | [`FillOutcome::OutOfMap`] |
//! | **匹配什么** | `:498-499` `FillValue(c) = collision ? (0, c.BackImage & ActiveCollisionMask)` / `Layer == Ground ? (c.BackLibrary, c.BackImage & 0x1FFFFFFF)` / `(c.MiddleLibrary, c.MiddleImage)` | 见 [`fill_key`]：地表 = `(btTiles, bk_img & 0x7FFF)`、中层 = `(btsmTiles, mid_img)`、阻挡 = 「该位是否已置」 |
//! | 已是目标值 | `:501` `if (source == (…Library, image)) return 0;` | [`FillOutcome::AlreadyFilled`]（不产生步骤） |
//! | 扩散 | `:502-508` `Queue` + `HashSet<int> visited`（键 = `cy * Width + cx`），四邻 | 同样的 BFS + `visited`（用 `Vec<bool>`，键 = `cx * h + cy`，与 `Map::cells` 同序） |
//! | **步长** | `:496-497` `int step = Layer == MapLayer.Ground ? 2 : 1; if (step == 2) { x &= ~1; y &= ~1; }` | **地表 step=2**（对齐到偶偶格、邻居 ±2）；中层/阻挡 step=1 |
//! | 四邻 | `:531-532` `±step` 的**上下左右四格**（不含斜角） | 同样四邻 |
//! | 撤销 | `:510` `History.Begin()` … `:534` `History.Commit()` —— **整次填充只一步** | 会话把整批变更一次压成一步（[`crate::UndoStack::commit`]） |
//!
//! # 二、地表为什么要 `step=2`（两处独立证据）
//!
//! - 客户端/渲染都**只铺偶偶格**：`miredit-render/src/compose.rs:107-116`（`if x & 1 != 0 { continue }`）、
//!   出处 `map_view.gd:199`、`GameScene.cpp:3549-3553`；
//! - 地表一帧是 96×64 = **覆盖 2×2 格**（`miredit-render` 的 `CELL_W×CELL_H` 注释、
//!   `miredit-gui/src/cellinfo.rs` 的「地表 96×64（覆盖 2×2 格）」）。
//!
//! ⇒ 奇偶格是**填充位**，写它既看不见、又会把"同图号"判据搅乱（真实地图里这些格大量是 0），
//! 所以填充按 C# 对齐到偶偶格再走 ±2。
//!
//! ⚠ 与**笔刷**的一处有意差异（`edit.rs:1056-1058` 原话保留）：笔刷对地表仍是**逐格**落笔
//! （既有语义，GUI 侧 177 条基线测试钉着）；填充按 C# 对齐。这个差异只影响"填充位要不要写"，
//! 不影响任何会被画出来的格。
//!
//! # 三、与官方的两处**有意差异**（都已写进注释，不是悄悄改）
//!
//! 1. **上限保护**：官方没有上限，本 crate 单次最多 [`FILL_MAX_CELLS`] 格，超过 ⇒
//!    [`FillOutcome::TooLarge`] 且**一格不写**（先把连通块收齐再动笔，见下）。
//! 2. **计的是"真改了几格"**：官方 `:530` 的 `count++` 数的是**出队格数**；
//!    本 crate 数的是 `Step.changes.len()`（值真的变了的格）。两者只在"块内已有目标值"时不同
//!    （例如地表块里 `bit15` 不一致而笔刷不带 `bit15`），此时前者会让"填充 N 格"
//!    与 `Ctrl+Z` 能还原的格数对不上 —— 取后者（`edit.rs:1026-1029`）。
//!
//! # 四、先收块后动笔
//!
//! [`collect_region`] 只吃 `&Map`（**只读**），返回整个连通块或一个明确的拒绝原因；
//! 会话拿到块之后才算 `after`、才写。因此"超限一格不写"是**类型层面**保证的，
//! 不靠调用方记得先检查。
//!
//! # 五、阻挡层只**置位**，不清位
//!
//! 官方 `:516` 是 `c.BackImage |= ActiveCollisionMask`（只有 `|=`，没有 `&=`），
//! 且 `:501` 的早退保证"点已置位的块"直接返回 0。所以「填充阻挡层」的语义是
//! **把连通的可走区域整块设为不可走**；要清位请用橡皮（阻挡层是 `& !0x8000`）。

use std::collections::VecDeque;

use mir2_map::{Cell, Map};

use crate::brush::Family;
use crate::layer::{COLLISION_BIT, EditLayer, SPRITE_MAX};

/// 单次填充的**格数上限 200,000**（防呆：不让一次误点把整张图吞进撤销栈）。
///
/// 来源：`miredit-gui/src/metrics.rs:203-211`（任务书 §三.2 指定的硬上限；官方 C# 无此保护
/// —— `MapViewport.cs:485-539` 的 `BucketFill` 直接 BFS 到底，`visited` 只防重复不防规模）。
/// 超过即**报错且一格不写**。
///
/// 量级参照：700×700 = 490,000 格 ⇒ 一张"全图同图号"的极端图必被拦住；
/// 一格 `Change` 约 76 B，200,000 格 ≈ 15 MB 撤销步，仍在可接受范围。
pub const FILL_MAX_CELLS: usize = 200_000;

/// 填充的**匹配键**：两个格子"算不算同一片区域"就比它。来源：`edit.rs:1573-1584`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillKey {
    /// 地表：`(btTiles, bk_img & 0x7FFF)`。
    Ground(u8, u16),
    /// 中层：`(btsmTiles, mid_img)`。
    Middle(u8, u16),
    /// 阻挡层：该位是否已置。
    Collision(bool),
    /// 不支持填充的层（物体层）。
    Unsupported,
}

/// 填充的匹配键计算。来源：`edit.rs:1559-1571`。
///
/// # 官方原文（`CrystalStudio/MapViewport.cs:498-499`）
///
/// ```csharp
/// (int Library, int Image) FillValue(Cell c) => collision ? (0, c.BackImage & ActiveCollisionMask) : Layer == MapLayer.Ground
///     ? (c.BackLibrary, c.BackImage & 0x1FFFFFFF) : (c.MiddleLibrary, c.MiddleImage);
/// ```
///
/// 逐条落到本 crate 的 16 位字段上：
///
/// 1. **地表**：`(btTiles, bk_img & 0x7FFF)`。官方的 `& 0x1FFFFFFF` 把 `0x60000000`
///    （它内存模型里的**低位/高位阻挡位**，`MapViewport.cs:23`）**掩掉了** ⇒
///    **碰撞位不参与匹配**。本 crate 的碰撞位是 `bk_img` 的 `bit15`（[`COLLISION_BIT`]）
///    ⇒ 掩 `0x7FFF`，语义一一对应。
/// 2. **中层**：`(btsmTiles, mid_img)` —— 官方 `(MiddleLibrary, MiddleImage)` **不做掩码**，
///    本 crate 照样用整 16 位（中层没有碰撞位）。
/// 3. **阻挡层**：官方 `(0, c.BackImage & ActiveCollisionMask)` ⇒ 键就是**该位是否已置**。
///    本 crate 同理：键 = 「这一层的 `bit15` 是不是 1」。
///
/// ⚠ 库号字节：`Old12` 档没有库号字节（`Cell::tiles` 恒 0），键自然退化成"只看图号"——
/// 与官方 `Old12` 图（`BackLibrary` 恒 0）的行为一致。
pub fn fill_key(layer: EditLayer, c: &Cell) -> FillKey {
    if layer.is_collision() {
        return FillKey::Collision(layer.img_of(c) & COLLISION_BIT != 0);
    }
    match layer {
        EditLayer::Ground => FillKey::Ground(c.tiles, c.bk_img & SPRITE_MAX),
        EditLayer::Middle => FillKey::Middle(c.sm_tiles, c.mid_img),
        // 物体层不支持填充（调用方已拦），这里给一个不会与任何格相等的键
        EditLayer::Front => FillKey::Unsupported,
        // 阻挡层在上面已返回
        EditLayer::LowCollision | EditLayer::HighCollision => FillKey::Unsupported,
    }
}

/// 该层的**网格步长**：地表 2、其余 1。来源：`edit.rs:1059`（官方 `MapViewport.cs:496-497`）。
pub const fn step_of(layer: EditLayer) -> u16 {
    if matches!(layer, EditLayer::Ground) {
        2
    } else {
        1
    }
}

/// 把点击格对齐到该层的网格（地表 `step=2` ⇒ 对齐到偶偶格；其余原样）。
/// 来源：`edit.rs:1060-1064`（官方 `if (step == 2) { x &= ~1; y &= ~1; }`）。
pub const fn align_start(layer: EditLayer, x: u16, y: u16) -> (u16, u16) {
    if step_of(layer) == 2 {
        (x & !1, y & !1)
    } else {
        (x, y)
    }
}

/// `(x, y)` 的**上下左右四邻**，步长 `step`。来源：`edit.rs:1590-1610`。
///
/// 官方 `MapViewport.cs:531-532` 只入队这四格，**不含斜角**。
/// `step = 2` 时只走偶偶格（地表层）；`step = 1` 是逐格。
fn neighbours(x: u16, y: u16, w: u16, h: u16, step: u16) -> impl Iterator<Item = (u16, u16)> {
    let mut out: [(u16, u16); 4] = [(0, 0); 4];
    let mut n = 0;
    if x >= step {
        out[n] = (x - step, y);
        n += 1;
    }
    if x + step < w {
        out[n] = (x + step, y);
        n += 1;
    }
    if y >= step {
        out[n] = (x, y - step);
        n += 1;
    }
    if y + step < h {
        out[n] = (x, y + step);
        n += 1;
    }
    out.into_iter().take(n)
}

/// [`collect_region`] 的结果（**只读阶段**的全部可能）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegionOutcome {
    /// 收齐的连通块（格序 = BFS 出队序；**还没动笔**）。
    Region(Vec<(u16, u16)>),
    /// 超过 [`FILL_MAX_CELLS`]：`cells` 是**扫到上限那一刻**的格数（一般是上限 + 1）。
    TooLarge {
        /// 已扫到的格数（在上限处截断）。
        cells: usize,
    },
    /// 起点在地图外（官方 `MapViewport.cs:493`）。
    OutOfMap {
        /// 对齐后的 X。
        x: u16,
        /// 对齐后的 Y。
        y: u16,
    },
}

/// 从 `(x, y)` 出发收「**同层且该层图号相同**」的四邻连通块 —— **只读，不动笔**。
///
/// 起点会先按 [`align_start`] 对齐（幂等：已经对齐过的坐标再对齐不变），
/// 因此调用方传原始点击坐标即可。
///
/// 上限判据与官方一致的地方：`visited` 只防重复、四邻不含斜角、匹配键见 [`fill_key`]。
/// 与官方**有意不同**的地方：扫到 [`FILL_MAX_CELLS`] + 1 格就立刻停手（官方 BFS 到底），
/// 且此时**一格都不写** —— 由"只读签名"保证，不靠调用方自觉。
pub fn collect_region(map: &Map, layer: EditLayer, x: u16, y: u16) -> RegionOutcome {
    let (x, y) = align_start(layer, x, y);
    let Some(clicked) = map.cell(x, y) else {
        return RegionOutcome::OutOfMap { x, y };
    };
    let key = fill_key(layer, clicked);
    if key == FillKey::Unsupported {
        // 物体层不支持填充：给一个空块（调用方本来就会先拦，这里只是兜底不 panic）
        return RegionOutcome::Region(Vec::new());
    }
    let step = step_of(layer);
    let (w, h) = (map.width, map.height);
    let total = usize::from(w) * usize::from(h);
    let mut visited = vec![false; total];
    let mut region: Vec<(u16, u16)> = Vec::new();
    let mut queue: VecDeque<(u16, u16)> = VecDeque::new();
    let idx = |cx: u16, cy: u16| usize::from(cx) * usize::from(h) + usize::from(cy);
    visited[idx(x, y)] = true;
    queue.push_back((x, y));
    while let Some((cx, cy)) = queue.pop_front() {
        region.push((cx, cy));
        if region.len() > FILL_MAX_CELLS {
            return RegionOutcome::TooLarge {
                cells: region.len(),
            };
        }
        for (nx, ny) in neighbours(cx, cy, w, h, step) {
            if visited[idx(nx, ny)] {
                continue;
            }
            let Some(c) = map.cell(nx, ny) else {
                continue;
            };
            if fill_key(layer, c) != key {
                continue;
            }
            visited[idx(nx, ny)] = true;
            queue.push_back((nx, ny));
        }
    }
    RegionOutcome::Region(region)
}

/// 一次填充的结果（**拒绝时地图一个字节都没动**）。来源：`edit.rs:1492-1512`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillOutcome {
    /// 真的填了这么多格（整次填充 = 撤销栈里的**一步**）。
    Filled(usize),
    /// 目标格已经是要填的值（官方 `MapViewport.cs:501` 的早退），没有产生步骤。
    AlreadyFilled,
    /// 拒绝：本层不支持填充（官方 `:488-492`，只有地表/中层/低高位阻挡）。
    UnsupportedLayer,
    /// 拒绝：图号层没有笔刷（不知道填成哪一号图）。
    NoBrush,
    /// 拒绝：点击格在地图外（官方 `:493`）。
    OutOfMap,
    /// 拒绝：笔刷族与当前层不一致（沿用 [`crate::PaintOutcome::LayerMismatch`] 的语义；
    /// 阻挡层收到图库笔刷也归到这一类，见 `edit.rs:1102-1110`）。
    LayerMismatch,
    /// 拒绝：连通块超过 [`FILL_MAX_CELLS`]，**一格都没写**。
    TooLarge {
        /// 已经扫到的格数（在上限处截断）。
        cells: usize,
    },
    /// 拒绝：**本格式的地表逐格落笔无效**（九种里只有 `Mir3Wemade`/Type5）。
    ///
    /// 依据：Type5 的背景图号在**半分辨率块**里（2×2 一组 3 字节，
    /// `mir2-format/src/model.rs:430-440`），`Cell::bk_img` 恒 0 且 writer 不写它
    /// （`mir2-format/src/formats/mir3_wemade.rs:250-306`）⇒ 逐格填 `bk_img` 会被
    /// **静默丢弃**。宁可在动笔前明确拒绝，也不许让用户以为填上了。
    FormatCannotPaintGround,
}

impl FillOutcome {
    /// 是否被拒（决定状态栏用警示色还是正文色）。来源：`edit.rs:1516-1525`。
    pub const fn is_refusal(self) -> bool {
        matches!(
            self,
            FillOutcome::UnsupportedLayer
                | FillOutcome::NoBrush
                | FillOutcome::OutOfMap
                | FillOutcome::LayerMismatch
                | FillOutcome::TooLarge { .. }
                | FillOutcome::FormatCannotPaintGround
        )
    }

    /// 真的改了几格（拒绝 / 无事可做 = 0）。来源：`edit.rs:1528-1533`。
    pub const fn cells(self) -> usize {
        match self {
            FillOutcome::Filled(n) => n,
            _ => 0,
        }
    }

    /// 拒绝原因的一句话（成功 / 无事可做 ⇒ `None`）。
    ///
    /// 文案与 `miredit-gui/src/ui_text.rs:495-511` 的模板同义（本 crate 不依赖 GUI，
    /// 所以这里落成成品字符串；`{brush}` / `{layer}` 由 `brush` / `layer` 参数填）。
    ///
    /// 任务书 §七「拒绝必须返回明确 outcome，不许静默不改」：调用方拿到 `changed == 0` 时，
    /// 必须能从本函数拿到**一句能上屏的原因**。
    pub fn note(self, layer: EditLayer, brush: Option<Family>) -> Option<String> {
        match self {
            FillOutcome::Filled(_) | FillOutcome::AlreadyFilled => None,
            FillOutcome::UnsupportedLayer => Some(
                "填充只支持地表图层 / 中层图层 / 低位阻挡 / 高位阻挡（物体层没有可比较的图号）"
                    .to_string(),
            ),
            FillOutcome::NoBrush => {
                Some("没有笔刷：先用吸管取一格，或在资源面板选一帧".to_string())
            }
            FillOutcome::OutOfMap => Some(format!("点击格在地图外（{}）", layer.name())),
            FillOutcome::LayerMismatch => Some(match brush {
                Some(f) => format!("笔刷族（{}）与当前图层（{}）不一致", f.label(), layer.name()),
                None => format!("笔刷族与当前图层（{}）不一致", layer.name()),
            }),
            FillOutcome::TooLarge { cells } => Some(format!(
                "连通块至少 {cells} 格，超过单次上限 {FILL_MAX_CELLS} 格 —— 未执行，地图一个字节都没动"
            )),
            FillOutcome::FormatCannotPaintGround => Some(
                "Type5 背图在块级，逐格写 bk_img 无效 —— 未执行，地图一个字节都没动".to_string(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::造图;
    use mir2_map::CellFormat;

    /// 匹配键：地表掩 `bit15`、中层不掩、阻挡只看位。来源：`edit.rs:1536-1571`。
    #[test]
    fn 匹配键逐层() {
        let c = Cell {
            bk_img: 0x815F,
            mid_img: 0x80DE,
            tiles: 3,
            sm_tiles: 4,
            ..Cell::default()
        };
        assert_eq!(fill_key(EditLayer::Ground, &c), FillKey::Ground(3, 0x015F));
        assert_eq!(fill_key(EditLayer::Middle, &c), FillKey::Middle(4, 0x80DE));
        assert_eq!(fill_key(EditLayer::LowCollision, &c), FillKey::Collision(true));
        assert_eq!(
            fill_key(EditLayer::HighCollision, &c),
            FillKey::Collision(false)
        );
        assert_eq!(fill_key(EditLayer::Front, &c), FillKey::Unsupported);
        // bit15 不参与地表匹配：低 15 位相同即同键
        let d = Cell {
            bk_img: 0x015F,
            ..c
        };
        assert_eq!(fill_key(EditLayer::Ground, &c), fill_key(EditLayer::Ground, &d));
    }

    /// 步长与对齐：地表 2（对齐偶偶格），其余 1。来源：`edit.rs:1044-1064`。
    #[test]
    fn 步长与对齐() {
        assert_eq!(step_of(EditLayer::Ground), 2);
        assert_eq!(step_of(EditLayer::Middle), 1);
        assert_eq!(step_of(EditLayer::LowCollision), 1);
        assert_eq!(align_start(EditLayer::Ground, 358, 1), (358, 0));
        assert_eq!(align_start(EditLayer::Ground, 359, 1), (358, 0));
        assert_eq!(align_start(EditLayer::Middle, 359, 1), (359, 1));
        // 幂等
        for (x, y) in [(0u16, 0u16), (357, 3), (65535, 65535)] {
            let (a, b) = align_start(EditLayer::Ground, x, y);
            assert_eq!(align_start(EditLayer::Ground, a, b), (a, b));
        }
    }

    /// 四邻不含斜角、越界不入队、`step=2` 只走偶偶格。来源：`edit.rs:1586-1610`。
    #[test]
    fn 四邻不含斜角() {
        let v: Vec<(u16, u16)> = neighbours(5, 5, 10, 10, 1).collect();
        assert_eq!(v, vec![(4, 5), (6, 5), (5, 4), (5, 6)]);
        // 左上角只有右与下
        let v: Vec<(u16, u16)> = neighbours(0, 0, 10, 10, 1).collect();
        assert_eq!(v, vec![(1, 0), (0, 1)]);
        // step=2 的邻居仍是偶偶格
        let v: Vec<(u16, u16)> = neighbours(4, 4, 10, 10, 2).collect();
        assert_eq!(v, vec![(2, 4), (6, 4), (4, 2), (4, 6)]);
        // 右边界外不入队
        let v: Vec<(u16, u16)> = neighbours(9, 5, 10, 10, 1).collect();
        assert_eq!(v, vec![(8, 5), (9, 4), (9, 6)]);
    }

    /// **先收块**：只读、不含斜角、不越界；块大小对得上。
    #[test]
    fn 收块只读且四邻连通() {
        let mut map = 造图(10, 10, CellFormat::New14);
        for c in map.cells.iter_mut() {
            c.bk_img = 9;
            c.mid_img = 9;
            c.tiles = 3;
            c.sm_tiles = 4;
        }
        // 左上 4×4 = 图号 7（三个图号字段一起改，便于同时测地表与中层）
        for y in 0..4u16 {
            for x in 0..4u16 {
                let i = x as usize * 10 + y as usize;
                map.cells[i].bk_img = 7;
                map.cells[i].mid_img = 7;
            }
        }
        let orig = map.to_bytes();
        // 地表 step=2 ⇒ 只有 4 个偶偶格
        match collect_region(&map, EditLayer::Ground, 0, 0) {
            RegionOutcome::Region(r) => {
                let mut got = r.clone();
                got.sort_unstable();
                assert_eq!(got, vec![(0, 0), (0, 2), (2, 0), (2, 2)]);
            }
            other => panic!("{other:?}"),
        }
        // 中层 step=1 ⇒ 16 格全在块里
        match collect_region(&map, EditLayer::Middle, 0, 0) {
            RegionOutcome::Region(r) => assert_eq!(r.len(), 16, "中层逐格，块应是 4×4"),
            other => panic!("{other:?}"),
        }
        assert_eq!(map.to_bytes(), orig, "收块阶段一个字节都不许动");
    }

    /// **超上限一格不写**：上限 200,000 格；700×700 同号图必被拦。
    /// 来源：`edit.rs:3645`（GUI 侧同名用例）、`metrics.rs:203-211`。
    #[test]
    fn 超上限停在收块阶段() {
        assert_eq!(FILL_MAX_CELLS, 200_000);
        // 700×700 = 490,000 格必须超上限（量级参照见 FILL_MAX_CELLS 的文档）
        const { assert!(700 * 700 > FILL_MAX_CELLS) };
        // 500×500 = 250,000 格 > 200,000：整张图同号（中层键 = (btsmTiles, mid_img)）⇒ 必被拦
        let mut map = 造图(500, 500, CellFormat::Old12);
        for c in map.cells.iter_mut() {
            c.mid_img = 1;
            c.sm_tiles = 0;
        }
        let orig = map.to_bytes();
        match collect_region(&map, EditLayer::Middle, 0, 0) {
            RegionOutcome::TooLarge { cells } => {
                assert_eq!(cells, FILL_MAX_CELLS + 1, "在上限 +1 处停手");
            }
            other => panic!("应 TooLarge，实际 {other:?}"),
        }
        assert_eq!(map.to_bytes(), orig, "超限时一个字节都不许动");
        // 恰好等于上限（400×500 = 200,000）不拦
        let mut map2 = 造图(400, 500, CellFormat::Old12);
        for c in map2.cells.iter_mut() {
            c.mid_img = 1;
            c.sm_tiles = 0;
        }
        match collect_region(&map2, EditLayer::Middle, 0, 0) {
            RegionOutcome::Region(r) => assert_eq!(r.len(), FILL_MAX_CELLS, "恰好等于上限应放行"),
            other => panic!("{other:?}"),
        }
    }

    /// 越界起点 ⇒ `OutOfMap`（不 panic、不返回空块冒充成功）。
    #[test]
    fn 越界起点报错() {
        let map = 造图(4, 4, CellFormat::Old12);
        assert_eq!(
            collect_region(&map, EditLayer::Ground, 9, 9),
            RegionOutcome::OutOfMap { x: 8, y: 8 },
            "报的是对齐后的坐标"
        );
        assert!(matches!(
            collect_region(&map, EditLayer::Middle, 4, 0),
            RegionOutcome::OutOfMap { x: 4, y: 0 }
        ));
    }

    /// 拒绝原因必须是一句能上屏的话（禁静默）。来源：`ui_text.rs:495-511`。
    #[test]
    fn 拒绝原因可上屏() {
        assert_eq!(FillOutcome::Filled(3).note(EditLayer::Ground, None), None);
        assert_eq!(FillOutcome::AlreadyFilled.note(EditLayer::Ground, None), None);
        for out in [
            FillOutcome::UnsupportedLayer,
            FillOutcome::NoBrush,
            FillOutcome::OutOfMap,
            FillOutcome::LayerMismatch,
            FillOutcome::TooLarge { cells: 200_001 },
        ] {
            assert!(out.is_refusal());
            let note = out.note(EditLayer::Ground, None).expect("拒绝必须有原因");
            assert!(!note.is_empty(), "{out:?} 的原因不许为空");
            assert_eq!(out.cells(), 0, "{out:?} 一格都不算改了");
        }
        let note = FillOutcome::TooLarge { cells: 200_001 }
            .note(EditLayer::Middle, None)
            .unwrap();
        assert!(note.contains("200001") && note.contains("200000"), "{note}");
        let note = FillOutcome::LayerMismatch
            .note(EditLayer::Front, Some(Family::Ground))
            .unwrap();
        assert!(note.contains("地表图层") && note.contains("物体"), "{note}");
    }
}
