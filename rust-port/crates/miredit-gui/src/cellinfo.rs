//! 单元格检查器取数：把 `.map` 里的一格 + 资源库里的帧，翻成界面上那六行真实文字。
//!
//! ## 为什么单独一个模块
//! 「面板上显示什么」必须能在**没有窗口**的情况下被测到（`cargo test --lib`），
//! 否则只能靠肉眼看截图判断"数字对不对"。所以取数逻辑与绘制彻底分开：
//! 本模块是**纯函数**（输入 `&Cell` + 帧源，输出字符串），绘制端只管排版。
//!
//! ## 字段与出处（逐条对得上，不猜）
//!
//! | 面板条目 | 取值 | 出处 |
//! |---|---|---|
//! | 阻挡 低 | `bk_img & 0x8000 != 0` | `mir2_map::Cell::is_walkable`（`mir2-map/src/cell.rs:181-188`）|
//! | 阻挡 高 | `fr_img & 0x8000 != 0` | 同上 |
//! | 地表 图号/库号 | `bk_img & 0x7FFF` / `btTiles`（`Cell::tiles`） | `miredit-render/src/compose.rs:98-102`（1-based 帧号，取图 `(img&0x7FFF)-1`）|
//! | 中层 图号/库号 | `mid_img & 0x7FFF` / `btsmTiles`（`Cell::sm_tiles`） | 同上 `:123-127` |
//! | 物体 图号/库号 | `fr_img & 0x7FFF` / `btArea`（`Cell::area`） | 同上 `:148-152` |
//! | 帧尺寸 | 向帧源要该帧的 `width×height` | `mir2_assets::SpriteSource::frame` |
//! | 元数据 | `light` / `door_index` / `door_offset` | `mir2-map/src/cell.rs` 字段表 off 6/7/11 |
//! | 图块动画 | `ani_frame` / `ani_tick` | 同上 off 8/9 |
//!
//! ⚠ **`btArea`（off10）只叫「表层库号」，且只在「物体」行显示一次**（审计 E2，2026-11-04）。
//! 上一版「图块动画」行还有第三列叫「区域」、取的是同一个 `Cell::area` —— 同一个字节在同一面板
//! 里有两个名字，其中一个还是错的。它是什么有四方证据：
//! 客户端 `GameScene.cpp:3468,3510`（`wunit = btArea` → `GetObjs(wunit, …)`）、
//! `map_loader.gd:203-208`（`get_front_lib_index` = off10）、
//! `mir2-assets/src/lib_paths.rs:15`（`Surface ← btArea`）、
//! C# `LegacyMapReader.cs:296` + `Assets.cs:206,208`（`frindex = off + 120` = Objects 家族）。
//! 命名源头在 `mir2-map/src/cell.rs:168-169`（那个 crate 本单不许改），所以**只在 gui 侧改显示名**。
//! `.map` 里没有第三个动画字段（off8/9/10 已被上面的表列全），故「图块动画」如实只留两行。
//!
//! ## 取不到的字段怎么办
//! **一律 `—`**（[`crate::ui_text::VAL_UNKNOWN`]），并且**不解释、不猜**：
//! - `Old12` 档没有 `btTiles`/`btsmTiles` 两个字节 ⇒ 地表/中层的「库号」是 `—`
//!   （不是 `0`：`0` 在这里是"确实用了 0 号库"的意思，两者不能混）；
//! - 帧取不到（库没登记 / 帧号越界 / 空帧）⇒ 「帧」是 `—`；
//! - 该层图号为 0（本来就没图）⇒ 图号照实写 `0`，帧写 `—`。
//!
//! ⚠ 任务书 §三.6 提到的「帧数」**不在 `.map` 里**（`mir2-map` 的格子只有 `ani_frame`
//! 一个字节 = 当前帧序号，没有"总帧数"），`mir2_assets` 也没有暴露每库帧数 ⇒ 本条如实不给，
//! 改用三个真取得到的字段（见上表末行）。

use mir2_assets::{LibRef, SpriteSource};
use mir2_map::{Cell, CellFormat};

use crate::ui_text as tx;

/// 六行值 + 标题行（顺序与 [`tx::INSPECTOR_KEYS`] 一一对应）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CellLines {
    /// 标题行：`单元格 x,y · N 个物体`。
    pub head: String,
    /// 六个值（第 0 条 = 阻挡，第 5 条 = 图块动画）。
    pub values: [String; 6],
}

impl CellLines {
    /// 什么都没悬停时的一屏：标题 `—`，六行模板里所有占位符都填 `—`。
    ///
    /// 注意**仍然走模板**（而不是另写一句"无数据"），这样占位符少填一个
    /// 会在界面上原样露出 `{size}`，一眼就能发现。
    pub fn unknown() -> Self {
        let u = tx::VAL_UNKNOWN;
        // 注意：`area` 不再出现在任何模板里（审计 E2 —— 它是「表层库号」，
        // 已经由 `VAL_OBJECTS_TMPL` 的 `{lib}` 显示一次，不再重复显示）。
        let kv: [(&str, &str); 10] = [
            ("low", u),
            ("high", u),
            ("img", u),
            ("lib", u),
            ("size", u),
            ("light", u),
            ("door", u),
            ("offset", u),
            ("frame", u),
            ("tick", u),
        ];
        Self {
            head: u.to_owned(),
            values: [
                tx::fill(tx::VAL_COLLISION_TMPL, &kv),
                tx::fill(tx::VAL_GROUND_TMPL, &kv),
                tx::fill(tx::VAL_MIDDLE_TMPL, &kv),
                tx::fill(tx::VAL_OBJECTS_TMPL, &kv),
                tx::fill(tx::VAL_METADATA_TMPL, &kv),
                tx::fill(tx::VAL_TILE_ANIMATION_TMPL, &kv),
            ],
        }
    }
}

/// 取一格的六行信息。
///
/// `cell == None`（鼠标不在图上 / 越界）⇒ [`CellLines::unknown`]。
pub fn cell_lines(
    x: u16,
    y: u16,
    cell: Option<&Cell>,
    fmt: CellFormat,
    src: Option<&dyn SpriteSource>,
) -> CellLines {
    let Some(c) = cell else {
        return CellLines::unknown();
    };

    let img = |raw: u16| raw & 0x7FFF; // 最高位是"不可通过"，不参与取图
    let g_img = img(c.bk_img);
    let m_img = img(c.mid_img);
    let f_img = img(c.fr_img);

    // 「N 个物体」= 该格有几层真的带图（图号非 0）——不是猜"物体数"，是数出来的。
    let objects = [g_img, m_img, f_img].iter().filter(|v| **v != 0).count();

    let head = tx::fill(
        tx::INSPECTOR_HEAD_TMPL,
        &[
            ("x", &x.to_string()),
            ("y", &y.to_string()),
            ("n", &objects.to_string()),
        ],
    );

    // 库号：Old12 档根本没有这两个字节 ⇒ 如实 `—`
    let lib_of = |v: u8| -> String {
        if fmt.has_lib_bytes() {
            v.to_string()
        } else {
            tx::VAL_UNKNOWN.to_owned()
        }
    };

    let collision = tx::fill(
        tx::VAL_COLLISION_TMPL,
        &[
            (
                "low",
                if c.bk_img & 0x8000 != 0 {
                    tx::VAL_YES
                } else {
                    tx::VAL_NO
                },
            ),
            (
                "high",
                if c.fr_img & 0x8000 != 0 {
                    tx::VAL_YES
                } else {
                    tx::VAL_NO
                },
            ),
        ],
    );

    let ground = layer_value(
        g_img,
        &lib_of(c.tiles),
        size_of(src, LibRef::ground(u16::from(c.tiles)), g_img),
        tx::VAL_GROUND_TMPL,
    );
    let middle = layer_value(
        m_img,
        &lib_of(c.sm_tiles),
        size_of(src, LibRef::middle(u16::from(c.sm_tiles)), m_img),
        tx::VAL_MIDDLE_TMPL,
    );
    let objects_v = layer_value(
        f_img,
        &c.area.to_string(),
        size_of(src, LibRef::surface(u16::from(c.area)), f_img),
        tx::VAL_OBJECTS_TMPL,
    );

    let meta = tx::fill(
        tx::VAL_METADATA_TMPL,
        &[
            ("light", &c.light.to_string()),
            ("door", &c.door_index.to_string()),
            ("offset", &c.door_offset.to_string()),
        ],
    );
    // 「图块动画」只填帧数/间隔（官方译名；审计 E2）：`area` 是**表层库号**，已经在「物体」行显示过，
    // 这里再显示一遍就是同一字节两个名字（上一版第二个名字还错叫「区域」）。
    let anim = tx::fill(
        tx::VAL_TILE_ANIMATION_TMPL,
        &[
            ("frame", &c.ani_frame.to_string()),
            ("tick", &c.ani_tick.to_string()),
        ],
    );

    CellLines {
        head,
        values: [collision, ground, middle, objects_v, meta, anim],
    }
}

/// 填「图号 / 库号 / 帧尺寸」这一行。
fn layer_value(img: u16, lib: &str, size: Option<(u32, u32)>, tmpl: &str) -> String {
    let size = match size {
        Some((w, h)) => tx::fill(tx::SIZE_TMPL, &[("w", &w.to_string()), ("h", &h.to_string())]),
        None => tx::VAL_UNKNOWN.to_owned(),
    };
    tx::fill(
        tmpl,
        &[("img", &img.to_string()), ("lib", lib), ("size", &size)],
    )
}

/// 向帧源要该帧的像素尺寸；图号为 0 或取不到帧 ⇒ `None`（界面写 `—`）。
fn size_of(
    src: Option<&dyn SpriteSource>,
    lib: LibRef,
    img: u16,
) -> Option<(u32, u32)> {
    if img == 0 {
        return None; // 该层本来没图，不是"缺帧"
    }
    let f = src?.frame(lib, u32::from(img - 1))?; // 1-based 帧号 ⇒ 0-based 索引
    Some((f.width, f.height))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mir2_assets::{Family, Frame};

    /// 假帧源：按族给出固定尺寸，便于断言"帧尺寸是问来的，不是编的"。
    struct 假源;

    impl SpriteSource for 假源 {
        fn frame(&self, lib: LibRef, index: u32) -> Option<Frame> {
            // 地表 96×64（覆盖 2×2 格）；中层 48×32；表层按族给 48×64（高于一格）
            let (w, h) = match lib.family {
                Family::Ground => (96, 64),
                Family::Middle => (48, 32),
                Family::Surface => (48, 64),
            };
            // 帧号 0 故意返回 None：用来验证"取不到帧 ⇒ —"
            if index == 0 {
                return None;
            }
            Some(Frame {
                width: w,
                height: h,
                rgba: vec![0; (w * h) as usize * 4],
                // 本测试只验帧尺寸是"问来的"，与帧锚点无关 ⇒ 其余字段（含 anchor）走 Default，
                // 将来 Frame 再加字段时这里不必再改（见 mir2-assets/src/frame.rs 的 Default 文档）。
                ..Frame::default()
            })
        }
    }

    /// 空悬停：标题与六个值全是 `—`，且模板占位符全部被替换（不残留 `{`）。
    #[test]
    fn 无悬停时全是破折号() {
        let l = CellLines::unknown();
        assert_eq!(l.head, tx::VAL_UNKNOWN);
        for v in &l.values {
            assert!(!v.contains('{'), "模板占位符没填干净：{v:?}");
            assert!(v.contains(tx::VAL_UNKNOWN), "空值行里应出现 —：{v:?}");
        }
    }

    /// 一个三层齐全、带阻挡位的格：逐字段核对。
    #[test]
    fn 三层齐全的格逐字段正确() {
        let c = Cell {
            bk_img: 0x8000 | 7,  // 低位阻挡 + 图号 7
            mid_img: 3,          // 图号 3
            fr_img: 0x8000 | 9,  // 高位阻挡 + 图号 9
            door_index: 2,
            door_offset: 5,
            ani_frame: 4,
            ani_tick: 6,
            area: 11,   // 表层库号
            light: 13,
            tiles: 1,   // 地表库号
            sm_tiles: 2, // 中层库号
            tail: [0; 22],
        };
        let l = cell_lines(380, 310, Some(&c), CellFormat::New14, Some(&假源));
        assert_eq!(l.head, "单元格 380,310 · 3 个物体");
        assert_eq!(l.values[0], "低位阻挡有 ·\n高位阻挡有");
        assert_eq!(l.values[1], "图号 7\n库号 1\n帧 96×64");
        assert_eq!(l.values[2], "图号 3\n库号 2\n帧 48×32");
        // 表层：帧号 9（1-based）⇒ 索引 8 ≠ 0 ⇒ 取到 48×64；`btArea` 叫「表层库号」
        assert_eq!(l.values[3], "图号 9\n表层库号 11\n帧 48×64");
        assert_eq!(l.values[4], "光照 13 · 门 2\n偏移 5");
        // 官方把 Frames/Tick 译作「帧数/间隔」（`0Frames1Tick2_DFC23F91`），照抄
        assert_eq!(l.values[5], "帧数 4 ·\n间隔 6");
    }

    /// **审计 E2**：`btArea`（off10）不许再叫「区域」，也不许被两个条目各显示一次。
    ///
    /// 这条是"必红"级回归：旧实现的 `values[5]` 是 `帧序 4 ·\n节拍 6 ·\n区域 11`（旧称），
    /// 其中 `11` 与 `values[3]` 的「库号 11」是**同一个字节**（`Cell::area`），
    /// 而且在同一个面板里被叫成了「区域」。任一条恢复旧行为，下面的断言就失败。
    #[test]
    fn 表层库号不许叫区域也不许重复显示() {
        let c = Cell {
            bk_img: 7,
            fr_img: 9,
            ani_frame: 4,
            ani_tick: 6,
            area: 40, // ← 表层资源库号（`btArea`），**唯一**语义
            tiles: 1,
            sm_tiles: 2,
            ..Default::default()
        };
        let l = cell_lines(1, 2, Some(&c), CellFormat::New14, Some(&假源));

        // ① 叫对名字：物体行必须点名「表层库号」
        assert!(
            l.values[3].contains("表层库号 40"),
            "btArea 必须显示为「表层库号」：{:?}",
            l.values[3]
        );
        // ② 不许再出现「区域」这个词（错标签）
        for v in &l.values {
            assert!(!v.contains("区域"), "面板里不许再出现「区域」：{v:?}");
        }
        // ③ 同一个字节只许出现一次：`area=40` 只能在物体行出现
        let hits = l.values.iter().filter(|v| v.contains("40")).count();
        assert_eq!(hits, 1, "0x0A 这一个字节只许显示一次，实际 {} 处", hits);
        // ④ 「图块动画」只剩两个真字段（官方译名：帧数 / 间隔）
        assert_eq!(l.values[5], "帧数 4 ·\n间隔 6");
    }

    /// **文案表里也不许再有「区域」**（防它从别的常量里爬回来）。
    ///
    /// 验收门要求「全 gui crate grep 区域 为 0（注释除外）」；这条把它变成一条能跑的门禁。
    #[test]
    fn 文案表里没有区域二字() {
        for (name, text) in tx::ALL {
            assert!(
                !text.contains("区域"),
                "文案 {name} 里还有「区域」（btArea 是表层库号，不是区域）：{text:?}"
            );
        }
    }

    /// 阻滞位为 0、图号为 0 的空格：图号照实写 0，帧写 `—`（不写尺寸）。
    #[test]
    fn 空格照实写零不写尺寸() {
        let c = Cell::default();
        let l = cell_lines(1, 2, Some(&c), CellFormat::New14, Some(&假源));
        assert_eq!(l.head, "单元格 1,2 · 0 个物体");
        assert_eq!(l.values[0], "低位阻挡无 ·\n高位阻挡无");
        assert_eq!(l.values[1], "图号 0\n库号 0\n帧 —");
    }

    /// Old12 档没有库号字节 ⇒ 库号必须是 `—`（**不能是 0**，那是"0 号库"的意思）。
    #[test]
    fn old12档库号为空() {
        let c = Cell {
            bk_img: 2, // 2（1-based）⇒ 索引 1 ≠ 0 ⇒ 假源给 96×64
            ..Default::default()
        };
        let l = cell_lines(0, 0, Some(&c), CellFormat::Old12, Some(&假源));
        assert_eq!(l.values[1], "图号 2\n库号 —\n帧 96×64");
    }

    /// 帧源缺失（资源库装载失败）⇒ 帧尺寸全是 `—`，其它字段照常。
    #[test]
    fn 没有帧源时尺寸为空() {
        let c = Cell {
            bk_img: 1,
            tiles: 3,
            ..Default::default()
        };
        let l = cell_lines(0, 0, Some(&c), CellFormat::New14, None);
        assert_eq!(l.values[1], "图号 1\n库号 3\n帧 —");
    }

    /// 帧号越界/空帧（帧源返回 None）⇒ 帧也是 `—`。
    #[test]
    fn 取不到帧时尺寸为空() {
        let c = Cell {
            bk_img: 1, // 1-based ⇒ 索引 0 ⇒ 假源故意返回 None
            ..Default::default()
        };
        let l = cell_lines(0, 0, Some(&c), CellFormat::New14, Some(&假源));
        assert_eq!(l.values[1], "图号 1\n库号 0\n帧 —");
    }
}
