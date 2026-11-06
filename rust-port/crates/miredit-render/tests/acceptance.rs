//! 验收门（任务书 §五）：
//!
//! 1. **确定性**：同一输入两次渲染 ⇒ 字节完全相同；
//! 2. **尺寸**：`64×48` 格 ⇒ `3072×1536` px；
//! 3. **真实地图抽样**：0.map / 41.map / n0104.map 各渲染 64×48 格、导出 PNG 到
//!    `layout/refs/render-<图名>.png`、打印 `stats`；**路径不存在就跳过**；
//! 4. **与客户端截图分块比对**（结构级）：需要先用 Godot 客户端出图，见文件末尾说明；
//! 5. **性能**：见 `cargo run --release --bin render_maps` 的输出（debug 下计时无意义，
//!    这里只做一个宽松的冒烟断言并打印）。
//!
//! 真实资源缺失时一律「打印原因 + 跳过」，**不 panic**，保证换机器也能 `cargo test` 绿。

use std::path::{Path, PathBuf};
use std::time::Instant;

use mir2_assets::{LibPaths, SpriteSource, WilSource};
use mir2_map::Map;
use miredit_render::compare::{compare_blocks, CompareOptions};
use miredit_render::png_io::{read_png, PngImage};
use miredit_render::{
    render_region, try_render_region, try_render_region_with, CollisionOverlay, LayerMask,
    RenderStats, Rendered, Viewport, CELL_H, CELL_W,
};

/// 抽样地图（**相对资产根** `MIREDIT_ASSET_ROOT`，不是猜的）。
const 抽样地图: [(&str, &str); 3] = [
    ("0", "GodotClient/maps/0.map"),
    ("41", "GodotClient/maps/41.map"),
    ("n0104", "client/热血传奇23周年客户端/Map/n0104.map"),
];

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
fn 读地图(相对: &str) -> Option<(Map, usize)> {
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
        Ok(m) => Some((m, bytes.len())),
        Err(e) => {
            eprintln!("[跳过] 解析失败 {}：{e}", p.display());
            None
        }
    }
}

/// 全零的合成图（用于不依赖磁盘资源的尺寸断言）。
fn 空图(w: u16, h: u16) -> Map {
    let mut bytes = vec![0u8; 52 + 12 * (w as usize * h as usize)];
    bytes[0..2].copy_from_slice(&w.to_le_bytes());
    bytes[2..4].copy_from_slice(&h.to_le_bytes());
    Map::parse(&bytes).expect("全零合成图应能解析")
}

/// 永远取不到帧的图源（「缺帧」路径用）。
struct 空源;
impl SpriteSource for 空源 {
    fn frame(&self, _lib: mir2_assets::LibRef, _index: u32) -> Option<mir2_assets::Frame> {
        None
    }
}

// =============================================================================
// 验收门 2：尺寸
// =============================================================================
#[test]
fn 验收门2_尺寸_64x48格等于3072x1536() {
    let map = 空图(64, 48);
    let out = render_region(&map, &空源, &Viewport::new(0, 0, 64, 48));
    assert_eq!(
        (out.width, out.height),
        (64 * CELL_W, 48 * CELL_H),
        "64×48 格应为 3072×1536（宽 64*48 / 高 48*32）"
    );
    assert_eq!((out.width, out.height), (3072, 1536));
    assert_eq!(out.rgba.len(), 3072 * 1536 * 4);
    assert!(out.rgba.iter().all(|b| *b == 0), "底色必须全透明黑");
    assert_eq!(out.stats, RenderStats::default());
    println!("[门2] 64×48 格 ⇒ {}×{} px ✔", out.width, out.height);
}

// =============================================================================
// 验收门 1：确定性（合成图 + 真实图各一次）
// =============================================================================
#[test]
fn 验收门1_确定性_合成图两次字节相等() {
    let map = 空图(32, 24);
    let a = render_region(&map, &空源, &Viewport::new(0, 0, 32, 24));
    let b = render_region(&map, &空源, &Viewport::new(0, 0, 32, 24));
    assert_eq!(a, b);
}

#[test]
fn 验收门1_确定性_真实地图两次字节相等() {
    let Some(src) = 打开图源() else { return };
    let Some((map, _)) = 读地图(抽样地图[0].1) else {
        return;
    };
    let vp = Viewport::new(0, 0, 64, 48);
    let a = render_region(&map, &src, &vp);
    let b = render_region(&map, &src, &vp);
    assert_eq!(a.width, b.width);
    assert_eq!(a.rgba, b.rgba, "两次渲染输出必须逐字节相同");
    assert_eq!(a.stats, b.stats);
    println!(
        "[门1] 0.map 64×48 两次渲染字节相同 ✔（{} 字节；stats={}）",
        a.rgba.len(),
        a.stats
    );
}

// =============================================================================
// 验收门 3：真实地图抽样 + 导出参考图
// =============================================================================
#[test]
fn 验收门3_真实地图抽样渲染并导出png() {
    let Some(src) = 打开图源() else { return };
    let dir = refs_目录();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("[跳过] 无法创建 {}：{e}", dir.display());
        return;
    }
    let vp = Viewport::new(0, 0, 64, 48);
    let mut 完成 = 0;
    for (name, path) in 抽样地图 {
        let Some((map, len)) = 读地图(path) else {
            continue;
        };
        let t0 = Instant::now();
        let out: Rendered = render_region(&map, &src, &vp);
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        println!(
            "[门3] {name}.map {}×{} 格 / {:?} / {len} 字节 ⇒ {}×{} px，{ms:.1} ms，stats: {}",
            map.width, map.height, map.format, out.width, out.height, out.stats
        );
        let out_png = dir.join(format!("render-{name}.png"));
        match out.save_png(&out_png) {
            Ok(()) => println!("        已导出 {}", out_png.display()),
            Err(e) => eprintln!("        [错误] 导出失败：{e}"),
        }
        完成 += 1;
    }
    if 完成 == 0 {
        eprintln!("[跳过] 三张抽样地图一张都没读到");
    }
    let fails = src.failures();
    if !fails.is_empty() {
        println!("[门3] 图源失败记录 {} 条（前 3 条）：", fails.len());
        for f in fails.iter().take(3) {
            println!("        - {f}");
        }
    }
}

// =============================================================================
// 验收门 6~9：低/高位阻挡可视化通道（默认关闭，开了才画）
//
//   口径（crate 文档 §七）：低阻挡 = `wBkImg & 0x8000`、高阻挡 = `wFrImg & 0x8000`
//   （客户端 `TMap.cpp:339`）；颜色/形状/透明度借 C# `CollisionMarkers.cs:6-7,19-25`。
// =============================================================================

/// 低阻挡标记色 + 默认 alpha（`CollisionMarkers.cs:6`）。
const 低标记: [u8; 4] = [240, 85, 105, CollisionOverlay::DEFAULT_ALPHA];
/// 高位阻挡标记色 + 默认 alpha（`CollisionMarkers.cs:7`）。
const 高标记: [u8; 4] = [65, 165, 245, CollisionOverlay::DEFAULT_ALPHA];

/// 直接从 `mir2-map` 的格数据数「窗口内带某位」的格数 —— **完全不经过渲染库**，
/// 用作独立口径（验收门 3 的对照组）。
fn 直算阻挡(map: &Map, vp: &Viewport) -> (u32, u32) {
    let mut low = 0u32;
    let mut high = 0u32;
    for dx in 0..vp.cells_w {
        for dy in 0..vp.cells_h {
            let x = vp.origin_x + dx as i32;
            let y = vp.origin_y + dy as i32;
            let (Ok(cx), Ok(cy)) = (u16::try_from(x), u16::try_from(y)) else {
                continue;
            };
            let Some(cell) = map.cell(cx, cy) else {
                continue;
            };
            if cell.bk_img & 0x8000 != 0 {
                low += 1;
            }
            if cell.fr_img & 0x8000 != 0 {
                high += 1;
            }
        }
    }
    (low, high)
}

#[test]
fn 验收门6_新旧路径逐字节相同() {
    // ① 合成图（不依赖磁盘资源，换机器也能跑）：三层都有图 + 各通道阻挡位都点着
    let mut bytes = vec![0u8; 52 + 12 * (8 * 6)];
    bytes[0..2].copy_from_slice(&8u16.to_le_bytes());
    bytes[2..4].copy_from_slice(&6u16.to_le_bytes());
    for i in 0..(8 * 6) {
        let base = 52 + i * 12;
        bytes[base..base + 2].copy_from_slice(&0x8001u16.to_le_bytes());
        bytes[base + 2..base + 4].copy_from_slice(&0x8001u16.to_le_bytes());
        bytes[base + 4..base + 6].copy_from_slice(&0x8001u16.to_le_bytes());
    }
    let map = Map::parse(&bytes).expect("合成图应能解析");
    let vp = Viewport::new(0, 0, 8, 6);
    let 旧 = try_render_region(&map, &空源, &vp, &LayerMask::all()).unwrap();
    let 新 = try_render_region_with(&map, &空源, &vp, &LayerMask::all(), CollisionOverlay::default())
        .unwrap();
    assert_eq!(旧.rgba, 新.rgba, "合成图：默认口径必须与旧路径逐字节相同");
    assert_eq!(旧.stats, 新.stats);
    assert_eq!(旧, 新);
    println!("[门6] 合成图 8×6：旧路径 vs 默认叠加口径 逐字节相同 ✔（{} 字节）", 旧.rgba.len());

    // ② 真实图（0.map 64×48，三层全开）：同样逐字节相同
    let Some(src) = 打开图源() else { return };
    let Some((map, _)) = 读地图(抽样地图[0].1) else {
        return;
    };
    let vp = Viewport::new(0, 0, 64, 48);
    let 旧 = try_render_region(&map, &src, &vp, &LayerMask::all()).unwrap();
    let 新 = try_render_region_with(&map, &src, &vp, &LayerMask::all(), CollisionOverlay::default())
        .unwrap();
    assert_eq!(旧.rgba, 新.rgba, "0.map：默认口径必须与旧路径逐字节相同");
    assert_eq!(旧.stats, 新.stats, "0.map：统计也必须完全相同");
    assert_eq!(旧, 新);
    assert_eq!(新.stats.cells_low_blocked, 0);
    assert_eq!(新.stats.cells_high_blocked, 0);
    println!(
        "[门6] 0.map 64×48：旧路径 vs 默认叠加口径 逐字节相同 ✔（{} 字节，stats={}）",
        旧.rgba.len(),
        旧.stats
    );
}

#[test]
fn 验收门7_标记计数与mir2map直算一致() {
    let Some(src) = 打开图源() else { return };
    let vp = Viewport::new(0, 0, 64, 48);
    let mut 核对 = 0u32;
    for (name, path) in 抽样地图 {
        let Some((map, _)) = 读地图(path) else {
            continue;
        };
        // 视口必须在图内（三张抽样图都够大；不够就跳过，别造越界数据）
        if (map.width as u32) < vp.cells_w || (map.height as u32) < vp.cells_h {
            eprintln!("[跳过] {name}.map 比 64×48 视口小");
            continue;
        }
        let (直算低, 直算高) = 直算阻挡(&map, &vp);
        let out =
            try_render_region_with(&map, &src, &vp, &LayerMask::all(), CollisionOverlay::all())
                .unwrap();
        println!(
            "[门7] {name}.map 64×48：渲染统计 低={} 高={} ／ mir2-map 直算 低={} 高={}",
            out.stats.cells_low_blocked, out.stats.cells_high_blocked, 直算低, 直算高
        );
        assert_eq!(
            out.stats.cells_low_blocked, 直算低,
            "{name}.map：低阻挡标记格数必须等于直算的 bk&0x8000 格数"
        );
        assert_eq!(
            out.stats.cells_high_blocked, 直算高,
            "{name}.map：高阻挡标记格数必须等于直算的 fr&0x8000 格数"
        );
        核对 += 1;
    }
    if 核对 == 0 {
        eprintln!("[跳过] 三张抽样地图一张都没读到");
    }
}

#[test]
fn 验收门8_只开一个通道时另一通道的格不被标记() {
    // 用 41.map：64×48 窗口里 3038 格**两层都带 0x8000**（实测），最能暴露「通道串味」
    let Some(src) = 打开图源() else { return };
    let Some((map, _)) = 读地图(抽样地图[1].1) else {
        return;
    };
    let vp = Viewport::new(0, 0, 64, 48);
    let (直算低, 直算高) = 直算阻挡(&map, &vp);
    assert!(直算低 > 0 && 直算高 > 0, "41.map 窗口内两层都该有阻挡位");
    println!("[门8] 41.map 64×48：直算 低={直算低} 高={直算高}（两者都有的图）");

    // 三层全关 ⇒ 画布本来全透明，标记色能原样读出来（over 在 a_d=0 时输出源色）
    let 只低 =
        try_render_region_with(&map, &src, &vp, &LayerMask::none(), CollisionOverlay::low_only())
            .unwrap();
    assert_eq!(只低.stats.cells_low_blocked, 直算低);
    assert_eq!(只低.stats.cells_high_blocked, 0, "只开低通道 ⇒ 高阻挡格一格都不该计");
    assert!(
        !只低.rgba.as_chunks::<4>().0.contains(&高标记),
        "只开低通道时，画面上不得出现任何高阻挡色"
    );

    let 只高 =
        try_render_region_with(&map, &src, &vp, &LayerMask::none(), CollisionOverlay::high_only())
            .unwrap();
    assert_eq!(只高.stats.cells_high_blocked, 直算高);
    assert_eq!(只高.stats.cells_low_blocked, 0, "只开高通道 ⇒ 低阻挡格一格都不该计");
    assert!(
        !只高.rgba.as_chunks::<4>().0.contains(&低标记),
        "只开高通道时，画面上不得出现任何低阻挡色"
    );

    // 两通道全开 ⇒ 同格左半低、右半高；逐格核对（不抽样）
    let 全开 =
        try_render_region_with(&map, &src, &vp, &LayerMask::none(), CollisionOverlay::all())
            .unwrap();
    assert_eq!(全开.stats.cells_low_blocked, 直算低);
    assert_eq!(全开.stats.cells_high_blocked, 直算高);
    let 半 = CELL_W / 2;
    let mut 核对格 = 0u32;
    for dx in 0..vp.cells_w {
        for dy in 0..vp.cells_h {
            let Some(cell) = map.cell(dx as u16, dy as u16) else {
                continue;
            };
            let 低 = cell.bk_img & 0x8000 != 0;
            let 高 = cell.fr_img & 0x8000 != 0;
            let (x0, y0) = (dx * CELL_W, dy * CELL_H);
            let 左 = 全开.pixel(x0, y0).unwrap();
            let 右 = 全开.pixel(x0 + CELL_W - 1, y0 + CELL_H - 1).unwrap();
            let 中右 = 全开.pixel(x0 + 半, y0).unwrap();
            match (低, 高) {
                (true, true) => {
                    assert_eq!(左, 低标记, "({dx},{dy}) 左半格应是低阻挡色");
                    assert_eq!(中右, 高标记, "({dx},{dy}) 右半格应是高阻挡色");
                    assert_eq!(右, 高标记);
                }
                (true, false) => {
                    assert_eq!(左, 低标记);
                    assert_eq!(右, 低标记, "({dx},{dy}) 只有低阻挡 ⇒ 整格低色");
                }
                (false, true) => {
                    assert_eq!(左, 高标记, "({dx},{dy}) 只有高阻挡 ⇒ 整格高色");
                    assert_eq!(右, 高标记);
                }
                (false, false) => {
                    assert_eq!(左, [0, 0, 0, 0], "({dx},{dy}) 没有阻挡位 ⇒ 不画");
                    assert_eq!(右, [0, 0, 0, 0]);
                }
            }
            核对格 += 1;
        }
    }
    println!("[门8] 逐格核对 {核对格} 格：左低右高/整格/空白全部符合 ✔");
}

#[test]
fn 验收门9_阻挡可视化出图_0map() {
    let Some(src) = 打开图源() else { return };
    let Some((map, _)) = 读地图(抽样地图[0].1) else {
        return;
    };
    let dir = refs_目录();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("[跳过] 无法创建 {}：{e}", dir.display());
        return;
    }
    let vp = Viewport::new(0, 0, 64, 48);
    let t0 = Instant::now();
    let out = try_render_region_with(&map, &src, &vp, &LayerMask::all(), CollisionOverlay::all())
        .expect("0.map 64×48 叠加渲染应成功");
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    let (直算低, 直算高) = 直算阻挡(&map, &vp);
    println!(
        "[门9] 0.map 64×48（low+high 全开）⇒ {}×{} px，{ms:.1} ms\n      stats: {}\n      直算: 低={直算低} 高={直算高}",
        out.width, out.height, out.stats
    );
    assert_eq!(out.stats.cells_low_blocked, 直算低);
    assert_eq!(out.stats.cells_high_blocked, 直算高);
    let png = dir.join("render-collision-0map.png");
    match out.save_png(&png) {
        Ok(()) => println!("      已导出 {}", png.display()),
        Err(e) => eprintln!("      [错误] 导出失败：{e}"),
    }
}

// =============================================================================
// 验收门 4：与客户端截图的结构级分块比对
//
//   需要外部先出图（本测试不自己拉 Godot，保持「纯逻辑测试」的边界）：
//
//     & '<Godot 可执行文件>' --path '<GodotClient 工程根>' `
//         res://tools/map_preview.tscn -- res://maps/0.map --at=380,310 --shot=<输出 PNG>
//
//   然后：
//     $env:MIREDIT_CLIENT_SHOT='<上面输出的 PNG>'
//     $env:MIREDIT_CLIENT_AT='380,310'
//     cargo test -p miredit-render -- --nocapture
//
//   两种比法都会打印：
//   · A「整图缩放到同尺寸」（任务书字面口径）：本库出的是 160×120 格的 7680×3840，
//     客户端整屏只有 1280×720 ⇒ 覆盖范围差 6 倍，命中率必然低，只作对照；
//   · B「同世界像素矩形 1:1 对齐」（**判定口径**）：按客户端相机算式反推它的世界像素矩形，
//     从本库渲染图里裁出**同一块世界像素**再比 —— 这才是「同区域」的可比口径。
// =============================================================================
#[test]
fn 验收门4_与客户端截图分块比对() {
    let Ok(shot_path) = std::env::var("MIREDIT_CLIENT_SHOT") else {
        eprintln!("[跳过] 未设置 MIREDIT_CLIENT_SHOT（先按上面注释里的命令出客户端截图）");
        return;
    };
    let at = std::env::var("MIREDIT_CLIENT_AT").unwrap_or_else(|_| "380,310".to_string());
    let Some(shot) = 读截图(&shot_path) else {
        return;
    };
    let Some(src) = 打开图源() else { return };
    let Some((map, _)) = 读地图(抽样地图[0].1) else {
        return;
    };

    let vp = Viewport::new(300, 250, 160, 120);
    let t0 = Instant::now();
    let out = render_region(&map, &src, &vp);
    println!(
        "[门4] 本库渲染 {}×{} px（视口 origin=(300,250) cells=160×120），{:.1} ms，stats: {}",
        out.width,
        out.height,
        t0.elapsed().as_secs_f64() * 1000.0,
        out.stats
    );
    println!(
        "[门4] 客户端截图 {}×{} px（--at={at}）",
        shot.width, shot.height
    );

    let 我们 = PngImage {
        width: out.width,
        height: out.height,
        rgba: out.rgba.clone(),
    };
    // 客户端清屏色：取截图左上角像素（该处常有物件，取右下角更稳；这里扫 4 个角取众数）
    let bg = 角底色(&shot);
    println!("[门4] 客户端底色采样（四角众数）= RGB{bg:?}");
    let opts = CompareOptions {
        block: 16,
        threshold: 32,
        background: bg,
    };

    // ---- A：整图缩放到同尺寸（任务书字面口径；覆盖范围差 6 倍，预期低）----
    let ra = compare_blocks(&我们, &shot, &opts);
    println!("[门4][A 整图缩放] {ra}");
    println!(
        "[门4][A 整图缩放] 命中率 {:.2}%（字面口径；客户端整屏只覆盖 160×120 格中的一小块，仅供参考）",
        ra.hit_rate * 100.0
    );

    // ---- B：同世界像素矩形 1:1 对齐（判定口径）----
    let (origin_px_x, origin_px_y) = (vp.origin_x * CELL_W as i32, vp.origin_y * CELL_H as i32);
    let Some((ox, oy, w, h)) = 反推对齐(&out, &shot, &at, origin_px_x, origin_px_y) else {
        eprintln!("[跳过][B] 无法反推客户端世界像素矩形（--at 与截图尺寸不匹配？）");
        return;
    };
    println!("[门4][B 对齐] 客户端世界像素原点 = ({ox},{oy})，尺寸 {w}×{h}；从本库图中裁同一块");
    let crop = 我们.crop((ox - origin_px_x) as u32, (oy - origin_px_y) as u32, w, h);
    let rb = compare_blocks(&crop, &shot, &opts);
    println!("[门4][B 对齐] {rb}");
    println!("[门4][B 对齐] 命中率 {:.2}%", rb.hit_rate * 100.0);
    // 证据落盘：把「同世界像素矩形」的裁剪图存成参考图（供人眼比对）
    let 对照图 = refs_目录().join("render-0map-crop-aligned.png");
    match 我们
        .crop((ox - origin_px_x) as u32, (oy - origin_px_y) as u32, w, h)
        .save_png(&对照图)
    {
        Ok(()) => println!("[门4][B 对齐] 已导出裁剪对照图 {}", 对照图.display()),
        Err(e) => eprintln!("[门4][B 对齐] 裁剪对照图导出失败：{e}"),
    }
    assert!(
        rb.at_least(0.70),
        "同区域对齐后命中率应 ≥70%，实测 {:.2}%（块色差 中位 {} / 最大 {}）",
        rb.hit_rate * 100.0,
        rb.median_delta,
        rb.max_delta
    );
}

/// 读客户端截图；不存在则打印并返回 `None`。
fn 读截图(path: &str) -> Option<PngImage> {
    if !Path::new(path).is_file() {
        eprintln!("[跳过] 截图不存在：{path}");
        return None;
    }
    match read_png(Path::new(path)) {
        Ok(img) => Some(img),
        Err(e) => {
            eprintln!("[跳过] 截图读取失败 {path}：{e}");
            None
        }
    }
}

/// 四角像素取众数当「清屏底色」（客户端整屏渲染，四角通常是没有地形的空处）。
fn 角底色(img: &PngImage) -> [u8; 3] {
    let pts = [
        (0u32, 0u32),
        (img.width - 1, 0),
        (0, img.height - 1),
        (img.width - 1, img.height - 1),
    ];
    let mut seen: Vec<([u8; 3], u32)> = Vec::new();
    for (x, y) in pts {
        if let Some(p) = img.pixel(x, y) {
            let c = [p[0], p[1], p[2]];
            match seen.iter_mut().find(|(k, _)| *k == c) {
                Some((_, n)) => *n += 1,
                None => seen.push((c, 1)),
            }
        }
    }
    seen.into_iter()
        .max_by_key(|(_, n)| *n)
        .map(|(c, _)| c)
        .unwrap_or([0, 0, 0])
}

/// 按 `map_preview.gd` 的相机算式反推客户端截图的**世界像素矩形**（左上原点 + 尺寸）。
///
/// 依据（逐行）：
/// - `map_preview.gd:82`：`center_tile = (_center.x / 2, _center.y / 2)` —— 整数除；
/// - `map_preview.gd:103`：`cam.position = (focus.x*96 + 48, focus.y*64 + 32)`；
/// - 窗口 1280×720（`project.godot:28-29`），`zoom = (1,1)`（`map_preview.gd:104`）
///   ⇒ 屏幕左上角世界像素 = `cam.position - 视口尺寸/2`。
fn 反推对齐(
    rendered: &Rendered,
    shot: &PngImage,
    at: &str,
    origin_px_x: i32,
    origin_px_y: i32,
) -> Option<(i32, i32, u32, u32)> {
    let mut it = at.split(',');
    let ax: i32 = it.next()?.trim().parse().ok()?;
    let ay: i32 = it.next()?.trim().parse().ok()?;
    let fx = ax / 2; // 整数除，与 GDScript 的 `/`（int/int）一致
    let fy = ay / 2;
    let cam_x = fx * 96 + 48;
    let cam_y = fy * 64 + 32;
    let ox = cam_x - shot.width as i32 / 2;
    let oy = cam_y - shot.height as i32 / 2;
    // 换算到「本库渲染图的像素坐标」，必须完全落在图内
    let rx = ox - origin_px_x;
    let ry = oy - origin_px_y;
    if rx < 0 || ry < 0 {
        return None;
    }
    if (rx as u32 + shot.width) > rendered.width || (ry as u32 + shot.height) > rendered.height {
        return None;
    }
    Some((ox, oy, shot.width, shot.height))
}

// =============================================================================
// 验收门 5：性能（debug 冒烟；权威数字见 release 版 render_maps）
// =============================================================================
#[test]
fn 验收门5_性能冒烟_64x48() {
    let Some(src) = 打开图源() else { return };
    let Some((map, _)) = 读地图(抽样地图[0].1) else {
        return;
    };
    let vp = Viewport::new(0, 0, 64, 48);
    let t0 = Instant::now();
    let out = render_region(&map, &src, &vp);
    let 冷 = t0.elapsed().as_secs_f64() * 1000.0;
    let t1 = Instant::now();
    let _ = render_region(&map, &src, &vp);
    let 热 = t1.elapsed().as_secs_f64() * 1000.0;
    println!(
        "[门5] debug（未优化）64×48 冷 {冷:.1} ms / 热 {热:.1} ms（帧解码 {} 次）；\
         release 数字请跑 cargo run --release --bin render_maps",
        out.stats.frames_decoded
    );
}
