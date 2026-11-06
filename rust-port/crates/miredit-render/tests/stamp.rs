//! 印章缩略图验收（任务书 §四）：
//!
//! 1. **合成口径**：地表/中层左上角锚定、物件底边锚定、层序 地表→中层→物件、
//!    层内列优先、图号 `0` 不画、缺帧计入 `missing` 且**不画占位**；
//! 2. **真实印章**：`CrystalStudio/Objects/mir/mir_00000.X`（25 格 / 包围盒 `(-1,-2,4,3)`）
//!    用 `mir2_stamp::Stamp::parse` 读出来适配成 [`PositionedCell`] 再渲染：
//!    原始尺寸、`content` 包围盒、`cells_drawn`、`missing`、**确定性**；
//! 3. **缩略图**：`box_px = Some((46,46))` ⇒ `≤ 46×46`、四角 alpha=0、缩小用非 Nearest 滤镜；
//! 4. **边界**：空 `cells` ⇒ 0×0 空图；单格三图号全 0 ⇒ 不画、`missing` 不增；
//! 5. **性能**：`Objects/mir/` 顺序取前 200 个 `.X`，release 下计时（debug 下只冒烟）；
//! 6. **肉眼检查**：4 枚不同印章的 46×46 缩略图拼一张 PNG 到 `layout/refs/stamp-thumbs.png`。
//!
//! 真实资源缺失时一律「打印原因 + 跳过」，**不 panic**，保证换机器也能 `cargo test` 绿。
//!
//! ⚠ `mir2-stamp` **只在本测试里出现**（`[dev-dependencies]`）：渲染库本体只吃
//! `miredit_render::stamp::PositionedCell`，不认识 `mir2_stamp::Stamp`。

use std::path::{Path, PathBuf};
use std::time::Instant;

use mir2_assets::{Frame, LibRef, SpriteSource, WilSource};
use mir2_stamp::Stamp;
use miredit_render::stamp::{
    render_stamp, FilterClass, PositionedCell, StampImage, StampRenderOptions,
};
use miredit_render::RenderError;

// ---------------------------------------------------------------------------
// 工具
// ---------------------------------------------------------------------------

/// 取证根缺省值：`.X` 的取证来源（**整个文件里只出现这一处**）。换机器用 `MIREDIT_UPSTREAM_ROOT`。
const 缺省取证根: &str = r"I:\miredit\_upstream";

/// 取证根：`MIREDIT_UPSTREAM_ROOT` → 缺省 [`缺省取证根`]；根不存在 ⇒ `None`。
///
/// 只做 `var_os` + `PathBuf`，不引入任何第三方库。
fn 取证根() -> Option<PathBuf> {
    let root = std::env::var_os("MIREDIT_UPSTREAM_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(缺省取证根));
    root.is_dir().then_some(root)
}

/// 印章目录（相对取证根：`Objects/mir`）。缺取证根 ⇒ `None`（调用方打印 `[跳过]` 并返回）。
fn 印章目录() -> Option<PathBuf> {
    let dir = 取证根()?.join("Objects").join("mir");
    dir.is_dir().then_some(dir)
}

/// `layout/refs`（相对本 crate 清单目录）。
fn refs_目录() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("layout")
        .join("refs")
}

/// 按文件名升序取印章目录里的 `.X`（顺序取，确定性）。
fn 印章列表() -> Option<Vec<PathBuf>> {
    let dir = match 印章目录() {
        Some(d) => d,
        None => {
            eprintln!("[跳过] 无取证根 MIREDIT_UPSTREAM_ROOT（或缺 Objects/mir）");
            return None;
        }
    };
    let mut v: Vec<PathBuf> = std::fs::read_dir(&dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .map(|x| x.eq_ignore_ascii_case("X"))
                .unwrap_or(false)
        })
        .collect();
    v.sort();
    if v.is_empty() {
        eprintln!("[跳过] 印章目录里没有 .X：{}", dir.display());
        return None;
    }
    Some(v)
}

/// 打开图源；根目录/图库不可用时打印原因并返回 `None`（调用方跳过，不 panic）。
fn 打开图源() -> Option<WilSource> {
    let root = Path::new(mir2_assets::DEFAULT_CLIENT_ROOT);
    if !root.is_dir() {
        eprintln!("[跳过] 图库根不存在：{}", root.display());
        return None;
    }
    let paths = match mir2_assets::LibPaths::discover(root) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[跳过] 图库扫描失败：{e}");
            return None;
        }
    };
    match WilSource::open(&paths) {
        Ok(s) => Some(s),
        Err(e) => {
            eprintln!("[跳过] 图源打开失败：{e}");
            None
        }
    }
}

/// 库号 `i16` → `PositionedCell` 的 `u8`。
///
/// 真实印章的库号都在 `0..=255`（本机 9,159 个 `.X` 实测），越界说明遇到了本库
/// 表达不了的数据 ⇒ **当场炸测试**，绝不静默截断成另一个库（那会取到别的图 = 假数据）。
fn 库号(v: i16) -> u8 {
    u8::try_from(v).unwrap_or_else(|_| panic!("印章库号 {v} 超出 u8：PositionedCell 表达不了"))
}

/// `mir2_stamp::Stamp` → 本 crate 的 [`PositionedCell`]（**适配在测试侧，渲染库不认识 `.X`**）。
fn 适配(stamp: &Stamp) -> Vec<PositionedCell> {
    stamp
        .cells
        .iter()
        .map(|c| PositionedCell {
            dx: c.x,
            dy: c.y,
            // 低 15 位才是图号（高位是编辑器的阻挡/标记位，见 `mir2-stamp` 模块文档 §五）
            bk: (c.cell.back_image & 0x7FFF) as u16,
            mid: c.cell.middle_image & 0x7FFF,
            fr: c
                .cell
                .foreground
                .map(|f| (f.image as u16) & 0x7FFF)
                .unwrap_or(0),
            bk_lib: 库号(c.cell.back_library),
            mid_lib: 库号(c.cell.middle_library),
            fr_lib: 库号(c.cell.foreground.map(|f| f.library).unwrap_or(0)),
        })
        .collect()
}

/// 读一个 `.X` 并适配；解析失败返回 `None`（打印原因，不 panic）。
fn 读印章(p: &Path) -> Option<(Stamp, Vec<PositionedCell>)> {
    let bytes = match std::fs::read(p) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("[跳过] 读不了 {}：{e}", p.display());
            return None;
        }
    };
    match Stamp::parse(&bytes) {
        Ok(s) => {
            let cells = 适配(&s);
            Some((s, cells))
        }
        Err(e) => {
            eprintln!("[跳过] 解析失败 {}：{e}", p.display());
            None
        }
    }
}

/// 假帧源：按族给确定性颜色，`index % 5 == 4` 的帧故意缺失（用来验缺帧口径）。
struct 假源 {
    /// 各族帧的尺寸（宽, 高）。
    地面: (u32, u32),
    中层: (u32, u32),
    物件: (u32, u32),
}

impl 假源 {
    fn 新() -> Self {
        假源 {
            地面: (96, 64),
            中层: (48, 32),
            物件: (48, 64),
        }
    }
}

impl SpriteSource for 假源 {
    fn frame(&self, lib: LibRef, index: u32) -> Option<Frame> {
        if index % 5 == 4 {
            return None; // 缺帧
        }
        let (w, h, c) = match lib.family {
            mir2_assets::Family::Ground => (self.地面.0, self.地面.1, [20u8, 0, 0, 255]),
            mir2_assets::Family::Middle => (self.中层.0, self.中层.1, [60, 0, 0, 255]),
            mir2_assets::Family::Surface => (self.物件.0, self.物件.1, [100, 0, 0, 255]),
        };
        let mut rgba = Vec::with_capacity((w * h) as usize * 4);
        for _ in 0..(w * h) {
            rgba.extend_from_slice(&c);
        }
        Some(Frame {
            width: w,
            height: h,
            rgba,
            ..mir2_assets::Frame::default()
        })
    }
}

/// 一格（三族都给 1 号图 = 0-based 帧 0）。
fn 一格(dx: i32, dy: i32) -> PositionedCell {
    PositionedCell {
        dx,
        dy,
        bk: 1,
        mid: 1,
        fr: 1,
        bk_lib: 0,
        mid_lib: 0,
        fr_lib: 0,
    }
}

/// **有纹理**的假源：8×8 黑白棋盘 + 按帧号偏色。
///
/// 专门用来验「滤镜真的生效」：纯色图在 Box/Lanczos/Nearest 下结果相同（都是同一个色），
/// 拿纯色图断言 `assert_ne!` 会假绿 —— 这是实测踩过的坑，故单独造一个带结构的源。
struct 纹理源;

impl SpriteSource for 纹理源 {
    fn frame(&self, lib: LibRef, index: u32) -> Option<Frame> {
        let (w, h) = match lib.family {
            mir2_assets::Family::Ground => (96u32, 64u32),
            mir2_assets::Family::Middle => (48, 32),
            mir2_assets::Family::Surface => (48, 64),
        };
        let mut rgba = Vec::with_capacity((w * h) as usize * 4);
        for y in 0..h {
            for x in 0..w {
                let v = if ((x / 8) + (y / 8)) % 2 == 0 { 240u8 } else { 16u8 };
                rgba.extend_from_slice(&[v, (index.wrapping_mul(7) & 0xFF) as u8, 255 - v, 255]);
            }
        }
        Some(Frame {
            width: w,
            height: h,
            rgba,
            ..mir2_assets::Frame::default()
        })
    }
}

/// 全透明图的检查（`StampImage` 里一个非零字节都不该有）。
fn 全透明(img: &StampImage) -> bool {
    img.rgba.iter().all(|b| *b == 0)
}

/// FNV-1a 64 位指纹：只用于把「两次渲染逐字节相等」压成一个可引用的数字。
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

// ---------------------------------------------------------------------------
// 一、合成口径（假源，不依赖磁盘）
// ---------------------------------------------------------------------------

#[test]
fn 落格口径_地表中层左上角_物件底边() {
    // 1 格：地表 96×64 覆盖 2×2 格、中层 48×32、物件 48×64（两格高）
    let cells = [一格(0, 0)];
    let img = render_stamp(&cells, &假源::新(), &StampRenderOptions::raw()).expect("渲染应成功");
    // 画布 = 1 格 = 48×32；超出部分被裁（模块文档 §二.2）
    assert_eq!((img.width, img.height), (48, 32));

    // 地表 96×64 左上角对齐 (0,0)：画布内 (0,0) 起就是地表色（被中层/物件压住前）
    let 只地表 = PositionedCell {
        bk: 1,
        mid: 0,
        fr: 0,
        ..一格(0, 0)
    };
    let img = render_stamp(&[只地表], &假源::新(), &StampRenderOptions::raw()).unwrap();
    assert_eq!(img.pixel(0, 0).unwrap(), [20, 0, 0, 255], "地表左上角锚定");
    assert_eq!(img.pixel(47, 31).unwrap(), [20, 0, 0, 255]);

    // 中层 48×32 左上角对齐 ⇒ 同样铺满整格
    let 只中层 = PositionedCell {
        bk: 0,
        mid: 1,
        fr: 0,
        ..一格(0, 0)
    };
    let img = render_stamp(&[只中层], &假源::新(), &StampRenderOptions::raw()).unwrap();
    assert_eq!(img.pixel(0, 0).unwrap(), [60, 0, 0, 255], "中层左上角锚定");

    // 物件 48×64（两格高）底边钉在格底 ⇒ top = (0+1)*32-64 = -32 ⇒ 画布内只有下半 32 行
    let 只物件 = PositionedCell {
        bk: 0,
        mid: 0,
        fr: 1,
        ..一格(0, 0)
    };
    let img = render_stamp(&[只物件], &假源::新(), &StampRenderOptions::raw()).unwrap();
    assert_eq!(
        img.pixel(0, 31).unwrap(),
        [100, 0, 0, 255],
        "物件底边钉格底：最后一行必须有像素"
    );
    assert_eq!(
        img.content,
        (0, 0, 47, 31),
        "帧高 64 > 格高 32 ⇒ 上半截被裁到画布外，内容框贴满整格"
    );
}

#[test]
fn 物件底边锚点_上边界随帧高变化() {
    // 物件帧 48×16（比格矮）⇒ top = 32-16 = 16，画布 16 行以上必须全透明
    struct 矮源;
    impl SpriteSource for 矮源 {
        fn frame(&self, _lib: LibRef, _index: u32) -> Option<Frame> {
            Some(Frame {
                width: 48,
                height: 16,
                rgba: [9u8, 9, 9, 255].repeat(48 * 16),
                ..mir2_assets::Frame::default()
            })
        }
    }
    let cells = [PositionedCell {
        bk: 0,
        mid: 0,
        fr: 1,
        ..一格(0, 0)
    }];
    let img = render_stamp(&cells, &矮源, &StampRenderOptions::raw()).unwrap();
    assert_eq!(img.content, (0, 16, 47, 31), "帧底边贴格底 y=31");
    assert_eq!(img.pixel(0, 15).unwrap()[3], 0, "帧上方不该有像素");
    assert_eq!(img.pixel(0, 16).unwrap(), [9, 9, 9, 255]);
}

#[test]
fn 层序_地表压中层压物件() {
    // 三层同格：物件在最上（100）> 中层（60）> 地表（20）
    let img = render_stamp(&[一格(0, 0)], &假源::新(), &StampRenderOptions::raw()).unwrap();
    assert_eq!(img.pixel(0, 0).unwrap(), [100, 0, 0, 255], "物件在最上");

    // 去掉物件 ⇒ 中层在最上
    let 两层 = PositionedCell {
        fr: 0,
        ..一格(0, 0)
    };
    let img = render_stamp(&[两层], &假源::新(), &StampRenderOptions::raw()).unwrap();
    assert_eq!(img.pixel(0, 0).unwrap(), [60, 0, 0, 255], "中层压地表");
}

#[test]
fn 层内顺序_列优先_后面的格压前面的() {
    // 两个重叠的物件格：(1,0) 在 (0,0) 右边一格，物件帧 96×64 会横向盖住左边
    // 列优先 ⇒ (0,0) 先画、(1,0) 后画 ⇒ (1,0) 的帧压在 (0,0) 上。
    struct 宽源;
    impl SpriteSource for 宽源 {
        fn frame(&self, _lib: LibRef, index: u32) -> Option<Frame> {
            // 帧号 0 = 红、帧号 1 = 蓝
            let c = if index == 0 {
                [200u8, 0, 0, 255]
            } else {
                [0, 0, 200, 255]
            };
            Some(Frame {
                width: 96,
                height: 64,
                rgba: c.repeat(96 * 64),
                ..mir2_assets::Frame::default()
            })
        }
    }
    // 故意**倒序**给输入（先给右边那格），验证内部按 (dx,dy) 排序而不是按入参顺序
    let cells = [
        PositionedCell {
            fr: 2,
            bk: 0,
            mid: 0,
            ..一格(1, 0)
        },
        PositionedCell {
            fr: 1,
            bk: 0,
            mid: 0,
            ..一格(0, 0)
        },
    ];
    let img = render_stamp(&cells, &宽源, &StampRenderOptions::raw()).unwrap();
    // (1,0) 的物件帧左上角 = (48, 32-64=-32) ⇒ 覆盖 x=48..143、y=-32..31
    // 在画布内 (48..95, 0..31) 是 (1,0) 的帧；它盖住 (0,0) 的帧（0..95, -32..31）右半
    assert_eq!(
        img.pixel(48, 0).unwrap(),
        [0, 0, 200, 255],
        "后画的 (1,0) 必须压住先画的 (0,0)（列优先）"
    );
}

#[test]
fn 缺帧不画且计missing_图号0不计() {
    // 假源对 index % 5 == 4 返回 None。图号 5 ⇒ 0-based 帧 4 ⇒ 缺帧
    let 缺 = PositionedCell {
        bk: 5,
        mid: 5,
        fr: 5,
        ..一格(0, 0)
    };
    let img = render_stamp(&[缺], &假源::新(), &StampRenderOptions::raw()).unwrap();
    assert_eq!(img.missing, 3, "三层各缺一次");
    assert_eq!(img.cells_drawn, 0, "缺帧一格都没画");
    assert!(全透明(&img), "缺帧必须什么都不画（禁洋红/占位）");

    // 图号 0 ⇒ 该层本来没图 ⇒ 不画、也不计 missing
    let 空 = PositionedCell {
        bk: 0,
        mid: 0,
        fr: 0,
        ..一格(0, 0)
    };
    let img = render_stamp(&[空], &假源::新(), &StampRenderOptions::raw()).unwrap();
    assert_eq!(img.missing, 0, "图号 0 不是缺帧");
    assert_eq!(img.cells_drawn, 0);

    // 混合：一格全有 + 一格全缺
    let img = render_stamp(&[一格(0, 0), 缺], &假源::新(), &StampRenderOptions::raw()).unwrap();
    assert_eq!(img.missing, 3);
    assert_eq!(img.cells_drawn, 1, "只有真的画了像素的格才计数");
}

#[test]
fn 印章与地图渲染逐字节同源() {
    // 最强的一条「同源」证据：**同一格**分别走「地图渲染」与「印章渲染」，像素必须逐字节相同。
    //
    // 为什么 1×1 视口能对上：地图的 1×1 视口画布正好是 48×32，且格 (0,0) 是偶偶格
    // （地表照铺）；印章 1 格的包围盒也是 48×32。两边的锚点/帧号/层序/裁剪规则全同 ⇒
    // 输出必须完全一样（这同时证明印章没有另立一套落格口径）。
    const BK: u16 = 3;
    const MID: u16 = 2;
    const FR: u16 = 5;
    const TILES: u8 = 0;
    const SM: u8 = 0;
    const AREA: u8 = 0;

    // New14 档（14 B/格）：前 12 字节共用字段 + 偏移 12/13 的 btTiles/btsmTiles
    // 出处：`mir2-map/src/cell.rs:23-39` 的偏移表
    let mut bytes = vec![0u8; 52 + 14];
    bytes[0..2].copy_from_slice(&1u16.to_le_bytes());
    bytes[2..4].copy_from_slice(&1u16.to_le_bytes());
    let b = 52;
    bytes[b..b + 2].copy_from_slice(&BK.to_le_bytes());
    bytes[b + 2..b + 4].copy_from_slice(&MID.to_le_bytes());
    bytes[b + 4..b + 6].copy_from_slice(&FR.to_le_bytes());
    bytes[b + 10] = AREA; // btArea（偏移 10）
    bytes[b + 12] = TILES; // btTiles（偏移 12）
    bytes[b + 13] = SM; // btsmTiles（偏移 13）
    let map = mir2_map::Map::parse(&bytes).expect("1×1 New14 地图应能解析");

    let src = 假源::新();
    let 地图 = miredit_render::render_region(&map, &src, &miredit_render::Viewport::new(0, 0, 1, 1));
    let 印章 = render_stamp(
        &[PositionedCell {
            dx: 0,
            dy: 0,
            bk: BK,
            mid: MID,
            fr: FR,
            bk_lib: TILES,
            mid_lib: SM,
            fr_lib: AREA,
        }],
        &src,
        &StampRenderOptions::raw(),
    )
    .unwrap();

    assert_eq!(
        (地图.width, 地图.height),
        (印章.width, 印章.height),
        "同一格的画布尺寸必须一致"
    );
    assert_eq!(
        地图.rgba, 印章.rgba,
        "同一格在「地图渲染」与「印章渲染」下必须逐字节相同（口径同源）"
    );
    assert_eq!(地图.stats.cells_drawn as usize, 印章.cells_drawn);
    assert_eq!(地图.stats.frames_decoded as usize, 印章.frames_decoded);
    let 缺 = 地图.stats.missing_ground + 地图.stats.missing_middle + 地图.stats.missing_front;
    assert_eq!(缺 as usize, 印章.missing);
}

#[test]
fn 图号最高位不参与取图() {
    // 0x8001 = 最高位（不可通过）+ 图号 1 ⇒ 取 0-based 帧 0，与图号 1 等价
    let 带位 = PositionedCell {
        bk: 0x8001,
        mid: 0x8001,
        fr: 0x8001,
        ..一格(0, 0)
    };
    let a = render_stamp(&[带位], &假源::新(), &StampRenderOptions::raw()).unwrap();
    let b = render_stamp(&[一格(0, 0)], &假源::新(), &StampRenderOptions::raw()).unwrap();
    assert_eq!(a.rgba, b.rgba, "最高位是阻挡标记，不该改变取到的帧");
}

#[test]
fn 原始尺寸等于格包围盒乘48乘32() {
    // 包围盒 3×2 格（含负坐标）⇒ 144×64
    let cells = [一格(-1, -1), 一格(1, 0)];
    let img = render_stamp(&cells, &假源::新(), &StampRenderOptions::raw()).unwrap();
    assert_eq!((img.width, img.height), (3 * 48, 2 * 32));
    assert_eq!(img.rgba.len(), 144 * 64 * 4);

    // 留白 2 ⇒ 四边各 +2
    let 带留白 = StampRenderOptions {
        margin_px: 2,
        ..StampRenderOptions::raw()
    };
    let img = render_stamp(&cells, &假源::新(), &带留白).unwrap();
    assert_eq!((img.width, img.height), (144 + 4, 64 + 4));
    // 留白区必须全透明（(0,0) 与最右列都在留白里）
    assert_eq!(img.pixel(0, 0).unwrap()[3], 0);
    assert_eq!(img.pixel(1, 1).unwrap()[3], 0);
    assert_eq!(img.pixel(147, 67).unwrap()[3], 0);
}

#[test]
fn 确定性_同输入两次逐字节相等() {
    let cells = [一格(0, 0), 一格(1, 1), 一格(-1, 2)];
    let a = render_stamp(&cells, &假源::新(), &StampRenderOptions::thumbnail(46, 46)).unwrap();
    let b = render_stamp(&cells, &假源::新(), &StampRenderOptions::thumbnail(46, 46)).unwrap();
    assert_eq!(a, b, "同一输入两次渲染必须完全相等");
}

// ---------------------------------------------------------------------------
// 二、缩略图口径
// ---------------------------------------------------------------------------

#[test]
fn 缩略图不超过框且四角透明() {
    // 3×2 格 = 144×64（宽高比 2.25:1）
    let cells = [一格(0, 0), 一格(1, 0), 一格(2, 1)];
    let opts = StampRenderOptions::thumbnail(46, 46);
    let img = render_stamp(&cells, &假源::新(), &opts).unwrap();

    assert!(
        img.width <= 46 && img.height <= 46,
        "缩略图必须装进 46×46，实际 {}×{}",
        img.width,
        img.height
    );
    // 留白 2 px ⇒ 四角必须全透明（缩略图能直接贴在任何底色上）
    for (x, y) in [
        (0, 0),
        (img.width - 1, 0),
        (0, img.height - 1),
        (img.width - 1, img.height - 1),
    ] {
        assert_eq!(img.pixel(x, y).unwrap()[3], 0, "四角 ({x},{y}) 必须透明");
    }
    // 宽高比与内容一致（等比缩放，不拉伸）：允许 ±1 px 的取整误差
    let 输出比 = img.content_width() as f64 / img.content_height() as f64;
    let 内容比 = 144.0 / 64.0;
    assert!(
        (输出比 - 内容比).abs() / 内容比 < 0.12,
        "宽高比被拉伸了：输出 {输出比:.3} vs 内容 {内容比:.3}"
    );
}

#[test]
fn 缩略图缩小用非最近邻滤镜() {
    // ⚠ 必须用**有纹理**的源：纯色图在三种滤镜下结果相同，断言 assert_ne! 会假绿
    let cells: Vec<PositionedCell> = (0..3)
        .flat_map(|y| (0..3).map(move |x| 一格(x, y)))
        .collect();
    let 盒式 = render_stamp(
        &cells,
        &纹理源,
        &StampRenderOptions {
            filter: FilterClass::Box,
            ..StampRenderOptions::thumbnail(46, 46)
        },
    )
    .unwrap();
    let 最近邻 = render_stamp(
        &cells,
        &纹理源,
        &StampRenderOptions {
            filter: FilterClass::Nearest,
            ..StampRenderOptions::thumbnail(46, 46)
        },
    )
    .unwrap();
    // 两种滤镜必须**真的不同**（否则说明 filter 参数没生效 = 假功能）
    assert_ne!(
        盒式.rgba, 最近邻.rgba,
        "Box 与 Nearest 输出相同 ⇒ 滤镜参数没生效"
    );
    // Lanczos 也必须是另一套结果
    let lanczos = render_stamp(
        &cells,
        &纹理源,
        &StampRenderOptions {
            filter: FilterClass::Lanczos,
            ..StampRenderOptions::thumbnail(46, 46)
        },
    )
    .unwrap();
    assert_ne!(lanczos.rgba, 最近邻.rgba, "Lanczos 与 Nearest 输出相同");
    // 三者尺寸必须一致（滤镜只影响像素，不该影响尺寸）
    assert_eq!((盒式.width, 盒式.height), (最近邻.width, 最近邻.height));
    assert_eq!((lanczos.width, lanczos.height), (最近邻.width, 最近邻.height));
}

#[test]
fn 放大强制最近邻() {
    // scale = 2.0（放大）⇒ 无论传 Box 还是 Nearest，输出必须逐字节相同（像素画规则）。
    // 用有纹理的源才有说服力：真按 Box 放大会插值出中间色，那样两者必然不等。
    let cells = [一格(0, 0), 一格(1, 0)];
    let 传盒式 = render_stamp(
        &cells,
        &纹理源,
        &StampRenderOptions {
            scale: 2.0,
            filter: FilterClass::Box,
            margin_px: 0,
            ..StampRenderOptions::raw()
        },
    )
    .unwrap();
    let 传最近邻 = render_stamp(
        &cells,
        &纹理源,
        &StampRenderOptions {
            scale: 2.0,
            filter: FilterClass::Nearest,
            margin_px: 0,
            ..StampRenderOptions::raw()
        },
    )
    .unwrap();
    assert_eq!(传盒式.rgba, 传最近邻.rgba, "放大必须强制 Nearest");
    // 内容 96×32（两格宽、一格高，地表帧 96×64 被画布裁到 32 高）⇒ ×2 = 192×64
    assert_eq!((传盒式.width, 传盒式.height), (192, 64));
}

#[test]
fn 非法选项明确报错不panic() {
    let cells = [一格(0, 0)];
    let src = 假源::新();
    // scale = 0
    let e = render_stamp(
        &cells,
        &src,
        &StampRenderOptions {
            scale: 0.0,
            ..StampRenderOptions::raw()
        },
    )
    .unwrap_err();
    assert!(matches!(e, RenderError::InvalidStampScale { .. }), "{e:?}");
    // scale = NaN
    let e = render_stamp(
        &cells,
        &src,
        &StampRenderOptions {
            scale: f32::NAN,
            ..StampRenderOptions::raw()
        },
    )
    .unwrap_err();
    assert!(matches!(e, RenderError::InvalidStampScale { .. }), "{e:?}");
    // 目标框 0×0
    let e = render_stamp(
        &cells,
        &src,
        &StampRenderOptions {
            box_px: Some((0, 46)),
            ..StampRenderOptions::raw()
        },
    )
    .unwrap_err();
    assert!(matches!(e, RenderError::InvalidStampBox { .. }), "{e:?}");
    // 目标框比留白还小（46×46 留白 24 ⇒ 可用 0）
    let e = render_stamp(
        &cells,
        &src,
        &StampRenderOptions {
            box_px: Some((46, 46)),
            margin_px: 24,
            ..StampRenderOptions::raw()
        },
    )
    .unwrap_err();
    assert!(matches!(e, RenderError::InvalidStampBox { .. }), "{e:?}");
}

// ---------------------------------------------------------------------------
// 三、边界：空输入 / 单格全零
// ---------------------------------------------------------------------------

#[test]
fn 空格集返回空图不panic() {
    let img = render_stamp(&[], &假源::新(), &StampRenderOptions::default()).unwrap();
    assert_eq!((img.width, img.height), (0, 0), "空印章 ⇒ 0×0（不是 Err、不 panic）");
    assert!(img.rgba.is_empty());
    assert_eq!(img.content, (0, 0, 0, 0));
    assert_eq!(img.cells_drawn, 0);
    assert_eq!(img.missing, 0);
    // 缩略图选项下也一样（不因为要缩放就炸）
    let img = render_stamp(&[], &假源::新(), &StampRenderOptions::thumbnail(46, 46)).unwrap();
    assert_eq!((img.width, img.height), (0, 0));
}

#[test]
fn 单格三图号全零不画且missing不增() {
    let 全零 = PositionedCell {
        dx: 0,
        dy: 0,
        bk: 0,
        mid: 0,
        fr: 0,
        bk_lib: 0,
        mid_lib: 0,
        fr_lib: 0,
    };
    let img = render_stamp(&[全零], &假源::新(), &StampRenderOptions::raw()).unwrap();
    assert_eq!(img.missing, 0, "图号 0 不是缺帧");
    assert_eq!(img.cells_drawn, 0);
    assert_eq!((img.width, img.height), (0, 0), "一个像素都没画 ⇒ 空图（模块文档 §五）");
    assert!(img.rgba.is_empty());
}

// ---------------------------------------------------------------------------
// 四、真实印章（`mir_00000.X`，README 范例）
// ---------------------------------------------------------------------------

/// 读 README 范例那枚印章；文件不在就返回 `None`。
fn 读范例() -> Option<(Stamp, Vec<PositionedCell>)> {
    let Some(dir) = 印章目录() else {
        eprintln!("[跳过] 无取证根 MIREDIT_UPSTREAM_ROOT（或缺 Objects/mir）");
        return None;
    };
    let p = dir.join("mir_00000.X");
    if !p.is_file() {
        eprintln!("[跳过] 范例印章不存在：{}", p.display());
        return None;
    }
    读印章(&p)
}

#[test]
fn 真实印章_mir00000_原始尺寸与计数() {
    let Some((stamp, cells)) = 读范例() else {
        return;
    };
    // 与 `mir2-stamp` 模块文档 §七 / README 范例对账（25 格、包围盒 (-1,-2,4,3)）
    assert_eq!(stamp.cells.len(), 25, "README 范例：25 格");
    assert_eq!(stamp.bounds(), (-1, -2, 4, 3), "README 范例：包围盒 (-1,-2,4,3)");

    let Some(src) = 打开图源() else {
        return;
    };
    let opts = StampRenderOptions::raw();
    let img = render_stamp(&cells, &src, &opts).expect("真实印章渲染应成功");

    // 包围盒 6×6 格 ⇒ 288×192
    assert_eq!(
        (img.width, img.height),
        (6 * 48, 6 * 32),
        "6×6 格 ⇒ 288×192"
    );
    assert_eq!(img.rgba.len(), 288 * 192 * 4);
    assert!(img.content_width() > 0 && img.content_height() > 0, "内容框不能为空");
    assert!(img.cells_drawn > 0, "至少有一格真的画了像素");
    assert!(img.missing <= 25 * 3, "missing 不可能超过「格数×3 层」");

    println!(
        "[真实印章] mir_00000.X 格数={} 包围盒={:?} 尺寸={}×{} content={:?} ({}×{}) \
         cells_drawn={} frames_decoded={} missing={}",
        cells.len(),
        stamp.bounds(),
        img.width,
        img.height,
        img.content,
        img.content_width(),
        img.content_height(),
        img.cells_drawn,
        img.frames_decoded,
        img.missing
    );
    // 全零格（三族都没图）不计入 missing，这里只是把数量打出来便于对照
    let 全零格 = cells
        .iter()
        .filter(|c| c.bk == 0 && c.mid == 0 && c.fr == 0)
        .count();
    println!("[真实印章] 三图号全零的格数={全零格}");
}

#[test]
fn 真实印章_mir00000_确定性() {
    let Some((_, cells)) = 读范例() else {
        return;
    };
    let Some(src) = 打开图源() else {
        return;
    };
    let opts = StampRenderOptions::raw();
    let a = render_stamp(&cells, &src, &opts).unwrap();
    let b = render_stamp(&cells, &src, &opts).unwrap();
    assert_eq!(a.rgba, b.rgba, "同一输入两次渲染，RGBA 必须逐字节相等");
    assert_eq!(a, b);
    println!(
        "[确定性] 原始尺寸 {}×{}：两次渲染字节相同 ✔（{} 字节，FNV-1a64 = {:016x}）",
        a.width,
        a.height,
        a.rgba.len(),
        fnv1a(&a.rgba)
    );

    // 缩略图路径同样要确定
    let opts = StampRenderOptions::thumbnail(46, 46);
    let a = render_stamp(&cells, &src, &opts).unwrap();
    let b = render_stamp(&cells, &src, &opts).unwrap();
    assert_eq!(a.rgba, b.rgba, "缩略图两次渲染必须逐字节相等");
    println!(
        "[确定性] 缩略图 {}×{}：两次渲染字节相同 ✔（{} 字节，FNV-1a64 = {:016x}）",
        a.width,
        a.height,
        a.rgba.len(),
        fnv1a(&a.rgba)
    );
}

#[test]
fn 真实印章_mir00000_缩略图46() {
    let Some((_, cells)) = 读范例() else {
        return;
    };
    let Some(src) = 打开图源() else {
        return;
    };
    let img = render_stamp(&cells, &src, &StampRenderOptions::thumbnail(46, 46)).unwrap();

    assert!(
        img.width <= 46 && img.height <= 46,
        "缩略图必须 ≤ 46×46，实际 {}×{}",
        img.width,
        img.height
    );
    for (x, y) in [
        (0, 0),
        (img.width - 1, 0),
        (0, img.height - 1),
        (img.width - 1, img.height - 1),
    ] {
        assert_eq!(img.pixel(x, y).unwrap()[3], 0, "四角 ({x},{y}) 必须 alpha=0");
    }
    // 内容框必须落在成品里，且宽高比与原始内容一致（等比缩放）
    assert!(img.content_width() > 0 && img.content_height() > 0);
    let 原始比 = 288.0 / 192.0;
    let 输出比 = img.content_width() as f64 / img.content_height() as f64;
    assert!(
        (输出比 - 原始比).abs() / 原始比 < 0.25,
        "缩略图宽高比与内容差太多：{输出比:.3} vs {原始比:.3}"
    );

    println!(
        "[缩略图] mir_00000.X 46×46 ⇒ 实际 {}×{} content={:?} ({}×{}) cells_drawn={} missing={}",
        img.width,
        img.height,
        img.content,
        img.content_width(),
        img.content_height(),
        img.cells_drawn,
        img.missing
    );

    // 出图供肉眼检查（`layout/refs/` 已存在；不存在则跳过而不是 panic）
    let dir = refs_目录();
    if dir.is_dir() {
        let p = dir.join("stamp-mir00000-46.png");
        match img.save_png(&p) {
            Ok(()) => println!("[出图] {}", p.display()),
            Err(e) => eprintln!("[跳过出图] {}：{e}", p.display()),
        }
    }
}

// ---------------------------------------------------------------------------
// 五、性能：200 枚真实印章
// ---------------------------------------------------------------------------

#[test]
fn 性能_200枚真实印章() {
    let Some(列表) = 印章列表() else {
        return;
    };
    let Some(src) = 打开图源() else {
        return;
    };
    let 取 = 列表.iter().take(200).collect::<Vec<_>>();
    let 枚数 = 取.len();

    // 先把 200 个文件解析完（解析成本不算进渲染计时）
    let mut 数据: Vec<(PathBuf, Vec<PositionedCell>)> = Vec::with_capacity(枚数);
    for p in &取 {
        if let Some((_, cells)) = 读印章(p) {
            数据.push(((*p).clone(), cells));
        }
    }
    if 数据.is_empty() {
        eprintln!("[跳过] 200 枚里一个都读不出来");
        return;
    }

    let 原尺寸 = StampRenderOptions::raw();
    let 缩略 = StampRenderOptions::thumbnail(46, 46);

    // 预热一遍（让图库页头/调色板进缓存，计时只反映渲染本身）
    let _ = render_stamp(&数据[0].1, &src, &原尺寸);

    let mut 单枚耗时 = Vec::with_capacity(数据.len());
    let mut 空图数 = 0usize;
    let mut 缺帧总数 = 0usize;
    let mut 最大边 = 0u32;
    // 「三图号全零的印章」单独数一遍：空图到底是「本来就没图」还是「帧取不到」，不能混着报
    let mut 全零印章 = 0usize;
    for (_, cells) in &数据 {
        if cells.iter().all(|c| c.bk == 0 && c.mid == 0 && c.fr == 0) {
            全零印章 += 1;
        }
    }
    let 总 = Instant::now();
    for (_, cells) in &数据 {
        let t = Instant::now();
        let img = render_stamp(cells, &src, &原尺寸).expect("真实印章渲染应成功");
        assert!(img.width == 0 || img.rgba.len() == img.width as usize * img.height as usize * 4);
        if img.width == 0 {
            空图数 += 1;
        }
        缺帧总数 += img.missing;
        最大边 = 最大边.max(img.width).max(img.height);
        单枚耗时.push(t.elapsed());
    }
    let 总耗时 = 总.elapsed();

    let mut 缩略耗时 = Vec::with_capacity(数据.len());
    let 缩略总 = Instant::now();
    for (_, cells) in &数据 {
        let t = Instant::now();
        let img = render_stamp(cells, &src, &缩略).expect("缩略图渲染应成功");
        assert!(img.width <= 46 && img.height <= 46);
        缩略耗时.push(t.elapsed());
    }
    let 缩略总耗时 = 缩略总.elapsed();

    let 中位数 = |v: &mut Vec<std::time::Duration>| -> std::time::Duration {
        v.sort();
        v[v.len() / 2]
    };
    let 原中位 = 中位数(&mut 单枚耗时);
    let 缩中位 = 中位数(&mut 缩略耗时);

    println!(
        "[性能] 模式={} 枚数={} 原尺寸总耗时={:?} 中位数={:?} | 缩略图总耗时={:?} 中位数={:?}",
        if cfg!(debug_assertions) { "debug" } else { "release" },
        数据.len(),
        总耗时,
        原中位,
        缩略总耗时,
        缩中位
    );
    println!(
        "[性能] 200 枚里空图（一个像素都没画）={空图数}，其中三图号全零的印章={全零印章}，\
         缺帧合计={缺帧总数}，最大边={最大边}px"
    );

    // 宽松护栏：只拦「数量级错」的回归（debug 下解码慢，给更宽的阈值）
    let 上限 = if cfg!(debug_assertions) {
        std::time::Duration::from_millis(500)
    } else {
        std::time::Duration::from_millis(50)
    };
    assert!(
        原中位 < 上限,
        "单枚原始尺寸渲染中位数 {原中位:?} 超过护栏 {上限:?}"
    );
}

// ---------------------------------------------------------------------------
// 六、肉眼检查：4 枚印章的 46×46 缩略图拼一张 PNG
// ---------------------------------------------------------------------------

#[test]
fn 四枚印章缩略图拼图() {
    let Some(列表) = 印章列表() else {
        return;
    };
    let Some(src) = 打开图源() else {
        return;
    };
    let dir = refs_目录();
    if !dir.is_dir() {
        eprintln!("[跳过] 参考图目录不存在：{}", dir.display());
        return;
    }

    const 边: u32 = 46;
    const 间隔: u32 = 6;
    let 拼宽 = 间隔 + 2 * (边 + 间隔);
    let 拼高 = 拼宽;
    let mut 拼图 = vec![0u8; 拼宽 as usize * 拼高 as usize * 4];

    let mut 成功 = 0usize;
    for (i, p) in 列表.iter().take(4).enumerate() {
        let Some((_, cells)) = 读印章(p) else {
            continue;
        };
        let img = render_stamp(&cells, &src, &StampRenderOptions::thumbnail(边, 边)).unwrap();
        let (ox, oy) = (
            间隔 + (i as u32 % 2) * (边 + 间隔),
            间隔 + (i as u32 / 2) * (边 + 间隔),
        );
        // 逐像素贴（缩略图本来就带 2 px 透明边，直接盖上去即可）
        for y in 0..img.height.min(边) {
            for x in 0..img.width.min(边) {
                let si = (y as usize * img.width as usize + x as usize) * 4;
                let di = ((oy + y) as usize * 拼宽 as usize + (ox + x) as usize) * 4;
                拼图[di..di + 4].copy_from_slice(&img.rgba[si..si + 4]);
            }
        }
        成功 += 1;
    }
    if 成功 == 0 {
        eprintln!("[跳过] 一枚都读不出来");
        return;
    }

    let out = dir.join("stamp-thumbs.png");
    match miredit_render::png_io::write_rgba_png(&out, 拼宽, 拼高, &拼图) {
        Ok(()) => println!("[出图] {}（{拼宽}×{拼高}，{成功} 枚）", out.display()),
        Err(e) => eprintln!("[跳过出图] {}：{e}", out.display()),
    }
}
