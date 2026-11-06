//! 绘制原语：把常用的「填一块 / 画一条 / 写字」收敛成几个小函数。
//!
//! 只用 egui 最稳定的 API（`rect_filled` / `line_segment` / `text` / `circle_filled`），
//! **不用** `rect_stroke`（该 API 在 0.31 之后多了 `StrokeKind` 参数，版本间签名不稳），
//! 需要边框时一律用四条 1px 实心矩形拼出来，跨版本稳定。

use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Stroke};

/// 填一块矩形（无圆角）。`radius` 传 0.0 是跨版本安全的写法。
pub fn fill(p: &Painter, r: Rect, color: Color32) {
    p.rect_filled(r, 0.0, color);
}

/// 填一块圆角矩形。
pub fn fill_round(p: &Painter, r: Rect, radius: f32, color: Color32) {
    p.rect_filled(r, radius, color);
}

/// 1px 矩形描边（四条实心细矩形拼成，避免 `rect_stroke` 的版本签名差异）。
pub fn border(p: &Painter, r: Rect, color: Color32) {
    let w = 1.0;
    fill(
        p,
        Rect::from_min_max(r.min, Pos2::new(r.right(), r.top() + w)),
        color,
    );
    fill(
        p,
        Rect::from_min_max(Pos2::new(r.left(), r.bottom() - w), r.max),
        color,
    );
    fill(
        p,
        Rect::from_min_max(r.min, Pos2::new(r.left() + w, r.bottom())),
        color,
    );
    fill(
        p,
        Rect::from_min_max(Pos2::new(r.right() - w, r.top()), r.max),
        color,
    );
}

/// 指定粗细的矩形描边（四条实心细矩形）。
pub fn border_w(p: &Painter, r: Rect, w: f32, color: Color32) {
    fill(
        p,
        Rect::from_min_max(r.min, Pos2::new(r.right(), r.top() + w)),
        color,
    );
    fill(
        p,
        Rect::from_min_max(Pos2::new(r.left(), r.bottom() - w), r.max),
        color,
    );
    fill(
        p,
        Rect::from_min_max(r.min, Pos2::new(r.left() + w, r.bottom())),
        color,
    );
    fill(
        p,
        Rect::from_min_max(Pos2::new(r.right() - w, r.top()), r.max),
        color,
    );
}

/// 横向 1px 线。
pub fn hline(p: &Painter, r: Rect, y: f32, color: Color32) {
    fill(
        p,
        Rect::from_min_max(Pos2::new(r.left(), y), Pos2::new(r.right(), y + 1.0)),
        color,
    );
}

/// 竖向 1px 线。
pub fn vline(p: &Painter, r: Rect, x: f32, color: Color32) {
    fill(
        p,
        Rect::from_min_max(Pos2::new(x, r.top()), Pos2::new(x + 1.0, r.bottom())),
        color,
    );
}

/// 画一段文字。
///
/// - `anchor` 决定 `pos` 是该文字的哪个角（如 `Align2::LEFT_TOP`）。
/// - 返回文字实际占用的矩形，方便调用方做右对齐排版。
pub fn text(p: &Painter, pos: Pos2, anchor: Align2, s: &str, size: f32, color: Color32) -> Rect {
    p.text(pos, anchor, s, FontId::proportional(size), color)
}

/// 画一段文字并返回其宽度（不关心高度时用）。
pub fn text_w(p: &Painter, s: &str, size: f32) -> f32 {
    // `layout_no_wrap` 只做排版不进绘制队列，用来量宽
    p.layout_no_wrap(s.to_owned(), FontId::proportional(size), Color32::WHITE)
        .size()
        .x
}

/// 画一条折线（逐段线段，等价于 polyline）。
pub fn polyline(p: &Painter, pts: &[Pos2], stroke: Stroke) {
    for w in pts.windows(2) {
        p.line_segment([w[0], w[1]], stroke);
    }
}
