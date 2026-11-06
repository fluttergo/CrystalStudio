//! 标题栏：深金色底 + 应用图标 + 窗口标题 + 右侧三个窗口按钮。
//!
//! 全部数值来自 [`crate::metrics`]（每条都注明截图来源）；
//! 颜色来自 [`crate::theme`]（`TITLE_BAR` 取自截图 (300,6)-(1200,26) 直方图 100% `#604715`）；
//! 文案来自 [`crate::ui_text`]（本文件不再出现界面字面量）。
//!
//! 这是**自绘标题栏**（`with_decorations(false)`），所以客户区就是 1500×950，
//! 与参考截图的观感一致（参考截图同样是 Avalonia 的自绘标题栏）。
//!
//! ⚠ 标题文字**本单起是动态的**：上一版写死 `未命名 • 256 × 256 格 — 地图编辑器`，
//! 图一打开这句话就是假的（图根本不是 256×256）。现在由 [`dyn_labels`] 用
//! [`crate::ui_text::TITLE_TMPL`] 填真实文件名与格数；品牌段按 2026-11-06 用户裁定
//! 固定写 `Crystal Studio`（旧版的「地图编辑器」只是历史写法）。

use egui::{pos2, vec2, Align2, Painter, Rect, Response, Sense, Ui};

use crate::layout::Layout;
use crate::metrics as m;
use crate::paint as pt;
use crate::screen_text::DocText;
use crate::text_fit::{DynLabel, Label, LabelStyle};
use crate::theme as t;
use crate::ui_text as tx;

/// 第 `i` 个窗口按钮的矩形（自右向左排：0 = 关闭、1 = 最大化、2 = 最小化）。
fn caption_rect(l: &Layout, i: usize) -> Rect {
    let r = l.title_bar;
    let right = r.right() - m::CAPTION_BTN_W * i as f32;
    Rect::from_min_size(
        pos2(right - m::CAPTION_BTN_W, r.top()),
        vec2(m::CAPTION_BTN_W, r.height()),
    )
}

/// 标题栏上的**静态**文字标签（三个窗口按钮字形）。
///
/// 标题本身是动态的，见 [`dyn_labels`]。
pub fn labels(l: &Layout) -> Vec<Label> {
    let mut out = Vec::with_capacity(3);
    for (i, g) in tx::CAPTION_GLYPHS.iter().enumerate() {
        let btn = caption_rect(l, i);
        out.push(Label {
            name: ["titlebar.close", "titlebar.maximize", "titlebar.minimize"][i],
            text: g,
            font: m::CAPTION_FONT,
            pos: btn.center(),
            anchor: Align2::CENTER_CENTER,
            clip: btn,
            color: t::ui().title_bar_text,
        });
    }
    out
}

/// 标题栏上的**动态**文字标签（窗口标题 = 真实文件名 + 格数）。
pub fn dyn_labels(l: &Layout, d: &DocText) -> Vec<DynLabel> {
    let r = l.title_bar;
    // 右边不得侵入三个窗口按钮区（3 × 46 = 138）
    vec![DynLabel::new(
        "titlebar.title",
        tx::TITLE_TMPL,
        d.title.clone(),
        LabelStyle::new(
            m::TITLE_FONT,
            pos2(r.left() + m::TITLE_TEXT_X, r.center().y),
            Align2::LEFT_CENTER,
            Rect::from_min_max(
            pos2(r.left() + m::TITLE_TEXT_X, r.top()),
            pos2(r.right() - m::CAPTION_BTN_W * 3.0, r.bottom()),
        ),
            t::ui().title_bar_text,
        ),
    )]
}

/// 画标题栏。
pub fn draw(p: &Painter, l: &Layout, d: &DocText) {
    let r = l.title_bar;
    pt::fill(p, r, t::ui().title_bar);

    // ① 应用图标：距左 8 的 15×15 圆角方块（截图 x=16..30 的浅色字形簇）
    let icon = Rect::from_min_size(
        pos2(
            r.left() + m::TITLE_ICON_X,
            r.center().y - m::TITLE_ICON_SIZE * 0.5,
        ),
        vec2(m::TITLE_ICON_SIZE, m::TITLE_ICON_SIZE),
    );
    pt::fill_round(p, icon, 3.0, t::ui().title_bar_text);
    // 图标内部的 2×2 网格镂空（模拟参考图那个"窗口分格"小图标）
    let cell = icon.width() / 2.0;
    for gy in 0..2 {
        for gx in 0..2 {
            pt::fill(
                p,
                Rect::from_min_size(
                    pos2(
                        icon.left() + gx as f32 * cell,
                        icon.top() + gy as f32 * cell,
                    ),
                    vec2(cell, cell),
                )
                .shrink(cell * 0.18),
                t::ui().title_bar,
            );
        }
    }

    // ② 标题文字（动态） + 三个窗口按钮字形
    for lab in dyn_labels(l, d) {
        let mut lab = lab;
        lab.truncate_to_fit(p);
        lab.draw(p);
    }
    for lab in labels(l) {
        lab.draw(p);
    }
}

/// 标题栏拖拽移动窗口：在标题栏区域按下并拖动时让系统开始拖动。
///
/// 注意：右侧三个窗口按钮所在区域**不参与拖拽**，避免"想点关闭却把窗口拖走"。
pub fn handle_drag(ui: &Ui, l: &Layout) -> Response {
    let drag_rect = Rect::from_min_max(
        l.title_bar.min,
        pos2(
            l.title_bar.right() - m::CAPTION_BTN_W * 3.0,
            l.title_bar.bottom(),
        ),
    );
    let resp = ui.interact(
        drag_rect,
        ui.id().with("titlebar_drag"),
        Sense::click_and_drag(),
    );
    if resp.drag_started() {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
    resp
}

/// 关闭按钮矩形（`app::interact` 里命中测试用）。
pub fn close_rect(l: &Layout) -> Rect {
    caption_rect(l, 0)
}
