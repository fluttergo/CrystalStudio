//! 菜单栏：`文件 / 编辑 / 视图 / 工具 / 帮助` + 右端品牌字 `Crystal Studio`。
//!
//! 几何取自 [`crate::metrics`]（菜单左内边距 18、项内 padding 12），
//! 颜色取自 [`crate::theme`]（`MENU_BAR` = 截图 (300,36)-(1100,60) 直方图 100% `#F5F6F7`），
//! 文案取自 [`crate::ui_text`]。
//!
//! **本单不做下拉菜单**（任务书 §一：不做编辑逻辑）；菜单项只画、只响应 hover 高亮。

use egui::{pos2, Align2, Painter, Rect, Ui};

use crate::layout::Layout;
use crate::metrics as m;
use crate::paint as pt;
use crate::text_fit::Label;
use crate::theme as t;
use crate::ui_text as tx;

/// 算出每个菜单项的矩形（供绘制与命中测试共用）。
///
/// 宽度是**量出来的**（文字宽 + 左右 padding 各 12），所以中文菜单项会自然排开，
/// 不会像写死宽度那样把"工具"挤成两半。
pub fn item_rects(p: &Painter, l: &Layout) -> Vec<Rect> {
    let mut out = Vec::with_capacity(tx::MENU_ITEMS.len());
    let mut x = l.menu_bar.left() + m::MENU_PAD_X;
    for it in tx::MENU_ITEMS {
        let w = pt::text_w(p, it, m::MENU_FONT) + m::MENU_ITEM_PAD_X * 2.0;
        out.push(Rect::from_min_size(
            pos2(x, l.menu_bar.top()),
            egui::vec2(w, l.menu_bar.height()),
        ));
        x += w;
    }
    out
}

/// 菜单栏上的文字标签（5 个菜单项 + 右端品牌字）。
pub fn labels(p: &Painter, l: &Layout) -> Vec<Label> {
    let r = l.menu_bar;
    let rects = item_rects(p, l);
    let mut out = Vec::with_capacity(rects.len() + 1);

    for (i, (rect, item)) in rects.iter().zip(tx::MENU_ITEMS.iter()).enumerate() {
        out.push(Label {
            name: ["menu.file", "menu.edit", "menu.view", "menu.tools", "menu.help"][i],
            text: item,
            font: m::MENU_FONT,
            pos: rect.center(),
            anchor: Align2::CENTER_CENTER,
            clip: *rect,
            color: t::ui().menu_bar_text,
        });
    }

    // 右端品牌字：右边距同左端 18；左边不得早于最后一个菜单项右边
    let brand_left = rects.last().map_or(r.left(), |x| x.right()) + m::MENU_ITEM_PAD_X;
    out.push(Label {
        name: "menu.brand",
        text: tx::BRAND,
        font: m::MENU_FONT,
        pos: pos2(r.right() - m::MENU_PAD_X, r.center().y),
        anchor: Align2::RIGHT_CENTER,
        clip: Rect::from_min_max(
            pos2(brand_left, r.top()),
            // 右对齐文字的裁剪盒右边界与文字右边界重合，外扩 `TEXT_CLIP_SLACK` 才能判定"没被切"
            pos2(r.right() - m::MENU_PAD_X + m::TEXT_CLIP_SLACK, r.bottom()),
        ),
        color: t::ui().menu_bar_text,
    });
    out
}

/// 画菜单栏。
pub fn draw(p: &Painter, l: &Layout) {
    let r = l.menu_bar;
    pt::fill(p, r, t::ui().menu_bar);
    // 菜单栏下沿 1px 分隔线（截图 y=66 整行为 `#EAECEF`）。
    // 不画的话菜单栏与页签条同为 `#F5F6F7`，两者会连成一片（实测菜单栏被量成 74px）。
    pt::fill(p, l.menu_sep, t::ui().separator);

    for lab in labels(p, l) {
        lab.draw(p);
    }
}

/// 菜单项的 hover 命中测试（本单只做视觉反馈，不做下拉）。
pub fn handle_hover(ui: &Ui, l: &Layout) -> Option<usize> {
    let p = ui.painter();
    for (i, rect) in item_rects(p, l).iter().enumerate() {
        let resp = ui.interact(*rect, ui.id().with(("menu_item", i)), egui::Sense::hover());
        if resp.hovered() {
            return Some(i);
        }
    }
    None
}
