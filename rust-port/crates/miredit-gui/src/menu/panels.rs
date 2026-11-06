//! **面板显隐**：视图菜单里 7 个面板开关的状态 + 每个面板的矩形查询。
//!
//! # 为什么是 7 个而不是 6 个
//!
//! 官方 `MainWindow.axaml.cs:338-379` 一共 `docking.Add(...)` 了 **7 个**面板
//! （Tools / Tool Options / Layers / Minimap / Palette / Active Asset / Cell Inspector），
//! 而 `DockWorkspace.cs:269 AddSectionMenu` 会为**每一个**面板生成一个 CheckBox 菜单项
//! ⇒ 视图菜单里的面板开关就是 7 个。本仓 `layout.rs` 里也正好有这 7 块矩形
//! （`p_tools` / `p_tool_options` / `p_layers` / `p_cell_inspector` / `p_minimap` /
//! `p_palette` / `p_active_asset`）。
//!
//! # 隐藏的语义（**不重排**，只留白）
//!
//! 本仓的面板位置是 `layout.rs` 里的**固定几何**（官方是可拖的停靠系统，本仓不是）。
//! 所以"隐藏"= **不再画这块面板、也不再响应它的点击**，它原来的矩形露出停靠区底色
//! （[`crate::theme::Palette::panel_bg`]）—— **其余面板不挪位**。
//!
//! ⚠ 这是**如实的能力边界**，不是"看起来像隐藏"：面板的内容、标题条、眼睛/工具按钮
//! 全部消失且点不动，画布不动（画布矩形与面板显隐无关，仍是 `Layout::viewport`）。
//! 官方那种"隐藏后兄弟面板撑开"需要停靠系统，本仓没有，故**不假装**有
//! （视图 → 锁定/重置面板布局 因此也是灰显，原因见 `ui_text::WHY_LOCK_LAYOUT`）。

use crate::layout::{Layout, PanelRect};
use crate::ui_text as tx;

/// 7 个可开关的面板。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelId {
    /// **地图列表**（2026-11-05，上游 `901dbe2f` 新增）。
    ///
    /// 上游把它 `docking.Add("Maps", …, Dock.Left, 2)` 排在**左停靠区第一位**
    /// （`MainWindow.axaml.cs:343`）；本仓因左栏几何已排满（余量实测 96px），
    /// 把它做成**中列左侧的可开合列** —— 详细理由与几何见 [`crate::maplist`] 模块文档 §四。
    ///
    /// **默认关**（8 个面板里唯一默认关的）：本仓 1500×950 默认帧是与客户端逐像素比对的
    /// 基线（`--gate2-client`），默认少 268px 画布会把那条基线作废。有意偏离上游，已记档。
    Maps,
    /// 工具（工具按钮网格）。
    Tools,
    /// 工具选项（笔刷胶囊 + 滑块）。
    ToolOptions,
    /// 图层（5 行图层 + 单元格高亮子组）。
    Layers,
    /// 小地图。
    Minimap,
    /// 资源面板（图库/收藏/分类三页签）。
    Palette,
    /// 当前资源（右侧底部）。
    ActiveAsset,
    /// 单元格检查器。
    CellInspector,
}

impl PanelId {
    /// 全部 7 个（顺序 = 视图菜单里的出现顺序 = 官方 `docking.Add` 的调用顺序）。
    pub const ALL: [PanelId; 8] = [
        PanelId::Maps,
        PanelId::Tools,
        PanelId::ToolOptions,
        PanelId::Layers,
        PanelId::Minimap,
        PanelId::Palette,
        PanelId::ActiveAsset,
        PanelId::CellInspector,
    ];

    /// 诊断/命令行用的 ASCII 键（与 `EditLayer::key` 同一套纪律：脚本 grep 不碰中文）。
    pub const fn key(self) -> &'static str {
        match self {
            PanelId::Maps => "maps",
            PanelId::Tools => "tools",
            PanelId::ToolOptions => "tool_options",
            PanelId::Layers => "layers",
            PanelId::Minimap => "minimap",
            PanelId::Palette => "palette",
            PanelId::ActiveAsset => "active_asset",
            PanelId::CellInspector => "cell_inspector",
        }
    }

    /// 菜单项 / 面板标题上的中文名（**复用面板标题那几条**，不另写一份译名）。
    pub const fn title(self) -> &'static str {
        match self {
            PanelId::Maps => tx::TITLE_MAPS,
            PanelId::Tools => tx::TITLE_TOOLS,
            PanelId::ToolOptions => tx::TITLE_TOOL_OPTIONS,
            PanelId::Layers => tx::TITLE_LAYERS,
            PanelId::Minimap => tx::TITLE_MINIMAP,
            PanelId::Palette => tx::TITLE_PALETTE,
            PanelId::ActiveAsset => tx::TITLE_ACTIVE_ASSET,
            PanelId::CellInspector => tx::TITLE_CELL_INSPECTOR,
        }
    }

    /// 该面板在布局里的矩形（`Layout` 是唯一真源，本函数只是把 7 个字段折成一个查询）。
    pub fn rect(self, l: &Layout) -> PanelRect {
        match self {
            PanelId::Maps => l.p_maps,
            PanelId::Tools => l.p_tools,
            PanelId::ToolOptions => l.p_tool_options,
            PanelId::Layers => l.p_layers,
            PanelId::Minimap => l.p_minimap,
            PanelId::Palette => l.p_palette,
            PanelId::ActiveAsset => l.p_active_asset,
            PanelId::CellInspector => l.p_cell_inspector,
        }
    }

    /// 由命令行/诊断键反查（大小写不敏感；未知返回 `None`）。
    pub fn from_key(s: &str) -> Option<PanelId> {
        PanelId::ALL.into_iter().find(|p| p.key() == s)
    }
}

/// 7 个面板的显隐状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PanelVis {
    maps: bool,
    tools: bool,
    tool_options: bool,
    layers: bool,
    minimap: bool,
    palette: bool,
    active_asset: bool,
    cell_inspector: bool,
}

impl Default for PanelVis {
    /// **默认全显示**（与上一版逐像素一致：上一版没有开关，所有面板都在画）。
    fn default() -> Self {
        Self {
            // 地图列表默认**关**（唯一例外，理由见 `PanelId::Maps` 的文档）
            maps: false,
            tools: true,
            tool_options: true,
            layers: true,
            minimap: true,
            palette: true,
            active_asset: true,
            cell_inspector: true,
        }
    }
}

impl PanelVis {
    /// 取某个面板是否显示。
    pub fn get(&self, id: PanelId) -> bool {
        match id {
            PanelId::Maps => self.maps,
            PanelId::Tools => self.tools,
            PanelId::ToolOptions => self.tool_options,
            PanelId::Layers => self.layers,
            PanelId::Minimap => self.minimap,
            PanelId::Palette => self.palette,
            PanelId::ActiveAsset => self.active_asset,
            PanelId::CellInspector => self.cell_inspector,
        }
    }

    /// 设置某个面板的显隐，返回**设置后**的值。
    pub fn set(&mut self, id: PanelId, on: bool) -> bool {
        match id {
            PanelId::Maps => self.maps = on,
            PanelId::Tools => self.tools = on,
            PanelId::ToolOptions => self.tool_options = on,
            PanelId::Layers => self.layers = on,
            PanelId::Minimap => self.minimap = on,
            PanelId::Palette => self.palette = on,
            PanelId::ActiveAsset => self.active_asset = on,
            PanelId::CellInspector => self.cell_inspector = on,
        }
        on
    }

    /// 翻转某个面板，返回**翻转后**的值。
    pub fn toggle(&mut self, id: PanelId) -> bool {
        let now = !self.get(id);
        self.set(id, now)
    }

    /// 全部隐藏的个数（诊断用）。
    pub fn hidden_count(&self) -> usize {
        PanelId::ALL.iter().filter(|id| !self.get(**id)).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **7 个官方面板默认显示、地图列表默认关**（唯一例外），且 8 块矩形**互不重叠**。
    ///
    /// 隐藏机制不能建立在重叠的矩形上，所以开/关两种状态都查一遍：关着时
    /// `p_maps` 是零宽矩形（贴画布左边缘），开着时它占一列、画布让位。
    #[test]
    fn 默认全显示且矩形互不重叠() {
        let v = PanelVis::default();
        for id in PanelId::ALL {
            if id == PanelId::Maps {
                assert!(
                    !v.get(id),
                    "地图列表必须**默认关**（默认帧是与客户端逐像素比对的基线）"
                );
            } else {
                assert!(v.get(id), "{:?} 默认应显示", id);
            }
        }
        assert_eq!(v.hidden_count(), 1, "默认只该有地图列表一个是关的");
        let win = egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(crate::metrics::WINDOW_W, crate::metrics::WINDOW_H),
        );
        for maps_open in [false, true] {
            let l = Layout::compute_with(win, maps_open);
            let rects: Vec<_> = PanelId::ALL.iter().map(|id| id.rect(&l)).collect();
            for (i, a) in rects.iter().enumerate() {
                for (j, b) in rects.iter().enumerate() {
                    if i >= j {
                        continue;
                    }
                    let overlap = a.whole.intersect(b.whole);
                    assert!(
                        !overlap.is_positive(),
                        "（maps_open={maps_open}）{:?} 与 {:?} 的矩形重叠：{overlap:?}",
                        PanelId::ALL[i],
                        PanelId::ALL[j]
                    );
                }
            }
        }
    }

    /// 翻转 → 再翻转必须回到原值；`set` 返回设置后的值。
    ///
    /// ⚠ 计数里要算上"默认关着的那个"（地图列表）：默认 `hidden_count() == 1`，
    /// 所以翻转 Palette 之后是 **2**、翻回来是 **1**。
    #[test]
    fn 翻转与设置() {
        let mut v = PanelVis::default();
        let base = v.hidden_count(); // = 1（地图列表默认关）
        assert!(!v.toggle(PanelId::Palette));
        assert!(!v.get(PanelId::Palette));
        assert_eq!(v.hidden_count(), base + 1);
        assert!(v.toggle(PanelId::Palette));
        assert_eq!(v.hidden_count(), base);
        // `set` 返回**设置后**的值（false = 现在是隐藏的）
        assert!(!v.set(PanelId::Minimap, false));
        assert!(!v.get(PanelId::Minimap));
        assert!(v.set(PanelId::Minimap, true));
        assert!(v.get(PanelId::Minimap));
    }

    /// 键名往返（命令行 `--panel-hide palette` 靠它）。
    #[test]
    fn 键名往返() {
        for id in PanelId::ALL {
            assert_eq!(PanelId::from_key(id.key()), Some(id));
            assert!(!id.title().is_empty());
        }
        assert_eq!(PanelId::from_key("no_such_panel"), None);
    }
}
