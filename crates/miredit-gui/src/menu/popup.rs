//! 菜单**下拉弹层**的几何与绘制。
//!
//! # 几何从哪来
//!
//! * 横向：弹层左边缘 = 该菜单标签的矩形左边缘（`menubar::item_rects`），
//!   所以「视图(V)」的弹层正好从「视图(V)」左端挂下来（与 Windows 菜单一致）；
//! * 纵向：弹层顶边 = 菜单栏下沿（`Layout::menu_bar.bottom()`，不含那条 1px 分隔线）；
//! * 宽高：**量出来的**（文字宽 + 内边距 + 快捷键列），不是写死的 ——
//!   中文项名长短差别很大（`退出(X)` vs `实际大小（100%）(A)`），写死宽度必然有的挤有的空。
//!
//! 所有尺寸来自 [`crate::metrics`]（`MENU_POPUP_*`），所有颜色来自
//! [`crate::theme::Palette`]（`menu_popup_bg` / `menu_popup_border` / `menu_popup_hover`），
//! 所有文案来自 [`crate::ui_text`]。
//!
//! # 分隔线
//!
//! 位置**照官方**：`MainWindow.axaml:5` 里每个 `<Separator />` 落在哪两项之间，
//! 本模块就在那一项前面画一条（判据在 [`crate::menu::model::Item::sep_before`]）。
//!
//! # 子菜单（2026-11-05）
//!
//! 官方有 3 项是**可展开容器**（`文件 → 最近打开`、`视图 → 地图光照`、`视图 → 语言`），
//! 本仓 2026-11-05 之前把它们压成了叶子项（审查缺陷 1、2）。现在的行为：
//!
//! * 高亮到（鼠标 hover / 键盘选中）带子菜单的项时，在**父项右侧**挂一个子面板
//!   （[`SubGeom`]），父项右侧画一个**矢量**右指三角 —— 不用字符 `▸`：
//!   `Deng.ttf` 里 U+2713 都没字形（见 [`draw_check`] 的注释），箭头同样不赌字体；
//! * 子面板的**行数**由 [`crate::menu::model::Sub`] 决定：静态子项按表；
//!   动态列表（最近打开）按 [`crate::menu::model::SubState::recent_rows`]，
//!   空列表时就是官方 `RecentMaps.cs:39` 那条灰行；
//! * 子面板**不越出窗口**：右边放不下就翻到父项左侧（Avalonia/Windows 的行为），
//!   翻过去之后**左/上/下三条边同样夹**到窗口内（审查第 7 轮缺陷 6；窗口小到装不下时
//!   先把面板尺寸缩到窗口大小，保证矩形整块落在 `Layout::content` 里）。
//! * **子行的键盘焦点**（2026-11-05 追加）：焦点进了子菜单时
//!   （[`crate::menu::MenuState::sub_focus`] 可读 `(父项, 子行)`），**那一行铺高亮底色**
//!   —— 与父菜单里高亮项**同一套**视觉（`Palette::menu_popup_hover` + `text`/`text_muted`；
//!   父层高亮本来也不改文字色，所以子层沿用同源的两个色，没有新造颜色）。
//!   入口是 [`draw_with_focus`]；[`draw`] 是它的兼容包装（`focus = None` ⇒ 不画子行高亮）。
//!
//! ⚠ 父项的 `State::Todo` **不影响子面板展开**：父项本来就不是可执行动作（官方是容器），
//! 它灰着只是如实说明"这个功能本仓没实现"（「最近打开」2026-11-05 起已 `Ready`；
//! 「地图光照」/「语言」仍是 `Todo`，它们的子项各自带自己的三态）。
//!
//! ⚠ 动态行的点击（2026-11-05 起**真实现**）：列表来自设置文件的 `recent`
//! （[`crate::menu::settings`]，`[0]` = 最后打开的那张），行文案 = 官方
//! `Path.GetFileName`、可点性 = 官方 `File.Exists(path)`；点一行 / 回车走
//! [`crate::menu::Click::RunRecent`] / [`crate::menu::KeyResult::RunRecent`]（带 0 基下标）
//! ⇒ [`crate::menu::action::open_recent_index`] ⇒ 与「打开地图…」**同一个**
//! [`crate::menu::action::open_path`]。只有列表为空时才画官方 `RecentMaps.cs:39`
//! 那条灰行（`action == None` ⇒ 点了什么都不做）。

use egui::{pos2, vec2, Align2, Painter, Rect};

use crate::layout::Layout;
use crate::menu::model::{self, Action, Sub, SubState, MENUS};
use crate::menu::panels::PanelVis;
use crate::metrics as m;
use crate::paint as pt;
use crate::text_fit::Label;
use crate::theme as t;

/// 子菜单箭头的半高（**由字号推**：`metrics.rs` 不在本单写域，而箭头是纯装饰，
/// 取 `MENU_POPUP_FONT` 的 0.35 倍正好与文字同高）。
const ARROW_HALF_H: f32 = m::MENU_POPUP_FONT * 0.35;
/// 子菜单箭头的半宽（同上，0.28 倍 ≈ 一个半角字符的宽度）。
const ARROW_HALF_W: f32 = m::MENU_POPUP_FONT * 0.28;

/// 弹层里的一行（菜单项行或分隔线行）。
#[derive(Clone, Copy, Debug)]
pub struct Row {
    /// 菜单内下标（分隔线行等于它**后面那一项**的下标）。
    pub index: usize,
    /// 行矩形。
    pub rect: Rect,
    /// 是不是分隔线行。
    pub sep: bool,
}

/// 子面板里的一行。
#[derive(Clone, Debug)]
pub struct SubRow {
    /// 子项下标（静态子项 = 表内下标；动态行 = 列表下标；空态行 = 0）。
    pub index: usize,
    /// 行矩形。
    pub rect: Rect,
    /// 文案（静态子项取 `Action::label()`；动态行取文件名 / 空态文案）。
    pub label: String,
    /// 点它要执行的动作。
    ///
    /// `None` = 灰行（官方空态行 `IsEnabled=false`），点了什么都不做。
    pub action: Option<Action>,
    /// 能不能点：`None` = 静态子项（由 [`model::state`] 按有没有开图决定），
    /// `Some(v)` = 动态行（v 就是官方 `IsEnabled = File.Exists(path)` 的结果）。
    pub enabled: Option<bool>,
}

/// 一个展开中的子面板（挂在父项右侧）。
#[derive(Clone, Debug)]
pub struct SubGeom {
    /// 父项在菜单内的下标。
    pub parent: usize,
    /// 子面板矩形。
    pub rect: Rect,
    /// 各行（自上而下）。
    pub rows: Vec<SubRow>,
}

/// 一个弹层的几何（绘制与命中测试共用同一份）。
#[derive(Clone, Debug)]
pub struct Geom {
    /// 弹层整体矩形。
    pub rect: Rect,
    /// 各行（自上而下）。
    pub rows: Vec<Row>,
    /// 展开中的**子面板**（`None` = 当前高亮项没有子菜单 / 没有高亮）。
    pub sub: Option<SubGeom>,
}

/// 算一个弹层的几何（**不含子面板**：诊断/兼容入口，[`crate::menu::popup_rect`] 用它）。
pub fn geom(p: &Painter, l: &Layout, menu: usize) -> Geom {
    geom_with(p, l, menu, None, &[])
}

/// 算一个弹层的几何，并在 `active` 项带子菜单时**连带算子面板**。
///
/// `active` = 当前高亮项（鼠标 hover 优先，其次键盘）；`recent` = 最近地图列表
/// （只用于「最近打开」动态列表的行数与文案；空列表 ⇒ 官方空态行）。
pub fn geom_with(
    p: &Painter,
    l: &Layout,
    menu: usize,
    active: Option<usize>,
    recent: &[String],
) -> Geom {
    let mut g = base_geom(p, l, menu);
    if let (Some(idx), Some(menu_def)) = (active, MENUS.get(menu)) {
        if let Some(item) = menu_def.items.get(idx) {
            if let Some(sub) = item.sub {
                g.sub = Some(sub_geom(p, l, &g, idx, sub, recent));
            }
        }
    }
    g
}

/// 子面板的几何：挂在父项右侧（右边放不下就翻到左侧）。
fn sub_geom(
    p: &Painter,
    l: &Layout,
    g: &Geom,
    parent: usize,
    sub: Sub,
    recent: &[String],
) -> SubGeom {
    // 行：`(文案, 动作, 动态行的 enabled)`
    let mut rows_src: Vec<(String, Option<Action>, Option<bool>)> = Vec::new();
    match sub {
        Sub::Static(items) => {
            for it in items {
                rows_src.push((it.action.label().to_owned(), Some(it.action), None));
            }
        }
        Sub::Recent => {
            // 动态列表（2026-11-05 起真数据）：行数/文案由 `SubState::recent_rows` 给，
            // 真源 = 设置文件里的 `recent`（最近优先）；**空列表**才退化成官方
            // `RecentMaps.cs:39` 那条灰行（`text` = 空态文案、`on = false`）。
            let st = SubState {
                recent: recent.to_vec(),
                ..SubState::default()
            };
            let empty = st.recent.is_empty();
            for (text, on) in st.recent_rows() {
                rows_src.push((
                    text,
                    if empty { None } else { Some(Action::OpenRecent) },
                    Some(on),
                ));
            }
        }
    }
    // 宽度 = 最宽的那一行（勾选列 + 文字）+ 左右内边距（与父弹层同一套度量）
    let mut widest = m::MENU_POPUP_MIN_W;
    for (text, _, _) in &rows_src {
        let need = m::MENU_POPUP_PAD_X * 2.0
            + m::MENU_POPUP_CHECK_W
            + pt::text_w(p, text, m::MENU_POPUP_FONT);
        widest = widest.max(need);
    }
    let widest = widest.ceil();
    // 父项行：子面板顶边与它对齐
    let parent_top = g
        .rows
        .iter()
        .find(|r| !r.sep && r.index == parent)
        .map(|r| r.rect.top())
        .unwrap_or(g.rect.top());
    let height = m::MENU_POPUP_PAD_Y * 2.0 + rows_src.len() as f32 * m::MENU_POPUP_ROW_H;
    // 尺寸先夹进窗口（窗口比面板还窄/矮时才生效，正常 ≥1100×720 永不触发）：
    // 否则下面"夹位置"无从谈起 —— 位置区间 [content.left, content.right − 宽] 会反号。
    let size = vec2(
        widest.min(l.content.width()),
        height.min(l.content.height()),
    );
    // 右边放不下 ⇒ 翻到父弹层左侧（Avalonia/Windows 的子菜单行为）
    let mut left = g.rect.right();
    if left + size.x > l.content.right() {
        left = g.rect.left() - size.x;
    }
    // 四条边都夹（审查第 7 轮缺陷 6：旧版只夹右边，翻到左侧后可能压出窗口左边，
    // 纵向也可能挂到状态栏以下）。口径与右边一致：先 `max` 左/上界、再 `min` 右/下界。
    let left = left.max(l.content.left()).min(l.content.right() - size.x);
    let top = parent_top
        .max(l.content.top())
        .min(l.content.bottom() - size.y);
    let rect = Rect::from_min_size(pos2(left, top), size);

    let mut rows = Vec::with_capacity(rows_src.len());
    let mut y = rect.top() + m::MENU_POPUP_PAD_Y;
    for (i, (label, action, enabled)) in rows_src.into_iter().enumerate() {
        rows.push(SubRow {
            index: i,
            rect: Rect::from_min_size(
                pos2(rect.left() + m::MENU_POPUP_BORDER, y),
                vec2(
                    rect.width() - m::MENU_POPUP_BORDER * 2.0,
                    m::MENU_POPUP_ROW_H,
                ),
            ),
            label,
            action,
            enabled,
        });
        y += m::MENU_POPUP_ROW_H;
    }
    SubGeom {
        parent,
        rect,
        rows,
    }
}

/// 弹层底板与逐行几何（**不含**子面板）。
fn base_geom(p: &Painter, l: &Layout, menu: usize) -> Geom {
    let Some(menu_def) = MENUS.get(menu) else {
        return Geom {
            rect: Rect::NOTHING,
            rows: Vec::new(),
            sub: None,
        };
    };
    let label_rects = crate::menubar::item_rects(p, l);
    let left = label_rects
        .get(menu)
        .map(|r| r.left())
        .unwrap_or(l.menu_bar.left() + m::MENU_PAD_X);

    // 宽度 = 最宽的那一项（勾选列 + 文字 + 间距 + 快捷键）+ 左右内边距
    let mut widest = m::MENU_POPUP_MIN_W;
    let mut height = m::MENU_POPUP_PAD_Y * 2.0;
    for item in menu_def.items {
        let label_w = pt::text_w(p, item.action.label(), m::MENU_POPUP_FONT);
        let sc_w = item
            .action
            .shortcut()
            .map(|s| pt::text_w(p, s, m::MENU_POPUP_FONT))
            .unwrap_or(0.0);
        let need = m::MENU_POPUP_PAD_X * 2.0
            + m::MENU_POPUP_CHECK_W
            + label_w
            + if sc_w > 0.0 {
                m::MENU_POPUP_SHORTCUT_GAP + sc_w
            } else {
                0.0
            };
        widest = widest.max(need);
        height += if item.sep_before {
            m::MENU_POPUP_SEP_H + m::MENU_POPUP_ROW_H
        } else {
            m::MENU_POPUP_ROW_H
        };
    }
    let rect = Rect::from_min_size(
        pos2(left, l.menu_bar.bottom()),
        vec2(widest.ceil(), height),
    );

    // 逐行排：分隔线行高 `MENU_POPUP_SEP_H`，项行高 `MENU_POPUP_ROW_H`
    let mut rows = Vec::with_capacity(menu_def.items.len() + 3);
    let mut y = rect.top() + m::MENU_POPUP_PAD_Y;
    for (i, item) in menu_def.items.iter().enumerate() {
        if item.sep_before {
            rows.push(Row {
                index: i,
                rect: Rect::from_min_size(
                    pos2(rect.left() + m::MENU_POPUP_PAD_X, y),
                    vec2(rect.width() - m::MENU_POPUP_PAD_X * 2.0, m::MENU_POPUP_SEP_H),
                ),
                sep: true,
            });
            y += m::MENU_POPUP_SEP_H;
        }
        rows.push(Row {
            index: i,
            rect: Rect::from_min_size(
                pos2(rect.left() + m::MENU_POPUP_BORDER, y),
                vec2(
                    rect.width() - m::MENU_POPUP_BORDER * 2.0,
                    m::MENU_POPUP_ROW_H,
                ),
            ),
            sep: false,
        });
        y += m::MENU_POPUP_ROW_H;
    }
    Geom {
        rect,
        rows,
        sub: None,
    }
}

/// 画一个**矢量勾**（两段折线）。
///
/// 为什么不用文字 `✓`：系统字体 `Deng.ttf` 里 **U+2713 没有字形**
/// （`fonts::tests::文案全部字符都有字形` 实测缺字，会画成豆腐块）。
/// 矢量绘制与 `panel.rs` 的折叠箭头同一套做法，且不受字体版本影响。
fn draw_check(p: &Painter, c: egui::Pos2, color: egui::Color32) {
    let s = egui::Stroke::new(1.5_f32, color);
    p.line_segment(
        [pos2(c.x - 3.5, c.y), pos2(c.x - 1.0, c.y + 2.5)],
        s,
    );
    p.line_segment([pos2(c.x - 1.0, c.y + 2.5), pos2(c.x + 3.5, c.y - 2.5)], s);
}

/// 命中测试：返回**菜单内下标**（分隔线行不算命中）。
pub fn hit(g: &Geom, pos: egui::Pos2) -> Option<usize> {
    g.rows
        .iter()
        .find(|r| !r.sep && r.rect.contains(pos))
        .map(|r| r.index)
}

/// 子面板的命中测试：返回**子行下标**。
///
/// 不判 `enabled`：灰行也要能被点到（点了给"为什么不能点"的回执，与顶级灰显项同一套
/// 语义），只有 `action == None` 的空态行点了才什么都不做。
pub fn hit_sub(g: &Geom, pos: egui::Pos2) -> Option<usize> {
    let s = g.sub.as_ref()?;
    s.rows.iter().find(|r| r.rect.contains(pos)).map(|r| r.index)
}

/// 画一个弹层（含子面板）—— **[`draw_with_focus`] 的兼容包装**（`focus = None`）。
///
/// `active` = 当前高亮项（鼠标 hover 或键盘选中，菜单内下标）；`None` = 不高亮。
/// `grid` = 画布当前的单元格网格开关（勾选型项要画**真**勾，见 [`model::check_with`]）。
/// `doors` = 画布当前的**开启门**开关（同上，也是勾选型项的真状态）。
/// `sub` = 子菜单里"本仓没有真状态"的档位（语言 / 最近列表，官方缺省，见 [`SubState`]）。
///
/// ⚠ 这个包装按**缺省白天**画光照三档的勾；生产路径走 [`draw_with_focus`]
/// （`lighting` 来自 `app.lighting` 真源），本函数只留给"不关心档位"的调用方与测试。
///
/// ⚠ 这个版本**不画子行的键盘焦点**（没有 `focus` 参数）。要画请用 [`draw_with_focus`]：
/// 生产路径（`menu::draw_with`）走的就是它，`focus` 取 `MenuState::sub_focus()`。
#[allow(clippy::too_many_arguments)] // 10 个参数都是"这一帧的输入"，打包成结构体反而更绕
pub fn draw(
    p: &Painter,
    l: &Layout,
    menu: usize,
    active: Option<usize>,
    has_map: bool,
    dark: bool,
    grid: bool,
    doors: bool,
    panels: &PanelVis,
    sub: &SubState,
) {
    draw_with_focus(
        p,
        l,
        menu,
        active,
        None,
        has_map,
        dark,
        grid,
        doors,
        model::MapLight::Day,
        panels,
        sub,
    );
}

/// 画一个弹层（含子面板），并给**子菜单里的当前行**铺高亮。
///
/// `focus` = [`crate::menu::MenuState::sub_focus`] 的原文 `(父项下标, 子行下标)`；
/// `None` = 焦点在父弹层（子面板照画，只是没有子行高亮）。
///
/// 子行高亮与父项高亮**同一套视觉**（底色 `Palette::menu_popup_hover`、文字色
/// `text`/`text_muted`）：父层高亮本来就不改文字色，子层照同一口径 ⇒ 两处看起来是
/// "同一个光标换了个位置"，而不是两套风格。
#[allow(clippy::too_many_arguments)] // 12 个参数都是"这一帧的输入"，与 `draw` 同口径
pub fn draw_with_focus(
    p: &Painter,
    l: &Layout,
    menu: usize,
    active: Option<usize>,
    focus: Option<(usize, usize)>,
    has_map: bool,
    dark: bool,
    grid: bool,
    doors: bool,
    lighting: model::MapLight,
    panels: &PanelVis,
    sub: &SubState,
) {
    let Some(menu_def) = MENUS.get(menu) else {
        return;
    };
    let pal = t::ui();
    let g = geom_with(p, l, menu, active, &sub.recent);
    if !g.rect.is_positive() {
        return;
    }
    // 底板 + 1px 边框（先描边再填，避免边框被底色盖掉一半）
    pt::fill_round(p, g.rect, m::MENU_POPUP_RADIUS, pal.menu_popup_bg);
    pt::border(p, g.rect, pal.menu_popup_border);

    for row in &g.rows {
        if row.sep {
            // 分隔线画在行的垂直中线（上下各留一半呼吸）
            pt::fill(
                p,
                Rect::from_min_size(
                    pos2(row.rect.left(), row.rect.center().y.floor()),
                    vec2(row.rect.width(), 1.0),
                ),
                pal.menu_popup_border,
            );
            continue;
        }
        let Some(item) = menu_def.items.get(row.index) else {
            continue;
        };
        let st = model::state(item.action);
        let enabled = st.enabled(has_map);
        // 高亮：**带子菜单的项即使灰显也要高亮** —— 它的子面板正挂在右边，
        // 不高亮就看不出来"这块面板是谁开出来的"（父项本身是容器、不是可执行动作，
        // 所以"灰显"与"当前项"并不冲突）。
        let hovered = active == Some(row.index) && (enabled || item.sub.is_some());
        if hovered {
            pt::fill(p, row.rect, pal.menu_popup_hover);
        }
        let color = if enabled { pal.text } else { pal.text_muted };

        // 勾选列（只有勾选型菜单项才画勾；勾的是**真状态** —— 网格关着就不画）
        let check = model::check_with(item.action, dark, grid, doors, lighting, panels, sub);
        let check_x = row.rect.left() + m::MENU_POPUP_PAD_X + m::MENU_POPUP_CHECK_W * 0.5;
        if check == Some(true) {
            draw_check(p, egui::pos2(check_x, row.rect.center().y), color);
        }

        // 有子菜单的项：右侧留出箭头的位置（文案裁剪盒跟着收窄，别压到箭头上）
        let has_sub = item.sub.is_some();
        let text_right = if has_sub {
            row.rect.right() - m::MENU_POPUP_PAD_X - ARROW_HALF_W * 2.0 - 4.0
        } else {
            row.rect.right() - m::MENU_POPUP_PAD_X
        };

        // 项名
        let label_x = row.rect.left() + m::MENU_POPUP_PAD_X + m::MENU_POPUP_CHECK_W;
        Label {
            name: "menu.popup.label",
            text: item.action.label(),
            font: m::MENU_POPUP_FONT,
            pos: pos2(label_x, row.rect.center().y),
            anchor: Align2::LEFT_CENTER,
            clip: Rect::from_min_max(
                pos2(label_x, row.rect.top()),
                pos2(text_right, row.rect.bottom()),
            ),
            color,
        }
        .draw(p);

        // 右侧快捷键（官方 `InputGesture` 原文；没有就不画）
        if let Some(sc) = item.action.shortcut() {
            pt::text(
                p,
                pos2(text_right, row.rect.center().y),
                Align2::RIGHT_CENTER,
                sc,
                m::MENU_POPUP_FONT,
                pal.text_muted,
            );
        }

        // 子菜单箭头：**矢量三角**（不赌字体有 `▸` 的字形，理由同 [`draw_check`]）
        if has_sub {
            let cx = row.rect.right() - m::MENU_POPUP_PAD_X - ARROW_HALF_W;
            let cy = row.rect.center().y;
            p.add(egui::Shape::convex_polygon(
                vec![
                    pos2(cx - ARROW_HALF_W * 0.5, cy - ARROW_HALF_H),
                    pos2(cx + ARROW_HALF_W * 0.5, cy),
                    pos2(cx - ARROW_HALF_W * 0.5, cy + ARROW_HALF_H),
                ],
                color,
                egui::Stroke::NONE,
            ));
        }
    }

    // 子面板（当前高亮项带子菜单时才存在）
    if let Some(sg) = &g.sub {
        draw_sub(
            p,
            sg,
            SubCtx {
                focus,
                has_map,
                dark,
                grid,
                doors,
                lighting,
                panels,
                sub,
            },
            pal,
        );
    }
}

/// 画子面板时要的**这一帧的输入**（把 7 个参数打包）。
///
/// 为什么打包：`draw_sub` 原本吃 11 个参数（`p`/`sg`/`pal` + 这 8 项），超 clippy
/// `too_many_arguments` 的 7 参阈值。这 8 项在每一帧都**成套出现**（父弹层
/// [`draw_with_focus`] 拿到的就是它们），其中 7 项又正是 [`model::check_with`] 的入参
/// ⇒ 打包之后"画子面板要看哪些状态"变成一个有名字的东西，比逐个平铺更清楚。
///
/// 本结构体是 `Copy`：里面只有 `bool` / `Option<(usize, usize)>` 与两个共享引用，
/// 所以 [`draw_sub`] 可以按值收、再解构回同名的局部变量（函数体因此一行不用改）。
#[derive(Clone, Copy)]
struct SubCtx<'a> {
    /// 子行的键盘焦点 `(父项下标, 子行下标)`；`None` = 焦点在父弹层。
    focus: Option<(usize, usize)>,
    /// 当前有没有打开地图（决定灰显）。
    has_map: bool,
    /// 深色模式开关（勾选型项的真状态）。
    dark: bool,
    /// 单元格网格开关。
    grid: bool,
    /// 开启门开关。
    doors: bool,
    /// 地图光照档位（宿主真源，三档互斥的勾由它决定）。
    lighting: model::MapLight,
    /// 8 个面板的可见性。
    panels: &'a PanelVis,
    /// 子菜单那几项"本仓还没有真状态"的档位（语言 / 最近列表）。
    sub: &'a SubState,
}

/// 画子面板（底板 + 边框 + 各行 + **当前行高亮**）。几何来自 [`sub_geom`]，这里只负责上色与文字。
fn draw_sub(p: &Painter, sg: &SubGeom, ctx: SubCtx<'_>, pal: &t::Palette) {
    // 解构回局部变量：函数体与"10 个参数平铺"那版逐字相同，只是入口换了包装
    let SubCtx {
        focus,
        has_map,
        dark,
        grid,
        doors,
        lighting,
        panels,
        sub,
    } = ctx;
    // 底板：与父弹层同一套颜色/圆角/边框
    pt::fill_round(p, sg.rect, m::MENU_POPUP_RADIUS, pal.menu_popup_bg);
    pt::border(p, sg.rect, pal.menu_popup_border);

    for row in &sg.rows {
        // 能不能点：动态行看官方 `File.Exists`，静态子项看它自己的三态
        let enabled = match (row.action, row.enabled) {
            (None, _) => false,
            (Some(a), Some(dyn_on)) => model::state(a).enabled(has_map) && dyn_on,
            (Some(a), None) => model::state(a).enabled(has_map),
        };
        // 勾选态：静态子项问 `check_with`（光照 3 项 / 语言 2 项都是勾选型）；
        // 动态行不是勾选型 ⇒ `None` ⇒ 不画勾。
        let check = row
            .action
            .and_then(|a| model::check_with(a, dark, grid, doors, lighting, panels, sub));
        let color = if enabled { pal.text } else { pal.text_muted };

        // **子行的键盘焦点**：`focus == Some((父项, 子行))` ⇒ 这一行铺高亮底色。
        //
        // 与父层 `hovered` 有**一处有意差别**：这里**不按 `enabled` 过滤**。
        // 理由：`↑↓` 在子菜单里会停在灰行（`MenuState::move_sub` 不过滤），
        // 灰行若不亮，用户就"看不见光标停在哪"—— 那正是本函数要修的缺口。
        // 颜色与父菜单高亮项同源（`menu_popup_hover`），没有新造色值。
        if focus == Some((sg.parent, row.index)) {
            pt::fill(p, row.rect, pal.menu_popup_hover);
        }

        let check_x = row.rect.left() + m::MENU_POPUP_PAD_X + m::MENU_POPUP_CHECK_W * 0.5;
        if check == Some(true) {
            draw_check(p, egui::pos2(check_x, row.rect.center().y), color);
        }
        let label_x = row.rect.left() + m::MENU_POPUP_PAD_X + m::MENU_POPUP_CHECK_W;
        // 子项文案是 `String`（动态行来自最近地图列表）⇒ 直接走 `paint::text` + 裁剪盒
        // （与 `Label::draw` 同一套做法）。不用 `Label`：它只吃 `&'static str`；
        // 也不用 `DynLabel`：它要求 `tmpl` 必须是 `ui_text::ALL` 里的模板，而子项文案
        // 住在 `menu::model`（光照 3 项 / 语言 2 项的 label 由 `Action::label()` 给），
        // 不满足那条纪律。
        let clip = Rect::from_min_max(
            pos2(label_x, row.rect.top()),
            pos2(row.rect.right() - m::MENU_POPUP_PAD_X, row.rect.bottom()),
        );
        pt::text(
            &p.with_clip_rect(clip),
            pos2(label_x, row.rect.center().y),
            Align2::LEFT_CENTER,
            &row.label,
            m::MENU_POPUP_FONT,
            color,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fonts;
    use crate::menu::{handle_key_with, KeyResult, MenuKey, MenuState};

    fn 画布() -> (egui::Context, Painter, Layout) {
        let ctx = fonts::measure_ctx();
        let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(m::WINDOW_W, m::WINDOW_H));
        let p = Painter::new(ctx.clone(), egui::LayerId::debug(), screen);
        let l = Layout::compute(screen);
        (ctx, p, l)
    }

    /// 弹层几何自洽：行高/分隔线高与常量一致、总高 = 内边距 + 各行之和、
    /// 弹层顶边贴菜单栏下沿、左边缘对齐菜单标签。
    #[test]
    fn 弹层几何自洽() {
        let (_ctx, p, l) = 画布();
        let labels = crate::menubar::item_rects(&p, &l);
        for (mi, menu) in MENUS.iter().enumerate() {
            let g = geom(&p, &l, mi);
            assert!((g.rect.top() - l.menu_bar.bottom()).abs() < 0.01, "弹层没贴在菜单栏下沿");
            assert!((g.rect.left() - labels[mi].left()).abs() < 0.01, "弹层没对齐菜单标签");
            assert!(g.rect.width() >= m::MENU_POPUP_MIN_W);
            let mut sum = m::MENU_POPUP_PAD_Y * 2.0;
            for item in menu.items {
                sum += m::MENU_POPUP_ROW_H;
                if item.sep_before {
                    sum += m::MENU_POPUP_SEP_H;
                }
            }
            assert!(
                (g.rect.height() - sum).abs() < 0.01,
                "{:?} 弹层高 {} 与逐行累加 {sum} 不符",
                menu.title,
                g.rect.height()
            );
            // 项行数 = 菜单项数（分隔线行另算）
            assert_eq!(g.rows.iter().filter(|r| !r.sep).count(), menu.items.len());
            assert_eq!(
                g.rows.iter().filter(|r| r.sep).count(),
                menu.items.iter().filter(|i| i.sep_before).count()
            );
        }
    }

    /// 命中测试：每个项行的中心点必须命中它自己；分隔线行不命中。
    #[test]
    fn 命中测试逐行() {
        let (_ctx, p, l) = 画布();
        for mi in 0..MENUS.len() {
            let g = geom(&p, &l, mi);
            for row in &g.rows {
                let hit_idx = hit(&g, row.rect.center());
                if row.sep {
                    // 分隔线行的中心落在"上一项行"与"下一项行"之间 ⇒ 不该命中任何项
                    assert!(hit_idx.is_none(), "分隔线行不该命中：{hit_idx:?}");
                } else {
                    assert_eq!(hit_idx, Some(row.index), "第 {} 行没命中自己", row.index);
                }
            }
            // 弹层外面不命中
            assert!(hit(&g, pos2(g.rect.right() + 5.0, g.rect.top() + 2.0)).is_none());
        }
    }

    /// 弹层不得超出窗口右边界（视图菜单最靠右的项名很长）。
    #[test]
    fn 弹层不越出窗口右边() {
        let (_ctx, p, l) = 画布();
        for mi in 0..MENUS.len() {
            let g = geom(&p, &l, mi);
            assert!(
                g.rect.right() <= l.content.right() + 0.01,
                "第 {mi} 个弹层右边 {} 越出窗口 {}",
                g.rect.right(),
                l.content.right()
            );
        }
    }

    /// **极小窗口下子面板整块落在窗口内**（审查第 7 轮缺陷 6 的回归用例）。
    ///
    /// 320×240 是审查给的例子，它**真的会打中两条旧缺口**，不是"当前尺寸下不可达"的空跑：
    /// * 纵向：「视图 → 语言」的父项行顶 = 菜单栏下沿 66 + 内边距 4 + 前 8 项（含 3 条分隔线）
    ///   ≈ 298 > 240 —— 父项本身已经在窗口外，子面板若照旧贴 `parent_top` 就会挂到 240 以下；
    /// * 横向：窗口窄到 240 时，「帮助」弹层翻左后 `left = 弹层左 − 子面板宽` 会变成负数。
    ///
    /// ⇒ 两种尺寸都要求子面板矩形（以及每一行）落在 `l.content` 内。
    #[test]
    fn 极小窗口下子面板不越界() {
        let ctx = fonts::measure_ctx();
        for (w, h) in [(320.0_f32, 240.0_f32), (240.0, 200.0)] {
            let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(w, h));
            let p = Painter::new(ctx.clone(), egui::LayerId::debug(), screen);
            let l = Layout::compute(screen);
            let mut seen = 0;
            for (mi, menu) in MENUS.iter().enumerate() {
                for (ii, item) in menu.items.iter().enumerate() {
                    if item.sub.is_none() {
                        continue;
                    }
                    let g = geom_with(&p, &l, mi, Some(ii), &[]);
                    let sg = g.sub.as_ref().expect("带子菜单的项必须展开子面板");
                    seen += 1;
                    assert!(
                        l.content.contains_rect(sg.rect),
                        "{w}×{h} 第 {mi} 个菜单第 {ii} 项：子面板 {:?} 越出窗口 {:?}",
                        sg.rect,
                        l.content
                    );
                    // 行矩形是命中判据：跑到窗口外就等于"看得见点不到"
                    for row in &sg.rows {
                        assert!(
                            l.content.contains_rect(row.rect),
                            "{w}×{h} 第 {mi} 个菜单第 {ii} 项的子行 {} 越界：{:?}",
                            row.index,
                            row.rect
                        );
                    }
                }
            }
            assert_eq!(seen, 3, "三个子菜单容器（最近打开/地图光照/语言）都要量到");
        }
    }

    /// **子行的键盘焦点在画面上真被画出来**（本轮 ② 的回归用例）。
    ///
    /// 判据（用户原话）：`sub_focus = Some((2, 1))` 时，第 2 个菜单（视图）的子面板
    /// **第 1 行**的绘制矩形里有高亮图元，**其余行没有**。
    ///
    /// 两处刻意"不手搓"：
    /// * `sub_focus` 由**真键盘路径**产生 —— `handle_key_with(Right)` 进子菜单、
    ///   `handle_key_with(Down)` 下一次（`sub_focus` 是私有字段，本来也只能这么造；
    ///   这样测的是"用户按 → 再按 ↓ 之后屏幕上长什么样"，不是"我能不能写个字段"）；
    /// * 高亮图元从 `ctx.run` 收到的 `out.shapes` 里**量**（与 `grid::tests` 同一套做法），
    ///   不问 `draw_sub`"你会不会画"。
    #[test]
    fn 子行焦点高亮只画在那一行() {
        let ctx = fonts::measure_ctx();
        let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(m::WINDOW_W, m::WINDOW_H));
        let l = Layout::compute(screen);
        let sub_state = SubState::default();

        // ① 造状态：视图(菜单 2) → 第 3 项「地图光照」(项 2) → `→` 进子菜单 → `↓` 到第 1 行
        let mut s = MenuState::default();
        s.open(2);
        assert!(s.focus(2), "视图菜单第 3 项（地图光照）应能设成高亮项");
        assert_eq!(
            handle_key_with(&mut s, MenuKey::Right, &sub_state),
            KeyResult::Changed
        );
        assert_eq!(s.sub_focus(), Some((2, 0)), "→ 应把焦点送进子菜单第 0 行");
        assert_eq!(
            handle_key_with(&mut s, MenuKey::Down, &sub_state),
            KeyResult::Changed
        );
        assert_eq!(s.sub_focus(), Some((2, 1)), "↓ 应落到第 1 行");

        // ② 几何：子面板的行矩形就是绘制/命中矩形（同一份 `sub_geom`）
        let probe = Painter::new(ctx.clone(), egui::LayerId::debug(), screen);
        let g = geom_with(&probe, &l, 2, s.active(), &[]);
        let sg = g.sub.as_ref().expect("地图光照的子面板");
        assert_eq!(sg.rows.len(), 3, "地图光照有 3 个子项（黎明/白天/夜晚）");
        assert_eq!(sg.parent, 2, "子面板的父项就是地图光照那一项");

        // ③ 真跑一次绘制，收图元
        let out = ctx.run(egui::RawInput::default(), |ctx| {
            let p = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Background,
                egui::Id::new("menu-sub-focus"),
            ));
            draw_with_focus(
                &p,
                &l,
                2,
                s.active(),
                s.sub_focus(),
                true,
                false,
                true,
                false,
                model::MapLight::Day,
                &PanelVis::default(),
                &sub_state,
            );
        });

        // 高亮色 = `menu_popup_hover`。测试并行跑、`theme::set_dark` 是全局开关
        // ⇒ 两套色板的同一个字段**都认**（不赌此刻是哪套）。
        let hover = [t::LIGHT.menu_popup_hover, t::DARK.menu_popup_hover];
        let 该矩形有高亮 = |r: Rect| -> bool {
            out.shapes.iter().any(|cs| match &cs.shape {
                egui::Shape::Rect(rect) => {
                    hover.contains(&rect.fill)
                        && (rect.rect.left() - r.left()).abs() < 0.5
                        && (rect.rect.top() - r.top()).abs() < 0.5
                        && (rect.rect.width() - r.width()).abs() < 0.5
                        && (rect.rect.height() - r.height()).abs() < 0.5
                }
                _ => false,
            })
        };
        for (i, row) in sg.rows.iter().enumerate() {
            let got = 该矩形有高亮(row.rect);
            if i == 1 {
                assert!(got, "第 1 行必须有高亮图元（矩形 {:?}）", row.rect);
            } else {
                assert!(!got, "第 {i} 行不该有高亮图元（矩形 {:?}）", row.rect);
            }
        }
        // 反向对照：同一帧里**父项**（地图光照那行）也有 hover 底色 ⇒
        // "其余行没有"不是因为整帧没画这个颜色，而是确实只给了第 1 行。
        let parent_row = g
            .rows
            .iter()
            .find(|r| !r.sep && r.index == 2)
            .expect("父项行");
        assert!(
            该矩形有高亮(parent_row.rect),
            "父项（地图光照）本身也该有高亮底色（它是 active 项）"
        );
    }

    /// **没有焦点就没有子行高亮**（兼容入口 `draw` 的口径）：同一帧换成 `focus = None`
    /// ⇒ 子面板照画（底板/文字都在），但一行高亮都不该有。
    ///
    /// 这条挡的是"把高亮做成恒亮"——那样键盘焦点在父层时，子面板第一行也会发亮，
    /// 用户会以为光标已经在子菜单里了。
    #[test]
    fn 无焦点时子面板不画高亮() {
        let ctx = fonts::measure_ctx();
        let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(m::WINDOW_W, m::WINDOW_H));
        let l = Layout::compute(screen);
        let sub_state = SubState::default();

        let mut s = MenuState::default();
        s.open(2);
        assert!(s.focus(2));
        assert_eq!(s.sub_focus(), None, "只设了高亮项，焦点还在父层");

        let probe = Painter::new(ctx.clone(), egui::LayerId::debug(), screen);
        let g = geom_with(&probe, &l, 2, s.active(), &[]);
        let sg = g.sub.as_ref().expect("子面板照画");

        let out = ctx.run(egui::RawInput::default(), |ctx| {
            let p = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Background,
                egui::Id::new("menu-sub-nofocus"),
            ));
            // 兼容入口：10 参、没有 focus
            draw(
                &p,
                &l,
                2,
                s.active(),
                true,
                false,
                true,
                false,
                &PanelVis::default(),
                &sub_state,
            );
        });
        let hover = [t::LIGHT.menu_popup_hover, t::DARK.menu_popup_hover];
        for row in &sg.rows {
            let got = out.shapes.iter().any(|cs| match &cs.shape {
                egui::Shape::Rect(rect) => {
                    hover.contains(&rect.fill)
                        && (rect.rect.left() - row.rect.left()).abs() < 0.5
                        && (rect.rect.top() - row.rect.top()).abs() < 0.5
                        && (rect.rect.width() - row.rect.width()).abs() < 0.5
                }
                _ => false,
            });
            assert!(!got, "焦点在父层时子行 {} 不该有高亮", row.index);
        }
    }
}
