//! WZL/WZX 二进制格式解码器（新版 Mir2 图库；热血传奇 23 周年客户端等）。
//!
//! 与 [`crate::wil_decoder`]（老版 WIL/WIX）**格式不同**，字段布局全部由真实样本
//! 字节实证得到，证据见 `E:\GodM2\deliverables\勘察-新增三资源(23周年客户端+苹果引擎+绿盟登录器)-2026-10-11.md` §A.1。
//!
//! ## 实测布局
//!
//! **`.wzx`（索引）**：
//! ```text
//! offset 0x00..0x2B  保留区（样本全 0）
//! offset 0x2C        u32 帧数 count
//! offset 0x30 起     count × u32 偏移（相对 .wzl 文件起点）
//! 文件大小 = 48 + count*4（四库实测精确匹配）
//! ```
//!
//! **`.wzl`（数据）**：
//! ```text
//! offset 0x00..0x3F  64 字节头（0x2C 处同 count；数据区自 0x40 起）
//! 每帧块（自 .wzx 给出的偏移起）：
//!   [0:2]  u16 变体标记（低字节 0x03=8bit / 0x05=16bit；高字节为变体）
//!   [2:4]  u16 未用（实测 0）
//!   [4:6]  u16 宽 w
//!   [6:8]  u16 高 h
//!   [8:10] i16 锚点 X
//!   [10:12] i16 锚点 Y
//!   [12:16] u32 压缩字节数 csize
//!   [16:16+csize] zlib 载荷
//! 校验：下一帧偏移 = 当前 + 16 + csize（npc/npc2/npc3/Hair 全部精确匹配）
//! ```
//!
//! ## 载荷（解压后）
//!
//! - **8bit 库/帧**（标记低字节 `0x03`）：长度 = `width_bytes(8,w) * h`（行 4 字节对齐），
//!   自底向上（BMP 序），与 WIL 的 `indices_to_rgba` 8 位分支**同构**，可直接复用。
//! - **16bit 帧**（标记低字节 `0x05`，含 `0x0105`/`0x0505`/`0x0905`）：长度 =
//!   `width_bytes(16,w) * h`（= `align4(w*2)*h`），R5G6B5，同样复用 16 位分支。
//!   （`0x0905` 载荷在颜色面之后另有尾随数据，本解码器只取前 `width_bytes(16,w)*h` 字节，实测渲染正确。）
//!
//! ## 已知未支持
//!
//! - 标记低字节 `0x06`（实测 3 字节/像素，`width_bytes(24,w)*h`；全 627 库仅 103 帧，集中在
//!   `Effect` 库）—— 判为无效帧并计入统计，不臆造解码。
//! - **无内嵌调色板**：WZL 头（64 字节）内无 1024 字节调色板；8bit 帧需由调用方提供
//!   （实测 `reference_full/Resources/data/npc.wil` 内嵌调色板与 Cocos 客户端硬编码表
//!   `palette_client::CLIENT_PALETTE` **逐项一致 0/256**，故回退该表即可）。

use anyhow::{bail, Context, Result};

use crate::wil_decoder::{
    index_block_from_raw, indices_to_rgba, inflate_zlib, width_bytes, DecodedFrame,
    DecodedIndexFrame, FrameInfo, FrameKind, Palette, WilHeader, COLOR_COUNT_16BIT,
};

/// WZX 索引头长度（实测 48 字节；帧数在 0x2C）。
pub const WZX_HEADER_LEN: usize = 48;
/// .wzl 数据区起始偏移（实测 64；48 字节头 + 16 字节空帧占位块）。
pub const WZL_DATA_START: u32 = 64;
/// 每帧块头长度（实测 16 字节）。
pub const WZL_BLOCK_HEADER_LEN: usize = 16;
/// WZL 帧尺寸上界（实测最大 2134×4320，宽于 WIL 的 2048，故放宽）。
pub const MAX_WZL_IMAGE_SIZE: u32 = 8192;
/// 帧尺寸下界（与 WIL 一致）。
pub const MIN_WZL_IMAGE_SIZE: u32 = 2;

/// 单帧解析结果（元信息 + 载荷定位）。
struct WzlFrame {
    info: FrameInfo,
    /// 该帧位深对应的 ColorCount（0x100=8bit / 0x10000=16bit）；无效帧为 0。
    color_count: i32,
    /// 解压前 zlib 载荷在 .wzl 内的偏移（无效帧为 0）。
    payload_off: usize,
    /// 解压前 zlib 载荷长度（无效帧为 0）。
    payload_len: usize,
}

/// WZL 主解析器：持有整个 .wzl 字节与 .wzx 偏移表。
pub struct WzlLib {
    pub header: WilHeader,
    /// WZL 无内嵌调色板，恒为 `None`（调用方需回退到外部调色板）。
    pub file_palette: Option<Palette>,
    /// .wzx 偏移表（原始 u32，未加密）。
    pub offsets: Vec<u32>,
    frames: Vec<WzlFrame>,
    data: Vec<u8>,
    /// 统计：识别为有效帧的位深计数（诊断用）。
    pub count_8bit: usize,
    pub count_16bit: usize,
}

impl WzlLib {
    /// 从 .wzl 字节流与 .wzx 字节流解析。
    pub fn open(wzl: Vec<u8>, wzx: &[u8]) -> Result<Self> {
        if wzx.len() < WZX_HEADER_LEN + 4 {
            bail!("WZX 文件过小（{} 字节），连 48 字节头 + 帧数都读不下", wzx.len());
        }
        let count = u32::from_le_bytes(wzx[0x2C..0x30].try_into().unwrap()) as usize;
        if count == 0 || count > 10_000_000 {
            bail!("WZX 帧数非法: {count}");
        }
        let need = WZX_HEADER_LEN + count * 4;
        if wzx.len() < need {
            bail!("WZX 偏移数组越界：需要 {need} 字节，实际 {}", wzx.len());
        }
        let mut offsets = Vec::with_capacity(count);
        for i in 0..count {
            let s = WZX_HEADER_LEN + i * 4;
            offsets.push(u32::from_le_bytes(wzx[s..s + 4].try_into().unwrap()));
        }

        let mut frames = Vec::with_capacity(count);
        let mut count_8bit = 0usize;
        let mut count_16bit = 0usize;
        for &off in &offsets {
            let f = parse_frame(&wzl, off);
            match f.color_count {
                0x100 => count_8bit += 1,
                COLOR_COUNT_16BIT => count_16bit += 1,
                // color_count == 0：空帧 / 尺寸越界 / 未知标记（0x06 等）——`parse_frame`
                // 已将三者统一折叠为无效帧（`valid=false`），此处无可统计的有效项。
                _ => {}
            }
            frames.push(f);
        }

        // 头部为合成值（WZL 头无 WIL 那套 flag/title/colorCount 字段）：
        // color_count 取占多数的位深，仅供 main 打印诊断；解码走逐帧 color_count。
        let color_count = if count_16bit >= count_8bit {
            COLOR_COUNT_16BIT
        } else {
            0x100
        };
        let header = WilHeader {
            flag: 0x19,
            title: "WZL (new-format Mir2 library)".to_string(),
            password: 0,
            image_count: count as i32,
            color_count,
            palette_size: 0,
            index_offset: 0,
        };

        Ok(Self {
            header,
            file_palette: None,
            offsets,
            frames,
            data: wzl,
            count_8bit,
            count_16bit,
        })
    }

    /// 解析第 `index` 帧的元信息（不解码像素）。index 越界返回 None。
    pub fn frame_info(&self, index: usize) -> Option<FrameInfo> {
        self.frames.get(index).map(|f| f.info.clone())
    }

    /// 完整解码第 `index` 帧为 RGBA8888。
    pub fn decode_frame(&self, index: usize, palette: &Palette) -> Result<DecodedFrame> {
        let f = self
            .frames
            .get(index)
            .with_context(|| format!("帧索引 {index} 越界"))?;
        if !f.info.valid {
            bail!("帧 {index} 无效（空帧 / 尺寸越界 / 未知标记），无法解码");
        }
        let (w, h) = (f.info.width, f.info.height);
        let raw = self.payload_bytes(f, index)?;
        let rgba = indices_to_rgba(&raw, w, h, f.color_count, palette)
            .with_context(|| format!("帧 {index} 像素转换失败"))?;
        Ok(DecodedFrame {
            info: f.info.clone(),
            rgba,
            width: w,
            height: h,
        })
    }

    /// 尝试把第 `index` 帧解码为 **8 位调色板索引**（供 `--emit-indexed`）。
    /// 仅 8bit 帧（标记低字节 0x03）返回 `Ok(Some(..))`；16bit 帧返回 `Ok(None)`。
    pub fn decode_frame_indices_opt(&self, index: usize) -> Result<Option<DecodedIndexFrame>> {
        let f = self
            .frames
            .get(index)
            .with_context(|| format!("帧索引 {index} 越界"))?;
        if !f.info.valid {
            bail!("帧 {index} 无效（空帧 / 尺寸越界 / 未知标记），无法解码");
        }
        if f.color_count != 0x100 {
            return Ok(None);
        }
        let (w, h) = (f.info.width, f.info.height);
        let raw = self.payload_bytes(f, index)?;
        let indices =
            index_block_from_raw(&raw, w, h).with_context(|| format!("帧 {index} 取索引块失败"))?;
        Ok(Some(DecodedIndexFrame {
            info: f.info.clone(),
            indices,
            width: w,
            height: h,
        }))
    }

    /// 取某帧像素字节：`csize > 0` ⇒ zlib 解压；`csize == 0` ⇒ **未压缩裸数据**
    /// （实测：块头后紧跟 `width_bytes(bits,w)*h` 字节，例 Hair#2944 8×9=72 字节）。
    /// 两种形态都截取到 `width_bytes(bits,w)*h`（丢弃 0x0905 的尾随数据）。
    fn payload_bytes(&self, f: &WzlFrame, index: usize) -> Result<Vec<u8>> {
        let bits = if f.color_count == COLOR_COUNT_16BIT { 16 } else { 8 };
        let stride = width_bytes(bits, f.info.width) as usize;
        let need = stride * f.info.height as usize;
        if f.payload_len == 0 {
            let end = f.payload_off + need;
            if end > self.data.len() {
                bail!(
                    "帧 {index} 裸数据越界：需要 {need} 字节，文件剩余 {}",
                    self.data.len().saturating_sub(f.payload_off)
                );
            }
            return Ok(self.data[f.payload_off..end].to_vec());
        }
        let end = f.payload_off + f.payload_len;
        let dec = inflate_zlib(&self.data[f.payload_off..end])
            .with_context(|| format!("帧 {index} zlib 解压失败"))?;
        if dec.len() < need {
            bail!(
                "帧 {index} 解压后数据不足：需要 {need} 字节（stride={stride}×h={}），实际 {}",
                f.info.height,
                dec.len()
            );
        }
        Ok(dec[..need].to_vec())
    }

    /// 数据字节（供诊断）。
    pub fn data_len(&self) -> usize {
        self.data.len()
    }
}

/// 解析单帧块头（不触碰 zlib 载荷）。
fn parse_frame(data: &[u8], off: u32) -> WzlFrame {
    let empty = |o: u32| WzlFrame {
        info: FrameInfo {
            offset: o,
            width: 0,
            height: 0,
            px: 0,
            py: 0,
            kind: FrameKind::Empty,
            valid: false,
        },
        color_count: 0,
        payload_off: 0,
        payload_len: 0,
    };
    // 空帧哨兵：偏移 < 64（实测 0 或 48）指向头部/零块。
    if off < WZL_DATA_START {
        return empty(off);
    }
    let o = off as usize;
    if o + WZL_BLOCK_HEADER_LEN > data.len() {
        return empty(off);
    }
    let marker = u16::from_le_bytes([data[o], data[o + 1]]);
    let w = u16::from_le_bytes([data[o + 4], data[o + 5]]) as u32;
    let h = u16::from_le_bytes([data[o + 6], data[o + 7]]) as u32;
    let px = i16::from_le_bytes([data[o + 8], data[o + 9]]);
    let py = i16::from_le_bytes([data[o + 10], data[o + 11]]);
    let csize = u32::from_le_bytes(data[o + 12..o + 16].try_into().unwrap()) as usize;

    // 位深由标记低字节决定（实测：0x03=8bit；0x05=16bit，含 0x0105/0x0505/0x0905）。
    let color_count = match marker & 0xFF {
        0x03 => 0x100,
        0x05 => COLOR_COUNT_16BIT,
        _ => return empty(off), // 0x00（空）/ 0x06（24bit，未支持）
    };

    let payload_off = o + WZL_BLOCK_HEADER_LEN;
    // 载荷越界检查：csize==0 时为未压缩裸数据，需 `width_bytes(bits,w)*h` 字节。
    let bits = if color_count == COLOR_COUNT_16BIT { 16 } else { 8 };
    let raw_need = width_bytes(bits, w) as usize * h as usize;
    let payload_ok = if csize == 0 {
        payload_off + raw_need <= data.len()
    } else {
        payload_off + csize <= data.len()
    };
    let valid = w >= MIN_WZL_IMAGE_SIZE
        && w <= MAX_WZL_IMAGE_SIZE
        && h >= MIN_WZL_IMAGE_SIZE
        && h <= MAX_WZL_IMAGE_SIZE
        && payload_ok;

    WzlFrame {
        info: FrameInfo {
            offset: off,
            width: w,
            height: h,
            px,
            py,
            kind: if valid {
                if csize == 0 {
                    FrameKind::Raw
                } else {
                    FrameKind::Zlib
                }
            } else {
                FrameKind::Empty
            },
            valid,
        },
        color_count: if valid { color_count } else { 0 },
        payload_off,
        payload_len: csize,
    }
}
