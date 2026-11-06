//! `Map` 结构：解析、写回、按坐标取格、可走判定。
//!
//! # 存储顺序：列优先
//!
//! 文件里第 `(x, y)` 格的字节偏移为：
//!
//! ```text
//! offset = 52 + (x * h + y) * cell_size
//! ```
//!
//! `x` 外层、`y` 内层（即先走完一整列 `y=0..h`，再进到 `x+1`）。
//! [`Map::cells`] 的下标与之一致：`idx = x * h + y`，**不做转置、不做重排**，
//! 这样 `parse` 与 `to_bytes` 天然对称。
//!
//! # 保真两条铁律
//!
//! 1. [`Map::header_raw`] 原样保留 52 字节头，写回时逐字节照抄（含 w/h 之外的 48 字节）；
//! 2. [`Cell::tail`]（`Ext36` 的 14..35）原样保存原样写回，**绝不清零**。

use crate::cell::{
    read_cell, validate_dimensions, write_cell, Cell, CellFormat, HEADER_LEN,
};
use crate::error::MapError;

/// 一张地图。
///
/// 字段全部公开：本库是「保真读写层」，不做封装隐藏，调用方（编辑器）需要直接看到
/// 原始头与每个格的全部字节级状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Map {
    /// 52 字节文件头原样副本。写回时逐字节照抄。
    ///
    /// 其中 `[0..2]` 是 `wWidth`、`[2..4]` 是 `wHeight`，其余 48 字节未解释、原样保留。
    pub header_raw: [u8; HEADER_LEN],
    /// 宽（从 `header_raw[0..2]` 小端读出）。
    pub width: u16,
    /// 高（从 `header_raw[2..4]` 小端读出）。
    pub height: u16,
    /// 格长档位（由文件长度唯一判定）。
    pub format: CellFormat,
    /// 全部格，**列优先顺序**：`cells[x * height + y]` 对应 `(x, y)`。
    pub cells: Vec<Cell>,
}

impl Map {
    /// 解析 `.map` 字节流。
    ///
    /// 判定顺序（尺寸护栏**先于**格长判据，与 C# `LegacyMapReader.cs:47-54`
    /// 的 `ValidateDimensions` 同序，尺寸判据本身与 `MapDimensions.cs:11-12@acfbbf8a`
    /// 的 `CanRoundTrip` 三条件一一对应）：
    /// 1. `len < 52` ⇒ [`MapError::TooShort`]；
    /// 2. `w == 0 || h == 0` ⇒ [`MapError::ZeroDimension`]
    ///    （真值：客户端 `map_loader.gd:94-95` 与 C# `LegacyMapReader.cs:50` 都直接拒绝。
    ///    放行会把「52 字节全零」这类损坏/截断文件静默救活成 0×0 空地图，且编辑器仍认为它可保存）；
    /// 3. `w > 32767 || h > 32767` ⇒ [`MapError::DimensionTooLarge`]
    ///    （真值：C# `CanRoundTrip` 的两条 `<= short.MaxValue`。`.map` 头的宽高是
    ///    16 位**有符号**字段，超限写盘后回读不回来 ⇒ 必须在入口就拒，不许读到内存里再截断）；
    /// 4. `w*h > MAX_CELLS`（16_000_000）⇒ [`MapError::TooManyCells`]
    ///    （**必须在分配之前**：`Cell` 是 36 B 定长，12 B/格的图内存约为文件的 3 倍，
    ///    口径见 [`crate::MAX_CELLS`] 的文档）；
    /// 5. 按头部 w/h 算三档期望长度，与 `len` 精确比对 ⇒ 得 [`CellFormat`]；
    ///    三档都不等 ⇒ [`MapError::LengthMismatch`]（错误里列出三档期望长度与实际长度）；
    /// 6. 按档位逐格解码，列优先。
    ///
    /// 第 2~4 条由 [`crate::validate_dimensions`] 单点实现（顺序即官方短路顺序），
    /// 本函数不另写一套判断。
    ///
    /// 本函数不校验任何字段语义（语义未确认），只保证字节可逆。
    pub fn parse(bytes: &[u8]) -> Result<Map, MapError> {
        if bytes.len() < HEADER_LEN {
            return Err(MapError::TooShort { len: bytes.len() });
        }

        let mut header_raw = [0u8; HEADER_LEN];
        header_raw.copy_from_slice(&bytes[..HEADER_LEN]);
        let width = u16::from_le_bytes([header_raw[0], header_raw[1]]);
        let height = u16::from_le_bytes([header_raw[2], header_raw[3]]);

        // 尺寸护栏（官方三条件合取，顺序照 C# `MapDimensions.cs:11-12@acfbbf8a` 的短路顺序）：
        // ① 0 尺寸；② 单边 > 32767；③ 格数 > 16M。
        //
        // ① 若放到长度判据之后，`w*h == 0` 会因为三档期望长度都等于 52 而被判成 Old12 空图；
        // ② 若缺失，会一条条漏到长度判据去报「长度不匹配」——`32768×1` 的最初表现正是如此，
        //    那是把「尺寸非法」误诊成「文件损坏」；
        // ③ 必须在 `Vec::with_capacity` 之前判：否则一张声明 5000×5000 的坏头部会先申请约 900 MB。
        validate_dimensions(width, height)?;
        let cell_count = u64::from(width) * u64::from(height);

        let format = CellFormat::detect(width, height, bytes.len())
            .ok_or_else(|| MapError::length_mismatch(width, height, bytes.len()))?;

        let cell_size = format.cell_size();
        // cell_count 已 ≤ MAX_CELLS（16M）⇒ 转 usize 与后续 `idx * cell_size` 在 64 位机上不会溢出；
        // 且长度已与 52 + cell_size*cell_count 精确相等，取格切片不会越界。
        let cell_count = cell_count as usize;
        let mut cells = Vec::with_capacity(cell_count);
        for idx in 0..cell_count {
            let base = HEADER_LEN + idx * cell_size;
            cells.push(read_cell(format, &bytes[base..base + cell_size]));
        }

        Ok(Map {
            header_raw,
            width,
            height,
            format,
            cells,
        })
    }

    /// 序列化回字节流；对 [`Map::parse`] 的成功输入，输出必须与之逐字节相等。
    ///
    /// 输出长度 = `52 + cells.len() * format.cell_size()`。
    /// 注意：本函数按 `cells` 的**实际长度**输出，不读 `width`/`height` 做补齐或截断；
    /// 头部字节一律取自 [`Map::header_raw`]。因此若要改变地图尺寸，必须同时改
    /// `header_raw[0..4]`、`width`、`height` 与 `cells`，否则写出的文件会自相矛盾
    /// （长度与头部声明的 w/h 不匹配）。
    ///
    /// ⚠ 本函数**不**再做尺寸校验，也不再需要：`header_raw` 是逐字节照抄的，
    /// 不存在 `as i16` 这类会把宽高改掉的窄化转换。合法入口只有 [`Map::parse`]，
    /// 它已经用 [`crate::validate_dimensions`] 把宽高限在 `1..=32767` 且格数 `<= 16M`；
    /// 若你手工构造/改写了 `Map` 的字段，请自己对 `(width, height)` 调一次
    /// [`crate::validate_dimensions`]，否则可能写出一张「存得下、读不回来」的图。
    pub fn to_bytes(&self) -> Vec<u8> {
        let cell_size = self.format.cell_size();
        let mut out = Vec::with_capacity(HEADER_LEN + self.cells.len() * cell_size);
        out.extend_from_slice(&self.header_raw);
        for cell in &self.cells {
            write_cell(cell, self.format, &mut out);
        }
        out
    }

    /// 按坐标取格；`x >= width` 或 `y >= height` 返回 `None`。
    pub fn cell(&self, x: u16, y: u16) -> Option<&Cell> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let idx = x as usize * self.height as usize + y as usize;
        self.cells.get(idx)
    }

    /// 该格是否可走：背景层与前景层都不带 `0x8000` 位。
    ///
    /// 越界坐标一律返回 `false`（地图外没有可走的地面），不 panic。
    pub fn walkable(&self, x: u16, y: u16) -> bool {
        match self.cell(x, y) {
            Some(cell) => cell.is_walkable(),
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell::{expected_lengths, OLD12_CELL_SIZE, TAIL_LEN};

    /// 造一张 `w*h` 的 Old12 图原始字节：头 52 字节填充非零，格内容按 idx 变化。
    fn 造图(w: u16, h: u16, fmt: CellFormat) -> Vec<u8> {
        let size = fmt.cell_size();
        let mut bytes = vec![0u8; HEADER_LEN + size * (w as usize * h as usize)];
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(31).wrapping_add(7);
        }
        bytes[0..2].copy_from_slice(&w.to_le_bytes());
        bytes[2..4].copy_from_slice(&h.to_le_bytes());
        bytes
    }

    #[test]
    fn 三档解析与写回逐字节一致() {
        for fmt in [CellFormat::Old12, CellFormat::New14, CellFormat::Ext36] {
            let raw = 造图(7, 5, fmt);
            let map = Map::parse(&raw).expect("长度合法应解析成功");
            assert_eq!(map.format, fmt);
            assert_eq!(map.width, 7);
            assert_eq!(map.height, 5);
            assert_eq!(map.cells.len(), 35);
            assert_eq!(map.to_bytes(), raw, "{fmt:?} 档往返不一致");

            // 头 52 字节必须原样（含 w/h 之外的 48 字节）
            assert_eq!(&map.header_raw[..], &raw[..HEADER_LEN]);
        }
    }

    #[test]
    fn 列优先下标() {
        // 4 宽 3 高；列优先 idx = x*h + y（行优先会算成 y*w + x，两者不同）
        let w = 4u16;
        let h = 3u16;
        let mut raw = 造图(w, h, CellFormat::New14);
        // (x=2,y=1) → idx 7；若是行优先则会落到 idx 6
        let idx_a = 2 * h as usize + 1;
        let base_a = HEADER_LEN + idx_a * 14;
        raw[base_a] = 0xAA;
        raw[base_a + 1] = 0xAA;
        // (x=1,y=2) → idx 5（= 1*h + 2）；若是行优先则会落到 idx 9
        // 注：写成 `h as usize + 2` 而不是 `1 * h as usize + 2`，只为躲开 clippy 的
        //     `identity_op`（原式在本次改动前就触发该告警）；算术结果完全相同。
        let idx_b = h as usize + 2;
        let base_b = HEADER_LEN + idx_b * 14;
        raw[base_b] = 0xBB;
        raw[base_b + 1] = 0xBB;

        let map = Map::parse(&raw).unwrap();
        assert_eq!(
            map.cell(2, 1).unwrap().bk_img,
            0xAAAA,
            "列优先 idx = x*h + y 不对"
        );
        assert_eq!(
            map.cell(1, 2).unwrap().bk_img,
            0xBBBB,
            "列优先 idx = x*h + y 不对"
        );
        assert_eq!(map.to_bytes(), raw);
    }

    #[test]
    fn 尾区原样保留() {
        let mut raw = 造图(2, 2, CellFormat::Ext36);
        // (x=1,y=1) 在 2×2 图里的列优先下标 = 1*2 + 1 = 3。
        // 直接写 3 而不是 `(1 * 2 + 1)`：只为躲开 clippy 的 `identity_op`
        // （原式在本次改动前就触发该告警），下标值完全相同。
        let idx = 3usize;
        let base = HEADER_LEN + idx * 36;
        raw[base + 18] = 0xAA;
        raw[base + 25] = 0xBB;
        let map = Map::parse(&raw).unwrap();
        let cell = map.cell(1, 1).unwrap();
        assert_eq!(cell.tail[4], 0xAA, "off18 应落在 tail[4]");
        assert_eq!(cell.tail[11], 0xBB, "off25 应落在 tail[11]");
        assert_eq!(map.to_bytes(), raw);
    }

    #[test]
    fn 越界取格与可走判定() {
        let mut raw = 造图(2, 3, CellFormat::Old12);
        // (0,0) 清成可走，(1,0) 背景带 0x8000，(0,1) 前景带 0x8000
        let base = HEADER_LEN;
        raw[base..base + 12].copy_from_slice(&[0u8; 12]);
        let b1 = HEADER_LEN + 12; // (x=1,y=0)
        raw[b1..b1 + 12].copy_from_slice(&[0u8; 12]);
        raw[b1] = 0x00;
        raw[b1 + 1] = 0x80;
        let b2 = HEADER_LEN + 2 * 12; // (x=0,y=1)
        raw[b2..b2 + 12].copy_from_slice(&[0u8; 12]);
        raw[b2 + 4] = 0x00;
        raw[b2 + 5] = 0x80;
        let map = Map::parse(&raw).unwrap();
        assert!(map.walkable(0, 0));
        assert!(!map.walkable(1, 0), "背景层 0x8000 应不可走");
        assert!(!map.walkable(0, 1), "前景层 0x8000 应不可走");
        assert!(map.cell(2, 0).is_none());
        assert!(map.cell(0, 3).is_none());
        assert!(!map.walkable(9, 9), "越界应不可走且不 panic");
    }

    #[test]
    fn 长度不足与三档皆不匹配() {
        let short = vec![0u8; 51];
        assert_eq!(Map::parse(&short), Err(MapError::TooShort { len: 51 }));

        let mut raw = 造图(3, 3, CellFormat::Old12);
        raw.push(0); // 161 字节，四档都不等
        match Map::parse(&raw) {
            Err(MapError::LengthMismatch {
                width,
                height,
                len,
                expect_old12,
                expect_new14,
                expect_ext36,
            }) => {
                assert_eq!((width, height, len), (3, 3, 161));
                assert_eq!(expect_old12, 160);
                assert_eq!(expect_new14, 178);
                assert_eq!(expect_ext36, 376);
            }
            other => panic!("应当 LengthMismatch，实际 {other:?}"),
        }
    }

    #[test]
    fn 尾区长度常量一致() {
        // 防止有人改 TAIL_LEN 却没改格长常量
        assert_eq!(
            crate::cell::TAIL_OFFSET + TAIL_LEN,
            CellFormat::Ext36.cell_size()
        );
        assert_eq!(
            crate::cell::COMMON_CELL_LEN + 2 + TAIL_LEN,
            CellFormat::Ext36.cell_size()
        );
        assert_eq!(expected_lengths(1, 1), (64, 66, 88));
    }

    /// E4：`w == 0 || h == 0` 一律报错（含「52 字节全零」这种被截断/损坏的文件）。
    ///
    /// 旧行为（把 0 尺寸当合法空地图）已被本用例取代 —— 那是本次修掉的缺陷，
    /// 不是被放宽的断言。
    #[test]
    fn 零尺寸一律报错且信息带实际宽高() {
        for (w, h) in [(0u16, 0u16), (0, 7), (7, 0)] {
            let raw = 造图(w, h, CellFormat::Old12); // 52 + 12*0 = 52 字节
            assert_eq!(raw.len(), HEADER_LEN, "0 格图应正好 52 字节");
            let err = Map::parse(&raw)
                .err()
                .unwrap_or_else(|| panic!("{w}×{h} 不得被当成合法空地图解析成功"));
            match err {
                MapError::ZeroDimension {
                    width,
                    height,
                } => assert_eq!((width, height), (w, h), "错误里的宽高应是头部实际值"),
                other => panic!("{w}×{h} 应报 ZeroDimension，实际 {other:?}"),
            }
            let msg = err.to_string();
            assert!(msg.contains(&format!("w={w}")), "{w}×{h}：{msg}");
            assert!(msg.contains(&format!("h={h}")), "{w}×{h}：{msg}");
        }
    }

    /// E5：16M 格上限护栏 —— 假头部声明 5000×5000（25M 格）必须在**分配之前**报错。
    ///
    /// 若护栏缺失或位置错（放在 `Vec::with_capacity` 之后），本用例会先去申请
    /// 25_000_000 × 36 B ≈ 900 MB（本机可能直接被 OOM 杀掉，或耗时数十秒）。
    #[test]
    fn 超格数上限先报错不分配() {
        let 假头 = |w: u16, h: u16| {
            let mut raw = vec![0u8; HEADER_LEN]; // 故意只给 52 字节：长度判据必然不成立
            raw[0..2].copy_from_slice(&w.to_le_bytes());
            raw[2..4].copy_from_slice(&h.to_le_bytes());
            raw
        };

        match Map::parse(&假头(5000, 5000)).expect_err("5000×5000 = 25M 格，必须 Err") {
            MapError::TooManyCells {
                width,
                height,
                cells,
                limit,
            } => {
                assert_eq!((width, height), (5000, 5000));
                assert_eq!(cells, 25_000_000);
                assert_eq!(limit, 16_000_000, "上限必须与 C# LegacyMapReader.cs:50 同口径");
            }
            other => panic!("应报 TooManyCells，实际 {other:?}"),
        }
        // 报的必须是「超格数」而不是「长度不匹配」⇒ 证明护栏跑在长度判据之前
        assert!(matches!(
            Map::parse(&假头(5000, 5000)),
            Err(MapError::TooManyCells { .. })
        ));

        // 边界：恰好等于上限（4000×4000 = 16_000_000）**不拦**，落到长度判据
        assert!(
            matches!(
                Map::parse(&假头(4000, 4000)),
                Err(MapError::LengthMismatch { .. })
            ),
            "恰好等于上限应放行到长度判据（护栏只拦「超过」）"
        );
        // 只超 1 格就拦（4001×4000 = 16_004_000）
        assert!(matches!(
            Map::parse(&假头(4001, 4000)),
            Err(MapError::TooManyCells { .. })
        ));
    }

    #[test]
    fn 上限常量与csharp同口径() {
        // 防有人改 MAX_CELLS 却不同步注释/C# 口径
        assert_eq!(crate::MAX_CELLS, 16_000_000);
        assert_eq!(4000usize * 4000, crate::MAX_CELLS);
        // 单边上限同理（C# `MapDimensions.cs:11-12` 的两条 `<= short.MaxValue`）
        assert_eq!(crate::MAX_DIMENSION, 32767);
    }

    /// 单边超 32767 必须在**长度判据之前**报「尺寸超限」，
    /// 而不是掉到长度判据去报 [`MapError::LengthMismatch`]（更不许报 [`MapError::TooManyCells`]）。
    ///
    /// 这是本次修的核心缺口：旧代码只查 `w*h > 16M`，`32768×1`（32768 格）一路漏到长度判据，
    /// 用户看到的是「文件损坏」，实际是「尺寸根本存不下去」。
    #[test]
    fn 超单边上限先报尺寸超限不报长度() {
        let 假头 = |w: u16, h: u16| {
            let mut raw = vec![0u8; HEADER_LEN]; // 只给 52 字节：若漏到长度判据必然 LengthMismatch
            raw[0..2].copy_from_slice(&w.to_le_bytes());
            raw[2..4].copy_from_slice(&h.to_le_bytes());
            raw
        };

        for (w, h) in [(32768u16, 1u16), (1, 32768), (65535, 1), (1, 65535), (40000, 1)] {
            let err = Map::parse(&假头(w, h))
                .err()
                .unwrap_or_else(|| panic!("{w}×{h} 单边超 32767，必须 Err"));
            match err {
                MapError::DimensionTooLarge {
                    width,
                    height,
                    limit,
                } => {
                    assert_eq!((width, height), (w, h), "错误里的宽高应是头部实际值");
                    assert_eq!(limit, 32767, "上限必须是 short.MaxValue");
                }
                other => panic!("{w}×{h} 应报 DimensionTooLarge，实际 {other:?}"),
            }
            let msg = err.to_string();
            assert!(msg.contains("32767"), "{w}×{h} 的信息必须点名 32767：{msg}");
            assert!(
                msg.contains("CanRoundTrip"),
                "{w}×{h} 的信息必须点名 C# CanRoundTrip：{msg}"
            );
            assert!(
                !msg.contains("长度"),
                "{w}×{h} 不该被描述成长度问题：{msg}"
            );
        }

        // 32767×1 只给 52 字节 ⇒ 尺寸合法，落到长度判据（证明 32767「放行」是真的放行）
        assert!(
            matches!(
                Map::parse(&假头(32767, 1)),
                Err(MapError::LengthMismatch { .. })
            ),
            "32767×1 尺寸合法，应放行到长度判据"
        );
    }

    /// 同时命中「单边超 32767」与「格数超 16M」时，按官方短路顺序报**尺寸**超限。
    #[test]
    fn 双超时报尺寸而非格数() {
        let mut raw = vec![0u8; HEADER_LEN];
        raw[0..2].copy_from_slice(&30000u16.to_le_bytes());
        raw[2..4].copy_from_slice(&30000u16.to_le_bytes());
        // 30000×30000 = 900M 格（超 16M）且单边 30000 ≤ 32767（不超）
        assert!(
            matches!(Map::parse(&raw), Err(MapError::TooManyCells { .. })),
            "30000×30000 单边没超 32767，应报格数超上限"
        );

        // 40000×40000：两条都超 ⇒ 官方先短路在 short.MaxValue 上
        raw[0..2].copy_from_slice(&40000u16.to_le_bytes());
        raw[2..4].copy_from_slice(&40000u16.to_le_bytes());
        assert!(
            matches!(
                Map::parse(&raw),
                Err(MapError::DimensionTooLarge { .. })
            ),
            "40000×40000 同时超两条，应报尺寸超 32767（官方短路顺序）"
        );
    }

    /// 单边恰好 32767 的**整图往返**：解析 → 写回 → 逐字节相等。
    ///
    /// 体积：`52 + 32767 × 12 = 393_256` 字节（约 384 KiB），可控。
    #[test]
    fn 单边32767整图往返() {
        let w = 32767u16;
        let h = 1u16;
        let raw = 造图(w, h, CellFormat::Old12);
        assert_eq!(raw.len(), HEADER_LEN + 32767 * OLD12_CELL_SIZE);
        assert_eq!(raw.len(), 393_256);

        let map = Map::parse(&raw).expect("32767×1 是合法尺寸，必须解析成功");
        assert_eq!((map.width, map.height), (w, h));
        assert_eq!(map.format, CellFormat::Old12);
        assert_eq!(map.cells.len(), 32767);
        // 两端都要取得到，且中间不串位（列优先 idx = x*h + y，h=1 ⇒ idx = x）
        assert!(map.cell(0, 0).is_some());
        assert!(map.cell(32766, 0).is_some());
        assert!(map.cell(32767, 0).is_none(), "x=32767 越界");

        let back = map.to_bytes();
        assert_eq!(back.len(), raw.len());
        assert_eq!(back, raw, "32767×1 的整图往返必须逐字节相等");

        // 再解析一次：宽高与格数都得回来
        let again = Map::parse(&back).expect("写回的字节必须能再解析");
        assert_eq!((again.width, again.height), (w, h));
        assert_eq!(again.cells.len(), 32767);
    }
}
