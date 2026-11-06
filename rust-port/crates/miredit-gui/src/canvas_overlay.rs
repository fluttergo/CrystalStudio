//! 画布**叠加层**：矩形选区框（金色虚线 + 半透明填充）与印章待放置轮廓框。
//!
//! # 一、两条硬纪律（任务书 §四.2 / §三.5）
//!
//! 1. **选区必须可见**：金色虚线框 + 半透明填充，颜色全部来自 [`crate::theme`]；
//! 2. **印章预览只画轮廓，不许画假像素**：它只描一条边，**不填任何像素**、不贴缩略图。
//!    理由：预览时手上只有**包围盒**（真实数据），没有逐格像素；若硬画一层半透明底
//!    让人以为"图已经在那儿了"，点下去才发现位置不对 —— 那是假数据。
//!
//! # 二、为什么单独一个模块
//!
//! [`crate::canvas`] 负责"把地图贴上去 + 悬停格高亮 + 缩放浮层"，它画的东西**全部有
//! 数据含义且来自地图纹理**。本模块画的是**编辑器自己的 UI 叠加层**（选区、待放置轮廓），
//! 两者生命周期与刷新条件都不同（叠加层每帧都变，地图纹理只在视口变化时重渲）。
//! 混在一个文件里，改叠加层就有碰坏"同视点分块比对"的风险。
//!
//! # 三、虚线是**用实心小矩形拼的**，不用 `dashed_line`
//!
//! 与 [`crate::paint`] 的既有纪律一致：`egui` 的描边类 API 在 0.31 之后签名漂过
//! （`rect_stroke` 多了 `StrokeKind`），而"一排小矩形"在任何版本上都是同一份结果，
//! 且**逐像素可预测**（截图验收要能数出虚线段数）。[`dashes_along`] 是纯函数，
//! 段长/空档由 [`crate::metrics`] 给，本文件里没有一个"看起来差不多"的字面量。

use egui::{pos2, Pos2, Rect};

use miredit_core::Selection;

use crate::canvas_view::CanvasView;
use crate::metrics as m;
use crate::paint as pt;
use crate::theme as t;

/// 选区在屏幕上的矩形（闭区间两端格的并集）。
///
/// 反解：`cell_rect` 的 `min` 是格左上角、`max` 是右下角 ⇒ 左上取 `(x0,y0)` 的 `min`、
/// 右下取 `(x1,y1)` 的 `max`。选区已被内核裁剪到图内（`Selection::clamp_to`），
/// 所以这里不必再判越界；`x1 < x0` 的反向选区由 `Selection` 自己规范化。
pub fn selection_rect(view: &CanvasView, canvas: Rect, sel: Selection) -> Rect {
    let (x0, y0) = (
        i32::from(sel.x0.min(sel.x1)),
        i32::from(sel.y0.min(sel.y1)),
    );
    let (x1, y1) = (
        i32::from(sel.x0.max(sel.x1)),
        i32::from(sel.y0.max(sel.y1)),
    );
    let tl = view.cell_rect(canvas, x0, y0);
    let br = view.cell_rect(canvas, x1, y1);
    Rect::from_min_max(tl.min, br.max)
}

/// 一条边上的虚线段：返回 `(起点偏移, 终点偏移)` 列表（相对该边起点，单位 px）。
///
/// 纯函数，便于单测核对"段长/空档/末段不越界"。`dash + gap` 至少 0.5 px，
/// 否则 `gap = -dash` 之类的坏参数会让循环原地打转。
pub fn dashes_along(len: f32, dash: f32, gap: f32) -> Vec<(f32, f32)> {
    if len <= 0.0 || dash <= 0.0 {
        return Vec::new();
    }
    let period = (dash + gap).max(0.5);
    let mut out = Vec::new();
    let mut s = 0.0_f32;
    // `len` 最大 65535 格 × 48 px ≈ 3.1e6；period ≥ 0.5 ⇒ 最多 6.3e6 次 ——
    // 但选区不会真这么大（画布可视区只有约 1000 px），加一道上限兜住坏参数。
    while s < len && out.len() < 100_000 {
        let e = (s + dash).min(len);
        if e > s {
            out.push((s, e));
        }
        s += period;
    }
    out
}

/// 虚线边框的**四边小矩形**（`w` = 线宽；上下边占满整宽，左右边让开上下边以免四角加粗）。
pub fn dash_rects(r: Rect, w: f32, dash: f32, gap: f32) -> Vec<Rect> {
    let mut out = Vec::new();
    // 上边 / 下边：整宽
    for (a, b) in dashes_along(r.width(), dash, gap) {
        out.push(Rect::from_min_max(
            pos2(r.left() + a, r.top()),
            pos2(r.left() + b, r.top() + w),
        ));
        out.push(Rect::from_min_max(
            pos2(r.left() + a, r.bottom() - w),
            pos2(r.left() + b, r.bottom()),
        ));
    }
    // 左边 / 右边：让开上下边（各 w 高）
    let inner_h = (r.height() - 2.0 * w).max(0.0);
    for (a, b) in dashes_along(inner_h, dash, gap) {
        out.push(Rect::from_min_max(
            pos2(r.left(), r.top() + w + a),
            pos2(r.left() + w, r.top() + w + b),
        ));
        out.push(Rect::from_min_max(
            pos2(r.right() - w, r.top() + w + a),
            pos2(r.right(), r.top() + w + b),
        ));
    }
    out
}

/// 画矩形选区：**半透明填充 + 金色虚线边框**。
///
/// 顺序：先填后描 —— 填充是"这块被选中"的底色，虚线是边界；反过来会被填充盖掉半边线宽。
pub fn draw_selection(p: &egui::Painter, canvas: Rect, view: &CanvasView, sel: Selection) {
    let r = selection_rect(view, canvas, sel);
    if r.width() <= 0.0 || r.height() <= 0.0 {
        return;
    }
    pt::fill(p, r, t::ui().selection_fill);
    for seg in dash_rects(r, m::SEL_STROKE_W, m::SEL_DASH_LEN, m::SEL_GAP_LEN) {
        pt::fill(p, seg, t::ui().selection_stroke);
    }
}

/// 矩形轮廓的**四条边**（宽 `w`，贴着 `r` 的四条边、互不重叠）。
///
/// 单列成函数是为了让"只画轮廓、不填内部"这件事**可以被单测证明**：
/// 断言四条边都不含矩形中心点（`tests::印章轮廓不填内部`）。
/// 若直接用 [`crate::paint::border_w`] 画，测试只能看代码、不能看几何。
pub fn border_strips(r: Rect, w: f32) -> [Rect; 4] {
    [
        // 上
        Rect::from_min_max(pos2(r.left(), r.top()), pos2(r.right(), r.top() + w)),
        // 下
        Rect::from_min_max(pos2(r.left(), r.bottom() - w), pos2(r.right(), r.bottom())),
        // 左
        Rect::from_min_max(pos2(r.left(), r.top()), pos2(r.left() + w, r.bottom())),
        // 右
        Rect::from_min_max(pos2(r.right() - w, r.top()), pos2(r.right(), r.bottom())),
    ]
}

/// 画印章**待放置轮廓**：只有一圈 2px 实线，**不填任何像素**（任务书 §三.5）。
///
/// `bounds` 是印章的包围盒（闭区间、可负）；越界部分照画，由 painter 的裁剪盒裁到画布。
pub fn draw_stamp_outline(
    p: &egui::Painter,
    canvas: Rect,
    view: &CanvasView,
    anchor: (u16, u16),
    bounds: (i32, i32, i32, i32),
) {
    let r = crate::stamp_place::outline_rect(view, canvas, anchor, bounds);
    if r.width() <= 0.0 || r.height() <= 0.0 {
        return;
    }
    // **只填四条边**：不调 `rect_filled` 铺内部，也不贴任何缩略图 —— 那会是假像素。
    for s in border_strips(r, m::STAMP_PREVIEW_STROKE_W) {
        pt::fill(p, s, t::ui().stamp_preview_stroke);
    }
}

/// 画布左上角的**待放置徽标**（`Pos2` 由调用方给：贴在鼠标格上方，避免盖住落点）。
///
/// 为什么要有它：待放置态本身在画布上没有任何标记（轮廓只在按住 `Alt` 时画），
/// 用户按了 `Enter` 之后如果鼠标在别处，界面上唯一的变化就是状态栏那一句 ——
/// 一个跟着鼠标走的小标记能让"我现在点下去会发生什么"变得明确。
///
/// 返回徽标矩形（绘制方/验收都要知道它占了哪儿）。
pub fn draw_armed_badge(p: &egui::Painter, at: Pos2, text: &str) -> Rect {
    let w = pt::text_w(p, text, m::STATUS_FONT) + m::ARMED_BADGE_PAD_X * 2.0;
    let r = Rect::from_min_size(
        pos2(at.x, at.y - m::ARMED_BADGE_H),
        egui::vec2(w, m::ARMED_BADGE_H),
    );
    pt::fill_round(p, r, m::ARMED_BADGE_RADIUS, t::ui().selection_stroke);
    pt::text(
        p,
        r.center(),
        egui::Align2::CENTER_CENTER,
        text,
        m::STATUS_FONT,
        t::ui().title_bar_text,
    );
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::vec2;

    fn canvas() -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 800.0))
    }

    /// 选区矩形 = 两端格的并集：3×3 ⇒ 144×96 px（1× 缩放）。
    #[test]
    fn 选区矩形是三乘三格() {
        let view = CanvasView::default();
        let r = selection_rect(&view, canvas(), Selection::new(200, 200, 202, 202));
        assert!((r.width() - 3.0 * 48.0).abs() < 0.01, "宽 {}", r.width());
        assert!((r.height() - 3.0 * 32.0).abs() < 0.01, "高 {}", r.height());
        assert!((r.left() - 200.0 * 48.0).abs() < 0.01, "左 {}", r.left());
        assert!((r.top() - 200.0 * 32.0).abs() < 0.01, "上 {}", r.top());
    }

    /// 反向选区（右下 → 左上）画出来是**同一块**（不能因为端点顺序画成负宽）。
    #[test]
    fn 反向选区画同一块() {
        let view = CanvasView::default();
        let a = selection_rect(&view, canvas(), Selection::new(10, 20, 12, 22));
        let b = selection_rect(&view, canvas(), Selection::new(12, 22, 10, 20));
        assert_eq!(a, b);
        assert!(a.width() > 0.0 && a.height() > 0.0);
    }

    /// 虚线段：段长 6 / 空档 4 ⇒ 长 25 的边得到 `(0,6) (10,16) (20,25)`（末段截到边尾）。
    #[test]
    fn 虚线段长与空档() {
        let d = dashes_along(25.0, 6.0, 4.0);
        assert_eq!(d, vec![(0.0, 6.0), (10.0, 16.0), (20.0, 25.0)]);
        // 空/坏参数 ⇒ 空表（不 panic、不原地打转）
        assert!(dashes_along(0.0, 6.0, 4.0).is_empty());
        assert!(dashes_along(10.0, 0.0, 4.0).is_empty());
        // `gap` 为负 ⇒ period 被夹到 ≥0.5，段数 = 10 / 0.5 = 20（有限、不挂死）
        assert_eq!(dashes_along(10.0, 6.0, -6.0).len(), 20);
    }

    /// 虚线矩形：四边都有段，且**没有一段越出矩形**（否则会盖到相邻格上）。
    #[test]
    fn 虚线不越界() {
        let r = Rect::from_min_size(pos2(100.0, 50.0), vec2(144.0, 96.0));
        let segs = dash_rects(r, m::SEL_STROKE_W, m::SEL_DASH_LEN, m::SEL_GAP_LEN);
        assert!(!segs.is_empty());
        for s in &segs {
            assert!(
                s.left() >= r.left() - 0.01
                    && s.right() <= r.right() + 0.01
                    && s.top() >= r.top() - 0.01
                    && s.bottom() <= r.bottom() + 0.01,
                "虚线段 {s:?} 越出了选区矩形 {r:?}"
            );
            assert!(s.width() > 0.0 && s.height() > 0.0, "零面积段 {s:?}");
        }
        // 上下边各占满整宽 ⇒ 横向段的最大右边界必须正好等于矩形右边
        let max_right = segs.iter().map(|s| s.right()).fold(f32::MIN, f32::max);
        assert!((max_right - r.right()).abs() < 0.01, "横向段没铺到右边：{max_right}");
    }

    /// **印章轮廓不填内部**（任务书 §三.5「只画轮廓不画假像素」的几何闸门）。
    ///
    /// 三条断言：
    /// 1. 四条边都**不含矩形中心点**（内部是空的）；
    /// 2. 四条边都**落在矩形内**（不越界到相邻格上）；
    /// 3. 四条边的面积和 = 周长 × 线宽（不多不少 ⇒ 内部一格像素都没铺）。
    #[test]
    fn 印章轮廓不填内部() {
        let r = Rect::from_min_size(pos2(100.0, 50.0), vec2(288.0, 192.0));
        let w = m::STAMP_PREVIEW_STROKE_W;
        let strips = border_strips(r, w);
        let c = r.center();
        for s in &strips {
            assert!(!s.contains(c), "轮廓内部被填了：{s:?} 含中心 {c:?}");
            assert!(
                s.left() >= r.left() - 0.01
                    && s.right() <= r.right() + 0.01
                    && s.top() >= r.top() - 0.01
                    && s.bottom() <= r.bottom() + 0.01,
                "轮廓边越出了矩形：{s:?}"
            );
        }
        // 面积和 = 2×(宽×w) + 2×(高×w)。
        // ⚠ 四个角会被上下边与左右边各覆盖一次（同色 ⇒ 像素上没差别，与既有
        // `paint::border_w` 的行为一致），所以这里按"四条边各一条"算。
        let want = 2.0 * r.width() * w + 2.0 * r.height() * w;
        let got: f32 = strips.iter().map(|s| s.width() * s.height()).sum();
        assert!(
            (got - want).abs() < 0.01,
            "四条边的总面积 {got} ≠ 只描边应有的 {want}（说明内部被铺了像素）"
        );
    }
}
