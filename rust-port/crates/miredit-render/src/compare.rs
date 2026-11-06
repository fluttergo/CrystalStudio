//! 结构级分块比对（验收门 4）：两份图 → 缩到同尺寸 → 切 16×16 块 → 比平均色。
//!
//! 口径（**不是像素级**，这是故意的）：
//! 1. 两图先缩放到公共尺寸 `min(宽) × min(高)`（面积平均降采样，确定性）；
//! 2. 切成 `block × block`（默认 16×16）的块；
//! 3. 每块算 RGB 平均色，`Δ = sqrt(dr² + dg² + db²)`；
//! 4. `Δ < threshold`（默认 32）记一次命中；命中块数 / 总块数 = 命中率。
//!
//! 为什么不做逐像素：客户端截图是**整屏 1280×720 的实时渲染**（相机取整、图集过滤、
//! 精灵半像素对齐都会引入偏差），逐像素比只会得到「全不匹配」这种无信息结论。
//! 分块平均色能把「地形结构对不对」与「有没有 1px 接缝」分开。
//!
//! 透明像素的处理：比对前会把两图都**压到同一个底色**（[`CompareOptions::background`]），
//! 否则「本库的透明黑」与「客户端清屏灰」会被当成真实色差。

use crate::png_io::PngImage;

/// 比对参数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompareOptions {
    /// 分块边长（像素）。
    pub block: u32,
    /// 命中阈值（RGB 欧氏距离，0..=441）。
    pub threshold: u32,
    /// 压平透明像素用的底色（RGB）。
    pub background: [u8; 3],
}

impl Default for CompareOptions {
    fn default() -> Self {
        CompareOptions {
            block: 16,
            threshold: 32,
            background: [0, 0, 0],
        }
    }
}

/// 一次分块比对的结果。
#[derive(Debug, Clone, PartialEq)]
pub struct BlockReport {
    /// 比对所用公共宽。
    pub width: u32,
    /// 比对所用公共高。
    pub height: u32,
    /// 横向块数。
    pub blocks_x: u32,
    /// 纵向块数。
    pub blocks_y: u32,
    /// 总块数。
    pub total: u32,
    /// 命中块数（`Δ < threshold`）。
    pub hit: u32,
    /// 命中率（0.0 ~ 1.0）。
    pub hit_rate: f64,
    /// 最大块色差。
    pub max_delta: u32,
    /// 平均块色差。
    pub mean_delta: f64,
    /// 块色差中位数（便于判断「整体偏一点」还是「局部完全不对」）。
    pub median_delta: u32,
}

impl BlockReport {
    /// 是否达到给定命中率门槛（例如 `0.70`）。
    pub fn at_least(&self, min_rate: f64) -> bool {
        self.hit_rate >= min_rate
    }
}

impl core::fmt::Display for BlockReport {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "分块比对 {}×{} px → {}×{} 块：命中 {}/{} = {:.2}%；块色差 平均 {:.1} / 中位 {} / 最大 {}",
            self.width,
            self.height,
            self.blocks_x,
            self.blocks_y,
            self.hit,
            self.total,
            self.hit_rate * 100.0,
            self.mean_delta,
            self.median_delta,
            self.max_delta
        )
    }
}

/// 主入口：把 `a`（通常是本库渲染结果）与 `b`（通常是客户端截图）按块比较。
pub fn compare_blocks(a: &PngImage, b: &PngImage, opts: &CompareOptions) -> BlockReport {
    let w = a.width.min(b.width);
    let h = a.height.min(b.height);
    let a2 = flatten_over(&resize_to(a, w, h), opts.background);
    let b2 = flatten_over(&resize_to(b, w, h), opts.background);

    let block = opts.block.max(1);
    let blocks_x = w.div_ceil(block);
    let blocks_y = h.div_ceil(block);
    let mut deltas: Vec<u32> = Vec::with_capacity((blocks_x * blocks_y) as usize);

    for by in 0..blocks_y {
        for bx in 0..blocks_x {
            let x0 = bx * block;
            let y0 = by * block;
            let x1 = (x0 + block).min(w);
            let y1 = (y0 + block).min(h);
            let ca = 块平均色(&a2, x0, y0, x1, y1);
            let cb = 块平均色(&b2, x0, y0, x1, y1);
            let dr = ca[0] as f64 - cb[0] as f64;
            let dg = ca[1] as f64 - cb[1] as f64;
            let db = ca[2] as f64 - cb[2] as f64;
            let d = (dr * dr + dg * dg + db * db).sqrt().round() as u32;
            deltas.push(d);
        }
    }

    let total = deltas.len() as u32;
    let hit = deltas.iter().filter(|d| **d < opts.threshold).count() as u32;
    let max_delta = deltas.iter().copied().max().unwrap_or(0);
    let sum: u64 = deltas.iter().map(|d| *d as u64).sum();
    let mean_delta = if total == 0 {
        0.0
    } else {
        sum as f64 / total as f64
    };
    let mut sorted = deltas.clone();
    sorted.sort_unstable();
    let median_delta = if sorted.is_empty() {
        0
    } else {
        sorted[sorted.len() / 2]
    };

    BlockReport {
        width: w,
        height: h,
        blocks_x,
        blocks_y,
        total,
        hit,
        hit_rate: if total == 0 {
            0.0
        } else {
            hit as f64 / total as f64
        },
        max_delta,
        mean_delta,
        median_delta,
    }
}

/// 面积平均缩放（确定性；`dw/dh` 大于源尺寸时退化为最近邻取整，不放大插值）。
pub fn resize_to(img: &PngImage, dw: u32, dh: u32) -> PngImage {
    if img.width == dw && img.height == dh {
        return img.clone();
    }
    let mut out = vec![0u8; dw as usize * dh as usize * 4];
    for dy in 0..dh {
        // 目标像素覆盖的源行区间
        let sy0 = (dy as u64 * img.height as u64 / dh as u64) as u32;
        let sy1 = (((dy as u64 + 1) * img.height as u64 / dh as u64) as u32).max(sy0 + 1);
        let sy1 = sy1.min(img.height);
        for dx in 0..dw {
            let sx0 = (dx as u64 * img.width as u64 / dw as u64) as u32;
            let sx1 = (((dx as u64 + 1) * img.width as u64 / dw as u64) as u32).max(sx0 + 1);
            let sx1 = sx1.min(img.width);
            let mut acc = [0u64; 4];
            let mut n = 0u64;
            for sy in sy0..sy1 {
                for sx in sx0..sx1 {
                    let i = (sy as usize * img.width as usize + sx as usize) * 4;
                    // 逐通道累加（写法改成 iter_mut().enumerate() 只为消 clippy 的
                    // needless_range_loop，累加语义与原 `for c in 0..4 { acc[c] += … }` 完全一致）
                    for (c, 槽) in acc.iter_mut().enumerate() {
                        *槽 += img.rgba[i + c] as u64;
                    }
                    n += 1;
                }
            }
            let di = (dy as usize * dw as usize + dx as usize) * 4;
            if n == 0 {
                continue;
            }
            for c in 0..4 {
                out[di + c] = ((acc[c] + n / 2) / n) as u8;
            }
        }
    }
    PngImage {
        width: dw,
        height: dh,
        rgba: out,
    }
}

/// 把 alpha 压到底色上（`a == 255` 的像素原样保留）。
pub fn flatten_over(img: &PngImage, bg: [u8; 3]) -> PngImage {
    let mut out = img.rgba.clone();
    // 按 4 字节（1 像素）分块；`as_chunks_mut::<4>()` 与 `chunks_exact_mut(4)` 等价
    // （改用前者只为消 clippy 1.98 的 chunks_exact_to_as_chunks，像素语义不变）
    let (像素们, _余) = out.as_chunks_mut::<4>();
    for px in 像素们 {
        let a = px[3] as u32;
        if a == 255 {
            continue;
        }
        for c in 0..3 {
            let v = px[c] as u32 * a + bg[c] as u32 * (255 - a);
            px[c] = ((v + 127) / 255) as u8;
        }
        px[3] = 255;
    }
    PngImage {
        width: img.width,
        height: img.height,
        rgba: out,
    }
}

/// 一块区域的平均色（RGB；块内不含像素时返回全黑）。
fn 块平均色(img: &PngImage, x0: u32, y0: u32, x1: u32, y1: u32) -> [u8; 3] {
    let mut acc = [0u64; 3];
    let mut n = 0u64;
    for y in y0..y1 {
        for x in x0..x1 {
            let i = (y as usize * img.width as usize + x as usize) * 4;
            // 同 resize_to：累加语义不变，只为消 needless_range_loop
            for (c, 槽) in acc.iter_mut().enumerate() {
                *槽 += img.rgba[i + c] as u64;
            }
            n += 1;
        }
    }
    if n == 0 {
        return [0, 0, 0];
    }
    [
        ((acc[0] + n / 2) / n) as u8,
        ((acc[1] + n / 2) / n) as u8,
        ((acc[2] + n / 2) / n) as u8,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn 纯色(w: u32, h: u32, c: [u8; 4]) -> PngImage {
        let mut rgba = Vec::with_capacity((w * h) as usize * 4);
        for _ in 0..(w * h) {
            rgba.extend_from_slice(&c);
        }
        PngImage {
            width: w,
            height: h,
            rgba,
        }
    }

    #[test]
    fn 完全相同则全命中() {
        let a = 纯色(64, 32, [10, 20, 30, 255]);
        let r = compare_blocks(&a, &a, &CompareOptions::default());
        assert_eq!(r.total, r.hit);
        assert_eq!(r.max_delta, 0);
        assert!(r.at_least(1.0));
    }

    #[test]
    fn 色差超阈值则零命中() {
        let a = 纯色(64, 32, [0, 0, 0, 255]);
        let b = 纯色(64, 32, [100, 100, 100, 255]);
        let r = compare_blocks(&a, &b, &CompareOptions::default());
        assert_eq!(r.hit, 0);
        assert!(!r.at_least(0.7));
    }

    #[test]
    fn 缩放与透明压平() {
        let a = 纯色(40, 40, [200, 100, 50, 255]);
        let b = resize_to(&a, 20, 20);
        assert_eq!((b.width, b.height), (20, 20));
        assert_eq!(&b.rgba[0..4], &[200, 100, 50, 255], "纯色缩放应保持颜色");

        let t = 纯色(2, 2, [255, 0, 0, 0]); // 全透明
        let f = flatten_over(&t, [9, 9, 9]);
        assert_eq!(&f.rgba[0..4], &[9, 9, 9, 255], "全透明应完全取底色");
    }
}
