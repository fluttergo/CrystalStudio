//! PNG 读写（**非渲染核心**：只用于导出参考图与读客户端截图做比对）。
//!
//! 之所以单独一模块：渲染核心（`canvas` / `compose` / `frames`）只碰 `Vec<u8>` RGBA，
//! 不依赖任何图像编解码库；这里才把 `png` 依赖收进来，方便以后换实现（GUI 侧要直接
//! 上纹理时甚至可以不要本模块）。

use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;

/// 一张读回来的 RGBA8 图（行优先，`rgba.len() == width*height*4`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PngImage {
    /// 像素宽。
    pub width: u32,
    /// 像素高。
    pub height: u32,
    /// 行优先 RGBA8。
    pub rgba: Vec<u8>,
}

impl PngImage {
    /// 取某像素 RGBA；越界返回 `None`。
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = (y as usize * self.width as usize + x as usize) * 4;
        let s = self.rgba.get(i..i + 4)?;
        Some([s[0], s[1], s[2], s[3]])
    }

    /// 导出为 PNG（RGBA8）。
    pub fn save_png(&self, path: &Path) -> std::io::Result<()> {
        write_rgba_png(path, self.width, self.height, &self.rgba)
    }

    /// 从 `x, y` 起裁一块 `w × h`（越界部分用全透明补齐，不 panic）。
    pub fn crop(&self, x: u32, y: u32, w: u32, h: u32) -> PngImage {
        let mut out = vec![0u8; w as usize * h as usize * 4];
        for row in 0..h {
            let sy = y + row;
            if sy >= self.height {
                break;
            }
            for col in 0..w {
                let sx = x + col;
                if sx >= self.width {
                    break;
                }
                let si = (sy as usize * self.width as usize + sx as usize) * 4;
                let di = (row as usize * w as usize + col as usize) * 4;
                out[di..di + 4].copy_from_slice(&self.rgba[si..si + 4]);
            }
        }
        PngImage {
            width: w,
            height: h,
            rgba: out,
        }
    }
}

/// 把行优先 RGBA8 写成 PNG。目录不存在会直接报错（不隐式建目录，避免到处乱造路径）。
pub fn write_rgba_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> std::io::Result<()> {
    let expect = width as usize * height as usize * 4;
    if rgba.len() != expect {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "RGBA 字节数 {} 与 {width}×{height} 不符（应为 {expect}）",
                rgba.len()
            ),
        ));
    }
    let file = File::create(path)?;
    let w = BufWriter::new(file);
    let mut enc = png::Encoder::new(w, width, height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header()?;
    writer.write_image_data(rgba)?;
    writer.finish()?;
    Ok(())
}

/// 读一张 PNG，统一转成 RGBA8（RGB / 灰度 / 带 alpha 的灰度都会展开成 RGBA）。
pub fn read_png(path: &Path) -> std::io::Result<PngImage> {
    let file = BufReader::new(File::open(path)?);
    解码(file)
}

/// 从**内存字节**读一张 PNG，口径与 [`read_png`] 完全相同（同一个解码器）。
///
/// 用途：光罩贴图用 `include_bytes!` 在**编译期**嵌进二进制（见 `crate::lighting`），
/// 运行期不碰磁盘 —— 这样「贴图缺失」是编译错误而不是运行期静默降级。
///
/// ⚠ 本函数仍然只住在本模块：`png` 依赖**只出现在 `png_io.rs` 一个文件**，
/// 渲染核心（`canvas` / `compose` / `lighting`）只吃 `Vec<u8>`（模块文档口径不变）。
pub fn read_png_from_memory(bytes: &[u8]) -> std::io::Result<PngImage> {
    解码(std::io::Cursor::new(bytes))
}

/// 解码器本体（`read_png` / `read_png_from_memory` 共用的唯一实现）。
fn 解码<R: std::io::BufRead + std::io::Seek>(input: R) -> std::io::Result<PngImage> {
    let mut dec = png::Decoder::new(input);
    // EXPAND：把调色板/低位深展开成 8bit；ALPHA：把 tRNS 变成真 alpha。
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let mut reader = dec.read_info()?;
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "PNG 尺寸无法确定"))?;
    let mut buf = vec![0u8; size];
    let info = reader.next_frame(&mut buf)?;
    let src = &buf[..info.buffer_size()];
    let (w, h) = (info.width, info.height);
    let n = w as usize * h as usize;

    let rgba = match info.color_type {
        png::ColorType::Rgba => src.to_vec(),
        png::ColorType::Rgb => {
            let mut out = vec![255u8; n * 4];
            for i in 0..n {
                out[i * 4] = src[i * 3];
                out[i * 4 + 1] = src[i * 3 + 1];
                out[i * 4 + 2] = src[i * 3 + 2];
            }
            out
        }
        png::ColorType::Grayscale => {
            let mut out = vec![255u8; n * 4];
            for i in 0..n {
                let g = src[i];
                out[i * 4] = g;
                out[i * 4 + 1] = g;
                out[i * 4 + 2] = g;
            }
            out
        }
        png::ColorType::GrayscaleAlpha => {
            let mut out = vec![0u8; n * 4];
            for i in 0..n {
                let g = src[i * 2];
                let a = src[i * 2 + 1];
                out[i * 4] = g;
                out[i * 4 + 1] = g;
                out[i * 4 + 2] = g;
                out[i * 4 + 3] = a;
            }
            out
        }
        png::ColorType::Indexed => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "调色板 PNG 未按 EXPAND 展开（不该发生）",
            ))
        }
    };

    Ok(PngImage {
        width: w,
        height: h,
        rgba,
    })
}
