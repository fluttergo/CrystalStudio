//! **单元格网格**（`视图 → 单元格网格`，官方手势 `G`）。
//!
//! # 一、画什么
//!
//! 世界格 **48×32**（[`crate::metrics::CELL_W`] / [`crate::metrics::CELL_H`] —— 与渲染库
//! `miredit_render::CELL_W/CELL_H` 同一个真值，见 `layout::tests::世界格与渲染库一致`），
//! 线画在**格边界**上（`x = k×48`、`y = k×32`），**不是格中心**：
//! 格中心画线会把每一格劈成两半，用户落笔时对不上"这一格到底是哪一格"。
//!
//! # 二、只画视口内可见范围，并**裁到地图边界**
//!
//! 可见格窗口由 [`CanvasView::visible_cells`] 给出（与重渲调度、小地图视口框**同一份判据**），
//! 本模块只枚举该窗口内的边界。700×700 的图有 701 + 701 条边界，
//! 而 1× 时视口只看得到约 21×26 格 ⇒ 每帧只画 40~50 条线，与地图多大无关。
//!
//! 枚举完再按**地图范围**夹两次（[`lines_in`] 的 `map` 参数），两处都照官方
//! `CrystalStudio/MapViewport.Presentation.cs:37-39@901dbe2f`（枚举窗口）与
//! `:58-61@901dbe2f`（线段端点）：
//!
//! 1. **夹枚举**：`Math.Max(0, …)` / `Math.Min(Document.Width/Height, …)`
//!    ⇒ 图外那些格边界根本不进列表；
//! 2. **夹线段长度**：官方 `DrawLine(pen, new(x*48, y0*32), new(x*48, y1*32))` 的两个端点
//!    也是夹过的下标 ⇒ 竖线只从地图上边界画到下边界（[`Lines::y_span`]），横线同理
//!    （[`Lines::x_span`]）。
//!
//! ⚠ **只做第 1 步是不够的**：2026-11-05 截图取证实测，只夹枚举时图外仍有 20361 px 差异
//! —— 每条竖线都横贯整块画布，图外露出的是画布底色（[`crate::theme`] 的 `canvas_bg`），
//! 在那儿画线等于告诉用户"图外还有格子"。
//!
//! ⚠ [`CanvasView::visible_cells`] 的语义**不许动**：它同时被
//! [`CanvasView::ensure`]（重渲调度）与小地图视口框共用，夹地图范围这件事只能在**本模块**
//! 做（`lines_in` 里那几次 `max/min`）。
//!
//! # 三、**没有"多小就不画"的阈值** —— 永远画，缩小时按 `step` 抽稀
//!
//! 上游 `901dbe2f`（2026-11-05 07:17）把 `MapViewport.Presentation.cs` 的网格整段换了：
//! **删掉了 `if (GridVisible && Zoom >= .4)` 的 0.4 门槛**，改成「永远画 + 抽稀」
//! （`:51-61@901dbe2f`）：
//!
//! ```text
//! if (GridVisible)
//! {
//!     // Keep grid lines visible above artwork at every zoom. Group cells
//!     // at overview scales so lines stay at least eight pixels apart.
//!     int step = 1;
//!     while (step * 32 * Zoom < 8) step *= 2;                      // :55-56
//!     var pen = new ScenePen(ThemeService.Brush("CanvasGrid"), 1 / Zoom);   // :57
//!     for (int x = (x0 + step - 1) / step * step; x <= x1; x += step)
//!         dc.DrawLine(pen, new(x * 48, y0 * 32), new(x * 48, y1 * 32));     // :58-59
//!     for (int y = (y0 + step - 1) / step * step; y <= y1; y += step)
//!         dc.DrawLine(pen, new(x0 * 48, y * 32), new(x1 * 48, y * 32));     // :60-61
//! }
//! ```
//!
//! 本仓照抄，判据三条：
//!
//! 1. **不再有"多小就不画"**：0.25×（本仓缩放区间下限）也照画。旧版 `MIN_SCALE = 0.4` /
//!    `visible_at()` 那套隐藏语义**整个删除** —— 它抄的那行代码已经被上游删了；
//! 2. **相邻两条线在屏幕上至少 8px**：按 `step × 32 × Zoom >= 8` 逐次翻倍。
//!    32 取**格高** [`crate::metrics::CELL_H`]（官方写死 32）：它是两个方向里更紧的一边，
//!    按它算出的 `step` 对竖线（一格 48 宽）只会更疏，绝不会违反"≥ 8px"；
//! 3. 线起点**对齐到 `step` 的整数倍**（`(i0 + step - 1) / step * step`，向上取整）——
//!    抽稀必须钉在**世界坐标**的固定相位上，否则一平移线就在屏幕上乱跳。
//!
//! | zoom | `32 × zoom` | `step` | 屏上横线间距 | 屏上竖线间距 |
//! |---|---|---|---|---|
//! | 0.5 | 16 | 1 | 16 | 24 |
//! | 0.25 | 8（**含等号**） | 1 | 8 | 12 |
//! | 0.2 | 6.4 | 2 | 12.8 | 19.2 |
//! | 0.1 | 3.2 | 4 | 12.8 | 19.2 |
//!
//! 判据只有 [`step_for`] 一个出口，绘制与单元测试共用，**不许各写一套**。
//!
//! # 四、笔宽 = `1/Zoom`（屏上恒 1px）与整像素对齐
//!
//! 官方 `:57@901dbe2f` 的笔宽是**世界单位** `1 / Zoom` ⇒ 乘回 `Zoom` 后**屏幕恒 1px**
//! （旧版是固定 0.6，那行已被 `901dbe2f` 删除）。本仓画的是屏幕空间矩形，
//! 故直接取 [`crate::metrics::GRID_LINE_W`] = **1 屏幕像素**，不随缩放变粗。
//! 每条线的屏幕坐标**四舍五入到整像素**：egui 的矩形是浮点，1px 的线若落在 x=100.5
//! 会被抗锯齿摊成两列半透明像素 —— 那既不是 1px，也会让"像素差只落在格线"的验收
//! 变成一片糊。
//!
//! # 五、默认**开**（照官方 `IsChecked="True"`）
//!
//! 官方 `CrystalStudio/MainWindow.axaml:5` 的「单元格网格」是
//! `ToggleType="CheckBox"` + `IsChecked="True"` ⇒ **默认勾选**。
//!
//! 本仓 2026-11-05 之前默认**关**，理由两条（都还成立，但已不足以压过"与官方形态一致"）：
//! ① 画布上贴的是真实地图位图，默认画线会让「与客户端同视点分块比对」（`--gate2-*`）
//!    凭空多出网格线色差；② 历次 `--shot` 取证截图与 `layout/refs/` 基线全是无网格态。
//!
//! 审查（内部报告《审查-菜单 39 项》缺陷 4，已移出公开仓）判定「默认不勾」是**形态偏离官方**，
//! 已改成默认开（判据在 [`crate::cli::Cli::cell_grid_enabled`]）。上面那两类场景改用
//! **显式关** `--no-cell-grid` 复现，不再靠"默认恰好是关"。
//!
//! ⇒ 打开/关闭走的是**同一个**开关（[`crate::app::MireditApp`] 的 `grid` 字段）：
//! 菜单 `视图 → 单元格网格`、快捷键 `G`、命令行 `--cell-grid` / `--no-cell-grid`。
//! 不存在"命令行能开、菜单开不了"（三条路径都落到 [`crate::menu::action::apply`] 或
//! 同一个字段上，见 `app::handle_keys` 与 `menu::action`）。

use std::cell::Cell;

use egui::{pos2, vec2, Painter, Rect};

use crate::canvas_view::CanvasView;
use crate::metrics as m;
use crate::paint as pt;
use crate::theme as t;
use crate::ui_text as tx;

/// 一组格线的**屏幕坐标**（已取整到整像素，升序）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lines {
    /// 竖线（世界格边界 `x = k×48`，`k` 是 [`Lines::step`] 的整数倍）。
    pub xs: Vec<f32>,
    /// 横线（世界格边界 `y = k×32`，`k` 是 [`Lines::step`] 的整数倍）。
    pub ys: Vec<f32>,
    /// 本帧的**抽稀步长** `step`（1 = 每格一条、2 = 隔一格一条…）。
    ///
    /// 由 [`step_for`] 按当前缩放算出（官方 `:55-56@901dbe2f`）。诊断行
    /// [`tx::DIAG_GRID`] 里那个「每 N 格一条」就是这个值 —— 验收靠它核对抽稀真的生效。
    pub step: u32,
    /// 竖线要画的**纵向屏幕范围** `(上, 下)` —— 已被地图夹过。
    ///
    /// 为什么线不能横贯整个画布：官方 `MapViewport.Presentation.cs:59@901dbe2f`
    /// `dc.DrawLine(pen, new(x * 48, y0 * 32), new(x * 48, y1 * 32))` 里
    /// **线段的两个端点也是夹过的格下标**（`y0 = Math.Max(0, …)`、`y1 = Math.Min(Height, …)`）。
    /// 只夹"枚举哪些格"而不夹"每条线画多长"，图外照样会拖出一条条整屏的线
    /// （2026-11-05 截图取证抓到的就是这个：图外 20361 px 差异）。
    pub y_span: (f32, f32),
    /// 横线要画的**横向屏幕范围** `(左, 右)` —— 已被地图夹过。
    pub x_span: (f32, f32),
}

impl Lines {
    /// 一共几条线（诊断/测试用）。
    pub fn len(&self) -> usize {
        self.xs.len() + self.ys.len()
    }

    /// 一条都没有（= 视口/地图尺寸为 0，或缩放是 NaN/0 —— **不是**"缩放太小"）。
    pub fn is_empty(&self) -> bool {
        self.xs.is_empty() && self.ys.is_empty()
    }
}

/// 抽稀步长 `step` —— **唯一判据**（绘制与测试共用）。
///
/// 官方 `MapViewport.Presentation.cs:55-56@901dbe2f`：
/// `int step = 1; while (step * 32 * Zoom < 8) step *= 2;`
/// —— 屏幕上相邻两条线**至少 8px**。
///
/// ⚠ **边界是含等号的**：比较用的是 `< 8`（不是 `<= 8`）⇒ 间距**恰好 8px 时不抽稀**。
/// `32 × 0.25 = 8` ⇒ 0.25× 的 `step` **仍是 1**（用例 `抽稀步长四档` 钉住这一档）。
/// 谁把它改成 `<=`，0.25× 就会白丢一半的线，而且看起来"还画着"、极难发现。
///
/// 32 取**格高** [`crate::metrics::CELL_H`]（官方就写死 32）：格宽 48 比它大，
/// 按更紧的格高算出来的 `step` 对竖线只会更疏 ⇒ 两个方向都满足"≥ 8px"。
///
/// `zoom` 非有限 / ≤ 0 时返回 1（不翻倍、不死循环）：这类缩放本来就画不出线
/// （[`CanvasView::visible_cells`] 会给出 0 格），这里只保证**不会卡住**。
pub fn step_for(zoom: f32) -> u32 {
    if !(zoom.is_finite() && zoom > 0.0) {
        return 1;
    }
    let mut step = 1u32;
    // 上限 1<<16 是**防御**（真实缩放区间 0.25..=4 只翻到 1）：缩放到 1e-30 这种
    // 非物理值时，`step * 32 * zoom` 永远 < 8，没有上限就会一直翻倍到 u32 溢出。
    while (step as f32) * m::CELL_H * zoom < 8.0 && step < (1 << 16) {
        step *= 2;
    }
    step
}

/// 一条世界格边界落在屏幕上的坐标（**取整到整像素**，见模块文档 §四）。
fn line_pos(origin: f32, pan: f32, scale: f32, world: f32) -> f32 {
    (origin + (world - pan) * scale).round()
}

/// 把世界格下标向上取整到 `step` 的整数倍（官方 `:58`/`:60@901dbe2f`
/// 的 `(x0 + step - 1) / step * step`）。
///
/// 用 `div_euclid` 而不是 `/`：兼容档（[`lines`]，不知道地图多大）允许下标为负，
/// C# 那边 `x0` 已先被 `Math.Max(0, …)` 夹成非负所以直接整除就对；这里换成
/// 欧几里得除法后对负数同样是"向上取整到倍数"，语义一致且不产生 -0 号怪值。
fn ceil_to_step(i: i32, step: i32) -> i32 {
    (i + step - 1).div_euclid(step) * step
}

/// 算出当前视口要画的那组线（**纯几何**，不碰 GPU / 字体，可在无窗口测试里逐条核对）。
///
/// 返回的坐标已经落在 `canvas` 内（含左/上边界，不含右/下边界）——
/// 最后一条边界正好贴在 `canvas.right()` 时，它的 1px 会整根落在裁剪盒外面
/// （画了也看不见），所以这里先筛掉，让 [`Lines::len`] 报出来的数就是**真画上去的条数**。
///
/// `map` = 地图尺寸 `(宽格数, 高格数)`；`None` 表示**不知道地图多大** ⇒ 不夹范围
/// （兼容入口，[`draw`] 走的也是这条）。真机由 [`crate::canvas`] 传
/// `Some((map_w, map_h))`，见 [`lines_in`]。
pub fn lines(view: &CanvasView, canvas: Rect) -> Lines {
    lines_in(view, canvas, None)
}

/// [`lines`] 的**带地图范围**版本：只画 `[0, map_w] × [0, map_h]` 内的格边界。
///
/// 夹范围这一步照官方 `CrystalStudio/MapViewport.Presentation.cs:37-39@901dbe2f`：
///
/// ```text
/// int x0 = Math.Max(0, (int)Math.Floor(-origin.X / Zoom / 48));   // 左边界夹到 0
/// int x1 = Math.Min(Document.Width,  (int)((ActualWidth  - origin.X) / Zoom / 48) + 1);
/// int y1 = Math.Min(Document.Height, (int)((ActualHeight - origin.Y) / Zoom / 32) + 1);
/// ```
///
/// ⇒ 图外**一条线都不画**；地图右下角那条边界线**要画**（官方 `for x = x0; x <= x1`）。
/// 视口整块在地图外时（比如把图拖出屏幕）返回空集，而不是画一片空白格。
///
/// **抽稀**（模块文档 §三）在夹完范围之后做：起点先按 `ceil_to_step` 对齐到 `step` 的倍数，
/// 再 `+= step` 前进（官方 `:58`/`:60@901dbe2f` 的循环头）。
///
/// 三档语义（**不许混**）：
/// * `None` —— 不知道地图多大 ⇒ 不夹（老行为，兼容档）；
/// * `Some((0, _))` / `Some((_, 0))` —— **没有地图** ⇒ 一条线都不画
///   （`crate::canvas` 在无图时就是填 `0`；绝不能退化成"不夹"去画一片空图）；
/// * `Some((w, h))` 且都 > 0 —— 按地图夹。
pub fn lines_in(view: &CanvasView, canvas: Rect, map: Option<(u16, u16)>) -> Lines {
    // 数值兜底（**不是**可见性阈值）：NaN / 0 / 负数缩放画不出有意义的线。
    // 上游 `901dbe2f` 删掉的是"缩放太小就不画"，不是这条 NaN 防护。
    if !view.scale.is_finite()
        || view.scale <= 0.0
        || canvas.width() < 1.0
        || canvas.height() < 1.0
    {
        return Lines::default();
    }
    let (x0, y0, w, h) = view.visible_cells(canvas);
    if w == 0 || h == 0 {
        return Lines::default();
    }
    // 世界格下标窗口（闭区间 `[i0, i1]`）：先取可见窗口，再按地图夹一次
    let (mut i0, mut i1, mut j0, mut j1) = (x0, x0 + w as i32, y0, y0 + h as i32);
    if let Some((mw, mh)) = map {
        if mw == 0 || mh == 0 {
            // 没有地图：没有格子可画（**不许**退化成"不夹"）
            return Lines::default();
        }
        i0 = i0.max(0);
        j0 = j0.max(0);
        i1 = i1.min(mw as i32);
        j1 = j1.min(mh as i32);
        if i1 < i0 || j1 < j0 {
            // 视口整块落在图外（或图比一格还小）：一格线都不该有
            return Lines::default();
        }
    }
    // 抽稀步长（官方 `:55-56@901dbe2f`）：屏幕上相邻两条线至少 8px
    let step = step_for(view.scale) as i32;
    let si0 = ceil_to_step(i0, step);
    let sj0 = ceil_to_step(j0, step);
    let inside_x = |x: f32| x + m::GRID_LINE_W > canvas.left() && x < canvas.right();
    let inside_y = |y: f32| y + m::GRID_LINE_W > canvas.top() && y < canvas.bottom();

    let mut xs = Vec::with_capacity(((i1 - si0).max(0) / step + 2) as usize);
    let mut i = si0;
    while i <= i1 {
        let x = line_pos(
            canvas.left(),
            view.pan_x,
            view.scale,
            i as f32 * m::CELL_W,
        );
        if inside_x(x) {
            xs.push(x);
        }
        i += step;
    }
    let mut ys = Vec::with_capacity(((j1 - sj0).max(0) / step + 2) as usize);
    let mut j = sj0;
    while j <= j1 {
        let y = line_pos(
            canvas.top(),
            view.pan_y,
            view.scale,
            j as f32 * m::CELL_H,
        );
        if inside_y(y) {
            ys.push(y);
        }
        j += step;
    }
    // 线段端点也要夹（官方 `:59`/`:61@901dbe2f` 的 `DrawLine` 端点就是夹过的格下标，
    // 而且取的是**夹过的窗口端点** `y0/y1`、`x0/x1`，不是抽稀后的起点 —— 别顺手改）。
    let y_span = (
        line_pos(canvas.top(), view.pan_y, view.scale, j0 as f32 * m::CELL_H)
            .max(canvas.top()),
        line_pos(canvas.top(), view.pan_y, view.scale, j1 as f32 * m::CELL_H)
            .min(canvas.bottom()),
    );
    let x_span = (
        line_pos(canvas.left(), view.pan_x, view.scale, i0 as f32 * m::CELL_W)
            .max(canvas.left()),
        line_pos(canvas.left(), view.pan_x, view.scale, i1 as f32 * m::CELL_W)
            .min(canvas.right()),
    );
    Lines {
        xs,
        ys,
        step: step as u32,
        y_span,
        x_span,
    }
}

/// 画网格（调用方传进来的 painter 必须**已经裁剪到画布矩形**）。
///
/// 颜色走 [`crate::theme::ui`]（浅/深两套板各有一个 `grid_line`），
/// 线宽走 [`crate::metrics`]，抽稀走本模块 [`step_for`]，文案走 [`crate::ui_text`]
/// —— 本函数里没有字面量。
///
/// ⚠ 这是**不知道地图多大**的兼容入口（不夹地图范围）。真机走 [`draw_in`]。
pub fn draw(p: &Painter, canvas: Rect, view: &CanvasView) {
    draw_in(p, canvas, view, None);
}

/// [`draw`] 的**带地图范围**版本：图外一条线都不画（官方 `Presentation.cs:37-39@901dbe2f`）。
///
/// `map = Some((w, h))` 按地图夹范围；`Some((0, _))` / `Some((_, 0))` = **没有地图**
/// ⇒ 一条线都不画；`None` = 不知道地图多大 ⇒ 不夹（老行为）。三档语义见 [`lines_in`]。
/// 真机调用点是 [`crate::canvas::draw`]，尺寸来自 `MapDoc::map` 的 `width/height`。
pub fn draw_in(p: &Painter, canvas: Rect, view: &CanvasView, map: Option<(u16, u16)>) {
    let ls = lines_in(view, canvas, map);
    if ls.is_empty() {
        return;
    }
    let color = t::ui().grid_line;
    // 笔宽 = 官方世界单位 `1 / Zoom` × `Zoom` ⇒ **屏幕恒 1px**（见模块文档 §四）
    let w = m::GRID_LINE_W;
    // 线段长度取 `Lines` 里的 `y_span` / `x_span`（已被地图夹过），不是整个画布
    let (sy0, sy1) = ls.y_span;
    let (sx0, sx1) = ls.x_span;
    for x in &ls.xs {
        pt::fill(
            p,
            Rect::from_min_size(pos2(*x, sy0), vec2(w, (sy1 - sy0).max(0.0))),
            color,
        );
    }
    for y in &ls.ys {
        pt::fill(
            p,
            Rect::from_min_size(pos2(sx0, *y), vec2((sx1 - sx0).max(0.0), w)),
            color,
        );
    }
    report_once(&ls, canvas, view);
}

thread_local! {
    /// 本次运行是否已经打过那行网格诊断（**只打一次**：`draw` 每帧都跑，每帧打就是刷屏）。
    static REPORTED: Cell<bool> = const { Cell::new(false) };
}

/// 拼那行「首线坐标 + 步长 + 抽稀 + 视口/平移/缩放」诊断（**纯函数**，测试直接调它核对文案）。
///
/// 为什么需要它：网格是压在地图位图上的 1px 线，在截图里与地图自身的深色像素混在一起，
/// 光看 PNG 很难判"这条线是不是正好落在格边界上"。这行把**真实算出来的**几何一次说清，
/// 验收脚本拿它就能把 PNG 里每个差异像素反查回世界格边界（`x = 首线x + 48·step·k`）。
///
/// 返回空串 = 这一帧一条线都没有（没东西可说）。
fn diag_line(ls: &Lines, canvas: Rect, view: &CanvasView) -> String {
    let (Some(x1), Some(y1)) = (ls.xs.first(), ls.ys.first()) else {
        return String::new();
    };
    // 步长报**屏上真实间距**（格宽/格高 × 缩放 × 抽稀），不是单格间距 ——
    // 抽稀后若还报 48×zoom，验收就会拿它去反查一个不存在的格线。
    let dx = m::CELL_W * view.scale * ls.step as f32;
    let dy = m::CELL_H * view.scale * ls.step as f32;
    tx::fill(
        tx::DIAG_GRID,
        &[
            ("nv", &ls.xs.len().to_string()),
            ("nh", &ls.ys.len().to_string()),
            ("x1", &num(*x1)),
            ("dx", &num(dx)),
            ("y1", &num(*y1)),
            ("dy", &num(dy)),
            ("step", &ls.step.to_string()),
            ("left", &num(canvas.left())),
            ("top", &num(canvas.top())),
            ("px", &num(view.pan_x)),
            ("py", &num(view.pan_y)),
            ("scale", &num(view.scale)),
        ],
    )
}

/// 第一次真正画出网格时打一行诊断（内容见 [`diag_line`]）。
fn report_once(ls: &Lines, canvas: Rect, view: &CanvasView) {
    REPORTED.with(|c| {
        if c.replace(true) {
            return;
        }
        let line = diag_line(ls, canvas, view);
        if line.is_empty() {
            return;
        }
        eprintln!("{line}");
    });
}

/// 数值 → 诊断串：整数值不带小数点（`48` 而不是 `48.00`），非整数留两位。
fn num(v: f32) -> String {
    if (v - v.round()).abs() < 1e-4 {
        format!("{:.0}", v)
    } else {
        format!("{v:.2}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 运行时的画布矩形（与 `Layout::compute` 同一份计算，1500×950 窗口）。
    fn 画布() -> Rect {
        crate::layout::Layout::compute(Rect::from_min_size(
            pos2(0.0, 0.0),
            vec2(m::WINDOW_W, m::WINDOW_H),
        ))
        .viewport
    }

    /// 视口：`scale` 倍、平移 `(px, py)` 世界像素。
    fn 视口(scale: f32, px: f32, py: f32) -> CanvasView {
        let mut v = CanvasView::default();
        v.scale = scale;
        v.pan_x = px;
        v.pan_y = py;
        v
    }

    /// 一列屏幕坐标里**相邻两条的最小间距**（px）。少于两条时返回 `f32::INFINITY`。
    fn 最小间距(v: &[f32]) -> f32 {
        v.windows(2)
            .map(|w| w[1] - w[0])
            .fold(f32::INFINITY, f32::min)
    }

    /// 屏幕坐标反算成**世界格下标**（`(屏幕 - 画布原点) / 缩放 + 平移`，再除以格宽/高）。
    fn 世界下标(屏幕: f32, 原点: f32, pan: f32, zoom: f32, cell: f32) -> f32 {
        ((屏幕 - 原点) / zoom + pan) / cell
    }

    /// 真跑一次绘制，把 egui 收到的矩形图元都量出来（与 `画出来的都是1px线` 同一套做法）。
    fn 量绘制(v: &CanvasView, c: Rect) -> Vec<Rect> {
        let ctx = egui::Context::default();
        let out = ctx.run(egui::RawInput::default(), |ctx| {
            let p = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Background,
                egui::Id::new("grid-measure"),
            ));
            draw(&p.with_clip_rect(c), c, v);
        });
        out.shapes
            .iter()
            .filter_map(|cs| match &cs.shape {
                egui::Shape::Rect(r) => Some(r.rect),
                _ => None,
            })
            .collect()
    }

    /// **没有"缩放太小就不画"这回事**（上游 `901dbe2f` 订正）。
    ///
    /// 旧版照 `MapViewport.Presentation.cs` 的 `if (GridVisible && Zoom >= .4)` 抄了
    /// `MIN_SCALE = 0.4` + `visible_at()`，于是 0.39× 一条线都不画。**那行代码已被上游删除**
    /// （`901dbe2f`，2026-11-05 07:17），所以"0.39 不画"的断言在新行为下**是错的**：
    /// `32 × 0.39 = 12.48 ≥ 8` ⇒ `step = 1` ⇒ **照画**。
    ///
    /// ⇒ 这条是**订正**（不是放宽）：0.39× 必须画，且 `step == 1`。
    #[test]
    fn 不设缩放阈值永远画() {
        let c = 画布();
        for zoom in [4.0f32, 1.0, 0.5, 0.4, 0.39, m::CANVAS_MIN_SCALE] {
            let v = 视口(zoom, 0.0, 0.0);
            let ls = lines(&v, c);
            assert!(
                !ls.is_empty(),
                "{zoom}× 必须画线（上游 901dbe2f 已删除 0.4 门槛）"
            );
            assert_eq!(
                ls.step, 1,
                "{zoom}× 的 step 应为 1（32×{zoom} ≥ 8，边界含等号）"
            );
        }
        // 0.39 单独再钉一次：这正是旧断言"不画"被**订正**的那一档
        let ls39 = lines(&视口(0.39, 0.0, 0.0), c);
        assert!(
            !ls39.is_empty(),
            "0.39× 必须画（32×0.39 = 12.48 ≥ 8 ⇒ step = 1），旧断言「0.39 不画」依据的上游代码已删"
        );
        assert_eq!(ls39.step, 1, "0.39× 的 step 必须是 1");
        // 数值兜底（**不是**可见性阈值）：NaN / 0 / 负数缩放画不出有意义的线
        for bad in [f32::NAN, 0.0, -1.0] {
            assert!(
                lines(&视口(bad, 0.0, 0.0), c).is_empty(),
                "scale={bad} 时不该有线（NaN/0/负数是数值兜底，不是缩放门槛）"
            );
        }
    }

    /// **抽稀步长四档**（锁定新语义：`step` 而不是 0.4 阈值）。
    ///
    /// 依据 = 官方 `MapViewport.Presentation.cs:55-56@901dbe2f`
    /// `while (step * 32 * Zoom < 8) step *= 2`，`<` 不含等号 ⇒ 0.25× 仍是 1。
    ///
    /// ⚠ 这条取代了旧的 `缩放阈值判据`：旧用例守的是"阈值必须是官方 0.4"，而上游
    /// `901dbe2f` 已经把 0.4 删了 ⇒ 那条断言若留着，**谁按上游改谁就红**。
    #[test]
    fn 抽稀步长四档() {
        // (zoom, 期望 step, 依据)
        let tiers = [
            (0.5f32, 1u32, "32×0.5 = 16 ≥ 8"),
            (0.25, 1, "32×0.25 = 8 ≥ 8（边界，含等号）"),
            (0.2, 2, "32×0.2 = 6.4 < 8 ⇒ 2（6.4×2 = 12.8）"),
            (0.1, 4, "3.2 ⇒ 2(6.4) ⇒ 4(12.8)"),
        ];
        for (zoom, want, why) in tiers {
            assert_eq!(step_for(zoom), want, "{zoom}× 的 step 应为 {want}（{why}）");
            println!("[抽稀] zoom={zoom} → step={}（{why}）", step_for(zoom));
        }
        // 官方写死 32 就是本仓的格高；格宽 48 更松 ⇒ 竖线只会更疏
        assert_eq!(m::CELL_H, 32.0, "官方抽稀公式里的 32 必须是格高");
        // 缩放区间内**恒满足**"至少 8px"：这是 `step` 的定义，不是巧合。
        // 扫描从 0.05 起（低于缩放下限 0.25）—— 否则 `step > 1` 那半条判据永远跑不到。
        let mut z = 0.05f32;
        while z <= m::CANVAS_MAX_SCALE {
            let s = step_for(z) as f32;
            assert!(
                s * m::CELL_H * z >= 8.0 - 1e-3,
                "{z}× 抽稀后间距 {} < 8px",
                s * m::CELL_H * z
            );
            // 而且不能过度抽稀（翻倍前的上一档必须 < 8px，否则就是白丢线）
            if s > 1.0 {
                assert!(
                    (s / 2.0) * m::CELL_H * z < 8.0,
                    "{z}× 抽稀过头：step/2 也够 8px"
                );
            }
            z += 0.05;
        }
    }

    /// **抽稀的真实验证（四档）**：①`step` 值 ②相邻两条线的**屏幕间距 ≥ 8px** ③线起点是
    /// `step` 的整数倍。三条一起钉，缺一条就证明不了"抽稀真的生效"。
    ///
    /// 用**同一张图、同一个中心**（700×700、平移 0、画布不变）只换缩放 ——
    /// 这样四档之间唯一的变量就是 `step`。
    #[test]
    fn 抽稀后间距不小于八像素且起点对齐() {
        let c = 画布();
        let tiers = [(0.5f32, 1u32), (0.25, 1), (0.2, 2), (0.1, 4)];
        for (zoom, want_step) in tiers {
            let v = 视口(zoom, 0.0, 0.0);
            let ls = lines_in(&v, c, Some((700, 700)));
            // ① step
            assert_eq!(ls.step, want_step, "{zoom}× 的 step 应为 {want_step}");
            assert!(!ls.is_empty(), "{zoom}× 必须有线");
            // ② 相邻两条线的屏幕间距 ≥ 8px（贴实测值）
            let gx = 最小间距(&ls.xs);
            let gy = 最小间距(&ls.ys);
            assert!(
                gy >= 8.0 - 1e-3,
                "{zoom}× 横线最小间距 {gy} < 8px（step={}）",
                ls.step
            );
            assert!(
                gx >= 8.0 - 1e-3,
                "{zoom}× 竖线最小间距 {gx} < 8px（step={}）",
                ls.step
            );
            // ③ 起点（以及每一条）都落在 `step` 的整数倍世界格下标上
            //
            // 容差按**整像素取整**给：屏幕坐标是 `round()` 过的（模块文档 §四），
            // 0.5px 的取整误差换算成格数就是 `0.5 / (zoom × 格宽/高)`（0.1× 时约 0.10 格）。
            // 但真错了档（比如 step 少一半）会偏**整格**，这个容差挡得住。
            for (名字, 线, 原点, pan, cell) in [
                ("竖线", &ls.xs, c.left(), v.pan_x, m::CELL_W),
                ("横线", &ls.ys, c.top(), v.pan_y, m::CELL_H),
            ] {
                let 容差 = 0.5 / (zoom * cell) + 1e-3;
                for (k, 屏幕) in 线.iter().enumerate() {
                    let i = 世界下标(*屏幕, 原点, pan, zoom, cell);
                    assert!(
                        (i - i.round()).abs() < 容差,
                        "{zoom}× {名字}#{k} 屏幕 {屏幕} 不在整格上（下标 {i}，容差 {容差}）"
                    );
                    let i = i.round() as i32;
                    assert_eq!(
                        i.rem_euclid(want_step as i32),
                        0,
                        "{zoom}× {名字}#{k} 世界下标 {i} 不是 step={want_step} 的整数倍"
                    );
                }
            }
            let 首竖线 = 世界下标(ls.xs[0], c.left(), v.pan_x, zoom, m::CELL_W).round() as i32;
            let 首横线 = 世界下标(ls.ys[0], c.top(), v.pan_y, zoom, m::CELL_H).round() as i32;
            println!(
                "[抽稀实测] zoom={zoom} step={} 竖线 {} 条（最小间距 {gx:.2}px，首线世界下标 {首竖线}）\
                 / 横线 {} 条（最小间距 {gy:.2}px，首线世界下标 {首横线}）",
                ls.step,
                ls.xs.len(),
                ls.ys.len()
            );
        }
    }

    /// **步长对齐对负数也成立**：`ceil_to_step` 必须给出「**不小于** `i0` 的最小 `step` 倍数」。
    ///
    /// 为什么要有这条：兼容档（[`lines`]，不知道地图多大）与"视口拖到图左/上方之外"都会让
    /// 世界格下标变成**负数**，而 C# 那边 `x0` 已先被 `Math.Max(0, …)` 夹成非负，
    /// 所以官方那句 `(x0 + step - 1) / step * step` 从未被负数考验过 ——
    /// 本仓若直接照抄 `/`（截断除法）会往**零**的方向取整，负半轴就变成"向下取整"，
    /// 首线落到窗口外 ⇒ 屏幕上少一条线且相位跳一格。
    #[test]
    fn 步长对齐向上取整含负数() {
        // (i0, step, 期望)：不小于 i0 的最小 step 倍数
        let cases = [
            (0i32, 1i32, 0i32),
            (0, 4, 0),
            (1, 4, 4),
            (3, 4, 4),
            (4, 4, 4),
            (5, 4, 8),
            (-1, 4, 0),
            (-3, 4, 0),
            (-4, 4, -4),
            (-5, 4, -4),
            (-8, 4, -8),
            (-3, 2, -2),
            (-1, 2, 0),
            (5, 2, 6),
            (-7, 3, -6),
        ];
        for (i0, step, want) in cases {
            let got = ceil_to_step(i0, step);
            assert_eq!(got, want, "ceil_to_step({i0}, {step}) 应为 {want}");
            assert!(got >= i0, "对齐结果 {got} 不得小于起点 {i0}");
            assert_eq!(got % step, 0, "对齐结果 {got} 不是 {step} 的倍数");
            assert!(got - step < i0, "对齐结果 {got} 不是**最小**的（{got}-{step} 仍 ≥ {i0}）");
        }
        // 端到端：视口拖到图左上之外（pan 为负 ⇒ 世界下标为负）且 step=2 时，
        // 首线仍是"≥ x0 的最小 2 的倍数"，不是 0、也不是向下取整的 -22。
        let c = 画布();
        let v = 视口(0.2, -1000.0, -640.0);
        let ls = lines(&v, c);
        assert_eq!(ls.step, 2, "0.2× 的 step 应为 2");
        let (x0, y0, _, _) = v.visible_cells(c);
        assert!(x0 < 0 && y0 < 0, "这组平移应让世界下标为负：({x0},{y0})");
        let 首竖线 = 世界下标(ls.xs[0], c.left(), v.pan_x, v.scale, m::CELL_W).round() as i32;
        let 首横线 = 世界下标(ls.ys[0], c.top(), v.pan_y, v.scale, m::CELL_H).round() as i32;
        assert_eq!(首竖线.rem_euclid(2), 0, "首竖线 {首竖线} 不是 2 的倍数");
        assert_eq!(首横线.rem_euclid(2), 0, "首横线 {首横线} 不是 2 的倍数");
        assert!(
            首竖线 >= x0 && 首竖线 - x0 < 2,
            "首竖线应是 ≥ x0 的最小 2 的倍数：x0={x0} 首={首竖线}"
        );
        assert!(
            首横线 >= y0 && 首横线 - y0 < 2,
            "首横线应是 ≥ y0 的最小 2 的倍数：y0={y0} 首={首横线}"
        );
        println!("[负数对齐实测] zoom=0.2 step=2 x0={x0} → 首竖线 {首竖线}；y0={y0} → 首横线 {首横线}");
    }

    /// **0.25× 照画**（旧用例 `零点二五倍不画任何线` 的**订正**）。
    ///
    /// 0.25 是缩放区间下限，也是"≥ 8px"那条判据的**边界**：`32 × 0.25 = 8` 恰好等于 8
    /// ⇒ 官方 `<` 不含等号 ⇒ `step = 1`，横线间距正好 8px、竖线 12px。
    #[test]
    fn 零点二五倍照画且步长为一() {
        let c = 画布();
        for (px, py) in [(0.0, 0.0), (18240.0, 9920.0), (-500.0, -300.0)] {
            let v = 视口(m::CANVAS_MIN_SCALE, px, py);
            let ls = lines(&v, c);
            assert!(
                !ls.is_empty(),
                "0.25× 且平移 ({px},{py}) 时必须画线（上游 901dbe2f 删了 0.4 门槛）"
            );
            assert_eq!(ls.step, 1, "0.25× 的 32×0.25 = 8 **含等号** ⇒ step 仍是 1");
            assert!(
                (最小间距(&ls.ys) - m::CELL_H * 0.25).abs() < 1.0,
                "0.25× 横线间距应为 8px，实测 {}",
                最小间距(&ls.ys)
            );
            assert!(
                (最小间距(&ls.xs) - m::CELL_W * 0.25).abs() < 1.0,
                "0.25× 竖线间距应为 12px，实测 {}",
                最小间距(&ls.xs)
            );
        }
    }

    /// **格线裁到地图边界**：官方 `Presentation.cs:37-39@901dbe2f` 把枚举窗口夹成
    /// `Math.Max(0, …) … Math.Min(Document.Width/Height, …)` ⇒ 图外一条线都不画。
    ///
    /// 判据三条：① 带地图范围时，最右/最下的线正好落在**地图右下边界**上，再往外没有线；
    /// ② 同一视口**不传地图范围**（兼容档）线更多（证明夹范围真的生效，不是恒等）；
    /// ③ 视口整块在图外 ⇒ 空集（不是画一片空白格）。
    #[test]
    fn 地图外不画线() {
        let c = 画布();
        let v = 视口(1.0, 0.0, 0.0);
        // 一张 8×6 的小图：屏幕上是 8×48 = 384 px 宽、6×32 = 192 px 高
        let (mw, mh) = (8u16, 6u16);
        let ls = lines_in(&v, c, Some((mw, mh)));
        assert!(!ls.is_empty(), "图在视口里就必须有线");
        assert_eq!(ls.step, 1, "1× 的 step 应为 1");
        let right = c.left() + mw as f32 * m::CELL_W;
        let bottom = c.top() + mh as f32 * m::CELL_H;
        for x in &ls.xs {
            assert!(
                *x <= right + 0.01,
                "竖线 {x} 越出地图右边界 {right}（图外不许画）"
            );
        }
        for y in &ls.ys {
            assert!(
                *y <= bottom + 0.01,
                "横线 {y} 越出地图下边界 {bottom}（图外不许画）"
            );
        }
        // 地图右下那条边界线**要画**（官方 `for x = x0; x <= x1`）
        assert!(
            (ls.xs[ls.xs.len() - 1] - right).abs() < 0.01,
            "最右竖线 {} 应正好在地图右边界 {right}",
            ls.xs[ls.xs.len() - 1]
        );
        assert!(
            (ls.ys[ls.ys.len() - 1] - bottom).abs() < 0.01,
            "最下横线 {} 应正好在地图下边界 {bottom}",
            ls.ys[ls.ys.len() - 1]
        );
        // ② 不传地图范围（兼容档）时线更多 —— 证明夹范围真的起了作用
        let all = lines(&v, c);
        assert!(
            all.xs.len() > ls.xs.len() && all.ys.len() > ls.ys.len(),
            "兼容档（不夹范围）应比夹范围多画：竖 {} vs {}、横 {} vs {}",
            all.xs.len(),
            ls.xs.len(),
            all.ys.len(),
            ls.ys.len()
        );
        // ②b **线段长度也要夹**（官方 `:59`/`:61@901dbe2f` 的 `DrawLine` 端点就是夹过的格下标）：
        //     只夹"枚举哪些格"、不夹"每条线多长"，图外照样会拖出整屏的线
        //     （2026-11-05 截图取证实测：图外 20361 px 差异，全来自这半条判据）。
        assert!(
            (ls.y_span.0 - c.top()).abs() < 0.01 && (ls.y_span.1 - bottom).abs() < 0.01,
            "竖线的纵向范围应是地图上下边界：{:?} vs ({}, {bottom})",
            ls.y_span,
            c.top()
        );
        assert!(
            (ls.x_span.0 - c.left()).abs() < 0.01 && (ls.x_span.1 - right).abs() < 0.01,
            "横线的横向范围应是地图左右边界：{:?} vs ({}, {right})",
            ls.x_span,
            c.left()
        );
        // 兼容档（不夹）的线段范围是整块画布
        assert!(
            (all.y_span.1 - c.bottom()).abs() < 0.01 && (all.x_span.1 - c.right()).abs() < 0.01,
            "兼容档的线段范围应是整块画布：{:?} / {:?}",
            all.y_span,
            all.x_span
        );
        // ③ 把图整个拖到视口左上方之外 ⇒ 空集
        let out = lines_in(&视口(1.0, -1_000.0, -1_000.0), c, Some((mw, mh)));
        assert!(out.is_empty(), "视口整块在图外时不许画线");
        // ④ 没有地图（`Some((0, 0))`）⇒ 一条线都不画（**不许**退化成"不夹"去画一片空图）
        assert!(
            lines_in(&v, c, Some((0, 0))).is_empty(),
            "无地图时不许画网格"
        );
        assert!(
            lines_in(&v, c, Some((mw, 0))).is_empty(),
            "地图高为 0 时不许画网格"
        );
    }

    /// **线在格边界、间距正好 48 / 32**（1× 且平移为整数时，屏幕步长就是世界步长）。
    #[test]
    fn 线落在格边界且步长四八三二() {
        let c = 画布();
        let v = 视口(1.0, 0.0, 0.0);
        let ls = lines(&v, c);
        assert!(!ls.is_empty(), "1× 下必须有线");
        assert_eq!(ls.step, 1, "1× 的 32×1 = 32 ≥ 8 ⇒ step = 1");
        // 竖线：起点 = 画布左 + (0 - 0)×1 = 画布左；步长 48
        assert!((ls.xs[0] - c.left()).abs() < 0.01, "首条竖线应在 x={}", c.left());
        for w in ls.xs.windows(2) {
            assert!(
                (w[1] - w[0] - m::CELL_W).abs() < 0.01,
                "竖线间距 {} 不是 {}",
                w[1] - w[0],
                m::CELL_W
            );
        }
        for w in ls.ys.windows(2) {
            assert!(
                (w[1] - w[0] - m::CELL_H).abs() < 0.01,
                "横线间距 {} 不是 {}",
                w[1] - w[0],
                m::CELL_H
            );
        }
        // 每一条都必须落在"画布原点 + 整数倍世界步长"上（这正是验收要反查的那条判据）
        for x in &ls.xs {
            let k = (x - c.left() + v.pan_x) / m::CELL_W;
            assert!((k - k.round()).abs() < 1e-3, "x={x} 不是 48 的整数倍（k={k}）");
        }
        for y in &ls.ys {
            let k = (y - c.top() + v.pan_y) / m::CELL_H;
            assert!((k - k.round()).abs() < 1e-3, "y={y} 不是 32 的整数倍（k={k}）");
        }
    }

    /// **只画视口内**：每条线都落在画布矩形里（含左/上，不含右/下），
    /// 且"首线 = 第一条落进视口的格边界、末线 = 最后一条"，条数是**视口量级**而非地图量级。
    #[test]
    fn 只画视口内可见范围() {
        let c = 画布();
        let v = 视口(1.0, 0.0, 0.0);
        let ls = lines(&v, c);
        for x in &ls.xs {
            assert!(*x >= c.left() && *x < c.right(), "竖线 {x} 跑到画布外");
        }
        for y in &ls.ys {
            assert!(*y >= c.top() && *y < c.bottom(), "横线 {y} 跑到画布外");
        }
        // 首线 = 第一条 ≥ 画布左边界的世界格边界（判据用世界坐标独立算，不复用 `visible_cells`）
        let first_x = (v.pan_x / m::CELL_W).ceil() * m::CELL_W;
        let want_x = c.left() + (first_x - v.pan_x) * v.scale;
        assert!((ls.xs[0] - want_x).abs() <= 1.0, "首条竖线 {} 应为 {want_x}", ls.xs[0]);
        let first_y = (v.pan_y / m::CELL_H).ceil() * m::CELL_H;
        let want_y = c.top() + (first_y - v.pan_y) * v.scale;
        assert!((ls.ys[0] - want_y).abs() <= 1.0, "首条横线 {} 应为 {want_y}", ls.ys[0]);
        // 末线必须离右/下边界不到一格（否则说明漏画了）
        assert!(
            c.right() - ls.xs[ls.xs.len() - 1] <= m::CELL_W * v.scale + 1.0,
            "右侧漏画：末线 {} 距右边 {} 超过一格",
            ls.xs[ls.xs.len() - 1],
            c.right()
        );
        assert!(
            c.bottom() - ls.ys[ls.ys.len() - 1] <= m::CELL_H * v.scale + 1.0,
            "下侧漏画：末线 {} 距下边 {} 超过一格",
            ls.ys[ls.ys.len() - 1],
            c.bottom()
        );
        // 一屏的线数是"视口能塞几格"量级（987/48 ≈ 21、815/32 ≈ 26），不是"地图多大"量级
        let (_, _, w, h) = v.visible_cells(c);
        assert!(
            (ls.xs.len() as i64 - w as i64).abs() <= 1,
            "竖线 {} 条与可见列数 {w} 不符",
            ls.xs.len()
        );
        assert!(
            (ls.ys.len() as i64 - h as i64).abs() <= 1,
            "横线 {} 条与可见行数 {h} 不符",
            ls.ys.len()
        );
        assert!(ls.xs.len() < 64, "竖线 {} 条太多了", ls.xs.len());
        assert!(ls.ys.len() < 64, "横线 {} 条太多了", ls.ys.len());
    }

    /// 平移改变**首线位置**（线跟着世界走），但**步长不变**；
    /// 平移"正好一格高"时横线首线一动不动 —— 这正是"网格钉在世界坐标上"的证据。
    #[test]
    fn 平移改首线不改步长() {
        let c = 画布();
        let a = lines(&视口(1.0, 0.0, 0.0), c);
        // 平移 100 世界像素（= 2×48 + 4）：竖线整体左移 100，但首线仍是"第一条落进视口的边界"
        let b = lines(&视口(1.0, 100.0, 0.0), c);
        assert_ne!(a.xs[0], b.xs[0], "平移 100 世界像素后首线位置必须变");
        assert!(
            (b.xs[0] - c.left()).abs() < m::CELL_W,
            "首线 {} 必须落在 [left, left+48) 里",
            b.xs[0]
        );
        for w in b.xs.windows(2) {
            assert!((w[1] - w[0] - m::CELL_W).abs() < 0.01, "步长必须仍是 48");
        }
        // 平移 32（正好一格高）：横线的首线**不动**（32 是 32 的整数倍）
        let d = lines(&视口(1.0, 0.0, m::CELL_H), c);
        assert!(
            (d.ys[0] - a.ys[0]).abs() < 0.01,
            "平移一格高时横线首线应不动：{} vs {}",
            d.ys[0],
            a.ys[0]
        );
        for w in d.ys.windows(2) {
            assert!((w[1] - w[0] - m::CELL_H).abs() < 0.01, "步长必须仍是 32");
        }
    }

    /// 放大到 2× 时步长跟着变 96 / 64，但**线宽仍是 1px**（线宽不随缩放）。
    #[test]
    fn 放大后步长加倍而线宽不变() {
        let c = 画布();
        let ls = lines(&视口(2.0, 0.0, 0.0), c);
        assert_eq!(ls.step, 1, "2× 不需要抽稀");
        for w in ls.xs.windows(2) {
            assert!((w[1] - w[0] - m::CELL_W * 2.0).abs() < 0.01, "2× 步长应为 96");
        }
        for w in ls.ys.windows(2) {
            assert!((w[1] - w[0] - m::CELL_H * 2.0).abs() < 0.01, "2× 步长应为 64");
        }
        assert_eq!(m::GRID_LINE_W, 1.0, "线宽恒 1px，不随缩放变");
    }

    /// **线宽恒 1px**：在 0.5 与 0.1 两档下量**同一条线**（世界 `x = 0` 那条竖线）的屏幕宽度。
    ///
    /// 官方口径是**世界单位**笔宽 `1 / Zoom`（`MapViewport.Presentation.cs:57@901dbe2f`），
    /// 乘回 `Zoom` 后**屏幕恒 1px** —— 所以两档必须量出同一个数（旧版固定 0.6 已随上游删除）。
    #[test]
    fn 两档缩放线宽都恒一像素() {
        let c = 画布();
        for zoom in [0.5f32, 0.1] {
            // 官方口径自证：世界笔宽 1/Zoom × Zoom = 1 屏幕像素
            let 世界笔宽 = 1.0 / zoom;
            assert!(
                (世界笔宽 * zoom - 1.0).abs() < 1e-4,
                "{zoom}× 的世界笔宽 {世界笔宽} 乘回缩放应为 1px"
            );
            let v = 视口(zoom, 0.0, 0.0);
            let ls = lines_in(&v, c, Some((700, 700)));
            assert!(!ls.is_empty(), "{zoom}× 必须有线");
            let rects = 量绘制(&v, c);
            // 竖线 = 宽 1、高 > 1 的矩形；世界 x=0 那条就是最左边那条（平移为 0）
            let mut 竖线: Vec<Rect> = rects
                .iter()
                .copied()
                .filter(|r| r.width() < 2.0 && r.height() > 2.0)
                .collect();
            assert_eq!(竖线.len(), ls.xs.len(), "{zoom}× 竖线图元数应与线数一致");
            竖线.sort_by(|a, b| a.left().partial_cmp(&b.left()).unwrap());
            let 首条 = 竖线[0];
            assert!(
                (首条.width() - 1.0).abs() < 1e-6,
                "{zoom}× 首条竖线宽 {} ≠ 1px",
                首条.width()
            );
            assert!(
                (首条.left() - c.left()).abs() < 0.01,
                "{zoom}× 首条竖线应在世界 x=0（画布左 {}），实测 {}",
                c.left(),
                首条.left()
            );
            println!(
                "[线宽实测] zoom={zoom} 世界笔宽 1/Zoom={世界笔宽:.4} × Zoom = {:.4}px；\
                 世界 x=0 那条竖线屏幕宽 {}px（left={}）",
                世界笔宽 * zoom,
                首条.width(),
                首条.left()
            );
            // 所有竖线都是 1px（不是 2px，也不是随缩放变粗）
            for r in &竖线 {
                assert!(
                    (r.width() - m::GRID_LINE_W).abs() < 1e-6,
                    "{zoom}× 有竖线宽 {} ≠ {}",
                    r.width(),
                    m::GRID_LINE_W
                );
            }
        }
    }

    /// 画出来的**每一个矩形都是 1px 的线**（不是 2px，也不是整格填充）。
    ///
    /// 这条是"线宽恒 1px"的结构性判据：光断言常量等于 1.0 挡不住"绘制处又乘了 scale"
    /// 这种改法，所以这里真跑一次绘制、把 egui 收到的图元逐个量宽高。
    #[test]
    fn 画出来的都是1px线() {
        let c = 画布();
        let v = 视口(1.0, 0.0, 0.0);
        let ls = lines(&v, c);
        assert!(!ls.is_empty());

        let ctx = egui::Context::default();
        let out = ctx.run(egui::RawInput::default(), |ctx| {
            let p = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Background,
                egui::Id::new("grid-1px"),
            ));
            draw(&p.with_clip_rect(c), c, &v);
        });

        let mut n = 0;
        for cs in &out.shapes {
            let egui::Shape::Rect(r) = &cs.shape else {
                continue;
            };
            n += 1;
            let (w, h) = (r.rect.width(), r.rect.height());
            assert!(
                (w - m::GRID_LINE_W).abs() < 0.01 || (h - m::GRID_LINE_W).abs() < 0.01,
                "图元 {w}×{h} 不是 1px 的线（线宽必须恒 {}）",
                m::GRID_LINE_W
            );
        }
        assert_eq!(
            n,
            ls.len(),
            "画上去的图元数必须等于算出来的线数（竖 {} + 横 {}）",
            ls.xs.len(),
            ls.ys.len()
        );
    }

    /// **诊断行必须写明抽稀步长**（验收第 3 条要"贴 `[网格]` 诊断行"核对 `step`）。
    ///
    /// 四档各拼一次真实文案并打出来；同时钉住"步长报的是**屏上真实间距**"这条语义
    /// （抽稀后若还报单格间距 `48×zoom`，验收就会拿它去反查一条不存在的格线）。
    #[test]
    fn 诊断行带抽稀步长() {
        let c = 画布();
        for (zoom, want_step) in [(0.5f32, 1u32), (0.25, 1), (0.2, 2), (0.1, 4)] {
            let v = 视口(zoom, 0.0, 0.0);
            let ls = lines_in(&v, c, Some((700, 700)));
            assert_eq!(ls.step, want_step, "{zoom}× 的 step 应为 {want_step}");
            let line = diag_line(&ls, c, &v);
            println!("{line}");
            assert!(!line.contains('{'), "诊断行还有没替换的占位符：{line}");
            assert!(
                line.contains(&format!("每 {} 格一条", ls.step)),
                "{zoom}× 的诊断行没写明抽稀步长 {want_step}：{line}"
            );
            // 步长 = 屏上真实间距（格宽/格高 × 缩放 × step）
            let 期望dx = num(m::CELL_W * zoom * want_step as f32);
            let 期望dy = num(m::CELL_H * zoom * want_step as f32);
            assert!(
                line.contains(&format!("步长 {期望dx}")) && line.contains(&format!("步长 {期望dy}")),
                "{zoom}× 的诊断行步长应为 dx={期望dx} / dy={期望dy}：{line}"
            );
        }
    }

    /// 诊断串里的数值格式：整数不带小数点，非整数两位（验收脚本按它解析首线坐标）。
    #[test]
    fn 诊断数值格式() {
        assert_eq!(num(48.0), "48");
        assert_eq!(num(-470.0), "-470");
        assert_eq!(num(0.5), "0.50");
        assert_eq!(num(48.000_001), "48");
    }
}
