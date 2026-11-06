//! **开启门（门开帧偏移）验收门** —— 对照取证报告
//! `deliverables/取证-地图光照与开启门-2026-11-05.md` §2.3/§2.4/§2.6 与客户端
//! `client/reference_full/Classes/scenes/GameScene.cpp:3328-3388`。
//!
//! | # | 测试 | 钉住什么 |
//! |---|---|---|
//! | 1 | [`门帧_默认口径与旧路径逐字节相同`] | `doors_open == false` ⇒ 输出与改动前**逐字节相同**（门字段完全不影响画布）|
//! | 2 | [`门帧_掩码低7位_加6不加134`] | `door_offset = 0x80\|6` 时加的是 **6** 不是 134；`door_index & 0x7F == 0` 时**不加**；门开帧取不到 ⇒ 计 `missing_front` 且**不回退画闭门帧** |
//! | 3 | [`门帧_0map_门格帧号加6`] | 真实 `0.map` 门格 `(318,106)`：`fr_img=1748 / door_index=55 / door_offset=6` ⇒ 帧号 `1747 → 1753`（+6）|
//! | 4 | [`门帧_0map_像素差异`] | 真实图库出图：开关前后**差异像素数 > 0**，并导出两张 PNG 供肉眼对照 |
//! | 5 | [`门帧_地表中层不受影响`] | 只开地表+中层时，`doors_open` 开与关**逐字节相同**（门只在前景层）|
//! | 6 | [`门帧_真实语料_63map_11map`] | 真实语料（`63.map` / `11.map`）里 `area==0` 且 `door_offset` 带 `0x80` 的格 ⇒ 增量 == `door_offset & 0x7F`（=127），**不是整字节 255** |
//! | 7 | [`门帧_缩放档也生效`] | `RenderScale::One` 与 1× 路径逐字节相同；`0.5×` 档同样吃到门偏移 |
//!
//! 真实资源（地图 / 图库）缺失时一律「打印原因 + 跳过」，**不 panic**，保证换机器也能绿。

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use mir2_assets::{Frame, LibPaths, LibRef, SpriteSource, WilSource};
use mir2_map::Map;
use miredit_render::{
    try_render_region, try_render_region_scaled_with, try_render_region_with,
    try_render_region_with_options, CollisionOverlay, LayerMask, RenderOptions, RenderScale,
    Viewport,
};

/// 0.map（客户端版地图，700×700 / 12 B 格）。路径**相对资产根** `MIREDIT_ASSET_ROOT`。
const MAP_0: &str = "GodotClient/maps/0.map";

/// 真实语料：两张「`door_offset` 高位已置位」的图 + 一个「库号 0（`Objects`）且
/// `fr_img != 0`、`door_index` 低 7 位非 0」的格（本次全量复扫定位，坐标见下）。
/// 路径**相对资产根**。
const 语料: [(&str, &str, u16, u16); 2] = [
    (
        "63.map",
        "client/热血传奇23周年客户端/Map/63.map",
        0,
        6,
    ),
    (
        "11.map",
        "client/热血传奇23周年客户端/Map/11.map",
        0,
        13,
    ),
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

// =============================================================================
// 小工具
// =============================================================================

/// 造一张全零合成图，逐格交给 `填` 写 12 字节格体。
fn 合成图(w: u16, h: u16, 填: impl Fn(u16, u16, &mut [u8])) -> Map {
    let mut bytes = vec![0u8; 52 + 12 * (w as usize * h as usize)];
    bytes[0..2].copy_from_slice(&w.to_le_bytes());
    bytes[2..4].copy_from_slice(&h.to_le_bytes());
    for y in 0..h {
        for x in 0..w {
            let base = 52 + 12 * (y as usize * w as usize + x as usize);
            let 格 = &mut bytes[base..base + 12];
            填(x, y, 格);
        }
    }
    Map::parse(&bytes).expect("合成图应能解析")
}

/// 写前景图号（`fr_img`，off 4）。
fn 写前景(格: &mut [u8], fr: u16) {
    格[4..6].copy_from_slice(&fr.to_le_bytes());
}

/// 只开前景层（本文件绝大多数断言只关心「前景层取哪一帧」）。
const 只前景: LayerMask = LayerMask {
    ground: false,
    middle: false,
    front: true,
};

/// 记账源：**永远返回 `None`**（缺帧），但把每次请求的 `(库, 帧号)` 记下来。
///
/// 用它就能在**不依赖磁盘图库**的前提下，精确断言「渲染层到底去取第几帧」——
/// 帧号对不对，不该靠肉眼比 PNG。
#[derive(Default)]
struct 记账源 {
    记录: RefCell<Vec<(LibRef, u32)>>,
}

impl 记账源 {
    fn 请求(&self) -> Vec<(LibRef, u32)> {
        self.记录.borrow().clone()
    }
}

impl SpriteSource for 记账源 {
    fn frame(&self, lib: LibRef, index: u32) -> Option<Frame> {
        self.记录.borrow_mut().push((lib, index));
        None
    }
}

/// 纯色源：任意请求都给一帧 48×32 实心像素，**颜色由帧号编码** ⇒ 换帧必然换像素。
struct 纯色源;

impl SpriteSource for 纯色源 {
    fn frame(&self, _lib: LibRef, index: u32) -> Option<Frame> {
        let 像素 = [
            (index & 0xFF) as u8,
            ((index >> 8) & 0xFF) as u8,
            128,
            255,
        ];
        Some(Frame {
            width: 48,
            height: 32,
            rgba: 像素.repeat(48 * 32),
            ..mir2_assets::Frame::default()
        })
    }
}

/// 图库根（可用环境变量覆盖，默认本机 23 周年客户端）。
fn 图库根() -> PathBuf {
    std::env::var("MIREDIT_CLIENT_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(mir2_assets::DEFAULT_CLIENT_ROOT))
}

/// 打开真实图源；不可用时打印原因并返回 `None`。
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

/// 读并解析一张地图（`相对` = 相对资产根的路径段）；不可用时打印原因并返回 `None`。
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

/// `layout/refs`（相对本 crate 清单目录）。
fn refs_目录() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("layout")
        .join("refs")
}

/// 两张同尺寸 RGBA 的**差异像素数**。
fn 差异像素数(a: &[u8], b: &[u8]) -> usize {
    a.as_chunks::<4>()
        .0
        .iter()
        .zip(b.as_chunks::<4>().0.iter())
        .filter(|(x, y)| x != y)
        .count()
}

/// 关门口径渲染（语法糖，减少重复）。
fn 闭门(
    map: &Map,
    src: &dyn SpriteSource,
    vp: &Viewport,
    mask: &LayerMask,
) -> miredit_render::Rendered {
    try_render_region_with_options(
        map,
        src,
        vp,
        mask,
        CollisionOverlay::none(),
        RenderOptions::closed(),
    )
    .expect("闭门口径渲染应成功")
}

/// 门全开口径渲染。
fn 开门(
    map: &Map,
    src: &dyn SpriteSource,
    vp: &Viewport,
    mask: &LayerMask,
) -> miredit_render::Rendered {
    try_render_region_with_options(
        map,
        src,
        vp,
        mask,
        CollisionOverlay::none(),
        RenderOptions::all_doors_open(),
    )
    .expect("门全开口径渲染应成功")
}

// =============================================================================
// 门 1：默认口径 = 改动前（逐字节）
// =============================================================================
#[test]
fn 门帧_默认口径与旧路径逐字节相同() {
    // ① 合成图：前景有图 + 门字段全非 0（若默认口径有一丝门偏移，这里必然变字节）
    let 带门 = 合成图(3, 2, |x, y, 格| {
        写前景(格, 100 + x + y * 3);
        格[6] = 1 + x as u8; // door_index 低 7 位非 0
        格[7] = 6; // door_offset 低 7 位非 0
        格[10] = 0; // area = 0 ⇒ Objects
    });
    // 同一张图，但把门字段抹成 0（对照组）
    let 无门 = 合成图(3, 2, |x, y, 格| {
        写前景(格, 100 + x + y * 3);
    });
    let vp = Viewport::new(0, 0, 3, 2);
    let src = 纯色源;

    let 旧 = try_render_region(&带门, &src, &vp, &LayerMask::all()).unwrap();
    let 默认 = try_render_region_with_options(
        &带门,
        &src,
        &vp,
        &LayerMask::all(),
        CollisionOverlay::default(),
        RenderOptions::default(),
    )
    .unwrap();
    let 显式关 = 闭门(&带门, &src, &vp, &LayerMask::all());
    let 抹掉门字段 = try_render_region(&无门, &src, &vp, &LayerMask::all()).unwrap();

    assert_eq!(旧.rgba, 默认.rgba, "RenderOptions::default() 必须与旧路径逐字节相同");
    assert_eq!(旧.stats, 默认.stats);
    assert_eq!(旧, 默认);
    assert_eq!(旧.rgba, 显式关.rgba, "RenderOptions::closed() 同上");
    assert_eq!(
        旧.rgba, 抹掉门字段.rgba,
        "doors_open=false 时门字段必须完全不影响画布（连一像素都不许变）"
    );
    println!(
        "[门1] 合成图 3×2（带门字段 vs 抹掉门字段）：逐字节相同 ✔（{} 字节，stats={}）",
        旧.rgba.len(),
        旧.stats
    );

    // ② 真实 0.map 64×48：默认口径与旧入口逐字节相同（需要图库；缺就跳过）
    let Some(真实源) = 打开图源() else { return };
    let Some(map) = 读地图(MAP_0) else { return };
    let vp = Viewport::new(0, 0, 64, 48);
    let 旧 = try_render_region_with(&map, &真实源, &vp, &LayerMask::all(), CollisionOverlay::none())
        .unwrap();
    let 新 = try_render_region_with_options(
        &map,
        &真实源,
        &vp,
        &LayerMask::all(),
        CollisionOverlay::none(),
        RenderOptions::default(),
    )
    .unwrap();
    assert_eq!(旧.rgba, 新.rgba, "0.map 64×48：默认口径必须与旧入口逐字节相同");
    assert_eq!(旧.stats, 新.stats);
    assert_eq!(旧, 新);
    println!(
        "[门1] 0.map 64×48 默认口径：逐字节相同 ✔（{} 字节，stats={}）",
        旧.rgba.len(),
        旧.stats
    );
}

// =============================================================================
// 门 2：`& 0x7F` 掩码（合成图，不依赖磁盘）
// =============================================================================
#[test]
fn 门帧_掩码低7位_加6不加134() {
    let vp = Viewport::new(0, 0, 1, 1);

    // ---- ① door_offset = 0x80|6 = 134、door_index = 1 ⇒ 加的是 6 ----
    let 门 = 合成图(1, 1, |_, _, 格| {
        写前景(格, 1000); // 闭门帧 = 999
        格[6] = 1; // door_index 低 7 位非 0
        格[7] = 0x80 | 6; // 134：高位（运行期已开）+ 低 7 位 6
    });
    let 闭源 = 记账源::default();
    let 闭出 = 闭门(&门, &闭源, &vp, &只前景);
    assert_eq!(闭源.请求(), vec![(LibRef::surface(0), 999)]);

    let 开源 = 记账源::default();
    let 开出 = 开门(&门, &开源, &vp, &只前景);
    assert_eq!(
        开源.请求(),
        vec![(LibRef::surface(0), 1005)],
        "应加 6（134 & 0x7F），得到 999 + 6 = 1005"
    );
    assert_ne!(
        开源.请求(),
        vec![(LibRef::surface(0), 1133)],
        "整字节相加会得到 999 + 134 = 1133 —— 那是编辑器 MapPreview.cs 的偏差，本库不许跟"
    );
    // 门开帧取不到（记账源返回 None）⇒ 按缺帧口径计数，且**不回退去画闭门帧**
    assert_eq!(开出.stats.missing_front, 1);
    assert_eq!(闭出.stats.missing_front, 1);
    assert!(
        开出.rgba.iter().all(|b| *b == 0),
        "门开帧取不到时不许画任何东西（更不许回退画闭门帧）"
    );
    println!("[门2] door_offset=0x86 / door_index=1 ⇒ 999 → 1005（+6，不是 +134）✔");

    // ---- ② door_index == 0 ⇒ 不加（即便 door_offset 非 0）----
    let 无门索引 = 合成图(1, 1, |_, _, 格| {
        写前景(格, 1000);
        格[6] = 0; // 没有门
        格[7] = 0x80 | 6;
    });
    let 源 = 记账源::default();
    开门(&无门索引, &源, &vp, &只前景);
    assert_eq!(
        源.请求(),
        vec![(LibRef::surface(0), 999)],
        "door_index == 0 的格不受门开关影响"
    );

    // ---- ③ door_index 只有高位（0x80，低 7 位 0）⇒ 也不加 ----
    let 索引仅高位 = 合成图(1, 1, |_, _, 格| {
        写前景(格, 1000);
        格[6] = 0x80;
        格[7] = 6;
    });
    let 源 = 记账源::default();
    开门(&索引仅高位, &源, &vp, &只前景);
    assert_eq!(
        源.请求(),
        vec![(LibRef::surface(0), 999)],
        "判据是 (door_index & 0x7F) != 0，只有 0x80 位不算有门"
    );
    println!("[门2] door_index=0 / 0x80 ⇒ 均不加偏移（帧号仍 999）✔");
}

// =============================================================================
// 门 3：真实 0.map 门格 ⇒ 帧号 +6（不依赖图库，只读地图字节）
// =============================================================================
#[test]
fn 门帧_0map_门格帧号加6() {
    let Some(map) = 读地图(MAP_0) else { return };
    // 取证报告 §2.4⑤ 点名的门格之一：(318,106) fr_img=1748 / door_index=55 / door_offset=6
    let 格 = map.cell(106, 318).expect("0.map 应含 (106,318)");
    assert_eq!(格.fr_img, 1748, "取证报告：该门格前景图号 1748");
    assert_eq!(格.door_index, 55, "门索引低 7 位非 0");
    assert_eq!(格.door_offset, 6, "门偏移 6（无高位）");
    assert_eq!(格.area, 0, "库号 0 = Objects");

    let vp = Viewport::new(106, 318, 1, 1);
    let 闭源 = 记账源::default();
    闭门(&map, &闭源, &vp, &只前景);
    let 开源 = 记账源::default();
    开门(&map, &开源, &vp, &只前景);

    let 闭帧 = 闭源.请求();
    let 开帧 = 开源.请求();
    assert_eq!(闭帧, vec![(LibRef::surface(0), 1747)], "闭门帧 = 1748 - 1 = 1747");
    assert_eq!(开帧, vec![(LibRef::surface(0), 1753)], "开门帧 = 1747 + 6 = 1753");
    assert_eq!(1753 - 1747, 6);
    println!(
        "[门3] 0.map (106,318)：fr_img={} door_index={} door_offset={} ⇒ 闭门帧 {} → 开门帧 {}（+{}）✔",
        格.fr_img,
        格.door_index,
        格.door_offset,
        闭帧[0].1,
        开帧[0].1,
        开帧[0].1 - 闭帧[0].1
    );
}

// =============================================================================
// 门 4：真实图库出图 ⇒ 差异像素数 > 0（+ 导出两张 PNG）
// =============================================================================
#[test]
fn 门帧_0map_像素差异() {
    let Some(src) = 打开图源() else { return };
    let Some(map) = 读地图(MAP_0) else { return };
    // 覆盖 (106..108, 316..318) 这组门格（列优先坐标；行优先写法是 (315..318,106..108)）
    let vp = Viewport::new(106, 316, 3, 3);
    let 闭 = 闭门(&map, &src, &vp, &LayerMask::all());
    let 开 = 开门(&map, &src, &vp, &LayerMask::all());
    assert_eq!((闭.width, 闭.height), (开.width, 开.height));
    let 差 = 差异像素数(&闭.rgba, &开.rgba);
    assert!(差 > 0, "开启门必须真的换掉像素（否则偏移没生效）");
    // 门格只占一小片 ⇒ 差异不该铺满整幅（防「整屏乱画」这种反向错误）
    let 总 = (闭.width as usize) * (闭.height as usize);
    assert!(差 < 总, "差异像素数不该等于总像素数");

    let dir = refs_目录();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("[跳过导出] 无法创建 {}：{e}", dir.display());
    } else {
        for (名, out) in [
            ("render-doors-closed-0map.png", &闭),
            ("render-doors-open-0map.png", &开),
        ] {
            let p = dir.join(名);
            match out.save_png(&p) {
                Ok(()) => println!("        已导出 {}", p.display()),
                Err(e) => eprintln!("        [错误] 导出失败：{e}"),
            }
        }
    }
    println!(
        "[门4] 0.map (106,316)+3×3：闭门 vs 开门 差异像素 {差} / {总}（{:.2}%），stats 闭={} 开={}",
        差 as f64 * 100.0 / 总 as f64,
        闭.stats,
        开.stats
    );
}

// =============================================================================
// 门 5：地表 / 中层不受影响
// =============================================================================
#[test]
fn 门帧_地表中层不受影响() {
    // 2×2：三层都有图号，门字段也点着
    let map = 合成图(2, 2, |_, _, 格| {
        格[0..2].copy_from_slice(&1u16.to_le_bytes()); // bk_img
        格[2..4].copy_from_slice(&1u16.to_le_bytes()); // mid_img
        写前景(格, 1);
        格[6] = 1;
        格[7] = 6;
    });
    let vp = Viewport::new(0, 0, 2, 2);
    let 只地表中层 = LayerMask {
        ground: true,
        middle: true,
        front: false,
    };

    let 闭源 = 记账源::default();
    let 闭 = 闭门(&map, &闭源, &vp, &只地表中层);
    let 开源 = 记账源::default();
    let 开 = 开门(&map, &开源, &vp, &只地表中层);

    assert_eq!(闭源.请求(), 开源.请求(), "地表/中层取帧序列必须完全一致");
    assert_eq!(闭.rgba, 开.rgba, "只画地表+中层时，门开关一个像素都不该改");
    assert_eq!(闭.stats, 开.stats);
    assert_eq!(闭, 开);

    // 对照组：把前景也打开，就必须有差异（否则上一条断言是「假通过」）
    let 全闭 = 闭门(&map, &纯色源, &vp, &LayerMask::all());
    let 全开 = 开门(&map, &纯色源, &vp, &LayerMask::all());
    assert!(
        差异像素数(&全闭.rgba, &全开.rgba) > 0,
        "同一张图上开前景层后必须出现差异（证明门偏移确实只作用于前景层）"
    );
    println!(
        "[门5] 2×2 三层全有图：只地表中层 闭/开 逐字节相同 ✔；加前景层后差异 {} 像素",
        差异像素数(&全闭.rgba, &全开.rgba)
    );
}

// =============================================================================
// 门 6：真实语料（63.map / 11.map 的 area==0 且 door_offset 带 0x80 的格）
// =============================================================================
#[test]
fn 门帧_真实语料_63map_11map() {
    let vp1 = Viewport::new(0, 0, 1, 1);
    let mut 核对 = 0u32;
    for (名, 路径, x, y) in 语料 {
        let Some(map) = 读地图(路径) else { continue };
        let Some(格) = map.cell(x, y) else {
            eprintln!("[跳过] {名} 不含格 ({x},{y})");
            continue;
        };
        // 先钉住「这一格确实是取证报告说的那类格」
        assert_ne!(格.door_offset & 0x80, 0, "{名} ({x},{y})：door_offset 高位应已置位");
        assert_eq!(格.area, 0, "{名} ({x},{y})：库号应为 0（Objects，真实存在）");
        assert_ne!(格.door_index & 0x7F, 0, "{名} ({x},{y})：门索引低 7 位应非 0");
        assert_ne!(格.fr_img & 0x7FFF, 0, "{名} ({x},{y})：应有前景图（真的会画）");

        let 闭帧 = u32::from(格.fr_img & 0x7FFF) - 1;
        let 掩码增量 = u32::from(格.door_offset & 0x7F);
        let 整字节增量 = u32::from(格.door_offset);

        let vp = Viewport::new(i32::from(x), i32::from(y), vp1.cells_w, vp1.cells_h);
        let 闭源 = 记账源::default();
        闭门(&map, &闭源, &vp, &只前景);
        let 开源 = 记账源::default();
        开门(&map, &开源, &vp, &只前景);

        assert_eq!(闭源.请求(), vec![(LibRef::surface(0), 闭帧)], "{名}：闭门帧");
        assert_eq!(
            开源.请求(),
            vec![(LibRef::surface(0), 闭帧 + 掩码增量)],
            "{名}：开门帧 = 闭门帧 + (door_offset & 0x7F)"
        );
        assert_ne!(
            掩码增量, 整字节增量,
            "{名}：该格 door_offset 高位已置位 ⇒ 掩码增量必须小于整字节增量（否则本测试失去意义）"
        );
        println!(
            "[门6] {名} ({x},{y})：fr_img={}（闭门帧 {}）door_index={} door_offset={:#04x} ⇒ 开门帧 {}（+{}，整字节会 +{}）✔",
            格.fr_img,
            闭帧,
            格.door_index,
            格.door_offset,
            闭帧 + 掩码增量,
            掩码增量,
            整字节增量
        );
        核对 += 1;
    }
    if 核对 == 0 {
        eprintln!("[跳过] 两张语料图一张都没读到");
    }
}

// =============================================================================
// 门 7：缩放档也吃到门偏移
// =============================================================================
#[test]
fn 门帧_缩放档也生效() {
    // 2×2，只有 (1,1) 是门格
    let map = 合成图(2, 2, |x, y, 格| {
        写前景(格, 1000);
        if x == 1 && y == 1 {
            格[6] = 1;
            格[7] = 6;
        }
    });
    let vp = Viewport::new(0, 0, 2, 2);
    let src = 纯色源;

    // ① One 档与 1× 路径逐字节相同（含门偏移）
    let 一 = try_render_region_scaled_with(
        &map,
        &src,
        &vp,
        &LayerMask::all(),
        CollisionOverlay::none(),
        RenderScale::One,
        RenderOptions::all_doors_open(),
    )
    .unwrap();
    let 直 = 开门(&map, &src, &vp, &LayerMask::all());
    assert_eq!(一.rgba, 直.rgba, "One 档必须直接转调 1× 路径");
    assert_eq!(一.stats, 直.stats);
    assert_eq!(一.scale, RenderScale::One);

    // ② 0.5× 档：门开与门关必须出不同的图
    let 半闭 = try_render_region_scaled_with(
        &map,
        &src,
        &vp,
        &LayerMask::all(),
        CollisionOverlay::none(),
        RenderScale::Half,
        RenderOptions::closed(),
    )
    .unwrap();
    let 半开 = try_render_region_scaled_with(
        &map,
        &src,
        &vp,
        &LayerMask::all(),
        CollisionOverlay::none(),
        RenderScale::Half,
        RenderOptions::all_doors_open(),
    )
    .unwrap();
    assert_eq!((半闭.width, 半闭.height), (半开.width, 半开.height));
    let 差 = 差异像素数(&半闭.rgba, &半开.rgba);
    assert!(差 > 0, "0.5× 档也必须吃到门偏移");
    println!("[门7] One 档与 1× 逐字节相同 ✔；0.5× 档 闭/开 差异 {差} 像素 ✔");
}
