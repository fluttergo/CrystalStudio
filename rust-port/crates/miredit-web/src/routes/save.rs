//! `POST /api/save` —— 原子保存（**走 `miredit_core`，本 crate 不碰字节**）。
//!
//! # 一、保存语义（全部来自内核，这里只做转发与记账）
//!
//! `miredit_core::save::save_session` 的六步：`tmp → write_all → sync_all → 读回逐字节核对
//! → rename → 尽力 fsync 父目录`（`miredit-core/src/save.rs` 模块文档）。所以本端点：
//!
//! - **没有未保存修改时一个字节都不写**（内核直接返回 `note`），响应里如实说明；
//! - 失败 ⇒ 500 `io_error`，`message` 带内核给的阶段名（写临时文件 / 刷盘 / 读回核对 / 原子替换）；
//! - 成功 ⇒ `bytes` 是**保存后磁盘上的真实长度**（`fs::metadata`，不是内存里的估算）。
//!
//! # 二、响应
//!
//! ```json
//! {"saved": true, "bytes": 5880052, "note": "已保存 …（5,880,052 字节）"}
//! ```
//!
//! `saved` 恒为 `true`：走到这里说明「文件此刻在磁盘上与内存一致」，
//! 包括「本来就没有修改、无需写盘」那种情况（`note` 会写明）。
//! 前端 `web/README.md` §3.6 只要求 `{saved, bytes}`，多出的 `note` 是任务书 §三 要求的。

use axum::extract::State;
use axum::Json;
use miredit_core::Command;
use serde_json::{json, Value};

use crate::error::{ApiError, ApiResult};
use crate::routes::edit::emit_error;
use crate::state::{display_path, Event, Shared};

/// `POST /api/save`（无请求体，或 `{}` —— 两者都接受）。
pub async fn save(State(st): State<Shared>) -> ApiResult<Json<Value>> {
    let (outcome, bytes, path) = {
        let mut guard = st.write_session();
        let session = guard.as_mut().ok_or_else(ApiError::no_map)?;
        let path = session.document().path.clone();

        let outcome = match miredit_core::apply(session, Command::Save) {
            Ok(o) => o,
            Err(e) => {
                let err = ApiError::from_core(e);
                // 事件必须在**锁外**发（广播不该发生在临界区里）
                drop(guard);
                emit_error(&st, &err);
                return Err(err);
            }
        };

        // 保存后磁盘上的真实长度：读不到就如实报错（不写 0 冒充）
        let bytes = std::fs::metadata(&path).map(|m| m.len()).map_err(|e| {
            ApiError::io_error(format!(
                "保存后读文件长度失败：{}（{e}）",
                display_path(&path)
            ))
        })?;
        (outcome, bytes, path)
    };

    // 保存完 ⇒ 一定不脏；广播出去让前端顶栏星号消失
    st.emit(Event::Dirty { dirty: outcome.dirty });
    println!(
        "[保存] {} ⇒ {} 字节（dirty={}）",
        display_path(&path),
        bytes,
        outcome.dirty
    );

    Ok(Json(json!({
        "saved": true,
        "bytes": bytes,
        "note": outcome.note,
    })))
}
