# CrystalStudio 界面真值（Avalonia 11 / .NET 10）— 抽取口径与未解项

> 本目录存放从上游 C# 客户端 **CrystalStudio**（提交 `901dbe2f`）**只读**抽取的界面真值，供本仓 Rust 版复刻界面时对照：
> `crystalstudio-ui.json`（控件树 / 主题色 / 度量 / 代码建控件几何）与 `crystalstudio-strings.json`（界面文案 key → 中文）。
> 源工程全程只读：未改动、未编译、未运行。

| 项 | 值 |
|---|---|
| 产物 | `crystalstudio-ui.json` |
| 大小 / 编码 | 514,877 字节 / UTF-8 / 缩进 2 / 末尾换行 |
| SHA256 | `98CC05D0FD1CA39DDD2447FC83C44056FAE7D5AF2BD71C49D2B01C61540BB0CF` |
| 统计行 | `windows=7, total_nodes=499, theme_colors=71, code_built=52, unknowns=18` |
| 抽取日 | 2026-11-04 |
| 可重复性 | 同一输入重复运行输出字节完全一致（已连跑两次比 SHA256） |

⚠ **与早期抽取清单有 7 处不同，逐条列在 §4，没有一处被静默改掉。**

---

## 1 数据源（全部只读）

| 文件 | 用途 | 行数 |
|---|---|---|
| `App.axaml` | 应用资源字典（71 个色刷）+ Window/TextBox/ComboBox/Slider 样式 | 94 |
| `EditorControls.axaml` | 控件主题（Button/QuietButton/PrimaryButton/ToolButton/CheckBox/SectionTitle/Palette* /LayerRow/LayerVisibility/LayerHighlight/Slider/StampCategoryItem） | 116 |
| `MainWindow.axaml` | 主窗骨架 | 73 |
| `ProvinceGeneratorWindow.axaml` | 省域生成对话框 | 47 |
| `TempleGeneratorWindow.axaml` | 地牢生成对话框 | 44 |
| `TerrainGeneratorWindow.axaml` | 地形生成对话框 | 61 |
| `TownStampWindow.axaml` | 城镇戳记对话框 | 36 |
| `SpecialBrushWindow.axaml` | 特殊笔刷（树/灌木/岩石）对话框 | 61 |
| `WallBrushWindow.axaml` | 墙体笔刷对话框 | 16 |
| `DockWorkspace.cs` | 停靠工作区（代码建控件 + 全部面板几何） | 332 |
| `SettingsWindow.cs` | 设置窗（代码建控件） | 46 |
| `StampCategoryPicker.cs` | 多选分类下拉（代码建控件） | 32 |
| `ThemeService.Avalonia.cs` | 运行期调色（把静态色板重算） | 58 |
| `ResponsivePanel.cs` | 自适应换列容器（`ResponsivePanel` 的算法） | 43 |
| `EditorWindow.cs` | 所有窗口基类（`Opened` 时改写宽高） | 143 |
| `DefaultWorkspace.json` | 内嵌默认面板布局（运行期真实停靠数据） | 100 |
| `CrystalStudio.csproj` | Avalonia 11.3.4 / net10.0（版本真值） | 44 |

---

## 2 JSON 口径

### 2.1 顶层键

| 键 | 含义 |
|---|---|
| `source` / `extracted_at` | 来源与抽取日 |
| `xaml_files` | **9 个 axaml 全在**，每个带 `role`：`application` / `control-theme` / `window` |
| `theme` | 字体、51 条度量、4 张颜色表 |
| `windows` | **7 个真正的窗口**（见 §4 冲突 1） |
| `code_built` | 52 条写死的几何常量，全部 `文件:行号` |
| `unknowns` | 18 条静态不可解项 |

### 2.2 `kind` 取值表（12 类，无未分类节点）

| kind | 含义 | 元素 | 节点数 |
|---|---|---|---|
| `label` | 文本 | TextBlock, Run | 138 |
| `panel` | 容器 | Border, Grid, StackPanel, DockPanel, WrapPanel, ScrollViewer, Expander, ResponsivePanel | 129 |
| `button` | 可点 | Button, RadioButton, CheckBox, ToggleButton, Menu, MenuItem | 107 |
| `combo` | 下拉 | ComboBox, ComboBoxItem | 45 |
| `canvas` | 画布/图元 | Canvas, Viewbox, Path, Image, Ellipse, MapViewport | 24 |
| `slider` | 滑杆 | Slider | 20 |
| `tree` | 列表/树 | ListBox, ListBoxItem | 9 |
| `separator` | 分隔线 | Separator | 8 |
| `input` | 输入框 | TextBox | 7 |
| `tab` | 页签 | TabControl, TabItem | 3 |
| `splitter` | 分隔条 | GridSplitter | 1 |
| `status` | 进度/状态 | ProgressBar | 1 |

### 2.3 节点字段

| 字段 | 说明 |
|---|---|
| `kind` / `tag` | 语义类别 / XAML 元素原名（如 `RadioButton`） |
| `name` | `x:Name`，**192 个节点带名，全部保留** |
| `text` | `Text`/`Content`/`Header`，无则由 `Run` 拼接（绑定表达式按原文保留） |
| `rect` | `{x,y,w,h}`；**算不出来整体为 `null`**，部分未知则该分量为 `null` |
| `layout` | `row:/col:/span:/dock:/orient:/halign:/valign:`，都没有则为 `fill` |
| `style_ref` | `Theme="{StaticResource X}"` 的 `X` |
| `icon` | 仅 `button` 类节点带：`path`（含每条 Path 的 `d/fill/stroke/stroke_thickness` + Viewbox 尺寸 + Canvas 设计栅格）或 `glyph`（MDL2 字体 + 字号 + 字符 + 码位） |
| `grid_rows`/`grid_columns` | 仅 `Grid` 节点带：该网格自己的轨道声明 |
| `margin`/`padding`/`font_size` | 原文字符串/数值，未写为 `null` |
| `visible`/`enabled` | `IsVisible`/`IsEnabled`，未写为 `null` |
| `tag_value` | Avalonia 的 `Tag`（工具按钮靠它区分笔刷，如 `Brush`/`Wall`） |
| `tooltip`/`automation` | 悬停提示 / `AutomationProperties.Name`（图标按钮的语义名） |
| `notes` | 由代码生成 / 自适应 / 默认隐藏禁用 等，**只写有依据的** |
| `depth`/`children` | 层级与子节点 |

### 2.4 `rect` 的推导规则（这是唯一有"算"的地方，规则写清以便复核）

能算：
1. 元素自身 `Width`/`Height`（如 `Width="78"`）；
2. 父 `Grid` 的**固定**行/列尺寸（如 `Width="72"` 的列 → 该列子节点 `w=72`）；
3. 父 `Grid` 无列/行声明时视为**单隐式轨道**，子节点撑满父格 → `w`/`h` 取父格尺寸；
4. 绝对 `x/y` **只在**"从窗口根到该节点整条祖先链偏移都已知"时给出（即祖先全是固定轨道的 Grid）。`MainWindow` 实测：标题栏 `y=0,h=36`、工具条 `y=36,h=48`、工作区 `y=84`、左侧工具列 `x=0,w=72`、分隔条 `w=5`、右栏 `w=360`。

算不出（写 `null`，不猜）：
1. 星号 `*` 与 `Auto` 轨道的尺寸；2. 星号轨道**之后**任何轨道的起点（如状态栏 `y`）；3. `StackPanel`/`WrapPanel`/`DockPanel` 内子元素的偏移（依赖文字度量）；4. 跨行/跨列（`RowSpan/ColumnSpan > 1`）时的尺寸。

492 个树节点里：**420 个 `rect` 完全为 `null`**、24 个只有 `w/h` 而 `x/y` 未知、**只有 45 个能给出绝对 `x/y`**——这就是"XAML 看不出像素"的规模，如实反映。

### 2.5 颜色：四张表

| 表 | 条数 | 内容 |
|---|---|---|
| `theme.colors` | **71** | `App.axaml` 里 `SolidColorBrush` 的**资源键名 → 色值**原文（`Theme14171D`→`#14171D` … `ThemeED1C2028`→`#ED1C2028`，另有 `Panel`/`Muted`/`Accent`/`Line`） |
| `theme.semantic` | **55** | 语义角色 → `{resource, hex, source}`，**每条带 `文件:行号`**（如 `window_bg` → `Theme14171D` `#14171D` @ `App.axaml:85`） |
| `theme.inline_colors` | **6** | 没走资源的写死颜色（低/高碰撞色块 `#F05569`/`#41A5F5`、地牢预览底 `#20272D`、设置窗错误红 `#E49E97`、停靠落点 `#418498FF`/`#8498FF`） |
| `theme.runtime_computed_colors` | **5** | 运行期才算得出的键（`OnAccent`/`CanvasBackground`/`CanvasPaper`/`CanvasGrid`/`Accent`），给公式与行号 |

### 2.6 `metrics`（51 条，每条带 `metrics_source`）

早期清单点名的 8 个：`border_thickness=1`、`radius_sm=2`、`radius_md=3`、`radius_lg=5`、`icon_sm=14`、`icon_md=16`、`icon_lg=24`（+ 实存 `radius_row=4`、`radius_xl=6`）。
**早期清单写的 `icon_sm=12 / icon_lg=20` 在工程里没有出处**，见 §4 冲突 3。
另有 `tool_button=32`、`tool_button_margin=2`、`tool_item_min_width=36`、`slider_thumb_width=12`、`slider_thumb_height=18`、`slider_track_height=3`、`checkbox_box=14`、`checkbox_gap=7`、`combo_min_height=34`、`tab_item_font_size=11`、`section_title_font_size=10`、`scrollbar_min=9` 等。

---

## 3 主窗骨架实测

### 3.1 窗口

| 项 | 值 | 出处 |
|---|---|---|
| Width×Height | **1500×950** | `MainWindow.axaml:1` |
| MinWidth×MinHeight | **1100×720** | `MainWindow.axaml:1` |
| FontFamily | **Segoe UI**（单字体） | `MainWindow.axaml:1` |
| 样式回退链 | `Segoe UI, Microsoft YaHei, sans-serif` | `App.axaml:85` |
| FontSize | **12** | `MainWindow.axaml:1` + `App.axaml:85` |
| ThemeVariant | **Dark** | `App.axaml:1` |
| 背景/前景 | `Theme14171D` / `ThemeE5E9F0` | `MainWindow.axaml:1` |
| UseLayoutRounding | True | `MainWindow.axaml:1` |

### 3.2 根 Grid（`MainWindow.axaml:3-4`）

行 = `36 / 0 / 48 / * / 30`（**5 行**）；**根 Grid 没有列定义**。

| 行 | 高 | 内容 | 实测 rect |
|---|---|---|---|
| 0 | 36 | 标题栏 Border（`DockPanel Margin="18,0"` + 菜单 + 右侧 "Crystal Studio"） | `Y=0 H=36` |
| 1 | 0 | 空保留行 | `H=0` |
| 2 | 48 | 工具选项条 Border（笔刷尺寸滑杆 `BrushSize` + `3×3` 徽标 + 城镇/墙体/拣选四组 WrapPanel） | `Y=36 H=48` |
| 3 | * | 工作区 Grid（见 3.3） | `Y=84` |
| 4 | 30 | 状态栏 Border（`Status` / `Coordinates` / `LibraryDot` / `LibraryStatus` / `Performance`） | `H=30, Y=null`（星号行后起点不可知） |

### 3.3 嵌套工作区 Grid（`MainWindow.axaml:7`）

列 = `72 / * / 5 / 360`（**4 列**），第 4 列 `MinWidth=300 MaxWidth=480`。

| 列 | 宽 | 内容 | 实测 rect |
|---|---|---|---|
| 0 | 72 | Border → `ResponsivePanel#ToolPanel`（`ItemMinWidth="36"`，10 个工具按钮） | `X=0 W=72` |
| 1 | * | `Grid` → `MapViewport#Viewport`（自绘画布）+ 缩放浮层 | `X=72 W=null` |
| 2 | 5 | `GridSplitter`（`ResizeBehavior=PreviousAndNext`） | `W=5` |
| 3 | 360 | Border → 右栏 Grid（`Margin="16,14"`，6 行：Layers / 资产头 / 资产区 / 分页 / 当前资产 / 检查器） | `W=360` |

### 3.4 10 个工具按钮（`MainWindow.axaml:9-19`）

`ToolButton` 主题：`32×32`、`Margin=2`、`GroupName=EditorTools`、圆角 5；图标 Viewbox `16×16`，内部 Canvas `24×24`（缩放比 0.667）。
**墙体工具是唯一例外：Viewbox `18×18`。**

| Tag | 图标类型 | 细节 |
|---|---|---|
| `Brush` | path | 2 条 Path（填充 + 描边 1.5）16×16 |
| `Pencil` | glyph | MDL2 `U+E70F` @16 |
| `Bucket` | path | 2 条 Path（描边 1.6 + 填充）16×16 |
| `Eraser` | glyph | MDL2 `U+E75C` @16 |
| `Select` | glyph | MDL2 `U+EF20` @16 |
| `Stamp` | glyph | MDL2 `U+E8B9` @16 |
| `Pickup` | glyph | MDL2 `U+E7C2` @16 |
| `Clone` | glyph | MDL2 `U+E8C8` @16 |
| `Wall` | path | 1 条多段 Path（描边 1.5）**18×18** |
| `SpecialBrush` | path | 1 条 Path（填充）16×16 |

### 3.5 各窗口节点数

| 窗口 | 尺寸 | 最小 | 节点 | 根 Grid 行 / 列 |
|---|---|---|---|---|
| MainWindow | 1500×950 | 1100×720 | 194 | `[36,0,48,*,30]` / 无列（嵌套 4 列） |
| ProvinceGeneratorWindow | 1100×780 | 850×650 | 53 | `[*,Auto,Auto]` / `[265,20,*]` |
| TempleGeneratorWindow | 1060×760 | 830×630 | 39 | `[*,Auto,Auto]` / `[260,20,*]` |
| TerrainGeneratorWindow | 1060×760 | 830×630 | 60 | `[*,Auto,Auto]` / `[280,20,*]` |
| TownStampWindow | 1060×760 | 850×610 | 64 | `[*,Auto,Auto]` / `[260,20,*]` |
| SpecialBrushWindow | 1140×820 | 1000×680 | 72 | `[Auto,*,Auto,Auto]` / 无列 |
| WallBrushWindow | 700×610 | 530×500 | 17 | `[Auto,Auto,Auto,*,Auto,Auto]` / 无列 |

---

## 4 与早期清单数字的差异（7 条，逐条摆出，未静默改）

| # | 早期清单写的 | 实测 | 处置 |
|---|---|---|---|
| 1 | "解析出的窗口数 = **9**" | 9 个 axaml 里只有 **7 个窗口**：`App.axaml` 根是 `Application`、`EditorControls.axaml` 根是 `ResourceDictionary`，**都不是窗口** | `windows` 只有 7 条；9 个文件全在 `xaml_files` 里按 role 标注；校验按 7 断言，并**显式记录早期口径 9** 的差异，不硬凑 |
| 2 | "主窗列 `72/*/5/360`（**列 4**）" | 根 Grid 只声明 5 行、**无列定义**；这 4 列属于 `MainWindow.axaml:7` 的**嵌套工作区 Grid** | `window.grid_columns=[]`；4 列记在该嵌套 Grid 节点的 `grid_columns` 上；验收门拆成"根行 5"+"嵌套列 4"两条断言 |
| 3 | `icon_sm=12`、`icon_md=16`、`icon_lg=20` | 工程里**没有 12 也没有 20 的图标尺寸**。实测：工具图标 16×16、墙体图标 18×18、复选框方块 14×14、图层眼睛 16×16、滑块拇指 12（宽）×18（高）、Canvas 设计栅格 24×24 | `icon_sm=14`、`icon_md=16`、`icon_lg=24`，另加 `icon_wall=18`、`icon_eye=16`；12 归到 `slider_thumb_width` |
| 4 | "FontFamily Segoe UI / FontSize 12" | 对，但**两套来源**：MainWindow 显式写 `Segoe UI`（单字体）；`App.axaml:85` 的 Window 样式是回退链 `Segoe UI, Microsoft YaHei, sans-serif`。6 个对话框里有 4 个写了 Background/Foreground 但没写字体，全部走样式 | `theme.font_family` 与 `font_family_fallback` 分开记，不合并 |
| 5 | "代码生成清单：3 个文件" | 3 个文件按要求列全（`DockWorkspace.cs` **20** 条 / `SettingsWindow.cs` **5** 条 / `StampCategoryPicker.cs` **6** 条 = 31），**另加 `MainWindow.axaml.cs` 21 条**，合计 52 | 因为运行期布局真值在那里（`InitializeDocking()` 拆 XAML 骨架 + 各面板停靠边/权重）。不列这段，Rust 侧会照着 XAML 复刻出一个实际不存在的界面 |
| 6 | "行 `36/0/48/*/30`"（当作静态骨架） | 行值对，但**第 2 行 48px 的工具选项条在停靠化后被置 0**（`MainWindow.axaml.cs:319`），内容搬进 Tool Options 面板；`BusyProgress` 从 Row 2 挪到 Row 0（`:321`） | `grid_rows` 记 XAML 声明值（属实），运行期变化写进 `code_built` + `unknowns` |
| 7 | 统计行格式 `windows=9, ...` | 实际 `windows=7, total_nodes=499, theme_colors=71, code_built=52, unknowns=18` | 原样打印见 §6，格式照抄、数字按实测 |

另有两处无实质影响的小偏差：早期清单写 `App.axaml` 93 行（实际 **94**）、`MainWindow.axaml` 72 行（实际 **73**），差额是文件末尾换行。

---

## 5 未解项（18 条，与 JSON `unknowns` 同源同序）

1. **Viewbox/Path 矢量图标**：只能拿到路径数据与视口尺寸（16×16 内含 24×24 Canvas，缩放比 0.667），最终栅格像素取决于 Avalonia `Stretch=Uniform` 实现与抗锯齿；按 16/24 换算，1.5px 描边落在约 1.0px。
2. **Segoe MDL2 Assets 字形**：只给码位（U+E70F 等），字形轮廓/字宽/基线依赖系统字体；缺字体即方框。Rust 侧须自备等效矢量图标（建议按 §3.4 的码位一一换）。
3. **Fluent 主题 11.3.4 的隐式模板**：Menu / MenuItem / ComboBox / ScrollViewer / Expander / TabControl / ListBox 在本工程**没有** ControlTheme，其内边距/项高/下拉层尺寸来自 Fluent 包，随版本变化。
4. **Menu/MenuItem 项高与内边距**：主窗菜单完全依赖 Fluent 默认模板，XAML 里拿不到像素。
5. **ResponsivePanel 自适应**：算法已读全（列数 `max(1,min(n,floor(width/ItemMinWidth)))`，行高取该行 `DesiredSize` 最大值），但 `DesiredSize` 依赖字体度量，最终列宽行高必须运行期算；`ItemMinWidth` 取值：ToolPanel=36、LayerList=160、Cell Highlights=160、InspectorDetails=245。
6. **EditorWindow.Opened 改写宽高**（`EditorWindow.cs:34-38`）：把 Width/Height 减去"外框−客户区"，使外框最终等于 1500×950，**客户区比它略小**；XAML 数字是外框目标值，不是控件可用面积。
7. **PopupHeight.Fit**：窗口 `Opened` 时重新拟合弹层高度（`PopupHeight.Avalonia.cs`），未逐行读完，弹层最终高度不可静态确定。
8. **运行期主窗布局被整体替换**：`MainWindow.axaml.cs:267-391` 的 `InitializeDocking()` 拆除 XAML 的 `72/*/5/360` 工作区 Grid，改由 `DockWorkspace` 接管；真实尺寸来自内嵌 `DefaultWorkspace.json`（左区 DockSize=156：Tools 174 / Tool Options 92 / Layers 262 / Cell Inspector 400；右区 DockSize=356：Minimap 220 / Palette 556 / Active Asset 189），以及用户 `profile/workspace.json` 的覆盖。注意 Layers、Cell Inspector 在代码里分别以 `Dock.Right`/`Dock.Bottom` 加入，却被默认布局改到 `Left(0)`。
9. **主题色运行期被重算**（`ThemeService.Avalonia.cs:35-57`）：所有 `Theme*` 键按亮度/蓝色偏移与系统 accent 混合，`Accent` 取系统强调色（缺省 RGB(132,152,255)），并动态生成 `OnAccent`/`CanvasBackground`/`CanvasPaper`/`CanvasGrid`；`App.axaml` 里的静态值只是"深色基线"。
10. **`OnAccent` 设计时不存在**（只在 `ThemeService:52` 运行期创建），但 `EditorControls.axaml:18` 的 `PrimaryButton.Foreground` 静态引用它 ⇒ 该前景色在 XAML 视图里不可知。
11. **字体度量**：所有 Auto/星号尺寸与文本宽度依赖 Segoe UI 12px 的度量与 DPI 缩放，无法静态换算像素；中文标签回退 Microsoft YaHei，宽度与拉丁字不同。
12. **MapViewport 的画布内容**（地图/网格/选择框）全部自绘（`Rendering/*.cs`），不产生 XAML 节点，不在本 JSON 覆盖范围。
13. **窗口非客户区**（标题栏/边框）高度由 OS 决定，不是 XAML 数字。
14. **对话框 `CenterOwner` 的屏幕坐标**取决于 Owner 位置。
15. **StackPanel/WrapPanel 内子元素绝对 y** = 累计 `DesiredSize` 的结果，静态不可知（这些节点的 `x/y` 为 `null`，最多只给 `w/h`）。
16. **AssetList 列数**：每项宽度写死 78（`MainWindow.axaml:66`），但列数由外层 WrapPanel 按面板宽度换行决定。
17. **ComboBox 下拉层 / ToolTip / ContextMenu** 的尺寸由 Fluent 模板与内容决定，不在 XAML 里。
18. **Grid 星号余量分配**：含 Auto 轨道时星号行/列的余量分配属 Avalonia 布局算法内部，本 JSON 只给轨道声明，不给最终像素。

---

## 6 JSON 的校验口径

`crystalstudio-ui.json` 不是手抄表，落盘前经过两道机器校验：

1. **抽取结果 vs 源文件**：另写一个独立脚本用 `xml.etree` 重新解析 9 个 `.axaml`，与产出 JSON 交叉校验
   （窗口数、主窗尺寸、根 Grid 轨道、节点总数），避免「读自己写过的中间结果」式的自证。
2. **行号可回源**：`code_built` / `metrics` / `semantic` / `inline_colors` 里每一条 `文件:行号`，
   写盘前都会**回读该行并断言目标片段存在**，一条不符就中止退出。

统计行（该快照的事实）：`windows=7, total_nodes=499, theme_colors=71, code_built=52, unknowns=18`。

> 这些数字是**某一版上游快照**的事实；源工程一变动就必须重新抽取、重新取基线，不要复用本文数字。

---
## 7 复现与维护

1. **抽取器**：一个纯函数式的 Python 脚本（标准库 `xml.etree` + `json` + `re`），按 §2 规则把 9 个 axaml 折成树，再拼上 `code_built` / `unknowns` 两张手写表。按"只允许写两个产出文件"的约束，脚本未落在仓库里（跑完即删），但规则在 §2 已完整写明，可直接照此重写。
2. **手写表自带校验**：`code_built`/`metrics`/`semantic`/`inline_colors` 里的每一条 `文件:行号`，在写盘前都会**回读该行并断言目标片段存在**，一条不符就中止退出——这轮就靠它抓出 22 处行号/写法偏差（如 `App.axaml:70` 的键是 `Muted/Accent/Line` 而非 `Theme929CAB`；`Color.FromRgb(228,158,151)` 没有十六进制字面量）。所以 JSON 里的行号是"机器验过的"，不是抄的。
3. **改动源工程后**：JSON 的断言值（窗口数 7、主窗数字、节点数 499）都是**该快照的事实**，源变了必须重新抽取并重新取基线，不要复用本文数字。
4. **给 Rust 侧的三条提醒**：
   - 别照 `MainWindow.axaml` 直接复刻主窗——运行期是 `DockWorkspace` 的停靠面板（§5 第 8 条）；
   - 颜色别只取 `App.axaml` 静态值——运行期会被 `ThemeService.ApplyPalette` 按系统 accent 重算（§5 第 9 条）；
   - 图标别只存 `Viewbox` 尺寸——墙体的 18×18 是例外，且 MDL2 字形需要自备矢量（§5 第 1、2 条）。
