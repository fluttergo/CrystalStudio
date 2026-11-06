//! 文字标签的「位置 + 字号 + 裁剪盒」统一描述 —— **绘制与截断验收共用同一份数据**。
//!
//! 为什么要有这一层：验收要求逐条报「标签 → 文字右边界 vs 裁剪盒右边界」。
//! 如果坐标散落在绘制函数里，验收只能靠肉眼看截图；把它抽成 [`Label`] 之后，
//! ① 绘制照旧按这些坐标画；② 单元测试能把每条标签的**实际排版宽度**和裁剪盒宽算清楚，
//! 一旦中文比英文宽到越界，测试当场失败，不必等到截图。
//!
//! 标签的 `name` 一律用 ASCII id（如 `layer.0`、`menu.2`），**不放中文** ——
//! 中文只允许出现在 [`crate::ui_text`] 里。
//!
//! # 两类标签
//!
//! | 类型 | 文案来源 | 谁检查宽度 |
//! |---|---|---|
//! | [`Label`]（静态） | `ui_text` 里的**字面量常量** | 测试断言"排版宽度 ≤ 裁剪盒宽"（写死的内容必须放得下）|
//! | [`DynLabel`]（动态） | `ui_text` 里的**模板 + 真实数据** | 绘制前按裁剪盒宽度**实测截断**（数据长度不可控，见下）|
//!
//! 动态文案为什么不能"写死放得下"：地图文件名、错误原因、图库数量都是运行期才知道的，
//! 长度不可控。硬塞会溢出到邻列，所以 [`DynLabel::truncate_to_fit`] 用**同一套字体测量**
//! 逐字删到放得下、末尾补 `…`，并把 `truncated` 记下来供测试/汇报核对
//! （"真实数据没被切"这件事要能证明，而不是靠眼睛看截图）。

use egui::{Align2, Color32, Painter, Pos2, Rect};

use crate::layout::Layout;
use crate::screen_text::DocText;
use crate::ui_text as tx;

/// 一条要上屏的文字。
#[derive(Clone, Debug)]
pub struct Label {
    /// 标签 id（ASCII，验收表用）。
    pub name: &'static str,
    /// 文案（来自 [`crate::ui_text`] 的常量，不会是临时拼出来的串）。
    pub text: &'static str,
    /// 字号。
    pub font: f32,
    /// 锚点位置。
    pub pos: Pos2,
    /// 锚点对齐方式。
    pub anchor: Align2,
    /// 允许占用的矩形 = 绘制时的裁剪盒。文字右边界必须 ≤ 本矩形右边界。
    pub clip: Rect,
    /// 文字颜色。
    pub color: Color32,
}

impl Label {
    /// 按本条标签画字（自动套上裁剪盒，保证不会溢到邻列）。
    pub fn draw(&self, p: &Painter) {
        let pc = p.with_clip_rect(self.clip);
        crate::paint::text(&pc, self.pos, self.anchor, self.text, self.font, self.color);
    }
}

/// 一条标签的**排版**：字号 / 锚点 / 对齐 / 裁剪盒 / 颜色。
///
/// 为什么单列一个类型：[`DynLabel::new`] 原本吃 8 个参数
/// （`name` / `tmpl` / `text` + 这 5 项），超 clippy `too_many_arguments` 的 7 参阈值。
/// 这 5 项在全部 27 个调用点里都**成套出现**，而且与静态 [`Label`] 的同名字段一一对应
/// —— 它们描述的本来就是同一件事："这条字画在哪、多大、什么色"。
/// 打包成一个类型比用 `allow(clippy::too_many_arguments)` 压掉更能表达这一点。
#[derive(Clone, Copy, Debug)]
pub struct LabelStyle {
    /// 字号。
    pub font: f32,
    /// 锚点位置。
    pub pos: Pos2,
    /// 锚点对齐方式。
    pub anchor: Align2,
    /// 裁剪盒（文字右边界必须 ≤ 本矩形右边界）。
    pub clip: Rect,
    /// 文字颜色。
    pub color: Color32,
}

impl LabelStyle {
    /// 按「字号 / 锚点 / 对齐 / 裁剪盒 / 颜色」的顺序造一份排版。
    ///
    /// 五个参数类型互不相同（`f32` / `Pos2` / `Align2` / `Rect` / `Color32`），
    /// 顺序写反编译不过 ⇒ 按位置传参不会静默出错。
    pub fn new(font: f32, pos: Pos2, anchor: Align2, clip: Rect, color: Color32) -> Self {
        Self {
            font,
            pos,
            anchor,
            clip,
            color,
        }
    }
}

/// 一条**运行期动态**文字：文案由 `ui_text` 的模板 + 真实数据填出。
#[derive(Clone, Debug)]
pub struct DynLabel {
    /// 标签 id（ASCII；多行的每一行共用一个 id，与静态标签的约定一致）。
    pub name: &'static str,
    /// 生成它的模板（必须是 [`crate::ui_text::ALL`] 里的一行，供自检核对）。
    pub tmpl: &'static str,
    /// 填好的文案（可能被 [`Self::truncate_to_fit`] 截断）。
    pub text: String,
    /// 字号。
    pub font: f32,
    /// 锚点位置。
    pub pos: Pos2,
    /// 锚点对齐方式。
    pub anchor: Align2,
    /// 裁剪盒。
    pub clip: Rect,
    /// 文字颜色。
    pub color: Color32,
    /// 是否因为放不下而被截断过（`true` 说明真实数据没完整显示）。
    pub truncated: bool,
}

impl DynLabel {
    /// 造一条动态标签（`truncated` 初值 `false`）。
    ///
    /// 参数是 4 个而不是 8 个：`font` / `pos` / `anchor` / `clip` / `color` 五项打包成
    /// [`LabelStyle`]（理由见该类型的文档）。语义与逐项传参**完全等价**，
    /// 只是把"这条字画在哪、多大、什么色"收成一个有名字的东西。
    pub fn new(name: &'static str, tmpl: &'static str, text: String, style: LabelStyle) -> Self {
        let LabelStyle {
            font,
            pos,
            anchor,
            clip,
            color,
        } = style;
        Self {
            name,
            tmpl,
            text,
            font,
            pos,
            anchor,
            clip,
            color,
            truncated: false,
        }
    }

    /// 按裁剪盒宽度实测截断：放不下就逐字删、末尾补 `…`，并置 `truncated`。
    ///
    /// 用**与绘制完全相同的测量函数**（[`crate::paint::text_w`]），
    /// 所以"测出来放得下"与"画出来不越界"是同一件事。
    pub fn truncate_to_fit(&mut self, p: &Painter) {
        if crate::paint::text_w(p, &self.text, self.font) <= self.clip.width() {
            return;
        }
        let suffix = tx::TRUNC_SUFFIX;
        let mut chars: Vec<char> = self.text.chars().collect();
        while !chars.is_empty() {
            chars.pop();
            let mut s: String = chars.iter().collect();
            s.push_str(suffix);
            if crate::paint::text_w(p, &s, self.font) <= self.clip.width() {
                self.text = s;
                self.truncated = true;
                return;
            }
        }
        self.text = suffix.to_owned();
        self.truncated = true;
    }

    /// 画字（套裁剪盒）。
    pub fn draw(&self, p: &Painter) {
        let pc = p.with_clip_rect(self.clip);
        crate::paint::text(&pc, self.pos, self.anchor, &self.text, self.font, self.color);
    }
}

/// 全部**静态**标签（顺序：标题栏 → 菜单栏 → 左停靠 → 中列 → 右停靠 → 状态栏）。
///
/// 需要 `Painter` 是因为菜单项与 Palette 页签的宽度是**量出来的**（`layout_no_wrap`），
/// 与绘制时用的是同一次测量口径。
pub fn all_labels(p: &Painter, l: &Layout) -> Vec<Label> {
    let mut out = Vec::with_capacity(64);
    out.extend(crate::titlebar::labels(l));
    out.extend(crate::menubar::labels(p, l));
    out.extend(crate::dock_left::labels(p, l));
    out.extend(crate::canvas::labels(l));
    out.extend(crate::dock_right::labels(p, l));
    out.extend(crate::statusbar::labels(p, l));
    out
}

/// 全部**动态**标签（已按各自裁剪盒实测截断）。
pub fn all_dyn_labels(
    p: &Painter,
    l: &Layout,
    t: &DocText,
    pal: &crate::palette::PaletteState,
) -> Vec<DynLabel> {
    let mut out = Vec::with_capacity(32);
    out.extend(crate::titlebar::dyn_labels(l, t));
    out.extend(crate::canvas::dyn_labels(l, t));
    // 常驻告警黄条（画布顶部）：它也是运行期文字，同样要过排版自检
    out.extend(crate::banner::dyn_labels(p, l, t));
    out.extend(crate::dock_left::dyn_labels(l, t));
    out.extend(crate::dock_right::dyn_labels(l, t, pal));
    out.extend(crate::statusbar::dyn_labels(p, l, t));
    for lab in &mut out {
        lab.truncate_to_fit(p);
    }
    out
}

/// 一行文字（可能是多行文本）的排版宽度 = 各行宽度的最大值。
///
/// egui 的 `layout_no_wrap` 支持 `\n` 硬断行，但这里逐行量更直观：
/// 验收表要报的是"哪一行越界"。
pub fn text_width(p: &Painter, text: &str, font: f32) -> f32 {
    text.split('\n')
        .map(|line| crate::paint::text_w(p, line, font))
        .fold(0.0_f32, f32::max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas_view::CanvasView;
    use crate::cellinfo::CellLines;
    use crate::fonts;
    use crate::mapdoc::{Assets, DocState};
    use crate::metrics as m;

    /// 造一个「装了中文字体」的测量用 Painter（与运行时同一份字体、同一套注册逻辑）。
    fn measure_painter() -> Painter {
        let ctx = fonts::measure_ctx();
        Painter::new(
            ctx,
            egui::LayerId::debug(),
            Rect::from_min_size(Pos2::ZERO, egui::vec2(m::WINDOW_W, m::WINDOW_H)),
        )
    }

    fn window() -> Rect {
        Rect::from_min_size(Pos2::ZERO, egui::vec2(m::WINDOW_W, m::WINDOW_H))
    }

    /// 空素材面板（一个库都没登记）：用于验证"帧数未知时如实回落成 `素材库 N 个`"。
    fn 空面板() -> crate::palette::PaletteState {
        crate::palette::PaletteState::new(None)
    }

    /// **真实的 36B（`Shanda2012`）告警原文** —— 造一张 `52 + 36×4×4 = 628` 字节的合成图，
    /// 让 `miredit_core::Document::edit_warning()` 自己拼出来。
    ///
    /// 为什么不用字面量抄一遍：那段 caveat 约 250 字，是 `mir2-format` 的常量；
    /// 抄进本文件就是两套真值（那边改一个字，这里的"最坏情况"就悄悄变短了）。
    /// 合成图的判别依据：`len == 52 + 36·w·h` ⇒ `Shanda2012`（`mir2-format/src/registry.rs:35`）。
    fn 真实36b黄条() -> String {
        let (w, h) = (4u16, 4u16);
        let mut b = vec![0u8; 52 + 36 * w as usize * h as usize];
        b[0..2].copy_from_slice(&w.to_le_bytes());
        b[2..4].copy_from_slice(&h.to_le_bytes());
        let doc = crate::mapdoc::MapDoc::from_bytes(std::path::Path::new(r"E:\x\36b.map"), b)
            .expect("628 字节的合成图应被判别为 36B 格式");
        let warn = doc
            .edit_warning()
            .expect("36B 格式必须带 editable_caveat ⇒ 必须有告警");
        // 兜底断言：这段原文确实"长"（≥200 字），否则本文件的"最坏情况"就不再是最坏情况
        assert!(
            warn.chars().count() >= 200,
            "36B 告警只有 {} 字，最坏情况样本要重新评估：{warn}",
            warn.chars().count()
        );
        warn
    }

    /// 造一份**最长可能**的运行期文案（真实数据里最坏的情况）。
    ///
    /// 取值依据：文件名取本机最长的一批（`REDCAVE02.map`）、地图尺寸取 700×700、
    /// 错误原因给 60 个汉字（远超实际报错长度，用来验证截断确实动作）。
    /// 中文一律用 `ui_text` 里的现成字块拼，**不在测试里写中文界面文案**。
    fn 最坏情况文案() -> DocText {
        let mut lines_values: [String; 6] = Default::default();
        let kv: [(&str, &str); 3] = [("img", "65535"), ("lib", "255"), ("size", "65535×65535")];
        lines_values[0] = tx::fill(
            tx::VAL_COLLISION_TMPL,
            &[("low", tx::VAL_YES), ("high", tx::VAL_YES)],
        );
        // 前两行共用「图号/库号/帧」，第三行（物件）的库号叫「表层库号」，单独一行拼
        for v in lines_values.iter_mut().skip(1).take(2) {
            *v = tx::fill(tx::VAL_GROUND_TMPL, &kv);
        }
        lines_values[3] = tx::fill(tx::VAL_OBJECTS_TMPL, &kv);
        lines_values[4] = tx::fill(
            tx::VAL_METADATA_TMPL,
            &[("light", "255"), ("door", "255"), ("offset", "255")],
        );
        lines_values[5] = tx::fill(
            tx::VAL_TILE_ANIMATION_TMPL,
            &[("frame", "255"), ("tick", "255")],
        );
        DocText {
            title: tx::fill(
                tx::TITLE_TMPL,
                &[("name", "REDCAVE02.map"), ("w", "700"), ("h", "700")],
            ),
            tab: "REDCAVE02.map".to_string(),
            // 最坏情况：胶囊里是**最大尺寸** `31×31`（比 `3×3` 宽一倍）
            pill: tx::fill(tx::PILL_SIZE_TMPL, &[("n", "31")]),
            brush_size: 31,
            status_left: tx::fill(tx::STATUS_MAP_FAIL_TMPL, &[("err", &tx::VAL_YES.repeat(60))]),
            status_tool: tx::fill(tx::STATUS_TOOL_TMPL, &[("tool", tx::TOOL_UNWIRED)]),
            status_layer: tx::fill(tx::STATUS_LAYER_TMPL, &[("layer", tx::LAYER_OBJECTS)]),
            status_brush: tx::fill(
                tx::STATUS_BRUSH_TMPL,
                &[
                    ("fam", tx::LAYER_OBJECTS),
                    ("img", "65535"),
                    ("lib", "255"),
                ],
            ),
            // 最坏情况：**族不匹配的拒绝长句**（比"落笔字段"那句长得多，用来验证截断真的动作）
            status_hint: tx::fill(
                tx::EDIT_LAYER_MISMATCH_TMPL,
                &[("brush", tx::LAYER_GROUND), ("layer", tx::LAYER_OBJECTS)],
            ),
            status_hint_warn: true,
            status_coord: tx::fill(
                tx::STATUS_COORD_TMPL,
                &[("x", "65535"), ("y", "65535")],
            ),
            status_lib: tx::fill(tx::STATUS_LIB_TMPL, &[("n", "65535")]),
            // 最坏情况：**磁盘格式名也是最长的那一条**（`CrystalCs` 的说明最长）
            status_format: tx::fill(
                tx::STATUS_FORMAT_TMPL,
                &[("fmt", "CrystalCs（变长/格 + 2 字节头 + 逐块索引，无标题）")],
            ),
            // 最坏情况：**黄条是真实的 36B 格式 caveat 原文**（2026-11-05 起）。
            //
            // ⚠ 这里**不抄文案**：`Shanda2012` 的 `editable_caveat` 是 `mir2-format` 的常量，
            //    抄一份进测试就是两套真值（那边改一个字，这里就悄悄过期了）。
            //    改为**造一张 36B/格的合成图**（`52 + 36×4×4 = 628` 字节，判别式见
            //    `mir2-format/src/registry.rs:35`），让 `Document::edit_warning()` 自己吐出原文。
            banner: Some(真实36b黄条()),
            ground_locked: true,
            status_stamp: None,
            // 最坏情况：填充回执也在（六位格数 + 中文），状态栏八段同时在场
            status_fill: Some(tx::fill(tx::STATUS_FILL_TMPL, &[("n", "200000")])),
            status_op: None,
            status_op_warn: false,
            status_sel: None,
            status_sel_tmpl: None,
            zoom: tx::fill(tx::ZOOM_PCT_TMPL, &[("pct", "400")]),
            page: tx::fill(tx::PAGE_LABEL_TMPL, &[("n", "65535")]),
            inspector_head: tx::fill(
                tx::INSPECTOR_HEAD_TMPL,
                &[("x", "65535"), ("y", "65535"), ("n", "3")],
            ),
            inspector_values: lines_values,
        }
    }

    /// 造一份「真实会出现的」文案（0.map + 380,310 + 117 个库），用于断言**没被截断**。
    fn 典型文案() -> DocText {
        let mut values: [String; 6] = Default::default();
        let kv: [(&str, &str); 3] = [("img", "1234"), ("lib", "40"), ("size", "96×64")];
        values[0] = tx::fill(
            tx::VAL_COLLISION_TMPL,
            &[("low", tx::VAL_NO), ("high", tx::VAL_NO)],
        );
        for v in values.iter_mut().skip(1).take(2) {
            *v = tx::fill(tx::VAL_GROUND_TMPL, &kv);
        }
        values[3] = tx::fill(tx::VAL_OBJECTS_TMPL, &kv);
        values[4] = tx::fill(
            tx::VAL_METADATA_TMPL,
            &[("light", "0"), ("door", "0"), ("offset", "0")],
        );
        values[5] = tx::fill(
            tx::VAL_TILE_ANIMATION_TMPL,
            &[("frame", "0"), ("tick", "0")],
        );
        DocText {
            title: tx::fill(
                tx::TITLE_TMPL,
                &[("name", "0.map"), ("w", "700"), ("h", "700")],
            ),
            tab: "0.map".to_string(),
            pill: tx::fill(tx::PILL_SIZE_TMPL, &[("n", "7")]),
            brush_size: 7,
            status_left: tx::fill(
                tx::STATUS_LEFT_TMPL,
                &[("name", "0.map"), ("w", "700"), ("h", "700")],
            ),
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
            status_coord: tx::fill(tx::STATUS_COORD_TMPL, &[("x", "380"), ("y", "310")]),
            status_lib: tx::fill(tx::STATUS_LIB_TMPL, &[("n", "117")]),
            status_format: tx::fill(tx::STATUS_FORMAT_TMPL, &[("fmt", "Shanda12（12B/格，52B 头）")]),
            banner: None,
            ground_locked: false,
            status_stamp: None,
            status_fill: None,
            status_op: None,
            status_op_warn: false,
            status_sel: None,
            status_sel_tmpl: None,
            zoom: tx::fill(tx::ZOOM_PCT_TMPL, &[("pct", "100")]),
            page: tx::fill(tx::PAGE_LABEL_TMPL, &[("n", "117")]),
            inspector_head: tx::fill(
                tx::INSPECTOR_HEAD_TMPL,
                &[("x", "380"), ("y", "310"), ("n", "3")],
            ),
            inspector_values: values,
        }
    }

    /// **文案未截断（验收门 §五-6 的程序化版本）**：静态标签的排版宽度必须放得进裁剪盒。
    ///
    /// 输出两段：
    /// - 人看的明细表（`name w=… cap=…`）；
    /// - 机器读的 `BOX\t…` 行（供截图逐像素核对"文字像素右边界 vs 裁剪盒右边界"）。
    #[test]
    fn 文案不超裁剪盒() {
        let p = measure_painter();
        let l = Layout::compute(window());
        let mut bad = Vec::new();
        let mut rows = Vec::new();
        for lab in all_labels(&p, &l) {
            let w = text_width(&p, lab.text, lab.font);
            let cap = lab.clip.width();
            rows.push(format!(
                "{:<22} w={:6.1} cap={:6.1} pos={:>7.1},{:>6.1}",
                lab.name, w, cap, lab.pos.x, lab.pos.y
            ));
            eprintln!(
                "BOX\t{}\t{:.1}\t{:.1}\t{:.1}\t{:.1}\t{:.1}\t{:.1}",
                lab.name,
                lab.clip.left(),
                lab.clip.top(),
                lab.clip.right(),
                lab.clip.bottom(),
                w,
                lab.font
            );
            if w > cap + 0.5 {
                bad.push(format!(
                    "{} = {:?} 宽 {:.1} > 裁剪盒 {:.1}",
                    lab.name, lab.text, w, cap
                ));
            }
        }
        eprintln!("标签宽度明细（{} 条）：\n{}", rows.len(), rows.join("\n"));
        assert!(bad.is_empty(), "以下标签会截断：\n{}", bad.join("\n"));
    }

    /// 标签表本身要自洽：非空文案、裁剪盒宽为正、同一 id 不会在同一位置出现两次。
    ///
    /// 注意：**多行值**（`\n` 拆出来的每一行）共用同一个 id 是设计如此（验收表按行报），
    /// 所以这里查的是 `(id, y)` 唯一，而不是 id 唯一。
    #[test]
    fn 标签表自洽() {
        let p = measure_painter();
        let l = Layout::compute(window());
        let labels = all_labels(&p, &l);
        assert!(labels.len() >= 40, "标签太少（{}），可能漏收", labels.len());
        let mut seen: Vec<(&str, i32)> = labels
            .iter()
            .map(|x| (x.name, (x.pos.y * 10.0).round() as i32))
            .collect();
        let n = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), n, "同一标签 id 在同一 y 上出现两次");
        for lab in &labels {
            assert!(!lab.text.is_empty(), "{} 文案为空", lab.name);
            assert!(lab.clip.width() > 0.0, "{} 裁剪盒宽非正", lab.name);
        }
    }

    /// 文案都必须来自 `ui_text` 的表（防"顺手写死一句中文"）。
    ///
    /// 多行值被 `\n` 拆成多行后，每一行都必须是某条表项的一行 —— 这样既允许拆行，
    /// 又不允许出现表里没有的字。
    #[test]
    fn 标签文案都来自文案表() {
        let p = measure_painter();
        let l = Layout::compute(window());
        let lines: Vec<&str> = crate::ui_text::ALL
            .iter()
            .flat_map(|(_, s)| s.split('\n'))
            .collect();
        for lab in all_labels(&p, &l) {
            assert!(
                lines.contains(&lab.text),
                "标签 {} 的文案 {:?} 不在 ui_text::ALL 里",
                lab.name,
                lab.text
            );
        }
    }

    /// 动态标签的模板必须来自 `ui_text::ALL`（防止出现一条表里没有的文案）。
    #[test]
    fn 动态标签模板都来自文案表() {
        let p = measure_painter();
        let l = Layout::compute(window());
        let all: Vec<&str> = crate::ui_text::ALL.iter().map(|(_, s)| *s).collect();
        for t in [典型文案(), 最坏情况文案()] {
            for lab in all_dyn_labels(&p, &l, &t, &空面板()) {
                assert!(
                    all.contains(&lab.tmpl),
                    "动态标签 {} 的模板 {:?} 不在 ui_text::ALL 里",
                    lab.name,
                    lab.tmpl
                );
            }
        }
    }

    /// 动态标签**永远不会溢出裁剪盒**（宽度是实测截断后量出来的）。
    #[test]
    fn 动态标签不溢出() {
        let p = measure_painter();
        let l = Layout::compute(window());
        for t in [典型文案(), 最坏情况文案()] {
            for lab in all_dyn_labels(&p, &l, &t, &空面板()) {
                let w = text_width(&p, &lab.text, lab.font);
                assert!(
                    w <= lab.clip.width() + 0.5,
                    "动态标签 {} = {:?} 宽 {:.1} > 裁剪盒 {:.1}",
                    lab.name,
                    lab.text,
                    w,
                    lab.clip.width()
                );
                assert!(!lab.text.is_empty(), "动态标签 {} 文案为空", lab.name);
            }
        }
    }

    /// **真实数据没被截断**：典型取值下所有动态标签都必须完整显示（`truncated == false`）。
    ///
    /// 这条是"截断机制"的防滥用闸门：如果有人把裁剪盒改窄，真实内容就会被切，
    /// 这里当场变红，而不是等用户看到 `0.ma…`。
    #[test]
    fn 典型动态文案不被截断() {
        let p = measure_painter();
        let l = Layout::compute(window());
        let mut bad = Vec::new();
        for lab in all_dyn_labels(&p, &l, &典型文案(), &空面板()) {
            if lab.truncated {
                bad.push(format!("{} = {:?}", lab.name, lab.text));
            }
        }
        assert!(bad.is_empty(), "典型数据被截断了：\n{}", bad.join("\n"));
    }

    /// 最坏情况确实会截断（证明截断机制真的在工作，不是形同虚设）。
    #[test]
    fn 超长文案会被截断() {
        let p = measure_painter();
        let l = Layout::compute(window());
        let labs = all_dyn_labels(&p, &l, &最坏情况文案(), &空面板());
        let hit = labs.iter().filter(|x| x.truncated).count();
        assert!(hit > 0, "60 字错误原因/12 字文件名竟一个都没截断，测量口径可疑");
        for lab in labs.iter().filter(|x| x.truncated) {
            assert!(
                lab.text.ends_with(tx::TRUNC_SUFFIX),
                "截断后必须以省略号收尾：{:?}",
                lab.text
            );
        }
    }

    /// 动态标签的 id 在同一 y 上不重复（与静态标签同一口径）。
    #[test]
    fn 动态标签id与行位置唯一() {
        let p = measure_painter();
        let l = Layout::compute(window());
        let labels = all_dyn_labels(&p, &l, &典型文案(), &空面板());
        let mut seen: Vec<(&str, i32)> = labels
            .iter()
            .map(|x| (x.name, (x.pos.y * 10.0).round() as i32))
            .collect();
        let n = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), n, "同一动态标签 id 在同一 y 上出现两次");
    }

    /// 没打开地图时也要能排出一整套动态标签（空态不能崩、不能留空串）。
    #[test]
    fn 空文档动态标签可排出() {
        let p = measure_painter();
        let l = Layout::compute(window());
        let doc = DocState {
            doc: None,
            assets: Assets::Failed("x".into()),
            map_error: None,
            map_gen: 1,
        };
        let t = crate::screen_text::build(
            &doc,
            &CanvasView::default(),
            None,
            &CellLines::unknown(),
            &crate::edit::EditSession::default(),
        );
        let labs = all_dyn_labels(&p, &l, &t, &空面板());
        assert!(labs.len() >= 15, "动态标签太少（{}）", labs.len());
        for lab in &labs {
            assert!(!lab.text.is_empty(), "{} 空串", lab.name);
        }
    }

    /// **250 字的 36B caveat 在两种窗口尺寸下都不越界，且至少显示前 200 字**。
    ///
    /// 背景（2026-11-05）：`Shanda2012` 的 `editable_caveat` 约 250 字，比上一版那句
    /// 60 字套话长 4 倍。黄条因此改成**折行 + 高度自适应**（见 [`crate::banner`] §三）。
    ///
    /// 这条用例同时钉住三件事：
    /// 1. **不横向溢出**：黄条整条在画布内，且每一行实测宽 ≤ 可用宽；
    /// 2. **不丢字**：折好的行拼起来与 core 原文**逐字相等**；
    /// 3. **至少显示前 200 字** —— 为什么是 200：caveat 全文约 250 字，前 200 字里已经包含
    ///    「22 字节」「原样保留」「rel 14-15」「不随图号/光照更新」四个关键信息；
    ///    200 是**兜底下限**（正常折行是全文显示，一个字都不截）。
    #[test]
    fn 三十六b告警在两种窗口下都不越界() {
        let p = measure_painter();
        let text = 真实36b黄条();
        for (w, h, 名字) in [
            (m::WINDOW_W, m::WINDOW_H, "1500×950"),
            (m::WINDOW_MIN_W, m::WINDOW_MIN_H, "1100×720"),
        ] {
            let screen = Rect::from_min_size(Pos2::ZERO, egui::vec2(w, h));
            let l = Layout::compute(screen);
            let mut d = 最坏情况文案();
            d.banner = Some(text.clone());
            let b = crate::banner::layout(&p, &l, &d)
                .unwrap_or_else(|| panic!("{名字}：250 字的 caveat 必须有黄条"));
            // ① 不越界
            assert!(
                l.viewport.contains_rect(b.rect),
                "{名字}：黄条 {:?} 越出画布 {:?}",
                b.rect,
                l.viewport
            );
            assert!(!b.truncated, "{名字}：正常窗口下不该触发高度上限");
            // ② 不丢字
            let joined: String = b.lines.concat();
            assert_eq!(joined, text, "{名字}：折行丢了字");
            // ③ 至少显示前 200 字
            assert!(
                joined.chars().count() >= 200,
                "{名字}：只显示了 {} 字",
                joined.chars().count()
            );
            // ④ 每一行都放得下（横向不溢出）
            let avail = b.rect.width() - crate::banner::BANNER_TEXT_PAD
                - crate::banner::BANNER_TEXT_PAD_R;
            for line in &b.lines {
                let lw = crate::paint::text_w(&p, line, m::BODY_FONT);
                assert!(
                    lw <= avail + 0.01,
                    "{名字}：某行宽 {lw:.1} 超过可用宽 {avail:.1}：{line}"
                );
            }
            // ⑤ 关键词在（不是"折到看不见"）
            for kw in ["22 字节", "原样保留"] {
                assert!(joined.contains(kw), "{名字}：关键词 {kw} 丢了");
            }
            eprintln!(
                "{名字}：画布 {}×{}，黄条 {}×{}，折 {} 行，全文 {} 字",
                l.viewport.width(),
                l.viewport.height(),
                b.rect.width(),
                b.rect.height(),
                b.lines.len(),
                joined.chars().count()
            );
        }
    }

    /// **品牌字与窗口标题在两种窗口尺寸下都完整可见**（2026-11-06：软件名固定 `Crystal Studio`）。
    ///
    /// `Crystal Studio` 比旧的「地图编辑器」长 5 个字符，而标题栏标题是「左对齐 + 裁剪盒」、
    /// 菜单栏品牌字是「右对齐 + 裁剪盒」—— 窗口一窄就可能被切。这条用例把
    /// 1500×950（默认窗）与 1100×720（`crystalstudio-ui.json` 的最小窗）两档都钉住：
    /// ① 品牌字排版宽度 ≤ 裁剪盒宽；② 典型标题（真文件名 + 700×700 格）不得被截断。
    /// 报出的 `w` / `cap` 就是实测像素（见回执）。
    #[test]
    fn 品牌字与标题在两种窗口下都不被裁剪() {
        let p = measure_painter();
        let mut rows = Vec::new();
        for (w, h, 名字) in [
            (m::WINDOW_W, m::WINDOW_H, "1500×950"),
            (m::WINDOW_MIN_W, m::WINDOW_MIN_H, "1100×720"),
        ] {
            let l = Layout::compute(Rect::from_min_size(Pos2::ZERO, egui::vec2(w, h)));

            // ① 菜单栏右端品牌字：文案必须是新品牌名，且放得进裁剪盒
            let brand = crate::menubar::labels(&p, &l)
                .into_iter()
                .find(|x| x.name == "menu.brand")
                .expect("菜单栏必须有 menu.brand");
            assert_eq!(brand.text, tx::BRAND, "{名字}：品牌字取错");
            let bw = text_width(&p, brand.text, brand.font);
            rows.push(format!(
                "{名字}  menu.brand w={bw:.1} cap={:.1} 左端 x={:.1}",
                brand.clip.width(),
                brand.clip.left()
            ));
            assert!(
                bw <= brand.clip.width() + 0.5,
                "{名字}：品牌字 {:?} 宽 {bw:.1} > 裁剪盒 {:.1}，会被切",
                brand.text,
                brand.clip.width()
            );

            // ② 标题栏：典型标题（真文件名 + 真格数）不得被截断
            let d = 典型文案();
            for lab in crate::titlebar::dyn_labels(&l, &d) {
                let lw = text_width(&p, &lab.text, lab.font);
                rows.push(format!(
                    "{名字}  {} w={lw:.1} cap={:.1}",
                    lab.name,
                    lab.clip.width()
                ));
                assert!(
                    !lab.truncated,
                    "{名字}：标题被截断成 {:?}（裁剪盒 {:.1}）",
                    lab.text,
                    lab.clip.width()
                );
            }
        }
        eprintln!("品牌/标题实测：\n{}", rows.join("\n"));
    }
}
