//! 低/高位阻挡标记的**绘制**（编辑器「碰撞层」可视化）。
//!
//! 这里只做两件事：**判定某格是否带阻挡位**、**把该格的标记色块画到画布上**。
//! 遍历（哪些格要画）在 `compose.rs`；口径说明见 crate 文档 §七「阻挡可视化口径」。
//!
//! # 一、判定口径（**以客户端为准**）
//!
//! | 通道 | 判据 | 出处 |
//! |---|---|---|
//! | 低阻挡 | `wBkImg & 0x8000 != 0` | `client/reference_full/Classes/map/TMap.cpp:339` |
//! | 高位阻挡 | `wFrImg & 0x8000 != 0` | 同上 |
//!
//! `TMap.cpp:339` 的原句是 `Result = ((pt.wBkImg & 0x8000) + (pt.wFrImg & 0x8000)) == 0;`
//! （在 `CanWalk` 里，两层都不带 `0x8000` 才可走）⇒ 两层各自独立地表达「挡住」，
//! 所以这里**逐层取位**，而不是像 `is_walkable` 那样合成一个布尔。
//!
//! # 二、视觉口径（借 C# `CrystalStudio` 的样式，不借它的位编码）
//!
//! - 色值：低阻挡 `RGB(240,85,105)` / 高位阻挡 `RGB(65,165,245)`
//!   —— `CrystalStudio/CollisionMarkers.cs:6-7` 的 `First` / `Second`；
//! - 透明度：`90/255`（同上两行的首参 alpha）；
//! - 形状：**整格** 48×32 纯色块；同格两者都有 ⇒ 左半格（24×32）低、右半格高
//!   —— `CollisionMarkers.cs:19-25`（`type == 3` 分支按 `cell.Width / 2` 切两半，
//!   其余情况整格铺一种色）；
//! - 时机：三层精灵**画完之后**再叠（`MapViewport.Presentation.cs:40-42`
//!   把 `CollisionMarkers.Draw` 放在地图绘制之后、灯光/门标记之前）。
//!
//! ⚠ C# 的 `MapViewport.cs:23` 用 `BackImage` 的 `0x20000000` / `0x40000000` 表示低/高阻挡，
//! 那是**编辑器自己的待写回编码**，与客户端 `0x8000` 语义无关 ⇒ 本库只照抄颜色/形状/时机，
//! 判定一律按 §一。
//!
//! # 三、与 `cells_drawn` 的关系
//!
//! 标记是**独立通道**：它不写 `cells_drawn`（那个计数始终只统计地表/中层/表层的落笔），
//! 自己走 `cells_low_blocked` / `cells_high_blocked`，且只统计**真的画出像素**的格
//! （`alpha == 0` 时一律 0，见 `Canvas::fill_rect` 的返回值口径）。

use mir2_map::Cell;

use crate::canvas::Canvas;
use crate::types::{CollisionOverlay, CELL_H, CELL_W};

/// 阻挡位：背景层的「低阻挡」与前景层的「高位阻挡」**都是** `0x8000`（各自字段上的同一 bit）。
///
/// 出处：`TMap.cpp:339`（`wBkImg & 0x8000`、`wFrImg & 0x8000`）。
pub(crate) const BLOCK_BIT: u16 = 0x8000;

/// 低阻挡标记色 `RGB(240,85,105)`。
///
/// 出处：`CrystalStudio/CollisionMarkers.cs:6`（`Color.FromArgb(90, 240, 85, 105)` 的 RGB 部分）。
pub(crate) const LOW_RGB: [u8; 3] = [240, 85, 105];

/// 高位阻挡标记色 `RGB(65,165,245)`。
///
/// 出处：`CrystalStudio/CollisionMarkers.cs:7`（`Color.FromArgb(90, 65, 165, 245)` 的 RGB 部分）。
pub(crate) const HIGH_RGB: [u8; 3] = [65, 165, 245];

/// 该格是否**低阻挡**：背景层 `wBkImg` 带 `0x8000`。
///
/// 出处：`TMap.cpp:339`；Rust 侧同口径 `mir2-map/src/cell.rs:210-212`。
pub(crate) const fn has_low(cell: &Cell) -> bool {
    cell.bk_img & BLOCK_BIT != 0
}

/// 该格是否**高位阻挡**：前景层 `wFrImg` 带 `0x8000`。
///
/// 出处：同上。
pub(crate) const fn has_high(cell: &Cell) -> bool {
    cell.fr_img & BLOCK_BIT != 0
}

/// 把某一格的阻挡标记叠到画布上，返回 `(画了低, 画了高)`。
///
/// `(px, py)` 是该格左上角的**画布像素**坐标（= `dx*48, dy*32`，与三层同一套锚点）。
///
/// 分三种情形（对应 `CollisionMarkers.cs:19-25`）：
/// - 两个通道都命中 ⇒ 左半格低、右半格高（`cell.Width / 2` ⇒ 24 px）；
/// - 只命中一个 ⇒ 整格铺该通道的色；
/// - 一个都没命中（含通道关着）⇒ 什么都不画。
pub(crate) fn draw_cell(
    canvas: &mut Canvas,
    overlay: CollisionOverlay,
    cell: &Cell,
    px: i32,
    py: i32,
) -> (bool, bool) {
    // 通道开关先与判定相与：**关掉的通道绝不参与**（只开 low 时，高阻挡格不得被染）
    let low = overlay.low && has_low(cell);
    let high = overlay.high && has_high(cell);
    let a = overlay.alpha;
    match (low, high) {
        (false, false) => (false, false),
        (true, true) => {
            // 同格两者都有：左半格低 / 右半格高（CollisionMarkers.cs:21-22）
            let half = CELL_W / 2;
            let 低 = canvas.fill_rect(带透明度(LOW_RGB, a), px, py, half, CELL_H);
            let 高 = canvas.fill_rect(带透明度(HIGH_RGB, a), px + half as i32, py, CELL_W - half, CELL_H);
            (低, 高)
        }
        (true, false) => (
            canvas.fill_rect(带透明度(LOW_RGB, a), px, py, CELL_W, CELL_H),
            false,
        ),
        (false, true) => (
            false,
            canvas.fill_rect(带透明度(HIGH_RGB, a), px, py, CELL_W, CELL_H),
        ),
    }
}

/// 把 `RGB` + `alpha` 拼成画布要的 RGBA 四元组。
#[inline]
const fn 带透明度(rgb: [u8; 3], alpha: u8) -> [u8; 4] {
    [rgb[0], rgb[1], rgb[2], alpha]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一格：只设两层图号（其余字段 0，与「空图」同构）。
    fn 格(bk: u16, fr: u16) -> Cell {
        Cell {
            bk_img: bk,
            fr_img: fr,
            ..Default::default()
        }
    }

    #[test]
    fn 判定只看各自层的_0x8000() {
        assert!(has_low(&格(0x8000, 0)));
        assert!(!has_high(&格(0x8000, 0)), "背景层的位不算高位阻挡");
        assert!(has_high(&格(0, 0x8000)));
        assert!(!has_low(&格(0, 0x8000)), "前景层的位不算低阻挡");
        assert!(has_low(&格(0x8001, 0x8001)));
        // 低 15 位是图号，与阻挡无关：0x7FFF 不带 0x8000
        assert!(!has_low(&格(0x7FFF, 0)));
    }

    #[test]
    fn 关掉的通道一个像素都不画() {
        let cell = 格(0x8000, 0x8000);
        let mut c = Canvas::new(CELL_W, CELL_H);
        // 通道全关（= 默认口径）
        assert_eq!(draw_cell(&mut c, CollisionOverlay::none(), &cell, 0, 0), (false, false));
        assert!(c.into_rgba().iter().all(|b| *b == 0), "默认不得画任何标记");

        // 只开 low ⇒ 只画低，高的位被忽略
        let mut c = Canvas::new(CELL_W, CELL_H);
        assert_eq!(draw_cell(&mut c, CollisionOverlay::low_only(), &cell, 0, 0), (true, false));
        let px = c.into_rgba();
        // 整格都是低阻挡色（右半格也不例外 —— 因为高通道关着）
        let 低 = [LOW_RGB[0], LOW_RGB[1], LOW_RGB[2], CollisionOverlay::DEFAULT_ALPHA];
        assert!(px.as_chunks::<4>().0.iter().all(|p| *p == 低));
    }

    #[test]
    fn 两者都有时左半低右半高() {
        let cell = 格(0x8000, 0x8000);
        let mut c = Canvas::new(CELL_W, CELL_H);
        assert_eq!(draw_cell(&mut c, CollisionOverlay::all(), &cell, 0, 0), (true, true));
        let px = c.into_rgba();
        let 低 = [LOW_RGB[0], LOW_RGB[1], LOW_RGB[2], CollisionOverlay::DEFAULT_ALPHA];
        let 高 = [HIGH_RGB[0], HIGH_RGB[1], HIGH_RGB[2], CollisionOverlay::DEFAULT_ALPHA];
        let 半 = (CELL_W / 2) as usize;
        for y in 0..CELL_H as usize {
            for x in 0..CELL_W as usize {
                let i = (y * CELL_W as usize + x) * 4;
                let want = if x < 半 { 低 } else { 高 };
                assert_eq!(&px[i..i + 4], &want, "({x},{y}) 颜色不符");
            }
        }
    }

    #[test]
    fn alpha为零时不算画了() {
        let cell = 格(0x8000, 0x8000);
        let mut c = Canvas::new(CELL_W, CELL_H);
        let 全透明 = CollisionOverlay {
            low: true,
            high: true,
            alpha: 0,
        };
        assert_eq!(draw_cell(&mut c, 全透明, &cell, 0, 0), (false, false));
        assert!(c.into_rgba().iter().all(|b| *b == 0));
    }
}
