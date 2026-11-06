//! `maplist` 模块的验收用例：自然序 / 大小与时间格式化 / 只读元数据 / 分帧 / 面板几何。
//!
//! # 这一组用例在防什么
//!
//! | 用例 | 挡的错 |
//! |---|---|
//! | [`自然序`] / [`自然序与字典序确实不同`] | 排序退回字典序（`n10` 跑到 `n2` 前面）|
//! | [`大小格式化`] / [`时间格式化`] | 单位/历法算错，显示一个"像样但假的"数字 |
//! | [`扫描只读元数据_坏文件也照样进列表`] | 有人"顺手"在读地图头判别格式（坏文件会被跳过）|
//! | [`源码里没有任何读内容的调用`] | 静态闸门：`maplist.rs` 里出现 `fs::read(` / `File::open(` 等 |
//! | [`实测扫描耗时与只读元数据对照`] | 性能口径：扫 1,097 张 vs 读一张 5.88 MB |
//! | [`分帧扫描结果与一次扫完相同`] | 分帧把条目搞丢/搞重 |
//! | [`空目录与扫描失败都给中文原因`] | 空/失败静默 |
//! | [`当前项下标与滚动跟随`] | 高亮错行、滚到列表外 |
//! | [`面板几何`] | 行/状态行越出面板（遮挡/错位）|

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, UNIX_EPOCH};

use egui::Rect;

use crate::layout::Layout;
use crate::maplist::{self, MapListPanel, Phase};
use crate::metrics as m;

// ───────────────────────── 夹具 ─────────────────────────

/// 用例专用临时目录（每次先清空；与 `acceptance_tests::common::临时目录` 同一套命名习惯）。
fn 临时目录(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("miredit_gui_maplist_{tag}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("建临时目录");
    d
}

/// 写一张 `w×w` 的 12B/格合成地图（只写头 + 全零格，够 `Document::load` 认）。
fn 写图(dir: &Path, name: &str, w: u16) -> PathBuf {
    let mut b = vec![0u8; 52 + 12 * (w as usize * w as usize)];
    b[0..2].copy_from_slice(&w.to_le_bytes());
    b[2..4].copy_from_slice(&w.to_le_bytes());
    let p = dir.join(name);
    std::fs::write(&p, &b).expect("写合成地图");
    p
}

/// 资产根缺省值：本机真实根（**整个文件里只出现这一处**）。换机器用 `MIREDIT_ASSET_ROOT`。
const 缺省资产根: &str = r"E:\GodM2";

/// 资产根：`MIREDIT_ASSET_ROOT` → 缺省 [`缺省资产根`]。
///
/// 根目录不存在（公开仓/PR 的机器上没有游戏素材）⇒ `None`，调用方打印 `[跳过]` 并返回。
/// 只做 `var_os` + `PathBuf`，不引入任何第三方库。
fn 资产根() -> Option<PathBuf> {
    let root = std::env::var_os("MIREDIT_ASSET_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(缺省资产根));
    root.is_dir().then_some(root)
}

/// 本仓两处真实语料（与 `acceptance_tests::formats` 的 `语料_a` / `语料_b` 同一份）。
fn 语料_a() -> Option<PathBuf> {
    Some(资产根()?.join("GodotClient").join("maps"))
}
fn 语料_b() -> Option<PathBuf> {
    Some(资产根()?.join("client").join("热血传奇23周年客户端").join("Map"))
}

/// 窗口矩形（1500×950，与截图口径一致）。
fn 窗() -> Rect {
    Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(m::WINDOW_W, m::WINDOW_H))
}

// ───────────────────────── 自然序 ─────────────────────────

/// 自然序：数字段按**数值**比，其余按小写比。
#[test]
fn 自然序() {
    use std::cmp::Ordering;
    // 核心判据：n2 在 n10 之前
    assert_eq!(maplist::natural_cmp("n2.map", "n10.map"), Ordering::Less);
    assert_eq!(maplist::natural_cmp("n10.map", "n2.map"), Ordering::Greater);
    // 相等（数值相等 ⇒ 落原始字节）
    assert_eq!(maplist::natural_cmp("n2.map", "n2.map"), Ordering::Equal);
    // 前导零：`n007` 与 `n7` 数值相等，原始字节兜底 ⇒ `n007` 在前（`0` < `7`）
    assert_eq!(maplist::natural_cmp("n007.map", "n7.map"), Ordering::Less);
    // 大小写不敏感：`A.map` 与 `a.map` 只按原始字节分先后，不会散到两端
    assert_eq!(maplist::natural_cmp("A.map", "a.map"), Ordering::Less);
    // 前缀：短的在前
    assert_eq!(maplist::natural_cmp("n1.map", "n1.map.bak"), Ordering::Less);
    // 任意长度的数字段（不溢出）
    let big = "n99999999999999999999999999999999999999.map";
    let bigger = "n999999999999999999999999999999999999990.map";
    assert_eq!(maplist::natural_cmp(big, bigger), Ordering::Less);
}

/// **字典序会把 `n10` 排在 `n2` 前面** —— 这条用例把这个差异钉死（证明改动有效）。
#[test]
fn 自然序与字典序确实不同() {
    let mut v = vec!["n10.map", "n2.map", "n1.map", "n20.map", "n3.map"];
    let mut lex = v.clone();
    lex.sort();
    v.sort_by(|a, b| maplist::natural_cmp(a, b));
    assert_eq!(v, vec!["n1.map", "n2.map", "n3.map", "n10.map", "n20.map"]);
    assert_eq!(lex, vec!["n1.map", "n10.map", "n2.map", "n20.map", "n3.map"]);
    assert_ne!(v, lex, "自然序必须与字典序不同（否则排序没生效）");
}

/// 排序结果**可复现**（同一批输入两次排出来一模一样，不依赖输入顺序）。
#[test]
fn 排序可复现() {
    let a = vec!["n10.map", "N2.map", "n2.map", "n02.map", "n1.map"];
    let mut x = a.clone();
    let mut y = a.clone();
    x.sort_by(|p, q| maplist::natural_cmp(p, q));
    y.reverse();
    y.sort_by(|p, q| maplist::natural_cmp(p, q));
    assert_eq!(x, y, "输入顺序不同但结果必须一致：{x:?} / {y:?}");
}

// ───────────────────────── 格式化 ─────────────────────────

/// 大小格式化：1024 进制、单位来自 `ui_text`。
#[test]
fn 大小格式化() {
    assert_eq!(maplist::format_size(0), "0 B");
    assert_eq!(maplist::format_size(1023), "1023 B");
    assert_eq!(maplist::format_size(1024), "1.0 KB");
    assert_eq!(maplist::format_size(1536), "1.5 KB");
    assert_eq!(maplist::format_size(1024 * 1024), "1.0 MB");
    assert_eq!(maplist::format_size(697 * 1024 * 1024), "697.0 MB");
    assert_eq!(maplist::format_size(3 * 1024 * 1024 * 1024), "3.00 GB");
}

/// 时间格式化：UTC 历法换算（`civil_from_days` 的逆向）。
#[test]
fn 时间格式化() {
    // 1970-01-01 00:00
    assert_eq!(
        maplist::format_mtime(UNIX_EPOCH).as_deref(),
        Some("1970-01-01 00:00")
    );
    // 2026-11-05 10:41:17 UTC（本单开工时刻）
    let t = UNIX_EPOCH + Duration::from_secs(1_793_875_277);
    assert_eq!(maplist::format_mtime(t).as_deref(), Some("2026-11-05 10:41"));
    // 闰日：2024-02-29
    let leap = UNIX_EPOCH + Duration::from_secs(1_709_164_800); // 2024-02-29 00:00 UTC
    assert_eq!(maplist::format_mtime(leap).as_deref(), Some("2024-02-29 00:00"));
}

// ───────────────────────── 只读元数据 ─────────────────────────

/// **坏文件也照样进列表** —— 这是"扫描不解析地图"的行为级证据。
///
/// 一张 12 字节的垃圾文件不是合法地图（`Document::load` 必失败），但列表里
/// 照样有它、字节数照样是 12。如果扫描阶段解析过地图内容，它会被跳过或报错。
#[test]
fn 扫描只读元数据_坏文件也照样进列表() {
    let dir = 临时目录("metadata_only");
    // ① 一张合法合成图
    let good = 写图(&dir, "n2.map", 4);
    // ② 一张"绝不是地图"的垃圾文件（12 字节随机内容）
    let bad = dir.join("n10.map");
    std::fs::write(&bad, b"not-a-map!!!").expect("写垃圾文件");
    // ③ 非 .map 文件（不该进列表）
    std::fs::write(dir.join("readme.txt"), b"hello").expect("写非地图文件");
    // ④ 子目录里的图（递归枚举：上游 `RecurseSubdirectories = true`）
    let sub = dir.join("sub");
    std::fs::create_dir_all(&sub).expect("建子目录");
    let deep = 写图(&sub, "n3.map", 4);

    let (entries, stats, errors) = maplist::scan_all(&dir).expect("扫描应成功");
    assert!(errors.is_empty(), "不该有错误：{errors:?}");
    let names: Vec<&str> = entries.iter().map(|e| e.rel.as_str()).collect();
    // 自然序按**相对路径**排：`n2.map` < `n10.map` < `sub\n3.map`
    // （`sub` 的首字符 's' > 'n'，所以子目录里的图排在两张根目录图之后）
    assert_eq!(names, vec!["n2.map", "n10.map", "sub\\n3.map"], "实际：{names:?}");
    assert_eq!(stats.entries, 3);
    // 垃圾文件也在，字节数就是它真实的 12
    let bad_entry = entries.iter().find(|e| e.rel == "n10.map").expect("垃圾文件要在列表里");
    assert_eq!(bad_entry.bytes, 12, "只读了长度，没有解析内容");
    assert_eq!(bad_entry.path, bad);
    // 合法图的字节数 = 52 + 12×16
    let good_entry = entries.iter().find(|e| e.rel == "n2.map").expect("合法图");
    assert_eq!(good_entry.bytes, 52 + 12 * 16);
    assert_eq!(good_entry.path, good);
    // 子目录的条目 rel 带目录前缀
    assert_eq!(
        entries.iter().find(|e| e.rel.contains("n3.map")).map(|e| e.path.clone()),
        Some(deep)
    );
    // mtime 是真实值（刚写的文件必然有）
    assert!(good_entry.mtime.is_some(), "真实文件的修改时间不该是 None");
    // 统计口径：走过 2 个目录（根 + sub），看到 5 个条目（根 4 项 + sub 1 项；
    // 目录本身也算"看到的条目"——`seen` 统计的是 `read_dir` 给出的项数）
    assert_eq!(stats.dirs, 2, "根 + sub");
    assert_eq!(stats.seen, 5, "根 4 项（n2/n10/readme/sub）+ sub 1 项");
    let _ = std::fs::remove_dir_all(&dir);
}

/// **静态闸门**：`maplist.rs` 里不许出现任何"读文件内容"的调用。
///
/// 用 `include_str!` 把源码本身编进来查 —— 这条比"人工 grep"硬：谁以后顺手加了
/// 一行 `fs::read(path)`，编译后跑测试就红。
#[test]
fn 源码里没有任何读内容的调用() {
    const SRC: &str = include_str!("maplist.rs");
    for bad in [
        "fs::read(",
        "fs::read_to_string(",
        "File::open(",
        "Document::load(",
        "Map::parse(",
        "read_any_tolerant(",
    ] {
        assert!(
            !SRC.contains(bad),
            "`maplist.rs` 里出现了 {bad} —— 列表阶段只许读元数据，不许读内容"
        );
    }
    // 正面证据：只该出现这两个 IO 入口
    assert!(SRC.contains("fs::read_dir("), "扫描要靠 read_dir");
    assert!(SRC.contains("ent.metadata()"), "元数据要靠 DirEntry::metadata");
}

// ───────────────────────── 分帧 ─────────────────────────

/// 分帧扫描的结果必须与"一次扫完"**逐条相同**（分帧只切时间，不切语义）。
#[test]
fn 分帧扫描结果与一次扫完相同() {
    let dir = 临时目录("chunked");
    for i in 0..30u16 {
        写图(&dir, &format!("n{i}.map"), 3);
    }
    let (all, _, _) = maplist::scan_all(&dir).expect("一次扫完");
    let mut sc = maplist::Scanner::new(&dir).expect("开扫描");
    let mut steps = 0;
    // 极小预算 + 极小批量 ⇒ 必定多帧
    while sc.step(Duration::from_millis(0), 4) {
        steps += 1;
        assert!(steps < 1000, "分帧没收敛");
    }
    let (chunked, stats, _) = sc.into_parts();
    assert!(steps > 1, "30 张 + 4 项/步 应该要多步，实际 {steps}");
    assert_eq!(chunked, all, "分帧与一次扫完的条目必须逐条相同");
    assert_eq!(stats.entries, 30);
    assert!(stats.steps > 1, "步数要记进统计：{}", stats.steps);
    let _ = std::fs::remove_dir_all(&dir);
}

/// **每帧预算生效**：`step` 处理满 `max_items` 就返回（不会一口气扫完）。
#[test]
fn 单帧处理量受上限约束() {
    let dir = 临时目录("budget");
    for i in 0..20u16 {
        写图(&dir, &format!("n{i}.map"), 3);
    }
    let mut sc = maplist::Scanner::new(&dir).expect("开扫描");
    // 预算给足（1 秒），只靠批量上限卡：一次最多 5 项
    let more = sc.step(Duration::from_secs(1), 5);
    assert!(more, "20 个条目 + 5 项/步 ⇒ 还没扫完");
    assert!(sc.found() <= 5, "单帧最多处理 5 项，实际 {}", sc.found());
    let _ = std::fs::remove_dir_all(&dir);
}

// ───────────────────────── 空目录 / 失败 ─────────────────────────

/// 空目录 ⇒ `Ready` + 0 条 + **中文原因**；根不存在 ⇒ `Failed` + 中文原因。
#[test]
fn 空目录与扫描失败都给中文原因() {
    // ① 空目录
    let dir = 临时目录("empty");
    let mut p = MapListPanel::new();
    p.start(&dir);
    while p.poll() {}
    assert_eq!(p.phase(), &Phase::Ready);
    assert!(p.is_empty(), "空目录必须一条都没有");
    let s = p.status_text();
    assert!(s.contains("没有 .map 文件"), "空目录要给中文原因：{s}");
    assert!(s.contains(&dir.display().to_string()), "原因里要有目录：{s}");

    // ② 根不存在
    let missing = dir.join("no_such_dir");
    let mut q = MapListPanel::new();
    q.start(&missing);
    assert!(!q.poll(), "根不可用时不该有扫描器在跑");
    match q.phase() {
        Phase::Failed(why) => {
            assert!(why.contains("读不了地图目录"), "失败原因要中文：{why}");
            assert!(why.contains(&missing.display().to_string()), "要带路径：{why}");
        }
        other => panic!("根不存在必须是 Failed，实际 {other:?}"),
    }
    let s = q.status_text();
    assert!(s.contains("扫描失败"), "状态行要说清失败：{s}");
    assert!(!s.contains("{why}"), "模板占位符没被填：{s}");

    // ③ 根是文件而不是目录
    let file = 写图(&dir, "a.map", 3);
    let mut r = MapListPanel::new();
    r.start(&file);
    match r.phase() {
        Phase::Failed(why) => assert!(why.contains("不是目录"), "要说是「不是目录」：{why}"),
        other => panic!("根是文件必须是 Failed，实际 {other:?}"),
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// ───────────────────────── 当前项 / 滚动 ─────────────────────────

/// 当前项下标（大小写不敏感）+ 滚动跟随 + 滚动夹取。
#[test]
fn 当前项下标与滚动跟随() {
    let dir = 临时目录("follow");
    for i in 0..50u16 {
        写图(&dir, &format!("n{i}.map"), 3);
    }
    let mut p = MapListPanel::new();
    p.start(&dir);
    while p.poll() {}
    assert_eq!(p.len(), 50);

    // ① 下标：按路径找（大小写不敏感，Windows 语义）
    let target = p.entries()[37].path.clone();
    assert_eq!(p.index_of(Some(&target)), Some(37));
    let upper = PathBuf::from(target.to_string_lossy().to_uppercase());
    assert_eq!(p.index_of(Some(&upper)), Some(37), "路径比较必须大小写不敏感");
    assert_eq!(p.index_of(None), None);
    assert_eq!(p.index_of(Some(Path::new(r"E:\nope\x.map"))), None);

    // ② 跟随：当前项在视野外 ⇒ 滚进来
    let rows = 10;
    p.scroll_to(37, rows);
    assert_eq!(p.top(), 37 + 1 - rows, "滚到「最后一行正好是 37」");
    assert!(
        p.top() <= 37 && 37 < p.top() + rows,
        "第 37 行必须在 [{}, {}) 里",
        p.top(),
        p.top() + rows
    );

    // ③ 夹取：不许滚过末尾留白
    p.scroll_rows(1000, rows);
    assert_eq!(p.top(), 50 - rows);
    p.scroll_rows(-1000, rows);
    assert_eq!(p.top(), 0);

    // ④ 可见行数为 0 时不动（面板被折叠/高度不足）
    p.scroll_to(40, 0);
    assert_eq!(p.top(), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

// ───────────────────────── 面板几何 ─────────────────────────

/// 面板几何：工具条 / 列表 / 状态行首尾相接、行不越界、能放下足够多行。
#[test]
fn 面板几何() {
    let l = Layout::compute_with(窗(), true);
    let c = l.p_maps.content;
    let tb = maplist::toolbar_rect(&l);
    let list = maplist::list_rect(&l);
    let st = maplist::status_rect(&l);
    assert!((tb.top() - c.top()).abs() < 0.01);
    assert!((tb.bottom() - list.top()).abs() < 0.01, "工具条与列表必须首尾相接");
    assert!((list.bottom() - st.top()).abs() < 0.01, "列表与状态行必须首尾相接");
    assert!((st.bottom() - c.bottom()).abs() < 0.01, "状态行必须贴面板底");
    // 刷新按钮在工具条里
    let btn = maplist::refresh_rect(&l);
    assert!(tb.contains_rect(btn), "刷新按钮越出工具条：{btn:?}");
    // 可见行数：面板内容高 ≈ 815 - 26 - 18 = 771 ⇒ 35 行
    let rows = maplist::visible_rows(&l);
    assert!(rows >= 20, "1500×950 下至少要能看 20 行，实际 {rows}");
    // 每一行都在列表区里、且行高 22
    for i in [0usize, 1, rows - 1] {
        let r = maplist::row_rect(&l, i);
        assert!((r.height() - m::MAPS_ROW_H).abs() < 0.01);
        assert!(list.contains_rect(r), "第 {i} 行越出列表区：{r:?} vs {list:?}");
    }
    // 滚动条在列表右侧、不与行重叠
    let sb = maplist::scrollbar_rect(&l);
    assert!(list.contains_rect(sb));
    assert!(
        maplist::row_rect(&l, 0).right() <= sb.left() + 0.01,
        "行与滚动条重叠"
    );
    // 面板关着时这些矩形全都塌成零宽（画不出东西，也点不到）
    let shut = Layout::compute_with(窗(), false);
    assert!(maplist::list_rect(&shut).width().abs() < 0.01);
    assert_eq!(maplist::visible_rows(&shut), 0);
}

// ───────────────────────── 实测耗时（验收证据） ─────────────────────────

/// **实测**：两个真实语料目录的扫描耗时，以及"读一张图的内容"作对照。
///
/// 打印到 stderr（`cargo test -- --nocapture` 可见），断言只钉弱不变量
/// （扫 1,097 张必须 < 1 s —— 若真读了 697 MB 内容，本机也要 1 s 以上）。
#[test]
fn 实测扫描耗时与只读元数据对照() {
    // ① 语料 A：61 张 / 35.8 MB
    // ② 语料 B：1,097 张 / 697.2 MB
    for (tag, dir) in [("A", 语料_a()), ("B", 语料_b())] {
        let Some(dir) = dir else {
            eprintln!("[跳过] 语料 {tag}：无资产根 MIREDIT_ASSET_ROOT");
            continue;
        };
        if !dir.is_dir() {
            eprintln!("[跳过] 语料 {tag} 不存在：{}", dir.display());
            continue;
        }
        // 三轮取中位数（第一轮含目录项缓存预热，后面几轮更快）
        let mut times = Vec::new();
        let mut n = 0usize;
        let mut bytes = 0u64;
        for _ in 0..3 {
            let t0 = Instant::now();
            let (entries, stats, errors) = maplist::scan_all(&dir).expect("扫描");
            let ms = t0.elapsed().as_secs_f64() * 1000.0;
            assert!(errors.is_empty(), "真实语料不该有读不了的文件：{errors:?}");
            n = stats.entries;
            bytes = entries.iter().map(|e| e.bytes).sum();
            assert_eq!(n, entries.len());
            times.push(ms);
        }
        times.sort_by(|a, b| a.total_cmp(b));
        eprintln!(
            "[地图列表] 语料{tag}：{n} 张 / {:.1} MB ⇒ 扫描 {:.2} ms（三轮 {:.2} / {:.2} / {:.2}）\
             ⇒ {:.1} 张/ms",
            bytes as f64 / (1024.0 * 1024.0),
            times[1],
            times[0],
            times[1],
            times[2],
            n as f64 / times[1].max(0.001)
        );
        assert!(
            times[1] < 1000.0,
            "扫 {n} 张用了 {:.1} ms —— 若只读元数据不该这么慢（读了内容？）",
            times[1]
        );
        if tag == "B" {
            assert_eq!(n, 1097, "23 周年客户端 Map 目录应有 1,097 张 .map");
        }
    }

    // ③ 对照：**读一张图的内容**要多久（同一台机、同一块盘）
    let big = 语料_a().map(|d| d.join("0.map")).unwrap_or_default();
    if big.is_file() {
        let len = std::fs::metadata(&big).map(|m| m.len()).unwrap_or(0);
        let t0 = Instant::now();
        let data = std::fs::read(&big).expect("读 0.map");
        let read_ms = t0.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(data.len() as u64, len);
        eprintln!(
            "[地图列表] 对照：读一张 0.map（{:.2} MB，内容全读）{:.2} ms ⇒ \
             1097 张若也读内容 ≈ {:.0} ms",
            len as f64 / (1024.0 * 1024.0),
            read_ms,
            read_ms * 1097.0
        );
        assert!(read_ms > 0.0, "读 5.88 MB 不可能 0 ms");
    } else {
        eprintln!("[跳过] 没有对照样本 {}", big.display());
    }
}
