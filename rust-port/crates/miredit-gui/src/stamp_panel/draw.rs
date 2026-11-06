//! 印章面板的**绘制与交互**：网格（缩略图 / 缺帧占位 / 名称行 / 选中金框）、分类下拉弹层，
//!
//! 以及这一页签的点击/滚轮/键盘（`interact`）。
//!
//! 本模块是 [`super`] 的子模块：直接操作父模块的私有字段（同一份状态，不分家）。
//!
//! 三条纪律：
//!
//! 1. **只画可见格 + 1 行预取**，预取那一行一个像素都不画到可见区外；
//! 2. 缺帧 / 未登记一律**灰底 + 明确文字**，绝不画洋红、绝不用上一枚的图充数；
//! 3. 所有几何来自 [`crate::metrics`]，文案来自 [`crate::ui_text`]。

use egui::{
    pos2, vec2, Align2, Color32, ColorImage, Context, Painter, Rect, Sense, TextureId,
    TextureOptions, Ui,
};

use mir2_assets::WilSource;

use crate::layout::Layout;
use crate::metrics as m;
use crate::paint as pt;
use crate::theme as t;
use crate::ui_text as tx;

use super::geom::{max_row_off, page_range, page_rows};
use super::thumbs::{render_thumb, Hit, Thumb};
use super::{fit_text, Item, StampAction, StampPanel};

impl StampPanel {
    /// 查缓存（只返回 `Copy` 数据，借用不外泄）。
    fn lookup(&mut self, key: &str) -> Hit {
        match self.cache.get(key) {
            Some(Thumb::Ready { tex, w, h }) => Hit::Ready(tex.id(), *w, *h),
            Some(Thumb::Missing) => Hit::Missing,
            None => Hit::Miss,
        }
    }

    /// 取（必要时渲染并上传）列表第 `idx` 项的缩略图。
    ///
    /// 缺帧 / 读不出 / 渲染失败一律 `Hit::Missing`（负缓存），**绝不返回一张假图**。
    fn ensure(&mut self, ctx: &Context, src: Option<&WilSource>, idx: usize) -> Hit {
        let key = match (self.catalog(), self.visible.get(idx)) {
            (Some(cat), Some(Item::Entry(i))) => cat
                .all()
                .get(*i as usize)
                .map(|e| e.id.clone())
                .unwrap_or_default(),
            (_, Some(Item::Unlisted(i))) => format!("#unlisted:{i}"),
            _ => return Hit::Miss,
        };
        match self.lookup(&key) {
            Hit::Ready(t, w, h) => {
                self.hits += 1;
                return Hit::Ready(t, w, h);
            }
            Hit::Missing => {
                self.hits += 1;
                return Hit::Missing;
            }
            Hit::Miss => {}
        }
        // 未登记项**不做缩略图**（任务书 §三.5：只显示文件名，不猜分类、不猜内容）
        if matches!(self.visible.get(idx), Some(Item::Unlisted(_))) {
            self.cache.insert(key, Thumb::Missing);
            return Hit::Missing;
        }
        let Some(src) = src else {
            return Hit::Miss; // 图源未就绪：不缓存（图源可能稍后就绪）
        };
        let path = self
            .catalog_mut()
            .and_then(|c| c.resolve(&key))
            .map(|r| r.path);
        let Some(path) = path else {
            self.cache.insert(key, Thumb::Missing);
            self.rendered_missing += 1;
            return Hit::Missing;
        };
        self.decodes += 1;
        match render_thumb(&path, src) {
            Ok((_, img)) if img.width > 0 && img.height > 0 && img.cells_drawn > 0
                && img.missing == 0 =>
            {
                let color = ColorImage::from_rgba_unmultiplied(
                    [img.width as usize, img.height as usize],
                    &img.rgba,
                );
                let tex = ctx.load_texture(
                    format!("miredit-stamp-{key}"),
                    color,
                    TextureOptions::LINEAR,
                );
                let (id, w, h) = (tex.id(), img.width, img.height);
                self.rendered_ok += 1;
                self.cache.insert(key, Thumb::Ready { tex, w, h });
                Hit::Ready(id, w, h)
            }
            Ok(_) => {
                // `cells_drawn == 0`（空图/全缺帧/帧全透明）或 `missing > 0`（部分缺帧）
                // ⇒ **明确的缺帧占位**，不画半张图（任务书 §三.5）
                self.rendered_missing += 1;
                self.cache.insert(key, Thumb::Missing);
                Hit::Missing
            }
            Err(_) => {
                // 读不出 / 解析失败 / 库号越界 / 渲染报错：同样是"没有图"，不造假
                self.rendered_missing += 1;
                self.cache.insert(key, Thumb::Missing);
                Hit::Missing
            }
        }
    }

    /// 选中项的大缩略图（给「当前资源」预览框用；走同一份缓存与计数）。
    pub fn selected_texture(
        &mut self,
        ctx: &Context,
        src: Option<&WilSource>,
    ) -> Option<(TextureId, u32, u32)> {
        let id = self.selected.as_ref()?.id.clone();
        if let Hit::Ready(t, w, h) = self.lookup(&id) {
            self.hits += 1;
            return Some((t, w, h));
        }
        if let Hit::Missing = self.lookup(&id) {
            return None;
        }
        let src = src?;
        let path = self.catalog_mut().and_then(|c| c.resolve(&id))?.path;
        self.decodes += 1;
        match render_thumb(&path, src) {
            Ok((_, img)) if img.width > 0 && img.cells_drawn > 0 && img.missing == 0 => {
                let color = ColorImage::from_rgba_unmultiplied(
                    [img.width as usize, img.height as usize],
                    &img.rgba,
                );
                let tex =
                    ctx.load_texture(format!("miredit-stamp-sel-{id}"), color, TextureOptions::LINEAR);
                let (tid, w, h) = (tex.id(), img.width, img.height);
                self.rendered_ok += 1;
                self.cache.insert(id, Thumb::Ready { tex, w, h });
                Some((tid, w, h))
            }
            _ => {
                self.rendered_missing += 1;
                self.cache.insert(id, Thumb::Missing);
                None
            }
        }
    }

    // ── 交互 ──

    /// 处理「分类」页签这一帧的全部点击/滚轮/键盘。
    ///
    /// ⚠ 页签本身的切换**不在这里**（由 [`crate::palette::PaletteState`] 拥有 `tab` 状态）。
    pub fn interact(&mut self, ui: &mut Ui, l: &Layout) -> StampAction {
        let mut action = StampAction::None;

        // ① 分类下拉：点框开合；展开时先吃掉弹层内的点击（否则会被下面的网格抢走）
        let combo = crate::dock_right::combo_rect(l);
        if ui
            .interact(combo, ui.id().with("stamp_combo"), Sense::click())
            .clicked()
        {
            self.combo_open = !self.combo_open;
        }
        if self.combo_open {
            let pop = self.combo_popup_rect(l);
            if ui
                .ctx()
                .pointer_hover_pos()
                .map(|p| pop.expand(2.0).contains(p))
                .unwrap_or(false)
            {
                let dy = ui.input(|i| i.smooth_scroll_delta.y);
                if dy.abs() > 1.0 {
                    let step = ((dy.abs() / m::PALETTE_COMBO_ROW_H).round() as usize).max(1);
                    let max = self.kind_rows().len().saturating_sub(self.popup_rows());
                    self.combo_scroll = if dy > 0.0 {
                        self.combo_scroll.saturating_sub(step)
                    } else {
                        (self.combo_scroll + step).min(max)
                    };
                }
            }
            let rows = self.popup_rows();
            for i in 0..rows {
                let idx = self.combo_scroll + i;
                if ui
                    .interact(
                        self.combo_row_rect(l, i),
                        ui.id().with(("stamp_combo_row", i)),
                        Sense::click(),
                    )
                    .clicked()
                {
                    if let Some((f, _)) = self.kind_rows().get(idx).cloned() {
                        self.set_filter(f);
                    }
                }
            }
            if ui.input(|i| i.pointer.any_click()) {
                let inside = ui
                    .ctx()
                    .pointer_hover_pos()
                    .map(|p| pop.expand(2.0).contains(p) || combo.contains(p))
                    .unwrap_or(false);
                if !inside {
                    self.combo_open = false;
                }
            }
            return action; // 弹层展开时不响应面板其它控件（避免误触）
        }

        // ② 搜索框：点一下获得焦点；有焦点就吃键盘（**即时过滤**，不需要回车）
        let search = crate::dock_right::search_rect(l);
        if ui
            .interact(search, ui.id().with("stamp_search"), Sense::click())
            .clicked()
        {
            self.focus = true;
        } else if ui.input(|i| i.pointer.any_click()) && self.focus {
            let inside = ui
                .ctx()
                .pointer_hover_pos()
                .map(|p| search.contains(p))
                .unwrap_or(false);
            if !inside {
                self.focus = false;
            }
        }
        if self.focus {
            let (texts, backspace) = ui.input(|i| {
                let mut texts = Vec::new();
                for e in &i.events {
                    if let egui::Event::Text(t) = e {
                        texts.push(t.clone());
                    }
                }
                (texts, i.key_pressed(egui::Key::Backspace))
            });
            let mut changed = false;
            for t in texts {
                for c in t.chars() {
                    if !c.is_control() {
                        self.search.push(c);
                        changed = true;
                    }
                }
            }
            if backspace && self.search.pop().is_some() {
                changed = true;
            }
            if changed {
                self.selected = None;
                self.rebuild();
            }
        }

        // ③ 网格：滚轮翻行（页内）+ 点选
        let vis = Self::visible_rect(l);
        let hover_in_grid = ui
            .ctx()
            .pointer_hover_pos()
            .map(|p| vis.contains(p))
            .unwrap_or(false);
        if hover_in_grid {
            let dy = ui.input(|i| i.smooth_scroll_delta.y);
            if dy.abs() > 1.0 {
                let step = ((dy.abs() / m::STAMP_CELL_H).round() as usize).max(1);
                let max = max_row_off(self.total(), self.page, Self::rows_visible(l));
                self.row_off = if dy > 0.0 {
                    self.row_off.saturating_sub(step)
                } else {
                    (self.row_off + step).min(max)
                };
            }
        }
        let resp = ui.interact(vis, ui.id().with("stamp_grid"), Sense::click());
        if resp.clicked() {
            if let Some(pos) = resp.interact_pointer_pos() {
                if let Some(idx) = self.hit_test(l, pos) {
                    action = self.pick(idx);
                }
            }
        }

        // ④ 分页行：上/下一页（整页 96 枚）
        let (prev, next) = crate::dock_right::pager_rects(l);
        if ui
            .interact(prev, ui.id().with("stamp_page_prev"), Sense::click())
            .clicked()
        {
            self.set_page(self.page.saturating_sub(1));
        }
        if ui
            .interact(next, ui.id().with("stamp_page_next"), Sense::click())
            .clicked()
        {
            self.set_page((self.page + 1).min(self.pages().saturating_sub(1)));
        }

        action
    }

    // ── 绘制 ──

    /// 画网格本体（含缩略图 / 缺帧占位 / 选中金框 / 名称行）。**只渲染可见格**。
    pub fn draw(&mut self, p: &Painter, l: &Layout, src: Option<&WilSource>) {
        let g = Self::grid_rect(l);
        let vis = Self::visible_rect(l);
        pt::fill(p, g, t::ui().input_search_bg);
        let clip = p.with_clip_rect(vis);
        let rows = Self::rows_visible(l);
        if rows == 0 || self.total() == 0 {
            return;
        }
        let (start, end) = page_range(self.total(), self.page);
        self.row_off = self.row_off.min(max_row_off(self.total(), self.page, rows));
        // +1 行预取：滚一行就能立刻出图（与资源库页同一策略），但**一个像素都不画到可见区外**
        let last = (self.row_off + rows + 1).min(page_rows(self.total(), self.page));
        for row in self.row_off..last {
            for col in 0..m::STAMP_GRID_COLS {
                let idx = start + row * m::STAMP_GRID_COLS + col;
                if idx >= end {
                    break;
                }
                let cell = Self::cell_rect(l, self.row_off, row, col);
                self.draw_cell(&clip, p, cell, idx, src);
            }
        }
    }

    /// 画一格：底 + 缩略图（或缺帧占位）+ 名称行 + 选中金框。
    fn draw_cell(&mut self, clip: &Painter, p: &Painter, cell: Rect, idx: usize, src: Option<&WilSource>) {
        pt::fill(clip, cell, t::ui().panel_bg);
        pt::border(clip, cell, t::ui().separator);
        let thumb = Rect::from_min_size(
            pos2(
                cell.center().x - m::STAMP_THUMB_W * 0.5,
                cell.top() + m::STAMP_CELL_PAD,
            ),
            vec2(m::STAMP_THUMB_W, m::STAMP_THUMB_H),
        );
        match self.ensure(p.ctx(), src, idx) {
            Hit::Ready(tex, w, h) => {
                let dst = crate::palette::fit_rect(thumb, w, h);
                clip.image(
                    tex,
                    dst,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            Hit::Missing => {
                // 缺帧 / 未登记：**灰底 + 明确文字**，绝不画洋红、绝不用上一枚的图充数
                pt::fill(clip, thumb, t::ui().stamp_missing_bg);
                pt::border(clip, thumb, t::ui().checkbox_border);
                let label = if matches!(self.visible.get(idx), Some(Item::Unlisted(_))) {
                    tx::STAMP_UNLISTED
                } else {
                    tx::STAMP_MISSING
                };
                pt::text(
                    clip,
                    thumb.center(),
                    Align2::CENTER_CENTER,
                    label,
                    m::STAMP_NAME_FONT,
                    t::ui().text_muted,
                );
            }
            Hit::Miss => {
                // 图源未就绪：什么都不画（提示行已如实写「资源库未装载」）
            }
        }
        // 名称行（数据驱动 ⇒ 用 `fit_text` 截断，不塞进文案表）
        if let Some((name, _)) = self.item_text(idx) {
            let band = Rect::from_min_max(
                pos2(cell.left() + m::STAMP_CELL_PAD, thumb.bottom() + m::STAMP_NAME_GAP),
                pos2(
                    cell.right() - m::STAMP_CELL_PAD,
                    thumb.bottom() + m::STAMP_NAME_GAP + m::STAMP_NAME_H,
                ),
            );
            let text = fit_text(p, &name, m::STAMP_NAME_FONT, band.width());
            let pc = clip.with_clip_rect(band);
            pt::text(
                &pc,
                pos2(band.center().x, band.center().y),
                Align2::CENTER_CENTER,
                &text,
                m::STAMP_NAME_FONT,
                t::ui().text,
            );
        }
        if self
            .selected
            .as_ref()
            .is_some_and(|s| self.selected_id_matches(idx, &s.id))
        {
            pt::border_w(clip, cell, m::PALETTE_SELECT_STROKE, t::ui().accent_dark);
        }
    }

    /// 第 `idx` 项的 id 是否等于 `id`（选中描边用）。
    fn selected_id_matches(&self, idx: usize, id: &str) -> bool {
        match (self.catalog(), self.visible.get(idx)) {
            (Some(cat), Some(Item::Entry(i))) => cat
                .all()
                .get(*i as usize)
                .map(|e| e.id == id)
                .unwrap_or(false),
            (Some(cat), Some(Item::Unlisted(i))) => cat
                .unlisted()
                .get(*i as usize)
                .map(|rel| rel == id)
                .unwrap_or(false),
            _ => false,
        }
    }

    /// 画分类下拉的弹层（**必须最后画**，它压在网格之上）。
    pub fn draw_combo_popup(&self, p: &Painter, l: &Layout) {
        if !self.combo_open {
            return;
        }
        let pop = self.combo_popup_rect(l);
        pt::fill(p, pop, t::ui().input_bg);
        pt::border(p, pop, t::ui().input_border);
        let rows = self.popup_rows();
        let all = self.kind_rows();
        for i in 0..rows {
            let idx = self.combo_scroll + i;
            let Some((f, n)) = all.get(idx) else {
                break;
            };
            let r = self.combo_row_rect(l, i);
            if *f == self.filter {
                pt::fill(p, r, t::ui().accent_soft);
            }
            let pc = p.with_clip_rect(Rect::from_min_max(
                pos2(r.left() + 4.0, r.top()),
                pos2(r.right() - 44.0, r.bottom()),
            ));
            pt::text(
                &pc,
                pos2(r.left() + 4.0, r.center().y),
                Align2::LEFT_CENTER,
                &f.label(),
                m::PALETTE_LABEL_FONT,
                t::ui().text,
            );
            let count = tx::fill(tx::STAMP_KIND_COUNT_TMPL, &[("n", &n.to_string())]);
            let pc2 = p.with_clip_rect(Rect::from_min_max(
                pos2(r.right() - 44.0, r.top()),
                pos2(r.right() - 4.0, r.bottom()),
            ));
            pt::text(
                &pc2,
                pos2(r.right() - 4.0, r.center().y),
                Align2::RIGHT_CENTER,
                &count,
                m::PALETTE_LABEL_FONT,
                t::ui().text_muted,
            );
        }
    }
}

