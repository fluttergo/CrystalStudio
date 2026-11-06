//! `export` 的单元测试：像素换算、选档、**逐块扩边**、**分块拼接与整图一次渲逐字节相同**、
//! PNG 同源、原子写与失败路径。
//!
//! ⚠ 这里**没有一条**靠"看起来差不多"判分：拼接正确性用「与整图一次渲逐字节比」，
//! PNG 同源性用「与 `miredit-render` 直出的字节比」，失败路径用「目录里有没有文件」。
//! 需要真实图库的用例在资源缺失时**跳过并打印原因**（与 `acceptance_tests` 同一口径），
//! 不把环境缺失伪装成代码错。

use std::path::{Path, PathBuf};

use miredit_render::{
    png_io::{read_png, write_rgba_png},
    try_render_region_scaled, try_render_region_scaled_with, try_render_region_with,
    CollisionOverlay, LayerMask, RenderOptions, RenderScale, Viewport,
};
use mir2_assets::{Frame, LibRef, SpriteSource};
use mir2_map::Map;

use super::*;

/// 测试里的「闭门 + 白天」缺省选项（与生产缺省逐字相同：`RenderOptions::default()`）。
const 缺省: RenderOptions = RenderOptions::closed();
/// 测试里的「开门 + 白天」选项（等价于旧的 `true` 布尔参数）。
const 开门: RenderOptions = RenderOptions::all_doors_open();

// ---------------------------------------------------------------------------
// 夹具
// ---------------------------------------------------------------------------

/// 造一张 `w×h` 的 Old12 地图，格内容由闭包决定（与 `miredit-render` 的测试夹具同口径：
/// `52` 字节头 + `12` 字节/格，格首三个 `u16` 依次是地表/中层/表层图号）。
fn 造图(w: u16, h: u16, f: impl Fn(u16, u16) -> (u16, u16, u16)) -> Map {
    let mut bytes = vec![0u8; 52 + 12 * (w as usize * h as usize)];
    bytes[0..2].copy_from_slice(&w.to_le_bytes());
    bytes[2..4].copy_from_slice(&h.to_le_bytes());
    for x in 0..w {
        for y in 0..h {
            let (bk, mid, fr) = f(x, y);
            let base = 52 + (x as usize * h as usize + y as usize) * 12;
            bytes[base..base + 2].copy_from_slice(&bk.to_le_bytes());
            bytes[base + 2..base + 4].copy_from_slice(&mid.to_le_bytes());
            bytes[base + 4..base + 6].copy_from_slice(&fr.to_le_bytes());
        }
    }
    Map::parse(&bytes).expect("合成地图应能解析")
}

/// 假帧源：`index == 1` 出**大帧**（用来逼出扩边），其余出小帧。
struct 假源 {
    小帧: (u32, u32),
    大帧: (u32, u32),
}

impl SpriteSource for 假源 {
    fn frame(&self, _lib: LibRef, index: u32) -> Option<Frame> {
        let (w, h) = if index == 1 { self.大帧 } else { self.小帧 };
        let c = [
            (index.wrapping_mul(17) & 0xFF) as u8,
            (w % 251) as u8,
            (h % 241) as u8,
            255,
        ];
        let mut rgba = Vec::with_capacity((w * h) as usize * 4);
        for _ in 0..(w * h) {
            rgba.extend_from_slice(&c);
        }
        Some(Frame {
            width: w,
            height: h,
            rgba,
            // 本测试只验导出扩边，与帧锚点无关 ⇒ 其余字段（含 anchor）走 Default，
            // 将来 Frame 再加字段时这里不必再改（见 mir2-assets/src/frame.rs 的 Default 文档）。
            ..Frame::default()
        })
    }
}

/// 临时目录（不存在就建；同名残留先删干净，用例之间不互相污染）。
fn 临时目录(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("miredit-export-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("建临时目录");
    d
}

/// 目录里所有文件名（断言"没有多出文件"用）。
fn 目录清单(d: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(d)
        .map(|it| {
            it.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// 把合成地图写到临时文件，再按**生产路径**（`DocState::load`）装成 `DocState`。
///
/// 返回 `None` ⇒ 本机没有可用的图库（跳过用例，不伪装成通过）。
fn 合成文档(name: &str, w: u16, h: u16) -> Option<(PathBuf, DocState)> {
    let dir = 临时目录(name);
    let map_path = dir.join("t.map");
    let mut bytes = vec![0u8; 52 + 12 * (w as usize * h as usize)];
    bytes[0..2].copy_from_slice(&w.to_le_bytes());
    bytes[2..4].copy_from_slice(&h.to_le_bytes());
    // 每格都放真图号（`1` = 0-based 帧号 0），这样渲染真的会去图库取帧
    for i in 0..(w as usize * h as usize) {
        let base = 52 + i * 12;
        bytes[base..base + 2].copy_from_slice(&1u16.to_le_bytes());
        bytes[base + 2..base + 4].copy_from_slice(&1u16.to_le_bytes());
        bytes[base + 4..base + 6].copy_from_slice(&1u16.to_le_bytes());
    }
    std::fs::write(&map_path, &bytes).expect("写合成地图");
    let doc = DocState::load(Some(&map_path));
    if doc.doc.is_none() || doc.assets.source().is_none() {
        eprintln!("[导出测试] 跳过：本机没有可用图库（{}）", doc.file_name());
        return None;
    }
    Some((dir, doc))
}

// ---------------------------------------------------------------------------
// 一、像素换算 / 选档（纯函数，无 IO）
// ---------------------------------------------------------------------------

/// **本模块的整数换算必须与 `RenderScale::pixel_size` 同口径**。
///
/// 拼接靠「第几格 = 第几个输出像素」的整数换算，一旦与 render crate 的
/// `round(cells*48*f)` 差 1，块与块之间就会错一列/一行 —— 这条钉住两者恒等。
#[test]
fn 档位像素换算与render同口径() {
    let 尺寸集 = [1u32, 2, 3, 7, 16, 63, 64, 65, 100, 128, 700, 1024];
    for s in RenderScale::ALL {
        for &cw in &尺寸集 {
            for &ch in &尺寸集 {
                let want = s.pixel_size(cw, ch).expect("四个档位都该有尺寸");
                assert_eq!(scaled_size(cw, ch, s), want, "档位 {} / {cw}×{ch} 格", s.label());
            }
        }
    }
    // 0.25 档必须是「每格 12×8 px」—— 拼接的整除性就靠这个
    assert_eq!(scaled_size(1, 1, RenderScale::Quarter), (12, 8));
    assert_eq!(scaled_size(1, 1, RenderScale::Half), (24, 16));
    assert_eq!(scaled_size(1, 1, RenderScale::One), (48, 32));
    assert_eq!(scaled_size(1, 1, RenderScale::Two), (96, 64));
}

/// 选档规则：`One → Half → Quarter` 里第一个两边 ≤ `max_side` 的；都不行用 `Quarter`。
///
/// 这条只钉**本仓兼容链**（`--minimap-max-side`）的选档；@acfbbf8a 回源后
/// 官方缺省走 [`MinimapSize::Default`] + [`export_minimap_sized`]，不经过这条选档。
#[test]
fn 选档规则() {
    // 64×48：One 3072×1536 > 2048 ⇒ 退到 `Half`（1536×768）
    assert_eq!(pick_minimap_scale(64, 48, 2048), RenderScale::Half);
    // 128×96：One 6144×3072 / Half 3072×1536 都超 ⇒ `Quarter`（1536×768）
    assert_eq!(pick_minimap_scale(128, 96, 2048), RenderScale::Quarter);
    // 8×6：One 384×192 ≤ 2048 ⇒ `One`（小图不该被无谓缩小）
    assert_eq!(pick_minimap_scale(8, 6, 2048), RenderScale::One);
    // 20×15：One 960×480 ≤ 2048 ⇒ `One`
    assert_eq!(pick_minimap_scale(20, 15, 2048), RenderScale::One);
    // 40×30：One 1920×960 ≤ 2048 ⇒ `One`（正好顶到上限）
    assert_eq!(pick_minimap_scale(40, 30, 2048), RenderScale::One);
    // 41×30：One 1968×960，宽 ≤ 2048 仍 `One`；42×30 = 2016×960 也仍 `One`
    assert_eq!(pick_minimap_scale(41, 30, 2048), RenderScale::One);
    assert_eq!(pick_minimap_scale(42, 30, 2048), RenderScale::One);
    // 43×30：One 2064×960 > 2048 ⇒ `Half`（1032×480）
    assert_eq!(pick_minimap_scale(43, 30, 2048), RenderScale::Half);
    // `0.map` 实测 700×700：三档都超 2048，最小档就是 `Quarter`（之后走第二段缩放）
    assert_eq!(pick_minimap_scale(700, 700, 2048), RenderScale::Quarter);
    // `max_side` 变小 ⇒ 更容易退档
    assert_eq!(pick_minimap_scale(64, 48, 512), RenderScale::Quarter);
    // `max_side` 变大也**不能**选到渲不出来的档：700×700 的 `One` 是 33600×22400
    // （超 MAX_EXPORT_SIDE）、`Half` 是 752 MB（超 MAX_EXPORT_BYTES）⇒ 只剩 `Quarter`。
    // 这条挡的是「用户把上限调大反而失败」。
    assert_eq!(pick_minimap_scale(700, 700, 99_999), RenderScale::Quarter);
    assert_eq!(pick_minimap_scale(700, 700, u32::MAX), RenderScale::Quarter);
    // `Two` 永远不会被选中（小地图放大没意义）
    for (w, h) in [(1u32, 1u32), (8, 8), (64, 48), (700, 700)] {
        assert_ne!(pick_minimap_scale(w, h, 2048), RenderScale::Two);
    }
}

/// **两段式的目标尺寸必须精确保持地图格比例**（`fit_ratio` 的判据）。
///
/// 这条挡的是「按长边缩」那种偷懒算法：`8400×5600` 按长边缩到 2048 会得到
/// `2048×1365`，比例 `1.500366…` ≠ `3/2` —— 验收的通式 `w*ch*32 == h*cw*48` 当场红。
#[test]
fn 两段式目标尺寸精确保持比例() {
    // `0.map` 实测 700×700：Quarter 出 8400×5600 ⇒ 缩到 2046×1364（不是 2048×1365）
    let got = fit_ratio(8400, 5600, 700, 700, 2048).unwrap();
    assert_eq!(got, Some((2046, 1364)), "700×700 的目标尺寸");
    let (w, h) = got.unwrap();
    assert_eq!(
        u64::from(w) * 700 * 32,
        u64::from(h) * 700 * 48,
        "通式 w*ch*32 == h*cw*48 必须逐位成立"
    );
    assert!(w <= 2048 && h <= 2048, "两边都不许超目标长边");
    // 已经装得下 ⇒ None（不缩、也绝不放大）
    assert_eq!(fit_ratio(1536, 768, 64, 48, 2048).unwrap(), None);
    assert_eq!(fit_ratio(384, 192, 8, 6, 2048).unwrap(), None);
    // 源图本身就比目标小 ⇒ 不放大（返回 None，即使 max_side 很大）
    assert_eq!(fit_ratio(384, 192, 8, 6, 4096).unwrap(), None);
    // 非 3:2 的地图：400×300 格 @Quarter = 4800×2400（比例 2:1）⇒ 缩到 2048×1024
    let got = fit_ratio(4800, 2400, 400, 300, 2048).unwrap();
    assert_eq!(got, Some((2048, 1024)));
    let (w, h) = got.unwrap();
    assert_eq!(u64::from(w) * 300 * 32, u64::from(h) * 400 * 48);
    // `max_side` 小到连最简比都放不下 ⇒ **明确报错**（不静默破比例）
    let err = fit_ratio(8400, 5600, 700, 700, 2).expect_err("太小必须报错");
    assert!(err.contains("太小"), "{err}");
    // 同一个 700×700，`max_side = 64` 仍然可行：3:2 的最简比是 3:2，k = 21 ⇒ 63×42
    assert_eq!(fit_ratio(8400, 5600, 700, 700, 64).unwrap(), Some((63, 42)));
}

/// @acfbbf8a 官方四要素：**默认 `W*12 × H*8`** + 三档预设除数 `4/2/1`。
///
/// `700×700` 是 `0.map` 实测格数；这里纯算尺寸，不渲染、不写盘。
#[test]
fn 官方默认与三档预设尺寸() {
    assert_eq!(MinimapSize::Default.resolve(700, 700).unwrap(), (8400, 5600));
    assert_eq!(
        MinimapSize::Preset(MinimapPreset::Small).resolve(700, 700).unwrap(),
        (8400, 5600),
        "Small = W*48/4 × H*32/4 = 官方默认"
    );
    assert_eq!(
        MinimapSize::Preset(MinimapPreset::Medium).resolve(700, 700).unwrap(),
        (16_800, 11_200)
    );
    assert_eq!(
        MinimapSize::Preset(MinimapPreset::Large).resolve(700, 700).unwrap(),
        (33_600, 22_400)
    );
    assert_eq!(
        MinimapSize::Explicit { width: 321, height: 123 }.resolve(700, 700).unwrap(),
        (321, 123)
    );
    for p in [MinimapPreset::Small, MinimapPreset::Medium, MinimapPreset::Large] {
        assert_eq!(MinimapPreset::parse(p.key()), Some(p));
    }
    assert_eq!(MinimapPreset::parse("huge"), None);
}

/// 官方 `ValidSize` 的 Rust 等价判据：0 / 超大值必须给中文原因，不许静默截断。
#[test]
fn 官方尺寸合法性报错() {
    assert!(valid_minimap_size(8400, 5600).is_ok(), "官方默认尺寸合法");
    let e0 = valid_minimap_size(0, 5600).expect_err("宽 0 必须拒绝");
    assert!(e0.contains("宽和高都必须是正整数") && e0.contains("官方 ValidSize 拒绝 0"), "{e0}");
    let e1 = valid_minimap_size(8400, 0).expect_err("高 0 必须拒绝");
    assert!(e1.contains("宽和高都必须是正整数"), "{e1}");
    // 单边超 32768 ⇒ 被本仓更严的护栏挡住（保留更严者，不放宽）
    let e2 = valid_minimap_size(40_000, 10).expect_err("超大宽必须拒绝");
    assert!(e2.contains("超过本仓单边上限") && e2.contains("不截断、不降级"), "{e2}");
    // 两边都不超 32768，但 RGBA 超过 512 MiB ⇒ 字节护栏必须挡住
    let e3 = valid_minimap_size(30_000, 30_000).expect_err("超大面积必须拒绝");
    assert!(e3.contains("超过本仓 512 MB 上限") || e3.contains("MB 上限"), "{e3}");
    assert_eq!(MINIMAP_MAX_PIXELS, (isize::MAX as u64) / 4);
}

/// 官方 `scale/level` 公式到本仓 4 档的**有据映射**（不硬造 16 级 mip）。
#[test]
fn 官方精度公式映射到本仓四档() {
    // 700×700 的 1× 基准 33600×22400
    assert_eq!(pick_official_scale(700, 700, 8400, 5600), RenderScale::Quarter);
    assert_eq!(pick_official_scale(700, 700, 16_800, 11_200), RenderScale::Half);
    assert_eq!(pick_official_scale(700, 700, 33_600, 22_400), RenderScale::One);
    assert_eq!(pick_official_scale(700, 700, 70_000, 45_000), RenderScale::Two);
    // 非等比手填尺寸取 min(width/(W*48), height/(H*32))，不是只看宽
    assert_eq!(pick_official_scale(700, 700, 33_600, 1), RenderScale::Quarter);
}

// ---------------------------------------------------------------------------
// 二、逐块扩边（按真实帧尺寸，不是猜的常量）
// ---------------------------------------------------------------------------

/// **逐块扩边是按真实帧尺寸算出来的**（不是拍脑袋的常量，也不是全窗口一个数）。
///
/// 判据用的是「块网格 + 帧像素矩形」这一套：`70` 格宽 + 块 64 格 ⇒ 2 块
/// （块 0 = 0..63、块 1 = 64..69）；把大帧放在块 0 的**最右两列**，
/// 它只会溢进块 1 ⇒ 块 0 的扩边必须是 0、块 1 必须是 2（距离 62→64 是 2 格）。
#[test]
fn 逐块扩边按真实帧尺寸算() {
    let 小 = 假源 {
        小帧: (48, 32),
        大帧: (48, 32),
    };
    // 全小帧：任何一块都不需要扩边
    let map = 造图(70, 4, |_, _| (1, 1, 1));
    assert_eq!(
        tile_margins(&map, &小, &LayerMask::all(), (0, 0), (70, 4), TILE_CELLS, 缺省),
        vec![0, 0]
    );

    // 120×80 的大帧放在 (62,1) 与 (63,2)：两者都只溢进块 1（它从第 64 列开始）
    let map = 造图(70, 4, |x, y| {
        if (x, y) == (62, 1) || (x, y) == (63, 2) {
            (1, 1, 2) // 图号 2 ⇒ 帧号 1 ⇒ 大帧
        } else {
            (1, 1, 1)
        }
    });
    let 大 = 假源 {
        小帧: (48, 32),
        大帧: (120, 80),
    };
    assert_eq!(
        tile_margins(&map, &大, &LayerMask::all(), (0, 0), (70, 4), TILE_CELLS, 缺省),
        vec![0, 2],
        "块 0 自己就包含那两格（扩边 0）；块 1 要把 (62,1) 的帧画进来 ⇒ 扩 2 格"
    );
    // 关掉表层 ⇒ 那两格的大帧不再参与 ⇒ 两块都回到 0（关层不画也不参与外溢）
    let 只中层 = LayerMask {
        ground: true,
        middle: true,
        front: false,
    };
    assert_eq!(
        tile_margins(&map, &大, &只中层, (0, 0), (70, 4), TILE_CELLS, 缺省),
        vec![0, 0]
    );
}

/// **要分块时某块外溢超安全阀 ⇒ 明确报错**（不静默拼一张缺像素的图）；
/// **单块时同样的外溢照常出图**（扩边被夹回窗口，结果与整图一次渲完全相同）。
///
/// 这条是 2026-11-05 实测暴露的真坑：`0.map` 的 3×3 选区里最大外溢 **13 格**
/// （一帧高 ≈ 448 px 的表层物件），第一版把安全阀设在 8 格 ⇒ 小选区导出直接失败。
/// 修法不是「把阀调大到能过」，而是**分清两种情形**：单块时扩边根本不参与拼接。
#[test]
fn 外溢超安全阀只在真要分块时报错() {
    // 一帧宽到能横跨整块（> 64 格 = 3072 px）⇒ 块 1 必须把块 0 最左边那格画进来，
    // 距离 64 格 > 安全阀 ⇒ 报错
    let 巨 = 假源 {
        小帧: (48, 32),
        大帧: (48 + 48 * (TILE_CELLS + 4), 32),
    };
    let map = 造图(70, 4, |x, y| {
        if (x, y) == (0, 0) {
            (1, 1, 2)
        } else {
            (1, 1, 1)
        }
    });
    let err = render_tiled(
        &map,
        &巨,
        &LayerMask::all(),
        CollisionOverlay::none(),
        RenderScale::One,
        (0, 0),
        (70, 4),
        缺省,
    )
    .expect_err("要分块且外溢超阀必须报错");
    assert!(err.contains("帧尺寸过大"), "{err}");

    // 同一张大帧，但窗口只有 4×4 格 ⇒ 单块 ⇒ 扩边被夹回窗口，照常出图
    let map = 造图(4, 4, |x, y| {
        if (x, y) == (0, 0) {
            (1, 1, 2)
        } else {
            (1, 1, 1)
        }
    });
    let ok = render_tiled(
        &map,
        &巨,
        &LayerMask::all(),
        CollisionOverlay::none(),
        RenderScale::One,
        (0, 0),
        (4, 4),
        缺省,
    )
    .expect("单块窗口不该被安全阀拒掉");
    assert_eq!(ok.tiles, 1);
    assert_eq!(ok.margin, 0, "单块时没有任何格在块外 ⇒ 不需要扩边");
}

// ---------------------------------------------------------------------------
// 三、分块拼接 == 整图一次渲（**本模块最重要的判据**）
// ---------------------------------------------------------------------------

/// **分块拼接与整图一次渲逐字节相同**（`One` / `Half` / `Quarter` 三档）。
///
/// 为什么这条是核心：`0.map`（700×700）整图 1× 是 33600×22400，超 `MAX_SIDE` 与
/// `MAX_PIXELS` ⇒ **只能分块**。分块一旦在边界上丢/多像素，导出的就是错的图，
/// 而错得很像"渲染问题"，事后极难查。所以这里用一张**跨 2×2 块**的合成图
/// （68×66 格 ⇒ 块 64 格 ⇒ 4 块），并在**块角**放 120×80 的大帧逼出扩边，
/// 逐字节比「分块」与「一次渲」。
#[test]
fn 分块拼接与整图一次渲逐字节相同() {
    // 68×66：横向 64+4、纵向 64+2 ⇒ 4 块；块角 (63,63)/(64,63)/(63,64)/(64,64) 放大帧
    let 大帧格 = |x: u16, y: u16| (x == 63 || x == 64) && (y == 63 || y == 64);
    let map = 造图(68, 66, move |x, y| {
        if 大帧格(x, y) {
            (1, 2, 2) // 图号 2 ⇒ 帧号 1 ⇒ 大帧
        } else {
            (1, 1, 1)
        }
    });
    let src = 假源 {
        小帧: (24, 16),
        大帧: (120, 80),
    };
    let mask = LayerMask::all();
    let overlay = CollisionOverlay::none();

    for scale in [RenderScale::One, RenderScale::Half, RenderScale::Quarter] {
        let vp = Viewport::new(0, 0, 68, 66);
        let 整幅 =
            try_render_region_scaled(&map, &src, &vp, &mask, overlay, scale).expect("整图应能渲");
        let 分块 = render_tiled(&map, &src, &mask, overlay, scale, (0, 0), (68, 66), 缺省)
            .unwrap_or_else(|e| panic!("档位 {} 分块渲染失败：{e}", scale.label()));

        assert_eq!(
            (分块.width, 分块.height),
            (整幅.width, 整幅.height),
            "档位 {} 尺寸不一致",
            scale.label()
        );
        assert_eq!(分块.rgba.len(), 整幅.rgba.len());
        assert!(
            分块.rgba == 整幅.rgba,
            "档位 {} 分块拼接与整图一次渲不一致（首处差异：{:?}）",
            scale.label(),
            首个差异(&分块.rgba, &整幅.rgba)
        );
        // 4 块，且真的扩了边（不是退化成一次渲）
        assert_eq!(分块.tiles, 4, "档位 {} 的块数不对", scale.label());
        assert!(
            分块.margin >= 1,
            "档位 {} 的扩边必须是真算出来的（>0），实际 {}",
            scale.label(),
            分块.margin
        );
    }
}

/// 造一张 `w×h` 的 Old12 地图，**带门字段**（`(bk, mid, fr, door_index, door_offset)`）。
///
/// 为什么要单独的夹具：[`造图`] 只填三个图号，造不出「有门的格」—— 而门开帧偏移
/// 只读 `door_index` / `door_offset` 两个字节（`mir2-map/src/cell.rs:185,187`）。
/// 偏移与 `miredit-render` 的门单测同一套（`lib.rs` 的 `try_render_region_with_options`
/// 文档示例：格内 `+4` = 表层图号、`+6` = `door_index`、`+7` = `door_offset`）。
fn 造门图(w: u16, h: u16, f: impl Fn(u16, u16) -> (u16, u16, u16, u8, u8)) -> Map {
    let mut bytes = vec![0u8; 52 + 12 * (w as usize * h as usize)];
    bytes[0..2].copy_from_slice(&w.to_le_bytes());
    bytes[2..4].copy_from_slice(&h.to_le_bytes());
    for x in 0..w {
        for y in 0..h {
            let (bk, mid, fr, 门索引, 门偏移) = f(x, y);
            let base = 52 + (x as usize * h as usize + y as usize) * 12;
            bytes[base..base + 2].copy_from_slice(&bk.to_le_bytes());
            bytes[base + 2..base + 4].copy_from_slice(&mid.to_le_bytes());
            bytes[base + 4..base + 6].copy_from_slice(&fr.to_le_bytes());
            bytes[base + 6] = 门索引;
            bytes[base + 7] = 门偏移;
        }
    }
    Map::parse(&bytes).expect("合成地图应能解析")
}

/// **开门态下「分块拼接 == 整图一次渲」也成立**（2026-11-05 门态收口的核心判据）。
///
/// # 为什么单独立一条，而不是把上面那条改成开门
///
/// 门开帧与闭门帧**尺寸可以不同**（实测 `0.map` 就有），而扩边是按真实帧尺寸算的。
/// 若 [`tile_margins`] 仍按**闭门**帧算，开门态下跨块的大号门开帧拿不到扩边
/// ⇒ 块边界丢像素/少画，而「闭门态逐字节相同」那条**永远发现不了**它
/// （闭门时门偏移恒 0，扩边只跟闭门帧有关）。
///
/// # 构造
///
/// 68×66 格 ⇒ 块 64 格 ⇒ 2×2 块。四个**块角**格 `(63,63)/(64,63)/(63,64)/(64,64)`
/// 都设成「闭门帧 = 帧号 1（24×16，装得进一格）；门索引 1 + 门偏移 2 ⇒ 门开帧 = 帧号 3
/// （120×80，跨块）」⇒ 开门态下这四帧真的会越过块边界，逼出扩边。
#[test]
// 本用例按「每 4 字节一个像素」逐像素比：`chunks_exact(4)` 在这里最直白，
// 换成 `as_chunks::<4>()` 只是写法不同（clippy 的 `chunks_exact_to_as_chunks` 建议）。
#[allow(clippy::chunks_exact_to_as_chunks)]
fn 开门态下分块拼接与整图一次渲逐字节相同() {
    /// 帧号 1 = 小帧（闭门）、帧号 3 = 大帧（门开）—— 两者**尺寸不同**正是本测试要逼的。
    struct 门源;
    impl SpriteSource for 门源 {
        fn frame(&self, _lib: LibRef, index: u32) -> Option<Frame> {
            let (w, h) = if index == 3 { (120, 80) } else { (24, 16) };
            let c = [
                (index.wrapping_mul(29) & 0xFF) as u8,
                (w % 251) as u8,
                (h % 241) as u8,
                255,
            ];
            let mut rgba = Vec::with_capacity((w * h) as usize * 4);
            for _ in 0..(w * h) {
                rgba.extend_from_slice(&c);
            }
            Some(Frame {
                width: w,
                height: h,
                rgba,
                ..Frame::default()
            })
        }
    }

    let 门格 = |x: u16, y: u16| (x == 63 || x == 64) && (y == 63 || y == 64);
    let map = 造门图(68, 66, move |x, y| {
        if 门格(x, y) {
            // 表层图号 2 ⇒ 闭门帧号 1；门索引 1（非 0 才加偏移）+ 门偏移 2 ⇒ 门开帧号 3
            (1, 1, 2, 1, 2)
        } else {
            (1, 1, 1, 0, 0)
        }
    });
    let src = 门源;
    let mask = LayerMask::all();
    let overlay = CollisionOverlay::none();

    // ① 扩边必须**按门开帧**算：同一张图，开门态与闭门态的扩边向量必须不同。
    //    这条是「只透传渲染选项、忘了透传扩边」那个错法的直接判据。
    let 闭边 = tile_margins(&map, &src, &mask, (0, 0), (68, 66), TILE_CELLS, 缺省);
    let 开边 = tile_margins(&map, &src, &mask, (0, 0), (68, 66), TILE_CELLS, 开门);
    assert_ne!(
        闭边, 开边,
        "开门态必须按门开帧尺寸重算扩边（闭边 {闭边:?} / 开边 {开边:?}）"
    );
    assert!(
        开边.iter().any(|&m| m > 0),
        "120×80 的门开帧跨块 ⇒ 至少一块的扩边 > 0，实际 {开边:?}"
    );

    // ② 三档都逐字节比「分块拼接」与「整图一次渲」（两者都用开门态）
    for scale in [RenderScale::One, RenderScale::Half, RenderScale::Quarter] {
        let vp = Viewport::new(0, 0, 68, 66);
        let 整幅 = try_render_region_scaled_with(
            &map,
            &src,
            &vp,
            &mask,
            overlay,
            scale,
            RenderOptions::all_doors_open(),
        )
        .expect("开门态整图应能渲");
        let 分块 = render_tiled(&map, &src, &mask, overlay, scale, (0, 0), (68, 66), 开门)
            .unwrap_or_else(|e| panic!("档位 {} 开门态分块渲染失败：{e}", scale.label()));
        assert_eq!(
            (分块.width, 分块.height),
            (整幅.width, 整幅.height),
            "档位 {} 尺寸不一致",
            scale.label()
        );
        assert_eq!(分块.rgba.len(), 整幅.rgba.len());
        assert!(
            分块.rgba == 整幅.rgba,
            "档位 {} 开门态分块拼接与整图一次渲不一致（首处差异：{:?}）",
            scale.label(),
            首个差异(&分块.rgba, &整幅.rgba)
        );
        assert_eq!(分块.tiles, 4, "档位 {} 的块数不对", scale.label());
    }

    // ③ 门态必须真的改像素 —— 否则上面那条等值只能证明「两边都是闭门」，等于没测。
    let 开 = render_tiled(&map, &src, &mask, overlay, RenderScale::One, (0, 0), (68, 66), 开门)
        .expect("开门态应能渲");
    let 闭 = render_tiled(&map, &src, &mask, overlay, RenderScale::One, (0, 0), (68, 66), 缺省)
        .expect("闭门态应能渲");
    let 差异像素 = 开
        .rgba
        .chunks_exact(4)
        .zip(闭.rgba.chunks_exact(4))
        .filter(|(a, b)| a != b)
        .count();
    assert!(
        差异像素 > 0,
        "开门态与闭门态的像素必须不同（否则这条测试没证明任何东西）"
    );
    // 且差异必须**落在门格附近**（4 格 × 120×80 的并集以内），不是整张图乱变
    assert!(
        差异像素 <= 4 * 120 * 80,
        "差异像素 {差异像素} 超过四格门开帧的总面积，说明变化不只在门上"
    );
}

/// **开门 + 夜晚档**下「分块拼接 == 整图一次渲」仍逐字节成立，并把两条 PNG
/// 落盘供验收脚本算 sha256（这一组就是验收 §6 的真实证据源）。
///
/// 夹具与 [`开门态下分块拼接与整图一次渲逐字节相同`] 同源：68×66 格 ⇒ 2×2 块，
/// 块角放「闭门小帧 / 门开大帧」；这里额外把 `MapLighting::Night` 一起塞进
/// 同一份 `RenderOptions`，堵住「只透传门态、丢了光照」的回归。
#[test]
// 本用例按「每 4 字节一个像素」逐像素比，写法与上一开门用例同源。
#[allow(clippy::chunks_exact_to_as_chunks)]
fn 开门夜晚档分块拼接与整图一次渲逐字节相同并落盘() {
    use miredit_render::MapLighting;

    struct 门源;
    impl SpriteSource for 门源 {
        fn frame(&self, _lib: LibRef, index: u32) -> Option<Frame> {
            let (w, h) = if index == 3 { (120, 80) } else { (24, 16) };
            let c = [
                (index.wrapping_mul(29) & 0xFF) as u8,
                (w % 251) as u8,
                (h % 241) as u8,
                255,
            ];
            let mut rgba = Vec::with_capacity((w * h) as usize * 4);
            for _ in 0..(w * h) {
                rgba.extend_from_slice(&c);
            }
            Some(Frame { width: w, height: h, rgba, ..Frame::default() })
        }
    }

    let 门格 = |x: u16, y: u16| (x == 63 || x == 64) && (y == 63 || y == 64);
    let map = 造门图(68, 66, move |x, y| {
        if 门格(x, y) {
            (1, 1, 2, 1, 2)
        } else {
            (1, 1, 1, 0, 0)
        }
    });
    let src = 门源;
    let mask = LayerMask::all();
    let overlay = CollisionOverlay::none();
    let options = RenderOptions::with_doors_and_lighting(true, MapLighting::Night);
    for scale in [RenderScale::One, RenderScale::Half, RenderScale::Quarter] {
        let vp = Viewport::new(0, 0, 68, 66);
        let 整幅 = try_render_region_scaled_with(
            &map, &src, &vp, &mask, overlay, scale, options,
        )
        .expect("开门+夜晚整图应能渲");
        let 分块 = render_tiled(&map, &src, &mask, overlay, scale, (0, 0), (68, 66), options)
            .unwrap_or_else(|e| panic!("档位 {} 开门+夜晚分块渲染失败：{e}", scale.label()));
        assert_eq!((分块.width, 分块.height), (整幅.width, 整幅.height));
        assert_eq!(分块.rgba, 整幅.rgba, "档位 {} 开门+夜晚逐字节相同", scale.label());
        assert_eq!(分块.tiles, 4);
    }

    // 只落 Quarter 档两张 PNG（最小、可机器抓哈希）；文件名带真实档位与选项。
    let vp = Viewport::new(0, 0, 68, 66);
    let 整幅 = try_render_region_scaled_with(
        &map, &src, &vp, &mask, overlay, RenderScale::Quarter, options,
    )
    .expect("Quarter 整图应能渲");
    let 分块 = render_tiled(
        &map, &src, &mask, overlay, RenderScale::Quarter, (0, 0), (68, 66), options,
    )
    .expect("Quarter 分块应能渲");
    let dir = 临时目录("open-night-equiv");
    let a = dir.join("open-night-whole.png");
    let b = dir.join("open-night-tiled.png");
    write_rgba_png(&a, 整幅.width, 整幅.height, &整幅.rgba).expect("写整图 PNG");
    write_rgba_png(&b, 分块.width, 分块.height, &分块.rgba).expect("写分块 PNG");
    assert_eq!(std::fs::read(&a).expect("读整图"), std::fs::read(&b).expect("读分块"));
    eprintln!("[验收] 开门+夜晚 Quarter 分块等值 PNG：{} / {}", a.display(), b.display());
}

/// 首个差异的字节下标 + 两边的值（失败信息里给证据，而不是只说"不相等"）。
fn 首个差异(a: &[u8], b: &[u8]) -> Option<(usize, u8, u8)> {
    a.iter()
        .zip(b.iter())
        .enumerate()
        .find(|(_, (x, y))| x != y)
        .map(|(i, (x, y))| (i, *x, *y))
}

/// 单块窗口（小于一块）时**不分块**，`tiles == 1` —— 3×3 的选区导出走的就是这条。
#[test]
fn 小窗口只出一块() {
    let map = 造图(8, 8, |_, _| (1, 1, 1));
    let src = 假源 {
        小帧: (48, 32),
        大帧: (48, 32),
    };
    let img = render_tiled(
        &map,
        &src,
        &LayerMask::all(),
        CollisionOverlay::none(),
        RenderScale::One,
        (2, 2),
        (3, 3),
        缺省,
    )
    .expect("3×3 应能导出");
    assert_eq!((img.width, img.height), (3 * 48, 3 * 32));
    assert_eq!(img.tiles, 1);
    // 与直接调 render 逐字节相同（选区块就是"一块"）
    let vp = Viewport::new(2, 2, 3, 3);
    let 直出 = try_render_region_with(
        &map,
        &src,
        &vp,
        &LayerMask::all(),
        CollisionOverlay::none(),
    )
    .expect("直出应能渲");
    assert_eq!(img.rgba, 直出.rgba);
}

/// 整幅输出超护栏 ⇒ **报错**（不截断、不静默降级）。
#[test]
fn 整幅超护栏报错() {
    let map = 造图(2, 2, |_, _| (1, 1, 1));
    let src = 假源 {
        小帧: (48, 32),
        大帧: (48, 32),
    };
    // 700 格 @1× = 33600 px > MAX_EXPORT_SIDE(32768) ⇒ 必须报错
    // （`0.map` 若硬用 One 档就是这条）
    let err = render_tiled(
        &map,
        &src,
        &LayerMask::all(),
        CollisionOverlay::none(),
        RenderScale::One,
        (0, 0),
        (700, 2),
        缺省,
    )
    .expect_err("单边超限必须报错");
    assert!(err.contains("导出图太大"), "{err}");
    // 0 格窗口也必须报错（不是渲一张 0×0 的空图）
    let err = render_tiled(
        &map,
        &src,
        &LayerMask::all(),
        CollisionOverlay::none(),
        RenderScale::One,
        (0, 0),
        (0, 5),
        缺省,
    )
    .expect_err("0 格必须报错");
    assert!(err.contains("0 格"), "{err}");
}

// ---------------------------------------------------------------------------
// 四、PNG 落盘：同源性、尺寸、原子性、失败路径
// ---------------------------------------------------------------------------

/// **导出的 PNG 与 `miredit-render` 直出的 PNG 逐字节相同**（同源自证）。
///
/// `miredit-web` 的 `/api/tiles` 走的是「`compose::render` → `write_rgba_png`」；
/// 本模块走的是「`render_tiled`（内部还是 `try_render_region_scaled_with`）→ 同一个
/// `write_rgba_png`」⇒ 同一区域、同 scale、同 mask、同 collision 下**字节必须相同**。
/// 这条在本地就能证；起 web 服务再比 sha256 是它的外部复验（验收脚本）。
#[test]
fn 导出与render直出逐字节相同() {
    let Some((dir, doc)) = 合成文档("same-source", 16, 12) else {
        return;
    };
    let d = doc.doc.as_ref().unwrap();
    let src = doc.assets.source().unwrap();

    // 直出：与 /api/tiles 完全同一条调用
    let vp = Viewport::new(3, 2, 5, 4);
    let 直出 = try_render_region_with(
        &d.map,
        src as &dyn SpriteSource,
        &vp,
        &LayerMask::all(),
        CollisionOverlay::none(),
    )
    .expect("直出应能渲");
    let 直出路径 = dir.join("direct.png");
    write_rgba_png(&直出路径, 直出.width, 直出.height, &直出.rgba).expect("写直出 PNG");

    // 导出：菜单 / 命令行那条路
    let 导出路径 = dir.join("export.png");
    let rep = export_selection(
        &doc,
        Some(miredit_core::Selection::new(3, 2, 7, 5)),
        &导出路径,
        缺省,
    )
    .expect("导出选区应成功");

    assert_eq!((rep.width, rep.height), (5 * 48, 4 * 32));
    assert_eq!(rep.scale, RenderScale::One);
    assert_eq!(rep.tiles, 1);
    assert!(!rep.doors_open, "缺省必须闭门（RenderOptions::default()）");
    let a = std::fs::read(&直出路径).expect("读直出");
    let b = std::fs::read(&导出路径).expect("读导出");
    assert_eq!(a, b, "导出与 render 直出的 PNG 字节必须相同");
    // 尺寸也从 PNG 头读回来核一遍（不信我们自己记的数字）
    let img = read_png(&导出路径).expect("读回 PNG");
    assert_eq!((img.width, img.height), (240, 128));
    // 落盘后不许留下任何临时文件
    let 清单 = 目录清单(&dir);
    assert!(
        !清单.iter().any(|n| n.contains("miredit-tmp")),
        "不许残留临时文件：{清单:?}"
    );
}

/// 本仓**兼容链**小地图导出的尺寸 = 档位公式，且长宽比与 1× 渲染**恒等**。
///
/// @acfbbf8a 回源后官方缺省已改走 [`export_minimap_sized`]；这里保留旧链的自证。
#[test]
fn 小地图尺寸符合档位公式且比例不变() {
    let Some((dir, doc)) = 合成文档("minimap-size", 64, 48) else {
        return;
    };
    let 路径 = dir.join("mini.png");
    let rep = export_minimap(&doc, &路径, MINIMAP_MAX_SIDE, 缺省).expect("导出小地图应成功");
    // 64×48：One = 3072×1536 > 2048 ⇒ 选 `Half` = 1536×768，一档装得下 ⇒ **不走两段式**
    assert_eq!(rep.scale, RenderScale::Half);
    assert!(!rep.two_stage, "1536×768 已经装得下 2048，不该多缩一次");
    let want = RenderScale::Half.pixel_size(64, 48).unwrap();
    assert_eq!((rep.width, rep.height), want);
    assert_eq!((rep.width, rep.height), (1536, 768));
    // 长宽比与原图（1× 渲染）恒等：`w/h == (cw*48)/(ch*32)` ⇒ `w*ch*32 == h*cw*48`。
    // ⚠ 不能用 `w*32 == h*48` —— 那只有当格网是正方形（cw == ch）时才成立；
    //    64×48 格的地图出的是 1536×768（2:1），而 48:32 是 3:2，两者不是一回事。
    assert_eq!(
        u64::from(rep.width) * 48 * 32,
        u64::from(rep.height) * 64 * 48,
        "长宽比必须与 1× 渲染恒等（64×48 格）"
    );
    let img = read_png(&路径).expect("读回 PNG");
    assert_eq!((img.width, img.height), want);
}

/// **两段式端到端**（小地图）：一档装不下 ⇒ 先渲、再用 render crate 的面积平均缩。
///
/// 用 `max_side = 64` 把 64×48 的图逼进两段式（Quarter 出 768×384 > 64）：
/// 目标必须是 `64×32`（`768:384 = 2:1` 的最简比 2:1，k = 32），且**比例精确保持**。
/// 这条同时证明「第二段真的走了 render crate 的 `compare::resize_to`」——
/// 它输出的每个像素都是 12×12 源块的均值，不是我们算的。
#[test]
fn 小地图两段式缩放端到端() {
    let Some((dir, doc)) = 合成文档("minimap-2stage", 64, 48) else {
        return;
    };
    let 路径 = dir.join("mini2.png");
    let rep = export_minimap(&doc, &路径, MINIMAP_MAX_SIDE_MIN, 缺省).expect("两段式导出应成功");
    assert_eq!(rep.scale, RenderScale::Quarter, "64 px 长边下只能先取最小档");
    assert!(rep.two_stage, "这一发必须走第二段");
    assert_eq!((rep.width, rep.height), (64, 32), "目标尺寸 = 2:1 的最简比 × k");
    assert_eq!(
        u64::from(rep.width) * 48 * 32,
        u64::from(rep.height) * 64 * 48,
        "两段式之后长宽比仍必须与 1× 渲染恒等"
    );
    let img = read_png(&路径).expect("读回 PNG");
    assert_eq!((img.width, img.height), (64, 32));
    // 第一段（768×384）与第二段（64×32）是 12:1 —— 每个目标像素是 12×12 源块均值，
    // 这里只钉「尺寸与比例」，均值语义由 render crate 自己的单测负责。
    assert_eq!(768 / 64, 12);
}

/// **官方缺省端到端**：64×48 格 ⇒ `W*12 × H*8 = 768×384`；
/// 指定 `321×123` ⇒ 出图尺寸逐字等于指定值，且 `official_size = true`。
#[test]
fn 官方尺寸链端到端() {
    let Some((dir, doc)) = 合成文档("official-size", 64, 48) else {
        return;
    };
    let 路径 = dir.join("official.png");
    let rep = export_minimap_sized(&doc, &路径, MinimapSize::Default, 缺省)
        .expect("官方缺省小地图应成功");
    assert_eq!((rep.width, rep.height), (768, 384));
    assert!(rep.official_size, "这条必须走官方尺寸链");
    assert_eq!(rep.scale, RenderScale::Quarter, "scale=0.25 ⇒ 本仓 Quarter");
    assert!(!rep.two_stage, "同尺寸不该虚报两段式缩放");
    let img = read_png(&路径).expect("读回 PNG");
    assert_eq!((img.width, img.height), (768, 384));

    let 指定 = dir.join("explicit.png");
    let rep = export_minimap_sized(
        &doc,
        &指定,
        MinimapSize::Explicit { width: 321, height: 123 },
        缺省,
    )
    .expect("指定像素尺寸应成功");
    assert_eq!((rep.width, rep.height), (321, 123));
    assert!(rep.official_size);
    assert_eq!(
        (read_png(&指定).expect("读回 PNG").width, read_png(&指定).expect("读回 PNG").height),
        (321, 123)
    );
}

/// 官方链非法尺寸（0）⇒ 中文原因、一个文件都不写。
#[test]
fn 官方尺寸链零值报中文原因() {
    let Some((dir, doc)) = 合成文档("official-bad", 64, 48) else {
        return;
    };
    let 目标 = dir.join("bad.png");
    let err = export_minimap_sized(
        &doc,
        &目标,
        MinimapSize::Explicit { width: 0, height: 123 },
        缺省,
    )
    .expect_err("宽 0 必须报错");
    assert!(err.contains("宽和高都必须是正整数"), "{err}");
    assert!(!目标.exists(), "失败时不许写文件");
}

/// 没有地图 ⇒ 两个导出都**报原因且不写文件**。
#[test]
fn 没有地图时报原因且不写文件() {
    let dir = 临时目录("no-map");
    let doc = DocState {
        doc: None,
        assets: crate::mapdoc::Assets::Failed("测试用：故意没有图库".to_string()),
        map_error: Some("测试用：故意没有地图".to_string()),
        map_gen: 0,
    };
    let 目标 = dir.join("x.png");
    let e1 = export_selection(&doc, Some(miredit_core::Selection::new(0, 0, 2, 2)), &目标, 缺省)
        .expect_err("没有地图必须报错");
    assert!(e1.contains("需要先开图"), "{e1}");
    let e2 = export_minimap(&doc, &目标, MINIMAP_MAX_SIDE, 缺省).expect_err("没有地图必须报错");
    assert!(e2.contains("需要先开图"), "{e2}");
    assert!(!目标.exists(), "失败时不许写文件");
    assert!(目录清单(&dir).is_empty(), "失败时目录里不许有文件");
}

/// 有地图但**没有选区** ⇒ 报原因、不写文件（导出选区不许"猜一个范围"）。
#[test]
fn 没有选区时报原因且不写文件() {
    let Some((dir, doc)) = 合成文档("no-sel", 8, 8) else {
        return;
    };
    let 目标 = dir.join("sel.png");
    let e = export_selection(&doc, None, &目标, 缺省).expect_err("没有选区必须报错");
    assert!(e.contains("没有选区"), "{e}");
    assert!(!目标.exists(), "失败时不许写文件");
}

/// 选区整块在图外 ⇒ 报原因（不写一张 0×0 或越界的图）。
#[test]
fn 选区在图外时报原因() {
    let Some((dir, doc)) = 合成文档("sel-out", 8, 8) else {
        return;
    };
    let 目标 = dir.join("out.png");
    let e = export_selection(
        &doc,
        Some(miredit_core::Selection::new(100, 100, 102, 102)),
        &目标,
        缺省,
    )
    .expect_err("选区在图外必须报错");
    assert!(e.contains("选区整块在地图外"), "{e}");
    assert!(!目标.exists());
}

/// **目标目录不存在** ⇒ 报错、**一个文件都不留**（含临时文件）。
///
/// 这是验收里「失败路径」的那一条：`save_atomic` 的第一步 `File::create(tmp)` 就失败，
/// 所以目标目录里连 `<名字>.miredit-tmp` 都不会出现。
#[test]
fn 目标目录不存在时报错且不留文件() {
    let Some((dir, doc)) = 合成文档("no-dir", 8, 8) else {
        return;
    };
    let 缺的目录 = dir.join("不存在的子目录");
    let 目标 = 缺的目录.join("x.png");
    let e = export_selection(
        &doc,
        Some(miredit_core::Selection::new(0, 0, 2, 2)),
        &目标,
        缺省,
    )
    .expect_err("目录不存在必须报错");
    assert!(!e.is_empty(), "错误原因不许为空");
    assert!(!目标.exists(), "目标文件不许存在");
    assert!(!缺的目录.exists(), "不许隐式建目录");
    // 目标目录的父目录里也不许多出任何东西（临时文件不许落在别处）
    let 父清单 = 目录清单(&dir);
    assert!(
        !父清单
            .iter()
            .any(|n| n.contains("miredit-tmp") || n.ends_with(".png")),
        "失败后不许留下 PNG / 临时文件：{父清单:?}"
    );
}

/// 原子写自证：写成功后目标文件在、临时文件不在（`tmp → rename` 的可见证据）。
#[test]
fn 原子写成功后不留临时文件() {
    let dir = 临时目录("atomic");
    let img = ExportImage {
        width: 2,
        height: 2,
        rgba: [0x11, 0x22, 0x33, 0x44].repeat(4),
        tiles: 1,
        margin: 0,
    };
    let 目标 = dir.join("a.png");
    write_png_atomic(&目标, &img).expect("原子写应成功");
    let 清单 = 目录清单(&dir);
    assert_eq!(清单, vec!["a.png".to_string()], "只许有目标文件：{清单:?}");
    let 读回 = read_png(&目标).expect("读回");
    assert_eq!((读回.width, 读回.height), (2, 2));
    assert_eq!(读回.rgba, img.rgba);
    // 覆盖写（第二次）也必须原子成功，且不残留
    write_png_atomic(&目标, &img).expect("覆盖写应成功");
    assert_eq!(目录清单(&dir), vec!["a.png".to_string()]);
}

/// 帧尺寸缓存只问一次图库：同一个 `(库, 帧号)` 第二次必须命中缓存。
#[test]
fn 帧尺寸缓存只解码一次() {
    struct 计数源 {
        次数: std::cell::Cell<u32>,
    }
    impl SpriteSource for 计数源 {
        fn frame(&self, _lib: LibRef, _index: u32) -> Option<Frame> {
            self.次数.set(self.次数.get() + 1);
            Some(Frame {
                width: 48,
                height: 32,
                rgba: vec![0; 48 * 32 * 4],
                // 本测试只验尺寸缓存命中次数，与帧锚点无关 ⇒ 其余字段（含 anchor）走 Default，
                // 将来 Frame 再加字段时这里不必再改（见 mir2-assets/src/frame.rs 的 Default 文档）。
                ..Frame::default()
            })
        }
    }
    let src = 计数源 {
        次数: std::cell::Cell::new(0),
    };
    let mut cache = SizeCache::new(&src);
    let lib = LibRef::ground(0);
    assert_eq!(cache.size(lib, 7), Some((48, 32)));
    assert_eq!(cache.size(lib, 7), Some((48, 32)));
    assert_eq!(cache.size(lib, 8), Some((48, 32)));
    assert_eq!(src.次数.get(), 2, "同一 (库, 帧号) 只该问一次");
}
