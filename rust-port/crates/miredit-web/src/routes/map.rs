//! `GET /api/map` 与 `POST /api/map/open`。
//!
//! # 一、响应字段
//!
//! 干净图（`GodotClient/maps/0.map`：700×700 / 12 B 每格 / 尾块 0，**实测**）：
//!
//! ```json
//! {"path":"E:\\…\\0.map","width":700,"height":700,"format":"Old12",
//!  "dirty":false,"tolerant_trailer_bytes":0,
//!  "format_label":"Old12（12B/格，TMapInfo 旧版）","editable":true,
//!  "ground_paint_supported":true,"edit_warning":null,
//!  "disk_cell_len":12}
//! ```
//!
//! 带尾块的 Old12（`client/热血传奇23周年客户端/Map/EM001.map`：30×35 / 尾块 8,040 B，**实测**）
//! —— `editable` 仍是 `true`，`edit_warning` **只剩尾块那一句**，格式段一个字都没有：
//!
//! ```json
//! {"path":"E:\\…\\EM001.map","width":30,"height":35,"format":"Old12",
//!  "dirty":false,"tolerant_trailer_bytes":8040,
//!  "format_label":"Old12（12B/格，TMapInfo 旧版）","editable":true,
//!  "ground_paint_supported":true,
//!  "edit_warning":"本图有 8040 字节未解释尾块（按头部 30×35 解释）——若真实尺寸不是头部声明值，列优先展开会让整图错位",
//!  "disk_cell_len":12}
//! ```
//!
//! | 字段 | 来源 | 说明 |
//! |---|---|---|
//! | `path` | `Document::path` | 绝对路径（去掉 Windows `\\?\` 前缀）；没开图 ⇒ `null` |
//! | `width` / `height` | `Document::size()` | 世界格 |
//! | `format` | `Document::format_name()` | `Old12` / `New14` / `Ext36` —— 这是**视图档位**（三档语义投影），不是磁盘格长 |
//! | `dirty` | `EditSession::dirty()` | 有未保存修改 |
//! | `tolerant_trailer_bytes` | `mir2_format::read_any_tolerant` 的 `RawMap::trailer_len()` | **只有宽松判别才非 0** |
//! | `format_label` | `Document::format_label()` | **九种格式的真名**（带格长与出处），例如 `Shanda14（14B/格，TNewMapInfo）` |
//! | `editable` | `Document::caps().editable` | 该格式是否承诺「改格后语义自洽」。**九种里只有 `Old12` / `Shanda14` 为 `true`**（2026-11-05 起；`Old12` 旧值是 `false`，已订正）；其余 7 种为 `false`，各带一句**具体原因**（`FormatCaps::editable_caveat`，例如 `Shanda2012` 的「每格 22 字节未建模尾区（格内 rel 14-35）…」）。⚠ `false` **不等于不能编辑** |
//! | `ground_paint_supported` | `Document::ground_paint_supported()` | 地表能否**逐格**落笔（九种里只有 `Mir3Wemade`/Type5 为 `false`） |
//! | `edit_warning` | `EditSession::edit_warning()` | **常驻告警原文**（`null` = 没事）。前端要把它整句上屏，不许改写 |
//! | `disk_cell_len` | `Document::disk_cell_len()` | **磁盘单格字节数**（`null` = 变长格式，如 `CrystalCs`；`Wemade2010` 定长 15 B/格 ⇒ `15`） |
//!
//! **没打开地图时**：`path` / `format` / `format_label` / `tolerant_trailer_bytes` /
//! `edit_warning` / `disk_cell_len` 全为 `null`，`editable` / `ground_paint_supported` 也为 `null`，
//! `width` / `height` 为 `0`。
//! ⚠ 这两个布尔**不写 `false`**：`false` 是「这个格式不支持」，`null` 才是「没有图，谈不上」——
//! 写 `false` 会让前端把"没开图"误报成"该格式不可落笔"（假数据）。
//!
//! ## 1.1 `editable` 与 `edit_warning` 的契约（2026-11-05 订正）
//!
//! 契约真源在 `mir2-format/src/registry.rs` 的 `editable与caveat必须一致`：
//! **`editable == false` ⟺ `FormatCaps::editable_caveat` 非空**。本 crate **只转发、不推断**
//! （`editable` 取 `caps().editable`，`edit_warning` 取 `EditSession::edit_warning()`）。
//!
//! 由此得到两条**可断言**的推论（本文件测试段的 `editable与告警互为反相` 钉住）：
//!
//! 1. `editable == false` ⇒ `edit_warning` **必然非空**（至少含那句 caveat；
//!    若同时有未解尾块，两段用「；」连接，尾块那句在前）；
//! 2. `editable == true`（只有 `Old12` / `Shanda14`）⇒ 格式段**一个字都不出**，
//!    `edit_warning` 只可能来自未解尾块 ⇒ **尾块 0 时恒为 `null`**。
//!
//! ⚠ 订正记录：旧口径 `editable` 只有 `Shanda14` 为真、且 `edit_warning` 对**任何**格式都追加
//! 一句「本格式（X）未解区语义未知…」。对 `Old12` 那是事实错误（12 字节全部有名字、无未解区），
//! 后果是 1,158 张真实图里 **1,051 张（90.8%，含缺省打开的 `0.map`）**一开图就挂常驻黄条。
//! 改判 + 拆出 `editable_caveat` 之后，告警覆盖降到 **262 张**
//! （`Old12` 856 张里只剩 67 张因真尾块告警）。这两个数字来自 `mir2-format` 的
//! `real_samples.rs::告警覆盖实测`（`MIR2_GATE_CORPUS=1` 才跑），**不是本 crate 估的**。
//!
//! # 二、`tolerant_trailer_bytes` 为什么要单独算
//!
//! `miredit-core` 用 `mir2_map::Map::parse`（**长度必须精确整除**），它不告诉你
//! 「这个文件有多少字节没被解释」。而 `mir2-format` 的宽松判别会把多出来的字节收进
//! `trailer` 并**明确要求上层告警**（`mir2-format/src/registry.rs` 的契约段）。
//! 所以本字段 = 「这份文件在宽松口径下有多少字节没被解释」：**0 才是干净**。
//! 算不出来（文件读不了 / 九种格式都不命中）⇒ `null`，**不写 0 冒充干净**。
//!
//! 结果按路径缓存：地图文件不变时只需读一次 5.6 MB。

use axum::body::Bytes;
use axum::extract::State;
use axum::Json;
use miredit_core::EditSession;
use serde_json::{json, Value};

use crate::error::{ApiError, ApiResult};
use crate::state::{display_path, Event, Shared};
use crate::routes::parse_body;

/// `GET /api/map`：当前地图元数据；**没打开也算成功**（返回全 null / 0）。
///
/// 前端的判据是 `path === null` ⇒ 显示「后端已连接，但没有打开地图」
/// （`web/README.md` §3.2），所以这里不能报 409。
pub async fn get_map(State(st): State<Shared>) -> Json<Value> {
    let guard = st.read_session();
    match guard.as_ref() {
        None => Json(empty_map_json()),
        Some(session) => Json(map_json(&st, session)),
    }
}

/// 没打开地图时的响应体（单独成函数：字段齐全这件事要能被单测钉住）。
///
/// ⚠ 两个能力布尔写 `null` 而不是 `false`：`false` 是「这个格式不支持」，
/// `null` 才是「没有图，谈不上」（理由见模块文档第一节末）。
pub fn empty_map_json() -> Value {
    json!({
        "path": Value::Null,
        "width": 0,
        "height": 0,
        "format": Value::Null,
        "dirty": false,
        "tolerant_trailer_bytes": Value::Null,
        "format_label": Value::Null,
        "editable": Value::Null,
        "ground_paint_supported": Value::Null,
        "edit_warning": Value::Null,
        "disk_cell_len": Value::Null,
    })
}

/// `POST /api/map/open`：打开一张地图（**只允许白名单根内**）。
///
/// 错误：空 `path` ⇒ 400；`..` 或根外 ⇒ 403；不存在 ⇒ 404；解析不了 ⇒ 422。
pub async fn open_map(State(st): State<Shared>, body: Bytes) -> ApiResult<Json<Value>> {
    let v = parse_body(&body)?;
    let requested = v
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::bad_request("请求体缺少字符串字段 path"))?;
    // 白名单四道闸（含 `..` 拒绝）全在 state 里，这里不重复实现
    let path = st.resolve_map_path(requested)?;

    // 读 + 解析 5 MB 级文件：扔进阻塞线程池，别占 tokio worker
    let load_path = path.clone();
    let loaded = tokio::task::spawn_blocking(move || EditSession::load(&load_path))
        .await
        .map_err(|e| ApiError::internal(format!("打开任务异常结束：{e}")))?;
    let session = match loaded {
        Ok(s) => s,
        Err(e) => {
            // 解析失败（422）与读盘失败（500）由 from_core 分流
            return Err(ApiError::from_core(e));
        }
    };

    let info = {
        let mut guard = st.write_session();
        *guard = Some(session);
        // 刚写进去的东西立刻读回来：这里**不用 `expect`**（生产路径零 panic），
        // 万一真取不到就如实报内部错误，而不是把整个进程带走。
        match guard.as_ref() {
            Some(s) => map_json(&st, s),
            None => return Err(ApiError::internal("会话写入后读回失败")),
        }
    };
    st.emit(Event::Dirty { dirty: false });
    println!("[打开] {}", info["path"].as_str().unwrap_or("?"));
    Ok(Json(info))
}

/// 把会话状态装成响应体（`get_map` 与 `open_map` 共用，保证两处字段永远一致）。
///
/// 字段来源全部是 `miredit-core` 的公开访问器，**本 crate 不自己推断格式**：
/// 判别真值在 `mir2-format`，`Document` 只是把它投影出来（见模块文档第一节）。
fn map_json(st: &Shared, session: &EditSession) -> Value {
    let trailer = tolerant_trailer_bytes(st, &session.document().path);
    map_fields(session, trailer)
}

/// 会话 → 响应字段（**纯函数**：不碰磁盘、不碰 `AppState`，便于单测钉住字段来源）。
///
/// `trailer` 由调用方算好传进来（它要读盘，且带路径缓存，见
/// [`tolerant_trailer_bytes`]）。其余字段全部来自 `EditSession` / `Document` 的公开访问器。
pub fn map_fields(session: &EditSession, trailer: Option<usize>) -> Value {
    let doc = session.document();
    let (width, height) = doc.size();
    json!({
        "path": display_path(&doc.path),
        "width": width,
        "height": height,
        "format": doc.format_name(),
        "dirty": session.dirty(),
        "tolerant_trailer_bytes": trailer,
        "format_label": doc.format_label(),
        "editable": doc.caps().editable,
        "ground_paint_supported": doc.ground_paint_supported(),
        // 原样转发（None ⇒ null）。前端要把它**整句**上屏，这里不做任何改写/截断
        "edit_warning": session.edit_warning(),
        // 变长格式（`CrystalCs`）⇒ null，**不写 0 冒充定长**；`Wemade2010` 定长 15 B/格 ⇒ 返回 15
        "disk_cell_len": doc.disk_cell_len(),
    })
}

/// 宽松口径下「格数组之后还有多少字节没被解释」。
///
/// - 读不到文件 / 九种格式都不命中 ⇒ `None`（**不写 0 冒充干净**）；
/// - 严格判别成立 ⇒ `Some(0)`；
/// - 走了宽松判别 ⇒ `Some(非 0)`（上层必须告警，见 `mir2-format/src/registry.rs`）。
fn tolerant_trailer_bytes(st: &Shared, path: &std::path::Path) -> Option<usize> {
    let key = display_path(path);
    {
        let cache = st.trailer_cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((cached_path, value)) = cache.as_ref() {
            if *cached_path == key {
                return *value;
            }
        }
    }
    let value = std::fs::read(path)
        .ok()
        .and_then(|bytes| mir2_format::read_any_tolerant(&bytes).ok())
        .map(|m| m.trailer_len());
    if let Some(n) = value {
        if n != 0 {
            eprintln!(
                "[告警] {key} 有 {n} 字节未被解释（宽松判别）——按头部 w·h 解释，整图可能错位"
            );
        }
    }
    let mut cache = st.trailer_cache.lock().unwrap_or_else(|e| e.into_inner());
    *cache = Some((key, value));
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// 资产根缺省值：本机真实根（**整个测试模块里只出现这一处**）。换机器用 `MIREDIT_ASSET_ROOT`。
    const 缺省资产根: &str = r"E:\GodM2";

    /// 资产根：`MIREDIT_ASSET_ROOT` → 缺省 [`缺省资产根`]。
    ///
    /// 根目录不存在（公开仓/PR 的机器上没有游戏素材）⇒ `None`，调用方打印 `[跳过]` 并返回。
    /// 只做 `var_os` + `PathBuf`，不引入任何第三方库。
    fn 资产根() -> Option<std::path::PathBuf> {
        let root = std::env::var_os("MIREDIT_ASSET_ROOT")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(缺省资产根));
        root.is_dir().then_some(root)
    }

    /// 23 周年客户端地图目录（`client/热血传奇23周年客户端/Map`，挂在资产根下）。
    fn 语料_b(root: &Path) -> std::path::PathBuf {
        root.join("client").join("热血传奇23周年客户端").join("Map")
    }

    /// Godot 客户端自带地图目录（`GodotClient/maps`，挂在资产根下）。
    fn 语料_a(root: &Path) -> std::path::PathBuf {
        root.join("GodotClient").join("maps")
    }

    /// `/api/map` 的**字段全集**（少一个就是断链：前端按名字取值，缺了就是 `undefined`）。
    const 地图字段: [&str; 11] = [
        "path",
        "width",
        "height",
        "format",
        "dirty",
        "tolerant_trailer_bytes",
        "format_label",
        "editable",
        "ground_paint_supported",
        "edit_warning",
        "disk_cell_len",
    ];

    /// 没打开地图时字段必须**齐全**，且能力布尔是 `null` 而不是 `false`。
    ///
    /// 判据：`null` = 没有图，谈不上；`false` = 该格式不支持 —— 两者混用会让前端
    /// 把"没开图"误报成"该格式不可落笔"（假数据）。
    #[test]
    fn 未开图字段齐全且能力位为null() {
        let v = empty_map_json();
        for k in 地图字段 {
            assert!(v.get(k).is_some(), "未开图的响应缺字段 {k}");
        }
        assert_eq!(v["path"], Value::Null);
        assert_eq!(v["width"], 0);
        assert_eq!(v["editable"], Value::Null, "editable 不能写 false 冒充「该格式不支持」");
        assert_eq!(v["ground_paint_supported"], Value::Null);
        assert_eq!(v["disk_cell_len"], Value::Null);
        assert_eq!(v["edit_warning"], Value::Null);
    }

    /// 真图：字段值必须来自 `miredit-core` 的公开访问器（本 crate 不自己推断格式）。
    ///
    /// 环境缺资源不算代码错 —— 与 `routes/mod.rs` 的 `出图与render_maps逐字节相同` 同一口径。
    #[test]
    fn 真图字段来自内核() {
        let Some(root) = 资产根() else {
            eprintln!("[跳过] 无资产根 MIREDIT_ASSET_ROOT（需要真实地图素材）");
            return;
        };
        let path = 语料_a(&root).join("41.map");
        let path = path.as_path();
        if !path.is_file() {
            eprintln!("[跳过] 真实地图不存在：{}", path.display());
            return;
        }
        let session = EditSession::load(path).expect("打开 41.map");
        let v = map_fields(&session, Some(0));
        for k in 地图字段 {
            assert!(v.get(k).is_some(), "响应缺字段 {k}");
        }
        // 41.map = 200×200 / 14B 每格 ⇒ Shanda14（承诺可编辑的两种格式之一，另一种是 Old12）
        assert_eq!(v["width"], 200);
        assert_eq!(v["height"], 200);
        assert_eq!(v["disk_cell_len"], 14);
        assert_eq!(v["editable"], true, "Shanda14 必须承诺可编辑语义");
        assert_eq!(v["ground_paint_supported"], true);
        assert_eq!(v["edit_warning"], Value::Null, "干净文件不该有常驻告警");
        let label = v["format_label"].as_str().expect("format_label 必须是字符串");
        assert!(label.contains("Shanda14"), "format_label 实际 = {label}");
    }

    /// 带尾块的 Old12 真图（`EM001.map`，8,040 B 尾块）：`edit_warning` **只含尾块那一句**。
    ///
    /// # 2026-11-05 订正（旧断言必红，且**不是**放宽而是改对）
    ///
    /// 旧断言写的是 `editable == false` + `warn.contains("Old12")` —— 那是旧口径的产物：
    /// `Old12` 当时 `editable = false`，`edit_warning` 对任何格式都追加「本格式（X）未解区语义未知」。
    /// 现在 `Old12` 的 12 字节全部有名字、无未解区 ⇒ `editable = true`、格式段一个字都不出。
    /// ⇒ 本测试改为钉**新语义**：`editable == true`；告警里**有**尾块那句；**没有**格式段。
    #[test]
    fn 带尾块的真图必给告警() {
        let Some(root) = 资产根() else {
            eprintln!("[跳过] 无资产根 MIREDIT_ASSET_ROOT（需要真实地图素材）");
            return;
        };
        let path = 语料_b(&root).join("EM001.map");
        let path = path.as_path();
        if !path.is_file() {
            eprintln!("[跳过] 真实地图不存在：{}", path.display());
            return;
        }
        let session = EditSession::load(path).expect("打开 EM001.map");
        let trailer = std::fs::read(path)
            .ok()
            .and_then(|b| mir2_format::read_any_tolerant(&b).ok())
            .map(|m| m.trailer_len());
        let v = map_fields(&session, trailer);
        assert_eq!(v["tolerant_trailer_bytes"], 8040);
        let warn = v["edit_warning"].as_str().expect("尾块图必须有告警原文");
        assert!(warn.contains("8040"), "告警里必须写出未解释字节数，实际 = {warn}");
        // 12B/格 ⇒ Old12 ⇒ **承诺**编辑语义（2026-11-05 起）⇒ 告警里不该有格式段
        assert_eq!(v["disk_cell_len"], 12);
        assert_eq!(v["editable"], true, "Old12 已改判为承诺可编辑语义");
        assert!(
            !warn.contains("Old12"),
            "Old12 不该再出现在告警里（格式段已取消），实际 = {warn}"
        );
        assert!(
            !warn.contains("本格式"),
            "告警必须只剩尾块那一句，不许带格式段，实际 = {warn}"
        );
        assert!(
            !warn.contains("未解区语义未知"),
            "「未解区语义未知」是旧口径的套话，对 Old12 是事实错误，实际 = {warn}"
        );
        // 「只含尾块那句」的严格形态：告警**恰好等于**内核拼出来的那一句（分号连接的段落只有 1 段）
        assert!(
            !warn.contains('；'),
            "只有一段告警时不该出现分隔符「；」，实际 = {warn}"
        );
        assert!(
            warn.starts_with("本图有 8040 字节未解释尾块"),
            "告警必须以尾块那句开头，实际 = {warn}"
        );
    }

    /// **契约**：`editable == false` ⇒ `edit_warning` 必非空；`editable == true` 且无尾块 ⇒ 必为 `null`。
    ///
    /// 契约真源是 `mir2-format/src/registry.rs` 的 `editable与caveat必须一致`
    /// （`editable == false` ⟺ `editable_caveat` 非空），本测试从**消费侧**（`/api/map` 的响应）
    /// 再钉一遍 —— 因为前端只看得见这两个字段，`miredit-core` 哪天改坏了格式段拼接，
    /// 只有这里能拦住。
    ///
    /// 四张真实图覆盖两个分支（2026-11-05 实测口径）：
    ///
    /// | 图 | 格式 | editable | 尾块 | edit_warning |
    /// |---|---|---|---|---|
    /// | `GodotClient/maps/0.map` | Old12 | `true` | 0 | `null` |
    /// | `GodotClient/maps/41.map` | Shanda14 | `true` | 0 | `null` |
    /// | `…/Map/alx.map` | Shanda2012 | `false` | 0 | 22 B 尾区那句 caveat |
    /// | `…/Map/EM001.map` | Old12 | `true` | 8,040 | 只有尾块那句 |
    #[test]
    fn editable与告警互为反相() {
        let Some(root) = 资产根() else {
            eprintln!("[跳过] 无资产根 MIREDIT_ASSET_ROOT（需要真实地图素材）");
            return;
        };
        // (相对资产根的路径, 期望 editable, 告警必含片段, 告警必不含片段)
        let 用例: [(&str, bool, &[&str], &[&str]); 4] = [
            ("GodotClient/maps/0.map", true, &[], &["本格式"]),
            ("GodotClient/maps/41.map", true, &[], &["本格式"]),
            (
                "client/热血传奇23周年客户端/Map/alx.map",
                false,
                &["22 字节", "rel 14-35"],
                &["尾块"],
            ),
            (
                "client/热血传奇23周年客户端/Map/EM001.map",
                true,
                &["8040"],
                &["本格式", "Old12"],
            ),
        ];
        let mut 测到 = 0usize;
        let mut 测到不可编辑 = 0usize;
        let mut 有不可编辑样本 = false;
        for (p, 期望可编辑, 必含, 必不含) in 用例 {
            let path_buf = root.join(p);
            let path = path_buf.as_path();
            if !path.is_file() {
                eprintln!("[跳过] 真实地图不存在：{}", path.display());
                continue;
            }
            if !期望可编辑 {
                有不可编辑样本 = true;
            }
            let session = EditSession::load(path).expect("打开真实地图");
            let trailer = std::fs::read(path)
                .ok()
                .and_then(|b| mir2_format::read_any_tolerant(&b).ok())
                .map(|m| m.trailer_len());
            let v = map_fields(&session, trailer);
            测到 += 1;
            assert_eq!(
                v["editable"], 期望可编辑,
                "{} 的 editable 与实测取值表不符",
                path.display()
            );
            let warn = v["edit_warning"].as_str();
            // ── 契约两条推论（与具体图无关，任何图都必须成立）──
            if v["editable"] == false {
                测到不可编辑 += 1;
                let w = warn.unwrap_or_else(|| {
                    panic!(
                        "{}：editable=false 时 edit_warning 必须非空（契约：false ⟺ caveat 非空）",
                        path.display()
                    )
                });
                assert!(
                    !w.trim().is_empty(),
                    "{}：editable=false 时 edit_warning 不许是空串",
                    path.display()
                );
            } else if v["tolerant_trailer_bytes"] == 0 {
                assert_eq!(
                    warn,
                    None,
                    "{}：editable=true 且无尾块 ⇒ edit_warning 必须为 null（格式段已取消）",
                    path.display()
                );
            }
            // ── 逐图实测片段 ──
            if let Some(w) = warn {
                for 片段 in 必含 {
                    assert!(w.contains(片段), "{}：告警缺「{片段}」，实际 = {w}", path.display());
                }
                for 片段 in 必不含 {
                    assert!(
                        !w.contains(片段),
                        "{}：告警不该含「{片段}」，实际 = {w}",
                        path.display()
                    );
                }
            } else {
                assert!(
                    必含.is_empty(),
                    "{}：期望告警含 {必含:?}，实际是 null",
                    path.display()
                );
            }
        }
        if 测到 == 0 {
            eprintln!("[跳过] 四张真实地图一张都不在，契约未被验证");
            return;
        }
        // 两个分支都得真的测到，否则这条测试会退化成「只测了 true 那半边」而假装通过。
        // 但样本文件本身缺失属于「环境缺资源」（与 `真图字段来自内核` 同口径），那时只报不红。
        if 有不可编辑样本 {
            assert!(
                测到不可编辑 > 0,
                "样本里有 editable=false 的图，却一张都没测到 ⇒ 契约的 false 分支没被验证"
            );
        } else {
            eprintln!("[跳过] 唯一的 editable=false 样本（alx.map）不在 ⇒ 契约 false 分支未验证");
        }
    }
}
