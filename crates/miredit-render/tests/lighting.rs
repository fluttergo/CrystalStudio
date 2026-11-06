//! 地图光照通道验收（2026-11-05）：**默认档逐字节不变** + **三档差异** + **逐格光罩真的加光**
//! + **光罩位置用帧锚点**（`Frame::anchor` = 官方 `sprite.X/Y`）。
//!
//! 两类测试：
//!
//! 1. **合成数据**（不依赖磁盘）：用假图源造「一束光 + 满屏白块」，把「遮罩压暗 / 光罩减 alpha /
//!    物件层关掉只压暗 / 光心位移 == 锚点 / 锚点为零时逐字节不变 / 开门不移动光罩」六条口径钉死，
//!    换机器也一定能跑；
//! 2. **真实数据**（`0.map` + 真实图库）：钉住 `RenderOptions::default()` 的 FNV 基线
//!    `0xa924aa9dedc74183`（= 引入光照前的逐字节指纹），并挑取证报告点名的发光格
//!    （`0.map (265,261)`，以及最坏 561 px 的 `hf001.map`）做锚点 A/B 对照。
//!    资源缺失时**打印原因并跳过**（与 `tests/scale.rs:81-101` 同款；本机有资源 ⇒ 真跑）。

use std::path::{Path, PathBuf};

use mir2_assets::{Frame, LibPaths, LibRef, SpriteSource, WilSource};
use mir2_map::Map;
use miredit_render::{
    emits_light, light_bounds, try_render_region_with_options, CollisionOverlay, LayerMask,
    MapLighting, RenderOptions, Rendered, Viewport,
};

/// 引入光照通道**之前**的 `0.map` 64×48 1× 输出 FNV-1a64（见 `deliverables/审查-今夜新增代码-2026-11-05.md`）。
const 改前基线FNV: u64 = 0xa924_aa9d_edc7_4183;

/// 抽样地图（**相对资产根** `MIREDIT_ASSET_ROOT`，不是猜的）。
const 真实地图: &str = "GodotClient/maps/0.map";

/// 取证报告 §5.3 点名的「最坏 561 px」地图（在客户端原目录，不在 `GodotClient/maps`）。
const 最坏地图: &str = "client/热血传奇23周年客户端/Map/hf001.map";

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
// 工具
// ---------------------------------------------------------------------------

/// 打开真实图源；失败返回 `None`（打印原因，不 panic）。
fn 打开图源() -> Option<WilSource> {
    let root = std::env::var("MIREDIT_CLIENT_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(mir2_assets::DEFAULT_CLIENT_ROOT));
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
fn 读地图(相对: &str) -> Option<(Vec<u8>, Map)> {
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
        Ok(m) => Some((bytes, m)),
        Err(e) => {
            eprintln!("[跳过] 解析失败 {}：{e}", p.display());
            None
        }
    }
}

/// FNV-1a 64 位指纹（与 `tests/scale.rs` / `tests/stamp.rs` 同款）。
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 整幅平均亮度（RGB 三通道算术平均，0..=255 的 f64）。
fn 平均亮度(img: &Rendered) -> f64 {
    let mut 和 = 0u64;
    for px in img.rgba.chunks(4) {
        和 += u64::from(px[0]) + u64::from(px[1]) + u64::from(px[2]);
    }
    let n = (img.rgba.len() / 4).max(1) as f64;
    和 as f64 / (n * 3.0)
}

/// 单像素亮度（RGB 平均）。
fn 亮度(px: [u8; 4]) -> f64 {
    (f64::from(px[0]) + f64::from(px[1]) + f64::from(px[2])) / 3.0
}

/// 一束光的**中心像素**（世界像素坐标 → 画布像素坐标；`(0,0)` 为视口左上角）。
fn 光心(vp: &Viewport, x: i32, y: i32, light: u8) -> Option<(u32, u32)> {
    光心_带偏移(vp, x, y, light, (0, 0))
}

/// 同上，但**显式给锚点偏移**（= 官方 `sprite.X/Y`，本库 `Frame::anchor`）。
///
/// 光心 = 光罩左上角 + 贴图尺寸/2（尺寸不随偏移变 ⇒ 整张光罩平移、光心跟着平移）。
fn 光心_带偏移(
    vp: &Viewport,
    x: i32,
    y: i32,
    light: u8,
    offset: (i32, i32),
) -> Option<(u32, u32)> {
    let (左, 上, 宽, 高) = light_bounds(x, y, light, offset)?;
    let cx = 左 + 宽 as i32 / 2 - vp.origin_x * miredit_render::CELL_W as i32;
    let cy = 上 + 高 as i32 / 2 - vp.origin_y * miredit_render::CELL_H as i32;
    let (w, h) = vp.pixel_size()?;
    if cx < 0 || cy < 0 || cx as u32 >= w || cy as u32 >= h {
        return None; // 光心在视口外，采不到
    }
    Some((cx as u32, cy as u32))
}

/// 假图源：任何库、任何帧号都返回一块**不透明纯白** `w×h`。
struct 白块源 {
    w: u32,
    h: u32,
}

impl SpriteSource for 白块源 {
    fn frame(&self, _lib: LibRef, _index: u32) -> Option<Frame> {
        Some(Frame {
            width: self.w,
            height: self.h,
            rgba: [255, 255, 255, 255].repeat((self.w * self.h) as usize),
            ..mir2_assets::Frame::default()
        })
    }
}

/// 该格的**光罩偏移**：与生产代码同口径（`lighting::叠加光源` 里的那三行）。
///
/// 帧号 = 静态基准帧 `(fr_img & 0x7FFF) - 1`（**不叠门偏移**）；库 = `LibRef::surface(area)`；
/// 取不到帧 ⇒ `(0, 0)`（官方 `Assets.Get` 失败时 `offset` 保持零）。
///
/// 测试里刻意**另写一份**而不是调用生产函数：若两边写成同一份实现，锚点接错也测不出来。
fn 锚点_取(src: &dyn SpriteSource, cell: &mir2_map::Cell) -> (i32, i32) {
    let 图号 = cell.fr_img & 0x7FFF;
    if 图号 == 0 {
        return (0, 0);
    }
    let 帧号 = u32::from(图号 - 1);
    src.anchor(LibRef::surface(u16::from(cell.area)), 帧号)
        .map_or((0, 0), |(ax, ay)| (i32::from(ax), i32::from(ay)))
}

/// 假图源：像素恒为不透明纯白 `w×h`；**锚点按「库 + 帧号」查表**（未列出的帧取 `(0, 0)`）。
///
/// 用它把「光罩位置是否真的用了锚点」变成可判定的事实：像素恒定 ⇒ 画面差异只可能来自光罩位移。
struct 带锚点源 {
    w: u32,
    h: u32,
    锚点: Vec<(LibRef, u32, (i16, i16))>,
}

impl 带锚点源 {
    fn new(w: u32, h: u32, 锚点: Vec<(LibRef, u32, (i16, i16))>) -> Self {
        Self { w, h, 锚点 }
    }
}

impl SpriteSource for 带锚点源 {
    fn frame(&self, _lib: LibRef, _index: u32) -> Option<Frame> {
        Some(Frame {
            width: self.w,
            height: self.h,
            rgba: [255, 255, 255, 255].repeat((self.w * self.h) as usize),
            ..mir2_assets::Frame::default()
        })
    }

    fn anchor(&self, lib: LibRef, index: u32) -> Option<(i16, i16)> {
        Some(
            self.锚点
                .iter()
                .find(|(l, i, _)| *l == lib && *i == index)
                .map_or((0, 0), |(_, _, a)| *a),
        )
    }
}

/// 「**改前行为**」的包装源：像素与锚点查询一律转发，但 `anchor()` 恒 `(0, 0)`。
///
/// 改动前光罩偏移写死 `(0,0)`；用它当 A/B 的「改前」一侧，就能在**同一份真实数据**上
/// 直接比出「补锚点前后差多少像素」，而不必去翻旧二进制。
struct 零锚点源<'a>(&'a dyn SpriteSource);

impl SpriteSource for 零锚点源<'_> {
    fn frame(&self, lib: LibRef, index: u32) -> Option<Frame> {
        self.0.frame(lib, index)
    }

    fn anchor(&self, _lib: LibRef, _index: u32) -> Option<(i16, i16)> {
        Some((0, 0))
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
            bytes[base + 11] = cell.light;
        }
    }
    Map::parse(&bytes).expect("合成地图应能解析")
}

/// 把某格的 `light` 字节改成 `值`，重新解析（A/B 对照用；`light` 恒在格内偏移 11）。
fn 改光照(bytes: &[u8], map: &Map, x: u16, y: u16, 值: u8) -> Map {
    let cs = map.format.cell_size();
    let mut b = bytes.to_vec();
    let at = 52 + (x as usize * map.height as usize + y as usize) * cs + 11;
    b[at] = 值;
    Map::parse(&b).expect("改 light 后仍应能解析")
}

// ---------------------------------------------------------------------------
// 一、合成数据（不依赖磁盘）
// ---------------------------------------------------------------------------

/// 16×12 格、满屏白块、**只有 (0,0) 发光**（`light = 9`，最大光罩 925×703）。
///
/// 为什么把光源放角落、地图开到 16×12：`light=9` 的光罩矩形是
/// `[-436, 489) × [-310, 393)`（[`light_bounds`]），画布必须**有一部分落在光罩外**
/// 才能同时验「光心亮」与「光罩外纯黑」。
fn 合成场景() -> (Map, 白块源, Viewport) {
    let map = 造图(16, 12, |x, y| {
        let mut c = mir2_map::Cell {
            fr_img: 1, // 每格都有前景图 ⇒ 底层一定有不透明白块
            ..Default::default()
        };
        if (x, y) == (0, 0) {
            c.light = 9;
        }
        c
    });
    let src = 白块源 { w: 48, h: 32 };
    (map, src, Viewport::new(0, 0, 16, 12))
}

/// 光罩外的采样点（在 16×12 = 768×384 画布内，且 `x >= 489` ⇒ 任何光罩都够不到）。
const 远处点: (u32, u32) = (600, 300);

fn 渲染(map: &Map, src: &dyn SpriteSource, vp: &Viewport, 档: MapLighting) -> Rendered {
    渲染选项(map, src, vp, RenderOptions::with_lighting(档))
}

/// 同上，但显式给整份选项（要同时动「开启门」与「光照」时用）。
fn 渲染选项(map: &Map, src: &dyn SpriteSource, vp: &Viewport, 选项: RenderOptions) -> Rendered {
    try_render_region_with_options(
        map,
        src,
        vp,
        &LayerMask::all(),
        CollisionOverlay::none(),
        选项,
    )
    .expect("合成视口应能渲染")
}

#[test]
fn 合成_白天档一个像素都不压暗() {
    let (map, src, vp) = 合成场景();
    let 白天 = 渲染(&map, &src, &vp, MapLighting::Day);
    // 满屏白块 ⇒ 全白不透明；白天档不该有任何压暗
    for px in 白天.rgba.chunks(4) {
        assert_eq!(px, [255, 255, 255, 255], "白天档必须原样保留");
    }
    // 且与「默认 RenderOptions」逐字节相同
    let 默认 = try_render_region_with_options(
        &map,
        &src,
        &vp,
        &LayerMask::all(),
        CollisionOverlay::none(),
        RenderOptions::default(),
    )
    .expect("默认档应能渲染");
    assert_eq!(白天.rgba, 默认.rgba, "默认档就是白天档");
}

#[test]
fn 合成_夜晚档压黑且光心更亮() {
    let (map, src, vp) = 合成场景();
    let 夜 = 渲染(&map, &src, &vp, MapLighting::Night);
    let (cx, cy) = 光心(&vp, 0, 0, 9).expect("光心应落在视口内");

    let 光心像素 = 夜.pixel(cx, cy).expect("光心像素");
    let 远处像素 = 夜.pixel(远处点.0, 远处点.1).expect("远处像素");
    println!(
        "[合成] 夜晚档：光心 ({cx},{cy}) = {光心像素:?} / 远处 {远处点:?} = {远处像素:?} / 整幅平均亮度 {:.1}",
        平均亮度(&夜)
    );

    // 光罩够不到的地方：遮罩 alpha 255 ⇒ 全黑
    assert_eq!(远处像素, [0, 0, 0, 255], "夜晚档光罩外必须是纯黑");
    // 光心：遮罩被减掉 ⇒ 白色露出来（明显亮）
    assert!(
        亮度(光心像素) > 200.0,
        "光心应几乎全亮，实得 {光心像素:?}"
    );
    assert!(
        亮度(光心像素) > 亮度(远处像素) + 100.0,
        "光心必须明显亮于光罩外"
    );
    assert!(
        亮度(光心像素) > 平均亮度(&夜),
        "光心必须亮于整幅平均亮度"
    );
}

#[test]
fn 合成_三档亮度严格递增() {
    let (map, src, vp) = 合成场景();
    let 白天 = 平均亮度(&渲染(&map, &src, &vp, MapLighting::Day));
    let 黄昏 = 平均亮度(&渲染(&map, &src, &vp, MapLighting::DawnDusk));
    let 夜 = 平均亮度(&渲染(&map, &src, &vp, MapLighting::Night));
    println!("[合成] 平均亮度：白天 {白天:.1} / 黎明黄昏 {黄昏:.1} / 夜晚 {夜:.1}");
    assert!(白天 > 黄昏, "白天必须亮于黎明黄昏");
    assert!(黄昏 > 夜, "黎明黄昏必须亮于夜晚");
}

#[test]
fn 合成_物件层关掉时只压暗不加光() {
    let (map, src, vp) = 合成场景();
    let (cx, cy) = 光心(&vp, 0, 0, 9).expect("光心应落在视口内");
    // 关掉物件层：官方 `Presentation.cs:73` 的 `if (VisibleLayers[2])` ⇒ 只画遮罩、不叠光
    let 无物件层 = try_render_region_with_options(
        &map,
        &src,
        &vp,
        &LayerMask {
            ground: true,
            middle: true,
            front: false,
        },
        CollisionOverlay::none(),
        RenderOptions::with_lighting(MapLighting::Night),
    )
    .expect("应能渲染");
    assert_eq!(
        无物件层.pixel(cx, cy),
        Some([0, 0, 0, 255]),
        "物件层关掉 ⇒ 光心也必须被压黑（光只从物件层取）"
    );
}

#[test]
fn 合成_黎明黄昏是中间档() {
    let (map, src, vp) = 合成场景();
    let 黄昏 = 渲染(&map, &src, &vp, MapLighting::DawnDusk);
    let 远处 = 黄昏.pixel(远处点.0, 远处点.1).expect("远处像素");
    // 白色 255 被 alpha=205 的黑色压过：255*(50/255) ≈ 50
    assert!(
        (45..=55).contains(&远处[0]),
        "黎明黄昏档远处应约剩 50（实得 {:?}）",
        远处
    );
    assert_eq!(远处[3], 255, "不透明目标压暗后仍不透明");
}

#[test]
fn 合成_无光源时夜晚只剩黑遮罩() {
    // 一格都不发光（light 全 0）：夜晚档整幅必须是纯黑不透明
    let map = 造图(4, 4, |_, _| mir2_map::Cell {
        fr_img: 1,
        ..Default::default()
    });
    let src = 白块源 { w: 48, h: 32 };
    let vp = Viewport::new(0, 0, 4, 4);
    let 夜 = 渲染(&map, &src, &vp, MapLighting::Night);
    for px in 夜.rgba.chunks(4) {
        assert_eq!(px, [0, 0, 0, 255], "没有光源 ⇒ 整幅纯黑");
    }
}

#[test]
fn 合成_light为10不发光() {
    // 官方 `EmitsLight` 是 `> 0 and < 10` ⇒ 10 不发光（数据里 10 不是合法光照档）
    let map = 造图(8, 6, |x, y| {
        let mut c = mir2_map::Cell {
            fr_img: 1,
            ..Default::default()
        };
        if (x, y) == (0, 0) {
            c.light = 10;
        }
        c
    });
    let src = 白块源 { w: 48, h: 32 };
    let vp = Viewport::new(0, 0, 8, 6);
    let 夜 = 渲染(&map, &src, &vp, MapLighting::Night);
    let (cx, cy) = 光心(&vp, 0, 0, 9).expect("几何位置仍可算");
    assert_eq!(
        夜.pixel(cx, cy),
        Some([0, 0, 0, 255]),
        "light=10 不该发光"
    );
}

#[test]
fn 合成_没有前景图的格子不发光() {
    // light=9 但 fr_img=0 ⇒ 官方 `EmitsLight` 不成立
    let map = 造图(8, 6, |x, y| {
        let mut c = mir2_map::Cell::default();
        if (x, y) == (0, 0) {
            c.light = 9;
        }
        c
    });
    let src = 白块源 { w: 48, h: 32 };
    let vp = Viewport::new(0, 0, 8, 6);
    let 夜 = 渲染(&map, &src, &vp, MapLighting::Night);
    let (cx, cy) = 光心(&vp, 0, 0, 9).expect("几何位置仍可算");
    assert_eq!(
        夜.pixel(cx, cy),
        Some([0, 0, 0, 255]),
        "没有前景图 ⇒ 不发光"
    );
}

#[test]
fn 合成_视口外的光源会外溢进来() {
    // 官方遍历整份文档的光源（`Presentation.cs:73`）⇒ 视口外的光源也会外溢进视口。
    // 本库用「视口外扩 16 格」等价实现（`lighting::扫描外扩格数`），本测试钉住它真的生效。
    //
    // 光源格 (5,5) 在视口 (0,10,20×10) **上方 5 格**（图内、但不在视口内）；
    // `light=9` 的光罩矩形左/上 = `(240-436, 160-310) = (-196, -150)`，贴图 925×703
    // ⇒ 下沿到 y=553，视口世界 y 从 320 起 ⇒ 画布 y 0..233 落在光罩里。
    let map = 造图(40, 30, |x, y| {
        let mut c = mir2_map::Cell {
            fr_img: 1,
            ..Default::default()
        };
        if (x, y) == (5, 5) {
            c.light = 9;
        }
        c
    });
    let 无光图 = 造图(40, 30, |_, _| mir2_map::Cell {
        fr_img: 1,
        ..Default::default()
    });
    let src = 白块源 { w: 48, h: 32 };
    let vp = Viewport::new(0, 10, 20, 10);

    let 有光 = 渲染(&map, &src, &vp, MapLighting::Night);
    let 无光 = 渲染(&无光图, &src, &vp, MapLighting::Night);

    let (px, py) = (266u32, 100u32); // 光罩中心列，视口内、离光源格 5 格
    let a = 有光.pixel(px, py).expect("有光像素");
    let b = 无光.pixel(px, py).expect("无光像素");
    println!("[合成] 视口外光源外溢：({px},{py}) 有光 {a:?} / 无光 {b:?}");
    assert_eq!(b, [0, 0, 0, 255], "没有光源时该处应纯黑");
    assert!(
        亮度(a) > 亮度(b) + 5.0,
        "视口外的光源必须外溢进视口：{a:?} vs {b:?}"
    );
}

// ---------------------------------------------------------------------------
// 二、锚点（`Frame::anchor` → 光罩位置）—— 合成数据
// ---------------------------------------------------------------------------

/// 造一张 `w×h` 的 Old12 地图：**每格都有前景图**（⇒ 整幅不透明白块），
/// 只在 `(sx, sy)` 那格设光照 / 指定帧号 / 可选的门字段。
///
/// 为什么满格前景图：光罩只改 alpha，压在**不透明**像素上才体现为「变亮」；
/// 空白格上压黑只会得到 RGB 恒 0 的黑块，看不出光心在哪。
fn 造单光源图(
    w: u16,
    h: u16,
    sx: u16,
    sy: u16,
    light: u8,
    fr_img: u16,
    门: Option<(u8, u8)>,
) -> Map {
    let mut bytes = vec![0u8; 52 + 12 * (w as usize * h as usize)];
    bytes[0..2].copy_from_slice(&w.to_le_bytes());
    bytes[2..4].copy_from_slice(&h.to_le_bytes());
    for x in 0..w {
        for y in 0..h {
            let base = 52 + (x as usize * h as usize + y as usize) * 12;
            bytes[base + 4..base + 6].copy_from_slice(&1u16.to_le_bytes()); // 每格都有前景图
        }
    }
    let base = 52 + (sx as usize * h as usize + sy as usize) * 12;
    bytes[base + 4..base + 6].copy_from_slice(&fr_img.to_le_bytes());
    if let Some((索引, 偏移)) = 门 {
        bytes[base + 6] = 索引;
        bytes[base + 7] = 偏移;
    }
    bytes[base + 11] = light;
    Map::parse(&bytes).expect("合成地图应能解析")
}

/// 锚点 `(7, -44)` 是取证报告里最常见的非零锚点（725 个发光格，占 29.2%）。
const 常用锚点: (i16, i16) = (7, -44);

#[test]
fn 合成_光罩光心位移等于帧锚点() {
    let (sx, sy) = (2u16, 2u16);
    let map = 造单光源图(16, 12, sx, sy, 9, 1, None);
    let vp = Viewport::new(0, 0, 16, 12);
    let src = 带锚点源::new(48, 32, vec![(LibRef::surface(0), 0, 常用锚点)]);

    let 改后 = 渲染(&map, &src, &vp, MapLighting::Night);
    let 改前 = 渲染(&map, &零锚点源(&src), &vp, MapLighting::Night);

    let 旧光心 = 光心(&vp, i32::from(sx), i32::from(sy), 9).expect("旧光心应在视口内");
    let 新光心 = 光心_带偏移(
        &vp,
        i32::from(sx),
        i32::from(sy),
        9,
        (i32::from(常用锚点.0), i32::from(常用锚点.1)),
    )
    .expect("新光心应在视口内");
    let 位移 = (
        新光心.0 as i64 - 旧光心.0 as i64,
        新光心.1 as i64 - 旧光心.1 as i64,
    );
    println!(
        "[合成] 光心：改前 {旧光心:?} → 改后 {新光心:?}（位移 {位移:?}，帧锚点 {常用锚点:?}）"
    );
    assert_eq!(
        位移,
        (i64::from(常用锚点.0), i64::from(常用锚点.1)),
        "光心位移必须**逐像素等于**该帧的 anchor"
    );
    assert_ne!(改后.rgba, 改前.rgba, "锚点非零 ⇒ 光罩位置必须真的变了");

    // 光罩真的移过去了：新光心处变亮、旧光心处变暗（同一张图、只差锚点）
    let 新处_改后 = 亮度(改后.pixel(新光心.0, 新光心.1).expect("像素"));
    let 新处_改前 = 亮度(改前.pixel(新光心.0, 新光心.1).expect("像素"));
    let 旧处_改后 = 亮度(改后.pixel(旧光心.0, 旧光心.1).expect("像素"));
    let 旧处_改前 = 亮度(改前.pixel(旧光心.0, 旧光心.1).expect("像素"));
    println!(
        "[合成] 亮度：新光心 {新处_改前:.1} → {新处_改后:.1}；旧光心 {旧处_改前:.1} → {旧处_改后:.1}"
    );
    assert!(
        新处_改后 > 新处_改前,
        "新光心应更亮：改后 {新处_改后:.1} vs 改前 {新处_改前:.1}"
    );
    assert!(
        旧处_改前 > 旧处_改后,
        "旧光心应变暗：改前 {旧处_改前:.1} vs 改后 {旧处_改后:.1}"
    );
}

#[test]
fn 合成_锚点为零时与改前逐字节相同() {
    // 51.0% 的发光格锚点恰为 (0,0) ⇒ 这些格**必须一个像素都不变**（回归红线）。
    let (sx, sy) = (2u16, 2u16);
    let map = 造单光源图(16, 12, sx, sy, 9, 1, None);
    let vp = Viewport::new(0, 0, 16, 12);
    let src = 带锚点源::new(48, 32, vec![(LibRef::surface(0), 0, (0, 0))]);

    let 改后 = 渲染(&map, &src, &vp, MapLighting::Night);
    let 改前 = 渲染(&map, &零锚点源(&src), &vp, MapLighting::Night);
    let 光心_零 = 光心(&vp, i32::from(sx), i32::from(sy), 9).expect("光心应在视口内");
    let 光心_带零 = 光心_带偏移(&vp, i32::from(sx), i32::from(sy), 9, (0, 0)).expect("同上");
    println!("[合成] 锚点 (0,0)：光心 改前 {光心_零:?} / 改后 {光心_带零:?}（应完全相同）");
    assert_eq!(光心_零, 光心_带零, "锚点 (0,0) 时光心坐标必须一模一样");
    assert_eq!(
        改后.rgba, 改前.rgba,
        "锚点 (0,0) ⇒ 光罩位置不变 ⇒ 整幅必须逐字节相同"
    );
}

#[test]
fn 合成_开门不移动光罩() {
    // ★ 官方光罩用**静态基准帧**的 `sprite.X/Y`（`(fr_img & 0x7FFF) - 1`），
    //   不含门偏移（`Presentation.cs:84-86`）。若光罩沿用了前景层的「当前帧号」（含门偏移），
    //   开门时光罩会跟着跳到门帧的锚点上。
    //
    // 判据设计：假源对**任何帧号**都返回同一块纯白像素 ⇒ 门开只可能通过「锚点」影响画面。
    // 故「开门前后逐字节相同」就等价于「光罩用的是静态基准帧」。
    let (sx, sy) = (2u16, 2u16);
    let map = 造单光源图(16, 12, sx, sy, 9, 1, Some((1, 5))); // 帧号 0，门索引 1，门偏移 5
    let vp = Viewport::new(0, 0, 16, 12);
    let src = 带锚点源::new(
        48,
        32,
        vec![
            (LibRef::surface(0), 0, 常用锚点),      // 静态基准帧
            (LibRef::surface(0), 5, (200, -150)), // 门帧（锚点故意夸张，用了就会明显偏）
        ],
    );

    let 闭 = 渲染选项(&map, &src, &vp, RenderOptions::with_doors_and_lighting(false, MapLighting::Night));
    let 开 = 渲染选项(&map, &src, &vp, RenderOptions::with_doors_and_lighting(true, MapLighting::Night));
    println!(
        "[合成] 开门：闭门 fnv {:016x} / 开门 fnv {:016x}（必须相同）",
        fnv1a(&闭.rgba),
        fnv1a(&开.rgba)
    );
    assert_eq!(
        开.rgba, 闭.rgba,
        "开门不得移动光罩：光罩取静态基准帧的锚点，与门偏移无关"
    );

    // 反向对照：证明这条用例**能**发现光罩位移（把基准帧锚点换成那个夸张值就该红）
    let 对照源 = 带锚点源::new(48, 32, vec![(LibRef::surface(0), 0, (200, -150))]);
    let 对照 = 渲染选项(
        &map,
        &对照源,
        &vp,
        RenderOptions::with_doors_and_lighting(false, MapLighting::Night),
    );
    assert_ne!(
        对照.rgba, 闭.rgba,
        "对照：锚点换成 (200,-150) 后画面必须不同（否则本用例对位移不敏感）"
    );
}

// ---------------------------------------------------------------------------
// 三、真实数据（0.map 64×48 + 真实图库）
// ---------------------------------------------------------------------------

#[test]
fn 真实_默认档逐字节不变_0map_64x48() {
    let Some((_, map)) = 读地图(真实地图) else { return };
    let Some(src) = 打开图源() else { return };
    let vp = Viewport::new(0, 0, 64, 48);
    let 图 = try_render_region_with_options(
        &map,
        &src,
        &vp,
        &LayerMask::all(),
        CollisionOverlay::none(),
        RenderOptions::default(),
    )
    .expect("0.map 64×48 应能渲染");

    let 指纹 = fnv1a(&图.rgba);
    println!(
        "[真实] 0.map 64×48 默认档：{}×{} / {} 字节 / fnv1a {:016x}（改前基线 {:016x}）/ stats {}",
        图.width,
        图.height,
        图.rgba.len(),
        指纹,
        改前基线FNV,
        图.stats
    );
    assert_eq!((图.width, 图.height), (3072, 1536));
    assert_eq!(
        指纹, 改前基线FNV,
        "默认档（白天 + 闭门）必须与引入光照前的输出**逐字节相同**"
    );
}

#[test]
fn 真实_三档互不相同且夜晚最暗() {
    let Some((_, map)) = 读地图(真实地图) else { return };
    let Some(src) = 打开图源() else { return };
    let vp = Viewport::new(0, 0, 64, 48);

    let 白天 = 渲染(&map, &src, &vp, MapLighting::Day);
    let 黄昏 = 渲染(&map, &src, &vp, MapLighting::DawnDusk);
    let 夜 = 渲染(&map, &src, &vp, MapLighting::Night);

    let (b, d, n) = (平均亮度(&白天), 平均亮度(&黄昏), 平均亮度(&夜));
    println!("[真实] 0.map 64×48 平均亮度：白天 {b:.3} / 黎明黄昏 {d:.3} / 夜晚 {n:.3}");
    println!(
        "[真实] 三档 fnv1a：白天 {:016x} / 黎明黄昏 {:016x} / 夜晚 {:016x}",
        fnv1a(&白天.rgba),
        fnv1a(&黄昏.rgba),
        fnv1a(&夜.rgba)
    );

    assert_ne!(白天.rgba, 黄昏.rgba, "白天与黎明黄昏必须不同");
    assert_ne!(黄昏.rgba, 夜.rgba, "黎明黄昏与夜晚必须不同");
    assert_ne!(白天.rgba, 夜.rgba, "白天与夜晚必须不同");
    assert!(b > d, "白天必须亮于黎明黄昏");
    assert!(d > n, "黎明黄昏必须亮于夜晚");
}

#[test]
fn 真实_逐格光罩_光源格附近比周围亮() {
    let Some((bytes, map)) = 读地图(真实地图) else { return };
    let Some(src) = 打开图源() else { return };
    // ⚠ 视口必须**挑到有光源的地方**：`0.map` 全图共 50 个发光格，但默认视口
    // (0,0,64×48) 里**一个都没有**（实测候选 0 个，夜晚档整幅纯黑）⇒ 本测试改用
    // (240,250) 起点的 64×48，里面有 `(265,261) light=5` 与 `(289,285) light=5` 两格。
    let vp = Viewport::new(240, 250, 64, 48);

    let 白天 = 渲染(&map, &src, &vp, MapLighting::Day);
    let 夜 = 渲染(&map, &src, &vp, MapLighting::Night);

    // 扫语料找一格「有 light ∈ 1..=9 且有前景图」、光心在视口内、且底层真的有不透明像素的格
    let mut 选中: Option<(u16, u16, u8, u32, u32, [u8; 4])> = None;
    let mut 候选数 = 0u32;
    for dx in 0..vp.cells_w as i32 {
        for dy in 0..vp.cells_h as i32 {
            let x = vp.origin_x + dx;
            let y = vp.origin_y + dy;
            let (Ok(xu), Ok(yu)) = (u16::try_from(x), u16::try_from(y)) else {
                continue;
            };
            let Some(cell) = map.cell(xu, yu) else { continue };
            if !emits_light(cell) {
                continue;
            }
            候选数 += 1;
            if 选中.is_some() {
                continue;
            }
            // ⚠ 光心必须按**该帧的真实锚点**算（2026-11-05 起光罩用 `Frame::anchor`）；
            // 若仍按 (0,0) 算，采到的点可能已经不在光心上了（`0.map` 的发光格锚点全非零）。
            let 锚点 = 锚点_取(&src, cell);
            let Some((cx, cy)) = 光心_带偏移(&vp, x, y, cell.light, 锚点) else {
                continue;
            };
            // 底层必须真有不透明像素，否则「更亮」只体现在 alpha 上、RGB 恒 0
            let Some(底) = 白天.pixel(cx, cy) else { continue };
            if 底[3] != 255 || 底[0] as u32 + 底[1] as u32 + 底[2] as u32 == 0 {
                continue;
            }
            选中 = Some((xu, yu, cell.light, cx, cy, 底));
        }
    }

    let Some((x, y, light, cx, cy, 底)) = 选中 else {
        eprintln!("[跳过] 视口内没有满足条件的发光格（候选 {候选数} 个）");
        return;
    };

    let 光心像素 = 夜.pixel(cx, cy).expect("光心像素");
    // A/B 对照：把该格的 light 清零后重渲，其余一切不变
    let 对照图 = 改光照(&bytes, &map, x, y, 0);
    let 对照 = 渲染(&对照图, &src, &vp, MapLighting::Night);
    let 对照像素 = 对照.pixel(cx, cy).expect("对照像素");

    // 「周围」= 同一幅图里远离任何光源的一块（(2,2) 格中心附近）
    let 周围 = 夜.pixel(2 * 48 + 24, 2 * 32 + 16).expect("周围像素");

    println!(
        "[真实] 光源格 ({x},{y}) light={light} fr_img={} area={}",
        map.cell(x, y).map(|c| c.fr_img).unwrap_or(0),
        map.cell(x, y).map(|c| c.area).unwrap_or(0)
    );
    println!("[真实] 光心像素 ({cx},{cy})：夜晚 {光心像素:?} / 同格 light 清零后 {对照像素:?} / 白天底层 {底:?}");
    println!("[真实] 周围 (120,80)：{周围:?} / 整幅平均亮度 {:.3}", 平均亮度(&夜));

    // ① 光罩确实减掉了遮罩 alpha —— 两条互补判据，按底层是否不透明自动选：
    //    · 底层不透明（本测试挑的就是这种）：alpha 合成后恒 255，效果只体现在 **RGB 变亮**；
    //    · 底层半透明/空：效果体现为 **alpha 变小**。
    if 对照像素[3] == 255 {
        assert!(
            亮度(光心像素) > 亮度(对照像素),
            "底层不透明时光心应更亮：{光心像素:?} vs 清零 {对照像素:?}"
        );
    } else {
        assert!(
            光心像素[3] < 对照像素[3],
            "底层透明时光心的遮罩 alpha 应更小：{光心像素:?} vs 清零 {对照像素:?}"
        );
    }
    // ② 真的更亮（底层不透明，所以 RGB 直接可比）
    assert!(
        亮度(光心像素) > 亮度(对照像素) + 20.0,
        "光心必须明显亮于同格清零光源的结果：{光心像素:?} vs {对照像素:?}"
    );
    // ③ 比「周围」亮
    assert!(
        亮度(光心像素) > 亮度(周围) + 20.0,
        "光心必须亮于远处：{光心像素:?} vs {周围:?}"
    );
}

#[test]
fn 真实_白天档与旧入口逐字节相同() {
    // `try_render_region_with_options(..., RenderOptions::default())` 必须等于
    // 「完全不传选项」的旧入口（`try_render_region_with`），这是「默认逐字节不变」的第二重证据。
    let Some((_, map)) = 读地图(真实地图) else { return };
    let Some(src) = 打开图源() else { return };
    let vp = Viewport::new(0, 0, 64, 48);
    let 新 = try_render_region_with_options(
        &map,
        &src,
        &vp,
        &LayerMask::all(),
        CollisionOverlay::none(),
        RenderOptions::default(),
    )
    .expect("应能渲染");
    let 旧 = miredit_render::try_render_region_with(
        &map,
        &src,
        &vp,
        &LayerMask::all(),
        CollisionOverlay::none(),
    )
    .expect("应能渲染");
    assert_eq!(新.rgba, 旧.rgba, "默认档必须与旧入口逐字节相同");
    assert_eq!(新.stats, 旧.stats, "统计也必须相同");
}

/// **零锚点的真实帧**：`Objects` 帧 1127（48×67）实测 `anchor = (0,0)`。
///
/// 取证报告 §5.3：**51.0%** 的发光格锚点恰为 `(0,0)` ⇒ 这些格补偏移后必须**一个像素都不变**。
/// 本条用「真实图库里的真实帧」而非假源，钉住这一点（真实 `Objects` 里 8,359 个可渲染帧中
/// 只有 7 个是零锚点：1127..1133）。
#[test]
fn 真实_零锚点帧的光心坐标不变() {
    let Some(src) = 打开图源() else { return };
    let 帧号 = 1127u32;
    let 锚点 = src.anchor(LibRef::surface(0), 帧号);
    println!("[零锚点] Objects 帧 {帧号} 的 anchor = {锚点:?}");
    let Some((ax, ay)) = 锚点 else {
        eprintln!("[跳过] 本机 Objects 帧 {帧号} 取不到锚点");
        return;
    };
    assert_eq!((ax, ay), (0, 0), "实测：Objects 帧 1127 的 anchor 恰为 (0,0)");

    // 合成地图（只控光源位置），帧号指向这个**真实零锚点帧** ⇒ fr_img = 1127 + 1
    let (sx, sy) = (2u16, 2u16);
    let map = 造单光源图(16, 12, sx, sy, 9, (帧号 + 1) as u16, None);
    let vp = Viewport::new(0, 0, 16, 12);

    let 改后 = 渲染(&map, &src, &vp, MapLighting::Night);
    let 改前 = 渲染(&map, &零锚点源(&src), &vp, MapLighting::Night);

    let 旧光心 = 光心(&vp, i32::from(sx), i32::from(sy), 9).expect("旧光心应在视口内");
    let 新光心 = 光心_带偏移(&vp, i32::from(sx), i32::from(sy), 9, (0, 0)).expect("同上");
    println!("[零锚点] 光心：改前 {旧光心:?} / 改后 {新光心:?}（应相同）");
    assert_eq!(旧光心, 新光心, "零锚点 ⇒ 光心坐标必须一模一样");
    assert_eq!(
        改后.rgba, 改前.rgba,
        "零锚点 ⇒ 整幅必须逐字节相同（51% 的发光格属于这一类）"
    );
}

/// 取证报告 §5.5 点名的发光格：`0.map (265,261)`，`light=5`，`fr_img=2724`（⇒ 帧号 2723）。
///
/// 报告实测该帧 `anchor = (-51,-113)`、帧尺寸 100×100、当前（错）光罩左上 `(12284, 8042)`、
/// 正确左上 `(12233, 7929)` ⇒ 差 `(-51,-113)`。本条把这件事钉成测试。
#[test]
fn 真实_0map_265_261_光心位移等于该帧锚点() {
    let Some((_, map)) = 读地图(真实地图) else { return };
    let Some(src) = 打开图源() else { return };
    let vp = Viewport::new(240, 250, 64, 48); // 该视口内才有发光格（见上一条测试的注释）

    let (x, y) = (265u16, 261u16);
    let Some(cell) = map.cell(x, y) else {
        eprintln!("[跳过] 0.map 取不到格 ({x},{y})");
        return;
    };
    println!(
        "[真实] ({x},{y}) light={} fr_img={} area={}",
        cell.light, cell.fr_img, cell.area
    );
    assert_eq!(cell.light, 5, "取证报告：该格 light=5");
    assert_eq!(cell.fr_img & 0x7FFF, 2724, "取证报告：该格 fr_img=2724");
    assert_eq!(cell.area, 0, "取证报告：该格 area=0 ⇒ 库 Objects");

    let 帧号 = (cell.fr_img & 0x7FFF) - 1; // = 2723
    let Some(锚点) = src.anchor(LibRef::surface(0), u32::from(帧号)) else {
        eprintln!("[跳过] Objects 帧 {帧号} 本机取不到锚点");
        return;
    };
    println!("[真实] Objects 帧 {帧号} 的 anchor = {锚点:?}");
    assert_eq!(锚点, (-51, -113), "取证报告 §5.5：该帧 anchor = (-51,-113)");

    let 改后 = 渲染(&map, &src, &vp, MapLighting::Night);
    let 改前 = 渲染(&map, &零锚点源(&src), &vp, MapLighting::Night);
    assert_ne!(改后.rgba, 改前.rgba, "锚点非零 ⇒ 夜晚档画面必须变");

    let 偏移 = (i32::from(锚点.0), i32::from(锚点.1));
    let 旧光心 = 光心(&vp, i32::from(x), i32::from(y), cell.light).expect("旧光心应在视口内");
    let 新光心 = 光心_带偏移(&vp, i32::from(x), i32::from(y), cell.light, 偏移)
        .expect("新光心应在视口内");
    let 位移 = (
        新光心.0 as i64 - 旧光心.0 as i64,
        新光心.1 as i64 - 旧光心.1 as i64,
    );
    println!("[真实] 光心：改前 {旧光心:?} → 改后 {新光心:?}（位移 {位移:?}）");
    assert_eq!(
        位移,
        (i64::from(锚点.0), i64::from(锚点.1)),
        "光心位移必须逐像素等于该帧 anchor"
    );

    // 该格光罩左上角也要对得上取证报告的两个数（世界像素坐标）
    let 改前左上 = light_bounds(i32::from(x), i32::from(y), cell.light, (0, 0)).expect("合法 light");
    let 改后左上 = light_bounds(i32::from(x), i32::from(y), cell.light, 偏移).expect("合法 light");
    println!(
        "[真实] 光罩左上（世界像素）：改前 ({}, {}) → 改后 ({}, {})",
        改前左上.0, 改前左上.1, 改后左上.0, 改后左上.1
    );
    assert_eq!((改前左上.0, 改前左上.1), (12284, 8042), "取证报告 §5.5 的「当前（错）」左上");
    assert_eq!((改后左上.0, 改后左上.1), (12233, 7929), "取证报告 §5.5 的「正确」左上");

    let 新处_改后 = 亮度(改后.pixel(新光心.0, 新光心.1).expect("像素"));
    let 新处_改前 = 亮度(改前.pixel(新光心.0, 新光心.1).expect("像素"));
    println!("[真实] 新光心亮度：改前 {新处_改前:.1} → 改后 {新处_改后:.1}");
    assert!(
        新处_改后 > 新处_改前,
        "新光心应更亮（光罩移过来了）：{新处_改后:.1} vs {新处_改前:.1}"
    );
}

/// 一个发光格的实测记录（最坏案例扫描用；单独立结构体而不是 6 元组，免得 clippy 报复杂类型）。
struct 发光格记录 {
    x: u16,
    y: u16,
    light: u8,
    /// 帧号（0-based，= `(fr_img & 0x7FFF) - 1`）。
    帧号: u16,
    /// 库号（`area`，`N` ⇒ 文件 `Objects{N+1}`）。
    area: u8,
    锚点: (i16, i16),
    /// `|dx| + |dy|`（像素）。
    量级: i32,
}

/// 取证报告 §5.3 的**最坏案例**：`hf001.map` / `Objects23` / 帧 2408 / `anchor = (-296,-265)`
/// ⇒ `|dx|+|dy| = 561 px`（≈ 11.7 格宽 / 17.5 格高）。
#[test]
fn 真实_最坏561px_hf001_锚点与光心位移() {
    let Some((_, map)) = 读地图(最坏地图) else { return };
    let Some(src) = 打开图源() else { return };

    // 全图扫一遍发光格，取 |dx|+|dy| 最大者（与取证报告 §5.3 同一口径：light ∈ 1..=9 且有前景图）
    let mut 最坏: Option<发光格记录> = None;
    let mut 发光格 = 0u32;
    let mut 取不到 = 0u32;
    for x in 0..map.width {
        for y in 0..map.height {
            let Some(cell) = map.cell(x, y) else { continue };
            if !emits_light(cell) {
                continue;
            }
            发光格 += 1;
            let 图号 = cell.fr_img & 0x7FFF;
            let Some((ax, ay)) =
                src.anchor(LibRef::surface(u16::from(cell.area)), u32::from(图号 - 1))
            else {
                取不到 += 1;
                continue;
            };
            let 量级 = i32::from(ax).abs() + i32::from(ay).abs();
            if 最坏.as_ref().is_none_or(|r| 量级 > r.量级) {
                最坏 = Some(发光格记录 {
                    x,
                    y,
                    light: cell.light,
                    帧号: 图号 - 1,
                    area: cell.area,
                    锚点: (ax, ay),
                    量级,
                });
            }
        }
    }
    let Some(最坏) = 最坏 else {
        eprintln!("[跳过] {最坏地图} 没有可解析的发光格（发光格 {发光格} 个）");
        return;
    };
    let 发光格记录 {
        x,
        y,
        light,
        帧号,
        area,
        锚点,
        量级: 最大,
    } = 最坏;
    println!(
        "[最坏] 图 {} / 格 ({x},{y}) / light={light} / 帧号 {帧号} / area={area}（库 Objects{}）/ anchor={锚点:?} / |dx|+|dy|={最大}",
        Path::new(最坏地图).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        area + 1
    );
    println!("[最坏] 该图发光格 {发光格} 个（其中 {取不到} 个取不到锚点，按 0 计）");

    assert_eq!(最大, 561, "取证报告 §5.3：最坏 |dx|+|dy| = 561 px");
    assert_eq!(锚点, (-296, -265), "取证报告 §5.3：最坏 anchor = (-296,-265)");
    assert_eq!(area, 22, "取证报告 §5.3：库号 22 ⇒ Objects23");
    assert_eq!(帧号, 2408, "取证报告 §5.3：帧号 2408");

    // 光心位移 == 锚点（这就是「561 px」的来历）
    let vp = Viewport::new(0, 0, 16, 16); // 只借它做坐标换算，画布尺寸不影响算式
    let 偏移 = (i32::from(锚点.0), i32::from(锚点.1));
    let 旧 = light_bounds(i32::from(x), i32::from(y), light, (0, 0)).expect("合法 light");
    let 新 = light_bounds(i32::from(x), i32::from(y), light, 偏移).expect("合法 light");
    println!(
        "[最坏] 光罩左上（世界像素）：改前 ({}, {}) → 改后 ({}, {})；尺寸 {}×{}",
        旧.0, 旧.1, 新.0, 新.1, 新.2, 新.3
    );
    assert_eq!((新.0 - 旧.0, 新.1 - 旧.1), 偏移, "光罩位移必须等于锚点");
    assert_eq!((新.2, 新.3), (旧.2, 旧.3), "偏移只平移、不改贴图尺寸");
    let 旧光心 = 光心_带偏移(&vp, i32::from(x), i32::from(y), light, (0, 0));
    let 新光心 = 光心_带偏移(&vp, i32::from(x), i32::from(y), light, 偏移);
    // 该格可能落在 16×16 视口外，只在都在视口内时才比坐标
    if let (Some(a), Some(b)) = (旧光心, 新光心) {
        assert_eq!(
            (b.0 as i64 - a.0 as i64, b.1 as i64 - a.1 as i64),
            (i64::from(锚点.0), i64::from(锚点.1))
        );
    }
}

/// **偏移只给光罩用**：同一份真实数据下，「有锚点」与「零锚点」的**白天档必须逐字节相同**
/// （三层合成一行都不碰 anchor），而**夜晚档必须不同**（光罩用了 anchor）。
///
/// 这是任务书 §③ 的验收：补偏移不许改三层像素。
#[test]
fn 真实_锚点只影响光罩不影响三层像素() {
    let Some((_, map)) = 读地图(真实地图) else { return };
    let Some(src) = 打开图源() else { return };
    // 用 (240,250) 起点：默认视口 (0,0) 在 0.map 里一个发光格都没有，夜晚档会比不出差异。
    let vp = Viewport::new(240, 250, 64, 48);

    let 白天_真 = 渲染(&map, &src, &vp, MapLighting::Day);
    let 白天_零 = 渲染(&map, &零锚点源(&src), &vp, MapLighting::Day);
    println!(
        "[三层] 白天档 fnv：有锚点 {:016x} / 零锚点 {:016x}",
        fnv1a(&白天_真.rgba),
        fnv1a(&白天_零.rgba)
    );
    assert_eq!(
        白天_真.rgba, 白天_零.rgba,
        "三层像素不得受 anchor 影响（官方画精灵只在特例下用 sprite.X/Y）"
    );

    let 夜_真 = 渲染(&map, &src, &vp, MapLighting::Night);
    let 夜_零 = 渲染(&map, &零锚点源(&src), &vp, MapLighting::Night);
    println!(
        "[光罩] 夜晚档 fnv：有锚点 {:016x} / 零锚点 {:016x}",
        fnv1a(&夜_真.rgba),
        fnv1a(&夜_零.rgba)
    );
    assert_ne!(
        夜_真.rgba, 夜_零.rgba,
        "夜晚档必须不同（否则说明光罩根本没用上 anchor）"
    );
}
