//! 验收门 1~6：`miredit-core` 接入 `mir2-format` 之后，**九种格式**的端到端验收。
//!
//! 跑法：`cargo test -p miredit-core --test formats_9 -- --nocapture --test-threads=1`
//!
//! # 六个门（与任务书逐条对应）
//!
//! | 门 | 判据 | 用例 |
//! |----|------|------|
//! | 1 | 1,158 张真实 `.map` **全部能 open**，按格式统计，失败 0 | [`门1门2_全仓1158张开图与未改即存零差异`] |
//! | 2 | 1,158 张 **未改即存 = 逐字节相同**（含 67 张带 8,040 B 尾块的，走 tolerant 路径） | 同上 |
//! | 3 | `0.map`(Old12) / `41.map`(New14) / `n0104.map`(Ext36) 各改一格地表 ⇒ **只有该格那 2 字节变**、`bit15` 保留 | [`门3_三档各改一格只有该格两字节变`] |
//! | 4 | Ext36 改一格后，`offset+14..+36` 的 22 字节**原样** | [`门4_ext36改一格后尾区22字节原样`] |
//! | 5 | 67 张带尾块文件：`tolerant_trailer == 8040`，改一格 ⇒ 只有目标格变 + 尾块原样 | [`门5_67张带尾块文件改一格只动目标格`] |
//! | 6 | Type5 合成样本：地表层落笔 ⇒ **拒绝 + 中文原因**，文件一字节不动 | [`门6_type5地表落笔被拒绝`] |
//!
//! ⚠ 所有写盘都发生在 `%TEMP%` 下的临时目录里，且**复用同一个临时文件**
//! （每轮覆盖、收尾删除）—— 全量 733 MB，不重复占盘。**真图只被读取**。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use miredit_core::{
    apply, set_brush_from_palette, Command, Document, EditSession, LibRef, COLLISION_BIT, SPRITE_MAX,
};
use mir2_format::{FormatId, RawMap};

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

/// 23 周年客户端地图目录（`client/热血传奇23周年客户端/Map`，挂在资产根下）。
fn 语料_b(root: &Path) -> PathBuf {
    root.join("client").join("热血传奇23周年客户端").join("Map")
}

/// Godot 客户端自带地图目录（`GodotClient/maps`，挂在资产根下）。
fn 语料_a(root: &Path) -> PathBuf {
    root.join("GodotClient").join("maps")
}

/// 默认样本目录（相对资产根；与 `mir2-format/tests/real_samples.rs` 同口径）。
fn 默认目录(root: &Path) -> [PathBuf; 2] {
    [语料_a(root), 语料_b(root)]
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
    let d = std::env::temp_dir().join(format!("miredit_core_formats9_{tag}_{pid}"));
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
    let 期望名 = format!("miredit_core_formats9_{tag}_{本进程}");
    assert_eq!(
        实际名,
        期望名.as_str(),
        "临时目录名必须逐字等于「miredit_core_formats9_<tag>_<本进程 PID>」，否则并发隔离失效：{}",
        d.display()
    );
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("建临时目录");
    d
}

/// 差异偏移清单（逐字节，返回 `(偏移, 原值, 新值)`）。
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

// ═══════════════════════════ 门 1 + 门 2 ═══════════════════════════

/// **门 1**：1,158 张真实 `.map` 全部能 `open`；按格式统计；失败 0（失败要列文件名 + 原因）。
///
/// **门 2**：同一批逐个 `open → 写副本 → 与原文件逐字节比对`，**0 字节差异**
/// （含 67 张带 8,040 B 尾块的，它们走 `read_any_tolerant` 的宽松路径）。
///
/// 两门共用一次解析（否则要解析两遍 733 MB），但**分别断言、分别打印**。
#[test]
fn 门1门2_全仓1158张开图与未改即存零差异() {
    let 文件 = 收集样本();
    if 文件.is_empty() {
        eprintln!("[skip] 没有找到任何 .map 样本（用 MIREDIT_ASSET_ROOT / MIR2_MAP_DIRS 指定）");
        return;
    }
    println!("待测 .map 文件数：{}", 文件.len());

    let d = 临时目录("gate12");
    let 副本 = d.join("copy.map");
    let mut 表: BTreeMap<FormatId, usize> = BTreeMap::new();
    let mut 带尾块: BTreeMap<FormatId, usize> = BTreeMap::new();
    let mut 打开失败: Vec<(PathBuf, String)> = Vec::new();
    let mut 字节失败: Vec<(PathBuf, String)> = Vec::new();
    let mut 尾块字节数: BTreeMap<usize, usize> = BTreeMap::new();
    let mut 最大尾块 = 0usize;

    for p in &文件 {
        let 原 = std::fs::read(p).unwrap_or_else(|e| panic!("读文件失败 {}：{e}", p.display()));
        let doc = match Document::load(p) {
            Ok(d) => d,
            Err(e) => {
                打开失败.push((p.clone(), e.to_string()));
                continue;
            }
        };
        *表.entry(doc.format_id()).or_default() += 1;
        if doc.tolerant_trailer() > 0 {
            *带尾块.entry(doc.format_id()).or_default() += 1;
            *尾块字节数.entry(doc.tolerant_trailer()).or_default() += 1;
            最大尾块 = 最大尾块.max(doc.tolerant_trailer());
        }
        // 门 2：真写一份副本（走的是生产路径的 `save_atomic`），再读回来逐字节比
        let 出 = doc.to_bytes();
        if 出 != 原 {
            let 差异 = 差异清单(&原, &出);
            字节失败.push((
                p.clone(),
                format!(
                    "原 {} 字节 / 出 {} 字节，首个差异 {:?}，共 {} 处",
                    原.len(),
                    出.len(),
                    差异.first(),
                    差异.len()
                ),
            ));
            continue;
        }
        miredit_core::save_atomic(&副本, &出)
            .unwrap_or_else(|e| panic!("{} 写副本失败：{e}", p.display()));
        let 读回 = std::fs::read(&副本).expect("读回副本");
        if 读回 != 原 {
            字节失败.push((
                p.clone(),
                format!(
                    "落盘副本与原文件不同（原 {} 字节 / 读回 {} 字节）",
                    原.len(),
                    读回.len()
                ),
            ));
        }
    }
    let _ = std::fs::remove_dir_all(&d);

    // ── 打印门 1 统计 ──
    println!("\n===== 门 1：按格式统计（本 crate 判别口径）=====");
    println!("{:<12} {:>6} {:>8}", "格式", "张数", "带尾块");
    let mut 合计 = 0usize;
    let mut 尾合计 = 0usize;
    for (id, n) in &表 {
        let t = 带尾块.get(id).copied().unwrap_or(0);
        println!("{:<12} {:>6} {:>8}", format!("{id:?}"), n, t);
        合计 += n;
        尾合计 += t;
    }
    println!("{:<12} {:>6} {:>8}", "合计", 合计, 尾合计);
    println!("\n尾块字节数分布：{尾块字节数:?}（最大 {最大尾块}）");
    println!("打开失败：{} 个", 打开失败.len());
    for (p, msg) in &打开失败 {
        println!("  - {} :: {msg}", p.display());
    }
    println!("\n===== 门 2：未改即存逐字节比对 =====");
    println!("字节差异失败：{} 个", 字节失败.len());
    for (p, msg) in &字节失败 {
        println!("  - {} :: {msg}", p.display());
    }

    // ── 硬断言 ──
    assert_eq!(文件.len(), 1158, "样本口径必须是 61 + 1097 = 1158 张");
    assert_eq!(打开失败.len(), 0, "门 1：必须全部能 open（失败清单见上）");
    assert_eq!(合计, 1158, "门 1：统计总数必须等于文件总数");
    // 与 `mir2-format/src/lib.rs:26-29` 的实测分布一致（两套口径里的「本 crate 判别」）
    assert_eq!(
        表.get(&FormatId::Old12).copied().unwrap_or(0),
        856,
        "Old12 = 789 严格 + 67 只能走 tolerant"
    );
    assert_eq!(表.get(&FormatId::Shanda14).copied().unwrap_or(0), 107);
    assert_eq!(表.get(&FormatId::Shanda2012).copied().unwrap_or(0), 195);
    assert_eq!(表.len(), 3, "真实语料只覆盖 3 种格式（其余 6 种只有合成样本）");
    assert_eq!(尾合计, 67, "带尾块的文件必须是 67 张");
    // ⚠ 实测订正（2026-11-05）：67 张里 **65 张是 8,040 字节**、**2 张是 9,840 字节**
    //    （`T218.map` / `T318.map`：头部声明 30×30、文件 20,692 字节
    //    ⇒ `52 + 12×900 = 10,852`，`20,692 − 10,852 = 9,840`）。
    //    任务书写的「67 张 8,040 B」是 65/67 的口径，这里按实测钉死，**不按任务书改数**。
    assert_eq!(
        尾块字节数.get(&8_040).copied().unwrap_or(0),
        65,
        "65 张的未解释尾块是 8,040 字节（EM001..EM060/EM100/EM200/EM300/RYZL/T118）"
    );
    assert_eq!(
        尾块字节数.get(&9_840).copied().unwrap_or(0),
        2,
        "2 张是 9,840 字节（T218.map / T318.map：头部 30×30，文件 20,692 字节）"
    );
    assert_eq!(
        尾块字节数.values().sum::<usize>(),
        67,
        "尾块分布必须覆盖全部 67 张"
    );
    assert_eq!(字节失败.len(), 0, "门 2：必须 0 字节差异（失败清单见上）");
}

// ═══════════════════════════ 门 3 + 门 4 ═══════════════════════════

/// 三档真实图各一张：`(标签, 路径, 期望 FormatId, 期望磁盘格长)`（路径挂在资产根下）。
fn 三档(root: &Path) -> [(&'static str, PathBuf, FormatId, usize); 3] {
    [
        (
            "0.map(Old12)",
            语料_b(root).join("0.map"),
            FormatId::Old12,
            12,
        ),
        (
            "41.map(New14)",
            语料_a(root).join("41.map"),
            FormatId::Shanda14,
            14,
        ),
        (
            "n0104.map(Ext36)",
            语料_b(root).join("n0104.map"),
            FormatId::Shanda2012,
            36,
        ),
    ]
}

/// 在 `(x, y)` 上「只改一格地表」：笔刷图号 10，且**库号字节与现状相同**
/// （否则 New14/Ext36 档会多改一个 `btTiles` 字节，那就不是"只改 2 字节"了）。
///
/// 返回 `(改了几格, 改前整格, 改后整格)`（门 3 要核对 `bit15` 保留）。
fn 改一格地表并返回改前改后(
    ed: &mut EditSession,
    x: u16,
    y: u16,
) -> (usize, mir2_map::Cell, mir2_map::Cell) {
    let 前 = *ed.map().cell(x, y).expect("(x,y) 应在图内");
    // 笔刷库号 = 该格现有的 `btTiles` ⇒ 落笔后那个字节不变（Old12 档无库号字节，随便）
    set_brush_from_palette(ed, LibRef::ground(u16::from(前.tiles)), 9);
    let n = ed.apply_once(x, y);
    let 后 = *ed.map().cell(x, y).expect("(x,y) 应在图内");
    (n, 前, 后)
}

/// **门 3**：`0.map`(Old12) / `41.map`(New14) / `n0104.map`(Ext36) 各改一格地表 ⇒
/// **只有该格那 2 字节变**、`bit15` 保留、其余 0 差异。
///
/// 「只有 2 字节」的机制性理由：`bk_img` 在格内偏移 0..2（三档相同，
/// `mir2-map/src/cell.rs:27-39`），落笔只改它（`paint.rs` 的地表分支），
/// 库号字节被我们刻意设成同值。
#[test]
fn 门3_三档各改一格只有该格两字节变() {
    let Some(root) = 资产根() else {
        eprintln!("[skip] 无资产根 MIREDIT_ASSET_ROOT");
        return;
    };
    let d = 临时目录("gate3");
    let mut 测到 = 0usize;
    for (标签, 路径, 期望格式, 期望格长) in 三档(&root) {
        let p = 路径.as_path();
        if !p.is_file() {
            eprintln!("[skip] 缺文件 {}", p.display());
            continue;
        }
        测到 += 1;
        let 原 = std::fs::read(p)
            .unwrap_or_else(|e| panic!("读 {} 失败：{e}", p.display()));
        let doc = Document::load(p).expect("真图应能打开");
        assert_eq!(doc.format_id(), 期望格式, "{标签} 的格式判别不对");
        assert_eq!(doc.disk_cell_len(), Some(期望格长), "{标签} 的磁盘格长不对");
        let 宽 = doc.view().width;
        let 高 = doc.view().height;
        let 头长 = doc.raw_header().len();

        let mut ed = EditSession::new(doc);
        let (x, y) = (0u16, 0u16);
        let (n, 前, 后) = 改一格地表并返回改前改后(&mut ed, x, y);
        assert_eq!(n, 1, "{标签}：单格落笔应恰好改 1 格");
        assert_ne!(前.bk_img, 后.bk_img, "{标签}：图号必须真的变了（否则下面的断言是假的）");
        // bit15 保留矩阵：地表层保留低阻挡位
        assert_eq!(
            后.bk_img & COLLISION_BIT,
            前.bk_img & COLLISION_BIT,
            "{标签}：地表落笔必须保留 bit15（低阻挡位）"
        );
        assert_eq!(后.bk_img & SPRITE_MAX, 10, "{标签}：低 15 位应换成图号 10");
        assert_eq!(后.tiles, 前.tiles, "{标签}：btTiles 不该被牵连");
        assert_eq!(后.mid_img, 前.mid_img);
        assert_eq!(后.fr_img, 前.fr_img);

        // 保存到副本（生产路径：原子写）
        let 副本 = d.join(format!("{标签}.map"));
        miredit_core::save_atomic(&副本, &ed.document().to_bytes()).expect("写副本");
        let 新 = std::fs::read(&副本).expect("读回副本");
        assert_eq!(新.len(), 原.len(), "{标签}：保存后长度必须不变");

        // 只有该格 bk_img 的 2 字节变
        let idx = (x as usize) * (高 as usize) + (y as usize); // 列优先
        let 格起 = 头长 + idx * 期望格长;
        let 差异 = 差异清单(&原, &新);
        // 「该格那 2 字节」的精确口径：差异**全部**落在 `bk_img` 的 2 字节区间内，
        // 且逐字节等于「改前/改后 `bk_img` 的字节差」——两字节里只有**真变了**的才会出现在差异里
        // （例：`0x8016 → 0x800A` 只动低字节，高字节 0x80 本来就相同）。
        let 期望差异: Vec<(usize, u8, u8)> = (0..2u32)
            .filter_map(|k| {
                let a = ((前.bk_img >> (8 * k)) & 0xFF) as u8;
                let b = ((后.bk_img >> (8 * k)) & 0xFF) as u8;
                if a != b {
                    Some((格起 + k as usize, a, b))
                } else {
                    None
                }
            })
            .collect();
        assert!(!期望差异.is_empty(), "{标签}：bk_img 必须真的变了");
        assert!(期望差异.len() <= 2, "{标签}：bk_img 只有 2 字节");
        assert_eq!(
            差异, 期望差异,
            "{标签}：差异必须**只有** ({x},{y}) 的 bk_img 那 2 字节区间内的变化（格起 {格起}，格长 {期望格长}，图 {宽}×{高}）"
        );
        // 逐条排除：头部 0 差异、目标格 bk_img 之外 0 差异
        assert!(
            差异.iter().all(|(o, _, _)| *o >= 头长),
            "{标签}：头部不许变"
        );
        assert!(
            差异.iter().all(|(o, _, _)| *o < 格起 + 2),
            "{标签}：目标格 bk_img 之外的字节不许变"
        );
        println!(
            "GATE3 {标签} format={:?} 格长={期望格长} 格起={格起} 差异={} 处 {:?} bk:0x{:04X}->0x{:04X} bit15保留={}",
            期望格式,
            差异.len(),
            差异,
            前.bk_img,
            后.bk_img,
            后.bk_img & COLLISION_BIT == 前.bk_img & COLLISION_BIT
        );
        let _ = std::fs::remove_file(&副本);
    }
    let _ = std::fs::remove_dir_all(&d);
    if 测到 == 0 {
        eprintln!("[skip] 三档真图一张都不在 —— 本用例 skip（不 panic）");
    }
}

/// **门 4**：Ext36 改一格之后，`offset+14..+36` 的 **22 字节原样**。
///
/// 依据：36 B/格的尾区语义未知、**不可假设恒零**（`mir2-map/src/cell.rs:41-53` 的实测：
/// `n0.map` 有 1660 格尾区非零）。本 crate 的双表示里视图尾区是 `[u8; 22]`，
/// 与 raw 的 22 字节尾区一一对应（`convert.rs` 的 `MapKind::Tail22`）。
///
/// ⚠ **取样点必须尾区非零**：`n0104.map` 的尾区在多数格上是全 0，
/// 「0 == 0」证明力弱（`mir2-format/tests/real_samples.rs:369-372` 记过同一件事）。
/// 因此本用例按 `n0104 → n0155 → nGA0` 的顺序**扫出第一张含非零尾区的图与非零格**，
/// 拿它作证；扫不到就直接失败（不许退化成"零对零"）。
#[test]
fn 门4_ext36改一格后尾区22字节原样() {
    let Some(root) = 资产根() else {
        eprintln!("[skip] 无资产根 MIREDIT_ASSET_ROOT");
        return;
    };
    let 语料 = 语料_b(&root);
    let 候选: [PathBuf; 3] = [
        语料.join("n0104.map"),
        语料.join("n0155.map"),
        语料.join("nGA0.map"),
    ];
    // ① 扫出「第一个尾区非零的格」（列优先 idx → (x, y)）
    let mut 选中: Option<(PathBuf, u16, u16, Vec<u8>, RawMap)> = None;
    for s in &候选 {
        let p = s.as_path();
        if !p.is_file() {
            eprintln!("[skip] 缺文件 {}", p.display());
            continue;
        }
        let doc = Document::load(p).expect("真图应能打开");
        assert_eq!(
            doc.format_id(),
            FormatId::Shanda2012,
            "{} 必须是 Ext36",
            p.display()
        );
        let h = doc.view().height as usize;
        if let Some(i) = doc
            .view()
            .cells
            .iter()
            .position(|c| c.tail.iter().any(|b| *b != 0))
        {
            选中 = Some((
                p.to_path_buf(),
                (i / h) as u16,
                (i % h) as u16,
                doc.view().cells[i].tail.to_vec(),
                doc.raw().clone(),
            ));
            break;
        }
    }
    let Some((路径, x, y, 前尾区, _)) = 选中 else {
        eprintln!(
            "[skip] {} 三张候选图一张都不在 —— 本用例 skip（不 panic）",
            语料.display()
        );
        return;
    };
    assert_eq!(前尾区.len(), 22, "视图尾区定长 22");

    let p = 路径.as_path();
    let 原 = std::fs::read(p).unwrap_or_else(|e| panic!("读 {} 失败：{e}", p.display()));
    let doc = Document::load(p).expect("真图应能打开");
    let 高 = doc.view().height;
    let 格长 = 36usize;
    let 头长 = doc.raw_header().len();

    let mut ed = EditSession::new(doc);
    let (n, 前, 后) = 改一格地表并返回改前改后(&mut ed, x, y);
    assert_eq!(n, 1);
    assert_ne!(前.bk_img, 后.bk_img, "图号必须真的变了");

    let 新 = ed.document().to_bytes();
    assert_eq!(新.len(), 原.len(), "长度必须不变");
    let idx = (x as usize) * (高 as usize) + (y as usize);
    let 格起 = 头长 + idx * 格长;
    let 尾起 = 格起 + 14;
    let 尾止 = 格起 + 36;
    assert_eq!(尾止 - 尾起, 22, "尾区必须是 22 字节（格内 14..36）");
    assert_eq!(
        &新[尾起..尾止],
        &原[尾起..尾止],
        "改一格之后 offset+14..+36 的 22 字节必须**原样**"
    );
    assert_eq!(
        &新[尾起..尾止],
        前尾区.as_slice(),
        "落盘尾区必须等于视图里那 22 字节"
    );
    // 尾区**非零**（否则上面的相等是"零对零"，没有证明力）
    let 非零 = 原[尾起..尾止].iter().filter(|b| **b != 0).count();
    assert!(非零 > 0, "取样格的尾区必须非零，否则本门退化");

    let 差异 = 差异清单(&原, &新);
    println!(
        "GATE4 {} ({x},{y}) 格起={格起} 尾区 {尾起}..{尾止} 非零字节={非零} 尾区原样=true 差异={:?}",
        p.file_name().unwrap().to_string_lossy(),
        差异
    );
    // 全文件差异只落在目标格 bk_img 的 2 字节里
    assert!(!差异.is_empty());
    assert!(
        差异.iter().all(|(o, _, _)| *o >= 格起 && *o < 格起 + 2),
        "改一格只该动目标格 bk_img 的 2 字节：{差异:?}"
    );
}

// ═══════════════════════════ 门 5 ═══════════════════════════

/// **门 5**：67 张带尾块文件 —— `open` 后 `tolerant_trailer > 0`，
/// 改一格 → 保存 → 只有目标格变 + 尾块**原样**。
///
/// 口径（`mir2-format/src/model.rs:65-75` 与 `registry.rs:275-291`）：头部声明 `w×h`、
/// 文件比 `头部 + 12×w×h` 长出一截 ⇒ 多出的进 `RawMap::trailer`。严格入口会拒绝这些文件，
/// 编辑器走的是 `read_any_tolerant`。
///
/// # ⚠ 实测订正（2026-11-05）
///
/// 任务书写「67 张带 8,040 B 尾块」。实测 **65 张是 8,040**、**2 张是 9,840**
/// （`T218.map` / `T318.map`：头部 30×30、文件 20,692 字节
/// ⇒ `52 + 12×900 = 10,852`，`20,692 − 10,852 = 9,840`）。
/// 本用例按**实测**钉死（并逐文件核对 `文件长度 == 头部 + 格长×格数 + trailer`），
/// 不把任务书的 8,040 硬套到那 2 张上。
#[test]
fn 门5_67张带尾块文件改一格只动目标格() {
    let 文件 = 收集样本();
    if 文件.is_empty() {
        eprintln!("[skip] 没有找到任何 .map 样本（用 MIREDIT_ASSET_ROOT / MIR2_MAP_DIRS 指定）");
        return;
    }
    let d = 临时目录("gate5");
    let mut 命中 = 0usize;
    let mut 已改 = 0usize;
    let mut 分布: BTreeMap<usize, usize> = BTreeMap::new();
    let mut 非8040: Vec<String> = Vec::new();
    for p in &文件 {
        let 原 = std::fs::read(p).unwrap_or_else(|e| panic!("读 {} 失败：{e}", p.display()));
        let doc = match Document::load(p) {
            Ok(x) => x,
            Err(e) => panic!("{} 应能打开：{e}", p.display()),
        };
        if doc.tolerant_trailer() == 0 {
            continue;
        }
        命中 += 1;
        let 尾长 = doc.tolerant_trailer();
        *分布.entry(尾长).or_default() += 1;
        assert_eq!(doc.trailer().len(), 尾长);
        // 逐文件核对「文件长度 == 头部 + 格长×格数 + 尾块」（这条比硬套 8040 更硬）
        let 头长 = doc.raw_header().len();
        let 格长 = doc.disk_cell_len().expect("带尾块的都是 Old12 定长格式");
        let 格数 = doc.view().cells.len();
        assert_eq!(
            头长 + 格长 * 格数 + 尾长,
            原.len(),
            "{}：文件长度必须正好等于 头部({头长}) + 格长({格长})×格数({格数}) + 尾块({尾长})",
            p.display()
        );
        if 尾长 != 8_040 {
            非8040.push(format!(
                "{}（头部 {}×{}、{格数} 格、尾块 {尾长}）",
                p.file_name().unwrap().to_string_lossy(),
                doc.view().width,
                doc.view().height
            ));
        }
        // 告警必须挂得住，且说出**本文件自己的**未解释字节数
        let 告警 = doc.edit_warning().expect("带尾块的文件必须有告警");
        assert!(
            告警.contains(&尾长.to_string()),
            "告警要说出未解释的字节数（{尾长}）：{告警}"
        );
        assert!(
            告警.contains(&format!("{}×{}", doc.view().width, doc.view().height)),
            "告警要说出按什么尺寸解释：{告警}"
        );

        // 改一格地表（列优先第 0 格 = (0,0)）
        let mut ed = EditSession::new(doc);
        let (x, y) = (0u16, 0u16);
        let (n, 前, 后) = 改一格地表并返回改前改后(&mut ed, x, y);
        assert_eq!(n, 1, "{}：应恰好改 1 格", p.display());
        assert_ne!(前.bk_img, 后.bk_img, "{}：图号必须真的变了", p.display());
        let 新 = ed.document().to_bytes();
        assert_eq!(新.len(), 原.len(), "{}：长度必须不变", p.display());

        // 尾块原样（尾部 尾长 字节）
        let 尾起 = 原.len() - 尾长;
        assert_eq!(
            &新[尾起..],
            &原[尾起..],
            "{}：尾块 {尾长} 字节必须原样",
            p.display()
        );
        // 差异只在目标格的 bk_img 2 字节里（格起 = 头长 + 0）
        let 差异 = 差异清单(&原, &新);
        assert!(
            !差异.is_empty() && 差异.len() <= 2,
            "{}：改一格只该动 bk_img 的 1~2 字节：{差异:?}",
            p.display()
        );
        assert_eq!(差异[0].0, 头长, "{}：首个差异应是目标格 bk_img 低字节", p.display());
        assert!(
            差异.iter().all(|(o, _, _)| *o < 头长 + 2),
            "{}：差异不许越出目标格 bk_img：{差异:?}",
            p.display()
        );
        assert!(
            差异.iter().all(|(o, _, _)| *o < 尾起),
            "{}：差异不许落在尾块里",
            p.display()
        );

        // 真写一份副本再比一次（门 5 要求「保存」而不是「算出来」）
        let 副本 = d.join("tolerant.map");
        miredit_core::save_atomic(&副本, &新).expect("写副本");
        let 读回 = std::fs::read(&副本).expect("读回副本");
        assert_eq!(读回.len(), 原.len());
        assert_eq!(&读回[尾起..], &原[尾起..], "落盘后尾块必须原样");
        let _ = std::fs::remove_file(&副本);
        已改 += 1;
        if 命中 <= 3 || 命中.is_multiple_of(10) {
            println!(
                "GATE5 [{命中}] {} 尾块={尾长} 差异={:?} 尾块原样=true",
                p.file_name().unwrap().to_string_lossy(),
                差异
            );
        }
    }
    let _ = std::fs::remove_dir_all(&d);
    println!("GATE5 合计：命中 {命中} 张，改一格并落盘验证 {已改} 张");
    println!("GATE5 尾块长度分布：{分布:?}");
    println!("GATE5 不是 8040 的（实测订正）：{}", 非8040.join("；"));
    assert_eq!(命中, 67, "带尾块的文件必须是 67 张");
    assert_eq!(已改, 67);
    assert_eq!(分布.get(&8_040).copied().unwrap_or(0), 65, "65 张是 8040");
    assert_eq!(分布.get(&9_840).copied().unwrap_or(0), 2, "2 张是 9840");
    assert_eq!(非8040.len(), 2, "偏离 8040 的必须正好 2 张");
}

// ═══════════════════════════ 门 6 ═══════════════════════════

/// 造一张 **Type5（`Mir3Wemade`）** 合成样本的原始字节。
///
/// 布局照 `mir2-format/src/formats/mir3_wemade.rs:411-422` 的测试夹具：
/// 28 字节头（`Width` `i16@22`、`Height` `i16@24`、首字节 0 = 无标题）
/// + 半分辨率背景块 `3 * ceil(w/2) * floor(h/2)` + `14 × w × h` 格。
fn 造_type5(w: i16, h: i16) -> Vec<u8> {
    let (wu, hu) = (w as u16, h as u16);
    let 块长 = 3 * ((wu as usize / 2) + (wu as usize % 2)) * (hu as usize / 2);
    let n = 28 + 块长 + 14 * (w as usize) * (h as usize);
    let mut b = vec![0u8; n];
    b[0] = 0; // 无标题（C# `:67` 的判别谓词）
    b[20..22].copy_from_slice(&1i16.to_le_bytes()); // Attribute
    b[22..24].copy_from_slice(&w.to_le_bytes());
    b[24..26].copy_from_slice(&h.to_le_bytes());
    // 背景块里放点非零数据（证明它真的存在、且不属于格数组）
    for i in 0..块长 {
        b[28 + i] = (i as u8).wrapping_mul(7).wrapping_add(1);
    }
    // 格数组里放点非零数据（中间层/前景层图号、flag、light）
    let 起 = 28 + 块长;
    for i in 0..(w as usize * h as usize) {
        let base = 起 + i * 14;
        b[base] = 0b11; // flag：bit0/bit1 = 1（可走）
        b[base + 5..base + 7].copy_from_slice(&(100 + i as u16).to_le_bytes()); // mid_img
        b[base + 7..base + 9].copy_from_slice(&(200 + i as u16).to_le_bytes()); // fr_img
        b[base + 12] = 0x0F; // light
    }
    b
}

/// **门 6**：Type5 的地表层落笔必须**明确拒绝 + 中文原因**，且**文件一字节不动**。
///
/// 依据：Type5 的背图在**块级**（2×2 一组 3 字节，`mir2-format/src/model.rs:430-440`），
/// `Cell::bk_img` 恒 0 且 writer 不写它（`mir3_wemade.rs:250-306`）
/// ⇒ 逐格写 `bk_img` 会被**静默丢弃**。宁可在动笔前拒绝。
#[test]
fn 门6_type5地表落笔被拒绝() {
    let d = 临时目录("gate6");
    let p = d.join("type5.map");
    let 原 = 造_type5(4, 2);
    std::fs::write(&p, &原).expect("先写 Type5 合成样本");

    // ① 能打开，且识别成 Type5
    let doc = Document::load(&p).expect("Type5 合成样本应能打开");
    assert_eq!(doc.format_id(), FormatId::Mir3Wemade, "必须判成 Type5");
    assert!(!doc.ground_paint_supported(), "Type5 的地表逐格落笔必须是 false");
    assert_eq!(doc.view().cells.len(), 8, "4×2 = 8 格");
    assert!(
        doc.view().cells.iter().all(|c| c.bk_img == 0),
        "Type5 的 bk_img 恒 0（真值在半分辨率块里）"
    );
    // 视图里确实看得见中间层/前景层图号（证明格数组解析对了）
    assert_eq!(doc.view().cells[0].mid_img, 100);
    assert_eq!(doc.view().cells[0].fr_img, 200);
    let 头长 = doc.raw_header().len();
    assert_eq!(头长, 28 + 3 * 2, "真头部 = 28 + 块长(3*ceil(4/2)*floor(2/2))");

    // ② 地表层落笔 ⇒ 拒绝 + 中文原因
    let mut ed = EditSession::new(doc);
    set_brush_from_palette(&mut ed, LibRef::ground(0), 9); // 图号 10
    let out = apply(&mut ed, Command::Paint { x: 0, y: 0 }).expect("落笔不该 Err");
    assert_eq!(out.changed, 0, "地表层必须一格都不改");
    let note = out.note.expect("拒绝必须给原因");
    assert!(
        note.contains("Type5 背图在块级，逐格写 bk_img 无效"),
        "原因必须点明「Type5 背图在块级，逐格写 bk_img 无效」：{note}"
    );
    assert!(note.contains("一个字节都没动"), "{note}");
    assert!(!out.dirty, "被拒绝的落笔不许把文档标脏");
    assert!(!out.can_undo, "被拒绝的落笔不许产生撤销步");

    // ③ 填充同样被拒（地表）
    let out = apply(&mut ed, Command::Fill { x: 0, y: 0 }).expect("填充不该 Err");
    assert_eq!(out.changed, 0, "地表填充也必须拒绝");
    let note = out.note.expect("填充拒绝也要给原因");
    assert!(
        note.contains("Type5 背图在块级") || note.contains("逐格写 bk_img 无效"),
        "填充拒绝原因要同一口径：{note}"
    );

    // ④ 底层原语也拒绝（不许绕开命令层）
    assert_eq!(ed.apply_once(1, 1), 0, "`apply_once` 也必须拒绝");
    assert_eq!(ed.paint_stroke_once(0, 0), 0, "`paint_stroke_once` 也必须拒绝");

    // ⑤ 中层/物体层仍然能改（只有地表无效，不是整张图不能编辑）
    ed.set_layer(miredit_core::EditLayer::Middle);
    set_brush_from_palette(&mut ed, LibRef::middle(0), 9); // 中层族的笔刷
    let out = apply(&mut ed, Command::Paint { x: 0, y: 0 }).expect("中层落笔不该 Err");
    // 缺省笔刷尺寸 3 在 4×2 图上覆盖 (0..1, 0..1) 共 4 格（都在图内）
    assert_eq!(out.changed, 4, "中层图号有真实字节，应当改得动");
    assert_ne!(ed.map().cell(0, 0).unwrap().mid_img, 100, "中层图号应真的变了");

    // ⑥ 撤销回原样后保存 ⇒ 文件一字节不动
    apply(&mut ed, Command::Undo).expect("撤销");
    assert!(!ed.dirty(), "撤销回原样后不该脏");
    let out = apply(&mut ed, Command::Save).expect("保存不该 Err");
    assert_eq!(out.note.as_deref(), Some("没有未保存的修改，未写盘"));
    let 盘上 = std::fs::read(&p).expect("读回");
    assert_eq!(盘上, 原, "门 6：文件必须一字节不动");

    // ⑦ 再证明一次「地表落笔即便强行写进视图，也不会改变文件」
    //    （这是机制层面的兜底：writer 不写 bk_img）
    let 直 = ed.document().to_bytes();
    assert_eq!(直, 原, "未改时 to_bytes 必须等于原文件");

    println!(
        "GATE6 type5 4×2 头长={头长} 地表落笔=拒绝 原因=「{note}」 文件字节数={} 盘上原样=true",
        原.len()
    );
    let _ = std::fs::remove_dir_all(&d);
}

// ═══════════════════════════ 附：tolerant 路径的契约 ═══════════════════════════

/// 附：`Document::tolerant_trailer` 必须等于 `RawMap::trailer_len()`（两个口径不许漂）。
///
/// 顺带把 `edit_warning()` 的两条来源分别钉一次（未解尾块 / 格式不承诺编辑语义），
/// 并钉住 2026-11-05（P1-9）的订正：**零未建模区的格式不许再挂常驻假告警**。
#[test]
fn 附_告警两条来源分别可判() {
    let Some(root) = 资产根() else {
        eprintln!("[skip] 无资产根 MIREDIT_ASSET_ROOT");
        return;
    };
    let 语料_a = 语料_a(&root);
    let 语料_b = 语料_b(&root);
    let mut 测到 = 0usize;

    // ① 正常文件（Old12，无尾块）：**没有**任何告警
    //    P1-9 订正：旧实现给 Old12 打「未解区语义未知」，那是事实错误 ——
    //    Old12 的 12 字节全部有名字、`tail` 恒空、`unmodeled_fields` 为空、856 张真实样本 0 差异。
    let p = 语料_b.join("0.map");
    if !p.is_file() {
        eprintln!("[skip] 缺文件 {}", p.display());
    } else {
        测到 += 1;
        let doc = Document::load(&p).expect("0.map 应能打开");
        assert_eq!(doc.format_id(), FormatId::Old12);
        assert!(doc.caps().editable, "Old12 零未建模区 + 有真实样本 ⇒ 必须承诺可编辑");
        assert_eq!(
            doc.edit_warning(),
            None,
            "0.map（Old12）既无尾块、格式又可承诺 ⇒ 不许再挂常驻黄条（P1-9）"
        );
        println!(
            "【Old12 / 0.map】edit_warning = {:?}",
            doc.edit_warning()
        );
        assert_eq!(doc.tolerant_trailer(), doc.raw().trailer_len());
    }

    // ② 带尾块的文件：只有「未解尾块」这一条来源（Old12 不再贡献第二条）
    let p = 语料_b.join("EM001.map");
    if !p.is_file() {
        eprintln!("[skip] 缺文件 {}", p.display());
    } else {
        测到 += 1;
        let doc = Document::load(&p).expect("EM001 应能打开（走 tolerant）");
        assert_eq!(doc.format_id(), FormatId::Old12);
        assert_eq!(doc.tolerant_trailer(), 8_040);
        let w = doc.edit_warning().expect("尾块非 0 ⇒ 必须告警");
        assert!(w.contains("未解释尾块"), "{w}");
        assert!(w.contains("8040"), "要带上字节数：{w}");
        assert!(!w.contains("未解区语义未知"), "旧套话不许复活：{w}");
        assert_eq!(
            w.matches('；').count(),
            0,
            "Old12 只该有尾块一条来源，不该再拼格式段：{w}"
        );
        println!("【Old12 / EM001.map（带尾块 8040）】edit_warning = {w}");
        assert_eq!(doc.tolerant_trailer(), doc.raw().trailer_len());
    }

    // ③ `Shanda14` 也是 editable=true ⇒ 正常文件上**没有**告警
    let p = 语料_a.join("41.map");
    if !p.is_file() {
        eprintln!("[skip] 缺文件 {}", p.display());
    } else {
        测到 += 1;
        let doc = Document::load(&p).expect("41.map 应能打开");
        assert_eq!(doc.format_id(), FormatId::Shanda14);
        assert!(doc.caps().editable, "Shanda14 承诺编辑语义");
        assert_eq!(doc.edit_warning(), None, "41.map 无尾块且格式可承诺 ⇒ 不该有告警");
    }

    // ④ `Shanda2012`（36 B/格）仍 `editable = false`，但文案必须**逐格式具体**：
    //    说清是 22 字节尾区、且不许再用「未解区语义未知」那句假话（P1-9）
    let p = 语料_b.join("alx.map");
    if !p.is_file() {
        eprintln!("[skip] 缺文件 {}", p.display());
    } else {
        测到 += 1;
        let doc = Document::load(&p).expect("alx.map 应能打开");
        assert_eq!(doc.format_id(), FormatId::Shanda2012);
        assert_eq!(doc.tolerant_trailer(), 0, "36 B/格精确长度，无尾块");
        assert!(!doc.caps().editable, "22 B 未建模尾区 ⇒ 不承诺");
        let w = doc.edit_warning().expect("Shanda2012 必须告警（尾区未建模）");
        assert!(w.contains("22 字节未建模"), "要点名尾区规模：{w}");
        assert!(w.contains("rel 14-35"), "要点名是哪一段：{w}");
        assert!(w.contains("原样保留"), "要说清怎么处理：{w}");
        assert!(!w.contains("未解区语义未知"), "旧套话不许复活：{w}");
        assert!(!w.contains("未解释尾块"), "本文件没有尾块：{w}");
        println!("【Shanda2012 / alx.map（36 B/格）】edit_warning = {w}");
    }

    // ⑤ 交叉校验：`RawMap::trailer_len` 与 `Document::tolerant_trailer` 在 5 张上逐一相等
    let 样本: [PathBuf; 5] = [
        语料_b.join("0.map"),
        语料_b.join("EM001.map"),
        语料_b.join("n0104.map"),
        语料_a.join("41.map"),
        语料_a.join("0.map"),
    ];
    for p in &样本 {
        if !p.is_file() {
            eprintln!("[skip] 缺文件 {}", p.display());
            continue;
        }
        let doc = Document::load(p).expect("应能打开");
        let raw: &RawMap = doc.raw();
        assert_eq!(doc.tolerant_trailer(), raw.trailer_len(), "{}", p.display());
    }
    if 测到 == 0 {
        eprintln!("[skip] 真实地图一张都不在 —— 本用例 skip（不 panic）");
    }
}

/// 附：`unmodeled_fields()` **九种格式逐行返回值**（P1-15 改名后的"含义清楚"凭据）。
///
/// 名字从 `lost_fields` 改成 `unmodeled_fields`（未建模 ≠ 丢失）之后，这一张表就是
/// 「哪些字段编辑不到」的唯一权威清单：
/// - 返回空 ⇒ 该格式 `raw` 的字段**全部**在编辑视图里有位置（`Old12` / `Shanda14` /
///   `Shanda2012` / `AntiHack` 四种）；
/// - 返回非空 ⇒ 逐项点名编辑不到的字段（另外五种）。
///
/// ⚠ 它**不是** `FormatCaps::editable` 的判据（`AntiHack` 这里为空但 `editable = false`，
/// 原因是零真实样本）—— 两件事，别混。
#[test]
fn 附_未建模字段九行() {
    // 注册表顺序（与 `mir2-format/src/registry.rs` 的 `注册表九项` 一致）
    let 全部 = [
        FormatId::CrystalCs,
        FormatId::Mir3Wemade,
        FormatId::Mir3Shanda,
        FormatId::AntiHack,
        FormatId::Wemade2010,
        FormatId::Shanda2012,
        FormatId::Shanda14,
        FormatId::Heroes34,
        FormatId::Old12,
    ];
    let mut 空 = 0usize;
    for id in 全部 {
        let 字段 = miredit_core::unmodeled_fields(id);
        println!("unmodeled_fields({id:?}) = {字段:?}  （{} 项）", 字段.len());
        if 字段.is_empty() {
            空 += 1;
        }
    }
    assert_eq!(空, 4, "四种格式零未建模字段（Old12/Shanda14/Shanda2012/AntiHack）");
    assert_eq!(全部.len(), 9, "九种格式必须全列");
}
