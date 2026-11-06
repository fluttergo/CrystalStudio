//! 统一错误体：**所有失败**都返回 `{"error":{"code":"…","message":"…"}}` + 合适状态码。
//!
//! # 一、为什么错误要单独一个模块
//!
//! 任务书 §四的硬要求是「**不许 200 带 error**」——即失败必须体现在 HTTP 状态码上。
//! 所以本模块只做一件事：把「一个错误」表达成 [`ApiError`]（状态码 + 机器码 + 中文），
//! 再统一由 [`IntoResponse`] 转成响应体。**任何 handler 都不许自己拼错误 JSON**，
//! 否则迟早会出现「这个端点 400、那个端点 200 + error」的两套口径。
//!
//! # 二、`code` 取值
//!
//! 与前端 `web/README.md` §3.10 的码表一致（前端对未列出的 code 一律按「未知错误」展示原文
//! `message`，所以新增码不会打破前端）。本 crate 用到的：
//!
//! | `code` | HTTP | 何时 |
//! |---|---|---|
//! | `bad_request` | 400 | 参数缺失 / 越界 / 枚举不认识（含 `cmd` 结构不对） |
//! | `forbidden_path` | 403 | 路径逃出白名单根（`..` 或指向根外）——本 crate 新增的码 |
//! | `not_found` | 404 | 文件 / 资源不存在（含静态文件与未实现的端点） |
//! | `no_map` | 409 | 尚未打开地图 |
//! | `too_large` | 413 | 出图超限（目标图 > 64 MiB 或 1× 中间图 > 128 MiB）——本 crate 新增的码 |
//! | `parse_failed` | 422 | 文件在，但 `.map` 三档判别都不命中 |
//! | `io_error` | 500 | 读写磁盘失败（保存阶段失败也归这里） |
//! | `assets_unavailable` | 503 | 图库 / 印章目录 / 地图列表（`MIREDIT_MAP_ROOT`）不可用（**不降级、不造假数据**） |
//! | `internal` | 500 | 其它内部错误（如 PNG 编码失败） |
//!
//! # 三、`CoreError` 的映射
//!
//! [`ApiError::from_core`] 是**唯一**把内核错误翻成 HTTP 的地方：
//! 层名不合法 ⇒ 400（这是调用方的输入错）、解析失败 ⇒ 422、读写/保存失败 ⇒ 500。
//! 注意「被拒绝的落笔」**不是错误**（`changed == 0` + `note`），它走正常 200。
//!
//! # 四、`StampConvertError` 的映射
//!
//! [`ApiError::from_stamp_convert`] 是 `.X 印章 → 格表`（`miredit_core::cells_from_stamp`）
//! 那一条链的**唯一**映射点，固定 **422**、码沿用 `parse_failed`，`message` **逐字**用 core 的文本。
//! 理由见该函数的文档。

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use miredit_core::{CoreError, StampConvertError};
use serde_json::json;

/// 一个 API 错误：状态码 + 机器可读 `code` + 给人看的中文 `message`。
#[derive(Debug, Clone)]
pub struct ApiError {
    /// HTTP 状态码（**必须**是 4xx/5xx，绝不允许 2xx 带 error）。
    status: StatusCode,
    /// 机器可读的错误码（见模块文档第二节的表）。
    code: &'static str,
    /// 中文说明（直接展示给用户，不含内部调试信息）。
    message: String,
}

impl ApiError {
    /// 通用构造：显式给状态码与码名。
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }

    /// 400：参数缺失 / 越界 / 枚举不认识。
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "bad_request", message)
    }

    /// 403：路径逃出白名单根。
    pub fn forbidden_path(message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, "forbidden_path", message)
    }

    /// 404：文件 / 资源不存在。
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", message)
    }

    /// 409：尚未打开地图。
    pub fn no_map() -> Self {
        Self::new(StatusCode::CONFLICT, "no_map", "尚未打开地图")
    }

    /// 413：**出图超限** —— 请求本身合法，只是要的产物超过渲染库的字节护栏。
    ///
    /// 由 `/api/tiles` 的两条字节上限触发（`RenderError::TooLarge` 目标图 64 MiB /
    /// `RenderError::IntermediateTooLarge` 1× 中间图 128 MiB）。
    /// 为什么不并进 400：这不是「参数写错」而是「服务端不肯为这一发分配这么多内存」，
    /// 换档（`scale=2 → 1`）或缩格数就能成功；也**不许**变成 500（那不是故障）。
    pub fn too_large(message: impl Into<String>) -> Self {
        Self::new(StatusCode::PAYLOAD_TOO_LARGE, "too_large", message)
    }

    /// 422：文件在，但解析不了。
    pub fn parse_failed(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNPROCESSABLE_ENTITY, "parse_failed", message)
    }

    /// 500：读写磁盘失败。
    pub fn io_error(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "io_error", message)
    }

    /// 503：图库 / 印章目录不可用。
    pub fn assets_unavailable(message: impl Into<String>) -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, "assets_unavailable", message)
    }

    /// 500：其它内部错误。
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", message)
    }

    /// HTTP 状态码（日志与测试用）。
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// 机器码（日志与测试用）。
    pub fn code(&self) -> &'static str {
        self.code
    }

    /// 中文说明（WS 的 `error` 事件要原样带出去）。
    pub fn message(&self) -> &str {
        &self.message
    }

    /// 把内核错误翻成 HTTP 错误 —— **全仓唯一**的映射点（见模块文档第三节）。
    pub fn from_core(err: CoreError) -> Self {
        match err {
            // 层名不认识：调用方传错了，属输入错
            CoreError::BadLayer { .. } => Self::bad_request(err.to_string()),
            // 三档判别都不命中：文件在但读不懂
            CoreError::Map { .. } => Self::parse_failed(err.to_string()),
            // 读文件失败
            CoreError::Io { .. } => Self::io_error(err.to_string()),
            // 原子保存失败（原文件未改动）
            CoreError::Save { .. } => Self::io_error(err.to_string()),
        }
    }

    /// 把 `.X → 格表` 的转换错误翻成 HTTP 错误：**422 + `parse_failed`**（见模块文档第四节）。
    ///
    /// 为什么是 422：`.X` 文件确实在、也确实解析成功了，坏的是**文件内容**
    /// （某一族库号不在 `0..=255`，地图侧单字节装不下）。这不是「参数写错」（400），
    /// 也不是「服务端故障」（500）—— 换一枚印章就能成功，同一请求重发也不会变。
    ///
    /// 为什么码不新造一个：前端 `web/README.md` §3.10 的码表里 **422 只有 `parse_failed`**。
    /// 新造码会让前端落到「未知错误」分支（原文能显示，但分类丢了），
    /// 而这里要表达的东西与「文件在但读不懂」同类 —— 都是「这份资源的内容不能用」。
    ///
    /// `message` **原样**用 core 的 [`StampConvertError`] 文本，本函数不加任何前缀：
    /// 那句里已经有「第 N 格 / 地表|中层|物体 / 值 X」（`N` 从 1 起），
    /// 套一层前缀只会把它埋起来，前端想抠出来还得写正则。
    pub fn from_stamp_convert(err: StampConvertError) -> Self {
        Self::parse_failed(err.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = json!({
            "error": {
                "code": self.code,
                "message": self.message,
            }
        });
        (self.status, Json(body)).into_response()
    }
}

/// handler 的返回类型别名（少写一坨 `Result<_, ApiError>`）。
pub type ApiResult<T> = Result<T, ApiError>;
