//! **C 5 层计数**验收：官方用例语义 + 九格式归一化 + 全部改格路径 + 拒绝路径 + 整块重建。
//!
//! 逐条对照：
//!
//! | 用例 | 官方出处 |
//! |------|----------|
//! | `官方用例_替换删除与越界` | `CrystalStudio.Tests/LayerCountTests.cs:9-24@acfbbf8a` |
//! | `旧族_只有前景bit15不算高位阻挡` | 同上 `:13-14@acfbbf8a` + `LegacyMapReader.cs:153-161@acfbbf8a` |
//! | `crystalcs_字面值五层各一` | 同上 `:12-14@acfbbf8a`（u32 位路径） |
//! | `type5_标志位与地表拒绝` | `Document.LayerCounts.cs:18-22@acfbbf8a` + `LegacyMapReader.cs:438-439@acfbbf8a` |
//! | 其余 | 本仓 `layer_counts.rs` 模块文档第三节 |
//!
//! ⚠ 本文件**只读合成字节**（不碰磁盘真实地图），也不改任何生产代码路径。

use std::path::{Path, PathBuf};

use miredit_core::{
    set_brush_from_palette, Document, EditLayer, EditSession, FillOutcome, LayerCounts, LibRef,
    PlaceOutcome, Selection, StampCellInput, Tool,
};
use mir2_format::{format_by_id, Cell as RawCell, FormatId, RawMap};
use mir2_map::Cell as ViewCell;

/// 合成文档的诊断路径（不落盘）。
const 假路径: &str = r"I:\__miredit_core_layer_counts__\synth.map";

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

// ───────────────────────────── 夹具 ─────────────────────────────

/// 造一张**空 Old12** 图的字节（52 字节头 + 12 B/格，列优先）。
///
/// 12 B 格布局（`mir2_map` 口径）：`bk 0..2`、`mid 2..4`、`fr 4..6`、`door 6`、
/// `door_offset 7`、`ani_frame 8`、`ani_tick 9`、`area 10`、`light 11`。
fn 空old12(w: u16, h: u16) -> Vec<u8> {
    let mut b = vec![0u8; 52 + 12 * usize::from(w) * usize::from(h)];
    b[0..2].copy_from_slice(&w.to_le_bytes());
    b[2..4].copy_from_slice(&h.to_le_bytes());
    b
}

/// 改 Old12 图里的一格（三个图号；其余字段留 0）。
///
/// ⚠ 列优先：`i = x * h + y`（`mir2-map/src/map.rs` §存储顺序）。
/// 改 Old12 图里的一格（三个图号；其余字段留 0）。
///
/// 参数打成三个元组（尺寸 / 坐标 / 三族图号）：8 个裸参数会撞 clippy 的
/// `too_many_arguments`，而元组让「哪三个是一组」在调用点就看得出来。
///
/// ⚠ 列优先：`i = x * h + y`（`mir2-map/src/map.rs` §存储顺序）。
fn 置格(
    b: &mut [u8],
    尺寸: (u16, u16),
    坐标: (u16, u16),
    图号: (u16, u16, u16),
) {
    let ((w, h), (x, y), (bk, mid, fr)) = (尺寸, 坐标, 图号);
    debug_assert!(x < w && y < h, "置格越界：({x},{y}) 不在 {w}×{h} 内");
    let i = usize::from(x) * usize::from(h) + usize::from(y);
    let base = 52 + i * 12;
    b[base..base + 2].copy_from_slice(&bk.to_le_bytes());
    b[base + 2..base + 4].copy_from_slice(&mid.to_le_bytes());
    b[base + 4..base + 6].copy_from_slice(&fr.to_le_bytes());
}

/// Old12 字节 → 文档。
fn 文档(字节: Vec<u8>) -> Document {
    Document::from_bytes(Path::new(假路径), 字节).expect("合成 Old12 图应能解析")
}

/// 五层计数的简写（断言里读起来像官方 `layerCounts[5]`）。
fn 计(doc: &Document) -> (usize, usize, usize, usize, usize) {
    let c = doc.layer_counts();
    (
        c.ground,
        c.middle,
        c.front,
        c.low_collision,
        c.high_collision,
    )
}

/// 直接构造期望值（字段名比位置更难写错）。
fn 五层(g: usize, m: usize, f: usize, lo: usize, hi: usize) -> LayerCounts {
    LayerCounts {
        ground: g,
        middle: m,
        front: f,
        low_collision: lo,
        high_collision: hi,
    }
}

/// 造一张 `CrystalCs` 合成图的字节（`ver` 决定格长，`(x,y,bk,mid,fr)` 逐格覆盖）。
///
/// 头部 8 字节：`ver i16 @0`、`'C' '#' @2..4`、`w i16 @4`、`h i16 @6`
/// （`mir2-format/src/formats/crystal_cs.rs:7-12` 的头部表）。
fn 造_crystalcs(w: u16, h: u16, ver: i16, 格: &[(u16, u16, u32, u16, u16)]) -> Vec<u8> {
    let mut header = vec![0u8; 8];
    header[0..2].copy_from_slice(&ver.to_le_bytes());
    header[2] = b'C';
    header[3] = b'#';
    header[4..6].copy_from_slice(&w.to_le_bytes());
    header[6..8].copy_from_slice(&h.to_le_bytes());
    // `ver >= 2` 的格布局末尾有 12 个「未建模」字节（`crystal_cs.rs` 头部表），
    // writer 要求 `tail.len()` **恰好**等于该版本应消费的字节数。
    let tail_len = if ver >= 2 { 12 } else { 0 };
    let mut cells = vec![
        RawCell {
            tail: vec![0u8; tail_len],
            ..RawCell::default()
        };
        usize::from(w) * usize::from(h)
    ];
    for &(x, y, bk, mid, fr) in 格 {
        let i = usize::from(x) * usize::from(h) + usize::from(y);
        cells[i].bk_img = bk;
        cells[i].mid_img = mid;
        cells[i].fr_img = fr;
    }
    let raw = RawMap {
        format: FormatId::CrystalCs,
        width: w,
        height: h,
        header_raw: header,
        cells,
        trailer: Vec::new(),
    };
    format_by_id(FormatId::CrystalCs)
        .write(&raw)
        .expect("CrystalCs 合成图应能写出")
}

/// 造一张 **Type5（`Mir3Wemade`）** 合成图：`flags[i]` 是第 i 格（列优先）的 `flag` 字节。
///
/// 布局：28 字节头（首字节 0 = 无标题，`Width @22`、`Height @24`）+ 半分辨率背景块
/// `3 * ceil(w/2) * floor(h/2)` + `14 × w × h` 格（照 `tests/formats_9.rs:609-632` 的夹具）。
fn 造_type5(w: i16, h: i16, flags: &[u8]) -> Vec<u8> {
    let (wu, hu) = (w as u16, h as u16);
    let 块长 = 3 * ((usize::from(wu) / 2) + (usize::from(wu) % 2)) * (usize::from(hu) / 2);
    let n = 28 + 块长 + 14 * usize::from(wu) * usize::from(hu);
    let mut b = vec![0u8; n];
    b[0] = 0; // 无标题
    b[20..22].copy_from_slice(&1i16.to_le_bytes()); // Attribute
    b[22..24].copy_from_slice(&w.to_le_bytes());
    b[24..26].copy_from_slice(&h.to_le_bytes());
    let 起 = 28 + 块长;
    debug_assert_eq!(flags.len(), usize::from(wu) * usize::from(hu), "flags 必须逐格一个");
    for (i, &flag) in flags.iter().enumerate() {
        let base = 起 + i * 14;
        b[base] = flag;
        b[base + 3] = 255; // btArea：C# 的 frindex = -1（本仓物体计数不读它，见模块文档 §5.4）
        b[base + 5..base + 7].copy_from_slice(&(100 + i as u16).to_le_bytes()); // mid_img
        b[base + 7..base + 9].copy_from_slice(&(200 + i as u16).to_le_bytes()); // fr_img
        b[base + 12] = 0x0F; // light
    }
    b
}

// ───────────────────── ① 官方用例（LayerCountTests.cs:9-24） ─────────────────────

/// 官方 `CountsTrackReplacementRemovalAndIgnoreImageFlags` 的语义复现（Old12 载体）。
///
/// 官方用 `BackImage = 0x60000001`（bit29/30 + 图号 1）造「五层各 1」；Old12 的磁盘格只有
/// 16 位，等价物是 `bk = 0x8001`（bit15 → 装载期搬到 bit29）+ `fr = 0x8001`（bit15 → bit30）。
#[test]
fn 官方用例_替换删除与越界() {
    let mut b = 空old12(4, 4);
    // (0,0)：地表 bit15 + 图号 1、中层 1、前景 bit15 + 图号 1 ⇒ 五层各 1
    置格(&mut b, (4, 4), (0, 0), (0x8001, 1, 0x8001));
    // (1,1)：奇锚点、图号 1、前景只有 bit15 ⇒ 五层都不加（官方 :13-14 的同一个坑）
    置格(&mut b, (4, 4), (1, 1), (1, 0, 0x8000));
    let mut doc = 文档(b);
    assert_eq!(
        计(&doc),
        (1, 1, 1, 1, 1),
        "五层各 1（LayerCountTests.cs:12-14@acfbbf8a）"
    );

    // 换成「只有低位阻挡」的格（官方 `BackImage = 0x20000000` 的等价物）
    let 空 = ViewCell::default();
    let 只低位 = ViewCell {
        bk_img: 0x8000,
        ..ViewCell::default()
    };
    assert!(doc.write_view_cell(0, 0, 只低位), "换格必须真的写进去");
    assert_eq!(
        计(&doc),
        (0, 0, 0, 1, 0),
        "低位=1，其余 0（LayerCountTests.cs:15-20@acfbbf8a）"
    );

    // 清空 + 越界 Set（官方 `map.Set(-1, 0, …)` 必须被忽略）
    assert!(doc.write_view_cell(0, 0, 空));
    assert_eq!(计(&doc), (0, 0, 0, 0, 0));
    assert!(!doc.write_view_cell(4, 0, 只低位), "越界写必须返回 false");
    assert!(!doc.write_view_cell(0, 4, 只低位), "越界写必须返回 false");
    assert!(!doc.write_view_cell(0, 0, 空), "no-op 必须返回 false");
    assert_eq!(
        计(&doc),
        (0, 0, 0, 0, 0),
        "越界 / no-op 一格都不许改计数（LayerCountTests.cs:21-23@acfbbf8a）"
    );
}

/// u32 字面值路径：`bk = 0x60000001` 在 `CrystalCs` 里同样让五层各 1。
///
/// ⚠ 不再往下做「换成 0x20000000」那一步：`CrystalCs` 的 bit29/30 住在 `raw.bk_img`
/// 的高 16 位，视图（`u16`）根本改不到它 —— 这正是本仓「低位/高位阻挡按格式归一化、
/// 不按视图判」的边界（模块文档第三节）。
#[test]
fn crystalcs_字面值五层各一() {
    let b = 造_crystalcs(4, 4, 2, &[(0, 0, 0x6000_0001, 1, 1)]);
    let doc = 文档(b);
    assert_eq!(doc.format_id(), FormatId::CrystalCs);
    assert_eq!(
        计(&doc),
        (1, 1, 1, 1, 1),
        "(0,0) = bk 0x60000001 / mid 1 / fr 1 ⇒ 五层各 1（同 LayerCountTests.cs:12-14）"
    );
}

/// legacy 四族：只有 bit15 的前景**不算**物体，也**不算**高位阻挡（官方用例反证）。
#[test]
fn 旧族_只有前景bit15不算高位阻挡() {
    let mut b = 空old12(2, 2);
    置格(&mut b, (2, 2), (0, 0), (0x8001, 0, 0x8000));
    let mut doc = 文档(b);
    assert_eq!(
        计(&doc),
        (1, 0, 0, 1, 0),
        "前景只有 bit15 ⇒ 物体 0、高位阻挡 0（LegacyMapReader.cs:153-161@acfbbf8a）"
    );
    // 补上低 15 位 ⇒ 物体 + 高位阻挡各 +1，地表/中层/低位不变
    let mut 后 = *doc.view().cell(0, 0).unwrap();
    后.fr_img = 0x8001;
    assert!(doc.write_view_cell(0, 0, 后));
    assert_eq!(计(&doc), (1, 0, 1, 1, 1));
    // 再退回只有 bit15 ⇒ 计数精确退回
    后.fr_img = 0x8000;
    assert!(doc.write_view_cell(0, 0, 后));
    assert_eq!(计(&doc), (1, 0, 0, 1, 0));
}

// ───────────────────── ② Type5：标志位 + 地表拒绝 ─────────────────────

/// Type5（Mir3）：低位/高位阻挡来自 `flag` 的 bit0/bit1（极性：0 才是阻挡）；
/// 地表逐格落笔被格式能力拒绝 ⇒ 计数一格不变。
#[test]
fn type5_标志位与地表拒绝() {
    // 2×2，四格 flag 依次 0 / 1 / 2 / 3（列优先）
    let doc = 文档(造_type5(2, 2, &[0, 1, 2, 3]));
    assert_eq!(doc.format_id(), FormatId::Mir3Wemade);
    assert_eq!(
        计(&doc),
        (0, 4, 4, 2, 2),
        "flag bit0=0 的 2 格算低位、bit1=0 的 2 格算高位；中层/前景各 4 格（bk_img 恒 0）"
    );
    let mut ed = EditSession::new(doc);
    set_brush_from_palette(&mut ed, LibRef::ground(0), 9);
    ed.set_layer(EditLayer::Ground);
    let 前 = 计(ed.document());
    assert_eq!(ed.apply_once(0, 0), 0, "Type5 地表必须拒绝落笔");
    assert!(
        ed.note().unwrap_or_default().contains("Type5"),
        "拒绝必须给中文原因：{:?}",
        ed.note()
    );
    assert_eq!(计(ed.document()), 前, "被拒绝的落笔不许改计数");
    assert_eq!(ed.undo_len(), 0, "被拒绝的落笔不许产生撤销步");
}

// ───────────────────── ③ 变更路径：画笔 / 橡皮 ─────────────────────

/// 画笔五层 + 橡皮：每一步都断言五层计数**精确值**。
#[test]
fn 落笔与橡皮_计数精确() {
    let mut ed = EditSession::new(文档(空old12(6, 6)));
    assert_eq!(计(ed.document()), (0, 0, 0, 0, 0));

    // ① 地表（偶偶锚点）
    set_brush_from_palette(&mut ed, LibRef::ground(0), 9); // 帧 9 ⇒ 图号 10
    assert_eq!(ed.apply_once(0, 0), 1);
    assert_eq!(计(ed.document()), (1, 0, 0, 0, 0));
    // ② 奇数锚点：图号写进去了，但地表计数不变
    assert_eq!(ed.apply_once(1, 0), 1);
    assert_eq!(计(ed.document()), (1, 0, 0, 0, 0), "奇数锚点不算地表");
    // ③ 中层
    ed.set_layer(EditLayer::Middle);
    set_brush_from_palette(&mut ed, LibRef::middle(0), 9);
    assert_eq!(ed.apply_once(2, 2), 1);
    assert_eq!(计(ed.document()), (1, 1, 0, 0, 0));
    // ④ 物体
    ed.set_layer(EditLayer::Front);
    set_brush_from_palette(&mut ed, LibRef::surface(0), 9);
    assert_eq!(ed.apply_once(3, 3), 1);
    assert_eq!(计(ed.document()), (1, 1, 1, 0, 0));
    // ⑤ 地表再来一格（偶偶）
    ed.set_layer(EditLayer::Ground);
    set_brush_from_palette(&mut ed, LibRef::ground(0), 9);
    assert_eq!(ed.apply_once(4, 4), 1);
    assert_eq!(计(ed.document()), (2, 1, 1, 0, 0));
    // ⑥ 低位阻挡：阻挡层只改 bit15，且只接受「从地图吸来的」笔刷
    assert!(ed.pick_at(4, 4).is_some(), "吸管取 (4,4) 的笔刷");
    ed.set_layer(EditLayer::LowCollision);
    assert_eq!(ed.apply_once(4, 4), 1);
    assert_eq!(计(ed.document()), (2, 1, 1, 1, 0), "(4,4) 的 bk 变成 0x800A");
    // ⑦ 高位阻挡：前景先有图号（低 15 位非 0），再置 bit15
    ed.set_layer(EditLayer::Front);
    set_brush_from_palette(&mut ed, LibRef::surface(0), 9);
    assert_eq!(ed.apply_once(5, 5), 1);
    assert_eq!(计(ed.document()), (2, 1, 2, 1, 0));
    assert!(ed.pick_at(5, 5).is_some());
    ed.set_layer(EditLayer::HighCollision);
    assert_eq!(ed.apply_once(5, 5), 1);
    assert_eq!(
        计(ed.document()),
        (2, 1, 2, 1, 1),
        "前景 0x800A ⇒ 物体仍 1、高位阻挡 +1"
    );

    // ⑧ 橡皮（地表）：清图号、保留 bit15
    ed.set_layer(EditLayer::Ground);
    set_brush_from_palette(&mut ed, LibRef::ground(0), 9);
    ed.set_tool(Tool::Eraser);
    assert_eq!(ed.apply_once(0, 0), 1);
    assert_eq!(计(ed.document()), (1, 1, 2, 1, 1), "(0,0) 图号 10 → 0：地表 -1");
    assert_eq!(ed.apply_once(4, 4), 1);
    assert_eq!(
        计(ed.document()),
        (0, 1, 2, 1, 1),
        "橡皮把 0x800A 擦成 0x8000：地表 -1、低位 +0（bit15 保留）"
    );
    assert_eq!(
        ed.map().cell(4, 4).unwrap().bk_img,
        0x8000,
        "橡皮保留 bit15"
    );

    // ⑨ 撤销 / 重做对称
    assert_eq!(ed.undo(), 1);
    assert_eq!(计(ed.document()), (1, 1, 2, 1, 1));
    assert_eq!(ed.undo(), 1, "再撤一步 = 撤销 (0,0) 的橡皮");
    assert_eq!(计(ed.document()), (2, 1, 2, 1, 1));
    assert_eq!(ed.undo(), 1, "撤销高位阻挡那一笔");
    assert_eq!(计(ed.document()), (2, 1, 2, 1, 0));
    assert_eq!(ed.redo(), 1);
    assert_eq!(计(ed.document()), (2, 1, 2, 1, 1));
}

// ───────────────────── ④ 填充 / 印章 / 粘贴 / 删除 ─────────────────────

/// 地表填充：BFS 只走偶偶格（6×6 ⇒ 9 格），撤销/重做计数对称。
#[test]
fn 填充_地表计数等于偶偶格数() {
    let mut ed = EditSession::new(文档(空old12(6, 6)));
    set_brush_from_palette(&mut ed, LibRef::ground(0), 9);
    assert_eq!(ed.fill(0, 0), FillOutcome::Filled(9), "6×6 的偶偶格 = 9");
    assert_eq!(计(ed.document()), (9, 0, 0, 0, 0));
    assert_eq!(ed.undo(), 9);
    assert_eq!(计(ed.document()), (0, 0, 0, 0, 0), "撤销后计数必须回基线");
    assert_eq!(ed.redo(), 9);
    assert_eq!(计(ed.document()), (9, 0, 0, 0, 0), "重做后计数必须恢复");
}

/// 印章：三格（两格地表 + 一格中层）⇒ 计数按锚点与族精确变化；撤销/重做对称。
#[test]
fn 印章_计数精确且撤销对称() {
    let mut ed = EditSession::new(文档(空old12(4, 4)));
    let 印 = vec![
        StampCellInput {
            dx: 0,
            dy: 0,
            bk: 5,
            ..StampCellInput::default()
        },
        StampCellInput {
            dx: 1,
            dy: 1,
            bk: 6,
            ..StampCellInput::default()
        },
        StampCellInput {
            dx: 1,
            dy: 0,
            mid: 3,
            ..StampCellInput::default()
        },
    ];
    let out = ed.place_stamp(0, 0, &印);
    assert_eq!(out, PlaceOutcome::Placed { cells: 3, skipped: 0 });
    assert_eq!(
        计(ed.document()),
        (1, 1, 0, 0, 0),
        "(0,0) 偶偶地表、 (1,1) 奇奇不算地表、 (1,0) 中层"
    );
    assert_eq!(ed.undo(), 3);
    assert_eq!(计(ed.document()), (0, 0, 0, 0, 0));
    assert_eq!(ed.redo(), 3);
    assert_eq!(计(ed.document()), (1, 1, 0, 0, 0));
}

/// 粘贴 + 删除选区：每一步计数精确；两次撤销分别退回「删除」与「粘贴」。
#[test]
fn 粘贴与删除选区_计数精确() {
    // 源：2×2 三族全有（bk=5 / mid=2 / fr=3）
    let mut b = 空old12(4, 4);
    for (x, y) in [(0u16, 0u16), (0, 1), (1, 0), (1, 1)] {
        置格(&mut b, (4, 4), (x, y), (5, 2, 3));
    }
    let mut ed = EditSession::new(文档(b));
    assert_eq!(
        计(ed.document()),
        (1, 4, 4, 0, 0),
        "源块 2×2 里只有 (0,0) 是偶偶锚点"
    );

    assert!(ed.set_selection(Selection::new(0, 0, 1, 1)));
    assert_eq!(ed.copy_selection(), Some((2, 2)));
    let out = ed.paste_clipboard(2, 2);
    assert_eq!(out, PlaceOutcome::Placed { cells: 4, skipped: 0 });
    assert_eq!(
        计(ed.document()),
        (2, 8, 8, 0, 0),
        "粘贴块 (2,2)..(3,3) 里只有 (2,2) 是偶偶锚点；中层/物体各 +4"
    );

    // 删除粘出来的那一块 ⇒ 精确退回
    assert!(ed.set_selection(Selection::new(2, 2, 3, 3)));
    assert_eq!(ed.delete_selection(), 4);
    assert_eq!(计(ed.document()), (1, 4, 4, 0, 0));
    assert_eq!(ed.undo(), 4);
    assert_eq!(计(ed.document()), (2, 8, 8, 0, 0));
    assert_eq!(ed.undo(), 4, "再撤一步 = 撤销整次粘贴");
    assert_eq!(计(ed.document()), (1, 4, 4, 0, 0));
}

// ───────────────────── ⑤ 拒绝路径：计数一格不变 ─────────────────────

/// 族不匹配 / 图库笔刷落阻挡层 / 越界 / no-op：四种拒绝都不许改计数。
#[test]
fn 拒绝路径_计数一格不变() {
    let mut ed = EditSession::new(文档(空old12(4, 4)));
    let 基线 = 计(ed.document());

    // ① 族不匹配：物体族笔刷画到地表层
    set_brush_from_palette(&mut ed, LibRef::surface(0), 9);
    ed.set_layer(EditLayer::Ground);
    assert_eq!(ed.apply_once(0, 0), 0);
    assert_eq!(计(ed.document()), 基线);
    assert!(
        ed.note().unwrap_or_default().contains("不一致"),
        "族不匹配必须给原因：{:?}",
        ed.note()
    );

    // ② 图库笔刷落阻挡层
    set_brush_from_palette(&mut ed, LibRef::ground(0), 9);
    ed.set_layer(EditLayer::LowCollision);
    assert_eq!(ed.apply_once(0, 0), 0);
    assert_eq!(计(ed.document()), 基线);
    assert!(
        ed.note().unwrap_or_default().contains("只改 0x8000 位"),
        "图库笔刷落阻挡层必须给原因：{:?}",
        ed.note()
    );

    // ③ 越界
    ed.set_layer(EditLayer::Ground);
    set_brush_from_palette(&mut ed, LibRef::ground(0), 9);
    assert_eq!(ed.apply_once(9, 9), 0);
    assert_eq!(计(ed.document()), 基线);

    // ④ no-op：同一格同值再落一次
    assert_eq!(ed.apply_once(0, 0), 1);
    assert_eq!(计(ed.document()), (1, 0, 0, 0, 0));
    assert_eq!(ed.apply_once(0, 0), 0, "第二遍是 no-op");
    assert_eq!(
        计(ed.document()),
        (1, 0, 0, 0, 0),
        "no-op 不许把计数改两遍"
    );
    assert_eq!(ed.undo_len(), 1, "no-op 不许产生撤销步");
}

// ───────────────────── ⑥ 整块换视图后重建 ─────────────────────

/// 模拟 GUI 的 `mem::swap(document.view_mut(), &mut map)`：不调 rebuild 就是**旧计数**，
/// 调了之后与新视图一致，且之后的增量更新继续正确。
#[test]
fn 整块换视图后重建计数() {
    let mut b = 空old12(4, 4);
    置格(&mut b, (4, 4), (0, 0), (1, 0, 0));
    let mut doc = 文档(b);
    assert_eq!(计(&doc), (1, 0, 0, 0, 0));

    // 另一张同尺寸图：每格 bk=0x8001 / mid=1 / fr=0x8001
    let mut 新 = 空old12(4, 4);
    for y in 0..4u16 {
        for x in 0..4u16 {
            置格(&mut 新, (4, 4), (x, y), (0x8001, 1, 0x8001));
        }
    }
    let mut 新视图 = mir2_map::Map::parse(&新).expect("新图应能解析");
    std::mem::swap(doc.view_mut(), &mut 新视图);

    // 换视图绕过了 write_view_cell ⇒ 计数还是旧的（这正是「必须 rebuild」的理由）
    assert_eq!(计(&doc), (1, 0, 0, 0, 0), "未 rebuild 时计数停在旧视图上");
    assert_eq!(doc.rebuild_layer_counts(), 五层(4, 16, 16, 16, 16));
    assert_eq!(doc.layer_counts(), 五层(4, 16, 16, 16, 16));

    // rebuild 之后增量更新继续正确
    assert!(doc.write_view_cell(0, 0, ViewCell::default()));
    assert_eq!(计(&doc), (3, 15, 15, 15, 15));
    assert_eq!(doc.rebuild_layer_counts(), 五层(3, 15, 15, 15, 15), "增量 == 全量");
}

// ───────────────────── ⑦ 不变量 ─────────────────────

/// **真实语料回归**：`0.map`（700×700 Old12）的五层计数必须与逐格手算一致。
///
/// 实测（2026-11-06 本机）：`ground 122,500 / middle 9,347 / front 28,660 /
/// low 162,820 / high 63`（490,000 格）。
///
/// 这条用例专门钉住「**照抄磁盘 bit29 会得到假计数**」这个坑：`Old12` 的
/// `raw.bk_img` 只有 16 位 ⇒ 磁盘 bit29 恒 0；而磁盘 bit15 有 **162,820** 格
/// （官方口径的 bit29 是装载期从 bit15 归一化出来的）⇒ 实现一旦退回
/// `raw.bk_img & 0x2000_0000`，`low` 会从 162,820 掉到 0。
#[test]
fn 真实地图_零号图五层计数() {
    let Some(root) = 资产根() else {
        eprintln!("[skip] 无资产根 MIREDIT_ASSET_ROOT");
        return;
    };
    let 真图 = root
        .join("client")
        .join("热血传奇23周年客户端")
        .join("Map")
        .join("0.map");
    if !真图.is_file() {
        eprintln!("[skip] 缺文件 {}", 真图.display());
        return;
    }
    let bytes = std::fs::read(&真图)
        .unwrap_or_else(|e| panic!("真值地图读不到：{} ⇒ {e}", 真图.display()));
    let mut doc = Document::from_bytes(&真图, bytes).expect("真值地图应能解析");
    assert_eq!(doc.format_id(), FormatId::Old12, "0.map 是 12 B/格");
    let c = doc.layer_counts();
    assert_eq!(
        (
            c.ground,
            c.middle,
            c.front,
            c.low_collision,
            c.high_collision
        ),
        (122_500, 9_347, 28_660, 162_820, 63),
        "0.map 的五层计数（逐格手算，见用例文档）"
    );
    // 陷阱自证：本格式 raw.bk_img 的高 16 位恒 0
    assert_eq!(
        doc.raw()
            .cells
            .iter()
            .filter(|c| (c.bk_img & 0x2000_0000) != 0)
            .count(),
        0,
        "Old12 的 raw.bk_img 是 u16 ⇒ 磁盘 bit29 恒 0（照抄就是假计数）"
    );
    // 增量（构造时算的）与全量重建必须逐格相等
    assert_eq!(doc.rebuild_layer_counts(), c, "增量 == 全量（490,000 格逐格对拍）");
}

/// 计数之和 ≤ 5×格数；不相干的层不被误改；打开时「构造即计数」。
#[test]
fn 计数不变量与构造即计数() {
    let mut b = 空old12(6, 6);
    置格(&mut b, (6, 6), (0, 0), (0x8001, 1, 0x8001));
    置格(&mut b, (6, 6), (2, 2), (7, 0, 7));
    let mut ed = EditSession::new(文档(b));
    let c = ed.layer_counts();
    assert!(c.total() <= 5 * 36, "五层之和不可能超过 5×格数：{c:?}");
    assert_eq!(
        c,
        五层(2, 1, 2, 1, 1),
        "(0,0) 五层全占；(2,2) 地表+物体；中层只有 (0,0)"
    );
    let 第一次 = ed.document().layer_counts();
    let 第二次 = ed.document().layer_counts();
    assert_eq!(第一次, 第二次, "读两次必须一样（纯缓存读）");

    // 只画中层：其余四层一格都不许动
    ed.set_layer(EditLayer::Middle);
    set_brush_from_palette(&mut ed, LibRef::middle(0), 9);
    let 前 = ed.layer_counts();
    assert_eq!(ed.apply_once(4, 4), 1);
    let 后 = ed.layer_counts();
    assert_eq!(后.ground, 前.ground);
    assert_eq!(后.front, 前.front);
    assert_eq!(后.low_collision, 前.low_collision);
    assert_eq!(后.high_collision, 前.high_collision);
    assert_eq!(后.middle, 前.middle + 1);
    assert!(后.total() <= 5 * 36);

    // 地表填充后再验证一次不变量（数量级更大的路径）
    ed.set_layer(EditLayer::Ground);
    set_brush_from_palette(&mut ed, LibRef::ground(0), 9);
    ed.fill(0, 0);
    let c = ed.layer_counts();
    assert!(c.total() <= 5 * 36, "{c:?}");
}
