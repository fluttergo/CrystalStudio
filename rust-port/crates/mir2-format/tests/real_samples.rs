//! 真实样本集成测试：`read → write` 必须**逐字节相等**。
//!
//! # 一、样本口径
//!
//! | 相对路径（挂在资产根下） | 文件数 | 说明 |
//! |------|--------|------|
//! | `GodotClient/maps/*.map` | 61 | 客户端现役图 |
//! | `client/热血传奇23周年客户端/Map/*.map` | 1097 | 23 周年客户端全量 |
//! | **合计** | **1158** | 2026-11-04 实测 |
//!
//! 资产根由环境变量 `MIREDIT_ASSET_ROOT` 给出（缺省本机真实根，见 [`资产根`]）；
//! 目录整个可用 `MIR2_MAP_DIRS`（`;` 分隔）覆盖。**根或文件不存在时打印 `[skip]` 并跳过**
//! （不静默通过，也不 panic —— 公开仓/PR 的机器上没有游戏素材）。
//!
//! # 二、两遍跑法
//!
//! 1. **严格遍**（[`mir2_format::read_any`]）：长度必须精确等于「头部 + 格长 × 格数」
//!    （`CrystalCs` 例外，它的格式定义里本来就带可选尾块）。不符合的按规格书要求**报错**，
//!    本测试把每个失败文件的文件名 + 错误原因全部打印出来。
//! 2. **宽松遍**（[`mir2_format::read_any_tolerant`]）：只对严格遍失败的文件再跑一次，
//!    多出的字节进 `RawMap::trailer`。这一遍要求**0 失败 0 字节差异** —— 它证明
//!    「长度多一截」的文件同样可以做到字节级保真，只是不能算作 9 种格式的合法文件。
//!
//! # 三、字段偏移对照
//!
//! [`列优先与字段偏移对照_old12`] / [`列优先与字段偏移对照_shanda14`] 里写死的期望值，
//! 是用**独立的 Python 解码脚本**（不复用本 crate 任何代码）从同一批真实文件里读出来的，
//! 两边逐字段比对 —— 见交付报告第 4 节。

use mir2_format::{read_any, read_any_tolerant, FormatId};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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

/// 默认样本目录（相对资产根；可用 `MIR2_MAP_DIRS` 覆盖）。
fn 默认目录(root: &Path) -> [PathBuf; 2] {
    [
        root.join("GodotClient").join("maps"),
        root.join("client").join("热血传奇23周年客户端").join("Map"),
    ]
}

/// 一个格式的统计。
#[derive(Debug, Default, Clone, Copy)]
struct 统计 {
    成功: usize,
    失败: usize,
    字节差异: usize,
    带尾随: usize,
}

/// 收集待测文件（按目录 + 文件名排序，保证输出可复现）。
fn 收集样本() -> Vec<PathBuf> {
    let dirs: Vec<PathBuf> = match std::env::var("MIR2_MAP_DIRS") {
        Ok(v) if !v.trim().is_empty() => v
            .split(';')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .collect(),
        _ => match 资产根() {
            Some(root) => 默认目录(&root).to_vec(),
            None => {
                eprintln!("[skip] 无资产根 MIREDIT_ASSET_ROOT");
                return Vec::new();
            }
        },
    };
    let mut out = Vec::new();
    for d in dirs {
        if !d.is_dir() {
            eprintln!("跳过目录（不存在）：{}", d.display());
            continue;
        }
        let mut files: Vec<PathBuf> = std::fs::read_dir(&d)
            .unwrap_or_else(|e| panic!("读目录失败 {}：{e}", d.display()))
            .filter_map(|r| r.ok())
            .map(|r| r.path())
            .filter(|p| {
                p.is_file()
                    && p.extension()
                        .and_then(|s| s.to_str())
                        .is_some_and(|s| s.eq_ignore_ascii_case("map"))
            })
            .collect();
        files.sort();
        out.extend(files);
    }
    out
}

/// 打印统计表。
fn 打印统计(标题: &str, 表: &BTreeMap<FormatId, 统计>, 文件总数: usize) {
    println!("\n===== {标题} =====");
    println!(
        "{:<12} {:>6} {:>6} {:>6} {:>8} {:>6}",
        "格式", "成功", "失败", "字节差异", "带尾随", "小计"
    );
    let (mut ok, mut fail, mut diff, mut tr) = (0usize, 0usize, 0usize, 0usize);
    for (id, s) in 表 {
        println!(
            "{:<12} {:>6} {:>6} {:>6} {:>8} {:>6}",
            format!("{id:?}"),
            s.成功,
            s.失败,
            s.字节差异,
            s.带尾随,
            s.成功 + s.失败
        );
        ok += s.成功;
        fail += s.失败;
        diff += s.字节差异;
        tr += s.带尾随;
    }
    println!(
        "{:<12} {:>6} {:>6} {:>6} {:>8} {:>6}",
        "合计", ok, fail, diff, tr, 文件总数
    );
}

/// 严格遍 + 失败清单。
fn 严格遍(文件: &[PathBuf]) -> (BTreeMap<FormatId, 统计>, Vec<(PathBuf, String)>) {
    let mut 表: BTreeMap<FormatId, 统计> = BTreeMap::new();
    let mut 失败清单 = Vec::new();
    for p in 文件 {
        let raw = std::fs::read(p).unwrap_or_else(|e| panic!("读文件失败 {}：{e}", p.display()));
        match read_any(&raw) {
            Ok(map) => {
                let s = 表.entry(map.format).or_default();
                let out = mir2_format::format_by_id(map.format)
                    .write(&map)
                    .unwrap_or_else(|e| panic!("{} 写回失败：{e}", p.display()));
                if out == raw {
                    s.成功 += 1;
                } else {
                    s.字节差异 += 1;
                    panic!(
                        "{} 往返不是逐字节相等：原 {} 字节，写回 {} 字节",
                        p.display(),
                        raw.len(),
                        out.len()
                    );
                }
                if !map.trailer.is_empty() {
                    s.带尾随 += 1;
                }
            }
            Err(e) => {
                失败清单.push((p.clone(), e.to_string()));
                // 失败时不知道格式，记在 Old12 名下无意义 ⇒ 单独计一次「未归类失败」
                表.entry(FormatId::Old12).or_default();
                let _ = e;
            }
        }
    }
    (表, 失败清单)
}

#[test]
fn 真实样本严格遍与宽松遍全量往返() {
    let 文件 = 收集样本();
    if 文件.is_empty() {
        eprintln!("[skip] 没有找到任何 .map 样本（用 MIREDIT_ASSET_ROOT / MIR2_MAP_DIRS 指定）");
        return;
    }
    println!("待测 .map 文件数：{}", 文件.len());

    let (mut 表, 失败清单) = 严格遍(&文件);

    // 失败数要落进表里（严格遍里没有格式可归，统一记在「未归类」= Old12 桶的失败列）
    if !失败清单.is_empty() {
        let e = 表.entry(FormatId::Old12).or_default();
        e.失败 += 失败清单.len();
    }
    打印统计("严格遍（长度必须精确）", &表, 文件.len());

    if !失败清单.is_empty() {
        println!("\n严格遍失败文件（共 {} 个）：", 失败清单.len());
        for (p, msg) in &失败清单 {
            println!("  - {} :: {msg}", p.display());
        }
    }

    // 宽松遍：只对严格遍失败的文件再跑，要求 0 失败 0 字节差异
    let mut 宽松成功 = 0usize;
    let mut 宽松带尾随 = 0usize;
    let mut 宽松失败 = Vec::new();
    for (p, _) in &失败清单 {
        let raw = std::fs::read(p).unwrap();
        match read_any_tolerant(&raw) {
            Ok(map) => {
                let out = mir2_format::format_by_id(map.format).write(&map).unwrap();
                assert!(
                    out == raw,
                    "{} 宽松遍往返不是逐字节相等（原 {} 字节，写回 {} 字节）",
                    p.display(),
                    raw.len(),
                    out.len()
                );
                宽松成功 += 1;
                if !map.trailer.is_empty() {
                    宽松带尾随 += 1;
                }
            }
            Err(e) => 宽松失败.push((p.clone(), e.to_string())),
        }
    }
    println!(
        "\n===== 宽松遍（只跑严格遍失败的 {} 个）=====\n成功 {}（其中带尾随 {}）/ 失败 {}",
        失败清单.len(),
        宽松成功,
        宽松带尾随,
        宽松失败.len()
    );
    for (p, msg) in &宽松失败 {
        println!("  - {} :: {msg}", p.display());
    }

    // 硬断言：严格遍 0 字节差异；宽松遍 0 失败
    let 严格差异: usize = 表.values().map(|s| s.字节差异).sum();
    assert_eq!(严格差异, 0, "严格遍存在字节差异");
    assert_eq!(宽松失败.len(), 0, "宽松遍仍有读不了的文件");
}

// ─────────────────────────── 列优先 / 字段偏移对照 ───────────────────────────

/// 断言一条格记录与 Python 独立解码结果逐字段相等。
#[allow(clippy::too_many_arguments)]
fn 断言_old12(p: &Path, x: u16, y: u16, 期望: (u32, u16, u16, u8, u8, u8, u8, u8, u8)) {
    let raw = std::fs::read(p).unwrap_or_else(|e| panic!("读 {} 失败：{e}", p.display()));
    let map = read_any(&raw).unwrap_or_else(|e| panic!("解析 {} 失败：{e}", p.display()));
    assert_eq!(map.format, FormatId::Old12);
    let c = map
        .cell(x, y)
        .unwrap_or_else(|| panic!("{} ({x},{y}) 应在图内", p.display()));
    let 实测 = (
        c.bk_img,
        c.mid_img,
        c.fr_img,
        c.door,
        c.door_offset,
        c.ani_frame,
        c.ani_tick,
        c.area,
        c.light,
    );
    assert_eq!(
        实测,
        期望,
        "{} ({x},{y}) 的字段与 Python 解码不一致",
        p.display()
    );
}

/// `Old12` 真实样本：`GodotClient/maps/0.map`（700×700，12 B/格）。
///
/// 期望值来自独立 Python 解码（`off = 52 + (x*h + y)*12`）：
/// `(0,0) idx=0 off=52 bk=34620`；`(100,200) idx=70200 off=842452 bk=1454`；
/// `(699,699) idx=489999 off=5880040 bk=32768`。
#[test]
fn 列优先与字段偏移对照_old12() {
    let Some(root) = 资产根() else {
        eprintln!("[skip] 无资产根 MIREDIT_ASSET_ROOT");
        return;
    };
    let p = root.join("GodotClient").join("maps").join("0.map");
    if !p.is_file() {
        eprintln!("[skip] 缺文件 {}", p.display());
        return;
    }
    let raw = std::fs::read(&p).unwrap();
    let map = read_any(&raw).unwrap();
    assert_eq!((map.width, map.height), (700, 700));
    assert_eq!(map.cells.len(), 700 * 700);

    断言_old12(&p, 0, 0, (34620, 0, 0, 0, 0, 0, 0, 0, 0));
    断言_old12(&p, 100, 200, (1454, 0, 0, 0, 0, 0, 0, 0, 0));
    断言_old12(&p, 699, 699, (32768, 0, 0, 0, 0, 0, 0, 0, 0));

    // 列优先：idx 必须是 x*h + y（行优先会是 y*w + x）
    let h = 700usize;
    assert_eq!(
        map.cells[100 * h + 200].bk_img,
        1454,
        "cells[x*h+y] 才该是 (100,200)"
    );
    assert_ne!(
        map.cells[200 * 700 + 100].bk_img,
        1454,
        "(200,100) 不该有同一个值 ⇒ 排除行优先"
    );
}

/// `Shanda14` 真实样本：`GodotClient/maps/11.map`（160×165，14 B/格）。
///
/// 期望值来自独立 Python 解码（`off = 52 + (x*h + y)*14`）：
/// `(0,0) off=52 bk=34722 fr=32768`；`(80,90) off=186112 bk=5 fr=1 tiles=10`；
/// `(159,164) off=369638 bk=1953 fr=1`。
#[test]
fn 列优先与字段偏移对照_shanda14() {
    let Some(root) = 资产根() else {
        eprintln!("[skip] 无资产根 MIREDIT_ASSET_ROOT");
        return;
    };
    let p = root.join("GodotClient").join("maps").join("11.map");
    if !p.is_file() {
        eprintln!("[skip] 缺文件 {}", p.display());
        return;
    }
    let raw = std::fs::read(&p).unwrap();
    let map = read_any(&raw).unwrap();
    assert_eq!(map.format, FormatId::Shanda14, "11.map 应判成 14 B/格");
    assert_eq!((map.width, map.height), (160, 165));
    assert_eq!(map.cells.len(), 160 * 165);

    let 取 = |x: u16, y: u16| {
        let c = map
            .cell(x, y)
            .unwrap_or_else(|| panic!("( {x},{y}) 应在图内"));
        (c.bk_img, c.mid_img, c.fr_img, c.tiles, c.sm_tiles, c.light)
    };
    assert_eq!(取(0, 0), (34722, 0, 32768, 0, 0, 0));
    assert_eq!(取(80, 90), (5, 0, 1, 10, 0, 0));
    assert_eq!(取(159, 164), (1953, 0, 1, 0, 0, 0));

    // 列优先：idx = x*h + y
    let h = 165usize;
    assert_eq!(map.cells[80 * h + 90].bk_img, 5);
    assert_ne!(map.cells[90 * 160 + 80].bk_img, 5, "排除行优先");
}

// ─────────────────── ④ Type3 尾区三字段：与 Python 独立解码对照 ───────────────────

/// 对一张真实 36 B/格文件，按 `(x,y)` 断言三个尾区访问器的值。
///
/// 期望值来自**独立的 Python 脚本**（不复用本 crate 任何代码）：
/// `off = 52 + (x*h + y)*36`，然后
/// `TileAnimationImage = i16@off+14`、`TileAnimationFrames = u8@off+21`、
/// `TileAnimationOffset = i16@off+22`（对应 C# `LegacyMapReader.cs:300` / `:302` / `:303`）。
fn 断言_type3尾区(
    p: &Path,
    尺寸: (u16, u16),
    点: &[(u16, u16, u16, u8, u16)],
) {
    use mir2_format::formats::shanda2012::Shanda2012;

    if !p.is_file() {
        eprintln!("[skip] 缺文件 {}", p.display());
        return;
    }
    let raw = std::fs::read(p).unwrap();
    assert_eq!(
        raw.len() as u64,
        52 + 36 * u64::from(尺寸.0) * u64::from(尺寸.1),
        "{} 的长度应是 52 + 36*w*h",
        p.display()
    );
    let map =
        read_any(&raw).unwrap_or_else(|e| panic!("{} 应解析成功：{e}", p.display()));
    assert_eq!(
        map.format,
        FormatId::Shanda2012,
        "{} 应判成 Shanda2012",
        p.display()
    );
    assert_eq!((map.width, map.height), 尺寸, "{} 的宽高不对", p.display());

    for &(x, y, 图, 帧, 偏移) in 点 {
        let c = map
            .cell(x, y)
            .unwrap_or_else(|| panic!("{} ({x},{y}) 应在图内", p.display()));
        assert_eq!(
            Shanda2012::tile_anim_image(c),
            Some(图),
            "{} ({x},{y}) 的 TileAnimationImage（rel 14-15）与 Python 解码不一致",
            p.display()
        );
        assert_eq!(
            Shanda2012::tile_anim_frames(c),
            Some(帧),
            "{} ({x},{y}) 的 TileAnimationFrames（rel 21）与 Python 解码不一致",
            p.display()
        );
        assert_eq!(
            Shanda2012::tile_anim_offset(c),
            Some(偏移),
            "{} ({x},{y}) 的 TileAnimationOffset（rel 22-23）与 Python 解码不一致",
            p.display()
        );
    }

    // 往返必须仍然 0 差异（访问器不参与写）
    assert_eq!(
        mir2_format::format_by_id(map.format).write(&map).unwrap(),
        raw,
        "{} 往返不是逐字节相等",
        p.display()
    );
}

/// ④ 真实 36 B/格文件的尾区三字段 —— **三张文件、9 个取样点**，全部与 Python 独立解码一致。
///
/// ⚠ 为什么不用任务书点名的 `n0104.map` 单独作证：它的尾区在取样点上**全 0**
/// （`(0,0)` 与 `(19,19)` 都是 `0/0/0`），`0 == 0` 证明力弱。
/// 因此这里**同时**用两张尾区确有非零数据的真实文件（`n0155.map` / `nGA0.map`）作证，
/// 否则「访问器读错偏移」这种缺陷会被全零样本掩盖。
#[test]
fn type3尾区三字段与python独立解码一致() {
    let Some(root) = 资产根() else {
        eprintln!("[skip] 无资产根 MIREDIT_ASSET_ROOT");
        return;
    };
    let 语料 = root.join("client").join("热血传奇23周年客户端").join("Map");

    // n0104.map：20×20 = 14452 B，取样点尾区全 0（作「全零也一致」的对照）
    断言_type3尾区(
        &语料.join("n0104.map"),
        (20, 20),
        &[(0, 0, 0, 0, 0), (19, 19, 0, 0, 0)],
    );

    // n0155.map：50×50 = 90052 B，尾区有真实非零数据（Python 实测只有 4 个格非零）
    断言_type3尾区(
        &语料.join("n0155.map"),
        (50, 50),
        &[
            (5, 16, 8419, 49, 8451),
            (5, 32, 8418, 32, 8419),
            (5, 0, 6306, 0, 0),
            (5, 27, 8483, 0, 0),
        ],
    );

    // nGA0.map：150×160 = 864052 B，尾区有 114 个非零格（图块动画密集）
    断言_type3尾区(
        &语料.join("nGA0.map"),
        (150, 160),
        &[(42, 69, 981, 20, 8257), (43, 68, 985, 20, 8257)],
    );
}

// ─────────────────── ⑦ tolerant 路径「吞掉多少字节」必须可查 ───────────────────

/// ⑦ 那 67 张长度不自洽的文件：宽松入口读出的 `trailer.len()` 必须 **正好 8040**。
///
/// 口径（Python 独立复算）：
/// - 文件长度 `20,692`；
/// - 头部声明 `30 × 35 = 1050` 格，`52 + 12 × 1050 = 12,652`；
/// - `20,692 − 12,652 = **8,040**`（= `670` 格，占 `(len-52)/12 = 1720` 格的 **39%**）。
///
/// 同时断言：严格入口**必须拒绝**这类文件（`LengthMismatch`），
/// 即它们**不会**被静默当成正常文件读进来。
#[test]
fn 宽松入口暴露未解释字节数为8040() {
    let Some(root) = 资产根() else {
        eprintln!("[skip] 无资产根 MIREDIT_ASSET_ROOT");
        return;
    };
    let p = root
        .join("client")
        .join("热血传奇23周年客户端")
        .join("Map")
        .join("EM001.map");
    if !p.is_file() {
        eprintln!("[skip] 缺文件 {}", p.display());
        return;
    }
    let raw = std::fs::read(&p).unwrap();
    assert_eq!(raw.len(), 20_692, "样本长度实测应为 20692");

    // 严格入口必须拒绝
    match read_any(&raw) {
        Err(mir2_format::FormatError::LengthMismatch {
            format,
            width,
            height,
            len,
            expected,
        }) => {
            assert_eq!(format, FormatId::Old12);
            assert_eq!((width, height), (30, 35), "头部声明 30×35");
            assert_eq!(len, 20_692);
            assert_eq!(expected, 12_652, "52 + 12*30*35 = 12652");
        }
        other => panic!("严格入口应报 LengthMismatch，实际 {other:?}"),
    }

    // 宽松入口能读，且「吞了多少」可查
    let map = read_any_tolerant(&raw).expect("宽松入口应能读");
    assert_eq!(map.format, FormatId::Old12);
    assert_eq!((map.width, map.height), (30, 35));
    assert_eq!(map.cells.len(), 1050, "只有 1050 格进了 cells");
    assert_eq!(
        map.trailer_len(),
        8_040,
        "trailer.len() 必须是 8040（= 20692 - 12652，670 格，占 39%）"
    );
    assert!(
        map.has_unexplained_trailer(),
        "非空 trailer 必须让 has_unexplained_trailer() 为 true"
    );
    // 8040 = 670 格 × 12 字节
    assert_eq!(map.trailer_len(), 670 * 12, "8040 正好是 670 个 12 字节格");

    // 字节级往返仍然 0 差异
    assert_eq!(
        mir2_format::format_by_id(map.format).write(&map).unwrap(),
        raw,
        "宽松入口读出的图写回必须逐字节相等"
    );
}

// ─────────────────── ⑧ 告警覆盖实测（P1-9 门 4） ───────────────────

/// ⑧ **告警覆盖实测**：全量真实语料里有多少张图会挂常驻告警，逐格式列清。
///
/// # 判据（与 `miredit-core/src/document.rs` 的 `edit_warning` **一一对应**）
///
/// 两条来源，任一成立即出告警：
/// 1. `RawMap::trailer_len() > 0` ⇒ 「未解尾块」告警（真警告，任何改动都不许削弱）；
/// 2. `caps().editable == false` ⇒ 「格式不承诺改格后语义自洽」告警。
///
/// # 为什么这条要存在（P1-9 的量化门）
///
/// 2026-11-05 之前 `editable` 只有 `Shanda14` 为真 ⇒ 除它之外**全部**真实图都挂黄条
/// （实测 1,051 / 1,158 = 90.8%，含缺省打开的 `0.map`）；改判后 `Old12` 不再挂，
/// 只剩「真尾块」+ `Shanda2012`。这条测试把「到底多少张会告警」变成可复跑的数字，
/// 免得下次又靠肉眼估。
///
/// # 重活，默认跳过
///
/// 读 1,158 个文件 / 约 768 MB，默认**不跑**。要跑：
///
/// ```text
/// $env:MIR2_GATE_CORPUS='1'
/// cargo test -p mir2-format --test real_samples 告警覆盖实测 -- --nocapture
/// ```
#[test]
fn 告警覆盖实测() {
    if std::env::var("MIR2_GATE_CORPUS").ok().as_deref() != Some("1") {
        eprintln!("跳过 告警覆盖实测：需要 MIR2_GATE_CORPUS=1（重活：读全量真实语料）");
        return;
    }
    /// 一种格式的告警统计。
    #[derive(Debug, Default, Clone, Copy)]
    struct 告警统计 {
        文件数: usize,
        会告警: usize,
        因尾块: usize,
        因格式: usize,
    }
    let 文件 = 收集样本();
    assert!(!文件.is_empty(), "样本目录一个文件都没读到，统计无意义");
    let mut 表: BTreeMap<FormatId, 告警统计> = BTreeMap::new();
    let mut 可编辑: BTreeMap<FormatId, bool> = BTreeMap::new();
    // `0.map` 单点核对（缺省打开的那张：Old12、无尾块 ⇒ 必须不再告警）。
    // ⚠ 两个样本目录里各有一张 `0.map`（`GodotClient/maps` 与 23 周年 `Map`）⇒ 收集全部、逐张核。
    let mut 零点告警: Vec<String> = Vec::new();

    for p in &文件 {
        let bytes = std::fs::read(p).unwrap_or_else(|e| panic!("读文件失败 {}：{e}", p.display()));
        let map = read_any_tolerant(&bytes)
            .unwrap_or_else(|e| panic!("{} 应能被宽松入口读进来：{e}", p.display()));
        let caps = mir2_format::format_by_id(map.format).caps();
        可编辑.insert(map.format, caps.editable);
        let 因尾块 = map.trailer_len() > 0;
        let 因格式 = !caps.editable;
        let s = 表.entry(map.format).or_default();
        s.文件数 += 1;
        if 因尾块 {
            s.因尾块 += 1;
        }
        if 因格式 {
            s.因格式 += 1;
        }
        if 因尾块 || 因格式 {
            s.会告警 += 1;
        }
        if p.file_name().and_then(|n| n.to_str()) == Some("0.map") {
            零点告警.push(format!(
                "{}({:?}) trailer={} editable={} ⇒ {}",
                p.display(),
                map.format,
                map.trailer_len(),
                caps.editable,
                if 因尾块 || 因格式 { "会告警" } else { "不告警" }
            ));
        }
    }

    println!("\n===== 告警覆盖实测（P1-9 门 4；判据见本测试文档） =====");
    println!(
        "{:<12} {:>6} {:>8} {:>7} {:>7} {:>7}",
        "格式", "文件数", "editable", "会告警", "因尾块", "因格式"
    );
    let (mut 总, mut 总告警, mut 总尾, mut 总格式) = (0usize, 0usize, 0usize, 0usize);
    for (id, s) in &表 {
        println!(
            "{:<12} {:>6} {:>8} {:>7} {:>7} {:>7}",
            format!("{id:?}"),
            s.文件数,
            if 可编辑.get(id).copied().unwrap_or(false) { "true" } else { "false" },
            s.会告警,
            s.因尾块,
            s.因格式
        );
        总 += s.文件数;
        总告警 += s.会告警;
        总尾 += s.因尾块;
        总格式 += s.因格式;
    }
    println!(
        "{:<12} {:>6} {:>8} {:>7} {:>7} {:>7}",
        "合计", 总, "-", 总告警, 总尾, 总格式
    );
    for line in &零点告警 {
        println!("0.map 单点：{line}");
    }

    // ① 每张告警都必须能归到某一条来源（不许出现"来源不明"的告警）
    assert_eq!(总告警, 总尾 + 总格式, "会告警数必须等于两条来源之和");
    // ② `Old12` 的告警只可能来自尾块（它已 `editable = true`）—— 这是 P1-9 的核心
    let old12 = 表.get(&FormatId::Old12).copied().unwrap_or_default();
    assert_eq!(
        old12.因格式, 0,
        "Old12 已 editable=true ⇒ 不许再出「格式」来源的告警（P1-9）"
    );
    assert_eq!(
        old12.会告警, old12.因尾块,
        "Old12 的告警数必须正好等于它带尾块的文件数"
    );
    // ③ 两张 `0.map`（缺省打开的那张）都必须不再告警
    assert!(!零点告警.is_empty(), "语料里应当有 0.map");
    for line in &零点告警 {
        assert!(line.ends_with("不告警"), "0.map 必须不再出告警：{line}");
    }
}
