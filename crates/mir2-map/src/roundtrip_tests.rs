//! 全量往返门（数据驱动，跑真实地图资源）。
//!
//! 用法：`cargo test -p mir2-map -- --nocapture`（加 `--nocapture` 才能看到统计行）。
//!
//! 纪律：
//! - **目录或文件不存在 ⇒ 打印警告并 skip，不 panic**（环境缺资源不算代码错）；
//! - 跳过必须**打印原因**，不静默；
//! - 任何真实字节不一致 ⇒ 断言失败并给出**文件名 + 首个不一致偏移 + 前后各 4 字节**。

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::cell::{Cell, CellFormat, HEADER_LEN, TAIL_LEN, TAIL_OFFSET};
use crate::error::MapError;
use crate::map::Map;

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

/// 36 档地图目录：官方 23 周年客户端（实测 1097 个 `.map`，其中 Ext36 = 194 个）。
fn 目录_36档(root: &Path) -> PathBuf {
    root.join("client").join("热血传奇23周年客户端").join("Map")
}

/// 12/14 档地图目录：Godot 客户端自带（实测 60 个 12/14 档 `.map`）。
fn 目录_12_14档(root: &Path) -> PathBuf {
    root.join("GodotClient").join("maps")
}

/// 尾区重点样本（实测 700×700、1660 格尾区非零）。
fn 文件_n0(root: &Path) -> PathBuf {
    目录_36档(root).join("n0.map")
}

/// 往返统计。
#[derive(Default)]
struct Stats {
    /// 扫描到的 `.map` 文件数。
    scanned: usize,
    /// 往返成功数。
    success: usize,
    /// 往返失败数。
    failed: usize,
    /// 跳过数（非本用例档位 / 长度不符 / 读取失败）。
    skipped: usize,
    /// 跳过原因 → 次数。
    skip_reasons: BTreeMap<String, usize>,
    /// 跳过样例（最多 5 条，便于人看）。
    skip_samples: Vec<String>,
    /// 失败详情（最多 20 条）。
    failures: Vec<String>,
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
        assert!(
            self.success > 0,
            "一个文件都没跑通：检查资源目录是否为空、或档位筛选是否全部落空"
        );
    }
}

/// 列出目录下所有 `.map`（大小写不敏感），按路径排序保证输出稳定。
///
/// 目录不存在 / 不可读 / 无 `.map` ⇒ 打印警告并返回 `None`（调用方 skip，不 panic）。
fn 列出地图(dir: &Path) -> Option<Vec<PathBuf>> {
    let p = dir;
    if !p.exists() {
        eprintln!("[跳过] 目录不存在：{} —— 本用例 skip（不 panic）", dir.display());
        return None;
    }
    let rd = match fs::read_dir(p) {
        Ok(rd) => rd,
        Err(e) => {
            eprintln!("[跳过] 目录无法读取：{}（{e}） —— 本用例 skip", dir.display());
            return None;
        }
    };
    let mut out = Vec::new();
    let mut 读失败 = 0usize;
    for entry in rd {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                eprintln!("[警告] 目录项读取失败：{e}");
                读失败 += 1;
                continue;
            }
        };
        let path = entry.path();
        let 是地图 = path
            .extension()
            .map(|e| e.eq_ignore_ascii_case("map"))
            .unwrap_or(false);
        if !是地图 {
            continue;
        }
        match entry.file_type() {
            Ok(ft) if ft.is_file() => out.push(path),
            Ok(_) => {}
            Err(e) => eprintln!("[警告] 无法取文件类型 {}：{e}", path.display()),
        }
    }
    out.sort();
    if out.is_empty() {
        eprintln!("[跳过] 目录里没有 .map 文件：{} —— 本用例 skip", dir.display());
        return None;
    }
    if 读失败 > 0 {
        eprintln!(
            "[警告] 目录 {} 有 {读失败} 个目录项读取失败，已计入警告但未参与统计",
            dir.display()
        );
    }
    Some(out)
}

fn 文件名(path: &Path) -> String {
    path.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// 取头部宽高（调用前须保证 `bytes.len() >= HEADER_LEN`）。
fn 头部宽高(bytes: &[u8]) -> (u16, u16) {
    (
        u16::from_le_bytes([bytes[0], bytes[1]]),
        u16::from_le_bytes([bytes[2], bytes[3]]),
    )
}

/// 首个不一致偏移。
fn 首个差异(a: &[u8], b: &[u8]) -> Option<usize> {
    a.iter().zip(b.iter()).position(|(x, y)| x != y)
}

/// `parse → to_bytes` 并向原始字节逐字节比对。
///
/// 成功返回解析出的 [`Map`]；失败返回「首个不一致偏移 + 前后 4 字节」的可读描述。
fn 往返检查(bytes: &[u8]) -> Result<Map, String> {
    let map = Map::parse(bytes).map_err(|e| format!("parse 失败：{e}"))?;
    let out = map.to_bytes();
    if out.len() != bytes.len() {
        return Err(format!("写回长度 {} ≠ 原始 {}", out.len(), bytes.len()));
    }
    if let Some(i) = 首个差异(bytes, &out) {
        let lo = i.saturating_sub(4);
        return Err(format!(
            "首个不一致偏移 {i}（0x{i:X}）：原始 {:02X?} vs 写回 {:02X?}",
            &bytes[lo..(i + 4).min(bytes.len())],
            &out[lo..(i + 4).min(out.len())]
        ));
    }
    Ok(map)
}

/// 用例 1：36 档全量往返（目录内所有 `.map`，按长度筛出 Ext36）。
#[test]
fn ext36_全量往返() {
    let Some(root) = 资产根() else {
        eprintln!("[skip] 无资产根 MIREDIT_ASSET_ROOT");
        return;
    };
    let 目录 = 目录_36档(&root);
    let Some(files) = 列出地图(&目录) else {
        return;
    };

    let mut st = Stats::default();
    let mut 档位分布: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut 不符三档 = 0usize;

    for path in &files {
        st.scanned += 1;
        let bytes = match fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                st.skip("读取失败", format!("{}：{e}", 文件名(path)));
                continue;
            }
        };
        if bytes.len() < HEADER_LEN {
            st.skip(
                "头部不足 52 字节",
                format!("{}（{} 字节）", 文件名(path), bytes.len()),
            );
            continue;
        }
        let (w, h) = 头部宽高(&bytes);
        match CellFormat::detect(w, h, bytes.len()) {
            Some(CellFormat::Ext36) => {
                *档位分布.entry("Ext36").or_insert(0) += 1;
                match 往返检查(&bytes) {
                    Ok(_) => st.success += 1,
                    Err(msg) => {
                        st.fail(format!("{} [{w}x{h}]：{msg}", 文件名(path)));
                        // 失败也计在 Ext36 档内，下面的总数用 success+failed 核对
                    }
                }
            }
            Some(other) => {
                *档位分布.entry(other.name()).or_insert(0) += 1;
                st.skip(
                    &format!("非 Ext36 档（{}）", other.name()),
                    format!("{}（{} 字节）", 文件名(path), bytes.len()),
                );
            }
            None => {
                不符三档 += 1;
                st.skip(
                    "长度与三档皆不匹配",
                    format!("{}（{} 字节）", 文件名(path), bytes.len()),
                );
            }
        }
    }

    let ext36_共 = *档位分布.get("Ext36").unwrap_or(&0);
    println!("========== 用例 1：Ext36 全量往返 ==========");
    println!("[EXT36 用例] 目录：{}", 目录.display());
    println!(
        "[EXT36 用例] 扫描 .map {} 个：Old12={} New14={} Ext36={} 长度不符三档={}",
        st.scanned,
        *档位分布.get("Old12").unwrap_or(&0),
        *档位分布.get("New14").unwrap_or(&0),
        ext36_共,
        不符三档
    );
    println!(
        "EXT36 共 {ext36_共} 个（成功 {} / 失败 {} / 跳过 {}）",
        st.success,
        st.failed,
        ext36_共 - st.success - st.failed
    );
    st.打印跳过摘要();
    st.断言全绿();
    assert_eq!(
        ext36_共,
        st.success + st.failed,
        "Ext36 计数自洽性检查失败（跳出档位筛选逻辑？）"
    );
}

/// 用例 2：12/14 档全量往返（Godot 客户端自带地图目录）。
///
/// 目录里若混入 Ext36 文件（实测有一个 `t36_probe.map`），
/// 一律按「非本用例档位」计入跳过并打印原因，不静默吞掉。
#[test]
fn old12_new14_全量往返() {
    let Some(root) = 资产根() else {
        eprintln!("[skip] 无资产根 MIREDIT_ASSET_ROOT");
        return;
    };
    let 目录 = 目录_12_14档(&root);
    let Some(files) = 列出地图(&目录) else {
        return;
    };

    let mut st = Stats::default();
    let mut 档位分布: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut 不符三档 = 0usize;

    for path in &files {
        st.scanned += 1;
        let bytes = match fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                st.skip("读取失败", format!("{}：{e}", 文件名(path)));
                continue;
            }
        };
        if bytes.len() < HEADER_LEN {
            st.skip(
                "头部不足 52 字节",
                format!("{}（{} 字节）", 文件名(path), bytes.len()),
            );
            continue;
        }
        let (w, h) = 头部宽高(&bytes);
        match CellFormat::detect(w, h, bytes.len()) {
            Some(fmt @ (CellFormat::Old12 | CellFormat::New14)) => {
                *档位分布.entry(fmt.name()).or_insert(0) += 1;
                match 往返检查(&bytes) {
                    Ok(_) => st.success += 1,
                    Err(msg) => st.fail(format!("{} [{w}x{h}] {fmt:?}：{msg}", 文件名(path))),
                }
            }
            Some(other) => {
                *档位分布.entry(other.name()).or_insert(0) += 1;
                st.skip(
                    &format!("非 12/14 档（{}）", other.name()),
                    format!("{}（{} 字节，{}×{}）", 文件名(path), bytes.len(), w, h),
                );
            }
            None => {
                不符三档 += 1;
                st.skip(
                    "长度与三档皆不匹配",
                    format!("{}（{} 字节）", 文件名(path), bytes.len()),
                );
            }
        }
    }

    let old12 = *档位分布.get("Old12").unwrap_or(&0);
    let new14 = *档位分布.get("New14").unwrap_or(&0);
    let 共 = old12 + new14;
    println!("========== 用例 2：Old12 + New14 全量往返 ==========");
    println!("[12/14 用例] 目录：{}", 目录.display());
    println!(
        "[12/14 用例] 扫描 .map {} 个：Old12={old12} New14={new14} Ext36={} 长度不符三档={}",
        st.scanned,
        *档位分布.get("Ext36").unwrap_or(&0),
        不符三档
    );
    println!(
        "OLD12+NEW14 共 {共} 个（成功 {} / 失败 {} / 跳过 {}）",
        st.success,
        st.failed,
        共 - st.success - st.failed
    );
    st.打印跳过摘要();
    st.断言全绿();
    assert_eq!(共, st.success + st.failed, "12/14 计数自洽性检查失败");
}

/// 用例 3：`n0.map` 尾区定向断言。
///
/// 在 `x=300..340`、`y=250..300` 区间内，对**所有尾区非零的格**断言：
/// `parse` 后的 `cell.tail` 与原文件对应 22 字节逐字节相等，且 `to_bytes` 后仍相等。
/// 同时断言该区间确实存在非零尾区格（否则断言会空转）。
#[test]
fn n0_尾区定向断言() {
    let Some(root) = 资产根() else {
        eprintln!("[skip] 无资产根 MIREDIT_ASSET_ROOT");
        return;
    };
    let path = 文件_n0(&root);
    if !path.exists() {
        eprintln!("[跳过] 文件不存在：{} —— 本用例 skip（不 panic）", path.display());
        return;
    }
    let bytes = fs::read(&path)
        .unwrap_or_else(|e| panic!("读取 {} 失败：{e}", path.display()));
    let map = Map::parse(&bytes)
        .unwrap_or_else(|e| panic!("{} 解析失败：{e}", path.display()));

    assert_eq!(map.format, CellFormat::Ext36, "n0.map 应为 36 档");
    assert_eq!((map.width, map.height), (700, 700), "n0.map 应为 700×700");
    assert_eq!(map.cells.len(), 700 * 700);

    let h = map.height as usize;
    let out = map.to_bytes();
    assert_eq!(out.len(), bytes.len(), "写回长度变了");

    let mut 区间格数 = 0usize;
    let mut 非零格数 = 0usize;
    let mut 样本: Vec<(u16, u16, Vec<u8>)> = Vec::new();
    for x in 300u16..=340 {
        for y in 250u16..=300 {
            区间格数 += 1;
            let base = HEADER_LEN + (x as usize * h + y as usize) * 36 + TAIL_OFFSET;
            let 原始尾区 = &bytes[base..base + TAIL_LEN];
            let cell = map
                .cell(x, y)
                .unwrap_or_else(|| panic!("({x},{y}) 应命中，实际 None"));
            assert_eq!(
                cell.tail.as_slice(),
                原始尾区,
                "({x},{y}) parse 后 tail 与原文件 22 字节不一致"
            );
            assert_eq!(
                cell.tail.as_slice(),
                &out[base..base + TAIL_LEN],
                "({x},{y}) to_bytes 后 tail 与原文件 22 字节不一致"
            );
            if 原始尾区.iter().any(|&b| b != 0) {
                非零格数 += 1;
                if 样本.len() < 6 {
                    样本.push((x, y, 原始尾区.to_vec()));
                }
            }
        }
    }

    assert!(
        非零格数 > 0,
        "定向区间内没有 tail 非零的格 ⇒ 断言会空转，判为失败（资源变了？）"
    );

    // 全图尾区非零格数（实测值 1660，仅打印作证据，不做硬断言以免资源更新即误红）
    let mut 全图非零 = 0usize;
    for x in 0..map.width as usize {
        for y in 0..h {
            let base = HEADER_LEN + (x * h + y) * 36 + TAIL_OFFSET;
            if bytes[base..base + TAIL_LEN].iter().any(|&b| b != 0) {
                全图非零 += 1;
            }
        }
    }

    println!("========== 用例 3：n0.map 尾区定向断言 ==========");
    println!(
        "[n0 定向] 区间 x=300..340 × y=250..300 共 {区间格数} 格，其中 tail 非零 {非零格数} 格；\
         parse 后与 to_bytes 后均与原始 22 字节逐字节相等"
    );
    for (x, y, tail) in &样本 {
        println!("  样本 ({x},{y}) tail = {:02X?}", tail);
    }
    println!(
        "[n0 定向] 全图 700×700 尾区非零格数 = {全图非零}（占 {:.4}%）",
        100.0 * 全图非零 as f64 / (700.0 * 700.0)
    );
    assert_eq!(out, bytes, "n0.map 整文件写回不一致");
}

/// 用例 4：负向 —— 长度不匹配三档必须 `Err`，且错误信息含三档期望长度。
#[test]
fn 负向_长度不匹配必须报错() {
    println!("========== 用例 4：负向（长度不匹配） ==========");

    // 3×3 图：Old12 期望 52+108=160、New14=178、Ext36=376；这里给 161
    let mut bytes = vec![0u8; HEADER_LEN + 12 * 9 + 1];
    bytes[0] = 3;
    bytes[2] = 3;
    let err = Map::parse(&bytes).expect_err("长度 161 不与三档匹配，必须返回 Err");
    assert!(
        matches!(err, MapError::LengthMismatch { .. }),
        "错误类型不对：{err:?}"
    );
    let msg = err.to_string();
    for 期望 in ["160", "178", "376", "161"] {
        assert!(msg.contains(期望), "错误信息缺少「{期望}」：{msg}");
    }
    for 档 in ["Old12", "New14", "Ext36"] {
        assert!(msg.contains(档), "错误信息缺少档位名「{档}」：{msg}");
    }
    println!("[负向] 161 字节被拒绝 ✅ 错误信息 = {msg}");

    // 其它「差一字节」形态也必须拒绝：头部声明 3×3（三档期望 160/178/376），
    // 文件长度与三档都不等 ⇒ **必须是 LengthMismatch**。
    // ⚠ 头部必须写死 3×3：若拿 52 字节以上的全零（w=0,h=0）文件来测，
    //    会先被尺寸护栏（ZeroDimension）拦下 —— 那时这条断言就测不到长度判据了。
    for len in [159usize, 164, 177, 179, 375, 377] {
        let mut bytes = vec![0u8; len];
        bytes[0] = 3;
        bytes[2] = 3;
        let e = Map::parse(&bytes).expect_err("长度不符三档必须 Err");
        assert!(
            matches!(e, MapError::LengthMismatch { .. }),
            "长度 {len} 字节应报 LengthMismatch，实际 {e:?}"
        );
    }

    // 头部不足 52 字节 ⇒ TooShort（长度判据与尺寸护栏之前就拦下）
    for len in [0usize, 12, 51] {
        let e = Map::parse(&vec![0u8; len]).expect_err("头部不足必须 Err");
        assert!(matches!(e, MapError::TooShort { len: l } if l == len), "{e:?}");
    }

    // 头部不足 52 字节
    let short = vec![0u8; 51];
    let e = Map::parse(&short).expect_err("51 字节必须 Err");
    assert!(
        matches!(e, MapError::TooShort { len: 51 }),
        "错误类型不对：{e:?}"
    );
    println!("[负向] 51 字节被拒绝 ✅ 错误信息 = {e}");
}

/// 用例 5：反向用例。
///
/// a) 把某 14 档图补零到 36 档长度 ⇒ **必须判为 Ext36**，且前 14 字节字段与尾区全零、
///    往返一致；
/// b) 把该 36 档图「按 14 档长度截断」⇒ 长度恰好等于 New14 期望长度，
///    **长度判据下必然判为 New14**（这是判据固有限制，无法 Err —— 见下方注释），
///    因此真正的负向断言改为「截到 14 档长度 − 1 ⇒ 必须 Err」。
#[test]
fn 反向_14b补零升档与36b截断() {
    println!("========== 用例 5：反向（补零升档 / 截断） ==========");
    let Some(root) = 资产根() else {
        eprintln!("[skip] 无资产根 MIREDIT_ASSET_ROOT");
        return;
    };
    let 目录 = 目录_12_14档(&root);
    let Some(files) = 列出地图(&目录) else {
        return;
    };

    // 挑最小的 New14 样本，且要求第 2 格非空（下面用来证明「截断后内容错位」）
    let mut 样本: Option<(PathBuf, Vec<u8>, Map)> = None;
    let mut 最小长 = usize::MAX;
    for p in &files {
        let Ok(bytes) = fs::read(p) else { continue };
        if bytes.len() < HEADER_LEN {
            continue;
        }
        let (w, h) = 头部宽高(&bytes);
        if CellFormat::detect(w, h, bytes.len()) != Some(CellFormat::New14) {
            continue;
        }
        if bytes.len() >= 最小长 {
            continue;
        }
        let map = match Map::parse(&bytes) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("[警告] {} 判为 New14 但解析失败：{e}", 文件名(p));
                continue;
            }
        };
        if map.cells.len() < 2 || map.cells[1] == Cell::default() {
            continue; // 需要一格已知非空作为错位证据
        }
        最小长 = bytes.len();
        样本 = Some((p.clone(), bytes, map));
    }

    let Some((样本路径, raw14, map14)) = 样本 else {
        eprintln!(
            "[跳过] {} 里找不到「第 2 格非空」的 New14 样本 —— 本用例 skip",
            目录.display()
        );
        return;
    };
    let w = map14.width as usize;
    let h = map14.height as usize;
    let 格数 = w * h;
    assert_eq!(map14.cells.len(), 格数);
    println!(
        "[反向] 样本：{}  {}×{} = {格数} 格，原始 {} 字节（New14）",
        文件名(&样本路径),
        w,
        h,
        raw14.len()
    );

    // ---- a) 补零升档到 36 档长度 ----
    let mut ext = vec![0u8; HEADER_LEN + 36 * 格数];
    ext[..HEADER_LEN].copy_from_slice(&raw14[..HEADER_LEN]);
    for i in 0..格数 {
        let src = HEADER_LEN + i * 14;
        let dst = HEADER_LEN + i * 36;
        ext[dst..dst + 14].copy_from_slice(&raw14[src..src + 14]);
    }
    assert_eq!(
        CellFormat::detect(map14.width, map14.height, ext.len()),
        Some(CellFormat::Ext36),
        "补零到 36 档长度后必须判为 Ext36"
    );
    let map36 = Map::parse(&ext).expect("36 档长度应解析成功");
    assert_eq!(map36.format, CellFormat::Ext36);
    assert_eq!(map36.cells.len(), 格数);
    for i in 0..格数 {
        let a = &map14.cells[i];
        let b = &map36.cells[i];
        assert_eq!(
            (
                a.bk_img,
                a.mid_img,
                a.fr_img,
                a.door_index,
                a.door_offset,
                a.ani_frame,
                a.ani_tick,
                a.area,
                a.light,
                a.tiles,
                a.sm_tiles
            ),
            (
                b.bk_img,
                b.mid_img,
                b.fr_img,
                b.door_index,
                b.door_offset,
                b.ani_frame,
                b.ani_tick,
                b.area,
                b.light,
                b.tiles,
                b.sm_tiles
            ),
            "第 {i} 格前 14 字节字段搬运不一致"
        );
        assert_eq!(b.tail, [0u8; TAIL_LEN], "补零后 tail 必须全零（第 {i} 格）");
    }
    assert_eq!(map36.to_bytes(), ext, "补零图往返必须逐字节一致");
    println!("[反向] a) 14 档补零到 36 档长度 ⇒ 判 Ext36 ✅ 往返一致、tail 全零");

    // ---- b) 36 档图按 14 档长度截断 ----
    let len14 = HEADER_LEN + 14 * 格数;
    let 截断: Vec<u8> = ext[..len14].to_vec();
    assert_eq!(截断.len(), len14);
    match Map::parse(&截断) {
        Ok(m) => {
            // 说明：长度判据只看 len，14 档长度恰好成立 ⇒ 必然判 New14。
            // 这不是「通过项」，而是判据的固有限制，必须显式打印出来。
            assert_eq!(
                m.format,
                CellFormat::New14,
                "截到 14 档长度后按长度只能判 New14，档位不符说明判据被改动"
            );
            assert_eq!(m.cells.len(), 格数);
            assert_eq!(
                m.cells[1],
                Cell::default(),
                "截断后第 2 格应落在原 36 档第 1 格的尾区零字节里 ⇒ 必然全零"
            );
            assert_ne!(
                m.cells[1], map14.cells[1],
                "截断读出的第 2 格不该等于原图第 2 格（数据已错位）"
            );
            println!(
                "[反向] b) ⚠ 36 档图截到 14 档长度（{len14} 字节）⇒ 长度恰好等于 New14 期望值，\
                 判据无法识别，被解析为 New14（数据已错位：第 2 格变全零）。\
                 这是「唯一判据 = 长度」的固有限制，不是本库缺陷。"
            );
        }
        Err(e) => panic!("截到 14 档长度后长度恰好成立，不该 Err：{e}"),
    }

    // 真正的负向：截到 14 档长度 − 1 ⇒ 必须 Err 且含三档期望长度
    let 少一: Vec<u8> = ext[..len14 - 1].to_vec();
    let err = Map::parse(&少一).expect_err("14 档长度 −1 不与三档匹配，必须 Err");
    let msg = err.to_string();
    let (e12, e14, e36) = crate::cell::expected_lengths(map14.width, map14.height);
    for 期望 in [
        e12.to_string(),
        e14.to_string(),
        e36.to_string(),
        少一.len().to_string(),
    ] {
        assert!(msg.contains(&期望), "错误信息缺少「{期望}」：{msg}");
    }
    println!("[反向] c) 36 档图截到 14 档长度 −1 ⇒ 正确 Err ✅ 错误信息 = {msg}");
}
