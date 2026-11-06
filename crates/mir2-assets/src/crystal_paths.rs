//! 原版 CrystalStudio `.lib` 的路径发现与三族登记表（对齐 `Assets.cs:342-405@901dbe2f`）。
//!
//! # 一、上游怎么登记 `.lib`（逐行）
//!
//! `AssetStore.Register`（`Assets.cs:355-376@901dbe2f`）先定「基础目录」：
//!
//! ```text
//! map = root/Map 存在 ? root/Map : root        // Assets.cs:360
//! ```
//!
//! 然后用**固定表**逐条 `Add(id, map, 相对路径)`，`Add` 只有「文件存在才登记」一个条件
//! （`Assets.cs:354`），不存在的文件静默跳过。三族对应的部分：
//!
//! | 格式 | 族 | 上游写法 | 索引 |
//! |---|---|---|---|
//! | `WemadeMir2` | 地面 | `WemadeMir2\Tiles.lib` | `0` |
//! | | 中层 | `WemadeMir2\Smtiles.lib` | `0` |
//! | | 表层 | `WemadeMir2\Objects.lib` + `Objects2..98.lib` | `0` + `1..=97` |
//! | `ShandaMir2` | 地面 | `ShandaMir2\Tiles.lib` + `Tiles2..10.lib` | `0..=9` |
//! | | 中层 | `ShandaMir2\SmTiles.lib` + `SmTiles2..10.lib` | `0..=9` |
//! | | 表层 | `ShandaMir2\Objects.lib` + `Objects2..31.lib` | `0..=30` |
//! | `Flat` | 地面 | `Tiles.lib` | `0` |
//! | | 中层 | `SmTiles.lib` | `0` |
//! | | 表层 | `Objects.lib` + `Objects2..98.lib` | `0` + `1..=97` |
//!
//! 三处**容易写错的地方**，这里逐条照抄上游：
//!
//! 1. `WemadeMir2` 的编号层只有 `Objects`，**没有** `Tiles{K}` / `Smtiles{K}` ——
//!    因为上游那两行 `Add` 不循环（`Assets.cs:361`）。
//! 2. 表层编号的上限是 `i < 99`（`Assets.cs:362`），即 `Objects2..=Objects98` ⇒
//!    索引 `1..=97`。**不是** `Objects99`。
//! 3. `Flat` 兜底（`Assets.cs:374`）同样只给表层做编号；且上游是「一个库都没登记到」
//!    才走兜底，这里对应 [`CrystalAssetFormat::Auto`] 的最后一步。
//!
//! 编号 → 索引的换算沿用 [`crate::LibRef`] 的既有口径：`index = 0` 是基名本身，
//! `index > 0` 对应 `{Stem}{index+1}`。所以 `Objects2.lib` = `surface(1)`、
//! `Tiles10.lib` = `ground(9)`。
//!
//! # 二、三种格式为什么必须择一
//!
//! 上游把所有库登记进**一个扁平 ID 表**（`0..99` 是 WemadeMir2、`100..199` 是 ShandaMir2、
//! `200+` 是 Mir3…），所以同一个 `Tiles.lib` 在两张表里是两个不同 ID。
//! 本 crate 的定位键是 [`LibRef`]（族 + 族内索引），**没有**扁平号那一维：
//! `ground(0)` 只能指向一个文件。若同时登记 WemadeMir2 与 ShandaMir2，
//! `ground(0)` 就会变成「谁后写谁赢」的静默二选一。
//!
//! ⇒ 本表按 [`CrystalAssetFormat`] **择一**登记。要看另一套库，换个格式再 `discover` 一次。
//! [`CrystalAssetFormat::Auto`] 按 `WemadeMir2 → ShandaMir2 → Flat` 顺序探测，
//! 第一个非空者胜出；三个都空则返回**空表**（[`CrystalLibPaths::is_empty`]），不报错、不 panic ——
//! 「扫描事实」与「配置错误」分开，由调用方决定（[`crate::CrystalLibSource::open`] 会把空表当错误）。
//!
//! # 三、大小写与规则外文件
//!
//! Windows 文件系统大小写不敏感，上游一律用 `File.Exists` 直接拼串。本实现按
//! **大小写不敏感**匹配基名与扩展名，因此 `smtiles.lib`、`SMTILES.LIB` 都能登记。
//!
//! 规则外的文件**不猜映射**，逐条记进 [`CrystalLibPaths::skipped`]：
//!
//! | 原因 | 例子 |
//! |---|---|
//! | [`CrystalSkipReason::OutOfRule`] | `Tiles1.lib`（上游永不生成 `{Stem}1`）、`Objects0.lib`、`Tiles2c.lib` |
//! | [`CrystalSkipReason::OutOfRange`] | `WemadeMir2/Tiles5.lib`（该格式只登记基名）、`ShandaMir2/Tiles11.lib` |
//! | [`CrystalSkipReason::NotInThreeFamilies`] | `AniTiles1.lib`（上游另有扁平 ID 190）、Mir3 库、其它命名 |
//!
//! # 四、未覆盖（本单不做，后续单）
//!
//! - **Mir3**：`WemadeMir3\{wood,sand,snow,forest,none}\{Tilesc,Tiles30c,…}.lib`（ID `200+`）与
//!   `ShandaMir3\{names}{states}.lib`（ID `300+`），`Assets.cs:366-372@901dbe2f`；
//! - **完整扁平 AssetStore ID 表**：`AniTiles1 = 190`、`Tilesc/Housesc/Dungeonsc…`、
//!   Sanxiu 的 `TryMapId`（`Tiles=0 / SmTiles=1000 / Objects=2000`，`Assets.cs:378-404`）；
//! - `DataPaths.ResolveLibraryFolder(root)`（`Assets.cs:357`）的解析规则**待验证**，
//!   本实现只做上游紧接着那一步的 `root/Map` 判定（`Assets.cs:360`）。

use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::error::AssetError;
use crate::lib_paths::{Family, LibRef};

/// 原版 `.lib` 的目录格式（= 上游登记表的哪一段）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CrystalAssetFormat {
    /// 自动探测：`WemadeMir2 → ShandaMir2 → Flat`，取第一个非空的。
    #[default]
    Auto,
    /// `WemadeMir2\` 子目录（上游 ID `0..99`，`Assets.cs:361-362@901dbe2f`）。
    WemadeMir2,
    /// `ShandaMir2\` 子目录（上游 ID `100..199`，`Assets.cs:363-365@901dbe2f`）。
    ShandaMir2,
    /// 平铺在基础目录下（上游兜底，`Assets.cs:374@901dbe2f`）。
    Flat,
}

impl CrystalAssetFormat {
    /// [`CrystalAssetFormat::Auto`] 的探测顺序。
    pub const PROBE_ORDER: [CrystalAssetFormat; 3] = [
        CrystalAssetFormat::WemadeMir2,
        CrystalAssetFormat::ShandaMir2,
        CrystalAssetFormat::Flat,
    ];

    /// 该格式的库文件所在子目录名；`Flat` 无子目录（返回 `None`）。
    pub fn subdir(self) -> Option<&'static str> {
        match self {
            CrystalAssetFormat::Auto => None,
            CrystalAssetFormat::WemadeMir2 => Some("WemadeMir2"),
            CrystalAssetFormat::ShandaMir2 => Some("ShandaMir2"),
            CrystalAssetFormat::Flat => None,
        }
    }

    /// 中文名（日志与汇报用）。
    pub fn label(self) -> &'static str {
        match self {
            CrystalAssetFormat::Auto => "自动探测",
            CrystalAssetFormat::WemadeMir2 => "WemadeMir2",
            CrystalAssetFormat::ShandaMir2 => "ShandaMir2",
            CrystalAssetFormat::Flat => "平铺",
        }
    }
}

impl fmt::Display for CrystalAssetFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// 一条「基名 → 族 + 允许的最大后缀号」规则。
///
/// `stem` 照抄上游写法（`Smtiles` / `SmTiles` 的 T 大小写上游就不统一），
/// 实际匹配**大小写不敏感**。
struct Rule {
    /// 目标族。
    family: Family,
    /// 基名（不含 `.lib`）。
    stem: &'static str,
    /// 允许的最大后缀号 `K`；`0` 表示只登记基名本身。
    ///
    /// `K >= 2` 时索引为 `K - 1`（`K == 1` 在上游永不生成，按规则外处理）。
    max_number: u32,
}

/// `WemadeMir2\` 的三条规则（`Assets.cs:361-362@901dbe2f`）。
const WEMADE_RULES: [Rule; 3] = [
    Rule {
        family: Family::Ground,
        stem: "Tiles",
        max_number: 0,
    },
    Rule {
        family: Family::Middle,
        stem: "Smtiles",
        max_number: 0,
    },
    Rule {
        family: Family::Surface,
        stem: "Objects",
        max_number: 98,
    },
];

/// `ShandaMir2\` 的三条规则（`Assets.cs:363-365@901dbe2f`）。
const SHANDA_RULES: [Rule; 3] = [
    Rule {
        family: Family::Ground,
        stem: "Tiles",
        max_number: 10,
    },
    Rule {
        family: Family::Middle,
        stem: "SmTiles",
        max_number: 10,
    },
    Rule {
        family: Family::Surface,
        stem: "Objects",
        max_number: 31,
    },
];

/// 平铺兜底的三条规则（`Assets.cs:374@901dbe2f`）。
const FLAT_RULES: [Rule; 3] = [
    Rule {
        family: Family::Ground,
        stem: "Tiles",
        max_number: 0,
    },
    Rule {
        family: Family::Middle,
        stem: "SmTiles",
        max_number: 0,
    },
    Rule {
        family: Family::Surface,
        stem: "Objects",
        max_number: 98,
    },
];

/// 取某格式的规则表。
fn rules(format: CrystalAssetFormat) -> &'static [Rule] {
    match format {
        // `Auto` 不会走到分类（探测时已解析成具体格式）；给空表做防御。
        CrystalAssetFormat::Auto => &[],
        CrystalAssetFormat::WemadeMir2 => &WEMADE_RULES,
        CrystalAssetFormat::ShandaMir2 => &SHANDA_RULES,
        CrystalAssetFormat::Flat => &FLAT_RULES,
    }
}

/// 一个基名的归类结果。
enum Classified {
    /// 命中：族 + 族内索引。
    Mapped(Family, u16),
    /// 未命中：记原因。
    Skipped(CrystalSkipReason),
}

/// 扫描时被跳过的文件及原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrystalSkipReason {
    /// 基名或后缀不符合三族命名规则（含上游永不生成的 `{Stem}1`、非数字后缀）。
    OutOfRule,
    /// 后缀序号本身合规（`>= 2`），但超出该格式登记的序号上限。
    ///
    /// 例：`WemadeMir2/Tiles5.lib`（该格式地面只登记基名）、`ShandaMir2/Tiles11.lib`（上限 10）。
    OutOfRange,
    /// 基名不属于本单三族规则 —— 上游可能以**扁平 ID**登记（`AniTiles1` = 190、Mir3 库、
    /// Sanxiu 的 `Tiles/SmTiles/Objects` 编号表等），本单不映射（见模块文档 §四）。
    NotInThreeFamilies,
}

impl fmt::Display for CrystalSkipReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CrystalSkipReason::OutOfRule => {
                f.write_str("基名/后缀不符合 Tiles/SmTiles/Objects 命名规则")
            }
            CrystalSkipReason::OutOfRange => f.write_str("后缀序号超出该格式的登记上限"),
            CrystalSkipReason::NotInThreeFamilies => {
                f.write_str("不属于本单三族规则（上游可能以扁平 ID 登记，未覆盖）")
            }
        }
    }
}

/// 一条跳过记录（路径 + 原因）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrystalSkipped {
    /// 被跳过的 `.lib` 文件路径。
    pub path: PathBuf,
    /// 跳过原因。
    pub reason: CrystalSkipReason,
}

/// 登记表里的一条库信息（[`CrystalLibPaths::iter`] 的产出项）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrystalLibInfo<'a> {
    /// 定位键。
    pub lib: LibRef,
    /// `.lib` 文件路径。
    pub path: &'a Path,
}

impl CrystalLibInfo<'_> {
    /// 文件基名（不含扩展名，如 `Objects21`）。
    pub fn stem(&self) -> Option<&str> {
        self.path.file_stem().and_then(|s| s.to_str())
    }
}

/// `.lib` 定位表：三个族各一张 `族内索引 → 文件路径` 的映射。
///
/// 由 [`CrystalLibPaths::discover`] 扫描得到；规则外文件连原因一起留着
/// （[`CrystalLibPaths::skipped`]），不做静默丢弃。
#[derive(Debug, Clone)]
pub struct CrystalLibPaths {
    /// 传入的扫描根。
    root: PathBuf,
    /// 实际生效的基础目录（`root/Map` 或 `root`，见 `Assets.cs:360@901dbe2f`）。
    map_dir: PathBuf,
    /// 实际扫描的目录（`map_dir` 或 `map_dir/<格式子目录>`）。
    scan_dir: PathBuf,
    /// 调用方要求的格式（可能仍为 [`CrystalAssetFormat::Auto`]）。
    format: CrystalAssetFormat,
    /// 实际使用的格式；空表时为 `None`（探测没命中）。
    resolved: Option<CrystalAssetFormat>,
    /// 下标 = [`Family::index`]。
    tables: [BTreeMap<u16, PathBuf>; 3],
    /// 规则外文件。
    skipped: Vec<CrystalSkipped>,
    /// 同一（族, 索引）被更早候选占用而忽略的文件。
    duplicates: Vec<PathBuf>,
}

impl CrystalLibPaths {
    /// 扫描给定根目录并按 `format` 登记 `.lib` 库。
    ///
    /// 基础目录取 `root/Map`（存在时）否则 `root`，与 `Assets.cs:360@901dbe2f` 一致；
    /// 具体格式再往下走一层子目录（`Flat` 不离层）。
    ///
    /// 错误只有两种：根不是目录 → [`AssetError::RootNotDir`]；目录读不了 → [`AssetError::Io`]。
    /// 「一个库都没找到」返回 `Ok`（[`CrystalLibPaths::is_empty`]）——
    /// 这是扫描事实，是否当错误由调用方决定（[`crate::CrystalLibSource::open`] 会当错误）。
    pub fn discover(root: &Path, format: CrystalAssetFormat) -> Result<Self, AssetError> {
        if !root.is_dir() {
            return Err(AssetError::RootNotDir {
                path: root.to_path_buf(),
            });
        }
        let map = root.join("Map");
        let map_dir = if map.is_dir() {
            map
        } else {
            root.to_path_buf()
        };
        match format {
            CrystalAssetFormat::Auto => {
                let mut probed: Vec<Self> =
                    Vec::with_capacity(CrystalAssetFormat::PROBE_ORDER.len());
                for candidate in CrystalAssetFormat::PROBE_ORDER {
                    let paths = Self::scan(root, &map_dir, format, candidate)?;
                    if !paths.is_empty() {
                        return Ok(paths);
                    }
                    probed.push(paths);
                }
                // 三种格式都没登记到任何库：返回空表（明确的扫描事实，不是错误），
                // 但把三个目录里的跳过记录都带上 —— 否则「为什么一个库都没找到」
                // 这条最要紧的诊断信息就丢了。
                let mut empty = Self::empty(root, &map_dir, format);
                for p in probed {
                    empty.skipped.extend(p.skipped);
                    empty.duplicates.extend(p.duplicates);
                }
                Ok(empty)
            }
            concrete => Self::scan(root, &map_dir, format, concrete),
        }
    }

    /// 扫描某一具体格式（私有）。
    fn scan(
        root: &Path,
        map_dir: &Path,
        configured: CrystalAssetFormat,
        concrete: CrystalAssetFormat,
    ) -> Result<Self, AssetError> {
        let scan_dir = match concrete.subdir() {
            Some(sub) => map_dir.join(sub),
            None => map_dir.to_path_buf(),
        };
        let mut tables: [BTreeMap<u16, PathBuf>; 3] = [BTreeMap::new(), BTreeMap::new(), BTreeMap::new()];
        let mut skipped: Vec<CrystalSkipped> = Vec::new();
        let mut duplicates: Vec<PathBuf> = Vec::new();

        // 目录不存在 = 该格式没有库（上游 `File.Exists` 逐条判存在，等价于全跳过）。
        if scan_dir.is_dir() {
            for path in read_dir_sorted(&scan_dir)? {
                if !path.is_file() {
                    continue;
                }
                let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                let Some(stem) = strip_lib_ext(name) else {
                    // 非 `.lib` 文件不在本表职责内，直接忽略（不记为 skip）。
                    continue;
                };
                match classify(concrete, stem) {
                    Classified::Mapped(family, index) => {
                        match tables[family.index()].entry(index) {
                            Entry::Vacant(slot) => {
                                slot.insert(path);
                            }
                            // Windows 下同名不可能重复；大小写敏感的文件系统上才可能。
                            Entry::Occupied(_) => duplicates.push(path),
                        }
                    }
                    Classified::Skipped(reason) => skipped.push(CrystalSkipped { path, reason }),
                }
            }
        }

        Ok(Self {
            root: root.to_path_buf(),
            map_dir: map_dir.to_path_buf(),
            scan_dir,
            format: configured,
            resolved: Some(concrete),
            tables,
            skipped,
            duplicates,
        })
    }

    /// 空表（三种格式都没探测到库）。
    fn empty(root: &Path, map_dir: &Path, format: CrystalAssetFormat) -> Self {
        Self {
            root: root.to_path_buf(),
            map_dir: map_dir.to_path_buf(),
            scan_dir: map_dir.to_path_buf(),
            format,
            resolved: None,
            tables: [BTreeMap::new(), BTreeMap::new(), BTreeMap::new()],
            skipped: Vec::new(),
            duplicates: Vec::new(),
        }
    }

    /// 扫描根（[`CrystalLibPaths::discover`] 的入参）。
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 实际生效的基础目录（`root/Map` 或 `root`）。
    pub fn map_dir(&self) -> &Path {
        &self.map_dir
    }

    /// 实际扫描的目录（格式化子目录已拼上）。
    pub fn scan_dir(&self) -> &Path {
        &self.scan_dir
    }

    /// 调用方要求的格式（可能仍是 [`CrystalAssetFormat::Auto`]）。
    pub fn format(&self) -> CrystalAssetFormat {
        self.format
    }

    /// 实际使用的格式；空表 / 尚未探测出结果时为 `None`。
    pub fn resolved_format(&self) -> Option<CrystalAssetFormat> {
        self.resolved
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

    /// 按定位键取 `.lib` 路径。
    ///
    /// **同索引跨族不互通**：`resolve(LibRef::ground(0))` 是 `Tiles.lib`，
    /// `resolve(LibRef::surface(0))` 是 `Objects.lib`；某族没登记该索引时返回 `None`，
    /// **不会**回落到别的族或别的格式去找。
    pub fn resolve(&self, r: LibRef) -> Option<&Path> {
        self.tables[r.family.index()].get(&r.index).map(|p| p.as_path())
    }

    /// 按文件基名（大小写不敏感，如 `Objects21`）反查定位键。
    ///
    /// 主要用于工具/诊断，不要在渲染主路径上用它。
    pub fn find_stem(&self, stem: &str) -> Option<LibRef> {
        for family in Family::ALL {
            for (index, path) in &self.tables[family.index()] {
                let hit = path
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
    pub fn iter(&self) -> impl Iterator<Item = CrystalLibInfo<'_>> + '_ {
        Family::ALL.into_iter().flat_map(move |family| {
            self.tables[family.index()]
                .iter()
                .map(move |(index, path)| CrystalLibInfo {
                    lib: LibRef {
                        family,
                        index: *index,
                    },
                    path: path.as_path(),
                })
        })
    }

    /// 某族已登记的索引列表（升序）。
    pub fn indices(&self, family: Family) -> Vec<u16> {
        self.tables[family.index()].keys().copied().collect()
    }

    /// 扫描时被跳过的文件（含原因）。
    pub fn skipped(&self) -> &[CrystalSkipped] {
        &self.skipped
    }

    /// 因「同族同索引已被更早的候选占用」而被忽略的文件。
    pub fn duplicates(&self) -> &[PathBuf] {
        &self.duplicates
    }
}

/// 把一个 `.lib` 基名归类到「族 + 族内索引」，或给出跳过原因。
fn classify(format: CrystalAssetFormat, stem: &str) -> Classified {
    for rule in rules(format) {
        let Some(rest) = strip_prefix_ci(stem, rule.stem) else {
            continue;
        };
        if rest.is_empty() {
            // 基名本身 ⇒ 索引 0。
            return Classified::Mapped(rule.family, 0);
        }
        if rest.len() > 5 || !rest.bytes().all(|b| b.is_ascii_digit()) {
            return Classified::Skipped(CrystalSkipReason::OutOfRule);
        }
        let Ok(k) = rest.parse::<u32>() else {
            return Classified::Skipped(CrystalSkipReason::OutOfRule);
        };
        // 上游只会生成 `{Stem}{i+1}`（i > 0）⇒ `{Stem}0` / `{Stem}1` 都不在规则内。
        if k < 2 {
            return Classified::Skipped(CrystalSkipReason::OutOfRule);
        }
        if k > rule.max_number {
            return Classified::Skipped(CrystalSkipReason::OutOfRange);
        }
        return Classified::Mapped(rule.family, (k - 1) as u16);
    }
    Classified::Skipped(CrystalSkipReason::NotInThreeFamilies)
}

/// 大小写不敏感地去掉前缀。
fn strip_prefix_ci<'a>(hay: &'a str, prefix: &str) -> Option<&'a str> {
    if hay.len() < prefix.len() {
        return None;
    }
    let (head, tail) = hay.split_at(prefix.len());
    if head.eq_ignore_ascii_case(prefix) {
        Some(tail)
    } else {
        None
    }
}

/// 拆出 `.lib` 的基名（扩展名大小写不敏感）；其它扩展名返回 `None`。
fn strip_lib_ext(name: &str) -> Option<&str> {
    let (stem, ext) = name.rsplit_once('.')?;
    if stem.is_empty() || !ext.eq_ignore_ascii_case("lib") {
        return None;
    }
    Some(stem)
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

    /// 三族基名与编号换算（**WemadeMir2** 口径：地面/中层只登记基名）。
    #[test]
    fn 分类_wemade口径() {
        use CrystalAssetFormat::WemadeMir2 as F;
        assert!(matches!(
            classify(F, "Tiles"),
            Classified::Mapped(Family::Ground, 0)
        ));
        assert!(matches!(
            classify(F, "Smtiles"),
            Classified::Mapped(Family::Middle, 0)
        ));
        assert!(matches!(
            classify(F, "Objects"),
            Classified::Mapped(Family::Surface, 0)
        ));
        // Objects2 ⇒ surface(1)；Objects98 ⇒ surface(97)（上游 i < 99）
        assert!(matches!(
            classify(F, "Objects2"),
            Classified::Mapped(Family::Surface, 1)
        ));
        assert!(matches!(
            classify(F, "Objects98"),
            Classified::Mapped(Family::Surface, 97)
        ));
        // 规则外 / 超范围
        assert!(matches!(
            classify(F, "Tiles1"),
            Classified::Skipped(CrystalSkipReason::OutOfRule)
        ));
        assert!(matches!(
            classify(F, "Tiles5"),
            Classified::Skipped(CrystalSkipReason::OutOfRange)
        ));
        assert!(matches!(
            classify(F, "Objects99"),
            Classified::Skipped(CrystalSkipReason::OutOfRange)
        ));
        assert!(matches!(
            classify(F, "AniTiles1"),
            Classified::Skipped(CrystalSkipReason::NotInThreeFamilies)
        ));
    }

    /// **ShandaMir2** 口径：三个族都有编号，上限 10 / 10 / 31。
    #[test]
    fn 分类_shanda口径() {
        use CrystalAssetFormat::ShandaMir2 as F;
        assert!(matches!(
            classify(F, "Tiles10"),
            Classified::Mapped(Family::Ground, 9)
        ));
        assert!(matches!(
            classify(F, "SmTiles2"),
            Classified::Mapped(Family::Middle, 1)
        ));
        assert!(matches!(
            classify(F, "Objects31"),
            Classified::Mapped(Family::Surface, 30)
        ));
        assert!(matches!(
            classify(F, "Tiles11"),
            Classified::Skipped(CrystalSkipReason::OutOfRange)
        ));
        assert!(matches!(
            classify(F, "Objects32"),
            Classified::Skipped(CrystalSkipReason::OutOfRange)
        ));
    }

    /// 大小写不敏感 + 扩展名过滤。
    #[test]
    fn 基名与扩展名大小写不敏感() {
        assert_eq!(strip_lib_ext("Tiles.LIB"), Some("Tiles"));
        assert_eq!(strip_lib_ext("smtiles.lib"), Some("smtiles"));
        assert_eq!(strip_lib_ext("Tiles.wil"), None);
        assert_eq!(strip_lib_ext(".lib"), None);
        assert_eq!(strip_lib_ext("noext"), None);
        assert!(matches!(
            classify(CrystalAssetFormat::Flat, "TILES"),
            Classified::Mapped(Family::Ground, 0)
        ));
        assert!(matches!(
            classify(CrystalAssetFormat::Flat, "sMtIlEs5"),
            Classified::Skipped(CrystalSkipReason::OutOfRange)
        ));
    }

    /// 根不存在必须是错误，不是空表。
    #[test]
    fn 不存在的根返回错误() {
        let err =
            CrystalLibPaths::discover(Path::new(r"E:\__不存在的_lib_根__"), CrystalAssetFormat::Auto)
                .unwrap_err();
        assert!(matches!(err, AssetError::RootNotDir { .. }), "{err:?}");
    }
}
