//! 格式 `CrystalCs` —— CrystalStudio 自定义格式（魔数 `C#`），**变长格**，C# `LoadMapType100`。
//!
//! # 一、文件头（8 字节）
//!
//! | 偏移 | 字段 | 类型 | 说明 | 源码行号 |
//! |------|------|------|------|----------|
//! | 0 | `ver` | `i16` | 版本，**必须 1..=5** | `:566-567` |
//! | 2 | 魔数 `C#` | `[u8; 2]` | `0x43 0x23`，也是判别依据 | `:60-61` |
//! | 4 | `Width` | `i16` | 宽 | `:569` |
//! | 6 | `Height` | `i16` | 高 | `:571` |
//!
//! ⚠ C# `:568` 的 `offset += 4` 是「读掉 ver（2B）+ 跳过魔数（2B）」，所以宽在 **4**、高在 **6**。
//!
//! # 二、单元格（**变长**）
//!
//! C# `:572` 的 `ValidateDimensions(8, ver == 1 ? 26 : ver == 2 ? 38 : ver == 3 ? 35 : 33)`
//! 给的是**最小**格长（= 没有前景图时的长度）。有前景图时格会变长：
//!
//! | 段 | 偏移增量 | 出现条件 | 源码行号 |
//! |----|---------|----------|----------|
//! | 背景库号 `i16` | +2 | 恒有 | `:578` |
//! | 背景图号 `u32` | +4 | 恒有 | `:580` |
//! | 中间层库号 `i16` | +2 | 恒有 | `:582` |
//! | 中间图号 `u16` | +2 | 恒有 | `:584` |
//! | 前景图个数 `u8` | +1 | `ver > 2` | `:588` |
//! | 前景库号 `i16` | +2 | `ver > 2 && frcount == 1` | `:592` |
//! | 前景图号 `i16` | +2 | `ver > 2 && frcount == 1` | `:594` |
//! | 前景动画帧 + 节拍 `u8`×2 | +2 | `ver > 3 && frcount == 1` | `:600-601` |
//! | 未建模 `u8` | +1 | `ver > 4 && frcount == 1` | `:602` |
//! | 前景库号 `i16` | +2 | `ver <= 2` | `:613` |
//! | 前景图号 `i16` | +2 | `ver <= 2` | `:615` |
//! | 门索引 `u8`（语义 `& 0x7F`） | +1 | 恒有 | `:625` |
//! | 门偏移 `u8` | +1 | 恒有 | `:626` |
//! | 前景动画帧 + 节拍 `u8`×2 | +2 | `ver < 4 && (HasFront \|\| ver <= 2)` | `:631-632` |
//! | 未建模 `u8`×2 | +2 | `ver < 4 && !HasFront && ver > 2`（`offset += 2` 跳过） | `:636` |
//! | 中间层动画帧 `u8` | +1 | 恒有 | `:639` |
//! | 中间层动画节拍 `u8` | +1 | 恒有 | `:640` |
//! | `TileAnimationImage` `i16` | +2 | 恒有 | `:641` |
//! | `TileAnimationOffset` `i16` | +2 | 恒有 | `:643` |
//! | `TileAnimationFrames` `u8` | +1 | 恒有 | `:645` |
//! | 光照 `u8` | +1 | 恒有 | `:646` |
//! | 未建模 `u8`×12 | +12 | `ver >= 2`（`:650` `+8`、`:651` `+2`、`:652` `+2`） | — |
//!
//! 由此推得各版本的最小/最大格长：
//!
//! | ver | 最小（`frcount == 0`） | 最大（`frcount == 1`） |
//! |-----|----------------------|----------------------|
//! | 1 | **26** | 26（无 `frcount` 字节，恒按 4 字节前景写） |
//! | 2 | **38** | 38 |
//! | 3 | **35** | 39 |
//! | 4 | **33** | 39 |
//! | 5 | **33** | 40 |
//!
//! ⇒ 格长**不是常量**，只能顺序解析（C# 也是顺序推进 `offset`，从不按固定步长跳）。
//!
//! # 三、`HasFront` 的判定（决定 `ver < 4` 那一支走哪条路）
//!
//! - `ver > 2`：`HasFront = (frcount == 1)`（`:590-591`）；
//! - `ver <= 2`：`HasFront = (frindex != 0 || frimage != 0)`（`:621`）；
//! - **`ver < 4` 读到前景动画帧/节拍之后**（C# `LegacyMapReader.cs:631-633`）：
//!   `HasFront |= (FrontAnimationFrame != 0 || FrontAnimationTick != 0)`。
//!
//! ⚠ 第三条**只影响语义，不改变任何字节**：C# `:629` 的分支判断用的是**更新前**的
//! `HasFront`，`offset` 已经在 `:631-632` 按同一支推进过了。因此本模块 `read`/`write`
//! 的**分支仍用前两条**（[`CrystalCs::has_front`]），第三条只在
//! [`csharp_has_front`] 里补 —— 那是给上层「这一格到底有没有前景」用的完整值。
//!
//! ⚠ 第三条实际只在 `ver ∈ {1,2}` 上改变结果：`ver == 3` 时那一支只有
//! `frcount == 1` 才会走（`:629` 的 `HasFront` 已为真），`ver > 3` 则根本不进 `:627` 的块。
//!
//! # 四、写回：**全部版本都支持**
//!
//! 任务书允许「只写 v2 或明确报不支持写」。本 crate 选择**全版本可写**，理由：
//! 读路径已经把每一段字节（含所有「未建模跳过」段）按固定顺序落进 [`Cell`] 的具名字段与
//! [`Cell::tail`]，写路径只是把同一顺序倒过来；变长部分完全由 `(ver, fr_count)` 决定，
//! 没有任何不可逆信息。逐版本（v1..v5 × `frcount` 0/1）的字节级往返由
//! `tests/synthetic.rs::crystal_cs_全版本往返` 覆盖。
//!
//! # 五、存储顺序：**列优先**
//!
//! ```text
//! 第 i 格（i = x*h + y）的起点 = 8 + 前面 i 格的格长之和
//! ```
//!
//! C# `:574-575` 是 `for (x) for (y)` 双层循环，`offset` 在内层单调推进 ⇒ 列优先。
//!
//! # 六、真实样本
//!
//! **无**（本机 1158 个真实 `.map` 里 0 个带 `C#` 魔数）⇒ 只有合成样本验证。
//!
//! # 七、⚠ `editable = false`：本格式是「错了也测不出来」的最高风险项
//!
//! 2026-11-05 把 [`FormatCaps::editable`](crate::FormatCaps::editable) 从 `true` **收紧为 `false`**，
//! 依据内部取证文档《审查-mir2-format 九格式》§五（已移出公开仓，不随本仓分发）。
//!
//! **理由**：本格式是九种里**唯一「布局理解错了、连字节级往返都照样通过」**的格式，三条叠加：
//!
//! 1. **格长变长**（26/38/35/33+，由 `(ver, fr_count)` 决定）⇒ 任何字段顺序错位都会被
//!    「读多几字节 / 读少几字节」吸收，格边界自己会漂到能自洽的位置；
//! 2. **`trailer` 把多余字节全吞掉**（`read` 里 `trailer = bytes[cur.off..]`，`write` 原样追加）
//!    ⇒ 读错后剩下的字节照样逐字节写回；
//! 3. **`detect` 只查下界**（`detect` 里 `len >= min_len`，对齐 C# `:572` 的 `ValidateDimensions`）
//!    ⇒ 尾块长度任意都算合法文件。
//!
//! 而它**零真实样本**（本机 1158 个 `.map` 里 0 个带 `C#` 魔数）。
//! ⇒ 字段顺序一旦有误，会在「可编辑 + 往返全绿」的双重假象下**把真实数据写坏**。
//!
//! **处置**：`can_write = true`（能写；不改格时字节级往返仍然成立）、`editable = false`
//! （**不承诺改格后语义自洽**）。上层编辑器**不得**在本格式上开放「改格」功能。
//! **待拿到真实 Type100 样本、并用 C# 双向对读验证字段顺序之后，才可改回 `true`。**

use crate::error::FormatError;
use crate::model::{
    i16_at, put_i16, put_u16, validate_dims, validate_write_dims, Cell, CollisionPolarity,
    FormatCaps, FormatId, MapFormat, RawMap,
};

/// 文件头字节数。
pub const HEADER_LEN: usize = 8;

/// 魔数 `C#` 的两个字节。
pub const MAGIC: [u8; 2] = [0x43, 0x23];

/// 版本允许范围（C# `:567`）。
pub const VER_MIN: i16 = 1;
/// 版本允许范围（C# `:567`）。
pub const VER_MAX: i16 = 5;

/// 某版本的最小格长（C# `:572` 的 `ValidateDimensions` 第三个参数）。
pub const fn min_cell_len(ver: i16) -> usize {
    match ver {
        1 => 26,
        2 => 38,
        3 => 35,
        _ => 33,
    }
}

/// 格式单例。
#[derive(Debug, Clone, Copy, Default)]
pub struct CrystalCs;

/// 带越界检查的顺序游标（变长格必须边走边查）。
struct Cursor<'a> {
    b: &'a [u8],
    off: usize,
    format: FormatId,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], FormatError> {
        if self.off + n > self.b.len() {
            return Err(FormatError::Truncated {
                format: self.format,
                len: self.b.len(),
                need: (self.off + n) as u64,
            });
        }
        let s = &self.b[self.off..self.off + n];
        self.off += n;
        Ok(s)
    }

    fn u8(&mut self) -> Result<u8, FormatError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, FormatError> {
        let s = self.take(2)?;
        Ok(u16::from_le_bytes([s[0], s[1]]))
    }

    fn i16(&mut self) -> Result<i16, FormatError> {
        let s = self.take(2)?;
        Ok(i16::from_le_bytes([s[0], s[1]]))
    }

    fn u32(&mut self) -> Result<u32, FormatError> {
        let s = self.take(4)?;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
}

impl CrystalCs {
    /// 读版本号（不足 2 字节返回 `None`）。
    pub fn version(bytes: &[u8]) -> Option<i16> {
        if bytes.len() < 2 {
            return None;
        }
        Some(i16_at(bytes, 0))
    }

    /// 本格式的最小文件长度（`8 + min_cell*w*h`）。
    pub fn min_len(ver: i16, width: u16, height: u16) -> u64 {
        HEADER_LEN as u64 + min_cell_len(ver) as u64 * width as u64 * height as u64
    }

    fn header_dims(bytes: &[u8]) -> Option<(i32, i32)> {
        if bytes.len() < HEADER_LEN {
            return None;
        }
        Some((i32::from(i16_at(bytes, 4)), i32::from(i16_at(bytes, 6))))
    }

    /// `HasFront` 判定（见模块文档第三节）。
    ///
    /// ⚠ 这里只算 **C# `:590-591` / `:621` 两条**，也就是 `:629` 分支判断**当时**的值
    /// —— C# `:633` 的 `HasFront |= (frame != 0 || tick != 0)` 发生在分支判断**之后**，
    /// 不改变任何偏移。完整值（含 `:633`）见 [`csharp_has_front`]。
    const fn has_front(ver: i16, fr_count: u8, fr_idx: i16, fr_img: u16) -> bool {
        if ver > 2 {
            fr_count == 1
        } else {
            fr_idx != 0 || fr_img != 0
        }
    }
}

impl MapFormat for CrystalCs {
    fn id(&self) -> FormatId {
        FormatId::CrystalCs
    }

    fn name(&self) -> &'static str {
        "CrystalCs（Type100，变长格 26/38/35/33+，魔数 C#）"
    }

    fn detect(&self, bytes: &[u8]) -> bool {
        if !self.header_signature(bytes) {
            return false;
        }
        let Some(ver) = Self::version(bytes) else {
            return false;
        };
        if !(VER_MIN..=VER_MAX).contains(&ver) {
            return false;
        }
        match self.dims(bytes) {
            Some((w, h)) => (bytes.len() as u64) >= Self::min_len(ver, w, h),
            None => false,
        }
    }

    /// C# `:60`：`Bytes[2] == 0x43 && Bytes[3] == 0x23`（`"C#"`）。
    fn header_signature(&self, bytes: &[u8]) -> bool {
        bytes.len() >= 4 && bytes[2] == MAGIC[0] && bytes[3] == MAGIC[1]
    }

    fn dims(&self, bytes: &[u8]) -> Option<(u16, u16)> {
        let ver = Self::version(bytes)?;
        if !(VER_MIN..=VER_MAX).contains(&ver) {
            return None;
        }
        let (w, h) = Self::header_dims(bytes)?;
        if w > 0 && h > 0 {
            Some((w as u16, h as u16))
        } else {
            None
        }
    }

    fn read(&self, bytes: &[u8]) -> Result<RawMap, FormatError> {
        if bytes.len() < HEADER_LEN {
            return Err(FormatError::TooShort {
                format: FormatId::CrystalCs,
                len: bytes.len(),
                need: HEADER_LEN,
            });
        }
        let ver = Self::version(bytes).unwrap_or(0);
        if !(VER_MIN..=VER_MAX).contains(&ver) {
            return Err(FormatError::UnsupportedVersion { ver: ver as u16 });
        }
        let (w, h) = Self::header_dims(bytes).ok_or(FormatError::InvalidDimensions {
            format: FormatId::CrystalCs,
            width: 0,
            height: 0,
        })?;
        let (width, height) = validate_dims(FormatId::CrystalCs, w, h)?;
        let count = width as usize * height as usize;
        let min = Self::min_len(ver, width, height);
        if (bytes.len() as u64) < min {
            return Err(FormatError::Truncated {
                format: FormatId::CrystalCs,
                len: bytes.len(),
                need: min,
            });
        }

        let mut cur = Cursor {
            b: bytes,
            off: HEADER_LEN,
            format: FormatId::CrystalCs,
        };
        let mut cells = Vec::with_capacity(count);
        for _ in 0..count {
            // 前四个字段是恒定段（所有版本都有），先读完再建结构体
            let bk_idx = cur.i16()?;
            let bk_img = cur.u32()?;
            let mid_idx = cur.i16()?;
            let mid_img = cur.u16()?;
            let mut cell = Cell {
                bk_idx,
                bk_img,
                mid_idx,
                mid_img,
                ..Cell::default()
            };
            if ver > 2 {
                cell.fr_count = cur.u8()?;
                if cell.fr_count > 1 {
                    return Err(FormatError::inconsistent(
                        "CrystalCs 每格最多一张前景图（frcount > 1）",
                    ));
                }
                if cell.fr_count == 1 {
                    cell.fr_idx = cur.i16()?;
                    cell.fr_img = cur.u16()?;
                    if ver > 3 {
                        cell.ani_frame = cur.u8()?;
                        cell.ani_tick = cur.u8()?;
                        if ver > 4 {
                            cell.tail.push(cur.u8()?);
                        }
                    }
                }
            } else {
                cell.fr_idx = cur.i16()?;
                cell.fr_img = cur.u16()?;
            }
            cell.door = cur.u8()?;
            cell.door_offset = cur.u8()?;
            if ver < 4 {
                // C# `:629` 的条件是 `HasFront || ver <= 2` ⇒ `ver <= 2` 时**必定**读这两字节。
                if ver <= 2 || Self::has_front(ver, cell.fr_count, cell.fr_idx, cell.fr_img) {
                    cell.ani_frame = cur.u8()?;
                    cell.ani_tick = cur.u8()?;
                } else {
                    cell.tail.extend_from_slice(cur.take(2)?);
                }
            }
            cell.mid_ani_frame = cur.u8()?;
            cell.mid_ani_tick = cur.u8()?;
            cell.tiles = cur.u16()?;
            cell.sm_tiles = cur.u16()?;
            cell.tile_frames = cur.u8()?;
            cell.light = cur.u8()?;
            if ver >= 2 {
                cell.tail.extend_from_slice(cur.take(12)?);
            }
            cells.push(cell);
        }
        Ok(RawMap {
            format: FormatId::CrystalCs,
            width,
            height,
            header_raw: bytes[..HEADER_LEN].to_vec(),
            cells,
            trailer: bytes[cur.off..].to_vec(),
        })
    }

    fn write(&self, map: &RawMap) -> Result<Vec<u8>, FormatError> {
        // 尺寸门：CrystalCs 是变长格式、不走 `write_fixed`，所以必须自己挂在最顶部。
        // 与读侧 `validate_dims` 同一判据（见 `validate_write_dims` 文档）；
        // 放在任何窄化转换之前 ⇒ 超 32767 的宽高不会静默截断成一个别的尺寸。
        validate_write_dims(FormatId::CrystalCs, map.width, map.height)?;
        if map.format != FormatId::CrystalCs {
            return Err(FormatError::inconsistent(format!(
                "目标格式是 CrystalCs，而 RawMap.format 是 {:?}",
                map.format
            )));
        }
        if map.header_raw.len() != HEADER_LEN {
            return Err(FormatError::inconsistent(format!(
                "CrystalCs 的 header_raw 必须是 {HEADER_LEN} 字节，实际 {} 字节",
                map.header_raw.len()
            )));
        }
        let ver = Self::version(&map.header_raw).unwrap_or(0);
        if !(VER_MIN..=VER_MAX).contains(&ver) {
            return Err(FormatError::UnsupportedVersion { ver: ver as u16 });
        }
        let count = map.width as usize * map.height as usize;
        if map.cells.len() != count {
            return Err(FormatError::inconsistent(format!(
                "CrystalCs 的 w*h = {}×{} = {count}，而 cells.len() = {}",
                map.width,
                map.height,
                map.cells.len()
            )));
        }
        let mut out = Vec::with_capacity(map.header_raw.len() + count * min_cell_len(ver));
        out.extend_from_slice(&map.header_raw);
        for (i, cell) in map.cells.iter().enumerate() {
            let mut t = 0usize; // 已消费的 tail 字节数
            put_i16(&mut out, cell.bk_idx);
            out.extend_from_slice(&cell.bk_img.to_le_bytes());
            put_i16(&mut out, cell.mid_idx);
            put_u16(&mut out, cell.mid_img);
            if ver > 2 {
                if cell.fr_count > 1 {
                    return Err(FormatError::inconsistent(format!(
                        "CrystalCs 第 {i} 格 fr_count = {}，最多 1",
                        cell.fr_count
                    )));
                }
                out.push(cell.fr_count);
                if cell.fr_count == 1 {
                    put_i16(&mut out, cell.fr_idx);
                    put_u16(&mut out, cell.fr_img);
                    if ver > 3 {
                        out.push(cell.ani_frame);
                        out.push(cell.ani_tick);
                        if ver > 4 {
                            out.push(tail_at(cell, i, &mut t)?);
                        }
                    }
                }
            } else {
                put_i16(&mut out, cell.fr_idx);
                put_u16(&mut out, cell.fr_img);
            }
            out.push(cell.door);
            out.push(cell.door_offset);
            if ver < 4 {
                if ver <= 2 || Self::has_front(ver, cell.fr_count, cell.fr_idx, cell.fr_img) {
                    out.push(cell.ani_frame);
                    out.push(cell.ani_tick);
                } else {
                    out.push(tail_at(cell, i, &mut t)?);
                    out.push(tail_at(cell, i, &mut t)?);
                }
            }
            out.push(cell.mid_ani_frame);
            out.push(cell.mid_ani_tick);
            put_u16(&mut out, cell.tiles);
            put_u16(&mut out, cell.sm_tiles);
            out.push(cell.tile_frames);
            out.push(cell.light);
            if ver >= 2 {
                for _ in 0..12 {
                    out.push(tail_at(cell, i, &mut t)?);
                }
            }
            if t != cell.tail.len() {
                return Err(FormatError::inconsistent(format!(
                    "CrystalCs 第 {i} 格的 tail 长度不对：该版本应消费 {t} 字节，实际给了 {} 字节",
                    cell.tail.len()
                )));
            }
        }
        out.extend_from_slice(&map.trailer);
        Ok(out)
    }

    fn caps(&self) -> FormatCaps {
        FormatCaps {
            can_write: true,
            // ★ 2026-11-05 从 `true` 收紧为 `false`（依据审查报告 §五「最高风险一句话」）。
            //
            // 本格式是九种里**唯一「布局理解错了、连字节级往返都照样通过」**的格式，三条叠加：
            //   ① 格长**变长**（由 `(ver, fr_count)` 决定）⇒ 字段顺序错位会被格边界漂移吸收；
            //   ② `trailer = bytes[cur.off..]` 把多余字节**全吞掉**、写回照样原样吐出；
            //   ③ `detect` 只查**下界**（`len >= min_len`，对齐 C# `:572`）⇒ 尾块长度任意都合法。
            // 而它**零真实样本**（本机 1158 个 .map 里 0 个带 `C#` 魔数）。
            // ⇒ 字段顺序一旦有误，会在「可编辑 + 往返全绿」的双重假象下**把真实数据写坏**。
            //
            // 处置：`can_write = true`（能写，且不改格时字节级往返仍然成立），
            //       `editable = false`（**不承诺改格后语义自洽**）。
            // **待拿到真实 Type100 样本、并用 C# 双向对读验证字段顺序之后，才可改回 `true`。**
            editable: false,
            editable_caveat: "格长变长（26/38/35/33/39/40，由 ver 与 fr_count 决定）；ver >= 2 每格 12 字节、ver > 4 另 1 字节未建模（编辑时原样保留）；背景图号 32 位、tiles/sm_tiles 高 8 位不在编辑视图里 ⇒ 改不到；且本机 0 张真实样本 —— 字段顺序一旦有误，连字节级往返都照样通过，测不出来",
            // 背图是格内 `u32`（C# `:580`）且 `write` 真的写它 ⇒ 逐格落笔有效。
            ground_paint_supported: true,
            has_lib_bytes: true,
            fixed_cell_len: None,
            has_tail: true,
            has_real_samples: false,
            collision_note: "前景图号（`i16`）的 0x8000 位在 ver > 1 时给背景打 0x40000000 阻挡标记（C# `:595-596`、`:622-623`）；门索引语义上要 `& 0x7F`（`:625`）",
            collision_polarity: CollisionPolarity::NativeFlags,
            source: "CrystalStudio/LegacyMapReader.cs:561-661（LoadMapType100）",
        }
    }

    fn header_len(&self) -> usize {
        HEADER_LEN
    }

    fn expected_len(&self, _width: u16, _height: u16) -> Option<u64> {
        // 变长格：没有精确长度，只有下界（见 `CrystalCs::min_len`）。
        None
    }
}

/// 从 `cell.tail` 里按顺序取第 `*t` 个字节并推进游标。
fn tail_at(cell: &Cell, idx: usize, t: &mut usize) -> Result<u8, FormatError> {
    match cell.tail.get(*t) {
        Some(v) => {
            *t += 1;
            Ok(*v)
        }
        None => Err(FormatError::inconsistent(format!(
            "CrystalCs 第 {idx} 格的 tail 太短：还需要第 {} 个字节，但只有 {} 个",
            *t,
            cell.tail.len()
        ))),
    }
}

/// C# `:625`：门索引语义 = `原始字节 & 0x7F`。
pub fn csharp_door_index(raw: u8) -> u8 {
    raw & 0x7F
}

/// C# `:595-596` / `:622-623`：前景图号带 `0x8000` ⇒ 背景打 `0x40000000`（仅 `ver > 1` / `ver > 2`）。
pub fn csharp_back_mark(back: u32, fr_img: u16, ver: i16) -> u32 {
    if ver > 1 && (fr_img & 0x8000) != 0 {
        back | 0x4000_0000
    } else {
        back
    }
}

/// C# `:621`（`ver <= 2`）：`HasFront = (frindex != 0 || frimage != 0)`。
pub fn csharp_has_front_v1v2(fr_idx: i16, fr_img: u16) -> bool {
    fr_idx != 0 || fr_img != 0
}

/// 完整 `HasFront`（= C# 把一整格读完之后的值），**含 `LegacyMapReader.cs:631-633` 的那条 OR 更新**。
///
/// C# `:631-633`：
/// ```csharp
/// if (MapCells[x, y].HasFront || ver <= 2) {
///     MapCells[x, y].FrontAnimationFrame = Bytes[offset++];
///     MapCells[x, y].FrontAnimationTick  = Bytes[offset++];
///     MapCells[x, y].HasFront |= MapCells[x, y].FrontAnimationFrame != 0
///                             || MapCells[x, y].FrontAnimationTick  != 0;
/// }
/// ```
/// ⇒ `ver < 4` 且那一支真走了、且两字节非 0 时，`HasFront` 会被**从假翻成真**。
///
/// ⚠ **字节层无影响**：这条更新在 C# `:629` 的分支判断**之后**，`offset` 已经推进完了。
/// 本模块 `read`/`write` 的分支仍用 [`CrystalCs::has_front`]（更新前的值），所以往返不变。
///
/// ⚠ 实际只有 `ver ∈ {1,2}` 会被它改变：`ver == 3` 时那一支只在 `frcount == 1`
/// （`HasFront` 已为真）时走；`ver > 3` 不进 `:627` 的块。
///
/// 入参都是 [`Cell`] 的字段：`fr_count` = `cell.fr_count`、`fr_idx` = `cell.fr_idx`、
/// `fr_img` = `cell.fr_img`、`ani_frame` = `cell.ani_frame`、`ani_tick` = `cell.ani_tick`。
pub fn csharp_has_front(
    ver: i16,
    fr_count: u8,
    fr_idx: i16,
    fr_img: u16,
    ani_frame: u8,
    ani_tick: u8,
) -> bool {
    let base = CrystalCs::has_front(ver, fr_count, fr_idx, fr_img);
    // C# `:627-637`：`ver < 4` 且（`HasFront` 或 `ver <= 2`）才读这两字节并做 OR 更新。
    if ver < 4 && (base || ver <= 2) {
        base || ani_frame != 0 || ani_tick != 0
    } else {
        base
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 按源码手工拼一张 `ver` 版、`w×h` 的 CrystalCs 字节流（**无真实样本**）。
    /// `with_front` 决定每格是否带前景图。
    fn 造图(ver: i16, w: u16, h: u16, with_front: bool) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&ver.to_le_bytes());
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&(w as i16).to_le_bytes());
        out.extend_from_slice(&(h as i16).to_le_bytes());
        for i in 0..(w as usize * h as usize) {
            out.extend_from_slice(&(i as i16).to_le_bytes()); // 背景库号
            out.extend_from_slice(&(0x1000u32 + i as u32).to_le_bytes()); // 背景图号
            out.extend_from_slice(&(2i16).to_le_bytes()); // 中间层库号
            out.extend_from_slice(&(0x2000u16 + i as u16).to_le_bytes()); // 中间图号
            if ver > 2 {
                out.push(u8::from(with_front));
                if with_front {
                    out.extend_from_slice(&(7i16).to_le_bytes());
                    out.extend_from_slice(&(0x3000u16).to_le_bytes());
                    if ver > 3 {
                        out.push(0x11);
                        out.push(0x22);
                        if ver > 4 {
                            out.push(0x33);
                        }
                    }
                }
            } else {
                out.extend_from_slice(&(if with_front { 7i16 } else { 0i16 }).to_le_bytes());
                out.extend_from_slice(&(if with_front { 0x3000u16 } else { 0u16 }).to_le_bytes());
            }
            out.push(0x81); // 门索引（高位 0x80 用于验证「原样保留」）
            out.push(0x02); // 门偏移
            if ver < 4 {
                if with_front || ver <= 2 {
                    out.push(0x44);
                    out.push(0x55);
                } else {
                    out.push(0x66);
                    out.push(0x77);
                }
            }
            out.push(0x08); // 中间层动画帧
            out.push(0x09); // 中间层动画节拍
            out.extend_from_slice(&(0x1234i16).to_le_bytes()); // TileAnimationImage
            out.extend_from_slice(&(0x5678i16).to_le_bytes()); // TileAnimationOffset
            out.push(0x0A); // TileAnimationFrames
            out.push(0x0B); // 光照
            if ver >= 2 {
                for k in 0..12u8 {
                    out.push(0xC0 + k);
                }
            }
        }
        out
    }

    #[test]
    fn 各版本各前景分支都逐字节往返() {
        for ver in 1..=5i16 {
            for with_front in [false, true] {
                let raw = 造图(ver, 3, 2, with_front);
                let m = CrystalCs
                    .read(&raw)
                    .unwrap_or_else(|e| panic!("ver={ver} front={with_front} 应解析成功：{e}"));
                assert_eq!((m.width, m.height), (3, 2));
                assert_eq!(m.cells.len(), 6);
                assert_eq!(
                    CrystalCs
                        .write(&m)
                        .unwrap_or_else(|e| panic!("ver={ver} front={with_front} 应写回成功：{e}")),
                    raw,
                    "ver={ver} front={with_front} 往返不一致"
                );
            }
        }
    }

    #[test]
    fn 门索引高位原样保留() {
        let raw = 造图(2, 1, 1, true);
        let m = CrystalCs.read(&raw).unwrap();
        let c = m.cell(0, 0).unwrap();
        assert_eq!(c.door, 0x81, "磁盘原值必须原样保留");
        assert_eq!(csharp_door_index(c.door), 0x01, "语义值要 & 0x7F");
    }

    #[test]
    fn 前景动画两字节非零也算有前景() {
        // ver=2、`with_front = false` ⇒ C# `:621` 判 `HasFront = false`（前景两字段都为 0）；
        // 但 `:629` 的 `|| ver <= 2` 让那一支照样走，读到动画两字节 0x44 / 0x55
        // ⇒ C# `:633` 把 `HasFront` 置真（这就是审查 D6.1 漏掉的那条）。
        let raw = 造图(2, 1, 1, false);
        let m = CrystalCs.read(&raw).expect("应解析成功");
        let c = m.cell(0, 0).expect("第 0 格存在");
        assert_eq!((c.fr_idx, c.fr_img), (0, 0), "磁盘前景两字段为 0");
        assert_eq!((c.ani_frame, c.ani_tick), (0x44, 0x55), "动画两字节落进 Cell");
        assert!(
            !CrystalCs::has_front(2, c.fr_count, c.fr_idx, c.fr_img),
            "`:629` 分支判断当时是假"
        );
        assert!(
            csharp_has_front(2, c.fr_count, c.fr_idx, c.fr_img, c.ani_frame, c.ani_tick),
            "`:633` 的 OR 之后翻真"
        );
        // 字节层不受影响：这条更新在分支判断之后，往返仍逐字节一致。
        assert_eq!(CrystalCs.write(&m).expect("应写回"), raw);

        // 两字节都为 0 ⇒ 仍是假（这条 OR 不是无条件置真）。
        assert!(!csharp_has_front(2, 0, 0, 0, 0, 0));
        // ver=3 且 frcount==0 ⇒ C# `:636` 只是跳过 2 字节、不做更新 ⇒ 假。
        assert!(!csharp_has_front(3, 0, 0, 0, 0x44, 0x55));
        // ver=3 且 frcount==1 ⇒ 本来就有前景。
        assert!(csharp_has_front(3, 1, 7, 0x3000, 0, 0));
        // ver=4 不进 `:627` 的块 ⇒ 动画两字节不参与。
        assert!(!csharp_has_front(4, 0, 0, 0, 0x44, 0x55));
    }

    #[test]
    fn 最小格长与源码常量一致() {
        assert_eq!(min_cell_len(1), 26);
        assert_eq!(min_cell_len(2), 38);
        assert_eq!(min_cell_len(3), 35);
        assert_eq!(min_cell_len(4), 33);
        assert_eq!(min_cell_len(5), 33);
        // 实测：造出来的图长度正好等于「8 + 每格实际长度 × 格数」
        let raw = 造图(3, 2, 2, false);
        assert_eq!(raw.len(), HEADER_LEN + min_cell_len(3) * 4);
        let raw = 造图(3, 2, 2, true);
        assert_eq!(raw.len(), HEADER_LEN + 39 * 4, "ver3 带前景应是 39 B/格");
    }

    #[test]
    fn 版本越界报错() {
        let mut raw = 造图(2, 1, 1, false);
        raw[0..2].copy_from_slice(&9i16.to_le_bytes());
        assert!(!CrystalCs.detect(&raw));
        assert!(matches!(
            CrystalCs.read(&raw),
            Err(FormatError::UnsupportedVersion { ver: 9 })
        ));
    }

    #[test]
    fn 变换对齐源码() {
        assert_eq!(csharp_door_index(0xFF), 0x7F);
        assert_eq!(csharp_back_mark(0, 0x8000, 1), 0);
        assert_eq!(csharp_back_mark(0, 0x8000, 2), 0x4000_0000);
        assert!(csharp_has_front_v1v2(1, 0));
        assert!(!csharp_has_front_v1v2(0, 0));
    }
}
