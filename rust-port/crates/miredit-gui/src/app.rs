//! 应用主体：把各模块的绘制与交互串起来。
//!
//! 绘制顺序（自底向上）：
//! 窗口底 → 左右分隔线 → 左停靠区 → 中列（页签条/真实地图/悬停高亮/缩放浮层）→ 右停靠区
//! → 菜单栏 → 标题栏 → 状态栏 → 关闭拦截模态（有未保存修改时才画）。
//!
//! # 每帧的固定次序（**别调换，有因果**）
//!
//! 1. **快捷键 + 关窗拦截**（`Ctrl+Z/Y/S`、`Esc`）——必须在交互之前，这样本帧的撤销/保存
//!    就参与本轮布局与重渲判定；
//! 2. **自动化驱动**（`--autodrag` 时按帧平移）；
//! 3. **交互**（拖拽/滚轮/点击/图层开关/**工具落格**）；
//! 4. **悬停格**（由交互后的视口算出）；
//! 5. **重渲调度**（[`CanvasView::ensure`]，只在视口/图层/地图变化时真渲）；
//! 6. **装配文案**（`screen_text::build`，全部来自 `ui_text` 模板）；
//! 7. **绘制**；
//! 8. **截图/拖动的自动化收尾**。
//!
//! # 本单接线的编辑闭环（详见 [`crate::edit`] / [`crate::save`]）
//!
//! | 动作 | 入口 | 粒度 |
//! |---|---|---|
//! | 吸管取格 | 左键点画布（工具 = 吸管）| 即时 |
//! | 笔刷落格 | 左键按下并拖动（工具 = 画笔）| 一次拖动 = 一步撤销；**每格按 N×N 落** |
//! | 橡皮清图号 | 左键按下并拖动（工具 = 橡皮）| 同上 |
//! | **填充** | 左键点画布（工具 = 填充，快捷键 `F`）| **整次填充 = 一步撤销** |
//! | **笔刷尺寸** | `[` / `]`（各 ±2，夹 3..=31）| 只改落笔范围，**不重渲** |
//! | 撤销 / 重做 | `Ctrl+Z` / `Ctrl+Y` | 一步 |
//! | 保存写回 | `Ctrl+S`（原子替换，仅在 `dirty` 时真写）| — |
//! | 关闭拦截 | 右上角 `×` / 系统关窗 | `dirty` 时先弹模态 |
//!
//! ## 印章放置（2026-11-05 新增，详见 [`crate::stamp_place`]）
//!
//! | 动作 | 入口 | 说明 |
//! |---|---|---|
//! | 选中一枚印章 | 资源面板「分类」页签里点缩略图 | 只选中，**不放置** |
//! | 进入**待放置态** | `Enter`（搜索框有焦点时不抢）| 状态栏 + 画布徽标双提示 |
//! | **落位** | 待放置态下左键点画布 | 读 `.X` → 注册 → 落位；**整次 = 一步撤销** |
//! | 看轮廓 | 待放置态下**按住 `Alt`** | 只描边、不填像素 |
//! | 退出待放置态 | `Esc` / 点别的工具 / 落位成功 | — |
//!
//! ## 矩形选区 + 剪贴板（2026-11-05 新增，详见 [`crate::selection_ops`]）
//!
//! | 动作 | 入口 | 说明 |
//! |---|---|---|
//! | 画选区 | 工具列第 5 个「选择」+ 左键拖动 | 金色虚线框 + 半透明填充；**不置脏** |
//! | 复制 / 剪切 / 粘贴 | `Ctrl+C` / `Ctrl+X` / `Ctrl+V`（粘贴锚点 = 鼠标格按源奇偶对齐，内核 `Clipboard::anchor`）| 各一步撤销 |
//! | 删除选区内容 | `Delete` | 一步撤销 |
//! | 填充选区 | `Ctrl+Shift+V`（**无官方依据，本仓自定**）| 一步撤销 |
//! | 取消选区 | `Esc` | **不清剪贴板** |
//!
//! 所有几何由 [`Layout::compute`] 一次性算出，所有颜色来自 [`crate::theme`]。

use std::path::PathBuf;
use std::time::Instant;

use egui::{Align2, Rect, Sense, Vec2};
use mir2_assets::SpriteSource;
use miredit_render::{MapLighting, RenderOptions};

use crate::banner;
use crate::canvas::{self, CanvasCtx};
use crate::canvas_view::CanvasView;
use crate::cellinfo::{self, CellLines};
use crate::cli::Cli;
use crate::dock_left;
use crate::dock_right;
use crate::edit::{self, EditSession, Tool};
use crate::layout::Layout;
use crate::mapdoc::{Assets, DocState};
use crate::maplist::{self, MapListPanel};
use crate::menu::panels::{PanelId, PanelVis};
use crate::menu::{self, Overlay};
use crate::menubar;
use crate::metrics as m;
use crate::paint as pt;
use crate::palette::{PaletteAction, PaletteState};
use crate::stamp_panel::{self, StampAction, StampPanel};
use crate::save;
use crate::screen_text::{self, DocText};
use crate::selection_ops::{self, Cmd as SelCmd, Drag as SelDrag};
use crate::selfcheck;
use crate::stamp_place::{self, Armed};
use crate::statusbar;
use crate::theme as t;
use crate::titlebar;
use crate::ui_text as tx;

/// 自动化动作（截图 / 拖动实测 / 素材滚动实测）的状态机。
#[derive(Default)]
struct Auto {
    /// 截图落盘路径（`--shot`）。
    shot: Option<PathBuf>,
    /// 第几帧请求截图（`--frames`，默认 30）。
    frames: u64,
    /// 是否已发出截图请求（只发一次）。
    requested: bool,
    /// 截图是否已落盘。
    saved: bool,
    /// 客户端截图路径（`--gate2-client`）。
    gate2_client: Option<PathBuf>,
    /// 客户端截图的世界格中心（`--gate2-at`）。
    gate2_at: Option<(i32, i32)>,
    /// 拖动实测秒数（`--autodrag`）。
    autodrag: Option<f32>,
    /// 拖动开始时刻。
    drag_started: Option<Instant>,
    /// 拖动报告是否已打印（只打一次）。
    drag_reported: bool,
    /// 素材面板滚动实测秒数（`--palette-autoscroll`）。
    palette_scroll: Option<f32>,
    /// 素材滚动开始时刻。
    palette_started: Option<Instant>,
    /// 素材滚动报告是否已打印（只打一次）。
    palette_reported: bool,
    /// `--palette-report`：退出/截图前打一次素材面板统计。
    palette_report: bool,
    /// `--stamp-report`：退出/截图前打一次印章面板统计。
    stamp_report: bool,
}

/// `--stamp-kind/--stamp-search/--stamp-page/--stamp-pick` 的原始取值。
///
/// 为什么不能在建面板时直接应用：目录要读 5.09 MB JSON（48~83 ms），
/// 而首帧不能等它 —— 所以这些开关一律**等装载完成后再落**（见 [`apply_stamp_cli`]）。
#[derive(Clone, Debug, Default)]
struct PendingStamp {
    /// `--stamp-kind <all|unclassified|unlisted|英文 kind>`。
    kind: Option<String>,
    /// `--stamp-search <文字>`。
    search: Option<String>,
    /// `--stamp-page <1 基页码>`。
    page: Option<usize>,
    /// `--stamp-pick <当前页第几个，0 基>`。
    pick: Option<usize>,
    /// `--stamp-arm`：装载完成后进入待放置态（= 按 `Enter`）。
    arm: bool,
    /// `--stamp-place-at <x,y>`：装载完成后在 `(x,y)` 落位（隐含 `arm`）。
    place_at: Option<(u16, u16)>,
}

/// 编辑器应用状态。
pub struct MireditApp {
    /// 文档 + 图库。
    pub doc: DocState,
    /// 画布视口（平移/缩放/图层/纹理缓存）。
    pub view: CanvasView,
    /// 编辑会话（工具 / 笔刷 / 撤销重做 / 脏标记）。
    pub edit: EditSession,
    /// 素材面板（真图库枚举 / 缩略图缓存 / 虚拟滚动 / 搜索）。
    pub palette: PaletteState,
    /// 资源面板「分类」页签 = 印章浏览器（9,159 枚 `.X`，后台装载 + LRU 缩略图）。
    pub stamps: StampPanel,
    /// 当前选中的工具下标（0..10，对应 `icons::TOOLS`）。
    pub selected_tool: usize,
    /// 鼠标所在世界格（不在图上为 `None`）。
    pub hover: Option<(u16, u16)>,
    /// `--hover` 指定的伪造悬停格（截图取证用，优先于真实鼠标）。
    forced_hover: Option<(u16, u16)>,
    /// 当前单元格信息（缓存，见 [`Self::refresh_cell_lines`]）。
    lines: CellLines,
    /// 上面那份信息对应的 `(x, y, 地图代号)`；变了才重算（要问图库要帧，不能每帧算）。
    lines_key: Option<(u16, u16, u64)>,
    /// 这一帧装配好的全部动态文字。
    text: DocText,
    /// 首次布局后把视口对准目标格（只做一次）。
    pending_center: Option<(f32, f32)>,
    /// 是否已做过首次对准。
    centered: bool,
    /// 已渲染帧数（拖动实测的分母）。
    frames: u64,
    /// 帧间隔累计（毫秒）。
    frame_gap_ms: f64,
    /// `update()` 自耗时累计（毫秒）。
    update_ms: f64,
    /// 上一帧的时刻（算帧间隔）。
    last_frame: Option<Instant>,
    /// 最近一次布局算出的画布矩形（截图比对要用）。
    last_canvas: Rect,
    /// 最近一次布局算出的缩放浮层矩形（截图比对时要排除它）。
    last_overlay: Rect,
    /// 关闭拦截模态是否已经弹出（有未保存修改时点第一次 × 变成 `true`）。
    confirm_close: bool,
    /// 已发给窗口系统的标题（变了才发 `ViewportCommand::Title`，避免每帧发指令）。
    shown_title: String,
    /// 自动化状态机。
    auto: Auto,
    /// `--stamp-*` 的待应用项：印章目录是**后台装载**的，启动时还没就绪
    /// ⇒ 先存着，等装载完成的那一帧再落到面板上（只做一次）。
    pending_stamp: Option<PendingStamp>,
    /// **印章待放置态**（`None` = 不在待放置态）。
    ///
    /// 进入：选中一枚印章后按 `Enter`（[`Self::handle_keys`]）。
    /// 退出：点画布落位成功、按 `Esc`、或改选别的工具。
    armed: Option<Armed>,
    /// 矩形选区的拖动起点（`None` = 当前没在拖）。
    sel_drag: Option<SelDrag>,
    /// **最近一次选区 / 印章操作的回执**：`(文案, 是否拒绝)`。
    ///
    /// 为什么存在宿主里而不是会话里：内核的 `note` 会被状态栏「落笔」那一段消费，
    /// 若两段都说同一句话，用户会以为发生了两件事。所以选区/印章操作的回执由
    /// [`crate::selection_ops::take_note`] 从内核**取走**，存在这里，
    /// 由 `update` 填进 [`DocText::status_op`]；撤销/重做时清掉（它描述的是一步已不存在的操作）。
    op: Option<(String, bool)>,
    /// **菜单系统状态**（展开的是哪个菜单、键盘/鼠标高亮哪一项）。
    menu: menu::MenuState,
    /// **7 个面板的显隐**（视图菜单的 7 个开关；默认全显示）。
    panels: PanelVis,
    /// **深色模式开关**（真源；`theme::set_dark` 由 `dispatch_menu` 同步）。
    dark: bool,
    /// **单元格网格开关**（真源；`--cell-grid` / `--no-cell-grid` 置初值，`G` / 菜单 /
    /// `--menu-click` 翻转）。
    ///
    /// 默认**开**（官方 `MainWindow.axaml:5` 的 `IsChecked="True"`；2026-11-05 之前本仓默认关，
    /// 审查判定为形态偏离后订正 —— 判据在 [`crate::cli::Cli::cell_grid_enabled`]，
    /// 要无网格基线请显式传 `--no-cell-grid`）。
    /// 画布每帧读它决定画不画格线（[`crate::canvas::CanvasCtx::grid`]），
    /// 菜单弹层的勾也读它（[`crate::menu::model::check`]）—— **同一个字段，不许两套**。
    grid: bool,
    /// **开启门开关**（真源；`--open-doors` 置初值，菜单 / `--menu-click view.open_doors` 翻转）。
    ///
    /// 默认 **`false`** = 闭门（`miredit_render::RenderOptions::default()`）⇒ 与接线前
    /// **逐字节相同**（门偏移恒 0）。
    ///
    /// 三条纪律：
    /// 1. **同一个字段**：菜单勾（`model::check_with` 的 `doors`）、画布渲染
    ///    （`CanvasView::ensure` 的 `doors_open`，进缓存键 + `RenderOptions`）、
    ///    `--open-doors` 的初值 —— 三处读的都是它，不许有第二份；
    /// 2. **进缓存键**：门开合只改前景层帧号，视口/图层/地图都没变 ⇒ 不进键就不会重渲
    ///    （见 `canvas_view` 模块文档 §六）；
    /// 3. **不改地图**：门态是渲染开关，不是编辑 ⇒ 不许 `touch_map()`。
    doors_open: bool,
    /// **地图光照档位**（真源；`--lighting` 置初值，菜单三子项 / `--menu-click
    /// view.light_*` 切档）。
    ///
    /// 缺省 [`MapLighting::Day`]（render crate 的 `RenderOptions::default()`）⇒ 与接线前
    /// **逐字节相同**（白天档连遮罩都不画）。
    ///
    /// 四条纪律（与 `doors_open` 同一套口径）：
    /// 1. **同一个字段**：菜单勾（`model::check_with` 的 `lighting`）、画布渲染
    ///    （`CanvasView::ensure` 的 `RenderOptions`，进缓存键 + 渲染选项）、导出
    ///    （`export::export_*` 收同一份 `RenderOptions`）、`--lighting` 的初值 ——
    ///    四处读的都是它，不许有第二份；
    /// 2. **进缓存键**：切档只叠遮罩，视口/图层/地图都没变 ⇒ 不进键就不会重渲；
    /// 3. **不改地图**：光照是渲染开关，不是编辑 ⇒ 不许 `touch_map()`；
    /// 4. **不持久化**：官方 SettingsStore 里没有这个偏好（见 `miredit_render::lighting`
    ///    的模块文档），本仓同样只在内存里留这一档。
    lighting: MapLighting,
    /// 当前浮层（打开地图模态 / 快捷键浮层）。
    overlay: Overlay,
    /// 设置文件路径（`%LOCALAPPDATA%\miredit\settings.json`，可用 `--settings` 覆盖）。
    settings_path: PathBuf,
    /// **最近打开的地图列表**（最近优先；真源 = 设置文件里的 `recent`）。
    ///
    /// 谁改它：
    /// * 启动时读一次（[`menu::settings::load`]）；
    /// * 打开成功 ⇒ [`menu::action::open_path`] 提升/追加并落盘；
    /// * 点了一条已不存在的历史 ⇒ [`menu::action::open_recent_index`] 移除并落盘。
    ///
    /// 谁读它：菜单「文件 → 最近打开」的子菜单行（[`Self::sub_state`]）——
    /// **画的行数与键盘能走的行数必须是同一份**，否则 `↑↓` 会走到不存在的行上。
    recent: Vec<String>,
    /// `--menu-click` 的待执行队列 `(第几帧执行, 菜单项 id)`。
    ///
    /// 为什么要等：首次布局算出画布矩形之后，缩放/适应才有锚点。
    /// 为什么要帧号：验收「同一会话里切换开关 ⇒ 画面重新渲染」必须**先渲一帧、再切**，
    /// 全部在第 1 帧执行时切换发生在首帧重渲之前，缓存键失效观察不到
    /// （语法见 [`crate::cli::parse_menu_click`]，缺省第 1 帧 = 旧行为）。
    pending_menu_clicks: Vec<(u64, String)>,
    /// `--open-map` 的待执行路径（要等首次布局算出画布矩形才能 fit）。
    pending_open_map: Option<PathBuf>,
    /// **地图列表面板**（2026-11-05，上游 `901dbe2f` 移植；状态 + 分帧扫描器）。
    ///
    /// 面板**显不显示**不归它管：真源是 `panels.get(PanelId::Maps)`（与视图菜单同一份），
    /// 这里只装"扫到了什么、滚到第几行"。两者分开的理由见 [`crate::maplist`] 模块文档 §四。
    maps: MapListPanel,
    /// `--map-list-root` 是否**钉住了**根（钉住后不再跟着当前地图换目录）。
    maps_root_pinned: bool,
}

/// **地图列表面板的扫描根**：`--map-list-root` > 当前地图所在目录 > `None`。
///
/// 为什么不复用 `menu::settings` 的 `LibraryFolder`（上游是从设置里的游戏目录
/// `DataPaths.FindMapFolder` 找 `Map` 子目录）：本仓的设置文件只存深色偏好 + 最近地图
/// （`menu::settings::Settings`），**没有**"游戏目录"这个概念；再为它加一个持久字段
/// 会把"设置文件格式"这件事也拖进本单。当前地图所在目录是**零成本且永远正确**的根
/// —— 你正在看哪张图，列表就列它旁边那些图。显式指定走 `--map-list-root`。
pub fn maps_root(cli: &Cli, doc: &DocState) -> Option<PathBuf> {
    if let Some(r) = cli.map_list_root.as_ref() {
        return Some(r.clone());
    }
    let p = doc.doc.as_ref()?.path.clone();
    match p.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => Some(dir.to_path_buf()),
        _ => None,
    }
}

/// **地表落笔前端拦截的判据**（纯函数：`app` 与验收用例共用这一份，绝不各写一套）。
///
/// # 为什么要在前端拦（而不是等内核拒绝）
///
/// 任务书 §三.2：「`ground_paint_supported == false`（Type5）⇒ 地表图层行加「不可落笔」标记，
/// 且**点画布前先拦**（别等 core 拒绝）」。理由：内核拒绝发生在 `touch()` 里，
/// 那时画笔已经 `begin()` 过一笔、指针已经走过一串格 —— 用户看到的是"画了半天没反应"，
/// 而内核只会给一句 `note`。前端拦则是"按下即说明原因"，且**一条命令都不发**。
///
/// # 判据（两条同时成立才拦）
///
/// 1. `MapDoc::ground_paint_supported() == false` —— 九种格式里只有 Type5 / `Mir3Wemade`；
/// 2. 当前编辑层写的是**地表**（`EditLayer::Ground`，落笔字段 `bk_img`）。
///    Type5 的中层 / 物体 / 低位阻挡 / 高位阻挡**照常可写**（`mid_img` / `fr_img` 都在格内）。
///
/// 返回 `Some(原因)` = 拦下，调用方**必须直接 return**（一条编辑命令都不许发）；
/// `None` = 放行。原因文案见 [`tx::STATUS_GROUND_BLOCKED_TMPL`]。
pub fn ground_block_reason(doc: &DocState, layer: edit::EditLayer) -> Option<String> {
    let d = doc.doc.as_ref()?;
    if d.ground_paint_supported() {
        return None;
    }
    if layer != edit::EditLayer::Ground {
        return None;
    }
    Some(tx::fill(
        tx::STATUS_GROUND_BLOCKED_TMPL,
        &[("fmt", d.format_label())],
    ))
}

impl MireditApp {
    /// 按命令行开关构造应用（**先装载再开窗**：标题栏/画布第一帧就是真实数据）。
    pub fn new(cli: &Cli) -> Self {
        let mut doc = DocState::load(cli.map.as_deref());

        let mut view = CanvasView::default();
        if let Some(s) = cli.scale {
            view.scale = s.clamp(m::CANVAS_MIN_SCALE, m::CANVAS_MAX_SCALE);
        }
        if let Some([g, mid, f]) = cli.layers {
            view.mask = miredit_render::LayerMask {
                ground: g,
                middle: mid,
                front: f,
            };
        }
        // `--collision`：初始**阻挡标记通道**（缺省两个都关 ⇒ 与上一版逐字节一致）。
        // 截图取证时鼠标被锁死，点不了眼睛，只能靠它把通道打开。
        if let Some(overlay) = cli.collision {
            view.overlay = overlay;
        }

        // 编辑会话：先按 `--tool`（缺省第 0 个 = 画笔）选中工具。
        let mut edit = EditSession::default();
        // ⓪ **把真文档绑进会话**（2026-11-05 九格式收口，必须在任何落笔/选区之前）：
        //    内核的库号字节还原、Type5 地表闸门、选区入口裁剪读的都是 `raw`（字节层），
        //    不绑定的话 `raw` 停在 1×1 `Old12` 占位，三处全按错误格式判定。
        //    失败要**说出来**（保存仍然正确，但"按格式判定的编辑语义"会退化成占位档位）。
        if let Some(d) = doc.doc.as_ref() {
            if !edit.bind_document(&d.path, d.orig_bytes()) {
                eprintln!(
                    "{}",
                    tx::fill(
                        tx::DIAG_EDIT_BIND_FAIL,
                        &[("path", &d.path.display().to_string())]
                    )
                );
            }
        }
        let selected_tool = cli.tool.unwrap_or(0).min(crate::icons::TOOLS.len() - 1);
        edit.select(selected_tool);
        // `--layer`：本次运行的**当前编辑层**（缺省 = 地表层）
        if let Some(layer) = cli.layer {
            edit.set_layer(layer);
        }
        // `--brush-size`：本次运行的**笔刷尺寸**（缺省 3；夹进 3..=31 的奇数）
        if let Some(n) = cli.brush_size {
            edit.set_brush_size(n);
        }

        // 素材面板：库列表来自**已装载的登记表**（装载失败 ⇒ 一个库都没有 ⇒ 面板走空态）。
        //
        // ⚠ 必须在 `--edit-*` 脚本**之前**建好并处理 `--palette-pick`：
        //   脚本里的 `--edit-brush` 要吃掉"当前笔刷"，而笔刷正是从素材面板选的。
        let mut palette = PaletteState::new(match &doc.assets {
            Assets::Ready { paths, .. } => Some(paths),
            Assets::Failed(_) => None,
        });
        apply_palette_cli(&mut palette, &mut edit, cli, doc.assets.source());

        // 再执行 `--edit-*` 脚本（GUI 里同样执行，截图取证靠它把"改动痕迹"画上去）。
        if !cli.edits.is_empty() {
            let _ = edit::apply_script(&mut doc, &mut edit, &cli.edits);
            // 脚本会按指令切工具，这里恢复成命令行/缺省选中的那个（状态栏显示的才是"当前工具"）
            edit.select(selected_tool);
        }

        // `--select <x0,y0,x1,y1>` / `--select-copy`：**走与鼠标拖动完全相同**的
        // `EditSession::set_selection`（内核在入口处裁剪一次），不是另写一条"直接赋值"的捷径
        // —— 否则命令行能造出"整块在图外"的非法选区，而鼠标造不出来。
        if let Some((x0, y0, x1, y1)) = cli.select {
            let mut ok = false;
            if let Some(d) = doc.doc.as_mut() {
                ok = edit.set_selection(&mut d.map, miredit_core::Selection::new(x0, y0, x1, y1));
            }
            if cli.select_copy {
                if let Some(d) = doc.doc.as_mut() {
                    let _ = edit.copy_selection(&mut d.map);
                }
            }
            let clip = match edit.clipboard() {
                Some(c) => format!("{}×{}", c.width(), c.height()),
                None => tx::VAL_UNKNOWN.to_owned(),
            };
            eprintln!(
                "{}",
                tx::fill(
                    tx::DIAG_SEL_CLI,
                    &[
                        ("x0", &x0.to_string()),
                        ("y0", &y0.to_string()),
                        ("x1", &x1.to_string()),
                        ("y1", &y1.to_string()),
                        ("ok", if ok { "1" } else { "0" }),
                        ("clip", &clip),
                    ]
                )
            );
        }

        let text = screen_text::build(&doc, &view, None, &CellLines::unknown(), &edit);

        // ── 主题 + 面板显隐 + 菜单初始态（2026-11-05） ──
        //
        // ① 设置文件：读失败**回落浅色**（`settings::load` 已把原因打进 stderr），绝不崩；
        // ② 命令行 `--theme` / `--dark` 覆盖设置文件（取证要能强制浅/深）；
        // ③ `theme::set_dark` 是绘制取色的唯一开关，必须在**任何一帧之前**设好。
        let settings_path = cli
            .settings
            .clone()
            .unwrap_or_else(menu::settings::default_path);
        // 一次读盘拿两个字段（深色偏好 + 最近地图列表）：读失败 ⇒ 回落浅色 + 空列表，
        // 并把这句原因打到 stderr（**不静默**，见 `menu::settings` 的三条纪律）。
        let (mut dark, recent) = match menu::settings::load(&settings_path) {
            Ok(s) => (s.dark, s.recent),
            Err(e) => {
                eprintln!(
                    "{}",
                    tx::fill(
                        tx::DIAG_SETTINGS_FALLBACK,
                        &[("path", &settings_path.display().to_string()), ("err", &e)]
                    )
                );
                (false, Vec::new())
            }
        };        if cli.dark {
            dark = true;
        }
        if let Some(d) = cli.theme {
            dark = d;
        }
        t::set_dark(dark);

        // 面板显隐：`--panel-hide` 逐个关（认不出的键在 cli 里已经报过错）
        let mut panels = PanelVis::default();
        let hid = menu::action::hide_panels(&mut panels, &cli.panel_hide);
        if !cli.panel_hide.is_empty() {
            eprintln!(
                "[面板] --panel-hide 生效 {} / {} 个：{}",
                hid.len(),
                cli.panel_hide.len(),
                hid.join(",")
            );
        }

        // `--show-maps`：展开地图列表面板（它默认关；截图/验收要它开着）。
        // 与视图菜单勾选走**同一个字段**（`PanelVis`），不另存一份开关。
        if cli.show_maps {
            panels.set(PanelId::Maps, true);
        }

        // 菜单初始态：`--menu-open` 展开某个菜单、`--shortcuts` 直接开浮层
        let mut menu_state = menu::MenuState::default();
        if let Some(i) = cli.menu_open {
            menu_state.open(i);
        }
        // `--menu-hover`：把某一项设为高亮项（子菜单取证用；只在 `--shot`/`--menu-open`
        // 同时给时生效，避免影响正常交互）。越界/没展开菜单 ⇒ 打一条诊断并忽略。
        if let Some(i) = cli.menu_hover {
            // 两个条件合并成一个 `if`（clippy `collapsible_if`）：`&&` 的求值顺序与
            // 嵌套 `if` 相同 —— 先看有没有 `--shot`/`--menu-open`，再看 `focus(i)`
            // 成不成立，所以 `focus` 的副作用（把第 i 项设为高亮）不会多发生一次。
            if (cli.shot.is_some() || cli.menu_open.is_some()) && !menu_state.focus(i) {
                eprintln!(
                    "{}",
                    tx::fill(
                        tx::DIAG_BAD_ARG,
                        &[("arg", &format!("--menu-hover {i}（当前展开的菜单没有这一项）"))]
                    )
                );
            }
        }
        // `--menu-sub-focus <父项>,<行>`：把**键盘焦点**送进子菜单并停在第 <行> 行
        // （子行高亮的截图取证入口；只在 `--shot` + `--menu-open` 同给时生效）。
        //
        // 走**真键盘路径**（`menu::handle_key_with`：先 `→` 进子菜单，再按 <行> 次 `↓`），
        // **不是**直接写 `MenuState::sub_focus`（那是私有字段，也不该为截图开后门）
        // ⇒ 截图证的是"用户按 → 再按 ↓ 之后屏幕上长什么样"，与手动操作同一条链。
        if let (Some(mi), Some((parent, row))) = (cli.menu_open, cli.menu_sub_focus) {
            if cli.shot.is_some() {
                let 有子菜单 = menu::model::MENUS
                    .get(mi)
                    .and_then(|m| m.items.get(parent))
                    .map(|it| it.sub.is_some())
                    .unwrap_or(false);
                let sub_state = menu::model::SubState {
                    recent: recent.clone(),
                    ..menu::model::SubState::default()
                };
                let mut ok = 有子菜单 && menu_state.focus(parent);
                if ok {
                    ok = menu::handle_key_with(&mut menu_state, menu::MenuKey::Right, &sub_state)
                        == menu::KeyResult::Changed;
                }
                for _ in 0..row {
                    if !ok {
                        break;
                    }
                    ok = menu::handle_key_with(&mut menu_state, menu::MenuKey::Down, &sub_state)
                        == menu::KeyResult::Changed;
                }
                if !ok || menu_state.sub_focus() != Some((parent, row)) {
                    eprintln!(
                        "{}",
                        tx::fill(
                            tx::DIAG_BAD_ARG,
                            &[(
                                "arg",
                                &format!(
                                    "--menu-sub-focus {parent},{row}（菜单 {mi} 第 {parent} 项没有这个子行，\
                                     或没展开/没有子菜单；当前 sub_focus={:?}）",
                                    menu_state.sub_focus()
                                )
                            )]
                        )
                    );
                }
            }
        }
        let overlay = if cli.shortcuts {
            Overlay::Shortcuts
        } else {
            Overlay::None
        };

        // ── 地图列表面板（2026-11-05，上游 `901dbe2f` 移植） ──
        //
        // 根：`--map-list-root` > 当前地图所在目录。扫描**立刻开始**（分帧推进，
        // 见 `maplist::Scanner`）：1,097 张实测 10~14 ms，摊到每帧 4 ms 只要 3~4 帧，
        // 所以"启动就开始扫"既不会拖慢开窗，也让 `--shot` 那一帧的列表**已经扫完**。
        let mut maps = MapListPanel::new();
        let maps_root_pinned = cli.map_list_root.is_some();
        if let Some(root) = maps_root(cli, &doc) {
            maps.start(&root);
        }

        Self {
            doc,
            view,
            edit,
            palette,
            stamps: StampPanel::new(),
            selected_tool,
            hover: None,
            forced_hover: cli.hover,
            lines: CellLines::unknown(),
            lines_key: None,
            text,
            pending_center: cli.center,
            centered: false,
            frames: 0,
            frame_gap_ms: 0.0,
            update_ms: 0.0,
            last_frame: None,
            last_canvas: Rect::NOTHING,
            last_overlay: Rect::NOTHING,
            confirm_close: false,
            shown_title: String::new(),
            auto: Auto {
                shot: cli.shot.clone(),
                frames: cli.frames.unwrap_or(30),
                gate2_client: cli.gate2_client.clone(),
                gate2_at: cli.gate2_at,
                autodrag: cli.autodrag,
                palette_scroll: cli.palette_autoscroll,
                palette_report: cli.palette_report,
                stamp_report: cli.stamp_report,
                ..Default::default()
            },
            pending_stamp: Some(PendingStamp {
                kind: cli.stamp_kind.clone(),
                search: cli.stamp_search.clone(),
                page: cli.stamp_page,
                pick: cli.stamp_pick,
                arm: cli.stamp_arm || cli.stamp_place_at.is_some(),
                place_at: cli.stamp_place_at,
            }),
            armed: None,
            sel_drag: None,
            op: None,
            menu: menu_state,
            panels,
            dark,
            // 单元格网格初值：官方 `MainWindow.axaml:5` 的 `IsChecked="True"` ⇒ **默认开**。
            // 优先级 `--no-cell-grid` > `--cell-grid` > 默认 true（判据在
            // [`crate::cli::Cli::cell_grid_enabled`]，不是在这里另写一套）。
            grid: cli.cell_grid_enabled(),
            // 开启门的初值：`--open-doors` ⇒ true，缺省 false（闭门，与改动前逐字节相同）。
            // 不产生回执：回执会改状态栏文字，那样"开/关两张图的像素差"就不止落在门帧上。
            doors_open: cli.open_doors,
            // 地图光照初值：`--lighting <档>` ⇒ 那一档；缺省 Day（与改动前逐字节相同）。
            // 不产生回执：回执会改状态栏文字，那样"三档截图的像素差"就不止落在遮罩上。
            lighting: cli.lighting,
            overlay,
            settings_path,
            recent,
            pending_menu_clicks: {
                // `--menu-click` 的值可能带 `@<帧号>` 后缀（见 `cli::parse_menu_click`）。
                // 帧号晚于截图帧 ⇒ 这一条**永远不会执行**，必须当场说出来（不静默）。
                let shot_frame = cli.frames.unwrap_or(30);
                let mut q = Vec::with_capacity(cli.menu_clicks.len());
                for raw in &cli.menu_clicks {
                    let (frame, id) = crate::cli::parse_menu_click(raw);
                    if cli.shot.is_some() && frame > shot_frame {
                        eprintln!(
                            "{}",
                            tx::fill(
                                tx::DIAG_CLICK_TOO_LATE,
                                &[
                                    ("id", id),
                                    ("frame", &frame.to_string()),
                                    ("shot", &shot_frame.to_string()),
                                ]
                            )
                        );
                    }
                    q.push((frame, id.to_owned()));
                }
                q
            },
            pending_open_map: cli.open_map.clone(),
            maps,
            maps_root_pinned,
        }
    }

    /// 这一帧的**子菜单状态**（菜单绘制 / 命中 / 键盘共用同一份）。
    ///
    /// 为什么每次都现建：`SubState` 里只有 `recent` 是运行期数据（语言取官方缺省，
    /// 光照档位已提到 [`Self::lighting`] 真源，见 [`menu::model::SubState`] 的文档），
    /// 而 `recent` 必须与 `self.recent` **同一份**
    /// —— 否则画出来的行数与键盘能走的行数会不一致。
    fn sub_state(&self) -> menu::model::SubState {
        menu::model::SubState {
            recent: self.recent.clone(),
            ..menu::model::SubState::default()
        }
    }

    /// 当前窗口标题（= 标题栏文字；由真实文件名与格数装配）。
    pub fn window_title(&self) -> &str {
        &self.text.title
    }

    /// 按当前窗口尺寸算布局（地图列表面板开着时画布让出一列）。
    ///
    /// **`maps_open` 取自 `panels.get(PanelId::Maps)`** —— 与视图菜单勾选、`--show-maps`、
    /// `--panel-hide maps` 是同一个字段，不存在"菜单勾着但画布不让位"这种两套状态。
    fn layout(&self, screen: Rect) -> Layout {
        Layout::compute_with(screen, self.panels.get(PanelId::Maps))
    }

    /// 首次布局后把视口对准目标：有 `--center` 用它，否则对准地图中心。
    fn init_view(&mut self, l: &Layout) {
        if self.centered {
            return;
        }
        if let Some((x, y)) = self.pending_center {
            self.view.center_on_cell(l.viewport, x, y);
        } else if let Some(d) = self.doc.doc.as_ref() {
            self.view.center_on_cell(
                l.viewport,
                d.map.width as f32 * 0.5,
                d.map.height as f32 * 0.5,
            );
        }
        self.centered = true;
    }

    /// 重渲调度：只在视口/图层/**渲染选项（门态 + 光照档）**/地图变化时真调渲染库
    /// （见 [`CanvasView::ensure`]）。
    fn ensure_texture(&mut self, ctx: &egui::Context, l: &Layout) {
        let map = self.doc.doc.as_ref().map(|d| &d.map);
        let src = self
            .doc
            .assets
            .source()
            .map(|s| s as &dyn SpriteSource);
        // `doors_open` / `lighting` 是**唯一**真源：菜单勾、缓存键、渲染选项、导出都读它们。
        // 打包成整份 `RenderOptions` 一次交给 `ensure`（它整体比较缓存键 + 原样传渲染库），
        // 两个 option 一起生效，不许拆成两条路径把门态弄丢。
        let options = RenderOptions::with_doors_and_lighting(self.doors_open, self.lighting);
        self.view
            .ensure(ctx, map, src, l.viewport, self.doc.map_gen, options);
    }

    /// 悬停格：`--hover` 优先，否则取真实指针位置并做地图范围校验。
    fn update_hover(&mut self, ctx: &egui::Context, l: &Layout) {
        let cell = if let Some((x, y)) = self.forced_hover {
            Some((i32::from(x), i32::from(y)))
        } else {
            ctx.pointer_hover_pos()
                .and_then(|p| self.view.screen_to_cell(l.viewport, p))
        };
        self.hover = cell.and_then(|(x, y)| match self.doc.doc.as_ref() {
            Some(d) => {
                if x >= 0 && y >= 0 && x < i32::from(d.map.width) && y < i32::from(d.map.height) {
                    Some((x as u16, y as u16))
                } else {
                    None
                }
            }
            None => None,
        });
        self.refresh_cell_lines();
    }

    /// 悬停格变了才重算六行信息（它要问图库要帧尺寸，每帧重算会把鼠标拖卡）。
    fn refresh_cell_lines(&mut self) {
        let key = self.hover.map(|(x, y)| (x, y, self.doc.map_gen));
        if key == self.lines_key {
            return;
        }
        self.lines_key = key;
        self.lines = match (self.hover, self.doc.doc.as_ref()) {
            (Some((x, y)), Some(d)) => {
                let src = self.doc.assets.source().map(|s| s as &dyn SpriteSource);
                cellinfo::cell_lines(x, y, d.map.cell(x, y), d.map.format, src)
            }
            _ => CellLines::unknown(),
        };
    }

    // ───────────────────────── 编辑：快捷键 / 落格 / 保存 ─────────────────────────

    /// 改写地图后统一走这里：**地图代号 +1** 且**强刷单元格信息**。
    ///
    /// 为什么用 `map_gen`：画布纹理缓存与单元格信息的缓存键里都有它
    /// （见 [`CanvasView::ensure`] 与 [`Self::refresh_cell_lines`]），
    /// 于是"改了图就重渲一次"是自动的，而且**只在真改过的帧里发生**——
    /// 不会每帧重渲（重渲一次在 0.25× 下要出 12000 格，约 20~40 ms）。
    fn touch_map(&mut self) {
        self.doc.map_gen = self.doc.map_gen.wrapping_add(1);
        self.lines_key = None;
    }

    /// 把「地表落笔被前端拦下」这件事**同时**报到 stderr 与状态栏回执段。
    ///
    /// 为什么要两个出口：状态栏只在窗口里看得见，而无人值守验收读的是 stderr ——
    /// "拦下了"必须有**可机器抓**的输出，否则只能靠人工看截图（与 [`tx::DIAG_EDIT_REJECT`]
    /// 同一条纪律）。
    ///
    /// ⚠ 回执用**警示色**（`warn = true`）：这是一次被拒绝的操作，不是成功回执。
    fn report_ground_blocked(&mut self, why: &str) {
        let fmt = self
            .doc
            .doc
            .as_ref()
            .map(|d| d.format_label())
            .unwrap_or(tx::VAL_UNKNOWN);
        eprintln!(
            "{}",
            tx::fill(tx::DIAG_EDIT_GROUND_BLOCKED, &[("fmt", fmt), ("why", why)])
        );
        self.op = Some((why.to_owned(), true));
    }

    /// `Ctrl+Z` / `Ctrl+Y` / `Ctrl+S` / `Esc` + 工具快捷键（菜单栏的下拉另单做）。
    ///
    /// 工具快捷键**照官方**（`MainWindow.axaml.cs:861`）：
    /// `Key.F => EditorTool.Bucket` —— 本单只接 `F`（填充）；
    /// 笔刷 `B` / 铅笔 `P` / 橡皮 `E` / 吸管 `V` 官方也有，但本仓的图标顺序与官方不同，
    /// 逐个接会引入"按 B 选到第 0 个还是第几个"的新歧义，故只接本单新增的那一个。
    ///
    /// 笔刷尺寸 `[` / `]`：官方 `MainWindow.axaml.cs:871`
    /// `BrushSize.Value = Math.Clamp(BrushSize.Value + (OemOpenBrackets ? -2 : 2), 3, 31);`
    /// ⇒ 一次走 2 格、夹在 3..31（见 [`EditSession::grow_brush_size`]）。
    fn handle_keys(&mut self, ctx: &egui::Context) {
        // ⓪ **浮层优先**：打开地图模态 / 快捷键浮层在的时候，键盘整体让给它
        //    （只留 Esc/回车/退格/文本，由 `menu::dialog` 处理）。
        //    不让位的话，敲路径里的 `f` 会切工具、`o` 会切印章 —— 那是真实踩过的坑。
        if self.overlay.is_some() {
            return;
        }
        // ⓪b **菜单键盘**：弹层展开时 ↑↓←→ / Enter / Esc 归菜单，且**吃掉这次按键**
        //     （否则 Esc 会顺带清选区、↑↓ 会滚画布）。
        if self.menu.open_index().is_some() {
            let key = ctx.input(|i| {
                if i.key_pressed(egui::Key::Escape) {
                    Some(menu::MenuKey::Escape)
                } else if i.key_pressed(egui::Key::ArrowDown) {
                    Some(menu::MenuKey::Down)
                } else if i.key_pressed(egui::Key::ArrowUp) {
                    Some(menu::MenuKey::Up)
                } else if i.key_pressed(egui::Key::ArrowLeft) {
                    Some(menu::MenuKey::Left)
                } else if i.key_pressed(egui::Key::ArrowRight) {
                    Some(menu::MenuKey::Right)
                } else if i.key_pressed(egui::Key::Enter) {
                    Some(menu::MenuKey::Enter)
                } else {
                    None
                }
            });
            if let Some(k) = key {
                let canvas = self.last_canvas;
                // 子菜单状态（`recent` 决定「最近打开」有几行可走）——
                // 键盘与绘制必须问**同一份**，否则 ↑↓ 会走到不存在的行上。
                let sub = self.sub_state();
                match menu::handle_key_with(&mut self.menu, k, &sub) {
                    menu::KeyResult::Run(a) => self.dispatch_menu(a, canvas),
                    // 回车选了一条最近地图 ⇒ 打开**那一条**（与鼠标点击同一条路）
                    menu::KeyResult::RunRecent(i) => self.open_recent_at(i, canvas),
                    menu::KeyResult::Changed => {}
                    menu::KeyResult::None => {}
                }
                ctx.request_repaint();
                return;
            }
        }
        // ⓪c `F1`：帮助 → 键盘快捷键（官方手势，见 `Action::KeyboardShortcuts`）
        if ctx.input(|i| i.key_pressed(egui::Key::F1)) {
            let canvas = self.last_canvas;
            self.dispatch_menu(menu::model::Action::KeyboardShortcuts, canvas);
            ctx.request_repaint();
            return;
        }
        // ⓪d `Ctrl+O`：文件 → 打开地图（官方手势）
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::O)) {
            let canvas = self.last_canvas;
            self.dispatch_menu(menu::model::Action::OpenMap, canvas);
            ctx.request_repaint();
            return;
        }
        // ⓪e `G`：视图 → 单元格网格（官方手势 `MainWindow.axaml` 的 `InputGesture="G"`）。
        //
        // 只认**不带修饰键**的裸 `G`：`Ctrl+G` / `Alt+G` 是别的程序里的习惯（本仓也没绑），
        // 抢过来只会让人误触。走的仍是 `dispatch_menu` ⇒ 与鼠标点菜单项**同一条**路径
        // （回执、诊断行、状态翻转全一致，不存在"键盘能开、菜单开不了"）。
        if ctx.input(|i| {
            !i.modifiers.command && !i.modifiers.alt && !i.modifiers.shift
                && i.key_pressed(egui::Key::G)
        }) {
            let canvas = self.last_canvas;
            self.dispatch_menu(menu::model::Action::CellGrid, canvas);
            ctx.request_repaint();
            return;
        }
        let (undo, redo, save, save_as, esc) = ctx.input(|i| {
            (
                // 撤销：`Ctrl+Z`，且**不带 Shift**（带 Shift 是重做）
                i.modifiers.command && !i.modifiers.shift && i.key_pressed(egui::Key::Z),
                // 重做：官方 `Ctrl+Shift+Z`，本仓另接受旧键 `Ctrl+Y`（两者都真实可用，
                // 快捷键浮层里也是这么写的）
                i.modifiers.command
                    && ((i.modifiers.shift && i.key_pressed(egui::Key::Z))
                        || i.key_pressed(egui::Key::Y)),
                // 保存：`Ctrl+S`，**不带 Shift**（带 Shift 是另存为，见下一条）
                i.modifiers.command && !i.modifiers.shift && i.key_pressed(egui::Key::S),
                // **另存为**：官方 `Ctrl+Shift+S`（`MainWindow.axaml` 的 `InputGesture`）。
                // 2026-11-05 起真绑定：走与鼠标点「文件 → 另存为…」**完全相同**的
                // `dispatch_menu(Action::SaveAs)`（打开同一个路径输入模态），不另写一条捷径。
                i.modifiers.command && i.modifiers.shift && i.key_pressed(egui::Key::S),
                i.key_pressed(egui::Key::Escape),
            )
        });
        // 工具 / 尺寸快捷键：带 Ctrl/Alt 时不算（那是别的命令的修饰键）
        let (fill_key, select_key, stamp_key, shrink, grow) = ctx.input(|i| {
            let plain = !i.modifiers.command && !i.modifiers.alt;
            (
                plain && i.key_pressed(egui::Key::F),
                // 官方 `MainWindow.axaml.cs:861`：`Key.M => EditorTool.Select`、`Key.O => EditorTool.Stamp`
                plain && i.key_pressed(egui::Key::M),
                plain && i.key_pressed(egui::Key::O),
                plain && i.key_pressed(egui::Key::OpenBracket),
                plain && i.key_pressed(egui::Key::CloseBracket),
            )
        });
        if fill_key {
            self.selected_tool = edit::TOOL_BUCKET_IDX;
            self.edit.select(edit::TOOL_BUCKET_IDX);
            ctx.request_repaint();
        }
        // `M` / `O`：选择 / 印章（两支宿主层工具，内核 `Tool` 里没有；见 `selection_ops` §一）
        for (hit, slot) in [
            (select_key, selection_ops::TOOL_SELECT_IDX),
            (stamp_key, selection_ops::TOOL_STAMP_IDX),
        ] {
            if hit {
                self.selected_tool = slot;
                self.edit.select(slot);
                ctx.request_repaint();
            }
        }
        if shrink || grow {
            // `[` 减 2、`]` 加 2（官方同一行代码里的三元表达式）
            let delta = if shrink {
                -(m::BRUSH_STEP as isize)
            } else {
                m::BRUSH_STEP as isize
            };
            let before = self.edit.brush_size();
            let now = self.edit.grow_brush_size(delta);
            eprintln!(
                "{}",
                tx::fill(tx::DIAG_EDIT_SIZE, &[("n", &now.to_string())])
            );
            // ⚠ **不 `touch_map()`**：改尺寸不改地图一个字节 ⇒ 不该让画布丢纹理缓存重渲一遍
            //    （见 [`EditSession::set_brush_size`] 的文档）。只重画界面（胶囊文字变了）。
            if now != before {
                ctx.request_repaint();
            }
        }
        // ── 选区 / 剪贴板快捷键（2026-11-05） ──
        //
        // 判定顺序有因果：`Ctrl+Shift+V`（填充选区）必须**先于** `Ctrl+V`（粘贴）判，
        // 否则 shift 按住时两个条件同时成立，会先粘贴再填充。
        self.handle_sel_keys(ctx);
        // ── 印章待放置态：`Enter` 进入 / `Esc` 退出 ──
        self.handle_arm_keys(ctx);
        if esc {
            self.confirm_close = false;
            // `Esc` 的第三层语义：取消选区（官方 `Deselect`，`MapViewport.cs:103-108`）。
            // 顺序：先关模态 → 再退待放置态 → 最后才清选区（越"重"的越先撤）。
            if self.armed.is_none() && self.edit.selection().is_some() {
                self.edit.clear_selection();
                ctx.request_repaint();
            }
        }
        if undo {
            let n = match self.doc.doc.as_mut() {
                Some(d) => self.edit.undo(&mut d.map),
                None => 0,
            };
            // 撤销之后"上一次操作的回执"就不再成立了（它描述的那一步已经没了）
            self.op = None;
            if n > 0 {
                self.touch_map();
            }
        }
        if redo {
            let n = match self.doc.doc.as_mut() {
                Some(d) => self.edit.redo(&mut d.map),
                None => 0,
            };
            self.op = None;
            if n > 0 {
                self.touch_map();
            }
        }
        if save {
            self.do_save();
        }
        if save_as {
            // 与菜单项同一条路径：打开「另存为」模态（输完路径回车才真写盘）
            let canvas = self.last_canvas;
            self.dispatch_menu(menu::model::Action::SaveAs, canvas);
            ctx.request_repaint();
        }
    }

    // ───────────────────── 菜单系统 / 深色模式（2026-11-05） ─────────────────────

    /// **执行一条菜单动作**（鼠标点击、键盘回车、`--menu-click` 三条路径的唯一出口）。
    ///
    /// 真正干活的判定在 [`crate::menu::action::apply`]；这里只负责
    /// ① 拼上下文 ② 把结果落到宿主状态（回执 / 重渲 / 诊断行）。
    fn dispatch_menu(&mut self, action: menu::model::Action, canvas: Rect) {
        let out = {
            let mut cx = menu::action::Ctx {
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
                settings_path: &self.settings_path,
                recent: &mut self.recent,
                tool: self.selected_tool,
            };
            menu::action::apply(action, &mut cx)
        };
        self.finish_menu(action, out);
    }

    /// 把一次菜单动作的结果落到宿主状态。
    ///
    /// ⚠ **灰显项也要有诊断行**：`--menu-click file.exit` 这种"点了但没做"必须能被
    /// 脚本抓到（验收门 §四.4 的机器判据就是这一行 + 状态栏那句）。
    fn finish_menu(&mut self, action: menu::model::Action, out: menu::action::Outcome) {
        let result = match &out.receipt {
            Some((text, warn)) => format!("{text}{}", if *warn { "（警示）" } else { "" }),
            None => "无回执".to_owned(),
        };
        eprintln!(
            "{}",
            tx::fill(
                tx::DIAG_MENU_CLICK,
                &[
                    ("id", action.id()),
                    ("label", action.label()),
                    ("result", &result),
                ]
            )
        );
        if out.touched_map {
            self.touch_map();
        }
        if let Some((text, warn)) = out.receipt {
            self.op = Some((text, warn));
        }
    }

    /// 打开一张地图（模态里点「打开」/ 回车）。
    fn open_map_path(&mut self, path: &str, canvas: Rect) {
        let out = {
            let mut cx = menu::action::Ctx {
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
                settings_path: &self.settings_path,
                recent: &mut self.recent,
                tool: self.selected_tool,
            };
            menu::action::open_path(&mut cx, path)
        };
        self.finish_menu(menu::model::Action::OpenMap, out);
        // 打开成功（文档真的换了）⇒ 地图列表跟着换目录；失败不动（当前图还在）
        if self.doc.doc.is_some() {
            self.follow_map_dir();
        }
    }

    /// **地图列表跟着当前地图换目录**（只在根没被 `--map-list-root` 钉住时）。
    ///
    /// 为什么要有它：列表根默认取"当前地图所在目录"。用户从模态打开了**别的目录**的图
    /// 之后，若列表还停在旧目录，点一行就会跳回旧目录 —— 那不是"列表"该有的行为。
    /// 换根是**重新扫**（`MapListPanel::start`），不是增量补几条：旧条目的相对路径
    /// 全都失效了，留着只会显示错名字。
    fn follow_map_dir(&mut self) {
        if self.maps_root_pinned {
            return;
        }
        let Some(d) = self.doc.doc.as_ref() else {
            return;
        };
        let Some(dir) = d.path.parent() else {
            return;
        };
        if dir.as_os_str().is_empty() {
            return;
        }
        let same = self
            .maps
            .root()
            .map(|r| maplist::same_path(r, dir))
            .unwrap_or(false);
        if !same {
            let root = dir.to_path_buf();
            self.maps.start(&root);
        }
    }

    /// **打开「最近打开」子菜单里的第 `idx` 行**（鼠标点击 / 回车都走这里）。
    ///
    /// 与其它菜单动作同一套骨架：拼上下文 → 调 [`menu::action::open_recent_index`]
    /// → 把回执落到状态栏 + 打一行诊断（`[菜单] 点击 file.open_recent（…）：…`）。
    /// 真正干活的判定全在 `menu::action` 里：文件还在 ⇒ 走与「打开地图…」**完全相同**的
    /// `open_path`；文件已不存在 ⇒ 状态栏给中文原因 + 从列表里移除。
    fn open_recent_at(&mut self, idx: usize, canvas: Rect) {
        let out = {
            let mut cx = menu::action::Ctx {
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
                settings_path: &self.settings_path,
                recent: &mut self.recent,
                tool: self.selected_tool,
            };
            menu::action::open_recent_index(&mut cx, idx)
        };
        self.finish_menu(menu::model::Action::OpenRecent, out);
    }

    /// **另存为到指定路径**（模态里点「另存为」/ 回车）。
    ///
    /// 与 [`Self::open_map_path`] 同一套骨架：拼上下文 → 调
    /// [`menu::action::save_as_path`] → 把回执落到状态栏 + 打一行诊断。
    /// 真正的写盘与"改认新路径"在 [`crate::save::save_as_session`] 里，
    /// 这里一行写盘逻辑都没有。
    fn save_as_path(&mut self, path: &str, canvas: Rect) {
        let out = {
            let mut cx = menu::action::Ctx {
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
                settings_path: &self.settings_path,
                recent: &mut self.recent,
                tool: self.selected_tool,
            };
            menu::action::save_as_path(&mut cx, path)
        };
        self.finish_menu(menu::model::Action::SaveAs, out);
    }

    /// **导出选区到指定路径**（导出选区模态里点「导出」/ 回车）。
    ///
    /// 与 [`Self::save_as_path`] 同一套骨架：拼上下文 → 调
    /// [`menu::action::export_selection_path`] → 把回执落到状态栏 + 打一行诊断。
    /// 渲染/分块/PNG 编码/原子写全在 [`crate::export`]，这里一行都没有。
    fn export_selection_path(&mut self, path: &str, canvas: Rect) {
        let out = {
            let mut cx = menu::action::Ctx {
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
                settings_path: &self.settings_path,
                recent: &mut self.recent,
                tool: self.selected_tool,
            };
            menu::action::export_selection_path(&mut cx, path)
        };
        self.finish_menu(menu::model::Action::ExportSelection, out);
    }

    /// **导出小地图到指定路径**（导出小地图模态里点「导出」/ 回车）。
    fn export_minimap_path(&mut self, path: &str, canvas: Rect) {
        let out = {
            let mut cx = menu::action::Ctx {
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
                settings_path: &self.settings_path,
                recent: &mut self.recent,
                tool: self.selected_tool,
            };
            menu::action::export_minimap_path(&mut cx, path)
        };
        self.finish_menu(menu::model::Action::ExportMinimap, out);
    }

    /// 处理浮层的输入（打开地图模态 / 另存为模态 / 快捷键浮层）；返回是否**吃掉了这一帧的交互**。
    fn handle_overlay(&mut self, ui: &mut egui::Ui, l: &Layout) -> bool {
        if !self.overlay.is_some() {
            return false;
        }
        let mut act = menu::dialog::DialogAction::None;
        if matches!(self.overlay, Overlay::OpenMap { .. }) {
            act = menu::dialog::interact_open_map(ui, l, &mut self.overlay);
        } else if matches!(self.overlay, Overlay::SaveAs { .. }) {
            // 另存为模态：与打开模态**同一套**输入/校验/回车提交（见 `menu::dialog`）
            act = menu::dialog::interact_save_as(ui, l, &mut self.overlay);
        } else if matches!(self.overlay, Overlay::ExportSelection { .. }) {
            // 导出选区模态：同一套输入，提交后走 `crate::export::export_selection`
            act = menu::dialog::interact_export_selection(ui, l, &mut self.overlay);
        } else if matches!(self.overlay, Overlay::ExportMinimap { .. }) {
            // 导出小地图模态：同一套输入，提交后走 `crate::export::export_minimap`
            act = menu::dialog::interact_export_minimap(ui, l, &mut self.overlay);
        } else if matches!(self.overlay, Overlay::Shortcuts) {
            // 只读浮层：任意点击 / Esc 关闭
            if ui.input(|i| i.pointer.any_click()) || ui.input(|i| i.key_pressed(egui::Key::Escape))
            {
                act = menu::dialog::DialogAction::Cancel;
            }
        }
        match act {
            menu::dialog::DialogAction::None => {}
            menu::dialog::DialogAction::Cancel => {
                self.overlay = Overlay::None;
                ui.ctx().request_repaint();
            }
            menu::dialog::DialogAction::Open(path) => {
                self.open_map_path(&path, l.viewport);
                ui.ctx().request_repaint();
            }
            menu::dialog::DialogAction::SaveAs(path) => {
                self.save_as_path(&path, l.viewport);
                ui.ctx().request_repaint();
            }
            menu::dialog::DialogAction::ExportSelection(path) => {
                self.export_selection_path(&path, l.viewport);
                ui.ctx().request_repaint();
            }
            menu::dialog::DialogAction::ExportMinimap(path) => {
                self.export_minimap_path(&path, l.viewport);
                ui.ctx().request_repaint();
            }
        }
        true
    }

    /// `Ctrl+C` / `Ctrl+X` / `Ctrl+V` / `Delete` / `Ctrl+Shift+V` —— 选区与剪贴板的五个命令。
    ///
    /// 全部走 [`crate::selection_ops::apply`]（唯一实现），本方法只负责
    /// ① 把键折成 [`SelCmd`]、② 取鼠标格、③ 把回执落到状态栏 + 需要时重渲。
    ///
    /// 官方依据逐条见 [`crate::selection_ops`] 模块文档 §二（`Ctrl+Shift+V` 是本仓自定）。
    fn handle_sel_keys(&mut self, ctx: &egui::Context) {
        let (copy, cut, paste, del, fill_sel) = ctx.input(|i| {
            let cmd = i.modifiers.command;
            (
                cmd && !i.modifiers.shift && i.key_pressed(egui::Key::C),
                cmd && !i.modifiers.shift && i.key_pressed(egui::Key::X),
                cmd && !i.modifiers.shift && i.key_pressed(egui::Key::V),
                !cmd && !i.modifiers.alt && i.key_pressed(egui::Key::Delete),
                // ⚠ **无官方依据**：官方没有给"填充选区"绑键（见 `selection_ops` §二）
                cmd && i.modifiers.shift && i.key_pressed(egui::Key::V),
            )
        });
        let cmd = if fill_sel {
            Some(SelCmd::Fill)
        } else if copy {
            Some(SelCmd::Copy)
        } else if cut {
            Some(SelCmd::Cut)
        } else if paste {
            Some(SelCmd::Paste)
        } else if del {
            Some(SelCmd::Delete)
        } else {
            None
        };
        let Some(cmd) = cmd else {
            return;
        };
        // 粘贴传的是**鼠标格**（点击点）；真正的落点由内核按上游 `RegionCopy.cs:70` 的 `Anchor`
        // 重算（地表可见时按源的奇偶往左上对齐，负锚点整块拒绝）—— GUI 不做第二次对齐，
        // 以免出现"界面预览/回执一套、落笔另一套"。
        // 这里现算而不复用 `self.hover`：`handle_keys` 跑在 `update_hover` 之前，
        // 复用会拿到**上一帧**的鼠标格 —— 移动鼠标后立刻按 `Ctrl+V` 就会贴到旧位置。
        let cursor = self.cursor_cell(ctx);
        let Some(d) = self.doc.doc.as_mut() else {
            return;
        };
        let Some(rec) = selection_ops::apply(&mut self.edit, &mut d.map, cmd, cursor) else {
            return; // 只有"粘贴时鼠标不在图上"会走到这里（什么都没做）
        };
        eprintln!(
            "{}",
            tx::fill(
                tx::DIAG_SEL_CMD,
                &[
                    ("cmd", cmd.key()),
                    ("warn", if rec.warn { "1" } else { "0" }),
                    ("n", &rec.changed.to_string()),
                    ("undo", &self.edit.undo_len().to_string()),
                    ("text", &rec.text),
                ]
            )
        );
        self.op = Some((rec.text, rec.warn));
        if rec.changed > 0 {
            self.touch_map();
        }
        ctx.request_repaint();
    }

    /// `Enter` = 把"当前选中的印章"变成待放置态；`Esc` = 退出（见 [`Self::handle_keys`] 的 `esc`）。
    ///
    /// 为什么是 `Enter` 而不是"点一下就放"：选中是**浏览**动作（面板里一枚枚看过去），
    /// 放置是**破坏性**动作（写地图 + 占一步撤销）。两者必须分开，
    /// 否则用户翻页看缩略图就会在图上到处留印章。
    fn handle_arm_keys(&mut self, ctx: &egui::Context) {
        let enter = ctx.input(|i| {
            !i.modifiers.command && !i.modifiers.alt && i.key_pressed(egui::Key::Enter)
        });
        if !enter {
            return;
        }
        // 搜索框有焦点时 `Enter` 属于搜索框（不抢）
        if self.stamps.focused() {
            return;
        }
        self.arm_selected();
        ctx.request_repaint();
    }

    /// 把**当前选中的印章**置为待放置态（`Enter` 与 `--stamp-arm` 共用这一条）。
    ///
    /// 返回是否真的进入了待放置态（没选中任何一枚 ⇒ `false`，什么都不做）。
    fn arm_selected(&mut self) -> bool {
        let Some(a) = self.stamps.armed() else {
            return false;
        };
        eprintln!(
            "{}",
            tx::fill(
                tx::DIAG_STAMP_ARM,
                &[
                    ("id", &a.id),
                    ("name", &a.name),
                    (
                        "bounds",
                        &match a.bounds {
                            Some((x0, y0, x1, y1)) => format!("({x0},{y0},{x1},{y1})"),
                            None => tx::VAL_UNKNOWN.to_owned(),
                        }
                    ),
                ]
            )
        );
        // 待放置态的提示进 `status_op`（不是 `status_stamp`）：后者是"选中的是谁"，
        // 前者是"现在点下去会发生什么"，两件事分开说。
        self.op = Some((stamp_place::armed_text(&a.name), false));
        self.armed = Some(a);
        // 进待放置态时切到「印章」工具图标，让工具列也反映当前意图
        self.selected_tool = selection_ops::TOOL_STAMP_IDX;
        self.edit.select(selection_ops::TOOL_STAMP_IDX);
        true
    }

    /// 鼠标当前所在的世界格（**现算**，不吃 `self.hover` 的上一帧值）。
    fn cursor_cell(&self, ctx: &egui::Context) -> Option<(u16, u16)> {
        if let Some(h) = self.forced_hover {
            return Some(h);
        }
        let pos = ctx.pointer_hover_pos()?;
        let (x, y) = self.view.screen_to_cell(self.last_canvas, pos)?;
        self.cell_in_map(x, y)
    }

    /// **落位**：把待放置的印章贴到 `(x, y)`。
    ///
    /// 链路（全在 [`stamp_place::place`] 里）：读 `.X` → `register_stamp` → `place_stamp_by_id`。
    /// 失败时地图**一个字节都不动**（三道闸门都在写之前），原因进状态栏。
    fn commit_place(&mut self, x: u16, y: u16) {
        let Some(armed) = self.armed.clone() else {
            return;
        };
        // 路径来自目录的 `resolve`（真实读盘结果）；未登记 / 解析失败 ⇒ 不猜路径
        let path = self.stamps.selected_path().map(|p| p.to_path_buf());
        let Some(path) = path else {
            let why = tx::fill(tx::STAMP_NO_PATH, &[("id", &armed.id)]);
            eprintln!(
                "{}",
                tx::fill(
                    tx::DIAG_STAMP_PLACE_FAIL,
                    &[("id", &armed.id), ("x", &x.to_string()), ("y", &y.to_string()), ("why", &why)]
                )
            );
            self.op = Some((stamp_place::fail_text(&why), true));
            return;
        };
        let Some(d) = self.doc.doc.as_mut() else {
            return;
        };
        let out = stamp_place::place(&mut self.edit, &mut d.map, &armed, &path, x, y);
        // 内核在成功时也可能留一句"库号字节被忽略"的说明。**取走它，但不拼进回执**：
        //
        // ① 那句话很长（实测 88 字：`Old12（12B/格，TMapInfo 旧版） 里 物体 的库号字节没有
        //    对应字节（writer 不写它）—— 该字节已忽略，只写图号`）。拼进回执后回执整段
        //    **放不下状态栏，被"放不下就不画"的规则整段丢掉** —— 实测截图里
        //    `放置印章「…」8 格` 直接消失，用户看不到任何"放成功了"的确认。
        // ② 它说的信息「本图没有库号字节」**已经在「落笔」段里**了
        //    （`落笔 bk_img（本图 12B 档无库号字节，只写图号）`）⇒ 不重复占用状态栏。
        // ③ 它仍然是真实数据、必须可查 ⇒ 走 stderr 的诊断行（本仓所有"细节事实"都在那儿）。
        let extra = selection_ops::take_note(&mut self.edit);
        if let Some(n) = &extra {
            eprintln!(
                "{}",
                tx::fill(tx::DIAG_STAMP_PLACE_NOTE, &[("id", &armed.id), ("note", n)])
            );
        }
        match out {
            Ok(p) => {
                eprintln!(
                    "{}",
                    tx::fill(
                        tx::DIAG_STAMP_PLACE,
                        &[
                            ("id", &armed.id),
                            ("x", &x.to_string()),
                            ("y", &y.to_string()),
                            ("n", &p.cells.to_string()),
                            ("k", &p.skipped.to_string()),
                            ("undo", &self.edit.undo_len().to_string()),
                        ]
                    )
                );
                self.op = Some((stamp_place::receipt(&armed.name, p), false));
                // 落位成功 ⇒ 退出待放置态（放第二枚要重新按 Enter，避免连点误放）
                self.armed = None;
                self.touch_map();
            }
            Err(why) => {
                eprintln!(
                    "{}",
                    tx::fill(
                        tx::DIAG_STAMP_PLACE_FAIL,
                        &[
                            ("id", &armed.id),
                            ("x", &x.to_string()),
                            ("y", &y.to_string()),
                            ("why", &why),
                        ]
                    )
                );
                self.op = Some((stamp_place::fail_text(&why), true));
                // 失败**不退**待放置态：用户多半是点到了图外，换个地方再点即可
            }
        }
    }


    /// `Ctrl+S`：**只在有修改时**才写盘，走 [`save::save_atomic`] 原子替换。
    ///
    /// 失败时把原因原样留在会话里（状态栏左段显示），且**原文件保持原样**。
    /// 真正干活的判定与写盘都在 [`save::save_session`] 里（无窗驱动器共用同一份）。
    fn do_save(&mut self) {
        let _ = save::save_session(&mut self.doc, &mut self.edit);
        // 存成功/失败都要重算文案（标题的 `*` 与状态栏左段都跟着变）
        self.text = screen_text::build(
            &self.doc,
            &self.view,
            self.hover,
            &self.lines,
            &self.edit,
        );
    }

    /// 画布上的工具落格（左键）：吸管即时取，画笔/橡皮按拖动累积成**一步**。
    ///
    /// 平移优先：调用方已判定"不是平移"才会进来（见 [`Self::interact`]）。
    fn handle_tool_pointer(&mut self, ui: &egui::Ui, l: &Layout) {
        let (down, pressed, released, pos) = ui.input(|i| {
            (
                i.pointer.primary_down(),
                i.pointer.button_pressed(egui::PointerButton::Primary),
                i.pointer.button_released(egui::PointerButton::Primary),
                i.pointer.interact_pos(),
            )
        });
        let cell = pos
            .and_then(|p| self.view.screen_to_cell(l.viewport, p))
            .and_then(|(x, y)| self.cell_in_map(x, y));

        // ⓪ **待放置态优先**：点画布 = 落位（不进任何笔画，也不碰当前工具）
        //
        //    为什么放在最前面：待放置态是一个"待确认的破坏性动作"，
        //    此时左键的语义必须唯一 —— 落位。若还让画笔落格，一次点击会同时写两样东西。
        if pressed && self.armed.is_some() {
            if let Some((x, y)) = cell {
                self.commit_place(x, y);
            }
            ui.ctx().request_repaint();
            return;
        }

        // ⓪b **选择工具**：拖动 = 画矩形选区（`SetSelection`），松手定格
        if self.selected_tool == selection_ops::TOOL_SELECT_IDX {
            self.handle_select_drag(ui, cell);
            return;
        }

        // ⓪c **地表落笔前端拦截**（九种格式里只有 Type5 会走到）：
        //     判据与文案在 [`ground_block_reason`]（与验收用例同一份），这里只负责
        //     **在 `begin()` / `touch_footprint()` 之前**把这一下拦掉并说出来。
        //     吸管（`Tool::Pickup`）只读、`Tool::Unwired` 什么都不做 ⇒ 两者不拦。
        if pressed && matches!(self.edit.tool(), Tool::Brush | Tool::Eraser | Tool::Bucket) {
            if let Some(why) = ground_block_reason(&self.doc, self.edit.layer()) {
                self.report_ground_blocked(&why);
                ui.ctx().request_repaint();
                return;
            }
        }

        // ① 按下：吸管当场取；画笔/橡皮开一笔
        if pressed {
            if let Some((x, y)) = cell {
                match self.edit.tool() {
                    Tool::Pickup => {
                        let picked = match self.doc.doc.as_ref() {
                            Some(d) => self.edit.pick_at(&d.map, x, y),
                            None => None,
                        };
                        let picked_cell = match self.doc.doc.as_ref() {
                            Some(d) => self.edit.pick_cell(&d.map, x, y),
                            None => None,
                        };
                        let fmt = self.doc.doc.as_ref().map(|d| d.map.format);
                        match picked {
                            Some(b) => {
                                eprintln!(
                                    "{}",
                                    tx::fill(
                                        tx::DIAG_EDIT_PICK,
                                        &[
                                            ("x", &x.to_string()),
                                            ("y", &y.to_string()),
                                            ("fam", b.family_name()),
                                            ("img", &b.sprite().to_string()),
                                            ("lib", &edit::brush_lib_text(fmt, &b)),
                                        ]
                                    )
                                );
                                // 整格三族也打一行（吸管读整格，但笔刷只取当前层字段）
                                if let Some(pc) = picked_cell {
                                    eprintln!(
                                        "{}",
                                        tx::fill(
                                            tx::DIAG_EDIT_PICK_CELL,
                                            &[
                                                ("x", &x.to_string()),
                                                ("y", &y.to_string()),
                                                ("bk", &pc.bk_img.to_string()),
                                                ("mid", &pc.mid_img.to_string()),
                                                ("fr", &pc.fr_img.to_string()),
                                            ]
                                        )
                                    );
                                }
                            }
                            None => eprintln!(
                                "{}",
                                tx::fill(
                                    tx::DIAG_EDIT_PICK_NONE,
                                    &[("x", &x.to_string()), ("y", &y.to_string())]
                                )
                            ),
                        }
                    }
                    Tool::Brush | Tool::Eraser => {
                        self.edit.begin();
                        self.paint_to(x, y);
                    }
                    Tool::Bucket => {
                        // 填充：点一下填一次（**整次填充 = 一步撤销**，由 `fill` 自己记账）
                        let out = match self.doc.doc.as_mut() {
                            Some(d) => self.edit.fill(&mut d.map, x, y),
                            None => edit::FillOutcome::NoBrush,
                        };
                        match out {
                            edit::FillOutcome::Filled(n) => {
                                eprintln!(
                                    "{}",
                                    tx::fill(
                                        tx::DIAG_EDIT_FILL,
                                        &[
                                            ("x", &x.to_string()),
                                            ("y", &y.to_string()),
                                            ("n", &n.to_string()),
                                            ("layer", self.edit.layer().name()),
                                        ]
                                    )
                                );
                                self.touch_map();
                            }
                            edit::FillOutcome::AlreadyFilled => {}
                            other => {
                                // 拒绝（越界/上限/层不支持/无笔刷/族不匹配）：原因已在 `note` 里
                                eprintln!(
                                    "{}",
                                    tx::fill(
                                        tx::DIAG_EDIT_FILL_REFUSED,
                                        &[("why", self.edit.note().unwrap_or(tx::VAL_UNKNOWN))]
                                    )
                                );
                                let _ = other;
                            }
                        }
                        ui.ctx().request_repaint();
                    }
                    Tool::Unwired => {}
                }
                ui.ctx().request_repaint();
            }
        }

        // ② 拖动中：笔画没结束就继续落格（指针移出画布时暂停，回来接着画）
        if down && self.edit.stroke_active() {
            if let Some((x, y)) = cell {
                self.paint_to(x, y);
            }
        }

        // ③ 松开：结算成一步（**一次拖动 = 一步撤销**）
        if released {
            let n = self.edit.end();
            if n > 0 {
                ui.ctx().request_repaint();
            }
        }
    }

    /// 从上一格插值到 `(x, y)` 并落格；真改了东西才 `touch_map`（触发重渲）。
    ///
    /// **整条轨迹都按当前笔刷尺寸落**（N×N，居中，边缘裁剪）——见
    /// [`EditSession::touch_footprint`]；插值仍然是 Bresenham（[`edit::line_cells`]）。
    ///
    /// ⚠ 进来先过一遍 [`ground_block_reason`]：拖动途中若用户把当前层切到地表
    /// （Type5 上），后续每一格也必须**一格都不写**，而不是"前几格写进去了"。
    fn paint_to(&mut self, x: u16, y: u16) {
        if ground_block_reason(&self.doc, self.edit.layer()).is_some() {
            return;
        }
        let from = self.edit.last_cell().unwrap_or((x, y));
        let path = edit::line_cells(from, (x, y));
        let mut changed = 0;
        if let Some(d) = self.doc.doc.as_mut() {
            for (cx, cy) in path {
                changed += self.edit.touch_footprint(&mut d.map, cx, cy);
            }
        }
        if changed > 0 {
            self.touch_map();
        }
    }

    /// **选择工具**的拖动：按下记锚点、拖动每帧重设选区、松手定格。
    ///
    /// 语义（闭区间、越界裁剪）全在内核 `Selection::clamp_to` + `EditSession::set_selection`；
    /// 本方法只做三件事：
    /// ① 按下时把鼠标格记成锚点（**锚点必须在图内**，否则内核会把整块判成图外）；
    /// ② 拖动时用「锚点 → 当前格」重设选区（指针移出画布时**保持上一次**，不静默清空）；
    /// ③ 松手把拖动状态清掉（选区留在会话里，直到下次拖动 / `Esc`）。
    ///
    /// ⚠ **不置脏、不进撤销栈**：选区是 UI 状态，内核已保证（`session.rs` 的选区段）。
    fn handle_select_drag(&mut self, ui: &egui::Ui, cell: Option<(u16, u16)>) {
        let (pressed, down, released) = ui.input(|i| {
            (
                i.pointer.button_pressed(egui::PointerButton::Primary),
                i.pointer.primary_down(),
                i.pointer.button_released(egui::PointerButton::Primary),
            )
        });
        if pressed {
            self.sel_drag = cell.map(|c| SelDrag { anchor: c });
        }
        if down {
            if let (Some(d), Some(cur), Some(doc)) =
                (self.sel_drag, cell, self.doc.doc.as_mut())
            {
                let sel = selection_ops::selection_of(d.anchor, cur);
                if self.edit.set_selection(&mut doc.map, sel) {
                    ui.ctx().request_repaint();
                }
            }
        }
        if released && self.sel_drag.is_some() {
            self.sel_drag = None;
            ui.ctx().request_repaint();
        }
    }

    /// 世界格是否在地图内（越界 = `None`，不静默夹到边界上）。
    fn cell_in_map(&self, x: i32, y: i32) -> Option<(u16, u16)> {
        let d = self.doc.doc.as_ref()?;
        if x < 0 || y < 0 || x >= i32::from(d.map.width) || y >= i32::from(d.map.height) {
            return None;
        }
        Some((x as u16, y as u16))
    }

    /// 系统关窗请求的拦截：有未保存修改时取消关窗、弹出模态（第二次才放行）。
    ///
    /// ⚠ **自动化取证运行例外**：带 `--shot` / `--autodrag` 时程序自己会发
    /// `ViewportCommand::Close` 收尾，若也拦下来，无人值守的截图/拖动实测就永远退不出去。
    /// 那两种模式下**不拦**（它们本来就只读磁盘、不保存）。
    fn guard_close_request(&mut self, ctx: &egui::Context) {
        if self.auto.shot.is_some()
            || self.auto.autodrag.is_some()
            || self.auto.palette_scroll.is_some()
        {
            return;
        }
        if !ctx.input(|i| i.viewport().close_requested()) {
            return;
        }
        if self.edit.dirty() && !self.confirm_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm_close = true;
            ctx.request_repaint();
        }
        // 不脏 / 已确认 ⇒ 什么都不做，让关窗照常发生
    }

    /// 标题变了才发 `ViewportCommand::Title`（`*` 要同步到 OS 窗口标题上）。
    fn sync_window_title(&mut self, ctx: &egui::Context) {
        if self.text.title == self.shown_title {
            return;
        }
        self.shown_title = self.text.title.clone();
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(self.shown_title.clone()));
    }

    /// 关闭拦截模态（简单居中提示框 + 半透明遮罩）。
    ///
    /// **不加可交互矩形**：右上角 `×` 的命中测试在 [`Self::interact`] 里按矩形算，
    /// 与绘制先后无关，所以"再点关闭"照样点得到 —— 这正是提示文案说的操作。
    fn draw_close_modal(&self, p: &egui::Painter, l: &Layout) {
        if !self.confirm_close {
            return;
        }
        pt::fill(p, l.content, t::ui().modal_scrim);
        let c = Rect::from_center_size(
            l.viewport.center(),
            Vec2::new(m::MODAL_W, m::MODAL_H),
        );
        pt::fill_round(p, c, m::MODAL_RADIUS, t::ui().panel_bg);
        pt::border(p, c, t::ui().accent_dark);
        pt::text(
            p,
            c.center(),
            Align2::CENTER_CENTER,
            tx::CLOSE_CONFIRM,
            m::MODAL_FONT,
            t::ui().text,
        );
    }

    /// 拖动实测（`--autodrag`）：按帧平移，时间到就打印报告并关窗。
    /// 走的是**与鼠标拖动完全相同的** [`CanvasView::pan_by`]，所以"每帧会不会重复
    /// `render_region`"这件事测出来的就是真实拖动时的行为。
    fn drive_autodrag(&mut self, ctx: &egui::Context) {
        let Some(secs) = self.auto.autodrag else {
            return;
        };
        let started = *self.auto.drag_started.get_or_insert_with(Instant::now);
        let elapsed = started.elapsed().as_secs_f32();
        if elapsed < secs {
            // 每帧右移 6px、下移 2px（斜向拖，两个轴都在变，才能覆盖"跨格重渲"路径）
            self.view.pan_by(Vec2::new(6.0, 2.0));
            ctx.request_repaint();
            return;
        }
        if self.auto.drag_reported {
            return;
        }
        self.auto.drag_reported = true;
        let avg_gap = if self.frames > 0 {
            self.frame_gap_ms / self.frames as f64
        } else {
            0.0
        };
        let (_, _, cw, ch) = self.view.visible_cells(self.last_canvas);
        eprintln!(
            "{}",
            tx::fill(
                tx::DIAG_AUTODRAG,
                &[
                    ("frames", &self.frames.to_string()),
                    ("calls", &self.view.render_calls.to_string()),
                    ("ms", &format!("{avg_gap:.2}")),
                    ("scale", &format!("{}", self.view.scale)),
                    ("cw", &cw.to_string()),
                    ("ch", &ch.to_string()),
                ]
            )
        );
        eprintln!(
            "        其中 update() 自耗时平均 {:.2} ms/帧；最近一次重渲 {:.1} ms；stats: {}",
            if self.frames > 0 {
                self.update_ms / self.frames as f64
            } else {
                0.0
            },
            self.view.last_render_ms,
            self.view.stats
        );
        // 有截图任务就先等截图，否则直接收尾
        if self.auto.shot.is_none() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// 素材面板滚动实测（`--palette-autoscroll`）：每帧往下滚一行，时间到就打印报告并关窗。
    ///
    /// 走的是**与滚轮完全相同的** [`PaletteState::scroll_by`] 与 [`PaletteState::draw`]，
    /// 所以"每滚一屏会不会把已缓存的帧重新解码一遍"这件事测出来的就是真实行为。
    fn drive_palette_autoscroll(&mut self, ctx: &egui::Context) {
        let Some(secs) = self.auto.palette_scroll else {
            return;
        };
        let started = *self.auto.palette_started.get_or_insert_with(Instant::now);
        let elapsed = started.elapsed().as_secs_f32();
        if elapsed < secs {
            self.palette.scroll_by(1);
            ctx.request_repaint();
            return;
        }
        if self.auto.palette_reported {
            return;
        }
        self.auto.palette_reported = true;
        let avg_gap = if self.frames > 0 {
            self.frame_gap_ms / self.frames as f64
        } else {
            0.0
        };
        eprintln!(
            "{}",
            tx::fill(
                tx::DIAG_PALETTE_SCROLL,
                &[
                    ("frames", &self.frames.to_string()),
                    ("ms", &format!("{avg_gap:.2}")),
                    ("decodes", &self.palette.decodes().to_string()),
                    ("hits", &self.palette.hits().to_string()),
                    ("row", &self.palette.scroll().to_string()),
                ]
            )
        );
        self.report_palette();
        if self.auto.shot.is_none() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// 把素材面板的统计打到 stderr（`--palette-report` / 滚动实测收尾时各调一次）。
    fn report_palette(&self) {        let pal = &self.palette;
        let stem = pal
            .current()
            .map(|e| e.stem.clone())
            .unwrap_or_else(|| tx::VAL_UNKNOWN.to_owned());
        let fam = pal
            .current()
            .map(|e| e.family_label())
            .unwrap_or(tx::VAL_UNKNOWN);
        let total = pal
            .current_count()
            .map(|n| n.to_string())
            .unwrap_or_else(|| tx::VAL_UNKNOWN.to_owned());
        eprintln!(
            "{}",
            tx::fill(
                tx::DIAG_PALETTE,
                &[
                    ("stem", &stem),
                    ("fam", fam),
                    ("total", &total),
                    ("slots", &pal.slots().to_string()),
                    ("decodes", &pal.decodes().to_string()),
                    ("hits", &pal.hits().to_string()),
                    ("empty", &pal.empty_count().to_string()),
                    ("row", &pal.scroll().to_string()),
                    ("rows", &pal.rows_drawn().to_string()),
                ]
            )
        );
        // 解码失败/空帧在面板上合成一个数（`空帧 N`），这里把失败原文单独说清楚
        if let Some(src) = self.doc.assets.source() {
            let fails = src.failures();
            if let Some(first) = fails.first() {
                eprintln!(
                    "{}",
                    tx::fill(
                        tx::DIAG_PALETTE_FAIL,
                        &[("n", &fails.len().to_string()), ("first", first)]
                    )
                );
            }
        }
    }

    /// 打一行"选中了哪枚印章"的诊断（**真实 id/名称/分类/格数/包围盒**，不是占位）。
    fn report_stamp_pick(&self) {
        let Some(s) = self.stamps.selected() else {
            return;
        };
        let unk = || tx::VAL_UNKNOWN.to_owned();
        let cells = s.cells.map(|n| n.to_string()).unwrap_or_else(unk);
        let b = s.bounds;
        let num = |i: usize| match b {
            Some(t) => [t.0, t.1, t.2, t.3][i].to_string(),
            None => tx::VAL_UNKNOWN.to_owned(),
        };
        eprintln!(
            "{}",
            tx::fill(
                tx::DIAG_STAMP_PICK,
                &[
                    ("id", &s.id),
                    ("name", &s.name),
                    ("kind", &s.kind_zh),
                    ("cells", &cells),
                    ("x0", &num(0)),
                    ("y0", &num(1)),
                    ("x1", &num(2)),
                    ("y1", &num(3)),
                ]
            )
        );
    }

    /// 印章面板统计（`--stamp-report`）：过滤条件 / 页 / 缓存峰值 / 渲染与命中数。
    fn report_stamps(&self) {
        eprintln!("{}", stamp_panel::report_text(&self.stamps));
    }

    /// 处理截图：到帧就请求一次；收到事件就落盘（并按需做同视点比对）。
    fn handle_screenshot(&mut self, ctx: &egui::Context) {
        // ① 收图
        let events = ctx.input(|i| i.events.clone());
        for e in events {
            if let egui::Event::Screenshot { image, .. } = e {
                let Some(path) = self.auto.shot.clone() else {
                    continue;
                };
                if self.auto.saved {
                    continue;
                }
                match selfcheck::save_screenshot(&image, &path) {
                    Ok(()) => {
                        self.auto.saved = true;
                        self.run_gate2(&path);
                        // `--palette-report`：截图这一帧的素材面板状态（真数据，不是估计值）
                        if self.auto.palette_report {
                            self.report_palette();
                        }
                        // `--stamp-report`：同一帧的印章面板状态（缓存峰值/命中/缺帧数）
                        if self.auto.stamp_report {
                            self.report_stamps();
                        }
                    }
                    Err(err) => {
                        eprintln!("{}", tx::fill(tx::DIAG_SHOT_FAIL, &[("err", &err)]));
                        self.auto.saved = true; // 失败也算"做过了"，不再反复请求
                    }
                }
            }
        }

        // ② 请求（只发一次）
        //
        // ⚠ **取证运行必须自己推进帧**：egui 只在有输入/有请求时重绘，
        //    而 `--shot` 运行**锁死了鼠标输入**（[`Self::input_locked`]）⇒ 没人碰窗口就没人重绘，
        //    `self.frames` 会停在几十帧以内、永远到不了 `--frames`，于是"截图不落盘、窗口不关"
        //    （2026-11-05 实测：进程空转 10 分钟、CPU 累计 0.95 s、PNG 未生成）。
        //    所以在**还没发出截图请求**之前，每帧主动请求下一帧。
        if self.auto.shot.is_some() && !self.auto.requested && self.frames < self.auto.frames {
            ctx.request_repaint();
        }
        if self.auto.shot.is_some() && !self.auto.requested && self.frames >= self.auto.frames {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(
                egui::UserData::default(),
            ));
            self.auto.requested = true;
            ctx.request_repaint();
        }

        // ③ 收尾：截图完成且没有拖动任务 ⇒ 关窗
        if self.auto.saved && self.auto.autodrag.is_none() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// 截图拍完后，若带了 `--gate2-client`，就地做同视点分块比对。
    fn run_gate2(&self, shot_path: &std::path::Path) {
        let (Some(client), Some(at)) = (self.auto.gate2_client.clone(), self.auto.gate2_at) else {
            return;
        };
        let shot = match miredit_render::png_io::read_png(shot_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[门2] 读回截图失败：{e}");
                return;
            }
        };
        match selfcheck::gate2(
            &shot,
            self.last_canvas,
            self.last_overlay,
            &self.view,
            &client,
            at,
            shot_path,
        ) {
            Ok(g) => {
                let pass = g.rate >= 0.95;
                eprintln!(
                    "[门2] 判定：命中率 {:.2}%（要求 ≥95%）⇒ {}",
                    g.rate * 100.0,
                    if pass { "PASS" } else { "FAIL" }
                );
            }
            Err(e) => eprintln!("[门2] 比对失败：{e}"),
        }
    }
}

impl eframe::App for MireditApp {
    /// 窗口清屏色：直接用面板底色，避免任何一帧出现黑边。
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        let c = t::ui().panel_bg;
        [
            c.r() as f32 / 255.0,
            c.g() as f32 / 255.0,
            c.b() as f32 / 255.0,
            1.0,
        ]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let t0 = Instant::now();
        // 帧间隔统计（拖动实测要报"平均帧时间"）
        let now = Instant::now();
        if let Some(prev) = self.last_frame {
            self.frame_gap_ms += (now - prev).as_secs_f64() * 1000.0;
        }
        self.last_frame = Some(now);
        self.frames += 1;

        // 强制 1 逻辑像素 = 1 物理像素，保证窗口是精确的 1500×950（截图口径）。
        if (ctx.pixels_per_point() - 1.0).abs() > f32::EPSILON {
            ctx.set_pixels_per_point(1.0);
        }

        // ⓪ 快捷键（Ctrl+Z/Y/S、Esc）与关窗拦截 —— 必须在交互/布局之前，
        //    否则本帧的撤销/保存要等到下一帧才反映到画布与标题上。
        self.handle_keys(ctx);
        self.guard_close_request(ctx);

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                let screen = ctx.content_rect();
                let l = self.layout(screen);
                // 面板自身不留边距，painter 的裁剪盒扩到整个窗口
                let p = ui.painter().with_clip_rect(screen);

                // ① 自动化驱动（平移 / 素材滚动）——必须在交互/布局之前
                self.drive_autodrag(ctx);
                self.drive_palette_autoscroll(ctx);
                // ② 首次对准（需要画布矩形）
                self.init_view(&l);
                self.last_canvas = l.viewport;
                self.last_overlay = l.zoom_pill;

                // ②b `--menu-click`：等首次布局算出画布矩形后再执行（缩放/适应要锚点）。
                //     走的是与鼠标点击**完全相同**的 `dispatch_menu`，不是另写一条捷径。
                //
                //     每条带一个**帧号**（缺省 1）：到点才执行，其余留到下一帧
                //     （`@40` 那种推迟执行就是靠这里 —— 见 `pending_menu_clicks` 的文档）。
                if !self.pending_menu_clicks.is_empty() {
                    let mut 到期: Vec<String> = Vec::new();
                    let mut 未到: Vec<(u64, String)> = Vec::with_capacity(self.pending_menu_clicks.len());
                    for (frame, id) in std::mem::take(&mut self.pending_menu_clicks) {
                        if self.frames >= frame {
                            到期.push(id);
                        } else {
                            未到.push((frame, id));
                        }
                    }
                    self.pending_menu_clicks = 未到;
                    for id in 到期 {
                        // 顶级项与静态子项都认（`view.light_night` 这类子项 id 靠
                        // `find_action_by_id` 才找得到，见该函数的文档）。
                        match menu::model::find_action_by_id(&id) {
                            Some(a) => self.dispatch_menu(a, l.viewport),
                            None => eprintln!("[菜单] 没有这个菜单项 id：{id}"),
                        }
                    }
                }
                // ②c `--open-map`：走「文件 → 打开地图…」模态点「打开」时的**同一个函数**
                if let Some(p) = self.pending_open_map.take() {
                    let text = p.to_string_lossy().to_string();
                    self.open_map_path(&text, l.viewport);
                }

                // ③ 交互（先算，后画，保证 hover 态与点击同帧生效）
                self.interact(ui, &l);

                // ④ 悬停格 + 单元格信息
                self.update_hover(ctx, &l);

                // ⑤ 重渲调度（只在变化时真渲）
                self.ensure_texture(ctx, &l);
                // ⑤b 素材面板帧数（锁定输入的取证运行里也要查，否则网格画不出来）
                self.palette.refresh_count(self.doc.assets.source());
                // ⑤d **地图列表面板**：推进一帧扫描（4 ms 预算 + 512 项上限）。
                //     还在扫 ⇒ 每帧请求重绘，否则面板会停在「扫描中…」等人动鼠标。
                if self.maps.poll() {
                    ctx.request_repaint();
                }
                // ⑤c 印章面板：收后台装载结果；就绪后把 `--stamp-*` 落到面板上（只做一次）。
                //    装载中要持续请求重绘，否则画面会停在「正在加载印章目录…」等用户动鼠标。
                let loaded = self.stamps.poll();
                if loaded || self.stamps.is_loading() {
                    ctx.request_repaint();
                }
                if self.stamps.is_ready() {
                    if let Some(p) = self.pending_stamp.take() {
                        apply_stamp_cli(&mut self.stamps, &mut self.palette, &p);
                        // `--stamp-arm` / `--stamp-place-at`：装载完成后**立即**走与
                        // `Enter` + 点画布完全相同的两条路径（见 `handle_arm_keys` / `commit_place`）
                        if p.arm {
                            self.arm_selected();
                        }
                        if let Some((x, y)) = p.place_at {
                            self.commit_place(x, y);
                        }
                    }
                }

                // ⑥ 装配文案
                self.text = screen_text::build(
                    &self.doc,
                    &self.view,
                    self.hover,
                    &self.lines,
                    &self.edit,
                );
                // ⑥a 状态栏追加「印章：<名称>」（选中印章后才有；没选就是 `None`，
                //     绝不写"印章：无"这种占位 —— 见 `stamp_panel::status_text`）
                self.text.status_stamp = self.stamps.status_text();
                // ⑥a2 状态栏「上一次操作回执」（印章放置 / 选区命令）：
                //     存在宿主里（`self.op`），因为 `screen_text::build` 不碰跨帧状态。
                if let Some((text, warn)) = &self.op {
                    self.text.status_op = Some(text.clone());
                    self.text.status_op_warn = *warn;
                }
                // ⑥a3 工具名要认得宿主层的两支（选择 / 印章）：内核 `Tool` 没有它们，
                //     直接取 `ed.tool().name()` 会显示成「未接线」——那是假信息。
                self.text.status_tool = tx::fill(
                    tx::STATUS_TOOL_TMPL,
                    &[("tool", edit::slot_label(self.selected_tool))],
                );

                // ⑥b 小地图的真实数据：地图格数（`.map` 头）+ 当前可见格窗口
                //     （= 真实 pan/scale/画布矩形算出来的）。**一个常量都不掺**（审计 E10）。
                let minimap = dock_right::MiniMap::new(
                    self.doc.doc.as_ref().map(|d| d.size()),
                    self.view.visible_cells(l.viewport),
                );

                // ⑦ 绘制
                pt::fill(&p, screen, t::ui().panel_bg);
                // 中列与右停靠区之间的 1px 分隔线（截图 x=1135 为 `#E7EBEF`）。
                // 必须显式画：地图纹理比视口大，只靠背景色会被顶掉。
                pt::fill(&p, l.dock_divider, t::ui().dock_divider);
                dock_left::draw(
                    &p,
                    &l,
                    self.selected_tool,
                    self.edit.layer(),
                    &self.view.mask,
                    &self.view.overlay,
                    &self.text,
                    &self.panels,
                );
                // ⑦a2 **地图列表面板**（中列左侧一列）：先让它把当前图那行滚进可见区，
                //      再画。顺序有因果：`tick` 改 `top`，`draw` 按 `top` 出图 ——
                //      反过来会先画一帧旧滚动位置再跳，截图就会拍到"当前项不在视野里"。
                let cur_path = self.doc.doc.as_ref().map(|d| d.path.clone());
                maplist::tick(&l, &mut self.maps, cur_path.as_deref());
                maplist::draw(&p, &l, &self.maps, cur_path.as_deref());
                canvas::draw(
                    &p,
                    &l,
                    &CanvasCtx {
                        tex: self.view.texture(),
                        view: &self.view,
                        text: &self.text,
                        hover: self.hover,
                        has_map: self.doc.doc.is_some(),
                        // 单元格网格（视图 → 单元格网格）：**与菜单勾选同一个字段**
                        grid: self.grid,
                        // 地图尺寸：网格按它把格线裁到地图边界（官方 `Presentation.cs:37-39`）。
                        // 无图 ⇒ 0 ⇒ `grid::lines_in` 直接返回空集（不是"不夹"档）。
                        map_w: self.doc.doc.as_ref().map(|d| d.map.width).unwrap_or(0),
                        map_h: self.doc.doc.as_ref().map(|d| d.map.height).unwrap_or(0),
                        selection: self.edit.selection(),
                        // 待放置轮廓：**只有按住 Alt 才画**（任务书 §三.5 的推荐做法）——
                        // 常驻显示会盖住地图，按住 Alt 才看是"临时预览"的语义。
                        stamp_preview: self.stamp_preview(ctx),
                        armed_badge: self.armed_badge(),
                    },
                );
                // ⑦b **常驻告警黄条**：压在画布纹理之上（必须在 `canvas::draw` 之后，
                //     否则会被比视口还大的纹理顶掉）。内容 = `Document::edit_warning()` 原文，
                //     不是 toast、不自动消失（见 [`crate::banner`]）。
                banner::draw(&p, &l, &self.text);
                dock_right::draw(
                    &p,
                    &l,
                    &self.text,
                    &mut self.palette,
                    &mut self.stamps,
                    self.doc.assets.source(),
                    &minimap,
                    &self.panels,
                );
                menubar::draw(&p, &l);
                // ⑦c **菜单弹层**：必须在菜单栏之后画（要压住面板与画布）
                //
                // `draw_with` 而不是 `draw`：子菜单的行来自**真数据**（设置文件里的
                // `recent`），与 `interact_with` / `handle_key_with` 问的是同一份
                // `SubState`（否则"画 3 行、键盘只走 1 行"这种错位就出来了）。
                let sub = self.sub_state();
                menu::draw_with(
                    &p,
                    &l,
                    &self.menu,
                    self.doc.doc.is_some(),
                    self.dark,
                    // 勾选型项的勾必须反映真状态（网格开着才画勾 / 门开着才画勾 /
                    // 当前是哪档光照就只勾哪一档）
                    self.grid,
                    self.doors_open,
                    self.lighting,
                    &self.panels,
                    &sub,
                );
                // ⑦c2 首帧几何（截图比对要用：画布 / 弹层 / 缩放浮层 / 告警黄条四块矩形 + 当前主题）
                if self.frames == 1 && self.menu.open_index().is_some() {
                    if let Some(pr) = menu::popup_rect(&p, &l, &self.menu) {
                        let br = banner::rect(&p, &l, &self.text).unwrap_or(Rect::NOTHING);
                        eprintln!(
                            "{}",
                            tx::fill(
                                tx::DIAG_MENU_GEOM,
                                &[
                                    ("cx", &format!("{:.0}", l.viewport.left())),
                                    ("cy", &format!("{:.0}", l.viewport.top())),
                                    ("cw", &format!("{:.0}", l.viewport.width())),
                                    ("ch", &format!("{:.0}", l.viewport.height())),
                                    ("px", &format!("{:.0}", pr.left())),
                                    ("py", &format!("{:.0}", pr.top())),
                                    ("pw", &format!("{:.0}", pr.width())),
                                    ("ph", &format!("{:.0}", pr.height())),
                                    ("gx", &format!("{:.0}", l.zoom_pill.left())),
                                    ("gy", &format!("{:.0}", l.zoom_pill.top())),
                                    ("gw", &format!("{:.0}", l.zoom_pill.width())),
                                    ("gh", &format!("{:.0}", l.zoom_pill.height())),
                                    ("dark", if self.dark { "1" } else { "0" }),
                                    ("bx", &format!("{:.0}", br.left())),
                                    ("by", &format!("{:.0}", br.top())),
                                    ("bw", &format!("{:.0}", br.width())),
                                    ("bh", &format!("{:.0}", br.height())),
                                ]
                            )
                        );
                    }
                }
                titlebar::draw(&p, &l, &self.text);
                statusbar::draw(&p, &l, &self.text);
                // 关闭拦截模态压在一切之上（只有真有未保存修改时才会画）
                self.draw_close_modal(&p, &l);
                // ⑦d **浮层**：打开地图模态 / 另存为模态 / 快捷键浮层，压在最上层
                match &self.overlay {
                    Overlay::OpenMap { path } => menu::dialog::draw_open_map(&p, &l, path),
                    Overlay::SaveAs { path } => menu::dialog::draw_save_as(&p, &l, path),
                    // 导出两个模态与另存为**同一套排版**，只是标题/提示/按钮文案不同
                    Overlay::ExportSelection { path } => {
                        menu::dialog::draw_export_selection(&p, &l, path)
                    }
                    Overlay::ExportMinimap { path } => {
                        menu::dialog::draw_export_minimap(&p, &l, path)
                    }
                    Overlay::Shortcuts => menu::dialog::draw_shortcuts(&p, &l),
                    Overlay::None => {}
                }

                // 窗口尺寸自检：与常量表不符说明窗口没开成 1500×950
                debug_assert!(
                    (screen.width() - m::WINDOW_W).abs() < 1.0 || screen.width() >= m::WINDOW_MIN_W,
                    "窗口宽度 {} 小于最小宽度 {}",
                    screen.width(),
                    m::WINDOW_MIN_W
                );
            });

        // ⑧ 标题同步（`*` 要上 OS 窗口标题）+ 截图流程
        self.sync_window_title(ctx);
        self.handle_screenshot(ctx);

        self.update_ms += t0.elapsed().as_secs_f64() * 1000.0;
    }
}

impl MireditApp {
    /// 自动化取证运行（`--shot` / `--autodrag` / `--palette-autoscroll`）期间**忽略鼠标输入**。
    ///
    /// 取证要的是一张**可复现**的图：外部鼠标滚轮会改缩放、误点会在图上落笔，
    /// 两者都会让"同一命令两次跑出不同截图"（实测：同一命令行两次跑，缩放一次 200%、
    /// 一次 133%，因为期间收到了一次滚轮事件）。所以这两种模式下把交互整段关掉 ——
    /// 它们本来就不该由人来点。
    fn input_locked(&self) -> bool {
        self.auto.shot.is_some() || self.auto.autodrag.is_some() || self.auto.palette_scroll.is_some()
    }

    /// 这一帧要不要画**印章待放置轮廓**：`(锚点, 包围盒)`。
    ///
    /// 四条同时成立才画：① 处于待放置态；② **按住 `Alt`**；③ 鼠标在某格上；
    /// ④ 目录给得出包围盒（`resolve` 成功）。
    ///
    /// 为什么"按住 Alt 才画"而不是常驻：轮廓是一块矩形描边，常驻会一直盖在地图上；
    /// 按住 `Alt` 才看是**临时预览**的语义（任务书 §三.5 的推荐做法）。
    /// 为什么缺 ④ 就不画：没有包围盒就没有真实几何，随便框一个"大概大小"就是假数据。
    fn stamp_preview(&self, ctx: &egui::Context) -> Option<stamp_place::Outline> {
        let a = self.armed.as_ref()?;
        if !ctx.input(|i| i.modifiers.alt) {
            return None;
        }
        Some((self.hover?, a.bounds?))
    }

    /// 这一帧要不要画**待放置徽标**（跟着鼠标走的小标签）。
    fn armed_badge(&self) -> Option<String> {
        let a = self.armed.as_ref()?;
        Some(tx::fill(tx::STAMP_ARMED_BADGE_TMPL, &[("name", &a.name)]))
    }

    /// 处理全部点击/拖拽。
    fn interact(&mut self, ui: &mut egui::Ui, l: &Layout) {
        if self.input_locked() {
            return;
        }
        // ⓪ **浮层是模态**：打开地图 / 快捷键浮层在的时候，其它一切交互让位
        if self.handle_overlay(ui, l) {
            return;
        }
        // ⓪b **菜单**：标签点击 → 展开/收起/切换；弹层点击 → 执行。
        //    展开态下其余交互整体让位（菜单是"模态式"的：这一次点击归它，
        //    否则点弹层外面既会收起弹层、又会顺手在画布上落一笔）。
        let was_open = self.menu.open_index().is_some();
        // 子菜单状态（「最近打开」的行数靠它）——与 `draw` 用**同一份**，
        // 否则画出来的行与能点的行会错位。
        let sub = self.sub_state();
        match menu::interact_with(ui, l, &mut self.menu, &sub) {
            menu::Click::Run(a) => {
                self.dispatch_menu(a, l.viewport);
                ui.ctx().request_repaint();
                return;
            }
            // 点了一条最近地图 ⇒ 打开**那一条**（与键盘回车同一条路）
            menu::Click::RunRecent(i) => {
                self.open_recent_at(i, l.viewport);
                ui.ctx().request_repaint();
                return;
            }
            menu::Click::Toggled => {
                ui.ctx().request_repaint();
                if was_open {
                    return;
                }
            }
            menu::Click::None => {}
        }
        if self.menu.open_index().is_some() {
            return;
        }
        // ① 标题栏拖拽
        titlebar::handle_drag(ui, l);

        // ② 三个窗口按钮：只有最右的 × 真正关闭窗口（最小化/最大化仍是装饰）
        //
        // ⚠ **关闭拦截**：有未保存修改时第一次点 × 只弹模态（不关窗），
        //    再点一次才真放弃 —— 与 `ui_text::CLOSE_CONFIRM` 的文案一字对应。
        if ui
            .interact(
                titlebar::close_rect(l),
                ui.id().with("caption_close"),
                Sense::click(),
            )
            .clicked()
        {
            if self.edit.dirty() && !self.confirm_close {
                self.confirm_close = true;
                ui.ctx().request_repaint();
            } else {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }

        // ③ 菜单栏 hover 高亮：本版起由 `menu::interact`（点击 + 弹层）接管，
        //    这里不再单独跑一遍 `menubar::handle_hover`（两个命中盒会打架）。

        // ③b 素材面板：页签 / 库下拉 / 搜索 / 网格点选 / 分页
        //
        // ⚠ 「分类」页签（印章浏览器）**只借用页签行**：其余交互由 `stamp_panel` 自己处理
        //   （两套下拉/搜索/网格语义不同，不能共用同一份命中测试）。
        // ⚠ 资源面板被隐藏时**整段交互跳过**（否则看不见的控件还能被点到）。
        let src = self.doc.assets.source();
        if !self.panels.get(PanelId::Palette) {
            // 面板隐藏：不响应任何点击
        } else if self.palette.tab() == stamp_panel::TAB_INDEX {
            self.palette.interact_tabs(ui, l);
            if let StampAction::Pick = self.stamps.interact(ui, l) {
                self.report_stamp_pick();
                ui.ctx().request_repaint();
            }
        } else {
            match self.palette.interact(ui, l, src) {
                PaletteAction::SetBrush(key) => {
                    if let Some(b) = self.edit.set_brush_from_palette(key.lib, key.index) {
                        // 点选即切到画笔（任务书 §四.4）
                        self.selected_tool = edit::TOOL_BRUSH_IDX;
                        self.edit.select(edit::TOOL_BRUSH_IDX);
                        let stem = self
                            .palette
                            .current()
                            .map(|e| e.stem.clone())
                            .unwrap_or_else(|| tx::VAL_UNKNOWN.to_owned());
                        eprintln!(
                            "{}",
                            tx::fill(
                                tx::DIAG_PALETTE_PICK,
                                &[
                                    ("stem", &stem),
                                    ("fam", b.family_name()),
                                    ("img", &b.sprite().to_string()),
                                    ("lib", &edit::brush_lib_text(None, &b)),
                                ]
                            )
                        );
                        ui.ctx().request_repaint();
                    } else {
                        // 面板侧已经拦过图号上限，走到这里说明约束又变了：如实报，不假装成功
                        eprintln!("{}", tx::PALETTE_FRAME_TOO_BIG);
                    }
                }
                PaletteAction::None => {}
            }
        }

        // ③c **地图列表面板**：点一行 = 打开那张图。
        //
        // 走的是 [`Self::open_map_path`]（= 「打开地图…」模态点「打开」时的同一个函数
        // → [`menu::action::open_path`] → `Document::load`），**不另写加载旁路**。
        // 「刷新」重扫同一个根（换根由 `follow_map_dir` 在打开别的目录的图时自动做）。
        if self.panels.get(PanelId::Maps) {
            match maplist::interact(ui, l, &mut self.maps) {
                maplist::Action::Open(i) => {
                    let target = self.maps.entries().get(i).map(|e| e.path.clone());
                    if let Some(p) = target {
                        let text = p.display().to_string();
                        self.open_map_path(&text, l.viewport);
                        ui.ctx().request_repaint();
                        return;
                    }
                }
                maplist::Action::Refresh => {
                    let root = self.maps.root().map(|p| p.to_path_buf());
                    if let Some(r) = root {
                        self.maps.start(&r);
                    }
                    ui.ctx().request_repaint();
                    return;
                }
                maplist::Action::None => {}
            }
        }

        // ④ 10 个工具按钮（点哪个就切哪个；未接线的工具照样能选，只是落格无效果）
        //    ⚠ 工具面板隐藏时整段跳过（看不见的按钮不该还能点）。
        if self.panels.get(PanelId::Tools) {
            for (i, r) in l.tool_buttons.iter().enumerate() {
                if ui
                    .interact(*r, ui.id().with(("tool_btn", i)), Sense::click())
                    .clicked()
                {
                    self.selected_tool = i;
                    self.edit.select(i);
                    // 换了工具就退出待放置态：待放置态的语义是"下一次左键 = 落位"，
                    // 用户既然去点别的工具，说明不想放了（否则点下去会莫名其妙落一枚印章）。
                    if self.armed.is_some() {
                        self.armed = None;
                        self.op = None;
                    }
                }
            }
        }

        // ⑤ 5 个图层行：**两个互不影响的动作**
        //    · 点眼睛 = 切该行可见性（前三行 → `LayerMask`；后两行 → `CollisionOverlay`）
        //    · 点行内其它位置 = 把该行设为**当前编辑层**（决定落笔写哪个字段/哪个位）
        //    五行全部接线（2026-11-04 审计 P0-9）：只有越界行才是"点了没反应"。
        //    ⚠ 图层面板隐藏时整段跳过。
        for (i, r) in l.layer_rows.iter().enumerate() {
            if !self.panels.get(PanelId::Layers) {
                break;
            }
            let resp = ui.interact(*r, ui.id().with(("layer_row", i)), Sense::click());
            if !resp.clicked() {
                continue;
            }
            let eye = dock_left::eye_rect(l, i).expand(4.0);
            let on_eye = resp
                .interact_pointer_pos()
                .map(|p| eye.contains(p))
                .unwrap_or(false);
            match dock_left::dispatch_click(i, on_eye) {
                dock_left::LayerClick::ToggleVisibility(_) => {
                    // 前三行动 `LayerMask`、后两行动 `CollisionOverlay`；
                    // 两者都进画布缓存键 ⇒ 下一帧 `ensure` 必然重渲一次（不是"点了没反应"）
                    if dock_left::toggle_visibility(&mut self.view.mask, &mut self.view.overlay, i) {
                        ui.ctx().request_repaint();
                    }
                }
                dock_left::LayerClick::SetEditLayer(layer) => {
                    self.edit.set_layer(layer);
                    // 状态栏的「当前层 / 落笔字段」要立刻跟着变
                    self.text = screen_text::build(
                        &self.doc,
                        &self.view,
                        self.hover,
                        &self.lines,
                        &self.edit,
                    );
                    ui.ctx().request_repaint();
                }
                dock_left::LayerClick::Ignore => {}
            }
        }

        // ⑥ 画布：中键拖动 / 空格+左键拖动 → 平移；否则左键 = 工具落格；滚轮 → 以鼠标为中心缩放
        let resp = ui.interact(l.viewport, ui.id().with("canvas"), Sense::click_and_drag());
        let space = ui.input(|i| i.key_down(egui::Key::Space));
        let panning = resp.dragged_by(egui::PointerButton::Middle)
            || (space && resp.dragged_by(egui::PointerButton::Primary));
        if panning {
            self.view.pan_by(resp.drag_delta());
            // 平移中途打断了一笔画：**把它当一步结算掉**（已写进地图的字节必须有对应的撤销步，
            // 否则这些字节既撤销不了、也不会让 `dirty` 变真 ⇒ 关窗时不提示保存，直接丢改动）
            if self.edit.stroke_active() {
                let n = self.edit.end();
                if n > 0 {
                    ui.ctx().request_repaint();
                }
            }
        } else {
            self.handle_tool_pointer(ui, l);
        }
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll.abs() > 0.5 {
                if let Some(pos) = ui.ctx().pointer_hover_pos() {
                    let notches = scroll / m::SCROLL_PX_PER_NOTCH;
                    let factor = m::ZOOM_PER_NOTCH.powf(notches);
                    self.view.zoom_at(l.viewport, pos, factor);
                }
            }
        }

        // ⑦ 缩放浮层三个可点区域（放在画布之后 = 盖在画布之上，先吃到点击）
        if ui
            .interact(
                canvas::zoom_minus_rect(l),
                ui.id().with("zoom_minus"),
                Sense::click(),
            )
            .clicked()
        {
            let c = l.viewport.center();
            self.view.zoom_at(l.viewport, c, 1.0 / m::ZOOM_PER_NOTCH);
        }
        if ui
            .interact(
                canvas::zoom_plus_rect(l),
                ui.id().with("zoom_plus"),
                Sense::click(),
            )
            .clicked()
        {
            let c = l.viewport.center();
            self.view.zoom_at(l.viewport, c, m::ZOOM_PER_NOTCH);
        }
        if ui
            .interact(
                canvas::zoom_fit_rect(l),
                ui.id().with("zoom_fit"),
                Sense::click(),
            )
            .clicked()
        {
            if let Some(d) = self.doc.doc.as_ref() {
                self.view.fit_map(l.viewport, d.map.width, d.map.height);
            }
        }
        // ⑦b **「原始大小」按钮**（上游 `901dbe2f` 的 `OriginalSizeButton`，
        //     `MainWindow.axaml:22`，`Click="ResetZoomClick"`）。
        //
        //     走**同一条 action**：[`menu::model::Action::ActualSize`] ⇒ 与菜单
        //     「视图 → 实际大小」、`--menu-click view.actual_size` 完全同一条代码路径
        //     （回执文案、锚点、夹取规则都不可能出现两套）。
        if ui
            .interact(
                canvas::zoom_original_rect(l),
                ui.id().with("zoom_original"),
                Sense::click(),
            )
            .clicked()
        {
            self.dispatch_menu(menu::model::Action::ActualSize, l.viewport);
            ui.ctx().request_repaint();
        }
    }
}

/// 把 `--palette-*` 开关落到面板上（**启动时**一次性；走的都是 GUI 里的同一套方法）。
///
/// 为什么要有它：验收要的是"同一段代码路径、可复现的一次运行"。
/// 切库 / 滚到某行 / 搜图号 / 切页签 / 点选设笔刷全部由这里驱动，**不另写一套捷径**；
/// GUI（[`MireditApp::new`]）与无窗驱动器（[`crate::editrun::run`]）共用同一份。
pub fn apply_palette_cli(
    pal: &mut PaletteState,
    ed: &mut EditSession,
    cli: &Cli,
    src: Option<&mir2_assets::WilSource>,
) {
    pal.refresh_count(src);
    if let Some(stem) = cli.palette_lib.as_deref() {
        if pal.select_lib_by_stem(stem) {
            pal.refresh_count(src);
        } else {
            eprintln!(
                "{}",
                tx::fill(
                    tx::DIAG_BAD_ARG,
                    &[("arg", &format!("--palette-lib {stem}（没有这个库）"))]
                )
            );
        }
    }
    if let Some(row) = cli.palette_scroll {
        pal.set_scroll(row);
    }
    if let Some(t) = cli.palette_tab {
        pal.set_tab(t);
    }
    if let Some(q) = cli.palette_search.as_deref() {
        pal.push_text(q);
        eprintln!("{}", pal.submit_search());
    }
    if let Some(img) = cli.palette_pick {
        // 与鼠标点选**同一个** `pick`（三族都允许；族与图层的把关在落笔那一刻）
        if let PaletteAction::SetBrush(key) = pal.pick_frame(img) {
            match ed.set_brush_from_palette(key.lib, key.index) {
                Some(b) => eprintln!(
                    "{}",
                    tx::fill(
                        tx::DIAG_PALETTE_PICK,
                        &[
                            (
                                "stem",
                                pal.current()
                                    .map(|e| e.stem.as_str())
                                    .unwrap_or(tx::VAL_UNKNOWN),
                            ),
                            ("fam", b.family_name()),
                            ("img", &b.sprite().to_string()),
                            ("lib", &edit::brush_lib_text(None, &b)),
                        ]
                    )
                ),
                None => eprintln!("{}", tx::PALETTE_FRAME_TOO_BIG),
            }
        } else {
            eprintln!(
                "{}",
                tx::fill(
                    tx::DIAG_BAD_ARG,
                    &[("arg", &format!("--palette-pick {img}（该帧不能设为笔刷）"))]
                )
            );
        }
    }
    if cli.palette_combo {
        pal.set_combo_open(true);
    }
}

/// 把 `--stamp-*` 落到印章面板上（**目录装载完成后**才调，见 [`PendingStamp`]）。
///
/// 走的都是 GUI 里的同一套方法（[`StampPanel::set_filter_key`] / `set_search` /
/// `set_page` / `pick_slot`），**不另写一条捷径** —— 这样"命令行能复现的，鼠标也能做出来"。
///
/// 只要有任何一个 `--stamp-*` 开关生效，就**顺带把页签切到「分类」**：
/// 否则用户看到的是一张"改了但看不见"的图（截图验收会因此拍空）。
fn apply_stamp_cli(stamps: &mut StampPanel, pal: &mut PaletteState, p: &PendingStamp) {
    let mut any = false;
    if let Some(kind) = p.kind.as_deref() {
        any = true;
        if !stamps.set_filter_key(kind) {
            eprintln!(
                "{}",
                tx::fill(
                    tx::DIAG_BAD_ARG,
                    &[("arg", &format!("--stamp-kind {kind}（没有这个分类）"))]
                )
            );
        }
    }
    if let Some(q) = p.search.as_deref() {
        any = true;
        stamps.set_search(q);
    }
    if let Some(page) = p.page {
        any = true;
        // 命令行给的是**1 基页码**（与界面上的「第 N 页」一致），内部是 0 基
        stamps.set_page(page.saturating_sub(1));
    }
    if let Some(slot) = p.pick {
        any = true;
        if !stamps.pick_slot(slot) {
            eprintln!(
                "{}",
                tx::fill(
                    tx::DIAG_BAD_ARG,
                    &[("arg", &format!("--stamp-pick {slot}（当前页没有这一格）"))]
                )
            );
        }
    }
    if any {
        pal.set_tab(stamp_panel::TAB_INDEX);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 资产根缺省值：本机真实根（**整个测试模块里只出现这一处**）。换机器用 `MIREDIT_ASSET_ROOT`。
    const 缺省资产根: &str = r"E:\GodM2";

    /// 资产根：`MIREDIT_ASSET_ROOT` → 缺省 [`缺省资产根`]。
    ///
    /// 根目录不存在（公开仓/PR 的机器上没有游戏素材）⇒ `None`，调用方打印 `[skip]` 并返回。
    /// 只做 `var_os` + `PathBuf`，不引入任何第三方库。
    fn 资产根() -> Option<PathBuf> {
        let root = std::env::var_os("MIREDIT_ASSET_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(缺省资产根));
        root.is_dir().then_some(root)
    }

    /// 用例专用的设置文件路径（**绝不许碰真实用户设置**）。
    ///
    /// # 为什么必须隔离（2026-11-05 实测踩到）
    ///
    /// 「最近打开」接线后，[`MireditApp::open_map_path`] 成功时会 `remember_recent` →
    /// **写设置文件**。不隔离的话，跑一次 `cargo test` 就会把开发机
    /// `%LOCALAPPDATA%\miredit\settings.json` 里的 `recent` 冲成测试用的地图路径
    /// —— 本轮实测发生过：一次 `LOCALAPPDATA` 指向临时目录的全量测试，
    /// 把 `10.map` 写进了那个"应该是空"的目录（正是它暴露了这个坑）。
    fn 用例设置路径(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("miredit_gui_app_settings_{tag}.json"))
    }

    fn 空应用() -> MireditApp {
        let settings = 用例设置路径("empty_app");
        // 每条用例都从"没有设置文件"开始（否则上一轮跑剩的 `recent` / `dark` 会漏进来，
        // 结果就不可复现了）
        let _ = std::fs::remove_file(&settings);
        MireditApp::new(&Cli {
            map: Some(PathBuf::from(r"E:\__no_such_map__.map")),
            settings: Some(settings),
            ..Default::default()
        })
    }

    /// 打不开地图时：`doc` 为空、`map_error` 有原因、缩放仍在合法区间（不能是 NaN/0）。
    #[test]
    fn 地图失败时不崩且缩放合法() {
        let a = 空应用();
        assert!(a.doc.doc.is_none());
        assert!(a.doc.map_error.is_some());
        assert!(a.view.scale >= m::CANVAS_MIN_SCALE && a.view.scale <= m::CANVAS_MAX_SCALE);
        assert_eq!(a.hover, None);
        assert_eq!(a.lines.head, "—");
    }

    /// `--layers` 要真的落到 `LayerMask` 上（否则验收门 4 的证据就是假的）。
    #[test]
    fn 初始图层开关生效() {
        let a = MireditApp::new(&Cli {
            map: Some(PathBuf::from(r"E:\__no_such_map__.map")),
            layers: Some([true, true, false]),
            ..Default::default()
        });
        assert!(a.view.mask.ground);
        assert!(a.view.mask.middle);
        assert!(!a.view.mask.front, "门4 要的就是关掉物件层");
    }

    /// `--scale` 越界要夹住（不许把 0.01 传进渲染）。
    #[test]
    fn 初始缩放被夹住() {
        let a = MireditApp::new(&Cli {
            map: Some(PathBuf::from(r"E:\__no_such_map__.map")),
            scale: Some(0.01),
            ..Default::default()
        });
        assert!((a.view.scale - m::CANVAS_MIN_SCALE).abs() < 1e-6);
    }

    /// `--hover` 伪造悬停优先于真实鼠标（截图取证靠它）。
    #[test]
    fn 伪造悬停优先() {
        let a = MireditApp::new(&Cli {
            map: Some(PathBuf::from(r"E:\__no_such_map__.map")),
            hover: Some((100, 100)),
            ..Default::default()
        });
        assert_eq!(a.forced_hover, Some((100, 100)));
    }

    /// **改笔刷尺寸不重渲**（任务书 §三.1 的最后一条）：尺寸只改"下一次落笔多大一块"，
    /// 已写进地图的字节一个都不动 ⇒ `map_gen` 不许变（画布缓存键里有它，变了就重渲一次）。
    #[test]
    fn 改笔刷尺寸不重渲() {
        let mut a = MireditApp::new(&Cli {
            map: Some(PathBuf::from(r"E:\__no_such_map__.map")),
            brush_size: Some(7),
            ..Default::default()
        });
        // `--brush-size 7` 真的落到会话上，且**没碰**地图代号
        assert_eq!(a.edit.brush_size(), 7);
        assert_eq!(a.doc.map_gen, 1, "--brush-size 不许改地图代号");
        assert_eq!(a.edit.undo_len(), 0, "--brush-size 不许产生撤销步");
        assert!(!a.edit.dirty());

        // 再改几次：地图代号与撤销栈都必须纹丝不动
        let gen = a.doc.map_gen;
        for n in [3usize, 5, 15, 31, 2, 99] {
            let got = a.edit.set_brush_size(n);
            assert_eq!(got, a.edit.brush_size());
            assert_eq!(a.doc.map_gen, gen, "改尺寸到 {n} 不该重渲");
        }
        assert_eq!(a.edit.undo_len(), 0);
        // 文案里的胶囊跟着变（这是"尺寸真的接上了"的可见证据）
        a.text = screen_text::build(&a.doc, &a.view, a.hover, &a.lines, &a.edit);
        assert_eq!(a.text.pill, "31×31", "99 会被夹到 31");
    }

    /// 关闭拦截模态：底板必须整块落在画布内，且那句中文放得进底板（不留半句被裁）。
    ///
    /// 这条挡的是"只在关窗时才会出现、平时截图看不到"的控件 —— 它要是排歪了，
    /// 只有真去关窗才发现，那时用户已经以为"程序没反应"。
    #[test]
    fn 关闭拦截模态放得下文案() {
        let l = Layout::compute(Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(m::WINDOW_W, m::WINDOW_H),
        ));
        let c = Rect::from_center_size(l.viewport.center(), Vec2::new(m::MODAL_W, m::MODAL_H));
        assert!(
            l.viewport.contains_rect(c),
            "模态底板 {c:?} 超出了画布 {:?}",
            l.viewport
        );
        let p = crate::fonts::measure_ctx();
        let painter = egui::Painter::new(
            p,
            egui::LayerId::debug(),
            Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(m::WINDOW_W, m::WINDOW_H)),
        );
        let w = pt::text_w(&painter, tx::CLOSE_CONFIRM, m::MODAL_FONT);
        assert!(
            w <= c.width() - 40.0,
            "提示文案宽 {w:.0} 放不进底板 {:.0}（需留左右各 20 的呼吸）",
            c.width()
        );
    }

    /// 自动化取证运行（`--shot` / `--autodrag`）必须锁鼠标输入：否则外部滚轮会把缩放改掉，
    /// 同一命令两次跑出两张不同的图（实测 200% 与 133% 都出现过）。
    #[test]
    fn 取证运行锁鼠标输入() {
        let mut cli = Cli {
            map: Some(PathBuf::from(r"E:\__no_such_map__.map")),
            ..Default::default()
        };
        assert!(!MireditApp::new(&cli).input_locked(), "普通运行不锁");
        cli.shot = Some(PathBuf::from(r"I:\tmp\x.png"));
        assert!(MireditApp::new(&cli).input_locked(), "带 --shot 必须锁");
        cli.shot = None;
        cli.autodrag = Some(1.0);
        assert!(MireditApp::new(&cli).input_locked(), "带 --autodrag 必须锁");
    }

    // ───────────────────── 菜单系统 / 深色模式（2026-11-05） ─────────────────────

    /// 造一个"有图"的应用（用内存里的 2×2 合成图，不依赖磁盘上的 0.map）。
    ///
    /// 为什么要合成图：`0.map` 有 5.88 MB，每条用例都读一遍会把测试拖到分钟级；
    /// 而这里要验的是"菜单动作有没有改到文档/视口/面板状态"，与地图多大无关。
    fn 有图应用() -> MireditApp {
        let mut app = 空应用();
        let mut b = vec![0u8; 52 + 12 * 16];
        b[0..2].copy_from_slice(&4u16.to_le_bytes());
        b[2..4].copy_from_slice(&4u16.to_le_bytes());
        app.doc.doc = Some(
            crate::mapdoc::MapDoc::from_bytes(std::path::Path::new(r"E:\x\4.map"), b)
                .expect("4×4 合成图应能解析"),
        );
        app.doc.map_error = None;
        if let Some(d) = app.doc.doc.as_ref() {
            let _ = app.edit.bind_document(&d.path, d.orig_bytes());
        }
        app
    }

    /// 画布矩形（与 `update` 里同一份布局）。
    fn 画布() -> Rect {
        Layout::compute(Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(m::WINDOW_W, m::WINDOW_H),
        ))
        .viewport
    }

    /// **无图时点「需要先开图」的项**：回执是「需要先开图：…」，且文档一个字节都不动。
    #[test]
    fn 无图时菜单项被拦且不改文档() {
        let mut app = 空应用();
        let canvas = 画布();
        for a in [
            menu::model::Action::SaveMap,
            menu::model::Action::Undo,
            menu::model::Action::ZoomIn,
            menu::model::Action::FitMap,
        ] {
            app.dispatch_menu(a, canvas);
            let (text, warn) = app.op.clone().expect("必须有回执");
            assert!(warn, "{} 的拒绝回执要用警示色", a.id());
            assert!(
                text.contains("需要先开图"),
                "{} 的回执没说清原因：{text}",
                a.id()
            );
            assert!(app.doc.doc.is_none(), "{} 不该造出一张图", a.id());
        }
    }

    /// **灰显项点了要说原因**，且地图/视口/面板一个都不许变。
    #[test]
    fn 灰显项只给原因不改任何状态() {
        let mut app = 有图应用();
        let canvas = 画布();
        let scale = app.view.scale;
        let gen = app.doc.map_gen;
        let panels = app.panels;
        for a in [
            menu::model::Action::Exit,
            menu::model::Action::NewMap,
            menu::model::Action::GenTown,
            menu::model::Action::LockLayout,
        ] {
            app.dispatch_menu(a, canvas);
            let (text, warn) = app.op.clone().expect("必须有回执");
            assert!(warn, "{} 的拒绝回执要用警示色", a.id());
            assert!(text.starts_with("未实现："), "{} 的回执开头不对：{text}", a.id());
            assert!(text.len() > 8, "{} 的原因太短：{text}", a.id());
        }
        assert_eq!(app.view.scale, scale, "灰显项不该改缩放");
        assert_eq!(app.doc.map_gen, gen, "灰显项不该改地图（map_gen 是改图的唯一记号）");
        assert_eq!(app.panels, panels, "灰显项不该改面板显隐");
        assert!(app.overlay == Overlay::None, "灰显项不该弹浮层");
    }

    /// **深色模式**：翻转开关 + 写设置文件 + `theme::set_dark` 同步（三件事必须一起发生）。
    #[test]
    fn 深色模式翻转并落盘() {
        let dir = std::env::temp_dir().join("miredit_dark_test");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("settings.json");
        let mut app = MireditApp::new(&Cli {
            map: Some(PathBuf::from(r"E:\__no_such_map__.map")),
            settings: Some(path.clone()),
            ..Default::default()
        });
        assert!(!app.dark, "默认浅色");
        assert!(!t::is_dark());
        let canvas = 画布();
        app.dispatch_menu(menu::model::Action::DarkMode, canvas);
        assert!(app.dark, "菜单没翻转开关");
        assert!(t::is_dark(), "theme::set_dark 没跟上（绘制会仍是浅色）");
        let saved = std::fs::read_to_string(&path).expect("设置文件必须真写出来");
        assert!(saved.contains("\"dark\": true"), "文件内容不对：{saved}");
        // 再点一次 → 关，文件跟着变
        app.dispatch_menu(menu::model::Action::DarkMode, canvas);
        assert!(!app.dark);
        assert!(!t::is_dark());
        let saved = std::fs::read_to_string(&path).expect("再写一次");
        assert!(saved.contains("\"dark\": false"), "文件内容不对：{saved}");
        let _ = std::fs::remove_dir_all(&dir);
        // 收尾：把线程局部的主题恢复成浅色，别污染同进程的其它用例
        t::set_dark(false);
    }

    /// **单元格网格**（视图 → 单元格网格）：`--cell-grid` / `--no-cell-grid` 置初值、
    /// 菜单项翻转、回执说人话、菜单勾跟着真状态，且**不动地图一个字节**（网格是叠加层，不是编辑）。
    ///
    /// 这条盯的是"三处必须同一份真源"：`app.grid`（画布读它）、
    /// 菜单勾（`model::check` 读它）、回执文案。任何一处没跟上，
    /// 用户看到的就是"点了没反应"或"勾着却没有线"。
    #[test]
    fn 单元格网格可开可关且不动地图() {
        // ① 缺省**开**：官方 `MainWindow.axaml:5` 的 `GridToggle` 是
        //    `ToggleType="CheckBox" IsChecked="True"`。
        //    ⚠ 2026-11-05 订正：旧版这里断言"默认必须是关"（当时的理由写在
        //    `grid` 模块文档 §五），审查（内部报告《审查-菜单 39 项》，已移出公开仓
        //    缺陷 4）判定那是**形态偏离官方** ⇒ 改成默认开，**不是放宽断言**。
        let mut app = 有图应用();
        assert!(app.grid, "官方 IsChecked=True ⇒ 默认必须开");
        let canvas = 画布();
        let gen = app.doc.map_gen;

        // ② 点一次 → 关；回执说清是关
        app.dispatch_menu(menu::model::Action::CellGrid, canvas);
        assert!(!app.grid, "菜单项没翻转开关");
        let (text, warn) = app.op.clone().expect("必须有回执");
        assert!(!warn, "关网格不是拒绝");
        assert!(text.contains('关'), "回执没说清开还是关：{text}");
        assert_eq!(
            app.doc.map_gen, gen,
            "网格是叠加层，不许改地图（map_gen 不许变）"
        );

        // ③ 菜单勾必须跟着真状态（勾着却无线 / 有线却没勾，都是假信息）
        assert_eq!(
            menu::model::check(
                menu::model::Action::CellGrid,
                app.dark,
                app.grid,
                &app.panels
            ),
            Some(false),
            "网格关着时不许画勾"
        );

        // ④ 再点一次 → 开（回执要写出真实格尺寸）
        app.dispatch_menu(menu::model::Action::CellGrid, canvas);
        assert!(app.grid, "再点没打开");
        let (text, _) = app.op.clone().expect("回执");
        assert!(text.contains('开'), "{text}");
        assert!(text.contains("48×32"), "开网格的回执要写出真实格尺寸：{text}");
        assert_eq!(app.doc.map_gen, gen);
        assert_eq!(
            menu::model::check(
                menu::model::Action::CellGrid,
                app.dark,
                app.grid,
                &app.panels
            ),
            Some(true),
            "网格开着时菜单必须画勾"
        );

        // ⑤ 两个开关都置初值（与菜单翻转的是**同一个字段**）
        //    `--cell-grid` = 显式开（默认已开 ⇒ 幂等）；`--no-cell-grid` 才是关的手段
        let on = MireditApp::new(&Cli {
            map: Some(PathBuf::from(r"E:\__no_such_map__.map")),
            cell_grid: true,
            ..Default::default()
        });
        assert!(on.grid, "--cell-grid 没生效");
        assert_eq!(on.doc.map_gen, 1, "--cell-grid 不许改地图代号");
        let off = MireditApp::new(&Cli {
            map: Some(PathBuf::from(r"E:\__no_such_map__.map")),
            no_cell_grid: true,
            ..Default::default()
        });
        assert!(!off.grid, "--no-cell-grid 必须关掉网格");
        // 两个都给时 `--no-cell-grid` 优先（关是"更明确"的那个诉求）
        let both = MireditApp::new(&Cli {
            map: Some(PathBuf::from(r"E:\__no_such_map__.map")),
            cell_grid: true,
            no_cell_grid: true,
            ..Default::default()
        });
        assert!(!both.grid, "--no-cell-grid 优先级最高");
    }

    /// **开启门**（视图 → 开启门，2026-11-05 接线）：`--open-doors` 置初值、菜单项翻转、
    /// 回执说人话、菜单勾跟着真状态，且**不动地图一个字节**（门态是渲染开关，不是编辑）。
    ///
    /// 这条盯的是"三处必须同一份真源"：`app.doors_open`（画布渲染读它，
    /// 见 `CanvasView::ensure` 的缓存键 + `RenderOptions`）、菜单勾
    /// （`model::check_with` 的 `doors` 参数）、`--open-doors` 的初值。
    /// 任何一处没跟上，用户看到的就是"点了没反应"或"勾着却是闭门画面"。
    #[test]
    fn 开启门可开可关且不动地图() {
        let mut app = 有图应用();
        assert!(!app.doors_open, "缺省必须闭门（RenderOptions::default()）");
        let canvas = 画布();
        let gen = app.doc.map_gen;
        let cell = app.doc.doc.as_ref().and_then(|d| d.map.cell(0, 0).copied());

        // ① 点一次 → 开
        app.dispatch_menu(menu::model::Action::OpenDoors, canvas);
        assert!(app.doors_open, "菜单项没翻转开关");
        let (text, warn) = app.op.clone().expect("必须有回执");
        assert!(!warn, "开门不是拒绝");
        assert_eq!(text, tx::MENU_OPEN_DOORS_ON);
        assert_eq!(app.doc.map_gen, gen, "门态是渲染开关，不许改地图代号");
        assert_eq!(
            app.doc.doc.as_ref().and_then(|d| d.map.cell(0, 0).copied()),
            cell,
            "门态是渲染开关，不许改格数据"
        );

        // ② 菜单勾必须跟着真状态（勾着却是闭门画面 = 假信息）
        assert_eq!(
            menu::model::check_with(
                menu::model::Action::OpenDoors,
                app.dark,
                app.grid,
                app.doors_open,
                app.lighting,
                &app.panels,
                &app.sub_state()
            ),
            Some(true),
            "门开着时菜单必须画勾"
        );

        // ③ 再点一次 → 关
        app.dispatch_menu(menu::model::Action::OpenDoors, canvas);
        assert!(!app.doors_open, "再点没关上");
        let (text, _) = app.op.clone().expect("回执");
        assert_eq!(text, tx::MENU_OPEN_DOORS_OFF);
        assert_eq!(app.doc.map_gen, gen);
        assert_eq!(
            menu::model::check_with(
                menu::model::Action::OpenDoors,
                app.dark,
                app.grid,
                app.doors_open,
                app.lighting,
                &app.panels,
                &app.sub_state()
            ),
            Some(false),
            "门关着时不许画勾"
        );

        // ④ `--open-doors` 置的是**同一个字段**的初值
        let on = MireditApp::new(&Cli {
            map: Some(PathBuf::from(r"E:\__no_such_map__.map")),
            open_doors: true,
            ..Default::default()
        });
        assert!(on.doors_open, "--open-doors 没生效");
        assert_eq!(on.doc.map_gen, 1, "--open-doors 不许改地图代号");
        // 缺省（不给开关）仍是闭门
        let off = MireditApp::new(&Cli {
            map: Some(PathBuf::from(r"E:\__no_such_map__.map")),
            ..Default::default()
        });
        assert!(!off.doors_open, "不给 --open-doors 时必须闭门");
    }

    /// **地图光照三档**（2026-11-05 接线）：菜单三子项切档、回执中文、勾选态读真状态、
    /// `--lighting` 置初值、三档互斥，且**不动地图一个字节**（光照是渲染开关，不是编辑）。
    ///
    /// 与 `开启门可开可关且不动地图` 同一套口径，钉的是"三处必须同一份真源"：
    /// `app.lighting`（画布渲染读它，见 `CanvasView::ensure` 的缓存键 + `RenderOptions`）、
    /// 菜单勾（`model::check_with` 的 `lighting` 参数）、`--lighting` 的初值。
    #[test]
    fn 地图光照三档可切且不动地图() {
        use menu::model::{Action, MapLight};
        let mut app = 有图应用();
        assert_eq!(app.lighting, MapLight::Day, "缺省必须白天（RenderOptions::default()）");
        let canvas = 画布();
        let gen = app.doc.map_gen;
        let cell = app.doc.doc.as_ref().and_then(|d| d.map.cell(0, 0).copied());

        let 勾 = |a: Action, app: &MireditApp| {
            menu::model::check_with(
                a,
                app.dark,
                app.grid,
                app.doors_open,
                app.lighting,
                &app.panels,
                &app.sub_state(),
            )
        };
        // ① 白天 ⇒ 只勾白天
        assert_eq!(勾(Action::LightDay, &app), Some(true));
        assert_eq!(勾(Action::LightDawnDusk, &app), Some(false));
        assert_eq!(勾(Action::LightNight, &app), Some(false));

        // ② 三个子项逐一切档：回执是中文、勾选态互斥、地图一格不动
        for (a, 档, 回执) in [
            (Action::LightDawnDusk, MapLight::DawnDusk, tx::MENU_LIGHT_DAWN_DUSK),
            (Action::LightNight, MapLight::Night, tx::MENU_LIGHT_NIGHT),
            (Action::LightDay, MapLight::Day, tx::MENU_LIGHT_DAY),
        ] {
            app.dispatch_menu(a, canvas);
            assert_eq!(app.lighting, 档, "{} 没切到档位", a.id());
            let (text, warn) = app.op.clone().expect("必须有回执");
            assert!(!warn, "切档不是拒绝");
            assert_eq!(text, 回执, "{} 回执文案不对", a.id());
            assert_eq!(app.doc.map_gen, gen, "光照是渲染开关，不许改地图代号");
            assert_eq!(
                app.doc.doc.as_ref().and_then(|d| d.map.cell(0, 0).copied()),
                cell,
                "光照是渲染开关，不许改格数据"
            );
            let 命中 = [Action::LightDawnDusk, Action::LightDay, Action::LightNight]
                .iter()
                .filter(|x| 勾(**x, &app) == Some(true))
                .count();
            assert_eq!(命中, 1, "{档:?} 档位下必须正好一个勾");
            assert_eq!(勾(a, &app), Some(true), "{档:?} 档位点中的那一项必须画勾");
        }

        // ③ 父项是容器：直接派发不切档，只给一句"去哪选"的回执
        app.dispatch_menu(Action::MapLighting, canvas);
        assert_eq!(app.lighting, MapLight::Day, "容器项不许切档");
        assert_eq!(app.op.clone().expect("回执").0, tx::STATUS_LIGHT_CONTAINER);

        // ④ `--lighting` 置的是**同一个字段**的初值
        for (档, want) in [
            ("dawn_dusk", MapLight::DawnDusk),
            ("day", MapLight::Day),
            ("night", MapLight::Night),
        ] {
            let cli = crate::cli::Cli::parse([
                "--map".to_owned(),
                r"E:\__no_such_map__.map".to_owned(),
                "--lighting".to_owned(),
                档.to_owned(),
            ]);
            assert!(cli.errors.is_empty(), "{档} 应被识别：{:?}", cli.errors);
            let a = MireditApp::new(&cli);
            assert_eq!(a.lighting, want, "--lighting {档} 没生效");
            assert_eq!(a.doc.map_gen, 1, "--lighting 不许改地图代号");
            assert!(!a.doors_open, "--lighting 不许把门态弄开");
        }
        assert_eq!(
            MireditApp::new(&crate::cli::Cli::default()).lighting,
            MapLight::Day,
            "不给 --lighting 时必须是白天"
        );
    }

    /// **`--menu-hover` 只在取证档（`--shot` / `--menu-open`）生效**。
    ///
    /// 为什么钉这条：它是给截图取证开的洞（子菜单只在某项成为高亮项时才展开，
    /// 而 `--shot` 锁死输入 ⇒ 没有 hover）。正常交互下它**不该**动状态，
    /// 否则"命令行能改高亮"就成了一条影子入口。
    #[test]
    fn 菜单高亮项只在取证档生效() {
        let p = || PathBuf::from(r"E:\__no_such_map__.map");
        // ① 单独给 ⇒ 忽略（菜单也没展开）
        let alone = MireditApp::new(&Cli {
            map: Some(p()),
            menu_hover: Some(2),
            ..Default::default()
        });
        assert!(
            alone.menu.active().is_none(),
            "没有 --shot / --menu-open 时不许设高亮项"
        );
        // ② 与 `--menu-open` 同给 ⇒ 生效
        let with_open = MireditApp::new(&Cli {
            map: Some(p()),
            menu_open: Some(2),
            menu_hover: Some(2),
            ..Default::default()
        });
        assert_eq!(
            with_open.menu.active(),
            Some(2),
            "--menu-open + --menu-hover 必须设上高亮项（否则子面板拍不到）"
        );
        // ③ 与 `--shot` 同给 ⇒ 也生效
        let with_shot = MireditApp::new(&Cli {
            map: Some(p()),
            menu_open: Some(0),
            menu_hover: Some(2),
            shot: Some(std::env::temp_dir().join("miredit-shot-probe.png")),
            ..Default::default()
        });
        assert_eq!(with_shot.menu.active(), Some(2));
    }

    /// **8 个面板开关**：逐个翻转、回执带面板名、状态真的变。
    ///
    /// ⚠ 地图列表（`PanelId::Maps`）**默认关**（唯一例外，理由见该枚举的文档），
    /// 所以它的第一下是"打开"、回执是「已显示面板」；其余 7 个默认开、第一下是"隐藏"。
    /// 这条用例按"默认值"分支，而不是硬写"默认全开"—— 否则加第 9 个面板时又要改。
    #[test]
    fn 面板开关逐项生效() {
        let mut app = 有图应用();
        let canvas = 画布();
        for id in PanelId::ALL {
            let before = app.panels.get(id);
            assert_eq!(
                before,
                id != PanelId::Maps,
                "{:?} 的默认显隐不对（只有地图列表默认关）",
                id
            );
            app.dispatch_menu(menu::model::Action::Panel(id), canvas);
            assert_eq!(app.panels.get(id), !before, "{:?} 点了没翻转", id);
            let (text, warn) = app.op.clone().expect("必须有回执");
            assert!(!warn, "面板开关不是拒绝");
            assert!(
                text.contains(if before { "已隐藏面板" } else { "已显示面板" }),
                "回执文案不对：{text}"
            );
            assert!(text.contains(id.title()), "回执里要有面板名：{text}");
            app.dispatch_menu(menu::model::Action::Panel(id), canvas);
            assert_eq!(app.panels.get(id), before, "{:?} 再点没恢复", id);
        }
    }

    /// **缩放三项 + 适应地图**：缩放百分比真的变（不是"点了没反应"）。
    #[test]
    fn 缩放三项真的改视口() {
        let mut app = 有图应用();
        let canvas = 画布();
        app.view.scale = 1.0;
        app.dispatch_menu(menu::model::Action::ZoomIn, canvas);
        let up = app.view.scale;
        assert!(up > 1.0, "放大没生效：{up}");
        app.dispatch_menu(menu::model::Action::ZoomOut, canvas);
        let down = app.view.scale;
        assert!(down < up, "缩小没生效：{down}");
        app.dispatch_menu(menu::model::Action::ActualSize, canvas);
        assert!(
            (app.view.scale - 1.0).abs() < 1e-6,
            "实际大小没回到 1.0：{}",
            app.view.scale
        );
        app.dispatch_menu(menu::model::Action::FitMap, canvas);
        let fit = app.view.scale;
        // 4×4 的图在 987×815 的画布里只需 ~2.5×（放大而不是缩小），所以这里只断言
        // 「落在合法区间内」+「与 100% 不同」—— 具体倍数由 `CanvasView::fit_map` 决定。
        assert!(
            (m::CANVAS_MIN_SCALE..=m::CANVAS_MAX_SCALE).contains(&fit),
            "适应地图后的缩放越界：{fit}"
        );
        assert!(
            (fit - 1.0).abs() > 1e-6,
            "4×4 的图适应窗口后不该正好是 100%：{fit}"
        );
        // 回执里必须有真实百分比
        let (text, _) = app.op.clone().expect("回执");
        assert!(text.contains('%'), "回执里要有百分比：{text}");
    }

    /// **打开地图**：`open_path` 成功换图（文档换掉、map_gen+1、撤销栈清空），
    /// 失败**保留原图**且回执带原因。
    #[test]
    fn 打开地图成功换图失败保留原图() {
        let mut app = 有图应用();
        let canvas = 画布();
        let gen = app.doc.map_gen;
        // 失败：文件不存在 → 原图不动，map_error 有原因
        app.open_map_path(r"E:\__no_such_map__.map", canvas);
        assert!(app.doc.doc.is_some(), "打开失败不该丢掉手上的图");
        assert!(app.doc.map_error.is_some(), "失败原因要落到 map_error");
        let (text, warn) = app.op.clone().expect("回执");
        assert!(warn);
        assert!(text.contains("打开失败"), "{text}");
        assert_eq!(app.doc.map_gen, gen, "失败不该换图");
        // 成功：真读一张磁盘上的图（10.map 只有 177 KB，够小）
        // 路径相对资产根（`MIREDIT_ASSET_ROOT`，缺省本机真实根）；缺文件就只验失败分支。
        match 资产根() {
            Some(root) => {
                let p = root.join("GodotClient").join("maps").join("10.map");
                if !p.is_file() {
                    eprintln!("[skip] 缺文件 {}", p.display());
                } else {
                    app.open_map_path(&p.display().to_string(), canvas);
                    let d = app.doc.doc.as_ref().expect("应已换图");
                    assert_eq!(d.file_name, "10.map");
                    assert!(app.doc.map_gen > gen, "换图要 +1（画布缓存靠它失效）");
                    assert_eq!(app.edit.undo_len(), 0, "换图必须清撤销栈（旧增量不能作用到新图）");
                    let (text, warn) = app.op.clone().expect("回执");
                    assert!(!warn, "成功不该用警示色");
                    assert!(text.contains("已打开地图：10.map"), "{text}");
                }
            }
            None => eprintln!("[skip] 无资产根 MIREDIT_ASSET_ROOT"),
        }
    }


    // ───────────────────── 地图列表（2026-11-05，上游 901dbe2f） ─────────────────────

    /// **点地图列表里的一行 ⇒ 地图真的打开**（走 `open_map_path`，与「打开地图…」同一条路）。
    ///
    /// 四件事一起钉：
    /// ① 列表扫出 2 张、按**自然序**排（`n2.map` 在 `n10.map` 前）；
    /// ② 当前打开的那张 `index_of` 命中（面板据此画金框高亮）；
    /// ③ 打开第 2 行 ⇒ `doc.path` 真的换了 + 回执说「已打开地图：n10.map」；
    /// ④ 高亮跟着换成第 2 行（`index_of` 从 0 变 1）。
    #[test]
    fn 地图列表点击换图且高亮跟随() {
        let dir = std::env::temp_dir().join("miredit_gui_app_maplist_click");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建临时目录");
        let mut made = Vec::new();
        for (name, w) in [("n10.map", 5u16), ("n2.map", 4u16)] {
            let mut b = vec![0u8; 52 + 12 * (w as usize * w as usize)];
            b[0..2].copy_from_slice(&w.to_le_bytes());
            b[2..4].copy_from_slice(&w.to_le_bytes());
            let p = dir.join(name);
            std::fs::write(&p, &b).expect("写合成地图");
            made.push(p);
        }
        let settings = 用例设置路径("maplist_click");
        let _ = std::fs::remove_file(&settings);
        let mut app = MireditApp::new(&Cli {
            map: Some(made[1].clone()), // n2.map
            settings: Some(settings),
            map_list_root: Some(dir.clone()),
            show_maps: true,
            ..Default::default()
        });
        // 分帧扫描在 `update` 里推进；用例里手动推完
        while app.maps.poll() {}
        assert_eq!(app.maps.len(), 2, "两张图都要进列表");
        assert_eq!(app.maps.entries()[0].rel, "n2.map", "自然序：n2 在 n10 前");
        assert_eq!(app.maps.entries()[1].rel, "n10.map");
        // ② 当前项 = 第 0 行
        let cur = app.doc.doc.as_ref().map(|d| d.path.clone());
        assert_eq!(app.maps.index_of(cur.as_deref()), Some(0));

        // ③ 打开第 1 行（= 列表里点它）
        let canvas = 画布();
        let target = app.maps.entries()[1].path.clone();
        app.open_map_path(&target.display().to_string(), canvas);
        let (text, warn) = app.op.clone().expect("要有回执");
        assert!(!warn, "打开成功不该是警示色：{text}");
        assert!(text.contains("已打开地图：n10.map"), "回执文案不对：{text}");
        assert_eq!(
            app.doc.doc.as_ref().map(|d| d.path.clone()),
            Some(target),
            "文档必须换成点的那张"
        );
        // ④ 高亮跟着换
        let cur = app.doc.doc.as_ref().map(|d| d.path.clone());
        assert_eq!(
            app.maps.index_of(cur.as_deref()),
            Some(1),
            "当前项高亮必须跟着换到第 1 行"
        );
        // 面板开着时布局真的让出一列（不是"开关点了没反应"）
        let l = app.layout(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(m::WINDOW_W, m::WINDOW_H),
        ));
        assert!(l.p_maps.whole.width() > 1.0, "面板开着却没占位");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **列表根跟着当前地图换目录**（`--map-list-root` 没给时）。
    ///
    /// 判据：从 `dir_a` 的图切到 `dir_b` 的图 ⇒ 列表根变成 `dir_b`、条目换成 dir_b 的。
    #[test]
    fn 列表根跟着当前地图换目录() {
        let base = std::env::temp_dir().join("miredit_gui_app_maplist_follow");
        let (a, b) = (base.join("a"), base.join("b"));
        let _ = std::fs::remove_dir_all(&base);
        for d in [&a, &b] {
            std::fs::create_dir_all(d).expect("建目录");
        }
        let 造 = |dir: &std::path::Path, name: &str| -> PathBuf {
            let mut bytes = vec![0u8; 52 + 12 * 16];
            bytes[0..2].copy_from_slice(&4u16.to_le_bytes());
            bytes[2..4].copy_from_slice(&4u16.to_le_bytes());
            let p = dir.join(name);
            std::fs::write(&p, &bytes).expect("写图");
            p
        };
        let ma = 造(&a, "only_a.map");
        let mb = 造(&b, "only_b.map");
        let settings = 用例设置路径("maplist_follow");
        let _ = std::fs::remove_file(&settings);
        let mut app = MireditApp::new(&Cli {
            map: Some(ma.clone()),
            settings: Some(settings),
            ..Default::default()
        });
        while app.maps.poll() {}
        assert_eq!(app.maps.len(), 1);
        assert_eq!(app.maps.entries()[0].rel, "only_a.map");
        // 换到 b 目录的图 ⇒ 根跟着换
        app.open_map_path(&mb.display().to_string(), 画布());
        while app.maps.poll() {}
        assert_eq!(
            app.maps.root().map(|p| p.to_path_buf()),
            Some(b.clone()),
            "根必须跟着换到 b"
        );
        assert_eq!(app.maps.entries()[0].rel, "only_b.map");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// **`--map-list-root` 钉住根**：从别的目录开图也不换根（验收要能对固定目录复跑）。
    #[test]
    fn 钉住的列表根不跟着换() {
        let base = std::env::temp_dir().join("miredit_gui_app_maplist_pin");
        let (a, b) = (base.join("a"), base.join("b"));
        let _ = std::fs::remove_dir_all(&base);
        for d in [&a, &b] {
            std::fs::create_dir_all(d).expect("建目录");
        }
        let mut bytes = vec![0u8; 52 + 12 * 16];
        bytes[0..2].copy_from_slice(&4u16.to_le_bytes());
        bytes[2..4].copy_from_slice(&4u16.to_le_bytes());
        let ma = a.join("a.map");
        let mb = b.join("b.map");
        std::fs::write(&ma, &bytes).expect("写图 a");
        std::fs::write(&mb, &bytes).expect("写图 b");
        let settings = 用例设置路径("maplist_pin");
        let _ = std::fs::remove_file(&settings);
        let mut app = MireditApp::new(&Cli {
            map: Some(ma),
            settings: Some(settings),
            map_list_root: Some(a.clone()),
            ..Default::default()
        });
        while app.maps.poll() {}
        app.open_map_path(&mb.display().to_string(), 画布());
        while app.maps.poll() {}
        assert_eq!(
            app.maps.root().map(|p| p.to_path_buf()),
            Some(a),
            "给了 --map-list-root 就该钉住"
        );
        assert_eq!(app.maps.entries()[0].rel, "a.map");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// **「原始大小」按钮**：先放大到 200%，点一下回到 100%（走 `Action::ActualSize`）。
    #[test]
    fn 原始大小按钮回到百分之百() {
        let mut app = 有图应用();
        let canvas = 画布();
        // 先放大两档（走同一个 `zoom_at`，与滚轮一致）
        for _ in 0..2 {
            app.view.zoom_at(canvas, canvas.center(), m::ZOOM_PER_NOTCH);
        }
        assert!(app.view.scale > 1.0, "先要真的放大：{}", app.view.scale);
        // 点按钮 = 派发同一条 action
        app.dispatch_menu(menu::model::Action::ActualSize, canvas);
        assert!(
            (app.view.scale - 1.0).abs() < 1e-4,
            "点了「原始大小」缩放必须回到 100%，实际 {}",
            app.view.scale
        );
        let (text, warn) = app.op.clone().expect("要有回执");
        assert!(!warn, "回到 100% 不是拒绝：{text}");
        assert_eq!(text, tx::MENU_ACTUAL_SIZE_OK);
        assert_eq!(app.view.zoom_percent(), 100);
    }

    /// 快捷键浮层：菜单项把它打开、`Overlay::is_some` 为真；再按 Esc 关掉。
    #[test]
    fn 快捷键浮层可开可关() {
        let mut app = 有图应用();
        let canvas = 画布();
        assert!(app.overlay == Overlay::None);
        app.dispatch_menu(menu::model::Action::KeyboardShortcuts, canvas);
        assert!(app.overlay == Overlay::Shortcuts, "菜单项没打开浮层");
        assert!(app.overlay.is_some());
    }

    /// **`--menu-click file.open_recent`**：容器项**不执行任何动作**，只回一条"去哪选"的话。
    ///
    /// 这条挡的是"脚本/命令行直接派发容器项"时误跑父项动作 ——
    /// 与键盘回车、鼠标点击是同一件事的第三条入口（审查缺陷 3 的三条路要一致）。
    #[test]
    fn 直接派发最近打开只给指路回执() {
        let mut app = 有图应用();
        let canvas = 画布();
        let gen = app.doc.map_gen;
        app.dispatch_menu(menu::model::Action::OpenRecent, canvas);
        let (text, warn) = app.op.clone().expect("必须有回执");
        assert!(!warn, "这不是错误（不用警示色）：{text}");
        assert_eq!(text, menu::model::STATUS_RECENT_CONTAINER);
        assert_eq!(app.doc.map_gen, gen, "容器项不许改地图");
        assert!(app.overlay == Overlay::None, "容器项不许弹浮层");
    }

    /// **「最近打开」端到端**（2026-11-05 接线）：真开两张图 ⇒ 设置文件里的 `recent`
    /// 最近优先；再开第一张 ⇒ 提升到第一位且**不重复**；删掉文件再点它
    /// ⇒ 状态栏给中文原因 + 该条从列表与设置文件里一起移除。
    ///
    /// ⚠ 用的是 `GodotClient/maps/{10,41}.map` 的**临时副本**：真文件是工程资产，
    /// 本用例要"删一个文件再点它"，绝不许动到真文件（删了就是事故）。
    #[test]
    fn 最近打开端到端记入提升与移除() {
        // 路径相对资产根（`MIREDIT_ASSET_ROOT`，缺省本机真实根）——
        // 公开仓/PR 的机器上没有游戏素材 ⇒ 打印 `[skip]` 并返回（不 panic）。
        let Some(root) = 资产根() else {
            eprintln!("[skip] 无资产根 MIREDIT_ASSET_ROOT");
            return;
        };
        let 语料 = root.join("GodotClient").join("maps");
        let src10 = 语料.join("10.map");
        let src41 = 语料.join("41.map");
        if !src10.is_file() || !src41.is_file() {
            // 样本不在 ⇒ 如实跳过（不编假路径，也不假装验过）
            eprintln!(
                "[跳过] 最近打开端到端：缺样本 {} / {}",
                src10.display(),
                src41.display()
            );
            return;
        }
        let dir = std::env::temp_dir().join("miredit_recent_e2e");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建临时目录");
        let m10 = dir.join("10.map");
        let m41 = dir.join("41.map");
        std::fs::copy(&src10, &m10).expect("拷 10.map");
        std::fs::copy(&src41, &m41).expect("拷 41.map");
        let settings = dir.join("settings.json");
        let mut app = MireditApp::new(&Cli {
            map: Some(PathBuf::from(r"E:\__no_such_map__.map")),
            settings: Some(settings.clone()),
            ..Default::default()
        });
        assert!(app.recent.is_empty(), "首次运行（无设置文件）⇒ 没有历史");
        let canvas = 画布();

        // ⓪ **打开失败不记入历史**（单子：只记成功打开的）
        app.open_map_path(r"E:\__no_such_map__.map", canvas);
        assert!(
            app.recent.is_empty(),
            "打不开的路径不许进最近列表：{:?}",
            app.recent
        );
        assert!(!settings.exists(), "失败时一个字节都不许写设置文件");

        // ① 依次打开 10.map、41.map ⇒ 最近优先
        app.open_map_path(&m10.display().to_string(), canvas);
        app.open_map_path(&m41.display().to_string(), canvas);
        let on_disk = menu::settings::load_quiet(&settings).expect("设置文件必须真写出来");
        // 证据：把真写出来的设置文件原样打到测试日志（`--nocapture` 可读）
        eprintln!(
            "[证据] 打开 10.map → 41.map 之后，{} 的内容：\n{}",
            settings.display(),
            std::fs::read_to_string(&settings).unwrap_or_default()
        );
        assert_eq!(
            on_disk.recent,
            vec![m41.display().to_string(), m10.display().to_string()],
            "最近打开的那张必须排第一"
        );
        assert_eq!(app.recent, on_disk.recent, "内存与磁盘必须是同一份");

        // ② 再打开 10.map ⇒ 提升到第一位、且**不重复**
        app.open_map_path(&m10.display().to_string(), canvas);
        let on_disk = menu::settings::load_quiet(&settings).expect("再读设置");
        eprintln!(
            "[证据] 再打开 10.map 之后（提升、不重复）：\n{}",
            std::fs::read_to_string(&settings).unwrap_or_default()
        );
        assert_eq!(
            on_disk.recent,
            vec![m10.display().to_string(), m41.display().to_string()],
            "重新打开要把它提升到第一位"
        );
        assert_eq!(on_disk.recent.len(), 2, "不许出现两份 10.map：{on_disk:?}");
        assert_eq!(app.recent, on_disk.recent);

        // ③ 子菜单的行就是这份列表（文件名 + 都能点）
        let rows = app.sub_state().recent_rows();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, "10.map", "官方取 Path.GetFileName");
        assert_eq!(rows[1].0, "41.map");
        assert!(rows.iter().all(|(_, on)| *on), "两个文件都在 ⇒ 都可点");

        // ④ 删掉 41.map 再"点"它（第 2 行）⇒ 状态栏给中文原因 + 从列表移除
        std::fs::remove_file(&m41).expect("删掉副本 41.map");
        app.open_recent_at(1, canvas);
        let (text, warn) = app.op.clone().expect("必须有回执");
        assert!(warn, "打不开是一条拒绝回执");
        assert!(
            text.contains("最近打开的文件已不存在："),
            "回执要说明原因：{text}"
        );
        assert!(text.contains("41.map"), "回执里要有那条路径：{text}");
        eprintln!("[证据] 点已不存在的 41.map ⇒ 状态栏回执：{text}（warn={warn}）");
        assert_eq!(
            app.recent,
            vec![m10.display().to_string()],
            "点了已不存在的历史 ⇒ 该条必须从列表里移除"
        );
        let on_disk = menu::settings::load_quiet(&settings).expect("三读设置");
        eprintln!(
            "[证据] 移除之后 settings.json：\n{}",
            std::fs::read_to_string(&settings).unwrap_or_default()
        );
        assert_eq!(
            on_disk.recent,
            vec![m10.display().to_string()],
            "移除必须落盘（否则下次启动又冒出来）"
        );
        // ⑤ 当前地图**不受影响**（打开失败不许丢掉手上的图）
        assert_eq!(
            app.doc.doc.as_ref().map(|d| d.file_name.clone()),
            Some("10.map".to_owned()),
            "打开历史失败不许换掉当前文档"
        );

        // ⑥ 清空列表 ⇒ 子菜单只剩一行官方空态行（`--menu-dump` 同一条判据）
        app.recent.clear();
        let rows = app.sub_state().recent_rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, menu::model::SUB_RECENT_EMPTY);
        assert!(!rows[0].1, "空态行灰显");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
