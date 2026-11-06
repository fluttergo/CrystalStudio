//! `GET /api/tiles` —— **出图端点**（返回 `image/png`，不是 JSON）。
//!
//! # 一、内部就一句话：调 `miredit_render` 的两个入口
//!
//! ```text
//! 查询参数 → Viewport + LayerMask + CollisionOverlay + RenderScale + RenderOptions
//!          scale == 1 ⇒ try_render_region_with_options(map, src, &vp, &mask, overlay, opts)
//!          scale != 1 ⇒ try_render_region_scaled_with(map, src, &vp, &mask, overlay, 档, opts)
//!          → Rendered{rgba,w,h,scale}
//!          → miredit_render::png_io::write_rgba_png（**复用，不自己写 PNG**）→ PNG 字节
//! ```
//!
//! `opts` 缺省 = [`RenderOptions::closed()`]（闭门）：`One` 档下它与旧入口
//! `try_render_region_with` 是**同一次 `compose::render` 调用** ⇒ 缺省产物与改动前逐字节相同（§七）。
//!
//! 出图与 `render_maps` 走的是**同一条代码路径**（同一个 `compose::render`、同一个 PNG 编码器），
//! 所以同一区域逐字节相同（验收门 §五.4）。
//!
//! # 二、参数（两种写法都收，见 [`parse_layers`] / [`parse_collision`]）
//!
//! | 参数 | 取值 | 缺省 |
//! |---|---|---|
//! | `x` / `y` | 世界格 ≥ 0 | `0` |
//! | `w` / `h` | `1..=64` 格（前端固定 16） | `16` |
//! | `scale` | `0.25` / `0.5` / `1` / `2`（也收 `quarter` / `half` / `one` / `two`，**大小写不敏感**） | `1` |
//! | `layers` | `ground,middle,front` 列表 **或** 位掩码 `0..=7`（1=地表 2=中层 4=物体） | 三层全开 |
//! | `collision` | `low,high` 列表 **或** 位掩码 `0..=3`（1=低位 2=高位） | `0`（不叠加） |
//! | `doors` | `0`（闭门）/ `1`（门全开） | `0`（= 改动前的产物，逐字节相同） |
//! | `lighting` | `day` / `dawn_dusk` / `night`（大小写不敏感） | `day` |
//!
//! # 三、多倍率：为什么服务端出图，而不是「1× 出图 + 浏览器缩」
//!
//! 前端原来一律请求 `scale=1` 再用 canvas `drawImage` 缩小。25% 档时一块 16×16 格的瓦片是
//! `768×512` 的 1× 图（整屏 64×48 更是 `3072×1536` 的 **4.5 MB** PNG），浏览器再缩到 `192×128` 显示
//! —— 流量与内存全花在**马上要被丢掉的像素**上。服务端按 `scale=0.25` 出的是 `192×128`（约 12 KB），
//! 体积差一个数量级以上，且 Box 面积加权的画质**优于**浏览器 `drawImage` 的缩放。
//!
//! 三档口径（`miredit-render` 已实现，本 crate **只做参数解析与错误映射**，不改它的算法）：
//!
//! - **`One` 档必须与旧路径逐字节相同**：`try_render_region_scaled(.., One)` 内部直接
//!   `return compose::render(..)`，与 `try_render_region_with` 是同一次调用。本文件仍然**显式**
//!   分派到旧入口，让「1× 走老路」成为结构性保证（`mod.rs` 的逐字节比对测试钉住这一点）；
//! - 缩小（`0.25` / `0.5`）走面积加权 `Box`，放大（`2`）走 `Nearest`（像素画不许糊）；
//! - 输出尺寸**精确等于** `round(w*48*s) × round(h*32*s)`。
//!
//! # 四、超限：413，不静默降级
//!
//! 渲染库有两条字节护栏，超限时**在分配缓冲 / 取帧之前**返回（不硬渲、不悄悄退回 1×）：
//!
//! | 变体 | 管的是 | 上限 |
//! |---|---|---|
//! | [`RenderError::TooLarge`] | 目标图（要交给网页的那张） | 64 MiB |
//! | [`RenderError::IntermediateTooLarge`] | 1× 中间图（内部先合成的那张） | 128 MiB |
//!
//! 两者本文件一律映射成 **413 + 中文原因**（含 `want` / `limit` 数字与「缩小视口格数或降低倍率」
//! 的建议），**绝不允许变成 500**。典型触发：`64×48 格 @2×` 的目标图是 `6144×3072×4 = 72 MiB`。
//!
//! # 五、越界与缺帧
//!
//! - 窗口超出地图 ⇒ 越界部分**透明**（`compose` 里逐格 `continue`），不报错；
//! - 帧取不到 ⇒ 不画、计入 `RenderStats`，**绝不画洋红占位块**（渲染库的口径）；
//! - 图库不可用 ⇒ 503（没有图源就只能出一张全透明图，那是假数据，宁可报错）。
//!
//! # 六、响应头：`X-Render-Scale` / `X-Render-Doors` / `X-Render-Lighting`
//!
//! - **`X-Render-Scale`** 带**实际出图档位**（取自 `Rendered.scale`，不是回显请求参数），便于排查
//!   「前端要 0.25、实际拿到的却是 1×」这类串图问题。
//! - **`X-Render-Doors`** 带本次出图用的**门口径**（`0` / `1`），便于排查「勾了开启门却没变」。
//!   渲染库不回传「实际用了几次门偏移」，所以这一条**如实**是请求口径的规范化回显 ——
//!   与 `X-Render-Scale` 的「取自产物」不同，别把两者混为一谈。它不会撒谎：`doors` 只有
//!   两个合法值，非法值在**进渲染之前**就已经 400，所以回显值恒等于生效值。
//! - **`X-Render-Lighting`** 带本次出图用的**光照档位**（`day` / `dawn_dusk` / `night`），取自
//!   **真正传给渲染库**的那份 [`RenderOptions::lighting`]，不是回显请求原文。与
//!   `X-Render-Doors` 一样是「规范化回显」：非法值在进渲染之前已 400，故回显值恒等于生效值；
//!   与 `X-Render-Scale` 的「取自产物」不同，别把三者混为一谈。
//!
//! 三个头都**必须**列进 `server::EXPOSED_HEADERS`，否则跨源部署时页面 JS 读到的恒为 `null`，
//! 诊断永久失效且从界面上看不出失效 —— 见 `server.rs` 模块文档第二节。
//!
//! # 七、`doors`（开启门）：只改前景层帧号，缺省与改动前**逐字节相同**
//!
//! 语义与出处全在 `miredit_render::RenderOptions` 的类型文档里（本 crate **只传参，不改算法**）：
//!
//! ```text
//! doors_open == false（缺省）：帧号 = (fr_img & 0x7FFF) - 1              ← 与引入该选项前逐字节相同
//! doors_open == true        ：若 (door_index & 0x7F) != 0
//!                             帧号 += door_offset & 0x7F                ← 门全开帧
//! ```
//!
//! 出处：官方客户端 `client/reference_full/Classes/scenes/GameScene.cpp:3328-3388`。
//!
//! 参数口径：**只认 `0` / `1`，缺省 `0`**。其它任何值（含 `?doors=` 空值、`2`、`true`、`01`）
//! 一律 **400 + 中文原因**，**不静默当 0** —— 静默降级会让「勾了没变」这类问题永远查不出来，
//! 与 `scale` 的口径一致（那边也拒绝 `0.3`、不吸附到 `0.25`）。
//!
//! 注意 `?doors=`（给了但为空）与**没给**是两件事：前者走校验 ⇒ 400，后者才是缺省 `0`。
//!
//! # 八、`lighting`（地图光照）：三档互斥，缺省 `day` **逐字节不变**
//!
//! 参数只认 `day | dawn_dusk | night`（大小写不敏感），缺省 `day`；其它值（含空值、
//! `dawn-dusk`、`DAWNDUSK` 之外的拼写）一律 **400 + 中文原因**，不静默当成白天。
//! 本 crate **只做协议值 → `MapLighting` 的显式映射**，语义与 205 / 0 / 255 三档数值全在
//! `miredit_render::lighting` 里；接口值 `dawn_dusk` 与渲染库日志短名 `dawn-dusk` 是两套
//! 契约，绝不复用 `MapLighting::name()` 去拼 URL 或响应头。
//!
//! 门口径与光照档位必须一次组合进 [`RenderOptions::with_doors_and_lighting`]：
//! 先按 `doors` 解析出布尔值，再按 `lighting` 解析出枚举，最后**只调一次构造器**。
//! 分两步 `with_lighting(...)` + `with_doors_open(...)` 会互相覆盖另一个开关。

use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::http::{header, HeaderName, HeaderValue};
use axum::response::{IntoResponse, Response};
use miredit_render::{
    CollisionOverlay, LayerMask, MapLighting, RenderError, RenderOptions, RenderScale, Viewport,
};
use std::collections::HashMap;

use crate::error::{ApiError, ApiResult};
use crate::routes::{blocking, encode_png, Params};
use crate::state::Shared;

/// 单次请求的最大格数（与前端 `w/h ≤ 64` 的约定一致）。
const MAX_CELLS: u32 = 64;

/// 合法 `scale` 取值的中文枚举 —— 错误消息与模块文档共用这一处措辞。
const SCALE_HINT: &str = "0.25 / 0.5 / 1 / 2（也可写 quarter / half / one / two，大小写不敏感）";

/// 合法 `doors` 取值的中文枚举 —— 错误消息与模块文档共用这一处措辞。
const DOORS_HINT: &str = "0（闭门，默认）或 1（门全开）";

/// 合法 `lighting` 取值的中文枚举 —— 错误消息与模块文档共用这一处措辞。
const LIGHTING_HINT: &str = "day（白天，默认）/ dawn_dusk（黎明·黄昏）/ night（夜晚）";

/// `GET /api/tiles`。
pub async fn tiles(
    State(st): State<Shared>,
    q: Result<Query<HashMap<String, String>>, QueryRejection>,
) -> ApiResult<Response> {
    let p = Params::from_query(q)?;
    let vp = parse_viewport(&p)?;
    let scale = parse_scale(&p)?;
    let mask = parse_layers(p.raw("layers"))?;
    let overlay = parse_collision(p.raw("collision"))?;
    // 参数错先于环境错（没图库/没开图时也该先拿到 400，见 §七 / §八）；
    // 两个开关必须一次组合，避免后调用的构造器覆盖前一个（见模块文档 §八）。
    let doors_open = parse_doors_open(p.raw_verbatim("doors"))?;
    let lighting = parse_lighting(p.raw_verbatim("lighting"))?;
    let options = RenderOptions::with_doors_and_lighting(doors_open, lighting);

    // 图库优先判：没有图源就没法出图（比"先渲一张全透明"诚实）
    let src = st.need_source()?;

    let (png, scale_label, doors_value, lighting_value) = {
        let guard = st.read_session();
        let session = guard.as_ref().ok_or_else(ApiError::no_map)?;
        let map = session.map();
        // 渲染是同步 CPU 活（64×48 格 @1× ≈ 3000×1500 px），用 block_in_place 让出 worker，
        // 同时**不把读锁带过任何 await**（这里整个块都是同步代码）
        blocking(
            || -> ApiResult<(Vec<u8>, &'static str, &'static str, &'static str)> {
                let rendered = render(map, src, &vp, &mask, overlay, scale, options)?;
                // 档位取自**实际产物**（`Rendered.scale`），不是回显请求参数；
                // 门态 / 光照取自**真正传给渲染库**的那份 options，不是回显请求原文。
                let label = rendered.scale.label();
                Ok((
                    encode_png(&rendered)?,
                    label,
                    doors_label(options),
                    lighting_label(options),
                ))
            },
        )?
    };

    Ok((
        [
            (header::CONTENT_TYPE, HeaderValue::from_static("image/png")),
            (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
            (
                HeaderName::from_static("x-render-scale"),
                HeaderValue::from_static(scale_label),
            ),
            (
                HeaderName::from_static("x-render-doors"),
                HeaderValue::from_static(doors_value),
            ),
            (
                HeaderName::from_static("x-render-lighting"),
                HeaderValue::from_static(lighting_value),
            ),
        ],
        png,
    )
        .into_response())
}

/// 按档位分派出图 —— **`One` 显式走 1× 入口**（逐字节相同的结构性保证，见模块文档 §三）。
///
/// 两个入口都是 `_with` 版：`options == RenderOptions::closed()` 时它们与旧入口
/// （`try_render_region_with` / `try_render_region_scaled`）**是同一次调用**
/// —— 后者就是按默认值转调前者。所以「`doors` 缺省 ⇒ 产物逐字节不变」不是靠约定，
/// 而是结构上只有一条路。
fn render(
    map: &mir2_map::Map,
    src: &dyn mir2_assets::SpriteSource,
    vp: &Viewport,
    mask: &LayerMask,
    overlay: CollisionOverlay,
    scale: RenderScale,
    options: RenderOptions,
) -> ApiResult<miredit_render::Rendered> {
    let r = match scale {
        // 1×：与改动前的 `/api/tiles` 同一条 `compose::render` 调用（只多传了 options）
        RenderScale::One => {
            miredit_render::try_render_region_with_options(map, src, vp, mask, overlay, options)
        }
        // 多倍率：0.25 / 0.5 / 2（门偏移在 1× 合成阶段生效，重采样不改变画了哪一帧）
        other => miredit_render::try_render_region_scaled_with(
            map, src, vp, mask, overlay, other, options,
        ),
    };
    r.map_err(|e| render_error_to_api(e, vp, scale))
}

/// 查询参数 → [`Viewport`]（`scale` 不在这里解析，见 [`parse_scale`]）。
fn parse_viewport(p: &Params) -> ApiResult<Viewport> {
    let x = p.i64_or("x", 0)?;
    let y = p.i64_or("y", 0)?;
    let w = p.u32_or("w", 16)?;
    let h = p.u32_or("h", 16)?;

    if !(0..=i64::from(i32::MAX)).contains(&x) || !(0..=i64::from(i32::MAX)).contains(&y) {
        return Err(ApiError::bad_request(format!(
            "x / y 必须在 0..=2147483647 之间，收到 x={x} y={y}"
        )));
    }
    if !(1..=MAX_CELLS).contains(&w) || !(1..=MAX_CELLS).contains(&h) {
        return Err(ApiError::bad_request(format!(
            "w / h 必须在 1..={MAX_CELLS} 之间，收到 w={w} h={h}"
        )));
    }
    Ok(Viewport::new(x as i32, y as i32, w, h))
}

/// `scale` → [`RenderScale`]；缺省 `1`（= 改动前的行为，逐字节不变）。
///
/// 只认四个档位（`miredit_render::RenderScale::from_param` 的口径），**不做模糊匹配**：
/// `0.3` 不会被吸到 `0.25`、`2x` 不会被吸到 `2` —— 静默吸附出来的尺寸与前端算的不一致，
/// 那就是假数据。非法值一律 **400 + 中文原因（列出合法取值）**。
///
/// 大小写不敏感（本 crate 先 `to_ascii_lowercase` 再交给 `from_param`，
/// 后者本身是大小写敏感的 `quarter|half|one|two` 口径）。
fn parse_scale(p: &Params) -> ApiResult<RenderScale> {
    let Some(raw) = p.raw("scale") else {
        return Ok(RenderScale::One);
    };
    RenderScale::from_param(&raw.to_ascii_lowercase()).ok_or_else(|| {
        ApiError::bad_request(format!(
            "scale 只支持 {SCALE_HINT}，收到：{raw}（不做四舍五入，也不静默降级成 1×）"
        ))
    })
}

/// `layers`：`ground,middle,front` 列表 **或** 位掩码 `0..=7`（前端用位掩码）。
///
/// 位序与前端 `web/README.md` §3.4 一致：`1`=地表 `2`=中层 `4`=物体。
fn parse_layers(raw: Option<&str>) -> ApiResult<LayerMask> {
    let Some(s) = raw else {
        return Ok(LayerMask::all());
    };
    if let Ok(bits) = s.parse::<u32>() {
        if bits > 7 {
            return Err(ApiError::bad_request(format!(
                "layers 位掩码必须在 0..=7 之间，收到：{bits}"
            )));
        }
        return Ok(LayerMask {
            ground: bits & 1 != 0,
            middle: bits & 2 != 0,
            front: bits & 4 != 0,
        });
    }
    let mut mask = LayerMask::none();
    let mut seen = false;
    for part in s.split(',') {
        let name = part.trim().to_ascii_lowercase();
        if name.is_empty() {
            continue;
        }
        seen = true;
        match name.as_str() {
            "ground" | "地表" => mask.ground = true,
            "middle" | "中层" => mask.middle = true,
            "front" | "object" | "表层" | "物体" => mask.front = true,
            "all" => mask = LayerMask::all(),
            "none" => mask = LayerMask::none(),
            other => {
                return Err(ApiError::bad_request(format!(
                    "未知图层名：{other:?}（可用 ground / middle / front，或位掩码 0..=7）"
                )))
            }
        }
    }
    if !seen {
        return Ok(LayerMask::all());
    }
    Ok(mask)
}

/// `collision`：`low,high` 列表 **或** 位掩码 `0..=3`（`1`=低位 `2`=高位，前端口径）。
///
/// 缺省 = 两个通道都关（与 `CollisionOverlay::default()` 同义，输出与旧路径逐字节相同）。
fn parse_collision(raw: Option<&str>) -> ApiResult<CollisionOverlay> {
    let Some(s) = raw else {
        return Ok(CollisionOverlay::none());
    };
    if let Ok(bits) = s.parse::<u32>() {
        if bits > 3 {
            return Err(ApiError::bad_request(format!(
                "collision 位掩码必须在 0..=3 之间，收到：{bits}"
            )));
        }
        return Ok(CollisionOverlay {
            low: bits & 1 != 0,
            high: bits & 2 != 0,
            alpha: CollisionOverlay::DEFAULT_ALPHA,
        });
    }
    let mut ov = CollisionOverlay::none();
    let mut seen = false;
    for part in s.split(',') {
        let name = part.trim().to_ascii_lowercase();
        if name.is_empty() {
            continue;
        }
        seen = true;
        match name.as_str() {
            "low" | "低位" | "低" => ov.low = true,
            "high" | "高位" | "高" => ov.high = true,
            "all" => ov = CollisionOverlay::all(),
            "none" => ov = CollisionOverlay::none(),
            other => {
                return Err(ApiError::bad_request(format!(
                    "未知阻挡通道：{other:?}（可用 low / high，或位掩码 0..=3）"
                )))
            }
        }
    }
    if !seen {
        return Ok(CollisionOverlay::none());
    }
    Ok(ov)
}

/// `doors` → 布尔开关；**缺省 `false`（闭门）**，只认 `0` / `1`。
///
/// 为什么用 [`Params::raw_verbatim`] 而不是 `raw`：`?doors=`（给了但为空）与「没给」是两件事。
/// 前者是**非法值** ⇒ 400；后者才是缺省 `false`。若把空值当缺省，就等于「静默当成 0」，
/// 而这正是本次要根除的形态（勾了没变、参数写错都查不出来）。
///
/// 也**不做模糊匹配**：`01`、`true`、`yes`、`0.0` 一律 400 —— 与 `scale` 拒绝 `0.3` 同口径。
///
/// ⚠ 本函数只负责**解析布尔值**，不直接返回 [`RenderOptions`]：光照是另一个开关，
/// 两者只能在 handler 里用 [`RenderOptions::with_doors_and_lighting`] 一次组合。
fn parse_doors_open(raw: Option<&str>) -> ApiResult<bool> {
    let Some(s) = raw else {
        return Ok(false);
    };
    match s {
        "0" => Ok(false),
        "1" => Ok(true),
        other => Err(ApiError::bad_request(format!(
            "doors 只支持 {DOORS_HINT}，收到：{}（不静默当成 0）",
            if other.is_empty() { "空值" } else { other }
        ))),
    }
}

/// `lighting` → [`MapLighting`]；**缺省白天**，只认 `day | dawn_dusk | night`（大小写不敏感）。
///
/// 这是**协议值到渲染库枚举的显式映射**：`dawn_dusk` 是本接口的 URL 写法，渲染库
/// `MapLighting::name()` 给日志用的短名是 `dawn-dusk`，两者不复用、不互相推断。
/// `?lighting=`（给了但为空）与其它非法值一样 400，不静默当成 `day`。
fn parse_lighting(raw: Option<&str>) -> ApiResult<MapLighting> {
    let Some(s) = raw else {
        return Ok(MapLighting::Day);
    };
    match s.to_ascii_lowercase().as_str() {
        "day" => Ok(MapLighting::Day),
        "dawn_dusk" => Ok(MapLighting::DawnDusk),
        "night" => Ok(MapLighting::Night),
        _ => Err(ApiError::bad_request(format!(
            "lighting 只支持 {LIGHTING_HINT}，收到：{}（不静默当成 day；大小写不敏感）",
            if s.is_empty() { "空值" } else { s }
        ))),
    }
}

/// `doors` 的响应头值：与**真正传给渲染库**的那份 [`RenderOptions`] 同源，不会各写一套。
fn doors_label(options: RenderOptions) -> &'static str {
    if options.doors_open {
        "1"
    } else {
        "0"
    }
}

/// `lighting` 的响应头值：与**真正传给渲染库**的那份 [`RenderOptions::lighting`] 同源。
///
/// 这里不复用 [`MapLighting::name()`]：库短名是 `dawn-dusk`（连字符，给日志/文件名用），
/// 接口契约是 `dawn_dusk`（下划线，给 URL / 响应头 / 前端缓存键用）。两套写法各有用途，
/// 混用会让「日志能看懂、URL 解析不了」。
fn lighting_label(options: RenderOptions) -> &'static str {
    match options.lighting {
        MapLighting::DawnDusk => "dawn_dusk",
        MapLighting::Day => "day",
        MapLighting::Night => "night",
    }
}

/// 渲染失败 → HTTP 错误。
///
/// 两条**字节护栏**（目标图 64 MiB / 1× 中间图 128 MiB）映射成 **413**：
/// 请求本身合法，只是它要的产物超出了服务端愿意分配的上限 ⇒ 换档或缩格数就能成功，
/// 这既不是参数写错（400），更不是服务端故障（500）。其余变体仍是 400。
///
/// 文案里补上「请求的视口与档位」，因为渲染库的 `Display` 只知道自己算出来的字节数，
/// 不知道调用方要的是哪一块。
fn render_error_to_api(e: RenderError, vp: &Viewport, scale: RenderScale) -> ApiError {
    let ctx = format!("{}×{} 格 @ scale={}", vp.cells_w, vp.cells_h, scale.label());
    match e {
        RenderError::TooLarge { .. } | RenderError::IntermediateTooLarge { .. } => {
            ApiError::too_large(format!("出图超限（{ctx}）：{e}"))
        }
        other => ApiError::bad_request(format!("渲染参数非法（{ctx}）：{other}")),
    }
}

/// 原有测试段单独一个文件（本文件已接近 600 行上限；纯搬迁，无逻辑改动）。
#[cfg(test)]
mod tests;

/// 本轮「地图光照」测试单独一个文件：不挤爆原测试文件，也不改其它 crate。
#[cfg(test)]
mod lighting_tests;
