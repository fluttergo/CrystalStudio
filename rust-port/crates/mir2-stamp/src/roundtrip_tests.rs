//! 全量往返门（数据驱动，跑真实印章资源）+ 负向用例 + 字段级转储。
//!
//! 用法：`cargo test -p mir2-stamp -- --nocapture`（加 `--nocapture` 才能看到统计行）。
//!
//! 纪律（照抄 `mir2-map` 的同款门禁风格）：
//! - **目录不存在 ⇒ 打印警告并 skip，不 panic**（环境缺资源不算代码错）；
//! - 跳过必须**打印原因**，不静默；
//! - 任何真实字节不一致 ⇒ 断言失败并给出**文件名 + 首个不一致偏移 + 前后各 8 字节**；
//! - 负向用例**必须** `Err`，`Ok` 即判失败。

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::cell::{Front, StampCellData, HEADER_LEN};
use crate::error::StampError;
use crate::stamp::{trailing_bytes, Stamp, StampCell};

/// 取证根缺省值：`.X` 的取证来源（**本文件里只出现这一处**）。换机器用 `MIREDIT_UPSTREAM_ROOT`。
const 缺省取证根: &str = r"I:\miredit\_upstream";

/// 往返统计。
#[derive(Default)]
struct Stats {
    /// 扫描到的 `.X` 文件数。
    scanned: usize,
    /// 往返成功数。
    success: usize,
    /// 往返失败数。
    failed: usize,
    /// 跳过数（读取失败等）。
    skipped: usize,
    /// 跳过原因 → 次数。
    skip_reasons: BTreeMap<String, usize>,
    /// 跳过样例（最多 5 条）。
    skip_samples: Vec<String>,
    /// 失败详情（最多 20 条）。
    failures: Vec<String>,
    /// 累计格数。
    cells: usize,
    /// 有前景的格数。
    cells_with_front: usize,
}

impl Stats {
    fn fail(&mut self, msg: String) {
        self.failed += 1;
        if self.failures.len() < 20 {
            self.failures.push(msg);
        }
    }

    fn skip(&mut self, reason: &str, detail: String) {
        self.skipped += 1;
        *self.skip_reasons.entry(reason.to_string()).or_insert(0) += 1;
        if self.skip_samples.len() < 5 {
            self.skip_samples.push(detail);
        }
    }

    fn 打印跳过摘要(&self) {
        if self.skipped == 0 {
            return;
        }
        println!("[跳过摘要] 共 {} 个：", self.skipped);
        for (reason, n) in &self.skip_reasons {
            println!("  - {reason} × {n}");
        }
        for s in &self.skip_samples {
            println!("  例：{s}");
        }
    }

    fn 断言全绿(&self) {
        assert!(
            self.failed == 0,
            "有 {} 个文件往返不一致（前 {} 条）：\n{}",
            self.failed,
            self.failures.len(),
            self.failures.join("\n")
        );
        assert!(self.success > 0, "一个文件都没跑通：检查资源目录是否为空");
    }
}

/// 取证根：`MIREDIT_UPSTREAM_ROOT` → 缺省 [`缺省取证根`]；根不存在 ⇒ `None`。
///
/// 只做 `var_os` + `PathBuf`，不引入任何第三方库。
fn 取证根() -> Option<PathBuf> {
    let root = std::env::var_os("MIREDIT_UPSTREAM_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(缺省取证根));
    root.is_dir().then_some(root)
}

/// 印章根目录（`MIR2_STAMP_ROOT` 优先；否则 `<取证根>/Objects`）。缺取证根 ⇒ `None`。
fn 印章根目录() -> Option<PathBuf> {
    match std::env::var_os("MIR2_STAMP_ROOT") {
        Some(v) => Some(PathBuf::from(v)),
        None => Some(取证根()?.join("Objects")),
    }
}

/// 递归列出目录下所有 `.X`（扩展名大小写不敏感），按路径排序保证输出稳定。
///
/// 目录不存在 / 不可读 ⇒ 打印警告并返回 `None`（调用方 skip，不 panic）。
fn 列出印章(dir: &Path) -> Option<Vec<PathBuf>> {
    if !dir.exists() {
        eprintln!(
            "[跳过] 目录不存在：{} —— 本用例 skip（不 panic）",
            dir.display()
        );
        return None;
    }
    let mut out = Vec::new();
    let mut 栈 = vec![dir.to_path_buf()];
    while let Some(当前) = 栈.pop() {
        let rd = match fs::read_dir(&当前) {
            Ok(rd) => rd,
            Err(e) => {
                eprintln!("[警告] 目录无法读取：{}（{e}）", 当前.display());
                continue;
            }
        };
        for entry in rd {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    eprintln!("[警告] 目录项读取失败：{e}");
                    continue;
                }
            };
            let path = entry.path();
            let 是印章 = path
                .extension()
                .map(|e| e.eq_ignore_ascii_case("X"))
                .unwrap_or(false);
            match entry.file_type() {
                Ok(ft) if ft.is_dir() => 栈.push(path),
                Ok(ft) if ft.is_file() && 是印章 => out.push(path),
                Ok(_) => {}
                Err(e) => eprintln!("[警告] 无法取文件类型 {}：{e}", path.display()),
            }
        }
    }
    out.sort();
    Some(out)
}

/// 相对根目录的展示名（正斜杠，跨平台/跨脚本一致）。
fn 展示名(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// 首个不一致偏移；完全相同返回 `None`。
fn 首个差异(a: &[u8], b: &[u8]) -> Option<usize> {
    if let Some(i) = a.iter().zip(b.iter()).position(|(x, y)| x != y) {
        return Some(i);
    }
    if a.len() == b.len() {
        None
    } else {
        Some(a.len().min(b.len()))
    }
}

/// 大写十六进制串，空格分隔。
fn 十六进制(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 3);
    for b in bytes {
        s.push_str(&format!("{b:02X} "));
    }
    s.trim_end().to_string()
}

/// 打印一条不一致详情：文件名 + 首个不一致偏移 + 前后各 8 字节。
fn 打印差异(label: &str, original: &[u8], regenerated: &[u8]) -> String {
    let off = 首个差异(original, regenerated).unwrap_or(0);
    let start = off.saturating_sub(8);
    let end_o = (off + 8).min(original.len());
    let end_r = (off + 8).min(regenerated.len());
    format!(
        "{label}：长度 原={} 新={}，首个不一致偏移={off}\n        原[{start}..{end_o}] = {}\n        新[{start}..{end_r}] = {}",
        original.len(),
        regenerated.len(),
        十六进制(&original[start..end_o]),
        十六进制(&regenerated[start..end_r]),
    )
}

/// 门禁 1：**全量往返**。逐个 `parse → to_bytes`，断言与原文件逐字节相等。
#[test]
fn 全量往返_全部印章逐字节相等() {
    let Some(root) = 印章根目录() else {
        println!("[往返] 无取证根 MIREDIT_UPSTREAM_ROOT ⇒ 本用例 skip（0 成功 / 0 失败）");
        return;
    };
    println!("[往返] 根目录 = {}", root.display());
    let Some(files) = 列出印章(&root) else {
        println!("[往返] 目录不存在 ⇒ 本用例 skip（0 成功 / 0 失败）");
        return;
    };
    println!("[往返] 扫描到 .X 文件 {} 个", files.len());
    let mut stats = Stats::default();
    for path in &files {
        stats.scanned += 1;
        let label = 展示名(&root, path);
        let original = match fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                stats.skip("文件读取失败", format!("{label}：{e}"));
                continue;
            }
        };
        let stamp = match Stamp::parse(&original) {
            Ok(s) => s,
            Err(e) => {
                stats.fail(format!("{label}：parse 失败 —— {e}"));
                continue;
            }
        };
        // 长度自洽：7 + Σ单格字节 必须精确等于文件长度（尾部不允许有多余字节）
        let expect_len = stamp.byte_len();
        if expect_len != original.len() {
            stats.fail(format!(
                "{label}：长度不自洽 —— 按格算出 {expect_len} 字节，实际 {} 字节",
                original.len()
            ));
            continue;
        }
        let regenerated = stamp.to_bytes();
        if 首个差异(&original, &regenerated).is_some() {
            stats.fail(打印差异(&label, &original, &regenerated));
            continue;
        }
        stats.cells += stamp.cells.len();
        stats.cells_with_front += stamp
            .cells
            .iter()
            .filter(|c| c.cell.foreground.is_some())
            .count();
        stats.success += 1;
    }
    println!(
        "[往返] 成功 = {} / 失败 = {} / 跳过 = {}（共 {} 个文件）",
        stats.success, stats.failed, stats.skipped, stats.scanned
    );
    println!(
        "[往返] 累计格数 = {}（有前景 {} / 无前景 {}）",
        stats.cells,
        stats.cells_with_front,
        stats.cells - stats.cells_with_front
    );
    stats.打印跳过摘要();
    stats.断言全绿();
}

/// 门禁 2：**版本分布**。只读文件头 3 字节（按 C# `:107` 的探测规则分类）。
#[test]
fn 版本分布统计() {
    let Some(root) = 印章根目录() else {
        println!("[版本] 无取证根 MIREDIT_UPSTREAM_ROOT ⇒ 本用例 skip");
        return;
    };
    let Some(files) = 列出印章(&root) else {
        println!("[版本] 目录不存在 ⇒ 本用例 skip");
        return;
    };
    let mut 计数: BTreeMap<String, usize> = BTreeMap::new();
    let mut 读失败 = 0usize;
    let mut 样例: BTreeMap<String, String> = BTreeMap::new();
    for path in &files {
        let mut buf = [0u8; 3];
        let mut n = 0usize;
        match fs::File::open(path) {
            Ok(mut f) => {
                while n < 3 {
                    match f.read(&mut buf[n..]) {
                        Ok(0) => break,
                        Ok(k) => n += k,
                        Err(_) => break,
                    }
                }
            }
            Err(_) => {
                读失败 += 1;
                continue;
            }
        }
        let 版本 = if n >= 3 && buf[0] == b'C' && buf[1] == b'#' {
            match buf[2] {
                0..=2 => format!("v{}", buf[2]),
                other => format!("未知({other})"),
            }
        } else if n >= 3 {
            "v0(无文件头/裸流)".to_string()
        } else {
            "文件过短".to_string()
        };
        *计数.entry(版本.clone()).or_insert(0) += 1;
        样例.entry(版本).or_insert_with(|| 展示名(&root, path));
    }
    println!("[版本] 共 {} 个 .X（读取失败 {} 个）", files.len(), 读失败);
    for (版本, n) in &计数 {
        println!(
            "  - {版本} × {n}（例：{}）",
            样例.get(版本).map(String::as_str).unwrap_or("-")
        );
    }
    assert!(files.is_empty() || 计数.values().sum::<usize>() == files.len() - 读失败);
}

/// 门禁 2·附：真实资源**尾部必须没有多余字节**（否则往返会静默丢尾巴）。
#[test]
fn 尾随字节必须为0() {
    let Some(root) = 印章根目录() else {
        println!("[尾随] 无取证根 MIREDIT_UPSTREAM_ROOT ⇒ 本用例 skip");
        return;
    };
    let Some(files) = 列出印章(&root) else {
        println!("[尾随] 目录不存在 ⇒ 本用例 skip");
        return;
    };
    let mut 非零 = Vec::new();
    for path in &files {
        let bytes = match fs::read(path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        match trailing_bytes(&bytes) {
            Ok(0) => {}
            Ok(n) => 非零.push(format!("{}：尾随 {n} 字节", 展示名(&root, path))),
            Err(e) => 非零.push(format!("{}：解析失败 {e}", 展示名(&root, path))),
        }
    }
    println!(
        "[尾随] 检查 {} 个文件，尾随字节非 0 的有 {} 个",
        files.len(),
        非零.len()
    );
    assert!(非零.is_empty(), "以下文件有尾随字节：\n{}", 非零.join("\n"));
}

/// 负向 1：**截断文件必须 `Err`**（两条：预检拦下 / 预检通过但逐格越界）。
#[test]
fn 负向_截断文件必须报错() {
    // 用例 A：格数 1，但格体只给 20 字节（< 29）⇒ `Document.cs:111` 预检拦下
    let mut a = vec![b'C', b'#', 2];
    a.extend_from_slice(&1i32.to_le_bytes());
    a.extend_from_slice(&[0u8; 20]);
    match Stamp::parse(&a) {
        Err(StampError::TruncatedCellData {
            count,
            minimum_cell_bytes,
            required,
            available,
        }) => {
            println!(
                "[负向1-A] OK：TruncatedCellData count={count} 单格最小={minimum_cell_bytes} 需要={required} 实际={available}"
            );
            assert_eq!(
                (count, minimum_cell_bytes, required, available),
                (1, 29, 29, 20)
            );
        }
        other => panic!("用例 A 应报 TruncatedCellData，实得 {other:?}"),
    }

    // 用例 B：预检能过（30 ≥ 29），但格内声明 1 个前景 ⇒ 实际需要 35 字节 ⇒ 逐格越界
    let mut b = vec![b'C', b'#', 2];
    b.extend_from_slice(&1i32.to_le_bytes());
    let mut cell = vec![0u8; 30];
    cell[18] = 1; // 前景个数 = 1
    b.extend_from_slice(&cell);
    match Stamp::parse(&b) {
        Err(StampError::CellTruncated {
            index,
            offset,
            need,
            len,
        }) => {
            println!(
                "[负向1-B] OK：CellTruncated index={index} offset={offset} need={need} len={len}"
            );
            assert_eq!((index, offset, need, len), (0, 34, 8, 37));
        }
        other => panic!("用例 B 应报 CellTruncated，实得 {other:?}"),
    }

    // 用例 C：把真实文件砍一半 ⇒ 必须 Err（不能「读多少算多少」）
    let 语料 = 印章根目录().and_then(|r| 列出印章(&r));
    if let Some(files) = 语料 {
        if let Some(first) = files.first() {
            if let Ok(bytes) = fs::read(first) {
                let half = &bytes[..bytes.len() / 2];
                let r = Stamp::parse(half);
                println!(
                    "[负向1-C] {} 砍半（{} → {} 字节）⇒ {:?}",
                    first.display(),
                    bytes.len(),
                    half.len(),
                    r.as_ref().err()
                );
                assert!(r.is_err(), "真实文件砍半后仍被解析成功，说明截断没被拦住");
            }
        }
    } else {
        println!("[负向1-C] 目录不存在 ⇒ 该子用例 skip");
    }
}

/// 负向 2：**未知版本必须 `Err`**。
#[test]
fn 负向_未知版本必须报错() {
    for 版本 in [3u8, 4, 9, 255] {
        let mut bytes = vec![b'C', b'#', 版本];
        bytes.extend_from_slice(&0i32.to_le_bytes());
        match Stamp::parse(&bytes) {
            Err(StampError::UnsupportedVersion { version }) => {
                println!("[负向2] 版本 {版本} ⇒ OK：UnsupportedVersion({version})");
                assert_eq!(version, 版本);
            }
            other => panic!("版本 {版本} 应报 UnsupportedVersion，实得 {other:?}"),
        }
    }
    // 边界：版本 2 本身合法（0 格空印章）
    let mut ok = vec![b'C', b'#', 2];
    ok.extend_from_slice(&0i32.to_le_bytes());
    let stamp = Stamp::parse(&ok).expect("v2 空印章应可解析");
    assert_eq!((stamp.version, stamp.cells.len()), (2, 0));
    println!("[负向2] 边界 v2 空印章 ⇒ OK：version=2 cells=0");
}

/// 负向 3：**`count` 与剩余长度不符必须 `Err`**（含负格数、超护栏）。
#[test]
fn 负向_格数与剩余长度不符必须报错() {
    // 3-A：声称 5 格，只给 1 格
    let mut a = vec![b'C', b'#', 2];
    a.extend_from_slice(&5i32.to_le_bytes());
    a.extend_from_slice(&[0u8; 29]);
    match Stamp::parse(&a) {
        Err(StampError::TruncatedCellData {
            count,
            required,
            available,
            ..
        }) => {
            println!("[负向3-A] OK：count={count} 需要={required} 实际={available}");
            assert_eq!((count, required, available), (5, 145, 29));
        }
        other => panic!("用例 3-A 应报 TruncatedCellData，实得 {other:?}"),
    }

    // 3-B：负格数
    let mut b = vec![b'C', b'#', 2];
    b.extend_from_slice(&(-1i32).to_le_bytes());
    match Stamp::parse(&b) {
        Err(StampError::InvalidCellCount { count, limit }) => {
            println!("[负向3-B] OK：InvalidCellCount count={count} limit={limit}");
            assert_eq!((count, limit), (-1, 1_000_000));
        }
        other => panic!("用例 3-B 应报 InvalidCellCount，实得 {other:?}"),
    }

    // 3-C：格数超护栏（2,000,000）
    let mut c = vec![b'C', b'#', 2];
    c.extend_from_slice(&2_000_000i32.to_le_bytes());
    match Stamp::parse(&c) {
        Err(StampError::InvalidCellCount { count, limit }) => {
            println!("[负向3-C] OK：InvalidCellCount count={count} limit={limit}");
            assert_eq!((count, limit), (2_000_000, 1_000_000));
        }
        other => panic!("用例 3-C 应报 InvalidCellCount，实得 {other:?}"),
    }
}

/// 历史版本：**v0 / v1 能读**，但**只能语义往返、不能字节往返**（C# 保存统一 v2）。
#[test]
fn 历史版本_v0_v1可读且只能语义往返() {
    // ---- v0：无前景个数字节，恒按 1 个前景读；Frames/Tick 在 Door 之后 ----
    let mut v0 = vec![b'C', b'#', 0];
    v0.extend_from_slice(&1i32.to_le_bytes());
    v0.extend_from_slice(&3i32.to_le_bytes()); // X
    v0.extend_from_slice(&(-4i32).to_le_bytes()); // Y
    v0.extend_from_slice(&1i16.to_le_bytes()); // BackLibrary
    v0.extend_from_slice(&2i32.to_le_bytes()); // BackImage
    v0.extend_from_slice(&3i16.to_le_bytes()); // MiddleLibrary
    v0.extend_from_slice(&4u16.to_le_bytes()); // MiddleImage
    v0.extend_from_slice(&5i16.to_le_bytes()); // 前景 Library
    v0.extend_from_slice(&6i16.to_le_bytes()); // 前景 Image
    v0.extend_from_slice(&[7, 8]); // Door, DoorOffset
    v0.extend_from_slice(&[9, 10]); // 前景 Frames, Tick（v<2 位置）
    v0.extend_from_slice(&[11, 12]); // MiddleFrames, MiddleTick
    v0.extend_from_slice(&13i16.to_le_bytes()); // TileAnimation
    v0.extend_from_slice(&14i16.to_le_bytes()); // TileOffset
    v0.extend_from_slice(&[15, 16]); // TileFrames, Light
    assert_eq!(
        v0.len(),
        HEADER_LEN + 34,
        "v0 单格应为 34 字节（Document.cs:110）"
    );

    let s0 = Stamp::parse(&v0).expect("v0 应可解析");
    assert_eq!(s0.version, 0);
    assert_eq!(s0.cells.len(), 1);
    let c0 = &s0.cells[0];
    assert_eq!((c0.x, c0.y), (3, -4));
    assert_eq!(
        c0.cell,
        StampCellData {
            back_library: 1,
            back_image: 2,
            middle_library: 3,
            middle_image: 4,
            foreground: Some(Front {
                library: 5,
                image: 6,
                frames: 9,
                tick: 10
            }),
            door: 7,
            door_offset: 8,
            middle_frames: 11,
            middle_tick: 12,
            tile_animation: 13,
            tile_offset: 14,
            tile_frames: 15,
            light: 16,
        }
    );
    let v2_bytes = s0.to_bytes();
    assert_eq!(v2_bytes[2], 2, "重新保存必须写 v2");
    assert_ne!(v2_bytes, v0, "v0 不应与 v2 字节相同");
    let s0b = Stamp::parse(&v2_bytes).expect("v2 应可解析");
    assert_eq!(s0b.cells, s0.cells, "v0 → v2 必须字段级不变");
    println!(
        "[历史版本] v0（{} 字节）⇒ 读出版本 {}、格数 {}；重写为 v2（{} 字节），字段级一致",
        v0.len(),
        s0.version,
        s0.cells.len(),
        v2_bytes.len()
    );

    // ---- v1：有个数字节，前景块只有 4 字节（无 Frames/Tick），Frames/Tick 仍在 Door 之后 ----
    let mut v1 = vec![b'C', b'#', 1];
    v1.extend_from_slice(&1i32.to_le_bytes());
    v1.extend_from_slice(&(-7i32).to_le_bytes()); // X
    v1.extend_from_slice(&9i32.to_le_bytes()); // Y
    v1.extend_from_slice(&1i16.to_le_bytes()); // BackLibrary
    v1.extend_from_slice(&2i32.to_le_bytes()); // BackImage
    v1.extend_from_slice(&3i16.to_le_bytes()); // MiddleLibrary
    v1.extend_from_slice(&4u16.to_le_bytes()); // MiddleImage
    v1.push(1); // 前景个数
    v1.extend_from_slice(&5i16.to_le_bytes()); // 前景 Library
    v1.extend_from_slice(&6i16.to_le_bytes()); // 前景 Image
    v1.extend_from_slice(&[7, 8]); // Door, DoorOffset
    v1.extend_from_slice(&[9, 10]); // 前景 Frames, Tick（v<2 位置）
    v1.extend_from_slice(&[11, 12]); // MiddleFrames, MiddleTick
    v1.extend_from_slice(&13i16.to_le_bytes()); // TileAnimation
    v1.extend_from_slice(&14i16.to_le_bytes()); // TileOffset
    v1.extend_from_slice(&[15, 16]); // TileFrames, Light
    assert_eq!(
        v1.len(),
        HEADER_LEN + 35,
        "v1 有前景单格应为 35 字节（31 + 4）"
    );

    let s1 = Stamp::parse(&v1).expect("v1 应可解析");
    assert_eq!(s1.version, 1);
    assert_eq!(s1.cells[0].cell, c0.cell, "v1 与 v0 解出的字段应相同");
    let v1_as_v2 = s1.to_bytes();
    assert_ne!(v1_as_v2, v1, "v1 不应与 v2 字节相同");
    assert_eq!(
        Stamp::parse(&v1_as_v2).expect("v2 应可解析").cells,
        s1.cells,
        "v1 → v2 必须字段级不变"
    );
    println!(
        "[历史版本] v1（{} 字节）⇒ 读出版本 {}；重写为 v2（{} 字节），字段级一致",
        v1.len(),
        s1.version,
        v1_as_v2.len()
    );

    // ---- v1 无前景：Door 之后那 2 字节（frames/tick）C# 读完就丢（:121），本库同样丢弃 ----
    let mut v1_no_front = vec![b'C', b'#', 1];
    v1_no_front.extend_from_slice(&1i32.to_le_bytes());
    v1_no_front.extend_from_slice(&0i32.to_le_bytes());
    v1_no_front.extend_from_slice(&0i32.to_le_bytes());
    v1_no_front.extend_from_slice(&[0u8; 10]); // 前四字段
    v1_no_front.push(0); // 前景个数 = 0
    v1_no_front.extend_from_slice(&[0u8; 2]); // Door, DoorOffset
    v1_no_front.extend_from_slice(&[0xAA, 0xBB]); // v<2 的 frames/tick ⇒ 无前景时被丢弃
    v1_no_front.extend_from_slice(&[0u8; 8]); // tail
    assert_eq!(
        v1_no_front.len(),
        HEADER_LEN + 31,
        "v1 无前景单格应为 31 字节"
    );
    let s1n = Stamp::parse(&v1_no_front).expect("v1 无前景应可解析");
    assert!(s1n.cells[0].cell.foreground.is_none());
    assert_eq!(
        s1n.to_bytes().len(),
        HEADER_LEN + 29,
        "无前景格重写为 v2 应是 29 字节"
    );
    println!(
        "[历史版本] v1 无前景（31 字节格）⇒ 重写为 v2（29 字节格），Door 后的 frames/tick 按 C# 语义丢弃"
    );
}

/// README 范例锚点：`CrystalStudio/README.md:211-215` 写
/// 「`mir_00000.X` 25 个单元格、8 个前景图片引用，`7 + 25 × 29 + 8 × 6 = 780` 字节」。
/// 本用例把这条**人写的口径**钉在代码上，防止文档与实现各说各话。
#[test]
fn readme范例_mir_00000必须吻合() {
    let Some(root) = 印章根目录() else {
        println!("[README范例] 无取证根 MIREDIT_UPSTREAM_ROOT ⇒ 本用例 skip");
        return;
    };
    let path = root.join("mir").join("mir_00000.X");
    if !path.exists() {
        println!("[README范例] 文件不存在（{}）⇒ 本用例 skip", path.display());
        return;
    }
    let bytes = fs::read(&path).expect("mir_00000.X 应可读");
    let stamp = Stamp::parse(&bytes).expect("mir_00000.X 应可解析");
    let fronts = stamp
        .cells
        .iter()
        .filter(|c| c.cell.foreground.is_some())
        .count();
    println!(
        "[README范例] 实测 {} 字节 / {} 格 / {} 格带前景；README 口径 7 + 25 × 29 + 8 × 6 = 780",
        bytes.len(),
        stamp.cells.len(),
        fronts
    );
    assert_eq!(bytes.len(), 780, "README 声明的文件长度对不上");
    assert_eq!(stamp.cells.len(), 25, "README 声明的格数对不上");
    assert_eq!(fronts, 8, "README 声明的前景格数对不上");
    assert_eq!(
        stamp.byte_len(),
        bytes.len(),
        "按格算出的长度必须等于文件长度"
    );
}

/// 包围盒：闭区间 + 空印章约定。
#[test]
fn 包围盒_闭区间与空印章约定() {
    let mk = |x: i32, y: i32| StampCell {
        x,
        y,
        cell: StampCellData::default(),
    };
    let stamp = Stamp {
        version: 2,
        cells: vec![mk(3, 5), mk(-2, 7), mk(10, -1)],
    };
    assert_eq!(stamp.bounds(), (-2, -1, 10, 7));
    // 与 CrystalStudio 的用法对齐：宽 = max - min + 1
    let (min_x, min_y, max_x, max_y) = stamp.bounds();
    assert_eq!((max_x - min_x + 1, max_y - min_y + 1), (13, 9));
    println!("[包围盒] 3 格 ⇒ {:?}，宽×高 = 13×9", stamp.bounds());

    let empty = Stamp::default();
    assert_eq!(empty.bounds(), (0, 0, -1, -1));
    assert_eq!(empty.to_bytes(), vec![b'C', b'#', 2, 0, 0, 0, 0]);
    println!(
        "[包围盒] 空印章 ⇒ {:?}；空印章字节 = 7 字节头（count=0）",
        empty.bounds()
    );
}

/// 抽样下标（确定性 LCG：同一台机器每次跑都抽同一批，方便和 Python 独立实现对照）。
fn 抽样下标(n: usize, k: usize) -> Vec<usize> {
    let k = k.min(n);
    let mut seed: u64 = 0x5EED_2026_0BAD_F00D;
    let mut picked: Vec<usize> = Vec::with_capacity(k);
    let mut guard = 0usize;
    while picked.len() < k && guard < 100_000 {
        guard += 1;
        seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let idx = ((seed >> 33) as usize) % n;
        if !picked.contains(&idx) {
            picked.push(idx);
        }
    }
    picked.sort_unstable();
    picked
}

/// 把印章转成 `DUMP|` 行（供与 Python 独立实现逐字段对照）。
fn 转储行(label: &str, stamp: &Stamp, bytes_len: usize, trailing: usize) -> Vec<String> {
    let mut out = Vec::with_capacity(stamp.cells.len() + 2);
    out.push(format!("DUMP|file|{label}"));
    out.push(format!(
        "DUMP|header|version={}|cells={}|bytes={}|trailing={}",
        stamp.version,
        stamp.cells.len(),
        bytes_len,
        trailing
    ));
    for (i, c) in stamp.cells.iter().enumerate() {
        let front = match &c.cell.foreground {
            Some(f) => format!("{}:{}:{}:{}", f.library, f.image, f.frames, f.tick),
            None => "none".to_string(),
        };
        out.push(format!(
            "DUMP|cell|{i}|x={}|y={}|back_library={}|back_image={}|middle_library={}|middle_image={}|front={front}|door={}|door_offset={}|middle_frames={}|middle_tick={}|tile_animation={}|tile_offset={}|tile_frames={}|light={}",
            c.x,
            c.y,
            c.cell.back_library,
            c.cell.back_image,
            c.cell.middle_library,
            c.cell.middle_image,
            c.cell.door,
            c.cell.door_offset,
            c.cell.middle_frames,
            c.cell.middle_tick,
            c.cell.tile_animation,
            c.cell.tile_offset,
            c.cell.tile_frames,
            c.cell.light
        ));
    }
    out
}

/// 字段级抽查：确定性抽 5 个印章，把**每个字段**打成 `DUMP|` 行。
///
/// 与 `stamp_dump.py`（按 `CellCodec` 顺序独立写的 Python 解码器）的输出逐行比对，
/// 必须完全相同。本用例同时断言「抽到的 5 个文件自身往返逐字节相等」。
#[test]
fn 字段级转储_5个样本() {
    let Some(root) = 印章根目录() else {
        println!("[转储] 无取证根 MIREDIT_UPSTREAM_ROOT ⇒ 本用例 skip");
        return;
    };
    let Some(files) = 列出印章(&root) else {
        println!("[转储] 目录不存在 ⇒ 本用例 skip");
        return;
    };
    let 下标 = 抽样下标(files.len(), 5);
    println!(
        "[转储] 从 {} 个文件里确定性抽 {} 个：",
        files.len(),
        下标.len()
    );
    for i in &下标 {
        println!("  idx={i} → {}", 展示名(&root, &files[*i]));
    }
    for i in &下标 {
        let path = &files[*i];
        let label = 展示名(&root, path);
        let bytes = fs::read(path).expect("抽样文件应可读");
        let stamp = Stamp::parse(&bytes).expect("抽样文件应可解析");
        assert_eq!(stamp.to_bytes(), bytes, "{label} 往返不一致");
        let trailing = trailing_bytes(&bytes).expect("尾随字节应可算");
        for line in 转储行(&label, &stamp, bytes.len(), trailing) {
            println!("{line}");
        }
    }
}
