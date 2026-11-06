//! **双表示转换层**：`mir2_format::RawMap`（字节层真值） ⇄ `mir2_map::Map`（编辑/渲染视图）。
//!
//! 本模块只做一件事：把九种格式的 [`mir2_format::RawMap`] 投影成渲染层认识的
//! [`mir2_map::Map`]，再在保存前把视图里改过的字段**按位宽**写回 `raw`。
//! 它不解释语义、不猜字段、不做归一化 —— 一切以 `raw` 的字节为准。
//!
//! # 一、为什么要两套表示（而不是二选一）
//!
//! | 表示 | 谁在用 | 为什么不能换掉对方 |
//! |------|--------|--------------------|
//! | [`mir2_format::RawMap`] | 本 crate 的**保存路径** | 只有它知道九种格式的头部长度、格长、未解尾区；`mir2_map::Map` 只认 12/14/36 三档（`mir2-map/src/cell.rs:96-104`） |
//! | [`mir2_map::Map`] | **渲染层**（`miredit-render` 的入口签名就是 `&mir2_map::Map`）与全部编辑逻辑（`paint` / `fill` / `undo`） | 渲染层与 103 条既有测试都钉在它上面；换掉它等于重写整个编辑器 |
//!
//! ⇒ 本 crate 采用「`raw` 为真值、`view` 为投影」的双表示：**编辑只改 `view`**，
//! **保存只读 `raw`**，两者由本模块单向同步。
//!
//! # 二、字段映射表（`raw` → `view`，逐条对应）
//!
//! `raw` 的字段定义见 `mir2-format/src/model.rs:190-287`；`view` 的字段定义见
//! `mir2-map/src/cell.rs:176-202`。两边的**字段名不完全相同**（`door` vs `door_index`），
//! 下面这张表是唯一权威口径，代码里的 [`FIELD_MAP`] 与它逐条一致（有测试钉住）。
//!
//! | # | `view` 字段（`mir2_map::Cell`） | `raw` 字段（`mir2_format::Cell`） | 位宽处理 | 写回 |
//! |---|---|---|---|---|
//! | 1 | `bk_img: u16` | `bk_img: u32` | [`MapKind::Low16`]：视图只承载**低 16 位** | 只写低 16 位，高 16 位由 `raw` 原样保留 |
//! | 2 | `mid_img: u16` | `mid_img: u16` | [`MapKind::Exact`] | 整字段 |
//! | 3 | `fr_img: u16` | `fr_img: u16` | [`MapKind::Exact`] | 整字段 |
//! | 4 | `door_index: u8` | `door: u8` | [`MapKind::Exact`] | 整字段 |
//! | 5 | `door_offset: u8` | `door_offset: u8` | [`MapKind::Exact`] | 整字段 |
//! | 6 | `ani_frame: u8` | `ani_frame: u8` | [`MapKind::Exact`] | 整字段 |
//! | 7 | `ani_tick: u8` | `ani_tick: u8` | [`MapKind::Exact`] | 整字段 |
//! | 8 | `area: u8` | `area: u8` | [`MapKind::Exact`] | 整字段 |
//! | 9 | `light: u8` | `light: u8` | [`MapKind::Exact`] | 整字段 |
//! | 10 | `tiles: u8` | `tiles: u16` | [`MapKind::Low8`]：视图只承载**低 8 位** | 只写低 8 位，高 8 位由 `raw` 原样保留 |
//! | 11 | `sm_tiles: u8` | `sm_tiles: u16` | [`MapKind::Low8`] | 只写低 8 位 |
//! | 12 | `tail: [u8; 22]` | `tail: Vec<u8>` | [`MapKind::Tail22`]：**仅当 `raw.tail.len() == 22`** 才同步 | 条件同步 |
//!
//! ## 2.1 「只写低位」为什么不是偷懒，而是保真的**前提**
//!
//! 写回公式一律是「**保留 raw 的高位 + 覆盖视图能看见的低位**」：
//!
//! ```text
//! raw.bk_img  = (raw.bk_img  & 0xFFFF_0000) | (view.bk_img  as u32)
//! raw.tiles   = (raw.tiles   & 0xFF00)      | (view.tiles   as u16)
//! raw.sm_tiles= (raw.sm_tiles& 0xFF00)      | (view.sm_tiles as u16)
//! ```
//!
//! 这三条在「没改过」时是**恒等变换**（`(x & !m) | (x & m) == x`）⇒ 未改即存 = 逐字节相同
//! （验收门 2 的机制性保证）。若改成「整字段覆盖」，`Wemade2010`（`bk_img` 是磁盘上
//! XOR 过的 `u32`，`mir2-format/src/model.rs:195`）与 `CrystalCs`（`tiles`/`sm_tiles`
//! 都是 `u16`，`mir2-format/src/model.rs:280-282`）会在**第一次保存**就被削掉高半截。
//!
//! # 三、视图档位（[`view_cell_format`]）是**语义投影**，不是磁盘格长
//!
//! [`mir2_map::CellFormat`] 只有三档（12 / 14 / 36），九种格式对不上。视图档位**只**影响
//! 两件事：① `paint` 的 `fmt.has_lib_bytes()`（格内有没有库号字节，`miredit-core/src/paint.rs:266`）；
//! ② `format_name()` 的显示名。因此它按**「格内有没有库号字节」+「有没有 22 B 定长尾区」**
//! 投影，而不是按磁盘格长：
//!
//! | 格式 | 磁盘格长（`caps().fixed_cell_len`） | 视图档位 | 依据 |
//! |------|-----------------------------------|----------|------|
//! | `Old12` | 12 | `Old12` | 无库号字节（`old12.rs:169` `has_lib_bytes=false`） |
//! | `AntiHack` | 12 | `Old12` | 同上（`antihack.rs:201`） |
//! | `Wemade2010` | 15 | `Old12` | 15 B/格但**格内无库号字节**（`wemade2010.rs:350`） |
//! | `Heroes34` | 15 | `Old12` | 同上（`heroes34.rs:181`） |
//! | `Shanda14` | 14 | `New14` | 有库号字节（`shanda14.rs:204`） |
//! | `Shanda2012` | 36 | `Ext36` | 有库号字节 + 22 B 定长尾区（`shanda2012.rs:366-368`） |
//! | `Mir3Wemade` | 14 | `New14` | 有库号字节（`mir3_wemade.rs:316`） |
//! | `Mir3Shanda` | 20 | `New14` | 同上（`mir3_shanda.rs:206`） |
//! | `CrystalCs` | 变长 | `New14` | 同上（`crystal_cs.rs:457`） |
//!
//! ⚠ **磁盘格长请用 [`mir2_format::FormatCaps::fixed_cell_len`]，不要用 `view.format`。**
//!
//! # 四、`view.header_raw` 是**视图头**（永不写回）
//!
//! [`mir2_map::Map::header_raw`] 是定长 `[u8; 52]`，而九种格式的头部长度是
//! `52 / 54 / 64 / 40 / 28+块 / 8`（见各模块 `header_len()`）。因此视图头只能这样构造：
//!
//! 1. 前 4 字节**强制**为 `w` / `h` 小端 —— 满足 `mir2_map::Map` 自己的不变量
//!    （`mir2-map/src/map.rs:30-36`：`[0..2]` 是 `wWidth`、`[2..4]` 是 `wHeight`）；
//! 2. 其余字节从 `raw.header_raw` 的**前缀照抄**，不足 52 字节的部分填 0。
//!
//! ⚠ 对 `Old12` / `Shanda14` / `Shanda2012`（头部正好 52 字节）这一步是**恒等**的
//! （真实文件的前 4 字节本来就是 w/h）；对另外六种格式它是**视图专用**的合成，
//! **绝不写回**。要真头部请用 `Document::raw_header()`。
//!
//! # 五、视图里**没有**的字段（编辑不到，但写回时原样保留 ⇒ **不叫"丢失"**）
//!
//! `raw` 有 9 个字段在 [`mir2_map::Cell`] 里没有对应位置。它们**不丢**（`raw` 原样保留、
//! 原样写回、`sync_view_into_raw` 一个字节都不碰），只是编辑器改不到 ——
//! 见 [`LOST_IN_VIEW`] 与 [`unmodeled_fields`]。
//!
//! ⚠ **命名订正（2026-11-05，P1-15）**：本函数原名 `lost_fields`，名不副实 ——
//! 这 9 个字段在 `sync_view_into_raw` 里**一个字节都不被触碰**，往返 0 丢失。
//! 「编辑不到」≠「丢失」，故改名 [`unmodeled_fields`]（未建模 ≠ 丢失）。
//! 三档真实格式（`Old12` / `Shanda14` / `Shanda2012`）的 [`unmodeled_fields`] 是**空**，
//! 也就是说**有真实样本的格式 0 未建模字段**。
//!
//! # 六、库号字节「写不进去」的格式要**显式拒绝**，不许静默丢
//!
//! `paint` 只按 `CellFormat::has_lib_bytes()` 决定要不要写库号字节
//! （`miredit-core/src/paint.rs:296-299`），它不知道「这个格式的 writer 到底写不写那个字节」。
//! **两个方向**都会对不上（[`writes_layer_lib_byte`] 是权威表）：
//!
//! ## 6.1 `paint` 会写、writer 不写 ⇒ 落笔后还原
//!
//! | 格式 | 层 | 视图会写 | writer 实际写 | 后果 |
//! |------|----|----------|---------------|------|
//! | `Mir3Wemade` | 中层 | `sm_tiles` | 不写（`mir3_wemade.rs:250-306` 无 `sm_tiles`） | 静默丢 |
//! | `Mir3Shanda` | 中层 | `sm_tiles` | 不写（`mir3_shanda.rs:158-197` 无 `sm_tiles`） | 静默丢 |
//! | `CrystalCs` | 物体层 | `area` | 不写（`crystal_cs.rs:354-435` 无 `area`） | 静默丢 |
//!
//! ⇒ [`restore_unwritten_lib_byte`] 在落笔之后把这些字节**还原成改前值**并返回 `true`，
//! 由会话把「该层在本格式里没有独立的库号字节」说给用户听。**宁可拒绝，也不许让用户以为改了。**
//!
//! ## 6.2 writer 会写、`paint` 不写 ⇒ 落笔方必须**显式补写**
//!
//! | 格式 | 层 | `has_lib_bytes()` | writer 实际写 | 后果（若不补写） |
//! |------|----|-------------------|---------------|------------------|
//! | `Old12` / `AntiHack` | **物体层** | `false`（视图档位 `CellFormat::Old12`） | **写 `area`**（`old12.rs:158` 的 `out.push(cell.area)`） | 视图里"改了库号"、文件里没动 |
//!
//! 这一条**曾长期漏判**（2026-11-05 审查路 P1 订正）：`writes_layer_lib_byte` 旧实现对
//! `Old12`/`AntiHack` 三格全返回 `false`，连带
//! ① 删除/剪切选区时 `area` 残留不清（C# `CutSource` 会清，`RegionCopy.cs:61-64`）；
//! ② 印章放置时物体库号被当成"写不进去"而忽略；
//! ③ 提示文案成了假陈述（"writer 不写它"）。
//!
//! 现在 `Old12`/`AntiHack` 的物体层判 `true` ⇒ `clear_plan` 会清 `area`、
//! `stamp::写一族` 会**显式** `set_lib_byte` 一次（不依赖 `paint` 的 `has_lib_bytes` 闸门）。

use mir2_format::{format_by_id, Cell as RawCell, FormatId, RawMap};
use mir2_map::{Cell as ViewCell, CellFormat, Map, HEADER_LEN as VIEW_HEADER_LEN, TAIL_LEN as VIEW_TAIL_LEN};

use crate::layer::EditLayer;

// ─────────────────────────────── 字段映射表 ───────────────────────────────

/// 一条「视图字段 ↔ raw 字段」的映射口径（[`FIELD_MAP`] 的元素）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldMap {
    /// 视图侧字段名（`mir2_map::Cell` 的字段名）。
    pub view: &'static str,
    /// 字节层字段名（`mir2_format::Cell` 的字段名）。
    pub raw: &'static str,
    /// 位宽处理方式。
    pub kind: MapKind,
    /// 为什么这样处理（带源码位置）。
    pub note: &'static str,
}

/// 一个字段在两个表示之间的位宽关系。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapKind {
    /// 整字段一一对应（位宽相同）。
    Exact,
    /// 视图只承载 raw 的**低 16 位**；高 16 位留在 raw 里。
    Low16,
    /// 视图只承载 raw 的**低 8 位**；高 8 位留在 raw 里。
    Low8,
    /// 仅当 `raw.tail.len()` 正好等于 `mir2_map::TAIL_LEN`（22）时才同步。
    Tail22,
}

impl MapKind {
    /// 该映射是否**可能**丢失 raw 的信息（`Exact` / `Tail22` 为否）。
    ///
    /// `Tail22` 不算丢：`raw.tail` 长度不是 22 时**整段不同步**（原样保留），
    /// 同步时长度相等、逐字节照抄，两头都不会掉字节。
    pub const fn is_lossy(self) -> bool {
        matches!(self, MapKind::Low16 | MapKind::Low8)
    }
}

/// 12 条字段映射（**唯一权威表**，`view_from_raw` / `sync_view_into_raw` 都按它实现）。
///
/// 顺序与模块文档第二节的表格逐条一致；有测试（`字段映射表与实现同源`）钉住。
pub const FIELD_MAP: [FieldMap; 12] = [
    FieldMap {
        view: "bk_img",
        raw: "bk_img",
        kind: MapKind::Low16,
        note: "raw 是 u32：Wemade2010 磁盘上 XOR 0xAA38AA38（model.rs:195）、Heroes34 原样 u32（model.rs:196）、CrystalCs 原样 u32（model.rs:198）",
    },
    FieldMap {
        view: "mid_img",
        raw: "mid_img",
        kind: MapKind::Exact,
        note: "两边都是 u16（mir2-map/src/cell.rs:181 / model.rs:201）",
    },
    FieldMap {
        view: "fr_img",
        raw: "fr_img",
        kind: MapKind::Exact,
        note: "两边都是 u16（mir2-map/src/cell.rs:183 / model.rs:203）",
    },
    FieldMap {
        view: "door_index",
        raw: "door",
        kind: MapKind::Exact,
        note: "字段名不同、语义相同：`btDoorIndex`（model.rs:234）",
    },
    FieldMap {
        view: "door_offset",
        raw: "door_offset",
        kind: MapKind::Exact,
        note: "`btDoorOffset`（model.rs:236）",
    },
    FieldMap {
        view: "ani_frame",
        raw: "ani_frame",
        kind: MapKind::Exact,
        note: "`btAniFrame`（model.rs:245）",
    },
    FieldMap {
        view: "ani_tick",
        raw: "ani_tick",
        kind: MapKind::Exact,
        note: "`btAniTick`（model.rs:253）",
    },
    FieldMap {
        view: "area",
        raw: "area",
        kind: MapKind::Exact,
        note: "`btArea`，同时是前景库号（model.rs:273）",
    },
    FieldMap {
        view: "light",
        raw: "light",
        kind: MapKind::Exact,
        note: "`btLight`（model.rs:275）",
    },
    FieldMap {
        view: "tiles",
        raw: "tiles",
        kind: MapKind::Low8,
        note: "raw 是 u16：CrystalCs 真的写 u16（crystal_cs.rs:419），14/36 档只写 1 字节",
    },
    FieldMap {
        view: "sm_tiles",
        raw: "sm_tiles",
        kind: MapKind::Low8,
        note: "同 `tiles`（crystal_cs.rs:420）",
    },
    FieldMap {
        view: "tail",
        raw: "tail",
        kind: MapKind::Tail22,
        note: "视图尾区定长 22（mir2-map/src/cell.rs:68）；raw 尾区长度逐格式不同（0/1/3/4/7/22/变长）",
    },
];

/// `raw` 里有、视图里**没有对应字段**的 9 项（编辑不到，但写回时原样保留）。
///
/// 逐条来源：`mir2-format/src/model.rs:208-284`。
pub const LOST_IN_VIEW: [&str; 9] = [
    "bk_idx",
    "mid_idx",
    "fr_idx",
    "fr_count",
    "flag",
    "mid_ani_frame",
    "mid_ani_tick",
    "tile_frames",
    "tail（长度 ≠ 22 的整段）",
];

/// 该格式里**编辑视图没有承载的 raw 字段**（`raw` 有、`mir2_map::Cell` 没位置）。
///
/// 返回空切片 = 该格式的 `raw` 字段**全部**在视图里有对应位置。
///
/// # 名字为什么是 `unmodeled`（未建模）而不是 `lost`（丢失）
///
/// 2026-11-05（P1-15）改名。原名 `lost_fields` **名不副实**：
/// 这 9 个字段在 [`sync_view_into_raw`] 里**一个字节都不被触碰**（它只按 [`FIELD_MAP`]
/// 把视图字段写进 `raw`，其余字段原样留着），`raw` 又是保存的唯一真值来源
/// （`document.rs` 的 `try_to_bytes`）⇒ **往返 0 丢失**。
/// 「编辑不到」与「丢失」是两件事，本模块文档 §五 早已写明，函数名却一直反着说。
///
/// # 判据
///
/// 只有「该格式的 `read` 真的给这个字段赋过非默认值」才计入 —— 给恒为 0 的字段
/// 记一笔「未建模」是假问题（它本来就无处可放、也无内容可丢）。
///
/// # 与 `FormatCaps::editable` 的关系
///
/// 本函数**不是** `editable` 的判据：`editable` 判的是「改格后语义是否自洽」
/// （见 `mir2_format::FormatCaps::editable` 的两条判据），而本函数只回答
/// 「哪些字段编辑不到」。例如 `Old12` 与 `AntiHack` 都返回空，但 `AntiHack` 因零真实样本
/// 而 `editable == false`。
pub fn unmodeled_fields(id: FormatId) -> &'static [&'static str] {
    match id {
        // ── 三档真实格式 + AntiHack：`read` 只赋了视图里有的字段 ──
        // Old12  read: bk_img/mid_img/fr_img/door/door_offset/ani_frame/ani_tick/area/light（old12.rs:126-139）
        // Shanda14 再加 tiles/sm_tiles（shanda14.rs:145-156）
        // Shanda2012 再加 22 B 尾区（shanda2012.rs:304-318）
        // AntiHack 与 Old12 同（antihack.rs:152-161）
        FormatId::Old12 | FormatId::Shanda14 | FormatId::Shanda2012 | FormatId::AntiHack => &[],
        // Wemade2010：bk_img 是 u32（wemade2010.rs:231/255）；尾区**定长 1 字节**（`:240` `tail: vec![c[14]]`）
        // —— 旧「17 B 档 3 字节」是自造备档，已整条删除（见 `formats/wemade2010.rs` 模块文档 2.2）
        FormatId::Wemade2010 => &["bk_img 高 16 位", "tail（1 字节）"],
        // Heroes34：bk_img u32（heroes34.rs:137）；尾区 1 字节（:146）
        FormatId::Heroes34 => &["bk_img 高 16 位", "tail（1 字节）"],
        // Mir3Wemade：mid_idx / flag / mid_ani_frame / 尾区 4 字节（mir3_wemade.rs:230-245）
        FormatId::Mir3Wemade => &["mid_idx", "flag", "mid_ani_frame", "tail（4 字节）"],
        // Mir3Shanda：bk_idx / mid_idx / flag / mid_ani_frame / 尾区 7 字节（mir3_shanda.rs:142-152）
        FormatId::Mir3Shanda => &[
            "bk_idx",
            "mid_idx",
            "flag",
            "mid_ani_frame",
            "tail（7 字节）",
        ],
        // CrystalCs：变长格，9 项里除了 area 全有（crystal_cs.rs:284-335）
        FormatId::CrystalCs => &[
            "bk_idx",
            "mid_idx",
            "fr_idx",
            "fr_count",
            "mid_ani_frame",
            "mid_ani_tick",
            "tile_frames",
            "tail（变长）",
            "bk_img 高 16 位",
            "tiles 高 8 位",
            "sm_tiles 高 8 位",
        ],
    }
}

// ─────────────────────────────── 视图档位 ───────────────────────────────

/// 把九种格式投影成 `mir2_map::CellFormat` 的三档（判据见模块文档第三节）。
///
/// ⚠ 这是**编辑语义**的投影，**不是**磁盘格长。磁盘格长请用
/// `mir2_format::format_by_id(id).caps().fixed_cell_len`。
pub const fn view_cell_format(id: FormatId) -> CellFormat {
    match id {
        // 无库号字节 ⇒ Old12（paint 不写 btTiles/btsmTiles）
        FormatId::Old12 | FormatId::AntiHack | FormatId::Wemade2010 | FormatId::Heroes34 => {
            CellFormat::Old12
        }
        // 有库号字节 + 22 B 定长尾区 ⇒ Ext36（九种里只有它同时满足）
        FormatId::Shanda2012 => CellFormat::Ext36,
        // 其余：有库号字节、无 22 B 定长尾区 ⇒ New14
        FormatId::Shanda14 | FormatId::Mir3Wemade | FormatId::Mir3Shanda | FormatId::CrystalCs => {
            CellFormat::New14
        }
    }
}

// ─────────────────────────────── raw → view ───────────────────────────────

/// 把 [`RawMap`] 投影成 [`Map`]（**只读投影，不改 `raw`**）。
///
/// 逐格按 [`FIELD_MAP`] 搬运；视图装不下的字段（[`LOST_IN_VIEW`]）**不进视图**，
/// 但它们仍在 `raw` 里，写回时原样带上 —— 这就是「编辑不到 ≠ 丢失」。
///
/// # Panics
///
/// 不 panic。`raw.cells.len()` 与 `w*h` 对不上时按**较短的一侧**填视图
/// （`Map::cells` 是 `Vec`，长度自洽由 [`sync_view_into_raw`] 的校验兜住）。
pub fn view_from_raw(raw: &RawMap) -> Map {
    let want = raw.width as usize * raw.height as usize;
    let mut cells = Vec::with_capacity(raw.cells.len().min(want));
    for c in raw.cells.iter().take(want) {
        cells.push(view_cell(c));
    }
    Map {
        header_raw: view_header(raw),
        width: raw.width,
        height: raw.height,
        format: view_cell_format(raw.format),
        cells,
    }
}

/// 视图头（构造规则见模块文档第四节）：前 4 字节 = `w`/`h`，其余照抄 `raw.header_raw` 前缀。
fn view_header(raw: &RawMap) -> [u8; VIEW_HEADER_LEN] {
    let mut h = [0u8; VIEW_HEADER_LEN];
    let n = raw.header_raw.len().min(VIEW_HEADER_LEN);
    h[..n].copy_from_slice(&raw.header_raw[..n]);
    // 强制前 4 字节 = w/h：`mir2_map::Map` 的公开不变量（mir2-map/src/map.rs:30-36）。
    // 对 Old12/Shanda14/Shanda2012 这是恒等变换（真实文件本来就长这样）。
    h[0..2].copy_from_slice(&raw.width.to_le_bytes());
    h[2..4].copy_from_slice(&raw.height.to_le_bytes());
    h
}

/// 单格 `raw → view`（[`FIELD_MAP`] 的逐条实现）。
fn view_cell(c: &RawCell) -> ViewCell {
    let mut tail = [0u8; VIEW_TAIL_LEN];
    if c.tail.len() == VIEW_TAIL_LEN {
        tail.copy_from_slice(&c.tail);
    }
    ViewCell {
        // Low16：视图只装得下低 16 位（高 16 位留在 raw 里，见 MapKind::Low16）
        bk_img: c.bk_img as u16,
        mid_img: c.mid_img,
        fr_img: c.fr_img,
        door_index: c.door,
        door_offset: c.door_offset,
        ani_frame: c.ani_frame,
        ani_tick: c.ani_tick,
        area: c.area,
        light: c.light,
        // Low8：视图只装得下低 8 位
        tiles: c.tiles as u8,
        sm_tiles: c.sm_tiles as u8,
        // Tail22：长度不等时整段不同步（视图里留 0，且**永不写回**）
        tail,
    }
}

// ─────────────────────────────── view → raw ───────────────────────────────

/// [`sync_view_into_raw`] 的失败原因。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConvertError {
    /// 视图与 raw 的形状对不上（有人绕过 `Document` 改了 `view.width` / `view.cells`）。
    ///
    /// 这条护栏存在的理由：`view` 的宽高一旦与 `raw` 不一致，写回就会**错位**整张图
    /// （列优先展开的 `h` 用错 ⇒ 不只是缺尾，而是整图错行）。
    Shape {
        /// 视图宽。
        view_width: u16,
        /// 视图高。
        view_height: u16,
        /// 视图格数。
        view_cells: usize,
        /// raw 宽。
        raw_width: u16,
        /// raw 高。
        raw_height: u16,
        /// raw 格数。
        raw_cells: usize,
    },
}

impl std::fmt::Display for ConvertError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConvertError::Shape {
                view_width,
                view_height,
                view_cells,
                raw_width,
                raw_height,
                raw_cells,
            } => write!(
                f,
                "视图与字节层形状不一致：视图 {view_width}×{view_height}（{view_cells} 格）\
                 vs 字节层 {raw_width}×{raw_height}（{raw_cells} 格）"
            ),
        }
    }
}

impl std::error::Error for ConvertError {}

/// 把视图里改过的字段写回 `raw`；返回**真的变了**的格数。
///
/// 实现逐条对应 [`FIELD_MAP`]：
/// - `Exact` ⇒ 整字段覆盖；
/// - `Low16` / `Low8` ⇒ **保留 raw 高位 + 覆盖低位**（未改时是恒等变换，见模块文档 2.1）；
/// - `Tail22` ⇒ `raw.tail.len() == 22` 时逐字节照抄，否则**整段不动**。
///
/// # Errors
///
/// 视图与 raw 的宽高/格数对不上 ⇒ [`ConvertError::Shape`]（**绝不猜着写**）。
pub fn sync_view_into_raw(view: &Map, raw: &mut RawMap) -> Result<usize, ConvertError> {
    let want = raw.width as usize * raw.height as usize;
    if view.width != raw.width || view.height != raw.height || view.cells.len() != want {
        return Err(ConvertError::Shape {
            view_width: view.width,
            view_height: view.height,
            view_cells: view.cells.len(),
            raw_width: raw.width,
            raw_height: raw.height,
            raw_cells: raw.cells.len(),
        });
    }
    let mut changed = 0usize;
    for (i, dst) in raw.cells.iter_mut().enumerate() {
        let src = &view.cells[i];
        let before = dst.clone();
        // 1) bk_img：Low16 —— 高 16 位保留
        dst.bk_img = (dst.bk_img & 0xFFFF_0000) | u32::from(src.bk_img);
        // 2-9) Exact 字段：整字段覆盖
        dst.mid_img = src.mid_img;
        dst.fr_img = src.fr_img;
        dst.door = src.door_index;
        dst.door_offset = src.door_offset;
        dst.ani_frame = src.ani_frame;
        dst.ani_tick = src.ani_tick;
        dst.area = src.area;
        dst.light = src.light;
        // 10-11) tiles / sm_tiles：Low8 —— 高 8 位保留
        dst.tiles = (dst.tiles & 0xFF00) | u16::from(src.tiles);
        dst.sm_tiles = (dst.sm_tiles & 0xFF00) | u16::from(src.sm_tiles);
        // 12) tail：仅当 raw 的尾区正好 22 字节才同步（否则 raw 原样保留）
        if dst.tail.len() == VIEW_TAIL_LEN {
            dst.tail.copy_from_slice(&src.tail);
        }
        if *dst != before {
            changed += 1;
        }
    }
    Ok(changed)
}

// ─────────────────────────── 库号字节「写不进去」的格式 ───────────────────────────

/// 该格式的 `write` 是否真的把「该层的库号字节」落到盘上。
///
/// 三个库号字节在视图里的位置（`miredit-core/src/layer.rs:282-301`）：
/// `Ground → tiles`、`Middle → sm_tiles`、`Front → area`。阻挡层不写库号字节，恒 `false`。
///
/// 依据是**各格式 `write` 函数的实际写盘字段**（不是 `caps().has_lib_bytes`，
/// 也不是 `paint` 会不会去写它）：
///
/// | 格式 | `tiles` | `sm_tiles` | `area` | 源码 |
/// |------|---------|------------|--------|------|
/// | `Old12` | ✗ | ✗ | **✓** | `old12.rs:142-162`：12 B 布局里**没有** `btTiles`/`btsmTiles`，但 `:158` 有 `out.push(cell.area)` ⇒ `btArea` 是**写**的 |
/// | `AntiHack` | ✗ | ✗ | **✓** | 同 Old12（`antihack.rs` 也是 12 B 布局 + 写 `area`） |
/// | `Wemade2010` | ✗ | ✗ | ✗ | 15 B 格里没有库号字节 |
/// | `Heroes34` | ✗ | ✗ | ✗ | 同上 |
/// | `Shanda14` | ✓ | ✓ | ✓ | `shanda14.rs` 写 12/13 与 10 |
/// | `Shanda2012` | ✓ | ✓ | ✓ | `shanda2012.rs:323-359` |
/// | `Mir3Wemade` | ✗ | ✗ | ✓ | `mir3_wemade.rs:250-306`（只写 `area` 与 `mid_idx`） |
/// | `Mir3Shanda` | ✗ | ✗ | ✓ | `mir3_shanda.rs:158-197`（只写 `area`） |
/// | `CrystalCs` | ✓ | ✓ | ✗ | `crystal_cs.rs:354-435`（写 `tiles`/`sm_tiles`，**不写** `area`） |
///
/// # ⚠ 订正记录（2026-11-05，审查路 P1）
///
/// 旧实现对 `Old12`/`AntiHack`/`Wemade2010`/`Heroes34` 的**三格全部**返回 `false`，
/// 并在文档里写「`area` 虽然写，但 `paint` 在 `has_lib_bytes=false` 时根本不调 `set_lib_byte`」。
/// 那是**用调用方的行为去反推格式能力**，结论是错的：
///
/// - `old12.rs:158` 的 `out.push(cell.area)` 明写 writer **会**把 `btArea` 落盘；
/// - 于是删除/剪切选区时 `area` **残留不清**（C# `CutSource` 会连带清，`RegionCopy.cs:61-64`），
///   印章放置时物体库号也会被当成"写不进去"而忽略；
/// - 连带 `session.rs::lib_byte_drop_note` 与 `selection.rs` 的提示文案成了**假陈述**
///   （"writer 不写它"），已一并改成「该层在本格式里没有独立的库号字节」。
///
/// 修法：`Old12`/`AntiHack` 的**物体层**（`area`）判 `true`，地表/中层仍 `false`
/// （`tiles`/`sm_tiles` 确实不在 12 B 布局里）。`Wemade2010`/`Heroes34` 的 15 B 格
/// 连 `area` 都没有对应字节 ⇒ 三格仍全 `false`。
pub const fn writes_layer_lib_byte(id: FormatId, layer: EditLayer) -> bool {
    // 阻挡层永远不写库号字节（layer.rs:232-234）
    if layer.is_collision() {
        return false;
    }
    match (id, layer) {
        // 三档有库号字节的 14/36 档：三个字节都写
        (FormatId::Shanda14 | FormatId::Shanda2012, _) => true,
        // CrystalCs：只写 tiles / sm_tiles，不写 area
        (FormatId::CrystalCs, EditLayer::Ground | EditLayer::Middle) => true,
        (FormatId::CrystalCs, EditLayer::Front) => false,
        // Mir3 两支：只写 area
        (FormatId::Mir3Wemade | FormatId::Mir3Shanda, EditLayer::Front) => true,
        (FormatId::Mir3Wemade | FormatId::Mir3Shanda, _) => false,
        // Old12 / AntiHack：12 B 布局里**只有** area（old12.rs:158），tiles/sm_tiles 没有位置
        (FormatId::Old12 | FormatId::AntiHack, EditLayer::Front) => true,
        (FormatId::Old12 | FormatId::AntiHack, _) => false,
        // 15 B 布局（Wemade2010 / Heroes34）：三个库号字节都没有位置
        (FormatId::Wemade2010 | FormatId::Heroes34, _) => false,
        // 阻挡层已在函数开头返回 false；这里覆盖编译器要求的穷尽性
        (_, EditLayer::LowCollision | EditLayer::HighCollision) => false,
    }
}

/// 落笔后**把「writer 根本不写」的库号字节还原成改前值**；返回是否发生了还原。
///
/// 存在的理由见模块文档第六节：`paint` 只认 `CellFormat::has_lib_bytes()`，它不知道
/// 具体格式的 writer 写不写那个字节。不还原的话，用户会看到「库号改了、文件没变」。
///
/// ⚠ 只还原**该层那一个**字节，另外两个字节一律不碰（`layer.set_lib_byte` 也只写一个）。
pub fn restore_unwritten_lib_byte(
    id: FormatId,
    layer: EditLayer,
    before: &ViewCell,
    after: &mut ViewCell,
) -> bool {
    if layer.is_collision() || writes_layer_lib_byte(id, layer) {
        return false;
    }
    let (b, a) = (layer.lib_byte_of(before), layer.lib_byte_of(after));
    if a == b {
        return false;
    }
    layer.set_lib_byte(after, b);
    true
}

/// 该格式的「地表逐格落笔」是否有效（= `caps().ground_paint_supported`）。
///
/// 九种里只有 `Mir3Wemade`（Type5）为 `false`：它的背景图号在**半分辨率块**里
/// （`mir2-format/src/model.rs:430-440`），`Cell::bk_img` 恒 0 且 `write` 不写它
/// （`mir3_wemade.rs:250-306`）⇒ 逐格写 `bk_img` 必然被静默丢弃。
pub fn ground_paint_supported(id: FormatId) -> bool {
    format_by_id(id).caps().ground_paint_supported
}

#[cfg(test)]
mod tests {
    use super::*;
    use mir2_format::formats::mir3_wemade::Mir3Wemade;
    use mir2_format::{registry, read_any};

    /// 造一张最小的 `Old12` 图字节（2×2，头部 52 字节 + 4 格 × 12 字节）。
    fn old12_bytes() -> Vec<u8> {
        let mut b = vec![0u8; 52 + 4 * 12];
        b[0..2].copy_from_slice(&2u16.to_le_bytes());
        b[2..4].copy_from_slice(&2u16.to_le_bytes());
        b
    }

    /// **字段映射表与实现同源**：12 条映射的字段名必须都能在两边结构体上找到对应，
    /// 且 `view` 侧字段名与 `mir2_map::Cell` 的实际字段一一对上（防有人改表不改代码）。
    #[test]
    fn 字段映射表与实现同源() {
        assert_eq!(FIELD_MAP.len(), 12, "映射表必须有 12 条");
        let views: Vec<&str> = FIELD_MAP.iter().map(|m| m.view).collect();
        assert_eq!(
            views,
            vec![
                "bk_img", "mid_img", "fr_img", "door_index", "door_offset", "ani_frame", "ani_tick",
                "area", "light", "tiles", "sm_tiles", "tail"
            ],
            "视图侧字段名必须与 mir2_map::Cell 的字段名逐条相同"
        );
        // 逐条走一遍真实结构体，证明字段名不是写错也不会编译失败（类型系统兜底）
        let v = ViewCell {
            bk_img: 1,
            mid_img: 2,
            fr_img: 3,
            door_index: 4,
            door_offset: 5,
            ani_frame: 6,
            ani_tick: 7,
            area: 8,
            light: 9,
            tiles: 10,
            sm_tiles: 11,
            tail: [12u8; VIEW_TAIL_LEN],
        };
        // `zeroed(22)` 让 raw 的尾区长度正好等于视图尾区长度（Tail22 的同步前提）
        let mut r = RawCell::zeroed(VIEW_TAIL_LEN);
        r.bk_img = 1;
        r.mid_img = 2;
        r.fr_img = 3;
        r.door = 4;
        r.door_offset = 5;
        r.ani_frame = 6;
        r.ani_tick = 7;
        r.area = 8;
        r.light = 9;
        r.tiles = 10;
        r.sm_tiles = 11;
        r.tail = vec![12u8; VIEW_TAIL_LEN];
        assert_eq!(view_cell(&r), v, "12 条映射必须逐字段对得上");
        // 位宽处理方式：只有 bk_img / tiles / sm_tiles 是有损的
        let lossy: Vec<&str> = FIELD_MAP
            .iter()
            .filter(|m| m.kind.is_lossy())
            .map(|m| m.view)
            .collect();
        assert_eq!(lossy, vec!["bk_img", "tiles", "sm_tiles"]);
        assert_eq!(
            FIELD_MAP.iter().filter(|m| m.kind == MapKind::Tail22).count(),
            1
        );
    }

    /// 四种格式（三档真实格式 + `AntiHack`）**零未建模字段** —— 这是本单最强的一条结论，必须钉住。
    ///
    /// ⚠ 名字从「零信息丢失」改成「零未建模字段」（2026-11-05 P1-15）：
    /// 这 4 种确实**一个字段都不缺**（`raw` 的字段全在视图里有位置），但"未建模"才是准确说法 ——
    /// 另外 5 种也**不是"丢失"**（它们同样原样写回），只是编辑视图里没有对应位置。
    #[test]
    fn 四种格式零未建模字段() {
        for id in [
            FormatId::Old12,
            FormatId::Shanda14,
            FormatId::Shanda2012,
            FormatId::AntiHack,
        ] {
            assert!(
                unmodeled_fields(id).is_empty(),
                "{id:?} 的视图装得下它的全部字段，不该报未建模"
            );
        }
        // 反向：另外五种必须**报得出**未建模字段（否则表就是空的、等于没审）
        for id in [
            FormatId::Wemade2010,
            FormatId::Heroes34,
            FormatId::Mir3Wemade,
            FormatId::Mir3Shanda,
            FormatId::CrystalCs,
        ] {
            assert!(
                !unmodeled_fields(id).is_empty(),
                "{id:?} 应当报出未建模字段"
            );
        }
        assert_eq!(LOST_IN_VIEW.len(), 9, "raw 里视图装不下的字段共 9 项");
        // 逐格式钉住返回值（改名后"含义清楚"的凭据：空 = 全建模，非空 = 逐项点名）
        assert_eq!(unmodeled_fields(FormatId::Old12), &[] as &[&str]);
        // Wemade2010 的尾区定长 1 字节：`wemade2010.rs:128` `CELL_LEN = 15`、`:131` `TAIL_LEN = 1`。
        // 旧断言写「1 或 3 字节」，其中「3 字节」来自已删除的自造 17 B 档（见 `formats/wemade2010.rs`
        // 模块文档 2.2：C# `LegacyMapReader.cs` 里 `17` 零命中，唯一格长常量是 `:183` 的 `ValidateDimensions(54, 15)`）
        // ⇒ 断言值跟着收窄成「1 字节」，不改任何被测逻辑。
        assert_eq!(
            unmodeled_fields(FormatId::Wemade2010),
            ["bk_img 高 16 位", "tail（1 字节）"]
        );
        assert_eq!(
            unmodeled_fields(FormatId::Mir3Shanda),
            ["bk_idx", "mid_idx", "flag", "mid_ani_frame", "tail（7 字节）"]
        );
    }

    /// **视图档位 = 「格内有没有库号字节」的投影**（不是磁盘格长）。
    #[test]
    fn 视图档位按库号字节投影() {
        for f in registry() {
            let caps = f.caps();
            let v = view_cell_format(f.id());
            assert_eq!(
                v.has_lib_bytes(),
                caps.has_lib_bytes,
                "{:?} 的视图档位 has_lib_bytes 必须与 caps 一致（否则 paint 会写不存在的字节）",
                f.id()
            );
            if caps.has_tail && caps.fixed_cell_len.is_some() {
                // 只有 Shanda2012 的尾区正好 22 字节（= mir2_map::TAIL_LEN）
                assert_eq!(
                    v.has_tail(),
                    f.id() == FormatId::Shanda2012,
                    "{:?} 的视图档位 has_tail 只该在尾区正好 22 字节时为真",
                    f.id()
                );
            }
        }
        assert_eq!(view_cell_format(FormatId::Shanda2012), CellFormat::Ext36);
        assert_eq!(view_cell_format(FormatId::Old12), CellFormat::Old12);
    }

    /// `raw → view → raw` 在**未改**时是恒等变换（验收门 2 的机制性保证）。
    #[test]
    fn 未改时同步是恒等变换() {
        let raw = read_any(&old12_bytes()).expect("2×2 Old12 应解析成功");
        let view = view_from_raw(&raw);
        let mut back = raw.clone();
        let changed = sync_view_into_raw(&view, &mut back).expect("形状一致应成功");
        assert_eq!(changed, 0, "没改过就不该有任何格变化");
        assert_eq!(back, raw, "未改时同步必须是恒等变换");
        // 字节也必须是恒等
        let f = format_by_id(raw.format);
        assert_eq!(f.write(&back).unwrap(), old12_bytes());
    }

    /// **高位保留**：`raw` 的 `bk_img` 高 16 位 / `tiles` 高 8 位在同步后必须原样。
    #[test]
    fn 高位字段不被视图削掉() {
        let mut raw = read_any(&old12_bytes()).unwrap();
        // 人为把高半截填成非零（模拟 Wemade2010 / CrystalCs 的 u32 / u16 形态）
        raw.cells[0].bk_img = 0xAA38_1234;
        raw.cells[0].tiles = 0xAB_07;
        raw.cells[0].sm_tiles = 0xCD_08;
        let view = view_from_raw(&raw);
        assert_eq!(view.cells[0].bk_img, 0x1234, "视图只看得到低 16 位");
        assert_eq!(view.cells[0].tiles, 0x07, "视图只看得到低 8 位");
        assert_eq!(view.cells[0].sm_tiles, 0x08);
        let mut back = raw.clone();
        assert_eq!(sync_view_into_raw(&view, &mut back).unwrap(), 0);
        assert_eq!(back.cells[0].bk_img, 0xAA38_1234, "高 16 位必须原样");
        assert_eq!(back.cells[0].tiles, 0xAB_07, "高 8 位必须原样");
        assert_eq!(back.cells[0].sm_tiles, 0xCD_08);
    }

    /// **尾区只在长度正好 22 时同步**：长度不等时 raw 的尾区一个字节都不许动。
    #[test]
    fn 尾区长度不等时不同步() {
        // Heroes34 的尾区是 1 字节（heroes34.rs:146）⇒ 视图的 [u8;22] 装不下
        let mut raw = read_any(&old12_bytes()).unwrap();
        raw.cells[0].tail = vec![0x5A];
        let view = view_from_raw(&raw);
        assert_eq!(view.cells[0].tail, [0u8; VIEW_TAIL_LEN], "长度不等 ⇒ 视图里留 0");
        let mut back = raw.clone();
        sync_view_into_raw(&view, &mut back).unwrap();
        assert_eq!(back.cells[0].tail, vec![0x5A], "长度不等时 raw 尾区必须原样");
        // 长度正好 22 ⇒ 逐字节同步
        raw.cells[0].tail = (0..VIEW_TAIL_LEN as u8).collect();
        let view = view_from_raw(&raw);
        let mut back = raw.clone();
        back.cells[0].tail = vec![0u8; VIEW_TAIL_LEN];
        sync_view_into_raw(&view, &mut back).unwrap();
        assert_eq!(back.cells[0].tail, raw.cells[0].tail, "长度相等时逐字节同步");
    }

    /// 形状对不上必须**报错**，绝不猜着写。
    #[test]
    fn 形状不符时报错不写() {
        let raw = read_any(&old12_bytes()).unwrap();
        let mut view = view_from_raw(&raw);
        view.width = 3; // 人为破坏
        let mut back = raw.clone();
        match sync_view_into_raw(&view, &mut back) {
            Err(ConvertError::Shape {
                view_width,
                raw_width,
                ..
            }) => {
                assert_eq!(view_width, 3);
                assert_eq!(raw_width, 2);
            }
            other => panic!("应报 Shape，实际 {other:?}"),
        }
        assert_eq!(back, raw, "报错时一个字段都不许写");
    }

    /// 视图头：前 4 字节 = w/h，其余照抄 raw 前缀，不足零填；且**从不回写 raw**。
    #[test]
    fn 视图头前四字节是宽高() {
        let mut b = old12_bytes();
        // 头部里放点非零垃圾，证明「其余字节照抄」
        b[4] = 0x11;
        b[51] = 0x22;
        let raw = read_any(&b).unwrap();
        let view = view_from_raw(&raw);
        assert_eq!(&view.header_raw[0..2], &2u16.to_le_bytes());
        assert_eq!(&view.header_raw[2..4], &2u16.to_le_bytes());
        assert_eq!(view.header_raw[4], 0x11, "第 5 字节应照抄 raw");
        assert_eq!(view.header_raw[51], 0x22, "第 52 字节应照抄 raw");
        // 写回时用的是 raw.header_raw，视图头不参与
        let mut back = raw.clone();
        sync_view_into_raw(&view, &mut back).unwrap();
        assert_eq!(back.header_raw, raw.header_raw);
    }

    /// **库号字节表**：三种「writer 不写那个字节」的格式必须被标出来。
    #[test]
    fn 库号字节写不进去的三种格式() {
        // 应当写进去的
        for (id, layer) in [
            (FormatId::Shanda14, EditLayer::Ground),
            (FormatId::Shanda14, EditLayer::Middle),
            (FormatId::Shanda14, EditLayer::Front),
            (FormatId::Shanda2012, EditLayer::Ground),
            (FormatId::Shanda2012, EditLayer::Middle),
            (FormatId::Shanda2012, EditLayer::Front),
            (FormatId::Mir3Wemade, EditLayer::Front),
            (FormatId::Mir3Shanda, EditLayer::Front),
            (FormatId::CrystalCs, EditLayer::Ground),
            (FormatId::CrystalCs, EditLayer::Middle),
        ] {
            assert!(
                writes_layer_lib_byte(id, layer),
                "{id:?}/{layer:?} 的 writer 真的写这个字节"
            );
        }
        // 写不进去的（静默丢的三处 + 无库号字节的四种）
        for (id, layer) in [
            (FormatId::Mir3Wemade, EditLayer::Middle),
            (FormatId::Mir3Shanda, EditLayer::Middle),
            (FormatId::CrystalCs, EditLayer::Front),
            (FormatId::Old12, EditLayer::Ground),
            (FormatId::AntiHack, EditLayer::Ground),
            (FormatId::Wemade2010, EditLayer::Ground),
            (FormatId::Heroes34, EditLayer::Ground),
        ] {
            assert!(
                !writes_layer_lib_byte(id, layer),
                "{id:?}/{layer:?} 的 writer 不写这个字节，不许当成写得进去"
            );
        }
        // 阻挡层永远不写
        for f in registry() {
            for l in [EditLayer::LowCollision, EditLayer::HighCollision] {
                assert!(!writes_layer_lib_byte(f.id(), l));
            }
        }
    }

    /// 还原函数：只还原**该层那一个**字节，另外两个不碰；写得进去的格式不还原。
    #[test]
    fn 还原写不进去的库号字节() {
        let before = ViewCell {
            tiles: 1,
            sm_tiles: 2,
            area: 3,
            ..ViewCell::default()
        };
        // Mir3Shanda 的中层：sm_tiles 写不进去 ⇒ 必须还原成 2，且 tiles/area 不动
        let mut after = ViewCell {
            tiles: 9,
            sm_tiles: 8,
            area: 7,
            ..ViewCell::default()
        };
        assert!(restore_unwritten_lib_byte(
            FormatId::Mir3Shanda,
            EditLayer::Middle,
            &before,
            &mut after
        ));
        assert_eq!(after.sm_tiles, 2, "写不进去的字节必须还原");
        assert_eq!(after.tiles, 9, "别的层的库号字节不许被牵连");
        assert_eq!(after.area, 7);
        // 写得进去的格式：不还原
        let mut ok = ViewCell {
            sm_tiles: 8,
            ..ViewCell::default()
        };
        assert!(!restore_unwritten_lib_byte(
            FormatId::Shanda14,
            EditLayer::Middle,
            &before,
            &mut ok
        ));
        assert_eq!(ok.sm_tiles, 8);
        // 值没变 ⇒ 不算还原
        let mut same = before;
        assert!(!restore_unwritten_lib_byte(
            FormatId::Mir3Shanda,
            EditLayer::Middle,
            &before,
            &mut same
        ));
    }

    /// Type5（`Mir3Wemade`）的地表逐格落笔能力必须是 `false`（九种里唯一一个）。
    #[test]
    fn type5地表落笔无效() {
        assert!(!ground_paint_supported(FormatId::Mir3Wemade));
        for f in registry() {
            if f.id() != FormatId::Mir3Wemade {
                assert!(ground_paint_supported(f.id()), "{:?} 应当支持地表落笔", f.id());
            }
        }
        // 交叉校验：Type5 的 `read` 里 `bk_img` 是写死的 0（mir3_wemade.rs:233）
        let raw = mir2_format::format_by_id(FormatId::Mir3Wemade)
            .read(&{
                // 按 mir3_wemade 的测试手法造一张 4×2 的合成图（28 + 3*2*1 + 14*8）
                let n = Mir3Wemade::cells_offset(4, 2) + 14 * 4 * 2;
                let mut b = vec![0u8; n];
                b[22..24].copy_from_slice(&4i16.to_le_bytes());
                b[24..26].copy_from_slice(&2i16.to_le_bytes());
                b
            })
            .expect("合成 Type5 应解析成功");
        assert!(
            raw.cells.iter().all(|c| c.bk_img == 0),
            "Type5 的 bk_img 恒 0（真值在块里）"
        );
        let view = view_from_raw(&raw);
        assert_eq!(view.format, CellFormat::New14, "Type5 的视图档位按库号字节投影");
    }
}
