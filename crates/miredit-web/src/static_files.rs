//! 静态前端：`GET /` 与 `GET /<其它路径>` 都从 `MIREDIT_WEB_ROOT`（缺省 = 本仓 `<仓库根>/web`）读。
//!
//! # 一、三条硬规矩（任务书 §三）
//!
//! 1. **找不到就 404** —— 绝不返回一个假的 `index.html`（那会让人以为前端接上了）；
//! 2. **不许目录穿越** —— `..` 一律拒绝，且拼出来的路径必须仍在 `web_root` 之内；
//! 3. **未实现的 `/api/*` 也是 404**（JSON 错误体，不是静态 404 页面）——
//!    这样前端拿到的是 `{"error":{"code":"not_found",…}}`，能照常走它的错误分支。
//!
//! # 二、为什么不用 `tower-http` 的 `ServeDir`
//!
//! 任务书 §四只允许 `axum` + `tokio` + `serde`。手写这 100 行换来的是：
//! 错误体与其它端点**同一套 JSON 形状**（`ServeDir` 会吐纯文本 404），
//! 以及「目录不存在」与「文件不存在」用同一句中文说明。

use axum::extract::{OriginalUri, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use std::path::Path as StdPath;

use crate::error::ApiError;
use crate::state::Shared;

/// `GET /`：前端入口页。
pub async fn index(State(st): State<Shared>) -> Response {
    serve_file(&st, "index.html").await
}

/// 未命中任何路由时的兜底：静态文件，或 `/api/*` 的 JSON 404。
///
/// ⚠ 这里**不能用** `Path<String>` 取路径：`Router::fallback` 是"没匹配上任何路由"的兜底，
/// 它**不携带路径参数**（用 `Path<String>` 会 500「Wrong number of path arguments」——
/// 这是本 crate 第一版真踩过的坑）。所以从 [`OriginalUri`] 里自己取原始路径。
pub async fn fallback(State(st): State<Shared>, OriginalUri(uri): OriginalUri) -> Response {
    let raw = uri.path().to_string();
    let trimmed = raw.trim_start_matches('/');
    // `/api/...` 走 JSON 404（见模块文档第一节第 3 条）
    if trimmed == "api" || trimmed.starts_with("api/") {
        return ApiError::not_found(format!("未实现的端点：/{trimmed}")).into_response();
    }
    if trimmed.is_empty() {
        return serve_file(&st, "index.html").await;
    }
    serve_file(&st, trimmed).await
}

/// 读 `web_root` 下的一个文件并返回（带 MIME）。
async fn serve_file(st: &Shared, rel: &str) -> Response {
    let Some(root) = st.web_root.as_ref() else {
        return ApiError::not_found(
            "静态前端目录不存在（设置 MIREDIT_WEB_ROOT 指向本仓的 web/ 目录）",
        )
        .into_response();
    };
    // 闸②：任何 `..` 分量一律拒绝（`\` 与 `/` 都算分隔符）
    if has_parent(rel) {
        return ApiError::forbidden_path(format!("静态路径不允许包含 `..`：{rel}")).into_response();
    }
    let candidate = root.join(rel);
    // 目录请求（如 `/css`）→ 404，不做目录列表
    let Ok(real) = tokio::fs::canonicalize(&candidate).await else {
        return ApiError::not_found(format!("静态文件不存在：{rel}")).into_response();
    };
    if !real.starts_with(root) {
        return ApiError::forbidden_path(format!("静态路径逃出根目录：{rel}")).into_response();
    }
    match tokio::fs::read(&real).await {
        Ok(bytes) => {
            let mime = mime_of(&real);
            (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, mime),
                    // 编辑器每次改动都重取瓦片，静态页缓存住反而更难排查
                    (header::CACHE_CONTROL, "no-cache"),
                ],
                bytes,
            )
                .into_response()
        }
        Err(e) => ApiError::not_found(format!("静态文件读不出：{rel}（{e}）")).into_response(),
    }
}

/// 按扩展名给 MIME（只覆盖本前端用到的几种；其它一律 `application/octet-stream`）。
fn mime_of(p: &StdPath) -> &'static str {
    match p
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("html") | Some("htm") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") | Some("mjs") => "text/javascript; charset=utf-8",
        Some("json") | Some("map") => "application/json; charset=utf-8",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("txt") | Some("md") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// 路径里是否有 `..` 分量（`\` 与 `/` 都算分隔符，与 `state.rs` 同口径）。
fn has_parent(rel: &str) -> bool {
    rel.split(['\\', '/']).any(|seg| seg == "..")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 拒绝穿越() {
        assert!(has_parent("../etc/passwd"));
        assert!(has_parent(r"..\..\windows\win.ini"));
        assert!(!has_parent("css/app.css"));
        assert!(!has_parent("index.html"));
        // 名字里带两个点不算分量
        assert!(!has_parent("tiles/a..b.png"));
    }

    #[test]
    fn mime_按扩展名() {
        assert_eq!(mime_of(StdPath::new("index.html")), "text/html; charset=utf-8");
        assert_eq!(mime_of(StdPath::new("a/b/app.css")), "text/css; charset=utf-8");
        assert_eq!(
            mime_of(StdPath::new("js/api.js")),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(mime_of(StdPath::new("tiles/0/25/0_0.png")), "image/png");
        assert_eq!(mime_of(StdPath::new("noext")), "application/octet-stream");
    }
}
