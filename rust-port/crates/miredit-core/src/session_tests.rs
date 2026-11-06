use super::*;
use crate::brush::Family;
use crate::layer::COLLISION_BIT;
use crate::testutil::{造图, 造文档};
use mir2_map::CellFormat;

fn 会话(w: u16, h: u16, fmt: CellFormat) -> EditSession {
    EditSession::new(造文档(w, h, fmt))
}

/// 缺省状态：画笔 / 地表层 / 尺寸 3 / 无笔刷 / 不脏。来源：`edit.rs:634-656`。
#[test]
fn 缺省状态() {
    let ed = 会话(3, 3, CellFormat::New14);
    assert_eq!(ed.tool(), Tool::Brush);
    assert_eq!(ed.layer(), EditLayer::Ground);
    assert_eq!(ed.brush_size(), 3);
    assert_eq!(ed.brush(), None);
    assert!(!ed.dirty());
    assert_eq!(ed.undo_len(), 0);
    assert_eq!(ed.pos(), 0);
    assert_eq!(ed.saved_pos(), 0);
}

/// 吸管取到图号与库号（整 16 位含阻挡位）。来源：`edit.rs:2138-2155`。
#[test]
fn 吸管取到图号与库号() {
    let mut ed = 会话(3, 3, CellFormat::New14);
    {
        let map = &mut ed.document_mut().view_mut();
        let i = crate::document::index_of(map, 1, 2).unwrap();
        map.cells[i].bk_img = 0x8000 | 901;
        map.cells[i].tiles = 4;
    }
    let b = ed.pick_at(1, 2).expect("图内格应能取");
    assert_eq!(b.family, Family::Ground, "缺省当前层是地表层 ⇒ 笔刷族=地面");
    assert_eq!(b.img, 0x8000 | 901);
    assert_eq!(b.sprite(), 901, "图号要去掉阻挡位");
    assert_eq!(b.lib_byte, 4);
    assert!(b.blocked(), "阻挡位必须一起取下来");
    assert_eq!(b.from, Some((1, 2)));
    assert_eq!(ed.brush(), Some(b));
    // 吸管不改地图、不进撤销栈、不动脏标记
    assert_eq!(ed.undo_len(), 0);
    assert!(!ed.dirty());
}

/// 越界吸管：拿不到笔刷，也不改已有笔刷（不静默清空）。来源：`edit.rs:2159-2168`。
#[test]
fn 越界吸管不动笔刷() {
    let mut ed = 会话(2, 2, CellFormat::Old12);
    assert_eq!(ed.pick_at(9, 9), None);
    assert_eq!(ed.brush(), None);
    ed.pick_at(0, 0);
    let before = ed.brush();
    assert_eq!(ed.pick_at(5, 5), None);
    assert_eq!(ed.brush(), before);
}

/// `Old12` 档吸管取不到库号（`lib == None`），但图号照取。来源：`edit.rs:1241-1246`。
#[test]
fn 十二字节档吸管不知道库号() {
    let mut ed = 会话(2, 2, CellFormat::Old12);
    let b = ed.pick_at(0, 0).unwrap();
    assert_eq!(b.lib, None, "Old12 档真的不知道库号");
    assert!(b.img > 0);
}

/// **改前必红**（任务书 §六.2）：把旧逻辑（整 16 位覆盖）与正确逻辑放在一起比。
///
/// 旧实现 `set_img(..., b.img)` 把整 16 位覆盖 ⇒ `0x815F → 0x000A`
/// （等于"改个地表顺手拆墙"）。本用例在旧逻辑下必须 FAILED —— 见 `tests/bit15_matrix.rs`
/// 里"旧逻辑对照"那条（用同一组输入跑出错误值，钉住"公式必须是保留式"）。
#[test]
fn 画笔写图号保留地表bit15() {
    let mut ed = 会话(3, 3, CellFormat::New14);
    let idx = {
        let map = &mut ed.document_mut().view_mut();
        let idx = crate::document::index_of(map, 1, 1).unwrap();
        map.cells[idx].bk_img = 0x815F;
        idx
    };
    assert!(!ed.map().walkable(1, 1), "带 bit15 的格本来不可走");
    ed.set_layer(EditLayer::Ground);
    // 帧索引 9 ⇒ 图号 10（纯图号，bit15 = 0）
    ed.set_brush_from_palette(LibRef::ground(3), 9)
        .expect("合法帧应能设为笔刷");
    assert_eq!(ed.apply_once(1, 1), 1, "应真的改一格");
    assert_eq!(
        ed.map().cells[idx].bk_img,
        0x800A,
        "bit15 必须保留：0x815F 写图号 10 应得 0x800A"
    );
    assert_eq!(ed.map().cells[idx].bk_img & crate::SPRITE_MAX, 10);
    assert!(!ed.map().walkable(1, 1), "改图号不许改变可走性");
}

/// 族不匹配 ⇒ 拒绝且一字节不动，并给出原因。来源：`edit.rs:2938-2971`。
#[test]
fn 族不匹配拒绝且一字节不动() {
    let mut ed = 会话(4, 4, CellFormat::New14);
    ed.set_layer(EditLayer::Front);
    ed.set_brush_from_palette(LibRef::ground(3), 9).unwrap(); // 地表笔刷配物体层
    let before = ed.map().to_bytes();
    assert_eq!(ed.apply_once(1, 1), 0, "必须拒绝");
    assert_eq!(ed.map().to_bytes(), before, "一字节都不许动");
    assert!(ed.note().is_some(), "拒绝必须说原因");
    assert!(!ed.dirty(), "没改动就不该脏");
    assert_eq!(ed.undo_len(), 0);
    // 换层后提示自动重算（同族 ⇒ 清掉）
    ed.set_layer(EditLayer::Ground);
    assert_eq!(ed.note(), None, "族匹配后提示必须消失");
}

/// 阻挡层拒绝图库笔刷、接受地图吸来的笔刷。来源：`edit.rs:3296-3343`。
#[test]
fn 阻挡层笔刷闸门() {
    let mut ed = 会话(4, 4, CellFormat::New14);
    ed.set_layer(EditLayer::LowCollision);
    // 图库笔刷 ⇒ 拒绝
    ed.set_brush_from_palette(LibRef::ground(3), 9).unwrap();
    assert_eq!(ed.apply_once(1, 1), 0);
    assert!(ed.note().unwrap().contains("图库"));
    // 地图吸来的笔刷 ⇒ 允许（只置位）
    ed.pick_at(0, 0);
    assert_eq!(ed.apply_once(1, 1), 1, "地图吸来的笔刷允许落笔");
    assert_eq!(ed.map().cells[crate::document::index_of(ed.map(), 1, 1).unwrap()].bk_img & COLLISION_BIT, COLLISION_BIT);
}

/// 尺寸 3 / 7 / 15 单点落笔 = N×N 格，撤销后逐字节还原。来源：`edit.rs:3469-3490`。
#[test]
fn 尺寸三档单点覆盖格数() {
    for (size, want) in [(3usize, 9usize), (7, 49), (15, 225)] {
        let mut ed = 会话(40, 40, CellFormat::New14);
        ed.set_tool(Tool::Brush);
        ed.pick_at(0, 0);
        ed.set_brush_size(size);
        assert_eq!(ed.paint_stroke_once(20, 20), want, "尺寸 {size} 单点应改 {want} 格");
        assert_eq!(ed.undo_len(), 1, "一次落笔 = 一步");
        let orig = 造图(40, 40, CellFormat::New14).to_bytes();
        assert_eq!(ed.undo(), want);
        assert_eq!(ed.map().to_bytes(), orig, "尺寸 {size} 撤销后必须逐字节还原");
    }
}

/// 边缘裁剪：中心落在角上时只落图内那几格。来源：`edit.rs:3492-3518`。
#[test]
fn 尺寸在边缘正确裁剪() {
    for (size, want) in [(7usize, 16usize), (15, 64), (31, 256)] {
        let mut ed = 会话(40, 40, CellFormat::New14);
        ed.pick_at(20, 20);
        ed.set_brush_size(size);
        assert_eq!(ed.paint_stroke_once(0, 0), want, "尺寸 {size} 左上角应只剩 {want} 格");
    }
}

/// 拖动整条轨迹按尺寸落、整条轨迹 = 一步。来源：`edit.rs:3520-3531`。
#[test]
fn 拖动整条轨迹按尺寸落() {
    let mut ed = 会话(40, 40, CellFormat::New14);
    ed.pick_at(0, 0);
    ed.set_brush_size(3);
    assert_eq!(ed.paint_stroke(Some((10, 10)), (12, 10)), 15, "3 格轨迹 × 3×3 ⇒ 5×3");
    assert_eq!(ed.undo_len(), 1, "整条轨迹 = 一步");
}

/// Bresenham 插值：含首尾、不重复、跨度正确。来源：`edit.rs:2495-2507`。
#[test]
fn 划线插值覆盖路径() {
    assert_eq!(line_cells((0, 0), (2, 0)), vec![(0, 0), (1, 0), (2, 0)]);
    assert_eq!(line_cells((3, 3), (3, 3)), vec![(3, 3)]);
    let diag = line_cells((0, 0), (3, 3));
    assert_eq!(diag.len(), 4);
    assert_eq!(diag.first(), Some(&(0, 0)));
    assert_eq!(diag.last(), Some(&(3, 3)));
    let rev = line_cells((5, 5), (2, 5));
    assert_eq!(rev.first(), Some(&(5, 5)));
    assert_eq!(rev.last(), Some(&(2, 5)));
}

/// 脏标记：改一格变脏 → 保存后干净 → 撤销又脏 → 重做回干净。
/// 判据来源：`edit.rs:2376-2394`；且必须与 `UndoStack::dirty()` 一致（单一真值）。
#[test]
fn 脏标记随保存与撤销变化() {
    let mut ed = 会话(2, 2, CellFormat::Old12);
    assert!(!ed.dirty(), "刚打开时不该脏");
    {
        let map = &mut ed.document_mut().view_mut();
        map.cells[0].bk_img = 3;
    }
    ed.pick_at(0, 0);
    ed.apply_once(1, 1);
    assert!(ed.dirty(), "改过就该脏");
    ed.mark_saved();
    ed.note_save_ok();
    assert!(!ed.dirty(), "保存后必须变干净");
    assert_eq!(ed.save_error(), None);
    ed.undo();
    assert!(ed.dirty(), "撤销到保存点之前又该脏");
    ed.redo();
    assert!(!ed.dirty(), "重做回保存点应重新变干净");
}

/// 未结算的笔画也算脏（否则关窗不提示、字节静默丢失）。来源：`edit.rs:2458-2470`。
#[test]
fn 未结算笔画也算脏() {
    let mut ed = 会话(3, 3, CellFormat::New14);
    {
        let map = &mut ed.document_mut().view_mut();
        map.cells[0].bk_img = 77;
    }
    ed.pick_at(0, 0);
    assert!(!ed.dirty());
    ed.begin();
    assert_eq!(ed.touch(2, 2), 1);
    assert!(ed.dirty(), "笔画还没结算也必须算脏");
    assert_eq!(ed.undo_len(), 0);
    assert_eq!(ed.end(), 1);
    assert_eq!(ed.undo_len(), 1);
}

/// 填充只填同号连通块；地表按 step=2 只碰偶偶格。来源：`edit.rs:3541-3602`。
#[test]
fn 填充只填同号连通块() {
    let mut ed = 会话(10, 10, CellFormat::New14);
    ed.set_layer(EditLayer::Ground);
    {
        let map = &mut ed.document_mut().view_mut();
        for c in map.cells.iter_mut() {
            c.bk_img = 9;
            c.tiles = 3;
        }
        for y in 0..4u16 {
            for x in 0..4u16 {
                let i = crate::document::index_of(map, x, y).unwrap();
                map.cells[i].bk_img = 7;
            }
        }
        // 造一支"图号 1、库号 3"的地表笔刷：临时改一格吸走，稍后再改回去
        let i99 = crate::document::index_of(map, 9, 9).unwrap();
        map.cells[i99].bk_img = 1;
    }
    ed.pick_at(9, 9).expect("图内格应能取");
    {
        let map = &mut ed.document_mut().view_mut();
        let i99 = crate::document::index_of(map, 9, 9).unwrap();
        map.cells[i99].bk_img = 9;
    }
    let before = ed.map().to_bytes();
    // 地表 step=2 ⇒ 4×4 的块里只有 (0,0)/(0,2)/(2,0)/(2,2) 四个偶偶格
    assert_eq!(ed.fill(0, 0), FillOutcome::Filled(4));
    let after = ed.map().to_bytes();
    assert_ne!(after, before);
    // 奇偶填充位一格都不许动
    for (x, y) in [(1u16, 0u16), (0, 1), (1, 1), (3, 3)] {
        let i = crate::document::index_of(ed.map(), x, y).unwrap();
        assert_eq!(ed.map().cells[i].bk_img, 7, "({x},{y}) 是填充位，不许动");
    }
    // 块外一格不动
    let i = crate::document::index_of(ed.map(), 5, 5).unwrap();
    assert_eq!(ed.map().cells[i].bk_img, 9);
    assert_eq!(ed.last_fill(), Some(FillRecord { cells: 4, layer: EditLayer::Ground }));
}

/// 填充撤销一步逐字节还原。来源：`edit.rs:3617-3644`。
#[test]
fn 填充撤销一步逐字节还原() {
    let mut ed = 会话(10, 10, CellFormat::New14);
    ed.set_layer(EditLayer::Ground);
    {
        let map = &mut ed.document_mut().view_mut();
        for c in map.cells.iter_mut() {
            c.bk_img = 9;
            c.tiles = 3;
        }
        let i99 = crate::document::index_of(map, 9, 9).unwrap();
        map.cells[i99].bk_img = 1;
    }
    ed.pick_at(9, 9).expect("图内格应能取");
    let orig = ed.map().to_bytes();
    let n = ed.fill(0, 0).cells();
    assert!(n > 0);
    assert_eq!(ed.undo_len(), 1, "整次填充 = 一步");
    assert_eq!(ed.undo(), n);
    assert_eq!(ed.map().to_bytes(), orig, "撤销后必须逐字节还原");
    assert!(!ed.dirty());
}

/// 填充物体层被拒；没笔刷被拒；越界被拒 —— 三种都必须有原因、且一字节不动。
/// 来源：`edit.rs:3734-3763`。
#[test]
fn 填充三种拒绝() {
    let mut ed = 会话(6, 6, CellFormat::New14);
    let before = ed.map().to_bytes();
    // 物体层
    ed.set_layer(EditLayer::Front);
    ed.set_brush_from_palette(LibRef::surface(0), 0).unwrap();
    assert_eq!(ed.fill(1, 1), FillOutcome::UnsupportedLayer);
    assert!(ed.note().is_some());
    // 没笔刷
    let mut ed2 = 会话(6, 6, CellFormat::New14);
    ed2.set_layer(EditLayer::Ground);
    assert_eq!(ed2.fill(1, 1), FillOutcome::NoBrush);
    assert!(ed2.note().unwrap().contains("没有笔刷"));
    // 越界
    let mut ed3 = 会话(6, 6, CellFormat::New14);
    ed3.set_layer(EditLayer::Middle);
    ed3.set_brush_from_palette(LibRef::middle(0), 0).unwrap();
    assert_eq!(ed3.fill(99, 99), FillOutcome::OutOfMap);
    assert!(ed3.note().unwrap().contains("99"));
    assert_eq!(ed.map().to_bytes(), before, "拒绝时一格都不许动");
    assert!(!ed.dirty() && !ed2.dirty() && !ed3.dirty());
}

/// 填充阻挡层只置位（不清位）。来源：`edit.rs:3707-3733`。
#[test]
fn 填充阻挡层只置位() {
    let mut ed = 会话(6, 6, CellFormat::New14);
    ed.set_layer(EditLayer::LowCollision);
    let n = ed.fill(0, 0).cells();
    assert_eq!(n, 36, "整张 6×6 都可走 ⇒ 全部置位（step=1）");
    for c in ed.map().cells.iter() {
        assert_eq!(c.bk_img & COLLISION_BIT, COLLISION_BIT);
    }
    // 再填一次 ⇒ 已是目标值
    assert_eq!(ed.fill(0, 0), FillOutcome::AlreadyFilled);
}

/// `paint_note` 在 0 格时必须给出具体原因（禁静默）。任务书 §七。
#[test]
fn 落笔零格必有原因() {
    // 越界
    let mut ed = 会话(4, 4, CellFormat::Old12);
    ed.pick_at(0, 0);
    assert_eq!(ed.paint_stroke_once(99, 99), 0);
    assert!(ed.paint_note(99, 99).contains("地图外"));
    // 没笔刷
    let mut ed2 = 会话(4, 4, CellFormat::Old12);
    assert_eq!(ed2.paint_stroke_once(1, 1), 0);
    assert!(ed2.paint_note(1, 1).contains("没有笔刷"));
    // 未接线工具
    let mut ed3 = 会话(4, 4, CellFormat::Old12);
    ed3.pick_at(0, 0);
    ed3.set_tool(Tool::Unwired);
    assert_eq!(ed3.paint_stroke_once(1, 1), 0);
    assert!(ed3.paint_note(1, 1).contains("未接线"));
    // 值没变（整张图同号 ⇒ 落笔前后一模一样）
    let mut ed4 = 会话(4, 4, CellFormat::Old12);
    {
        let map = &mut ed4.document_mut().view_mut();
        for c in map.cells.iter_mut() {
            c.bk_img = 5;
        }
    }
    ed4.pick_at(1, 1);
    assert_eq!(ed4.paint_stroke_once(1, 1), 0);
    assert!(ed4.paint_note(1, 1).contains("相同"));
}

/// 尺寸夹到 3..=31 的奇数，且改尺寸不产生步骤、不动脏标记。来源：`edit.rs:3437-3467`。
#[test]
fn 尺寸不改地图也不进撤销栈() {
    let mut ed = 会话(40, 40, CellFormat::New14);
    for (ask, want) in [(0usize, 3usize), (4, 5), (31, 31), (99, 31)] {
        assert_eq!(ed.set_brush_size(ask), want);
    }
    ed.set_brush_size(7);
    assert_eq!(ed.grow_brush_size(-2), 5);
    assert_eq!(ed.grow_brush_size(2), 7);
    ed.set_brush_size(3);
    assert_eq!(ed.grow_brush_size(-2), 3, "已在下限，`[` 不再减");
    ed.set_brush_size(31);
    assert_eq!(ed.grow_brush_size(2), 31, "已在上限，`]` 不再加");
    assert_eq!(ed.undo_len(), 0);
    assert!(!ed.dirty());
}

/// 切层会把"另一层"的填充回执清掉（否则张冠李戴）。来源：`edit.rs:766-773`。
#[test]
fn 切层清掉别层回执() {
    let mut ed = 会话(6, 6, CellFormat::New14);
    ed.set_layer(EditLayer::LowCollision);
    assert!(ed.fill(0, 0).cells() > 0);
    assert!(ed.last_fill().is_some());
    ed.set_layer(EditLayer::Ground);
    assert_eq!(ed.last_fill(), None, "切到别的层，回执必须作废");
}

/// 从磁盘装载：读不到就 `Err`，不造空文档。
#[test]
fn 装载失败如实报错() {
    let e = EditSession::load(Path::new(r"I:\__no_such_map_miredit_core__\x.map"));
    assert!(e.is_err());
}

/// **印章注册表**：覆盖语义 + 三道护栏（空格表 / 单枚超上限 / 表满）逐条钉住。
///
/// 判据来源：`mir2-core/src/session.rs` 的 [`EditSession::register_stamp`] 文档
/// （单枚上限同 C# `Document.cs:140` 的 1,000,000；表上限是本仓护栏）。
#[test]
fn 印章注册表护栏() {
    let mut ed = 会话(4, 4, CellFormat::New14);
    let 一格 = |bk: u16| crate::stamp::StampCellInput {
        dx: 0,
        dy: 0,
        bk,
        ..Default::default()
    };
    // ① 正常注册
    assert_eq!(ed.register_stamp("甲", vec![一格(1), 一格(2)]).unwrap(), 2);
    assert_eq!(ed.stamp_ids(), vec!["甲"]);
    assert_eq!(ed.stamp_cells("甲").map(|s| s.len()), Some(2));
    assert_eq!(ed.stamp_cells("乙"), None, "未注册 ⇒ None，不猜");
    // ② 同 id 再注册 = 覆盖（不是追加）
    assert_eq!(ed.register_stamp("甲", vec![一格(9)]).unwrap(), 1);
    assert_eq!(ed.stamp_cells("甲").map(|s| s.len()), Some(1), "覆盖后只剩 1 格");
    assert_eq!(ed.stamp_ids().len(), 1, "覆盖不新增 id");
    // ③ 空格表 ⇒ 拒绝，且不动旧内容
    let e = ed.register_stamp("甲", Vec::new()).expect_err("空格表必须拒绝");
    assert!(e.contains("格表是空的"), "{e}");
    assert_eq!(ed.stamp_cells("甲").map(|s| s.len()), Some(1), "失败不许动旧内容");
    // ④ 单枚超上限 ⇒ 拒绝
    let 超大 = vec![crate::stamp::StampCellInput::default(); crate::stamp::STAMP_MAX_CELLS + 1];
    let e = ed.register_stamp("超大", 超大).expect_err("超上限必须拒绝");
    assert!(e.contains("超过单枚上限"), "{e}");
    assert_eq!(ed.stamp_ids(), vec!["甲"], "被拒绝的 id 不许进表");
    // ⑤ 表满 ⇒ 新 id 拒绝；覆盖已有 id 仍可（不占新槽位）
    let mut ed2 = 会话(2, 2, CellFormat::Old12);
    for i in 0..crate::stamp::STAMP_REGISTRY_MAX {
        ed2.register_stamp(format!("s{i}"), vec![一格(1)]).unwrap();
    }
    assert_eq!(ed2.stamp_ids().len(), crate::stamp::STAMP_REGISTRY_MAX);
    let e = ed2.register_stamp("再塞一个", vec![一格(1)]).expect_err("表满必须拒绝");
    assert!(e.contains("注册表已满"), "{e}");
    assert!(e.contains("remove_stamp"), "要指出腾位办法：{e}");
    assert_eq!(
        ed2.register_stamp("s0", vec![一格(7)]).expect("覆盖已有 id 不受表满影响"),
        1
    );
    // ⑥ remove_stamp 腾位后能再塞
    assert!(ed2.remove_stamp("s0"));
    assert!(!ed2.remove_stamp("s0"), "第二次删同一个 id ⇒ false");
    assert_eq!(ed2.register_stamp("新id", vec![一格(1)]).unwrap(), 1);
}

/// 按 id 放置：未注册 ⇒ 拒绝（原因带已注册清单）；注册后 ⇒ 真的放。
#[test]
fn 按id放置() {
    let mut ed = 会话(6, 6, CellFormat::New14);
    // ⚠ 占用闸门（`PlacedStamps.cs:62-63`）要求落点格没有物件（`fr_img & 0x7FFF == 0`），
    // 而 `造图` 夹具每格都带物体图号（`testutil.rs:27` 的 `img.wrapping_add(200)`）
    // ⇒ 先把 (2,2)/(3,2) 两处落点的物体层清空，否则印章会被整枚拒绝。
    // 清空发生在 `原` 快照之前 ⇒ 末尾"撤销回 `原`"的断言仍然成立。
    {
        let map = &mut ed.document_mut().view_mut();
        for (x, y) in [(2u16, 2u16), (3u16, 2u16)] {
            let i = crate::document::index_of(map, x, y).expect("图内格");
            map.cells[i].fr_img = 0;
        }
    }
    let 原 = ed.map().to_bytes();
    let out = ed.place_stamp_by_id(1, 1, "没有这个");
    let reason = out.refused().expect("必须拒绝").to_string();
    assert!(reason.contains("未注册的印章 id"), "{reason}");
    assert!(reason.contains("一个都没注册"), "空表也要说清楚：{reason}");
    assert_eq!(ed.map().to_bytes(), 原, "拒绝时一格都不许动");
    assert_eq!(ed.note(), Some(reason.as_str()), "原因必须同时进 note（状态栏要显示）");
    // 注册后放
    ed.register_stamp(
        "两格",
        vec![
            crate::stamp::StampCellInput { dx: 0, dy: 0, bk: 31, ..Default::default() },
            crate::stamp::StampCellInput { dx: 1, dy: 0, bk: 32, ..Default::default() },
        ],
    )
    .unwrap();
    let out = ed.place_stamp_by_id(2, 2, "两格");
    assert_eq!(out, PlaceOutcome::Placed { cells: 2, skipped: 0 });
    assert_eq!(ed.map().cell(2, 2).unwrap().bk_img, 31);
    assert_eq!(ed.map().cell(3, 2).unwrap().bk_img, 32);
    assert_eq!(ed.undo_len(), 1);
    ed.undo();
    assert_eq!(ed.map().to_bytes(), 原);
}

// ── 复制按图层可见性过滤（2026-11-05，对齐 `RegionCopy.cs:106-107`） ──

/// **① 三层全开**：3×3 选区复制出 9 格，三族图号全在。
///
/// 判据来源：`RegionCopy.cs:107`（`VisibleLayers[0..2]` 原样传进快照）
/// ⇒ 全开时一格都不许丢；复制是纯 UI 动作（不进撤销栈）。
#[test]
fn 复制三层全开抓满三族() {
    let mut ed = 会话(5, 5, CellFormat::New14);
    assert_eq!(ed.visible_layers(), [true; 3], "缺省三层全开");
    assert!(ed.set_selection(Selection::new(1, 1, 3, 3)));
    assert_eq!(ed.copy_selection(), Some((3, 3)));

    let clip = ed.clipboard().expect("复制成功 ⇒ 剪贴板必须有内容");
    assert_eq!((clip.width(), clip.height()), (3, 3));
    assert_eq!(clip.cells().len(), 9, "整块 9 格都要抓（含空格）");
    for (i, c) in clip.cells().iter().enumerate() {
        assert_ne!(c.bk, 0, "第 {i} 格地表图号没抓到：{c:?}");
        assert_ne!(c.mid, 0, "第 {i} 格中层图号没抓到：{c:?}");
        assert_ne!(c.fr, 0, "第 {i} 格物体图号没抓到：{c:?}");
    }
    assert_eq!(ed.undo_len(), 0, "复制不产生撤销步");
}

/// **② 关掉物体层**：剪贴板里 `fr` 全 0，`bk` / `mid` 一个字节都不变。
///
/// 判据来源：`RegionCopy.cs:107` 把可见性定格进快照 + `:78-82` 粘贴时
/// `pasteObjects = objects && …` ⇒ 隐藏层的族**不参与**。
/// 本 crate 的等价做法是复制时就把该族字段填 0（`Clipboard::capture` 的可见性过滤）。
#[test]
fn 复制隐藏物体层只滤物体族() {
    let mut ed = 会话(5, 5, CellFormat::New14);
    assert!(ed.set_selection(Selection::new(1, 1, 3, 3)));
    assert_eq!(ed.copy_selection(), Some((3, 3)));
    let 全开: Vec<_> = ed.clipboard().expect("剪贴板").cells().to_vec();
    assert_eq!(全开.len(), 9);

    // 只关物体层（地表 / 中层仍开）
    ed.set_visible_layers([true, true, false]);
    assert_eq!(ed.copy_selection(), Some((3, 3)));
    let 关物体: Vec<_> = ed.clipboard().expect("剪贴板").cells().to_vec();
    assert_eq!(关物体.len(), 9, "隐藏层不改变抓取格数（空格也抓）");
    for (i, (新, 旧)) in 关物体.iter().zip(&全开).enumerate() {
        // 物体族整族为 0：图号 + 库号 + 门（`RegionCopy.cs:86` 的 `target.Door = source.Door`）
        // + 前景动画（`target.Foreground = source.Foreground` 整体赋值）
        assert_eq!(新.fr, 0, "第 {i} 格：物体层隐藏 ⇒ 物体图号必须为 0");
        assert_eq!(新.fr_lib, 0, "第 {i} 格：物体层隐藏 ⇒ 物体库号必须为 0");
        assert_eq!(新.door, 0, "第 {i} 格：门属于物体层");
        assert_eq!(新.door_offset, 0, "第 {i} 格：门偏移属于物体层");
        assert_eq!(新.ani_frame, 0, "第 {i} 格：前景动画帧属于物体层");
        assert_eq!(新.ani_tick, 0, "第 {i} 格：前景动画节拍属于物体层");
        // 另外两族逐字段不变
        assert_eq!(新.bk, 旧.bk, "第 {i} 格：地表图号不该被物体层的开关影响");
        assert_eq!(新.bk_lib, 旧.bk_lib, "第 {i} 格：地表库号不该受影响");
        assert_eq!(新.mid, 旧.mid, "第 {i} 格：中层图号不该受影响");
        assert_eq!(新.mid_lib, 旧.mid_lib, "第 {i} 格：中层库号不该受影响");
    }

    // 反向各试一次：关地表 ⇒ 只剩中层/物体；关中层 ⇒ 只剩地表/物体
    ed.set_visible_layers([false, true, true]);
    assert_eq!(ed.copy_selection(), Some((3, 3)));
    assert!(
        ed.clipboard().unwrap().cells().iter().all(|c| c.bk == 0 && c.bk_lib == 0),
        "关地表 ⇒ 地表图号与库号必须全 0"
    );
    assert!(
        ed.clipboard().unwrap().cells().iter().all(|c| c.mid != 0 && c.fr != 0),
        "关地表不影响中层/物体"
    );
    ed.set_visible_layers([true, false, true]);
    assert_eq!(ed.copy_selection(), Some((3, 3)));
    assert!(
        ed.clipboard().unwrap().cells().iter().all(|c| c.mid == 0 && c.mid_lib == 0),
        "关中层 ⇒ 中层图号与库号必须全 0"
    );
    assert!(
        ed.clipboard().unwrap().cells().iter().all(|c| c.bk != 0 && c.fr != 0),
        "关中层不影响地表/物体"
    );
    assert_eq!(ed.undo_len(), 0, "复制（含过滤）不产生撤销步");
}

/// **③ 三层全关**：拒绝 + 中文原因；剪贴板保持原样、撤销步不增、地图字节不动。
///
/// 判据来源：`RegionCopy.cs:106`
/// （`if (!VisibleLayers.Any(v => v) && !CollisionVisible) { …"Show at least one layer to copy"… return; }`）。
#[test]
fn 复制三层全关拒绝且不动任何状态() {
    let mut ed = 会话(5, 5, CellFormat::New14);
    assert!(ed.set_selection(Selection::new(1, 1, 3, 3)));
    // 先复制一份"旧剪贴板"：拒绝时它必须原样留着（不静默清空）
    assert_eq!(ed.copy_selection(), Some((3, 3)));
    let 旧剪贴板 = ed.clipboard().expect("旧剪贴板").clone();
    let 旧字节 = ed.map().to_bytes();
    let 旧步数 = ed.undo_len();

    ed.set_visible_layers([false, false, false]);
    assert_eq!(ed.copy_selection(), None, "三层全关必须拒绝");

    let note = ed.note().expect("拒绝必须带中文原因（禁静默）");
    assert!(note.contains("全部隐藏"), "原因要说清是三层全关：{note}");
    assert!(note.contains("RegionCopy.cs:106"), "原因要带源码出处：{note}");
    assert!(note.contains("剪贴板未变"), "要说明剪贴板没被清：{note}");
    assert_eq!(ed.undo_len(), 旧步数, "拒绝不许产生撤销步");
    assert_eq!(ed.map().to_bytes(), 旧字节, "拒绝时地图一个字节都不许动");
    assert_eq!(ed.clipboard(), Some(&旧剪贴板), "拒绝时剪贴板保持原样");
    assert_eq!(ed.visible_layers(), [false; 3], "可见性状态照用户所设保留（拒绝不改状态）");
    assert_eq!(ed.selection(), Some(Selection::new(1, 1, 3, 3)), "选区也不受影响");
}

/// **④ 剪切也按可见性**：关掉物体层再剪切 ⇒ 物体族既没进剪贴板、也**没被清掉**。
///
/// 判据来源：`RegionCopy.cs:108`（`if (cut) … CutSource(…)`）→ `:52/57/61` 的三个
/// `if (ground && …)` / `if (middle && …)` / `if (objects && …)`。
/// 为什么要单独钉：复制按可见性滤、剪切却照旧三族全清 ⇒ 会出现
/// 「该层没进剪贴板但被删掉」的静默数据丢失。
#[test]
fn 剪切按可见性只清可见层() {
    let mut ed = 会话(5, 5, CellFormat::New14);
    assert!(ed.set_selection(Selection::new(1, 1, 3, 3)));
    let 原 = ed.map().to_bytes();
    let 原格 = *ed.map().cell(1, 1).expect("图内格");

    ed.set_visible_layers([true, true, false]); // 只关物体层
    assert_eq!(ed.cut_selection(), Some((3, 3)));

    // 剪贴板：物体族被滤掉（同 ② 的口径）
    let clip = ed.clipboard().expect("剪切成功 ⇒ 剪贴板必须有内容");
    assert!(
        clip.cells().iter().all(|c| c.fr == 0 && c.fr_lib == 0 && c.door == 0),
        "物体层隐藏 ⇒ 剪贴板里物体族必须全 0"
    );
    assert!(
        clip.cells().iter().all(|c| c.bk != 0 && c.mid != 0),
        "可见的地表/中层仍要抓进剪贴板"
    );

    // 地图：可见的两层被清，隐藏的物体层一个字节都不许动
    let 后 = ed.map().cell(1, 1).expect("图内格");
    assert_eq!(后.bk_img & 0x7FFF, 0, "可见的地表层必须被清");
    assert_eq!(后.mid_img & 0x7FFF, 0, "可见的中层必须被清");
    assert_eq!(后.fr_img, 原格.fr_img, "隐藏的物体层不许被清（否则就是静默丢数据）");
    assert_eq!(后.area, 原格.area, "物体库号也不许被清");

    // 整次剪切 = 一步撤销，撤销后逐字节回到原样
    assert_eq!(ed.undo_len(), 1, "剪切 = 一步撤销");
    ed.undo();
    assert_eq!(ed.map().to_bytes(), 原, "撤销要逐字节回到剪切前");
}
