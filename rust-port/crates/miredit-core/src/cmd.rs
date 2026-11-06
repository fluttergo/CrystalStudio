//! 可序列化的编辑命令（**给网页版用**）：`Command` / `Outcome` / [`apply`]。
//!
//! # 一、为什么命令是"数据"而不是"方法调用"
//!
//! PC 原生版直接调 [`crate::EditSession`] 的方法；网页版（wasm/JS）手里只有一条
//! JSON 之类的**消息**。所以把"用户做了什么"固化成 [`Command`]：
//! 网页层把消息翻成 `Command` → 调 [`apply`] → 拿 [`Outcome`] 回话。
//!
//! `Command` **只依赖 `std`**（任务书 §五）：本 crate 不引 serde，序列化由网页层自己做。
//! 于是"命令集"成了两边唯一需要对齐的契约，而且它小到能一眼看完。
//!
//! # 二、命令 ↔ 会话方法的对照（一条命令只做一件事）
//!
//! | 命令 | 落到 | 语义来源 |
//! |---|---|---|
//! | [`Command::SetLayer`] | [`EditSession::set_layer`] | `edit.rs:766-773`；层名走 [`EditLayer::from_key`]（不认识的**报错**，不猜） |
//! | [`Command::SetTool`] | [`EditSession::set_tool`] | `edit.rs:757-760`；工具名走 [`Tool::from_key`]（只收 4 个已接线键，不认识的**不改状态**） |
//! | [`Command::SetBrushSize`] | [`EditSession::set_brush_size`] | `edit.rs:784-787`（夹进 3..=31 的奇数） |
//! | [`Command::Paint`] | [`EditSession::paint_stroke_once`] | `edit.rs:977-982`（**吃笔刷尺寸**，按下即松开） |
//! | [`Command::Stroke`] | [`EditSession::paint_stroke`] | `edit.rs:988-1002`（Bresenham 插值，整条轨迹一步） |
//! | [`Command::Fill`] | [`EditSession::fill`] | `edit.rs:1036-1206` |
//! | [`Command::PlaceStamp`] | [`EditSession::place_stamp_by_id`] | `CrystalStudio/PlacedStamps.cs:114-121`（逐格 `MergeStamp` + 一步撤销）；越界/格式闸门见 [`crate::stamp`] 模块文档 |
//! | [`Command::Pick`] | [`EditSession::pick_at`] | `edit.rs:1234-1252` |
//! | [`Command::SetSelection`] | [`EditSession::set_selection`] | `MapViewport.cs:390`、`:399-400`（越界裁剪） |
//! | [`Command::ClearSelection`] | [`EditSession::clear_selection`] | `MapViewport.cs:103-108`（`Deselect`，**不清剪贴板**） |
//! | [`Command::CopySelection`] | [`EditSession::copy_selection`] | `RegionCopy.cs:101`、`:21-27`（整块都抓，含空格） |
//! | [`Command::CutSelection`] | [`EditSession::cut_selection`] | `RegionCopy.cs:102`、`:108`、`:45-69`（复制 + 清空，一步撤销） |
//! | [`Command::PasteSelection`] | [`EditSession::paste_clipboard`] | `RegionCopy.cs:72-91`（复用 `stamp::plan_stamp`；逐格越界跳过）；`:70` 锚点按源奇偶重算、负锚点整块拒绝 |
//! | [`Command::DeleteSelection`] | [`EditSession::delete_selection`] | `RegionCopy.cs:45-69`（三族按层清 + 库号写 0） |
//! | [`Command::FillSelection`] | [`EditSession::fill_selection`] | `MapViewport.cs:540`（地表取 `step=1`，见 `selection.rs` §五） |
//! | [`Command::Undo`] / [`Command::Redo`] | [`EditSession::undo`] / [`EditSession::redo`] | `edit.rs:1289-1323`（**一次一步**，与 GUI 的 `--edit-undo N` 循环等价） |
//! | [`Command::Save`] | [`crate::save_session`] | `save.rs:289-330` |
//!
//! # 三、`changed == 0` 时必须有 `note`
//!
//! 任务书 §七：「拒绝必须返回明确 outcome，不许静默不改」。所以本模块的每条命令在
//! `changed == 0` 时都**一定**带一句原因（族不匹配 / 图库笔刷落阻挡层 / 越界 / 没笔刷 /
//! 值没变 / 没有可撤销的步骤 / 没有未保存的修改）。判据在
//! [`EditSession::paint_note`] 与 [`FillOutcome::note`] 里，本模块只负责调用与组装。
//!
//! # 四、`Err` 只在"调用没完成"时出现
//!
//! - 层名不认识 ⇒ [`CoreError::BadLayer`]（猜一个最像的层就是把字节写进别的字段）；
//! - 保存失败 ⇒ [`CoreError::Save`]（带阶段、路径、临时文件、系统原文）。
//!
//! 其余一律 `Ok(Outcome)`，因为"被拒绝"是**语义结果**而不是程序故障
//! （见 `error.rs` 模块文档的对照表）。
//!
//! ## 为什么工具名不合法**不**走 `Err`（与层名不合法相反）
//!
//! 层名猜错会**写错字段**（把地表图号写进 `mid_img`），那是数据损坏，必须硬失败；
//! 工具名猜错**一个字段都不写**（工具只决定"下一次落笔按哪条分支"），
//! 它只是"这次调用没生效" ⇒ 走 `Ok(Outcome { changed: 0, note })`，
//! 调用方还能同时拿到 `dirty` / `can_undo` / `can_redo` 三个状态。
//! 这也是**不加 `CoreError` 变体**的原因：`miredit-web/src/error.rs:117-128`
//! 对 `CoreError` 做了穷尽 match，新变体会让那个 crate 编译不过（本单不许改它）。

use crate::brush::LibRef;
use crate::error::CoreError;
use crate::fill::FillOutcome;
use crate::layer::EditLayer;
use crate::paint::Tool;
use crate::save::{self, SaveOutcome};
use crate::selection::Selection;
use crate::session::EditSession;
use crate::stamp::PlaceOutcome;

/// 一条编辑命令（**可序列化**：字段只有 `u16` / `String` / 元组，网页层自己转）。
///
/// 变体与字段按任务书 §五逐条落，**没有增删**。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// 切换当前编辑层。
    ///
    /// `layer` 只接受 `ground` / `middle` / `front` / `low_collision` / `high_collision`
    /// （[`EditLayer::key`] 的五个值）；其它字符串 ⇒ [`CoreError::BadLayer`]。
    SetLayer {
        /// 层的 ASCII 键。
        layer: String,
    },
    /// 设笔刷尺寸（会被夹进 3..=31 的奇数，回执里给真正生效的值）。
    SetBrushSize {
        /// 请求的尺寸（任意值，越界/偶数都会被规范化）。
        size: u16,
    },
    /// 切换当前工具。
    ///
    /// `tool` 只接受 [`Tool::key`] 的**四个已接线**值：`brush` / `bucket` / `eraser` / `pickup`
    /// （[`Tool::from_key`] 的词汇表，2026-11-05 契约裁定：`unwired` 与任何别名一律不收）。
    ///
    /// 非法键 ⇒ **`Ok(Outcome { changed: 0, note: Some(中文原因) })`** 且**不改会话状态**
    /// （工具、撤销栈、脏标记一个都不动）。为什么不是 `Err`：见模块文档第四节 ——
    /// "没认出来"不是程序故障，调用方要能同时拿到 `dirty` / `can_undo` 等状态。
    SetTool {
        /// 工具的 ASCII 键（四个之一）。
        tool: String,
    },
    /// 以 `(x, y)` 为锚点放置一枚**已注册**的印章（**整次放置 = 一步撤销**）。
    ///
    /// `stamp` 是**印章 id**，不是文件路径：本 crate 不读盘（不依赖 `mir2-stamp`），
    /// 格表由上层先调 [`EditSession::register_stamp`] 注册；id 不存在 ⇒
    /// `Outcome { changed: 0, note: "未注册的印章 id：…" }`（原因里列出已注册的 id）。
    ///
    /// 逐格语义（越界裁剪 / 三族图号 / `bit15` 保留 / Type5 闸门）全在 [`crate::stamp`]。
    PlaceStamp {
        /// 锚点世界格 X。
        x: u16,
        /// 锚点世界格 Y。
        y: u16,
        /// 印章 id（[`EditSession::register_stamp`] 注册时用的那个键）。
        stamp: String,
    },
    /// 落一笔（**按当前笔刷尺寸**，= 鼠标"按下即松开"）。
    Paint {
        /// 世界格 X。
        x: u16,
        /// 世界格 Y。
        y: u16,
    },
    /// 拖一笔（Bresenham 插值，整条轨迹按当前笔刷尺寸落，**一步撤销**）。
    Stroke {
        /// 起点。
        from: (u16, u16),
        /// 终点。
        to: (u16, u16),
    },
    /// 擦一笔（**按当前笔刷尺寸**，= 鼠标"按下即松开" + 橡皮工具）。
    ///
    /// 语义与 [`EditSession::touch`] 在 `tool == Eraser` 时完全一致：
    /// 图号层 `img &= 0x8000`（保留阻挡位）、阻挡层 `img &= !0x8000`（保留图号）。
    /// 执行后**当前工具留在橡皮**（与 GUI 的 `--edit-erase` 一致：那条路径也是
    /// `select(Eraser)` 之后不再切回）——网页版若要"擦完回画笔"，自己再发一条
    /// 切工具的消息即可，命令本身不做隐式状态切换。
    Erase {
        /// 世界格 X。
        x: u16,
        /// 世界格 Y。
        y: u16,
    },
    /// 填充（BFS 连通块，**整次填充 = 一步撤销**）。
    Fill {
        /// 世界格 X。
        x: u16,
        /// 世界格 Y。
        y: u16,
    },
    /// 吸管（把该格当前层的内容取成笔刷；不改地图、不进撤销栈）。
    Pick {
        /// 世界格 X。
        x: u16,
        /// 世界格 Y。
        y: u16,
    },
    /// **设矩形选区**（闭区间；越界会被裁剪到图内）。
    ///
    /// 语义来源：`CrystalStudio/MapViewport.cs:390`（起点必须图内）、`:399`（终点 `Math.Clamp`）、
    /// `:400`（宽高 = 端点差 + 1）。裁到全空 ⇒ 选区被取消，`changed == 0` + 中文原因。
    ///
    /// ⚠ 选区是 **UI 状态**：这条命令**不改地图、不置脏、不产生撤销步**
    /// （C# 把 `Selection` 与 `History` 并列存，`:147`/`:152`）。
    SetSelection {
        /// 左端列（含）。
        x0: u16,
        /// 上端行（含）。
        y0: u16,
        /// 右端列（**含**）。
        x1: u16,
        /// 下端行（**含**）。
        y1: u16,
    },
    /// **取消选区**（`Deselect()`，`MapViewport.cs:103-108`）。**不清剪贴板**。
    ClearSelection,
    /// **复制选区进剪贴板**（整块都抓，含空格）。来源：`RegionCopy.cs:101`、`:21-27`。
    ///
    /// `changed == 0`（不改地图）；成功时 `note` 给「已复制 W×H 格」，失败时给中文原因
    /// （没有选区 / 选区为空）。
    CopySelection,
    /// **剪切选区** = 复制 + 清空（**整次 = 一步撤销**）。来源：`RegionCopy.cs:102`、`:108`、`:45-69`。
    ///
    /// `changed` = 真的清掉的格数。
    CutSelection,
    /// **粘贴剪贴板**，`(x, y)` = **点击格**（鼠标格），不是粘贴左上角。
    /// 来源：`RegionCopy.cs:121-122`（先 `Anchor` 再 `CanPaste`）+ `:72-91`（`Paste`）。
    ///
    /// 真正的左上角由 [`crate::Clipboard::anchor`] 按上游 `RegionCopy.cs:70` 的公式重算：
    /// 复制时地表可见 ⇒ 按**源的奇偶**往左上对齐；算出来为负 ⇒ **整块拒绝**（`Refused`，一格不写）。
    /// 无源剪贴板（[`crate::Clipboard::from_cells`] 造的）与"复制时地表不可见"⇒ 不重算。
    ///
    /// 逐格越界跳过（`note` 里报 `skipped`）、三族图号为 0 的族不写、保留目标格 `bit15`、
    /// Type5 闸门（剪贴板里有 `bk≠0` ⇒ 拒绝），整次 = 一步撤销。
    PasteSelection {
        /// 点击格（鼠标所在世界格）X —— **不是**左上角。
        x: u16,
        /// 点击格（鼠标所在世界格）Y —— **不是**左上角。
        y: u16,
    },
    /// **删除选区内容**（三族按层清 + 库号字节写 0，**整次 = 一步撤销**）。
    /// 来源：`RegionCopy.cs:45-69`（`CutSource` 口径，见 `selection.rs` 模块文档 §二）。
    DeleteSelection,
    /// **用当前笔刷填满选区**（**整次 = 一步撤销**）。来源：`MapViewport.cs:540`。
    ///
    /// 地表取 `step=1`（严格填满选区），与 C# 的 `step=2` 有意不同 ——
    /// 理由与代价见 `selection.rs` 模块文档 §五。
    FillSelection,
    /// 撤销**一步**。
    Undo,
    /// 重做**一步**。
    Redo,
    /// 保存到文档自己的路径（没有修改时**一个字节都不写**）。
    Save,
}

/// 一条命令执行后的状态回执（网页层据此更新按钮的可用态与状态栏）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// 这条命令真正改了几格（`0` = 什么都没改，原因见 `note`）。
    pub changed: usize,
    /// 一句能上屏的中文（拒绝原因 / 成功回执 / 保存结果）；`None` = 无需额外说明。
    pub note: Option<String>,
    /// 执行完之后是否有未保存的修改。
    pub dirty: bool,
    /// 执行完之后还能不能撤销。
    pub can_undo: bool,
    /// 执行完之后还能不能重做。
    pub can_redo: bool,
}

/// 执行一条命令，返回状态回执。
///
/// 语义与 GUI 的对应方法**逐条一致**（见模块文档第二节的表）；本函数只做三件事：
/// ① 把命令翻成一次会话调用；② 把结果折成 [`Outcome`]；③ 在"改了 0 格"时补一句原因。
pub fn apply(session: &mut EditSession, cmd: Command) -> Result<Outcome, CoreError> {
    match cmd {
        Command::SetLayer { layer } => {
            let l = EditLayer::from_key(&layer).ok_or(CoreError::BadLayer { key: layer })?;
            session.set_layer(l);
            // 阻挡层没有库号字节可写 ⇒ 单独一句，不套「写 {img} + {lib}」（`edit.rs:1783-1804`）
            let note = if l.is_collision() {
                format!("当前图层：{}（只改 {} 的 0x8000 位）", l.name(), l.img_field())
            } else {
                format!(
                    "当前图层：{} · 图号 {} · 库号 {}",
                    l.name(),
                    l.img_field(),
                    l.lib_field().unwrap_or("—")
                )
            };
            Ok(session.outcome(0, Some(note)))
        }
        Command::SetBrushSize { size } => {
            let got = session.set_brush_size(usize::from(size));
            Ok(session.outcome(0, Some(format!("笔刷尺寸：{got}×{got}"))))
        }
        Command::SetTool { tool } => Ok(set_tool_by_key(session, &tool)),
        Command::Paint { x, y } => {
            let n = session.paint_stroke_once(x, y);
            let note = if n == 0 {
                Some(session.paint_note(x, y))
            } else {
                Some(format!("落笔 {n} 格（{}）", session.layer().name()))
            };
            Ok(session.outcome(n, note))
        }
        Command::Stroke { from, to } => {
            let n = session.paint_stroke(Some(from), to);
            let note = if n == 0 {
                Some(session.paint_note(to.0, to.1))
            } else {
                Some(format!(
                    "拖动 {n} 格（{} → {}，{}×{}）",
                    from.0,
                    to.0,
                    session.brush_size(),
                    session.brush_size()
                ))
            };
            Ok(session.outcome(n, note))
        }
        Command::Erase { x, y } => {
            // 与 GUI 的 `--edit-erase` 同一条路径：先切橡皮，再按当前尺寸落一笔
            session.set_tool(Tool::Eraser);
            let n = session.paint_stroke_once(x, y);
            let note = if n == 0 {
                Some(session.paint_note(x, y))
            } else {
                Some(format!("擦除 {n} 格（{}）", session.layer().name()))
            };
            Ok(session.outcome(n, note))
        }
        Command::Fill { x, y } => {
            let out = session.fill(x, y);
            let note = match out {
                FillOutcome::Filled(n) => {
                    Some(format!("填充 {n} 格（{}）", session.layer().name()))
                }
                FillOutcome::AlreadyFilled => {
                    Some("目标格已经是要填的值，未改动".to_string())
                }
                // 越界那种要带具体坐标，其余用 FillOutcome 自带的原因
                _ => Some(
                    session
                        .note()
                        .map(str::to_owned)
                        .or_else(|| out.note(session.layer(), session.brush().map(|b| b.family)))
                        .unwrap_or_else(|| "填充被拒绝".to_string()),
                ),
            };
            Ok(session.outcome(out.cells(), note))
        }
        Command::PlaceStamp { x, y, stamp } => {
            let out = session.place_stamp_by_id(x, y, &stamp);
            let (n, note) = match &out {
                PlaceOutcome::Placed { cells, skipped } => {
                    let mut s = format!("放置印章「{stamp}」{cells} 格");
                    if *skipped > 0 {
                        s.push_str(&format!("（越界跳过 {skipped} 格）"));
                    }
                    // 成功也可能带告警（库号字节被忽略 / 三族全 0 没改）—— 必须一起上屏
                    if let Some(w) = session.note() {
                        s.push('；');
                        s.push_str(w);
                    }
                    (*cells, Some(s))
                }
                // 拒绝：原因原样给出（未注册的 id / 全部越界 / Type5 / 未开图 / 空印章）
                PlaceOutcome::Refused { reason } => (0, Some(reason.clone())),
            };
            Ok(session.outcome(n, note))
        }
        Command::Pick { x, y } => match session.pick_at(x, y) {
            Some(b) => {
                let lib = match b.lib {
                    Some(l) => l.index.to_string(),
                    None => "—".to_string(),
                };
                Ok(session.outcome(
                    0,
                    Some(format!(
                        "吸管：{} 图号 {} 库 {}（来自 ({x},{y})）",
                        b.family_name(),
                        b.sprite(),
                        lib
                    )),
                ))
            }
            None => Ok(session.outcome(
                0,
                Some(format!("点击格 ({x},{y}) 在地图外，笔刷未变")),
            )),
        },
        Command::SetSelection { x0, y0, x1, y1 } => {
            let asked = Selection::new(x0, y0, x1, y1);
            let note = if session.set_selection(asked) {
                // 裁剪后的真实选区（`set_selection` 返回 true ⇒ 一定拿得到；拿不到也不 panic）
                match session.selection() {
                    Some(s) => format!(
                        "选区：{}×{} 格（({},{})..({},{})，闭区间）",
                        s.width(),
                        s.height(),
                        s.x0,
                        s.y0,
                        s.x1,
                        s.y1
                    ),
                    None => "选区已设置".to_string(),
                }
            } else {
                session
                    .note()
                    .map(str::to_owned)
                    .unwrap_or_else(|| "选区整块在地图外，已取消选区".to_string())
            };
            // 选区是 UI 状态 ⇒ 不改地图、不置脏、不产生撤销步
            Ok(session.outcome(0, Some(note)))
        }
        Command::ClearSelection => {
            let had = session.selection().is_some();
            session.clear_selection();
            let note = if had {
                "已取消选区（剪贴板保留，仍可粘贴）".to_string()
            } else {
                "当前没有选区，未改动".to_string()
            };
            Ok(session.outcome(0, Some(note)))
        }
        Command::CopySelection => {
            let note = match session.copy_selection() {
                Some((w, h)) => format!(
                    "已复制 {w}×{h} 格（{} 格进剪贴板，含空格）",
                    usize::from(w) * usize::from(h)
                ),
                // 失败原因（没有选区 / 选区为空）由 `copy_selection` 写进 note
                None => session
                    .note()
                    .map(str::to_owned)
                    .unwrap_or_else(|| "复制被拒绝".to_string()),
            };
            Ok(session.outcome(0, Some(note)))
        }
        Command::CutSelection => match session.cut_selection() {
            Some((w, h)) => {
                // 回执由 `cut_selection` 组装成**一句**（复制数 + 清空数，`session.rs` 的"只出一句"
                // 约定）⇒ 这里**只透传**，绝不二次拼接（否则会同时出现"N 格被清空"与"未改动"）。
                let n = session.last_clear().unwrap_or(0);
                let note = session.note().map(str::to_owned).unwrap_or_else(|| {
                    let 已复制 = usize::from(w) * usize::from(h);
                    format!("剪切 {w}×{h} 格（已复制 {已复制} 格进剪贴板；清空 {n} 格）")
                });
                Ok(session.outcome(n, Some(note)))
            }
            None => Ok(session.outcome(
                0,
                Some(
                    session
                        .note()
                        .map(str::to_owned)
                        .unwrap_or_else(|| "剪切被拒绝".to_string()),
                ),
            )),
        },
        Command::PasteSelection { x, y } => {
            // ⚠ 回执里的坐标必须是**实际锚点**、不是点击点（2026-11-06）：
            // `paste_clipboard` 会先按上游 `RegionCopy.cs:70` 的 `Anchor` 把左上角按**源的奇偶**
            // 往左上对齐（地表可见时点击点常常不是落点）⇒ 报点击点就是假陈述。
            // 这里**只读**算一次（`clipboard()` 是不可变借用、`anchor` 是纯函数，不改任何状态）；
            // `None`（负锚点）时坐标不会被用到（回执走拒绝分支）。
            let (锚x, 锚y) = session
                .clipboard()
                .and_then(|c| c.anchor(x, y))
                .unwrap_or((x, y));
            let out = session.paste_clipboard(x, y);
            let (n, note) = match &out {
                PlaceOutcome::Placed { cells, skipped } => {
                    let mut s = format!("粘贴 {cells} 格到 ({锚x},{锚y})");
                    if (锚x, 锚y) != (x, y) {
                        s.push_str(&format!("（点击点 ({x},{y}) 已按源奇偶对齐）"));
                    }
                    if *skipped > 0 {
                        s.push_str(&format!("（越界跳过 {skipped} 格）"));
                    }
                    // 成功也可能带告警（库号字节被忽略 / 三族全 0 没改）—— 必须一起上屏，
                    // 但要用「另：」隔开（告警不是"粘了几格"的一部分）
                    if let Some(w) = session.note() {
                        s.push_str("；另：");
                        s.push_str(w);
                    }
                    (*cells, Some(s))
                }
                // 拒绝：原因原样给出（剪贴板为空 / 锚点为负 / 全部越界 / Type5）
                PlaceOutcome::Refused { reason } => (0, Some(reason.clone())),
            };
            Ok(session.outcome(n, note))
        }
        Command::DeleteSelection => {
            let n = session.delete_selection();
            // 回执由 `delete_selection` 组装成**一句**（成功回执 / 没有可清的内容，二者互斥）
            let note = session
                .note()
                .map(str::to_owned)
                .unwrap_or_else(|| format!("删除选区内容 {n} 格（三族按层清）"));
            Ok(session.outcome(n, Some(note)))
        }
        Command::FillSelection => {
            let n = session.fill_selection();
            let note = if n == 0 {
                Some(
                    session
                        .note()
                        .map(str::to_owned)
                        .unwrap_or_else(|| "选区内已经是要填的值，未改动".to_string()),
                )
            } else {
                let mut s = format!("填充选区 {n} 格（{}）", session.layer().name());
                // 库号字节写不进去的告警与"填了几格"是两件事 ⇒ 用「另：」隔开，不混成一句
                if let Some(w) = session.note() {
                    s.push_str("；另：");
                    s.push_str(w);
                }
                Some(s)
            };
            Ok(session.outcome(n, note))
        }
        Command::Undo => {
            let n = session.undo();
            let note = if n == 0 {
                "没有可撤销的步骤".to_string()
            } else {
                format!("撤销 {n} 格")
            };
            Ok(session.outcome(n, Some(note)))
        }
        Command::Redo => {
            let n = session.redo();
            let note = if n == 0 {
                "没有可重做的步骤".to_string()
            } else {
                format!("重做 {n} 格")
            };
            Ok(session.outcome(n, Some(note)))
        }
        Command::Save => {
            // 保存失败走 Err（带阶段），成功/跳过走 Outcome
            let out: SaveOutcome = save::save_session(session)?;
            Ok(session.outcome(0, Some(out.note())))
        }
    }
}

/// 按**工具键**切工具（`Command::SetTool` 的唯一实现，GUI/网页也可以直接调它）。
///
/// 键的词汇表只有 [`Tool::from_key`] 认的四个：`brush` / `bucket` / `eraser` / `pickup`
/// （严格小写、精确匹配；`unwired` 与任何别名都不收 —— 2026-11-05 契约裁定）。
///
/// 非法键 ⇒ **`Outcome { changed: 0, note: Some(中文原因) }`**，且**一个状态都不改**
/// （工具 / 撤销栈 / 脏标记 / 笔刷 / 层全保持原样）。为什么不用 `Err`：
/// 与 `SetLayer` 的 `BadLayer` 不同，认不出工具名**不会写错任何字段**，
/// 它只是"这次调用没生效"，调用方更需要 `dirty` / `can_undo` 这些状态（见模块文档第四节）。
pub fn set_tool_by_key(session: &mut EditSession, key: &str) -> Outcome {
    match Tool::from_key(key) {
        Some(t) => {
            // `set_tool` 返回"该工具是否已接线"：四个键都接线，所以恒为 true；
            // 仍然如实按返回值写文案（将来若把某个工具摘掉，这里不用改）
            let wired = session.set_tool(t);
            let note = if wired {
                format!("当前工具：{}（{}）", t.name(), t.key())
            } else {
                format!("当前工具：{}（{}）—— 该工具未接线，落到画布上不会改图", t.name(), t.key())
            };
            session.outcome(0, Some(note))
        }
        None => session.outcome(
            0,
            Some(format!(
                "工具名不合法：{key:?}（只接受 brush / bucket / eraser / pickup 四个小写键，\
                 不接受别名；工具未变）"
            )),
        ),
    }
}

/// 从素材面板选帧设笔刷的便捷入口（网页版的"选素材"面板用）。
///
/// 为什么单独给一个函数而不做成 [`Command`]：任务书 §五的命令集是**封闭的**，
/// 而"从素材面板选帧"要带一个 [`LibRef`]（族 + 索引）—— 塞进命令就得给 `Command`
/// 加一个嵌套结构体，会破坏"字段只有 `u16`/`String`"的可序列化约定。
/// 网页层可以把它做成一条**自己的**消息，进来后调这个函数。
pub fn set_brush_from_palette(
    session: &mut EditSession,
    lib: LibRef,
    frame_index: u32,
) -> Outcome {
    match session.set_brush_from_palette(lib, frame_index) {
        Some(b) => session.outcome(
            0,
            Some(format!(
                "笔刷已设为 {} 图号 {}（库 {}）",
                b.family_name(),
                b.sprite(),
                b.lib_byte
            )),
        ),
        None => session.outcome(
            0,
            Some(format!(
                "帧索引 {frame_index} 越界（换算后的图号会超过 0x7FFF 或撞上阻挡位），笔刷未变"
            )),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::EditLayer;
    use crate::testutil::造文档;
    use mir2_map::CellFormat;

    fn 会话(w: u16, h: u16, fmt: CellFormat) -> EditSession {
        EditSession::new(造文档(w, h, fmt))
    }

    /// 层名合法 ⇒ 切层成功；非法 ⇒ `Err(BadLayer)` 且**层没变**（不猜）。任务书 §五。
    #[test]
    fn 层名非法就报错不改状态() {
        let mut ed = 会话(4, 4, CellFormat::New14);
        let out = apply(
            &mut ed,
            Command::SetLayer {
                layer: "middle".to_string(),
            },
        )
        .unwrap();
        assert_eq!(ed.layer(), EditLayer::Middle);
        assert_eq!(out.changed, 0);
        assert!(out.note.unwrap().contains("中层图层"));

        let err = apply(
            &mut ed,
            Command::SetLayer {
                layer: "Middle".to_string(),
            },
        )
        .unwrap_err();
        assert!(matches!(err, CoreError::BadLayer { .. }), "{err}");
        assert_eq!(ed.layer(), EditLayer::Middle, "报错时层不许变");
    }

    /// 尺寸命令夹到 3..=31 的奇数，回执给真正生效的值。
    #[test]
    fn 尺寸命令夹紧() {
        let mut ed = 会话(4, 4, CellFormat::New14);
        for (ask, want) in [(4u16, 5usize), (0, 3), (99, 31)] {
            let out = apply(&mut ed, Command::SetBrushSize { size: ask }).unwrap();
            assert_eq!(ed.brush_size(), want);
            assert!(out.note.unwrap().contains(&format!("{want}×{want}")));
        }
    }

    /// 落笔：改了 N 格 ⇒ `changed == N` 且 `can_undo`；没笔刷 ⇒ `changed == 0` 且**有原因**。
    #[test]
    fn 落笔回执() {
        let mut ed = 会话(8, 8, CellFormat::New14);
        // 没笔刷
        let out = apply(&mut ed, Command::Paint { x: 1, y: 1 }).unwrap();
        assert_eq!(out.changed, 0);
        assert!(out.note.unwrap().contains("没有笔刷"));
        assert!(!out.dirty && !out.can_undo);
        // 吸管取笔刷后落 3×3
        apply(&mut ed, Command::Pick { x: 0, y: 0 }).unwrap();
        let out = apply(&mut ed, Command::Paint { x: 4, y: 4 }).unwrap();
        assert_eq!(out.changed, 9, "尺寸 3 ⇒ 9 格");
        assert!(out.dirty && out.can_undo && !out.can_redo);
        // 再落同一格 ⇒ 值没变，0 格且有原因
        let out = apply(&mut ed, Command::Paint { x: 4, y: 4 }).unwrap();
        assert_eq!(out.changed, 0);
        assert!(out.note.unwrap().contains("相同"));
    }

    /// 拖动：整条轨迹一步；撤销一步回到原样。
    #[test]
    fn 拖动回执与撤销() {
        let mut ed = 会话(20, 20, CellFormat::New14);
        apply(&mut ed, Command::Pick { x: 0, y: 0 }).unwrap();
        let orig = ed.map().to_bytes();
        let out = apply(
            &mut ed,
            Command::Stroke {
                from: (5, 5),
                to: (9, 5),
            },
        )
        .unwrap();
        assert_eq!(
            out.changed, 21,
            "5 个中心 × 3×3 ⇒ x 4..10（7 列）× y 4..6（3 行）= 21 格"
        );
        assert_eq!(ed.undo_len(), 1, "整条轨迹 = 一步");
        let out = apply(&mut ed, Command::Undo).unwrap();
        assert_eq!(out.changed, 21);
        assert_eq!(ed.map().to_bytes(), orig);
        assert!(!out.dirty);
        // 再撤一次 ⇒ 没有可撤销的
        let out = apply(&mut ed, Command::Undo).unwrap();
        assert_eq!(out.changed, 0);
        assert_eq!(out.note.unwrap(), "没有可撤销的步骤");
        // 重做
        let out = apply(&mut ed, Command::Redo).unwrap();
        assert_eq!(out.changed, 21);
        assert!(out.can_undo);
    }

    /// 吸管：越界也给明确回执（笔刷不变）。
    #[test]
    fn 吸管回执() {
        let mut ed = 会话(4, 4, CellFormat::New14);
        let out = apply(&mut ed, Command::Pick { x: 1, y: 1 }).unwrap();
        assert!(out.note.unwrap().contains("吸管"));
        assert!(ed.brush().is_some());
        let before = ed.brush();
        let out = apply(&mut ed, Command::Pick { x: 99, y: 99 }).unwrap();
        assert_eq!(out.changed, 0);
        assert!(out.note.unwrap().contains("地图外"));
        assert_eq!(ed.brush(), before, "越界不许清掉笔刷");
    }

    /// 填充：成功给格数；已是目标值给"未改动"；物体层被拒且有原因。
    #[test]
    fn 填充回执() {
        let mut ed = 会话(6, 6, CellFormat::New14);
        ed.set_layer(EditLayer::LowCollision);
        let out = apply(&mut ed, Command::Fill { x: 0, y: 0 }).unwrap();
        assert_eq!(out.changed, 36);
        assert!(out.note.unwrap().contains("填充 36 格"));
        let out = apply(&mut ed, Command::Fill { x: 0, y: 0 }).unwrap();
        assert_eq!(out.changed, 0);
        assert!(out.note.unwrap().contains("已经是要填的值"));
        // 物体层
        let mut ed2 = 会话(6, 6, CellFormat::New14);
        apply(
            &mut ed2,
            Command::SetLayer {
                layer: "front".to_string(),
            },
        )
        .unwrap();
        let out = apply(&mut ed2, Command::Fill { x: 1, y: 1 }).unwrap();
        assert_eq!(out.changed, 0);
        assert!(out.note.unwrap().contains("物体层"));
    }

    /// 十九条命令都必须能构造出来（`Command` 是网页层唯一契约，少一条就断链）。
    ///
    /// 2026-11-05：由九条增到十条 —— 新增 [`Command::Erase`]（网页版也要擦除，
    /// 必须走同一条 `apply` 通路，否则就是"两条路径"的分叉起点）；
    /// 同日再由十条增到十二条 —— 新增 [`Command::SetTool`]（擦完必须能切回画笔，
    /// 见 [`Command::Erase`] 的文档）与 [`Command::PlaceStamp`]（印章放置）；
    /// 同日再由十二条增到十九条 —— 新增选区/剪贴板 7 条（审计 P0-4）。
    #[test]
    fn 命令集封闭十九条() {
        let cmds = [
            Command::SetLayer {
                layer: "ground".to_string(),
            },
            Command::SetTool {
                tool: "brush".to_string(),
            },
            Command::SetBrushSize { size: 3 },
            Command::Paint { x: 0, y: 0 },
            Command::Erase { x: 0, y: 0 },
            Command::Stroke {
                from: (0, 0),
                to: (1, 1),
            },
            Command::Fill { x: 0, y: 0 },
            Command::PlaceStamp {
                x: 0,
                y: 0,
                stamp: "mir_00000".to_string(),
            },
            Command::Pick { x: 0, y: 0 },
            Command::SetSelection {
                x0: 0,
                y0: 0,
                x1: 1,
                y1: 1,
            },
            Command::ClearSelection,
            Command::CopySelection,
            Command::CutSelection,
            Command::PasteSelection { x: 0, y: 0 },
            Command::DeleteSelection,
            Command::FillSelection,
            Command::Undo,
            Command::Redo,
            Command::Save,
        ];
        assert_eq!(cmds.len(), 19);
        // 每条都能 clone / 比较（网页层要能把它当值传来传去）
        for c in &cmds {
            assert_eq!(c.clone(), *c);
        }
    }

    /// [`Command::SetTool`]：四个已接线键都能切；非法键（含 `unwired` / 别名 / 大小写）
    /// **不改状态**且给中文原因。
    ///
    /// 契约：2026-11-05 裁定（词汇表只有 `brush`/`bucket`/`eraser`/`pickup`）。
    #[test]
    fn 切工具命令() {
        let mut ed = 会话(4, 4, CellFormat::New14);
        for (key, want) in [
            ("bucket", Tool::Bucket),
            ("eraser", Tool::Eraser),
            ("pickup", Tool::Pickup),
            ("brush", Tool::Brush),
        ] {
            let out = apply(
                &mut ed,
                Command::SetTool {
                    tool: key.to_string(),
                },
            )
            .expect("切工具不该 Err");
            assert_eq!(ed.tool(), want, "{key} 应切到 {want:?}");
            assert_eq!(out.changed, 0, "切工具不改地图");
            assert!(!out.dirty, "切工具不许置脏");
            assert!(!out.can_undo, "切工具不许产生撤销步");
            assert!(out.note.unwrap().contains(want.name()));
        }
        // 非法键：状态一个都不改
        let before = (ed.tool(), ed.undo_len(), ed.dirty());
        for bad in ["unwired", "fill", "pick", "Brush", "brush ", "", "pencil"] {
            let out = apply(
                &mut ed,
                Command::SetTool {
                    tool: bad.to_string(),
                },
            )
            .expect("非法工具名不是程序故障（走 Ok + note）");
            assert_eq!(out.changed, 0);
            let note = out.note.expect("非法键必须给原因");
            assert!(note.contains("工具名不合法"), "{bad:?} ⇒ {note}");
            assert!(note.contains("brush / bucket / eraser / pickup"), "{note}");
            assert_eq!(
                (ed.tool(), ed.undo_len(), ed.dirty()),
                before,
                "{bad:?} 不许改任何状态"
            );
        }
    }

    /// [`Command::PlaceStamp`]：已注册的 id 真的放置（一步撤销）；未注册的 id 明确拒绝。
    ///
    /// 注册表的存在理由见 [`EditSession::register_stamp`]：本 crate 不读盘，
    /// 印章 id → 格表由上层填。
    #[test]
    fn 放置印章命令() {
        let mut ed = 会话(8, 8, CellFormat::New14);
        // ⚠ 占用闸门（`PlacedStamps.cs:62-63` 的 `if(c.HasForeground)return false;`）要求落点格
        // **没有物件**（`fr_img & 0x7FFF == 0`），而 `造图` 夹具每格都带物体图号
        // （`testutil.rs:27` 的 `img.wrapping_add(200)`）⇒ 先把两处落点的物体层清空，
        // 否则印章会被整枚拒绝，本用例测到的就不是"放置 + 一步撤销"而是"闸门怎么拒"。
        // 清空发生在 `原` 快照之前 ⇒ 后面"撤销回 `原`"的断言仍然成立。
        {
            let map = &mut ed.document_mut().view_mut();
            for (x, y) in [(3u16, 3u16), (4u16, 4u16)] {
                let i = crate::document::index_of(map, x, y).expect("图内格");
                map.cells[i].fr_img = 0;
            }
        }
        let 原 = ed.map().to_bytes();
        let out = apply(
            &mut ed,
            Command::PlaceStamp {
                x: 2,
                y: 2,
                stamp: "不存在".to_string(),
            },
        )
        .expect("未注册不是程序故障（走 Ok + note）");
        assert_eq!(out.changed, 0);
        let note = out.note.expect("拒绝必须给原因");
        assert!(note.contains("未注册的印章 id"), "{note}");
        assert!(note.contains("register_stamp"), "要点明该怎么办：{note}");
        assert_eq!(ed.map().to_bytes(), 原, "拒绝时一格都不许动");
        assert!(!out.dirty && !out.can_undo);
        // ② 注册后放置 ⇒ 2 格真的改了，一步撤销
        assert_eq!(
            ed.register_stamp(
                "小印章",
                vec![
                    crate::stamp::StampCellInput {
                        dx: 0,
                        dy: 0,
                        bk: 11,
                        mid: 12,
                        fr: 13,
                        ..Default::default()
                    },
                    crate::stamp::StampCellInput {
                        dx: 1,
                        dy: 1,
                        bk: 14,
                        mid: 15,
                        fr: 16,
                        ..Default::default()
                    },
                ],
            )
            .expect("注册应成功"),
            2
        );
        assert_eq!(ed.stamp_ids(), vec!["小印章"]);
        let out = apply(
            &mut ed,
            Command::PlaceStamp {
                x: 3,
                y: 3,
                stamp: "小印章".to_string(),
            },
        )
        .expect("放置不该 Err");
        assert_eq!(out.changed, 2, "2 格印章");
        assert!(out.dirty && out.can_undo);
        assert!(out.note.unwrap().contains("放置印章「小印章」2 格"));
        assert_eq!(ed.map().cell(3, 3).unwrap().bk_img, 11);
        assert_eq!(ed.map().cell(4, 4).unwrap().fr_img, 16);
        assert_eq!(ed.undo_len(), 1, "整次放置 = 一步");
        // ③ 撤销一步回到放之前
        apply(&mut ed, Command::Undo).unwrap();
        assert_eq!(ed.map().to_bytes(), 原);
    }

    /// [`Command::Erase`]：真的擦（图号层清图号、保留阻挡位），且擦完工具留在橡皮。
    ///
    /// 判据与 GUI 的 `--edit-erase` 完全一致（`edit.rs:1855-1883`）：
    /// `select(Eraser)` → `paint_stroke_once` → 一次一步撤销。
    #[test]
    fn 擦除命令() {
        let mut ed = 会话(8, 8, CellFormat::New14);
        {
            let map = &mut ed.document_mut().view_mut();
            for c in map.cells.iter_mut() {
                c.bk_img = 0x8000 | 1234;
            }
        }
        let orig = ed.map().to_bytes();
        // 尺寸 3 ⇒ 9 格
        let out = apply(&mut ed, Command::Erase { x: 4, y: 4 }).unwrap();
        assert_eq!(out.changed, 9, "尺寸 3 的橡皮应擦 9 格");
        assert_eq!(ed.tool(), Tool::Eraser, "擦完工具留在橡皮（与 GUI 一致）");
        assert_eq!(ed.undo_len(), 1, "一次擦除 = 一步撤销");
        // 图号清掉、阻挡位留着
        let c = *ed.map().cell(4, 4).unwrap();
        assert_eq!(c.bk_img, 0x8000, "只清图号，保留阻挡位");
        assert!(!c.is_walkable());
        // 撤销回原样
        apply(&mut ed, Command::Undo).unwrap();
        assert_eq!(ed.map().to_bytes(), orig);
        // 越界 ⇒ 0 格且给原因
        let out = apply(&mut ed, Command::Erase { x: 99, y: 99 }).unwrap();
        assert_eq!(out.changed, 0);
        assert!(out.note.unwrap().contains("地图外"));
    }

    /// 素材面板选帧的便捷入口：越界帧拒绝且不动笔刷。
    #[test]
    fn 选帧入口() {
        let mut ed = 会话(4, 4, CellFormat::New14);
        let out = set_brush_from_palette(&mut ed, LibRef::ground(3), 9);
        assert!(out.note.unwrap().contains("图号 10"));
        assert_eq!(ed.brush().unwrap().sprite(), 10);
        let before = ed.brush();
        let out = set_brush_from_palette(&mut ed, LibRef::ground(3), u32::MAX);
        assert_eq!(out.changed, 0);
        assert!(out.note.unwrap().contains("越界"));
        assert_eq!(ed.brush(), before, "越界不许动笔刷");
    }
}
