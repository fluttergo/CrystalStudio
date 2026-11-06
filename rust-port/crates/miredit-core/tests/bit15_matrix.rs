//! 验收门 1 + 2：**`bit15` 保留矩阵**（三族各一条）与**改前必红**对照。
//!
//! 跑法：`cargo test -p miredit-core --test bit15_matrix -- --nocapture`
//!
//! # 门 1（任务书 §六.1）三族各一条
//!
//! | 层 | 原值 | 写图号 10 后 | 说明 |
//! |---|---|---|---|
//! | 地表 `bk_img` | `0x815F` | `0x800A` | 保留 `bit15`（低阻挡） |
//! | 中层 `mid_img` | `0x80DE` | `0x000A` | **不保留**（纯图号） |
//! | 物体 `fr_img` | `0x814D` | `0x800A` | 保留 `bit15`（高位阻挡） |
//!
//! # 门 2（任务书 §六.2）改前必红
//!
//! 同一个输入，用**旧逻辑**（整 16 位覆盖，`set_img(..., b.img)`）跑出来的值是 `0x000A`，
//! 与正确值 `0x800A` 不等 ⇒ 旧逻辑在这条断言上**必然 FAILED**。
//! 本文件把两条公式并排跑一遍并把两个值都打印出来，把"红→绿"这件事固化成常驻用例。
//! （另有一次真实的人工红/绿：改 `paint.rs` 回旧逻辑跑 `cargo test` 得 FAILED，改回来得 ok，
//! 证据见汇报。）

use miredit_core::{
    apply, Command, EditLayer, EditSession, Family, LibRef, COLLISION_BIT, SPRITE_MAX,
};
use mir2_map::{Cell, CellFormat, Map, HEADER_LEN};

/// 按族造一个"图库笔刷"的定位键（库号统一取 3，三族索引空间互不相通）。
fn 库(f: Family) -> LibRef {
    match f {
        Family::Ground => LibRef::ground(3),
        Family::Middle => LibRef::middle(3),
        Family::Surface => LibRef::surface(3),
    }
}

/// 造一张 `w*h` 的 New14 图，全部格清零（`bk_img/mid_img/fr_img` 都是 0）。
fn 空图(w: u16, h: u16) -> Map {
    let mut bytes = vec![0u8; HEADER_LEN + 14 * (w as usize * h as usize)];
    bytes[0..2].copy_from_slice(&w.to_le_bytes());
    bytes[2..4].copy_from_slice(&h.to_le_bytes());
    Map::parse(&bytes).expect("空图应能解析")
}

fn 列优先(h: u16, x: u16, y: u16) -> usize {
    x as usize * h as usize + y as usize
}

/// **门 1**：三族 × `bit15` 保留矩阵（走完整的会话路径：设笔刷 → 落笔 → 读回）。
#[test]
fn 门1_bit15保留矩阵三族各一条() {
    // (层, 原值, 期望值)
    let 用例 = [
        (EditLayer::Ground, 0x815F_u16, 0x800A_u16),
        (EditLayer::Middle, 0x80DE, 0x000A),
        (EditLayer::Front, 0x814D, 0x800A),
    ];

    for (layer, 原值, 期望) in 用例 {
        // 1×1 图：缺省笔刷尺寸 3 在 1×1 上被裁到只剩 1 格 ⇒ "改一格"这件事没有歧义
        let h = 1u16;
        let mut map = 空图(1, 1);
        let idx = 列优先(h, 0, 0);
        layer.set_img(&mut map.cells[idx], 原值);
        let 改前可走 = map.cells[idx].is_walkable();

        // 用字节把这张图交给会话（Document::from_bytes 只读，不落盘）
        let bytes = map.to_bytes();
        let doc = miredit_core::Document::from_bytes(
            // 仅作 `Document` 的路径标签，不落盘 ⇒ 无需进程唯一后缀（若将来改成落盘，必须加 PID）。
            std::path::Path::new(r"I:\__miredit_core_test__\bit15.map"),
            bytes,
        )
        .expect("合成图应能构造文档");
        let mut ed = EditSession::new(doc);
        apply(
            &mut ed,
            Command::SetLayer {
                layer: layer.key().to_string(),
            },
        )
        .expect("层名合法");
        // 帧索引 9 ⇒ 图号 10（纯图号，bit15 = 0）
        miredit_core::set_brush_from_palette(&mut ed, 库(layer.pick_family()), 9);
        let out = apply(&mut ed, Command::Paint { x: 0, y: 0 }).expect("落笔不该报错");
        assert_eq!(out.changed, 1, "{layer:?} 应真的改一格");

        let after = ed.map().cells[idx];
        assert_eq!(
            layer.img_of(&after),
            期望,
            "{layer:?}：0x{原值:04X} 写图号 10 ⇒ 应 0x{期望:04X}（bit15 保留矩阵）"
        );
        assert_eq!(
            layer.img_of(&after) & SPRITE_MAX,
            10,
            "{layer:?} 低 15 位应换成 10"
        );
        if layer.img_of_has_collision_bit() {
            assert_eq!(
                layer.img_of(&after) & COLLISION_BIT,
                原值 & COLLISION_BIT,
                "{layer:?} 的 bit15 前后必须相同"
            );
        } else {
            assert_eq!(
                layer.img_of(&after) & COLLISION_BIT,
                0,
                "{layer:?} 不保留 bit15 ⇒ 写图号后最高位必须是 0（原值里的 0x8000 属异常数据，按纯图号清掉）"
            );
        }
        assert_eq!(
            after.is_walkable(),
            改前可走,
            "{layer:?} 改图号不许改变可走性"
        );
        // 另外两个图号字段一格不动
        for other in EditLayer::IMAGE {
            if other.img_field() != layer.img_field() {
                assert_eq!(
                    other.img_of(&after),
                    0,
                    "{layer:?} 落笔不许动 {other:?} 的字段"
                );
            }
        }
        eprintln!(
            "BIT15_MATRIX layer={} field={} keep={} before=0x{原值:04X} after=0x{:04X} walkable={}",
            layer.key(),
            layer.img_field(),
            layer.img_of_has_collision_bit(),
            layer.img_of(&after),
            after.is_walkable(),
        );
    }
}

/// **门 2**：改前必红 —— 旧逻辑（整 16 位覆盖）与正确逻辑（保留低 15 位）并排跑。
///
/// 旧逻辑的真身是 `edit.rs` 修复前的 `layer.set_img(&mut out, b.img)`：
/// 把笔刷的 16 位原样写进去，`0x815F` 于是变成 `0x000A`（顺手把墙拆了）。
#[test]
fn 门2_改前必红() {
    // 笔刷图号 10（帧索引 9 + 1），与门 1 同一个输入
    let 笔刷图号: u16 = 10;

    for (layer, 原值, 正确, 旧逻辑) in [
        (EditLayer::Ground, 0x815F_u16, 0x800A_u16, 0x000A_u16),
        (EditLayer::Middle, 0x80DE, 0x000A, 0x000A),
        (EditLayer::Front, 0x814D, 0x800A, 0x000A),
    ] {
        // ① 旧逻辑：整 16 位覆盖（不保留）
        let mut 旧 = Cell::default();
        layer.set_img(&mut 旧, 原值);
        layer.set_img(&mut 旧, 笔刷图号);
        assert_eq!(
            layer.img_of(&旧),
            旧逻辑,
            "{layer:?} 旧逻辑（不保留）应得 0x{旧逻辑:04X}"
        );

        // ② 正确逻辑：走 paint（经会话落笔）；1×1 图 ⇒ 落笔恰好 1 格
        let h = 1u16;
        let mut map = 空图(1, 1);
        layer.set_img(&mut map.cells[列优先(h, 0, 0)], 原值);
        let doc = miredit_core::Document::from_bytes(
            // 仅作 `Document` 的路径标签，不落盘 ⇒ 无需进程唯一后缀（若将来改成落盘，必须加 PID）。
            std::path::Path::new(r"I:\__miredit_core_test__\bit15_redgreen.map"),
            map.to_bytes(),
        )
        .expect("合成图应能构造文档");
        let mut ed = EditSession::new(doc);
        apply(
            &mut ed,
            Command::SetLayer {
                layer: layer.key().to_string(),
            },
        )
        .expect("层名合法");
        miredit_core::set_brush_from_palette(&mut ed, 库(layer.pick_family()), 9);
        apply(&mut ed, Command::Paint { x: 0, y: 0 }).expect("落笔不该报错");
        let 实得 = layer.img_of(&ed.map().cells[列优先(h, 0, 0)]);
        assert_eq!(实得, 正确, "{layer:?} 正确逻辑应得 0x{正确:04X}");

        // ③ 红/绿判据：两层逻辑在"保留"的两族上必须**不同**；在中层上**相同**（中层本就不保留）
        let 应保留 = layer.img_of_has_collision_bit();
        assert_eq!(
            应保留,
            正确 != 旧逻辑,
            "{layer:?}：保留 bit15 的层必须能区分新旧逻辑（正确 0x{正确:04X} vs 旧 0x{旧逻辑:04X}）"
        );
        if 应保留 {
            assert_ne!(
                正确, 旧逻辑,
                "{layer:?} 旧逻辑必然红：正确 0x{正确:04X} ≠ 旧 0x{旧逻辑:04X}"
            );
        } else {
            assert_eq!(
                正确, 旧逻辑,
                "{layer:?} 中层本就不保留 bit15 ⇒ 新旧逻辑同值（0x{正确:04X}），这条不构成红"
            );
        }
        eprintln!(
            "BIT15_REDGREEN layer={} keep={} before=0x{原值:04X} 正确=0x{正确:04X} 旧逻辑=0x{旧逻辑:04X} 旧逻辑会红={}",
            layer.key(),
            layer.img_of_has_collision_bit(),
            正确 != 旧逻辑,
        );
    }
}

/// 门 1 的补充：**笔刷自身也掩掉 `bit15`**（吸管从带阻挡的格取下来的笔刷不许把阻挡"贴"过去）。
/// 来源：`edit.rs:1458-1461`。
#[test]
fn 笔刷自带bit15不传播() {
    let h = 4u16;
    let mut map = 空图(4, 4);
    // 源格：bk_img = 0x8001（不可走）；目标格：可走
    map.cells[列优先(h, 0, 0)].bk_img = 0x8001;
    let 目标 = 列优先(h, 2, 2);
    assert!(map.cells[目标].is_walkable());
    let doc = miredit_core::Document::from_bytes(
        // 仅作 `Document` 的路径标签，不落盘 ⇒ 无需进程唯一后缀（若将来改成落盘，必须加 PID）。
        std::path::Path::new(r"I:\__miredit_core_test__\bit15_brush.map"),
        map.to_bytes(),
    )
    .expect("合成图应能构造文档");
    let mut ed = EditSession::new(doc);
    apply(&mut ed, Command::Pick { x: 0, y: 0 }).expect("吸管");
    let b = ed.brush().expect("吸到了笔刷");
    assert_eq!(b.img, 0x8001, "吸管确实取到了带阻挡位的源格内容");
    assert!(b.blocked());
    let out = apply(&mut ed, Command::Paint { x: 2, y: 2 }).expect("落笔");
    // 尺寸 3 在 (2,2) 上覆盖 (1..3,1..3) 共 9 格（都在图内）⇒ 全都会从 0 变成图号 1
    assert_eq!(out.changed, 9, "3×3 笔刷在 (2,2) 覆盖 9 格");
    assert_eq!(
        ed.map().cells[目标].bk_img, 0x0001,
        "笔刷的 bit15 不许贴到目标格上"
    );
    assert!(ed.map().cells[目标].is_walkable(), "目标格改完还得可走");
}

/// 门 1 的补充：`Old12` 档只写图号、**不写库号字节**（不凭空补第 13 个字节）。
/// 来源：`edit.rs:56-57`、`:1470-1473`。
#[test]
fn 十二字节档不写库号字节() {
    let w = 1u16;
    let h = 1u16;
    let mut bytes = vec![0u8; HEADER_LEN + 12 * (w as usize * h as usize)];
    bytes[0..2].copy_from_slice(&w.to_le_bytes());
    bytes[2..4].copy_from_slice(&h.to_le_bytes());
    let mut map = Map::parse(&bytes).expect("Old12 空图");
    assert_eq!(map.format, CellFormat::Old12);
    map.cells[列优先(h, 0, 0)].bk_img = 0x815F;
    let doc = miredit_core::Document::from_bytes(
        // 仅作 `Document` 的路径标签，不落盘 ⇒ 无需进程唯一后缀（若将来改成落盘，必须加 PID）。
        std::path::Path::new(r"I:\__miredit_core_test__\old12.map"),
        map.to_bytes(),
    )
    .expect("合成图应能构造文档");
    let 原长 = doc.to_bytes().len();
    let mut ed = EditSession::new(doc);
    miredit_core::set_brush_from_palette(&mut ed, LibRef::ground(3), 9);
    apply(&mut ed, Command::Paint { x: 0, y: 0 }).expect("落笔");
    assert_eq!(ed.map().cells[列优先(h, 0, 0)].bk_img, 0x800A);
    assert_eq!(
        ed.map().to_bytes().len(),
        原长,
        "Old12 档不许凭空长出一个库号字节"
    );
    // 差异只落在目标格 bk_img 的 2 个字节上
    let d = ed.document().diff();
    assert_eq!(d.bytes, 2, "只动 bk_img 的 2 字节：{d:?}");
    assert_eq!(d.cells, 1);
    assert_eq!(d.header, 0, "52 字节头不许动");
}
