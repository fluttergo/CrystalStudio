//! `miredit-core::Command` 的 **JSON 直译**（手写，因为那个 crate 刻意不引 serde）。
//!
//! # 一、为什么是手写而不是 `#[derive(Deserialize)]`
//!
//! `miredit-core` 的依赖白名单只有 `std` + `mir2-map`（`miredit-core/Cargo.toml` 有注释），
//! `Command` 上**没有**任何 serde derive。所以「JSON ↔ Command」这一层必须在 web 侧写，
//! 而且**必须照 `cmd.rs` 的变体与字段名逐字对齐**——这是前端与内核之间唯一需要对齐的契约。
//!
//! # 二、实际字段（以 `miredit-core/src/cmd.rs` 的 `enum Command` 为准，共 **十九** 个变体）
//!
//! | JSON | `Command` | 说明 |
//! |---|---|---|
//! | `{"SetLayer":{"layer":"ground"}}` | `SetLayer { layer: String }` | 只认 `ground`/`middle`/`front`/`low_collision`/`high_collision` |
//! | `{"SetBrushSize":{"size":7}}` | `SetBrushSize { size: u16 }` | 会被夹进 3..=31 的奇数 |
//! | `{"SetTool":{"tool":"eraser"}}` | `SetTool { tool: String }` | 只认 `brush`/`bucket`/`eraser`/`pickup`（见 §四） |
//! | `{"Paint":{"x":100,"y":200}}` | `Paint { x: u16, y: u16 }` | 按当前笔刷尺寸落一笔 |
//! | `{"Stroke":{"from":[10,10],"to":[20,20]}}` | `Stroke { from: (u16,u16), to: (u16,u16) }` | 元组 ⇒ JSON 数组 |
//! | `{"Erase":{"x":10,"y":10}}` | `Erase { x: u16, y: u16 }` | 按当前笔刷尺寸擦一笔；**执行后工具留在橡皮**（见 §四） |
//! | `{"Fill":{"x":358,"y":0}}` | `Fill { x: u16, y: u16 }` | BFS 连通填充 |
//! | `{"Pick":{"x":10,"y":10}}` | `Pick { x: u16, y: u16 }` | 吸管 |
//! | `{"SetSelection":{"x0":1,"y0":2,"x1":5,"y1":6}}` | `SetSelection { x0, y0, x1, y1: u16 }` | 矩形选区（**闭区间**）；纯 UI 状态，不改图不置脏 |
//! | `"ClearSelection"` | `ClearSelection` | 取消选区（**不清剪贴板**） |
//! | `"CopySelection"` | `CopySelection` | 复制选区进剪贴板（不改图） |
//! | `"CutSelection"` | `CutSelection` | 剪切 = 复制 + 清空（一步撤销） |
//! | `{"PasteSelection":{"x":1,"y":2}}` | `PasteSelection { x, y: u16 }` | 以 `(x,y)` 为左上角粘贴（越界逐格跳过） |
//! | `"DeleteSelection"` | `DeleteSelection` | 清空选区内容（一步撤销） |
//! | `"FillSelection"` | `FillSelection` | 用当前笔刷填满选区（一步撤销） |
//! | `{"PlaceStamp":{"x":1,"y":2,"stamp":"mir/mir_00000"}}` | `PlaceStamp { x, y, stamp: String }` | 放印章；格表由 web 先读盘注册（见 §六） |
//! | `"Undo"` / `"Redo"` / `"Save"` | `Undo` / `Redo` / `Save` | 无参变体 ⇒ JSON 字符串（也接受 `{"Undo":null}`） |
//! | **（本层不开放）** | `PlaceStamp { x, y, stamp: String }` | 需要先注册印章，本 crate 没有注册入口 ⇒ 见 §六 |
//!
//! **与任务书 §三示例的差异**：任务书写的 `{"Stroke":{"from":[10,10],"to":[20,20]}}` 与
//! `cmd.rs` 的 `from: (u16, u16)` 完全一致（元组就是两元素数组），**没有差异**。
//! 唯一要记的是：`Paint`/`Erase`/`Fill`/`Pick` 只吃 `x`/`y`，**没有** `value` 字段
//! （笔刷内容由 `Pick` 或 [`EditCmd::SetBrushFromPalette`] 决定，不是每笔带一个值）。
//!
//! # 三、多出来的一条「网页侧命令」
//!
//! [`EditCmd::SetBrushFromPalette`] **不是** `Command` 的变体，而是 `cmd::set_brush_from_palette`
//! 的直译（`miredit-core/src/cmd.rs:239-261`）。它需要带一个 `LibRef`（族 + 索引）——
//! 塞进 `Command` 会破坏「字段只有 u16/String」的可序列化约定，所以内核把它单独做成函数，
//! 本 crate 就单独做成一条网页侧命令：
//!
//! ```json
//! {"SetBrushFromPalette":{"family":"ground","lib":0,"frame":3}}
//! ```
//!
//! # 四、`SetTool` 走内核变体（2026-11-05 契约变更）
//!
//! `miredit-core` 已新增 `Command::SetTool { tool: String }`（实现 = `cmd::set_tool_by_key`
//! → `EditSession::set_tool`）。**网页侧不再自建同语义命令**，只做键的直译：
//!
//! ```json
//! {"SetTool":{"tool":"eraser"}}
//! ```
//!
//! ## 键的词汇表（**唯一真源 = `Tool::from_key`，四个小写键**）
//!
//! `brush` / `bucket` / `eraser` / `pickup`。`unwired` 与任何别名**一律不收** ——
//! 本层**不做**别名翻译（`fill`/`pick` 这些前端词汇由 `web/js/config.js` 的
//! `ME.TOOLS[*].toolKey` 翻好再发，线上只有一套词汇表）。
//!
//! 非法键 ⇒ 内核返回 `Ok(Outcome { changed: 0, note: 中文原因 })` 且**不改会话状态**
//! （不是 400：那不是"这次调用没完成"，而是"没认出来"）。
//!
//! ## 为什么必须有 `SetTool`
//!
//! [`Command::Erase`] 执行后**工具留在橡皮**（`cmd.rs` 的注释写明与 GUI 的
//! `--edit-erase` 一致：`select(Eraser)` 之后不再切回）。而 `Command::Paint` 走的是
//! `paint_stroke_once` → `touch_footprint`，**按当前工具分派**。⇒ 擦完直接发 `Paint`
//! 会**继续擦**。所以"切回画笔"必须有一条显式命令，这条就是它。
//!
//! # 五、`touched_cell()` 为什么要覆盖 `Erase`
//!
//! 它只用于 `/api/events` 的 `rerender` 区域提示。`Erase` 与 `Paint` 一样是**真的改格**，
//! 落进 `_ => None` 会让前端收到「整屏」提示（`w=h=0`）——不是错，但白丢精度。
//! 2026-11-05 补齐（上次加 `Erase` 变体时漏了这一处）。
//!
//! # 六、`Command::PlaceStamp` 要**先注册印章**（分工：web 读盘，core 零 IO）
//!
//! ```text
//! web:  StampCatalog::resolve(id) → 读 .X → Stamp::parse
//!       → miredit_core::cells_from_stamp（唯一换算口）
//!       → session.register_stamp(id, cells) → apply(Command::PlaceStamp{ x, y, stamp: id })
//! core: cells_from_stamp / register_stamp / place_stamp_by_id / Command::PlaceStamp（一格 IO 都没有）
//! ```
//!
//! 本层只做 **JSON ↔ 命令**；注册发生在 `routes::edit` 的 `prepare_stamp`
//! （**锁外**读盘，锁内只 `register_stamp` + `apply`）。
//!
//! 失败口径（见 `routes::stamps::load_stamp_cells`）：
//!
//! | 情况 | 状态码 |
//! |---|---|
//! | 印章目录不可用 | **503** `assets_unavailable` |
//! | id 不在目录里 | **404** `not_found` |
//! | `.X` 读不了 / 解析失败 | **500** `io_error` / **422** `parse_failed` |
//! | 解析成功但某族库号越界（`>255` / 负） | **422** `parse_failed`（`message` = core 原文） |
//! | 未注册就放（本层已挡） | 不会发生；真发生时 core 给 `changed=0 + 中文原因`（仍是 200） |
//!
//! # 七、为什么 `name()` 是**穷举无兜底**、`touched_cell()` 是**逐条显式**
//!
//! 两个函数对"内核新增变体"的反应**故意不同**，各有代价：
//!
//! - [`EditCmd::name`]：**无 `_` 臂** ⇒ 内核一加变体这里**编译红**。代价是每次 core 动
//!   `Command` 都会挡一下；收益是"新命令在网页层没接"这件事**不可能被静默忽略**。
//! - [`EditCmd::touched_cell`]：**每条都写出来**（`None` 也写）⇒ 新变体照样编译红。
//!   早先这里是 `_ => None` 兜底：看起来安全，实际会把**改格命令**静默归为 `None`，
//!   WS 的 `rerender` 区域就悄悄丢了 —— `Erase` 就是这么漏的（2026-11-05 补）。
//!
//! **纪律**：动 `Command` 之前先 `grep` 全 workspace 谁穷举 match 了它。
//! 本 workspace 只有两处：`miredit-core/src/cmd.rs` 的 `apply()`（core 自己）
//! 与本文件的 `name()` / `touched_cell()`。`miredit-gui` **不是**消费方
//! （它直接 `miredit_core::apply` 转发，不 match 变体；`app.rs` 里的 `Command::` 是 egui 的
//! `ViewportCommand`，同名不同物）。

use miredit_core::{Command, Family, LibRef, Outcome};
use serde_json::{json, Map, Value};

use crate::error::{ApiError, ApiResult};

/// 一条网页侧编辑命令：内核 [`Command`] 或 `set_brush_from_palette` 的直译。
#[derive(Debug, Clone)]
pub enum EditCmd {
    /// 内核的十九条命令之一（`SetTool` / `PlaceStamp` / 七条选区命令都在里面）。
    Core(Command),
    /// 「从资源面板选一帧当笔刷」（`cmd::set_brush_from_palette`）。
    SetBrushFromPalette {
        /// 图库族（`ground` / `middle` / `surface`）。
        family: Family,
        /// 族内库号（= 地图格里的库号字节）。
        lib: u16,
        /// **0 基**帧索引。
        frame: u32,
    },
}

impl EditCmd {
    /// 这条命令的短名（日志与事件用）。
    ///
    /// ⚠ **穷举、无 `_` 臂**：内核一加变体这里就编译红 —— 那是**故意的**，
    /// 逼着人把新命令在网页层也接上（本会话已栽过两次：`Erase` 漏 `json.rs`、
    /// `SetTool` 派两路）。见模块文档 §七。
    pub fn name(&self) -> &'static str {
        match self {
            EditCmd::Core(Command::SetLayer { .. }) => "SetLayer",
            EditCmd::Core(Command::SetBrushSize { .. }) => "SetBrushSize",
            EditCmd::Core(Command::SetTool { .. }) => "SetTool",
            EditCmd::Core(Command::Paint { .. }) => "Paint",
            EditCmd::Core(Command::Stroke { .. }) => "Stroke",
            EditCmd::Core(Command::Erase { .. }) => "Erase",
            EditCmd::Core(Command::Fill { .. }) => "Fill",
            EditCmd::Core(Command::Pick { .. }) => "Pick",
            EditCmd::Core(Command::SetSelection { .. }) => "SetSelection",
            EditCmd::Core(Command::ClearSelection) => "ClearSelection",
            EditCmd::Core(Command::CopySelection) => "CopySelection",
            EditCmd::Core(Command::CutSelection) => "CutSelection",
            EditCmd::Core(Command::PasteSelection { .. }) => "PasteSelection",
            EditCmd::Core(Command::DeleteSelection) => "DeleteSelection",
            EditCmd::Core(Command::FillSelection) => "FillSelection",
            EditCmd::Core(Command::PlaceStamp { .. }) => "PlaceStamp",
            EditCmd::Core(Command::Undo) => "Undo",
            EditCmd::Core(Command::Redo) => "Redo",
            EditCmd::Core(Command::Save) => "Save",
            EditCmd::SetBrushFromPalette { .. } => "SetBrushFromPalette",
        }
    }

    /// 这条命令影响的格坐标（`None` = 与具体坐标无关，如切层 / 换工具 / 撤销 / 保存）。
    ///
    /// 只用于 `/api/events` 的 `rerender` 区域提示（前端目前整屏重取，区域仅作参考）。
    ///
    /// ⚠ 这里**故意每条都写出来**（不靠 `_ => None` 兜底）：兜底会把将来新增的
    /// **改格命令**静默归为 `None`，WS 的 `rerender` 区域就悄悄丢了（`Erase` 就这么漏过）。
    pub fn touched_cell(&self) -> Option<(u16, u16)> {
        match self {
            EditCmd::Core(Command::Paint { x, y })
            | EditCmd::Core(Command::Erase { x, y })
            | EditCmd::Core(Command::Fill { x, y })
            | EditCmd::Core(Command::Pick { x, y })
            // 印章放置也**真的改格**（逐格 MergeStamp）
            | EditCmd::Core(Command::PlaceStamp { x, y, .. })
            // 粘贴以 `(x, y)` 为左上角：区域从这一点起，报它最贴近
            | EditCmd::Core(Command::PasteSelection { x, y }) => Some((*x, *y)),
            EditCmd::Core(Command::Stroke { to, .. }) => Some(*to),
            // 选区是**矩形**，一个坐标装不下 ⇒ 报 None（前端整屏重取）
            EditCmd::Core(Command::SetSelection { .. }) => None,
            // 下面五条都作用在**当前选区**上，命令里没有坐标 ⇒ 只能报 None
            // （`Delete`/`Fill`/`Cut` 确实改格，但改哪一片由会话里的选区决定）
            EditCmd::Core(Command::ClearSelection)
            | EditCmd::Core(Command::CopySelection)
            | EditCmd::Core(Command::CutSelection)
            | EditCmd::Core(Command::DeleteSelection)
            | EditCmd::Core(Command::FillSelection) => None,
            EditCmd::Core(Command::SetLayer { .. })
            | EditCmd::Core(Command::SetBrushSize { .. })
            | EditCmd::Core(Command::SetTool { .. })
            | EditCmd::Core(Command::Undo)
            | EditCmd::Core(Command::Redo)
            | EditCmd::Core(Command::Save)
            | EditCmd::SetBrushFromPalette { .. } => None,
        }
    }
}

/// 把 JSON 解析成 [`EditCmd`]（结构不对一律 400，且消息里写清期望形状）。
pub fn parse_edit_cmd(v: &Value) -> ApiResult<EditCmd> {
    match v {
        // 无参命令：字符串形式
        Value::String(s) => match s.as_str() {
            "Undo" => Ok(EditCmd::Core(Command::Undo)),
            "Redo" => Ok(EditCmd::Core(Command::Redo)),
            "Save" => Ok(EditCmd::Core(Command::Save)),
            // 选区里的六条无参命令也收字符串形式（与 Undo/Redo/Save 同一套写法）
            "ClearSelection" => Ok(EditCmd::Core(Command::ClearSelection)),
            "CopySelection" => Ok(EditCmd::Core(Command::CopySelection)),
            "CutSelection" => Ok(EditCmd::Core(Command::CutSelection)),
            "DeleteSelection" => Ok(EditCmd::Core(Command::DeleteSelection)),
            "FillSelection" => Ok(EditCmd::Core(Command::FillSelection)),
            other => Err(ApiError::bad_request(format!(
                "未知命令：{other:?}（无参命令只有 \"Undo\" / \"Redo\" / \"Save\" / \
                 \"ClearSelection\" / \"CopySelection\" / \"CutSelection\" / \
                 \"DeleteSelection\" / \"FillSelection\"）"
            ))),
        },
        // 带参命令：外部标记的单键对象 {"Variant": {...}}
        Value::Object(m) => {
            let (tag, body) = single_entry(m)?;
            parse_tagged(tag, body)
        }
        other => Err(ApiError::bad_request(format!(
            "cmd 必须是字符串或单键对象，收到：{other}"
        ))),
    }
}

/// 取单键对象的「键 + 值」；不是恰好一个键就报 400。
fn single_entry(m: &Map<String, Value>) -> ApiResult<(&str, &Value)> {
    let mut it = m.iter();
    let first = it
        .next()
        .ok_or_else(|| ApiError::bad_request("cmd 对象不能为空"))?;
    if it.next().is_some() {
        return Err(ApiError::bad_request(
            "cmd 对象必须恰好一个键（外部标记枚举，如 {\"Paint\":{\"x\":1,\"y\":2}}）",
        ));
    }
    Ok((first.0.as_str(), first.1))
}

/// 按变体名分派。
fn parse_tagged(tag: &str, body: &Value) -> ApiResult<EditCmd> {
    match tag {
        "SetLayer" => {
            let o = as_object(body, "SetLayer")?;
            let layer = str_field(o, "layer", "SetLayer")?;
            Ok(EditCmd::Core(Command::SetLayer { layer }))
        }
        "SetBrushSize" => {
            let o = as_object(body, "SetBrushSize")?;
            let size = u16_field(o, "size", "SetBrushSize")?;
            Ok(EditCmd::Core(Command::SetBrushSize { size }))
        }
        "Paint" => {
            let o = as_object(body, "Paint")?;
            Ok(EditCmd::Core(Command::Paint {
                x: u16_field(o, "x", "Paint")?,
                y: u16_field(o, "y", "Paint")?,
            }))
        }
        "Fill" => {
            let o = as_object(body, "Fill")?;
            Ok(EditCmd::Core(Command::Fill {
                x: u16_field(o, "x", "Fill")?,
                y: u16_field(o, "y", "Fill")?,
            }))
        }
        "Pick" => {
            let o = as_object(body, "Pick")?;
            Ok(EditCmd::Core(Command::Pick {
                x: u16_field(o, "x", "Pick")?,
                y: u16_field(o, "y", "Pick")?,
            }))
        }
        "Stroke" => {
            let o = as_object(body, "Stroke")?;
            Ok(EditCmd::Core(Command::Stroke {
                from: pair_field(o, "from", "Stroke")?,
                to: pair_field(o, "to", "Stroke")?,
            }))
        }
        "Erase" => {
            let o = as_object(body, "Erase")?;
            Ok(EditCmd::Core(Command::Erase {
                x: u16_field(o, "x", "Erase")?,
                y: u16_field(o, "y", "Erase")?,
            }))
        }
        // 无参变体也允许对象形式（{"Undo":null}），方便前端统一按对象发
        "Undo" => Ok(EditCmd::Core(Command::Undo)),
        "Redo" => Ok(EditCmd::Core(Command::Redo)),
        "Save" => Ok(EditCmd::Core(Command::Save)),
        // ── 选区 / 剪贴板（七条，2026-11-05 接入）──
        "SetSelection" => {
            let o = as_object(body, "SetSelection")?;
            Ok(EditCmd::Core(Command::SetSelection {
                x0: u16_field(o, "x0", "SetSelection")?,
                y0: u16_field(o, "y0", "SetSelection")?,
                x1: u16_field(o, "x1", "SetSelection")?,
                y1: u16_field(o, "y1", "SetSelection")?,
            }))
        }
        "PasteSelection" => {
            let o = as_object(body, "PasteSelection")?;
            Ok(EditCmd::Core(Command::PasteSelection {
                x: u16_field(o, "x", "PasteSelection")?,
                y: u16_field(o, "y", "PasteSelection")?,
            }))
        }
        "ClearSelection" => Ok(EditCmd::Core(Command::ClearSelection)),
        "CopySelection" => Ok(EditCmd::Core(Command::CopySelection)),
        "CutSelection" => Ok(EditCmd::Core(Command::CutSelection)),
        "DeleteSelection" => Ok(EditCmd::Core(Command::DeleteSelection)),
        "FillSelection" => Ok(EditCmd::Core(Command::FillSelection)),
        "SetBrushFromPalette" => {
            let o = as_object(body, "SetBrushFromPalette")?;
            let family = parse_family(&str_field(o, "family", "SetBrushFromPalette")?)?;
            let lib = u16_field(o, "lib", "SetBrushFromPalette")?;
            let frame = u32_field(o, "frame", "SetBrushFromPalette")?;
            Ok(EditCmd::SetBrushFromPalette { family, lib, frame })
        }
        // 换工具：走内核变体（`cmd::set_tool_by_key` 校验四个键，非法键给 changed=0 + 原因）
        "SetTool" => {
            let o = as_object(body, "SetTool")?;
            Ok(EditCmd::Core(Command::SetTool {
                tool: str_field(o, "tool", "SetTool")?,
            }))
        }
        // 放印章：格表注册在 `routes::edit::prepare_stamp`（锁外读盘），本层只翻 JSON
        "PlaceStamp" => {
            let o = as_object(body, "PlaceStamp")?;
            Ok(EditCmd::Core(Command::PlaceStamp {
                x: u16_field(o, "x", "PlaceStamp")?,
                y: u16_field(o, "y", "PlaceStamp")?,
                stamp: str_field(o, "stamp", "PlaceStamp")?,
            }))
        }
        other => Err(ApiError::bad_request(format!(
            "未知命令：{other:?}（可用：SetLayer / SetBrushSize / SetTool / Paint / Stroke / \
             Erase / Fill / Pick / SetSelection / ClearSelection / CopySelection / CutSelection / \
             PasteSelection / DeleteSelection / FillSelection / PlaceStamp / Undo / Redo / Save / \
             SetBrushFromPalette）"
        ))),
    }
}

/// 把 [`Outcome`] 装成 `/api/edit` 的响应体。
pub fn outcome_json(o: &Outcome) -> Value {
    json!({
        "changed": o.changed,
        "dirty": o.dirty,
        "note": o.note,
        "can_undo": o.can_undo,
        "can_redo": o.can_redo,
    })
}

/// 族名（`ground` / `middle` / `surface`）→ [`Family`]；`object`/`front` 是 `surface` 的别名。
///
/// 别名存在的理由：前端资源面板把表层叫 `object`（`web/README.md` §3.7），
/// 而内核把同一族叫 `Surface`；两个名字都收，避免调用方被迫记两套词汇。
pub fn parse_family(s: &str) -> ApiResult<Family> {
    match s.trim().to_ascii_lowercase().as_str() {
        "ground" => Ok(Family::Ground),
        "middle" => Ok(Family::Middle),
        "surface" | "object" | "front" => Ok(Family::Surface),
        other => Err(ApiError::bad_request(format!(
            "未知图库族：{other:?}（只接受 ground / middle / surface，surface 可写 object 或 front）"
        ))),
    }
}

/// 构造内核的 [`LibRef`]（族 + 族内索引）。
pub fn lib_ref(family: Family, lib: u16) -> LibRef {
    match family {
        Family::Ground => LibRef::ground(lib),
        Family::Middle => LibRef::middle(lib),
        Family::Surface => LibRef::surface(lib),
    }
}

/// 内核族 → `mir2-assets` 的族（**两个 crate 各有一份同名枚举，必须显式换算**）。
///
/// 为什么不能共享一个类型：`miredit-core` 的依赖白名单只有 `std` + `mir2-map`，
/// 它按同口径**重新定义**了 `Family` / `LibRef`（见 `miredit-core/src/brush.rs` 模块文档）。
/// 本 crate 同时依赖两边，所以这里做一次显式映射 —— 三个分支写死，**不用 `as` 转换**
/// （枚举判别值相同只是巧合，靠它转迟早出事）。
pub fn assets_family(family: Family) -> mir2_assets::Family {
    match family {
        Family::Ground => mir2_assets::Family::Ground,
        Family::Middle => mir2_assets::Family::Middle,
        Family::Surface => mir2_assets::Family::Surface,
    }
}

/// 内核族 + 族内索引 → `mir2-assets` 的定位键（出图 / 取帧数时用）。
pub fn assets_lib(family: Family, index: u16) -> mir2_assets::LibRef {
    let f = assets_family(family);
    match f {
        mir2_assets::Family::Ground => mir2_assets::LibRef::ground(index),
        mir2_assets::Family::Middle => mir2_assets::LibRef::middle(index),
        mir2_assets::Family::Surface => mir2_assets::LibRef::surface(index),
    }
}

// ───────────────────────── 字段读取（全部给中文错误）─────────────────────────

/// 取对象；不是对象就报 400。
fn as_object<'a>(v: &'a Value, cmd: &str) -> ApiResult<&'a Map<String, Value>> {
    v.as_object()
        .ok_or_else(|| ApiError::bad_request(format!("{cmd} 的参数必须是对象，收到：{v}")))
}

/// 取字符串字段。
fn str_field(o: &Map<String, Value>, key: &str, cmd: &str) -> ApiResult<String> {
    match o.get(key) {
        Some(Value::String(s)) => Ok(s.clone()),
        Some(other) => Err(ApiError::bad_request(format!(
            "{cmd}.{key} 必须是字符串，收到：{other}"
        ))),
        None => Err(ApiError::bad_request(format!("{cmd} 缺少字段 {key}"))),
    }
}

/// 取 `u16` 字段（接受 `0..=65535` 的整数；负数/超界/小数一律 400）。
fn u16_field(o: &Map<String, Value>, key: &str, cmd: &str) -> ApiResult<u16> {
    let n = u64_field(o, key, cmd, u64::from(u16::MAX))?;
    Ok(n as u16)
}

/// 取 `u32` 字段。
fn u32_field(o: &Map<String, Value>, key: &str, cmd: &str) -> ApiResult<u32> {
    let n = u64_field(o, key, cmd, u64::from(u32::MAX))?;
    Ok(n as u32)
}

/// 取整数并校验上界（`x`/`y` 这类坐标字段的统一入口）。
fn u64_field(o: &Map<String, Value>, key: &str, cmd: &str, max: u64) -> ApiResult<u64> {
    match o.get(key) {
        Some(Value::Number(n)) => {
            let v = n.as_u64().ok_or_else(|| {
                ApiError::bad_request(format!("{cmd}.{key} 必须是非负整数，收到：{n}"))
            })?;
            if v > max {
                return Err(ApiError::bad_request(format!(
                    "{cmd}.{key} 超出范围 0..={max}，收到：{v}"
                )));
            }
            Ok(v)
        }
        Some(other) => Err(ApiError::bad_request(format!(
            "{cmd}.{key} 必须是非负整数，收到：{other}"
        ))),
        None => Err(ApiError::bad_request(format!("{cmd} 缺少字段 {key}"))),
    }
}

/// 取 `[x, y]` 两元素数组（对应内核的 `(u16, u16)` 元组）。
fn pair_field(o: &Map<String, Value>, key: &str, cmd: &str) -> ApiResult<(u16, u16)> {
    let arr = match o.get(key) {
        Some(Value::Array(a)) => a,
        Some(other) => {
            return Err(ApiError::bad_request(format!(
                "{cmd}.{key} 必须是 [x, y] 数组，收到：{other}"
            )))
        }
        None => return Err(ApiError::bad_request(format!("{cmd} 缺少字段 {key}"))),
    };
    if arr.len() != 2 {
        return Err(ApiError::bad_request(format!(
            "{cmd}.{key} 必须正好两个元素 [x, y]，收到 {} 个",
            arr.len()
        )));
    }
    let mut out = [0u16; 2];
    for (i, v) in arr.iter().enumerate() {
        let n = v.as_u64().ok_or_else(|| {
            ApiError::bad_request(format!("{cmd}.{key}[{i}] 必须是非负整数，收到：{v}"))
        })?;
        if n > u64::from(u16::MAX) {
            return Err(ApiError::bad_request(format!(
                "{cmd}.{key}[{i}] 超出范围 0..=65535，收到：{n}"
            )));
        }
        out[i] = n as u16;
    }
    Ok((out[0], out[1]))
}

// 测试段拆到 `json_tests.rs`：本体 + 测试会超过 600 行（与 `miredit-core/src/session_tests.rs` 同一做法）。
#[cfg(test)]
#[path = "json_tests.rs"]
mod tests;
