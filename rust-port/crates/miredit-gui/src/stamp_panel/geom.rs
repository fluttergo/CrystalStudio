//! 印章面板的**几何与分页算术**（绘制、命中测试、探针共用同一份）。
//!
//! 全部由 [`crate::metrics`] 的常量推出：**本文件不出现任何"看起来差不多"的字面量**。

use egui::{pos2, vec2, Rect};

use crate::layout::Layout;
use crate::metrics as m;

use super::StampPanel;

impl StampPanel {
    // ── 几何（绘制与命中测试共用同一份；全部由 metrics 常量推出） ──

    /// 网格区（复用资源库页签的同一块矩形：页签/下拉/搜索之下、提示行与分页行之上）。
    pub fn grid_rect(l: &Layout) -> Rect {
        crate::palette::PaletteState::grid_rect(l)
    }

    /// 提示行（网格与分页行之间那条 16px 窄带）。
    pub fn note_rect(l: &Layout) -> Rect {
        crate::palette::PaletteState::note_rect(l)
    }

    /// 可见行数 = 网格高 / 单格高（向下取整，不铺半行）。
    pub fn rows_visible(l: &Layout) -> usize {
        let h = Self::grid_rect(l).height();
        if h <= 0.0 {
            return 0;
        }
        (h / m::STAMP_CELL_H).floor().max(0.0) as usize
    }

    /// **真正可见**的网格矩形（整行数 × 单格高，切掉底部那截半行）。
    pub fn visible_rect(l: &Layout) -> Rect {
        let g = Self::grid_rect(l);
        let rows = Self::rows_visible(l);
        Rect::from_min_max(g.min, pos2(g.right(), g.top() + rows as f32 * m::STAMP_CELL_H))
    }

    /// 某格的屏幕矩形（`row` 是**页内**行号，已含 `row_off` 之外的偏移）。
    pub fn cell_rect(l: &Layout, row_off: usize, row: usize, col: usize) -> Rect {
        let g = Self::grid_rect(l);
        let dy = (row as f32 - row_off as f32) * m::STAMP_CELL_H;
        Rect::from_min_size(
            pos2(g.left() + col as f32 * m::STAMP_CELL_W, g.top() + dy),
            vec2(m::STAMP_CELL_W, m::STAMP_CELL_H),
        )
    }

    /// 把屏幕点换算成列表下标（不在网格内 / 越界 → `None`）。
    pub fn hit_test(&self, l: &Layout, pos: egui::Pos2) -> Option<usize> {
        let vis = Self::visible_rect(l);
        if !vis.contains(pos) {
            return None;
        }
        let col = ((pos.x - vis.left()) / m::STAMP_CELL_W).floor();
        let row = ((pos.y - vis.top()) / m::STAMP_CELL_H).floor();
        if col < 0.0 || row < 0.0 || col >= m::STAMP_GRID_COLS as f32 {
            return None;
        }
        let (start, end) = page_range(self.total(), self.page);
        let idx = start + (self.row_off + row as usize) * m::STAMP_GRID_COLS + col as usize;
        (idx < end).then_some(idx)
    }

    /// 分类下拉弹层矩形（复用下拉框矩形向下展开）。
    pub fn combo_popup_rect(&self, l: &Layout) -> Rect {
        let combo = crate::dock_right::combo_rect(l);
        let rows = self.popup_rows();
        Rect::from_min_size(
            pos2(combo.left(), combo.bottom() + 2.0),
            vec2(
                combo.width(),
                rows as f32 * m::PALETTE_COMBO_ROW_H + 2.0 * m::TEXT_CLIP_SLACK,
            ),
        )
    }

    /// 弹层一次显示几行。
    pub fn popup_rows(&self) -> usize {
        self.kind_rows().len().min(m::STAMP_COMBO_ROWS)
    }

    /// 弹层第 `i` 行（0 = 弹层第一行）的矩形。
    pub fn combo_row_rect(&self, l: &Layout, i: usize) -> Rect {
        let pop = self.combo_popup_rect(l);
        Rect::from_min_size(
            pos2(
                pop.left() + 1.0,
                pop.top() + m::TEXT_CLIP_SLACK + i as f32 * m::PALETTE_COMBO_ROW_H,
            ),
            vec2(pop.width() - 2.0, m::PALETTE_COMBO_ROW_H),
        )
    }
}

/// 每页条数 → 总页数（空列表也算 1 页）。
pub fn page_count(total: usize) -> usize {
    total.div_ceil(m::STAMP_PAGE_SIZE).max(1)
}

/// 某页覆盖的列表区间 `[start, end)`（越界页夹到最后一页）。
pub fn page_range(total: usize, page: usize) -> (usize, usize) {
    let page = page.min(page_count(total).saturating_sub(1));
    let start = (page * m::STAMP_PAGE_SIZE).min(total);
    let end = (start + m::STAMP_PAGE_SIZE).min(total);
    (start, end)
}

/// 该页的行数（用于把页内滚动夹在合法范围）。
pub fn page_rows(total: usize, page: usize) -> usize {
    let (start, end) = page_range(total, page);
    (end - start).div_ceil(m::STAMP_GRID_COLS)
}

/// 页内滚动的最大值（`rows_visible` 行之外还能滚几行）。
pub fn max_row_off(total: usize, page: usize, rows_visible: usize) -> usize {
    page_rows(total, page).saturating_sub(rows_visible)
}

