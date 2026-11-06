//! 状态栏：左「地图文件名 · 宽×高 格」（或错误原因）、中「工具：笔刷」「当前层：物体」
//! 「笔刷：物体 图号 N · 资源库 M」「落笔 fr_img + btArea」、右「单元格 x,y」与「● 资源库 N 个」。
//!
//! 几何（高 30 = 1px 分隔线 + 29px 内容；左右内边距 16）见 [`crate::metrics`]；
//! 颜色（底色 `#F7F8F9`、正文 `#434D5A`）见 [`crate::theme`]；文案见 [`crate::ui_text`]。
//!
//! # 八段全是真实数据
//!
//! | 段 | 取值 | 说明 |
//! |---|---|---|
//! | 左 | `0.map · 700×700 格`；失败时是具体错误（含保存失败）| 优先级见 [`crate::screen_text`] |
//! | 工具 | `工具：笔刷` / `工具：吸管` / `工具：橡皮擦` / `工具：填充` | — |
//! | **当前层** | `当前层：物体` | 落笔写哪一层（金强调色）|
//! | 笔刷 | `笔刷：物体 图号 901 · 资源库 4`；没取过是 `笔刷：未取（先用吸管）` | 笔刷**带族** |
//! | **填充** | `填充 1234 格` / `应用低位阻挡 1234 格` | **本版新增**：上一次填充的回执（官方 `MapViewport.cs:537`）；没填过整段不存在，挤不下时整段让位 |
//! | **落笔** | `落笔 fr_img + btArea` / `落笔 mid_img（本图 12B 档无库号字节，只写图号）` / 族不匹配的拒绝原因 | — |
//! | 坐标 | 鼠标真正所在格；不在图上为 `—` | — |
//! | 库数 | `● 资源库 117 个`（失败时如实 0）| — |
//!
//! 各段宽度都是**量出来的**（动态段按测量宽让位），所以必须先有文案再算锚点
//! —— 这也是为什么 [`anchors`] 要接收 `lib_text` 而不是读常量。

use egui::{pos2, Align2, Painter, Rect};

use crate::layout::Layout;
use crate::metrics as m;
use crate::paint as pt;
use crate::screen_text::DocText;
use crate::text_fit::{DynLabel, Label, LabelStyle};
use crate::theme as t;
use crate::ui_text as tx;

/// 状态栏的几何关键点：`(库状态文字左边, 圆点中心 x, 坐标文字右边界)`。
///
/// 右侧三段是**从右往左**摆的（库状态右对齐 → 圆点 → 坐标右对齐），
/// 所以必须先量出库状态文字宽。绘制与验收共用这一份计算。
fn anchors(p: &Painter, l: &Layout, lib_text: &str) -> (f32, f32, f32) {
    let inner = l.status_inner;
    let lib_w = pt::text_w(p, lib_text, m::STATUS_FONT);
    let lib_left = inner.right() - lib_w;
    let dot_cx = lib_left - m::STATUS_DOT_GAP - m::STATUS_DOT_D * 0.5;
    let coord_right = dot_cx - m::STATUS_DOT_D * 0.5 - m::STATUS_COORD_GAP;
    (lib_left, dot_cx, coord_right)
}

/// 状态栏**没有静态文字**了（三段全是运行期数据），保留空实现是为了让
/// [`crate::text_fit::all_labels`] 的签名与其它模块保持一致。
pub fn labels(_p: &Painter, _l: &Layout) -> Vec<Label> {
    Vec::new()
}

/// 状态栏五段（自左向右：左段 / 工具 / **当前层** / 笔刷 / **落笔字段**）+ 右对齐两段（坐标 / 库数）。
///
/// 排布顺序：左段贴左内边距；左段真实文字之后依次排「工具」「当前层」「笔刷」「落笔」四段
/// （各自按测量宽度让位）；坐标段与库数段仍从右往左算（见 [`anchors`]）。
/// 七段**各有独立裁剪盒**，互不覆盖 —— 单元测试 [`七段互不重叠`] 会逐条核对。
pub fn dyn_labels(p: &Painter, l: &Layout, d: &DocText) -> Vec<DynLabel> {
    let inner = l.status_inner;
    let cy = inner.center().y;
    let (lib_left, _dot_cx, coord_right) = anchors(p, l, &d.status_lib);
    let mid_x = inner.center().x;

    // 坐标文字的真实左边界（按测量宽算）：落笔段的裁剪盒不得越过它
    let coord_w = pt::text_w(p, &d.status_coord, m::STATUS_FONT);
    let coord_left = coord_right - coord_w;

    // 左段的**真实**占用右边界（左段自己有 mid_x 的裁剪盒，不许越过）
    let left_w = pt::text_w(p, &d.status_left, m::STATUS_FONT);
    let left_end0 = (inner.left() + left_w).min(mid_x);

    // ⓪ **磁盘格式段**（`格式：Shanda14（14B/格，TNewMapInfo）`）：紧跟左段之后、工具段之前。
    //
    //    **本版新增**（2026-11-05 九格式收口）：左段的 `{w}×{h} 格` 是**视图**口径
    //    （三档投影），九种格式里有五种在磁盘上根本不是 12/14/36 —— 格式必须单列一句，
    //    否则用户会以为"我这张 Type5 存下去就是 New14"。
    //
    //    `status_format` 为空 = 没有打开地图 ⇒ **整段不存在**（既有七段布局逐字节不变，
    //    所以 `statusbar` 自己的七段几何测试不受影响）。
    //    与左段同一条约束：不得越过中线（右半幅是工具/当前层/笔刷/落笔/坐标/库状态）；
    //    **放不下就整段不画**（与填充回执/印章/选区/回执同一条纪律）——
    //    绝不留一条 1px 宽的碎条（那会被截成 `…`，比不画更糟）。
    let mut fmt_seg: Option<DynLabel> = None;
    let mut left_end = left_end0;
    if !d.status_format.is_empty() {
        let x = left_end0 + m::STATUS_SEG_GAP;
        let w = pt::text_w(p, &d.status_format, m::STATUS_FONT);
        if x + w <= mid_x {
            fmt_seg = Some(DynLabel::new(
                "status.format",
                tx::STATUS_FORMAT_TMPL,
                d.status_format.clone(),
                LabelStyle::new(
                    m::STATUS_FONT,
                    pos2(x, cy),
                    Align2::LEFT_CENTER,
                    Rect::from_min_max(pos2(x, inner.top()), pos2(x + w, inner.bottom())),
                    t::ui().accent_dark,
                ),
            ));
            left_end = x + w;
        }
    }
    let tool_x = left_end + m::STATUS_SEG_GAP;
    let tool_w = pt::text_w(p, &d.status_tool, m::STATUS_FONT);
    let layer_x = tool_x + tool_w + m::STATUS_SEG_GAP;
    let layer_w = pt::text_w(p, &d.status_layer, m::STATUS_FONT);
    let brush_x = layer_x + layer_w + m::STATUS_SEG_GAP;
    let brush_w = pt::text_w(p, &d.status_brush, m::STATUS_FONT);
    let mut hint_x = brush_x + brush_w + m::STATUS_SEG_GAP;

    // **可选段的预算（2026-11-05）**
    //
    // 中间那一串（填充回执 / 印章 / 选区 / 回执）谁都可以"放不下就不画"，
    // 但**优先级不同**：`status.op` 是"刚完成的那个破坏性动作的回执"（放置印章 / 剪切 /
    // 粘贴），它必须能显示 —— 否则用户放完印章看不到任何确认，会以为没生效。
    //
    // 所以先量出它的宽度当**预留**，其余三段只能用剩下的地方：
    // 每一段的判据从 `hint_x + w + gap + hint_w ≤ coord_left - gap`
    // 收紧成 `hint_x + w + gap + 预留 + hint_w ≤ coord_left - gap`。
    // 预留为 0（没有回执 / 回执自己都放不下）时与旧行为逐字相同。
    //
    // ⚠ 回执段自己用**放宽**的判据（只要求不与坐标段重叠）：
    // 它一旦画不出来，用户就完全没有"放成功了"的确认；而「落笔」段被挤窄只是被截断成
    // `落笔 bk_img（本图 12B…`，信息仍在（完整原文另在 stderr 的诊断行里）。
    let hint_w = pt::text_w(p, &d.status_hint, m::STATUS_FONT);
    let op_reserve = match d.status_op.as_deref() {
        Some(text) => {
            let w = pt::text_w(p, text, m::STATUS_FONT);
            if hint_x + w <= coord_left - m::STATUS_SEG_GAP {
                w + m::STATUS_SEG_GAP
            } else {
                0.0
            }
        }
        None => 0.0,
    };

    // ⑧ **填充回执**（`填充 1234 格`）：只有填过才有这一段，夹在「笔刷」与「落笔」之间。
    //    官方把这句话写在状态栏上（`MapViewport.cs:537`），本仓给它**独立一段** ——
    //    混进「落笔」段会把"要写哪些字段"顶掉，而那是另一件事。
    //
    //    ⚠ **放不下就不画这一段**（而不是把它挤成 `…`）：落笔段写的是"这一笔会改哪些字节"，
    //    比一句回执重要得多。窗口够宽（1500 的常态）时它一定在；只有极端长文案
    //    （超长报错 + 最大图号 + 六位格数同时出现）才会让位。
    let mut fill_seg: Option<DynLabel> = None;
    if let Some(text) = d.status_fill.as_deref() {
        let w = pt::text_w(p, text, m::STATUS_FONT);
        if hint_x + w + m::STATUS_SEG_GAP + op_reserve + hint_w
            <= coord_left - m::STATUS_SEG_GAP
        {
            let right = (hint_x + w).max(hint_x + 1.0);
            fill_seg = Some(DynLabel::new(
                "status.fill",
                tx::STATUS_FILL_TMPL,
                text.to_owned(),
                LabelStyle::new(
                    m::STATUS_FONT,
                    pos2(hint_x, cy),
                    Align2::LEFT_CENTER,
                    Rect::from_min_max(pos2(hint_x, inner.top()), pos2(right, inner.bottom())),
                    t::ui().accent_dark,
                ),
            ));
            hint_x += w + m::STATUS_SEG_GAP;
        }
    }
    // 落笔段右边界 = 坐标文字左边再让出一个间距（放不下就靠截断，绝不压到坐标上）
    let seg_right = (coord_left - m::STATUS_SEG_GAP).max(hint_x + 1.0);

    // ⑨ **印章段**（`印章：<名称>`）：选中一枚印章后才出现，插在「笔刷」之后、「落笔」之前。
    //
    //    与「填充回执」同一条纪律：**放不下就不画这一段**（而不是把落笔段挤成 `…`）——
    //    「这一笔会改哪些字节」永远比"我刚点了一枚印章"重要。
    let mut stamp_seg: Option<DynLabel> = None;
    if let Some(text) = d.status_stamp.as_deref() {
        let w = pt::text_w(p, text, m::STATUS_FONT);
        if hint_x + w + m::STATUS_SEG_GAP + op_reserve + hint_w
            <= coord_left - m::STATUS_SEG_GAP
        {
            let right = (hint_x + w).max(hint_x + 1.0);
            stamp_seg = Some(DynLabel::new(
                "status.stamp",
                tx::STAMP_STATUS_TMPL,
                text.to_owned(),
                LabelStyle::new(
                    m::STATUS_FONT,
                    pos2(hint_x, cy),
                    Align2::LEFT_CENTER,
                    Rect::from_min_max(pos2(hint_x, inner.top()), pos2(right, inner.bottom())),
                    t::ui().text,
                ),
            ));
            hint_x += w + m::STATUS_SEG_GAP;
        }
    }

    // ⑩ **选区 / 剪贴板段**（`选区 3×3（200,200..202,202）· 剪贴板 3×3`）。
    //
    //    这是**纯 UI 状态**（内核保证不置脏、不进撤销栈），每帧从会话现算。
    //    仍然遵守"放不下就不画"：它是一句"当前状态"，比不上「落笔」那一段重要。
    let mut sel_seg: Option<DynLabel> = None;
    if let (Some(text), Some(tmpl)) = (d.status_sel.as_deref(), d.status_sel_tmpl) {
        let w = pt::text_w(p, text, m::STATUS_FONT);
        if hint_x + w + m::STATUS_SEG_GAP + op_reserve + hint_w
            <= coord_left - m::STATUS_SEG_GAP
        {
            let right = (hint_x + w).max(hint_x + 1.0);
            sel_seg = Some(DynLabel::new(
                "status.sel",
                tmpl,
                text.to_owned(),
                LabelStyle::new(
                    m::STATUS_FONT,
                    pos2(hint_x, cy),
                    Align2::LEFT_CENTER,
                    Rect::from_min_max(pos2(hint_x, inner.top()), pos2(right, inner.bottom())),
                    t::ui().selection_stroke,
                ),
            ));
            hint_x += w + m::STATUS_SEG_GAP;
        }
    }

    // ⑪ **上一次操作回执段**（`放置印章「mir_00000」8 格` / `剪切 3×3 格（清空 9 格）`）。
    //
    //    排在「落笔」之前、选区之后；拒绝时用警示粉（与「笔刷族不匹配」同一档），
    //    成功时用强调金 —— 成功也报红就是假警报。
    //
    //    ⚠ 判据是**放宽**的那一条（不与坐标段重叠即可，不要求「落笔」段还完整）：
    //    见上面 `op_reserve` 的注释 —— 回执画不出来的代价比「落笔」段被截断大得多。
    let mut op_seg: Option<DynLabel> = None;
    if let Some(text) = d.status_op.as_deref() {
        let w = pt::text_w(p, text, m::STATUS_FONT);
        if hint_x + w <= coord_left - m::STATUS_SEG_GAP {
            let right = (hint_x + w).max(hint_x + 1.0);
            op_seg = Some(DynLabel::new(
                "status.op",
                tx::STATUS_OP_TMPL,
                text.to_owned(),
                LabelStyle::new(
                    m::STATUS_FONT,
                    pos2(hint_x, cy),
                    Align2::LEFT_CENTER,
                    Rect::from_min_max(pos2(hint_x, inner.top()), pos2(right, inner.bottom())),
                    if d.status_op_warn {
                    t::ui().collision_low_mark
                } else {
                    t::ui().accent_dark
                },
                ),
            ));
            hint_x += w + m::STATUS_SEG_GAP;
        }
    }

    let mut out = vec![
        DynLabel::new(
            "status.left",
            tx::STATUS_LEFT_TMPL,
            d.status_left.clone(),
            LabelStyle::new(
                m::STATUS_FONT,
                pos2(inner.left(), cy),
                Align2::LEFT_CENTER,
                // 只占左半幅：右半幅是工具/当前层/笔刷/落笔/坐标/库状态，别把它们的墨迹算到左段头上
            Rect::from_min_max(pos2(inner.left(), inner.top()), pos2(mid_x, inner.bottom())),
                t::ui().text,
            ),
        ),
        DynLabel::new(
            "status.tool",
            tx::STATUS_TOOL_TMPL,
            d.status_tool.clone(),
            LabelStyle::new(
                m::STATUS_FONT,
                pos2(tool_x, cy),
                Align2::LEFT_CENTER,
                Rect::from_min_max(
                pos2(tool_x, inner.top()),
                pos2(layer_x - m::STATUS_SEG_GAP, inner.bottom()),
            ),
                t::ui().text,
            ),
        ),
        DynLabel::new(
            "status.layer",
            tx::STATUS_LAYER_TMPL,
            d.status_layer.clone(),
            LabelStyle::new(
                m::STATUS_FONT,
                pos2(layer_x, cy),
                Align2::LEFT_CENTER,
                Rect::from_min_max(
                pos2(layer_x, inner.top()),
                pos2(brush_x - m::STATUS_SEG_GAP, inner.bottom()),
            ),
                // 当前层是"破坏性信息"：用强调金，比正文更显眼（复用 theme 现有色）
            t::ui().accent_dark,
            ),
        ),
        DynLabel::new(
            "status.brush",
            tx::STATUS_BRUSH_TMPL,
            d.status_brush.clone(),
            LabelStyle::new(
                m::STATUS_FONT,
                pos2(brush_x, cy),
                Align2::LEFT_CENTER,
                Rect::from_min_max(
                pos2(brush_x, inner.top()),
                pos2(hint_x - m::STATUS_SEG_GAP, inner.bottom()),
            ),
                t::ui().text,
            ),
        ),
        DynLabel::new(
            "status.hint",
            tx::STATUS_WRITE_TMPL,
            d.status_hint.clone(),
            LabelStyle::new(
                m::STATUS_FONT,
                pos2(hint_x, cy),
                Align2::LEFT_CENTER,
                Rect::from_min_max(pos2(hint_x, inner.top()), pos2(seg_right, inner.bottom())),
                // 拒绝（族不匹配）用警示粉；正常态用次要色（它是"说明"，不是"主信息"）
            if d.status_hint_warn {
                t::ui().collision_low_mark
            } else {
                t::ui().text_muted
            },
            ),
        ),
        DynLabel::new(
            "status.coord",
            tx::STATUS_COORD_TMPL,
            d.status_coord.clone(),
            LabelStyle::new(
                m::STATUS_FONT,
                pos2(coord_right, cy),
                Align2::RIGHT_CENTER,
                Rect::from_min_max(
                pos2(inner.left(), inner.top()),
                pos2(coord_right + m::TEXT_CLIP_SLACK, inner.bottom()),
            ),
                t::ui().text,
            ),
        ),
        DynLabel::new(
            "status.lib",
            tx::STATUS_LIB_TMPL,
            d.status_lib.clone(),
            LabelStyle::new(
                m::STATUS_FONT,
                pos2(inner.right(), cy),
                Align2::RIGHT_CENTER,
                Rect::from_min_max(
                pos2(lib_left - m::TEXT_CLIP_SLACK, inner.top()),
                pos2(inner.right() + m::TEXT_CLIP_SLACK, inner.bottom()),
            ),
                t::ui().text,
            ),
        ),
    ];
    // 可选段按固定次序插在「笔刷」之后、「落笔」之前：
    //   填充回执 → 印章 → 选区/剪贴板 → 上一次操作回执。
    //
    // 用**递增的插入位**而不是"按下标硬算"：每一段都可能因为放不下而**没被创建**
    // （上面三个 `if` 里的宽度检查），硬算下标会让"没创建"的那一段把后面的段挤到「落笔」之后。
    //
    // ⓪ **磁盘格式段**固定插在下标 1（紧跟「左段」、在「工具」之前）—— 它是左段的补充说明，
    //    不是"可选回执"，所以不参与上面那串递增插入位。
    if let Some(seg) = fmt_seg {
        out.insert(1, seg);
    }
    let mut at = 4;
    if let Some(seg) = fill_seg {
        out.insert(at, seg);
        at += 1;
    }
    if let Some(seg) = stamp_seg {
        out.insert(at, seg);
        at += 1;
    }
    if let Some(seg) = sel_seg {
        out.insert(at, seg);
        at += 1;
    }
    if let Some(seg) = op_seg {
        out.insert(at, seg);
    }
    out
}

/// 画状态栏。
pub fn draw(p: &Painter, l: &Layout, d: &DocText) {
    // 上沿 1px 分隔线（截图 y=912 为 `#EAECEF`）
    pt::fill(p, l.status_sep, t::ui().separator);
    // 29px 内容底
    pt::fill(
        p,
        Rect::from_min_max(
            pos2(l.status_bar.left(), l.status_sep.bottom()),
            l.status_bar.max,
        ),
        t::ui().status_bar,
    );

    let (_lib_left, dot_cx, _coord_right) = anchors(p, l, &d.status_lib);

    // 圆点：库状态文字左边 STATUS_DOT_GAP 处，直径 6
    p.circle_filled(
        pos2(dot_cx, l.status_inner.center().y),
        m::STATUS_DOT_D * 0.5,
        t::ui().status_dot,
    );

    for mut lab in dyn_labels(p, l, d) {
        lab.truncate_to_fit(p);
        lab.draw(p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas_view::CanvasView;
    use crate::cellinfo::CellLines;
    use crate::fonts;
    use crate::mapdoc::{Assets, DocState};

    fn 文案(lib: &str, coord: &str, left: &str) -> DocText {
        DocText {
            title: String::new(),
            tab: String::new(),
            pill: tx::fill(tx::PILL_SIZE_TMPL, &[("n", "3")]),
            brush_size: 3,
            status_left: left.to_string(),
            status_tool: tx::fill(tx::STATUS_TOOL_TMPL, &[("tool", tx::TOOL_BRUSH)]),
            status_layer: tx::fill(tx::STATUS_LAYER_TMPL, &[("layer", tx::LAYER_GROUND)]),
            status_brush: tx::fill(
                tx::STATUS_BRUSH_TMPL,
                &[("fam", tx::LAYER_GROUND), ("img", "901"), ("lib", "4")],
            ),
            status_hint: tx::fill(
                tx::STATUS_WRITE_TMPL,
                &[("img", "bk_img"), ("lib", "btTiles")],
            ),
            status_hint_warn: false,
            status_coord: coord.to_string(),
            status_lib: lib.to_string(),
            status_format: String::new(),
            banner: None,
            ground_locked: false,
        status_stamp: None,
            status_fill: None,
            status_op: None,
            status_op_warn: false,
            status_sel: None,
            status_sel_tmpl: None,
            zoom: String::new(),
            page: String::new(),
            inspector_head: String::new(),
            inspector_values: Default::default(),
        }
    }

    fn 窗口() -> Layout {
        Layout::compute(Rect::from_min_size(
            pos2(0.0, 0.0),
            egui::vec2(m::WINDOW_W, m::WINDOW_H),
        ))
    }

    /// 右段宽度变化时，坐标段必须让位（不能与库状态文字叠在一起）。
    #[test]
    fn 库数变长时坐标段让位() {
        let ctx = fonts::measure_ctx();
        let p = Painter::new(
            ctx,
            egui::LayerId::debug(),
            Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(m::WINDOW_W, m::WINDOW_H)),
        );
        let l = 窗口();
        let a = dyn_labels(&p, &l, &文案("资源库 0 个", "—", "0.map · 700×700 格"));
        let b = dyn_labels(&p, &l, &文案("资源库 65535 个", "—", "0.map · 700×700 格"));
        let ax = a.iter().find(|x| x.name == "status.coord").unwrap().pos.x;
        let bx = b.iter().find(|x| x.name == "status.coord").unwrap().pos.x;
        assert!(bx < ax, "库数变长后坐标段应左移：{bx} 应 < {ax}");
        // 坐标段右边 + 间距 + 圆点 + 间距 = 库状态左边
        assert!(bx < m::WINDOW_W);
    }

    /// 三段不重叠：左段裁剪盒右边界 < 坐标文字左边界，坐标文字右边界 < 库状态文字左边界。
    #[test]
    fn 三段不重叠() {
        let ctx = fonts::measure_ctx();
        let p = Painter::new(
            ctx,
            egui::LayerId::debug(),
            Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(m::WINDOW_W, m::WINDOW_H)),
        );
        let l = 窗口();
        let d = 文案("资源库 117 个", "单元格 380,310", "0.map · 700×700 格");
        let labs = dyn_labels(&p, &l, &d);
        let coord_w = crate::paint::text_w(&p, &d.status_coord, m::STATUS_FONT);
        let lib_w = crate::paint::text_w(&p, &d.status_lib, m::STATUS_FONT);
        let coord = labs.iter().find(|x| x.name == "status.coord").unwrap();
        let lib = labs.iter().find(|x| x.name == "status.lib").unwrap();
        let left = labs.iter().find(|x| x.name == "status.left").unwrap();
        let coord_left = coord.pos.x - coord_w;
        let lib_left = lib.pos.x - lib_w;
        assert!(
            left.clip.right() < coord_left,
            "左段裁剪盒不得越过坐标文字左边：{} vs {coord_left}",
            left.clip.right()
        );
        assert!(
            coord.pos.x < lib_left,
            "坐标文字右边不得越过库状态文字左边：{} vs {lib_left}",
            coord.pos.x
        );
    }

    /// 空状态：坐标是 `—`，也能正常排出（不崩）。
    #[test]
    fn 空状态可排出() {
        let ctx = fonts::measure_ctx();
        let p = Painter::new(
            ctx,
            egui::LayerId::debug(),
            Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(m::WINDOW_W, m::WINDOW_H)),
        );
        let l = 窗口();
        let doc = DocState {
            doc: None,
            assets: Assets::Failed("x".into()),
            map_error: None,
            map_gen: 1,
        };
        let d = crate::screen_text::build(
            &doc,
            &CanvasView::default(),
            None,
            &CellLines::unknown(),
            &crate::edit::EditSession::default(),
        );
        let labs = dyn_labels(&p, &l, &d);
        assert_eq!(labs.len(), 7, "状态栏本版起是七段（左/工具/当前层/笔刷/落笔/坐标/库数）");
        assert_eq!(
            labs.iter().find(|x| x.name == "status.coord").unwrap().text,
            "—"
        );
        assert_eq!(
            labs.iter().find(|x| x.name == "status.brush").unwrap().text,
            tx::STATUS_BRUSH_NONE
        );
        // 没打开地图时：当前层仍如实写（缺省地表图层），落笔字段如实说"未打开地图"
        assert_eq!(
            labs.iter().find(|x| x.name == "status.layer").unwrap().text,
            tx::fill(tx::STATUS_LAYER_TMPL, &[("layer", tx::LAYER_GROUND)])
        );
        assert_eq!(
            labs.iter().find(|x| x.name == "status.hint").unwrap().text,
            tx::STATUS_HINT_NO_MAP
        );
    }

    /// **阻挡层的落笔位必须完整显示**（不许被截断）。
    ///
    /// 这条挡的是"状态栏说了一半"：`落笔 bk_img.0x8000（低位阻挡）· 置位=不可走`
    /// 比原来的 `落笔 bk_img + btTiles` 长得多，若放不下就会被 `…` 截断 ——
    /// 而"置位=不可走"正是用户唯一能看见方向的地方，截掉它等于没写。
    #[test]
    fn 阻挡层落笔位不被截断() {
        use crate::edit::EditLayer;
        let ctx = fonts::measure_ctx();
        let p = Painter::new(
            ctx,
            egui::LayerId::debug(),
            Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(m::WINDOW_W, m::WINDOW_H)),
        );
        let l = 窗口();
        // ⚠ 2026-11-05：`MapDoc` 多了私有的 `document`（字节层真值）⇒ 字面量构造不再可能，
        // 统一走 `MapDoc::from_bytes`（与生产路径同一份判别）。
        let doc = DocState {
            doc: Some(
                crate::mapdoc::MapDoc::from_bytes(
                    std::path::Path::new(r"E:\x\0.map"),
                    {
                        let mut b = vec![0u8; 52 + 12 * 4];
                        b[0..2].copy_from_slice(&2u16.to_le_bytes());
                        b[2..4].copy_from_slice(&2u16.to_le_bytes());
                        b
                    },
                )
                .expect("2×2 合成图应能解析"),
            ),
            assets: Assets::Failed("x".into()),
            map_error: None,
            map_gen: 1,
        };
        for layer in [EditLayer::LowCollision, EditLayer::HighCollision] {
            let mut ed = crate::edit::EditSession::default();
            ed.set_layer(layer);
            let d = crate::screen_text::build(
                &doc,
                &CanvasView::default(),
                Some((380, 310)),
                &CellLines::unknown(),
                &ed,
            );
            let mut labs = dyn_labels(&p, &l, &d);
            for lab in labs.iter_mut() {
                lab.truncate_to_fit(&p);
            }
            let hint = labs
                .iter()
                .find(|x| x.name == "status.hint")
                .expect("status.hint");
            assert!(
                !hint.truncated,
                "{layer:?} 的落笔位被截断成 {:?}（必须完整显示）",
                hint.text
            );
            assert!(hint.text.contains("置位=不可走"), "方向说明不许被截掉");
            assert!(hint.text.contains("0x8000"), "位名不许被截掉");
        }
    }

    /// **七段互不重叠**：逐段核对「前一段的真实文字右边界 ≤ 后一段的左边界」。
    #[test]
    fn 七段互不重叠() {        let ctx = fonts::measure_ctx();
        let p = Painter::new(
            ctx,
            egui::LayerId::debug(),
            Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(m::WINDOW_W, m::WINDOW_H)),
        );
        let l = 窗口();
        let d = 文案("资源库 117 个", "单元格 380,310", "edit-0.map · 700×700 格");
        let labs = dyn_labels(&p, &l, &d);
        let get = |n: &str| labs.iter().find(|x| x.name == n).expect(n);
        let order = [
            "status.left",
            "status.tool",
            "status.layer",
            "status.brush",
            "status.hint",
        ];
        // 相邻两段：前一段的文字右边界 ≤ 后一段的左边界
        for w in order.windows(2) {
            let a = get(w[0]);
            let b = get(w[1]);
            let a_right = a.pos.x + crate::paint::text_w(&p, &a.text, m::STATUS_FONT);
            assert!(
                a_right <= b.pos.x,
                "{} 的文字右边界 {a_right} 越过了 {} 的左边 {}",
                w[0],
                w[1],
                b.pos.x
            );
        }
        // 落笔段文字右边界 ≤ 坐标段文字左边界
        let hint = get("status.hint");
        let hint_right = hint.pos.x + crate::paint::text_w(&p, &hint.text, m::STATUS_FONT);
        let coord = get("status.coord");
        let coord_left = coord.pos.x - crate::paint::text_w(&p, &coord.text, m::STATUS_FONT);
        assert!(
            hint_right <= coord_left,
            "落笔段文字右边界 {hint_right} 越过了坐标段左边 {coord_left}"
        );
        // 坐标段右边界 ≤ 库状态文字左边
        let lib = get("status.lib");
        let lib_left = lib.pos.x - crate::paint::text_w(&p, &lib.text, m::STATUS_FONT);
        assert!(coord.pos.x <= lib_left, "坐标段不得压到库状态上");
        // 各段裁剪盒都必须比自己的文字宽（说明没被截断）
        for (name, lab) in [
            ("status.tool", get("status.tool")),
            ("status.layer", get("status.layer")),
            ("status.brush", get("status.brush")),
            ("status.hint", get("status.hint")),
        ] {
            let w = crate::paint::text_w(&p, &lab.text, m::STATUS_FONT);
            assert!(
                lab.clip.right() + 0.5 >= lab.pos.x + w,
                "{name} 的裁剪盒 {} 比文字右边界 {} 还窄（会被截断）",
                lab.clip.right(),
                lab.pos.x + w
            );
        }
    }

    /// **填充回执是独立一段**（本版新增）：有回执时状态栏是 8 段、没回执时 7 段；
    /// 有回执时它必须夹在「笔刷」与「落笔」之间，且**不与任何一段重叠**。
    #[test]
    fn 填充回执独立成段不重叠() {
        let ctx = fonts::measure_ctx();
        let p = Painter::new(
            ctx,
            egui::LayerId::debug(),
            Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(m::WINDOW_W, m::WINDOW_H)),
        );
        let l = 窗口();
        let mut d = 文案("资源库 117 个", "单元格 380,310", "0.map · 700×700 格");
        // 没填过：7 段
        assert_eq!(dyn_labels(&p, &l, &d).len(), 7, "没填过不该多出一段");
        // 填过：8 段，且 status.fill 在 status.brush 之后、status.hint 之前
        d.status_fill = Some(tx::fill(tx::STATUS_FILL_TMPL, &[("n", "1234")]));
        let labs = dyn_labels(&p, &l, &d);
        assert_eq!(labs.len(), 8, "填过之后应是 8 段");
        let names: Vec<&str> = labs.iter().map(|x| x.name).collect();
        assert_eq!(
            names,
            vec![
                "status.left",
                "status.tool",
                "status.layer",
                "status.brush",
                "status.fill",
                "status.hint",
                "status.coord",
                "status.lib",
            ]
        );
        let get = |n: &str| labs.iter().find(|x| x.name == n).expect(n);
        assert_eq!(get("status.fill").text, "填充 1234 格");
        // 相邻不重叠：笔刷右边界 ≤ 填充左边；填充右边界 ≤ 落笔左边
        let brush = get("status.brush");
        let fill = get("status.fill");
        let brush_right = brush.pos.x + crate::paint::text_w(&p, &brush.text, m::STATUS_FONT);
        assert!(brush_right <= fill.pos.x, "填充段压到了笔刷段上");
        let fill_right = fill.pos.x + crate::paint::text_w(&p, &fill.text, m::STATUS_FONT);
        assert!(
            fill_right <= get("status.hint").pos.x,
            "填充段压到了落笔段上：{fill_right} vs {}",
            get("status.hint").pos.x
        );
        // 它自己的裁剪盒放得下自己的字（不会被截成 `…`）
        assert!(fill.clip.right() + 0.5 >= fill_right);
    }

    /// **放不下就不画填充段**：极端长文案（超长报错 + 六位格数）下，落笔段优先保住。
    #[test]
    fn 填充段放不下时让位给落笔段() {
        let ctx = fonts::measure_ctx();
        let p = Painter::new(
            ctx,
            egui::LayerId::debug(),
            Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(m::WINDOW_W, m::WINDOW_H)),
        );
        let l = 窗口();
        let mut d = 文案(
            "资源库 65535 个",
            "单元格 65535,65535",
            &tx::fill(tx::STATUS_MAP_FAIL_TMPL, &[("err", &"有".repeat(200))]),
        );
        d.status_brush = tx::fill(
            tx::STATUS_BRUSH_TMPL,
            &[("fam", tx::LAYER_OBJECTS), ("img", "65535"), ("lib", "255")],
        );
        d.status_fill = Some(tx::fill(tx::STATUS_FILL_TMPL, &[("n", "200000")]));
        let labs = dyn_labels(&p, &l, &d);
        assert!(
            labs.iter().all(|x| x.name != "status.fill"),
            "挤不下时填充段必须整段不画（不是压扁落笔段）"
        );
        // 落笔段仍然有地方（裁剪盒不是 1px）
        let hint = labs.iter().find(|x| x.name == "status.hint").expect("hint");
        assert!(
            hint.clip.width() > 10.0,
            "落笔段被挤成 {}px 了",
            hint.clip.width()
        );
    }

    /// **回执段优先于印章段 / 选区段**（2026-11-05 的实跑缺陷回归门）。
    ///
    /// 实测原文：放完印章后截图里状态栏只剩 `印章：Market stall - mir_00000`，
    /// **回执 `放置印章「…」8 格` 整段没画** —— 用户看不到任何"放成功了"的确认。
    /// 根因：四段可选段按"放不下就不画"逐段让位，谁先来谁占地方，而回执排在最后。
    ///
    /// 现在的契约：回执段是"刚完成的破坏性动作的确认"，其余三段只能用**剩下的**地方。
    /// 这条用例用**极端长的印章名**把空间压到只够一段，断言活下来的是回执段。
    #[test]
    fn 回执段优先于印章段() {
        let ctx = fonts::measure_ctx();
        let p = Painter::new(
            ctx,
            egui::LayerId::debug(),
            Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(m::WINDOW_W, m::WINDOW_H)),
        );
        let l = 窗口();
        let mut d = 文案("资源库 117 个", "单元格 380,310", "0.map · 700×700 格");
        // 极端长的印章名（真实数据里最长的也到不了 40 字）
        d.status_stamp = Some(tx::fill(tx::STAMP_STATUS_TMPL, &[("name", &"长".repeat(40))]));
        d.status_op = Some(tx::fill(
            tx::STAMP_PLACE_OK_TMPL,
            &[("name", "mir_00000"), ("n", "8")],
        ));
        let labs = dyn_labels(&p, &l, &d);
        let op = labs
            .iter()
            .find(|x| x.name == "status.op")
            .expect("回执段必须能显示（放完印章看不到确认 = 假成功）");
        assert_eq!(op.text, "放置印章「mir_00000」8 格");
        assert!(
            labs.iter().all(|x| x.name != "status.stamp"),
            "空间不够时让位的是印章段，不是回执段"
        );
        // 回执段不得压到落笔段 / 坐标段上
        let hint = labs.iter().find(|x| x.name == "status.hint").expect("hint");
        let op_right = op.pos.x + crate::paint::text_w(&p, &op.text, m::STATUS_FONT);
        assert!(op_right <= hint.pos.x, "回执段压到了落笔段上：{op_right} vs {}", hint.pos.x);
        let coord = labs.iter().find(|x| x.name == "status.coord").expect("coord");
        let coord_left = coord.pos.x - crate::paint::text_w(&p, &coord.text, m::STATUS_FONT);
        let hint_right = hint.pos.x + crate::paint::text_w(&p, &hint.text, m::STATUS_FONT);
        assert!(hint_right <= coord_left, "落笔段越过了坐标段");
    }

    /// 拒绝回执用警示色、成功回执用强调色（成功也报红就是假警报）。
    #[test]
    fn 回执段拒绝与成功分色() {
        let ctx = fonts::measure_ctx();
        let p = Painter::new(
            ctx,
            egui::LayerId::debug(),
            Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(m::WINDOW_W, m::WINDOW_H)),
        );
        let l = 窗口();
        for (warn, want) in [(false, t::ui().accent_dark), (true, t::ui().collision_low_mark)] {
            let mut d = 文案("资源库 117 个", "—", "0.map · 700×700 格");
            d.status_op = Some("放置失败：Mir3Wemade 不支持逐格写地表背图".to_string());
            d.status_op_warn = warn;
            let labs = dyn_labels(&p, &l, &d);
            let op = labs.iter().find(|x| x.name == "status.op").expect("op");
            assert_eq!(op.color, want, "warn={warn} 时的颜色不对");
        }
    }
}
