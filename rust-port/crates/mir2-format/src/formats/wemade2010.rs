//! 格式 `Wemade2010` —— WeMade 2010 图（标题 `Map 2010 Ver 1.0`），C# `LoadMapType1`。
//!
//! # 一、文件头（54 字节）
//!
//! | 偏移 | 字段 | 类型 | 说明 | 源码行号 |
//! |------|------|------|------|----------|
//! | 0 | 标题长度前缀 | `u8` | `0x10` = 16 | `:85` |
//! | 1 | 标题 | `[u8; 16]` | ASCII `Map 2010 Ver 1.0`（由判据字节 `B[2]='a'`、`B[7]='1'`、`B[14]='1'` 唯一反推） | `:85` |
//! | 21 | `w` | `i16` | **异或前**的宽 | `:176` |
//! | 23 | `xor` | `i16` | 宽高/中间/前景图号共用的 XOR 掩码 | `:178` |
//! | 25 | `h` | `i16` | **异或前**的高 | `:180` |
//! | 27 | — | `[u8; 27]` | 未读，原样进 `header_raw` | `:185` |
//!
//! `Width = w ^ xor`、`Height = h ^ xor`（`:181-182`）。C# 的 `ValidateDimensions(54, 15)` 在 `:183`。
//!
//! # 二、单元格：**15 B/格，唯一一档**
//!
//! ## 2.1 逐句复算（`offSet` 从 54 起，`X += n` 是**先加后读**）
//!
//! | 语句 | 行 | `offSet` 变化 | 读到的格内偏移 |
//! |------|----|--------------|---------------|
//! | `ToInt32(Bytes, offSet)` | `:193` | 54（不变） | rel 0..3 |
//! | `ToUInt16(Bytes, offSet += 4)` | `:195` | 54→**58** | rel 4..5 |
//! | `ToInt16(Bytes, offSet += 2)` | `:196` | 58→**60** | rel 6..7 |
//! | `Bytes[offSet += 2]` | `:197` | 60→**62** | **rel 8**（不是 rel 10） |
//! | `Bytes[++offSet]` | `:198` | 62→63 | rel 9 |
//! | `Bytes[++offSet]` | `:199` | 63→64 | rel 10 |
//! | `Bytes[++offSet]` | `:200` | 64→65 | rel 11 |
//! | `Bytes[++offSet]` | `:201` | 65→66 | rel 12 |
//! | `Bytes[++offSet]` | `:203` | 66→67 | rel 13 |
//! | `offSet += 2` | `:205` | 67→**69** | 跳过 rel 14，下一格起点 = 54+**15** |
//!
//! **格长 = 69 − 54 = 15**，与 `:183` 的 `ValidateDimensions(54, 15)` 自洽。
//!
//! ## 2.2 为什么**不是** 17 B/格 —— 旧「备档」已整条删除（2026-11-05）
//!
//! 本模块旧版曾实现第二档 `CELL_LEN_ALT = 17` / `TAIL_LEN_ALT = 3`，来源是规格书
//! 《规格-地图格式全量 9 种》§二 的「实测 17 B/格（声明 15）」主张（该文档已移出公开仓）。
//! **该档已删除**，四条独立证据（内部审查报告《审查-六种无样本格式逐行对照》
//! §1 D1.2 已逐条核实；该报告同样已移出公开仓）：
//!
//! 1. **C# 里 `17` 零命中**：`LegacyMapReader.cs` 通篇没有「17 字节/格」这个概念，
//!    唯一的格长常量是 `:183` 的 `ValidateDimensions(54, 15)`；
//! 2. **规格书自相矛盾**：§二 主张 17，而**同一份文件** §十二 已订正为 **15** ⇒ §十二 才对；
//!    病根是「`ToInt32` 读 4 字节但**不推进** `offSet`」被误当成推进，与本仓 Type3 的
//!    40 B→36 B 是同一个病（见 `shanda2012::Shanda2012::csharp_40b_variant` 的注释）；
//! 3. **长度上无法区分 ⇒ 会静默读错**：`54 + 17·w·h` 与 `54 + 15·w·h + 2·w·h` 是
//!    **同一个长度**。若保留 17 B 档并优先判它，一张真实的 15 B/格文件只要带 `2·w·h`
//!    字节尾随，就会被判成 17 B 档 ⇒ **每格起点漂移 2 字节、除前 3 个字段外全图字段错位**；
//!    而 `write` 严格镜像 `read` ⇒ **往返测试仍然逐字节相等、测不出来**。
//!    这是本仓最危险的一类错（「理解错 ⇒ 样本也错 ⇒ 往返照样绿」）；
//! 4. **本机零真实样本**：`GodotClient/maps`（61 个）与
//!    `client/热血传奇23周年客户端/Map`（1097 个）里 **0 个**命中本支 ⇒ 17 B 分支
//!    **没有任何文件支撑**，留着只有误判风险，没有收益。
//!
//! ⇒ 现在**只有 15 B/格一档**；判据里不再出现任何「17」。样本实证见本模块
//! `尾随字节进trailer且逐格字段不漂移` 测试（258 = 54+17·12 的坏样本必须读成 15 B/格 + 24 字节尾随）。
//!
//! ## 2.3 格内字段表（15 B/格，`tail` 1 字节 = rel 14）
//!
//! | 格内偏移 | 字段 | 类型 | 磁盘变换 | 源码行号 |
//! |---------|------|------|----------|----------|
//! | 0 | 背景图号 | `u32` | **XOR `0xAA38AA38`** | `:193` |
//! | 4 | 中间图号 | `u16` | **XOR `xor`** | `:195` |
//! | 6 | 前景图号 | `u16` | **XOR `xor`** | `:196` |
//! | 8 | 门索引 | `u8` | — | `:197` |
//! | 9 | 门偏移 | `u8` | — | `:198` |
//! | 10 | 前景动画帧 | `u8` | — | `:199` |
//! | 11 | 前景动画节拍 | `u8` | — | `:200` |
//! | 12 | `btArea` / 前景库号 | `u8` | `+2` | `:201` |
//! | 13 | `btLight` | `u8` | — | `:203` |
//! | 14 | — | `u8` | 未使用（`:205` 的 `+= 2` 跳过它） | — |
//!
//! # 三、存储顺序：**列优先**
//!
//! ```text
//! offset = 54 + (x * h + y) * 15      // x 外层、y 内层（C# `:187-188`）
//! ```
//!
//! # 四、C# 的变换
//!
//! | 变换 | C# 源码 | 本模块函数 |
//! |------|---------|-----------|
//! | 背景图号 `^ 0xAA38AA38` | `:193` | [`csharp_back_image`] |
//! | 中间图号 `^ xor` | `:195` | [`csharp_middle_image`] |
//! | 前景图号 `^ xor` | `:196` | [`csharp_front_image`] |
//! | 前景库号 = `btArea + 2` | `:201` | [`csharp_front_lib`] |
//! | 有前景 = **常量 `true`**（本支没有 `if`） | `:202` | [`csharp_has_front`] |
//! | 前景带 `0x8000` ⇒ 背景 `\| 0x40000000` | `:209-210` | [`csharp_back_mark`] |
//! | **Low 碰撞位**：C# **漏搬**（9 种里唯一一种） | `:190-204` 无对应段 | [`back_image_fixed`]（**按修正语义，不照抄缺陷**） |
//!
//! ⚠ [`csharp_has_front`] 与 Type0 `:151` / Type2 `:253` / Type3 `:309` / Type4 `:364` 的
//! 「`(fr_img & 0x7FFF) > 0 && frindex > 0`」**不是一回事**：本支 `:202` 写的是
//! `HasFront = true,`（对象初始化器里的**常量赋值**）。详见 [`csharp_has_front`] 的文档。
//!
//! # 五、⚠ 原版缺陷：Type1 漏搬 Low 碰撞位（**不照抄**）
//!
//! Type0 `:147`、Type2 `:249`、Type3 `:305`、Type4 `:360`、Type7 `:543` 都有
//! 「`if ((BackImage & 0x8000) != 0) BackImage = (BackImage & 0x7FFF) | 0x20000000;`」，
//! **唯独 Type1 `:190-204` 没有**。后果：`0x8000` 原样留在 32 位 `BackImage` 里，
//! 被下游 `& 0x1FFFFFFF` 取图号时**当成图号的一部分**（图号偏大 `0x8000`）⇒ 取到错误帧。
//!
//! 本 crate 的处置：
//! - **字节层不照抄**——[`Cell::bk_img`](crate::Cell::bk_img) 存磁盘原值，往返不受影响；
//! - **语义层提供修正版** —— [`back_image_fixed`] 按「`0x8000` = Low 阻挡位、图号取低 15 位」
//!   返回，并在文档里注明这是对原版缺陷的修正，不是原版行为；
//! - 原版行为仍可用 [`csharp_back_image`] 取到，方便对照。
//!
//! # 六、判别 与 尾随字节
//!
//! - **头部谓词**（C# `:85`）：`Bytes[0]==0x10 && Bytes[2]==0x61 && Bytes[7]==0x31 && Bytes[14]==0x31`；
//! - **严格判别**（[`MapFormat::detect`]）：头部谓词 **且** 文件长度精确等于 `54 + 15·w·h`
//!   —— 与其余定长格式（`Old12` / `Heroes34` …）同一口径；
//! - **尾随字节**（[`MapFormat::read`]）：只要求「够长」（C# `ValidateDimensions` 也只查下界），
//!   多出来的字节进 [`RawMap::trailer`]、原样写回。**不再**因为多几字节就整张拒绝 ——
//!   旧版是本仓九种里唯一「带尾随字节连 `read_any_tolerant` 都救不回」的格式（审查 D1.3）。

use crate::error::FormatError;
use crate::model::{
    check_tail, i16_at, put_u16, put_u32, read_fixed, u16_at, u32_at, validate_dims, write_fixed,
    Cell, CollisionPolarity, FormatCaps, FormatId, MapFormat, RawMap,
};

/// 文件头字节数。
pub const HEADER_LEN: usize = 54;

/// 单格字节数（**唯一一档**：按 `LegacyMapReader.cs:174-205` 逐句复算，见模块文档 2.1）。
pub const CELL_LEN: usize = 15;

/// 格内尾区字节数（格内偏移 14 的 1 个未使用字节，`:205` 的 `offSet += 2` 跳过它）。
pub const TAIL_LEN: usize = 1;

/// 背景图号的 XOR 常量（C# `:193`）。
pub const BACK_XOR: u32 = 0xAA38_AA38;

/// 宽/高/中间图号/前景图号共用的 XOR 掩码在头内的偏移（C# `:178`）。
pub const XOR_OFFSET: usize = 23;

/// 格式单例。
#[derive(Debug, Clone, Copy, Default)]
pub struct Wemade2010;

impl Wemade2010 {
    /// 从头部读出 `(w, xor, h)`；不足 27 字节返回 `None`。
    fn header_triplet(bytes: &[u8]) -> Option<(i32, i32, i32)> {
        if bytes.len() < 27 {
            return None;
        }
        Some((
            i32::from(i16_at(bytes, 21)),
            i32::from(i16_at(bytes, XOR_OFFSET)),
            i32::from(i16_at(bytes, 25)),
        ))
    }
}

impl MapFormat for Wemade2010 {
    fn id(&self) -> FormatId {
        FormatId::Wemade2010
    }

    fn name(&self) -> &'static str {
        "Wemade2010（15B/格，XOR 0xAA38AA38 / xor 宽高）"
    }

    /// 头部谓词 **且** 长度精确等于 `54 + 15·w·h`。
    ///
    /// 「精确长度」这一条与 `Old12`（`old12.rs:93-100`）、`Heroes34`（`heroes34.rs:88-97`）
    /// 完全同口径：严格判别不认领「长度多一截」的文件，那种文件走
    /// [`crate::read_any_tolerant`]，多出的字节进 [`RawMap::trailer`]。
    ///
    /// ⚠ 旧版这里还接受 `54 + 17·w·h`（自造的备档），已删除 —— 理由见模块文档 2.2。
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
        bytes.len() >= 15
            && bytes[0] == 0x10
            && bytes[2] == 0x61
            && bytes[7] == 0x31
            && bytes[14] == 0x31
    }

    fn dims(&self, bytes: &[u8]) -> Option<(u16, u16)> {
        let (w, xor, h) = Self::header_triplet(bytes)?;
        let width = w ^ xor;
        let height = h ^ xor;
        if width > 0 && height > 0 {
            Some((width as u16, height as u16))
        } else {
            None
        }
    }

    fn read(&self, bytes: &[u8]) -> Result<RawMap, FormatError> {
        if bytes.len() < HEADER_LEN {
            return Err(FormatError::TooShort {
                format: FormatId::Wemade2010,
                len: bytes.len(),
                need: HEADER_LEN,
            });
        }
        let (w, xor, h) = Self::header_triplet(bytes).ok_or(FormatError::InvalidDimensions {
            format: FormatId::Wemade2010,
            width: 0,
            height: 0,
        })?;
        let (width, height) = validate_dims(FormatId::Wemade2010, w ^ xor, h ^ xor)?;
        // 与其余八种定长格式一致（`read_fixed` 的契约）：
        //   ① 只要求「够长」，不够 ⇒ `Truncated`（对齐 C# `ValidateDimensions` 的下界检查）；
        //   ② 多出来的字节**不拒绝**，原样进 `RawMap::trailer`（写回时逐字节照抄）。
        // 旧版在这里又判了一次「长度必须精确等于 54+15n 或 54+17n」，于是带尾随字节的真实
        // 文件连 `read_any_tolerant` 都读不出来 —— 本仓九种里唯一的例外（审查 D1.3）。
        read_fixed(
            FormatId::Wemade2010,
            bytes,
            HEADER_LEN,
            CELL_LEN,
            width,
            height,
            |c| {
                Ok(Cell {
                    bk_img: u32_at(c, 0),
                    mid_img: u16_at(c, 4),
                    fr_img: u16_at(c, 6),
                    door: c[8],
                    door_offset: c[9],
                    ani_frame: c[10],
                    ani_tick: c[11],
                    area: c[12],
                    light: c[13],
                    tail: vec![c[14]],
                    ..Cell::default()
                })
            },
        )
    }

    fn write(&self, map: &RawMap) -> Result<Vec<u8>, FormatError> {
        write_fixed(
            FormatId::Wemade2010,
            map,
            HEADER_LEN,
            CELL_LEN,
            |cell, out| {
                check_tail(cell, TAIL_LEN, FormatId::Wemade2010)?;
                put_u32(out, cell.bk_img);
                put_u16(out, cell.mid_img);
                put_u16(out, cell.fr_img);
                out.push(cell.door);
                out.push(cell.door_offset);
                out.push(cell.ani_frame);
                out.push(cell.ani_tick);
                out.push(cell.area);
                out.push(cell.light);
                out.push(cell.tail[0]);
                Ok(())
            },
        )
    }

    fn caps(&self) -> FormatCaps {
        FormatCaps {
            can_write: true,
            // 2026-11-05（P1-9）复核后**保持 `false`**：两条判据都不满足。
            //   判据 1：格内有未建模字节 —— 格内 rel 14 共 1 字节（`:205` 的 `offSet += 2`
            //           跳过它，C# 对它的语义**无注释**，规格书疑为「middle 层扩展或图号高位」
            //           （内部规格书《规格-地图格式全量 9 种》§11.1 第 860 行，已移出公开仓）
            //           ⇒ 若与**可编辑的**图号联动，改图号后它不会跟着变；
            //           且 `bk_img` 是 32 位、编辑视图只承载低 16 位 ⇒ 高 16 位改不到；
            //   判据 2：本机 0 张真实样本（布局只有 C# 源码依据）。
            editable: false,
            editable_caveat: "每格 1 字节未建模（格内 rel 14：C# `:205` 的 `offSet += 2` 跳过它、无注释，编辑时原样保留）；且背景图号是 32 位、编辑视图只承载低 16 位 ⇒ 高 16 位改不到；另本机 0 张真实样本（布局仅据 C# 源码 LegacyMapReader.cs:170-219）",
            ground_paint_supported: true,
            has_lib_bytes: false,
            fixed_cell_len: Some(CELL_LEN),
            has_tail: true,
            has_real_samples: false,
            collision_note: "背景图号（XOR 还原后）的 0x8000 = Low 阻挡位——但 C# `:190-204` **漏搬**该位（9 种里唯一），本 crate 提供修正版 `back_image_fixed`；前景带 0x8000 时另打 0x40000000 渲染标记，不落盘",
            collision_polarity: CollisionPolarity::Bit15SetMeansBlocked,
            source: "CrystalStudio/LegacyMapReader.cs:170-219（LoadMapType1）；格长 15 的逐句复算见本模块文档 2.1",
        }
    }

    fn header_len(&self) -> usize {
        HEADER_LEN
    }

    fn expected_len(&self, width: u16, height: u16) -> Option<u64> {
        // 15 B/格（唯一一档）的精确长度。
        Some(HEADER_LEN as u64 + width as u64 * height as u64 * CELL_LEN as u64)
    }
}

/// C# `:193`：背景图号 = 磁盘 `u32` **XOR `0xAA38AA38`**。
pub fn csharp_back_image(raw: u32) -> u32 {
    raw ^ BACK_XOR
}

/// **修正版**背景图号：在 [`csharp_back_image`] 之上补回 C# 漏掉的 Low 搬运。
///
/// C# `:190-204` 缺了 Type0 `:147-148` 那段「`0x8000` ⇒ `(v & 0x7FFF) | 0x20000000`」，
/// 导致 `0x8000` 被当成图号的一部分。本函数按修正语义返回：有 `0x8000` ⇒ 清位并或上
/// `0x20000000`（Low 阻挡标记）；否则原样。
///
/// ⚠ 这是**对原版缺陷的修正**，不是原版行为；要原版行为请用 [`csharp_back_image`]。
pub fn back_image_fixed(raw: u32) -> u32 {
    let v = csharp_back_image(raw);
    if v & 0x8000 != 0 {
        (v & 0x7FFF) | 0x2000_0000
    } else {
        v
    }
}

/// C# `:195`：中间图号 = 磁盘 `u16` XOR `xor`（`xor` 取 `i16` 位模式）。
pub fn csharp_middle_image(raw: u16, xor: i16) -> u16 {
    raw ^ (xor as u16)
}

/// C# `:196`：前景图号 = 磁盘 `u16` XOR `xor`。
pub fn csharp_front_image(raw: u16, xor: i16) -> u16 {
    raw ^ (xor as u16)
}

/// C# `:201`：前景库号 = `btArea + 2`。
pub fn csharp_front_lib(area: u8) -> u16 {
    u16::from(area) + 2
}

/// C# `:202`：本支把 `HasFront` **无条件设为 `true`**（`HasFront = true,`，常量赋值）。
///
/// # ⚠ 本支**没有** `if`
///
/// Type0 `:151` / Type2 `:253` / Type3 `:309` / Type4 `:364` 才是条件式
/// 「`(fr_img & 0x7FFF) > 0 && frindex > 0`」；本支 `:202` 是对象初始化器里的
/// **常量 `true`** ⇒ **每一格都有前景**，与 `fr_img` 的取值无关。
///
/// 返回值恒为 `true`，写成函数只为把这条源码事实显式记录下来，
/// 免得上层误以为「`fr_img == 0` 就是没前景」。
///
/// # 它影响什么：**渲染层**，不是编辑能力位
///
/// `HasFront` 是「画不画前景层」的开关（C# `:207` 的 `if (HasFront)` 因恒真而等价于
/// 无条件执行 —— 这也是 [`csharp_back_mark`] 不把它当条件的原因，那处**本来就是对的**）。
/// 按旧的条件式判据，空前景格（`fr_img` 低 15 位为 0，真实地图里占多数）的前景层会被
/// 整层跳过不画。它与 [`FormatCaps::editable`]（能不能编辑）**无关**。
pub fn csharp_has_front() -> bool {
    true
}

/// C# `:209-210`：前景带 `0x8000` ⇒ 背景图号再或上 `0x40000000`。
///
/// ⚠ `fr_img` 要传 **XOR 还原后**的值（C# `:196` 已 `^ xor` 过）：直接传
/// [`Cell::fr_img`]（磁盘原值）在 `xor` 的 bit15 为 1 时会把阻挡标记判反。
pub fn csharp_back_mark(back: u32, fr_img: u16) -> u32 {
    if (fr_img & 0x8000) != 0 {
        back | 0x4000_0000
    } else {
        back
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一张 **15 B/格** 的 Wemade2010（`w×h`），**逐格字段值互不相同**。
    ///
    /// 头按 C# `:85` 的签名 + `:176-182` 的 XOR 宽高规则填写；格数据按 `:193-205` 的
    /// 15 B 布局**手工编码**（不走 `write`，避免「用实现验实现」）。
    fn 造图(w: i16, h: i16, xor: i16, fr_img_base: u16) -> Vec<u8> {
        let cells = w as usize * h as usize;
        let mut b = vec![0u8; HEADER_LEN + CELL_LEN * cells];
        // 标题 `Map 2010 Ver 1.0`（`Bytes[0]=0x10` 是长度前缀，标题在 1..17）
        b[0] = 0x10;
        b[1..17].copy_from_slice(b"Map 2010 Ver 1.0");
        b[21..23].copy_from_slice(&(w ^ xor).to_le_bytes());
        b[23..25].copy_from_slice(&xor.to_le_bytes());
        b[25..27].copy_from_slice(&(h ^ xor).to_le_bytes());
        for i in 0..cells {
            let o = HEADER_LEN + i * CELL_LEN;
            let k = i as u8; // 格序号（超过 255 会回绕，测试只关心「逐格不同」）
            b[o..o + 4].copy_from_slice(&(0x1122_3300u32 + i as u32).to_le_bytes());
            b[o + 4..o + 6].copy_from_slice(&(0x0100u16 + i as u16).to_le_bytes());
            b[o + 6..o + 8].copy_from_slice(&fr_img_base.wrapping_add(u16::from(k)).to_le_bytes());
            b[o + 8] = k; // 门索引
            b[o + 9] = 10u8.wrapping_add(k); // 门偏移
            b[o + 10] = 20u8.wrapping_add(k); // 前景动画帧
            b[o + 11] = 30u8.wrapping_add(k); // 前景动画节拍
            b[o + 12] = 40u8.wrapping_add(k); // btArea（前景库号 = +2）
            b[o + 13] = 50u8.wrapping_add(k); // btLight
            b[o + 14] = 60u8.wrapping_add(k); // 未使用字节（进 tail）
        }
        b
    }

    #[test]
    fn 十五字节往返逐字节一致() {
        let raw = 造图(4, 3, 0x0F0F, 0x0200);
        assert_eq!(raw.len(), HEADER_LEN + CELL_LEN * 12);
        assert!(Wemade2010.detect(&raw), "精确长度应自认");
        let m = Wemade2010.read(&raw).expect("应解析成功");
        assert_eq!((m.width, m.height), (4, 3));
        assert_eq!(m.cells.len(), 12);
        assert_eq!(m.cells[0].tail.len(), TAIL_LEN);
        assert_eq!(m.trailer.len(), 0);
        assert_eq!(Wemade2010.write(&m).expect("应写回"), raw);
    }

    /// ★ **「曾被误判」的坏样本**：15 B/格 + `2·w·h` 字节尾随，长度恰好落进旧 17 B/格判据。
    ///
    /// 旧实现按「长度 == `54+17·w·h`」优先判 17 B 档 ⇒ 每格起点漂移 2 字节、全图字段错位，
    /// 而 `write` 严格镜像 `read` ⇒ 往返测试仍然绿。本测试钉住三件事：
    /// 1. 现在读成 **15 B/格 + 24 字节尾随**；
    /// 2. **逐格字段值与原 234 字节文件完全相同**（关键格的图号/库号逐个对照）；
    /// 3. `to_bytes()`（`write`）**保留那 24 字节尾随**，不丢。
    #[test]
    fn 尾随字节进trailer且逐格字段不漂移() {
        // ① 一张合法的 15 B/格 文件：w=4, h=3 ⇒ 54 + 15*12 = 234
        let 原 = 造图(4, 3, 0x0F0F, 0x0200);
        assert_eq!(原.len(), 234, "54 + 15*12 = 234");
        let 原map = Wemade2010.read(&原).expect("234 字节应能读");

        // ② 追加 2*w*h = 24 字节尾随 ⇒ 258 = 54 + 17*12（**正好落进旧 17 B/格判据**）
        let mut 坏 = 原.clone();
        坏.extend((0..24u8).map(|i| 0xA0 | (i & 0x0F)));
        assert_eq!(坏.len(), 258);
        assert_eq!(坏.len(), 54 + 17 * 12, "258 恰好等于旧「17 B/格」判据的长度");

        // ③ 严格入口：与其余定长格式一致，报 `LengthMismatch`（不猜、不硬读）
        match crate::read_any(&坏) {
            Err(FormatError::LengthMismatch {
                format,
                width,
                height,
                len,
                expected,
            }) => {
                assert_eq!(format, FormatId::Wemade2010);
                assert_eq!((width, height, len, expected), (4, 3, 258, 234));
            }
            other => panic!("严格入口应报 LengthMismatch，实际 {other:?}"),
        }

        // ④ 宽松入口：读成 15 B/格 + 24 字节尾随（**旧实现这里直接失败**）
        let 坏map = crate::read_any_tolerant(&坏).expect("宽松入口必须能读");
        assert_eq!(坏map.format, FormatId::Wemade2010);
        assert_eq!(坏map.trailer.len(), 24, "2*w*h = 24 字节应进 trailer");
        assert_eq!(坏map.cells.len(), 12);

        // ⑤ 逐格字段与原 234 字节文件**完全相同**（等价于「每格没有漂 2 字节」）
        for (i, (a, b)) in 原map.cells.iter().zip(坏map.cells.iter()).enumerate() {
            assert_eq!(a, b, "第 {i} 格（idx={i}）的字段必须与原 234 字节文件完全一致");
        }
        // 关键格的图号/库号对照（`--nocapture` 可看到实测值）
        for (x, y) in [(0u16, 0u16), (1, 0), (0, 2), (3, 2)] {
            let a = 原map.cell(x, y).expect("原文件该格在");
            let b = 坏map.cell(x, y).expect("坏样本该格在");
            println!(
                "格({x},{y}) idx={}: 原[234B] bk={:#010X} mid={:#06X} fr={:#06X} area={} \
                 / 带尾随[258B] bk={:#010X} mid={:#06X} fr={:#06X} area={}",
                x as usize * 3 + y as usize,
                a.bk_img,
                a.mid_img,
                a.fr_img,
                a.area,
                b.bk_img,
                b.mid_img,
                b.fr_img,
                b.area
            );
            assert_eq!(
                (a.bk_img, a.mid_img, a.fr_img, a.area),
                (b.bk_img, b.mid_img, b.fr_img, b.area),
                "格({x},{y}) 的图号/库号必须与原 234 字节文件完全相同"
            );
        }
        // 反证：15 B/格切法下第 1 格起点是 54+15；若按 17 B/格切（起点 54+17）就漂了 2 字节
        assert_eq!(
            u32_at(&坏, 54 + 15),
            原map.cells[1].bk_img,
            "第 1 格起点必须是 54+15"
        );
        println!(
            "旧17B切法 第1格 bk={:#010X}（真实 {:#010X}）",
            u32_at(&坏, 54 + 17),
            原map.cells[1].bk_img
        );
        assert_ne!(
            u32_at(&坏, 54 + 17),
            原map.cells[1].bk_img,
            "17 B/格切法会把第 1 格背景图号读成错值（每格漂 2 字节）"
        );

        // ⑥ 往返必须**保留**那 24 字节尾随
        let 回 = Wemade2010.write(&坏map).expect("应能写回");
        assert_eq!(回.len(), 258, "写回长度必须仍是 258（尾随 24 字节不丢）");
        assert_eq!(回, 坏, "写回必须逐字节相等（含 24 字节尾随）");
    }

    #[test]
    fn 长度不足报截断() {
        let mut raw = 造图(4, 3, 0x0F0F, 0x0200);
        raw.truncate(raw.len() - 1);
        assert!(!Wemade2010.detect(&raw), "长度不足不该自认");
        match Wemade2010.read(&raw) {
            Err(FormatError::Truncated { format, .. }) => assert_eq!(format, FormatId::Wemade2010),
            other => panic!("应报 Truncated，实际 {other:?}"),
        }
    }

    #[test]
    fn 宽高异或还原() {
        let raw = 造图(0x12, 0x34, 0x55, 0x0200);
        // 头里存的确实是异或后的值
        assert_eq!(u16_at(&raw, 21), 0x12 ^ 0x55);
        assert_eq!(u16_at(&raw, 25), 0x34 ^ 0x55);
        let m = Wemade2010.read(&raw).unwrap();
        assert_eq!(m.width, 0x12);
        assert_eq!(m.height, 0x34);
    }

    /// ★ `HasFront` 证据：C# `:202` 是**常量 `true`**，不是「`fr_img` 低 15 位非 0」的条件式。
    ///
    /// 造一张**每格前景图号都是 0** 的 Wemade2010（空前景格），断言 `has_front` 仍为 `true`。
    /// 影响面：`HasFront` 是**渲染层**「画不画前景」的开关（C# `:207` 的 `if (HasFront)` 恒真），
    /// 与 `caps.editable`（编辑能力）无关。旧条件式会让这类格（真实地图里占多数）的前景层
    /// 整层不画。
    #[test]
    fn 有前景是常量true且不受fr_img影响() {
        let mut raw = 造图(4, 3, 0x0F0F, 0x0200);
        for i in 0..12 {
            let o = HEADER_LEN + i * CELL_LEN + 6; // 格内 rel 6..8 = 前景图号
            raw[o..o + 2].copy_from_slice(&0u16.to_le_bytes());
        }
        let m = Wemade2010.read(&raw).expect("应解析成功");
        assert!(m.cells.iter().all(|c| c.fr_img == 0), "前提：每格前景图号都是 0");
        assert!(
            csharp_has_front(),
            "C# `:202` 是常量 `HasFront = true` ⇒ 恒 true（与 fr_img 无关）"
        );
        // 被删掉的那条错判据在 fr_img=0 时给 false —— 显式留证（取值来自读出的格，非字面常量）
        let 空前景 = m.cells[0].fr_img;
        assert_eq!(空前景, 0, "前提：第 0 格前景图号为 0");
        assert_eq!(
            空前景 & 0x7FFF,
            0,
            "旧条件式 `(fr_img & 0x7FFF) > 0` 在 fr_img=0 时给 false，本支不适用"
        );
        // 渲染层语义：有前景 ⇒ 前景库号也要取到（`btArea + 2`，`:201`）
        assert_eq!(csharp_front_lib(m.cells[0].area), 42);
        assert_eq!(Wemade2010.write(&m).unwrap(), raw);
    }

    #[test]
    fn 变换对齐源码() {
        assert_eq!(csharp_back_image(0), 0xAA38_AA38);
        assert_eq!(csharp_middle_image(0x1234, 0x0F0F), 0x1234 ^ 0x0F0F);
        assert_eq!(csharp_front_image(0x1234, 0x0F0F), 0x1234 ^ 0x0F0F);
        assert_eq!(csharp_front_lib(5), 7);
        assert!(csharp_has_front());
        assert_eq!(csharp_back_mark(0x10, 0x8000), 0x4000_0010);
        assert_eq!(csharp_back_mark(0x10, 0x0001), 0x10);
    }

    #[test]
    fn 修正版补回原版漏掉的low位() {
        // 磁盘 0 ⇒ XOR 后 0xAA38AA38，低 16 位 0xAA38 的 bit15 是 1
        assert_eq!(csharp_back_image(0), 0xAA38_AA38);
        // ⇒ 修正版清 bit15 并打 Low 标记 0x20000000
        assert_eq!(back_image_fixed(0), 0x2000_2A38);
        // 磁盘 0x8000 ⇒ XOR 后低 16 位变 0x2A38，bit15 = 0 ⇒ 修正版原样不动
        assert_eq!(csharp_back_image(0x8000), 0xAA38_2A38);
        assert_eq!(back_image_fixed(0x8000), 0xAA38_2A38);
    }
}
