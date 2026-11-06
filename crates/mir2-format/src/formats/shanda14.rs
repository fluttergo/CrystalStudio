//! 格式 `Shanda14` —— **14 字节/格**（`TNewMapInfo`），C# `LoadMapType2`。
//!
//! # 一、文件头（52 字节，`TMapHeader`）
//!
//! 与 [`crate::formats::old12`] **完全相同**：`wWidth` `i16@0`、`wHeight` `i16@2`
//! （C# `LegacyMapReader.cs:226`、`:228`）。C# 的 `ValidateDimensions(52, 14)` 在 `:229`。
//!
//! # 二、单元格（14 字节，`TNewMapInfo`）
//!
//! | 格内偏移 | 字段 | 类型 | 说明 | 源码行号 |
//! |---------|------|------|------|----------|
//! | 0 | `wBkImg` | `u16` | 背景图号 | `:235` |
//! | 2 | `wMidImg` | `u16` | 中间图号 | `:237` |
//! | 4 | `wFrImg` | `u16` | 前景图号 | `:239` |
//! | 6 | `btDoorIndex` | `u8` | 门索引 | `:241` |
//! | 7 | `btDoorOffset` | `u8` | 门偏移 | `:242` |
//! | 8 | `btAniFrame` | `u8` | 前景动画帧 | `:243` |
//! | 9 | `btAniTick` | `u8` | 前景动画节拍 | `:244` |
//! | 10 | `btArea` | `u8` | 区域 / 前景库号 | `:245` |
//! | 11 | `btLight` | `u8` | 光照 | `:246` |
//! | 12 | `btTiles` | `u8` | **背景库号** | `:247` |
//! | 13 | `btsmTiles` | `u8` | **中间层库号** | `:248` |
//!
//! 合计 14 字节，**无尾区**。
//!
//! # 三、⚠ 第 12/13 字节的**命名冲突**（已查实，两种说法读的是同一批字节）
//!
//! | 来源 | 第 12 字节 | 第 13 字节 |
//! |------|-----------|-----------|
//! | `client/reference_full/Classes/map/TMap.h`（`TNewMapInfo`） | `byte btTiles` | `byte btsmTiles` |
//! | C# `LegacyMapReader.cs:247-248` | `BackIndex = Bytes[offset++] + 100` | `MiddleIndex = Bytes[offset++] + 110` |
//!
//! ⇒ 两者**不矛盾**：同一个字节，客户端当「瓦片库号」，C# 当「图层库号 + 100/110 偏移」。
//! 本 crate 存**原样字节**，放在 [`crate::Cell::tiles`] / [`crate::Cell::sm_tiles`]
//! （沿用任务书的字段名），并额外提供 [`csharp_back_lib`] / [`csharp_middle_lib`]
//! 给出 C# 的 `+100/+110` 解释。
//!
//! # 四、存储顺序：**列优先**
//!
//! ```text
//! offset = 52 + (x * h + y) * 14      // x 外层、y 内层（C# `:231-232`）
//! ```
//!
//! # 五、C# 的变换
//!
//! | 变换 | C# 源码 | 本模块函数 |
//! |------|---------|-----------|
//! | 前景库号 = `btArea + 120` | `:245` | [`csharp_front_lib`] |
//! | 背景库号 = `btTiles + 100` | `:247` | [`csharp_back_lib`] |
//! | 中间层库号 = `btsmTiles + 110` | `:248` | [`csharp_middle_lib`] |
//! | 背景带 `0x8000` ⇒ `(v & 0x7FFF) \| 0x20000000` | `:249-250` | [`csharp_back_image`] |
//! | 有前景 ⇔ `(wFrImg & 0x7FFF) > 0 && (btArea+120) > 0` | `:253` | [`csharp_has_front`] |
//!
//! # 六、判别为什么不能只看标题
//!
//! C# 用标题字节判这一支：`Bytes[4] == 0x0F && Bytes[18] == 0x0D && Bytes[19] == 0x0A`
//! （`:91`）。实测两个方向都会错：
//!
//! - **漏判**：`GodotClient/maps/10.map`（110×118，`52+14*12980 = 181772` 精确成立）
//!   的标题首字节是 `0x07` 而不是 `0x0F` ⇒ C# 落到 `LoadMapType0` 按 12 B/格读，读成花图；
//! - **误判**：`client/热血传奇23周年客户端/Map/hell107.map`（250×250，`52+12*62500 = 750052`）
//!   标题恰好是 `0x0F…0x0D0A`，但长度是 12 B/格 ⇒ C# 按 14 B/格读，越界读穿。
//!
//! ⇒ [`Shanda14::detect`] = C# 标题谓词 **且** 长度精确等于 `52 + 14*w*h`。

use crate::error::FormatError;
use crate::model::{
    check_tail, i16_at, put_u16, read_fixed, tmap_dims, u16_at, validate_dims, write_fixed, Cell,
    CollisionPolarity, FormatCaps, FormatId, MapFormat, RawMap,
};

/// 文件头字节数（`TMapHeader`）。
pub const HEADER_LEN: usize = 52;

/// 单格字节数（`TNewMapInfo`）。
pub const CELL_LEN: usize = 14;

/// 尾区字节数（本格式没有尾区）。
pub const TAIL_LEN: usize = 0;

/// 阻挡位。
pub const BLOCK_BIT: u16 = 0x8000;

/// 前景库号偏移（C# `:245` 的 `+120`）。
pub const FRONT_LIB_OFFSET: u16 = 120;

/// 背景库号偏移（C# `:247` 的 `+100`）。
pub const BACK_LIB_OFFSET: u16 = 100;

/// 中间层库号偏移（C# `:248` 的 `+110`）。
pub const MIDDLE_LIB_OFFSET: u16 = 110;

/// 格式单例。
#[derive(Debug, Clone, Copy, Default)]
pub struct Shanda14;

impl MapFormat for Shanda14 {
    fn id(&self) -> FormatId {
        FormatId::Shanda14
    }

    fn name(&self) -> &'static str {
        "Shanda14（14B/格，TNewMapInfo）"
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

    /// C# `:91`：`Bytes[4] == 0x0F && Bytes[18] == 0x0D && Bytes[19] == 0x0A`
    /// （标题第 0 字节 `0x0F`、第 14/15 字节 `\r\n`）。
    ///
    /// ⚠ `Shanda2012` 与本格式**共用**这条谓词，两者只能靠文件长度区分（C# `:95` 同此思路）。
    fn header_signature(&self, bytes: &[u8]) -> bool {
        bytes.len() >= 20 && bytes[4] == 0x0F && bytes[18] == 0x0D && bytes[19] == 0x0A
    }

    fn dims(&self, bytes: &[u8]) -> Option<(u16, u16)> {
        tmap_dims(bytes)
    }

    fn read(&self, bytes: &[u8]) -> Result<RawMap, FormatError> {
        if bytes.len() < HEADER_LEN {
            return Err(FormatError::TooShort {
                format: FormatId::Shanda14,
                len: bytes.len(),
                need: HEADER_LEN,
            });
        }
        let (w, h) = tmap_dims(bytes).ok_or(FormatError::InvalidDimensions {
            format: FormatId::Shanda14,
            width: i16_at(bytes, 0) as i32,
            height: i16_at(bytes, 2) as i32,
        })?;
        let (w, h) = validate_dims(FormatId::Shanda14, w as i32, h as i32)?;
        read_fixed(FormatId::Shanda14, bytes, HEADER_LEN, CELL_LEN, w, h, |c| {
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
                tiles: u16::from(c[12]),
                sm_tiles: u16::from(c[13]),
                tail: Vec::new(),
                ..Cell::default()
            })
        })
    }

    fn write(&self, map: &RawMap) -> Result<Vec<u8>, FormatError> {
        write_fixed(
            FormatId::Shanda14,
            map,
            HEADER_LEN,
            CELL_LEN,
            |cell, out| {
                check_tail(cell, TAIL_LEN, FormatId::Shanda14)?;
                if cell.bk_img > u32::from(u16::MAX) {
                    return Err(FormatError::inconsistent(format!(
                        "Shanda14 的背景图号只有 16 位，收到 {}",
                        cell.bk_img
                    )));
                }
                for (name, v) in [("tiles", cell.tiles), ("sm_tiles", cell.sm_tiles)] {
                    if v > u16::from(u8::MAX) {
                        return Err(FormatError::inconsistent(format!(
                            "Shanda14 的 {name} 只有 8 位，收到 {v}"
                        )));
                    }
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
                out.push(cell.tiles as u8);
                out.push(cell.sm_tiles as u8);
                Ok(())
            },
        )
    }

    fn caps(&self) -> FormatCaps {
        FormatCaps {
            can_write: true,
            // 2026-11-05（P1-9）复核后**保持 `true`**：格内 14 字节全部有名字
            // （本模块文档第二节，逐条带 `LegacyMapReader.cs` 行号）、`tail` 恒空、
            // `unmodeled_fields(Shanda14)` 为空、有真实样本（`41.map` 等）⇒ 两条判据都满足。
            editable: true,
            // 无保留（契约：`editable == true` ⟺ 本字段是空串）。
            editable_caveat: "",
            ground_paint_supported: true,
            has_lib_bytes: true,
            fixed_cell_len: Some(CELL_LEN),
            has_tail: false,
            has_real_samples: true,
            collision_note: "wBkImg / wFrImg 的 0x8000 = 阻挡位（同 Old12）；C# 另用 0x20000000 / 0x40000000 作渲染层标记，不落盘",
            collision_polarity: CollisionPolarity::Bit15SetMeansBlocked,
            source: "CrystalStudio/LegacyMapReader.cs:221-270（LoadMapType2）+ TMap.h 的 TNewMapInfo",
        }
    }

    fn header_len(&self) -> usize {
        HEADER_LEN
    }

    fn expected_len(&self, width: u16, height: u16) -> Option<u64> {
        Some(HEADER_LEN as u64 + width as u64 * height as u64 * CELL_LEN as u64)
    }
}

/// C# `:245`：前景库号 = `btArea + 120`。
pub fn csharp_front_lib(area: u8) -> u16 {
    u16::from(area) + FRONT_LIB_OFFSET
}

/// C# `:247`：背景库号 = `btTiles + 100`。
pub fn csharp_back_lib(tiles: u8) -> u16 {
    u16::from(tiles) + BACK_LIB_OFFSET
}

/// C# `:248`：中间层库号 = `btsmTiles + 110`。
pub fn csharp_middle_lib(sm_tiles: u8) -> u16 {
    u16::from(sm_tiles) + MIDDLE_LIB_OFFSET
}

/// C# `:249-250`：背景图号带 `0x8000` ⇒ 清位并打渲染层标记 `0x20000000`。
pub fn csharp_back_image(raw: u16) -> u32 {
    if raw & BLOCK_BIT != 0 {
        u32::from(raw & 0x7FFF) | 0x2000_0000
    } else {
        u32::from(raw)
    }
}

/// C# `:253`：有前景 ⇔ `(wFrImg & 0x7FFF) > 0 && (btArea + 120) > 0`
/// （后者恒真，因为 `btArea + 120 >= 120`）。
pub fn csharp_has_front(fr_img: u16) -> bool {
    (fr_img & 0x7FFF) > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一张 `w×h` 的 14 B/格原始字节，标题按 C# 签名填。
    fn 造图(w: u16, h: u16, 带签名: bool) -> Vec<u8> {
        let mut b = vec![0u8; HEADER_LEN + CELL_LEN * w as usize * h as usize];
        for (i, x) in b.iter_mut().enumerate() {
            *x = (i as u8).wrapping_mul(53).wrapping_add(3);
        }
        b[0..2].copy_from_slice(&w.to_le_bytes());
        b[2..4].copy_from_slice(&h.to_le_bytes());
        b[4] = if 带签名 { 0x0F } else { 0x07 };
        b[18] = 0x0D;
        b[19] = 0x0A;
        b
    }

    #[test]
    fn 往返逐字节一致且字段落位正确() {
        let raw = 造图(4, 3, true);
        let m = Shanda14.read(&raw).expect("应解析成功");
        assert_eq!((m.width, m.height), (4, 3));
        assert_eq!(m.cells.len(), 12);
        // 列优先 (x=3,y=2) → idx = 3*3+2 = 11
        let base = HEADER_LEN + 11 * CELL_LEN;
        let c = m.cell(3, 2).expect("(3,2) 应在图内");
        assert_eq!(c.tiles, u16::from(raw[base + 12]), "第 12 字节应进 tiles");
        assert_eq!(
            c.sm_tiles,
            u16::from(raw[base + 13]),
            "第 13 字节应进 sm_tiles"
        );
        assert_eq!(c.area, raw[base + 10]);
        assert_eq!(c.light, raw[base + 11]);
        assert_eq!(Shanda14.write(&m).expect("应写回成功"), raw);
    }

    #[test]
    fn 库号偏移对齐源码() {
        assert_eq!(csharp_front_lib(0), 120);
        assert_eq!(csharp_front_lib(1), 121);
        assert_eq!(csharp_back_lib(0), 100);
        assert_eq!(csharp_middle_lib(0), 110);
        assert_eq!(csharp_back_image(0x8000), 0x2000_0000);
    }

    #[test]
    fn 标题不符时不自认() {
        let raw = 造图(4, 3, false);
        assert!(!Shanda14.detect(&raw), "标题首字节 0x07 不该自认 Shanda14");
        assert!(!Shanda14.header_signature(&raw));
    }
}
