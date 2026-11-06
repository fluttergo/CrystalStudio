//! 解析错误类型（手写 `Display` + `Error`，不引 thiserror）。

use std::fmt;

use crate::cell::{expected_lengths, HEADER_LEN};

/// `.map` 解析失败的原因。
///
/// 只覆盖「结构层面无法继续」的情况：长度不足、宽或高为 0、单边超 32767、
/// 格数超护栏、长度与三档格长都不匹配。
/// 本库对格内容不做任何语义校验（字段语义尚未确认，见 [`crate::cell`] 模块文档），
/// 因此不存在「字段非法」这一类错误 —— 解析失败必须显式返回 `Err`，禁止静默降级。
///
/// 三个尺寸变体（[`ZeroDimension`](MapError::ZeroDimension) /
/// [`DimensionTooLarge`](MapError::DimensionTooLarge) /
/// [`TooManyCells`](MapError::TooManyCells)）是 C# `MapDimensions.CanRoundTrip`
/// （`MapDimensions.cs:11-12@acfbbf8a`）三条件的一一对应，由
/// [`crate::validate_dimensions`] 按官方短路顺序择一返回。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MapError {
    /// 文件长度不足 52 字节，连 `TMapHeader` 都读不全。
    TooShort {
        /// 实际文件长度（字节）。
        len: usize,
    },
    /// 头部 `wWidth` 或 `wHeight` 为 0（含「52 字节全零」这种被截断/损坏的文件）。
    ///
    /// 真值：客户端 `map_loader.gd:94-95` 与 CrystalStudio 的 C#
    /// `LegacyMapReader.cs:50` 都判为错误并直接返回/抛异常。
    ///
    /// 为什么不能放行：`w*h == 0` 时三档期望长度都等于 52（`52 + 0`），长度判据本身
    /// 命中 `Old12` ⇒ 会把损坏文件**静默救活**成「0×0 空地图」，而编辑器仍认为它可保存，
    /// 用户一按保存就把「可修复的损坏」写成一张真正的空图。
    ZeroDimension {
        /// 头部读出的宽（实际值，用于定位问题文件）。
        width: u16,
        /// 头部读出的高（实际值）。
        height: u16,
    },
    /// 头部宽或高超过 **32767**（`i16::MAX` = C# `short.MaxValue`）⇒ 存下去就回读不回来。
    ///
    /// 真值：C# `MapDimensions.cs:11-12@acfbbf8a` 的 `CanRoundTrip`：
    ///
    /// ```csharp
    /// public static bool CanRoundTrip(int width, int height) => width is > 0 and <= short.MaxValue &&
    ///     height is > 0 and <= short.MaxValue && (long)width * height <= MaximumCells;
    /// ```
    ///
    /// 为什么这是**往返缺陷**而不是「上限太保守」：`.map` 头的 `wWidth`/`wHeight` 是
    /// **有符号 16 位**字段 —— C# `Document.cs:90` 写的是 `w.Write(checked((short)Width))`，
    /// 本仓 `mir2-format` 用 `i16_at` 读回。`32768..=65535` 这一段虽然塞得进 `u16`，
    /// 写盘后要么在 C# 侧 `checked` 抛溢出异常，要么回读成负数 ⇒ 地图存了打不开。
    ///
    /// 与另两个尺寸变体的分工（顺序即官方短路顺序）：0 尺寸 ⇒ [`ZeroDimension`](MapError::ZeroDimension)；
    /// 单边 > 32767 ⇒ 本条；格数 > 16_000_000 ⇒ [`TooManyCells`](MapError::TooManyCells)。
    DimensionTooLarge {
        /// 头部读出的宽（实际值，用于定位问题文件）。
        width: u16,
        /// 头部读出的高（实际值）。
        height: u16,
        /// 单边上限（[`crate::MAX_DIMENSION`]，恒为 32767）。
        limit: u16,
    },
    /// 头部声明的格数 `w*h` 超过 [`crate::MAX_CELLS`] 护栏。
    ///
    /// 真值：C# `MapDimensions.cs:7` 的 `MaximumCells = 16_000_000`，判据在
    /// `MapDimensions.cs:11-12@acfbbf8a` 的 `CanRoundTrip` 第三条
    /// （同值另见 `LegacyMapReader.cs:50` 的 `count > 16_000_000`、`Document.cs:58`）。
    /// 这不是客户端语义（`map_loader.gd` 无此上限），而是**编辑器护栏**：
    /// 本库的格结构体是 36 B 定长，12 B/格的图内存约为文件的 3 倍，超限就会变成 OOM 而不是明确报错。
    ///
    /// ⚠ 本变体**只**表示「格数超 16M」；单边超 32767 走
    /// [`DimensionTooLarge`](MapError::DimensionTooLarge)，两者不许混报。
    TooManyCells {
        /// 头部读出的宽。
        width: u16,
        /// 头部读出的高。
        height: u16,
        /// 头部声明的格数 `w*h`。
        cells: u64,
        /// 护栏上限（[`crate::MAX_CELLS`]）。
        limit: u64,
    },
    /// 文件长度既不等于 `52 + 12*w*h`、也不等于 `52 + 14*w*h`、也不等于 `52 + 36*w*h`。
    LengthMismatch {
        /// 头部读出的宽。
        width: u16,
        /// 头部读出的高。
        height: u16,
        /// 实际文件长度（字节）。
        len: u64,
        /// `Old12` 档期望长度。
        expect_old12: u64,
        /// `New14` 档期望长度。
        expect_new14: u64,
        /// `Ext36` 档期望长度。
        expect_ext36: u64,
    },
}

impl MapError {
    /// 按头部宽高与实际长度构造「长度不匹配」错误（三档期望值一次算齐）。
    pub(crate) fn length_mismatch(width: u16, height: u16, len: usize) -> MapError {
        let (expect_old12, expect_new14, expect_ext36) = expected_lengths(width, height);
        MapError::LengthMismatch {
            width,
            height,
            len: len as u64,
            expect_old12,
            expect_new14,
            expect_ext36,
        }
    }
}

impl fmt::Display for MapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MapError::TooShort { len } => write!(
                f,
                "地图长度不足：实际 {len} 字节，TMapHeader 至少需要 {HEADER_LEN} 字节"
            ),
            MapError::ZeroDimension { width, height } => write!(
                f,
                "地图尺寸非法：w={width} h={height} —— 宽和高都必须 > 0；\
                 客户端 map_loader.gd:94-95 与 C# LegacyMapReader.cs:50 同口径直接拒绝，\
                 本库不把 0 尺寸当成「合法空地图」"
            ),
            MapError::DimensionTooLarge {
                width,
                height,
                limit,
            } => write!(
                f,
                "地图尺寸超出可往返上限：w={width} h={height}，单边必须 ≤ {limit}\
                 （= 32767 = i16::MAX = C# short.MaxValue；真值见 MapDimensions.cs:11-12 \
                 @acfbbf8a 的 CanRoundTrip）；.map 头的宽高是 16 位有符号字段，\
                 超过 32767 写盘后回读不回来，本库不静默截断"
            ),
            MapError::TooManyCells {
                width,
                height,
                cells,
                limit,
            } => write!(
                f,
                "地图格数超出编辑器上限：w={width} h={height} 共 {cells} 格 > 上限 {limit} 格\
                 （C# MapDimensions.cs:7 的 MaximumCells，即 CanRoundTrip 三条件的第三条；\
                 同值见 LegacyMapReader.cs:50 的 16_000_000）；\
                 已在分配之前拒绝解析，避免内存放大到文件的数倍后 OOM"
            ),
            MapError::LengthMismatch {
                width,
                height,
                len,
                expect_old12,
                expect_new14,
                expect_ext36,
            } => write!(
                f,
                "地图长度与三档格长均不匹配：w={width} h={height} 格数={} 实际={len} 字节；\
                 期望 Old12(12B/格)={expect_old12} / New14(14B/格)={expect_new14} / \
                 Ext36(36B/格)={expect_ext36} 字节",
                u64::from(*width) * u64::from(*height)
            ),
        }
    }
}

impl std::error::Error for MapError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 错误信息含三档期望长度() {
        let err = MapError::length_mismatch(3, 3, 161);
        let msg = err.to_string();
        assert!(msg.contains("160"), "缺少 Old12 期望长度 160：{msg}");
        assert!(msg.contains("178"), "缺少 New14 期望长度 178：{msg}");
        assert!(msg.contains("376"), "缺少 Ext36 期望长度 376：{msg}");
        assert!(msg.contains("161"), "缺少实际长度 161：{msg}");
        assert!(msg.contains("Old12") && msg.contains("New14") && msg.contains("Ext36"));
    }

    #[test]
    fn 长度不足错误信息() {
        let err = MapError::TooShort { len: 51 };
        let msg = err.to_string();
        assert!(msg.contains("51"), "{msg}");
        assert!(msg.contains("52"), "{msg}");
    }

    #[test]
    fn 零尺寸错误信息带实际宽高() {
        // 三种形态各自的错误信息必须能看出「到底是哪个维度为 0」，便于定位坏文件
        for (w, h, 应含w, 应含h) in [
            (0u16, 0u16, "w=0", "h=0"),
            (0, 7, "w=0", "h=7"),
            (7, 0, "w=7", "h=0"),
        ] {
            let msg = MapError::ZeroDimension {
                width: w,
                height: h,
            }
            .to_string();
            assert!(msg.contains(应含w), "{w}×{h} 信息缺「{应含w}」：{msg}");
            assert!(msg.contains(应含h), "{w}×{h} 信息缺「{应含h}」：{msg}");
            assert!(msg.contains("LegacyMapReader.cs:50"), "{msg}");
        }
    }

    #[test]
    fn 超格数错误信息带格数与上限() {
        let msg = MapError::TooManyCells {
            width: 5000,
            height: 5000,
            cells: 25_000_000,
            limit: 16_000_000,
        }
        .to_string();
        for 片段 in ["5000", "25000000", "16000000", "LegacyMapReader.cs:50"] {
            assert!(msg.contains(片段), "信息缺「{片段}」：{msg}");
        }
    }

    /// 尺寸超单边上限的信息必须**点名 32767 与 C# `CanRoundTrip`**，
    /// 否则用户看不出「为什么 40000 格的小图也不让存」。
    #[test]
    fn 尺寸超限错误信息点名32767与官方判据() {
        let msg = MapError::DimensionTooLarge {
            width: 65535,
            height: 1,
            limit: 32767,
        }
        .to_string();
        for 片段 in ["65535", "32767", "i16::MAX", "short.MaxValue", "CanRoundTrip"] {
            assert!(msg.contains(片段), "信息缺「{片段}」：{msg}");
        }
        // 必须说清「这不是长度问题」：长度/格数由另两个变体负责
        assert!(!msg.contains("长度"), "不该被描述成长度问题：{msg}");
        assert!(!msg.contains("格数"), "不该被描述成格数问题：{msg}");
    }

    #[test]
    fn 可当标准错误装箱() {
        // 确认实现了 std::error::Error（可参与 Box<dyn Error> 链路）
        let e: Box<dyn std::error::Error> = Box::new(MapError::TooShort { len: 0 });
        assert!(e.to_string().contains("0"));
    }
}
