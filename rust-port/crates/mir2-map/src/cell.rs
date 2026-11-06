//! 单元格布局与格长档位（`CellFormat` / `Cell`）。
//!
//! # 一、三档格长判别式（唯一判据 = 文件长度精确整除，禁标题字节）
//!
//! | 档位 | 单格字节 | 期望文件总长 |
//! |------|----------|--------------|
//! | [`CellFormat::Old12`] | 12 | `52 + 12*w*h` |
//! | [`CellFormat::New14`] | 14 | `52 + 14*w*h` |
//! | [`CellFormat::Ext36`] | 36 | `52 + 36*w*h` |
//!
//! 判据只看 `len` 与 `w*h` 的等式，**不看任何标题/魔法字节**，也不看扩展名。
//! 三档皆不成立 ⇒ 解析必须返回 [`crate::MapError::LengthMismatch`]，
//! 错误信息里同时列出三档期望长度与实际长度。
//!
//! 边界一：`w*h == 0` —— 长度判据**本身**不区分「0 格」（此时三档期望长度都等于 52），
//! 但 [`crate::Map::parse`] 在调用本判据**之前**就因 `w == 0 || h == 0` 返回
//! [`crate::MapError::ZeroDimension`]（与客户端 `map_loader.gd:94-95`、
//! C# `LegacyMapReader.cs:50` 同口径）⇒ 「52 字节的空图」不是合法地图。
//!
//! 边界二：`w > 32767` 或 `h > 32767`（[`MAX_DIMENSION`]）—— 同样先于长度判据被拦下
//! ⇒ 得到 [`crate::MapError::DimensionTooLarge`]。真值是 C# `MapDimensions.cs:11-12@acfbbf8a`
//! 的 `CanRoundTrip`（`width is > 0 and <= short.MaxValue && …`）：`.map` 头的宽高在
//! C# 侧是 `checked((short)Width)`（`Document.cs:90`）写出的 **16 位有符号** 字段，
//! `32768..=65535` 这一段虽然塞得进 `u16`，写盘后回读却回不来 ⇒ 属于往返缺陷，不是可选优化。
//!
//! 边界三：`w*h > 16_000_000`（[`MAX_CELLS`]）—— 同样先于长度判据被拦下
//! ⇒ 超限文件得到的是 [`crate::MapError::TooManyCells`]，而不是「先分配再崩」。
//!
//! ①②③ 的**顺序**就是 C# `CanRoundTrip` 的短路顺序（0 → 32767 → 16M），
//! 由 [`validate_dimensions`] 单点实现，[`crate::Map::parse`] 只是它的调用方。
//!
//! # 二、单元格字节偏移（三档共用前 12 字节，14/36 档再带两个库号字节）
//!
//! ```text
//! 偏移  字段            类型    说明
//!   0   wBkImg         u16     背景层图号
//!   2   wMidImg         u16     中间层图号
//!   4   wFrImg          u16     前景层图号
//!   6   btDoorIndex    u8      门索引
//!   7   btDoorOffset   u8      门偏移
//!   8   btAniFrame     u8      动画帧
//!   9   btAniTick      u8      动画节拍
//!  10   btArea          u8      区域
//!  11   btLight         u8      光照
//!  12   btTiles         u8      仅 14/36 档（Old12 档无此字节）
//!  13   btsmTiles       u8      仅 14/36 档（Old12 档无此字节）
//!  14   ...             [u8;22] 仅 36 档：尾区，语义未知，原样保存原样写回
//! ```
//!
//! # 三、`Ext36` 尾区（偏移 14..36）**不可假设恒零**
//!
//! 实测（2026-11-04，本机真实客户端资源）：
//!
//! - `client/热血传奇23周年客户端/Map/n0.map`（700×700 = 490000 格）全格扫描：
//!   **1660 格**尾区存在非零字节 = **0.3388%**；
//!   其中 **1658 格**在 off18/19/20 非零，另有 2 格（`(342,275)`、`(343,275)`）
//!   只在 off25..28 非零。
//! - 非零格空间分布：`x ∈ [297..396]`、`y ∈ [199..303]`（一条斜向带，不是单点噪声）。
//! - 对照组 `n3.map` / `n4.map` / `NALX.map` 尾区全零。
//!
//! ⇒ 尾区**既不能丢、也不能重置为 0**，更不能按「零」来推断字段语义。
//! 本库只做**原样保存 + 原样写回**，不解释、不命名、不做任何归一化。

use crate::error::MapError;

/// 文件头（`TMapHeader`）字节数。
pub const HEADER_LEN: usize = 52;

/// 三档共用字段区的字节数（偏移 0..12）。
pub const COMMON_CELL_LEN: usize = 12;

/// 库号字节（`btTiles` / `btsmTiles`）在单元格内的起始偏移，仅 14/36 档有效。
pub const LIB_OFFSET: usize = COMMON_CELL_LEN;

/// 尾区在单元格内的起始偏移，仅 36 档有效。
pub const TAIL_OFFSET: usize = 14;

/// `Ext36` 档单格尾区（偏移 14..36）字节数。
pub const TAIL_LEN: usize = 22;

/// `Old12` 档单格字节数。
pub const OLD12_CELL_SIZE: usize = 12;

/// `New14` 档单格字节数。
pub const NEW14_CELL_SIZE: usize = 14;

/// `Ext36` 档单格字节数。
pub const EXT36_CELL_SIZE: usize = 36;

/// 单张地图允许的最大格数（`w * h`）—— 它只是**三条件合取中的第三条**。
///
/// 口径来源（真值）：CrystalStudio C# `MapDimensions.cs:7`（`MaximumCells = 16_000_000`）
/// 与 `MapDimensions.cs:11-12@acfbbf8a` 的 `CanRoundTrip`：
///
/// ```csharp
/// public static bool CanRoundTrip(int width, int height) => width is > 0 and <= short.MaxValue &&
///     height is > 0 and <= short.MaxValue && (long)width * height <= MaximumCells;
/// ```
///
/// 即合法尺寸 = **①`w>0` 且 `h>0` ②`w<=32767` 且 `h<=32767`（[`MAX_DIMENSION`]）
/// ③`w*h<=16_000_000`（本常量）**，三条**合取**，缺一不可。
///
/// ⚠ **旧表述「只要 `w*h <= 16_000_000` 就合法」是错的**（本次订正）：
/// `40000×1` 只有 40000 格，完全满足第三条，却因为 40000 > 32767 而写盘后回读不回来
/// （`.map` 头的宽高是 16 位有符号字段）。所以本常量**不能**单独当合法性判据用，
/// 判尺寸请一律调 [`validate_dimensions`]（或 [`can_round_trip`]）。
///
/// 读写两侧的其它同值出处（不是另一套真值，是同一数值在 C# 另两处的落点）：
/// `LegacyMapReader.cs:50` 的 `ValidateDimensions`（`count > 16_000_000`）、`Document.cs:58`。
///
/// 为什么还需要这条「格数」护栏：本库的 [`Cell`] 是 **36 B 定长**（14 B 字段 + 22 B 尾区），
/// 而 `Old12` 档每格只有 12 B ⇒ 内存约为文件大小的 3 倍。没有上限时，一张 200 MB 的
/// 合法 12 B/格图会去申请约 600 MB，失败方式是 OOM 而不是明确报错。
///
/// 与客户端的关系：Godot 客户端 `map_loader.gd` **没有**这条格数上限（只判 `w/h > 0`），
/// 所以格数护栏是本库有意加的；但 `w/h <= 32767` 那一条**不是**本库自造的护栏 ——
/// 它是上游 `CanRoundTrip` 的原文，也是 i16 头的物理容量。
pub const MAX_CELLS: usize = 16_000_000;

/// 单边维度上限（宽、高各自的最大值）= **32767** = `i16::MAX` = C# `short.MaxValue`。
///
/// 口径来源（真值）：C# `MapDimensions.cs:11-12@acfbbf8a` 的
/// `CanRoundTrip` 里两条 `<= short.MaxValue`。
///
/// 为什么必须有这一条（是**往返缺陷**，不是可选优化）：
/// `.map` 文件头的 `wWidth` / `wHeight` 在 C# 侧是**有符号 16 位**字段 ——
/// `Document.cs:90` 写的是 `w.Write(checked((short)Width))`；本仓 `mir2-format` 也按
/// `i16` 读回（`tmap_dims` 用 `i16_at`）。`32768..=65535` 这一段虽然塞得进 `u16`，
/// 写盘后要么在 C# 侧 `checked` 直接抛溢出异常，要么回读成负数 ⇒ **存下去就再也读不回来**。
///
/// ⇒ 「能塞进 `u16`」≠「能往返」。判尺寸一律用 [`validate_dimensions`] / [`can_round_trip`]，
/// 不要自己写 `w * h <= MAX_CELLS` 单条判断（那正是本次修掉的缺口）。
pub const MAX_DIMENSION: u16 = i16::MAX as u16;

/// 该宽高能不能**往返**（解析成功 → 写回 → 再解析，宽高与字节都不变）。
///
/// 与 C# `MapDimensions.CanRoundTrip` 逐字对应（`MapDimensions.cs:11-12@acfbbf8a`），
/// 三条件**合取**，短路顺序也与官方一致：
///
/// 1. `width > 0` 且 `height > 0`；
/// 2. `width <= `[`MAX_DIMENSION`]` (= 32767)` 且 `height <= `[`MAX_DIMENSION`]；
/// 3. `width * height <= `[`MAX_CELLS`]` (= 16_000_000)`。
///
/// 实现上直接复用 [`validate_dimensions`]（`is_ok()`），保证「布尔判据」与
/// 「带原因的判据」**永远同源**，不会各写一套后漂移。
///
/// ```
/// use mir2_map::{can_round_trip, MAX_DIMENSION};
///
/// assert_eq!(MAX_DIMENSION, 32767);
/// assert!(can_round_trip(32767, 1));
/// assert!(!can_round_trip(32768, 1));      // 单边超 32767 ⇒ 存下去读不回来
/// assert!(!can_round_trip(65535, 1));
/// assert!(!can_round_trip(0, 10));
/// assert!(can_round_trip(4000, 4000));     // 恰好 16_000_000 格
/// assert!(!can_round_trip(4001, 4000));    // 16_004_000 格
/// ```
pub fn can_round_trip(width: u16, height: u16) -> bool {
    validate_dimensions(width, height).is_ok()
}

/// 尺寸校验：`Ok(())` ⇒ 合法；`Err` ⇒ **第一个**不满足的条件（官方的短路顺序）。
///
/// 顺序**严格**照 C# `CanRoundTrip`（`MapDimensions.cs:11-12@acfbbf8a`）：
///
/// | 序 | 条件 | 拒绝时的错误变体 |
/// |----|------|------------------|
/// | ① | `width == 0` 或 `height == 0` | [`MapError::ZeroDimension`] |
/// | ② | `width > 32767` 或 `height > 32767` | [`MapError::DimensionTooLarge`] |
/// | ③ | `width * height > 16_000_000` | [`MapError::TooManyCells`] |
///
/// ②在③之前的理由不只是「照抄官方」：`40000×40000`（1.6G 格）两条都命中，
/// 报「单边超 32767」才是根因（先改成 32767 以内，才谈得上讨论格数上限）。
/// 注意 `30000×30000` 只命中③（30000 ≤ 32767），它报的是「格数超上限」。
///
/// 这是本 crate **唯一的尺寸判据**：[`crate::Map::parse`] 直接调它，
/// 上层要判「这个尺寸能不能存」也调它，不许另写一套。
///
/// ```
/// use mir2_map::{MapError, validate_dimensions};
///
/// assert!(validate_dimensions(32767, 1).is_ok());
/// assert!(matches!(
///     validate_dimensions(32768, 1),
///     Err(MapError::DimensionTooLarge { width: 32768, height: 1, limit: 32767 })
/// ));
/// assert!(matches!(
///     validate_dimensions(0, 1),
///     Err(MapError::ZeroDimension { .. })
/// ));
/// ```
pub fn validate_dimensions(width: u16, height: u16) -> Result<(), MapError> {
    if width == 0 || height == 0 {
        return Err(MapError::ZeroDimension { width, height });
    }
    if width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(MapError::DimensionTooLarge {
            width,
            height,
            limit: MAX_DIMENSION,
        });
    }
    let cells = u64::from(width) * u64::from(height);
    if cells > MAX_CELLS as u64 {
        return Err(MapError::TooManyCells {
            width,
            height,
            cells,
            limit: MAX_CELLS as u64,
        });
    }
    Ok(())
}

/// 格长档位（由文件长度唯一确定，见模块文档第一节）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CellFormat {
    /// 单格 12 字节：只有前 12 字节共用字段。
    Old12,
    /// 单格 14 字节：12 字节共用字段 + `btTiles` + `btsmTiles`。
    New14,
    /// 单格 36 字节：14 字节字段 + 22 字节尾区（语义未知，原样保留）。
    Ext36,
}

impl CellFormat {
    /// 单格字节数。
    pub const fn cell_size(self) -> usize {
        match self {
            CellFormat::Old12 => OLD12_CELL_SIZE,
            CellFormat::New14 => NEW14_CELL_SIZE,
            CellFormat::Ext36 => EXT36_CELL_SIZE,
        }
    }

    /// 是否带库号字节（`btTiles` / `btsmTiles`）。
    pub const fn has_lib_bytes(self) -> bool {
        matches!(self, CellFormat::New14 | CellFormat::Ext36)
    }

    /// 是否带 22 字节尾区。
    pub const fn has_tail(self) -> bool {
        matches!(self, CellFormat::Ext36)
    }

    /// 档位名（用于日志/错误信息）。
    pub const fn name(self) -> &'static str {
        match self {
            CellFormat::Old12 => "Old12",
            CellFormat::New14 => "New14",
            CellFormat::Ext36 => "Ext36",
        }
    }

    /// 按「文件长度精确整除」判定档位；三档皆不成立返回 `None`。
    ///
    /// 判定顺序固定 `Old12 → New14 → Ext36`。本函数**只**看长度等式，不判尺寸合法性：
    /// `w*h == 0` 时三档同长 52，顺序上会落到 `Old12`。但正常入口 [`crate::Map::parse`]
    /// 在此之前就因 0 尺寸返回 [`crate::MapError::ZeroDimension`]、因超格数返回
    /// [`crate::MapError::TooManyCells`] ⇒ 这两个分支在解析路径上不可达。
    /// 保留纯长度语义是有意的：「唯一判据 = 文件长度精确整除」这条规则不许被
    /// 偷偷塞进第二套真相（尺寸合法性由 [`crate::Map::parse`] 单独把关）。
    pub fn detect(width: u16, height: u16, len: usize) -> Option<CellFormat> {
        let (old12, new14, ext36) = expected_lengths(width, height);
        let len = len as u64;
        if len == old12 {
            Some(CellFormat::Old12)
        } else if len == new14 {
            Some(CellFormat::New14)
        } else if len == ext36 {
            Some(CellFormat::Ext36)
        } else {
            None
        }
    }
}

/// 按 `w*h` 算出三档期望文件总长，顺序为 `(Old12, New14, Ext36)`。
///
/// 全程用 `u64` 计算：`u16::MAX * u16::MAX = 4294836225` 格，再乘 36 会溢出 `u32`，
/// 但不会溢出 `u64`。
pub fn expected_lengths(width: u16, height: u16) -> (u64, u64, u64) {
    let cells = width as u64 * height as u64;
    let head = HEADER_LEN as u64;
    (
        head + cells * OLD12_CELL_SIZE as u64,
        head + cells * NEW14_CELL_SIZE as u64,
        head + cells * EXT36_CELL_SIZE as u64,
    )
}

/// 一个地图格。
///
/// 字段与文件字节一一对应，不含任何推导值。`tiles` / `sm_tiles` 在 `Old12` 档恒为 0
/// 且写回时**不写出这两个字节**；`tail` 仅在 `Ext36` 档有意义，其余档恒为 `[0; 22]`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Cell {
    /// 背景层图号（off 0）。
    pub bk_img: u16,
    /// 中间层图号（off 2）。
    pub mid_img: u16,
    /// 前景层图号（off 4）。
    pub fr_img: u16,
    /// 门索引（off 6）。
    pub door_index: u8,
    /// 门偏移（off 7）。
    pub door_offset: u8,
    /// 动画帧（off 8）。
    pub ani_frame: u8,
    /// 动画节拍（off 9）。
    pub ani_tick: u8,
    /// 区域（off 10）。
    pub area: u8,
    /// 光照（off 11）。
    pub light: u8,
    /// `btTiles`（off 12，仅 14/36 档；`Old12` 档恒 0）。
    pub tiles: u8,
    /// `btsmTiles`（off 13，仅 14/36 档；`Old12` 档恒 0）。
    pub sm_tiles: u8,
    /// 尾区 22 字节（off 14..36，仅 `Ext36` 档）。语义未知，**原样保存原样写回**。
    pub tail: [u8; TAIL_LEN],
}

impl Cell {
    /// 可走判定：背景层与前景层都不带 `0x8000` 位。
    ///
    /// 规格写法是 `((wBkImg & 0x8000) + (wFrImg & 0x8000)) == 0`。
    /// 这里改用等价的 `&&` 形式：`u16` 上 `0x8000 + 0x8000` 会溢出
    /// （release 下回绕成 0 ⇒ 把「两层都挡」误判成可走），`&&` 无此风险且语义完全相同。
    pub const fn is_walkable(&self) -> bool {
        (self.bk_img & 0x8000) == 0 && (self.fr_img & 0x8000) == 0
    }
}

/// 从单格原始字节解出 [`Cell`]。`raw.len()` 必须等于 `format.cell_size()`。
pub(crate) fn read_cell(format: CellFormat, raw: &[u8]) -> Cell {
    let mut tail = [0u8; TAIL_LEN];
    let (tiles, sm_tiles) = if format.has_lib_bytes() {
        (raw[LIB_OFFSET], raw[LIB_OFFSET + 1])
    } else {
        (0, 0)
    };
    if format.has_tail() {
        tail.copy_from_slice(&raw[TAIL_OFFSET..TAIL_OFFSET + TAIL_LEN]);
    }
    Cell {
        bk_img: u16::from_le_bytes([raw[0], raw[1]]),
        mid_img: u16::from_le_bytes([raw[2], raw[3]]),
        fr_img: u16::from_le_bytes([raw[4], raw[5]]),
        door_index: raw[6],
        door_offset: raw[7],
        ani_frame: raw[8],
        ani_tick: raw[9],
        area: raw[10],
        light: raw[11],
        tiles,
        sm_tiles,
        tail,
    }
}

/// 把 [`Cell`] 按档位追加到 `out` 尾部，字节数与 `format.cell_size()` 严格相等。
///
/// `Old12` 档**只写 12 字节**：`tiles` / `sm_tiles` / `tail` 即便被人为设成非零也不会落盘
/// （这两个字段在该档无对应字节，凭空补写会让文件变长、破坏往返保真）。
pub(crate) fn write_cell(cell: &Cell, format: CellFormat, out: &mut Vec<u8>) {
    out.extend_from_slice(&cell.bk_img.to_le_bytes());
    out.extend_from_slice(&cell.mid_img.to_le_bytes());
    out.extend_from_slice(&cell.fr_img.to_le_bytes());
    out.push(cell.door_index);
    out.push(cell.door_offset);
    out.push(cell.ani_frame);
    out.push(cell.ani_tick);
    out.push(cell.area);
    out.push(cell.light);
    if format.has_lib_bytes() {
        out.push(cell.tiles);
        out.push(cell.sm_tiles);
    }
    if format.has_tail() {
        out.extend_from_slice(&cell.tail);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 档位字节数() {
        assert_eq!(CellFormat::Old12.cell_size(), 12);
        assert_eq!(CellFormat::New14.cell_size(), 14);
        assert_eq!(CellFormat::Ext36.cell_size(), 36);
        assert!(!CellFormat::Old12.has_lib_bytes());
        assert!(CellFormat::New14.has_lib_bytes());
        assert!(CellFormat::Ext36.has_lib_bytes());
        assert!(!CellFormat::New14.has_tail());
        assert!(CellFormat::Ext36.has_tail());
    }

    #[test]
    fn 判别式按长度精确整除() {
        // 30x35 = 1050 格
        assert_eq!(
            expected_lengths(30, 35),
            (52 + 12600, 52 + 14700, 52 + 37800)
        );
        assert_eq!(CellFormat::detect(30, 35, 12652), Some(CellFormat::Old12));
        assert_eq!(CellFormat::detect(30, 35, 14752), Some(CellFormat::New14));
        assert_eq!(CellFormat::detect(30, 35, 37852), Some(CellFormat::Ext36));
        // 差一字节即不成立
        assert_eq!(CellFormat::detect(30, 35, 12651), None);
        assert_eq!(CellFormat::detect(30, 35, 12653), None);
        assert_eq!(CellFormat::detect(30, 35, 14751), None);
        assert_eq!(CellFormat::detect(30, 35, 37851), None);
    }

    #[test]
    fn 零尺寸地图三档同长取old12() {
        // 纯长度语义：0×0 ⇒ 三档期望长度都是 52，顺序上落到 Old12。
        // ⚠ 这个分支在正式解析路径上**不可达**：Map::parse 先判 w/h 为 0 就报
        //    ZeroDimension（见 map.rs 的 `零尺寸一律报错...` 用例）。
        assert_eq!(expected_lengths(0, 0), (52, 52, 52));
        assert_eq!(CellFormat::detect(0, 0, 52), Some(CellFormat::Old12));
        assert_eq!(CellFormat::detect(0, 0, 53), None);
        // 单边为 0 同理（`52 + 0`）
        assert_eq!(expected_lengths(0, 7), (52, 52, 52));
        assert_eq!(expected_lengths(7, 0), (52, 52, 52));
    }

    #[test]
    fn 单元格读写三档自洽() {
        // 造一个各字段都非零、尾区也非零的格
        let mut cell = Cell {
            bk_img: 0x8123,
            mid_img: 0x4567,
            fr_img: 0x89AB,
            door_index: 1,
            door_offset: 2,
            ani_frame: 3,
            ani_tick: 4,
            area: 5,
            light: 6,
            tiles: 7,
            sm_tiles: 8,
            tail: [0u8; TAIL_LEN],
        };
        for (i, b) in cell.tail.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(7).wrapping_add(1);
        }

        for fmt in [CellFormat::Old12, CellFormat::New14, CellFormat::Ext36] {
            let mut buf = Vec::new();
            write_cell(&cell, fmt, &mut buf);
            assert_eq!(buf.len(), fmt.cell_size(), "{fmt:?} 写出的字节数不对");
            let back = read_cell(fmt, &buf);
            assert_eq!(back.bk_img, cell.bk_img);
            assert_eq!(back.mid_img, cell.mid_img);
            assert_eq!(back.fr_img, cell.fr_img);
            assert_eq!(back.door_index, cell.door_index);
            assert_eq!(back.door_offset, cell.door_offset);
            assert_eq!(back.ani_frame, cell.ani_frame);
            assert_eq!(back.ani_tick, cell.ani_tick);
            assert_eq!(back.area, cell.area);
            assert_eq!(back.light, cell.light);
            if fmt.has_lib_bytes() {
                assert_eq!(back.tiles, cell.tiles);
                assert_eq!(back.sm_tiles, cell.sm_tiles);
            } else {
                assert_eq!(back.tiles, 0, "Old12 档 tiles 必须为 0");
                assert_eq!(back.sm_tiles, 0, "Old12 档 sm_tiles 必须为 0");
            }
            if fmt.has_tail() {
                assert_eq!(back.tail, cell.tail);
            } else {
                assert_eq!(back.tail, [0u8; TAIL_LEN], "{fmt:?} 档 tail 必须为 0");
            }
        }
    }

    #[test]
    fn 可走判定两层都看() {
        let mut c = Cell::default();
        assert!(c.is_walkable(), "两层都为 0 应可走");
        c.bk_img = 0x8000;
        assert!(!c.is_walkable(), "背景层带 0x8000 应不可走");
        c.bk_img = 0;
        c.fr_img = 0x8000;
        assert!(!c.is_walkable(), "前景层带 0x8000 应不可走");
        c.bk_img = 0x8000;
        assert!(
            !c.is_walkable(),
            "两层都带 0x8000 应不可走（不得因 u16 溢出误判）"
        );
        // 其它高位不影响
        c.bk_img = 0x7FFF;
        c.fr_img = 0x4000;
        assert!(c.is_walkable());
    }

    // ─────────────────── 尺寸判据（C# `MapDimensions.CanRoundTrip`） ───────────────────

    /// 单边 32767 接受、32768 / 65535 拒绝；0 一律拒绝。
    ///
    /// 真值：`MapDimensions.cs:11-12@acfbbf8a` 的两条 `<= short.MaxValue`。
    #[test]
    fn 单边上限32767的边界() {
        assert_eq!(MAX_DIMENSION, 32767, "单边上限必须是 i16::MAX");
        assert_eq!(MAX_DIMENSION, i16::MAX as u16);

        assert!(can_round_trip(32767, 1), "32767×1 应可往返");
        assert!(can_round_trip(1, 32767), "1×32767 应可往返");
        assert!(!can_round_trip(32768, 1), "32768×1 必须拒绝（超 short.MaxValue）");
        assert!(!can_round_trip(1, 32768), "1×32768 必须拒绝");
        assert!(!can_round_trip(65535, 1), "65535×1 必须拒绝（u16 塞得进，i16 塞不进）");
        assert!(!can_round_trip(65535, 65535), "65535×65535 必须拒绝");

        // 0 尺寸（C# 第一条 `width is > 0`）
        assert!(!can_round_trip(0, 0));
        assert!(!can_round_trip(0, 10));
        assert!(!can_round_trip(10, 0));
    }

    /// 三条件是**合取**：单看格数会放过 `40000×1`（40000 格 ≪ 16M，但单边超 32767）。
    ///
    /// 这正是本次修掉的缺口 —— 旧代码只有「`w*h <= 16M`」一条。
    #[test]
    fn 三条合取_格数合格不代表尺寸合格() {
        // 40000×1：格数只有 40000（远小于 16M），旧的「只看格数」判据会放行它
        assert!(40_000u64 < MAX_CELLS as u64);
        assert!(!can_round_trip(40000, 1), "40000×1 单边超 32767，必须拒绝");
        assert!(matches!(
            validate_dimensions(40000, 1),
            Err(MapError::DimensionTooLarge {
                width: 40000,
                height: 1,
                limit: 32767
            })
        ));
        // 40000×40000：单边超 32767（且格数也超 16M），按官方短路顺序报前者
        assert!(matches!(
            validate_dimensions(40000, 40000),
            Err(MapError::DimensionTooLarge { .. })
        ));
        // 30000×30000：单边没超 32767，只有格数超 ⇒ 报 TooManyCells（别把两者混为一谈）
        assert!(matches!(
            validate_dimensions(30000, 30000),
            Err(MapError::TooManyCells { .. })
        ));
    }

    /// 格数上限边界：`16_000_000` 恰好通过，且**对每个宽度**「再多一格」必被拒。
    ///
    /// ⚠ `16_000_001` 本身**不可表示**为 `u16×u16` 的乘积：它最小的非平凡因子是 109，
    /// 余因子 146789 > 65535（另两组 229×69869、641×24961 同样越界）。
    /// 所以「恰好 16M 接受、16M+1 拒绝」这条边界改用**逐宽度穷举**证明：
    /// 对每个 `w`，取 `h0 = floor(16M/w)`，则 `w*h0 <= 16M`（接受）而 `w*(h0+1) > 16M`（拒绝），
    /// 即判据的临界点精确落在 `16_000_000 / 16_000_001` 之间，没有被写成 `>=` 或别的偏移。
    #[test]
    fn 格数上限边界逐宽度穷举() {
        // 4000×4000 恰好等于上限 —— 必须通过（边界是「>」不是「>=」）
        assert_eq!(4000u64 * 4000, MAX_CELLS as u64);
        assert!(can_round_trip(4000, 4000), "恰好 16_000_000 格应通过");
        assert!(validate_dimensions(4000, 4000).is_ok());

        // 4001×4000 = 16_004_000 只超 4000 格 —— 必须报「格数超上限」而不是「尺寸超 32767」
        assert!(matches!(
            validate_dimensions(4001, 4000),
            Err(MapError::TooManyCells {
                cells: 16_004_000,
                limit: 16_000_000,
                ..
            })
        ));

        // 逐宽度穷举临界点：w 从 489 起 h0 才会 ≤ 32767（16_000_000 / 32767 ≈ 488.3）
        for w in 489u16..=MAX_DIMENSION {
            let h0 = (MAX_CELLS as u64 / u64::from(w)) as u16;
            assert!(
                (1..=MAX_DIMENSION).contains(&h0),
                "w={w} 时 h0={h0} 越界"
            );
            assert!(
                u64::from(w) * u64::from(h0) <= MAX_CELLS as u64,
                "w={w} h0={h0} 应在上限内"
            );
            assert!(can_round_trip(w, h0), "w={w} h0={h0} 应接受");
            assert!(
                !can_round_trip(w, h0 + 1),
                "w={w} h={} 已超 16M，必须拒绝",
                h0 + 1
            );
            assert!(
                matches!(
                    validate_dimensions(w, h0 + 1),
                    Err(MapError::TooManyCells { .. })
                ),
                "w={w} h={} 应报 TooManyCells",
                h0 + 1
            );
        }
    }

    /// 布尔判据与带原因的判据**必须永远同源**（`can_round_trip` 只是 `validate_dimensions` 的 `is_ok()`）。
    ///
    /// 这条用例防的是「两份判据各写一套，改了一处忘了另一处」。
    #[test]
    fn 布尔判据与带原因判据同源() {
        let 样本: [(u16, u16); 14] = [
            (0, 0),
            (0, 5),
            (5, 0),
            (1, 1),
            (32767, 1),
            (1, 32767),
            (32768, 1),
            (1, 32768),
            (65535, 1),
            (65535, 65535),
            (4000, 4000),
            (4000, 4001),
            (30000, 30000),
            (1000, 600),
        ];
        for (w, h) in 样本 {
            assert_eq!(
                can_round_trip(w, h),
                validate_dimensions(w, h).is_ok(),
                "{w}×{h}：布尔判据与 validate_dimensions 不一致"
            );
        }
    }
}
