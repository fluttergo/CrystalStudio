//! 笔刷内容（族 / 图号 / 库号 / 来源）、吸管整格、以及笔刷尺寸的规范化。
//!
//! 抽取自 `miredit-gui/src/edit.rs:459-552`（`palette_index_to_sprite` / `Brush` / `PickedCell`）
//! 与 `miredit-gui/src/metrics.rs:159-199`（`BRUSH_MIN/MAX/STEP/DEFAULT` / `clamp_brush_size`）。
//!
//! # 一、为什么本 crate 自带 `Family` / `LibRef`
//!
//! `miredit-gui` 里的 [`Family`] / `LibRef` 来自 `mir2-assets`（`lib_paths.rs:47` / `:108`）。
//! 本 crate 的依赖白名单只有 `std` + `mir2-map`（任务书 §七），因此这里按**同口径重新定义**：
//!
//! - [`Family`]：三族 = 地图格三个图库字段（`btTiles` 地面 / `btsmTiles` 中层 / `btArea` 表层），
//!   与 `mir2-assets/src/lib_paths.rs:47-54` 同名同序同语义；
//! - [`LibRef`]：`{ family, index }`，`index` 就是地图格字段**原值**（`mir2-assets/src/lib_paths.rs:103-113`）。
//!
//! ⚠ **抽取期间两边允许重复**（任务书 §七）：GUI 仍用自己的 `mir2_assets::Family`，
//! 等主线程决定谁做门面时，这里换成 `pub use mir2_assets::{Family, LibRef}` 即可（机械替换）。
//! 三族索引空间互不相通：`LibRef::ground(0)`（`Tiles`）与 `LibRef::surface(0)`（`Objects`）
//! 是两个不同的库，绝不可互换（`mir2-assets/src/lib_paths.rs:105-107` 同款警告）。
//!
//! # 二、图号是 **1 基**（`0` = 该层没有图）
//!
//! [`palette_index_to_sprite`] 把素材面板的**0 基帧索引**换成**1 基图号**（`+1`）。
//! 真值链（`edit.rs:459-477`）：
//! - 渲染 `miredit-render/src/compose.rs` 三层都是 `(img & 0x7FFF)` → `0` 跳过 → `cache.get(lib, img - 1)`；
//! - 客户端 `GodotClient/game/world/map_loader.gd:51-52`「帧号是 1-based，取图前先 -1」；
//! - 原作 `GameScene.cpp:3567-3577`：`nImgNumber = wBkImg & 0x7FFF; if (nImgNumber > 0) nImgNumber -= 1`。
//!
//! 换算**只在这里做一次**，别处不要再 ±1（`edit.rs:1272` 原话）。

use std::fmt;

use mir2_map::Map;

use crate::layer::{COLLISION_BIT, EditLayer, SPRITE_MAX};

// ───────────────────────── 族 / 图库定位键 ─────────────────────────

/// 图库族：与地图格的三个图库字段一一对应（口径同 `mir2-assets/src/lib_paths.rs:47-54`）。
///
/// # ⚠ 与 `mir2_assets::Family` 的关系（2026-11-05 裁定：**不合并**）
///
/// 两个类型**语义一一对应**、**按序号一一对应**：
///
/// | 序号 | 本 crate | `mir2_assets::Family` | 地图格字段 | 文件基名 |
/// |---|---|---|---|---|
/// | 0 | [`Family::Ground`] | `Ground` | `btTiles` | `Tiles` |
/// | 1 | [`Family::Middle`] | `Middle` | `btsmTiles` | `SmTiles` |
/// | 2 | [`Family::Surface`] | `Surface` | `btArea` | `Objects` |
///
/// **但两者的 [`Family::label`] 是两套不同用途的词表，不可合并**：
/// - 本 crate 的 `label()` = **面向用户的界面词**（`地表图层` / `中层图层` / `物体`），
///   与 [`EditLayer::name`] 逐字相同 —— 它会被写进状态栏的拒绝提示
///   （`笔刷族（地表图层）与当前图层（物体）不一致`）；
/// - `mir2_assets::Family::label()` = **面向数据层的日志词**（`地面` / `中层` / `表层`），
///   用在图库扫描/打开的日志里。
///
/// 曾考虑 `pub use mir2_assets::{Family, LibRef}` 消灭副本，实测会把这 6 处 note 文案
/// 从「地表图层」改成「地面」，打红 4 条**逐字**断言（core `族名与层名同源`、
/// core `拒绝原因可上屏`、gui `族不匹配拒绝且一字节不等`、gui `阻挡层拒绝图库笔刷`）
/// ⇒ 违反"断言不许放宽"，故**保留两份定义**，并用测试
/// `族与mir2_assets按序号一一对应` 钉住"变体数 / 序号 / 基名"三者不许各自漂。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Family {
    /// 地面层（地图格 `btTiles`）→ 文件基名 `Tiles` / `Tiles{index+1}`。
    Ground,
    /// 中层（地图格 `btsmTiles`）→ 文件基名 `SmTiles` / `SmTiles{index+1}`。
    Middle,
    /// 表层（地图格 `btArea`）→ 文件基名 `Objects` / `Objects{index+1}`。
    Surface,
}

impl Family {
    /// 三个族的规范顺序。来源：`mir2-assets/src/lib_paths.rs:58`。
    pub const ALL: [Family; 3] = [Family::Ground, Family::Middle, Family::Surface];

    /// 文件基名（不含序号）。来源：`mir2-assets/src/lib_paths.rs:61-67`。
    pub const fn stem(self) -> &'static str {
        match self {
            Family::Ground => "Tiles",
            Family::Middle => "SmTiles",
            Family::Surface => "Objects",
        }
    }

    /// 族在 [`Family::ALL`] 中的下标。来源：`mir2-assets/src/lib_paths.rs:88-94`。
    pub const fn index(self) -> usize {
        match self {
            Family::Ground => 0,
            Family::Middle => 1,
            Family::Surface => 2,
        }
    }

    /// 族的中文层名 —— **与 [`EditLayer::name`] 同一批文案**（`edit.rs:366-368`：
    /// 「两处各写一套中文的话，状态栏与素材面板迟早会不同名」）。
    pub const fn label(self) -> &'static str {
        match self {
            Family::Ground => EditLayer::Ground.name(),
            Family::Middle => EditLayer::Middle.name(),
            Family::Surface => EditLayer::Front.name(),
        }
    }
}

impl fmt::Display for Family {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// 图库定位键：**族 + 族内索引**（`index` 就是地图格 `btTiles`/`btsmTiles`/`btArea` 的原值）。
///
/// 口径同 `mir2-assets/src/lib_paths.rs:103-113`；本 crate 不解析 `.wil/.wzl`，
/// 只用它承载"笔刷来自哪个库"这件事（状态栏要显示**真实库号**，`edit.rs:499-505`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LibRef {
    /// 所属族。
    pub family: Family,
    /// 族内 0 基索引（= 地图格字段原值；`N > 0` 时对应文件 `{Stem}{N+1}`）。
    pub index: u16,
}

impl LibRef {
    /// 构造地面层定位键（`btTiles`）。
    pub const fn ground(index: u16) -> Self {
        LibRef {
            family: Family::Ground,
            index,
        }
    }

    /// 构造中层定位键（`btsmTiles`）。
    pub const fn middle(index: u16) -> Self {
        LibRef {
            family: Family::Middle,
            index,
        }
    }

    /// 构造表层定位键（`btArea`）。
    pub const fn surface(index: u16) -> Self {
        LibRef {
            family: Family::Surface,
            index,
        }
    }
}

impl fmt::Display for LibRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}#{}", self.family.label(), self.index)
    }
}

/// 层 + 族内索引 → 图库定位键（三族的索引空间互不相通，必须按族构造）。来源：`edit.rs:1352-1358`。
///
/// 阻挡层按**它那个位所在的图号字段**那一族构造：低位阻挡 ⇒ `bk_img` ⇒ 地面族；
/// 高位阻挡 ⇒ `fr_img` ⇒ 表层族。吸管在阻挡层上取到的笔刷因此与在图号层取的一致。
pub const fn lib_ref_of(layer: EditLayer, index: u16) -> LibRef {
    match layer.pick_family() {
        Family::Ground => LibRef::ground(index),
        Family::Middle => LibRef::middle(index),
        Family::Surface => LibRef::surface(index),
    }
}

// ───────────────────────── 帧索引 → 图号 ─────────────────────────

/// 素材面板帧索引（**0 基**）→ 地图格图号（**1 基**）；超出 15 位则 `None`。
/// 来源：`edit.rs:471-477`。
///
/// 上限是 [`SPRITE_MAX`]（`0x7FFF`）而不是 `u16::MAX`：`bk_img` 最高位是低阻挡位
/// （[`COLLISION_BIT`]），图号只能占低 15 位 ⇒ **最大可用帧索引 = 0x7FFE**。
pub fn palette_index_to_sprite(index: u32) -> Option<u16> {
    let img = index.checked_add(1)?;
    if img > u32::from(SPRITE_MAX) {
        return None;
    }
    Some(img as u16)
}

// ───────────────────────── 笔刷 ─────────────────────────

/// 当前笔刷：**某一族**的内容（该族图号 16 位原样 + 14/36 档该族库号字节）。
/// 来源：`edit.rs:486-506`。
///
/// 笔刷**带族**：素材面板可以从中层（`SmTiles`）/ 物件（`Objects`）选帧，写哪个字段则由
/// **当前编辑层**决定 —— 两者不一致时一律拒绝（[`crate::PaintOutcome::LayerMismatch`]）。
///
/// 图号保留最高位：吸管是「把这一格该层的内容原样取下来」，取的时候自作主张清掉最高位，
/// 粘出来的格子就跟源格不是一回事了（`edit.rs:484-485`）。
/// ⚠ 落笔时**笔刷自身必须掩掉 `bit15`**（`img & !0x8000`，见 [`crate::paint`]）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Brush {
    /// 这一笔属于哪一族（地面 / 中层 / 表层）。
    pub family: Family,
    /// 该族**图号**（1 基，整 16 位，含最高位；`0` = 该层没有图）。
    ///
    /// 从素材面板来的一律是「0 基帧索引 + 1」（见 [`palette_index_to_sprite`]），
    /// 从地图吸来的是格子里原本那个字节 —— **两者同义，不许再加/减 1**。
    pub img: u16,
    /// 该族的库号字节（`btTiles` / `btsmTiles` / `btArea`）；`Old12` 档没有这些字节，
    /// 值仍记着（从素材面板选时知道），但**不写回**。
    pub lib_byte: u8,
    /// 笔刷来自哪个图库（`None` = 只知道图号、不知道库号：`Old12` 档吸管就是这种情形）。
    pub lib: Option<LibRef>,
    /// 从哪一格取的（`None` = **不是从地图上取的**）。
    ///
    /// 这一位是"阻挡层要不要拒绝"的判据：图库笔刷（`from == None`）在阻挡层被拒，
    /// 地图吸来的笔刷（`from == Some`）允许落笔（`edit.rs:1404-1412`、`:1619-1627`）。
    pub from: Option<(u16, u16)>,
}

impl Brush {
    /// 图号（去掉最高位后的低 15 位）—— 界面显示的就是它。来源：`edit.rs:510-512`。
    pub const fn sprite(&self) -> u16 {
        self.img & SPRITE_MAX
    }

    /// 该笔刷内容是否带最高位（地表/物件层里它就是阻挡位）。来源：`edit.rs:515-517`。
    pub const fn blocked(&self) -> bool {
        self.img & COLLISION_BIT != 0
    }

    /// 笔刷族的中文层名（状态栏/诊断用）。来源：`edit.rs:520-522`。
    pub const fn family_name(&self) -> &'static str {
        self.family.label()
    }
}

/// 吸管从一格读到的**整格三族**内容（三个图号字段原样）。来源：`edit.rs:529-541`。
///
/// 为什么单独一个类型：吸管读的是整格，但"设为笔刷"只取**当前编辑层**那一个字段
/// （`edit.rs:526-528`）。两件事分开，才不会出现"吸管悄悄把中层图号塞进 `bk_img`"这类脏数据。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PickedCell {
    /// 世界格 X。
    pub x: u16,
    /// 世界格 Y。
    pub y: u16,
    /// 地表图号（`bk_img`，整 16 位）。
    pub bk_img: u16,
    /// 中层图号（`mid_img`，整 16 位）。
    pub mid_img: u16,
    /// 物件图号（`fr_img`，整 16 位）。
    pub fr_img: u16,
}

impl PickedCell {
    /// 按层取图号（三族同义；阻挡层取它那个位所在字段的整 16 位）。来源：`edit.rs:545-551`。
    pub const fn img_of(&self, layer: EditLayer) -> u16 {
        match layer {
            EditLayer::Ground | EditLayer::LowCollision => self.bk_img,
            EditLayer::Middle => self.mid_img,
            EditLayer::Front | EditLayer::HighCollision => self.fr_img,
        }
    }
}

/// 吸管读**整格三族**（三个图号字段原样）；越界 = `None`。来源：`edit.rs:1218-1227`。
///
/// 只读不写：它不改地图、不进撤销栈。
///
/// 为什么要一个**吃外部 `&Map`** 的自由函数（而不是只留
/// [`crate::EditSession::pick_cell`]）：宿主（GUI）把地图放在自己手里，
/// 它的 `pick_cell(&self, map: &Map, …)` 是 `&self` + 外部借用，
/// 没法把地图换进会话（那要 `&mut`）⇒ 抽成纯函数，两边共用同一份判据。
pub fn pick_cell_of(map: &Map, x: u16, y: u16) -> Option<PickedCell> {
    let c = map.cell(x, y)?;
    Some(PickedCell {
        x,
        y,
        bk_img: c.bk_img,
        mid_img: c.mid_img,
        fr_img: c.fr_img,
    })
}

// ───────────────────────── 笔刷尺寸（3..=31 奇数） ─────────────────────────

/// 笔刷尺寸的下限 / 上限 / 步进 —— **与官方滑块逐字一致**。
///
/// 来源：`miredit-gui/src/metrics.rs:175-177`，真值 `CrystalStudio/MainWindow.axaml:6`
/// （`Minimum="3" Maximum="31" Value="3" TickFrequency="2" IsSnapToTickEnabled="True"`）
/// ⇒ 3 / 5 / … / 31 共 15 档，**只有奇数**。
/// 键盘 `[` / `]` 各走 **2**：`MainWindow.axaml.cs:871`
/// （`Math.Clamp(BrushSize.Value + (…? -2 : 2), 3, 31)`）。
pub const BRUSH_MIN: usize = 3;
/// 笔刷尺寸上限。来源同 [`BRUSH_MIN`]。
pub const BRUSH_MAX: usize = 31;
/// 笔刷尺寸步进（`[` / `]` 各走 2，保持奇数）。来源同 [`BRUSH_MIN`]。
pub const BRUSH_STEP: usize = 2;
/// 缺省尺寸 3。来源：`MainWindow.axaml:6` 的 `Value="3"`（`metrics.rs:179`）。
pub const BRUSH_DEFAULT: usize = 3;

/// 把任意尺寸夹进「3..=31 的奇数」—— **唯一的规范化入口**。来源：`metrics.rs:186-199`。
///
/// 先夹范围再抬奇数（`| 1` 的等价写法），于是 `0/1/2` → 3、`32/40` → 31、`4` → 5、`8` → 9。
/// 与 `MapViewport.cs:27` 的 `Math.Clamp(value | 1, 3, 31)` 等价
/// （差别只在"先夹后抬"与"先抬后夹"在边界上的次序，两处都夹到 3..31 的奇数集合内）。
pub const fn clamp_brush_size(n: usize) -> usize {
    let v = if n < BRUSH_MIN { BRUSH_MIN } else { n };
    let v = if v > BRUSH_MAX { BRUSH_MAX } else { v };
    // 抬成奇数：偶数 +1（31 是奇数，+1 只会发生在被上面夹住的偶数上，故不会越界）
    if v % 2 == 0 {
        if v + 1 > BRUSH_MAX {
            BRUSH_MAX
        } else {
            v + 1
        }
    } else {
        v
    }
}

/// 以 `(x, y)` 为中心的 **N×N 落笔格**（已按地图边界裁剪）。来源：`edit.rs:812-827`。
///
/// 形状 = **居中方形**（官方 `MapViewport.cs:476-478` 的双层循环
/// `for iy in -radius..=radius { for ix in -radius..=radius { … if (!Document.Contains(sx, sy)) continue; } }`；
/// 官方工具提示 `CentredBrushFootprintAndtoResize_55DDE2E3` 也写的是「以中心定位笔刷范围」）。
/// 边缘裁剪 = **逐格跳过越界格**（与官方 `continue` 同一判据），不是"把方块整体挪回界内"。
///
/// 为什么要一个**吃显式宽高**的自由函数（而不是只留 [`crate::EditSession::footprint`]）：
/// 宿主的 `footprint(&self, x, y, w, h)` 只拿得到宽高、拿不到地图句柄，
/// 没法把地图换进会话 ⇒ 抽成纯函数，`EditSession::footprint` 也转调它（单一实现）。
pub fn footprint_of(brush_size: usize, x: u16, y: u16, w: u16, h: u16) -> Vec<(u16, u16)> {
    let n = brush_size.max(1);
    let radius = (n / 2) as i32;
    let mut out = Vec::with_capacity(n * n);
    for iy in -radius..=radius {
        for ix in -radius..=radius {
            let sx = i32::from(x) + ix;
            let sy = i32::from(y) + iy;
            if sx < 0 || sy < 0 || sx >= i32::from(w) || sy >= i32::from(h) {
                continue; // 越界格直接跳过（官方 `if (!Document.Contains(sx, sy)) continue;`）
            }
            out.push((sx as u16, sy as u16));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 帧索引 → 图号必须 **+1**，且上限按图号（0x7FFF）判、不按索引判。来源：`edit.rs:1262-1272`。
    #[test]
    fn 帧索引加一且上限按图号判() {
        assert_eq!(palette_index_to_sprite(0), Some(1), "第 0 帧 ⇒ 图号 1（不是 0）");
        assert_eq!(palette_index_to_sprite(9), Some(10));
        assert_eq!(palette_index_to_sprite(903), Some(904));
        // 最大可用帧索引 = 0x7FFE ⇒ 图号 0x7FFF
        assert_eq!(palette_index_to_sprite(0x7FFE), Some(SPRITE_MAX));
        assert_eq!(palette_index_to_sprite(0x7FFF), None, "图号会撞上 0x8000，拒绝");
        assert_eq!(palette_index_to_sprite(u32::MAX), None, "不许溢出");
    }

    /// 尺寸夹到 3..=31 的奇数（逐档对 `metrics.rs:697-730` 的用例）。来源：`metrics.rs:186-199`。
    #[test]
    fn 尺寸夹到三到三十一的奇数() {
        for (ask, want) in [
            (0usize, 3usize),
            (1, 3),
            (2, 3),
            (3, 3),
            (4, 5),
            (7, 7),
            (15, 15),
            (31, 31),
            (32, 31),
            (99, 31),
        ] {
            assert_eq!(clamp_brush_size(ask), want, "clamp_brush_size({ask})");
        }
        // 全部落点都是奇数且在界内
        for n in 0..=200usize {
            let v = clamp_brush_size(n);
            assert!((BRUSH_MIN..=BRUSH_MAX).contains(&v), "{n} → {v} 越界");
            assert_eq!(v % 2, 1, "{n} → {v} 必须是奇数");
        }
        assert_eq!(BRUSH_STEP, 2);
        assert_eq!(BRUSH_DEFAULT, 3);
    }

    /// 笔刷自身的 `sprite()` / `blocked()` 只看位，不改值。来源：`edit.rs:510-517`。
    #[test]
    fn 笔刷取图号与阻挡位() {
        let b = Brush {
            family: Family::Ground,
            img: 0x8000 | 901,
            lib_byte: 4,
            lib: Some(LibRef::ground(4)),
            from: Some((1, 2)),
        };
        assert_eq!(b.sprite(), 901, "图号要去掉阻挡位");
        assert!(b.blocked(), "阻挡位必须一起取下来");
        assert_eq!(b.family_name(), EditLayer::Ground.name());
    }

    /// 族的中文名与层名同源（三族 ↔ 三层一一对应）。来源：`edit.rs:366-368`。
    #[test]
    fn 族名与层名同源() {
        assert_eq!(Family::Ground.label(), EditLayer::Ground.name());
        assert_eq!(Family::Middle.label(), EditLayer::Middle.name());
        assert_eq!(Family::Surface.label(), EditLayer::Front.name());
        assert_eq!(Family::ALL.len(), 3);
        for (i, f) in Family::ALL.iter().enumerate() {
            assert_eq!(f.index(), i);
        }
    }

    /// 阻挡层的吸管族 = 它那个位所在字段的族。来源：`edit.rs:1352-1358`。
    #[test]
    fn 阻挡层吸管族按字段() {
        assert_eq!(lib_ref_of(EditLayer::LowCollision, 3), LibRef::ground(3));
        assert_eq!(lib_ref_of(EditLayer::HighCollision, 3), LibRef::surface(3));
        assert_eq!(lib_ref_of(EditLayer::Middle, 5), LibRef::middle(5));
    }

    /// 吸管整格读三族、按层取其一。来源：`edit.rs:545-551`。
    #[test]
    fn 整格三族按层取() {
        let p = PickedCell {
            x: 1,
            y: 2,
            bk_img: 0x8001,
            mid_img: 0x8002,
            fr_img: 0x8003,
        };
        assert_eq!(p.img_of(EditLayer::Ground), 0x8001);
        assert_eq!(p.img_of(EditLayer::LowCollision), 0x8001);
        assert_eq!(p.img_of(EditLayer::Middle), 0x8002);
        assert_eq!(p.img_of(EditLayer::Front), 0x8003);
        assert_eq!(p.img_of(EditLayer::HighCollision), 0x8003);
    }

    /// **本 crate 的 [`Family`] 与 `mir2_assets::Family` 按序号一一对应**
    /// （防"一边加了变体、另一边没加"这类静默漂移）。
    ///
    /// 为什么允许本 crate 在 **dev-dependencies** 里依赖 `mir2-assets`：
    /// 库本体仍只用 `std` + `mir2-map`（依赖白名单），这条测试只在 `cfg(test)` 下跑；
    /// 不这么做就**没有任何机制**能发现两边变体集合已经不同步（词表不同、序号必须同）。
    #[test]
    fn 族与mir2_assets按序号一一对应() {
        use mir2_assets::Family as 资产族;
        assert_eq!(
            Family::ALL.len(),
            资产族::ALL.len(),
            "变体数必须一致（一边加了变体，另一边也要加）"
        );
        for (i, f) in Family::ALL.iter().enumerate() {
            let a = 资产族::ALL[i];
            assert_eq!(f.index(), i, "本 crate 第 {i} 个族的 index() 必须等于 {i}");
            assert_eq!(a.index(), i, "mir2_assets 第 {i} 个族的 index() 必须等于 {i}");
            assert_eq!(
                f.stem(),
                a.stem(),
                "第 {i} 个族的文件基名必须相同（Tiles / SmTiles / Objects）"
            );
        }
        // 词表**故意不同**（本 crate 面向界面、mir2-assets 面向数据层日志）：
        // 这条断言钉的是"差异是有意的"，不是"差异可以随便漂"。
        assert_ne!(
            Family::Ground.label(),
            资产族::Ground.label(),
            "label() 是两套不同用途的词表，若哪天被合并成一样，本注释与 §五 的裁定要一起改"
        );
    }

    /// 落笔范围：居中 N×N、逐格跳过越界。来源：`edit.rs:812-827`。
    #[test]
    fn 落笔范围居中且裁剪() {
        // 尺寸 3（半径 1）在图中央 ⇒ 9 格，中心在 (5,5)
        let v = footprint_of(3, 5, 5, 20, 20);
        assert_eq!(v.len(), 9);
        assert_eq!(v.first(), Some(&(4, 4)));
        assert_eq!(v.last(), Some(&(6, 6)));
        // 左上角 ⇒ 只剩 2×2 = 4 格（逐格跳过越界格，不把方块整体挪回界内）
        let v = footprint_of(3, 0, 0, 20, 20);
        assert_eq!(v, vec![(0, 0), (1, 0), (0, 1), (1, 1)]);
        // 尺寸 7（半径 3）在左上角 ⇒ 4×4 = 16
        assert_eq!(footprint_of(7, 0, 0, 40, 40).len(), 16);
        // 尺寸 31（半径 15）在左上角 ⇒ 16×16 = 256
        assert_eq!(footprint_of(31, 0, 0, 40, 40).len(), 256);
        // 尺寸 0/1 也按 1 处理（`max(1)`），不会返回空
        assert_eq!(footprint_of(0, 3, 3, 10, 10), vec![(3, 3)]);
        // 图比笔刷还小 ⇒ 全部裁剪掉
        assert!(footprint_of(31, 0, 0, 2, 2).len() == 4);
    }

    /// 吸管整格（吃外部 `&Map`）：越界 `None`，图内三族原样。来源：`edit.rs:1218-1227`。
    #[test]
    fn 吸管整格吃外部地图() {
        let map = crate::testutil::造图(3, 3, mir2_map::CellFormat::New14);
        let c = pick_cell_of(&map, 1, 2).expect("图内格");
        let raw = *map.cell(1, 2).unwrap();
        assert_eq!((c.x, c.y), (1, 2));
        assert_eq!(c.bk_img, raw.bk_img);
        assert_eq!(c.mid_img, raw.mid_img);
        assert_eq!(c.fr_img, raw.fr_img);
        assert!(pick_cell_of(&map, 3, 0).is_none(), "越界不许给 Some");
    }
}
