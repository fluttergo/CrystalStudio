//! **多倍率渲染**（`0.25 / 0.5 / 1 / 2`）的验收门 —— 任务书 §四 门 2/3/4/5 + 四档耗时 + 可选出图。
//!
//! | 门 | 用例 | 判据 |
//! |---|---|---|
//! | 2 | [`门2_一档与旧路径逐字节相同_0map_64x48`] | `0.map` 64×48：`RenderScale::One` 与 `try_render_region_with` 的 `rgba` **逐字节相等** |
//! | 3 | [`门3_四档尺寸精确_8x6`] | 四档实际 `width×height` == `round(w*48*f) × round(h*32*f)` |
//! | 3b | [`门3b_中间图超限在取帧之前就报错`] | `200×150 @0.25`（目标 11.0 MiB / 中间图 175.8 MiB）⇒ `IntermediateTooLarge`，帧源 **0 次**；同一视口 `@1` 仍正常出图 |
//! | 4 | [`门4_降采样是块平均而不是丢点采样`] | `0.5×` 每像素 == 对应 2×2 块的**面积加权均值**（容差内）；且不等于任何源色 |
//! | 4b | [`门4b_半倍_64x48_与改前基线逐字节相同`] | `0.map` 64×48 `@0.5×` 的 FNV-1a64 == 改前基线 `0x52ed654a6ead95ec`（流式 Box 不得改数值）|
//! | 5 | [`门5_二倍超限返回过大错误且不分配`] | 64×48 @ 2× ⇒ `TooLarge`，且帧源**一次都没被调用**（没开始渲） |
//! | — | [`四档耗时_64x48`] | 打印 release 耗时（`MIREDIT_TIMING=1` 才跑，避免拖慢日常 `cargo test`） |
//! | 6 | [`门6_四档出图供肉眼比对`] | 4 张 PNG 落到 `layout/refs/render-scale-{025,050,100,200}.png` |
//!
//! 真实资源缺失时一律「打印原因 + 跳过」，**不 panic**（与 `tests/acceptance.rs` 同一条纪律）。
//!
//! 口径提醒：`0.5×` 的「块均值」要在**预乘 alpha** 空间比 —— 这是 `resample.rs` 写死的
//! 滤波口径（透明像素的 RGB 是调色板索引 0 的颜色，直通 alpha 平均会把黑边糊进边缘）。
//! 直通空间的偏差在 alpha 不一致的块上本来就会偏大，那是**正确**的，不是缺陷。

use std::path::{Path, PathBuf};
use std::time::Instant;

use mir2_assets::{Frame, LibPaths, LibRef, SpriteSource, WilSource};
use mir2_map::Map;
use miredit_render::{
    try_render_region_scaled, try_render_region_with, CollisionOverlay, LayerMask, RenderError,
    RenderScale, Viewport, INTERMEDIATE_MAX_BYTES, SCALED_MAX_BYTES,
};

/// 抽样地图（**相对资产根** `MIREDIT_ASSET_ROOT`，不是猜的）：`0.map` 700×700 / Old12。
const 真实地图: &str = "GodotClient/maps/0.map";

/// 资产根缺省值：本机真实根（**整个文件里只出现这一处**）。换机器用 `MIREDIT_ASSET_ROOT`。
const 缺省资产根: &str = r"E:\GodM2";

/// 资产根：`MIREDIT_ASSET_ROOT` → 缺省 [`缺省资产根`]。
///
/// 根目录不存在（公开仓/PR 的机器上没有游戏素材）⇒ `None`，调用方打印 `[跳过]` 并返回。
/// 只做 `var_os` + `PathBuf`，不引入任何第三方库。
fn 资产根() -> Option<PathBuf> {
    let root = std::env::var_os("MIREDIT_ASSET_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(缺省资产根));
    root.is_dir().then_some(root)
}

// ---------------------------------------------------------------------------
// 资源 / 造图小工具（与 tests/acceptance.rs 同款，缺资源就跳过）
// ---------------------------------------------------------------------------

/// `layout/refs`（相对本 crate 清单目录）。
fn refs_目录() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("layout")
        .join("refs")
}

/// 图库根（可用环境变量覆盖，默认本机 23 周年客户端）。
fn 图库根() -> PathBuf {
    std::env::var("MIREDIT_CLIENT_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(mir2_assets::DEFAULT_CLIENT_ROOT))
}

/// 打开图源；根目录/图库不可用时打印原因并返回 `None`（调用方跳过，不 panic）。
fn 打开图源() -> Option<WilSource> {
    let root = 图库根();
    if !root.is_dir() {
        eprintln!("[跳过] 图库根不存在：{}", root.display());
        return None;
    }
    let paths = match LibPaths::discover(&root) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[跳过] 图库扫描失败：{e}");
            return None;
        }
    };
    if paths.available() == 0 {
        eprintln!("[跳过] {} 下一个图库都没登记", root.display());
        return None;
    }
    match WilSource::open(&paths) {
        Ok(s) => Some(s),
        Err(e) => {
            eprintln!("[跳过] 图源打开失败：{e}");
            None
        }
    }
}

/// 读并解析一张地图（`相对` = 相对资产根的路径段）；失败返回 `None`（打印原因）。
fn 读地图(相对: &str) -> Option<Map> {
    let Some(root) = 资产根() else {
        eprintln!("[跳过] 无资产根 MIREDIT_ASSET_ROOT（需要真实地图素材）");
        return None;
    };
    let p = root.join(相对);
    if !p.is_file() {
        eprintln!("[跳过] 地图不存在：{}", p.display());
        return None;
    }
    let bytes = match std::fs::read(&p) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("[跳过] 读文件失败 {}：{e}", p.display());
            return None;
        }
    };
    match Map::parse(&bytes) {
        Ok(m) => Some(m),
        Err(e) => {
            eprintln!("[跳过] 解析失败 {}：{e}", p.display());
            None
        }
    }
}

/// 造一张 `w×h` 的 Old12 地图，格内容由闭包决定（不依赖磁盘资源）。
fn 造图(w: u16, h: u16, f: impl Fn(u16, u16) -> mir2_map::Cell) -> Map {
    let mut bytes = vec![0u8; 52 + 12 * (w as usize * h as usize)];
    bytes[0..2].copy_from_slice(&w.to_le_bytes());
    bytes[2..4].copy_from_slice(&h.to_le_bytes());
    for x in 0..w {
        for y in 0..h {
            let cell = f(x, y);
            let base = 52 + (x as usize * h as usize + y as usize) * 12;
            bytes[base..base + 2].copy_from_slice(&cell.bk_img.to_le_bytes());
            bytes[base + 2..base + 4].copy_from_slice(&cell.mid_img.to_le_bytes());
            bytes[base + 4..base + 6].copy_from_slice(&cell.fr_img.to_le_bytes());
        }
    }
    Map::parse(&bytes).expect("合成地图应能解析")
}

/// FNV-1a 64 位指纹（本仓 `tests/stamp.rs` 同款；用来把「逐字节相等」变成可贴进汇报的数字）。
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 纯色帧源：整帧一个颜色，尺寸由参数给。
struct 纯色源 {
    w: u32,
    h: u32,
    c: [u8; 4],
}

impl SpriteSource for 纯色源 {
    fn frame(&self, _lib: LibRef, _index: u32) -> Option<Frame> {
        Some(Frame {
            width: self.w,
            height: self.h,
            rgba: self.c.repeat((self.w * self.h) as usize),
            ..mir2_assets::Frame::default()
        })
    }
}

/// 记账帧源：每次 `frame()` 调用 +1，用来证明「超限报错发生在取帧之前」。
struct 记账源 {
    次数: std::cell::Cell<u32>,
}

impl SpriteSource for 记账源 {
    fn frame(&self, _lib: LibRef, _index: u32) -> Option<Frame> {
        self.次数.set(self.次数.get() + 1);
        Some(Frame {
            width: 48,
            height: 32,
            rgba: [1, 2, 3, 255].repeat(48 * 32),
            ..mir2_assets::Frame::default()
        })
    }
}

/// 记账 + **极小帧**（8×8）帧源：同样计 `frame()` 次数，但让「同视口 @1× 正常出图」的
/// 对照渲染足够快（`200×150` 格要贴 9 万次，48×32 的帧在 debug 下太慢）。
struct 记账微帧源 {
    次数: std::cell::Cell<u32>,
}

impl SpriteSource for 记账微帧源 {
    fn frame(&self, _lib: LibRef, _index: u32) -> Option<Frame> {
        self.次数.set(self.次数.get() + 1);
        Some(Frame {
            width: 8,
            height: 8,
            rgba: [1, 2, 3, 255].repeat(8 * 8),
            ..mir2_assets::Frame::default()
        })
    }
}

// =============================================================================
// 门 2：`RenderScale::One` 与旧路径逐字节相同（0.map 64×48）
// =============================================================================
#[test]
fn 门2_一档与旧路径逐字节相同_0map_64x48() {
    let Some(map) = 读地图(真实地图) else { return };
    let Some(src) = 打开图源() else { return };
    let vp = Viewport::new(0, 0, 64, 48);

    let 旧 = try_render_region_with(
        &map,
        &src,
        &vp,
        &LayerMask::all(),
        CollisionOverlay::none(),
    )
    .expect("旧路径应成功");
    let 新 = try_render_region_scaled(
        &map,
        &src,
        &vp,
        &LayerMask::all(),
        CollisionOverlay::none(),
        RenderScale::One,
    )
    .expect("1× 档应成功");

    println!(
        "[门2] 0.map 64×48：旧 {}×{} / {} 字节 / fnv1a {:016x} / stats {}",
        旧.width,
        旧.height,
        旧.rgba.len(),
        fnv1a(&旧.rgba),
        旧.stats
    );
    println!(
        "[门2] 0.map 64×48：新 {}×{} / {} 字节 / fnv1a {:016x} / stats {} / scale {:?}",
        新.width,
        新.height,
        新.rgba.len(),
        fnv1a(&新.rgba),
        新.stats,
        新.scale
    );

    assert_eq!((旧.width, 旧.height), (3072, 1536), "64×48 格 ⇒ 3072×1536");
    assert_eq!(旧.rgba.len(), 3072 * 1536 * 4);
    assert_eq!(旧.rgba, 新.rgba, "1× 档必须与旧路径**逐字节**相同");
    assert_eq!(旧.stats, 新.stats, "统计也必须相同");
    assert_eq!(新.scale, RenderScale::One, "1× 档的 scale 字段应是 One");
    println!("[门2] 逐字节相等 ✔（{} 字节）", 旧.rgba.len());
}

// =============================================================================
// 门 3：四档尺寸精确（8×6 格小视口）
// =============================================================================
#[test]
fn 门3_四档尺寸精确_8x6() {
    let map = 造图(8, 6, |_, _| mir2_map::Cell {
        bk_img: 1,
        mid_img: 1,
        fr_img: 1,
        ..Default::default()
    });
    let src = 纯色源 {
        w: 96,
        h: 64,
        c: [30, 40, 50, 255],
    };
    let vp = Viewport::new(0, 0, 8, 6);
    for s in RenderScale::ALL {
        let out = try_render_region_scaled(
            &map,
            &src,
            &vp,
            &LayerMask::all(),
            CollisionOverlay::none(),
            s,
        )
        .expect("四档都应成功");
        // 期望值按公式**独立**再算一遍（不用被测代码的 `pixel_size`，避免自证）
        let f = f64::from(s.factor());
        let 期望 = (
            (8.0 * 48.0 * f).round() as u32,
            (6.0 * 32.0 * f).round() as u32,
        );
        println!(
            "[门3] {:>5}× 实际 {}×{} / 期望 round(8*48*{f}) × round(6*32*{f}) = {}×{} / rgba {} 字节",
            s.label(),
            out.width,
            out.height,
            期望.0,
            期望.1,
            out.rgba.len()
        );
        assert_eq!((out.width, out.height), 期望, "{}", s.label());
        assert_eq!(out.rgba.len(), (期望.0 * 期望.1 * 4) as usize, "{}", s.label());
        assert_eq!(out.scale, s);
    }
}

// =============================================================================
// 门 4：降采样质量 —— 0.5× 每像素 ≈ 对应 2×2 块均值（不是丢点采样）
// =============================================================================
#[test]
fn 门4_降采样是块平均而不是丢点采样() {
    // ---- (a) 合成棋盘：不依赖磁盘资源，判据最强（每个 2×2 块恰好含四种源色）----
    struct 四色棋盘;
    impl SpriteSource for 四色棋盘 {
        fn frame(&self, _lib: LibRef, _index: u32) -> Option<Frame> {
            let 色 = [
                [255u8, 0, 0, 255],
                [0, 255, 0, 255],
                [0, 0, 255, 255],
                [255, 255, 255, 255],
            ];
            let mut rgba = Vec::with_capacity(96 * 64 * 4);
            for y in 0..64u32 {
                for x in 0..96u32 {
                    rgba.extend_from_slice(&色[((x % 2) + 2 * (y % 2)) as usize]);
                }
            }
            Some(Frame {
                width: 96,
                height: 64,
                rgba,
                ..mir2_assets::Frame::default()
            })
        }
    }
    let map = 造图(8, 6, |_, _| mir2_map::Cell {
        bk_img: 1,
        ..Default::default()
    });
    let vp = Viewport::new(0, 0, 8, 6);
    let 一 = try_render_region_scaled(
        &map,
        &四色棋盘,
        &vp,
        &LayerMask::all(),
        CollisionOverlay::none(),
        RenderScale::One,
    )
    .unwrap();
    let 半 = try_render_region_scaled(
        &map,
        &四色棋盘,
        &vp,
        &LayerMask::all(),
        CollisionOverlay::none(),
        RenderScale::Half,
    )
    .unwrap();
    assert_eq!((一.width, 一.height), (384, 192));
    assert_eq!((半.width, 半.height), (192, 96));

    let mut 最大偏差 = 0i32;
    let mut 超差像素 = 0usize;
    for y in 0..半.height {
        for x in 0..半.width {
            let mut acc = [0i32; 4];
            for dy in 0..2 {
                for dx in 0..2 {
                    let p = 一.pixel(x * 2 + dx, y * 2 + dy).unwrap();
                    for c in 0..4 {
                        acc[c] += i32::from(p[c]);
                    }
                }
            }
            let 期望 = acc.map(|v| (v as f64 / 4.0).round() as i32);
            let 实际 = 半.pixel(x, y).unwrap();
            for c in 0..3 {
                let d = (i32::from(实际[c]) - 期望[c]).abs();
                最大偏差 = 最大偏差.max(d);
                if d > 1 {
                    超差像素 += 1;
                }
            }
        }
    }
    println!(
        "[门4][a 合成棋盘] 0.5× 每像素 vs 2×2 块均值：最大偏差 {最大偏差}（{} 个像素 × 3 通道，超差 {超差像素} 个）",
        半.width * 半.height
    );
    assert_eq!(超差像素, 0, "0.5× 必须逐像素等于 2×2 块均值（只允许 ±1 取整）");
    assert!(最大偏差 <= 1);

    // 反证：点采样只会吐出四种源色之一；块均值是 (128,128,128) 量级
    let 源色 = [[255u8, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 255]];
    let p00 = 半.pixel(0, 0).unwrap();
    assert!(
        !源色.iter().any(|c| *c == [p00[0], p00[1], p00[2]]),
        "0.5× 的 (0,0) 等于源色 {p00:?} ⇒ 这是丢点采样"
    );
    println!("[门4][a 合成棋盘] 反证：0.5× 的 (0,0) = {p00:?}（既非纯红/绿/蓝/白，说明是均值）✔");

    // ---- (b) 真实 0.map：参考值 = 测试内独立实现的**预乘 2×2 块均值** ----
    let Some(map) = 读地图(真实地图) else { return };
    let Some(src) = 打开图源() else { return };
    let vp = Viewport::new(300, 250, 16, 12);
    let 一 = try_render_region_scaled(
        &map,
        &src,
        &vp,
        &LayerMask::all(),
        CollisionOverlay::none(),
        RenderScale::One,
    )
    .unwrap();
    let 半 = try_render_region_scaled(
        &map,
        &src,
        &vp,
        &LayerMask::all(),
        CollisionOverlay::none(),
        RenderScale::Half,
    )
    .unwrap();
    assert_eq!((一.width, 一.height), (16 * 48, 12 * 32));
    assert_eq!((半.width, 半.height), (8 * 48, 6 * 32));

    let mut 最大预乘偏差 = 0.0f64;
    let mut 最大alpha偏差 = 0.0f64;
    let mut 最大直通偏差 = 0i32;
    let mut 非均匀块 = 0usize;
    let mut 不透明像素 = 0usize;
    for y in 0..半.height {
        for x in 0..半.width {
            let mut mc = [0.0f64; 3];
            let mut ma = 0.0f64;
            let mut 直通 = [0i32; 4];
            let mut 首像素 = None;
            let mut 块内不一致 = false;
            for dy in 0..2 {
                for dx in 0..2 {
                    let p = 一.pixel(x * 2 + dx, y * 2 + dy).unwrap();
                    if p[3] != 0 {
                        不透明像素 += 1;
                    }
                    match 首像素 {
                        None => 首像素 = Some(p),
                        Some(q) if q != p => 块内不一致 = true,
                        _ => {}
                    }
                    let a = f64::from(p[3]);
                    for c in 0..3 {
                        mc[c] += f64::from(p[c]) * a / 255.0;
                        直通[c] += i32::from(p[c]);
                    }
                    ma += a;
                    直通[3] += i32::from(p[3]);
                }
            }
            if 块内不一致 {
                非均匀块 += 1;
            }
            for v in mc.iter_mut() {
                *v /= 4.0;
            }
            ma /= 4.0;
            let q = 半.pixel(x, y).unwrap();
            let aq = f64::from(q[3]);
            for c in 0..3 {
                let 实际预乘 = f64::from(q[c]) * aq / 255.0;
                最大预乘偏差 = 最大预乘偏差.max((实际预乘 - mc[c]).abs());
                // 直通空间只作对照（alpha 不一致的块上本来就会偏大，不算缺陷）
                let 直通均值 = (直通[c] as f64 / 4.0).round() as i32;
                最大直通偏差 = 最大直通偏差.max((i32::from(q[c]) - 直通均值).abs());
            }
            最大alpha偏差 = 最大alpha偏差.max((aq - ma).abs());
        }
    }
    println!(
        "[门4][b 0.map 16×12@(300,250)] 0.5× vs 预乘 2×2 块均值：最大偏差 {最大预乘偏差:.3} / alpha 最大偏差 {最大alpha偏差:.3}（直通空间对照 {最大直通偏差}）"
    );
    println!(
        "[门4][b 0.map 16×12@(300,250)] 样本构成：{} 个 2×2 块中 {非均匀块} 个块内像素不一致；不透明源像素 {} / {}",
        半.width * 半.height,
        不透明像素,
        一.width * 一.height
    );
    assert!(
        非均匀块 > 0,
        "这个区域每个 2×2 块都同色 ⇒ 比对是空转的，必须换一块有细节的区域"
    );
    assert!(
        最大预乘偏差 <= 2.0,
        "预乘空间的块均值偏差应 ≤ 2（只有取整误差），实测 {最大预乘偏差:.3}"
    );
    assert!(最大alpha偏差 <= 1.0, "alpha 偏差应 ≤ 1，实测 {最大alpha偏差:.3}");
}

// =============================================================================
// 门 3b：**1× 中间图**护栏 —— 目标很小、中间图很大（200×150 @0.25）
// =============================================================================
#[test]
fn 门3b_中间图超限在取帧之前就报错() {
    // 200×150 格 @0.25：
    //   目标图   = 2400×1200×4 =  11,520,000 B（11.0 MiB）—— **远低于** 64 MiB 目标上限
    //   1× 中间图 = 9600×4800×4 = 184,320,000 B（175.8 MiB）—— 新护栏必须拦下
    // ⇒ 只卡目标尺寸的老护栏会放行并**真分配** 176 MB，这一条就是堵它。
    let map = 造图(200, 150, |_, _| mir2_map::Cell {
        bk_img: 1,
        ..Default::default()
    });
    let src = 记账微帧源 {
        次数: std::cell::Cell::new(0),
    };
    let vp = Viewport::new(0, 0, 200, 150);

    // 先自证「目标图确实没超限」——否则这条用例就退化成在测老护栏了
    let (目标宽, 目标高) = RenderScale::Quarter.pixel_size(200, 150).unwrap();
    let 目标字节 = u64::from(目标宽) * u64::from(目标高) * 4;
    println!(
        "[门3b] 目标图 {目标宽}×{目标高}×4 = {目标字节} B（{:.1} MiB），目标上限 {:.0} MiB ⇒ 目标图不超限",
        目标字节 as f64 / (1024.0 * 1024.0),
        SCALED_MAX_BYTES as f64 / (1024.0 * 1024.0)
    );
    assert!(
        目标字节 < SCALED_MAX_BYTES,
        "本例的前提是「目标图很小」，目标图必须先不超限"
    );

    let err = try_render_region_scaled(
        &map,
        &src,
        &vp,
        &LayerMask::all(),
        CollisionOverlay::none(),
        RenderScale::Quarter,
    )
    .expect_err("200×150 @0.25 必须被中间图护栏拒绝");
    println!("[门3b] 错误原文：{err}");

    match &err {
        RenderError::IntermediateTooLarge { want, limit } => {
            assert_eq!(*want, 9600 * 4800 * 4, "应报 1× 中间图字节数");
            assert_eq!(*limit, INTERMEDIATE_MAX_BYTES, "上限应是中间图上限");
            assert_eq!(*limit, 128 * 1024 * 1024, "中间图上限 = 128 MiB");
            assert!(*want > *limit, "必须真的超限");
            println!(
                "[门3b] 中间图 {want} B（{:.1} MiB）> 上限 {limit} B（{:.0} MiB）✔",
                *want as f64 / (1024.0 * 1024.0),
                *limit as f64 / (1024.0 * 1024.0)
            );
        }
        other => panic!("应为 IntermediateTooLarge，实际 {other:?}"),
    }

    // 文案必须说清「这是中间图超限，不是目标图超限」并给出建议（否则运维会去调目标图上限）
    let 文案 = err.to_string();
    for 关键词 in ["1× 中间图", "不是目标图超限", "缩小视口格数"] {
        assert!(
            文案.contains(关键词),
            "错误文案缺少关键词 {关键词:?}：{文案}"
        );
    }

    // **没开始渲**：帧源一次都不该被调用（连 1× 合成都还没进）
    assert_eq!(
        src.次数.get(),
        0,
        "中间图超限必须在取帧之前返回（帧源一次都不该被调用）"
    );
    println!("[门3b] 帧源被调用 0 次 ✔（守卫发生在 compose 之前）");

    // 同一视口 @1× 必须**正常出图**：`One` 不产生中间图，不受这条上限约束
    let 一 = try_render_region_scaled(
        &map,
        &src,
        &vp,
        &LayerMask::all(),
        CollisionOverlay::none(),
        RenderScale::One,
    )
    .expect("同一视口 @1× 不该受中间图上限影响（One 不产生中间图）");
    assert_eq!((一.width, 一.height), (9600, 4800), "200×150 格 @1× ⇒ 9600×4800");
    assert_eq!(一.rgba.len(), 184_320_000, "1× 输出就是中间图本身（184.3 MB）");
    println!(
        "[门3b] 对照：同一 200×150 视口 @1× ⇒ {}×{} / {} 字节 正常出图 ✔（帧源调用 {} 次）",
        一.width,
        一.height,
        一.rgba.len(),
        src.次数.get()
    );
}

// =============================================================================
// 门 4b：0.5× @64×48 与**改动前**逐字节相同（流式 Box vs 整幅预乘 Box）
// =============================================================================
#[test]
fn 门4b_半倍_64x48_与改前基线逐字节相同() {
    // 改前基线（2026-11-05 02:2x，release，探针实测，见汇报）：
    //   0.map 64×48 @0.5× ⇒ 1536×768 / 4,718,592 B / FNV-1a64 = 0x52ed654a6ead95ec
    //
    // 缩小路径从「整幅 f64 预乘的 Box」换成「流式面积加权 Box」，**数值一个 bit 都不许变**
    //（网页端已有 0.5× 的视觉基准，悄悄变样等于假数据）。
    const 改前基线: u64 = 0x52ed_654a_6ead_95ec;

    let Some(map) = 读地图(真实地图) else { return };
    let Some(src) = 打开图源() else { return };
    let vp = Viewport::new(0, 0, 64, 48);

    let 一 = try_render_region_scaled(
        &map,
        &src,
        &vp,
        &LayerMask::all(),
        CollisionOverlay::none(),
        RenderScale::Half,
    )
    .expect("0.5× 应成功");
    let 指纹 = fnv1a(&一.rgba);
    println!(
        "[门4b] 0.map 64×48 @0.5× ⇒ {}×{} / {} 字节 / fnv1a {:016x}（改前基线 {:016x}）",
        一.width,
        一.height,
        一.rgba.len(),
        指纹,
        改前基线
    );
    assert_eq!((一.width, 一.height), (1536, 768), "64×48 格 @0.5× ⇒ 1536×768");
    assert_eq!(一.rgba.len(), 1536 * 768 * 4);
    assert_eq!(
        指纹, 改前基线,
        "流式 Box 必须与改前的整幅预乘 Box **逐字节相同**（0x{指纹:016x} != 0x{改前基线:016x}）"
    );

    // 再渲一次：确定性（不是碰巧撞上基线）
    let 二 = try_render_region_scaled(
        &map,
        &src,
        &vp,
        &LayerMask::all(),
        CollisionOverlay::none(),
        RenderScale::Half,
    )
    .expect("0.5× 第二次也应成功");
    assert_eq!(二.rgba, 一.rgba, "两次渲染必须逐字节相同（确定性）");
    println!("[门4b] 逐字节相同 ✔（{} 字节；FNV-1a64 = {:016x}）", 一.rgba.len(), 指纹);
}

// =============================================================================
// 门 5：超限保护 —— 64×48 @ 2× 超 64 MiB ⇒ TooLarge，且**不分配 / 不取帧**
// =============================================================================
#[test]
fn 门5_二倍超限返回过大错误且不分配() {
    // 视口给 64×48 格：2× ⇒ 6144×3072×4 B = 75,497,472 B > 67,108,864 B（64 MiB）
    let map = 造图(64, 48, |_, _| mir2_map::Cell {
        bk_img: 1,
        ..Default::default()
    });
    let src = 记账源 {
        次数: std::cell::Cell::new(0),
    };
    let vp = Viewport::new(0, 0, 64, 48);
    let t = Instant::now();
    let err = try_render_region_scaled(
        &map,
        &src,
        &vp,
        &LayerMask::all(),
        CollisionOverlay::none(),
        RenderScale::Two,
    )
    .expect_err("64×48 @ 2× 必须被拒绝");
    let 耗时 = t.elapsed();

    println!("[门5] 错误原文：{err}");
    println!(
        "[门5] 守卫耗时 {:.3} ms；帧源被调用 {} 次（应为 0 —— 证明没开始渲、没分配中间图）",
        耗时.as_secs_f64() * 1000.0,
        src.次数.get()
    );

    match &err {
        RenderError::TooLarge { want, limit } => {
            assert_eq!(*want, 6144 * 3072 * 4, "应报目标字节数");
            assert_eq!(*limit, SCALED_MAX_BYTES, "上限应是 64 MiB");
            assert_eq!(*limit, 64 * 1024 * 1024);
            assert!(*want > *limit);
        }
        other => panic!("应为 TooLarge，实际 {other:?}"),
    }
    assert_eq!(src.次数.get(), 0, "超限必须在分配/取帧之前返回");

    // 同一视口的 0.25 / 0.5 / 1 都不超限（证明上面的 0 次取帧不是别的原因造成的）
    for s in [RenderScale::Quarter, RenderScale::Half, RenderScale::One] {
        let out = try_render_region_scaled(
            &map,
            &src,
            &vp,
            &LayerMask::all(),
            CollisionOverlay::none(),
            s,
        )
        .unwrap_or_else(|e| panic!("{} 不该超限：{e}", s.label()));
        println!("[门5] 对照：{}× ⇒ {}×{} 正常出图", s.label(), out.width, out.height);
    }
}

// =============================================================================
// 四档耗时（release 才有意义；默认跳过，设 MIREDIT_TIMING=1 才跑）
// =============================================================================
#[test]
fn 四档耗时_64x48() {
    if std::env::var("MIREDIT_TIMING").as_deref() != Ok("1") {
        eprintln!("[跳过] 未设置 MIREDIT_TIMING=1（计时只在 release 下有参考价值：cargo test -p miredit-render --release --test scale 四档耗时 -- --nocapture --test-threads=1）");
        return;
    }
    let Some(map) = 读地图(真实地图) else { return };
    let Some(src) = 打开图源() else { return };
    let vp = Viewport::new(0, 0, 64, 48);
    for s in RenderScale::ALL {
        let mut 一次 = Vec::new();
        let mut 说明 = String::new();
        for _ in 0..2 {
            let t = Instant::now();
            match try_render_region_scaled(
                &map,
                &src,
                &vp,
                &LayerMask::all(),
                CollisionOverlay::none(),
                s,
            ) {
                Ok(out) => {
                    一次.push(t.elapsed().as_secs_f64() * 1000.0);
                    说明 = format!("{}×{} px（{} 字节）", out.width, out.height, out.rgba.len());
                }
                Err(e) => {
                    一次.push(t.elapsed().as_secs_f64() * 1000.0);
                    说明 = format!("拒绝：{e}");
                    break;
                }
            }
        }
        let 最快 = 一次.iter().cloned().fold(f64::INFINITY, f64::min);
        println!(
            "[耗时] {:>5}× 64×48 视口 ⇒ {说明}；第 1 次 {:.1} ms，最快 {:.1} ms",
            s.label(),
            一次[0],
            最快
        );
    }
    // 2× 在「装得下」的视口上的真实耗时（64×48 会被 64 MiB 上限拒掉）
    let vp24 = Viewport::new(300, 250, 32, 24);
    let t = Instant::now();
    let out = try_render_region_scaled(
        &map,
        &src,
        &vp24,
        &LayerMask::all(),
        CollisionOverlay::none(),
        RenderScale::Two,
    )
    .expect("32×24 @ 2× 应成功");
    println!(
        "[耗时] 2× 32×24 视口 ⇒ {}×{} px（{:.1} MiB）耗时 {:.1} ms",
        out.width,
        out.height,
        out.rgba.len() as f64 / (1024.0 * 1024.0),
        t.elapsed().as_secs_f64() * 1000.0
    );
}

// =============================================================================
// 门 6（可选）：四档出图供肉眼比对
// =============================================================================
#[test]
fn 门6_四档出图供肉眼比对() {
    let Some(map) = 读地图(真实地图) else { return };
    let Some(src) = 打开图源() else { return };
    // 32×24 格：1× = 1536×768，2× = 3072×1536（18.9 MiB，未触 64 MiB 上限）
    let vp = Viewport::new(300, 250, 32, 24);
    let 目录 = refs_目录();
    if !目录.is_dir() {
        eprintln!("[跳过] 目录不存在：{}", 目录.display());
        return;
    }
    for (s, 名) in [
        (RenderScale::Quarter, "025"),
        (RenderScale::Half, "050"),
        (RenderScale::One, "100"),
        (RenderScale::Two, "200"),
    ] {
        let out = try_render_region_scaled(
            &map,
            &src,
            &vp,
            &LayerMask::all(),
            CollisionOverlay::none(),
            s,
        )
        .expect("四档都应成功");
        let path = 目录.join(format!("render-scale-{名}.png"));
        match out.save_png(&path) {
            Ok(()) => println!(
                "[门6] {}× ⇒ {}×{} px ⇒ {}",
                s.label(),
                out.width,
                out.height,
                path.display()
            ),
            Err(e) => eprintln!("[门6] 导出失败 {}：{e}", path.display()),
        }
    }
}
