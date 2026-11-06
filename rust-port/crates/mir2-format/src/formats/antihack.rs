//! 格式 `AntiHack` —— WeMade 反外挂图（laby 图，标题 `Mir2 AntiHack`），**12 字节/格**，C# `LoadMapType4`。
//!
//! # 一、文件头（64 字节）
//!
//! | 偏移 | 字段 | 类型 | 说明 | 源码行号 |
//! |------|------|------|------|----------|
//! | 31 | `w` | `i16` | **异或前**的宽 | `:333` |
//! | 33 | `xor` | `i16` | 宽高共用的 XOR 掩码 | `:335` |
//! | 35 | `h` | `i16` | **异或前**的高 | `:337` |
//!
//! `Width = w ^ xor`、`Height = h ^ xor`（`:338-339`）。
//! `0..31` 与 `37..64` 的字节本格式不解释，原样进 `header_raw`。
//! C# 的 `ValidateDimensions(64, 12)` 在 `:340`。
//!
//! # 二、单元格（12 字节）
//!
//! | 格内偏移 | 字段 | 类型 | 磁盘变换 | 源码行号 |
//! |---------|------|------|----------|----------|
//! | 0 | 背景图号 | `u16` | **XOR `xor`** | `:348` |
//! | 2 | 中间图号 | `u16` | **XOR `xor`** | `:350` |
//! | 4 | 前景图号 | `u16` | **XOR `xor`** | `:352` |
//! | 6 | `btDoorIndex` | `u8` | — | `:354` |
//! | 7 | `btDoorOffset` | `u8` | — | `:355` |
//! | 8 | `btAniFrame` | `u8` | — | `:356` |
//! | 9 | `btAniTick` | `u8` | — | `:357` |
//! | 10 | `btArea` | `u8` | — | `:358` |
//! | 11 | `btLight` | `u8` | — | `:359` |
//!
//! 合计 12 字节，**无尾区**。
//!
//! # 三、存储顺序：**列优先**
//!
//! ```text
//! offset = 64 + (x * h + y) * 12      // x 外层、y 内层（C# `:342-343`）
//! ```
//!
//! # 四、C# 的变换
//!
//! | 变换 | C# 源码 | 本模块函数 |
//! |------|---------|-----------|
//! | 背景 / 中间 / 前景图号 `^ xor` | `:348`/`:350`/`:352` | [`csharp_image`] |
//! | 前景库号 = `btArea + 2` | `:358` | [`csharp_front_lib`] |
//! | 有前景 ⇔ `(wFrImg & 0x7FFF) > 0 && (btArea+2) > 0`（后者恒真） | `:364` | [`csharp_has_front`] |
//! | 背景带 `0x8000` ⇒ `(v & 0x7FFF) \| 0x20000000` | `:360-361` | [`csharp_back_image`] |
//!
//! ⚠ **入参契约**：[`csharp_back_image`] 吃的是 **XOR 还原后**的值 —— C# `:348` 先
//! `^ xor` 还原，`:360` 才判 `& 0x8000`。磁盘原值必须先过 [`csharp_image`]；
//! 手边只有磁盘值时请直接调 [`csharp_back_image_from_disk`]，别自己拼。
//!
//! # 五、真实样本
//!
//! **无**。本机 1158 个真实 `.map` 里 0 个命中本支 ⇒ 只有合成样本验证。
//!
//! # 六、判别
//!
//! C# `:79`：`Bytes[0]==0x15 && Bytes[4]==0x32 && Bytes[6]==0x41 && Bytes[19]==0x31`。

use crate::error::FormatError;
use crate::model::{
    check_tail, i16_at, put_u16, read_fixed, u16_at, validate_dims, write_fixed, Cell,
    CollisionPolarity, FormatCaps, FormatId, MapFormat, RawMap,
};

/// 文件头字节数。
pub const HEADER_LEN: usize = 64;

/// 单格字节数。
pub const CELL_LEN: usize = 12;

/// 尾区字节数（无）。
pub const TAIL_LEN: usize = 0;

/// 宽/高/图号共用的 XOR 掩码在头内的偏移（C# `:335`）。
pub const XOR_OFFSET: usize = 33;

/// 格式单例。
#[derive(Debug, Clone, Copy, Default)]
pub struct AntiHack;

impl AntiHack {
    /// 从头部读出 `(w, xor, h)`；不足 37 字节返回 `None`。
    fn header_triplet(bytes: &[u8]) -> Option<(i32, i32, i32)> {
        if bytes.len() < 37 {
            return None;
        }
        Some((
            i32::from(i16_at(bytes, 31)),
            i32::from(i16_at(bytes, XOR_OFFSET)),
            i32::from(i16_at(bytes, 35)),
        ))
    }
}

impl MapFormat for AntiHack {
    fn id(&self) -> FormatId {
        FormatId::AntiHack
    }

    fn name(&self) -> &'static str {
        "AntiHack（12B/格，WeMade 反外挂 laby 图）"
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
        bytes.len() >= 20
            && bytes[0] == 0x15
            && bytes[4] == 0x32
            && bytes[6] == 0x41
            && bytes[19] == 0x31
    }

    fn dims(&self, bytes: &[u8]) -> Option<(u16, u16)> {
        let (w, xor, h) = Self::header_triplet(bytes)?;
        let width = w ^ xor;
        let height = h ^ xor;
        if width > 0 && height > 0 {
            Some((width as u16, height as u16))
        } else {
            None
        }
    }

    fn read(&self, bytes: &[u8]) -> Result<RawMap, FormatError> {
        if bytes.len() < HEADER_LEN {
            return Err(FormatError::TooShort {
                format: FormatId::AntiHack,
                len: bytes.len(),
                need: HEADER_LEN,
            });
        }
        let (w, xor, h) = Self::header_triplet(bytes).ok_or(FormatError::InvalidDimensions {
            format: FormatId::AntiHack,
            width: 0,
            height: 0,
        })?;
        let (width, height) = validate_dims(FormatId::AntiHack, w ^ xor, h ^ xor)?;
        read_fixed(
            FormatId::AntiHack,
            bytes,
            HEADER_LEN,
            CELL_LEN,
            width,
            height,
            |c| {
                Ok(Cell {
                    bk_img: u16_at(c, 0) as u32,
                    mid_img: u16_at(c, 2),
                    fr_img: u16_at(c, 4),
                    door: c[6],
                    door_offset: c[7],
                    ani_frame: c[8],
                    ani_tick: c[9],
                    area: c[10],
                    light: c[11],
                    tail: Vec::new(),
                    ..Cell::default()
                })
            },
        )
    }

    fn write(&self, map: &RawMap) -> Result<Vec<u8>, FormatError> {
        write_fixed(
            FormatId::AntiHack,
            map,
            HEADER_LEN,
            CELL_LEN,
            |cell, out| {
                check_tail(cell, TAIL_LEN, FormatId::AntiHack)?;
                if cell.bk_img > u32::from(u16::MAX) {
                    return Err(FormatError::inconsistent(format!(
                        "AntiHack 的背景图号只有 16 位，收到 {}",
                        cell.bk_img
                    )));
                }
                put_u16(out, cell.bk_img as u16);
                put_u16(out, cell.mid_img);
                put_u16(out, cell.fr_img);
                out.push(cell.door);
                out.push(cell.door_offset);
                out.push(cell.ani_frame);
                out.push(cell.ani_tick);
                out.push(cell.area);
                out.push(cell.light);
                Ok(())
            },
        )
    }

    fn caps(&self) -> FormatCaps {
        FormatCaps {
            can_write: true,
            // 2026-11-05（P1-9）复核后**保持 `false`**：判据 1 满足、**判据 2 不满足**。
            //   判据 1：格内 12 字节全部有名字（本模块文档第二节），`TAIL_LEN = 0`（`:67`）
            //           ⇒ **没有未建模字节**，改格不会被未知区破坏；
            //   判据 2：本机 1158 张真实 `.map` 里 **0 张**命中本支（本模块文档第五节）
            //           ⇒ 布局只有 C# 源码依据，未经真实文件验证。
            //   零样本 ⇒ 不承诺（与 `CrystalCs` 同一处置，见 `crystal_cs.rs:81-100`
            //   与 `registry.rs` 的「零样本 ⇒ 不许承诺可编辑」判据）。
            //   ⚠ 这一位与「有没有未解区」是两件事：本格式的 `false` **不是**因为语义未知。
            editable: false,
            editable_caveat: "格内 12 字节全部有名字、无未建模字节，但本机 1158 张真实 .map 里 0 张命中本格式 ⇒ 布局只有 C# 源码依据（LegacyMapReader.cs:328-380），未经真实文件验证；改格后请自行核对",
            ground_paint_supported: true,
            has_lib_bytes: false,
            fixed_cell_len: Some(CELL_LEN),
            has_tail: false,
            has_real_samples: false,
            collision_note:
                "背景图号（XOR 还原后）的 0x8000 = 阻挡位；C# 另用 0x20000000 作渲染层标记，不落盘",
            collision_polarity: CollisionPolarity::Bit15SetMeansBlocked,
            source: "CrystalStudio/LegacyMapReader.cs:328-380（LoadMapType4）",
        }
    }

    fn header_len(&self) -> usize {
        HEADER_LEN
    }

    fn expected_len(&self, width: u16, height: u16) -> Option<u64> {
        Some(HEADER_LEN as u64 + width as u64 * height as u64 * CELL_LEN as u64)
    }
}

/// C# `:348`/`:350`/`:352`：图号 = 磁盘 `u16` XOR `xor`（`xor` 取 `i16` 位模式）。
pub fn csharp_image(raw: u16, xor: i16) -> u16 {
    raw ^ (xor as u16)
}

/// C# `:358`：前景库号 = `btArea + 2`。
pub fn csharp_front_lib(area: u8) -> u16 {
    u16::from(area) + 2
}

/// C# `:364`：有前景 ⇔ `(wFrImg & 0x7FFF) > 0 && (btArea+2) > 0`（后者恒真）。
pub fn csharp_has_front(fr_img: u16) -> bool {
    (fr_img & 0x7FFF) > 0
}

/// C# `:360-361`：背景图号带 `0x8000` ⇒ 清位并打渲染层标记 `0x20000000`。
///
/// # ⚠ 入参契约：**必须传「异或还原后」的值**（不是磁盘原值）
///
/// C# 分两步，顺序不能倒（`LegacyMapReader.cs:348` → `:360`）：
///
/// 1. `:348` `MapCells.BackImage = (short)(ToInt16(Bytes, offset) ^ xor);` —— **先异或还原**；
/// 2. `:360-361` `if ((BackImage & 0x8000) != 0) …` —— 判的是**还原后**的那个值。
///
/// 因此本函数的入参**不是** [`Cell::bk_img`](crate::Cell::bk_img)（那是磁盘原值），
/// 必须先过 [`csharp_image`]。若直接喂磁盘值，`xor` 的 bit15 为 1 时阻挡位会被**判反**
/// （该走的路变阻挡、该挡的路变可走）。
///
/// 手边只有磁盘值时调 [`csharp_back_image_from_disk`]（一次做对，不用自己拼两步）。
///
/// ⚠ 与另外两个**同名**函数契约不同，接线时按模块查：
/// - `heroes34.rs::csharp_back_image`：Type7 本来就不 XOR ⇒ 入参就是磁盘值；
/// - `wemade2010.rs::csharp_back_image`：**内部**做 XOR ⇒ 入参是磁盘值。
pub fn csharp_back_image(restored: u16) -> u32 {
    // 契约钉子（C# `:348` → `:360`）：本函数**不做** XOR。
    //   入参若来自磁盘，必须已过 `csharp_image(disk, xor)`。
    //   函数体内拿不到 `xor`、无从校验，所以契约只能靠
    //   参数名 `restored` + 上面这段文档 + 本注释钉住 —— 改动前先读这三处。
    if restored & 0x8000 != 0 {
        u32::from(restored & 0x7FFF) | 0x2000_0000
    } else {
        u32::from(restored)
    }
}

/// 从**磁盘原值**出发的背景图号变换：先按 C# `:348` 异或还原，再按 `:360` 判 `0x8000`。
///
/// 等价于 `csharp_back_image(csharp_image(disk, xor))`；给「手里只有
/// [`Cell::bk_img`](crate::Cell::bk_img)」的调用方用，避免漏掉第一步。
pub fn csharp_back_image_from_disk(disk: u16, xor: i16) -> u32 {
    csharp_back_image(csharp_image(disk, xor))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 按源码手工构造一张 `w×h` 的 AntiHack 字节流（**无真实样本**）。
    ///
    /// 头里存的是**异或前**的值（`w ^ xor` / `h ^ xor`），与 C# `:338-339` 的还原式对应。
    fn 造图(w: i16, h: i16, xor: i16) -> Vec<u8> {
        let mut b = vec![0u8; HEADER_LEN + CELL_LEN * (w as usize) * (h as usize)];
        for (i, x) in b.iter_mut().enumerate() {
            *x = (i as u8).wrapping_mul(23).wrapping_add(9);
        }
        b[0] = 0x15;
        b[4] = 0x32;
        b[6] = 0x41;
        b[19] = 0x31;
        b[31..33].copy_from_slice(&(w ^ xor).to_le_bytes());
        b[33..35].copy_from_slice(&xor.to_le_bytes());
        b[35..37].copy_from_slice(&(h ^ xor).to_le_bytes());
        b
    }

    #[test]
    fn 合成样本往返逐字节一致() {
        let raw = 造图(5, 4, 0x0303);
        let m = AntiHack.read(&raw).expect("应解析成功");
        assert_eq!((m.width, m.height), (5, 4));
        assert_eq!(m.cells.len(), 20);
        assert_eq!(AntiHack.write(&m).expect("应写回"), raw);
    }

    #[test]
    fn 头部六十四字节且宽高异或还原() {
        let raw = 造图(0x20, 0x11, 0x07);
        assert_eq!(u16_at(&raw, 31), 0x20 ^ 0x07);
        let m = AntiHack.read(&raw).unwrap();
        assert_eq!(m.header_raw.len(), HEADER_LEN);
        assert_eq!(m.width, 0x20);
        assert_eq!(m.height, 0x11);
    }

    #[test]
    fn 变换对齐源码() {
        assert_eq!(csharp_image(0x1234, 0x00FF), 0x12CB);
        assert_eq!(csharp_front_lib(0), 2);
        assert!(csharp_has_front(0x7FFF));
        assert!(!csharp_has_front(0x8000));
        assert_eq!(csharp_back_image(0x8001), 0x2000_0001);
    }

    #[test]
    fn 背景图号入参必须是异或还原后的值() {
        // C# `:348` 先 `^ xor`、`:360` 才判 ⇒ 入参是**还原后**的值。
        // 取 `xor = 0x8000`（bit15 置位）：磁盘 0x0001 还原成 0x8001 ⇒ 带阻挡位。
        let xor: i16 = i16::from_le_bytes([0x00, 0x80]);
        assert_eq!(csharp_image(0x0001, xor), 0x8001, "还原后才带 bit15");
        assert_eq!(
            csharp_back_image(csharp_image(0x0001, xor)),
            0x2000_0001,
            "喂还原后的值 ⇒ 判定阻挡（C# `:360` 的真值）"
        );
        assert_eq!(
            csharp_back_image_from_disk(0x0001, xor),
            0x2000_0001,
            "从磁盘值出发的入口必须与两步写法等价"
        );
        // 反面证据：直接喂磁盘原值 0x0001 ⇒ 阻挡位被**判反**（这正是参数改名的理由）。
        assert_eq!(
            csharp_back_image(0x0001),
            0x0001,
            "直接喂磁盘值 ⇒ 丢失阻挡标记（判反）"
        );
    }
}
