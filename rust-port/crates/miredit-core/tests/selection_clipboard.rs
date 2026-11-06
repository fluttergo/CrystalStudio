//! 矩形选区 + 剪贴板的**验收门 2~7**（真实地图 + 逐字节自证）+ **A-016 锚点回归**。
//!
//! 跑法：`cargo test -p miredit-core --test selection_clipboard -- --nocapture --test-threads=1`
//!
//! # 六个门（与派单 §四 逐条对应）+ 一条回归门
//!
//! | 门 | 判据 | 用例 |
//! |----|------|------|
//! | 2 | `0.map` 副本上选 3×3 → 复制 → 粘到 `(200,200)`（落点 = **锚点**，见下）→ 保存 ⇒ **只有目标 9 格变**、三族图号逐格与源相同、`bit15` 保留、其余 0 差异 | [`门2门4_复制粘贴0map副本_逐字节核对与撤销`] |
//! | 2 补 | 合成 `New14` 图上三族图号 + **三个库号字节**逐格搬过去（`0.map` 是 12 B/格，没有库号字节） | [`门2补充_三族与三个库号逐格搬运`] |
//! | 3 | 剪切 3×3 → 保存 ⇒ **只有源 9 格被清**（按层语义），其余 0 差异 | [`门3门4_剪切3x3_只清源9格且撤销逐字节`] |
//! | 4 | 粘贴后撤销 / 剪切后撤销，各与操作前**逐字节相等** | 同上两条 |
//! | 5 | 选区越界 ⇒ 裁剪到图内；粘贴越界 ⇒ 逐格跳过并报 `skipped` | [`门5_选区裁剪与粘贴越界跳过`] |
//! | 6 | 选 3×3 → `fill_selection` ⇒ 9 格全变成笔刷值（地表 `step=1`，见 `selection.rs` §五） | [`门6_填充选区9格`] |
//! | 7 | 合成 Type5 上粘贴含 `bk≠0` 的剪贴板 ⇒ 拒绝 + 中文原因 | [`门7_type5拒绝带地表剪贴板`] |
//! | A-016 | 粘贴落点 = `RegionCopy.cs:70` 的 `Anchor(点击格)`：源奇偶重算、无源/无地表不重算、负锚点整块拒绝且一格不写 | [`A016_粘贴锚点按源奇偶`] |
//!
//! # 为什么期望值要在测试里**独立推导**一遍
//!
//! 直接拿 `paste_clipboard` 的结果跟它自己比，这条测试永远绿 —— 那是假门。
//! 所以本文件按**规则**（不是按实现）重算每个格应该变成什么：
//!
//! ```text
//! 粘贴锚点（`RegionCopy.cs:70`，2026-11-06 对齐）：
//!   地表可见(ground) 且知道源左上角 ⇒ (x - ((x - sourceX) & 1), y - ((y - sourceY) & 1))
//!   否则                        ⇒ (x, y)
//!   锚点任一坐标为负 ⇒ 整块拒绝（对应 C# `CanPaste` 返回 false），地图一格不写
//! 粘贴（源格 s、目标格 t，三族各自独立判断）：
//!   地表 bk：s.bk == 0 ? t.bk : (t.bk & 0x8000) | (s.bk & 0x7FFF)
//!   中层 mid：s.mid == 0 ? t.mid : s.mid & 0x7FFF          // 不保留 bit15
//!   物体 fr：s.fr == 0 ? t.fr : (t.fr & 0x8000) | (s.fr & 0x7FFF)
//!   库号：该族图号非 0 才写（bk→btTiles / mid→btsmTiles / fr→btArea）
//! 清空（三族各自独立判断）：
//!   地表 bk：bk & 0x7FFF != 0 ? bk & 0x8000 : bk
//!   中层 mid：mid & 0x7FFF != 0 ? 0 : mid
//!   物体 fr：fr & 0x7FFF != 0 ? fr & 0x8000 : fr
//!   库号：该族被清时写 0（writer 不写该字节的格式除外，见 selection.rs §二）
//! ```
//!
//! 规则出处：`miredit-core/src/paint.rs` 的 `Tool::Brush` 分支（= `edit.rs:1462-1468`）
//! + `RegionCopy.cs:78-87`（粘贴逐族判断 + 保留目标阻挡位）+ `RegionCopy.cs:45-69`（清空）。

use std::collections::BTreeMap;
use std::path::PathBuf;

use miredit_core::{
    apply, save_session, Clipboard, Command, EditSession, PlaceOutcome, Selection, StampCellInput,
};
use mir2_format::FormatId;
use mir2_map::Cell;

/// 资产根缺省值：本机真实根（**整个文件里只出现这一处**）。换机器用 `MIREDIT_ASSET_ROOT`。
const 缺省资产根: &str = r"E:\GodM2";

/// 资产根：`MIREDIT_ASSET_ROOT` → 缺省 [`缺省资产根`]。
///
/// 根目录不存在（公开仓/PR 的机器上没有游戏素材）⇒ `None`，调用方打印 `[skip]` 并返回。
/// 只做 `var_os` + `PathBuf`，不引入任何第三方库。
fn 资产根() -> Option<PathBuf> {
    let root = std::env::var_os("MIREDIT_ASSET_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(缺省资产根));
    root.is_dir().then_some(root)
}

/// 真实地图样本（700×700 / 12 B 每格 / 无库号字节）。
fn 地图路径() -> Option<PathBuf> {
    let p = 资产根()?.join("GodotClient").join("maps").join("0.map");
    p.is_file().then_some(p)
}

/// 临时目录（`%TEMP%` 下，按 `tag` + **进程 ID** 分开；每次先清空）。
///
/// # 为什么目录名必须带 PID（⚠ 别改回去）
///
/// 两个 `cargo test` 进程并发跑同一个测试目标时，**固定目录名**会让两边互相
/// `remove_dir_all`、并抢同一个临时文件，产生**假红**（实测症状：
/// 「写入不完整：期望 5880052 字节，读回 480052」）。
/// 加上 [`std::process::id`] 之后两个进程各用各的目录，并发互不干扰；
/// `tag` 仍然保留，便于在 `%TEMP%` 里按用例名排查。
fn 临时目录(tag: &str) -> PathBuf {
    let pid = std::process::id();
    let d = std::env::temp_dir().join(format!("miredit_core_sel_{tag}_{pid}"));
    // 自测（审查报告 2026-11-05 第 7 轮 · 缺陷 8）：光比"后缀是 _{pid}"是**自证** ——
    // `d` 就是上一行用同一个 `pid` 拼出来的，把 `std::process::id()` 换成常量 0 也照样绿。
    // 改成三条真判据：
    //   ① 期望串里的 pid 由**再一次独立调用**取回，不复用上面的 `pid`（两处不同源才叫交叉验证）；
    //   ② 与**实际目录名逐字相等**比较（不是只比后缀）；
    //   ③ 额外钉死"进程号不可能是 0"。
    // ⇒ 把 `let pid = std::process::id();` 改成 `let pid = 0;`，本断言必红（已做变异实验）。
    let 本进程 = std::process::id();
    assert_ne!(本进程, 0, "进程号不可能是 0，但 std::process::id() 返回了 0");
    let 实际名 = d.file_name().and_then(|s| s.to_str()).unwrap_or("<非 UTF-8>");
    let 期望名 = format!("miredit_core_sel_{tag}_{本进程}");
    assert_eq!(
        实际名,
        期望名.as_str(),
        "临时目录名必须逐字等于「miredit_core_sel_<tag>_<本进程 PID>」，否则并发隔离失效：{}",
        d.display()
    );
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("建临时目录");
    d
}

/// 逐字节差异：返回 `(偏移, 原值, 新值)` 列表。
fn 差异清单(旧: &[u8], 新: &[u8]) -> Vec<(usize, u8, u8)> {
    let mut out = Vec::new();
    for i in 0..旧.len().max(新.len()) {
        let a = 旧.get(i).copied();
        let b = 新.get(i).copied();
        if a != b {
            out.push((i, a.unwrap_or(0xFF), b.unwrap_or(0xFF)));
        }
    }
    out
}

/// 「世界坐标 → 格」的快照表。
type 格表 = BTreeMap<(u16, u16), Cell>;

/// 把一块矩形选区里的旧格快照下来。
fn 快照(ed: &EditSession, sel: Selection) -> 格表 {
    let mut m = 格表::new();
    for (x, y) in sel.iter() {
        m.insert((x, y), *ed.map().cell(x, y).expect("选区已在图内"));
    }
    m
}

/// 按**规则**（不是按实现）算出粘贴后的期望格。
///
/// 规则出处：`RegionCopy.cs:78-87`
/// - `:78-82` 三族各自判断（源该族图号为 0 ⇒ 该族不写）；
/// - `:84` 地表保留目标 `bit15`（`target.BackImage & 0x60000000`）；
/// - `:86` `if (pasteObjects) { target.Foreground = source.Foreground;
///   target.Door = source.Door; target.DoorOffset = source.DoorOffset; }`
///   ⇒ 有前景就**无条件**赋值（源门为 0 会把目标的门清成 0），`Front.Library` 落到 `btArea`；
/// - `:84-86` **没有** `Light` ⇒ 光照一律保持目标格原值。
fn 期望粘贴(旧: Cell, s: &StampCellInput) -> Cell {
    let mut c = 旧;
    if s.bk != 0 {
        c.bk_img = (旧.bk_img & 0x8000) | (s.bk & 0x7FFF);
    }
    if s.mid != 0 {
        c.mid_img = s.mid & 0x7FFF;
    }
    if s.fr != 0 {
        c.fr_img = (旧.fr_img & 0x8000) | (s.fr & 0x7FFF);
        // `RegionCopy.cs:86` 的 `target.Door = source.Door` / `target.Foreground = source.Foreground`
        c.door_index = s.door;
        c.door_offset = s.door_offset;
        c.ani_frame = s.ani_frame;
        c.ani_tick = s.ani_tick;
        c.area = s.fr_lib;
    }
    // `light` 不动（`:84-86` 里没有这一行）
    c
}

/// 按**规则**算出清空后的期望格（三族各自独立判断，`selection.rs` §二）。
fn 期望清空(旧: Cell) -> Cell {
    let mut c = 旧;
    if 旧.bk_img & 0x7FFF != 0 {
        c.bk_img = 旧.bk_img & 0x8000;
    }
    if 旧.mid_img & 0x7FFF != 0 {
        c.mid_img = 0;
    }
    if 旧.fr_img & 0x7FFF != 0 {
        c.fr_img = 旧.fr_img & 0x8000;
    }
    c
}

/// 12 B/格（`Old12`）的整格字节。
fn 格字节12(c: Cell) -> Vec<u8> {
    let mut v = Vec::with_capacity(12);
    v.extend_from_slice(&c.bk_img.to_le_bytes());
    v.extend_from_slice(&c.mid_img.to_le_bytes());
    v.extend_from_slice(&c.fr_img.to_le_bytes());
    v.extend_from_slice(&[
        c.door_index,
        c.door_offset,
        c.ani_frame,
        c.ani_tick,
        c.area,
        c.light,
    ]);
    v
}

/// 逐格数"到底有几个字节变了"（只统计 `格子` 里的格）。
fn 逐格差异字节(旧: &格表, 新: &格表) -> usize {
    旧.iter()
        .map(|(k, a)| {
            let b = &新[k];
            let (x, y) = (格字节12(*a), 格字节12(*b));
            x.iter().zip(y.iter()).filter(|(p, q)| p != q).count()
        })
        .sum()
}

/// 把「字节偏移」换算成「格坐标」（`Old12`：52 字节头 + 12 字节/格，列优先）。
fn 偏移转格(off: usize, 头长: usize, 格长: usize, 高: u16) -> (u16, u16) {
    let idx = (off - 头长) / 格长;
    ((idx / 高 as usize) as u16, (idx % 高 as usize) as u16)
}

/// 在图上找一块 3×3、**9 格地表都有图号**的区域（真实地图的地表是密铺的，物件/中层是稀疏的）。
///
/// 三级优先（越靠前，三族的搬运验得越全）：
/// 1. 9 格地表都有 + **至少一格中层** + **至少一格物体**（三族齐全）；
/// 2. 9 格地表都有 + 中层或物体至少有一格；
/// 3. 9 格地表都有（兜底）。
///
/// `避开` = 要跳过的区域（保证源块与粘贴目标不重叠，读起来更直观）。
fn 找地表3x3(ed: &EditSession, 避开: Selection) -> (u16, u16) {
    let (w, h) = (ed.map().width, ed.map().height);
    let mut 二级: Option<(u16, u16)> = None;
    let mut 三级: Option<(u16, u16)> = None;
    for y in 0..h.saturating_sub(2) {
        for x in 0..w.saturating_sub(2) {
            let sel = Selection::new(x, y, x + 2, y + 2);
            // 与"避开区"有任何交集就跳过
            if sel.contains(避开.x0, 避开.y0)
                || sel.contains(避开.x1, 避开.y1)
                || 避开.contains(x, y)
                || 避开.contains(x + 2, y + 2)
            {
                continue;
            }
            let (mut 全有地表, mut 有中层, mut 有物体) = (true, false, false);
            for dy in 0..3 {
                for dx in 0..3 {
                    let Some(c) = ed.map().cell(x + dx, y + dy) else {
                        全有地表 = false;
                        continue;
                    };
                    if c.bk_img & 0x7FFF == 0 {
                        全有地表 = false;
                    }
                    if c.mid_img & 0x7FFF != 0 {
                        有中层 = true;
                    }
                    if c.fr_img & 0x7FFF != 0 {
                        有物体 = true;
                    }
                }
            }
            if !全有地表 {
                continue;
            }
            if 有中层 && 有物体 {
                return (x, y); // 一级：三族齐全
            }
            if (有中层 || 有物体) && 二级.is_none() {
                二级 = Some((x, y));
            }
            if 三级.is_none() {
                三级 = Some((x, y));
            }
        }
    }
    二级.or(三级).unwrap_or_else(|| {
        panic!("0.map 上找不到 3×3 全带地表图号的区域（测试前提不成立）")
    })
}

// ═══════════════════════════ 门 2 + 门 4（复制粘贴） ═══════════════════════════

/// **门 2**：`0.map` 副本上选 3×3 → 复制 → 粘到 `(200,200)` → 保存 ⇒
/// 只有目标 9 格变、三族图号逐格与源相同、`bit15` 保留、其余 0 差异。
///
/// **门 4**：粘贴后撤销 ⇒ `to_bytes()` 与操作前逐字节相等。
///
/// ⚠ **落点 = 锚点，不是点击点**（2026-11-06 对齐 `RegionCopy.cs:70`）：
/// 地表可见的剪贴板会按**源的奇偶**把左上角往左上对齐，所以本用例先用
/// `clip.anchor(200,200)` 算出**实际锚点**，再按锚点独立推导期望表与差异格集合
/// （见 ②b / ③ / ⑤ / ⑦）。
#[test]
fn 门2门4_复制粘贴0map副本_逐字节核对与撤销() {
    let Some(地图) = 地图路径() else {
        eprintln!("[skip] 无资产根 MIREDIT_ASSET_ROOT（或缺 GodotClient/maps/0.map）");
        return;
    };
    let d = 临时目录("gate2");
    let 副本 = d.join("0.map");
    std::fs::copy(&地图, &副本).expect("复制 0.map 副本");
    let 原字节 = std::fs::read(&副本).expect("读副本");
    let mut ed = EditSession::load(&副本).expect("打开 0.map 副本");
    assert_eq!(ed.document().format_id(), FormatId::Old12, "0.map 是 12 B/格");
    let (w, h) = (ed.map().width, ed.map().height);
    let 头长 = ed.document().raw_header().len();
    let 格长 = ed.document().disk_cell_len().expect("定长格式");

    // ① 选源块：3×3、9 格地表都有图号，且与粘贴点击点 (200,200) 一带不重叠
    let 避开 = Selection::new(200, 200, 202, 202);
    let (sx, sy) = 找地表3x3(&ed, 避开);
    let 源 = Selection::new(sx, sy, sx + 2, sy + 2);
    let 源旧 = 快照(&ed, 源);
    println!("GATE2 源块={源:?}（{w}×{h} 图内）粘贴点击点=(200,200)");

    // ② 设选区 + 复制（**不改地图、不置脏**）
    assert!(ed.set_selection(源), "源块在图内 ⇒ 选区非空");
    assert_eq!(ed.selection(), Some(源));
    let (cw, ch) = ed.copy_selection().expect("应能复制");
    assert_eq!((cw, ch), (3, 3), "3×3");
    assert_eq!(ed.undo_len(), 0, "复制不许产生撤销步");
    assert!(!ed.dirty(), "复制不许置脏（选区/剪贴板是 UI 状态）");
    assert_eq!(ed.document().to_bytes(), 原字节, "复制一个字节都不许动");
    // 剪贴板里就是源块的**整格**：三族图号 + 三个库号 + 门/光照/前景动画（逐格核对）
    let clip = ed.clipboard().expect("复制后剪贴板应存在").clone();
    assert_eq!(clip.cells().len(), 9, "整块 9 格都抓（含空格）");
    for s in clip.cells() {
        let c = 源旧[&((sx as i32 + s.dx) as u16, (sy as i32 + s.dy) as u16)];
        assert_eq!(s.bk, c.bk_img & 0x7FFF, "剪贴板地表图号 = 源格低 15 位");
        assert_eq!(s.mid, c.mid_img & 0x7FFF, "剪贴板中层图号");
        assert_eq!(s.fr, c.fr_img & 0x7FFF, "剪贴板物体图号");
        assert_eq!(s.bk_lib, c.tiles, "剪贴板地表库号");
        assert_eq!(s.mid_lib, c.sm_tiles, "剪贴板中层库号");
        assert_eq!(s.fr_lib, c.area, "剪贴板物体库号");
        // **整格抓**（`RegionCopy.cs:24` 的 `.Copy()`）：门/光照/前景动画也要一并抓到
        assert_eq!(s.door, c.door_index, "剪贴板门索引 = 源格");
        assert_eq!(s.door_offset, c.door_offset, "剪贴板门偏移 = 源格");
        assert_eq!(s.light, c.light, "剪贴板光照 = 源格（整格快照，粘贴时按 C# 不落）");
        assert_eq!(s.ani_frame, c.ani_frame, "剪贴板前景动画帧 = 源格");
        assert_eq!(s.ani_tick, c.ani_tick, "剪贴板前景动画节拍 = 源格");
        // 12/14 B 布局里没有这 5 个字节 ⇒ 必须是 0（不猜、不编）
        assert_eq!(
            (s.mid_ani_frame, s.mid_ani_tick, s.tile_animation, s.tile_offset, s.tile_frames),
            (0, 0, 0, 0, 0),
            "视图里没有字节的 5 个动画字段必须留 0"
        );
    }

    // ②b **粘贴锚点**（`RegionCopy.cs:70`）：地表可见的剪贴板会按**源的奇偶**把左上角往左上
    //     对齐 ⇒ 点击点 (200,200) 不一定是落点。期望表必须按**实际锚点**构造，
    //     否则下面的逐格断言会整体错一格（假红/假绿都算）。
    let (tx, ty) = clip
        .anchor(200, 200)
        .expect("源块在图内、点击点 (200,200) ⇒ 重算后至多退到 (199,199)，不会是负锚点");
    let 目标 = Selection::new(tx, ty, tx + 2, ty + 2);
    assert_eq!(
        (tx, ty),
        (
            200 - ((200 - i32::from(sx)) & 1) as u16,
            200 - ((200 - i32::from(sy)) & 1) as u16
        ),
        "锚点必须逐字等于 `RegionCopy.cs:70` 的公式（源左上角 = ({sx},{sy})）"
    );
    println!("GATE2 粘贴锚点=({tx},{ty})（点击点 (200,200)，源左上角=({sx},{sy})）");

    // ③ 期望表（独立推导）：目标 9 格按规则算出应有值
    let 目标旧 = 快照(&ed, 目标);
    let mut 目标新 = 格表::new();
    for s in clip.cells() {
        let key = ((i32::from(tx) + s.dx) as u16, (i32::from(ty) + s.dy) as u16);
        目标新.insert(key, 期望粘贴(目标旧[&key], s));
    }
    let 期望变化格 = 目标新
        .iter()
        .filter(|(k, v)| 目标旧.get(*k) != Some(*v))
        .count();

    // ④ 粘贴（点击点 = (200,200)，落点 = 锚点 (tx,ty)）
    let out = ed.paste_clipboard(200, 200);
    let (改动格, 跳过格) = match &out {
        PlaceOutcome::Placed { cells, skipped } => (*cells, *skipped),
        PlaceOutcome::Refused { reason } => panic!("(200,200) 在图内，不该被拒：{reason}"),
    };
    assert_eq!(跳过格, 0, "没有越界格");
    assert_eq!(改动格, 期望变化格, "改动格数必须等于独立推导的变化格数");
    assert_eq!(ed.undo_len(), 1, "整次粘贴 = 一步撤销");

    // ⑤ 逐格核对：三族图号与源**逐格相同**（源该族非 0 时）、`bit15` 保留
    let mut 源带地表 = 0;
    let mut 源带中层 = 0;
    let mut 源带物体 = 0;
    for s in clip.cells() {
        let key = ((i32::from(tx) + s.dx) as u16, (i32::from(ty) + s.dy) as u16);
        let 旧 = 目标旧[&key];
        let 实得 = *ed.map().cell(key.0, key.1).expect("图内格");
        let 期望 = 目标新[&key];
        assert_eq!(实得.bk_img, 期望.bk_img, "({},{}) 地表", key.0, key.1);
        assert_eq!(实得.mid_img, 期望.mid_img, "({},{}) 中层", key.0, key.1);
        assert_eq!(实得.fr_img, 期望.fr_img, "({},{}) 物体", key.0, key.1);
        // 「与源逐格相同」：源该族有内容时，目标该族的**低 15 位**必须等于源
        if s.bk != 0 {
            源带地表 += 1;
            assert_eq!(实得.bk_img & 0x7FFF, s.bk, "({},{}) 地表图号必须等于源", key.0, key.1);
        }
        if s.mid != 0 {
            源带中层 += 1;
            assert_eq!(实得.mid_img & 0x7FFF, s.mid, "({},{}) 中层图号必须等于源", key.0, key.1);
        }
        if s.fr != 0 {
            源带物体 += 1;
            assert_eq!(实得.fr_img & 0x7FFF, s.fr, "({},{}) 物体图号必须等于源", key.0, key.1);
        }
        // `bit15` 保留：目标格的阻挡位不许被源格贴掉
        assert_eq!(实得.bk_img & 0x8000, 旧.bk_img & 0x8000, "({},{}) 地表 bit15 保留", key.0, key.1);
        assert_eq!(实得.fr_img & 0x8000, 旧.fr_img & 0x8000, "({},{}) 物体 bit15 保留", key.0, key.1);
        assert_eq!(实得.mid_img & 0x8000, 0, "中层不保留 bit15");
        // 附加字段：按 `RegionCopy.cs:84-86` 的**规则**核对（不是笼统的"原样"）
        assert_eq!(实得.door_index, 期望.door_index, "({},{}) 门索引", key.0, key.1);
        assert_eq!(实得.door_offset, 期望.door_offset, "({},{}) 门偏移", key.0, key.1);
        assert_eq!(实得.ani_frame, 期望.ani_frame, "({},{}) 前景动画帧", key.0, key.1);
        assert_eq!(实得.ani_tick, 期望.ani_tick, "({},{}) 前景动画节拍", key.0, key.1);
        assert_eq!(实得.area, 期望.area, "({},{}) 物体库号 btArea", key.0, key.1);
        // 光照：C# 的 `Paste` 里**没有** `Light`（`RegionCopy.cs:84-86`）⇒ 必须保持目标格原值
        assert_eq!(实得.light, 旧.light, "({},{}) 光照不许被粘贴改", key.0, key.1);
        assert_eq!(实得.tail, 旧.tail, "({},{}) 尾区不参与", key.0, key.1);
    }
    assert_eq!(源带地表, 9, "源块 9 格地表都有内容（选块判据）");
    assert!(
        源带中层 >= 1 && 源带物体 >= 1,
        "源块必须三族齐全（找地表3x3 的三级优先失效了？带中层={源带中层} 带物体={源带物体}）"
    );
    println!("GATE2 剪贴板 9 格：带地表={源带地表} 带中层={源带中层} 带物体={源带物体} 改动格={改动格}");

    // ⑥ 门 4：撤销 ⇒ 逐字节相等
    let 粘之后 = ed.document().to_bytes();
    let 撤 = ed.undo();
    assert_eq!(撤, 改动格, "撤销应还原同样多的格");
    assert_eq!(ed.document().to_bytes(), 原字节, "门 4：粘贴后撤销必须逐字节相等");
    assert!(!ed.dirty(), "撤销回保存点后不该脏");
    println!("GATE4 粘贴后撤销 {撤} 格 ⇒ to_bytes() 与原文件逐字节相等（{} 字节）", 原字节.len());
    // 重做回"已粘贴"状态（下面验的是**盘上**字节）
    assert_eq!(ed.redo(), 改动格);
    assert_eq!(ed.document().to_bytes(), 粘之后);

    // ⑦ 保存 + 逐字节核对：**只有目标 9 格变**
    let so = save_session(&mut ed).expect("保存应成功");
    assert!(so.wrote(), "有修改就必须真的写盘：{}", so.note());
    let 新字节 = std::fs::read(&副本).expect("读回保存后的文件");
    assert_eq!(新字节.len(), 原字节.len(), "长度必须不变");
    let 差异 = 差异清单(&原字节, &新字节);
    let mut 差异格: BTreeMap<(u16, u16), usize> = BTreeMap::new();
    for (off, a, b) in &差异 {
        assert!(*off >= 头长, "头部不许有差异：偏移 {off}（头长 {头长}）0x{a:02X}→0x{b:02X}");
        let (x, y) = 偏移转格(*off, 头长, 格长, h);
        assert!(
            目标.contains(x, y),
            "差异落在目标 3×3 之外：格 ({x},{y}) 偏移 {off} 0x{a:02X}→0x{b:02X}"
        );
        *差异格.entry((x, y)).or_default() += 1;
    }
    assert_eq!(
        差异格.len(),
        期望变化格,
        "差异涉及的格数必须等于期望变化格数（多一格就是写到了不该写的格）"
    );
    assert_eq!(
        差异.len(),
        逐格差异字节(&目标旧, &目标新),
        "差异字节数必须等于逐字段推导值"
    );
    println!(
        "GATE2 保存后：差异字节={} 差异格={} 头部差异=0（期望变化格={期望变化格}）",
        差异.len(),
        差异格.len()
    );

    // ⑧ 保存之后再撤销 ⇒ 内存字节回到原文件（此时该脏：盘上是"有粘贴"的那份）
    assert_eq!(ed.undo(), 改动格);
    assert_eq!(ed.document().to_bytes(), 原字节, "保存后撤销：内存字节回到原文件");
    assert!(ed.dirty(), "盘上是带粘贴的那份 ⇒ 内存退回后必须判脏");
    let _ = std::fs::remove_dir_all(&d);
}

// ═══════════════════════════ 门 2 补充（三族 + 三个库号） ═══════════════════════════

/// **门 2 补充**：合成 `New14` 图上，3×3 的三族图号 + **三个库号字节**逐格搬到目标块。
///
/// 为什么要这条：`0.map` 是 12 B/格（无库号字节）⇒ 库号字节的搬运在上面的门 2 里
/// **验不到**（见 `stamp_place.rs` 模块文档同一段说明）。这里用 14 B 档把三个字节钉死。
#[test]
fn 门2补充_三族与三个库号逐格搬运() {
    // 8×8 的 New14 合成图：每格三族图号可区分、三个库号也可区分
    let (w, h) = (8u16, 8u16);
    let mut 字节 = vec![0u8; 52 + 14 * (w as usize) * (h as usize)];
    字节[0..2].copy_from_slice(&w.to_le_bytes());
    字节[2..4].copy_from_slice(&h.to_le_bytes());
    for i in 0..(w as usize * h as usize) {
        let base = 52 + i * 14;
        let img = (i as u16).wrapping_add(1) & 0x7FFF;
        字节[base..base + 2].copy_from_slice(&img.to_le_bytes());
        字节[base + 2..base + 4].copy_from_slice(&(img.wrapping_add(100)).to_le_bytes());
        字节[base + 4..base + 6].copy_from_slice(&(img.wrapping_add(200)).to_le_bytes());
        字节[base + 12] = (i % 251) as u8; // btTiles
        字节[base + 13] = (i % 241) as u8; // btsmTiles
    }
    let d = 临时目录("gate2b");
    let p = d.join("new14.map");
    std::fs::write(&p, &字节).expect("写合成图");
    let mut ed = EditSession::load(&p).expect("打开合成图");
    assert_eq!(ed.document().format_id(), FormatId::Shanda14, "14 B 档");
    assert!(ed.document().disk_cell_len().is_some());
    let 原字节 = std::fs::read(&p).expect("读回");
    let 源 = Selection::new(1, 1, 3, 3);

    assert!(ed.set_selection(源));
    assert_eq!(ed.copy_selection(), Some((3, 3)));
    let clip = ed.clipboard().expect("剪贴板").clone();
    // 剪贴板里三个库号都非 0（14 B 档才抓得到）
    assert!(
        clip.cells().iter().all(|s| s.bk_lib != 0 && s.mid_lib != 0),
        "14 B 档必须抓到地表/中层库号"
    );
    // **锚点**：源左上角 (1,1)、地表可见 ⇒ 只有与源**同奇偶**的点击点才是锚点本身。
    // 这里故意点 (5,5)（奇,奇 ⇒ 与源同奇偶）：锚点 = 点击点，落点整块在图内且不与源块重叠。
    assert_eq!(clip.source(), Some((1, 1)), "源左上角必须是复制时的选区左上角");
    assert!(clip.ground(), "地表可见 ⇒ ground 为真");
    assert_eq!(clip.anchor(5, 5), Some((5, 5)), "同奇偶 ⇒ 不挪");
    assert_eq!(clip.anchor(4, 4), Some((3, 3)), "异奇偶 ⇒ 左上各退一格（公式对照）");
    let 目标 = Selection::new(5, 5, 7, 7);
    let 目标旧 = 快照(&ed, 目标);
    let out = ed.paste_clipboard(5, 5);
    assert!(matches!(out, PlaceOutcome::Placed { cells: 9, skipped: 0 }), "{out:?}");

    for s in clip.cells() {
        let key = ((5i32 + s.dx) as u16, (5i32 + s.dy) as u16);
        let 旧 = 目标旧[&key];
        let c = *ed.map().cell(key.0, key.1).expect("图内格");
        assert_eq!(c.bk_img, (旧.bk_img & 0x8000) | s.bk, "({},{}) 地表图号", key.0, key.1);
        assert_eq!(c.mid_img, s.mid, "({},{}) 中层图号", key.0, key.1);
        assert_eq!(c.fr_img, (旧.fr_img & 0x8000) | s.fr, "({},{}) 物体图号", key.0, key.1);
        assert_eq!(c.tiles, s.bk_lib, "({},{}) btTiles 逐格搬运", key.0, key.1);
        assert_eq!(c.sm_tiles, s.mid_lib, "({},{}) btsmTiles 逐格搬运", key.0, key.1);
        assert_eq!(c.area, s.fr_lib, "({},{}) btArea 逐格搬运", key.0, key.1);
    }
    // 撤销 ⇒ 逐字节相等
    assert_eq!(ed.undo(), 9);
    assert_eq!(ed.document().to_bytes(), 原字节, "撤销后必须逐字节相等");
    // 重做后保存 ⇒ 只有目标 9 格变
    assert_eq!(ed.redo(), 9);
    let so = save_session(&mut ed).expect("保存应成功");
    assert!(so.wrote(), "{}", so.note());
    let 新字节 = std::fs::read(&p).expect("读回");
    let 差异 = 差异清单(&原字节, &新字节);
    let 头长 = ed.document().raw_header().len();
    for (off, a, b) in &差异 {
        assert!(*off >= 头长, "头部不许有差异：{off} 0x{a:02X}→0x{b:02X}");
        let (x, y) = 偏移转格(*off, 头长, 14, h);
        assert!(目标.contains(x, y), "差异落在目标 3×3 之外：({x},{y})");
    }
    println!("GATE2B New14：差异字节={} 格={w}×{h}（三族+三个库号逐格搬运）", 差异.len());
    let _ = std::fs::remove_dir_all(&d);
}

// ═══════════════════════════ 门 3 + 门 4（剪切） ═══════════════════════════

/// **门 3**：剪切 3×3 → 保存 ⇒ **只有源 9 格被清**（按层语义），其余 0 差异。
///
/// **门 4**：剪切后撤销 ⇒ 逐字节相等。
#[test]
fn 门3门4_剪切3x3_只清源9格且撤销逐字节() {
    let Some(地图) = 地图路径() else {
        eprintln!("[skip] 无资产根 MIREDIT_ASSET_ROOT（或缺 GodotClient/maps/0.map）");
        return;
    };
    let d = 临时目录("gate3");
    let 副本 = d.join("0.map");
    std::fs::copy(&地图, &副本).expect("复制 0.map 副本");
    let 原字节 = std::fs::read(&副本).expect("读副本");
    let mut ed = EditSession::load(&副本).expect("打开副本");
    let h = ed.map().height;
    let 头长 = ed.document().raw_header().len();
    let 格长 = ed.document().disk_cell_len().expect("定长格式");

    // 源块：3×3、9 格地表都有图号（避开 (200,200)，免得与别的门混淆）
    let (sx, sy) = 找地表3x3(&ed, Selection::new(200, 200, 202, 202));
    let 源 = Selection::new(sx, sy, sx + 2, sy + 2);
    let 源旧 = 快照(&ed, 源);
    let mut 源新 = 格表::new();
    for (k, v) in &源旧 {
        源新.insert(*k, 期望清空(*v));
    }
    let 期望变化格 = 源新
        .iter()
        .filter(|(k, v)| 源旧.get(*k) != Some(*v))
        .count();

    assert!(ed.set_selection(源));
    let (cw, ch) = ed.cut_selection().expect("应能剪切");
    assert_eq!((cw, ch), (3, 3));
    assert_eq!(ed.undo_len(), 1, "整次剪切 = 一步撤销");
    assert_eq!(ed.last_clear(), Some(期望变化格), "清空回执 = 真的改了几格");

    // 逐格核对（三族按层语义）
    for (k, 旧) in &源旧 {
        let 实得 = *ed.map().cell(k.0, k.1).expect("图内格");
        let 期望 = 源新[k];
        assert_eq!(实得.bk_img, 期望.bk_img, "({},{}) 地表应清成 0/0x8000", k.0, k.1);
        assert_eq!(实得.mid_img, 期望.mid_img, "({},{}) 中层应清 0", k.0, k.1);
        assert_eq!(实得.fr_img, 期望.fr_img, "({},{}) 物体应清成 0/0x8000", k.0, k.1);
        // 阻挡位保留
        assert_eq!(实得.bk_img & 0x8000, 旧.bk_img & 0x8000, "({},{}) 地表 bit15 保留", k.0, k.1);
        assert_eq!(实得.fr_img & 0x8000, 旧.fr_img & 0x8000, "({},{}) 物体 bit15 保留", k.0, k.1);
        // 其它字段一律不动
        assert_eq!(实得.door_index, 旧.door_index);
        assert_eq!(实得.door_offset, 旧.door_offset);
        assert_eq!(实得.ani_frame, 旧.ani_frame, "动画帧不动（selection.rs §二 第 3 条）");
        assert_eq!(实得.ani_tick, 旧.ani_tick, "动画节拍不动");
        assert_eq!(实得.area, 旧.area);
        assert_eq!(实得.light, 旧.light);
        assert_eq!(实得.tail, 旧.tail);
    }

    // 门 4：撤销 ⇒ 逐字节相等
    let 剪之后 = ed.document().to_bytes();
    let 撤 = ed.undo();
    assert_eq!(撤, 期望变化格);
    assert_eq!(ed.document().to_bytes(), 原字节, "门 4：剪切后撤销必须逐字节相等");
    assert!(!ed.dirty());
    println!("GATE4 剪切后撤销 {撤} 格 ⇒ 逐字节相等（{} 字节）", 原字节.len());
    assert_eq!(ed.redo(), 期望变化格);
    assert_eq!(ed.document().to_bytes(), 剪之后);

    // 门 3：保存 ⇒ 只有源 9 格变
    let so = save_session(&mut ed).expect("保存应成功");
    assert!(so.wrote(), "{}", so.note());
    let 新字节 = std::fs::read(&副本).expect("读回");
    let 差异 = 差异清单(&原字节, &新字节);
    let mut 差异格: BTreeMap<(u16, u16), usize> = BTreeMap::new();
    for (off, a, b) in &差异 {
        assert!(*off >= 头长, "头部不许有差异：{off} 0x{a:02X}→0x{b:02X}");
        let (x, y) = 偏移转格(*off, 头长, 格长, h);
        assert!(源.contains(x, y), "差异落在源 3×3 之外：({x},{y})");
        *差异格.entry((x, y)).or_default() += 1;
    }
    assert_eq!(差异格.len(), 期望变化格, "差异格数必须等于期望变化格数");
    assert_eq!(差异.len(), 逐格差异字节(&源旧, &源新), "差异字节数必须等于逐字段推导值");
    println!(
        "GATE3 剪切保存后：差异字节={} 差异格={} 头部差异=0（源块={源:?}）",
        差异.len(),
        差异格.len()
    );
    let _ = std::fs::remove_dir_all(&d);
}

// ═══════════════════════════ 门 5（越界） ═══════════════════════════

/// **门 5**：选区越界 ⇒ 裁剪到图内；粘贴越界 ⇒ 逐格跳过并报 `skipped`。
#[test]
fn 门5_选区裁剪与粘贴越界跳过() {
    // 8×8 的 Old12 合成图（每格图号 = 格序 + 1）
    let (w, h) = (8u16, 8u16);
    let mut 字节 = vec![0u8; 52 + 12 * (w as usize) * (h as usize)];
    字节[0..2].copy_from_slice(&w.to_le_bytes());
    字节[2..4].copy_from_slice(&h.to_le_bytes());
    for i in 0..(w as usize * h as usize) {
        let base = 52 + i * 12;
        let img = (i as u16).wrapping_add(1);
        字节[base..base + 2].copy_from_slice(&img.to_le_bytes());
        字节[base + 2..base + 4].copy_from_slice(&img.wrapping_add(100).to_le_bytes());
        字节[base + 4..base + 6].copy_from_slice(&img.wrapping_add(200).to_le_bytes());
    }
    let d = 临时目录("gate5");
    let p = d.join("small.map");
    std::fs::write(&p, &字节).expect("写合成图");
    let mut ed = EditSession::load(&p).expect("打开合成图");
    let 原字节 = std::fs::read(&p).expect("读回");

    // ① 选区越界 ⇒ 裁剪（`MapViewport.cs:399` 的 Math.Clamp）
    assert!(ed.set_selection(Selection::new(6, 6, 99, 99)), "裁剪后仍有格 ⇒ 非空");
    assert_eq!(
        ed.selection(),
        Some(Selection::new(6, 6, 7, 7)),
        "必须裁到 (6,6)..(7,7)"
    );
    // 起点就越界 ⇒ 整块在图外 ⇒ 取消选区 + 中文原因
    assert!(!ed.set_selection(Selection::new(8, 0, 12, 4)), "整块在图外 ⇒ 空");
    assert_eq!(ed.selection(), None, "整块在图外 ⇒ 选区被取消");
    assert!(ed.note().is_some_and(|n| n.contains("地图外")), "必须给中文原因：{:?}", ed.note());
    assert_eq!(ed.document().to_bytes(), 原字节, "选区操作一个字节都不许动");

    // ② 复制一块 3×3，粘到 (7,7)。
    //    ⚠ 落点 = **锚点**（`RegionCopy.cs:70`）：源左上角 (0,0)、地表可见 ⇒ 点击点 (7,7)
    //    被对齐到 (6,6)，块覆盖 (6,6)..(8,8) ⇒ 8×8 图里只有 4 格在图内、5 格越界跳过。
    assert!(ed.set_selection(Selection::new(0, 0, 2, 2)));
    assert_eq!(ed.copy_selection(), Some((3, 3)));
    let clip = ed.clipboard().expect("剪贴板").clone();
    let (ax, ay) = clip.anchor(7, 7).expect("(7,7) 重算后是 (6,6)，不是负锚点");
    assert_eq!((ax, ay), (6, 6), "点击点 (7,7) 按源奇偶对齐到 (6,6)");
    // 期望表（独立推导）：锚点块里哪些格在图内、各自应变成什么
    let mut 期望: BTreeMap<(u16, u16), Cell> = BTreeMap::new();
    for s in clip.cells() {
        let (x, y) = (i32::from(ax) + s.dx, i32::from(ay) + s.dy);
        if x < 0 || y < 0 || x >= i32::from(w) || y >= i32::from(h) {
            continue;
        }
        let key = (x as u16, y as u16);
        let 旧 = *ed.map().cell(key.0, key.1).expect("图内格");
        期望.insert(key, 期望粘贴(旧, s));
    }
    assert_eq!(期望.len(), 4, "锚点块 (6,6)..(8,8) 与 8×8 图相交只剩 4 格");
    let out = ed.paste_clipboard(7, 7);
    match &out {
        PlaceOutcome::Placed { cells, skipped } => {
            assert_eq!(*skipped, 5, "9 格里 5 格在锚点块外（图外）⇒ 越界跳过");
            assert_eq!(*cells, 期望.len(), "只有图内那 4 格真的改了");
        }
        PlaceOutcome::Refused { reason } => panic!("有 4 格在图内，不该整体拒绝：{reason}"),
    }
    for (key, want) in &期望 {
        let got = *ed.map().cell(key.0, key.1).expect("图内格");
        assert_eq!(got.bk_img, want.bk_img, "({},{}) 地表按规则写", key.0, key.1);
        assert_eq!(got.mid_img, want.mid_img, "({},{}) 中层按规则写", key.0, key.1);
        assert_eq!(got.fr_img, want.fr_img, "({},{}) 物体按规则写", key.0, key.1);
    }
    assert_eq!(ed.undo_len(), 1, "越界跳过不影响'一步撤销'");
    // 撤销 ⇒ 逐字节相等（越界格本来就没写）
    assert_eq!(ed.undo(), 4);
    assert_eq!(ed.document().to_bytes(), 原字节, "撤销后逐字节相等");

    // ③ 全部越界 ⇒ Refused + 中文原因 + 一格不写
    //    （点击点 (100,100) 与源 (0,0) 同奇偶 ⇒ 锚点不动，整块都在图外）
    assert_eq!(clip.anchor(100, 100), Some((100, 100)), "同奇偶 ⇒ 锚点 = 点击点");
    let out = ed.paste_clipboard(100, 100);
    let reason = match &out {
        PlaceOutcome::Refused { reason } => reason.clone(),
        PlaceOutcome::Placed { cells, .. } => panic!("全越界必须拒绝，实际放了 {cells} 格"),
    };
    assert!(reason.contains("全部 9 格都在地图外"), "{reason}");
    assert_eq!(ed.undo_len(), 0, "拒绝不许产生撤销步");
    assert_eq!(ed.document().to_bytes(), 原字节);
    println!(
        "GATE5 选区裁剪 (6,6,99,99)→(6,6,7,7)；点击 (7,7) 锚点={ax},{ay} ⇒ skipped=5 改了 4 格；\
         全越界 ⇒ Refused「{reason}」"
    );
    let _ = std::fs::remove_dir_all(&d);
}

// ═══════════════════════════ 门 6（填充选区） ═══════════════════════════

/// **门 6**：选 3×3 → `fill_selection` ⇒ 9 格全变成笔刷值，且**一步撤销**。
///
/// 地表取 **step=1**（严格填满选区），与 C# `MapViewport.cs:540` + `:474-475` 的
/// `step=2` 有意不同 —— 理由见 `selection.rs` 模块文档 §五。
#[test]
fn 门6_填充选区9格() {
    let (w, h) = (8u16, 8u16);
    let mut 字节 = vec![0u8; 52 + 12 * (w as usize) * (h as usize)];
    字节[0..2].copy_from_slice(&w.to_le_bytes());
    字节[2..4].copy_from_slice(&h.to_le_bytes());
    for i in 0..(w as usize * h as usize) {
        let base = 52 + i * 12;
        let img = (i as u16).wrapping_add(1);
        字节[base..base + 2].copy_from_slice(&img.to_le_bytes());
        字节[base + 2..base + 4].copy_from_slice(&img.wrapping_add(100).to_le_bytes());
        字节[base + 4..base + 6].copy_from_slice(&img.wrapping_add(200).to_le_bytes());
    }
    let d = 临时目录("gate6");
    let p = d.join("fill.map");
    std::fs::write(&p, &字节).expect("写合成图");
    let mut ed = EditSession::load(&p).expect("打开合成图");
    let 原字节 = std::fs::read(&p).expect("读回");
    assert_eq!(ed.document().format_id(), FormatId::Old12);

    // 笔刷从 (0,0) 吸（当前层 = 地表层）；选区在 (4,4)..(6,6)，与 (0,0) 不重叠
    let brush = ed.pick_at(0, 0).expect("图内格");
    let 笔刷图号 = brush.sprite();
    let 源格 = *ed.map().cell(0, 0).expect("图内格");
    let 选区 = Selection::new(4, 4, 6, 6);
    let 旧 = 快照(&ed, 选区);
    assert!(ed.set_selection(选区));

    let n = ed.fill_selection();
    assert_eq!(n, 9, "3×3 必须 9 格全填（step=1，不溢出也不漏）");
    assert_eq!(ed.undo_len(), 1, "整次填充 = 一步撤销");
    assert_eq!(ed.last_fill().map(|f| f.cells), Some(9));
    for (k, 旧格) in &旧 {
        let c = *ed.map().cell(k.0, k.1).expect("图内格");
        assert_eq!(
            c.bk_img,
            (旧格.bk_img & 0x8000) | 笔刷图号,
            "({},{}) 应变成笔刷值（保留目标 bit15）",
            k.0,
            k.1
        );
        // 中层/物体不归地表层管 ⇒ 原样
        assert_eq!(c.mid_img, 旧格.mid_img, "({},{}) 中层不许动", k.0, k.1);
        assert_eq!(c.fr_img, 旧格.fr_img, "({},{}) 物体不许动", k.0, k.1);
    }
    // 选区外一格都不许动
    let 外 = *ed.map().cell(3, 3).expect("图内格");
    assert_ne!(外.bk_img & 0x7FFF, 笔刷图号, "(3,3) 在选区外，不许被填");
    // 撤销 ⇒ 逐字节相等
    assert_eq!(ed.undo(), 9);
    assert_eq!(ed.document().to_bytes(), 原字节, "撤销后必须逐字节相等");

    // 再填一次并保存 ⇒ 只有选区 9 格变
    assert_eq!(ed.fill_selection(), 9);
    let so = save_session(&mut ed).expect("保存应成功");
    assert!(so.wrote(), "{}", so.note());
    let 新字节 = std::fs::read(&p).expect("读回");
    let 头长 = ed.document().raw_header().len();
    for (off, a, b) in 差异清单(&原字节, &新字节) {
        assert!(off >= 头长, "头部不许有差异：{off} 0x{a:02X}→0x{b:02X}");
        let (x, y) = 偏移转格(off, 头长, 12, h);
        assert!(选区.contains(x, y), "差异落在选区之外：({x},{y})");
    }
    println!("GATE6 填充选区 3×3 ⇒ 9 格（笔刷图号={笔刷图号}，源格 bk=0x{:04X}）", 源格.bk_img);
    let _ = std::fs::remove_dir_all(&d);
}

// ═══════════════════════════ 门 7（Type5 闸门） ═══════════════════════════

/// 造一张 **Type5（`Mir3Wemade`）** 合成样本（与 `stamp_place.rs::造_type5` 逐字节同口径）。
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

/// **门 7**：合成 Type5 上粘贴**含 `bk≠0`** 的剪贴板 ⇒ 拒绝 + 中文原因；一格不写。
///
/// 反向对照：把剪贴板的地表清成 0（只剩中层/物体）⇒ 同一张图必须**放行**，
/// 证明"拒的是地表，不是整张图"。
#[test]
fn 门7_type5拒绝带地表剪贴板() {
    let d = 临时目录("gate7");
    let p = d.join("type5.map");
    let 原 = 造_type5(4, 2);
    std::fs::write(&p, &原).expect("写 Type5 合成样本");
    let mut ed = EditSession::load(&p).expect("Type5 应能打开");
    assert_eq!(ed.document().format_id(), FormatId::Mir3Wemade);
    assert!(!ed.ground_paint_supported());

    // Type5 的 `bk_img` 恒 0（`mir2-format/src/model.rs:430-440`）⇒ 直接写视图造一份
    // "带地表"的剪贴板（`document_mut` 是测试/工具专用的旁路，且 Type5 不编码 bk_img）。
    {
        let map = &mut ed.document_mut().view_mut();
        for k in 0..2 {
            let i = miredit_core::index_of(map, k, 0).expect("图内格");
            map.cells[i].bk_img = 7 + k;
            map.cells[i].mid_img = 20 + k;
            map.cells[i].fr_img = 30 + k;
        }
    }
    let 视图改后 = ed.document().to_bytes();
    assert!(ed.set_selection(Selection::new(0, 0, 1, 0)));
    assert_eq!(ed.copy_selection(), Some((2, 1)));
    let clip = ed.clipboard().expect("剪贴板").clone();
    assert!(clip.cells().iter().all(|s| s.bk != 0), "剪贴板必须带地表（bk≠0）");
    // 锚点（`RegionCopy.cs:70`）：源左上角 (0,0) 双偶 ⇒ 点在哪就贴到哪，本用例的落点不受锚点影响。
    // ⚠ 必须**显式**钉住：哪天选区的源变成奇数列，(0,0) 就会算出负锚点、整块被拒，
    // 那时这条用例会从"Refused(Type5)"变成"Refused(负锚点)"而**看起来还绿**。
    assert_eq!(clip.source(), Some((0, 0)), "源左上角");
    assert_eq!(clip.anchor(0, 0), Some((0, 0)), "双偶 ⇒ 不挪");

    // ① 带地表 ⇒ 拒绝
    let out = ed.paste_clipboard(0, 0);
    let reason = match &out {
        PlaceOutcome::Refused { reason } => reason.clone(),
        PlaceOutcome::Placed { cells, .. } => panic!("Type5 带地表必须拒绝，实际放了 {cells} 格"),
    };
    assert!(reason.contains("不支持逐格写地表背图"), "{reason}");
    assert!(reason.contains("bk≠0"), "{reason}");
    assert!(reason.contains("一个字节都没动"), "{reason}");
    assert_eq!(ed.undo_len(), 0, "拒绝不许产生撤销步");
    assert_eq!(ed.document().to_bytes(), 视图改后, "拒绝时字节必须一字节不动");
    let so = save_session(&mut ed).expect("保存应成功（走 Clean）");
    assert!(!so.wrote(), "没有修改就不许写盘");
    assert_eq!(std::fs::read(&p).unwrap(), 原, "盘上文件必须一字节不动");

    // ② 只有中层/物体（bk 全 0）⇒ 放行
    {
        let map = &mut ed.document_mut().view_mut();
        for k in 0..2 {
            let i = miredit_core::index_of(map, k, 0).expect("图内格");
            map.cells[i].bk_img = 0;
        }
    }
    assert_eq!(ed.copy_selection(), Some((2, 1)));
    // 粘到 (2,0)：目标格的 mid/fr 来自夹具（与剪贴板不同）⇒ 两格都真的变
    let out = ed.paste_clipboard(2, 0);
    assert!(matches!(out, PlaceOutcome::Placed { cells: 2, skipped: 0 }), "{out:?}");
    assert_eq!(ed.map().cell(2, 0).unwrap().mid_img, 20);
    assert_eq!(ed.map().cell(3, 0).unwrap().fr_img, 31);
    assert_eq!(ed.undo_len(), 1, "放行后是一步撤销");
    println!("GATE7 Type5：带地表 ⇒ Refused「{reason}」；只带中层/物体 ⇒ 放行 2 格");
    let _ = std::fs::remove_dir_all(&d);
}

// ═══════════════ A-016：粘贴锚点按**源的奇偶**重算（`RegionCopy.cs:70`） ═══════════════

/// 造 4×4 `Old12` 合成图（52 B 头 + 12 B/格，**列优先**：`idx = x*4 + y`）。
///
/// 夹具（源块 = (0,0)..(1,1)，只有 (0,0) 带地表 ⇒ "块里含地表"但点击点不该拿到它）：
///
/// | 格 | bk | mid | fr | 用途 |
/// |---|---|---|---|---|
/// | (0,0) | 0x42 | 0x10 | 0 | 源块左上角：**唯一**带地表的一格 |
/// | (1,0) | 0 | 0x11 | 0 | 源块：只有中层 |
/// | (0,1) | 0 | 0x12 | 0 | 源块：只有中层 |
/// | (1,1) | 0 | 0x13 | 0 | 源块：只有中层 |
/// | (2,2) | 0x77 | 0x78 | 0x79 | **哨兵**：旧行为（点击点=锚点）会把源 (1,1) 的中层写到这里 |
/// | (3,3) | 0x88 | 0 | 0 | 远处哨兵：任何情况下都不许被写 |
fn 造锚点样本() -> Vec<u8> {
    let (w, h) = (4u16, 4u16);
    let mut b = vec![0u8; 52 + 12 * (w as usize) * (h as usize)];
    b[0..2].copy_from_slice(&w.to_le_bytes());
    b[2..4].copy_from_slice(&h.to_le_bytes());
    fn 置(b: &mut [u8], h: u16, x: u16, y: u16, bk: u16, mid: u16, fr: u16) {
        let i = 52 + ((x * h + y) as usize) * 12;
        b[i..i + 2].copy_from_slice(&bk.to_le_bytes());
        b[i + 2..i + 4].copy_from_slice(&mid.to_le_bytes());
        b[i + 4..i + 6].copy_from_slice(&fr.to_le_bytes());
    }
    置(&mut b, h, 0, 0, 0x42, 0x10, 0);
    置(&mut b, h, 1, 0, 0, 0x11, 0);
    置(&mut b, h, 0, 1, 0, 0x12, 0);
    置(&mut b, h, 1, 1, 0, 0x13, 0);
    置(&mut b, h, 2, 2, 0x77, 0x78, 0x79);
    置(&mut b, h, 3, 3, 0x88, 0, 0);
    b
}

/// 把源块 4 格清成空（**测试旁路**：`document_mut().view_mut()`，不改 dirty、不进撤销栈）。
fn 清空源块(ed: &mut EditSession) {
    let map = ed.document_mut().view_mut();
    for (x, y) in [(0u16, 0u16), (1, 0), (0, 1), (1, 1)] {
        let i = miredit_core::index_of(map, x, y).expect("源块在图内");
        map.cells[i].bk_img = 0;
        map.cells[i].mid_img = 0;
        map.cells[i].fr_img = 0;
    }
}

/// **A-016**：粘贴落点 = `RegionCopy.cs:70` 的 `Anchor(点击格)`，**不是点击格本身**。
///
/// 上游逐字：
/// ```text
/// public (int X, int Y) Anchor(int x, int y)
///     => ground ? (x - ((x - sourceX) & 1), y - ((y - sourceY) & 1)) : (x, y);   // :70
/// var (x, y) = copiedRegion.Anchor(hover.X, hover.Y);                            // :121
/// if (!copiedRegion.Paste(Document, History, x, y, …)) { "Region extends outside the map" }  // :122
/// ```
///
/// 四条断言：
/// 1. **重算**：源左上角 (0,0)、地表可见 ⇒ 点 (1,1) 的锚点是 (0,0)，内容写到锚点块
///    `(0,0)..(1,1)`；点击点 (1,1) **拿不到**源的 (0,0) 格（若按旧行为"点=锚点"，
///    源 (0,0) 的地表 0x42 会落到点击点，源 (1,1) 的中层 0x13 会落到哨兵 (2,2)）；
/// 2. **无源剪贴板不重算**：`Clipboard::from_cells` 的 `source=None`/`ground=false` ⇒ 锚点原样；
/// 3. **地表不可见不重算**：只抓中层/物体的剪贴板（`ground=false`）⇒ 点 (1,1) 就贴到 (1,1)；
/// 4. **负锚点整块拒绝**：源 `sourceX` 为奇数 ⇒ 点 `x=0` 算出 `-1` ⇒ `anchor=None`、
///    `paste_clipboard` 返回 `Refused`、地图**一个字节不动**、剪贴板不被吃掉。
///
/// ⚠ 偏差：本 crate 的 `EditSession` **没有**公开的"灌剪贴板"入口（`clipboard()` 只读），
/// 所以第 2 条只能验到 `Clipboard::anchor` 这一层（= `paste_clipboard` 唯一消费它的地方）；
/// 会话级"把无源剪贴板真的粘下去"没有 API 可走，未造这层假入口。
// 用例名里的 `A016` 是台账编号（A-016），这里保留它以便按 id 检索；ASCII 大写会让
// `non_snake_case` 报警，而 `-D warnings` 门禁下必须显式豁免（只豁免这一条，不放宽整个文件）。
#[allow(non_snake_case)]
#[test]
fn A016_粘贴锚点按源奇偶() {
    let d = 临时目录("a016");
    let p = d.join("anchor.map");
    std::fs::write(&p, 造锚点样本()).expect("写合成图");
    let mut ed = EditSession::load(&p).expect("打开合成图");
    assert_eq!(ed.document().format_id(), FormatId::Old12);
    let 头长 = ed.document().raw_header().len();
    assert_eq!(头长, 52);
    let h = ed.map().height;

    // ── 1. 重算：源 (0,0) 的 2×2，点 (1,1) ⇒ 锚点 (0,0) ──
    assert!(ed.set_selection(Selection::new(0, 0, 1, 1)));
    assert_eq!(ed.copy_selection(), Some((2, 2)));
    let clip = ed.clipboard().expect("剪贴板").clone();
    assert_eq!(clip.source(), Some((0, 0)), "源左上角 = 复制时选区左上角（RegionCopy.cs:18）");
    assert!(clip.ground(), "地表可见（RegionCopy.cs:107）⇒ 要重算锚点");
    assert_eq!(clip.anchor(1, 1), Some((0, 0)), "点 (1,1) 按源奇偶退到 (0,0)");
    assert_eq!(clip.anchor(2, 2), Some((2, 2)), "同奇偶 ⇒ 不挪");

    // 清掉源块（否则"写到哪"会被"值本来就一样"掩盖）
    清空源块(&mut ed);
    let 清空后 = ed.document().to_bytes();
    assert_eq!(
        ed.map().cell(0, 0).unwrap().bk_img,
        0,
        "源块已清空（测试前提）"
    );

    let out = ed.paste_clipboard(1, 1);
    assert!(
        matches!(out, PlaceOutcome::Placed { cells: 4, skipped: 0 }),
        "2×2 全在图内、4 格都变：{out:?}"
    );
    // ① 锚点格被写入（源 (0,0) 的地表 + 中层）
    let 角 = *ed.map().cell(0, 0).expect("图内格");
    assert_eq!(角.bk_img, 0x42, "锚点格 (0,0) 必须拿到源左上角的地表");
    assert_eq!(角.mid_img, 0x10, "锚点格 (0,0) 必须拿到源左上角的中层");
    // ② 点击点**没有**拿到源的左上角内容（旧行为会拿到 ⇒ 这条就是 A-016 的判别式）
    let 点 = *ed.map().cell(1, 1).expect("图内格");
    assert_eq!(
        点.bk_img, 0,
        "点击点 (1,1) 不许拿到源 (0,0) 的地表 0x42 —— 旧行为（点=锚点）会写到这里"
    );
    assert_eq!(点.mid_img, 0x13, "点击点 (1,1) 该拿到源 (1,1) 的中层（整块按锚点搬）");
    // ③ 哨兵格一格不动（旧行为会把源 (1,1) 的 0x13 写到哨兵的 (2,2)）
    let 哨 = *ed.map().cell(2, 2).expect("图内格");
    assert_eq!((哨.bk_img, 哨.mid_img, 哨.fr_img), (0x77, 0x78, 0x79), "哨兵 (2,2) 不许被写");
    assert_eq!(ed.map().cell(3, 3).unwrap().bk_img, 0x88, "远处哨兵 (3,3) 不许被写");
    // ④ 逐字节：改动只落在锚点块 (0,0)..(1,1)
    let 粘后 = ed.document().to_bytes();
    let 差异 = 差异清单(&清空后, &粘后);
    assert!(!差异.is_empty(), "粘贴必须真的改字节");
    for (off, a, b) in &差异 {
        assert!(*off >= 头长, "头部不许有差异：{off} 0x{a:02X}→0x{b:02X}");
        let (x, y) = 偏移转格(*off, 头长, 12, h);
        assert!(
            x <= 1 && y <= 1,
            "差异必须全部落在锚点块 (0,0)..(1,1) 内，实际 ({} ,{}) 偏移 {off}",
            x,
            y
        );
    }
    assert_eq!(ed.undo_len(), 1, "整次粘贴 = 一步撤销");
    assert_eq!(ed.undo(), 4, "撤销 4 格");
    assert_eq!(ed.document().to_bytes(), 清空后, "撤销后逐字节回到粘贴前");

    // ── 2. 无源剪贴板不重算（`Clipboard::from_cells` ⇒ source=None/ground=false）──
    let 无源 = Clipboard::from_cells(
        2,
        2,
        clip.cells()
            .iter()
            .map(|s| StampCellInput {
                bk: 0x42,
                ..*s
            })
            .collect(),
    )
    .expect("2×2");
    assert_eq!(无源.source(), None);
    assert!(!无源.ground());
    assert_eq!(无源.anchor(1, 1), Some((1, 1)), "无源 ⇒ 不重算（= 旧行为）");
    assert_eq!(无源.anchor(0, 0), Some((0, 0)), "无源 ⇒ 永远不会算出负锚点");

    // ── 3. 地表不可见（只 mid/fr）⇒ 不重算，点 (1,1) 就贴 (1,1) ──
    let mut 无地表 = EditSession::load(&p).expect("重开合成图");
    无地表.set_visible_layers([false, true, true]);
    assert!(无地表.set_selection(Selection::new(0, 0, 1, 1)));
    assert_eq!(无地表.copy_selection(), Some((2, 2)));
    let 无地表剪贴板 = 无地表.clipboard().expect("剪贴板").clone();
    assert_eq!(无地表剪贴板.source(), Some((0, 0)), "源坐标照记");
    assert!(!无地表剪贴板.ground(), "地表隐藏 ⇒ ground=false");
    assert!(
        无地表剪贴板.cells().iter().all(|s| s.bk == 0),
        "地表隐藏 ⇒ 剪贴板里没有地表（RegionCopy.cs:107 的过滤）"
    );
    assert_eq!(
        无地表剪贴板.anchor(1, 1),
        Some((1, 1)),
        "地表不可见 ⇒ 上游 `ground ? … : (x, y)` 走后半支"
    );
    清空源块(&mut 无地表);
    let out = 无地表.paste_clipboard(1, 1);
    assert!(
        matches!(out, PlaceOutcome::Placed { cells: 4, skipped: 0 }),
        "按点击点 (1,1) 贴、4 格都变：{out:?}"
    );
    assert_eq!(无地表.map().cell(0, 0).unwrap().mid_img, 0, "(0,0) 不该被写（没重算）");
    assert_eq!(无地表.map().cell(1, 1).unwrap().mid_img, 0x10, "(1,1) 拿到源 (0,0) 的中层");

    // ── 4. 负锚点：源 sourceX 为奇数、点 x=0 ⇒ 整块拒绝 ──
    let mut 负 = EditSession::load(&p).expect("重开合成图");
    // 源块 (1,2)..(2,3)：夹具里这几格大多为空 ⇒ 先填上可区分的内容，
    // 否则"同一块贴回原处"值不变、`cells` 会是 0，验不出"换合法落点就放行"。
    // （`view_mut` 是测试旁路：不改 dirty、不进撤销栈。）
    let 侧块 = [(1u16, 2u16), (2, 2), (1, 3), (2, 3)];
    {
        let map = 负.document_mut().view_mut();
        for (k, (x, y)) in 侧块.into_iter().enumerate() {
            let i = miredit_core::index_of(map, x, y).expect("图内格");
            map.cells[i].bk_img = 0; // 这块只给中层/物体（锚点判据不依赖"有没有地表"）
            map.cells[i].mid_img = 0x30 + k as u16;
            map.cells[i].fr_img = 0x40 + k as u16;
        }
    }
    assert!(负.set_selection(Selection::new(1, 2, 2, 3)), "源 (1,2)..(2,3)：sourceX=1 为奇数");
    assert_eq!(负.copy_selection(), Some((2, 2)));
    let 负剪贴板 = 负.clipboard().expect("剪贴板").clone();
    assert_eq!(负剪贴板.source(), Some((1, 2)));
    assert_eq!(负剪贴板.anchor(0, 3), None, "x: 0 - ((0-1)&1) = -1 ⇒ 负锚点");
    assert_eq!(负剪贴板.anchor(0, 2), None, "y 同奇偶也要整块拒绝，不许只挪一边");
    assert_eq!(负剪贴板.anchor(1, 2), Some((1, 2)), "同奇偶 ⇒ 合法");
    // 清空源块 ⇒ 下面"放行"才有可观测改动
    {
        let map = 负.document_mut().view_mut();
        for (x, y) in 侧块 {
            let i = miredit_core::index_of(map, x, y).expect("图内格");
            map.cells[i].bk_img = 0;
            map.cells[i].mid_img = 0;
            map.cells[i].fr_img = 0;
        }
    }
    let 负前 = 负.document().to_bytes();
    let out = 负.paste_clipboard(0, 3);
    let reason = match &out {
        PlaceOutcome::Refused { reason } => reason.clone(),
        PlaceOutcome::Placed { cells, .. } => panic!("负锚点必须整块拒绝，实际放了 {cells} 格"),
    };
    assert!(reason.contains("锚点为负"), "原因要点名负锚点：{reason}");
    assert!(reason.contains("CanPaste"), "原因要指向 C# 的 CanPaste：{reason}");
    assert!(reason.contains("一个字节都没动"), "原因要给出没动地图的保证：{reason}");
    assert_eq!(负.undo_len(), 0, "拒绝不许产生撤销步");
    assert_eq!(负.document().to_bytes(), 负前, "拒绝时地图必须逐字节不变");
    assert!(负.clipboard().is_some(), "拒绝不许吃掉剪贴板（粘完还能再粘）");
    // 同一份剪贴板换个合法落点 ⇒ 放行（证明拒的是锚点，不是剪贴板坏了）
    let out = 负.paste_clipboard(1, 2);
    assert!(matches!(out, PlaceOutcome::Placed { cells: 4, skipped: 0 }), "{out:?}");
    println!(
        "A016 锚点：点(1,1)→(0,0) 写锚点块、点击点未被写；无源/无地表不重算；\
         负锚点 ⇒ Refused「{reason}」"
    );
    let _ = std::fs::remove_dir_all(&d);
}

// ═══════════════════════════ 附：命令层契约 ═══════════════════════════

/// 七条选区/剪贴板命令都走 `apply` 通路，且**每条都带中文回执**（`cmd.rs` 模块文档 §三）。
#[test]
fn 附_选区命令链路() {
    let (w, h) = (8u16, 8u16);
    let mut 字节 = vec![0u8; 52 + 12 * (w as usize) * (h as usize)];
    字节[0..2].copy_from_slice(&w.to_le_bytes());
    字节[2..4].copy_from_slice(&h.to_le_bytes());
    for i in 0..(w as usize * h as usize) {
        let base = 52 + i * 12;
        let img = (i as u16).wrapping_add(1);
        字节[base..base + 2].copy_from_slice(&img.to_le_bytes());
        字节[base + 2..base + 4].copy_from_slice(&img.wrapping_add(100).to_le_bytes());
        字节[base + 4..base + 6].copy_from_slice(&img.wrapping_add(200).to_le_bytes());
    }
    let d = 临时目录("cmd");
    let p = d.join("cmd.map");
    std::fs::write(&p, &字节).expect("写合成图");
    let mut ed = EditSession::load(&p).expect("打开合成图");
    let 原字节 = ed.document().to_bytes();

    // ① 没选区就复制/删除/填充 ⇒ changed=0 + 中文原因，状态一个都不改
    for cmd in [
        Command::CopySelection,
        Command::CutSelection,
        Command::DeleteSelection,
        Command::FillSelection,
    ] {
        let out = apply(&mut ed, cmd).expect("不是程序故障");
        assert_eq!(out.changed, 0, "没有选区 ⇒ 0 格");
        let note = out.note.expect("拒绝必须给原因");
        assert!(note.contains("没有选区"), "{note}");
        assert!(!out.dirty && !out.can_undo, "拒绝不许改状态");
        assert_eq!(ed.document().to_bytes(), 原字节);
    }
    // ② 设选区 ⇒ 不改地图、不置脏、不产生撤销步
    let out = apply(
        &mut ed,
        Command::SetSelection {
            x0: 1,
            y0: 1,
            x1: 3,
            y1: 3,
        },
    )
    .expect("设选区不该 Err");
    assert_eq!(out.changed, 0);
    assert!(out.note.unwrap().contains("3×3"));
    assert_eq!(ed.selection(), Some(Selection::new(1, 1, 3, 3)));
    assert!(!out.dirty && !out.can_undo);
    // ③ 复制 ⇒ 0 格但给"已复制 3×3"
    let out = apply(&mut ed, Command::CopySelection).expect("复制不该 Err");
    assert_eq!(out.changed, 0);
    assert!(out.note.unwrap().contains("已复制 3×3"));
    assert!(!out.dirty);
    // ④ 粘贴 ⇒ 9 格 + 一步撤销
    //    ⚠ 落点 = 锚点（`RegionCopy.cs:70`）：源左上角 (1,1)、地表可见 ⇒ 点击 (4,4) 被对齐到 (3,3)，
    //    回执里的坐标必须是**实际锚点**（报点击点就是假陈述）。
    assert_eq!(
        ed.clipboard().and_then(|c| c.anchor(4, 4)),
        Some((3, 3)),
        "点击 (4,4) 与源 (1,1) 异奇偶 ⇒ 锚点 (3,3)"
    );
    let out = apply(&mut ed, Command::PasteSelection { x: 4, y: 4 }).expect("粘贴不该 Err");
    assert_eq!(out.changed, 9);
    let note = out.note.unwrap();
    assert!(note.contains("粘贴 9 格到 (3,3)"), "回执必须报**实际锚点**：{note}");
    assert!(note.contains("点击点 (4,4) 已按源奇偶对齐"), "回执要说清点击点被对齐：{note}");
    assert_eq!(ed.undo_len(), 1);
    // ⑤ 把选区挪到刚粘的那块（锚点 (3,3) 起 3×3），删除 ⇒ 9 格（三族按层清）+ 第二步撤销
    apply(
        &mut ed,
        Command::SetSelection {
            x0: 3,
            y0: 3,
            x1: 5,
            y1: 5,
        },
    )
    .expect("设选区");
    let out = apply(&mut ed, Command::DeleteSelection).expect("删除不该 Err");
    assert_eq!(out.changed, 9, "刚粘贴的 9 格都有内容 ⇒ 全清");
    assert!(out.note.unwrap().contains("删除选区内容 9 格"));
    assert_eq!(ed.undo_len(), 2);
    // ⑥ 填充选区（先吸笔刷，再把选区挪到空白处）⇒ 9 格
    apply(&mut ed, Command::Pick { x: 0, y: 0 }).expect("吸管");
    apply(
        &mut ed,
        Command::SetSelection {
            x0: 5,
            y0: 5,
            x1: 7,
            y1: 7,
        },
    )
    .expect("设选区");
    let out = apply(&mut ed, Command::FillSelection).expect("填充不该 Err");
    assert_eq!(out.changed, 9);
    assert!(out.note.unwrap().contains("填充选区 9 格"));
    assert_eq!(ed.undo_len(), 3);
    // ⑦ 取消选区 ⇒ 0 格、剪贴板保留（仍可再粘）
    let out = apply(&mut ed, Command::ClearSelection).expect("取消选区不该 Err");
    assert_eq!(out.changed, 0);
    assert!(out.note.unwrap().contains("已取消选区"));
    assert_eq!(ed.selection(), None);
    assert!(ed.clipboard().is_some(), "取消选区不许清剪贴板");
    // ⑦ 还能再粘：点击 (1,5) 与源 (1,1) 同奇偶 ⇒ 锚点 = 点击点，(1,5) 起 9 格都在图内。
    //    ⚠ 不能点 (0,5)：x=0 与源 x=1 异奇偶 ⇒ 锚点 x 算成 -1 ⇒ 负锚点整块拒绝
    //    （`RegionCopy.cs:70-71` 的 `CanPaste`），那条行为由 `A016_粘贴锚点按源奇偶` 专门验。
    assert_eq!(
        ed.clipboard().and_then(|c| c.anchor(0, 5)),
        None,
        "点击 (0,5) 会算出负锚点（源 x=1 是奇数）"
    );
    let out = apply(&mut ed, Command::PasteSelection { x: 1, y: 5 }).expect("还能粘");
    assert_eq!(out.changed, 9, "剪贴板仍是 3×3，(1,5) 起 9 格都在图内");
    // ⑧ 越界选区 ⇒ 裁剪（不 Err）
    let out = apply(
        &mut ed,
        Command::SetSelection {
            x0: 6,
            y0: 6,
            x1: 99,
            y1: 99,
        },
    )
    .expect("越界不是程序故障");
    assert_eq!(ed.selection(), Some(Selection::new(6, 6, 7, 7)), "必须裁到图内");
    assert!(out.note.unwrap().contains("2×2"));
    println!("CMD 七条选区命令全部走通（changed / note / dirty / can_undo 均符合契约）");
    let _ = std::fs::remove_dir_all(&d);
}

/// **回执不许自相矛盾**（2026-11-05 实测缺陷的回归门）。
///
/// 实测原文（修前）：
/// `剪切 5×4 格（20 格被清空，已进剪贴板）；剪切 0 格（5×4）已进剪贴板；选区 … 里没有可清的内容…未改动`
/// —— 同一句里既说"20 格被清空"（假）又说"未改动"，根因是**两段回执被拼接**。
///
/// 现在的契约：`CutSelection` 的 note **只出一句**，复制数与清空数都在里面；
/// `changed` 报**真的清了几格**（不许沿用上一次 `DeleteSelection` 的旧回执）。
#[test]
fn 附_剪切回执不出假话() {
    let (w, h) = (8u16, 8u16);
    let mut 字节 = vec![0u8; 52 + 12 * (w as usize) * (h as usize)];
    字节[0..2].copy_from_slice(&w.to_le_bytes());
    字节[2..4].copy_from_slice(&h.to_le_bytes());
    for i in 0..(w as usize * h as usize) {
        let base = 52 + i * 12;
        let img = (i as u16).wrapping_add(1);
        字节[base..base + 2].copy_from_slice(&img.to_le_bytes());
        字节[base + 2..base + 4].copy_from_slice(&img.wrapping_add(100).to_le_bytes());
        字节[base + 4..base + 6].copy_from_slice(&img.wrapping_add(200).to_le_bytes());
    }
    let d = 临时目录("cutnote");
    let p = d.join("cutnote.map");
    std::fs::write(&p, &字节).expect("写合成图");
    let mut ed = EditSession::load(&p).expect("打开合成图");

    // 选区 (1,1)..(3,3)：9 格都有内容
    apply(
        &mut ed,
        Command::SetSelection {
            x0: 1,
            y0: 1,
            x1: 3,
            y1: 3,
        },
    )
    .expect("设选区");
    // ① 先删除 ⇒ 真的清 9 格
    let del = apply(&mut ed, Command::DeleteSelection).expect("删除");
    assert_eq!(del.changed, 9, "9 格都有内容 ⇒ 全清");
    assert_eq!(ed.undo_len(), 1);
    let 清空后 = ed.document().to_bytes();

    // ② 再剪切：复制得到 9 格，但**清空 0 格**（内容已经在上一步被清光了）
    let cut = apply(&mut ed, Command::CutSelection).expect("剪切不该 Err");
    assert_eq!(cut.changed, 0, "一格都没清 ⇒ changed 必须是 0（不许沿用上一次的 9）");
    let note = cut.note.expect("必须有回执");
    assert!(note.contains("选区内容本来就是空的"), "要点明原因：{note}");
    assert!(!note.contains("被清空"), "不许出现『N 格被清空』这种假话：{note}");
    assert!(!note.contains("未改动"), "复制确实发生了，不许说『未改动』：{note}");
    assert!(note.contains("已复制 9 格进剪贴板"), "复制数要如实给出：{note}");
    assert_eq!(ed.undo_len(), 1, "没清东西 ⇒ 不许产生第二步撤销");
    assert_eq!(ed.document().to_bytes(), 清空后, "字节必须与剪切前一致");
    assert!(ed.clipboard().is_some(), "剪切必须先真的复制到剪贴板");

    // ③ 反向对照：有内容时剪切 ⇒ changed = 真清掉的格数
    assert!(ed.set_selection(Selection::new(4, 4, 6, 6)));
    let cut2 = apply(&mut ed, Command::CutSelection).expect("剪切");
    assert_eq!(cut2.changed, 9, "这一块有内容 ⇒ 清 9 格");
    let note2 = cut2.note.expect("必须有回执");
    assert!(note2.contains("清空 9 格"), "{note2}");
    assert!(!note2.contains("本来就是空的"), "{note2}");
    println!("NOTE 剪切回执：空选区 ⇒「{note}」；有内容 ⇒「{note2}」");
    let _ = std::fs::remove_dir_all(&d);
}

/// 选区/剪贴板是 **UI 状态**：**不置脏、不产生撤销步、不动一个字节**。
///
/// 来源：`MapViewport.cs:147`/`:152`（`Selection` 与 `History` **并列**存进 `MapEditorState`）。
/// 这条是给 web 那路"看到 `dirty:true` 就以为选区置脏了"的误判钉一个反例。
#[test]
fn 附_选区操作不置脏不产生撤销步() {
    let (w, h) = (4u16, 4u16);
    let mut 字节 = vec![0u8; 52 + 12 * (w as usize) * (h as usize)];
    字节[0..2].copy_from_slice(&w.to_le_bytes());
    字节[2..4].copy_from_slice(&h.to_le_bytes());
    for i in 0..(w as usize * h as usize) {
        let base = 52 + i * 12;
        let img = (i as u16).wrapping_add(1);
        字节[base..base + 2].copy_from_slice(&img.to_le_bytes());
    }
    let d = 临时目录("clean");
    let p = d.join("clean.map");
    std::fs::write(&p, &字节).expect("写合成图");
    let mut ed = EditSession::load(&p).expect("打开合成图");
    let 原字节 = ed.document().to_bytes();
    assert!(!ed.dirty() && ed.undo_len() == 0, "刚打开：干净、无撤销步");

    for cmd in [
        Command::SetSelection {
            x0: 1,
            y0: 1,
            x1: 2,
            y1: 2,
        },
        Command::CopySelection,
        Command::ClearSelection,
        Command::SetSelection {
            x0: 9,
            y0: 9,
            x1: 12,
            y1: 12,
        }, // 整块越界 ⇒ 取消选区
    ] {
        let out = apply(&mut ed, cmd.clone()).expect("选区命令不该 Err");
        assert_eq!(out.changed, 0, "{cmd:?} 不改地图");
        assert!(!out.dirty, "{cmd:?} 不许置脏");
        assert!(!out.can_undo, "{cmd:?} 不许产生撤销步");
        assert!(!ed.dirty(), "{cmd:?} 之后会话仍必须干净");
        assert_eq!(ed.undo_len(), 0, "{cmd:?} 之后撤销栈必须为空");
        assert_eq!(ed.document().to_bytes(), 原字节, "{cmd:?} 一个字节都不许动");
    }
    println!("CLEAN 四条选区命令跑完：dirty=false / undo_len=0 / 字节未变");
    let _ = std::fs::remove_dir_all(&d);
}

/// `Clipboard::from_cells` 的公开闸门（网页层把上次复制的区域带回来时用）。
#[test]
fn 附_剪贴板公开构造() {
    let cells = vec![
        StampCellInput {
            dx: 0,
            dy: 0,
            bk: 5,
            ..Default::default()
        },
        StampCellInput {
            dx: 1,
            dy: 0,
            bk: 6,
            ..Default::default()
        },
    ];
    let c = Clipboard::from_cells(2, 1, cells.clone()).expect("2×1");
    assert_eq!((c.width(), c.height()), (2, 1));
    assert_eq!(c.cells().len(), 2);
    // 无源剪贴板**不做奇偶重算**（2026-11-06）：`source`/`ground` 都缺 ⇒ 锚点原样
    assert_eq!(c.source(), None, "from_cells 不知道复制时选区在哪");
    assert!(!c.ground(), "from_cells 没有『复制时地表可见』这回事");
    for (x, y) in [(0u16, 0u16), (1, 1), (0, 1), (1, 0)] {
        assert_eq!(c.anchor(x, y), Some((x, y)), "无源 ⇒ 锚点就是点击点：({x},{y})");
    }
    assert!(Clipboard::from_cells(3, 1, cells).is_none(), "格数对不上必须拒绝");
}

// ══════════════ 验收门（2026-11-05 新增）：粘贴路径**不套**占用闸门 ══════════════

/// 在图上找一块 3×3，满足三件事：
/// ① **至少有一格带物件**（`fr_img & 0x7FFF != 0`）—— 这样印章路径才会被占用闸门拒；
/// ② 源格表里**带物件的那一格正好落在目标块已有物件的位置**（同一相对偏移）——
///    这样粘贴之后能拿到"原有物件被源物件覆盖"的**直接证据**，而不是只证明"粘贴没被拒"；
/// ③ 落点与源**同奇偶** ⇒ 粘贴锚点就是落点本身（`RegionCopy.cs:70` 的 `(x-sourceX)&1 == 0`），
///    否则整块会被锚点挪一格，②的"同一相对偏移"前提就不成立了。
fn 找有物件3x3(
    ed: &EditSession,
    避开: Selection,
    源原点: (u16, u16),
    源: &[StampCellInput],
) -> (u16, u16) {
    let (w, h) = (ed.map().width, ed.map().height);
    let (sx, sy) = 源原点;
    for y in 0..h.saturating_sub(2) {
        for x in 0..w.saturating_sub(2) {
            // ③ 同奇偶（锚点 = 落点）
            if (x.wrapping_sub(sx)) & 1 != 0 || (y.wrapping_sub(sy)) & 1 != 0 {
                continue;
            }
            let sel = Selection::new(x, y, x + 2, y + 2);
            if sel.contains(避开.x0, 避开.y0)
                || sel.contains(避开.x1, 避开.y1)
                || 避开.contains(x, y)
                || 避开.contains(x + 2, y + 2)
            {
                continue;
            }
            let mut 有物件 = false;
            let mut 物件正好重叠 = false;
            for s in 源 {
                let key = (x as i32 + s.dx, y as i32 + s.dy);
                if key.0 < 0 || key.1 < 0 || key.0 >= i32::from(w) || key.1 >= i32::from(h) {
                    continue;
                }
                let Some(c) = ed.map().cell(key.0 as u16, key.1 as u16) else {
                    continue;
                };
                if c.fr_img & 0x7FFF != 0 {
                    有物件 = true;
                    if s.fr != 0 {
                        物件正好重叠 = true;
                    }
                }
            }
            if 有物件 && 物件正好重叠 {
                return (x, y);
            }
        }
    }
    panic!("0.map 上找不到「有物件且与源物件位置重叠」的 3×3（测试前提不成立）");
}

/// **验收门（粘贴路径不套占用闸门）**：`RegionCopy.Paste` 的前置只有 `CanPaste`
/// （`RegionCopy.cs:71` 的 `document.Contains(x,y) && document.Contains(x+Width-1,y+Height-1)`，
/// **纯边界判断**），没有 `CanPlaceStamp` 的占用闸门 —— 粘贴就是**覆盖**语义。
///
/// 同一落点、同一份格表，只换入口，做三段对照：
/// 1. `place_stamp`（`PlanMode::Stamp`）⇒ **整枚拒绝**（落点已有物件，`PlacedStamps.cs:62-63`）；
/// 2. `paste_clipboard`（`PlanMode::Paste`）⇒ **成功**，并把落点内容覆盖成源块；
/// 3. 同点再粘一次（内容相同）⇒ 仍然**成功**（粘贴路径没有"第二次被拒"这回事），
///    值没变 ⇒ 0 格、不产生空步。
#[test]
fn 粘贴路径不套占用闸门() {
    let Some(地图) = 地图路径() else {
        eprintln!("[skip] 无资产根 MIREDIT_ASSET_ROOT（或缺 GodotClient/maps/0.map）");
        return;
    };
    let d = 临时目录("paste_nogate");
    let 副本 = d.join("0.map");
    std::fs::copy(&地图, &副本).expect("复制 0.map 副本");
    let 原字节 = std::fs::read(&副本).expect("读副本");
    let mut ed = EditSession::load(&副本).expect("打开 0.map 副本");

    // 源块：3×3、9 格地表都有图号（与后面挑的目标块不重叠）
    let (sx, sy) = 找地表3x3(&ed, Selection::new(500, 500, 502, 502));
    let 源 = Selection::new(sx, sy, sx + 2, sy + 2);
    assert!(ed.set_selection(源));
    assert_eq!(ed.copy_selection(), Some((3, 3)));
    let clip = ed.clipboard().expect("剪贴板").clone();

    // 目标块：3×3、至少一格已有物件，且源物件正好落在其中一格上（能验"覆盖"）。
    // 选块时就要求与源**同奇偶** ⇒ 下面 `clip.anchor(tx,ty) == (tx,ty)` 成立，
    // 三段对照（印章/粘贴/再粘）才都落在同一块上。
    let (tx, ty) = 找有物件3x3(&ed, 源, (sx, sy), clip.cells());
    let 目标 = Selection::new(tx, ty, tx + 2, ty + 2);
    let 目标旧 = 快照(&ed, 目标);
    let 原有物件 = 目标旧.values().filter(|c| c.fr_img & 0x7FFF != 0).count();
    assert!(原有物件 >= 1, "目标块必须本来就有物件（否则验不到闸门差异）");
    // 锚点契约：落点与源同奇偶 ⇒ 锚点 = 落点（这样"同一落点"的三段对照才成立）
    assert_eq!(
        clip.anchor(tx, ty),
        Some((tx, ty)),
        "同奇偶 ⇒ 锚点必须等于点击点（源左上角 ({sx},{sy})）"
    );

    // ① 印章路径在同一落点 ⇒ 占用闸门整枚拒绝
    let out = ed.place_stamp(tx, ty, clip.cells());
    let reason = match &out {
        PlaceOutcome::Refused { reason } => reason.clone(),
        PlaceOutcome::Placed { cells, .. } => {
            panic!("落点有物件，印章路径必须拒绝，实际放了 {cells} 格")
        }
    };
    assert!(reason.contains("已有物件"), "{reason}");
    assert_eq!(ed.undo_len(), 0, "拒绝不许产生撤销步");
    assert_eq!(ed.document().to_bytes(), 原字节, "拒绝时字节必须一字节不动");

    // ② 粘贴路径在同一落点 ⇒ 成功（覆盖）
    let out = ed.paste_clipboard(tx, ty);
    let 改动格 = match &out {
        PlaceOutcome::Placed { cells, skipped } => {
            assert_eq!(*skipped, 0, "3×3 全在图内");
            *cells
        }
        PlaceOutcome::Refused { reason } => {
            panic!("粘贴不许套占用闸门，却被拒：{reason}")
        }
    };
    assert_eq!(ed.undo_len(), 1, "整次粘贴 = 一步撤销");
    let mut 源带物件 = 0usize;
    let mut 覆盖掉原有物件 = 0usize;
    for s in clip.cells() {
        let key = ((tx as i32 + s.dx) as u16, (ty as i32 + s.dy) as u16);
        let 旧 = 目标旧[&key];
        let 实得 = *ed.map().cell(key.0, key.1).expect("图内格");
        if s.bk != 0 {
            assert_eq!(
                实得.bk_img,
                (旧.bk_img & 0x8000) | s.bk,
                "({},{}) 地表 = 源",
                key.0,
                key.1
            );
        }
        if s.mid != 0 {
            assert_eq!(实得.mid_img, s.mid, "({},{}) 中层 = 源", key.0, key.1);
        }
        if s.fr != 0 {
            源带物件 += 1;
            assert_eq!(
                实得.fr_img,
                (旧.fr_img & 0x8000) | s.fr,
                "({},{}) 物体 = 源（覆盖）",
                key.0,
                key.1
            );
            // `RegionCopy.cs:86`：有前景 ⇒ 门/门偏移无条件搬
            assert_eq!(实得.door_index, s.door, "({},{}) 门索引", key.0, key.1);
            assert_eq!(实得.door_offset, s.door_offset, "({},{}) 门偏移", key.0, key.1);
            if 旧.fr_img & 0x7FFF != 0 {
                覆盖掉原有物件 += 1;
            }
        }
        // `RegionCopy.cs:84-86` 没有 `Light` ⇒ 光照必须是目标格原值
        assert_eq!(实得.light, 旧.light, "({},{}) 光照不许被粘贴改", key.0, key.1);
    }
    assert!(源带物件 > 0, "源块必须有物件，否则验不到'覆盖'");
    assert!(
        覆盖掉原有物件 > 0,
        "必须有一格「原有物件被源物件覆盖」（选块判据 ② 保证了这点）"
    );
    let 粘一次 = ed.document().to_bytes();
    assert_ne!(粘一次, 原字节, "粘贴必须真的改字节");
    assert!(改动格 > 0, "至少要改一格");

    // ③ 同点再粘一次（内容相同）⇒ 仍然成功；值没变 ⇒ 0 格、不产生空步
    let out = ed.paste_clipboard(tx, ty);
    assert!(matches!(out, PlaceOutcome::Placed { cells: 0, skipped: 0 }), "{out:?}");
    assert_eq!(ed.undo_len(), 1, "值没变 ⇒ 空步不入栈");
    assert_eq!(ed.document().to_bytes(), 粘一次);

    // ④ 撤销一次 ⇒ 逐字节回到原文件
    assert_eq!(ed.undo(), 改动格);
    assert_eq!(ed.document().to_bytes(), 原字节, "撤销后必须逐字节相等");
    println!(
        "GATE-PASTE 目标块 ({tx},{ty}) 原有物件 {原有物件} 格 ⇒ 印章路径 Refused「{reason}」；\
         粘贴路径 Placed {改动格} 格（源带物件 {源带物件} 格，覆盖掉原有物件 {覆盖掉原有物件} 格）；\
         同点再粘 ⇒ Placed 0 格不增步"
    );
    let _ = std::fs::remove_dir_all(&d);
}
