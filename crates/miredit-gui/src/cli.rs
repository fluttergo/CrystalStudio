//! 命令行参数解析（**不引第三方 CLI 库**：本单只需要十几个开关，手写百来行足够）。
//!
//! 支持的开关见 [`crate::ui_text::DIAG_USAGE`]。设计原则：
//! 1. **解析失败不静默**：无法识别的参数往 `stderr` 打印一条 `[参数] 无法解析：…`
//!    并打印用法，然后**继续以默认值启动**（GUI 不该因为一个拼错的开关就打不开）；
//! 2. 数值解析失败同样报出来并把该开关当没给（不猜一个"差不多"的值）；
//! 3. 所有开关都是**可选**的：一个参数都不给时行为见 [`Cli::default`]。
//!
//! # 编辑开关（本单新增）
//!
//! `--edit-pick/--edit-brush/--edit-erase/--edit-undo/--edit-redo/--edit-check` 是
//! **可重复、按出现顺序执行**的编辑脚本，`--save` 在脚本之后原子写回，`--no-window`
//! 表示只跑脚本不开窗（`--save` / `--save-as` 都隐含它）。为什么要这套开关：
//! ① 关掉窗口以后无法用鼠标做**可复现**的验收（截图之外还要逐字节比对）；
//! ② 这套开关走的是与鼠标**完全相同的** [`crate::edit::EditSession`] 与
//!    [`crate::save::save_atomic`]，不是另写一套；GUI 模式下同一份脚本也会在开窗前执行
//!    （截图里的"改动痕迹"就是这么来的）。
//!
//! `--save-as <路径>`（2026-11-05）是「文件 → 另存为…」/ `Ctrl+Shift+S` 的无窗版：
//! 调的是模态回车时调的同一个 [`crate::save::save_as_session`]（写新路径 + 改认新路径）。

use std::path::PathBuf;

use crate::edit::EditCmd;

/// 解析后的命令行开关。
#[derive(Default, Clone, Debug)]
pub struct Cli {
    /// `--map <路径>`：打开哪张 `.map`（不给则用 [`crate::mapdoc::DEFAULT_MAP_PATH`]，
    /// 那个常量**恒为空** ⇒ 启动时「未指定地图」，不猜任何路径）。
    pub map: Option<PathBuf>,
    /// `--center <x,y>`：把视口中心对准这个世界格（浮点允许，便于精确对位）。
    pub center: Option<(f32, f32)>,
    /// `--scale <倍数>`：初始缩放（0.25..=4.0，越界会被夹住）。
    pub scale: Option<f32>,
    /// `--layers ground,middle,front`：初始图层开关（给谁开谁；没列出的即关）。
    pub layers: Option<[bool; 3]>,
    /// `--collision low|high|all|none`：初始**阻挡标记通道**（低位/高位阻挡画不画）。
    ///
    /// 与 `--layers` 是**两件事**：`--layers` 管三层精灵（`LayerMask`），
    /// `--collision` 管阻挡标记通道（`miredit_render::CollisionOverlay`）。
    /// 缺省两个通道都关（= 与上一版逐字节一致）。为什么要这个开关：
    /// 截图取证时鼠标输入被锁死，点不了眼睛，只能靠它把通道打开。
    pub collision: Option<miredit_render::CollisionOverlay>,
    /// `--layer ground|middle|front|low_collision|high_collision`：初始**当前编辑层**（缺省 = 地表层）。
    ///
    /// 与 `--layers` 是**两件事**：`--layers` 管"画不画"（可见性），`--layer` 管"写到哪一层"。
    /// 名字只差一个 `s`，所以两边的注释都点名写清 —— 取证时最怕把它们搞混。
    /// 取值与 [`crate::edit::EditLayer::key`] 一一对应（有单元测试钉住往返）。
    pub layer: Option<crate::edit::EditLayer>,
    /// `--tool <0..9>`：初始选中的工具图标下标（0 = 画笔、2 = 填充、3 = 橡皮、6 = 吸管）。
    pub tool: Option<usize>,
    /// `--brush-size <3..31 奇数>`：初始**笔刷尺寸**（越界/偶数会被夹成合法值，见
    /// [`crate::metrics::clamp_brush_size`]）。
    ///
    /// 为什么要有它：`--shot` 运行锁死鼠标输入，改不了尺寸；而"3 / 7 / 15 三档落笔各改几格"
    /// 必须能无人值守复现。走的是与 `[` / `]` 键**同一个** `EditSession::set_brush_size`。
    pub brush_size: Option<usize>,
    /// `--edit-*`：按出现顺序执行的编辑脚本（见模块文档）。
    pub edits: Vec<EditCmd>,
    /// `--save`：脚本跑完后**原子写回**当前路径（没有修改就不写）。
    pub save: bool,
    /// `--save-as <路径>`：脚本跑完后**另存为**到该路径（2026-11-05）。
    ///
    /// 与 `--save` 是**两件事**：`--save` 写回文档自己的路径；`--save-as` 写到一个新路径，
    /// 成功后再把文档路径改成它（标题/状态栏跟着变）。两者可以同时给（先存原路径再另存）。
    ///
    /// 为什么要有它：菜单项「文件 → 另存为…」与 `Ctrl+Shift+S` 都要先弹模态敲路径，
    /// 无人值守运行里敲不了字；这个开关调的就是模态回车时调的
    /// [`crate::save::save_as_session`]（不是另写一条写盘捷径），
    /// 于是"另存为真的能写、真的改认新路径"可以**无人值守取证**。
    pub save_as: Option<PathBuf>,
    /// `--no-window`：只跑脚本/保存，不开窗。
    pub no_window: bool,
    /// `--shot <png>`：启动后自动截图到该路径。
    pub shot: Option<PathBuf>,
    /// `--frames <N>`：第 N 帧请求截图（默认 30，给字体/纹理留出装配时间）。
    pub frames: Option<u64>,
    /// `--gate2-client <png>`：与这张客户端截图做同视点分块比对。
    pub gate2_client: Option<PathBuf>,
    /// `--gate2-at <x,y>`：客户端截图的世界格中心（与出图命令里的 `--at=` 一致）。
    pub gate2_at: Option<(i32, i32)>,
    /// `--autodrag <秒>`：自动平移这么多秒，然后打印「调用次数/帧数/平均帧时间」并退出。
    pub autodrag: Option<f32>,
    /// `--hover <x,y>`：伪造鼠标悬停在该格（截图取证用，不影响真实鼠标）。
    pub hover: Option<(u16, u16)>,
    /// `--probe <x,y>`：**不开窗**，只打印该格的单元格信息（与面板同一套取数）。
    pub probe: Option<(u16, u16)>,
    /// `--dump-render <png>`：**不开窗**，把 `--dump-origin/--dump-cells` 指定的窗口用
    /// `miredit_render::render_region` 渲出来存盘（用于把「渲染库的锅」和「界面的锅」分开）。
    pub dump_render: Option<PathBuf>,
    /// `--dump-origin <x,y>`：上面那个窗口的世界格原点（默认 0,0）。
    pub dump_origin: Option<(i32, i32)>,
    /// `--dump-cells <w,h>`：上面那个窗口的格数（默认 64,48）。
    pub dump_cells: Option<(u32, u32)>,
    /// `--palette-lib <基名>`：启动时把素材面板切到该库（如 `Tiles5`、`Objects21`）。
    pub palette_lib: Option<String>,
    /// `--palette-scroll <行>`：启动时把网格滚到该行（虚拟滚动的验收用）。
    pub palette_scroll: Option<usize>,
    /// `--palette-search <文字>`：启动时往搜索框里填这段文字并回车（走真实搜索路径）。
    pub palette_search: Option<String>,
    /// `--palette-tab <0..3>`：启动时选中第几个页签（1..3 会显示"未实现"）。
    pub palette_tab: Option<usize>,
    /// `--palette-autoscroll <秒>`：自动往下滚这么多秒，然后打印「帧数/平均帧时间/解码次数」。
    pub palette_autoscroll: Option<f32>,
    /// `--palette-report`：截图/退出前把素材面板的统计打到 stderr。
    pub palette_report: bool,
    /// `--palette-pick <帧索引>`：启动时把当前库的该帧**设为笔刷**（走与鼠标点选相同的路径）。
    ///
    /// ⚠ **这个参数收的是 0 基帧索引**（= `palette::FrameKey::index`），不是界面上那个
    /// 「图号」：`图号 = 帧索引 + 1`（审计 E1 的全仓口径）。两者差 1，所以注释里必须点名，
    /// 否则取证时最容易在这里错一格 —— 落格后 `bk_img = 帧索引 + 1`，
    /// 诊断行打印的是**图号**（`DIAG_PALETTE_PICK`），照它对账即可。
    ///
    /// 为什么需要它：点选 → 落格 → 保存这条闭环必须能**无人值守复现**，
    /// 而 `--shot` 运行会锁住鼠标输入。这里调用的就是点击处理里的同一个
    /// [`crate::palette::PaletteState::pick`]，不是另写一条捷径。
    pub palette_pick: Option<u32>,
    /// `--palette-combo`：启动时把库下拉**展开**（截图取证用；展开后网格被弹层压住一部分）。
    pub palette_combo: bool,
    /// `--stamp-kind <all|unclassified|unlisted|英文 kind>`：「分类」页签的分类过滤。
    ///
    /// 英文 kind 直接取 `palette.json` 的 `type` 原值（`Terrain` / `Cliff` …），
    /// 与 `mir2_stamp::StampCatalog::by_kind` 同一个键，**不做中文反查**
    /// （中文名是对照表的显示层，不是查询键）。
    pub stamp_kind: Option<String>,
    /// `--stamp-search <文字>`：「分类」页签搜索框的初始内容（子串、大小写不敏感）。
    pub stamp_search: Option<String>,
    /// `--stamp-page <1 基页码>`：「分类」页签翻到第几页（越界夹到最后一页）。
    pub stamp_page: Option<usize>,
    /// `--stamp-pick <0 基格号>`：选中**当前页**第几格（4 列，所以 0..=95）。
    pub stamp_pick: Option<usize>,
    /// `--stamp-report`：截图/退出前把印章面板的统计（缓存峰值/命中/缺帧数）打到 stderr。
    pub stamp_report: bool,
    /// `--stamp-probe`：**不开窗**，跑印章目录对账 + 缩略图渲染对照 + LRU 压测（验收门 §四.2~4）。
    pub stamp_probe: bool,
    /// `--stamp-sweep`：配合 `--stamp-probe`，**逐枚渲染全库 9,159 枚**并清点四类结果。
    ///
    /// 为什么单列一个开关：这一遍要读 9,159 个文件 + 渲染（release 十几秒），
    /// 不该拖慢日常探针；但验收问「实际渲染了多少枚、失败多少」时必须真跑。
    pub stamp_sweep: bool,
    /// `--stamp-arm`：印章目录装载完成后，把**当前选中**的那一枚置为待放置态
    /// （等价于按 `Enter`，见 [`crate::app::MireditApp`] 的 `handle_arm_keys`）。
    ///
    /// 为什么需要它：`--shot` 运行会锁死鼠标/键盘输入，`Enter` 按不了，
    /// 而"待放置态长什么样"必须能无人值守复现。
    pub stamp_arm: bool,
    /// `--stamp-place-at <x,y>`：装载完成后先 `--stamp-pick` 选中的那一枚置为待放置态，
    /// 再在 `(x,y)` 落位（走与鼠标点击**完全相同**的 [`crate::stamp_place::place`]）。
    ///
    /// 截图验收要的"放置后 + 状态栏回执"就是靠它复现的；它**隐含** `--stamp-arm`。
    pub stamp_place_at: Option<(u16, u16)>,
    /// `--select <x0,y0,x1,y1>`：启动时设一个矩形选区（闭区间，越界由内核裁剪）。
    ///
    /// 走的是与鼠标拖动**完全相同**的 `EditSession::set_selection`，
    /// 不是另写一条"直接赋值"的捷径（那会绕过裁剪，选区可能整块在图外）。
    pub select: Option<(u16, u16, u16, u16)>,
    /// `--select-copy`：把 `--select` 设的选区复制进剪贴板（截图里要显示「剪贴板 W×H」）。
    pub select_copy: bool,
    /// `--dark`：启动即深色（覆盖设置文件里的选择）。
    ///
    /// 与 `--theme` 二选一；两个都给时以 `--theme` 为准（后者更明确）。
    pub dark: bool,
    /// `--theme <light|dark>`：启动主题（覆盖设置文件）。
    pub theme: Option<bool>,
    /// `--cell-grid`：**显式打开**单元格网格（视图 → 单元格网格，官方手势 `G`）。
    ///
    /// 为什么要有它：`--shot` 运行会锁死鼠标与键盘（见 `app::input_locked`），
    /// 菜单点不到、`G` 也按不了，而"网格开/关两张图做像素差"必须能无人值守复现。
    ///
    /// ⚠ 它置的是 [`crate::app::MireditApp::grid`] 的**初值**（与菜单翻转同一个字段），
    /// 不是另写一条"只影响绘制"的旁路；也**不产生回执**（回执会改状态栏文字，
    /// 那样开/关两张图的像素差就不止落在格线上了 —— 见验收口径）。
    ///
    /// ⚠ 2026-11-05 起网格**默认开**（官方 `MainWindow.axaml:5` 的 `IsChecked="True"`）
    /// ⇒ 本开关退化成"幂等的显式开"，**关的手段是 [`Cli::no_cell_grid`]**。
    /// 两者都判在 [`Cli::cell_grid_enabled`] 里，别在别处各写一套优先级。
    pub cell_grid: bool,
    /// `--no-cell-grid`：**显式关**单元格网格（要"无网格基线"截图时用）。
    ///
    /// 优先级：`--no-cell-grid` > `--cell-grid` > 默认 `true`（见 [`Cli::cell_grid_enabled`]）。
    /// 为什么"关"优先：默认已经是开，写 `--no-cell-grid` 的人一定是想要关；
    /// 两个都给属于自相矛盾，取"更明确的那一步"并让结果可预期（关）。
    pub no_cell_grid: bool,
    /// `--open-doors`：启动时**开启门**（视图 → 开启门，前景层按门开帧偏移渲染）。
    ///
    /// 它置的是 [`crate::app::MireditApp::doors_open`] 的**初值**（与菜单翻转同一个字段），
    /// 不是另写一条"只影响渲染"的旁路；也**不产生回执**（回执会改状态栏文字，
    /// 那样开/关两张图的像素差就不止落在门帧上 —— 见验收口径）。
    ///
    /// 为什么需要它：`--shot` / `--dump-render` 运行会锁死输入，菜单点不到，
    /// 而"门开/门关两张图做像素差"必须能无人值守复现（渲染层实测：`0.map` 的
    /// `(106,316)` 起 3×3 窗口有 4662/13824 个像素不同）。
    ///
    /// 缺省 **`false`**（= 闭门，与改动前逐字节相同；判据在
    /// `miredit_render::RenderOptions::default()`）。
    pub open_doors: bool,
    /// `--lighting <dawn_dusk|day|night>`：**地图光照档位**（视图 → 地图光照三档）。
    ///
    /// 三条纪律：
    /// 1. 类型是 render crate 的同一个 [`miredit_render::MapLighting`] 枚举，不另造一份；
    /// 2. 缺省 [`miredit_render::MapLighting::Day`] ⇒ 不画遮罩，与接线前**逐字节相同**
    ///    （`0.map` 64×48 的 FNV-1a64 仍是 `a924aa9dedc74183`）；
    /// 3. 它同时喂给窗口（`app.lighting` 的初值 → 画布缓存键 + `RenderOptions`）
    ///    与**所有离屏取证路径**（`--dump-render` / `--export-selection` / `--export-minimap`）
    ///    —— 判据只有一处 [`Cli::export_options`]。
    ///
    /// 取值用下划线 `dawn_dusk`（与任务书一致），另兼容 render crate 的短名
    /// `dawn-dusk` / `dawndusk`（见 [`parse_lighting`]）；其它写法一律报参数错误并回落白天。
    pub lighting: miredit_render::MapLighting,
    /// `--menu-open <0..4>`：启动即展开第几个菜单（截图取证用；`0` = 文件）。
    pub menu_open: Option<usize>,
    /// `--menu-hover <菜单内下标>`：启动即把当前展开菜单里的第几项设为**高亮项**。
    ///
    /// 为什么需要它：子菜单（地图光照 / 语言 / 最近打开）只在某项成为高亮项时才展开，
    /// 而高亮来自鼠标 hover 或 ↑↓ —— `--shot` 会锁死输入，两样都没有
    /// ⇒ 没有这个开关，子面板在截图里永远拍不到。
    ///
    /// ⚠ **只在同时给了 `--shot` 或 `--menu-open` 时生效**（避免影响正常交互），
    /// 判据在 `app::MireditApp::new`。越界/没展开菜单时打一条诊断并忽略。
    pub menu_hover: Option<usize>,
    /// `--menu-sub-focus <父项>,<行>`：启动即把**键盘焦点**送进子菜单并停在第 `<行>` 行。
    ///
    /// 为什么需要它：`--menu-hover` 只设"高亮项"（`active`），**不设** `sub_focus`
    /// ⇒ 子面板会展开，但子行高亮（2026-11-05 新增的绘制）在截图里永远拍不到。
    /// 而 `--shot` 锁死键盘输入（`app::input_locked`），命令行没法真按键。
    ///
    /// ⚠ 实现上**走真键盘路径**：`menu::handle_key_with` 的 `→`（进子菜单）+ N 次 `↓`
    /// ⇒ 截图证的是"用户按 → 再按 ↓ 之后屏幕上长什么样"，不是直接写私有字段。
    /// ⚠ **只在同时给了 `--shot` 与 `--menu-open` 时生效**（判据在 `app::MireditApp::new`）；
    /// 没展开菜单 / 那一项没有子菜单 / 行号越界 ⇒ 打一条诊断并忽略。
    pub menu_sub_focus: Option<(usize, usize)>,
    /// `--menu-click <id[@帧号]>`：按顺序执行菜单动作（`file.save_map` / `view.zoom_in` …）。
    ///
    /// 走的是与鼠标点击**完全相同**的 [`crate::menu::action::apply`]，
    /// 所以"命令行能跑"= "鼠标点得动"。
    ///
    /// **可选 `@<帧号>` 后缀**（2026-11-05 加，缺省 = 第 1 帧）：把这一条推迟到第 N 帧执行。
    /// 为什么需要：验收「同一会话里切换开关 ⇒ 画面重新渲染」要求**先渲一帧、再切、再渲**；
    /// 全部在第 1 帧执行时，切换发生在首帧重渲**之前**，观察不到缓存键失效。
    /// 语法见 [`parse_menu_click`]（`view.open_doors@40` ⇒ 第 40 帧点「开启门」）。
    pub menu_clicks: Vec<String>,
    /// `--menu-dump`：**不开窗**，把 5 菜单 39 项逐条打到 stderr 后退出。
    pub menu_dump: bool,
    /// `--export-selection <路径>`：**不开窗**把当前选区按 1:1 渲成 PNG 写到该路径。
    ///
    /// 与「文件 → 导出选区(E)…」模态回车走的是**同一个**
    /// [`crate::export::export_selection`]（选区取自 `--select`，与画布上那个金色框
    /// 同一份数据）⇒ "命令行能跑" = "菜单点得动"。
    ///
    /// 为什么需要它：无人值守取证要能一条命令复现「选区 → PNG」，
    /// 而模态里的路径输入框没法在脚本里敲字。
    pub export_selection: Option<PathBuf>,
    /// `--export-minimap <路径>`：**不开窗**把整张地图渲成 PNG。
    ///
    /// @acfbbf8a 回源订正后，缺省走官方尺寸链 [`crate::export::export_minimap_sized`]
    /// （`W*12 × H*8`）；给了 `--minimap-preset` / `--minimap-width`+`--minimap-height`
    /// 就按官方预设 / 指定像素出图。大图自动分块，分块护栏原样保留。
    /// **只有**显式给了旧开关 `--minimap-max-side` 时，才回退到本仓兼容链
    /// [`crate::export::export_minimap`]（2048 长边两段式）。
    pub export_minimap: Option<PathBuf>,
    /// `--export-doors`：**导出时按「门全开」出图**（前景层帧号 `+= door_offset & 0x7F`）。
    ///
    /// 缺省**跟当前门态**（判据只有一处：[`Cli::export_doors_open`]）——
    /// 也就是"画布上看到的"与"导出的"永远一致，这正是本开关存在的理由：
    /// 收口「画布开着门、导出是闭门」那个不一致。
    ///
    /// 为什么还要单独一个开关：`--export-*` 是无人值守入口，取一张"开门态 PNG"做取证时
    /// 不该被要求连带把窗口/菜单状态也改掉；显式传它 = 明确要开门态。
    ///
    /// 为什么需要它（实测判据）：`0.map` 的 `(106,316)` 起 3×3 窗口，
    /// 闭门导出 `32,842 B / sha256 e1acd638…4335`，开门导出
    /// `32,812 B / sha256 3e59dd73…9b8d`（与 `miredit-web` 的 `?doors=1` 同一份字节）。
    pub export_doors: bool,
    /// `--minimap-max-side <像素>`：小地图导出的**本仓兼容链目标长边**。
    ///
    /// @acfbbf8a 回源订正后，官方缺省已改为 `W*12 × H*8`（见
    /// [`crate::export::MINIMAP_MAX_SIDE`] 的官方四要素注释）。这里**只有显式给了该开关**
    /// 才走旧的 2048 长边两段式；下限 [`crate::export::MINIMAP_MAX_SIDE_MIN`] 只保护
    /// 该兼容链的 `fit_ratio` 整数比，不是官方尺寸判据。
    pub minimap_max_side: Option<u32>,
    /// `--minimap-width <像素>` / `--minimap-height <像素>`：官方尺寸面板的宽 / 高。
    /// 两者必须同时给（与 `MinimapExportWindow.OutputWidth/OutputHeight` 同源）；
    /// 合法性由 [`crate::export::valid_minimap_size`] 在导出前统一裁决。
    pub minimap_width: Option<u32>,
    pub minimap_height: Option<u32>,
    /// `--minimap-preset <small|medium|large>`：官方三档尺寸预设（除数 4 / 2 / 1）。
    pub minimap_preset: Option<crate::export::MinimapPreset>,
    /// `--open-map <路径>`：启动后走**「文件 → 打开地图…」的同一条路径**
    /// （[`crate::menu::action::open_path`]）换成这张图。
    ///
    /// 为什么需要它：模态里的路径输入框没法在无人值守运行里敲字，
    /// 而"打开地图真的能换图"必须能复现 —— 这个开关调的就是模态点「打开」时调的那个函数。
    pub open_map: Option<PathBuf>,
    /// `--shortcuts`：启动即打开「键盘快捷键」浮层（截图取证用）。
    pub shortcuts: bool,
    /// `--panel-hide <key>`：启动即隐藏某面板（`tools` / `palette` / `cell_inspector` …）。
    pub panel_hide: Vec<String>,
    /// `--map-list-root <目录>`：**地图列表面板的扫描根**（2026-11-05，上游 `901dbe2f` 移植）。
    ///
    /// 不给时按"当前地图所在目录"自动取（见 [`crate::app::maps_root`]）。
    /// 显式给了就**钉住**：之后从列表/模态打开别的目录的图，列表不会跟着换根
    /// （验收要能对着固定目录复跑）。
    pub map_list_root: Option<PathBuf>,
    /// `--show-maps`：启动即**展开地图列表面板**（它默认关，理由见
    /// [`crate::menu::panels::PanelId::Maps`]；截图取证需要它开着）。
    pub show_maps: bool,
    /// `--settings <路径>`：覆盖设置文件路径（验收时不去动真实的 `%LOCALAPPDATA%`）。
    pub settings: Option<PathBuf>,
    /// `--help`：打印用法后退出。
    pub help: bool,
    /// 解析期遇到的错误（已原样打印到 stderr，这里留档便于测试断言）。
    pub errors: Vec<String>,
}

impl Cli {
    /// 是否走「不开窗」的脚本模式（`--save` / `--save-as` 都隐含它：存完文件就该退出）。
    ///
    /// 两个导出开关也隐含它：`--export-selection` / `--export-minimap` 是无人值守取证入口，
    /// 写完 PNG 就该退出（否则脚本会卡在一个永远不关的窗口上）。
    pub fn headless(&self) -> bool {
        self.no_window
            || self.save
            || self.save_as.is_some()
            || self.export_selection.is_some()
            || self.export_minimap.is_some()
    }

    /// **单元格网格的初值**（`app::MireditApp::grid` 的唯一来源）。
    ///
    /// 三档优先级（唯一判据，别处不许再写一套）：
    ///
    /// ```text
    /// --no-cell-grid  ⇒ false        （"关"最明确，两个都给时它赢）
    /// --cell-grid     ⇒ true         （显式开；默认已开 ⇒ 幂等）
    /// 默认            ⇒ true         （官方 MainWindow.axaml:5 的 IsChecked="True"）
    /// ```
    pub fn cell_grid_enabled(&self) -> bool {
        // 三档优先级里只有"关"能翻盘：`--cell-grid` 与默认同值（都是开），
        // 所以判据就是"没传 --no-cell-grid"。`--cell-grid` 那个字段仍然要解析出来
        // （它表示"用户显式要求开"，`--help` 里也列着），只是对结果幂等。
        !self.no_cell_grid
    }

    /// **导出用的门态**（`--export-selection` / `--export-minimap` 的唯一判据）。
    ///
    /// ```text
    /// --export-doors  ⇒ true   （显式要开门态：取证用，与画布状态无关）
    /// --open-doors    ⇒ true   （当前门态就是开 —— 无窗模式下它就是 app.doors_open 的初值）
    /// 默认            ⇒ false  （闭门 = RenderOptions::default()，与接线前逐字节相同）
    /// ```
    ///
    /// 为什么"缺省跟当前门态"：`--export-*` 是无人值守入口，没有窗口、也没有菜单，
    /// 但它导的必须是**用户在同一张图上看到的那副样子**。所以缺省取 `--open-doors`
    /// （= 宿主字段 `app::MireditApp::doors_open` 的初值，见 `app.rs`），
    /// 而不是硬写 `false` —— 硬写就是「画布开着门、导出是闭门」那个不一致的翻版。
    pub fn export_doors_open(&self) -> bool {
        self.export_doors || self.open_doors
    }

    /// **导出/转储用的整份渲染选项**（`--dump-render` / `--export-selection` /
    /// `--export-minimap` 的唯一判据，2026-11-05 地图光照接线时补）。
    ///
    /// 把门态与光照档位打包成**同一个** [`miredit_render::RenderOptions`]：
    /// 两个 option 一起生效，不许出现"切了夜晚却丢了开门"或"开了门却是白天"的两副样子。
    ///
    /// ```text
    /// --export-doors  ⇒ doors_open = true  （显式要开门态，与画布状态无关）
    /// --open-doors    ⇒ doors_open = true  （当前门态就是开）
    /// --lighting X    ⇒ lighting = X      （缺省 Day = 不画遮罩）
    /// 默认            ⇒ RenderOptions::default() = 闭门 + 白天（与接线前逐字节相同）
    /// ```
    pub fn export_options(&self) -> miredit_render::RenderOptions {
        miredit_render::RenderOptions::with_doors_and_lighting(
            self.export_doors_open(),
            self.lighting,
        )
    }

    /// **小地图尺寸口径**（CLI 三条新开关 + 旧 `--minimap-max-side` 的唯一判据）。
    ///
    /// ```text
    /// --minimap-preset P              ⇒ Preset(P)（官方三档，优先级高于显式宽高）
    /// --minimap-width W + --height H ⇒ Explicit { W, H }（官方手填尺寸）
    /// 都没给                          ⇒ Default（W*12 × H*8，官方缺省）
    /// ```
    ///
    /// 显式给了 `--minimap-max-side` ⇒ 由调用方走**兼容链** [`crate::export::export_minimap`]，
    /// 这里返回的 `MinimapSize` 不参与；兼容链与官方链在 `editrun` 里互斥，不许混用。
    /// 只给宽 / 只给高 ⇒ `Err(中文原因)`，不猜另一个数。
    pub fn minimap_size(&self) -> Result<crate::export::MinimapSize, String> {
        if let Some(p) = self.minimap_preset {
            if self.minimap_width.is_some() || self.minimap_height.is_some() {
                return Err("小地图尺寸冲突：--minimap-preset 与 --minimap-width/--minimap-height 不能同时给".to_owned());
            }
            return Ok(crate::export::MinimapSize::Preset(p));
        }
        match (self.minimap_width, self.minimap_height) {
            (Some(width), Some(height)) => Ok(crate::export::MinimapSize::Explicit { width, height }),
            (None, None) => Ok(crate::export::MinimapSize::Default),
            _ => Err("小地图尺寸不完整：--minimap-width 与 --minimap-height 必须同时给（官方尺寸面板两个输入框都必填）".to_owned()),
        }
    }
}

impl Cli {
    /// 解析 `std::env::args().skip(1)`。
    pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Self {
        let mut c = Cli::default();
        let mut it = args.into_iter();
        while let Some(a) = it.next() {
            match a.as_str() {
                "--help" | "-h" => c.help = true,
                "--map" => match it.next() {
                    Some(v) => c.map = Some(PathBuf::from(v)),
                    None => c.bad(&a, true),
                },
                "--center" => match it.next().map(|v| parse_pair_f32(&v)) {
                    Some(Some(p)) => c.center = Some(p),
                    _ => c.bad(&a, true),
                },
                "--scale" => match it.next().map(|v| v.parse::<f32>()) {
                    Some(Ok(v)) => c.scale = Some(v),
                    _ => c.bad(&a, true),
                },
                "--layers" => match it.next().map(|v| parse_layers(&v)) {
                    Some(Some(m)) => c.layers = Some(m),
                    _ => c.bad(&a, true),
                },
                "--collision" => match it.next().map(|v| parse_collision(&v)) {
                    Some(Some([low, high])) => {
                        c.collision = Some(miredit_render::CollisionOverlay {
                            low,
                            high,
                            alpha: miredit_render::CollisionOverlay::DEFAULT_ALPHA,
                        })
                    }
                    _ => c.bad(&a, true),
                },
                "--layer" => match it.next().map(|v| parse_layer(&v)) {
                    Some(Some(l)) => c.layer = Some(l),
                    _ => c.bad(&a, true),
                },
                "--edit-layer" => match it.next().map(|v| parse_layer(&v)) {
                    Some(Some(l)) => c.edits.push(EditCmd::Layer(l)),
                    _ => c.bad(&a, true),
                },
                "--tool" => match it.next().map(|v| v.parse::<usize>()) {
                    Some(Ok(v)) if v < crate::icons::TOOLS.len() => c.tool = Some(v),
                    _ => c.bad(&a, true),
                },
                "--brush-size" => match it.next().map(|v| v.parse::<usize>()) {
                    // 非 0 的整数都收；**规范化**（夹到 3..=31 的奇数）由 `set_brush_size` 做，
                    // 这样命令行、`[`/`]` 键、官方滑块三者是同一套判据（不在这里另写一遍）
                    Some(Ok(v)) => c.brush_size = Some(v),
                    _ => c.bad(&a, true),
                },
                "--edit-size" => match it.next().map(|v| v.parse::<usize>()) {
                    Some(Ok(v)) => c.edits.push(EditCmd::Size(v)),
                    _ => c.bad(&a, true),
                },
                "--edit-pick" => match it.next().map(|v| parse_pair_u16(&v)) {
                    Some(Some((x, y))) => c.edits.push(EditCmd::Pickup(x, y)),
                    _ => c.bad(&a, true),
                },
                "--edit-brush" => match it.next().map(|v| parse_pair_u16(&v)) {
                    Some(Some((x, y))) => c.edits.push(EditCmd::Brush(x, y)),
                    _ => c.bad(&a, true),
                },
                "--edit-erase" => match it.next().map(|v| parse_pair_u16(&v)) {
                    Some(Some((x, y))) => c.edits.push(EditCmd::Erase(x, y)),
                    _ => c.bad(&a, true),
                },
                "--edit-fill" => match it.next().map(|v| parse_pair_u16(&v)) {
                    Some(Some((x, y))) => c.edits.push(EditCmd::Fill(x, y)),
                    _ => c.bad(&a, true),
                },
                "--edit-stroke" => match it.next().map(|v| parse_quad_u16(&v)) {
                    Some(Some((x0, y0, x1, y1))) => c
                        .edits
                        .push(EditCmd::Stroke((x0, y0), (x1, y1))),
                    _ => c.bad(&a, true),
                },
                "--edit-undo" => match it.next().map(|v| v.parse::<u32>()) {
                    Some(Ok(n)) => c.edits.push(EditCmd::Undo(n)),
                    _ => c.bad(&a, true),
                },
                "--edit-redo" => match it.next().map(|v| v.parse::<u32>()) {
                    Some(Ok(n)) => c.edits.push(EditCmd::Redo(n)),
                    _ => c.bad(&a, true),
                },
                "--edit-check" => c.edits.push(EditCmd::Check),
                "--save" => c.save = true,
                "--save-as" => match it.next() {
                    Some(v) => c.save_as = Some(PathBuf::from(v)),
                    None => c.bad(&a, true),
                },
                "--no-window" => c.no_window = true,
                "--shot" => match it.next() {
                    Some(v) => c.shot = Some(PathBuf::from(v)),
                    None => c.bad(&a, true),
                },
                "--frames" => match it.next().map(|v| v.parse::<u64>()) {
                    Some(Ok(v)) => c.frames = Some(v),
                    _ => c.bad(&a, true),
                },
                "--gate2-client" => match it.next() {
                    Some(v) => c.gate2_client = Some(PathBuf::from(v)),
                    None => c.bad(&a, true),
                },
                "--gate2-at" => match it.next().map(|v| parse_pair_i32(&v)) {
                    Some(Some(p)) => c.gate2_at = Some(p),
                    _ => c.bad(&a, true),
                },
                "--autodrag" => match it.next().map(|v| v.parse::<f32>()) {
                    Some(Ok(v)) => c.autodrag = Some(v),
                    _ => c.bad(&a, true),
                },
                "--hover" => match it.next().map(|v| parse_pair_u16(&v)) {
                    Some(Some(p)) => c.hover = Some(p),
                    _ => c.bad(&a, true),
                },
                "--probe" => match it.next().map(|v| parse_pair_u16(&v)) {
                    Some(Some(p)) => c.probe = Some(p),
                    _ => c.bad(&a, true),
                },
                "--dump-render" => match it.next() {
                    Some(v) => c.dump_render = Some(PathBuf::from(v)),
                    None => c.bad(&a, true),
                },
                "--dump-origin" => match it.next().map(|v| parse_pair_i32(&v)) {
                    Some(Some(p)) => c.dump_origin = Some(p),
                    _ => c.bad(&a, true),
                },
                "--dump-cells" => match it.next().map(|v| parse_pair_u32(&v)) {
                    Some(Some(p)) => c.dump_cells = Some(p),
                    _ => c.bad(&a, true),
                },
                "--palette-lib" => match it.next() {
                    Some(v) => c.palette_lib = Some(v),
                    None => c.bad(&a, true),
                },
                "--palette-scroll" => match it.next().map(|v| v.parse::<usize>()) {
                    Some(Ok(v)) => c.palette_scroll = Some(v),
                    _ => c.bad(&a, true),
                },
                "--palette-search" => match it.next() {
                    Some(v) => c.palette_search = Some(v),
                    None => c.bad(&a, true),
                },
                "--palette-tab" => match it.next().map(|v| v.parse::<usize>()) {
                    Some(Ok(v)) if v < crate::ui_text::PALETTE_TABS.len() => c.palette_tab = Some(v),
                    _ => c.bad(&a, true),
                },
                "--palette-autoscroll" => match it.next().map(|v| v.parse::<f32>()) {
                    Some(Ok(v)) => c.palette_autoscroll = Some(v),
                    _ => c.bad(&a, true),
                },
                "--palette-report" => c.palette_report = true,
                "--palette-pick" => match it.next().map(|v| v.parse::<u32>()) {
                    Some(Ok(v)) => c.palette_pick = Some(v),
                    _ => c.bad(&a, true),
                },
                "--palette-combo" => c.palette_combo = true,
                "--stamp-kind" => match it.next() {
                    Some(v) => c.stamp_kind = Some(v),
                    None => c.bad(&a, true),
                },
                "--stamp-search" => match it.next() {
                    Some(v) => c.stamp_search = Some(v),
                    None => c.bad(&a, true),
                },
                "--stamp-page" => match it.next().map(|v| v.parse::<usize>()) {
                    Some(Ok(v)) if v >= 1 => c.stamp_page = Some(v),
                    _ => c.bad(&a, true),
                },
                "--stamp-pick" => match it.next().map(|v| v.parse::<usize>()) {
                    Some(Ok(v)) => c.stamp_pick = Some(v),
                    _ => c.bad(&a, true),
                },
                "--stamp-report" => c.stamp_report = true,
                "--stamp-probe" => c.stamp_probe = true,
                "--stamp-sweep" => c.stamp_sweep = true,
                // ── 印章放置 / 选区（2026-11-05）：截图取证与无窗验收共用 ──
                "--stamp-arm" => c.stamp_arm = true,
                "--stamp-place-at" => match it.next().map(|v| parse_pair_u16(&v)) {
                    Some(Some(p)) => c.stamp_place_at = Some(p),
                    _ => c.bad(&a, true),
                },
                "--select" => match it.next().map(|v| parse_quad_u16(&v)) {
                    Some(Some(r)) => c.select = Some(r),
                    _ => c.bad(&a, true),
                },
                "--select-copy" => c.select_copy = true,
                // ── 菜单系统 / 深色模式（2026-11-05） ──
                "--dark" => c.dark = true,
                "--theme" => match it.next().map(|v| parse_theme(&v)) {
                    Some(Some(dark)) => c.theme = Some(dark),
                    _ => c.bad(&a, true),
                },
                // 单元格网格（视图 → 单元格网格 / `G`）：截图取证要能开/关各拍一张
                "--cell-grid" => c.cell_grid = true,
                // 默认开（官方 IsChecked="True"）之后，"关"才是需要开关的那一边
                "--no-cell-grid" => c.no_cell_grid = true,
                // 开启门（视图 → 开启门）：门开帧偏移的初始值，截图/转储取证要能开/关各拍一张
                "--open-doors" => c.open_doors = true,
                // 地图光照（视图 → 地图光照）：缺省 Day，三档取证各拍一张
                "--lighting" => match it.next().map(|v| parse_lighting(&v)) {
                    Some(Some(l)) => c.lighting = l,
                    _ => c.bad(&a, true),
                },
                "--menu-open" => match it.next().map(|v| v.parse::<usize>()) {
                    Some(Ok(v)) if v < crate::menu::model::MENUS.len() => c.menu_open = Some(v),
                    _ => c.bad(&a, true),
                },
                // 子菜单取证入口：把某一项设成高亮项（子面板才会展开），见 `Cli::menu_hover`
                "--menu-hover" => match it.next().map(|v| v.parse::<usize>()) {
                    Some(Ok(v)) => c.menu_hover = Some(v),
                    _ => c.bad(&a, true),
                },
                // 子行取证入口：把键盘焦点送进子菜单并停在第 <行> 行（子行高亮），
                // 见 `Cli::menu_sub_focus`
                "--menu-sub-focus" => match it.next().map(|v| parse_pair_usize(&v)) {
                    Some(Some(pair)) => c.menu_sub_focus = Some(pair),
                    _ => c.bad(&a, true),
                },
                "--menu-click" => match it.next() {
                    // 认 id 时要先剥掉可选的 `@帧号` 后缀（见 [`parse_menu_click`]）。
                    // `find_action_by_id` 同时认顶级项与静态子项（`view.light_night` 这类
                    // 子项 id 是地图光照接线后新增的可派发动作，见该函数文档）。
                    Some(v)
                        if crate::menu::model::find_action_by_id(parse_menu_click(&v).1).is_some() =>
                    {
                        c.menu_clicks.push(v)
                    }
                    // 认不出的 id 也要**记下来**（启动后打一条"没有这个菜单项"，
                    // 而不是静默忽略 —— 静默忽略会让验收以为点过了）
                    Some(v) => {
                        c.bad(&format!("{a} {v}"), true);
                        c.menu_clicks.push(v);
                    }
                    None => c.bad(&a, true),
                },
                "--menu-dump" => c.menu_dump = true,
                // 导出取证入口（不开窗；与菜单模态回车走同一个 `crate::export`）
                "--export-selection" => match it.next() {
                    Some(v) => c.export_selection = Some(PathBuf::from(v)),
                    None => c.bad(&a, true),
                },
                "--export-minimap" => match it.next() {
                    Some(v) => c.export_minimap = Some(PathBuf::from(v)),
                    None => c.bad(&a, true),
                },
                // 导出时按门全开出图（缺省跟当前门态，见 `Cli::export_doors_open`）
                "--export-doors" => c.export_doors = true,
                // 小地图目标长边：小于下限就判非法（不静默夹到下限 —— 那会让人以为
                // 「我给的 16 生效了」，实际出的是别的尺寸）
                "--minimap-max-side" => match it.next() {
                    Some(v) => match v.trim().parse::<u32>() {
                        Ok(n) if n >= crate::export::MINIMAP_MAX_SIDE_MIN => {
                            c.minimap_max_side = Some(n)
                        }
                        _ => c.bad(&a, true),
                    },
                    None => c.bad(&a, true),
                },
                // 官方 @acfbbf8a 尺寸面板：宽 / 高三档预设。0 也先收进来，
                // 由 export::valid_minimap_size 给中文原因（不是静默夹到 1）。
                "--minimap-width" => match it.next().map(|v| v.trim().parse::<u32>()) {
                    Some(Ok(n)) => c.minimap_width = Some(n),
                    _ => c.bad(&a, true),
                },
                "--minimap-height" => match it.next().map(|v| v.trim().parse::<u32>()) {
                    Some(Ok(n)) => c.minimap_height = Some(n),
                    _ => c.bad(&a, true),
                },
                "--minimap-preset" => match it.next().and_then(|v| crate::export::MinimapPreset::parse(&v)) {
                    Some(p) => c.minimap_preset = Some(p),
                    None => c.bad(&a, true),
                },
                "--open-map" => match it.next() {
                    Some(v) => c.open_map = Some(PathBuf::from(v)),
                    None => c.bad(&a, true),
                },
                "--shortcuts" => c.shortcuts = true,
                "--panel-hide" => match it.next() {
                    Some(v) => c.panel_hide.push(v),
                    None => c.bad(&a, true),
                },
                "--map-list-root" => match it.next() {
                    Some(v) => c.map_list_root = Some(PathBuf::from(v)),
                    None => c.bad(&a, true),
                },
                "--show-maps" => c.show_maps = true,
                "--settings" => match it.next() {
                    Some(v) => c.settings = Some(PathBuf::from(v)),
                    None => c.bad(&a, true),
                },
                _ => c.bad(&a, false),
            }
        }
        c
    }

    /// 记一条参数错误（`need_value` 决定要不要额外说明"缺值"）。
    fn bad(&mut self, arg: &str, need_value: bool) {
        let text = if need_value {
            format!("{arg}（缺少或非法的参数值）")
        } else {
            arg.to_owned()
        };
        eprintln!("{}", crate::ui_text::fill(crate::ui_text::DIAG_BAD_ARG, &[("arg", &text)]));
        self.errors.push(text);
    }
}

/// `x,y` → `(f32, f32)`
fn parse_pair_f32(s: &str) -> Option<(f32, f32)> {
    let mut it = s.split(',');
    let x: f32 = it.next()?.trim().parse().ok()?;
    let y: f32 = it.next()?.trim().parse().ok()?;
    Some((x, y))
}

/// `x,y` → `(i32, i32)`
fn parse_pair_i32(s: &str) -> Option<(i32, i32)> {
    let mut it = s.split(',');
    let x: i32 = it.next()?.trim().parse().ok()?;
    let y: i32 = it.next()?.trim().parse().ok()?;
    Some((x, y))
}

/// `x,y` → `(u16, u16)`
fn parse_pair_u16(s: &str) -> Option<(u16, u16)> {
    let mut it = s.split(',');
    let x: u16 = it.next()?.trim().parse().ok()?;
    let y: u16 = it.next()?.trim().parse().ok()?;
    Some((x, y))
}

/// `父项,子行` → `(usize, usize)`（`--menu-sub-focus` 用）
fn parse_pair_usize(s: &str) -> Option<(usize, usize)> {
    let mut it = s.split(',');
    let p: usize = it.next()?.trim().parse().ok()?;
    let r: usize = it.next()?.trim().parse().ok()?;
    Some((p, r))
}

/// `x,y` → `(u32, u32)`
fn parse_pair_u32(s: &str) -> Option<(u32, u32)> {
    let mut it = s.split(',');
    let x: u32 = it.next()?.trim().parse().ok()?;
    let y: u32 = it.next()?.trim().parse().ok()?;
    Some((x, y))
}

/// `x0,y0,x1,y1` → 四元组（`--edit-stroke` 用：一次拖动 = 起点 + 终点）
fn parse_quad_u16(s: &str) -> Option<(u16, u16, u16, u16)> {
    let mut it = s.split(',');
    let x0: u16 = it.next()?.trim().parse().ok()?;
    let y0: u16 = it.next()?.trim().parse().ok()?;
    let x1: u16 = it.next()?.trim().parse().ok()?;
    let y1: u16 = it.next()?.trim().parse().ok()?;
    Some((x0, y0, x1, y1))
}

/// `dawn_dusk` / `day` / `night` → 地图光照档位（其他写法一律 `None`，由调用方报错）。
///
/// 主名按下划线（与任务书一致），另兼容 render crate 的短名 `dawn-dusk` / `dawndusk`
/// （`render_maps` 二进制用的就是前者）；大小写不敏感。
/// 档位值直接取 [`miredit_render::MapLighting`] 的枚举，不另造一份映射。
fn parse_lighting(s: &str) -> Option<miredit_render::MapLighting> {
    use miredit_render::MapLighting as L;
    match s.trim().to_ascii_lowercase().as_str() {
        "dawn_dusk" | "dawn-dusk" | "dawndusk" => Some(L::DawnDusk),
        "day" => Some(L::Day),
        "night" => Some(L::Night),
        _ => None,
    }
}

/// `light` / `dark` → 是否深色（其他写法一律 `None`，由调用方报错）。
fn parse_theme(s: &str) -> Option<bool> {
    match s.trim().to_ascii_lowercase().as_str() {
        "light" => Some(false),
        "dark" => Some(true),
        _ => None,
    }
}

/// `--menu-click` 的取值 → `(第几帧执行, 菜单项 id)`。
///
/// # 语法
///
/// ```text
/// view.open_doors          ⇒ (1, "view.open_doors")      // 缺省：第 1 帧（首帧布局后）
/// view.open_doors@40       ⇒ (40, "view.open_doors")     // 推迟到第 40 帧
/// ```
///
/// # 为什么要有 `@帧号`
///
/// 「同一会话里切换开关 ⇒ 画面重新渲染」这条验收要求**先渲一帧、再切、再渲**：
/// 所有点击都在第 1 帧执行时，切换发生在首帧重渲**之前**（`update` 里
/// ②b 点击 → ⑤ 重渲），于是缓存键失效这件事根本观察不到。
/// 给了 `@帧号` 就能在同一次运行里看到两条 `[渲染]` 诊断（`doors=0` 然后 `doors=1`）。
///
/// # 边界
///
/// * `@` 后面不是非负整数 ⇒ 整条按"没写帧号"处理（第 1 帧），**不静默改 id**；
/// * 帧号 `0` 与 `1` 等价（首帧），`u64::MAX` 合法（等于永不执行 —— 调用方会打诊断）。
pub fn parse_menu_click(s: &str) -> (u64, &str) {
    match s.rsplit_once('@') {
        Some((id, frame)) if !id.is_empty() => match frame.trim().parse::<u64>() {
            Ok(f) => (f.max(1), id),
            Err(_) => (1, s),
        },
        _ => (1, s),
    }
}


/// `ground` / `middle` / `front` / `low_collision` / `high_collision` → 当前编辑层
/// （其他写法一律 `None`，由调用方报错）。
///
/// 取值与 [`crate::edit::EditLayer::key`] **逐字对应**（单元测试钉住往返）：
/// 命令行、`EDIT_RESULT` 日志、界面层名三者不许各写一套名字。
fn parse_layer(s: &str) -> Option<crate::edit::EditLayer> {
    let t = s.trim();
    crate::edit::EditLayer::ALL
        .into_iter()
        .find(|l| l.key() == t)
}

/// `low` / `high` / `all` / `none`（也接受 `low,high` 这种子集写法）→ `[低位, 高位]`。///
/// 为什么不用 `--layers` 那套"列出即开"：阻挡只有两个通道，`all` / `none` 更顺手，
/// 而 `low,high` 与 `--layers` 的写法保持一致，两种都能用。
fn parse_collision(s: &str) -> Option<[bool; 2]> {
    match s.trim() {
        "none" => return Some([false, false]),
        "all" => return Some([true, true]),
        _ => {}
    }
    let mut out = [false; 2];
    let mut seen = false;
    for part in s.split(',') {
        match part.trim() {
            "low" => {
                out[0] = true;
                seen = true;
            }
            "high" => {
                out[1] = true;
                seen = true;
            }
            "" => {}
            _ => return None,
        }
    }
    if seen {
        Some(out)
    } else {
        None
    }
}

/// `ground,middle,front` 的子集 → `[bool; 3]`（未列出的即 `false`）。
///
/// 这样 `--layers ground,middle` 就是"关掉物件层"，比 `--hide front` 少一个反向逻辑。
fn parse_layers(s: &str) -> Option<[bool; 3]> {
    let mut out = [false; 3];
    let mut seen = false;
    for part in s.split(',') {
        match part.trim() {
            "ground" => {
                out[0] = true;
                seen = true;
            }
            "middle" => {
                out[1] = true;
                seen = true;
            }
            "front" => {
                out[2] = true;
                seen = true;
            }
            "" => {}
            _ => return None,
        }
    }
    if seen {
        Some(out)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Cli {
        Cli::parse(args.iter().map(|s| (*s).to_string()))
    }

    /// @acfbbf8a 官方尺寸链：缺省 `W*12×H*8`，三档预设与显式宽高从 CLI 正常入口进来。
    #[test]
    fn 小地图官方尺寸开关() {
        let c = p(&[]);
        assert_eq!(c.minimap_size().unwrap(), crate::export::MinimapSize::Default);
        assert!(c.minimap_max_side.is_none(), "缺省不许走旧 2048 兼容链");

        for (name, preset) in [
            ("small", crate::export::MinimapPreset::Small),
            ("medium", crate::export::MinimapPreset::Medium),
            ("large", crate::export::MinimapPreset::Large),
        ] {
            let c = p(&["--minimap-preset", name]);
            assert_eq!(
                c.minimap_size().unwrap(),
                crate::export::MinimapSize::Preset(preset),
                "--minimap-preset {name}"
            );
            assert!(c.errors.is_empty(), "{name} 不该报错：{:?}", c.errors);
        }
        let c = p(&["--minimap-width", "321", "--minimap-height", "123"]);
        assert_eq!(
            c.minimap_size().unwrap(),
            crate::export::MinimapSize::Explicit { width: 321, height: 123 }
        );
        // 0 先收进来，由 export::valid_minimap_size 给中文原因；不在解析层偷偷夹成 1
        let c = p(&["--minimap-width", "0", "--minimap-height", "123"]);
        assert_eq!(c.minimap_width, Some(0));
        assert!(c.errors.is_empty(), "0 是合法 u32，错误由尺寸判据给");
        // 只给一个 / 冲突 / 拼错 ⇒ 明确错误
        assert!(p(&["--minimap-width", "10"]).minimap_size().is_err());
        assert!(p(&["--minimap-preset", "small", "--minimap-width", "1", "--minimap-height", "1"])
            .minimap_size()
            .is_err());
        assert_eq!(p(&["--minimap-preset", "huge"]).errors.len(), 1);
        assert_eq!(p(&["--minimap-width", "x"]).errors.len(), 1);
        assert_eq!(p(&["--minimap-height"]).errors.len(), 1);
    }

    #[test]
    fn 缺省全部为空() {
        let c = p(&[]);
        assert!(c.map.is_none());
        assert!(c.errors.is_empty());
        assert!(!c.help);
    }

    #[test]
    fn 常规开关() {
        // ⚠ 这些路径**只做字符串往返**（解析后原样比对），不落盘 ⇒ 用相对路径即可，
        // 不写死盘符（公开仓/PR 的机器上没有 `E:`/`I:`）。
        let c = p(&[
            "--map",
            r"maps\0.map",
            "--center",
            "380,310",
            "--scale",
            "0.5",
            "--shot",
            r"shots\a.png",
            "--frames",
            "12",
            "--autodrag",
            "2",
            "--hover",
            "100,100",
        ]);
        assert_eq!(c.map.unwrap().to_string_lossy(), r"maps\0.map");
        assert_eq!(c.center, Some((380.0, 310.0)));
        assert_eq!(c.scale, Some(0.5));
        assert_eq!(c.frames, Some(12));
        assert_eq!(c.autodrag, Some(2.0));
        assert_eq!(c.hover, Some((100, 100)));
        assert!(c.errors.is_empty(), "{:?}", c.errors);
    }

    #[test]
    fn 图层开关是白名单() {
        assert_eq!(parse_layers("ground,middle"), Some([true, true, false]));
        assert_eq!(parse_layers("front"), Some([false, false, true]));
        assert_eq!(parse_layers(""), None);
        assert_eq!(parse_layers("ground,bogus"), None);
    }

    /// `--layer` 设**当前编辑层**、`--edit-layer` 进编辑脚本；五个层名都认。
    #[test]
    fn 当前编辑层开关() {
        use crate::edit::{EditCmd, EditLayer};
        let c = p(&["--layer", "front"]);
        assert_eq!(c.layer, Some(EditLayer::Front));
        assert!(c.edits.is_empty(), "--layer 不是脚本指令");

        let c = p(&["--edit-layer", "middle"]);
        assert_eq!(c.edits, vec![EditCmd::Layer(EditLayer::Middle)]);
        assert!(c.layer.is_none(), "--edit-layer 不改初始值");

        // 阻挡层现在也是编辑层（审计 P0-9 接线）
        for (name, want) in [
            ("low_collision", EditLayer::LowCollision),
            ("high_collision", EditLayer::HighCollision),
        ] {
            assert_eq!(p(&["--layer", name]).layer, Some(want), "{name} 应被接受");
            assert_eq!(
                p(&["--edit-layer", name]).edits,
                vec![EditCmd::Layer(want)],
                "{name} 应能进脚本"
            );
        }

        // 拼错 / 缩写 / 组合写 / 空串一律拒绝并报错（不猜）
        for bad in ["low", "high", "ground,middle", "", "Ground", "lowcollision"] {
            let c = p(&["--layer", bad]);
            assert!(c.layer.is_none(), "{bad:?} 不该被接受");
            assert_eq!(c.errors.len(), 1, "{bad:?} 应记一条参数错误");
        }
    }

    /// **命令行取值与 `EditLayer::key()` 逐字往返**：日志里的 `layer=` 直接能喂回 `--layer`。
    #[test]
    fn 层名与命令行取值往返() {
        use crate::edit::EditLayer;
        for layer in EditLayer::ALL {
            let key = layer.key();
            assert_eq!(
                p(&["--layer", key]).layer,
                Some(layer),
                "--layer {key} 应解析回 {layer:?}"
            );
            assert_eq!(p(&["--edit-layer", key]).edits, vec![EditCmd::Layer(layer)]);
        }
    }

    /// `--collision` 只有四个合法取值（含 `low,high` 子集写法）。
    #[test]
    fn 阻挡通道开关() {
        let c = p(&["--collision", "low"]);
        let o = c.collision.expect("应解析出 overlay");
        assert!(o.low && !o.high);
        assert_eq!(o.alpha, miredit_render::CollisionOverlay::DEFAULT_ALPHA);

        let o = p(&["--collision", "high"]).collision.unwrap();
        assert!(!o.low && o.high);
        let o = p(&["--collision", "all"]).collision.unwrap();
        assert!(o.low && o.high);
        let o = p(&["--collision", "none"]).collision.unwrap();
        assert!(!o.low && !o.high);
        let o = p(&["--collision", "low,high"]).collision.unwrap();
        assert!(o.low && o.high);

        // 缺省是 None（= 不覆盖，两个通道都关）
        assert!(p(&[]).collision.is_none());
        // 非法取值要报错且不采用
        for bad in ["", "both", "low,bogus", "1"] {
            let c = p(&["--collision", bad]);
            assert!(c.collision.is_none(), "{bad:?} 不该被接受");
            assert_eq!(c.errors.len(), 1, "{bad:?} 应记一条参数错误");
        }
    }

    /// `--lighting` 三档 + 兼容写法；缺省 Day；非法取值报错且不采用。
    #[test]
    fn 光照档位开关() {
        use miredit_render::{MapLighting as L, RenderOptions};
        let c = p(&[]);
        assert_eq!(c.lighting, L::Day, "缺省必须白天（逐字节不变的前提）");
        assert_eq!(c.export_options(), RenderOptions::default());
        for (name, want) in [
            ("dawn_dusk", L::DawnDusk),
            ("dawn-dusk", L::DawnDusk),
            ("dawndusk", L::DawnDusk),
            ("DAWN_DUSK", L::DawnDusk),
            ("day", L::Day),
            ("night", L::Night),
        ] {
            let c = p(&["--lighting", name]);
            assert_eq!(c.lighting, want, "--lighting {name} 解析错");
            assert!(c.errors.is_empty(), "{name} 不该报错：{:?}", c.errors);
            assert_eq!(c.export_options().lighting, want, "导出选项必须带同一档");
        }
        // 非法值 / 缺值：报错、回落白天（不猜一个"差不多"的档）
        for bad in ["", "dusk", "dawn dusk", "0", "Daylight"] {
            let c = p(&["--lighting", bad]);
            assert_eq!(c.lighting, L::Day, "{bad:?} 不该被采用");
            assert_eq!(c.errors.len(), 1, "{bad:?} 应记一条参数错误");
        }
        assert_eq!(p(&["--lighting"]).lighting, L::Day);
        assert_eq!(p(&["--lighting"]).errors.len(), 1);
        // 门态与光照同时给 ⇒ 两个 option 都进同一份 RenderOptions（不许二选一）
        let c = p(&["--open-doors", "--lighting", "night"]);
        assert_eq!(c.export_options(), RenderOptions::with_doors_and_lighting(true, L::Night));
        let c = p(&["--lighting", "dawn_dusk", "--export-doors"]);
        assert_eq!(
            c.export_options(),
            RenderOptions::with_doors_and_lighting(true, L::DawnDusk)
        );
    }

    /// `--menu-click` 必须认**子项 id**（地图光照三档），否则验收 4 跑不通。
    #[test]
    fn 菜单点击认子项id() {
        for id in [
            "view.light_dawn_dusk@40",
            "view.light_day",
            "view.light_night@60",
            "view.map_lighting",
            "view.open_doors@40",
        ] {
            let c = p(&["--menu-click", id]);
            assert!(c.errors.is_empty(), "{id} 应被认出：{:?}", c.errors);
            assert_eq!(c.menu_clicks, vec![id.to_owned()]);
        }
        let c = p(&["--menu-click", "view.no_such_item"]);
        assert_eq!(c.errors.len(), 1, "认不出的 id 仍要报错（不静默忽略）");
        assert_eq!(c.menu_clicks, vec!["view.no_such_item".to_owned()]);
    }

    #[test]
    fn 非法参数被记下但不中断() {
        let c = p(&["--bogus", "--scale", "abc", "--gate2-at", "1"]);
        assert_eq!(c.errors.len(), 3, "{:?}", c.errors);
        assert!(c.scale.is_none(), "解析失败的值不能被采用");
        assert!(c.gate2_at.is_none());
    }

    #[test]
    fn 缺值也报错() {
        let c = p(&["--map"]);
        assert_eq!(c.errors.len(), 1);
        assert!(c.map.is_none());
    }

    #[test]
    fn 探针与帮助() {
        let c = p(&["--probe", "380,310", "--help"]);
        assert_eq!(c.probe, Some((380, 310)));
        assert!(c.help);
    }

    /// 编辑脚本按**出现顺序**入列（顺序错了会把画笔打在没取笔刷之前）。
    #[test]
    fn 编辑脚本保序() {
        let c = p(&[
            "--edit-pick",
            "10,20",
            "--edit-brush",
            "30,40",
            "--edit-erase",
            "41,42",
            "--edit-undo",
            "2",
            "--edit-redo",
            "1",
            "--edit-check",
            "--save",
        ]);
        assert_eq!(
            c.edits,
            vec![
                EditCmd::Pickup(10, 20),
                EditCmd::Brush(30, 40),
                EditCmd::Erase(41, 42),
                EditCmd::Undo(2),
                EditCmd::Redo(1),
                EditCmd::Check,
            ]
        );
        assert!(c.save);
        assert!(c.headless(), "带 --save 就该是不开窗模式");
        assert!(c.errors.is_empty(), "{:?}", c.errors);
    }

    /// `--tool` 越界要被夹掉并报错（不静默接受一个点不存在的图标）。
    #[test]
    fn 工具下标越界报错() {
        let c = p(&["--tool", "99"]);
        assert_eq!(c.tool, None);
        assert_eq!(c.errors.len(), 1);
        let c = p(&["--tool", "3"]);
        assert_eq!(c.tool, Some(3));
    }

    /// 编辑脚本的坐标解析失败必须报错且不入列（不能默默当成 0,0 去改图）。
    #[test]
    fn 编辑坐标非法被拒() {
        let c = p(&["--edit-brush", "abc", "--edit-pick", "1"]);
        assert!(c.edits.is_empty(), "{:?}", c.edits);
        assert_eq!(c.errors.len(), 2);
    }

    /// `--no-window` 单独也是不开窗模式；两者都没有就是正常开窗。
    #[test]
    fn 不开窗开关() {
        assert!(p(&["--no-window"]).headless());
        assert!(!p(&[]).headless());
        assert!(!p(&["--edit-check"]).headless(), "只跑检查仍可开窗取证");
    }

    /// **`--save-as <路径>`**（2026-11-05）：
    /// ① 带值才收；② 缺值报错且不采用；③ **隐含不开窗**（存完就该退出）；
    /// ④ 与 `--save` 互不影响（两个开关各自独立）。
    #[test]
    fn 另存为开关() {
        let c = p(&["--save-as", r"saveas\b.map"]);
        assert_eq!(
            c.save_as.as_deref(),
            Some(std::path::Path::new(r"saveas\b.map"))
        );
        assert!(c.errors.is_empty(), "{:?}", c.errors);
        assert!(c.headless(), "--save-as 必须隐含不开窗（否则无人值守跑不起来）");
        assert!(!c.save, "--save-as 不该顺带置 --save");

        // 缺值：报错且不采用
        let c = p(&["--save-as"]);
        assert!(c.save_as.is_none());
        assert_eq!(c.errors.len(), 1);

        // 与 --save 同时给：两个都记下（先写原路径、再另存到新路径）
        let c = p(&["--save", "--save-as", r"x\b.map"]);
        assert!(c.save && c.save_as.is_some());
        assert!(c.errors.is_empty(), "{:?}", c.errors);

        // 缺省：没有 --save-as 就是 None（不猜一个默认目标路径）
        assert!(p(&[]).save_as.is_none());
    }

    /// `--cell-grid` / `--no-cell-grid` 是**无值开关**，且优先级 `--no-cell-grid` > `--cell-grid`
    /// > 默认 `true`（官方 `MainWindow.axaml:5` 的 `IsChecked="True"`）。
    ///
    /// 为什么单测这一条：它是"网格开/关两张图做像素差"的唯一无人值守入口，
    /// 一旦被写成要带值（或误把下一个参数当值吞掉），验收脚本会静默拍出两张一样的图。
    #[test]
    fn 单元格网格开关() {
        // ① 缺省**开**（2026-11-05 订正：旧版这里断言"缺省必须是关"，那是形态偏离官方）
        assert!(p(&[]).cell_grid_enabled(), "官方 IsChecked=True ⇒ 缺省必须开");
        let c = p(&["--cell-grid"]);
        assert!(c.cell_grid);
        assert!(c.cell_grid_enabled(), "--cell-grid 是显式开");
        assert!(c.errors.is_empty(), "{:?}", c.errors);
        // 后面跟别的开关时不许吞掉它们
        let c = p(&["--cell-grid", "--dark", "--menu-click", "view.cell_grid"]);
        assert!(c.cell_grid && c.dark);
        assert_eq!(c.menu_clicks, vec!["view.cell_grid".to_string()]);
        assert!(c.errors.is_empty(), "{:?}", c.errors);
        // 不开窗模式也认（headless 只跑脚本，但开关本身要解析得出来）
        assert!(p(&["--cell-grid", "--no-window"]).headless());
        // ② `--no-cell-grid` 才是关的手段
        let off = p(&["--no-cell-grid"]);
        assert!(off.no_cell_grid);
        assert!(!off.cell_grid_enabled(), "--no-cell-grid 必须关掉网格");
        assert!(off.errors.is_empty(), "{:?}", off.errors);
        // ③ 两个都给 ⇒ 关优先（"关"是更明确的那一步）
        let both = p(&["--cell-grid", "--no-cell-grid"]);
        assert!(!both.cell_grid_enabled(), "--no-cell-grid 优先级最高");
        assert!(both.errors.is_empty(), "{:?}", both.errors);
        // ④ 不带值：不会把后面的开关吞掉
        let c = p(&["--no-cell-grid", "--dark"]);
        assert!(c.no_cell_grid && c.dark && !c.cell_grid_enabled());
        assert!(c.errors.is_empty(), "{:?}", c.errors);
    }

    /// `--menu-hover <菜单内下标>`：子菜单取证入口，取值与报错口径。
    ///
    /// 它只在同时给了 `--shot` / `--menu-open` 时生效（判据在 `app::MireditApp::new`），
    /// 但**解析层不看那个条件** —— 组合由调用方判，这里只保证"值解析得出来、坏值报错"。
    #[test]
    fn 菜单高亮下标() {
        assert_eq!(p(&[]).menu_hover, None, "缺省不设高亮项");
        assert_eq!(p(&["--menu-hover", "2"]).menu_hover, Some(2));
        // 缺值 / 非数字 ⇒ 报错（不静默当成 0）
        assert_eq!(p(&["--menu-hover"]).errors.len(), 1);
        assert_eq!(p(&["--menu-hover", "x"]).errors.len(), 1);
        // 截图取证的标准组合
        let c = p(&["--menu-open", "2", "--menu-hover", "2", "--shot", r"I:\x.png"]);
        assert_eq!((c.menu_open, c.menu_hover), (Some(2), Some(2)));
        assert!(c.errors.is_empty(), "{:?}", c.errors);
        // 后面跟别的开关时不许吞掉它们
        let c = p(&["--menu-hover", "3", "--dark"]);
        assert_eq!(c.menu_hover, Some(3));
        assert!(c.dark && c.errors.is_empty(), "{:?}", c.errors);
    }

    /// `--open-doors`：开启门的**初始值**（缺省 false = 闭门，与改动前逐字节相同）。
    ///
    /// 这条只钉解析：真生效由 `app::tests` 与 `canvas_view::tests` 各自钉一处
    /// （宿主字段 / 缓存键），三处用的是同一个字段，不许各写一套。
    #[test]
    fn 开启门开关() {
        assert!(!p(&[]).open_doors, "缺省必须闭门（RenderOptions::default()）");
        assert!(p(&["--open-doors"]).open_doors);
        // 不带值：不会把后面的开关吞掉
        let c = p(&["--open-doors", "--dark"]);
        assert!(c.open_doors && c.dark);
        assert!(c.errors.is_empty(), "{:?}", c.errors);
    }

    /// **导出用的门态**（`--export-doors` / `--open-doors` 的优先级，唯一判据）。
    ///
    /// 判据：① 两个都不给 ⇒ 闭门（与接线前逐字节相同）；
    /// ② 只给 `--open-doors` ⇒ 开（**缺省跟当前门态**，不是硬写 false）；
    /// ③ 只给 `--export-doors` ⇒ 开（不改窗口状态也能取到开门态 PNG）；
    /// ④ 两个都给 ⇒ 开（`--export-doors` 是"更明确的那一步"，不该被别的开关翻掉）。
    #[test]
    fn 导出默认跟当前门态() {
        let 缺 = p(&[]);
        assert!(!缺.export_doors, "缺省不许自己开导出开门");
        assert!(!缺.export_doors_open(), "缺省必须闭门（RenderOptions::default()）");
        assert!(p(&["--open-doors"]).export_doors_open(), "缺省跟当前门态");
        assert!(p(&["--export-doors"]).export_doors_open());
        let 都 = p(&["--export-doors", "--open-doors"]);
        assert!(都.export_doors_open());
        assert!(都.errors.is_empty(), "{:?}", 都.errors);
    }

    /// `--menu-click` 的 `@帧号` 后缀（推迟执行；缺省第 1 帧）。
    ///
    /// 判据：① 不带 `@` ⇒ 第 1 帧（旧脚本行为**逐字不变**）；
    /// ② `@40` ⇒ 第 40 帧；③ 坏帧号 ⇒ 回落到第 1 帧且 **id 一个字都不改**；
    /// ④ 认不出的 id 仍然入列并报错（`--menu-click` 原有的"不静默忽略"纪律）。
    #[test]
    fn 菜单点击可推迟到指定帧() {
        assert_eq!(parse_menu_click("view.open_doors"), (1, "view.open_doors"));
        assert_eq!(parse_menu_click("view.open_doors@40"), (40, "view.open_doors"));
        assert_eq!(parse_menu_click("view.open_doors@0"), (1, "view.open_doors"));
        // 坏帧号：整条按"没写帧号"处理（id 不许被截断成 "view.open_doors@x"）
        assert_eq!(parse_menu_click("view.open_doors@x"), (1, "view.open_doors@x"));
        assert_eq!(parse_menu_click("@40"), (1, "@40"));
        // 解析层就认 id（剥掉后缀之后）：带后缀的合法项不许报错
        let c = p(&["--menu-click", "view.open_doors@40"]);
        assert_eq!(c.menu_clicks, vec!["view.open_doors@40".to_string()]);
        assert!(c.errors.is_empty(), "{:?}", c.errors);
        // 认不出的 id：入列 + 报错（启动后由 app 打"没有这个菜单项"）
        let c = p(&["--menu-click", "view.nope@40"]);
        assert_eq!(c.menu_clicks.len(), 1);
        assert_eq!(c.errors.len(), 1);
    }
}
