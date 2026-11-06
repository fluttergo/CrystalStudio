//! **常驻告警黄条**：把 `miredit_core::Document::edit_warning()` 的原文钉在画布顶部。
//!
//! # 一、为什么是"常驻"，不是 toast
//!
//! 黄条说的是**这张图本身的性质**，不是"刚才那一下失败了"：
//!
//! | 来源 | 判据 | 真实例子 |
//! |---|---|---|
//! | 未解尾块 | `Document::tolerant_trailer() > 0` | `EM001.map` 等 **67 张**，全部 `8040` 字节 |
//! | 格式未承诺编辑语义 | `caps().editable == false`（**逐格式**，`editable=false` 的格式带各自的 `editable_caveat`）| Type5 / Type1 / Type4 / Mir3 / CrystalCs … |
//!
//! ⚠ **2026-11-05 订正（旧注释已过期）**：本文件上一版写的是「九种里只有 `Shanda14` 为真」。
//! 现在 `Old12` 也是 `editable = true`（`mir2-format` 的 `0507d75f` 口径重推），
//! 而 `editable = false` 的格式**各带一段具体 caveat**（`Shanda2012` 那段约 250 字）。
//! ⇒ 黄条不再是一句 60 字的套话，**必须能折行**（见 §三）。
//!
//! 用户画一整天它都在。做成 toast（几秒消失）等于让用户错过它 —— 而它描述的正是
//! "你改的东西可能不会按你想的那样联动"。
//!
//! 口径与网页版一致（`miredit-web` 的 `routes/map.rs` 把 `edit_warning` 原样放进 JSON，
//! 前端整句上屏、不改写）；本模块同样**一个字符都不加工**（只按宽度折行）。
//!
//! # 二、为什么不进状态栏
//!
//! 状态栏已经有八段（左 / 工具 / 当前层 / 笔刷 / 填充 / 印章 / 选区 / 回执 / 落笔 / 坐标 / 库数），
//! 每段都有独立裁剪盒与"放不下就让位"的预算（见 [`crate::statusbar::dyn_labels`]）。
//! 一句 250 字的告警塞进去，不是被截成 `本图有 8040 字节未解释尾块（按…`，
//! 就是把「落笔」段挤没。**告警值得独占一条**。
//!
//! # 三、几何与**折行策略**（宿主新增，无截图依据）
//!
//! 参考截图是 CrystalStudio 的运行期界面，它**没有**"九格式 / 未解尾块"这件事，
//! 也就没有对应控件。这条黄条是按任务书 §三.2 新增的宿主 UI，几何取"画布顶部一条"：
//!
//! ```text
//! viewport ┌────────────────────────────────────────────┐
//!          │ ▌ 每格有 22 字节未建模尾区（格内 rel 14-35，编辑时  │ ← 单行 18 + 上下 8
//!          │ ▌ 原样保留、不随图号/光照更新）：其中 rel 14-15 …    │ ← 折行，高度自适应
//!          └────────────────────────────────────────────┘
//! ```
//!
//! 折行规则（[`wrap`]）：
//! 1. **按字符贪心**（中文没有词边界，按词折会得到参差的右边）；
//! 2. 硬换行 `\n` 原样保留（core 原文用 `；` 连接两段，不会带 `\n`，但将来可能带）；
//! 3. **绝不横向溢出**：每行宽 ≤ 文字可用宽（`text_width` 实测，不是估算）；
//! 4. **不缩字、不省略关键词**：高度不够时**先加高**，只有加到
//!    [`crate::metrics::BANNER_MAX_VIEWPORT_RATIO`] 上限还不够（只可能是"画布被压成一条"）
//!    才在**最后一行**加 [`crate::ui_text::TRUNC_SUFFIX`] 并把后续行丢掉 ——
//!    那时画布本身已经没地方放告警了。
//!
//! 配色见 [`crate::theme::Palette`] 的 `warn_banner_*`（浅琥珀底 + 金边 + 深褐字；
//! 深色模式换成官方深色板的琥珀边 + 亮字，见 `theme::DARK` 的注释）。

use egui::{pos2, vec2, Align2, Painter, Rect};

use crate::layout::Layout;
use crate::metrics as m;
use crate::paint as pt;
use crate::screen_text::DocText;
use crate::text_fit::{DynLabel, LabelStyle};
use crate::theme as t;
use crate::ui_text as tx;

/// 黄条与画布边缘的内缩（px）—— 四条边都缩这么多，避免贴死在纹理上。
pub const BANNER_INSET: f32 = 8.0;
/// 文字距黄条左内沿（px）：左侧还有 4px 的强调竖条，所以留 12。
pub const BANNER_TEXT_PAD: f32 = 12.0;
/// 文字距黄条右内沿（px）。
pub const BANNER_TEXT_PAD_R: f32 = 10.0;
/// 左侧强调竖条宽度（px）。
pub const BANNER_EDGE_W: f32 = 4.0;

/// 黄条的排版结果（矩形 + 折好的行 + 是否被迫截断）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Banner {
    /// 黄条矩形（高度随行数变化）。
    pub rect: Rect,
    /// 折好的每一行（**未经截断**时拼起来就是 core 原文）。
    pub lines: Vec<String>,
    /// 是否因为高度上限而丢掉了尾部（正常情况恒 `false`）。
    pub truncated: bool,
}

/// 文字可用宽度（= 黄条宽 - 左内沿 - 右内沿）。
fn text_width_avail(r: Rect) -> f32 {
    (r.right() - BANNER_TEXT_PAD_R).max(r.left() + BANNER_TEXT_PAD + 1.0)
        - (r.left() + BANNER_TEXT_PAD)
}

/// 折行缓存的键值：`(文案, 字号, 可用宽, 折好的行)`。
type WrapCacheEntry = (String, f32, f32, Vec<String>);

/// **按字符贪心折行**：每行实测宽 ≤ `max_w`；`\n` 硬断行。
///
/// 为什么按字符而不是按词：中文没有空格词边界，按词折会让右边参差；
/// 而本条的读者是"要看清 22 字节/原样保留这些关键词"的用户，**整齐的右边**比"按词"重要。
///
/// ⚠ **带单条记忆**（`WRAP_CACHE`）：本函数每帧都会被 `draw` + `dyn_labels` 各调一次，
/// 而贪心折行对每个前缀都要真排版一次（250 字 ≈ 250 次 `layout_no_wrap`，实测 3~8 ms）。
/// 告警文案只在**换图**时变，所以按 `(文案, 可用宽)` 记住上一次的结果，
/// 换图之外每帧只做一次字符串比较。缓存只有 1 条：告警永远只有一句。
pub fn wrap(p: &Painter, text: &str, font: f32, max_w: f32) -> Vec<String> {
    thread_local! {
        static WRAP_CACHE: std::cell::RefCell<Option<WrapCacheEntry>> =
            const { std::cell::RefCell::new(None) };
    }
    if let Some(lines) = WRAP_CACHE.with(|c| {
        c.borrow().as_ref().and_then(|(t, f, w, lines)| {
            if t == text && (*f - font).abs() < 0.01 && (*w - max_w).abs() < 0.5 {
                Some(lines.clone())
            } else {
                None
            }
        })
    }) {
        return lines;
    }
    let lines = wrap_uncached(p, text, font, max_w);
    WRAP_CACHE.with(|c| {
        *c.borrow_mut() = Some((text.to_owned(), font, max_w, lines.clone()));
    });
    lines
}

/// [`wrap`] 的实际实现（无缓存；单测直接测它，避免受缓存影响）。
pub fn wrap_uncached(p: &Painter, text: &str, font: f32, max_w: f32) -> Vec<String> {
    let mut out = Vec::new();
    for hard in text.split('\n') {
        let mut cur = String::new();
        for c in hard.chars() {
            let mut probe = cur.clone();
            probe.push(c);
            if !cur.is_empty() && pt::text_w(p, &probe, font) > max_w {
                out.push(std::mem::take(&mut cur));
                cur.push(c);
            } else {
                cur = probe;
            }
        }
        out.push(cur);
    }
    out
}

/// 算黄条的排版；**没有告警 / 画布太窄**时返回 `None`（不占位、不画空条）。
pub fn layout(p: &Painter, l: &Layout, d: &DocText) -> Option<Banner> {
    let text = d.banner.as_deref()?;
    let vp = l.viewport;
    let w = vp.width() - 2.0 * BANNER_INSET;
    let max_h = (vp.height() - 2.0 * BANNER_INSET).min(vp.height() * m::BANNER_MAX_VIEWPORT_RATIO);
    if w < 2.0 * (BANNER_TEXT_PAD + BANNER_TEXT_PAD_R) || max_h < m::BANNER_LINE_H {
        return None;
    }
    let rect_w = Rect::from_min_size(
        pos2(vp.left() + BANNER_INSET, vp.top() + BANNER_INSET),
        vec2(w, m::BANNER_LINE_H),
    );
    let avail = text_width_avail(rect_w);
    let mut lines = wrap(p, text, m::BODY_FONT, avail);
    // 高度 = 上下内边距 + 行数 × 行高；超上限就丢尾行并在最后一行加省略号
    let fit_lines = (((max_h - 2.0 * m::BANNER_PAD_Y) / m::BANNER_LINE_H).floor() as usize).max(1);
    let mut truncated = false;
    if lines.len() > fit_lines {
        lines.truncate(fit_lines);
        if let Some(last) = lines.last_mut() {
            last.push_str(tx::TRUNC_SUFFIX);
        }
        truncated = true;
    }
    let h = (2.0 * m::BANNER_PAD_Y + lines.len() as f32 * m::BANNER_LINE_H).min(max_h);
    Some(Banner {
        rect: Rect::from_min_size(rect_w.min, vec2(w, h)),
        lines,
        truncated,
    })
}

/// 黄条的矩形；**没有告警时返回 `None`**（不占位、不画空条）。
pub fn rect(p: &Painter, l: &Layout, d: &DocText) -> Option<Rect> {
    layout(p, l, d).map(|b| b.rect)
}

/// 每一行的文字裁剪盒（绘制与 [`dyn_labels`] 共用同一份计算）。
fn line_clip(r: Rect, i: usize) -> Rect {
    let top = r.top() + m::BANNER_PAD_Y + i as f32 * m::BANNER_LINE_H;
    Rect::from_min_max(
        pos2(r.left() + BANNER_TEXT_PAD, top),
        pos2(
            (r.right() - BANNER_TEXT_PAD_R).max(r.left() + BANNER_TEXT_PAD + 1.0),
            top + m::BANNER_LINE_H,
        ),
    )
}

/// 黄条的文字标签（**一行一条**，参与 `text_fit` 的排版自检）。
///
/// 多行的每一行共用同一个 id `banner.warning`（与 [`crate::text_fit::DynLabel`] 的约定一致），
/// 验收表按 id 分组就能看到"这一条折了几行"。
pub fn dyn_labels(p: &Painter, l: &Layout, d: &DocText) -> Vec<DynLabel> {
    let Some(b) = layout(p, l, d) else {
        return Vec::new();
    };
    b.lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let clip = line_clip(b.rect, i);
            DynLabel::new(
                "banner.warning",
                tx::BANNER_TEXT_TMPL,
                line.clone(),
                LabelStyle::new(
                    m::BODY_FONT,
                    pos2(clip.left(), clip.center().y),
                    Align2::LEFT_CENTER,
                    clip,
                    t::ui().warn_banner_text,
                ),
            )
        })
        .collect()
}

/// 画黄条（底 → 左竖条 → 1px 边 → 逐行文字）。
///
/// ⚠ 必须在 [`crate::canvas::draw`] **之后**调用：它压在画布纹理上，
/// 反过来会被纹理盖掉（纹理比视口大，只靠背景色顶不住）。
pub fn draw(p: &Painter, l: &Layout, d: &DocText) {
    let Some(b) = layout(p, l, d) else {
        return;
    };
    let r = b.rect;
    pt::fill(p, r, t::ui().warn_banner_bg);
    pt::fill(
        p,
        Rect::from_min_max(r.min, pos2(r.left() + BANNER_EDGE_W, r.bottom())),
        t::ui().warn_banner_edge,
    );
    pt::border(p, r, t::ui().warn_banner_edge);
    for mut lab in dyn_labels(p, l, d) {
        lab.truncate_to_fit(p);
        lab.draw(p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fonts;
    use crate::screen_text::DocText;

    fn 画布() -> (Painter, Layout) {
        let ctx = fonts::measure_ctx();
        let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(m::WINDOW_W, m::WINDOW_H));
        let p = Painter::new(ctx, egui::LayerId::debug(), screen);
        (p, Layout::compute(screen))
    }


    /// 一份最小 `DocText`（只关心黄条那几个字段）。
    fn 文案(banner: Option<&str>) -> DocText {
        DocText {
            title: String::new(),
            tab: String::new(),
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
            banner: banner.map(str::to_owned),
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

    /// **没有告警就一条都不画**：既没有矩形，也没有标签（不是画一条空的黄边）。
    #[test]
    fn 无告警时不占位() {
        let (p, l) = 画布();
        let d = 文案(None);
        assert!(rect(&p, &l, &d).is_none(), "没有告警时不许有黄条矩形");
        assert!(dyn_labels(&p, &l, &d).is_empty(), "没有告警时不许登记文字");
    }

    /// **有告警就必须在画布内**（四条边都不越出 `viewport`），且文字盒在条内。
    #[test]
    fn 黄条落在画布内且文字不越界() {
        let (p, l) = 画布();
        let d = 文案(Some("本图有 8040 字节未解释尾块（按头部 30×35 解释）"));
        let r = rect(&p, &l, &d).expect("有告警必须有矩形");
        assert!(
            l.viewport.contains_rect(r),
            "黄条 {r:?} 必须整个落在画布 {:?} 内",
            l.viewport
        );
        // 单行文案 ⇒ 高度 = 上下内边距 + 一行（与上一版的 34 逐像素相同）
        assert!(
            (r.height() - (2.0 * m::BANNER_PAD_Y + m::BANNER_LINE_H)).abs() < 0.01,
            "单行高度不对：{}",
            r.height()
        );
        assert!(r.top() >= l.viewport.top(), "不许压到页签条");
        let clip = line_clip(r, 0);
        assert!(r.contains_rect(clip), "文字裁剪盒必须落在黄条内");
        assert!(clip.width() > 0.0, "文字裁剪盒不能是负宽");
        let labs = dyn_labels(&p, &l, &d);
        assert_eq!(labs.len(), 1, "单行文案恰好一条文字");
        assert_eq!(labs[0].name, "banner.warning");
        assert_eq!(labs[0].text, d.banner.clone().unwrap(), "文案必须是 core 原文");
    }

    /// 画布被压到极窄时**不画**（而不是画一条宽度为负的怪东西）。
    #[test]
    fn 画布过窄时不画() {
        let (p, mut l) = 画布();
        l.viewport = Rect::from_min_size(pos2(100.0, 100.0), vec2(10.0, 200.0));
        let d = 文案(Some("x"));
        assert!(rect(&p, &l, &d).is_none());
    }

    /// 折行必须**逐行实测**：每行宽 ≤ 可用宽，且拼起来等于原文（一个字符都不丢）。
    #[test]
    fn 折行不丢字不溢出() {
        let (p, l) = 画布();
        let text = "每格有 22 字节未建模尾区（格内 rel 14-35，编辑时原样保留、不随图号/光照更新）：\
                    其中 rel 14-15 / 21 / 22-23 共 5 字节 C# 有名字，rel 16-20 共 5 字节与 \
                    rel 24-35 共 12 字节无名字；改图号或光照后尾区不会跟着变，故不承诺改格后语义自洽";
        let d = 文案(Some(text));
        let b = layout(&p, &l, &d).expect("长文案也要有黄条");
        assert!(b.lines.len() >= 2, "250 字左右必须折行：{} 行", b.lines.len());
        let avail = text_width_avail(b.rect);
        for line in &b.lines {
            let w = pt::text_w(&p, line, m::BODY_FONT);
            assert!(w <= avail + 0.01, "某行宽 {w} 超出可用宽 {avail}：{line}");
        }
        let joined: String = b.lines.concat();
        assert_eq!(joined, text, "折行不许丢字/加字");
        assert!(!b.truncated, "这个长度不该触发高度上限");
        assert!(
            l.viewport.contains_rect(b.rect),
            "折行后黄条 {b:?} 必须仍在画布内"
        );
        // 每一行的裁剪盒都在黄条内
        for i in 0..b.lines.len() {
            assert!(b.rect.contains_rect(line_clip(b.rect, i)));
        }
    }

    /// **关键词必须看得见**：`22 字节` / `原样保留` 这类词不许被折行吃掉。
    #[test]
    fn 关键词不被折掉() {
        let (p, l) = 画布();
        let d = 文案(Some(
            "每格有 22 字节未建模尾区（格内 rel 14-35，编辑时原样保留、不随图号/光照更新）",
        ));
        let b = layout(&p, &l, &d).expect("黄条");
        let joined = b.lines.concat();
        for kw in ["22 字节", "原样保留", "不随图号"] {
            assert!(joined.contains(kw), "关键词 {kw} 被折掉了：{joined}");
        }
    }
}
