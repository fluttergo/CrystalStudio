//! 矩形选区与剪贴板：**选择工具**（工具列第 5 个）+ 五个快捷键命令 + 状态栏回执。
//!
//! # 一、为什么「选择」是宿主层的工具，而不是 `miredit_core::Tool` 的一支
//!
//! 内核 `Tool` 的契约只覆盖 **4 个会改地图的工具** + `Unwired`（`paint.rs` 模块文档），
//! 而"选择"**不改地图**：它只写 `EditSession::sel`（UI 状态），改地图的是它后面的
//! `CopySelection` / `Paste` / `DeleteSelection` / `FillSelection` 四条命令。
//! 内核把这些命令单独提供（`session.rs` 的选区段），**没有**把它们塞进 `Tool` 的落笔分派。
//! 所以本模块在宿主层做两件事：
//!
//! 1. 把"拖动"折成 [`Selection`]（闭区间）并交给 `EditSession::set_selection`；
//! 2. 把快捷键折成 [`Cmd`] 并逐条调内核的选区 API。
//!
//! # 二、快捷键的官方依据（逐条点名，**没有依据的显式标注**）
//!
//! | 键 | 动作 | 官方依据 |
//! |---|---|---|
//! | `Ctrl+C` | 复制选区 | `CrystalStudio/RegionCopy.cs:101`（`CopySelection`）+ `MapViewport.cs` 的 `Key.C` 分支 |
//! | `Ctrl+X` | 剪切选区 | `RegionCopy.cs:102`（`CutSelection`） |
//! | `Ctrl+V` | 粘贴：传**鼠标格**，落点 = 按源奇偶重算的锚点（`Clipboard::anchor`）| `RegionCopy.cs:70`（`Anchor`）+ `:121-122`（先 `Anchor` 再 `CanPaste`）+ `:72-91`（`Paste`）+ 粘贴后提示 "Ctrl+V to paste again"（`:123`）|
//! | `Delete` | 删除选区内容 | `RegionCopy.cs:45-69`（`CutSource`）+ `MapViewport.cs:546`（`ClearSelection`）|
//! | `Ctrl+Shift+V` | **填充选区** | ⚠ **无官方依据** —— 官方填充只有"工具 `F` + 点击"（`MainWindow.axaml.cs:861`），`MapViewport.cs` 里没有给 `FillSelection` 绑任何键。本仓为选区补这一个键，显式记档（[`tx::SEL_KEYS_HINT`] 里也写了"本仓自定"）|
//!
//! # 三、回执只出一句，且**成功/拒绝分色**
//!
//! 内核每条选区命令都会把中文回执写进 `EditSession::note`（成功与拒绝都写）。
//! 本模块用 [`take_note`] 把它**取走**（取走后 `note` 置空），折成一条 [`Receipt`]：
//!
//! - `warn = false` ⇒ 成功回执，用正文色（与「填充 N 格」同一档）；
//! - `warn = true` ⇒ 拒绝原因，用警示粉（与「笔刷族不匹配」同一档）。
//!
//! 为什么要取走：`note` 同时被状态栏的「落笔字段」那一段消费（`status_hint`），
//! 若不取走，一句"删除选区内容 9 格"会**同时**顶掉落笔字段、又被染成警示色 ——
//! 那是"成功也报红"的假警报。
//!
//! # 四、选区不置脏，但粘贴/删除/填充会
//!
//! 内核已保证：`set_selection` / `copy_selection` **不碰地图、不进撤销栈、不动脏标记**
//! （`session.rs` 的选区段 + `selection_clipboard.rs::附_选区操作不置脏不产生撤销步`）。
//! 真正改地图的三条（粘贴/删除/填充）各产生**一步**撤销，撤销后逐字节回到操作前。
//!
//! ⚠ **`Ctrl+C` 不置脏**：复制只读地图、只写剪贴板，磁盘上没有任何东西需要保存。
//! 任务书 §四.5 把"复制"也列进了"会置脏"，与内核已定的契约冲突 ⇒ 本仓按内核
//! （复制不置脏）实现，并在交付汇报里点名这处不一致。

use mir2_map::Map;

use miredit_core::{Clipboard, PlaceOutcome, Selection};

use crate::edit::EditSession;
use crate::ui_text as tx;

/// 「选择」在工具图标表里的下标（= `icons::TOOLS[4]`，JSON `tag_value = "Select"`，
/// 官方工具提示 `Select · M`）。
///
/// ⚠ 这个数**不能**交给 `miredit_core::Tool::from_index`：内核那一支没有"选择"，
/// 会返回 `Unwired`。本模块用它做宿主层的分派（见 [`crate::app`]）。
pub const TOOL_SELECT_IDX: usize = 4;

/// 「印章」在工具图标表里的下标（= `icons::TOOLS[5]`，JSON `tag_value = "Stamp"`）。
///
/// 与 [`TOOL_SELECT_IDX`] 同理：内核没有这一支；真正落位走
/// [`crate::stamp_place::place`]（`register_stamp` + `place_stamp_by_id`）。
pub const TOOL_STAMP_IDX: usize = 5;

/// 一次拖动的起点（`None` = 当前没有在拖）。
///
/// 只存锚点，不存"当前点"：当前点每帧从鼠标算，存下来就多一份会过期的真值。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Drag {
    /// 按下时所在的格（选区的一端）。
    pub anchor: (u16, u16),
}

/// 两个格 → 矩形选区（**闭区间**，两端都含；反向拖动会被内核规范化）。
pub fn selection_of(anchor: (u16, u16), cur: (u16, u16)) -> Selection {
    Selection::new(anchor.0, anchor.1, cur.0, cur.1)
}

/// 一条选区命令（快捷键折出来的就是它）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmd {
    /// `Ctrl+C`：复制选区进剪贴板。
    Copy,
    /// `Ctrl+X`：剪切 = 复制 + 清空。
    Cut,
    /// `Ctrl+V`：粘贴 —— 传的是**鼠标格**，真正的左上角由内核按源奇偶重算（`Clipboard::anchor`）。
    Paste,
    /// `Delete`：删除选区内容（三族按层清）。
    Delete,
    /// `Ctrl+Shift+V`：用当前笔刷填满选区（**无官方快捷键依据**，见模块文档 §二）。
    Fill,
}

impl Cmd {
    /// 诊断/日志用的 ASCII 键（与 `Tool::key` 同一套纪律：脚本 grep 不碰中文）。
    pub const fn key(self) -> &'static str {
        match self {
            Cmd::Copy => "copy",
            Cmd::Cut => "cut",
            Cmd::Paste => "paste",
            Cmd::Delete => "delete",
            Cmd::Fill => "fill_selection",
        }
    }
}

/// 一次选区命令的宿主侧回执。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Receipt {
    /// 给状态栏的一句话（成功回执或拒绝原因，都是中文）。
    pub text: String,
    /// 是不是**拒绝**（决定状态栏那一段用警示色还是正文色）。
    pub warn: bool,
    /// 真的改了几格（`> 0` 才需要重渲画布）。
    pub changed: usize,
}

/// 把内核写进 `note` 的那一句**取走**（取走后 `note` 置空，避免状态栏两段都说同一件事）。
pub fn take_note(ed: &mut EditSession) -> Option<String> {
    let s = ed.note().map(str::to_owned);
    if s.is_some() {
        ed.clear_note();
    }
    s
}

/// 造一条**拒绝**回执：优先用内核刚写的原因，没有就退到调用方给的兜底句。
fn refused(ed: &mut EditSession, fallback: &str) -> Receipt {
    Receipt {
        text: take_note(ed).unwrap_or_else(|| fallback.to_owned()),
        warn: true,
        changed: 0,
    }
}

/// 执行一条选区命令；`cursor` = 当前鼠标所在格（只有粘贴用得上）。
///
/// 返回 `None` 只在"粘贴时鼠标不在图上"这一种情况出现 —— 此时**什么都没做**，
/// 连剪贴板都没动（不调内核，故没有 `note` 可取）。
pub fn apply(
    ed: &mut EditSession,
    map: &mut Map,
    cmd: Cmd,
    cursor: Option<(u16, u16)>,
) -> Option<Receipt> {
    match cmd {
        Cmd::Copy => Some(match ed.copy_selection(map) {
            Some((w, h)) => Receipt {
                text: tx::fill(
                    tx::SEL_COPY_TMPL,
                    &[("w", &w.to_string()), ("h", &h.to_string())],
                ),
                warn: false,
                changed: 0,
            },
            None => refused(ed, tx::SEL_ERR_NO_SELECTION),
        }),
        Cmd::Cut => Some(match ed.cut_selection(map) {
            Some((w, h)) => {
                // `last_clear` 是内核记的"这次真清了几格"（与回执里的数同源）
                let n = ed.last_clear().unwrap_or(0);
                Receipt {
                    text: tx::fill(
                        tx::SEL_CUT_TMPL,
                        &[("w", &w.to_string()), ("h", &h.to_string()), ("n", &n.to_string())],
                    ),
                    warn: false,
                    changed: n,
                }
            }
            None => refused(ed, tx::SEL_ERR_NO_SELECTION),
        }),
        Cmd::Paste => {
            // 鼠标不在图上 ⇒ 连内核都不调（返回 `None`，什么都没做）
            let (x, y) = cursor?;
            // 粘贴前先取剪贴板尺寸（粘贴后内核会把剪贴板放回，尺寸不变，但先取更直白）
            let dims = ed.clipboard().map(|c| (c.width(), c.height()));
            Some(match ed.paste_clipboard(map, x, y) {
                PlaceOutcome::Placed { cells, skipped } => {
                    let (w, h) = dims.unwrap_or((0, 0));
                    let mut text = tx::fill(
                        tx::SEL_PASTE_TMPL,
                        &[("w", &w.to_string()), ("h", &h.to_string())],
                    );
                    // 粘贴复用内核的印章放置：越界格逐格跳过，必须如实报出来
                    if skipped > 0 {
                        text.push_str(&tx::fill(
                            tx::SEL_PASTE_SKIP_TMPL,
                            &[("k", &skipped.to_string())],
                        ));
                    }
                    Receipt { text, warn: false, changed: cells }
                }
                PlaceOutcome::Refused { .. } => refused(ed, tx::SEL_ERR_EMPTY_CLIPBOARD),
            })
        }
        Cmd::Delete => Some({
            let n = ed.delete_selection(map);
            if n > 0 {
                Receipt {
                    text: tx::fill(tx::SEL_DELETE_TMPL, &[("n", &n.to_string())]),
                    warn: false,
                    changed: n,
                }
            } else {
                refused(ed, tx::SEL_ERR_NOTHING_TO_DELETE)
            }
        }),
        Cmd::Fill => Some({
            let n = ed.fill_selection(map);
            if n > 0 {
                Receipt {
                    text: tx::fill(tx::SEL_FILL_TMPL, &[("n", &n.to_string())]),
                    warn: false,
                    changed: n,
                }
            } else {
                refused(ed, tx::SEL_ERR_NOTHING_TO_FILL)
            }
        }),
    }
}

/// 状态栏「选区 / 剪贴板」那一段的文字（`None` = 两样都没有，整段不画）。
///
/// 三条模板互斥，都登记在 [`crate::ui_text::ALL`] 里：
/// 只有选区 ⇒ [`tx::STATUS_SEL_TMPL`]；两样都有 ⇒ [`tx::STATUS_SEL_CLIP_TMPL`]；
/// 只有剪贴板 ⇒ [`tx::STATUS_CLIP_TMPL`]。
///
/// 为什么要显示坐标而不是只显示 `W×H`：同一张图上"复制了哪一块"是用户下一步粘贴时
/// 唯一的参照（粘贴锚点 = 鼠标格**按源奇偶对齐后**的格，不是源位置，也不是点击点本身 ——
/// 见内核 `Clipboard::anchor` 与 `RegionCopy.cs:70`）。
pub fn status_text(sel: Option<Selection>, clip: Option<&Clipboard>) -> Option<String> {
    let clip_dims = clip.map(|c| (c.width(), c.height()));
    match (sel, clip_dims) {
        (None, None) => None,
        (Some(s), None) => Some(tx::fill(
            tx::STATUS_SEL_TMPL,
            &[
                ("w", &s.width().to_string()),
                ("h", &s.height().to_string()),
                ("x0", &s.x0.to_string()),
                ("y0", &s.y0.to_string()),
                ("x1", &s.x1.to_string()),
                ("y1", &s.y1.to_string()),
            ],
        )),
        (Some(s), Some((cw, ch))) => Some(tx::fill(
            tx::STATUS_SEL_CLIP_TMPL,
            &[
                ("w", &s.width().to_string()),
                ("h", &s.height().to_string()),
                ("x0", &s.x0.to_string()),
                ("y0", &s.y0.to_string()),
                ("x1", &s.x1.to_string()),
                ("y1", &s.y1.to_string()),
                ("cw", &cw.to_string()),
                ("ch", &ch.to_string()),
            ],
        )),
        (None, Some((cw, ch))) => Some(tx::fill(
            tx::STATUS_CLIP_TMPL,
            &[("cw", &cw.to_string()), ("ch", &ch.to_string())],
        )),
    }
}

/// 状态栏那一段该用哪个模板（`DynLabel` 要求 `tmpl` 登记在 `ui_text::ALL` 里）。
///
/// 与 [`status_text`] 的三条分支一一对应；两样都没有时返回 `None`（整段不画）。
pub fn status_tmpl(sel: Option<Selection>, clip: Option<&Clipboard>) -> Option<&'static str> {
    match (sel.is_some(), clip.is_some()) {
        (false, false) => None,
        (true, false) => Some(tx::STATUS_SEL_TMPL),
        (true, true) => Some(tx::STATUS_SEL_CLIP_TMPL),
        (false, true) => Some(tx::STATUS_CLIP_TMPL),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use miredit_core::EditLayer;

    /// 合成 8×8 `New14` 图：地表图号 = 序号 + 1，`btTiles` = 3。
    fn 造图(w: u16, h: u16) -> Map {
        let mut bytes = vec![0u8; mir2_map::HEADER_LEN + 14 * (w as usize * h as usize)];
        bytes[0..2].copy_from_slice(&w.to_le_bytes());
        bytes[2..4].copy_from_slice(&h.to_le_bytes());
        for i in 0..(w as usize * h as usize) {
            let base = mir2_map::HEADER_LEN + i * 14;
            bytes[base..base + 2].copy_from_slice(&((i as u16) + 1).to_le_bytes());
            bytes[base + 12] = 3;
        }
        Map::parse(&bytes).expect("合成图应能解析")
    }

    /// 工具下标就是官方工具表的第 5 / 第 6 个（0 基）。
    #[test]
    fn 工具下标是四五() {
        assert_eq!(TOOL_SELECT_IDX, 4, "选择 = 工具列第 5 个");
        assert_eq!(TOOL_STAMP_IDX, 5, "印章 = 工具列第 6 个");
    }

    /// 拖动两端 → 闭区间选区（反向拖动也被规范化到同一块）。
    #[test]
    fn 拖动折成闭区间选区() {
        let a = selection_of((10, 20), (12, 22));
        assert_eq!((a.x0, a.y0, a.x1, a.y1), (10, 20, 12, 22));
        assert_eq!((a.width(), a.height()), (3, 3));
        let b = selection_of((12, 22), (10, 20));
        assert_eq!((b.width(), b.height()), (3, 3), "反向拖动也是 3×3");
        assert_eq!(b.iter().count(), 9);
        // 单格拖动 ⇒ 1×1
        let c = selection_of((5, 5), (5, 5));
        assert_eq!((c.width(), c.height()), (1, 1));
    }

    /// **选区不置脏、不产生撤销步**（内核契约），复制也只动剪贴板。
    #[test]
    fn 选区与复制不置脏() {
        let mut map = 造图(8, 8);
        let mut ed = EditSession::default();
        let before = map.to_bytes();
        assert!(ed.set_selection(&mut map, Selection::new(1, 1, 3, 3)));
        assert_eq!(ed.selection(), Some(Selection::new(1, 1, 3, 3)));
        assert!(!ed.dirty(), "设选区不该置脏");
        assert_eq!(ed.undo_len(), 0, "设选区不该产生撤销步");
        // 复制：只写剪贴板
        let r = apply(&mut ed, &mut map, Cmd::Copy, None).expect("复制总有回执");
        assert_eq!(r.text, "已复制 3×3 格");
        assert!(!r.warn);
        assert_eq!(r.changed, 0);
        assert!(!ed.dirty(), "复制不该置脏（磁盘上没有要保存的东西）");
        assert_eq!(ed.undo_len(), 0);
        assert_eq!(map.to_bytes(), before, "复制一个字节都不许动地图");
        let c = ed.clipboard().expect("剪贴板应有内容");
        assert_eq!((c.width(), c.height()), (3, 3));
        assert_eq!(c.cells().len(), 9, "整块都抓（含空格）");
    }

    /// **没有选区**时四条命令都拒绝，且回执是中文原因（不是静默）。
    #[test]
    fn 无选区时命令被拒() {
        let mut map = 造图(8, 8);
        for cmd in [Cmd::Copy, Cmd::Cut, Cmd::Delete, Cmd::Fill] {
            let mut ed = EditSession::default();
            let r = apply(&mut ed, &mut map, cmd, Some((1, 1))).expect("非粘贴总有回执");
            assert!(r.warn, "{:?} 无选区必须算拒绝", cmd.key());
            assert!(r.text.contains("没有选区"), "{:?} 原因：{}", cmd.key(), r.text);
            assert_eq!(r.changed, 0);
        }
        // 空剪贴板粘贴：也拒绝
        let mut ed = EditSession::default();
        let r = apply(&mut ed, &mut map, Cmd::Paste, Some((1, 1))).expect("有鼠标格就有回执");
        assert!(r.warn);
        assert!(r.text.contains("剪贴板是空的"), "{}", r.text);
        // 鼠标不在图上：连内核都不调（返回 None，什么都不做）
        let mut ed = EditSession::default();
        assert!(apply(&mut ed, &mut map, Cmd::Paste, None).is_none());
    }

    /// **粘贴 / 删除 / 填充各产生一步撤销**，且撤销后逐字节回到操作前。
    #[test]
    fn 三条改地图的命令各一步撤销() {
        // ① 粘贴
        let mut map = 造图(8, 8);
        let before = map.to_bytes();
        let mut ed = EditSession::default();
        ed.set_selection(&mut map, Selection::new(0, 0, 2, 2));
        apply(&mut ed, &mut map, Cmd::Copy, None).unwrap();
        let r = apply(&mut ed, &mut map, Cmd::Paste, Some((4, 4))).unwrap();
        assert_eq!(r.changed, 9, "3×3 粘到空地 ⇒ 9 格都变");
        assert!(ed.dirty(), "粘贴必须置脏");
        assert_eq!(ed.undo_len(), 1, "整次粘贴 = 一步撤销");
        assert_eq!(ed.undo(&mut map), 9);
        assert_eq!(map.to_bytes(), before, "撤销后必须逐字节回到操作前");

        // ② 删除
        let mut map = 造图(8, 8);
        let before = map.to_bytes();
        let mut ed = EditSession::default();
        ed.set_selection(&mut map, Selection::new(1, 1, 3, 3));
        let r = apply(&mut ed, &mut map, Cmd::Delete, None).unwrap();
        assert_eq!(r.changed, 9, "9 格地表图号被清");
        assert_eq!(ed.undo_len(), 1, "整次删除 = 一步撤销");
        assert_eq!(ed.undo(&mut map), 9);
        assert_eq!(map.to_bytes(), before);

        // ③ 填充（先取一格做笔刷）
        let mut map = 造图(8, 8);
        let before = map.to_bytes();
        let mut ed = EditSession::default();
        ed.set_layer(EditLayer::Ground);
        assert!(ed.pick_at(&map, 0, 0).is_some(), "吸管取 (0,0) 做笔刷");
        ed.set_selection(&mut map, Selection::new(2, 2, 4, 4));
        let r = apply(&mut ed, &mut map, Cmd::Fill, None).unwrap();
        assert!(r.changed > 0, "填充必须真的改格");
        assert_eq!(ed.undo_len(), 1, "整次填充 = 一步撤销");
        ed.undo(&mut map);
        assert_eq!(map.to_bytes(), before);
    }

    /// **剪切**：只有源块被清，剪贴板拿到源内容。
    #[test]
    fn 剪切复制源块并清空源() {
        let mut map = 造图(8, 8);
        let src: Vec<u16> = Selection::new(1, 1, 3, 3)
            .iter()
            .map(|(x, y)| map.cell(x, y).unwrap().bk_img)
            .collect();
        let mut ed = EditSession::default();
        ed.set_selection(&mut map, Selection::new(1, 1, 3, 3));
        let r = apply(&mut ed, &mut map, Cmd::Cut, None).unwrap();
        assert!(!r.warn, "剪切成功不该是拒绝");
        assert_eq!(r.text, "剪切 3×3 格（清空 9 格）");
        assert_eq!(r.changed, 9);
        // 源块的地表图号被清成 0
        for (x, y) in Selection::new(1, 1, 3, 3).iter() {
            assert_eq!(map.cell(x, y).unwrap().bk_img, 0, "({x},{y}) 应被清空");
        }
        // 剪贴板里的三族图号与源逐格相同
        let clip = ed.clipboard().expect("剪贴板");
        for (i, c) in clip.cells().iter().enumerate() {
            assert_eq!(c.bk, src[i], "第 {i} 格地表图号应等于源");
        }
    }

    /// **状态栏三段文案**：只有选区 / 两者都有 / 只有剪贴板，两样都没有则整段不画。
    #[test]
    fn 状态栏选区文案三分支() {
        let sel = Selection::new(200, 200, 202, 202);
        assert_eq!(status_text(None, None), None);
        assert_eq!(status_tmpl(None, None), None);
        assert_eq!(
            status_text(Some(sel), None).as_deref(),
            Some("选区 3×3（200,200..202,202）")
        );
        assert_eq!(status_tmpl(Some(sel), None), Some(tx::STATUS_SEL_TMPL));

        // 造一份剪贴板（`from_cells` 是公开构造，网页层也用它）
        let clip = Clipboard::from_cells(
            3,
            3,
            vec![miredit_core::StampCellInput { dx: 0, dy: 0, bk: 5, ..Default::default() }; 9],
        )
        .expect("3×3 剪贴板应能构造");
        assert_eq!(
            status_text(Some(sel), Some(&clip)).as_deref(),
            Some("选区 3×3（200,200..202,202）· 剪贴板 3×3")
        );
        assert_eq!(
            status_tmpl(Some(sel), Some(&clip)),
            Some(tx::STATUS_SEL_CLIP_TMPL)
        );
        assert_eq!(status_text(None, Some(&clip)).as_deref(), Some("剪贴板 3×3"));
        assert_eq!(status_tmpl(None, Some(&clip)), Some(tx::STATUS_CLIP_TMPL));
    }

    /// 回执**取走** `note`：取走后 `note` 必须是空的（否则状态栏两段说同一件事）。
    #[test]
    fn 回执取走note() {
        let mut map = 造图(4, 4);
        let mut ed = EditSession::default();
        // 无选区复制 ⇒ 内核写 note
        let r = apply(&mut ed, &mut map, Cmd::Copy, None).unwrap();
        assert!(r.warn);
        assert!(ed.note().is_none(), "回执必须把 note 取走：{:?}", ed.note());
    }
}
