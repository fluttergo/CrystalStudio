//! 印章缩略图的重采样内核：`Nearest` / `Box`（面积平均）/ `Lanczos3`。
//!
//! # 为什么自己写（不引图像库）
//!
//! 与 `canvas.rs` 同一条纪律：渲染核心只用 `std` + `mir2-assets`，不为了「缩一张小图」
//! 拉进一个图像处理库。三种滤镜加起来不到 200 行，且**结果可复现**（同一输入两次
//! 逐字节相等，见 `stamp.rs` 的确定性用例）。
//!
//! # 口径（三条，缺一条就会出「假像素」）
//!
//! 1. **一律在「预乘 alpha」上滤波**：图库帧的透明像素 RGB 是调色板索引 0 的颜色
//!    （常见为黑），直接对直通 alpha 做平均会把黑边糊进边缘 ⇒ 先把 `c*a/255` 乘进去，
//!    滤完再除回来。这是 Box/Lanczos 与「客户端缩略图观感」一致的唯一做法。
//! 2. **缩放中心对齐**：目标像素 `i` 覆盖源区间 `[i*src/dst, (i+1)*src/dst)`，
//!    Nearest 取该区间的中心（不是左上角）——否则整体会偏半个像素。
//! 3. **不做 gamma 校正**：本库所有合成（`canvas.rs` 的 `over`）都在 sRGB 直通空间做整数
//!    混合，缩略图沿用同一空间，避免「地图一套、印章另一套」的口径分裂。
//!
//! # 与「像素画」的关系
//!
//! **放大一律用 Nearest**（见 [`crate::render_stamp`] 的滤镜裁决）：像素画放大时
//! Box/Lanczos 会把边缘糊成渐变，那在编辑器里就是错的观感。本模块只负责「按给定滤镜算」，
//! 什么时候强制 Nearest 由调用方（`stamp.rs`）决定。

use crate::stamp::FilterClass;

/// 把 `sw×sh` 的 RGBA8 重采样成 `dw×dh`（`dw`/`dh` 必须 ≥ 1，由调用方保证）。
///
/// `src.len()` 必须是 `sw*sh*4`；越界的源缓冲只会让本函数读到错误数据，
/// 因此调用方（`stamp.rs`）必须先保证长度自洽（那里用 `Canvas` 产出，天然自洽）。
pub(crate) fn 缩放(
    src: &[u8],
    sw: u32,
    sh: u32,
    dw: u32,
    dh: u32,
    filter: FilterClass,
) -> Vec<u8> {
    debug_assert!(dw > 0 && dh > 0 && sw > 0 && sh > 0);
    debug_assert_eq!(src.len(), sw as usize * sh as usize * 4);
    match filter {
        FilterClass::Nearest => 最近邻(src, sw, sh, dw, dh),
        FilterClass::Box => 盒式(src, sw, sh, dw, dh),
        FilterClass::Lanczos => lanczos3(src, sw, sh, dw, dh),
    }
}

/// 最近邻：目标像素取源区间中心那一个像素（不混合、不糊边）。
fn 最近邻(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<u8> {
    let mut out = vec![0u8; dw as usize * dh as usize * 4];
    for y in 0..dh {
        // 区间 [y*sh/dh, (y+1)*sh/dh) 的中心 ⇒ ((2y+1)*sh) / (2dh)
        let sy = ((2 * u64::from(y) + 1) * u64::from(sh) / (2 * u64::from(dh)))
            .min(u64::from(sh) - 1) as usize;
        for x in 0..dw {
            let sx = ((2 * u64::from(x) + 1) * u64::from(sw) / (2 * u64::from(dw)))
                .min(u64::from(sw) - 1) as usize;
            let si = (sy * sw as usize + sx) * 4;
            let di = (y as usize * dw as usize + x as usize) * 4;
            out[di..di + 4].copy_from_slice(&src[si..si + 4]);
        }
    }
    out
}

/// 盒式（面积平均）：目标像素 = 源矩形内的**面积加权**均值。
///
/// 非整数倍缩小时，一个目标像素覆盖的源矩形边缘只被覆盖一部分 ⇒ 按重叠面积加权，
/// 否则缩放比例一变，边缘亮度就跳（那是「假像素」的一种）。
///
/// ⚠ 本函数会把**整幅源图预乘成 `f64`**（每像素 32 B）。`3072×1536` 的源就是 **151 MB**
/// 瞬时缓冲；大图请用 [`盒式_流式`]（与它逐字节等价，峰值只剩「目标图 + 一行源」）。
fn 盒式(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<u8> {
    let 源 = 预乘(src, sw, sh);
    let mut out = vec![0u8; dw as usize * dh as usize * 4];
    for y in 0..dh {
        let y0 = y as f64 * f64::from(sh) / f64::from(dh);
        let y1 = (y as f64 + 1.0) * f64::from(sh) / f64::from(dh);
        let iy0 = y0.floor() as i64;
        let iy1 = y1.ceil() as i64;
        for x in 0..dw {
            let x0 = x as f64 * f64::from(sw) / f64::from(dw);
            let x1 = (x as f64 + 1.0) * f64::from(sw) / f64::from(dw);
            let ix0 = x0.floor() as i64;
            let ix1 = x1.ceil() as i64;
            let mut acc = [0.0f64; 4];
            let mut 权重和 = 0.0f64;
            for iy in iy0..iy1 {
                let wy = 重叠(
                    iy as f64,
                    iy as f64 + 1.0,
                    y0,
                    y1,
                );
                if wy <= 0.0 {
                    continue;
                }
                let cy = 夹格(iy, sh) as usize;
                for ix in ix0..ix1 {
                    let wx = 重叠(ix as f64, ix as f64 + 1.0, x0, x1);
                    if wx <= 0.0 {
                        continue;
                    }
                    let cx = 夹格(ix, sw) as usize;
                    let w = wx * wy;
                    权重和 += w;
                    let p = 源[cy * sw as usize + cx];
                    for (a, v) in acc.iter_mut().zip(p.iter()) {
                        *a += v * w;
                    }
                }
            }
            if 权重和 > 0.0 {
                for a in acc.iter_mut() {
                    *a /= 权重和;
                }
            }
            let di = (y as usize * dw as usize + x as usize) * 4;
            out[di..di + 4].copy_from_slice(&还原(acc));
        }
    }
    out
}

/// **流式**面积加权盒式：与 [`盒式`] **逐字节等价**，但**不落整幅 `f64` 预乘缓冲**。
///
/// # 一、为什么要它
///
/// [`盒式`] 先把整幅源图预乘成 `Vec<[f64; 4]>`（每像素 32 B）：
///
/// | 源图（1× 合成结果）| 整幅预乘缓冲 | 说明 |
/// |---|---|---|
/// | `3072×1536`（`64×48` 格）| `4,718,592 × 32 B = 151 MB` | 0.5× 实测比 1× 慢 4 倍的主因 |
/// | `9600×4800`（`200×150` 格）| `46,080,000 × 32 B = 1.47 GB` | 直接能把服务端打死 |
///
/// 本变体**边读源行边累加**：峰值缓冲 = 目标图 + **一行**源（`sw × 32 B`，3072 宽只有
/// 96 KiB）+ 每个目标列一个累加器（`dw × 32 B`）+ 每列一份横向权重（`dw × 16 B`）。
///
/// # 二、为什么能保证**逐字节等价**（不是「差不多」）
///
/// 逐项对照 [`盒式`] 的求值顺序（下表每一行都是「同一表达式、同一 `f64` 输入」）：
///
/// | 项 | [`盒式`] | 本函数 |
/// |---|---|---|
/// | `x0 / x1 / ix0 / ix1` | `x as f64 * f64::from(sw) / f64::from(dw)` … | 同一表达式 |
/// | `wx` | `重叠(ix as f64, ix as f64 + 1.0, x0, x1)`，`<= 0.0` 跳过 | 同一表达式、同样跳过 |
/// | `wy` | 同上（`y` 方向）| 同上 |
/// | `w` | `wx * wy` | `wx * wy`（同一乘序）|
/// | 预乘 | `[c0*k, c1*k, c2*k, a]`，`k = a/255.0` | 同一表达式（[`载入预乘行`]）|
/// | `acc` 累加 | 单个目标像素内 **`iy` 升序 → `ix` 升序** | **同一顺序**（`iy` 外层、`ix` 内层）|
/// | `权重和` | 同一 `(iy, ix)` 顺序 | 同一顺序 |
/// | 归一化 | `*a /= 权重和`（仅 `权重和 > 0.0`）| 同一表达式、同一条件 |
/// | 还原 | [`还原`] | [`还原`] |
///
/// **关键点**：浮点加法不满足结合律，所以「先按行求和、再把行和累加」会与 [`盒式`] 不同。
/// 本实现把每个目标像素的累加器**跨源行保留**（`累加[x]`），逐 `(iy, ix)` 直接 `+=`，
/// 于是每个像素看到的加法序列与 [`盒式`] **完全一致** ⇒ 结果逐字节相同
/// （`tests` 里的 `流式盒式与整幅盒式逐字节相同` 用 9 组尺寸 × 含透明度的随机内容钉住）。
///
/// # 三、只给缩小用（放大仍正确，只是不划算）
///
/// 放大（`dh > sh`）时同一源行会被多个目标行重复使用，本变体每行都重新预乘一次
/// （结果仍正确，只是多读几遍）。[`crate::scale`] 的放大档走 `Nearest`，用不到这里；
/// 印章缩略图（`stamp.rs`）仍走 [`缩放`]。
pub(crate) fn 盒式_流式(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<u8> {
    debug_assert!(dw > 0 && dh > 0 && sw > 0 && sh > 0);
    debug_assert_eq!(src.len(), sw as usize * sh as usize * 4);

    // 一个目标列的横向参数：`源列[k]` 与 `权[k]` 一一对应，且**保持 `ix` 升序**
    //（= `盒式` 的内层循环顺序；顺序一乱，浮点加法就不可结合）。
    struct 列参数 {
        源列: Vec<usize>,
        权: Vec<f64>,
    }

    // ---- 1. 横向参数只算一次（`盒式` 是每个目标像素重算一遍）----
    let mut 列: Vec<列参数> = Vec::with_capacity(dw as usize);
    for x in 0..dw {
        let x0 = x as f64 * f64::from(sw) / f64::from(dw);
        let x1 = (x as f64 + 1.0) * f64::from(sw) / f64::from(dw);
        let ix0 = x0.floor() as i64;
        let ix1 = x1.ceil() as i64;
        let mut 源列: Vec<usize> = Vec::new();
        let mut 权: Vec<f64> = Vec::new();
        for ix in ix0..ix1 {
            let wx = 重叠(ix as f64, ix as f64 + 1.0, x0, x1);
            if wx <= 0.0 {
                continue;
            }
            源列.push(夹格(ix, sw) as usize);
            权.push(wx);
        }
        列.push(列参数 { 源列, 权 });
    }

    // ---- 2. 逐目标行推进，累加器跨源行保留 ----
    let mut out = vec![0u8; dw as usize * dh as usize * 4];
    // 每个目标列的 `acc`（`盒式` 里是「每像素一个局部 acc」，这里是「每列一个、按行重置」）
    let mut 累加 = vec![[0.0f64; 4]; dw as usize];
    let mut 权和 = vec![0.0f64; dw as usize];
    // 单行预乘缓冲：`sw` 个像素 × 32 B
    let mut 行 = vec![[0.0f64; 4]; sw as usize];
    // 已装进 `行` 的源行号（源行号在 `iy` 上单调不减 ⇒ 同一行不会来回装两次）
    let mut 已载行: Option<u32> = None;

    for y in 0..dh {
        let y0 = y as f64 * f64::from(sh) / f64::from(dh);
        let y1 = (y as f64 + 1.0) * f64::from(sh) / f64::from(dh);
        let iy0 = y0.floor() as i64;
        let iy1 = y1.ceil() as i64;
        // 新目标行 ⇒ 累加器清零（等价于 `盒式` 里「每个目标像素新建 acc」）
        for a in 累加.iter_mut() {
            *a = [0.0; 4];
        }
        for w in 权和.iter_mut() {
            *w = 0.0;
        }
        for iy in iy0..iy1 {
            let wy = 重叠(iy as f64, iy as f64 + 1.0, y0, y1);
            if wy <= 0.0 {
                continue;
            }
            // 与 `盒式` 同一条 `夹格`。这里它其实是**恒等**（`0 <= iy0` 且 `iy1 <= sh`，
            // 由 `y0 >= 0` / `y1 <= sh` 推出），但照抄同一表达式，避免将来改算式时偏一格。
            let cy = 夹格(iy, sh);
            if 已载行 != Some(cy) {
                载入预乘行(src, sw, cy, &mut 行);
                已载行 = Some(cy);
            }
            for (x, c) in 列.iter().enumerate() {
                for (k, wx) in c.权.iter().enumerate() {
                    let w = wx * wy;
                    权和[x] += w;
                    let p = 行[c.源列[k]];
                    for (t, v) in 累加[x].iter_mut().zip(p.iter()) {
                        *t += v * w;
                    }
                }
            }
        }
        // 归一化 + 还原（与 `盒式` 同表达式、同条件、同顺序）
        for (x, acc) in 累加.iter_mut().enumerate() {
            if 权和[x] > 0.0 {
                for a in acc.iter_mut() {
                    *a /= 权和[x];
                }
            }
            let di = (y as usize * dw as usize + x) * 4;
            out[di..di + 4].copy_from_slice(&还原(*acc));
        }
    }
    out
}

/// 把源图第 `row` 行预乘成 `f64` 写进 `out`（`out.len()` 必须等于 `sw`）。
///
/// 表达式与 [`预乘`] **逐项相同**（同 `a / 255.0`、同 `c * 系数` 的求值顺序）——
/// 这是 [`盒式_流式`] 能与 [`盒式`] 逐字节等价的前提之一。
fn 载入预乘行(src: &[u8], sw: u32, row: u32, out: &mut [[f64; 4]]) {
    debug_assert_eq!(out.len(), sw as usize);
    let base = row as usize * sw as usize * 4;
    for (i, o) in out.iter_mut().enumerate() {
        let p = &src[base + i * 4..base + i * 4 + 4];
        let a = f64::from(p[3]);
        let 系数 = a / 255.0;
        *o = [
            f64::from(p[0]) * 系数,
            f64::from(p[1]) * 系数,
            f64::from(p[2]) * 系数,
            a,
        ];
    }
}

/// Lanczos3（可分离两趟：先横后纵），预乘 alpha。
///
/// **缩小时把核按缩放比展宽**（`support = 3*scale`）：不展宽就等于「先点采样再滤波」，
/// 高频内容会整块闪掉（典型症状：缩略图里的栏杆/柱子时有时无）。
fn lanczos3(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<u8> {
    let 源 = 预乘(src, sw, sh);
    let 横权 = 权重表(sw, dw);
    let 纵权 = 权重表(sh, dh);

    // 第一趟：横向（结果仍是预乘值，尺寸 dw × sh）
    let mut 中间 = vec![[0.0f64; 4]; dw as usize * sh as usize];
    for y in 0..sh as usize {
        for (x, 权) in 横权.iter().enumerate() {
            let mut acc = [0.0f64; 4];
            for &(sx, w) in 权 {
                let p = 源[y * sw as usize + sx];
                for (a, v) in acc.iter_mut().zip(p.iter()) {
                    *a += v * w;
                }
            }
            中间[y * dw as usize + x] = acc;
        }
    }

    // 第二趟：纵向（dw × dh）⇒ 还原成直通 alpha
    let mut out = vec![0u8; dw as usize * dh as usize * 4];
    for (y, 权) in 纵权.iter().enumerate() {
        for x in 0..dw as usize {
            let mut acc = [0.0f64; 4];
            for &(sy, w) in 权 {
                let p = 中间[sy * dw as usize + x];
                for (a, v) in acc.iter_mut().zip(p.iter()) {
                    *a += v * w;
                }
            }
            let di = (y * dw as usize + x) * 4;
            out[di..di + 4].copy_from_slice(&还原(acc));
        }
    }
    out
}

/// 生成「目标下标 → [(源下标, 权重)]」表；权重按行归一化（`Σw = 1`）。
fn 权重表(源长: u32, 目标长: u32) -> Vec<Vec<(usize, f64)>> {
    let scale = f64::from(源长) / f64::from(目标长);
    // 缩小 ⇒ 展宽核；放大 ⇒ 核宽固定 3（本库放大走 Nearest，这条只作兜底）
    let 展宽 = scale.max(1.0);
    let support = 3.0 * 展宽;
    let mut 表 = Vec::with_capacity(目标长 as usize);
    for i in 0..目标长 {
        let center = (f64::from(i) + 0.5) * scale - 0.5;
        let lo = (center - support).floor() as i64;
        let hi = (center + support).ceil() as i64;
        let mut 项: Vec<(usize, f64)> = Vec::new();
        let mut 和 = 0.0f64;
        for j in lo..=hi {
            let t = (j as f64 - center) / 展宽;
            let w = 核(t);
            if w == 0.0 {
                continue;
            }
            let 源下标 = 夹格(j, 源长) as usize;
            项.push((源下标, w));
            和 += w;
        }
        if 和 != 0.0 {
            for (_, w) in 项.iter_mut() {
                *w /= 和;
            }
        }
        表.push(项);
    }
    表
}

/// Lanczos3 核：`sinc(t) * sinc(t/3)`，`|t| >= 3` 时为 0。
fn 核(t: f64) -> f64 {
    let a = t.abs();
    if a < 1e-12 {
        return 1.0;
    }
    if a >= 3.0 {
        return 0.0;
    }
    let x = std::f64::consts::PI * t;
    (x.sin() / x) * ((x / 3.0).sin() / (x / 3.0))
}

/// 把可能越界的源下标夹到 `0..len-1`（边缘像素重复，与常见重采样库同口径）。
#[inline]
fn 夹格(i: i64, len: u32) -> u32 {
    i.clamp(0, i64::from(len) - 1) as u32
}

/// 两个区间的重叠长度（无重叠 ⇒ 0）。
#[inline]
fn 重叠(a0: f64, a1: f64, b0: f64, b1: f64) -> f64 {
    (a1.min(b1) - a0.max(b0)).max(0.0)
}

/// 直通 alpha 的 RGBA8 → 预乘 f64（颜色 = `c*a/255`，alpha 保持 `0..=255`）。
fn 预乘(src: &[u8], sw: u32, sh: u32) -> Vec<[f64; 4]> {
    let n = sw as usize * sh as usize;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let p = &src[i * 4..i * 4 + 4];
        let a = f64::from(p[3]);
        let 系数 = a / 255.0;
        out.push([
            f64::from(p[0]) * 系数,
            f64::from(p[1]) * 系数,
            f64::from(p[2]) * 系数,
            a,
        ]);
    }
    out
}

/// 预乘 f64 → 直通 alpha 的 RGBA8（四舍五入；`alpha == 0` ⇒ 全透明黑）。
fn 还原(p: [f64; 4]) -> [u8; 4] {
    let a = p[3].clamp(0.0, 255.0);
    // +0.5 后向下取整 = 四舍五入；`as u8` 在 Rust 里是饱和转换，不会回绕
    let a8 = (a + 0.5) as u8;
    if a8 == 0 {
        return [0, 0, 0, 0];
    }
    let 除 = |v: f64| -> u8 { ((v * 255.0 / a).clamp(0.0, 255.0) + 0.5) as u8 };
    [除(p[0]), 除(p[1]), 除(p[2]), a8]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一张 `w×h` 的纯色图（alpha 由参数给）。
    fn 纯色(w: u32, h: u32, c: [u8; 4]) -> Vec<u8> {
        let mut v = Vec::with_capacity((w * h) as usize * 4);
        for _ in 0..(w * h) {
            v.extend_from_slice(&c);
        }
        v
    }

    #[test]
    fn 纯色缩放后仍是同一颜色() {
        // 纯色图在任何滤镜下都必须还原成同一个 RGBA（否则说明权重没归一化 / 预乘漏了）
        let src = 纯色(8, 6, [10, 200, 30, 255]);
        for f in [FilterClass::Nearest, FilterClass::Box, FilterClass::Lanczos] {
            let out = 缩放(&src, 8, 6, 3, 2, f);
            for px in out.as_chunks::<4>().0 {
                assert_eq!(px, &[10, 200, 30, 255], "滤镜 {f:?} 把纯色缩成了别的颜色");
            }
        }
    }

    #[test]
    fn 最近邻不产生新颜色() {
        // 2×2 四色图放大 4 倍：最近邻只许出现这四种颜色（Box/Lanczos 会插值出新色）
        let src = vec![
            255, 0, 0, 255, // (0,0) 红
            0, 255, 0, 255, // (1,0) 绿
            0, 0, 255, 255, // (0,1) 蓝
            255, 255, 0, 255, // (1,1) 黄
        ];
        let out = 缩放(&src, 2, 2, 4, 4, FilterClass::Nearest);
        let 允许: [[u8; 4]; 4] = [
            [255, 0, 0, 255],
            [0, 255, 0, 255],
            [0, 0, 255, 255],
            [255, 255, 0, 255],
        ];
        for px in out.as_chunks::<4>().0 {
            assert!(允许.contains(px), "最近邻出现了插值色：{px:?}");
        }
        // 左上角 2×2 必须全是红（区间中心映射，不是偏半格）
        assert_eq!(&out[0..4], &[255, 0, 0, 255]);
        assert_eq!(&out[4..8], &[255, 0, 0, 255]);
        assert_eq!(&out[16..20], &[255, 0, 0, 255]);
    }

    #[test]
    fn 透明像素不把颜色带黑() {
        // 左半不透明白、右半全透明（RGB 故意留黑）：预乘滤波后边缘不得变灰
        let mut src = vec![0u8; 16];
        for x in 0..2 {
            let i = x * 4;
            src[i..i + 4].copy_from_slice(&[255, 255, 255, 255]);
        }
        // 右半 alpha=0、RGB=0（已经是 0）
        let out = 缩放(&src, 4, 1, 2, 1, FilterClass::Box);
        // 左目标像素完全落在不透明区 ⇒ 纯白；右目标像素完全落在透明区 ⇒ 全透明
        assert_eq!(&out[0..4], &[255, 255, 255, 255]);
        assert_eq!(&out[4..8], &[0, 0, 0, 0]);
    }

    #[test]
    fn 全透明图缩放后仍全透明() {
        let src = vec![0u8; 8 * 8 * 4];
        for f in [FilterClass::Nearest, FilterClass::Box, FilterClass::Lanczos] {
            let out = 缩放(&src, 8, 8, 3, 3, f);
            assert!(out.iter().all(|b| *b == 0), "滤镜 {f:?} 把全透明图缩出了像素");
        }
    }

    #[test]
    fn 缩放确定性_同输入两次字节相等() {
        let mut src = Vec::new();
        for i in 0..(12 * 9) {
            src.extend_from_slice(&[
                (i * 7 % 256) as u8,
                (i * 13 % 256) as u8,
                (i * 29 % 256) as u8,
                if i % 5 == 0 { 0 } else { 255 },
            ]);
        }
        for f in [FilterClass::Nearest, FilterClass::Box, FilterClass::Lanczos] {
            let a = 缩放(&src, 12, 9, 5, 4, f);
            let b = 缩放(&src, 12, 9, 5, 4, f);
            assert_eq!(a, b, "滤镜 {f:?} 不确定");
        }
    }

    /// 造一张带透明度的伪随机图（`alpha` 覆盖 0 / 半透 / 全透，逼出预乘→除回的取整分歧）。
    fn 带透明随机图(sw: u32, sh: u32, 种子: u32) -> Vec<u8> {
        let mut s = 种子 | 1;
        let mut 下一个 = move || {
            // xorshift32：确定性、不引 rand
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            (s >> 16) as u8
        };
        let mut v = Vec::with_capacity((sw * sh) as usize * 4);
        for i in 0..(sw * sh) {
            let a = match i % 4 {
                0 => 0,
                1 => 128,
                2 => 255,
                _ => 下一个(),
            };
            v.extend_from_slice(&[下一个(), 下一个(), 下一个(), a]);
        }
        v
    }

    #[test]
    fn 流式盒式与整幅盒式逐字节相同() {
        // 覆盖：整数倍缩小（2:1 / 4:1）、非整数倍缩小（3:2 / 5:4 / 7:3）、1:1、放大（2:1 / 3:2），
        // 以及真实视口的一行宽度（3072）。**逐字节相等**，不是「容差内相等」。
        let 尺寸表: [(u32, u32, u32, u32); 10] = [
            (8, 6, 4, 3),
            (8, 6, 2, 1),
            (7, 5, 3, 2),
            (16, 12, 5, 4),
            (4, 4, 4, 4),
            (4, 3, 8, 6),
            (5, 5, 7, 3),
            (96, 64, 48, 32),
            (96, 64, 24, 16),
            (3072, 8, 1536, 4),
        ];
        for (i, (sw, sh, dw, dh)) in 尺寸表.into_iter().enumerate() {
            let src = 带透明随机图(sw, sh, 0x1234_5678 + i as u32 * 7919);
            let 整幅 = 盒式(&src, sw, sh, dw, dh);
            let 流式 = 盒式_流式(&src, sw, sh, dw, dh);
            assert_eq!(
                整幅.len(),
                流式.len(),
                "{sw}×{sh} → {dw}×{dh}：长度不一致"
            );
            assert_eq!(
                整幅, 流式,
                "{sw}×{sh} → {dw}×{dh}：流式盒式必须与整幅盒式**逐字节相同**"
            );
        }
    }

    #[test]
    fn 流式盒式与整幅盒式在纯色与全透明上同样一致() {
        // 纯色（预乘/还原必须原样还回来）+ 全透明（alpha=0 一律吐 [0,0,0,0]）
        for (src, sw, sh) in [
            (纯色(12, 9, [10, 200, 30, 255]), 12u32, 9u32),
            (纯色(12, 9, [7, 8, 9, 128]), 12, 9),
            (vec![0u8; 12 * 9 * 4], 12, 9),
        ] {
            for (dw, dh) in [(6u32, 4u32), (3, 2), (5, 4), (12, 9), (24, 18)] {
                assert_eq!(
                    盒式(&src, sw, sh, dw, dh),
                    盒式_流式(&src, sw, sh, dw, dh),
                    "{sw}×{sh} → {dw}×{dh}：纯色/全透明下两条路径必须一致"
                );
            }
        }
    }

    #[test]
    fn 流式盒式在非整除比例下同样逐字节等价() {
        // 非整除缩小（3:2 / 5:4 / 7:3）时一个目标像素会横跨 2~3 个源像素、目标行也会跨源行，
        // 权重不再全是 1 ⇒ 最容易暴露「顺序/权重算错」。这里把边界尺寸也过一遍。
        for (sw, sh, dw, dh) in [
            (9u32, 7u32, 6u32, 5u32),
            (11, 13, 5, 6),
            (23, 17, 8, 7),
            (2, 3, 1, 2),
            (3, 2, 2, 1),
        ] {
            let src = 带透明随机图(sw, sh, 0x0BAD_F00D ^ (sw << 8) ^ sh);
            assert_eq!(
                盒式(&src, sw, sh, dw, dh),
                盒式_流式(&src, sw, sh, dw, dh),
                "{sw}×{sh} → {dw}×{dh}：非整除比例下两条路径必须一致"
            );
        }
    }
}
