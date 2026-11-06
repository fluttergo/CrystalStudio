//! `.X` 印章 → [`StampCellInput`] 的**唯一**转换口。
//!
//! 上游真值：`mir2-stamp/src/cell.rs` 的字段表（C# `Document.cs:7-32` 的 `Cell` 字段清单 +
//! `:84-98` 的 `CellCodec.Write(..., map: false)` 印章分支）；下游结构见 [`StampCellInput`]。
//!
//! # 一、为什么把这一步收进内核
//!
//! 2026-11-05 之前，「印章 id → 格表」由**每个调用方各写一份**（`miredit-gui/src/stamp_place.rs`、
//! `miredit-web/src/routes/stamps.rs`、以及本 crate 的 `tests/stamp_place.rs` 里那个 `map` 闭包）。
//! 三份实现各自决定「图号掩不掩 `0x7FFF`」「库号 `as u8` 还是报错」「五个视图装不下的字段填不填」，
//! ⇒ 同一个 `.X` 在不同入口能转出**不同的格表**，而且**没有任何机制**能发现这种漂移。
//!
//! 现在只有一个入口（本模块）。调用方只负责**读字节**（`std::fs::read` +
//! [`mir2_stamp::Stamp::parse`]），掩码 / 报错 / 字段搬运的口径全部钉在这里。
//!
//! # 二、字段映射（逐条可对照）
//!
//! | [`StampCellInput`] | `.X` 侧 `StampCellData` | 处理 |
//! |---|---|---|
//! | `dx` / `dy` | `StampCell::x` / `.y`（`i32`，**可负**） | 原样 |
//! | `bk` | `back_image`（`i32`） | 取低 16 位再 `& 0x7FFF`（见 §三） |
//! | `mid` | `middle_image`（`u16`） | `& 0x7FFF` |
//! | `fr` | `foreground.image`（`i16`） | 按位转 `u16` 再 `& 0x7FFF` |
//! | `bk_lib` | `back_library`（`i16`） | 必须落在 `0..=255`，否则报错（见 §四） |
//! | `mid_lib` | `middle_library`（`i16`） | 同上 |
//! | `fr_lib` | `foreground.library`（`i16`） | 同上；**无前景 ⇒ `0`** |
//! | `door` / `door_offset` | `door` / `door_offset`（`u8`） | 原样 |
//! | `light` | `light`（`u8`） | 原样 |
//! | `ani_frame` / `ani_tick` | `foreground.frames` / `.tick`（`u8`） | 原样；**无前景 ⇒ `0`** |
//! | `mid_ani_frame` / `mid_ani_tick` | `middle_frames` / `middle_tick`（`u8`） | 原样 |
//! | `tile_animation` / `tile_offset` | `tile_animation` / `tile_offset`（`i16`） | 按位转 `u16` |
//! | `tile_frames` | `tile_frames`（`u8`） | 原样 |
//!
//! ## 关于「若 `.X` 无对应字段就留 0」的那五个
//!
//! 任务书给 `mid_ani_frame` / `mid_ani_tick` / `tile_animation` / `tile_offset` / `tile_frames`
//! 留了退路：「`.X` 无对应字段就留 0 并注释」。**实测 `.X` 全都有对应字段**
//! （`mir2-stamp/src/cell.rs` 的 `StampCellData` 字段表；v2 格体偏移 `27..34`）
//! ⇒ 本模块**照搬真值、不填 0**。
//!
//! 它们搬不动的其实是**下游**：`mir2_map::Cell`（12/14/36 三档）没有这五个字段的位置，
//! 所以 `crate::stamp::plan_stamp` 会在有非 0 值时给一句中文说明（`convert.rs` 的 `LOST_IN_VIEW`）。
//! 这里若提前填 0，连「如实报告」都做不到 —— 那才是静默丢数据。
//!
//! # 三、三族图号一律 `& 0x7FFF`
//!
//! `bit15` 在 CrystalStudio 里是**阻挡/标记位**，不是图号的一部分：
//! 前景取 `Image & 0x7FFF` 见 `CrystalStudio/README.md:205-207` 与 `StampIdentifier.cs:84`；
//! 地表 `BackImage` 的高位（`0x20000000` / `0x40000000` = 低/高阻挡）更是在**第 29 位以上**。
//! ⇒ 三族统一掩掉 `bit15`，与 `crate::paint` 的写入口径（`v & 0x7FFF`）对齐，
//! 免得「转换时带进来的标记位」被 `plan_stamp` 当成图号写进地图。
//!
//! ⚠ 真实样本 `mir_00000.X` 的 25 格里 21 格 `back_image = 0x60000000`（**只有标记位、图号为 0**）：
//! 不掩码就会把 `0x60000000` 截成 `0x0000`（恰好也是 0）—— 掩不掩在这一格看不出差别，
//! 所以本模块的口径由 `stamp_convert` 的单元测试逐字段钉住，而不是靠这个样本自证。
//!
//! # 四、库号 > 255 一律报错，**绝不截断**
//!
//! `.X` 的库号是 `i16`（`short`），而地图侧落的是**单字节**（`btTiles` / `btsmTiles` / `btArea`）。
//! 截断（`as u8`）会把 `300` 静默变成 `44` ⇒ 贴出来的印章指向**另一个图库**，
//! 外观错、还不报错。这类「看起来成功」的错误正是本项目零容忍的假数据。
//! ⇒ `0..=255` 之外（含负数：`-1 as u8 == 255`）一律 [`StampConvertError::LibraryOutOfRange`]，
//! 消息含「第 N 格 / 地表|中层|物体 / 值」，`N` **从 1 起**（与界面「第 N 格」口径一致）。

use std::fmt;

use mir2_stamp::{Stamp, StampCellData};

use crate::stamp::StampCellInput;

/// 三族库号（地表 / 中层 / 物体）—— 只用于「哪一族的库号越界」这句话。
///
/// 三族的落点见 [`StampCellInput`] 的字段文档：`bk_lib` → `btTiles`、`mid_lib` → `btsmTiles`、
/// `fr_lib` → `btArea`。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StampLibLayer {
    /// 地表库号（`.X` 的 `BackLibrary`）。
    Ground,
    /// 中层库号（`.X` 的 `MiddleLibrary`）。
    Middle,
    /// 物体库号（`.X` 的 `Foreground.Library`）。
    Front,
}

impl StampLibLayer {
    /// 中文族名，**逐字**取 `地表` / `中层` / `物体`（错误消息的固定格式，别改）。
    ///
    /// ⚠ 与 `crate::layer::NAME_GROUND`（`"地表图层"`）**不是**同一个词表：那三个是界面标题，
    /// 这里要的是能嵌进「第 N 格 / 地表 / 值」这句话的短名。
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ground => "地表",
            Self::Middle => "中层",
            Self::Front => "物体",
        }
    }
}

/// [`cells_from_stamp`] 的失败原因（手写 [`fmt::Display`] + [`std::error::Error`]，不引 `thiserror`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StampConvertError {
    /// 某一族的库号装不进地图侧的单字节字段。
    LibraryOutOfRange {
        /// 格序号，**从 0 起**（= `stamp.cells` 的下标）；消息里显示 `index + 1`。
        index: usize,
        /// 哪一族。
        layer: StampLibLayer,
        /// `.X` 里的原始值（`i16`，未截断）。
        value: i16,
    },
}

impl fmt::Display for StampConvertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LibraryOutOfRange {
                index,
                layer,
                value,
            } => write!(
                f,
                "第 {} 格 / {} / 值 {value} 超出库号字节范围 0..=255（拒绝截断：截断会静默指向另一个图库）",
                index + 1,
                layer.label()
            ),
        }
    }
}

impl std::error::Error for StampConvertError {}

/// 把一枚 `.X` 印章**整枚**转成内核的格表（顺序、相对坐标、格数一律保持原样）。
///
/// 逐格映射见模块文档 §二；三族图号掩 `0x7FFF`（§三）；库号越界即报错（§四）。
///
/// # 错误
///
/// 任意一格的任意一族库号不在 `0..=255` ⇒ [`StampConvertError::LibraryOutOfRange`]，
/// 消息含「第 N 格 / 地表|中层|物体 / 值」。
/// **整枚失败，不返回半份格表**（不做「跳过坏格继续」：那会让印章缺格而调用方不知情）。
///
/// # 例
///
/// ```no_run
/// use mir2_stamp::Stamp;
///
/// let bytes = std::fs::read("<dataRoot>/Objects/mir/mir_00000.X")?;
/// let stamp = Stamp::parse(&bytes)?;
/// let cells = miredit_core::cells_from_stamp(&stamp)?;
/// assert_eq!(cells.len(), 25);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn cells_from_stamp(stamp: &Stamp) -> Result<Vec<StampCellInput>, StampConvertError> {
    let mut out = Vec::with_capacity(stamp.cells.len());
    for (index, cell) in stamp.cells.iter().enumerate() {
        out.push(cell_from_stamp(index, cell.x, cell.y, &cell.cell)?);
    }
    Ok(out)
}

/// 单格转换（[`cells_from_stamp`] 的实现体；`index` 是**0 起**的格序号，只用于报错）。
fn cell_from_stamp(
    index: usize,
    x: i32,
    y: i32,
    cell: &StampCellData,
) -> Result<StampCellInput, StampConvertError> {
    let front = cell.foreground;
    Ok(StampCellInput {
        dx: x,
        dy: y,
        // 见模块文档 §三：`back_image` 是 `i32`，低 16 位里 bit15 是阻挡位，高位是编辑器标记。
        bk: (cell.back_image as u16) & 0x7FFF,
        mid: cell.middle_image & 0x7FFF,
        fr: front.map_or(0, |f| (f.image as u16) & 0x7FFF),
        bk_lib: lib_byte(index, StampLibLayer::Ground, cell.back_library)?,
        mid_lib: lib_byte(index, StampLibLayer::Middle, cell.middle_library)?,
        fr_lib: match front {
            Some(f) => lib_byte(index, StampLibLayer::Front, f.library)?,
            None => 0,
        },
        door: cell.door,
        door_offset: cell.door_offset,
        light: cell.light,
        ani_frame: front.map_or(0, |f| f.frames),
        ani_tick: front.map_or(0, |f| f.tick),
        // 这五个 `.X` 里真有字段（见模块文档 §二 末段），照搬；下游装不下时由 `plan_stamp` 如实报。
        mid_ani_frame: cell.middle_frames,
        mid_ani_tick: cell.middle_tick,
        tile_animation: cell.tile_animation as u16,
        tile_offset: cell.tile_offset as u16,
        tile_frames: cell.tile_frames,
    })
}

/// 库号 `i16` → 单字节；`0..=255` 之外一律报错（**不做 `as u8`**，见模块文档 §四）。
fn lib_byte(
    index: usize,
    layer: StampLibLayer,
    value: i16,
) -> Result<u8, StampConvertError> {
    u8::try_from(value).map_err(|_| StampConvertError::LibraryOutOfRange {
        index,
        layer,
        value,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// 取证根缺省值：`.X` 的取证来源（**本模块里只出现这一处**）。换机器用 `MIREDIT_UPSTREAM_ROOT`。
    const 缺省取证根: &str = r"I:\miredit\_upstream";

    /// 取证根：`MIREDIT_UPSTREAM_ROOT` → 缺省 [`缺省取证根`]；根不存在 ⇒ `None`。
    ///
    /// 只做 `var_os` + `PathBuf`，不引入任何第三方库。
    fn 取证根() -> Option<PathBuf> {
        let root = std::env::var_os("MIREDIT_UPSTREAM_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(缺省取证根));
        root.is_dir().then_some(root)
    }

    /// 读真实印章字节；**缺取证根或文件 ⇒ `None`**（公开仓/PR 的机器上没有 `.X` 素材，
    /// 那不算代码错 ⇒ 调用方打印 `[skip]` 并返回）。
    fn 读真实印章() -> Option<Vec<u8>> {
        let root = 取证根()?;
        let p = root.join("Objects").join("mir").join("mir_00000.X");
        if !p.is_file() {
            eprintln!("[skip] 缺文件 {}", p.display());
            return None;
        }
        match std::fs::read(&p) {
            Ok(b) => Some(b),
            Err(e) => panic!("读不到印章 {}：{e}", p.display()),
        }
    }

    fn i16_at(b: &[u8], at: usize) -> i16 {
        i16::from_le_bytes([b[at], b[at + 1]])
    }

    fn u16_at(b: &[u8], at: usize) -> u16 {
        u16::from_le_bytes([b[at], b[at + 1]])
    }

    fn i32_at(b: &[u8], at: usize) -> i32 {
        i32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
    }

    /// 原始字节里的**一格**：字段名与 `.X` 侧同名，**不做任何掩码**
    /// —— 掩码是被测行为，必须在断言里显式写出，不能借用实现里的同一个表达式。
    #[derive(Debug, Clone, Copy)]
    struct 原始格 {
        x: i32,
        y: i32,
        back_library: i16,
        back_image: i32,
        middle_library: i16,
        middle_image: u16,
        fronts: u8,
        front_library: i16,
        front_image: i16,
        front_frames: u8,
        front_tick: u8,
        door: u8,
        door_offset: u8,
        middle_frames: u8,
        middle_tick: u8,
        tile_animation: i16,
        tile_offset: i16,
        tile_frames: u8,
        light: u8,
    }

    /// 从 `.X` 原始字节**逐格重解**一遍：**不经过** `mir2-stamp`、**不经过** [`cells_from_stamp`]。
    ///
    /// 布局照 `mir2-stamp/src/cell.rs` 模块文档 §一 的表（v2 印章分支）：
    /// 格起点 = `X(4) + Y(4)`，随后 `BackLibrary(2) BackImage(4) MiddleLibrary(2) MiddleImage(2)`、
    /// 前景个数(1)，有前景再加 `Library(2) Image(2) Frames(1) Tick(1)`，
    /// 然后 `Door(1) DoorOffset(1) MiddleFrames(1) MiddleTick(1) TileAnimation(2) TileOffset(2)
    /// TileFrames(1) Light(1)`。
    fn 逐格解字节(bytes: &[u8]) -> Vec<原始格> {
        assert_eq!(&bytes[0..2], b"C#", "魔数必须是 C#");
        let count = i32_at(bytes, 3);
        assert!(count > 0, "格数应 > 0，实测 {count}");
        let mut pos = 7usize;
        let mut out = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let c = &bytes[pos..];
            let fronts = c[18];
            assert!(fronts <= 1, "一格最多 1 个前景，实测 {fronts}");
            // 前景块之后才是 Door（`Document.cs:96` 的顺序）
            let tail = if fronts == 1 { 25 } else { 19 };
            out.push(原始格 {
                x: i32_at(c, 0),
                y: i32_at(c, 4),
                back_library: i16_at(c, 8),
                back_image: i32_at(c, 10),
                middle_library: i16_at(c, 14),
                middle_image: u16_at(c, 16),
                fronts,
                front_library: if fronts == 1 { i16_at(c, 19) } else { 0 },
                front_image: if fronts == 1 { i16_at(c, 21) } else { 0 },
                front_frames: if fronts == 1 { c[23] } else { 0 },
                front_tick: if fronts == 1 { c[24] } else { 0 },
                door: c[tail],
                door_offset: c[tail + 1],
                middle_frames: c[tail + 2],
                middle_tick: c[tail + 3],
                tile_animation: i16_at(c, tail + 4),
                tile_offset: i16_at(c, tail + 6),
                tile_frames: c[tail + 8],
                light: c[tail + 9],
            });
            pos += if fronts == 1 { 35 } else { 29 };
        }
        assert_eq!(pos, bytes.len(), "格数 × 单格字节 必须精确等于文件长度（尾部不许有冗余）");
        out
    }

    /// 按**规则**（模块文档 §二 的表）算出该格应得的 [`StampCellInput`]。
    ///
    /// 三族图号的 `& 0x7FFF` **在这里显式再写一遍**：如果实现漏了掩码，这条会红。
    fn 期望输入(g: &原始格) -> StampCellInput {
        StampCellInput {
            dx: g.x,
            dy: g.y,
            bk: (g.back_image as u16) & 0x7FFF,
            mid: g.middle_image & 0x7FFF,
            fr: (g.front_image as u16) & 0x7FFF,
            bk_lib: u8::try_from(g.back_library).expect("样本地表库号应装得进 u8"),
            mid_lib: u8::try_from(g.middle_library).expect("样本中层库号应装得进 u8"),
            fr_lib: if g.fronts == 1 {
                u8::try_from(g.front_library).expect("样本物体库号应装得进 u8")
            } else {
                0
            },
            door: g.door,
            door_offset: g.door_offset,
            light: g.light,
            ani_frame: g.front_frames,
            ani_tick: g.front_tick,
            mid_ani_frame: g.middle_frames,
            mid_ani_tick: g.middle_tick,
            tile_animation: g.tile_animation as u16,
            tile_offset: g.tile_offset as u16,
            tile_frames: g.tile_frames,
        }
    }

    /// 测试 1：真实 `mir_00000.X` —— 格数 25，`cells[0]` 的三族图号/库号与**直接读该格**一致，
    /// 并顺带把 25 格逐字段全比一遍（含 8 格带前景的 `fr` / `fr_lib` / `ani_frame` / `ani_tick`）。
    #[test]
    fn 真实印章二十五格_首格三族与直接读该格一致() {
        let Some(bytes) = 读真实印章() else {
            return;
        };
        let stamp = Stamp::parse(&bytes).expect("mir_00000.X 应能解析");
        let cells = cells_from_stamp(&stamp).expect("样本库号全在 0..=255 内，应转换成功");

        assert_eq!(stamp.cells.len(), 25, "README:211-215 记该文件 25 格");
        assert_eq!(cells.len(), 25, "转换不得增删格");

        let 原始 = 逐格解字节(&bytes);
        assert_eq!(原始.len(), 25);

        // ① 任务书点名的 `cells[0]`：三族图号 + 三族库号，逐个与直接读该格的结果比。
        assert_eq!(cells[0].bk, (原始[0].back_image as u16) & 0x7FFF);
        assert_eq!(cells[0].mid, 原始[0].middle_image & 0x7FFF);
        assert_eq!(cells[0].fr, (原始[0].front_image as u16) & 0x7FFF);
        assert_eq!(cells[0].bk_lib, 0);
        assert_eq!(cells[0].mid_lib, 1);
        assert_eq!(cells[0].fr_lib, 0);
        assert_eq!((cells[0].dx, cells[0].dy), (1, -2), "相对坐标可负");
        // 该格实测 `back_image = 0x60000000`：只有高位阻挡标记、图号为 0
        // ⇒ 掩码后必须是 0（这条同时证明高位标记没有漏进图号）。
        assert_eq!(原始[0].back_image, 0x6000_0000);
        assert_eq!(cells[0].bk, 0);

        // ② 全 25 格逐字段（掩码口径见 `期望输入`）。
        for (i, (got, g)) in cells.iter().zip(原始.iter()).enumerate() {
            assert_eq!(*got, 期望输入(g), "第 {} 格与原始字节不符", i + 1);
        }
    }

    /// 测试 2：库号 300 必须 `Err`，消息含「300」；顺带把三族的族名与 1 起格号钉住。
    #[test]
    fn 库号三百必须报错_消息含格号族名与值() {
        let Some(bytes) = 读真实印章() else {
            return;
        };
        let mut stamp = Stamp::parse(&bytes).expect("解析");
        stamp.cells[0].cell.back_library = 300;

        let err = cells_from_stamp(&stamp).expect_err("库号 300 装不进单字节，必须拒绝");
        let msg = err.to_string();
        assert!(msg.contains("300"), "消息应含原始值 300：{msg}");
        assert!(msg.contains("第 1 格"), "消息应含 1 起的格号：{msg}");
        assert!(msg.contains("地表"), "消息应含族名：{msg}");
        assert_eq!(
            err,
            StampConvertError::LibraryOutOfRange {
                index: 0,
                layer: StampLibLayer::Ground,
                value: 300,
            }
        );

        // 手写的 `Display` + `std::error::Error` 真能当 trait object 用（不是只挂了个名字）。
        let dyn_err: &dyn std::error::Error = &err;
        assert_eq!(dyn_err.to_string(), msg);

        // 中层：负数同样装不进 `u8`（`-1 as u8 == 255`，截断会变成另一个库）。
        stamp.cells[0].cell.back_library = 0;
        stamp.cells[0].cell.middle_library = -1;
        let err = cells_from_stamp(&stamp).expect_err("负库号必须拒绝");
        assert!(err.to_string().contains("中层"), "{err}");
        assert!(err.to_string().contains("值 -1"), "{err}");
        stamp.cells[0].cell.middle_library = 1;

        // 物体：找第一个带前景的格（样本有 8 格），把它的库号改成 256。
        let idx = stamp
            .cells
            .iter()
            .position(|c| c.cell.foreground.is_some())
            .expect("样本应有带前景的格");
        stamp.cells[idx]
            .cell
            .foreground
            .as_mut()
            .expect("刚查到带前景")
            .library = 256;
        let err = cells_from_stamp(&stamp).expect_err("物体库号 256 必须拒绝");
        let msg = err.to_string();
        assert!(msg.contains("物体"), "{msg}");
        assert!(msg.contains("256"), "{msg}");
        assert!(msg.contains(&format!("第 {} 格", idx + 1)), "{msg}");
    }
}
