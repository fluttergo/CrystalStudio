//! 路由装配 + CORS 中间件。
//!
//! # 一、路由表（**与任务书 §三 一一对应**）
//!
//! | 方法 | 路径 | handler |
//! |---|---|---|
//! | GET | `/api/health` | [`health`] |
//! | GET | `/api/map` | `routes::map::get_map` |
//! | POST | `/api/map/open` | `routes::map::open_map` |
//! | GET | `/api/maps` | `routes::maps::list_maps`（地图列表，只读元数据 + 分页） |
//! | GET | `/api/tiles` | `routes::tiles::tiles` |
//! | POST | `/api/edit` | `routes::edit::edit` |
//! | POST | `/api/save` | `routes::save::save` |
//! | GET | `/api/palette` | `routes::palette::palette` |
//! | GET | `/api/stamps` | `routes::stamps::stamps` |
//! | GET | `/api/events` | `routes::events::events`（WebSocket 升级） |
//! | GET | `/` | `static_files::index` |
//! | GET | `/*` | `static_files::fallback`（未实现的 `/api/*` 走 JSON 404） |
//!
//! **没有的端点不返回编造内容**：任何未列出的 `/api/*` 都是 404 + JSON 错误体。
//!
//! # 二、CORS：只放行同源与 `localhost` 开发
//!
//! 任务书 §四要求「允许同源 + `localhost` 开发；不需要放开 `*`」。判据：
//!
//! - `Origin` 缺失 ⇒ 同源或非浏览器请求，什么都不加；
//! - `Origin` 是 `http://localhost:<端口>` / `http://127.0.0.1:<端口>` / `http://[::1]:<端口>`
//!   （任意端口）⇒ 回显该 `Origin` + `Vary: Origin` + `Expose-Headers`（见下条）；
//! - 其它来源 ⇒ **不加任何 CORS 头**（浏览器自己会拦住）。
//!
//! **`Access-Control-Expose-Headers` 不是可选项**：`Allow-Origin` 只让浏览器把**响应体**
//! 交给页面，非 CORS 安全清单里的响应头仍然读不到。本 crate 的自定义响应头是
//! `/api/tiles` 的 `X-Render-Scale`（`routes/tiles.rs`，表示**实际出图档位**）、
//! `X-Render-Doors`（本次门口径 `0`/`1`，排查「勾了开启门却没变」用）与
//! `X-Render-Lighting`（本次光照档位 `day|dawn_dusk|night`，排查「切了没反应」用）。
//! 前端 `api.js` 靠前者判断「请求档位与实测档位是否一致」，靠后者显示档位真值。
//! 不列进 `Expose-Headers`，跨源部署时 `response.headers.get('x-render-scale')` /
//! `get('x-render-lighting')` 恒为 `null` —— 诊断永久失效，且从界面上看不出失效。
//! 暴露清单集中在 [`EXPOSED_HEADERS`]，新增自定义响应头时改那一处即可。
//!
//! 为什么不用 `Access-Control-Allow-Origin: *`：一旦将来加了鉴权或写盘能力，
//! `*` 会让任意网页都能调 `/api/save` 改你的地图文件。
//!
//! 预检（`OPTIONS`）单独处理：`Allow-Methods: GET, POST, OPTIONS` +
//! `Allow-Headers: content-type` + `Max-Age: 600`。

use axum::extract::Request;
use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::routes;
use crate::state::{AppState, Shared};
use crate::static_files;

/// 组装整个应用（`main.rs` 与测试共用，保证两边路由一致）。
pub fn app(state: Shared) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/map", get(routes::map::get_map))
        .route("/api/map/open", post(routes::map::open_map))
        .route("/api/maps", get(routes::maps::list_maps))
        .route("/api/tiles", get(routes::tiles::tiles))
        .route("/api/edit", post(routes::edit::edit))
        .route("/api/save", post(routes::save::save))
        .route("/api/palette", get(routes::palette::palette))
        .route("/api/stamps", get(routes::stamps::stamps))
        .route("/api/events", get(routes::events::events))
        .route("/", get(static_files::index))
        .fallback(static_files::fallback)
        .layer(axum::middleware::from_fn(cors))
        .with_state(state)
}

/// `GET /api/health` —— 探活。前端 2.5s 超时，超时即判「未连接」。
///
/// 只要进程还能应答就返回 `ok:true`：**图库/印章目录不可用不算"进程不健康"**
/// （那是 `/api/tiles` 的 503 该说的事）。这样前端的「后端已连接」与
/// 「某个资源没配好」两个状态不会互相污染。
pub async fn health() -> Json<Value> {
    Json(json!({
        "ok": true,
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

/// 跨源时**必须暴露给页面 JS** 的自定义响应头（逗号分隔，HTTP 头名大小写不敏感）。
///
/// 现状三条，都是 `/api/tiles` 的（`routes/tiles.rs`）：
/// `X-Render-Scale`（实际出图档位）、`X-Render-Doors`（本次门口径 `0`/`1`）与
/// `X-Render-Lighting`（本次光照档位 `day|dawn_dusk|night`）。
/// 新增自定义响应头（`X-*`）时**必须**加到这里，否则跨源读不到 —— 见模块文档第二节。
const EXPOSED_HEADERS: &str = "x-render-scale, x-render-doors, x-render-lighting";

/// CORS 中间件（判据见模块文档第二节）。
async fn cors(req: Request, next: Next) -> Response {
    let origin = req
        .headers()
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let allowed = origin.filter(|o| is_allowed_origin(o));

    // 预检：直接答，不进业务 handler
    if req.method() == Method::OPTIONS {
        let mut resp = StatusCode::NO_CONTENT.into_response();
        let h = resp.headers_mut();
        h.insert(
            header::ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_static("GET, POST, OPTIONS"),
        );
        h.insert(
            header::ACCESS_CONTROL_ALLOW_HEADERS,
            HeaderValue::from_static("content-type"),
        );
        h.insert(
            header::ACCESS_CONTROL_MAX_AGE,
            HeaderValue::from_static("600"),
        );
        if let Some(o) = allowed {
            if let Ok(v) = HeaderValue::from_str(&o) {
                h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, v);
                h.insert(header::VARY, HeaderValue::from_static("Origin"));
            }
        }
        return resp;
    }

    let mut resp = next.run(req).await;
    // 前端契约 §3.0 要求 `application/json; charset=utf-8`；`axum::Json` 只给 `application/json`。
    // 这里统一补齐（JSON 本来就恒为 UTF-8，补 charset 只是让契约逐字对得上）。
    if resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        == Some("application/json")
    {
        resp.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json; charset=utf-8"),
        );
    }
    if let Some(o) = allowed {
        if let Ok(v) = HeaderValue::from_str(&o) {
            let h = resp.headers_mut();
            h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, v);
            h.insert(header::VARY, HeaderValue::from_static("Origin"));
            // 只放行响应体不够：不列进 Expose-Headers，页面 JS 读 `x-render-scale` 恒为 null
            h.insert(
                header::ACCESS_CONTROL_EXPOSE_HEADERS,
                HeaderValue::from_static(EXPOSED_HEADERS),
            );
        }
    }
    resp
}

/// 只放行 `localhost` / `127.0.0.1` / `[::1]`（任意端口）——本地开发够用，且不放 `*`。
fn is_allowed_origin(origin: &str) -> bool {
    let Some(rest) = origin.strip_prefix("http://") else {
        return false;
    };
    let host = rest.split('/').next().unwrap_or(rest);
    let host_no_port = match host.rsplit_once(':') {
        // `[::1]:5173` 这种带方括号的 IPv6
        Some((h, _)) if h.starts_with('[') => h,
        Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) => h,
        _ => host,
    };
    matches!(host_no_port, "localhost" | "127.0.0.1" | "[::1]")
}

/// 便于测试/诊断：把状态装成 `Arc`（`main.rs` 也用这个）。
pub fn share(state: AppState) -> Shared {
    std::sync::Arc::new(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 只放行本地来源() {
        assert!(is_allowed_origin("http://localhost:5173"));
        assert!(is_allowed_origin("http://localhost"));
        assert!(is_allowed_origin("http://127.0.0.1:8080"));
        assert!(is_allowed_origin("http://[::1]:8080"));
        // 不放行外部来源，也不放行 https（本服务只跑 http）
        assert!(!is_allowed_origin("http://evil.example.com"));
        assert!(!is_allowed_origin("https://localhost:5173"));
        assert!(!is_allowed_origin("http://localhost.evil.com"));
        assert!(!is_allowed_origin("null"));
    }

    /// 路由表必须能装起来（路径拼错 / handler 签名不对在这里就炸）。
    #[test]
    fn 路由可装配() {
        let state = share(AppState::from_env());
        let _router: Router = app(state);
    }

    /// 暴露清单必须含 `x-render-scale`（前端诊断的唯一依据）。
    #[test]
    fn 暴露清单含出图档位头() {
        let list: Vec<String> = EXPOSED_HEADERS
            .split(',')
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .collect();
        assert!(
            list.iter().any(|h| h == "x-render-scale"),
            "EXPOSED_HEADERS = {EXPOSED_HEADERS:?} 缺少 x-render-scale"
        );
    }

    /// 跨源请求 `/api/tiles` 必须**真能读到** `X-Render-Scale`：
    /// `Access-Control-Allow-Origin` 只放行响应体，非安全清单里的头还须列进
    /// `Access-Control-Expose-Headers`，否则页面 JS 读到的恒为 `null`，
    /// 前端 `p.scale || '1'` 的兜底值会冒充实测值。
    ///
    /// 这里起**真 TCP 服务 + 真 HTTP/1.1 请求**（不 mock 中间件），断言的是浏览器实际收到的字节。
    /// `/api/tiles` 在本机没图库时是 503、有图库但没开图时是 409 —— 都不是 200，但 CORS 头由
    /// 中间件统一加、**与业务状态码无关**，所以断言不受本机环境（有没有图库）影响。
    #[tokio::test(flavor = "multi_thread")]
    async fn 跨源请求能读到出图档位头() {
        use std::io::{Read as _, Write as _};

        let state = share(AppState::from_env());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("回环端口应可绑定");
        let addr = listener.local_addr().expect("应能取到本地地址");
        // 端口 0 ⇒ 由内核分配，不会与其它测试/进程抢端口
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app(state)).await;
        });

        // 客户端走**同步 std 套接字**：本 crate 没开 tokio 的 `io-util`，读不到 `AsyncReadExt`，
        // 而同步读写在多线程运行时里是安全的（只阻塞这一条测试线程）。
        let raw = std::thread::spawn(move || {
            let mut s = std::net::TcpStream::connect(addr).expect("应能连上测试服务");
            let _ = s.set_read_timeout(Some(std::time::Duration::from_secs(10)));
            let req = concat!(
                "GET /api/tiles?x=0&y=0&w=2&h=2 HTTP/1.1\r\n",
                "Host: localhost\r\n",
                "Origin: http://localhost:5173\r\n",
                "Connection: close\r\n\r\n"
            );
            s.write_all(req.as_bytes()).expect("应能发出请求");
            let mut buf = Vec::new();
            s.read_to_end(&mut buf).expect("应能读到完整响应");
            // 头名大小写不敏感，统一小写后按行找，避免受 hyper 大小写策略影响
            String::from_utf8_lossy(&buf).to_ascii_lowercase()
        })
        .join()
        .expect("客户端线程不应 panic");
        server.abort();

        assert!(raw.starts_with("http/1.1 "), "不是 HTTP 响应：{raw}");
        assert!(
            raw.contains("access-control-allow-origin: http://localhost:5173"),
            "跨源放行头缺失或写错：{raw}"
        );
        let expose = raw
            .lines()
            .find(|l| l.starts_with("access-control-expose-headers:"))
            .unwrap_or_else(|| panic!("响应里没有 access-control-expose-headers：{raw}"));
        assert!(
            expose.contains("x-render-scale"),
            "Expose-Headers 未包含 x-render-scale：{expose}"
        );
    }

    /// 暴露清单必须含 `x-render-doors` / `x-render-lighting`；新增头不能把旧的挤掉。
    #[test]
    fn 暴露清单含开启门与光照头() {
        let list: Vec<String> = EXPOSED_HEADERS
            .split(',')
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .collect();
        for must in ["x-render-scale", "x-render-doors", "x-render-lighting"] {
            assert!(
                list.iter().any(|h| h == must),
                "EXPOSED_HEADERS = {EXPOSED_HEADERS:?} 缺少 {must}"
            );
        }
    }

    /// 跨源时**三条**自定义头都必须真能读到（只改常量不算，得看浏览器实际收到的头）。
    ///
    /// 与上一条一样起真 TCP 服务；`/api/tiles` 在本机没开图时是 409、没图库时是 503，
    /// 但 CORS 头由中间件统一加、与业务状态码无关。
    #[tokio::test(flavor = "multi_thread")]
    async fn 跨源请求能读到开启门与光照头() {
        use std::io::{Read as _, Write as _};

        let state = share(AppState::from_env());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("回环端口应可绑定");
        let addr = listener.local_addr().expect("应能取到本地地址");
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app(state)).await;
        });

        let raw = std::thread::spawn(move || {
            let mut s = std::net::TcpStream::connect(addr).expect("应能连上测试服务");
            let _ = s.set_read_timeout(Some(std::time::Duration::from_secs(10)));
            let req = concat!(
                "GET /api/tiles?x=0&y=0&w=2&h=2&doors=1&lighting=night HTTP/1.1\r\n",
                "Host: localhost\r\n",
                "Origin: http://localhost:5173\r\n",
                "Connection: close\r\n\r\n"
            );
            s.write_all(req.as_bytes()).expect("应能发出请求");
            let mut buf = Vec::new();
            s.read_to_end(&mut buf).expect("应能读到完整响应");
            String::from_utf8_lossy(&buf).to_ascii_lowercase()
        })
        .join()
        .expect("客户端线程不应 panic");
        server.abort();

        let expose = raw
            .lines()
            .find(|l| l.starts_with("access-control-expose-headers:"))
            .unwrap_or_else(|| panic!("响应里没有 access-control-expose-headers：{raw}"));
        assert!(
            expose.contains("x-render-doors"),
            "Expose-Headers 未包含 x-render-doors：{expose}"
        );
        assert!(
            expose.contains("x-render-scale"),
            "Expose-Headers 未包含 x-render-scale：{expose}"
        );
        assert!(
            expose.contains("x-render-lighting"),
            "Expose-Headers 未包含 x-render-lighting：{expose}"
        );
    }

    /// 非白名单来源**一条 CORS 头都不能有**（含新增的 Expose-Headers）——
    /// 否则等于把诊断头泄露给任意网页。
    #[tokio::test(flavor = "multi_thread")]
    async fn 外部来源不加任何cors头() {
        use std::io::{Read as _, Write as _};

        let state = share(AppState::from_env());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("回环端口应可绑定");
        let addr = listener.local_addr().expect("应能取到本地地址");
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app(state)).await;
        });

        let raw = std::thread::spawn(move || {
            let mut s = std::net::TcpStream::connect(addr).expect("应能连上测试服务");
            let _ = s.set_read_timeout(Some(std::time::Duration::from_secs(10)));
            let req = concat!(
                "GET /api/health HTTP/1.1\r\n",
                "Host: localhost\r\n",
                "Origin: http://evil.example.com\r\n",
                "Connection: close\r\n\r\n"
            );
            s.write_all(req.as_bytes()).expect("应能发出请求");
            let mut buf = Vec::new();
            s.read_to_end(&mut buf).expect("应能读到完整响应");
            String::from_utf8_lossy(&buf).to_ascii_lowercase()
        })
        .join()
        .expect("客户端线程不应 panic");
        server.abort();

        assert!(raw.starts_with("http/1.1 "), "不是 HTTP 响应：{raw}");
        assert!(
            !raw.contains("access-control-"),
            "外部来源不该拿到任何 CORS 头：{raw}"
        );
    }
}
