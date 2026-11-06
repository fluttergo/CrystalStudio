//! **完整菜单系统**（5 个菜单 / 40 项）+ 弹层 / 浮层状态机。
//!
//! # 模块划分
//!
//! | 模块 | 职责 |
//! |---|---|
//! | [`model`] | **静态表**：5 菜单 40 项、官方分组、官方快捷键、三态判据 |
//! | [`action`] | **唯一执行入口**：鼠标 / 键盘 / `--menu-click` 三条路径共用 |
//! | [`popup`] | 弹层几何与绘制（量出来的宽高，颜色/尺寸全走 `theme` / `metrics`）|
//! | [`panels`] | 7 个面板的显隐状态（视图菜单那 7 个开关）|
//! | [`settings`] | `%LOCALAPPDATA%\miredit\settings.json`（深色偏好 + **最近打开的地图列表**）|
//! | [`shortcuts`] | 帮助 → 键盘快捷键：**只列真实绑定**的只读浮层 |
//! | [`dialog`] | 打开地图模态 + 另存为模态 + 快捷键浮层的绘制与输入 |
//!
//! # 交互契约
//!
//! ```text
//! 点菜单标签        → 展开该菜单（再点一次收起；展开时点别的标签直接切换）
//! 点弹层里的项      → 关闭弹层 + 执行（灰显项也执行"拒绝"这条路径，状态栏给原因）
//! 点带子菜单的项    → **只展开/收起子菜单，不执行父项动作**（官方那 3 项是容器）
//! 点别处            → 收起
//! Esc               → 焦点在子菜单里 ⇒ 先收子菜单（**逐级**）；否则收起弹层 / 关闭浮层
//! ↑ ↓               → 在项之间移动（**灰显项也停**，这样键盘也能问"为什么不能点"）；
//!                     焦点在子菜单里时改为在**子项**之间移动
//! →                 → 当前项带子菜单 ⇒ 进子菜单；否则切到下一个菜单
//! ←                 → 焦点在子菜单里 ⇒ 回父项；否则切到上一个菜单
//! Enter             → 焦点在子菜单里 ⇒ 执行该子项；父弹层里：**带子菜单的项只展开/收起**，
//!                     叶子项才执行
//! F1                → 打开快捷键浮层
//! ```
//!
//! ⚠ **`Enter` 与鼠标点同一行必须同语义**（审查缺陷 3）：官方那 3 个容器项
//! （`file.open_recent` / `view.map_lighting` / `view.language`）在 axaml 里**没有 `Click`**，
//! 它们不是动作 —— 所以两条输入路径都**不许**执行父项的 `Action`，
//! 只能改"子菜单展开/焦点"这件事（[`MenuState::sub_focus`]）。
//!
//! ⚠ 「点别处收起」的判据是 `pointer.any_click()` 且点击点**不在**弹层/菜单栏内 ——
//! 不用 egui 的 `Area`/`Window`：本界面是**纯 Painter 自绘**（见 `app.rs` 的绘制顺序），
//! 弹层必须自己排在菜单栏之后画，才能压住面板与画布。

pub mod action;
pub mod dialog;
pub mod model;
pub mod panels;
pub mod popup;
pub mod settings;
pub mod shortcuts;

use egui::{Rect, Ui};

use crate::layout::Layout;
use crate::menu::model::{Action, Sub, SubState, MENUS};
use crate::menu::panels::PanelVis;
use crate::ui_text as tx;

/// 当前浮层（同一时刻只有一个）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Overlay {
    /// 没有浮层。
    None,
    /// 「文件 → 打开地图…」的路径输入模态。
    OpenMap {
        /// 输入框里的路径（初值 = 当前地图路径）。
        path: String,
    },
    /// 「文件 → 另存为…」的路径输入模态（2026-11-05 接线）。
    ///
    /// 与 [`Overlay::OpenMap`] **同一套几何与输入**（见 [`dialog`]），初值 = 当前地图路径
    /// （用户改文件名最省事）；回车后走 [`crate::save::save_as_session`]，
    /// **存到新路径并把文档路径改成它**，原文件一字节不动。
    SaveAs {
        /// 输入框里的目标路径（初值 = 当前地图路径）。
        path: String,
    },
    /// 「文件 → 导出选区(E)…」的路径输入模态（2026-11-05 接线）。
    ///
    /// 与 [`Overlay::SaveAs`] **同一套几何与输入**，只是文案与提交动作不同；
    /// 回车后走 [`crate::export::export_selection`]（`scale = 1`，三层全开）。
    ExportSelection {
        /// 输入框里的目标 `.png` 路径（初值 = 地图同目录的 `<地图名>_选区.png`）。
        path: String,
    },
    /// 「文件 → 导出小地图(M)…」的路径输入模态（2026-11-05 接线）。
    ///
    /// @acfbbf8a 回源订正（2026-11-06）：回车后走
    /// [`crate::export::export_minimap_sized`] + [`crate::export::MinimapSize::Default`]
    /// （官方缺省 `W*12 × H*8`），档位映射见 [`crate::export::pick_official_scale`]。
    ExportMinimap {
        /// 输入框里的目标 `.png` 路径（初值 = 地图同目录的 `<地图名>_小地图.png`）。
        path: String,
    },
    /// 「帮助 → 键盘快捷键…」的只读浮层。
    Shortcuts,
}

impl Overlay {
    /// 是否有浮层（有 ⇒ 弹层与画布交互要让位）。
    pub fn is_some(&self) -> bool {
        !matches!(self, Overlay::None)
    }
}

/// 菜单栏 + 弹层的交互状态。
#[derive(Clone, Debug, Default)]
pub struct MenuState {
    /// 当前展开的菜单下标（`None` = 全部收起）。
    open: Option<usize>,
    /// 弹层里鼠标 hover 的项（菜单内下标）。
    hover: Option<usize>,
    /// 弹层里键盘选中的项（菜单内下标）。
    kb: Option<usize>,
    /// 鼠标悬停在哪个菜单标签上（`None` = 不在菜单栏上）。
    ///
    /// 收起态也要有 hover 反馈：菜单栏上那 5 个字是唯一"可以点"的提示，
    /// 鼠标划过去一点反应都没有会让人以为菜单栏是死的（上一版就是这样）。
    hover_label: Option<usize>,
    /// 键盘焦点在子菜单里时：`(父项下标, 子行下标)`；`None` = 焦点在父弹层。
    ///
    /// # 为什么需要它（审查缺陷 3 的修法）
    ///
    /// `popup.rs` 的子面板只按「父项是不是高亮项」决定**画不画**，没有"子行高亮"
    /// 这个概念（那个文件不在本单写域）⇒ "键盘到底进没进子菜单"只能由本结构记着。
    /// 没有它：`↑↓` 不知道该动父项还是子项、`Enter` 不知道该执行谁、`Esc` 不知道该收哪一级。
    ///
    /// ⚠ 已知显示层缺口：子行的键盘选中**在画面上没有高亮**（同上，要 `popup.rs` 支持）。
    /// 状态与执行是对的（`sub_focus()` 可读、`--menu-dump`/单元测试可验），
    /// 只是眼下"看不见光标停在哪一行"—— 待 `popup.rs` 归属方补 `active_row`。
    sub_focus: Option<(usize, usize)>,
}

impl MenuState {
    /// 当前展开的菜单（供绘制判断）。
    pub fn open_index(&self) -> Option<usize> {
        self.open
    }

    /// 键盘焦点在子菜单里吗？`Some((父项下标, 子行下标))`。
    ///
    /// 给绘制/测试/宿主读的**只读**出口（写只走 [`MenuKey`] 那条路，不许外面直接改）。
    pub fn sub_focus(&self) -> Option<(usize, usize)> {
        self.sub_focus
    }

    /// 展开第 `i` 个菜单（越界忽略）。
    pub fn open(&mut self, i: usize) {
        if i < MENUS.len() {
            self.open = Some(i);
            self.hover = None;
            self.kb = None;
            self.sub_focus = None;
        }
    }

    /// 把当前展开菜单里的第 `idx` 项设为**键盘选中项**（= 高亮项 ⇒ 带子菜单的项会展开子面板）。
    ///
    /// 返回是否真的设上了（`false` = 没有展开的菜单，或 `idx` 越界）。
    ///
    /// # 为什么需要它
    ///
    /// 子菜单只有在某项**成为高亮项**时才展开，而高亮项来自鼠标 hover 或 ↑↓ 键。
    /// `--shot` 运行会锁死输入（`app::input_locked`）⇒ 没有 hover；也没有命令行能按键
    /// ⇒ 子面板在截图里永远看不到。`--menu-hover <下标>` 就是补这个洞的取证入口
    /// （只在同时给了 `--shot` / `--menu-open` 时生效，见 `app.rs`）。
    ///
    /// 正常交互不走这里：`kb` 由 ↑↓ 维护、`hover` 由鼠标维护。
    pub fn focus(&mut self, idx: usize) -> bool {
        let Some(mi) = self.open else { return false };
        let Some(menu) = MENUS.get(mi) else {
            return false;
        };
        if idx >= menu.items.len() {
            return false;
        }
        self.kb = Some(idx);
        self.hover = None;
        self.sub_focus = None;
        true
    }

    /// 收起。
    pub fn close(&mut self) {
        self.open = None;
        self.hover = None;
        self.kb = None;
        self.sub_focus = None;
    }

    /// 鼠标是否停在某个菜单标签上（绘制用）。
    pub fn hover_label(&self) -> Option<usize> {
        self.hover_label
    }

    /// 点标签：已展开的同一个菜单则收起，否则展开它。
    pub fn toggle(&mut self, i: usize) {
        if self.open == Some(i) {
            self.close();
        } else {
            self.open(i);
        }
    }

    /// 当前高亮项（鼠标优先，其次键盘）。
    pub fn active(&self) -> Option<usize> {
        self.hover.or(self.kb)
    }

    /// 键盘上下移动（`delta` = ±1；**灰显项也停**，见模块文档）。
    fn move_kb(&mut self, delta: i32) {
        let Some(mi) = self.open else { return };
        let Some(menu) = MENUS.get(mi) else { return };
        let n = menu.items.len() as i32;
        if n == 0 {
            return;
        }
        let cur = self.kb.map(|k| k as i32).unwrap_or(if delta > 0 { -1 } else { 0 });
        let next = (cur + delta).clamp(0, n - 1);
        self.kb = Some(next as usize);
        self.hover = None;
        // 换了父项 ⇒ 焦点离开子菜单（子面板的内容跟着父项变，旧行号已经无意义）
        self.sub_focus = None;
    }

    /// 键盘左右切换菜单（展开态）。
    fn move_menu(&mut self, delta: i32) {
        let Some(mi) = self.open else { return };
        let n = MENUS.len() as i32;
        let next = ((mi as i32 + delta).rem_euclid(n)) as usize;
        self.open = Some(next);
        self.hover = None;
        self.kb = None;
        self.sub_focus = None;
    }

    /// 把焦点送进 / 送出子菜单（**容器项**的「展开 / 收起」）。
    ///
    /// 返回 `true` = 现在焦点在子菜单里（展开），`false` = 收回父项（收起）。
    ///
    /// ⚠ 这只改"焦点在哪"，不改"子面板画不画"——后者由 `popup::geom_with` 按
    /// 「父项是不是高亮项」决定（`popup.rs` 不在本单写域）。所以鼠标点容器项时
    /// 子面板本来就在（父项被 hover），本函数只是把焦点也送进去，让 `↑↓`/`Enter` 有落点。
    fn toggle_sub(&mut self, idx: usize) -> bool {
        if self.sub_focus.map(|(p, _)| p) == Some(idx) {
            self.sub_focus = None;
            false
        } else {
            self.sub_focus = Some((idx, 0));
            true
        }
    }

    /// 焦点回父项（`←` / `Esc` 的第一级）。返回是否真的动过（`false` = 本来就在父弹层）。
    fn leave_sub(&mut self) -> bool {
        self.sub_focus.take().is_some()
    }

    /// 子菜单内上下移动（`n` = 子行数；夹在 `0..n-1`）。
    fn move_sub(&mut self, delta: i32, n: usize) {
        let Some((p, cur)) = self.sub_focus else { return };
        if n == 0 {
            return;
        }
        let next = (cur as i32 + delta).clamp(0, n as i32 - 1) as usize;
        self.sub_focus = Some((p, next));
    }

    /// 让"焦点"与"当前高亮项"对齐：父项变了（鼠标移到别处）⇒ 焦点退回父弹层。
    ///
    /// 什么时候会不同步：`sub_focus = Some((2, 0))` 之后鼠标划到第 5 项 ——
    /// 那时子面板里已经是"语言"的行了，若还按旧行号派发，`Enter` 会执行错的子项。
    fn sync_sub_focus(&mut self) {
        if let Some((p, _)) = self.sub_focus {
            if self.active() != Some(p) {
                self.sub_focus = None;
            }
        }
    }
}

/// 键盘处理的结果：要么执行一条动作，要么只是改了展开状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyResult {
    /// 什么都没发生。
    None,
    /// 要执行这条动作（调用方走 [`action::apply`]）。
    Run(Action),
    /// 要**打开「最近打开」子菜单里的第 `n` 行**（0 基，列表顺序 = 最近优先）。
    ///
    /// 为什么不复用 [`KeyResult::Run`]：`Sub::Recent` 的每一行都是"打开**这一条**"，
    /// 而 [`Action::OpenRecent`] 不带下标 —— 只靠它，宿主不知道用户选的是哪条。
    /// 与 [`Click::RunRecent`] 同口径（两条输入路径共用同一个宿主入口）。
    RunRecent(usize),
    /// 状态变了（展开/收起/移动），只需重绘。
    Changed,
}

/// 菜单相关的键盘处理（在 `app::handle_keys` 里**先于**其它快捷键调用）。
///
/// `f1` / `esc` 等按键由调用方读出来传进来，本函数不碰 `egui::InputState` ——
/// 这样它能被单元测试直接驱动（不需要真窗口）。
///
/// 这是**不带子菜单状态**的版本（子菜单行数按官方缺省 = 空列表算）：
/// 真宿主请用 [`handle_key_with`]，否则「最近打开」有几行会数错。
pub fn handle_key(state: &mut MenuState, key: MenuKey) -> KeyResult {
    handle_key_with(state, key, &SubState::default())
}

/// [`handle_key`] 的**带子菜单状态**版本（`recent` 决定「最近打开」有几行可走）。
///
/// # 语义（审查缺陷 3 的修法）
///
/// ```text
/// Esc   焦点在子菜单 ⇒ 收子菜单（逐级）；否则收弹层
/// ↑ ↓   焦点在子菜单 ⇒ 在子项之间移动；否则在父项之间移动
/// →     当前项带子菜单且焦点在父层 ⇒ 进子菜单；否则切下一个菜单
/// ←     焦点在子菜单 ⇒ 回父项；否则切上一个菜单
/// Enter 焦点在子菜单 ⇒ 执行该子项；父层：带子菜单的项**只展开/收起**，叶子项才执行
/// ```
pub fn handle_key_with(state: &mut MenuState, key: MenuKey, sub: &SubState) -> KeyResult {
    // 当前高亮项若带子菜单，它就是"键盘可以进去的那个容器"：`(父项下标, 子菜单)`
    let active_sub = state
        .open
        .zip(state.active())
        .and_then(|(mi, idx)| {
            MENUS
                .get(mi)
                .and_then(|m| m.items.get(idx))
                .map(|it| (idx, it.sub))
        })
        .and_then(|(idx, s)| s.map(|s| (idx, s)));
    match key {
        MenuKey::Escape => {
            // ① **逐级**收起：焦点在子菜单里 ⇒ 只收子菜单，弹层留着
            if state.leave_sub() {
                return KeyResult::Changed;
            }
            if state.open.is_some() {
                state.close();
                KeyResult::Changed
            } else {
                KeyResult::None
            }
        }
        MenuKey::Down | MenuKey::Up => {
            if state.open.is_none() {
                return KeyResult::None;
            }
            let delta = if key == MenuKey::Down { 1 } else { -1 };
            // 焦点在子菜单里（且父项没变）⇒ 在子项之间移动
            if let (Some((p, _)), Some((ai, s))) = (state.sub_focus, active_sub) {
                if p == ai {
                    state.move_sub(delta, sub.sub_rows(s));
                    return KeyResult::Changed;
                }
            }
            state.move_kb(delta);
            KeyResult::Changed
        }
        MenuKey::Right => {
            if state.open.is_none() {
                return KeyResult::None;
            }
            // 当前项带子菜单、焦点还在父层 ⇒ **进子菜单**（Windows 的行为）
            if let Some((ai, _)) = active_sub {
                if state.sub_focus.is_none() {
                    state.sub_focus = Some((ai, 0));
                    return KeyResult::Changed;
                }
            }
            state.move_menu(1);
            KeyResult::Changed
        }
        MenuKey::Left => {
            if state.open.is_none() {
                return KeyResult::None;
            }
            // 焦点在子菜单里 ⇒ 回父项（**不**切菜单）
            if state.leave_sub() {
                return KeyResult::Changed;
            }
            state.move_menu(-1);
            KeyResult::Changed
        }
        MenuKey::Enter => {
            // ① 焦点在子菜单里 ⇒ 执行**那一行**
            if let (Some((p, row)), Some((ai, s))) = (state.sub_focus, active_sub) {
                if p == ai {
                    // 动态列表（最近打开）：按下标派发，宿主据此打开**那一条**
                    if matches!(s, Sub::Recent) {
                        if sub.sub_row_action(s, row).is_some() {
                            state.close();
                            return KeyResult::RunRecent(row);
                        }
                        // 空态行（官方 IsEnabled=false）⇒ 什么都不做，弹层留着
                        return KeyResult::None;
                    }
                    // 静态子项（光照 / 语言）：执行它自己的动作（灰显项也执行 ⇒ 状态栏给原因）
                    if let Some(a) = sub.sub_row_action(s, row) {
                        state.close();
                        return KeyResult::Run(a);
                    }
                    return KeyResult::None;
                }
            }
            // ② **容器项：只展开/收起子菜单，绝不执行父项动作**（审查缺陷 3）
            //
            // 官方那 3 项在 axaml 里没有 `Click`（`file.open_recent` 的容器性见
            // `MainWindow.RecentMaps.cs:23-40`），鼠标点它们也只走 `Click::Toggled`。
            if let Some((ai, _)) = active_sub {
                state.toggle_sub(ai);
                return KeyResult::Changed;
            }
            // ③ 叶子项：执行并收起
            let (Some(mi), Some(idx)) = (state.open, state.active()) else {
                return KeyResult::None;
            };
            let Some(item) = MENUS.get(mi).and_then(|m| m.items.get(idx)) else {
                return KeyResult::None;
            };
            let a = item.action;
            state.close();
            KeyResult::Run(a)
        }
    }
}

/// 菜单键盘事件（由 `app` 从 `egui::InputState` 折出来）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuKey {
    /// `Esc`。
    Escape,
    /// `↑`。
    Up,
    /// `↓`。
    Down,
    /// `←`。
    Left,
    /// `→`。
    Right,
    /// `Enter`。
    Enter,
}

/// 鼠标交互的结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Click {
    /// 没点到与菜单有关的东西。
    None,
    /// 只是展开/收起了（要重绘）。
    Toggled,
    /// 点中了某项，要执行它。
    Run(Action),
    /// 点中了「最近打开」子菜单里的第 `n` 行（0 基，最近优先）⇒ 宿主去打开**那一条**。
    ///
    /// 与 [`KeyResult::RunRecent`] 同口径：`Action::OpenRecent` 不带下标，
    /// 只靠它宿主分不清用户点的是哪条历史。
    RunRecent(usize),
}

/// 鼠标点/悬停在菜单的哪一块（**纯几何判据**，`interact` 与单元测试共用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    /// 菜单栏上的第几个标签。
    Label(usize),
    /// 展开的弹层里的第几项（菜单内下标）。
    Item(usize),
    /// 展开的**子面板**里的第几行（子行下标，见 [`popup::SubRow`]）。
    Sub(usize),
    /// 在弹层底板里但不在任何项上（分隔线 / 内边距）。
    Popup,
    /// 与菜单无关的地方（画布 / 面板 / 状态栏…）。
    Elsewhere,
}

/// 给定鼠标位置，判断它落在菜单的哪一块（子菜单状态取官方缺省）。
pub fn hit_test(p: &egui::Painter, l: &Layout, state: &MenuState, pos: egui::Pos2) -> Hit {
    hit_test_with(p, l, state, pos, &SubState::default())
}

/// [`hit_test`] 的**带子菜单状态**版本（`recent` 会影响「最近打开」的行数 ⇒ 影响命中）。
///
/// 判据顺序有因果：**先子面板 → 再弹层 → 后标签** —— 子面板挂在弹层右侧、不与标签重叠，
/// 但它压在弹层外面，必须先判；弹层会盖住菜单栏下方的内容，所以弹层先于标签。
pub fn hit_test_with(
    p: &egui::Painter,
    l: &Layout,
    state: &MenuState,
    pos: egui::Pos2,
    sub: &SubState,
) -> Hit {
    if let Some(mi) = state.open {
        let g = popup::geom_with(p, l, mi, state.active(), &sub.recent);
        if let Some(sg) = &g.sub {
            if sg.rect.contains(pos) {
                return match popup::hit_sub(&g, pos) {
                    Some(idx) => Hit::Sub(idx),
                    None => Hit::Popup,
                };
            }
        }
        if g.rect.contains(pos) {
            return match popup::hit(&g, pos) {
                Some(idx) => Hit::Item(idx),
                None => Hit::Popup,
            };
        }
    }
    let labels = crate::menubar::item_rects(p, l);
    match labels.iter().position(|r| r.contains(pos)) {
        Some(i) => Hit::Label(i),
        None => Hit::Elsewhere,
    }
}

/// 处理菜单栏与弹层的鼠标交互（子菜单状态取官方缺省 = 空列表）。
///
/// 必须在**其它交互之前**调用（弹层压在画布/面板之上，点击先归它）。
///
/// ⚠ **`app.rs` 用的是 [`interact_with`]**（2026-11-05 起「最近打开」的行来自设置文件）：
/// 用这个版本会把子菜单算成只有空态行 ⇒ 点不到真历史。本函数只留给不关心最近列表的调用方。
pub fn interact(ui: &mut Ui, l: &Layout, state: &mut MenuState) -> Click {
    interact_with(ui, l, state, &SubState::default())
}

/// [`interact`] 的**带子菜单状态**版本（`recent` 决定「最近打开」的行数与命中）。
pub fn interact_with(ui: &mut Ui, l: &Layout, state: &mut MenuState, sub: &SubState) -> Click {
    let p = ui.painter();
    let labels = crate::menubar::item_rects(p, l);
    let pointer = ui.ctx().pointer_hover_pos();

    // ⓪ 标签 hover（收起态也要有反馈）
    state.hover_label = pointer.and_then(|pos| labels.iter().position(|r| r.contains(pos)));

    // ① 点菜单标签：展开 / 收起 / 切换
    for (i, r) in labels.iter().enumerate() {
        if ui
            .interact(*r, ui.id().with(("menu_label", i)), egui::Sense::click())
            .clicked()
        {
            state.toggle(i);
            return Click::Toggled;
        }
    }

    // ② 展开态：弹层内的 hover 与点击
    if let Some(mi) = state.open {
        let g = popup::geom_with(p, l, mi, state.active(), &sub.recent);
        // 展开时鼠标移到别的标签上 ⇒ 直接切过去（Windows 菜单的既有行为）
        if let Some(pos) = pointer {
            if !g.rect.contains(pos) && !g.sub.as_ref().is_some_and(|s| s.rect.contains(pos)) {
                for (i, r) in labels.iter().enumerate() {
                    if r.contains(pos) && i != mi {
                        state.open(i);
                        return Click::Toggled;
                    }
                }
            }
        }
        // hover：指针在弹层里 ⇒ 就是那一行；指针**在子面板里** ⇒ 保持父项
        // （否则"指针一离开父项子面板就消失"，用户永远点不到子项 —— 子菜单的经典陷阱）
        state.hover = pointer.and_then(|pos| {
            if let Some(sg) = &g.sub {
                if sg.rect.contains(pos) {
                    return Some(sg.parent);
                }
            }
            popup::hit(&g, pos)
        });
        // 鼠标换了父项 ⇒ 键盘焦点必须退回父层（否则回车会执行**上一条**子项）
        state.sync_sub_focus();
        if ui.input(|i| i.pointer.any_click()) {
            if let Some(pos) = ui
                .input(|i| i.pointer.interact_pos())
                .or_else(|| ui.ctx().pointer_hover_pos())
            {
                match hit_test_with(p, l, state, pos, sub) {
                    Hit::Sub(si) => {
                        // 子面板的行：先看这是不是**动态列表**（最近打开）——
                        // 那里的每一行都是"打开这一条"，必须带下标回去
                        let parent_sub = g
                            .sub
                            .as_ref()
                            .and_then(|sg| MENUS.get(mi).and_then(|m| m.items.get(sg.parent)))
                            .and_then(|it| it.sub);
                        if matches!(parent_sub, Some(Sub::Recent)) && !sub.recent.is_empty() {
                            // 灰行（文件已不存在）**也派发**：宿主要给原因 + 把它从列表里去掉
                            state.close();
                            return Click::RunRecent(si);
                        }
                        // 静态子项：有动作就执行；`None`（官方空态行）⇒ 什么都不做
                        let action = g.sub.as_ref().and_then(|sg| {
                            sg.rows
                                .iter()
                                .find(|r| r.index == si)
                                .and_then(|r| r.action)
                        });
                        if let Some(a) = action {
                            state.close();
                            return Click::Run(a);
                        }
                        return Click::None;
                    }
                    Hit::Item(idx) => {
                        if let Some(item) = MENUS.get(mi).and_then(|m| m.items.get(idx)) {
                            // 带子菜单的父项**不是动作**（官方那 3 项是容器）：
                            // 点了不执行、也不收起 —— 只把**键盘焦点**送进/送出子菜单，
                            // 与 `Enter` 完全同一套语义（审查缺陷 3：两条路径不许相反）
                            if item.sub.is_some() {
                                state.toggle_sub(idx);
                                return Click::Toggled;
                            }
                            let a = item.action;
                            state.close();
                            return Click::Run(a);
                        }
                    }
                    // 点在弹层外（含别的面板 / 画布）⇒ 收起，且**不**把这次点击继续传给下面
                    Hit::Elsewhere => {
                        state.close();
                        return Click::Toggled;
                    }
                    // 点在弹层底板里但不在项上（分隔线/内边距）⇒ 什么都不做
                    Hit::Popup | Hit::Label(_) => {}
                }
            }
        }
    }
    Click::None
}

/// 弹层（含展开中的子面板）是否挡住了某个点
/// （供 app 决定"这一帧要不要把点击让给弹层"）。
///
/// ⚠ 本函数按**空**最近地图列表算子面板几何（= 只出空态行）。当前仓内没有调用方
/// （`app` 用的是 [`hit_test_with`] / [`interact_with`]，它们都带真 `SubState`）；
/// 将来要用请改用带 `SubState` 的版本，否则「最近打开」有历史时子面板高度会算小。
pub fn popup_contains(p: &egui::Painter, l: &Layout, state: &MenuState, pos: egui::Pos2) -> bool {
    match state.open {
        Some(mi) => {
            let g = popup::geom_with(p, l, mi, state.active(), &[]);
            g.rect.contains(pos) || g.sub.as_ref().is_some_and(|s| s.rect.contains(pos))
        }
        None => false,
    }
}

/// 画菜单栏 + 弹层（在 `menubar::draw` 之后调用，保证弹层压住面板与画布）。
///
/// `grid` = 画布当前的单元格网格开关：`视图 → 单元格网格` 是勾选型项，
/// 勾必须反映真状态（见 [`model::check_with`]）。
/// `doors` = 画布当前的**开启门**开关：同上（勾必须反映真状态）。
///
/// ⚠ 这是 [`draw_with`] 的**默认档位**版本（缺省白天 / 简体中文 / 空列表）。
/// 生产路径（`app.rs`）用 [`draw_with`] 传真的 `lighting`，本函数只给"不关心档位"的调用方与测试。
/// **`app.rs` 用的是 [`draw_with`]**（2026-11-05 起「最近打开」的行来自设置文件，
/// 用这个版本会把子菜单画成空态）；本函数只留给"不关心最近列表"的调用方与测试。
#[allow(clippy::too_many_arguments)] // 9 个都是"这一帧的输入"，与 `popup::draw` 同口径
pub fn draw(
    p: &egui::Painter,
    l: &Layout,
    state: &MenuState,
    has_map: bool,
    dark: bool,
    grid: bool,
    doors: bool,
    panels: &PanelVis,
) {
    draw_with(
        p,
        l,
        state,
        has_map,
        dark,
        grid,
        doors,
        model::MapLight::Day,
        panels,
        &SubState::default(),
    );
}

/// [`draw`] 的**带子菜单状态 + 真光照档位**版本（生产路径：`app.rs` 每帧调它）。
#[allow(clippy::too_many_arguments)] // 10 个都是"这一帧的输入"，与 `popup::draw` 同口径
pub fn draw_with(
    p: &egui::Painter,
    l: &Layout,
    state: &MenuState,
    has_map: bool,
    dark: bool,
    grid: bool,
    doors: bool,
    lighting: model::MapLight,
    panels: &PanelVis,
    sub: &SubState,
) {
    let Some(mi) = state.open else {
        // 收起态：鼠标停在标签上就垫一层浅底 + 重画文字（与展开态同一套配色）
        if let Some(hi) = state.hover_label {
            let labels = crate::menubar::item_rects(p, l);
            if let Some(r) = labels.get(hi) {
                let pal = crate::theme::ui();
                crate::paint::fill(p, *r, pal.menu_popup_hover);
                crate::paint::text(
                    p,
                    r.center(),
                    egui::Align2::CENTER_CENTER,
                    MENUS[hi].title,
                    crate::metrics::MENU_FONT,
                    pal.menu_bar_text,
                );
            }
        }
        return;
    };
    // 展开的菜单标签要有"按下去"的观感：用弹层 hover 色垫一下，**再把标签文字重画一遍**
    // （底色会盖掉 `menubar::draw` 已经画过的字）。
    let labels = crate::menubar::item_rects(p, l);
    if let Some(r) = labels.get(mi) {
        let pal = crate::theme::ui();
        crate::paint::fill(p, *r, pal.menu_popup_hover);
        crate::paint::text(
            p,
            r.center(),
            egui::Align2::CENTER_CENTER,
            MENUS[mi].title,
            crate::metrics::MENU_FONT,
            pal.menu_bar_text,
        );
    }
    popup::draw_with_focus(
        p,
        l,
        mi,
        state.active(),
        state.sub_focus(),
        has_map,
        dark,
        grid,
        doors,
        lighting,
        panels,
        sub,
    );
}

/// `--menu-dump` 追加的两列（审查缺陷 8：让子项与分隔线**自证**）。
///
/// 列名用 ASCII：验收脚本要 grep 它们；中文列名会被 `ui_text` 的
/// 「界面文案不许在代码里写字面量」纪律挡住，而 `ui_text.rs` 不在本单写域。
const DUMP_COL_PARENT: &str = "parent";
const DUMP_COL_SEP: &str = "sep";

/// 拼一行 dump = `ui_text::DIAG_MENU_DUMP_ROW` 原文 + `parent=` / `sep=` 两列。
#[allow(clippy::too_many_arguments)] // 8 个字段就是这一行的 8 列，包结构体只会更绕
fn dump_row(
    menu: &str,
    n: &str,
    id: &str,
    label: &str,
    shortcut: &str,
    state: &str,
    parent: &str,
    sep: &str,
) -> String {
    format!(
        "{} | {}={} | {}={}",
        tx::fill(
            tx::DIAG_MENU_DUMP_ROW,
            &[
                ("menu", menu),
                ("n", n),
                ("id", id),
                ("label", label),
                ("shortcut", shortcut),
                ("state", state),
            ]
        ),
        DUMP_COL_PARENT,
        parent,
        DUMP_COL_SEP,
        sep
    )
}

/// `--menu-dump` 的**纯函数**版本：返回要打的每一行（[`dump`] 只负责打到 stderr）。
///
/// # 为什么要加 `parent` / `sep` 两列（审查缺陷 8）
///
/// 原来只按 `MENUS` **平铺**打印：不打印"有无子项"、不打印 `sep_before`
/// ⇒ 它自己证明不了"39 项与官方一致"，发现子菜单缺失全靠人工比对 axaml。加两列之后：
///
/// * `parent=-` ⇒ 顶级项；`parent=view.map_lighting` ⇒ 该行是地图光照的子项；
/// * `sep=yes|no` ⇒ 这一项上面有没有官方 `<Separator />`；子项/空态行恒 `-`
///   （官方 3 个子菜单里都没有分隔线）。
///
/// 子项**紧跟父项**打印，编号 `#3.1` 式（父项号.子项号），这样"哪 5 个子项、
/// 挂在谁下面"一眼可读；汇总行仍只数**顶级 39 项**（子项不计入，否则口径会从 39 变 44）。
///
/// 这是**不带最近地图列表**的版本（按空列表 ⇒ 只出官方空态行）；
/// 真宿主请用 [`dump_lines_with`]。
pub fn dump_lines(has_map: bool, dark: bool, panels: &PanelVis) -> Vec<String> {
    dump_lines_with(has_map, dark, panels, &SubState::default())
}

/// [`dump_lines`] 的**带子菜单状态**版本（`recent` 决定「最近打开」打几行）。
///
/// 「最近打开」的两态（2026-11-05 接线后）：
///
/// * `recent` 空 ⇒ 一行官方空态行：`file.open_recent.empty` / 文案「暂无最近地图」/
///   状态串「空态（灰显）」；
/// * `recent` 非空 ⇒ 逐条 `file.open_recent.1` / `.2` …（1 基，1 = 最近打开的那张），
///   文案是文件名（官方 `Path.GetFileName`），状态串「已实现」/「文件已不存在（灰显）」。
pub fn dump_lines_with(
    has_map: bool,
    dark: bool,
    panels: &PanelVis,
    sub: &SubState,
) -> Vec<String> {
    let mut lines = Vec::new();
    let mut ready = 0;
    let mut need_map = 0;
    let mut todo = 0;
    lines.push(tx::fill(
        tx::DIAG_MENU_DUMP_HEAD,
        &[
            ("menus", &MENUS.len().to_string()),
            ("items", &model::item_count().to_string()),
        ],
    ));
    for menu in MENUS.iter() {
        for (i, item) in menu.items.iter().enumerate() {
            let st = model::state(item.action);
            match st {
                model::State::Ready => ready += 1,
                model::State::NeedsMap => need_map += 1,
                model::State::Todo(_) => todo += 1,
            }
            let n = (i + 1).to_string();
            lines.push(dump_row(
                menu.title,
                &n,
                item.action.id(),
                item.action.label(),
                item.action.shortcut().unwrap_or(tx::VAL_UNKNOWN),
                &st.text(),
                "-",
                if item.sep_before { "yes" } else { "no" },
            ));
            // 子项紧跟父项
            match item.sub {
                Some(model::Sub::Static(items)) => {
                    for (j, s) in items.iter().enumerate() {
                        lines.push(dump_row(
                            menu.title,
                            &format!("{}.{}", i + 1, j + 1),
                            s.action.id(),
                            s.action.label(),
                            s.action.shortcut().unwrap_or(tx::VAL_UNKNOWN),
                            &model::state(s.action).text(),
                            item.action.id(),
                            "-",
                        ));
                    }
                }
                Some(model::Sub::Recent) => {
                    // 动态列表：行 = 设置文件里的 `recent`（最近优先）；
                    // 空 ⇒ 官方空态行（`RecentMaps.cs:39`）
                    let empty = sub.recent.is_empty();
                    for (j, (label, exists)) in sub.recent_rows().iter().enumerate() {
                        let (id, state) = if empty {
                            (
                                model::ID_RECENT_EMPTY.to_owned(),
                                model::STATE_RECENT_EMPTY.to_owned(),
                            )
                        } else {
                            (
                                format!("{}{}", model::ID_RECENT_ROW_PREFIX, j + 1),
                                if *exists {
                                    tx::STATE_READY.to_owned()
                                } else {
                                    model::STATE_RECENT_GONE.to_owned()
                                },
                            )
                        };
                        lines.push(dump_row(
                            menu.title,
                            &format!("{}.{}", i + 1, j + 1),
                            &id,
                            label,
                            tx::VAL_UNKNOWN,
                            &state,
                            item.action.id(),
                            "-",
                        ));
                    }
                }
                None => {}
            }
        }
    }
    lines.push(tx::fill(
        tx::DIAG_MENU_DUMP_TAIL,
        &[
            ("ready", &ready.to_string()),
            ("needmap", &need_map.to_string()),
            ("todo", &todo.to_string()),
        ],
    ));
    let _ = (has_map, dark, panels);
    lines
}

/// `--menu-dump` 用的子菜单状态：**从设置文件读**最近地图列表。
///
/// # 为什么在这里读，而不是让 `main.rs` 传进来
///
/// `--menu-dump` 在 `main.rs` 里只有一行调用（`menu::dump(has_map, dark, &panels)`），
/// 而那个文件不在本单写域。设置文件路径就是 [`settings::default_path()`]
/// （与 `app` 启动时**同一份**；`--settings` 只覆盖窗口运行时的路径，不影响 dump ——
/// 这一条写在注释里，免得验收时以为 dump 该跟着 `--settings` 走）。
///
/// 文件**不存在**是正常首跑（静默空列表）；文件存在却读不出来 ⇒ 打一行诊断
/// （[`model::DIAG_RECENT_UNREADABLE`]）—— 不许假装"没有历史"。
fn dump_sub_state() -> SubState {
    let path = settings::default_path();
    match settings::load_quiet(&path) {
        Ok(s) => SubState {
            recent: s.recent,
            ..SubState::default()
        },
        Err(e) => {
            if path.exists() {
                eprintln!(
                    "{}",
                    tx::fill(
                        model::DIAG_RECENT_UNREADABLE,
                        &[("path", &path.display().to_string()), ("err", &e)]
                    )
                );
            }
            SubState::default()
        }
    }
}

/// `--menu-dump`：把 40 项（含子项行）逐条打到 stderr（**不开窗**，验收门 §四.2 的证据来源）。
///
/// 返回**顶级项数**（= 40；子项行不计入，与旧口径一致）。
pub fn dump(has_map: bool, dark: bool, panels: &PanelVis) -> usize {
    let sub = dump_sub_state();
    for line in dump_lines_with(has_map, dark, panels, &sub) {
        eprintln!("{line}");
    }
    model::item_count()
}

/// 弹层矩形（诊断/截图比对用；`None` = 没展开）。
pub fn popup_rect(p: &egui::Painter, l: &Layout, state: &MenuState) -> Option<Rect> {
    state.open.map(|mi| popup::geom(p, l, mi).rect)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::model::{find_by_id, item_count};

    /// 40 项、5 菜单（`dump` 的计数与静态表必须一致）。
    #[test]
    fn 表与常量一致() {
        assert_eq!(MENUS.len(), 5);
        assert_eq!(item_count(), 40);
        assert!(find_by_id("view.dark_mode").is_some());
    }

    /// 展开 / 收起 / 切换。
    #[test]
    fn 展开收起与切换() {
        let mut s = MenuState::default();
        assert!(s.open_index().is_none());
        s.toggle(2);
        assert_eq!(s.open_index(), Some(2));
        s.toggle(2);
        assert!(s.open_index().is_none(), "再点同一个标签应收起");
        s.toggle(0);
        s.open(4);
        assert_eq!(s.open_index(), Some(4));
        s.open(9); // 越界
        assert_eq!(s.open_index(), Some(4), "越界不该改状态");
        s.close();
        assert!(s.open_index().is_none());
    }

    /// 键盘：↓ 从无到第一项，↑ 到头停住，Enter 返回那一项并收起。
    #[test]
    fn 键盘上下与回车() {
        let mut s = MenuState::default();
        s.open(0);
        assert_eq!(handle_key(&mut s, MenuKey::Down), KeyResult::Changed);
        assert_eq!(s.active(), Some(0));
        for _ in 0..20 {
            let _ = handle_key(&mut s, MenuKey::Down);
        }
        assert_eq!(s.active(), Some(FILE_LAST), "↓ 到底要停在最后一项");
        let r = handle_key(&mut s, MenuKey::Enter);
        assert_eq!(r, KeyResult::Run(model::Action::Exit));
        assert!(s.open_index().is_none(), "回车后弹层收起");
        // 收起状态下 ↓ 什么都不做
        assert_eq!(handle_key(&mut s, MenuKey::Down), KeyResult::None);
    }

    /// 文件菜单最后一项的下标（`Exit`）。
    const FILE_LAST: usize = 9;

    /// 键盘：← → 在 5 个菜单之间循环切换，且切换后清空选中项。
    #[test]
    fn 键盘左右切菜单() {
        let mut s = MenuState::default();
        s.open(0);
        let _ = handle_key(&mut s, MenuKey::Down);
        assert_eq!(handle_key(&mut s, MenuKey::Right), KeyResult::Changed);
        assert_eq!(s.open_index(), Some(1));
        assert_eq!(s.active(), None, "切菜单后选中项要清空");
        let _ = handle_key(&mut s, MenuKey::Left);
        let _ = handle_key(&mut s, MenuKey::Left);
        assert_eq!(s.open_index(), Some(4), "从 0 往左循环到 4");
    }

    /// Esc 先收弹层（返回 `Changed`），收起后再按才是 `None`（交给上层的其它 Esc 语义）。
    #[test]
    fn 键盘esc先收弹层() {
        let mut s = MenuState::default();
        assert_eq!(handle_key(&mut s, MenuKey::Escape), KeyResult::None);
        s.open(3);
        assert_eq!(handle_key(&mut s, MenuKey::Escape), KeyResult::Changed);
        assert!(s.open_index().is_none());
    }

    /// **审查缺陷 3 的钉子：`Enter` 在 3 个容器项上绝不执行父项动作**。
    ///
    /// 判据（逐条可机器核对）：
    /// ① 父层里按回车 ⇒ 返回 [`KeyResult::Changed`]（**不是** `Run(父项动作)`），
    ///    弹层还开着，`sub_focus` 变成 `Some((父项, 0))` = 展开子菜单；
    /// ② 再按一次回车 ⇒ 允许"执行子项"（Windows 的标准行为：回车进子菜单后第二下激活
    ///    第一项），但返回的动作**必须是这个容器自己的子项**，**绝不能是父项**；
    ///    空列表的「最近打开」第 0 行是灰行 ⇒ 什么都不做（`None`），弹层留着。
    ///
    /// 再反向钉一条：**叶子项**的 `Enter` 仍然执行 —— 否则"不执行"可能只是"整个键盘坏了"。
    #[test]
    fn 回车在容器项上只切换子菜单() {
        for id in ["file.open_recent", "view.map_lighting", "view.language"] {
            let (mi, ii, item) = model::find_by_id(id).expect("容器项必须存在");
            let parent_action = item.action;
            assert!(item.sub.is_some(), "{id} 必须是可展开容器");
            let mut s = MenuState::default();
            s.open(mi);
            assert!(s.focus(ii), "{id} 设不上高亮项");
            // ① 父层回车 = 展开（焦点进子菜单），不执行父项动作、不收弹层
            let r = handle_key(&mut s, MenuKey::Enter);
            assert_eq!(r, KeyResult::Changed, "{id} 的回车不许执行父项动作：{r:?}");
            // 证据（`--nocapture` 可读）：父层回车返回的不是 `Run(父项)`，而是"只改状态"
            eprintln!(
                "[证据] {id}（{}，父项动作 {}）：父层回车 ⇒ {r:?}；sub_focus={:?}",
                model::state(parent_action).text(),
                parent_action.id(),
                s.sub_focus()
            );
            assert_eq!(s.open_index(), Some(mi), "{id} 的回车不许收起弹层");
            assert_eq!(s.sub_focus(), Some((ii, 0)), "{id} 的回车要把焦点送进子菜单");
            // ② 第二下回车：要么激活子项，要么灰行不动 —— 但**绝不是父项**
            let r = handle_key(&mut s, MenuKey::Enter);
            match r {
                KeyResult::Run(a) => {
                    assert_ne!(a, parent_action, "{id} 第二下回车执行了父项动作");
                    let (_, pi, _, _) = model::find_sub_by_id(a.id())
                        .unwrap_or_else(|| panic!("{id} 第二下回车执行了非子项动作：{}", a.id()));
                    assert_eq!(pi, ii, "{id} 第二下回车执行的子项不挂在它下面");
                }
                KeyResult::None => {
                    // 只有"空列表的最近打开"会走到这里（第 0 行是官方灰行）
                    assert_eq!(id, "file.open_recent", "{id} 不该出现「回车没反应」");
                }
                other => panic!("{id} 第二下回车结果意外：{other:?}"),
            }
        }
        // 反向：叶子项（灰显的「开启门」）回车**仍然**执行 ⇒ 状态栏给原因
        let (mi, ii, item) = model::find_by_id("view.open_doors").expect("开启门");
        assert!(item.sub.is_none(), "开启门是叶子项");
        let mut s = MenuState::default();
        s.open(mi);
        assert!(s.focus(ii));
        assert_eq!(
            handle_key(&mut s, MenuKey::Enter),
            KeyResult::Run(Action::OpenDoors),
            "叶子项（哪怕灰显）回车要照常执行"
        );
        assert!(s.open_index().is_none(), "叶子项执行后弹层收起");
    }

    /// **键盘能进子菜单**（审查缺陷 3 的另一半：以前子菜单只能鼠标到达）。
    ///
    /// `→` 进子菜单 → `↑↓` 在子项间移动（父项高亮不变）→ `←` 回父级（**不**切菜单）
    /// → 再 `←` 才切上一个菜单。
    #[test]
    fn 右箭头进子菜单且上下移动左箭头回父级() {
        let (mi, ii, _) = model::find_by_id("view.map_lighting").expect("地图光照");
        let mut s = MenuState::default();
        s.open(mi);
        assert!(s.focus(ii));
        // → 进子菜单（地图光照 3 行）
        assert_eq!(handle_key(&mut s, MenuKey::Right), KeyResult::Changed);
        assert_eq!(s.sub_focus(), Some((ii, 0)), "→ 必须把焦点送进子菜单");
        assert_eq!(s.open_index(), Some(mi), "→ 进子菜单时不许切菜单");
        // ↓ 在子项里走：0 → 1 → 2 → 停在 2（3 行，到底夹住）
        for want in [1usize, 2, 2] {
            assert_eq!(handle_key(&mut s, MenuKey::Down), KeyResult::Changed);
            assert_eq!(s.sub_focus(), Some((ii, want)), "子项内 ↓ 停错行");
        }
        assert_eq!(s.active(), Some(ii), "焦点在子菜单里时父项高亮不许变");
        // ↑ 回来
        assert_eq!(handle_key(&mut s, MenuKey::Up), KeyResult::Changed);
        assert_eq!(s.sub_focus(), Some((ii, 1)));
        // ← 回父级（**不**切菜单）
        assert_eq!(handle_key(&mut s, MenuKey::Left), KeyResult::Changed);
        assert_eq!(s.sub_focus(), None, "← 回父级要把焦点交回父弹层");
        assert_eq!(s.open_index(), Some(mi), "← 回父级时不许切菜单");
        // 再 ← 才是切上一个菜单
        assert_eq!(handle_key(&mut s, MenuKey::Left), KeyResult::Changed);
        assert_eq!(s.open_index(), Some(mi - 1));
    }

    /// **`Esc` 逐级收起**：焦点在子菜单 ⇒ 只收子菜单；再按一次才收弹层。
    #[test]
    fn esc逐级收起() {
        let (mi, ii, _) = model::find_by_id("view.map_lighting").expect("地图光照");
        let mut s = MenuState::default();
        s.open(mi);
        assert!(s.focus(ii));
        let _ = handle_key(&mut s, MenuKey::Right);
        assert_eq!(s.sub_focus(), Some((ii, 0)));
        assert_eq!(handle_key(&mut s, MenuKey::Escape), KeyResult::Changed);
        assert_eq!(s.sub_focus(), None, "第一级 Esc 只收子菜单");
        assert_eq!(s.open_index(), Some(mi), "第一级 Esc 不许收弹层");
        assert_eq!(handle_key(&mut s, MenuKey::Escape), KeyResult::Changed);
        assert!(s.open_index().is_none(), "第二级 Esc 收弹层");
        assert_eq!(handle_key(&mut s, MenuKey::Escape), KeyResult::None);
    }

    /// **「最近打开」子菜单：回车按下标派发**（宿主据此打开**那一条**）。
    ///
    /// 空列表 ⇒ 只有空态行（官方 `IsEnabled=false`）⇒ 回车什么都不做、弹层留着；
    /// 非空 ⇒ 回车返回 [`KeyResult::RunRecent`]（带 0 基下标），弹层收起。
    #[test]
    fn 回车在最近打开里派发下标() {
        let (mi, ii, _) = model::find_by_id("file.open_recent").expect("最近打开");
        // ① 空列表
        let mut s = MenuState::default();
        s.open(mi);
        assert!(s.focus(ii));
        let _ = handle_key(&mut s, MenuKey::Right);
        assert_eq!(s.sub_focus(), Some((ii, 0)));
        assert_eq!(
            handle_key(&mut s, MenuKey::Enter),
            KeyResult::None,
            "空态行回车什么都不做（官方 IsEnabled=false）"
        );
        assert_eq!(s.open_index(), Some(mi), "空态行回车不许收弹层");

        // ② 非空：第 2 行（下标 1）派发下标 1
        let sub = SubState {
            recent: vec![r"E:\m\41.map".to_owned(), r"E:\m\10.map".to_owned()],
            ..SubState::default()
        };
        let mut s = MenuState::default();
        s.open(mi);
        assert!(s.focus(ii));
        assert_eq!(
            handle_key_with(&mut s, MenuKey::Right, &sub),
            KeyResult::Changed
        );
        assert_eq!(
            handle_key_with(&mut s, MenuKey::Down, &sub),
            KeyResult::Changed
        );
        assert_eq!(s.sub_focus(), Some((ii, 1)), "↓ 要在两条历史里走");
        assert_eq!(
            handle_key_with(&mut s, MenuKey::Enter, &sub),
            KeyResult::RunRecent(1),
            "回车要带下标派发（宿主才知道是哪条历史）"
        );
        assert!(s.open_index().is_none(), "执行后弹层收起");
    }

    /// **鼠标点容器项 ⇒ 只 `Toggled`，不执行父项动作**（审查缺陷 3 的鼠标那一半）。
    ///
    /// 与 [`tests::回车在容器项上只切换子菜单`] 合起来就是"同一行两条路径同语义"的钉子：
    /// 鼠标返回 `Toggled`、键盘返回 `Changed`，**两者都不是 `Run(父项)`**。
    #[test]
    fn 点击容器项只切换子菜单() {
        let ctx = crate::fonts::measure_ctx();
        let screen = egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(crate::metrics::WINDOW_W, crate::metrics::WINDOW_H),
        );
        let l = Layout::compute(screen);
        // 文件 → 最近打开（菜单内下标 2）
        let (mi, ii, item) = model::find_by_id("file.open_recent").expect("最近打开");
        assert!(item.sub.is_some());
        let probe = egui::Painter::new(ctx.clone(), egui::LayerId::debug(), screen);
        let g = popup::geom(&probe, &l, mi);
        let row = g
            .rows
            .iter()
            .find(|r| !r.sep && r.index == ii)
            .expect("最近打开那一行")
            .rect
            .center();
        let mut s = MenuState::default();
        s.open(mi);
        let mut input = egui::RawInput {
            screen_rect: Some(screen),
            events: vec![
                egui::Event::PointerMoved(row),
                egui::Event::PointerButton {
                    pos: row,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
                egui::Event::PointerButton {
                    pos: row,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                },
            ],
            ..Default::default()
        };
        input.time = Some(1.0);
        let mut got = Click::None;
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ctx, |ui| {
                    got = interact_with(ui, &l, &mut s, &SubState::default());
                });
        });
        assert_eq!(
            got,
            Click::Toggled,
            "点容器项只切换子菜单（绝不返回 Run(父项动作)）"
        );
        assert_eq!(s.sub_focus(), Some((ii, 0)), "点容器项要把焦点送进子菜单");
        assert_eq!(s.open_index(), Some(mi), "点容器项不许收起弹层");
    }

    /// **鼠标点「最近打开」的一行 ⇒ 带下标派发**（[`Click::RunRecent`]）。
    ///
    /// 这条钉的是"点的是哪一条"：子面板只画文件名（官方 `Path.GetFileName`），
    /// 宿主必须拿到下标才知道该打开哪条历史。用 egui 的合成指针事件真跑一次
    /// [`interact_with`]（不开窗，与 `子面板真的被绘制` 同一套做法）。
    #[test]
    fn 点击最近打开的一行派发下标() {
        let ctx = crate::fonts::measure_ctx();
        let screen = egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(crate::metrics::WINDOW_W, crate::metrics::WINDOW_H),
        );
        let l = Layout::compute(screen);
        let sub = SubState {
            recent: vec![r"E:\m\41.map".to_owned(), r"E:\m\10.map".to_owned()],
            ..SubState::default()
        };
        let mut s = MenuState::default();
        s.open(0); // 文件
        s.hover = Some(2); // 最近打开
        // 先量出第 2 行（下标 1）的中心
        let probe = egui::Painter::new(ctx.clone(), egui::LayerId::debug(), screen);
        let g = popup::geom_with(&probe, &l, 0, s.active(), &sub.recent);
        let row = g
            .sub
            .as_ref()
            .expect("最近打开必须展开子面板")
            .rows
            .iter()
            .find(|r| r.index == 1)
            .expect("第 2 行")
            .rect
            .center();
        // 合成一次点击（同一帧里按下 + 抬起）
        let mut input = egui::RawInput {
            screen_rect: Some(screen),
            events: vec![
                egui::Event::PointerMoved(row),
                egui::Event::PointerButton {
                    pos: row,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
                egui::Event::PointerButton {
                    pos: row,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                },
            ],
            ..Default::default()
        };
        input.time = Some(1.0);
        let mut got = Click::None;
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ctx, |ui| {
                    got = interact_with(ui, &l, &mut s, &sub);
                });
        });
        assert_eq!(got, Click::RunRecent(1), "点第 2 行要带下标 1 派发");
        assert!(s.open_index().is_none(), "点了子项要收起弹层");
    }

    /// **`--menu-dump` 的「最近打开」两态**（空 ⇒ 1 行空态；非空 ⇒ 逐条 `file.open_recent.N`）。
    ///
    /// 用**真文件**验"在 / 不在"两档（不编假路径）：`41.map` 真写出来 ⇒ 状态「已实现」，
    /// `9.map` 不写 ⇒ 状态「文件已不存在（灰显）」。空态行**不许**与非空行同时出现。
    #[test]
    fn dump带最近地图行() {
        let panels = PanelVis::default();
        // 空态：行数与旧口径一致（表头 + 40 顶级 + 5 静态子项 + 1 空态 + 汇总）
        let lines = dump_lines(false, false, &panels);
        assert_eq!(lines.len(), 1 + item_count() + 5 + 1 + 1);
        let dir = std::env::temp_dir().join("miredit_gui_menu_dump_recent");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建临时目录");
        let a = dir.join("41.map");
        std::fs::write(&a, b"x").expect("写临时地图");
        let gone = dir.join("9.map");
        let sub = SubState {
            recent: vec![a.display().to_string(), gone.display().to_string()],
            ..SubState::default()
        };
        let lines = dump_lines_with(false, false, &panels, &sub);
        let rows: Vec<&String> = lines
            .iter()
            .filter(|l| l.contains("parent=file.open_recent"))
            .collect();
        assert_eq!(rows.len(), 2, "两条历史 ⇒ 两行：{rows:?}");
        assert!(
            rows[0].contains("file.open_recent.1")
                && rows[0].contains("41.map")
                && rows[0].contains(tx::STATE_READY),
            "第 1 行（最近优先）必须是 41.map 且状态「已实现」：{}",
            rows[0]
        );
        assert!(
            rows[1].contains("file.open_recent.2")
                && rows[1].contains("9.map")
                && rows[1].contains(model::STATE_RECENT_GONE),
            "第 2 行必须是 9.map 且状态「文件已不存在」：{}",
            rows[1]
        );
        assert!(
            !lines.iter().any(|l| l.contains(model::ID_RECENT_EMPTY)),
            "有历史时不许再打空态行"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `Overlay` 的三态判定。
    #[test]
    fn 浮层三态() {
        assert!(!Overlay::None.is_some());
        assert!(Overlay::Shortcuts.is_some());
        assert!(Overlay::OpenMap {
            path: String::new()
        }
        .is_some());
        // 另存为模态（2026-11-05）：与打开模态一样算「有浮层」（弹层与画布交互要让位）
        assert!(Overlay::SaveAs {
            path: String::new()
        }
        .is_some());
        // 两个导出模态（2026-11-05）同理：它们也是路径输入模态，同样要让位
        assert!(Overlay::ExportSelection {
            path: String::new()
        }
        .is_some());
        assert!(Overlay::ExportMinimap {
            path: String::new()
        }
        .is_some());
    }

    /// **鼠标命中判据**（点击语义的纯几何部分）：标签 / 弹层项 / 弹层空白 / 别处。
    ///
    /// 这条挡的是"点菜单标签打不开、点别处收不起"这类只有真鼠标才能发现的问题 ——
    /// 判据被抽成纯函数之后，不需要窗口也能逐点核对。
    #[test]
    fn 命中判据逐点() {
        let ctx = crate::fonts::measure_ctx();
        let screen = egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(crate::metrics::WINDOW_W, crate::metrics::WINDOW_H),
        );
        let p = egui::Painter::new(ctx, egui::LayerId::debug(), screen);
        let l = Layout::compute(screen);
        let labels = crate::menubar::item_rects(&p, &l);

        // 收起态：标签上 → Label(i)；画布中心 → Elsewhere
        let closed = MenuState::default();
        for (i, r) in labels.iter().enumerate() {
            assert_eq!(hit_test(&p, &l, &closed, r.center()), Hit::Label(i));
        }
        assert_eq!(
            hit_test(&p, &l, &closed, l.viewport.center()),
            Hit::Elsewhere
        );

        // 展开「文件」：弹层里第 0 项 → Item(0)；弹层空白（分隔线）→ Popup；
        // 弹层外的画布 → Elsewhere；弹层盖住的那块菜单栏下方 → Elsewhere
        let mut open = MenuState::default();
        open.open(0);
        let g = popup::geom(&p, &l, 0);
        let first = g.rows.iter().find(|r| !r.sep).expect("第一项");
        assert_eq!(hit_test(&p, &l, &open, first.rect.center()), Hit::Item(0));
        let sep = g.rows.iter().find(|r| r.sep).expect("分隔线");
        assert_eq!(hit_test(&p, &l, &open, sep.rect.center()), Hit::Popup);
        assert_eq!(
            hit_test(&p, &l, &open, l.viewport.center()),
            Hit::Elsewhere
        );
        // 展开态下点**别的**标签仍然命中标签（切换菜单靠它）
        assert_eq!(hit_test(&p, &l, &open, labels[3].center()), Hit::Label(3));
    }

    /// **子菜单几何与命中**：高亮父项 ⇒ 子面板出现，命中判据落在**子行**上。
    ///
    /// 这条挡的是三件只有真交互才会暴露的事：
    /// ① 子面板必须挂在父弹层**右侧**（重叠就点不到）；
    /// ② 子行的中心点必须命中 `Hit::Sub(i)`（否则点了没反应）；
    /// ③ 子面板不许越出窗口右边（语言菜单最靠右，最容易撞）。
    #[test]
    fn 子菜单几何与命中() {
        let ctx = crate::fonts::measure_ctx();
        let screen = egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(crate::metrics::WINDOW_W, crate::metrics::WINDOW_H),
        );
        let p = egui::Painter::new(ctx, egui::LayerId::debug(), screen);
        let l = Layout::compute(screen);
        let mut s = MenuState::default();

        // 视图菜单（下标 2）→ 地图光照（菜单内下标 2）
        s.open(2);
        s.hover = Some(2);
        let g = popup::geom_with(&p, &l, 2, s.active(), &[]);
        let sg = g.sub.as_ref().expect("地图光照必须展开子面板");
        assert_eq!(sg.parent, 2);
        assert_eq!(sg.rows.len(), 3, "黎明 / 黄昏 · 白天 · 夜晚");
        assert!(
            sg.rect.left() >= g.rect.right() - 0.01,
            "子面板必须挂在父弹层右侧：sub.left={} parent.right={}",
            sg.rect.left(),
            g.rect.right()
        );
        assert!(
            sg.rect.right() <= l.content.right() + 0.01,
            "子面板越出窗口右边：{} > {}",
            sg.rect.right(),
            l.content.right()
        );
        for row in &sg.rows {
            assert_eq!(
                hit_test(&p, &l, &s, row.rect.center()),
                Hit::Sub(row.index),
                "子行 {} 没命中自己",
                row.index
            );
        }

        // 视图菜单 → 语言（菜单内下标 8）：2 行
        s.hover = Some(8);
        let g = popup::geom_with(&p, &l, 2, s.active(), &[]);
        let sg = g.sub.as_ref().expect("语言必须展开子面板");
        assert_eq!(sg.rows.len(), 2, "简体中文 · English");
        assert!(sg.rect.right() <= l.content.right() + 0.01);

        // 文件菜单（下标 0）→ 最近打开（菜单内下标 2）：空态 1 行、无动作
        s.open(0);
        s.hover = Some(2);
        let g = popup::geom_with(&p, &l, 0, s.active(), &[]);
        let sg = g.sub.as_ref().expect("最近打开必须展开子面板");
        assert_eq!(sg.rows.len(), 1, "本仓没有最近地图 ⇒ 只有空态行");
        assert_eq!(sg.rows[0].label, crate::menu::model::SUB_RECENT_EMPTY);
        assert!(sg.rows[0].action.is_none(), "空态行没有动作（官方 IsEnabled=false）");
        assert_eq!(sg.rows[0].enabled, Some(false), "空态行灰显");

        // 叶子项（放大）没有子面板
        s.open(2);
        s.hover = Some(4);
        let g = popup::geom_with(&p, &l, 2, s.active(), &[]);
        assert!(g.sub.is_none(), "叶子项不该有子面板");
    }

    /// `--menu-hover` 的落点：`MenuState::focus` 把某项变成**高亮项**（子面板才会展开）。
    ///
    /// 没有它，`--shot`（锁死输入 ⇒ 没有 hover）就永远拍不到子菜单。
    #[test]
    fn focus设高亮项() {
        let mut s = MenuState::default();
        // 没展开菜单 ⇒ 设不上（先 open 才谈得上"菜单内下标"）
        assert!(!s.focus(2));
        s.open(2);
        assert!(s.focus(2), "视图菜单第 3 项（地图光照）应能设上");
        assert_eq!(s.active(), Some(2));
        // 越界 ⇒ 设不上，且不改现有高亮（不静默改状态）
        assert!(!s.focus(99));
        assert_eq!(s.active(), Some(2));
        // 换一项
        assert!(s.focus(8), "视图菜单第 9 项（语言）应能设上");
        assert_eq!(s.active(), Some(8));
        // 收起后 ⇒ 设不上
        s.close();
        assert!(!s.focus(0));
    }

    /// **子面板真的被画出来**（几何算对了但没上屏，等于没做）。
    ///
    /// 判据：`draw_with` 在"父项被高亮"时，egui 收到的图元里必须有一个矩形的宽高
    /// 与 [`popup::SubGeom::rect`] 一致 —— 那就是子面板底板（`paint::fill_round`）。
    ///
    /// ⚠ 为什么不用截图证明：`--shot` 运行会**锁死鼠标输入**（`app::input_locked`），
    /// 没有 hover 就没有 `active()` ⇒ 子面板不会展开；而"让某项成为高亮项"目前没有
    /// 命令行入口（`--menu-open` 只设 `open`，不设 `hover`/`kb`）。所以这一条用
    /// "真跑一次绘制、量图元"来钉（与 `grid::tests::画出来的都是1px线` 同一套做法）。
    #[test]
    fn 子面板真的被绘制() {
        let ctx = crate::fonts::measure_ctx();
        let screen = egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(crate::metrics::WINDOW_W, crate::metrics::WINDOW_H),
        );
        let l = Layout::compute(screen);
        let mut s = MenuState::default();
        s.open(2); // 视图
        s.hover = Some(2); // 地图光照
        let probe = egui::Painter::new(ctx.clone(), egui::LayerId::debug(), screen);
        let g = popup::geom_with(&probe, &l, 2, s.active(), &[]);
        let want = g.sub.as_ref().expect("地图光照子面板").rect;

        let out = ctx.run(egui::RawInput::default(), |ctx| {
            let p = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Background,
                egui::Id::new("menu-sub"),
            ));
            draw_with(
                &p,
                &l,
                &s,
                true,
                false,
                true,
                false,
                model::MapLight::Day,
                &PanelVis::default(),
                &SubState::default(),
            );
        });
        let mut found = false;
        for cs in &out.shapes {
            if let egui::Shape::Rect(r) = &cs.shape {
                if (r.rect.width() - want.width()).abs() < 0.5
                    && (r.rect.height() - want.height()).abs() < 0.5
                    && (r.rect.left() - want.left()).abs() < 0.5
                {
                    found = true;
                    break;
                }
            }
        }
        assert!(
            found,
            "子面板底板没画出来（要 {}×{} @ x={}）：图元里找不到",
            want.width(),
            want.height(),
            want.left()
        );
    }

    /// **`--menu-dump` 带 `parent` / `sep` 两列**（审查缺陷 8 的机器判据）。
    ///
    /// 四件事一起钉：
    /// ① 每个项行都带 `parent=` 与 `sep=`（缺一列就等于没加）；
    /// ② 顶级 40 行 `parent=-`，`sep=yes` 恰好 11 条（官方 `<Separator />` 总数）；
    /// ③ 子项行 `parent=<父项id>`：地图光照 3 + 语言 2 + 最近打开空态 1，且 `sep=-`；
    /// ④ 汇总行仍是 **40 口径**（子项不计入）。
    #[test]
    fn dump带parent与sep两列() {
        let panels = PanelVis::default();
        let lines = dump_lines(false, false, &panels);
        assert_eq!(
            lines.len(),
            1 + item_count() + 5 + 1 + 1,
            "表头 + 40 顶级行 + 5 子项行 + 1 空态行 + 汇总行"
        );
        let rows: Vec<&String> = lines.iter().filter(|l| l.contains(" | parent=")).collect();
        assert_eq!(rows.len(), item_count() + 5 + 1, "每个项行都要带 parent 列");
        for l in &rows {
            assert!(l.contains(" | sep="), "缺 sep 列：{l}");
        }
        // ② 顶级行
        let top = rows.iter().filter(|l| l.contains("parent=- ")).count();
        assert_eq!(top, item_count(), "顶级行必须是 40 条 parent=-");
        // 分隔线总数 = **11**：
        //   静态 8（实测 axaml 全文 `<Separator />` 只有 8 个：文件 2 / 编辑 2 / 视图 3 / 工具 1）
        // + 视图动态追加 3（`MainWindow.Localization.cs:15` 语言前 1 条、`axaml.cs:383` 面板前
        //   1 条、`axaml.cs:387` 锁定前 1 条）= 11，其中视图 3+3=6。
        // ⚠ 审查报告 §8.1 写「分隔线 12 条（视图 7）」是**多算了一条**（2026-11-05 复核：
        //   `[regex]::Matches($axaml,'<Separator\s*/>').Count == 8`，代码追加 3 条），
        //   本仓 `sep_before` 的 11 处才是对的 —— 以代码实测为准，报告那个数已过期。
        let seps = rows.iter().filter(|l| l.contains("sep=yes")).count();
        assert_eq!(
            seps, 11,
            "分隔线 11 条（静态 8 + 视图动态 3；报告写的 12 是多算了视图一条）"
        );
        // ③ 子项行
        let subs: Vec<&&String> = rows.iter().filter(|l| !l.contains("parent=- ")).collect();
        assert_eq!(subs.len(), 6, "5 个静态子项 + 1 条最近打开空态行");
        for id in ["view.light_dawn_dusk", "view.light_day", "view.light_night"] {
            assert!(
                rows.iter()
                    .any(|l| l.contains(id) && l.contains("parent=view.map_lighting")),
                "地图光照缺子项行：{id}"
            );
        }
        for id in ["view.language_zh_cn", "view.language_en"] {
            assert!(
                rows.iter()
                    .any(|l| l.contains(id) && l.contains("parent=view.language")),
                "语言缺子项行：{id}"
            );
        }
        assert!(
            rows.iter().any(|l| l.contains(crate::menu::model::ID_RECENT_EMPTY)
                && l.contains(crate::menu::model::SUB_RECENT_EMPTY)
                && l.contains("parent=file.open_recent")),
            "最近打开缺空态行"
        );
        for l in &subs {
            assert!(l.contains("sep=-"), "子项行的 sep 必须是 -（子菜单里没有分隔线）：{l}");
        }
        // ④ 汇总行：仍是 **40 口径**（子项不许计进去）
        //
        // 这里**不硬写** 11/8/20：那三个数会随"哪项接线了"变化（实测 2026-11-05 已经从
        // 报告里的 11/8/20 变成 12/8/19，因为「另存为」接线了）。要钉的是**不变量**：
        // 三档之和 == 40 == 顶级项数 ⇒ 子项与空态行没有被计进汇总。
        let tail = lines.last().expect("汇总行");
        let num_after = |prefix: &str| -> usize {
            tail.split(prefix)
                .nth(1)
                .and_then(|s| s.split(|c: char| !c.is_ascii_digit()).next())
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or_else(|| panic!("汇总行缺 {prefix} 的计数：{tail}"))
        };
        let (ready, need_map, todo) = (
            num_after("已实现 "),
            num_after("需先开图 "),
            num_after("未实现 "),
        );
        assert_eq!(
            ready + need_map + todo,
            item_count(),
            "汇总行必须仍是 40 口径（子项不许计进去）：{tail}"
        );
    }
}
