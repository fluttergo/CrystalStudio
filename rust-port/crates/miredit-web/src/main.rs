//! `miredit-web` —— 网页版地图编辑器的 **HTTP 后端**（二进制入口）。
//!
//! # 一、它是什么
//!
//! 把两个库暴露成 API：
//!
//! - **`miredit-core`**（编辑内核）：`Document` / `EditSession` / `Command` / `apply` /
//!   `save_session` / `set_brush_from_palette` —— **所有编辑都走它**，本 crate 不自己改地图；
//! - **`miredit-render`**（出图）：`try_render_region_with` + `png_io` —— **所有出图都走它**，
//!   本 crate 不自己写 PNG 编码。
//!
//! 本 crate 自己只做四件事：HTTP/WS 框架（axum）、JSON ↔ `Command` 的直译、
//! 路径白名单、进程内状态与锁。
//!
//! # 二、怎么跑
//!
//! ```text
//! CARGO_INCREMENTAL=0  CARGO_TARGET_DIR='<任意可写目录>/miredit-web-target'
//! MIREDIT_MAP_ROOT=<地图目录>  MIREDIT_CLIENT_ROOT=<客户端根>  \
//! MIREDIT_PALETTE_JSON=<dataRoot>/catalogue/palette.json  \
//! cargo run -p miredit-web --release
//! # 浏览器打开 http://127.0.0.1:8080/（前端目录存在时）或直接 curl 各端点
//! ```
//!
//! 环境变量见 [`state`] 的模块文档（`MIREDIT_MAP_ROOT` / `MIREDIT_CLIENT_ROOT` /
//! `MIREDIT_PALETTE_JSON` / `MIREDIT_WEB_ROOT` / `MIREDIT_BIND`）。
//!
//! # 三、模块地图
//!
//! | 模块 | 负责 |
//! |---|---|
//! | [`server`] | 路由表 + CORS 中间件 + `/api/health` |
//! | [`state`] | 共享状态（会话锁 / 图库 / 印章目录 / 广播）+ 路径白名单 |
//! | [`json`] | `miredit_core::Command` 的 JSON 直译（手写，内核不引 serde） |
//! | [`error`] | 统一错误体 `{"error":{"code","message"}}` |
//! | [`static_files`] | `../web/` 静态文件（找不到就 404，不伪造页面） |
//! | [`routes`] | 各端点 handler，一域一个文件 |
//!
//! # 四、关掉服务的两种方式
//!
//! `Ctrl-C` 与（Linux）`SIGTERM` 都会走优雅关闭 —— 容器里 `docker stop` 发的是 SIGTERM，
//! 不接它就等于每次都要等 10 秒被 SIGKILL。

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod error;
mod json;
mod routes;
mod server;
mod state;
mod static_files;

use std::net::SocketAddr;

use state::AppState;

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("[启动失败] {e}");
        std::process::exit(1);
    }
}

/// 读 env → 建状态 → 起服务。
async fn run() -> Result<(), String> {
    let bind = std::env::var("MIREDIT_BIND")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| state::DEFAULT_BIND.to_string());
    let addr: SocketAddr = bind
        .parse()
        .map_err(|e| format!("MIREDIT_BIND = {bind:?} 不是合法的 host:port（{e}）"))?;

    let state = server::share(AppState::from_env());

    // 启动横幅：把「实际用了哪些路径」打出来，省得靠猜（Docker 里尤其重要）
    println!("[启动] miredit-web v{}", env!("CARGO_PKG_VERSION"));
    println!("[启动] 监听 {addr}");
    println!(
        "[启动] 地图白名单根 = {}",
        if state.map_root.as_os_str().is_empty() {
            "未设置 MIREDIT_MAP_ROOT ⇒ 地图端点一律 503（不猜缺省目录）".to_owned()
        } else {
            state::display_path(&state.map_root)
        }
    );
    match state.web_root.as_ref() {
        Some(p) => println!("[启动] 静态前端目录 = {}", state::display_path(p)),
        None => println!("[启动] 静态前端目录 = 不存在（GET / 将返回 404，不伪造页面）"),
    }
    println!(
        "[启动] 印章目录索引 = {}",
        if state.palette_json.as_os_str().is_empty() {
            "未设置 MIREDIT_PALETTE_JSON ⇒ /api/stamps 503（不猜缺省索引）".to_owned()
        } else if state.palette_json.is_file() {
            format!(
                "{}（存在，首次 /api/stamps 时加载）",
                state::display_path(&state.palette_json)
            )
        } else {
            format!(
                "{}（不存在，/api/stamps 将返回 503）",
                state::display_path(&state.palette_json)
            )
        }
    );

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| format!("监听 {addr} 失败：{e}"))?;
    println!("[启动] 就绪");

    axum::serve(listener, server::app(state))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|e| format!("服务异常结束：{e}"))?;
    println!("[退出] 已停止");
    Ok(())
}

/// 等 Ctrl-C（任何平台）或 SIGTERM（Linux 容器）。
async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            eprintln!("[退出] 无法监听 Ctrl-C：{e}");
            // 监听不了就一直等 SIGTERM 分支（好过假装能优雅关闭）
            std::future::pending::<()>().await;
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(e) => {
                eprintln!("[退出] 无法监听 SIGTERM：{e}");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
    println!("[退出] 收到停止信号，等待在途请求结束…");
}
