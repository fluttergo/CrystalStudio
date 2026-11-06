//! 帮助 → 键盘快捷键：**只列当前真实绑定**的只读浮层。
//!
//! # 为什么必须"只列真的"
//!
//! 官方 `KeyboardShortcuts` 对话框会列出它自己的全套键（含 `Ctrl+N` / `Ctrl+Shift+S` /
//! `Ctrl+Shift+N` 等）。本仓**没有**实现那些功能（见
//! [`crate::menu::model::State::Todo`]），把它们列出来就是**假信息**：
//! 用户照着按，什么都不会发生。
//!
//! ⇒ 本表与下列实现**逐条对应**，加一条必须先在代码里真的绑上：
//!
//! | 表里的行 | 实现位置 |
//! |---|---|
//! | `Ctrl+S` / `Ctrl+O` | [`crate::app::MireditApp::handle_keys`]（打开地图走 `Overlay::OpenMap`）|
//! | `Ctrl+Shift+S`（另存为）| 同上 → `dispatch_menu(Action::SaveAs)` → `Overlay::SaveAs`（2026-11-05 起真实现）|
//! | `Ctrl+Z` / `Ctrl+Shift+Z` / `Ctrl+Y` | 同上（重做同时接受官方键与本仓旧键）|
//! | `F` / `M` / `O` / `[` `]` / `Enter` | 同上 + [`crate::selection_ops`] + [`crate::stamp_place`] |
//! | `G`（单元格网格） | 同上 → [`crate::menu::action::apply`] → [`crate::grid`]（2026-11-05 起真实现）|
//! | `Ctrl+C/X/V` / `Delete` / `Ctrl+Shift+V` | [`crate::app::MireditApp::handle_sel_keys`] |
//! | 空格+拖动 / 中键拖动 / 滚轮 | [`crate::app::MireditApp::interact`] |
//! | 按住 `Alt` | [`crate::app::MireditApp::stamp_preview`] |
//! | `F1` / `Esc` | [`crate::app::MireditApp::handle_keys`] + [`crate::menu::handle_key_with`] |
//!
//! ⚠ 菜单自身的导航键（弹层展开时的 `↑ ↓ ← →` / `Enter` / `Esc`）**不列在这里**：
//! 它们只在弹层展开时有效（[`crate::menu::handle_key_with`]），不是全局绑定。
//! 弹层的交互契约（含 `Enter` 遇容器项只展开、`→` 进子菜单、`Esc` 逐级收起）写在
//! [`crate::menu`] 的模块文档里。

use egui::{pos2, vec2, Align2, Painter, Rect};

use crate::layout::Layout;
use crate::metrics as m;
use crate::paint as pt;
use crate::theme as t;
use crate::ui_text as tx;

/// 一条绑定：`键` + `做什么`。
#[derive(Clone, Copy, Debug)]
pub struct Row {
    /// 键（`Ctrl+S` / `滚轮` …）。
    pub keys: &'static str,
    /// 做什么（中文，来自 `ui_text`）。
    pub what: &'static str,
}

/// 一组绑定。
#[derive(Clone, Copy, Debug)]
pub struct Group {
    /// 组名。
    pub title: &'static str,
    /// 组内各行。
    pub rows: &'static [Row],
}

/// 全表（**只含真实绑定**，见模块文档）。
pub const GROUPS: &[Group] = &[
    Group {
        title: tx::SC_GROUP_FILE,
        rows: &[
            Row {
                keys: tx::KEY_CTRL_S,
                what: tx::SC_SAVE,
            },
            Row {
                // 官方 `Ctrl+Shift+S`：2026-11-05 起真绑定（打开另存为路径输入模态）
                keys: tx::KEY_CTRL_SHIFT_S,
                what: tx::SC_SAVE_AS,
            },
            Row {
                keys: tx::KEY_CTRL_O,
                what: tx::SC_OPEN,
            },
        ],
    },
    Group {
        title: tx::SC_GROUP_EDIT,
        rows: &[
            Row {
                keys: tx::KEY_CTRL_Z,
                what: tx::SC_UNDO,
            },
            Row {
                keys: tx::KEY_CTRL_SHIFT_Z,
                what: tx::SC_REDO,
            },
        ],
    },
    Group {
        title: tx::SC_GROUP_VIEW,
        rows: &[Row {
            keys: tx::KEY_G,
            what: tx::SC_TOGGLE_GRID,
        }],
    },
    Group {
        title: tx::SC_GROUP_TOOLS,
        rows: &[
            Row {
                keys: tx::KEY_F,
                what: tx::SC_TOOL_FILL,
            },
            Row {
                keys: tx::KEY_M,
                what: tx::SC_TOOL_SELECT,
            },
            Row {
                keys: tx::KEY_O,
                what: tx::SC_TOOL_STAMP,
            },
            Row {
                keys: tx::KEY_BRACKETS,
                what: tx::SC_BRUSH_SIZE,
            },
            Row {
                keys: tx::KEY_ENTER,
                what: tx::SC_ARM_STAMP,
            },
        ],
    },
    Group {
        title: tx::SC_GROUP_SELECTION,
        rows: &[
            Row {
                keys: tx::KEY_CTRL_C,
                what: tx::SC_COPY,
            },
            Row {
                keys: tx::KEY_CTRL_X,
                what: tx::SC_CUT,
            },
            Row {
                keys: tx::KEY_CTRL_V,
                what: tx::SC_PASTE,
            },
            Row {
                keys: tx::KEY_DELETE,
                what: tx::SC_DELETE,
            },
            Row {
                keys: tx::KEY_CTRL_SHIFT_V,
                what: tx::SC_FILL_SEL,
            },
        ],
    },
    Group {
        title: tx::SC_GROUP_MOUSE,
        rows: &[
            Row {
                keys: tx::KEY_SPACE_DRAG,
                what: tx::SC_PAN,
            },
            Row {
                keys: tx::KEY_MIDDLE_DRAG,
                what: tx::SC_PAN,
            },
            Row {
                keys: tx::KEY_WHEEL,
                what: tx::SC_WHEEL_ZOOM,
            },
            Row {
                keys: tx::KEY_ALT,
                what: tx::SC_ALT_OUTLINE,
            },
            Row {
                keys: tx::KEY_F1,
                what: tx::SC_OPEN_OVERLAY,
            },
            Row {
                keys: tx::KEY_ESC,
                what: tx::SC_ESC,
            },
        ],
    },
];

/// 全表行数（标题 + 各行）。
pub fn line_count() -> usize {
    GROUPS
        .iter()
        .map(|g| 1 + g.rows.len())
        .sum()
}

/// 浮层矩形（居中）。
pub fn rect(l: &Layout) -> Rect {
    Rect::from_center_size(
        l.content.center(),
        vec2(m::SHORTCUTS_W, m::SHORTCUTS_H),
    )
}

/// 画浮层（遮罩 + 面板 + 标题 + 说明 + 各组）。
pub fn draw(p: &Painter, l: &Layout) {
    let pal = t::ui();
    // 遮罩：压住整个窗口（浮层是"只读对话框"，底下的东西不该被误点）
    pt::fill(p, l.content, pal.modal_scrim);
    let r = rect(l);
    pt::fill(p, r, pal.panel_bg);
    pt::border(p, r, pal.menu_popup_border);

    let inner_l = r.left() + m::SHORTCUTS_PAD;
    let mut y = r.top() + m::SHORTCUTS_PAD;

    // 标题
    pt::text(
        p,
        pos2(inner_l, y + m::SHORTCUTS_TITLE_H * 0.5),
        Align2::LEFT_CENTER,
        tx::SHORTCUTS_TITLE,
        m::PANEL_TITLE_FONT,
        pal.text,
    );
    y += m::SHORTCUTS_TITLE_H;
    // 说明
    pt::text(
        p,
        pos2(inner_l, y + m::SHORTCUTS_NOTE_H * 0.5),
        Align2::LEFT_CENTER,
        tx::SHORTCUTS_NOTE,
        m::BODY_FONT,
        pal.text_muted,
    );
    y += m::SHORTCUTS_NOTE_H;
    // 标题下 1px 分隔线
    pt::fill(
        p,
        Rect::from_min_size(pos2(inner_l, y), vec2(r.width() - m::SHORTCUTS_PAD * 2.0, 1.0)),
        pal.separator,
    );
    y += 6.0;

    for g in GROUPS {
        // 组名（强调色，比正文更显眼）
        pt::text(
            p,
            pos2(inner_l, y + m::SHORTCUTS_ROW_H * 0.5),
            Align2::LEFT_CENTER,
            g.title,
            m::BODY_FONT,
            pal.accent_dark,
        );
        y += m::SHORTCUTS_ROW_H;
        for row in g.rows {
            pt::text(
                p,
                pos2(inner_l + 10.0, y + m::SHORTCUTS_ROW_H * 0.5),
                Align2::LEFT_CENTER,
                row.keys,
                m::BODY_FONT,
                pal.text,
            );
            pt::text(
                p,
                pos2(inner_l + m::SHORTCUTS_KEY_COL_W, y + m::SHORTCUTS_ROW_H * 0.5),
                Align2::LEFT_CENTER,
                row.what,
                m::BODY_FONT,
                pal.text_muted,
            );
            y += m::SHORTCUTS_ROW_H;
        }
        y += 4.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// 表必须放得进浮层（**这是"列出真实绑定"的物理约束**：放不下就会被裁掉）。
    #[test]
    fn 表放得进浮层() {
        let need = m::SHORTCUTS_PAD * 2.0
            + m::SHORTCUTS_TITLE_H
            + m::SHORTCUTS_NOTE_H
            + 6.0
            + line_count() as f32 * m::SHORTCUTS_ROW_H
            + GROUPS.len() as f32 * 4.0;
        assert!(
            need <= m::SHORTCUTS_H,
            "快捷键表要 {need}px，浮层只有 {}px（会被裁掉）",
            m::SHORTCUTS_H
        );
    }

    /// 每一行的键与说明都非空，且键**不重复**（重复说明有两处绑同一个键，必然有一处是错的）。
    #[test]
    fn 键唯一且说明非空() {
        let mut keys = BTreeSet::new();
        for g in GROUPS {
            assert!(!g.title.is_empty());
            for row in g.rows {
                assert!(!row.keys.is_empty(), "{} 组里有空键", g.title);
                assert!(!row.what.is_empty(), "键 {} 没有说明", row.keys);
                assert!(keys.insert(row.keys), "键 {} 重复登记", row.keys);
            }
        }
        assert!(keys.len() >= 15, "真实绑定至少 15 条，现在只有 {}", keys.len());
    }

    /// **官方有、本仓没实现的键不许出现在表里**（列了就是假信息）。
    #[test]
    fn 不列未实现的官方键() {
        // 这些键官方有，但本仓对应功能仍是 Todo（见 `menu::model::state`）
        for forbidden in ["Ctrl+N", "Ctrl+Shift+N"] {
            let hit = GROUPS
                .iter()
                .flat_map(|g| g.rows.iter())
                .any(|r| r.keys == forbidden);
            assert!(!hit, "表里不该出现未实现的键：{forbidden}");
        }
        // 反向：真的实现了的必须列出来
        // （`G` = 视图 → 单元格网格 2026-11-05 起真实现；`Ctrl+Shift+S` = 另存为同日真实现）
        for must in [
            "Ctrl+S",
            "Ctrl+Shift+S",
            "Ctrl+Z",
            "G",
            "F",
            "M",
            "O",
            "Ctrl+C",
            "Delete",
            "F1",
            "Esc",
        ] {
            let hit = GROUPS
                .iter()
                .flat_map(|g| g.rows.iter())
                .any(|r| r.keys == must);
            assert!(hit, "真实绑定 {must} 没列进表");
        }
    }
}
