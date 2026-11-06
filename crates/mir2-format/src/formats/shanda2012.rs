//! 格式 `Shanda2012` —— Shanda 2012 图，**36 字节/格**，C# `LoadMapType3`。
//!
//! # 一、格长定案：**36 B/格，C# 也是 36**（2026-11-05 订正）
//!
//! ⚠ **旧文档曾写「C# 每格推进 40 B、与本格式自相矛盾」——那是误读，已删除。**
//! 真值：`LegacyMapReader.cs:272-326` 的一格**就是 36 字节**，与 `:280` 的
//! `ValidateDimensions(52, 36)` 完全自洽。逐句复算（`offset` 进入格时为 `P`）：
//!
//! | 源码行 | 代码 | `offset` 变化 | 读到的格内偏移 |
//! |---|---|---|---|
//! | `:286` | `ToInt16` | P（不推进） | 0-1 |
//! | `:287` | `offset += 2` | →P+2 | — |
//! | `:288` | `ToUInt16` | P+2（不推进） | 2-3 |
//! | `:289` | `offset += 2` | →P+4 | — |
//! | `:290` | `ToInt16` | P+4（不推进） | 4-5 |
//! | `:291` | `offset += 2` | →P+6 | — |
//! | `:292`-`:299` | `Bytes[offset++]` × 8 | →P+14 | 6..13 |
//! | `:300` | `ToInt16(offset)` —— **不推进** | P+14 | **14-15**（`TileAnimationImage`） |
//! | `:301` | `offset += 7` | →**P+21** | 跳过 16-20（5 B） |
//! | `:302` | `Bytes[offset++]` | →P+22 | **21**（`TileAnimationFrames`） |
//! | `:303` | `ToInt16(offset)` —— **不推进** | P+22 | **22-23**（`TileAnimationOffset`） |
//! | `:304` | `offset += 14` | →**P+36** | 跳过 22-35（14 B，**与 `:303` 的读重叠**） |
//!
//! **关键点**：`:300` 与 `:303` 的 `ToInt16(Bytes, offset)` 都**没有** `offset += 2`
//! ⇒ `:303` 那 2 字节落在 `:304` 的 `offset += 14` 跳过区（rel 22-35）**内部**。
//! 把 `:303` 的 2 字节再算一次才会得出 40 —— 那是重复计数。
//! 合计 `6 + 8 + 2 + 7 + 1 + 14 = 36`。（同结论：审查报告 §一.3，含 195 个真实文件的
//! 「末 offset 恰好等于文件长度」实证。）
//!
//! 真实样本一侧（2026-11-04 实测，本机）：
//!
//! | 目录 | 36 B/格文件数 | 40 B/格文件数 |
//! |------|--------------|--------------|
//! | `GodotClient/maps/*.map`（61 个） | **1** | **0** |
//! | `client/热血传奇23周年客户端/Map/*.map`（1097 个） | **194** | **0** |
//!
//! ⇒ 本 crate 按 36 B/格实现。**`52 + 40·w·h` 这个长度形态在真实语料里 0 个**
//! （见 [`Shanda2012::csharp_40b_variant`] 的「无实证，仅防御」说明），
//! 保留该探测器只为防未来出现怪文件；它**只在长度精确成立时命中，不会误伤 36 B/格文件**。
//!
//! # 二、文件头（52 字节，`TMapHeader`）
//!
//! 与 `Old12` / `Shanda14` 相同（`wWidth` `i16@0`、`wHeight` `i16@2`，C# `:277`/`:279`）。
//!
//! # 三、单元格（36 字节）
//!
//! 前 14 字节与 [`crate::formats::shanda14`] **逐字段相同**（同一份 `TNewMapInfo`），
//! 其后 22 字节是**尾区**。尾区里 C# **命名并读取了 3 个字段**，本 crate 不改
//! [`Cell::tail`](crate::Cell::tail) 的存储方式，另给**只读访问器**（见第六节）：
//!
//! | 格内偏移 | 字段 | 类型 | 说明 | 源码行号 |
//! |---------|------|------|------|----------|
//! | 0 | `wBkImg` | `u16` | 背景图号 | C# `:286` |
//! | 2 | `wMidImg` | `u16` | 中间图号 | `:288` |
//! | 4 | `wFrImg` | `u16` | 前景图号 | `:290` |
//! | 6 | `btDoorIndex` | `u8` | 门索引 | `:292` |
//! | 7 | `btDoorOffset` | `u8` | 门偏移 | `:293` |
//! | 8 | `btAniFrame` | `u8` | 前景动画帧 | `:294` |
//! | 9 | `btAniTick` | `u8` | 前景动画节拍 | `:295` |
//! | 10 | `btArea` | `u8` | 区域 / 前景库号 | `:296` |
//! | 11 | `btLight` | `u8` | 光照 | `:297` |
//! | 12 | `btTiles` | `u8` | 背景库号 | `:298` |
//! | 13 | `btsmTiles` | `u8` | 中间层库号 | `:299` |
//! | **14-15** | `TileAnimationImage` | `i16` | **C# 命名并读取**（`ToInt16` 不推进 offset） | `:300` |
//! | 16-20 | — | `[u8; 5]` | 未命名（`:301` 的 `+= 7` 里含这 5 字节） | `:301` |
//! | **21** | `TileAnimationFrames` | `u8` | **C# 命名并读取** | `:302` |
//! | **22-23** | `TileAnimationOffset` | `i16` | **C# 命名并读取**（`ToInt16` 不推进 offset） | `:303` |
//! | 24-35 | — | `[u8; 12]` | 未命名（`:304` 的 `+= 14` 覆盖 22-35，与上两行**重叠**） | `:304` |
//!
//! 上述 14..36 **整段**进 [`Cell::tail`](crate::Cell::tail)（长度 22，`TAIL_LEN`）。
//! ⇒ `Cell::tiles` / `sm_tiles` / `tile_frames` 在本格式**恒 0**（它们的语义被 12/13 与
//! 尾区的 `TileAnimation*` 承载），要取那三个字段请用第六节的访问器。
//!
//! # 四、尾区**不可假设恒零**（实测）
//!
//! 2026-11-04 对 194 个真实 36 B/格文件做全格扫描，按格内偏移统计「非零出现次数」
//! （2026-11-05 对 195 个文件复算，数字一致）：
//!
//! | 偏移 | 14 | 15 | 16 | 17 | **18** | **19** | **20** | 21 | 22 | 23 | **24** | 25..35 |
//! |------|----|----|----|----|--------|--------|--------|----|----|----|--------|--------|
//! | 非零次数 | 429 | 383 | 9 | 9 | **2039** | **2039** | **2038** | 428 | 717 | 719 | **95990** | 290..394 |
//!
//! ⇒ 尾区既**不能丢**、也**不能重置为 0**、更不能拿「全零」去反推字段语义。
//! 本模块只做「原样保存 + 原样写回」。（同结论见 `crates/mir2-map/src/cell.rs` 的实测记录。）
//!
//! # 五、存储顺序：**列优先**
//!
//! ```text
//! offset = 52 + (x * h + y) * 36      // x 外层、y 内层（C# `:282-283`）
//! ```
//!
//! # 六、尾区三字段的**只读访问器**（2026-11-05 新增）
//!
//! 编辑器要在 36 B 图上显示「图块动画」时，数据在尾区而不在 `ani_frame`/`ani_tick`。
//! 本模块提供三个**只读**访问器，按固定偏移从 [`Cell::tail`](crate::Cell::tail) 取，
//! 越界返回 `None`：
//!
//! | 访问器 | 返回 | 从 `tail` 的偏移 | 对应格内偏移 | C# 行号 |
//! |--------|------|------------------|--------------|---------|
//! | [`Shanda2012::tile_anim_image`] | `Option<u16>`（`i16` 位模式） | `[0..2]` | 14-15 | `:300` |
//! | [`Shanda2012::tile_anim_frames`] | `Option<u8>` | `[7]` | 21 | `:302` |
//! | [`Shanda2012::tile_anim_offset`] | `Option<u16>`（`i16` 位模式） | `[8..10]` | 22-23 | `:303` |
//!
//! ⚠ **访问器不参与写**：`write` 仍只把 `tail` 整段原样吐出 ⇒ 往返 0 差异不受影响。
//! ⚠ 为什么不做成 `Cell::tile_anim_image()`：`tail` 的布局**逐格式不同**
//! （`Mir3Wemade` 的 `tail` 是 rel 9/10/11/13、`Heroes34` 是 rel 14……），
//! 挂在 `Cell` 上会在另外 8 种格式上**静默返回错值**。绑在 `Shanda2012` 上，
//! 调用点必须显式写出格式名，误用不可能通过编译。
//!
//! # 七、判别
//!
//! 与 `Shanda14` 共用 C# 标题谓词（`Bytes[4]==0x0F && Bytes[18]==0x0D && Bytes[19]==0x0A`，
//! C# `:91`），只能靠长度区分：C# 自己在 `:95` 用 `Bytes.Length > 52 + W*H*14` 分流，
//! 本 crate 改成**精确等于** `52 + 36*w*h`。

use crate::error::FormatError;
use crate::model::{
    check_tail, i16_at, put_u16, read_fixed, tmap_dims, u16_at, validate_dims, write_fixed, Cell,
    CollisionPolarity, FormatCaps, FormatId, MapFormat, RawMap,
};

/// 文件头字节数（`TMapHeader`）。
pub const HEADER_LEN: usize = 52;

/// 单格字节数（真实文件口径）。
pub const CELL_LEN: usize = 36;

/// 尾区字节数（格内 14..36）。
pub const TAIL_LEN: usize = 22;

/// 前 14 字节（`TNewMapInfo`）的字节数。
pub const COMMON_LEN: usize = 14;

/// **无实证的 40 B/格长度形态**用的「假格长」——命中就报 [`FormatError::UnsupportedVariant`]。
///
/// ⚠ **不是「C# 的格长」**：C# `LoadMapType3` 就是 36 B/格（见模块文档第一节）。
/// 真实语料里 `52 + 40·w·h` 的文件 **0 个**；保留这个常量与探测器**只为防御未来出现的怪文件**。
pub const CSHARP_CELL_LEN: usize = 40;

/// 尾区里 `TileAnimationImage`（格内 rel 14-15）相对 `tail` 的偏移。
///
/// 依据 C# `LegacyMapReader.cs:300`（`ToInt16(Bytes, offset)`，`offset` 当时为 `P+14`）。
pub const TAIL_OFF_ANIM_IMAGE: usize = 0;

/// 尾区里 `TileAnimationFrames`（格内 rel 21）相对 `tail` 的偏移。
///
/// 依据 C# `LegacyMapReader.cs:302`（`:301` 的 `offset += 7` 之后 `Bytes[offset++]`，落在 `P+21`）。
pub const TAIL_OFF_ANIM_FRAMES: usize = 7;

/// 尾区里 `TileAnimationOffset`（格内 rel 22-23）相对 `tail` 的偏移。
///
/// 依据 C# `LegacyMapReader.cs:303`（`ToInt16(Bytes, offset)`，`offset` 当时为 `P+22`）。
pub const TAIL_OFF_ANIM_OFFSET: usize = 8;

/// 前景库号偏移（与 `Shanda14` 同源，C# `:296`）。
pub const FRONT_LIB_OFFSET: u16 = 120;

/// 背景库号偏移（C# `:298`）。
pub const BACK_LIB_OFFSET: u16 = 100;

/// 中间层库号偏移（C# `:299`）。
pub const MIDDLE_LIB_OFFSET: u16 = 110;

/// 格式单例。
#[derive(Debug, Clone, Copy, Default)]
pub struct Shanda2012;

impl Shanda2012 {
    /// 判断是不是「`52 + 40·w·h`」这个长度形态：**只看长度**是否正好 `52 + 40*w*h`。
    ///
    /// 返回 `Some((w, h))` 表示命中。
    ///
    /// # ⚠ **无实证，仅防御**
    ///
    /// - **真实语料 0 个**：本机 1,158 个真实 `.map` 里，长度精确等于 `52 + 40·w·h` 的
    ///   文件 **0 个**（`52 + 36·w·h` 的有 195 个）。⇒ 本分支在真实数据上**永不触发**。
    /// - **它不代表 C# 的格长**：C# `LoadMapType3` 本来就是 36 B/格
    ///   （`LegacyMapReader.cs:280` 的 `ValidateDimensions(52, 36)` 与实现自洽，见模块文档第一节）。
    ///   旧文档把它称作「C# 40 B/格变体」是**误读**，已删除。
    /// - **保留理由**：万一未来出现长度恰好凑成 `52 + 40·w·h` 的怪文件，宁可显式报
    ///   [`FormatError::UnsupportedVariant`]，也不许按 36 B 硬读（那会把整张图读成花图）。
    /// - **不会误伤 36 B/格文件**：命中条件是 `len == 52 + 40·w·h` 的**精确等式**，
    ///   而 `52 + 40·w·h == 52 + 36·w·h` 只在 `w·h == 0` 时成立（已被 `validate_dims` 拦掉）。
    ///
    /// 为什么**不看标题**：标题字节已被真实样本证伪（`GodotClient/maps` 里 15 个 14 B/格
    /// 文件的标题首字节是 `0x03/0x07/0x0B/0x0D/0xFF` 而不是 `0x0F`）。而 `52 + 40*w*h`
    /// 与 `52 + 12*w*h`、`52 + 14*w*h`、`52 + 36*w*h` 在同一组 `w/h` 下**互斥**
    /// （除非 `w*h == 0`，那种情况已被 `validate_dims` 拦掉）⇒ 长度本身就是唯一判据。
    pub fn csharp_40b_variant(bytes: &[u8]) -> Option<(u16, u16)> {
        let (w, h) = tmap_dims(bytes)?;
        let need = HEADER_LEN as u64 + w as u64 * h as u64 * CSHARP_CELL_LEN as u64;
        if (bytes.len() as u64) == need {
            Some((w, h))
        } else {
            None
        }
    }

    /// 只读访问器：`TileAnimationImage`（格内 rel 14-15，C# `:300`）。
    ///
    /// 从 [`Cell::tail`](crate::Cell::tail) 的 `[0..2]` 按小端取 `i16` 的位模式；
    /// `tail` 不足 2 字节（非本格式的格、或人为截断）返回 `None`。
    ///
    /// **不参与写** —— `write` 仍把 `tail` 整段原样吐出，往返不受影响。
    ///
    /// 真实样本例（Python 独立解码，`client/热血传奇23周年客户端/Map/n0155.map`，
    /// 50×50）：`(x=5,y=16)` ⇒ `8419`；`(x=5,y=32)` ⇒ `8418`；`(x=5,y=0)` ⇒ `6306`。
    pub fn tile_anim_image(cell: &Cell) -> Option<u16> {
        let s = cell
            .tail
            .get(TAIL_OFF_ANIM_IMAGE..TAIL_OFF_ANIM_IMAGE + 2)?;
        Some(u16::from_le_bytes([s[0], s[1]]))
    }

    /// 只读访问器：`TileAnimationFrames`（格内 rel 21，C# `:302`）。
    ///
    /// 从 [`Cell::tail`](crate::Cell::tail) 的 `[7]` 取 1 字节；`tail` 不足 8 字节返回 `None`。
    ///
    /// 真实样本例（同上）：`(x=5,y=16)` ⇒ `49`；`(x=5,y=32)` ⇒ `32`；`(x=5,y=0)` ⇒ `0`。
    pub fn tile_anim_frames(cell: &Cell) -> Option<u8> {
        cell.tail.get(TAIL_OFF_ANIM_FRAMES).copied()
    }

    /// 只读访问器：`TileAnimationOffset`（格内 rel 22-23，C# `:303`）。
    ///
    /// 从 [`Cell::tail`](crate::Cell::tail) 的 `[8..10]` 按小端取 `i16` 的位模式；
    /// `tail` 不足 10 字节返回 `None`。
    ///
    /// ⚠ C# 的 `:303` **不推进 `offset`**，其 2 字节落在 `:304` 的 `offset += 14` 跳过区
    /// （rel 22-35）**内部** —— 这正是「40 B/格」误读的来源，见模块文档第一节。
    ///
    /// 真实样本例（同上）：`(x=5,y=16)` ⇒ `8451`；`(x=5,y=32)` ⇒ `8419`；`(x=5,y=0)` ⇒ `0`。
    pub fn tile_anim_offset(cell: &Cell) -> Option<u16> {
        let s = cell
            .tail
            .get(TAIL_OFF_ANIM_OFFSET..TAIL_OFF_ANIM_OFFSET + 2)?;
        Some(u16::from_le_bytes([s[0], s[1]]))
    }
}

impl MapFormat for Shanda2012 {
    fn id(&self) -> FormatId {
        FormatId::Shanda2012
    }

    fn name(&self) -> &'static str {
        "Shanda2012（36B/格，14B 字段 + 22B 未建模尾区）"
    }

    fn detect(&self, bytes: &[u8]) -> bool {
        self.header_signature(bytes)
            && match self.dims(bytes) {
                Some((w, h)) => {
                    (bytes.len() as u64)
                        == HEADER_LEN as u64 + w as u64 * h as u64 * CELL_LEN as u64
                }
                None => false,
            }
    }

    fn header_signature(&self, bytes: &[u8]) -> bool {
        bytes.len() >= 20 && bytes[4] == 0x0F && bytes[18] == 0x0D && bytes[19] == 0x0A
    }

    fn dims(&self, bytes: &[u8]) -> Option<(u16, u16)> {
        tmap_dims(bytes)
    }

    fn read(&self, bytes: &[u8]) -> Result<RawMap, FormatError> {
        if bytes.len() < HEADER_LEN {
            return Err(FormatError::TooShort {
                format: FormatId::Shanda2012,
                len: bytes.len(),
                need: HEADER_LEN,
            });
        }
        let (w, h) = tmap_dims(bytes).ok_or(FormatError::InvalidDimensions {
            format: FormatId::Shanda2012,
            width: i16_at(bytes, 0) as i32,
            height: i16_at(bytes, 2) as i32,
        })?;
        let (w, h) = validate_dims(FormatId::Shanda2012, w as i32, h as i32)?;
        // 先拦「52 + 40·w·h」这个长度形态：无实证、仅防御（见 csharp_40b_variant 的说明）。
        if Self::csharp_40b_variant(bytes).is_some() {
            return Err(FormatError::unsupported(
                FormatId::Shanda2012,
                format!(
                    "文件长度 {} = 52 + 40*{w}*{h}，命中「40 B/格」长度形态。\
                     该形态**无实证**（本机 1158 个真实样本中 0 个命中；36 B/格有 195 个），\
                     C# LoadMapType3 本身就是 36 B/格（:280 ValidateDimensions(52, 36)），\
                     本 crate 不认这个形态，也不按 36 B 硬读",
                    bytes.len()
                ),
            ));
        }
        read_fixed(
            FormatId::Shanda2012,
            bytes,
            HEADER_LEN,
            CELL_LEN,
            w,
            h,
            |c| {
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
                    tiles: u16::from(c[12]),
                    sm_tiles: u16::from(c[13]),
                    tail: c[COMMON_LEN..CELL_LEN].to_vec(),
                    ..Cell::default()
                })
            },
        )
    }

    fn write(&self, map: &RawMap) -> Result<Vec<u8>, FormatError> {
        write_fixed(
            FormatId::Shanda2012,
            map,
            HEADER_LEN,
            CELL_LEN,
            |cell, out| {
                check_tail(cell, TAIL_LEN, FormatId::Shanda2012)?;
                if cell.bk_img > u32::from(u16::MAX) {
                    return Err(FormatError::inconsistent(format!(
                        "Shanda2012 的背景图号只有 16 位，收到 {}",
                        cell.bk_img
                    )));
                }
                for (name, v) in [("tiles", cell.tiles), ("sm_tiles", cell.sm_tiles)] {
                    if v > u16::from(u8::MAX) {
                        return Err(FormatError::inconsistent(format!(
                            "Shanda2012 的 {name} 只有 8 位，收到 {v}"
                        )));
                    }
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
                out.push(cell.tiles as u8);
                out.push(cell.sm_tiles as u8);
                out.extend_from_slice(&cell.tail);
                Ok(())
            },
        )
    }

    fn caps(&self) -> FormatCaps {
        FormatCaps {
            can_write: true,
            // 2026-11-05（P1-9）复核后**保持 `false`**：判据 1 不满足。
            //   前 14 字节全具名（本模块文档第三节），但 `tail` 有 22 字节未建模（rel 14-35），
            //   而 C# 自己的注释把其中 5 字节疑为「背图索引 / 背图文件索引」（`LegacyMapReader.cs:301`）、
            //   12 字节疑为「光照/混合参数」（`:304`）⇒ 与**可编辑的**背景图号 / 光照**联动**，
            //   改图号后尾区不会跟着变 ⇒ 语义不自洽。
            //   实测那 12 字节非零出现 95,990 次（本模块文档第四节）⇒ 不是可忽略的填充。
            editable: false,
            editable_caveat: "每格有 22 字节未建模尾区（格内 rel 14-35，编辑时原样保留、不随图号/光照更新）：其中 rel 14-15 / 21 / 22-23 共 5 字节 C# 有名字（TileAnimationImage / TileAnimationFrames / TileAnimationOffset，本 crate 只给只读访问器），rel 16-20 共 5 字节与 rel 24-35 共 12 字节无名字；C# 注释把前者疑为「背图索引 / 背图文件索引」（与背景图号联动）、后者疑为「光照/混合参数」（与光照联动）—— 改图号或光照后尾区不会跟着变，故不承诺改格后语义自洽",
            ground_paint_supported: true,
            has_lib_bytes: true,
            fixed_cell_len: Some(CELL_LEN),
            has_tail: true,
            has_real_samples: true,
            collision_note: "前 14 字节的 wBkImg / wFrImg 的 0x8000 = 阻挡位（同 Shanda14）；22 字节尾区未建模（5 字节 C# 有名 / 17 字节无名），不得用于碰撞判定",
            collision_polarity: CollisionPolarity::Bit15SetMeansBlocked,
            source: "CrystalStudio/LegacyMapReader.cs:272-326（LoadMapType3，36B/格）+ 真实样本实测 36B/格",
        }
    }

    fn header_len(&self) -> usize {
        HEADER_LEN
    }

    fn expected_len(&self, width: u16, height: u16) -> Option<u64> {
        Some(HEADER_LEN as u64 + width as u64 * height as u64 * CELL_LEN as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn 造图(w: u16, h: u16, cell: usize) -> Vec<u8> {
        let mut b = vec![0u8; HEADER_LEN + cell * w as usize * h as usize];
        for (i, x) in b.iter_mut().enumerate() {
            *x = (i as u8).wrapping_mul(29).wrapping_add(5);
        }
        b[0..2].copy_from_slice(&w.to_le_bytes());
        b[2..4].copy_from_slice(&h.to_le_bytes());
        b[4] = 0x0F;
        b[18] = 0x0D;
        b[19] = 0x0A;
        b
    }

    #[test]
    fn 三十六字节格往返且尾区原样() {
        let raw = 造图(3, 2, CELL_LEN);
        let m = Shanda2012.read(&raw).expect("应解析成功");
        assert_eq!(m.cells.len(), 6);
        let base = HEADER_LEN + 5 * CELL_LEN; // (x=2,y=1) → idx=5
        assert_eq!(
            m.cell(2, 1).expect("(2,1) 在图内").tail,
            raw[base + COMMON_LEN..base + CELL_LEN].to_vec()
        );
        assert_eq!(Shanda2012.write(&m).expect("应写回"), raw);
    }

    #[test]
    fn 四十字节变体显式报不支持() {
        let raw = 造图(3, 2, CSHARP_CELL_LEN);
        assert!(Shanda2012::csharp_40b_variant(&raw).is_some());
        assert!(!Shanda2012.detect(&raw), "40B 变体不该被 detect 认成 36B");
        match Shanda2012.read(&raw) {
            Err(FormatError::UnsupportedVariant { format, detail }) => {
                assert_eq!(format, FormatId::Shanda2012);
                assert!(detail.contains("40"), "{detail}");
            }
            other => panic!("应报 UnsupportedVariant，实际 {other:?}"),
        }
    }

    /// 反向断言：**36 B/格的文件绝不会被「40 B/格」探测器命中**（否则 195 个真实文件会被拒）。
    ///
    /// 依据：`52 + 40·w·h == 52 + 36·w·h` 只在 `w·h == 0` 时成立，而 `validate_dims` 已拦掉。
    #[test]
    fn 三十六字节文件不会被四十字节探测器命中() {
        for (w, h) in [(1u16, 1u16), (3, 2), (20, 20), (400, 520)] {
            let raw = 造图(w, h, CELL_LEN);
            assert_eq!(raw.len(), HEADER_LEN + CELL_LEN * w as usize * h as usize);
            assert_eq!(
                Shanda2012::csharp_40b_variant(&raw),
                None,
                "{w}×{h} 的 36 B/格文件不该命中 40 B/格探测器"
            );
            assert!(Shanda2012.detect(&raw), "{w}×{h} 应被 detect 认成 Shanda2012");
            assert!(
                Shanda2012.read(&raw).is_ok(),
                "{w}×{h} 的 36 B/格文件不该被 read 拒绝"
            );
        }
    }

    /// 尾区访问器：越界返回 `None`（不 panic），正常格取到固定偏移上的值。
    #[test]
    fn 尾区访问器按固定偏移取值且越界返回none() {
        // 造一格：rel14-15 = 0x20E3、rel21 = 0x31、rel22-23 = 0x2103
        let mut cell = Cell::zeroed(TAIL_LEN);
        cell.tail[TAIL_OFF_ANIM_IMAGE..TAIL_OFF_ANIM_IMAGE + 2]
            .copy_from_slice(&0x20E3u16.to_le_bytes());
        cell.tail[TAIL_OFF_ANIM_FRAMES] = 0x31;
        cell.tail[TAIL_OFF_ANIM_OFFSET..TAIL_OFF_ANIM_OFFSET + 2]
            .copy_from_slice(&0x2103u16.to_le_bytes());
        assert_eq!(Shanda2012::tile_anim_image(&cell), Some(0x20E3));
        assert_eq!(Shanda2012::tile_anim_frames(&cell), Some(0x31));
        assert_eq!(Shanda2012::tile_anim_offset(&cell), Some(0x2103));

        // 越界：tail 太短 ⇒ None（不是 panic、也不是 0）
        for 短 in 0..TAIL_OFF_ANIM_OFFSET + 2 {
            let c = Cell::zeroed(短);
            assert_eq!(
                Shanda2012::tile_anim_image(&c),
                if 短 >= TAIL_OFF_ANIM_IMAGE + 2 {
                    Some(0)
                } else {
                    None
                },
                "tail={短} 时 tile_anim_image 的返回不对"
            );
            assert_eq!(
                Shanda2012::tile_anim_frames(&c),
                if 短 > TAIL_OFF_ANIM_FRAMES {
                    Some(0)
                } else {
                    None
                },
                "tail={短} 时 tile_anim_frames 的返回不对"
            );
            assert_eq!(
                Shanda2012::tile_anim_offset(&c),
                if 短 >= TAIL_OFF_ANIM_OFFSET + 2 {
                    Some(0)
                } else {
                    None
                },
                "tail={短} 时 tile_anim_offset 的返回不对"
            );
        }
    }

    /// 访问器**不参与写**：改访问器读到的值不会改变写回结果（它们只读 `tail`）。
    #[test]
    fn 尾区访问器不参与写回() {
        let raw = 造图(3, 2, CELL_LEN);
        let m = Shanda2012.read(&raw).expect("应解析成功");
        let _ = Shanda2012::tile_anim_image(&m.cells[0]);
        let _ = Shanda2012::tile_anim_frames(&m.cells[0]);
        let _ = Shanda2012::tile_anim_offset(&m.cells[0]);
        assert_eq!(Shanda2012.write(&m).expect("应写回"), raw);
    }
}
