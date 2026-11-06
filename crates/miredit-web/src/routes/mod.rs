//! 各端点的 handler（一个文件一个域，见任务书 §四的拆分要求）。
//!
//! 这里只放**三个共享小工具**：请求体解析、查询参数包装、PNG 编码。
//! 业务逻辑一律在子模块里，别往这个文件堆。

pub mod edit;
pub mod events;
pub mod map;
pub mod maps;
pub mod palette;
pub mod save;
pub mod stamps;
pub mod tiles;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::body::Bytes;
use axum::extract::rejection::QueryRejection;
use axum::extract::Query;
use miredit_render::Rendered;
use serde_json::Value;

use crate::error::{ApiError, ApiResult};

/// 把请求体解析成 JSON（空体 ⇒ `Null`，由各 handler 自己判「缺字段」）。
pub fn parse_body(body: &Bytes) -> ApiResult<Value> {
    if body.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_slice(body)
        .map_err(|e| ApiError::bad_request(format!("请求体不是合法 JSON：{e}")))
}

/// 查询参数包装：**全部按字符串取**，由调用方自己解析成数字。
///
/// 为什么不用 `Query<某个结构体>`：那样「`w=abc`」的报错是 serde 的英文原文，
/// 而本 crate 的错误体要求中文 `message`（任务书 §四）。这里统一手解析，错误消息自己写。
pub struct Params(HashMap<String, String>);

impl Params {
    /// 从 axum 的 `Query` 提取器结果构造（提取失败 ⇒ 400）。
    pub fn from_query(q: Result<Query<HashMap<String, String>>, QueryRejection>) -> ApiResult<Self> {
        match q {
            Ok(Query(m)) => Ok(Params(m)),
            Err(e) => Err(ApiError::bad_request(format!("查询参数无法解析：{e}"))),
        }
    }

    /// 取原始值（**空串按未给**：`?kind=&q=` 是前端常见的"没选"表达）。
    pub fn raw(&self, key: &str) -> Option<&str> {
        self.0
            .get(key)
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
    }

    /// 取原始值，但**空串不按未给处理**（只 trim，不过滤空串）。
    ///
    /// 与 [`Params::raw`] 的区别只有这一条。用于「**给了就必须合法**」的枚举参数：
    /// `/api/tiles` 的 `doors` 用本方法 —— `?doors=` 是「给了个空值」⇒ 400，
    /// 而不是「没给」⇒ 缺省 `0`。若按 `raw` 处理，就等于**静默当成 0**，
    /// 参数写错与「勾了没变」都永远查不出来。
    pub fn raw_verbatim(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(|s| s.trim())
    }

    /// 取 `u32`（缺省用 `default`；给了但不是整数 ⇒ 400）。
    pub fn u32_or(&self, key: &str, default: u32) -> ApiResult<u32> {
        match self.raw(key) {
            None => Ok(default),
            Some(s) => s
                .parse::<u32>()
                .map_err(|_| ApiError::bad_request(format!("{key} 必须是非负整数，收到：{s}"))),
        }
    }

    /// 取 `i64`（坐标用；允许负值）。
    pub fn i64_or(&self, key: &str, default: i64) -> ApiResult<i64> {
        match self.raw(key) {
            None => Ok(default),
            Some(s) => s
                .parse::<i64>()
                .map_err(|_| ApiError::bad_request(format!("{key} 必须是整数，收到：{s}"))),
        }
    }
}

/// 把渲染结果编码成 PNG 字节。
///
/// **刻意复用** `miredit_render::png_io::write_rgba_png`（任务书 §四：别自己写 PNG 编码）：
/// 它是 `render_maps` 导出参考图用的同一个函数，所以 `/api/tiles` 出的 PNG 与
/// `render_maps` 出的同区域 PNG **逐字节相同**（验收门 §五.4 就是比这个）。
///
/// 为什么走临时文件：`png_io` 的公开 API 只有「写文件」（`write_rgba_png` / `PngImage::save_png`），
/// 没有「返回 `Vec<u8>`」的形态。为了不改别人的 crate（任务书 §六），这里写完立刻读回、随即删除。
/// 临时文件名带 pid + 自增序号，并发请求不会撞名。
pub fn encode_png(r: &Rendered) -> ApiResult<Vec<u8>> {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("miredit-web-{}-{seq}.png", std::process::id()));
    miredit_render::png_io::write_rgba_png(&path, r.width, r.height, &r.rgba).map_err(|e| {
        ApiError::internal(format!(
            "PNG 编码失败（{}×{}，临时文件 {}）：{e}",
            r.width,
            r.height,
            path.display()
        ))
    })?;
    let bytes = std::fs::read(&path).map_err(|e| {
        ApiError::internal(format!("PNG 读回失败（{}）：{e}", path.display()))
    });
    // 无论成败都删掉临时文件（失败也只留一行 stderr，不掩盖原错误）
    let removed = std::fs::remove_file(&path);
    if removed.is_err() {
        eprintln!("[瓦片] 临时文件清理失败：{}", path.display());
    }
    bytes
}

/// 在**不阻塞 tokio 工作线程**的前提下跑一段同步 CPU/IO 代码。
///
/// `block_in_place` 会把当前 worker 上的其它任务挪到别的线程，所以这里能安全地做
/// 几十毫秒级的渲染 / PNG 编码 / 5 MB 读盘。要求运行时是 multi-thread（`main.rs` 就是）。
pub fn blocking<T>(f: impl FnOnce() -> T) -> T {
    tokio::task::block_in_place(f)
}

#[cfg(test)]
mod tests {
    use super::*;
    use miredit_render::{CollisionOverlay, LayerMask, Viewport};
    use mir2_assets::{LibPaths, WilSource};

    /// 本机真值路径（不存在就跳过，换机器也能跑 —— 与 `miredit-render` 的
    /// `render_maps` 同一口径：环境缺资源不算代码错）。
    /// 路径**相对资产根** `MIREDIT_ASSET_ROOT`（缺省本机真实根）。
    const 真地图: &str = "GodotClient/maps/0.map";

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

    /// 真实地图的**绝对路径**（相对段挂在资产根下）；缺根 ⇒ `None`。
    fn 真地图路径() -> Option<std::path::PathBuf> {
        Some(资产根()?.join(真地图))
    }

    /// **验收门 §五.4 的自证**：`/api/tiles` 的 PNG 必须与 `render_maps` 的 PNG 逐字节相同。
    ///
    /// 两条路径在这里各跑一遍：
    /// - A（= `render_maps`）：`render_region` + `Rendered::save_png`；
    /// - B（= `/api/tiles`）：`try_render_region_with(全层, 无阻挡)` + 本模块的 [`encode_png`]。
    ///
    /// 两者只差「`try_render_region_with` 传了个默认 overlay」（渲染库保证与旧路径逐字节相同），
    /// 以及 PNG 写到内存还是写文件 —— 所以字节必须完全一样。
    #[test]
    fn 出图与render_maps逐字节相同() {
        let Some(map_path_buf) = 真地图路径() else {
            eprintln!("[跳过] 无资产根 MIREDIT_ASSET_ROOT（需要真实地图素材）");
            return;
        };
        let map_path = map_path_buf.as_path();
        if !map_path.is_file() {
            eprintln!("[跳过] 真实地图不存在：{}", map_path.display());
            return;
        }
        let Some(root) = mir2_assets::client_root() else {
            eprintln!("[跳过] 未设置 {}（图库扫描根）", mir2_assets::CLIENT_ROOT_ENV);
            return;
        };
        let Ok(paths) = LibPaths::discover(&root) else {
            eprintln!("[跳过] 图库根扫描失败：{}", root.display());
            return;
        };
        if paths.is_empty() {
            eprintln!("[跳过] 图库根下一个库都没有：{}", root.display());
            return;
        }
        let src = WilSource::open(&paths).expect("图库打开");
        let bytes = std::fs::read(map_path).expect("读地图");
        let map = mir2_map::Map::parse(&bytes).expect("解析地图");

        // A：`render_maps` 的路径（`render_region` + `save_png`）
        let vp = Viewport::new(0, 0, 64, 48);
        let a = miredit_render::render_region(&map, &src, &vp);
        let tmp = std::env::temp_dir().join("miredit-web-与render_maps比对.png");
        a.save_png(&tmp).expect("A 导出 PNG");
        let a_bytes = std::fs::read(&tmp).expect("A 读回");
        let _ = std::fs::remove_file(&tmp);

        // B：`/api/tiles` 的路径
        let b = miredit_render::try_render_region_with(
            &map,
            &src,
            &vp,
            &LayerMask::all(),
            CollisionOverlay::none(),
        )
        .expect("B 渲染");
        assert_eq!((a.width, a.height), (b.width, b.height), "两条路径的像素尺寸必须一致");
        assert_eq!(a.rgba, b.rgba, "两条路径的 RGBA 必须逐字节相同");
        let b_bytes = encode_png(&b).expect("B 编码 PNG");
        assert_eq!(a_bytes, b_bytes, "两条路径的 PNG 必须逐字节相同（验收门 §五.4）");
        assert!(a_bytes.len() > 1024, "PNG 不该是空壳");
    }

    /// **多倍率的自证**（验收门 §五.2 / §五.3）：
    ///
    /// 1. `RenderScale::One` 档与旧路径 `try_render_region_with` **逐字节相同**
    ///    （`/api/tiles?...&scale=1` 与改动前完全一致，这是硬保证）；
    /// 2. `0.25` / `0.5` 的输出尺寸**精确等于** `round(w*48*s) × round(h*32*s)`；
    /// 3. 缩小档的 PNG 体积**显著小于** 1×（服务端出小图的收益就在这里）。
    ///
    /// 用 16×12 格（`1× = 768×512`）而不是 64×48，避免测试里再编一张 4.5 MB 的 PNG；
    /// 体积比与尺寸口径与 64×48 同源（同一个 `RenderScale::pixel_size` 与同一个编码器）。
    #[test]
    fn 倍率出图尺寸与体积() {
        use miredit_render::RenderScale;

        let Some(map_path_buf) = 真地图路径() else {
            eprintln!("[跳过] 无资产根 MIREDIT_ASSET_ROOT（需要真实地图素材）");
            return;
        };
        let map_path = map_path_buf.as_path();
        if !map_path.is_file() {
            eprintln!("[跳过] 真实地图不存在：{}", map_path.display());
            return;
        }
        let Some(root) = mir2_assets::client_root() else {
            eprintln!("[跳过] 未设置 {}（图库扫描根）", mir2_assets::CLIENT_ROOT_ENV);
            return;
        };
        let Ok(paths) = LibPaths::discover(&root) else {
            eprintln!("[跳过] 图库根扫描失败：{}", root.display());
            return;
        };
        if paths.is_empty() {
            eprintln!("[跳过] 图库根下一个库都没有：{}", root.display());
            return;
        }
        let src = WilSource::open(&paths).expect("图库打开");
        let bytes = std::fs::read(map_path).expect("读地图");
        let map = mir2_map::Map::parse(&bytes).expect("解析地图");

        let vp = Viewport::new(0, 0, 16, 12);
        let mask = LayerMask::all();
        let none = CollisionOverlay::none();

        // ---- 1. One 档与旧路径逐字节相同 ----
        let 旧 = miredit_render::try_render_region_with(&map, &src, &vp, &mask, none)
            .expect("旧路径渲染");
        let 新 = miredit_render::try_render_region_scaled(
            &map,
            &src,
            &vp,
            &mask,
            none,
            RenderScale::One,
        )
        .expect("One 档渲染");
        assert_eq!(新.scale, RenderScale::One, "One 档的 Rendered.scale 必须是 One");
        assert_eq!(旧.rgba, 新.rgba, "One 档与旧路径的 RGBA 必须逐字节相同");
        assert_eq!(
            encode_png(&旧).expect("旧路径编码"),
            encode_png(&新).expect("One 档编码"),
            "One 档与旧路径的 PNG 必须逐字节相同（验收门 §五.2）"
        );

        // ---- 2/3. 三档尺寸与体积 ----
        let 一 = encode_png(&旧).expect("1× 编码");
        let mut sizes = Vec::new();
        for (档, 期望) in [
            // 16×12 格：1× = 768×512 ⇒ 0.25 档 = 192×96、0.5 档 = 384×192
            (RenderScale::Quarter, (192u32, 96u32)),
            (RenderScale::Half, (384, 192)),
        ] {
            let out = miredit_render::try_render_region_scaled(&map, &src, &vp, &mask, none, 档)
                .expect("缩小档渲染");
            assert_eq!((out.width, out.height), 期望, "{} 档尺寸", 档.label());
            assert_eq!(out.scale, 档);
            assert_eq!(
                out.rgba.len(),
                (期望.0 * 期望.1 * 4) as usize,
                "{} 档 RGBA 长度", 档.label()
            );
            let png = encode_png(&out).expect("缩小档编码");
            assert!(png.len() < 一.len(), "{} 档的 PNG 必须小于 1×", 档.label());
            sizes.push((档.label(), png.len()));
        }
        eprintln!(
            "[倍率体积] 16×12 格：1× = {} B，{} = {} B，{} = {} B",
            一.len(),
            sizes[0].0,
            sizes[0].1,
            sizes[1].0,
            sizes[1].1
        );
        // 0.25 档的面积是 1× 的 1/16，PNG 至少小 4 倍（实测通常 20 倍以上）
        assert!(
            sizes[0].1 * 4 < 一.len(),
            "0.25 档体积 {} 应远小于 1× 的 {}",
            sizes[0].1,
            一.len()
        );
    }
}

