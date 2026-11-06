//! RGBA8 画布与「源覆盖（over）」合成。
//!
//! # 为什么要自己写混合
//!
//! 图库帧是带透明的（调色板索引 0 = 全透明，见 `wil2atlas/src/indexed.rs:38`
//! 「索引 0 约定为全透明」），而客户端是「逐格贴精灵」：后贴的盖住先贴的，
//! 透明像素不遮不挡。这与 `Sprite2D` / `draw_texture` 的默认 alpha 混合同语义。
//!
//! 本模块只管三件事：**整型 over 混合**（无浮点、无 SIMD、结果可复现）、**前景加性
//! （additive）混合**（`FrontBlend`，见 `CrystalStudio/MapSpriteDrawing.cs:5,68-96@6450da90`）、
//! 以及把帧按像素坐标**裁剪**贴进画布。不含任何游戏语义 —— 落格规则在 `compose.rs`，
//! 这里只认「像素坐标」。

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
        // 全透明像素（`索引 0`）：不碰目标（与精灵绘制的「不遮挡」一致）⇒ 闭包返回 `None`。
        self.逐像素贴(frame, x, y, |src, dst| {
            if src[3] == 0 {
                None
            } else {
                Some(over(src, dst))
            }
        })
    }

    /// 把一帧按**加性混合**（additive）贴在 `(x, y)`：裁剪 / 空帧 / 越界 / `sa == 0`
    /// 这些边界与 [`Canvas::blit`] **完全同一条**（两者共用 `逐像素贴` 骨架），
    /// **只有「单像素怎么算」这一步不同**（`additive`）。
    ///
    /// # 出处（上游语义）
    ///
    /// 上游 `CrystalStudio/MapSpriteDrawing.cs:5@6450da90`：
    /// `FrontBlend(frames) => (frames & 0x80) != 0` 选中这条混合；`:68-96` 的
    /// `Composite` 在 **Pbgra32（预乘）** 空间逐像素做：
    ///
    /// ```text
    /// :91  rgb 逐通道  min(255, dst + src)                  // 预乘空间直接相加
    /// :92  a   = max(旧 dst.a, max(加后 rgb 三通道))         // alpha 不参与加法
    /// ```
    ///
    /// 结果**保持预乘**（`RasterFormats.Pbgra32`），展回 straight 是 Skia 落地时的事。
    ///
    /// # 本画布是 straight ⇒ 三步折算
    ///
    /// [`Canvas`] 存的是 **straight（非预乘）RGBA8**（[`Canvas::blit`] 的 `over` 也是
    /// straight），所以这里必须自己「预乘 → 相加 → 反预乘」：
    ///
    /// ```text
    /// p      = (c * a + 127) / 255                    // 预乘（四舍五入）
    /// r/g/b  = min(255, p(dst) + p(src))              // 与 :91 同一条
    /// A      = max(dst.a, max(r, g, b))               // 与 :92 同一条（读**加后**的 RGB）
    /// out.rgb= clamp((p * 255 + A/2) / A, 0, 255)     // 反预乘（四舍五入）
    /// out.a  = A
    /// ```
    ///
    /// ⚠ **预乘 / 反预乘的舍入口径是本仓自定的（待回源）**：上游把反预乘交给 Skia 的
    /// `Pbgra32 → 屏幕` 转换，本仓**没有取到** Skia/Avalonia 那一步的确切舍入方式
    /// ⇒ 这里取「四舍五入 + 夹 255」这套与本模块 `over` 一致的整型口径。
    /// **有据的部分**是「相加」「取 max」「alpha 不参与加法」三步（`:91-92` 逐字）；
    /// **不确定的只有反预乘那一次舍入**（是否 round-half-up、是否用 `(p*255 + A/2)/A`
    /// 这种近似）。**列待回源**：Skia 的 Pbgra32 反预乘舍入。
    ///
    /// 不透明底（`dst.a == 255`）走快路径：此时预乘与反预乘都是恒等变换 ⇒
    /// 直接 `min(255, dst.rgb + 预乘(src.rgb))`、`A = 255`，**一次舍入都不引入**
    /// （这一条是**精确等价**，不是近似）。
    ///
    /// 返回 `true` 的口径与 [`Canvas::blit`] 逐条相同：**真的往画布写了至少 1 个像素**。
    /// `sa == 0` 的像素一个字节都不碰（也不计落笔）—— 加性混合下它的贡献恒为 0。
    pub(crate) fn blit_additive(&mut self, frame: &Frame, x: i32, y: i32) -> bool {
        self.逐像素贴(frame, x, y, |src, dst| {
            if src[3] == 0 {
                None
            } else {
                Some(additive(src, dst))
            }
        })
    }

    /// 逐像素合成的公共骨架：**求交裁剪 + 循环 + 「到底写了没」**。
    ///
    /// `逐像素(src, dst)` 返回 `Some(out)` ⇒ 把该像素写回 `out`（**不比较新旧值**：
    /// 只要执行了写入就算「写了」，与本模块既有口径一致）；返回 `None` ⇒ 该像素
    /// **一个字节都不碰**，也不算写。
    ///
    /// [`Canvas::blit`]（`over`）与 [`Canvas::blit_additive`]（`additive`）**只有这一步
    /// 的闭包不同** ⇒ 三条边界（空帧、与画布无交集、整片全透明）只有一份实现，不会两边漂。
    fn 逐像素贴(
        &mut self,
        frame: &Frame,
        x: i32,
        y: i32,
        mut 逐像素: impl FnMut([u8; 4], [u8; 4]) -> Option<[u8; 4]>,
    ) -> bool {
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
                let dst = [
                    self.rgba[di],
                    self.rgba[di + 1],
                    self.rgba[di + 2],
                    self.rgba[di + 3],
                ];
                let Some(out) = 逐像素(src, dst) else {
                    continue;
                };
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

/// 单像素「前景加性混合」（straight RGBA8 画布上的整型实现）。
///
/// 上游 `CrystalStudio/MapSpriteDrawing.cs:68-96@6450da90` 在 **Pbgra32（预乘）**
/// 空间做 `rgb = min(255, dst+src)`、`a = max(旧 dst.a, max(加后 rgb))`，并保持预乘。
/// 本画布是 straight ⇒ 这里**先预乘、再相加、再反预乘**（公式与「本仓自定舍入」的
/// 说明见 [`Canvas::blit_additive`] 的文档）。
///
/// 调用方（`blit_additive`）已保证 `src[3] != 0`（`sa == 0` 的像素在加性混合下贡献
/// 恒为 0 ⇒ 根本不进来）。
#[inline]
fn additive(src: [u8; 4], dst: [u8; 4]) -> [u8; 4] {
    let sa = u32::from(src[3]);
    let da = u32::from(dst[3]);
    // 预乘：`p = (c*a + 127)/255`（四舍五入）。`a == 255` 时 `p == c`（恒等）。
    let 预乘 = |c: u8, a: u32| (u32::from(c) * a + 127) / 255;
    let (sr, sg, sb) = (预乘(src[0], sa), 预乘(src[1], sa), 预乘(src[2], sa));

    // 不透明底快路径：`da == 255` ⇒ dst 的预乘值就是原值、`A` 必为 255、
    // 反预乘是恒等 ⇒ 直接相加，不引入任何舍入（精确等价，不是近似）。
    if da == 255 {
        let 加 = |d: u8, s: u32| (u32::from(d) + s).min(255) as u8;
        return [
            加(dst[0], sr),
            加(dst[1], sg),
            加(dst[2], sb),
            255,
        ];
    }

    let (dr, dg, db) = (预乘(dst[0], da), 预乘(dst[1], da), 预乘(dst[2], da));
    // 逐通道相加并夹到 255（`:91`）
    let r = (dr + sr).min(255);
    let g = (dg + sg).min(255);
    let b = (db + sb).min(255);
    // alpha 不参与加法：取「旧 dst.a」与「加后 RGB 最大值」中的大者（`:92`）
    let out_a = da.max(r).max(g).max(b);
    if out_a == 0 {
        // 只有「底色全透明 + 源 RGB 也是 0」才会到这里：加性结果确实是全透明黑。
        return [0, 0, 0, 0];
    }
    // 反预乘：`out = (p*255 + A/2)/A`，夹 255（本仓自定舍入口径，见方法文档）
    let 反预乘 = |p: u32| ((p * 255 + out_a / 2) / out_a).min(255) as u8;
    [
        反预乘(r),
        反预乘(g),
        反预乘(b),
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

    // -----------------------------------------------------------------------
    // 前景加性混合（`MapSpriteDrawing.cs:5,68-96@6450da90`）
    //
    // 下面用 `1×1` 画布：底色**直接种进缓冲区**（绕开 `over` 自己的舍入），
    // 保证「dst 就是这个值」这个前提精确，再把一帧同样 1×1 的源贴上去。
    // -----------------------------------------------------------------------

    /// 1×1 画布：先把底色种成 `dst`，再用 `src` 走 [`Canvas::blit_additive`]。
    fn 加性单像素(dst: [u8; 4], src: [u8; 4]) -> [u8; 4] {
        let mut c = Canvas::new(1, 1);
        c.rgba.copy_from_slice(&dst);
        assert!(
            c.blit_additive(&帧(1, 1, src.to_vec()), 0, 0),
            "`sa != 0` 的加性像素必须真正写入并回报 true"
        );
        let px = c.into_rgba();
        [px[0], px[1], px[2], px[3]]
    }

    /// **向量 1**：不透明底 + 不透明源 ⇒ 逐通道 `min(255, dst+src)`，alpha 恒 255。
    /// 同一组参数走 `over()` 得到的是「覆盖」（源不透明 ⇒ 直接取源），两者必须不同。
    #[test]
    fn 加性_向量1_不透明底逐通道加法() {
        let dst = [50, 100, 150, 255];
        let src = [200, 200, 200, 255];
        assert_eq!(
            加性单像素(dst, src),
            [250, 255, 255, 255],
            "r 250（50+200）/ g 255（100+200 夹到 255）/ b 255（150+200 夹到 255）"
        );
        // 对照：`over` 在同参数下是「源覆盖」——`sa == 255` 直接取源
        assert_eq!(
            over(src, dst),
            [200, 200, 200, 255],
            "over 是覆盖不是加法（sa=255 快路径直接取源）"
        );
    }

    /// **向量 2**：全透明底 + 半透明源 ⇒ 预乘相加后反预乘。
    ///
    /// 整数公式逐步核（全程 `u32`）：
    /// `sr=(200*100+127)/255=78`、`sg=(150*100+127)/255=59`、`sb=(100*100+127)/255=39`；
    /// `oa=max(0, max(78,59,39))=78`；
    /// 反预乘 `(78*255+39)/78=255`、`(59*255+39)/78=193`、`(39*255+39)/78=128`。
    #[test]
    fn 加性_向量2_透明底预乘加后反预乘() {
        assert_eq!(
            加性单像素([0, 0, 0, 0], [200, 150, 100, 100]),
            [255, 193, 128, 78],
            "预乘 (78,59,39) + oa=max(0,78)=78 ⇒ 反预乘 (255,193,128)"
        );
    }

    /// **向量 3**：高亮夹到 255；再补一个 `src.a == 128` 的变体（走完整预乘/反预乘路径）。
    #[test]
    fn 加性_向量3_高亮夹断与半透明源变体() {
        // ① 不透明底 + 接近满值的源 ⇒ 三通道全部夹在 255
        assert_eq!(
            加性单像素([250, 250, 250, 255], [250, 250, 250, 255]),
            [255, 255, 255, 255]
        );
        // ② 全透明底 + 灰 (200) / a=128：sr=(200*128+127)/255=100 ⇒ oa=100
        //    反预乘 (100*255+50)/100 = 255（255.5 截成 255，不溢出 u8）
        assert_eq!(
            加性单像素([0, 0, 0, 0], [200, 200, 200, 128]),
            [255, 255, 255, 100],
            "预乘 100 / oa 100 ⇒ 反预乘 255（夹断在 255）"
        );
        // ③ 半透明底（a=128）+ 半透明源：走完整三步，且 `A = max(旧 dst.a, max(rgb))`
        //    必须不小于旧 dst.a（`:92` 的 max 只升不降）
        let out = 加性单像素([0, 0, 0, 128], [10, 20, 30, 64]);
        assert_eq!(
            out,
            [6, 10, 16, 128],
            "sr/sg/sb=(3,5,8)、oa=max(128,max)=128 ⇒ 反预乘 (6,10,16)"
        );
        assert!(out[3] >= 128, "A 不许低于旧 dst.a：{out:?}");
    }

    /// 加性混合的三条边界与 `blit` 同口径（共用同一份骨架）。
    #[test]
    fn 加性_边界与blit同口径() {
        // ① `sa == 0` ⇒ 一个字节都不碰、返回 false
        let mut c = Canvas::new(1, 1);
        c.rgba.copy_from_slice(&[7, 8, 9, 255]);
        assert!(
            !c.blit_additive(&帧(1, 1, vec![200, 200, 200, 0]), 0, 0),
            "全透明源在加性混合下贡献恒 0 ⇒ 不许写、不许算落笔"
        );
        assert_eq!(c.into_rgba(), vec![7, 8, 9, 255]);

        // ② 空帧 / 完全在画布外 ⇒ false
        let mut c2 = Canvas::new(2, 2);
        assert!(!c2.blit_additive(&帧(0, 0, Vec::new()), 0, 0));
        assert!(!c2.blit_additive(&帧(4, 4, [1u8, 2, 3, 255].repeat(16)), 10, 10));
        assert!(c2.into_rgba().iter().all(|b| *b == 0));

        // ③ 越界裁剪：2×2 帧贴在 (-1,-1) ⇒ 只有 (0,0) 落在画布内。
        //    注意底色**全透明**时的加性结果不是「原样贴上源」：预乘相加得
        //    `(100,0,0)`、`A = max(0, 100) = 100` ⇒ 反预乘 `(255,0,0,100)`
        //    （= 预乘语义下的同一像素，肉眼上等价于「39% 的红」）。
        let mut c3 = Canvas::new(2, 2);
        assert!(c3.blit_additive(&帧(2, 2, [100u8, 0, 0, 255].repeat(4)), -1, -1));
        let px = c3.into_rgba();
        assert_eq!(&px[0..4], &[255, 0, 0, 100], "(0,0) 应被写到");
        assert_eq!(&px[4..8], &[0, 0, 0, 0], "(1,0) 在帧外");
        assert_eq!(&px[12..16], &[0, 0, 0, 0], "(0,1) 在帧外");
    }

    /// 抽公共骨架后，**默认路径（`blit`）必须与 `over()` 逐字节相同**（防误改）。
    #[test]
    fn 默认路径_blit与over逐字节相同() {
        let 源 = [10u8, 20, 30, 64];
        let 底 = [50u8, 100, 150, 128];
        let mut c = Canvas::new(1, 1);
        c.rgba.copy_from_slice(&底);
        assert!(c.blit(&帧(1, 1, 源.to_vec()), 0, 0));
        assert_eq!(
            c.into_rgba(),
            over(源, 底).to_vec(),
            "`blit` 改走公共骨架后，输出必须仍逐字节等于 `over(src, dst)`"
        );
    }
}
