//! 解析/序列化错误类型（手写 `Display` + `std::error::Error`，不引 thiserror）。
//!
//! 每个变体都对应 CrystalStudio `Document.cs` 里的一处**显式抛错点**或一处
//! **会抛 `EndOfStreamException` 的越界读**，不做任何「猜一个默认值顶上」的降级：
//! 本库的纪律是「读不懂就 `Err`」，绝不静默截断、绝不用 0 填充糊过去。

use std::fmt;

/// `.X` 印章读写失败的原因。
///
/// 真值来源逐条标注 CrystalStudio `Document.cs` 行号（只读参照，未改动）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StampError {
    /// 文件长度连「头 + 4 字节格数」都不够。
    ///
    /// C# 侧对应 `r.ReadInt32()`（`Document.cs:109`）抛 `EndOfStreamException`。
    HeaderTruncated {
        /// 实际文件长度（字节）。
        len: usize,
        /// 至少要这么长才能读到格数。
        need: usize,
    },
    /// 版本号大于 2（`Document.cs:108`：`if (version > 2) throw new InvalidDataException(...)`）。
    UnsupportedVersion {
        /// 文件头第 3 字节读出的版本号（原值，便于定位坏文件）。
        version: u8,
    },
    /// 格数为负或超过 [`crate::MAX_CELLS`]（`Document.cs:111` 的 `count < 0 || count > 1_000_000`）。
    InvalidCellCount {
        /// 文件里读出的格数（原值）。
        count: i32,
        /// 护栏上限（[`crate::MAX_CELLS`]）。
        limit: usize,
    },
    /// 格数 × 单格最小字节 > 剩余字节（`Document.cs:110-112` 的预检）。
    ///
    /// 这是 C# 的**廉价预检**：它只用「无前景格」的最小长度算下界，
    /// 因此预检通过并不代表文件完整 —— 后续逐格读取仍可能越界
    /// （那种情况报 [`StampError::CellTruncated`]）。
    TruncatedCellData {
        /// 文件里读出的格数。
        count: i32,
        /// 该版本的单格最小字节（v0=34 / v1=31 / v2=29）。
        minimum_cell_bytes: usize,
        /// `count × minimum_cell_bytes`。
        required: u64,
        /// 读完格数后剩余的字节数。
        available: u64,
    },
    /// 逐字段读取时越界（C# 侧对应 `r.ReadByte()/ReadInt16()/ReadInt32()` 抛 `EndOfStreamException`）。
    CellTruncated {
        /// 第几格（0 起）。
        index: usize,
        /// 越界读的起始偏移（文件绝对偏移）。
        offset: usize,
        /// 本次想读多少字节。
        need: usize,
        /// 文件总长度。
        len: usize,
    },
    /// 单格前景个数 > 1（`Document.cs:119`：
    /// `if (fronts > 1) throw new InvalidDataException("An object cell supports one foreground image.")`）。
    MultipleForegrounds {
        /// 第几格（0 起）。
        index: usize,
        /// 文件里读出的前景个数（原值）。
        fronts: u8,
    },
    /// 序列化时格数超过护栏（`Document.cs:140`：`if (list.Count > 1_000_000) throw ...`）。
    ///
    /// 只可能出现在**手工构造**的 [`crate::Stamp`] 上：`parse` 自己就会拦住 > 1,000,000 的文件。
    TooManyCells {
        /// 待写出的格数。
        count: usize,
        /// 护栏上限（[`crate::MAX_CELLS`]）。
        limit: usize,
    },
}

impl fmt::Display for StampError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StampError::HeaderTruncated { len, need } => write!(
                f,
                "印章文件太短：只有 {len} 字节，连「头 + 4 字节格数」都不够（至少 {need} 字节）"
            ),
            StampError::UnsupportedVersion { version } => write!(
                f,
                "未知的印章版本号 {version}：CrystalStudio 只认 v0/v1/v2（Document.cs:108）"
            ),
            StampError::InvalidCellCount { count, limit } => write!(
                f,
                "印章格数非法：{count}（合法范围 0..={limit}，Document.cs:111）"
            ),
            StampError::TruncatedCellData {
                count,
                minimum_cell_bytes,
                required,
                available,
            } => write!(
                f,
                "印章数据被截断：{count} 格 × 单格最小 {minimum_cell_bytes} 字节 = 需要 {required} 字节，实际只剩 {available} 字节（Document.cs:111）"
            ),
            StampError::CellTruncated {
                index,
                offset,
                need,
                len,
            } => write!(
                f,
                "第 {index} 格读取越界：偏移 {offset} 处想读 {need} 字节，但文件总长只有 {len} 字节"
            ),
            StampError::MultipleForegrounds { index, fronts } => write!(
                f,
                "第 {index} 格声明了 {fronts} 个前景图：印章单格只允许 1 个（Document.cs:119）"
            ),
            StampError::TooManyCells { count, limit } => write!(
                f,
                "印章格数 {count} 超过写出护栏 {limit}（Document.cs:140）"
            ),
        }
    }
}

impl std::error::Error for StampError {}
