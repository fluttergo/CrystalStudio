//! 画布视口：世界像素 ↔ 屏幕像素的换算、重渲调度、纹理缓存。
//!
//! # 一、坐标口径（两层，别混）
//!
//! | 层 | 单位 | 例 |
//! |---|---|---|
//! | **世界像素** | 地图位图的像素，1 格 = 48×32（[`crate::metrics::CELL_W/CELL_H`]）| 380 格 ⇒ x = 18240 |
//! | **屏幕像素** | 窗口里的逻辑像素（本程序强制 1 逻辑 = 1 物理）| 画布左上角 = (156,105) |
//!
//! `pan` 存的是**视口左上角对应的世界像素**（可为负，负 = 看地图左侧空白），
//! 于是任意点的换算只有一组式子：
//!
//! ```text
//! 屏幕 = 画布左上角 + (世界 - pan) * scale
//! 世界 = pan + (屏幕 - 画布左上角) / scale
//! ```
//!
//! # 二、重渲纪律（本单的性能护栏）
//!
//! **绝不每帧重渲**。`miredit_render::render_region` 出的是 1:1 位图（1× 下
//! 987×815 的画布就是 3.2 MB，0.25× 下还要 ×16），每帧重算再上传必然掉帧。
//! 本模块的做法：
//!
//! 1. 缓存一块**比可见区大一圈**的纹理（四周各留
//!    `metrics::CANVAS_RENDER_MARGIN_CELLS` = 10 格）。这 10 格不是"为了拖动顺滑"
//!    拍出来的，而是**实测出来的下限**：`miredit-render` 只画窗口内的格子，
//!    而表层物件是整帧底边对齐贴的，城门楼那种一帧 300~500 px 的大件，
//!    锚点会在可视区外 6~9 格；余量只有 3 格时与客户端同视点命中率 **79.77%**，
//!    10 格时 **99.96%**（实测表见 `metrics::CANVAS_RENDER_MARGIN_CELLS`）。
//! 2. 每帧只做一次**包含判定**：可见格是否仍落在缓存区内 + 图层开关/地图代号是否没变
//!    + **纹理过滤档是否没变**（[`FilterClass`]，见 §三）+ **阻挡标记通道是否没变**
//!      （[`CollisionOverlay`]，见 §五）+ **开启门是否没变**（见 §六）；
//! 3. 只有判定失败才真调 `render_region`，并把新纹理整个换掉；
//! 4. 画的时候**永远把整块缓存纹理按它的世界位置贴**，视口裁切由 painter 的 clip 完成
//!    —— 这样平移就只是"同一张纹理挪个位置"，不需要重渲。
//!
//! 平移 2 秒（`--autodrag`）的实测见交付汇报：`render_region` 调用次数远小于帧数。
//!
//! # 三、缩放
//!
//! `render_region` 的 `scale` **恒为 1.0**（它只支持 1.0，见 `miredit-render/lib.rs` §四）；
//! 缩放只体现在"贴图矩形的尺寸"上。过滤方式：
//! - **整数倍**（1×、2×、4×…）用 [`egui::TextureOptions::NEAREST`]：一个世界像素正好铺满
//!   整数个屏幕像素，最近邻才不会把 1px 网格糊成灰边；
//! - **非整数倍**用 [`egui::TextureOptions::LINEAR`]：此时本来就不是 1:1，最近邻会造成
//!   明显的像素跳变（马赛克抖动），线性采样更接近"缩略"的观感。
//!
//! # 四、缩放/平移的取整纪律
//!
//! `pan` 每次变动后都吸附到「屏幕整数像素」上（`pan * scale` 取整再除回），
//! 于是 **1× 时 pan 恒为整数世界像素**（像素级 1:1，截图比对才可能逐点对上），
//! 4× 时步进 0.25 世界像素（= 1 屏幕像素，拖动不跳字）。
//!
//! # 五、阻挡标记通道（审计 P0-9「画完不知道能不能走人」的收口）
//!
//! 低位/高位阻挡是**格子属性**，不是"第四/第五层精灵"，所以它不进 [`LayerMask`]，
//! 而走渲染库的独立通道 [`CollisionOverlay`]（`low` / `high` / `alpha`）：
//!
//! | 通道 | 判据 | 出处 |
//! |---|---|---|
//! | 低阻挡 | `bk_img & 0x8000` | `TMap.cpp:339` / `mir2-map/src/cell.rs` 的 `is_walkable` |
//! | 高位阻挡 | `fr_img & 0x8000` | 同上 |
//!
//! 三条纪律：
//! 1. **默认全关** ⇒ 传给渲染库的是 [`CollisionOverlay::default()`]，
//!    渲染库对"全关"整段短路 ⇒ 画布与上一版**逐字节相同**；
//! 2. **必须进缓存键**：切眼睛时视口/地图都没变，键里少这一项就不会重渲 ⇒ 假交互；
//! 3. **统计独立**：标记只进 `RenderStats::cells_low_blocked` / `cells_high_blocked`，
//!    不进 `cells_drawn`（后者口径始终是三层精灵的落笔）。
//!
//! # 六、渲染选项（开启门 + 地图光照，2026-11-05 接线）
//!
//! `视图 → 开启门` / `视图 → 地图光照` / `--open-doors` / `--lighting` /
//! `--menu-click view.light_*` 这些入口翻的是宿主字段
//! `crate::app::MireditApp::doors_open` 与 `crate::app::MireditApp::lighting`；
//! 本模块把它们打包成**一整个 [`RenderOptions`]** 当参数收
//! （[`CanvasView::ensure`] 的 `options`），并做两件事：
//!
//! | 作用 | 判据 |
//! |---|---|
//! | 进缓存键 | `Cache::options != options` ⇒ 重渲（`render_calls + 1`）|
//! | 进渲染选项 | 同一个 `options` 原样传给 `try_render_region_with_options` |
//!
//! 为什么两件事都必须做、且**用同一个值**：门开合只改前景层帧号、光照只叠遮罩，
//! 视口、图层、阻挡通道、过滤档、地图代号**一个都没变**：
//! 只传渲染不进键 ⇒ 切档不重渲（画面不动）；只进键不传渲染 ⇒ 重渲了还是旧档（白渲）。
//! `RenderOptions` 是 `Copy + Eq + Hash`，整体比较天然覆盖「以后再加第三个 option」。
//!
//! ⚠ **光照必须进缓存键**（本轮最容易漏的一步，见验收 4）：切到夜晚档时视口一格没变，
//! 键里少了 `lighting` 就只会留着白天的纹理 ⇒ 「勾了夜晚、画面还是白天」的假交互。
//!
//! 默认 [`RenderOptions::default()`] = 闭门 + 白天 ⇒ 与改动前**逐字节相同**
//! （白天档连遮罩都不画，实跑 `0.map` 64×48 的 FNV-1a64 仍是 `a924aa9dedc74183`）。
//!
//! ✅ **导出（`crate::export`）已并入本口径**（2026-11-05 收口）：那两条路径
//! （导出选区 / 导出小地图）现在也收整个 `RenderOptions` 并走
//! `try_render_region_scaled_with` ⇒ 「画布切了夜晚/开了门、导出却是白天闭门」的
//! 不一致**已经不存在**；缺省仍是 `RenderOptions::default()` ⇒ 与接线前逐字节相同。
//! ⚠ 导出侧的扩边也必须按同一份选项算（`export::tile_margins` 收同一个 `options`），
//! 否则开门态下分块拼接会破 —— 理由见 `crate::export` 的模块文档 §六。

use egui::{Context, Pos2, Rect, TextureHandle, TextureOptions, Vec2};
use mir2_assets::SpriteSource;
use mir2_map::Map;
use miredit_render::{
    CollisionOverlay, LayerMask, RenderOptions, RenderStats, Viewport, CELL_H, CELL_W,
};

use crate::metrics as m;
use crate::ui_text as tx;

/// 纹理过滤档：**整数倍 NEAREST / 非整数倍 LINEAR** 的二值化（见模块文档 §三）。
///
/// 为什么把它单独提出来、还要放进缓存键（审计 E3）：
/// `TextureOptions` **只在 `ctx.load_texture` 那一刻生效** —— 纹理一旦建好就带着当初那档
/// 过滤方式。`egui 0.33` 里唯一的"事后改"接口是
/// `TextureHandle::set(&mut self, image: impl Into<ImageData>, options: TextureOptions)`
/// （`epaint-0.33.3/src/texture_handle.rs`），它内部走 `ImageDelta::full(...)`：
/// **必须把整张图再传一遍** ⇒ 等于重传整块缓存纹理。而这块纹理不小：
/// 实测 1× 时 1968×1504 px ≈ 11.8 MB、0.25× 时约 78 MB（`--shot` 日志里的重渲尺寸），
/// 比重新渲一次还贵，而且要把 `ColorImage` 一直留着（再多一份内存）。
/// 所以"缩放档变了 ⇒ 过滤档变了 ⇒ 必须重渲一次"这条不变式**由缓存键表达**，
/// 否则过滤方式会永远停在首帧那一档：以 1× 起手放大到 1.5× 全程 NEAREST（马赛克抖动），
/// 以 0.25×（`--fit` / Fit Map）起手放大到 1× 全程 LINEAR（像素级 1:1 时发糊）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterClass {
    /// 整数倍（1×/2×/4×…）：一个世界像素正好铺满整数个屏幕像素。
    Nearest,
    /// 非整数倍：最近邻会像素跳变，线性采样更接近"缩略"观感。
    Linear,
}

/// 一块已渲染的纹理 + 它覆盖的世界格窗口。
struct Cache {
    /// 这块纹理覆盖的世界格窗口（`scale` 恒 1.0）。
    vp: Viewport,
    /// GPU 纹理。
    tex: TextureHandle,
    /// 生成它时的图层开关（开关变了必须重渲）。
    mask: LayerMask,
    /// 生成它时的**阻挡标记通道**（切低/高位阻挡必须重渲，见 §五）。
    overlay: CollisionOverlay,
    /// 生成它时的**整份渲染选项**（门开合改前景层帧号、光照叠全屏遮罩 ⇒
    /// 任一档位变了都必须重渲，见 §六）。整体比较而不是逐字段：以后再加 option
    /// 不用再改缓存键。
    options: RenderOptions,
    /// 生成它时的地图代号（换图必须重渲）。
    map_gen: u64,
    /// 生成它时的**过滤档**（缩放跨过整数倍/非整数倍界线时必须重渲，见 [`FilterClass`]）。
    filter: FilterClass,
    /// 生成它时**真正传给 `load_texture` 的 `TextureOptions`**。
    ///
    /// 单独存一份而不是由 `filter` 推：验收要断言"上屏纹理真的拿到了 NEAREST"，
    /// 而这个字段就是那一刻传进去的那个值（推出来的不算证据）。
    loaded: TextureOptions,
}

/// 画布视口状态（平移/缩放/图层开关 + 纹理缓存 + 统计）。
pub struct CanvasView {
    /// 视口左上角对应的世界像素 X。
    pub pan_x: f32,
    /// 视口左上角对应的世界像素 Y。
    pub pan_y: f32,
    /// 缩放（0.25..=4.0，1.0 = 100%）。
    pub scale: f32,
    /// 三层开关（地表/中层/物件）。
    pub mask: LayerMask,
    /// **阻挡标记通道**（低位阻挡 / 高位阻挡各自独立）。
    ///
    /// 默认两个通道都关 ⇒ 传给渲染库的就是 [`CollisionOverlay::default()`]，
    /// 而渲染库对"全关"是**整段短路**的（`compose::render` 里 `overlay.any()`），
    /// 于是画布与"没有这个功能"逐字节相同（见 `miredit-render/lib.rs` §7.4.2）。
    ///
    /// 它**不在** [`LayerMask`] 里：那是 `miredit-render` 的"三层精灵"开关，
    /// 阻挡标记是独立通道（口径见 `miredit-render/src/collision.rs` 模块文档）。
    pub overlay: CollisionOverlay,
    /// 纹理缓存（无地图/无图库时为 `None`）。
    cache: Option<Cache>,
    /// 最近一次重渲的统计（界面上不显示，诊断与验收用）。
    pub stats: RenderStats,
    /// `render_region` 真实调用次数（验收要的数字，**只在这里 +1**）。
    pub render_calls: u64,
    /// 最近一次重渲耗时（毫秒）。
    pub last_render_ms: f32,
    /// 是否因为"视口格数超上限"而跳过了上一次重渲。
    pub budget_hit: bool,
}

impl Default for CanvasView {
    fn default() -> Self {
        Self {
            pan_x: 0.0,
            pan_y: 0.0,
            scale: m::CANVAS_DEFAULT_SCALE,
            mask: LayerMask::all(),
            // 默认**不画**阻挡标记（与上一版逐字节一致）
            overlay: CollisionOverlay::default(),
            cache: None,
            stats: RenderStats::default(),
            render_calls: 0,
            last_render_ms: 0.0,
            budget_hit: false,
        }
    }
}

impl CanvasView {
    /// 丢弃纹理缓存（换图 / 换图库时调）。
    pub fn drop_cache(&mut self) {
        self.cache = None;
    }

    /// 把视口中心对准某个世界格。
    pub fn center_on_cell(&mut self, canvas: Rect, cx: f32, cy: f32) {
        let world_x = (cx + 0.5) * CELL_W as f32;
        let world_y = (cy + 0.5) * CELL_H as f32;
        self.pan_x = world_x - canvas.width() * 0.5 / self.scale;
        self.pan_y = world_y - canvas.height() * 0.5 / self.scale;
        self.snap();
    }

    /// 「适应窗口」：整张图缩到可见区里，并居中。
    ///
    /// 缩放夹在 `CANVAS_MIN_SCALE..=CANVAS_MAX_SCALE`：700×700 的图在 987×815 的画布里
    /// 只需 0.25×（3072 px 宽 / 987），再小就没意义了（夹到 0.25 后会有滚动余量，如实可见）。
    pub fn fit_map(&mut self, canvas: Rect, w: u16, h: u16) {
        if w == 0 || h == 0 || canvas.width() <= 1.0 || canvas.height() <= 1.0 {
            return;
        }
        let map_w = w as f32 * CELL_W as f32;
        let map_h = h as f32 * CELL_H as f32;
        let sx = canvas.width() / map_w;
        let sy = canvas.height() / map_h;
        self.scale = sx.min(sy).clamp(m::CANVAS_MIN_SCALE, m::CANVAS_MAX_SCALE);
        // 居中：整幅图在画布里的左右/上下留白相等
        self.pan_x = (map_w - canvas.width() / self.scale) * 0.5;
        self.pan_y = (map_h - canvas.height() / self.scale) * 0.5;
        self.snap();
    }

    /// 平移（`delta_screen` 是**屏幕像素**，方向与鼠标移动一致）。
    pub fn pan_by(&mut self, delta_screen: Vec2) {
        self.pan_x -= delta_screen.x / self.scale;
        self.pan_y -= delta_screen.y / self.scale;
        self.snap();
    }

    /// 以 `anchor`（屏幕坐标）为中心缩放 `factor` 倍：锚点下的世界点保持不动。
    pub fn zoom_at(&mut self, canvas: Rect, anchor: Pos2, factor: f32) {
        let new = (self.scale * factor).clamp(m::CANVAS_MIN_SCALE, m::CANVAS_MAX_SCALE);
        if (new - self.scale).abs() < 1e-6 {
            return;
        }
        let local = anchor - canvas.min;
        let wx = self.pan_x + local.x / self.scale;
        let wy = self.pan_y + local.y / self.scale;
        self.scale = new;
        self.pan_x = wx - local.x / new;
        self.pan_y = wy - local.y / new;
        self.snap();
    }

    /// 吸附到屏幕整数像素（见模块文档 §四）。
    fn snap(&mut self) {
        if self.scale <= 0.0 {
            return;
        }
        self.pan_x = (self.pan_x * self.scale).round() / self.scale;
        self.pan_y = (self.pan_y * self.scale).round() / self.scale;
    }

    /// 当前可见的世界格窗口 `(原点x, 原点y, 宽, 高)`。
    pub fn visible_cells(&self, canvas: Rect) -> (i32, i32, u32, u32) {
        if canvas.width() < 1.0 || canvas.height() < 1.0 || self.scale <= 0.0 {
            return (0, 0, 0, 0);
        }
        let wl = self.pan_x;
        let wt = self.pan_y;
        let wr = wl + canvas.width() / self.scale;
        let wb = wt + canvas.height() / self.scale;
        let x0 = (wl / CELL_W as f32).floor() as i32;
        let y0 = (wt / CELL_H as f32).floor() as i32;
        let x1 = (wr / CELL_W as f32).ceil() as i32;
        let y1 = (wb / CELL_H as f32).ceil() as i32;
        (x0, y0, (x1 - x0).max(0) as u32, (y1 - y0).max(0) as u32)
    }

    /// 重渲余量的格数（固定 [`m::CANVAS_RENDER_MARGIN_CELLS`]，理由见该常量的实测表）。
    ///
    /// 注意它**不随缩放变**：余量要盖住的是"帧体尺寸"（世界像素），
    /// 与当前缩放无关 —— 缩放只影响"同一块余量在屏幕上占多少像素"。
    fn margin_cells(&self) -> u32 {
        m::CANVAS_RENDER_MARGIN_CELLS
    }

    /// 纹理过滤方式（整数倍最近邻 / 非整数倍线性，见模块文档 §三）。
    fn texture_options(&self) -> TextureOptions {
        match self.filter_class() {
            FilterClass::Nearest => TextureOptions::NEAREST,
            FilterClass::Linear => TextureOptions::LINEAR,
        }
    }

    /// 当前缩放档对应的过滤档（缓存键的一部分）。
    fn filter_class(&self) -> FilterClass {
        let rounded = self.scale.round();
        if (self.scale - rounded).abs() < 1e-4 {
            FilterClass::Nearest
        } else {
            FilterClass::Linear
        }
    }

    /// 保证纹理能覆盖当前可见区；必要时才真调 `render_region`。
    ///
    /// 没有地图或没有图库时清空缓存（画布会走空态分支，不留上一张图的残影）。
    ///
    /// `options` = **整份渲染选项**（`视图 → 开启门` / `视图 → 地图光照` /
    /// `--open-doors` / `--lighting` 的真源 `app::MireditApp::doors_open` +
    /// `app::MireditApp::lighting`）。它有两个作用，缺一不可：
    /// ① 进**缓存键**（见 §六：门开合只改前景层帧号、光照只叠遮罩，视口/图层/地图
    ///    一个都没变，键里少了它就不会重渲 ⇒ 切档画面不动，是假交互）；
    /// ② 原样传给 `miredit_render::try_render_region_with_options`（**不能用**
    ///    `try_render_region_with`：那个入口写死 `RenderOptions::default()` =
    ///    闭门 + 白天，会把门态与光照一起丢掉）。
    pub fn ensure(
        &mut self,
        ctx: &Context,
        map: Option<&Map>,
        src: Option<&dyn SpriteSource>,
        canvas: Rect,
        map_gen: u64,
        options: RenderOptions,
    ) {
        let (Some(map), Some(src)) = (map, src) else {
            self.drop_cache();
            return;
        };
        let (vx, vy, vw, vh) = self.visible_cells(canvas);
        if vw == 0 || vh == 0 {
            return;
        }
        // ① 缓存命中判定：覆盖 + 图层 + **阻挡通道** + 地图代号 + **过滤档** + **渲染选项**
        //
        // ⚠ 过滤档必须在键里（审计 E3）：`texture_options()` 只在 `load_texture` 时生效，
        // 少了这一项，缩放从 1.5× 跳到 4× 时（可见格变少 ⇒ 覆盖判定仍成立）就不会重渲，
        // 上屏纹理永远停在 1.5× 那档 LINEAR。
        //
        // ⚠ 阻挡通道也必须在键里（审计 P0-9）：切"低位阻挡"眼睛时视口一格都没变、
        // 地图代号也没变，少了这一项就**不会重渲** ⇒ 眼睛点了、画布没反应（假交互）。
        //
        // ⚠ 整份 `RenderOptions` 必须在键里（2026-11-05 接线）：门开合只改**前景层帧号**
        // （`帧号 += door_offset & 0x7F`）、光照只叠**全屏遮罩 + 逐格光罩**，两者都不动
        // 视口/图层/阻挡/过滤档/地图代号 ⇒ 少了这一项，勾了「开启门」或切到「夜晚」
        // 画面上一个像素都不会动（`render_calls` 也不涨）。
        let up_to_date = match &self.cache {
            Some(c) => {
                c.map_gen == map_gen
                    && c.mask == self.mask
                    && c.overlay == self.overlay
                    && c.filter == self.filter_class()
                    && c.options == options
                    && c.vp.origin_x <= vx
                    && c.vp.origin_y <= vy
                    && c.vp.origin_x + c.vp.cells_w as i32 >= vx + vw as i32
                    && c.vp.origin_y + c.vp.cells_h as i32 >= vy + vh as i32
            }
            None => false,
        };
        if up_to_date {
            return;
        }

        // ② 预算护栏（正常窗口不会触发，见 metrics 里的推导）
        let mg = self.margin_cells() as i32;
        let (ox, oy) = (vx - mg, vy - mg);
        let cw = vw + 2 * mg as u32;
        let ch = vh + 2 * mg as u32;
        if u64::from(cw) * u64::from(ch) > u64::from(m::CANVAS_RENDER_MAX_CELLS) {
            if !self.budget_hit {
                self.budget_hit = true;
                eprintln!(
                    "{}",
                    tx::fill(
                        tx::DIAG_RENDER_BUDGET,
                        &[
                            ("cells", &(u64::from(cw) * u64::from(ch)).to_string()),
                            ("max", &m::CANVAS_RENDER_MAX_CELLS.to_string()),
                        ]
                    )
                );
            }
            return;
        }
        self.budget_hit = false;

        // ③ 真渲一次
        //
        // ⚠ 必须用 **`try_render_region_with_options` + `self.mask` + `self.overlay`
        // + 调用方递来的整份 `options`**，
        // 不能用 `render_region` / `try_render_region_with`：前两个是"三层全开、
        // 不画阻挡、**门恒闭 + 恒白天**"的便捷入口（内部写死 `LayerMask::all()`、
        // `CollisionOverlay::default()` 与 `RenderOptions::default()`），
        // 拿它们做图层开关/开门/切光照会得到"开关点了、图没变"的假交互
        // （图层实测：四种开关组合出的图像素差只有 38~76 px，等于没生效；
        //  门实测：`0.map` 的 `(106,316)` 起 3×3 窗口有 4662/13824 个像素不同）。
        let vp = Viewport::new(ox, oy, cw, ch);
        let t0 = std::time::Instant::now();
        let out = match miredit_render::try_render_region_with_options(
            map,
            src,
            &vp,
            &self.mask,
            self.overlay,
            options,
        ) {
            Ok(o) => o,
            Err(e) => {
                // 视口/缩放非法（正常路径不会走到，`visible_cells` 已挡住 0 格与越界）
                eprintln!("[渲染] 跳过（{e}）");
                return;
            }
        };
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        self.render_calls += 1;
        self.stats = out.stats;
        self.last_render_ms = ms as f32;

        let img = egui::ColorImage::from_rgba_unmultiplied(
            [out.width as usize, out.height as usize],
            &out.rgba,
        );
        // 取一次、两处用：既传给 `load_texture`，也记进缓存（验收要断言的就是"传进去的那个值"）
        let opts = self.texture_options();
        let tex = ctx.load_texture("miredit-canvas", img, opts);

        if std::env::var_os("MIREDIT_QUIET").is_none() {
            eprintln!(
                "{}",
                tx::fill(
                    tx::DIAG_RENDER,
                    &[
                        ("n", &self.render_calls.to_string()),
                        ("cw", &cw.to_string()),
                        ("ch", &ch.to_string()),
                        ("pw", &out.width.to_string()),
                        ("ph", &out.height.to_string()),
                        ("ox", &ox.to_string()),
                        ("oy", &oy.to_string()),
                        ("ms", &format!("{ms:.1}")),
                        // 门态 + 光照档位必须打进诊断：验收要能**从日志**看出"这一次重渲
                        // 到底用的哪一组选项"（`--menu-click view.light_night@40` 那次会话
                        // 里会出现两条，一条 lighting=Day、一条 lighting=Night —— 这就是
                        // "切档 ⇒ 重渲"的实跑证据）。`MapLighting::name()` 是稳定短名
                        // （day / dawn-dusk / night），`Debug` 名恰好是 DawnDusk/Day/Night。
                        ("doors", if options.doors_open { "1" } else { "0" }),
                        ("lighting", &format!("{:?}", options.lighting)),
                        ("stats", &out.stats.to_string()),
                    ]
                )
            );
        }

        self.cache = Some(Cache {
            vp,
            tex,
            mask: self.mask,
            overlay: self.overlay,
            options,
            map_gen,
            filter: self.filter_class(),
            loaded: opts,
        });
    }

    /// 缓存纹理建立时**真正传给 `load_texture` 的过滤方式**（`None` = 还没渲过）。
    ///
    /// 这是 E3 的取证口：只测纯函数 `texture_options()` 是空转（就算 `ensure` 从不使用它
    /// 也不会红），必须能读到"上屏纹理拿到的到底是哪一档"。
    pub fn loaded_options(&self) -> Option<TextureOptions> {
        self.cache.as_ref().map(|c| c.loaded)
    }

    /// 缓存纹理建立时**传进渲染库的整份选项**（`None` = 还没渲过）。
    ///
    /// 与 [`Self::loaded_options`] 同一套取证口径：验收要断言"上屏那张纹理真的是按
    /// 开门帧 / 夜晚档渲的"，而这个字段就是那一刻原样传给
    /// `try_render_region_with_options` 的值（门态 + 光照档位一起，不许拆成两个取证口）。
    pub fn cached_render_options(&self) -> Option<RenderOptions> {
        self.cache.as_ref().map(|c| c.options)
    }

    /// 当前缓存纹理（没渲过就 `None`）。
    pub fn texture(&self) -> Option<&TextureHandle> {
        self.cache.as_ref().map(|c| &c.tex)
    }

    /// 缓存纹理覆盖的世界格窗口（验收/诊断用）。
    pub fn cached_viewport(&self) -> Option<Viewport> {
        self.cache.as_ref().map(|c| c.vp)
    }

    /// 缓存纹理贴到屏幕上的目标矩形 + uv（整块贴，裁切交给 painter）。
    pub fn texture_dest(&self, canvas: Rect) -> Option<(Rect, Rect)> {
        let c = self.cache.as_ref()?;
        let wx = c.vp.origin_x as f32 * CELL_W as f32;
        let wy = c.vp.origin_y as f32 * CELL_H as f32;
        let ww = c.vp.cells_w as f32 * CELL_W as f32;
        let wh = c.vp.cells_h as f32 * CELL_H as f32;
        let min = canvas.min
            + Vec2::new(
                (wx - self.pan_x) * self.scale,
                (wy - self.pan_y) * self.scale,
            );
        Some((
            Rect::from_min_size(min, Vec2::new(ww * self.scale, wh * self.scale)),
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
        ))
    }

    /// 屏幕点 → 世界格（不做地图范围判定，越界由调用方用 `Map::cell` 兜）。
    pub fn screen_to_cell(&self, canvas: Rect, pos: Pos2) -> Option<(i32, i32)> {
        if !canvas.contains(pos) {
            return None;
        }
        let wx = self.pan_x + (pos.x - canvas.left()) / self.scale;
        let wy = self.pan_y + (pos.y - canvas.top()) / self.scale;
        Some((
            (wx / CELL_W as f32).floor() as i32,
            (wy / CELL_H as f32).floor() as i32,
        ))
    }

    /// 某个世界格在屏幕上的矩形（悬停高亮用）。
    pub fn cell_rect(&self, canvas: Rect, x: i32, y: i32) -> Rect {
        let min = canvas.min
            + Vec2::new(
                (x as f32 * CELL_W as f32 - self.pan_x) * self.scale,
                (y as f32 * CELL_H as f32 - self.pan_y) * self.scale,
            );
        Rect::from_min_size(
            min,
            Vec2::new(CELL_W as f32 * self.scale, CELL_H as f32 * self.scale),
        )
    }

    /// 缩放百分比（四舍五入到整数，界面显示用）。
    pub fn zoom_percent(&self) -> i32 {
        (self.scale * 100.0).round() as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mir2_assets::{Frame, LibRef};
    use miredit_render::MapLighting;

    fn canvas() -> Rect {
        Rect::from_min_size(Pos2::new(156.0, 105.0), Vec2::new(987.0, 815.0))
    }

    /// 1× 时 pan 必须是整数世界像素（否则截图逐点比对永远对不齐）。
    #[test]
    fn 一倍的平移吸附到整像素() {
        let mut v = CanvasView::default();
        v.pan_by(Vec2::new(10.4, 7.6));
        assert_eq!(v.pan_x.fract(), 0.0, "1× 下 pan_x 应为整数：{}", v.pan_x);
        assert_eq!(v.pan_y.fract(), 0.0, "1× 下 pan_y 应为整数：{}", v.pan_y);
        // 往右下拖 10.4/7.6 ⇒ 内容跟手 ⇒ pan 变小
        assert!((v.pan_x - -10.0).abs() < 1e-6, "实际 {}", v.pan_x);
        assert!((v.pan_y - -8.0).abs() < 1e-6, "实际 {}", v.pan_y);
    }

    /// 缩放锚点不动：把鼠标停在 (500,300) 上放大，该点下的世界像素不变。
    #[test]
    fn 缩放锚点保持不动() {
        let c = canvas();
        let mut v = CanvasView {
            pan_x: 1000.0,
            pan_y: 2000.0,
            ..Default::default()
        };
        let anchor = Pos2::new(500.0, 300.0);
        let before = (
            v.pan_x + (anchor.x - c.left()) / v.scale,
            v.pan_y + (anchor.y - c.top()) / v.scale,
        );
        v.zoom_at(c, anchor, 2.0);
        let after = (
            v.pan_x + (anchor.x - c.left()) / v.scale,
            v.pan_y + (anchor.y - c.top()) / v.scale,
        );
        assert!((before.0 - after.0).abs() <= 1.0, "X 锚点漂了：{before:?} → {after:?}");
        assert!((before.1 - after.1).abs() <= 1.0, "Y 锚点漂了：{before:?} → {after:?}");
        assert!((v.scale - 2.0).abs() < 1e-6, "缩放应为 2×：{}", v.scale);
    }

    /// 缩放被夹在 0.25..=4.0（滚轮连滚不会跑飞）。
    #[test]
    fn 缩放上下限() {
        let c = canvas();
        let mut v = CanvasView::default();
        for _ in 0..40 {
            v.zoom_at(c, c.center(), 1.15);
        }
        assert!((v.scale - m::CANVAS_MAX_SCALE).abs() < 1e-6, "上限 {}", v.scale);
        for _ in 0..60 {
            v.zoom_at(c, c.center(), 0.87);
        }
        assert!((v.scale - m::CANVAS_MIN_SCALE).abs() < 1e-6, "下限 {}", v.scale);
        assert_eq!(v.zoom_percent(), 25);
    }

    /// 屏幕点 ↔ 世界格的往返一致（1× / 4× / 0.25× 各测一次）。
    #[test]
    fn 屏幕世界往返() {
        let c = canvas();
        for scale in [1.0_f32, 4.0, 0.25] {
            let v = CanvasView {
                scale,
                pan_x: 18240.0,
                pan_y: 9920.0,
                ..Default::default()
            };
            // 指到世界像素 (18264, 9936) = 世界格 (380,310) 的中心附近；
            // 屏幕偏移要随缩放换算，否则 0.25× 时同一个屏幕点会落到 (382,312)。
            let p = Pos2::new(
                c.left() + 24.0 * scale,
                c.top() + 16.0 * scale,
            );
            let (cx, cy) = v.screen_to_cell(c, p).expect("画布内应能换算");
            let r = v.cell_rect(c, cx, cy);
            assert!(
                r.contains(p),
                "scale={scale} 时 {p:?} 应落在格 ({cx},{cy}) 的矩形 {r:?} 里"
            );
            assert_eq!((cx, cy), (380, 310), "scale={scale} 换算的格不对");
        }
    }

    /// 画布外的点不换算成格（否则状态栏会显示一个根本不存在的坐标）。
    #[test]
    fn 画布外不换算() {
        let c = canvas();
        let v = CanvasView::default();
        assert!(v.screen_to_cell(c, Pos2::new(10.0, 10.0)).is_none());
        assert!(v.screen_to_cell(c, Pos2::new(c.right() + 1.0, 200.0)).is_none());
    }

    /// 「适应窗口」后整张图的四周都在画布内（不留半张图在外面）。
    #[test]
    fn 适应窗口整图可见() {
        let c = canvas();
        let mut v = CanvasView::default();
        v.fit_map(c, 64, 48);
        let (vx, vy, vw, vh) = v.visible_cells(c);
        assert!(vx <= 0, "左边界应露出图外：{vx}");
        assert!(vy <= 0, "上边界应露出图外：{vy}");
        assert!(
            vx + vw as i32 >= 64,
            "右边界应覆盖到 64 格：{}",
            vx + vw as i32
        );
        assert!(
            vy + vh as i32 >= 48,
            "下边界应覆盖到 48 格：{}",
            vy + vh as i32
        );
    }

    /// 重渲余量必须 ≥ 10 格（实测阈值：3 格时同视点命中率只有 79.77%，10 格 99.96%）。
    ///
    /// 这条断言是**防回归**的：把余量改小会让城门楼那种大件整块消失，
    /// 而这种缺失在单元测试里看不出来，只能靠这一条把数字钉住。
    #[test]
    fn 重渲余量不小于实测阈值() {
        let v = CanvasView::default();
        assert_eq!(v.margin_cells(), m::CANVAS_RENDER_MARGIN_CELLS);
        assert!(
            v.margin_cells() >= 10,
            "余量降到 {} 格会让大物件缺失（实测 3 格命中率 79.77%）",
            v.margin_cells()
        );
        // 余量不随缩放变（要盖住的是世界像素尺寸，不是屏幕像素）
        let v2 = CanvasView {
            scale: 0.25,
            ..Default::default()
        };
        assert_eq!(v2.margin_cells(), v.margin_cells());
    }

    /// 纹理过滤：整数倍最近邻，非整数倍线性（注释里写明的规则必须真生效）。
    ///
    /// ⚠ 这条**只测纯函数**，历史上正是 E3 长期为绿的原因：`ensure` 从不使用
    /// `texture_options()` 它也不会红。真正守门的是下面那条
    /// [`缩放档变化会让缓存键失效并重渲`]。两条都留着：这条守"档位判定"本身，
    /// 那条守"判定结果真的作用到上屏纹理"。
    #[test]
    fn 整数倍用最近邻() {
        let mut v = CanvasView::default();
        for (scale, nearest) in [
            (1.0_f32, true),
            (2.0, true),
            (4.0, true),
            (0.25, false),
            (1.5, false),
        ] {
            v.scale = scale;
            let o = v.texture_options();
            let is_nearest = o.magnification == egui::TextureFilter::Nearest;
            assert_eq!(is_nearest, nearest, "scale={scale} 的过滤方式不对");
            assert_eq!(
                v.filter_class(),
                if nearest {
                    FilterClass::Nearest
                } else {
                    FilterClass::Linear
                },
                "scale={scale} 的缓存键过滤档不对"
            );
        }
    }

    /// 假帧源（两层都用同一份：地表 96×64 覆盖 2×2 格，中层 48×32）。
    struct 全有源;

    impl SpriteSource for 全有源 {
        fn frame(&self, lib: LibRef, _index: u32) -> Option<Frame> {
            let (w, h) = match lib.family {
                mir2_assets::Family::Ground => (96, 64),
                mir2_assets::Family::Middle => (48, 32),
                mir2_assets::Family::Surface => (48, 64),
            };
            Some(Frame {
                width: w,
                height: h,
                rgba: vec![255; (w * h) as usize * 4],
                // 本测试只验合成与缓存，与帧锚点无关 ⇒ 其余字段（含 anchor）走 Default，
                // 将来 Frame 再加字段时这里不必再改（见 mir2-assets/src/frame.rs 的 Default 文档）。
                ..Frame::default()
            })
        }
    }

    /// 2×2 三层都带图的合成图。
    fn 二层图() -> Map {
        let mut bytes = vec![0u8; 52 + 12 * 4];
        bytes[0..2].copy_from_slice(&2u16.to_le_bytes());
        bytes[2..4].copy_from_slice(&2u16.to_le_bytes());
        for i in 0..4 {
            let base = 52 + i * 12;
            bytes[base..base + 6].copy_from_slice(&[1, 0, 1, 0, 1, 0]);
        }
        Map::parse(&bytes).expect("合成图应能解析")
    }

    /// **缩放档变化 ⇒ 缓存键失效 ⇒ 重渲，且新纹理真的换到对的过滤档**（审计 E3）。
    ///
    /// 这条测试是**结构性的**，不是纯函数级别的：
    /// - 「缓存键含过滤档」被拆掉 ⇒ `1.5 → 4.0` 那一步不会重渲（放大后可见格变少，
    ///   覆盖判定仍然成立），`render_calls` 停在 1、`loaded_options()` 停在 LINEAR ⇒ **红**；
    /// - 「渲完把过滤档记进缓存」被拆掉 ⇒ `loaded_options()` 是 `None` ⇒ **红**。
    ///
    /// 它断言的是**上屏纹理那一刻真正拿到的 `TextureOptions`**（`Cache::loaded`），
    /// 而不是"再算一遍 `texture_options()`"（后者与实现同源，属于空转）。
    #[test]
    fn 缩放档变化会让缓存键失效并重渲() {
        use egui::TextureFilter;
        let map = 二层图();
        let src = 全有源;
        let ctx = egui::Context::default();
        let canvas = Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(96.0, 64.0));

        let mut v = CanvasView {
            scale: 1.5, // 非整数倍 ⇒ LINEAR
            ..Default::default()
        };
        v.ensure(&ctx, Some(&map), Some(&src), canvas, 1, RenderOptions::default());
        assert_eq!(v.render_calls, 1, "第一次必然要渲");
        assert_eq!(
            v.loaded_options().expect("渲完必须有缓存").magnification,
            TextureFilter::Linear,
            "1.5× 上屏纹理必须是 LINEAR"
        );

        // 同一档再调一次：什么都不该变（缓存要真命中，别每帧重渲）
        v.ensure(&ctx, Some(&map), Some(&src), canvas, 1, RenderOptions::default());
        assert_eq!(v.render_calls, 1, "缩放到同一档不该重渲");

        // **放大到 4×**：可见格变少 ⇒ 覆盖判定仍成立，唯一变化是过滤档 LINEAR → NEAREST
        v.scale = 4.0;
        v.ensure(&ctx, Some(&map), Some(&src), canvas, 1, RenderOptions::default());
        assert_eq!(
            v.render_calls, 2,
            "过滤档变了必须重渲（旧实现缓存键里没有 scale，这里会停在 1）"
        );
        assert_eq!(
            v.loaded_options().expect("重渲后应有缓存").magnification,
            TextureFilter::Nearest,
            "4× 上屏纹理必须换成 NEAREST"
        );
        assert_eq!(v.filter_class(), FilterClass::Nearest);

        // 再放大到另一整数倍（4× → 2× 是缩小，覆盖不再成立；改用 4× → 8× 越界，
        // 故这里退回到「1× → 1.5×」这一侧：整数倍 → 非整数倍，覆盖仍成立）
        v.scale = 1.0;
        v.ensure(&ctx, Some(&map), Some(&src), canvas, 1, RenderOptions::default());
        let after_nearest = v.render_calls;
        v.scale = 1.5; // 可见格变多一点点，但缓存余量 10 格足够覆盖
        v.ensure(&ctx, Some(&map), Some(&src), canvas, 1, RenderOptions::default());
        assert_eq!(
            v.render_calls,
            after_nearest + 1,
            "1× → 1.5× 也必须重渲（NEAREST → LINEAR）"
        );
        assert_eq!(
            v.loaded_options().unwrap().magnification,
            TextureFilter::Linear,
            "1.5× 上屏纹理必须换回 LINEAR"
        );
    }

    /// 「适应窗口」把缩放设成非整数倍后放大到 1×，纹理必须切到 NEAREST。
    ///
    /// 这是审计 E3 点名的另一条真实路径（`--fit` / Fit Map 起手：非整数倍起步全程 LINEAR，
    /// 放大到像素级 1:1 时发糊）。
    ///
    /// ⚠ 画布/地图都取小尺寸：egui 默认 `Context` 的纹理边长上限是 2048，
    /// 0.25× 下 987×815 的画布会渲出 4992×3904 的纹理（那是产品代码的真实上限问题，
    /// 与本条要验证的东西无关，别在这里炸）。
    #[test]
    fn 适应窗口后放大到一倍切到最近邻() {
        use egui::TextureFilter;
        // 20×20 的 Old12 图（每格都有地表图号 1），尺寸与 fit_map 的参数一致
        let mut bytes = vec![0u8; 52 + 12 * 400];
        bytes[0..2].copy_from_slice(&20u16.to_le_bytes());
        bytes[2..4].copy_from_slice(&20u16.to_le_bytes());
        for i in 0..400 {
            let base = 52 + i * 12;
            bytes[base..base + 2].copy_from_slice(&1u16.to_le_bytes());
        }
        let map = Map::parse(&bytes).expect("20×20 合成图应能解析");
        let src = 全有源;
        let ctx = egui::Context::default();
        let canvas = Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(300.0, 200.0));
        let mut v = CanvasView::default();
        v.fit_map(canvas, 20, 20);
        assert!(v.scale < 1.0, "20×20 的图在 300×200 画布里必须缩到 1× 以下");
        assert_ne!(
            v.filter_class(),
            FilterClass::Nearest,
            "适应窗口的倍数是非整数倍 ⇒ 必须是 LINEAR 档"
        );
        v.ensure(&ctx, Some(&map), Some(&src), canvas, 1, RenderOptions::default());
        assert_eq!(
            v.loaded_options().unwrap().magnification,
            TextureFilter::Linear,
            "适应窗口那一档必须是 LINEAR"
        );
        let calls = v.render_calls;
        v.scale = 1.0;
        v.ensure(&ctx, Some(&map), Some(&src), canvas, 1, RenderOptions::default()); // 放大 ⇒ 覆盖仍成立，只有过滤档变
        assert_eq!(v.render_calls, calls + 1, "1× 的过滤档不同，必须重渲");
        assert_eq!(
            v.loaded_options().unwrap().magnification,
            TextureFilter::Nearest,
            "放大到 1× 必须切回 NEAREST（否则像素级 1:1 发糊）"
        );
    }

    /// **图层开关必须真的改变渲染结果**。
    ///
    /// 这条测试是给一个真实踩过的坑守门的：`miredit_render::render_region` 是
    /// 「三层全开」的便捷入口（内部写死 `LayerMask::all()`），拿它做图层开关会得到
    /// "开关点了、图没变"的假交互（当时四种开关组合出的图像素差只有 38~76 px）。
    /// 必须走 `try_render_region` + `mask`。
    #[test]
    fn 图层开关真的改变渲染结果() {
        let map = 二层图();
        let src = 全有源;
        let ctx = egui::Context::default();
        let canvas = Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(96.0, 64.0));

        let mut all = CanvasView::default();
        all.ensure(&ctx, Some(&map), Some(&src), canvas, 1, RenderOptions::default());
        let all_decoded = all.stats.frames_decoded;

        let mut no_front = CanvasView::default();
        no_front.mask.front = false;
        no_front.ensure(&ctx, Some(&map), Some(&src), canvas, 1, RenderOptions::default());
        let no_front_decoded = no_front.stats.frames_decoded;

        assert!(
            all_decoded > no_front_decoded,
            "关掉物件层后解码帧数必须变少（全开 {all_decoded} vs 关物件 {no_front_decoded}）—— 相等说明 mask 没传进渲染"
        );
        assert_eq!(all.render_calls, 1);
        assert_eq!(no_front.render_calls, 1);
    }

    /// 带阻挡位的 2×2 图：`(0,0)` 低位阻挡 + `(1,0)` 高位阻挡（两个通道各有 1 格）。
    fn 阻挡图() -> Map {
        let mut bytes = vec![0u8; 52 + 12 * 4];
        bytes[0..2].copy_from_slice(&2u16.to_le_bytes());
        bytes[2..4].copy_from_slice(&2u16.to_le_bytes());
        // 列优先：下标 = x*2 + y
        let 格 = |x: usize, y: usize| 52 + (x * 2 + y) * 12;
        // (0,0)：地表图号 1 + 低位阻挡位
        bytes[格(0, 0)..格(0, 0) + 2].copy_from_slice(&(0x8000u16 | 1).to_le_bytes());
        // (1,0)：物件图号 1 + 高位阻挡位
        bytes[格(1, 0) + 4..格(1, 0) + 6].copy_from_slice(&(0x8000u16 | 1).to_le_bytes());
        Map::parse(&bytes).expect("合成图应能解析")
    }

    /// **阻挡通道必须进缓存键**（审计 P0-9：画完不知道能不能走人）。
    ///
    /// 切"低位阻挡"眼睛时视口/图层/地图代号**一个都没变**，缓存键里少 `overlay` 这一项
    /// 就不会重渲 ⇒ 眼睛点了、画布没反应（假交互）。这条把它钉死：
    /// - 开通道 ⇒ `render_calls +1` 且 `stats.cells_low_blocked > 0`；
    /// - 同一状态再调 ⇒ **不许**重渲；
    /// - 关通道 ⇒ 再 `+1`，且计数归零（"逐字节回到原图"由渲染库的短路保证，
    ///   GUI 侧的判据是"没有一格被标记"）。
    #[test]
    fn 阻挡通道进缓存键并触发重渲() {
        let map = 阻挡图();
        let src = 全有源;
        let ctx = egui::Context::default();
        let canvas = Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(96.0, 64.0));

        let mut v = CanvasView::default();
        assert!(
            !v.overlay.low && !v.overlay.high,
            "默认两个通道都关（与上一版逐字节一致的前提）"
        );
        v.ensure(&ctx, Some(&map), Some(&src), canvas, 1, RenderOptions::default());
        assert_eq!(v.render_calls, 1, "第一次必然要渲");
        assert_eq!(v.stats.cells_low_blocked, 0, "默认不得画任何低阻挡标记");
        assert_eq!(v.stats.cells_high_blocked, 0, "默认不得画任何高位阻挡标记");
        let 落笔 = v.stats.cells_drawn;

        // 同一状态再调：缓存必须命中
        v.ensure(&ctx, Some(&map), Some(&src), canvas, 1, RenderOptions::default());
        assert_eq!(v.render_calls, 1, "同一状态不得重渲（别每帧重算）");

        // 开低位阻挡 ⇒ 必须重渲，且 stats 真的数出标记
        v.overlay.low = true;
        v.ensure(&ctx, Some(&map), Some(&src), canvas, 1, RenderOptions::default());
        assert_eq!(
            v.render_calls, 2,
            "切阻挡通道必须重渲（缓存键里没有 overlay 时这里会停在 1）"
        );
        assert_eq!(v.stats.cells_low_blocked, 1, "(0,0) 带低位阻挡位");
        assert_eq!(v.stats.cells_high_blocked, 0, "高位通道还关着，一格都不该计");
        assert_eq!(
            v.stats.cells_drawn, 落笔,
            "标记是独立通道，不许改 cells_drawn 的口径"
        );

        // 同一状态再调：又不许重渲
        v.ensure(&ctx, Some(&map), Some(&src), canvas, 1, RenderOptions::default());
        assert_eq!(v.render_calls, 2, "通道状态没变就不许重渲");

        // 再开高位阻挡 ⇒ 再 +1，两个通道各自计数
        v.overlay.high = true;
        v.ensure(&ctx, Some(&map), Some(&src), canvas, 1, RenderOptions::default());
        assert_eq!(v.render_calls, 3);
        assert_eq!(v.stats.cells_low_blocked, 1);
        assert_eq!(v.stats.cells_high_blocked, 1, "(1,0) 带高位阻挡位");

        // 两个都关 ⇒ 再 +1，计数归零
        v.overlay = CollisionOverlay::none();
        v.ensure(&ctx, Some(&map), Some(&src), canvas, 1, RenderOptions::default());
        assert_eq!(v.render_calls, 4, "关掉通道也必须重渲");
        assert_eq!(v.stats.cells_low_blocked, 0, "关掉后不得再有任何标记");
        assert_eq!(v.stats.cells_high_blocked, 0);
        assert_eq!(v.stats.cells_drawn, 落笔, "开关标记通道不许影响落笔计数");
    }

    /// 只开低位阻挡时，**高位阻挡格一个像素都不染**（通道独立，`miredit-render` §7.4.1）。
    ///
    /// GUI 侧的判据：`cells_high_blocked == 0` 且 `cells_low_blocked` 只数低位那格。
    #[test]
    fn 只开低位时高位格不计() {
        let map = 阻挡图();
        let src = 全有源;
        let ctx = egui::Context::default();
        let canvas = Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(96.0, 64.0));
        let mut v = CanvasView {
            overlay: CollisionOverlay::low_only(),
            ..Default::default()
        };
        v.ensure(&ctx, Some(&map), Some(&src), canvas, 1, RenderOptions::default());
        assert_eq!(v.stats.cells_low_blocked, 1);
        assert_eq!(v.stats.cells_high_blocked, 0, "高位通道关着 ⇒ 一格都不许计");
    }

    /// **带门的 1×1 图**：前景图号 1000（⇒ 闭门帧 999）、门索引 1、门偏移 `0x80|6 = 134`。
    ///
    /// 字节口径与 `miredit-render` 的 `try_render_region_with_options` 文档例子**逐字相同**
    /// （`fr_img` 在格内偏移 4、`door_index` 6、`door_offset` 7）——
    /// 门偏移加的是 `134 & 0x7F = 6`，所以开门帧 = 999 + 6 = **1005**。
    fn 门图() -> Map {
        let mut bytes = vec![0u8; 52 + 12];
        bytes[0..2].copy_from_slice(&1u16.to_le_bytes());
        bytes[2..4].copy_from_slice(&1u16.to_le_bytes());
        bytes[52 + 4..52 + 6].copy_from_slice(&1000u16.to_le_bytes());
        bytes[52 + 6] = 1;
        bytes[52 + 7] = 134;
        Map::parse(&bytes).expect("带门的合成图应能解析")
    }

    /// **记账帧源**：记下被请求的 `(族, 帧号)`，并返回一块纯色帧（不依赖磁盘资源）。
    struct 记账源(std::cell::RefCell<Vec<(mir2_assets::Family, u32)>>);

    impl SpriteSource for 记账源 {
        fn frame(&self, lib: LibRef, index: u32) -> Option<Frame> {
            self.0.borrow_mut().push((lib.family, index));
            Some(Frame {
                width: 48,
                height: 32,
                rgba: vec![200; 48 * 32 * 4],
                // 本测试只验开门帧号是否被请求，与帧锚点无关 ⇒ 其余字段（含 anchor）走 Default，
                // 将来 Frame 再加字段时这里不必再改（见 mir2-assets/src/frame.rs 的 Default 文档）。
                ..Frame::default()
            })
        }
    }

    /// **「开启门」必须同时进缓存键与渲染选项**（2026-11-05 接线，最容易漏的一步）。
    ///
    /// 这条是**结构性的**（不是纯函数空转），三件事一起钉：
    /// ① 同一会话里 `false → true` ⇒ `render_calls + 1`（缓存键少了 `doors_open` 会停在 1）；
    /// ② 重渲时**真的换了纹理**（`TextureHandle::id` 变了，不是复用旧那张）；
    /// ③ 渲染库**真的被要了开门帧**：闭门那次请求 999、开门那次请求 **1005**
    ///    （少了 `RenderOptions` 那一步会仍然请求 999 —— "重渲了但画面没变"）。
    ///
    /// ③ 是这条测试的核心：它证明 GUI 传下去的 `doors_open` 一路走到了
    /// `miredit_render::compose` 的帧号计算，而不是只进了个缓存键。
    #[test]
    fn 开启门进缓存键并触发重渲() {
        let map = 门图();
        let src = 记账源(Default::default());
        let ctx = egui::Context::default();
        let canvas = Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(48.0, 32.0));

        let mut v = CanvasView::default();
        // ① 闭门（缺省口径）：渲一次，请求的是闭门帧
        v.ensure(&ctx, Some(&map), Some(&src), canvas, 1, RenderOptions::default());
        assert_eq!(v.render_calls, 1, "第一次必然要渲");
        assert_eq!(v.cached_render_options(), Some(RenderOptions::default()), "缓存里记的是闭门");
        assert!(
            src.0.borrow().contains(&(mir2_assets::Family::Surface, 999)),
            "闭门时应请求前景帧 999，实际：{:?}",
            src.0.borrow()
        );
        let id0 = v.texture().expect("渲完必须有纹理").id();

        // ② 同一状态再调：缓存必须命中（别每帧重渲）
        v.ensure(&ctx, Some(&map), Some(&src), canvas, 1, RenderOptions::default());
        assert_eq!(v.render_calls, 1, "门态没变就不许重渲");

        // ③ 开门 ⇒ 必须重渲（缓存键里没有 doors_open 时这里会停在 1）
        src.0.borrow_mut().clear();
        v.ensure(&ctx, Some(&map), Some(&src), canvas, 1, RenderOptions::all_doors_open());
        assert_eq!(
            v.render_calls, 2,
            "开启门必须重渲（缓存键里少了 doors_open 时这里会停在 1）"
        );
        assert_eq!(v.cached_render_options(), Some(RenderOptions::all_doors_open()), "缓存里记的是开门");
        let id1 = v.texture().expect("重渲后应有纹理").id();
        assert_ne!(id0, id1, "重渲必须换一张纹理，不是复用旧那张");
        assert!(
            src.0.borrow().contains(&(mir2_assets::Family::Surface, 1005)),
            "开门时应请求前景帧 1005（= 999 + (134 & 0x7F)），实际：{:?}",
            src.0.borrow()
        );
        assert!(
            !src.0.borrow().contains(&(mir2_assets::Family::Surface, 999)),
            "开门那次不许再请求闭门帧 999"
        );

        // ④ 关回去 ⇒ 再 +1，且重新请求闭门帧（开关是双向的）
        src.0.borrow_mut().clear();
        v.ensure(&ctx, Some(&map), Some(&src), canvas, 1, RenderOptions::default());
        assert_eq!(v.render_calls, 3, "关回去也必须重渲");
        assert_eq!(v.cached_render_options(), Some(RenderOptions::default()));
        assert!(
            src.0.borrow().contains(&(mir2_assets::Family::Surface, 999)),
            "关回去时应重新请求闭门帧 999，实际：{:?}",
            src.0.borrow()
        );
    }

    /// **地图光照必须进缓存键并原样传到渲染库**（2026-11-05 接线，验收 4 的机器版）。
    ///
    /// 切档时视口/图层/地图代号/过滤档/门态一个都没变，只有 `options.lighting` 变了：
    /// 缓存键里少这一项 ⇒ `render_calls` 停在 1、上屏纹理仍是白天（假交互）。
    /// 这条同时钉住「门态不丢」：切到夜晚时仍带着 `doors_open = true`，
    /// 渲染请求里必须既有夜晚遮罩、又请求开门帧 1005（不是 999）。
    #[test]
    fn 光照档位进缓存键并触发重渲且门态不丢() {
        let map = 门图();
        let src = 记账源(Default::default());
        let ctx = egui::Context::default();
        let canvas = Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(48.0, 32.0));
        let 白天 = RenderOptions::with_doors_and_lighting(true, MapLighting::Day);
        let 黄昏 = RenderOptions::with_doors_and_lighting(true, MapLighting::DawnDusk);
        let 夜晚 = RenderOptions::with_doors_and_lighting(true, MapLighting::Night);

        let mut v = CanvasView::default();
        v.ensure(&ctx, Some(&map), Some(&src), canvas, 1, 白天);
        assert_eq!(v.render_calls, 1, "第一次必然要渲");
        assert_eq!(v.cached_render_options(), Some(白天));
        let id0 = v.texture().expect("渲完必须有纹理").id();

        // 同一档再调：缓存必须命中
        v.ensure(&ctx, Some(&map), Some(&src), canvas, 1, 白天);
        assert_eq!(v.render_calls, 1, "档位没变就不许重渲");

        // 逐档切换：每次 +1，且缓存里记的正是那一档（含门态）
        for (i, (档, 名)) in [(黄昏, "DawnDusk"), (夜晚, "Night"), (白天, "Day")]
            .iter()
            .enumerate()
        {
            v.ensure(&ctx, Some(&map), Some(&src), canvas, 1, *档);
            assert_eq!(
                v.render_calls,
                i as u64 + 2,
                "切到 {名} 必须重渲（缓存键里少了 lighting 时这里会停在 1）"
            );
            assert_eq!(v.cached_render_options(), Some(*档), "{名} 档缓存记错");
            assert!(档.doors_open, "切光照不许把门态弄丢：{名}");
        }
        let id1 = v.texture().expect("重渲后应有纹理").id();
        assert_ne!(id0, id1, "重渲必须换一张纹理");

        // 最后一次重渲（回到白天）仍请求的是**开门**帧 1005，不是 999
        assert!(
            src.0
                .borrow()
                .contains(&(mir2_assets::Family::Surface, 1005)),
            "切光照时门态必须保住（应请求开门帧 1005），实际：{:?}",
            src.0.borrow()
        );
    }
}
