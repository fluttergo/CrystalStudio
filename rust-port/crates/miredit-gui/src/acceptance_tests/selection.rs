//! 门 4（复制粘贴 3×3）+ 门 5（剪切 3×3）+ 门 6（Type5 拒绝，**注入验证**）。
//!
//! 门 4/5 走的是 [`crate::selection_ops::apply`]（= 快捷键处理函数调用的同一个函数），
//! 不是直接调内核 —— 验的是"接上线之后磁盘字节对不对"。
//!
//! 门 6 的注入说明见 [`super`] 模块文档 §三。

use std::collections::BTreeSet;

use mir2_map::Map;

use crate::edit::EditSession;
use crate::save::{self, SaveOutcome};
use crate::selection_ops::{self, Cmd};
use crate::stamp_place::{self, Armed};

use miredit_core::Selection;

use super::common::{
    变化格, 复制地图, 格字节, 差异清单, 找地表3x3, 印章路径, 文档, 头长, 格长, hex,
};

// ═══════════════════════════ 门 4 ═══════════════════════════

/// **门 4**：选 3×3 → `Ctrl+C` → 移到 `(200,200)` → `Ctrl+V` ⇒ 保存后**只有目标 9 格变**、
/// 三族图号与源逐格相同、`bit15` 保留。
///
/// 走的是 [`selection_ops::apply`]（= 快捷键处理函数调用的同一个函数），不是直接调内核。
#[test]
fn 门4_复制粘贴3x3_逐字节核对() {
    let Some((d, 副本, 原字节)) = 复制地图("gate4") else {
        return;
    };
    let mut doc = 文档(&副本);
    let mut ed = EditSession::default();
    let 目标 = Selection::new(200, 200, 202, 202);
    let 原图 = Map::parse(&原字节).expect("原字节应能解析");
    let 高 = 原图.height;
    let Some((sx, sy)) = 找地表3x3(&原图, Some(目标)) else {
        panic!("0.map 上找不到 3×3 全是地表图号的区域 —— 样本变了？");
    };
    let 源 = Selection::new(sx, sy, sx + 2, sy + 2);
    println!("GATE4 源选区={源:?} 目标={目标:?}");

    // ① 设选区 + 复制（Ctrl+C）
    assert!(
        ed.set_selection(&mut doc.doc.as_mut().expect("文档").map, 源),
        "源选区应生效"
    );
    let copy = selection_ops::apply(&mut ed, &mut doc.doc.as_mut().expect("文档").map, Cmd::Copy, None)
        .expect("复制总有回执");
    assert!(!copy.warn, "复制不该被拒：{}", copy.text);
    assert_eq!(copy.text, "已复制 3×3 格");
    assert_eq!(copy.changed, 0, "复制不改地图");
    assert!(!ed.dirty(), "复制不置脏（磁盘上没有要保存的东西）");

    // ② 粘贴到 (200,200)（Ctrl+V，锚点 = 鼠标格）
    let paste = selection_ops::apply(
        &mut ed,
        &mut doc.doc.as_mut().expect("文档").map,
        Cmd::Paste,
        Some((目标.x0, 目标.y0)),
    )
    .expect("有鼠标格就有回执");
    assert!(!paste.warn, "粘贴不该被拒：{}", paste.text);
    assert_eq!(paste.text, "粘贴 3×3 格");
    assert_eq!(paste.changed, 9, "3×3 粘到异值区域 ⇒ 9 格都变");
    assert!(ed.dirty(), "粘贴必须置脏");
    assert_eq!(ed.undo_len(), 1, "整次粘贴 = 一步撤销");

    // ③ 逐格核对三族**图号**。
    //
    // 两条口径必须分开写，否则会写出一个"看起来对、其实假"的断言：
    // 1. 比的是**低 15 位**（= 图号）；`bit15` 是阻挡位，按规则**取自目标格**而非源格，
    //    所以由下面第 ④ 步单独核对；
    // 2. **图号 0 = 该族没有内容 ⇒ 整族不写**（core `stamp.rs` 模块文档 §一 第 3 条，
    //    来源 `PlacedStamps.cs:90-92` 的 `if(a.BackImage!=0){…}`）⇒
    //    源格某族为 0 时，目标格该族**保持原值**（不是被清成 0）。真实地图上物体层很稀疏，
    //    3×3 里大多数格 `fr == 0`，所以这条分支是常态而不是边角。
    // ⚠ 借用一个**作用域**：下面第 ④ 步要把 `&mut doc` 交给 `save_session`
    //    （出字节要 O(1) 对调 `map` 与 `document.view`），长借会被借用检查器拦住。
    {
        let map = &doc.doc.as_ref().expect("文档").map;
        let mut 整族不写 = 0usize;
        for dy in 0..3u16 {
            for dx in 0..3u16 {
                let a = 原图.cell(源.x0 + dx, 源.y0 + dy).expect("源格");
                let t = 原图.cell(目标.x0 + dx, 目标.y0 + dy).expect("目标原格");
                let b = map.cell(目标.x0 + dx, 目标.y0 + dy).expect("目标新格");
                for (族, 源值, 目标原值, 目标新值) in [
                    ("地表", a.bk_img, t.bk_img, b.bk_img),
                    ("中层", a.mid_img, t.mid_img, b.mid_img),
                    ("物体", a.fr_img, t.fr_img, b.fr_img),
                ] {
                    if 源值 & 0x7FFF == 0 {
                        整族不写 += 1;
                        assert_eq!(
                            目标新值 & 0x7FFF,
                            目标原值 & 0x7FFF,
                            "({dx},{dy}) {族}：源格图号为 0 ⇒ 该族不许写，目标应保持原值"
                        );
                    } else {
                        assert_eq!(
                            目标新值 & 0x7FFF,
                            源值 & 0x7FFF,
                            "({dx},{dy}) {族}图号必须与源相同"
                        );
                    }
                }
            }
        }
        println!("GATE4 三族核对：9 格 × 3 族，其中 {整族不写} 个「源为 0 ⇒ 整族不写」");
    }

    // ④ 保存 → 逐字节
    let saved = save::save_session(&mut doc, &mut ed);
    assert!(matches!(saved, SaveOutcome::Written { .. }), "保存应写盘：{saved:?}");
    let 落盘 = std::fs::read(&副本).expect("读回");
    let 差异 = 差异清单(&原字节, &落盘);
    let 变格 = 变化格(&差异, 高);
    let 期望: BTreeSet<(u16, u16)> = 目标.iter().collect();
    // 断言是**子集**（= 任务书 §四.4 的原话"只有目标 9 格变"）：
    // 若某格的源内容与目标原值三族全同，那一格本来就不会变，不必强求 9 格全动。
    assert!(
        变格.is_subset(&期望),
        "落盘后变化的格必须**全部落在**目标 3×3 内，越界的有：{:?}",
        变格.difference(&期望).collect::<Vec<_>>()
    );
    assert!(!变格.is_empty(), "粘贴必须真的改了格（变化格为空）");
    // bit15 保留（逐格）—— 保存只把 `map` 换进 `document.view` 再换回来，内容不变，
    // 所以这里重新借一次即可（与保存前那份 `map` 是同一批格）。
    let map = &doc.doc.as_ref().expect("文档").map;
    for (x, y) in 目标.iter() {
        let a = 原图.cell(x, y).expect("原格");
        let b = map.cell(x, y).expect("新格");
        assert_eq!(b.bk_img & 0x8000, a.bk_img & 0x8000, "({x},{y}) 地表 bit15");
        assert_eq!(b.fr_img & 0x8000, a.fr_img & 0x8000, "({x},{y}) 物体 bit15");
    }
    // 差异字节全部落在三族图号区间
    for (off, _, _) in &差异 {
        let 格内 = (*off - 头长) % 格长;
        assert!(格内 < 6, "偏移 {off} 落在格内第 {格内} 字节（只许动三族图号）");
    }
    println!(
        "GATE4 落盘：变化格={} 变化字节={}（三族图号区间内）",
        变格.len(),
        差异.len()
    );
    for (x, y) in 目标.iter() {
        println!(
            "GATE4 ({x},{y}) 旧={} 新={}",
            hex(&格字节(&原字节, x, y, 高)),
            hex(&格字节(&落盘, x, y, 高))
        );
    }

    // ⑤ 撤销粘贴 ⇒ 逐字节回到操作前（粘贴是一步）
    let n = ed.undo(&mut doc.doc.as_mut().expect("文档").map);
    assert_eq!(n, 9, "撤销应还原 9 格");
    assert_eq!(
        doc.doc.as_ref().expect("文档").map.to_bytes(),
        原字节,
        "撤销粘贴后必须逐字节回到原文件"
    );

    let _ = std::fs::remove_dir_all(&d);
}

// ═══════════════════════════ 门 5 ═══════════════════════════

/// **门 5**：`Ctrl+X` 剪切 3×3 ⇒ 保存后**只有源 9 格被清**（按层语义），其余 0 差异。
///
/// "按层语义"逐条：
/// - 地表：`bk_img &= 0x8000`（**保留阻挡位**，只清图号）；
/// - 中层：`mid_img = 0`（中层没有阻挡位可言）；
/// - 物体：`fr_img &= 0x8000`（**保留阻挡位**）；
/// - `Old12` 档没有库号字节 ⇒ 不存在"库号写 0"这回事（有库号字节的档位由 core 的
///   `selection_clipboard.rs::门2补充_三族与三个库号逐格搬运` 覆盖）。
#[test]
fn 门5_剪切3x3_只清源9格() {
    let Some((d, 副本, 原字节)) = 复制地图("gate5") else {
        return;
    };
    let mut doc = 文档(&副本);
    let mut ed = EditSession::default();
    let 原图 = Map::parse(&原字节).expect("原字节应能解析");
    let 高 = 原图.height;
    let Some((sx, sy)) = 找地表3x3(&原图, None) else {
        panic!("0.map 上找不到 3×3 全是地表图号的区域 —— 样本变了？");
    };
    let 源 = Selection::new(sx, sy, sx + 2, sy + 2);
    println!("GATE5 源选区={源:?}");

    assert!(ed.set_selection(&mut doc.doc.as_mut().expect("文档").map, 源));
    let cut = selection_ops::apply(&mut ed, &mut doc.doc.as_mut().expect("文档").map, Cmd::Cut, None)
        .expect("剪切总有回执");
    assert!(!cut.warn, "剪切不该被拒：{}", cut.text);
    assert_eq!(cut.text, "剪切 3×3 格（清空 9 格）");
    assert_eq!(cut.changed, 9);
    assert_eq!(ed.undo_len(), 1, "整次剪切 = 一步撤销");

    // 剪贴板拿到源内容（三族逐格）
    let clip = ed.clipboard().expect("剪贴板").clone();
    assert_eq!((clip.width(), clip.height()), (3, 3));
    for (i, c) in clip.cells().iter().enumerate() {
        let (x, y) = (源.x0 + (i as u16 % 3), 源.y0 + (i as u16 / 3));
        let a = 原图.cell(x, y).expect("源格");
        assert_eq!(c.bk, a.bk_img & 0x7FFF, "第 {i} 格地表图号");
        assert_eq!(c.mid, a.mid_img & 0x7FFF, "第 {i} 格中层图号");
        assert_eq!(c.fr, a.fr_img & 0x7FFF, "第 {i} 格物体图号");
    }

    // 按层语义逐格核对
    let map = &doc.doc.as_ref().expect("文档").map;
    for (x, y) in 源.iter() {
        let a = 原图.cell(x, y).expect("原格");
        let b = map.cell(x, y).expect("新格");
        assert_eq!(b.bk_img, a.bk_img & 0x8000, "({x},{y}) 地表只清图号、保留 bit15");
        assert_eq!(b.mid_img, 0, "({x},{y}) 中层清成 0");
        assert_eq!(b.fr_img, a.fr_img & 0x8000, "({x},{y}) 物体只清图号、保留 bit15");
        // 其余字段原样
        assert_eq!(b.door_index, a.door_index);
        assert_eq!(b.area, a.area);
        assert_eq!(b.light, a.light);
    }

    let saved = save::save_session(&mut doc, &mut ed);
    assert!(matches!(saved, SaveOutcome::Written { .. }), "保存应写盘：{saved:?}");
    let 落盘 = std::fs::read(&副本).expect("读回");
    let 差异 = 差异清单(&原字节, &落盘);
    let 变格 = 变化格(&差异, 高);
    let 期望: BTreeSet<(u16, u16)> = 源.iter().collect();
    assert_eq!(变格, 期望, "落盘后变化的格必须**正好**是源 3×3 那 9 格");
    for (off, _, _) in &差异 {
        let 格内 = (*off - 头长) % 格长;
        assert!(格内 < 6, "偏移 {off} 落在格内第 {格内} 字节（只许动三族图号）");
    }
    println!(
        "GATE5 落盘：变化格={} 变化字节={}（只清源 9 格的三族图号）",
        变格.len(),
        差异.len()
    );
    for (x, y) in 源.iter() {
        println!(
            "GATE5 ({x},{y}) 旧={} 新={}",
            hex(&格字节(&原字节, x, y, 高)),
            hex(&格字节(&落盘, x, y, 高))
        );
    }

    // 撤销剪切 ⇒ 逐字节回到操作前
    let n = ed.undo(&mut doc.doc.as_mut().expect("文档").map);
    assert_eq!(n, 9);
    assert_eq!(doc.doc.as_ref().expect("文档").map.to_bytes(), 原字节);

    let _ = std::fs::remove_dir_all(&d);
}


// ═══════════════════════════ 门 6（Type5 闸门，注入验证） ═══════════════════════════

/// 造一张 **Type5（`Mir3Wemade`）** 合成样本（与 core 的 `造_type5` 逐字节同口径）。
///
/// 布局：28 字节头 + `3 × ceil(w/2) × floor(h/2)` 字节背图块 + `14 × w × h` 格。
/// 头部 `[22..24]` = 宽、`[24..26]` = 高、`[20..22]` = 版本 1。
fn 造_type5(w: i16, h: i16) -> Vec<u8> {
    let (wu, hu) = (w as u16, h as u16);
    let 块长 = 3 * ((wu as usize / 2) + (wu as usize % 2)) * (hu as usize / 2);
    let n = 28 + 块长 + 14 * (w as usize) * (h as usize);
    let mut b = vec![0u8; n];
    b[0] = 0;
    b[20..22].copy_from_slice(&1i16.to_le_bytes());
    b[22..24].copy_from_slice(&w.to_le_bytes());
    b[24..26].copy_from_slice(&h.to_le_bytes());
    for i in 0..块长 {
        b[28 + i] = (i as u8).wrapping_mul(7).wrapping_add(1);
    }
    let 起 = 28 + 块长;
    for i in 0..(w as usize * h as usize) {
        let base = 起 + i * 14;
        b[base] = 0b11;
        b[base + 5..base + 7].copy_from_slice(&(100 + i as u16).to_le_bytes());
        b[base + 7..base + 9].copy_from_slice(&(200 + i as u16).to_le_bytes());
        b[base + 12] = 0x0F;
    }
    b
}

/// **门 6**：Type5 档位上**带地表**（`bk≠0`）的内容 ⇒ 拒绝 + 中文原因，**一格不写**。
///
/// ⚠ **这是注入验证**（见模块文档 §三）：
/// 1. 档位是注入的（GUI 打不开 Type5 文件）；
/// 2. "带地表的内容"是**从真实地图复制出来的剪贴板**（`造图16` 每格地表图号非 0），
///    而不是某个真实的 `.X` —— 因为本机那枚真实印章 `mir_00000` 是**纯物件**印章
///    （25 格 `bk` 全 0），它**本来就该**在 Type5 上被放行（见下面的反向对照）。
///
/// 走的是 `Ctrl+V` 的同一条链路（[`selection_ops::apply`] → 内核 `paste_clipboard`
/// → `place_stamp` → `plan_stamp` 的 Type5 闸门）。
///
/// 四条断言：
/// 1. 回执是**拒绝**（`warn`）且点名档位 `Mir3Wemade`、"不支持逐格写地表背图"、"bk≠0"；
/// 2. `undo_len() == 0`（**没发命令**：拒绝发生在 `plan_stamp` 里，撤销栈一步都没多）；
/// 3. 地图 `to_bytes()` 逐字节不变；
/// 4. **反向对照**：同一张 Type5 图、同一枚真实印章（`bk` 全 0）⇒ **放行**，
///    证明拒的是"逐格地表"这一项能力，不是整张图。
#[test]
fn 门6_type5拒绝带地表内容_注入验证() {
    let mut 宿主 = 造图16();
    let mut ed = EditSession::default();

    // ① 先在一张正常的 New14 图上"框一块 + 复制"，得到一份**真的带地表**的剪贴板
    //    （走 `set_selection` / `copy_selection` 两条真实路径，不手工拼剪贴板）
    assert!(
        ed.set_selection(&mut 宿主, Selection::new(0, 0, 2, 2)),
        "New14 图上 3×3 选区应生效"
    );
    let copy = selection_ops::apply(&mut ed, &mut 宿主, Cmd::Copy, None).expect("复制总有回执");
    assert!(!copy.warn, "复制不该被拒：{}", copy.text);
    let clip = ed.clipboard().expect("剪贴板").clone();
    assert!(
        clip.cells().iter().any(|c| c.bk != 0),
        "这份剪贴板必须真的带地表（否则本用例验不到 Type5 闸门）"
    );

    // ② **注入** Type5 档位（16×16，与宿主地图同尺寸 ⇒ 不会被 `sync_document_format` 重建掉）
    assert!(
        ed.inject_document(造_type5(16, 16)),
        "Type5 合成样本应能被解析成文档（否则注入失败，本用例无效）"
    );
    let 注入前字节 = 宿主.to_bytes();

    // ③ 粘贴（Ctrl+V）⇒ 拒绝
    let paste = selection_ops::apply(&mut ed, &mut 宿主, Cmd::Paste, Some((4, 4)))
        .expect("有鼠标格就有回执");
    println!("GATE6 拒绝回执：{}", paste.text);
    assert!(paste.warn, "Type5 上贴带地表的内容必须被拒：{}", paste.text);
    assert!(paste.text.contains("粘贴被拒绝"), "回执要说明是粘贴被拒：{}", paste.text);
    assert!(paste.text.contains("Mir3Wemade"), "原因要点名档位：{}", paste.text);
    assert!(
        paste.text.contains("不支持逐格写地表背图"),
        "原因要说清能力缺口：{}",
        paste.text
    );
    assert!(paste.text.contains("bk≠0"), "原因要点明哪一族：{}", paste.text);
    assert!(paste.text.contains("一个字节都没动"), "原因要给出没动地图的保证：{}", paste.text);
    assert_eq!(paste.changed, 0, "拒绝时不该改格");
    assert_eq!(ed.undo_len(), 0, "拒绝不该产生撤销步（= 没发命令）");
    assert!(!ed.dirty(), "拒绝不该置脏");
    assert_eq!(宿主.to_bytes(), 注入前字节, "拒绝时地图必须逐字节不变");

    // ④ 反向对照：同一张 Type5 图 + 真实印章（`bk` 全 0）⇒ 放行
    if let Some(印章) = 印章路径() {
        let armed = Armed {
            id: "mir/mir_00000".to_string(),
            name: "mir_00000".to_string(),
            bounds: Some((-1, -2, 4, 3)),
        };
        let out = stamp_place::place(&mut ed, &mut 宿主, &armed, &印章, 8, 8)
            .expect("纯物件印章在 Type5 上应放行（拒的是逐格地表，不是整张图）");
        assert_eq!(out.cells, 8, "New14/Type5 上放同一枚纯物件印章都改 8 格");
        println!("GATE6 反向对照（Type5 + 纯物件印章）放行：改动 {} 格", out.cells);
    } else {
        eprintln!(
            "[跳过] 反向对照需要取证根 MIREDIT_UPSTREAM_ROOT 与 Objects/mir/mir_00000.X"
        );
    }
}

/// 16×16 `New14` 合成图（每格地表图号 = 序号 + 1、`btTiles` = 3）。
fn 造图16() -> Map {
    let w = 16u16;
    let h = 16u16;
    let mut bytes = vec![0u8; mir2_map::HEADER_LEN + 14 * (w as usize * h as usize)];
    bytes[0..2].copy_from_slice(&w.to_le_bytes());
    bytes[2..4].copy_from_slice(&h.to_le_bytes());
    for i in 0..(w as usize * h as usize) {
        let base = mir2_map::HEADER_LEN + i * 14;
        bytes[base..base + 2].copy_from_slice(&((i as u16) + 1).to_le_bytes());
        bytes[base + 12] = 3;
    }
    Map::parse(&bytes).expect("16×16 合成图应能解析")
}
