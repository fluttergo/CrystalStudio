//! 地图光照通道（官方「视图 → 地图光照」的渲染层实现，2026-11-05 新增）。
//!
//! # 一、官方机制（**不是逐格改色**）
//!
//! 取证报告 `deliverables/取证-地图光照与开启门-2026-11-05.md` §1.2-§1.4 的结论：
//! 官方做法是「**在精灵场景之上盖一张全屏黑色遮罩（alpha = 全局环境光缺口），
//! 再用『光罩贴图』把该处的 alpha 减掉（= 加光）**」，不是给每格像素乘亮度系数、
//! 也不是换调色板、更不是改 `btLight` 字节的解释。
//!
//! | # | 位置（CrystalStudio） | 关键代码 | 本模块对应 |
//! |---|---|---|---|
//! | 1 | `MapViewport.cs:89` | `DarknessVisible => Lighting != MapLighting.Day` | [`MapLighting::Day`] ⇒ [`apply`] **直接返回**，一个字节都不碰 |
//! | 2 | `MapViewport.Presentation.cs:34-35` | `dc.DrawImage(DarknessMask(), …)` | `Canvas::apply_black_mask` |
//! | 3 | `MapViewport.Presentation.cs:70` | `DawnDusk => 205, Day => 0, _ => 255` | [`MapLighting::darkness`] |
//! | 4 | `MapViewport.Presentation.cs:87-93` | `pixels[i] = Math.Max(0, pixels[i] - texture.Pixels[…])` | `叠加光源` 的 `saturating_sub` |
//! | 5 | `MapPreview.cs:14-15` | `EmitsLight` = `Light ∈ 1..9` **且**有前景图 | [`emits_light`] |
//! | 6 | `MapPreview.cs:17-19` | 11 项 `lightSizes` 表 | [`LIGHT_SIZES`] |
//! | 7 | `MapPreview.cs:23` | `index = Math.Min(light * 3, 9)` | [`light_bounds`] |
//! | 8 | `MapPreview.cs:26` | **定位用 `sizes[index]`、贴图用 `sizes[index+1]`** | [`light_bounds`]（**故意保留这格错位，不"修正"**） |
//! | 9 | `MapPreview.cs:34` | 贴图宽 `1→445 / 2→685 / 其它→925` | 由 `sizes[index+1]` 自然得出，见 [`LIGHT_SIZES`] 的自洽核验 |
//!
//! # 二、三档的**全部**差异就是一个字节
//!
//! | 档 | 环境光 ambient（注释值） | 遮罩 alpha = `255 - ambient` | 代码字面量 |
//! |---|---:|---:|---|
//! | [`MapLighting::DawnDusk`] | 50 | **205** | `Presentation.cs:70` `DawnDusk => 205` |
//! | [`MapLighting::Day`] | 255 | **0** | 同左 `Day => 0` |
//! | [`MapLighting::Night`] | 0 | **255** | 同左 `_ => 255`（默认分支 = Night） |
//!
//! `255-50=205` / `255-255=0` / `255-0=255` ⇒ 注释（`Presentation.cs:68-69`「dawn/evening = 50,
//! day = 255, standard night = 0」）与字面量**双证自洽**。
//!
//! ⚠ **本模块不生成任何衰减曲线**：三张光罩 PNG 是从官方目录**原样拷贝**的
//! （`assets/lights/{445,685,925}.png`，SHA256 与 `CrystalStudio/CrystalStudio/PreviewLights/`
//! 逐字节相同），本模块只做「解码 → 取 R → 减 alpha」。**没有 `205/0/255` 以外的任何自定数值**，
//! 也没有任何三档之间的过渡/动画（官方是三档互斥单选，`MainWindow.axaml.cs:626-633`）。
//!
//! # 三、四条硬口径
//!
//! 1. **「白天」= 关**：`darkness == 0` ⇒ [`apply`] 第一行就 `return`，连遮罩都不建
//!    （照官方 `MapViewport.cs:89` 的短路写，**不造一个恒等系数**）。这也是
//!    [`crate::RenderOptions::default()`] 能与改动前**逐字节相同**的来源；
//! 2. **盖三层、裁在地图矩形内**：本模块在 `compose` 的三层循环**全部结束之后**才跑
//!    （见 `compose.rs` 的调用点），而离屏画布**本身就是地图矩形**
//!    （`宽 = cells_w*48`、`高 = cells_h*32`）⇒ 「裁到地图矩形」在这里是恒等裁剪，
//!    不需要额外 `clip`。官方在屏幕上多出来的视口边缘（`Presentation.cs:31-32` 的
//!    `PushClip(map)`）在本库不存在；
//! 3. **光源只从物件层（表层）取，且要该层可见**：`front_visible == false` 时
//!    **只画遮罩、不叠任何光**（官方 `Presentation.cs:73` 的 `if (VisibleLayers[2])`）；
//! 4. **合成式是 `max(0, alpha - intensity)`**：`u8::saturating_sub`，不是乘、不是加。
//!
//! # 四、偏移口径与已知偏差（如实登记，不静默）
//!
//! （① 曾经是偏差、2026-11-05 已补齐；②③ 仍是偏差，逐条写明。）
//!
//! **① 光罩的世界坐标偏移 `offset` = 前景帧的 `anchor`（2026-11-05 已补齐）。**
//! 官方 `MapViewport.Presentation.cs:84-87@901dbe2f(未入库)`：
//! `offset = new(sprite.X, sprite.Y)`，`sprite` 取 `Assets.Get(front.Library, (front.Image & 0x7FFF) - 1)`
//! ⇒ **无条件**生效（不看库号/图号/尺寸），且是**静态基准帧**（不含门偏移、不含动画步进）。
//! 本模块照此实现：帧号走 [`crate::compose::帧号`]（= `(fr_img & 0x7FFF) - 1`，
//! **不叠加** [`crate::compose::门偏移`]），偏移取该帧的 [`mir2_assets::Frame::anchor`]。
//!
//! ⚠ **订正一句旧表述**：本模块 2026-11-05 早先版本写「这个偏移在当前数据链路上**不可得**」。
//! 按 `deliverables/取证-帧偏移数据可得性-2026-11-05.md` §1.4 实测，**数据一直都在**：
//! 解析层 `FrameInfo.px/py` 早就读出来了（`wil_decoder.rs:40-58@412fe3e8`），
//! 图集 meta 也早就在写 `anchor`（`meta.rs:27@412fe3e8`），只有 `mir2_assets::Frame`
//! 没暴露 ⇒ 准确说法是「**当前 API 未暴露**」，不是「数据链路上不可得」。现已补上。
//!
//! ⚠ **订正第二句旧表述**：旧文写「会差 `(sprite.X, sprite.Y)` 个像素（**通常是个位数到几十像素**）」。
//! 实测（同报告 §5.3，2,482 个发光格全扫）：**51.0% 恰为 `(0,0)`**（那半本来就对）；
//! 其余**中位 51 px、p90 140 px、最大 561 px**（≈ 11.7 格宽 / 17.5 格高）。
//! ⇒ 不是「个位数到几十」这种可忽略量级，而是「一半的图一个像素都不差、另一半每一束光都错」的双峰分布。
//!
//! **② 光罩强度的取法**：官方把 PNG 转成 **Pbgra32（预乘）** 后取 **R 通道**
//! （`MapPreview.cs:40-43`，注释「Premultiplication reproduces SourceAlpha * colour」）⇒
//! 本模块取 `R × A / 255`（四舍五入 `(r*a+127)/255`）。⚠ 预乘的**舍入方式**（截断 vs 四舍五入）
//! 未从 Avalonia 源码核实，差 **±1**，视觉不可辨；已按「四舍五入」实现并在此登记。
//! 实测三张 PNG 的 `R = G = B`、`A ≥ R`（例：`445.png` 中心 `(222,169)` = `A255 R237`，
//! `(222,10)` = `A74 R41`），故预乘后强度 ≤ 237，**单个光源不可能把遮罩压到全亮**。
//!
//! **③ 图外光源的扫描范围**：官方遍历整份文档的 `presentationCells`（`Presentation.cs:73`），
//! 图外的光源也会外溢到视口内。本库为省掉「整图扫描」按光罩尺寸反算了一个**充分**外扩量
//! （见 [`扫描外扩格数`]），等价于全图扫描（推导写在那个常量的文档里）。

use std::sync::OnceLock;

use mir2_assets::{LibRef, SpriteSource};
use mir2_map::{Cell, Map};

use crate::canvas::Canvas;
use crate::compose::{取格, 帧号};
use crate::png_io::read_png_from_memory;
use crate::types::{Viewport, CELL_H, CELL_W};

/// 地图光照档位（官方 `public enum MapLighting { DawnDusk, Day, Night }`，`MapLighting.cs:3`）。
///
/// **默认 = [`MapLighting::Day`]**，与官方两处默认一致：
/// `MapViewport.cs:88` 的 `Lighting { get; set; } = MapLighting.Day`、
/// `MainWindow.axaml:5` 的 `DayLighting … IsChecked="True"`。
///
/// ⚠ 官方**不持久化**这个选择（`SettingsStore.cs` / `SettingsWindow.cs` 里一处都没有）⇒
/// 本库同样不做持久化，默认值就是「白天」。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MapLighting {
    /// 黎明 / 黄昏：环境光 50 ⇒ 遮罩 alpha **205**（`Presentation.cs:70`）。
    DawnDusk,
    /// 白天：环境光 255 ⇒ 遮罩 alpha **0** ⇒ **完全不画遮罩**（`MapViewport.cs:89` 短路）。
    ///
    /// 这是 [`Default`]，也是「输出与改动前逐字节相同」的那一档。
    #[default]
    Day,
    /// 夜晚：环境光 0 ⇒ 遮罩 alpha **255**（`Presentation.cs:70` 的 `_ => 255` 默认分支）。
    Night,
}

impl MapLighting {
    /// 全屏黑色遮罩的 alpha：`255 - 环境光`。
    ///
    /// 逐值对应官方 `MapViewport.Presentation.cs:70` 的
    /// `Lighting switch { DawnDusk => 205, Day => 0, _ => 255 }`。
    pub const fn darkness(self) -> u8 {
        match self {
            MapLighting::DawnDusk => 205,
            MapLighting::Day => 0,
            MapLighting::Night => 255,
        }
    }

    /// 官方注释里的环境光值（`Presentation.cs:68-69`：dawn/evening = 50、day = 255、night = 0）。
    ///
    /// 与 [`MapLighting::darkness`] 的关系恒为 `darkness == 255 - ambient`（测试钉住）。
    pub const fn ambient(self) -> u8 {
        match self {
            MapLighting::DawnDusk => 50,
            MapLighting::Day => 255,
            MapLighting::Night => 0,
        }
    }

    /// 稳定短名（`"dawn-dusk" / "day" / "night"`），给 CLI / 文件名用。
    pub const fn name(self) -> &'static str {
        match self {
            MapLighting::DawnDusk => "dawn-dusk",
            MapLighting::Day => "day",
            MapLighting::Night => "night",
        }
    }
}

/// 官方 11 项 `lightSizes` 表（`CrystalStudio/CrystalStudio/MapPreview.cs:17-19`，逐字抄）。
///
/// | idx | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 |
/// |---|---|---|---|---|---|---|---|---|---|---|---|
/// | w×h | 125×95 | 205×156 | 285×217 | 365×277 | 445×338 | 525×399 | 605×460 | 685×521 | 765×581 | 845×642 | 925×703 |
///
/// **自洽核验（不是巧合）**：`index = min(light*3, 9)`、贴图取 `sizes[index+1]` ⇒
/// `light=1 → sizes[4] = 445×338`、`light=2 → sizes[7] = 685×521`、`light=3..9 → sizes[10] = 925×703`，
/// 与官方 `MapPreview.cs:34` 的 `width = light == 1 ? 445 : light == 2 ? 685 : 925` **逐值吻合**，
/// 也与三张 PNG 的 IHDR 尺寸（445×338 / 685×521 / 925×703）**逐值吻合**。
/// ⇒ 错位关系（定位用 `index`、贴图用 `index+1`）是官方**故意保留**的，**不许"修正"**。
pub const LIGHT_SIZES: [(u32, u32); 11] = [
    (125, 95),
    (205, 156),
    (285, 217),
    (365, 277),
    (445, 338),
    (525, 399),
    (605, 460),
    (685, 521),
    (765, 581),
    (845, 642),
    (925, 703),
];

/// 这一格**是否发光**（官方 `MapPreview.EmitsLight`，`MapPreview.cs:14-15`）。
///
/// 条件（两个都要）：
///
/// 1. `cell.light ∈ 1..=9`（官方 `cell.Light is > 0 and < 10`）——**`light == 0` 不发光，
///    `light >= 10` 也不发光**（数据里 10..255 是非法/其它语义，官方直接不认）；
/// 2. 该格**有前景图**（官方 `(front.Image & 0x7FFF) != 0`）——**没有前景图的格子即便
///    `light` 合法也不发光**（这是最容易漏的一条）。
///
/// 官方第三个条件 `Foreground is { Library: >= 0 }`（库号非负 = 有库）在本仓**恒成立**：
/// `mir2_map::Cell::area` 是 `u8`（`mir2-map/src/cell.rs:193`），不存在 C# 里 `-1` 那种
/// 「无库」哨兵值 ⇒ 不是漏判，是类型上不可能为负。
pub fn emits_light(cell: &Cell) -> bool {
    (1..10).contains(&cell.light) && (cell.fr_img & 0x7FFF) != 0
}

/// 一束光在**世界像素坐标**里的矩形：`(左上 x, 左上 y, 贴图宽, 贴图高)`。
///
/// 逐字对应官方 `MapPreview.LightBounds`（`MapPreview.cs:21-29`）：
///
/// ```text
/// index        = min(light * 3, 9)
/// positionSize = lightSizes[index]        // 定位用
/// textureSize  = lightSizes[index + 1]    // 贴图用（官方故意的错位）
/// x0 = x*48 - positionSize.Width  / 2 - 14 + offset.x
/// y0 = y*32 + 11 - positionSize.Height / 2      + offset.y
/// ```
///
/// ⚠ 三个细节都不许"顺手修正"：
/// - **`index` 与 `index+1` 的错位**（`MapPreview.cs:24-25` 注释原文「The client creates
///   textures starting at LightSizes[1], but positions them using LightSizes[index].
///   Preserve that one-entry difference.」）；
/// - **`-14` / `+11`** 这两个非对称常数；
/// - **`/2` 是整数除法**（C# `Size.Width` 是 `int` ⇒ `365/2 = 182`，不是 182.5）。
///
/// `offset` = 该格前景帧的 [`mir2_assets::Frame::anchor`]（= 官方 `sprite.X / sprite.Y`）。
/// 取法见模块文档 §四①：**静态基准帧**、**无条件生效**、不叠加门偏移；
/// 帧不可得时传 `(0, 0)`（官方 `Assets.Get` 失败时 `offset` 保持 `new()`，即零）。
///
/// `light` 不在 `1..=9` 时返回 `None`（调用方应先过 [`emits_light`]）。
pub fn light_bounds(x: i32, y: i32, light: u8, offset: (i32, i32)) -> Option<(i32, i32, u32, u32)> {
    if !(1..10).contains(&light) {
        return None;
    }
    let index = usize::from(light) * 3;
    let index = index.min(9);
    let (pos_w, pos_h) = LIGHT_SIZES[index];
    let (tex_w, tex_h) = LIGHT_SIZES[index + 1];
    // `saturating_*`：本函数是公开 API，`x/y` 是调用方给的 `i32`；正常路径（`x` 来自
    // 已校验的格坐标，≤ 65535）离溢出还差几个数量级，极端值下只饱和、不 panic。
    Some((
        x.saturating_mul(CELL_W as i32)
            .saturating_sub((pos_w / 2) as i32)
            .saturating_sub(14)
            .saturating_add(offset.0),
        y.saturating_mul(CELL_H as i32)
            .saturating_add(11)
            .saturating_sub((pos_h / 2) as i32)
            .saturating_add(offset.1),
        tex_w,
        tex_h,
    ))
}

/// 图外光源的扫描外扩量（**格**）。
///
/// 官方遍历整份文档的所有「有光/有门」格（`Presentation.cs:73`），所以**视口外的光源也会
/// 外溢进视口**。本库不整图扫描，改为「视口外扩 N 格」——N 由光罩尺寸反算，取**充分**值：
///
/// 一束光只可能在满足下式时碰到视口（`ox/oy` = 视口左上角世界像素、`W/H` = 视口像素宽高）：
///
/// ```text
/// x*48 - posW/2 - 14 + texW > ox*48   ⇒  x > ox + (posW/2 + 14 - texW) / 48
/// x*48 - posW/2 - 14        < ox*48+W ⇒  x < ox + cells_w + (posW/2 + 14) / 48
/// y*32 + 11 - posH/2 + texH > oy*32   ⇒  y > oy + (posH/2 - 11 - texH) / 32
/// y*32 + 11 - posH/2        < oy*32+H ⇒  y < oy + cells_h + (posH/2 - 11) / 32
/// ```
///
/// 代入 [`LIGHT_SIZES`] 三档（`light=1` / `2` / `3..9`）取极值：
///
/// | 方向 | 最紧的档 | 临界值（格） |
/// |---|---|---:|
/// | 左 | `light>=3`：`(422+14-925)/48` | **−10.19** |
/// | 右 | `light>=3`：`(422+14)/48` | **+9.08** |
/// | 上 | `light>=3`：`(321-11-703)/32` | **−12.28** |
/// | 下 | `light>=3`：`(321-11)/32` | **+9.69** |
///
/// ⇒ 理论最小充分外扩 = 左 10 / 右 10 / 上 12 / 下 10 格；本常量统一取 **16**（向上取整 + 余量），
/// 结果与「全图扫描」等价，代价只有 `(cells_w+32) × (cells_h+32)` 次取格。
pub const 扫描外扩格数: i32 = 16;

/// 把光照遮罩叠到**已经画完三层**的画布上（`compose` 在阻挡标记之前调用）。
///
/// # 行为
///
/// 1. `lighting.darkness() == 0`（= 白天）⇒ **立即返回，画布一个字节都不动**；
/// 2. 否则建一张 `宽×高` 的遮罩表，初值全是 `darkness`；
/// 3. `front_visible` 为真时，把每束光按 [`light_bounds`] 的矩形贴上去，
///    逐像素 `遮罩 = 遮罩.saturating_sub(强度)`（= 官方 `Math.Max(0, a - i)`）；
/// 4. 用 `Canvas::apply_black_mask` 把遮罩整幅叠到画布（纯黑源覆盖）。
///
/// # 参数
///
/// - `front_visible`：物件层（表层）是否可见。官方 `Presentation.cs:73` 的
///   `if (VisibleLayers[2])` ⇒ **层关掉时只压暗、不加光**；
/// - `src`：帧源，**只用来取前景帧的锚点偏移**（[`SpriteSource::anchor`]，不解码像素）。
///   官方光罩位置用 `sprite.X / sprite.Y`（`Presentation.cs:84-86`），这里就取那个值。
///   光罩**强度**与它无关（强度只来自贴图）—— 官方同口径：`EmitsLight` 只判图号非 0，
///   不判 `Assets.Get` 是否成功，取不到帧时 `offset` 保持零、光罩照画。
pub(crate) fn apply(
    canvas: &mut Canvas,
    map: &Map,
    vp: &Viewport,
    front_visible: bool,
    lighting: MapLighting,
    src: &dyn SpriteSource,
) {
    let darkness = lighting.darkness();
    if darkness == 0 {
        return; // 白天：连遮罩都不画（官方 `MapViewport.cs:89`）
    }
    let Some((宽, 高)) = vp.pixel_size() else {
        return; // 视口本身非法（`compose` 早一步已拒，这里只是不 panic）
    };
    let mut 遮罩 = vec![darkness; 宽 as usize * 高 as usize];
    if front_visible {
        叠加光源(&mut 遮罩, 宽, 高, map, vp, src);
    }
    canvas.apply_black_mask(&遮罩);
}

/// 逐格把光罩强度从遮罩 alpha 里减掉（官方 `Presentation.cs:87-93` 的循环）。
fn 叠加光源(
    遮罩: &mut [u8],
    宽: u32,
    高: u32,
    map: &Map,
    vp: &Viewport,
    src: &dyn SpriteSource,
) {
    // 视口左上角对应的世界像素坐标（官方 `origin` 的角色）。
    //
    // 全程 `saturating_*`：`Viewport::origin_x/origin_y` 是调用方给的 `i32`（可为负），
    // 极端值下乘法/加减会溢出 —— 这里宁可饱和成 `i32::MIN/MAX`（后面 `取格` 一律返回
    // `None`、`px0 >= px1` 直接跳过），也不许在 debug 构建里 panic。
    let 原点x = vp.origin_x.saturating_mul(CELL_W as i32);
    let 原点y = vp.origin_y.saturating_mul(CELL_H as i32);
    let 宽i = 宽 as i32;
    let 高i = 高 as i32;

    let x0 = vp.origin_x.saturating_sub(扫描外扩格数);
    let x1 = vp
        .origin_x
        .saturating_add(vp.cells_w as i32)
        .saturating_add(扫描外扩格数);
    let y0 = vp.origin_y.saturating_sub(扫描外扩格数);
    let y1 = vp
        .origin_y
        .saturating_add(vp.cells_h as i32)
        .saturating_add(扫描外扩格数);

    for x in x0..x1 {
        for y in y0..y1 {
            let Some(cell) = 取格(map, x, y) else {
                continue; // 图外：没有光源
            };
            if !emits_light(cell) {
                continue;
            }
            // 偏移 = 该格**静态基准帧**的 anchor（官方 `Presentation.cs:84-86`）。
            //
            // 三个必须守住的细节：
            // 1. **静态基准帧**：帧号用 `帧号(cell.fr_img)`（= `(fr_img & 0x7FFF) - 1`），
            //    **不叠加** `门偏移` —— 门开帧是运行期状态，官方光罩不看它，
            //    否则开门时光罩会跟着跳；
            // 2. **取不到帧 ⇒ 偏移 (0,0)**：官方 `Assets.Get` 返回 null 时 `offset` 保持
            //    `new()`（零），光罩照画（`EmitsLight` 只判图号非 0）；
            // 3. **只给光罩用**：这个 anchor 不许回流到三层合成（见 `mir2_assets::Frame` 的文档）。
            let offset = 帧号(cell.fr_img)
                .and_then(|索引| src.anchor(LibRef::surface(u16::from(cell.area)), 索引))
                .map_or((0, 0), |(ax, ay)| (i32::from(ax), i32::from(ay)));
            let Some((左世界, 上世界, 贴图宽, 贴图高)) = light_bounds(x, y, cell.light, offset)
            else {
                continue;
            };
            let Some(tex) = 光罩(贴图宽) else {
                continue; // 贴图不可用（不该发生：include_bytes! 编译期嵌入）
            };
            // 世界像素 → 画布像素
            let 左 = 左世界 - 原点x;
            let 上 = 上世界 - 原点y;
            let px0 = 左.max(0);
            let py0 = 上.max(0);
            let px1 = (左 + 贴图宽 as i32).min(宽i);
            let py1 = (上 + 贴图高 as i32).min(高i);
            if px0 >= px1 || py0 >= py1 {
                continue; // 整束光落在视口外
            }
            for py in py0..py1 {
                let ty = (py - 上) as u32;
                if ty >= tex.height {
                    break; // 贴图比表里的尺寸小（不该发生）：不越界，直接停
                }
                let 遮罩行 = py as usize * 宽 as usize;
                let 贴图行 = ty as usize * tex.width as usize;
                for px in px0..px1 {
                    let tx = (px - 左) as u32;
                    if tx >= tex.width {
                        break;
                    }
                    let i = 遮罩行 + px as usize;
                    let 强度 = tex.intensity[贴图行 + tx as usize];
                    if 强度 != 0 {
                        // 官方 `pixels[i] = (byte)Math.Max(0, pixels[i] - intensity)`
                        遮罩[i] = 遮罩[i].saturating_sub(强度);
                    }
                }
            }
        }
    }
}

/// 一张解码好的光罩：只保留**强度**（预乘 R），不保留颜色。
struct LightMask {
    width: u32,
    height: u32,
    /// 行优先强度，长度 = `width * height`。
    intensity: Vec<u8>,
}

/// 编译期嵌进二进制的三张官方光罩（**原样拷贝**，见模块文档 §二）。
///
/// 为什么用 `include_bytes!` 而不是运行期读盘：本 crate 的既有口径是「缺图不报错、
/// 换机器也能跑」（`bin/render_maps.rs:13`），若运行期读，无客户端的机器上光照会**静默消失**；
/// 编译期嵌入让「贴图缺失」变成**编译错误**。
const PNG_445: &[u8] = include_bytes!("../assets/lights/445.png");
const PNG_685: &[u8] = include_bytes!("../assets/lights/685.png");
const PNG_925: &[u8] = include_bytes!("../assets/lights/925.png");

static 光罩445: OnceLock<Option<LightMask>> = OnceLock::new();
static 光罩685: OnceLock<Option<LightMask>> = OnceLock::new();
static 光罩925: OnceLock<Option<LightMask>> = OnceLock::new();

/// 按贴图**宽**取光罩（官方 `MapPreview.LightTexture` 就是按宽做键，`MapPreview.cs:34`）。
///
/// 只认 `445 / 685 / 925` 三个宽度（[`LIGHT_SIZES`] 里 `index+1` 的取值只有这三个）。
/// 解码失败返回 `None`（⇒ 该束光不生效，**不 panic、也不伪造强度**）。
fn 光罩(宽度: u32) -> Option<&'static LightMask> {
    let 槽 = match 宽度 {
        445 => &光罩445,
        685 => &光罩685,
        925 => &光罩925,
        _ => return None,
    };
    槽.get_or_init(|| 解码光罩(宽度)).as_ref()
}

/// 解码一张 PNG 成强度表：`强度 = R × A / 255`（官方转 Pbgra32 后取 R，`MapPreview.cs:40-43`）。
fn 解码光罩(宽度: u32) -> Option<LightMask> {
    let 字节 = match 宽度 {
        445 => PNG_445,
        685 => PNG_685,
        925 => PNG_925,
        _ => return None,
    };
    let img = read_png_from_memory(字节).ok()?;
    let mut intensity = Vec::with_capacity(img.width as usize * img.height as usize);
    for px in img.rgba.chunks(4) {
        let r = u32::from(px[0]);
        let a = u32::from(px[3]);
        // 预乘：SourceAlpha × colour（四舍五入；舍入方式见模块文档 §四②）
        intensity.push(((r * a + 127) / 255) as u8);
    }
    Some(LightMask {
        width: img.width,
        height: img.height,
        intensity,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 三档常量与注释自洽() {
        for 档 in [MapLighting::DawnDusk, MapLighting::Day, MapLighting::Night] {
            assert_eq!(
                档.darkness(),
                255 - 档.ambient(),
                "{:?}：遮罩 alpha 必须等于 255 - 环境光",
                档
            );
        }
        assert_eq!(MapLighting::DawnDusk.darkness(), 205);
        assert_eq!(MapLighting::Day.darkness(), 0);
        assert_eq!(MapLighting::Night.darkness(), 255);
        assert_eq!(MapLighting::default(), MapLighting::Day, "默认必须是白天");
    }

    #[test]
    fn 尺寸表错位与贴图宽自洽() {
        // 官方 MapPreview.cs:34：1→445 / 2→685 / 其它→925
        for light in 1..=9u8 {
            let (_, _, 贴图宽, 贴图高) = light_bounds(0, 0, light, (0, 0)).expect("1..=9 合法");
            let index = (usize::from(light) * 3).min(9);
            assert_eq!((贴图宽, 贴图高), LIGHT_SIZES[index + 1], "light={light}");
            let 期望宽 = match light {
                1 => 445,
                2 => 685,
                _ => 925,
            };
            assert_eq!(贴图宽, 期望宽, "light={light} 的贴图宽");
        }
        assert!(light_bounds(0, 0, 0, (0, 0)).is_none());
        assert!(light_bounds(0, 0, 10, (0, 0)).is_none());
    }

    #[test]
    fn 定位尺寸用index而非index加一() {
        // light=1 ⇒ index=3 ⇒ 定位 (365,277)/2 = (182,138)；贴图 445×338
        assert_eq!(light_bounds(0, 0, 1, (0, 0)), Some((-196, -127, 445, 338)));
        // light=2 ⇒ index=6 ⇒ 定位 (605,460)/2 = (302,230)；贴图 685×521
        assert_eq!(light_bounds(0, 0, 2, (0, 0)), Some((-316, -219, 685, 521)));
        // light=9 ⇒ index=9 ⇒ 定位 (845,642)/2 = (422,321)；贴图 925×703
        assert_eq!(light_bounds(0, 0, 9, (0, 0)), Some((-436, -310, 925, 703)));
        // 换一格：x*48 / y*32 的位移
        assert_eq!(light_bounds(3, 2, 9, (0, 0)), Some((3 * 48 - 436, 2 * 32 - 310, 925, 703)));
        // offset 直接相加（官方 offset.X / offset.Y）
        assert_eq!(light_bounds(0, 0, 9, (7, -5)), Some((-429, -315, 925, 703)));
    }

    #[test]
    fn 发光判据要求有前景图() {
        let 无前景 = Cell {
            light: 5,
            ..Cell::default()
        };
        assert!(!emits_light(&无前景), "没有前景图 ⇒ 不发光");
        let 有前景 = Cell {
            light: 5,
            fr_img: 100,
            ..Cell::default()
        };
        assert!(emits_light(&有前景));
        for 非法 in [0u8, 10, 11, 255] {
            let c = Cell {
                light: 非法,
                fr_img: 100,
                ..Cell::default()
            };
            assert!(!emits_light(&c), "light={非法} 不该发光");
        }
        // 前景图只带 0x8000（图号 0）⇒ 仍然没有前景图
        let 只有高位 = Cell {
            light: 5,
            fr_img: 0x8000,
            ..Cell::default()
        };
        assert!(!emits_light(&只有高位), "fr_img & 0x7FFF == 0 ⇒ 不发光");
    }

    #[test]
    fn 三张光罩都能解码且尺寸与表一致() {
        for (宽, 期望高, 期望中心) in [(445u32, 338u32, 237u8), (685, 521, 239), (925, 703, 239)] {
            let tex = 光罩(宽).unwrap_or_else(|| panic!("{宽}.png 必须能解码"));
            assert_eq!((tex.width, tex.height), (宽, 期望高));
            assert_eq!(tex.intensity.len(), (宽 * 期望高) as usize);
            // 中心最亮、边角全黑（衰减是从中心向外的）
            let 中心 = tex.intensity[(期望高 / 2 * 宽 + 宽 / 2) as usize];
            assert_eq!(中心, 期望中心, "{宽}.png 中心预乘 R（实测值，见交付文档）");
            assert_eq!(tex.intensity[0], 0, "{宽}.png 左上角应为 0");
            assert_eq!(
                tex.intensity[tex.intensity.len() - 1],
                0,
                "{宽}.png 右下角应为 0"
            );
            // 实测：整张图只有极少数像素的预乘 R 达到 255（445→1 个 / 685→3 个 / 925→2 个）
            // ⇒ 「一束光几乎压不到全亮」，中心只有 237/239/239。
            let 全亮 = tex.intensity.iter().filter(|v| **v == 255).count();
            assert!(
                全亮 <= 8,
                "{宽}.png 达到 255 的像素应极少（实测 ≤3），实得 {全亮}"
            );
        }
    }

    #[test]
    fn 白天的遮罩不碰画布() {
        let mut c = Canvas::new(4, 4);
        // 先铺一块不透明红，白天档必须原样保留
        let 红 = mir2_assets::Frame {
            width: 4,
            height: 4,
            rgba: [255, 0, 0, 255].repeat(16),
            ..mir2_assets::Frame::default()
        };
        assert!(c.blit(&红, 0, 0));
        let 前 = c.into_rgba();
        // 直接验 apply_black_mask 的全 0 输入（等价于 apply 在 Day 档的短路）
        let mut c2 = Canvas::new(4, 4);
        assert!(c2.blit(&红, 0, 0));
        c2.apply_black_mask(&[0u8; 16]);
        assert_eq!(c2.into_rgba(), 前);
    }

    #[test]
    fn 黑色遮罩按alpha压暗() {
        let 红 = mir2_assets::Frame {
            width: 1,
            height: 1,
            rgba: vec![255, 0, 0, 255],
            ..mir2_assets::Frame::default()
        };
        // alpha=128 ⇒ RGB 约减半、alpha 保持 255
        let mut c = Canvas::new(1, 1);
        assert!(c.blit(&红, 0, 0));
        c.apply_black_mask(&[128]);
        let px = c.into_rgba();
        assert_eq!(px[3], 255, "不透明目标压黑后仍不透明");
        assert!((120..=135).contains(&px[0]), "红色通道应约减半，实得 {}", px[0]);
        // alpha=255 ⇒ 全黑不透明
        let mut c = Canvas::new(1, 1);
        assert!(c.blit(&红, 0, 0));
        c.apply_black_mask(&[255]);
        assert_eq!(c.into_rgba(), vec![0, 0, 0, 255]);
        // 空白画布 + alpha=255 ⇒ 黑 + 不透明
        let mut c = Canvas::new(1, 1);
        c.apply_black_mask(&[255]);
        assert_eq!(c.into_rgba(), vec![0, 0, 0, 255]);
    }
}
