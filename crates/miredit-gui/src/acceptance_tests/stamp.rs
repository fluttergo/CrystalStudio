//! 门 2（印章放置逐字节）+ 门 3（撤销逐字节）。
//!
//! 两个门共用一个 `0.map` 副本，顺序是：放 → 断言 → **撤销比字节（门 3）** →
//! 再放 → 保存 → 比字节（门 2）。门 3 比的是"还没写盘的原文件"，门 2 比的是"写盘后的文件"，
//! 两门互不污染。

use std::collections::{BTreeMap, BTreeSet};

use mir2_map::Map;

use crate::edit::EditSession;
use crate::save::{self, SaveOutcome};
use crate::stamp_place::{self, Armed};

use super::common::{
    变化格, 复制地图, 格字节, 差异清单, 印章路径, 文档, 头长, 格长, hex,
};

// ═══════════════════════════ 门 2 + 门 3 ═══════════════════════════

/// **门 2**：`0.map` 副本上放 `mir/mir_00000` 到 `(100,100)` ⇒ 保存后**只有印章覆盖的格变**、
/// `bit15` 保留、其余 0 差异；贴 hex 与变化格数（应为 core 的 **8 格**）。
///
/// **门 3**：放置后撤销 ⇒ `to_bytes()` 与原文件**逐字节相等**。
///
/// 顺序说明：先放 → 断言 → **撤销比字节（门 3）** → 再放 → 保存 → 比字节（门 2）。
/// 这样门 3 比的是"还没写盘的原文件"，门 2 比的是"写盘后的文件"，两门互不污染。
#[test]
fn 门2门3_真实印章贴0map副本_逐字节核对与撤销() {
    let Some((d, 副本, 原字节)) = 复制地图("gate2") else {
        return;
    };
    let 印章 = match 印章路径() {
        Some(p) => p,
        None => {
            eprintln!("[跳过] 无取证根 MIREDIT_UPSTREAM_ROOT（或缺 Objects/mir/mir_00000.X）");
            let _ = std::fs::remove_dir_all(&d);
            return;
        }
    };
    if !印章.is_file() {
        eprintln!("[跳过] 本机没有印章样本 {}", 印章.display());
        let _ = std::fs::remove_dir_all(&d);
        return;
    }
    let mut doc = 文档(&副本);
    let mut ed = EditSession::default();
    let armed = Armed {
        id: "mir/mir_00000".to_string(),
        name: "mir_00000".to_string(),
        bounds: Some((-1, -2, 4, 3)),
    };
    let 高 = doc.doc.as_ref().expect("文档").map.height;
    assert_eq!(高, 700, "0.map 是 700×700");

    // ① 放置（读盘 → 注册 → 落位，全在 `stamp_place::place` 里）
    let 落点 = (100u16, 100u16);
    let out = stamp_place::place(
        &mut ed,
        &mut doc.doc.as_mut().expect("文档").map,
        &armed,
        &印章,
        落点.0,
        落点.1,
    )
    .expect("真实印章贴真实地图不该被拒");
    assert_eq!(out.skipped, 0, "锚点 (100,100) 在 700×700 图内，不该有越界格");
    assert_eq!(out.cells, 8, "core 门 2 的结论：这枚印章真正改动 8 格");
    assert_eq!(ed.undo_len(), 1, "整次放置 = 一步撤销");
    assert!(ed.dirty(), "放置必须置脏");
    let 回执 = stamp_place::receipt(&armed.name, out);
    assert_eq!(回执, "放置印章「mir_00000」8 格", "状态栏回执的原文");

    // ② 收集"印章覆盖的格"（独立推导，不看实现）
    //
    // ⚠ 2026-11-05：`StampCellInput` 从 8 字段扩到 18 字段（门 / 光照 / 动画 / 库号），
    // 所以这里同时留一份 **`(x,y) → 印章那一格的输入`** 的映射，
    // 逐字段核对"写进去的值 = 印章里的值"（而不是只看三族图号）。
    let mut 覆盖: BTreeSet<(u16, u16)> = BTreeSet::new();
    let mut 印章格: BTreeMap<(u16, u16), miredit_core::StampCellInput> = BTreeMap::new();
    {
        let cells = ed.stamp_cells(&armed.id).expect("注册过").to_vec();
        for c in cells {
            let k = (
                (i32::from(落点.0) + c.dx) as u16,
                (i32::from(落点.1) + c.dy) as u16,
            );
            覆盖.insert(k);
            印章格.insert(k, c);
        }
    }
    assert_eq!(覆盖.len(), 25, "mir_00000 是 25 格");

    // ③ 逐格核对：改动只发生在覆盖范围内；`bit15` 保留
    let map = &doc.doc.as_ref().expect("文档").map;
    let 原图 = Map::parse(&原字节).expect("原字节应能解析");
    let mut 变化明细: BTreeMap<(u16, u16), (Vec<u8>, Vec<u8>)> = BTreeMap::new();
    for &(x, y) in &覆盖 {
        let a = 原图.cell(x, y).expect("图内格");
        let b = map.cell(x, y).expect("图内格");
        let src = 印章格.get(&(x, y)).expect("覆盖格必有印章输入");
        assert_eq!(
            b.bk_img & 0x8000,
            a.bk_img & 0x8000,
            "({x},{y}) 地表 bit15 必须保留"
        );
        assert_eq!(
            b.fr_img & 0x8000,
            a.fr_img & 0x8000,
            "({x},{y}) 物体 bit15 必须保留"
        );
        assert_eq!(b.mid_img & 0x8000, 0, "中层不保留 bit15");
        // 三族图号 = 印章里的值（`0` = 该族没内容 ⇒ 不写，保持原值）
        if src.bk != 0 {
            assert_eq!(b.bk_img & 0x7FFF, src.bk, "({x},{y}) 地表图号");
        }
        if src.mid != 0 {
            assert_eq!(b.mid_img & 0x7FFF, src.mid, "({x},{y}) 中层图号");
        }
        if src.fr != 0 {
            assert_eq!(b.fr_img & 0x7FFF, src.fr, "({x},{y}) 物体图号");
        }
        // 门 / 门偏移：**非 0 才搬**（`PlacedStamps.cs:93`）
        if src.door != 0 {
            assert_eq!(b.door_index, src.door, "({x},{y}) 门索引");
            assert_eq!(b.door_offset, src.door_offset, "({x},{y}) 门偏移");
        } else {
            assert_eq!(b.door_index, a.door_index, "({x},{y}) 印章没门 ⇒ 原值不动");
        }
        // 光照：非 0 才搬（`PlacedStamps.cs:94`）
        if src.light != 0 {
            assert_eq!(b.light, src.light, "({x},{y}) 光照");
        } else {
            assert_eq!(b.light, a.light, "({x},{y}) 印章没光照 ⇒ 原值不动");
        }
        // 前景动画帧/节拍：物体图号非 0 才搬（`c.Foreground = front` 整体赋值）
        if src.fr != 0 {
            assert_eq!(b.ani_frame, src.ani_frame, "({x},{y}) 前景动画帧");
            assert_eq!(b.ani_tick, src.ani_tick, "({x},{y}) 前景动画节拍");
        } else {
            assert_eq!(b.ani_frame, a.ani_frame, "({x},{y}) 没前景 ⇒ 动画帧原值");
            assert_eq!(b.ani_tick, a.ani_tick, "({x},{y}) 没前景 ⇒ 动画节拍原值");
        }
        // **物体库号 `btArea`（`area`）会被写**（2026-11-05 订正，旧断言是错的）：
        // `mir2-format/src/formats/old12.rs:158` 的 writer 明写 `out.push(cell.area)`，
        // 而 `miredit-core` 的补写按 `writes_layer_lib_byte` 显式补一次，
        // 否则 Old12 会出现"视图里改了库号、文件里没动"。
        // ⇒ 有前景时 `area` = 印章的 `fr_lib`；没前景时保持原值。
        if src.fr != 0 {
            assert_eq!(b.area, src.fr_lib, "({x},{y}) 物体库号 btArea");
        } else {
            assert_eq!(b.area, a.area, "({x},{y}) 没前景 ⇒ 库号原值");
        }
        if a != b {
            let 前 = 格字节(&原字节, x, y, 高);
            let 后 = 格字节(&map.to_bytes(), x, y, 高);
            变化明细.insert((x, y), (前, 后));
        }
    }
    assert_eq!(变化明细.len(), 8, "真正变的格数必须是 8（= core 的 cells）");
    println!(
        "GATE2 印章=mir_00000 格=25 包围盒=(-1,-2,4,3) 落点={落点:?} 改动格={} 覆盖格={}",
        变化明细.len(),
        覆盖.len()
    );
    for ((x, y), (前, 后)) in &变化明细 {
        println!("GATE2 ({x},{y}) 旧={} 新={}", hex(前), hex(后));
    }

    // ④ **门 3**：撤销 ⇒ `to_bytes()` 与原文件逐字节相等
    let 撤销格 = ed.undo(&mut doc.doc.as_mut().expect("文档").map);
    assert_eq!(撤销格, 8, "撤销应还原 8 格");
    let 撤销后 = doc.doc.as_ref().expect("文档").map.to_bytes();
    let 逐字节相等 = 撤销后 == 原字节;
    assert!(
        逐字节相等,
        "门 3：撤销后 to_bytes() 必须与原文件逐字节相等，差异 {:?}",
        差异清单(&原字节, &撤销后)
    );
    assert!(!ed.dirty(), "撤销回原样后不该还是脏的");
    println!(
        "GATE3 撤销后逐字节相等={逐字节相等} 长度={}（原 {}）",
        撤销后.len(),
        原字节.len()
    );

    // ⑤ 再放一次 → 保存 → 与磁盘逐字节比对（门 2 的落盘侧）
    let out2 = stamp_place::place(
        &mut ed,
        &mut doc.doc.as_mut().expect("文档").map,
        &armed,
        &印章,
        落点.0,
        落点.1,
    )
    .expect("第二次放置也应成功");
    assert_eq!(out2, out, "两次放置结果必须一致（幂等）");
    let saved = save::save_session(&mut doc, &mut ed);
    assert!(
        matches!(saved, SaveOutcome::Written { .. }),
        "保存应真的写盘，实际 {saved:?}"
    );
    let 落盘 = std::fs::read(&副本).expect("读回写盘结果");
    assert_eq!(落盘.len(), 原字节.len(), "保存不许改文件长度");
    let 差异 = 差异清单(&原字节, &落盘);
    let 变格 = 变化格(&差异, 高);
    assert_eq!(
        变格, 变化明细.keys().copied().collect::<BTreeSet<_>>(),
        "落盘后变化的格必须**正好**是那 8 格"
    );
    // 逐格逐字节核对：差异只允许落在两类字节上
    //
    // | 格内偏移（12 B 布局） | 字段 | 为什么允许变 |
    // |---|---|---|
    // | `0..6` | `wBkImg` / `wMidImg` / `wFrImg` | 印章的三族图号 |
    // | `10..11` | `btArea`（物体库号） | **2026-11-05 订正**：`mir2-format/src/formats/old12.rs:158` 的 writer 明写 `out.push(cell.area)`，而 `miredit-core` 的补写会按 `writes_layer_lib_byte` 把印章的 `fr_lib` 落进去 —— 旧断言只允许 `0..6`，在 core 补写之后**必然失败**（实测差异落在第 10 字节） |
    //
    // 其余字节（门 6/7、前景动画 8/9、光照 11）一律**不许动** —— `mir_00000` 这三项全是 0，
    // 按「非 0 才搬」的规则它们必须保持原值（上面逐格断言已经核对过）。
    let 允许: [std::ops::Range<usize>; 2] = [0..6, 10..11];
    for (off, _, _) in &差异 {
        let 格内偏移 = (*off - 头长) % 格长;
        assert!(
            允许.iter().any(|r| r.contains(&格内偏移)),
            "偏移 {off} 落在格内第 {格内偏移} 字节（只允许三族图号 0..6 与物体库号 10）—— 印章不该动其它字段"
        );
    }
    println!(
        "GATE2 落盘：变化格={} 变化字节={} 全部落在三族图号(0..6) + 物体库号(10)",
        变格.len(),
        差异.len()
    );

    let _ = std::fs::remove_dir_all(&d);
}

