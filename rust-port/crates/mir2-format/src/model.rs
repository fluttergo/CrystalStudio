//! 公共模型：`FormatId` / `Cell` / `RawMap` / `FormatCaps` / `MapFormat` 契约，以及
//! 各格式模块共用的定长格读写骨架。
//!
//! # 一、`Cell` 为什么比「规格里的 u16」宽
//!
//! 任务书给的 `Cell` 骨架是 `bk_img: u16, mid_img: u16, fr_img: u16, door, door_offset,
//! ani_frame, ani_tick, area, light, tiles, sm_tiles, tail`。落地时发现三处必须扩宽/加字段，
//! 否则**装不下真实字节**（扩宽理由逐条对应源码行号，全部有据）：
//!
//! | 字段 | 规格 | 实际 | 依据 |
//! |------|------|------|------|
//! | `bk_img` | `u16` | **`u32`** | `Wemade2010` 的背景图号是 **4 字节**（`LegacyMapReader.cs:193` `ToInt32(...) ^ 0xAA38AA38`），`Heroes34` 也是 4 字节（`:530` `ToInt32`） |
//! | `bk_idx` / `mid_idx` / `fr_idx` | 无 | **新增 `i16`** | `Shanda14` 的 back/middle 库号在格内是 1 字节（`:247-248`），但 `CrystalCs` 的库号是 `i16`（`:578`/`:582`/`:592`）；`Mir3Wemade`/`Mir3Shanda` 的库号带 `-1` 哨兵（`:404`/`:470`）。统一用 `i16` 装「原样值」可同时覆盖 1 字节（存 0..=255）与 2 字节两种形态，无损 |
//! | `tile_frames` | 无 | **新增 `u8`** | `TileAnimationFrames`（`:302`、`:645`）是格中间的 1 字节，用 `tail` 装会让「读/写交错切分」变得不可验证 |
//! | `mid_ani_frame` / `mid_ani_tick` | 无 | **新增 `u8`×2** | `Mir3`/`CrystalCs` 的中间层动画帧与节拍（`:416`、`:639-640`）与前景层是**两组不同字节** |
//! | `fr_count` | 无 | **新增 `u8`** | `CrystalCs` v3+ 每格开头有「前景图个数」字节（`:588`），它是**变长格**的开关：写回时必须原样重现，否则格长漂移 |
//!
//! # 二、`Cell` 里存的是「原样值」，不是「渲染值」
//!
//! 这是字节级往返的**前提**。C# 在读的时候就做了变换（例：`BackImage & 0x7FFF | 0x20000000`、
//! `Bytes[offset] + 120`、`light & 0x0F` 再 `* 4`），若照抄那些值再写回，**必然对不上原文件**。
//!
//! ⇒ 本 crate 的 [`Cell`] 一律存**磁盘原样字节**；C# 的每一种变换都在对应格式模块里
//! 实现成**独立函数**（名字统一带 `csharp_` 前缀）并标注源码行号，供上层按需调用。
//!
//! # 二·五、语义值 vs 磁盘值：C# 的 5 组硬编码常量字段（2026-11-05 订正）
//!
//! C# 有 **5 组**（共 **8 条赋值语句**）字段**不是从文件读的**，而是读循环里写死的常量。
//! 本 crate 存「磁盘值」，这些字段在磁盘上**没有位置**，因此：
//!
//! | 字段 | 常量值 | 出现在哪些格式 | C# 行号 |
//! |------|--------|----------------|---------|
//! | [`Cell::mid_idx`]（`MiddleIndex`） | **1** | `Old12` / `Wemade2010` / `AntiHack` / `Heroes34` | `:134` / `:194` / `:347` / `:531` |
//! | [`Cell::mid_ani_tick`]（`MiddleAnimationTick`） | **1** | `Mir3Shanda` | `:489` |
//! | [`Cell::ani_tick`]（`franimationtick`） | **1** | `Mir3Shanda` | `:490` |
//! | [`Cell::mid_ani_tick`] | **0** | `Mir3Wemade` | `:420` |
//! | [`Cell::ani_tick`] | **0** | `Mir3Wemade` | `:421` |
//!
//! **本 crate 的处置：上表 5 组一律留 `0`（`Cell::default()` 的默认值），不虚构 C# 的常量。**
//! ⚠ 仅限**上表这 5 组**；同名字段在别的格式里是**真实磁盘字段**（例如 `mid_idx` 在
//! `Shanda14`/`Shanda2012`/`Mir3Wemade`/`Mir3Shanda`/`CrystalCs` 里照常读写），不受此影响。
//!
//! **为什么不影响往返**：上表 5 组所在的格式，其 `write` **都不写对应字段** ——
//! 磁盘格内根本没有它们的位置：
//!
//! - `old12.rs` / `wemade2010.rs` / `antihack.rs` / `heroes34.rs` 的写路径里**没有 `mid_idx`**；
//! - `mir3_shanda.rs` 的写路径里**没有 `mid_ani_tick`、没有 `ani_tick`**；
//! - `mir3_wemade.rs` 的写路径里**没有 `mid_ani_tick`、没有 `ani_tick`**。
//!
//! ⇒ 存 0 还是存 1，写出的字节**完全相同**（`tests/real_samples.rs` 的 1,158 张往返即证）。
//!
//! ⚠ **但上层必须分清两个概念**：
//! - **磁盘值**＝本 crate 字段里的值（0）＝文件里真实存在的字节；
//! - **语义值**＝C# 在内存里给 `CellInfo` 赋的值（上表）＝渲染/逻辑要用的值。
//!
//! 上层要「语义值」时，请**按格式显式加常量**，不要直接把 `mid_idx` 当 `MiddleIndex` 用，
//! 否则 Type0/1/4/7 的中间层会全部落到库 0（真实图应在库 1）。
//!
//! # 三、`RawMap::header_raw` 的边界定义
//!
//! `header_raw` = **「格数组之前的全部字节」**，对 8 种定长格式就等于该格式的头部长度
//! （52 / 54 / 64 / 40 / 54 / 8 …）；对 `Mir3Wemade` 它等于 `28 + 半分辨率背景块长度`
//! —— 那一块不是逐格数据，详见 [`crate::formats::mir3_wemade`] 模块头。
//!
//! # 四、`RawMap::trailer`
//!
//! 「格数组之后还剩的字节」。真实样本里确实存在（`client/热血传奇23周年客户端/Map/EM001.map`
//! 等 67 个文件：头部声明 30×35 = 1050 格，`52 + 12*1050 = 12652`，而文件是 20692 字节）。
//! C# 只读 1050 格、把尾部 8040 字节**直接丢掉**（`ValidateDimensions` 只查「够不够」，
//! 不查「是否恰好」）；本 crate 要求字节级往返 ⇒ 必须原样保留这块，否则写回少 8040 字节。
//!
//! ⚠ **`trailer` 非空 ＝ 有字节没被解释 ⇒ 调用方必须告警。**
//! 这 8,040 字节 = **670 格（占 39%）**；若真实尺寸是 `40×43` 而不是头部的 `30×35`，
//! 列优先的 `h` 用错会让**整图错位**（不只是缺尾）。判据与方法见
//! [`RawMap::trailer_len`] / [`RawMap::has_unexplained_trailer`]。
//!
//! # 五、尺寸护栏：**三条件合取，读写两侧同一判据**（2026-11-06 补）
//!
//! 真值：C# `MapDimensions.cs:11-12@acfbbf8a` 的 `CanRoundTrip` ——
//! `w>0 && h>0` **且** `w<=32767 && h<=32767`（[`crate::MAX_DIMENSION`]）**且**
//! `w*h<=16_000_000`（[`crate::MAX_CELLS`]）。
//!
//! | 判据实现 | 可见性 | 调用点 |
//! |----------|--------|--------|
//! | `dims_verdict` | 模块内私有 | **唯一真源**，下面三个都走它 |
//! | [`can_round_trip`] | `pub` | 上层判断「这个尺寸能不能存」 |
//! | `validate_dims` | `pub(crate)` | 九个 `read` |
//! | `validate_write_dims` | `pub(crate)` | `write_fixed`（七种定长格式）+ `CrystalCs` / `Mir3Wemade` 两个自定义 `write` |
//!
//! ⚠ 「只看 16M 格」是**不完整**口径：`40000×1` 只有 40000 格，但头部宽高是 16 位有符号
//! 字段（C# `Document.cs:90` 的 `checked((short)Width)`），超 32767 写盘后读不回来。
//! ⚠ 读写**两侧**都要挂：只改读入口会留下「保存侧仍能写出 > 32767」的漏洞。

use crate::error::{FormatError, MAX_CELLS, MAX_DIMENSION};

// ───────────────────────────── 小端读取/写入工具 ─────────────────────────────

/// 读 `u16`（小端）。
///
/// 调用方**必须先校验长度**（本 crate 的所有解析路径都在进入格循环前做过
/// `len >= header + cell_len*count` 校验），因此这里的索引是证明过不会越界的。
#[inline]
pub(crate) fn u16_at(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([b[off], b[off + 1]])
}

/// 读 `i16`（小端）。
#[inline]
pub(crate) fn i16_at(b: &[u8], off: usize) -> i16 {
    i16::from_le_bytes([b[off], b[off + 1]])
}

/// 读 `u32`（小端）。
#[inline]
pub(crate) fn u32_at(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

/// 写 `u16`（小端）。
#[inline]
pub(crate) fn put_u16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// 写 `i16`（小端）。
#[inline]
pub(crate) fn put_i16(out: &mut Vec<u8>, v: i16) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// 写 `u32`（小端）。
#[inline]
pub(crate) fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

// ─────────────────────────────────── 格式标识 ───────────────────────────────────

/// 九种地图格式的标识。
///
/// 声明顺序**就是** C# `LegacyMapReader.cs:55-117` 的判别链顺序
/// （`CrystalCs` → `Mir3Wemade` → `Mir3Shanda` → `AntiHack` → `Wemade2010` →
/// `Shanda2012`/`Shanda14` → `Heroes34` → `Old12` 兜底）。
/// [`crate::registry`] 的切片顺序与之一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FormatId {
    /// 旧版 12 字节/格（`TMapInfo`）。C# `LoadMapType0`。
    Old12,
    /// Shanda/WeMade 14 字节/格（`TNewMapInfo`）。C# `LoadMapType2`。
    Shanda14,
    /// Shanda 2012 —— **36 字节/格**。C# `LoadMapType3`（`ValidateDimensions(52, 36)`，同样是 36）。
    Shanda2012,
    /// WeMade 2010，15 字节/格。C# `LoadMapType1`。
    Wemade2010,
    /// WeMade AntiHack（laby 图），12 字节/格。C# `LoadMapType4`。
    AntiHack,
    /// WeMade Mir3（无标题），14 字节/格 + 半分辨率背景块。C# `LoadMapType5`。
    Mir3Wemade,
    /// Shanda Mir3（标题 `(C) SNDA, MIR3`），20 字节/格。C# `LoadMapType6`。
    Mir3Shanda,
    /// 3/4 Heroes（Myth/Lifcos），15 字节/格。C# `LoadMapType7`。
    Heroes34,
    /// CrystalStudio 自定义（`C#` 魔数 + 版本号），变长格。C# `LoadMapType100`。
    CrystalCs,
}

impl FormatId {
    /// 该格式对应的 C# 读法函数名（用于报告/错误信息溯源）。
    pub const fn csharp_loader(self) -> &'static str {
        match self {
            FormatId::Old12 => "LoadMapType0",
            FormatId::Shanda14 => "LoadMapType2",
            FormatId::Shanda2012 => "LoadMapType3",
            FormatId::Wemade2010 => "LoadMapType1",
            FormatId::AntiHack => "LoadMapType4",
            FormatId::Mir3Wemade => "LoadMapType5",
            FormatId::Mir3Shanda => "LoadMapType6",
            FormatId::Heroes34 => "LoadMapType7",
            FormatId::CrystalCs => "LoadMapType100",
        }
    }
}

// ──────────────────────────────────── 单元格 ────────────────────────────────────

/// 一个地图格。
///
/// 全部字段存**磁盘原样值**（见模块文档第二节）；`tail` 装该格式里本 crate 未建模的
/// 字节，**顺序原样、长度固定**（`CrystalCs` 例外，见其模块头）。
///
/// # 位级丢弃的原始字节落在哪个字段
///
/// 规格书 §11.2 列了 7 处「同一字节内被掩、`tail` 兜不住」的位级丢弃，要求模型额外存原始字节。
/// 本 crate 的做法是**这些字段本身就存原值**（不存掩过的值），因此不需要再开 `*_raw` 字段：
///
/// | 规格书的 `*_raw` | 本 crate 字段 | 被掩的位 | 源码 |
/// |------------------|---------------|----------|------|
/// | `flag_raw` | [`Cell::flag`] | 整字节 8 位，只有 bit0/bit1 被 C# 用来派生碰撞 | `:415`、`:469` |
/// | `light_raw` | [`Cell::light`] | 高 4 位（C# 读时 `& 0x0F` 再 `* 4`） | `:433-434`、`:491-492` |
/// | `fr_anim_raw` | [`Cell::ani_frame`] | Type5 的 bit4-6（`& 0x8F`）、Type6 的 `> 0x0F` 高位 | `:418`、`:487` |
/// | `door_raw` | [`Cell::door`] | Type100 的 bit7（C# 读时 `& 0x7F`） | `:625` |
///
/// ⇒ 上层要「语义值」时调用对应模块里带 `csharp_` 前缀的函数（例如
/// [`formats::crystal_cs::csharp_door_index`](crate::formats::crystal_cs::csharp_door_index)），
/// 要「原值」时直接读字段。**两者都不会丢字节**。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cell {
    /// 背景层图号。
    ///
    /// - 12/14/36 字节格（`Old12`/`Shanda14`/`Shanda2012`，以及同样 12 B 的 `AntiHack`）：
    ///   `u16`（高 16 位恒 0），可能带 `0x8000` 阻挡位；
    /// - `Wemade2010`：`u32`，**磁盘上已 XOR `0xAA38AA38`**（`LegacyMapReader.cs:193`）；
    /// - `Heroes34`：`u32`，磁盘原样无 XOR（`:530`）；
    /// - `Mir3*`：`u16`，磁盘值 `+1` 才是真实图号（`:405`、`:476`）；
    /// - `CrystalCs`：`u32` 原样（`:580`）。
    pub bk_img: u32,
    /// 中间层图号。
    pub mid_img: u16,
    /// 前景层图号（高位 `0x8000` = 阻挡/不可飞越）。
    pub fr_img: u16,
    /// 背景层库号（原样）。
    ///
    /// 1 字节形态存 `0..=255`（`Shanda14`/`Shanda2012`/`Mir3*`），2 字节形态存 `i16`（`CrystalCs`）。
    /// `Mir3*` 的 `255` 是 `-1` 哨兵（`:404`/`:470`），这里保留原字节，语义由模块的 `csharp_` 函数给出。
    pub bk_idx: i16,
    /// 中间层库号（原样）。语义同 [`Cell::bk_idx`]。
    ///
    /// # ⚠ 四种格式里它是**常量 1**，不是从文件读的
    ///
    /// C# 在这四支里硬写 `MiddleIndex = 1`（**格内没有对应字节**）：
    ///
    /// | 格式 | C# 行号 | 写法 |
    /// |------|---------|------|
    /// | `Old12`（Type0） | `:134` | `MapCells[x, y].MiddleIndex = 1;` |
    /// | `Wemade2010`（Type1） | `:194` | `MiddleIndex = 1,`（对象初始化器） |
    /// | `AntiHack`（Type4） | `:347` | `MapCells[x, y].MiddleIndex = 1;` |
    /// | `Heroes34`（Type7） | `:531` | `MiddleIndex = 1,`（对象初始化器） |
    ///
    /// 本 crate 在这四种里把 `mid_idx` 留成 **0**（`Cell::default()`），**不影响往返** ——
    /// 因为这四支的 `write` **根本不写 `mid_idx`**（格内没有它的位置），`mid_idx` 只是
    /// 「语义值」而非「磁盘值」。上层要 C# 的语义值请按格式加 1（见下面「语义值 vs 磁盘值」）。
    ///
    /// 其余五种格式里 `mid_idx` 是**真实磁盘字段**（`Shanda14`/`Shanda2012` 用 `sm_tiles`
    /// 承载、`Mir3Wemade`/`Mir3Shanda` 是格内 1 字节、`CrystalCs` 是 `i16`），照常读写。
    pub mid_idx: i16,
    /// 前景层库号（原样）。仅 `CrystalCs` 有独立的 2 字节字段（`:592`、`:613`）。
    pub fr_idx: i16,
    /// `CrystalCs` v3+ 的「前景图个数」字节（`:588`）。其余格式恒 0 且不写出。
    pub fr_count: u8,
    /// 门索引（原样）。`CrystalCs` 语义上要 `& 0x7F`（`:625`），原样保留才能往返。
    pub door: u8,
    /// 门偏移（原样）。
    pub door_offset: u8,
    /// Mir3 系列的**格标志字节**（`flag`），只出现在 `Mir3Wemade`（格内偏移 0）与
    /// `Mir3Shanda`（格内偏移 0）。
    ///
    /// 语义（C# `LegacyMapReader.cs:436-437`、`:494-495`）：`bit0 == 1` ⇒ 背景层可走、
    /// `bit1 == 1` ⇒ 前景层可走；对应位不为 1 时 C# 给背景图号打上
    /// `0x20000000` / `0x40000000` 阻挡标记。其余格式恒 0 且**不写出**。
    pub flag: u8,
    /// 前景动画帧（原样）。
    pub ani_frame: u8,
    /// 前景动画节拍（原样）。
    ///
    /// # ⚠ `Mir3Shanda`（Type6）里它是**常量 1**，不是从文件读的
    ///
    /// C# `:490` 写 `byte franimationtick = 1;`（格内没有对应字节），`:502` 再把它赋给
    /// `FrontAnimationTick`。本 crate 留 **0**，**不影响往返** —— `mir3_shanda.rs` 的 `write`
    /// 不写 `ani_tick`（格内没有它的位置）。**语义值 1 ≠ 磁盘值 0**，见模块文档第四节。
    pub ani_tick: u8,
    /// 中间层动画帧（原样，仅 `Mir3*`/`CrystalCs`）。
    pub mid_ani_frame: u8,
    /// 中间层动画节拍（原样，仅 `CrystalCs` 真读）。
    ///
    /// # ⚠ `Mir3*` 两支里它是**常量**，不是从文件读的
    ///
    /// - `Mir3Shanda`（Type6）：C# `:489` 写 `MapCells[x, y].MiddleAnimationTick = 1;`
    ///   ⇒ **常量 1**；
    /// - `Mir3Wemade`（Type5）：C# `:420` 写 `MapCells[x, y].MiddleAnimationTick = 0;`
    ///   ⇒ **常量 0**（与本 crate 的默认值相同，等价）。
    ///
    /// 本 crate 在 `Mir3Shanda` 里留 **0**（而非 C# 语义的 1），**不影响往返** ——
    /// `mir3_shanda.rs` 的 `write` 不写 `mid_ani_tick`。见模块文档第四节。
    pub mid_ani_tick: u8,
    /// `TMapInfo.btArea`（原样）。
    ///
    /// 12/14/15/36 字节格里它同时是**前景库号**（C# 的 `frindex`）：
    /// `Old12` 是 `area + 2`（`:145`），`Shanda14`/`Shanda2012` 是 `area + 120`（`:245`），
    /// `Mir3*` 是 `area + 200 / + 300`（`:422`、`:474`）。
    pub area: u8,
    /// 光照（原样）。`Mir3*` 语义上要 `& 0x0F` 再 `* 4`（`:433-434`、`:491-492`）。
    pub light: u8,
    /// `TileAnimationImage`（原样，`CrystalCs` 是 `i16` 位模式）。
    ///
    /// 14/36 字节格里它就是 `TNewMapInfo.btTiles`（1 字节，值存 0..=255）——
    /// C# 把它读成「背景库号 + 100」（`:247`），`TMap.h` 把它命名为 `btTiles`。
    pub tiles: u16,
    /// `TileAnimationOffset`（原样）。14/36 字节格里是 `TNewMapInfo.btsmTiles`。
    pub sm_tiles: u16,
    /// `TileAnimationFrames`（原样）。
    pub tile_frames: u8,
    /// 该格式里未建模的字节，**原样顺序保存**。长度由格式决定（0 / 1 / 5 / 7 / 22 / 变长）。
    pub tail: Vec<u8>,
}

impl Cell {
    /// 造一个「除尾区外全 0」的格，尾区长度固定为 `tail_len`（全 0）。
    ///
    /// 供上层构造新图/测试用；避免每次手写 `tail: vec![0; n]`。
    pub fn zeroed(tail_len: usize) -> Cell {
        Cell {
            tail: vec![0u8; tail_len],
            ..Cell::default()
        }
    }
}

// ───────────────────────────────────── 地图 ─────────────────────────────────────

/// 一张地图的**原始字节级**表示。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawMap {
    /// 该地图的格式。
    pub format: FormatId,
    /// 宽（从头部按该格式的变换还原）。
    pub width: u16,
    /// 高。
    pub height: u16,
    /// 格数组之前的全部字节，写回时逐字节照抄（含宽高之外的保留区）。
    ///
    /// ⚠ 改尺寸必须**同时**改这里（宽高通常就存在这段里）与 [`RawMap::width`]/[`RawMap::height`]，
    /// 否则写出的文件会自相矛盾。
    pub header_raw: Vec<u8>,
    /// 全部格，**列优先**：`cells[x * height + y]` 对应 `(x, y)`。
    pub cells: Vec<Cell>,
    /// 格数组之后剩余的字节（原样保留）。
    ///
    /// 两个来源：
    /// 1. **`CrystalCs` 的尾部元数据块**（`Document.cs:77` → `PlacedStamps.cs:159-166`：
    ///    `Brotli(JSON) + Int64 长度 + SHA256(32) + magic "CRYSTAL-STAMPS01"(16)`；无放置物时
    ///    整块不写）。它是格式定义的一部分，**严格入口也会保留**；
    /// 2. **长度多一截的畸形文件**（`client/热血传奇23周年客户端/Map/EM001.map` 等 67 个：
    ///    头部声明 30×35 = 1050 格、`52 + 12*1050 = 12652`，实际 20692 字节）。
    ///    这类文件**严格入口会拒绝**（`FormatError::LengthMismatch`），只有显式走
    ///    `read_any_tolerant` 才会读进来、并把多出的字节放这里。
    ///
    /// # ⚠ `trailer` 非空 ⇒ 调用方**必须告警**
    ///
    /// 本 crate 只会按**头部声明的 `w·h`** 去解释格数组。`trailer` 非空意味着
    /// **「剩余 N 字节没有被解释」**，而这 N 字节有可能不是垃圾，而是**真实的格数据**
    /// —— 头部声明的尺寸与实际不符时，`h` 用错会让**整图列优先错位**（不只是缺尾）。
    ///
    /// 已知实例：`EM001.map` 等 67 个文件，`20,692 = 52 + 12 × 1720`（`1720 = 40×43`），
    /// 而头部写 30×35 = 1050 ⇒ `trailer.len() = 8,040`（= **670 格，占 39%**）。
    /// 「`1720 = 40×43`」与「头部 30×35」二者必有一错，本机无第三方证据可判
    /// （见审查报告 §四 P1-1）。
    ///
    /// ⇒ 上层拿到 [`RawMap::trailer_len`]` != 0` 时，**必须提示用户**：
    /// 「本工具按头部 `w·h` 解释，剩余 N 字节未解释」。**不要静默当成正常文件。**
    pub trailer: Vec<u8>,
}

impl RawMap {
    /// 按坐标取格；越界返回 `None`。
    pub fn cell(&self, x: u16, y: u16) -> Option<&Cell> {
        if x >= self.width || y >= self.height {
            return None;
        }
        self.cells
            .get(x as usize * self.height as usize + y as usize)
    }

    /// 按坐标取可变格；越界返回 `None`。
    pub fn cell_mut(&mut self, x: u16, y: u16) -> Option<&mut Cell> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let h = self.height as usize;
        self.cells.get_mut(x as usize * h + y as usize)
    }

    /// [`RawMap::trailer`] 的字节数 —— **「本 crate 没解释的剩余字节」**。
    ///
    /// 专供调用方做「必须告警」的判断（见 [`RawMap::trailer`] 的 ⚠ 段）。
    /// 等价于 `self.trailer.len()`，单独成方法是为了让「非 0 就必须告警」这条契约
    /// 在调用点可读、可 grep。
    ///
    /// ```
    /// # use mir2_format::read_any;
    /// # let mut raw = vec![0u8; 52 + 4 * 12];
    /// # raw[0..2].copy_from_slice(&2u16.to_le_bytes());
    /// # raw[2..4].copy_from_slice(&2u16.to_le_bytes());
    /// let map = read_any(&raw).unwrap();
    /// assert_eq!(map.trailer_len(), 0);
    /// ```
    pub fn trailer_len(&self) -> usize {
        self.trailer.len()
    }

    /// 有没有「本 crate 没解释的剩余字节」（`trailer_len() != 0`）。
    ///
    /// 为 `true` 时调用方**必须**提示用户「本工具按头部 `w·h` 解释，剩余 N 字节未解释」。
    pub fn has_unexplained_trailer(&self) -> bool {
        !self.trailer.is_empty()
    }
}

// ─────────────────────────────────── 能力声明 ───────────────────────────────────

/// 一种格式的能力声明（供上层决定能不能编辑/能不能保存/怎么解释碰撞位）。
///
/// 三个档位口径**必须分开陈述**（依据内部取证文档《规格-地图格式全量 9 种》§11.3；
/// 该文档已移出公开仓，不随本仓分发）：
///
/// - **A 档（9/9）**：全部字节都能被「已知字段 + [`Cell::tail`] + [`RawMap::trailer`]」覆盖
///   ⇒ **不改格的往返 9/9 都能逐字节相等**（本 crate 的硬门，见 `tests/synthetic.rs`）；
/// - **B 档（2/9）**：改格之后语义仍然自洽 ⇒ 只有 `Old12` 与 `Shanda14` 是
///   [`FormatCaps::editable`]` = true`，其余 7 种**可读可往返、但不承诺编辑语义**
///   （判据与逐格式依据见 [`FormatCaps::editable`] 的文档；不承诺的具体原因必须写在
///   [`FormatCaps::editable_caveat`]）；
/// - **C 档（1/9）**：C# 自身真有写入器的只有 Type100/v1。
///
/// # 为什么 B 档是 2/9 而不是 1/9（2026-11-05 P1-9 订正）
///
/// 旧结论「只有 `Shanda14` 可编辑」是**按"敢不敢承诺"**定的，不是按判据推的：
/// `Old12` 的 12 字节**全部有名字**（`formats/old12.rs:17-27` 的字段表）、`tail` 恒空、
/// `crate::formats::old12` 的 `read` 只给视图里有的字段赋值（`old12.rs:126-139`）、
/// 856 张真实样本往返 0 字节差异 ⇒ **它根本没有"未解区"**。
/// 把它标成 `false` 的直接后果是 `miredit-core` 给 1,051/1,158 张真实图（90.8%）挂常驻黄条，
/// 而文案「未解区语义未知」对 `Old12` 是**事实错误**，且把真正的「尾块 8,040 字节可能整图错位」
/// 警告淹没（内部取证文档《审查-今夜新增代码》P1-9，已移出公开仓）。
/// ⇒ 现按 [`FormatCaps::editable`] 文档里的两条判据逐格式重推。
///
/// # 为什么 `CrystalCs` 从「可编辑」降为「不可编辑」（2026-11-05 订正）
///
/// 依据内部取证文档《审查-mir2-format 九格式》§五「最高风险一句话」（已移出公开仓）。
/// `CrystalCs` 是**九种里唯一「布局理解错了、连字节级往返都照样通过」**的格式，三条叠加：
///
/// 1. **格长变长**（26/38/35/33+，由 `(ver, fr_count)` 决定）：任何字段顺序错位都会被
///    「读多几字节 / 读少几字节」吸收掉，格边界自己会漂到能自洽的位置；
/// 2. **`trailer` 把多余字节全吞掉**（`crystal_cs.rs` 的 `trailer = bytes[cur.off..]`）：
///    读错后剩下的字节照样原样写回；
/// 3. **`detect` 只查下界**（`crystal_cs.rs` 的 `detect` 只判 `len >= min_len`，
///    对齐 C# `:572` 的 `ValidateDimensions`）：尾块长度任意都算合法。
///
/// 而它**零真实样本**（本机 1158 个 `.map` 里 0 个带 `C#` 魔数）。
/// ⇒ 字段顺序一旦有误，会在「可编辑 + 往返全绿」的双重假象下**把真实数据写坏**。
/// ⇒ **待拿到真实 Type100 样本、并用 C# 双向对读验证字段顺序之后，才可改回 `true`。**
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatCaps {
    /// 是否支持写回。本 crate 九种**全部**为 `true`（A 档口径：不改格的字节级往返）。
    pub can_write: bool,
    /// 是否支持「改格后仍语义自洽」（B 档口径）。
    ///
    /// # 判据（2026-11-05 P1-9 重定：**逐格式推导**，不按「敢不敢承诺」）
    ///
    /// `editable = true` 必须**同时**满足两条，缺一即 `false`：
    ///
    /// 1. **格内没有被编辑破坏的未建模字节**：格内全部字节都能被「[`Cell`] 具名字段 +
    ///    [`Cell::tail`]」覆盖，**且** `tail` 里那些未建模字节的语义**不与可编辑字段联动**
    ///    （若联动，改了图号/光照之后它们不会跟着变 ⇒ 新旧数据不自洽）；
    /// 2. **布局有真实样本验证**（[`FormatCaps::has_real_samples`]` == true`）：
    ///    零样本 ⇒ 布局只有 C# 源码依据，改格后无从判断"自洽"。
    ///    这正是 `CrystalCs` 的失效模式 —— 布局理解错了，连字节级往返都照样通过
    ///    （见 `formats/crystal_cs.rs:81-100`）。
    ///
    /// # 逐格式结论（两条判据的实际取值）
    ///
    /// | 格式 | 判据 1（未建模区与编辑字段交互） | 判据 2（真实样本） | `editable` |
    /// |------|--------------------------------|--------------------|-----------|
    /// | `Old12` | 无未建模区（12 B 全具名、`tail` 恒空） | 856 张 | **true** |
    /// | `Shanda14` | 无未建模区（14 B 全具名、`tail` 恒空） | 有 | **true** |
    /// | `Shanda2012` | 22 B 尾区未建模，5 B 疑背图索引、12 B 疑光照/混合 | 195 张 | false |
    /// | `AntiHack` | 无未建模区（12 B 全具名、`tail` 恒空） | **0 张** | false |
    /// | `Wemade2010` | 1 B 尾区未建模（格内 rel 14） | 0 张 | false |
    /// | `Heroes34` | 1 B 尾区未建模 + `bk_img` 高 16 位改不到 | 0 张 | false |
    /// | `Mir3Wemade` | 4 B 尾区未建模 + `flag`（碰撞/可走）不在视图里 | 0 张 | false |
    /// | `Mir3Shanda` | 7 B 尾区零注释 + `flag` 不在视图里 | 0 张 | false |
    /// | `CrystalCs` | 变长格 + 12/1 B 未建模 + 高 16 位/高 8 位改不到 | 0 张 | false |
    ///
    /// ⚠ `false` **不等于不能写**：仍然能逐字节写回（A 档 9/9），只是不承诺「改格后语义自洽」。
    /// 具体原因写在 [`FormatCaps::editable_caveat`]，上层**必须**把它原样展示给用户
    /// （`miredit-core/src/document.rs` 的 `edit_warning` 就是这么做的）。
    ///
    /// ⚠ `true` 也不等于「文件级无损」：九种格式的头部都有未解释字节（48/27/48/… 字节），
    /// 它们原样保留、**从不参与编辑**，所以不影响这一位。
    pub editable: bool,
    /// **`editable == false` 的具体原因**（逐格式一条；空串 ⟺ `editable == true`）。
    ///
    /// # 契约
    ///
    /// `editable == false` ⟺ 本字段非空；`editable == true` ⇒ 本字段必须是空串。
    /// 由 `registry.rs` 的 `editable与caveat必须一致` 测试逐格式钉住 ——
    /// 否则会出现「标了不可承诺、却说不清为什么」的旗标（那种旗标等于没标）。
    ///
    /// # 文案要求
    ///
    /// **说清是哪一段、多少字节、编辑时怎么处理**，并**逐格式具体化**。
    /// 禁止写「未解区语义未知」这类一句话套九种的套话 —— 对 `Old12` 那种零未建模区的格式，
    /// 那句话是**事实错误**（P1-9）。零真实样本的格式写「布局未经真实文件验证」，
    /// 不许写成「语义未知」（前者是"没验证过"，后者是"知道有未知字节"，两件事）。
    pub editable_caveat: &'static str,
    /// **地表层能不能「逐格」落笔**（写 `Cell::bk_img` 后写回是否真的改变文件）。
    ///
    /// `false` 只出现在 `Mir3Wemade`（Type5）：它的背景图号来自**半分辨率 3 B / 2×2 块**
    /// （C# `:399-408`），块里的一个 3 字节组同时决定 4 个格的背景；
    /// `Cell::bk_img` 在该格式里恒 0 且**写回时不参与编码**
    /// （`mir3_wemade.rs` 的 `write` 不写 `bk_img`）。
    ///
    /// ⇒ 若上层在地表层按格写 `bk_img`，写回时会被**静默丢弃**（不报错、文件不变）。
    /// 上层必须先查这一位：`false` 时禁止开放「地表笔刷」，或改成块级编辑（块在
    /// [`RawMap::header_raw`] 里，可用 `mir3_wemade::decode_back_tiles` 只读解出）。
    pub ground_paint_supported: bool,
    /// 是否带**独立的库号字节**（`TNewMapInfo.btTiles/btsmTiles`、`Mir3` 的 back/middle 库号、
    /// `CrystalCs` 的 `i16` 库号）。`Old12` 没有 ⇒ `false`。
    pub has_lib_bytes: bool,
    /// 定长格式的单格字节数；变长格式（`CrystalCs`）见其模块文档。
    ///
    /// ⚠ `Wemade2010` 是**定长 15 B/格**（`fixed_cell_len = Some(15)`）：旧版曾把它当
    /// 「15/17 双档」，那是规格书 §二 的自造档，已删除（见 `formats/wemade2010.rs` 模块文档 2.2）。
    pub fixed_cell_len: Option<usize>,
    /// 格内是否带未建模尾区（[`Cell::tail`] 非空）。
    pub has_tail: bool,
    /// 本机是否**有真实样本**验证过（`false` ⇒ 只有按源码手工构造的合成样本验证）。
    pub has_real_samples: bool,
    /// 碰撞位说明（`0x8000` 之类的语义 + **极性**），直接引用权威注释。
    pub collision_note: &'static str,
    /// 碰撞极性：`bit15/flag 置位` 到底是「阻挡」还是「可走」。
    ///
    /// 九种格式**不是同一套极性**：`Old12`/`Shanda14`/`Shanda2012`/`Wemade2010`/`AntiHack`/
    /// `Heroes34` 是「图号 bit15 = 1 ⇒ 阻挡」；`Mir3Wemade`/`Mir3Shanda` 是
    /// 「`flag` 位 = 1 ⇒ **可走**」（C# `:436-437`、`:494-495` 的 `!= 1` 取反）；
    /// `CrystalCs` 直接存原生标志位（`0x20000000` = Low、`0x40000000` = High）。
    pub collision_polarity: CollisionPolarity,
    /// 权威源码位置（`文件:行号`），用于报告溯源。
    pub source: &'static str,
}

/// 碰撞极性（见 [`FormatCaps::collision_polarity`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollisionPolarity {
    /// 图号 `bit15 == 1` ⇒ **阻挡**（Low）；前景 `bit15 == 1` ⇒ High。
    Bit15SetMeansBlocked,
    /// `flag` 的位 `== 1` ⇒ **可走**；位 `== 0` ⇒ 置阻挡标记（与上一条**相反**）。
    FlagBitSetMeansWalkable,
    /// 标志位原样存在 `BackImage` 里，不做任何搬运。
    NativeFlags,
}

// ─────────────────────────────────── 格式契约 ───────────────────────────────────

/// 一种地图格式必须实现的能力。
///
/// 所有实现都是**无状态零尺寸类型**（`&'static dyn MapFormat` 常驻注册表），
/// 因此要求 `Send + Sync`。
pub trait MapFormat: Send + Sync {
    /// 格式标识。
    fn id(&self) -> FormatId;

    /// 人读名字（含格长，例如 `"Old12（12B/格，TMapInfo）"`）。
    fn name(&self) -> &'static str;

    /// 这个字节流是不是**本格式**。
    ///
    /// 判据 = 「C# 头部谓词」（[`MapFormat::header_signature`]）**且**「本格式的长度自洽」。
    ///
    /// 为什么比 C# 多一条长度自洽：C# 的判别只看标题字节，实测在真实样本上会误判
    /// （`client/热血传奇23周年客户端/Map/hell107.map` 等 4 个 12 B/格文件的标题恰好
    /// 命中 14 B/格 的签名，`GodotClient/maps/10.map` 等 17 个 14 B/格文件的标题首字节
    /// 是 `0x07` 而不是 `0x0F`）。C# 自己也在 `:91-104` 用「文件长度」区分 14/36 B 两支
    /// ⇒ 加长度自洽只是把 C# 已有的思路贯彻到底，不是新判据。
    fn detect(&self, bytes: &[u8]) -> bool;

    /// **纯 C# 头部谓词**（不看长度）。
    ///
    /// 与 [`MapFormat::detect`] 分开，是为了让上层能按 C# 原样复现判别链
    /// （[`crate::detect_format_csharp_order`]）同时又能用更稳的 [`crate::detect_format`]。
    fn header_signature(&self, bytes: &[u8]) -> bool;

    /// 从头部读出宽高（已按该格式的变换还原）。读不出返回 `None`。
    fn dims(&self, bytes: &[u8]) -> Option<(u16, u16)>;

    /// 解析成 [`RawMap`]。
    fn read(&self, bytes: &[u8]) -> Result<RawMap, FormatError>;

    /// 写回字节流；对 [`MapFormat::read`] 的成功输入必须逐字节相等。
    fn write(&self, map: &RawMap) -> Result<Vec<u8>, FormatError>;

    /// 能力声明。
    fn caps(&self) -> FormatCaps;

    /// 格数组之前的字节数（`Mir3Wemade` 是动态的，见其 [`MapFormat::read`]）。
    fn header_len(&self) -> usize;

    /// 定长格式的**精确**文件长度；变长格式返回 `None`。
    fn expected_len(&self, width: u16, height: u16) -> Option<u64>;
}

// ────────────────────────────── 尺寸判据（读写共用） ──────────────────────────────

/// 该宽高能不能**往返**（写盘后再读回，宽高与字节都不变）。
///
/// 与 C# `MapDimensions.CanRoundTrip` 逐字对应（`MapDimensions.cs:11-12@acfbbf8a`）：
///
/// ```csharp
/// public static bool CanRoundTrip(int width, int height) => width is > 0 and <= short.MaxValue &&
///     height is > 0 and <= short.MaxValue && (long)width * height <= MaximumCells;
/// ```
///
/// 三条件**合取**，短路顺序同官方：
/// ①`width > 0` 且 `height > 0`；②`width <= `[`MAX_DIMENSION`]` (= 32767)` 且 `height` 同理；
/// ③`width * height <= `[`MAX_CELLS`]` (= 16_000_000)`。
///
/// ⚠ 「只看 16M 格」是**不完整**的口径：`40000×1` 只有 40000 格，却因为单边超 32767
/// 而写盘后读不回来（头部宽高是 16 位有符号字段）。本次已把②补上。
///
/// 本函数与 [`crate::model`] 内部的 `validate_dims`（读侧）/ `validate_write_dims`（写侧）
/// **共用同一个判定核心**（私有 `dims_verdict`），因此「布尔判据」与「带原因的错误」
/// 不会各写一套后漂移 —— 由 `尺寸判据读写两侧同源` 用例穷举钉住。
///
/// ```
/// use mir2_format::{can_round_trip, MAX_DIMENSION};
///
/// assert_eq!(MAX_DIMENSION, 32767);
/// assert!(can_round_trip(32767, 1));
/// assert!(!can_round_trip(32768, 1));   // 单边超 32767 ⇒ 存下去读不回来
/// assert!(!can_round_trip(0, 10));
/// assert!(can_round_trip(4000, 4000));  // 恰好 16_000_000 格
/// assert!(!can_round_trip(4001, 4000)); // 16_004_000 格
/// ```
pub fn can_round_trip(width: u16, height: u16) -> bool {
    dims_verdict(i64::from(width), i64::from(height)).is_none()
}

/// 尺寸判定不合法的**类别**（内部形态，只用于在两个校验入口里挑错误变体）。
///
/// 刻意不公开：对外只有 [`can_round_trip`]（布尔）与 `FormatError` 变体（带原因），
/// 免得再出现第三套尺寸语义。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DimsReject {
    /// 宽或高 `<= 0`（官方第一条 `width is > 0`）。
    NonPositive,
    /// 单边 `> 32767`（官方两条 `<= short.MaxValue`）。
    TooLarge,
    /// 格数 `> 16_000_000`（官方第三条 `(long)width * height <= MaximumCells`）。
    TooMany(u64),
}

/// **尺寸判定的唯一真源**：返回「第一个不满足的条件」，`None` ⇒ 合法。
///
/// 顺序严格照 C# `CanRoundTrip`（`MapDimensions.cs:11-12@acfbbf8a`）的短路顺序：
/// 非正 → 单边 > 32767 → 格数 > 16M。
///
/// ②在③之前不只是「照抄官方」：`40000×40000` 两条都命中，报「单边超 32767」才是根因
/// （先降到 32767 以内，才谈得上讨论格数上限）。注意 `30000×30000` 只命中③
/// （30000 ≤ 32767），它报的是格数超上限。
///
/// 用 `i64` 收参数：读侧的宽高来自 `i32`（`i16` 读取或 XOR 还原，可能是负值），
/// 写侧来自 `u16`，两者都能无损装进 `i64`，且**任何窄化转换都发生在本函数判定通过之后**。
fn dims_verdict(width: i64, height: i64) -> Option<DimsReject> {
    if width <= 0 || height <= 0 {
        return Some(DimsReject::NonPositive);
    }
    if width > i64::from(MAX_DIMENSION) || height > i64::from(MAX_DIMENSION) {
        return Some(DimsReject::TooLarge);
    }
    // 到这里 width/height ∈ 1..=32767 ⇒ 乘积最大约 1.07e9，u64 不会溢出
    let cells = width as u64 * height as u64;
    if cells > MAX_CELLS {
        return Some(DimsReject::TooMany(cells));
    }
    None
}

/// 校验宽高（**读路径**；对齐 C# `LegacyMapReader.cs:47-54` 的 `ValidateDimensions`
/// 与 `MapDimensions.cs:11-12@acfbbf8a` 的 `CanRoundTrip`）。
///
/// 判定与 [`can_round_trip`] 同源（都走 `dims_verdict`）：
/// 非正 ⇒ [`FormatError::InvalidDimensions`]；单边 > 32767 ⇒
/// [`FormatError::DimensionTooLarge`]；格数 > 16M ⇒ [`FormatError::TooManyCells`]。
///
/// 返回值里的 `as u16` 是**已证明无损**的：`dims_verdict` 通过就意味着
/// `width`/`height` ∈ `1..=32767`。这就是为什么 `> 32767` 的检查必须卡在转换**之前**。
pub(crate) fn validate_dims(
    format: FormatId,
    width: i32,
    height: i32,
) -> Result<(u16, u16), FormatError> {
    match dims_verdict(i64::from(width), i64::from(height)) {
        // 已证明在 1..=32767，转换无损
        None => Ok((width as u16, height as u16)),
        Some(DimsReject::NonPositive) => Err(FormatError::InvalidDimensions {
            format,
            width,
            height,
        }),
        Some(DimsReject::TooLarge) => Err(FormatError::DimensionTooLarge {
            format,
            width: i64::from(width),
            height: i64::from(height),
            limit: MAX_DIMENSION,
        }),
        Some(DimsReject::TooMany(cells)) => Err(FormatError::TooManyCells {
            format,
            // 已证明在 1..=32767，转换无损
            width: width as u16,
            height: height as u16,
            cells,
            limit: MAX_CELLS,
        }),
    }
}

/// 校验宽高（**写路径**；[`MapFormat::write`] 的统一前置门）。
///
/// 与读路径 [`validate_dims`] **同一判据**（都走 `dims_verdict`）。只挂读侧会留下
/// 「保存侧仍能写出 > 32767 的宽高」的漏洞：`RawMap.width` 是 `u16`，上层完全可以
/// 构造出 65535，而九种格式的头部宽高都是 16 位**有符号**字段 ⇒ 存下去就回读不回来
/// （甚至被读成负数）。
///
/// 调用点（三处，覆盖九种格式）：
/// - [`write_fixed`] 顶部 —— 覆盖 `Old12` / `Shanda14` / `Shanda2012` / `Wemade2010` /
///   `AntiHack` / `Mir3Shanda` / `Heroes34` 七种；
/// - `formats::crystal_cs` 与 `formats::mir3_wemade` 的自定义 `write` 顶部 —— 覆盖剩下两种。
///
/// ⚠ 必须在任何 `as u16` / `as i16` 转换**之前**调用；违者就是静默截断。
pub(crate) fn validate_write_dims(
    format: FormatId,
    width: u16,
    height: u16,
) -> Result<(), FormatError> {
    match dims_verdict(i64::from(width), i64::from(height)) {
        None => Ok(()),
        Some(DimsReject::NonPositive) => Err(FormatError::InvalidDimensions {
            format,
            width: i32::from(width),
            height: i32::from(height),
        }),
        Some(DimsReject::TooLarge) => Err(FormatError::DimensionTooLarge {
            format,
            width: i64::from(width),
            height: i64::from(height),
            limit: MAX_DIMENSION,
        }),
        Some(DimsReject::TooMany(cells)) => Err(FormatError::TooManyCells {
            format,
            width,
            height,
            cells,
            limit: MAX_CELLS,
        }),
    }
}

// ────────────────────────────── 共用骨架：定长格格式 ──────────────────────────────

/// 定长格格式的读骨架：校验长度 → 抄头部 → 逐格解码（**列优先** `idx = x*h + y`）→ 收尾区。
///
/// `decode` 拿到的切片**恰好**是一格（长度 `cell_len`），因此格内偏移不会越界。
pub(crate) fn read_fixed<F>(
    format: FormatId,
    bytes: &[u8],
    header_len: usize,
    cell_len: usize,
    width: u16,
    height: u16,
    mut decode: F,
) -> Result<RawMap, FormatError>
where
    F: FnMut(&[u8]) -> Result<Cell, FormatError>,
{
    let count = width as u64 * height as u64;
    let need = header_len as u64 + count * cell_len as u64;
    if (bytes.len() as u64) < need {
        return Err(FormatError::Truncated {
            format,
            len: bytes.len(),
            need,
        });
    }
    // need 已 ≤ len，且 count ≤ MAX_CELLS ⇒ 下面的切片都在范围内。
    let need = need as usize;
    let mut map = RawMap {
        format,
        width,
        height,
        header_raw: bytes[..header_len].to_vec(),
        cells: Vec::with_capacity(count as usize),
        trailer: bytes[need..].to_vec(),
    };
    for i in 0..count as usize {
        let base = header_len + i * cell_len;
        map.cells.push(decode(&bytes[base..base + cell_len])?);
    }
    Ok(map)
}

/// 定长格格式的写骨架：**尺寸校验** → 校验自洽 → 抄头部 → 逐格编码 → 追加尾区。
///
/// # 尺寸校验放在最前面（本次补的缺口）
///
/// 本函数原先只查「格式 / 头长 / `cells.len()`」三样，**从不查尺寸** —— 于是一个
/// `RawMap { width: 32768, height: 1, .. }` 能一路写出去，落地成一张「C# 侧
/// `checked((short)Width)` 会抛溢出、本仓按 `i16` 读回会变成负数」的文件：
/// 写的时候不报错，读的时候读不回来。九种格式里七个走本函数
/// （`Old12` / `Shanda14` / `Shanda2012` / `Wemade2010` / `AntiHack` / `Mir3Shanda` /
/// `Heroes34`），所以门设在**这里**，而不是在七个 `write` 里各抄一遍。
///
/// ⚠ 新增的定长格式 writer 必须走本函数，否则会绕过尺寸门。
/// 变长格式（`CrystalCs` / `Mir3Wemade`）在自己的 `write` 顶部调
/// [`validate_write_dims`]，别漏。
pub(crate) fn write_fixed<F>(
    format: FormatId,
    map: &RawMap,
    header_len: usize,
    cell_len: usize,
    mut encode: F,
) -> Result<Vec<u8>, FormatError>
where
    F: FnMut(&Cell, &mut Vec<u8>) -> Result<(), FormatError>,
{
    // ① 尺寸门：必须在任何 `as u16` 之前（见 `validate_write_dims` 文档）。
    validate_write_dims(format, map.width, map.height)?;
    if map.format != format {
        return Err(FormatError::inconsistent(format!(
            "目标格式是 {format:?}，而 RawMap.format 是 {:?}",
            map.format
        )));
    }
    if map.header_raw.len() != header_len {
        return Err(FormatError::inconsistent(format!(
            "{format:?} 的 header_raw 必须是 {header_len} 字节，实际 {} 字节",
            map.header_raw.len()
        )));
    }
    let count = map.width as usize * map.height as usize;
    if map.cells.len() != count {
        return Err(FormatError::inconsistent(format!(
            "{format:?} 的 w*h = {}×{} = {count}，而 cells.len() = {}",
            map.width,
            map.height,
            map.cells.len()
        )));
    }
    let mut out = Vec::with_capacity(header_len + count * cell_len + map.trailer.len());
    out.extend_from_slice(&map.header_raw);
    for (i, cell) in map.cells.iter().enumerate() {
        if let Err(e) = encode(cell, &mut out) {
            return Err(FormatError::inconsistent(format!(
                "第 {i} 格（idx={i}）编码失败：{e}"
            )));
        }
    }
    out.extend_from_slice(&map.trailer);
    Ok(out)
}

/// 校验一格尾区长度（定长格式）。
/// 调用点在 [`write_fixed`] 的 `encode` 闭包里，外层会补上「第几格」的定位信息。
pub(crate) fn check_tail(cell: &Cell, want: usize, format: FormatId) -> Result<(), FormatError> {
    if cell.tail.len() != want {
        return Err(FormatError::inconsistent(format!(
            "{format:?} 的格尾区必须是 {want} 字节，实际 {} 字节",
            cell.tail.len()
        )));
    }
    Ok(())
}

/// 读 52 字节 `TMapHeader` 的宽高：`wWidth` `i16@0`、`wHeight` `i16@2`。
///
/// 结构体定义见 `client/reference_full/Classes/map/TMap.h`（`TMapHeader`：
/// `USHORT wWidth; USHORT wHeight; char sTitle[16]; double UpdateDate; byte Reserved[24];`
/// = 2 + 2 + 16 + 8 + 24 = **52 字节**），C# 侧用 `BitConverter.ToInt16` 读（有符号），
/// 因此 `>= 0x8000` 的宽高会变成负数并被 [`validate_dims`] 拒绝。
///
/// `Old12` / `Shanda14` / `Shanda2012` 三种格式共用这个头部。
pub(crate) fn tmap_dims(bytes: &[u8]) -> Option<(u16, u16)> {
    if bytes.len() < 4 {
        return None;
    }
    let w = i16_at(bytes, 0);
    let h = i16_at(bytes, 2);
    if w > 0 && h > 0 {
        Some((w as u16, h as u16))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{format_by_id, registry};

    // ─────────────────── 尺寸判据（C# `MapDimensions.CanRoundTrip`） ───────────────────

    /// 单边 32767 接受、32768 / 65535 拒绝；0 一律拒绝。
    #[test]
    fn 单边上限32767的边界() {
        assert_eq!(MAX_DIMENSION, 32767, "单边上限必须是 i16::MAX");
        assert_eq!(MAX_DIMENSION, i16::MAX as u16);

        assert!(can_round_trip(32767, 1));
        assert!(can_round_trip(1, 32767));
        assert!(!can_round_trip(32768, 1), "32768×1 必须拒绝（超 short.MaxValue）");
        assert!(!can_round_trip(1, 32768));
        assert!(!can_round_trip(65535, 1), "65535×1 塞得进 u16，塞不进 i16");
        assert!(!can_round_trip(65535, 65535));

        assert!(!can_round_trip(0, 0));
        assert!(!can_round_trip(0, 10));
        assert!(!can_round_trip(10, 0));
    }

    /// 三条件是**合取**：`40000×1` 格数远小于 16M，但单边超 32767 ⇒ 必须拒绝。
    #[test]
    fn 三条合取_格数合格不代表尺寸合格() {
        let 格数: u64 = u64::from(40000u16);
        assert!(格数 < MAX_CELLS, "40000 格远小于 16M；单看格数会放过它");
        assert!(!can_round_trip(40000, 1), "40000×1 单边超 32767，必须拒绝");
        assert!(matches!(
            validate_write_dims(FormatId::Old12, 40000, 1),
            Err(FormatError::DimensionTooLarge {
                width: 40000,
                height: 1,
                limit: 32767,
                ..
            })
        ));
        // 40000×40000：两条都超 ⇒ 按官方短路顺序报「尺寸」；30000×30000 只超格数 ⇒ 报「格数」
        assert!(matches!(
            validate_write_dims(FormatId::Old12, 40000, 40000),
            Err(FormatError::DimensionTooLarge { .. })
        ));
        assert!(matches!(
            validate_write_dims(FormatId::Old12, 30000, 30000),
            Err(FormatError::TooManyCells { .. })
        ));
    }

    /// 格数上限：`16_000_000` 恰好通过，且**对每个宽度**「再多一格」必被拒。
    ///
    /// ⚠ `16_000_001` **不可表示**为 `u16×u16`：最小非平凡因子 109，余因子 146789 > 65535
    /// （另两组 229×69869、641×24961 同样越界）。所以用逐宽度穷举证明临界点精确落在
    /// `16_000_000 / 16_000_001` 之间，而不是被写成 `>=` 或别的偏移。
    #[test]
    fn 格数上限边界逐宽度穷举() {
        assert_eq!(4000u64 * 4000, MAX_CELLS);
        assert!(can_round_trip(4000, 4000), "恰好 16_000_000 格应通过");
        assert!(validate_write_dims(FormatId::Old12, 4000, 4000).is_ok());

        // 4001×4000 = 16_004_000：只超 4000 格 ⇒ 报格数，不许报尺寸
        match validate_write_dims(FormatId::Old12, 4001, 4000) {
            Err(FormatError::TooManyCells {
                cells: 16_004_000,
                limit: 16_000_000,
                ..
            }) => {}
            other => panic!("4001×4000 应报 TooManyCells(16004000 > 16000000)，实际 {other:?}"),
        }

        // w 从 489 起 h0 才会 ≤ 32767（16_000_000 / 32767 ≈ 488.3）
        for w in 489u16..=MAX_DIMENSION {
            let h0 = (MAX_CELLS / u64::from(w)) as u16;
            assert!(
                (1..=MAX_DIMENSION).contains(&h0),
                "w={w} 时 h0={h0} 越界"
            );
            assert!(
                u64::from(w) * u64::from(h0) <= MAX_CELLS,
                "w={w} h0={h0} 应在上限内"
            );
            assert!(can_round_trip(w, h0), "w={w} h0={h0} 应接受");
            assert!(
                !can_round_trip(w, h0 + 1),
                "w={w} h={} 已超 16M，必须拒绝",
                h0 + 1
            );
            assert!(
                matches!(
                    validate_write_dims(FormatId::Old12, w, h0 + 1),
                    Err(FormatError::TooManyCells { .. })
                ),
                "w={w} h={} 应报 TooManyCells",
                h0 + 1
            );
        }
    }

    /// 读侧 `validate_dims` 与写侧 `validate_write_dims`、以及 [`can_round_trip`]
    /// **必须永远同源**（三者都走 `dims_verdict`）。
    #[test]
    fn 尺寸判据读写两侧同源() {
        let 样本: [(u16, u16); 14] = [
            (0, 0),
            (0, 5),
            (5, 0),
            (1, 1),
            (32767, 1),
            (1, 32767),
            (32768, 1),
            (1, 32768),
            (65535, 1),
            (65535, 65535),
            (4000, 4000),
            (4000, 4001),
            (30000, 30000),
            (1000, 600),
        ];
        for (w, h) in 样本 {
            let 读 = validate_dims(FormatId::Old12, i32::from(w), i32::from(h)).is_ok();
            let 写 = validate_write_dims(FormatId::Old12, w, h).is_ok();
            let 判 = can_round_trip(w, h);
            assert_eq!(读, 判, "{w}×{h}：读侧与布尔判据不一致");
            assert_eq!(写, 判, "{w}×{h}：写侧与布尔判据不一致");
        }
    }

    /// 读侧超限的变体与取值：`> 32767` 必须在 `as u16` **之前**被拒。
    ///
    /// 反证：若检查缺失，`validate_dims(_, 40000, 1)` 会返回 `Ok((40000, 1))` ——
    /// 宽高被当成合法读进来，写回时再溢出。`70000` 更能证明「不是截断成 4464」。
    #[test]
    fn 读侧超限报尺寸且不截断() {
        assert!(matches!(
            validate_dims(FormatId::Shanda14, 32768, 1),
            Err(FormatError::DimensionTooLarge {
                width: 32768,
                height: 1,
                limit: 32767,
                ..
            })
        ));
        // 70000 若是 `as u16` 会变成 4464（看起来「合法」），必须报错且原值进错误
        match validate_dims(FormatId::Shanda14, 70000, 1) {
            Err(FormatError::DimensionTooLarge { width, .. }) => {
                assert_eq!(width, 70000, "错误里必须是原值，不是截断后的 4464")
            }
            other => panic!("70000 应报 DimensionTooLarge，实际 {other:?}"),
        }
        // 错误信息必须点名 32767 与官方判据名
        let msg = validate_dims(FormatId::Shanda14, 70000, 1)
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        for 片段 in ["32767", "CanRoundTrip", "i16::MAX", "short.MaxValue"] {
            assert!(msg.contains(片段), "信息缺「{片段}」：{msg}");
        }
        // 合法值仍然正常返回
        assert_eq!(validate_dims(FormatId::Old12, 32767, 1), Ok((32767, 1)));
    }

    /// **九个 writer 全部**拒绝超限尺寸，且报的是「尺寸超 32767」而不是别的错。
    ///
    /// 构造的图故意**只满足格式/头长以外的字段**（`cells` 为空、头全 0）：
    /// 若尺寸门不在 `write` 最顶部，这里拿到的一定是 `Inconsistent`（头长/cells 对不上）
    /// 或 `UnsupportedVersion`（`CrystalCs` 版本号 0）⇒ 本用例同时钉住「门在最前面」。
    #[test]
    fn 九个writer都拒绝超限尺寸() {
        let 格式们 = registry();
        assert_eq!(格式们.len(), 9, "注册表应有九种格式");
        for f in 格式们 {
            let id = f.id();
            let 超限图 = RawMap {
                format: id,
                width: 32768,
                height: 1,
                header_raw: vec![0u8; f.header_len()],
                cells: Vec::new(),
                trailer: Vec::new(),
            };
            let err = f
                .write(&超限图)
                .err()
                .unwrap_or_else(|| panic!("{id:?} 的 write 必须拒绝 32768×1"));
            match err {
                FormatError::DimensionTooLarge {
                    format,
                    width,
                    height,
                    limit,
                } => {
                    assert_eq!(format, id, "{id:?} 的报错格式自相矛盾");
                    assert_eq!((width, height), (32768, 1), "{id:?} 报的宽高不对");
                    assert_eq!(limit, 32767, "{id:?} 的单边上限必须是 short.MaxValue");
                }
                other => panic!("{id:?} 应报 DimensionTooLarge，实际 {other:?}"),
            }

            // 65535（u16 上界）同样必须拒
            let mut 更超 = 超限图.clone();
            更超.width = 65535;
            assert!(
                matches!(
                    f.write(&更超),
                    Err(FormatError::DimensionTooLarge { width: 65535, .. })
                ),
                "{id:?} 的 write 必须拒绝 65535×1"
            );
        }
    }

    /// 写侧**不误伤**：`32767×1` 是合法尺寸，`Old12` 必须能写出来（约 384 KiB）。
    ///
    /// 与上一条配对，防止「为了拦超限把边界也拦掉」。
    #[test]
    fn 单边32767可写出且能读回() {
        let w = 32767u16;
        let h = 1u16;
        let mut header = vec![0u8; 52];
        header[0..2].copy_from_slice(&w.to_le_bytes());
        header[2..4].copy_from_slice(&h.to_le_bytes());
        let map = RawMap {
            format: FormatId::Old12,
            width: w,
            height: h,
            header_raw: header,
            cells: vec![Cell::zeroed(0); usize::from(w) * usize::from(h)],
            trailer: Vec::new(),
        };

        let out = format_by_id(FormatId::Old12)
            .write(&map)
            .expect("32767×1 是合法尺寸，必须写得出来");
        assert_eq!(out.len(), 52 + 32767 * 12);
        assert_eq!(out.len(), 393_256);

        let back = format_by_id(FormatId::Old12)
            .read(&out)
            .expect("写出的 32767×1 必须能读回");
        assert_eq!((back.width, back.height), (w, h));
        assert_eq!(back.cells.len(), 32767);
        assert_eq!(
            format_by_id(FormatId::Old12).write(&back).expect("重写"),
            out,
            "32767×1 的写→读→写必须逐字节相等"
        );
    }
}
