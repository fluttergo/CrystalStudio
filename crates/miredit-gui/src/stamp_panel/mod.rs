//! 资源面板「分类」页签 = **印章浏览器**（9,159 枚 `.X`，按官方中文分类分组 + 搜索 + 缩略图 + 选中）。
//!
//! # 一、本页签做什么 / **不做什么**
//!
//! 做：把 `CrystalStudio/Objects/**/*.X` 按 `catalogue/palette.json` 的 19 个分类
//! （中文名取自 `mir2_stamp::kind_label_zh`）分组，配搜索、缩略图网格、分页与选中；
//! 选中的那一枚会出现在右栏「当前资源」与状态栏。
//!
//! **不做**（任务书 §三.6 明确留白，不假装）：放置、拖拽、旋转/翻转、收藏。
//! 所以本模块**不产生任何编辑动作** —— [`StampAction`] 只有 `Pick`（选中）一种。
//!
//! # 二、数据从哪来（**一条都不自己造**）
//!
//! | 用途 | 来源 | 本模块的角色 |
//! |---|---|---|
//! | 条目 / 分类 / 搜索 / 未登记 / 包围盒 / 格数 | [`mir2_stamp::StampCatalog`] | 只调用，不重建索引 |
//! | 分类中文名 | [`mir2_stamp::kind_label_zh`]（20 条实测表） | 只查表，查不到回落英文 |
//! | `.X` 字节 → 格 | [`mir2_stamp::Stamp::parse`] | 只解析，不改字段 |
//! | 格 → RGBA 缩略图 | [`miredit_render::stamp::render_stamp`] + `thumbnail(46,32)` | 只调它，**不自己画** |
//! | 帧像素 | [`mir2_assets::WilSource`]（与地图渲染同一套） | 只读 |
//!
//! # 三、装载不阻塞首帧
//!
//! `StampCatalog::load` 实测 **48 ~ 83 ms**（release，页缓存已命中）。虽然不长，但它要在
//! 首帧之前跑完才能出列表 —— 所以走**后台线程 + 每帧 `try_recv`**：
//! 首帧照常画（提示行写「正在加载印章目录…」），装载完的那一帧自动刷新列表。
//! 装载失败**不降级成空目录**：提示行写 `印章目录加载失败：<原因>`，列表为空。
//!
//! # 四、缩略图：LRU 缓存 + **缺帧不造假**
//!
//! 全库 9,159 枚若全上传约 54 MB 纹理，本单明确禁止（任务书 §三.3）。策略：
//!
//! 1. 只渲染**当前可见格**（一屏 20 枚，滚轮/翻页时才换）；
//! 2. 渲染结果进 [`LruCache`]（上限 [`m::STAMP_THUMB_CACHE_CAP`] = 256，LRU 淘汰）；
//! 3. **负缓存**：缺帧 / 读不出 / 解析失败 / 库号越界 / 渲染报错，一律记 `Missing`，
//!    同一枚不会每帧去读一次盘（与 [`crate::palette`] 的负缓存同思路）。
//!
//! 缺帧判据（任务书 §三.5）：`StampImage::missing > 0` 或 `cells_drawn == 0` 或 `width == 0`
//! ⇒ 格子里画**灰底 +「缺帧」**，**绝不画洋红、绝不用上一枚的图充数**。
//! 部分缺帧（画了一部分、丢了几层）也按缺帧处理并在统计里分开计数（[`StampStats`]）。
//!
//! # 五、几何全部来自 [`crate::metrics`]
//!
//! 页签/标签行/下拉/搜索/网格顶/提示行/分页行的纵向偏移**复用** `PALETTE_*_OFF`（同一个面板骨架），
//! 本模块只定义格子（4 列 × 84 px、46×32 缩略图、53 px 行高）与每页 96 枚 —— 全在
//! `metrics.rs` 里带推导注释，本文件**没有一个"看起来差不多"的字面量**。
//!
//! # 六、文件划分（单文件 ≤600 行）
//!
//! | 文件 | 内容 |
//! |---|---|
//! | `mod.rs`（本文件） | 状态 / 装载 / 查询 / 统计 / 文档 |
//! | `thumbs.rs` | LRU 缓存 / `.X` → `PositionedCell` 适配 / 渲染 / 文本截断 |
//! | `draw.rs` | 网格绘制（缩略图 / 缺帧占位 / 名称行 / 选中框）与分类下拉弹层 |
//! | `geom.rs` | 网格/弹层矩形、命中测试、分页算术 |
//! | `select.rs` | 分类行表 / 搜索 / 翻页 / **唯一**选中入口 |
//!
//! 四个子模块与父模块共享**同一份私有状态**（Rust 的子模块能看见父模块的私有项），
//! 所以拆分不产生第二套真值，也不把字段被迫改成 `pub`。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Instant;

use mir2_stamp::{kind_label_zh, StampCatalog};

use crate::metrics as m;
use crate::ui_text as tx;

mod draw;
mod geom;
mod select;
mod thumbs;

pub(crate) use thumbs::LruCache;
use thumbs::Thumb;
pub use geom::{max_row_off, page_count, page_range, page_rows};
pub use thumbs::{fit_text, positioned_cells, render_thumb};


/// 指定 `palette.json` 的环境变量（与 `mir2-stamp` 的 `examples/catalog_probe.rs` 同一个名字，
/// 免得同一个仓库里出现两套"去哪儿找目录索引"的约定）。
pub const PALETTE_ENV: &str = "MIR2_STAMP_PALETTE";

/// 缺省 `palette.json` 路径：**恒为空串**（=「没有内建缺省路径」）。
///
/// 公开仓不内置任何开发机路径：真正的入口是环境变量 [`PALETTE_ENV`]。
/// 空串不是路径 —— [`StampPanel::with_path`] 收到空路径时**不读盘**，
/// 直接进「加载失败」态并如实显示「未设置环境变量」，界面不会静默空表。
pub const DEFAULT_PALETTE: &str = "";

/// 「分类」页签在 [`crate::ui_text::PALETTE_TABS`] 里的下标（0 = 资源库 / 1 = 收藏 / 2 = 最近使用 / 3 = 分类）。
///
/// ⚠ 这个数**必须**等于文案表里的位置：`stamp_panel_tests::分类页签下标与文案表一致`
/// 用 `PALETTE_TABS[TAB_INDEX] == TAB_CATEGORIES` 钉住它（改文案表顺序会当场红）。
pub const TAB_INDEX: usize = 3;

/// 网格交互的产物。**只有"选中"一种** —— 本页签不做放置/拖拽/旋转/收藏（任务书 §三.6）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StampAction {
    /// 什么都没发生。
    None,
    /// 用户选中了一枚印章（调用方负责把它写进「当前资源」与状态栏）。
    Pick,
}

/// 分类过滤。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Filter {
    /// 不过滤（9,159 枚）。
    All,
    /// 某个英文 kind（`Terrain` / `Cliff` …，来自 `palette.json` 的 `type`）。
    Kind(String),
    /// `type` 为 `null` 的那些（实测 9 条，中文名取 `kind_label_zh("")`）。
    Unclassified,
    /// 磁盘上有、`palette.json` 没登记的 `.X`（实测 11 个，只显示文件名）。
    Unlisted,
}

impl Filter {
    /// 下拉框里显示的名字（分类名走 `mir2_stamp` 的对照表，不在这里重抄）。
    pub fn label(&self) -> String {
        match self {
            Filter::All => tx::STAMP_KIND_ALL.to_owned(),
            Filter::Kind(k) => kind_label_zh(k).unwrap_or(k.as_str()).to_owned(),
            Filter::Unclassified => kind_label_zh("").unwrap_or(tx::VAL_UNKNOWN).to_owned(),
            Filter::Unlisted => tx::STAMP_UNLISTED.to_owned(),
        }
    }

    /// 下拉里的稳定键（`--stamp-kind` 也用它）：`all` / `unclassified` / `unlisted` / 英文 kind。
    pub fn key(&self) -> String {
        match self {
            Filter::All => "all".to_owned(),
            Filter::Kind(k) => k.clone(),
            Filter::Unclassified => "unclassified".to_owned(),
            Filter::Unlisted => "unlisted".to_owned(),
        }
    }
}

/// 列表里的一项：已登记条目（带缩略图）或未登记文件（只有文件名）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Item {
    /// `catalog.all()` 的下标。
    Entry(u32),
    /// `catalog.unlisted()` 的下标。
    Unlisted(u32),
}

/// 目录装载状态机。
enum Load {
    /// 后台线程正在装载（`Receiver` 断开 = 线程异常退出）。
    Loading(Receiver<Result<StampCatalog, String>>),
    /// 装载完成。
    Ready(Box<StampCatalog>),
    /// 装载失败（原文，**不降级成空目录**）。
    Failed(String),
}

/// 选中的那一枚（**自持快照**：右栏与状态栏只要这几项，不必再摸目录）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selected {
    /// 主键（`mir/mir_00000`）或未登记文件的相对路径。
    pub id: String,
    /// 显示名（未登记项 = 文件名）。
    pub name: String,
    /// 中文分类（未登记项 = 「未登记」；未知 kind 回落英文）。
    pub kind_zh: String,
    /// 格数（`resolve` 失败时为 `None`，界面如实填 `—`）。
    pub cells: Option<usize>,
    /// 包围盒 `(x0,y0,x1,y1)`（闭区间；`resolve` 失败时为 `None`）。
    pub bounds: Option<(i32, i32, i32, i32)>,
    /// `.X` 的**绝对路径**（2026-11-05 新增：放置要真读盘）。
    ///
    /// 取值来自 `StampCatalog::resolve`（已过逃逸护栏，一定落在 `objectRoot` 内）。
    /// `None` 的两种情形都**不猜路径**：
    /// - 未登记项（磁盘上有、`palette.json` 没登记）—— 没有 `resolve` 可言；
    /// - 已登记但 `resolve` 失败（文件缺失 / `.X` 解析失败 / 库号越界）。
    ///
    /// 两种情形放置时都会给出中文原因（[`crate::ui_text::STAMP_NO_PATH`]），
    /// 绝不"顺手猜一个同目录下的文件名"（那会放一枚用户没选的印章）。
    pub path: Option<PathBuf>,
}

/// 面板统计（`--stamp-report` / 探针 / 单测共用同一份口径）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StampStats {
    /// 真渲染次数（每枚每次渲染 +1，命中缓存不加）。
    pub decodes: u64,
    /// 缓存命中次数（含负缓存命中）。
    pub hits: u64,
    /// LRU 淘汰次数。
    pub evictions: u64,
    /// 缓存条目数（当前）。
    pub cached: usize,
    /// 缓存条目数（历史峰值）。
    pub peak: usize,
    /// 真渲染成功并上传的枚数。
    pub rendered_ok: u64,
    /// 判成「缺帧」的枚数（含 `missing > 0` 的部分缺帧）。
    pub rendered_missing: u64,
    /// 库号超出 `PositionedCell` 的 `u8` 表达范围而放弃渲染的枚数（**不静默截断成别的库**）。
    pub lib_overflow: u64,
}

/// 印章浏览器面板（状态 + 交互 + 绘制）。
pub struct StampPanel {
    /// 目录装载状态机。
    load: Load,
    /// 开始装载的时刻（报耗时用）。
    load_started: Instant,
    /// 装载耗时（毫秒，装载完成后才有值）。
    load_ms: Option<f64>,
    /// 实际使用的 `palette.json` 路径。
    path: PathBuf,
    /// `id`（小写）→ `all()` 下标（把 `search()` 的结果映射回下标，保持 palette 顺序）。
    by_id: HashMap<String, u32>,
    /// 19 个 kind + 各自条数（按条数降序，**真实统计**，不是常量表）。
    kinds: Vec<(String, usize)>,
    /// `type` 为 `null` 的条数（实测 9）。
    unclassified: usize,
    /// 未登记文件数（= `catalog.unlisted().len()`）。
    unlisted: usize,
    /// 当前分类过滤。
    filter: Filter,
    /// 搜索框文字。
    search: String,
    /// 搜索框是否有焦点（有焦点才吃键盘）。
    focus: bool,
    /// 分类下拉是否展开。
    combo_open: bool,
    /// 下拉弹层的滚动行。
    combo_scroll: usize,
    /// 当前页（0 基）。
    page: usize,
    /// 页内顶部行（0 基；滚轮改它，翻页时归零）。
    row_off: usize,
    /// 当前过滤条件下的条目（**保持 palette.json 顺序**）。
    visible: Vec<Item>,
    /// 选中项。
    selected: Option<Selected>,
    /// 缩略图 LRU 缓存。
    cache: LruCache<Thumb>,
    /// 真渲染次数。
    decodes: u64,
    /// 缓存命中次数。
    hits: u64,
    /// 真渲染成功枚数。
    rendered_ok: u64,
    /// 判成缺帧的枚数。
    rendered_missing: u64,
    /// 库号越界而放弃渲染的枚数。
    lib_overflow: u64,
}

impl Default for StampPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl StampPanel {
    /// 用缺省路径（环境变量 [`PALETTE_ENV`] → [`DEFAULT_PALETTE`]）**后台**装载，立刻返回。
    ///
    /// 两者都没给（= 空路径）时由 [`StampPanel::with_path`] 如实进失败态，**不猜**任何路径。
    pub fn new() -> Self {
        let path = match std::env::var_os(PALETTE_ENV) {
            Some(v) => PathBuf::from(v),
            None => PathBuf::from(DEFAULT_PALETTE),
        };
        Self::with_path(path)
    }

    /// 指定 `palette.json` 路径并**后台**装载（测试与 `--stamp-*` 用同一条路径）。
    ///
    /// **空路径 ⇒ 不读盘、不起线程**，直接进 [`Load::Failed`] 并写明「未设置哪个环境变量」：
    /// 公开仓没有内建缺省索引，`StampCatalog::load("")` 只会给出一个语焉不详的 io 错误。
    pub fn with_path(path: PathBuf) -> Self {
        let load = if path.as_os_str().is_empty() {
            Load::Failed(format!("<未设置 {}>", PALETTE_ENV))
        } else {
            let (txr, rx) = mpsc::channel();
            let p = path.clone();
            std::thread::spawn(move || {
                let out = StampCatalog::load(&p).map_err(|e| e.to_string());
                let _ = txr.send(out);
            });
            Load::Loading(rx)
        };
        Self {
            load,
            load_started: Instant::now(),
            load_ms: None,
            path,
            by_id: HashMap::new(),
            kinds: Vec::new(),
            unclassified: 0,
            unlisted: 0,
            filter: Filter::All,
            search: String::new(),
            focus: false,
            combo_open: false,
            combo_scroll: 0,
            page: 0,
            row_off: 0,
            visible: Vec::new(),
            selected: None,
            cache: LruCache::new(),
            decodes: 0,
            hits: 0,
            rendered_ok: 0,
            rendered_missing: 0,
            lib_overflow: 0,
        }
    }

    /// 每帧收一次装载结果（装载完的那一帧把列表刷出来）。
    ///
    /// 返回 `true` 表示"这一帧状态变了"，调用方据此 `request_repaint`。
    pub fn poll(&mut self) -> bool {
        let Load::Loading(rx) = &self.load else {
            return false;
        };
        match rx.try_recv() {
            Ok(Ok(catalog)) => {
                self.load_ms = Some(self.load_started.elapsed().as_secs_f64() * 1e3);
                self.adopt(catalog);
                true
            }
            Ok(Err(err)) => {
                self.load = Load::Failed(err);
                true
            }
            Err(TryRecvError::Empty) => false,
            Err(TryRecvError::Disconnected) => {
                self.load = Load::Failed(tx::STAMP_THREAD_LOST.to_owned());
                true
            }
        }
    }

    /// 装载成功后建索引（分类统计 / id → 下标 / 未登记数），再刷列表。
    fn adopt(&mut self, catalog: StampCatalog) {
        let mut by_id = HashMap::with_capacity(catalog.len());
        let mut kinds: Vec<(String, usize)> = Vec::new();
        let mut unclassified = 0usize;
        for (i, e) in catalog.all().iter().enumerate() {
            by_id.insert(e.id.to_lowercase(), i as u32);
            match e.kind.as_deref() {
                Some(k) if !k.trim().is_empty() => match kinds.iter_mut().find(|(kk, _)| kk == k) {
                    Some(slot) => slot.1 += 1,
                    None => kinds.push((k.to_owned(), 1)),
                },
                _ => unclassified += 1,
            }
        }
        // 条数降序、同条数按英文名升序（与 `kind_labels` 的实测读数表同一排序口径）
        kinds.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        self.kinds = kinds;
        self.unclassified = unclassified;
        self.unlisted = catalog.unlisted().len();
        self.by_id = by_id;
        self.load = Load::Ready(Box::new(catalog));
        self.rebuild();
    }

    /// 目录（装载完成后才有）。
    pub(super) fn catalog(&self) -> Option<&StampCatalog> {
        match &self.load {
            Load::Ready(c) => Some(c),
            _ => None,
        }
    }

    pub(super) fn catalog_mut(&mut self) -> Option<&mut StampCatalog> {
        match &mut self.load {
            Load::Ready(c) => Some(c),
            _ => None,
        }
    }

    /// 装载是否已完成。
    pub fn is_ready(&self) -> bool {
        matches!(self.load, Load::Ready(_))
    }

    /// 是否还在后台装载（**只有这一种状态需要继续请求重绘**；
    /// `Failed` 不需要 —— 否则会变成每帧空转）。
    pub fn is_loading(&self) -> bool {
        matches!(self.load, Load::Loading(_))
    }

    /// 实际使用的 `palette.json` 路径（诊断/探针用）。
    pub fn palette_path(&self) -> &std::path::Path {
        &self.path
    }

    /// 后台装载耗时（毫秒；装载完成后才有值，**不猜一个数字**）。
    pub fn load_ms(&self) -> Option<f64> {
        self.load_ms
    }

    /// 提示行该写什么（装载中 / 失败 / 无图源 / 未登记分组说明）。
    ///
    /// 全部来自 [`crate::ui_text`]，没有一句是临时拼的。
    pub fn note(&self, has_source: bool) -> Option<String> {
        match &self.load {
            Load::Loading(_) => Some(tx::STAMP_LOADING.to_owned()),
            Load::Failed(err) => Some(tx::fill(tx::STAMP_LOAD_FAIL_TMPL, &[("err", err)])),
            Load::Ready(_) => {
                if !has_source {
                    return Some(tx::STAMP_NO_SOURCE.to_owned());
                }
                if self.filter == Filter::Unlisted {
                    return Some(tx::fill(
                        tx::STAMP_UNLISTED_NOTE_TMPL,
                        &[("n", &self.unlisted.to_string())],
                    ));
                }
                None
            }
        }
    }

    /// 当前过滤条件下的条数。
    pub fn total(&self) -> usize {
        self.visible.len()
    }

    /// 总页数（空列表也算 1 页，好让分页行有东西可显示）。
    pub fn pages(&self) -> usize {
        page_count(self.visible.len())
    }

    /// 当前页（0 基）。
    pub fn page(&self) -> usize {
        self.page
    }

    /// 页内顶部行（0 基）。
    pub fn row_off(&self) -> usize {
        self.row_off
    }

    /// 当前过滤。
    pub fn filter(&self) -> &Filter {
        &self.filter
    }

    /// 搜索框文字。
    pub fn search_text(&self) -> &str {
        &self.search
    }

    /// 搜索框是否有焦点。
    pub fn focused(&self) -> bool {
        self.focus
    }

    /// 分类下拉是否展开。
    pub fn combo_open(&self) -> bool {
        self.combo_open
    }

    /// 选中的那一枚。
    pub fn selected(&self) -> Option<&Selected> {
        self.selected.as_ref()
    }

    /// 状态栏那一段（没选中就是 `None`，不写"印章：无"占位）。
    pub fn status_text(&self) -> Option<String> {
        self.selected
            .as_ref()
            .map(|s| tx::fill(tx::STAMP_STATUS_TMPL, &[("name", &s.name)]))
    }

    /// 把当前选中项折成**待放置态**（`None` = 还没选中任何一枚）。
    ///
    /// 只搬三个字段（`id` / `name` / `bounds`），**不复制格表**：格表要等用户真点画布时
    /// 才现读盘（[`crate::stamp_place::place`]）—— 选中只是"看一眼"，
    /// 那时就读 9,159 枚里的某一枚纯属浪费，而且用户可能只是翻页路过。
    pub fn armed(&self) -> Option<crate::stamp_place::Armed> {
        self.selected.as_ref().map(|s| crate::stamp_place::Armed {
            id: s.id.clone(),
            name: s.name.clone(),
            bounds: s.bounds,
        })
    }

    /// 当前选中项的 `.X` 绝对路径（`None` = 未登记 / `resolve` 失败 ⇒ 放置会报中文原因）。
    pub fn selected_path(&self) -> Option<&std::path::Path> {
        self.selected.as_ref()?.path.as_deref()
    }

    /// 统计快照。
    pub fn stats(&self) -> StampStats {
        StampStats {
            decodes: self.decodes,
            hits: self.hits,
            evictions: self.cache.evictions(),
            cached: self.cache.len(),
            peak: self.cache.peak(),
            rendered_ok: self.rendered_ok,
            rendered_missing: self.rendered_missing,
            lib_overflow: self.lib_overflow,
        }
    }


    // ── 几何 / 分页 / 过滤与选中 ──
    //
    // 这三段在 `geom.rs`（几何 + 分页算术）与 `select.rs`（分类行表 / 搜索 / 翻页 / 选中）里，
    // 都是本类型的 `impl` 块 —— 调用点写起来与在同一文件里完全一样。
}

/// `Objects/a/b.X` → `b.X`（未登记项**只显示文件名**，不显示目录、不猜分类）。
pub fn file_name(rel: &str) -> String {
    rel.rsplit(['/', '\\']).next().unwrap_or(rel).to_owned()
}


/// 当前过滤条件下的诊断行（`--stamp-report` / 探针收尾各打一次）。
pub fn report_text(panel: &StampPanel) -> String {
    let st = panel.stats();
    tx::fill(
        tx::DIAG_STAMP,
        &[
            ("filter", &panel.filter().label()),
            ("total", &panel.total().to_string()),
            ("page", &(panel.page() + 1).to_string()),
            ("pages", &panel.pages().to_string()),
            ("cached", &st.cached.to_string()),
            ("peak", &st.peak.to_string()),
            ("cap", &m::STAMP_THUMB_CACHE_CAP.to_string()),
            ("decodes", &st.decodes.to_string()),
            ("hits", &st.hits.to_string()),
            ("missing", &st.rendered_missing.to_string()),
        ],
    )
}
