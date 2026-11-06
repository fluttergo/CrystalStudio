//! 几何常量表（**所有尺寸的唯一来源**，代码里禁止再写"看起来差不多"的数字）。
//!
//! 计量对象：运行期截图 `layout/refs/crystalstudio-main-runtime-1500x950.png`（1500×950）。
//!
//! ## 截图内容区原点（关键，先读这段）
//! 该 PNG 里 **应用窗口不是从 (0,0) 铺满**：
//! - x=7 与 x=1492 是 1px 金色窗口边框（`#604715`，整列/整行恒定，实测 col x=7 连续 943 行同色）；
//! - x=0..6、x=1493..1499 是**身后的另一个窗口**（`#171717`/`#131315`），y=943..949 同理。
//!
//! ⇒ 应用**内容区原点 = (8, 0)**，内容区尺寸 = **1484 × 942**（x 8..1491，y 0..941）。
//!
//! 因此本文件所有 `REF_*` 常量都写成「相对内容区原点」的坐标，
//! 我们自己的窗口没有那条 1px 金边（无边框窗口），内容区原点就是 (0,0)，尺寸 1500×950。
//! 验收脚本 §七-4 也是按「各自内容区原点」去量的。
//!
//! 全部数值均由 Pillow 逐像素扫描得到（扫描脚本见交付汇报 §四）。

/// 参考截图内容区原点（应用窗口客户区左上角在 PNG 中的位置）。
pub const REF_CONTENT_ORIGIN: (f32, f32) = (8.0, 0.0);
/// 参考截图内容区尺寸（= 窗口客户区）。
pub const REF_CONTENT_SIZE: (f32, f32) = (1484.0, 942.0);

// ───────────────────────── 窗口 ─────────────────────────

/// 窗口客户区尺寸 1500×950。
/// 来源：`layout/crystalstudio-ui.json` → `windows[0].width/height`（= 1500/950），
/// 与参考 PNG 的 1500×950 一致（任务书 §三「窗口」）。
pub const WINDOW_W: f32 = 1500.0;
pub const WINDOW_H: f32 = 950.0;

/// 窗口最小尺寸 1100×720。来源：`crystalstudio-ui.json` → `windows[0].min_width/min_height`。
pub const WINDOW_MIN_W: f32 = 1100.0;
pub const WINDOW_MIN_H: f32 = 720.0;

// ───────────────────────── 顶/底条 ─────────────────────────

/// 标题栏高 31。来源：截图列 x=700 / x=1200，`y0..30` 均为 `#604715`，`y31` 起为 `#F5F6F7`。
pub const TITLE_BAR_H: f32 = 31.0;

/// 菜单栏高 35。来源：截图列 x=700、x=1200，`y31..65` 均为 `#F5F6F7`，`y66` 为分隔线 `#EAECEF`。
pub const MENU_BAR_H: f32 = 35.0;

/// 菜单栏下方 1px 分隔线。来源：截图 y=66 整行为 `#EAECEF`（x=50/152/180/1200 取样一致）。
pub const SEPARATOR_H: f32 = 1.0;

/// 状态栏总高 30 = 上沿 1px 分隔线(`#EAECEF`, y=912) + 29px 内容(`#F7F8F9`, y=913..941)。
pub const STATUS_BAR_H: f32 = 30.0;

/// 状态栏左右内边距 16。来源：状态栏左文本首像素 x=24 ⇒ 24 - 内容区原点 8 = 16；
/// 右侧 `Libraries not loaded` 末像素 x=1475 ⇒ 1491 - 1475 = 16。
/// 与 `crystalstudio-ui.json` → `theme.metrics.statusbar_margin_x` = 16 一致。
pub const STATUS_PAD_X: f32 = 16.0;

// ───────────────────────── 三列停靠 ─────────────────────────

/// 左停靠区宽 156。来源：截图 row y=600，`x8..163` 为 `#F5F6F7`（左面板底），`x164` 起进入中列。
pub const LEFT_DOCK_W: f32 = 156.0;

/// 右停靠区宽 356。来源：截图 row y=620，`x1136..1491` 为 `#F5F6F7`，`x1135` 为分隔线 `#E7EBEF`。
pub const RIGHT_DOCK_W: f32 = 356.0;

/// 停靠区与中列之间的 1px 竖分隔线（`#E7EBEF`）。来源：截图 x=224 与 x=1135。
pub const DOCK_DIVIDER_W: f32 = 1.0;

// ───────────────────────── 面板通用骨架 ─────────────────────────

/// 面板标题条高 23。来源：截图列 x=152，`y67..89`、`y228..250`、`y315..337`、`y554..576`
/// 四段均为 `#E3E6E9`，每段 23 行。（右停靠区同：`y67..89`、`y262..284`、`y748..770`。）
pub const PANEL_HEADER_H: f32 = 23.0;

/// 面板内容区内边距 5。来源：第一个工具按钮左边 x=15，左停靠区左边 x=8，
/// 且按钮自身还有 2px margin（见 `TOOL_MARGIN`）⇒ 15 - 8 - 2 = 5。
pub const PANEL_PAD: f32 = 5.0;

/// 面板标题文字左边距 7（相对面板左边）。来源：`Tools` 的 ⣿ 装饰符起于 x=15，左停靠区左边 x=8 ⇒ 7。
pub const PANEL_TITLE_PAD_L: f32 = 7.0;

/// 面板标题文字相对面板左边的偏移 21 = `PANEL_TITLE_PAD_L` 7 + 握把 6 + 间距 8。
pub const PANEL_TITLE_GAP: f32 = 8.0;

/// 面板标题右侧折叠箭头距右边缘 12。
pub const PANEL_CHEVRON_INSET: f32 = 12.0;

/// 面板标题字号 11。来源：`crystalstudio-ui.json` → `theme.metrics.glyph_mark_font_size` = 11
/// （与截图标题条 23px 高、字形约 8px 高相符）。
pub const PANEL_TITLE_FONT: f32 = 11.0;

/// 标题条左侧 ⣿ 装饰符尺寸：2 列 × 3 行小方点，整体 6 宽 × 10 高。
/// 来源：截图 (15,75)-(20,84) 的暗像素簇（`Tools` 标题左侧）。
pub const GRIP_W: f32 = 6.0;
pub const GRIP_H: f32 = 10.0;

// ───────────────────────── 左停靠区各面板高度 ─────────────────────────

/// Tools 面板总高 161（标题条 23 + 内容 138）。
/// 来源：截图列 x=152，标题条 `y67..89`，内容 `y90..227`（138 行）。
pub const PANEL_TOOLS_H: f32 = 161.0;

/// Tool Options 面板总高 87（标题条 23 + 内容 64）。
/// 来源：截图列 x=152，标题条 `y228..250`，内容 `y251..314`（64 行）。
pub const PANEL_TOOL_OPTIONS_H: f32 = 87.0;

/// Layers 面板总高 239（标题条 23 + 内容 216）。
/// 来源：截图列 x=152，标题条 `y315..337`，内容 `y338..553`（216 行）。
pub const PANEL_LAYERS_H: f32 = 239.0;

/// 右停靠区 Minimap 面板总高 195（标题条 23 + 内容 172）。
/// 来源：截图列 x=1480，标题条 `y67..89`，内容 `y90..261`。
pub const PANEL_MINIMAP_H: f32 = 195.0;

/// 右停靠区 Palette 面板总高 495（标题条 23 + 内容 472）。
/// 来源：截图列 x=1480，标题条 `y262..284`，内容 `y285..747`（463 行）；
/// 本单窗口高 950 比参考内容区 942 多 8px，多出的 9px 全部并入 Palette 内容区
/// ⇒ 463 + 9 = 472。Cell Inspector / Active Asset 两个「末位面板」吸收剩余高度。
pub const PANEL_PALETTE_H: f32 = 495.0;

// ───────────────────────── Tools 面板：工具按钮网格 ─────────────────────────

/// 工具按钮边长 32。来源：截图首个（选中）按钮的金色描边框 (15,97)-(46,128) = 32×32。
/// 与 `crystalstudio-ui.json` → `theme.metrics.tool_button` = 32 一致。
pub const TOOL_BTN: f32 = 32.0;

/// 工具按钮外边距 2。来源：`theme.metrics.tool_button_margin` = 2；
/// 截图校验：面板内容左边 8+5=13，按钮左边 15 ⇒ 差 2。
pub const TOOL_MARGIN: f32 = 2.0;

/// 工具按钮格距 36 = 32 + 2×2。来源：`theme.metrics.tool_item_min_width` = 36；
/// 截图校验：第 2 个按钮图标簇 (60,75) 相对第 1 个 (23,38) 偏移 37（含图标自身偏心 ±1）。
pub const TOOL_ITEM: f32 = 36.0;

/// 工具按钮网格列数 **4**。
/// ⚠ **与任务书 §三「2×5 图标网格」不符，以截图为准**：
/// 左停靠区可用内容宽 = 156 - 2×5 = 146，4×36 = 144 放得下，5×36 = 180 放不下；
/// 截图实测 10 个按钮排成 **4 / 4 / 2** 三行（行 y=97/133/169），列 x=15/51/87/123。
pub const TOOL_COLS: usize = 4;

/// Tools 内容区首个按钮的行偏移 7（内容区顶 y=90 → 按钮顶 y=97）。
/// = 面板内边距 5 + 按钮 margin 2。
pub const TOOLS_FIRST_ROW_OFF: f32 = 7.0;

// ───────────────────────── Tool Options 面板 ─────────────────────────

/// 色块 / 滑杆滑块 12 宽 × 16 高。来源：截图 `#AD7F22` 像素簇 (14,271)-(25,288) ⇒ 12×18，
/// 去掉上下各 1px 抗锯齿后取 12×16。
pub const SWATCH_W: f32 = 12.0;
pub const SWATCH_H: f32 = 16.0;

/// 滑杆轨道：起点相对面板左边 18（截图 x=26），长 64（到 x=90），高 3。
/// 来源：截图 y=280 行 `x26..90` 为 `#E3E6EA`；高度取自 `theme.metrics.slider_track_height` = 3。
pub const SLIDER_X: f32 = 18.0;
pub const SLIDER_W: f32 = 64.0;
pub const SLIDER_TRACK_H: f32 = 3.0;

/// `3×3` 胶囊标签：左 x=92（相对面板 84）、宽 45、高 22。
/// 来源：截图 y=280 行 `x92..136` 为 `#ECEEF0`。
pub const PILL_X: f32 = 84.0;
pub const PILL_W: f32 = 45.0;
pub const PILL_H: f32 = 22.0;

// ───────────────────────── 笔刷尺寸（3..=31 奇数） ─────────────────────────

/// 笔刷尺寸的下限 / 上限 / 步进 —— **与官方滑块逐字一致**。
///
/// 真值：`CrystalStudio/MainWindow.axaml:6`
/// `<Slider x:Name="BrushSize" Minimum="3" Maximum="31" Value="3"
///   TickFrequency="2" IsSnapToTickEnabled="True" …/>`
/// ⇒ 3 / 5 / … / 31 共 15 档，**只有奇数**（`TickFrequency=2` + 吸附）。
///
/// 键盘 `[` / `]` 各走 **2**（保持奇数），并夹在 3..31：
/// `MainWindow.axaml.cs:871`
/// `BrushSize.Value = Math.Clamp(BrushSize.Value + (e.Key == Key.OemOpenBrackets ? -2 : 2), 3, 31);`
///
/// 视图层的同一口径：`MapViewport.cs:27`
/// `public int BrushSize { get => brushSize; set => brushSize = Math.Clamp(value | 1, 3, 31); }`
/// —— `value | 1` 就是"偶数一律抬成奇数"，与本文件的 `clamp_brush_size` 同一判据。
pub const BRUSH_MIN: usize = 3;
pub const BRUSH_MAX: usize = 31;
pub const BRUSH_STEP: usize = 2;
/// 缺省尺寸 3（`MainWindow.axaml:6` 的 `Value="3"`，也是胶囊初始文案 `3×3` 的来历）。
pub const BRUSH_DEFAULT: usize = 3;

/// 把任意尺寸夹进「3..=31 的奇数」—— **唯一的规范化入口**。
///
/// 先夹范围再抬奇数（`| 1`），于是 `0/1/2` → 3、`32/40` → 31、`4` → 5、`8` → 9。
/// 与 `MapViewport.cs:27` 的 `Math.Clamp(value | 1, 3, 31)` 等价
/// （差别只在"先夹后抬"与"先抬后夹"在边界上的次序，两处都夹到 3..31 的奇数集合内）。
pub const fn clamp_brush_size(n: usize) -> usize {
    let v = if n < BRUSH_MIN { BRUSH_MIN } else { n };
    let v = if v > BRUSH_MAX { BRUSH_MAX } else { v };
    // 抬成奇数：偶数 +1（31 是奇数，+1 只会发生在被上面夹住的偶数上，故不会越界）
    if v % 2 == 0 {
        if v + 1 > BRUSH_MAX {
            BRUSH_MAX
        } else {
            v + 1
        }
    } else {
        v
    }
}

// ───────────────────────── 填充（Bucket） ─────────────────────────

/// 单次填充的**格数上限 200,000**（防呆：不让一次误点把整张图吞进撤销栈）。
///
/// 来源：任务书 §三.2 指定的硬上限（官方 C# 无此保护 —— `MapViewport.cs:485-539`
/// 的 `BucketFill` 直接 BFS 到底，`visited` 只防重复不防规模）。
/// 超过即**报错且一格不写**（见 `edit::EditSession::fill`）。
///
/// 量级参照：700×700 = 490,000 格 ⇒ 一张"全图同图号"的极端图必被拦住；
/// 一格 `Change` 约 76 B，200,000 格 ≈ 15 MB 撤销步，仍在可接受范围。
pub const FILL_MAX_CELLS: usize = 200_000;

// ───────────────────────── Layers 面板 ─────────────────────────

/// 层列表行高 26。来源：截图选中行 `Ground Tiles` 的金色框 (17,342)-(155,367) = 26 行；
/// 相邻行眼睛图标 y 中心间隔亦为 26。与 `theme.metrics.layer_row_min_height` = 26 一致。
pub const LAYER_ROW_H: f32 = 26.0;

/// 层列表行左边缩进 9（左停靠区左边 x=8 → 行左边 x=17）。
pub const LAYER_ROW_INSET_L: f32 = 9.0;
/// 层列表行右边缩进 8（左停靠区右边 x=163 → 行右边 x=155）。
pub const LAYER_ROW_INSET_R: f32 = 8.0;

/// 层列表第一行顶部偏移：内容区顶 y=338 → 行顶 y=342，偏移 4。
pub const LAYER_FIRST_ROW_OFF: f32 = 4.0;

/// 眼睛按钮宽 26。来源：`crystalstudio-ui.json` → `GroundVisible` 节点 `rect.w` = 26。
pub const LAYER_EYE_BTN_W: f32 = 26.0;

/// 眼睛图标 16×16。来源：截图 (26,348)-(45,364) 图标簇 ≈ 16；
/// 与 `theme.metrics.icon_eye` = 16 一致。
pub const ICON_EYE: f32 = 16.0;

/// 子组 `Cell Highlights` 前的分隔线之上留白 6：
/// 截图第 5 行（High Collisions）占 y=446..471，分隔线在 y=478 ⇒ 472..477 共 6 行留白。
pub const LAYER_GROUP_GAP: f32 = 6.0;

/// `Cell Highlights` 小标题高 22：截图分隔线 y=478，`Light Cells` 行自 y=500 起 ⇒ 478..499 共 22 行。
pub const LAYER_GROUP_LABEL_H: f32 = 22.0;

/// 复选框边长 14。来源：截图 Light Cells 复选框 (23,500)-(37,514)；
/// 与 `theme.metrics.checkbox_box` = 14 一致。
pub const CHECKBOX: f32 = 14.0;

// ───────────────────────── Layers 面板内部排版（原为 dock_left.rs 字面量，本轮上收） ─────────────────────────

/// 眼睛按钮盒子相对行左边的内缩 4。
pub const LAYER_EYE_INSET: f32 = 4.0;

/// 眼睛盒子右边 → 图层名文字左边的间距 2。
pub const LAYER_TEXT_GAP: f32 = 2.0;

/// 碰撞标记色块 7×7（截图 Low/High Collisions 行里的小方块）。
pub const LAYER_MARK_W: f32 = 7.0;

/// 标记块左边缘 → 文字左边缘的偏移 13（块 7 宽 + 6 间距）。
pub const LAYER_MARK_TEXT_OFF: f32 = 13.0;

/// 子组标题距分隔线底边 4（截图分隔线 y=478、标题文字 y=485 起）。
pub const LAYER_GROUP_TITLE_OFF: f32 = 4.0;

/// 子组标题字号 11（截图 `Cell Highlights` 比图层名小一档）。
pub const LAYER_GROUP_TITLE_FONT: f32 = 11.0;

/// 复选框相对行左边的内缩 6。
pub const LAYER_CB_INSET: f32 = 6.0;

/// 复选框 → 标签文字的左偏移 27（相对行左边）。
pub const LAYER_CB_TEXT_X: f32 = 27.0;

/// 「当前编辑层」行左侧金条宽 3px（贴在 `ACCENT_DARK` 的 1px 金框内沿）。
///
/// **取值依据**：参考截图里选中行 (17,342)-(155,367) 只有 1px 金框 + 浅金底
/// （`#AD7F22` / `#F7EFDE`，见 [`crate::theme`]）。1px 在 1500×950 的截图里与"眼睛开关"
/// 混在一起不好分辨，故本版把"当前编辑层"再加一根 **3px 竖条**（同一支金色 `ACCENT_DARK`，
/// 不引入新颜色）。3 是 26px 行高下肉眼可辨的最小宽度（2px 在 100% 缩放下会与边框糊成一片）。
pub const LAYER_EDIT_BAR_W: f32 = 3.0;

/// 金条相对行内沿的偏移 1（正好压在 1px 金框的内侧，不与边框重叠）。
pub const LAYER_EDIT_BAR_INSET: f32 = 1.0;

// ───────────────────────── Tool Options 面板内部 ─────────────────────────

/// 色块相对内容区左边的内缩 6。
pub const SWATCH_INSET: f32 = 6.0;

// ───────────────────────── Cell Inspector 面板内部排版 ─────────────────────────
//
// 这些值原先是 `dock_left.rs` 里的字面量（9 / 75 / 15 / 11 / 4）。本轮为中文排版核对
// 上收到常量表：验收要逐条算"文字右边界 vs 裁剪盒右边界"，散在绘制代码里没法程序化核对。

/// 信息头距内容区顶 10。
pub const INSPECTOR_HEAD_OFF: f32 = 10.0;

/// 键列左边相对面板内容左边 9。来源：截图键文本起 x=17，面板左边 x=8。
pub const INSPECTOR_KEY_X: f32 = 9.0;

/// 值列左边相对面板内容左边 75。
///
/// 截图值文本起 x=79（相对 71），这里取 75 是为了给键列留出比英文更宽松的裁剪余量：
/// 中文键最长 `图块动画` 4 字 ≈ 44px，键列直到 `INSPECTOR_VAL_X - INSPECTOR_KEY_CLIP_PAD` = 71
/// 都放得下。
/// **中文实测调整：无**（值列可用宽 = `LEFT_DOCK_W - 75` = 81px，各条中文值均已换行适配，
/// 见 `ui_text::VAL_METADATA` 的说明）。
pub const INSPECTOR_VAL_X: f32 = 75.0;

/// 键列裁剪盒右边界再往左 4（键文本最多画到值列左边 -4）。
pub const INSPECTOR_KEY_CLIP_PAD: f32 = 4.0;

/// 行高 15（截图相邻行文字基线差 15）。
pub const INSPECTOR_LINE_H: f32 = 15.0;

/// 字号 11（截图 Inspector 文字明显小于正文 12）。
pub const INSPECTOR_FONT: f32 = 11.0;

/// 每条 `(键, 值)` 之后的行间隔 4。
pub const INSPECTOR_ROW_GAP: f32 = 4.0;

// ───────────────────────── 中列：页签条 / 画布 / 网格 ─────────────────────────

/// 页签条高 38。来源：截图列 x=700，`y66..103` 为 `#F5F6F7`（页签条底），`y104` 起为 `#F9FAFB`（视口底）。
pub const TABSTRIP_H: f32 = 38.0;

/// 页签高 30，宽 98。来源：截图页签 `Untitled 1 ×` 占 `x164..262`（98 宽）、`y66..102`。
/// 下划线（选中指示）2px，取自截图 y=103..104 的 `#AD7F22`。
pub const TAB_W: f32 = 98.0;
pub const TAB_H: f32 = 30.0;
pub const TAB_UNDERLINE_H: f32 = 2.0;

/// 世界格尺寸 48 宽 × 32 高（**像素**）。
///
/// 来源（三重一致，本单只依赖这一条）：
/// 1. 任务书 §三「48×32 网格线」；
/// 2. 截图校验：水平网格线 y=175/207/239/271…（步长 32），竖向步长 48；
/// 3. 客户端真值 `miredit_render::CELL_W/CELL_H`（= `Grobal.h:1980-1981` UNITX/UNITY，
///    Godot 侧 `map_view.gd:9-10`）—— 单元测试 `世界格与渲染库一致` 会断言三者数值相等。
///
/// ⚠ 本单起这两个常量不再是"网格线间距"，而是**真实地图的格像素**：
/// 视口里贴的是 `miredit_render::render_region` 出的 1:1 位图，缩放只作用于贴图矩形。
pub const CELL_W: f32 = 48.0;
pub const CELL_H: f32 = 32.0;

// ───────────────────── 单元格网格（视图 → 单元格网格，2026-11-05 新增） ─────────────────────
//
// 网格本身画在世界格边界上（间距就是上面的 `CELL_W` / `CELL_H`，**不另立一套间距**），
// 这里只放"怎么画"的常量：**线多粗**。

// 网格**没有任何"多小就不画"的缩放门槛**：上游 `901dbe2f`（2026-11-05 07:17）删掉了
// `if (GridVisible && Zoom >= .4)`，改成「永远画 + 缩小时按 `step` 抽稀」
// （`MapViewport.Presentation.cs:51-61@901dbe2f`，判据 `while (step * 32 * Zoom < 8) step *= 2`）。
// ⇒ 抽稀步长的唯一出口是 `crate::grid::step_for`。本文件**不许**再加缩放阈值
//   （原 `GRID_MIN_SCALE = 0.5` 与后来的 `grid::MIN_SCALE = 0.4` **都已删除**）。

/// 网格线宽 **1 屏幕像素**（恒 1，**不随缩放变**）。
///
/// 官方口径是**世界单位**笔宽 `1 / Zoom`（`MapViewport.Presentation.cs:57@901dbe2f`
/// `new ScenePen(ThemeService.Brush("CanvasGrid"), 1 / Zoom)`；旧版固定 0.6 那行已随
/// `901dbe2f` 删除）—— 乘回 `Zoom` 后**屏幕恒 1px**，与本常量一致。
///
/// 为什么不像地图那样按 `scale` 放大：网格是**读图辅助线**，不是地图内容。
/// 2× 时若线也变 2px，一屏的线面积就翻 4 倍，压图更重；而且"1px 网格"是编辑器界面的
/// 通行观感（Aseprite / Tiled 的格线都是 1px）。取整到整像素的理由见 `crate::grid` 模块文档。
pub const GRID_LINE_W: f32 = 1.0;

// ───────────────────────── 中列：真实地图画布（本单新增） ─────────────────────────
//
// 这一组常量的来源分两类，逐条注明：
// - 「任务书」：任务书 §三.4 直接给定的数值（缩放区间）；
// - 「推导」：由既有几何常量推出来、截图量不到的新量（拖动余量、重渲上限）。

/// 画布最小缩放 0.25（= 25%）。来源：任务书 §三.4「滚轮以鼠标位置为中心缩放（0.25×–4×，1×=100%）」。
pub const CANVAS_MIN_SCALE: f32 = 0.25;

/// 画布最大缩放 4.0（= 400%）。来源：同上。
pub const CANVAS_MAX_SCALE: f32 = 4.0;

/// 滚轮每一「格」的缩放倍率 1.15。来源：推导 —— 25%→400% 共 16 档，
/// `1.15^16 ≈ 9.36` 略大于 16 倍，实际约 20 档，手感与常见编辑器一致；
/// 不用 1.25（25%→400% 只需 7 档，滚一格跳太多）。
pub const ZOOM_PER_NOTCH: f32 = 1.15;

/// 滚轮「一格」对应的滚动像素 50（egui `smooth_scroll_delta` 的单位是像素，
/// 鼠标滚轮一格约 50px）。来源：推导 + egui 默认行高约定。
pub const SCROLL_PX_PER_NOTCH: f32 = 50.0;

/// 鼠标所在格的高亮描边宽 2。来源：截图金色选框描边 2px（x=607..608 连续两列金色像素）——
/// 本单把这个已实测的值从"装饰框"改用到"悬停格边框"，宽度不变。
pub const CELL_HOVER_STROKE: f32 = 2.0;

/// 重渲时为平移/大物件预留的余量 **10 格**（960×640 世界像素）。
///
/// 来源：**实测推导，不是拍的**（2026-11-04，`0.map` + `--at=380,310` 客户端同视点比对）：
///
/// | 余量 | 本机渲出的窗口 | 与客户端 16×16 分块命中率 |
/// |---|---|---|
/// | 3 格 | 28×33 | **79.77%** ← 城门楼大片缺失 |
/// | 6 格 | 34×39 | 98.12% |
/// | 10 格 | 42×47 | **99.96%** |
/// | 14 / 20 格 | 50×55 / 62×67 | 99.96%（饱和，再多没用）|
///
/// 为什么需要这么大：`miredit-render` 只画**窗口内**的格子，而表层（物件）是
/// **底边钉格底**贴整帧的，一帧城门楼宽达 300~500 px（>6 格）—— 锚点落在窗口外
/// 但帧体压进可视区的物件，如果不在窗口里就整块不画。3 格（144 px）远远不够。
/// 10 格 = 480 px 已覆盖实测最大帧体；14/20 格不再提升，说明 10 格是真实阈值。
pub const CANVAS_RENDER_MARGIN_CELLS: u32 = 10;

/// 一次性重渲的格数上限 16000（≈ 16000×48×32×4 B ≈ 98 MB RGBA）。
///
/// 来源：推导（防御性护栏，不是语义限制）。正常 1500×950 窗口：
/// 1× 时可见约 21×26 格、加余量 41×46 = 1886 格；0.25× 时可见约 83×102 格、
/// 加余量 103×122 = 12566 格 —— 都在上限内。只有把窗口拉到 4K 再缩到 0.25×
/// 才可能触碰，触到会**如实报错并保持上一帧纹理**，不会去分配几百 MB 把界面卡死。
pub const CANVAS_RENDER_MAX_CELLS: u32 = 16_000;

/// 打开程序时的默认画布缩放 1.0（= 100%，1 像素 = 1 世界像素）。
pub const CANVAS_DEFAULT_SCALE: f32 = 1.0;

/// 缩放浮层（`−  100%  +  |  Fit Map  原始大小`）尺寸 **292**×40，距视口左/下各 18。
///
/// 来源分两段：
/// * **228×40** 是 `901dbe2f` 之前的截图量值：外框 (182,854)-(409,893)；
///   视口左 x=164 ⇒ 182-164 = 18；视口底 y=911 ⇒ 911-893 = 18；
/// * **+64** 来自上游 `901dbe2f` 在浮层尾部新增的 `OriginalSizeButton`
///   （`MainWindow.axaml:22`，`Click="ResetZoomClick"`）。按上游同一套
///   `Padding="10,4"` 反推：4 字中文 ≈ 48 + 左右内边距 20 ≈ 56~58 宽，
///   与前一个按钮之间留 8 ⇒ 228 + 8 + 56 = 292。**没有截图可量**（上游该版截图未随仓提供），
///   故这一段的依据是"上游控件顺序 + 同一条内边距规则"，已在模块文档 §五 记为推导值。
pub const ZOOM_PILL_W: f32 = 292.0;
pub const ZOOM_PILL_H: f32 = 40.0;
pub const ZOOM_PILL_OFF: f32 = 18.0;

/// 缩放浮层内部各元素的横向位置（相对浮层左边）。
/// 来源：截图浮层左 x=182，`−` 中心 204.5、`100%` 中心 255.5、`+` 中心 306.5、
/// 竖线 x=332、`Fit Map` 起 x=351 ⇒ 相对左边 22.5 / 73.5 / 124.5 / 150 / 169。
pub const ZOOM_MINUS_X: f32 = 22.5;
pub const ZOOM_PERCENT_X: f32 = 73.5;
pub const ZOOM_PLUS_X: f32 = 124.5;
pub const ZOOM_DIVIDER_X: f32 = 150.0;
pub const ZOOM_FIT_X: f32 = 169.0;

/// 「适应地图」按钮的可点带右边界（相对浮层左边）= 169 + 55。
///
/// 55 是截图里 `Fit Map` 文字宽度（5 字英文 ≈ 45）加两侧留白得到的实测值；
/// **必须显式写出来**：加「原始大小」之前，这条带是"一直延伸到浮层右边缘"的
/// （`ZOOM_PILL_W - 4`），加了新按钮后若还那么算，两个按钮的命中盒会重叠。
pub const ZOOM_FIT_R: f32 = 224.0;

/// 「原始大小」按钮的起点（相对浮层左边）= `ZOOM_FIT_R` 224 + 按钮间距 8。
///
/// 上游顺序：`−  100%  +  |  适应地图  原始大小`（`MainWindow.axaml:22` 的
/// `FitMapButton` 之后紧跟 `OriginalSizeButton`）。
pub const ZOOM_ORIGINAL_X: f32 = 232.0;

/// 「原始大小」按钮的可点带右边界（相对浮层左边）= 232 + 56（4 字中文 + 两侧留白）。
/// 浮层右边缘 = 292 - 4 = 288，正好给这条带留 4px 右内边距。
pub const ZOOM_ORIGINAL_R: f32 = 288.0;

/// `−` / `+` 字形字号 13（截图两符号明显大于 `100%` 那行正文）。
pub const ZOOM_SIGN_FONT: f32 = 13.0;

/// 页签标题距页签左边 11（截图 `Untitled 1` 文字起 x=175，页签左 x=164）。
pub const TAB_TITLE_INSET: f32 = 11.0;

// ───────────────────────── 中列：地图列表面板（2026-11-05，上游 901dbe2f 移植） ─────────
//
// 为什么这一组不在截图里：`crystalstudio-main-runtime-1500x950.png` 是 `901dbe2f` **之前**
// 的运行截图，那时上游还没有地图列表面板。上游把它加在**左停靠区第一个**
// （`MainWindow.axaml.cs:343` 的 `docking.Add("Maps", …, Dock.Left, 2)`），
// 而本仓左停靠区是**固定几何、156 宽、四块面板已排满**（实测单元格检查器只剩 96px 余量，
// 见 `layout` 的 `左停靠区余量` 测试）—— 硬塞第五块面板会把检查器最后两行挤出面板。
// 所以本仓把这块面板做成**中列左侧的可开合列**：开时把画布往右推（不遮挡地图）、
// 关时画布矩形与旧版**逐像素相同**（默认态不变，见 `Layout::compute` 与 `compute_with`）。
//
// 尺寸依据：
// * 宽 268 —— 推导：一行要放下「文件名 + 大小」两列。文件名按 `n0104.map` 这类
//   9 字符 @11px ≈ 54px；大小按 `697.2 MB` 8 字符 @10px ≈ 48px；加左右内边距
//   (6+6) 与两列间距 8 ⇒ 122 是下限。取 268 是为了**长文件名**（如
//   `n0104_副本_2026.map`）也能整行显示，且不超过中列的 1/3（987/3 = 329）。
// * 行高 22 —— 与上游 `MapFileList.ItemContainerTheme` 的
//   `HeightProperty = 22.0`（`MainWindow.MapBrowser.cs:32-34`）逐值相同。
// * 字号 11 / 10 —— 上游列表项 `FontSize = 11`（`MapBrowser.cs:37`），
//   状态行 `FontSize = 10`（`MapBrowser.cs:12`）。

/// 地图列表面板宽度 268。
pub const MAPS_W: f32 = 268.0;

/// 面板内容左右内边距 6。来源：上游 `CreateMapBrowser` 的 `Grid { Margin = new Thickness(6) }`。
pub const MAPS_PAD: f32 = 6.0;

/// 面板内工具条高度 26（放「刷新」按钮；上游同样有一行 `mapFolderButton / mapRefreshButton`）。
pub const MAPS_TOOLBAR_H: f32 = 26.0;

/// 「刷新」按钮尺寸 58×20（文字 2 字 + 内边距）。
pub const MAPS_BTN_W: f32 = 58.0;
pub const MAPS_BTN_H: f32 = 20.0;

/// 列表行高 22。来源：上游 `MapFileList` 的 `ItemContainerTheme`
/// `Layoutable.HeightProperty = 22.0` / `MinHeightProperty = 22.0`（`MapBrowser.cs:32-34`）。
pub const MAPS_ROW_H: f32 = 22.0;

/// 行内文件名字号 11（上游 `MapBrowser.cs:37` 的 `FontSize = 11`）。
pub const MAPS_ROW_FONT: f32 = 11.0;

/// 行内大小字号 10（比文件名小一档：它是辅助信息，不该抢文件名）。
pub const MAPS_SIZE_FONT: f32 = 10.0;

/// 行内左右内边距 4（上游 `TextBlock { Margin = new Thickness(3, 0) }`）。
pub const MAPS_ROW_PAD: f32 = 4.0;

/// 文件名列与大小列之间的间距 8（两列不贴在一起，长的文件名才不会被数字糊住）。
pub const MAPS_COL_GAP: f32 = 8.0;

/// 底部状态行高 18（上游 `mapFileStatus` 是最后一行 `Auto`，`FontSize = 10`）。
pub const MAPS_STATUS_H: f32 = 18.0;

/// 底部状态行字号 10（上游 `MapBrowser.cs:12`）。
pub const MAPS_STATUS_FONT: f32 = 10.0;

/// 当前地图行左侧的金条宽 3（与图层行的「当前编辑层」金条同一视觉语言）。
pub const MAPS_CURRENT_BAR_W: f32 = 3.0;

/// 列表滚动条宽 6（与资源面板的滚动条同宽）。
pub const MAPS_SCROLLBAR_W: f32 = 6.0;

/// 每帧扫描预算 4 ms（**分帧扫描**的上限；见 `crate::maplist` 模块文档 §三）。
///
/// 为什么是 4 ms：60 fps 的帧预算 16.7 ms，扫描最多占 1/4，输入与绘制都不受影响；
/// 实测 1,097 张图的**整段**扫描约 10~14 ms（见 `maplist::tests::实测扫描耗时`），
/// 4 ms/帧 ⇒ 3~4 帧扫完，用户看不到停顿。
pub const MAPS_SCAN_BUDGET_MS: u64 = 4;

/// 每帧最多处理的目录项数 512（**第二道**限时闸门：目录项在缓存里时
/// `metadata()` 只要几微秒，纯按时间限会让单帧处理量不可预测）。
pub const MAPS_SCAN_MAX_ITEMS: usize = 512;

// ───────────────────────── 右停靠区 ─────────────────────────

/// 右停靠区内容左右内边距 10。来源：截图组合框左边 x=1146，右停靠区左边 x=1136 ⇒ 10；
/// 组合框右边 x=1482，右停靠区右边 x=1491 ⇒ 9（取 10）。
pub const RIGHT_PAD_X: f32 = 10.0;

/// TextBox / ComboBox 高 32。来源：截图 Terrain Patterns 组合框 (1146,352)-(1482,383) = 32 行；
/// 与 `theme.metrics.combo_min_height` = 34 相差 2（以截图的 32 为准）。
pub const COMBO_H: f32 = 32.0;

/// 搜索框高 29。来源：截图 (1146,396)-(1482,424) = 29 行。
pub const SEARCH_H: f32 = 29.0;

/// 页签（Library / Favourites / Recently Used / Categories）行高 30。
/// 来源：截图 Palette 页签行 y=285..315（`#F5F6F7`），下划线在 y=312..315。
pub const PALETTE_TABS_H: f32 = 30.0;

/// Palette 页签字号 11 = `theme.metrics.tab_item_font_size`。
pub const TAB_FONT: f32 = 11.0;

/// 主按钮（Set Library Folder）尺寸 191×29。
/// 来源：截图金色像素 (1218,604)-(1408,632) ⇒ 191×29。
pub const PRIMARY_BTN_W: f32 = 191.0;
pub const PRIMARY_BTN_H: f32 = 29.0;

/// Minimap 内部地图块尺寸 200×138。
/// 来源：截图 row y=150，`#EFF2F5` 区段 `x1214..1413`（200 宽）；
/// 纵向 `y105..242`（138 高，由列 x=1480 的 `#F9FAFB` 区段 y100..246 内缩得到）。
pub const MINIMAP_MAP_W: f32 = 200.0;
pub const MINIMAP_MAP_H: f32 = 138.0;

/// Minimap 浅底内框（`#F9FAFB`）相对面板内容的左右/上下内缩 10。
/// 来源：截图 row y=150，内框 `x1146..1481`；面板内容左右边 = 1136/1491 ⇒ 各内缩 10。
pub const MINIMAP_INNER_INSET: f32 = 10.0;

// ⚠ 已删除 `MINIMAP_VIEWBOX = 14.0`（原「视口指示小方框 14×14」）。
//
// 删除理由（审计 E10，2026-11-04）：那个方块的位置恒为 `(块左上+2, 块左上+2)`、
// 尺寸恒 14×14，与 `pan` / `scale` / 地图尺寸**全都无关** —— 它用几何形状宣称
// "你的视口在整图左上角"，属于假数据。现行几何由 `dock_right::minimap_rects` 按
// 真实地图格数 + `CanvasView::visible_cells` 现算，**不再需要任何尺寸常量**。
//
// 这段用 `//` 而不是 `///`：它讲的是**已经不存在**的常量，不是下面那个常量的文档。
// （原先是 `///`，紧跟一个空行再接 `MINIMAP_VIEWBOX_MIN_PX` 的文档 ⇒ clippy
//  `empty_line_after_doc_comments` 报"这段文档注释后面跟了空行"。）

/// 小地图视口框的**最小可见边长** 1px（纯栅格化下限，不是"编的尺寸"）。
///
/// 为什么需要：视口缩到极小（例如 4× 放大时可见区只有几格）时，按真实比例换算出的框
/// 边长会 **< 1px**，`Rect` 退化后连边框都画不出来 —— 那等于"看不见"，
/// 同样是失真。抬到 1px 只是承认"屏幕上最近的相邻像素就是 1px"，位置仍严格按真实值算。
pub const MINIMAP_VIEWBOX_MIN_PX: f32 = 1.0;

/// 状态栏 `Cell 11, 22` 文本右端到 `● Libraries not loaded` 圆点左端的间距 52。
/// 来源：截图坐标文本末像素 x=1323、圆点起 x=1375 ⇒ 52。
pub const STATUS_COORD_GAP: f32 = 52.0;

/// 状态栏圆点直径 6，与右侧文字间距 8。
/// 来源：截图圆点 (1375..1380) = 6 宽；文字起 x=1388 ⇒ 间距 8。
/// `theme.metrics` 亦有 `LibraryDot` `rect.w` = 6。
pub const STATUS_DOT_D: f32 = 6.0;
pub const STATUS_DOT_GAP: f32 = 8.0;

/// 状态栏「左段 → 工具 → 笔刷」两段之间的间距 16。
///
/// 来源：**推导**（本单新增段，参考截图里没有"工具/笔刷"这两段可比对）。
/// 取 16 = [`STATUS_PAD_X`]（状态栏左右内边距），与整条状态栏的呼吸一致；
/// 实测可用宽度：左段 `0.map · 700×700 格` 约 130px，右端坐标段左边界约 1250px，
/// 两段合计最坏 **285.3px**（`工具：橡皮擦` 72.0 + `笔刷：地表图层 图号 65535 · 资源库 255` 213.3；
/// 2026-11-04 用 `paint::text_w` 实测，见状态栏改名「橡皮擦」后的复测），余量充足
/// （典型态落笔段仍余 572.5px）。
pub const STATUS_SEG_GAP: f32 = 16.0;

// ───────────────────────── 关闭拦截模态（本单新增控件） ─────────────────────────

/// 关闭拦截模态的底板尺寸 560×92。
///
/// 来源：**推导**（参考截图里没有这个控件，它只在"有未保存修改时点关闭"才出现）。
/// 文案 `有未保存的修改，按 Ctrl+S 保存 / 再点关闭放弃` = 20 个汉字 + 3 个拉丁词，
/// 13px 下实测约 340px 宽 ⇒ 560 留出左右各 110px 的呼吸，居中于画布不显得挤。
pub const MODAL_W: f32 = 560.0;
pub const MODAL_H: f32 = 92.0;
/// 模态文字字号 13（与标题栏同档；它是一句需要看清的警告，不用 11px 小字）。
pub const MODAL_FONT: f32 = 13.0;
/// 模态底板圆角 6（与缩放浮层同档，见 [`crate::canvas`] 的 `fill_round(.., 6.0, ..)`）。
pub const MODAL_RADIUS: f32 = 6.0;

// ───────────────────────── Active Asset 面板卡片 ─────────────────────────

/// Active Asset 内容卡片：334 宽 × 109 高，相对面板内容左上偏移 (11, 16)。
/// 来源：截图 `#EFF1F3` 卡片 `x1147..1480`、`y787..895`；
/// 面板内容左边 x=1136、上边 y=771 ⇒ 偏移 (11, 16)。
pub const AA_CARD_OFF: (f32, f32) = (11.0, 16.0);
pub const AA_CARD_W: f32 = 334.0;
pub const AA_CARD_H: f32 = 109.0;

/// 卡片内预览框：188 宽 × 88 高，相对卡片左上偏移 (10, 10)。
/// 来源：截图 `#F7F7F8` 区段 `x1157..1344`（188 宽）。
pub const AA_PREVIEW_OFF: f32 = 10.0;
pub const AA_PREVIEW_W: f32 = 188.0;
pub const AA_PREVIEW_H: f32 = 88.0;

/// Palette 分页按钮 `←` / `→` 尺寸 31×22，分页行高 22。
/// 来源：截图 row y=703，`#F2F3F5` 区段 `x1150..1180` 与 `x1447..1477`（各 31 宽）。
pub const PAGER_BTN_W: f32 = 31.0;
pub const PAGER_ROW_H: f32 = 22.0;

/// 分页按钮相对 Palette 内容区左右的内缩 4。
pub const PAGER_BTN_INSET: f32 = 4.0;

/// 分页按钮里箭头字形的字号 12（`←` / `→` 用真字形时的显示字号）。
pub const PAGER_ARROW_FONT: f32 = 12.0;

/// Palette 页签行左内缩 6（截图第一个页签文字起 x=1152，内容区左右内边距右界 x0=1146）。
pub const PALETTE_TAB_INSET_L: f32 = 6.0;

/// Palette 页签下划线高 3（截图 y=312..315 的金色）。
pub const PALETTE_UNDERLINE_H: f32 = 3.0;

/// Palette 页签下划线左右各外扩 6（宽 = 文字宽 + 12）。
pub const PALETTE_UNDERLINE_PAD: f32 = 6.0;

/// Palette 顶部 `资源库` / `帧 N` 标签字号 11。
pub const PALETTE_LABEL_FONT: f32 = 11.0;

/// 搜索框文字与右侧清除符的左右内缩 12（截图占位文字起 x=1158，输入框左 x=1146）。
pub const SEARCH_TEXT_INSET: f32 = 12.0;

/// Active Asset 预览框右边 → 右侧三行文字左边的间距 8。
pub const AA_TEXT_GAP: f32 = 8.0;

/// Active Asset 右侧三行文字字号 10.5：
/// 卡片右半区可用宽 = 卡片宽 334 - 预览内缩 10 - 预览宽 188 - 间距 8 = 128px，
/// 参考截图里 `Choose a Terrain Pattern` 也就压到约 110px 宽。
pub const AA_LINE_FONT: f32 = 10.5;

/// 右侧三行文字相对卡片顶的纵向位置 22 / 44 / 68。
pub const AA_LINE_CAPTION_Y: f32 = 22.0;
pub const AA_LINE_LABEL_Y: f32 = 44.0;
pub const AA_LINE_PIN_Y: f32 = 68.0;

/// `收藏` 行星形半径 4.5；星心 → 文字左边缘 12。
pub const AA_STAR_R: f32 = 4.5;
pub const AA_STAR_TEXT_OFF: f32 = 12.0;

// ───────────────────────── 标题栏 / 菜单栏 ─────────────────────────

/// 标题栏应用图标：边长 15，距内容区左边 8（截图 x=16..30，内容区原点 8）。
pub const TITLE_ICON_X: f32 = 8.0;
pub const TITLE_ICON_SIZE: f32 = 15.0;

/// 标题栏文字起点距内容区左边 29（截图标题文字起 x=37，37 - 8 = 29）。
pub const TITLE_TEXT_X: f32 = 29.0;

/// 标题栏字号 13：截图标题文字高约 10px（y=8..18），按 13px 比例字号还原。
/// （JSON `theme.font_size` = 12 是全局正文；标题栏未单独给出，取与菜单栏同档 13。）
pub const TITLE_FONT: f32 = 13.0;

/// 标题栏右侧 3 个窗口按钮：每个 46 宽 × 31 高（= 标题栏高），紧贴右边缘排布。
/// 来源：截图字形中心 x=1377.5 / 1422.5 / 1468.5（间距 45~46），
/// 最右按钮右边 = 内容区右边 1491 ⇒ 1491 - 46 = 1445，依次左推。
pub const CAPTION_BTN_W: f32 = 46.0;

/// 三个窗口按钮字形（`—` / `□` / `×`）的字号 13。
pub const CAPTION_FONT: f32 = 13.0;

/// 菜单栏左内边距 18。来源：截图首个菜单项 `File` 文字起 x=38，
/// 项内 padding 12 ⇒ 项左边 26 ⇒ 26 - 内容区原点 8 = 18；
/// 与 JSON `theme.metrics.titlebar_margin_x` = 18 一致。
pub const MENU_PAD_X: f32 = 18.0;

/// 菜单项内左右 padding 12。来源：截图相邻菜单项文字间距恒为 25~26（`File` 末 55 → `Edit` 起 80）。
pub const MENU_ITEM_PAD_X: f32 = 12.0;

/// 菜单栏字号 13。来源：截图菜单文字高约 9~10px（y=46..55），按 13px 比例字号还原。
pub const MENU_FONT: f32 = 13.0;

/// 正文/标签字号 12。来源：JSON `theme.font_size` = 12；
/// 截图层列表行文字高约 9px、行高 26，12px 字号与之相符。
pub const BODY_FONT: f32 = 12.0;

/// 状态栏字号 12（与正文同档；截图状态栏文字高约 9px）。
pub const STATUS_FONT: f32 = 12.0;

/// 文字裁剪盒的呼吸余量 2。
///
/// 有些标签的裁剪盒是**按测量出的文字宽**给出的（Palette 页签、状态栏右对齐文字），
/// 盒子宽度恰好等于排版宽度 —— 一旦字形右边缘带抗锯齿溢出半像素，
/// 就会被裁掉一刀，"恰好放下"和"被切掉"在像素层面分不清。
/// 统一外扩 2px：既不影响布局（页签间距 10、状态栏右侧还有 52px 空隙），
/// 又让"文字像素右边界 vs 裁剪盒右边界"的核对有可判读的余量。
pub const TEXT_CLIP_SLACK: f32 = 2.0;

// ───────────────────────── Palette 面板内部纵向偏移 ─────────────────────────
//
// 全部「相对 Palette 内容区顶边」计量。参考截图里 Palette 内容区顶边 = y285。

/// 页签行：偏移 0，高 `PALETTE_TABS_H`(30)。截图页签文字 y=294..306，行区 y=285..315。
pub const PALETTE_TABS_OFF: f32 = 0.0;

/// 页签之间水平间距 10。来源：截图 `Library` 末 x=1190 → `Favourites` 起 x=1200。
pub const PALETTE_TAB_GAP: f32 = 10.0;

/// `Terrain Patterns` / `0 sets` 标签行：偏移 45。来源：截图文字 y=330..340（285 + 45 = 330）。
pub const PALETTE_LABEL_OFF: f32 = 45.0;

/// 组合框：偏移 67。来源：截图组合框 (1146,352)-(1482,383)（285 + 67 = 352）。
pub const PALETTE_COMBO_OFF: f32 = 67.0;

/// 搜索框：偏移 111。来源：截图搜索框 (1146,396)-(1482,424)（285 + 111 = 396）。
pub const PALETTE_SEARCH_OFF: f32 = 111.0;

/// 空态网格图标：偏移 203、尺寸 20×20。来源：截图图标区 (1308,488)-(1330,508)。
pub const PALETTE_EMPTY_ICON_OFF: f32 = 203.0;
pub const EMPTY_ICON_SIZE: f32 = 22.0;

/// 空态标题 `Your Assets Live Here`：偏移 248。来源：截图文字 y=533..543。
pub const PALETTE_EMPTY_TITLE_OFF: f32 = 248.0;

/// 空态说明两行：偏移 268。来源：截图 `Choose your Crystal library folder once.` y≈553。
pub const PALETTE_EMPTY_DESC_OFF: f32 = 268.0;

/// 空态主按钮 `Set Library Folder`：偏移 319。来源：截图金色按钮 y=604..632（285 + 319 = 604）。
pub const PALETTE_BTN_OFF: f32 = 319.0;

/// 分页行距面板内容区底边的内边距 10。来源：截图分页行 y=695..717，面板内容区底 y=747。
pub const PALETTE_BOTTOM_PAD: f32 = 10.0;

/// 空态说明文字的行高 16（两行 553/569）。
pub const EMPTY_DESC_LINE_H: f32 = 16.0;

// ───────────────────────── Palette 面板：图块网格（本单新增） ─────────────────────────
//
// 这一段全部是**推导值**（参考截图的资源面板是空态，没有网格可量），
// 每条都写明由哪个既有常量推出来 —— 没有一条是"看着差不多"。

/// 网格顶部相对 Palette 内容区上边 **150**。
///
/// 推导：搜索框底 = `PALETTE_SEARCH_OFF`(111) + `SEARCH_H(29)` = 140，再留 10 的呼吸。
pub const PALETTE_GRID_OFF: f32 = 150.0;

/// 每行 **8** 格。
///
/// 推导：内容区可用宽 = `RIGHT_DOCK_W(356) - 2 × RIGHT_PAD_X(10)` = **336**，
/// 336 / 8 = **42** 整除 ⇒ 8 列刚好铺满、不留半格。
pub const PALETTE_GRID_COLS: usize = 8;

/// 单格边长 **42**（= 336 / 8，见上一条）。格子是正方形，缩略图等比缩进格内。
pub const PALETTE_CELL: f32 = 42.0;

/// 缩略图相对格子四周的内缩 **2**：留出 1px 格线 + 1px 呼吸，
/// 于是缩略图可用区 = 42 - 2×2 = 38×38（96×64 的地表帧按 1.5:1 缩成 38×25）。
pub const PALETTE_THUMB_PAD: f32 = 2.0;

/// 网格底与分页行之间留 **6**（分页行本身 22 高、再往下 `PALETTE_BOTTOM_PAD` 10）。
///
/// 由此得可见高度 = 465（内容高）- 150 - 22 - 10 - 6 = **277**，
/// 可见行数 = 277 / 42 = **6 行**（252 px，余 25 px 不铺第 7 行——半行会看起来像渲染错位）。
pub const PALETTE_GRID_GAP: f32 = 6.0;

/// 库下拉弹层每行高 **20**（比格子里的小字行高 16 略宽，便于点）。
pub const PALETTE_COMBO_ROW_H: f32 = 20.0;

/// 库下拉弹层最多显示 **8** 行（= 160 px，向下不越过分页行）；更多的库靠滚轮翻。
pub const PALETTE_COMBO_ROWS: usize = 8;

/// 素材面板里"选中格"的金色描边宽 **2**（与画布悬停格 `CELL_HOVER_STROKE` 同档）。
pub const PALETTE_SELECT_STROKE: f32 = 2.0;

/// 网格与分页行之间那条**提示行**的高度 **16**。
///
/// 它的用途：搜索失败/拒绝设笔刷/空帧计数这些"必须说出来"的话要有地方显示，
/// 又不能压在缩略图上。16px 是 `PALETTE_LABEL_FONT(11)` 的一行高度（含 3px 呼吸）。
pub const PALETTE_NOTE_H: f32 = 16.0;

// ───────────────────────── Palette 面板：「分类」页签 = 印章网格（本单新增） ─────────────────────────
//
// 纵向三段的偏移（页签 / 标签行 / 下拉 / 搜索 / 网格顶 / 提示行 / 分页行）**完全复用**
// 上面 Palette 的那一套（`PALETTE_*_OFF`）：两页签共用同一个面板骨架，只有"格子怎么排"不同。
// 下面只定义**格子本身**与**每页条数**，每条都写明由哪个既有常量推出来。

/// 印章网格每行 **4** 枚。
///
/// 推导：内容区可用宽 = `RIGHT_DOCK_W(356) - 2 × RIGHT_PAD_X(10)` = **336**，336 / 4 = **84** 整除
/// ⇒ 4 列刚好铺满。取 4 而不是资源库页的 8：印章缩略图是 **46×32**（任务书 §三.2 指定）
/// 再加下方一行名称，8 列时每列只剩 42 px、名称会挤成两三个字，失去辨识度。
pub const STAMP_GRID_COLS: usize = 4;

/// 单格宽 **84**（= 336 / 4，见上一条）。
pub const STAMP_CELL_W: f32 = 84.0;

/// 缩略图宽 **46**（任务书 §三.2 指定；`miredit_render::StampRenderOptions::thumbnail(46, 32)`）。
pub const STAMP_THUMB_W: f32 = 46.0;

/// 缩略图高 **32**（同上）。46×32 与一格地表帧 48×32 同比例，缩下来不糊。
pub const STAMP_THUMB_H: f32 = 32.0;

/// 缩略图相对格子四周的内缩 **3**：留 1px 格线 + 2px 呼吸（与 `PALETTE_THUMB_PAD` 同思路，
/// 但格子更矮，2 会显得贴边）。
pub const STAMP_CELL_PAD: f32 = 3.0;

/// 名称行与缩略图之间的间隙 **2**。
pub const STAMP_NAME_GAP: f32 = 2.0;

/// 名称行字号 **11**（= `PALETTE_LABEL_FONT`，与资源库页的小字同档，不另立字号）。
pub const STAMP_NAME_FONT: f32 = 11.0;

/// 名称行行高 **13**（11px 字号 + 2px 呼吸；比 `PALETTE_NOTE_H(16)` 紧，因为格子里只有一行）。
pub const STAMP_NAME_H: f32 = 13.0;

/// 单格高 **53** = 上内缩 3 + 缩略图 32 + 间隙 2 + 名称行 13 + 下内缩 3。
///
/// 由此得可见行数 = 网格高 277 / 53 = **5 行**（265 px，余 12 px 不铺第 6 行 ——
/// 与资源库页"半行看起来像渲染错位"同一条纪律，见 `PALETTE_GRID_GAP` 的注释）。
pub const STAMP_CELL_H: f32 = 53.0;

/// 印章页签**每页 96 枚**。
///
/// 推导：`STAMP_GRID_COLS(4) × 24 行 = 96` —— 整行整列，一页正好 24 行。
/// 一屏只能看到 5 行（20 枚），所以一页要滚 4.8 屏才看完；取 96 而不是"翻一屏"的理由：
/// ① 印章目录 9,159 枚，按屏翻会翻出 458 页，"第 N/M 页"失去定位意义；
/// ② 96 与资源库页签的"每屏 48 格"成整倍数关系（2 屏），两个页签的翻页手感一致。
///
/// ⚠ 与资源库页的**分页按钮语义不同**：那边是"上下翻一屏"，这边是"上/下一页"（整页 96）。
pub const STAMP_PAGE_SIZE: usize = 96;

/// 印章缩略图纹理缓存上限 **256** 张。
///
/// 取值依据：一张 46×32 RGBA = 5.9 KB ⇒ 256 张 ≈ 1.5 MB 显存 + 纹理对象开销。
/// 一页 96 枚、一屏 20 枚 ⇒ 256 覆盖 **2.6 页**的滚动历史；
/// 而全库 9,159 枚若全上传要 54 MB（本单明确禁止，见任务书 §三.3）。
pub const STAMP_THUMB_CACHE_CAP: usize = 256;

/// 印章分类下拉弹层最多显示 **8** 行（= 160 px，与 `PALETTE_COMBO_ROWS` 同档；
/// 21 个选项靠滚轮翻）。
pub const STAMP_COMBO_ROWS: usize = 8;

/// 印章信息两行（「当前资源」面板）的字号 **10.5**（= `AA_LINE_FONT`，不另立字号）。
///
/// ⚠ 该面板文字区只有 **128 px** 宽（`AA_CARD_W(334) - AA_PREVIEW_OFF(10) - AA_PREVIEW_W(188)
/// - AA_TEXT_GAP(8)`），而印章信息必须容下 `25 格 · 包围盒 (-2,-1,3,4)` 这一串；
/// 因此**拆成两行**（名称+分类 / 格数+包围盒），并由
/// `stamp_panel_tests::印章信息两行都放得进文字区` 用真实字体量宽把关。
pub const STAMP_AA_FONT: f32 = 10.5;

// ───────────────────── 画布叠加层：矩形选区框 / 印章轮廓（2026-11-05 新增） ─────────────────────
//
// ⚠ 这一组**没有截图真值**：官方参考截图里既没有选区也没有印章轮廓
// （`crystalstudio-main-runtime-1500x950.png` 是空画布态）。所以**线宽 / 虚线节拍**这些数
// 都是**推导值**，推导依据写在每一条上；将来若拿到带选区/轮廓的官方截图，按截图改这里即可，
// **不许**在绘制代码里就地写字面量。
// 唯一例外是**填充色**：它有官方源码真值（见下方 `selection_fill` 那条），不是推导值。

/// 选区虚线的线宽 **1.5**。
///
/// 推导：与既有 `CELL_HOVER_STROKE(2.0)` 同档偏细 —— 悬停框是"当前格"（要显眼），
/// 选区框是"一块区域"（要能看清框内的地图），1.5 在 100% 缩放下正好落在 1~2 物理像素之间，
/// 既不会像 1px 那样被抗锯齿吃掉，也不会像 2px 那样盖住格子边。
pub const SEL_STROKE_W: f32 = 1.5;

/// 选区虚线的**实线段长** **6**。
///
/// 推导：一格是 48×32（`CELL_W`/`CELL_H`）。6 + 4 = 10 一段 ⇒ 横向一格约 4.8 段、
/// 纵向约 3.2 段，肉眼一眼能数出"这是虚线"；取 8 以上在 32px 高的格子上只剩 2 段，
/// 看着更像实线。
pub const SEL_DASH_LEN: f32 = 6.0;

/// 选区虚线的**空档长** **4**（理由同上：6 + 4 = 10 一段）。
pub const SEL_GAP_LEN: f32 = 4.0;

// 选区半透明填充的颜色**不在本文件**：真值在 `theme::Palette::selection_fill`。
//
// 官方 `CrystalStudio/MapViewport.cs:295` 用的是**固定蓝**
// `Color.FromArgb(30, 110, 150, 255)`（α = 30），本仓按预乘落成 `rgba_pm(12, 17, 30, 30)`
// —— 与主题（浅色 / 深色）**无关**。此处原 `SEL_FILL_ALPHA = 40` 及其
// "颜色本体取强调金 `#CC9629`" 的注释都是误记（金 × α40 是自造值，`theme.rs` 已订正为官方蓝），
// 故整条删除，避免有人再照这个 α 去乘一次。

/// 印章轮廓框的线宽 **2.0**（= `CELL_HOVER_STROKE`）。
///
/// 推导：轮廓是**只有边框、不画任何像素**的预览（任务书 §三.5 明令"不许画假像素"），
/// 所以它必须比选区虚线更粗一点，才能在"没有填充色"的情况下被认出来。
pub const STAMP_PREVIEW_STROKE_W: f32 = 2.0;

/// 画布左上角「待放置」徽标的高度 **20**。
///
/// 推导：徽标里是 `STATUS_FONT(12)` 一行字，上下各留 4 px 呼吸 ⇒ 12 + 8 = 20。
/// 取 20 而不是更高的 24：它跟着鼠标走，太高会盖住落点附近的两行格。
pub const ARMED_BADGE_H: f32 = 20.0;

/// 徽标的左右内边距 **6**（= `PALETTE_TAB_INSET_L`，与面板小标签同档）。
pub const ARMED_BADGE_PAD_X: f32 = 6.0;

/// 徽标的圆角半径 **4**（与 `PALETTE_SELECT_STROKE` 同一档视觉重量）。
pub const ARMED_BADGE_RADIUS: f32 = 4.0;

// ───────────────────────── 告警黄条（2026-11-05 折行改造） ─────────────────────────
//
// ⚠ 黄条本体是宿主新增的控件（无官方截图依据，见 `banner` 模块文档 §三）。
// 上一版是**单行 34px 固定高**；2026-11-05 起 `Shanda2012` 的 `editable_caveat` 约 250 字
// ⇒ 必须折行、高度自适应，否则不是横向溢出就是把关键词截掉。

/// 告警黄条的**单行**高度 **18**（12px 字 + 上下各 3 呼吸）。
///
/// 与上一版的 `BANNER_H = 34` 的关系：`34 = 2 × 8（上下内边距）+ 18 × 1 行`
/// ⇒ 单行文案的几何与上一版**逐像素相同**（既有截图与用例都不受影响）。
pub const BANNER_LINE_H: f32 = 18.0;

/// 告警黄条的上下内边距 **8**。
pub const BANNER_PAD_Y: f32 = 8.0;

/// 告警黄条允许占用的最大画布高度比例 **0.45**。
///
/// 为什么是 45%：告警要"看得见但不至于把地图全遮住"。
/// 实测 250 字的 caveat 在 1500×950 下折 4 行 = 88px（占画布 815 的 10.8%），
/// 在 1100×720 下折 6 行 = 124px（占 600 的 20.7%）⇒ **正常情况远不到上限**，
/// 只有"画布被压成一条"时才会触发（那时才允许在最后一行加省略号）。
pub const BANNER_MAX_VIEWPORT_RATIO: f32 = 0.45;

// ───────────────────────── 菜单下拉弹层（2026-11-05 新增） ─────────────────────────
//
// ⚠ **这一组没有官方截图真值**：参考截图（`crystalstudio-main-runtime-1500x950.png` /
// `crystalstudio-new-zh.png`）都是**菜单收起**态，官方也没给过展开态的图。
// 所以数值一律**按官方 Avalonia Fluent 菜单的度量推导**，推导链写在这里（不是"看着差不多"）：
//
// * 行高：Fluent 的 `MenuItem` 最小高是 32（字号 14）⇒ 本仓菜单字号 13（`MENU_FONT`，
//   截图实测）⇒ 等比 32 × 13/14 ≈ 29.7，再按"13px 字 + 上下各 5.5 呼吸"取 **24**；
//   取 24 而不是 30 的理由：视图菜单有 **18 项**，30 会让弹层高 540px（超出 950 窗口的可用高度）。
// * 左右内边距 12 = `MENU_ITEM_PAD_X`（与菜单栏同一档，弹层文字与菜单栏文字左对齐观感一致）。
// * 分隔线占 **9px**（1px 线 + 上下各 4px），Fluent 的 `Separator` 同款做法。
// * 勾选列 **18px**：没有勾选的项也占这一列，保证所有项文字左对齐（不因有无勾选而参差）。

/// 菜单弹层的行高 **24**（推导见本节注释）。
pub const MENU_POPUP_ROW_H: f32 = 24.0;

/// 菜单弹层的上下内边距 **4**。
pub const MENU_POPUP_PAD_Y: f32 = 4.0;

/// 菜单弹层的左右内边距 **12**（= `MENU_ITEM_PAD_X`）。
pub const MENU_POPUP_PAD_X: f32 = 12.0;

/// 菜单弹层里一条分隔线占的高度 **9**（1px 线 + 上下各 4）。
pub const MENU_POPUP_SEP_H: f32 = 9.0;

/// 菜单弹层的最小宽度 **168**（放得下最长的单字项「实际大小（100%）(A)」之外的余量由测量决定）。
pub const MENU_POPUP_MIN_W: f32 = 168.0;

/// 菜单项文字与右侧快捷键之间的**最小**间距 **28**（不足则弹层加宽）。
pub const MENU_POPUP_SHORTCUT_GAP: f32 = 28.0;

/// 勾选列宽 **18**（无勾选也占位，保证文字左对齐）。
pub const MENU_POPUP_CHECK_W: f32 = 18.0;

/// 菜单弹层的 1px 边框宽。
pub const MENU_POPUP_BORDER: f32 = 1.0;

/// 菜单弹层字号 **12**（= `BODY_FONT`；比菜单栏 13 小一档，弹层才不显笨）。
pub const MENU_POPUP_FONT: f32 = 12.0;

/// 菜单弹层圆角 **4**（与 `ARMED_BADGE_RADIUS` 同档）。
pub const MENU_POPUP_RADIUS: f32 = 4.0;

// ───────────────────── 键盘快捷键浮层 / 打开地图模态（2026-11-05 新增） ─────────────────────

/// 快捷键浮层宽 **560**：一列（快捷键 170 + 说明），一行一条绑定。
pub const SHORTCUTS_W: f32 = 560.0;

/// 快捷键浮层高 **620**：标题 26 + 说明 18 + 6 组 × 若干行（共 28 行 × 18）+ 上下内边距 36
/// + 组间 6 × 4 = **614**，留 6px 余量。
///
/// ⚠ 2026-11-05 由 560 调到 600：`视图 → 单元格网格` 真实现后，`G` 进了快捷键表
/// （新增「视图」组 1 行 ⇒ +2 行 = +36px）。原 560 只够 25 行（556px），
/// 加了就放不下 —— `menu::shortcuts::tests::表放得进浮层` 会当场红，所以这里是**跟着表算的**，
/// 不是"看着差不多放大一点"。
///
/// ⚠ 同日再 600 → 620：「文件 → 另存为」真实现后 `Ctrl+Shift+S` 进了表（文件组 +1 行 = +18px），
/// 600 只剩 4px 余量（596），加一行就是 614 > 600 ⇒ 同样**跟着表算**。
pub const SHORTCUTS_H: f32 = 620.0;

/// 快捷键浮层的内边距 **18**。
pub const SHORTCUTS_PAD: f32 = 18.0;

/// 快捷键浮层行高 **18**（字号 12 + 上下各 3 呼吸）。
pub const SHORTCUTS_ROW_H: f32 = 18.0;

/// 快捷键浮层的标题行高 **26**。
pub const SHORTCUTS_TITLE_H: f32 = 26.0;

/// 快捷键浮层的说明行高 **18**。
pub const SHORTCUTS_NOTE_H: f32 = 18.0;

/// 快捷键浮层的「键」列宽 **170**（右侧留给说明文字）。
pub const SHORTCUTS_KEY_COL_W: f32 = 170.0;

/// 打开地图模态的宽 **640**（路径输入框要放得下 80 个字符的路径）。
pub const OPEN_DLG_W: f32 = 640.0;

/// 打开地图模态的高 **160**（标题 + 输入框 + 提示 + 两个按钮）。
pub const OPEN_DLG_H: f32 = 160.0;

/// 打开地图模态的内边距 **16**。
pub const OPEN_DLG_PAD: f32 = 16.0;

/// 打开地图模态里路径输入框的高 **28**。
pub const OPEN_DLG_FIELD_H: f32 = 28.0;

/// 打开地图模态的按钮宽 **88** / 高 **26**。
pub const OPEN_DLG_BTN_W: f32 = 88.0;
pub const OPEN_DLG_BTN_H: f32 = 26.0;

#[cfg(test)]
mod tests {
    use super::*;

    /// 规范化只产出 3..=31 的奇数（与官方 `Clamp(value | 1, 3, 31)` 同集合）。
    #[test]
    fn 笔刷尺寸只产出三到三十一的奇数() {
        // 15 档：3/5/…/31
        let all: Vec<usize> = (BRUSH_MIN..=BRUSH_MAX).filter(|n| n % 2 == 1).collect();
        assert_eq!(all.len(), 15, "3..=31 的奇数正好 15 档");
        assert_eq!(all.first(), Some(&3));
        assert_eq!(all.last(), Some(&31));
        for n in 0..=64 {
            let v = clamp_brush_size(n);
            assert!((BRUSH_MIN..=BRUSH_MAX).contains(&v), "{n} → {v} 越界");
            assert_eq!(v % 2, 1, "{n} → {v} 不是奇数");
        }
        // 边界：0/1/2 → 3；32/40/99 → 31；偶数抬成下一个奇数
        for n in [0, 1, 2] {
            assert_eq!(clamp_brush_size(n), 3, "{n} 应被夹到下限 3");
        }
        for n in [32, 33, 40, 99, usize::MAX] {
            assert_eq!(clamp_brush_size(n), 31, "{n} 应被夹到上限 31");
        }
        for n in [4, 6, 8, 30] {
            assert_eq!(clamp_brush_size(n), n + 1, "偶数 {n} 应抬成奇数");
        }
        for n in [3, 5, 7, 15, 29, 31] {
            assert_eq!(clamp_brush_size(n), n, "奇数 {n} 原样");
        }
    }

    /// 步进 2：`[` / `]` 一次走两格，且不会停在偶数上（官方 `TickFrequency=2`）。
    #[test]
    fn 步进为二() {
        assert_eq!(BRUSH_STEP, 2);
        assert_eq!(BRUSH_DEFAULT, 3);
        let mut n = BRUSH_DEFAULT;
        let mut seen = vec![n];
        while n < BRUSH_MAX {
            n = clamp_brush_size(n + BRUSH_STEP);
            seen.push(n);
        }
        assert_eq!(seen, vec![3, 5, 7, 9, 11, 13, 15, 17, 19, 21, 23, 25, 27, 29, 31]);
    }

    /// 填充上限就是任务书那个数（改它必须改测试）。
    #[test]
    fn 填充上限二十万格() {
        assert_eq!(FILL_MAX_CELLS, 200_000);
        // 700×700 的整图（490,000 格）必须被这个上限拦住。
        //
        // 两边都是常量 ⇒ 写成 `const { … }` 块，让它在**编译期**就断掉
        // （clippy `assertions_on_constants`：运行期断言常量是死代码，
        //  一旦有人把 `FILL_MAX_CELLS` 调到 490,000 以上，本 crate 直接编不过，
        //  比"跑测试才发现红"更早拦住）。
        const { assert!(700 * 700 > FILL_MAX_CELLS) };
    }

    /// 印章网格的几何必须自洽：4 列整除可用宽、单格高由内缩/缩略图/名称行拼出来、
    /// 一页正好是整行（96 = 4 × 24）。
    ///
    /// 这条挡的是"改一个数忘了改另一个"——例如把缩略图高从 32 调到 40 却忘了 `STAMP_CELL_H`，
    /// 名称就会被压到下一格上（界面上看起来像串行）。
    ///
    /// ⚠ 全部比较都先落到局部变量：直接 `assert!(常量表达式)` 会被 clippy 判成
    /// `assertions_on_constants`（"断言恒真"），而这里要的正是"常量之间必须恒真"。
    #[test]
    fn 印章网格几何自洽() {
        // 4 列整除 336（= RIGHT_DOCK_W - 2 × RIGHT_PAD_X）
        let usable = RIGHT_DOCK_W - 2.0 * RIGHT_PAD_X;
        assert_eq!(usable, 336.0, "可用宽变了，STAMP_GRID_COLS/STAMP_CELL_W 要重算");
        let grid_w = STAMP_CELL_W * STAMP_GRID_COLS as f32;
        assert_eq!(grid_w, usable);
        // 单格高 = 3 + 32 + 2 + 13 + 3
        let parts_h = STAMP_CELL_PAD * 2.0 + STAMP_THUMB_H + STAMP_NAME_GAP + STAMP_NAME_H;
        assert_eq!(STAMP_CELL_H, parts_h);
        // 缩略图必须放得进格子（宽高都要留出内缩）
        let fits_w = STAMP_THUMB_W + STAMP_CELL_PAD * 2.0 <= STAMP_CELL_W;
        let fits_h = STAMP_THUMB_H + STAMP_CELL_PAD * 2.0 <= STAMP_CELL_H;
        assert!(fits_w, "缩略图宽放不进格子");
        assert!(fits_h, "缩略图高放不进格子");
        // 一页 = 整行
        let rem = STAMP_PAGE_SIZE % STAMP_GRID_COLS;
        assert_eq!(rem, 0, "每页必须是整行数");
        let rows = STAMP_PAGE_SIZE / STAMP_GRID_COLS;
        assert_eq!(rows, 24, "一页 24 行");
        // 缓存上限必须能装下一整页（否则翻页时同一页内部就会自我淘汰）
        let cap_enough = STAMP_THUMB_CACHE_CAP >= STAMP_PAGE_SIZE;
        assert!(
            cap_enough,
            "缓存上限 {STAMP_THUMB_CACHE_CAP} 装不下一页 {STAMP_PAGE_SIZE}"
        );
    }
}
