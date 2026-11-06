//! 文档与图库装载：把「地图文件 + 图库源」变成界面能直接用的状态。
//!
//! ## 四条纪律
//! 1. **失败必须说出来**：读文件失败 / `.map` 解析失败 / 图库扫描失败 / 图库打开失败，
//!    一律同时 ① 写 `stderr`（带完整原因）② 落到状态栏（界面可见）；
//!    绝不"静默空白"——那会让用户以为程序卡住。
//! 2. **不造假数据**：拿不到图库就把库数如实记 0，不塞一个好看的数字；
//!    拿不到地图就 `doc = None`，画布画 [`crate::ui_text::CANVAS_NO_MAP`]。
//! 3. **加载一次、只读共享**：`Map` 与 `WilSource` 装好之后整场只读，
//!    重渲只读不写（`miredit_render::render_region` 也就吃 `&Map` + `&dyn SpriteSource`）。
//! 4. **九格式由内核判、宿主不重判**（2026-11-05）：打开走
//!    `miredit_core::Document::load`（内部 `mir2_format::read_any_tolerant`），
//!    宿主**不再**自己 `mir2_map::Map::parse` —— 那只认 12 / 14 / 36 三档。
//!    保存走 [`MapDoc::to_bytes`]，宿主**不许**再 `map.to_bytes()`。

use std::path::{Path, PathBuf};

use mir2_assets::{AssetError, Family, LibPaths, WilSource};
use mir2_map::Map;

use crate::ui_text as tx;

/// 缺省打开的地图：**恒为空串**（=「没有内建缺省地图」）。
///
/// 公开仓不内置任何开发机路径。没有 `--map` 时启动即「未指定地图」：画布画
/// [`crate::ui_text::CANVAS_NO_MAP`]、状态栏如实说原因，**不 panic、不猜路径**。
pub const DEFAULT_MAP_PATH: &str = "";

/// 一张已打开的地图：**编辑视图**（`map`）+ **字节层真值**（`document`）+ 出处信息。
///
/// # 为什么要两份（2026-11-05，九格式收口）
///
/// `miredit_core::Document` 是 raw+view 双层（见 `miredit-core/src/document.rs` 模块文档）：
///
/// ```text
/// document.raw()  : mir2_format::RawMap  ← 打开时 read_any_tolerant(bytes) 的产物
/// document.view() : mir2_map::Map        ← 渲染/编辑只认它
/// ```
///
/// 九种格式（含 Type5 / Type1 / Type4 / Mir3 / CrystalCs）的**头部长度、格长、尾区**各不相同，
/// 而 `mir2_map::Map::to_bytes()` 只会写 12 / 14 / 36 三档 ⇒ **保存绝不能走 `map.to_bytes()`**
/// （那会把 Type5 写成 New14、把 67 张带尾块的图截断）。所以：
///
/// - **`map`（pub）**：宿主全程用的编辑视图，渲染/落笔/撤销都改它（既有 API 一字未变）；
/// - **`document`（私有）**：打开时那份字节的真值，**保存的唯一出口**是
///   [`MapDoc::to_bytes`]，它先把 `map` 用 O(1) 的 `std::mem::swap` 换进 `document.view`，
///   再让 `Document::try_to_bytes` 出字节（九格式各自 writer + `header_raw` + `trailer` 原样）。
///
/// ⚠ **不变量**：`document.view()` 与 `map` 是**同一批格的两份拷贝**，只有
/// [`MapDoc::to_bytes`] 会在换进/换出的那一瞬间让它们一致；**任何其它地方都不许读
/// `document.view()`**（它是私有的，物理上读不到）。
/// 为什么不做成"只有一份"：`EditSession` 的每个改格方法签名都是 `&mut Map`
/// （`edit.rs:528` 起共 20 个），而 `miredit-core` 只读不许改 —— 保持 `map` 是独立一份，
/// 才能让 80 多条单元测试与宿主调用点一行不改。
pub struct MapDoc {
    /// 打开时的文件路径（原样保留，用于标题与诊断）。
    pub path: PathBuf,
    /// 文件名（含扩展名，如 `0.map`）——标题栏、页签、状态栏都用它。
    pub file_name: String,
    /// **编辑视图**（渲染 / 落笔 / 撤销都改它）。
    pub map: Map,
    /// **字节层真值**（九格式判别 + 未解尾块 + 原文件字节）；保存只走 [`MapDoc::to_bytes`]。
    document: miredit_core::Document,
    /// 文件字节数（诊断用；也是"真的读了这么多字节"的凭据）。
    pub bytes: usize,
}

impl MapDoc {
    /// 从磁盘打开一张 `.map`（与 [`DocState::load`] 走**同一条** `Document::load`）。
    ///
    /// 为什么单独暴露：验收夹具与单元测试要造 [`DocState`] 但不想扫 117 个资源库，
    /// 走这里就与生产路径的解析、错误文案完全一致（不再各写一份 `Map::parse`）。
    pub fn open(path: &Path) -> Result<MapDoc, String> {
        read_map(path)
    }

    /// 从**内存字节**构造（合成样本 / 上传字节用）；`path` 只作为出处与诊断。
    pub fn from_bytes(path: &Path, bytes: Vec<u8>) -> Result<MapDoc, String> {
        let document = miredit_core::Document::from_bytes(path, bytes).map_err(core_err_text)?;
        Ok(Self::from_document(document))
    }

    /// 把内核文档折成宿主文档（取一次视图克隆，见类型文档）。
    fn from_document(document: miredit_core::Document) -> MapDoc {
        let map = document.view().clone();
        let bytes = document.orig_len();
        MapDoc {
            path: document.path.clone(),
            file_name: document.file_name.clone(),
            map,
            document,
            bytes,
        }
    }

    /// 宽 × 高（世界格）。
    pub fn size(&self) -> (u16, u16) {
        (self.map.width, self.map.height)
    }

    /// **视图档位名**（`Old12` / `New14` / `Ext36`）—— 这是 `mir2_map::CellFormat` 的三档
    /// 语义投影，**不是**磁盘格长（Type5 的视图也是 `New14`）。
    ///
    /// 真格式名请用 [`MapDoc::format_label`]。
    pub fn format_name(&self) -> &'static str {
        self.map.format.name()
    }

    /// **九格式人读名**（例如 `"Mir3Wemade（14B/格 + 半分辨率背景块，无标题）"`）。
    ///
    /// 真值来自 `mir2-format` 的判别（`Document::format_label` → `format_by_id(id).name()`），
    /// 不是猜的、也不是从格长反推的。
    pub fn format_label(&self) -> &'static str {
        self.document.format_label()
    }

    /// **常驻告警**（`None` = 没有要提醒的事）：未解尾块 / 格式未承诺编辑语义。
    ///
    /// 文案是 `miredit_core::Document::edit_warning` 的**原文**（本函数一个字符都不加工），
    /// 界面按「常驻黄条」显示（见 [`crate::banner`]），**不是 toast、不自动消失**。
    pub fn edit_warning(&self) -> Option<String> {
        self.document.edit_warning()
    }

    /// `raw.trailer` 的字节数 —— **本工具没解释的剩余字节**（实测那 67 张真实文件都是 `8040`）。
    pub fn tolerant_trailer(&self) -> usize {
        self.document.tolerant_trailer()
    }

    /// 该格式的「地表逐格落笔」是否有效（九种里只有 Type5 / `Mir3Wemade` 为 `false`）。
    ///
    /// `false` ⇒ 地表图层行要打「不可落笔」标记，且**点画布之前就要拦**（见
    /// [`crate::app`] 的 `ground_refusal`），不能等内核拒绝。
    pub fn ground_paint_supported(&self) -> bool {
        self.document.ground_paint_supported()
    }

    /// 打开时那份**磁盘原文件字节**（只读；给编辑会话绑定真文档用）。
    pub fn orig_bytes(&self) -> &[u8] {
        self.document.orig_bytes()
    }

    /// 磁盘单格字节数（**变长格式**如 `CrystalCs` 返回 `None` —— 没有"一格几字节"这件事）。
    pub fn disk_cell_len(&self) -> Option<usize> {
        self.document.disk_cell_len()
    }

    /// 当前内存地图 vs 磁盘原文件的**逐字节差异**（九格式口径）。
    ///
    /// 为什么不用 `miredit_core::byte_diff(&Map, &[u8])`：那个只对 12/14/36 三档成立
    /// （它按 `map.format.cell_size()` 折格、头部固定按 52 字节算）。九种格式的头部长度与
    /// 磁盘格长各不相同 ⇒ 必须走 `Document::diff()`（它按 `raw_header().len()` +
    /// `disk_cell_len()` 折格）。
    ///
    /// 实现同 [`MapDoc::to_bytes`]：`map` 与 `document.view` 用 O(1) 对调再换回。
    pub fn diff(&mut self) -> miredit_core::ByteDiff {
        std::mem::swap(self.document.view_mut(), &mut self.map);
        let d = self.document.diff();
        std::mem::swap(self.document.view_mut(), &mut self.map);
        d
    }

    /// **把文档的出处路径改成新路径**（「另存为」成功后调，2026-11-05）。
    ///
    /// 改三处，**必须一起改**：
    /// 1. `self.path` —— 之后 [`crate::save::save_session`] 写的就是它；
    /// 2. `self.file_name` —— 标题栏 / 页签 / 状态栏左段都读它（见 [`crate::screen_text::build`]）；
    /// 3. 内核 `Document` 的同名字段（`miredit-core/src/document.rs:76-78` 是 `pub`）——
    ///    它是"这张图从哪来"的字节层真值，`Document::load` / `from_bytes` 都按路径记名。
    ///
    /// 少改任何一处就会出现**两套真值**：标题写着 `b.map`、`Ctrl+S` 却写回 `a.map`
    /// （那正是「另存为」最容易出的数据事故）。
    ///
    /// # 它**不**改什么（有意，写清楚免得被当成漏改）
    ///
    /// - **字节层 `orig_bytes` 仍是"打开那一刻"的磁盘内容**：`Document` 里那份是私有字段、
    ///   只有构造器能写（`miredit-core/src/document.rs:80`），而"打开时是什么"本来就不该被
    ///   一次另存为改写。于是 [`MapDoc::diff`] 的语义保持为**"相对打开时那份文件的总改动"**
    ///   —— 另存为之后它照样成立（另存为只是把当前内容落到新文件，没有回退任何改动）。
    /// - 不写盘、不碰脏标记、不重建 `document`（重建要走一遍"字节 → 解析"的往返，
    ///   九种格式的往返保真不在本单的证明范围内，不能拿用户资产去赌）。
    pub fn retarget(&mut self, path: &Path) {
        self.path = path.to_path_buf();
        self.file_name = file_name_of(path);
        self.document.path = self.path.clone();
        self.document.file_name = self.file_name.clone();
    }

    /// **出保存字节**：九格式各自的 writer + 未解尾块原样带上。
    ///
    /// 实现：把编辑视图 `map` 与 `document.view` 用 `std::mem::swap` 对调（**O(1)，不拷格**），
    /// 让 `Document::try_to_bytes` 看到"当前编辑结果"，出完字节再换回来。
    ///
    /// ⚠ 失败**绝不回退**到 `map.to_bytes()`（那会把 Type5 写成 New14、把尾块截断 ——
    /// 用户以为存上了，磁盘上却是另一种格式）。
    pub fn to_bytes(&mut self) -> Result<Vec<u8>, String> {
        std::mem::swap(self.document.view_mut(), &mut self.map);
        let out = self.document.try_to_bytes().map_err(core_err_text);
        std::mem::swap(self.document.view_mut(), &mut self.map);
        out
    }
}

/// 图库状态：要么装好了（能取帧），要么带着**具体原因**失败。
///
/// ⚠ `src` 是 `Box<WilSource>`，不是直接内联：`WilSource` 内含帧缓存，本体 ≥400 B，
/// 内联会让整个枚举最小 400 B、而 `Failed` 那支只要 24 B —— clippy
/// `large_enum_variant` 报的就是这个（"枚举至少 400 字节，两支差 376"）。
/// `DocState` 里这个枚举是**长期驻留的单例**，不值得为它撑 400 B 的栈/结构体尺寸；
/// 指针化之后 `Ready` 只剩登记表 + 一个指针，两支尺寸相当。
/// 调用方无感：`Assets::source()` 仍然返回 `Option<&WilSource>`。
pub enum Assets {
    /// 已登记并打开：`paths` 是登记表，`src` 是取帧入口。
    Ready {
        /// 图库登记表（库号 → 文件）。
        paths: LibPaths,
        /// 取帧源（内含帧缓存；装箱理由见本枚举文档）。
        src: Box<WilSource>,
    },
    /// 装载失败，`0` 在这里是**真实原因字符串**，不是占位。
    Failed(String),
}

impl Assets {
    /// 已登记的库总数（失败即 0 —— 状态栏如实显示 0）。
    pub fn lib_count(&self) -> usize {
        match self {
            Assets::Ready { paths, .. } => paths.available(),
            Assets::Failed(_) => 0,
        }
    }

    /// 取帧源；失败时 `None`（调用方据此跳过渲染，而不是画假图）。
    pub fn source(&self) -> Option<&WilSource> {
        match self {
            // `src` 是 `&Box<WilSource>` ⇒ `as_ref()` 给出 `&WilSource`（不拷帧缓存）
            Assets::Ready { src, .. } => Some(src.as_ref()),
            Assets::Failed(_) => None,
        }
    }

    /// 失败原因（界面要显示时用）。
    pub fn error(&self) -> Option<&str> {
        match self {
            Assets::Ready { .. } => None,
            Assets::Failed(e) => Some(e),
        }
    }
}

/// 整个文档状态（本单只支持一张图，故不是 Vec）。
pub struct DocState {
    /// 当前地图；`None` = 没打开（或打开失败，原因见 [`DocState::map_error`]）。
    pub doc: Option<MapDoc>,
    /// 图库状态。
    pub assets: Assets,
    /// 地图打开失败的原因（`Some` 时状态栏左段显示它）。
    pub map_error: Option<String>,
    /// 地图代号：每次换上不同的图就 +1，供画布缓存判定"要不要丢掉纹理"。
    pub map_gen: u64,
}

impl DocState {
    /// 装载「地图 + 图库」，并把结果（成功/失败）全部写进 `stderr`。
    ///
    /// `map_path` 传 `None` 时用 [`DEFAULT_MAP_PATH`]；那个常量为空（公开仓没有内建缺省图）
    /// ⇒ **不打开任何图**，`map_error` 写「未指定地图」，界面如实显示，**不 panic**。
    pub fn load(map_path: Option<&Path>) -> Self {
        let path = map_path.map(Path::to_path_buf).filter(|p| !p.as_os_str().is_empty());
        let (doc, map_error) = match &path {
            Some(p) => Self::load_map(p),
            None => {
                let msg = tx::fill(tx::DIAG_MAP_NONE, &[]);
                eprintln!("{msg}");
                (None, Some(msg))
            }
        };
        // 图库（与地图互不影响：图库挂了地图照样开）
        let assets = load_assets();
        Self {
            doc,
            assets,
            map_error,
            map_gen: 1,
        }
    }

    /// **只装载地图**（不扫资源库）—— 这就是 GUI 的地图装载路径本身。
    ///
    /// 为什么单独拆出来：全仓 1,158 张图的开图 / 未改即存验收（`acceptance_tests/formats.rs`）
    /// 要的正是"这一条路径"，而扫 117 个资源库与逐字节比对毫无关系、只会把用例拖到几十分钟。
    /// 拆出来之后验收走的是**与生产完全相同**的 `read_map`（`Document::load`），
    /// 不是另写一条 `Map::parse` 的旁路。
    ///
    /// 返回 `(文档, 失败原因)`：成功时后者为 `None`，失败时前者为 `None`。
    pub fn load_map(path: &Path) -> (Option<MapDoc>, Option<String>) {
        match read_map(path) {
            Ok(d) => {
                eprintln!(
                    "{}",
                    tx::fill(
                        tx::DIAG_MAP_OK,
                        &[
                            ("path", &d.path.display().to_string()),
                            ("w", &d.map.width.to_string()),
                            ("h", &d.map.height.to_string()),
                            // 2026-11-05：`{fmt}` 从三档档位名换成**九格式人读名**
                            // （`Document::format_label`）—— 三档名对 Type5/Type1/… 是错的投影。
                            ("fmt", d.format_label()),
                            ("bytes", &d.bytes.to_string()),
                        ]
                    )
                );
                // 未解尾块 / 格式未承诺编辑语义：**开机就说**（界面另有常驻黄条）
                if let Some(w) = d.edit_warning() {
                    eprintln!("{}", tx::fill(tx::DIAG_MAP_WARN, &[("warn", &w)]));
                }
                (Some(d), None)
            }
            Err(e) => {
                eprintln!(
                    "{}",
                    tx::fill(
                        tx::DIAG_MAP_FAIL,
                        &[("path", &path.display().to_string()), ("err", &e)]
                    )
                );
                (None, Some(e))
            }
        }
    }

    /// 地图文件名（没打开时给 `—`，**不编一个 `未命名.map`**）。
    pub fn file_name(&self) -> &str {
        match &self.doc {
            Some(d) => &d.file_name,
            None => tx::VAL_UNKNOWN,
        }
    }

    /// 标题栏 / 窗口标题用的完整标题（模板见 [`crate::ui_text::TITLE_TMPL`]）。
    ///
    /// ⚠ **上屏的标题不直接用本函数**：有未保存修改时要在文件名后加 ` *`
    /// （模板 [`crate::ui_text::TITLE_DIRTY_TMPL`]），那件事由
    /// [`crate::screen_text::build`] 按编辑会话的脏标记装配。本函数只给
    /// "干净状态下的标题"与测试用，两条路径用的是同一套模板常量。
    pub fn title(&self) -> String {
        match &self.doc {
            Some(d) => tx::fill(
                tx::TITLE_TMPL,
                &[
                    ("name", &d.file_name),
                    ("w", &d.map.width.to_string()),
                    ("h", &d.map.height.to_string()),
                ],
            ),
            None => tx::WINDOW_TITLE_EMPTY.to_owned(),
        }
    }
}

/// 路径 → 文件名（含扩展名）；**取不到文件名时退回整条路径**（与
/// `miredit_core::Document::from_bytes` 的 `file_name` 口径逐字相同，见
/// `miredit-core/src/document.rs:122-125`）。
///
/// 为什么不能自己另写一套：另存为之后 `MapDoc::file_name` 必须与"内核给这张图起的名字"
/// 一致，否则重新打开同一个文件会显示另一个名字（两套真值）。
fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

/// 读并解析一张 `.map`；错误一律折成**中文可读原因**（含具体数值，便于定位）。
///
/// 解析**委托给 `miredit_core::Document::load`**（它内部是 `std::fs::read` +
/// `mir2_format::read_any_tolerant`，九种格式的判据全在 `mir2-format` 里）；本函数只把
/// 内核的结构化错误折回**与迁移前逐字相同**的字符串（见 [`core_err_text`]），
/// 再把内核文档整份收进 [`MapDoc`]（`raw` 是真值、`view` 克隆一份给宿主编辑）。
///
/// ⚠ **迁移前**这里是 `mir2_map::Map::parse` —— 它只认 12 / 14 / 36 三档，
/// Type5 / Type1 / Type4 / Mir3 / CrystalCs 一律 `LengthMismatch` 打不开，
/// 67 张带尾块的真实图也一律拒绝。**现在九种 + 宽松尾块全部能开**，
/// 且 `raw` 从打开那一刻起就是真文档（`sync_document_format` 因此在生产路径上永远早退，
/// 见 `edit.rs` 的同名方法）。
fn read_map(path: &Path) -> Result<MapDoc, String> {
    let document = miredit_core::Document::load(path).map_err(core_err_text)?;
    Ok(MapDoc::from_document(document))
}

/// `CoreError` → **与迁移前逐字相同**的中文原因。
///
/// 为什么要拆：迁移前这里是 `std::fs::read` 的 `io::Error::to_string()` 与
/// `Map::parse` 的 `MapError::to_string()`；`miredit_core::Document::load` 把两者
/// 分别包成 `CoreError::Io { detail }` / `CoreError::Map { err }`，而 `detail` / `err`
/// **就是原来那两个串**（`FormatError` 的 `Display` 同样写清了"长度不符/尺寸非法"与具体数值）。
/// 这里只把包装层剥掉 —— 不加工、不改写、不加前缀，
/// 于是状态栏 / stderr 上的文案结构一个字符都不变。
fn core_err_text(e: miredit_core::CoreError) -> String {
    match e {
        miredit_core::CoreError::Io { detail, .. } => detail,
        miredit_core::CoreError::Map { err, .. } => err.to_string(),
        // 另外两种（`BadLayer` / `Save`）在本函数里不可达；万一将来可达，如实报全
        other => other.to_string(),
    }
}

/// 扫描并打开图库；任何一步失败都返回 [`Assets::Failed`]（原因原样带出）。
///
/// 根目录走 [`mir2_assets::discover_default`]（读 `MIREDIT_CLIENT_ROOT`）：
/// 未设置 ⇒ [`AssetError::RootUnset`]，界面与 stderr 都如实说「未设置哪个环境变量」，
/// **不内置任何开发机路径、不 panic**。
fn load_assets() -> Assets {
    let paths = match mir2_assets::discover_default() {
        Ok(p) => p,
        Err(e) => return Assets::Failed(asset_err(&e)),
    };
    let src = match WilSource::open(&paths) {
        Ok(s) => s,
        Err(e) => return Assets::Failed(asset_err(&e)),
    };
    eprintln!(
        "{}",
        tx::fill(
            tx::DIAG_LIB_OK,
            &[
                ("n", &paths.available().to_string()),
                ("g", &paths.available_in(Family::Ground).to_string()),
                ("m", &paths.available_in(Family::Middle).to_string()),
                ("s", &paths.available_in(Family::Surface).to_string()),
                ("root", &paths.root().display().to_string()),
            ]
        )
    );
    // `src` 装箱：见 [`Assets`] 的文档（`large_enum_variant`）
    Assets::Ready {
        paths,
        src: Box::new(src),
    }
}

/// 把 [`AssetError`] 折成一句话（`Display` 已经写清了哪个库/哪个文件为什么失败）。
fn asset_err(e: &AssetError) -> String {
    let s = e.to_string();
    eprintln!("{}", tx::fill(tx::DIAG_LIB_FAIL, &[("err", &s)]));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 缺省地图路径**必须是空的**：公开仓不内置任何开发机路径。
    ///
    /// 这条挡的是「有人图省事又把某台机器的绝对路径写回来」——那会让公开 clone 后
    /// 一启动就去摸一个别人的盘符，且在没有磁盘 D/E 的机器上静默失败。
    #[test]
    fn 缺省地图路径为空() {
        assert_eq!(DEFAULT_MAP_PATH, "", "DEFAULT_MAP_PATH 必须为空（未指定）");
    }

    /// 没给 `--map`（且 `DEFAULT_MAP_PATH` 为空）⇒ **不打开任何图**，但如实给出原因。
    ///
    /// 判据三条：① `doc` 为空；② `map_error` 是「未指定地图」那一条（不是空串）；
    /// ③ 图库照旧被装载（地图与图库互不影响）。
    #[test]
    fn 没给地图时如实报未指定() {
        let st = DocState::load(None);
        assert!(st.doc.is_none(), "没有缺省图 ⇒ 不该打开任何地图");
        assert_eq!(st.map_error.as_deref(), Some(tx::DIAG_MAP_NONE));
        assert_eq!(st.file_name(), tx::VAL_UNKNOWN);
    }

    /// 打开一个**不存在**的路径：必须拿到 `None` + 具体原因，且原因里带路径。
    #[test]
    fn 打不开的地图如实报错() {
        let bad = Path::new(r"E:\__no_such_map__.map");
        let st = DocState::load(Some(bad));
        assert!(st.doc.is_none(), "不存在的图不该被打开");
        let err = st.map_error.as_deref().expect("必须有失败原因");
        assert!(!err.is_empty());
        assert_eq!(st.file_name(), tx::VAL_UNKNOWN);
        assert_eq!(st.title(), tx::WINDOW_TITLE_EMPTY);
    }

    /// 手工造一张 2×2 的 Old12 地图能解析出来，且宽高/档位如实。
    #[test]
    fn 解析小地图() {
        let mut bytes = vec![0u8; 52 + 12 * 4];
        bytes[0..2].copy_from_slice(&2u16.to_le_bytes());
        bytes[2..4].copy_from_slice(&2u16.to_le_bytes());
        let dir = std::env::temp_dir();
        let p = dir.join("miredit_gui_unit_2x2.map");
        std::fs::write(&p, &bytes).expect("写临时地图");
        let d = read_map(&p).expect("应能解析");
        assert_eq!(d.size(), (2, 2));
        assert_eq!(d.format_name(), "Old12");
        assert_eq!(d.bytes, 52 + 12 * 4);
        let _ = std::fs::remove_file(&p);
    }

    /// **`retarget` 改三处出处**（另存为的"以后认哪个路径"）：宿主 path / 宿主 file_name /
    /// 内核 Document 的同名字段，**三处必须一致**。
    ///
    /// 这条挡的是"标题改了、保存还写旧路径"（那正是另存为最容易出的数据事故）。
    #[test]
    fn 另存为改路径后三处出处一致() {
        let mut bytes = vec![0u8; 52 + 12 * 4];
        bytes[0..2].copy_from_slice(&2u16.to_le_bytes());
        bytes[2..4].copy_from_slice(&2u16.to_le_bytes());
        let dir = std::env::temp_dir().join("miredit_gui_retarget");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建临时目录");
        let a = dir.join("a.map");
        std::fs::write(&a, &bytes).expect("写临时地图");

        let mut d = read_map(&a).expect("应能解析");
        assert_eq!(d.path, a);
        assert_eq!(d.file_name, "a.map");
        // 标题/状态栏用的名字（同一份 `file_name`）
        assert!(d.file_name == "a.map");

        let b = dir.join("b.map");
        d.retarget(&b);
        assert_eq!(d.path, b, "宿主 path 必须指向新路径");
        assert_eq!(d.file_name, "b.map", "标题/状态栏读的是它");
        assert_eq!(d.document.path, b, "内核 Document 的 path 也要改");
        assert_eq!(d.document.file_name, "b.map", "内核 Document 的 file_name 也要改");
        // 改路径**不动字节**：内存里的图仍是原来那份（写盘是 `save_as_session` 的事）
        assert_eq!(d.bytes, 52 + 12 * 4);
        assert!(!b.exists(), "retarget 自己不许写盘");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 取不到文件名时退回整条路径（与内核 `Document::from_bytes` 同口径）。
    ///
    /// 用 `std::env::temp_dir() + 进程号` 造前缀（**不钉盘符**）：这条测的是
    /// `Path::file_name()` 的取值口径，与"在哪个盘"无关 —— 钉死 `I:\` 之类的字面量
    /// 会让非 Windows / 无该盘的机器上无意义地红。
    #[test]
    fn 取不到文件名时退回整条路径() {
        let dir = std::env::temp_dir().join(format!("miredit_gui_fname_{}", std::process::id()));
        let map = dir.join("b.map");
        assert_eq!(file_name_of(&map), "b.map");
        assert_eq!(
            file_name_of(&dir),
            dir.file_name().and_then(|s| s.to_str()).unwrap_or_default(),
            "指到目录时要能取到最后一段"
        );
        assert!(!file_name_of(&dir).is_empty(), "目录也不许退化成空串");
        // 真正的"没有文件名"：根 ⇒ 退回整条路径（不许变成空串）
        let root = dir.ancestors().last().expect("至少有一段");
        assert_eq!(file_name_of(root), root.display().to_string());
        assert!(!file_name_of(root).is_empty());
    }

    /// 图库失败态的库数必须是 0（不是 117 也不是 1）。
    #[test]
    fn 图库失败态库数为零() {
        let a = Assets::Failed("x".to_string());
        assert_eq!(a.lib_count(), 0);
        assert!(a.source().is_none());
        assert_eq!(a.error(), Some("x"));
    }
}
