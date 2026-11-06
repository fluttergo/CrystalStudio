//! **菜单项静态表**：5 个菜单 / 39 项（+ 3 个子菜单 / 5 个静态子项 + 1 个动态列表），
//! 逐项照抄官方（分组 = 官方 `<Separator />` 的位置）。
//!
//! # 数据来源（两处互相印证）
//!
//! 1. `CrystalStudio/MainWindow.axaml:5` 的 `<Menu>` 树 —— 每项 `Header` 是
//!    `{local:Loc <Key>}`，`<Separator />` 的位置就是官方分组；
//! 2. `layout/crystalstudio-strings.json` 里同一批 key 的 `Chinese`。
//!
//! 运行时由代码追加的那几项（语言 / 7 个面板开关 / 锁定 / 重置）见
//! `ui_text` 里「下拉菜单项」小节的说明与 [`crate::menu::panels`] 的模块文档。
//!
//! # 40 项的账
//!
//! ```text
//! 文件 10 + 编辑 4 + 视图 19 + 工具 6 + 帮助 1 = 40
//! 视图 19 = XAML 8 + 语言 1 + 面板开关 8 + 锁定/重置 2
//!
//! ⚠ **2026-11-05 起是 40 项**（原 39）：上游 `901dbe2f` 新增了第 8 个面板
//! （`docking.Add("Maps", …)`），而 `DockWorkspace.cs:269 AddSectionMenu` 会为
//! **每一个**面板生成一条 CheckBox 菜单项 ⇒ 面板开关 7 → 8，视图 18 → 19。
//! ```
//!
//! # 子菜单的账（2026-11-05 补：审查缺陷 1 / 2）
//!
//! 官方有 3 项是**可展开容器**，本仓 2026-11-05 之前把它们压成了叶子项
//! （内部审查报告《审查-菜单 39 项》缺陷 1、2；已移出公开仓）。现在按 [`Sub`] 展开：
//!
//! ```text
//! 文件 → 最近打开      动态列表（官方 RecentMaps.cs:27-39）；行 = 设置文件里的 `recent` 列表
//!                      （最近优先，最多 10 条，见 [`crate::menu::settings`]）；
//!                      列表为空 ⇒ 只出现官方空态行「暂无最近地图」（:39，IsEnabled=false）
//! 视图 → 地图光照      3 个复选子项：黎明 / 黄昏(D) · 白天(D)[官方 IsChecked="True"] · 夜晚(N)
//! 视图 → 语言（重启后生效） 2 个复选子项：简体中文 · English（官方 Localization.cs:10-11）
//! ```
//!
//! 静态子项共 **5** 个（[`sub_count`]），另有 1 个动态列表（行数随 `recent` 变：0 条 ⇒ 1 行空态）。
//! 子项**不进** [`MENUS`] 的 `items`（那是"顶级 39 项"的口径），
//! 所以 [`item_count`] 仍是 40、`--menu-dump` 的汇总行仍是 40 —— 子项在 dump 里
//! 靠 `parent=` 列自证（见 [`crate::menu::dump_lines`]）。
//!
//! [`tests::顶级39项与子项5项计数`] 把这两笔账都钉住。
//!
//! # 每项的三态
//!
//! * [`State::Ready`] —— 可点、真执行；
//! * [`State::NeedsMap`] —— **需要先开图**（没有地图时灰显，点了状态栏说原因）；
//! * [`State::Todo`] —— **未实现**（永远灰显，点了状态栏说 `未实现：<原因>`）。
//!
//! 判据只有一个出口 [`state`]：绘制、鼠标命中、命令行 `--menu-click`、`--menu-dump`
//! 全部问它，**不许各写一套**。

use crate::menu::panels::PanelId;
use crate::ui_text as tx;
pub use miredit_render::MapLighting as MapLight;

// ───────────────────────── 子项文案（2026-11-05） ─────────────────────────
//
// ⚠ **这 6 条暂放本模块**：`ui_text.rs` 不在本单写域。它们与 `ui_text::MENU_*`
// 是同一类东西（界面文案），待 `ui_text` 归属方回收时整体搬过去，并纳入
// `ui_text::ALL`（那样才会被 `fonts::tests` 的字形覆盖测试扫到）。

/// 地图光照子项：`黎明 / 黄昏(D)`（官方 `Strings.json` 的 `DawnDusk_FDEA5B80` = `黎明 / 黄昏(_D)`）。
pub const SUB_LIGHT_DAWN_DUSK: &str = "黎明 / 黄昏(D)";
/// 地图光照子项：`白天(D)`（官方 `Day_0CB25AFB` = `白天(_D)`；官方 `IsChecked="True"`）。
pub const SUB_LIGHT_DAY: &str = "白天(D)";
/// 地图光照子项：`夜晚(N)`（官方 `Night_F7AFD490` = `夜晚(_N)`）。
pub const SUB_LIGHT_NIGHT: &str = "夜晚(N)";
/// 语言子项：`简体中文`（官方 `MainWindow.Localization.cs:10` 的字面量，**无助记符**）。
pub const SUB_LANGUAGE_ZH_CN: &str = "简体中文";
/// 语言子项：`English`（官方 `MainWindow.Localization.cs:11` 的字面量，**无助记符**）。
pub const SUB_LANGUAGE_EN: &str = "English";
/// 「最近打开」的空态行：`暂无最近地图`
/// （官方 `Strings.json` 的 `NoRecentMaps_156CD974`，由 `MainWindow.RecentMaps.cs:39`
/// 以 `IsEnabled = false` 加进子菜单）。
pub const SUB_RECENT_EMPTY: &str = "暂无最近地图";

/// 空态行在诊断里的**伪 id**（`--menu-dump` 用）。
///
/// 它不是一个可派发的动作（官方那条 `MenuItem` 连 `Click` 都没有、还 `IsEnabled=false`），
/// 所以不进 [`Action`]，只在这里给一个稳定的诊断标识。
pub const ID_RECENT_EMPTY: &str = "file.open_recent.empty";

/// 空态行在诊断里的**状态串**（`--menu-dump` 用）。
///
/// 它既不是"已实现"也不是"未实现"（它本来就不是动作），所以单列一个词，
/// 免得验收脚本把它数进「未实现 20」。
pub const STATE_RECENT_EMPTY: &str = "空态（灰显）";

// ─────────────────── 最近地图（2026-11-05 接线，`file.open_recent`） ───────────────────
//
// ⚠ 与上面那批一样，这几条**暂放本模块**（`ui_text.rs` 不在本单写域）。
// 它们是界面文案（状态栏回执 / stderr 诊断），待 `ui_text` 归属方回收时整体搬过去，
// 并纳入 `ui_text::ALL`（那样才会被 `fonts::tests` 的字形覆盖测试扫到 ——
// 在那之前由 [`tests::子项文案字形齐全`] 代管）。

/// 动态行在诊断里的 **id 前缀**：`file.open_recent.1` / `.2` …（`--menu-dump` 用，1 基）。
///
/// 官方那 20 条子项没有名字（运行时 `Items.Add`），所以只能按序号给一个稳定标识；
/// 序号 = 列表位置（1 = 最近打开的那张）。
pub const ID_RECENT_ROW_PREFIX: &str = "file.open_recent.";

/// 动态行的状态串：文件**已不存在**（官方 `IsEnabled = File.Exists(path)` = false ⇒ 灰显）。
///
/// 文件还在的行用 [`crate::ui_text::STATE_READY`]（与其它"能点"的项同一个词，
/// 不另造一个新词，免得验收脚本要认两种"能点"）。
pub const STATE_RECENT_GONE: &str = "文件已不存在（灰显）";

/// 「点了一条最近地图，但文件已不存在」——状态栏给用户的原因（`{path}` = 完整路径）。
///
/// 官方只在 ToolTip 里说「`{path}`\nFile no longer exists.」（`RecentMaps.cs:35`），
/// 且**不把这条从列表里去掉**；本仓按 2026-11-05 单子：状态栏给中文原因 **并移除**该条
/// （见 [`crate::menu::action::open_recent_index`]）。**有意偏离**，理由：留一条点不动的
/// 死条目只会让人反复点，删掉才自洽。
pub const STATUS_RECENT_GONE_TMPL: &str = "最近打开的文件已不存在：{path}";

/// 「最近打开」是**子菜单容器**（官方 `OpenRecentButton` 没有 `Click`）⇒
/// 直接派发它（`--menu-click file.open_recent`）时的回执：**不执行任何动作**，只说去哪选。
pub const STATUS_RECENT_CONTAINER: &str = "「最近打开」是子菜单：请从右侧列表选一项";

/// 记住一条最近地图（[`crate::menu::action::open_path`] 成功之后）的诊断行。
pub const DIAG_RECENT_REMEMBERED: &str = "[菜单] 已记入最近地图（{n} 条）：{path}";

/// 设置**写盘失败**的诊断行（深色偏好与最近地图共用；写失败不影响刚做完的操作）。
pub const DIAG_SETTINGS_WRITE_FAIL: &str = "[设置] 写入失败：{path}：{err}";

/// 最近地图**已不存在** ⇒ 从列表移除的诊断行（状态栏那句之外，给脚本一个可抓的出口）。
pub const DIAG_RECENT_GONE: &str = "[菜单] 最近地图已不存在，已从列表移除：{path}";

/// `--menu-dump` **读设置失败**的诊断行（按空列表显示，但要说出来 —— 不许假装"没有历史"）。
pub const DIAG_RECENT_UNREADABLE: &str = "[菜单] 最近地图列表读取失败（按空列表显示）：{path}：{err}";

/// 最近地图下标**越界**的回执（列表在"点击"与"派发"之间被改短了才会出现）。
pub const STATUS_RECENT_STALE: &str = "最近地图列表已变化：请重新展开「最近打开」再选";


/// 「重做」**在菜单里**显示的快捷键：官方 `MainWindow.axaml:5` 只写 `Ctrl+Shift+Z`。
///
/// ⚠ 与 [`crate::ui_text::KEY_CTRL_SHIFT_Z`]（= `Ctrl+Shift+Z / Ctrl+Y`）**故意不同**，
/// 这不是"两套不一致"，而是两个不同的口径：
///
/// * **菜单**要照官方显示 —— 官方菜单手势只有 `Ctrl+Shift+Z`（`Ctrl+Y` 官方只在
///   `MainWindow.axaml.cs:848` 的 `KeyDownHandler` 里绑，**不写进菜单**）；
/// * **帮助 → 键盘快捷键浮层**列的是"本仓真实绑定了什么"，本仓确实两个键都吃
///   ⇒ 那里必须写全（`menu::shortcuts` 用的是 `ui_text::KEY_CTRL_SHIFT_Z`）。
///
/// 审查（缺陷 3）判定的就是"菜单里多出 `/ Ctrl+Y`"这一条显示层偏离。
pub const KEY_REDO_MENU: &str = "Ctrl+Shift+Z";

/// 子菜单内容。
#[derive(Clone, Copy, Debug)]
pub enum Sub {
    /// 静态子项（官方 XAML 里就写死的：地图光照 3 项、语言 2 项）。
    Static(&'static [Item]),
    /// **动态列表**（官方 `MainWindow.RecentMaps.cs:27-39` 往 `OpenRecentButton.Items` 里填）。
    ///
    /// 行数随 [`SubState::recent`] 变；空列表时按官方 `:39` 加一条灰显的
    /// [`SUB_RECENT_EMPTY`]。
    ///
    /// # 官方有没有「清除最近列表」这个入口？（2026-11-05 逐行查证：**没有**）
    ///
    /// * `MainWindow.RecentMaps.cs` 全文 41 行只做三件事：`Load()`（`:10`）、
    ///   `RememberMap`（`:16-21`）、`BuildRecentMapMenu`（`:23-40`）—— 菜单里**只有**
    ///   每条历史一行（`:30-37`）与空态行（`:39`）；
    /// * `RecentMapStore.cs` 全文 40 行只有 `Load` / `Remember`，**没有任何
    ///   `Clear` / `Remove` / 清空历史的 API**；
    /// * `:25` 的 `OpenRecentButton.Items.Clear()` 只是"重建菜单前的清屏"，
    ///   **不是**用户可点的「清除最近列表」。
    ///
    /// ⇒ 本仓**不做**这个入口（无原作依据的功能不做 —— 项目铁律「禁造无原作依据的功能」）。
    /// 想清空历史，用户可以删 `settings.json` 里的 `recent` 数组（文件本身是人可读的）。
    Recent,
}

/// 界面语言（官方 `MainWindow.Localization.cs:12-13` 的 `ChooseLanguage("zh-CN" / "en-US")`）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Lang {
    /// `简体中文`（官方缺省，`Localization.cs:34` 的 `selected == "zh-CN"`）。
    #[default]
    ZhCn,
    /// `English`。
    En,
}

impl Lang {
    /// 官方语言码原文（诊断用）。
    pub const fn code(self) -> &'static str {
        match self {
            Lang::ZhCn => "zh-CN",
            Lang::En => "en-US",
        }
    }
}

/// 子菜单里那几个"本仓还没有真状态"的勾选态。
///
/// # 为什么需要它
///
/// 顶级 40 项里真正有**运行期真状态**的有五样：深色模式（`app.dark`）、
/// 单元格网格（`app.grid`）、开启门（`app.doors_open`）、**地图光照（`app.lighting`）**、
/// 8 个面板显隐（`app.panels`）—— 它们由 [`check`] / [`check_with`] 直接从宿主字段读，
/// **不经过本结构**（光照那一档尤其不许再存第二份，见下）。
///
/// 子菜单里还剩两类没有可写真状态的：
///
/// * 语言：本仓文案只有中文一版（`State::Todo`），没有语言偏好；
/// * 最近打开：**已接线**（2026-11-05）—— 列表真源是设置文件里的
///   `recent`（[`crate::menu::settings`]），由 [`crate::app::MireditApp`] 每帧喂进来。
///
/// ⇒ 语言的勾选态取官方缺省（简体中文）；**绝不编一个用户没设过的状态**。
///
/// ⚠ **2026-11-05 地图光照接线后**：`app.lighting: MapLight`（真源，类型就是
/// `miredit_render::MapLighting`）由 [`check_with`] 的 `lighting` 参数直接喂进来，
/// 本结构**不再有 `light` 字段** —— 一处真源、一个缓存键、一次渲染，没有第二份档位。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SubState {
    /// 当前界面语言（官方缺省 = [`Lang::ZhCn`]，`Localization.cs:34` 的 `selected == "zh-CN"`）。
    pub language: Lang,
    /// 最近地图列表（官方 `RecentMapStore` 里的路径，**新→旧**）。
    pub recent: Vec<String>,
}

impl SubState {
    /// 「最近打开」子菜单要画的行：`(文案, 能不能点)`。
    ///
    /// 照官方 `MainWindow.RecentMaps.cs:27-39`：
    /// * 有列表 ⇒ 每条形如 `Path.GetFileName(path)`，`IsEnabled = File.Exists(path)`；
    /// * 空列表 ⇒ 一条 [`SUB_RECENT_EMPTY`]，`IsEnabled = false`。
    ///
    /// 列表来自 [`SubState::recent`]（设置文件的 `recent` 字段，最近优先）；
    /// 空 ⇒ 走第二条（官方空态行）。
    pub fn recent_rows(&self) -> Vec<(String, bool)> {
        if self.recent.is_empty() {
            return vec![(SUB_RECENT_EMPTY.to_owned(), false)];
        }
        self.recent
            .iter()
            .map(|p| {
                let name = std::path::Path::new(p)
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| p.clone());
                (name, std::path::Path::new(p).is_file())
            })
            .collect()
    }

    /// 子菜单**有多少行**（键盘导航要它 —— `Sub::Recent` 的行数随列表变）。
    ///
    /// 与 [`crate::menu::popup::SubGeom::rows`] 的行数**必须一致**（绘制与键盘不能各数一套）：
    /// 静态子项 = 表长；动态列表 = 空 ? 1（空态行）: `recent.len()`。
    pub fn sub_rows(&self, sub: Sub) -> usize {
        match sub {
            Sub::Static(items) => items.len(),
            Sub::Recent => {
                if self.recent.is_empty() {
                    1
                } else {
                    self.recent.len()
                }
            }
        }
    }

    /// 子菜单第 `row` 行**能不能点**（回车/点击前问它一次，别自己判）。
    ///
    /// * 静态子项：问 [`state`] 的三态（光照/语言都是 `Todo` ⇒ 能停但点了给原因）；
    /// * 动态列表：空列表 ⇒ 空态行**没有动作**（`None`）；非空 ⇒ 有动作，
    ///   但**文件已不存在**时按官方 `IsEnabled=false` 灰显（灰行仍可点，点了给原因 + 移除）。
    pub fn sub_row_action(&self, sub: Sub, row: usize) -> Option<Action> {
        match sub {
            Sub::Static(items) => items.get(row).map(|i| i.action),
            Sub::Recent => {
                if self.recent.is_empty() || row >= self.recent.len() {
                    None
                } else {
                    // 每一行都是"打开这一条"；**哪一条**由调用方按下标派发
                    // （`Click::RunRecent` / `KeyResult::RunRecent`），不是这个枚举值。
                    Some(Action::OpenRecent)
                }
            }
        }
    }

    /// 动态行（[`Sub::Recent`]）第 `row` 行对应的**完整路径**（`None` = 空态行 / 越界）。
    ///
    /// 点/回车一条最近地图时要的是完整路径，而子面板只画文件名
    /// （官方 `Path.GetFileName`）⇒ 只能按下标回查列表。
    pub fn recent_at(&self, row: usize) -> Option<&str> {
        if self.recent.is_empty() {
            return None;
        }
        self.recent.get(row).map(|s| s.as_str())
    }
}

/// 一条菜单动作（= 一个可派发的命令）。
///
/// 与 [`crate::menu::action::apply`] 一一对应：这里只描述"是哪一条"，
/// 真正干活的判定/写盘/回执都在 `action` 里。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    // ── 文件 ──
    NewMap,
    OpenMap,
    OpenRecent,
    SaveMap,
    SaveAs,
    ExportMinimap,
    NewStamp,
    ImportStamp,
    ExportSelection,
    Exit,
    // ── 编辑 ──
    Undo,
    Redo,
    FillSelection,
    Settings,
    // ── 视图 ──
    DarkMode,
    CellGrid,
    MapLighting,
    OpenDoors,
    ZoomIn,
    ZoomOut,
    ActualSize,
    FitMap,
    Language,
    // ── 视图 · 地图光照的 3 个复选子项（官方 `MapLightingMenu` 的子菜单）──
    /// `黎明 / 黄昏(D)`（官方 `Tag="DawnDusk"`）。
    LightDawnDusk,
    /// `白天(D)`（官方 `Tag="Day"`，`IsChecked="True"` ⇒ 缺省勾选）。
    LightDay,
    /// `夜晚(N)`（官方 `Tag="Night"`）。
    LightNight,
    // ── 视图 · 语言的 2 个复选子项（官方 `LanguageMenu` 的子菜单）──
    /// `简体中文`（官方 `Name="ChineseLanguage"`）。
    LangChinese,
    /// `English`（官方 `Name="EnglishLanguage"`）。
    LangEnglish,
    /// 面板显隐（7 个，每个一条菜单项）。
    Panel(PanelId),
    LockLayout,
    ResetLayout,
    // ── 工具 ──
    GenTown,
    GenTerrain,
    GenProvince,
    GenDungeon,
    IdentifyObjects,
    ExtractObjects,
    // ── 帮助 ──
    KeyboardShortcuts,
}

impl Action {
    /// 诊断/命令行用的 ASCII 键（`--menu-click view.zoom_in`）。
    pub const fn id(self) -> &'static str {
        match self {
            Action::NewMap => "file.new_map",
            Action::OpenMap => "file.open_map",
            Action::OpenRecent => "file.open_recent",
            Action::SaveMap => "file.save_map",
            Action::SaveAs => "file.save_as",
            Action::ExportMinimap => "file.export_minimap",
            Action::NewStamp => "file.new_stamp",
            Action::ImportStamp => "file.import_stamp",
            Action::ExportSelection => "file.export_selection",
            Action::Exit => "file.exit",
            Action::Undo => "edit.undo",
            Action::Redo => "edit.redo",
            Action::FillSelection => "edit.fill_selection",
            Action::Settings => "edit.settings",
            Action::DarkMode => "view.dark_mode",
            Action::CellGrid => "view.cell_grid",
            Action::MapLighting => "view.map_lighting",
            Action::OpenDoors => "view.open_doors",
            Action::ZoomIn => "view.zoom_in",
            Action::ZoomOut => "view.zoom_out",
            Action::ActualSize => "view.actual_size",
            Action::FitMap => "view.fit_map",
            Action::Language => "view.language",
            Action::LightDawnDusk => "view.light_dawn_dusk",
            Action::LightDay => "view.light_day",
            Action::LightNight => "view.light_night",
            Action::LangChinese => "view.language_zh_cn",
            Action::LangEnglish => "view.language_en",
            Action::Panel(PanelId::Maps) => "view.panel_maps",
            Action::Panel(PanelId::Tools) => "view.panel_tools",
            Action::Panel(PanelId::ToolOptions) => "view.panel_tool_options",
            Action::Panel(PanelId::Layers) => "view.panel_layers",
            Action::Panel(PanelId::Minimap) => "view.panel_minimap",
            Action::Panel(PanelId::Palette) => "view.panel_palette",
            Action::Panel(PanelId::ActiveAsset) => "view.panel_active_asset",
            Action::Panel(PanelId::CellInspector) => "view.panel_cell_inspector",
            Action::LockLayout => "view.lock_layout",
            Action::ResetLayout => "view.reset_layout",
            Action::GenTown => "tools.gen_town",
            Action::GenTerrain => "tools.gen_terrain",
            Action::GenProvince => "tools.gen_province",
            Action::GenDungeon => "tools.gen_dungeon",
            Action::IdentifyObjects => "tools.identify_objects",
            Action::ExtractObjects => "tools.extract_objects",
            Action::KeyboardShortcuts => "help.keyboard_shortcuts",
        }
    }

    /// 菜单项文字（全部来自 [`crate::ui_text`]）。
    pub const fn label(self) -> &'static str {
        match self {
            Action::NewMap => tx::MENU_NEW_MAP,
            Action::OpenMap => tx::MENU_OPEN_MAP,
            Action::OpenRecent => tx::MENU_OPEN_RECENT,
            Action::SaveMap => tx::MENU_SAVE_MAP,
            Action::SaveAs => tx::MENU_SAVE_AS,
            Action::ExportMinimap => tx::MENU_EXPORT_MINIMAP,
            Action::NewStamp => tx::MENU_NEW_STAMP,
            Action::ImportStamp => tx::MENU_IMPORT_STAMP,
            Action::ExportSelection => tx::MENU_EXPORT_SELECTION,
            Action::Exit => tx::MENU_EXIT,
            Action::Undo => tx::MENU_UNDO,
            Action::Redo => tx::MENU_REDO,
            Action::FillSelection => tx::MENU_FILL_SELECTION,
            Action::Settings => tx::MENU_SETTINGS,
            Action::DarkMode => tx::MENU_DARK_MODE,
            Action::CellGrid => tx::MENU_CELL_GRID,
            Action::MapLighting => tx::MENU_MAP_LIGHTING,
            Action::OpenDoors => tx::MENU_OPEN_DOORS,
            Action::ZoomIn => tx::MENU_ZOOM_IN,
            Action::ZoomOut => tx::MENU_ZOOM_OUT,
            Action::ActualSize => tx::MENU_ACTUAL_SIZE,
            Action::FitMap => tx::MENU_FIT_MAP,
            Action::Language => tx::MENU_LANGUAGE,
            Action::LightDawnDusk => SUB_LIGHT_DAWN_DUSK,
            Action::LightDay => SUB_LIGHT_DAY,
            Action::LightNight => SUB_LIGHT_NIGHT,
            Action::LangChinese => SUB_LANGUAGE_ZH_CN,
            Action::LangEnglish => SUB_LANGUAGE_EN,
            Action::Panel(id) => id.title(),
            Action::LockLayout => tx::MENU_LOCK_LAYOUT,
            Action::ResetLayout => tx::MENU_RESET_LAYOUT,
            Action::GenTown => tx::MENU_GEN_TOWN,
            Action::GenTerrain => tx::MENU_GEN_TERRAIN,
            Action::GenProvince => tx::MENU_GEN_PROVINCE,
            Action::GenDungeon => tx::MENU_GEN_DUNGEON,
            Action::IdentifyObjects => tx::MENU_IDENTIFY_OBJECTS,
            Action::ExtractObjects => tx::MENU_EXTRACT_OBJECTS,
            Action::KeyboardShortcuts => tx::MENU_KEYBOARD_SHORTCUTS,
        }
    }

    /// 官方 `InputGesture`（`MainWindow.axaml:5`）；没有就是 `None`。
    ///
    /// ⚠ 这里只放**官方真的写了**的那些键：`Ctrl+N` / `Ctrl+O` / `Ctrl+S` /
    /// `Ctrl+Shift+S` / `Ctrl+Shift+N` / `Ctrl+Z` / `Ctrl+Shift+Z` / `G` / `F1`。
    /// 菜单右侧显示的快捷键文字就取它（**不是**"我们实现了什么"）。
    /// 我们**实际**绑定了什么，由帮助 → 键盘快捷键那个浮层列（见 `menu::shortcuts`）。
    ///
    /// 两个口径**故意可以不同**，唯一一处就是「重做」：菜单显示 [`KEY_REDO_MENU`]
    /// （官方菜单只有 `Ctrl+Shift+Z`），浮层显示 `ui_text::KEY_CTRL_SHIFT_Z`
    /// （本仓 `Ctrl+Shift+Z` 与 `Ctrl+Y` 两个键都真吃）。见 [`KEY_REDO_MENU`] 的注释。
    pub const fn shortcut(self) -> Option<&'static str> {
        match self {
            Action::NewMap => Some(tx::KEY_CTRL_N),
            Action::OpenMap => Some(tx::KEY_CTRL_O),
            Action::SaveMap => Some(tx::KEY_CTRL_S),
            Action::SaveAs => Some(tx::KEY_CTRL_SHIFT_S),
            Action::NewStamp => Some(tx::KEY_CTRL_SHIFT_N),
            Action::Undo => Some(tx::KEY_CTRL_Z),
            // ⚠ 菜单口径：**只** `Ctrl+Shift+Z`（官方菜单就这一个）。
            // 浮层口径（含 `Ctrl+Y`）在 `menu::shortcuts`，别把两者混成一条。
            Action::Redo => Some(KEY_REDO_MENU),
            Action::CellGrid => Some(tx::KEY_G),
            Action::KeyboardShortcuts => Some(tx::KEY_F1),
            _ => None,
        }
    }
}

/// 一项的三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// 已实现：可点、真执行。
    Ready,
    /// 需要先开图（没有地图时灰显）。
    NeedsMap,
    /// 未实现：永远灰显，`why` 是**给用户看的原因**。
    Todo(&'static str),
}

impl State {
    /// 诊断/`--menu-dump` 用的中文状态串。
    pub fn text(self) -> String {
        match self {
            State::Ready => tx::STATE_READY.to_owned(),
            State::NeedsMap => tx::STATE_NEED_MAP.to_owned(),
            State::Todo(why) => tx::fill(tx::STATE_TODO_TMPL, &[("why", why)]),
        }
    }

    /// 现在能不能点（`has_map` = 有没有打开地图）。
    pub fn enabled(self, has_map: bool) -> bool {
        match self {
            State::Ready => true,
            State::NeedsMap => has_map,
            State::Todo(_) => false,
        }
    }

    /// 「为什么不能点」那一句；`Ready` / `NeedsMap` 返回状态名本身（**绝不编一句假原因**）。
    pub fn todo_reason(self) -> &'static str {
        match self {
            State::Todo(why) => why,
            State::Ready => tx::STATE_READY,
            State::NeedsMap => tx::STATE_NEED_MAP,
        }
    }
}

/// 一项菜单项。
#[derive(Clone, Copy, Debug)]
pub struct Item {
    /// 动作。
    pub action: Action,
    /// 它上面要不要先画一条分隔线（= 官方 XAML 里的 `<Separator />`）。
    pub sep_before: bool,
    /// **子菜单**（`None` = 叶子项）。官方这 3 项是可展开容器，不是可执行动作
    /// （见模块文档「子菜单的账」）。
    pub sub: Option<Sub>,
}

/// 一个菜单。
#[derive(Clone, Copy, Debug)]
pub struct Menu {
    /// 菜单栏上的标题（[`crate::ui_text::MENU_ITEMS`] 的同一批常量）。
    pub title: &'static str,
    /// 菜单项（顺序即显示顺序）。
    pub items: &'static [Item],
}

/// 便捷构造（表里少写点字）。
const fn it(action: Action, sep_before: bool) -> Item {
    Item {
        action,
        sep_before,
        sub: None,
    }
}

/// 便捷构造：**带子菜单**的项（官方那 3 个可展开容器）。
const fn it_sub(action: Action, sep_before: bool, sub: Sub) -> Item {
    Item {
        action,
        sep_before,
        sub: Some(sub),
    }
}

/// 地图光照子项（官方 `MainWindow.axaml:5` 的 `MapLightingMenu` 下 3 个
/// `ToggleType="CheckBox"` 子项，顺序即显示顺序）。
pub const LIGHT_ITEMS: &[Item] = &[
    it(Action::LightDawnDusk, false),
    it(Action::LightDay, false),
    it(Action::LightNight, false),
];

/// 语言子项（官方 `MainWindow.Localization.cs:10-11`，顺序即 `menu.Items.Add` 的顺序）。
pub const LANGUAGE_ITEMS: &[Item] = &[
    it(Action::LangChinese, false),
    it(Action::LangEnglish, false),
];

/// 文件菜单（10 项，分隔线在 #6/#7 之间与 #9/#10 之间 —— 官方 XAML 原样）。
///
/// #3「最近打开」是**子菜单**（官方 `OpenRecentButton` 无 `Click`，是容器型菜单项）。
pub const FILE_ITEMS: [Item; 10] = [
    it(Action::NewMap, false),
    it(Action::OpenMap, false),
    it_sub(Action::OpenRecent, false, Sub::Recent),
    it(Action::SaveMap, false),
    it(Action::SaveAs, false),
    it(Action::ExportMinimap, false),
    it(Action::NewStamp, true),
    it(Action::ImportStamp, false),
    it(Action::ExportSelection, false),
    it(Action::Exit, true),
];

/// 编辑菜单（4 项）。
pub const EDIT_ITEMS: [Item; 4] = [
    it(Action::Undo, false),
    it(Action::Redo, false),
    it(Action::FillSelection, true),
    it(Action::Settings, true),
];

/// 视图菜单（19 项）。
///
/// 分隔线位置照官方：深色模式｜单元格网格｜地图光照+开启门｜放大/缩小/实际大小/适应地图｜
/// 语言｜7 个面板｜锁定/重置。
///
/// `地图光照`（#3）与 `语言`（#9）是**子菜单**（官方 XAML 里 `MapLightingMenu` 就带 3 个
/// 子项；`语言` 由 `MainWindow.Localization.cs:15` 追加时也带 2 个）。
pub const VIEW_ITEMS: [Item; 19] = [
    it(Action::DarkMode, false),
    it(Action::CellGrid, true),
    it_sub(Action::MapLighting, true, Sub::Static(LIGHT_ITEMS)),
    it(Action::OpenDoors, false),
    it(Action::ZoomIn, true),
    it(Action::ZoomOut, false),
    it(Action::ActualSize, false),
    it(Action::FitMap, false),
    it_sub(Action::Language, true, Sub::Static(LANGUAGE_ITEMS)),
    it(Action::Panel(PanelId::Maps), true),
    it(Action::Panel(PanelId::Tools), false),
    it(Action::Panel(PanelId::ToolOptions), false),
    it(Action::Panel(PanelId::Layers), false),
    it(Action::Panel(PanelId::Minimap), false),
    it(Action::Panel(PanelId::Palette), false),
    it(Action::Panel(PanelId::ActiveAsset), false),
    it(Action::Panel(PanelId::CellInspector), false),
    it(Action::LockLayout, true),
    it(Action::ResetLayout, false),
];

/// 工具菜单（6 项，分隔线在 4/5 之间）。
pub const TOOLS_ITEMS: [Item; 6] = [
    it(Action::GenTown, false),
    it(Action::GenTerrain, false),
    it(Action::GenProvince, false),
    it(Action::GenDungeon, false),
    it(Action::IdentifyObjects, true),
    it(Action::ExtractObjects, false),
];

/// 帮助菜单（1 项）。
pub const HELP_ITEMS: [Item; 1] = [it(Action::KeyboardShortcuts, false)];

/// 5 个菜单（顺序 = 菜单栏顺序）。
pub const MENUS: [Menu; 5] = [
    Menu {
        title: tx::MENU_FILE,
        items: &FILE_ITEMS,
    },
    Menu {
        title: tx::MENU_EDIT,
        items: &EDIT_ITEMS,
    },
    Menu {
        title: tx::MENU_VIEW,
        items: &VIEW_ITEMS,
    },
    Menu {
        title: tx::MENU_TOOLS,
        items: &TOOLS_ITEMS,
    },
    Menu {
        title: tx::MENU_HELP,
        items: &HELP_ITEMS,
    },
];

/// 全部菜单项总数（= 40）。
pub fn item_count() -> usize {
    MENUS.iter().map(|m| m.items.len()).sum()
}

/// 静态子项总数（= **5**：地图光照 3 + 语言 2）。
///
/// 「最近打开」的动态列表**不计入**（行数随设置文件里的 `recent` 变：
/// 0 条 ⇒ 1 行官方空态行）。子项**不进** [`MENUS`] 的 `items`，所以 [`item_count`] 仍是 40
/// —— 两笔账分开数，免得"加了子项就把顶级 39 撑成 44"。
pub fn sub_count() -> usize {
    let mut n = 0;
    for m in &MENUS {
        for item in m.items {
            n += sub_items(item).len();
        }
    }
    n
}

/// 某一项的**静态子项**（`None`/动态列表 ⇒ 空切片）。
///
/// 动态列表（[`Sub::Recent`]）的行要用 [`SubState::recent_rows`] 取（行数随数据变）。
pub fn sub_items(item: &Item) -> &'static [Item] {
    match item.sub {
        Some(Sub::Static(items)) => items,
        _ => &[],
    }
}

/// 按 id 找 `(菜单下标, 菜单内下标, 项)`。
pub fn find_by_id(id: &str) -> Option<(usize, usize, &'static Item)> {
    for (mi, m) in MENUS.iter().enumerate() {
        for (ii, item) in m.items.iter().enumerate() {
            if item.action.id() == id {
                return Some((mi, ii, item));
            }
        }
    }
    None
}

/// **按 id 找动作**（顶级项 ∪ 静态子项）——`--menu-click` 的唯一校验/派发入口
/// （2026-11-05 地图光照接线时补）。
///
/// 为什么需要它：`--menu-click view.light_night` 里的 `view.light_night` 是**子项** id，
/// 原来的 [`find_by_id`] 只扫顶级 40 项 ⇒ 判成「没有这个菜单项」并报参数错误，
/// 而验收要求「同一会话点一档光照 ⇒ 切档 + 重渲」必须能无人值守复现。
/// 现在 [`crate::cli::Cli::parse`] 与 [`crate::app::MireditApp`] 的 `pending_menu_clicks`
/// 都问这一个函数：认得顶级项与静态子项，动态行（最近打开）仍不支持（它们没有稳定 id）。
pub fn find_action_by_id(id: &str) -> Option<Action> {
    if let Some((_, _, item)) = find_by_id(id) {
        return Some(item.action);
    }
    find_sub_by_id(id).map(|(_, _, _, item)| item.action)
}

/// 按 id 找**子项**：`(菜单下标, 父项下标, 子项下标, 子项)`。
///
/// 与 [`find_by_id`] 分开，是因为 [`crate::menu::action::apply`] 只吃顶级项的
/// `(菜单下标, 菜单内下标)` 二元定位（`--menu-click` 也按那个口径校验）；
/// 子项的 id 只用于诊断/`--menu-dump` 自证与单元测试。
pub fn find_sub_by_id(id: &str) -> Option<(usize, usize, usize, &'static Item)> {
    for (mi, m) in MENUS.iter().enumerate() {
        for (pi, item) in m.items.iter().enumerate() {
            for (si, sub) in sub_items(item).iter().enumerate() {
                if sub.action.id() == id {
                    return Some((mi, pi, si, sub));
                }
            }
        }
    }
    None
}

/// **三态的唯一判据**。
pub const fn state(action: Action) -> State {
    match action {
        // ── 已实现（真执行） ──
        Action::OpenMap | Action::DarkMode | Action::KeyboardShortcuts => State::Ready,
        Action::Panel(_) => State::Ready,
        // 「单元格网格」2026-11-05 起**真实现**：画布上按世界格 48×32 画格线
        // （`crate::grid`），`G` / 菜单 / `--cell-grid` / `--no-cell-grid` 都能切换。
        // 官方是 `ToggleType="CheckBox"` + `IsChecked="True"` ⇒ **默认勾选**；
        // 本仓 2026-11-05 之前默认关，审查缺陷 4 判定为形态偏离后已订正为默认开
        // （判据 `crate::cli::Cli::cell_grid_enabled`，理由见 `crate::grid` 模块文档 §五）。
        Action::CellGrid => State::Ready,
        // 「开启门」2026-11-05 起**真实现**：渲染层早已有能力（`miredit-render` 的
        // `RenderOptions::doors_open`：`doors_open && (door_index & 0x7F) != 0` 时
        // `帧号 += door_offset & 0x7F`，**只动前景层**），门态字段就在
        // `mir2-map/src/cell.rs` 的 `door_index` / `door_offset`；本仓把它接到了
        // 生产路径：`app.doors_open`（真源）→ `CanvasView::ensure`（缓存键 + 渲染选项）
        // → `try_render_region_with_options`。菜单勾、回执、`--open-doors` 读的都是这一个字段。
        //
        // 为什么是 `Ready` 而不是 `NeedsMap`：没有地图时它也不灰显 ——
        // 切换一个"渲染开关"本身不依赖地图（没有地图时画布是空态，勾照样能翻），
        // 与「深色模式」「单元格网格」同一条口径。
        Action::OpenDoors => State::Ready,
        Action::SaveMap
        | Action::Undo
        | Action::Redo
        | Action::FillSelection
        | Action::ZoomIn
        | Action::ZoomOut
        | Action::ActualSize
        | Action::FitMap => State::NeedsMap,
        // 「另存为」2026-11-05 起**真实现**：菜单项 / `Ctrl+Shift+S` / `--save-as` 三条入口
        // 都打开同一个路径输入模态（`Overlay::SaveAs`），回车后走
        // [`crate::save::save_as_session`]（复用同一套原子写：tmp → sync → 读回核对 →
        // rename → 尽力 fsync 目录），成功再把文档路径与标题改成新路径。
        //
        // 为什么是 `Ready` 而不是 `NeedsMap`：本项**已实现**这件事必须能在 `--menu-dump`
        // 里读出来（验收口径：那一行要显示「已实现」）。没有地图时它**不灰显**，
        // 而是在 [`crate::menu::action::apply`] 里给一条带原因的真实回执
        // （「需要先开图：当前没有打开地图（文件 → 打开地图…）」）—— 比灰显更能说明原因，
        // 也不会出现"点了没反应"。
        // 「地图光照」父项是**子菜单容器**（官方 `MapLightingMenu` 无 `Click`）：
        // 2026-11-05 起三档真接线（`miredit_render::MapLighting` + `RenderOptions::lighting`
        // + 三张官方光罩 PNG），父项同样可点、不灰 —— 直接派发它（鼠标点击 / 回车 /
        // `--menu-click`）只展开子菜单，回执说清去哪选档，见
        // [`crate::menu::action::apply`] 的 `Action::MapLighting` 分支。
        //
        // 为什么是 `Ready` 而不是 `Todo`：渲染层早已就绪（125 测试全过），缺的只是
        // GUI 的生产调用点；这一轮把它接上了（`app.lighting` → `CanvasView::ensure`
        // 的缓存键 + `RenderOptions`），再灰显就是骗人。
        Action::MapLighting => State::Ready,
        // 三个复选子项：切档 ⇒ 改 `app.lighting` ⇒ 下一帧缓存键失效 ⇒ 真重渲
        // （回执中文「地图光照：夜晚」等，见 `ui_text::MENU_LIGHT_*`）。
        // 白天是缺省档（`MapLight::default()`），三档互斥由 [`check_with`] 读真状态保证。
        Action::LightDawnDusk | Action::LightDay | Action::LightNight => State::Ready,
        Action::SaveAs => State::Ready,
        // 「导出小地图」/「导出选区」2026-11-05 起**真实现**：`Overlay::ExportMinimap` /
        // `Overlay::ExportSelection` 路径输入模态 → `crate::export::export_minimap` /
        // `export_selection`（分块渲染 + 拼接 → `write_rgba_png` → 现有原子写）。
        //
        // 为什么是 `Ready` 而不是 `NeedsMap`：与「另存为」同一条理由 ——
        // **本项已实现**这件事必须能在 `--menu-dump` 里读出来（那一行打的是
        // [`State::text`]）；没有地图时它**不灰显**，而是在
        // [`crate::menu::action::apply`] 里给一条带原因的真实回执
        // （「需要先开图：当前没有打开地图（文件 → 打开地图…）」）。
        Action::ExportMinimap | Action::ExportSelection => State::Ready,
        // 「最近打开」2026-11-05 起**真实现**：子菜单的行来自设置文件里的 `recent`
        // （[`crate::menu::settings`]），点一条 ⇒ [`crate::menu::action::open_recent_index`]
        // ⇒ 走与「打开地图…」**完全相同**的 [`crate::menu::action::open_path`]。
        //
        // 为什么是 `Ready`：官方这一项是**容器**（`OpenRecentButton` 在 axaml 里没有
        // `Click`，`MainWindow.RecentMaps.cs:23-40` 只往 `Items` 里填行），它自己的
        // `IsEnabled` 跟着 `busy` 走（`axaml.cs` 的 `OpenRecentButton.IsEnabled = !busy`）
        // ⇒ 常态可点。本仓同样：可点、不灰，点了（或回车）只展开/收起子菜单，
        // **不执行任何父项动作**（回执见 [`STATUS_RECENT_CONTAINER`]）。
        Action::OpenRecent => State::Ready,
        // ── 未实现（灰显 + 原因） ──
        //
        // ⚠ `Action::OpenRecent` 2026-11-05 已接线（见上面 `State::Ready` 那一段）：
        // 它原来挂的那条灰显原因（「本仓只持久化深色偏好」）随之过期，2026-11-05 已连同
        // `ui_text::ALL` 里的条目一起删除（本条注释不再写出它的名字，免得 grep 搜到死常量）。
        Action::NewMap => State::Todo(tx::WHY_NEW_MAP),
        Action::NewStamp => State::Todo(tx::WHY_NEW_STAMP),
        Action::ImportStamp => State::Todo(tx::WHY_IMPORT_STAMP),
        Action::Exit => State::Todo(tx::WHY_EXIT),
        Action::Settings => State::Todo(tx::WHY_SETTINGS),
        Action::Language => State::Todo(tx::WHY_LANGUAGE),
        // 「语言」两子项：本仓文案只有中文一版 ⇒ 灰显 + **与父项同一条原因**
        // （不另编一句"子项未实现"，免得同一件事出现两种说法）。
        Action::LangChinese | Action::LangEnglish => State::Todo(tx::WHY_LANGUAGE),
        Action::LockLayout => State::Todo(tx::WHY_LOCK_LAYOUT),
        Action::ResetLayout => State::Todo(tx::WHY_RESET_LAYOUT),
        Action::GenTown => State::Todo(tx::WHY_GEN_TOWN),
        Action::GenTerrain => State::Todo(tx::WHY_GEN_TERRAIN),
        Action::GenProvince => State::Todo(tx::WHY_GEN_PROVINCE),
        Action::GenDungeon => State::Todo(tx::WHY_GEN_DUNGEON),
        Action::IdentifyObjects => State::Todo(tx::WHY_IDENTIFY_OBJECTS),
        Action::ExtractObjects => State::Todo(tx::WHY_EXTRACT_OBJECTS),
    }
}

/// **官方缺省勾选态**（`MainWindow.axaml:5` 的 `IsChecked=` 原文）；`None` = 不是勾选型。
///
/// 这是"官方开箱时勾着什么"，**不是**"本仓现在勾着什么"—— 后者见 [`check_with`]。
/// 两者只在"本仓没有可写真状态"的那几项上重合（见 [`SubState`] 的文档）。
///
/// 逐条出处：
///
/// | 项 | 官方 | 缺省 |
/// |---|---|---|
/// | 单元格网格 | `GridToggle … ToggleType="CheckBox" IsChecked="True"` | **勾** |
/// | 开启门 | `DoorsOpenToggle … ToggleType="CheckBox"`（**没写 IsChecked**）| 不勾 |
/// | 锁定面板布局 | `axaml.cs:391` `ToggleType=MenuItemToggleType.CheckBox`（没写 IsChecked）| 不勾 |
/// | 白天 | `DayLighting … ToggleType="CheckBox" IsChecked="True"` | **勾** |
/// | 黎明 / 黄昏 · 夜晚 | 同段两个 `CheckBox`，都没写 IsChecked | 不勾 |
/// | 简体中文 | `Localization.cs:34` `IsChecked = selected == "zh-CN"`（缺省 zh-CN）| **勾** |
/// | English | 同上（`selected == "en-US"`）| 不勾 |
/// | 深色模式 | `DarkModeToggle` 无 IsChecked（`axaml.cs:120` 取 `ThemeService.ForceDark`）| 不勾 |
/// | 7 个面板 | 官方 `AddSectionMenu` 逐项 `IsChecked = true` | 勾 |
pub const fn default_checked(action: Action) -> Option<bool> {
    match action {
        Action::CellGrid => Some(true),
        Action::OpenDoors => Some(false),
        Action::LockLayout => Some(false),
        Action::LightDawnDusk => Some(false),
        Action::LightDay => Some(true),
        Action::LightNight => Some(false),
        Action::LangChinese => Some(true),
        Action::LangEnglish => Some(false),
        Action::DarkMode => Some(false),
        Action::Panel(_) => Some(true),
        _ => None,
    }
}

/// 该项当前的**勾选态**（`None` = 不是勾选型菜单项）——[`crate::app`] 用的 4 参入口。
///
/// 等价于 `check_with(…, doors_open = false, lighting = Day, &SubState::default())`：
/// 「深色关 / 网格由参数给 / **门关** / **白天**」的官方缺省口径。
///
/// ⚠ 这个 4 参签名**不许改**：`app.rs` 的用例按它调（`menu::model::check(a, dark, grid, panels)`）。
/// 需要「开启门」/「地图光照」真状态时用 [`check_with`]（生产绘制路径走的就是它，
/// `doors` 来自 `app::MireditApp::doors_open`，`lighting` 来自 `app::MireditApp::lighting`）。
pub fn check(
    action: Action,
    dark: bool,
    grid: bool,
    panels: &crate::menu::panels::PanelVis,
) -> Option<bool> {
    check_with(
        action,
        dark,
        grid,
        false,
        MapLight::Day,
        panels,
        &SubState::default(),
    )
}

/// 该项当前的**勾选态**（带子菜单状态 + 四个真开关）—— 绘制与测试用的完整入口。
///
/// # 判据（唯一一处，绘制/测试都问它）
///
/// * **有真状态的**（深色模式 / 单元格网格 / 开启门 / 地图光照 / 8 个面板）：读宿主字段，
///   勾必须反映真状态 —— `grid` 传的是画布那一帧真的画没画格线；`doors` 传的是画布
///   那一帧**真的**用 `RenderOptions::doors_open` 渲的开关；`lighting` 传的是那一帧
///   **真的**用 `RenderOptions::lighting` 渲的档位（三者与
///   [`crate::canvas_view::CanvasView::ensure`] 的缓存键同一份）。
///   勾上了却没画线/没开门/没换档、或反过来，都是假信息。
/// * **本仓没有真状态的**（锁定面板布局 / 语言 2 项）：取 [`default_checked`] 或
///   [`SubState`] 里的档位 —— 官方缺省，**绝不编一个用户没设过的值**。
///
/// ⚠ `OpenDoors` 的历史教训：接线前它与 `LockLayout` 合并成 `default_checked` ⇒ 永远
/// `Some(false)`，是**假勾**。光照三档绝不重蹈覆辙：直接读 `lighting` 参数，
/// 不碰 `default_checked`、不碰 `SubState`。
pub fn check_with(
    action: Action,
    dark: bool,
    grid: bool,
    doors: bool,
    lighting: MapLight,
    panels: &crate::menu::panels::PanelVis,
    sub: &SubState,
) -> Option<bool> {
    match action {
        Action::DarkMode => Some(dark),
        Action::CellGrid => Some(grid),
        Action::Panel(id) => Some(panels.get(id)),
        // 开启门：**真状态**（`app.doors_open` ⇒ `CanvasView::ensure` 的渲染选项）。
        Action::OpenDoors => Some(doors),
        // 地图光照：**真状态**（`app.lighting` ⇒ `CanvasView::ensure` 的渲染选项）。
        // 三档互斥由「恰好一个枚举值与参数相等」结构性保证，不需要额外判据。
        Action::LightDawnDusk => Some(lighting == MapLight::DawnDusk),
        Action::LightDay => Some(lighting == MapLight::Day),
        Action::LightNight => Some(lighting == MapLight::Night),
        // 锁定面板布局：官方 `ToggleType="CheckBox"` 且没写 `IsChecked` ⇒ 勾选型 + 缺省不勾。
        // 本仓没有可锁的停靠系统（`State::Todo`）⇒ 没有真状态可读，取官方缺省。
        Action::LockLayout => default_checked(action),
        Action::LangChinese => Some(sub.language == Lang::ZhCn),
        Action::LangEnglish => Some(sub.language == Lang::En),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::panels::PanelVis;
    use std::collections::BTreeSet;

    /// **40 项的账**：文件 10 / 编辑 4 / 视图 19 / 工具 6 / 帮助 1。
    ///
    /// 这条是验收门 §四.2 的机器判据：任何人加/删一项，这里先红。
    #[test]
    fn 菜单项数与官方账一致() {
        assert_eq!(FILE_ITEMS.len(), 10, "文件菜单");
        assert_eq!(EDIT_ITEMS.len(), 4, "编辑菜单");
        assert_eq!(
            VIEW_ITEMS.len(),
            19,
            "视图菜单（XAML 8 + 语言 1 + 面板 8 + 锁定/重置 2）"
        );
        assert_eq!(TOOLS_ITEMS.len(), 6, "工具菜单");
        assert_eq!(HELP_ITEMS.len(), 1, "帮助菜单");
        assert_eq!(item_count(), 40, "5 个菜单合计");
        assert_eq!(MENUS.len(), 5);
        // 视图里的面板开关正好 8 个，且与 PanelId::ALL 一一对应
        let panels: Vec<_> = VIEW_ITEMS
            .iter()
            .filter_map(|i| match i.action {
                Action::Panel(p) => Some(p),
                _ => None,
            })
            .collect();
        assert_eq!(
            panels,
            PanelId::ALL.to_vec(),
            "视图里的面板开关必须正好 8 个且顺序一致"
        );
    }

    /// id 唯一、label 非空、`find_by_id` 往返。
    #[test]
    fn id唯一且可反查() {
        let mut ids = BTreeSet::new();
        for m in &MENUS {
            for item in m.items {
                let id = item.action.id();
                assert!(ids.insert(id), "菜单项 id 重复：{id}");
                assert!(!item.action.label().is_empty(), "{id} 的文案是空串");
                let (mi, ii, got) = find_by_id(id).expect("必须能按 id 反查");
                assert_eq!(got.action, item.action);
                assert_eq!(MENUS[mi].items[ii].action.id(), id);
            }
        }
        assert_eq!(ids.len(), 40);
        assert!(find_by_id("no.such_item").is_none());
    }

    /// **每项都有明确归属**：要么 `Ready`/`NeedsMap`（会真执行），要么 `Todo(原因非空)`。
    ///
    /// 这条挡的是"点了没反应也不说话"——`Todo` 必须带一句原因。
    #[test]
    fn 三态全覆盖且原因非空() {
        let mut ready = 0;
        let mut need_map = 0;
        let mut todo = 0;
        for m in &MENUS {
            for item in m.items {
                match state(item.action) {
                    State::Ready => ready += 1,
                    State::NeedsMap => need_map += 1,
                    State::Todo(why) => {
                        assert!(!why.is_empty(), "{} 的未实现原因是空串", item.action.id());
                        assert!(why.chars().count() >= 8, "{} 的原因太短：{why}", item.action.id());
                        todo += 1;
                    }
                }
            }
        }
        assert_eq!(ready + need_map + todo, 40);
        // 任务书 §三.3 点名必须真接上的那一批：逐条钉住，防止有人把它们改回 Todo
        for a in [
            Action::OpenMap,
            Action::SaveMap,
            Action::Undo,
            Action::Redo,
            Action::FillSelection,
            Action::DarkMode,
            // 2026-11-05 起真实现：画布按世界格 48×32 画格线（`crate::grid`）
            Action::CellGrid,
            // 2026-11-05 起真实现：`RenderOptions::doors_open` 接到生产路径
            // （`app.doors_open` → `CanvasView::ensure` 的缓存键 + 渲染选项）
            Action::OpenDoors,
            // 2026-11-05 起真实现：`Overlay::SaveAs` 路径输入模态 → `save::save_as_session`
            Action::SaveAs,
            // 2026-11-05 起真实现：`Overlay::ExportMinimap` / `Overlay::ExportSelection`
            // 路径输入模态 → `crate::export::export_minimap` / `export_selection`
            // （分块渲染 → `write_rgba_png` → 现有原子写；`--export-minimap` /
            //   `--export-selection` 是无窗同一入口）
            Action::ExportMinimap,
            Action::ExportSelection,
            // 2026-11-05 起真实现：子菜单行来自设置文件 `recent`，
            // 点一条 ⇒ `action::open_recent_index` ⇒ `open_path`（同一套打开逻辑）
            Action::OpenRecent,
            // 2026-11-05 起真实现：`miredit_render::MapLighting` 三档接到生产路径
            // （`app.lighting` → `CanvasView::ensure` 的缓存键 + `RenderOptions`）
            Action::MapLighting,
            Action::LightDawnDusk,
            Action::LightDay,
            Action::LightNight,
            Action::ZoomIn,
            Action::ZoomOut,
            Action::ActualSize,
            Action::FitMap,
            Action::KeyboardShortcuts,
        ] {
            assert!(
                matches!(state(a), State::Ready | State::NeedsMap),
                "{} 必须已实现",
                a.id()
            );
        }
        for p in PanelId::ALL {
            assert_eq!(state(Action::Panel(p)), State::Ready, "{:?} 面板开关必须已实现", p);
        }
        // 任务书 §三.4 点名"一律灰显"的那一批：逐条钉住
        // （「导出小地图」/「导出选区」已在本轮真实现，从这一批移出，见上面的已实现列表）
        for a in [
            Action::NewMap,
            Action::NewStamp,
            Action::ImportStamp,
            Action::Exit,
            Action::Settings,
            Action::Language,
            // 子项与父项同一条原因（本仓没有英文文案）
            Action::LangChinese,
            Action::LangEnglish,
            Action::LockLayout,
            Action::ResetLayout,
            Action::GenTown,
            Action::GenTerrain,
            Action::GenProvince,
            Action::GenDungeon,
            Action::IdentifyObjects,
            Action::ExtractObjects,
        ] {
            assert!(matches!(state(a), State::Todo(_)), "{} 必须灰显", a.id());
        }
    }

    /// `enabled` 的判据：`NeedsMap` 只在有图时可点；`Todo` 永远不可点。
    #[test]
    fn 无图时需开图的项被拦() {
        assert!(State::Ready.enabled(false));
        assert!(!State::NeedsMap.enabled(false));
        assert!(State::NeedsMap.enabled(true));
        assert!(!State::Todo("x").enabled(true));
        assert!(!State::Todo("x").enabled(false));
        // 状态串里必须带原因（灰显项点了要给用户看的就是这句）
        let s = State::Todo(tx::WHY_EXIT).text();
        assert!(s.contains(tx::WHY_EXIT), "状态串里丢了原因：{s}");
    }

    /// 快捷键只挂在官方写了 `InputGesture` 的那几项上（共 9 条）。
    #[test]
    fn 快捷键只来自官方手势() {
        let mut n = 0;
        for m in &MENUS {
            for item in m.items {
                if let Some(k) = item.action.shortcut() {
                    n += 1;
                    assert!(!k.is_empty());
                }
            }
        }
        assert_eq!(n, 9, "官方 InputGesture 共 9 条：Ctrl+N/O/S/Shift+S/Shift+N/Z/Shift+Z + G + F1");
        assert_eq!(Action::SaveMap.shortcut(), Some("Ctrl+S"));
        assert_eq!(Action::CellGrid.shortcut(), Some("G"));
        assert_eq!(Action::KeyboardShortcuts.shortcut(), Some("F1"));
        assert_eq!(Action::ZoomIn.shortcut(), None, "放大在官方没有手势");
        // 子项都没有官方手势（官方 3+2 个子项一个 `InputGesture` 都没写）
        for a in [
            Action::LightDawnDusk,
            Action::LightDay,
            Action::LightNight,
            Action::LangChinese,
            Action::LangEnglish,
        ] {
            assert_eq!(a.shortcut(), None, "{} 不该有手势", a.id());
        }
    }

    /// **「重做」菜单手势只显示 `Ctrl+Shift+Z`**（审查缺陷 3）。
    ///
    /// 官方 `MainWindow.axaml:5` 的 `InputGesture="Ctrl+Shift+Z"` 是菜单里唯一那个；
    /// `Ctrl+Y` 官方只在 `axaml.cs:848` 绑、**不写进菜单**。
    /// 本仓的 `Ctrl+Y` 是真绑定 ⇒ 它该出现在**帮助浮层**（`menu::shortcuts` 用
    /// `ui_text::KEY_CTRL_SHIFT_Z`），但**不该**出现在菜单右侧。
    #[test]
    fn 重做菜单手势只显示官方那一个() {
        let sc = Action::Redo.shortcut().expect("重做有官方手势");
        assert_eq!(sc, "Ctrl+Shift+Z");
        assert!(
            !sc.contains("Ctrl+Y"),
            "菜单里不许出现 / Ctrl+Y（官方菜单没写它）：{sc}"
        );
        assert_eq!(sc, KEY_REDO_MENU);
        // 浮层那条常量**仍然**含 Ctrl+Y（那是「本仓真实绑定」的口径，不许一起改掉）
        assert!(
            tx::KEY_CTRL_SHIFT_Z.contains("Ctrl+Y"),
            "帮助浮层的口径是「真实绑定」，必须保留 Ctrl+Y"
        );
    }

    /// **「另存为」已实现**（2026-11-05 接线）。
    ///
    /// 判据三条，全部对着验收口径：
    /// ① `state` 必须是 `Ready` —— `--menu-dump` 那一行打的就是 [`State::text`]，
    ///    验收直接 grep「已实现」；
    /// ② 无图时**仍可点**（`Ready.enabled(false) == true`）：点了会拿到一条带原因的回执，
    ///    而不是灰着不动（原因见 [`super::state`] 里 `SaveAs` 那一段的注释）；
    /// ③ 快捷键仍是官方手势 `Ctrl+Shift+S`，且这个键**真的绑上了**
    ///    （真绑定由 `menu::shortcuts` 的用例反向核对）。
    #[test]
    fn 另存为已实现且状态串是已实现() {
        assert_eq!(state(Action::SaveAs), State::Ready);
        assert_eq!(state(Action::SaveAs).text(), tx::STATE_READY);
        assert!(
            state(Action::SaveAs).enabled(false),
            "Ready 项在无图时也可点（点了给原因，不灰着不动）"
        );
        assert_eq!(Action::SaveAs.shortcut(), Some("Ctrl+Shift+S"));
        // 状态串里不许再出现「未实现」三个字（那是 2026-11-05 之前的样子）
        assert!(!state(Action::SaveAs).text().contains("未实现"));
    }

    /// 勾选态只对深色模式 / 单元格网格 / 7 个面板开关有值。
    #[test]
    fn 勾选态只给勾选型项() {
        let panels = PanelVis::default();
        assert_eq!(check(Action::DarkMode, true, false, &panels), Some(true));
        assert_eq!(check(Action::DarkMode, false, false, &panels), Some(false));
        assert_eq!(
            check(Action::Panel(PanelId::Palette), false, false, &panels),
            Some(true)
        );
        // 单元格网格：勾必须跟着**真状态**（`grid` 参数就是画布那一帧的开关）
        assert_eq!(
            check(Action::CellGrid, true, true, &panels),
            Some(true),
            "网格开着就必须画勾（画了线不画勾 = 假信息）"
        );
        assert_eq!(
            check(Action::CellGrid, true, false, &panels),
            Some(false),
            "网格关着就不许画勾"
        );
        assert_eq!(check(Action::Undo, true, false, &panels), None);
    }

    /// 分隔线的位置必须与官方 XAML 的 `<Separator />` 一致（逐条钉住）。
    #[test]
    fn 分隔线位置照官方() {
        let seps = |items: &[Item]| -> Vec<usize> {
            items
                .iter()
                .enumerate()
                .filter(|(_, i)| i.sep_before)
                .map(|(n, _)| n)
                .collect()
        };
        assert_eq!(seps(&FILE_ITEMS), vec![6, 9], "文件：导出小地图之后、退出之前");
        assert_eq!(seps(&EDIT_ITEMS), vec![2, 3], "编辑：撤销/重做之后、填充之后");
        // 2026-11-05：面板开关由 7 变 8（新增「地图列表」），分隔线**位置不变**
        // （仍是"面板组之前"一条），下标因此从 16 后移一位到 17。
        assert_eq!(
            seps(&VIEW_ITEMS),
            vec![1, 2, 4, 8, 9, 17],
            "视图：深色模式后 / 网格后 / 门后 / 适应地图后 / 语言后 / 面板后"
        );
        assert_eq!(seps(&TOOLS_ITEMS), vec![4], "工具：4 个生成器之后");
        assert_eq!(seps(&HELP_ITEMS), Vec::<usize>::new());
    }

    /// **子项文案的字形覆盖**（补 `fonts::tests::文案全部字符都有字形` 的缺口）。
    ///
    /// 为什么单列这一条：那 6 条子项文案 + 菜单专用的重做手势暂放本模块
    /// （`ui_text.rs` 不在本单写域），**进不了 `ui_text::ALL`** ⇒ 字形自测扫不到它们。
    /// 它们照样画在弹层上，缺字就是豆腐块（`DIAG_GRID` 那次红就是同款），
    /// 所以这里用**同一条 `fonts::load()` 路径**逐字查 `glyph_id`（0 = `.notdef`）。
    ///
    /// 等这 7 条搬进 `ui_text`（连同 `ALL`）之后，本用例可以删。
    #[test]
    fn 子项文案字形齐全() {
        use ab_glyph::Font as _;
        let f = crate::fonts::load().unwrap_or_else(|e| panic!("加载中文字体失败：{e}"));
        for (key, text) in [
            ("SUB_LIGHT_DAWN_DUSK", SUB_LIGHT_DAWN_DUSK),
            ("SUB_LIGHT_DAY", SUB_LIGHT_DAY),
            ("SUB_LIGHT_NIGHT", SUB_LIGHT_NIGHT),
            ("SUB_LANGUAGE_ZH_CN", SUB_LANGUAGE_ZH_CN),
            ("SUB_LANGUAGE_EN", SUB_LANGUAGE_EN),
            ("SUB_RECENT_EMPTY", SUB_RECENT_EMPTY),
            ("KEY_REDO_MENU", KEY_REDO_MENU),
            // 2026-11-05：最近地图那一批（状态栏回执 + stderr 诊断）
            ("STATE_RECENT_GONE", STATE_RECENT_GONE),
            ("STATUS_RECENT_GONE_TMPL", STATUS_RECENT_GONE_TMPL),
            ("STATUS_RECENT_CONTAINER", STATUS_RECENT_CONTAINER),
            ("DIAG_RECENT_REMEMBERED", DIAG_RECENT_REMEMBERED),
            ("DIAG_SETTINGS_WRITE_FAIL", DIAG_SETTINGS_WRITE_FAIL),
            ("DIAG_RECENT_GONE", DIAG_RECENT_GONE),
            ("DIAG_RECENT_UNREADABLE", DIAG_RECENT_UNREADABLE),
            ("STATUS_RECENT_STALE", STATUS_RECENT_STALE),
            ("ID_RECENT_ROW_PREFIX", ID_RECENT_ROW_PREFIX),
        ] {
            for c in text.chars() {
                if c == '\n' {
                    continue;
                }
                assert!(
                    f.font.glyph_id(c).0 != 0,
                    "{key} 缺字形 {c:?} U+{:04X}：{text}",
                    c as u32
                );
            }
        }
    }

    /// **40 顶级 + 5 子项**（审查缺陷 1/2 的计数钉子）。
    ///
    /// 两笔账分开数：
    /// * 顶级 40（[`item_count`] —— `--menu-dump` 汇总行的口径）；
    /// * 静态子项 5（[`sub_count`] = 地图光照 3 + 语言 2）。
    ///
    /// 再加「最近打开」那个**动态列表**：它没有静态子项，空列表时按官方
    /// `RecentMaps.cs:39` 给一条灰行 —— 这一条也在这里钉住。
    #[test]
    fn 顶级40项与子项5项计数() {
        // ① 顶级仍是 40（加子项不许把顶级撑大）
        assert_eq!(item_count(), 40);
        // ② 静态子项 5，且顺序照官方
        assert_eq!(sub_count(), 5, "地图光照 3 + 语言 2");
        assert_eq!(
            LIGHT_ITEMS.iter().map(|i| i.action.id()).collect::<Vec<_>>(),
            vec![
                "view.light_dawn_dusk",
                "view.light_day",
                "view.light_night"
            ],
            "地图光照子项顺序照官方 XAML（DawnDusk / Day / Night）"
        );
        assert_eq!(
            LANGUAGE_ITEMS
                .iter()
                .map(|i| i.action.id())
                .collect::<Vec<_>>(),
            vec!["view.language_zh_cn", "view.language_en"],
            "语言子项顺序照官方 Localization.cs:10-11"
        );
        // ③ 三个父项真是"可展开容器"，挂的是对的那份子项
        let (_, _, recent) = find_by_id("file.open_recent").expect("最近打开");
        assert!(
            matches!(recent.sub, Some(Sub::Recent)),
            "最近打开必须是动态列表（官方 RecentMaps.cs:27）"
        );
        assert!(sub_items(recent).is_empty(), "动态列表没有静态子项");
        let (_, _, light) = find_by_id("view.map_lighting").expect("地图光照");
        assert_eq!(sub_items(light).len(), 3);
        let (_, _, lang) = find_by_id("view.language").expect("语言");
        assert_eq!(sub_items(lang).len(), 2);
        // ④ 子项 id 唯一、可反查、文案非空、**不许与顶级 id 撞**
        let mut ids = BTreeSet::new();
        for m in &MENUS {
            for item in m.items {
                for sub in sub_items(item) {
                    let id = sub.action.id();
                    assert!(ids.insert(id), "子项 id 重复：{id}");
                    assert!(!sub.action.label().is_empty(), "{id} 的文案是空串");
                    let (mi, pi, si, got) = find_sub_by_id(id).expect("必须能按 id 反查子项");
                    assert_eq!(got.action, sub.action);
                    assert_eq!(MENUS[mi].items[pi].action.id(), item.action.id());
                    assert!(si < sub_items(item).len(), "{id} 的子项下标越界");
                    assert!(find_by_id(id).is_none(), "子项 id {id} 与顶级项撞了");
                }
            }
        }
        assert_eq!(ids.len(), 5);
        // ⑤ 最近打开的空态：`SubState::default()` 的列表是空的（官方缺省，不是用户数据）
        //    ⇒ 恒一条灰行。真列表由 `app` 每帧从设置文件喂进来（见下面的非空分支）。
        let st = SubState::default();
        assert!(st.recent.is_empty(), "缺省档位就是空列表（官方缺省）");
        let rows = st.recent_rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, SUB_RECENT_EMPTY);
        assert!(!rows[0].1, "官方空态行是 IsEnabled=false");
        // 非空时按官方 `Path.GetFileName` + `File.Exists`（用真文件验，不编假路径）
        let dir = std::env::temp_dir().join("miredit_gui_model_recent");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建临时目录");
        let p = dir.join("a.map");
        std::fs::write(&p, b"x").expect("写临时文件");
        let st = SubState {
            recent: vec![
                p.display().to_string(),
                dir.join("gone.map").display().to_string(),
            ],
            ..SubState::default()
        };
        let rows = st.recent_rows();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, "a.map", "官方取 Path.GetFileName");
        assert!(rows[0].1, "文件在 ⇒ 可点");
        assert!(!rows[1].1, "文件不在 ⇒ 灰显（官方 IsEnabled = File.Exists）");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **三个复选框的官方缺省勾选态**（审查缺陷 4 / 5）。
    ///
    /// * `单元格网格`：官方 `IsChecked="True"` ⇒ 缺省**勾**；
    /// * `开启门` / `锁定面板布局`：官方 `ToggleType="CheckBox"` 且**没写** IsChecked
    ///   ⇒ 勾选型 + 缺省不勾。
    ///
    /// 这条同时钉两件事：① 三者都**是勾选型**（`check` 返回 `Some`，不是 `None`，
    /// 否则弹层根本画不出勾）；② **官方缺省**值等于官方 `IsChecked=` 原文。
    ///
    /// ⚠ 2026-11-05 订正：`开启门` 的运行期勾选态**已不再取官方缺省** ——
    /// 它读 `app.doors_open`（见 [`check_with`]），本测试只钉 [`default_checked`]
    /// 那张"官方开箱"表；运行期真值由 `app::tests` 与 `canvas_view::tests` 各钉一处。
    #[test]
    fn 三个复选框的官方缺省勾选态() {
        assert_eq!(
            default_checked(Action::CellGrid),
            Some(true),
            "官方 GridToggle IsChecked=True"
        );
        assert_eq!(
            default_checked(Action::OpenDoors),
            Some(false),
            "官方 DoorsOpenToggle 没写 IsChecked"
        );
        assert_eq!(
            default_checked(Action::LockLayout),
            Some(false),
            "官方 lockLayout（axaml.cs:391）没写 IsChecked"
        );
        let panels = PanelVis::default();
        // 三者都必须被 `check` 认成勾选型（Some 才算"画得出勾"）
        // `check` 的 `doors` 恒为 false（4 参入口 = 闭门缺省）⇒ 这一行仍然是 Some(false)，
        // 但它证的是"勾选型"，不是"运行期真值"（后者见下面那条测试）
        assert_eq!(check(Action::OpenDoors, false, true, &panels), Some(false));
        assert_eq!(check(Action::LockLayout, false, true, &panels), Some(false));
        // 单元格网格的**运行期**勾选态取真状态（`grid` 参数 = 画布那一帧的开关）
        assert_eq!(check(Action::CellGrid, false, true, &panels), Some(true));
        assert_eq!(check(Action::CellGrid, false, false, &panels), Some(false));
        // 非勾选型仍是 None（"没有勾"与"不是勾选型"不许混成一种）
        assert_eq!(check(Action::Undo, false, true, &panels), None);
        assert_eq!(default_checked(Action::Undo), None);
    }

    /// **「开启门」的勾选态读真状态**（2026-11-05 接线后新增）。
    ///
    /// 判据：`doors = true` ⇒ 画勾；`doors = false` ⇒ 不画勾。
    /// 反例（接线前）：两项都取 `default_checked` ⇒ 永远 `Some(false)`，
    /// 用户勾上了却看不见勾 —— 那就是本测试要挡的假信息。
    #[test]
    fn 开启门勾选态读真状态() {
        let panels = PanelVis::default();
        let d = SubState::default();
        assert_eq!(
            check_with(Action::OpenDoors, false, true, true, MapLight::Day, &panels, &d),
            Some(true),
            "门开着必须画勾"
        );
        assert_eq!(
            check_with(Action::OpenDoors, false, true, false, MapLight::Day, &panels, &d),
            Some(false),
            "门关着不许画勾"
        );
        // 4 参入口（app 用例 / 无宿主状态处）恒按闭门算：不许把"不知道"说成"开着"
        assert_eq!(check(Action::OpenDoors, false, true, &panels), Some(false));
        // 其余两项不受 `doors` 影响（避免把新参数误接到别的分支上）
        assert_eq!(
            check_with(Action::LockLayout, false, true, true, MapLight::Day, &panels, &d),
            Some(false)
        );
        assert_eq!(
            check_with(Action::CellGrid, false, true, true, MapLight::Day, &panels, &d),
            Some(true)
        );
    }

    /// 子项的勾选态跟着**宿主真状态**走（光照读 `lighting` 参数、语言读 [`SubState`]；缺省 = 白天 / 简体中文）。
    #[test]
    fn 子项勾选态跟档位走() {
        let panels = PanelVis::default();
        let d = SubState::default();
        assert_eq!(d.language, Lang::ZhCn, "官方缺省 zh-CN");
        // 白天是 `MapLight::default()`（render crate 与 GUI 共用同一枚举）⇒ 缺省只勾白天
        assert_eq!(MapLight::default(), MapLight::Day, "官方 DayLighting IsChecked=True");
        assert_eq!(
            check_with(Action::LightDay, false, true, false, MapLight::Day, &panels, &d),
            Some(true)
        );
        assert_eq!(
            check_with(Action::LightDawnDusk, false, true, false, MapLight::Day, &panels, &d),
            Some(false)
        );
        assert_eq!(
            check_with(Action::LightNight, false, true, false, MapLight::Day, &panels, &d),
            Some(false)
        );
        assert_eq!(
            check_with(Action::LangChinese, false, true, false, MapLight::Day, &panels, &d),
            Some(true)
        );
        assert_eq!(
            check_with(Action::LangEnglish, false, true, false, MapLight::Day, &panels, &d),
            Some(false)
        );
        // 换档位 ⇒ 勾跟着走，且三档/两语**互斥**（同一时刻正好一个勾）。
        // ⚠ 这里传的是 `lighting` 参数（= 宿主真源 `app.lighting`），**不是** `SubState` 里的
        // 假档位 —— 这正是「OpenDoors 假勾」教训在光照上的落实。
        for light in [MapLight::DawnDusk, MapLight::Day, MapLight::Night] {
            let n = [Action::LightDawnDusk, Action::LightDay, Action::LightNight]
                .iter()
                .filter(|a| check_with(**a, false, true, false, light, &panels, &d) == Some(true))
                .count();
            assert_eq!(n, 1, "{light:?} 档位下必须正好一个勾");
        }
        for language in [Lang::ZhCn, Lang::En] {
            let s = SubState {
                language,
                ..SubState::default()
            };
            let n = [Action::LangChinese, Action::LangEnglish]
                .iter()
                .filter(|a| check_with(**a, false, true, false, MapLight::Day, &panels, &s) == Some(true))
                .count();
            assert_eq!(n, 1, "{} 下必须正好一个勾", language.code());
        }
        // 4 参 `check`（app 入口）取缺省档（白天）⇒ 与 `default_checked` 一致
        assert_eq!(
            check(Action::LightDay, false, true, &panels),
            default_checked(Action::LightDay)
        );
        assert_eq!(check(Action::LangChinese, false, true, &panels), Some(true));
        // 光照档位不影响门态/网格/语言的勾（避免把新参数误接到别的分支上）
        assert_eq!(
            check_with(Action::OpenDoors, false, true, true, MapLight::Night, &panels, &d),
            Some(true),
            "夜晚档下开门勾选态仍是真门态"
        );
        assert_eq!(
            check_with(Action::LangChinese, false, true, false, MapLight::Night, &panels, &d),
            Some(true),
            "夜晚档下语言勾选态不变"
        );
    }

    /// **「最近打开」子菜单的行数 / 动作 / 回查路径**（键盘导航与派发都问这三个）。
    ///
    /// 三件事一起钉：
    /// ① 行数必须与 `popup::sub_geom` 数的一致（空 ⇒ 1 行空态；非空 ⇒ `recent.len()`）；
    /// ② 空态行**没有动作**（回车/点击什么都不做），非空行有动作；
    /// ③ 动态行只画文件名，所以"点的是哪一条"必须能按下标回查完整路径。
    #[test]
    fn 最近地图子项行数与动作() {
        // ① 空列表
        let empty = SubState::default();
        assert_eq!(empty.sub_rows(Sub::Recent), 1, "空 ⇒ 只有空态行");
        assert_eq!(empty.sub_row_action(Sub::Recent, 0), None, "空态行没有动作");
        assert_eq!(empty.recent_at(0), None);
        assert_eq!(empty.sub_row_action(Sub::Recent, 5), None, "越界也要 None");

        // ② 两条
        let st = SubState {
            recent: vec![r"E:\m\41.map".to_owned(), r"E:\m\10.map".to_owned()],
            ..SubState::default()
        };
        assert_eq!(st.sub_rows(Sub::Recent), 2);
        for i in 0..2 {
            assert_eq!(
                st.sub_row_action(Sub::Recent, i),
                Some(Action::OpenRecent),
                "第 {i} 行必须有动作（点了就是打开它）"
            );
        }
        assert_eq!(st.recent_at(0), Some(r"E:\m\41.map"), "最近优先");
        assert_eq!(st.recent_at(1), Some(r"E:\m\10.map"));
        assert_eq!(st.recent_at(2), None, "越界");
        assert_eq!(st.sub_row_action(Sub::Recent, 2), None, "越界");

        // ③ 静态子项走同一套入口（行数 = 表长、动作 = 子项自己的）
        assert_eq!(st.sub_rows(Sub::Static(LIGHT_ITEMS)), 3);
        assert_eq!(
            st.sub_row_action(Sub::Static(LIGHT_ITEMS), 0),
            Some(Action::LightDawnDusk)
        );
        assert_eq!(st.sub_row_action(Sub::Static(LIGHT_ITEMS), 9), None);
        assert_eq!(st.sub_rows(Sub::Static(LANGUAGE_ITEMS)), 2);
    }

    /// **`file.open_recent` 已接线**：状态串必须是「已实现」（`--menu-dump` 那一行读它），
    /// 且**不许**再挂"本仓没有最近文件列表"那条过期原因。
    #[test]
    fn 最近打开已实现且不再是未实现() {
        assert_eq!(state(Action::OpenRecent), State::Ready);
        assert_eq!(state(Action::OpenRecent).text(), tx::STATE_READY);
        assert!(!state(Action::OpenRecent).text().contains("未实现"));
        assert!(
            state(Action::OpenRecent).enabled(false),
            "容器项在无图时也可点（点了只展开子菜单）"
        );
        // 它仍然是"可展开容器"（官方 OpenRecentButton 没有 Click）
        let (_, _, item) = find_by_id("file.open_recent").expect("最近打开");
        assert!(matches!(item.sub, Some(Sub::Recent)));
    }
}
