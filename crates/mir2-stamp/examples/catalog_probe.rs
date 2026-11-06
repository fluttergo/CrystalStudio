//! 印章目录索引的**取证探针**（不是测试）：一次跑出验收门要的全部数字。
//!
//! 用法（release，务必带 `--release`，否则 `load()` 的耗时是 debug 口径）：
//!
//! ```text
//! set CARGO_INCREMENTAL=0
//! set CARGO_TARGET_DIR=<任意可写目录>\miredit-stamp-target
//! cargo run --release -p mir2-stamp --example catalog_probe
//! ```
//!
//! 输出覆盖：
//! 1. `load()` 耗时（**进程内首次 = 近似冷**；同进程第二次 = 热）；
//! 2. 解析条数 / 跳过数 / 文件命中与缺失（含缺失前 5 条）；
//! 3. `unlisted()` 数量与前 5 个；
//! 4. 前 5 个分类的计数 + `Building` / `Tree` + `by_region("Province")`；
//! 5. `search()` 三次查询的计数与前 3 个 id + 单次耗时（1,000 次取最小/平均）；
//! 6. `resolve()` 5 个样本与**直接 `Stamp::parse`** 的逐项对照。
//!
//! ⚠ 本探针**只读** `CrystalStudio/`，不写任何文件。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use mir2_stamp::catalogue::StampCatalog;
use mir2_stamp::Stamp;

/// 取证根缺省值：`.X` / `palette.json` 的取证来源（**本文件里只出现这一处**）。
/// 换机器用 `MIREDIT_UPSTREAM_ROOT`。
const 缺省取证根: &str = r"I:\miredit\_upstream";

/// 取证根：`MIREDIT_UPSTREAM_ROOT` → 缺省 [`缺省取证根`]；根不存在 ⇒ `None`。
///
/// 只做 `var_os` + `PathBuf`，不引入任何第三方库。
fn 取证根() -> Option<PathBuf> {
    let root = std::env::var_os("MIREDIT_UPSTREAM_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(缺省取证根));
    root.is_dir().then_some(root)
}

/// palette.json 路径（`MIR2_STAMP_PALETTE` 优先；否则 `<取证根>/catalogue/palette.json`）。
fn 目录文件() -> Result<PathBuf, String> {
    match std::env::var_os("MIR2_STAMP_PALETTE") {
        Some(v) => Ok(PathBuf::from(v)),
        None => 取证根()
            .map(|r| r.join("catalogue").join("palette.json"))
            .ok_or_else(|| "无取证根 MIREDIT_UPSTREAM_ROOT（探针需要取证素材）".to_string()),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = 目录文件()?;
    println!("[探针] palette.json = {}", path.display());

    // —— 1. load 冷/热 ——
    let t0 = Instant::now();
    let mut catalog = StampCatalog::load(&path)?;
    let 冷 = t0.elapsed();
    let t1 = Instant::now();
    let 热目录 = StampCatalog::load(&path)?;
    let 热 = t1.elapsed();
    println!("[性能] load 首次（进程内首跑，近似冷）= {:.3} ms", 冷.as_secs_f64() * 1e3);
    println!("[性能] load 第二次（同进程，热）= {:.3} ms", 热.as_secs_f64() * 1e3);
    println!("[性能] 两次 len 一致 = {}", catalog.len() == 热目录.len());
    drop(热目录);

    // —— 2. 解析条数与文件命中 ——
    let stats = catalog.stats().clone();
    println!("[解析] objects[] 原始条数 = {}", stats.object_entries);
    println!("[解析] 进入索引条数 len() = {}", catalog.len());
    println!("[解析] 跳过条数 = {}（其中 id 重复 {}）", stats.skipped_entries, stats.duplicate_ids);
    if !stats.skipped_samples.is_empty() {
        println!("[解析] 跳过样例：{:?}", stats.skipped_samples);
    }
    println!(
        "[解析] 文件命中 = {} / 缺失 = {}（objectRoot 存在 = {}，扫到 .X {} 个，扫描错误 {}）",
        stats.files_present,
        stats.files_missing,
        stats.object_root_present,
        stats.scanned_files,
        stats.scan_dir_errors
    );
    if !stats.missing_samples.is_empty() {
        println!("[解析] 缺失前 5 个：{:?}", stats.missing_samples);
    }

    // —— 3. 未登记文件 ——
    let unlisted = catalog.unlisted();
    println!("[未登记] unlisted() 数量 = {}", unlisted.len());
    for (i, rel) in unlisted.iter().take(5).enumerate() {
        println!("[未登记]   #{} {}", i + 1, rel);
    }

    // —— 4. 分类 / 区域计数 ——
    let mut 分类计数: BTreeMap<&str, usize> = BTreeMap::new();
    for entry in catalog.all() {
        if let Some(kind) = &entry.kind {
            if !kind.trim().is_empty() {
                *分类计数.entry(kind.as_str()).or_insert(0) += 1;
            }
        }
    }
    let mut 排序: Vec<(&str, usize)> = 分类计数.into_iter().collect();
    排序.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    println!("[分类] 不同 kind 数 = {}", 排序.len());
    for (kind, n) in 排序.iter().take(5) {
        println!("[分类]   前5：{kind} = {n}（by_kind 实查 = {}）", catalog.by_kind(kind).len());
    }
    for kind in ["Building", "Tree"] {
        println!("[分类]   {kind} by_kind = {}", catalog.by_kind(kind).len());
    }
    println!("[区域] by_region(\"Province\") = {}", catalog.by_region("Province").len());
    println!(
        "[区域] by_region_group(\"BichonProvince\") = {}",
        catalog.by_region_group("BichonProvince").len()
    );

    // —— 5. 搜索 ——
    for needle in ["market", "树", "mir_00000"] {
        let hits = catalog.search(needle);
        let ids: Vec<&str> = hits.iter().take(3).map(|e| e.id.as_str()).collect();
        println!("[搜索] search({needle:?}) = {} 条，前 3 个 id = {:?}", hits.len(), ids);
    }
    // 单次耗时：1,000 次，取最小 / 平均。
    let mut 最小 = f64::MAX;
    let mut 合计 = 0.0f64;
    let 轮数 = 1000;
    for _ in 0..轮数 {
        let t = Instant::now();
        let hits = catalog.search("market");
        let cost = t.elapsed().as_secs_f64() * 1e3;
        std::hint::black_box(hits.len());
        合计 += cost;
        if cost < 最小 {
            最小 = cost;
        }
    }
    println!(
        "[性能] search(\"market\") 单次：最小 {:.4} ms / 平均 {:.4} ms（{} 次）",
        最小,
        合计 / f64::from(轮数),
        轮数
    );

    // —— 6. resolve 与直接解析对照（5 个样本：等距抽样，保证可复现）——
    println!("[resolve] 样本对照（resolve vs 直接 Stamp::parse）：");
    let total = catalog.len();
    let 样本下标 = [0usize, total / 4, total / 2, total * 3 / 4, total - 1];
    for 下标 in 样本下标 {
        let id = catalog.all()[下标].id.clone();
        let resolved = catalog.resolve(&id);
        match resolved {
            Some(r) => {
                let bytes = std::fs::read(&r.path)?;
                let direct = Stamp::parse(&bytes)?;
                let 直接格数 = direct.cells.len();
                let 直接包围盒 = direct.bounds();
                println!(
                    "[resolve]   {id} → 存在={} 格数 {}/{} 包围盒 {:?}/{:?} 一致={} 路径={}",
                    r.path.is_file(),
                    r.cell_count,
                    直接格数,
                    r.bounds,
                    直接包围盒,
                    r.cell_count == 直接格数 && r.bounds == 直接包围盒,
                    r.path.display()
                );
            }
            None => println!("[resolve]   {id} → None（读不出或解析失败）"),
        }
    }
    // 缓存命中耗时（第二次 resolve 应只走 HashMap）。
    let t = Instant::now();
    let 重复 = catalog.resolve(&catalog.all()[0].id.clone());
    println!(
        "[性能] resolve 缓存命中耗时 = {:.4} ms（命中 = {}）",
        t.elapsed().as_secs_f64() * 1e3,
        重复.is_some()
    );
    println!("[统计] resolve 读失败 {} / 解析失败 {}", catalog.stats().resolve_read_errors, catalog.stats().resolve_parse_errors);
    println!("[统计] object_root = {}", catalog.object_root().display());
    println!("[统计] preview_root = {}", catalog.preview_root().display());
    Ok(())
}
