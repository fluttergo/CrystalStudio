//! **菜单动作的唯一执行入口**。
//!
//! 鼠标点击、`--menu-click`、弹层里的回车 —— 三条路径**都**走 [`apply`]，
//! 所以"点了之后到底发生了什么"只有一处实现（不会出现"命令行能跑、鼠标跑不了"）。
//!
//! # 三态在这里怎么落地
//!
//! ```text
//! State::Todo(why)      → 不动任何数据，回执「未实现：why」（状态栏警示色）
//! State::NeedsMap + 无图 → 不动任何数据，回执「需要先开图：…」
//! 其余                   → 真执行，回执写真实结果（成功/失败都带原因）
//! ```
//!
//! ⚠ **灰显项绝不许"悄悄返回"**：每条都产生一条回执，且 `apply` 的调用方
//! （[`crate::app::MireditApp`]）会把它写进状态栏并打一行 stderr 诊断 ——
//! 这就是验收门 §四.4「点一个灰显项 ⇒ 状态栏出现原因，且地图一字节不动」的机器判据。

use std::path::{Path, PathBuf};

use egui::Rect;
use miredit_render::MapLighting;

use crate::canvas_view::CanvasView;
use crate::edit::EditSession;
use crate::mapdoc::DocState;
use crate::menu::model::{self, Action, State};
use crate::menu::panels::{PanelId, PanelVis};
use crate::menu::settings::{self, Settings};
use crate::save::{self, SaveOutcome};
use crate::selection_ops::{self, Cmd as SelCmd};
use crate::theme;
use crate::ui_text as tx;

/// 宿主为一次菜单动作提供的可变上下文。
///
/// 全部是 `&mut`：菜单动作会真改文档/视口/会话/面板/主题，
/// 但它**不持有** app（否则 `app.rs` 又会被借来借去）。
pub struct Ctx<'a> {
    /// 文档 + 图库。
    pub doc: &'a mut DocState,
    /// 画布视口（缩放/适应都改它）。
    pub view: &'a mut CanvasView,
    /// 编辑会话（撤销/重做/填充都改它）。
    pub edit: &'a mut EditSession,
    /// 7 个面板的显隐。
    pub panels: &'a mut PanelVis,
    /// 深色模式开关（真源在 app，菜单只翻转它）。
    pub dark: &'a mut bool,
    /// **单元格网格开关**（真源在 app，菜单只翻转它；画布取的是同一个字段）。
    ///
    /// 为什么不放在 `view` 里：`CanvasView` 管的是"地图怎么贴"（平移/缩放/图层/纹理缓存），
    /// 网格是**叠加层开关**、不参与重渲缓存键（开了不必重渲地图），两者生命周期不同。
    pub grid: &'a mut bool,
    /// **开启门开关**（真源在 app，菜单只翻转它）。
    ///
    /// 为什么与 `grid` 分开而不是并进 `view`：`CanvasView` 的字段是"这一帧怎么贴"的输入，
    /// 而本开关是**菜单的勾选态真源**（`model::check_with` 读它），且要能被
    /// `--menu-click` / 菜单 / `--open-doors` 三条入口翻转 —— 与 `grid` 同一套口径。
    /// 画布那一侧只把它当参数收（`CanvasView::ensure(…, doors_open)`），不自己存一份，
    /// 免得"菜单勾着、画布没开门"这种两套状态。
    pub doors_open: &'a mut bool,
    /// **地图光照档位**（真源在 app，菜单三个子项改它）。
    ///
    /// 与 `doors_open` 同一套口径：它是**渲染选项 + 缓存键**的真源
    /// （`CanvasView::ensure` 收整个 `RenderOptions`），菜单勾选态、离屏导出读的都是这一个字段；
    /// 缺省 [`MapLighting::Day`] ⇒ 与接线前逐字节相同。
    /// 三个子项**互斥**由"恰好赋一个枚举值"结构性保证，不需要额外判据。
    pub lighting: &'a mut MapLighting,
    /// 画布矩形（缩放/适应要它；`Layout::viewport`）。
    pub canvas: Rect,
    /// 当前浮层（打开地图模态 / 快捷键浮层）。
    pub overlay: &'a mut crate::menu::Overlay,
    /// 设置文件路径（深色偏好 + 最近地图写这里）。
    pub settings_path: &'a Path,
    /// **最近打开的地图列表**（最近优先；真源 = 设置文件的 `recent`，宿主每帧读它）。
    ///
    /// 为什么放进 `Ctx`：打开成功后要**提升/追加**这一条并落盘
    /// （[`open_path`] → [`remember_recent`]），"点了一条已不存在的历史"要**移除**它
    /// （[`open_recent_index`]）—— 两件事都得改列表，而列表归宿主所有。
    pub recent: &'a mut Vec<String>,
    /// 当前选中的工具下标（换图后重建会话时要恢复它）。
    pub tool: usize,
}

/// 一次动作的宿主侧结果。
#[derive(Clone, Debug, Default)]
pub struct Outcome {
    /// 状态栏「上一次操作」回执：`(文案, 是否拒绝/警示色)`。
    pub receipt: Option<(String, bool)>,
    /// 地图是否被改过（`true` ⇒ 调用方要 `touch_map()` 重渲 + 刷新单元格信息）。
    pub touched_map: bool,
    /// 界面文案是否要重算（标题/状态栏/缩放百分比）。
    pub rebuild_text: bool,
    /// 是否请求重绘。
    pub repaint: bool,
}

impl Outcome {
    /// 一条拒绝回执（警示色）。
    fn refused(text: String) -> Self {
        Self {
            receipt: Some((text, true)),
            repaint: true,
            ..Self::default()
        }
    }

    /// 一条成功回执。
    fn ok(text: String) -> Self {
        Self {
            receipt: Some((text, false)),
            rebuild_text: true,
            repaint: true,
            ..Self::default()
        }
    }
}

/// 无图时点「需要先开图」的项，回执里那句话。
fn need_map_receipt() -> String {
    tx::fill(tx::STATUS_MENU_NEED_MAP_TMPL, &[("why", tx::WHY_NEED_MAP)])
}

/// 把当前设置（深色偏好 + 最近地图）落盘。
///
/// 失败只打一行诊断（[`model::DIAG_SETTINGS_WRITE_FAIL`]）—— **不回滚**刚做完的操作：
/// 深色已经翻过去了、地图已经打开了，为了一份偏好文件写不上而假装"没做"更糟。
fn save_settings(cx: &mut Ctx<'_>, dark: bool) {
    let s = Settings {
        dark,
        recent: cx.recent.clone(),
    };
    if let Err(e) = settings::save(cx.settings_path, &s) {
        eprintln!(
            "{}",
            tx::fill(
                model::DIAG_SETTINGS_WRITE_FAIL,
                &[
                    ("path", &cx.settings_path.display().to_string()),
                    ("err", &e),
                ]
            )
        );
    }
}

/// 把一条**刚写盘成功**的地图记进最近列表并落盘（最近优先、去重、上限 10）。
///
/// 三个调用点，都是"文档真的换到 / 落到了这个路径"之后：
/// * [`open_path`] 的成功分支 —— 打开一张图（失败分支早就 `return` 了，
///   所以"打不开的路径不会被记进历史"是**结构上**成立的，不靠额外判断）；
/// * [`save_as_path`] 的成功分支 —— 另存为把文档改认新路径之后
///   （官方 `MainWindow.axaml.cs:501` 也是 `RememberMap(target)`，与打开同一套）；
/// * 无窗 `--save-as`（[`crate::editrun::run`]）—— 它**不经过** `Ctx`，
///   所以直接调下面那个不依赖 `Ctx` 的 [`remember_recent_into`]。
///
/// 列表没变（这条已经在第一位）⇒ 不重复写盘。
fn remember_recent(cx: &mut Ctx<'_>, path: &Path) {
    // 先把"从 cx 借出来的东西"取成独立值：下面要同时把 `cx.recent` 借成 `&mut`
    let dark = *cx.dark;
    let settings_path = cx.settings_path.to_path_buf();
    let _ = remember_recent_into(&settings_path, dark, cx.recent, path);
}

/// [`remember_recent`] 的**不依赖 [`Ctx`]** 版本（无窗 `--save-as` 用）。
///
/// 语义与 GUI 路径**逐字相同**（同一份 `push_recent` + 同一份 `Settings` 整份重写 +
/// 同一条 `[最近]` 诊断行）—— 两条路径**不各写一套**，否则"命令行另存"与"菜单另存"
/// 会在最近列表上分成两种行为。
///
/// 返回列表是否真的变了（`false` = 这条已经在第一位，没写盘）。
pub fn remember_recent_into(
    settings_path: &Path,
    dark: bool,
    recent: &mut Vec<String>,
    path: &Path,
) -> bool {
    let text = path.display().to_string();
    if !settings::push_recent(recent, &text) {
        return false;
    }
    let s = Settings {
        dark,
        recent: recent.clone(),
    };
    if let Err(e) = settings::save(settings_path, &s) {
        eprintln!(
            "{}",
            tx::fill(
                model::DIAG_SETTINGS_WRITE_FAIL,
                &[
                    ("path", &settings_path.display().to_string()),
                    ("err", &e),
                ]
            )
        );
    }
    eprintln!(
        "{}",
        tx::fill(
            model::DIAG_RECENT_REMEMBERED,
            &[("n", &recent.len().to_string()), ("path", &text)]
        )
    );
    true
}

/// 执行一条菜单动作。
pub fn apply(action: Action, cx: &mut Ctx<'_>) -> Outcome {
    let has_map = cx.doc.doc.is_some();
    match model::state(action) {
        State::Todo(why) => {
            // **不动任何数据**：连 `repaint` 之外什么都不改（地图一字节不动）
            return Outcome::refused(tx::fill(tx::STATUS_MENU_TODO_TMPL, &[("why", why)]));
        }
        State::NeedsMap if !has_map => return Outcome::refused(need_map_receipt()),
        _ => {}
    }
    match action {
        // ───────────── 文件 ─────────────
        Action::OpenMap => {
            let start = cx
                .doc
                .doc
                .as_ref()
                .map(|d| d.path.display().to_string())
                .unwrap_or_else(|| crate::mapdoc::DEFAULT_MAP_PATH.to_owned());
            *cx.overlay = crate::menu::Overlay::OpenMap { path: start };
            Outcome::ok(tx::OPEN_DLG_TITLE.to_owned())
        }
        // 「最近打开」是**子菜单容器**（官方 `OpenRecentButton` 在 axaml 里没有 `Click`，
        // `MainWindow.RecentMaps.cs:23-40` 只往 `Items` 里填行）⇒ 派发它本身**不执行动作**。
        //
        // 鼠标点它 / 回车 = `Click::Toggled` / `KeyResult::Changed`（只展开/收起子菜单），
        // 走不到这条分支；这里只为 `--menu-click file.open_recent` 这种**直接派发**兜底，
        // 回一条"去哪选"的话（不是错误，所以不用警示色）。
        Action::OpenRecent => Outcome::ok(model::STATUS_RECENT_CONTAINER.to_owned()),
        Action::SaveMap => match save::save_session(cx.doc, cx.edit) {
            SaveOutcome::Written { path, bytes, .. } => {
                let name = path
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.display().to_string());
                Outcome {
                    touched_map: false,
                    ..Outcome::ok(tx::fill(
                        tx::MENU_SAVE_OK_TMPL,
                        &[("name", &name), ("bytes", &bytes.to_string())],
                    ))
                }
            }
            SaveOutcome::Clean => Outcome::ok(tx::MENU_SAVE_CLEAN.to_owned()),
            SaveOutcome::NoMap => Outcome::refused(need_map_receipt()),
            SaveOutcome::Failed(why) => {
                Outcome::refused(tx::fill(tx::MENU_SAVE_FAIL_TMPL, &[("why", &why)]))
            }
        },
        // **另存为**（2026-11-05 接线）：打开路径输入模态，初值 = 当前地图路径
        // （用户改个文件名最省事）。真正的写盘在模态回车后走 [`save_as_path`]。
        //
        // 没有地图时**不给模态**（输完路径也存不了），直接给一条带原因的回执 ——
        // 这就是本项选 `State::Ready` 而不是灰显的配套（见 `model::state` 的注释）。
        Action::SaveAs => {
            let Some(d) = cx.doc.doc.as_ref() else {
                return Outcome::refused(need_map_receipt());
            };
            *cx.overlay = crate::menu::Overlay::SaveAs {
                path: d.path.display().to_string(),
            };
            Outcome::ok(tx::SAVE_AS_DLG_TITLE.to_owned())
        }
        // **导出选区**（2026-11-05 接线）：打开路径输入模态，初值 = 地图同目录的
        // `<地图名>_选区.png`。真正的渲染与写盘在模态回车后走 [`export_selection_path`]。
        //
        // 没有地图时**不给模态**（输了路径也导不出），直接给一条带原因的回执 ——
        // 这就是本项选 `State::Ready` 而不是灰显的配套（见 `model::state` 的注释）。
        Action::ExportSelection => match cx.doc.doc.as_ref() {
            Some(d) => {
                *cx.overlay = crate::menu::Overlay::ExportSelection {
                    path: default_export_path(&d.path, "_选区"),
                };
                Outcome::ok(tx::EXPORT_SEL_DLG_TITLE.to_owned())
            }
            None => Outcome::refused(need_map_receipt()),
        },
        // **导出小地图**：同一套模态，初值 = `<地图名>_小地图.png`。
        Action::ExportMinimap => match cx.doc.doc.as_ref() {
            Some(d) => {
                *cx.overlay = crate::menu::Overlay::ExportMinimap {
                    path: default_export_path(&d.path, "_小地图"),
                };
                Outcome::ok(tx::EXPORT_MINI_DLG_TITLE.to_owned())
            }
            None => Outcome::refused(need_map_receipt()),
        },
        // ───────────── 编辑 ─────────────
        Action::Undo => {
            let n = match cx.doc.doc.as_mut() {
                Some(d) => cx.edit.undo(&mut d.map),
                None => 0,
            };
            if n > 0 {
                Outcome {
                    touched_map: true,
                    ..Outcome::ok(tx::fill(tx::MENU_UNDO_OK_TMPL, &[("n", &n.to_string())]))
                }
            } else {
                Outcome::refused(tx::MENU_UNDO_NONE.to_owned())
            }
        }
        Action::Redo => {
            let n = match cx.doc.doc.as_mut() {
                Some(d) => cx.edit.redo(&mut d.map),
                None => 0,
            };
            if n > 0 {
                Outcome {
                    touched_map: true,
                    ..Outcome::ok(tx::fill(tx::MENU_REDO_OK_TMPL, &[("n", &n.to_string())]))
                }
            } else {
                Outcome::refused(tx::MENU_REDO_NONE.to_owned())
            }
        }
        Action::FillSelection => {
            let Some(d) = cx.doc.doc.as_mut() else {
                return Outcome::refused(need_map_receipt());
            };
            match selection_ops::apply(cx.edit, &mut d.map, SelCmd::Fill, None) {
                Some(rec) => Outcome {
                    touched_map: rec.changed > 0,
                    receipt: Some((rec.text, rec.warn)),
                    rebuild_text: true,
                    repaint: true,
                },
                // 只有"粘贴时鼠标不在图上"才会 `None`；填充不会走到
                None => Outcome::refused(tx::SEL_ERR_NO_SELECTION.to_owned()),
            }
        }
        // ───────────── 视图：主题 ─────────────
        Action::DarkMode => {
            let on = !*cx.dark;
            *cx.dark = on;
            theme::set_dark(on);
            // 落盘时**必须带上最近地图列表**：`Settings` 是整份重写的，
            // 这里若只写 `dark`，用户的历史会被这次"切深色"悄悄清空。
            save_settings(cx, on);
            Outcome::ok(if on { tx::MENU_DARK_ON } else { tx::MENU_DARK_OFF }.to_owned())
        }
        // ───────────── 视图：单元格网格 ─────────────
        //
        // 三条入口（菜单点击 / `G` / `--menu-click view.cell_grid`）都落到这里，
        // 翻转的是**画布真正读的那个字段**（`crate::app::MireditApp::grid`），
        // 所以"命令行能开"= "菜单能开"= "G 能开"，不存在第三条假路径。
        Action::CellGrid => {
            let on = !*cx.grid;
            *cx.grid = on;
            Outcome::ok(if on {
                tx::MENU_CELL_GRID_ON
            } else {
                tx::MENU_CELL_GRID_OFF
            }
            .to_owned())
        }
        // ───────────── 视图：开启门 ─────────────
        //
        // 三条入口（菜单点击 / `--menu-click view.open_doors` / `--open-doors` 的初值）
        // 落到**同一个字段**（`crate::app::MireditApp::doors_open`），而它又是
        // `CanvasView::ensure` 的**缓存键 + 渲染选项**（`RenderOptions::doors_open`）
        // ⇒ 翻一下必然重渲一次，画面上门就开了/关了（不存在"勾了没反应"）。
        //
        // 门开合**只动前景层帧号**（`帧号 += door_offset & 0x7F`，且只对
        // `door_index & 0x7F != 0` 的格），地表/中层不受影响 —— 回执里点明这一句。
        Action::OpenDoors => {
            let on = !*cx.doors_open;
            *cx.doors_open = on;
            Outcome::ok(if on {
                tx::MENU_OPEN_DOORS_ON
            } else {
                tx::MENU_OPEN_DOORS_OFF
            }
            .to_owned())
        }
        // ───────────── 视图：面板显隐 ─────────────
        Action::Panel(id) => {
            let now = cx.panels.toggle(id);
            let tmpl = if now {
                tx::MENU_PANEL_SHOW_TMPL
            } else {
                tx::MENU_PANEL_HIDE_TMPL
            };
            Outcome::ok(tx::fill(tmpl, &[("name", id.title())]))
        }
        // ───────────── 视图：缩放 ─────────────
        Action::ZoomIn => zoom_by(cx, crate::metrics::ZOOM_PER_NOTCH),
        Action::ZoomOut => zoom_by(cx, 1.0 / crate::metrics::ZOOM_PER_NOTCH),
        Action::ActualSize => {
            let before = cx.view.scale;
            if before > 0.0 {
                let c = cx.canvas.center();
                cx.view.zoom_at(cx.canvas, c, 1.0 / before);
            }
            Outcome::ok(tx::MENU_ACTUAL_SIZE_OK.to_owned())
        }
        Action::FitMap => {
            let Some(d) = cx.doc.doc.as_ref() else {
                return Outcome::refused(need_map_receipt());
            };
            cx.view.fit_map(cx.canvas, d.map.width, d.map.height);
            Outcome::ok(tx::fill(
                tx::MENU_FIT_OK_TMPL,
                &[("pct", &cx.view.zoom_percent().to_string())],
            ))
        }
        // ───────────── 帮助 ─────────────
        Action::KeyboardShortcuts => {
            *cx.overlay = crate::menu::Overlay::Shortcuts;
            Outcome::ok(tx::SHORTCUTS_TITLE.to_owned())
        }
        // ───────────── 视图：地图光照（2026-11-05 接线） ─────────────
        //
        // 父项是**子菜单容器**（官方 `MapLightingMenu` 无 `Click`）：直接派发它本身
        // （`--menu-click view.map_lighting`）不切档，只说清去哪选 —— 与「最近打开」
        // 容器同一条口径，不是"点了没反应"。
        Action::MapLighting => Outcome::ok(tx::STATUS_LIGHT_CONTAINER.to_owned()),
        // 三个复选子项：改的是**宿主真源** `app.lighting`（= `Ctx::lighting`），
        // 下一帧 `CanvasView::ensure` 的缓存键与它不等 ⇒ 真重渲一次（见 `canvas_view` §七）。
        // 光照是渲染开关、不是编辑 ⇒ `touched_map` 保持 false，地图一格都不改。
        // 回执用中文点名档位（例如「地图光照：夜晚」）。
        Action::LightDawnDusk => {
            *cx.lighting = MapLighting::DawnDusk;
            Outcome::ok(tx::MENU_LIGHT_DAWN_DUSK.to_owned())
        }
        Action::LightDay => {
            *cx.lighting = MapLighting::Day;
            Outcome::ok(tx::MENU_LIGHT_DAY.to_owned())
        }
        Action::LightNight => {
            *cx.lighting = MapLighting::Night;
            Outcome::ok(tx::MENU_LIGHT_NIGHT.to_owned())
        }
        // 其余在 `model::state` 里已经是 `Todo`，上面第一个 `match` 就返回了。
        // 真走到这里说明"状态机与实现分支不一致"——**绝不 panic**（GUI 里 panic 等于崩给用户看），
        // 而是照样给一条带原因的拒绝回执，让用户与验收都能看见。
        other => Outcome::refused(tx::fill(
            tx::STATUS_MENU_TODO_TMPL,
            &[("why", model::state(other).todo_reason())],
        )),
    }
}

/// 缩放一档（锚点 = 画布中心，与缩放浮层的 +/− 完全一致）。
fn zoom_by(cx: &mut Ctx<'_>, factor: f32) -> Outcome {
    let c = cx.canvas.center();
    cx.view.zoom_at(cx.canvas, c, factor);
    Outcome::ok(tx::fill(
        tx::MENU_ZOOM_TMPL,
        &[("pct", &cx.view.zoom_percent().to_string())],
    ))
}

/// **打开一张地图**（`文件 → 打开地图…` 模态点「打开」时走这里）。
///
/// 走的是与启动时**完全相同**的 [`DocState::load_map`]（内部 `Document::load` 九格式判别），
/// 不是另写一条 `Map::parse` 旁路。失败时**保留当前地图**，只把原因写进状态栏。
pub fn open_path(cx: &mut Ctx<'_>, path: &str) -> Outcome {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Outcome::refused(tx::fill(tx::MENU_OPEN_FAIL_TMPL, &[("why", "路径为空")]));
    }
    let p = PathBuf::from(trimmed);
    let (doc, err) = DocState::load_map(&p);
    let Some(new_doc) = doc else {
        let why = err.unwrap_or_else(|| tx::VAL_UNKNOWN.to_owned());
        // 当前地图**保持原样**（不因为一次失败的打开丢掉手上的活）
        cx.doc.map_error = Some(why.clone());
        return Outcome::refused(tx::fill(tx::MENU_OPEN_FAIL_TMPL, &[("why", &why)]));
    };
    let name = new_doc.file_name.clone();
    cx.doc.doc = Some(new_doc);
    cx.doc.map_error = None;
    cx.doc.map_gen = cx.doc.map_gen.wrapping_add(1);
    // 换图 ⇒ 会话必须**重建**：撤销栈/选区/剪贴板都是上一张图的（拿旧增量改新图 = 串数据）
    let layer = cx.edit.layer();
    let brush = cx.edit.brush_size();
    let tool = cx.tool;
    *cx.edit = EditSession::default();
    cx.edit.select(tool);
    cx.edit.set_layer(layer);
    cx.edit.set_brush_size(brush);
    if let Some(d) = cx.doc.doc.as_ref() {
        if !cx.edit.bind_document(&d.path, d.orig_bytes()) {
            eprintln!(
                "{}",
                tx::fill(tx::DIAG_EDIT_BIND_FAIL, &[("path", &d.path.display().to_string())])
            );
        }
    }
    // 视口：丢掉旧纹理 + 整图适应（新图尺寸可能完全不同）
    cx.view.drop_cache();
    if let Some(d) = cx.doc.doc.as_ref() {
        cx.view.fit_map(cx.canvas, d.map.width, d.map.height);
    }
    // **记入最近地图**（2026-11-05 接线）：只有真把文档换掉之后才走到这里 ⇒
    // "打不开的路径不进历史"是结构上成立的（失败分支在上面已经 return）。
    remember_recent(cx, &p);
    *cx.overlay = crate::menu::Overlay::None;
    Outcome {
        touched_map: true,
        ..Outcome::ok(tx::fill(tx::MENU_OPEN_OK_TMPL, &[("name", &name)]))
    }
}

/// **打开「最近打开」子菜单里的第 `idx` 行**（0 基，列表顺序 = 最近优先）。
///
/// 三条判据（2026-11-05 单子）：
///
/// 1. 文件还在 ⇒ 走与「打开地图…」**完全相同**的 [`open_path`]
///    （不另写一条加载旁路，九格式判别/会话重建/视口适应全都同一份）；
/// 2. 文件**已不存在** ⇒ 状态栏给中文原因（[`model::STATUS_RECENT_GONE_TMPL`]）
///    **并把这一条从列表里移除**（官方 `RecentMaps.cs:29-35` 只把它灰掉、不删；
///    本仓有意偏离 —— 理由见那条常量的注释）；
/// 3. 下标越界 ⇒ 给一句"列表已变化"的原因，**不 panic**（GUI 里 panic 等于崩给用户看）。
///
/// 为什么下标由调用方传：`Action::OpenRecent` 不带下标（它是容器项），
/// 而"点的是哪一条历史"只有 `Click::RunRecent` / `KeyResult::RunRecent` 知道。
pub fn open_recent_index(cx: &mut Ctx<'_>, idx: usize) -> Outcome {
    let Some(path) = cx.recent.get(idx).cloned() else {
        return Outcome::refused(model::STATUS_RECENT_STALE.to_owned());
    };
    if !Path::new(&path).is_file() {
        // ① 从列表移除 + 落盘（顺序：先改内存，再写盘 —— 写失败也不至于让列表与界面不一致）
        settings::remove_recent(cx.recent, &path);
        save_settings(cx, *cx.dark);
        eprintln!(
            "{}",
            tx::fill(model::DIAG_RECENT_GONE, &[("path", &path)])
        );
        // ② 状态栏给中文原因（警示色：这是一次没打开成功的操作）
        return Outcome::refused(tx::fill(
            model::STATUS_RECENT_GONE_TMPL,
            &[("path", &path)],
        ));
    }
    open_path(cx, &path)
}

/// **另存为到指定路径**（`文件 → 另存为…` 模态点「另存为」/ 回车时走这里）。
///
/// 与 [`open_path`] 同一套骨架：先拦空路径、失败**保留当前文档与路径**、
/// 成功才把浮层关掉（失败时模态留着，用户改一下路径就能重试）。
///
/// 成功还会把**新路径**记进最近列表（[`remember_recent`]）—— 官方另存成功后同样
/// `RememberMap(target)`（`MainWindow.axaml.cs:501`），与打开地图走同一套。
///
/// 真正的写盘与"改认新路径"在 [`crate::save::save_as_session`]：它复用
/// [`crate::save::save_atomic`] 的原子写（tmp → `sync_all` → 读回核对 → `rename` →
/// 尽力 fsync 目录），成功后把文档路径与标题改成新路径并清脏标记。
pub fn save_as_path(cx: &mut Ctx<'_>, path: &str) -> Outcome {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        // 与 `save_as_session` 用**同一条**文案常量（不许各写一套）
        return Outcome::refused(tx::fill(
            tx::MENU_SAVE_AS_FAIL_TMPL,
            &[("why", tx::SAVE_AS_ERR_EMPTY)],
        ));
    }
    let target = PathBuf::from(trimmed);
    match save::save_as_session(cx.doc, cx.edit, &target) {
        SaveOutcome::Written { path, bytes, .. } => {
            *cx.overlay = crate::menu::Overlay::None;
            // **记入最近地图**（2026-11-05 补）：官方另存成功后也 `RememberMap(target)`
            // （`MainWindow.axaml.cs:501`），与 `OpenMap` 的 `:458/462` 同一套
            // ⇒ 本仓走与 [`open_path`] **完全相同**的 [`remember_recent`]
            // （最近优先 / 去重 / 上限 10 / 立刻落盘）。
            //
            // 位置：只有 `save_as_session` 真写成功才走到这里（失败分支在下面），
            // 所以"没存成的路径不进历史"同样是**结构上**成立的。
            remember_recent(cx, &path);
            Outcome {
                touched_map: false,
                ..Outcome::ok(tx::fill(
                    tx::MENU_SAVE_AS_OK_TMPL,
                    &[
                        ("path", &path.display().to_string()),
                        ("bytes", &bytes.to_string()),
                    ],
                ))
            }
        }
        SaveOutcome::NoMap => Outcome::refused(need_map_receipt()),
        // 另存为**照写**（见 `save_as_session` 的文档），所以没有 `Clean` 这一档；
        // 真出现就如实报，不假装存过。
        SaveOutcome::Clean => Outcome::refused(tx::MENU_SAVE_CLEAN.to_owned()),
        SaveOutcome::Failed(why) => Outcome::refused(tx::fill(
            tx::MENU_SAVE_AS_FAIL_TMPL,
            &[("why", &why)],
        )),
    }
}

/// 导出默认落点：`<地图所在目录>/<地图名><后缀>.png`。
///
/// 为什么给默认值而不是空串：模态是**真输入**（没有文件选择器，见 `menu::dialog` 文档），
/// 空串会让用户从零敲一个完整路径；给一个「就在地图旁边」的默认值，回车即可。
fn default_export_path(map: &Path, suffix: &str) -> String {
    let stem = map
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "map".to_owned());
    let name = format!("{stem}{suffix}.png");
    match map.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.join(name).display().to_string(),
        _ => name,
    }
}

/// 目标路径的**前置拦截**：空串 / 已存在目录 ⇒ 直接给原因，连临时文件都不写。
///
/// 返回 `Ok(是否已存在同名文件)` —— 那个布尔值决定回执里要不要加
/// [`tx::EXPORT_OVERWROTE`]（本仓对「目标已存在」的处理是**覆盖并回执**，
/// 不弹二次确认：模态本身就是一次明确的人工确认，再问一次只会让人多点一次）。
fn precheck_export_target(path: &str) -> Result<(PathBuf, bool), String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(tx::EXPORT_ERR_EMPTY.to_owned());
    }
    let target = PathBuf::from(trimmed);
    if target.is_dir() {
        return Err(tx::fill(tx::EXPORT_ERR_IS_DIR_TMPL, &[("path", trimmed)]));
    }
    Ok((target.clone(), target.exists()))
}

/// 把一次导出结果折成菜单回执（两个导出共用）。
fn export_outcome(
    cx: &mut Ctx<'_>,
    result: Result<crate::export::ExportReport, String>,
    existed: bool,
    tmpl: &str,
) -> Outcome {
    match result {
        Ok(rep) => {
            *cx.overlay = crate::menu::Overlay::None;
            let mut text = tx::fill(
                tmpl,
                &[
                    ("path", &rep.path.display().to_string()),
                    ("w", &rep.width.to_string()),
                    ("h", &rep.height.to_string()),
                    ("bytes", &rep.bytes.to_string()),
                    ("scale", rep.scale.label()),
                    ("tiles", &rep.tiles.to_string()),
                ],
            );
            if rep.two_stage {
                text.push_str(tx::EXPORT_TWO_STAGE);
            }
            if existed {
                text.push_str(tx::EXPORT_OVERWROTE);
            }
            Outcome {
                touched_map: false,
                ..Outcome::ok(text)
            }
        }
        // 失败时**模态留着**（与另存为同一口径：改一下路径就能重试）
        Err(why) => Outcome::refused(tx::fill(tx::MENU_EXPORT_FAIL_TMPL, &[("why", &why)])),
    }
}

/// **导出选区到指定路径**（模态点「导出」/ 回车时走这里）。
///
/// 渲染与写盘全在 [`crate::export::export_selection`]：选区取自
/// [`EditSession::selection`]（与画布上那个金色框**同一份数据**），
/// `scale = 1`、三层全开、不叠阻挡 ⇒ 与 `miredit-web` 的 `/api/tiles?...&scale=1`
/// 逐字节相同。没有选区时**不写文件**，回执里给原因。
///
/// **门态**（2026-11-05 补）：传 [`Ctx::doors_open`]（= 菜单「视图 → 开启门」的勾选态
/// 真源，也是画布渲染读的同一个字段）⇒ 「画布上看到门是开的」与「导出的 PNG 门是开的」
/// 是同一件事，不再是两副样子。缺省闭门 ⇒ 与接线前逐字节相同。
pub fn export_selection_path(cx: &mut Ctx<'_>, path: &str) -> Outcome {
    let (target, existed) = match precheck_export_target(path) {
        Ok(v) => v,
        Err(why) => return Outcome::refused(tx::fill(tx::MENU_EXPORT_FAIL_TMPL, &[("why", &why)])),
    };
    let sel = cx.edit.selection();
    let out = crate::export::export_selection(
        cx.doc,
        sel,
        &target,
        miredit_render::RenderOptions::with_doors_and_lighting(*cx.doors_open, *cx.lighting),
    );
    export_outcome(cx, out, existed, tx::MENU_EXPORT_SEL_OK_TMPL)
}

/// **导出小地图到指定路径**（模态点「导出」/ 回车时走这里）。
///
/// @acfbbf8a 回源订正：菜单路径与 CLI 缺省都走官方尺寸链
/// [`crate::export::export_minimap_sized`] + [`crate::export::MinimapSize::Default`]
/// （`W*12 × H*8`），**不再**经过本仓旧的 2048 长边两段式。
/// 整图渲不出来时自动分块，**采样仍由 render crate 做**；官方 16 级 overview mip
/// 到本仓 4 档的有据映射见 [`crate::export::pick_official_scale`]。
///
/// **门态**同 [`export_selection_path`]：传 [`Ctx::doors_open`]，缺省闭门。
pub fn export_minimap_path(cx: &mut Ctx<'_>, path: &str) -> Outcome {
    let (target, existed) = match precheck_export_target(path) {
        Ok(v) => v,
        Err(why) => return Outcome::refused(tx::fill(tx::MENU_EXPORT_FAIL_TMPL, &[("why", &why)])),
    };
    let out = crate::export::export_minimap_sized(
        cx.doc,
        &target,
        crate::export::MinimapSize::Default,
        miredit_render::RenderOptions::with_doors_and_lighting(*cx.doors_open, *cx.lighting),
    );
    export_outcome(cx, out, existed, tx::MENU_EXPORT_MINI_OK_TMPL)
}

/// 由 `--panel-hide` 之类的键把面板逐个关掉（返回真正认出来的键）。
pub fn hide_panels(panels: &mut PanelVis, keys: &[String]) -> Vec<String> {
    let mut ok = Vec::new();
    for k in keys {
        if let Some(id) = PanelId::from_key(k) {
            panels.set(id, false);
            ok.push(k.clone());
        }
    }
    ok
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::Overlay;

    /// 造一张 `w×h` 的 12B 地图文件（列优先；每格 `bk_img = 下标 + 1`）。
    fn 造图(tag: &str, w: u16, h: u16) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("miredit_gui_act_{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建临时目录");
        let p = dir.join(format!("{tag}.map"));
        let mut bytes = vec![0u8; mir2_map::HEADER_LEN + 12 * (w as usize * h as usize)];
        bytes[0..2].copy_from_slice(&w.to_le_bytes());
        bytes[2..4].copy_from_slice(&h.to_le_bytes());
        for i in 0..(w as usize * h as usize) {
            let base = mir2_map::HEADER_LEN + i * 12;
            bytes[base..base + 2].copy_from_slice(&((i as u16) + 1).to_le_bytes());
        }
        std::fs::write(&p, &bytes).expect("写临时地图");
        p
    }

    /// 一次菜单动作需要的全部宿主状态（`Ctx` 的借用方）。
    struct 夹具 {
        doc: DocState,
        view: CanvasView,
        edit: EditSession,
        panels: PanelVis,
        dark: bool,
        grid: bool,
        /// 开启门开关（`Ctx::doors_open` 的宿主侧真源；缺省闭门）。
        doors_open: bool,
        /// 地图光照档位（`Ctx::lighting` 的宿主侧真源；缺省白天）。
        lighting: MapLighting,
        overlay: Overlay,
        settings: PathBuf,
        recent: Vec<String>,
    }

    impl 夹具 {
        /// 打开一张真实文件（`MapDoc::open` 与生产路径同一条）。
        fn 打开(p: &Path) -> Self {
            let d = crate::mapdoc::MapDoc::open(p).expect("应能打开");
            let mut edit = EditSession::default();
            assert!(edit.bind_document(&d.path, d.orig_bytes()), "绑定真文档");
            edit.select(0);
            Self {
                doc: DocState {
                    doc: Some(d),
                    assets: crate::mapdoc::Assets::Failed("单元测试不扫资源库".to_owned()),
                    map_error: None,
                    map_gen: 1,
                },
                view: CanvasView::default(),
                edit,
                panels: PanelVis::default(),
                dark: false,
                grid: false,
                doors_open: false,
                lighting: MapLighting::Day,
                overlay: Overlay::None,
                settings: std::env::temp_dir().join("miredit_gui_act_settings.json"),
                recent: Vec::new(),
            }
        }

        /// 改一笔（让文档变脏，另存为才有"内容"可存）。
        fn 改一笔(&mut self) {
            let rep = crate::edit::apply_script(
                &mut self.doc,
                &mut self.edit,
                &[crate::edit::EditCmd::Pickup(0, 0), crate::edit::EditCmd::Brush(1, 1)],
            );
            assert!(rep.edited_cells > 0, "必须真改到格");
            assert!(self.edit.dirty());
        }

        fn ctx(&mut self, canvas: Rect) -> Ctx<'_> {
            Ctx {
                doc: &mut self.doc,
                view: &mut self.view,
                edit: &mut self.edit,
                panels: &mut self.panels,
                dark: &mut self.dark,
                grid: &mut self.grid,
                doors_open: &mut self.doors_open,
                lighting: &mut self.lighting,
                canvas,
                overlay: &mut self.overlay,
                settings_path: &self.settings,
                recent: &mut self.recent,
                tool: 0,
            }
        }
    }

    fn 画布() -> Rect {
        Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(crate::metrics::WINDOW_W, crate::metrics::WINDOW_H),
        )
        .shrink(80.0)
    }

    /// **菜单项「另存为」打开模态**：初值 = 当前地图路径（用户改个文件名最省事）。
    #[test]
    fn 另存为菜单项打开模态且初值是当前路径() {
        let a = 造图("dlg", 4, 4);
        let mut f = 夹具::打开(&a);
        let canvas = 画布();
        let out = apply(Action::SaveAs, &mut f.ctx(canvas));
        assert_eq!(
            f.overlay,
            Overlay::SaveAs {
                path: a.display().to_string()
            },
            "模态初值必须是当前地图路径"
        );
        let (text, warn) = out.receipt.clone().expect("要有回执");
        assert!(!warn, "打开模态是正常动作，不该用警示色");
        assert_eq!(text, tx::SAVE_AS_DLG_TITLE);
        let _ = std::fs::remove_dir_all(a.parent().unwrap());
    }

    /// **导出选区 / 导出小地图菜单项打开模态**（2026-11-05 接线）。
    ///
    /// 初值必须是「地图同目录 + 地图名 + 后缀 + `.png`」——模态是真输入
    /// （没有文件选择器），给个默认值才让「回车即可」成立。
    #[test]
    fn 导出菜单项打开模态且初值是地图同目录png() {
        let a = 造图("exportdlg", 4, 4);
        let dir = a.parent().unwrap().to_path_buf();
        let mut f = 夹具::打开(&a);
        let canvas = 画布();

        let out = apply(Action::ExportSelection, &mut f.ctx(canvas));
        assert_eq!(
            f.overlay,
            Overlay::ExportSelection {
                path: dir.join("exportdlg_选区.png").display().to_string()
            },
            "导出选区的模态初值必须是 <地图名>_选区.png"
        );
        let (text, warn) = out.receipt.clone().expect("要有回执");
        assert!(!warn, "打开模态是正常动作，不该用警示色");
        assert_eq!(text, tx::EXPORT_SEL_DLG_TITLE);

        let out = apply(Action::ExportMinimap, &mut f.ctx(canvas));
        assert_eq!(
            f.overlay,
            Overlay::ExportMinimap {
                path: dir.join("exportdlg_小地图.png").display().to_string()
            },
            "导出小地图的模态初值必须是 <地图名>_小地图.png"
        );
        let (text, warn) = out.receipt.clone().expect("要有回执");
        assert!(!warn);
        assert_eq!(text, tx::EXPORT_MINI_DLG_TITLE);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **没有地图时两个导出都不给模态**，只给带原因的回执（`State::Ready` 的配套）。
    #[test]
    fn 无图时导出只给原因不弹模态() {
        let mut f = 夹具 {
            doc: DocState {
                doc: None,
                assets: crate::mapdoc::Assets::Failed("无".to_owned()),
                map_error: None,
                map_gen: 0,
            },
            view: CanvasView::default(),
            edit: EditSession::default(),
            panels: PanelVis::default(),
            dark: false,
            grid: false,
            doors_open: false,
            lighting: MapLighting::Day,
            overlay: Overlay::None,
            settings: std::env::temp_dir().join("miredit_gui_act_settings.json"),
            recent: Vec::new(),
        };
        let canvas = 画布();
        for a in [Action::ExportSelection, Action::ExportMinimap] {
            let out = apply(a, &mut f.ctx(canvas));
            let (text, warn) = out.receipt.clone().expect("必须有回执");
            assert!(warn, "{} 无图是拒绝，要用警示色", a.id());
            assert!(text.contains("需要先开图"), "{} 回执没说清原因：{text}", a.id());
            assert_eq!(f.overlay, Overlay::None, "{} 无图时不该弹模态", a.id());
        }
    }

    /// 导出路径为空 / 是目录 ⇒ **回执带原因、一个文件都不写**（连临时文件都不写）。
    #[test]
    fn 导出路径非法时不写文件() {
        let a = 造图("exportbad", 4, 4);
        let dir = a.parent().unwrap().to_path_buf();
        let mut f = 夹具::打开(&a);
        let canvas = 画布();

        // 空串
        let out = export_selection_path(&mut f.ctx(canvas), "   ");
        let (text, warn) = out.receipt.clone().expect("必须有回执");
        assert!(warn);
        assert!(text.contains(tx::EXPORT_ERR_EMPTY), "回执没说清原因：{text}");

        // 已存在的目录
        let out = export_minimap_path(&mut f.ctx(canvas), &dir.display().to_string());
        let (text, warn) = out.receipt.clone().expect("必须有回执");
        assert!(warn);
        assert!(text.contains("是个目录"), "回执没说清原因：{text}");

        // 目录里只该有原地图文件，一个 PNG / 临时文件都不许有
        let mut names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        names.sort();
        assert_eq!(names, vec!["exportbad.map".to_string()], "失败时不许写任何文件");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **导出走的是真实现**：夹具没有图库 ⇒ 回执里必须是「没有图库」这个**真原因**，
    /// 而不是"未实现"或空串。这条挡的是"菜单点了但根本没接到 exporter 上"。
    #[test]
    fn 导出接到真实现上_没图库时报真原因() {
        let a = 造图("exportwired", 4, 4);
        let dir = a.parent().unwrap().to_path_buf();
        let mut f = 夹具::打开(&a);
        let canvas = 画布();
        f.edit
            .set_selection(&mut f.doc.doc.as_mut().unwrap().map, miredit_core::Selection::new(0, 0, 1, 1));
        let target = dir.join("out.png");
        let out = export_selection_path(&mut f.ctx(canvas), &target.display().to_string());
        let (text, warn) = out.receipt.clone().expect("必须有回执");
        assert!(warn);
        assert!(text.contains("没有图库"), "回执必须带真原因：{text}");
        assert!(!target.exists(), "失败时不许写文件");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **没有地图时不给模态**，只给一条带原因的回执（这就是本项选 `Ready` 的配套）。
    #[test]
    fn 无图时另存为只给原因不弹模态() {
        let mut f = 夹具 {
            doc: DocState {
                doc: None,
                assets: crate::mapdoc::Assets::Failed("无".to_owned()),
                map_error: None,
                map_gen: 0,
            },
            view: CanvasView::default(),
            edit: EditSession::default(),
            panels: PanelVis::default(),
            dark: false,
            grid: false,
            doors_open: false,
            lighting: MapLighting::Day,
            overlay: Overlay::None,
            settings: std::env::temp_dir().join("miredit_gui_act_settings.json"),
            recent: Vec::new(),
        };
        let canvas = 画布();
        let out = apply(Action::SaveAs, &mut f.ctx(canvas));
        let (text, warn) = out.receipt.clone().expect("必须有回执");
        assert!(warn, "无图是拒绝，要用警示色");
        assert!(text.contains("需要先开图"), "回执没说清原因：{text}");
        assert_eq!(f.overlay, Overlay::None, "无图时不该弹模态");
    }

    /// **另存为端到端（菜单路径）**：回车提交 → 新文件在、原文件不动、文档改认新路径、
    /// 回执写清新路径、模态关闭。
    #[test]
    fn 另存为提交后写盘并改认新路径() {
        let a = 造图("commit", 6, 6);
        let orig = std::fs::read(&a).expect("读原图");
        let mut f = 夹具::打开(&a);
        f.改一笔();
        let canvas = 画布();
        // ① 菜单项：打开模态
        let _ = apply(Action::SaveAs, &mut f.ctx(canvas));
        assert!(matches!(f.overlay, Overlay::SaveAs { .. }));
        // ② 模态里回车：把路径改成新路径再提交
        let b = a.parent().unwrap().join("b.map");
        let out = save_as_path(&mut f.ctx(canvas), &b.display().to_string());
        let (text, warn) = out.receipt.clone().expect("必须有回执");
        assert!(!warn, "成功回执不该是警示色：{text}");
        assert!(text.contains("已另存为"), "回执文案不对：{text}");
        assert!(text.contains("b.map"), "回执里要有新文件名：{text}");
        assert_eq!(f.overlay, Overlay::None, "成功后模态必须关闭");
        // ③ 磁盘与文档状态
        assert!(b.is_file(), "新文件必须真的在盘上");
        assert_eq!(std::fs::read(&b).unwrap().len(), orig.len(), "长度不变");
        assert_eq!(std::fs::read(&a).unwrap(), orig, "原文件必须一字节不动");
        let d = f.doc.doc.as_ref().expect("文档还在");
        assert_eq!(d.path, b, "文档必须改认新路径");
        assert_eq!(d.file_name, "b.map");
        assert!(!f.edit.dirty(), "成功后脏标记要清");
        let _ = std::fs::remove_dir_all(a.parent().unwrap());
    }

    /// **另存为失败（目标是目录）**：回执带原因、模态留着（好让用户改路径重试）、
    /// 原文件与文档路径都不动。
    #[test]
    fn 另存为失败时保留模态且不动原文件() {
        let a = 造图("faildir", 4, 4);
        let orig = std::fs::read(&a).expect("读原图");
        let mut f = 夹具::打开(&a);
        f.改一笔();
        let canvas = 画布();
        let _ = apply(Action::SaveAs, &mut f.ctx(canvas));
        let target = a.parent().unwrap().join("b.map");
        std::fs::create_dir_all(&target).unwrap();

        let out = save_as_path(&mut f.ctx(canvas), &target.display().to_string());
        let (text, warn) = out.receipt.clone().expect("必须有回执");
        assert!(warn, "失败要用警示色：{text}");
        assert!(text.contains("另存为失败"), "回执文案不对：{text}");
        assert!(text.contains("目录"), "原因要说清是目录：{text}");
        assert!(
            matches!(f.overlay, Overlay::SaveAs { .. }),
            "失败时模态要留着（用户改路径就能重试）"
        );
        assert_eq!(std::fs::read(&a).unwrap(), orig, "原文件必须一字节不动");
        assert_eq!(f.doc.doc.as_ref().unwrap().path, a, "失败不许改认路径");
        assert!(f.edit.dirty(), "失败不许清脏标记");
        let _ = std::fs::remove_dir_all(a.parent().unwrap());
    }

    /// 空路径：回执用的是**同一条**文案常量（`SAVE_AS_ERR_EMPTY`），且什么都不做。
    #[test]
    fn 另存为空路径只给原因() {        let a = 造图("empty", 4, 4);
        let orig = std::fs::read(&a).expect("读原图");
        let mut f = 夹具::打开(&a);
        f.改一笔();
        let canvas = 画布();
        let out = save_as_path(&mut f.ctx(canvas), "   ");
        let (text, warn) = out.receipt.clone().expect("必须有回执");
        assert!(warn);
        assert!(
            text.contains(tx::SAVE_AS_ERR_EMPTY),
            "回执要用同一条原因常量：{text}"
        );
        assert_eq!(std::fs::read(&a).unwrap(), orig, "一个字节都不许写");
        assert_eq!(f.doc.doc.as_ref().unwrap().path, a);
        let _ = std::fs::remove_dir_all(a.parent().unwrap());
    }

    /// **另存为成功也记进最近列表**（官方 `MainWindow.axaml.cs:501` 的
    /// `RememberMap(target)` —— 另存成功同样要记，不只是"打开"才记）。
    ///
    /// 判据：`--save-as` 到新路径后 `recent[0]` 是**新路径**（不是原路径），
    /// 且**已落盘**（`settings::load` 读回来还在）—— 只在内存里改不算。
    #[test]
    fn 另存为成功也记入最近列表() {
        let a = 造图("recent", 4, 4);
        let dir = a.parent().unwrap().to_path_buf();
        let mut f = 夹具::打开(&a);
        // 独享一份设置文件：`夹具::打开` 给的是 temp 下的公共路径，
        // 并行测试会互相覆盖（这条要读回文件，必须自己一份）
        f.settings = dir.join("settings.json");
        f.改一笔();
        let canvas = 画布();
        let b = dir.join("b.map");

        // before：列表空（原路径也没被记 —— 夹具是直接 `MapDoc::open`，没走 `open_path`）
        assert!(f.recent.is_empty(), "起点必须没有历史");

        let out = save_as_path(&mut f.ctx(canvas), &b.display().to_string());
        let (text, warn) = out.receipt.clone().expect("必须有回执");
        assert!(!warn, "成功回执不该是警示色：{text}");

        // after①：内存里首条 = 新路径（`push_recent` 存的是绝对化写法）
        let want = settings::normalize_path(&b.display().to_string());
        assert_eq!(
            f.recent.first().map(|s| s.as_str()),
            Some(want.as_str()),
            "最近列表首条必须是**新路径**，实际：{:?}",
            f.recent
        );
        // after②：真落盘了
        let on_disk = settings::load(&f.settings).expect("读回设置文件");
        assert_eq!(
            on_disk.recent.first().map(|s| s.as_str()),
            Some(want.as_str()),
            "设置文件里也要有新路径，实际：{:?}",
            on_disk.recent
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **另存为失败不许往历史里塞东西**（失败分支在 `remember_recent` 之前就 return 了）。
    #[test]
    fn 另存为失败不记入最近列表() {
        let a = 造图("recentfail", 4, 4);
        let dir = a.parent().unwrap().to_path_buf();
        let mut f = 夹具::打开(&a);
        f.settings = dir.join("settings.json");
        f.改一笔();
        let canvas = 画布();
        let target = dir.join("c.map");
        std::fs::create_dir_all(&target).unwrap(); // 目标是个目录 ⇒ 必失败

        let out = save_as_path(&mut f.ctx(canvas), &target.display().to_string());
        assert!(out.receipt.clone().expect("回执").1, "失败要用警示色");
        assert!(f.recent.is_empty(), "失败不许进历史：{:?}", f.recent);
        assert!(!f.settings.exists(), "失败连设置文件都不该写");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **「开启门」翻转宿主字段**（菜单 / `--menu-click` / `--open-doors` 三条入口的公共出口）。
    ///
    /// 判据：① 点一次 ⇒ `doors_open` 真翻转；② 回执说清开还是关；
    /// ③ **不动地图**（门态是渲染开关，不是编辑 ⇒ `map_gen` 与文档字节都不许变）。
    #[test]
    fn 开启门翻转开关且不动地图() {
        let a = 造图("doors", 4, 4);
        let dir = a.parent().unwrap().to_path_buf();
        let mut f = 夹具::打开(&a);
        let canvas = 画布();
        let gen = f.doc.map_gen;
        // 格 (0,0) 的**真值快照**（门态是渲染开关，格数据一格都不许变）
        let cell_before = f.doc.doc.as_ref().and_then(|d| d.map.cell(0, 0).copied());

        assert!(!f.doors_open, "缺省必须闭门（RenderOptions::default()）");

        // ① 开
        let out = apply(Action::OpenDoors, &mut f.ctx(canvas));
        assert!(f.doors_open, "菜单项没翻转开关");
        let (text, warn) = out.receipt.clone().expect("必须有回执");
        assert!(!warn, "开门不是拒绝");
        assert_eq!(text, tx::MENU_OPEN_DOORS_ON);
        assert!(text.contains('开'), "回执没说清开还是关：{text}");
        assert!(!out.touched_map, "门态是渲染开关，不许标成改图");

        // ② 关
        let out = apply(Action::OpenDoors, &mut f.ctx(canvas));
        assert!(!f.doors_open, "再点没关上");
        let (text, _) = out.receipt.clone().expect("回执");
        assert_eq!(text, tx::MENU_OPEN_DOORS_OFF);

        // ③ 地图一个字节都不动
        assert_eq!(f.doc.map_gen, gen, "开关门不许改地图代号");
        assert_eq!(
            f.doc.doc.as_ref().and_then(|d| d.map.cell(0, 0).copied()),
            cell_before,
            "开关门不许改格数据"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **「地图光照」三个子项切档**（2026-11-05 接线）：改的是宿主真源 `Ctx::lighting`、
    /// 回执是中文、`touched_map` 恒 false（渲染开关不改地图），且三档互斥；
    /// 父项是容器，直接派发只给一句"去哪选"。
    #[test]
    fn 地图光照三子项切档且不动地图() {
        let a = 造图("lighting", 4, 4);
        let dir = a.parent().unwrap().to_path_buf();
        let mut f = 夹具::打开(&a);
        let canvas = 画布();
        let gen = f.doc.map_gen;
        let cell_before = f.doc.doc.as_ref().and_then(|d| d.map.cell(0, 0).copied());

        assert_eq!(f.lighting, MapLighting::Day, "缺省必须白天");
        for (a, want, 回执) in [
            (Action::LightDawnDusk, MapLighting::DawnDusk, tx::MENU_LIGHT_DAWN_DUSK),
            (Action::LightDay, MapLighting::Day, tx::MENU_LIGHT_DAY),
            (Action::LightNight, MapLighting::Night, tx::MENU_LIGHT_NIGHT),
        ] {
            let out = apply(a, &mut f.ctx(canvas));
            assert_eq!(f.lighting, want, "{} 没切到档位", a.id());
            let (text, warn) = out.receipt.clone().expect("必须有回执");
            assert!(!warn, "切档不是拒绝");
            assert_eq!(text, 回执, "{} 回执文案不对", a.id());
            assert!(text.contains("地图光照"), "回执要点名档位所属功能：{text}");
            assert!(!out.touched_map, "光照是渲染开关，不许标成改图");
        }
        // 父项容器：不切档、只给一句"去哪选"
        let out = apply(Action::MapLighting, &mut f.ctx(canvas));
        assert_eq!(f.lighting, MapLighting::Night, "容器项不许切档");
        assert_eq!(out.receipt.clone().expect("回执").0, tx::STATUS_LIGHT_CONTAINER);
        // 三态守卫：父项与三子项全部可点（`state` 已是 Ready，不是 Todo）
        for a in [
            Action::MapLighting,
            Action::LightDawnDusk,
            Action::LightDay,
            Action::LightNight,
        ] {
            assert!(matches!(model::state(a), model::State::Ready), "{} 必须已实现", a.id());
        }
        // 地图一个字节都不动
        assert_eq!(f.doc.map_gen, gen, "切光照不许改地图代号");
        assert_eq!(
            f.doc.doc.as_ref().and_then(|d| d.map.cell(0, 0).copied()),
            cell_before,
            "切光照不许改格数据"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
