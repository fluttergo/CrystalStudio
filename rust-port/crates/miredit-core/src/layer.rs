//! 编辑层（五个）与**字段映射 / `bit15` 保留矩阵**。
//!
//! 抽取自 `miredit-gui/src/edit.rs:127-360`（`EditLayer` 全块）。**语义逐条照搬，未做任何"优化"**。
//!
//! # 一、五个层 = 左侧「图层」面板的五行的落笔目标
//!
//! | 行 | 层 | 落笔改什么 | 来源 |
//! |---|---|---|---|
//! | 0 | [`EditLayer::Ground`] | `bk_img` 的**低 15 位**（图号）+ 14/36 档 `btTiles` | `edit.rs:107` |
//! | 1 | [`EditLayer::Middle`] | `mid_img` 的**低 15 位** + 14/36 档 `btsmTiles` | `edit.rs:108` |
//! | 2 | [`EditLayer::Front`] | `fr_img` 的**低 15 位** + 14/36 档 `btArea` | `edit.rs:109` |
//! | 3 | [`EditLayer::LowCollision`] | **只** `bk_img` 的 `bit15`（`0x8000` = 低阻挡） | `edit.rs:110` |
//! | 4 | [`EditLayer::HighCollision`] | **只** `fr_img` 的 `bit15`（`0x8000` = 高位阻挡） | `edit.rs:111` |
//!
//! 后两行是**阻挡层**（[`EditLayer::is_collision`]）：不是"第 4/5 个图库族"，而是同一批格子的
//! 另外两个位 ⇒ 没有图库族（[`EditLayer::family`] 返回 `None`）、没有库号字节可写
//! （[`EditLayer::lib_field`] 返回 `None`）。
//!
//! # 二、`bit15` 保留矩阵（2026-11-04 裁定，**这是本 crate 最不能漂的一条**）
//!
//! | 层 | 字段 | 保留 `bit15` | 依据 |
//! |---|---|---|---|
//! | 地表 [`EditLayer::Ground`] | `bk_img` | **是**（低阻挡） | `mir2-map/src/cell.rs:205-212` `Cell::is_walkable`；客户端 `client/reference_full/Classes/map/TMap.cpp:339` `CanWalk`；C# `MapViewport.cs:480` |
//! | 物体 [`EditLayer::Front`] | `fr_img` | **是**（高阻挡） | 同上 + C# `LegacyMapReader.cs:158-159`、`MainWindow.axaml.cs:220`、`NativeCoastSupport.cs:161` |
//! | 中层 [`EditLayer::Middle`] | `mid_img` | **否**（纯图号） | C# `Document.cs:12` 的 `MiddleImage` 是纯 `ushort`；`MapViewport.cs:480`/`:525` 直接赋 `image`、`:546` 清 0；且不参与 `is_walkable` |
//!
//! 写图号的统一公式（`edit.rs:1462-1468`）：
//!
//! ```text
//! new = if keep { (cur & 0x8000) | (v & 0x7FFF) } else { v & 0x7FFF }
//! ```
//!
//! 其中"笔刷自身也掩掉 `bit15`"是同一句里的 `v & 0x7FFF`：吸管从带阻挡的格取下来的
//! `Brush::img` 含 `bit15`，若原样 `|` 进去就等于把源格的阻挡"贴"到目标格上（反向拆墙/筑墙）。
//!
//! ⚠ **绝不用 C# 的 `0x20000000` / `0x40000000`**（`MapViewport.cs:23` 的
//! `ActiveCollisionMask`）：那是它内存模型的私有位，写进 `.map` 我们客户端读不懂
//! （`edit.rs:125-126` 同款警告）。
//!
//! # 三、为什么"保留与否"按**字段**定性、不按工具定性
//!
//! 笔刷、填充、拖动、脚本落笔全部经 [`crate::paint`]，所以只在
//! [`EditLayer::img_of_has_collision_bit`] 判一次就四条路径一起生效（`edit.rs:271-273`）。

use mir2_map::Cell;

use crate::brush::Family;

/// 低阻挡位（存在 `bk_img` 最高位）。来源：`edit.rs:385`。
///
/// 语义真值：`mir2-map/src/cell.rs:205-212` 的 `Cell::is_walkable`
/// （`(bk_img & 0x8000) == 0 && (fr_img & 0x8000) == 0` 才可走）与客户端
/// `client/reference_full/Classes/map/TMap.cpp:339` 的 `CanWalk`。
pub const COLLISION_BIT: u16 = 0x8000;

/// 图号上限 **32767**（`bk_img` 的最高位被 [`COLLISION_BIT`] 占用，图号只剩低 15 位）。
///
/// 来源：`edit.rs:392`。三个图号字段（`bk_img`/`mid_img`/`fr_img`）都是同样形态
/// ⇒ 同一个闸门三族共用；素材面板选帧时超过它就拒绝（打不进地图格）。
pub const SPRITE_MAX: u16 = 0x7FFF;

// ── 层名（中文，与 `miredit-gui/src/ui_text.rs:150-154` 逐字相同） ──
//
// 为什么不从 GUI 引：本 crate 不许依赖 `miredit-gui`（那是被抽取方）。
// 文案只此一份副本，改文案时两处必须同步（GUI 迁到本 crate 后这里就是唯一真源）。

/// 行 0 的层名。来源：`miredit-gui/src/ui_text.rs:150`（官方 `GroundTiles_3931E191`）。
pub const NAME_GROUND: &str = "地表图层";
/// 行 1 的层名。来源：`miredit-gui/src/ui_text.rs:151`（官方 `MiddleTiles_E80971A2`）。
pub const NAME_MIDDLE: &str = "中层图层";
/// 行 2 的层名。来源：`miredit-gui/src/ui_text.rs:152`（官方 `Objects_DE5236EE`，无「层」字）。
pub const NAME_FRONT: &str = "物体";
/// 行 3 的层名。来源：`miredit-gui/src/ui_text.rs:153`（官方 `LowCollisions_CC739D3F`）。
pub const NAME_LOW_COLLISION: &str = "低位阻挡";
/// 行 4 的层名。来源：`miredit-gui/src/ui_text.rs:154`（官方 `HighCollisions_830FB039`）。
pub const NAME_HIGH_COLLISION: &str = "高位阻挡";

/// 三个图号字段名（ASCII，与 `mir2_map::Cell` 的字段同名）。来源：`edit.rs:236-238`
/// ⇒ `miredit-gui/src/ui_text.rs:467` 的 `LAYER_IMG_FIELDS`。
pub const LAYER_IMG_FIELDS: [&str; 3] = ["bk_img", "mid_img", "fr_img"];

/// 三个库号字段名。来源：`edit.rs:245-247` ⇒ `miredit-gui/src/ui_text.rs:469`。
pub const LAYER_LIB_FIELDS: [&str; 3] = ["btTiles", "btsmTiles", "btArea"];

/// **当前编辑层**：决定「落笔写哪个字段」（见模块文档第一节）。
///
/// 抽取自 `edit.rs:127-140`，变体顺序 = 面板行序 = [`EditLayer::ALL`] 下标。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum EditLayer {
    /// 地表层：写 `bk_img`（+ 14/36 档 `btTiles`）。`edit.rs:131`。
    #[default]
    Ground,
    /// 中间层：写 `mid_img`（+ 14/36 档 `btsmTiles`）。`edit.rs:133`。
    Middle,
    /// 物件层：写 `fr_img`（+ 14/36 档 `btArea`）。`edit.rs:135`。
    Front,
    /// 低位阻挡：**只**写 `bk_img` 的 `0x8000` 位（置位 = 不可走）。`edit.rs:137`。
    LowCollision,
    /// 高位阻挡：**只**写 `fr_img` 的 `0x8000` 位（置位 = 不可走）。`edit.rs:139`。
    HighCollision,
}

impl EditLayer {
    /// 五个层（顺序 = 左侧面板行序）。来源：`edit.rs:144-150`。
    pub const ALL: [EditLayer; 5] = [
        EditLayer::Ground,
        EditLayer::Middle,
        EditLayer::Front,
        EditLayer::LowCollision,
        EditLayer::HighCollision,
    ];

    /// 三个**按图号工作**的层（吸管/画笔/橡皮的"图号"语义只对它们成立）。来源：`edit.rs:156`。
    ///
    /// 阻挡层不在其中：它们改的是位、不是图号 —— 凡是"写图号"的循环都必须用它，
    /// 用 [`Self::ALL`] 会把阻挡层也套进"图号"语义里（那是错的口径）。
    pub const IMAGE: [EditLayer; 3] = [EditLayer::Ground, EditLayer::Middle, EditLayer::Front];

    /// 在左侧「图层」面板里的行下标（0..=4）。来源：`edit.rs:159-167`。
    pub const fn row(self) -> usize {
        match self {
            EditLayer::Ground => 0,
            EditLayer::Middle => 1,
            EditLayer::Front => 2,
            EditLayer::LowCollision => 3,
            EditLayer::HighCollision => 4,
        }
    }

    /// 行下标 → 编辑层（五行都能当编辑层）；越界返回 `None`。来源：`edit.rs:170-179`。
    pub const fn from_row(i: usize) -> Option<Self> {
        match i {
            0 => Some(EditLayer::Ground),
            1 => Some(EditLayer::Middle),
            2 => Some(EditLayer::Front),
            3 => Some(EditLayer::LowCollision),
            4 => Some(EditLayer::HighCollision),
            _ => None,
        }
    }

    /// 是不是**阻挡层**（只改 `bit15`，不写图号/库号）。来源：`edit.rs:182-184`。
    pub const fn is_collision(self) -> bool {
        matches!(self, EditLayer::LowCollision | EditLayer::HighCollision)
    }

    /// 该层写图号时属于**哪一族**；阻挡层没有族 ⇒ `None`。来源：`edit.rs:190-197`。
    ///
    /// 判"笔刷族与当前层一致"时用它（`None` 表示这一层压根不认族）。
    pub const fn family(self) -> Option<Family> {
        match self {
            EditLayer::Ground => Some(Family::Ground),
            EditLayer::Middle => Some(Family::Middle),
            EditLayer::Front => Some(Family::Surface),
            EditLayer::LowCollision | EditLayer::HighCollision => None,
        }
    }

    /// **吸管**在该层取哪一族的内容（阻挡层取它那个位所在的图号字段那一族）。来源：`edit.rs:204-210`。
    ///
    /// 低位阻挡 ⇒ `bk_img` ⇒ [`Family::Ground`]；高位阻挡 ⇒ `fr_img` ⇒ [`Family::Surface`]。
    /// 于是"在阻挡层吸一格"得到的笔刷与"在地表层吸同一格"完全一致 ——
    /// 吸管取的是**格子内容**，不是"这一层能写什么"。
    pub const fn pick_family(self) -> Family {
        match self {
            EditLayer::Ground | EditLayer::LowCollision => Family::Ground,
            EditLayer::Middle => Family::Middle,
            EditLayer::Front | EditLayer::HighCollision => Family::Surface,
        }
    }

    /// 层名（界面显示用）。来源：`edit.rs:213-215` ⇒ `ui_text.rs:156-162` 的 `LAYER_NAMES`。
    pub const fn name(self) -> &'static str {
        match self {
            EditLayer::Ground => NAME_GROUND,
            EditLayer::Middle => NAME_MIDDLE,
            EditLayer::Front => NAME_FRONT,
            EditLayer::LowCollision => NAME_LOW_COLLISION,
            EditLayer::HighCollision => NAME_HIGH_COLLISION,
        }
    }

    /// 命令行 / 日志 / **网页命令**用的 ASCII 键。来源：`edit.rs:223-231`。
    ///
    /// 为什么另给一个 ASCII 名：`EDIT_RESULT` 这类给脚本 grep 的行里放中文层名，
    /// 会让"按空格/等号切字段"的脚本在不同代码页下出错。与 [`Self::from_key`] 必须往返一致
    /// （有单元测试钉住）。
    pub const fn key(self) -> &'static str {
        match self {
            EditLayer::Ground => "ground",
            EditLayer::Middle => "middle",
            EditLayer::Front => "front",
            EditLayer::LowCollision => "low_collision",
            EditLayer::HighCollision => "high_collision",
        }
    }

    /// [`Self::key`] 的逆映射；**不认识就 `None`**（绝不猜成某一层）。来源：`edit.rs:223-231` 的取值集合。
    ///
    /// 这是 `cmd::Command::SetLayer` 的唯一入口 —— 猜错层名就是把图号写进别的字段。
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "ground" => Some(EditLayer::Ground),
            "middle" => Some(EditLayer::Middle),
            "front" => Some(EditLayer::Front),
            "low_collision" => Some(EditLayer::LowCollision),
            "high_collision" => Some(EditLayer::HighCollision),
            _ => None,
        }
    }

    /// 该层动哪个**图号**字段名（阻挡层 = 它那个位所在的字段）。来源：`edit.rs:234-240`。
    pub const fn img_field(self) -> &'static str {
        match self {
            EditLayer::Ground | EditLayer::LowCollision => LAYER_IMG_FIELDS[0],
            EditLayer::Middle => LAYER_IMG_FIELDS[1],
            EditLayer::Front | EditLayer::HighCollision => LAYER_IMG_FIELDS[2],
        }
    }

    /// 该层写的**库号**字段名；**阻挡层不写库号字节** ⇒ `None`。来源：`edit.rs:243-250`。
    pub const fn lib_field(self) -> Option<&'static str> {
        match self {
            EditLayer::Ground => Some(LAYER_LIB_FIELDS[0]),
            EditLayer::Middle => Some(LAYER_LIB_FIELDS[1]),
            EditLayer::Front => Some(LAYER_LIB_FIELDS[2]),
            EditLayer::LowCollision | EditLayer::HighCollision => None,
        }
    }

    /// 该层是否写库号字节（`Old12` 档一律不写；阻挡层任何档都不写）。来源：`edit.rs:253-255`。
    pub const fn writes_lib_byte(self) -> bool {
        !self.is_collision()
    }

    /// 读该层的图号（整 16 位，含最高位）。来源：`edit.rs:261-267`。
    ///
    /// 阻挡层读的是**它那个位所在的字段**（低位阻挡 = `bk_img`、高位阻挡 = `fr_img`），
    /// 于是"置位/清位"可以直接写成 `img_of(&c) | 0x8000` / `img_of(&c) & !0x8000`。
    pub const fn img_of(self, c: &Cell) -> u16 {
        match self {
            EditLayer::Ground | EditLayer::LowCollision => c.bk_img,
            EditLayer::Middle => c.mid_img,
            EditLayer::Front | EditLayer::HighCollision => c.fr_img,
        }
    }

    /// 该层 [`Self::img_of`] 读到的**图号字段**，其 `bit15` 是否属于碰撞位。
    ///
    /// 这是「写图号时能不能顺手覆盖最高位」的**唯一判据**（`bit15` 保留矩阵，见模块文档第二节）。
    /// 来源：`edit.rs:316-324`；逐层裁定表在 `edit.rs:306-311`。
    ///
    /// - 地表 `bk_img` ⇒ `true`（低位阻挡）；
    /// - 物体 `fr_img` ⇒ `true`（高位阻挡，**绝不是"异常数据"**）；
    /// - 中层 `mid_img` ⇒ `false`（纯图号；原值或笔刷带 `bit15` 属异常数据，按"只写图号"清掉，
    ///   不传播、不保留 —— C# `MiddleImage` 从不含碰撞位）。
    pub const fn img_of_has_collision_bit(self) -> bool {
        match self {
            EditLayer::Ground
            | EditLayer::Front
            | EditLayer::LowCollision
            | EditLayer::HighCollision => true,
            EditLayer::Middle => false,
        }
    }

    /// **只写该层的图号**（其余字段原样）。来源：`edit.rs:327-333`。
    pub fn set_img(self, c: &mut Cell, v: u16) {
        match self {
            EditLayer::Ground | EditLayer::LowCollision => c.bk_img = v,
            EditLayer::Middle => c.mid_img = v,
            EditLayer::Front | EditLayer::HighCollision => c.fr_img = v,
        }
    }

    /// 读该层图号所在字段的**库号字节**（`Old12` 档在 `Cell` 里恒为 0 且不会被写回）。
    /// 来源：`edit.rs:340-346`。
    ///
    /// 阻挡层读的是它那个字段自己的库号（`btTiles` / `btArea`）—— 这是给**吸管**用的：
    /// 在低位阻挡层吸一格，取到的笔刷应当与在地表层吸同一格完全一致。
    /// ⚠ 它与 [`Self::writes_lib_byte`] 是两件事：读得到 ≠ 会写回（阻挡层永不写库号）。
    pub const fn lib_byte_of(self, c: &Cell) -> u8 {
        match self {
            EditLayer::Ground | EditLayer::LowCollision => c.tiles,
            EditLayer::Middle => c.sm_tiles,
            EditLayer::Front | EditLayer::HighCollision => c.area,
        }
    }

    /// **只写该层的库号字节**；阻挡层是**空操作**。来源：`edit.rs:352-359`。
    ///
    /// 调用方仍应先判 `format.has_lib_bytes() && layer.writes_lib_byte()` ——
    /// 这里的空操作只是最后一道保险，不是"可以随便调用"的许可。
    pub fn set_lib_byte(self, c: &mut Cell, v: u8) {
        match self {
            EditLayer::Ground => c.tiles = v,
            EditLayer::Middle => c.sm_tiles = v,
            EditLayer::Front => c.area = v,
            EditLayer::LowCollision | EditLayer::HighCollision => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 键与层往返一致() {
        for l in EditLayer::ALL {
            assert_eq!(EditLayer::from_key(l.key()), Some(l), "{l:?} 往返失败");
            assert_eq!(EditLayer::from_row(l.row()), Some(l));
        }
        assert_eq!(EditLayer::from_key("Ground"), None, "大小写不宽容");
        assert_eq!(EditLayer::from_key(""), None);
        assert_eq!(EditLayer::from_key("lowcollision"), None, "不许模糊匹配");
        assert_eq!(EditLayer::from_row(5), None);
    }

    /// **`bit15` 保留矩阵**（任务书 §四.1）：三族逐条钉死，不许漂。
    #[test]
    fn bit15保留矩阵() {
        // (层, 保留?)
        let 矩阵 = [
            (EditLayer::Ground, true),
            (EditLayer::Front, true),
            (EditLayer::Middle, false),
            (EditLayer::LowCollision, true),
            (EditLayer::HighCollision, true),
        ];
        for (layer, keep) in 矩阵 {
            assert_eq!(
                layer.img_of_has_collision_bit(),
                keep,
                "{layer:?} 的字段语义裁定不许漂"
            );
        }
        // 与"字段归属"一致：地表/低位阻挡 → bk_img；中层 → mid_img；物体/高位阻挡 → fr_img
        for l in EditLayer::ALL {
            let c = Cell {
                bk_img: 0x1111,
                mid_img: 0x2222,
                fr_img: 0x3333,
                ..Cell::default()
            };
            let 应读 = match l {
                EditLayer::Ground | EditLayer::LowCollision => 0x1111,
                EditLayer::Middle => 0x2222,
                EditLayer::Front | EditLayer::HighCollision => 0x3333,
            };
            assert_eq!(l.img_of(&c), 应读, "{l:?} 读错字段");
            // set_img 只改一个字段（另外两个必须原样）
            let mut after = c;
            l.set_img(&mut after, 0xABCD);
            assert_eq!(l.img_of(&after), 0xABCD, "{l:?} 写错字段");
            let 变的字段数 = usize::from(after.bk_img != c.bk_img)
                + usize::from(after.mid_img != c.mid_img)
                + usize::from(after.fr_img != c.fr_img);
            assert_eq!(变的字段数, 1, "{l:?} 只许改一个图号字段");
            assert_eq!(after.tiles, c.tiles);
            assert_eq!(after.sm_tiles, c.sm_tiles);
            assert_eq!(after.area, c.area);
        }
    }

    /// 阻挡层不写库号字节、没有族；图号层反之。来源：`edit.rs:190-255`。
    #[test]
    fn 阻挡层没有族也不写库号() {
        for l in [EditLayer::LowCollision, EditLayer::HighCollision] {
            assert!(l.is_collision());
            assert_eq!(l.family(), None);
            assert_eq!(l.lib_field(), None);
            assert!(!l.writes_lib_byte());
            // 但吸管仍按它那个位所在的字段取族
            assert!(matches!(
                l.pick_family(),
                Family::Ground | Family::Surface
            ));
        }
        for l in EditLayer::IMAGE {
            assert!(!l.is_collision());
            assert!(l.family().is_some());
            assert!(l.lib_field().is_some());
            assert!(l.writes_lib_byte());
        }
    }

    /// 吸管族 = 图号字段所在族（低位阻挡与地表同族、高位阻挡与物体同族）。
    /// 来源：`edit.rs:199-210`。
    #[test]
    fn 吸管族按字段归属() {
        assert_eq!(EditLayer::LowCollision.pick_family(), EditLayer::Ground.pick_family());
        assert_eq!(
            EditLayer::HighCollision.pick_family(),
            EditLayer::Front.pick_family()
        );
    }

    /// 阻挡层写库号是空操作（最后一道保险，不是许可）。来源：`edit.rs:348-359`。
    #[test]
    fn 阻挡层写库号是空操作() {
        let c = Cell {
            tiles: 7,
            sm_tiles: 8,
            area: 9,
            ..Cell::default()
        };
        for l in [EditLayer::LowCollision, EditLayer::HighCollision] {
            let mut got = c;
            l.set_lib_byte(&mut got, 0xFF);
            assert_eq!(got, c, "{l:?} 不许写库号字节");
        }
        // 图号层确实会写（否则上面那条就是空转）
        let mut g = c;
        EditLayer::Ground.set_lib_byte(&mut g, 0xFF);
        assert_eq!(g.tiles, 0xFF);
    }

    /// 层名与 `miredit-gui/src/ui_text.rs:156-162` 逐字相同（改文案必须两处同步）。
    #[test]
    fn 层名与界面文案同源() {
        assert_eq!(EditLayer::Ground.name(), "地表图层");
        assert_eq!(EditLayer::Middle.name(), "中层图层");
        assert_eq!(EditLayer::Front.name(), "物体");
        assert_eq!(EditLayer::LowCollision.name(), "低位阻挡");
        assert_eq!(EditLayer::HighCollision.name(), "高位阻挡");
    }
}
