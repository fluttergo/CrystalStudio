//! 面板通用骨架：标题条 + ⣿ 装饰符 + 标题 + 折叠箭头。
//!
//! 依据任务书 §三「面板通用样式」：标题条（左侧 ⣿ 装饰符 + 11px 标题 + 折叠箭头）、
//! 1px 分隔线、8px 内边距。实测按截图取值（面板标题条高 23、内边距 5，见 `metrics`）。
//!
//! > ⚠ 截图里面板标题条**看不出一枚明确的折叠箭头**（23px 高、右端为纯 `#E3E6E9`）。
//! > 任务书明确要求这一元素，故按「低对比度 chevron」补绘，颜色取 `theme::TEXT_MUTED`，
//! > 并在汇报 §六 记为「截图不可辨、按任务书要求补绘」。

use egui::{pos2, Align2, Painter, Rect};

use crate::layout::PanelRect;
use crate::metrics as m;
use crate::paint as pt;
use crate::text_fit::Label;
use crate::theme as t;

/// ⣿ 装饰符：2 列 × 3 行 2×2 小方点（总 6×10），每点间距 2。
fn draw_grip(p: &Painter, origin: egui::Pos2, color: egui::Color32) {
    const DOT: f32 = 2.0;
    const GAP: f32 = 2.0;
    for row in 0..3 {
        for col in 0..2 {
            let x = origin.x + col as f32 * (DOT + GAP);
            let y = origin.y + row as f32 * (DOT + GAP);
            pt::fill(
                p,
                Rect::from_min_size(pos2(x, y), egui::vec2(DOT, DOT)),
                color,
            );
        }
    }
    // 用常量校验一下总尺寸，改常量时立刻能发现
    debug_assert!(((2.0 * DOT + GAP) - m::GRIP_W).abs() < f32::EPSILON);
    debug_assert!(((3.0 * DOT + 2.0 * GAP) - m::GRIP_H).abs() < f32::EPSILON);
}

/// 折叠箭头（chevron，方向朝下表示可展开/收起）。
fn draw_chevron(p: &Painter, center: egui::Pos2, color: egui::Color32) {
    const W: f32 = 4.0;
    const H: f32 = 2.5;
    let stroke = egui::Stroke::new(1.0_f32, color);
    p.line_segment(
        [
            pos2(center.x - W, center.y - H),
            pos2(center.x, center.y + H),
        ],
        stroke,
    );
    p.line_segment(
        [
            pos2(center.x, center.y + H),
            pos2(center.x + W, center.y - H),
        ],
        stroke,
    );
}

/// 面板标题的文字标签（绘制与截断验收共用）。
///
/// 裁剪盒右边界 = 标题条右边 - `PANEL_CHEVRON_INSET`（折叠箭头位置）- 4px 余量，
/// 中文标题再长也不能压到箭头。
pub fn title_label(panel: &PanelRect, title: &'static str, name: &'static str) -> Label {
    let h = panel.header;
    let title_x = h.left() + m::PANEL_TITLE_PAD_L + m::GRIP_W + m::PANEL_TITLE_GAP;
    Label {
        name,
        text: title,
        font: m::PANEL_TITLE_FONT,
        pos: pos2(title_x, h.center().y),
        anchor: Align2::LEFT_CENTER,
        clip: Rect::from_min_max(
            pos2(title_x, h.top()),
            pos2(h.right() - m::PANEL_CHEVRON_INSET - 4.0, h.bottom()),
        ),
        color: t::ui().text,
    }
}

/// 画一条面板标题条。
pub fn draw_header(p: &Painter, panel: &PanelRect, title: &'static str, name: &'static str) {
    let h = panel.header;
    // 标题条底色
    pt::fill(p, h, t::ui().panel_header);
    // ⣿ 装饰符：距面板左边 7、距标题条顶 8（取自截图 grip 像素簇 (15,75)-(20,84)）
    draw_grip(
        p,
        pos2(h.left() + m::PANEL_TITLE_PAD_L, h.top() + 8.0),
        t::ui().text,
    );
    // 标题文字
    title_label(panel, title, name).draw(p);
    // 折叠箭头：距右边缘 12
    draw_chevron(
        p,
        pos2(h.right() - m::PANEL_CHEVRON_INSET, h.center().y),
        t::ui().text_muted,
    );
}

/// 画面板主体底色。
pub fn draw_body(p: &Painter, panel: &PanelRect, bg: egui::Color32) {
    pt::fill(p, panel.content, bg);
}

/// 画面板整体（底色 + 标题条）。
pub fn draw_panel(
    p: &Painter,
    panel: &PanelRect,
    title: &'static str,
    name: &'static str,
    body_bg: egui::Color32,
) {
    draw_body(p, panel, body_bg);
    draw_header(p, panel, title, name);
}
