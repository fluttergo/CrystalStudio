//! `WS /api/events` —— 服务端**单向**推送（任务书 §三）。
//!
//! # 一、报文
//!
//! ```json
//! {"type":"dirty","payload":{"dirty":true}}
//! {"type":"rerender","payload":{"x":0,"y":0,"w":1,"h":1}}
//! {"type":"error","payload":{"code":"io_error","message":"保存失败：…"}}
//! ```
//!
//! 三种 `type` 与前端 `web/README.md` §3.9 完全一致；`payload` 里的字段见
//! [`crate::state::Event::to_json`]。
//!
//! # 二、为什么用「心跳 + 事件」两条腿，而不是 `split()` 读写分离
//!
//! `WebSocket::split()` 需要 `futures_util::StreamExt`，而本 crate 的依赖白名单只有
//! `axum` + `tokio` + `serde`（任务书 §四）。所以这里用最朴素的办法：
//!
//! ```text
//! loop {
//!     select! {
//!         _ = 心跳(30s)  => socket.send(Ping)   // 顺便探活：对端走了 send 会 Err
//!         ev = rx.recv() => socket.send(JSON)   // 有事件立刻下发
//!     }
//! }
//! ```
//!
//! 两条臂只有一条会碰 `socket`，因此**不违反借用规则**；代价是「客户端主动关闭」
//! 最多要等到下一次心跳/事件才被感知（30s 内一定发现，不会泄漏连接）。
//! 客户端发来的任何消息都**忽略**（本通道是单向的，编辑走 HTTP `/api/edit`）。
//!
//! # 三、背压
//!
//! `broadcast` 容量 64。客户端追不上时 `recv()` 返回 `Lagged(n)`：这里**跳过**并继续
//! （丢掉的是过期的中间态，下一条 `dirty`/`rerender` 会带来最新状态），
//! 不会因此断开连接，也不会把错误算到编辑器头上。

use axum::body::Bytes;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use std::time::Duration;
use tokio::sync::broadcast::error::RecvError;

use crate::state::{Event, Shared};

/// 心跳间隔：既是探活，也是「客户端关闭」的发现窗口。
const HEARTBEAT: Duration = Duration::from_secs(30);

/// `GET /api/events`（升级为 WebSocket）。
pub async fn events(ws: WebSocketUpgrade, State(st): State<Shared>) -> Response {
    let rx = st.events.subscribe();
    ws.on_upgrade(move |socket| pump(socket, rx))
}

/// 把广播通道里的事件一条条写进 WS。
///
/// ⚠ **只有「心跳」与「事件」两条臂**，且只有一条会碰 `socket` —— 这是借用规则的要求：
/// `select!` 会先把所有分支的 future 都造出来，若再加一条 `socket.recv()` 分支，
/// 那条分支的可变借用会与心跳臂里的 `socket.send()` 冲突（编译期就报错）。
/// 代价：客户端主动关闭最多 30s 后才被感知（见模块文档第二节）。
async fn pump(mut socket: WebSocket, mut rx: tokio::sync::broadcast::Receiver<Event>) {
    let mut ticker = tokio::time::interval(HEARTBEAT);
    // 第一次 tick 会立刻返回，跳过它（否则连上就发一个 Ping，没必要）
    ticker.tick().await;
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                // Ping 同时承担「探活」职责：对端已经走了，send 会立刻 Err
                if socket.send(Message::Ping(Bytes::new())).await.is_err() {
                    break;
                }
            }
            got = rx.recv() => match got {
                Ok(ev) => {
                    let text = ev.to_json().to_string();
                    if socket.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
                }
                // 追不上：丢掉过期中间态，继续（见模块文档第三节）
                Err(RecvError::Lagged(n)) => {
                    eprintln!("[WS] 客户端落后 {n} 条事件，已跳过");
                    continue;
                }
                // 发送端没了（服务在收尾）⇒ 正常结束
                Err(RecvError::Closed) => break,
            },
        }
    }
}
