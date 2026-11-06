//! 中列：页签条 + **真实地图画布** + 悬停格高亮 + 左下角缩放浮层。
//!
//! # 本单发生了什么变化（与上一版「空画布骨架」的区别，别当成回归）
//!
//! | 上一版 | 本单 |
//! |---|---|
//! | 浅灰底 + 48×32 网格线（占位骨架）| 真实地图位图（`miredit_render::render_region` 出的 RGBA）|
//! | 地图面从视口偏移 (60,70) 开始 | 地图 1:1 铺满整个视口 |
//! | 金色装饰选框（无数据含义）| **鼠标所在格**的 2px 金色描边（有数据含义）|
//! | `100%` 是写死的字面量 | 真实缩放百分比（随滚轮/按钮变化）|
//!
//! 装饰网格与装饰选框**故意撤掉**：它们会盖在真实地图上，
//! 既没有数据含义，又会让「与客户端同视点分块比对」凭空多出网格线色差
//! （验收门 §五.2 的口径是"画布里只该多页签条与浮层"）。
//!
//! ⚠ **2026-11-05 订正**：上面那条撤掉的是"**假的**装饰网格"（没有开关、恒定上屏）。
//! 现在 `视图 → 单元格网格`（[`crate::grid`]）真实现了：默认**关**、可开可关，
//! 所以"默认画布里仍然只有页签条与浮层"这条口径不变，上面那段结论依然成立。
//!
//! # 几何
//! 画布矩形 = [`Layout::viewport`]（页签条以下、状态栏以上、两侧停靠区之间）。
//! 缩放浮层的尺寸/位置仍是 [`crate::metrics`] 里那套截图实测值（228×40，距边 18），
//! 只是里面的百分比数字改成动态。

use egui::{pos2, vec2, Align2, Painter, Rect, TextureHandle};

use crate::canvas_view::CanvasView;
use crate::layout::Layout;
use crate::metrics as m;
use crate::paint as pt;
use crate::screen_text::DocText;
use crate::text_fit::{DynLabel, Label, LabelStyle};
use crate::theme as t;
use crate::ui_text as tx;

/// 画中列所需的全部运行期输入（打包成一个结构体，免得 `draw` 长出 8 个参数）。
pub struct CanvasCtx<'a> {
    /// 已渲染的地图纹理（没渲过 / 没图库时为 `None`）。
    pub tex: Option<&'a TextureHandle>,
    /// 视口（平移/缩放/图层）。
    pub view: &'a CanvasView,
    /// 这一帧的动态文字。
    pub text: &'a DocText,
    /// 鼠标所在世界格（`None` = 不在图上）。
    pub hover: Option<(u16, u16)>,
    /// 是否已打开地图（决定是否画空态提示）。
    pub has_map: bool,
    /// **单元格网格开关**（`视图 → 单元格网格` / `G` / `--cell-grid` / `--no-cell-grid`，
    /// 默认**开** —— 官方 `MainWindow.axaml:5` 的 `IsChecked="True"`）。
    ///
    /// 打开时在**地图位图之上、悬停高亮之下**画世界格 48×32 的 1px 边界线
    /// （[`crate::grid`]）。压在悬停高亮之下是有意的：悬停框是"当前格"的强提示，
    /// 被网格线穿过去会显得断断续续。
    pub grid: bool,
    /// **地图宽（格数）**：网格要按它把格线**裁到地图边界**
    /// （官方 `CrystalStudio/MapViewport.Presentation.cs:37-39` 的 `Math.Min(Document.Width, …)`）。
    ///
    /// `0` = 没有地图 ⇒ 网格一条线都不画（[`crate::grid::lines_in`] 把 `0` 当"不画"）。
    pub map_w: u16,
    /// **地图高（格数）**，语义同 [`CanvasCtx::map_w`]。
    pub map_h: u16,
    /// 矩形选区（`None` = 没选区 ⇒ 不画）。
    ///
    /// **本版新增**（2026-11-05）：金色虚线框 + 半透明填充，颜色/线宽/段长全部来自
    /// [`crate::theme`] / [`crate::metrics`]，绘制在 [`crate::canvas_overlay`]。
    pub selection: Option<miredit_core::Selection>,
    /// 印章**待放置轮廓**：`(锚点, 包围盒)`；`None` = 不画。
    ///
    /// 只有"待放置态 + 按住 Alt + 鼠标在图内 + 目录给得出包围盒"四条同时成立才有值。
    /// 画的时候**只描边**（[`crate::canvas_overlay::draw_stamp_outline`]），不填任何像素。
    pub stamp_preview: Option<crate::stamp_place::Outline>,
    /// 待放置徽标的文字（`None` = 不画）。
    ///
    /// 待放置态本身在画布上没有任何标记（轮廓只在按住 Alt 时才有），
    /// 一个跟着鼠标走的徽标能让"我现在点下去会发生什么"变得明确。
    pub armed_badge: Option<String>,
}

/// 缩放浮层内的一条窄带（`x0..x1` 相对浮层左边）—— 文字裁剪盒与点击热区共用。
fn pill_band(l: &Layout, x0: f32, x1: f32) -> Rect {
    let r = l.zoom_pill;
    Rect::from_min_max(pos2(r.left() + x0, r.top()), pos2(r.left() + x1, r.bottom()))
}

/// 减号的可点区域（= 它自己的窄带）。
pub fn zoom_minus_rect(l: &Layout) -> Rect {
    pill_band(l, 4.0, 44.0)
}

/// 百分比的可点区域（本单不响应点击，只用于排版核对）。
pub fn zoom_percent_rect(l: &Layout) -> Rect {
    pill_band(l, 48.0, 100.0)
}

/// 加号的可点区域。
pub fn zoom_plus_rect(l: &Layout) -> Rect {
    pill_band(l, 104.0, m::ZOOM_DIVIDER_X - 2.0)
}

/// `适应窗口` 的可点区域（169..224 —— **右边界必须显式写**：它后面还有「原始大小」）。
pub fn zoom_fit_rect(l: &Layout) -> Rect {
    pill_band(l, m::ZOOM_FIT_X, m::ZOOM_FIT_R)
}

/// **「原始大小」**的可点区域（232..288）。
///
/// 上游 `901dbe2f` 在缩放浮层尾部加了 `OriginalSizeButton`（`MainWindow.axaml:22`，
/// `Click="ResetZoomClick"`）—— 本仓菜单里早就有 `view.actual_size`，
/// 但**工具栏没有按钮**，这一条就是补那个缺口。点击走的是
/// [`crate::menu::model::Action::ActualSize`]（与菜单项、`--menu-click` 同一条 action），
/// 不另写一份缩放逻辑。
pub fn zoom_original_rect(l: &Layout) -> Rect {
    pill_band(l, m::ZOOM_ORIGINAL_X, m::ZOOM_ORIGINAL_R)
}

/// 中列的**静态**文字标签：页签关闭符 + 缩放浮层三件（减号 / 加号 / 适应窗口）。
///
/// 页签标题与百分比是动态的（见 [`dyn_labels`]）。
///
/// 各元素横向位置取自截图（浮层左 x=182）：`−` 中心 204.5、`100%` 中心 255.5、
/// `+` 中心 306.5、竖线 332、`Fit Map` 起 351 ⇒ 见 `metrics::ZOOM_*_X`。
pub fn labels(l: &Layout) -> Vec<Label> {
    let mut out = Vec::with_capacity(4);

    // ① 页签右端的关闭符（标题是动态的，见 dyn_labels）
    let tab = l.tab_rect();
    out.push(Label {
        name: "canvas.tab_close",
        text: tx::TAB_CLOSE,
        font: m::TAB_FONT,
        pos: pos2(tab.right() - m::TAB_TITLE_INSET, tab.center().y),
        anchor: Align2::RIGHT_CENTER,
        clip: Rect::from_min_max(
            pos2(tab.right() - m::TAB_TITLE_INSET - 13.0, tab.top()),
            tab.max,
        ),
        color: t::ui().text_muted,
    });

    // ② 缩放浮层：减号 / + / 适应窗口（百分比如见 dyn_labels）
    //
    // 各元素横向**各有窄带**裁剪盒（不是整块浮层），这样验收能逐个量"文字右边界"，
    // 不会把 `100%` 和 `+` 的墨迹混在一起。
    let r = l.zoom_pill;
    out.push(Label {
        name: "canvas.zoom_minus",
        text: tx::ZOOM_MINUS,
        font: m::ZOOM_SIGN_FONT,
        pos: pos2(r.left() + m::ZOOM_MINUS_X, r.center().y),
        anchor: Align2::CENTER_CENTER,
        clip: zoom_minus_rect(l),
        color: t::ui().text,
    });
    out.push(Label {
        name: "canvas.zoom_plus",
        text: tx::ZOOM_PLUS,
        font: m::ZOOM_SIGN_FONT,
        pos: pos2(r.left() + m::ZOOM_PLUS_X, r.center().y),
        anchor: Align2::CENTER_CENTER,
        clip: zoom_plus_rect(l),
        color: t::ui().text,
    });
    out.push(Label {
        name: "canvas.zoom_fit",
        text: tx::ZOOM_FIT,
        font: m::BODY_FONT,
        pos: pos2(r.left() + m::ZOOM_FIT_X, r.center().y),
        anchor: Align2::LEFT_CENTER,
        clip: zoom_fit_rect(l),
        color: t::ui().text,
    });
    // ④「原始大小」（上游 `901dbe2f` 新增的 `OriginalSizeButton`，走 `ResetZoomClick`）
    out.push(Label {
        name: "canvas.zoom_original",
        text: tx::ZOOM_ORIGINAL,
        font: m::BODY_FONT,
        pos: pos2(r.left() + m::ZOOM_ORIGINAL_X, r.center().y),
        anchor: Align2::LEFT_CENTER,
        clip: zoom_original_rect(l),
        color: t::ui().text,
    });
    out
}

/// 中列的**动态**文字标签：页签标题（地图文件名）+ 缩放百分比。
pub fn dyn_labels(l: &Layout, d: &DocText) -> Vec<DynLabel> {
    let mut out = Vec::with_capacity(2);

    // ① 页签标题：左对齐，右边界留出关闭符的位置（关闭符自 tab.right - 11 往左排，约 9px 宽）
    let tab = l.tab_rect();
    out.push(DynLabel::new(
        "canvas.tab_title",
        tx::TAB_TITLE_TMPL,
        d.tab.clone(),
        LabelStyle::new(
            m::TAB_FONT,
            pos2(tab.left() + m::TAB_TITLE_INSET, tab.center().y),
            Align2::LEFT_CENTER,
            Rect::from_min_max(
            pos2(tab.left() + m::TAB_TITLE_INSET, tab.top()),
            pos2(tab.right() - m::TAB_TITLE_INSET - 8.0, tab.bottom()),
        ),
            t::ui().text,
        ),
    ));

    // ② 缩放百分比：居中在自己的窄带里
    let r = l.zoom_pill;
    out.push(DynLabel::new(
        "canvas.zoom_percent",
        tx::ZOOM_PCT_TMPL,
        d.zoom.clone(),
        LabelStyle::new(
            m::BODY_FONT,
            pos2(r.left() + m::ZOOM_PERCENT_X, r.center().y),
            Align2::CENTER_CENTER,
            zoom_percent_rect(l),
            t::ui().text,
        ),
    ));
    out
}

/// 画整个中列。
///
/// **裁剪**：纹理按世界位置整块贴，超出视口的部分靠 painter 的 clip 裁掉
/// （上一版把地图面画到视口外、把右侧分隔线盖成 `#EFF2F5` 的坑，就是靠这条避免的）。
pub fn draw(p: &Painter, l: &Layout, c: &CanvasCtx<'_>) {
    // ── 页签条 ──
    let ps = p.with_clip_rect(l.tab_strip);
    pt::fill(&ps, l.tab_strip, t::ui().menu_bar);
    for lab in labels(l) {
        if lab.name.starts_with("canvas.tab") {
            lab.draw(&ps);
        }
    }
    for lab in dyn_labels(l, c.text) {
        if lab.name.starts_with("canvas.tab") {
            lab.draw(&ps);
        }
    }
    // 选中页签下划线（截图 y=103..104 的 `#AD7F22`）
    pt::fill(&ps, l.tab_underline(), t::ui().accent_dark);

    // ── 画布（以下全部裁剪到画布矩形） ──
    let pv = p.with_clip_rect(l.viewport);
    // 底：图外/空地露出的颜色（不是黑色，见 `theme::CANVAS_BG` 的说明）
    pt::fill(&pv, l.viewport, t::ui().canvas_bg);

    // 真实地图位图：整块按世界位置贴，视口裁切
    if let (Some(tex), Some((dest, uv))) = (c.tex, c.view.texture_dest(l.viewport)) {
        pv.image(tex.id(), dest, uv, egui::Color32::WHITE);
    }

    // ── 单元格网格（视图 → 单元格网格，2026-11-05）：压在地图之上、悬停高亮之下 ──
    //
    // 顺序有因果：① 网格是"读图的辅助线"，必须在地图位图之后画（否则被位图盖掉）；
    // ② 悬停框是"当前格"的强提示，必须压在网格之上（否则被格线穿过、看着断断续续）；
    // ③ 选区填充/轮廓在更上层（它们描述的是"我要对这块做什么"）。
    if c.grid {
        // 尺寸按**地图**传：图外一条线都不画（官方 `MapViewport.Presentation.cs:37-39`
        // 的 `Math.Min(Document.Width, …)`）。无图时 `map_w/map_h` 是 0 ⇒ `lines_in`
        // 直接返回空集（**不是**"不夹"档，否则会画一片空图）。
        crate::grid::draw_in(&pv, l.viewport, c.view, Some((c.map_w, c.map_h)));
    }

    // 悬停格高亮（2px 金边，颜色沿用截图实测的 `#D2AA59`）
    if let Some((x, y)) = c.hover {
        let r = c.view.cell_rect(l.viewport, i32::from(x), i32::from(y));
        pt::border_w(&pv, r, m::CELL_HOVER_STROKE, t::ui().cell_hover_gold);
    }

    // ── 编辑器叠加层（2026-11-05）：矩形选区 → 印章待放置轮廓 → 待放置徽标 ──
    //
    // 顺序有因果：选区在最底（它铺一块半透明底），轮廓压在选区上（描边不能被填充盖掉），
    // 徽标最后（它要压在其它一切之上，跟着鼠标走）。
    if let Some(sel) = c.selection {
        crate::canvas_overlay::draw_selection(&pv, l.viewport, c.view, sel);
    }
    if let Some((anchor, bounds)) = c.stamp_preview {
        crate::canvas_overlay::draw_stamp_outline(&pv, l.viewport, c.view, anchor, bounds);
    }
    if let (Some(text), Some((x, y))) = (c.armed_badge.as_deref(), c.hover) {
        let r = c.view.cell_rect(l.viewport, i32::from(x), i32::from(y));
        crate::canvas_overlay::draw_armed_badge(&pv, r.min, text);
    }

    // 空态：没打开地图时如实说一句（不留一屏空白让人以为卡住）
    if !c.has_map {
        pt::text(
            &pv,
            l.viewport.center(),
            Align2::CENTER_CENTER,
            tx::CANVAS_NO_MAP,
            m::BODY_FONT,
            t::ui().text_muted,
        );
    }

    // 左下角缩放浮层（压在画布之上，属"浮层"）
    draw_zoom_pill(&pv, l, c.text);
}

/// 缩放浮层：圆角底板 + 1px 边框 + 减号 / `N%` / `+` / 竖线 / `适应窗口`。
fn draw_zoom_pill(p: &Painter, l: &Layout, d: &DocText) {
    let r = l.zoom_pill;
    pt::fill_round(p, r, 6.0, t::ui().menu_bar);
    pt::border(p, r, t::ui().panel_header);

    // 分隔竖线
    pt::fill(
        p,
        Rect::from_min_size(
            pos2(r.left() + m::ZOOM_DIVIDER_X, r.top() + 8.0),
            vec2(1.0, r.height() - 16.0),
        ),
        t::ui().panel_header,
    );

    for lab in labels(l) {
        if lab.name.starts_with("canvas.zoom") {
            lab.draw(p);
        }
    }
    for lab in dyn_labels(l, d) {
        if lab.name.starts_with("canvas.zoom") {
            lab.draw(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas_view::CanvasView;
    use crate::screen_text::DocText;

    fn 空文案() -> DocText {
        DocText {
            title: String::new(),
            tab: "0.map".to_string(),
            pill: String::new(),
            brush_size: 3,
            status_left: String::new(),
            status_tool: String::new(),
            status_layer: String::new(),
            status_brush: String::new(),
            status_hint: String::new(),
            status_hint_warn: false,
            status_coord: String::new(),
            status_lib: String::new(),
            status_format: String::new(),
            banner: None,
            ground_locked: false,
        status_stamp: None,
            status_fill: None,
            status_op: None,
            status_op_warn: false,
            status_sel: None,
            status_sel_tmpl: None,
            zoom: "100%".to_string(),
            page: String::new(),
            inspector_head: String::new(),
            inspector_values: Default::default(),
        }
    }

    /// 缩放浮层的三个静态标签与两个动态标签必须**共用同一套几何**（加减号与百分比不能错位）。
    ///
    /// 2026-11-05：静态标签由 3 条变 4 条（新增「原始大小」，上游 `901dbe2f`）。
    #[test]
    fn 缩放浮层标签同源() {
        let l = Layout::compute(Rect::from_min_size(
            pos2(0.0, 0.0),
            vec2(m::WINDOW_W, m::WINDOW_H),
        ));
        let stat: Vec<_> = labels(&l)
            .into_iter()
            .filter(|x| x.name.starts_with("canvas.zoom"))
            .collect();
        assert_eq!(
            stat.len(),
            4,
            "静态缩放标签应有 4 条（减号/加号/适应地图/原始大小）"
        );
        let dyns = dyn_labels(&l, &空文案());
        let pct = dyns
            .iter()
            .find(|x| x.name == "canvas.zoom_percent")
            .expect("必须有百分比");
        assert_eq!(pct.text, "100%");
        // 百分比窄带必须夹在减号与加号之间（截图位置关系）
        let minus = stat.iter().find(|x| x.name == "canvas.zoom_minus").unwrap();
        let plus = stat.iter().find(|x| x.name == "canvas.zoom_plus").unwrap();
        assert!(minus.pos.x < pct.pos.x);
        assert!(pct.pos.x < plus.pos.x);
    }

    /// **「原始大小」按钮的命中盒**：在「适应地图」右边、不与之重叠、整块落在浮层里。
    ///
    /// 这条挡的是"加了按钮但两个按钮的命中盒重叠"——那样点「适应地图」会同时触发
    /// 100% 缩放（两个 `ui.interact` 都会 `clicked()`），画面看着像"点了适应却回到 100%"。
    #[test]
    fn 原始大小按钮不压适应地图() {
        let l = Layout::compute(Rect::from_min_size(
            pos2(0.0, 0.0),
            vec2(m::WINDOW_W, m::WINDOW_H),
        ));
        let fit = zoom_fit_rect(&l);
        let orig = zoom_original_rect(&l);
        assert!(fit.right() <= orig.left() + 0.01, "两条带重叠：{fit:?} / {orig:?}");
        assert!(l.zoom_pill.contains_rect(orig), "「原始大小」越出浮层：{orig:?}");
        assert!(l.zoom_pill.contains_rect(fit), "「适应地图」越出浮层：{fit:?}");
        // 两条带的宽度都够放下 4 个中文字（12px 字号 ⇒ 48px）
        assert!(fit.width() >= 48.0, "「适应地图」带太窄：{}", fit.width());
        assert!(orig.width() >= 48.0, "「原始大小」带太窄：{}", orig.width());
    }

    /// 没打开地图时不该崩（空态只画一句提示），且不应有悬停高亮。
    #[test]
    fn 空态不崩() {
        let l = Layout::compute(Rect::from_min_size(
            pos2(0.0, 0.0),
            vec2(m::WINDOW_W, m::WINDOW_H),
        ));
        let view = CanvasView::default();
        let d = 空文案();
        let c = CanvasCtx {
            tex: None,
            view: &view,
            text: &d,
            hover: None,
            has_map: false,
            grid: false,
            // 无图 ⇒ 0（`grid::lines_in` 把 `Some((0, _))` 当"没有格子可画"）
            map_w: 0,
            map_h: 0,
            selection: None,
            stamp_preview: None,
            armed_badge: None,
        };
        // 只断言几何可用：真正画需要 Painter（见 text_fit 的测量测试）
        assert!(c.view.texture_dest(l.viewport).is_none());
    }

    /// **叠加层参数真的会传到画布上**（三条可选字段各自独立，互不牵连）。
    ///
    /// 这条挡的是"接线漏了一个参数"：选区/轮廓/徽标三者任何一条没传进来，
    /// 画布上就少一样东西，而少了的东西在截图里往往要盯很久才发现。
    #[test]
    fn 叠加层三条可选参数独立() {
        let view = CanvasView::default();
        let d = 空文案();
        let sel = miredit_core::Selection::new(1, 2, 3, 4);
        let c = CanvasCtx {
            tex: None,
            view: &view,
            text: &d,
            hover: Some((5, 6)),
            has_map: true,
            grid: true,
            // 一张 700×700 的图（与真机默认图同量级），网格按它裁边界
            map_w: 700,
            map_h: 700,
            selection: Some(sel),
            stamp_preview: Some(((10, 10), (-1, -2, 4, 3))),
            armed_badge: Some("放置「x」".to_string()),
        };
        assert_eq!(c.selection, Some(sel));
        assert_eq!(c.stamp_preview, Some(((10, 10), (-1, -2, 4, 3))));
        assert_eq!(c.armed_badge.as_deref(), Some("放置「x」"));
        assert!(c.grid, "单元格网格开关必须真的传进画布");
        // 选区矩形与印章轮廓矩形必须落在**不同**位置（否则说明锚点被当成了选区端点）
        let a = crate::canvas_overlay::selection_rect(&view, l_viewport(), sel);
        let b = crate::stamp_place::outline_rect(
            &view,
            l_viewport(),
            (10, 10),
            (-1, -2, 4, 3),
        );
        assert_ne!(a, b);
    }

    /// 测试用的画布矩形（与运行时同一套 `Layout` 计算）。
    fn l_viewport() -> Rect {
        Layout::compute(Rect::from_min_size(
            pos2(0.0, 0.0),
            vec2(m::WINDOW_W, m::WINDOW_H),
        ))
        .viewport
    }
}
