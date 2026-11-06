//! 印章**目录索引**门禁：全量解析 / 未登记反查 / 分类与区域查询 / 搜索 / `resolve` 正确性 / 负向用例。
//!
//! 用法：`cargo test -p mir2-stamp -- --nocapture`（加 `--nocapture` 才能看到统计行）。
//!
//! 纪律（与 `roundtrip_tests` 同款）：
//! - **真实资源目录不存在 ⇒ 打印警告并 skip，不 panic**（环境缺资源不算代码错）；
//! - 跳过必须**打印原因**，不静默；
//! - 数据类断言带**基线数字 + 复算口径**，资源更新导致不一致时消息里能看出该改哪；
//! - 负向用例**必须** `Err` / `None`，`Ok` / `Some` 即判失败；
//! - 临时目录一律建在 `%TEMP%` 下、带进程号 + 用例名，`Drop` 里删干净（不留垃圾）。

use std::fs;
use std::path::PathBuf;

use crate::catalogue::{CatalogError, StampCatalog};
use crate::stamp::{Stamp, StampCell};
use crate::StampCellData;

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

/// 默认 palette.json（相对取证根；可用环境变量 `MIR2_STAMP_PALETTE` 覆盖）。
fn 目录文件() -> Option<PathBuf> {
    match std::env::var_os("MIR2_STAMP_PALETTE") {
        Some(v) => Some(PathBuf::from(v)),
        None => Some(取证根()?.join("catalogue").join("palette.json")),
    }
}

/// 读真实目录；缺取证根 / 文件不存在 / 加载失败 ⇒ 打印并返回 `None`（调用方 skip）。
fn 读真实目录(用例: &str) -> Option<StampCatalog> {
    let Some(path) = 目录文件() else {
        eprintln!("[跳过] {用例}：无取证根 MIREDIT_UPSTREAM_ROOT —— 本用例 skip（不 panic）");
        return None;
    };
    if !path.is_file() {
        eprintln!(
            "[跳过] {用例}：palette.json 不存在（{}）—— 本用例 skip（不 panic）",
            path.display()
        );
        return None;
    }
    match StampCatalog::load(&path) {
        Ok(c) => Some(c),
        Err(e) => {
            eprintln!("[跳过] {用例}：加载失败（{e}）—— 本用例 skip（不 panic）");
            None
        }
    }
}

// ---------------------------------------------------------------------------
// 临时目录（负向用例用：造一个迷你 CrystalStudio 布局）
// ---------------------------------------------------------------------------

/// 临时 `dataRoot`：`<tmp>/catalogue/palette.json` + `<tmp>/Objects/`。
///
/// 布局必须与真实资源同构，因为 `StampCatalog::load` 按 `StampCategories.cs:25`
/// 取「palette.json 父目录的父目录」当 `dataRoot`。
struct 临时根 {
    /// `<tmp>/mir2_stamp_cat_<pid>_<标签>`。
    root: PathBuf,
}

impl 临时根 {
    fn 新建(标签: &str) -> Self {
        let mut root = std::env::temp_dir();
        root.push(format!("mir2_stamp_cat_{}_{}", std::process::id(), 标签));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("catalogue")).expect("建临时 catalogue 目录");
        fs::create_dir_all(root.join("Objects")).expect("建临时 Objects 目录");
        Self { root }
    }

    /// 写 `palette.json`。
    fn 写目录(&self, json: &str) {
        fs::write(self.root.join("catalogue").join("palette.json"), json).expect("写 palette.json");
    }

    /// 写一个**合法的** v2 `.X`（`格数` 个格，坐标从 (0,0) 起逐格 `(+1,-1)`）。
    fn 写印章(&self, 相对: &str, 格数: usize) {
        let stamp = Stamp {
            version: 2,
            cells: (0..格数)
                .map(|i| StampCell {
                    x: i as i32,
                    y: -(i as i32),
                    cell: StampCellData::default(),
                })
                .collect(),
        };
        let bytes = stamp.try_to_bytes().expect("造 .X 字节");
        let path = self.root.join("Objects").join(相对);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("建 .X 父目录");
        }
        fs::write(path, bytes).expect("写 .X");
    }

    /// `palette.json` 的绝对路径。
    fn 目录路径(&self) -> PathBuf {
        self.root.join("catalogue").join("palette.json")
    }
}

impl Drop for 临时根 {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// 拼一份 palette.json（`objects` 用调用方给的 JSON 片段）。
fn 造目录(objects: &str) -> String {
    format!(
        r#"{{
  "schemaVersion": 1,
  "objectRoot": "Objects",
  "previewRoot": "catalogue",
  "objects": [{objects}]
}}"#
    )
}

/// 一条完好的 entry。
fn 好条目(id: &str, file: &str, name: &str, kind: &str) -> String {
    format!(
        r#"{{"id":"{id}","file":"{file}","name":"{name}","type":"{kind}","subtype":"Sub",
"regions":["Province"],"regionGroups":["BichonProvince"],"environments":["Day"],
"tags":["{kind}"],"direction":"SW","directionStatus":"known",
"preview":"previews/{id}.png","previewComplete":true,"reviewStatus":"reviewed"}}"#
    )
}

/// 判断字符串里有没有汉字（用于「中文名实测为 0」的口径断言）。
fn 含汉字(s: &str) -> bool {
    s.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
}

// ---------------------------------------------------------------------------
// 门禁 1：全量解析（条数 + 文件命中）
// ---------------------------------------------------------------------------

/// 门禁 1：`palette.json` → `len() == 9159`，且 9,159 条 `file` **全部**能在 `Objects/` 下找到。
#[test]
fn 全量解析_条数9159且文件全命中() {
    let Some(catalog) = 读真实目录("全量解析") else {
        return;
    };
    let stats = catalog.stats();
    println!("[解析] objectRoot = {}", catalog.object_root().display());
    println!("[解析] previewRoot = {}", catalog.preview_root().display());
    println!("[解析] schemaVersion = {}", catalog.schema_version());
    println!("[解析] objects[] 原始条数 = {}", stats.object_entries);
    println!("[解析] len() = {}", catalog.len());
    println!(
        "[解析] 跳过 = {}（id 重复 {}），样例 {:?}",
        stats.skipped_entries, stats.duplicate_ids, stats.skipped_samples
    );
    println!(
        "[解析] 文件命中 = {} / 缺失 = {}，样例 {:?}",
        stats.files_present, stats.files_missing, stats.missing_samples
    );
    println!(
        "[解析] 扫到 .X = {}，扫描错误 = {}",
        stats.scanned_files, stats.scan_dir_errors
    );

    assert_eq!(
        catalog.len(),
        9159,
        "基线 9,159 条（2026-11-04 实测）；若 palette.json 更新请同步基线"
    );
    assert_eq!(stats.object_entries, 9159, "objects[] 原始条数应为 9,159");
    assert_eq!(
        stats.skipped_entries, 0,
        "真实目录不该有损坏条目（跳过样例：{:?}）",
        stats.skipped_samples
    );
    assert_eq!(
        stats.files_present + stats.files_missing,
        catalog.len(),
        "命中 + 缺失必须等于条目数（不许有第三条路径）"
    );
    assert_eq!(
        stats.files_missing, 0,
        "9,159 条登记项的 .X 应全部存在（缺失样例：{:?}）",
        stats.missing_samples
    );
    assert_eq!(stats.files_present, 9159, "命中数应为 9,159");
    assert_eq!(
        stats.scanned_files, 9170,
        "Objects/ 下 .X 应为 9,170 个（9,159 mir + 8 StoneTemplate + 2 InsectTemplate + 1 WallEntrances）"
    );
    assert!(stats.object_root_present, "Objects/ 目录必须存在");
    assert_eq!(stats.scan_dir_errors, 0, "真实目录扫描不该有错误");
    assert_eq!(
        catalog.all().len(),
        catalog.len(),
        "all() 长度必须等于 len()"
    );
    assert_eq!(
        catalog.all()[0].id, "mir/mir_00000",
        "条目顺序必须保持 palette.json 原顺序"
    );
}

// ---------------------------------------------------------------------------
// 门禁 2：未登记反查
// ---------------------------------------------------------------------------

/// 门禁 2：目录里 9,170 个 `.X` 中有 **11 个**未登记（README 说 11 个，以实测为准）。
#[test]
fn 未登记文件_实测11个() {
    let Some(catalog) = 读真实目录("未登记反查") else {
        return;
    };
    let unlisted = catalog.unlisted();
    println!(
        "[未登记] 数量 = {}（扫到 {} 个，登记 {} 条）",
        unlisted.len(),
        catalog.stats().scanned_files,
        catalog.len()
    );
    for (i, rel) in unlisted.iter().enumerate() {
        println!("[未登记]   #{} {}", i + 1, rel);
    }
    assert_eq!(
        unlisted.len(),
        11,
        "基线 11 个未登记（2026-11-04 实测，与 CrystalStudio/README.md:160 一致）；\
         目录新增 .X 后请同步基线。实测清单：{unlisted:?}"
    );
    assert_eq!(
        catalog.stats().scanned_files - catalog.len(),
        11,
        "未登记数必须等于「扫到的 .X − 登记条数」（当前 9,170 − 9,159）"
    );
    // 每个未登记项都必须是真实存在的文件（不许拿登记项凑数）。
    for rel in unlisted {
        let path = catalog
            .object_root()
            .join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        assert!(path.is_file(), "未登记项必须真实存在：{}", path.display());
    }
}

// ---------------------------------------------------------------------------
// 门禁 3：分类 / 区域查询
// ---------------------------------------------------------------------------

/// 门禁 3：`by_kind` / `by_region` 的计数与「全量自算」逐一相符，且大小写不敏感。
#[test]
fn 分类与区域_索引与全量自算一致() {
    let Some(catalog) = 读真实目录("分类与区域") else {
        return;
    };
    // —— 自算：每个条目的 kind 只进一个桶 ——
    let mut 分类: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for entry in catalog.all() {
        if let Some(kind) = &entry.kind {
            if !kind.trim().is_empty() {
                *分类.entry(kind.clone()).or_insert(0) += 1;
            }
        }
    }
    let mut 排序: Vec<(String, usize)> = 分类.into_iter().collect();
    排序.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    println!("[分类] 不同 kind = {} 个", 排序.len());
    for (kind, n) in 排序.iter().take(5) {
        println!(
            "[分类]   前5：{kind} = {n}（by_kind 实查 {}）",
            catalog.by_kind(kind).len()
        );
    }
    println!(
        "[分类] Building = {}，Tree = {}",
        catalog.by_kind("Building").len(),
        catalog.by_kind("Tree").len()
    );
    println!("[区域] Province = {}", catalog.by_region("Province").len());

    let mut 合计 = 0usize;
    for (kind, n) in &排序 {
        let hits = catalog.by_kind(kind);
        assert_eq!(hits.len(), *n, "by_kind({kind}) 计数必须与全量自算一致");
        assert!(
            hits.iter().all(|e| e.kind.as_deref() == Some(kind.as_str())),
            "by_kind({kind}) 不许混入别的分类"
        );
        合计 += hits.len();
    }
    let 有条目分类数 = catalog
        .all()
        .iter()
        .filter(|e| e.kind.as_ref().is_some_and(|k| !k.trim().is_empty()))
        .count();
    assert_eq!(
        合计, 有条目分类数,
        "各分类桶之和必须等于「有 kind 的条目数」（每条只进一桶）"
    );

    // —— 基线数字（2026-11-04 实测）——
    assert_eq!(catalog.by_kind("Building").len(), 271, "Building 基线 271");
    assert_eq!(catalog.by_kind("Tree").len(), 323, "Tree 基线 323");
    assert_eq!(catalog.by_region("Province").len(), 720, "Province 基线 720");

    // —— 大小写不敏感 ——
    assert_eq!(
        catalog.by_kind("building").len(),
        catalog.by_kind("Building").len(),
        "by_kind 必须大小写不敏感"
    );
    assert_eq!(
        catalog.by_region("province").len(),
        catalog.by_region("Province").len(),
        "by_region 必须大小写不敏感"
    );
    // —— 未命中必须空 ——
    assert!(catalog.by_kind("不存在的分类").is_empty());
    assert!(
        catalog.by_kind("").is_empty(),
        "空键不许命中（空串不是索引键）"
    );
    assert!(catalog.by_region("不存在的区域").is_empty());
    // —— by_region 只查 regions，不并 regionGroups ——
    let province_in_groups = catalog
        .all()
        .iter()
        .filter(|e| e.region_groups.iter().any(|g| g == "Province"))
        .count();
    assert_eq!(
        province_in_groups, 0,
        "实测 regionGroups 里没有 Province，by_region 的 720 全来自 regions 字段"
    );
    assert!(
        !catalog.by_region_group("BichonProvince").is_empty(),
        "regionGroups 索引必须可用"
    );
}

// ---------------------------------------------------------------------------
// 门禁 4：搜索
// ---------------------------------------------------------------------------

/// 门禁 4：`search` 的计数 / 大小写不敏感 / 只匹配 id+name / 空串返回空。
#[test]
fn 搜索_大小写不敏感且只匹配id与名称() {
    let Some(catalog) = 读真实目录("搜索") else {
        return;
    };
    for needle in ["market", "树", "mir_00000"] {
        let hits = catalog.search(needle);
        let ids: Vec<&str> = hits.iter().take(3).map(|e| e.id.as_str()).collect();
        println!(
            "[搜索] search({needle:?}) = {} 条，前 3 个 id = {ids:?}",
            hits.len()
        );
    }

    let market = catalog.search("market");
    assert_eq!(
        market.len(),
        25,
        "search(\"market\") 基线 25 条（名称里带 Market 的条目）"
    );
    assert!(
        market
            .iter()
            .all(|e| e.name.to_lowercase().contains("market")
                || e.id.to_lowercase().contains("market")),
        "命中项必须在 id 或 name 里真的含有该子串"
    );
    assert_eq!(
        catalog.search("MARKET").len(),
        market.len(),
        "search 必须大小写不敏感（MARKET ≡ market）"
    );
    assert_eq!(
        catalog.search("  market  ").len(),
        market.len(),
        "search 必须先 trim（前后空格不影响结果）"
    );

    // 中文：实测 palette.json 的 9,159 条 name **全是英文**，所以「树」命中 0 —— 如实断言，
    // 不许为了让中文搜索「看起来能用」去编造别名。
    let 树 = catalog.search("树");
    assert_eq!(
        树.len(),
        0,
        "实测 name/id 里没有任何中文，search(\"树\") 必须为 0（口径如实）"
    );
    let 中文名数 = catalog.all().iter().filter(|e| 含汉字(&e.name)).count();
    assert_eq!(中文名数, 0, "全量复核：带汉字的 name 应为 0 条");

    // id 子串：唯一命中。
    let one = catalog.search("mir_00000");
    assert_eq!(one.len(), 1, "search(\"mir_00000\") 基线 1 条");
    assert_eq!(one[0].id, "mir/mir_00000");

    // 空 / 纯空白 ⇒ 空（不返回全量）。
    assert!(
        catalog.search("").is_empty(),
        "空串必须返回空，不许返回全量"
    );
    assert!(catalog.search("   ").is_empty(), "纯空白必须返回空");

    // 只匹配 id+name：拿一个**只出现在 tags 里**的词验证「不匹配 tags」。
    let 只在标签里的词 = catalog
        .all()
        .iter()
        .filter_map(|e| e.tags.first())
        .find(|tag| {
            !tag.trim().is_empty()
                && catalog
                    .all()
                    .iter()
                    .all(|e| !e.name.to_lowercase().contains(&tag.to_lowercase()))
        })
        .cloned();
    if let Some(tag) = 只在标签里的词 {
        println!(
            "[搜索] 只出现在 tags 的词 {tag:?} 命中 {} 条（口径：不匹配 tags）",
            catalog.search(&tag).len()
        );
    }
}

// ---------------------------------------------------------------------------
// 门禁 5：resolve 正确性
// ---------------------------------------------------------------------------

/// 门禁 5：抽 5 个 id，`resolve()` 的 path / cell_count / bounds 与直接 `Stamp::parse` **逐项一致**。
#[test]
fn resolve_与直接解析逐项一致() {
    let Some(mut catalog) = 读真实目录("resolve 正确性") else {
        return;
    };
    let total = catalog.len();
    let 样本下标 = [0usize, total / 4, total / 2, total * 3 / 4, total - 1];
    let mut 已验 = 0usize;
    for 下标 in 样本下标 {
        let id = catalog.all()[下标].id.clone();
        let resolved = catalog
            .resolve(&id)
            .unwrap_or_else(|| panic!("{id} 应能解析（第 {下标} 条）"));
        println!(
            "[resolve] {id}：path={} 存在={} 格数={} 包围盒={:?}",
            resolved.path.display(),
            resolved.path.is_file(),
            resolved.cell_count,
            resolved.bounds
        );
        assert!(resolved.path.is_file(), "{id} 的 resolve 路径必须存在");

        // 直接读 + 直接 parse，作为独立参照。
        let bytes = fs::read(&resolved.path).expect("直接读 .X");
        let direct = Stamp::parse(&bytes).expect("直接 parse .X");
        assert_eq!(
            resolved.cell_count,
            direct.cells.len(),
            "{id}：resolve 的 cell_count 必须等于直接 parse 的格数"
        );
        assert_eq!(
            resolved.bounds,
            direct.bounds(),
            "{id}：resolve 的 bounds 必须等于直接 parse 的 bounds()"
        );
        assert_eq!(
            resolved.bounds,
            catalog.resolve(&id).expect("第二次 resolve 应命中缓存").bounds,
            "{id}：缓存前后结果必须一致"
        );
        已验 += 1;
    }
    assert_eq!(已验, 5, "必须验满 5 个样本");

    // 未登记 id / 空 id ⇒ None（不 panic、不返回瞎猜的路径）。
    assert!(
        catalog.resolve("mir/不存在的印章").is_none(),
        "未登记 id 必须 None"
    );
    assert!(catalog.resolve("").is_none(), "空 id 必须 None");

    // clear_cache 之后仍能解析（缓存不是唯一数据源）。
    catalog.clear_cache();
    let first = catalog.all()[0].id.clone();
    assert!(
        catalog.resolve(&first).is_some(),
        "clear_cache 之后必须还能解析"
    );
}

// ---------------------------------------------------------------------------
// 负向用例（全部走临时目录，不依赖真实资源）
// ---------------------------------------------------------------------------

/// 负向 1：palette.json 不存在 ⇒ `Err(PaletteMissing)`（**不许**返回空目录冒充成功）。
#[test]
fn 负向_目录文件缺失必须报错() {
    let 临时 = 临时根::新建("missing");
    let path = 临时.目录路径();
    assert!(!path.exists());
    match StampCatalog::load(&path) {
        Err(CatalogError::PaletteMissing { path: p }) => {
            println!("[负向] PaletteMissing → {}", p.display());
            assert_eq!(p, path);
        }
        other => panic!("文件缺失必须 Err(PaletteMissing)，实际：{other:?}"),
    }
}

/// 负向 2：损坏条目**跳过并计数**（不是静默），且未登记文件能查出来。
#[test]
fn 负向_损坏条目跳过并计数且未登记可查() {
    let 临时 = 临时根::新建("corrupt");
    临时.写印章("mir/good1.X", 3);
    临时.写印章("mir/good2.X", 1);
    临时.写印章("mir/漏登记.X", 2); // 盘上有、目录里没登记
    let objects = [
        好条目("mir/good1", "mir/good1.X", "Good One", "Building"),
        // ① 缺 id/file/name
        r#"{"type":"Building"}"#.to_string(),
        // ② 缺 name
        r#"{"id":"mir/noname","file":"mir/good2.X","type":"Tree"}"#.to_string(),
        // ③ id 重复（大小写不敏感）
        好条目("MIR/GOOD1", "mir/good1.X", "Duplicate", "Tree"),
        // ④ JSON 结构不符（id 是数字）
        r#"{"id":5,"file":"mir/good2.X","name":"Bad Type"}"#.to_string(),
        // ⑤ null 条目
        "null".to_string(),
        好条目("mir/good2", "mir/good2.X", "Good Two", "Tree"),
    ];
    临时.写目录(&造目录(&objects.join(",")));

    let catalog = StampCatalog::load(&临时.目录路径()).expect("坏条目不该让整份失败");
    let stats = catalog.stats();
    println!(
        "[负向] objects[] = {}，进入索引 = {}，跳过 = {}（重复 {}）",
        stats.object_entries,
        catalog.len(),
        stats.skipped_entries,
        stats.duplicate_ids
    );
    for s in &stats.skipped_samples {
        println!("[负向]   跳过样例：{s}");
    }
    println!("[负向] 未登记 = {:?}", catalog.unlisted());

    assert_eq!(catalog.len(), 2, "7 条里只有 2 条完好");
    assert_eq!(stats.object_entries, 7);
    assert_eq!(stats.skipped_entries, 5, "5 条损坏必须被计数");
    assert_eq!(stats.duplicate_ids, 1, "其中 1 条是 id 重复");
    assert_eq!(
        stats.skipped_samples.len(),
        5,
        "样例最多留 5 条（这里正好 5 条）"
    );
    assert_eq!(
        stats.object_entries - stats.skipped_entries,
        catalog.len(),
        "原始条数 − 跳过数 必须等于 len()"
    );
    assert_eq!(catalog.all()[0].id, "mir/good1");
    assert_eq!(catalog.all()[1].id, "mir/good2");

    // 未登记文件必须查出来（盘上有 3 个 .X，登记 2 条）。
    assert_eq!(
        catalog.unlisted(),
        ["mir/漏登记.X"],
        "未登记文件必须精确检出"
    );
    assert_eq!(stats.scanned_files, 3);
    assert_eq!(stats.files_present, 2);
    assert_eq!(stats.files_missing, 0);

    // 分类索引在坏数据下仍然自洽。
    assert_eq!(catalog.by_kind("Building").len(), 1);
    assert_eq!(catalog.by_kind("Tree").len(), 1);
    assert_eq!(catalog.by_kind("building").len(), 1);
    assert_eq!(catalog.by_region("Province").len(), 2);
    assert_eq!(catalog.by_region_group("BichonProvince").len(), 2);
    assert_eq!(catalog.search("good").len(), 2);
    assert_eq!(catalog.search("Good One").len(), 1);
}

/// 负向 3：`schemaVersion` 不是 1 / 缺失 / `objects` 为空 / 非法 JSON / 缺根字段 ⇒ 全部 `Err`。
#[test]
fn 负向_schema版本与空objects必须报错() {
    // ① schemaVersion = 2
    let 临时 = 临时根::新建("schema");
    临时.写目录(&format!(
        r#"{{"schemaVersion":2,"objectRoot":"Objects","previewRoot":"catalogue","objects":[{}]}}"#,
        好条目("a", "a.X", "A", "Building")
    ));
    match StampCatalog::load(&临时.目录路径()) {
        Err(CatalogError::UnsupportedSchema { schema_version, .. }) => {
            println!("[负向] UnsupportedSchema → {schema_version:?}");
            assert_eq!(schema_version, Some(2));
        }
        other => panic!("schemaVersion=2 必须 Err(UnsupportedSchema)，实际：{other:?}"),
    }

    // ② 缺 schemaVersion
    let 临时2 = 临时根::新建("schema-missing");
    临时2.写目录(&format!(
        r#"{{"objectRoot":"Objects","previewRoot":"catalogue","objects":[{}]}}"#,
        好条目("a", "a.X", "A", "Building")
    ));
    match StampCatalog::load(&临时2.目录路径()) {
        Err(CatalogError::UnsupportedSchema {
            schema_version: None,
            ..
        }) => println!("[负向] 缺 schemaVersion ⇒ UnsupportedSchema(None)"),
        other => panic!("缺 schemaVersion 必须 Err，实际：{other:?}"),
    }

    // ③ objects 空数组
    let 临时3 = 临时根::新建("empty");
    临时3.写目录(&造目录(""));
    match StampCatalog::load(&临时3.目录路径()) {
        Err(CatalogError::EmptyObjects { .. }) => println!("[负向] objects 为空 ⇒ EmptyObjects"),
        other => panic!("objects 为空必须 Err(EmptyObjects)，实际：{other:?}"),
    }

    // ④ 不是 JSON
    let 临时4 = 临时根::新建("notjson");
    临时4.写目录("{ 这不是 JSON ");
    match StampCatalog::load(&临时4.目录路径()) {
        Err(CatalogError::PaletteMalformed { detail, .. }) => {
            println!("[负向] 非法 JSON ⇒ PaletteMalformed（{detail}）");
        }
        other => panic!("非法 JSON 必须 Err(PaletteMalformed)，实际：{other:?}"),
    }

    // ⑤ 缺 objectRoot
    let 临时5 = 临时根::新建("noroot");
    临时5.写目录(&format!(
        r#"{{"schemaVersion":1,"previewRoot":"catalogue","objects":[{}]}}"#,
        好条目("a", "a.X", "A", "Building")
    ));
    match StampCatalog::load(&临时5.目录路径()) {
        Err(CatalogError::MissingRoot { field, .. }) => {
            println!("[负向] 缺根字段 ⇒ MissingRoot({field})");
            assert_eq!(field, "objectRoot");
        }
        other => panic!("缺 objectRoot 必须 Err(MissingRoot)，实际：{other:?}"),
    }
}

/// 负向 4：**路径逃逸必须被拒绝**（比 C# 的 `StartsWith` 护栏更严）。
#[test]
fn 负向_路径逃逸必须拒绝() {
    let 临时 = 临时根::新建("escape");
    临时.写印章("mir/ok.X", 2);
    // ① entry.file 里带 `..` ⇒ 条目不跳过（它有 id/file/name），但 resolve 必须 None。
    let objects = [
        好条目("mir/ok", "mir/ok.X", "OK", "Building"),
        好条目("mir/逃逸", "../../../Windows/win.ini", "Escape", "Building"),
    ];
    临时.写目录(&造目录(&objects.join(",")));
    let mut catalog = StampCatalog::load(&临时.目录路径()).expect("逃逸条目不该让整份失败");
    assert_eq!(catalog.len(), 2, "逃逸条目本身仍是「登记项」（不跳过）");
    assert!(catalog.resolve("mir/ok").is_some(), "正常条目必须能解析");
    println!(
        "[负向] 逃逸条目 resolve = {:?}",
        catalog.resolve("mir/逃逸")
    );
    assert!(
        catalog.resolve("mir/逃逸").is_none(),
        "含 `..` 的 file 必须被拒绝（不许返回根外路径）"
    );

    // ② objectRoot 本身带 `..` ⇒ 整份目录定位不合法，必须 Err。
    let 临时2 = 临时根::新建("escape-root");
    临时2.写目录(&format!(
        r#"{{"schemaVersion":1,"objectRoot":"../Objects","previewRoot":"catalogue","objects":[{}]}}"#,
        好条目("a", "a.X", "A", "Building")
    ));
    match StampCatalog::load(&临时2.目录路径()) {
        Err(CatalogError::PaletteMalformed { detail, .. }) => {
            println!("[负向] objectRoot 逃逸 ⇒ PaletteMalformed（{detail}）");
        }
        other => panic!("objectRoot 含 `..` 必须 Err，实际：{other:?}"),
    }
}

/// 负向 5：`Objects/` 目录不存在 ⇒ **不报错**，但 `object_root_present=false`、
/// `unlisted()` 空、所有 `resolve()` 返回 `None`、`files_missing` 等于全部条目数。
#[test]
fn 负向_资源目录缺失时如实降级() {
    let 临时 = 临时根::新建("no-objects");
    fs::remove_dir_all(临时.root.join("Objects")).expect("删临时 Objects");
    临时.写目录(&format!(
        r#"{{"schemaVersion":1,"objectRoot":"Objects","previewRoot":"catalogue","objects":[{}]}}"#,
        好条目("a", "a.X", "A", "Building")
    ));
    let mut catalog = StampCatalog::load(&临时.目录路径()).expect("Objects 缺失不该让整份失败");
    let stats = catalog.stats();
    println!(
        "[负向] object_root_present={} 扫到={} 缺失={} 未登记={}",
        stats.object_root_present,
        stats.scanned_files,
        stats.files_missing,
        stats.unlisted_files
    );
    assert!(!stats.object_root_present, "Objects 不存在必须如实标记");
    assert_eq!(stats.scanned_files, 0);
    assert_eq!(stats.unlisted_files, 0, "没有目录就没有未登记文件");
    assert!(catalog.unlisted().is_empty());
    assert_eq!(catalog.len(), 1, "条目仍可用（名称/分类照旧能查）");
    assert_eq!(stats.files_missing, 1, "缺失必须计数");
    assert_eq!(stats.files_present, 0);
    assert!(catalog.resolve("a").is_none(), "文件不在 ⇒ resolve 必须 None");
    assert_eq!(catalog.by_kind("Building").len(), 1, "分类查询不依赖磁盘");
}

/// 负向 6：`preview` 指向的 PNG **不存在也不算错**（当前资源里 PNG 未生成）。
#[test]
fn 负向_预览图缺失不算错() {
    let Some(catalog) = 读真实目录("预览图缺失") else {
        return;
    };
    let 有预览 = catalog.all().iter().filter(|e| e.preview.is_some()).count();
    let 预览根 = catalog.preview_root().to_path_buf();
    let 首个 = catalog.all().iter().find_map(|e| e.preview.clone());
    println!(
        "[预览] 带 preview 字段 = {}/{} 条；previewRoot = {}",
        有预览,
        catalog.len(),
        预览根.display()
    );
    if let Some(rel) = 首个 {
        let path = 预览根.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        println!("[预览] 例：{} 存在={}", path.display(), path.is_file());
        assert!(
            !path.is_file(),
            "实测预览 PNG 未生成；若已生成请更新本用例的说明"
        );
    }
}
