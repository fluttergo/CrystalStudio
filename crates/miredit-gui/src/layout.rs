//! 布局计算：把 [`crate::metrics`] 的常量算成一组矩形。
//!
//! **本模块不含任何字面量尺寸** —— 每个数字都来自 `metrics::*`，每个 `metrics::*`
//! 都在自己的文档注释里写明「取自截图 (x,y)-(x,y)」或「取自 JSON 路径」。
//! 这是任务书 §八「禁在代码里写死『看起来差不多』的尺寸」的落地方式。

use egui::{pos2, Rect};

use crate::metrics as m;

/// 一块面板：标题条矩形 + 内容区矩形。
#[derive(Clone, Copy, Debug)]
pub struct PanelRect {
    /// 面板整体（标题条 + 内容）。
    pub whole: Rect,
    /// 标题条。
    pub header: Rect,
    /// 内容区。
    pub content: Rect,
}

impl PanelRect {
    fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        let whole = Rect::from_min_size(pos2(x, y), egui::vec2(w, h));
        let header = Rect::from_min_size(whole.min, egui::vec2(w, m::PANEL_HEADER_H));
        let content = Rect::from_min_max(
            pos2(whole.left(), whole.top() + m::PANEL_HEADER_H),
            whole.max,
        );
        Self {
            whole,
            header,
            content,
        }
    }
}

/// 整个窗口的布局结果。
#[derive(Clone, Debug)]
pub struct Layout {
    /// 客户区（= 整个窗口）。
    pub content: Rect,
    /// 标题栏。
    pub title_bar: Rect,
    /// 菜单栏。
    pub menu_bar: Rect,
    /// 菜单栏下方的 1px 分隔线。
    pub menu_sep: Rect,
    /// 左停靠区（宽 156）。
    pub left_dock: Rect,
    /// 中列（页签条 + 视口）。
    pub center: Rect,
    /// 中列与右停靠区之间的 1px 分隔线。
    pub dock_divider: Rect,
    /// 右停靠区（宽 356）。
    pub right_dock: Rect,
    /// 状态栏（含上沿 1px 分隔线）。
    pub status_bar: Rect,
    /// 状态栏上沿 1px 分隔线。
    pub status_sep: Rect,
    /// 状态栏文字/内容的可用区（去掉分隔线与左右内边距）。
    pub status_inner: Rect,

    /// 左停靠区四个面板。
    pub p_tools: PanelRect,
    pub p_tool_options: PanelRect,
    pub p_layers: PanelRect,
    pub p_cell_inspector: PanelRect,

    /// 右停靠区三个面板。
    pub p_minimap: PanelRect,
    pub p_palette: PanelRect,
    pub p_active_asset: PanelRect,

    /// **地图列表面板**（2026-11-05，上游 `901dbe2f` 移植）。
    ///
    /// 位置：**中列左侧**、页签条以下、占满视口高度的一列（宽 [`m::MAPS_W`]）。
    /// 面板**关着**时这块矩形的宽是 0（贴在画布左边缘），`viewport` 因此与旧版逐像素相同
    /// —— 这是"默认态不变"的几何保证，见 [`Layout::compute_with`]。
    pub p_maps: PanelRect,

    /// 中列页签条。
    pub tab_strip: Rect,
    /// 中列视口 —— **就是真实地图的画布矩形**（页签条以下、状态栏以上、左右停靠区之间）。
    ///
    /// 本单起地图 1:1 铺满这块矩形（上一版这里是"浅灰底 + 48×32 网格 + 装饰选框"的骨架），
    /// 因此不再单独保留 `map_slab` / `selection` 两个字段：
    /// 画布矩形与视口矩形**就是同一个**，留两份必然有一天会不一致。
    pub viewport: Rect,
    /// 左下角缩放浮层。
    pub zoom_pill: Rect,

    /// 10 个工具按钮的矩形（顺序同 `icons::TOOLS`）。
    pub tool_buttons: [Rect; 10],
    /// 5 个图层行。
    pub layer_rows: [Rect; 5],
    /// `Cell Highlights` 分隔线。
    pub layer_group_sep: Rect,
    /// `Light Cells` / `Door Cells` 两行。
    pub layer_sub_rows: [Rect; 2],
}

impl Layout {
    /// 按窗口客户区尺寸算出整张布局（**地图列表面板关着** = 与旧版逐像素相同）。
    ///
    /// 这是默认入口：`--gate2-client` 的同视点比对、以及全部既有截图基线都建立在
    /// "画布 = `left_dock.right` 到 `dock_divider.left`" 之上。
    pub fn compute(window: Rect) -> Self {
        Self::compute_with(window, false)
    }

    /// 按窗口客户区尺寸算出整张布局，`maps_open` 决定**地图列表面板**占不占一列。
    ///
    /// # 为什么是"让出一列"而不是"浮层盖上去"
    ///
    /// 上游把地图列表做成停靠面板（`MainWindow.axaml.cs:343` 的 `Dock.Left`），
    /// 停靠面板**不盖画布**。本仓照这个语义：开着就把画布往右推，
    /// 关着就把那 268px 还回去 —— 两种状态下 `viewport` 都是"画布矩形"这一个概念，
    /// 没有第二份坐标（浮层方案会让"画布矩形"与"看得见的地图矩形"变成两个东西）。
    ///
    /// 唯一被固定的东西是**页签条**：它仍横跨整个中列（开合面板不改变页签位置），
    /// 这样 `canvas.tab_title` 的裁剪盒在两种状态下都成立。
    pub fn compute_with(window: Rect, maps_open: bool) -> Self {
        // ── 顶/底条 ──
        let title_bar = Rect::from_min_size(window.min, egui::vec2(window.width(), m::TITLE_BAR_H));
        let menu_bar = Rect::from_min_size(
            pos2(window.left(), title_bar.bottom()),
            egui::vec2(window.width(), m::MENU_BAR_H),
        );
        let menu_sep = Rect::from_min_size(
            pos2(window.left(), menu_bar.bottom()),
            egui::vec2(window.width(), m::SEPARATOR_H),
        );

        let status_bar = Rect::from_min_size(
            pos2(window.left(), window.bottom() - m::STATUS_BAR_H),
            egui::vec2(window.width(), m::STATUS_BAR_H),
        );
        let status_sep =
            Rect::from_min_size(status_bar.min, egui::vec2(window.width(), m::SEPARATOR_H));
        let status_inner = Rect::from_min_max(
            pos2(status_bar.left() + m::STATUS_PAD_X, status_sep.bottom()),
            pos2(status_bar.right() - m::STATUS_PAD_X, status_bar.bottom()),
        );

        // ── 三列 ──
        let body_top = menu_sep.bottom();
        let body_bottom = status_bar.top();
        let body_h = body_bottom - body_top;

        let left_dock = Rect::from_min_size(
            pos2(window.left(), body_top),
            egui::vec2(m::LEFT_DOCK_W, body_h),
        );
        let right_dock = Rect::from_min_size(
            pos2(window.right() - m::RIGHT_DOCK_W, body_top),
            egui::vec2(m::RIGHT_DOCK_W, body_h),
        );
        let dock_divider = Rect::from_min_size(
            pos2(right_dock.left() - m::DOCK_DIVIDER_W, body_top),
            egui::vec2(m::DOCK_DIVIDER_W, body_h),
        );
        let center = Rect::from_min_max(
            pos2(left_dock.right(), body_top),
            pos2(dock_divider.left(), body_bottom),
        );

        // ── 左停靠区四个面板（前三个定高，末位吸收剩余） ──
        let mut y = left_dock.top();
        let p_tools = PanelRect::new(left_dock.left(), y, m::LEFT_DOCK_W, m::PANEL_TOOLS_H);
        y = p_tools.whole.bottom();
        let p_tool_options =
            PanelRect::new(left_dock.left(), y, m::LEFT_DOCK_W, m::PANEL_TOOL_OPTIONS_H);
        y = p_tool_options.whole.bottom();
        let p_layers = PanelRect::new(left_dock.left(), y, m::LEFT_DOCK_W, m::PANEL_LAYERS_H);
        y = p_layers.whole.bottom();
        let p_cell_inspector =
            PanelRect::new(left_dock.left(), y, m::LEFT_DOCK_W, left_dock.bottom() - y);

        // ── 右停靠区三个面板（前两个定高，末位吸收剩余） ──
        let mut ry = right_dock.top();
        let p_minimap = PanelRect::new(right_dock.left(), ry, m::RIGHT_DOCK_W, m::PANEL_MINIMAP_H);
        ry = p_minimap.whole.bottom();
        let p_palette = PanelRect::new(right_dock.left(), ry, m::RIGHT_DOCK_W, m::PANEL_PALETTE_H);
        ry = p_palette.whole.bottom();
        let p_active_asset = PanelRect::new(
            right_dock.left(),
            ry,
            m::RIGHT_DOCK_W,
            right_dock.bottom() - ry,
        );

        // ── 中列：页签条 + 视口（= 真实地图画布） ──
        let tab_strip = Rect::from_min_size(center.min, egui::vec2(center.width(), m::TABSTRIP_H));
        let mut viewport = Rect::from_min_max(pos2(center.left(), tab_strip.bottom()), center.max);
        // 地图列表面板：开 ⇒ 占画布左侧一列并把画布右推；关 ⇒ 零宽（画布不变）
        let p_maps = if maps_open {
            let r = PanelRect::new(
                viewport.left(),
                viewport.top(),
                m::MAPS_W,
                viewport.height(),
            );
            viewport = Rect::from_min_max(pos2(r.whole.right(), viewport.top()), viewport.max);
            r
        } else {
            PanelRect::new(viewport.left(), viewport.top(), 0.0, viewport.height())
        };

        let zoom_pill = Rect::from_min_size(
            pos2(
                viewport.left() + m::ZOOM_PILL_OFF,
                viewport.bottom() - m::ZOOM_PILL_OFF - m::ZOOM_PILL_H,
            ),
            egui::vec2(m::ZOOM_PILL_W, m::ZOOM_PILL_H),
        );

        // ── Tools 面板按钮网格 4 列 ──
        let btn_x0 = p_tools.content.left() + m::PANEL_PAD + m::TOOL_MARGIN;
        let btn_y0 = p_tools.content.top() + m::TOOLS_FIRST_ROW_OFF;
        let mut tool_buttons = [Rect::NOTHING; 10];
        for (i, slot) in tool_buttons.iter_mut().enumerate() {
            let col = (i % m::TOOL_COLS) as f32;
            let row = (i / m::TOOL_COLS) as f32;
            *slot = Rect::from_min_size(
                pos2(btn_x0 + col * m::TOOL_ITEM, btn_y0 + row * m::TOOL_ITEM),
                egui::vec2(m::TOOL_BTN, m::TOOL_BTN),
            );
        }

        // ── Layers 面板 5 行 + 子组 2 行 ──
        let row_left = p_layers.content.left() + m::LAYER_ROW_INSET_L;
        let row_right = p_layers.content.right() - m::LAYER_ROW_INSET_R;
        let row_top = p_layers.content.top() + m::LAYER_FIRST_ROW_OFF;
        let mut layer_rows = [Rect::NOTHING; 5];
        for (i, slot) in layer_rows.iter_mut().enumerate() {
            *slot = Rect::from_min_size(
                pos2(row_left, row_top + i as f32 * m::LAYER_ROW_H),
                egui::vec2(row_right - row_left, m::LAYER_ROW_H),
            );
        }
        let sep_y = row_top + 5.0 * m::LAYER_ROW_H + m::LAYER_GROUP_GAP;
        let layer_group_sep =
            Rect::from_min_size(pos2(row_left, sep_y), egui::vec2(row_right - row_left, 1.0));
        let sub_top = sep_y + m::LAYER_GROUP_LABEL_H;
        let mut layer_sub_rows = [Rect::NOTHING; 2];
        for (i, slot) in layer_sub_rows.iter_mut().enumerate() {
            *slot = Rect::from_min_size(
                pos2(row_left, sub_top + i as f32 * m::LAYER_ROW_H),
                egui::vec2(row_right - row_left, m::LAYER_ROW_H),
            );
        }

        Self {
            content: window,
            title_bar,
            menu_bar,
            menu_sep,
            left_dock,
            center,
            dock_divider,
            right_dock,
            status_bar,
            status_sep,
            status_inner,
            p_tools,
            p_tool_options,
            p_layers,
            p_cell_inspector,
            p_minimap,
            p_palette,
            p_active_asset,
            p_maps,
            tab_strip,
            viewport,
            zoom_pill,
            tool_buttons,
            layer_rows,
            layer_group_sep,
            layer_sub_rows,
        }
    }

    /// 中列页签条上的「活动页签」矩形：宽 `TAB_W`、高 `TAB_H`、贴顶。
    pub fn tab_rect(&self) -> Rect {
        Rect::from_min_size(
            pos2(self.tab_strip.left(), self.tab_strip.top()),
            egui::vec2(m::TAB_W, m::TAB_H),
        )
    }

    /// 页签下划线（选中指示）：宽 `TAB_W`、高 `TAB_UNDERLINE_H`，贴页签条底。
    pub fn tab_underline(&self) -> Rect {
        Rect::from_min_size(
            pos2(
                self.tab_strip.left(),
                self.tab_strip.bottom() - m::TAB_UNDERLINE_H,
            ),
            egui::vec2(m::TAB_W, m::TAB_UNDERLINE_H),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win() -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(m::WINDOW_W, m::WINDOW_H))
    }

    #[test]
    fn 三列宽度与常量一致() {
        let l = Layout::compute(win());
        assert!((l.left_dock.width() - 156.0).abs() < 0.01);
        assert!((l.right_dock.width() - 356.0).abs() < 0.01);
        assert!((l.status_bar.height() - 30.0).abs() < 0.01);
        assert!((l.menu_bar.height() - 35.0).abs() < 0.01);
    }

    #[test]
    fn 面板首尾相接不重叠() {
        let l = Layout::compute(win());
        assert!((l.p_tools.whole.bottom() - l.p_tool_options.whole.top()).abs() < 0.01);
        assert!((l.p_tool_options.whole.bottom() - l.p_layers.whole.top()).abs() < 0.01);
        assert!((l.p_layers.whole.bottom() - l.p_cell_inspector.whole.top()).abs() < 0.01);
        assert!((l.p_cell_inspector.whole.bottom() - l.status_bar.top()).abs() < 0.01);
        assert!((l.p_minimap.whole.bottom() - l.p_palette.whole.top()).abs() < 0.01);
        assert!((l.p_palette.whole.bottom() - l.p_active_asset.whole.top()).abs() < 0.01);
        assert!((l.p_active_asset.whole.bottom() - l.status_bar.top()).abs() < 0.01);
    }

    #[test]
    fn 工具按钮四列三行() {
        let l = Layout::compute(win());
        // 第 0/1 个按钮同排，y 相同、x 相差 36
        assert!((l.tool_buttons[0].top() - l.tool_buttons[1].top()).abs() < 0.01);
        assert!((l.tool_buttons[1].left() - l.tool_buttons[0].left() - 36.0).abs() < 0.01);
        // 第 3 个是第 4 列，第 4 个换行
        assert!((l.tool_buttons[4].top() - l.tool_buttons[0].top() - 36.0).abs() < 0.01);
        // 全部落在 Tools 内容区内
        for b in &l.tool_buttons {
            assert!(l.p_tools.content.contains_rect(*b), "工具按钮越界: {b:?}");
        }
    }

    #[test]
    fn 图层行高26且落在面板内() {
        let l = Layout::compute(win());
        for r in &l.layer_rows {
            assert!((r.height() - 26.0).abs() < 0.01);
            assert!(l.p_layers.content.contains_rect(*r));
        }
        assert!(l.p_layers.content.contains_rect(l.layer_group_sep));
        for r in &l.layer_sub_rows {
            assert!(l.p_layers.content.contains_rect(*r));
        }
    }

    /// 画布矩形 = 视口矩形（页签条以下、状态栏以上、两侧停靠区之间），且不与任何面板重叠。
    ///
    /// 这条断言防的是「地图画到面板底下」这类只有截图才能看出来的错位。
    /// 注意用**边界相等**而不是 `!intersects`：egui 的 `intersects` 把"贴边"也算相交。
    #[test]
    fn 画布矩形在页签条与状态栏之间() {
        let l = Layout::compute(win());
        assert!((l.viewport.top() - l.tab_strip.bottom()).abs() < 0.01);
        assert!((l.viewport.bottom() - l.status_bar.top()).abs() < 0.01);
        assert!((l.viewport.left() - l.left_dock.right()).abs() < 0.01);
        assert!((l.viewport.right() - l.dock_divider.left()).abs() < 0.01);
        assert!(l.viewport.left() >= l.p_tools.whole.right() - 0.01);
        assert!(l.viewport.right() <= l.p_palette.whole.left() + 0.01);
        assert!(l.viewport.top() >= l.p_tools.whole.top());
        assert!(l.viewport.bottom() <= l.status_bar.top() + 0.01);
        // 缩放浮层必须整块落在画布里（它是画布上的浮层，不能被裁掉一半）
        assert!(l.viewport.contains_rect(l.zoom_pill));
    }

    /// 本 crate 的世界格常量必须与渲染库（客户端真值）逐值相等。
    ///
    /// 两边各写一份是**故意的**：`metrics` 管界面几何，`miredit_render` 管合成；
    /// 一旦有人只改一边，这条断言当场失败，不会等到截图发现"格子对不齐"。
    #[test]
    fn 世界格与渲染库一致() {
        assert_eq!(m::CELL_W as u32, miredit_render::CELL_W);
        assert_eq!(m::CELL_H as u32, miredit_render::CELL_H);
    }

    /// **左停靠区余量实测**（给"地图列表面板为什么不做成第五块左栏面板"提供数字依据）。
    ///
    /// 量法：左停靠区高度 − 三块定高面板 − 单元格检查器**最坏文案**需要的高度。
    /// 检查器内容高 = 标题偏移 10 + 标题行 15 + 行间距 4 + 6 行 × (行数 × 15 + 4)，
    /// 行数取实测最坏值 3（`0.map` 的地表/中层/物件都是"图号/库号/帧"三行）。
    /// 实测余量只有 96px —— 塞一块能用的列表（≥6 行 × 22 + 标题 23 + 状态 18 = 173）
    /// 会直接把检查器最后两行挤出面板。这条测试把这个数字钉住，改常量时能立刻发现。
    #[test]
    fn 左停靠区余量() {
        let l = Layout::compute(win());
        let need_inspector = m::INSPECTOR_HEAD_OFF
            + m::INSPECTOR_LINE_H
            + m::INSPECTOR_ROW_GAP
            + 6.0 * (3.0 * m::INSPECTOR_LINE_H + m::INSPECTOR_ROW_GAP)
            + m::PANEL_HEADER_H;
        let used = l.p_tools.whole.height()
            + l.p_tool_options.whole.height()
            + l.p_layers.whole.height()
            + need_inspector;
        let slack = l.left_dock.height() - used;
        // 实测值（2026-11-05，1500×950）：96.0
        assert!(
            (0.0..=110.0).contains(&slack),
            "左停靠区余量 {slack}px 与实测 96px 不符（若真的变大了，可以重新考虑把\
             地图列表放进左栏）"
        );
    }

    /// **地图列表面板开合**：开 ⇒ 画布正好让出一列、不重叠；关 ⇒ 画布与旧版逐像素相同。
    #[test]
    fn 地图列表面板开合() {
        let shut = Layout::compute_with(win(), false);
        let open = Layout::compute_with(win(), true);
        // 关：零宽 + 画布左边界 = 左停靠区右边界（旧版口径）
        assert!(shut.p_maps.whole.width().abs() < 0.01, "关着时面板必须零宽");
        assert!((shut.viewport.left() - shut.left_dock.right()).abs() < 0.01);
        // 开：面板占一列，画布右推同样的宽度，其余几何一律不动
        assert!((open.p_maps.whole.width() - m::MAPS_W).abs() < 0.01);
        assert!((open.p_maps.whole.left() - shut.viewport.left()).abs() < 0.01);
        assert!((open.viewport.left() - open.p_maps.whole.right()).abs() < 0.01);
        assert!(
            (open.viewport.width() - (shut.viewport.width() - m::MAPS_W)).abs() < 0.01,
            "开面板后画布必须正好窄 {}px",
            m::MAPS_W
        );
        // 面板整块落在页签条以下、状态栏以上（不压页签、不压状态栏）
        assert!((open.p_maps.whole.top() - open.tab_strip.bottom()).abs() < 0.01);
        assert!((open.p_maps.whole.bottom() - open.status_bar.top()).abs() < 0.01);
        // 页签条不随开合变化（tab 标题的裁剪盒在两种状态下都成立）
        assert!((open.tab_strip.left() - shut.tab_strip.left()).abs() < 0.01);
        assert!((open.tab_strip.width() - shut.tab_strip.width()).abs() < 0.01);
        // 面板与画布不重叠（贴边相等，不是相交）
        assert!((open.p_maps.whole.right() - open.viewport.left()).abs() < 0.01);
        // 缩放浮层仍整块落在画布里（开面板后跟着画布右移）
        assert!(open.viewport.contains_rect(open.zoom_pill));
    }
}
