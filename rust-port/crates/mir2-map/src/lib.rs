//! `mir2-map` —— 传奇 M2 `.map` 地图**读写往返保真**库（地图编辑器 P0 核心层）。
//!
//! 本 crate 只做一件事：把 `.map` 解析成结构体，再原样写回，**逐字节相等**。
//! 不做 GUI、不做 CLI、不猜字段语义、不做任何「顺手归一化」。
//!
//! # 一、三档格长判别式（唯一判据 = 文件长度精确整除，禁标题字节）
//!
//! | 档位 | 单格字节 | 期望文件总长 | 说明 |
//! |------|----------|--------------|------|
//! | [`CellFormat::Old12`] | 12 | `52 + 12*w*h` | 只有前 12 字节共用字段 |
//! | [`CellFormat::New14`] | 14 | `52 + 14*w*h` | 12 字节 + `btTiles` + `btsmTiles` |
//! | [`CellFormat::Ext36`] | 36 | `52 + 36*w*h` | 14 字节 + 22 字节尾区 |
//!
//! - 文件头 `TMapHeader` 固定 **52 字节**：`wWidth` u16@0、`wHeight` u16@2，其余 48 字节原样保留；
//! - 三档全部对不上 ⇒ [`MapError::LengthMismatch`]，错误里列出三档期望长度与实际长度；
//! - **尺寸护栏先于长度判据**（三条，官方短路顺序）：`w == 0 || h == 0` ⇒ [`MapError::ZeroDimension`]、
//!   `w > 32767 || h > 32767`（[`MAX_DIMENSION`]）⇒ [`MapError::DimensionTooLarge`]、
//!   `w*h > 16_000_000`（[`MAX_CELLS`]）⇒ [`MapError::TooManyCells`]；
//! - **不看任何标题/魔法字节**，也不看扩展名 —— 长度等式是唯一判据。
//!
//! # 一·五、尺寸判据是**三条件合取**，不是「16M 格」单条
//!
//! 真值：CrystalStudio C# `MapDimensions.cs:11-12@acfbbf8a`：
//!
//! ```csharp
//! public static bool CanRoundTrip(int width, int height) => width is > 0 and <= short.MaxValue &&
//!     height is > 0 and <= short.MaxValue && (long)width * height <= MaximumCells;
//! ```
//!
//! ⇒ 合法尺寸 = `w>0 && h>0` **且** `w<=32767 && h<=32767` **且** `w*h<=16_000_000`。
//! 本 crate 把这三条实现成[`validate_dimensions`]（带原因）/ [`can_round_trip`]（布尔），
//! [`Map::parse`] 直接调前者，不另写一套判断。
//!
//! ⚠ **旧文档的「只有 16M 格上限」表述是错的，本次已订正**：`40000×1` 只有 40000 格，
//! 满足 16M 却违反 32767 —— 而 `.map` 头的宽高在 C# 侧是 `checked((short)Width)`
//! （`Document.cs:90`）写出的 **16 位有符号**字段，本仓按 `i16` 读回 ⇒ 这类图写盘后回读不回来。
//! 属于 round-trip 缺陷，不是「上限保守一点」的取舍。
//!
//! - **0 尺寸**：`w*h == 0` 时三档期望长度都等于 52（`52 + 0`），长度判据自己会命中 `Old12`
//!   ⇒ 若不先拦，52 字节全零的损坏文件会被**静默救活**成「0×0 空地图」，而且它仍然可保存。
//!   真值：客户端 `map_loader.gd:94-95`（`w <= 0 or h <= 0` ⇒ 报错）与 C# `LegacyMapReader.cs:50`
//!   （`Width <= 0 || Height <= 0` ⇒ 抛 `Invalid or excessive map dimensions.`）都拒绝。
//! - **16M 格**：真值取 C# `MapDimensions.cs:7` 的 `MaximumCells`（同值见
//!   `LegacyMapReader.cs:50` 的 `count > 16_000_000`、`Document.cs:58`）。
//!   客户端**没有**这条格数上限 ⇒ 这是本库有意的编辑器护栏：格结构体 36 B 定长，12 B/格的图
//!   内存约为文件的 3 倍，没有护栏时失败方式是 OOM 而不是明确报错。
//!   （`32767` 那条**不是**自造护栏，见上。）
//!
//! # 二、存储顺序：列优先
//!
//! ```text
//! offset = 52 + (x * h + y) * cell_size     // x 外层、y 内层
//! idx    = x * h + y                        // Map::cells 下标
//! ```
//!
//! # 三、`Ext36` 尾区（14..35，22 字节）**不可假设恒零**
//!
//! 实测（2026-11-04，本机真实资源）`client/热血传奇23周年客户端/Map/n0.map`
//! （700×700 = 490000 格）全格扫描：
//!
//! - **1660 格**尾区存在非零字节 = **0.3388%**；
//! - 其中 **1658 格**在 off18/19/20 非零，另有 2 格（`(342,275)`、`(343,275)`）
//!   仅在 off25..28 非零；
//! - 空间分布：`x ∈ [297..396]`、`y ∈ [199..303]`（斜向带，非孤立噪声）；
//! - 对照组 `n3.map` / `n4.map` / `NALX.map` 尾区全零。
//!
//! ⇒ 尾区语义未知，本库**只保存不解释**，写回时逐字节照抄，绝不重置为 0；
//! 也绝不据此推断字段含义或命名为 `reserved`/`flags` 之类的猜测名。
//!
//! # 四、用法
//!
//! ```no_run
//! use mir2_map::Map;
//!
//! let bytes = std::fs::read("n0.map").unwrap();
//! let map = Map::parse(&bytes).unwrap();
//! println!("{}x{} 档位={:?}", map.width, map.height, map.format);
//! assert_eq!(map.to_bytes(), bytes); // 往返保真
//! // 可走 = 背景层与前景层**都不**带 0x8000 位：
//! //     (wBkImg & 0x8000) == 0 && (wFrImg & 0x8000) == 0
//! // ⚠ 别照 C++ 写成 `((wBkImg & 0x8000) + (wFrImg & 0x8000)) == 0`：
//! //    C++ 靠 `unsigned short → int` 的整型提升相加才不溢出；Rust 的 `u16 + u16`
//! //    会回绕（0x8000 + 0x8000 == 0）⇒ 把「两层都阻挡」误判成可走。
//! //    实现见 cell.rs 的 `Cell::is_walkable`（用的就是 && 形式）。
//! let _可以走 = map.walkable(10, 20);
//! ```

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod cell;
mod error;
mod map;

pub use cell::{
    can_round_trip, expected_lengths, validate_dimensions, Cell, CellFormat, COMMON_CELL_LEN,
    EXT36_CELL_SIZE, HEADER_LEN, LIB_OFFSET, MAX_CELLS, MAX_DIMENSION, NEW14_CELL_SIZE,
    OLD12_CELL_SIZE, TAIL_LEN, TAIL_OFFSET,
};
pub use error::MapError;
pub use map::Map;

#[cfg(test)]
mod roundtrip_tests;
