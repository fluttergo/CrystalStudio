//! 印章**目录索引**：把 `catalogue/palette.json` 的分类记录与 `Objects/` 下的 `.X` 文件对起来，
//! 产出「按分类 / 区域 / 名称查询 + 按 id 解析真实文件」的索引（GUI「印章」页签的数据底座）。
//!
//! 真值来源（**只读**，本模块不写 `CrystalStudio/` 任何文件）：
//! - `<dataRoot>/catalogue/palette.json`（5,092,886 B / 9,159 条 `objects[]`）；
//! - `<dataRoot>/Objects/**/*.X`（9,170 个，全部 v2）；
//!   （`<dataRoot>` 是开发机上的 CrystalStudio 数据根，由 `MIR2_STAMP_PALETTE` / 挂载决定，
//!   公开仓不内置该路径）
//! - CrystalStudio 的读取口径：`CrystalStudio/StampCategories.cs`（`Read` 在 `:20-38`、
//!   `Resolve` 在 `:40-46`、`Groups` 在 `:61-67`、`Matches` 在 `:69-72`）。
//!
//! # 一、索引口径（**每条都可在验收里复算**）
//!
//! 1. **一条 `objects[]` = 一条 [`StampEntry`]**，顺序**原样保留 palette.json 的顺序**
//!    （不排序、不去重、不合并、不按类型重排）——GUI 网格的默认顺序即此顺序。
//! 2. [`StampCatalog::len`] = **成功进入索引的条目数** = `objects[]` 条数 − 跳过数。
//!    实测 `9,159 = 9,159 − 0`（`stats().object_entries` / `stats().skipped_entries` 可逐项对账）。
//! 3. JSON 的 `type` 字段在 Rust 里叫 [`StampEntry::kind`]（`type` 是关键字）；
//!    `"type": null` ⇒ `kind: None`。**空串不归一化成 `None`**（本 crate 的纪律是「不猜语义」），
//!    但**空串/纯空白不会成为索引键**（`by_kind("")` 恒为空）。
//! 4. `by_kind` / `by_region` / `by_region_group` 的**查询键与索引键都做 `to_lowercase()`**
//!    ⇒ `Building` ≡ `building`；返回顺序 = palette.json 顺序；返回的是**引用**，不复制条目。
//! 5. `by_region` 只匹配 JSON 的 `regions`，`by_region_group` 只匹配 `regionGroups` ——
//!    **两个字段分开查**。CrystalStudio 的 Region 页签是两者并集（`StampCategories.cs:63`
//!    `Regions.Concat(RegionGroups).Distinct()`），GUI 若要同口径请自行合并两次查询结果；
//!    本库不隐式并集，避免「`by_region` 到底匹配了哪个字段」说不清。
//! 6. [`StampCatalog::search`] 只匹配 **`id` 与 `name`** 的子串（大小写不敏感，`str::to_lowercase`）。
//!    **不匹配** `file` / `kind` / `subtype` / `regions` / `tags` —— 那是 CrystalStudio
//!    `StampCategories.Matches`（`:69-72`）的更宽口径，本库**不照抄**，免得 GUI 搜索框的行为
//!    被隐式放大到「搜类型名也能搜出东西」。要宽口径请自己组合 `by_kind` 等接口。
//!    空串/纯空白 `needle` **返回空**（不返回全量），防止空搜索框把 9,159 条一次画出来。
//! 7. **路径口径照抄 CrystalStudio**：`dataRoot` = `palette.json` 的**父目录的父目录**
//!    （`StampCategories.cs:25` 的 `Path.GetDirectoryName(Path.GetDirectoryName(...))`），
//!    `objectRoot` / `previewRoot` 相对 `dataRoot` 解析（`:26`）。
//!    ⇒ 真实布局：`dataRoot = <CrystalStudio 数据根>`，`objectRoot = <dataRoot>/Objects`，
//!    `previewRoot = <dataRoot>/catalogue`，条目路径 = `objectRoot` + `file`（`file` 形如 `mir/mir_00000.X`）。
//! 8. **逃逸护栏比 C# 更严**：C# 的 `Resolve`（`:40-46`）用 `StartsWith` 判前缀，允许
//!    `a/../b` 这种「绕一圈仍落在根内」的写法；本库**直接拒绝任何 `..` / 绝对路径 / 盘符**，
//!    因为合法的 catalogue 里不该出现 `..`（宁可 `None` 也不给越界路径）。
//! 9. [`StampCatalog::unlisted`] = `Objects/` 下**真实存在、但 palette.json 没登记**的 `.X`，
//!    输出为**相对 `objectRoot` 的路径**（`/` 分隔、保留磁盘大小写、已排序）；
//!    比对时两边都 `\\`→`/` + `to_lowercase()`（Windows 大小写不敏感，避免 `Mir/x.X` 误判成漏登记）。
//! 10. **存在性判定复用目录扫描结果**（相对路径集合），**不逐条 `stat`**：
//!     9,159 条逐个 `is_file()` 在 Windows 上要多花几十毫秒，而目录扫描本来就要做
//!     （`unlisted()` 需要全量文件表），两件事共用一份结果，口径也天然一致。
//!
//! # 二、缺失 / 损坏怎么算（**不许静默**）
//!
//! | 情况 | 处置 | 落在哪 |
//! |------|------|--------|
//! | `palette.json` 不存在 | **`Err(PaletteMissing)`** | —— |
//! | `palette.json` 读不出（权限 / 是目录 / IO 错） | **`Err(PaletteUnreadable)`** | —— |
//! | 不是合法 JSON | **`Err(PaletteMalformed)`** | —— |
//! | `schemaVersion` 缺失或 `!= 1` | **`Err(UnsupportedSchema)`** | —— |
//! | 缺 `objectRoot` / `previewRoot` | **`Err(MissingRoot)`** | —— |
//! | `objects` 缺失或为空数组 | **`Err(EmptyObjects)`** | —— |
//! | **单条** entry 损坏（JSON 结构不符 / 缺 `id`·`file`·`name` / `name` 全空白 / `id` 重复） | **跳过 + 计数** | `stats().skipped_entries`（重复 id 另计 `duplicate_ids`），前 5 条样例进 `stats().skipped_samples` |
//! | `file` 指向的 `.X` 不在盘上 | **不跳过、不报错**：条目仍可用，只是 `resolve()` 返回 `None` | `stats().files_missing` + 前 5 条 `stats().missing_samples` |
//! | `Objects/` 目录本身不存在 | **不报错**：`stats().object_root_present = false`，`unlisted()` 为空，所有 `resolve()` 返回 `None` | —— |
//! | 目录扫描时某个子目录/目录项读不出 | 跳过并计数 | `stats().scan_dir_errors` |
//! | `resolve()` 时文件读失败 / `.X` 解析失败 | 返回 `None` 并计数 | `stats().resolve_read_errors` / `resolve_parse_errors` |
//!
//! 「`file` 缺失不跳过」是**故意的**：GUI 要能显示「这条登记了但文件没了」，让它从列表里消失
//! 才是真正的静默丢数据。反过来，`id`/`file`/`name` 缺失的条目无法被 GUI 使用（没有主键、
//! 没有显示名），留着只会让下游到处 `if`，所以跳过——但**一定计数**。
//!
//! # 三、`resolve()` 的缓存
//!
//! [`StampCatalog::resolve`] 是 `&mut self`：解析结果（含 `None`）按 **id 小写**缓存在
//! `HashMap` 里，**同一条 id 只读一次盘**。代价是「缓存了 `None` 之后，把 `.X` 补到盘上
//! 也不会自动看见」——所以提供 [`StampCatalog::clear_cache`]，GUI 在「导入新印章」后调一次即可。
//!
//! # 四、用法
//!
//! ```no_run
//! use std::path::Path;
//! use mir2_stamp::catalogue::StampCatalog;
//!
//! let mut catalog = StampCatalog::load(Path::new(r"<dataRoot>/catalogue/palette.json"))?;
//! println!("登记 {} 条，未登记文件 {} 个", catalog.len(), catalog.unlisted().len());
//!
//! for entry in catalog.by_kind("Building").iter().take(3) {
//!     println!("{} {} {:?}", entry.id, entry.name, entry.subtype);
//! }
//!
//! let first = catalog.all()[0].id.clone();
//! if let Some(resolved) = catalog.resolve(&first) {
//!     println!("{} 格，包围盒 {:?}", resolved.cell_count, resolved.bounds);
//! }
//! # Ok::<(), mir2_stamp::catalogue::CatalogError>(())
//! ```
//!
//! # 五、实测基线（2026-11-04 21:4x，本机真实资源；换机请用 `MIR2_STAMP_PALETTE` 覆盖后复测）
//!
//! | 项 | 实测 |
//! |----|------|
//! | `objects[]` 原始条数 / `len()` / 跳过 | 9,159 / 9,159 / 0（重复 id 0） |
//! | `file` 命中 / 缺失 | 9,159 / 0 |
//! | `Objects/` 扫到 `.X` | 9,170（mir 9,159 + StoneTemplate 8 + InsectTemplate 2 + WallEntrances 1） |
//! | `unlisted()` | **11**（与 `CrystalStudio/README.md:160` 的「11 个未登记」一致） |
//! | 不同 `kind` 数 / 前 5 | 19 / Terrain 1,695 · Cliff 1,434 · Wall 1,152 · Rock 943 · Debris 759 |
//! | `by_kind("Building")` / `by_kind("Tree")` | 271 / 323 |
//! | `by_region("Province")` | 720（`regionGroups` 里没有任何 `Province`） |
//! | `search("market")` / `search("树")` / `search("mir_00000")` | 25 / **0** / 1 |
//! | `load()`（release，5 次实测，页缓存已命中） | **48 ~ 83 ms**（进程内首跑 48/50/63/70/83，第二次同量级） |
//! | `search("market")` 单次（1,000 次取最小 / 平均） | **0.070 ms** / 0.08 ~ 0.12 ms（平均值受同机其它进程干扰） |
//!
//! ⚠ 「冷/热」在本机**拉不开差距**：`palette.json` 只有 5 MB，跑过一次就进了 Windows 页缓存，
//! 而**丢弃页缓存需要提权工具**（RAMMap / `EmptyStandbyList`），本机没有 ⇒
//! 上表只能算「页缓存命中下的重复测量」，**不是真正的冷盘读数**（真冷盘请在同机重启后立刻跑一次探针）。
//!
//! ⚠ `search("树")` 命中 **0** 是**如实结果**：9,159 条 `name` **全是英文**（实测含汉字的 `name` = 0 条）。
//! 想让中文能搜，得先有中文名字段（本库不编别名，也不做中英映射）。
//!
//! 本模块**不硬编码**任何统计值，只硬编码**格式常量**（`schemaVersion == 1`、根目录两跳）；
//! 上表由 `examples/catalog_probe.rs` 与 `catalogue_tests` 每次实跑复现。

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

use crate::stamp::Stamp;

/// 本库认的 `schemaVersion`（CrystalStudio `StampCategories.cs:27` 的 `SchemaVersion != 1` 判据）。
const SUPPORTED_SCHEMA_VERSION: u64 = 1;

/// 跳过/缺失样例最多留几条（够定位问题，又不至于把 9,000 条全塞进统计里）。
const SAMPLE_LIMIT: usize = 5;

// ---------------------------------------------------------------------------
// palette.json 的原始形状（只取本库需要的字段，其余字段与未知字段一律忽略）
// ---------------------------------------------------------------------------

/// `palette.json` 顶层。未知字段**不拒绝**（README 明确「保留已有字段和未知字段」）。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPalette {
    /// `schemaVersion`（缺失 ⇒ `None`，随后按「不支持」报错）。
    #[serde(default)]
    schema_version: Option<u64>,
    /// `objectRoot`（相对 `dataRoot`）。
    #[serde(default)]
    object_root: Option<String>,
    /// `previewRoot`（相对 `dataRoot`）。
    #[serde(default)]
    preview_root: Option<String>,
    /// `objects[]`。**刻意存成 `Box<RawValue>`**（原始 JSON 切片，不建 DOM 树）：
    /// 这样单条 entry 的结构错误可以在下面**逐条**捕获并计数，而不是让一条坏数据
    /// 把整个 5 MB 文件判成非法；同时避免为 9,159 条各建一棵 `serde_json::Value`。
    #[serde(default)]
    objects: Option<Vec<Box<serde_json::value::RawValue>>>,
}

/// `objects[]` 的单条原始 entry。**全部字段可缺**，缺了在 [`StampCatalog::load`] 里判「损坏」。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawEntry {
    /// `id`（如 `mir/mir_00000`），主键。
    #[serde(default)]
    id: Option<String>,
    /// `file`（相对 `objectRoot`，如 `mir/mir_00000.X`）。
    #[serde(default)]
    file: Option<String>,
    /// `name`（显示名）。
    #[serde(default)]
    name: Option<String>,
    /// `type`（Rust 侧改名 `kind`）。
    #[serde(default, rename = "type")]
    kind: Option<String>,
    /// `subtype`。
    #[serde(default)]
    subtype: Option<String>,
    /// `regions`。
    #[serde(default)]
    regions: Option<Vec<String>>,
    /// `regionGroups`。
    #[serde(default)]
    region_groups: Option<Vec<String>>,
    /// `environments`。
    #[serde(default)]
    environments: Option<Vec<String>>,
    /// `tags`。
    #[serde(default)]
    tags: Option<Vec<String>>,
    /// `direction`。
    #[serde(default)]
    direction: Option<String>,
    /// `reviewStatus`。
    #[serde(default)]
    review_status: Option<String>,
    /// `preview`（相对 `previewRoot`）。
    #[serde(default)]
    preview: Option<String>,
}

// ---------------------------------------------------------------------------
// 对外数据结构
// ---------------------------------------------------------------------------

/// palette.json `objects[]` 里的一条登记项（字段与 JSON **一一对应**，只改了 `type`→`kind` 这个名字）。
///
/// 刻意**没有**带 `directionStatus` 与 `previewComplete`：本库只承载「查询索引」需要的字段，
/// 这两个是 CrystalStudio 的展示/统计辅助位（`StampCategories.cs:10` 的 `DirectionStatus` /
/// `PreviewComplete`），GUI 真要用再加，避免现在凭空定语义。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StampEntry {
    /// 主键（JSON `id`，如 `mir/mir_00000`）。
    pub id: String,
    /// 相对 `objectRoot` 的文件路径（JSON `file`，如 `mir/mir_00000.X`，`/` 分隔）。
    pub file: String,
    /// 显示名（JSON `name`）。
    pub name: String,
    /// JSON `type`（Rust 关键字冲突故改名）。`null` ⇒ `None`。
    pub kind: Option<String>,
    /// JSON `subtype`。
    pub subtype: Option<String>,
    /// JSON `regions`（地图区域，如 `Province`）。
    pub regions: Vec<String>,
    /// JSON `regionGroups`（区域组，如 `BichonProvince`）。
    pub region_groups: Vec<String>,
    /// JSON `environments`。
    pub environments: Vec<String>,
    /// JSON `tags`。
    pub tags: Vec<String>,
    /// JSON `direction`（如 `SW`）。
    pub direction: Option<String>,
    /// JSON `reviewStatus`。
    pub review_status: Option<String>,
    /// JSON `preview`（相对 `previewRoot` 的 PNG 路径；**当前资源里 PNG 未生成**，见模块文档）。
    pub preview: Option<String>,
}

/// [`StampCatalog::resolve`] 的结果：真实文件路径 + 解析出来的几何信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedStamp {
    /// `.X` 的**绝对**路径（已过逃逸护栏，一定落在 `objectRoot` 内）。
    pub path: PathBuf,
    /// 包围盒 `(min_x, min_y, max_x, max_y)`，**闭区间**，与 [`Stamp::bounds`] 完全同源。
    pub bounds: (i32, i32, i32, i32),
    /// 格数（`Stamp::cells.len()`，与 [`Stamp::parse`] 直接读出的格数一致）。
    pub cell_count: usize,
}

/// [`StampCatalog::load`] 的对账统计。**所有「跳过 / 缺失」都必须在这里看得见。**
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CatalogStats {
    /// `objects[]` 的**原始条数**（不管好坏）。
    pub object_entries: usize,
    /// 因损坏被跳过的条数（缺 `id`/`file`/`name`、`name` 全空白、JSON 结构不符、`id` 重复）。
    pub skipped_entries: usize,
    /// 其中因 `id` 重复被跳过的条数（**同时计入** `skipped_entries`：它确实没进索引）。
    pub duplicate_ids: usize,
    /// 跳过样例（最多 [`SAMPLE_LIMIT`] 条，含 `objects[下标]` 便于回查）。
    pub skipped_samples: Vec<String>,
    /// `Objects/` 下扫到的 `.X` 个数（目录不存在 ⇒ 0）。
    pub scanned_files: usize,
    /// `file` 能在磁盘上找到的条数。
    pub files_present: usize,
    /// `file` 在磁盘上找不到的条数（**不跳过条目**，只计数）。
    pub files_missing: usize,
    /// 缺失样例（最多 [`SAMPLE_LIMIT`] 条）。
    pub missing_samples: Vec<String>,
    /// 未登记文件数（= `unlisted().len()`）。
    pub unlisted_files: usize,
    /// `objectRoot` 目录是否存在。
    pub object_root_present: bool,
    /// 扫描目录时读不出的子目录/目录项个数。
    pub scan_dir_errors: usize,
    /// `resolve()` 时文件读失败次数（累计，含缓存前）。
    pub resolve_read_errors: usize,
    /// `resolve()` 时 `.X` 解析失败次数（累计）。
    pub resolve_parse_errors: usize,
}

/// [`StampCatalog::load`] 的失败原因。**每一种都是「整份目录不可用」，不做降级冒充成功。**
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogError {
    /// `palette.json` 不存在。
    PaletteMissing {
        /// 传入的路径。
        path: PathBuf,
    },
    /// `palette.json` 存在但读不出（权限 / 是目录 / IO 错）。
    PaletteUnreadable {
        /// 传入的路径。
        path: PathBuf,
        /// 底层错误文本（`std::io::Error::to_string()`；不存 `io::Error` 以便本枚举可 `Clone`/`Eq`）。
        detail: String,
    },
    /// `palette.json` 不是合法 JSON，或顶层结构不符（含「路径层数不足，定位不到 `dataRoot`」）。
    PaletteMalformed {
        /// 传入的路径。
        path: PathBuf,
        /// 出错位置与原因（serde_json 的原文，含行列号）。
        detail: String,
    },
    /// `schemaVersion` 缺失或不是 [`SUPPORTED_SCHEMA_VERSION`]。
    UnsupportedSchema {
        /// 传入的路径。
        path: PathBuf,
        /// 文件里读到的值（缺失 ⇒ `None`）。
        schema_version: Option<u64>,
    },
    /// 缺 `objectRoot` 或 `previewRoot`（无法定位 `.X` 与预览图，整份目录没有意义）。
    MissingRoot {
        /// 传入的路径。
        path: PathBuf,
        /// 缺的字段名（`objectRoot` / `previewRoot`）。
        field: &'static str,
    },
    /// `objects` 字段缺失或为空数组。
    EmptyObjects {
        /// 传入的路径。
        path: PathBuf,
    },
}

impl fmt::Display for CatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CatalogError::PaletteMissing { path } => {
                write!(f, "印章目录索引文件不存在：{}", path.display())
            }
            CatalogError::PaletteUnreadable { path, detail } => {
                write!(f, "印章目录索引文件读不出：{}（{detail}）", path.display())
            }
            CatalogError::PaletteMalformed { path, detail } => {
                write!(f, "印章目录索引不是合法 JSON：{}（{detail}）", path.display())
            }
            CatalogError::UnsupportedSchema {
                path,
                schema_version,
            } => match schema_version {
                Some(v) => write!(
                    f,
                    "印章目录索引的 schemaVersion = {v}，本库只认 {SUPPORTED_SCHEMA_VERSION}：{}",
                    path.display()
                ),
                None => write!(
                    f,
                    "印章目录索引缺少 schemaVersion（本库只认 {SUPPORTED_SCHEMA_VERSION}）：{}",
                    path.display()
                ),
            },
            CatalogError::MissingRoot { path, field } => write!(
                f,
                "印章目录索引缺少 {field} 字段，无法定位资源根目录：{}",
                path.display()
            ),
            CatalogError::EmptyObjects { path } => write!(
                f,
                "印章目录索引的 objects 为空（缺失或空数组）：{}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for CatalogError {}

// ---------------------------------------------------------------------------
// 索引本体
// ---------------------------------------------------------------------------

/// 印章目录索引：`palette.json` 的登记项 + `Objects/` 的磁盘事实 + 查询/解析缓存。
///
/// 构造只走 [`StampCatalog::load`]；内部索引（id→下标、分类→下标、区域→下标、搜索键）
/// 全部在 `load` 里一次建好，查询是纯内存操作。
#[derive(Debug)]
pub struct StampCatalog {
    /// 全部条目，**保持 palette.json 顺序**。
    entries: Vec<StampEntry>,
    /// 与 `entries` 同下标的搜索键（`id` + `\u{1}` + `name`，均已 `to_lowercase`）。
    ///
    /// 用 `\u{1}` 分隔是为了防「id 尾 + name 头」拼出原文里不存在的子串（假命中）。
    search_keys: Vec<String>,
    /// `id`（小写）→ `entries` 下标。
    by_id: HashMap<String, usize>,
    /// `kind`（小写）→ `entries` 下标（保持 palette.json 顺序）。
    kind_index: HashMap<String, Vec<u32>>,
    /// `regions` 元素（小写）→ `entries` 下标。
    region_index: HashMap<String, Vec<u32>>,
    /// `regionGroups` 元素（小写）→ `entries` 下标。
    region_group_index: HashMap<String, Vec<u32>>,
    /// `dataRoot` + `objectRoot`（绝对路径）。
    object_root: PathBuf,
    /// `dataRoot` + `previewRoot`（绝对路径）。
    preview_root: PathBuf,
    /// 文件里读到的 `schemaVersion`（恒为 1，留作诊断）。
    schema_version: u64,
    /// 磁盘上有、登记里没有的 `.X`（相对 `objectRoot`，`/` 分隔，已排序）。
    unlisted: Vec<String>,
    /// `load` 阶段的对账统计。
    stats: CatalogStats,
    /// `resolve()` 缓存：id（小写）→ 解析结果（**含 `None`**，见模块文档 §三）。
    resolve_cache: HashMap<String, Option<ResolvedStamp>>,
}

impl StampCatalog {
    /// 读 `palette.json` + 扫 `Objects/`，建立索引。
    ///
    /// # 参数
    ///
    /// `palette_json` 指向 `catalogue/palette.json`（相对/绝对均可；内部先
    /// `std::path::absolute` 再取「父目录的父目录」当 `dataRoot`，与 `StampCategories.cs:25` 同口径）。
    ///
    /// # 错误
    ///
    /// 见 [`CatalogError`]：文件缺失、读不出、JSON 非法、`schemaVersion != 1`、缺根字段、
    /// `objects` 为空 —— **一律 `Err`**。
    /// 单条 entry 损坏**不会**让整份失败，而是跳过并计入 [`CatalogStats`]（见模块文档 §二）。
    pub fn load(palette_json: &Path) -> Result<StampCatalog, CatalogError> {
        // —— 绝对化 + 定位 dataRoot（`StampCategories.cs:25` 的两跳父目录）——
        let palette_abs = std::path::absolute(palette_json).map_err(|e| {
            CatalogError::PaletteMalformed {
                path: palette_json.to_path_buf(),
                detail: format!("路径无法绝对化：{e}"),
            }
        })?;
        let bytes = match fs::read(&palette_abs) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(CatalogError::PaletteMissing {
                    path: palette_abs,
                })
            }
            Err(e) => {
                return Err(CatalogError::PaletteUnreadable {
                    path: palette_abs,
                    detail: e.to_string(),
                })
            }
        };
        let doc: RawPalette =
            serde_json::from_slice(&bytes).map_err(|e| CatalogError::PaletteMalformed {
                path: palette_abs.clone(),
                detail: e.to_string(),
            })?;

        // —— 结构性字段：缺一个就整份不可用（不猜默认值）——
        let schema_version =
            doc.schema_version
                .ok_or_else(|| CatalogError::UnsupportedSchema {
                    path: palette_abs.clone(),
                    schema_version: None,
                })?;
        if schema_version != SUPPORTED_SCHEMA_VERSION {
            return Err(CatalogError::UnsupportedSchema {
                path: palette_abs,
                schema_version: Some(schema_version),
            });
        }
        let object_root_rel =
            doc.object_root
                .ok_or_else(|| CatalogError::MissingRoot {
                    path: palette_abs.clone(),
                    field: "objectRoot",
                })?;
        let preview_root_rel =
            doc.preview_root
                .ok_or_else(|| CatalogError::MissingRoot {
                    path: palette_abs.clone(),
                    field: "previewRoot",
                })?;
        let data_root = palette_abs
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| CatalogError::PaletteMalformed {
                path: palette_abs.clone(),
                detail: "路径层数不足，取不到 dataRoot（应为 <root>/catalogue/palette.json）".to_string(),
            })?
            .to_path_buf();
        let object_root = join_within(&data_root, &object_root_rel).ok_or_else(|| {
            CatalogError::PaletteMalformed {
                path: palette_abs.clone(),
                detail: format!("objectRoot 非法（含 `..` 或绝对路径）：{object_root_rel}"),
            }
        })?;
        let preview_root = join_within(&data_root, &preview_root_rel).ok_or_else(|| {
            CatalogError::PaletteMalformed {
                path: palette_abs.clone(),
                detail: format!("previewRoot 非法（含 `..` 或绝对路径）：{preview_root_rel}"),
            }
        })?;

        let raw_objects = doc.objects.unwrap_or_default();
        if raw_objects.is_empty() {
            return Err(CatalogError::EmptyObjects { path: palette_abs });
        }

        // —— 逐条建索引（坏条目跳过并计数，绝不静默）——
        let mut stats = CatalogStats {
            object_entries: raw_objects.len(),
            ..CatalogStats::default()
        };
        let mut entries: Vec<StampEntry> = Vec::with_capacity(raw_objects.len());
        let mut search_keys: Vec<String> = Vec::with_capacity(raw_objects.len());
        let mut by_id: HashMap<String, usize> = HashMap::with_capacity(raw_objects.len());
        let mut kind_index: HashMap<String, Vec<u32>> = HashMap::new();
        let mut region_index: HashMap<String, Vec<u32>> = HashMap::new();
        let mut region_group_index: HashMap<String, Vec<u32>> = HashMap::new();

        for (pos, raw) in raw_objects.into_iter().enumerate() {
            let parsed: RawEntry = match serde_json::from_str(raw.get()) {
                Ok(v) => v,
                Err(e) => {
                    stats.skipped_entries += 1;
                    push_sample(
                        &mut stats.skipped_samples,
                        format!("objects[{pos}] JSON 结构不符：{e}"),
                    );
                    continue;
                }
            };
            let id = parsed.id.unwrap_or_default();
            let file = parsed.file.unwrap_or_default();
            let name = parsed.name.unwrap_or_default();
            if id.trim().is_empty() || file.trim().is_empty() || name.trim().is_empty() {
                stats.skipped_entries += 1;
                push_sample(
                    &mut stats.skipped_samples,
                    format!(
                        "objects[{pos}] 缺必填字段：id={:?} file={:?} name={:?}",
                        id, file, name
                    ),
                );
                continue;
            }
            let id_key = id.to_lowercase();
            if by_id.contains_key(&id_key) {
                stats.duplicate_ids += 1;
                stats.skipped_entries += 1;
                push_sample(
                    &mut stats.skipped_samples,
                    format!("objects[{pos}] id 重复：{id}"),
                );
                continue;
            }

            let index = u32::try_from(entries.len()).unwrap_or(u32::MAX);
            let entry = StampEntry {
                id,
                file,
                name,
                kind: parsed.kind,
                subtype: parsed.subtype,
                regions: parsed.regions.unwrap_or_default(),
                region_groups: parsed.region_groups.unwrap_or_default(),
                environments: parsed.environments.unwrap_or_default(),
                tags: parsed.tags.unwrap_or_default(),
                direction: parsed.direction,
                review_status: parsed.review_status,
                preview: parsed.preview,
            };
            // 分类索引：空串/纯空白不当键（`by_kind("")` 恒空）。
            if let Some(kind) = &entry.kind {
                if !kind.trim().is_empty() {
                    kind_index
                        .entry(kind.to_lowercase())
                        .or_default()
                        .push(index);
                }
            }
            for region in &entry.regions {
                if !region.trim().is_empty() {
                    region_index
                        .entry(region.to_lowercase())
                        .or_default()
                        .push(index);
                }
            }
            for group in &entry.region_groups {
                if !group.trim().is_empty() {
                    region_group_index
                        .entry(group.to_lowercase())
                        .or_default()
                        .push(index);
                }
            }
            search_keys.push(format!(
                "{}\u{1}{}",
                entry.id.to_lowercase(),
                entry.name.to_lowercase()
            ));
            by_id.insert(id_key, entries.len());
            entries.push(entry);
        }

        // —— 磁盘事实：扫 Objects/ + 对账 + 找漏登记 ——
        let object_root_present = object_root.is_dir();
        let mut scanned: Vec<String> = Vec::new();
        if object_root_present {
            scanned = scan_x_files(&object_root, &mut stats.scan_dir_errors);
        }
        stats.scanned_files = scanned.len();
        let scanned_set: HashSet<String> = scanned.iter().map(|r| normalize_rel(r)).collect();
        for entry in &entries {
            if scanned_set.contains(&normalize_rel(&entry.file)) {
                stats.files_present += 1;
            } else {
                stats.files_missing += 1;
                push_sample(
                    &mut stats.missing_samples,
                    format!("{} → {}", entry.id, entry.file),
                );
            }
        }
        let registered: HashSet<String> = entries.iter().map(|e| normalize_rel(&e.file)).collect();
        let mut unlisted: Vec<String> = scanned
            .into_iter()
            .filter(|rel| !registered.contains(&normalize_rel(rel)))
            .collect();
        unlisted.sort();
        stats.unlisted_files = unlisted.len();
        stats.object_root_present = object_root_present;

        Ok(StampCatalog {
            entries,
            search_keys,
            by_id,
            kind_index,
            region_index,
            region_group_index,
            object_root,
            preview_root,
            schema_version,
            unlisted,
            stats,
            resolve_cache: HashMap::new(),
        })
    }

    /// 进入索引的条目数（= `objects[]` 条数 − `stats().skipped_entries`）。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 索引是否为空（所有条目都被判损坏时才会为真）。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 全部条目，**保持 palette.json 顺序**。
    pub fn all(&self) -> &[StampEntry] {
        &self.entries
    }

    /// 按 JSON `type` 查（大小写不敏感）。`kind` 为 `null`/空白的条目不进任何分类桶。
    ///
    /// 返回顺序 = palette.json 顺序。
    pub fn by_kind(&self, kind: &str) -> Vec<&StampEntry> {
        collect(&self.entries, &self.kind_index, kind)
    }

    /// 按 JSON `regions` 元素查（大小写不敏感）。
    ///
    /// ⚠ **只查 `regions`**，不并 `regionGroups`；要 C# Region 页签的并集口径请配
    /// [`StampCatalog::by_region_group`]（见模块文档 §一.5）。返回顺序 = palette.json 顺序。
    pub fn by_region(&self, region: &str) -> Vec<&StampEntry> {
        collect(&self.entries, &self.region_index, region)
    }

    /// 按 JSON `regionGroups` 元素查（大小写不敏感）。返回顺序 = palette.json 顺序。
    pub fn by_region_group(&self, group: &str) -> Vec<&StampEntry> {
        collect(&self.entries, &self.region_group_index, group)
    }

    /// 名称 / id 的**子串**搜索（大小写不敏感）。空串/纯空白返回空（不返回全量）。
    ///
    /// 只匹配 `id` 与 `name`，不匹配 `file`/`kind`/`subtype`/`regions`/`tags`
    /// （口径理由见模块文档 §一.6）。返回顺序 = palette.json 顺序。
    pub fn search(&self, needle: &str) -> Vec<&StampEntry> {
        let needle = needle.trim().to_lowercase();
        if needle.is_empty() {
            return Vec::new();
        }
        self.entries
            .iter()
            .zip(self.search_keys.iter())
            .filter(|(_, key)| key.contains(&needle))
            .map(|(entry, _)| entry)
            .collect()
    }

    /// 该 id 的 `.X` 是否真实存在 + 解析出来的包围盒 / 格数。**结果缓存，同一 id 只读一次盘。**
    ///
    /// 返回 `None` 的情形：id 未登记、`file` 逃出 `objectRoot`、文件读不出、`.X` 解析失败。
    /// 失败也会被缓存（见 [`StampCatalog::clear_cache`]）。
    pub fn resolve(&mut self, id: &str) -> Option<ResolvedStamp> {
        let key = id.to_lowercase();
        if let Some(cached) = self.resolve_cache.get(&key) {
            return cached.clone();
        }
        let value = self.resolve_uncached(&key);
        self.resolve_cache.insert(key, value.clone());
        value
    }

    /// 清空 [`StampCatalog::resolve`] 的缓存（把新 `.X` 补进 `Objects/` 之后调一次）。
    pub fn clear_cache(&mut self) {
        self.resolve_cache.clear();
    }

    /// 磁盘上有、但 palette.json 没登记的 `.X`（相对 `objectRoot`，`/` 分隔，已排序）。
    ///
    /// 用来发现「漏登记」：`Objects/` 里有文件但索引里没有 ⇒ 那个印章在 GUI 里永远看不到。
    pub fn unlisted(&self) -> &[String] {
        &self.unlisted
    }

    /// `load` 阶段的对账统计（跳过 / 缺失 / 扫描 / 解析失败次数与样例）。
    pub fn stats(&self) -> &CatalogStats {
        &self.stats
    }

    /// `dataRoot` + `objectRoot` 解析出来的**绝对**路径。
    pub fn object_root(&self) -> &Path {
        &self.object_root
    }

    /// `dataRoot` + `previewRoot` 解析出来的**绝对**路径。
    pub fn preview_root(&self) -> &Path {
        &self.preview_root
    }

    /// 文件里读到的 `schemaVersion`（本库只认 1，留作诊断/显示）。
    pub fn schema_version(&self) -> u64 {
        self.schema_version
    }

    /// `resolve` 的实际工作体（未命中缓存时调用）。失败计数写进 `stats`。
    fn resolve_uncached(&mut self, id_key: &str) -> Option<ResolvedStamp> {
        let index = *self.by_id.get(id_key)?;
        let entry = &self.entries[index];
        let path = join_within(&self.object_root, &entry.file)?;
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(_) => {
                self.stats.resolve_read_errors += 1;
                return None;
            }
        };
        match Stamp::parse(&bytes) {
            Ok(stamp) => Some(ResolvedStamp {
                path,
                bounds: stamp.bounds(),
                cell_count: stamp.cells.len(),
            }),
            Err(_) => {
                self.stats.resolve_parse_errors += 1;
                None
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 内部工具
// ---------------------------------------------------------------------------

/// 按索引桶收集条目引用（键统一 `to_lowercase`）。
fn collect<'a>(
    entries: &'a [StampEntry],
    index: &HashMap<String, Vec<u32>>,
    key: &str,
) -> Vec<&'a StampEntry> {
    match index.get(&key.to_lowercase()) {
        Some(ids) => ids
            .iter()
            .filter_map(|&i| entries.get(i as usize))
            .collect(),
        None => Vec::new(),
    }
}

/// 记一条样例（最多 [`SAMPLE_LIMIT`] 条）。
fn push_sample(samples: &mut Vec<String>, text: String) {
    if samples.len() < SAMPLE_LIMIT {
        samples.push(text);
    }
}

/// 把 `relative` 拼到 `root` 下，**拒绝任何逃逸**：绝对路径、盘符、`..`、根前缀一律 `None`。
///
/// 比 C# `StampCategories.Resolve`（`:40-46`，用 `StartsWith` 判前缀）**更严**：
/// C# 允许 `a/../b` 这种绕一圈仍落在根内的写法，本库直接拒绝（合法 catalogue 里不该有 `..`）。
fn join_within(root: &Path, relative: &str) -> Option<PathBuf> {
    let rel = Path::new(relative);
    if relative.trim().is_empty() {
        return None;
    }
    for component in rel.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            // `..` / 根 / 盘符 / UNC 前缀 ⇒ 直接拒绝（不尝试规范化后比对）。
            _ => return None,
        }
    }
    let mut out = root.to_path_buf();
    for component in rel.components() {
        if let Component::Normal(segment) = component {
            out.push(segment);
        }
    }
    Some(out)
}

/// 相对路径归一化：`\`→`/`、去掉前导 `./`、整体小写（Windows 大小写不敏感）。
fn normalize_rel(relative: &str) -> String {
    relative
        .replace('\\', "/")
        .trim_start_matches("./")
        .to_lowercase()
}

/// 递归扫描 `root` 下所有 `.X`（扩展名大小写不敏感），返回**相对 `root`** 的路径
/// （`/` 分隔、保留磁盘大小写、已排序）。读不出的目录/目录项计入 `dir_errors`。
fn scan_x_files(root: &Path, dir_errors: &mut usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let read_dir = match fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(_) => {
                *dir_errors += 1;
                continue;
            }
        };
        for item in read_dir {
            let item = match item {
                Ok(v) => v,
                Err(_) => {
                    *dir_errors += 1;
                    continue;
                }
            };
            let path = item.path();
            let file_type = match item.file_type() {
                Ok(ft) => ft,
                Err(_) => {
                    *dir_errors += 1;
                    continue;
                }
            };
            if file_type.is_dir() {
                stack.push(path);
                continue;
            }
            if !file_type.is_file() {
                continue;
            }
            let is_stamp = path
                .extension()
                .map(|ext| ext.eq_ignore_ascii_case("X"))
                .unwrap_or(false);
            if !is_stamp {
                continue;
            }
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            out.push(rel);
        }
    }
    out.sort();
    out
}
