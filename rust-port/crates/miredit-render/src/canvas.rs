//! RGBA8 画布与「源覆盖（over）」合成。
//!
//! # 为什么要自己写混合
//!
//! 图库帧是带透明的（调色板索引 0 = 全透明，见 `wil2atlas/src/indexed.rs:38`
//! 「索引 0 约定为全透明」），而客户端是「逐格贴精灵」：后贴的盖住先贴的，
//! 透明像素不遮不挡。这与 `Sprite2D` / `draw_texture` 的默认 alpha 混合同语义。
//!
//! 本模块只做两件事：**整型 over 混合**（无浮点、无 SIMD、结果可复现）与**裁剪贴图**。
//! 不含任何游戏语义 —— 落格规则在 `compose.rs`，这里只认「像素坐标」。

use mir2_assets::Frame;

/// 行优先 RGBA8 画布，初值全透明黑 `(0,0,0,0)`。
pub(crate) struct Canvas {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl Canvas {
    /// 建一块全透明画布。
    pub(crate) fn new(width: u32, height: u32) -> Self {
        Canvas {
            width,
            height,
            rgba: vec![0u8; width as usize * height as usize * 4],
        }
    }

    /// 把一帧按**左上角**贴在 `(x, y)`（可为负；超出画布的部分裁掉，不报错）。
    ///
    /// 返回 `true` 表示**真的往画布写了至少 1 个像素**。以下情形一律返回 `false`：
    /// 帧宽或帧高为 0（`空帧`）、帧与画布无交集（整帧落在画布外）、
    /// 帧落在画布内的那块像素**全透明**（`索引 0` 全透明，写不进去任何东西）。
    ///
    /// 调用方（`compose`）据此判定「这一格算不算落笔」—— [`crate::RenderStats::cells_drawn`]
    /// 的口径就是「实际产生了绘制」，不是「扫过了这一格」。
    pub(crate) fn blit(&mut self, frame: &Frame, x: i32, y: i32) -> bool {
        if frame.width == 0 || frame.height == 0 {
            return false;
        }
        let fw = frame.width as i64;
        let fh = frame.height as i64;
        let dst_w = self.width as i64;
        let dst_h = self.height as i64;

        // 与画布求交（全程 i64，避免负坐标/加法溢出）。
        let x0 = (x as i64).max(0);
        let y0 = (y as i64).max(0);
        let x1 = (x as i64 + fw).min(dst_w);
        let y1 = (y as i64 + fh).min(dst_h);
        if x0 >= x1 || y0 >= y1 {
            return false;
        }

        let src_stride = frame.width as usize * 4;
        let dst_stride = self.width as usize * 4;
        // 是否真的写入过像素：全透明帧会走完整个循环却一个像素都不写 ⇒ 必须回报 false，
        // 否则 cells_drawn 会把「画了张全透明的图」也算成已落笔。
        let mut 写了 = false;
        for dy in y0..y1 {
            let sy = (dy - y as i64) as usize;
            let src_row = sy * src_stride;
            let dst_row = dy as usize * dst_stride;
            for dx in x0..x1 {
                let sx = (dx - x as i64) as usize;
                let si = src_row + sx * 4;
                let di = dst_row + dx as usize * 4;
                let src = [
                    frame.rgba[si],
                    frame.rgba[si + 1],
                    frame.rgba[si + 2],
                    frame.rgba[si + 3],
                ];
                if src[3] == 0 {
                    continue; // 全透明像素：不碰目标（与精灵绘制的「不遮挡」一致）
                }
                let dst = [
                    self.rgba[di],
                    self.rgba[di + 1],
                    self.rgba[di + 2],
                    self.rgba[di + 3],
                ];
                let out = over(src, dst);
                self.rgba[di..di + 4].copy_from_slice(&out);
                写了 = true;
            }
        }
        写了
    }

    /// 用**一种纯色**填一块矩形（同样走源覆盖合成），左上角 `(x, y)`、尺寸 `w×h`。
    ///
    /// 返回 `true` 表示**真的往画布写了至少 1 个像素**。以下情形返回 `false`：
    /// `rgba[3] == 0`（写回原值等于没写，不该算落笔）、宽或高为 0、矩形与画布无交集。
    ///
    /// 用途：阻挡标记这类「整块半透明色块」（见 `collision` 模块）。**不做游戏语义**，
    /// 与 [`Canvas::blit`] 一样只认像素坐标。
    pub(crate) fn fill_rect(&mut self, rgba: [u8; 4], x: i32, y: i32, w: u32, h: u32) -> bool {
        if rgba[3] == 0 || w == 0 || h == 0 {
            return false;
        }
        let dst_w = self.width as i64;
        let dst_h = self.height as i64;
        // 与画布求交（全程 i64，避免负坐标/加法溢出）。
        let x0 = (x as i64).max(0);
        let y0 = (y as i64).max(0);
        let x1 = (x as i64 + w as i64).min(dst_w);
        let y1 = (y as i64 + h as i64).min(dst_h);
        if x0 >= x1 || y0 >= y1 {
            return false;
        }

        let dst_stride = self.width as usize * 4;
        for dy in y0..y1 {
            let dst_row = dy as usize * dst_stride;
            for dx in x0..x1 {
                let di = dst_row + dx as usize * 4;
                let dst = [
                    self.rgba[di],
                    self.rgba[di + 1],
                    self.rgba[di + 2],
                    self.rgba[di + 3],
                ];
                let out = over(rgba, dst);
                self.rgba[di..di + 4].copy_from_slice(&out);
            }
        }
        true
    }

    /// 按**逐像素 alpha 遮罩**整幅叠一层纯黑（源覆盖合成，等价于把一张「黑色 +
    /// 该 alpha」的图铺满画布）。
    ///
    /// `alpha[i]` 是第 `i` 个像素的遮罩不透明度（`0` = 不压暗、`255` = 全黑）；
    /// 长度必须等于 `width * height`。**不做游戏语义**：环境光/光罩怎么算出这张表，
    /// 是 `lighting` 模块的事，这里只认「每个像素压多少」。
    ///
    /// 三条边界：
    ///
    /// 1. `alpha[i] == 0` 的像素**一个字节都不碰**（⇒ 全 0 遮罩等价于没调用）；
    /// 2. 合成公式与 [`Canvas::blit`] 的 `over()` 完全同一条：`A = a + a_d*(1-a)`，
    ///    `C = (c_d*a_d*(1-a)) / A`（源是纯黑 ⇒ 源项为 0）。画布本来全透明的地方
    ///    结果是「黑 + alpha」，与「往空白处铺一张半透明黑」一致；
    /// 3. 长度不符时**只处理两边都有的前缀**（不 panic、不越界）—— 调用方在
    ///    `lighting` 里按 `width*height` 建表，正常不会触发。
    pub(crate) fn apply_black_mask(&mut self, alpha: &[u8]) {
        let dst_stride = self.width as usize * 4;
        let 行数 = (alpha.len() / self.width.max(1) as usize).min(self.height as usize);
        for y in 0..行数 {
            let row = y * dst_stride;
            for x in 0..self.width as usize {
                let a = alpha[y * self.width as usize + x] as u32;
                if a == 0 {
                    continue; // 不压暗：不碰目标
                }
                let di = row + x * 4;
                if a == 255 {
                    // 全黑不透明：直接覆盖（over 的 a_s == 255 快路径）
                    self.rgba[di..di + 4].copy_from_slice(&[0, 0, 0, 255]);
                    continue;
                }
                let da = self.rgba[di + 3] as u32;
                let keep = da * (255 - a) / 255;
                let out_a = a + keep;
                if out_a == 0 {
                    continue;
                }
                // 源色恒为 0 ⇒ 只剩目标项：C = c_d * keep / out_a（与 over() 同款四舍五入）
                for k in 0..3 {
                    let c = self.rgba[di + k] as u32;
                    self.rgba[di + k] = ((c * keep + out_a / 2) / out_a).min(255) as u8;
                }
                self.rgba[di + 3] = out_a.min(255) as u8;
            }
        }
    }

    /// 取内部缓冲区（移动语义，渲染结束后不再需要画布）。
    pub(crate) fn into_rgba(self) -> Vec<u8> {
        self.rgba
    }
}

/// 单像素「源覆盖」合成（非预乘 alpha，整型四舍五入）。
///
/// 公式：`A = a_s + a_d*(1-a_s)`，`C = (c_s*a_s + c_d*a_d*(1-a_s)) / A`。
/// 全部用 `u32` 整数运算；`a_s == 255` 直接取源、`a_s == 0` 直接取目标（快路径）。
#[inline]
fn over(src: [u8; 4], dst: [u8; 4]) -> [u8; 4] {
    let sa = src[3] as u32;
    if sa == 0 {
        return dst;
    }
    if sa == 255 {
        return src;
    }
    let da = dst[3] as u32;
    let inv = 255 - sa;
    // 目标 alpha 折算到「源覆盖之后剩下的部分」
    let da_keep = da * inv / 255;
    let out_a = sa + da_keep;
    if out_a == 0 {
        return [0, 0, 0, 0];
    }
    let mix = |sc: u8, dc: u8| -> u8 {
        let num = sc as u32 * sa + dc as u32 * da_keep;
        ((num + out_a / 2) / out_a).min(255) as u8
    };
    [
        mix(src[0], dst[0]),
        mix(src[1], dst[1]),
        mix(src[2], dst[2]),
        out_a.min(255) as u8,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn 帧(w: u32, h: u32, rgba: Vec<u8>) -> Frame {
        Frame {
            width: w,
            height: h,
            rgba,
            ..mir2_assets::Frame::default()
        }
    }

    #[test]
    fn 全透明像素不遮挡() {
        let mut c = Canvas::new(2, 2);
        // 先铺一块不透明红
        assert!(c.blit(&帧(2, 2, [255, 0, 0, 255].repeat(4)), 0, 0));
        // 再铺一层全透明：返回 false（一个像素都没写），像素也不被改
        assert!(
            !c.blit(&帧(2, 2, [0, 0, 0, 0].repeat(4)), 0, 0),
            "全透明帧没有写入任何像素，必须返回 false"
        );
        let px = c.into_rgba();
        assert_eq!(&px[0..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn 负坐标与越界裁剪() {
        let mut c = Canvas::new(3, 3);
        // 2×2 全白，贴在 (-1,-1) ⇒ 只有 (0,0) 落在画布内
        let rgba: Vec<u8> = [9u8, 8, 7, 255].repeat(4);
        assert!(c.blit(&帧(2, 2, rgba.clone()), -1, -1));
        let px = c.into_rgba();
        assert_eq!(&px[0..4], &[9, 8, 7, 255], "(0,0) 应被写到");
        assert_eq!(&px[4..8], &[0, 0, 0, 0], "(1,0) 在帧外，应保持透明");
        assert_eq!(&px[12..16], &[0, 0, 0, 0], "(0,1) 在帧外，应保持透明");
        // 完全在画布外 ⇒ 返回 false 且不改像素
        let mut c2 = Canvas::new(2, 2);
        assert!(!c2.blit(&帧(4, 4, rgba), 10, 10));
        assert!(c2.into_rgba().iter().all(|b| *b == 0));
    }

    #[test]
    fn 半透明混合公式() {
        // 红底 + 50% 蓝 ⇒ alpha 255，色 = (127,0,128) 量级
        let out = over([0, 0, 255, 128], [255, 0, 0, 255]);
        assert_eq!(out[3], 255);
        assert!(out[0] > 120 && out[0] < 135, "红通道 {:?}", out);
        assert_eq!(out[1], 0);
        assert!(out[2] > 120 && out[2] < 135, "蓝通道 {:?}", out);
    }

    #[test]
    fn 纯色矩形填空与裁剪() {
        let mut c = Canvas::new(4, 2);
        // 半透明色块铺在**全透明底色**上 ⇒ 结果就是「该色 + 该 alpha」（over 的 a_s<255 分支）
        assert!(c.fill_rect([240, 85, 105, 90], 1, 0, 2, 1));
        let px = c.into_rgba();
        // (0,0) 在矩形外，保持全透明
        assert_eq!(&px[0..4], &[0, 0, 0, 0]);
        // (1,0)/(2,0) 被填：底色 alpha=0 ⇒ 输出 alpha 就等于源 alpha，颜色原样
        assert_eq!(&px[4..8], &[240, 85, 105, 90]);
        assert_eq!(&px[8..12], &[240, 85, 105, 90]);
        // (3,0) 在矩形外
        assert_eq!(&px[12..16], &[0, 0, 0, 0]);
        // 第二行整行没被碰
        assert!(px[16..].iter().all(|b| *b == 0));

        // alpha=0 ⇒ 一个像素都不写，必须返回 false（否则计数会把「没画」算成画了）
        let mut c2 = Canvas::new(2, 2);
        assert!(!c2.fill_rect([1, 2, 3, 0], 0, 0, 2, 2));
        assert!(c2.into_rgba().iter().all(|b| *b == 0));
        // 空矩形 / 完全在画布外 ⇒ 同样 false
        let mut c3 = Canvas::new(2, 2);
        assert!(!c3.fill_rect([1, 2, 3, 255], 0, 0, 0, 2));
        assert!(!c3.fill_rect([1, 2, 3, 255], 9, 9, 4, 4));
    }
}
