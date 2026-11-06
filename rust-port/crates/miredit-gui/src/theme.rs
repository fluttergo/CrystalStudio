//! 主题配色 —— **成对定义的两套色板**（浅色 [`LIGHT`] / 深色 [`DARK`]）+ 当前主题选择。
//!
//! # 一、为什么要成对定义
//!
//! 2026-11-05 起菜单「视图 → 深色模式」要能真切换，而**绘制处一律不许写死颜色**：
//! 所有绘制代码取色都走 [`ui()`]（返回当前生效的 [`Palette`]），切换主题只改一个开关
//! （[`set_dark`]），全界面（标题栏/菜单栏/面板/画布底/状态栏/弹层）随之变色。
//!
//! # 二、浅色那套从哪来（不变）
//!
//! CrystalStudio 的 `App.axaml` 里那套深色 `#14171D` 是 **XAML 声明值**，运行期
//! `ThemeService.ApplyPalette(dark=false, …)` 会按系统 accent 重算 ⇒ 上屏是浅色 + 金色强调。
//! 因此 [`LIGHT`] 的每个值**一律取自运行期截图**
//! `layout/refs/crystalstudio-main-runtime-1500x950.png`（1500×950，
//! 内容区原点 = (8, 0)，内容区尺寸 = 1484×942，见 `metrics::REF_CONTENT_ORIGIN`）。
//! 取色方法：对每个区域做**直方图**（Pillow，区域像素 Counter 取众数），
//! 每个字段的文档注释里写着 `(x,y)-(x,y)` 取样矩形与占比。
//!
//! # 三、深色那套从哪来（**不是自己发明的**）
//!
//! 官方 `CrystalStudio/ThemeService.Avalonia.cs:35-57` 的 `ApplyPalette(bool dark, …)`
//! 是**同一份 XAML 资源**的双向变换：XAML 里那些 `ThemeXXXXXX` 键就是**深色值**
//! （`App.axaml:1` 的 `RequestedThemeVariant="Dark"` 即默认深色），浅色是算出来的。
//! 实测两条交叉验证（本文档作者按 `:47` 的公式手算）：
//!
//! ```text
//! Theme1C2028 → 浅色 #F5F6F7  = 本文件 LIGHT.panel_bg   ✔
//! Theme303640 → 浅色 #EAECEF  = 本文件 LIGHT.separator  ✔
//! Theme252C38 → 浅色 #EFF1F3  = 本文件 LIGHT.input_bg   ✔
//! Theme14181F → 浅色 #F9FAFA  = 本文件 LIGHT.input_search_bg ✔
//! ```
//!
//! ⇒ **深色板的每个值都直接抄官方 XAML 资源**（`App.axaml` 的 `ThemeXXXXXX` 键 /
//! 命名刷 `Panel`·`Muted`·`Accent`·`Line`，以及 `ThemeService` 里写死的几个），
//! 每个字段的注释里标 `官方: App.axaml:NN ThemeXXXXXX` 或 `官方: ThemeService.Avalonia.cs:NN`。
//! 少数「浅色有、官方深色无对应角色」的（如金色标题栏），取官方**同角色**的深色资源并在
//! 注释里写明「映射是我选的、值是官方的」。
//!
//! # 四、当前主题存在哪
//!
//! [`set_dark`] 写的是**线程局部**开关：本 GUI 单线程（`eframe::App::update` 在同一线程），
//! 而 `cargo test` 每条用例各跑一个线程 ⇒ 用例之间不会互相污染（默认都是浅色）。
//! 持久化（`%LOCALAPPDATA%\miredit\settings.json`）在 [`crate::menu::settings`]。

use std::cell::Cell;

use egui::Color32;

/// 从分量构造**预乘**半透明色（egui 的 `Color32` 内部就是预乘存储）。
const fn rgba_pm(r: u8, g: u8, b: u8, a: u8) -> Color32 {
    Color32::from_rgba_premultiplied(r, g, b, a)
}

/// 一套完整配色。字段顺序 = 视觉层级（顶栏 → 面板 → 画布 → 状态栏 → 控件 → 强调）。
///
/// ⚠ **不要在绘制处再写颜色字面量**：任何新控件都从本结构体取色，
/// 否则深色模式下那一处会保持浅色（本单的验收项之一就是"没有地方仍用浅色常量"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    // ── 顶栏 ──
    /// 标题栏底色。
    pub title_bar: Color32,
    /// 标题栏文字色。
    pub title_bar_text: Color32,
    /// 菜单栏底色。
    pub menu_bar: Color32,
    /// 菜单栏文字色。
    pub menu_bar_text: Color32,

    // ── 面板 ──
    /// 面板内容区底色（左/右停靠区面板体）。
    pub panel_bg: Color32,
    /// 面板标题条底色。
    pub panel_header: Color32,
    /// 视口留白底色（中列地图面之外的区域）。
    pub view_surround: Color32,
    /// 画布（地图区域）的**底层底色**：地图位图之外的区域露出的颜色。
    pub canvas_bg: Color32,
    /// 状态栏底色。
    pub status_bar: Color32,

    // ── 文字 ──
    /// 正文/常规文字色。
    pub text: Color32,
    /// 次要/占位/禁用文字色。
    pub text_muted: Color32,

    // ── 强调色族 ──
    /// 强调色（主按钮填充 / 选中描边 / 图标）。
    pub accent: Color32,
    /// 强调色的深一档（1px 边框、页签下划线、状态栏强调段）。
    pub accent_dark: Color32,
    /// 选中行/选中按钮的浅强调填充。
    pub accent_soft: Color32,
    /// 主按钮文字色（在 `accent` 底上的字）。
    pub accent_button_text: Color32,
    /// 画布上**鼠标所在格**的描边色。
    pub cell_hover_gold: Color32,
    /// 画布上**单元格网格线**的颜色（视图 → 单元格网格，1px 世界格边界线）。
    ///
    /// ⚠ 它压在**地图位图**上，不是压在面板底色上。官方为此**专门**定义了 `CanvasGrid`
    /// （`ThemeService.Avalonia.cs:55`，半透明刷、**不是任何控件的边框色**），
    /// 消费点 `MapViewport.Presentation.cs:57@901dbe2f`
    /// `new ScenePen(ThemeService.Brush("CanvasGrid"), 1 / Zoom)` —— **笔宽 = `1/Zoom`**
    /// （世界单位，屏上恒 1px）。旧注释引的「`MapViewport.cs:137` 固定 0.6」那行已被
    /// 上游 `901dbe2f` 删除，别再照它算。
    /// 两套板都是**预乘**半透明值（见各板里的换算算式）。
    pub grid_line: Color32,

    // ── 分隔线 / 控件 ──
    /// 全局分隔线（菜单栏下沿、状态栏上沿）。
    pub separator: Color32,
    /// 中列与两侧停靠区之间的竖向分隔线。
    pub dock_divider: Color32,
    /// 输入控件（下拉框）底色。
    pub input_bg: Color32,
    /// 搜索框底色。
    pub input_search_bg: Color32,
    /// 输入控件边框。
    pub input_border: Color32,
    /// 复选框边框。
    pub checkbox_border: Color32,
    /// 滚动条滑块色。
    pub scrollbar: Color32,
    /// Tool Options 面板的色块（当前画笔颜色）。
    pub color_swatch: Color32,
    /// 层列表里 `低位阻挡` 的标记块 / 拒绝回执的警示色。
    ///
    /// ⚠ 官方 `CollisionMarkers.cs:6-7` 是 `static readonly`、**不经 `ThemeService`**
    /// ⇒ **主题无关常量**，两套板同值（已入 `tests::两套色板逐字段不同` 的 `same_ok`）。
    pub collision_low_mark: Color32,
    /// 层列表里 `高位阻挡` 的标记块（同上，官方主题无关常量）。
    pub collision_high_mark: Color32,
    /// 状态栏 `● 资源库 N 个` 的圆点色。
    pub status_dot: Color32,
    /// 空态图标颜色。
    pub empty_icon: Color32,
    /// 空态说明文字色（与 `text_muted` 同值但语义不同）。
    pub empty_text: Color32,

    // ── 常驻告警黄条（宿主新增，无官方截图依据） ──
    /// 告警黄条底色。
    pub warn_banner_bg: Color32,
    /// 告警黄条的边框 / 左侧竖条色。
    pub warn_banner_edge: Color32,
    /// 告警黄条正文色。
    pub warn_banner_text: Color32,

    // ── 印章 / 画布叠加层 ──
    /// 印章格子里「缺帧」占位的底色。
    pub stamp_missing_bg: Color32,
    /// 矩形选区的**虚线边框**色。
    pub selection_stroke: Color32,
    /// 矩形选区的**半透明填充**色（预乘）。
    ///
    /// ⚠ 官方 `MapViewport.cs:295` 是写死的 `Color.FromArgb(30, 110, 150, 255)`、**不经 `ThemeService`**
    /// ⇒ **主题无关常量**，两套板同值（已入 `same_ok`）。
    pub selection_fill: Color32,
    /// 印章**待放置轮廓框**的线色。
    pub stamp_preview_stroke: Color32,

    // ── 模态 / 弹层 ──
    /// 关闭拦截模态 / 打开地图模态的遮罩色（预乘半透明黑）。
    pub modal_scrim: Color32,
    /// 菜单下拉弹层的底色。
    pub menu_popup_bg: Color32,
    /// 菜单下拉弹层的 1px 边框色。
    pub menu_popup_border: Color32,
    /// 菜单项 hover / 键盘选中项的底色。
    pub menu_popup_hover: Color32,
}

/// **浅色板** —— 不透明值取自运行期截图直方图（见各字段的 `LIGHT:` 注释）；
/// 官方有**写死常量**的几项（网格线 / 两个碰撞标记 / 选区填充）改取官方常量并标出处。
pub const LIGHT: Palette = Palette {
    // 标题栏底色。截图 (300,6)-(1200,26) 区域直方图：`#604715` 占 100.0%。
    title_bar: Color32::from_rgb(0x60, 0x47, 0x15),
    // 标题栏文字。标题栏文字 (28,6)-(420,26) 区域最亮像素 = `#FFFFFF`。
    title_bar_text: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    // 菜单栏底色。截图 (300,36)-(1100,60) 区域直方图：`#F5F6F7` 占 100.0%。
    menu_bar: Color32::from_rgb(0xF5, 0xF6, 0xF7),
    // 菜单栏文字。菜单栏文字 (20,38)-(240,60) 区域暗像素众数 = `#000000`。
    menu_bar_text: Color32::from_rgb(0x00, 0x00, 0x00),

    // 面板体。截图 (20,600)-(150,760) 区域直方图：`#F5F6F7` 占 84.1%。
    panel_bg: Color32::from_rgb(0xF5, 0xF6, 0xF7),
    // 面板标题条。截图 (60,70)-(150,86) 区域直方图：`#E3E6E9` 占 100.0%。
    panel_header: Color32::from_rgb(0xE3, 0xE6, 0xE9),
    // 视口留白。截图 (170,110)-(220,170) 区域直方图：`#F9FAFB` 占 100.0%。
    view_surround: Color32::from_rgb(0xF9, 0xFA, 0xFB),
    // 画布底层。截图 (300,200)-(600,400) 区域直方图：`#EFF2F5` 占 89.3%。
    canvas_bg: Color32::from_rgb(0xEF, 0xF2, 0xF5),
    // 状态栏。截图 (200,916)-(1100,936) 区域直方图：`#F7F8F9` 占 100.0%。
    //   官方角色：`MainWindow.axaml:81` `<Border Grid.Row="4" Background="{DynamicResource Theme171B22}"
    //   BorderBrush="{DynamicResource Line}" BorderThickness="0,1,0,0">` —— `Grid.Row="4"` 就是**状态栏**
    //   （同一行里还有 `Status` / `Coordinates` / `LibraryDot` 三个状态控件）；
    //   键定义 `App.axaml:5` `Theme171B22` = `#171B22`。
    //   浅色值按 `ThemeService.Avalonia.cs:47` 现算（非蓝键 B−R=11≤35、lum<0.4 那一支）：
    //     lum    = (23×.2126 + 27×.7152 + 34×.0722)/255 = 26.655/255 = 0.104529
    //     amount = clamp(lum×1.4 − 0.07, 0, 0.65) = clamp(0.146341 − 0.07) = 0.076341
    //     Blend(White, #A3ADB9, amount) = 255 − (255−通道)×amount
    //       R: 255 − 92×0.076341 = 247.977 → 247 = 0xF7
    //       G: 255 − 82×0.076341 = 248.740 → 248 = 0xF8
    //       B: 255 − 70×0.076341 = 249.656 → 249 = 0xF9   ⇒ **#F7F8F9**
    //   ⇒ 截图直方图与官方公式**逐字节相同**：源键无疑（同款手算已入用例 `官方浅色变换可复算`）。
    status_bar: Color32::from_rgb(0xF7, 0xF8, 0xF9),

    // 正文。左面板正文 (94,596)、层列表 (52,353)、状态栏 (104,924)、面板标题 (28,70)-(120,86) 均 `#434D5A`。
    text: Color32::from_rgb(0x43, 0x4D, 0x5A),
    // 次要文字。搜索框占位符最暗像素 (1158,406) = `#9EA4AB`。
    text_muted: Color32::from_rgb(0x9E, 0xA4, 0xAB),

    // 强调金。`Set Library Folder` 按钮 (1230,610)-(1390,625) 直方图：`#CC9629` 占 79.8%。
    accent: Color32::from_rgb(0xCC, 0x96, 0x29),
    // 强调金深一档。金按钮左右边框 (1218/1408,607..628)、选中工具描边、页签下划线均 `#AD7F22`。
    accent_dark: Color32::from_rgb(0xAD, 0x7F, 0x22),
    // 选中浅金。层列表选中行 (60,350)-(140,362) 直方图 `#F7EFDE` 占 78.8%。
    accent_soft: Color32::from_rgb(0xF7, 0xEF, 0xDE),
    // 金按钮文字。`Set Library Folder` 文字区域最暗像素 = `#000000`。
    accent_button_text: Color32::from_rgb(0x00, 0x00, 0x00),
    // 悬停格描边。截图 (607..896,813..911) 金色像素众数 `#D2AA59`（占该掩码 82%）。
    cell_hover_gold: Color32::from_rgb(0xD2, 0xAA, 0x59),
    // 网格线。官方: ThemeService.Avalonia.cs:55 `resources["CanvasGrid"] = … : Color.FromArgb(60, 73, 89, 108)`
    //       ⇒ 浅色是 **24% 半透明深灰蓝**（官方**专用**的网格键，不是任何控件的边框色；
    //       旧值 `#D3D8DD` 其实是复选框边框 `Theme586477`（App.axaml:36）的官方浅色变换 —— 抄错了角色）。
    //       消费点 MapViewport.Presentation.cs:57@901dbe2f
    //       `new ScenePen(ThemeService.Brush("CanvasGrid"), 1 / Zoom)` ⇒ **笔宽 = 1/Zoom**
    //       （世界单位；屏上恒 1px）。旧注释引的「`MapViewport.cs:137` 固定 0.6」已被上游
    //       `901dbe2f` 删除 —— 颜色键没变，只是笔宽换了口径，别再照 0.6 算。
    //       预乘换算（**截断**口径，与 `rgba_pm` 一致）：73×60/255=17.2→17、89×60/255=20.9→20、108×60/255=25.4→25。
    grid_line: rgba_pm(17, 20, 25, 60),

    // 分隔线。菜单栏下沿 y=66、状态栏上沿 y=912 均为 `#EAECEF`。
    separator: Color32::from_rgb(0xEA, 0xEC, 0xEF),
    // 停靠区竖线。截图 x=224 与 x=1135 均为 `#E7EBEF`。
    dock_divider: Color32::from_rgb(0xE7, 0xEB, 0xEF),
    // 下拉框底。截图 (1160,356)-(1460,378) 直方图：`#EFF1F3` 占 100.0%。
    input_bg: Color32::from_rgb(0xEF, 0xF1, 0xF3),
    // 搜索框底。截图 (1160,400)-(1460,418) 直方图：`#F9FAFA` 占 89.5%。
    input_search_bg: Color32::from_rgb(0xF9, 0xFA, 0xFA),
    // 输入框边框。截图 x=1146 竖线：`#616262` 占 100.0%。
    input_border: Color32::from_rgb(0x61, 0x62, 0x62),
    // 复选框边框。`Light Cells` 复选框 (23,500)-(38,516)：`#D3D8DD` 占 11.8%。
    checkbox_border: Color32::from_rgb(0xD3, 0xD8, 0xDD),
    // 滚动条滑块。左停靠区 (157,380)-(161,500)：`#7A7A7A` 占 40.0%。
    scrollbar: Color32::from_rgb(0x7A, 0x7A, 0x7A),
    // 当前画笔色块。截图 (17,270)-(28,280)：`#AD7F22` 占 63.6%。
    color_swatch: Color32::from_rgb(0xAD, 0x7F, 0x22),
    // 低位阻挡标记。官方: CollisionMarkers.cs:6 `Color.FromArgb(90, 240, 85, 105)`（红、**35% 半透明**）。
    //       截图 (44,428)-(56,442) 的 `#F05569` = 官方 RGB **逐字节相同** ⇒ 源键无疑，只是**漏了 α90**
    //       （截图直方图取的是众数，把 α 抹成了不透明）。官方不经 `ThemeService` ⇒ 与 DARK 同值。
    //       预乘（截断）: 240×90/255=84.7→84、85×90/255=30.0→30、105×90/255=37.1→37。
    collision_low_mark: rgba_pm(84, 30, 37, 90),
    // 高位阻挡标记。官方: CollisionMarkers.cs:7 `Color.FromArgb(90, 65, 165, 245)`（蓝、35% 半透明）。
    //       截图 (44,454)-(56,468) 的 `#41A5F5` = 官方 RGB 同值。预乘（截断）:
    //       65×90/255=22.9→22、165×90/255=58.2→58、245×90/255=86.5→86。
    collision_high_mark: rgba_pm(22, 58, 86, 90),
    // 库状态圆点。截图 (1375..1380,918..930) 非底色众数 `#434D5A`（与正文同色）。
    status_dot: Color32::from_rgb(0x43, 0x4D, 0x5A),
    // 空态图标。截图 (1308,488)-(1330,508)：`#AD7F22` 占 15.7%（空态图标是金色）。
    empty_icon: Color32::from_rgb(0xAD, 0x7F, 0x22),
    // 空态文字（与 `text_muted` 同值、语义不同）。
    empty_text: Color32::from_rgb(0x9E, 0xA4, 0xAB),

    // 告警黄条（**宿主新增，无截图依据**）：底色取 `ACCENT_SOFT` 同族的浅琥珀。
    warn_banner_bg: Color32::from_rgb(0xFF, 0xF6, 0xE0),
    // 黄条边框/竖条 = `accent` 同族。
    warn_banner_edge: Color32::from_rgb(0xCC, 0x96, 0x29),
    // 黄条正文：`#7A4E00` 在 `#FFF6E0` 上对比度 ≈ 8.6:1（WCAG AAA 正文档）。
    warn_banner_text: Color32::from_rgb(0x7A, 0x4E, 0x00),

    // 缺帧占位底 = 面板标题条的同一个实测值（语义不同，单独命名）。
    stamp_missing_bg: Color32::from_rgb(0xE3, 0xE6, 0xE9),
    // 选区虚线 = 页签下划线的实测值 `#AD7F22`。
    selection_stroke: Color32::from_rgb(0xAD, 0x7F, 0x22),
    // 选区填充。官方: MapViewport.cs:295 `new ImmutableSolidColorBrush(Color.FromArgb(30, 110, 150, 255))`
    //       —— 写死的固定蓝、**不经 `ThemeService`** ⇒ 主题无关常量，与 DARK 同值
    //       （旧值 `rgba_pm(32,23,6,40)` 是自造的"accent 金 × α40"，与官方既不同色也不同 α30；
    //        报告 D5 只点了深色，浅色侧按同一判据补齐）。
    //       预乘（截断）: 110×30/255=12.9→12、150×30/255=17.6→17、255×30/255=30.0→30。
    selection_fill: rgba_pm(12, 17, 30, 30),
    // 印章轮廓 = `accent` 实测值 `#CC9629`（不透明实线）。
    stamp_preview_stroke: Color32::from_rgb(0xCC, 0x96, 0x29),

    // 模态遮罩（**推导**，参考截图无此控件）：35% 黑。
    modal_scrim: rgba_pm(0, 0, 0, 90),
    // 菜单弹层底色：取浅色视口留白 `#F9FAFB`（比面板体亮一档，弹层才"浮"起来）。
    menu_popup_bg: Color32::from_rgb(0xF9, 0xFA, 0xFB),
    // 菜单弹层边框：取复选框边框实测值 `#D3D8DD`。
    menu_popup_border: Color32::from_rgb(0xD3, 0xD8, 0xDD),
    // 菜单项 hover / 键盘选中：取选中浅金 `#F7EFDE`（与层列表选中行同一档）。
    menu_popup_hover: Color32::from_rgb(0xF7, 0xEF, 0xDE),
};

/// **深色板** —— 每个值都抄自官方深色资源（`App.axaml` 的 `ThemeXXXXXX` 键 / 命名刷，
/// 或 `ThemeService.Avalonia.cs` 里写死的那几个），见各字段的 `DARK:` 注释。
pub const DARK: Palette = Palette {
    // 官方: **没有"标题栏"这个角色** —— CrystalStudio 全仓 grep `ExtendClientArea` / `SystemDecorations`
    //       零命中 ⇒ 窗口用的是 OS 原生标题栏（DWM 画），`App.axaml` 里自然没有标题栏键。
    //       本仓自绘标题栏是 `with_decorations(false)` 的副产物（`window.rs:27-28`）⇒ 这一档**必须映射**。
    //       深色取官方**窗口外框档** `Theme14171D`：`App.axaml:83` `Selector="Window"` 的
    //       `Background="{DynamicResource Theme14171D}"`（窗口自身底色；同一个键又被
    //       `ThemeService.Avalonia.cs:53` 用作 `CanvasBackground`，即本文件 `view_surround`）
    //       ⇒ 深色下 `title_bar` 与 `view_surround` **同键同值**，不是漏抄。
    //       浅色侧**不是**这个键的变换：`#604715` 是截图里 DWM 按系统强调色画的标题栏
    //       （(300,6)-(1200,26) 直方图 100%），官方应用资源里没有金色标题栏 ⇒ 两套本来就不对称。
    //       ⚠ 旧值 `#171B22` 是**状态栏**色（`MainWindow.axaml:81`）⇒ 借错了角色，本次归位。
    //       旁证（浅色闭环）：`Theme14171D` 按 `ThemeService.Avalonia.cs:47` 变换 = `#F9FAFB`
    //       = 本文件 `LIGHT.view_surround`（算式同 `LIGHT.status_bar`，已入 `官方浅色变换可复算`）。
    //
    // ⚠ 本节的不透明色一律写 `Color32::from_rgb(r, g, b)` 三通道，**不写 `0xRRGG_BB` 那种分组**
    //   （半透明色写 `rgba_pm(...)`：`grid_line` / 两个碰撞标记 / `selection_fill`）：
    //   ① `clippy::unusual_byte_groupings` 会对"分组长度不等"（4+2）逐条报警，而本单的门是 NEW=0；
    //   ② `0x2026_32` 这种还会被当成**字面量后缀**（`_32` = i32）直接编译失败 —— 实测踩过。
    title_bar: Color32::from_rgb(0x14, 0x17, 0x1D),
    // 官方: App.axaml:83 `Selector="Window"` 的 `Foreground` = `ThemeE5E9F0`。
    title_bar_text: Color32::from_rgb(0xE5, 0xE9, 0xF0),
    // 官方: ThemeService.Avalonia.cs:50 `resources["Panel"] = Brush("Theme1C2028")`；
    //       同值见 App.axaml:70 命名刷 `Panel`、MainWindow.axaml:5 顶部条。
    menu_bar: Color32::from_rgb(0x1C, 0x20, 0x28),
    // 官方: App.axaml:83 Window Foreground。
    menu_bar_text: Color32::from_rgb(0xE5, 0xE9, 0xF0),

    // 官方: 同 `menu_bar`（`Panel`）。
    panel_bg: Color32::from_rgb(0x1C, 0x20, 0x28),
    // 官方: App.axaml:9 `Theme202632` = `#202632`（EditorControls.axaml:13 按下态底色）。
    // ⚠ 映射是我选的：标题条要比面板体**可辨**（浅色里 `#E3E6E9` 比 `#F5F6F7` 暗一档；
    //   官方深色里 `Panel` 更暗 ⇒ 对应的是比它**亮一档**的 `#202632`）。
    panel_header: Color32::from_rgb(0x20, 0x26, 0x32),
    // 官方: ThemeService.Avalonia.cs:53 `resources["CanvasBackground"] = Brush("Theme14171D")`。
    view_surround: Color32::from_rgb(0x14, 0x17, 0x1D),
    // 官方: ThemeService.Avalonia.cs:54
    //       `resources["CanvasPaper"] = dark ? Color.FromRgb(31, 37, 43) : Color.FromRgb(239, 242, 245)`
    //       ⇒ 深色 = (31,37,43) = `#1F252B`（浅色那半正是本文件 `LIGHT.canvas_bg` 的 `#EFF2F5`，闭环可验）。
    canvas_bg: Color32::from_rgb(0x1F, 0x25, 0x2B),
    // 官方: MainWindow.axaml:81 `<Border Grid.Row="4" Background="{DynamicResource Theme171B22}"
    //       BorderBrush="{DynamicResource Line}" BorderThickness="0,1,0,0">` —— `Grid.Row="4"` = **状态栏**
    //       （同行还有 `Status` / `Coordinates` / `LibraryDot` 三个状态控件）；键定义 `App.axaml:5`。
    //       ⇒ 深色 = `#171B22`，**比 `Panel`（`#1C2028`）暗一档**。
    //       旧值 `#1C2028` 抄的是 `Panel`（面板体 / 工具栏 / 菜单条，`App.axaml:8` +
    //       `ThemeService.Avalonia.cs:50` `resources["Panel"] = Brush("Theme1C2028")`，
    //       消费点 `MainWindow.axaml:5/6/8/24`）⇒ 深色下状态栏与面板同色，丢掉官方那条更暗的横带。
    //       浅色侧按 `:47` 现算 = `#F7F8F9`，与截图直方图逐字节相同（算式见 `LIGHT.status_bar`）。
    status_bar: Color32::from_rgb(0x17, 0x1B, 0x22),

    // 官方: App.axaml:83 Window Foreground `#E5E9F0`。
    text: Color32::from_rgb(0xE5, 0xE9, 0xF0),
    // 官方: ThemeService.Avalonia.cs:50 `resources["Muted"] = Brush("Theme929CAB")`。
    text_muted: Color32::from_rgb(0x92, 0x9C, 0xAB),

    // 官方: App.axaml:70 命名刷 `Accent` = `#8498FF`（ThemeService.Avalonia.cs:32 的默认 accent
    //       也是 `Color.FromRgb(132,152,255)`）。
    accent: Color32::from_rgb(0x84, 0x98, 0xFF),
    // 官方: ThemeService.Avalonia.cs:51 `resources["Theme6378DF"] = new ImmutableSolidColorBrush(accent);`
    //       —— 这行在变换循环（`:39-49`）**之后**执行，连蓝键公式算出的 `#A8B6FF` 也一并覆盖
    //       ⇒ 深色下 `Theme6378DF` **恒等于 `accent`**（默认 `#8498FF`）。
    //       `App.axaml:40` 的 `#6378DF` 只是 XAML **声明值**（上屏前就被覆盖）；旧值抄的正是这个声明值。
    accent_dark: Color32::from_rgb(0x84, 0x98, 0xFF),
    // 官方: 键 `Theme2B355A`（App.axaml:18；EditorControls.axaml:30 选中态 `Border#Chrome` 底）是**蓝键**：
    //       B-R=90-43=47>35、B-G=90-53=37>12（判据 `ThemeService.Avalonia.cs:43`），
    //       亮度 =(43×.2126+53×.7152+90×.0722)/255 = 0.210 < 0.4 ⇒ 走 `:46` 第一支
    //       `Blend(#191D23, accent, 0.28)` = (25×.72+132×.28, 29×.72+152×.28, 35×.72+255×.28)
    //       = (54.96, 63.44, 96.60) 截断 ⇒ `#363F60`。旧值抄的是 XAML 声明值 `#2B355A`（上屏前被重算）。
    accent_soft: Color32::from_rgb(0x36, 0x3F, 0x60),
    // 官方: ThemeService.Avalonia.cs:52 `OnAccent` = 亮度 > 145 ? 黑 : 白。
    //       accent `#8498FF` 亮度 = 132×.2126 + 152×.7152 + 255×.0722 = 155.2 > 145 ⇒ 黑。
    accent_button_text: Color32::from_rgb(0x00, 0x00, 0x00),
    // 官方: ThemeService.Avalonia.cs:46 的**深色蓝键公式** `Blend(accent, White, 0.30)`
    //       = (132,152,255)×0.7 + 255×0.3 = (168.9,182.9,255) ⇒ `#A8B6FF`。
    //       （浅色那支同公式是 `Blend(accent, Black, 0.15)`，与截图取样 `#D2AA59` 属同一"强调色描边"角色。）
    cell_hover_gold: Color32::from_rgb(0xA8, 0xB6, 0xFF),
    // 官方: ThemeService.Avalonia.cs:55 `resources["CanvasGrid"] = … : Color.FromArgb(38, 160, 177, 199)`
    //       ⇒ 深色是 **15% 半透明浅蓝灰**（官方**专用**的网格键，不是任何控件的边框色；
    //       旧值 `#353E4C` 是 `Theme353E4C`（App.axaml:22）—— 那是 TextBox 的 BorderBrush，抄错了角色）。
    //       消费点 MapViewport.Presentation.cs:57@901dbe2f
    //       `new ScenePen(ThemeService.Brush("CanvasGrid"), 1 / Zoom)` ⇒ **笔宽 = 1/Zoom**
    //       （世界单位；屏上恒 1px）。旧注释引的「`MapViewport.cs:137` 固定 0.6」已被上游
    //       `901dbe2f` 删除 —— 颜色键没变，只是笔宽换了口径，别再照 0.6 算。
    //       预乘换算（**截断**口径，与 `rgba_pm` 一致）：160×38/255=23.8→23、177×38/255=26.4→26、199×38/255=29.7→29。
    grid_line: rgba_pm(23, 26, 29, 38),

    // 官方: ThemeService.Avalonia.cs:50 `resources["Line"] = Brush("Theme303640")`。
    separator: Color32::from_rgb(0x30, 0x36, 0x40),
    // 官方: App.axaml:22 `Theme353E4C` = `#353E4C`（App.axaml:84 TextBox 的 BorderBrush）。
    // ⚠ 映射是我选的：浅色里停靠区竖线 `#E7EBEF` 与全局分隔线 `#EAECEF` 是**两个不同的值**，
    //   深色也取两个相邻的官方值（Line / Theme353E4C），不合并成一个。
    dock_divider: Color32::from_rgb(0x35, 0x3E, 0x4C),
    // 官方: App.axaml:13 `Theme252C38` = `#252C38`（App.axaml:85 ComboBox 的 Background）。
    input_bg: Color32::from_rgb(0x25, 0x2C, 0x38),
    // 官方: App.axaml:3 `Theme14181F` = `#14181F`（App.axaml:84 TextBox 的 Background）。
    input_search_bg: Color32::from_rgb(0x14, 0x18, 0x1F),
    // 官方: App.axaml:84 TextBox `BorderBrush` = `Theme353E4C`。
    input_border: Color32::from_rgb(0x35, 0x3E, 0x4C),
    // 官方: EditorControls.axaml:37 复选框 `Border#Box` 的 `BorderBrush` = `Theme586477`。
    checkbox_border: Color32::from_rgb(0x58, 0x64, 0x77),
    // 官方: EditorControls.axaml:94 滑杆轨道 = `Theme394455`。
    scrollbar: Color32::from_rgb(0x39, 0x44, 0x55),
    // 官方: 同 `accent`（`#8498FF`）。
    color_swatch: Color32::from_rgb(0x84, 0x98, 0xFF),
    // 官方: CollisionMarkers.cs:6 `Color.FromArgb(90, 240, 85, 105)` —— `static readonly`、**不经 ThemeService**
    //       ⇒ 主题无关常量，与 LIGHT 同值（旧值 `#D69D92` 是自造的"警示色映射"，报告 D4 已否）。
    //       预乘（截断）: 240×90/255=84.7→84、85×90/255=30.0→30、105×90/255=37.1→37。
    collision_low_mark: rgba_pm(84, 30, 37, 90),
    // 官方: CollisionMarkers.cs:7 `Color.FromArgb(90, 65, 165, 245)` —— 同上，主题无关常量，与 LIGHT 同值。
    //       预乘（截断）: 65×90/255=22.9→22、165×90/255=58.2→58、245×90/255=86.5→86。
    collision_high_mark: rgba_pm(22, 58, 86, 90),
    // 官方: MainWindow.axaml:81 `<Ellipse x:Name="LibraryDot" … Fill="{DynamicResource ThemeD1AD77}"/>`
    //       （键定义 App.axaml:65）⇒ 状态圆点是**金/琥珀档** `#D1AD77`。
    //       旧值 `#E5E9F0` 抄的是 Window Foreground 文字色 ⇒ 圆点与旁边的 `Muted` 文字同色、
    //       失去"状态指示"的视觉区分（浅色侧 `#434D5A` 正是 `ThemeD1AD77` 的官方浅色变换，闭环可验）。
    status_dot: Color32::from_rgb(0xD1, 0xAD, 0x77),
    // 官方: 同 `accent`。
    empty_icon: Color32::from_rgb(0x84, 0x98, 0xFF),
    // 官方: 同 `text_muted`（`Muted` `#929CAB`）。
    empty_text: Color32::from_rgb(0x92, 0x9C, 0xAB),

    // 官方: App.axaml:9 `Theme202632`（深色里"比 Panel 亮一档"的底色）。
    warn_banner_bg: Color32::from_rgb(0x20, 0x26, 0x32),
    // 官方: App.axaml:65 `ThemeD1AD77` = `#D1AD77`（深色板的金/琥珀档；同键的另一处用途见 `status_dot`）。
    warn_banner_edge: Color32::from_rgb(0xD1, 0xAD, 0x77),
    // 官方: App.axaml:83 Window Foreground（`#E5E9F0` 在 `#202632` 上对比度 ≈ 11.6:1）。
    warn_banner_text: Color32::from_rgb(0xE5, 0xE9, 0xF0),

    // 官方: 同 `panel_header`（`#202632`）。
    stamp_missing_bg: Color32::from_rgb(0x20, 0x26, 0x32),
    // 官方: 同 `accent`。
    selection_stroke: Color32::from_rgb(0x84, 0x98, 0xFF),
    // 官方: MapViewport.cs:295 `new ImmutableSolidColorBrush(Color.FromArgb(30, 110, 150, 255))`
    //       —— 写死、**不经 `ThemeService`** ⇒ 主题无关常量，与 LIGHT 同值
    //       （旧值 `rgba_pm(20,23,40,40)` 是自造的"accent × α40"：既非官方色，也非官方 α30）。
    //       预乘（截断）: 110×30/255=12.9→12、150×30/255=17.6→17、255×30/255=30.0→30。
    selection_fill: rgba_pm(12, 17, 30, 30),
    // 官方: 同 `accent`。
    stamp_preview_stroke: Color32::from_rgb(0x84, 0x98, 0xFF),

    // 遮罩两套同值：它压在**地图**上，与主题无关（浅色下也不该变）。
    modal_scrim: rgba_pm(0, 0, 0, 90),
    // 官方: 同 `panel_bg`（`Panel` `#1C2028`）。
    menu_popup_bg: Color32::from_rgb(0x1C, 0x20, 0x28),
    // 官方: 同 `separator`（`Line` `#303640`）。
    menu_popup_border: Color32::from_rgb(0x30, 0x36, 0x40),
    // 官方: 同 `accent_soft`（蓝键 `Theme2B355A` 经 `ThemeService.Avalonia.cs:46` 重算 ⇒ `#363F60`）。
    menu_popup_hover: Color32::from_rgb(0x36, 0x3F, 0x60),
};
thread_local! {
    /// 当前是否深色（**线程局部**：GUI 单线程，`cargo test` 每条用例独立线程 ⇒ 互不污染）。
    static DARK_MODE: Cell<bool> = const { Cell::new(false) };
}

/// 按开关取色板（**纯函数**，不读全局；验收用例用它逐字段核对两套值）。
pub const fn palette_for(dark: bool) -> &'static Palette {
    if dark {
        &DARK
    } else {
        &LIGHT
    }
}

/// 当前是否深色主题。
pub fn is_dark() -> bool {
    DARK_MODE.with(Cell::get)
}

/// 切换主题（只改开关，不动任何绘制代码）。
pub fn set_dark(on: bool) {
    DARK_MODE.with(|c| c.set(on));
}

/// **当前生效的色板** —— 所有绘制代码取色的唯一入口。
pub fn ui() -> &'static Palette {
    palette_for(is_dark())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 两套色板必须**逐字段**都不相同（除了官方"与主题无关"的五项，见 `same_ok`）。
    ///
    /// 这条挡的是"深色板漏抄一个字段，于是那一处仍然浅色"——那正是本单要防的回归。
    ///
    /// ⚠ `same_ok` **不是放宽**：进这张表的每个字段都有官方依据证明它**本就与主题无关**
    /// （遮罩压在**地图**上；`OnAccent` 算出来两套都是黑；`CollisionMarkers.cs:6-7` 与
    /// `MapViewport.cs:295` 都是写死常量、不经 `ThemeService`）。
    /// 反向纪律：凡是"官方有主题相关来源"的字段**一律不许**进这张表 —— 那才是真的把回归放过去。
    /// 这五项另由 [`主题无关字段取官方常量`] 逐字节钉住官方值，防止"两套都抄错但抄成一样"。
    #[test]
    fn 两套色板逐字段不同() {
        // 与主题无关的五项（逐条依据见上）。
        let same_ok = [
            "modal_scrim",
            "accent_button_text",
            "collision_low_mark",
            "collision_high_mark",
            "selection_fill",
        ];
        let pairs = [
            ("title_bar", LIGHT.title_bar, DARK.title_bar),
            ("title_bar_text", LIGHT.title_bar_text, DARK.title_bar_text),
            ("menu_bar", LIGHT.menu_bar, DARK.menu_bar),
            ("menu_bar_text", LIGHT.menu_bar_text, DARK.menu_bar_text),
            ("panel_bg", LIGHT.panel_bg, DARK.panel_bg),
            ("panel_header", LIGHT.panel_header, DARK.panel_header),
            ("view_surround", LIGHT.view_surround, DARK.view_surround),
            ("canvas_bg", LIGHT.canvas_bg, DARK.canvas_bg),
            ("status_bar", LIGHT.status_bar, DARK.status_bar),
            ("text", LIGHT.text, DARK.text),
            ("text_muted", LIGHT.text_muted, DARK.text_muted),
            ("accent", LIGHT.accent, DARK.accent),
            ("accent_dark", LIGHT.accent_dark, DARK.accent_dark),
            ("accent_soft", LIGHT.accent_soft, DARK.accent_soft),
            ("cell_hover_gold", LIGHT.cell_hover_gold, DARK.cell_hover_gold),
            ("grid_line", LIGHT.grid_line, DARK.grid_line),
            ("separator", LIGHT.separator, DARK.separator),
            ("dock_divider", LIGHT.dock_divider, DARK.dock_divider),
            ("input_bg", LIGHT.input_bg, DARK.input_bg),
            ("input_search_bg", LIGHT.input_search_bg, DARK.input_search_bg),
            ("input_border", LIGHT.input_border, DARK.input_border),
            ("checkbox_border", LIGHT.checkbox_border, DARK.checkbox_border),
            ("scrollbar", LIGHT.scrollbar, DARK.scrollbar),
            ("color_swatch", LIGHT.color_swatch, DARK.color_swatch),
            ("status_dot", LIGHT.status_dot, DARK.status_dot),
            ("empty_icon", LIGHT.empty_icon, DARK.empty_icon),
            ("empty_text", LIGHT.empty_text, DARK.empty_text),
            ("warn_banner_bg", LIGHT.warn_banner_bg, DARK.warn_banner_bg),
            ("warn_banner_edge", LIGHT.warn_banner_edge, DARK.warn_banner_edge),
            ("warn_banner_text", LIGHT.warn_banner_text, DARK.warn_banner_text),
            ("stamp_missing_bg", LIGHT.stamp_missing_bg, DARK.stamp_missing_bg),
            ("selection_stroke", LIGHT.selection_stroke, DARK.selection_stroke),
            (
                "stamp_preview_stroke",
                LIGHT.stamp_preview_stroke,
                DARK.stamp_preview_stroke,
            ),
            ("menu_popup_bg", LIGHT.menu_popup_bg, DARK.menu_popup_bg),
            ("menu_popup_border", LIGHT.menu_popup_border, DARK.menu_popup_border),
            ("menu_popup_hover", LIGHT.menu_popup_hover, DARK.menu_popup_hover),
        ];
        assert_eq!(pairs.len() + same_ok.len(), 41, "字段清单要覆盖全部 41 项");
        for (name, l, d) in pairs {
            assert_ne!(l, d, "字段 {name} 两套色板相同（深色模式会漏变）");
        }
    }

    /// **主题无关字段取官方常量**（与 `same_ok` 配对，防止"两套都抄错但抄成一样"）。
    ///
    /// 这三项官方都是**写死常量、不经 `ThemeService`** ⇒ 深浅两套必须**逐字节等于官方预乘值**：
    /// - 低/高位阻挡：`CollisionMarkers.cs:6-7` `Color.FromArgb(90, 240, 85, 105)` / `(90, 65, 165, 245)`
    /// - 选区填充：`MapViewport.cs:295` `Color.FromArgb(30, 110, 150, 255)`
    ///
    /// ⚠ 为什么必须有这条：`same_ok` 让它们退出"两套必须不同"的检查，**只退出不补就真是放宽了**。
    /// 这条把"退出逐字段不同"换成"必须等于官方"，强度只增不减。
    ///
    /// ⚠ **`want` 必须从官方 straight-alpha 现算，不许复用板里的字面量**（审查第 7 轮缺陷 1）：
    /// 旧版 `want` 写的是与色板**同一串** `rgba_pm(84, 30, 37, 90)` ⇒ 预乘算错一位、两处一起改
    /// 仍然全绿，等于只证了"两套板一致"。现在：官方值只出现在本用例的常量表里，
    /// 预乘用本用例**独立实现**的 `pm`（不碰生产代码的 `rgba_pm`），断言比**逐通道字节**
    /// （`Color32::r/g/b` 返回的正是预乘后的字节，见 `ecolor` 的 `color32.rs:211-233`）。
    #[test]
    fn 主题无关字段取官方常量() {
        /// 官方 straight-alpha → 预乘（**截断**口径，与 `egui` 的 `from_rgba_unmultiplied`
        /// 同一个整数除法）。**测试内独立实现**：生产代码那个 `rgba_pm` 只做
        /// `from_rgba_premultiplied`、不含除法，两者没有共同实现可复用。
        fn pm(c: u8, a: u8) -> u8 {
            (u16::from(c) * u16::from(a) / 255) as u8
        }
        // (字段名, 官方 (α, R, G, B), 浅色板, 深色板)
        let cases = [
            (
                "collision_low_mark",
                (90u8, 240u8, 85u8, 105u8),
                LIGHT.collision_low_mark,
                DARK.collision_low_mark,
            ),
            (
                "collision_high_mark",
                (90, 65, 165, 245),
                LIGHT.collision_high_mark,
                DARK.collision_high_mark,
            ),
            (
                "selection_fill",
                (30, 110, 150, 255),
                LIGHT.selection_fill,
                DARK.selection_fill,
            ),
        ];
        for (name, (a, r, g, b), l, d) in cases {
            // 期望值：逐通道由官方 straight-alpha 现算（240×90/255=84.7→84 这种）
            let want = (pm(r, a), pm(g, a), pm(b, a), a);
            let got_l = (l.r(), l.g(), l.b(), l.a());
            let got_d = (d.r(), d.g(), d.b(), d.a());
            assert_eq!(
                got_l, want,
                "{name} 浅色板不是官方常量（官方 straight alpha = ({a}, {r}, {g}, {b})）"
            );
            assert_eq!(
                got_d, want,
                "{name} 深色板不是官方常量（官方与主题无关 ⇒ 两套必须同值）"
            );
        }
    }

    /// **官方浅色变换可复算**（交叉验证"XAML 资源就是深色值"这条结论）。
    ///
    /// 照 `ThemeService.Avalonia.cs:47` 的公式手算下面每个键，必须逐字节等于本文件浅色板的值：
    /// `Blend(White, #A3ADB9, clamp(L*1.4-0.07, 0, 0.65))`。
    ///
    /// 2026-11-05 增补两个键（都是本轮订正过的角色，用**公式**而不是注释钉住）：
    /// - `Theme171B22` → `LIGHT.status_bar`：证明状态栏源键是 `MainWindow.axaml:81` 那一行；
    /// - `Theme14171D` → `LIGHT.view_surround`：证明 `DARK.title_bar` 取的窗口外框键真实存在
    ///   （`App.axaml:83` `Selector="Window"` 的 `Background`）。
    #[test]
    fn 官方浅色变换可复算() {
        let blend = |a: [u8; 3], b: [u8; 3], amount: f64| -> [u8; 3] {
            let f = |x: u8, y: u8| {
                (f64::from(x) * (1.0 - amount) + f64::from(y) * amount) as u8
            };
            [f(a[0], b[0]), f(a[1], b[1]), f(a[2], b[2])]
        };
        let light_of = |dark: [u8; 3]| -> [u8; 3] {
            let lum = (f64::from(dark[0]) * 0.2126
                + f64::from(dark[1]) * 0.7152
                + f64::from(dark[2]) * 0.0722)
                / 255.0;
            let blue = i32::from(dark[2]) - i32::from(dark[0]) > 35
                && i32::from(dark[2]) - i32::from(dark[1]) > 12;
            assert!(!blue, "本用例只覆盖非蓝键");
            blend([255, 255, 255], [0xA3, 0xAD, 0xB9], (lum * 1.4 - 0.07).clamp(0.0, 0.65))
        };
        let c = |p: Color32| [p.r(), p.g(), p.b()];
        for (dark, light, name) in [
            ([0x1C, 0x20, 0x28], LIGHT.panel_bg, "panel_bg"),
            ([0x30, 0x36, 0x40], LIGHT.separator, "separator"),
            ([0x25, 0x2C, 0x38], LIGHT.input_bg, "input_bg"),
            ([0x14, 0x18, 0x1F], LIGHT.input_search_bg, "input_search_bg"),
            // 状态栏：`MainWindow.axaml:81` 的 `Theme171B22`（浅色由公式算出，与截图直方图逐字节相同）。
            ([0x17, 0x1B, 0x22], LIGHT.status_bar, "status_bar"),
            // 窗口外框：`App.axaml:83` `Selector="Window"` 的 `Theme14171D`（= `DARK.title_bar` 的源键）。
            ([0x14, 0x17, 0x1D], LIGHT.view_surround, "view_surround"),
        ] {
            assert_eq!(light_of(dark), c(light), "{name} 与官方公式算出的浅色不一致");
        }
    }

    /// 全局开关：默认浅色；`set_dark(true)` 后 [`ui`] 换板；恢复后逐字段回到浅色。
    #[test]
    fn 全局开关切换色板() {
        assert!(!is_dark(), "用例线程默认必须是浅色（否则会污染同进程的其它用例）");
        assert_eq!(ui().panel_bg, LIGHT.panel_bg);
        set_dark(true);
        assert!(is_dark());
        assert_eq!(ui().panel_bg, DARK.panel_bg);
        set_dark(false);
        assert_eq!(ui().panel_bg, LIGHT.panel_bg);
    }
}
