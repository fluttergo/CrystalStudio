//! 五层计数（地表 / 中层 / 物体 / 低位阻挡 / 高位阻挡）——官方 `MapDocument.layerCounts` 的移植。
//!
//! 抽取自 CrystalStudio `Document.LayerCounts.cs:5-25@acfbbf8a`（`layerCounts` / `LayerCount` /
//! `UpdateLayerCounts`）与 `Document.cs:26@acfbbf8a`（`HasForeground`）。
//!
//! # 一、官方口径（照抄，不"优化"）
//!
//! ```text
//! 地表  = (x & 1) == 0 && (y & 1) == 0 && (BackImage & 0x1FFFFFFF) != 0
//! 中层  = MiddleImage != 0
//! 物体  = HasForeground = Foreground is { } && (Foreground.Image & 0x7FFF) != 0
//! 低位阻挡 = (BackImage & 0x20000000) != 0
//! 高位阻挡 = (BackImage & 0x40000000) != 0
//! ```
//!
//! 每次 `Set` 调 `UpdateLayerCounts(x, y, 旧格, 新格)`，计数按 `+after 位 - before 位` **增量**更新
//! （`Document.LayerCounts.cs:10-12@acfbbf8a`；`Document.cs:51-56@acfbbf8a` 是唯一调用点）⇒
//! 单个格 O(1)，**不是**每次全图重扫。
//!
//! # 二、三个口径必须分开（**这是本模块最容易做错的地方**）
//!
//! | 口径 | 内容 | 出处 |
//! |------|------|------|
//! | ① **官方掩码** | C# **归一化后的内存表示**：`BackImage` 的 bit29/30 是编辑器私有碰撞位 | `Document.LayerCounts.cs:18-22@acfbbf8a` |
//! | ② **磁盘表示** | legacy 格式把碰撞存在 **`bit15`**：地表 `bk_img & 0x8000`、物体 `fr_img & 0x8000`；Mir3 存在格首 `flag` 的 bit0/bit1 | `LegacyMapReader.cs:149-150`、`:160-161`、`:438-439`、`:496-497`、`:545-546@acfbbf8a` |
//! | ③ **本仓双表示** | [`crate::Document::raw`]（磁盘原值，u32 `bk_img`）+ [`crate::Document::view`]（`bk_img: u16`，只留低 16 位） | `convert.rs:390-412`、`:483-507` |
//!
//! 归一化就发生在 ①←②：`LegacyMapReader.cs:149-150@acfbbf8a`
//! `if ((BackImage & 0x8000) != 0) BackImage = (BackImage & 0x7FFF) | 0x20000000;`。
//! ⇒ **照抄 `raw.bk_img & 0x20000000` 会得到几乎全 0 的假计数**。
//!
//! ## 实测证据（2026-11-06，40 张真实 `.map`）
//!
//! - **39 / 40** 张磁盘 bit29/30 **全 0**；
//! - 磁盘 `bit15` 大量非 0（例如 `0.map` 有 **162,820** 格 `bk_img & 0x8000`）；
//! - 只有 `3.map` 有 **4,501** 格磁盘 bit29。
//!
//! ⇒ 本模块一律**按格式把磁盘表示归一化成官方掩码**再计数（逐格式见第三节）。
//!
//! ⚠ [`crate::layer`] 模块文档第二节写「**绝不用 C# 的 `0x20000000` / `0x40000000`**」——
//! 那条说的是**写盘**（不能把私有位写进 `.map`，客户端读不懂）。本模块是**读语义**：
//! 只把磁盘位**映射**成官方掩码来计数，**一个位都不写盘**，两条纪律不矛盾。
//!
//! # 三、逐格式归一化表（`cell_mask` 的全部口径）
//!
//! `锚点` = `(x & 1) == 0 && (y & 1) == 0`（地表精灵占 2×2、只在偶偶格绘制）。
//!
//! | 格式族 | 地表 | 中层 | 物体 | 低位阻挡 | 高位阻挡 |
//! |---|---|---|---|---|---|
//! | `Old12` / `AntiHack` / `Shanda14` / `Shanda2012` | 锚点 && `view.bk_img & 0x7FFF != 0` | `view.mid_img != 0` | `view.fr_img & 0x7FFF != 0` | `view.bk_img & 0x8000` | `view.fr_img & 0x8000` **且** `view.fr_img & 0x7FFF != 0` |
//! | `Heroes34` | 同上（u32：`raw` bit16..28 + `view` 低 15 位） | 同上 | 同上 | `view.bk_img & 0x8000` | `view.fr_img & 0x8000` |
//! | `Wemade2010` | u32：`raw.bk_img` bit16..28 + `view` 低 15 位 | 同上 | 同上 | `raw.bk_img & 0x2000_0000` | `raw.bk_img & 0x4000_0000` 或 `view.fr_img & 0x8000` |
//! | `Mir3Wemade` / `Mir3Shanda` | `view.bk_img & 0x7FFF != 0`（本格式 `bk_img` 恒 0：背图在半分辨率块里，`mir3_wemade.rs:40`） | 同上 | 同上 | `(raw.flag & 0x01) != 0x01` | `(raw.flag & 0x02) != 0x02` |
//! | `CrystalCs` | u32：`raw.bk_img` bit16..28 + `view` 低 15 位 | 同上 | 同上 | `raw.bk_img & 0x2000_0000` | `raw.bk_img & 0x4000_0000` 或 `csharp_back_mark` 的 bit30 |
//!
//! 逐条依据：
//!
//! - **地表**：官方 `(x&1)==0 && (y&1)==0 && (BackImage & 0x1FFFFFFF) != 0`
//!   （`Document.LayerCounts.cs:18@acfbbf8a`）。legacy 的 `BackImage & 0x1FFFFFFF` 归一化后
//!   只剩 `bk_img & 0x7FFF`（bit15 被搬去 bit29）⇒ 用 `view` 的低 15 位。
//!   u32 三族（`Wemade2010` / `Heroes34` / `CrystalCs`）的 `BackImage` 保留高 16 位
//!   （`LegacyMapReader.cs:193`、`:532`、`:582@acfbbf8a`）⇒ 要带上 `raw` 的 bit16..28。
//!   ⚠ **不能把 bit15 当图号**：本仓 `view.bk_img` 的 bit15 是阻挡位（`layer.rs:19-38`）。
//! - **中层**：`view.mid_img != 0`（官方 `MiddleImage != 0`）。
//! - **物体**：`view.fr_img & 0x7FFF != 0`。官方 `HasForeground` 有**两个**条件
//!   （`Foreground is { }` 且 `(Image & 0x7FFF) != 0`，`Document.cs:26@acfbbf8a`），
//!   对九种格式的装载路径而言第一个条件都**不额外收窄**第二个：只有 bit15 的前景图号
//!   （`0x8000`）在 legacy 里根本不会被装成 `Foreground`（`LegacyMapReader.cs:153`、
//!   `:255`、`:311`、`:366@acfbbf8a` 的 `(frimage & 0x7FFF) > 0 && frindex > 0`），
//!   在 `CrystalCs` v1/v2 里 `fr_img != 0` 也会让 `HasFront` 为真
//!   （`LegacyMapReader.cs:621-623@acfbbf8a`）⇒ 两类情况都退化成「低 15 位非 0」。
//!   唯一的例外是 Mir3 的 `frindex == -1` 哨兵，见第五节第 4 条。
//! - **低位 / 高位阻挡**：
//!   - legacy 四族 + `Heroes34`：磁盘 `bit15` → 官方 bit29（`LegacyMapReader.cs:149-150`、
//!     `:251-252`、`:307-308`、`:362-363`、`:545-546@acfbbf8a`）；
//!   - 高位阻挡在 legacy 四族还要求前景图号低 15 位非 0（`LegacyMapReader.cs:160-161`、
//!     `:262-263`、`:318-319`、`:373-374@acfbbf8a` 的 `if (frimage & 0x8000)` 在
//!     `if (((frimage & 0x7FFF) > 0) && (frindex > 0))` 之内）；`Heroes34` 没有这层门
//!     （`:535`、`:548-551@acfbbf8a` 里 `HasFront = true` 恒定）⇒ 只看 bit15；
//!   - Mir3：`flag` bit0/bit1（`LegacyMapReader.cs:438-439`、`:496-497@acfbbf8a`）；
//!   - `Wemade2010`：磁盘 u32 的 bit29/bit30 + 前景 bit15（`LegacyMapReader.cs:195`、
//!     `:209-213@acfbbf8a`）；
//!   - `CrystalCs`：磁盘 u32 的 bit29/bit30，外加前景 bit15 触发的 bit30
//!     （`LegacyMapReader.cs:597-598`、`:624-625@acfbbf8a`；本仓复用
//!     `mir2_format::formats::crystal_cs::csharp_back_mark`）。
//!
//! # 四、增量更新 / 整块替换 / GUI 接线状态
//!
//! - 单格写：**唯一入口**是 [`crate::Document::write_view_cell`]（写视图 + 更新计数，O(1)）；
//!   `EditSession` 的五处写格路径（落笔 / 填充 / 印章 / 清空选区 / 选区填充）全走它；
//! - 撤销 / 重做：`UndoStack::undo_with` / `redo_with` 每写回一格回调一次
//!   （[`crate::EditSession::undo`] / [`crate::EditSession::redo`] 用回调更新计数）；
//! - 整块替换：[`crate::Document::rebuild_layer_counts`]（O(n)）供 load 与**外部整块换视图**后调用。
//!   ⚠ GUI 的 `mapdoc.rs:154` / `:194` 与 `edit.rs:322` / `:324` 用
//!   `std::mem::swap(document.view_mut(), &mut map)` 整块换视图，**绕过**了
//!   [`crate::Document::write_view_cell`] ⇒ 换完**必须**调 `rebuild_layer_counts()`，
//!   否则计数停留在旧视图上（旧值不会报错，只会显示错数）。
//! - **GUI / 菜单 / 网页显示未接线**：本模块只给数据（`LayerCount` × 5），
//!   官方那五条 UI 文本（`MainWindow.LayerCounts.cs:8-12@acfbbf8a`）不在本单范围。
//!
//! # 五、与派单书 / 上游分支的差异（显式记账，不静默改口径）
//!
//! 1. **legacy 高位阻挡要看前景低 15 位**：派单书的简化式是 `view.fr_img & 0x8000`，
//!    但上游 `:160-161@acfbbf8a` 的 `if (frimage & 0x8000)` 在
//!    `if (((frimage & 0x7FFF) > 0) && (frindex > 0))` 之内。官方用例
//!    `LayerCountTests.cs:13-14@acfbbf8a` 也反证：`(1,1)` 放 `fr = 0x8000` 之后断言五层仍各 1
//!    ⇒ 只有 bit15 的前景**不能**算高位阻挡。本模块按上游实现（并有用例钉住）。
//! 2. **`Heroes34` 的高位阻挡没有这层门**（`:535`、`:548-551@acfbbf8a`），只看 bit15。
//! 3. **`Wemade2010` 不走 XOR 解码**：上游 `:193@acfbbf8a` 把磁盘 u32 与 `0xAA38AA38` 异或后
//!    才存进 `BackImage`。若照解码值判位，掩码自身的 bit29 = 1 会让「**几乎所有格**」都算低位阻挡
//!    （空格的磁盘值 0 解码成 `0xAA38AA38`）⇒ 本仓按**磁盘位**判（与派单书一致）。
//!    该格式在真实语料里 **0 个样本**，两种口径都无法用文件交叉验证，故以「位29/30 = 磁盘上
//!    的碰撞标记」这一可解释口径为准，差异在此记账。
//! 4. **Mir3 物体的 `frindex == -1` 哨兵未参与判定**：上游要求 `frindex > 0`
//!    （`:441`、`:499@acfbbf8a`），而 `btArea == 255` 时 `frindex = -1`。本仓的
//!    `mir2_map::Cell` 没有前景库号字段（只有 `area` 一字节），逐格判哨兵会把「库号 -1」与
//!    「库号 200」混在同一个字节上；且真实语料里没有 Mir3 样本可交叉验证 ⇒
//!    按官方 `HasForeground` 的**图像条件**判（`fr_img & 0x7FFF != 0`）。
//!    影响面：仅 `fr_img` 低 15 位非 0 且库号哨兵为 -1 的格（正常地图里「有前景」与
//!    「库号 -1」互斥），偏多计不计少计。
//! 5. **`CrystalCs` 的 `ver` / `fr_count` 门照抄 C#**：`ver > 1` 才补 bit30；`ver > 2` 时
//!    还要求 `fr_count == 1`（`:588-598@acfbbf8a` 的标记只在 `frcount == 1` 支里）。
//!    `ver` 从 `raw.header_raw[0..2]` 读（`crystal_cs.rs:7` 的头部表），读不到按 `0`
//!    （= 不补 bit30），**不猜**。

use mir2_format::{formats, Cell as RawCell, FormatId, RawMap};
use mir2_map::{Cell as ViewCell, Map};

use crate::layer::EditLayer;

/// 地表层位（官方 `mask` 的 bit0）。`Document.LayerCounts.cs:18@acfbbf8a`
pub const MASK_GROUND: u8 = 1;
/// 中层位（官方 bit1）。`Document.LayerCounts.cs:19@acfbbf8a`
pub const MASK_MIDDLE: u8 = 2;
/// 物体层位（官方 bit2）。`Document.LayerCounts.cs:20@acfbbf8a`
pub const MASK_FRONT: u8 = 4;
/// 低位阻挡位（官方 bit3）。`Document.LayerCounts.cs:21@acfbbf8a`
pub const MASK_LOW_COLLISION: u8 = 8;
/// 高位阻挡位（官方 bit4）。`Document.LayerCounts.cs:22@acfbbf8a`
pub const MASK_HIGH_COLLISION: u8 = 16;

/// 地表图号掩码（低 15 位）。官方 `BackImage & 0x1FFFFFFF` 归一化后的图号部分。
const IMG_MASK: u16 = 0x7FFF;
/// backdrop 阻挡位（legacy 地表的 `bit15` → 官方 bit29）。
const COLLISION_BIT: u16 = 0x8000;
/// u32 格式里保留在图号里的高 13 位（bit16..28）。官方掩码 `0x1FFFFFFF` 的上半部分。
const U32_IMG_HIGH_MASK: u32 = 0x1FFF_0000;
/// 官方低位阻挡位（C# `BackImage` 的 bit29）。
const LOW_BIT: u32 = 0x2000_0000;
/// 官方高位阻挡位（C# `BackImage` 的 bit30）。
const HIGH_BIT: u32 = 0x4000_0000;

/// `CrystalCs` 头部里 `ver` 的偏移（`i16`，`crystal_cs.rs:7` 的头部表第 0 行）。
const CRYSTAL_VER_OFFSET: usize = 0;

/// 五层计数的快照（官方 `int[5] layerCounts` 的等价物）。
///
/// 字段顺序与官方 `MapLayer` 枚举一致：`Ground, Middle, Objects, LowCollision, HighCollision`
/// （`Document.cs:5@acfbbf8a`），但这里用**具名字段**而不是数组下标 —— 下标一错就是
/// 「地表数显示成物体数」这类静默错数。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayerCounts {
    /// 地表层：偶偶锚点且地表图号（低 15 位）非 0 的格数。
    pub ground: usize,
    /// 中层：`mid_img != 0` 的格数。
    pub middle: usize,
    /// 物体：前景图号低 15 位非 0 的格数。
    pub front: usize,
    /// 低位阻挡：官方 `BackImage & 0x20000000`（legacy 的 `bk_img & 0x8000`）的格数。
    pub low_collision: usize,
    /// 高位阻挡：官方 `BackImage & 0x40000000`（legacy 的 `fr_img & 0x8000`）的格数。
    pub high_collision: usize,
}

impl LayerCounts {
    /// 全 0（空图 / 文件刚打开前的状态）。
    pub const fn zero() -> Self {
        Self {
            ground: 0,
            middle: 0,
            front: 0,
            low_collision: 0,
            high_collision: 0,
        }
    }

    /// 由一格掩码构造（五个 `0/1` 之和；测试与诊断用）。
    pub const fn of_mask(mask: u8) -> Self {
        Self {
            ground: ((mask & MASK_GROUND) != 0) as usize,
            middle: ((mask & MASK_MIDDLE) != 0) as usize,
            front: ((mask & MASK_FRONT) != 0) as usize,
            low_collision: ((mask & MASK_LOW_COLLISION) != 0) as usize,
            high_collision: ((mask & MASK_HIGH_COLLISION) != 0) as usize,
        }
    }

    /// 按 [`EditLayer`] 取某一层的计数（官方 `LayerCount(MapLayer)` 的等价物）。
    pub const fn get(&self, layer: EditLayer) -> usize {
        match layer {
            EditLayer::Ground => self.ground,
            EditLayer::Middle => self.middle,
            EditLayer::Front => self.front,
            EditLayer::LowCollision => self.low_collision,
            EditLayer::HighCollision => self.high_collision,
        }
    }

    /// 五层计数之和（**不是**「有内容的格数」：一格可以同时占多层）。
    pub const fn total(&self) -> usize {
        self.ground + self.middle + self.front + self.low_collision + self.high_collision
    }

    /// 五层是否全 0。
    pub const fn is_zero(&self) -> bool {
        self.total() == 0
    }

    /// 加一格掩码（重建时用；`mask` 里每个 bit 加 1）。
    pub(crate) fn add_mask(&mut self, mask: u8) {
        if (mask & MASK_GROUND) != 0 {
            self.ground += 1;
        }
        if (mask & MASK_MIDDLE) != 0 {
            self.middle += 1;
        }
        if (mask & MASK_FRONT) != 0 {
            self.front += 1;
        }
        if (mask & MASK_LOW_COLLISION) != 0 {
            self.low_collision += 1;
        }
        if (mask & MASK_HIGH_COLLISION) != 0 {
            self.high_collision += 1;
        }
    }

    /// 增量更新：`-before 位 +after 位`（官方 `UpdateLayerCounts` 的循环体）。
    ///
    /// 官方用 `int`（`Document.LayerCounts.cs:12@acfbbf8a`）；本仓用 `usize`，
    /// 减到负数说明「计数与地图脱节」—— debug 下直接炸，release 下饱和到 0 并**不掩盖**
    /// （下一次 [`LayerCounts::is_zero`] 之类的读会如实反映 0，而不是回绕成一个巨大数）。
    pub(crate) fn apply(&mut self, before: u8, after: u8) {
        Self::step(&mut self.ground, before, after, 0);
        Self::step(&mut self.middle, before, after, 1);
        Self::step(&mut self.front, before, after, 2);
        Self::step(&mut self.low_collision, before, after, 3);
        Self::step(&mut self.high_collision, before, after, 4);
    }

    /// 单层的一位增量。
    fn step(field: &mut usize, before: u8, after: u8, shift: u8) {
        let d = i32::from((after >> shift) & 1) - i32::from((before >> shift) & 1);
        if d == 0 {
            return;
        }
        debug_assert!(
            *field as i64 + i64::from(d) >= 0,
            "层计数被减成负数：说明计数与地图脱节（before={before:#04X} after={after:#04X}）"
        );
        *field = field.saturating_add_signed(d as isize);
    }
}

/// 归一化所需的**头部参数**：`FormatId` 之外，两种格式的判定还要头部里的两个值。
///
/// | 字段 | 谁用 | 来源 |
/// |------|------|------|
/// | `format` | 全部 | [`mir2_format::RawMap::format`] |
/// | `crystal_ver` | `CrystalCs` | `raw.header_raw[0..2]`（`i16`） |
/// | `wemade_xor` | `Wemade2010` | `raw.header_raw[23..25]`（`i16`，`wemade2010.rs` 的 `XOR_OFFSET`） |
///
/// 为什么不在 [`cell_mask`] 的签名里再收一个 `&RawMap`：那会把「纯函数、可逐格对拍」
/// 变成「必须拿整张图」，测试与网页层都更难用；这里只把**两个标量**带出来。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MaskContext {
    /// 磁盘格式。
    pub format: FormatId,
    /// `CrystalCs` 的版本号（其它格式忽略；读不到按 `0`）。
    pub crystal_ver: i16,
    /// `Wemade2010` 的头部 `xor` 掩码（其它格式忽略；读不到按 `0`）。
    ///
    /// ⚠ 当前实现**不用**它（见模块文档第五节第 3 条），保留是为了让调用方
    /// 能把「C# 解码所需参数」一并带出来，将来若改解码口径无需再改签名。
    pub wemade_xor: i16,
}

impl MaskContext {
    /// 只要格式（`crystal_ver` / `wemade_xor` 按 0）。
    ///
    /// 对七种不需要头部参数的格式这就是**完整**口径；对 `CrystalCs` / `Wemade2010`
    /// 等效于「ver=0（不补 bit30）/ xor 不参与」，仅供纯函数测试 —— 真值一律走
    /// [`MaskContext::from_raw`]。
    pub const fn of(format: FormatId) -> Self {
        Self {
            format,
            crystal_ver: 0,
            wemade_xor: 0,
        }
    }

    /// 从字节层真值取全部参数。
    pub fn from_raw(raw: &RawMap) -> Self {
        Self {
            format: raw.format,
            crystal_ver: read_i16(&raw.header_raw, CRYSTAL_VER_OFFSET).unwrap_or(0),
            wemade_xor: read_i16(
                &raw.header_raw,
                formats::wemade2010::XOR_OFFSET,
            )
            .unwrap_or(0),
        }
    }
}

/// 读一个小端 `i16`；越界返回 `None`（不 panic、不猜值）。
fn read_i16(bytes: &[u8], at: usize) -> Option<i16> {
    let s = bytes.get(at..at + 2)?;
    Some(i16::from_le_bytes([s[0], s[1]]))
}

/// 一格 → 官方五层掩码（**纯函数**，不改任何东西）。
///
/// 参数与语义见模块文档第三节；`x`/`y` 只用于地表锚点判定。
///
/// ⚠ 对 `CrystalCs` / `Wemade2010` 请用 [`cell_mask_ext`]（它们要头部参数）；
/// 本函数把 `crystal_ver` / `wemade_xor` 当 0，等价于「CrystalCs 不补 bit30」。
pub fn cell_mask(raw: &RawCell, view: &ViewCell, format: FormatId, x: u16, y: u16) -> u8 {
    cell_mask_ext(raw, view, MaskContext::of(format), x, y)
}

/// 同 [`cell_mask`]，但带上头部参数（`Document` 内部与 `CrystalCs` / `Wemade2010` 用）。
pub fn cell_mask_ext(raw: &RawCell, view: &ViewCell, ctx: MaskContext, x: u16, y: u16) -> u8 {
    let mut mask = 0u8;
    if (x & 1) == 0 && (y & 1) == 0 && has_ground_image(raw, view, ctx) {
        mask |= MASK_GROUND;
    }
    if view.mid_img != 0 {
        mask |= MASK_MIDDLE;
    }
    if (view.fr_img & IMG_MASK) != 0 {
        mask |= MASK_FRONT;
    }
    if has_low_collision(raw, view, ctx) {
        mask |= MASK_LOW_COLLISION;
    }
    if has_high_collision(raw, view, ctx) {
        mask |= MASK_HIGH_COLLISION;
    }
    mask
}

/// 官方 `(BackImage & 0x1FFFFFFF) != 0`（**不含**锚点判定）。
fn has_ground_image(raw: &RawCell, view: &ViewCell, ctx: MaskContext) -> bool {
    match ctx.format {
        // u32 三族：raw 的高 13 位（bit16..28）+ 视图的低 15 位（bit15 是阻挡位，不算图号）。
        FormatId::Wemade2010 | FormatId::Heroes34 | FormatId::CrystalCs => {
            let img = (raw.bk_img & U32_IMG_HIGH_MASK) | u32::from(view.bk_img & IMG_MASK);
            (img & 0x1FFF_FFFF) != 0
        }
        // legacy 六族（12/14/36 档 + Mir3 两支）：图号就是 `bk_img` 的低 15 位。
        // legacy 四族装载期把 bit15 搬到 bit29（`LegacyMapReader.cs:149-150@acfbbf8a`），
        // Mir3 两支用 `flag` 判碰撞（`:438-439@acfbbf8a`）⇒ 图号部分都只剩低 15 位。
        _ => (view.bk_img & IMG_MASK) != 0,
    }
}

/// 官方 `(BackImage & 0x20000000) != 0` 的**按格式归一化**版本。
fn has_low_collision(raw: &RawCell, view: &ViewCell, ctx: MaskContext) -> bool {
    match ctx.format {
        // legacy 五族：磁盘 bit15 → 官方 bit29（`LegacyMapReader.cs:149-150`、`:251-252`、
        // `:307-308`、`:362-363`、`:545-546@acfbbf8a`）。
        FormatId::Old12
        | FormatId::AntiHack
        | FormatId::Shanda14
        | FormatId::Shanda2012
        | FormatId::Heroes34 => (view.bk_img & COLLISION_BIT) != 0,
        // Mir3：格首 flag 的 bit0（`LegacyMapReader.cs:438-439`、`:496-497@acfbbf8a`）。
        // 视图里没有 flag（`mir2_map::Cell` 装不下）⇒ 本层计数**不随编辑变化**（编辑改不到它）。
        FormatId::Mir3Wemade | FormatId::Mir3Shanda => (raw.flag & 0x01) != 0x01,
        // u32 两族：磁盘位就是官方位（见模块文档第五节第 3 条：不做 XOR 解码）。
        FormatId::Wemade2010 | FormatId::CrystalCs => (raw.bk_img & LOW_BIT) != 0,
    }
}

/// 官方 `(BackImage & 0x40000000) != 0` 的**按格式归一化**版本。
fn has_high_collision(raw: &RawCell, view: &ViewCell, ctx: MaskContext) -> bool {
    match ctx.format {
        // legacy 四族：前景 bit15 → 官方 bit30，但上游只在「前景低 15 位 > 0 且库号 > 0」
        // 时才会装出 `Foreground`（`LegacyMapReader.cs:153-161`、`:255-263`、`:311-319`、
        // `:366-374@acfbbf8a`）⇒ 只有 bit15 的前景格**不能**算高位阻挡
        // （官方用例 `LayerCountTests.cs:13-14@acfbbf8a` 反证）。
        FormatId::Old12 | FormatId::AntiHack | FormatId::Shanda14 | FormatId::Shanda2012 => {
            (view.fr_img & COLLISION_BIT) != 0 && (view.fr_img & IMG_MASK) != 0
        }
        // Heroes34：`HasFront = true` 恒定（`:535`、`:548-551@acfbbf8a`）⇒ 只看 bit15。
        FormatId::Heroes34 => (view.fr_img & COLLISION_BIT) != 0,
        // Mir3：flag 的 bit1（`LegacyMapReader.cs:438-439`、`:496-497@acfbbf8a`）。
        FormatId::Mir3Wemade | FormatId::Mir3Shanda => (raw.flag & 0x02) != 0x02,
        // Wemade2010：磁盘 bit30 或「前景 bit15 ⇒ 补 bit30」（`:209-213@acfbbf8a`）。
        FormatId::Wemade2010 => {
            (raw.bk_img & HIGH_BIT) != 0 || (view.fr_img & COLLISION_BIT) != 0
        }
        // CrystalCs：磁盘 bit30，或按 ver/fr_count 门补的 bit30（`:588-598`、`:620-625@acfbbf8a`）。
        FormatId::CrystalCs => crystal_cs_high(raw, view, ctx),
    }
}

/// `CrystalCs` 的高位阻挡（`LegacyMapReader.cs:588-598` / `:620-625@acfbbf8a`）。
///
/// - `ver <= 2`：前景图号 bit15 ⇒ 补 bit30，条件是 `ver > 1`（`:624`）；
/// - `ver > 2`：只有 `fr_count == 1` 那一支才读前景、也才补 bit30（`:592-598`）。
fn crystal_cs_high(raw: &RawCell, view: &ViewCell, ctx: MaskContext) -> bool {
    if (raw.bk_img & HIGH_BIT) != 0 {
        return true;
    }
    if ctx.crystal_ver <= 1 {
        return false;
    }
    if ctx.crystal_ver > 2 && raw.fr_count != 1 {
        return false;
    }
    // 用 `csharp_back_mark(0, …)` 把「补位」这一步交给格式层（单一实现，不抄第二份）。
    (formats::crystal_cs::csharp_back_mark(0, view.fr_img, ctx.crystal_ver) & HIGH_BIT) != 0
}

/// 整张图重建五层计数（O(n)；`Document::rebuild_layer_counts` 的实现体）。
///
/// `raw` 与 `view` 按**列优先**逐格配对（`i = x * height + y`），长度不等时按较短的一侧
/// 停（与 [`mir2_format::RawMap::trailer_len`] 的容忍口径一致：不猜缺的格）。
pub(crate) fn count_map(raw: &RawMap, view: &Map) -> LayerCounts {
    let ctx = MaskContext::from_raw(raw);
    let h = usize::from(view.height);
    if h == 0 {
        return LayerCounts::zero();
    }
    let mut counts = LayerCounts::zero();
    for (i, (rc, vc)) in raw.cells.iter().zip(view.cells.iter()).enumerate() {
        let x = (i / h) as u16;
        let y = (i % h) as u16;
        counts.add_mask(cell_mask_ext(rc, vc, ctx, x, y));
    }
    counts
}

/// 「写一格 → 增量更新计数」的借用包（[`crate::Document`] 与撤销栈回调共用**同一实现**）。
///
/// 为什么单独一个类型：撤销栈要靠 `&mut Map` 写视图，同时还要改计数 ——
/// 若把 `&mut Document` 整个借出去，两件事会因为**同一个 `&mut self`** 打起来。
/// 这里只拿 `raw`（只读）+ `counts`（可写）+ 两个标量，与 `&mut view` 是**不重叠**的借用。
pub(crate) struct CountKeeper<'a> {
    /// 字节层真值（只读：只取 bit29/30、`flag`、`fr_count` 这些视图装不下的位）。
    raw: &'a RawMap,
    /// 待更新的计数。
    counts: &'a mut LayerCounts,
    /// 归一化参数（`Copy`）。
    ctx: MaskContext,
}

impl<'a> CountKeeper<'a> {
    /// 造一个借用包（`Document::view_and_keeper` 用）。
    pub(crate) fn new(raw: &'a RawMap, counts: &'a mut LayerCounts) -> Self {
        Self {
            raw,
            counts,
            ctx: MaskContext::from_raw(raw),
        }
    }

    /// 记一次格值变化：`before` → `after`（都是**视图格**；越界 / 格号对不上 = 不改计数）。
    ///
    /// ⚠ 调用方必须**先**把 `after` 写进视图、再调本方法：本方法只更新计数，
    /// 不写地图（写地图需要 `&mut Map`，那正是本类型借不出去的东西）。
    pub(crate) fn record(&mut self, x: u16, y: u16, before: &ViewCell, after: &ViewCell) {
        if x >= self.raw.width || y >= self.raw.height {
            return;
        }
        let i = x as usize * usize::from(self.raw.height) + usize::from(y);
        let Some(rc) = self.raw.cells.get(i) else {
            return;
        };
        let b = cell_mask_ext(rc, before, self.ctx, x, y);
        let a = cell_mask_ext(rc, after, self.ctx, x, y);
        self.counts.apply(b, a);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一个视图格（只填三个图号，其余缺省）。
    fn 视图(bk: u16, mid: u16, fr: u16) -> ViewCell {
        ViewCell {
            bk_img: bk,
            mid_img: mid,
            fr_img: fr,
            ..ViewCell::default()
        }
    }

    /// 造一个字节层格（只填用到的字段）。
    fn 字节(bk: u32, mid: u16, fr: u16, flag: u8, fr_count: u8) -> RawCell {
        RawCell {
            bk_img: bk,
            mid_img: mid,
            fr_img: fr,
            flag,
            fr_count,
            ..RawCell::default()
        }
    }

    /// 地表锚点：偶偶格才算，奇/奇、奇/偶、偶/奇都不算。九种格式同一判据。
    #[test]
    fn 地表只在偶偶锚点计数() {
        for fmt in [
            FormatId::Old12,
            FormatId::AntiHack,
            FormatId::Shanda14,
            FormatId::Shanda2012,
            FormatId::Heroes34,
            FormatId::Wemade2010,
            FormatId::Mir3Wemade,
            FormatId::Mir3Shanda,
            FormatId::CrystalCs,
        ] {
            let raw = 字节(1, 0, 0, 0, 0);
            let view = 视图(0x0001, 0, 0);
            let ctx = MaskContext::of(fmt);
            assert_eq!((cell_mask_ext(&raw, &view, ctx, 0, 0) & MASK_GROUND), MASK_GROUND, "{fmt:?} (0,0)");
            assert_eq!((cell_mask_ext(&raw, &view, ctx, 1, 0) & MASK_GROUND), 0, "{fmt:?} (1,0)");
            assert_eq!((cell_mask_ext(&raw, &view, ctx, 0, 1) & MASK_GROUND), 0, "{fmt:?} (0,1)");
            assert_eq!((cell_mask_ext(&raw, &view, ctx, 1, 1) & MASK_GROUND), 0, "{fmt:?} (1,1)");
        }
    }

    /// legacy 四族：`bit15` 是低位阻挡；**只有 bit15 的前景不算高位阻挡**（官方用例反证）。
    #[test]
    fn 旧族_bit15_归一化与前景门() {
        for fmt in [
            FormatId::Old12,
            FormatId::AntiHack,
            FormatId::Shanda14,
            FormatId::Shanda2012,
        ] {
            let ctx = MaskContext::of(fmt);
            // bk 只有 bit15 ⇒ 低位阻挡 1、地表 0（bit15 不是图号）
            let raw = 字节(0, 0, 0, 0, 0);
            let only_bit15 = 视图(0x8000, 0, 0);
            assert_eq!(cell_mask_ext(&raw, &only_bit15, ctx, 0, 0), MASK_LOW_COLLISION, "{fmt:?}");
            // 前景只有 bit15 ⇒ 物体 0、高位阻挡 0
            let only_fr_bit15 = 视图(0, 0, 0x8000);
            assert_eq!(cell_mask_ext(&raw, &only_fr_bit15, ctx, 0, 0), 0, "{fmt:?}");
            // 前景 bit15 + 低 15 位非 0 ⇒ 物体 + 高位阻挡
            let fr_high = 视图(0, 0, 0x8001);
            assert_eq!(
                cell_mask_ext(&raw, &fr_high, ctx, 0, 0),
                MASK_FRONT | MASK_HIGH_COLLISION,
                "{fmt:?}"
            );
        }
    }

    /// `Heroes34`：前景 `HasFront = true` 恒定 ⇒ 只看 bit15（没有低 15 位那道门）。
    #[test]
    fn 英雄三四高位阻挡只看_bit15() {
        let ctx = MaskContext::of(FormatId::Heroes34);
        let raw = 字节(0, 0, 0, 0, 0);
        let only = 视图(0, 0, 0x8000);
        assert_eq!(
            cell_mask_ext(&raw, &only, ctx, 0, 0),
            MASK_HIGH_COLLISION,
            "Heroes34 只有 bit15 的前景也算高位阻挡（LegacyMapReader.cs:535/548-551）"
        );
        // 地表：u32 的高位段也算图号（bit16..28），但 bit15 不算
        let u32_bk = 字节(0x0001_0000, 0, 0, 0, 0);
        let empty = 视图(0, 0, 0);
        assert_eq!(cell_mask_ext(&u32_bk, &empty, ctx, 0, 0), MASK_GROUND);
    }

    /// `Wemade2010`：u32 的 bit29/30 直接判低位/高位阻挡（不做 XOR 解码）。
    #[test]
    fn 伟玛德_u32_位判定() {
        let ctx = MaskContext::of(FormatId::Wemade2010);
        let view = 视图(0, 0, 0);
        let low = 字节(LOW_BIT, 0, 0, 0, 0);
        assert_eq!(cell_mask_ext(&low, &view, ctx, 0, 0), MASK_LOW_COLLISION);
        let high = 字节(HIGH_BIT, 0, 0, 0, 0);
        assert_eq!(cell_mask_ext(&high, &view, ctx, 0, 0), MASK_HIGH_COLLISION);
        // 前景 bit15 ⇒ 补高位阻挡
        let fr = 视图(0, 0, 0x8000);
        assert_eq!(cell_mask_ext(&low, &fr, ctx, 0, 0), MASK_LOW_COLLISION | MASK_HIGH_COLLISION);
        // 地面图号：raw 的 bit16..28 与视图低 15 位都算
        let empty_raw = 字节(0, 0, 0, 0, 0);
        assert_eq!(cell_mask_ext(&empty_raw, &视图(0x8000, 0, 0), ctx, 0, 0), 0, "bit15 不是图号");
        assert_eq!(cell_mask_ext(&empty_raw, &视图(0x0001, 0, 0), ctx, 0, 0), MASK_GROUND);
    }

    /// Mir3 两族：`flag` 的 bit0 = 低位阻挡、bit1 = 高位阻挡（极性：0 才是阻挡）。
    #[test]
    fn mir3_标志字节() {
        for fmt in [FormatId::Mir3Wemade, FormatId::Mir3Shanda] {
            let ctx = MaskContext::of(fmt);
            let view = 视图(0, 0, 0);
            for (flag, expect) in [
                (0u8, MASK_LOW_COLLISION | MASK_HIGH_COLLISION),
                (1, MASK_HIGH_COLLISION),
                (2, MASK_LOW_COLLISION),
                (3, 0),
            ] {
                let raw = 字节(0, 0, 0, flag, 0);
                assert_eq!(cell_mask_ext(&raw, &view, ctx, 0, 0), expect, "{fmt:?} flag={flag}");
            }
        }
    }

    /// `CrystalCs`：`ver > 1` 才补 bit30；`ver > 2` 还要求 `fr_count == 1`。
    #[test]
    fn crystalcs_版本门() {
        let view = 视图(0, 0, 0x8000);
        let raw_v1 = 字节(0, 0, 0x8000, 0, 1);
        let ctx1 = MaskContext {
            format: FormatId::CrystalCs,
            crystal_ver: 1,
            wemade_xor: 0,
        };
        assert_eq!(cell_mask_ext(&raw_v1, &view, ctx1, 0, 0), 0, "v1 不补 bit30");
        for ver in [2i16, 3, 5] {
            let ctx = MaskContext {
                format: FormatId::CrystalCs,
                crystal_ver: ver,
                wemade_xor: 0,
            };
            assert_eq!(
                cell_mask_ext(&raw_v1, &view, ctx, 0, 0),
                MASK_HIGH_COLLISION,
                "v{ver} 且 fr_count=1 时应补 bit30"
            );
            // ver > 2 且 fr_count == 0 ⇒ C# 根本不读前景，不补位
            let raw_no_front = 字节(0, 0, 0x8000, 0, 0);
            if ver > 2 {
                assert_eq!(cell_mask_ext(&raw_no_front, &view, ctx, 0, 0), 0, "v{ver} fr_count=0");
            }
        }
        // 磁盘 bit30 恒算（与 ver 无关）
        let disk_high = 字节(HIGH_BIT, 0, 0, 0, 0);
        assert_eq!(
            cell_mask_ext(&disk_high, &视图(0, 0, 0), ctx1, 0, 0),
            MASK_HIGH_COLLISION
        );
    }

    /// 掩码 → 计数：五层各 1 / 全 0；`get` 与具名字段一致。
    #[test]
    fn 掩码与计数一一对应() {
        let all = LayerCounts::of_mask(
            MASK_GROUND | MASK_MIDDLE | MASK_FRONT | MASK_LOW_COLLISION | MASK_HIGH_COLLISION,
        );
        assert_eq!(all, LayerCounts { ground: 1, middle: 1, front: 1, low_collision: 1, high_collision: 1 });
        assert_eq!(all.total(), 5);
        assert_eq!(all.get(EditLayer::Ground), 1);
        assert_eq!(all.get(EditLayer::Middle), 1);
        assert_eq!(all.get(EditLayer::Front), 1);
        assert_eq!(all.get(EditLayer::LowCollision), 1);
        assert_eq!(all.get(EditLayer::HighCollision), 1);
        assert!(LayerCounts::of_mask(0).is_zero());
        assert_eq!(LayerCounts::default(), LayerCounts::zero());
    }

    /// 增量：`+after -before` 与「重建」必须逐格相等（一格从一格值换成另一格值）。
    #[test]
    fn 增量与重建等价() {
        let m1 = MASK_GROUND | MASK_MIDDLE;
        let m2 = MASK_LOW_COLLISION | MASK_HIGH_COLLISION | MASK_FRONT;
        let mut c = LayerCounts::zero();
        assert!(c.is_zero());
        // 加一格 m1 ⇒ 与「整图只有这一格」的重建结果相同
        c.add_mask(m1);
        assert_eq!(c, LayerCounts::of_mask(m1));
        // 同一格换成 m2：-m1 +m2 ⇒ 等于「整图只有 m2」
        c.apply(m1, m2);
        assert_eq!(c, LayerCounts::of_mask(m2));
        // 再换回 m1
        c.apply(m2, m1);
        assert_eq!(c, LayerCounts::of_mask(m1));
        // 清空这一格
        c.apply(m1, 0);
        assert!(c.is_zero());
        // no-op：一格都不动
        c.apply(0, 0);
        assert!(c.is_zero());
    }
}
