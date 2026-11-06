//! `GET /api/maps` —— 地图列表（**只读元数据**，绝不解析地图内容）。
//!
//! # 一、为什么要这个端点
//!
//! 之前只能靠「打开地图…」对话框手打路径。`23周年客户端/Map` 有 **1,097 张 / 731 MB**
//! （2026-11-05 实测），手打既不可行也不安全。本端点把「有哪些图」一次列出来。
//!
//! # 二、硬约束：**只 stat，不 open**
//!
//! 列表阶段只允许三类系统调用：`read_dir`（列目录）、`DirEntry::file_type`（不跟符号链接）、
//! `DirEntry::metadata`（长度 + mtime）。**禁止** `std::fs::read` / `File::open` / 任何地图
//! 头解析 —— 1,097 张图逐个解析会读 731 MB，足以把服务卡死几十秒。
//!
//! 这条约束是**可实测**的：扫 1,097 张的耗时应当接近「列目录 + 1,097 次 stat」的量级
//! （本机实测 20~40 ms），而解析单张 5.6 MB 的 `0.map` 就要 ~30 ms —— 如果哪天有人
//! 在扫描里塞了解析，总耗时会跳到分钟级，一眼可辨。
//!
//! # 三、递归深度：**上限 3 层子目录**
//!
//! 两个真实地图目录（`GodotClient/maps`、`23周年客户端/Map`）**都是平的**（实测 0 个子目录），
//! 但客户端换版本后可能出现 `Map/高清/xxx.map` 这类分组目录，所以扫描做成递归。
//! 递归**必须有上限**：不限深度的 `read_dir` 递归遇到符号链接环或几十万小文件的目录会失控。
//! 取值 [`MAX_DEPTH`] = 3（`map_root` 自身算第 0 层，即最多扫到 `a/b/c/x.map`）；
//! 更深的目录**直接跳过**（不报错、不假装扫过），这是有意的截断，写在文档里而不是静默。
//!
//! 符号链接（含 Windows 的目录联接）**一律不跟**：既防环，也防「白名单根之外的东西」
//! 顺着链接混进列表 —— 列表里的 `path` 必须能被 `resolve_map_path` 接受（见下节）。
//!
//! # 四、`path` 的口径：**与 `/api/map/open` 完全一致**
//!
//! 返回的 `path` 是**相对 `MIREDIT_MAP_ROOT` 的路径**（`/` 分隔，例如 `n0.map`、`高清/a.map`）——
//! 这正是 `POST /api/map/open` 接受的形式（`state.rs::resolve_map_path`：相对路径按
//! `map_root` 拼接），也是 `web/js/api.js::openMap` 注释里写的「一律传相对 MIREDIT_MAP_ROOT 的路径」。
//! 所以前端拿到 `path` **原样**发回去就能打开，不需要任何拼接或反转义。
//!
//! 为什么不返回绝对路径：绝对路径在两台机器 / Docker 挂载点不同时必然失效，而且更长的字符串
//! 会让 1,097 条的响应白白大 10 倍。测试 `列表项路径能直接喂给open` 从**消费侧**钉住这条。
//!
//! # 五、响应形状与分页
//!
//! ```json
//! {"total": 61,
//!  "items": [{"name":"0.map","path":"0.map","bytes":490028,"mtime":1761812345}]}
//! ```
//!
//! | 字段 | 含义 |
//! |---|---|
//! | `total` | **总数**（不是本页条数）—— 前端靠它显示「共 1097 张」并决定要不要翻页 |
//! | `items` | 本页条目（`offset` 之后最多 `limit` 条） |
//! | `name` | 文件名（含 `.map` 后缀，不含目录） |
//! | `path` | 相对 `MIREDIT_MAP_ROOT`，`/` 分隔，可直接喂 `/api/map/open` |
//! | `bytes` | 文件字节数 |
//! | `mtime` | 修改时间，**Unix 秒**；取不到 ⇒ `null`（**不写 0 冒充 1970**） |
//!
//! 分页：`?offset=`（缺省 0）与 `?limit=`（缺省 [`DEFAULT_LIMIT`] = 100，上限 [`MAX_LIMIT`] = 500）。
//! **超过上限是 400，不是静默截断** —— 静默截断会让前端以为「要了 1000 条却只有 500 条」是数据缺失。
//! `limit=0` 合法（返回空 `items` + 正确 `total`，便于「只要总数」）。
//!
//! 排序：**文件名自然序**（连续数字段按数值比较 ⇒ `n2.map` 在 `n10.map` 之前），
//! 同名再按 `path` 字节序兜底（保证同一份目录每次返回的顺序完全一致，便于前端本地过滤与断言）。
//!
//! # 六、失败口径
//!
//! 根目录不存在 / 不是目录 / 某个子目录读不了 / 条目元数据读不了 ⇒ **503 + 中文原因**，
//! **绝不返回 200 + 空列表**（那会让用户以为「这个根下真的没有图」，是最坏的假数据）。

use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::Path;
use std::time::UNIX_EPOCH;

use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::Json;
use serde_json::{json, Value};

use crate::error::{ApiError, ApiResult};
use crate::routes::Params;
use crate::state::{display_path, Shared};

/// 每页缺省条数（`?limit=` 不给时）。
pub const DEFAULT_LIMIT: u32 = 100;

/// 每页条数上限；超过 ⇒ **400**（不静默截断）。
pub const MAX_LIMIT: u32 = 500;

/// 递归深度上限：`map_root` 自身算第 0 层，最多再向下 3 层子目录。
pub const MAX_DEPTH: usize = 3;

/// `.map` 条目总数上限；超过 ⇒ 503（防止病态目录把内存吃干）。
pub const MAX_ENTRIES: usize = 50_000;

/// 一张地图的**纯元数据**（列表项）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapEntry {
    /// 文件名（含 `.map` 后缀，不含目录）。
    pub name: String,
    /// 相对 `MIREDIT_MAP_ROOT` 的路径，`/` 分隔 —— **可直接喂给 `POST /api/map/open`**。
    pub path: String,
    /// 文件字节数。
    pub bytes: u64,
    /// 修改时间（Unix 秒）。取不到 ⇒ `None`（不写 0 冒充 1970）。
    pub mtime: Option<u64>,
}

impl MapEntry {
    /// 装成响应里的一项（字段名与模块文档第五节一致）。
    pub fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "path": self.path,
            "bytes": self.bytes,
            "mtime": self.mtime,
        })
    }
}

/// `GET /api/maps`：列出 `MIREDIT_MAP_ROOT` 下的 `.map`（只读元数据 + 分页）。
///
/// 错误：参数非法 ⇒ 400；根目录不可用 / 扫描失败 ⇒ 503（中文原因）。
pub async fn list_maps(
    State(st): State<Shared>,
    q: Result<Query<HashMap<String, String>>, QueryRejection>,
) -> ApiResult<Json<Value>> {
    let params = Params::from_query(q)?;
    let (offset, limit) = page_params(&params)?;

    // 扫目录是同步 IO：扔进阻塞线程池，别占 tokio worker（1097 个条目也一样）
    let root = st.map_root.clone();
    let scanned = tokio::task::spawn_blocking(move || scan_maps(&root))
        .await
        .map_err(|e| ApiError::internal(format!("地图列表扫描任务异常结束：{e}")))?;
    let all = scanned.map_err(scan_error)?;
    Ok(Json(page_json(&all, offset, limit)))
}

/// 解析分页参数：`(offset, limit)`；`limit` 超上限 ⇒ 400（**不静默截断**）。
pub fn page_params(params: &Params) -> ApiResult<(usize, usize)> {
    let offset = params.u32_or("offset", 0)?;
    let limit = params.u32_or("limit", DEFAULT_LIMIT)?;
    if limit > MAX_LIMIT {
        return Err(ApiError::bad_request(format!(
            "limit 上限是 {MAX_LIMIT}，收到：{limit}（本端点不静默截断，请分页取）"
        )));
    }
    Ok((offset as usize, limit as usize))
}

/// 扫描失败 ⇒ **503**（不是 200 空列表）。
///
/// 码沿用 `assets_unavailable`：语义是「这份资源现在用不了」，与图库 / 印章目录同档；
/// 新造码会让前端落到「未知错误」分支（`web/README.md` §3.10）。
pub fn scan_error(reason: String) -> ApiError {
    ApiError::assets_unavailable(format!(
        "地图列表不可用：{reason}（检查 MIREDIT_MAP_ROOT 指向的目录是否存在、是否可读）"
    ))
}

/// 一页响应：`total` **恒为总数**，`items` 只放 `offset..offset+limit` 这一段。
pub fn page_json(all: &[MapEntry], offset: usize, limit: usize) -> Value {
    let items: Vec<Value> = all.iter().skip(offset).take(limit).map(MapEntry::to_json).collect();
    json!({
        // 总数，不是本页条数（前端靠它显示「共 N 张」）
        "total": all.len(),
        "items": items,
    })
}

/// 扫 `MIREDIT_MAP_ROOT` 下所有 `.map`（**只读元数据**，按文件名自然序）。
///
/// 根不存在 / 不是目录 / 读不了 ⇒ `Err(中文原因)`。
pub fn scan_maps(root: &Path) -> Result<Vec<MapEntry>, String> {
    scan(root, MAX_DEPTH, MAX_ENTRIES)
}

/// [`scan_maps`] 的参数化形态（深度与条目上限可注入，便于单测钉住两条护栏）。
fn scan(root: &Path, max_depth: usize, max_entries: usize) -> Result<Vec<MapEntry>, String> {
    let meta = std::fs::metadata(root).map_err(|e| format!("地图根读不到：{}（{e}）", display_path(root)))?;
    if !meta.is_dir() {
        return Err(format!("地图根不是目录：{}", display_path(root)));
    }
    let mut out = Vec::new();
    walk(root, root, 0, max_depth, max_entries, &mut out)?;
    out.sort_by(|a, b| {
        natural_cmp(&a.name, &b.name).then_with(|| a.path.cmp(&b.path))
    });
    Ok(out)
}

/// 深度优先走目录（`depth` = 相对 `map_root` 的子目录层数，根 = 0）。
fn walk(
    root: &Path,
    dir: &Path,
    depth: usize,
    max_depth: usize,
    max_entries: usize,
    out: &mut Vec<MapEntry>,
) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("目录读不了：{}（{e}）", display_path(dir)))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("目录项读不了：{}（{e}）", display_path(dir)))?;
        let ft = entry
            .file_type()
            .map_err(|e| format!("目录项类型读不了：{}（{e}）", display_path(&entry.path())))?;
        // 符号链接 / 目录联接一律不跟：防环，也防白名单根之外的东西混进列表
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            if depth < max_depth {
                walk(root, &entry.path(), depth + 1, max_depth, max_entries, out)?;
            }
            continue;
        }
        if !ft.is_file() || !is_map_name(&entry.file_name()) {
            continue;
        }
        if out.len() >= max_entries {
            return Err(format!(
                "地图根下 `.map` 超过 {max_entries} 张，拒绝全量扫描：{}",
                display_path(root)
            ));
        }
        // 只取元数据（长度 / mtime）——**这里不许出现任何 read/open**
        let meta = entry
            .metadata()
            .map_err(|e| format!("文件元数据读不了：{}（{e}）", display_path(&entry.path())))?;
        let rel = entry
            .path()
            .strip_prefix(root)
            .map(Path::to_path_buf)
            .unwrap_or_else(|_| entry.path());
        out.push(MapEntry {
            name: entry.file_name().to_string_lossy().into_owned(),
            path: rel.to_string_lossy().replace('\\', "/"),
            bytes: meta.len(),
            mtime: meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs()),
        });
    }
    Ok(())
}

/// 扩展名是否 `.map`（大小写不敏感：Windows 上 `A.MAP` 也是地图）。
fn is_map_name(name: &std::ffi::OsStr) -> bool {
    Path::new(name)
        .extension()
        .map(|e| e.eq_ignore_ascii_case("map"))
        .unwrap_or(false)
}

/// 文件名自然序：连续数字段按**数值**比较（`n2` 在 `n10` 之前），其余按 ASCII 小写比较。
///
/// 末尾用原始字节序兜底，保证「大小写不同 / 前导零不同」的名字也有确定的先后
/// （`sort_by` 需要全序，否则同一份目录每次返回的顺序会随 `read_dir` 抖动）。
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (ab, bb) = (a.as_bytes(), b.as_bytes());
    let (mut i, mut j) = (0usize, 0usize);
    while i < ab.len() && j < bb.len() {
        if ab[i].is_ascii_digit() && bb[j].is_ascii_digit() {
            let si = i;
            while i < ab.len() && ab[i].is_ascii_digit() {
                i += 1;
            }
            let sj = j;
            while j < bb.len() && bb[j].is_ascii_digit() {
                j += 1;
            }
            // 去前导零后「位数多的大」⇒ 再比字典序；这样 2 < 10 而不是 "10" < "2"
            let da = trim_leading_zeros(&ab[si..i]);
            let db = trim_leading_zeros(&bb[sj..j]);
            let ord = da.len().cmp(&db.len()).then_with(|| da.cmp(db));
            if ord != Ordering::Equal {
                return ord;
            }
        } else {
            let (la, lb) = (ab[i].to_ascii_lowercase(), bb[j].to_ascii_lowercase());
            if la != lb {
                return la.cmp(&lb);
            }
            i += 1;
            j += 1;
        }
    }
    // 前缀短的在先（`n1.map` 在 `n10.map` 前）
    let ord = (ab.len() - i).cmp(&(bb.len() - j));
    if ord != Ordering::Equal {
        return ord;
    }
    ab.cmp(bb)
}

/// 去掉数字段的前导零（`"007"` ⇒ `"7"`、`"000"` ⇒ `"0"`）。
fn trim_leading_zeros(s: &[u8]) -> &[u8] {
    let mut k = 0;
    while k + 1 < s.len() && s[k] == b'0' {
        k += 1;
    }
    &s[k..]
}

#[cfg(test)]
mod tests;
