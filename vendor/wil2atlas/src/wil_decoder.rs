//! WIL/WIX 二进制格式解码器。
//!
//! 字段布局全部依据本地 C++ 源码实证（`E:\GodM2\client\reference_full\Classes\Wil\wmM2Def.cpp`
//! 及 `WIL.h`），并用真实样本（Hum.wil / Tiles.wil）字节验证过，证据见
//! `E:\GodM2\planning\协议\WIL格式实证笔记.md`。
//!
//! 与研判文档不一致处以源码/实证为准，主要差异：
//! 1. 老格式（IndexOffset==0，本项目全部样本均为老格式）WIL 头为 **56 字节**（含 IndexOffset
//!    字段名义上是 60 字节，但该字段为 0 时不计入实际数据布局），调色板 1024 字节紧跟在头部之后；
//! 2. WIX 头为 **48 字节**（Title[41] + bitCount[3] + IndexCount i32），偏移数组从 offset 48 开始，
//!    没有 "MirOf" 前缀，也没有 VerFlag；
//! 3. 本项目样本的帧数据是**未压缩**的裸调色板索引（每行按 4 字节对齐、自底向上存储），
//!    并非研判文档所述的「zlib 压缩」。zlib（PKGD）与内嵌 PNG 仅出现在带标记的新帧上。

use anyhow::{bail, Context, Result};

/// WIL 文件头（对应 C++ `TWMImageHeader`，`#pragma pack(2)` 下名义 60 字节）。
#[derive(Debug, Clone)]
pub struct WilHeader {
    /// 标志字节：0x19=普通（WEMADE），0xFF=加密（索引需 XOR Password），
    /// 0xFE=变体（Password 先 ^0x2B2B2B2B 再按 0xFF 处理）。实证样本为 0x19。
    pub flag: u8,
    /// 标题字符串（39 字节定长，0x19 布局下实际 38 字符 + 1 结尾零）。
    pub title: String,
    /// 加密口令（与 Flag 配合使用，样本为 0）。
    pub password: i32,
    /// 声明的帧数（Flag==0xFF 时需与 Password 异或后才是真实帧数）。
    pub image_count: i32,
    /// 颜色数：0x100=8 位调色板；0x10000=16 位 R5G6B5；0xFFFFFFFF=32 位 BGRA。
    pub color_count: i32,
    /// 调色板尺寸（字节），样本为 1024。
    pub palette_size: i32,
    /// 索引偏移：非 0 表示「新格式」（帧头多 4 字节 nSize 字段）；0 = 老格式。
    pub index_offset: i32,
}

/// 解析后的调色板，RGBA 每通道一字节，256 项。
pub type Palette = [[u8; 4]; 256];

/// 单帧元信息（不含像素）。
#[derive(Debug, Clone)]
pub struct FrameInfo {
    /// 帧在 WIL 文件中的字节偏移（来自 WIX，可能为 0 表示空帧）。
    pub offset: u32,
    /// 帧宽（像素）。
    pub width: u32,
    /// 帧高（像素）。
    pub height: u32,
    /// 锚点 X（对应 C++ TDXTextureInfo.px）。
    pub px: i16,
    /// 锚点 Y（对应 C++ TDXTextureInfo.py）。
    pub py: i16,
    /// 帧载荷类别：Raw=未压缩 8 位调色板索引；Raw16/Raw32=16/32 位裸数据；
    /// Zlib=PKGD 标记的 zlib 压缩数据；Png=内嵌 PNG。
    pub kind: FrameKind,
    /// 该帧是否为有效可渲染帧（空帧/尺寸越界的帧为 false）。
    pub valid: bool,
}

/// 帧载荷类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameKind {
    /// 未压缩 8 位调色板索引（本项目样本的实际形态）。
    Raw,
    /// 未压缩 16 位 R5G6B5（ColorCount==0x10000）。
    Raw16,
    /// 未压缩 32 位 BGRA（ColorCount==0xFFFFFFFF）。
    Raw32,
    /// PKGD 标记（0x44474B50）：dataLen 字节 zlib 数据，解压后同 Raw 布局。
    Zlib,
    /// PNG 标记（0x474E5089）：内嵌 PNG 文件流。
    Png,
    /// 空帧（WIX 偏移为 0）。
    Empty,
}

/// 解码完成的一帧：RGBA8888 像素 + 元信息。
pub struct DecodedFrame {
    pub info: FrameInfo,
    /// 长度 = width * height * 4，行优先自顶向下。
    pub rgba: Vec<u8>,
    /// 解码实际使用的宽（与 info.width 相同，保留字段以便后续裁剪逻辑）。
    pub width: u32,
    pub height: u32,
}

/// 解码完成的一帧：8 位调色板索引 + 元信息（--emit-indexed 路径）。
/// 与 [`DecodedFrame`] 一一对应：`rgba[i] == palette[indices[i]]`（索引 0 为透明）。
pub struct DecodedIndexFrame {
    pub info: FrameInfo,
    /// 长度 = width * height，行优先自顶向下；每字节为调色板索引（0..=255）。
    pub indices: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// WIL 主解析器：持有整个文件字节（Hum.wil 338MB，一次性载入可接受；
/// 后续图集打包阶段如需再优化为按偏移随机读）。
pub struct WilLib {
    pub header: WilHeader,
    /// 生效口令：Flag==0xFF 时等于 Password，否则 0；用于 XOR WIX 偏移。
    pub effective_password: i32,
    /// true = 老格式（IndexOffset==0，帧头 8 字节）；false = 新格式（12 字节）。
    pub old_format: bool,
    /// WIL 文件内嵌调色板（老格式位于 offset 56，新格式位于 offset 60）。
    pub file_palette: Option<Palette>,
    /// WIX 偏移表（已 XOR 解密）。
    pub offsets: Vec<u32>,
    data: Vec<u8>,
}

/// C++ `WidthBytes(nBit, nWidth)`：位宽 nBit 下每行字节数（4 字节对齐）。
/// 证据：wmM2Def.cpp `(((nWidth * nBit) + 31) >> 5) * 4`。
#[inline]
pub fn width_bytes(bit: u32, width: u32) -> u32 {
    ((width * bit + 31) >> 5) * 4
}

/// ColorCount 特殊值：0xFFFFFFFF = 32 位 BGRA；0x10000 = 16 位 R5G6B5（WIL.h 实证）。
pub const COLOR_COUNT_32BIT: i32 = 0xFFFFFFFFu32 as i32;
pub const COLOR_COUNT_16BIT: i32 = 0x10000;

/// C++ `MINIMAGESIZE` / `MAXIMAGESIZE`（WIL.h）。
pub const MIN_IMAGE_SIZE: u32 = 2;
pub const MAX_IMAGE_SIZE: u32 = 2048;

impl WilLib {
    /// 从字节流解析 WIL 头 + 调色板，并配合 WIX 字节流解析偏移表。
    pub fn open(wil: Vec<u8>, wix: &[u8]) -> Result<Self> {
        if wil.len() < 60 {
            bail!("WIL 文件过小（{} 字节），连 60 字节头都读不下", wil.len());
        }
        let header = parse_wil_header(&wil)?;
        let mut password = header.password;
        let mut flag = header.flag;
        // 对应 C++ Initialize()：Flag==0xFE 时先解一次口令再当 0xFF 处理。
        if flag == 0xFE {
            if password != 0 {
                password ^= 0x2B2B2B2Bu32 as i32;
            }
            flag = 0xFF;
        }
        let encrypted = flag == 0xFF;
        let effective_password = if encrypted { password } else { 0 };
        let real_image_count = if encrypted {
            header.image_count ^ password
        } else {
            header.image_count
        };
        let old_format = header.index_offset == 0;

        // 老格式：头实际占 56 字节（C++ 读满 60 后回退 4 字节，等价）；
        // 新格式：60 字节。调色板紧随其后。
        let palette_off = if old_format { 56 } else { 60 } as usize;
        let file_palette = if header.palette_size >= 1024 && wil.len() >= palette_off + 1024 {
            Some(parse_palette(&wil[palette_off..palette_off + 1024]))
        } else {
            None
        };

        // WIX 偏移表（见实证笔记：老格式头 48 字节，无 MirOf 前缀）。
        let offsets = parse_wix_offsets(wix, old_format, effective_password, real_image_count)?;

        Ok(Self {
            header,
            effective_password,
            old_format,
            file_palette,
            offsets,
            data: wil,
        })
    }

    /// 解析第 `index` 帧的元信息（不解码像素）。index 越界返回 None。
    pub fn frame_info(&self, index: usize) -> Option<FrameInfo> {
        if index >= self.offsets.len() {
            return None;
        }
        let position = self.offsets[index];
        // C++：position==0 表示空帧，直接跳过。
        if position == 0 {
            return Some(FrameInfo {
                offset: 0,
                width: 0,
                height: 0,
                px: 0,
                py: 0,
                kind: FrameKind::Empty,
                valid: false,
            });
        }
        let d = &self.data;
        let pos = position as usize;
        if pos + 8 > d.len() {
            // 偏移越界视为损坏帧（不 panic，记录为无效）。
            return Some(FrameInfo {
                offset: position,
                width: 0,
                height: 0,
                px: 0,
                py: 0,
                kind: FrameKind::Empty,
                valid: false,
            });
        }
        let width = u16::from_le_bytes([d[pos], d[pos + 1]]) as u32;
        let height = u16::from_le_bytes([d[pos + 2], d[pos + 3]]) as u32;
        let px = i16::from_le_bytes([d[pos + 4], d[pos + 5]]);
        let py = i16::from_le_bytes([d[pos + 6], d[pos + 7]]);

        // C++ ReadImageFile 的有效性门槛：0 尺寸直接跳过；<2 或 >2048 跳过。
        if (width == 0 && height == 0)
            || width > MAX_IMAGE_SIZE
            || height > MAX_IMAGE_SIZE
            || width < MIN_IMAGE_SIZE
            || height < MIN_IMAGE_SIZE
        {
            return Some(FrameInfo {
                offset: position,
                width,
                height,
                px,
                py,
                kind: FrameKind::Empty,
                valid: false,
            });
        }

        // 判定载荷类别：读 dataLen(4) + mark(4)，老格式帧头仅 8 字节，
        // 这 8 字节其实是像素数据开头，mark 命中不了 PNG/PKGD 即按裸数据回读
        //（与 C++「mark 不认识就回退 8 字节读裸数据」的行为一致）。
        let hdr_len = if self.old_format { 8 } else { 12 }; // 新格式多一个 nSize 字段
        let (kind, valid) = if pos + hdr_len + 8 <= d.len() {
            let mark =
                i32::from_le_bytes(d[pos + hdr_len + 4..pos + hdr_len + 8].try_into().unwrap());
            match mark {
                0x474E5089 => (FrameKind::Png, true),  // "\x89PNG" 小端
                0x44474B50 => (FrameKind::Zlib, true), // "PKGD" 小端
                _ => (self.raw_kind(), true),
            }
        } else {
            (self.raw_kind(), false)
        };
        Some(FrameInfo {
            offset: position,
            width,
            height,
            px,
            py,
            kind,
            valid,
        })
    }

    /// 按 ColorCount 决定裸数据位深类别。
    fn raw_kind(&self) -> FrameKind {
        match self.header.color_count {
            COLOR_COUNT_32BIT => FrameKind::Raw32,
            COLOR_COUNT_16BIT => FrameKind::Raw16,
            _ => FrameKind::Raw,
        }
    }

    /// 完整解码第 `index` 帧为 RGBA8888。
    pub fn decode_frame(&self, index: usize, palette: &Palette) -> Result<DecodedFrame> {
        let info = self
            .frame_info(index)
            .with_context(|| format!("帧索引 {index} 越界"))?;
        if !info.valid {
            bail!("帧 {index} 无效（空帧或尺寸越界），无法解码");
        }
        let d = &self.data;
        let pos = info.offset as usize;
        let hdr_len = if self.old_format { 8 } else { 12 };
        let (w, h) = (info.width, info.height);

        let rgba: Vec<u8> = match info.kind {
            FrameKind::Png => {
                // C++：读完 mark 后回退 4 字节（PNG 头自带 4 字节被 dataLen 覆盖），
                // 再读 dataLen 字节为完整 PNG 流。dataLen 位于 mark 前 4 字节。
                let data_len_pos = pos + hdr_len;
                let data_len =
                    i32::from_le_bytes(d[data_len_pos..data_len_pos + 4].try_into().unwrap())
                        as usize;
                // PNG 标记 0x474E5089 小端字节序即 "\x89PNG"——标记本身就是 PNG 流的前 4 字节，
                // 因此 PNG 流从 mark 位置（dataLen 之后）开始（C++ 读完 mark 回退 4 字节，等价）。
                let png_start = data_len_pos + 4;
                let png_end = (png_start + data_len).min(d.len());
                let img = image::load_from_memory(&d[png_start..png_end])
                    .with_context(|| format!("帧 {index} 内嵌 PNG 解码失败"))?;
                let raw_w = img.width();
                let raw_h = img.height();
                // C++ 把奇数宽高裁成偶数（渲染用），转换工具保留原始尺寸：
                // 若 PNG 尺寸与帧头声明不一致，以帧头为准做左上裁剪/保持。
                let mut rgba = img.to_rgba8().into_raw();
                let tw = w.min(raw_w);
                let th = h.min(raw_h);
                let mut out = vec![0u8; (w * h * 4) as usize];
                for row in 0..th {
                    let src = ((row * raw_w) as usize) * 4;
                    let dst = ((row * w) as usize) * 4;
                    out[dst..dst + (tw as usize) * 4]
                        .copy_from_slice(&rgba[src..src + (tw as usize) * 4]);
                }
                rgba = out;
                rgba
            }
            FrameKind::Zlib => {
                // PKGD：dataLen 字节 zlib 数据，解压后按裸数据布局处理。
                let data_len_pos = pos + hdr_len;
                let data_len =
                    i32::from_le_bytes(d[data_len_pos..data_len_pos + 4].try_into().unwrap())
                        as usize;
                let start = data_len_pos + 8;
                let end = (start + data_len).min(d.len());
                let raw = inflate_zlib(&d[start..end])
                    .with_context(|| format!("帧 {index} zlib 解压失败"))?;
                indices_to_rgba(&raw, w, h, self.header.color_count, palette)?
            }
            FrameKind::Raw | FrameKind::Raw16 | FrameKind::Raw32 => {
                // 裸数据：帧头后直接是像素（C++ 先读 8 字节当 dataLen/mark，
                // mark 不命中再回退 8 字节，等价于从头读）。
                let start = pos + hdr_len;
                let need = (width_bytes(bits_of(self.header.color_count), w) * h) as usize;
                if start + need > d.len() {
                    bail!(
                        "帧 {index} 裸数据越界：需要 {need} 字节，文件剩余 {}",
                        d.len() - start
                    );
                }
                indices_to_rgba(
                    &d[start..start + need],
                    w,
                    h,
                    self.header.color_count,
                    palette,
                )?
            }
            FrameKind::Empty => bail!("帧 {index} 为空帧"),
        };

        Ok(DecodedFrame {
            info,
            rgba,
            width: w,
            height: h,
        })
    }

    /// 尝试把第 `index` 帧解码为 **8 位调色板索引**（不查调色板）。
    ///
    /// 返回值语义（供 `--emit-indexed` 使用，避免把「不可索引」误当解码错误）：
    /// - `Ok(Some(frame))`：该帧原生就是 8 位索引（ColorCount==0x100 的 Raw / Zlib 载
    ///   荷），`indices` 可直接用作索引图；
    /// - `Ok(None)`：该帧非 8 位索引承载（内嵌 PNG / 16 位 R5G6B5 / 32 位 BGRA），
    ///   无法无中生有地取回索引，需上层决定是否做 RGBA→索引反向映射；
    /// - `Err(_)`：真实解码失败（越界/解压失败等）。
    ///
    /// 与 [`Self::decode_frame`] 的一致性：对 `Ok(Some)` 的帧，
    /// `palette[indices[i]]` 与 `decode_frame(...).rgba[i*4..i*4+4]` **逐像素相等**。
    pub fn decode_frame_indices_opt(&self, index: usize) -> Result<Option<DecodedIndexFrame>> {
        let info = self
            .frame_info(index)
            .with_context(|| format!("帧索引 {index} 越界"))?;
        if !info.valid {
            bail!("帧 {index} 无效（空帧或尺寸越界），无法解码");
        }
        // 仅 8 位调色板（ColorCount==0x100）的 Raw/Zlib 载荷才是原生索引。
        if bits_of(self.header.color_count) != 8 {
            return Ok(None);
        }
        let d = &self.data;
        let pos = info.offset as usize;
        let hdr_len = if self.old_format { 8 } else { 12 };
        let (w, h) = (info.width, info.height);

        let raw: Vec<u8> = match info.kind {
            FrameKind::Zlib => {
                let data_len_pos = pos + hdr_len;
                let data_len =
                    i32::from_le_bytes(d[data_len_pos..data_len_pos + 4].try_into().unwrap())
                        as usize;
                let start = data_len_pos + 8;
                let end = (start + data_len).min(d.len());
                inflate_zlib(&d[start..end]).with_context(|| format!("帧 {index} zlib 解压失败"))?
            }
            FrameKind::Raw => {
                let start = pos + hdr_len;
                let need = (width_bytes(8, w) * h) as usize;
                if start + need > d.len() {
                    bail!(
                        "帧 {index} 裸数据越界：需要 {need} 字节，文件剩余 {}",
                        d.len() - start
                    );
                }
                d[start..start + need].to_vec()
            }
            // PNG / 16 位 / 32 位：非 8 位索引承载。
            FrameKind::Png | FrameKind::Raw16 | FrameKind::Raw32 | FrameKind::Empty => {
                return Ok(None)
            }
        };

        let indices =
            index_block_from_raw(&raw, w, h).with_context(|| format!("帧 {index} 取索引块失败"))?;
        Ok(Some(DecodedIndexFrame {
            info,
            indices,
            width: w,
            height: h,
        }))
    }
}

/// 从「行 4 字节对齐 + 自底向上（BMP 序）」的裸 8 位索引块中，
/// 取出**无 padding、自顶向下**的 w×h 索引（与 [`indices_to_rgba`] 的 8 位分支行序一致）。
pub fn index_block_from_raw(raw: &[u8], w: u32, h: u32) -> Result<Vec<u8>> {
    let stride = width_bytes(8, w) as usize;
    let need = stride * h as usize;
    if raw.len() < need {
        bail!(
            "索引块越界：需要 {need} 字节（stride={stride}×h={h}），实际 {}",
            raw.len()
        );
    }
    let mut out = vec![0u8; (w * h) as usize];
    for row in 0..h as usize {
        let src = (h as usize - 1 - row) * stride;
        let dst = row * w as usize;
        out[dst..dst + w as usize].copy_from_slice(&raw[src..src + w as usize]);
    }
    Ok(out)
}

/// 按颜色深度把裸像素块转成 RGBA8888。
/// 关键点（C++ CopyBMP8/16/32ToImage 实证）：行存储是**自底向上**（BMP 顺序），
/// 每行按 `WidthBytes` 对齐。
pub fn indices_to_rgba(
    raw: &[u8],
    w: u32,
    h: u32,
    color_count: i32,
    palette: &Palette,
) -> Result<Vec<u8>> {
    let stride = width_bytes(bits_of(color_count), w) as usize;
    let mut out = vec![0u8; (w * h * 4) as usize];
    match color_count {
        // 32 位 BGRA：输出 R=pix[2], G=pix[1], B=pix[0], A=pix[3]（C++ CopyBMP32ToImage）。
        COLOR_COUNT_32BIT => {
            for row in 0..h as usize {
                let src = &raw[(h as usize - 1 - row) * stride..];
                for col in 0..w as usize {
                    let p = &src[col * 4..col * 4 + 4];
                    let o = (row * w as usize + col) * 4;
                    out[o] = p[2];
                    out[o + 1] = p[1];
                    out[o + 2] = p[0];
                    out[o + 3] = p[3];
                }
            }
        }
        // 16 位 R5G6B5：RGB 全 0 时 alpha=0，否则 255（C++ CopyBMP16ToImage）。
        COLOR_COUNT_16BIT => {
            for row in 0..h as usize {
                let src = &raw[(h as usize - 1 - row) * stride..];
                for col in 0..w as usize {
                    let c = u16::from_le_bytes([src[col * 2], src[col * 2 + 1]]);
                    let r = ((c & 0xF800) >> 11) << 3;
                    let g = ((c & 0x07E0) >> 5) << 2;
                    let b = (c & 0x001F) << 3;
                    let a: u8 = if r | g | b == 0 { 0x00 } else { 0xFF };
                    let o = (row * w as usize + col) * 4;
                    out[o] = r as u8;
                    out[o + 1] = g as u8;
                    out[o + 2] = b as u8;
                    out[o + 3] = a;
                }
            }
        }
        // 8 位调色板索引：RGBA = 调色板[idx]（C++ CopyBMP8ToImage；
        // 索引 0 在调色板中即全 0 = 透明）。
        _ => {
            for row in 0..h as usize {
                let src = &raw[(h as usize - 1 - row) * stride..];
                for (col, &idx) in src.iter().enumerate().take(w as usize) {
                    let o = (row * w as usize + col) * 4;
                    out[o..o + 4].copy_from_slice(&palette[idx as usize]);
                }
            }
        }
    }
    Ok(out)
}

#[inline]
fn bits_of(color_count: i32) -> u32 {
    match color_count {
        COLOR_COUNT_32BIT => 32,
        COLOR_COUNT_16BIT => 16,
        _ => 8,
    }
}

/// 解析 60 字节 WIL 头（C++ TWMImageHeader，pack(2) 布局与实际样本一致）。
fn parse_wil_header(d: &[u8]) -> Result<WilHeader> {
    Ok(WilHeader {
        flag: d[0],
        title: parse_wil_title(&d[1..40]),
        password: i32::from_le_bytes(d[40..44].try_into().unwrap()),
        image_count: i32::from_le_bytes(d[44..48].try_into().unwrap()),
        color_count: i32::from_le_bytes(d[48..52].try_into().unwrap()),
        palette_size: i32::from_le_bytes(d[52..56].try_into().unwrap()),
        index_offset: i32::from_le_bytes(d[56..60].try_into().unwrap()),
    })
}

/// 标题按首个 NUL 截断（39 字节定长，样本为 "WEMADE Entertainment inc."）。
fn parse_wil_title(raw: &[u8]) -> String {
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    String::from_utf8_lossy(&raw[..end]).into_owned()
}

/// 解析 WIL 文件内嵌调色板：1024 字节，每项 4 字节 {B, G, R, A}（BGRA 序，实证确认）。
pub fn parse_palette(raw: &[u8]) -> Palette {
    let mut pal: Palette = [[0u8; 4]; 256];
    for i in 0..256 {
        let b = raw[i * 4];
        let g = raw[i * 4 + 1];
        let r = raw[i * 4 + 2];
        let a = raw[i * 4 + 3];
        pal[i] = [r, g, b, a];
    }
    pal
}

/// 解析 WIX 偏移表。
///
/// 实证布局（老格式，本项目全部样本）：
///   offset 0..48  = Title[41] + bitCount[3] + IndexCount i32（IndexCount 在 44..48）
///   offset 48..   = IndexCount × i32 偏移数组（需 XOR Password）
///
/// C++ LoadIndex 另支持 "MirOf" 5 字节前缀 + 新格式 52 字节头（含 VerFlag），
/// 此处一并兼容。
fn parse_wix_offsets(
    wix: &[u8],
    old_format: bool,
    password: i32,
    expect_count: i32,
) -> Result<Vec<u32>> {
    if wix.len() < 5 {
        bail!("WIX 文件过小");
    }
    // C++：前 5 字节为 "MirOf" 时整体后移 5 字节。
    let base = if &wix[..5] == b"MirOf" { 5 } else { 0 };
    let hdr_len = if old_format { 48 } else { 52 };
    // IndexCount 恒在 Title[41]+bitCount[3] 之后（相对基址 44；新格式其后还有 VerFlag）。
    let count_pos = base + 44;
    if wix.len() < count_pos + 4 {
        bail!(
            "WIX 头不完整：需要 {} 字节，实际 {}",
            count_pos + 4,
            wix.len()
        );
    }
    let index_count = i32::from_le_bytes(wix[count_pos..count_pos + 4].try_into().unwrap());
    // C++ MAXIMAGECOUNT = 10000000 防御。
    if index_count <= 0 || index_count > 10_000_000 {
        bail!("WIX IndexCount 非法: {index_count}");
    }
    if let Some(exp) = expect_count_checked(expect_count) {
        if index_count != exp {
            // 不硬失败：WIL.ImageCount 与 WIX.IndexCount 可能不一致，以 WIX 为准但留痕。
            eprintln!(
                "[warn] WIL.ImageCount({exp}) 与 WIX.IndexCount({index_count}) 不一致，以 WIX 为准"
            );
        }
    }
    let start = base + hdr_len;
    let end = start + index_count as usize * 4;
    if wix.len() < end {
        bail!("WIX 偏移数组越界：需要 {} 字节，实际 {}", end, wix.len());
    }
    let mut out = Vec::with_capacity(index_count as usize);
    for i in 0..index_count as usize {
        let v = i32::from_le_bytes(wix[start + i * 4..start + i * 4 + 4].try_into().unwrap());
        out.push((v ^ password) as u32);
    }
    Ok(out)
}

fn expect_count_checked(n: i32) -> Option<i32> {
    if n > 0 && n <= 10_000_000 {
        Some(n)
    } else {
        None
    }
}

/// zlib 解压（对应 C++ decZlib，flate2 读端）。
pub fn inflate_zlib(input: &[u8]) -> Result<Vec<u8>> {
    use std::io::Read;
    let mut out = Vec::with_capacity(input.len() * 5);
    let mut decoder = flate2::read::ZlibDecoder::new(input);
    decoder
        .read_to_end(&mut out)
        .with_context(|| "zlib 解压失败".to_string())?;
    Ok(out)
}

/// zlib 压缩（公开给测试用于构造 PKGD fixture）。
#[doc(hidden)]
pub fn zlib_compress_for_fixture(input: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(input).unwrap();
    enc.finish().unwrap()
}
