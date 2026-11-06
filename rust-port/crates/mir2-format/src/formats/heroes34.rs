//! 格式 `Heroes34` —— 3/4 Heroes 图（Myth / Lifcos），**15 字节/格**，C# `LoadMapType7`。
//!
//! # 一、文件头（54 字节）
//!
//! | 偏移 | 字段 | 类型 | 说明 | 源码行号 |
//! |------|------|------|------|----------|
//! | 21 | `Width` | `i16` | 宽 | `:517` |
//! | 23 | — | `[u8; 2]` | C# `:518` 的 `offset += 4` 把它一起跳过 | — |
//! | 25 | `Height` | `i16` | 高 | `:519` |
//!
//! `0..21` 与 `27..54` 的字节本格式不解释，原样进 `header_raw`。
//! C# 的 `ValidateDimensions(54, 15)` 在 `:520`。
//!
//! # 二、单元格（15 字节）
//!
//! | 格内偏移 | 字段 | 类型 | 磁盘变换 | 源码行号 |
//! |---------|------|------|----------|----------|
//! | 0 | 背景图号 | `u32` | **无 XOR**（`ToInt32` 直接用） | `:530` |
//! | 4 | 中间图号 | `u16` | 无 | `:532` |
//! | 6 | 前景图号 | `u16` | 无 | `:533` |
//! | 8 | `btDoorIndex` | `u8` | 无 | `:534` |
//! | 9 | `btDoorOffset` | `u8` | 无 | `:535` |
//! | 10 | `btAniFrame` | `u8` | 无 | `:536` |
//! | 11 | `btAniTick` | `u8` | 无 | `:537` |
//! | 12 | `btArea` / 前景库号 | `u8` | 前景库号 = `+2` | `:538` |
//! | 13 | `btLight` | `u8` | 无 | `:540` |
//! | 14 | — | `u8` | 未使用（C# `:542` 与 `:551` 各 `offset++` 一次） | — |
//!
//! 合计 15 字节，尾区 1 字节。
//!
//! ⚠ C# `:539` 把 `HasFront` **无条件设成 `true`**（不判 `wFrImg`）——这与前六种格式不同，
//! 属于该支的特殊写法。本 crate 不做这个推导（它不影响字节），只把语义记录在
//! [`csharp_has_front`] 里。
//!
//! # 三、存储顺序：**列优先**
//!
//! ```text
//! offset = 54 + (x * h + y) * 15      // x 外层、y 内层（C# `:524-525`）
//! ```
//!
//! # 四、真实样本
//!
//! **无**（本机 1158 个真实 `.map` 里 0 个命中本支）⇒ 只有合成样本验证。
//!
//! # 五、判别
//!
//! C# `:108`：`Bytes[0]==0x0D && Bytes[1]==0x4C && Bytes[7]==0x20 && Bytes[11]==0x6D`。

use crate::error::FormatError;
use crate::model::{
    check_tail, i16_at, put_u16, put_u32, read_fixed, u16_at, u32_at, validate_dims, write_fixed,
    Cell, CollisionPolarity, FormatCaps, FormatId, MapFormat, RawMap,
};

/// 文件头字节数。
pub const HEADER_LEN: usize = 54;

/// 单格字节数。
pub const CELL_LEN: usize = 15;

/// 尾区字节数（格内偏移 14 的 1 个未使用字节）。
pub const TAIL_LEN: usize = 1;

/// 格式单例。
#[derive(Debug, Clone, Copy, Default)]
pub struct Heroes34;

impl Heroes34 {
    fn header_dims(bytes: &[u8]) -> Option<(i32, i32)> {
        if bytes.len() < HEADER_LEN {
            return None;
        }
        Some((i32::from(i16_at(bytes, 21)), i32::from(i16_at(bytes, 25))))
    }
}

impl MapFormat for Heroes34 {
    fn id(&self) -> FormatId {
        FormatId::Heroes34
    }

    fn name(&self) -> &'static str {
        "Heroes34（15B/格，3/4 Heroes Myth/Lifcos）"
    }

    fn detect(&self, bytes: &[u8]) -> bool {
        self.header_signature(bytes)
            && match self.dims(bytes) {
                Some((w, h)) => {
                    (bytes.len() as u64)
                        == HEADER_LEN as u64 + w as u64 * h as u64 * CELL_LEN as u64
                }
                None => false,
            }
    }

    fn header_signature(&self, bytes: &[u8]) -> bool {
        bytes.len() >= 12
            && bytes[0] == 0x0D
            && bytes[1] == 0x4C
            && bytes[7] == 0x20
            && bytes[11] == 0x6D
    }

    fn dims(&self, bytes: &[u8]) -> Option<(u16, u16)> {
        let (w, h) = Self::header_dims(bytes)?;
        if w > 0 && h > 0 {
            Some((w as u16, h as u16))
        } else {
            None
        }
    }

    fn read(&self, bytes: &[u8]) -> Result<RawMap, FormatError> {
        if bytes.len() < HEADER_LEN {
            return Err(FormatError::TooShort {
                format: FormatId::Heroes34,
                len: bytes.len(),
                need: HEADER_LEN,
            });
        }
        let (w, h) = Self::header_dims(bytes).ok_or(FormatError::InvalidDimensions {
            format: FormatId::Heroes34,
            width: 0,
            height: 0,
        })?;
        let (width, height) = validate_dims(FormatId::Heroes34, w, h)?;
        read_fixed(
            FormatId::Heroes34,
            bytes,
            HEADER_LEN,
            CELL_LEN,
            width,
            height,
            |c| {
                Ok(Cell {
                    bk_img: u32_at(c, 0),
                    mid_img: u16_at(c, 4),
                    fr_img: u16_at(c, 6),
                    door: c[8],
                    door_offset: c[9],
                    ani_frame: c[10],
                    ani_tick: c[11],
                    area: c[12],
                    light: c[13],
                    tail: vec![c[14]],
                    ..Cell::default()
                })
            },
        )
    }

    fn write(&self, map: &RawMap) -> Result<Vec<u8>, FormatError> {
        write_fixed(
            FormatId::Heroes34,
            map,
            HEADER_LEN,
            CELL_LEN,
            |cell, out| {
                check_tail(cell, TAIL_LEN, FormatId::Heroes34)?;
                put_u32(out, cell.bk_img);
                put_u16(out, cell.mid_img);
                put_u16(out, cell.fr_img);
                out.push(cell.door);
                out.push(cell.door_offset);
                out.push(cell.ani_frame);
                out.push(cell.ani_tick);
                out.push(cell.area);
                out.push(cell.light);
                out.push(cell.tail[0]);
                Ok(())
            },
        )
    }

    fn caps(&self) -> FormatCaps {
        FormatCaps {
            can_write: true,
            // 2026-11-05（P1-9）复核后**保持 `false`**：两条判据都不满足。
            //   判据 1：格内 rel 14 那 1 字节未建模（`TAIL_LEN = 1`，本模块文档第二节）；
            //           且 `bk_img` 是 32 位（`read` 里 `u32_at(c, 0)`），编辑视图只承载低 16 位
            //           （`convert.rs` 的 `FIELD_MAP` 对 `bk_img` 用 `MapKind::Low16`）⇒ 高 16 位改不到；
            //   判据 2：本机 0 张真实样本（本模块文档第四节）。
            editable: false,
            editable_caveat: "每格 1 字节未建模（格内 rel 14，C# `:542` 注释 `Skip the unused format byte`，编辑时原样保留）；且背景图号是 32 位、编辑视图只承载低 16 位 ⇒ 高 16 位改不到；另本机 0 张真实样本（布局仅据 C# 源码 LegacyMapReader.cs:512-559）",
            ground_paint_supported: true,
            has_lib_bytes: false,
            fixed_cell_len: Some(CELL_LEN),
            has_tail: true,
            has_real_samples: false,
            collision_note: "背景图号（32 位）的 0x8000 = 阻挡位（C# `:543-544` 同 Old12 的处理）；前景带 0x8000 时另打 0x40000000",
            collision_polarity: CollisionPolarity::Bit15SetMeansBlocked,
            source: "CrystalStudio/LegacyMapReader.cs:512-559（LoadMapType7）",
        }
    }

    fn header_len(&self) -> usize {
        HEADER_LEN
    }

    fn expected_len(&self, width: u16, height: u16) -> Option<u64> {
        Some(HEADER_LEN as u64 + width as u64 * height as u64 * CELL_LEN as u64)
    }
}

/// C# `:538`：前景库号 = `btArea + 2`。
pub fn csharp_front_lib(area: u8) -> u16 {
    u16::from(area) + 2
}

/// C# `:539`：本支把 `HasFront` **无条件设为 `true`**（不判 `wFrImg`）。
///
/// 返回值恒为 `true`，写成函数只为把这条源码事实显式记录下来，
/// 免得上层误以为「`fr_img == 0` 就是没前景」。
pub fn csharp_has_front() -> bool {
    true
}

/// C# `:543-544`：背景图号带 `0x8000` ⇒ 清位并打渲染层标记 `0x20000000`。
pub fn csharp_back_image(raw: u32) -> u32 {
    if raw & 0x8000 != 0 {
        (raw & 0x7FFF) | 0x2000_0000
    } else {
        raw
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 按源码手工构造一张 `w×h` 的 Heroes34 字节流（**无真实样本**）。
    fn 造图(w: u16, h: u16) -> Vec<u8> {
        let mut b = vec![0u8; HEADER_LEN + CELL_LEN * w as usize * h as usize];
        for (i, x) in b.iter_mut().enumerate() {
            *x = (i as u8).wrapping_mul(31).wrapping_add(1);
        }
        b[0] = 0x0D;
        b[1] = 0x4C;
        b[7] = 0x20;
        b[11] = 0x6D;
        b[21..23].copy_from_slice(&(w as i16).to_le_bytes());
        b[25..27].copy_from_slice(&(h as i16).to_le_bytes());
        b
    }

    #[test]
    fn 合成样本往返逐字节一致() {
        let raw = 造图(3, 5);
        let m = Heroes34.read(&raw).expect("应解析成功");
        assert_eq!((m.width, m.height), (3, 5));
        assert_eq!(m.cells.len(), 15);
        assert_eq!(Heroes34.write(&m).expect("应写回"), raw);
    }

    #[test]
    fn 背景图号是三十二位且无异或() {
        let mut raw = 造图(1, 1);
        raw[HEADER_LEN..HEADER_LEN + 4].copy_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
        let m = Heroes34.read(&raw).unwrap();
        assert_eq!(m.cell(0, 0).unwrap().bk_img, 0xDEAD_BEEF);
        assert_eq!(Heroes34.write(&m).unwrap(), raw);
    }

    #[test]
    fn 变换对齐源码() {
        assert_eq!(csharp_front_lib(0), 2);
        assert!(csharp_has_front());
        assert_eq!(csharp_back_image(0x0000_8001), 0x2000_0001);
        assert_eq!(csharp_back_image(0x0000_0001), 1);
        // 32 位高位（bit31）不是阻挡位，原样保留
        assert_eq!(csharp_back_image(0x8000_0000), 0x8000_0000);
    }
}
