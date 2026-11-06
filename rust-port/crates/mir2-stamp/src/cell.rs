//! 单格结构与逐字段编解码（`CellCodec.Write` 的 `map: false` 分支 + `LoadStamp` 的格体读取）。
//!
//! 真值：CrystalStudio `Document.cs`（只读参照，未改动）
//! - `Front` record：`:6`
//! - `Cell` record 字段清单：`:7-32`
//! - `StampCell` record：`:81`
//! - `CellCodec.Write(BinaryWriter, Cell, bool map)`：`:84-98`
//!   - 四个共用字段：`:86`
//!   - **`map == true`（地图）分支**：`:87-93`
//!   - **`map == false`（印章）分支**：`:94-96`
//! - `CellCodec.LoadStamp(Stream, ...)` 格体读取：`:117-122`
//!
//! # 一、印章格体（**v2，`map: false`**）逐字段偏移
//!
//! 偏移相对**单格起点**（即 `SaveStamp` 写完 X/Y 之后紧接着的那个字节，`Document.cs:145`）。
//! 所有整数均为**小端**（`BinaryWriter` 默认 `Encoding.UTF8` 只影响 `Write(char)`，
//! 本格式只写定长数值，故一律 LE）。
//!
//! | 格内偏移 | 长度 | 字段 | C# 类型 | 写入真值 |
//! |---------:|-----:|------|---------|----------|
//! | 0  | 4 | `X`             | `int`    | `:145` `w.Write(c.X)` |
//! | 4  | 4 | `Y`             | `int`    | `:145` `w.Write(c.Y)` |
//! | 8  | 2 | `BackLibrary`   | `short`  | `:86` |
//! | 10 | 4 | `BackImage`     | `int`    | `:86` |
//! | 14 | 2 | `MiddleLibrary` | `short`  | `:86` |
//! | 16 | 2 | `MiddleImage`   | `ushort` | `:86` |
//! | 18 | 1 | 前景个数        | `byte`   | `:94` `(byte)c.ForegroundImages.Count` |
//! | 19 | 2 | 前景 `Library`  | `short`  | `:95` |
//! | 21 | 2 | 前景 `Image`    | `short`  | `:95` |
//! | 23 | 1 | 前景 `Frames`   | `byte`   | `:95` |
//! | 24 | 1 | 前景 `Tick`     | `byte`   | `:95` |
//! | 25 | 1 | `Door`          | `byte`   | `:96` |
//! | 26 | 1 | `DoorOffset`    | `byte`   | `:96` |
//! | 27 | 1 | `MiddleFrames`  | `byte`   | `:96` |
//! | 28 | 1 | `MiddleTick`    | `byte`   | `:96` |
//! | 29 | 2 | `TileAnimation` | `short`  | `:96` |
//! | 31 | 2 | `TileOffset`    | `short`  | `:96` |
//! | 33 | 1 | `TileFrames`    | `byte`   | `:96` |
//! | 34 | 1 | `Light`         | `byte`   | `:96` |
//!
//! ⇒ **无前景 = 29 字节/格**（`0..29`）；**有前景 = 35 字节/格**（`0..35`，前景块 6 字节）。
//! 与 `Document.cs:110` 的 `minimumCellBytes = version == 2 ? 29` 自洽。
//!
//! # 二、`map: true`（地图）分支为什么**不能**套用到印章上
//!
//! `Document.cs:87-93` 的地图分支是**固定 8 字节前景槽**（`Library i16`、`Image i16`、
//! `Door`、`DoorOffset`、`Frames`、`Tick` 交错写入，且**没有前景个数字节**），
//! 而印章分支（`:94-96`）是「**先写个数，再写 6 字节前景块，然后把 `Door/DoorOffset`
//! 挪到前景块之后**」。两者字节顺序**不同**，把地图的写法套到印章上会立刻错位。
//!
//! # 三、v0 / v1 的历史格体（**只读**，本库永不写出）
//!
//! 读取器（`:110`、`:118-121`）按版本走三条不同路径，因此**同一个 `Cell` 在三个版本里
//! 字节布局都不一样**。以下偏移同样相对格起点（X/Y 之后）：
//!
//! - **v0**（`:118` `fronts = version == 0 ? 1 : ...` ⇒ **没有个数字节，恒按 1 个前景读**）
//!   ```text
//!   0..2  BackLibrary(i16)   2..6  BackImage(i32)   6..8  MiddleLibrary(i16)   8..10 MiddleImage(u16)
//!   10..12 前景 Library(i16) 12..14 前景 Image(i16)          ← 只有库号+图号，没有 Frames/Tick
//!   14 Door   15 DoorOffset
//!   16 frames 17 tick        ← `:121` 的 `if (version < 2)` 分支：前景动画帧数/节拍写在这里
//!   18 MiddleFrames  19 MiddleTick  20..22 TileAnimation(i16)  22..24 TileOffset(i16)
//!   24 TileFrames    25 Light
//!   ```
//!   格体 26 字节 + X/Y 8 字节 = **34 字节/格**（= `:110` 的 `version == 0 ? 34`）。
//! - **v1**（有个数字节，但前景块只有 4 字节；Frames/Tick 仍在 `Door/DoorOffset` 之后）
//!   ```text
//!   0..10 同 v2 前四字段   10 前景个数(byte)
//!   11..13 前景 Library(i16)   13..15 前景 Image(i16)
//!   15 Door   16 DoorOffset   17 frames   18 tick
//!   19 MiddleFrames  20 MiddleTick  21..23 TileAnimation  23..25 TileOffset  25 TileFrames  26 Light
//!   ```
//!   无前景时格体 23 字节 + 8 = **31 字节/格**（= `:110` 的 `version == 1 ? 31`）；
//!   有前景时前景块只有 4 字节（**v1 没有 `Frames`/`Tick`**）⇒ **35 字节/格**。
//!
//! **⚠ v0/v1 不可能与原文逐字节往返**：`SaveStamp`（`:137-147`）**只写 v2**，
//! 重新保存会把格体重新排版（前景块的 Frames/Tick 从 `Door` 之后挪到前景块内部）。
//! 这不是缺陷，是 C# 的既定归一化行为（`Document.cs` 注释：读取兼容 v0/v1/v2，保存统一 v2）。
//! 本库照抄该行为，并在 `roundtrip_tests` 里给出「v0/v1 只能语义往返、不能字节往返」的实测证据。
//!
//! # 四、位语义：本库**只保存不解释**
//!
//! `BackImage` 的高位（`0x60000000` / `0x1FFFFFFF` 掩码）与前景 `Image` 的 `0x8000` 位
//! 在 CrystalStudio 里被大量分支当作「阻挡 / 归属 / 变体」标记使用
//! （例：`NativeCoastSupport.cs:161`、`RiverBridgeWalkSupport.cs:31`、`StampIdentifier.cs:84`），
//! `CrystalStudio/README.md:205-207` 也把 `0x20000000`/`0x40000000` 解释为低/高阻挡、
//! 前景取 `Image & 0x7FFF`。但那是**编辑器约定**，不是文件格式约束。
//! 本库一个位都不碰、也不做任何归一化。

use crate::error::StampError;

/// 单格前景图，对应 C# `public record Front(short Library, short Image, byte Frames = 0, byte Tick = 0)`
/// （`Document.cs:6`）。
///
/// `Cell.Foreground` 是**可空**的（`Document.cs:14` `public Front? Foreground`），
/// 而 `ForegroundImages.Count`（`ForegroundImages.cs:13`）的定义是
/// `Value is null ? 0 : 1` —— **与 `Image` 是否为 0 无关**。
/// ⇒ 写回时「有没有前景块」只由 [`StampCellData::foreground`] 是否为 `Some` 决定，
/// 绝不能拿 `image == 0` 当「没有前景」的判据（那会把 `fronts=1, image=0` 的格写坏）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Front {
    /// 前景图库号（`short`，`Document.cs:6` 的 `Library`）。
    pub library: i16,
    /// 前景图序号（`short`，`Document.cs:6` 的 `Image`）。
    ///
    /// 高位 `0x8000` 在编辑器里被当标记位用（见模块文档 §四），本库不解释。
    pub image: i16,
    /// 前景动画帧数（`byte`，`Document.cs:6` 的 `Frames`；v2 起存进前景块，v0/v1 存在 `Door` 之后）。
    pub frames: u8,
    /// 前景动画节拍（`byte`，`Document.cs:6` 的 `Tick`；存放位置同 `frames`）。
    pub tick: u8,
}

/// 印章单格的数据体，字段与 C# `Cell`（`Document.cs:7-32`）**一一对应、不多不少**。
///
/// 字段顺序即**写出顺序**（`CellCodec.Write(..., map: false)`，`Document.cs:86/94-96`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct StampCellData {
    /// 背景层图库号（`short`，`Document.cs:9`）。
    pub back_library: i16,
    /// 背景层图序号（`int`，`Document.cs:10`）。
    pub back_image: i32,
    /// 中间层图库号（`short`，`Document.cs:11`）。
    pub middle_library: i16,
    /// 中间层图序号（`ushort`，`Document.cs:12`）。
    pub middle_image: u16,
    /// 前景图（`Front?`，`Document.cs:14`）。`None` ⇒ 写 `fronts = 0` 且不写前景块。
    pub foreground: Option<Front>,
    /// 门类型（`byte`，`Document.cs:17`）。
    pub door: u8,
    /// 门偏移（`byte`，`Document.cs:18`）。
    pub door_offset: u8,
    /// 中间层动画帧数（`byte`，`Document.cs:19`）。
    pub middle_frames: u8,
    /// 中间层动画节拍（`byte`，`Document.cs:20`）。
    pub middle_tick: u8,
    /// 瓦片动画图号（`short`，`Document.cs:21`）。
    pub tile_animation: i16,
    /// 瓦片动画偏移（`short`，`Document.cs:22`）。
    pub tile_offset: i16,
    /// 瓦片动画帧数（`byte`，`Document.cs:23`）。
    pub tile_frames: u8,
    /// 光照（`byte`，`Document.cs:24`）。
    pub light: u8,
}

/// `.X` 印章文件头两字节魔数（`Document.cs:144` `w.Write((byte)'C'); w.Write((byte)'#');`）。
pub const MAGIC: [u8; 2] = *b"C#";

/// 本库**写出**的版本号（`Document.cs:144` `w.Write((byte)2)`）。
///
/// 读取侧兼容 v0/v1/v2（`Document.cs:108`），写出侧**只有 v2**。
pub const WRITE_VERSION: u8 = 2;

/// 文件头长度：`'C'`(1) + `'#'`(1) + 版本(1) + 格数 int32(4) = **7 字节**。
pub const HEADER_LEN: usize = 7;

/// v2 单格**无前景**时的字节数（含 X/Y 各 4 字节）。
///
/// 推导：X 4 + Y 4 + `BackLibrary` 2 + `BackImage` 4 + `MiddleLibrary` 2 + `MiddleImage` 2
/// + 前景个数 1 + `Door` 1 + `DoorOffset` 1 + `MiddleFrames` 1 + `MiddleTick` 1
/// + `TileAnimation` 2 + `TileOffset` 2 + `TileFrames` 1 + `Light` 1 = **29**。
pub const CELL_BASE_LEN: usize = 29;

/// 前景块字节数：`Library` 2 + `Image` 2 + `Frames` 1 + `Tick` 1 = **6**（`Document.cs:95`）。
pub const FRONT_LEN: usize = 6;

/// 格数护栏上限（`Document.cs:111` 读侧与 `:140` 写侧共用同一个 `1_000_000`）。
pub const MAX_CELLS: usize = 1_000_000;

/// 给定版本的**单格最小字节**（含 X/Y），直接照抄 `Document.cs:110`：
/// `int minimumCellBytes = version == 0 ? 34 : version == 1 ? 31 : 29;`
///
/// 三个数都等于「该版本**无前景**格」的长度 ⇒ 它是**下界**，预检通过不代表文件完整。
#[must_use]
pub const fn minimum_cell_bytes(version: u8) -> usize {
    match version {
        0 => 34,
        1 => 31,
        _ => 29,
    }
}

/// 从 `bytes` 的 `pos` 处取 `need` 字节并前移 `pos`；不够就报 [`StampError::CellTruncated`]。
///
/// 对应 C# 侧 `BinaryReader.ReadByte/ReadInt16/ReadInt32` 在 EOF 抛 `EndOfStreamException`
/// （`Document.cs:117-122`）。**绝不返回不足长度的切片、绝不补零**。
pub(crate) fn take<'a>(
    bytes: &'a [u8],
    pos: &mut usize,
    need: usize,
    index: usize,
) -> Result<&'a [u8], StampError> {
    let end = match pos.checked_add(need) {
        Some(end) => end,
        None => {
            return Err(StampError::CellTruncated {
                index,
                offset: *pos,
                need,
                len: bytes.len(),
            })
        }
    };
    if end > bytes.len() {
        return Err(StampError::CellTruncated {
            index,
            offset: *pos,
            need,
            len: bytes.len(),
        });
    }
    let slice = &bytes[*pos..end];
    *pos = end;
    Ok(slice)
}

/// 把已保证长度的切片按小端解成 `i16`（切片长度必须 ≥ 2，由 [`take`] 保证）。
fn i16_at(s: &[u8]) -> i16 {
    i16::from_le_bytes([s[0], s[1]])
}

/// 把已保证长度的切片按小端解成 `u16`（切片长度必须 ≥ 2，由 [`take`] 保证）。
fn u16_at(s: &[u8]) -> u16 {
    u16::from_le_bytes([s[0], s[1]])
}

/// 把已保证长度的切片按小端解成 `i32`（切片长度必须 ≥ 4，由 [`take`] 保证）。
pub(crate) fn i32_at(s: &[u8]) -> i32 {
    i32::from_le_bytes([s[0], s[1], s[2], s[3]])
}

/// 读一格的数据体，严格照抄 `CellCodec.LoadStamp` 的 `:117-122`。
///
/// `pos` 进来时指向该格的**格体起点**（X/Y 之后），返回时指向下一格的起点。
pub(crate) fn read_cell(
    bytes: &[u8],
    pos: &mut usize,
    version: u8,
    index: usize,
) -> Result<StampCellData, StampError> {
    // :117 —— `BackLibrary(i16) BackImage(i32) MiddleLibrary(i16) MiddleImage(u16)`
    let head = take(bytes, pos, 10, index)?;
    let mut cell = StampCellData {
        back_library: i16_at(&head[0..2]),
        back_image: i32_at(&head[2..6]),
        middle_library: i16_at(&head[6..8]),
        middle_image: u16_at(&head[8..10]),
        ..StampCellData::default()
    };

    // :118 —— `int fronts = version == 0 ? 1 : r.ReadByte();`
    // v0 **没有**个数字节，恒按「有 1 个前景」读。
    let fronts = if version == 0 {
        1u8
    } else {
        take(bytes, pos, 1, index)?[0]
    };
    // :119 —— `if (fronts > 1) throw new InvalidDataException(...)`
    if fronts > 1 {
        return Err(StampError::MultipleForegrounds { index, fronts });
    }
    if fronts == 1 {
        // :120 —— 前景 `Library(i16) Image(i16)`；v2 起紧跟 `Frames(byte) Tick(byte)`
        let f = take(bytes, pos, 4, index)?;
        let mut front = Front {
            library: i16_at(&f[0..2]),
            image: i16_at(&f[2..4]),
            frames: 0,
            tick: 0,
        };
        if version >= 2 {
            let ft = take(bytes, pos, 2, index)?;
            front.frames = ft[0];
            front.tick = ft[1];
        }
        cell.foreground = Some(front);
    }

    // :121 —— `Door(byte) DoorOffset(byte)`；随后是 v<2 的 `frames/tick` 双字节
    let door = take(bytes, pos, 2, index)?;
    cell.door = door[0];
    cell.door_offset = door[1];
    if version < 2 {
        // :121 —— `byte frames = r.ReadByte(), tick = r.ReadByte(); if (c.Foreground is { } front) ...`
        // ⚠ 这两个字节**无论有没有前景都会读**；没有前景时 C# 直接丢弃它们。
        let ft = take(bytes, pos, 2, index)?;
        if let Some(front) = cell.foreground.as_mut() {
            front.frames = ft[0];
            front.tick = ft[1];
        }
    }

    // :122 —— `MiddleFrames(byte) MiddleTick(byte) TileAnimation(i16) TileOffset(i16) TileFrames(byte) Light(byte)`
    let tail = take(bytes, pos, 8, index)?;
    cell.middle_frames = tail[0];
    cell.middle_tick = tail[1];
    cell.tile_animation = i16_at(&tail[2..4]);
    cell.tile_offset = i16_at(&tail[4..6]);
    cell.tile_frames = tail[6];
    cell.light = tail[7];

    Ok(cell)
}

/// 写一格的数据体（**v2、`map: false`**，`CellCodec.Write` 的 `:86/94-96`）。
///
/// 顺序严格照抄：四个共用字段 → 前景个数 → 前景块 → `Door/DoorOffset` →
/// `MiddleFrames/MiddleTick/TileAnimation/TileOffset/TileFrames/Light`。
pub(crate) fn write_cell(out: &mut Vec<u8>, cell: &StampCellData) {
    // :86
    out.extend_from_slice(&cell.back_library.to_le_bytes());
    out.extend_from_slice(&cell.back_image.to_le_bytes());
    out.extend_from_slice(&cell.middle_library.to_le_bytes());
    out.extend_from_slice(&cell.middle_image.to_le_bytes());

    // :94 —— 个数只可能是 0 或 1（`ForegroundImages.Count` 的定义，见 `ForegroundImages.cs:13`）
    match &cell.foreground {
        Some(front) => {
            out.push(1);
            // :95
            out.extend_from_slice(&front.library.to_le_bytes());
            out.extend_from_slice(&front.image.to_le_bytes());
            out.push(front.frames);
            out.push(front.tick);
        }
        None => out.push(0),
    }

    // :96
    out.push(cell.door);
    out.push(cell.door_offset);
    out.push(cell.middle_frames);
    out.push(cell.middle_tick);
    out.extend_from_slice(&cell.tile_animation.to_le_bytes());
    out.extend_from_slice(&cell.tile_offset.to_le_bytes());
    out.push(cell.tile_frames);
    out.push(cell.light);
}

/// 该格按 v2 印章格式写出的**整格**字节数（**含 X/Y 各 4 字节**，与「29 / 35 字节每格」同口径）：
/// 无前景 [`CELL_BASE_LEN`] = 29，有前景 29 + [`FRONT_LEN`] = 35。
#[must_use]
pub fn cell_byte_len(cell: &StampCellData) -> usize {
    CELL_BASE_LEN
        + if cell.foreground.is_some() {
            FRONT_LEN
        } else {
            0
        }
}
