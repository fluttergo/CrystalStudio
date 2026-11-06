//! `miredit-render` —— **离屏地图合成器**（无窗口、纯计算）。
//!
//! 给定 `mir2_map::Map`（`.map` 解析结果）+ `mir2_assets::SpriteSource`（图库帧源）+
//! [`Viewport`]（世界格窗口），合成一块行优先 RGBA8 位图。GUI 之后用它上纹理，
//! 截图比对也用它出图 —— 所以它**不许有窗口依赖、不许有全局状态、不许有随机性**。
//!
//! ```no_run
//! use miredit_render::{render_region, Viewport};
//! use mir2_map::Map;
//! use mir2_assets::{LibPaths, SpriteSource, WilSource};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let bytes = std::fs::read("<maps>/0.map")?;
//! let map = Map::parse(&bytes)?;
//! let root = mir2_assets::client_root().expect("先设置 MIREDIT_CLIENT_ROOT");
//! let paths = LibPaths::discover(&root)?;
//! let src = WilSource::open(&paths)?;
//! let vp = Viewport::new(300, 250, 64, 48);
//! let out = render_region(&map, &src, &vp);
//! println!("{}×{} {}", out.width, out.height, out.stats);
//! # Ok(())
//! # }
//! ```
//!
//! # 一、渲染真值（全部逐行对照客户端源码；有冲突以源码为准，见 §五）
//!
//! | 条目 | 结论 | 出处（`文件:行号`）|
//! |---|---|---|
//! | 世界格 | **48×32 px** | `map_view.gd:9-10,13`（原作 `Grobal.h:1980-1981` UNITX/UNITY）|
//! | 地表瓦片 | **96×64 px**，覆盖 **2×2 世界格**，落格 = 世界格/2 | `map_view.gd:11-13,215` |
//! | 地表铺格范围 | **只铺偶偶格**（`x%2==0 && y%2==0`） | `map_view.gd:12,199`；原作 `GameScene.cpp:3545-3553` |
//! | 帧号口径 | **1-based**，`0`=无图，取图 `(img&0x7FFF)-1` | `map_loader.gd:51-52`、`map_overlay_layer.gd:20`；原作 `GameScene.cpp:3575-3577` |
//! | 库路由 | `btTiles`→地面 / `btsmTiles`→中层 / `btArea`→表层 | `map_loader.gd:39-45`、`map_overlay_layer.gd:67,71,84,88`；原作 `MShare.cpp:808-873` |
//! | 层序 | 地表 → 中层 → 表层 | `map_view.gd:108-114`、`map_overlay_layer.gd:22-24` |
//! | 层内顺序 | 列优先（`dx` 外 / `dy` 内） | `map_overlay_layer.gd:14-16,46-53` |
//! | 地表锚点 | 左上角 = `(x*48, y*32)` | `map_view.gd:13` |
//! | 中层锚点 | 左上角 = `(x*48, y*32)` | `map_overlay_layer.gd:79` |
//! | 表层锚点 | **底边**钉格底：`y_top=(y+1)*32-h` | `map_overlay_layer.gd:18-19,96-99`；原作 `GameScene.cpp:3515` |
//! | 可走判定 | `(bk&0x8000)==0 && (fr&0x8000)==0` | `mir2-map/src/cell.rs:181-188`；`map_loader.gd:34,311-319` |
//!
//! 逐条规则与更细的对照说明写在 [`compose`] 的模块文档里（那是唯一实现处）。
//!
//! # 二、缺帧口径（**禁假数据**）
//!
//! - 帧取不到（库没登记 / 帧号越界 / 空帧）⇒ **不画**，计入 `missing_ground/middle/front`；
//! - **绝不画洋红/纯色占位图**，也绝不猜「差不多是哪个库」；
//! - 该层图号为 0（本来就没图）⇒ 跳过且**不计入 missing**；
//! - 窗口在图外 ⇒ 不画也不计数。
//!
//! # 三、输出
//!
//! 行优先 RGBA8，底色**全透明黑** `(0,0,0,0)`。尺寸 = `cells_w*48 × cells_h*32`（`scale==1.0`）。
//!
//! # 四、缩放
//!
//! 旧入口（[`try_render_region`] / [`try_render_region_with`] / [`render_region`]）
//! **只支持 `scale == 1.0`**：其它值返回 [`RenderError::UnsupportedScale`]，
//! [`render_region`] 直接断言失败。**不做半吊子缩放**（点采样会糊、双线性与客户端不一致，
//! 两者都算假数据）。这条契约自本单起**一个字都没改**。
//!
//! 多倍率（`0.25 / 0.5 / 1 / 2`）走**新入口** [`try_render_region_scaled`]：
//! 先按 1× 合成（完全复用 [`compose`]），再用 [`resample`] 的内核整幅重采样；
//! `RenderScale::One` 档**直接转调旧路径**，输出与 [`try_render_region_with`] 逐字节相同。
//! 口径与滤镜裁决见 [`scale`] 模块文档（缩小用面积加权 `Box` 的**流式**实现、放大用
//! `Nearest`）。**两条字节护栏**，管的是两个不同的缓冲：
//!
//! | 护栏 | 管什么 | 上限 | 超限错误 |
//! |---|---|---|---|
//! | [`SCALED_MAX_BYTES`] | **目标图**（要交给网页的那张）| 64 MiB | [`RenderError::TooLarge`] |
//! | [`INTERMEDIATE_MAX_BYTES`] | **1× 中间图**（内部先合成的那张）| 128 MiB | [`RenderError::IntermediateTooLarge`] |
//!
//! 为什么两条都要：目标图小 ≠ 中间图小。`200×150` 格 `@0.25` 的目标只有 11.5 MB，而 1×
//! 中间图是 175.8 MiB —— 只卡目标尺寸等于放行一发 176 MB 的分配。两条都在**取帧之前**
//! 判定，不硬渲、不静默降级成 1×。[`RenderScale::One`] 不产生中间图，只受第一条约束。
//!
//! # 五、与源码的已知差异（如实登记）
//!
//! 1. **层内顺序**：任务书 §三.4 写「表层按 y 排序绘制」（= 原始 C++ 行优先，
//!    `GameScene.cpp:3429-3431`），而 Godot 客户端是**列优先**（`map_overlay_layer.gd:46-53`）。
//!    按「以源码为准」取 **Godot 客户端口径**（截图对照的基准就是它）。
//! 2. **地表帧尺寸**：本库按帧原始尺寸贴（与 C++ `postRender` 一致）；Godot 侧走
//!    `TileSetAtlasSource` 固定 96×64 格（`ground_tileset.gd:35,205`）。实测两张抽样图的地表帧
//!    都是 96×64，不触发。
//! 3. **表层 48×32 限制**：C++ `renderBridge` 只画 48×32 的帧（`GameScene.cpp:3513`），
//!    Godot 移植版无此限制 ⇒ 本库跟 `.gd`。
//! 4. **动画帧**：C++ 会按 `btAniFrame`/`btAniTick` 推动画步进（`GameScene.cpp:3350-3363`），
//!    Godot 客户端只取静态基准帧，本库同 `.gd`。
//!    ⚠ **门偏移不在此列**：它已由 [`RenderOptions::doors_open`] 实现（见 §九），默认关。
//!
//! # 六、依赖纪律
//!
//! 渲染核心（`canvas` / `compose` / `frames`）只用 `std` + `mir2-map` + `mir2-assets`；
//! `png` 只出现在 [`png_io`]（导出参考图 / 读客户端截图）与 [`compare`]（分块比对）。
//!
//! # 七、阻挡可视化口径（编辑器「碰撞层」用）
//!
//! 由 [`try_render_region_with`] 的 [`CollisionOverlay`] 参数开启，**默认两个通道都关**
//! ⇒ [`try_render_region`] / [`render_region`] 的行为一个字节都不变。
//!
//! ## 7.1 语义：以客户端为准
//!
//! | 通道 | 判据 | 出处 |
//! |---|---|---|
//! | 低阻挡 | `wBkImg & 0x8000 != 0` | `client/reference_full/Classes/map/TMap.cpp:339` |
//! | 高位阻挡 | `wFrImg & 0x8000 != 0` | 同上 |
//!
//! 原句 `Result = ((pt.wBkImg & 0x8000) + (pt.wFrImg & 0x8000)) == 0;`（`CanWalk`：
//! 两层都不带 `0x8000` 才可走）⇒ 两层**各自**表达「挡住」，故本库逐层取位；
//! Rust 侧同口径见 `mir2-map/src/cell.rs:204-212`（`Cell::is_walkable`）。
//!
//! ## 7.2 视觉：借 C# `CrystalStudio` 的样式
//!
//! | 项 | 取值 | 出处 |
//! |---|---|---|
//! | 低阻挡色 | `RGB(240,85,105)` | `CrystalStudio/CollisionMarkers.cs:6`（`First`）|
//! | 高位阻挡色 | `RGB(65,165,245)` | `CrystalStudio/CollisionMarkers.cs:7`（`Second`）|
//! | 透明度 | `alpha = 90/255`（默认） | 同上两行的首参 |
//! | 形状 | 整格 `48×32` 纯色块 | `CollisionMarkers.cs:24`（`DrawRectangle(..., cell)`）|
//! | 同格两者都有 | **左半格**低（`24×32`）+ **右半格**高（`24×32`） | `CollisionMarkers.cs:19-23`（`cell.Width / 2`）|
//! | 时机 | 地图精灵之后、灯光/门标记之前 | `MapViewport.Presentation.cs:40-42` |
//!
//! ## 7.3 与 C# 的**冲突**及裁定（照任务书「以客户端语义为准」）
//!
//! C# 编辑器把「低/高阻挡」编成 `Cell.BackImage` 的 `0x20000000` / `0x40000000` 两个
//! **自有位**（`MapViewport.cs:23` 的 `ActiveCollisionMask`，`:480` 写入、
//! `:83` 的 `VisibleCollisionMask` 过滤）——那是它**尚未写回 `.map` 的编辑态**，
//! 与客户端 `.map` 里的 `0x8000` 不是同一套位，**两者不可混用**。
//! ⇒ 本库：**判定按 §7.1 的客户端语义**，只借 §7.2 的颜色/形状/时机。
//!
//! ## 7.4 三条硬口径
//!
//! 1. **通道独立**：`low` / `high` 各自与判定相与 ⇒ 只开 `low` 时，高阻挡格**一个像素都不染**；
//! 2. **默认关闭 ⇒ 逐字节兼容**：两个通道都关时整个叠加环节被跳过（`compose` 里
//!    `overlay.any()` 直接短路），输出与「没有这个功能」完全相同；
//! 3. **计数独立**：标记只进 [`RenderStats::cells_low_blocked`] /
//!    [`RenderStats::cells_high_blocked`]，**不进** `cells_drawn`（后者口径始终是三层精灵的落笔），
//!    且只统计**真的画出像素**的格（`alpha == 0` ⇒ 0）。
//!
//! 实现处：[`compose`]（遍历）与 `collision`（判定 + 画色块）。
//!
//! # 八、印章缩略图（[`stamp`] 模块，2026-11-05 新增）
//!
//! [`stamp::render_stamp`] 把一枚 `.X` 印章的格集合渲染成**透明底 RGBA**，供 GUI 与网页版
//! 的「印章面板」出缩略图。三条必须记住的口径：
//!
//! 1. **落格规则与地图渲染同源**：地表/中层左上角锚定、物件底边锚定、帧号 `(img&0x7FFF)-1`
//!    —— 全部调 `compose` 的同一批函数（`compose::左上锚点` / `底边锚点` / `帧号`），
//!    **本库只有一份落格实现**；
//! 2. **不依赖 `mir2-stamp`**：入参是本 crate 自己的 [`stamp::PositionedCell`]，
//!    `.X` 的解析与适配在调用方（`mir2-stamp` 只出现在 `[dev-dependencies]` 里做验收）；
//! 3. **缺帧不画占位**：取不到帧计入 [`stamp::StampImage::missing`] 并跳过该格该层。
//!
//! 与地图渲染的**有意差异**（任务口径）：印章不套用「地表只铺偶偶格」；画布 = 格包围盒
//! （因此帧越界部分被裁）；空输入返回 0×0 空图而不是错误。逐条见 [`stamp`] 模块文档。
//!
//! # 九、开启门（门开帧偏移，[`RenderOptions::doors_open`]，2026-11-05 新增）
//!
//! 编辑器菜单「视图 → 开启门」（`MainWindow.axaml:5` 的 `DoorsOpenToggle`，**默认不勾**）
//! 在渲染层的实现：**只改前景层画哪一帧**，不改阻挡、不改数据、不做过渡动画。
//!
//! ## 9.1 公式（客户端口径，`GameScene.cpp:3328-3388`）
//!
//! ```text
//! 帧号 = (fr_img & 0x7FFF) - 1                       // 闭门帧（0-based）
//! if doors_open && (door_index & 0x7F) != 0
//!     帧号 += door_offset & 0x7F                     // 开门帧偏移
//! ```
//!
//! 客户端原文把偏移加在 `-1` 之前（`:3370`/`:3378` → `:3381`），但都是常数平移 ⇒ 等价。
//!
//! ## 9.2 三条硬口径
//!
//! 1. **默认 `false` ⇒ 逐字节不变**：旧入口（[`try_render_region`] /
//!    [`try_render_region_with`] / [`try_render_region_scaled`]）**签名一个都没改**，
//!    内部按 [`RenderOptions::default()`] 转调；`doors_open == false` 时门偏移恒 0。
//! 2. **只在前景层加**：地表 / 中层没有门，两处代码一行未动；门帧与闭门帧同属 `Objects` 库
//!    （`GameScene.cpp:3341,3388`）。
//! 3. **必须 `& 0x7F`**：`0x80` 是「运行期已开门」标志位，不是偏移。全量实测
//!    **43 张图 / 1,764 格**已置位，其中 **1,229 格**有前景图 + 非 0 门索引
//!    （**1,173 格库号 0 = `Objects` 真实存在，会真画出来**）⇒ 整字节相加会 `+128+low`
//!    画错帧。**官方 C# 编辑器 `MapPreview.cs:6-10` 没掩 `0x80`，那是它的偏差，本库不跟**
//!    （与 §七 阻挡位「判定按客户端、只借 C# 视觉」同一条纪律）。逐条数据见
//!    [`RenderOptions`] 类型文档 §三/§四。
//!
//! ## 9.3 入口与缓存
//!
//! 1× 走 [`try_render_region_with_options`]，多倍率走 [`try_render_region_scaled_with`]
//! （`RenderScale::One` 档仍**直接转调** 1× 路径 ⇒ 两入口在 1× 上逐字节相同）。
//! 门开合改变的是帧号 ⇒ **按视口缓存渲染结果的调用方必须把 `doors_open` 并进缓存键**
//! （官方编辑器 `MapViewport.cs:91-99` 的 `frontRows.Clear()` + `:85` 的
//! `(DoorsOpen ? 16 : 0)` 就是同一件事）。本库自身不跨次缓存，无需处理。
//!
//! ## 9.4 帧号 / 门偏移**只有一份**（2026-11-05 收口）
//!
//! [`帧号`] 与 [`门偏移`] 已从 `pub(crate)` 提成 **`pub` 并在本模块再导出**。原因不是
//! 「顺手开放 API」，而是 `miredit-gui` 导出时的扩边计算（`export.rs` 的 `tile_margins`）
//! **必须**按「真正会画出去的那一帧」的尺寸算外溢 —— 帧号算错一位就取错帧、取错尺寸，
//! 块边界上丢像素。那边原先因为跨 crate 取不到，**照抄了一份**（抄件注释写着
//! 「改口径必须两边一起改」）⇒ 两份必然漂，而**漂移的症状正是「画布开门、导出闭门」**。
//! 现在两边都调这里：`grep -rn "0x7F" crates/miredit-gui/src/export.rs` 只剩注释，
//! 一行实现都没有了。
//!
//! 反过来，**`CELL_W` / `CELL_H` 这类算式不共享**：`dx*48` 是从公开常量直接推出来的，
//! 抄了也漂不了；真正会漂的是 `0x7FFF` / `0x7F` 这种**没有常量可依的魔法掩码**。
//!
//! # 十、地图光照（[`RenderOptions::lighting`]，2026-11-05 新增）
//!
//! 编辑器菜单「视图 → 地图光照」三档（`MapLighting.cs:3` 的 `enum { DawnDusk, Day, Night }`，
//! **默认 `Day`**，`MapViewport.cs:88`）在渲染层的实现。官方机制**不是逐格改色**：
//!
//! ```text
//! ① 全屏黑色遮罩，alpha = 255 - 环境光            // 205 / 0 / 255
//! ② 每束光：alpha = max(0, alpha - 光罩强度)      // 逐格减
//! ③ 遮罩盖三层、裁在地图矩形内；光源只从物件层取
//! ④ 「白天」= 关（连遮罩都不画，MapViewport.cs:89）
//! ```
//!
//! ## 10.1 三档常量（`MapViewport.Presentation.cs:68-70`，注释 + 字面量双证自洽）
//!
//! | 档 | 环境光 | 遮罩 alpha |
//! |---|---:|---:|
//! | [`MapLighting::DawnDusk`] | 50 | **205** |
//! | [`MapLighting::Day`] | 255 | **0** |
//! | [`MapLighting::Night`] | 0 | **255** |
//!
//! ## 10.2 逐格光罩（`MapPreview.cs:14-34` + `Presentation.cs:73-93`）
//!
//! - **发光条件**：`cell.light ∈ 1..=9` **且**该格有前景图（`fr_img & 0x7FFF != 0`）
//!   —— 见 [`emits_light`]；没有前景图的格子即便 `light` 合法也**不发光**；
//! - **下标**：`index = min(light * 3, 9)`；**定位用 `LIGHT_SIZES[index]`、贴图用
//!   `LIGHT_SIZES[index + 1]`**（官方 `MapPreview.cs:24-25` 注释明说这是客户端原样、
//!   故意的错位，**不许"修正"**）⇒ 贴图宽 `1→445 / 2→685 / 其它→925`；
//! - **强度**：官方光罩 PNG 的**预乘 R 通道**（`MapPreview.cs:40-43`）；
//! - **合成**：`alpha = max(0, alpha - intensity)`。
//!
//! ## 10.3 三条硬口径
//!
//! 1. **默认（白天）逐字节不变**：`darkness == 0` ⇒ `lighting::apply` 立即返回；
//! 2. **三层之后、标记之前**：与官方 `Presentation.cs:34-35 → :40-50` 的次序一致
//!    ⇒ **阻挡标记不被压暗**（见 [`compose`] 模块文档 §八）；
//! 3. **不做无据自造**：不生成衰减曲线（只用官方 `assets/lights/*.png`）、
//!    不微调 `205/0/255`、不加档位过渡动画。
//!
//! ## 10.4 光罩偏移（2026-11-05 补齐，原「已知偏差」已消除）
//!
//! 光罩的 `offset` = 官方 `sprite.X/Y`（`MapViewport.Presentation.cs:84-86@901dbe2f(未入库)`），
//! 本仓取 [`mir2_assets::Frame::anchor`]（帧头那对 i16）。⚠ **订正旧表述**：本文件曾写它
//! 「在本仓**不可得**（`Frame` 只有 `width/height/rgba`）⇒ 取 `(0,0)`」——
//! 按 `deliverables/取证-帧偏移数据可得性-2026-11-05.md` §1.4，数据一直都在，
//! 缺的只是 `Frame` 这一个结构没暴露 ⇒ 准确说法是「**当前 API 未暴露**」，现已补上。
//! 逐条（含影响面实测：51% 的发光格恰为 `(0,0)`、其余中位 51 / p90 140 / 最大 561 px）
//! 见 [`lighting`] 模块文档 §四①。
//!
//! ⚠ **偏移只给光罩用**：三层合成**不碰** `anchor`（官方画精灵时 `sprite.X/Y` 只在特例下生效，
//! `MapSpriteDrawing.cs:19-31@901dbe2f(未入库)`）⇒ 补偏移后三层像素逐字节不变（测试钉住）。

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod canvas;
mod collision;
mod compose;
mod frames;
mod resample;
mod scale;
mod types;

pub mod compare;
pub mod lighting;
pub mod png_io;
pub mod stamp;

pub use types::{
    CollisionOverlay, LayerMask, RenderError, RenderOptions, RenderScale, RenderStats, Rendered,
    Viewport, CELL_H, CELL_W, INTERMEDIATE_MAX_BYTES, SCALED_MAX_BYTES, TILE_H, TILE_W,
};

pub use compare::{compare_blocks, BlockReport, CompareOptions};
// 帧号 / 门偏移：**跨 crate 的单一份**（见 crate 文档 §9.4）。
// 提成 `pub` 的唯一理由是 `miredit-gui` 的导出扩边必须与合成用同一帧号 ——
// 那边抄过一份，两份必然漂，症状是「画布开门、导出闭门」。别再抄第二份。
pub use compose::{门偏移, 帧号};
pub use lighting::{emits_light, light_bounds, MapLighting, LIGHT_SIZES};
pub use png_io::PngImage;
pub use scale::{try_render_region_scaled, try_render_region_scaled_with};
pub use stamp::{
    render_stamp, FilterClass, PositionedCell, StampImage, StampRenderOptions,
};

use mir2_assets::SpriteSource;
use mir2_map::Map;

/// 渲染一块世界格窗口（三层全开）。
///
/// **前置条件**：`vp.scale == 1.0`，`cells_w/cells_h > 0`，尺寸不越护栏。
/// 不满足时**断言失败**（`panic`）；需要 `Result` 请用 [`try_render_region`]。
///
/// 缺帧**不是错误**：取不到就不画、计入 [`RenderStats`]，函数照常返回。
pub fn render_region(map: &Map, src: &dyn SpriteSource, vp: &Viewport) -> Rendered {
    try_render_region(map, src, vp, &LayerMask::all())
        .unwrap_or_else(|e| panic!("render_region 前置校验失败：{e}"))
}

/// 可失败版本：校验 `scale` / 尺寸，并按 [`LayerMask`] 选择性绘制三层。
///
/// **不画阻挡标记**（等价于 `try_render_region_with(..., CollisionOverlay::default())`）。
/// 需要阻挡可视化请用 [`try_render_region_with`]。
pub fn try_render_region(
    map: &Map,
    src: &dyn SpriteSource,
    vp: &Viewport,
    mask: &LayerMask,
) -> Result<Rendered, RenderError> {
    try_render_region_with(map, src, vp, mask, CollisionOverlay::default())
}

/// 可失败版本 + **阻挡可视化通道**：在三层之后按 [`CollisionOverlay`] 叠加低/高位阻挡标记。
///
/// # 参数
///
/// - `mask`：三层绘制开关（口径与 [`try_render_region`] 完全一致）；
/// - `overlay`：阻挡标记通道。`overlay.low` / `overlay.high` 分别对应
///   `wBkImg & 0x8000` / `wFrImg & 0x8000`；`overlay.alpha` 是标记透明度。
///   **传 [`CollisionOverlay::default()`] 时输出与 [`try_render_region`] 逐字节相同**
///   （两个通道都关 ⇒ 整个叠加环节被短路）。
///
/// # 语义 / 视觉 / 与 C# 编辑器的差异
///
/// 见 crate 文档 §七「阻挡可视化口径」（判定按客户端 `TMap.cpp:339`，颜色与形状借
/// `CrystalStudio/CollisionMarkers.cs:6-7,19-25`）。
///
/// # 计数
///
/// 标记只进 [`RenderStats::cells_low_blocked`] / [`RenderStats::cells_high_blocked`]，
/// **不改** `cells_drawn` / `missing_*` / `frames_decoded`。
///
/// # 例子
///
/// ```
/// use miredit_render::{try_render_region_with, CollisionOverlay, LayerMask, Viewport};
/// use mir2_map::Map;
/// use mir2_assets::{Frame, LibRef, SpriteSource};
///
/// struct 空源;
/// impl SpriteSource for 空源 {
///     fn frame(&self, _lib: LibRef, _index: u32) -> Option<Frame> { None }
/// }
///
/// // 2×2 格地图，只有 (0,0) 带低阻挡位（0x8000），没有图号 ⇒ 精灵层什么都不画
/// let mut bytes = vec![0u8; 52 + 12 * 4];
/// bytes[0..2].copy_from_slice(&2u16.to_le_bytes());
/// bytes[2..4].copy_from_slice(&2u16.to_le_bytes());
/// bytes[52..54].copy_from_slice(&0x8000u16.to_le_bytes());
/// let map = Map::parse(&bytes)?;
///
/// let out = try_render_region_with(
///     &map,
///     &空源,
///     &Viewport::new(0, 0, 2, 2),
///     &LayerMask::all(),
///     CollisionOverlay::low_only(),
/// )?;
/// assert_eq!(out.stats.cells_low_blocked, 1);
/// assert_eq!(out.stats.cells_high_blocked, 0);
/// // 低阻挡标记色 RGB(240,85,105) + alpha 90，铺在 (0,0) 格的左上角
/// assert_eq!(out.pixel(0, 0).unwrap(), [240, 85, 105, 90]);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn try_render_region_with(
    map: &Map,
    src: &dyn SpriteSource,
    vp: &Viewport,
    mask: &LayerMask,
    overlay: CollisionOverlay,
) -> Result<Rendered, RenderError> {
    try_render_region_with_options(map, src, vp, mask, overlay, RenderOptions::default())
}

/// 可失败版本 + 阻挡可视化 + **渲染选项（开启门）**：本 crate 的**完整入口**。
///
/// 其余入口都是它的转调，区别只在传什么默认值：
///
/// | 入口 | 阻挡标记 | 开启门 |
/// |---|---|---|
/// | [`render_region`] | 关 | **关** |
/// | [`try_render_region`] | 关 | **关** |
/// | [`try_render_region_with`] | 由参数给 | **关** |
/// | 本函数 | 由参数给 | 由参数给 |
/// | [`try_render_region_scaled`] / [`try_render_region_scaled_with`] | 由参数给 | 关 / 由参数给 |
///
/// # 参数
///
/// - `mask` / `overlay`：与 [`try_render_region_with`] 完全同口径；
/// - `options`：[`RenderOptions`]。**`RenderOptions::default()`（= `doors_open: false`）
///   时输出与 [`try_render_region_with`] 逐字节相同**（门偏移恒 0）。
///
/// # 开启门改什么（**只改前景层帧号**）
///
/// ```text
/// if options.doors_open && (cell.door_index & 0x7F) != 0
///     帧号 += cell.door_offset & 0x7F
/// ```
///
/// 出处：官方客户端 `client/reference_full/Classes/scenes/GameScene.cpp:3328-3388`
/// （`:3367`/`:3375` 判据、`:3370`/`:3378` 加偏移、`:3381` `-1`、`:3388` 取帧）。
/// **必须 `& 0x7F`**（`0x80` 是运行期开合标志位，不是偏移），**不跟**官方编辑器
/// `MapPreview.cs:6-10` 的整字节相加 —— 理由与实测数据（43 张图 / 1,764 格 / 1,173 格
/// 库号为 0）见 [`RenderOptions`] 的类型文档 §三/§四。地表 / 中层**不受影响**（没有门）。
///
/// # 计数
///
/// 门开帧取不到时按缺帧口径计入 [`RenderStats::missing_front`]（**不回退画闭门帧**，
/// 那等于撒谎）；其余计数口径不变。
///
/// # 例子
///
/// ```
/// use miredit_render::{
///     try_render_region_with_options, CollisionOverlay, LayerMask, RenderOptions, Viewport,
/// };
/// use mir2_map::Map;
/// use mir2_assets::{Frame, LibRef, SpriteSource};
///
/// /// 记录「被请求了哪些帧」的空源：返回 None（缺帧），但请求已记账。
/// struct 记账源(std::cell::RefCell<Vec<(LibRef, u32)>>);
/// impl SpriteSource for 记账源 {
///     fn frame(&self, lib: LibRef, index: u32) -> Option<Frame> {
///         self.0.borrow_mut().push((lib, index));
///         None
///     }
/// }
///
/// // 1×1 格：前景图号 1000（⇒ 闭门帧 999）、门索引 1、门偏移 0x80|6 = 134
/// let mut bytes = vec![0u8; 52 + 12];
/// bytes[0..2].copy_from_slice(&1u16.to_le_bytes());
/// bytes[2..4].copy_from_slice(&1u16.to_le_bytes());
/// bytes[52 + 4..52 + 6].copy_from_slice(&1000u16.to_le_bytes());
/// bytes[52 + 6] = 1;
/// bytes[52 + 7] = 134;
/// let map = Map::parse(&bytes)?;
/// let vp = Viewport::new(0, 0, 1, 1);
///
/// let 闭 = 记账源(Default::default());
/// try_render_region_with_options(
///     &map, &闭, &vp, &LayerMask::all(), CollisionOverlay::none(), RenderOptions::closed())?;
/// assert_eq!(闭.0.borrow().as_slice(), &[(LibRef::surface(0), 999)]);
///
/// let 开 = 记账源(Default::default());
/// try_render_region_with_options(
///     &map, &开, &vp, &LayerMask::all(), CollisionOverlay::none(),
///     RenderOptions::all_doors_open())?;
/// // 加的是 6（= 134 & 0x7F），不是 134
/// assert_eq!(开.0.borrow().as_slice(), &[(LibRef::surface(0), 1005)]);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn try_render_region_with_options(
    map: &Map,
    src: &dyn SpriteSource,
    vp: &Viewport,
    mask: &LayerMask,
    overlay: CollisionOverlay,
    options: RenderOptions,
) -> Result<Rendered, RenderError> {
    compose::render(map, src, vp, mask, overlay, options)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mir2_assets::{Frame, LibRef};

    /// 假帧源：按「族 + 索引 + 帧号」造一块纯色帧，用于**不依赖磁盘资源**的结构断言。
    ///
    /// 颜色编码：`r = 族序号*40+20`，`g = 帧号*10`，`b = 帧高/4`，便于反查「贴的是哪一帧」。
    struct 假源 {
        w: u32,
        h: u32,
    }

    impl SpriteSource for 假源 {
        fn frame(&self, lib: LibRef, index: u32) -> Option<Frame> {
            let fam = lib.family.index() as u32;
            let c = [
                (fam * 40 + 20) as u8,
                (index.wrapping_mul(10) & 0xFF) as u8,
                (self.h / 4) as u8,
                255,
            ];
            let mut rgba = Vec::with_capacity((self.w * self.h) as usize * 4);
            for _ in 0..(self.w * self.h) {
                rgba.extend_from_slice(&c);
            }
            Some(Frame {
                width: self.w,
                height: self.h,
                rgba,
                ..mir2_assets::Frame::default()
            })
        }
    }

    /// 造一张 `w×h` 的 Old12 地图，格内容由闭包决定。
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

    #[test]
    fn 落笔格数只数真画了的格() {
        // a) 全空格地图：图号全 0，帧源再好也没有东西可画 ⇒ cells_drawn 必须是 0
        //    （「扫过 48 格」不等于「画了 48 格」）
        let map = 造图(8, 6, |_, _| mir2_map::Cell::default());
        let src = 假源 { w: 96, h: 64 };
        let out = render_region(&map, &src, &Viewport::new(0, 0, 8, 6));
        assert_eq!(out.stats.cells_drawn, 0, "全空格地图不得有任何落笔格");

        // b) 只有地表：偶偶格才铺（x%2==0 && y%2==0），8×6 视口里 = 4 列 × 3 行 = 12 格
        let map = 造图(8, 6, |x, y| {
            if x % 2 == 0 && y % 2 == 0 {
                mir2_map::Cell {
                    bk_img: 1,
                    ..Default::default()
                }
            } else {
                mir2_map::Cell::default()
            }
        });
        let out = render_region(&map, &src, &Viewport::new(0, 0, 8, 6));
        assert_eq!(
            out.stats.cells_drawn, 12,
            "只有地表的图：落笔格数应正好等于「铺了地表的格数」"
        );
        assert_eq!(out.stats.missing_ground, 0, "地表帧都取到了，不该有缺帧");
        assert_eq!(out.stats.missing_middle, 0);
        assert_eq!(out.stats.missing_front, 0);
    }

    #[test]
    fn 空帧与全透明帧不算落笔() {
        // 这两类帧「取得到」（所以**不计 missing**），但往画布上写不进任何像素。
        // 口径：`cells_drawn` 只认「实际写了像素」⇒ 必须是 0。
        // （`mir2_assets::SpriteSource` 的契约要求解码器把真空帧报成 None；本用例正是
        //   刻意用违规帧源证明**渲染层的统计不依赖帧源守规矩**，也证明它不是按「扫过」计的。）
        let map = 造图(2, 2, |_, _| mir2_map::Cell {
            mid_img: 1, // 中层逐格铺 ⇒ 4 格都会被「扫到」
            ..Default::default()
        });
        let vp = Viewport::new(0, 0, 2, 2);
        let 只中层 = LayerMask {
            ground: false,
            middle: true,
            front: false,
        };

        // ① 整帧全透明（48×32 全是 alpha=0）
        struct 全透明源;
        impl SpriteSource for 全透明源 {
            fn frame(&self, _lib: LibRef, _index: u32) -> Option<Frame> {
                Some(Frame {
                    width: 48,
                    height: 32,
                    rgba: vec![0u8; 48 * 32 * 4],
                    ..mir2_assets::Frame::default()
                })
            }
        }
        let out = try_render_region(&map, &全透明源, &vp, &只中层).unwrap();
        assert_eq!(out.stats.missing_middle, 0, "帧是取到的，不该计入缺帧");
        assert_eq!(out.stats.cells_drawn, 0, "整帧全透明 ⇒ 一个像素都没写，不算落笔");
        assert!(out.rgba.iter().all(|b| *b == 0));

        // ② 0×0 空帧（WIL/WZL 里确实存在尺寸为 0 的空帧，见记忆里 Prguse#405 那类）
        struct 空帧源;
        impl SpriteSource for 空帧源 {
            fn frame(&self, _lib: LibRef, _index: u32) -> Option<Frame> {
                Some(Frame {
                    width: 0,
                    height: 0,
                    rgba: Vec::new(),
                    ..mir2_assets::Frame::default()
                })
            }
        }
        let out = try_render_region(&map, &空帧源, &vp, &只中层).unwrap();
        assert_eq!(out.stats.missing_middle, 0, "空帧也算「取到了」，不计缺帧（口径不变）");
        assert_eq!(out.stats.cells_drawn, 0, "0×0 空帧什么都没写，不算落笔");
        assert!(out.rgba.iter().all(|b| *b == 0));

        // ③ 对照：同一张图换成正常帧 ⇒ 4 格都落笔（证明上面两例的 0 不是别的原因造成的）
        let src = 假源 { w: 48, h: 32 };
        let out = try_render_region(&map, &src, &vp, &只中层).unwrap();
        assert_eq!(out.stats.cells_drawn, 4, "正常不透明帧 ⇒ 4 格都落笔");
    }

    #[test]
    fn 尺寸等于格数乘世界格() {
        let map = 造图(64, 48, |_, _| mir2_map::Cell::default());
        let src = 假源 { w: 96, h: 64 };
        let out = render_region(&map, &src, &Viewport::new(0, 0, 64, 48));
        assert_eq!((out.width, out.height), (3072, 1536));
        assert_eq!(out.rgba.len(), 3072 * 1536 * 4);
        assert!(
            out.rgba.iter().all(|b| *b == 0),
            "全空格地图必须输出全透明，不得有任何占位色"
        );
        assert_eq!(out.stats, RenderStats::default());
    }

    #[test]
    fn 地表只铺偶偶格且锚点在左上角() {
        // 4×4 每格都给地表帧 1（0-based 帧 0）
        let map = 造图(4, 4, |_, _| mir2_map::Cell {
            bk_img: 1,
            ..Default::default()
        });
        let src = 假源 { w: 96, h: 64 };
        let out = render_region(&map, &src, &Viewport::new(0, 0, 4, 4));
        // (0,0) 有图；(1,0) 是奇列 ⇒ 但它是 (0,0) 那张 96×64 的右半 ⇒ 仍被覆盖
        assert!(out.pixel(0, 0).unwrap()[3] > 0);
        assert!(out.pixel(95, 63).unwrap()[3] > 0);
        // (2,2) 是第 2 张 96×64 的左上角
        assert!(out.pixel(96, 64).unwrap()[3] > 0);
        // 整幅 192×128 全被 4 张瓦片铺满
        // （`as_chunks::<4>().0` 等价于原来的 `chunks_exact(4)`，只为消 clippy 1.98 的 lint）
        assert!(out.rgba.as_chunks::<4>().0.iter().all(|p| p[3] == 255));
        // cells_drawn 是「落笔格数」，地表按偶偶格计 ⇒ 4 格
        assert_eq!(out.stats.cells_drawn, 4);
        assert_eq!(out.stats.frames_decoded, 1, "同一帧只解码一次（缓存命中）");
    }

    #[test]
    fn 缺帧不画且计数_不画占位() {
        struct 空源;
        impl SpriteSource for 空源 {
            fn frame(&self, _lib: LibRef, _index: u32) -> Option<Frame> {
                None
            }
        }
        let map = 造图(4, 4, |_, _| mir2_map::Cell {
            bk_img: 0x8001, // 最高位 = 不可通过；低 15 位 = 1 ⇒ 帧 0
            mid_img: 3,
            fr_img: 5,
            ..Default::default()
        });
        let out = render_region(&map, &空源, &Viewport::new(0, 0, 4, 4));
        assert!(out.rgba.iter().all(|b| *b == 0), "缺帧必须什么都不画");
        assert_eq!(out.stats.cells_drawn, 0);
        assert_eq!(out.stats.missing_ground, 4, "偶偶格 2×2 = 4");
        assert_eq!(out.stats.missing_middle, 16);
        assert_eq!(out.stats.missing_front, 16);
        assert_eq!(out.stats.frames_decoded, 0);
    }

    #[test]
    fn 图号为零不计缺帧() {
        struct 空源;
        impl SpriteSource for 空源 {
            fn frame(&self, _lib: LibRef, _index: u32) -> Option<Frame> {
                None
            }
        }
        let map = 造图(2, 2, |_, _| mir2_map::Cell::default());
        let out = render_region(&map, &空源, &Viewport::new(0, 0, 2, 2));
        assert_eq!(out.stats.missing_ground, 0);
        assert_eq!(out.stats.missing_middle, 0);
        assert_eq!(out.stats.missing_front, 0);
        assert_eq!(out.stats.cells_drawn, 0);
    }

    #[test]
    fn 图外格不画也不计数() {
        let map = 造图(2, 2, |_, _| mir2_map::Cell {
            bk_img: 1,
            ..Default::default()
        });
        let src = 假源 { w: 96, h: 64 };
        // 窗口整体在图外（负坐标）
        let out = render_region(&map, &src, &Viewport::new(-8, -8, 2, 2));
        assert!(out.rgba.iter().all(|b| *b == 0));
        assert_eq!(out.stats, RenderStats::default());
    }

    #[test]
    fn 缩放非一被拒绝() {
        let map = 造图(2, 2, |_, _| mir2_map::Cell::default());
        let src = 假源 { w: 96, h: 64 };
        let mut vp = Viewport::new(0, 0, 2, 2);
        vp.scale = 2.0;
        match try_render_region(&map, &src, &vp, &LayerMask::all()) {
            Err(RenderError::UnsupportedScale { scale_milli }) => assert_eq!(scale_milli, 2000),
            other => panic!("应拒绝 scale=2.0，实际 {other:?}"),
        }
        assert!(vp.pixel_size().is_none());
    }

    #[test]
    fn 零格与越界被拒绝() {
        let map = 造图(2, 2, |_, _| mir2_map::Cell::default());
        let src = 假源 { w: 96, h: 64 };
        let vp0 = Viewport::new(0, 0, 0, 4);
        assert_eq!(
            try_render_region(&map, &src, &vp0, &LayerMask::all()).unwrap_err(),
            RenderError::ZeroCells
        );
        let vp_big = Viewport::new(0, 0, 100_000, 4);
        assert!(matches!(
            try_render_region(&map, &src, &vp_big, &LayerMask::all()).unwrap_err(),
            RenderError::DimensionOverflow { .. }
        ));
    }

    #[test]
    fn 图层开关按位生效且层序为先中后前() {
        // 假源按族给色：地面 r=20 / 中层 r=60 / 表层 r=100（同时帧高 64 ⇒ b=16）
        let map = 造图(2, 2, |_, _| mir2_map::Cell {
            bk_img: 1,
            mid_img: 1,
            fr_img: 1,
            ..Default::default()
        });
        let src = 假源 { w: 96, h: 64 };
        let vp = Viewport::new(0, 0, 2, 2);
        let 取 = |m: LayerMask| try_render_region(&map, &src, &vp, &m).expect("单层渲染应成功");

        let 仅地面 = 取(LayerMask {
            ground: true,
            middle: false,
            front: false,
        });
        assert_eq!(仅地面.pixel(0, 0).unwrap(), [20, 0, 16, 255]);
        assert_eq!(仅地面.stats.cells_drawn, 1, "2×2 里只有偶偶格 (0,0) 铺地表");

        let 仅中层 = 取(LayerMask {
            ground: false,
            middle: true,
            front: false,
        });
        assert_eq!(仅中层.pixel(0, 0).unwrap(), [60, 0, 16, 255]);
        assert_eq!(仅中层.stats.cells_drawn, 4, "中层逐格铺");

        let 仅表层 = 取(LayerMask {
            ground: false,
            middle: false,
            front: true,
        });
        assert_eq!(仅表层.pixel(0, 0).unwrap(), [100, 0, 16, 255]);

        // 三层全开 ⇒ 表层压在最上（层序 地表 → 中层 → 表层）
        let 全开 = 取(LayerMask::all());
        assert_eq!(全开.pixel(0, 0).unwrap(), [100, 0, 16, 255]);
        assert_eq!(全开.stats.cells_drawn, 4);
        assert_eq!(全开.stats.frames_decoded, 3, "三族各解码一次");
        // 关掉的层不画也不计数
        assert_eq!(仅中层.stats.missing_ground, 0);
        assert_eq!(仅中层.stats.missing_front, 0);
    }

    #[test]
    fn 中层锚点左上角_表层底边对齐() {
        // 造 2×2 图，只有 (1,1) 有中层与表层帧；视口给 2×3，好观察帧底边是否越界
        let map = 造图(2, 2, |x, y| {
            if x == 1 && y == 1 {
                mir2_map::Cell {
                    mid_img: 1,
                    fr_img: 1,
                    ..Default::default()
                }
            } else {
                mir2_map::Cell::default()
            }
        });
        // 中层帧 48×32（正好一格），表层帧 48×64（两格高）
        struct 双源;
        impl SpriteSource for 双源 {
            fn frame(&self, lib: LibRef, _index: u32) -> Option<Frame> {
                let (w, h, c) = match lib.family {
                    mir2_assets::Family::Middle => (48u32, 32u32, [0u8, 200, 0, 255]),
                    _ => (48u32, 64u32, [200u8, 0, 0, 255]),
                };
                let mut rgba = Vec::new();
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
        let vp = Viewport::new(0, 0, 2, 3);
        let out = try_render_region(
            &map,
            &双源,
            &vp,
            &LayerMask {
                ground: false,
                middle: true,
                front: false,
            },
        )
        .unwrap();
        // 中层：格 (1,1) 左上角 = (48,32)
        assert_eq!(out.pixel(48, 32).unwrap(), [0, 200, 0, 255]);
        assert_eq!(out.pixel(47, 32).unwrap()[3], 0, "格左侧不该有像素");

        let out = try_render_region(
            &map,
            &双源,
            &vp,
            &LayerMask {
                ground: false,
                middle: false,
                front: true,
            },
        )
        .unwrap();
        // 表层：帧高 64 ⇒ y_top = (1+1)*32-64 = 0，底边正好压在 y=64（格底）
        assert_eq!(out.pixel(48, 0).unwrap(), [200, 0, 0, 255]);
        assert_eq!(out.pixel(48, 63).unwrap(), [200, 0, 0, 255]);
        assert_eq!(out.pixel(48, 64).unwrap()[3], 0, "帧底边不得超过格底");
    }

    /// 低阻挡标记色 + 默认 alpha（`CollisionMarkers.cs:6` 的 `Color.FromArgb(90,240,85,105)`）。
    const 低标记: [u8; 4] = [240, 85, 105, CollisionOverlay::DEFAULT_ALPHA];
    /// 高位阻挡标记色 + 默认 alpha（`CollisionMarkers.cs:7` 的 `Color.FromArgb(90,65,165,245)`）。
    const 高标记: [u8; 4] = [65, 165, 245, CollisionOverlay::DEFAULT_ALPHA];

    #[test]
    fn 默认叠加口径与旧路径逐字节相同() {
        // 一张「三层都有图 + 各通道阻挡位都点着」的图，最容易被误伤
        let map = 造图(4, 4, |x, _| mir2_map::Cell {
            bk_img: 0x8001 | (x << 1),
            mid_img: 3,
            fr_img: 0x8005,
            ..Default::default()
        });
        let src = 假源 { w: 96, h: 64 };
        let vp = Viewport::new(0, 0, 4, 4);
        let 旧 = try_render_region(&map, &src, &vp, &LayerMask::all()).unwrap();
        let 新 = try_render_region_with(&map, &src, &vp, &LayerMask::all(), CollisionOverlay::default())
            .unwrap();
        assert_eq!(旧.rgba, 新.rgba, "默认（两通道全关）必须与旧路径逐字节相同");
        assert_eq!(旧.stats, 新.stats, "统计也必须完全相同");
        assert_eq!(旧, 新, "整个 Rendered 必须相等");
        assert_eq!(新.stats.cells_low_blocked, 0, "默认不得有任何低阻挡标记");
        assert_eq!(新.stats.cells_high_blocked, 0, "默认不得有任何高阻挡标记");
        // 画面上也不能出现任何标记色（三层精灵都不透明，真叠了必然改像素）
        let px = 新.rgba.as_chunks::<4>().0;
        assert!(!(px.contains(&低标记) || px.contains(&高标记)));
    }

    /// 造一张 2×2 的「通道矩阵」图：
    /// `(0,0)` 只低阻挡 / `(0,1)` 只高阻挡 / `(1,0)` 两者都有 / `(1,1)` 都没有。
    fn 通道矩阵图() -> Map {
        造图(2, 2, |x, y| match (x, y) {
            (0, 0) => mir2_map::Cell {
                bk_img: 0x8000,
                ..Default::default()
            },
            (0, 1) => mir2_map::Cell {
                fr_img: 0x8000,
                ..Default::default()
            },
            (1, 0) => mir2_map::Cell {
                bk_img: 0x8000,
                fr_img: 0x8000,
                ..Default::default()
            },
            _ => mir2_map::Cell::default(),
        })
    }

    #[test]
    fn 只开低通道时高阻挡格一个像素都不染() {
        // 三层全关 ⇒ 画布本来是全透明，标记色能原样读出来（over 在 a_d=0 时输出源色）
        let map = 通道矩阵图();
        let src = 假源 { w: 96, h: 64 };
        let vp = Viewport::new(0, 0, 2, 2);
        let out =
            try_render_region_with(&map, &src, &vp, &LayerMask::none(), CollisionOverlay::low_only())
                .unwrap();

        assert_eq!(out.stats.cells_low_blocked, 2, "(0,0) 与 (1,0) 带低阻挡位");
        assert_eq!(out.stats.cells_high_blocked, 0, "高通道关着，一格都不该计");
        assert_eq!(out.stats.cells_drawn, 0, "三层全关，落笔格数必须仍是 0");

        // (0,0)：整格低标记
        assert_eq!(out.pixel(0, 0).unwrap(), 低标记);
        assert_eq!(out.pixel(47, 31).unwrap(), 低标记);
        // (1,0) 两者都有，但只开低 ⇒ **整格**低标记（右半格也不得染成高色）
        assert_eq!(out.pixel(48, 0).unwrap(), 低标记);
        assert_eq!(out.pixel(95, 31).unwrap(), 低标记);
        // (0,1) 只有高阻挡位 ⇒ 高通道关着 ⇒ 一个像素都不该画
        assert_eq!(out.pixel(0, 32).unwrap(), [0, 0, 0, 0], "只开低时高阻挡格不得被染");
        assert_eq!(out.pixel(47, 63).unwrap(), [0, 0, 0, 0]);
        // (1,1) 两个位都没有 ⇒ 空白
        assert_eq!(out.pixel(48, 32).unwrap(), [0, 0, 0, 0]);

        // 全图不得出现任何高标记色
        assert!(
            !out.rgba.as_chunks::<4>().0.contains(&高标记),
            "只开低通道时，画面上不得出现高阻挡色"
        );
    }

    #[test]
    fn 只开高通道时低阻挡格一个像素都不染() {
        let map = 通道矩阵图();
        let src = 假源 { w: 96, h: 64 };
        let vp = Viewport::new(0, 0, 2, 2);
        let out = try_render_region_with(
            &map,
            &src,
            &vp,
            &LayerMask::none(),
            CollisionOverlay::high_only(),
        )
        .unwrap();

        assert_eq!(out.stats.cells_low_blocked, 0, "低通道关着，一格都不该计");
        assert_eq!(out.stats.cells_high_blocked, 2, "(0,1) 与 (1,0) 带高阻挡位");
        // (0,0) 只有低阻挡位 ⇒ 高通道关着 ⇒ 不画
        assert_eq!(out.pixel(0, 0).unwrap(), [0, 0, 0, 0]);
        // (0,1)：整格高标记
        assert_eq!(out.pixel(0, 32).unwrap(), 高标记);
        // (1,0) 两者都有，但只开高 ⇒ 整格高标记（左半格不得是低色）
        assert_eq!(out.pixel(48, 0).unwrap(), 高标记);
        assert_eq!(out.pixel(95, 31).unwrap(), 高标记);
        assert!(
            !out.rgba.as_chunks::<4>().0.contains(&低标记),
            "只开高通道时，画面上不得出现低阻挡色"
        );
    }

    #[test]
    fn 两通道全开时同格左低右高() {
        let map = 通道矩阵图();
        let src = 假源 { w: 96, h: 64 };
        let vp = Viewport::new(0, 0, 2, 2);
        let out =
            try_render_region_with(&map, &src, &vp, &LayerMask::none(), CollisionOverlay::all())
                .unwrap();

        assert_eq!(out.stats.cells_low_blocked, 2);
        assert_eq!(out.stats.cells_high_blocked, 2);
        // (1,0) 两者都有 ⇒ 左半格低（x=48..71）、右半格高（x=72..95）
        assert_eq!(out.pixel(48, 0).unwrap(), 低标记);
        assert_eq!(out.pixel(71, 31).unwrap(), 低标记, "左半格边界 x=71 仍是低");
        assert_eq!(out.pixel(72, 0).unwrap(), 高标记, "右半格从 x=72 起是高");
        assert_eq!(out.pixel(95, 31).unwrap(), 高标记);
        // (0,1) 只有高 ⇒ 整格高；(0,0) 只有低 ⇒ 整格低
        assert_eq!(out.pixel(0, 0).unwrap(), 低标记);
        assert_eq!(out.pixel(0, 32).unwrap(), 高标记);
        // (1,1) 什么都没有
        assert_eq!(out.pixel(48, 32).unwrap(), [0, 0, 0, 0]);
    }

    #[test]
    fn 标记不改三层的落笔与缺帧计数() {
        // 一张有地表/中层/表层图号的图，格上再点满阻挡位
        let map = 造图(4, 4, |_, _| mir2_map::Cell {
            bk_img: 0x8001,
            mid_img: 0x8001,
            fr_img: 0x8001,
            ..Default::default()
        });
        let src = 假源 { w: 96, h: 64 };
        let vp = Viewport::new(0, 0, 4, 4);
        let 无标记 = try_render_region(&map, &src, &vp, &LayerMask::all()).unwrap();
        let 有标记 =
            try_render_region_with(&map, &src, &vp, &LayerMask::all(), CollisionOverlay::all())
                .unwrap();
        assert_eq!(有标记.stats.cells_drawn, 无标记.stats.cells_drawn, "落笔格数不受标记影响");
        assert_eq!(有标记.stats.missing_ground, 无标记.stats.missing_ground);
        assert_eq!(有标记.stats.missing_middle, 无标记.stats.missing_middle);
        assert_eq!(有标记.stats.missing_front, 无标记.stats.missing_front);
        assert_eq!(有标记.stats.frames_decoded, 无标记.stats.frames_decoded);
        // 16 格全带低阻挡位（4×4 视口全在图内）；高阻挡同理
        assert_eq!(有标记.stats.cells_low_blocked, 16);
        assert_eq!(有标记.stats.cells_high_blocked, 16);
        // 图不同了（叠了标记），像素必须不同 —— 否则说明「开了等于没开」
        assert_ne!(有标记.rgba, 无标记.rgba);
    }

    #[test]
    fn 阻挡标记的alpha为零时不计也不画() {
        let map = 通道矩阵图();
        let src = 假源 { w: 96, h: 64 };
        let vp = Viewport::new(0, 0, 2, 2);
        let 全透明 = CollisionOverlay {
            low: true,
            high: true,
            alpha: 0,
        };
        let out = try_render_region_with(&map, &src, &vp, &LayerMask::none(), 全透明).unwrap();
        assert_eq!(out.stats.cells_low_blocked, 0, "alpha=0 一个像素都没写，不该计数");
        assert_eq!(out.stats.cells_high_blocked, 0);
        assert!(out.rgba.iter().all(|b| *b == 0));
    }
}
