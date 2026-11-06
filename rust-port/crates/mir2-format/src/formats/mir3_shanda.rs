//! 格式 `Mir3Shanda` —— Shanda Mir3 图（标题 `(C) SNDA, MIR3`），**20 字节/格**，C# `LoadMapType6`。
//!
//! # 一、文件头（40 字节）
//!
//! | 偏移 | 字段 | 类型 | 说明 | 源码行号 |
//! |------|------|------|------|----------|
//! | 16 | `Width` | `i16` | 宽 | `:460` |
//! | 18 | `Height` | `i16` | 高 | `:462` |
//!
//! `0..16` 与 `20..40` 的字节本格式不解释，原样进 `header_raw`。
//! C# 的 `ValidateDimensions(40, 20)` 在 `:463`。
//!
//! # 二、单元格（20 字节）
//!
//! | 格内偏移 | 字段 | 类型 | 磁盘变换 | 源码行号 |
//! |---------|------|------|----------|----------|
//! | 0 | `flag` | `u8` | `bit0`/`bit1` = 背景/前景可走 | `:469`、`:494-495` |
//! | 1 | 背景库号 | `u8` | `255 ⇒ -1`，否则 `+300` | `:470` |
//! | 2 | 中间层库号 | `u8` | `255 ⇒ -1`，否则 `+300` | `:472` |
//! | 3 | `btArea` / 前景库号 | `u8` | `255 ⇒ -1`，否则 `+300` | `:474` |
//! | 4 | 背景图号 | `u16` | `+1` | `:476` |
//! | 6 | 中间图号 | `u16` | `+1` | `:478` |
//! | 8 | 前景图号 | `u16` | `+1` | `:480` |
//! | 10 | 中间层动画帧 | `u8` | — | `:484` |
//! | 11 | 前景动画帧 | `u8` | `255 ⇒ 0`；`> 0x0F` 再 `& 0x0F` | `:485-487` |
//! | 12 | `btLight` | `u8` | `& 0x0F` 再 `* 4` | `:491-492` |
//! | 13 | — | `[u8; 7]` | **跳过**（C# `:493` `offset += 8` 把 light 之后的 7 字节一起跳过） | — |
//!
//! 合计 20 字节，尾区 7 字节（偏移 13..20）。
//!
//! ⚠ C# 在 `:489-490` 把 `MiddleAnimationTick` 与 `franimationtick` **硬写成 1**
//! （不落盘、不参与判定），本 crate 不虚构这两个值。
//! ⚠ C# `:482-483` 的 `if ((frimage == 1) && (frindex == 200)) frindex = -1;` 是**死代码**：
//! `frindex` 只可能是 `-1` 或 `≥ 300`，永远不等于 200（那一支是从 Mir3Wemade 抄过来的）。
//! 本 crate 照抄语义但注明它不可达。
//!
//! # 三、存储顺序：**列优先**
//!
//! ```text
//! offset = 40 + (x * h + y) * 20      // x 外层、y 内层（C# `:465-466`）
//! ```
//!
//! # 四、真实样本
//!
//! **无**（本机 1158 个真实 `.map` 里 0 个命中本支）⇒ 只有合成样本验证。
//!
//! # 五、判别
//!
//! C# `:73`：`Bytes[0]==0x0F && Bytes[5]==0x53 && Bytes[14]==0x33`
//! （标题 `(C) SNDA, MIR3`：`\x0F` + `(C) S` … 第 5 字节 `S`、第 14 字节 `3`）。

use crate::error::FormatError;
use crate::model::{
    i16_at, put_u16, read_fixed, u16_at, validate_dims, write_fixed, Cell, CollisionPolarity,
    FormatCaps, FormatId, MapFormat, RawMap,
};

/// 文件头字节数。
pub const HEADER_LEN: usize = 40;

/// 单格字节数。
pub const CELL_LEN: usize = 20;

/// 尾区字节数（格内偏移 13..20）。
pub const TAIL_LEN: usize = 7;

/// 库号哨兵（C# `:470`/`:472`/`:474` 的 `255`）。
pub const LIB_SENTINEL: u8 = 255;

/// 库号偏移（C# `:470` 等的 `+300`）。
pub const LIB_OFFSET: i16 = 300;

/// 格式单例。
#[derive(Debug, Clone, Copy, Default)]
pub struct Mir3Shanda;

impl Mir3Shanda {
    fn header_dims(bytes: &[u8]) -> Option<(i32, i32)> {
        if bytes.len() < HEADER_LEN {
            return None;
        }
        Some((i32::from(i16_at(bytes, 16)), i32::from(i16_at(bytes, 18))))
    }
}

impl MapFormat for Mir3Shanda {
    fn id(&self) -> FormatId {
        FormatId::Mir3Shanda
    }

    fn name(&self) -> &'static str {
        "Mir3Shanda（20B/格，标题 (C) SNDA, MIR3）"
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
        bytes.len() >= 15 && bytes[0] == 0x0F && bytes[5] == 0x53 && bytes[14] == 0x33
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
                format: FormatId::Mir3Shanda,
                len: bytes.len(),
                need: HEADER_LEN,
            });
        }
        let (w, h) = Self::header_dims(bytes).ok_or(FormatError::InvalidDimensions {
            format: FormatId::Mir3Shanda,
            width: 0,
            height: 0,
        })?;
        let (width, height) = validate_dims(FormatId::Mir3Shanda, w, h)?;
        read_fixed(
            FormatId::Mir3Shanda,
            bytes,
            HEADER_LEN,
            CELL_LEN,
            width,
            height,
            |c| {
                Ok(Cell {
                    bk_img: u16_at(c, 4) as u32,
                    mid_img: u16_at(c, 6),
                    fr_img: u16_at(c, 8),
                    bk_idx: i16::from(c[1]),
                    mid_idx: i16::from(c[2]),
                    area: c[3],
                    flag: c[0],
                    mid_ani_frame: c[10],
                    ani_frame: c[11],
                    light: c[12],
                    tail: c[13..20].to_vec(),
                    ..Cell::default()
                })
            },
        )
    }

    fn write(&self, map: &RawMap) -> Result<Vec<u8>, FormatError> {
        write_fixed(
            FormatId::Mir3Shanda,
            map,
            HEADER_LEN,
            CELL_LEN,
            |cell, out| {
                if cell.tail.len() != TAIL_LEN {
                    return Err(FormatError::inconsistent(format!(
                        "Mir3Shanda 的格尾区必须是 {TAIL_LEN} 字节，实际 {} 字节",
                        cell.tail.len()
                    )));
                }
                if cell.bk_img > u32::from(u16::MAX) {
                    return Err(FormatError::inconsistent(format!(
                        "Mir3Shanda 的背景图号只有 16 位，收到 {}",
                        cell.bk_img
                    )));
                }
                for (name, v) in [("bk_idx", cell.bk_idx), ("mid_idx", cell.mid_idx)] {
                    if !(0..=255).contains(&v) {
                        return Err(FormatError::inconsistent(format!(
                            "Mir3Shanda 的 {name} 在磁盘上只有 1 字节，收到 {v}"
                        )));
                    }
                }
                out.push(cell.flag);
                out.push(cell.bk_idx as u8);
                out.push(cell.mid_idx as u8);
                out.push(cell.area);
                put_u16(out, cell.bk_img as u16);
                put_u16(out, cell.mid_img);
                put_u16(out, cell.fr_img);
                out.push(cell.mid_ani_frame);
                out.push(cell.ani_frame);
                out.push(cell.light);
                out.extend_from_slice(&cell.tail);
                Ok(())
            },
        )
    }

    fn caps(&self) -> FormatCaps {
        FormatCaps {
            can_write: true,
            // 2026-11-05（P1-9）复核后**保持 `false`**：两条判据都不满足。
            //   判据 1：格内 7 字节未建模（`TAIL_LEN = 7`，rel 13-19）—— 这是九种里**唯一**
            //           一处「大段跳过且零注释」（C# `:493` 的 `offset += 8`），规格书 §11.1 判它
            //           「中高风险：7 B 体量大，疑为光照/混合参数」⇒ 与**可编辑的**光照联动；
            //           且 `flag`（rel 0，派生碰撞/可走）、`bk_idx`、`mid_idx`、`mid_ani_frame`
            //           **不在编辑视图里**（`convert.rs` 的 `unmodeled_fields(Mir3Shanda)`）；
            //   判据 2：本机 0 张真实样本（本模块文档第四节）。
            editable: false,
            editable_caveat: "每格 7 字节未建模（格内 rel 13-19，C# `:493` 的 `offset += 8` 整段跳过且零注释，疑为光照/混合参数 ⇒ 与光照联动）；flag 字节（rel 0，派生碰撞/可走）与背景库号、中层库号、中层动画帧不在编辑视图里 ⇒ 改图号后不会跟着变；另本机 0 张真实样本",
            ground_paint_supported: true,
            has_lib_bytes: true,
            fixed_cell_len: Some(CELL_LEN),
            has_tail: true,
            has_real_samples: false,
            collision_note: "格内 flag 的 bit0 = 背景可走、bit1 = 前景可走（C# `:494-495` 是「非 1 就置阻挡位」）；C# 另把 light `& 0x0F` 再 `* 4`",
            collision_polarity: CollisionPolarity::FlagBitSetMeansWalkable,
            source: "CrystalStudio/LegacyMapReader.cs:454-510（LoadMapType6）",
        }
    }

    fn header_len(&self) -> usize {
        HEADER_LEN
    }

    fn expected_len(&self, width: u16, height: u16) -> Option<u64> {
        Some(HEADER_LEN as u64 + width as u64 * height as u64 * CELL_LEN as u64)
    }
}

/// C# `:470`/`:472`/`:474`：库号 `255 ⇒ -1`，否则 `+300`。
pub fn csharp_lib(raw: u8) -> i16 {
    if raw == LIB_SENTINEL {
        -1
    } else {
        i16::from(raw) + LIB_OFFSET
    }
}

/// C# `:476`/`:478`/`:480`：图号 `+1`。
pub fn csharp_image(raw: u16) -> u16 {
    raw.wrapping_add(1)
}

/// C# `:485-487`：前景动画帧 `255 ⇒ 0`；`> 0x0F` 再 `& 0x0F`。
pub fn csharp_front_ani_frame(raw: u8) -> u8 {
    let mut v = if raw == 255 { 0 } else { raw };
    if v > 0x0F {
        v &= 0x0F;
    }
    v
}

/// C# `:491-492`：光照 `& 0x0F` 再 `* 4`。
pub fn csharp_light(raw: u8) -> u8 {
    (raw & 0x0F).wrapping_mul(4)
}

/// C# `:494-495`：`flag` 的 bit0/bit1 **为 1 才可走**，返回 `(背景可走, 前景可走)`。
pub fn csharp_walk_bits(flag: u8) -> (bool, bool) {
    ((flag & 0x01) == 0x01, (flag & 0x02) == 0x02)
}

/// C# `:482-483` 的「`frimage == 1 && frindex == 200 ⇒ -1`」。
///
/// ⚠ **本处确为死代码**：`frindex` 只可能是 `-1` 或 `≥ 300`（C# `:474` 的 `+300`），
/// 永不等于 200。保留此函数只为把源码语义落成代码，调用它不会改变任何真实数据。
///
/// ⚠ **同一条规则在 Type5 里是活代码**（`frindex` 取值集合 `-1` 或 `200…454`，`200` 可达）
/// ⇒ 见 `mir3_wemade.rs::csharp_front_lib`。**别把本函数的「不可达」结论套到 Type5 上。**
pub fn csharp_dead_front_fix(fr_img: u16, lib: i16) -> i16 {
    if fr_img == 1 && lib == 200 {
        -1
    } else {
        lib
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 按源码手工构造一张 `w×h` 的 Mir3Shanda 字节流（**无真实样本**）。
    fn 造图(w: u16, h: u16) -> Vec<u8> {
        let mut b = vec![0u8; HEADER_LEN + CELL_LEN * w as usize * h as usize];
        for (i, x) in b.iter_mut().enumerate() {
            *x = (i as u8).wrapping_mul(19).wrapping_add(3);
        }
        b[0] = 0x0F;
        b[5] = 0x53;
        b[14] = 0x33;
        b[16..18].copy_from_slice(&(w as i16).to_le_bytes());
        b[18..20].copy_from_slice(&(h as i16).to_le_bytes());
        b
    }

    #[test]
    fn 合成样本往返逐字节一致() {
        let raw = 造图(4, 3);
        let m = Mir3Shanda.read(&raw).expect("应解析成功");
        assert_eq!((m.width, m.height), (4, 3));
        assert_eq!(m.cells.len(), 12);
        let base = HEADER_LEN + 5 * CELL_LEN; // (x=1,y=2) → idx = 1*3+2 = 5
        let c = m.cell(1, 2).expect("(1,2) 在图内");
        assert_eq!(c.flag, raw[base]);
        assert_eq!(c.bk_img, u32::from(u16_at(&raw, base + 4)));
        assert_eq!(c.tail, raw[base + 13..base + 20].to_vec());
        assert_eq!(Mir3Shanda.write(&m).expect("应写回"), raw);
    }

    #[test]
    fn 变换对齐源码() {
        assert_eq!(csharp_lib(255), -1);
        assert_eq!(csharp_lib(0), 300);
        assert_eq!(csharp_image(0xFFFF), 0);
        assert_eq!(csharp_front_ani_frame(255), 0);
        assert_eq!(csharp_front_ani_frame(0x1F), 0x0F);
        assert_eq!(csharp_front_ani_frame(0x07), 0x07);
        assert_eq!(csharp_light(0xAB), 44);
        assert_eq!(csharp_walk_bits(0b01), (true, false));
        assert_eq!(csharp_dead_front_fix(1, 200), -1);
        assert_eq!(csharp_dead_front_fix(1, 300), 300);
    }
}
