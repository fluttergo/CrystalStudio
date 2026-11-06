//! 印章文件的整体读写：`Stamp::parse` / `Stamp::to_bytes` / `Stamp::bounds`。
//!
//! 真值：CrystalStudio `Document.cs`
//! - `CellCodec.SaveStamp`：`:137-147`（写：`'C'`、`'#'`、`(byte)2`、`int32 count`，逐格 `X/Y + Write(map:false)`）
//! - `CellCodec.LoadStamp(Stream, ...)`：`:104-125`（读：头探测 → 版本护栏 → 格数 → 预检 → 逐格）
//!
//! 与 C# 的**行为对齐点**（逐条都照抄，不「改进」）：
//! 1. 头探测是「前 3 字节恰为 `'C' '#' v`」才算有头，否则**整份回退为 v0 裸流**
//!    （`:107` `else r.BaseStream.Position = 0;`）；
//! 2. 版本 `> 2` 直接报错（`:108`）；
//! 3. 格数 `count < 0 || count > 1_000_000` 直接报错（`:111`）；
//! 4. 预检 `count × 单格最小字节 > 剩余字节` 直接报错（`:111`）；
//! 5. **文件尾部多余字节被忽略**（C# 读到 `count` 格就返回，不看尾巴）——
//!    本库同样忽略，但**额外提供** [`trailing_bytes`] 让调用方能自己检查（真实 9,170 个文件实测全为 0）。

use crate::cell::{
    cell_byte_len, i32_at, minimum_cell_bytes, read_cell, take, write_cell, StampCellData,
    HEADER_LEN, MAGIC, MAX_CELLS, WRITE_VERSION,
};
use crate::error::StampError;

/// 一个印章格：绝对/相对坐标 + 格数据。对应 C# `public record StampCell(int X, int Y, Cell Cell)`
/// （`Document.cs:81`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StampCell {
    /// 横坐标（`int`，`Document.cs:81` 的 `X`）。印章里是**相对坐标**，可负。
    pub x: i32,
    /// 纵坐标（`int`，`Document.cs:81` 的 `Y`）。印章里是**相对坐标**，可负。
    pub y: i32,
    /// 格数据。
    pub cell: StampCellData,
}

/// 一个印章文件的内存表示。对应 C# `List<StampCell>` + 读到的版本号。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Stamp {
    /// **读到的**版本号（0/1/2）。无文件头时按 C# 语义记为 `0`。
    ///
    /// ⚠ 本字段只用于「这文件原来是第几版」的统计与诊断，
    /// **不影响写出**：[`Stamp::to_bytes`] 恒写 v2（`Document.cs:144`）。
    pub version: u8,
    /// 格列表，**保持文件里的原始顺序**（不排序、不去重、不合并）。
    pub cells: Vec<StampCell>,
}

impl Stamp {
    /// 解析 `.X` 印章字节流，兼容 v0 / v1 / v2（`Document.cs:104-125`）。
    ///
    /// # 错误
    ///
    /// 任何越界、截断、未知版本、格数非法、单格多个前景都会返回 [`StampError`]，
    /// **绝不 panic、绝不静默截断**。
    ///
    /// # 与 C# 一致的宽松点
    ///
    /// 文件尾部多余字节被忽略（C# 亦然）。要检查这一点请配 [`trailing_bytes`]。
    pub fn parse(bytes: &[u8]) -> Result<Stamp, StampError> {
        let (stamp, _consumed) = parse_inner(bytes)?;
        Ok(stamp)
    }

    /// 序列化为 **v2** 字节流（`Document.cs:137-147`）。
    ///
    /// **不 panic**：唯一的失败路径是「格数超过 1,000,000 或超出 `int32`」这种病态输入
    /// （只可能来自手工构造的 `Stamp`，`parse` 自己就会拦住），
    /// 此时返回**空 `Vec`**（fail-closed，绝不产出「看似合法但格数不全」的文件）。
    /// 需要拿到明确错误请用 [`Stamp::try_to_bytes`]。
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        self.try_to_bytes().unwrap_or_default()
    }

    /// 序列化为 **v2** 字节流，失败返回 [`StampError::TooManyCells`]。
    ///
    /// 这是 [`Stamp::to_bytes`] 的可报错版本，护栏值照抄 `Document.cs:140` 的 `1_000_000`。
    pub fn try_to_bytes(&self) -> Result<Vec<u8>, StampError> {
        if self.cells.len() > MAX_CELLS {
            return Err(StampError::TooManyCells {
                count: self.cells.len(),
                limit: MAX_CELLS,
            });
        }
        let count = match i32::try_from(self.cells.len()) {
            Ok(count) => count,
            Err(_) => {
                return Err(StampError::TooManyCells {
                    count: self.cells.len(),
                    limit: MAX_CELLS,
                })
            }
        };
        let mut out =
            Vec::with_capacity(HEADER_LEN + self.cells.len() * crate::cell::CELL_BASE_LEN);
        // :144 —— `w.Write((byte)'C'); w.Write((byte)'#'); w.Write((byte)2); w.Write(list.Count);`
        out.extend_from_slice(&MAGIC);
        out.push(WRITE_VERSION);
        out.extend_from_slice(&count.to_le_bytes());
        // :145 —— `foreach (var c in list) { w.Write(c.X); w.Write(c.Y); Write(w, c.Cell, false); }`
        for cell in &self.cells {
            out.extend_from_slice(&cell.x.to_le_bytes());
            out.extend_from_slice(&cell.y.to_le_bytes());
            write_cell(&mut out, &cell.cell);
        }
        Ok(out)
    }

    /// 所有格的**相对坐标包围盒** `(min_x, min_y, max_x, max_y)`，**闭区间**（含端点）。
    ///
    /// 空印章返回 `(0, 0, -1, -1)`（即 `max < min` ⇒ 空集约定）。
    ///
    /// 口径与 CrystalStudio 一致：`MainWindow.StampEditing.cs:65-67` 用
    /// `cells.Min(c => c.X)` / `cells.Max(c => c.X)` 算边界，宽度 = `max - min + 1`。
    #[must_use]
    pub fn bounds(&self) -> (i32, i32, i32, i32) {
        let Some(first) = self.cells.first() else {
            return (0, 0, -1, -1);
        };
        let (mut min_x, mut min_y, mut max_x, mut max_y) = (first.x, first.y, first.x, first.y);
        for cell in &self.cells[1..] {
            if cell.x < min_x {
                min_x = cell.x;
            }
            if cell.y < min_y {
                min_y = cell.y;
            }
            if cell.x > max_x {
                max_x = cell.x;
            }
            if cell.y > max_y {
                max_y = cell.y;
            }
        }
        (min_x, min_y, max_x, max_y)
    }

    /// 所有格按 v2 格式写出的总字节数（含 7 字节头）。
    ///
    /// 用于和真实文件长度对账：`assert_eq!(stamp.byte_len(), bytes.len())`。
    #[must_use]
    pub fn byte_len(&self) -> usize {
        HEADER_LEN
            + self
                .cells
                .iter()
                .map(|c| cell_byte_len(&c.cell))
                .sum::<usize>()
    }
}

/// 解析并返回「被忽略的尾部字节数」。
///
/// C# 的 `LoadStamp` 读完 `count` 格就返回，尾部多余字节**不报错也不使用**
/// （`Document.cs:113-124`）。本库照抄这一行为，同时把「忽略了多少」暴露出来，
/// 便于门禁断言「真实资源里这个数必须是 0」。
pub fn trailing_bytes(bytes: &[u8]) -> Result<usize, StampError> {
    let (_, consumed) = parse_inner(bytes)?;
    Ok(bytes.len() - consumed)
}

/// 解析主体，返回 `(Stamp, 已消费字节数)`。
fn parse_inner(bytes: &[u8]) -> Result<(Stamp, usize), StampError> {
    let mut pos = 0usize;
    let mut version = 0u8;
    // :107 —— `if (r.BaseStream.Length >= 3 && r.ReadByte() == 'C' && r.ReadByte() == '#')
    //             version = r.ReadByte(); else r.BaseStream.Position = 0;`
    if bytes.len() >= 3 && bytes[0] == MAGIC[0] && bytes[1] == MAGIC[1] {
        version = bytes[2];
        pos = 3;
    }
    // :108 —— `if (version > 2) throw new InvalidDataException("Unsupported object version.");`
    if version > 2 {
        return Err(StampError::UnsupportedVersion { version });
    }

    // :109 —— `int count = r.ReadInt32();`
    let head = take(bytes, &mut pos, 4, 0)?;
    let count = i32_at(head);
    // :111 —— `if (count < 0 || count > 1_000_000 || ...)`
    if count < 0 || count as usize > MAX_CELLS {
        return Err(StampError::InvalidCellCount {
            count,
            limit: MAX_CELLS,
        });
    }

    // :110-112 —— 预检：`(long)count * minimumCellBytes > r.BaseStream.Length - r.BaseStream.Position`
    let minimum = minimum_cell_bytes(version);
    let available = (bytes.len() - pos) as u64;
    let required = count as u64 * minimum as u64;
    if required > available {
        return Err(StampError::TruncatedCellData {
            count,
            minimum_cell_bytes: minimum,
            required,
            available,
        });
    }

    // :113-124 —— 逐格读
    let total = count as usize;
    let mut cells = Vec::with_capacity(total);
    for index in 0..total {
        // :117 —— `int x = r.ReadInt32(), y = r.ReadInt32();`
        let xy = take(bytes, &mut pos, 8, index)?;
        let x = i32_at(&xy[0..4]);
        let y = i32_at(&xy[4..8]);
        let cell = read_cell(bytes, &mut pos, version, index)?;
        cells.push(StampCell { x, y, cell });
    }

    Ok((Stamp { version, cells }, pos))
}
