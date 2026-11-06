//! 图库定位表（[`LibPaths`]）：把「族 + 族内索引」映射到磁盘文件。
//!
//! # 一、库号 → 文件规则（与客户端 C++ 逐字对齐，非猜测）
//!
//! 客户端 `Classes/common/MShare.cpp:827-873` 为三个图库数组逐个 `sprintf` 文件名：
//! `i == 0` 用基名常量，`i > 0` 用 `%d` 模板并传入 `i + 1`；宏定义见
//! `Classes/common/Share.h:36-41`（`OBJECTIMAGEFILE`/`OBJECTIMAGEFILE1`、
//! `TITLESIMAGEFILE`/`TITLESIMAGEFILE1`、`SMLTITLESIMAGEFILE`/`SMLTITLESIMAGEFILE1`）。
//! ⇒ 地图格里的三个图库字段（原值即 [`LibRef::index`]）对应：
//!
//! | 族（[`Family`]） | 地图格字段 | `index = 0` | `index > 0` |
//! |----|-----------|---------|---------|
//! | [`Family::Ground`]（地面） | `btTiles` | `Tiles` | `Tiles{index+1}` |
//! | [`Family::Middle`]（中层） | `btsmTiles` | `SmTiles` | `SmTiles{index+1}` |
//! | [`Family::Surface`]（表层） | `btArea` | `Objects` | `Objects{index+1}` |
//!
//! 三点必须写死：
//!
//! 1. **只认基名，不认扩展名**。宏里写的是 `.wil`，本机客户端目录（`23周年` 那套）实际是
//!    `.wzl`；两种容器都收（`wzl+wzx`、`wil+wix`），登记时以数据文件扩展名决定解码器。
//! 2. 后缀数字 `K` 必须满足「`K == 0`（即无后缀）」或「`K >= 2`」。`K == 1` 在 C++ 里
//!    **永远不会被生成**（`i=1` 生成的是 `{Stem}2`），故按规则外处理并记录，不做猜测映射。
//! 3. **三个族是三个互不相通的命名空间**。`btTiles` 与 `btArea` 都可能取 0..几，含义完全不同；
//!    因此定位键是二元组 [`LibRef`]（族 + 索引），**不存在能把三族压平的单一整数号**。
//!
//! # 二、为什么不用扁平号
//!
//! 早先版本用过一个「全局稠密号」（按族分块压成一个 `u16`）。它会把三族塞进同一整数空间：
//! 调用方只要把 `btArea` 的值当扁平号传进来，就会**静默拿到 `Tiles` 的某一页** —— 错得很安静。
//! 现在的 [`LibRef`] 让「哪个族」成为类型的一部分，跨族误传直接 `None`。

use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::path::{Path, PathBuf};

use crate::error::AssetError;

/// 扫描深度上限（`root` 记为 0 层）。
///
/// 实测两套客户端目录的库文件位置：`<client>/data/`（1 层，`23周年` 那套，628 对）
/// 与 `<client>/Resources/data/`（2 层，`reference_full` 那套，`.wil`）。
/// 取 2 层即可全部覆盖；再往下扩只会把无关目录卷进来。
const MAX_SCAN_DEPTH: usize = 2;

/// 图库族：与地图格的三个图库字段一一对应。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Family {
    /// 地面层（地图格 `btTiles`）→ 文件基名 `Tiles` / `Tiles{index+1}`。
    Ground,
    /// 中层（地图格 `btsmTiles`）→ 文件基名 `SmTiles` / `SmTiles{index+1}`。
    Middle,
    /// 表层（地图格 `btArea`）→ 文件基名 `Objects` / `Objects{index+1}`。
    Surface,
}

impl Family {
    /// 三个族的规范顺序（[`LibPaths::iter`] 的输出顺序）。
    pub const ALL: [Family; 3] = [Family::Ground, Family::Middle, Family::Surface];

    /// 文件基名（不含序号），与 C++ 宏里 `%d` 前的部分一致。
    pub fn stem(self) -> &'static str {
        match self {
            Family::Ground => "Tiles",
            Family::Middle => "SmTiles",
            Family::Surface => "Objects",
        }
    }

    /// 基名的小写形式（用于大小写不敏感匹配；客户端目录里 `smtiles5.wzl` 就是小写）。
    fn stem_lower(self) -> &'static str {
        match self {
            Family::Ground => "tiles",
            Family::Middle => "smtiles",
            Family::Surface => "objects",
        }
    }

    /// 中文名（日志与汇报用）。
    pub fn label(self) -> &'static str {
        match self {
            Family::Ground => "地面",
            Family::Middle => "中层",
            Family::Surface => "表层",
        }
    }

    /// 族在 [`Family::ALL`] 中的下标（内部数组索引）。
    pub fn index(self) -> usize {
        match self {
            Family::Ground => 0,
            Family::Middle => 1,
            Family::Surface => 2,
        }
    }
}

impl fmt::Display for Family {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// 图库定位键：**族 + 族内索引**（`index` 就是地图格 `btTiles`/`btsmTiles`/`btArea` 的原值）。
///
/// 三族互不相通：`LibRef::ground(0)`（`Tiles`）与 `LibRef::surface(0)`（`Objects`）
/// 是两个不同的库，绝不可互换。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LibRef {
    /// 所属族。
    pub family: Family,
    /// 族内 0-based 索引（= 地图格字段原值；`N > 0` 时对应文件 `{Stem}{N+1}`）。
    pub index: u16,
}

impl LibRef {
    /// 构造地面层定位键（`btTiles`）。
    pub const fn ground(index: u16) -> Self {
        LibRef {
            family: Family::Ground,
            index,
        }
    }

    /// 构造中层定位键（`btsmTiles`）。
    pub const fn middle(index: u16) -> Self {
        LibRef {
            family: Family::Middle,
            index,
        }
    }

    /// 构造表层定位键（`btArea`）。
    pub const fn surface(index: u16) -> Self {
        LibRef {
            family: Family::Surface,
            index,
        }
    }
}

impl fmt::Display for LibRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}#{}", self.family.label(), self.index)
    }
}

/// 一个已登记的图库：数据文件 + 索引文件（必须成对）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibFile {
    /// 数据文件（`.wzl` / `.wil`）。
    pub data: PathBuf,
    /// 索引文件（`.wzx` / `.wix`）。
    pub index: PathBuf,
}

/// 扫描时被跳过的文件及原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// 只有数据文件或只有索引文件，配不成一对，无法解码。
    IncompletePair,
    /// 基名后缀序号不符合 §一 规则（只允许无后缀或 `{Stem}{N+1}`，`N > 0`）。
    OutOfRule,
}

impl fmt::Display for SkipReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SkipReason::IncompletePair => f.write_str("数据文件与索引文件不成对"),
            SkipReason::OutOfRule => {
                f.write_str("基名后缀序号不符合 Tiles/SmTiles/Objects 命名规则")
            }
        }
    }
}

/// 一条跳过记录（路径 + 原因）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    /// 被跳过的文件路径。
    pub path: PathBuf,
    /// 跳过原因。
    pub reason: SkipReason,
}

/// 登记表里的一条库信息（[`LibPaths::iter`] 的产出项）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibInfo<'a> {
    /// 定位键。
    pub lib: LibRef,
    /// 文件对（数据 + 索引）。
    pub file: &'a LibFile,
}

impl LibInfo<'_> {
    /// 文件基名（不含扩展名，如 `Objects21`）。
    pub fn stem(&self) -> Option<&str> {
        self.file.data.file_stem().and_then(|s| s.to_str())
    }
}

/// 图库定位表：三个族各一张 `index → 文件对` 的映射。
///
/// 由 [`LibPaths::discover`] 扫描客户端目录得到；只登记「数据文件 + 索引文件都在」的库，
/// 被跳过的文件连原因一起留着（[`LibPaths::skipped`]），不做静默丢弃。
#[derive(Debug, Clone)]
pub struct LibPaths {
    root: PathBuf,
    /// 下标 = [`Family::index`]。
    tables: [BTreeMap<u16, LibFile>; 3],
    skipped: Vec<Skipped>,
    duplicates: Vec<PathBuf>,
}

/// 扫描中发现的一对候选文件（同一个小写基名下的数据/索引槽位）。
#[derive(Debug, Default)]
struct Pending {
    data: Option<PathBuf>,
    index: Option<PathBuf>,
}

impl LibPaths {
    /// 扫描给定根目录，按 §一 的命名规则登记可用的库文件。
    ///
    /// 扫描范围：`client_root` 及其至多 2 层子目录，**广度优先、同层按路径字典序**，
    /// 同一（族, 索引）先到先得，后到的记入 [`LibPaths::duplicates`]。
    ///
    /// 错误：根不存在/不是目录 → [`AssetError::RootNotDir`]；目录读不了 → [`AssetError::Io`]。
    /// 「一个库都没找到」返回 `Ok`（`available() == 0`），这是扫描事实而非失败 ——
    /// 由调用方决定是否当错误处理（例如 [`crate::WilSource::open`] 就会当错误）。
    pub fn discover(client_root: &Path) -> Result<Self, AssetError> {
        if !client_root.is_dir() {
            return Err(AssetError::RootNotDir {
                path: client_root.to_path_buf(),
            });
        }

        // 1) 收集待扫目录（广度优先、每层排序），并逐目录登记数据/索引槽位。
        let dirs = walk_dirs(client_root, MAX_SCAN_DEPTH)?;
        let mut pending: BTreeMap<String, Pending> = BTreeMap::new();
        let mut duplicates: Vec<PathBuf> = Vec::new();
        let mut unreckoned: Vec<PathBuf> = Vec::new();

        for dir in &dirs {
            for path in read_dir_sorted(dir)? {
                if !path.is_file() {
                    continue;
                }
                let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                let Some((stem, ext)) = split_name(name) else {
                    // 连「基名.扩展名」都拆不出来（如本机 `data\.wzl` 这种空基名文件）——
                    // 记下来而不是默默丢掉。
                    unreckoned.push(path);
                    continue;
                };
                let is_data = match ext.to_ascii_lowercase().as_str() {
                    "wzl" | "wil" => true,
                    "wzx" | "wix" => false,
                    // 其余扩展名（.bmp/.png/...）直接忽略：不在本 crate 职责内。
                    _ => continue,
                };
                let slot = pending.entry(stem.to_ascii_lowercase()).or_default();
                let target = if is_data {
                    &mut slot.data
                } else {
                    &mut slot.index
                };
                if target.is_none() {
                    *target = Some(path);
                } else if target.as_deref() != Some(path.as_path()) {
                    // 同一槽位已有更早的候选（如 `Tiles.wil` 与 `Tiles.wzl` 并存）→ 记下来。
                    duplicates.push(path);
                }
            }
        }

        // 2) 配对 + 归类。
        let mut classified: Vec<(Family, u16, LibFile)> = Vec::new();
        let mut skipped: Vec<Skipped> = unreckoned
            .into_iter()
            .map(|path| Skipped {
                path,
                reason: SkipReason::OutOfRule,
            })
            .collect();

        for (key, p) in &pending {
            let (data, index) = match (&p.data, &p.index) {
                (Some(d), Some(i)) => (d.clone(), i.clone()),
                _ => {
                    if let Some(d) = &p.data {
                        skipped.push(Skipped {
                            path: d.clone(),
                            reason: SkipReason::IncompletePair,
                        });
                    }
                    if let Some(i) = &p.index {
                        skipped.push(Skipped {
                            path: i.clone(),
                            reason: SkipReason::IncompletePair,
                        });
                    }
                    continue;
                }
            };
            match classify_stem(key) {
                Some((family, number)) => {
                    classified.push((family, number, LibFile { data, index }))
                }
                None => skipped.push(Skipped {
                    // 基名同一、扩展名只差一个字母，索引文件不再重复记一条（没有额外信息）。
                    path: data,
                    reason: SkipReason::OutOfRule,
                }),
            }
        }

        // 3) 填表：族内按索引升序（BTreeMap 天然有序）；同键冲突记重复。
        classified.sort_by_key(|(family, number, _)| (family.index(), *number));
        let mut tables = [BTreeMap::new(), BTreeMap::new(), BTreeMap::new()];
        for (family, number, file) in classified {
            match tables[family.index()].get(&number) {
                // 理论上不可达（同一小写基名只会有一条），留作防御。
                Some(_) => duplicates.push(file.data),
                None => {
                    tables[family.index()].insert(number, file);
                }
            }
        }

        Ok(Self {
            root: client_root.to_path_buf(),
            tables,
            skipped,
            duplicates,
        })
    }

    /// 扫描根（[`LibPaths::discover`] 的入参）。
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 已登记的库总数（三个族合计）。
    pub fn available(&self) -> usize {
        self.tables.iter().map(|t| t.len()).sum()
    }

    /// 某族已登记的库数。
    pub fn available_in(&self, family: Family) -> usize {
        self.tables[family.index()].len()
    }

    /// 全表是否为空。
    pub fn is_empty(&self) -> bool {
        self.available() == 0
    }

    /// 按定位键取数据文件路径（`.wzl` / `.wil`）。
    ///
    /// **同索引跨族不互通**：`resolve(LibRef::ground(0))` 是 `Tiles`，
    /// `resolve(LibRef::surface(0))` 是 `Objects`；某个族没登记该索引时返回 `None`，
    /// **不会**回落到别的族去找。
    pub fn resolve(&self, r: LibRef) -> Option<&Path> {
        self.resolve_lib(r).map(|f| f.data.as_path())
    }

    /// 按定位键取「数据 + 索引」文件对。
    ///
    /// 语义同 [`LibPaths::resolve`]：只在本族内查，跨族不互通。
    pub fn resolve_lib(&self, r: LibRef) -> Option<&LibFile> {
        self.tables[r.family.index()].get(&r.index)
    }

    /// 按文件基名（大小写不敏感，如 `Objects21`）反查定位键。
    ///
    /// 主要用于工具/诊断把「基名」还原成「族 + 索引」，不要在渲染主路径上用它。
    pub fn find_stem(&self, stem: &str) -> Option<LibRef> {
        for family in Family::ALL {
            for (index, file) in &self.tables[family.index()] {
                let hit = file
                    .data
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| s.eq_ignore_ascii_case(stem));
                if hit {
                    return Some(LibRef {
                        family,
                        index: *index,
                    });
                }
            }
        }
        None
    }

    /// 遍历全部登记项（族按 [`Family::ALL`] 顺序，族内按索引升序）。
    pub fn iter(&self) -> impl Iterator<Item = LibInfo<'_>> + '_ {
        Family::ALL.into_iter().flat_map(move |family| {
            self.tables[family.index()]
                .iter()
                .map(move |(index, file)| LibInfo {
                    lib: LibRef {
                        family,
                        index: *index,
                    },
                    file,
                })
        })
    }

    /// 某族已登记的索引列表（升序）。
    pub fn indices(&self, family: Family) -> Vec<u16> {
        self.tables[family.index()].keys().copied().collect()
    }

    /// 扫描时被跳过的文件（含原因）。
    pub fn skipped(&self) -> &[Skipped] {
        &self.skipped
    }

    /// 因「同族同索引已被更早的候选占用」而被忽略的文件。
    pub fn duplicates(&self) -> &[PathBuf] {
        &self.duplicates
    }
}

/// 把基名（小写）归类到「族 + 0-based 索引」；不符合规则返回 `None`。
fn classify_stem(stem_lower: &str) -> Option<(Family, u16)> {
    // 按基名长度降序匹配：`smtiles` 必须先于 `tiles`，否则前缀会互相抢。
    for family in [Family::Middle, Family::Surface, Family::Ground] {
        let prefix = family.stem_lower();
        let Some(rest) = stem_lower.strip_prefix(prefix) else {
            continue;
        };
        if rest.is_empty() {
            return Some((family, 0));
        }
        if !rest.bytes().all(|b| b.is_ascii_digit()) || rest.len() > 5 {
            return None;
        }
        let k: u32 = rest.parse().ok()?;
        // C++ 只会生成 `i + 1`（i>0）；`k == 1` 与 `k == 0` 都不在规则内。
        let number = k.checked_sub(1)?;
        if number == 0 || number > u32::from(u16::MAX) {
            return None;
        }
        return Some((family, number as u16));
    }
    None
}

/// 拆出「基名 + 扩展名」（扩展名不含点）；无扩展名返回 `None`。
fn split_name(name: &str) -> Option<(&str, &str)> {
    let (stem, ext) = name.rsplit_once('.')?;
    if stem.is_empty() || ext.is_empty() {
        return None;
    }
    Some((stem, ext))
}

/// 广度优先收集 `root` 及至多 `max_depth` 层子目录（每层按路径排序，结果确定）。
fn walk_dirs(root: &Path, max_depth: usize) -> Result<Vec<PathBuf>, AssetError> {
    let mut out = Vec::new();
    let mut queue: VecDeque<(PathBuf, usize)> = VecDeque::new();
    queue.push_back((root.to_path_buf(), 0));

    while let Some((dir, depth)) = queue.pop_front() {
        out.push(dir.clone());
        if depth >= max_depth {
            continue;
        }
        for path in read_dir_sorted(&dir)? {
            if path.is_dir() {
                queue.push_back((path, depth + 1));
            }
        }
    }
    Ok(out)
}

/// 读目录并按路径排序（保证扫描结果与文件系统返回顺序无关）。
fn read_dir_sorted(dir: &Path) -> Result<Vec<PathBuf>, AssetError> {
    let entries = std::fs::read_dir(dir).map_err(|e| AssetError::io(dir, e))?;
    let mut out = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| AssetError::io(dir, e))?;
        out.push(entry.path());
    }
    out.sort();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 归类遵守n加一规则() {
        assert_eq!(classify_stem("tiles"), Some((Family::Ground, 0)));
        assert_eq!(classify_stem("tiles2"), Some((Family::Ground, 1)));
        assert_eq!(classify_stem("tiles21"), Some((Family::Ground, 20)));
        assert_eq!(classify_stem("smtiles"), Some((Family::Middle, 0)));
        assert_eq!(classify_stem("smtiles5"), Some((Family::Middle, 4)));
        assert_eq!(classify_stem("objects"), Some((Family::Surface, 0)));
        assert_eq!(classify_stem("objects21"), Some((Family::Surface, 20)));
    }

    #[test]
    fn 规则外基名一律拒绝() {
        // 后缀 = 1 在 C++ 里永远不会生成
        assert_eq!(classify_stem("tiles1"), None);
        // 数字 0 也不是合法后缀
        assert_eq!(classify_stem("tiles0"), None);
        // 非纯数字后缀
        assert_eq!(classify_stem("objects518_ef"), None);
        assert_eq!(classify_stem("objects51effect4"), None);
        // 完全不相干的基名
        assert_eq!(classify_stem("hum"), None);
        assert_eq!(classify_stem(""), None);
        // 大小写不敏感
        assert_eq!(classify_stem("smtiles10"), Some((Family::Middle, 9)));
    }

    #[test]
    fn 基名拆分() {
        assert_eq!(split_name("Tiles.wzl"), Some(("Tiles", "wzl")));
        assert_eq!(split_name("a.b.wix"), Some(("a.b", "wix")));
        assert_eq!(split_name("noext"), None);
        assert_eq!(split_name(".wzl"), None);
        assert_eq!(split_name("x."), None);
    }

    #[test]
    fn 不存在的根返回错误而不是空表() {
        let err = LibPaths::discover(Path::new(r"E:\__不存在的目录__")).unwrap_err();
        assert!(matches!(err, AssetError::RootNotDir { .. }), "{err:?}");
    }

    #[test]
    fn 空目录返回空表() {
        let dir = std::env::temp_dir().join("mir2_assets_empty_dir_probe");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建临时目录");
        let paths = LibPaths::discover(&dir).expect("空目录应返回空表");
        assert_eq!(paths.available(), 0);
        assert!(paths.is_empty());
        assert!(paths.resolve(LibRef::ground(0)).is_none());
        assert!(paths.skipped().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 按族分组登记且族内升序() {
        use std::fs::File;

        let dir = std::env::temp_dir().join("mir2_assets_family_order_probe");
        let _ = std::fs::remove_dir_all(&dir);
        let sub = dir.join("data");
        std::fs::create_dir_all(&sub).expect("建临时目录");

        // 造 4 对空壳文件：Tiles / Tiles3 / SmTiles / Objects（内容不重要，只看登记）
        for stem in ["Tiles", "Tiles3", "SmTiles", "Objects"] {
            File::create(sub.join(format!("{stem}.wzl"))).expect("造数据文件");
            File::create(sub.join(format!("{stem}.wzx"))).expect("造索引文件");
        }
        // 一条不成对的、两条规则外的
        File::create(sub.join("Hum.wil")).expect("造孤立文件");
        File::create(sub.join("Objects518_ef.wzl")).expect("造规则外文件");
        File::create(sub.join("Objects518_ef.wzx")).expect("造规则外文件");

        let paths = LibPaths::discover(&dir).expect("扫描应成功");
        let list: Vec<LibRef> = paths.iter().map(|i| i.lib).collect();
        assert_eq!(
            list,
            vec![
                LibRef::ground(0),  // Tiles
                LibRef::ground(2),  // Tiles3（N=2 ⇒ Tiles{N+1}）
                LibRef::middle(0),  // SmTiles
                LibRef::surface(0), // Objects
            ]
        );
        assert_eq!(paths.available(), 4);
        assert_eq!(paths.available_in(Family::Ground), 2);
        let expect_tiles = sub.join("Tiles.wzl");
        assert_eq!(
            paths.resolve(LibRef::ground(0)),
            Some(expect_tiles.as_path())
        );
        assert_eq!(paths.find_stem("tiles3"), Some(LibRef::ground(2)));
        assert_eq!(paths.indices(Family::Ground), vec![0, 2]);
        assert!(paths
            .skipped()
            .iter()
            .any(|s| s.reason == SkipReason::IncompletePair));
        assert!(paths
            .skipped()
            .iter()
            .any(|s| s.reason == SkipReason::OutOfRule));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 同索引跨族不互通() {
        use std::fs::File;

        let dir = std::env::temp_dir().join("mir2_assets_cross_family_probe");
        let _ = std::fs::remove_dir_all(&dir);
        let sub = dir.join("data");
        std::fs::create_dir_all(&sub).expect("建临时目录");
        // 只放地面族：Tiles / Tiles5
        for stem in ["Tiles", "Tiles5"] {
            File::create(sub.join(format!("{stem}.wzl"))).expect("造数据文件");
            File::create(sub.join(format!("{stem}.wzx"))).expect("造索引文件");
        }

        let paths = LibPaths::discover(&dir).expect("扫描应成功");

        // N+1 规则：index=4 ⇒ Tiles5
        assert_eq!(paths.find_stem("Tiles"), Some(LibRef::ground(0)));
        assert_eq!(paths.find_stem("Tiles5"), Some(LibRef::ground(4)));
        let t5 = sub.join("Tiles5.wzl");
        assert_eq!(paths.resolve(LibRef::ground(4)), Some(t5.as_path()));

        // 同一个 index 在别的族里必须落空，绝不回落到地面族
        assert!(
            paths.resolve(LibRef::middle(4)).is_none(),
            "中层 4 号不存在，不许回落"
        );
        assert!(
            paths.resolve(LibRef::surface(4)).is_none(),
            "表层 4 号不存在，不许回落"
        );
        assert!(paths.resolve(LibRef::middle(0)).is_none());
        assert!(paths.resolve(LibRef::surface(0)).is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
