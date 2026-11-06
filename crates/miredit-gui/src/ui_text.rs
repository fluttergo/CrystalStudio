//! 界面文案总表 —— **所有界面字符串的唯一来源**（全量中文）。
//!
//! ## 三条规矩
//! 1. 绘制代码里**不许**再出现界面字面量（连 `×` / `—` / `←` 这类符号也算），一律引用本模块常量；
//! 2. 每条常量右端保留原英文 `// EN: ...`，便于回溯参考图，也便于将来做语言切换；
//! 3. 本表被 `fonts::tests::文案全部字符都有字形` **逐字遍历**，字体缺任何一个字形都会当场失败
//!    —— 这是"不许出现豆腐块"的第一道闸门（第二道是截图像素扫描）。
//!
//! ## 两类文案：静态字面量 与 运行期模板
//!
//! 本文件从「只画骨架」升级到「画真实地图」后，一部分文字必须带**真实数据**
//! （地图尺寸、资源库数量、鼠标所在格、单元格字段…）。为了仍然守住「文案唯一来源」，
//! 这类文字写成**带占位符的模板**（如 `单元格 {x},{y}`），由 [`fill`] 用真实值填充：
//!
//! - 模板本身是**编译期常量**，照样进 [`ALL`]，照样被字形测试逐字检查；
//! - 运行期填进去的只有**数字/文件名/库数**等数据，不含任何中文表述；
//! - 取不到的值一律填 [`VAL_UNKNOWN`]（`—`），**绝不编一个像样的假值**。
//!
//! ## 文案来源（2026-11-04 起：**以上游官方译文为准**）
//!
//! 口径按用户明确要求「翻译文字使用官方的」，权威来源两份，都在仓里：
//!
//! 1. `layout/crystalstudio-strings.json` —— 上游官方 893 条 `{English, Chinese}`
//!    （上游提交 `6f2dadde`）。**有官方译文的一律照抄**，每条常量右端注释里标
//!    `官方: <Strings.json 的 key>`，便于以后逐条追溯。
//! 2. `layout/refs/crystalstudio-new-zh.png` —— 新版运行截图（全中文），
//!    **最终视觉依据**。官方漏译处（例如该图里 `0 sets` 仍是英文）按截图里出现过的中文措辞
//!    自定，并在注释里写「官方漏译，按截图自定」。
//!
//! ⚠ **两份权威偶尔互相矛盾时取哪份**：`Strings.json` 是**译文表**（词条级），截图是**运行态**
//! （整句/带助记键）。矛盾处本文件在注释里两说并存，正文取截图（最终视觉依据），例如菜单项：
//! 表里是 `文件(_F)`，截图里是 `文件(F)`，正文取后者。
//!
//! 历史说明：本文件此前逐条照抄「任务书 §四 中文文案表」，`\n` 的断行位置沿用参考截图
//! `layout/refs/crystalstudio-main-runtime-1500x950.png`。那一版的若干译名与官方不一致
//! （旧称：素材面板 / 图库 / 物件层），本轮已全部按官方改正；旧称只在本文件注释里以「旧称」标注。
//!
//! ⚠ **品牌名例外**（2026-11-06 用户裁定，优先于上面两条口径）：软件显示名固定为
//! `Crystal Studio`（两个词，C/S 大写，不译成中文）。标题栏与菜单栏的品牌段一律写
//! `Crystal Studio`；`miredit-*` 只是 crate / 二进制的技术标识，不是产品名。
//!
//! 参考图里 Cell Inspector 的值列宽只有 81px（见 [`crate::metrics::INSPECTOR_VAL_X`]），
//! 中文比英文宽，`Metadata` 那条原为单行、中文单行会溢出 ⇒ 按参考图"长值换行"的既有做法拆成两行
//! （只拆行，不删字，不改语义）。

// ───────────────────────── 窗口标题 / 页签 ─────────────────────────

/// 窗口标题模板（同时用于 OS 窗口标题与自绘标题栏）。
///
/// 占位符：`{name}` 地图文件名、`{w}`/`{h}` 地图格数。
/// `•` U+2022、`×` U+00D7、`—` U+2014 都需要字体有字形（字形测试会查）。
///
/// ⚠ **品牌段固定为 `Crystal Studio`**（两个词，C/S 大写；品牌不译成中文）。
/// 依据 2026-11-06 用户明确裁定：软件显示名就叫 `Crystal Studio`，标题按新版截图的口径
/// 写 `— Crystal Studio`。官方译文表把 `Crystal Studio — Map Editor` 整句译作
/// `Crystal Studio — 地图编辑器`（`CrystalStudioMapEditor_AEA57DE1`），那**只作历史参照**，
/// 不再作为品牌口径 —— 上一版按该表把后缀译成「地图编辑器」的写法已订正。
pub const TITLE_TMPL: &str = "{name} • {w} × {h} 格 — Crystal Studio"; // EN: {name} • {w} × {h} cells — Crystal Studio ｜ 品牌不译（2026-11-06 裁定）；官方 key CrystalStudioMapEditor_AEA57DE1 仅作历史参照

/// 没有打开任何地图时的窗口标题（**不是**"未命名 + 假尺寸"，如实说没打开）。
pub const WINDOW_TITLE_EMPTY: &str = "Crystal Studio"; // EN: Crystal Studio ｜ 品牌不译（2026-11-06 裁定），同 [`BRAND`]

/// 窗口标题模板（**有未保存修改**）：文件名后加 ` *`。
///
/// 星号只表达"内存里的地图与磁盘不一致"这一件事，由
/// [`crate::edit::EditSession::dirty`] 驱动（撤销回到保存点后会自动消失）。
pub const TITLE_DIRTY_TMPL: &str = "{name} * • {w} × {h} 格 — Crystal Studio"; // EN: {name} * • {w} × {h} cells — Crystal Studio ｜ 品牌不译（2026-11-06 裁定），同 [`TITLE_TMPL`]

// ───────────────────────── 工具名（本单只接线 3 个） ─────────────────────────

/// 工具名：笔刷（`icons::TOOLS[0]`，JSON `tag_value = "Brush"`）。
///
/// 官方 `Brush_8624CA0F` = 「笔刷」（旧称「画笔」）。
pub const TOOL_BRUSH: &str = "笔刷"; // EN: Brush ｜ 官方: Brush_8624CA0F（旧称「画笔」）
/// 工具名：橡皮擦（`icons::TOOLS[3]`，MDL2 U+E75C 的等效自绘）。
///
/// 官方 `Eraser_686EEE3C` = 「橡皮擦」（旧称「橡皮」）。
pub const TOOL_ERASER: &str = "橡皮擦"; // EN: Eraser ｜ 官方: Eraser_686EEE3C（旧称「橡皮」）
/// 工具名：吸管（`icons::TOOLS[6]`，MDL2 U+E7C2 的等效自绘）。
///
/// ⚠ **这一条故意不采用官方译名**（2026-11-04 用户裁定）：官方 `Pickup_B685076A` 把
/// `Pickup` 译作「移动」，但那个 Pickup 的语义是"移动已记录的物体"
/// （`PickupVClickaRememberedStampThenClicktoMoveDeleteRemovesItEscapeC_544538C6`），
/// 本工具的语义是**从格子里把内容取成笔刷**（见 [`crate::edit::EditSession::pick_at`]），
/// 照抄「移动」会把"点一下就把内容拿走"说成"拖动搬东西" ⇒ 保留「吸管」，不误导用户。
pub const TOOL_PICKUP: &str = "吸管"; // EN: Pickup ｜ 官方: Pickup_B685076A =「移动」，语义不同，不采用（见上）
/// 工具名：填充（`icons::TOOLS[2]`，JSON `tag_value = "Bucket"`，快捷键 `F`）。
///
/// 官方 `Fill_033955D4` = 「填充」（工具提示是
/// `FillFFillConnectedMatchingTilesSmtilesorApplyLowHighCollisions_CED026EB` =
/// 「填充 · F / 填充连通且匹配的地表 / 中层图块，或应用高低位阻挡」）。
/// 快捷键真值：`MainWindow.axaml.cs:861` `Key.F => EditorTool.Bucket`。
pub const TOOL_BUCKET: &str = "填充"; // EN: Fill ｜ 官方: Fill_033955D4
/// 其余 6 个工具本单**未接线**（落到画布上什么都不做），如实显示这句。
pub const TOOL_UNWIRED: &str = "未接线"; // EN: Not wired

/// 工具名：**选择**（`icons::TOOLS[4]`，JSON `tag_value = "Select"`，工具提示 `Select · M`）。
///
/// 官方 `Strings.json` 里 `Select` 对应的词条是 `Select`（工具提示原文
/// `SelectMarqueeSelection_…` = 「选择 · M / 框选」），中文取「选择」。
/// ⚠ 内核 `miredit_core::Tool` **没有**这一支（它的契约只覆盖 4 个编辑工具 + `Unwired`），
/// 所以选择工具是**宿主层**的工具：它不改地图，只画选区（见 [`crate::selection_ops`]）。
pub const TOOL_SELECT: &str = "选择"; // EN: Select ｜ 官方: SelectMarqueeSelection（工具提示里的 `Select · M`）
/// 工具名：**印章**（`icons::TOOLS[5]`，JSON `tag_value = "Stamp"`，工具提示 `Stamp · O`）。
///
/// 与 [`TOOL_SELECT`] 同理：内核 `Tool` 没有这一支，印章工具是宿主层的
/// （真正落位走的是 [`crate::stamp_place`] 的 `register_stamp` + `place_stamp_by_id`）。
pub const TOOL_STAMP: &str = "印章"; // EN: Stamp ｜ 官方: Stamp_2910B6D7（物体模板）

// ───────────────────────── 菜单栏 ─────────────────────────

/// 菜单项：文件 / 编辑 / 视图 / 工具 / 帮助。
///
/// ⚠ **两份权威在这里不一致**（如实记录，不静默取一）：
/// - `Strings.json` 是**带下划线助记键**的 `文件(_F)` / `编辑(_E)` / `视图(_V)` / `工具(_T)` / `帮助(_H)`；
/// - 新版运行截图（`layout/refs/crystalstudio-new-zh.png` 菜单栏）是 **ASCII 括号、无下划线**的
///   `文件(F)` / `编辑(E)` / `视图(V)` / `工具(T)` / `帮助(H)`。
///
/// 正文取**截图**那一版（截图是最终视觉依据，且下划线助记键在自绘菜单栏里画不出来 —— 本仓
/// 不画下划线，写了 `_` 反而会多出一个可见的 `_`）。
pub const MENU_FILE: &str = "文件(F)"; // EN: File ｜ 官方: File_3B6577DD（表为 文件(_F)；截图实测 文件(F)）
pub const MENU_EDIT: &str = "编辑(E)"; // EN: Edit ｜ 官方: Edit_2B1AF61A（表为 编辑(_E)；截图实测 编辑(E)）
pub const MENU_VIEW: &str = "视图(V)"; // EN: View ｜ 官方: View_194376EE（表为 视图(_V)；截图实测 视图(V)）
pub const MENU_TOOLS: &str = "工具(T)"; // EN: Tools ｜ 官方: Tools_FEDDD6F8（表为 工具(_T)；截图实测 工具(T)）
pub const MENU_HELP: &str = "帮助(H)"; // EN: Help ｜ 官方: Help_00BC9B98（表为 帮助(_H)；截图实测 帮助(H)）
/// 菜单项（顺序与参考图一致）。
pub const MENU_ITEMS: [&str; 5] = [MENU_FILE, MENU_EDIT, MENU_VIEW, MENU_TOOLS, MENU_HELP];
/// 菜单栏右端品牌字：`Crystal Studio`（两个词，C/S 大写）。
///
/// 2026-11-06 用户明确裁定软件显示名固定 `Crystal Studio`；官方把
/// 「Crystal Studio — Map Editor」译作 `Crystal Studio — 地图编辑器`
/// （`CrystalStudioMapEditor_AEA57DE1`）**只作历史参照**，已不作品牌口径
/// —— 上一版取「地图编辑器」的写法已按该裁定订正（见 [`TITLE_TMPL`]）。
pub const BRAND: &str = "Crystal Studio"; // EN: Crystal Studio ｜ 品牌不译（2026-11-06 裁定）

// ───────────────────── 下拉菜单项（官方 5 菜单 / 39 项，2026-11-05） ─────────────────────
//
// **权威来源（两处，互相印证）**：
// 1. `CrystalStudio/MainWindow.axaml:5` 的 `<Menu>` 树 —— 每项 `Header` 是
//    `{local:Loc <Key>}`，`<Separator />` 的位置就是官方的分组；
// 2. `layout/crystalstudio-strings.json` 里同一批 key 的 `Chinese` 字段。
//
// ⚠ **助记键取"无下划线"写法**（`新建地图(N)…` 而非 `新建地图(_N)…`）：与菜单栏
// [`MENU_FILE`] 同一条口径（截图是 `文件(F)`；下划线助记键在自绘菜单里画不出来，
// 写 `_` 只会多一个可见下划线）。省略号用 U+2026 `…`（官方原文就是它）。
//
// ⚠ **视图菜单有 4 类项不在 XAML 里**，由代码追加（见各条注释）：
//    语言（`MainWindow.Localization.cs:15`）、7 个面板开关（`DockWorkspace.cs:269`
//    `AddSectionMenu`，面板清单来自 `MainWindow.axaml.cs:338-379` 的 `docking.Add(...)`）、
//    锁定 / 重置面板布局（`MainWindow.axaml.cs:389-396`）。
//
// ⚠ **"39 项"的账**：文件 10 + 编辑 4 + 视图 18 + 工具 6 + 帮助 1 = 39。
//    视图的 18 = XAML 的 8（深色模式/单元格网格/地图光照/开启门/放大/缩小/实际大小/适应地图）
//    + 语言 1 + 面板开关 7 + 锁定/重置 2。面板开关是 **7 个**（含「当前资源」），
//    不是 6 个 —— 官方 `docking.Add("Active Asset", …)` 同样会被 `AddSectionMenu` 生成开关项。

/// 文件 → 新建地图。
pub const MENU_NEW_MAP: &str = "新建地图(N)…"; // EN: _New Map… ｜ 官方: NewMap_8D5B167F
/// 文件 → 打开地图。
pub const MENU_OPEN_MAP: &str = "打开地图(O)…"; // EN: _Open Map… ｜ 官方: OpenMap_2171584A
/// 文件 → 最近打开（官方是子菜单，本仓灰显）。
pub const MENU_OPEN_RECENT: &str = "最近打开(R)"; // EN: Open _Recent ｜ 官方: OpenRecent_A86BC9C7
/// 文件 → 保存地图。
pub const MENU_SAVE_MAP: &str = "保存地图(S)"; // EN: _Save Map ｜ 官方: SaveMap_FFF39C02
/// 文件 → 另存为。
pub const MENU_SAVE_AS: &str = "另存为(A)…"; // EN: Save _As… ｜ 官方: SaveAs_04C37C25
/// 文件 → 导出小地图。
pub const MENU_EXPORT_MINIMAP: &str = "导出小地图(M)…"; // EN: Export _Minimap… ｜ 官方: ExportMinimap_C5960A2C
/// 文件 → 新建物体（官方 `New _Stamp`，中文是「新建物体」不是「新建印章」）。
pub const MENU_NEW_STAMP: &str = "新建物体(S)"; // EN: New _Stamp ｜ 官方: NewStamp_19F4EC8E
/// 文件 → 导入物体。
pub const MENU_IMPORT_STAMP: &str = "导入物体(I)…"; // EN: _Import Stamp… ｜ 官方: ImportStamp_164EFF44
/// 文件 → 导出选区。
pub const MENU_EXPORT_SELECTION: &str = "导出选区(E)…"; // EN: _Export Selection… ｜ 官方: ExportSelection_EEC5E2D3
/// 文件 → 退出。
pub const MENU_EXIT: &str = "退出(X)"; // EN: E_xit ｜ 官方: Exit_FF8B758C

/// 编辑 → 撤销。
pub const MENU_UNDO: &str = "撤销(U)"; // EN: _Undo ｜ 官方: Undo_0846D4AA
/// 编辑 → 重做。
pub const MENU_REDO: &str = "重做(R)"; // EN: _Redo ｜ 官方: Redo_47994261
/// 编辑 → 填充选区。
pub const MENU_FILL_SELECTION: &str = "填充选区(F)"; // EN: _Fill Selection ｜ 官方: FillSelection_986D5A86
/// 编辑 → 设置。
pub const MENU_SETTINGS: &str = "设置(S)…"; // EN: _Settings… ｜ 官方: Settings_9A14C9A0

/// 视图 → 深色模式（官方是 CheckBox 项）。
pub const MENU_DARK_MODE: &str = "深色模式(D)"; // EN: _Dark Mode ｜ 官方: DarkMode_15AAE2DB
/// 视图 → 单元格网格（官方是 CheckBox，默认勾选）。
pub const MENU_CELL_GRID: &str = "单元格网格(G)"; // EN: Cell _Grid ｜ 官方: CellGrid_1DB84E6F
/// 视图 → 地图光照（官方是子菜单：黎明/白天/夜晚）。
pub const MENU_MAP_LIGHTING: &str = "地图光照(L)"; // EN: Map _Lighting ｜ 官方: MapLighting_C3B7695B
/// 视图 → 开启门（官方是 CheckBox）。
pub const MENU_OPEN_DOORS: &str = "开启门(O)"; // EN: _Open Doors ｜ 官方: OpenDoors_B2088C97
/// 视图 → 放大。
pub const MENU_ZOOM_IN: &str = "放大(I)"; // EN: Zoom _In ｜ 官方: ZoomIn_BE6B96AF
/// 视图 → 缩小。
pub const MENU_ZOOM_OUT: &str = "缩小(O)"; // EN: Zoom _Out ｜ 官方: ZoomOut_8ABEB91F
/// 视图 → 实际大小（100%）。
pub const MENU_ACTUAL_SIZE: &str = "实际大小（100%）(A)"; // EN: _Actual Size (100%) ｜ 官方: ActualSize100_2CE765D4
/// 视图 → 适应地图。
pub const MENU_FIT_MAP: &str = "适应地图(F)"; // EN: _Fit Map ｜ 官方: FitMap_50B1C6FC
/// 视图 → 语言（重启后生效）。
pub const MENU_LANGUAGE: &str = "语言（重启后生效）"; // EN: Language (restart required) ｜ 官方: Languagerestartrequired_01562EF9
/// 视图 → 锁定面板布局。
pub const MENU_LOCK_LAYOUT: &str = "锁定面板布局"; // EN: Lock Panel Layout ｜ 官方: LockPanelLayout_457B6D69
/// 视图 → 重置面板布局。
pub const MENU_RESET_LAYOUT: &str = "重置面板布局"; // EN: Reset Panel Layout ｜ 官方: ResetPanelLayout_B670D664

/// 工具 → 生成城镇。
pub const MENU_GEN_TOWN: &str = "生成城镇(T)…"; // EN: Generate _Town… ｜ 官方: GenerateTown_3A2F6C86
/// 工具 → 生成地形。
pub const MENU_GEN_TERRAIN: &str = "生成地形(G)…"; // EN: _Generate Terrain… ｜ 官方: GenerateTerrain_DDD75B1E
/// 工具 → 生成省域。
pub const MENU_GEN_PROVINCE: &str = "生成省域(P)…"; // EN: Generate _Province… ｜ 官方: GenerateProvince_9C639A45
/// 工具 → 生成地牢。
pub const MENU_GEN_DUNGEON: &str = "生成地牢(D)…"; // EN: Generate _Dungeon… ｜ 官方: GenerateDungeon_BE5D20EE
/// 工具 → 识别物体。
pub const MENU_IDENTIFY_OBJECTS: &str = "识别物体(I)…"; // EN: _Identify Objects… ｜ 官方: IdentifyObjects_E08CB638
/// 工具 → 提取地图物体。
pub const MENU_EXTRACT_OBJECTS: &str = "提取地图物体"; // EN: Extract Map Objects ｜ 官方: ExtractMapObjects_1151B2C9

/// 帮助 → 键盘快捷键。
pub const MENU_KEYBOARD_SHORTCUTS: &str = "键盘快捷键(S)…"; // EN: Keyboard _Shortcuts… ｜ 官方: KeyboardShortcuts_873E9855

/// **"未实现"的灰显原因模板** —— 点击灰显项时整句进状态栏。
///
/// 前缀「未实现：」是任务书 §三.2 的硬要求：用户点了一个点不动的项，
/// 必须**立刻看到原因**，而不是"点了没反应"。
pub const STATUS_MENU_TODO_TMPL: &str = "未实现：{why}"; // EN: Not implemented: {why}
/// **"需要先开图"的灰显原因模板**（没有打开地图时点这些项）。
pub const STATUS_MENU_NEED_MAP_TMPL: &str = "需要先开图：{why}"; // EN: Open a map first: {why}
/// 「需要先开图」那句 `{why}` —— 统一一句话，点名去哪里开。
pub const WHY_NEED_MAP: &str = "当前没有打开地图（文件 → 打开地图…）"; // EN: no map is open (File → Open Map…)

// ── 菜单动作的回执（状态栏「上一次操作」那一段） ──
/// 打开地图成功。
pub const MENU_OPEN_OK_TMPL: &str = "已打开地图：{name}"; // EN: Opened map: {name}
/// 打开地图失败（原因原样带出）。
pub const MENU_OPEN_FAIL_TMPL: &str = "打开失败：{why}"; // EN: Open failed: {why}
/// 保存地图成功。
pub const MENU_SAVE_OK_TMPL: &str = "已保存地图：{name}（{bytes} 字节）"; // EN: Saved map: {name} ({bytes} bytes)
/// 保存时没有修改（**一个字节都没写**）。
pub const MENU_SAVE_CLEAN: &str = "没有未保存的修改，未写盘"; // EN: Nothing to save
/// 保存失败（原因原样带出）。
pub const MENU_SAVE_FAIL_TMPL: &str = "保存失败：{why}"; // EN: Save failed: {why}
/// **另存为**成功：新路径 + 字节数（2026-11-05 接线）。
///
/// 为什么把「文档路径已改为新路径」写进回执：另存为与「保存地图」的**唯一区别**就是
/// 它连文档路径一起换了（之后 `Ctrl+S` 写的是新文件）。这句话不说，用户会以为
/// 自己只是"复制了一份"，接着按 `Ctrl+S` 把改动写回原文件 —— 那是真实的数据风险。
pub const MENU_SAVE_AS_OK_TMPL: &str = "已另存为：{path}（{bytes} 字节），文档路径已改为新路径"; // EN: Saved as: {path} ({bytes} bytes); document path now points at it ｜ 官方: SaveAs_04C37C25
/// **另存为**失败（原因原样带出；原文件与文档路径都不动）。
pub const MENU_SAVE_AS_FAIL_TMPL: &str = "另存为失败：{why}"; // EN: Save As failed: {why} ｜ 官方: SaveAs_04C37C25
/// **导出选区**成功（2026-11-05 接线）：路径 + 尺寸 + 档位 + 字节数。
///
/// 为什么把「尺寸 / 档位 / 分块数」都写进回执：导出的图**不一定**等于选区的
/// `格数×48 × 格数×32`（分块拼接与档位都会影响最终像素数），不说清用户没法核对。
pub const MENU_EXPORT_SEL_OK_TMPL: &str = "已导出选区：{path}（{w}×{h} px，{bytes} 字节，档位 {scale}）"; // EN: Exported selection: {path} ({w}×{h} px, {bytes} bytes, scale {scale}) ｜ 官方: ExportSelection_EEC5E2D3
/// **导出小地图**成功：路径 + 尺寸 + 档位 + 分块数。
pub const MENU_EXPORT_MINI_OK_TMPL: &str = "已导出小地图：{path}（{w}×{h} px，{bytes} 字节，档位 {scale}，{tiles} 块）"; // EN: Exported minimap: {path} ({w}×{h} px, {bytes} bytes, scale {scale}, {tiles} tiles) ｜ 官方: ExportMinimap_C5960A2C
/// **导出**失败（原因原样带出；目标路径上**一个文件都不会多**）。
pub const MENU_EXPORT_FAIL_TMPL: &str = "导出失败：{why}"; // EN: Export failed: {why}
/// 撤销成功。
pub const MENU_UNDO_OK_TMPL: &str = "已撤销 {n} 步"; // EN: Undid {n} step(s)
/// 没有可撤销的步骤。
pub const MENU_UNDO_NONE: &str = "没有可撤销的步骤"; // EN: Nothing to undo
/// 重做成功。
pub const MENU_REDO_OK_TMPL: &str = "已重做 {n} 步"; // EN: Redid {n} step(s)
/// 没有可重做的步骤。
pub const MENU_REDO_NONE: &str = "没有可重做的步骤"; // EN: Nothing to redo
/// 缩放回执。
pub const MENU_ZOOM_TMPL: &str = "缩放 {pct}%"; // EN: Zoom {pct}%
/// 「实际大小」回执。
pub const MENU_ACTUAL_SIZE_OK: &str = "缩放 100%（实际大小）"; // EN: Zoom 100% (actual size)
/// 「适应地图」回执。
pub const MENU_FIT_OK_TMPL: &str = "适应地图：缩放 {pct}%"; // EN: Fit map: zoom {pct}%
/// 面板隐藏回执。
pub const MENU_PANEL_HIDE_TMPL: &str = "已隐藏面板：{name}"; // EN: Hid panel: {name}
/// 面板显示回执。
pub const MENU_PANEL_SHOW_TMPL: &str = "已显示面板：{name}"; // EN: Showed panel: {name}
/// 深色模式打开回执。
pub const MENU_DARK_ON: &str = "深色模式：开（已写入设置文件）"; // EN: Dark mode: on (saved)
/// 深色模式关闭回执。
pub const MENU_DARK_OFF: &str = "深色模式：关（已写入设置文件）"; // EN: Dark mode: off (saved)
/// 单元格网格打开回执（`视图 → 单元格网格`，官方手势 `G`）。
///
/// 为什么带上「48×32」：网格的几何只有这一组数（[`crate::metrics::CELL_W`] /
/// [`crate::metrics::CELL_H`]），回执里写出来，用户点完就知道自己看到的是什么间距的线。
pub const MENU_CELL_GRID_ON: &str = "单元格网格：开（世界格 48×32）"; // EN: Cell grid: on (48×32)
/// 单元格网格关闭回执。
pub const MENU_CELL_GRID_OFF: &str = "单元格网格：关"; // EN: Cell grid: off

/// 开启门打开回执（`视图 → 开启门`）。
///
/// 为什么要说"按门开帧偏移重渲前景层"：门开合**只动前景层的帧号**
/// （`帧号 += door_offset & 0x7F`，见 `miredit-render` 的 `RenderOptions` 文档），
/// 回执里点明这一句，用户点完就知道"变的是哪一层、怎么变的"。
pub const MENU_OPEN_DOORS_ON: &str = "开启门：开（前景层按门开帧偏移重渲）"; // EN: Open doors: on
/// 开启门关闭回执。
pub const MENU_OPEN_DOORS_OFF: &str = "开启门：关（前景层回到闭门帧）"; // EN: Open doors: off

/// **地图光照**切档回执（2026-11-05 接线）：`视图 → 地图光照` 三个复选子项各一条。
///
/// 为什么回执只写档位、不写"重渲成功"：切档必然触发一次真重渲（光照进
/// [`crate::canvas_view::CanvasView::ensure`] 的缓存键），画面变化是用户自己看得见的；
/// 而"当前是哪一档"必须有一句可机器抓的文字（`--menu-click` 的诊断行里带的就是它）。
pub const MENU_LIGHT_DAWN_DUSK: &str = "地图光照：黎明 / 黄昏"; // EN: Map lighting: Dawn / Dusk
pub const MENU_LIGHT_DAY: &str = "地图光照：白天"; // EN: Map lighting: Day
pub const MENU_LIGHT_NIGHT: &str = "地图光照：夜晚"; // EN: Map lighting: Night
/// 「地图光照」是**子菜单容器**（官方 `MapLightingMenu` 无 `Click`）⇒ 直接派发父项时
/// 不执行任何动作，只说清去哪选档位（与「最近打开」容器同一条口径）。
pub const STATUS_LIGHT_CONTAINER: &str = "「地图光照」是子菜单：请从右侧列表选一档"; // EN: map lighting is a submenu

// ── 灰显原因（每条都写"为什么"与"要什么才能做"） ──
/// 新建地图的原因。
pub const WHY_NEW_MAP: &str = "新建要先选九种磁盘格式之一与尺寸，本仓还没有格式选择器";
// ⚠ 上一版这里有一条「最近打开」的灰显原因（原文：「最近打开要在设置文件里维护
// 最近文件列表，本仓只持久化深色偏好」）。2026-11-05「最近打开」接线后两句话都不成立：
// 设置文件**已经**存 `recent`，那一项也已是 `State::Ready`（点击走 `Click::RunRecent`
// ⇒ `action::open_recent_index`）⇒ 那条常量已无任何引用，删掉，不留一句会误导后来人的
// 死文案（本条注释也刻意不再写出它的名字，免得 `grep` 还能搜到一个已不存在的常量）。
/// 新建物体的原因。
pub const WHY_NEW_STAMP: &str = "新建物体要写 .X 帧文件，本仓只读 .X（mir2-stamp）";
/// 导入物体的原因。
pub const WHY_IMPORT_STAMP: &str = "导入物体要往 .wil/.wzl 里追加帧，本仓没有写库能力";
/// 退出的原因。
pub const WHY_EXIT: &str = "菜单里的退出未接线，请用标题栏 × 或系统关窗（两者都走未保存拦截）";
/// 设置的原因。
pub const WHY_SETTINGS: &str = "设置面板未做，本仓设置文件里只有深色偏好一项";
// ⚠ 上一版这里有一条「地图光照」的灰显原因，它把官方机制说成了「按 Crystal 光照表
// 给每个格子换颜色」，并断言渲染库没有对应能力。2026-11-05 接线后这两点都被证伪：
// 官方真实机制是「全屏黑色遮罩 + 光罩贴图逐格减 alpha」，`miredit-render` 已实现
// `MapLighting` 三档（`RenderOptions::lighting`，三张光罩 PNG 内嵌在 render crate 的
// `assets/lights/`）⇒ 「地图光照」父项与 3 个子项都已是 `State::Ready`。
// 那条常量已无任何引用，连同 `ALL` 与守卫清单里的条目一起删掉，不留死文案
// （本条注释刻意不照抄旧原文，免得验收 grep 又命中一句早已作废的话）。
// ⚠ 这里原先有一条「开启门」的灰显原因，说的是"渲染库拿不到门的开合态、要按开门帧重渲"。
// 2026-11-05 接线后**两句话都是假的**：`miredit-render` 已实现门开帧偏移
// （`RenderOptions::doors_open`，见该类型文档 §三/§四），门的开合字段就在
// `mir2-map/src/cell.rs`（`door_index` / `door_offset`）⇒ 这一项已是 `State::Ready`。
// 那条常量已无任何引用，删掉，不留一句会误导后来人的死文案
// （本条注释也刻意不再写出它的名字，免得 `grep` 还能搜到一个已不存在的常量）。
/// 语言的原因。
pub const WHY_LANGUAGE: &str = "语言切换要把全表译成英文并重启，本仓文案只有中文一版";
/// 锁定面板布局的原因。
pub const WHY_LOCK_LAYOUT: &str = "本仓面板位置是固定几何（layout.rs），没有可拖的停靠系统可锁";
/// 重置面板布局的原因。
pub const WHY_RESET_LAYOUT: &str = "本仓没有停靠系统保存的布局，面板位置来自常量表";
/// 生成城镇的原因。
pub const WHY_GEN_TOWN: &str = "城镇生成器要按官方 TownStamp 规则铺印章，本仓没有生成器";
/// 生成地形的原因。
pub const WHY_GEN_TERRAIN: &str = "地形生成器（噪声/侵蚀）未实现";
/// 生成省域的原因。
pub const WHY_GEN_PROVINCE: &str = "省域生成器（分区与边界）未实现";
/// 生成地牢的原因。
pub const WHY_GEN_DUNGEON: &str = "地牢生成器（房间/走廊）未实现";
/// 识别物体的原因。
pub const WHY_IDENTIFY_OBJECTS: &str = "识别物体要用模板库匹配画布印章，本仓没有模板库";
/// 提取地图物体的原因。
pub const WHY_EXTRACT_OBJECTS: &str = "提取物体要把画布上的印章登记回资源库，本仓没有写库能力";

// ───────────────────────── 菜单下拉弹层 ─────────────────────────
//
// ⚠ **勾选标记不是文字**：`✓` U+2713 在系统字体 `Deng.ttf` 里**没有字形**
// （字形自测实测缺字），`√` 之类的替代字符又和官方观感差得远。
// 所以勾选标记由 [`crate::menu::popup`] 用**两段折线**画出来（与 `panel.rs` 的
// 折叠箭头同一套做法：矢量绘制，不依赖字体）。

// ───────────────────────── 键盘快捷键浮层（帮助 → 键盘快捷键） ─────────────────────────
//
// **只列真实绑定**：本表与 `app.rs::handle_keys` / `selection_ops` / `stamp_place` 的实现一一对应，
// 没接的官方键（如 `G` 单元格网格）**不列**。

/// 浮层标题。
pub const SHORTCUTS_TITLE: &str = "键盘快捷键"; // EN: Keyboard Shortcuts ｜ 官方: KeyboardShortcuts_873E9855
/// 浮层说明（一句话交代"只列真实绑定"）。
pub const SHORTCUTS_NOTE: &str = "只列当前真实生效的按键（Esc 关闭）"; // EN: Only keys that really work here (Esc closes)
/// 分组：文件。
pub const SC_GROUP_FILE: &str = "文件"; // EN: File
/// 分组：编辑。
pub const SC_GROUP_EDIT: &str = "编辑"; // EN: Edit
/// 分组：视图。
pub const SC_GROUP_VIEW: &str = "视图"; // EN: View
/// 分组：工具。
pub const SC_GROUP_TOOLS: &str = "工具"; // EN: Tools
/// 分组：选区与剪贴板。
pub const SC_GROUP_SELECTION: &str = "选区与剪贴板"; // EN: Selection and clipboard
/// 分组：鼠标。
pub const SC_GROUP_MOUSE: &str = "鼠标"; // EN: Mouse

/// 动作名：保存地图。
pub const SC_SAVE: &str = "保存地图"; // EN: Save map
/// 动作名：另存为（**2026-11-05 起真实现**：`Ctrl+Shift+S` 打开路径输入模态）。
pub const SC_SAVE_AS: &str = "另存为（输入新路径）"; // EN: Save As (type a new path) ｜ 官方: SaveAs_04C37C25
/// 动作名：打开地图（快捷键浮层里用**不带助记键**的写法，与菜单项区分）。
pub const SC_OPEN: &str = "打开地图"; // EN: Open map
/// 动作名：撤销。
pub const SC_UNDO: &str = "撤销"; // EN: Undo
/// 动作名：重做。
pub const SC_REDO: &str = "重做"; // EN: Redo
/// 动作名：填充工具。
pub const SC_TOOL_FILL: &str = "填充工具"; // EN: Fill tool
/// 动作名：选择工具。
pub const SC_TOOL_SELECT: &str = "选择工具"; // EN: Select tool
/// 动作名：印章工具。
pub const SC_TOOL_STAMP: &str = "印章工具"; // EN: Stamp tool
/// 动作名：笔刷尺寸 ±2。
pub const SC_BRUSH_SIZE: &str = "笔刷尺寸 ±2（夹 3..31）"; // EN: Brush size ±2 (clamped 3..31)
/// 动作名：放大。
pub const SC_ZOOM_IN: &str = "放大一档"; // EN: Zoom in one step
/// 动作名：缩小。
pub const SC_ZOOM_OUT: &str = "缩小一档"; // EN: Zoom out one step
/// 动作名：适应地图。
pub const SC_FIT: &str = "适应地图"; // EN: Fit map
/// 动作名：复制选区。
pub const SC_COPY: &str = "复制选区"; // EN: Copy selection
/// 动作名：剪切选区。
pub const SC_CUT: &str = "剪切选区"; // EN: Cut selection
/// 动作名：粘贴到鼠标格。
pub const SC_PASTE: &str = "粘贴到鼠标所在格"; // EN: Paste at cursor cell
/// 动作名：删除选区内容。
pub const SC_DELETE: &str = "删除选区内容"; // EN: Delete selection content
/// 动作名：用当前笔刷填充选区。
pub const SC_FILL_SEL: &str = "用当前笔刷填充选区"; // EN: Fill selection with brush
/// 动作名：进入印章待放置态。
pub const SC_ARM_STAMP: &str = "进入印章待放置态"; // EN: Arm selected stamp
/// 动作名：取消 / 关闭。
pub const SC_ESC: &str = "取消选区 / 退出待放置态 / 关闭弹层"; // EN: Deselect / disarm / close
/// 动作名：按住看印章轮廓。
pub const SC_ALT_OUTLINE: &str = "按住：显示待放置印章轮廓"; // EN: Hold to preview stamp outline
/// 动作名：平移画布。
pub const SC_PAN: &str = "平移画布"; // EN: Pan canvas
/// 动作名：以鼠标为中心缩放。
pub const SC_WHEEL_ZOOM: &str = "以鼠标为中心缩放"; // EN: Zoom at cursor
/// 动作名：打开本浮层。
pub const SC_OPEN_OVERLAY: &str = "打开本浮层"; // EN: Open this overlay
/// 动作名：开 / 关单元格网格（`视图 → 单元格网格` 的官方手势 `G`）。
pub const SC_TOGGLE_GRID: &str = "单元格网格 开 / 关"; // EN: Toggle cell grid

/// 键名：`Ctrl+N`。
pub const KEY_CTRL_N: &str = "Ctrl+N"; // EN: Ctrl+N
/// 键名：`Ctrl+O`。
pub const KEY_CTRL_O: &str = "Ctrl+O"; // EN: Ctrl+O
/// 键名：`Ctrl+S`。
pub const KEY_CTRL_S: &str = "Ctrl+S"; // EN: Ctrl+S
/// 键名：`Ctrl+Shift+S`。
pub const KEY_CTRL_SHIFT_S: &str = "Ctrl+Shift+S"; // EN: Ctrl+Shift+S
/// 键名：`Ctrl+Shift+N`（官方「新建物体」的手势）。
pub const KEY_CTRL_SHIFT_N: &str = "Ctrl+Shift+N"; // EN: Ctrl+Shift+N
/// 键名：`G`（官方「单元格网格」的手势）。
pub const KEY_G: &str = "G"; // EN: G
/// 键名：`Ctrl+Z`。
pub const KEY_CTRL_Z: &str = "Ctrl+Z"; // EN: Ctrl+Z
/// 键名：`Ctrl+Shift+Z`（官方重做键）与 `Ctrl+Y`（本仓另接受）。
pub const KEY_CTRL_SHIFT_Z: &str = "Ctrl+Shift+Z / Ctrl+Y"; // EN: Ctrl+Shift+Z / Ctrl+Y
/// 键名：`Ctrl+C`。
pub const KEY_CTRL_C: &str = "Ctrl+C"; // EN: Ctrl+C
/// 键名：`Ctrl+X`。
pub const KEY_CTRL_X: &str = "Ctrl+X"; // EN: Ctrl+X
/// 键名：`Ctrl+V`。
pub const KEY_CTRL_V: &str = "Ctrl+V"; // EN: Ctrl+V
/// 键名：`Ctrl+Shift+V`。
pub const KEY_CTRL_SHIFT_V: &str = "Ctrl+Shift+V"; // EN: Ctrl+Shift+V
/// 键名：`F1`。
pub const KEY_F1: &str = "F1"; // EN: F1
/// 键名：`F`。
pub const KEY_F: &str = "F"; // EN: F
/// 键名：`M`。
pub const KEY_M: &str = "M"; // EN: M
/// 键名：`O`。
pub const KEY_O: &str = "O"; // EN: O
/// 键名：`[` / `]`。
pub const KEY_BRACKETS: &str = "[ / ]"; // EN: [ / ]
/// 键名：`Enter`。
pub const KEY_ENTER: &str = "Enter"; // EN: Enter
/// 键名：`Esc`。
pub const KEY_ESC: &str = "Esc"; // EN: Esc
/// 键名：`Delete`。
pub const KEY_DELETE: &str = "Delete"; // EN: Delete
/// 键名：按住 `Alt`。
pub const KEY_ALT: &str = "按住 Alt"; // EN: Hold Alt
/// 键名：空格 + 左键拖动。
pub const KEY_SPACE_DRAG: &str = "空格 + 左键拖动"; // EN: Space + drag
/// 键名：中键拖动。
pub const KEY_MIDDLE_DRAG: &str = "中键拖动"; // EN: Middle drag
/// 键名：滚轮。
pub const KEY_WHEEL: &str = "滚轮"; // EN: Wheel

// ───────────────────────── 打开地图模态 ─────────────────────────

/// 模态标题。
pub const OPEN_DLG_TITLE: &str = "打开地图"; // EN: Open Map ｜ 官方: OpenMap_2171584A
/// 模态说明。
pub const OPEN_DLG_HINT: &str = "输入 .map 的完整路径（回车打开，Esc 取消）"; // EN: Type the full .map path (Enter opens, Esc cancels)
/// 模态主按钮。
pub const OPEN_DLG_OK: &str = "打开"; // EN: Open
/// 模态取消按钮。
pub const OPEN_DLG_CANCEL: &str = "取消"; // EN: Cancel

// ───────────────────────── 另存为模态（2026-11-05 接线） ─────────────────────────
//
// 与「打开地图」模态**同一套几何与输入**（见 `menu::dialog`），只是标题/提示/按钮文案
// 与提交动作不同。官方 `文件 → 另存为` 弹的是一个系统文件对话框
// （`SaveAs_04C37C25` 的 key 只给了标题），本仓没有 `rfd` 之类的依赖（见 `Cargo.toml`
// 顶部注释），所以这里做的是**真输入**：把目标 `.map` 的完整路径敲进去，回车就原子写到那里。

/// 另存为模态标题。
pub const SAVE_AS_DLG_TITLE: &str = "另存为"; // EN: Save As ｜ 官方: SaveAs_04C37C25
/// 另存为模态说明（点名"原文件不动"，这是与「保存地图」的差别）。
pub const SAVE_AS_DLG_HINT: &str = "输入目标 .map 的完整路径（回车另存，Esc 取消；原文件不动）"; // EN: Type the target .map path (Enter saves there, Esc cancels; the original file stays untouched) ｜ 官方: SaveAs_04C37C25
/// 另存为模态主按钮。
pub const SAVE_AS_DLG_OK: &str = "另存为"; // EN: Save As ｜ 官方: SaveAs_04C37C25

/// 另存为：目标路径是空串（**一个字节都没写**，文档路径也没动）。
pub const SAVE_AS_ERR_EMPTY: &str = "目标路径为空（没有写盘，文档路径也没改）"; // EN: empty target path (nothing written, document path unchanged)
/// 另存为：目标路径是一个**已存在的目录**（不能当文件写）。
///
/// 判据前置到写盘之前：`MoveFileExW` 把文件覆盖到目录上必然失败，但那时已经写了一份
/// 5.9 MB 的临时文件再删掉。先说清楚更省事，也让「失败路径」的证据链更干净。
pub const SAVE_AS_ERR_IS_DIR_TMPL: &str = "目标路径是个目录，不能当文件写：{path}（原文件与文档路径都没动）"; // EN: target path is a directory, not a file: {path} (original file and document path untouched)

// ───────────────────────── 导出 PNG 模态（2026-11-05 接线） ─────────────────────────
//
// 「文件 → 导出选区(E)…」与「文件 → 导出小地图(M)…」与「另存为」**共用同一套**
// 路径输入模态（`menu::dialog` 的 `Kind`），差别只有这六条文案与提交出来的动作。
// 目标扩展名是 `.png`（PNG 编码复用 `miredit_render::png_io`，不自己写）。

/// 导出选区模态标题。
pub const EXPORT_SEL_DLG_TITLE: &str = "导出选区"; // EN: Export Selection ｜ 官方: ExportSelection_EEC5E2D3
/// 导出选区模态说明（点名「1:1、只导选区、三层全开」这三条口径）。
pub const EXPORT_SEL_DLG_HINT: &str = "输入目标 .png 的完整路径（回车导出，Esc 取消；按 1:1 只导选区，三层全开）"; // EN: Type the target .png path (Enter exports, Esc cancels; 1:1, selection only, all three layers) ｜ 官方: ExportSelection_EEC5E2D3
/// 导出选区模态主按钮。
pub const EXPORT_SEL_DLG_OK: &str = "导出"; // EN: Export ｜ 官方: ExportSelection_EEC5E2D3

/// 导出小地图模态标题。
pub const EXPORT_MINI_DLG_TITLE: &str = "导出小地图"; // EN: Export Minimap ｜ 官方: ExportMinimap_C5960A2C
/// 导出小地图模态说明（点名「整图、按档位缩放」）。
pub const EXPORT_MINI_DLG_HINT: &str = "输入目标 .png 的完整路径（回车导出，Esc 取消；整图按档位缩放）"; // EN: Type the target .png path (Enter exports, Esc cancels; whole map scaled by a render tier) ｜ 官方: ExportMinimap_C5960A2C
/// 导出小地图模态主按钮。
pub const EXPORT_MINI_DLG_OK: &str = "导出"; // EN: Export ｜ 官方: ExportMinimap_C5960A2C

/// 导出：目标路径是空串（**一个字节都没写**）。
pub const EXPORT_ERR_EMPTY: &str = "目标路径为空（没有写盘，一个文件都没多）"; // EN: empty target path (nothing written)
/// 导出：目标路径是一个**已存在的目录**（不能当文件写）。
pub const EXPORT_ERR_IS_DIR_TMPL: &str = "目标路径是个目录，不能当文件写：{path}（没有写盘，一个文件都没多）"; // EN: target path is a directory, not a file: {path} (nothing written)
/// 导出回执的**覆盖标记**（目标文件原本已存在 ⇒ 这次是覆盖）。
///
/// 本仓对「目标已存在」的处理是**覆盖并回执**，不弹二次确认：模态本身就是一次
/// 明确的人工确认（敲路径 + 回车），再问一次只会让人多点一次，而回执已经把
/// 「覆盖了哪个文件」写清楚。
pub const EXPORT_OVERWROTE: &str = "；原文件已覆盖"; // EN: (existing file overwritten)
/// 导出回执的**两段式标记**（小地图：先按档位渲，再面积平均缩到目标长边）。
///
/// 为什么要在回执里说：两段式意味着「最终像素不是 render crate 某一档的原生尺寸」，
/// 不说清的话，拿 PNG 尺寸去反推档位会算不通。
pub const EXPORT_TWO_STAGE: &str = "；已按面积平均二次缩放到目标长边（render crate 的 resize_to）"; // EN: (area-average second stage to the target long side)
/// `--minimap-max-side` 的值非法（太小 / 不是整数）。
pub const EXPORT_ERR_BAD_MAX_SIDE: &str = "minimap-max-side 必须是整数像素且不小于 {min}（收到：{got}）"; // EN: minimap-max-side must be an integer >= {min}

// ───────────────────────── 诊断行（菜单 / 设置） ─────────────────────────

/// `--menu-dump` 表头。
pub const DIAG_MENU_DUMP_HEAD: &str = "[菜单] 共 {menus} 个菜单 / {items} 项"; // EN: [menu] {menus} menus / {items} items
/// `--menu-dump` 每行。
pub const DIAG_MENU_DUMP_ROW: &str = "[菜单] {menu} #{n} {id} | {label} | {shortcut} | {state}"; // EN: [menu] row
/// `--menu-dump` 汇总。
pub const DIAG_MENU_DUMP_TAIL: &str = "[菜单] 已实现 {ready} / 需先开图 {needmap} / 未实现 {todo}"; // EN: [menu] summary
/// 菜单项被点击（**灰显项也要打**：点了没动作必须有可机器抓的记录）。
pub const DIAG_MENU_CLICK: &str = "[菜单] 点击 {id}（{label}）：{result}"; // EN: [menu] click
/// `--menu-click` 排的帧号**晚于截图帧** ⇒ 这一条不会被执行（启动时说清楚，不静默）。
///
/// 为什么单独一条而不是复用 `[参数] 无法解析`：那条的意思是"参数写错了"，
/// 而这里参数**写得完全正确**，只是永远到不了那一帧 —— 说成"无法解析"是假信息。
///
/// ⚠ 文案里**不许出现 `⇒`**（U+21D2）：系统字体 `Deng.ttf` 没有这个字形，
/// `fonts::tests::文案全部字符都有字形` 会直接红（实测过）。所以这里用逗号。
pub const DIAG_CLICK_TOO_LATE: &str = "[菜单] {id} 排在第 {frame} 帧，但截图在第 {shot} 帧，这一条不执行"; // EN: [menu] click too late
/// 另存为成功（写盘结果 + 文档路径迁移）。
pub const DIAG_SAVE_AS_OK: &str = "[另存为] 已写入 {path}（{bytes} 字节，临时文件 {tmp}）；文档路径：{old} → {path}"; // EN: [save as] wrote {path}
/// 另存为失败（原因原样带出，**原文件与文档路径都没动**）。
pub const DIAG_SAVE_AS_FAIL: &str = "[另存为] 失败：{err}"; // EN: [save as] failed
/// 另存为之后的**标题原文**（由 [`crate::screen_text::build`] 装配，与窗口标题同一个函数）。
pub const DIAG_SAVE_AS_TITLE: &str = "[另存为] 标题：{title}"; // EN: [save as] title
/// 另存为之后的**状态栏左段原文**（同上，与窗口里显示的是同一份装配结果）。
pub const DIAG_SAVE_AS_STATUS: &str = "[另存为] 状态栏：{status}"; // EN: [save as] status bar
/// **导出成功**（无窗 `--export-selection` / `--export-minimap` 的可机器抓回执）。
///
/// 字段：目标路径 / 像素尺寸 / 文件字节数 / 档位 / 分块数 / 扩边格数 / 两段式 / **门态**。
/// 为什么带 `tiles` 与 `margin`：分块拼接是本模块的**唯一**非平凡实现，
/// 取证时要能看出「这次到底分了几块、扩了几格」，而不是只看到一个 PNG 文件。
/// 为什么带 `doors`（`1` = 开门、`0` = 闭门）：这是「画布开着门、导出是闭门」那个
/// 不一致收口后的**自证字段** —— 两张导出图字节不同（`32,842` vs `32,812`）时，
/// 光看回执认不出哪张是开门的；打出来就能与画布截图直接对上。
pub const DIAG_EXPORT_OK: &str = "[导出] 已写入 {path}：{w}×{h} px / {bytes} 字节 / 档位 {scale} / {tiles} 块 / 扩边 {margin} 格 / 两段式 {twostage} / 开门={doors} / 光照={lighting}"; // EN: [export] wrote {path}
/// **导出失败**（原因原样带出；目标路径上**一个文件都不会多**）。
pub const DIAG_EXPORT_FAIL: &str = "[导出] 失败：{err}"; // EN: [export] failed
/// 首帧几何（截图比对要用：画布 / 弹层 / 缩放浮层 / 告警黄条四块矩形 + 当前主题）。
pub const DIAG_MENU_GEOM: &str = "[几何] canvas=({cx},{cy},{cw},{ch}) popup=({px},{py},{pw},{ph}) pill=({gx},{gy},{gw},{gh}) banner=({bx},{by},{bw},{bh}) dark={dark}"; // EN: [geom] rects + theme
/// 设置文件写入成功。
pub const DIAG_SETTINGS_SAVED: &str = "[设置] 已写入 {path}：{json}"; // EN: [settings] saved
/// 设置文件读到了。
pub const DIAG_SETTINGS_LOADED: &str = "[设置] 读到 {path}：dark={dark}"; // EN: [settings] loaded
/// 设置文件读失败（**回落浅色，不崩**）。
pub const DIAG_SETTINGS_FALLBACK: &str = "[设置] {path} 读失败（回落浅色）：{err}"; // EN: [settings] fallback
/// 状态取值名（弹层/`--menu-dump` 共用）：已实现。
pub const STATE_READY: &str = "已实现"; // EN: implemented
/// 状态取值名：需要先开图。
pub const STATE_NEED_MAP: &str = "需要先开图"; // EN: needs a map
/// 状态取值名：未实现（后面接原因）。
pub const STATE_TODO_TMPL: &str = "未实现（{why}）"; // EN: not implemented ({why})

// ───────────────────────── 面板标题 ─────────────────────────

pub const TITLE_TOOLS: &str = "工具"; // EN: Tools ｜ 官方: Tools_EA93D6A2
pub const TITLE_TOOL_OPTIONS: &str = "工具选项"; // EN: Tool Options ｜ 官方: ToolOptions_9F7BE263
pub const TITLE_LAYERS: &str = "图层"; // EN: Layers ｜ 官方: Layers_0BCD66E6
pub const TITLE_CELL_INSPECTOR: &str = "单元格检查器"; // EN: Cell Inspector ｜ 官方: CellInspector_3FFA821E（旧称「单元格信息」）
pub const TITLE_MINIMAP: &str = "小地图"; // EN: Minimap ｜ 官方: Minimap_4BAD6CFF
pub const TITLE_PALETTE: &str = "资源面板"; // EN: Palette ｜ 官方: Palette_85794EFF（旧称「素材面板」）
pub const TITLE_ACTIVE_ASSET: &str = "当前资源"; // EN: Active Asset ｜ 官方: ActiveAsset_BF34816C（旧称「当前素材」）

/// **地图列表面板**标题（2026-11-05，上游 `901dbe2f` 新增面板）。
///
/// ⚠ 上游面板名 `"Maps"` 来自 `MainWindow.axaml.cs:343` 的 `docking.Add("Maps", …)`，
/// **不在** `Strings.json` 里（那批 `LocalText.T("…")` 用的字面量键都没进译文表）
/// ⇒ 官方漏译，按本仓口径自定（与「资源面板」「单元格检查器」同一套两字+名词风格）。
pub const TITLE_MAPS: &str = "地图列表"; // EN: Maps ｜ 上游 `MainWindow.axaml.cs:343`（Strings.json 无此键 ⇒ 自定）

// ───────────────────────── 左侧：工具选项 / 图层 ─────────────────────────

/// 笔刷尺寸胶囊里的文字：**真实尺寸** `N×N`（数字不译，`×` 是 U+00D7）。
///
/// 真值：`CrystalStudio/MainWindow.axaml:6` 的 `<TextBlock x:Name="BrushSizeLabel"
/// Text="3×3" …/>`，运行期由 `MainWindow.axaml.cs:932` 按当前工具与尺寸改写：
/// `BrushSizeLabel.Text = (Tool == Bucket ? T("Fill") : (Tool == Pencil ? "1×1"
///   : $"{BrushSize}×{BrushSize}"));`
///
/// 于是胶囊有**三种**取值，本仓一一对应（见 [`crate::screen_text::pill_text`]）：
/// - 填充工具 ⇒ [`TOOL_BUCKET`]（官方就是复用 `Fill` 这一条文案）；
/// - 铅笔 ⇒ `1×1`（铅笔本单未接线，但文案口径照样留着）；
/// - 其余（笔刷/橡皮/吸管）⇒ `N×N`（N = [`crate::metrics::BRUSH_MIN`]..=[`crate::metrics::BRUSH_MAX`] 的奇数）。
pub const PILL_SIZE_TMPL: &str = "{n}×{n}"; // EN: {n}×{n} ｜ 官方: BrushSizeLabel 的运行期格式（MainWindow.axaml.cs:932）
/// 铅笔工具的胶囊文案（官方 `1×1`：铅笔永远是单格）。
pub const PILL_PENCIL: &str = "1×1"; // EN: 1×1 ｜ 官方: MainWindow.axaml.cs:932

/// 五个图层行。
///
/// ⚠ **`物体` 没有「层」字**（不是「物体层」）：官方图层行 `Objects` 的译文就是 `物体`
/// （`Objects_DE5236EE`），新版截图左侧图层面板第三行也逐字是「物体」。
/// 前两行则**带「图层」二字**（`地表图层` / `中层图层`，官方 `GroundTiles_3931E191` /
/// `MiddleTiles_E80971A2`）—— 五行不齐整是官方的样子，不替它抹平。
pub const LAYER_GROUND: &str = "地表图层"; // EN: Ground Tiles ｜ 官方: GroundTiles_3931E191（旧称「地表层」）
pub const LAYER_MIDDLE: &str = "中层图层"; // EN: Middle Tiles ｜ 官方: MiddleTiles_E80971A2（旧称「中间层」）
pub const LAYER_OBJECTS: &str = "物体"; // EN: Objects ｜ 官方: Objects_DE5236EE（旧称「物件层」；官方无「层」字）
pub const LAYER_LOW_COLLISION: &str = "低位阻挡"; // EN: Low Collisions ｜ 官方: LowCollisions_CC739D3F（旧称「低阻挡」）
pub const LAYER_HIGH_COLLISION: &str = "高位阻挡"; // EN: High Collisions ｜ 官方: HighCollisions_830FB039（旧称「高阻挡」）
/// 图层行名称（顺序 = 行序）。
pub const LAYER_NAMES: [&str; 5] = [
    LAYER_GROUND,
    LAYER_MIDDLE,
    LAYER_OBJECTS,
    LAYER_LOW_COLLISION,
    LAYER_HIGH_COLLISION,
];

/// **地表图层行的「不可落笔」标记**（九种格式里只有 Type5 / `Mir3Wemade` 会挂）。
///
/// 依据 `mir2-format/src/model.rs:430-440` 的能力位 `ground_paint_supported == false`：
/// Type5 的背景图号在**半分辨率块**里（`mir3_wemade.rs:233` 的 `bk_img` 恒 0，`write` 也不写它），
/// 逐格改 `bk_img` 会被静默丢弃 ⇒ 界面上必须先说清楚，而不是让用户画完发现没变。
///
/// ⚠ 这句话只描述**能力**，不描述"整张图不能编辑"：Type5 的中层/物体/阻挡层都照常可写。
pub const GROUND_LOCKED_MARK: &str = "不可落笔"; // EN: ground paint unsupported

/// 图层子组标题。
pub const GROUP_CELL_HIGHLIGHTS: &str = "单元格高亮"; // EN: Cell Highlights ｜ 官方: CellHighlights_E3B54000
/// 子组两个复选框标签。
pub const HIGHLIGHT_LIGHT: &str = "灯光单元格"; // EN: Light Cells ｜ 官方: LightCells_086E58AD（旧称「光源格」）
pub const HIGHLIGHT_DOOR: &str = "门单元格"; // EN: Door Cells ｜ 官方: DoorCells_E715F346（旧称「门格」）
pub const HIGHLIGHT_NAMES: [&str; 2] = [HIGHLIGHT_LIGHT, HIGHLIGHT_DOOR];

// ⚠ 上一版这里有一条 `LAYER_NOT_WIRED_HINT`（「本单未接线」），用于把低位/高位阻挡两行
// 置灰并解释"点了没反应"。**本轮已接线**（眼睛切 `CollisionOverlay`、点行切
// `EditLayer::LowCollision`/`HighCollision`）⇒ 那句话已经不成立，删掉，
// 不留一句会误导后来人的死文案（它若还在文案表里，迟早会被谁当真）。

// ───────────────────────── 左侧：单元格检查器 ─────────────────────────

/// 信息头模板：`单元格 380,310 · 3 个物体`。
///
/// 占位符：`{x}`/`{y}` 世界格坐标、`{n}` 该格有图的层数（= 物体数）。
/// 坐标逗号后不留空格，比英文省 6px。
pub const INSPECTOR_HEAD_TMPL: &str = "单元格 {x},{y} · {n} 个物体"; // EN: Cell {x}, {y} · {n} objects ｜ 官方: objects_E4CD3029（物体；旧称「物件」）

/// 键列：阻挡 / 地表 / 中层 / 物体 / 元数据 / 图块动画。
///
/// 官方词条逐条对应：`Collision_F68A6490` / `Ground_151BF493` / `Middle_D93006EC` /
/// `Objects_DE5236EE` / `Metadata_9EDDF573` / `TileAnimation_46006268`。
/// ⚠ **最后一个官方译作「地表动画」**（`TileAnimation_46006268`），本仓仍写「图块动画」：
/// 这一行显示的是**三族通用**的 `ani_frame`/`ani_tick`（地表/中层/物体都有这两个字节，
/// 见 `mir2-map/src/cell.rs`），叫「地表动画」会把它误说成只属于地表层 —— 语义错，故不改；
/// 已登记为存疑项，等用户裁定。
pub const KEY_COLLISION: &str = "阻挡"; // EN: Collision ｜ 官方: Collision_F68A6490
pub const KEY_GROUND: &str = "地表"; // EN: Ground ｜ 官方: Ground_151BF493
pub const KEY_MIDDLE: &str = "中层"; // EN: Middle ｜ 官方: Middle_D93006EC
pub const KEY_OBJECTS: &str = "物体"; // EN: Objects ｜ 官方: Objects_DE5236EE（旧称「物件」）
pub const KEY_METADATA: &str = "元数据"; // EN: Metadata ｜ 官方: Metadata_9EDDF573
pub const KEY_TILE_ANIMATION: &str = "图块动画"; // EN: Tile Animation ｜ 官方作「地表动画」TileAnimation_46006268，语义不合故保留（见上）

/// 六个键（顺序 = 行序）。
pub const INSPECTOR_KEYS: [&str; 6] = [
    KEY_COLLISION,
    KEY_GROUND,
    KEY_MIDDLE,
    KEY_OBJECTS,
    KEY_METADATA,
    KEY_TILE_ANIMATION,
];

/// 取不到的值（统一用它，**不写 0 也不写 N/A**）。
pub const VAL_UNKNOWN: &str = "—"; // EN: — (unknown)
/// 阻挡位的两种取值。
pub const VAL_YES: &str = "有"; // EN: Yes
pub const VAL_NO: &str = "无"; // EN: None

/// 值列：阻挡模板（`bk & 0x8000` / `fr & 0x8000`，见 `mir2_map::Cell::is_walkable`）。
///
/// 官方 `LowCollisions_CC739D3F` = 低位阻挡、`HighCollisions_830FB039` = 高位阻挡
/// （旧称「低阻挡 / 高阻挡」）。改后最长一行 `低位阻挡有` = 5 字，值列可用宽 81px，
/// 实测余量见本轮验收报告（不越界）。
pub const VAL_COLLISION_TMPL: &str = "低位阻挡{low} ·\n高位阻挡{high}"; // EN: Low {low} ·\nHigh {high} ｜ 官方: LowCollisions_CC739D3F / HighCollisions_830FB039

/// 值列：地表模板。占位符：`{img}` 图号（低 15 位）、`{lib}` 库号（`btTiles`）、
/// `{size}` 帧像素尺寸（形如 `96×64`，取不到即 `—`）。
///
/// ⚠ **中文实测调整（2026-11-04）**：上一版写成两行 `图号 N · 库号 M` —— 中文比英文宽，
/// 值列可用宽只有 81px（`INSPECTOR_VAL_X=75`），而 `图号 1234 · 库号 40` 实测约 96px，
/// 必被右边界切掉。按参考图"长值换行"的既有做法改成**三行**（不删字段、不改语义）：
/// 最长一行 `图号 65535` 约 55px，留足余量。
pub const VAL_GROUND_TMPL: &str = "图号 {img}\n库号 {lib}\n帧 {size}"; // EN: Sprite {img}\nLib {lib}\nFrame {size}

/// 值列：中层模板（`wMidImg` / `btsmTiles`）。
pub const VAL_MIDDLE_TMPL: &str = "图号 {img}\n库号 {lib}\n帧 {size}"; // EN: Sprite {img}\nLib {lib}\nFrame {size}

/// 值列：物体（表层）模板（`wFrImg` / `btArea`）。
///
/// ⚠ **第二行必须写「表层库号」**（审计 E2）：`btArea`（文件 off10）是**表层资源库号**，
/// 客户端 `GameScene.cpp:3468,3510` 拿它当 `GetObjs(wunit, …)` 的库号、
/// `map_loader.gd:203-208` 的 `get_front_lib_index` 也等于 off10、
/// `mir2-assets/src/lib_paths.rs:15` 同样把 `Surface ← btArea`。
/// 上一版这里写的是通用「库号」，而同一面板的「图块动画」行又把这同一个字节叫「区域」，
/// 于是**同一字节在同一面板里有两个名字、其中一个还是错的**。两个毛病本单一起修：
/// 这里点名「表层库号」，「图块动画」行不再重复显示这个字节（见下一条）。
pub const VAL_OBJECTS_TMPL: &str = "图号 {img}\n表层库号 {lib}\n帧 {size}"; // EN: Sprite {img}\nSurface lib {lib}\nFrame {size} ｜ 官方: Objects_DE5236EE（物体）

/// 值列：元数据模板。占位符：`{light}` 光照、`{door}` 门索引、`{offset}` 门偏移。
///
/// ⚠ 中文实测调整（沿自上版）：原文 `Light 0 · Door 0 · Offset 0` 单行放得下是为了配合
/// 62px 宽的英文键列；中文值列可用宽只有 81px（`INSPECTOR_VAL_X=75`），单行必被右边界裁掉，
/// 故拆成两行（**不删任何字段**）。
pub const VAL_METADATA_TMPL: &str = "光照 {light} · 门 {door}\n偏移 {offset}"; // EN: Light {light} · Door {door}\nOffset {offset} ｜ 官方: Metadata_9EDDF573

/// 值列：图块动画模板。占位符：`{frame}` 动画当前帧序号、`{tick}` 动画间隔。
///
/// ⚠ **官方译法照抄，但「帧数」语义不严谨**（任务书点名要求照改并注明）：
/// 官方 `0Frames1Tick2_DFC23F91` 把 `Frames` 译作「帧数」、`Tick` 译作「间隔」，
/// 而本行 `{frame}` 填的是 `Cell::ani_frame` = **当前帧序号**（0..n，不是帧总数），
/// 字段实为当前帧序号（`ani_frame`）。照官方写成「帧数」，**此处语义以字段为准**，
/// 不因为叫「帧数」就去编一个总数（`.map` 里没有总数，见下）。
///
/// ⚠ **第三列「区域」已删除（审计 E2）**：上一版是 `帧序 · 节拍 · 区域 {area}`（旧称），那个
/// `{area}` 取的是 `Cell::area` = 文件 off10 = **表层资源库号**（证据见 [`VAL_OBJECTS_TMPL`]），
/// 而它已经作为「物体 · 表层库号」显示过一次。同一个字节用两个名字显示两遍，
/// 其中「区域」还是错的（听起来像地图分区）。`.map` 里**没有**第三个动画字段
/// （off8 当前帧序号 / off9 间隔 / off10 表层库号，见 `mir2-map/src/cell.rs` 字段表；
/// 另三个 off6/7/11 是光照/门索引/门偏移，已在「元数据」行列过），
/// 所以按纪律**不编第三个字段**，这一条如实只留两行。
/// 取不到整个格子时两行都是 `—`。
///
/// 另：任务书 §三.6 的「帧数」（动画帧**总**数）**.map 里没有**（只有当前帧序号 `ani_frame`），
/// `mir2_assets` 也不暴露每库帧数 ⇒ 如实不给，**不编**。
pub const VAL_TILE_ANIMATION_TMPL: &str = "帧数 {frame} ·\n间隔 {tick}"; // EN: Frame {frame} ·\nTick {tick} ｜ 官方: 0Frames1Tick2_DFC23F91（帧数 / 间隔；旧称「帧序 / 节拍」）

/// 帧像素尺寸模板（如 `96×64`）。取不到帧时**不填本模板**，直接填 [`VAL_UNKNOWN`]。
pub const SIZE_TMPL: &str = "{w}×{h}"; // EN: {w} × {h}

/// 动态文案超宽被截断时补的省略号（与搜索框里的 `…` 同一个码位 U+2026）。
pub const TRUNC_SUFFIX: &str = "…"; // EN: …

// ───────────────────────── 右侧：资源面板 ─────────────────────────

/// Palette 四个页签（顺序同参考图）。
pub const TAB_LIBRARY: &str = "资源库"; // EN: Library ｜ 官方: Library_DC20B3D5（旧称「图库」；截图页签首项即「资源库」）
pub const TAB_FAVOURITES: &str = "收藏"; // EN: Favourites ｜ 官方: Favourites_D97D51D3
pub const TAB_RECENT: &str = "最近使用"; // EN: Recently Used ｜ 官方: RecentlyUsed_D61031AC
pub const TAB_CATEGORIES: &str = "分类"; // EN: Categories ｜ 官方: Categories_B8B1D894
pub const PALETTE_TABS: [&str; 4] = [TAB_LIBRARY, TAB_FAVOURITES, TAB_RECENT, TAB_CATEGORIES];

/// 搜索框占位符（末尾省略号用 `…` U+2026，比三个句点窄，避免顶到右侧清除按钮）。
pub const SEARCH_HINT: &str = "搜索组合或图片编号…"; // EN: Search Pattern or Sprite ID… ｜ 官方: SearchPatternorSpriteID_D92EE399（旧称「搜索图块编号…」）

/// 空态标题。
pub const EMPTY_TITLE: &str = "资源配置"; // EN: Your Assets Live Here ｜ 官方: YourAssetsLiveHere_E5E1BE63（旧称「素材显示在这里」）
/// 空态两行说明。
///
/// 官方是一条**含换行**的词条 `ChooseyourCrystallibraryfolderonceItwillloadautomaticallynexttime_266906D0`
/// = `选择 Crystal 资源目录后，` ⏎ `下次启动会自动加载。`
/// —— 本仓按行拆成两个常量（断行位置与官方逐字一致，只拆行、不删字）。
pub const EMPTY_DESC_0: &str = "选择 Crystal 资源目录后，"; // EN: Choose your Crystal library folder once. ｜ 官方: ChooseyourCrystallibraryfolderonceItwillloadautomaticallynexttime_266906D0 第 1 行
pub const EMPTY_DESC_1: &str = "下次启动会自动加载。"; // EN: It will load automatically next time. ｜ 官方: 同上，第 2 行
pub const EMPTY_DESC: [&str; 2] = [EMPTY_DESC_0, EMPTY_DESC_1];
/// 空态主按钮。
pub const EMPTY_ACTION: &str = "设置资源目录"; // EN: Set Library Folder ｜ 官方: SetLibraryFolder_1E671E37（旧称「设置素材目录」）
/// Palette 分页行文字模板（`{n}` = 已登记资源库数；装载失败时为 0）。
pub const PAGE_LABEL_TMPL: &str = "资源库 {n} 个"; // EN: {n} libraries loaded ｜ 官方: libraries_01CFC309（资源库；旧称「素材库 {n} 个」）

// ───────────────── 右侧：资源面板 · 真实图块网格（本单新增） ─────────────────
//
// 面板从「空态」换成「真图块树」后新增的全部文案。**没有一句是占位文案**：
// 每条要么带真实数据（模板 + 运行期填充），要么是"确实没做到/确实没有"的如实说明。

/// 资源库选择行的左标签（旧称「地形组」那一行的位置，现在标的是"下面是资源库选择"）。
pub const PALETTE_LIB_LABEL: &str = "资源库"; // EN: Library ｜ 官方: library_B718F135（旧称「图库」）
/// 当前库的帧总数（右上角，与 `PALETTE_LIB_LABEL` 同一行）。
///
/// ⚠ **官方漏译，按截图自定**：新版截图这一行右侧是英文 `0 sets`（`sets_82C6DB3D` 译作「组合」，
/// 但运行态没接上译文）；且本仓这一格显示的是**帧数**不是「组合数」，语义不同 ⇒
/// 不照抄 `sets`，沿用本仓既有的「帧 {n}」（无禁用旧词，语义准确）。
pub const PALETTE_FRAMES_TMPL: &str = "帧 {n}"; // EN: {n} frames ｜ 官方漏译，按截图自定（截图该格为英文 `0 sets`）
/// 库下拉里的一行：文件基名 + 所属层（如 `Tiles · 地表图层`）。
pub const PALETTE_LIB_ROW_TMPL: &str = "{stem} · {fam}"; // EN: {stem} · {fam}
/// 分页行中间的可见帧区间（`1-48 / 共 31775 帧`）。
///
/// ⚠ **`{a}`/`{b}` 是 1 基「图号」**（审计 E1 统一口径）：本面板所有给人看的数字
/// 都是**图号**（= 0 基帧索引 + 1），与左侧「单元格检查器」的「图号」、
/// 地图格 `bk_img`、客户端 `map_loader.gd:51-52` 的 1 基帧号同义。
/// `{n}` 仍是该库帧总数（= 最大图号），所以区间永远落在 `1..={n}` 里。
pub const PALETTE_RANGE_TMPL: &str = "{a}-{b} / 共 {n} 帧"; // EN: {a}-{b} of {n} frames
/// 空帧计数（`.wix/.wzx` 偏移为 0 的帧——图集 meta 里同样记作 `empty_frames`）。
pub const PALETTE_EMPTY_FRAMES_TMPL: &str = "空帧 {n}"; // EN: {n} empty
/// 该库打不开 / 帧数为 0：**如实说没有**，不画占位缩略图。
pub const PALETTE_LIB_EMPTY: &str = "该库没有可用帧"; // EN: this library has no frames
/// 收藏 / 最近使用 / 分类三个页签的占位说明（本单**不做**，页签保留但点开只说这句）。
pub const PALETTE_TAB_UNIMPLEMENTED: &str = "该页签未实现（本单只做资源库页）"; // EN: not implemented
/// 搜索输入的不是纯数字。
pub const PALETTE_SEARCH_NEED_DIGITS: &str = "输入图号（纯数字）"; // EN: enter a numeric sprite id
/// 搜索的图号超出该库范围（`{max}` = 该库最大图号 = 帧总数；合法区间 `1-{max}`）。
///
/// ⚠ 区间是 `1-{max}` 而不是 `0-{max-1}`：搜索框收的是**图号（1 基）**（审计 E1）。
pub const PALETTE_SEARCH_OUT_OF_RANGE_TMPL: &str = "图号超出范围（1-{max}）"; // EN: out of range (1-{max})
/// 搜索命中并已跳转。
pub const PALETTE_SEARCH_JUMP_TMPL: &str = "已跳到图号 {n}"; // EN: jumped to sprite {n}
/// 图号超过地图格 15 位（三个图号字段的最高位都另有语义）能存的范围。
///
/// ⚠ **本版起不再有「只能从地面库选笔刷」这条**：三族（地面/中层/表层）都能设成笔刷，
/// 写哪个字段由**当前编辑层**决定；族与图层不一致时由 [`crate::edit`] 在**落笔那一刻**拒绝
/// 并在状态栏给出 [`EDIT_LAYER_MISMATCH_TMPL`] 的原因。旧的 `PALETTE_NEED_GROUND`
/// 那句"只能从地面库（Tiles）选笔刷"在本版已是错话，已删除而不是留着误导。
pub const PALETTE_FRAME_TOO_BIG: &str = "图号超过 32767，地图格里存不下"; // EN: sprite id too large

// ───────────────── 右侧：资源面板 ·「分类」页签 = 印章浏览器（本单新增） ─────────────────
//
// 印章 = `CrystalStudio/Objects/**/*.X`（`catalogue/palette.json` 登记 9,159 枚，
// 磁盘上另有 11 枚**未登记**）。数据全部来自 `mir2_stamp::StampCatalog`，本文件只管措辞。
//
// ## 术语：本页签统一叫「印章」，官方译名逐条留档
//
// 官方 `CrystalStudio/Localization/Strings.json` 把 `Stamp` 译作**「物体模板」**
// （`Stamp_2910B6D7`）、复数 `stamps` 译作「物体」（`stamps_48306FD0`）；
// 而本仓 `mir2-stamp` 全库、任务书、以及本面板的既有注释都用**「印章」**。
// ⇒ 为免同一个东西在界面上有两个名字，本页签的**新**文案统一用「印章」；
// 能逐字复用官方译文的就直接复用（如「全部分类」「第 N 页，共 M 页」「没有匹配的…」），
// 其余在 `官方:` 注释里留档官方原文与译法。
//
// ## 分类名不在这里
//
// 19 个 kind 的中文名（地形 / 崖壁 / 墙体…）与「未分类」一条来自
// `mir2_stamp::kind_label_zh`（20 条实测对照表），**不在本文件重抄一份** ——
// 抄一份就是两套真值，上游加分类时会悄悄对不上。

/// 「分类」页签：分类下拉行左标签。
pub const STAMP_KIND_LABEL: &str = "印章分类"; // EN: Stamp categories ｜ 官方: Stampcategories_564919CA（物体分类）
/// 分类下拉第一项（不过滤）。
pub const STAMP_KIND_ALL: &str = "全部分类"; // EN: All categories ｜ 官方: Allcategories_9D5097A8（逐字一致）
/// 分类下拉末项的「未登记」分组（磁盘上有、`palette.json` 没登记的 `.X`）。
///
/// 官方没有"未登记文件"这个概念（`ObjectsFolderNotFound` 是"目录不存在"，语义不同），
/// 故本仓自定；分组里**只显示文件名**，绝不按名字猜分类。
pub const STAMP_UNLISTED: &str = "未登记"; // EN: unlisted ｜ 官方: 无（官方无此概念）
/// 「未登记」分组上方那一行说明（条数是真实文件数）。
pub const STAMP_UNLISTED_NOTE_TMPL: &str = "未登记文件 {n} 个（只有文件名，不猜分类）"; // EN: {n} unlisted files ｜ 官方: 无
/// 分类下拉每行右侧的条数。
pub const STAMP_KIND_COUNT_TMPL: &str = "{n} 枚"; // EN: {n} ｜ 官方: 无（官方只给 `0 sets` 这种集合数）
/// 标签行右侧：当前过滤条件下的印章总数。
pub const STAMP_COUNT_TMPL: &str = "印章 {n}"; // EN: {n} stamps ｜ 官方: stamps_48306FD0（物体）
/// 搜索框占位符（搜的是**名称/编号子串**，与资源库页"搜图号"不同，措辞必须分开）。
pub const STAMP_SEARCH_HINT: &str = "搜索印章名称或编号…"; // EN: Search stamps by name or id… ｜ 官方: SearchStampsandCategories_A2AEEB6A（搜索物体和分类…）/ SearchStampsbyName_2D95796D
/// 分页行：`第 N 页，共 M 页 · 共 K 枚`。
///
/// `第 {page} 页，共 {pages} 页` 逐字取自官方 `Page0of1_DA914A2A`（`第 {0} 页，共 {1} 页`）；
/// `· 共 {total} 枚` 是本仓补的（官方没有"总数"这一条），量词「枚」按任务书 §三.2。
pub const STAMP_PAGE_TMPL: &str = "第 {page} 页，共 {pages} 页 · 共 {total} 枚"; // EN: Page {page} of {pages} · {total} stamps ｜ 官方: Page0of1_DA914A2A
/// 当前过滤条件下一条都没有。
pub const STAMP_NO_MATCH: &str = "没有匹配的印章"; // EN: No Matching Stamps ｜ 官方: NoMatchingStamps_0A129F82（没有匹配的物体）
/// 目录正在后台装载（**首帧不等它**，装载完再刷新列表）。
pub const STAMP_LOADING: &str = "正在加载印章目录…"; // EN: Loading stamps… ｜ 官方: Loadingstamp_BA36CAE0（正在加载物体…）
/// 目录装载失败（`{err}` = `CatalogError` 的原文，同时写 stderr）。
pub const STAMP_LOAD_FAIL_TMPL: &str = "印章目录加载失败：{err}"; // EN: Failed to load the stamp catalogue: {err} ｜ 官方: Unabletoreadcategories_4E57EE35（无法读取分类）
/// 后台装载线程没回传任何结果就退了（**不许当成"空目录"**）。
pub const STAMP_THREAD_LOST: &str = "后台装载线程异常退出（没有回传结果）"; // EN: loader thread died ｜ 官方: 无
/// 找不到 `palette.json`（探针/诊断用；界面上归入 [`STAMP_LOAD_FAIL_TMPL`]）。
pub const STAMP_CATALOG_MISSING_TMPL: &str = "未找到印章目录索引：{path}"; // EN: stamp catalogue not found: {path} ｜ 官方: NoobjectcataloguefoundLoadasavedbrushlisttogetstarted_A315EBAA（未找到物体目录…）
/// 格子里「缺帧」占位（灰底 + 这两个字；**绝不画洋红、绝不假装有图**）。
///
/// 判据见任务书 §三.5：`StampImage::missing > 0` 或 `cells_drawn == 0`。
/// 官方的 `NoForegroundImages_269B48CB`（没有前景图）是**另一件事**（那枚印章本来就没有物件层），
/// 不借用。
pub const STAMP_MISSING: &str = "缺帧"; // EN: missing ｜ 官方: 无（语义不同的那条不借用）
/// 图库没装载 ⇒ 缩略图无从生成（**这不是"缺帧"**，必须分开说）。
pub const STAMP_NO_SOURCE: &str = "资源库未装载，缩略图不可用"; // EN: libraries not loaded — no thumbnails ｜ 官方: Librariesnotloaded_58768669
/// 「当前资源」面板：选中一枚印章时的第一行（名称 · 中文分类）。
pub const STAMP_AA_TMPL: &str = "{name} · {kind}"; // EN: {name} · {kind} ｜ 官方: Stamp01cells_FC408222（物体：{0} • {1} 格；本仓拆成两行）
/// 「当前资源」面板：「分类」页签还没选中任何一枚时的提示。
///
/// 不借用资源库页那句 `选择要绘制的地形组合`（那是"设笔刷"，而本页签**不做放置**）。
pub const STAMP_AA_PROMPT: &str = "选择一枚印章查看信息"; // EN: select a stamp to inspect ｜ 官方: 无（官方此面板是"设为笔刷"，本仓不做放置）
/// 「当前资源」面板：选中一枚印章时的第二行（格数 · 包围盒）。
///
/// 包围盒是**格坐标**的闭区间 `(x0,y0,x1,y1)`，与 `mir2_stamp::Stamp::bounds` 同源。
pub const STAMP_AA_GEOM_TMPL: &str = "{n} 格 · 包围盒 ({x0},{y0},{x1},{y1})"; // EN: {n} cells · bounds ({x0},{y0},{x1},{y1}) ｜ 官方: Stamp01cells_FC408222
/// 状态栏追加的一段（选中印章后才有）。
pub const STAMP_STATUS_TMPL: &str = "印章：{name}"; // EN: Stamp: {name} ｜ 官方: Stamp_2910B6D7（物体模板）

// ───────────────────── 印章放置（2026-11-05 新增） ─────────────────────

/// 状态栏「待放置」提示：选中一枚印章并按 `Enter` 之后进入待放置态。
///
/// 为什么要把三个操作都写出来：待放置态**没有别的视觉标记**（画布上只在按住 `Alt` 时
/// 才画轮廓），若不写"点画布落位 / Esc 取消 / 按住 Alt 看轮廓"，用户会以为程序卡住了。
/// 占位符 `{name}` = 印章显示名（数据，不是文案）。
pub const STAMP_ARMED_TMPL: &str = "待放置：{name}（点画布落位 · Esc 取消 · 按住 Alt 看轮廓）"; // EN: Place {name}: click the canvas · Esc cancels · hold Alt to preview

/// 画布上跟着鼠标走的**待放置徽标**（比状态栏那句短，因为它要贴在一个格子上）。
pub const STAMP_ARMED_BADGE_TMPL: &str = "放置「{name}」"; // EN: Place "{name}"

/// 放置成功回执（任务书 §三.3 指定的句式）。
///
/// 占位符：`{name}` 印章显示名、`{n}` **真正改动的格数**（= 那一步撤销里的格数，
/// 与 `miredit_core::PlaceOutcome::Placed::cells` 同源，不是"印章有多少格"）。
pub const STAMP_PLACE_OK_TMPL: &str = "放置印章「{name}」{n} 格"; // EN: Placed stamp "{name}" ({n} cells)

/// 放置成功但有格落在地图外被逐格跳过（`{k}` = 跳过格数）。
///
/// 为什么单列一条而不是在成功回执后面拼半句：跳过是**用户必须知道**的事实
/// （印章被裁掉了一角），拼出来的句子在状态栏被截断时最容易被截掉后半截。
pub const STAMP_PLACE_SKIP_TMPL: &str = "放置印章「{name}」{n} 格（{k} 格在地图外，已跳过）"; // EN: Placed stamp "{name}" ({n} cells, {k} outside the map and skipped)

/// 放置被拒的统一前缀（原因由内核给：Type5 闸门 / 全越界 / 未注册）。
///
/// 官方拒绝文案 `Unabletoplacestamp_…`（无法放置物体）只有这一句笼统的话，
/// 本仓必须带上**具体原因**（任务书 §三.4「禁假数据：失败要显示原因」），
/// 所以只借它的句式，不借它的信息量。
pub const STAMP_PLACE_FAIL_TMPL: &str = "放置失败：{why}"; // EN: Could not place the stamp: {why} ｜ 官方: Unabletoplacestamp（无法放置物体）

/// 读不出 `.X` 文件（路径 + 系统/解析原文）。
///
/// 后半句"地图一个字节都没动"是**有保证的事实**：读盘发生在
/// `register_stamp` / `place_stamp` 之前，失败时根本没有进入内核的写路径。
pub const STAMP_READ_FAIL_TMPL: &str = "读不出印章文件 {path}：{err} —— 地图一个字节都没动"; // EN: cannot read stamp {path}: {err} — nothing was written

/// 印章目录里没有登记这一枚的文件路径（未登记项 / `resolve` 失败）。
///
/// 为什么不能猜一个路径：`Objects/**` 里有 11 个磁盘文件没进 `palette.json`，
/// 猜路径等于"放一枚用户没选的印章"，属假数据。
pub const STAMP_NO_PATH: &str = "印章 {id} 在目录里没有登记文件路径（未登记项无法放置）—— 地图一个字节都没动"; // EN: stamp {id} has no registered file path — nothing was written

// 下面这些只出现在 stderr（诊断/探针），不进画面，但照样进 [`ALL`] 做字形自检。

/// 目录装载完成的诊断行（`{ms}` 装载耗时、`{n}` 登记条数、`{unlisted}` 未登记数、`{skipped}` 跳过条数）。
pub const DIAG_STAMP_CATALOG: &str = "[印章] 目录 {path} 装载 {ms} ms / 登记 {n} 枚 / 未登记 {unlisted} 个 / 跳过 {skipped} 条"; // EN: [stamp] catalogue loaded
/// 单个分类的条数诊断行。
pub const DIAG_STAMP_KIND: &str = "[印章] 分类 {kind}（{zh}）= {n} 枚"; // EN: [stamp] kind count
/// 探针：单枚印章的渲染对账（`{same}` = 与直接调用 `render_stamp` 是否逐字节一致）。
pub const DIAG_STAMP_PROBE: &str = "[印章探针] {id} → 格 {cells} / 包围盒 {bounds} / 渲染 {w}×{h} / 画格 {drawn} / 缺帧 {missing} / 与直接渲染一致 {same}"; // EN: [stamp probe] render cross-check
/// 探针：缩略图像素统计（非全透明像素占比）。
pub const DIAG_STAMP_PIXELS: &str = "[印章探针] 像素统计 {id}：{w}×{h} 共 {total} 像素，非全透明 {opaque} 个（{pct}%）"; // EN: [stamp probe] pixel stats
/// 探针：LRU 压测结果（前进/回滚页数、纹理峰值、真渲染数、命中数、淘汰数）。
pub const DIAG_STAMP_LRU: &str = "[印章 LRU] 前进 {fwd} 页 + 回滚 {back} 页：纹理峰值 {peak}（上限 {cap}）/ 真渲染 {decodes} / 缓存命中 {hits} / 淘汰 {evictions}"; // EN: [stamp LRU] drill
/// 探针：**全库渲染清点**（9,159 枚逐枚渲染后的四类计数）。
///
/// 四类互斥且相加 = `{total}`：`{ok}` 真出图 / `{empty}` 空图（0×0）/ `{partial}` 部分缺帧 / `{err}` 读不出或解析失败。
pub const DIAG_STAMP_SWEEP: &str = "[印章清点] 共 {total} 枚：出图 {ok} / 空图 {empty} / 部分缺帧 {partial} / 读不出或解析失败 {err}（耗时 {ms} ms）"; // EN: [stamp sweep] all entries
/// 面板统计（`--stamp-report`）。
pub const DIAG_STAMP: &str = "[印章] 过滤 {filter} / 共 {total} 枚 / 第 {page}-{pages} 页 / 缓存 {cached}（峰值 {peak}，上限 {cap}）/ 真渲染 {decodes} / 命中 {hits} / 缺帧 {missing}"; // EN: [stamp] panel state
/// 目录不可用时的诊断行。
pub const DIAG_STAMP_FAIL: &str = "[印章] 目录不可用：{err}"; // EN: [stamp] catalogue unavailable
/// 选中一枚印章时的诊断行（`{x0}..{y1}` 是包围盒四元组；取不到时由调用方填 `—`）。
pub const DIAG_STAMP_PICK: &str = "[印章] 选中 {id}（{name} · {kind}）{cells} 格 包围盒 ({x0},{y0},{x1},{y1})"; // EN: [stamp] picked
/// 放置成功的诊断行（`{undo}` = 放置后的撤销栈深度，验收要看到它 +1）。
pub const DIAG_STAMP_PLACE: &str = "[印章] 放置 {id} 于 ({x},{y})：改 {n} 格 / 跳过 {k} 格 / 撤销栈 {undo}"; // EN: [stamp] placed at ({x},{y}): {n} cells changed
/// 放置被拒的诊断行（**一格未写**；原因同时进状态栏）。
pub const DIAG_STAMP_PLACE_FAIL: &str = "[印章] 放置被拒（{id} @ ({x},{y})）：{why}"; // EN: [stamp] placement refused: {why}
/// 放置**成功**时内核留下的说明（如"本格式没有该层的库号字节，已忽略"）。
///
/// 为什么只进 stderr、不进状态栏：那句话实测 88 字，拼进回执会让回执整段放不下状态栏；
/// 而它说的「本图没有库号字节」已经在「落笔」段里了（见 `app::commit_place` 的注释）。
pub const DIAG_STAMP_PLACE_NOTE: &str = "[印章] 放置说明（{id}）：{note}"; // EN: [stamp] placement note: {note}
/// 进入**待放置态**的诊断行（`{bounds}` 取不到时由调用方填 `—`）。
pub const DIAG_STAMP_ARM: &str = "[印章] 待放置 {id}（{name}）包围盒 {bounds} —— 点画布落位，Esc 取消"; // EN: [stamp] armed {id}: click the canvas to place
/// 一条**选区命令**的诊断行（`{cmd}` 是 ASCII 键，`{warn}` 1 = 拒绝，`{text}` = 回执原文）。
///
/// 为什么把回执原文也打出来：验收脚本要能 grep 到"到底给用户看了哪句话"，
/// 而不是只看状态栏截图（截图里那一句可能被截断）。
pub const DIAG_SEL_CMD: &str = "[选区] {cmd}：拒绝 {warn} / 改 {n} 格 / 撤销栈 {undo} —— {text}"; // EN: [selection] {cmd}: warn={warn} changed={n} undo={undo} — {text}
/// `--select` / `--select-copy` 的诊断行（`{clip}` 取不到时由调用方填 `—`）。
pub const DIAG_SEL_CLI: &str = "[选区] --select ({x0},{y0})..({x1},{y1})：生效 {ok} / 剪贴板 {clip}"; // EN: [selection] --select applied={ok} clipboard={clip}

// ───────────────────────── 右侧：当前资源 ─────────────────────────

/// 当前资源面板第二行（没选中任何帧时的提示）。
///
/// 官方 `Selectaterrainsettobrush_224AB93F` = `选择要绘制的地形组合`（旧称「请选择地形组」）。
/// 另有一条近义的 `ChooseaTerrainPattern_9FDBA167` = `选择地形组合`；取前者，因为它是
/// "选择要绘制的地形组合"这一整句的官方译文（与截图的动作语义一致）。
pub const AA_LABEL: &str = "选择要绘制的地形组合"; // EN: Select a terrain set to brush ｜ 官方: Selectaterrainsettobrush_224AB93F
/// 「当前资源」里**已选中**某一帧时的第二行：所属层 · 库 · 图号（全部真实数据）。
///
/// ⚠ `{img}` 是**图号（1 基）** = 面板选中帧的 0 基索引 + 1（审计 E1 全仓统一口径）。
pub const AA_SELECTED_TMPL: &str = "{fam} · {stem} · 图号 {img}"; // EN: {fam} · {stem} · sprite {img}
/// 「加入收藏」按钮文字（左边另有 [`PIN_STAR`] 星标，两者并排 = 官方截图的 `☆ 加入收藏`）。
pub const AA_PIN: &str = "加入收藏"; // EN: Pin to Favourites ｜ 官方: PintoFavourites_FA83A7D4（旧称「收藏」）

// ───────────────────────── 状态栏 ─────────────────────────

/// 状态栏左段模板：已打开地图时显示文件名与尺寸。
///
/// 占位符：`{name}` 文件名、`{w}`/`{h}` 格数。
/// （上一版这里是写死的 `资源配置已推迟`（官方 `Librarysetupdeferred_AB74EBDC`）；本单起改成
/// **真实的地图文件名**，理由：用户最需要确认"我看的是哪张图"，而库设置状态已经由右段
/// `资源库 N 个` 覆盖。）
pub const STATUS_LEFT_TMPL: &str = "{name} · {w}×{h} 格"; // EN: {name} · {w} × {h} cells

/// 状态栏左段：地图打开失败（`{err}` = 具体原因，同时写 stderr）。
pub const STATUS_MAP_FAIL_TMPL: &str = "地图打开失败：{err}"; // EN: Failed to open map: {err}

/// 状态栏左段：资源库装载失败（`{err}` = 具体原因，同时写 stderr）。
pub const STATUS_LIB_FAIL_TMPL: &str = "资源库装载失败：{err}"; // EN: Failed to load libraries: {err} ｜ 官方: libraries_01CFC309（旧称「图库装载失败」）

/// 状态栏左段：一张图都没打开。
pub const STATUS_NO_MAP: &str = "未打开地图"; // EN: No map open

/// 状态栏中段模板：鼠标所在单元格。
pub const STATUS_COORD_TMPL: &str = "单元格 {x},{y}"; // EN: Cell {x}, {y}
/// 状态栏右段模板：资源库数量。
///
/// 官方 `libraries_01CFC309` 把 libraries 译作「资源库」⇒ `资源库 {n} 个`
/// （旧称「素材库 {n} 个」）。
pub const STATUS_LIB_TMPL: &str = "资源库 {n} 个"; // EN: {n} libraries loaded ｜ 官方: libraries_01CFC309

/// 状态栏：**磁盘格式**（九格式人读名，取自 `miredit_core::Document::format_label`）。
///
/// 为什么与 [`STATUS_LEFT_TMPL`] 的 `{w}×{h} 格` 分开：那是**视图**口径（三档投影），
/// 九种格式里有五种（Type5 / Type1 / Type4 / Mir3 / CrystalCs）在视图上根本不是
/// 12/14/36 的磁盘格长 —— 混在一句里会让人以为"磁盘就是 New14"。
pub const STATUS_FORMAT_TMPL: &str = "格式：{fmt}"; // EN: Format: {fmt}

/// **地表落笔被前端拦下**时状态栏的那句回执（九种里只有 Type5 / `Mir3Wemade` 会走到）。
///
/// 文案口径与内核 `session.rs` 的 `ground_refusal` **同一件事、同一个理由**
/// （`mir2-format/src/model.rs:430-440`：Type5 的背图在**半分辨率块**里，逐格写 `bk_img` 无效），
/// 只是拦的位置不同：内核在 `touch()` 里拒，宿主在**点画布之前**就拦（任务书 §三.2）。
pub const STATUS_GROUND_BLOCKED_TMPL: &str =
    "地表落笔被拦：本格式（{fmt}）的背景图号在半分辨率块里，逐格写 bk_img 无效"; // EN: ground paint blocked

/// **常驻黄条**的文案登记标记（内容整句来自 `miredit_core::Document::edit_warning()`，
/// 宿主**一个字符都不加工**）。
///
/// 与 [`STATUS_OP_TMPL`] 同一条纪律：这是"运行期自由文案"的登记位，
/// 只为了让 [`ALL`] 与 `text_fit` 的自检能覆盖到这条动态文字（模板本身不做排版）。
pub const BANNER_TEXT_TMPL: &str = "{text}"; // EN: {text}

/// 状态栏中段：当前工具（`{tool}` 取自上面四个工具名之一）。
///
/// 位置在「左段（文件名）」右侧、坐标段左侧（见 [`crate::statusbar::dyn_labels`]），
/// 三段各自有独立裁剪盒，互不覆盖。
pub const STATUS_TOOL_TMPL: &str = "工具：{tool}"; // EN: Tool: {tool}

/// 状态栏中段：当前笔刷内容（**笔刷自己那一族**的图号 + 14/36 档的该族库号字节）。
///
/// 占位符：`{fam}` 笔刷族的中文层名（地面 = `地表图层`、中层 = `中层图层`、表层 = `物体`，
/// 与左侧图层面板同一批文案，见 [`crate::palette::family_label`]）、
/// `{img}` 图号（该族图号字段 `& 0x7FFF`）、`{lib}` 该族的库号字节（`Old12` 档是 `—`）。
///
/// ⚠ 上一版写死「地表」，在三族都能设笔刷之后就变成了错话（选一帧 `SmTiles`
/// 会显示成"笔刷：地表 图号 N"），故本版起改为按族填 `{fam}`。
/// ⚠ 末段原为 `· 库 {lib}`，按官方把 `Library` 译作「资源库」（`library_B718F135`）改成
/// `· 资源库 {lib}`（旧称「库 {lib}」）。
pub const STATUS_BRUSH_TMPL: &str = "笔刷：{fam} 图号 {img} · 资源库 {lib}"; // EN: Brush: {fam} sprite {img} · lib {lib} ｜ 官方: library_B718F135（资源库）

/// 状态栏中段：还没用吸管取过笔刷（**如实说没取**，绝不显示一个假的 0 号图）。
pub const STATUS_BRUSH_NONE: &str = "笔刷：未取（先用吸管）"; // EN: Brush: none (pick a cell first)

/// 状态栏中段：**当前编辑层**（三族里落笔写哪一层，由它决定）。
///
/// `{layer}` = 左侧图层面板的行名（`地表图层` / `中层图层` / `物体`，取自 [`LAYER_NAMES`]）。
/// 为什么状态栏也要显式写一遍：左侧那一行只是"选中态"，而"点位画到哪一层"是**破坏性**信息，
/// 必须在用户扫码就能看到的地方说清楚。
pub const STATUS_LAYER_TMPL: &str = "当前层：{layer}"; // EN: Edit layer: {layer}

/// 状态栏中段：当前层落笔会写哪些字段（14/36 档 = 图号 + 库号两个字段）。
///
/// `{img}` / `{lib}` 是**字段名**（`bk_img`/`btTiles`、`mid_img`/`btsmTiles`、`fr_img`/`btArea`，
/// 见 [`LAYER_IMG_FIELDS`] / [`LAYER_LIB_FIELDS`]），与 `mir2-map` 的字段同名 ——
/// 用户据此能自己核对"改的是不是我想要的那两个字节"。
pub const STATUS_WRITE_TMPL: &str = "落笔 {img} + {lib}"; // EN: writes {img} + {lib}

/// 状态栏中段：**12B 档没有库号字节**，只写图号 —— 如实说，不假装写了库号。
pub const STATUS_WRITE_12B_TMPL: &str = "落笔 {img}（本图 12B 档无库号字节，只写图号）"; // EN: writes {img} (12-byte format: no lib byte)

/// 状态栏中段：**阻挡层**的落笔位（只改 `bit15`，不写图号）。
///
/// 占位符：`{img}` = 该层动哪个图号字段（低位阻挡 = `bk_img`、高位阻挡 = `fr_img`）、
/// `{layer}` = 层名（`低位阻挡` / `高位阻挡`）。
///
/// 为什么要把 `0x8000` 和「置位=不可走」都写进状态栏：
/// 阻挡层改的不是"画什么"，而是"能不能走人" —— 这两个字**看不见摸不着**，
/// 只有把位名与方向（置位 / 清位）写在用户眼睛底下，才谈得上"画完知道能不能走"。
/// 语义出处：`client/reference_full/Classes/map/TMap.cpp:339`（`wBkImg & 0x8000` /
/// `wFrImg & 0x8000`）与 `mir2-map/src/cell.rs` 的 `Cell::is_walkable`。
pub const STATUS_WRITE_COLLISION_TMPL: &str = "落笔 {img}.0x8000（{layer}）· 置位=不可走"; // EN: writes {img}.0x8000 ({layer}) · set = blocked

/// 状态栏中段：**阻挡层拒绝"来自图库的笔刷"**（阻挡层不写图号）。
///
/// 占位符：`{brush}` 笔刷族的中文层名、`{layer}` 当前编辑层（低位/高位阻挡）。
///
/// 为什么必须拒绝而不是"顺手把位设上"：图库笔刷的意图是**盖一张图**，
/// 而阻挡层只动 `0x8000` 一个位 —— 若静默把位设上，用户会以为"图画上去了"，
/// 结果图号一个字节都没变（`bk_img` 低 15 位原样）。宁可拒绝并说清。
pub const EDIT_COLLISION_BRUSH_TMPL: &str =
    "笔刷来自图库（{brush}），{layer}只改 0x8000 位、不写图号"; // EN: palette brush ({brush}) — {layer} only flips 0x8000, never the sprite

/// 状态栏中段：还没打开地图，落笔字段无从谈起（**不写一个假的字段名**）。
pub const STATUS_HINT_NO_MAP: &str = "落笔：未打开地图"; // EN: writes: no map open

/// 状态栏中段：**族不匹配 ⇒ 拒绝落笔**（本版最重要的拒绝文案）。
///
/// 占位符：`{brush}` 笔刷族的中文层名、`{layer}` 当前编辑层的中文层名。
/// 语义：绝不跨字段乱写 —— 地表笔刷配「物体」层时，一个字节都不许动，并在这里说清两族。
///
/// ⚠ 措辞沿自任务书 B（原文用旧称：`笔刷族（地表层）与当前图层（物件层）不一致`），
/// 本轮只把层名换成官方译名（`地表图层` / `物体`），句式一字未改；
/// "已拒绝"那层意思由调用方补：stderr 那行是 `[编辑] 拒绝落笔：…`，
/// 状态栏里这句本来就挂在「落笔」那一段上，所以**不在这句里再重复**"已拒绝落笔"。
pub const EDIT_LAYER_MISMATCH_TMPL: &str = "笔刷族（{brush}）与当前图层（{layer}）不一致"; // EN: brush family ({brush}) ≠ edit layer ({layer})

/// 三层各自的**图号**字段名（ASCII 标识符，与 `mir2-map` 的 `Cell` 字段同名；不是界面文案）。
pub const LAYER_IMG_FIELDS: [&str; 3] = ["bk_img", "mid_img", "fr_img"];
/// 三层各自的**库号**字段名（`btTiles` / `btsmTiles` / `btArea`；`Old12` 档没有这三个字节）。
pub const LAYER_LIB_FIELDS: [&str; 3] = ["btTiles", "btsmTiles", "btArea"];

/// 状态栏左段：保存失败（`{err}` = 具体原因，**含系统报错原文**）。
///
/// 后半句"原文件未改动"是有保证的事实（见 [`crate::save`] 的原子替换顺序），
/// 用户看到它就知道不用去回收站找文件。
pub const STATUS_SAVE_FAIL_TMPL: &str = "保存失败：{err}（原文件未改动）"; // EN: Save failed: {err} (original untouched)

// ───────────────────────── 填充（Bucket） ─────────────────────────

/// 状态栏：**上一次填充填了几格**（图号层）。
///
/// 占位符：`{n}` = 真正改动的格数（= 那一步撤销里的格数）。
/// 官方原文是 `Filled {0:N0} tiles`（`MapViewport.cs:537`，`{0:N0}` 带千位分隔符）；
/// 本仓**不加千位分隔符**（任务书 §三.2 的示例是 `填充 1234 格`，且中文界面里
/// `1,234` 与 `1234` 并存会让 grep 对账变麻烦）。
pub const STATUS_FILL_TMPL: &str = "填充 {n} 格"; // EN: Filled {n} tiles ｜ 官方: Fill_033955D4 / MapViewport.cs:537 `Filled {0:N0} tiles`
/// 状态栏：**阻挡层**填充的结果（官方在这一层不说 "Filled tiles"，说的是应用阻挡）。
///
/// 官方原文：`Applied {0} collision to {1:N0} cells`（`MapViewport.cs:537`），
/// 其中 `{0}` 是 `low` / `high`（官方译文 `低位` / `高位`）。本仓把层名（`低位阻挡` /
/// `高位阻挡`）直接放进来，比"应用 low 到 N 格"更可读，语义不变。
pub const STATUS_FILL_COLLISION_TMPL: &str = "应用{layer} {n} 格"; // EN: Applied {layer} collision to {n} cells ｜ 官方: MapViewport.cs:537

// ───────────────────── 矩形选区 / 剪贴板（2026-11-05 新增） ─────────────────────

/// 状态栏：**只有选区**（`{w}`×`{h}` 是格数，`{x0},{y0}..{x1},{y1}` 是闭区间坐标）。
///
/// 官方在 `MapViewport.cs:412-415` 把选区画出来，但没有状态栏文案；
/// 本仓补一句是因为"框了多少格、从哪到哪"是用户下一步操作（复制/填充）的唯一依据。
pub const STATUS_SEL_TMPL: &str = "选区 {w}×{h}（{x0},{y0}..{x1},{y1}）"; // EN: Selection {w}×{h} ({x0},{y0}..{x1},{y1}) ｜ 官方: 无（官方不显示选区尺寸）

/// 状态栏：**选区 + 剪贴板**同时存在（中间用 `·` 分隔，与状态栏其它段同一分隔符）。
pub const STATUS_SEL_CLIP_TMPL: &str = "选区 {w}×{h}（{x0},{y0}..{x1},{y1}）· 剪贴板 {cw}×{ch}"; // EN: Selection {w}×{h} ({x0},{y0}..{x1},{y1}) · Clipboard {cw}×{ch} ｜ 官方: 无

/// 状态栏：**只有剪贴板**（选区已被取消，但复制过的东西还在 —— 官方 `Deselect`
/// 不清 `copiedRegion`，见 `RegionCopy.cs:96`/`:114`）。
pub const STATUS_CLIP_TMPL: &str = "剪贴板 {cw}×{ch}"; // EN: Clipboard {cw}×{ch} ｜ 官方: 无

/// 选区操作的**快捷键提示**（待选区态/状态栏 hint 用；三个有官方依据、一个没有）。
///
/// ⚠ 第 4 条 `Ctrl+Shift+V` 是**本仓自定**：官方 `MapViewport.cs` 只绑了
/// `Ctrl+C` / `Ctrl+X` / `Ctrl+V`（`:101`-`:121`）与 `Delete`，**没有**"填充选区"的快捷键
/// （填充是工具 `F` + 点击，`MainWindow.axaml.cs:861`）。本仓为选区填充补一个键，
/// **无官方依据**，在此显式记档。
pub const SEL_KEYS_HINT: &str = "选区：Ctrl+C 复制 · Ctrl+X 剪切 · Ctrl+V 粘贴 · Delete 删除 · Ctrl+Shift+V 填充（本仓自定）"; // EN: Selection: Ctrl+C / Ctrl+X / Ctrl+V / Delete / Ctrl+Shift+V fills (no upstream binding)

/// 选区操作回执：**已复制**（`{w}`×`{h}` = 格数）。
pub const SEL_COPY_TMPL: &str = "已复制 {w}×{h} 格"; // EN: Copied {w}×{h} cells ｜ 官方: RegionCopy.cs:101（官方只在视口上闪一句提示，无固定文案）

/// 选区操作回执：**已剪切**（复制到剪贴板 + 源块清空 `{n}` 格）。
pub const SEL_CUT_TMPL: &str = "剪切 {w}×{h} 格（清空 {n} 格）"; // EN: Cut {w}×{h} cells ({n} cleared) ｜ 官方: RegionCopy.cs:102

/// 选区操作回执：**已粘贴**（`{w}`×`{h}` = 剪贴板的尺寸，不是"改了几格"）。
pub const SEL_PASTE_TMPL: &str = "粘贴 {w}×{h} 格"; // EN: Pasted {w}×{h} cells ｜ 官方: RegionCopy.cs:121 `Region pasted · Ctrl+V to paste again`

/// 粘贴时越界格被逐格跳过（接在 [`SEL_PASTE_TMPL`] 后面）。
///
/// 为什么必须报：粘贴复用内核的印章放置（逐格越界裁剪），贴到图边时**只有一部分落地**；
/// 不报的话用户会以为"整块都粘上了"。
pub const SEL_PASTE_SKIP_TMPL: &str = "（{k} 格在地图外，已跳过）"; // EN: ({k} cells outside the map, skipped)

/// 选区操作回执：**已删除**（`{n}` = 真清掉的格数，三族按层清）。
pub const SEL_DELETE_TMPL: &str = "删除 {n} 格"; // EN: Deleted {n} cells ｜ 官方: RegionCopy.cs:45-69 `CutSource`

/// 选区操作回执：**已填充选区**（`{n}` = 真改动的格数）。
pub const SEL_FILL_TMPL: &str = "填充选区 {n} 格"; // EN: Filled {n} cells in the selection ｜ 官方: MapViewport.cs:540 `FillSelection`

/// 兜底拒绝：没有选区（内核正常会给更具体的原因，这条只在取不到 `note` 时用）。
pub const SEL_ERR_NO_SELECTION: &str = "没有选区：先框一个矩形选区"; // EN: no selection

/// 兜底拒绝：剪贴板是空的。
pub const SEL_ERR_EMPTY_CLIPBOARD: &str = "剪贴板是空的：先框一个选区再复制"; // EN: the clipboard is empty

/// 兜底拒绝：选区内没有可清的内容。
pub const SEL_ERR_NOTHING_TO_DELETE: &str = "选区里没有可清的内容（三族图号都是 0），未改动"; // EN: nothing to clear in the selection

/// 兜底拒绝：填充没改成任何格（没有笔刷 / 族不匹配 / 已是目标值）。
pub const SEL_ERR_NOTHING_TO_FILL: &str = "选区填充没有改动任何格（原因见落笔段），未改动"; // EN: the fill changed nothing

/// 状态栏「上一次操作回执」那一段的**登记模板**（`{text}` 由运行期填）。
///
/// 这一段装的是**自由文案**：放置印章回执、剪切/粘贴/删除/填充回执、以及它们的拒绝原因
/// （各自由 `STAMP_PLACE_*` / `SEL_*` 那几条模板生成）。`DynLabel` 要求 `tmpl` 必须是
/// [`ALL`] 里登记过的一行，所以这里留一个**标记模板** —— 它本身不会被拿去 `fill`，
/// 只用来在自检里表明"这一段是运行期拼的"。
pub const STATUS_OP_TMPL: &str = "{text}"; // EN: {text}（运行期自由文案的登记标记）



/// 填充被拒：**本层不支持填充**。
///
/// 官方原文（`MapViewport.cs:490`）：
/// `"Fill works on ground tiles, middle tiles (smtiles), and low/high collisions"`；
/// 官方译文（工具提示 `FillFFillConnectedMatchingTilesSmtilesorApplyLowHighCollisions_CED026EB`）：
/// 「填充连通且匹配的地表 / 中层图块，或应用高低位阻挡」。
/// 物体层被排除是因为它没有"图号"可比较（官方 `Foreground` 是 `(lib, img)` 对，且填充循环里
/// 只处理 Ground / Middle / 碰撞三种，见 `MapViewport.cs:498-527`）。
pub const FILL_ERR_LAYER: &str =
    "填充只支持地表图层 / 中层图层 / 低位阻挡 / 高位阻挡（物体层没有可比较的图号）"; // EN: fill works on ground/middle/low/high only ｜ 官方: FillFFillConnectedMatchingTilesSmtilesorApplyLowHighCollisions_CED026EB
/// 填充被拒：**手上没有笔刷**（图号层需要知道填成哪一号图）。
pub const FILL_ERR_NO_BRUSH: &str = "没有笔刷：先用吸管取一格，或在资源面板选一帧"; // EN: no brush — pick a cell or a palette frame first
/// 填充被拒：**点击格在地图外**（官方 `MapViewport.cs:493` 的 `!Document.Contains(x, y)`）。
pub const FILL_ERR_OUT_OF_MAP: &str = "点击格 ({x},{y}) 在地图外"; // EN: cell ({x},{y}) is outside the map
/// 填充被拒：**连通块超过上限，一格都没写**。
///
/// 占位符：`{n}` = 已扫到的格数（在上限处截断，故一般等于 `上限+1`）、`{max}` = 上限。
pub const FILL_ERR_TOO_LARGE: &str =
    "连通块至少 {n} 格，超过单次上限 {max} 格 —— 未执行，地图一个字节都没动"; // EN: region of {n}+ cells exceeds the {max}-cell limit — nothing written

// ───────────────────────── 中列：页签 / 缩放浮层 ─────────────────────────

/// 页签标题模板（`{name}` = 地图文件名）。
pub const TAB_TITLE_TMPL: &str = "{name}"; // EN: {name}
/// 页签标题模板（**有未保存修改**）。
pub const TAB_TITLE_DIRTY_TMPL: &str = "{name} *"; // EN: {name} *
/// 页签右端的关闭符。
pub const TAB_CLOSE: &str = "×"; // EN: ×

/// 缩放浮层的减号。
///
/// ⚠ **实测换码位**：参考图用的是 `−` U+2212 MINUS SIGN，但 `Deng.ttf`（等线）**没有 U+2212**
/// （`glyph_id = 0`），写它会掉到 egui 内置拉丁字体上、与中文混排时粗细不齐。
/// 改用同义且**字体确有字形**的 `－` U+FF0D FULLWIDTH HYPHEN-MINUS（`glyph_id = 28436`），
/// 字形是一根居中的横杠，在 13px 下与参考图的减号几乎一致（见 `metrics::ZOOM_SIGN_FONT`）。
pub const ZOOM_MINUS: &str = "－"; // EN: − (U+2212，Deng.ttf 缺该字形，见上方说明)
/// 缩放百分比模板（`{pct}` = 整数百分比，如 `100`）。
pub const ZOOM_PCT_TMPL: &str = "{pct}%"; // EN: {pct}%
pub const ZOOM_PLUS: &str = "+"; // EN: +
/// `100` 之外的取值也要有字形，这里把可能出现的整数百分比字符集写进清单：
/// 字形测试逐字检查 `ZOOM_PCT_TMPL` 的 `{pct}%`，数字与 `%` 均已在其它条目出现过。
pub const ZOOM_FIT: &str = "适应地图"; // EN: Fit Map ｜ 官方: FitMap_CDF75BEB（旧称「适应窗口」；截图实测「适应地图」）

/// 缩放浮层尾部「原始大小」按钮（**上游 `901dbe2f` 新增的 `OriginalSizeButton`**，
/// `MainWindow.axaml:22`，`Click="ResetZoomClick"` ⇒ 与本仓 `view.actual_size` 同一条动作）。
pub const ZOOM_ORIGINAL: &str = "原始大小"; // EN: Original Size ｜ 官方: OriginalSize_20261105

// ───────────────────────── 中列：地图列表面板（2026-11-05） ─────────────────────────
//
// 文案来源：上游 `MainWindow.MapBrowser.cs` 用的是**字面量键**（`LocalText.T("Refresh")`
// 这类），这些键**不在** `Strings.json` 里 ⇒ 官方漏译。本段全部按本仓口径自定，
// 并在每条后面标出它对应的上游控件，便于以后官方补译时逐条替换。

/// 面板内「刷新」按钮（上游 `mapRefreshButton` 的 `Content = LocalText.T("Refresh")`）。
pub const MAPLIST_REFRESH: &str = "刷新"; // EN: Refresh ｜ 上游 `MapBrowser.cs:10`

/// 列表行**文件名列**的模板（`{name}` = 相对路径；行内文字上游是 `entry.RelativePath`）。
///
/// 之所以要有模板而不是直接画数据：本 crate 的动态文字一律走
/// [`crate::text_fit::DynLabel`]（按裁剪盒实测截断），模板必须能进 [`ALL`]。
pub const MAPLIST_NAME_TMPL: &str = "{name}"; // EN: {name} ｜ 上游 `MapBrowser.cs:37`

/// 扫描中状态行（`{n}` = 已找到的张数）。
pub const MAPLIST_SCANNING_TMPL: &str = "扫描中…已找到 {n} 张"; // EN: Scanning… {n} maps found

/// 扫描完成状态行（`{n}` = 张数，`{ms}` = **纯扫描耗时**）。
///
/// 为什么把毫秒也上屏：任务书要求"实测贴出来"，而面板上直接显示能让用户
/// 一眼看出"这个目录扫得快不快"（比翻日志强）。耗时来自
/// [`crate::maplist::ScanStats::ms`]，不是估算值。
pub const MAPLIST_STATUS_TMPL: &str = "{n} 张 · 扫描 {ms} ms"; // EN: {n} maps · scan {ms} ms

/// 有跳过/失败项时追加到状态行尾部（**不静默**：读不了的目录要能看见）。
pub const MAPLIST_SKIPPED_TMPL: &str = " · 跳过 {skipped} 项 / 失败 {errors} 项"; // EN: · skipped {skipped} / failed {errors}

/// 空目录（`{path}` = 扫描根）——上游 `No .map files in the Map folder.`。
pub const MAPLIST_EMPTY_TMPL: &str = "目录里没有 .map 文件：{path}"; // EN: No .map files in the Map folder.

/// 扫描失败（`{why}` = 系统给的原因，中文前缀是本仓的）。
pub const MAPLIST_FAIL_TMPL: &str = "扫描失败：{why}"; // EN: Could not read map folder: {why}

/// 还没有扫描根（既没打开地图，也没给 `--map-list-root`）。
pub const MAPLIST_NO_ROOT: &str = "还没有地图目录：先打开一张地图，或用 --map-list-root 指定"; // EN: No map folder yet

/// 行悬停提示（绝对路径 + 大小 + 修改时间；上游 `label.SetTip(entry.FilePath)`）。
///
/// ⚠ 修改时间是 **UTC**，提示里必须写明 —— 本 crate 没有时区库（见
/// [`crate::maplist::format_mtime`] 的文档），把 UTC 值当本地时间显示就是假信息。
pub const MAPLIST_TIP_TMPL: &str = "{path}\n{size} · 修改时间 {mtime}（UTC）"; // EN: {path}\n{size} · modified {mtime} (UTC)

/// 扫描根读不了（不存在 / 没权限）。
pub const MAPLIST_ERR_ROOT: &str = "[地图列表] 读不了地图目录：{path}（{why}）"; // EN: cannot read map folder
/// 扫描根不是目录。
pub const MAPLIST_ERR_NOT_DIR: &str = "[地图列表] 不是目录：{path}"; // EN: not a directory
/// 子目录读不了（跳过但计数）。
pub const MAPLIST_ERR_DIR: &str = "[地图列表] 跳过读不了的子目录：{path}（{why}）"; // EN: skipped unreadable subdir
/// 单个文件取不到元数据（跳过但计数）。
pub const MAPLIST_ERR_FILE: &str = "[地图列表] 跳过取不到元数据的文件：{path}（{why}）"; // EN: skipped file without metadata
/// 目录项本身读失败（`ReadDir` 返回 `Err`）。
pub const MAPLIST_ERR_ENTRY: &str = "[地图列表] 目录项读取失败：{why}"; // EN: directory entry read failed

/// 扫描完成诊断行（stderr；`ms` = 纯扫描耗时，`wall` = 含分帧等待的墙钟耗时）。
pub const DIAG_MAPLIST_SCAN: &str = "[地图列表] 扫描完成 root={root} n={n} ms={ms}（墙钟 {wall} ms · {steps} 步 · {dirs} 目录 · {seen} 项 · 跳过 {skipped} · 失败 {errors}）"; // EN: map list scan done
/// 扫描根不可用诊断行（stderr）。
pub const DIAG_MAPLIST_FAIL: &str = "[地图列表] 扫描根不可用：{root}：{why}"; // EN: map list root unusable

// ───────────────────────── 文件大小的单位 ─────────────────────────

/// 文件大小模板（`{num}` = 数字，`{unit}` = 单位）。
pub const SIZE_VALUE_TMPL: &str = "{num} {unit}"; // EN: {num} {unit}
/// 字节（< 1 KB）。
pub const SIZE_UNIT_B: &str = "B"; // EN: B
/// 千字节（1024 进制）。
pub const SIZE_UNIT_KB: &str = "KB"; // EN: KB
/// 兆字节（1024 进制）。
pub const SIZE_UNIT_MB: &str = "MB"; // EN: MB
/// 吉字节（1024 进制）。
pub const SIZE_UNIT_GB: &str = "GB"; // EN: GB

// ───────────────────────── 中列：空态 ─────────────────────────

/// 没有地图时画布中央的提示（画布不画空白"假网格"，如实说没打开）。
pub const CANVAS_NO_MAP: &str = "未打开地图"; // EN: No map open

/// **未指定**地图（公开仓没有内建缺省图）：启动时没有 `--map` 的真实原因。
///
/// 与 [`CANVAS_NO_MAP`]（画布中央的空态）分开：这句进**状态栏左段**（`map_error`），
/// 要顺带告诉用户怎么打开一张图，否则界面上只剩"没开图"这一条无从下手的信息。
pub const DIAG_MAP_NONE: &str = "未指定地图：用 --map <路径> 打开"; // EN: no map given — pass --map <path>

// ───────────────────────── 通用符号 ─────────────────────────

/// 搜索框右端的清除符。
pub const SEARCH_CLEAR: &str = "×"; // EN: ×
/// 标题栏三个窗口按钮（自右向左：`×` 关闭、`□` 最大化、`—` 最小化）。
pub const CAPTION_CLOSE: &str = "×"; // EN: ×
pub const CAPTION_MAXIMIZE: &str = "□"; // EN: □
pub const CAPTION_MINIMIZE: &str = "—"; // EN: —
/// 标题栏三个按钮的字形（顺序 = 自右向左绘制顺序）。
///
/// ⚠ `□` 是**最大化按钮的图形本身**（U+25A1 白方框），不是缺字的豆腐块；
/// 截图自检时它会被算作"方框形"像素，属预期。
pub const CAPTION_GLYPHS: [&str; 3] = [CAPTION_CLOSE, CAPTION_MAXIMIZE, CAPTION_MINIMIZE];

/// 分页按钮的左右箭头。
///
/// ⚠ 决策依据见 [`crate::dock_right::draw_pager_arrow`]：先用字形测试确认系统字体
/// 是否真有 U+2190 / U+2192，有则直接写字形；没有才退回自绘。
pub const PAGER_PREV: &str = "←"; // EN: ←
pub const PAGER_NEXT: &str = "→"; // EN: →
/// `收藏` 行的五角星。
pub const PIN_STAR: &str = "☆"; // EN: ☆

// ───────────────────────── 关闭拦截模态 ─────────────────────────

/// 有未保存修改时点关闭按钮的拦截提示（简单模态，居中显示在画布上）。
///
/// 语义与操作**一一对应**：`Ctrl+S` 保存后模态自动消失；再点一次右上角 `×` 才真的放弃。
pub const CLOSE_CONFIRM: &str = "有未保存的修改，按 Ctrl+S 保存 / 再点关闭放弃"; // EN: Unsaved changes — Ctrl+S to save, click close again to discard

// ───────────────────────── 诊断文案（写 stderr，不上屏） ─────────────────────────
//
// 按「文案全部走 ui_text」的纪律，连诊断输出也不许在代码里写中文字面量。
// 这些模板只出现在 `eprintln!`/`println!` 里，不进画面，但照样进 [`ALL`] 做字形自检
// —— 理由很简单：它们将来可能被搬到界面上（例如错误浮层），先查一遍不吃亏。

/// 启动时的地图加载成功。
pub const DIAG_MAP_OK: &str = "[地图] 已打开 {path}（{w}×{h} 格，档位 {fmt}，{bytes} 字节）"; // EN: [map] opened
/// 地图加载失败（同时会写进状态栏左段）。
pub const DIAG_MAP_FAIL: &str = "[地图] 打开失败 {path}：{err}"; // EN: [map] open failed
/// **地图已打开但有告警**（未解尾块 / 格式未承诺编辑语义）—— 开机就打到 stderr，
/// 与界面上的常驻黄条（[`crate::banner`]）是**同一句 core 原文**。
pub const DIAG_MAP_WARN: &str = "[地图] 告警：{warn}"; // EN: [map] warning
/// 启动时的资源库加载成功。
pub const DIAG_LIB_OK: &str =
    "[资源库] 已登记 {n} 个（地面 {g} / 中层 {m} / 表层 {s}），根目录 {root}"; // EN: [lib] loaded ｜ 官方: libraries_01CFC309（旧称「[图库]」）
/// 资源库加载失败（状态栏右段会显示 `资源库 0 个`）。
pub const DIAG_LIB_FAIL: &str = "[资源库] 装载失败：{err}"; // EN: [lib] load failed ｜ 官方: libraries_01CFC309（旧称「[图库]」）
/// 每次真正重渲一行（**只在重渲时打印**，不会每帧刷屏）。
pub const DIAG_RENDER: &str = "[渲染] 第 {n} 次重渲：{cw}×{ch} 格 / {pw}×{ph} px，原点 ({ox},{oy})，用时 {ms} ms，doors={doors}，lighting={lighting}，{stats}"; // EN: [render]
/// 视口要求的格数超过一次性重渲上限（防御性护栏，正常窗口不会触发）。
pub const DIAG_RENDER_BUDGET: &str = "[渲染] 视口需要 {cells} 格，超过上限 {max} 格，本次不重渲（保持上一帧纹理）"; // EN: [render] budget
/// 单元格网格**第一次真正画出来**时打一行（`视图 → 单元格网格` / `--cell-grid`）。
///
/// 为什么要这一行：网格是"压在地图上的线"，它在截图里与地图自身的深色像素混在一起，
/// 光看 PNG 很难判"这条线是不是正好落在格边界上"。这行把**真实算出来的**首线坐标、
/// 步长、抽稀步长、视口原点、平移、缩放一次说清 —— 验收脚本拿它就能把 PNG 里每条差异像素
/// 反查回世界格边界（`x = 首线x + 48·step·k`）。
///
/// `{step}` 是**抽稀步长**（上游 `901dbe2f` 删掉 `Zoom >= .4` 门槛后改成"缩小时每 N 格一条"，
/// 见 `crate::grid::step_for`）；`{dx}`/`{dy}` 报的是**屏上真实间距**（含抽稀），不是单格间距。
///
/// ⚠ 只打一次（首次画网格那一帧）：`draw` 每帧都跑，每帧打就是刷屏。
pub const DIAG_GRID: &str = "[网格] 竖线 {nv} 条 / 横线 {nh} 条：首线 x={x1}（步长 {dx}）、y={y1}（步长 {dy}）；每 {step} 格一条；视口左 {left} 上 {top}，平移 {px},{py}，缩放 {scale}"; // EN: [grid] first draw
/// 拖动性能实测报告（`--autodrag`）。
pub const DIAG_AUTODRAG: &str = "[拖动实测] {frames} 帧 / render_region {calls} 次 / 平均帧时间 {ms} ms / 缩放 {scale} / 视口 {cw}×{ch} 格"; // EN: [drag] report
/// 截图已保存。
pub const DIAG_SHOT_OK: &str = "[截图] 已保存 {path}（{w}×{h}）"; // EN: [shot] saved
/// 截图失败。
pub const DIAG_SHOT_FAIL: &str = "[截图] 失败：{err}"; // EN: [shot] failed
/// 同视点分块比对报告头。
pub const DIAG_GATE2: &str = "[同视点比对] 本机画布 {w}×{h} 裁剪，客户端 {cw}×{ch} 裁剪，公共区 {iw}×{ih}，背景 RGB({r},{g},{b})"; // EN: [gate2]
/// 同视点分块比对结果。
pub const DIAG_GATE2_RESULT: &str = "[同视点比对] 分块命中 {hit}/{total} = {rate}%（阈值 {threshold}，块 {block}×{block}）"; // EN: [gate2] result
/// 同视点分块比对：**排除浮层**后的补充口径（任务书 §五.2「画布里只多了页签条和浮层」）。
pub const DIAG_GATE2_OVERLAY: &str = "[同视点比对] 排除画布浮层（缩放浮层占 {ph} px 高）后：命中 {hit}/{total} = {rate}%"; // EN: [gate2] excluding overlays
/// 单元格探针（`--probe`）。
///
/// `{walk}` = `mir2_map::Map::walkable(x,y)` 的**真实返回**（`1` 可走 / `0` 不可走）。
/// 为什么把这一项单列：阻挡位改得对不对，最终判据就是客户端那句
/// `CanWalk`（`TMap.cpp:339`）的等价实现 —— 面板上的「阻挡 有/无」是人看的，
/// `walkable` 是**程序能直接对账**的那个值（验收门 §四.2 贴的就是它）。
pub const DIAG_PROBE: &str = "[探针] {head}｜阻挡={collision}｜walkable={walk}｜地表={ground}｜中层={middle}｜物体={objects}｜元数据={meta}｜动画={anim}"; // EN: [probe] ｜ 官方: Objects_DE5236EE（物体；旧称「物件」）
/// 渲染转储（`--dump-render`，把「渲染库的锅」与「界面的锅」分开）。
pub const DIAG_DUMP: &str = "[渲染转储] {path}：{cw}×{ch} 格 / {pw}×{ph} px，原点 ({ox},{oy})，doors={doors}，lighting={lighting}，{stats}"; // EN: [dump]
/// 启动一行。
pub const DIAG_START: &str = "miredit-gui 启动中…"; // EN: starting
/// 正常退出。
pub const DIAG_EXIT: &str = "miredit-gui 已退出（码 {code}）"; // EN: exited
/// 启动失败。
pub const DIAG_EXIT_ERR: &str = "miredit-gui 启动失败：{err}"; // EN: failed to start
/// 命令行参数错误。
pub const DIAG_BAD_ARG: &str = "[参数] 无法解析：{arg}"; // EN: [cli] bad arg
/// 帮助。
pub const DIAG_USAGE: &str = "用法：miredit-gui [--map <路径>] [--center <x,y>] [--scale <倍数>] [--layers ground,middle,front] [--layer ground|middle|front|low_collision|high_collision] [--collision low|high|all|none] [--tool <0..9>] [--brush-size <3..31 奇数>] [--edit-layer ground|middle|front|low_collision|high_collision] [--edit-size <3..31 奇数>] [--edit-pick <x,y>] [--edit-brush <x,y>] [--edit-erase <x,y>] [--edit-stroke <x0,y0,x1,y1>] [--edit-fill <x,y>] [--edit-undo <n>] [--edit-redo <n>] [--edit-check] [--save] [--save-as <路径>] [--no-window] [--shot <png>] [--gate2-client <png>] [--gate2-at <x,y>] [--autodrag <秒>] [--probe <x,y>] [--frames <帧数>] [--palette-lib <基名>] [--palette-scroll <行>] [--palette-search <图号>] [--palette-tab <0..3>] [--palette-pick <帧索引0基>] [--palette-autoscroll <秒>] [--palette-report] [--stamp-kind <all|unclassified|unlisted|英文 kind>] [--stamp-search <文字>] [--stamp-page <1基页码>] [--stamp-pick <当前页格号0基>] [--stamp-report] [--stamp-probe] [--stamp-sweep] [--stamp-arm] [--stamp-place-at <x,y>] [--select <x0,y0,x1,y1>] [--select-copy] [--theme light|dark] [--dark] [--cell-grid] [--no-cell-grid] [--open-doors] [--lighting dawn_dusk|day|night] [--menu-open <0..4>] [--menu-hover <菜单内下标，仅配合 --shot/--menu-open>] [--menu-sub-focus <父项>,<行>，仅配合 --shot/--menu-open>] [--menu-click <菜单项id[@帧号]>] [--export-selection <路径>] [--export-minimap <路径>] [--export-doors] [--minimap-width <像素>] [--minimap-height <像素>] [--minimap-preset small|medium|large] [--minimap-max-side <像素>] [--menu-dump] [--shortcuts] [--panel-hide <tools|tool_options|layers|minimap|palette|active_asset|cell_inspector>] [--open-map <路径>] [--settings <路径>]"; // EN: usage

// ───────────────────────── 编辑/保存诊断（本单新增） ─────────────────────────

/// 编辑脚本执行时：一张图都没打开（**立刻停手**，不假装改过）。
pub const DIAG_EDIT_NO_MAP: &str = "[编辑] 没有打开地图，编辑脚本未执行"; // EN: [edit] no map open
/// 吸管取到笔刷（`{fam}` = 笔刷族的中文层名，图号与库号都取自**当前编辑层**那个字段）。
pub const DIAG_EDIT_PICK: &str = "[编辑] 吸管 ({x},{y}) → 笔刷 {fam} 图号 {img} · 资源库 {lib}"; // EN: [edit] pickup ｜ 官方: library_B718F135（资源库；旧称「库」）
/// 吸管取到的**整格三族**（三个图号字段原样读出来的值；笔刷只用其中当前层那一个）。
///
/// 为什么要打这一行：吸管读的是整格，但「设为笔刷」只取当前层字段 ——
/// 两件事分开打印，才不会让人以为"吸管把三族都塞进笔刷了"。
pub const DIAG_EDIT_PICK_CELL: &str = "[编辑] 吸管 ({x},{y}) 整格：地表 图号 {bk} · 中层 {mid} · 物体 {fr}"; // EN: [edit] picked cell (all three families) ｜ 官方: Objects_DE5236EE（物体；旧称「物件」）
/// 切换当前编辑层（`{layer}` = 层名，`{img}`/`{lib}` = 该层落笔字段）。
pub const DIAG_EDIT_LAYER: &str = "[编辑] 当前编辑层 → {layer}（落笔 {img} + {lib}）"; // EN: [edit] edit layer
/// 切换当前编辑层到**阻挡层**（没有库号字节可写，只有 `bit15` 一个位）。
///
/// 与 [`DIAG_EDIT_LAYER`] 分开说：那一句的 `{lib}` 在阻挡层没有对应字段，
/// 硬套会打出 `落笔 bk_img + —` 这种半真半假的诊断行。
pub const DIAG_EDIT_LAYER_COLLISION: &str =
    "[编辑] 当前编辑层 → {layer}（落笔 {img}.0x8000：置位=不可走 / 清位=可走）"; // EN: [edit] edit layer (collision bit)
/// **拒绝落笔**的原因（`{why}` = 与状态栏同一句原因，例如族不匹配）。
///
/// 为什么要另打一行：状态栏只在窗口里看得见，而无人值守验收读的是 stderr ——
/// 拒绝必须有**可机器抓**的输出，否则"拒绝了"这件事只能靠人工看截图。
pub const DIAG_EDIT_REJECT: &str = "[编辑] 拒绝落笔：{why}"; // EN: [edit] rejected
/// **地表落笔被前端拦下**（九种格式里只有 Type5 / `Mir3Wemade` 会走到）。
///
/// ⚠ 这是**宿主拦的，不是内核拒的**（任务书 §三.2：「点画布前先拦，别等 core 拒绝」）——
/// 命令一条都没发出去，所以这一行必须自己说清楚，否则"点了没反应"看起来就像坏了。
/// `{fmt}` = `Document::format_label()` 的真名，`{why}` = 宿主给的完整中文原因。
pub const DIAG_EDIT_GROUND_BLOCKED: &str = "[编辑] 地表落笔被前端拦下（{fmt}）：{why} —— 未发送任何编辑命令"; // EN: [edit] ground paint blocked by frontend
/// **真文档没能绑进编辑会话**（`EditSession::bind_document` 返回 `false`）。
///
/// 后果要如实说：保存仍然正确（走 `MapDoc::to_bytes`），但"按格式判定的编辑语义"
/// （库号字节还原 / Type5 地表闸门 / 选区裁剪）会退化成占位文档的 `Old12` 口径。
pub const DIAG_EDIT_BIND_FAIL: &str = "[编辑] 真文档绑定失败（{path}）：按格式判定的编辑语义退化为占位档位，保存仍走真文档"; // EN: [edit] bind failed
/// 吸管点到了图外（越界），笔刷保持不变。
pub const DIAG_EDIT_PICK_NONE: &str = "[编辑] 吸管 ({x},{y}) 在图外，笔刷未改变"; // EN: [edit] pickup out of bounds
/// 笔刷刷下一格（`{n}` = 本格是否真的改了：0 或 1）。
pub const DIAG_EDIT_BRUSH: &str = "[编辑] 笔刷 ({x},{y}) → 改动 {n} 格"; // EN: [edit] brush ｜ 官方: Brush_8624CA0F（旧称「画笔」）
/// 橡皮擦擦掉一格。
pub const DIAG_EDIT_ERASE: &str = "[编辑] 橡皮擦 ({x},{y}) → 改动 {n} 格"; // EN: [edit] eraser ｜ 官方: Eraser_686EEE3C（旧称「橡皮」）
/// 填充一次的结果（`{n}` = 真正改动的格数、`{layer}` = 当前编辑层）。
///
/// 与状态栏那句 [`STATUS_FILL_TMPL`] 是**同一件事的两处出口**：状态栏给人看，这一行给验收脚本 grep。
pub const DIAG_EDIT_FILL: &str = "[编辑] 填充 ({x},{y}) → 改动 {n} 格（{layer}）"; // EN: [edit] fill ｜ 官方: Fill_033955D4
/// 填充被拒（原因与状态栏同一句）。
pub const DIAG_EDIT_FILL_REFUSED: &str = "[编辑] 拒绝填充：{why}"; // EN: [edit] fill refused
/// 笔刷尺寸变化（`{n}` = 新尺寸，`N×N`）。
pub const DIAG_EDIT_SIZE: &str = "[编辑] 笔刷尺寸 → {n}×{n}"; // EN: [edit] brush size ｜ 官方: BrushSize_FB23F428（笔刷大小）
/// 一次拖动（含插值）的结果：`{cells}` = 改动格数、`{n}` = 尺寸、`{from}`/`{to}` = 起止格。
pub const DIAG_EDIT_STROKE: &str =
    "[编辑] 笔画 ({x0},{y0})→({x1},{y1}) 尺寸 {n}×{n} → 改动 {cells} 格"; // EN: [edit] stroke ｜ 官方: Brush_8624CA0F
/// 撤销结果。
pub const DIAG_EDIT_UNDO: &str = "[编辑] 撤销 {n} 步 → 还原 {cells} 格；撤销栈 {undo} 步 / 可重做 {redo} 步；未保存修改 {dirty}"; // EN: [edit] undo
/// 重做结果。
pub const DIAG_EDIT_REDO: &str = "[编辑] 重做 {n} 步 → 重放 {cells} 格；撤销栈 {undo} 步 / 可重做 {redo} 步；未保存修改 {dirty}"; // EN: [edit] redo
/// 逐字节比对：完全相同。
pub const DIAG_EDIT_CMP_SAME: &str = "[比对] 与磁盘原文件逐字节相等（{len} 字节）"; // EN: [cmp] byte-identical
/// 逐字节比对：有差异（`{cells}` 是按档位格长折算出的**涉及格数**）。
pub const DIAG_EDIT_CMP_DIFF: &str = "[比对] 与磁盘原文件不同：{bytes} 字节 / 涉及 {cells} 个格（格长 {size}）/ 首个差异 @{off}；头部差异 {head} 字节"; // EN: [cmp] diff
/// 逐字节比对：读不回原文件。
pub const DIAG_EDIT_CMP_ERR: &str = "[比对] 读不回原文件 {path}：{err}"; // EN: [cmp] read back failed
/// 保存：没有修改，跳过写盘。
pub const DIAG_EDIT_SAVE_SKIP: &str = "[保存] 没有未保存的修改，未写盘（文件一个字节都没动）"; // EN: [save] skipped
/// 保存成功（`{tmp}` 是被 rename 掉的临时文件路径）。
pub const DIAG_EDIT_SAVE_OK: &str = "[保存] 已原子替换 {path}（{bytes} 字节；临时文件 {tmp}）"; // EN: [save] ok
/// 保存失败（原文件保持原样）。
pub const DIAG_EDIT_SAVE_FAIL: &str = "[保存] 失败：{err}"; // EN: [save] failed
/// 保存：没有打开地图（无从写回）。
pub const DIAG_EDIT_SAVE_NOMAP: &str = "[保存] 没有打开地图，无从保存"; // EN: [save] no map
/// 目录项 fsync 成功（rename 之后刷父目录，保证目录项也落盘）。
pub const DIAG_SAVE_DIRSYNC_OK: &str = "[保存] 目录项已 fsync（{path}）"; // EN: [save] dir fsynced

// ───────────── 资源面板（真图块网格）的诊断输出（写 stderr，不上屏） ─────────────

/// 资源面板状态：库 / 总帧 / 可见槽位 / 实际解码数 / 缓存命中 / 空帧 / 滚动位置。
///
/// 「可见槽位」= 可见格数 + 1 行预取；「实际解码」只统计**真解出像素**的帧数
/// （空帧/无効帧不计入解码，单独记「空」），所以两个数不相等是正常的。
pub const DIAG_PALETTE: &str = "[资源] 库 {stem}（{fam}）总帧 {total} / 可见槽 {slots} / 已解码 {decodes} / 缓存命中 {hits} / 空帧 {empty} / 顶部行 {row} / 可见行 {rows}"; // EN: [palette] state ｜ 官方: Palette_85794EFF（旧称「[素材]」）
/// 资源面板滚动实测（`--palette-autoscroll`）：帧数 / 平均帧时间 / 解码次数 / 命中。
pub const DIAG_PALETTE_SCROLL: &str = "[资源滚动实测] {frames} 帧 / 平均帧时间 {ms} ms / 解码 {decodes} 次 / 缓存命中 {hits} / 顶部行 {row}"; // EN: [palette] scroll report ｜ 官方: Palette_85794EFF（旧称「[素材滚动实测]」）
/// 资源搜索：输入了什么、结果是什么。
pub const DIAG_PALETTE_SEARCH: &str = "[资源搜索] 输入 {q} → {msg}"; // EN: [palette] search ｜ 官方: Palette_85794EFF（旧称「[素材搜索]」）
/// 资源点选：哪一库哪一帧 → 笔刷（含真实库号与**真实笔刷族**）。
///
/// `{img}` 是**图号（1 基）**（= 选中帧的 0 基索引 + 1，审计 E1）；
/// 上一版同一个 `{img}` 既当"第 N 帧"又当"图号 N"，两个口径差 1 却长得一样，本单去重。
///
/// ⚠ **`{fam}` 必须是笔刷自己的族**：上一版这里写死「地表」，一旦允许从 `SmTiles`
/// 选笔刷，这行就会把中层笔刷报成"地表"（2026-11-04 验收实跑抓到的假数据），已改。
pub const DIAG_PALETTE_PICK: &str = "[资源] 点选 {stem} → 笔刷 {fam} 图号 {img} · 资源库 {lib}"; // EN: [palette] pick ｜ 官方: Palette_85794EFF / library_B718F135（旧称「[素材]」「库」）
/// 资源面板的库失败记录（`WilSource::failures`，**含解码失败**；空帧也会进这里）。
///
/// 为什么单独一行：格子里画不出来的原因有两种（空帧 / 真解码失败），
/// 面板上的 `空帧 N` 是二者合计，光看它分不清"这一帧本来就没有"还是"解码器出错了"。
pub const DIAG_PALETTE_FAIL: &str = "[资源] 资源库失败记录 {n} 条，首条：{first}"; // EN: [palette] lib failures ｜ 官方: Palette_85794EFF / libraries_01CFC309（旧称「[素材] 图库」）

// ───────────────────────── 保存失败原因（拼进上屏/诊断文案） ─────────────────────────

/// 写临时文件失败。
pub const SAVE_ERR_TEMP: &str = "写临时文件 {path} 失败：{err}"; // EN: write temp failed
/// 临时文件 `sync_all()` 失败（数据还没保证落盘，因此**不进入** rename 阶段）。
pub const SAVE_ERR_SYNC: &str = "刷盘临时文件 {path} 失败：{err}"; // EN: fsync temp failed
/// 临时文件读回来与内存内容不一致（短写/缓写）。
pub const SAVE_ERR_SHORT: &str = "临时文件 {path} 写入不完整（应 {want} 字节，实 {got} 字节）"; // EN: short write
/// 原子替换失败。
pub const SAVE_ERR_RENAME: &str = "原子替换 {path} 失败（临时文件 {tmp}）：{err}"; // EN: rename failed
/// **目录项 fsync 不可用**（rename 已成功、文件内容已落盘；只是目录项持久化没做成）。
///
/// 这条**不是**保存失败：文件已经在目标路径上是新内容。它只是一句如实交代 ——
/// 断电时"文件内容"与"目录里那条名字指向这个文件"是两件事，后者要刷父目录才保证。
/// Windows 上给目录句柄 `FlushFileBuffers` 通常直接返回 `ERROR_INVALID_HANDLE`
/// （NTFS 不支持对目录句柄刷缓冲），所以这里很可能长年走这条分支 —— 如实打印，不假装成功。
pub const SAVE_ERR_DIRSYNC: &str = "[保存] 目录 fsync 不可用（{path}）：{err} —— 文件内容已落盘，目录项持久化未验证"; // EN: dir fsync unavailable
/// **目录句柄都打不开**（Windows 上几乎只有一个原因：没带 `FILE_FLAG_BACKUP_SEMANTICS`）。
///
/// 与 [`SAVE_ERR_DIRSYNC`] 分开说：一个是"打不开目录"，一个是"打开了刷不动"，
/// 排查方向完全不同（前者是调用参数错，后者是文件系统不支持）。
pub const SAVE_ERR_DIRSYNC_OPEN: &str = "[保存] 打不开目录句柄（{path}）：{err} —— 文件内容已落盘，目录项持久化未验证"; // EN: dir open failed

/// 把模板里的 `{名字}` 占位符按 `kv` 替换成真实值。
///
/// 只做**字面替换**，不做格式化语法（模板里不会出现 `{:.2}` 这类格式说明符）：
/// 数值先由调用方按需要格成字符串，再传进来 —— 这样"排版格式"这件事只在一个地方决定。
pub fn fill(tmpl: &str, kv: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(tmpl.len() + 16);
    out.push_str(tmpl);
    for (k, v) in kv {
        out = out.replace(&format!("{{{k}}}"), v);
    }
    out
}

// ───────────────────────── 全量清单（字形测试用） ─────────────────────────

/// 全部界面文案 `(键名, 文案)`。
///
/// 字形测试 [`crate::fonts::tests`] 遍历的就是这张表；
/// `ui_text::tests::清单覆盖所有常量` 会反向核对"模块里每个常量都在清单里"，
/// 防止新增文案时漏进测试。
pub const ALL: &[(&str, &str)] = &[
    ("TITLE_TMPL", TITLE_TMPL),
    ("WINDOW_TITLE_EMPTY", WINDOW_TITLE_EMPTY),
    ("TITLE_DIRTY_TMPL", TITLE_DIRTY_TMPL),
    ("TOOL_BRUSH", TOOL_BRUSH),
    ("TOOL_ERASER", TOOL_ERASER),
    ("TOOL_PICKUP", TOOL_PICKUP),
    ("TOOL_BUCKET", TOOL_BUCKET),
    ("TOOL_UNWIRED", TOOL_UNWIRED),
    ("TOOL_SELECT", TOOL_SELECT),
    ("TOOL_STAMP", TOOL_STAMP),
    ("MENU_FILE", MENU_FILE),
    ("MENU_EDIT", MENU_EDIT),
    ("MENU_VIEW", MENU_VIEW),
    ("MENU_TOOLS", MENU_TOOLS),
    ("MENU_HELP", MENU_HELP),
    ("BRAND", BRAND),
    // 下拉菜单 39 项（2026-11-05）
    ("MENU_NEW_MAP", MENU_NEW_MAP),
    ("MENU_OPEN_MAP", MENU_OPEN_MAP),
    ("MENU_OPEN_RECENT", MENU_OPEN_RECENT),
    ("MENU_SAVE_MAP", MENU_SAVE_MAP),
    ("MENU_SAVE_AS", MENU_SAVE_AS),
    ("MENU_EXPORT_MINIMAP", MENU_EXPORT_MINIMAP),
    ("MENU_NEW_STAMP", MENU_NEW_STAMP),
    ("MENU_IMPORT_STAMP", MENU_IMPORT_STAMP),
    ("MENU_EXPORT_SELECTION", MENU_EXPORT_SELECTION),
    ("MENU_EXIT", MENU_EXIT),
    ("MENU_UNDO", MENU_UNDO),
    ("MENU_REDO", MENU_REDO),
    ("MENU_FILL_SELECTION", MENU_FILL_SELECTION),
    ("MENU_SETTINGS", MENU_SETTINGS),
    ("MENU_DARK_MODE", MENU_DARK_MODE),
    ("MENU_CELL_GRID", MENU_CELL_GRID),
    ("MENU_MAP_LIGHTING", MENU_MAP_LIGHTING),
    ("MENU_OPEN_DOORS", MENU_OPEN_DOORS),
    ("MENU_ZOOM_IN", MENU_ZOOM_IN),
    ("MENU_ZOOM_OUT", MENU_ZOOM_OUT),
    ("MENU_ACTUAL_SIZE", MENU_ACTUAL_SIZE),
    ("MENU_FIT_MAP", MENU_FIT_MAP),
    ("MENU_LANGUAGE", MENU_LANGUAGE),
    ("MENU_LOCK_LAYOUT", MENU_LOCK_LAYOUT),
    ("MENU_RESET_LAYOUT", MENU_RESET_LAYOUT),
    ("MENU_GEN_TOWN", MENU_GEN_TOWN),
    ("MENU_GEN_TERRAIN", MENU_GEN_TERRAIN),
    ("MENU_GEN_PROVINCE", MENU_GEN_PROVINCE),
    ("MENU_GEN_DUNGEON", MENU_GEN_DUNGEON),
    ("MENU_IDENTIFY_OBJECTS", MENU_IDENTIFY_OBJECTS),
    ("MENU_EXTRACT_OBJECTS", MENU_EXTRACT_OBJECTS),
    ("MENU_KEYBOARD_SHORTCUTS", MENU_KEYBOARD_SHORTCUTS),
    ("STATUS_MENU_TODO_TMPL", STATUS_MENU_TODO_TMPL),
    ("STATUS_MENU_NEED_MAP_TMPL", STATUS_MENU_NEED_MAP_TMPL),
    ("WHY_NEED_MAP", WHY_NEED_MAP),
    ("MENU_OPEN_OK_TMPL", MENU_OPEN_OK_TMPL),
    ("MENU_OPEN_FAIL_TMPL", MENU_OPEN_FAIL_TMPL),
    ("MENU_SAVE_OK_TMPL", MENU_SAVE_OK_TMPL),
    ("MENU_SAVE_CLEAN", MENU_SAVE_CLEAN),
    ("MENU_SAVE_FAIL_TMPL", MENU_SAVE_FAIL_TMPL),
    ("MENU_SAVE_AS_OK_TMPL", MENU_SAVE_AS_OK_TMPL),
    ("MENU_SAVE_AS_FAIL_TMPL", MENU_SAVE_AS_FAIL_TMPL),
    // 导出 PNG（2026-11-05 接线）
    ("MENU_EXPORT_SEL_OK_TMPL", MENU_EXPORT_SEL_OK_TMPL),
    ("MENU_EXPORT_MINI_OK_TMPL", MENU_EXPORT_MINI_OK_TMPL),
    ("MENU_EXPORT_FAIL_TMPL", MENU_EXPORT_FAIL_TMPL),
    ("EXPORT_SEL_DLG_TITLE", EXPORT_SEL_DLG_TITLE),
    ("EXPORT_SEL_DLG_HINT", EXPORT_SEL_DLG_HINT),
    ("EXPORT_SEL_DLG_OK", EXPORT_SEL_DLG_OK),
    ("EXPORT_MINI_DLG_TITLE", EXPORT_MINI_DLG_TITLE),
    ("EXPORT_MINI_DLG_HINT", EXPORT_MINI_DLG_HINT),
    ("EXPORT_MINI_DLG_OK", EXPORT_MINI_DLG_OK),
    ("EXPORT_ERR_EMPTY", EXPORT_ERR_EMPTY),
    ("EXPORT_ERR_IS_DIR_TMPL", EXPORT_ERR_IS_DIR_TMPL),
    ("EXPORT_OVERWROTE", EXPORT_OVERWROTE),
    ("EXPORT_TWO_STAGE", EXPORT_TWO_STAGE),
    ("EXPORT_ERR_BAD_MAX_SIDE", EXPORT_ERR_BAD_MAX_SIDE),
    ("DIAG_EXPORT_OK", DIAG_EXPORT_OK),
    ("DIAG_EXPORT_FAIL", DIAG_EXPORT_FAIL),
    ("MENU_UNDO_OK_TMPL", MENU_UNDO_OK_TMPL),
    ("MENU_UNDO_NONE", MENU_UNDO_NONE),
    ("MENU_REDO_OK_TMPL", MENU_REDO_OK_TMPL),
    ("MENU_REDO_NONE", MENU_REDO_NONE),
    ("MENU_ZOOM_TMPL", MENU_ZOOM_TMPL),
    ("MENU_ACTUAL_SIZE_OK", MENU_ACTUAL_SIZE_OK),
    ("MENU_FIT_OK_TMPL", MENU_FIT_OK_TMPL),
    ("MENU_PANEL_HIDE_TMPL", MENU_PANEL_HIDE_TMPL),
    ("MENU_PANEL_SHOW_TMPL", MENU_PANEL_SHOW_TMPL),
    ("MENU_DARK_ON", MENU_DARK_ON),
    ("MENU_DARK_OFF", MENU_DARK_OFF),
    ("MENU_CELL_GRID_ON", MENU_CELL_GRID_ON),
    ("MENU_CELL_GRID_OFF", MENU_CELL_GRID_OFF),
    ("MENU_OPEN_DOORS_ON", MENU_OPEN_DOORS_ON),
    ("MENU_OPEN_DOORS_OFF", MENU_OPEN_DOORS_OFF),
    ("MENU_LIGHT_DAWN_DUSK", MENU_LIGHT_DAWN_DUSK),
    ("MENU_LIGHT_DAY", MENU_LIGHT_DAY),
    ("MENU_LIGHT_NIGHT", MENU_LIGHT_NIGHT),
    ("STATUS_LIGHT_CONTAINER", STATUS_LIGHT_CONTAINER),
    ("WHY_NEW_MAP", WHY_NEW_MAP),
    ("WHY_NEW_STAMP", WHY_NEW_STAMP),
    ("WHY_IMPORT_STAMP", WHY_IMPORT_STAMP),
    ("WHY_EXIT", WHY_EXIT),
    ("WHY_SETTINGS", WHY_SETTINGS),
    ("WHY_LANGUAGE", WHY_LANGUAGE),
    ("WHY_LOCK_LAYOUT", WHY_LOCK_LAYOUT),
    ("WHY_RESET_LAYOUT", WHY_RESET_LAYOUT),
    ("WHY_GEN_TOWN", WHY_GEN_TOWN),
    ("WHY_GEN_TERRAIN", WHY_GEN_TERRAIN),
    ("WHY_GEN_PROVINCE", WHY_GEN_PROVINCE),
    ("WHY_GEN_DUNGEON", WHY_GEN_DUNGEON),
    ("WHY_IDENTIFY_OBJECTS", WHY_IDENTIFY_OBJECTS),
    ("WHY_EXTRACT_OBJECTS", WHY_EXTRACT_OBJECTS),

    // 快捷键浮层
    ("SHORTCUTS_TITLE", SHORTCUTS_TITLE),
    ("SHORTCUTS_NOTE", SHORTCUTS_NOTE),
    ("SC_GROUP_FILE", SC_GROUP_FILE),
    ("SC_GROUP_EDIT", SC_GROUP_EDIT),
    ("SC_GROUP_VIEW", SC_GROUP_VIEW),
    ("SC_GROUP_TOOLS", SC_GROUP_TOOLS),
    ("SC_GROUP_SELECTION", SC_GROUP_SELECTION),
    ("SC_GROUP_MOUSE", SC_GROUP_MOUSE),
    ("SC_SAVE", SC_SAVE),
    ("SC_SAVE_AS", SC_SAVE_AS),
    ("SC_OPEN", SC_OPEN),
    ("SC_UNDO", SC_UNDO),
    ("SC_REDO", SC_REDO),
    ("SC_TOOL_FILL", SC_TOOL_FILL),
    ("SC_TOOL_SELECT", SC_TOOL_SELECT),
    ("SC_TOOL_STAMP", SC_TOOL_STAMP),
    ("SC_BRUSH_SIZE", SC_BRUSH_SIZE),
    ("SC_ZOOM_IN", SC_ZOOM_IN),
    ("SC_ZOOM_OUT", SC_ZOOM_OUT),
    ("SC_FIT", SC_FIT),
    ("SC_COPY", SC_COPY),
    ("SC_CUT", SC_CUT),
    ("SC_PASTE", SC_PASTE),
    ("SC_DELETE", SC_DELETE),
    ("SC_FILL_SEL", SC_FILL_SEL),
    ("SC_ARM_STAMP", SC_ARM_STAMP),
    ("SC_ESC", SC_ESC),
    ("SC_ALT_OUTLINE", SC_ALT_OUTLINE),
    ("SC_PAN", SC_PAN),
    ("SC_WHEEL_ZOOM", SC_WHEEL_ZOOM),
    ("SC_OPEN_OVERLAY", SC_OPEN_OVERLAY),
    ("SC_TOGGLE_GRID", SC_TOGGLE_GRID),
    ("KEY_CTRL_N", KEY_CTRL_N),
    ("KEY_CTRL_O", KEY_CTRL_O),
    ("KEY_CTRL_S", KEY_CTRL_S),
    ("KEY_CTRL_SHIFT_S", KEY_CTRL_SHIFT_S),
    ("KEY_CTRL_SHIFT_N", KEY_CTRL_SHIFT_N),
    ("KEY_G", KEY_G),
    ("KEY_CTRL_Z", KEY_CTRL_Z),
    ("KEY_CTRL_SHIFT_Z", KEY_CTRL_SHIFT_Z),
    ("KEY_CTRL_C", KEY_CTRL_C),
    ("KEY_CTRL_X", KEY_CTRL_X),
    ("KEY_CTRL_V", KEY_CTRL_V),
    ("KEY_CTRL_SHIFT_V", KEY_CTRL_SHIFT_V),
    ("KEY_F1", KEY_F1),
    ("KEY_F", KEY_F),
    ("KEY_M", KEY_M),
    ("KEY_O", KEY_O),
    ("KEY_BRACKETS", KEY_BRACKETS),
    ("KEY_ENTER", KEY_ENTER),
    ("KEY_ESC", KEY_ESC),
    ("KEY_DELETE", KEY_DELETE),
    ("KEY_ALT", KEY_ALT),
    ("KEY_SPACE_DRAG", KEY_SPACE_DRAG),
    ("KEY_MIDDLE_DRAG", KEY_MIDDLE_DRAG),
    ("KEY_WHEEL", KEY_WHEEL),
    // 打开地图模态 / 诊断
    ("OPEN_DLG_TITLE", OPEN_DLG_TITLE),
    ("OPEN_DLG_HINT", OPEN_DLG_HINT),
    ("OPEN_DLG_OK", OPEN_DLG_OK),
    ("OPEN_DLG_CANCEL", OPEN_DLG_CANCEL),
    ("SAVE_AS_DLG_TITLE", SAVE_AS_DLG_TITLE),
    ("SAVE_AS_DLG_HINT", SAVE_AS_DLG_HINT),
    ("SAVE_AS_DLG_OK", SAVE_AS_DLG_OK),
    ("SAVE_AS_ERR_EMPTY", SAVE_AS_ERR_EMPTY),
    ("SAVE_AS_ERR_IS_DIR_TMPL", SAVE_AS_ERR_IS_DIR_TMPL),
    ("DIAG_MENU_DUMP_HEAD", DIAG_MENU_DUMP_HEAD),
    ("DIAG_MENU_DUMP_ROW", DIAG_MENU_DUMP_ROW),
    ("DIAG_MENU_DUMP_TAIL", DIAG_MENU_DUMP_TAIL),
    ("DIAG_MENU_CLICK", DIAG_MENU_CLICK),
    ("DIAG_CLICK_TOO_LATE", DIAG_CLICK_TOO_LATE),
    ("DIAG_SAVE_AS_OK", DIAG_SAVE_AS_OK),
    ("DIAG_SAVE_AS_FAIL", DIAG_SAVE_AS_FAIL),
    ("DIAG_SAVE_AS_TITLE", DIAG_SAVE_AS_TITLE),
    ("DIAG_SAVE_AS_STATUS", DIAG_SAVE_AS_STATUS),
    ("DIAG_MENU_GEOM", DIAG_MENU_GEOM),
    ("DIAG_SETTINGS_SAVED", DIAG_SETTINGS_SAVED),
    ("DIAG_SETTINGS_LOADED", DIAG_SETTINGS_LOADED),
    ("DIAG_SETTINGS_FALLBACK", DIAG_SETTINGS_FALLBACK),
    ("STATE_READY", STATE_READY),
    ("STATE_NEED_MAP", STATE_NEED_MAP),
    ("STATE_TODO_TMPL", STATE_TODO_TMPL),
    ("TITLE_TOOLS", TITLE_TOOLS),
    ("TITLE_TOOL_OPTIONS", TITLE_TOOL_OPTIONS),
    ("TITLE_LAYERS", TITLE_LAYERS),
    ("TITLE_CELL_INSPECTOR", TITLE_CELL_INSPECTOR),
    ("TITLE_MINIMAP", TITLE_MINIMAP),
    ("TITLE_PALETTE", TITLE_PALETTE),
    ("TITLE_ACTIVE_ASSET", TITLE_ACTIVE_ASSET),
    ("TITLE_MAPS", TITLE_MAPS),
    ("PILL_SIZE_TMPL", PILL_SIZE_TMPL),
    ("PILL_PENCIL", PILL_PENCIL),
    ("LAYER_GROUND", LAYER_GROUND),
    ("LAYER_MIDDLE", LAYER_MIDDLE),
    ("LAYER_OBJECTS", LAYER_OBJECTS),
    ("LAYER_LOW_COLLISION", LAYER_LOW_COLLISION),
    ("LAYER_HIGH_COLLISION", LAYER_HIGH_COLLISION),
    ("GROUND_LOCKED_MARK", GROUND_LOCKED_MARK),
    ("GROUP_CELL_HIGHLIGHTS", GROUP_CELL_HIGHLIGHTS),
    ("HIGHLIGHT_LIGHT", HIGHLIGHT_LIGHT),
    ("HIGHLIGHT_DOOR", HIGHLIGHT_DOOR),
    ("INSPECTOR_HEAD_TMPL", INSPECTOR_HEAD_TMPL),
    ("KEY_COLLISION", KEY_COLLISION),
    ("KEY_GROUND", KEY_GROUND),
    ("KEY_MIDDLE", KEY_MIDDLE),
    ("KEY_OBJECTS", KEY_OBJECTS),
    ("KEY_METADATA", KEY_METADATA),
    ("KEY_TILE_ANIMATION", KEY_TILE_ANIMATION),
    ("VAL_UNKNOWN", VAL_UNKNOWN),
    ("VAL_YES", VAL_YES),
    ("VAL_NO", VAL_NO),
    ("VAL_COLLISION_TMPL", VAL_COLLISION_TMPL),
    ("VAL_GROUND_TMPL", VAL_GROUND_TMPL),
    ("VAL_MIDDLE_TMPL", VAL_MIDDLE_TMPL),
    ("VAL_OBJECTS_TMPL", VAL_OBJECTS_TMPL),
    ("VAL_METADATA_TMPL", VAL_METADATA_TMPL),
    ("VAL_TILE_ANIMATION_TMPL", VAL_TILE_ANIMATION_TMPL),
    ("SIZE_TMPL", SIZE_TMPL),
    ("TRUNC_SUFFIX", TRUNC_SUFFIX),
    ("TAB_LIBRARY", TAB_LIBRARY),
    ("TAB_FAVOURITES", TAB_FAVOURITES),
    ("TAB_RECENT", TAB_RECENT),
    ("TAB_CATEGORIES", TAB_CATEGORIES),
    ("PALETTE_LIB_LABEL", PALETTE_LIB_LABEL),
    ("PALETTE_FRAMES_TMPL", PALETTE_FRAMES_TMPL),
    ("PALETTE_LIB_ROW_TMPL", PALETTE_LIB_ROW_TMPL),
    ("PALETTE_RANGE_TMPL", PALETTE_RANGE_TMPL),
    ("PALETTE_EMPTY_FRAMES_TMPL", PALETTE_EMPTY_FRAMES_TMPL),
    ("PALETTE_LIB_EMPTY", PALETTE_LIB_EMPTY),
    ("PALETTE_TAB_UNIMPLEMENTED", PALETTE_TAB_UNIMPLEMENTED),
    ("PALETTE_SEARCH_NEED_DIGITS", PALETTE_SEARCH_NEED_DIGITS),
    ("PALETTE_SEARCH_OUT_OF_RANGE_TMPL", PALETTE_SEARCH_OUT_OF_RANGE_TMPL),
    ("PALETTE_SEARCH_JUMP_TMPL", PALETTE_SEARCH_JUMP_TMPL),
    ("PALETTE_FRAME_TOO_BIG", PALETTE_FRAME_TOO_BIG),
    ("STAMP_KIND_LABEL", STAMP_KIND_LABEL),
    ("STAMP_KIND_ALL", STAMP_KIND_ALL),
    ("STAMP_UNLISTED", STAMP_UNLISTED),
    ("STAMP_UNLISTED_NOTE_TMPL", STAMP_UNLISTED_NOTE_TMPL),
    ("STAMP_KIND_COUNT_TMPL", STAMP_KIND_COUNT_TMPL),
    ("STAMP_COUNT_TMPL", STAMP_COUNT_TMPL),
    ("STAMP_SEARCH_HINT", STAMP_SEARCH_HINT),
    ("STAMP_PAGE_TMPL", STAMP_PAGE_TMPL),
    ("STAMP_NO_MATCH", STAMP_NO_MATCH),
    ("STAMP_LOADING", STAMP_LOADING),
    ("STAMP_LOAD_FAIL_TMPL", STAMP_LOAD_FAIL_TMPL),
    ("STAMP_THREAD_LOST", STAMP_THREAD_LOST),
    ("STAMP_CATALOG_MISSING_TMPL", STAMP_CATALOG_MISSING_TMPL),
    ("STAMP_MISSING", STAMP_MISSING),
    ("STAMP_NO_SOURCE", STAMP_NO_SOURCE),
    ("STAMP_AA_TMPL", STAMP_AA_TMPL),
    ("STAMP_AA_PROMPT", STAMP_AA_PROMPT),
    ("STAMP_AA_GEOM_TMPL", STAMP_AA_GEOM_TMPL),
    ("STAMP_STATUS_TMPL", STAMP_STATUS_TMPL),
    ("STAMP_ARMED_TMPL", STAMP_ARMED_TMPL),
    ("STAMP_ARMED_BADGE_TMPL", STAMP_ARMED_BADGE_TMPL),
    ("STAMP_PLACE_OK_TMPL", STAMP_PLACE_OK_TMPL),
    ("STAMP_PLACE_SKIP_TMPL", STAMP_PLACE_SKIP_TMPL),
    ("STAMP_PLACE_FAIL_TMPL", STAMP_PLACE_FAIL_TMPL),
    ("STAMP_READ_FAIL_TMPL", STAMP_READ_FAIL_TMPL),
    ("STAMP_NO_PATH", STAMP_NO_PATH),
    ("DIAG_STAMP_CATALOG", DIAG_STAMP_CATALOG),
    ("DIAG_STAMP_KIND", DIAG_STAMP_KIND),
    ("DIAG_STAMP_PROBE", DIAG_STAMP_PROBE),
    ("DIAG_STAMP_PIXELS", DIAG_STAMP_PIXELS),
    ("DIAG_STAMP_LRU", DIAG_STAMP_LRU),
    ("DIAG_STAMP_SWEEP", DIAG_STAMP_SWEEP),
    ("DIAG_STAMP", DIAG_STAMP),
    ("DIAG_STAMP_FAIL", DIAG_STAMP_FAIL),
    ("DIAG_STAMP_PICK", DIAG_STAMP_PICK),
    ("DIAG_STAMP_PLACE", DIAG_STAMP_PLACE),
    ("DIAG_STAMP_PLACE_FAIL", DIAG_STAMP_PLACE_FAIL),
    ("DIAG_STAMP_PLACE_NOTE", DIAG_STAMP_PLACE_NOTE),
    ("DIAG_STAMP_ARM", DIAG_STAMP_ARM),
    ("DIAG_SEL_CMD", DIAG_SEL_CMD),
    ("DIAG_SEL_CLI", DIAG_SEL_CLI),
    ("LAYER_IMG_FIELDS", LAYER_IMG_FIELDS[0]),
    ("LAYER_IMG_FIELDS[1]", LAYER_IMG_FIELDS[1]),
    ("LAYER_IMG_FIELDS[2]", LAYER_IMG_FIELDS[2]),
    ("LAYER_LIB_FIELDS", LAYER_LIB_FIELDS[0]),
    ("LAYER_LIB_FIELDS[1]", LAYER_LIB_FIELDS[1]),
    ("LAYER_LIB_FIELDS[2]", LAYER_LIB_FIELDS[2]),
    ("SEARCH_HINT", SEARCH_HINT),
    ("EMPTY_TITLE", EMPTY_TITLE),
    ("EMPTY_DESC_0", EMPTY_DESC_0),
    ("EMPTY_DESC_1", EMPTY_DESC_1),
    ("EMPTY_ACTION", EMPTY_ACTION),
    ("PAGE_LABEL_TMPL", PAGE_LABEL_TMPL),
    ("AA_LABEL", AA_LABEL),
    ("AA_SELECTED_TMPL", AA_SELECTED_TMPL),
    ("AA_PIN", AA_PIN),
    ("STATUS_LEFT_TMPL", STATUS_LEFT_TMPL),
    ("STATUS_MAP_FAIL_TMPL", STATUS_MAP_FAIL_TMPL),
    ("STATUS_LIB_FAIL_TMPL", STATUS_LIB_FAIL_TMPL),
    ("STATUS_NO_MAP", STATUS_NO_MAP),
    ("STATUS_COORD_TMPL", STATUS_COORD_TMPL),
    ("STATUS_LIB_TMPL", STATUS_LIB_TMPL),
    ("STATUS_FORMAT_TMPL", STATUS_FORMAT_TMPL),
    ("STATUS_GROUND_BLOCKED_TMPL", STATUS_GROUND_BLOCKED_TMPL),
    ("BANNER_TEXT_TMPL", BANNER_TEXT_TMPL),
    ("STATUS_TOOL_TMPL", STATUS_TOOL_TMPL),
    ("STATUS_BRUSH_TMPL", STATUS_BRUSH_TMPL),
    ("STATUS_BRUSH_NONE", STATUS_BRUSH_NONE),
    ("STATUS_LAYER_TMPL", STATUS_LAYER_TMPL),
    ("STATUS_WRITE_TMPL", STATUS_WRITE_TMPL),
    ("STATUS_WRITE_12B_TMPL", STATUS_WRITE_12B_TMPL),
    ("STATUS_WRITE_COLLISION_TMPL", STATUS_WRITE_COLLISION_TMPL),
    ("STATUS_HINT_NO_MAP", STATUS_HINT_NO_MAP),
    ("EDIT_LAYER_MISMATCH_TMPL", EDIT_LAYER_MISMATCH_TMPL),
    ("EDIT_COLLISION_BRUSH_TMPL", EDIT_COLLISION_BRUSH_TMPL),
    ("STATUS_SAVE_FAIL_TMPL", STATUS_SAVE_FAIL_TMPL),
    ("STATUS_FILL_TMPL", STATUS_FILL_TMPL),
    ("STATUS_FILL_COLLISION_TMPL", STATUS_FILL_COLLISION_TMPL),
    ("STATUS_SEL_TMPL", STATUS_SEL_TMPL),
    ("STATUS_SEL_CLIP_TMPL", STATUS_SEL_CLIP_TMPL),
    ("STATUS_CLIP_TMPL", STATUS_CLIP_TMPL),
    ("SEL_KEYS_HINT", SEL_KEYS_HINT),
    ("SEL_COPY_TMPL", SEL_COPY_TMPL),
    ("SEL_CUT_TMPL", SEL_CUT_TMPL),
    ("SEL_PASTE_TMPL", SEL_PASTE_TMPL),
    ("SEL_PASTE_SKIP_TMPL", SEL_PASTE_SKIP_TMPL),
    ("SEL_DELETE_TMPL", SEL_DELETE_TMPL),
    ("SEL_FILL_TMPL", SEL_FILL_TMPL),
    ("SEL_ERR_NO_SELECTION", SEL_ERR_NO_SELECTION),
    ("SEL_ERR_EMPTY_CLIPBOARD", SEL_ERR_EMPTY_CLIPBOARD),
    ("SEL_ERR_NOTHING_TO_DELETE", SEL_ERR_NOTHING_TO_DELETE),
    ("SEL_ERR_NOTHING_TO_FILL", SEL_ERR_NOTHING_TO_FILL),
    ("STATUS_OP_TMPL", STATUS_OP_TMPL),
    ("FILL_ERR_LAYER", FILL_ERR_LAYER),
    ("FILL_ERR_NO_BRUSH", FILL_ERR_NO_BRUSH),
    ("FILL_ERR_OUT_OF_MAP", FILL_ERR_OUT_OF_MAP),
    ("FILL_ERR_TOO_LARGE", FILL_ERR_TOO_LARGE),
    ("TAB_TITLE_TMPL", TAB_TITLE_TMPL),
    ("TAB_TITLE_DIRTY_TMPL", TAB_TITLE_DIRTY_TMPL),
    ("TAB_CLOSE", TAB_CLOSE),
    ("ZOOM_MINUS", ZOOM_MINUS),
    ("ZOOM_PCT_TMPL", ZOOM_PCT_TMPL),
    ("ZOOM_PLUS", ZOOM_PLUS),
    ("ZOOM_FIT", ZOOM_FIT),
    ("ZOOM_ORIGINAL", ZOOM_ORIGINAL),
    ("MAPLIST_REFRESH", MAPLIST_REFRESH),
    ("MAPLIST_NAME_TMPL", MAPLIST_NAME_TMPL),
    ("MAPLIST_SCANNING_TMPL", MAPLIST_SCANNING_TMPL),
    ("MAPLIST_STATUS_TMPL", MAPLIST_STATUS_TMPL),
    ("MAPLIST_SKIPPED_TMPL", MAPLIST_SKIPPED_TMPL),
    ("MAPLIST_EMPTY_TMPL", MAPLIST_EMPTY_TMPL),
    ("MAPLIST_FAIL_TMPL", MAPLIST_FAIL_TMPL),
    ("MAPLIST_NO_ROOT", MAPLIST_NO_ROOT),
    ("MAPLIST_TIP_TMPL", MAPLIST_TIP_TMPL),
    ("MAPLIST_ERR_ROOT", MAPLIST_ERR_ROOT),
    ("MAPLIST_ERR_NOT_DIR", MAPLIST_ERR_NOT_DIR),
    ("MAPLIST_ERR_DIR", MAPLIST_ERR_DIR),
    ("MAPLIST_ERR_FILE", MAPLIST_ERR_FILE),
    ("MAPLIST_ERR_ENTRY", MAPLIST_ERR_ENTRY),
    ("DIAG_MAPLIST_SCAN", DIAG_MAPLIST_SCAN),
    ("DIAG_MAPLIST_FAIL", DIAG_MAPLIST_FAIL),
    ("SIZE_VALUE_TMPL", SIZE_VALUE_TMPL),
    ("SIZE_UNIT_B", SIZE_UNIT_B),
    ("SIZE_UNIT_KB", SIZE_UNIT_KB),
    ("SIZE_UNIT_MB", SIZE_UNIT_MB),
    ("SIZE_UNIT_GB", SIZE_UNIT_GB),
    ("CANVAS_NO_MAP", CANVAS_NO_MAP),
    ("DIAG_MAP_NONE", DIAG_MAP_NONE),
    ("SEARCH_CLEAR", SEARCH_CLEAR),
    ("CAPTION_CLOSE", CAPTION_CLOSE),
    ("CAPTION_MAXIMIZE", CAPTION_MAXIMIZE),
    ("CAPTION_MINIMIZE", CAPTION_MINIMIZE),
    ("PAGER_PREV", PAGER_PREV),
    ("PAGER_NEXT", PAGER_NEXT),
    ("PIN_STAR", PIN_STAR),
    ("CLOSE_CONFIRM", CLOSE_CONFIRM),
    ("DIAG_MAP_OK", DIAG_MAP_OK),
    ("DIAG_MAP_FAIL", DIAG_MAP_FAIL),
    ("DIAG_MAP_WARN", DIAG_MAP_WARN),
    ("DIAG_LIB_OK", DIAG_LIB_OK),
    ("DIAG_LIB_FAIL", DIAG_LIB_FAIL),
    ("DIAG_RENDER", DIAG_RENDER),
    ("DIAG_RENDER_BUDGET", DIAG_RENDER_BUDGET),
    ("DIAG_AUTODRAG", DIAG_AUTODRAG),
    ("DIAG_SHOT_OK", DIAG_SHOT_OK),
    ("DIAG_SHOT_FAIL", DIAG_SHOT_FAIL),
    ("DIAG_GATE2", DIAG_GATE2),
    ("DIAG_GATE2_RESULT", DIAG_GATE2_RESULT),
    ("DIAG_GATE2_OVERLAY", DIAG_GATE2_OVERLAY),
    ("DIAG_PROBE", DIAG_PROBE),
    ("DIAG_DUMP", DIAG_DUMP),
    ("DIAG_BAD_ARG", DIAG_BAD_ARG),
    ("DIAG_START", DIAG_START),
    ("DIAG_EXIT", DIAG_EXIT),
    ("DIAG_EXIT_ERR", DIAG_EXIT_ERR),
    ("DIAG_USAGE", DIAG_USAGE),
    ("DIAG_EDIT_NO_MAP", DIAG_EDIT_NO_MAP),
    ("DIAG_EDIT_PICK", DIAG_EDIT_PICK),
    ("DIAG_EDIT_PICK_CELL", DIAG_EDIT_PICK_CELL),
    ("DIAG_EDIT_LAYER", DIAG_EDIT_LAYER),
    ("DIAG_EDIT_LAYER_COLLISION", DIAG_EDIT_LAYER_COLLISION),
    ("DIAG_EDIT_REJECT", DIAG_EDIT_REJECT),
    ("DIAG_EDIT_GROUND_BLOCKED", DIAG_EDIT_GROUND_BLOCKED),
    ("DIAG_EDIT_BIND_FAIL", DIAG_EDIT_BIND_FAIL),
    ("DIAG_EDIT_PICK_NONE", DIAG_EDIT_PICK_NONE),
    ("DIAG_EDIT_BRUSH", DIAG_EDIT_BRUSH),
    ("DIAG_EDIT_ERASE", DIAG_EDIT_ERASE),
    ("DIAG_EDIT_FILL", DIAG_EDIT_FILL),
    ("DIAG_EDIT_FILL_REFUSED", DIAG_EDIT_FILL_REFUSED),
    ("DIAG_EDIT_SIZE", DIAG_EDIT_SIZE),
    ("DIAG_EDIT_STROKE", DIAG_EDIT_STROKE),
    ("DIAG_EDIT_UNDO", DIAG_EDIT_UNDO),
    ("DIAG_EDIT_REDO", DIAG_EDIT_REDO),
    ("DIAG_EDIT_CMP_SAME", DIAG_EDIT_CMP_SAME),
    ("DIAG_EDIT_CMP_DIFF", DIAG_EDIT_CMP_DIFF),
    ("DIAG_EDIT_CMP_ERR", DIAG_EDIT_CMP_ERR),
    ("DIAG_EDIT_SAVE_SKIP", DIAG_EDIT_SAVE_SKIP),
    ("DIAG_EDIT_SAVE_OK", DIAG_EDIT_SAVE_OK),
    ("DIAG_EDIT_SAVE_FAIL", DIAG_EDIT_SAVE_FAIL),
    ("DIAG_EDIT_SAVE_NOMAP", DIAG_EDIT_SAVE_NOMAP),
    ("DIAG_SAVE_DIRSYNC_OK", DIAG_SAVE_DIRSYNC_OK),
    ("DIAG_PALETTE", DIAG_PALETTE),
    ("DIAG_PALETTE_SCROLL", DIAG_PALETTE_SCROLL),
    ("DIAG_PALETTE_SEARCH", DIAG_PALETTE_SEARCH),
    ("DIAG_PALETTE_PICK", DIAG_PALETTE_PICK),
    ("DIAG_PALETTE_FAIL", DIAG_PALETTE_FAIL),
    // 2026-11-05 补漏：网格那单只把 DIAG_GRID 加进了测试里的局部清单，漏了 ALL，
    // 被守卫测试 `清单覆盖所有常量` 抓到（字形测试会漏查这条文案的字符）。
    ("DIAG_GRID", DIAG_GRID),
    ("SAVE_ERR_TEMP", SAVE_ERR_TEMP),
    ("SAVE_ERR_SYNC", SAVE_ERR_SYNC),
    ("SAVE_ERR_SHORT", SAVE_ERR_SHORT),
    ("SAVE_ERR_RENAME", SAVE_ERR_RENAME),
    ("SAVE_ERR_DIRSYNC", SAVE_ERR_DIRSYNC),
    ("SAVE_ERR_DIRSYNC_OPEN", SAVE_ERR_DIRSYNC_OPEN),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// 模块里每一个「界面文案常量」都必须出现在 [`ALL`] 里，否则字形测试漏查。
    #[test]
    fn 清单覆盖所有常量() {
        let listed: Vec<&str> = ALL.iter().map(|(_, s)| *s).collect();
        let consts: &[(&str, &str)] = &[
            ("TITLE_TMPL", TITLE_TMPL),
            ("WINDOW_TITLE_EMPTY", WINDOW_TITLE_EMPTY),
            ("TITLE_DIRTY_TMPL", TITLE_DIRTY_TMPL),
            ("TOOL_BRUSH", TOOL_BRUSH),
            ("TOOL_ERASER", TOOL_ERASER),
            ("TOOL_PICKUP", TOOL_PICKUP),
            ("TOOL_UNWIRED", TOOL_UNWIRED),
            ("MENU_FILE", MENU_FILE),
            ("MENU_EDIT", MENU_EDIT),
            ("MENU_VIEW", MENU_VIEW),
            ("MENU_TOOLS", MENU_TOOLS),
            ("MENU_HELP", MENU_HELP),
            ("BRAND", BRAND),
            ("TITLE_TOOLS", TITLE_TOOLS),
            ("TITLE_TOOL_OPTIONS", TITLE_TOOL_OPTIONS),
            ("TITLE_LAYERS", TITLE_LAYERS),
            ("TITLE_CELL_INSPECTOR", TITLE_CELL_INSPECTOR),
            ("TITLE_MINIMAP", TITLE_MINIMAP),
            ("TITLE_PALETTE", TITLE_PALETTE),
            ("TITLE_ACTIVE_ASSET", TITLE_ACTIVE_ASSET),
            ("TITLE_MAPS", TITLE_MAPS),
            ("PILL_SIZE_TMPL", PILL_SIZE_TMPL),
    ("PILL_PENCIL", PILL_PENCIL),
            ("LAYER_GROUND", LAYER_GROUND),
            ("LAYER_MIDDLE", LAYER_MIDDLE),
            ("LAYER_OBJECTS", LAYER_OBJECTS),
            ("LAYER_LOW_COLLISION", LAYER_LOW_COLLISION),
            ("LAYER_HIGH_COLLISION", LAYER_HIGH_COLLISION),
            ("GROUND_LOCKED_MARK", GROUND_LOCKED_MARK),
            ("GROUP_CELL_HIGHLIGHTS", GROUP_CELL_HIGHLIGHTS),
            ("HIGHLIGHT_LIGHT", HIGHLIGHT_LIGHT),
            ("HIGHLIGHT_DOOR", HIGHLIGHT_DOOR),
            ("INSPECTOR_HEAD_TMPL", INSPECTOR_HEAD_TMPL),
            ("KEY_COLLISION", KEY_COLLISION),
            ("KEY_GROUND", KEY_GROUND),
            ("KEY_MIDDLE", KEY_MIDDLE),
            ("KEY_OBJECTS", KEY_OBJECTS),
            ("KEY_METADATA", KEY_METADATA),
            ("KEY_TILE_ANIMATION", KEY_TILE_ANIMATION),
            ("VAL_UNKNOWN", VAL_UNKNOWN),
            ("VAL_YES", VAL_YES),
            ("VAL_NO", VAL_NO),
            ("VAL_COLLISION_TMPL", VAL_COLLISION_TMPL),
            ("VAL_GROUND_TMPL", VAL_GROUND_TMPL),
            ("VAL_MIDDLE_TMPL", VAL_MIDDLE_TMPL),
            ("VAL_OBJECTS_TMPL", VAL_OBJECTS_TMPL),
            ("VAL_METADATA_TMPL", VAL_METADATA_TMPL),
            ("VAL_TILE_ANIMATION_TMPL", VAL_TILE_ANIMATION_TMPL),
            ("SIZE_TMPL", SIZE_TMPL),
            ("TRUNC_SUFFIX", TRUNC_SUFFIX),
            ("TAB_LIBRARY", TAB_LIBRARY),
            ("TAB_FAVOURITES", TAB_FAVOURITES),
            ("TAB_RECENT", TAB_RECENT),
            ("TAB_CATEGORIES", TAB_CATEGORIES),
            ("PALETTE_LIB_LABEL", PALETTE_LIB_LABEL),
            ("PALETTE_FRAMES_TMPL", PALETTE_FRAMES_TMPL),
            ("PALETTE_LIB_ROW_TMPL", PALETTE_LIB_ROW_TMPL),
            ("PALETTE_RANGE_TMPL", PALETTE_RANGE_TMPL),
            ("PALETTE_EMPTY_FRAMES_TMPL", PALETTE_EMPTY_FRAMES_TMPL),
            ("PALETTE_LIB_EMPTY", PALETTE_LIB_EMPTY),
            ("PALETTE_TAB_UNIMPLEMENTED", PALETTE_TAB_UNIMPLEMENTED),
            ("PALETTE_SEARCH_NEED_DIGITS", PALETTE_SEARCH_NEED_DIGITS),
            ("PALETTE_SEARCH_OUT_OF_RANGE_TMPL", PALETTE_SEARCH_OUT_OF_RANGE_TMPL),
            ("PALETTE_SEARCH_JUMP_TMPL", PALETTE_SEARCH_JUMP_TMPL),
            ("PALETTE_FRAME_TOO_BIG", PALETTE_FRAME_TOO_BIG),
            ("STAMP_KIND_LABEL", STAMP_KIND_LABEL),
            ("STAMP_KIND_ALL", STAMP_KIND_ALL),
            ("STAMP_UNLISTED", STAMP_UNLISTED),
            ("STAMP_UNLISTED_NOTE_TMPL", STAMP_UNLISTED_NOTE_TMPL),
            ("STAMP_KIND_COUNT_TMPL", STAMP_KIND_COUNT_TMPL),
            ("STAMP_COUNT_TMPL", STAMP_COUNT_TMPL),
            ("STAMP_SEARCH_HINT", STAMP_SEARCH_HINT),
            ("STAMP_PAGE_TMPL", STAMP_PAGE_TMPL),
            ("STAMP_NO_MATCH", STAMP_NO_MATCH),
            ("STAMP_LOADING", STAMP_LOADING),
            ("STAMP_LOAD_FAIL_TMPL", STAMP_LOAD_FAIL_TMPL),
            ("STAMP_THREAD_LOST", STAMP_THREAD_LOST),
            ("STAMP_CATALOG_MISSING_TMPL", STAMP_CATALOG_MISSING_TMPL),
            ("STAMP_MISSING", STAMP_MISSING),
            ("STAMP_NO_SOURCE", STAMP_NO_SOURCE),
            ("STAMP_AA_TMPL", STAMP_AA_TMPL),
            ("STAMP_AA_PROMPT", STAMP_AA_PROMPT),
            ("STAMP_AA_GEOM_TMPL", STAMP_AA_GEOM_TMPL),
            ("STAMP_STATUS_TMPL", STAMP_STATUS_TMPL),
            ("DIAG_STAMP_CATALOG", DIAG_STAMP_CATALOG),
            ("DIAG_STAMP_KIND", DIAG_STAMP_KIND),
            ("DIAG_STAMP_PROBE", DIAG_STAMP_PROBE),
            ("DIAG_STAMP_PIXELS", DIAG_STAMP_PIXELS),
            ("DIAG_STAMP_LRU", DIAG_STAMP_LRU),
            ("DIAG_STAMP_SWEEP", DIAG_STAMP_SWEEP),
            ("DIAG_STAMP", DIAG_STAMP),
            ("DIAG_STAMP_FAIL", DIAG_STAMP_FAIL),
            ("DIAG_STAMP_PICK", DIAG_STAMP_PICK),
            ("DIAG_STAMP_PLACE", DIAG_STAMP_PLACE),
            ("DIAG_STAMP_PLACE_FAIL", DIAG_STAMP_PLACE_FAIL),
    ("DIAG_STAMP_PLACE_NOTE", DIAG_STAMP_PLACE_NOTE),
    ("DIAG_STAMP_ARM", DIAG_STAMP_ARM),
    ("DIAG_SEL_CMD", DIAG_SEL_CMD),
    ("DIAG_SEL_CLI", DIAG_SEL_CLI),
            ("SEARCH_HINT", SEARCH_HINT),
            ("EMPTY_TITLE", EMPTY_TITLE),
            ("EMPTY_ACTION", EMPTY_ACTION),
            ("PAGE_LABEL_TMPL", PAGE_LABEL_TMPL),
            ("AA_LABEL", AA_LABEL),
            ("AA_SELECTED_TMPL", AA_SELECTED_TMPL),
            ("AA_PIN", AA_PIN),
            ("STATUS_LEFT_TMPL", STATUS_LEFT_TMPL),
            ("STATUS_MAP_FAIL_TMPL", STATUS_MAP_FAIL_TMPL),
            ("STATUS_LIB_FAIL_TMPL", STATUS_LIB_FAIL_TMPL),
            ("STATUS_NO_MAP", STATUS_NO_MAP),
            ("STATUS_COORD_TMPL", STATUS_COORD_TMPL),
            ("STATUS_LIB_TMPL", STATUS_LIB_TMPL),
            ("STATUS_FORMAT_TMPL", STATUS_FORMAT_TMPL),
            ("STATUS_GROUND_BLOCKED_TMPL", STATUS_GROUND_BLOCKED_TMPL),
    ("BANNER_TEXT_TMPL", BANNER_TEXT_TMPL),
            ("STATUS_TOOL_TMPL", STATUS_TOOL_TMPL),
            ("STATUS_BRUSH_TMPL", STATUS_BRUSH_TMPL),
            ("STATUS_BRUSH_NONE", STATUS_BRUSH_NONE),
            ("STATUS_LAYER_TMPL", STATUS_LAYER_TMPL),
            ("STATUS_WRITE_TMPL", STATUS_WRITE_TMPL),
            ("STATUS_WRITE_12B_TMPL", STATUS_WRITE_12B_TMPL),
            ("STATUS_WRITE_COLLISION_TMPL", STATUS_WRITE_COLLISION_TMPL),
            ("STATUS_HINT_NO_MAP", STATUS_HINT_NO_MAP),
            ("EDIT_LAYER_MISMATCH_TMPL", EDIT_LAYER_MISMATCH_TMPL),
            ("EDIT_COLLISION_BRUSH_TMPL", EDIT_COLLISION_BRUSH_TMPL),
            ("STATUS_SAVE_FAIL_TMPL", STATUS_SAVE_FAIL_TMPL),
    ("STATUS_FILL_TMPL", STATUS_FILL_TMPL),
    ("STATUS_FILL_COLLISION_TMPL", STATUS_FILL_COLLISION_TMPL),
    ("STATUS_SEL_TMPL", STATUS_SEL_TMPL),
    ("STATUS_SEL_CLIP_TMPL", STATUS_SEL_CLIP_TMPL),
    ("STATUS_CLIP_TMPL", STATUS_CLIP_TMPL),
    ("SEL_KEYS_HINT", SEL_KEYS_HINT),
    ("SEL_COPY_TMPL", SEL_COPY_TMPL),
    ("SEL_CUT_TMPL", SEL_CUT_TMPL),
    ("SEL_PASTE_TMPL", SEL_PASTE_TMPL),
    ("SEL_PASTE_SKIP_TMPL", SEL_PASTE_SKIP_TMPL),
    ("SEL_DELETE_TMPL", SEL_DELETE_TMPL),
    ("SEL_FILL_TMPL", SEL_FILL_TMPL),
    ("SEL_ERR_NO_SELECTION", SEL_ERR_NO_SELECTION),
    ("SEL_ERR_EMPTY_CLIPBOARD", SEL_ERR_EMPTY_CLIPBOARD),
    ("SEL_ERR_NOTHING_TO_DELETE", SEL_ERR_NOTHING_TO_DELETE),
    ("SEL_ERR_NOTHING_TO_FILL", SEL_ERR_NOTHING_TO_FILL),
    ("STATUS_OP_TMPL", STATUS_OP_TMPL),
    ("FILL_ERR_LAYER", FILL_ERR_LAYER),
    ("FILL_ERR_NO_BRUSH", FILL_ERR_NO_BRUSH),
    ("FILL_ERR_OUT_OF_MAP", FILL_ERR_OUT_OF_MAP),
    ("FILL_ERR_TOO_LARGE", FILL_ERR_TOO_LARGE),
            ("TAB_TITLE_TMPL", TAB_TITLE_TMPL),
            ("TAB_TITLE_DIRTY_TMPL", TAB_TITLE_DIRTY_TMPL),
            ("TAB_CLOSE", TAB_CLOSE),
            ("ZOOM_MINUS", ZOOM_MINUS),
            ("ZOOM_PCT_TMPL", ZOOM_PCT_TMPL),
            ("ZOOM_PLUS", ZOOM_PLUS),
            ("ZOOM_FIT", ZOOM_FIT),
            ("ZOOM_ORIGINAL", ZOOM_ORIGINAL),
            ("MAPLIST_REFRESH", MAPLIST_REFRESH),
            ("MAPLIST_NAME_TMPL", MAPLIST_NAME_TMPL),
            ("MAPLIST_SCANNING_TMPL", MAPLIST_SCANNING_TMPL),
            ("MAPLIST_STATUS_TMPL", MAPLIST_STATUS_TMPL),
            ("MAPLIST_SKIPPED_TMPL", MAPLIST_SKIPPED_TMPL),
            ("MAPLIST_EMPTY_TMPL", MAPLIST_EMPTY_TMPL),
            ("MAPLIST_FAIL_TMPL", MAPLIST_FAIL_TMPL),
            ("MAPLIST_NO_ROOT", MAPLIST_NO_ROOT),
            ("MAPLIST_TIP_TMPL", MAPLIST_TIP_TMPL),
            ("MAPLIST_ERR_ROOT", MAPLIST_ERR_ROOT),
            ("MAPLIST_ERR_NOT_DIR", MAPLIST_ERR_NOT_DIR),
            ("MAPLIST_ERR_DIR", MAPLIST_ERR_DIR),
            ("MAPLIST_ERR_FILE", MAPLIST_ERR_FILE),
            ("MAPLIST_ERR_ENTRY", MAPLIST_ERR_ENTRY),
            ("DIAG_MAPLIST_SCAN", DIAG_MAPLIST_SCAN),
            ("DIAG_MAPLIST_FAIL", DIAG_MAPLIST_FAIL),
            ("SIZE_VALUE_TMPL", SIZE_VALUE_TMPL),
            ("SIZE_UNIT_B", SIZE_UNIT_B),
            ("SIZE_UNIT_KB", SIZE_UNIT_KB),
            ("SIZE_UNIT_MB", SIZE_UNIT_MB),
            ("SIZE_UNIT_GB", SIZE_UNIT_GB),
            ("CANVAS_NO_MAP", CANVAS_NO_MAP),
            ("DIAG_MAP_NONE", DIAG_MAP_NONE),
            ("SEARCH_CLEAR", SEARCH_CLEAR),
            ("CAPTION_CLOSE", CAPTION_CLOSE),
            ("CAPTION_MAXIMIZE", CAPTION_MAXIMIZE),
            ("CAPTION_MINIMIZE", CAPTION_MINIMIZE),
            ("PAGER_PREV", PAGER_PREV),
            ("PAGER_NEXT", PAGER_NEXT),
            ("PIN_STAR", PIN_STAR),
            ("CLOSE_CONFIRM", CLOSE_CONFIRM),
            ("DIAG_MAP_OK", DIAG_MAP_OK),
            ("DIAG_MAP_FAIL", DIAG_MAP_FAIL),
            ("DIAG_MAP_WARN", DIAG_MAP_WARN),
            ("DIAG_LIB_OK", DIAG_LIB_OK),
            ("DIAG_LIB_FAIL", DIAG_LIB_FAIL),
            ("DIAG_RENDER", DIAG_RENDER),
            ("DIAG_RENDER_BUDGET", DIAG_RENDER_BUDGET),
            ("DIAG_GRID", DIAG_GRID),
            ("DIAG_AUTODRAG", DIAG_AUTODRAG),
            ("DIAG_SHOT_OK", DIAG_SHOT_OK),
            ("DIAG_SHOT_FAIL", DIAG_SHOT_FAIL),
            ("DIAG_GATE2", DIAG_GATE2),
            ("DIAG_GATE2_RESULT", DIAG_GATE2_RESULT),
            ("DIAG_GATE2_OVERLAY", DIAG_GATE2_OVERLAY),
            ("DIAG_PROBE", DIAG_PROBE),
            ("DIAG_DUMP", DIAG_DUMP),
            ("DIAG_BAD_ARG", DIAG_BAD_ARG),
            ("DIAG_START", DIAG_START),
            ("DIAG_EXIT", DIAG_EXIT),
            ("DIAG_EXIT_ERR", DIAG_EXIT_ERR),
            ("DIAG_USAGE", DIAG_USAGE),
            ("DIAG_EDIT_NO_MAP", DIAG_EDIT_NO_MAP),
            ("DIAG_EDIT_PICK", DIAG_EDIT_PICK),
            ("DIAG_EDIT_PICK_CELL", DIAG_EDIT_PICK_CELL),
            ("DIAG_EDIT_LAYER", DIAG_EDIT_LAYER),
            ("DIAG_EDIT_LAYER_COLLISION", DIAG_EDIT_LAYER_COLLISION),
            ("DIAG_EDIT_REJECT", DIAG_EDIT_REJECT),
    ("DIAG_EDIT_GROUND_BLOCKED", DIAG_EDIT_GROUND_BLOCKED),
    ("DIAG_EDIT_BIND_FAIL", DIAG_EDIT_BIND_FAIL),
            ("DIAG_EDIT_PICK_NONE", DIAG_EDIT_PICK_NONE),
            ("DIAG_EDIT_BRUSH", DIAG_EDIT_BRUSH),
            ("DIAG_EDIT_ERASE", DIAG_EDIT_ERASE),
    ("DIAG_EDIT_FILL", DIAG_EDIT_FILL),
    ("DIAG_EDIT_FILL_REFUSED", DIAG_EDIT_FILL_REFUSED),
    ("DIAG_EDIT_SIZE", DIAG_EDIT_SIZE),
    ("DIAG_EDIT_STROKE", DIAG_EDIT_STROKE),
            ("DIAG_EDIT_UNDO", DIAG_EDIT_UNDO),
            ("DIAG_EDIT_REDO", DIAG_EDIT_REDO),
            ("DIAG_EDIT_CMP_SAME", DIAG_EDIT_CMP_SAME),
            ("DIAG_EDIT_CMP_DIFF", DIAG_EDIT_CMP_DIFF),
            ("DIAG_EDIT_CMP_ERR", DIAG_EDIT_CMP_ERR),
            ("DIAG_EDIT_SAVE_SKIP", DIAG_EDIT_SAVE_SKIP),
            ("DIAG_EDIT_SAVE_OK", DIAG_EDIT_SAVE_OK),
            ("DIAG_EDIT_SAVE_FAIL", DIAG_EDIT_SAVE_FAIL),
            ("DIAG_EDIT_SAVE_NOMAP", DIAG_EDIT_SAVE_NOMAP),
            ("DIAG_SAVE_DIRSYNC_OK", DIAG_SAVE_DIRSYNC_OK),
            ("DIAG_PALETTE", DIAG_PALETTE),
            ("DIAG_PALETTE_SCROLL", DIAG_PALETTE_SCROLL),
            ("DIAG_PALETTE_SEARCH", DIAG_PALETTE_SEARCH),
            ("DIAG_PALETTE_PICK", DIAG_PALETTE_PICK),
            ("DIAG_PALETTE_FAIL", DIAG_PALETTE_FAIL),
            ("SAVE_ERR_TEMP", SAVE_ERR_TEMP),
            ("SAVE_ERR_SYNC", SAVE_ERR_SYNC),
            ("SAVE_ERR_SHORT", SAVE_ERR_SHORT),
            ("SAVE_ERR_RENAME", SAVE_ERR_RENAME),
            ("SAVE_ERR_DIRSYNC", SAVE_ERR_DIRSYNC),
            ("SAVE_ERR_DIRSYNC_OPEN", SAVE_ERR_DIRSYNC_OPEN),
            // 导出 PNG（2026-11-05 接线）
            ("MENU_EXPORT_SEL_OK_TMPL", MENU_EXPORT_SEL_OK_TMPL),
            ("MENU_EXPORT_MINI_OK_TMPL", MENU_EXPORT_MINI_OK_TMPL),
            ("MENU_EXPORT_FAIL_TMPL", MENU_EXPORT_FAIL_TMPL),
            ("EXPORT_SEL_DLG_TITLE", EXPORT_SEL_DLG_TITLE),
            ("EXPORT_SEL_DLG_HINT", EXPORT_SEL_DLG_HINT),
            ("EXPORT_SEL_DLG_OK", EXPORT_SEL_DLG_OK),
            ("EXPORT_MINI_DLG_TITLE", EXPORT_MINI_DLG_TITLE),
            ("EXPORT_MINI_DLG_HINT", EXPORT_MINI_DLG_HINT),
            ("EXPORT_MINI_DLG_OK", EXPORT_MINI_DLG_OK),
            ("EXPORT_ERR_EMPTY", EXPORT_ERR_EMPTY),
            ("EXPORT_ERR_IS_DIR_TMPL", EXPORT_ERR_IS_DIR_TMPL),
            ("EXPORT_OVERWROTE", EXPORT_OVERWROTE),
            ("EXPORT_TWO_STAGE", EXPORT_TWO_STAGE),
            ("EXPORT_ERR_BAD_MAX_SIDE", EXPORT_ERR_BAD_MAX_SIDE),
            ("DIAG_EXPORT_OK", DIAG_EXPORT_OK),
            ("DIAG_EXPORT_FAIL", DIAG_EXPORT_FAIL),
            // 地图光照（2026-11-05 接线）
            ("MENU_LIGHT_DAWN_DUSK", MENU_LIGHT_DAWN_DUSK),
            ("MENU_LIGHT_DAY", MENU_LIGHT_DAY),
            ("MENU_LIGHT_NIGHT", MENU_LIGHT_NIGHT),
            ("STATUS_LIGHT_CONTAINER", STATUS_LIGHT_CONTAINER),
        ];
        for (name, s) in consts {
            assert!(listed.contains(s), "常量 {name} 不在 ALL 清单里（字形测试会漏查）");
        }
        // 数组类常量逐项核对
        for s in MENU_ITEMS
            .iter()
            .chain(LAYER_NAMES.iter())
            .chain(HIGHLIGHT_NAMES.iter())
            .chain(PALETTE_TABS.iter())
            .chain(EMPTY_DESC.iter())
            .chain(CAPTION_GLYPHS.iter())
            .chain(INSPECTOR_KEYS.iter())
        {
            assert!(listed.contains(s), "数组内文案 {s:?} 不在 ALL 清单里");
        }
    }

    /// **带「笔刷」的三条文案必须用 `{fam}` 填真实族，不许写死某一族。**
    ///
    /// 这条挡的是一次真实事故：`DIAG_PALETTE_PICK` 曾写死「地表」，
    /// 从 `SmTiles` 选笔刷时把中层笔刷报成"地表"（2026-11-04 三档验收实跑抓到）。
    #[test]
    fn 笔刷文案必须带族占位符() {
        for (name, s) in [
            ("STATUS_BRUSH_TMPL", STATUS_BRUSH_TMPL),
            ("DIAG_EDIT_PICK", DIAG_EDIT_PICK),
            ("DIAG_PALETTE_PICK", DIAG_PALETTE_PICK),
        ] {
            assert!(s.contains("{fam}"), "{name} 必须用 {{fam}} 填真实族：{s}");
            for fixed in [LAYER_GROUND, LAYER_MIDDLE, LAYER_OBJECTS] {
                assert!(
                    !s.contains(fixed),
                    "{name} 不许写死层名 {fixed:?}（会报错族）：{s}"
                );
            }
        }
        // 同理：「落笔」两条必须用 {img}（字段名），不许写死某一层的字段
        for (name, s) in [
            ("STATUS_WRITE_TMPL", STATUS_WRITE_TMPL),
            ("STATUS_WRITE_12B_TMPL", STATUS_WRITE_12B_TMPL),
        ] {
            assert!(s.contains("{img}"), "{name} 必须用 {{img}} 填真实字段名：{s}");
            for fixed in LAYER_IMG_FIELDS {
                assert!(!s.contains(fixed), "{name} 不许写死字段名 {fixed:?}：{s}");
            }
        }
    }

    /// **阻挡层文案必须点名位（`0x8000`）与方向（置位 = 不可走）**（审计 P0-9 的文案闸门）。
    ///
    /// 为什么单测这一条：阻挡层改的是"能不能走人"，这件事在画面上看不见；
    /// 若状态栏只写「落笔 bk_img」而不写"哪个位、置位什么意思"，用户还是不知道自己在干什么。
    #[test]
    fn 阻挡层文案点名位与方向() {
        for (name, s) in [
            ("STATUS_WRITE_COLLISION_TMPL", STATUS_WRITE_COLLISION_TMPL),
            ("EDIT_COLLISION_BRUSH_TMPL", EDIT_COLLISION_BRUSH_TMPL),
        ] {
            assert!(s.contains("0x8000"), "{name} 必须写出位名 0x8000：{s}");
            assert!(s.contains("{layer}"), "{name} 必须用 {{layer}} 填真实层名：{s}");
        }
        assert!(
            STATUS_WRITE_COLLISION_TMPL.contains("{img}"),
            "落笔位必须用 {{img}} 填字段名（低位 bk_img / 高位 fr_img）"
        );
        assert!(
            STATUS_WRITE_COLLISION_TMPL.contains("置位=不可走"),
            "必须说明置位方向（置位 = 不可走）"
        );
        // 低位/高位两行共用这一句 ⇒ 不许写死某一层的字段名
        for fixed in LAYER_IMG_FIELDS {
            assert!(
                !STATUS_WRITE_COLLISION_TMPL.contains(fixed),
                "不许写死字段名 {fixed:?}（低位/高位共用同一句）：{STATUS_WRITE_COLLISION_TMPL}"
            );
        }
    }

    /// 表自身的基本一致性：键名唯一、文案非空。
    #[test]
    fn 键名唯一且文案非空() {
        let mut names: Vec<&str> = ALL.iter().map(|(k, _)| *k).collect();
        let n = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), n, "ALL 里有重复键名");
        for (k, s) in ALL {
            assert!(!s.is_empty(), "文案 {k} 是空串");
        }
    }

    /// [`fill`] 必须真的替换、且没写进模板的占位符要原样留下（便于一眼看出漏填）。
    #[test]
    fn 模板填充() {
        let s = fill(STATUS_COORD_TMPL, &[("x", "380"), ("y", "310")]);
        assert_eq!(s, "单元格 380,310");
        // 少给一个参数 ⇒ 该占位符原样保留（不静默变空串）
        let s = fill(STATUS_COORD_TMPL, &[("x", "1")]);
        assert_eq!(s, "单元格 1,{y}");
        // 同一个占位符出现两次也要全替换
        let t = fill("{a}-{a}", &[("a", "7")]);
        assert_eq!(t, "7-7");
    }

    /// 所有模板里的 `{...}` 占位符名必须只由小写 ASCII 组成 —— 防止有人手抖写进格式说明符。
    #[test]
    fn 模板占位符形态合法() {
        for (key, s) in ALL {
            let mut rest = *s;
            while let Some(i) = rest.find('{') {
                let after = &rest[i + 1..];
                let j = after
                    .find('}')
                    .unwrap_or_else(|| panic!("文案 {key} 的占位符没闭合：{s:?}"));
                let name = &after[..j];
                assert!(
                    !name.is_empty()
                        && name
                            .chars()
                            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()),
                    "文案 {key} 的占位符 {{{name}}} 不合法（只允许小写字母/数字）"
                );
                rest = &after[j + 1..];
            }
        }
    }
}
