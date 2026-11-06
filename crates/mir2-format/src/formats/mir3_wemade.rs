//! 格式 `Mir3Wemade` —— WeMade Mir3 图（**无标题**，文件首字节为 0），C# `LoadMapType5`。
//!
//! # 一、文件头（28 字节）+ **半分辨率背景块**
//!
//! | 偏移 | 字段 | 类型 | 说明 | 源码行号 |
//! |------|------|------|------|----------|
//! | 20 | `Attribute` | `i16` | 属性（本 crate 不解释，原样保留） | `:388` |
//! | 22 | `Width` | `i16` | 宽 | `:389` |
//! | 24 | `Height` | `i16` | 高 | `:390` |
//! | 26 | `EventFile` / `FogColor` | — | C# 明确「忽略」（`:391` 注释 `ignoring eventfile and fogcolor for now`） | — |
//!
//! 紧接 28 字节之后是**半分辨率背景块**，长度
//! `3 * ceil(w/2) * floor(h/2)`（C# `:394` 的 `ValidateDimensions(28 + 3*((Width/2)+(Width%2))*(Height/2), 14)`
//! 与 `:410` 的第二趟起点 `offset = 28 + 3*((Width/2)+(Width%2))*(Height/2)` 都给出这个式子）。
//!
//! 块内每 **3 字节**描述 4 个格（2×2）：
//!
//! | 块内字节 | 含义 | 源码行号 |
//! |---------|------|----------|
//! | +0 | 背景库号：`255` ⇒ `-1`，否则 `+200` | `:404` |
//! | +1..+2 | 背景图号：`ToInt16 + 1`（**有符号**，结果按 C# 的 `int` 放宽） | `:405` |
//!
//! 4 个格的下标是 `(x*2 + i%2, y*2 + i/2)`，`i` 取 `0..4`（`:402-406`）。
//!
//! # 二、⚠ 为什么背景块放在 `header_raw` 里，而不是拆进 `cells`
//!
//! C# 的写法有一个**自身不一致**：读背景块的循环是 `x < Width/2`、`y < Height/2`
//! （**向下取整**，`:399-400`），而第二趟的起点用的是 `ceil(w/2)`（`:410`）。
//! 宽为奇数时两者相差 `3 * floor(h/2)` 个字节 —— 那一段**C# 从未读取**。
//!
//! 若把背景块拆进 `cells`，写回时就必须从 `cells` 反推那一段，奇宽图上会**凭空补字节**，
//! 直接破坏字节级往返。⇒ 本 crate 把 `28 .. 28+块长` 整段原样放进
//! [`crate::RawMap::header_raw`]（模块文档对 `header_raw` 的定义就是「格数组之前的全部字节」），
//! 另提供 [`decode_back_tiles`] 按 C# 的循环把**能解释的那部分**解出来给上层用。
//!
//! # 二·五、⚠ `Cell::bk_img` 恒 0 —— **逐格写背景图号是无效操作**
//!
//! 本格式的背景来自**块**（一个 3 字节组同时决定 2×2 = 4 个格），不是每格一个字段：
//!
//! - 读：`read` 里 `bk_img: 0` 是**写死的**（见下方注释），真值在 `header_raw` 的块里；
//! - 写：`write` **根本不写 `bk_img`**（磁盘格内没有它的位置）。
//!
//! ⇒ 上层若在地表层按格写 [`Cell::bk_img`](crate::Cell::bk_img)，写回时会被**静默丢弃**
//! （不报错、文件字节不变）。这是「改了没生效」类事故的典型来源。
//!
//! 因此本格式的 [`FormatCaps::ground_paint_supported`](crate::FormatCaps::ground_paint_supported)
//! 为 **`false`**（九种里唯一一个）。上层必须先查这一位：`false` 时**禁止开放地表笔刷**，
//! 或改成块级编辑（块在 `header_raw` 里，可用 [`decode_back_tiles`] 只读解出、再自行改 `header_raw`）。
//!
//! # 三、单元格（14 字节）
//!
//! | 格内偏移 | 字段 | 类型 | 磁盘变换 | 源码行号 |
//! |---------|------|------|----------|----------|
//! | 0 | `flag` | `u8` | `bit0` 决定背景是否打 `0x20000000`，`bit1` 决定 `0x40000000` | `:415`、`:436-437` |
//! | 1 | 中间层动画帧 | `u8` | — | `:416` |
//! | 2 | 前景动画帧 | `u8` | `255 ⇒ 0`，再 `& 0x8F` | `:417-418` |
//! | 3 | `btArea` / 前景库号 | `u8` | `255 ⇒ -1`，否则 `+200`，再过 `:429-430` 的哨兵修正 | `:422`、`:429-430` |
//! | 4 | 中间层库号 | `u8` | `255 ⇒ -1`，否则 `+200` | `:424` |
//! | 5 | 中间图号 | `u16` | `+1` | `:426` |
//! | 7 | 前景图号 | `u16` | `+1` | `:428` |
//! | 9 | — | `[u8; 3]` | **跳过**（C# `:432` 注释「mir3 maps dont have doors」） | — |
//! | 12 | `btLight` | `u8` | `& 0x0F` 再 `* 4` | `:433-434` |
//! | 13 | — | `u8` | 未使用（C# `:435` `offset += 2` 把它一起跳过） | — |
//!
//! 合计 14 字节，尾区 **4** 字节（格内偏移 9 / 10 / 11 / 13；偏移 12 是 `Light`，
//! 单独成字段，**不重复进 tail**，否则改 `light` 会不生效）。
//!
//! ⚠ 注意 C# 在 `:420-421` 把 `MiddleAnimationTick` 与 `franimationtick` **硬写成 0**
//! （不落盘、不参与判定），本 crate 不虚构这两个值。
//!
//! # 三·五、⚠ 前景哨兵规则 `frimage == 1 && frindex == 200 ⇒ frindex = -1`（C# `LegacyMapReader.cs:429-430`）
//!
//! 这条规则在本格式里**是活代码**（不是从别处抄来的死代码）：
//!
//! - `frindex` 来自 `:422` 的 `Bytes[offset] != 255 ? Bytes[offset] + 200 : -1`
//!   ⇒ 取值集合是 **`-1` 或 `200…454`** ⇒ **`200` 可达**（该字节为 0 时）；
//! - `frimage` 来自 `:428` 的 `ToInt16 + 1` ⇒ `frimage == 1` ⇔ 磁盘那 2 字节为 0。
//!
//! 命中后果：C# 把前景库号改成 `-1` ⇒ `:439` 的 `(frimage & 0x7FFF) > 0 && frindex > 0`
//! 判假 ⇒ `HasFront` 不置位，该格**没有前景**。
//! 若漏掉这条，`area == 0 && 前景图号原始值 == 0` 的空前景格会被解成「库 200 / 图 1」
//! ⇒ 上层会在空地上画出**幻影前景**。
//!
//! 取库号请用 [`csharp_front_lib`]（已含本条），**不要**只用 [`csharp_lib`]。
//!
//! ⚠ 对照组：Type6（`mir3_shanda.rs`）里**同一条规则是死代码** —— 那一支的 `frindex`
//! 是 `-1` 或 `≥ 300`（`LegacyMapReader.cs:474` 的 `+300`），永不等于 200。
//! 本仓两处一活一死，**别照搬**（`mir3_shanda.rs:266-276` 的 `csharp_dead_front_fix` 保留无害）。
//!
//! # 四、存储顺序：**列优先**
//!
//! ```text
//! offset = 28 + 块长 + (x * h + y) * 14      // x 外层、y 内层（C# `:411-412`）
//! ```
//!
//! # 五、真实样本
//!
//! **无**（本机 1158 个真实 `.map` 里 0 个命中本支）⇒ 只有合成样本验证。
//!
//! # 六、判别
//!
//! C# `:67`：`Bytes[0] == 0`。这条谓词极宽（任何首字节为 0 的文件都命中），
//! 因此 [`Mir3Wemade::detect`] 额外要求长度精确等于
//! `28 + 块长 + 14*w*h`。

use crate::error::FormatError;
use crate::model::{
    i16_at, put_u16, u16_at, validate_dims, validate_write_dims, Cell, CollisionPolarity, FormatCaps,
    FormatId, MapFormat, RawMap,
};

/// 文件头字节数（不含半分辨率背景块）。
pub const HEADER_LEN: usize = 28;

/// 单格字节数。
pub const CELL_LEN: usize = 14;

/// 尾区字节数（格内偏移 9 / 10 / 11 / 13 共 4 字节；偏移 12 是 Light，单独成字段）。
pub const TAIL_LEN: usize = 4;

/// 半分辨率背景块里「一个 2×2 组」的字节数。
pub const BACK_GROUP_LEN: usize = 3;

/// 背景库号的哨兵值（C# `:404` 的 `255`）。
pub const BACK_LIB_SENTINEL: u8 = 255;

/// 背景库号偏移（C# `:404` 的 `+200`）。
pub const BACK_LIB_OFFSET: i16 = 200;

/// 前景/中间层库号偏移（C# `:422`/`:424` 的 `+200`）。
pub const LIB_OFFSET: i16 = 200;

/// 格式单例。
#[derive(Debug, Clone, Copy, Default)]
pub struct Mir3Wemade;

/// 半分辨率背景块里解出来的一个 2×2 组。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackGroup {
    /// 块内偏移（相对文件起点）。
    pub offset: usize,
    /// 组内 4 个格的左上角坐标 `(2x, 2y)`。
    pub x: u16,
    /// 组内 4 个格的左上角纵坐标 `2y`。
    pub y: u16,
    /// 背景库号：`255 ⇒ -1`，否则 `原值 + 200`（C# `:404`）。
    pub lib: i16,
    /// 背景图号：`ToInt16(磁盘) + 1`（C# `LegacyMapReader.cs:405`）。
    ///
    /// ⚠ **有符号**：`ToInt16` 是 `i16`，所以 `0x8000` 解出 `-32767`（不是 `32769`）；
    /// 结果类型是 `i32`，因为 C# 那边 `BackImage` 是 `int`、`+1` 在 `int` 里做
    /// （`0x7FFF` ⇒ `32768`，若用 `i16` 装会绕成 `-32768`，反而与 C# 差 65536）。
    pub img: i32,
}

impl Mir3Wemade {
    /// 半分辨率背景块的字节数：`3 * ceil(w/2) * floor(h/2)`。
    ///
    /// 与 C# `:394` / `:410` 的算式逐字一致（`ceil(w/2) = w/2 + w%2`，`floor(h/2) = h/2`）。
    pub const fn back_block_len(width: u16, height: u16) -> usize {
        let half_w = (width as usize / 2) + (width as usize % 2);
        let half_h = height as usize / 2;
        BACK_GROUP_LEN * half_w * half_h
    }

    /// 格数组的起点 = `28 + 块长`。
    pub const fn cells_offset(width: u16, height: u16) -> usize {
        HEADER_LEN + Self::back_block_len(width, height)
    }

    fn header_triplet(bytes: &[u8]) -> Option<(i32, i32, i32)> {
        if bytes.len() < HEADER_LEN {
            return None;
        }
        Some((
            i32::from(i16_at(bytes, 20)),
            i32::from(i16_at(bytes, 22)),
            i32::from(i16_at(bytes, 24)),
        ))
    }
}

impl MapFormat for Mir3Wemade {
    fn id(&self) -> FormatId {
        FormatId::Mir3Wemade
    }

    fn name(&self) -> &'static str {
        "Mir3Wemade（14B/格 + 半分辨率背景块，无标题）"
    }

    fn detect(&self, bytes: &[u8]) -> bool {
        self.header_signature(bytes)
            && match self.dims(bytes) {
                Some((w, h)) => {
                    (bytes.len() as u64)
                        == Self::cells_offset(w, h) as u64 + w as u64 * h as u64 * CELL_LEN as u64
                }
                None => false,
            }
    }

    /// C# `:67`：`Bytes[0] == 0`（「wemade mir3 maps have no title they just start with blank bytes」）。
    fn header_signature(&self, bytes: &[u8]) -> bool {
        bytes.first() == Some(&0) && bytes.len() >= HEADER_LEN
    }

    fn dims(&self, bytes: &[u8]) -> Option<(u16, u16)> {
        let (_, w, h) = Self::header_triplet(bytes)?;
        if w > 0 && h > 0 {
            Some((w as u16, h as u16))
        } else {
            None
        }
    }

    fn read(&self, bytes: &[u8]) -> Result<RawMap, FormatError> {
        if bytes.len() < HEADER_LEN {
            return Err(FormatError::TooShort {
                format: FormatId::Mir3Wemade,
                len: bytes.len(),
                need: HEADER_LEN,
            });
        }
        let (_, w, h) = Self::header_triplet(bytes).ok_or(FormatError::InvalidDimensions {
            format: FormatId::Mir3Wemade,
            width: 0,
            height: 0,
        })?;
        let (width, height) = validate_dims(FormatId::Mir3Wemade, w, h)?;
        let cells_off = Self::cells_offset(width, height);
        let count = width as u64 * height as u64;
        let need = cells_off as u64 + count * CELL_LEN as u64;
        if (bytes.len() as u64) < need {
            return Err(FormatError::Truncated {
                format: FormatId::Mir3Wemade,
                len: bytes.len(),
                need,
            });
        }
        let need = need as usize;
        let mut map = RawMap {
            format: FormatId::Mir3Wemade,
            width,
            height,
            header_raw: bytes[..cells_off].to_vec(),
            cells: Vec::with_capacity(count as usize),
            trailer: bytes[need..].to_vec(),
        };
        for i in 0..count as usize {
            let base = cells_off + i * CELL_LEN;
            let c = &bytes[base..base + CELL_LEN];
            map.cells.push(Cell {
                // ⚠ 背景层的图号在「半分辨率背景块」里（见 decode_back_tiles），**不在格内**。
                //    这里恒 0，`write` 也不写它 ⇒ 逐格改 bk_img 会被静默丢弃（见模块文档第二节·五）。
                bk_img: 0,
                mid_img: u16_at(c, 5),
                fr_img: u16_at(c, 7),
                flag: c[0],
                mid_ani_frame: c[1],
                ani_frame: c[2],
                mid_idx: i16::from(c[4]),
                area: c[3],
                light: c[12],
                // 尾区 4 字节：格内偏移 9 / 10 / 11 / 13（偏移 12 是 Light，单独成字段，不重复存）
                tail: vec![c[9], c[10], c[11], c[13]],
                ..Cell::default()
            });
        }
        Ok(map)
    }

    fn write(&self, map: &RawMap) -> Result<Vec<u8>, FormatError> {
        // 尺寸门：Mir3Wemade 的 header_raw 长度依赖 w/h（半分辨率背景块），走的不是
        // `write_fixed`，所以必须自己挂在最顶部 —— 而且要挂在 `cells_offset(w, h)` 之前：
        // 否则先算背景块长度就可能在超限尺寸上做无谓的大分配。
        validate_write_dims(FormatId::Mir3Wemade, map.width, map.height)?;
        if map.format != FormatId::Mir3Wemade {
            return Err(FormatError::inconsistent(format!(
                "目标格式是 Mir3Wemade，而 RawMap.format 是 {:?}",
                map.format
            )));
        }
        let cells_off = Self::cells_offset(map.width, map.height);
        if map.header_raw.len() != cells_off {
            return Err(FormatError::inconsistent(format!(
                "Mir3Wemade 的 header_raw 必须是 28 + 背景块 = {cells_off} 字节（{}×{}），实际 {} 字节",
                map.width,
                map.height,
                map.header_raw.len()
            )));
        }
        let count = map.width as usize * map.height as usize;
        if map.cells.len() != count {
            return Err(FormatError::inconsistent(format!(
                "Mir3Wemade 的 w*h = {}×{} = {count}，而 cells.len() = {}",
                map.width,
                map.height,
                map.cells.len()
            )));
        }
        let mut out = Vec::with_capacity(cells_off + count * CELL_LEN + map.trailer.len());
        out.extend_from_slice(&map.header_raw);
        for (i, cell) in map.cells.iter().enumerate() {
            if cell.tail.len() != TAIL_LEN {
                return Err(FormatError::inconsistent(format!(
                    "Mir3Wemade 第 {i} 格的 tail 必须是 {TAIL_LEN} 字节，实际 {} 字节",
                    cell.tail.len()
                )));
            }
            if !(0..=255).contains(&cell.mid_idx) {
                return Err(FormatError::inconsistent(format!(
                    "Mir3Wemade 第 {i} 格的 mid_idx 在磁盘上只有 1 字节，收到 {}",
                    cell.mid_idx
                )));
            }
            out.push(cell.flag);
            out.push(cell.mid_ani_frame);
            out.push(cell.ani_frame);
            out.push(cell.area);
            out.push(cell.mid_idx as u8);
            put_u16(&mut out, cell.mid_img);
            put_u16(&mut out, cell.fr_img);
            // 尾区 4 字节按 9 / 10 / 11 / 13 回填，Light 插在第 4 个之前
            out.push(cell.tail[0]);
            out.push(cell.tail[1]);
            out.push(cell.tail[2]);
            out.push(cell.light);
            out.push(cell.tail[3]);
        }
        out.extend_from_slice(&map.trailer);
        Ok(out)
    }

    fn caps(&self) -> FormatCaps {
        FormatCaps {
            can_write: true,
            // 2026-11-05（P1-9）复核后**保持 `false`**：两条判据都不满足。
            //   判据 1：格内 4 字节未建模（`TAIL_LEN = 4`：rel 9/10/11 是门信息、rel 13 无注释），
            //           更关键的是 `flag`（rel 0，C# `:436-437` 由 bit0/bit1 派生「可走/阻挡」）
            //           与 `mid_idx` / `mid_ani_frame` **不在编辑视图里**
            //           （`convert.rs` 的 `unmodeled_fields(Mir3Wemade)` 列了这四项）
            //           ⇒ 改了图号之后，碰撞/可走标志与中层库号不会跟着变；
            //   判据 2：本机 0 张真实样本（本模块文档第五节）。
            //   ⚠ 背图在**块级**（一个 3 B 组喂 2×2 格）已由 `ground_paint_supported = false`
            //     这条**独立轴**表达，不重复算进本位的理由（P1-9 要求两件事分开陈述）。
            editable: false,
            editable_caveat: "每格 4 字节未建模（格内 rel 9/10/11 = 门信息、C# 注释 `mir3 maps dont have doors`；rel 13 无注释、语义未知）；更关键的是 flag 字节（rel 0，C# 由 bit0/bit1 派生「可走/阻挡」）与中层库号、中层动画帧不在编辑视图里 ⇒ 改图号后碰撞/可走标志不会跟着变；另本机 0 张真实样本",
            // ★ 本格式的背图在**块级**（3 B / 2×2 组），不是每格 `bk_img`：
            //   `Cell::bk_img` 恒 0 且 `write` 不写它 ⇒ 上层按格写 `bk_img` 会被**静默丢弃**。
            //   依据：C# `LegacyMapReader.cs:399-408`（块循环）+ 本模块 `write`（不写 bk_img）。
            ground_paint_supported: false,
            has_lib_bytes: true,
            fixed_cell_len: Some(CELL_LEN),
            has_tail: true,
            has_real_samples: false,
            collision_note: "格内 flag 的 bit0 = 背景可走、bit1 = 前景可走（C# `:436-437` 是「非 1 就置阻挡位」，即 flag 位为 1 才可走）；C# 另把 light `& 0x0F` 再 `* 4`",
            collision_polarity: CollisionPolarity::FlagBitSetMeansWalkable,
            source: "CrystalStudio/LegacyMapReader.cs:382-452（LoadMapType5）",
        }
    }

    fn header_len(&self) -> usize {
        HEADER_LEN
    }

    fn expected_len(&self, width: u16, height: u16) -> Option<u64> {
        Some(
            Self::cells_offset(width, height) as u64
                + width as u64 * height as u64 * CELL_LEN as u64,
        )
    }
}

/// 按 C# `:399-407` 的循环解出半分辨率背景块（**只解 C# 真正读过的那些组**）。
///
/// 宽为奇数时，块尾有 `3 * floor(h/2)` 个字节 C# 从未读取 —— 那些字节仍在
/// [`RawMap::header_raw`] 里原样保存，本函数不返回它们。
///
/// 图号按 C# `LegacyMapReader.cs:405` 的**有符号**语义解（`BitConverter.ToInt16` ⇒ `i16`，再 `+1`），
/// 因此 bit15 置位时得到负值（`0x8000` ⇒ `-32767`），与无符号解释差 **65536**。
pub fn decode_back_tiles(map: &RawMap) -> Vec<BackGroup> {
    let w = map.width as usize;
    let h = map.height as usize;
    let mut out = Vec::new();
    let mut off = HEADER_LEN;
    // C# `:399-400`：x < Width/2、y < Height/2，都是**向下取整**。
    for x in 0..w / 2 {
        for y in 0..h / 2 {
            if off + BACK_GROUP_LEN > map.header_raw.len() {
                return out;
            }
            let raw_lib = map.header_raw[off];
            let lib = if raw_lib == BACK_LIB_SENTINEL {
                -1
            } else {
                i16::from(raw_lib) + BACK_LIB_OFFSET
            };
            // C# `:405`：`(int)(BitConverter.ToInt16(Bytes, offset + 1) + 1)`
            //   —— 先按 **i16 有符号**解释，再 `+1`；C# 的 `BackImage` 是 `int`，
            //   所以 `+1` 必须在 `i32` 里做（`i16` 装 `0x7FFF + 1` 会绕成 `-32768`）。
            let img = i32::from(i16_at(&map.header_raw, off + 1)) + 1;
            out.push(BackGroup {
                offset: off,
                x: (x * 2) as u16,
                y: (y * 2) as u16,
                lib,
                img,
            });
            off += BACK_GROUP_LEN;
        }
    }
    out
}

/// C# `:417-418`：前景动画帧 `255 ⇒ 0`，再 `& 0x8F`。
pub fn csharp_front_ani_frame(raw: u8) -> u8 {
    let v = if raw == 255 { 0 } else { raw };
    v & 0x8F
}

/// C# `:422`/`:424`：库号 `255 ⇒ -1`，否则 `+200`。
///
/// ⚠ 这是**两步里的第一步**：`btArea`（格内 rel 3）还要再过 `:429-430` 的哨兵修正，
/// 请用 [`csharp_front_lib`]；`mid_idx`（rel 4，C# `:424`）**没有**那条修正，用本函数。
pub fn csharp_lib(raw: u8) -> i16 {
    if raw == BACK_LIB_SENTINEL {
        -1
    } else {
        i16::from(raw) + LIB_OFFSET
    }
}

/// C# `LegacyMapReader.cs:429-430`：前景哨兵修正 —— `frimage == 1 && frindex == 200 ⇒ frindex = -1`。
///
/// ✅ **本格式（Type5）里是活规则，可达**：
/// `frindex` 来自 `:422` 的 `Bytes[offset] != 255 ? Bytes[offset] + 200 : -1`
/// ⇒ 取值集合是 `-1` 或 `200…454` ⇒ **`200` 可达**（`btArea` 字节为 0 时）。
///
/// 入参 `fr_img_disk` 是格内 rel 7..9 的**磁盘原值**（= [`Cell::fr_img`](crate::Cell::fr_img)）：
/// C# `:428` 先 `frimage = ToInt16(磁盘) + 1`，`:429` 才判 `frimage == 1`
/// ⇔ **本参数为 0**（本函数内部用 [`csharp_image`] 复现那一步，不写死 0，免得读代码的人对不上）。
///
/// 命中后果：前景库号被压成 `-1` ⇒ C# `:439` 的 `frindex > 0` 判假 ⇒ `HasFront` 不置位
/// ⇒ 该格**没有前景**（不命中则会被解成「库 200 / 图 1」，即**幻影前景**）。
///
/// ⚠ 只给 `btArea`（格内 rel 3，C# `:422`）用；`mid_idx`（rel 4，C# `:424`）没有这条修正。
///
/// ⚠ 对照组：Type6 里**同一条规则是死代码**（那一支的 `frindex` 只可能 `-1` 或 `≥ 300`，
/// `LegacyMapReader.cs:474` 的 `+300`），见 `mir3_shanda.rs::csharp_dead_front_fix`。
/// 两处一活一死，别照搬。
pub fn csharp_front_lib(area: u8, fr_img_disk: u16) -> i16 {
    let lib = csharp_lib(area);
    // C# `:428` 先 `+1` 得 `frimage`，`:429-430` 才判 `(frimage == 1) && (frindex == 200)`。
    if csharp_image(fr_img_disk) == 1 && lib == 200 {
        -1
    } else {
        lib
    }
}

/// C# `:426`/`:428`：图号 `+1`。
pub fn csharp_image(raw: u16) -> u16 {
    raw.wrapping_add(1)
}

/// C# `:433-434`：光照 `& 0x0F` 再 `* 4`。
pub fn csharp_light(raw: u8) -> u8 {
    (raw & 0x0F).wrapping_mul(4)
}

/// C# `:436-437`：`flag` 的 bit0/bit1 **为 1 才可走**。
///
/// 返回 `(背景层可走, 前景层可走)`；对应位不为 1 时 C# 会给背景图号打
/// `0x20000000` / `0x40000000` 阻挡标记。
pub fn csharp_walk_bits(flag: u8) -> (bool, bool) {
    ((flag & 0x01) == 0x01, (flag & 0x02) == 0x02)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 按源码手工构造一张 `w×h` 的 Mir3Wemade 字节流（**无真实样本**）。
    fn 造图(w: u16, h: u16) -> Vec<u8> {
        let n = Mir3Wemade::cells_offset(w, h) + CELL_LEN * w as usize * h as usize;
        let mut b = vec![0u8; n];
        for (i, x) in b.iter_mut().enumerate() {
            *x = (i as u8).wrapping_mul(17).wrapping_add(7);
        }
        b[0] = 0; // 无标题
        b[20..22].copy_from_slice(&1i16.to_le_bytes());
        b[22..24].copy_from_slice(&(w as i16).to_le_bytes());
        b[24..26].copy_from_slice(&(h as i16).to_le_bytes());
        b
    }

    #[test]
    fn 合成样本往返逐字节一致_偶数尺寸() {
        let raw = 造图(6, 4);
        let m = Mir3Wemade.read(&raw).expect("应解析成功");
        assert_eq!((m.width, m.height), (6, 4));
        assert_eq!(m.header_raw.len(), 28 + 3 * 3 * 2);
        assert_eq!(m.cells.len(), 24);
        assert_eq!(Mir3Wemade.write(&m).expect("应写回"), raw);
    }

    #[test]
    fn 合成样本往返逐字节一致_奇数宽() {
        // 奇宽：背景块按 ceil 算，C# 只读 floor(w/2) 组 ⇒ 块尾有 3*floor(h/2) 字节没被读
        let raw = 造图(5, 4);
        let m = Mir3Wemade.read(&raw).expect("应解析成功");
        assert_eq!(m.header_raw.len(), 28 + 3 * 3 * 2);
        assert_eq!(
            decode_back_tiles(&m).len(),
            2 * 2,
            "只解 C# 真正读过的 4 组"
        );
        assert_eq!(Mir3Wemade.write(&m).expect("应写回"), raw);
    }

    #[test]
    fn 背景块解出库号与图号() {
        let mut raw = 造图(4, 2);
        raw[28] = 0; // 库号 0 ⇒ +200
        raw[29..31].copy_from_slice(&0x1234u16.to_le_bytes());
        raw[31] = 255; // 哨兵 ⇒ -1
        let m = Mir3Wemade.read(&raw).unwrap();
        let g = decode_back_tiles(&m);
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].lib, 200);
        assert_eq!(g[0].img, 0x1235);
        assert_eq!(g[1].lib, -1);
    }

    #[test]
    fn 背景图号按有符号解_bit15置位差六五五三六() {
        // C# `:405`：`(int)(BitConverter.ToInt16(Bytes, offset + 1) + 1)`
        //   ⇒ 先 i16 有符号，再 +1。bit15 置位时与无符号解释**差 65536**。
        let mut raw = 造图(2, 2); // 2×2 ⇒ 只有 1 个背景组（cells_offset = 31）
        raw[28] = 0; // 库号 0 ⇒ 200
        raw[29..31].copy_from_slice(&0x8000u16.to_le_bytes());
        let m = Mir3Wemade.read(&raw).expect("应解析成功");
        let g = decode_back_tiles(&m);
        assert_eq!(g.len(), 1);

        // 新（有符号）：i16(0x8000) = -32768，+1 = -32767
        let 有符号 = g[0].img;
        assert_eq!(有符号, -32767, "C# 语义：ToInt16(0x8000) + 1 = -32768 + 1");
        // 旧（无符号）版本：u16(0x8000).wrapping_add(1) = 0x8001 = 32769
        let 旧无符号 = i32::from(u16::from_le_bytes([raw[29], raw[30]]).wrapping_add(1));
        assert_eq!(旧无符号, 32769, "旧 u16 版本的值");
        // 两个数相减 = 65536（正好一个 2^16）
        assert_eq!(旧无符号 - 有符号, 65536, "有符号 / 无符号差 65536");

        // 边界再钉一处：0x7FFF 不是 bit15，但 +1 会越过 i16 上界 ⇒
        // C# 的 `BackImage` 是 `int`，得 32768；若用 i16 装会绕成 -32768（那才是真错）。
        raw[29..31].copy_from_slice(&0x7FFFu16.to_le_bytes());
        let m2 = Mir3Wemade.read(&raw).expect("应解析成功");
        let g2 = decode_back_tiles(&m2);
        assert_eq!(g2[0].img, 32768, "i16 解释 + i32 加一，不绕回 -32768");
    }

    #[test]
    fn 变换对齐源码() {
        assert_eq!(csharp_front_ani_frame(255), 0);
        assert_eq!(csharp_front_ani_frame(0x8F), 0x8F);
        assert_eq!(csharp_front_ani_frame(0xFF), 0);
        assert_eq!(csharp_lib(255), -1);
        assert_eq!(csharp_lib(0), 200);
        // C# `:429-430` 的哨兵修正（Type5 里可达）：
        assert_eq!(csharp_front_lib(0, 0), -1, "frindex 200 + frimage 1 ⇒ -1");
        assert_eq!(csharp_front_lib(1, 0), 201, "201 不命中规则");
        assert_eq!(csharp_front_lib(0, 1), 200, "frimage 2（磁盘 1）不命中规则");
        assert_eq!(csharp_front_lib(255, 0), -1, "哨兵本来就是 -1");
        assert_eq!(csharp_image(0), 1);
        assert_eq!(csharp_light(0x0F), 60);
        assert_eq!(csharp_light(0xFF), 60);
        assert_eq!(csharp_walk_bits(0b11), (true, true));
        assert_eq!(csharp_walk_bits(0b10), (false, true));
    }

    #[test]
    fn 前景哨兵规则_frimage1且frindex200_解出负一() {
        // Type5 的 `frindex` 取值集合是 `-1` 或 `200…454`（C# `:422`）⇒ `200` 可达
        // ⇒ `:429-430` 在本格式里**是活代码**。
        let mut raw = 造图(2, 2);
        let base = Mir3Wemade::cells_offset(2, 2);
        // 第 0 格：btArea（rel 3）= 0 ⇒ frindex = 200；前景图号（rel 7..9）= 0 ⇒ frimage = 1
        raw[base + 3] = 0;
        raw[base + 7..base + 9].copy_from_slice(&0u16.to_le_bytes());
        let m = Mir3Wemade.read(&raw).expect("应解析成功");
        let c = m.cell(0, 0).expect("第 0 格存在");
        assert_eq!(c.area, 0, "磁盘 btArea 原值");
        assert_eq!(c.fr_img, 0, "磁盘前景图号原值");
        assert_eq!(csharp_lib(c.area), 200, "两步里的第一步：frindex = 200");
        assert_eq!(csharp_image(c.fr_img), 1, "frimage = 1");
        assert_eq!(
            csharp_front_lib(c.area, c.fr_img),
            -1,
            "命中 C# `:429-430` ⇒ frindex 变 -1（不是 200）"
        );

        // 再造 frindex = 201（btArea = 1）⇒ 只有 200 命中规则，值必须保持不变。
        raw[base + 3] = 1;
        let m2 = Mir3Wemade.read(&raw).expect("应解析成功");
        let c2 = m2.cell(0, 0).expect("第 0 格存在");
        assert_eq!(csharp_lib(c2.area), 201);
        assert_eq!(
            csharp_front_lib(c2.area, c2.fr_img),
            201,
            "201 不命中规则 ⇒ 值不变"
        );
    }
}
