//! 三个浮层的绘制与输入：**打开地图模态**、**另存为模态**（都可输入）与**快捷键浮层**（只读）。
//!
//! # 为什么"打开地图"要一个输入框而不是文件对话框
//!
//! 本 crate 的依赖纪律是「只用 egui/eframe + workspace 内库」（见 `Cargo.toml` 顶部注释），
//! 而 egui 自身**没有**文件选择器，`rfd` 之类是新增依赖 ⇒ 不加。
//! 于是这里做的是**真输入**：用户把 `.map` 的完整路径敲进输入框，回车后走
//! [`crate::menu::action::open_path`]（与启动时完全相同的 `Document::load`）。
//! 它不是假控件：输入的文字真的进了 `path`，真的被读盘、真的换图。
//!
//! # 「另存为」是同一套输入的两个实例（2026-11-05）
//!
//! 官方 `文件 → 另存为` 弹的是系统文件对话框（`SaveAs_04C37C25`），本仓同样没有那个依赖，
//! 所以走**同一套**输入：标题/提示/按钮文案换成 [`crate::ui_text::SAVE_AS_DLG_TITLE`] 那三条，
//! 回车后提交 [`DialogAction::SaveAs`] → [`crate::menu::action::save_as_path`] →
//! [`crate::save::save_as_session`]。
//!
//! ⚠ **几何、焦点、光标、退格、`Esc`/`Enter` 只有一份实现**
//! （[`dialog_rects`] + [`interact_path_dialog`]），两个模态的差别只有 [`Kind`] 里的四条文案
//! 与提交出来的动作 —— 不许各写一套（那样"打开能退格、另存为不能"这类偏差会立刻出现）。
//!
//! # 键盘怎么进到输入框
//!
//! 与资源面板搜索框同一套做法（`palette.rs` 手写焦点 + `Event::Text`）：
//! 本仓没有用 `egui::TextEdit`（它是 widget，要 `Ui` 布局），
//! 而是直接读 `Event::Text` / `Backspace` —— 好处是弹层的排版仍由 `metrics` 常量决定。
//!
//! ⚠ 模态打开期间 `app::handle_keys` **必须整体让位**（否则敲 `f` 会切工具、敲 `o` 会切印章），
//! 只保留 `Esc` / `Enter` / `Backspace` / 文本输入。

use egui::{pos2, vec2, Align2, Painter, Rect, Ui};

use crate::layout::Layout;
use crate::menu::Overlay;
use crate::metrics as m;
use crate::paint as pt;
use crate::theme as t;
use crate::ui_text as tx;

/// 路径输入模态的关键矩形（绘制与命中测试共用；两个模态**同一套几何**）。
#[derive(Clone, Copy, Debug)]
pub struct DialogRects {
    /// 面板整体。
    pub panel: Rect,
    /// 路径输入框。
    pub field: Rect,
    /// 主按钮（打开 / 另存为）。
    pub ok: Rect,
    /// 「取消」按钮。
    pub cancel: Rect,
}

/// 算路径输入模态的几何（居中）。
pub fn dialog_rects(l: &Layout) -> DialogRects {
    let panel = Rect::from_center_size(l.content.center(), vec2(m::OPEN_DLG_W, m::OPEN_DLG_H));
    let inner_l = panel.left() + m::OPEN_DLG_PAD;
    let inner_r = panel.right() - m::OPEN_DLG_PAD;
    // 纵向：标题 24 → 输入框 → 提示 18 → 按钮
    let field_top = panel.top() + m::OPEN_DLG_PAD + 24.0;
    let field = Rect::from_min_size(
        pos2(inner_l, field_top),
        vec2(inner_r - inner_l, m::OPEN_DLG_FIELD_H),
    );
    let btn_y = panel.bottom() - m::OPEN_DLG_PAD - m::OPEN_DLG_BTN_H;
    let cancel = Rect::from_min_size(
        pos2(inner_r - m::OPEN_DLG_BTN_W, btn_y),
        vec2(m::OPEN_DLG_BTN_W, m::OPEN_DLG_BTN_H),
    );
    let ok = Rect::from_min_size(
        pos2(cancel.left() - 10.0 - m::OPEN_DLG_BTN_W, btn_y),
        vec2(m::OPEN_DLG_BTN_W, m::OPEN_DLG_BTN_H),
    );
    DialogRects {
        panel,
        field,
        ok,
        cancel,
    }
}

/// 打开地图模态的几何（= [`dialog_rects`]，保留这个名字给既有调用方/用例）。
pub fn open_rects(l: &Layout) -> DialogRects {
    dialog_rects(l)
}

/// 另存为模态的几何（= [`dialog_rects`]；两个模态的排版**必须一致**）。
pub fn save_as_rects(l: &Layout) -> DialogRects {
    dialog_rects(l)
}

/// 模态里的一次输入结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DialogAction {
    /// 什么都不做。
    None,
    /// 取消（Esc / 点取消）。
    Cancel,
    /// 打开这个路径（回车 / 点打开）。
    Open(String),
    /// 另存为到这个路径（回车 / 点另存为）。
    SaveAs(String),
    /// 导出选区到这个路径（回车 / 点导出）。
    ExportSelection(String),
    /// 导出小地图到这个路径（回车 / 点导出）。
    ExportMinimap(String),
}

/// 路径输入模态的**四种**用途（差别只有四条文案与提交出来的动作）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// 「文件 → 打开地图…」。
    Open,
    /// 「文件 → 另存为…」。
    SaveAs,
    /// 「文件 → 导出选区(E)…」。
    ExportSelection,
    /// 「文件 → 导出小地图(M)…」。
    ExportMinimap,
}

impl Kind {
    /// 模态标题。
    fn title(self) -> &'static str {
        match self {
            Kind::Open => tx::OPEN_DLG_TITLE,
            Kind::SaveAs => tx::SAVE_AS_DLG_TITLE,
            Kind::ExportSelection => tx::EXPORT_SEL_DLG_TITLE,
            Kind::ExportMinimap => tx::EXPORT_MINI_DLG_TITLE,
        }
    }

    /// 输入框下方那句提示。
    fn hint(self) -> &'static str {
        match self {
            Kind::Open => tx::OPEN_DLG_HINT,
            Kind::SaveAs => tx::SAVE_AS_DLG_HINT,
            Kind::ExportSelection => tx::EXPORT_SEL_DLG_HINT,
            Kind::ExportMinimap => tx::EXPORT_MINI_DLG_HINT,
        }
    }

    /// 主按钮文字。
    fn ok_text(self) -> &'static str {
        match self {
            Kind::Open => tx::OPEN_DLG_OK,
            Kind::SaveAs => tx::SAVE_AS_DLG_OK,
            Kind::ExportSelection => tx::EXPORT_SEL_DLG_OK,
            Kind::ExportMinimap => tx::EXPORT_MINI_DLG_OK,
        }
    }

    /// 两个按钮的 egui id 前缀（**每个模态不同 id**：同一帧只有一个浮层，但 id 撞了会让
    /// `interact` 的点击归属变得依赖绘制顺序）。
    fn id_prefix(self) -> &'static str {
        match self {
            Kind::Open => "open_dlg",
            Kind::SaveAs => "save_as_dlg",
            Kind::ExportSelection => "export_sel_dlg",
            Kind::ExportMinimap => "export_mini_dlg",
        }
    }

    /// 把输入框里的文字折成提交动作。
    fn submit(self, path: &str) -> DialogAction {
        match self {
            Kind::Open => DialogAction::Open(path.to_owned()),
            Kind::SaveAs => DialogAction::SaveAs(path.to_owned()),
            Kind::ExportSelection => DialogAction::ExportSelection(path.to_owned()),
            Kind::ExportMinimap => DialogAction::ExportMinimap(path.to_owned()),
        }
    }
}

/// 从浮层里取出**该种类**的输入缓冲（种类与变体不匹配 ⇒ `None`，调用方什么都不做）。
fn path_mut(overlay: &mut Overlay, kind: Kind) -> Option<&mut String> {
    match (kind, overlay) {
        (Kind::Open, Overlay::OpenMap { path }) => Some(path),
        (Kind::SaveAs, Overlay::SaveAs { path }) => Some(path),
        (Kind::ExportSelection, Overlay::ExportSelection { path }) => Some(path),
        (Kind::ExportMinimap, Overlay::ExportMinimap { path }) => Some(path),
        _ => None,
    }
}

/// 画路径输入模态（打开 / 另存为共用）。
fn draw_path_dialog(p: &Painter, l: &Layout, path: &str, kind: Kind) {
    let pal = t::ui();
    pt::fill(p, l.content, pal.modal_scrim);
    let r = dialog_rects(l);
    pt::fill(p, r.panel, pal.panel_bg);
    pt::border(p, r.panel, pal.menu_popup_border);

    let inner_l = r.panel.left() + m::OPEN_DLG_PAD;
    // 标题
    pt::text(
        p,
        pos2(inner_l, r.panel.top() + m::OPEN_DLG_PAD + 12.0),
        Align2::LEFT_CENTER,
        kind.title(),
        m::PANEL_TITLE_FONT,
        pal.text,
    );

    // 输入框：底 + 边 + 文字（右端画一个竖线当光标，因为它是"正在输入"的那一格）
    pt::fill(p, r.field, pal.input_search_bg);
    pt::border(p, r.field, pal.input_border);
    let text_x = r.field.left() + 8.0;
    let text_right = r.field.right() - 8.0;
    let w = pt::text_w(p, path, m::BODY_FONT);
    // 太长就从左边裁：只画尾部（用户正在敲的通常是最右边那几个字）
    let visible = if w > (text_right - text_x) {
        let cut = ((w - (text_right - text_x)) / m::BODY_FONT).ceil() as usize + 1;
        path.chars().skip(cut.min(path.chars().count())).collect::<String>()
    } else {
        path.to_owned()
    };
    pt::text(
        p,
        pos2(text_x, r.field.center().y),
        Align2::LEFT_CENTER,
        &visible,
        m::BODY_FONT,
        pal.text,
    );
    let caret_x = (text_x + pt::text_w(p, &visible, m::BODY_FONT)).min(text_right);
    pt::fill(
        p,
        Rect::from_min_size(pos2(caret_x, r.field.top() + 5.0), vec2(1.0, r.field.height() - 10.0)),
        pal.accent_dark,
    );

    // 提示
    pt::text(
        p,
        pos2(inner_l, r.field.bottom() + 12.0),
        Align2::LEFT_CENTER,
        kind.hint(),
        m::BODY_FONT,
        pal.text_muted,
    );

    // 两个按钮：主按钮用强调色底 + `accent_button_text`，次按钮用输入框底
    pt::fill(p, r.ok, pal.accent);
    pt::border(p, r.ok, pal.accent_dark);
    pt::text(
        p,
        r.ok.center(),
        Align2::CENTER_CENTER,
        kind.ok_text(),
        m::BODY_FONT,
        pal.accent_button_text,
    );
    pt::fill(p, r.cancel, pal.input_bg);
    pt::border(p, r.cancel, pal.input_border);
    pt::text(
        p,
        r.cancel.center(),
        Align2::CENTER_CENTER,
        tx::OPEN_DLG_CANCEL,
        m::BODY_FONT,
        pal.text,
    );
}

/// 画打开地图模态。
pub fn draw_open_map(p: &Painter, l: &Layout, path: &str) {
    draw_path_dialog(p, l, path, Kind::Open);
}

/// 画另存为模态（与打开模态同一套排版，文案见 [`Kind::SaveAs`]）。
pub fn draw_save_as(p: &Painter, l: &Layout, path: &str) {
    draw_path_dialog(p, l, path, Kind::SaveAs);
}

/// 画「导出选区」模态（同一套排版，文案见 [`Kind::ExportSelection`]）。
pub fn draw_export_selection(p: &Painter, l: &Layout, path: &str) {
    draw_path_dialog(p, l, path, Kind::ExportSelection);
}

/// 画「导出小地图」模态（同一套排版，文案见 [`Kind::ExportMinimap`]）。
pub fn draw_export_minimap(p: &Painter, l: &Layout, path: &str) {
    draw_path_dialog(p, l, path, Kind::ExportMinimap);
}

/// 路径输入模态的输入处理（**只在对应的浮层变体在场时调用**）。
///
/// 顺序有因果：先判 `Esc` / `Enter` / 按钮点击，再吃文本与退格 ——
/// 否则同一次按键既会被当成"确认"又会被写进路径。
fn interact_path_dialog(
    ui: &mut Ui,
    l: &Layout,
    overlay: &mut Overlay,
    kind: Kind,
) -> DialogAction {
    let Some(path) = path_mut(overlay, kind) else {
        return DialogAction::None;
    };
    let r = dialog_rects(l);

    // ① Esc / Enter
    let (esc, enter) = ui.input(|i| {
        (
            i.key_pressed(egui::Key::Escape),
            i.key_pressed(egui::Key::Enter),
        )
    });
    if esc {
        return DialogAction::Cancel;
    }
    // ② 按钮点击
    if ui
        .interact(
            r.cancel,
            ui.id().with((kind.id_prefix(), "cancel")),
            egui::Sense::click(),
        )
        .clicked()
    {
        return DialogAction::Cancel;
    }
    if ui
        .interact(
            r.ok,
            ui.id().with((kind.id_prefix(), "ok")),
            egui::Sense::click(),
        )
        .clicked()
    {
        return kind.submit(path);
    }
    // ③ 回车 = 确认（放在按钮之后：点按钮那次点击的同一帧也可能带 Enter）
    if enter {
        return kind.submit(path);
    }
    // ④ 文本与退格
    let (texts, backspace) = ui.input(|i| {
        let mut texts = Vec::new();
        for e in &i.events {
            if let egui::Event::Text(t) = e {
                texts.push(t.clone());
            }
        }
        (texts, i.key_pressed(egui::Key::Backspace))
    });
    for t in texts {
        // 只收可见 ASCII + 中文路径里可能出现的字符；换行/制表一律丢掉
        for c in t.chars() {
            if !c.is_control() {
                path.push(c);
            }
        }
    }
    if backspace {
        path.pop();
    }
    DialogAction::None
}

/// 处理打开地图模态的输入（**只在 `Overlay::OpenMap` 时调用**）。
pub fn interact_open_map(ui: &mut Ui, l: &Layout, overlay: &mut Overlay) -> DialogAction {
    interact_path_dialog(ui, l, overlay, Kind::Open)
}

/// 处理另存为模态的输入（**只在 `Overlay::SaveAs` 时调用**）。
pub fn interact_save_as(ui: &mut Ui, l: &Layout, overlay: &mut Overlay) -> DialogAction {
    interact_path_dialog(ui, l, overlay, Kind::SaveAs)
}

/// 处理「导出选区」模态的输入（**只在 `Overlay::ExportSelection` 时调用**）。
pub fn interact_export_selection(ui: &mut Ui, l: &Layout, overlay: &mut Overlay) -> DialogAction {
    interact_path_dialog(ui, l, overlay, Kind::ExportSelection)
}

/// 处理「导出小地图」模态的输入（**只在 `Overlay::ExportMinimap` 时调用**）。
pub fn interact_export_minimap(ui: &mut Ui, l: &Layout, overlay: &mut Overlay) -> DialogAction {
    interact_path_dialog(ui, l, overlay, Kind::ExportMinimap)
}

/// 画快捷键浮层（内容在 [`crate::menu::shortcuts`]）。
pub fn draw_shortcuts(p: &Painter, l: &Layout) {
    crate::menu::shortcuts::draw(p, l);
}

/// 快捷键浮层里「关闭」的可点区域（整个浮层外都可点关闭，这里给出浮层矩形供判据用）。
pub fn shortcuts_rect(l: &Layout) -> Rect {
    crate::menu::shortcuts::rect(l)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn 布局() -> Layout {
        Layout::compute(Rect::from_min_size(
            pos2(0.0, 0.0),
            vec2(m::WINDOW_W, m::WINDOW_H),
        ))
    }

    /// 打开地图模态的几何自洽：整块在窗口内、输入框/按钮互不重叠、按钮在面板内。
    #[test]
    fn 打开模态几何自洽() {
        let l = 布局();
        let r = open_rects(&l);
        assert!(l.content.contains_rect(r.panel), "模态越出窗口");
        assert!(r.panel.contains_rect(r.field), "输入框越出面板");
        assert!(r.panel.contains_rect(r.ok), "打开按钮越出面板");
        assert!(r.panel.contains_rect(r.cancel), "取消按钮越出面板");
        assert!(!r.ok.intersects(r.cancel), "两个按钮重叠");
        assert!(!r.field.intersects(r.ok), "输入框压到按钮上");
        assert!(r.field.bottom() < r.ok.top(), "输入框在按钮之上");
    }

    /// **另存为模态与打开模态逐像素同几何**（同一套排版，只是文案不同）。
    ///
    /// 这条挡的是"另存为另写一套几何"——那样两个模态的输入框/按钮会一点点错位，
    /// 用户在两个几乎一样的框里点到不同的地方。
    #[test]
    fn 另存为模态与打开模态同几何() {
        let l = 布局();
        let a = open_rects(&l);
        let b = save_as_rects(&l);
        assert_eq!(a.panel, b.panel);
        assert_eq!(a.field, b.field);
        assert_eq!(a.ok, b.ok);
        assert_eq!(a.cancel, b.cancel);
        // 同一套判据也要过（另存为是同一个 `dialog_rects`，不许被特判放宽）
        assert!(l.content.contains_rect(b.panel));
        assert!(b.panel.contains_rect(b.field));
        assert!(!b.ok.intersects(b.cancel));
        assert!(b.field.bottom() < b.ok.top());
    }

    /// 两个模态的文案**各不相同**且都来自 `ui_text`（标题/提示/主按钮各一条）。
    #[test]
    fn 两个模态的文案各自独立() {
        assert_eq!(Kind::Open.title(), tx::OPEN_DLG_TITLE);
        assert_eq!(Kind::SaveAs.title(), tx::SAVE_AS_DLG_TITLE);
        assert_eq!(Kind::Open.hint(), tx::OPEN_DLG_HINT);
        assert_eq!(Kind::SaveAs.hint(), tx::SAVE_AS_DLG_HINT);
        assert_eq!(Kind::Open.ok_text(), tx::OPEN_DLG_OK);
        assert_eq!(Kind::SaveAs.ok_text(), tx::SAVE_AS_DLG_OK);
        // 标题与提示不许写成同一句（那会让用户分不清自己开的是哪个模态）
        assert_ne!(Kind::Open.title(), Kind::SaveAs.title());
        assert_ne!(Kind::Open.hint(), Kind::SaveAs.hint());
        // 另存为的提示必须点名"原文件不动"（这是它与「保存地图」的唯一差别）
        assert!(Kind::SaveAs.hint().contains("原文件不动"), "{}", Kind::SaveAs.hint());
        // 两个模态的 egui id 前缀必须不同（点击归属不许依赖绘制顺序）
        assert_ne!(Kind::Open.id_prefix(), Kind::SaveAs.id_prefix());
    }

    /// **提交动作按种类分开**：打开模态只会提交 `Open`，另存为只会提交 `SaveAs`。
    #[test]
    fn 提交动作按种类分开() {
        assert_eq!(
            Kind::Open.submit(r"I:\a.map"),
            DialogAction::Open(r"I:\a.map".to_owned())
        );
        assert_eq!(
            Kind::SaveAs.submit(r"I:\b.map"),
            DialogAction::SaveAs(r"I:\b.map".to_owned())
        );
        // 种类与浮层变体不匹配 ⇒ 什么都不做（不许把另存为的输入喂给打开动作）
        let mut ov = Overlay::OpenMap {
            path: "x".to_owned(),
        };
        assert!(path_mut(&mut ov, Kind::SaveAs).is_none());
        assert_eq!(path_mut(&mut ov, Kind::Open).map(|s| s.as_str()), Some("x"));
        let mut ov = Overlay::SaveAs {
            path: "y".to_owned(),
        };
        assert!(path_mut(&mut ov, Kind::Open).is_none());
        assert_eq!(path_mut(&mut ov, Kind::SaveAs).map(|s| s.as_str()), Some("y"));
        // 别的浮层（快捷键 / 无浮层）一律 `None`
        let mut ov = Overlay::Shortcuts;
        assert!(path_mut(&mut ov, Kind::Open).is_none());
        assert!(path_mut(&mut ov, Kind::SaveAs).is_none());
    }

    /// **四个模态的文案与 egui id 互不相同**（2026-11-05 加了两个导出模态）。
    ///
    /// 这条挡两件事：① 两个模态标题写成同一句 ⇒ 用户分不清自己开的是哪个；
    /// ② id 前缀撞了 ⇒ `interact` 的点击归属变成依赖绘制顺序（同一帧只有一个浮层，
    /// 但 id 撞了会在切换模态的那一帧串味）。
    #[test]
    fn 四个模态文案与id互不相同() {
        let kinds = [
            Kind::Open,
            Kind::SaveAs,
            Kind::ExportSelection,
            Kind::ExportMinimap,
        ];
        for i in 0..kinds.len() {
            assert!(!kinds[i].title().is_empty());
            assert!(!kinds[i].hint().is_empty());
            assert!(!kinds[i].ok_text().is_empty());
            for j in (i + 1)..kinds.len() {
                assert_ne!(kinds[i].title(), kinds[j].title(), "标题撞了：{:?}", kinds[i]);
                assert_ne!(kinds[i].hint(), kinds[j].hint(), "提示撞了：{:?}", kinds[i]);
                assert_ne!(
                    kinds[i].id_prefix(),
                    kinds[j].id_prefix(),
                    "id 前缀撞了：{:?}",
                    kinds[i]
                );
            }
        }
        // 两个导出的提示都要点名目标扩展名（它们是 PNG，不是 .map）
        assert!(Kind::ExportSelection.hint().contains(".png"));
        assert!(Kind::ExportMinimap.hint().contains(".png"));
        // 提交动作必须按种类分开（不许把导出的输入喂给另存为）
        assert_eq!(
            Kind::ExportSelection.submit(r"I:\a.png"),
            DialogAction::ExportSelection(r"I:\a.png".to_owned())
        );
        assert_eq!(
            Kind::ExportMinimap.submit(r"I:\b.png"),
            DialogAction::ExportMinimap(r"I:\b.png".to_owned())
        );
        let mut ov = Overlay::ExportSelection {
            path: "x.png".to_owned(),
        };
        assert!(path_mut(&mut ov, Kind::ExportMinimap).is_none());
        assert_eq!(path_mut(&mut ov, Kind::ExportSelection).map(|s| s.as_str()), Some("x.png"));
        let mut ov = Overlay::ExportMinimap {
            path: "y.png".to_owned(),
        };
        assert!(path_mut(&mut ov, Kind::ExportSelection).is_none());
        assert_eq!(path_mut(&mut ov, Kind::ExportMinimap).map(|s| s.as_str()), Some("y.png"));
    }

    /// 快捷键浮层也在窗口内，且**居中**。
    #[test]
    fn 快捷键浮层居中且在窗口内() {
        let l = 布局();
        let r = shortcuts_rect(&l);
        assert!(l.content.contains_rect(r));
        assert!((r.center().x - l.content.center().x).abs() < 0.01);
        assert!((r.center().y - l.content.center().y).abs() < 0.01);
    }
}
