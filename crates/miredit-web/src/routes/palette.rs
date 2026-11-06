//! `GET /api/palette` —— 资源面板的**图库列表**（真实文件 + 真实帧数）。
//!
//! # 一、返回什么
//!
//! ```json
//! {"items":[{"lib":0,"name":"Tiles","frames":1234}],"total":12}
//! ```
//!
//! | 字段 | 来源 | 说明 |
//! |---|---|---|
//! | `lib` | `LibRef::index` | 族内库号（= 地图格里的库号字节） |
//! | `name` | 磁盘上的文件基名（`Tiles` / `Tiles3` / `Objects21` …） | 真名，不是拼的 |
//! | `frames` | `WilSource::frame_count` | 索引表长度；**库打不开 ⇒ `null`**（不猜 0） |
//! | `total` | 该族的库总数 | 前端用它算页数 |
//!
//! # 二、为什么一条 item 是「一个库」而不是「一张图」
//!
//! 一张图一条会变成几十万条（`Objects21` 一个库就 1.6 万帧），既传不完也没必要：
//! 前端选中库之后再按帧取图。而「一个库有多少帧」正是分页/滚动条需要的信息。
//! 帧数**不是猜的**：`mir2-assets` 的 `frame_count` 读的就是 `.wzx/.wix` 索引表长度。
//!
//! # 三、`kind` 取值
//!
//! `ground`（地表，`Tiles*`）/ `middle`（中层，`SmTiles*`）/ `object`（表层，`Objects*`）；
//! 别名 `front` / `surface` 都指向表层（与内核 `Family::Surface` 对齐，见 [`parse_family`]）。
//! 缺省 `ground`（与前端 `web/README.md` §3.7 一致）。

use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::Json;
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::error::{ApiError, ApiResult};
use crate::json::parse_family;
use crate::routes::Params;
use crate::state::Shared;

/// 每页缺省条数（前端 §3.7 的「每页固定 200 条」）。
const DEFAULT_PAGE_SIZE: u32 = 200;
/// 每页上限（防「size=1000000」这种请求把内存打满）。
const MAX_PAGE_SIZE: u32 = 1000;

/// `GET /api/palette?kind=&page=&size=`。
pub async fn palette(
    State(st): State<Shared>,
    q: Result<Query<HashMap<String, String>>, QueryRejection>,
) -> ApiResult<Json<Value>> {
    let p = Params::from_query(q)?;
    let family = parse_family(p.raw("kind").unwrap_or("ground"))?;
    let page = p.u32_or("page", 0)?;
    let size = p.u32_or("size", DEFAULT_PAGE_SIZE)?;
    if size == 0 || size > MAX_PAGE_SIZE {
        return Err(ApiError::bad_request(format!(
            "size 必须在 1..={MAX_PAGE_SIZE} 之间，收到：{size}"
        )));
    }

    let libs = st.libs.as_ref().ok_or_else(|| {
        ApiError::assets_unavailable(
            "图库未就绪：未扫描到任何 Tiles/SmTiles/Objects 库（设置 MIREDIT_CLIENT_ROOT）",
        )
    })?;

    // 该族的全部库（`LibPaths::iter` 已按「族顺序 + 族内索引升序」排好，结果稳定）
    let want = crate::json::assets_family(family);
    let all: Vec<(u16, String)> = libs
        .iter()
        .filter(|info| info.lib.family == want)
        .map(|info| (info.lib.index, info.stem().unwrap_or("?").to_string()))
        .collect();
    let total = all.len() as u64;

    let skip = (page as usize).saturating_mul(size as usize);
    let items: Vec<Value> = all
        .into_iter()
        .skip(skip)
        .take(size as usize)
        .map(|(index, name)| {
            let lib = crate::json::assets_lib(family, index);
            // 帧数取不到就是 null（库文件损坏 / 索引表读不了），**不写 0 冒充空库**
            let frames = st.source.as_ref().and_then(|s| s.frame_count(lib));
            json!({"lib": index, "name": name, "frames": frames})
        })
        .collect();

    Ok(Json(json!({"items": items, "total": total})))
}
