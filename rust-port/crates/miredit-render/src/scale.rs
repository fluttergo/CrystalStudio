//! 多倍率渲染：`0.25 / 0.5 / 1 / 2`（**缩放是后处理，不改变合成结果**）。
//!
//! # 一、为什么单开一条入口，而不是让 `Viewport::scale` 支持非 1.0
//!
//! 旧入口 [`crate::try_render_region`] / [`crate::try_render_region_with`] /
//! [`crate::render_region`] 被 GUI、网页版与测试共 5 处调用，`scale != 1.0` 一律
//! [`RenderError::UnsupportedScale`]。那条契约**一个字都不改**（本单硬约束）：
//! 多倍率走新入口 [`try_render_region_scaled`]，倍率由 [`RenderScale`] 枚举显式给出。
//!
//! # 二、算法：先 1× 合成，再整幅重采样
//!
//! ```text
//! compose::render(1×)  ──►  RGBA w*48 × h*32  ──►  重采样(...)  ──►  w*48*f × h*32*f
//! ```
//!
//! 1. **1× 那一步完全复用** [`crate::compose::render`]（三层逻辑、锚点、帧号、缺帧口径
//!    一个字节都不动）⇒ 缩放**不可能**改变「画了什么」，只改「每个像素多大」；
//! 2. **重采样**：缩小（`0.25` / `0.5`）走 [`crate::resample::盒式_流式`]（面积加权 `Box`
//!    的**流式**实现，见 §七），放大（`2`）走 [`crate::resample::缩放`] + `Nearest`。
//!    不为地图渲染另写一套缩放：印章与地图的像素口径必须同源，否则同一张图在两个面板里
//!    观感会不一样（`resample.rs` 的模块文档 §口径 已经把这条写死）。
//!
//! # 三、滤镜裁决（**为什么缩小用 Box、放大用 Nearest**）
//!
//! | 档位 | 滤镜 | 理由 |
//! |---|---|---|
//! | `0.25` / `0.5` | [`FilterClass::Box`] | **面积加权平均**：目标像素 = 源矩形的面积加权均值。`0.5×` 时它**正好**是每个 2×2 块的均值、`0.25×` 是 4×4 块的均值 ⇒ 不丢细节、不引入新结构。 |
//! | `2` | [`FilterClass::Nearest`] | 放大时 Box/Lanczos 会把边缘插值成渐变，像素画就糊了 —— 那是「假像素」。与 `stamp.rs:466` 的 `选滤镜` 同一条裁决（放大一律 Nearest）。 |
//! | `1` | （不重采样） | 直接转调旧路径，见 §四。 |
//!
//! 缩小为什么**不用** `Lanczos3`：它带负瓣（振铃），在纯色块边界会过冲出 `0..=255` 之外的
//! 亮边/暗边，缩得越狠越明显。验收口径要求「`0.5×` 的每个像素 ≈ 对应 2×2 块的平均值」，
//! 振铃会让这条不成立；`Box` 是唯一**逐像素等于块均值**的选项。
//! （`Lanczos3` 仍留在 `resample.rs` 里给印章缩略图用 —— 那里要的是「锐」，不是「准」。）
//!
//! # 四、`RenderScale::One` ⇒ 与旧路径**逐字节相同**
//!
//! `One` 档**直接 `return compose::render(...)`**，不经过任何重采样代码路径
//! （连 `resample::缩放` 的函数入口都不进）。这是「逐字节相同」唯一可靠的保证方式：
//! 只要走一次 `Box(1:1)` 就多一层浮点往返，不能保证 `u8` 逐字节还原。
//!
//! # 五、尺寸与超限（**先算，再决定渲不渲**）
//!
//! - 目标尺寸口径：`round(w*48*factor) × round(h*32*factor)`（[`RenderScale::pixel_size`]）；
//! - **目标图字节上限** [`SCALED_MAX_BYTES`]（64 MiB）：目标 `w*h*4` 超限 ⇒
//!   [`RenderError::TooLarge`]，**在分配任何缓冲之前**就返回（顺序见函数体 §2.2）。
//!   典型：`64×48` 格 `2×` = `6144×3072×4 B = 75,497,472 B > 67,108,864 B` ⇒ 报错。
//! - **1× 中间图字节上限** [`INTERMEDIATE_MAX_BYTES`]（128 MiB）：**先算 `原宽*原高*4`，
//!   超限 ⇒ [`RenderError::IntermediateTooLarge`]**，同样在取帧之前返回（函数体 §2.3）。
//!   为什么必须有：`200×150` 格 `@0.25` 的目标只有 `2400×1200×4 = 11.5 MB`（远低于目标
//!   上限），而 1× 中间图是 `9600×4800×4 = 184,320,000 B（175.8 MiB）` —— 只卡目标尺寸
//!   的话这条请求会被放行并**真分配** 176 MB。目标小 ≠ 中间图小，这是两条独立的护栏。
//! - 1× 中间图本身另受 [`crate::Viewport::pixel_size`] 那条老护栏约束
//!   （单边 ≤ 32768、总像素 ≤ 256 M），不满足时是 [`RenderError::DimensionOverflow`]。
//! - ⚠ [`RenderScale::One`] **只受目标图/老护栏约束**：它不进重采样、不产生中间图
//!   （§1 直接转调 `compose::render`），套上中间图上限等于凭空砍掉一半可渲范围。
//!
//! # 六、统计口径（`stats` 不随缩放变）
//!
//! [`Rendered::stats`] 原样来自 1× 合成：`cells_drawn` / `missing_*` / `frames_decoded` /
//! `cells_*_blocked` 描述的**都是「画了什么」**，缩放是纯后处理，不改这些事实
//! （缩完不会多画一格，也不会少缺一帧）。所以 `0.5×` 与 `1×` 的 `stats` 必须完全相等
//! （有测试钉住）。
//!
//! # 七、峰值内存（2026-11-05 改动：从「整幅预乘」改成「流式累加」）
//!
//! **改前**：缩小档复用 [`crate::resample::缩放`] 的 `Box`，它内部把源图整幅预乘成
//! `Vec<[f64; 4]>`（每像素 32 B）。`64×48` 视口的 1× 源是 `3072×1536 = 4.72 M` 像素
//! ⇒ 预乘缓冲瞬时 **151 MB**，加上 1× 源 18.9 MB 与目标 4.5 MB，**0.5× 比 1× 慢 4 倍**，
//! 峰值工作集实测 269.8 MiB（改前）。
//!
//! **改后**：缩小档走 [`crate::resample::盒式_流式`] —— 边读源行边累加，峰值缓冲 =
//! 目标图 + **一行**源（`sw × 32 B`，3072 宽 = 96 KiB）+ 每列一个累加器（`dw × 32 B`）
//! + 每列一份横向权重（`dw × 16 B`）。`64×48 @0.5` 的额外开销从 151 MB 降到 **≈ 0.3 MB**。
//!
//! **数值不变**：两条路径逐项同表达式、同加法顺序 ⇒ 逐字节相同（`resample.rs` 单测
//! `流式盒式与整幅盒式逐字节相同` + `tests/scale.rs` 的 `门4b` 钉住改前 FNV 基线）。
//!
//! 剩下的固定成本是**必须**的：1× 中间图（`原宽*原高*4`，由
//! [`INTERMEDIATE_MAX_BYTES`] 封顶）+ 目标图（由 [`SCALED_MAX_BYTES`] 封顶）。

use mir2_assets::SpriteSource;
use mir2_map::Map;

use crate::compose;
use crate::resample;
use crate::stamp::FilterClass;
use crate::types::{
    目标像素, CollisionOverlay, LayerMask, RenderError, RenderOptions, RenderScale, Rendered,
    Viewport, INTERMEDIATE_MAX_BYTES, SCALED_MAX_BYTES, CELL_H, CELL_W, MAX_PIXELS, MAX_SIDE,
};

/// 多倍率渲染：按 [`RenderScale`] 出一张 `w*48*factor × h*32*factor` 的 RGBA8 位图。
///
/// # 语义
///
/// 1. **先按 1× 合成**（完全复用 [`crate::compose::render`]：三层、锚点、帧号、缺帧口径
///    与旧路径一字不差），**再整幅重采样**到目标尺寸；
/// 2. `scale == [`RenderScale::One`]` ⇒ **直接转调** [`crate::try_render_region_with`]
///    （或带选项的 [`crate::try_render_region_with_options`]）的同一条内部路径，
///    输出与它**逐字节相同**（含 `stats`）；
/// 3. 缩小（`0.25` / `0.5`）用面积加权 `Box`（**流式实现**，峰值只剩「目标图 + 一行源」），
///    放大（`2`）用 `Nearest`（理由见模块文档 §三、§七）；
/// 4. 输出尺寸**精确等于** `round(w*48*factor) × round(h*32*factor)`
///    （[`RenderScale::pixel_size`]），不会因为取整多一像素或少一像素。
///
/// # 错误
///
/// - [`RenderError::UnsupportedScale`]：`vp.scale != 1.0`。倍率由 `scale` 参数给，
///   **不读也不改写** `vp.scale`（两套倍率同时生效只会让人算错尺寸，不静默合并）；
/// - [`RenderError::ZeroCells`]：`cells_w` 或 `cells_h` 为 0；
/// - [`RenderError::DimensionOverflow`]：1× 中间图本身越护栏（单边 > 32768 或总像素 > 256 M）
///   —— 那种视口连旧路径都渲不出来；
/// - [`RenderError::TooLarge`]：目标 `w*h*4 > [`SCALED_MAX_BYTES`]`（64 MiB）。
///   **在分配任何缓冲之前**返回（不硬渲、不静默降级成 1×）；
/// - [`RenderError::IntermediateTooLarge`]：**1× 中间图** `原宽*原高*4 >
///   [`INTERMEDIATE_MAX_BYTES`]`（128 MiB）。**在取帧之前**返回 —— 目标图小不代表中间图小
///   （`200×150 @0.25`：目标 11.5 MB / 中间图 175.8 MiB），这一条专门堵那个洞。
///   只有缩放档会触发；[`RenderScale::One`] 不产生中间图，不受它约束。
///
/// 缺帧**不是错误**（与旧路径同口径：不画、计 `missing_*`）。
///
/// # 统计
///
/// `stats` 原样来自 1× 合成（模块文档 §六）：同一视口下 `0.5×` 与 `1×` 的 `stats` 完全相等。
///
/// # 例子
///
/// ```
/// use miredit_render::{
///     try_render_region_scaled, CollisionOverlay, LayerMask, RenderScale, Viewport,
/// };
/// use mir2_map::Map;
/// use mir2_assets::{Frame, LibRef, SpriteSource};
///
/// struct 空源;
/// impl SpriteSource for 空源 {
///     fn frame(&self, _lib: LibRef, _index: u32) -> Option<Frame> { None }
/// }
///
/// let mut bytes = vec![0u8; 52 + 12 * 4];
/// bytes[0..2].copy_from_slice(&2u16.to_le_bytes());
/// bytes[2..4].copy_from_slice(&2u16.to_le_bytes());
/// let map = Map::parse(&bytes)?;
///
/// // 2×2 格：1× = 96×64，半倍 = 48×32，双倍 = 192×128
/// for (档, 期望) in [
///     (RenderScale::Quarter, (24u32, 16u32)),
///     (RenderScale::Half, (48, 32)),
///     (RenderScale::One, (96, 64)),
///     (RenderScale::Two, (192, 128)),
/// ] {
///     let out = try_render_region_scaled(
///         &map, &空源, &Viewport::new(0, 0, 2, 2),
///         &LayerMask::all(), CollisionOverlay::none(), 档,
///     )?;
///     assert_eq!((out.width, out.height), 期望);
///     assert_eq!(out.scale, 档);
///     assert_eq!(out.rgba.len(), (期望.0 * 期望.1 * 4) as usize);
/// }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn try_render_region_scaled(
    map: &Map,
    src: &dyn SpriteSource,
    vp: &Viewport,
    mask: &LayerMask,
    overlay: CollisionOverlay,
    scale: RenderScale,
) -> Result<Rendered, RenderError> {
    try_render_region_scaled_with(map, src, vp, mask, overlay, scale, RenderOptions::default())
}

/// 与 [`try_render_region_scaled`] **逐字节相同**，只是多一个 [`RenderOptions`]
/// （`doors_open`：门开帧偏移，见 crate 文档 §九）。
///
/// `options == RenderOptions::default()` 时与 [`try_render_region_scaled`] 是同一次调用
/// —— 后者就是按默认值转调本函数。`RenderScale::One` 档仍然**直接转调 1× 路径**
/// （[`compose::render`]），所以 `One` 档与 [`crate::try_render_region_with_options`]
/// 在同一个 `options` 下逐字节相同。
///
/// 缩放是纯后处理（模块文档 §二/§六）：门偏移在 1× 合成阶段就生效，重采样只是改
/// 「每个像素多大」，不会改变「画了哪一帧」。
pub fn try_render_region_scaled_with(
    map: &Map,
    src: &dyn SpriteSource,
    vp: &Viewport,
    mask: &LayerMask,
    overlay: CollisionOverlay,
    scale: RenderScale,
    options: RenderOptions,
) -> Result<Rendered, RenderError> {
    // ---- 1. 1× 档：**原封不动**转调旧路径 ----
    //
    // 这是「与 `try_render_region_with` 逐字节相同」唯一可靠的保证：一旦走
    // `resample::缩放(1:1)`，就会多一层「预乘 f64 → 除回 u8」的往返，`u8` 未必逐字节还原。
    if scale == RenderScale::One {
        return compose::render(map, src, vp, mask, overlay, options);
    }

    // ---- 2. 前置校验（顺序即优先级，全部发生在分配之前）----
    //
    // 2.0 `Viewport::scale` 必须是 1.0：倍率只认 `scale` 参数，两套倍率不合并、不静默改写。
    if vp.scale != 1.0 {
        return Err(RenderError::UnsupportedScale {
            scale_milli: (vp.scale as f64 * 1000.0) as i64,
        });
    }
    if vp.cells_w == 0 || vp.cells_h == 0 {
        return Err(RenderError::ZeroCells);
    }
    // 2.1 1× 中间图必须**本身可渲**（与 `Viewport::pixel_size` / `compose` 同一条老护栏）。
    //     先判它，是为了让「连 1× 都渲不出来的视口」拿到诚实的 DimensionOverflow，
    //     而不是被后面的字节上限笼统地报成 TooLarge。
    let 原宽 = u64::from(vp.cells_w) * u64::from(CELL_W);
    let 原高 = u64::from(vp.cells_h) * u64::from(CELL_H);
    if 原宽 > MAX_SIDE || 原高 > MAX_SIDE || 原宽 * 原高 > MAX_PIXELS {
        return Err(RenderError::DimensionOverflow {
            width: 原宽,
            height: 原高,
        });
    }
    // 2.2 目标尺寸 + 字节数：**先算完再决定渲不渲**（超限时一个字节都不分配）。
    let (宽, 高) = 目标像素(vp.cells_w, vp.cells_h, scale);
    let 字节 = 宽.saturating_mul(高).saturating_mul(4);
    if 字节 > SCALED_MAX_BYTES {
        return Err(RenderError::TooLarge {
            want: 字节,
            limit: SCALED_MAX_BYTES,
        });
    }
    // 2.3 **1× 中间图**的字节数：同样先算完再决定渲不渲（超限时连一帧都不取）。
    //
    //     为什么必须有这一条：目标图小 ≠ 中间图小。`200×150` 格 `@0.25` 的目标只有
    //     `2400×1200×4 = 11.5 MB`（远低于 64 MiB 目标上限），可 1× 中间图是
    //     `9600×4800×4 = 184,320,000 B（175.8 MiB）` —— 老代码会**真分配**它。
    //     网页服务端一条 URL 就能被打爆，所以中间图必须有自己的上界。
    //
    //     顺序说明：**先判目标图、再判中间图**。目标是调用方明确要的交付物，它超限时
    //     报 `TooLarge` 更贴切；而且 `@2×` 时目标图恒等于中间图的 4 倍 ⇒ 目标图先撞线，
    //     本变体只可能在缩小档（`0.25` / `0.5`）触发 —— 正是要拦的那一类。
    let 中间字节 = 原宽.saturating_mul(原高).saturating_mul(4);
    if 中间字节 > INTERMEDIATE_MAX_BYTES {
        return Err(RenderError::IntermediateTooLarge {
            want: 中间字节,
            limit: INTERMEDIATE_MAX_BYTES,
        });
    }
    // 2.4 上面的字节上限已保证 `宽*高*4 ≤ 64 MiB` ⇒ 宽、高都 ≤ 16.7 M，`as u32` 不会截断。
    let (宽, 高) = (宽 as u32, 高 as u32);

    // ---- 3. 1× 合成（三层逻辑一字不改）----
    let 原 = compose::render(map, src, vp, mask, overlay, options)?;

    // ---- 4. 整幅重采样（纯后处理）----
    let rgba = 重采样(&原.rgba, 原.width, 原.height, 宽, 高, scale);

    Ok(Rendered {
        width: 宽,
        height: 高,
        rgba,
        // 统计描述的是「1× 合成了什么」，缩放不改这个事实（模块文档 §六）
        stats: 原.stats,
        scale,
    })
}

/// 把 1× 合成结果重采样到目标尺寸（**纯后处理**，不改任何「画了什么」）。
///
/// | 档位 | 走的实现 | 为什么 |
/// |---|---|---|
/// | `0.25` / `0.5` | [`resample::盒式_流式`] | 面积加权 `Box` 的**流式**版本：峰值只剩「目标图 + 一行源」，不再落整幅 `f64` 预乘（模块文档 §七）|
/// | `2` | [`resample::缩放`] + `Nearest` | 放大不插值（像素画不许糊），且放大不需要流式（源行只有 `sh` 行）|
/// | `1` | 走不到这里 | §1 已 `return compose::render(...)`，`One` 不产生中间图 |
///
/// 两条缩小路径**数值等价**：`盒式_流式` 与 `Box` 的 `盒式` 逐项同表达式、同加法顺序
/// ⇒ 输出逐字节相同（`resample.rs` 的单测 + `tests/scale.rs` 的 `门4b` 双向钉住）。
fn 重采样(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32, scale: RenderScale) -> Vec<u8> {
    if scale.is_downscale() {
        resample::盒式_流式(src, sw, sh, dw, dh)
    } else {
        resample::缩放(src, sw, sh, dw, dh, 选滤镜(scale))
    }
}

/// 滤镜裁决：缩小走面积加权 `Box`、放大走 `Nearest`（理由见模块文档 §三）。
///
/// 与 `stamp.rs:466` 的 `选滤镜` 是同一条口径（放大一律 Nearest），
/// 区别只在这里把档位直接映射成滤镜，不做「比例 ≥ 1」的浮点比较。
///
/// 注意：缩小档虽然在这里映射成 [`FilterClass::Box`]，实际执行走的是
/// [`resample::盒式_流式`]（`Box` 的流式等价实现，见 [`重采样`]）——
/// 口径仍是 `Box`，只是换了个更省内存的实现，**输出逐字节相同**。
fn 选滤镜(scale: RenderScale) -> FilterClass {
    match scale {
        // 面积加权平均：0.5× 正好等于 2×2 块均值、0.25× 等于 4×4 块均值
        RenderScale::Quarter | RenderScale::Half => FilterClass::Box,
        // 放大不插值（像素画不许糊）；`One` 走不到这里（§1 已提前返回），列出来只为穷尽
        RenderScale::One | RenderScale::Two => FilterClass::Nearest,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::RenderStats;
    use mir2_assets::{Frame, LibRef};
    use std::cell::Cell;

    /// 假帧源：整帧一个颜色（`rgba` 由参数给），尺寸也由参数给。
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
        次数: Cell<u32>,
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

    /// 造一张 `w×h` 的 Old12 地图，格内容由闭包决定（与 `lib.rs` 的测试同款）。
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
    fn 倍率因子与标签回环() {
        assert_eq!(RenderScale::Quarter.factor(), 0.25);
        assert_eq!(RenderScale::Half.factor(), 0.5);
        assert_eq!(RenderScale::One.factor(), 1.0);
        assert_eq!(RenderScale::Two.factor(), 2.0);
        // 标签 ↔ 解析必须回环（网页/命令行写 `label()` 能原样解析回来）
        for s in RenderScale::ALL {
            assert_eq!(RenderScale::from_param(s.label()), Some(s), "{}", s.label());
        }
        assert_eq!(RenderScale::ALL.len(), 4);
        assert!(RenderScale::Quarter.is_downscale() && RenderScale::Half.is_downscale());
        assert!(!RenderScale::One.is_downscale() && !RenderScale::Two.is_downscale());
    }

    #[test]
    fn 参数解析只认四个档位() {
        assert_eq!(RenderScale::from_param("0.25"), Some(RenderScale::Quarter));
        assert_eq!(RenderScale::from_param(".25"), Some(RenderScale::Quarter));
        assert_eq!(RenderScale::from_param("025"), Some(RenderScale::Quarter));
        assert_eq!(RenderScale::from_param("quarter"), Some(RenderScale::Quarter));
        assert_eq!(RenderScale::from_param(" 0.5 "), Some(RenderScale::Half));
        assert_eq!(RenderScale::from_param("half"), Some(RenderScale::Half));
        assert_eq!(RenderScale::from_param("1.0"), Some(RenderScale::One));
        assert_eq!(RenderScale::from_param("one"), Some(RenderScale::One));
        assert_eq!(RenderScale::from_param("2.0"), Some(RenderScale::Two));
        assert_eq!(RenderScale::from_param("two"), Some(RenderScale::Two));
        // 不做模糊匹配：差一点点的比例不许静默吸到某个档位（那是假数据）
        for bad in ["", "0", "0.3", "1.5", "2x", "3", "-1", "HALF", "1e0"] {
            assert_eq!(RenderScale::from_param(bad), None, "{bad:?} 不该被接受");
        }
    }

    #[test]
    fn 目标尺寸四舍五入且四档整除() {
        // 8×6 格：1× = 384×192，四档依次是它的 1/4、1/2、1、2 倍
        assert_eq!(RenderScale::Quarter.pixel_size(8, 6), Some((96, 48)));
        assert_eq!(RenderScale::Half.pixel_size(8, 6), Some((192, 96)));
        assert_eq!(RenderScale::One.pixel_size(8, 6), Some((384, 192)));
        assert_eq!(RenderScale::Two.pixel_size(8, 6), Some((768, 384)));
        // 与公式 round(w*48*f) × round(h*32*f) 逐项对照（奇数格数也要整除）
        for (cw, ch) in [(1u32, 1u32), (5, 3), (7, 9), (64, 48)] {
            for s in RenderScale::ALL {
                let f = f64::from(s.factor());
                let 期望 = (
                    (f64::from(cw) * 48.0 * f).round() as u32,
                    (f64::from(ch) * 32.0 * f).round() as u32,
                );
                assert_eq!(s.pixel_size(cw, ch), Some(期望), "{cw}×{ch} @ {s:?}");
            }
        }
        // 0 格 ⇒ None（渲染入口会报 ZeroCells）
        assert_eq!(RenderScale::Half.pixel_size(0, 6), None);
        assert_eq!(RenderScale::Half.pixel_size(8, 0), None);
    }

    #[test]
    fn 一档与旧路径逐字节相同() {
        // 三层都有图 + 阻挡通道全开：最容易暴露「多走一次重采样」的差异
        let map = 造图(4, 4, |x, _| mir2_map::Cell {
            bk_img: 0x8001 | (x << 1),
            mid_img: 3,
            fr_img: 0x8005,
            ..Default::default()
        });
        let src = 纯色源 {
            w: 96,
            h: 64,
            c: [7, 8, 9, 200],
        };
        let vp = Viewport::new(0, 0, 4, 4);
        let 旧 = crate::try_render_region_with(&map, &src, &vp, &LayerMask::all(), CollisionOverlay::all())
            .expect("旧路径应成功");
        let 新 = try_render_region_scaled(
            &map,
            &src,
            &vp,
            &LayerMask::all(),
            CollisionOverlay::all(),
            RenderScale::One,
        )
        .expect("1× 应成功");
        assert_eq!(旧.rgba, 新.rgba, "1× 档必须与旧路径逐字节相同");
        assert_eq!(旧.stats, 新.stats, "统计也必须相同");
        assert_eq!(新.scale, RenderScale::One);
        assert_eq!(旧, 新, "整个 Rendered 必须相等（含新增的 scale 字段）");
    }

    #[test]
    fn 四档输出尺寸精确且长度自洽() {
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
            let 期望 = s.pixel_size(8, 6).unwrap();
            assert_eq!((out.width, out.height), 期望, "{s:?}");
            assert_eq!(out.rgba.len(), (期望.0 * 期望.1 * 4) as usize, "{s:?}");
            assert_eq!(out.scale, s);
        }
    }

    #[test]
    fn 统计不随缩放改变() {
        // 有地表/中层/表层图号 + 一层缺帧（表层帧源给 None 的索引）—— 统计应逐项相等
        let map = 造图(8, 6, |x, y| mir2_map::Cell {
            bk_img: if x % 2 == 0 && y % 2 == 0 { 1 } else { 0 },
            mid_img: 2,
            fr_img: 3,
            ..Default::default()
        });
        struct 半缺源;
        impl SpriteSource for 半缺源 {
            fn frame(&self, lib: LibRef, index: u32) -> Option<Frame> {
                // 表层帧号 3 ⇒ index=2 一律取不到（制造 missing_front）
                if matches!(lib.family, mir2_assets::Family::Surface) && index == 2 {
                    return None;
                }
                Some(Frame {
                    width: 48,
                    height: 32,
                    rgba: [1, 2, 3, 255].repeat(48 * 32),
                    ..mir2_assets::Frame::default()
                })
            }
        }
        let src = 半缺源;
        let vp = Viewport::new(0, 0, 8, 6);
        let 一 = try_render_region_scaled(
            &map,
            &src,
            &vp,
            &LayerMask::all(),
            CollisionOverlay::low_only(),
            RenderScale::One,
        )
        .unwrap();
        for s in [RenderScale::Quarter, RenderScale::Half, RenderScale::Two] {
            let 缩 = try_render_region_scaled(
                &map,
                &src,
                &vp,
                &LayerMask::all(),
                CollisionOverlay::low_only(),
                s,
            )
            .unwrap();
            assert_eq!(缩.stats, 一.stats, "{s:?} 的统计必须与 1× 完全相等");
            assert!(一.stats.missing_front > 0, "本例应制造出表层缺帧");
        }
        // 顺带钉住：`RenderStats` 的默认值不该被这条路径改写
        assert_ne!(一.stats, RenderStats::default());
    }

    #[test]
    fn 超限在取帧之前就报错() {
        // 64×48 格 2× ⇒ 6144×3072×4 B = 75,497,472 B > 64 MiB ⇒ TooLarge
        let map = 造图(64, 48, |_, _| mir2_map::Cell {
            bk_img: 1,
            ..Default::default()
        });
        let src = 记账源 {
            次数: Cell::new(0),
        };
        let vp = Viewport::new(0, 0, 64, 48);
        let err = try_render_region_scaled(
            &map,
            &src,
            &vp,
            &LayerMask::all(),
            CollisionOverlay::none(),
            RenderScale::Two,
        )
        .expect_err("64×48 @ 2× 必须被拒绝");
        match err {
            RenderError::TooLarge { want, limit } => {
                assert_eq!(want, 6144 * 3072 * 4);
                assert_eq!(limit, SCALED_MAX_BYTES);
            }
            other => panic!("应为 TooLarge，实际 {other:?}"),
        }
        assert_eq!(
            src.次数.get(),
            0,
            "超限必须在分配 1× 中间图 / 取帧之前返回（帧源一次都不该被调用）"
        );
        // 同一视口的 0.25/0.5/1 都不超限（证明上面的 0 次取帧不是别的原因）
        for s in [RenderScale::Quarter, RenderScale::Half, RenderScale::One] {
            assert!(
                try_render_region_scaled(
                    &map,
                    &src,
                    &vp,
                    &LayerMask::all(),
                    CollisionOverlay::none(),
                    s
                )
                .is_ok(),
                "{s:?} 不该超限"
            );
        }
    }

    #[test]
    fn 视口倍率非一被拒绝不静默改写() {
        let map = 造图(2, 2, |_, _| mir2_map::Cell::default());
        let src = 纯色源 {
            w: 48,
            h: 32,
            c: [1, 2, 3, 255],
        };
        let mut vp = Viewport::new(0, 0, 2, 2);
        vp.scale = 0.5;
        match try_render_region_scaled(
            &map,
            &src,
            &vp,
            &LayerMask::all(),
            CollisionOverlay::none(),
            RenderScale::Half,
        ) {
            Err(RenderError::UnsupportedScale { scale_milli }) => assert_eq!(scale_milli, 500),
            other => panic!("vp.scale != 1.0 必须被拒绝，实际 {other:?}"),
        }
        // 视口本身没被改写（本库无状态）
        assert_eq!(vp.scale, 0.5);
    }

    #[test]
    fn 零格与一倍不可渲的视口各自报对错误() {
        let map = 造图(2, 2, |_, _| mir2_map::Cell::default());
        let src = 纯色源 {
            w: 48,
            h: 32,
            c: [1, 2, 3, 255],
        };
        // 0 格 ⇒ ZeroCells
        assert_eq!(
            try_render_region_scaled(
                &map,
                &src,
                &Viewport::new(0, 0, 0, 4),
                &LayerMask::all(),
                CollisionOverlay::none(),
                RenderScale::Half
            )
            .unwrap_err(),
            RenderError::ZeroCells
        );
        // 1× 中间图越护栏（100000 格宽 × 48 = 4,800,000 > 32768）⇒ DimensionOverflow，
        // **不是** TooLarge：连 1× 都渲不出来的视口，报字节超限是不诚实的
        assert!(matches!(
            try_render_region_scaled(
                &map,
                &src,
                &Viewport::new(0, 0, 100_000, 4),
                &LayerMask::all(),
                CollisionOverlay::none(),
                RenderScale::Quarter
            )
            .unwrap_err(),
            RenderError::DimensionOverflow { .. }
        ));
    }

    #[test]
    fn 缩小用盒式放大用最近邻() {
        assert_eq!(选滤镜(RenderScale::Quarter), FilterClass::Box);
        assert_eq!(选滤镜(RenderScale::Half), FilterClass::Box);
        assert_eq!(选滤镜(RenderScale::Two), FilterClass::Nearest);
        // 1× 走不到选滤镜，但函数本身必须是全映射（不 panic）
        assert_eq!(选滤镜(RenderScale::One), FilterClass::Nearest);
    }

    #[test]
    fn 半倍是二乘二块平均而不是丢点采样() {
        // 96×64 帧，像素按 (x%2, y%2) 四色循环 ⇒ 1× 图上每个 2×2 块恰好含四种颜色各一次
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

        let mut 最大偏差 = 0i32;
        for y in 0..半.height {
            for x in 0..半.width {
                // 参考值：对应 2×2 块的**直通 alpha 均值**（本例 alpha 恒 255，预乘无影响）
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
                    let 偏差 = (i32::from(实际[c]) - 期望[c]).abs();
                    最大偏差 = 最大偏差.max(偏差);
                }
            }
        }
        assert!(
            最大偏差 <= 1,
            "0.5× 每像素应等于 2×2 块均值（只允许 ±1 的取整误差），实测最大偏差 {最大偏差}"
        );

        // 反证：**点采样只会吐出四种源色之一**（255,0,0 / 0,255,0 / 0,0,255 / 255,255,255），
        // 块均值是 (128,128,128) 量级 ⇒ 只要结果不等于任何一个源色，就证明它不是丢点采样。
        let 源色 = [
            [255u8, 0, 0],
            [0, 255, 0],
            [0, 0, 255],
            [255, 255, 255],
        ];
        for (x, y) in [(0u32, 0u32), (1, 0), (0, 1), (半.width - 1, 半.height - 1)] {
            let p = 半.pixel(x, y).unwrap();
            assert!(
                !源色.iter().any(|c| *c == [p[0], p[1], p[2]]),
                "({x},{y}) 的像素等于某个源色 {p:?} ⇒ 这是丢点采样，不是面积平均"
            );
            // 每个通道都该落在 127/128 附近（四色均值 = 127.5）
            for (c, v) in p.iter().take(3).enumerate() {
                assert!(
                    (i32::from(*v) - 128).abs() <= 1,
                    "({x},{y}) 通道 {c} 应为块均值 ~128，实测 {v}"
                );
            }
        }
    }
}
