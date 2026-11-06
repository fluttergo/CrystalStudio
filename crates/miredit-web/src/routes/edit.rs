//! `POST /api/edit` —— **所有编辑都走 `miredit_core::apply`**（本 crate 不自己改地图）。
//!
//! # 一、请求 / 响应
//!
//! ```json
//! // 请求：固定一层 cmd
//! {"cmd": {"SetLayer": {"layer": "ground"}}}
//! {"cmd": {"Paint": {"x": 100, "y": 200}}}
//! {"cmd": "Undo"}
//!
//! // 响应
//! {"changed": 9, "dirty": true, "note": "落笔 9 格（地表图层）", "can_undo": true, "can_redo": false}
//! ```
//!
//! `cmd` 的形态表见 [`crate::json`] 的模块文档（那是与内核唯一的契约面）。
//!
//! 十二条内核命令之外还有**一条网页侧命令**（不是 `Command` 的变体）：
//! `{"SetBrushFromPalette":{…}}`（资源面板选帧当笔刷）。它与内核命令在**同一把写锁**里执行，
//! 所以天然串行、不会交错。
//!
//! ⚠ `{"SetTool":{"tool":…}}` 现在**是内核变体**（`Command::SetTool`），走 `apply` 那条通路；
//! 键的校验（只收 `brush`/`bucket`/`eraser`/`pickup`）在 `cmd::set_tool_by_key`，本 crate 不重复。
//!
//! # 二、并发：写命令串行
//!
//! 整个「取写锁 → `apply` → 放锁」是一个临界区，同一时刻只有一个写命令在跑
//! （任务书 §四）。出图（读锁）可以并发，但**看不到写了一半的状态**：
//! `apply` 内部要么改完要么没改，`RwLock` 保证读的人看不到中间态。
//!
//! # 三、`changed == 0` 不是错误
//!
//! 内核的「拒绝」（族不匹配 / 没有笔刷 / 值没变 / 没有可撤销的步骤）是**语义结果**，
//! 会返回 `200 + changed=0 + note=原因`（`miredit-core/src/error.rs` 模块文档的对照表）。
//! 只有「这次调用没完成」（层名不认识 / 保存失败）才是 4xx/5xx —— 那条线由
//! [`ApiError::from_core`] 画。
//!
//! # 四、事件
//!
//! 改到格子 ⇒ 广播 `rerender`（带受影响格，前端目前整屏重取）；
//! 脏标记可能变了 ⇒ 广播 `dirty`。`Undo`/`Redo`/`Save` 一定广播 `dirty`
//! （它们即使 `changed == 0` 也可能改了脏标记）。

use axum::body::Bytes;
use axum::extract::State;
use axum::Json;
use miredit_core::Command;
use serde_json::Value;

use crate::error::{ApiError, ApiResult};
use crate::json::{lib_ref, outcome_json, parse_edit_cmd, EditCmd};
use crate::routes::parse_body;
use crate::routes::stamps::load_stamp_cells;
use crate::state::{Event, Shared};

/// `POST /api/edit`。
pub async fn edit(State(st): State<Shared>, body: Bytes) -> ApiResult<Json<Value>> {
    let v = parse_body(&body)?;
    let cmd_value = v
        .get("cmd")
        .ok_or_else(|| ApiError::bad_request("请求体缺少 cmd 字段（形如 {\"cmd\":{\"Paint\":{...}}}）"))?;
    let cmd = parse_edit_cmd(cmd_value)?;
    let name = cmd.name();

    // ── 锁外：`PlaceStamp` 要先有格表（**读盘只能在锁外做**，见模块文档第二节）──
    let stamp_ready = prepare_stamp(&st, &cmd)?;

    // ── 临界区：写锁内只做 register_stamp + apply，不做任何 IO / await ──
    let (outcome, note) = {
        let mut guard = st.write_session();
        let session = guard.as_mut().ok_or_else(ApiError::no_map)?;
        // 格表注册是**幂等**的（同 id 覆盖同一份格表）；已注册过的不会再读盘。
        // 注册失败（空印章 / 超单枚上限 / 注册表满）⇒ 422：那是**这枚印章不能用**，
        // 不是"这次调用没完成"以外的输入错，且原因原样用内核的中文串。
        if let Some((id, cells)) = &stamp_ready {
            let n = session
                .register_stamp(id.clone(), cells.clone())
                .map_err(|e| ApiError::parse_failed(format!("印章无法注册（{id}）：{e}")))?;
            println!("[印章] 已注册 {id}（{n} 格）");
        }
        let outcome = match &cmd {
            // 十二条内核命令（含 `SetTool` / `PlaceStamp`）全走同一条通路：
            // 本 crate **不自己实现**任何编辑语义（`SetTool` 的键校验在 `cmd::set_tool_by_key`，
            // 印章的逐格语义在 `crate::stamp`）。
            EditCmd::Core(c) => miredit_core::apply(session, c.clone()).map_err(ApiError::from_core)?,
            // 唯一的网页侧命令：直译 `cmd::set_brush_from_palette`（不是 Command 的变体）
            EditCmd::SetBrushFromPalette { family, lib, frame } => {
                miredit_core::set_brush_from_palette(session, lib_ref(*family, *lib), *frame)
            }
        };
        let note = outcome.note.clone();
        (outcome, note)
    };

    // ── 事件（锁外发，避免广播拖慢写命令）──
    let always_dirty = matches!(
        &cmd,
        EditCmd::Core(Command::Undo) | EditCmd::Core(Command::Redo) | EditCmd::Core(Command::Save)
    );
    let cell = cmd.touched_cell();
    if outcome.changed > 0 {
        let (x, y) = cell.unwrap_or((0, 0));
        // w/h = 0 表示「整屏」（前端目前忽略区域，整屏重取）
        let span = u16::from(cell.is_some());
        st.emit(Event::Rerender { x, y, w: span, h: span });
    }
    if outcome.changed > 0 || always_dirty {
        st.emit(Event::Dirty {
            dirty: outcome.dirty,
        });
    }

    if let Some(n) = &note {
        println!(
            "[编辑] {name} ⇒ changed={} dirty={} note={n}",
            outcome.changed, outcome.dirty
        );
    } else {
        println!("[编辑] {name} ⇒ changed={} dirty={}", outcome.changed, outcome.dirty);
    }
    Ok(Json(outcome_json(&outcome)))
}

/// `PlaceStamp` 的前置：**把印章格表准备好**（web 读盘，core 零 IO）。
///
/// 返回 `Some((id, cells))` ⇒ 写锁内要注册；`None` ⇒ 这条命令不需要注册
/// （不是 `PlaceStamp`，或该 id 本会话已注册过，或还没开图）。
///
/// 为什么"已注册就不再读盘"：`register_stamp` 是整份格表覆盖写，每放一次都重读
/// 几十 KB 的 `.X` 纯属浪费；同一会话里地图不会换（换图会重建会话，注册表随之清空）。
fn prepare_stamp(
    st: &Shared,
    cmd: &EditCmd,
) -> ApiResult<Option<(String, Vec<miredit_core::StampCellInput>)>> {
    let EditCmd::Core(Command::PlaceStamp { stamp, .. }) = cmd else {
        return Ok(None);
    };
    // 一次读锁拿两个判断（已注册？开图了吗？），别为同一个问题锁两趟
    let (already, has_map) = {
        let guard = st.read_session();
        let already = guard
            .as_ref()
            .and_then(|s| s.stamp_cells(stamp))
            .is_some();
        (already, guard.is_some())
    };
    // 没开图 ⇒ 交给下面的写锁统一报 409（这里不读盘，避免无谓 IO）
    if already || !has_map {
        return Ok(None);
    }
    // 目录里查不到 ⇒ 404；`.X` 坏了 ⇒ 422（见 `load_stamp_cells` 的失败表）
    let cells = load_stamp_cells(st, stamp)?;
    Ok(Some((stamp.clone(), cells)))
}

/// 把错误同时推给 WS（前端右下角轻提示），HTTP 侧照常返回错误体。
///
/// 供 `routes::save` 的失败分支复用（保存失败是"用户必须立刻知道"的事件）。
pub fn emit_error(st: &Shared, err: &ApiError) {
    eprintln!("[错误] {} {} ⇒ {}", err.status(), err.code(), err.message());
    st.emit(Event::Error {
        code: err.code().to_string(),
        message: err.message().to_string(),
    });
}

/// 只有测试用的常量导出（避免"魔法数字"散落）。
#[cfg(test)]
pub const 响应字段: [&str; 5] = ["changed", "dirty", "note", "can_undo", "can_redo"];

#[cfg(test)]
mod tests {
    use super::*;
    use miredit_core::Outcome;

    #[test]
    fn 回执字段齐全() {
        let o = Outcome {
            changed: 3,
            note: Some("落笔 3 格".into()),
            dirty: true,
            can_undo: true,
            can_redo: false,
        };
        let v = outcome_json(&o);
        for k in 响应字段 {
            assert!(v.get(k).is_some(), "响应缺字段 {k}");
        }
        assert_eq!(v["changed"], 3);
        assert_eq!(v["can_redo"], false);
        assert_eq!(v["note"], "落笔 3 格");
        // note = None ⇒ JSON null（不是空串）
        let o2 = Outcome { note: None, ..o };
        assert_eq!(outcome_json(&o2)["note"], Value::Null);
    }

    #[test]
    fn 错误事件带机器码() {
        let err = ApiError::io_error("保存失败：磁盘只读");
        assert_eq!(err.code(), "io_error");
        assert_eq!(err.message(), "保存失败：磁盘只读");
        let v = Event::Error {
            code: err.code().to_string(),
            message: err.message().to_string(),
        }
        .to_json();
        assert_eq!(v["type"], "error");
        assert_eq!(v["payload"]["code"], "io_error");
        assert_eq!(v["payload"]["message"], "保存失败：磁盘只读");
    }
}
