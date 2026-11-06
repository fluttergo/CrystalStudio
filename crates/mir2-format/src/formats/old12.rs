//! 格式 `Old12` —— 传奇 M2 **旧版 12 字节/格**（`TMapInfo`），C# `LoadMapType0`。
//!
//! # 一、文件头（52 字节，`TMapHeader`）
//!
//! | 偏移 | 字段 | 类型 | 说明 |
//! |------|------|------|------|
//! | 0 | `wWidth` | `i16` | 宽（C# `LegacyMapReader.cs:124` `ToInt16`） |
//! | 2 | `wHeight` | `i16` | 高（`:126`） |
//! | 4 | `sTitle[16]` | — | 标题，**本格式不解释**，原样进 `header_raw` |
//! | 20 | `UpdateDate` | `f64` | 原样保留 |
//! | 28 | `Reserved[24]` | — | 原样保留 |
//!
//! 结构体定义：`client/reference_full/Classes/map/TMap.h`（`TMapHeader`，`#pragma pack(1)`）。
//!
//! # 二、单元格（12 字节，`TMapInfo`）
//!
//! | 格内偏移 | 字段 | 类型 | 说明 | 源码行号 |
//! |---------|------|------|------|----------|
//! | 0 | `wBkImg` | `u16` | 背景图号 | `:135` |
//! | 2 | `wMidImg` | `u16` | 中间图号 | `:137` |
//! | 4 | `wFrImg` | `u16` | 前景图号 | `:139` |
//! | 6 | `btDoorIndex` | `u8` | 门索引 | `:141` |
//! | 7 | `btDoorOffset` | `u8` | 门偏移 | `:142` |
//! | 8 | `btAniFrame` | `u8` | 前景动画帧 | `:143` |
//! | 9 | `btAniTick` | `u8` | 前景动画节拍 | `:144` |
//! | 10 | `btArea` | `u8` | 区域（同时是**前景库号**，见下） | `:145` |
//! | 11 | `btLight` | `u8` | 光照 | `:146` |
//!
//! 合计 12 字节，**无尾区**（`tail` 恒空）。C# 的 `ValidateDimensions(52, 12)`（`:127`）同口径。
//!
//! # 三、存储顺序：**列优先**
//!
//! ```text
//! offset = 52 + (x * h + y) * 12      // x 外层、y 内层
//! idx    = x * h + y                  // RawMap::cells 下标
//! ```
//!
//! 依据：C# `:129-130` 是 `for (x) for (y)` 双层循环，且 `offset` 在内层单调递增
//! （`TMap.cpp:163-209` 的 `offset = sizeof(TMapHeader) + nAline*i + sizeof(TMapInfo)*nTy`
//! 同样是「先按 x 跳整列、再按 y 走行」）。**本格式是列优先，不是行优先。**
//!
//! # 四、C# 的四种变换（读的时候做的，本 crate 一律不改原值，只提供函数）
//!
//! | 变换 | C# 源码 | 本模块函数 |
//! |------|---------|-----------|
//! | 背景图号带 `0x8000` ⇒ `(v & 0x7FFF) \| 0x20000000` | `:147-148` | [`csharp_back_image`] |
//! | 前景库号 = `btArea + 2` | `:145` | [`csharp_front_lib`] |
//! | 有前景 ⇔ `(wFrImg & 0x7FFF) > 0`（`frindex = btArea+2` 恒 ≥ 2，故 `frindex > 0` 恒真） | `:151` | [`csharp_has_front`] |
//! | 前景带 `0x8000` ⇒ 背景再 `\| 0x40000000` | `:158-159` | [`csharp_back_image_full`] |
//!
//! # 五、碰撞位
//!
//! `TMap.h` 对 `wBkImg` 的注释：「最高位为 1 则表示不能通过（如站着）、如河水、地表等，
//! 在判断是否可以飞过（加空气墙）时需要用到」；对 `wFrImg`：「最高位为 1 表示不能通过（建筑）」。
//! ⇒ `0x8000` = **阻挡位**，背景层与前景层各自独立。

use crate::error::FormatError;
use crate::model::{
    check_tail, i16_at, put_u16, read_fixed, tmap_dims, u16_at, validate_dims, write_fixed, Cell,
    CollisionPolarity, FormatCaps, FormatId, MapFormat, RawMap,
};

/// 文件头字节数（`TMapHeader`）。
pub const HEADER_LEN: usize = 52;

/// 单格字节数（`TMapInfo`）。
pub const CELL_LEN: usize = 12;

/// 尾区字节数（本格式没有尾区）。
pub const TAIL_LEN: usize = 0;

/// 阻挡位（`TMap.h` 对 `wBkImg` / `wFrImg` 的注释）。
pub const BLOCK_BIT: u16 = 0x8000;

/// 格式单例。
#[derive(Debug, Clone, Copy, Default)]
pub struct Old12;

impl MapFormat for Old12 {
    fn id(&self) -> FormatId {
        FormatId::Old12
    }

    fn name(&self) -> &'static str {
        "Old12（12B/格，TMapInfo 旧版）"
    }

    /// C# 的**兜底**分支（`:114-115`：「if it's none of the above load the default old school
    /// format」）⇒ 头部谓词恒真，唯一约束是长度必须正好是 `52 + 12*w*h`。
    ///
    /// 少了长度约束会把 17 个 14 B/格的真实文件（`GodotClient/maps/10.map` 等，标题首字节
    /// 是 `0x07` 而非 `0x0F`）误读成 12 B/格。
    fn detect(&self, bytes: &[u8]) -> bool {
        match self.dims(bytes) {
            Some((w, h)) => {
                (bytes.len() as u64) == HEADER_LEN as u64 + w as u64 * h as u64 * CELL_LEN as u64
            }
            None => false,
        }
    }

    fn header_signature(&self, bytes: &[u8]) -> bool {
        // 兜底分支没有头部谓词，只要求够 52 字节。
        bytes.len() >= HEADER_LEN
    }

    fn dims(&self, bytes: &[u8]) -> Option<(u16, u16)> {
        tmap_dims(bytes)
    }

    fn read(&self, bytes: &[u8]) -> Result<RawMap, FormatError> {
        if bytes.len() < HEADER_LEN {
            return Err(FormatError::TooShort {
                format: FormatId::Old12,
                len: bytes.len(),
                need: HEADER_LEN,
            });
        }
        let (w, h) = tmap_dims(bytes).ok_or(FormatError::InvalidDimensions {
            format: FormatId::Old12,
            width: i16_at(bytes, 0) as i32,
            height: i16_at(bytes, 2) as i32,
        })?;
        let (w, h) = validate_dims(FormatId::Old12, w as i32, h as i32)?;
        read_fixed(FormatId::Old12, bytes, HEADER_LEN, CELL_LEN, w, h, |c| {
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
        })
    }

    fn write(&self, map: &RawMap) -> Result<Vec<u8>, FormatError> {
        write_fixed(FormatId::Old12, map, HEADER_LEN, CELL_LEN, |cell, out| {
            check_tail(cell, TAIL_LEN, FormatId::Old12)?;
            if cell.bk_img > u32::from(u16::MAX) {
                return Err(FormatError::inconsistent(format!(
                    "Old12 的背景图号只有 16 位，收到 {}",
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
        })
    }

    fn caps(&self) -> FormatCaps {
        FormatCaps {
            can_write: true,
            // ★ 2026-11-05（P1-9）从 `false` 改 `true`：本格式**没有未建模区**，两条判据都满足。
            //   ① 格内 12 字节**全部有名字**（本模块文档第二节的字段表，逐条带 `LegacyMapReader.cs`
            //      行号：`:135`/`:137`/`:139`/`:141`/`:142`/`:143`/`:144`/`:145`/`:146`）；
            //   ② `TAIL_LEN = 0`（`old12.rs:70`），`read` 里 `tail: Vec::new()`（`:136`）⇒ 无未解字节；
            //   ③ `miredit-core/src/convert.rs` 的 `unmodeled_fields(Old12)` 为**空**
            //      （`read` 只给视图里有的字段赋值，`old12.rs:126-139`）；
            //   ④ **856 张**真实样本 `read → write` **0 字节差异**（`tests/real_samples.rs`）。
            //   旧值 `false` 的后果：`miredit-core` 给 1,051/1,158 张真实图（90.8%）挂常驻黄条，
            //   文案「未解区语义未知」对本格式是**事实错误**（P1-9）。
            editable: true,
            // 判据 1、2 都满足 ⇒ 无保留（契约：`editable == true` ⟺ 本字段是空串）。
            editable_caveat: "",
            ground_paint_supported: true,
            has_lib_bytes: false,
            fixed_cell_len: Some(CELL_LEN),
            has_tail: false,
            has_real_samples: true,
            collision_note: "背景 wBkImg 与前景 wFrImg 的最高位 0x8000 = 阻挡位（TMap.h 原注释：最高位为 1 则表示不能通过）；两层独立判定",
            collision_polarity: CollisionPolarity::Bit15SetMeansBlocked,
            source: "CrystalStudio/LegacyMapReader.cs:119-168（LoadMapType0）+ client/reference_full/Classes/map/TMap.h",
        }
    }

    fn header_len(&self) -> usize {
        HEADER_LEN
    }

    fn expected_len(&self, width: u16, height: u16) -> Option<u64> {
        Some(HEADER_LEN as u64 + width as u64 * height as u64 * CELL_LEN as u64)
    }
}

/// C# `:147-148`：背景图号最高位为 1 ⇒ 清掉该位并打上渲染层标记 `0x20000000`。
pub fn csharp_back_image(raw: u16) -> u32 {
    if raw & BLOCK_BIT != 0 {
        u32::from(raw & 0x7FFF) | 0x2000_0000
    } else {
        u32::from(raw)
    }
}

/// C# `:145`：前景库号 = `btArea + 2`（`FrontIndex = (short)(Bytes[offset++] + 2)`）。
pub fn csharp_front_lib(area: u8) -> u16 {
    u16::from(area) + 2
}

/// C# `:151`：有前景 ⇔ `(wFrImg & 0x7FFF) > 0 && frindex > 0`。
///
/// `frindex = btArea + 2` 恒 ≥ 2 ⇒ 后半条件恒真，等价于只看 `wFrImg & 0x7FFF`。
pub fn csharp_has_front(fr_img: u16) -> bool {
    (fr_img & 0x7FFF) > 0
}

/// C# `:147-159` 的**完整**背景图号变换：在 [`csharp_back_image`] 之上，
/// 若「有前景且前景带 `0x8000`」再或上 `0x40000000`。
pub fn csharp_back_image_full(bk_img: u16, fr_img: u16) -> u32 {
    let mut v = csharp_back_image(bk_img);
    if csharp_has_front(fr_img) && (fr_img & BLOCK_BIT) != 0 {
        v |= 0x4000_0000;
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一张 `w×h` 的 Old12 原始字节（头部 52 字节填非零，格内容按 idx 变化）。
    fn 造图(w: u16, h: u16) -> Vec<u8> {
        let mut b = vec![0u8; HEADER_LEN + CELL_LEN * w as usize * h as usize];
        for (i, x) in b.iter_mut().enumerate() {
            *x = (i as u8).wrapping_mul(37).wrapping_add(11);
        }
        b[0..2].copy_from_slice(&w.to_le_bytes());
        b[2..4].copy_from_slice(&h.to_le_bytes());
        b
    }

    #[test]
    fn 往返逐字节一致且列优先() {
        let raw = 造图(5, 3);
        let m = Old12.read(&raw).expect("应解析成功");
        assert_eq!((m.width, m.height), (5, 3));
        assert_eq!(m.cells.len(), 15);
        assert_eq!(m.trailer, Vec::<u8>::new());
        // 列优先：(x=2,y=1) → idx = 2*3+1 = 7
        let idx = 2 * 3 + 1;
        let base = HEADER_LEN + idx * CELL_LEN;
        let c = m.cell(2, 1).expect("(2,1) 应在图内");
        assert_eq!(c.bk_img, u32::from(u16_at(&raw, base)));
        assert_eq!(Old12.write(&m).expect("应写回成功"), raw);
    }

    #[test]
    fn 变换函数对齐源码() {
        assert_eq!(csharp_back_image(0x8005), 0x2000_0005);
        assert_eq!(csharp_back_image(0x0005), 0x0000_0005);
        assert_eq!(csharp_front_lib(0), 2);
        assert_eq!(csharp_front_lib(255), 257);
        assert!(csharp_has_front(1));
        assert!(!csharp_has_front(0x8000));
        assert_eq!(csharp_back_image_full(0, 0x8001), 0x4000_0000);
        assert_eq!(csharp_back_image_full(0x8000, 0), 0x2000_0000);
    }

    #[test]
    fn 长度不符时不自认() {
        let mut raw = 造图(4, 4);
        raw.push(0); // 变成 52+12*16+1
        assert!(!Old12.detect(&raw), "多 1 字节就不该自认 Old12");
        // read 仍可读（多出的字节进 trailer），但 detect 的「精确长度」语义不同
        let m = Old12.read(&raw).expect("应能读出，余 1 字节进 trailer");
        assert_eq!(m.trailer, vec![0u8]);
        assert_eq!(Old12.write(&m).expect("应写回"), raw);
    }
}
