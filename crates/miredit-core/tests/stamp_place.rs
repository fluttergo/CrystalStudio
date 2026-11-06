//! 印章放置的**验收门 2~7**（真实印章 + 真实地图 + 逐字节自证）。
//!
//! 跑法：`cargo test -p miredit-core --test stamp_place -- --nocapture --test-threads=1`
//!
//! # 六个门（与任务书逐条对应）
//!
//! | 门 | 判据 | 用例 |
//! |----|------|------|
//! | 2 | 真实印章 `mir_00000.X`（25 格 / 包围盒 `(-1,-2,4,3)`）贴进 `0.map` 副本的 `(100,100)` ⇒ 保存后**只有印章覆盖的格变**、`bit15` 保留、头部 0 差异 | [`门2门3_真实印章贴0map副本_逐字节核对与撤销`]、[`门2补充_真实印章贴带bit15合成图`] |
//! | 3 | 放置后**撤销** ⇒ `to_bytes()` 与原文件**逐字节相等** | 同门 2 |
//! | 4 | 同一枚印章贴到 `(0,0)`（负偏移出界）⇒ 报 `cells`/`skipped`，出界的格没写 | [`门4_越界裁剪_贴到原点`] |
//! | 5 | 贴到地图外 ⇒ `Refused`，文件一字节不动 | [`门5_全越界拒绝且文件一字节不动`] |
//! | 6 | 合成 Type5 样本上贴含 `bk != 0` 的印章 ⇒ `Refused` + 中文原因 | [`门6_type5拒绝带地表印章`] |
//! | 7 | 两格印章放同一点两次 ⇒ 第二次覆盖 / 不产生空步；撤销一次回到放之前 | [`门7_重叠与撤销一次`] |
//!
//! # 为什么期望值要在测试里**独立推导**一遍
//!
//! 如果直接拿 `place_stamp` 的结果跟自己比，这条测试永远绿 —— 那是假门。
//! 所以本文件按**规则**（不是按实现）重算每个格应该变成什么：
//!
//! ```text
//! 地表 bk：印章 bk == 0 ? 原值 : (原 bk & 0x8000) | (印章 bk & 0x7FFF)
//! 中层 mid：印章 mid == 0 ? 原值 : 印章 mid & 0x7FFF            // 不保留 bit15
//! 物体 fr：印章 fr == 0 ? 原值 : (原 fr & 0x8000) | (印章 fr & 0x7FFF)
//! 其余字段：原样
//! ```
//!
//! 规则的出处：`miredit-core/src/paint.rs` 的 `Tool::Brush` 分支（= `edit.rs:1462-1468`）
//! + `CrystalStudio/PlacedStamps.cs:90-92` 的「图号 0 ⇒ 该族不写」。
//!
//! ⚠ `0.map` 是 **12 B/格**（无库号字节）⇒ 这条门验不到库号字节；库号字节的闸门由
//! `src/stamp.rs` 的单元测试（`库号写不进去要说明` / `十二字节档库号忽略`）与
//! 门 2 补充里的 `tiles/sm_tiles/area` 断言分工覆盖。

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use miredit_core::{
    apply, save_session, Command, EditSession, PlaceOutcome, StampCellInput,
};
use mir2_format::FormatId;
use mir2_map::Cell;
use mir2_stamp::Stamp;

/// 资产根缺省值：本机真实根（**整个文件里只出现这一处**）。换机器用 `MIREDIT_ASSET_ROOT`。
const 缺省资产根: &str = r"E:\GodM2";
/// 取证根缺省值：`.X` / `palette.json` 的取证来源。换机器用 `MIREDIT_UPSTREAM_ROOT`。
const 缺省取证根: &str = r"I:\miredit\_upstream";

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

/// 取证根：`MIREDIT_UPSTREAM_ROOT` → 缺省 [`缺省取证根`]；根不存在 ⇒ `None`（同上）。
fn 取证根() -> Option<PathBuf> {
    let root = std::env::var_os("MIREDIT_UPSTREAM_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(缺省取证根));
    root.is_dir().then_some(root)
}

/// 真实印章样本（CrystalStudio 自带范例，README `:211-215` 记的就是它）。
fn 印章路径() -> Option<PathBuf> {
    let p = 取证根()?.join("Objects").join("mir").join("mir_00000.X");
    p.is_file().then_some(p)
}

/// 真实地图样本（700×700 / 12 B 每格 / `Shanda14` 之前的旧档）。
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
    let d = std::env::temp_dir().join(format!("miredit_core_stamp_{tag}_{pid}"));
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
    let 期望名 = format!("miredit_core_stamp_{tag}_{本进程}");
    assert_eq!(
        实际名,
        期望名.as_str(),
        "临时目录名必须逐字等于「miredit_core_stamp_<tag>_<本进程 PID>」，否则并发隔离失效：{}",
        d.display()
    );
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("建临时目录");
    d
}

/// 读真实印章并换成 [`StampCellInput`]；同时把「印章真实路径 + 格数 + 包围盒」一并交出来自证。
///
/// 缺取证根 / 缺 `.X` 文件 ⇒ `None`（公开仓/PR 的机器上没有取证素材，调用方直接 `return`）。
///
/// # 换算口只有一处（2026-11-05 收口）
///
/// 直接调 [`miredit_core::cells_from_stamp`]，**本文件不再手写任何字段映射**
/// —— 手写映射是本仓最后一次「`.X` → [`StampCellInput`]」的重复实现，已删除。
///
/// 旧手写版与库版的**四处**不一致（前两条会静默出错）：
///
/// | # | 字段 | 旧手写版 | 库版 |
/// |---|------|----------|------|
/// | 1 | `bk_lib` / `mid_lib` / `fr_lib` | `back_library as u8` 等 ⇒ 越界（300 / -1）**静默截断**成 44 / 255，悄悄指向另一个图库 | `u8::try_from`，越界即 `Err`，消息含「第 N 格 / 地表\|中层\|物体 / 值」 |
/// | 2 | `mid_ani_frame` / `mid_ani_tick` / `tile_animation` / `tile_offset` / `tile_frames` | 写死 `0` | **照搬 `.X` 真值**（`mir2-stamp/src/cell.rs:34-38` 的 off27/28/29..31/31..33/33 确实有字节） |
/// | 3 | `bk` | `(back_image as u32 & 0xFFFF) as u16`（漏掩码） | `(back_image as u16) & 0x7FFF` |
/// | 4 | `fr` | `f.image as u16`（漏掩码） | `(f.image as u16) & 0x7FFF` |
///
/// 第 3/4 条在本样本上「碰巧不出错」（`back_image = 0x6000_0000` 的低 16 位本来就是 0，
/// 且 `paint` 那边会再掩一次），但语义仍是错的。
///
/// 返回值的四元组是「(路径, 格表, 包围盒, 格数)」—— 本测试专用的小夹具，
/// 拆成 `struct` 只会让十来处调用点各多一行解构，收益为零，
/// 故对 `clippy::type_complexity` 就地豁免（2026-11-06 公开仓门禁 `-D warnings` 需要）。
#[allow(clippy::type_complexity)]
fn 读真实印章() -> Option<(PathBuf, Vec<StampCellInput>, (i32, i32, i32, i32), usize)> {
    let Some(p) = 印章路径() else {
        eprintln!("[skip] 无取证根 MIREDIT_UPSTREAM_ROOT（或缺 Objects/mir/mir_00000.X）");
        return None;
    };
    let bytes = std::fs::read(&p)
        .unwrap_or_else(|e| panic!("读不到印章 {}：{e}", p.display()));
    let stamp: Stamp = Stamp::parse(&bytes).unwrap_or_else(|e| panic!("印章解析失败：{e}"));
    let 有前景 = stamp.cells.iter().filter(|c| c.cell.foreground.is_some()).count();
    // 库号越界会在这里**大声失败**（`expect` 的消息来自 `StampConvertError::Display`），
    // 而不是像旧的 `as u8` 那样静默截断后继续跑。
    let cells = miredit_core::cells_from_stamp(&stamp).expect("真实印章应能转换");

    // ── 真值自证：把「库号」与「5 个视图装不下的动画字段」钉在 `.X` 的真值上 ──
    //
    // 断言口径从「填 0 / `as u8` 截断」改成「**等于 `.X` 真值**」之后必须显式比一遍：
    // 本样本这 5 个字段的真值**恰好全 0**（25 格逐格实测），光看结果值分辨不出
    // 「照搬」与「填 0」；写成「= `.X` 真值」后，换一枚带中层/地表动画的印章会立刻红。
    assert_eq!(cells.len(), stamp.cells.len(), "换算不得增删格");
    for (i, (c, s)) in cells.iter().zip(stamp.cells.iter()).enumerate() {
        let n = i + 1;
        // 库号：真值，且**必须装得进单字节**（装不进就在这里 panic，绝不静默截断）
        assert_eq!(
            c.bk_lib,
            u8::try_from(s.cell.back_library).expect("地表库号必须装得进单字节"),
            "第 {n} 格 地表库号 = `.X` 的 BackLibrary 真值"
        );
        assert_eq!(
            c.mid_lib,
            u8::try_from(s.cell.middle_library).expect("中层库号必须装得进单字节"),
            "第 {n} 格 中层库号 = `.X` 的 MiddleLibrary 真值"
        );
        assert_eq!(
            c.fr_lib,
            s.cell
                .foreground
                .map_or(Ok(0), |f| u8::try_from(f.library))
                .expect("物体库号必须装得进单字节"),
            "第 {n} 格 物体库号 = `.X` 的 Foreground.Library 真值"
        );
        // 5 个「视图装不下」的字段：**照搬真值**（旧手写版在这里写死 0）
        assert_eq!(c.mid_ani_frame, s.cell.middle_frames, "第 {n} 格 中层动画帧 = `.X` 真值");
        assert_eq!(c.mid_ani_tick, s.cell.middle_tick, "第 {n} 格 中层动画节拍 = `.X` 真值");
        assert_eq!(
            c.tile_animation,
            s.cell.tile_animation as u16,
            "第 {n} 格 地表动画图号 = `.X` 真值（i16 按位转 u16）"
        );
        assert_eq!(
            c.tile_offset,
            s.cell.tile_offset as u16,
            "第 {n} 格 地表动画偏移 = `.X` 真值（i16 按位转 u16）"
        );
        assert_eq!(c.tile_frames, s.cell.tile_frames, "第 {n} 格 地表动画帧数 = `.X` 真值");
    }

    Some((p, cells, stamp.bounds(), 有前景))
}

/// 按**规则**（不是按实现）算出一格贴完之后应该长什么样。
///
/// 规则出处：
/// - 三族：`PlacedStamps.cs:90-92`（图号 0 ⇒ 该族不写）+ `paint.rs` 的 `Tool::Brush` 分支
///   （地表/物体保留 `bit15`、中层不保留）；
/// - 库号字节：图号非 0 才写（`c.BackLibrary=a.BackLibrary` 在 `if(a.BackImage!=0)` 里面），
///   且只在**该档有库号字节**时（`有库号` = `CellFormat::has_lib_bytes()`）；
/// - 附加字段：`PlacedStamps.cs:93-96`（`if(a.Door!=0){…}` / `if(a.Light!=0)…` /
///   `c.Foreground = front` 整体赋值 ⇒ `fr != 0` 时连 `Library`/`Frames`/`Tick` 一起搬）。
fn 期望格(旧: Cell, s: &StampCellInput, 有库号: bool) -> Cell {
    let mut c = 旧;
    if s.bk != 0 {
        c.bk_img = (旧.bk_img & 0x8000) | (s.bk & 0x7FFF);
        if 有库号 {
            c.tiles = s.bk_lib;
        }
    }
    if s.mid != 0 {
        c.mid_img = s.mid & 0x7FFF;
        if 有库号 {
            c.sm_tiles = s.mid_lib;
        }
    }
    if s.fr != 0 {
        c.fr_img = (旧.fr_img & 0x8000) | (s.fr & 0x7FFF);
        // `PlacedStamps.cs:92` 的 `c.Foreground = front`：`Front.Library` 落到 `btArea`（off10）
        c.area = s.fr_lib;
        c.ani_frame = s.ani_frame;
        c.ani_tick = s.ani_tick;
    }
    // `PlacedStamps.cs:93`
    if s.door != 0 {
        c.door_index = s.door;
        c.door_offset = s.door_offset;
    }
    // `PlacedStamps.cs:94`
    if s.light != 0 {
        c.light = s.light;
    }
    c
}

/// 「世界坐标 → 格」的表：旧值快照与期望值各一张（拆成别名，免得签名过复杂）。
type 格表 = HashMap<(u16, u16), Cell>;

/// 把「印章 + 锚点」算成「世界坐标 → 期望格」的映射（同一格多次出现 ⇒ 后者胜）。
///
/// `有库号` 由视图档位决定（`CellFormat::has_lib_bytes()`）：12 B 档没有
/// `btTiles`/`btsmTiles` 两个字节，写进去也会被 writer 丢掉（`convert.rs` 第六节）。
fn 期望表(
    ed: &EditSession,
    锚点: (u16, u16),
    cells: &[StampCellInput],
) -> (格表, 格表, usize) {
    let (w, h) = (ed.map().width, ed.map().height);
    let 有库号 = ed.map().format.has_lib_bytes();
    let mut 旧表: 格表 = HashMap::new();
    let mut 新表: 格表 = HashMap::new();
    let mut 越界 = 0usize;
    for s in cells {
        let sx = i64::from(锚点.0) + i64::from(s.dx);
        let sy = i64::from(锚点.1) + i64::from(s.dy);
        if sx < 0 || sy < 0 || sx >= i64::from(w) || sy >= i64::from(h) {
            越界 += 1;
            continue;
        }
        let key = (sx as u16, sy as u16);
        let 当前 = 新表.get(&key).copied().unwrap_or_else(|| {
            let c = *ed.map().cell(key.0, key.1).expect("图内格");
            旧表.insert(key, c);
            c
        });
        新表.insert(key, 期望格(当前, s, 有库号));
    }
    (旧表, 新表, 越界)
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

// ═══════════════════════════ 门 2 + 门 3 ═══════════════════════════

/// **门 2**：真实印章贴进 `0.map` 副本的 `(100,100)` ⇒ 保存后只有印章覆盖的格变、
/// `bit15` 保留、头部 0 差异、其余 0 差异。
///
/// **门 3**：放置后撤销 ⇒ `to_bytes()` 与原文件逐字节相等。
#[test]
fn 门2门3_真实印章贴0map副本_逐字节核对与撤销() {
    let Some((印章, cells, bounds, 有前景)) = 读真实印章() else {
        return;
    };
    // ① 印章自证（任务书给的三个数必须对上，否则后面全是空转）
    assert_eq!(bounds, (-1, -2, 4, 3), "包围盒必须是 (-1,-2,4,3)");
    assert_eq!(cells.len(), 25, "格数必须是 25");
    assert_eq!(有前景, 8, "8 格带前景（README:211-215 的 7 + 25×29 + 8×6 = 780）");
    // 这枚印章到底带哪几族内容（决定下面几个门能验到哪一层）
    let 带地表 = cells.iter().filter(|c| c.bk != 0).count();
    let 带中层 = cells.iter().filter(|c| c.mid != 0).count();
    let 带物体 = cells.iter().filter(|c| c.fr != 0).count();
    println!(
        "GATE2 印章={} 格={} 包围盒={bounds:?} 带前景={有前景} 带地表={带地表} 带中层={带中层} 带物体={带物体}",
        印章.file_name().unwrap_or_default().to_string_lossy(),
        cells.len()
    );
    assert_eq!(带物体, 8, "8 格有前景图号（= 前景块个数）");
    assert_eq!(带地表 + 带中层, 0, "这枚是纯物件印章：back_image / middle_image 全 0");

    // ② 复制 0.map 到临时目录（真图只读，绝不改）
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
    assert_eq!((ed.map().width, ed.map().height), (700, 700));
    let 头长 = ed.document().raw_header().len();
    let 格长 = ed.document().disk_cell_len().expect("定长格式");
    assert_eq!((头长, 格长), (52, 12), "Old12：52 字节头 + 12 字节/格");

    // ③ 期望值（独立推导）
    let 锚点 = (100u16, 100u16);
    let (旧表, 新表, 期望越界) = 期望表(&ed, 锚点, &cells);
    assert_eq!(期望越界, 0, "锚点 (100,100) 在 700×700 图内，不该有越界格");
    let 期望变化格 = 新表.iter().filter(|(k, v)| 旧表.get(*k) != Some(*v)).count();

    // ④ 放置
    let out = ed.place_stamp(锚点.0, 锚点.1, &cells);
    let (改动格, 跳过格) = match &out {
        PlaceOutcome::Placed { cells, skipped } => (*cells, *skipped),
        PlaceOutcome::Refused { reason } => panic!("真实印章不该被拒：{reason}"),
    };
    assert_eq!(跳过格, 0, "没有越界格");
    assert_eq!(改动格, 期望变化格, "改动格数必须等于独立推导的变化格数");
    assert_eq!(ed.undo_len(), 1, "整次放置 = 一步撤销");

    // ⑤ 逐格核对（改前/改后 + bit15）
    let mut 打印 = 0usize;
    for (key, 期望) in &新表 {
        let 旧 = 旧表[key];
        let 实得 = *ed.map().cell(key.0, key.1).expect("图内格");
        assert_eq!(
            实得.bk_img, 期望.bk_img,
            "({},{}) 地表：旧 0x{:04X} 期望 0x{:04X} 实得 0x{:04X}",
            key.0, key.1, 旧.bk_img, 期望.bk_img, 实得.bk_img
        );
        assert_eq!(实得.mid_img, 期望.mid_img, "({},{}) 中层", key.0, key.1);
        assert_eq!(实得.fr_img, 期望.fr_img, "({},{}) 物体", key.0, key.1);
        // bit15 保留矩阵（`bit15` 只在 bk/fr 上，中层一律清掉）
        assert_eq!(
            实得.bk_img & 0x8000,
            旧.bk_img & 0x8000,
            "({},{}) 地表 bit15 必须保留",
            key.0,
            key.1
        );
        assert_eq!(
            实得.fr_img & 0x8000,
            旧.fr_img & 0x8000,
            "({},{}) 物体 bit15 必须保留",
            key.0,
            key.1
        );
        assert_eq!(实得.mid_img & 0x8000, 0, "中层不保留 bit15");
        // 附加字段：按 `PlacedStamps.cs:92-94` 的**规则**核对（不是笼统的"原样"）
        assert_eq!(实得.door_index, 期望.door_index, "({},{}) 门索引", key.0, key.1);
        assert_eq!(实得.door_offset, 期望.door_offset, "({},{}) 门偏移", key.0, key.1);
        assert_eq!(实得.light, 期望.light, "({},{}) 光照", key.0, key.1);
        assert_eq!(实得.ani_frame, 期望.ani_frame, "({},{}) 前景动画帧", key.0, key.1);
        assert_eq!(实得.ani_tick, 期望.ani_tick, "({},{}) 前景动画节拍", key.0, key.1);
        // 物体库号（`btArea`，off10）随 `c.Foreground = front` 一起搬（`PlacedStamps.cs:92`）
        assert_eq!(实得.area, 期望.area, "({},{}) 物体库号 btArea", key.0, key.1);
        assert_eq!(实得.tail, 旧.tail, "({},{}) 尾区不参与（12 B 档没有尾区）", key.0, key.1);
        if 实得 != 旧 && 打印 < 8 {
            println!(
                "GATE2 ({},{}) 旧 bk=0x{:04X} mid=0x{:04X} fr=0x{:04X} → 新 bk=0x{:04X} mid=0x{:04X} fr=0x{:04X}",
                key.0, key.1, 旧.bk_img, 旧.mid_img, 旧.fr_img, 实得.bk_img, 实得.mid_img, 实得.fr_img
            );
            打印 += 1;
        }
    }

    // ⑥ 门 3：**先**验撤销（此刻还没保存，`saved_pos` 仍是 0 ⇒ 撤销回原样后必须不脏）
    let 放之后 = ed.document().to_bytes();
    let 撤 = ed.undo();
    assert_eq!(撤, 改动格, "撤销应还原同样多的格");
    assert_eq!(
        ed.document().to_bytes(),
        原字节,
        "门 3：撤销后 to_bytes() 必须与原文件逐字节相等"
    );
    assert!(!ed.dirty(), "撤销回保存点后不该脏");
    println!(
        "GATE3 撤销 {撤} 格 ⇒ to_bytes() 与原文件逐字节相等（{} 字节）",
        原字节.len()
    );
    // 重做回"已放置"状态（保存路径验的是**盘上**字节，必须真的放着一枚印章）
    assert_eq!(ed.redo(), 改动格);
    assert_eq!(ed.document().to_bytes(), 放之后, "重做必须回到放置后的字节");

    // ⑦ 保存 + 逐字节核对
    let so = save_session(&mut ed).expect("保存应成功");
    assert!(so.wrote(), "有修改就必须真的写盘：{}", so.note());
    let 新字节 = std::fs::read(&副本).expect("读回保存后的文件");
    assert_eq!(新字节.len(), 原字节.len(), "长度必须不变");
    let 差异 = 差异清单(&原字节, &新字节);
    // 每个差异字节都必须落在「期望变化的格」的字节区间里
    let h = ed.map().height;
    let mut 差异格: BTreeMap<(u16, u16), usize> = BTreeMap::new();
    for (off, a, b) in &差异 {
        let rel = off.saturating_sub(头长);
        let idx = rel / 格长;
        let (x, y) = ((idx / h as usize) as u16, (idx % h as usize) as u16);
        assert!(
            *off >= 头长,
            "头部不许有差异：偏移 {off}（头长 {头长}）0x{a:02X} → 0x{b:02X}"
        );
        let 旧 = *旧表.get(&(x, y)).unwrap_or_else(|| {
            panic!("差异落在印章覆盖之外的格 ({x},{y})：偏移 {off} 0x{a:02X} → 0x{b:02X}")
        });
        assert_ne!(旧, 新表[&(x, y)], "差异格 ({x},{y}) 应当是期望变化的格");
        *差异格.entry((x, y)).or_default() += 1;
    }
    assert_eq!(
        差异格.len(),
        期望变化格,
        "差异涉及的格数必须等于期望变化格数（多一格就是写到了不该写的格）"
    );
    // 逐格字节数：按 12 B 布局逐字节比（bk@0 mid@2 fr@4 door@6 door_off@7 ani@8,9 area@10 light@11）
    let 格字节 = |c: Cell| -> Vec<u8> {
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
    };
    let 期望字节: usize = 新表
        .iter()
        .filter(|(k, v)| 旧表.get(*k) != Some(*v))
        .map(|(k, v)| {
            let (a, b) = (格字节(旧表[k]), 格字节(*v));
            a.iter().zip(b.iter()).filter(|(x, y)| x != y).count()
        })
        .sum();
    assert_eq!(差异.len(), 期望字节, "差异字节数必须等于逐字段推导值");
    println!(
        "GATE2 锚点={锚点:?} 印章格={} 改动格={改动格} 越界={跳过格} 差异字节={} 差异格={} 头部差异=0",
        cells.len(),
        差异.len(),
        差异格.len()
    );

    // ⑧ 保存之后再撤销 ⇒ 字节回到原文件，但**此时该脏**（盘上是"有印章"的那份）
    let 撤2 = ed.undo();
    assert_eq!(撤2, 改动格);
    assert_eq!(ed.document().to_bytes(), 原字节, "保存后撤销：内存字节回到原文件");
    assert!(ed.dirty(), "盘上是带印章的那份 ⇒ 内存退回后必须判脏（否则关窗会静默丢掉这次撤销）");

    let _ = std::fs::remove_dir_all(&d);
}

/// **门 2 补充**：把同一枚真实印章贴到**三族都带 `bit15`** 的合成图上 ⇒
/// 地表/物体的 `bit15` 必须原样保留，中层的必须被清掉。
///
/// 为什么要这条：`0.map` 的 `(100,100)` 附近未必有阻挡格，光看真实地图可能"验了个寂寞"。
#[test]
fn 门2补充_真实印章贴带bit15合成图() {
    let Some((_印章, cells, _, _)) = 读真实印章() else {
        return;
    };
    // 12×12 的 Old12 合成图：每格 bk/fr 都带 bit15（= 不可走），mid 也带（异常数据）
    let (w, h) = (12u16, 12u16);
    let mut 字节 = vec![0u8; 52 + 12 * (w as usize) * (h as usize)];
    字节[0..2].copy_from_slice(&w.to_le_bytes());
    字节[2..4].copy_from_slice(&h.to_le_bytes());
    for i in 0..(w as usize * h as usize) {
        let base = 52 + i * 12;
        let img = (i as u16).wrapping_add(1) & 0x7FFF;
        字节[base..base + 2].copy_from_slice(&(0x8000 | img).to_le_bytes());
        字节[base + 2..base + 4].copy_from_slice(&(0x8000 | img.wrapping_add(100)).to_le_bytes());
        // ⚠ 物体层**只留 bit15、低 15 位为 0**：占用闸门（`PlacedStamps.cs:62-63`）按
        // `fr_img & 0x7FFF != 0` 判"落点已有物件"，带图号会被整枚拒绝 ⇒
        // 那就测不到"贴上去之后物体层的 bit15 还在不在"了。本用例要验的是**bit15 矩阵**。
        字节[base + 4..base + 6].copy_from_slice(&0x8000u16.to_le_bytes());
    }
    let d = 临时目录("gate2b");
    let p = d.join("bit15.map");
    std::fs::write(&p, &字节).expect("写合成图");
    let mut ed = EditSession::load(&p).expect("打开合成图");
    let (旧表, 新表, 越界) = 期望表(&ed, (5, 5), &cells);
    assert_eq!(越界, 0, "12×12 图里锚点 (5,5) + 包围盒 (-1,-2,4,3) ⇒ x 4..=9 / y 3..=8，全在图内");
    let out = ed.place_stamp(5, 5, &cells);
    assert!(matches!(out, PlaceOutcome::Placed { .. }), "{out:?}");
    let mut 带bit15 = 0usize;
    for (key, 期望) in &新表 {
        let 旧 = 旧表[key];
        let 实得 = *ed.map().cell(key.0, key.1).expect("图内格");
        assert_eq!(旧.bk_img & 0x8000, 0x8000, "合成图的旧格本来就带 bit15");
        assert_eq!(实得.bk_img, 期望.bk_img, "({},{}) 地表", key.0, key.1);
        assert_eq!(实得.fr_img, 期望.fr_img, "({},{}) 物体", key.0, key.1);
        assert_eq!(实得.mid_img, 期望.mid_img, "({},{}) 中层", key.0, key.1);
        assert_eq!(实得.bk_img & 0x8000, 0x8000, "地表 bit15 必须留着");
        assert_eq!(实得.fr_img & 0x8000, 0x8000, "物体 bit15 必须留着");
        // 中层：**被写**（图号非 0）时 bit15 必须清掉；该族没内容（图号 0）时旧值原样留着
        assert!(
            实得.mid_img & 0x8000 == 0 || 实得.mid_img == 旧.mid_img,
            "中层 bit15 只允许在「该族未写」时留下：旧 0x{:04X} 新 0x{:04X}",
            旧.mid_img,
            实得.mid_img
        );
        带bit15 += 1;
    }
    // 撤销 ⇒ 逐字节回到合成图原样
    ed.undo();
    assert_eq!(ed.document().to_bytes(), 字节, "撤销后必须逐字节回到合成图原样");
    println!("GATE2b 带 bit15 的合成图：{带bit15} 格贴完仍全部保留阻挡位，撤销逐字节还原");
    let _ = std::fs::remove_dir_all(&d);
}

// ═══════════════════════════ 门 4 ═══════════════════════════

/// **门 4**：同一枚印章贴到 `(0,0)` ⇒ 负偏移的格逐格跳过，出界的格没写。
#[test]
fn 门4_越界裁剪_贴到原点() {
    let Some((_印章, cells, bounds, _)) = 读真实印章() else {
        return;
    };
    let Some(地图) = 地图路径() else {
        eprintln!("[skip] 无资产根 MIREDIT_ASSET_ROOT（或缺 GodotClient/maps/0.map）");
        return;
    };
    let d = 临时目录("gate4");
    let 副本 = d.join("0.map");
    std::fs::copy(&地图, &副本).expect("复制 0.map 副本");
    let 原字节 = std::fs::read(&副本).expect("读副本");
    let mut ed = EditSession::load(&副本).expect("打开副本");
    let (旧表, 新表, 期望越界) = 期望表(&ed, (0, 0), &cells);
    // 独立推导：包围盒 (-1,-2,4,3) ⇒ 锚点 (0,0) 时 dx<0 或 dy<0 的格都出界
    let 应跳过 = cells.iter().filter(|c| c.dx < 0 || c.dy < 0).count();
    assert_eq!(期望越界, 应跳过, "两种算法算出的越界格数必须一致");
    assert!(应跳过 > 0, "包围盒 {bounds:?} 说明原点锚点一定有负偏移格");

    let out = ed.place_stamp(0, 0, &cells);
    let (改动格, 跳过格) = match &out {
        PlaceOutcome::Placed { cells, skipped } => (*cells, *skipped),
        PlaceOutcome::Refused { reason } => panic!("部分越界不该整枚拒绝：{reason}"),
    };
    assert_eq!(跳过格, 应跳过, "越界格数必须等于独立推导值");
    assert_eq!(改动格, 新表.iter().filter(|(k, v)| 旧表.get(*k) != Some(*v)).count());
    assert!(
        改动格 + 跳过格 <= cells.len(),
        "每一格最多算一次（改动 {改动格} + 跳过 {跳过格} 不该超过 {} 格）",
        cells.len()
    );
    println!(
        "GATE4 图内格={} 其中真改={改动格}（其余是值本来相同）越界={跳过格}",
        cells.len() - 跳过格
    );
    // 出界的格一个字节都没写：差异只允许出现在「图内的印章格」上
    let 差异 = 差异清单(&原字节, &ed.document().to_bytes());
    let h = ed.map().height;
    for (off, a, b) in &差异 {
        let rel = off.saturating_sub(52);
        let idx = rel / 12;
        let (x, y) = ((idx / h as usize) as u16, (idx % h as usize) as u16);
        assert!(
            新表.contains_key(&(x, y)),
            "差异落在印章之外（或出界格）({x},{y})：偏移 {off} 0x{a:02X} → 0x{b:02X}"
        );
    }
    println!(
        "GATE4 锚点=(0,0) 印章格={} 图内改动={改动格} 越界跳过={跳过格}（独立推导 {应跳过}）差异字节={}",
        cells.len(),
        差异.len()
    );
    // 撤销 ⇒ 逐字节还原（越界格本来就没写，所以这一步同样成立）
    ed.undo();
    assert_eq!(ed.document().to_bytes(), 原字节, "撤销后必须逐字节还原");
    let _ = std::fs::remove_dir_all(&d);
}

// ═══════════════════════════ 门 5 ═══════════════════════════

/// **门 5**：贴到地图外 ⇒ `Refused`，文件**一字节不动**。
#[test]
fn 门5_全越界拒绝且文件一字节不动() {
    let Some((_印章, cells, _, _)) = 读真实印章() else {
        return;
    };
    let Some(地图) = 地图路径() else {
        eprintln!("[skip] 无资产根 MIREDIT_ASSET_ROOT（或缺 GodotClient/maps/0.map）");
        return;
    };
    let d = 临时目录("gate5");
    let 副本 = d.join("0.map");
    std::fs::copy(&地图, &副本).expect("复制 0.map 副本");
    let 原字节 = std::fs::read(&副本).expect("读副本");
    let mut ed = EditSession::load(&副本).expect("打开副本");

    let out = ed.place_stamp(700, 700, &cells);
    let reason = match &out {
        PlaceOutcome::Refused { reason } => reason.clone(),
        PlaceOutcome::Placed { cells, .. } => panic!("全越界必须拒绝，实际放了 {cells} 格"),
    };
    assert!(reason.contains("全部 25 格都在地图外"), "{reason}");
    assert!(reason.contains("一格未写"), "{reason}");
    assert_eq!(out.cells(), 0);
    assert_eq!(out.skipped(), 0, "拒绝时 skipped 记 0（原因里已经说了全部越界）");
    assert_eq!(ed.undo_len(), 0, "拒绝不许产生撤销步");
    assert!(!ed.dirty(), "拒绝不许置脏");
    // 文件：to_bytes 与原字节逐字节相等，且保存走 Clean（不写盘）
    assert_eq!(ed.document().to_bytes(), 原字节, "拒绝时字节必须一字节不动");
    let so = save_session(&mut ed).expect("保存应成功（走 Clean）");
    assert!(!so.wrote(), "没有修改就不许写盘：{}", so.note());
    assert_eq!(std::fs::read(&副本).unwrap(), 原字节, "盘上文件必须一字节不动");
    println!("GATE5 锚点=(700,700) ⇒ Refused：{reason}");
    let _ = std::fs::remove_dir_all(&d);
}

// ═══════════════════════════ 门 6 ═══════════════════════════

/// 造一张 **Type5（`Mir3Wemade`）** 合成样本的原始字节。
///
/// 布局照 `mir2-format/src/formats/mir3_wemade.rs` 的测试夹具（与
/// `tests/formats_9.rs::造_type5` 逐字节同口径）：
/// 28 字节头（`Width` `i16@22`、`Height` `i16@24`、首字节 0 = 无标题）
/// + 半分辨率背景块 `3 * ceil(w/2) * floor(h/2)` + `14 × w × h` 格。
///
/// ⚠ **前景图号故意留 0**（2026-11-05 订正）：占用闸门（`PlacedStamps.cs:62-63`）
/// 要求落点 `fr_img & 0x7FFF == 0`，而门 6 要验的是 **Type5 的地表能力闸门**，
/// 两码事 —— 夹具带前景会把 ②"只带中层/物体 ⇒ 放行"提前毙掉，测到的东西就错了。
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
        // 中层图号（格内 rel 5..7，`mir3_wemade.rs:234` 的 `u16_at(c, 5)`）
        b[base + 5..base + 7].copy_from_slice(&(100 + i as u16).to_le_bytes());
        // 光照（格内 rel 12）
        b[base + 12] = 0x0F;
    }
    b
}

/// **门 6**：合成 Type5 样本上贴**含 `bk != 0`** 的印章 ⇒ `Refused` + 中文原因。
#[test]
fn 门6_type5拒绝带地表印章() {
    let d = 临时目录("gate6");
    let p = d.join("type5.map");
    let 原 = 造_type5(4, 2);
    std::fs::write(&p, &原).expect("先写 Type5 合成样本");
    let mut ed = EditSession::load(&p).expect("Type5 合成样本应能打开");
    assert_eq!(ed.document().format_id(), FormatId::Mir3Wemade);
    assert!(!ed.ground_paint_supported());

    // ① 带地表（bk != 0）⇒ 拒绝
    let 带地表 = vec![
        StampCellInput { dx: 0, dy: 0, bk: 7, mid: 8, fr: 9, ..Default::default() },
        StampCellInput { dx: 1, dy: 1, bk: 10, ..Default::default() },
    ];
    let out = ed.place_stamp(0, 0, &带地表);
    let reason = match &out {
        PlaceOutcome::Refused { reason } => reason.clone(),
        PlaceOutcome::Placed { cells, .. } => panic!("Type5 带地表必须拒绝，实际放了 {cells} 格"),
    };
    assert!(reason.contains("不支持逐格写地表背图"), "{reason}");
    assert!(reason.contains("块"), "原因要点明「块级」：{reason}");
    assert!(reason.contains("2 格 bk≠0"), "原因要报出有几格带地表：{reason}");
    assert!(reason.contains("一个字节都没动"), "{reason}");
    assert_eq!(ed.undo_len(), 0);
    assert!(!ed.dirty());
    assert_eq!(ed.document().to_bytes(), 原, "拒绝时字节必须一字节不动");
    let so = save_session(&mut ed).expect("保存应成功（走 Clean）");
    assert!(!so.wrote(), "没有修改就不许写盘");
    assert_eq!(std::fs::read(&p).unwrap(), 原, "盘上文件必须一字节不动");

    // ② 只有中层/物体（bk 全 0）⇒ 放行（证明"拒的是地表，不是整张图"）
    let 无地表 = vec![
        StampCellInput { dx: 0, dy: 0, mid: 8, fr: 9, ..Default::default() },
        StampCellInput { dx: 1, dy: 1, mid: 11, fr: 12, ..Default::default() },
    ];
    let out = ed.place_stamp(0, 0, &无地表);
    assert!(matches!(out, PlaceOutcome::Placed { cells: 2, skipped: 0 }), "{out:?}");
    assert_eq!(ed.map().cell(0, 0).unwrap().mid_img, 8);
    assert_eq!(ed.map().cell(1, 1).unwrap().fr_img, 12);
    println!("GATE6 Type5：带地表 ⇒ Refused「{reason}」；只带中层/物体 ⇒ 放行 2 格");
    let _ = std::fs::remove_dir_all(&d);
}

// ═══════════════════════════ 门 7 ═══════════════════════════

/// 造一张 `w×h` 的 Old12 合成图字节（每格图号 = 格序 + 1，便于看"哪格变了"）。
///
/// ⚠ **前景图号留 0**（2026-11-05 订正）：占用闸门（`PlacedStamps.cs:62-63`）要求落点
/// `fr_img & 0x7FFF == 0`。带前景的夹具只适合"验闸门拒绝"的用例，不适合
/// "验放置/撤销"的用例 —— 后者需要空白落点。
fn 合成图(w: u16, h: u16) -> Vec<u8> {
    let mut b = vec![0u8; 52 + 12 * (w as usize) * (h as usize)];
    b[0..2].copy_from_slice(&w.to_le_bytes());
    b[2..4].copy_from_slice(&h.to_le_bytes());
    for i in 0..(w as usize * h as usize) {
        let base = 52 + i * 12;
        let img = (i as u16).wrapping_add(1);
        b[base..base + 2].copy_from_slice(&img.to_le_bytes());
        b[base + 2..base + 4].copy_from_slice(&img.wrapping_add(100).to_le_bytes());
    }
    b
}

/// **门 7**：两格印章放同一点**两次** —— 第二次被**占用闸门**整枚拒绝。
///
/// # 2026-11-05 订正（旧版断言"第二次覆盖、`cells == 0`"）
///
/// 旧版写的是"同一枚再贴一次 ⇒ 覆盖，值没变所以 0 格"。这条在 C# 里**不成立**：
/// `PlaceStamp` 前置 `CanPlaceStamp`（`PlacedStamps.cs:52-68`），其中
/// `:62-63` 是 `var c=Get((int)sx,(int)sy); if(c.HasForeground)return false;`
/// ⇒ 第一次贴完落点**已经有物件**了，第二次**必然被整枚拒绝**，根本没有"覆盖"这一步。
/// 覆盖语义只存在于粘贴路径（`RegionCopy.Paste` 没有占用闸门），已挪到
/// `selection_clipboard.rs::粘贴路径不套占用闸门` 里验。
///
/// 四件事必须同时成立（**断言只改语义、不放宽**）：
/// 1. 第一次成功：2 格、一步撤销、落点真的有了物件；
/// 2. 第二次 ⇒ `Refused`，原因**中文**且点名落点坐标；
/// 3. 拒绝**不增撤销步**、**字节一字节不动**（先算后写）；
/// 4. 撤销一次 ⇒ 逐字节回到放之前；撤销后落点又空了 ⇒ 同一枚可以再贴（闸门不粘）。
#[test]
fn 门7_重叠与撤销一次() {
    let d = 临时目录("gate7");
    let p = d.join("overlap.map");
    let 原 = 合成图(8, 8);
    std::fs::write(&p, &原).expect("写合成图");
    let mut ed = EditSession::load(&p).expect("打开合成图");
    // 甲带**物件**（`fr != 0`）：这样第一次贴完落点就被占用，第二次才会被闸门拒
    let 甲 = vec![
        StampCellInput { dx: 0, dy: 0, bk: 100, fr: 50, ..Default::default() },
        StampCellInput { dx: 1, dy: 1, bk: 101, fr: 51, ..Default::default() },
    ];
    // ① 第一次：真的贴上
    let out = ed.place_stamp(2, 2, &甲);
    assert!(matches!(out, PlaceOutcome::Placed { cells: 2, skipped: 0 }), "{out:?}");
    let 放一次 = ed.document().to_bytes();
    assert_ne!(放一次, 原);
    assert_eq!(ed.map().cell(2, 2).unwrap().bk_img, 100);
    assert_eq!(ed.map().cell(2, 2).unwrap().fr_img, 50, "落点从此有物件（`HasForeground` 成立）");
    assert_eq!(ed.undo_len(), 1, "整次放置 = 一步撤销");

    // ② 第二次（同一枚、同一锚点）⇒ 占用闸门整枚拒绝
    let out = ed.place_stamp(2, 2, &甲);
    let reason = match &out {
        PlaceOutcome::Refused { reason } => reason.clone(),
        PlaceOutcome::Placed { cells, .. } => {
            panic!("落点已有物件必须整枚拒绝（不是逐格跳过），实际放了 {cells} 格")
        }
    };
    assert!(reason.contains("已有物件"), "原因必须点明占用：{reason}");
    assert!(reason.contains("(2,2)"), "原因必须点名落点坐标：{reason}");
    assert!(reason.contains("一个字节都没动"), "{reason}");
    assert_eq!(out.cells(), 0, "拒绝 ⇒ 0 格");
    assert_eq!(ed.undo_len(), 1, "拒绝不许产生撤销步");
    assert_eq!(ed.document().to_bytes(), 放一次, "拒绝时字节必须一字节不动");

    // ③ 撤销一次 ⇒ 回到放之前
    let n = ed.undo();
    assert_eq!(n, 2);
    assert_eq!(ed.document().to_bytes(), 原, "门 7：撤销一次必须逐字节回到放之前");
    assert!(!ed.dirty());

    // ④ 撤销后落点又空了 ⇒ 同一枚可以再贴（证明"拒的是占用，不是这枚印章坏了"）
    let out = ed.place_stamp(2, 2, &甲);
    assert!(matches!(out, PlaceOutcome::Placed { cells: 2, skipped: 0 }), "{out:?}");
    println!(
        "GATE7 同点两次：第一次 2 格；第二次 ⇒ Refused「{reason}」且字节不动/步数不增；\
         撤销一次逐字节还原；撤销后同点可再贴"
    );
    let _ = std::fs::remove_dir_all(&d);
}

// ═══════════════════════════ 附：按 id 放置（注册表） ═══════════════════════════

/// `Command::PlaceStamp` 的完整链路：上层注册（读 `.X` 的那一层）→ 命令按 id 放置 → 撤销。
///
/// 这条同时钉住「本 crate 不读盘」这个设计：id 不在注册表里就**明确拒绝**，
/// 绝不猜、也绝不静默放一枚空印章。
#[test]
fn 附_按id放置与撤销() {
    let Some((_印章, cells, _, _)) = 读真实印章() else {
        return;
    };
    let d = 临时目录("byid");
    let p = d.join("byid.map");
    let 原 = 合成图(20, 20);
    std::fs::write(&p, &原).expect("写合成图");
    let mut ed = EditSession::load(&p).expect("打开合成图");
    // 未注册 ⇒ 拒绝
    let out = apply(
        &mut ed,
        Command::PlaceStamp { x: 5, y: 5, stamp: "mir_00000".to_string() },
    )
    .expect("未注册不是程序故障");
    assert_eq!(out.changed, 0);
    assert!(out.note.as_deref().unwrap_or("").contains("未注册的印章 id"));
    assert_eq!(ed.document().to_bytes(), 原);
    // 注册后 ⇒ 真的放
    assert_eq!(
        ed.register_stamp("mir_00000", cells.clone()).expect("注册应成功"),
        25
    );
    assert_eq!(ed.stamp_ids(), vec!["mir_00000"]);
    // 同一个 id 再注册 = **覆盖**（不是追加）：格数换成新的，注册表里仍只有一个 id
    assert_eq!(
        ed.register_stamp("mir_00000", cells[..3].to_vec()).expect("覆盖应成功"),
        3
    );
    assert_eq!(ed.stamp_ids(), vec!["mir_00000"], "覆盖不新增 id");
    assert_eq!(ed.register_stamp("mir_00000", cells.clone()).expect("再覆盖"), 25);
    // 空格表 / 超上限 ⇒ 明确拒绝，且不覆盖已有内容
    assert!(ed.register_stamp("空", Vec::new()).is_err());
    assert_eq!(
        ed.stamp_cells("mir_00000").map(|s| s.len()),
        Some(25),
        "失败不许动旧内容"
    );
    let out = apply(
        &mut ed,
        Command::PlaceStamp { x: 5, y: 5, stamp: "mir_00000".to_string() },
    )
    .expect("放置不该 Err");
    assert!(out.changed > 0);
    let note = out.note.clone().unwrap();
    assert!(note.contains("放置印章「mir_00000」"), "{note}");
    assert_eq!(ed.undo_len(), 1);
    // 撤销一步 ⇒ 逐字节回到放之前
    apply(&mut ed, Command::Undo).unwrap();
    assert_eq!(ed.document().to_bytes(), 原);
    println!("GATE-附 注册表：未注册拒绝 / 注册后 {note}");
    let _ = std::fs::remove_dir_all(&d);
}

// ═══════════════════════ 验收门 A：占用闸门（2026-11-05 新增） ═══════════════════════

/// 在图上找一块 `宽×高` 的**空落点**（整块 `fr_img & 0x7FFF == 0`）。
///
/// 为什么要它：占用闸门（`PlacedStamps.cs:62-63`）要求落点没有物件，硬编码一个
/// "碰巧是空的"坐标会在换图后假失败；这里按判据现场扫，扫不到就 panic（测试前提不成立）。
fn 找空落点(ed: &EditSession, 宽: u16, 高: u16, 起: u16) -> (u16, u16) {
    let (w, h) = (ed.map().width, ed.map().height);
    for y in 起..h.saturating_sub(高) {
        for x in 起..w.saturating_sub(宽) {
            let mut 空 = true;
            for dy in 0..高 {
                for dx in 0..宽 {
                    let c = ed.map().cell(x + dx, y + dy).expect("图内格");
                    if c.fr_img & 0x7FFF != 0 {
                        空 = false;
                    }
                }
            }
            if 空 {
                return (x, y);
            }
        }
    }
    panic!("{w}×{h} 的图上找不到 {宽}×{高} 的空落点（测试前提不成立）");
}

/// **验收门（占用闸门）**：`0.map` 副本上先放一枚印章，再往**同一落点**放另一枚 ⇒ 拒绝。
///
/// 判据逐字来自 `PlacedStamps.cs:62-63`：
/// ```csharp
/// var c=Get((int)sx,(int)sy);
/// if(c.HasForeground)return false;
/// ```
/// `HasForeground`（`Document.cs:26`）= `Foreground is { } front && (front.Image & 0x7FFF) != 0`
/// ⇒ 本 crate 的等价判据 = `fr_img & 0x7FFF != 0`。
///
/// 四件事必须同时成立（缺一条就是假门）：
/// 1. 第二次是**整枚拒绝**（不是逐格跳过 ⇒ `skipped() == 0`）；
/// 2. 原因是**中文**，点名落点坐标与该格的物体图号；
/// 3. `undo_len` **不增**；
/// 4. 内存字节与盘上字节**memcmp 不变**。
///
/// 反向对照：往**空白处**放同一枚 ⇒ 成功（证明没把功能关死）。
#[test]
fn 占用闸门_落点已有物件时整枚拒绝且字节不动() {
    let Some(地图) = 地图路径() else {
        eprintln!("[skip] 无资产根 MIREDIT_ASSET_ROOT（或缺 GodotClient/maps/0.map）");
        return;
    };
    let d = 临时目录("gate_occupied");
    let 副本 = d.join("0.map");
    std::fs::copy(&地图, &副本).expect("复制 0.map 副本");
    let 原字节 = std::fs::read(&副本).expect("读副本");
    let mut ed = EditSession::load(&副本).expect("打开副本");
    assert_eq!(ed.document().format_id(), FormatId::Old12);

    // 2×2 的印章（带物件：贴完落点就被占用，正是闸门的判据）
    let 甲 = vec![
        StampCellInput { dx: 0, dy: 0, bk: 10, fr: 50, ..Default::default() },
        StampCellInput { dx: 1, dy: 1, bk: 11, fr: 51, ..Default::default() },
    ];
    let 乙 = vec![
        StampCellInput { dx: 0, dy: 0, bk: 20, fr: 60, ..Default::default() },
        StampCellInput { dx: 1, dy: 1, bk: 21, fr: 61, ..Default::default() },
    ];

    // ① 先放甲（找一块空落点，绝不赌坐标）
    let (ax, ay) = 找空落点(&ed, 2, 2, 0);
    let out = ed.place_stamp(ax, ay, &甲);
    assert!(matches!(out, PlaceOutcome::Placed { cells: 2, skipped: 0 }), "{out:?}");
    let 放甲之后 = ed.document().to_bytes();
    assert_ne!(放甲之后, 原字节, "甲必须真的写进字节");
    assert_eq!(ed.undo_len(), 1, "整次放置 = 一步撤销");
    // **先把甲落盘**：否则下面"拒绝时盘上不动"会被"甲还没保存"的脏标记搅进来
    let so = save_session(&mut ed).expect("保存甲应成功");
    assert!(so.wrote(), "甲必须真的写盘：{}", so.note());
    assert_eq!(std::fs::read(&副本).unwrap(), 放甲之后, "盘上就是甲的那一份");

    // ② 同一落点放乙 ⇒ 整枚拒绝
    let out = ed.place_stamp(ax, ay, &乙);
    let reason = match &out {
        PlaceOutcome::Refused { reason } => reason.clone(),
        PlaceOutcome::Placed { cells, .. } => {
            panic!("落点已有物件必须整枚拒绝，实际放了 {cells} 格")
        }
    };
    assert!(reason.contains("已有物件"), "原因必须点明占用：{reason}");
    assert!(
        reason.contains(&format!("({ax},{ay})")),
        "原因必须点名落点 ({ax},{ay})：{reason}"
    );
    assert!(
        reason.contains("物体图号 = 50"),
        "原因要报出该格原本的物体图号 50：{reason}"
    );
    assert_eq!(out.cells(), 0, "拒绝 ⇒ 0 格");
    assert_eq!(out.skipped(), 0, "是**整枚拒绝**，不是逐格跳过");
    assert_eq!(ed.undo_len(), 1, "拒绝不许产生撤销步");
    assert_eq!(ed.document().to_bytes(), 放甲之后, "内存字节必须 memcmp 不变");
    // 盘上也不许动：此刻已无未保存修改 ⇒ 保存走 Clean，不写盘
    let so = save_session(&mut ed).expect("保存应成功（走 Clean）");
    assert!(!so.wrote(), "拒绝之后没有新修改，不许再写盘：{}", so.note());
    assert_eq!(
        std::fs::read(&副本).unwrap(),
        放甲之后,
        "盘上文件必须与'放甲之后'memcmp 相等（拒绝没写一个字节）"
    );

    // ③ 反向对照：往**空白处**放同一枚乙 ⇒ 成功（闸门只拦"落点被占"，没关死功能）
    let (bx, by) = 找空落点(&ed, 2, 2, 0);
    assert_ne!((bx, by), (ax, ay), "甲已占用 ({ax},{ay}) ⇒ 空落点必然不同");
    let out = ed.place_stamp(bx, by, &乙);
    assert!(matches!(out, PlaceOutcome::Placed { cells: 2, skipped: 0 }), "{out:?}");
    assert_eq!(ed.undo_len(), 2, "反向对照的放置是第 2 步");
    // 只比低 15 位：该格原来的 bit15（阻挡位）按 `paint` 的保留矩阵留着
    assert_eq!(
        ed.map().cell(bx, by).unwrap().fr_img & 0x7FFF,
        60,
        "乙真的写进去了"
    );
    let so = save_session(&mut ed).expect("保存应成功");
    assert!(so.wrote(), "这次有修改，必须真的写盘：{}", so.note());
    println!(
        "GATE-OCCUPIED 甲@({ax},{ay}) 成功；乙@同一落点 ⇒ Refused「{reason}」\
         （skipped=0 / undo_len 不增 / 字节 memcmp 不变）；乙@({bx},{by}) ⇒ 成功"
    );
    let _ = std::fs::remove_dir_all(&d);
}

// ═══════════════════════ 验收门 B：坐标重复闸门（2026-11-05 新增） ═══════════════════════

/// **验收门（坐标重复闸门）**：同一枚印章里 `(dx,dy)` 出现两次 ⇒ **整枚拒绝**。
///
/// 判据逐字来自 `PlacedStamps.cs:65-66`：
/// ```csharp
/// var visited=new HashSet<(int,int)>();
/// foreach(var s in cells)if(!visited.Add((s.X,s.Y)))return false;
/// ```
/// ⇒ 是"整枚拒绝"，**不是**"后者胜"（后者胜只存在于粘贴路径的矩形展开）。
#[test]
fn 坐标重复闸门_同一枚印章里重复坐标被拒() {
    let Some(地图) = 地图路径() else {
        eprintln!("[skip] 无资产根 MIREDIT_ASSET_ROOT（或缺 GodotClient/maps/0.map）");
        return;
    };
    let d = 临时目录("gate_dup");
    let 副本 = d.join("0.map");
    std::fs::copy(&地图, &副本).expect("复制 0.map 副本");
    let 原字节 = std::fs::read(&副本).expect("读副本");
    let mut ed = EditSession::load(&副本).expect("打开副本");
    let (ax, ay) = 找空落点(&ed, 2, 2, 0);

    // ① 同一枚印章里 (0,0) 出现两次 ⇒ 拒绝
    let 重复 = vec![
        StampCellInput { dx: 0, dy: 0, bk: 10, ..Default::default() },
        StampCellInput { dx: 0, dy: 0, bk: 20, ..Default::default() },
    ];
    let out = ed.place_stamp(ax, ay, &重复);
    let reason = match &out {
        PlaceOutcome::Refused { reason } => reason.clone(),
        PlaceOutcome::Placed { cells, .. } => {
            panic!("重复坐标必须整枚拒绝，实际放了 {cells} 格")
        }
    };
    assert!(reason.contains("坐标重复"), "原因必须点明重复：{reason}");
    assert!(reason.contains("(0,0)"), "原因必须点名重复的那个偏移：{reason}");
    assert!(reason.contains("整枚拒绝"), "要明说不是逐格跳过：{reason}");
    assert!(reason.contains("一个字节都没动"), "{reason}");
    assert_eq!(out.cells(), 0);
    assert_eq!(out.skipped(), 0, "整枚拒绝 ⇒ skipped 也是 0");
    assert_eq!(ed.undo_len(), 0, "拒绝不许产生撤销步");
    assert_eq!(ed.document().to_bytes(), 原字节, "拒绝时字节必须 memcmp 不变");
    let so = save_session(&mut ed).expect("保存应成功（走 Clean）");
    assert!(!so.wrote(), "没有修改就不许写盘：{}", so.note());
    assert_eq!(std::fs::read(&副本).unwrap(), 原字节, "盘上文件必须一字节不动");

    // ② 反向对照：把重复的那格挪到 (1,0) ⇒ 同一份内容可以放（拒的是重复，不是内容）
    let 旧0 = *ed.map().cell(ax, ay).expect("图内格");
    let 旧1 = *ed.map().cell(ax + 1, ay).expect("图内格");
    let 唯一 = vec![
        StampCellInput { dx: 0, dy: 0, bk: 10, ..Default::default() },
        StampCellInput { dx: 1, dy: 0, bk: 20, ..Default::default() },
    ];
    let out = ed.place_stamp(ax, ay, &唯一);
    assert!(matches!(out, PlaceOutcome::Placed { cells: 2, skipped: 0 }), "{out:?}");
    let 新0 = *ed.map().cell(ax, ay).expect("图内格");
    let 新1 = *ed.map().cell(ax + 1, ay).expect("图内格");
    assert_eq!(新0.bk_img & 0x7FFF, 10, "地表图号 = 印章值");
    assert_eq!(新1.bk_img & 0x7FFF, 20, "地表图号 = 印章值");
    // `paint` 的保留矩阵：地表保留目标格自己的 bit15（阻挡位）
    assert_eq!(新0.bk_img & 0x8000, 旧0.bk_img & 0x8000, "地表 bit15 保留");
    assert_eq!(新1.bk_img & 0x8000, 旧1.bk_img & 0x8000, "地表 bit15 保留");
    println!(
        "GATE-DUP 重复坐标 (0,0)×2 ⇒ Refused「{reason}」；坐标改成 (0,0)/(1,0) ⇒ 放行 2 格"
    );
    let _ = std::fs::remove_dir_all(&d);
}

// ═══════════════════ 验收门 C：字段搬运真的落盘（hex 证据，2026-11-05 新增） ═══════════════════

/// **验收门（字段搬运 hex）**：造一枚带 `door / door_offset / light / ani_frame / ani_tick`
/// 的 [`StampCellInput`]，贴到 **12 B/格** 图上 ⇒ 保存 ⇒ 那些字段**确实落盘**。
///
/// 字节偏移（`mir2-map/src/cell.rs:25-39` 的 12 B 布局）：
/// ```text
/// 0..2 bk_img | 2..4 mid_img | 4..6 fr_img | 6 door_index | 7 door_offset
/// 8 ani_frame | 9 ani_tick   | 10 area    | 11 light
/// ```
/// 判据：`PlacedStamps.cs:92-94`（`c.Foreground = front` ⇒ 连 `Library`/`Frames`/`Tick` 一起搬；
/// `if(a.Door!=0){c.Door=a.Door;c.DoorOffset=a.DoorOffset;}`；`if(a.Light!=0)c.Light=a.Light;`）。
#[test]
fn 字段搬运_门灯动画真的落盘hex() {
    let (w, h) = (4u16, 4u16);
    let 原 = 合成图(w, h); // 前景全 0 ⇒ 落点空闲（占用闸门放行）
    let d = 临时目录("gate_hex");
    let p = d.join("hex.map");
    std::fs::write(&p, &原).expect("写合成图");
    let mut ed = EditSession::load(&p).expect("打开合成图");
    assert_eq!(ed.document().format_id(), FormatId::Old12);

    // 一格，把 5 个"12 B 布局里真有字节"的附加字段全给上可辨认的值
    let 一格 = StampCellInput {
        dx: 0,
        dy: 0,
        bk: 0x0123,
        mid: 0x0456,
        fr: 0x0789,
        bk_lib: 0,   // 12 B 档没有 btTiles
        mid_lib: 0,  // 12 B 档没有 btsmTiles
        fr_lib: 0x11, // → btArea（off10），`PlacedStamps.cs:92` 的 `Front.Library`
        door: 0x2A,
        door_offset: 0x2B,
        light: 0x33,
        ani_frame: 0x05,
        ani_tick: 0x06,
        // 12 B 布局里没有这 5 个字节 ⇒ 一律 0（不猜，见 `selection.rs` 的 `Clipboard::capture`）
        mid_ani_frame: 0,
        mid_ani_tick: 0,
        tile_animation: 0,
        tile_offset: 0,
        tile_frames: 0,
    };
    let (ax, ay) = 找空落点(&ed, 1, 1, 0);
    let out = ed.place_stamp(ax, ay, &[一格]);
    assert!(matches!(out, PlaceOutcome::Placed { cells: 1, skipped: 0 }), "{out:?}");

    // ① 视图层先自证（值真的搬到了 Cell 上）
    let c = *ed.map().cell(ax, ay).expect("图内格");
    assert_eq!(c.bk_img, 0x0123);
    assert_eq!(c.mid_img, 0x0456);
    assert_eq!(c.fr_img, 0x0789);
    assert_eq!(c.door_index, 0x2A, "门索引");
    assert_eq!(c.door_offset, 0x2B, "门偏移");
    assert_eq!(c.ani_frame, 0x05, "前景动画帧");
    assert_eq!(c.ani_tick, 0x06, "前景动画节拍");
    assert_eq!(c.area, 0x11, "物体库号 btArea");
    assert_eq!(c.light, 0x33, "光照");

    // ② 保存后逐字节核对盘上内容（`idx = x * h + y`，`mir2-map` 的 x 外层排布）
    let so = save_session(&mut ed).expect("保存应成功");
    assert!(so.wrote(), "有修改就必须真的写盘：{}", so.note());
    let 新 = std::fs::read(&p).expect("读回保存后的文件");
    assert_eq!(新.len(), 原.len(), "长度必须不变");
    let idx = ax as usize * h as usize + ay as usize;
    let base = 52 + idx * 12;
    assert_eq!(新[base], 0x23, "bk_img 低字节（off0）");
    assert_eq!(新[base + 1], 0x01, "bk_img 高字节（off1）");
    assert_eq!(新[base + 2], 0x56, "mid_img 低字节（off2）");
    assert_eq!(新[base + 3], 0x04, "mid_img 高字节（off3）");
    assert_eq!(新[base + 4], 0x89, "fr_img 低字节（off4）");
    assert_eq!(新[base + 5], 0x07, "fr_img 高字节（off5）");
    assert_eq!(新[base + 6], 0x2A, "门索引（off6）");
    assert_eq!(新[base + 7], 0x2B, "门偏移（off7）");
    assert_eq!(新[base + 8], 0x05, "前景动画帧（off8）");
    assert_eq!(新[base + 9], 0x06, "前景动画节拍（off9）");
    assert_eq!(新[base + 10], 0x11, "物体库号 btArea（off10）");
    assert_eq!(新[base + 11], 0x33, "光照（off11）");
    // ③ 头部与其它格一个字节都不许动
    let 差异: Vec<usize> = (0..原.len()).filter(|i| 原[*i] != 新[*i]).collect();
    assert!(差异.iter().all(|i| *i >= base && *i < base + 12), "差异必须全落在这一格里：{差异:?}");
    let 十六进制 = |b: &[u8]| -> String {
        b.iter().map(|x| format!("{x:02X}")).collect::<Vec<_>>().join(" ")
    };
    println!(
        "GATE-HEX 格 ({ax},{ay}) idx={idx} 偏移 {base}..{}：\n  改前 = {}\n  改后 = {}",
        base + 12,
        十六进制(&原[base..base + 12]),
        十六进制(&新[base..base + 12])
    );
    println!("GATE-HEX 差异字节数={}（全部落在该格 12 B 内）", 差异.len());
    let _ = std::fs::remove_dir_all(&d);
}
