//! 统一错误类型 [`FormatError`]。
//!
//! 设计原则（对齐「禁假数据」铁律）：
//! - **解析失败一律 `Err`**，绝不返回「半个地图」或静默补零；
//! - 每个变体都带足以定位问题的数值（偏移、期望长度、实际长度、宽高）；
//! - 只用 std（手写 [`core::fmt::Display`] + [`std::error::Error`]，不引 thiserror）。

use crate::model::FormatId;
use std::fmt;

/// 单张地图允许的最大格数（`w * h`）—— 它只是**三条件合取中的第三条**。
///
/// 口径来源（真值）：C# `MapDimensions.cs:7`（`MaximumCells = 16_000_000`）与
/// `MapDimensions.cs:11-12@acfbbf8a` 的 `CanRoundTrip` 第三条
/// （同值另见 `LegacyMapReader.cs:50` 的 `count > 16_000_000`、`Document.cs:58`）。
///
/// ⚠ **本常量不是完整的合法性判据**：`40000×1` 只有 40000 格却存不下去（单边超
/// [`MAX_DIMENSION`]）。判尺寸请用 [`crate::can_round_trip`]（布尔）/ 内部
/// `validate_dims` / `validate_write_dims`（带原因），它们共用同一份三条件判据。
///
/// 为什么必须放在**分配之前**判：本 crate 的 [`crate::Cell`] 带一个 `Vec<u8>` 尾区，
/// 12 B/格的真实文件解析后内存约为文件的 3~5 倍；没有上限时，一个假头部声明
/// 5000×5000 的坏文件会先去申请上百 MB 然后才报错（失败方式是 OOM 而不是明确错误）。
pub const MAX_CELLS: u64 = 16_000_000;

/// 单边维度上限（宽、高各自的最大值）= **32767** = `i16::MAX` = C# `short.MaxValue`。
///
/// 口径来源（真值）：C# `MapDimensions.cs:11-12@acfbbf8a` 的 `CanRoundTrip` 里
/// 两条 `<= short.MaxValue`：
///
/// ```csharp
/// public static bool CanRoundTrip(int width, int height) => width is > 0 and <= short.MaxValue &&
///     height is > 0 and <= short.MaxValue && (long)width * height <= MaximumCells;
/// ```
///
/// 为什么这是**往返缺陷**而不是「上限保守一点」：九种格式的头部宽高最终都落在
/// 16 位**有符号**字段上（`tmap_dims` 用 `i16_at` 读；C# `Document.cs:90` 用
/// `w.Write(checked((short)Width))` 写）。`32768..=65535` 这一段虽然塞得进 `u16`，
/// 写盘后要么在 C# 侧 `checked` 抛溢出，要么回读成负数 ⇒ 存下去就再也读不回来。
///
/// ⚠ 读写**两侧**都用它：读侧在 [`crate::model`] 的 `validate_dims` 里（`as u16` 转换之前），
/// 写侧在 `validate_write_dims` 里（由 `write_fixed` 与两个自定义 writer 调用）。
/// 只挂读侧会留下「保存侧仍能写出 > 32767 的宽高」的漏洞。
pub const MAX_DIMENSION: u16 = i16::MAX as u16;

/// 九种格式共用的错误枚举。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatError {
    /// 文件短于该格式能接受的最小头部长度。
    TooShort {
        /// 判定用的格式。
        format: FormatId,
        /// 实际字节数。
        len: usize,
        /// 至少需要多少字节才能读出该格式的头部。
        need: usize,
    },
    /// 宽或高不是正数。
    ///
    /// 真值：C# `LegacyMapReader.cs:50`（`Width <= 0 || Height <= 0` ⇒ 抛异常）。
    InvalidDimensions {
        /// 判定用的格式。
        format: FormatId,
        /// 头部里读到的宽（已按该格式的变换还原）。
        width: i32,
        /// 头部里读到的高（已按该格式的变换还原）。
        height: i32,
    },
    /// 宽或高超过 **32767**（`i16::MAX` = C# `short.MaxValue`）⇒ 存下去就回读不回来。
    ///
    /// 真值：C# `MapDimensions.cs:11-12@acfbbf8a` 的 `CanRoundTrip` 里两条
    /// `<= short.MaxValue`。本变体是这条判据在**读侧与写侧**共用的拒绝形态。
    ///
    /// 为什么必须显式拒绝而不是 `as u16` 一转了之：九种格式的头部宽高最终都是
    /// 16 位**有符号**字段（`tmap_dims` 用 `i16_at`；C# `Document.cs:90` 写
    /// `checked((short)Width)`）。`32768..=65535` 塞得进 `u16` 但回读不回来；
    /// 静默 `as u16` 更是把原始值改成另一个数（写出去的是别的尺寸）—— 属于数据损坏。
    ///
    /// 与另两个尺寸变体的分工（顺序即官方短路顺序）：非正 ⇒
    /// [`InvalidDimensions`](FormatError::InvalidDimensions)；单边 > 32767 ⇒ 本条；
    /// 格数 > 16_000_000 ⇒ [`TooManyCells`](FormatError::TooManyCells)。
    DimensionTooLarge {
        /// 判定用的格式。
        format: FormatId,
        /// 还原后的宽（读侧来自头部，写侧来自 [`crate::RawMap::width`]）。
        ///
        /// 用 `i64` 是为了同时容纳读侧的 `i32`（可能是负值或缺省）与写侧的 `u16`，
        /// 且不会在报错前就发生任何窄化转换。
        width: i64,
        /// 还原后的高。与 `width` 同口径。
        height: i64,
        /// 单边上限（[`MAX_DIMENSION`]，恒为 32767）。
        limit: u16,
    },
    /// 格数超过 [`MAX_CELLS`]。
    TooManyCells {
        /// 判定用的格式。
        format: FormatId,
        /// 宽。
        width: u16,
        /// 高。
        height: u16,
        /// `w * h`。
        cells: u64,
        /// 上限。
        limit: u64,
    },
    /// 文件长度不足以容纳「头部 + 格长 × 格数」。
    ///
    /// 对应 C# `ValidateDimensions` 的 `headerBytes + count * minimumCellBytes > Bytes.LongLength`
    /// 分支（`LegacyMapReader.cs:52-53`）。
    Truncated {
        /// 判定用的格式。
        format: FormatId,
        /// 实际字节数。
        len: usize,
        /// 该格式 + 该宽高所需的最小字节数。
        need: u64,
    },
    /// 文件长度**既不够也不正好**：定长格式要求 `len == 头部 + 格长 × 格数`。
    ///
    /// 这条是本 crate 相对 C# 的**收紧**：C# 的 `ValidateDimensions` 只查「够不够」（下界），
    /// 多出来的字节被静默丢掉 —— 这正是 Type3（曾误算 40 B/格，实为 36）、
    /// Type1（曾自造 17 B/格，实为 15）、Type100（变长）三类问题的共同根因。本 crate 要求精确对上；确实要读那种「长度多一截」
    /// 的文件，请显式走 [`crate::read_any_tolerant`]（多出的字节会进
    /// [`crate::RawMap::trailer`]，仍然字节精确）。
    LengthMismatch {
        /// 判定用的格式。
        format: FormatId,
        /// 头部里声明的宽。
        width: u16,
        /// 头部里声明的高。
        height: u16,
        /// 实际字节数。
        len: usize,
        /// 该格式 + 该宽高**精确**应有的字节数。
        expected: u64,
    },
    /// 已知格式的**未实现变体**（不猜、不硬读）。
    ///
    /// # 当前唯一的来源：`52 + 40·w·h` 的「40 B/格」长度形态 —— **无实证，仅防御**
    ///
    /// **真实语料里 0 个**：本机 1,158 个真实 `.map` 里，长度精确等于 `52 + 40·w·h`
    /// 的文件 **0 个**（`52 + 36·w·h` 的有 195 个）。
    ///
    /// **C# 实现本来就是 36 B/格，不存在「C# 按 40 读」这回事**（2026-11-05 订正）：
    /// `LegacyMapReader.cs:300` 与 `:303` 的 `ToInt16(Bytes, offset)` **不推进 `offset`**，
    /// 所以 `:303` 那 2 字节落在 `:304` 的 `offset += 14` 跳过区**内部**（rel 22-23 与
    /// 跳过区 22-35 重叠），一格合计 `2+2+2+1+1+1+1+1+1 + 2+7+1+14 = 36`；
    /// 与 `:280` 的 `ValidateDimensions(52, 36)` **完全自洽**。
    ///
    /// ⇒ 本变体保留**只为防御未来出现的怪文件**（长度恰好凑成 `52 + 40·w·h`），
    /// 绝不按 36 B 硬读（那会把整张图读成花图）。
    /// **它只在长度精确成立时命中，不会误伤任何 36 B/格文件。**
    UnsupportedVariant {
        /// 判定用的格式。
        format: FormatId,
        /// 人读的说明（含判定用的长度算式）。
        detail: String,
    },
    /// CrystalCs（Type100）版本号不在 `1..=5`。
    ///
    /// 真值：C# `LegacyMapReader.cs:567`（`if (ver < 1 || ver > 5) throw ...`）。
    UnsupportedVersion {
        /// 头部里读到的版本号。
        ver: u16,
    },
    /// 九种格式的判别全部不成立。
    UnknownFormat {
        /// 实际字节数。
        len: usize,
    },
    /// 待写入的 [`crate::RawMap`] 与目标格式不自洽（写前校验）。
    ///
    /// 典型原因：`cells.len() != w*h`、`header_raw` 长度不是该格式的头部长度、
    /// `Cell::tail` 长度不是该格式的固定尾区长度、字段值超出该格式的位宽。
    Inconsistent {
        /// 人读的说明。
        detail: String,
    },
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FormatError::TooShort { format, len, need } => write!(
                f,
                "{format:?}：文件太短，实际 {len} 字节，至少需要 {need} 字节才能读出头部"
            ),
            FormatError::InvalidDimensions {
                format,
                width,
                height,
            } => write!(
                f,
                "{format:?}：地图尺寸非法（w={width} h={height}），宽高必须为正数"
            ),
            FormatError::DimensionTooLarge {
                format,
                width,
                height,
                limit,
            } => write!(
                f,
                "{format:?}：地图尺寸超出可往返上限（w={width} h={height}，单边必须 ≤ {limit}\
                 = i16::MAX = C# short.MaxValue = 32767，真值见 MapDimensions.cs:11-12 \
                 @acfbbf8a 的 CanRoundTrip）；头部宽高是 16 位有符号字段，\
                 超过 32767 写盘后读不回来，本库不静默截断"
            ),
            FormatError::TooManyCells {
                format,
                width,
                height,
                cells,
                limit,
            } => write!(
                f,
                "{format:?}：格数超上限（{width}×{height} = {cells} > {limit}）"
            ),
            FormatError::Truncated { format, len, need } => write!(
                f,
                "{format:?}：格数据被截断，实际 {len} 字节，按头部宽高至少需要 {need} 字节"
            ),
            FormatError::LengthMismatch {
                format,
                width,
                height,
                len,
                expected,
            } => write!(
                f,
                "{format:?}：长度不自洽 —— {width}×{height} 精确应为 {expected} 字节，实际 {len} 字节\
                 （定长格式要求精确相等；要读这种「长度多一截」的文件请显式用 read_any_tolerant）"
            ),
            FormatError::UnsupportedVariant { format, detail } => {
                write!(f, "{format:?}：不支持的变体 —— {detail}")
            }
            FormatError::UnsupportedVersion { ver } => write!(
                f,
                "CrystalCs：不支持的版本号 {ver}（C# 只接受 1..=5）"
            ),
            FormatError::UnknownFormat { len } => write!(
                f,
                "无法判别格式：{len} 字节，九种格式的判别式全部不成立"
            ),
            FormatError::Inconsistent { detail } => write!(f, "待写入的地图与格式不自洽：{detail}"),
        }
    }
}

impl std::error::Error for FormatError {}

impl FormatError {
    /// 构造 [`FormatError::Inconsistent`] 的便捷入口。
    pub(crate) fn inconsistent(detail: impl Into<String>) -> FormatError {
        FormatError::Inconsistent {
            detail: detail.into(),
        }
    }

    /// 构造 [`FormatError::UnsupportedVariant`] 的便捷入口。
    pub(crate) fn unsupported(format: FormatId, detail: impl Into<String>) -> FormatError {
        FormatError::UnsupportedVariant {
            format,
            detail: detail.into(),
        }
    }
}
