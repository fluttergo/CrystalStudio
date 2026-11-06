//! 印章（`.X`）缩略图合成：把「相对坐标 + 三族图号/库号」的格集合渲染成**透明底 RGBA**。
//!
//! 供 GUI 与网页版的「印章面板」用：面板只需要一张小图 + 一个「内容框」，不需要知道
//! `.X` 的字节布局，也不需要依赖 `mir2-stamp`。
//!
//! # 一、落格口径（**与地图渲染同源，不另立一套**）
//!
//! 三条锚点算式与帧号解码**只在 [`crate::compose`] 实现一次**，本模块调同一批函数：
//!
//! | 层 | 锚点 | 共用函数 | 出处 |
//! |---|---|---|---|
//! | 地表（`bk`） | **左上角** `(dx*48, dy*32)` | `compose::左上锚点`（`compose.rs:268`，调用点 `compose.rs:136`） | `map_view.gd:13` |
//! | 中层（`mid`） | **左上角** `(dx*48, dy*32)` | 同上（调用点 `compose.rs:163`） | `map_overlay_layer.gd:79` |
//! | 物件（`fr`） | **底边** `top=(dy+1)*32-h` | `compose::底边锚点`（`compose.rs:276`，调用点 `compose.rs:190`） | `map_overlay_layer.gd:18-19,96-99`；原作 `GameScene.cpp:3515` |
//! | 帧号 | `0`=不画；否则 `(img & 0x7FFF) - 1` | `compose::帧号`（`compose.rs:286`，调用点 `compose.rs:129/156/183`） | `map_loader.gd:51-52` |
//!
//! **层序**：地表 → 中层 → 物件（后画的压前画的），与 `compose.rs:113-202` 的段序一致
//! （`map_view.gd:108-114`、`map_overlay_layer.gd:22-24`）。
//!
//! **层内顺序**：**行优先**——`dy` 外、`dx` 内。印章的格是稀疏集合，本模块把格
//! **按 `(dy, dx)` 升序稳定排序**后再画；同坐标的重复格保持调用方给的顺序（稳定排序）。
//!
//! 出处（上游两条印章路径**都是行优先**，与地图的列优先无关）：
//!
//! | 上游文件:行 | 排序 |
//! |---|---|
//! | `CrystalStudio/MapViewport.StampRendering.cs:141` / `:143` | `cells.OrderBy(c => c.Y).ThenBy(c => c.X)`（中层、物件各一遍；`:137-138` 注释「Assembled stamps use the map's layer and row order」） |
//! | `CrystalStudio/StampPreview.cs:60` | `drawings.OrderBy(c => c.Layer).ThenBy(c => c.Y).ThenBy(c => c.X)` |
//!
//! ⚠ **与 [`crate::compose`] 的列优先是「有意不同」**（A-010 口径修正，2026-11-06）：地图渲染按
//! 格扫（`compose.rs` 模块文档「二、层序与层内顺序」：`for dx` 在外层，以 Godot 客户端
//! `map_overlay_layer.gd:46-53`
//! 为准），印章**不套用**那个顺序 —— 上游给印章单独定了行优先。因此**跨格重叠**（帧比格大、
//! 两格在 x/y 上交叉）时，印章的遮挡关系可能与地图渲染**相反**，这是口径而非 bug，别再「修回去」。
//! 格间不重叠（含 1×1 单格）时两种顺序输出逐字节相同，故 `tests/stamp.rs` 的
//! 「印章与地图渲染逐字节同源」与本条不冲突。
//!
//! # 二、与地图渲染的三处**有意差异**（都是任务口径，不是漏改）
//!
//! 1. **不套用「地表只铺偶偶格」**：地图里地表只铺 `x%2==0 && y%2==0`（`compose.rs:115-124`，
//!    出处 `map_view.gd:199`）。印章是「从地图里抠出来的一小块」，坐标已被归一化，
//!    奇偶性没有地图语义 ⇒ **给了地表就画**（任务书 §三 明示）。
//! 2. **画布 = 格包围盒**，不是「调用方给的视口」：`w = (max_dx-min_dx+1)*48`、
//!    `h = (max_dy-min_dy+1)*32`。因此**超出包围盒的帧像素会被裁掉**（地表帧 96×64 在最右/
//!    最下那一格会各多出 48/32 px）。这是为了让「印章面板」里每枚印章的尺寸可比；
//!    裁没裁得到由 [`StampImage::content`] 如实反映（内容框贴到画布边缘 = 被裁过）。
//! 3. **没有「图外格」**：地图里窗口落在图外的格不画也不计数；印章没有地图，所有格都在画布内。
//!
//! # 三、缺帧口径（**与 `compose.rs` §三 逐条相同，禁假数据**）
//!
//! - 图号 `0` ⇒ 该层本来就没图，**跳过且不计 missing**；
//! - 图号非 0 但 [`SpriteSource::frame`] 返回 `None`（库没登记 / 帧号越界 / 空帧）
//!   ⇒ **该格该层一个像素都不画**，`missing += 1`；**绝不画洋红/纯色占位**；
//! - `cells_drawn` 的口径是「**真的写了至少 1 个像素**的格数」，判据只用
//!   [`crate::canvas`] 的 `Canvas::blit` 返回值（与 `compose.rs:44-47` 同一口径）：
//!   0×0 空帧、整帧落在画布外、帧像素全透明 ⇒ 都不算落笔。
//!
//! # 四、缩放口径（缩略图）
//!
//! | 输入 | 输出尺寸 | 滤镜 |
//! |---|---|---|
//! | `box_px = None`、`scale = 1.0` | `包围盒 + 2*margin_px`（原始像素） | 不缩放 |
//! | `box_px = Some((w, h))` | **裁到内容框**后等比缩放到 `(w-2m, h-2m)` 内，再四边各留 `m` | 缩小按 `filter`；放大**强制** Nearest |
//! | `box_px = None`、`scale = s` | 裁到内容框后按 `s` 缩放，再四边各留 `m` | 同上 |
//!
//! 三条细节（都为了「缩略图不能糊、不能偏色」）：
//!
//! 1. **先裁到内容框再缩放**：印章包围盒里常有整行/整列没图（例如只有物件、没有地表），
//!    直接缩包围盒会让内容缩得很小、四周一圈空。
//! 2. **留白在缩放之后加**：这样 `margin_px = 2` 就是**成品图**上的 2 px 透明边，
//!    四角 alpha 恒为 0（缩略图能直接贴在任何底色上，不用调用方再补边）。
//! 3. **放大一律 Nearest**（像素画）：`scale > 1` 时即使传 `Box`/`Lanczos` 也走最近邻，
//!    否则边缘会被插值糊成渐变 —— 那是「假像素」。缩小则尊重 `filter`
//!    （默认 [`FilterClass::Box`]，缩小时非 Nearest，见任务书 §四.4）。
//!
//! 缩放内核在 [`crate::resample`]（预乘 alpha、面积加权/可分离 Lanczos3）。
//!
//! # 五、空输入 / 空内容
//!
//! `cells` 为空，或**所有格都没画出任何像素**（图号全 0 / 全缺帧 / 帧全透明）⇒
//! 返回 **`width = height = 0`、`rgba` 为空 `Vec`** 的 [`StampImage`]，**不是 `Err`**。
//!
//! 理由：空印章是**合法数据**（`.X` 允许 0 格，`mir2_stamp::Stamp` 也不把 0 格当错误），
//! 面板里出现一枚空格子是完全正常的业务状态；把它变成 `Err` 会逼每个调用方写一遍
//! 「这不是错误」的分支。`0×0 + 空 Vec` 是明确的「无内容」信号，不可能被误当成一张图。
//! 需要报错的输入（缩放比例非法、目标框非法、尺寸越界）仍然返回 [`RenderError`]。
//!
//! # 六、依赖纪律（**硬约束**）
//!
//! 本模块**不依赖 `mir2-stamp`**：入参是本 crate 自己的 [`PositionedCell`]，
//! 由调用方从 `.X`（或任何来源）适配。`mir2-stamp` 只出现在 `[dev-dependencies]`，
//! 供 `tests/stamp.rs` 用真实印章做验收。

use mir2_assets::{Frame, LibRef, SpriteSource};

use crate::canvas::Canvas;
use crate::compose::{底边锚点, 帧号, 左上锚点};
use crate::frames::FrameCache;
use crate::resample;
use crate::types::{RenderError, CELL_H, CELL_W, MAX_PIXELS, MAX_SIDE};

/// 一枚待渲染的格：**相对坐标** + 三族图号/库号（由调用方从 `.X` 或任何来源适配）。
///
/// 坐标语义：`(dx, dy)` 是相对印章原点的**世界格**坐标，可为负（`.X` 里就是相对坐标）。
/// 渲染时整体平移成非负（`dx - min_dx`），所以原点选在哪里不影响输出。
///
/// 图号语义与地图格**完全一致**：1-based，`0` = 该层没有图，最高位是「不可通过」标记
/// （不参与取图，渲染前按 `& 0x7FFF` 抹掉）。库号语义同 `mir2_assets::LibRef`：
/// 地表 → `Tiles`/`Tiles{n}`、中层 → `SmTiles`/`SmTiles{n}`、物件 → `Objects`/`Objects{n}`。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PositionedCell {
    /// 相对印章原点的格 X（可为负）。
    pub dx: i32,
    /// 相对印章原点的格 Y（可为负）。
    pub dy: i32,
    /// 地表图号（`wBkImg` 口径，1-based，`0` = 无）。
    pub bk: u16,
    /// 中层图号（`wMidImg` 口径，1-based，`0` = 无）。
    pub mid: u16,
    /// 物件图号（`wFrImg` 口径，1-based，`0` = 无）。
    pub fr: u16,
    /// 地表库号（`btTiles` 口径）。
    pub bk_lib: u8,
    /// 中层库号（`btsmTiles` 口径）。
    pub mid_lib: u8,
    /// 物件库号（`btArea` 口径）。
    pub fr_lib: u8,
}

/// 缩小滤镜的种类（放大一律走 Nearest，见模块文档 §四）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FilterClass {
    /// 最近邻：不产生新颜色，但缩小时会丢像素（锯齿/闪烁）。
    Nearest,
    /// 盒式（面积平均）：缩小的**默认**选择，结果稳定、不振铃。
    #[default]
    Box,
    /// Lanczos3：比 Box 锐，边缘会有轻微振铃（负瓣），适合缩得很小又要保结构。
    Lanczos,
}

/// 印章渲染选项。
///
/// 优先级：`box_px` **优先于** `scale`；两者都没设（`None` + `1.0`）时输出原始像素尺寸。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StampRenderOptions {
    /// 等比缩放到该框内（如 `Some((46, 46))` = 面板里的 46×46 缩略图）；
    /// `None` = 不按框缩放（看 [`StampRenderOptions::scale`]）。
    pub box_px: Option<(u32, u32)>,
    /// 直接指定缩放倍数（`box_px` 为 `None` 时生效）；必须 **有限且 > 0**。
    pub scale: f32,
    /// 缩小滤镜（放大一律强制 Nearest）。
    pub filter: FilterClass,
    /// 成品图四边的透明留白（像素），默认 2。
    pub margin_px: u32,
}

impl Default for StampRenderOptions {
    /// 默认：不缩放、`Box` 滤镜、四边留白 **2 px**。
    fn default() -> Self {
        StampRenderOptions {
            box_px: None,
            scale: 1.0,
            filter: FilterClass::Box,
            margin_px: 2,
        }
    }
}

impl StampRenderOptions {
    /// 原始像素尺寸（`scale = 1.0`、**无留白**）——验收/对账用。
    pub const fn raw() -> Self {
        StampRenderOptions {
            box_px: None,
            scale: 1.0,
            filter: FilterClass::Nearest,
            margin_px: 0,
        }
    }

    /// 缩略图选项：等比缩放到 `w×h` 框内，缩小用 `Box`，四边留白 2 px。
    pub const fn thumbnail(w: u32, h: u32) -> Self {
        StampRenderOptions {
            box_px: Some((w, h)),
            scale: 1.0,
            filter: FilterClass::Box,
            margin_px: 2,
        }
    }
}

/// 一枚印章的渲染产物：透明底 RGBA8 + 实际内容框 + 三个计数。
///
/// 底色是**全透明黑** `(0,0,0,0)`（与 [`crate::Rendered`] 同一口径），调用方自己决定叠在
/// 什么底色上看。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StampImage {
    /// 行优先 RGBA8，长度 = `width * height * 4`（空图为空 `Vec`）。
    pub rgba: Vec<u8>,
    /// 像素宽（空内容时为 0）。
    pub width: u32,
    /// 像素高（空内容时为 0）。
    pub height: u32,
    /// **实际有像素**的包围盒 `(min_x, min_y, max_x, max_y)`，**闭区间**（与
    /// `mir2_stamp::Stamp::bounds` 同一种表达）。无任何像素时为 `(0, 0, 0, 0)`
    /// —— 那种情况 `width == 0`，调用方应看尺寸而不是看这个元组。
    pub content: (u32, u32, u32, u32),
    /// 至少有一层真的写下像素的格数（同 `RenderStats::cells_drawn` 口径）。
    pub cells_drawn: usize,
    /// 真正向图库发起的成功解码次数（帧缓存未命中且取到帧才 +1）。
    pub frames_decoded: usize,
    /// 有图号但取不到帧的**层-格**次数（三层累加；图号为 0 的不计）。
    pub missing: usize,
}

impl StampImage {
    /// 取某像素 RGBA；越界返回 `None`。
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = (y as usize * self.width as usize + x as usize) * 4;
        let s = self.rgba.get(i..i + 4)?;
        Some([s[0], s[1], s[2], s[3]])
    }

    /// 内容框的宽（无内容 ⇒ 0）。
    pub fn content_width(&self) -> u32 {
        if self.width == 0 {
            0
        } else {
            self.content.2 - self.content.0 + 1
        }
    }

    /// 内容框的高（无内容 ⇒ 0）。
    pub fn content_height(&self) -> u32 {
        if self.height == 0 {
            0
        } else {
            self.content.3 - self.content.1 + 1
        }
    }

    /// 导出为 PNG（RGBA8）。目录不存在会报错（不隐式建目录）。
    pub fn save_png(&self, path: &std::path::Path) -> std::io::Result<()> {
        crate::png_io::write_rgba_png(path, self.width, self.height, &self.rgba)
    }
}

/// 渲染一枚印章：返回**带 alpha 的 RGBA**（背景透明），并给出实际内容包围盒。
///
/// # 参数
///
/// - `cells`：格集合（相对坐标 + 三族图号/库号），顺序随意（内部按 `(dy, dx)` **行优先**稳定排序，
///   见模块文档「层内顺序」）；
/// - `src`：帧源（`mir2_assets::SpriteSource`，与地图渲染同一套）；
/// - `opts`：缩放/留白选项，见 [`StampRenderOptions`]。
///
/// # 错误
///
/// - [`RenderError::InvalidStampScale`]：`scale` 为 `NaN`/`±inf`/`≤ 0`；
/// - [`RenderError::InvalidStampBox`]：`box_px` 的宽或高为 0，或框比两侧留白还小；
/// - [`RenderError::DimensionOverflow`]：包围盒（或目标框）超出护栏，先报错而不是 OOM。
///
/// **缺帧不是错误**（计入 [`StampImage::missing`]）；**空输入也不是错误**（返回 0×0 空图，
/// 见模块文档 §五）。
///
/// # 例子
///
/// ```
/// use miredit_render::stamp::{render_stamp, PositionedCell, StampRenderOptions};
/// use mir2_assets::{Frame, LibRef, SpriteSource};
///
/// /// 一格 48×32 的不透明红方块（只为演示，真实帧来自 .wzl/.wil）。
/// struct 红方块;
/// impl SpriteSource for 红方块 {
///     fn frame(&self, _lib: LibRef, _index: u32) -> Option<Frame> {
///         Some(Frame {
///             width: 48,
///             height: 32,
///             rgba: [200, 10, 10, 255].repeat(48 * 32),
///             ..Frame::default() // 锚点 (0,0)：印章/三层合成都不用 anchor
///         })
///     }
/// }
///
/// let cells = [PositionedCell {
///     dx: 0, dy: 0, bk: 0, mid: 1, fr: 0,
///     bk_lib: 0, mid_lib: 0, fr_lib: 0,
/// }];
/// let img = render_stamp(&cells, &红方块, &StampRenderOptions::raw())?;
/// // 1 格 ⇒ 48×32；中层左上角锚定 ⇒ 整幅都是那块红
/// assert_eq!((img.width, img.height), (48, 32));
/// assert_eq!(img.content, (0, 0, 47, 31));
/// assert_eq!(img.cells_drawn, 1);
/// assert_eq!(img.missing, 0);
/// # Ok::<(), miredit_render::RenderError>(())
/// ```
pub fn render_stamp(
    cells: &[PositionedCell],
    src: &dyn SpriteSource,
    opts: &StampRenderOptions,
) -> Result<StampImage, RenderError> {
    // ---- 1. 选项校验（先校验，保证任何输入都不会 panic / OOM）----
    let 目标框 = 校验选项(opts)?;

    if cells.is_empty() {
        return Ok(空图(0, 0, 0));
    }

    // ---- 2. 格包围盒 → 原始画布尺寸（模块文档 §二.2）----
    let (min_x, min_y, max_x, max_y) = 格包围盒(cells);
    let 格宽 = (i64::from(max_x) - i64::from(min_x) + 1) as u64;
    let 格高 = (i64::from(max_y) - i64::from(min_y) + 1) as u64;
    let 原始宽 = 格宽 * u64::from(CELL_W);
    let 原始高 = 格高 * u64::from(CELL_H);
    if 原始宽 > MAX_SIDE || 原始高 > MAX_SIDE || 原始宽 * 原始高 > MAX_PIXELS {
        return Err(RenderError::DimensionOverflow {
            width: 原始宽,
            height: 原始高,
        });
    }
    let 原始宽 = 原始宽 as u32;
    let 原始高 = 原始高 as u32;

    // ---- 3. 三层合成（层序/锚点/帧号全部走 compose 的同一批函数）----
    let mut canvas = Canvas::new(原始宽, 原始高);
    let mut cache = FrameCache::new(src);
    let mut missing = 0usize;
    // 「这一格至少写过 1 个像素」——`cells_drawn` 的唯一判据（同 `compose.rs:44-47`）
    let mut touched = vec![false; cells.len()];
    // 行优先（`dy` 外、`dx` 内）：A-010 口径修正，对齐上游 `MapViewport.StampRendering.cs:141/143`
    // （`cells.OrderBy(c => c.Y).ThenBy(c => c.X)`）与 `StampPreview.cs:60`（Layer → Y → X）。
    // ⚠ 与 `compose.rs` 的列优先**有意不同**（模块文档「层内顺序」一段），跨格重叠时遮挡可能相反。
    // 稳定排序 ⇒ 同坐标的重复格保持调用方给的顺序。
    let mut 顺序: Vec<usize> = (0..cells.len()).collect();
    顺序.sort_by_key(|i| (cells[*i].dy, cells[*i].dx));

    // ---- 地表：左上角锚定（`compose.rs:136`）----
    for &i in &顺序 {
        let c = cells[i];
        let Some(索引) = 帧号(c.bk) else {
            continue; // 该格本来没有地表图 —— 不是缺帧
        };
        match cache.get(LibRef::ground(u16::from(c.bk_lib)), 索引) {
            Some(f) => {
                let (px, py) = 左上锚点(c.dx - min_x, c.dy - min_y);
                if canvas.blit(&f, px, py) {
                    touched[i] = true;
                }
            }
            None => missing += 1,
        }
    }

    // ---- 中层：左上角锚定（`compose.rs:163`）----
    for &i in &顺序 {
        let c = cells[i];
        let Some(索引) = 帧号(c.mid) else {
            continue;
        };
        match cache.get(LibRef::middle(u16::from(c.mid_lib)), 索引) {
            Some(f) => {
                let (px, py) = 左上锚点(c.dx - min_x, c.dy - min_y);
                if canvas.blit(&f, px, py) {
                    touched[i] = true;
                }
            }
            None => missing += 1,
        }
    }

    // ---- 物件：底边钉在格底（`compose.rs:190`）----
    for &i in &顺序 {
        let c = cells[i];
        let Some(索引) = 帧号(c.fr) else {
            continue;
        };
        match cache.get(LibRef::surface(u16::from(c.fr_lib)), 索引) {
            Some(f) => {
                let (px, py) = 底边锚点(c.dx - min_x, c.dy - min_y, f.height);
                if canvas.blit(&f, px, py) {
                    touched[i] = true;
                }
            }
            None => missing += 1,
        }
    }

    let cells_drawn = touched.iter().filter(|t| **t).count();
    let frames_decoded = cache.decoded() as usize;
    let 原始 = canvas.into_rgba();

    // ---- 4. 内容框（无像素 ⇒ 0×0 空图，模块文档 §五）----
    let Some(内容) = 内容包围盒(&原始, 原始宽, 原始高) else {
        return Ok(空图(cells_drawn, frames_decoded, missing));
    };

    // ---- 5. 缩放 / 留白（模块文档 §四）----
    let (rgba, width, height) = if let Some((框宽, 框高)) = 目标框 {
        // 缩略图：裁到内容框 → 等比缩放到框内 → 四边留白
        let (裁, 裁宽, 裁高) = 裁内容(&原始, 原始宽, 内容);
        let 可用宽 = 框宽 - 2 * opts.margin_px;
        let 可用高 = 框高 - 2 * opts.margin_px;
        let 比例 = (f64::from(可用宽) / f64::from(裁宽)).min(f64::from(可用高) / f64::from(裁高));
        缩放并留白(裁, 裁宽, 裁高, 比例, opts.filter, opts.margin_px)?
    } else if (opts.scale - 1.0).abs() > f32::EPSILON {
        // 直接给比例：同样先裁到内容框，再按比例缩放 + 留白
        let (裁, 裁宽, 裁高) = 裁内容(&原始, 原始宽, 内容);
        缩放并留白(裁, 裁宽, 裁高, f64::from(opts.scale), opts.filter, opts.margin_px)?
    } else {
        // 原始像素尺寸：整块包围盒 + 留白（不裁内容框 —— 尺寸要可复算）
        贴留白(原始, 原始宽, 原始高, opts.margin_px)?
    };

    let 最终内容 = 内容包围盒(&rgba, width, height).unwrap_or((0, 0, 0, 0));
    Ok(StampImage {
        rgba,
        width,
        height,
        content: 最终内容,
        cells_drawn,
        frames_decoded,
        missing,
    })
}

/// 校验选项，返回「生效的目标框」（`None` = 原始尺寸路径）。
fn 校验选项(opts: &StampRenderOptions) -> Result<Option<(u32, u32)>, RenderError> {
    if let Some((w, h)) = opts.box_px {
        // 留白 ×2 用 u64 算，避免 `margin_px` 手抖写成 u32::MAX 时溢出
        let 留白二倍 = u64::from(opts.margin_px) * 2;
        let 可用宽 = u64::from(w).saturating_sub(留白二倍);
        let 可用高 = u64::from(h).saturating_sub(留白二倍);
        if w == 0 || h == 0 || 可用宽 == 0 || 可用高 == 0 {
            return Err(RenderError::InvalidStampBox {
                box_w: w,
                box_h: h,
            });
        }
        if u64::from(w) > MAX_SIDE
            || u64::from(h) > MAX_SIDE
            || u64::from(w) * u64::from(h) > MAX_PIXELS
        {
            return Err(RenderError::DimensionOverflow {
                width: u64::from(w),
                height: u64::from(h),
            });
        }
        return Ok(Some((w, h)));
    }
    let s = opts.scale;
    if !s.is_finite() || s <= 0.0 {
        // NaN 的 `as i64` 在 Rust 里是 0（饱和转换），日志里一眼能看出不是正常比例
        return Err(RenderError::InvalidStampScale {
            scale_milli: (f64::from(s) * 1000.0) as i64,
        });
    }
    Ok(None)
}

/// 按 `scale` 缩放（`box_px = None` 分支）后留白。
fn 缩放并留白(
    源: Vec<u8>,
    宽: u32,
    高: u32,
    比例: f64,
    滤镜: FilterClass,
    留白: u32,
) -> Result<(Vec<u8>, u32, u32), RenderError> {
    let 目标宽 = (f64::from(宽) * 比例).round().max(1.0) as u64;
    let 目标高 = (f64::from(高) * 比例).round().max(1.0) as u64;
    if 目标宽 > MAX_SIDE || 目标高 > MAX_SIDE || 目标宽 * 目标高 > MAX_PIXELS {
        return Err(RenderError::DimensionOverflow {
            width: 目标宽,
            height: 目标高,
        });
    }
    let 目标宽 = 目标宽 as u32;
    let 目标高 = 目标高 as u32;
    let 实际滤镜 = 选滤镜(比例, 滤镜);
    let 缩放后 = resample::缩放(&源, 宽, 高, 目标宽, 目标高, 实际滤镜);
    贴留白(缩放后, 目标宽, 目标高, 留白)
}

/// 滤镜裁决：**放大（比例 ≥ 1）一律 Nearest**（像素画），缩小尊重调用方。
fn 选滤镜(比例: f64, 请求: FilterClass) -> FilterClass {
    if 比例 >= 1.0 {
        FilterClass::Nearest
    } else {
        请求
    }
}

/// 把一块 RGBA 贴进「四边各 `留白` px」的透明画布（留白为 0 时原样返回）。
///
/// 走 [`Canvas::blit`] 的同一套 `over` 合成：目标全透明 ⇒ 源像素**逐字节原样**写回
/// （`over` 的 `a_s == 255` 快路径 / 半透明时 `a_d == 0` 的还原路径都不改色）。
fn 贴留白(
    源: Vec<u8>,
    宽: u32,
    高: u32,
    留白: u32,
) -> Result<(Vec<u8>, u32, u32), RenderError> {
    if 留白 == 0 {
        return Ok((源, 宽, 高));
    }
    let 总宽 = u64::from(宽) + 2 * u64::from(留白);
    let 总高 = u64::from(高) + 2 * u64::from(留白);
    if 总宽 > MAX_SIDE || 总高 > MAX_SIDE || 总宽 * 总高 > MAX_PIXELS {
        return Err(RenderError::DimensionOverflow {
            width: 总宽,
            height: 总高,
        });
    }
    let mut canvas = Canvas::new(总宽 as u32, 总高 as u32);
    let 帧 = Frame {
        width: 宽,
        height: 高,
        rgba: 源,
        ..mir2_assets::Frame::default()
    };
    canvas.blit(&帧, 留白 as i32, 留白 as i32);
    Ok((canvas.into_rgba(), 总宽 as u32, 总高 as u32))
}

/// 从整幅里裁出内容框（`content` 为闭区间 `(min_x, min_y, max_x, max_y)`）。
///
/// 返回 `(像素缓冲, 宽, 高)`；缩放前先裁掉包围盒里的空白边（模块文档 §四.1）。
fn 裁内容(源: &[u8], 源宽: u32, 内容: (u32, u32, u32, u32)) -> (Vec<u8>, u32, u32) {
    let (cx0, cy0, cx1, cy1) = 内容;
    let 宽 = cx1 - cx0 + 1;
    let 高 = cy1 - cy0 + 1;
    (裁剪(源, 源宽, cx0, cy0, 宽, 高), 宽, 高)
}

/// 从整幅里裁出 `(x, y, w, h)`（越界部分用全透明补齐，不 panic）。
fn 裁剪(src: &[u8], 源宽: u32, x: u32, y: u32, w: u32, h: u32) -> Vec<u8> {
    let mut out = vec![0u8; w as usize * h as usize * 4];
    for row in 0..h {
        for col in 0..w {
            let si = ((y + row) as usize * 源宽 as usize + (x + col) as usize) * 4;
            let di = (row as usize * w as usize + col as usize) * 4;
            out[di..di + 4].copy_from_slice(&src[si..si + 4]);
        }
    }
    out
}

/// 全部格的相对坐标包围盒 `(min_x, min_y, max_x, max_y)`（闭区间；调用方保证非空）。
fn 格包围盒(cells: &[PositionedCell]) -> (i32, i32, i32, i32) {
    let 首 = cells[0];
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (首.dx, 首.dy, 首.dx, 首.dy);
    for c in &cells[1..] {
        min_x = min_x.min(c.dx);
        min_y = min_y.min(c.dy);
        max_x = max_x.max(c.dx);
        max_y = max_y.max(c.dy);
    }
    (min_x, min_y, max_x, max_y)
}

/// 扫描 alpha 通道求**实际有像素**的包围盒；一个像素都没有 ⇒ `None`。
fn 内容包围盒(rgba: &[u8], w: u32, h: u32) -> Option<(u32, u32, u32, u32)> {
    let mut x0 = u32::MAX;
    let mut y0 = u32::MAX;
    let mut x1 = 0u32;
    let mut y1 = 0u32;
    let mut 有 = false;
    for y in 0..h {
        let 行 = y as usize * w as usize * 4;
        for x in 0..w {
            if rgba[行 + x as usize * 4 + 3] != 0 {
                有 = true;
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
    }
    有.then_some((x0, y0, x1, y1))
}

/// 0×0 空图（模块文档 §五）。
fn 空图(cells_drawn: usize, frames_decoded: usize, missing: usize) -> StampImage {
    StampImage {
        rgba: Vec::new(),
        width: 0,
        height: 0,
        content: (0, 0, 0, 0),
        cells_drawn,
        frames_decoded,
        missing,
    }
}
