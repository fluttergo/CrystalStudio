//! **地图列表面板**：把上游 `901dbe2f` 新增的「地图列表」能力移植到本仓。
//!
//! # 一、上游依据（逐条对得上，别当成"我想这么做的"）
//!
//! | 上游 | 本模块 |
//! |---|---|
//! | `CrystalStudio/MapFileCatalog.cs:12-29` —— 递归枚举 `*.map`，**只建文件索引**（`FilePath` / `RelativePath`），注释原文「只建立文件索引，点击后才解析地图；跳过目录链接，避免重复或循环扫描」 | [`Scanner`] / [`scan_all`]：只 `read_dir` + `entry.metadata()`，**一个字节的地图内容都不读** |
//! | `MainWindow.MapBrowser.cs`（整个文件 125 行）—— `ListBox` + 22px 行高 + `FontSize 11` + `Tapped` 才 `OpenMap(entry.FilePath)` | [`MapListPanel`]：行高 [`m::MAPS_ROW_H`] 22、字号 [`m::MAPS_ROW_FONT`] 11、**点击才走 [`crate::menu::action::open_path`]** |
//! | `MapBrowser.cs:37` —— 行内文字 = `entry.RelativePath`，`SetTip(entry.FilePath)` | 行内文字 = 相对路径（[`MapEntry::rel`]），悬停提示 = 绝对路径 + 大小 + 修改时间 |
//! | `MapBrowser.cs:115-116` —— 空目录文案 `No .map files in the Map folder.` / 数量 `{0:N0} maps` | [`MapListPanel::status_text`]：空目录与扫描失败都给**中文原因**，不静默 |
//! | `MapBrowser.cs:92-97` —— `catch (IOException / UnauthorizedAccessException)` ⇒ `Could not read map folder` + 原因 | [`Phase::Failed`]：中文原因 + stderr 诊断行 |
//!
//! **排序**：上游是 `OrderBy(RelativePath, OrdinalIgnoreCase)`；本仓按任务书要求改成
//! **文件名自然序**（[`natural_cmp`]：数字段按数值比较，`n2` 在 `n10` 之前）——
//! 上游那种字典序会把 `n10` 排在 `n2` 前面，1000+ 张高清图里翻起来很难受。
//!
//! # 二、"不解析地图"是怎么保证的（不是靠"代码看起来像"）
//!
//! 三道结构闸门 + 一条实测判据：
//!
//! 1. **只调 `read_dir` / `DirEntry::metadata()`**：全模块**没有** `File::open` /
//!    `fs::read` / `Document::load` / `mir2_map::Map::parse`；
//! 2. **[`MapEntry`] 里没有"地图宽高/格式"字段** —— 想放也没地方放，判别格式必须读头，
//!    这里连头都不读；
//! 3. **打开是另一条路**：点击 → [`crate::app::MireditApp::open_map_path`] →
//!    [`crate::menu::action::open_path`] → `Document::load`（与「打开地图…」模态同一条）。
//!
//! 实测判据见 `maplist_tests::实测扫描耗时与只读元数据`：1,097 张 / 697.2 MB 的目录
//! 扫完约 **10~14 ms**；而**读一张** 5.88 MB 的 `0.map` 就要 ~10 ms ⇒ 若真读了内容，
//! 1097 张至少要 11 s（≈ 1000 倍）。实测差三个数量级 ⇒ 只读了元数据。
//!
//! # 三、为什么分帧扫描（以及实测到底要不要分）
//!
//! 任务书问「一次扫完 1,097 个 `File::metadata` 大概多少 ms」：实测 **10~14 ms**
//! （见上），**低于**任务书给的 100 ms 阈值 ⇒ 单次扫完在本机是可行的。
//!
//! 但本模块**仍然分帧**，理由不是"1,097 张会卡"，而是**目录规模不可控**：
//! 上游枚举是递归的（`RecurseSubdirectories = true`），用户完全可以把根指到
//! 客户端根目录（几万个文件）；网络盘/机械盘上单次 `metadata` 可能到毫秒级
//! ⇒ "单次扫完"在那种输入下就是几秒的白屏。所以这里按
//! **每帧 [`m::MAPS_SCAN_BUDGET_MS`] 4 ms + 最多 [`m::MAPS_SCAN_MAX_ITEMS`] 512 项**
//! 两道闸门推进（[`Scanner::step`]），扫的过程中面板照常画、照常响应点击。
//!
//! 分帧的代价只有一个：面板要先显示「扫描中…」再显示列表。为了不让验收截图拍到
//! 半成品，[`MapListPanel::is_scanning`] 为真时宿主会每帧 `request_repaint()`
//! （见 `app::update` 的 ⑤d），1,097 张在 3~4 帧内就扫完。
//!
//! # 四、面板放哪（**与上游不同的地方，必须显式说明**）
//!
//! 上游把这块面板 `docking.Add("Maps", …, Dock.Left, 2)` 放在**左停靠区第一位**
//! （`MainWindow.axaml.cs:343`）。本仓左停靠区是 `layout.rs` 里的**固定几何**
//! （156 宽、四块面板首尾相接），实测单元格检查器在最坏文案下只剩 **96px** 余量
//! （`layout::tests::左停靠区余量`）—— 再塞第五块面板必然把检查器最后两行挤出面板下沿
//! （那就是"遮挡/错位"）。
//!
//! 所以本仓把地图列表做成**中列左侧的可开合列**（[`crate::layout::Layout::compute_with`]）：
//!
//! * **开**：画布往右让出 [`m::MAPS_W`] 268px，面板**不盖地图**（不是浮层）；
//! * **关**（默认）：画布矩形与旧版**逐像素相同** ⇒ 默认态不变，
//!   `--gate2-client` 的同视点比对基线不受影响。
//!
//! 默认关是**有意偏离上游**（上游默认可见）：本仓 1500×950 的默认帧是与客户端
//! 逐像素比对的基线（[`crate::selfcheck::gate2`]），默认就少 268px 画布会把那条基线
//! 直接作废。开它只需一次「视图 → 面板 → 地图列表」，或命令行 `--show-maps`。
//!
//! # 五、本模块的文案
//!
//! 全部来自 [`crate::ui_text`]（`MAPLIST_*` / `TITLE_MAPS` / `SIZE_*`），
//! 代码里**不出现界面字面量**；数字/文件名/路径是运行期数据。

use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use egui::{pos2, vec2, Align2, Painter, Rect, Sense};

use crate::layout::Layout;
use crate::metrics as m;
use crate::paint as pt;
use crate::panel;
use crate::text_fit::{DynLabel, LabelStyle};
use crate::theme as t;
use crate::ui_text as tx;

// ───────────────────────── 索引项 ─────────────────────────

/// 一条地图文件索引项 —— **只有元数据，没有地图内容**。
///
/// 刻意**不放** `width` / `height` / `format`：那三样都必须读文件头才能得到，
/// 一旦放进结构体，列表阶段就会有人顺手填上 ⇒ "只建索引"的纪律当场破功。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapEntry {
    /// 绝对路径（点击打开用）。
    pub path: PathBuf,
    /// 相对扫描根的路径（列表显示用；扁平目录下就是文件名）。
    pub rel: String,
    /// 文件字节数（`DirEntry::metadata().len()`）。
    pub bytes: u64,
    /// 修改时间；取不到（文件系统不支持/权限）就是 `None`，**绝不编一个值**。
    pub mtime: Option<SystemTime>,
}

/// 扫描阶段。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    /// 还没有扫描根（不知道扫哪儿）。
    #[default]
    Idle,
    /// 正在扫（分帧进行中）。
    Scanning,
    /// 扫完了（**可能是 0 条**，那就是空目录）。
    Ready,
    /// 失败（中文原因；不静默）。
    Failed(String),
}

// ───────────────────────── 自然序 ─────────────────────────

/// 判断一个路径是不是地图文件（扩展名 `.map`，**大小写不敏感**）。
///
/// 与上游 `MapFileCatalog.IsMapFile`（`OrdinalIgnoreCase`）同一口径。
pub fn is_map_file(path: &Path) -> bool {
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => ext.eq_ignore_ascii_case("map"),
        None => false,
    }
}

/// 取一段连续 ASCII 数字的**规范化形态**：去掉前导零，保留至少一位。
///
/// 为什么用"去零后的字符"而不是 `u64`：文件名里的数字段可以任意长
/// （`n0000000000000000001.map`），解析成整数会溢出；去零后
/// **先比长度、再比字典序** 就是任意精度的数值比较。
fn strip_leading_zeros(digits: &[char]) -> Vec<char> {
    let mut i = 0;
    while i + 1 < digits.len() && digits[i] == '0' {
        i += 1;
    }
    digits[i..].to_vec()
}

/// 两段数字按**数值**比较（任意长度，不溢出）。
fn cmp_digits(a: &[char], b: &[char]) -> Ordering {
    let (na, nb) = (strip_leading_zeros(a), strip_leading_zeros(b));
    // 长度不同 ⇒ 长的那个数值大（已去前导零，不会误判 `007` vs `7`）
    match na.len().cmp(&nb.len()) {
        Ordering::Equal => na.cmp(&nb),
        other => other,
    }
}

/// **文件名自然序**：数字段按数值比较，其余按 ASCII 小写比较（大小写不敏感）。
///
/// 判据（`maplist_tests::自然序`）：
/// * `n2.map` < `n10.map`（字典序会反过来）；
/// * `n0104.map` 与 `n104.map` 数值相等 ⇒ 落到最后的原始字节比较（`0` < `1`）；
/// * 大小写不敏感：`A.map` 与 `a.map` 相邻，不因大小写散开。
///
/// 总序性：主键是"字符/数字段序列"，末位用原始字符串兜底 ⇒ 不同字符串永不为
/// `Equal`，排序结果**可复现**（不依赖输入顺序）。
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let av: Vec<char> = a.chars().collect();
    let bv: Vec<char> = b.chars().collect();
    let (mut i, mut j) = (0usize, 0usize);
    while i < av.len() && j < bv.len() {
        let (ca, cb) = (av[i], bv[j]);
        if ca.is_ascii_digit() && cb.is_ascii_digit() {
            let mut ia = i;
            while ia < av.len() && av[ia].is_ascii_digit() {
                ia += 1;
            }
            let mut ib = j;
            while ib < bv.len() && bv[ib].is_ascii_digit() {
                ib += 1;
            }
            match cmp_digits(&av[i..ia], &bv[j..ib]) {
                Ordering::Equal => {
                    i = ia;
                    j = ib;
                }
                other => return other,
            }
            continue;
        }
        match ca.to_ascii_lowercase().cmp(&cb.to_ascii_lowercase()) {
            Ordering::Equal => {
                i += 1;
                j += 1;
            }
            other => return other,
        }
    }
    // 一方是另一方的前缀：短的在前
    match (i < av.len(), j < bv.len()) {
        (true, false) => return Ordering::Greater,
        (false, true) => return Ordering::Less,
        _ => {}
    }
    // 主键相等但字符串不同（`n007` vs `n7`、`A` vs `a`）⇒ 原始字节兜底，保证可复现
    a.cmp(b)
}

// ───────────────────────── 扫描器（分帧） ─────────────────────────

/// 一次扫描的统计（诊断/验收用）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScanStats {
    /// 找到的 `*.map` 条数。
    pub entries: usize,
    /// 走过的目录数。
    pub dirs: usize,
    /// `read_dir` 给出的条目总数（含非 `.map`）。
    pub seen: usize,
    /// 跳过的条目数（符号链接/重解析点、非常规文件）。
    pub skipped: usize,
    /// 读不了的目录/条目数（上游 `IgnoreInaccessible` 是静默的，本仓只跳过但**计数**）。
    pub errors: usize,
    /// **纯扫描耗时**（毫秒，不含调用方等待的帧间隔）。
    pub ms: f64,
    /// 分了几步（单次扫完 = 1）。
    pub steps: usize,
}

/// **增量扫描器**：每次 [`step`](Self::step) 只干一小段，干不完下次接着干。
///
/// 为什么不用线程：本仓没有异步运行时，起线程要自己管 join 与生命周期；而扫描本来就是
/// IO 密集且总量可控（1,097 张 ≈ 12 ms），分帧后单帧最多 4 ms，比线程间同步更简单、
/// 也更好测（`step` 是纯函数式的：给定预算推进一步，没有隐藏状态）。
pub struct Scanner {
    root: PathBuf,
    /// 待遍历目录栈（深度优先；`ReadDir` 本身就是迭代器，可以跨帧持有）。
    stack: Vec<fs::ReadDir>,
    entries: Vec<MapEntry>,
    stats: ScanStats,
    /// 前几条错误原文（最多 [`Scanner::MAX_ERRORS`] 条，其余只计数）。
    errors: Vec<String>,
    done: bool,
    /// 已经累计的扫描耗时（不含帧间等待）。
    elapsed: Duration,
}

impl Scanner {
    /// 最多保留几条错误原文（再多也没人看，只留计数）。
    pub const MAX_ERRORS: usize = 3;

    /// 开一次扫描。根不存在/不是目录 ⇒ `Err(中文原因)`（**不静默**）。
    pub fn new(root: &Path) -> Result<Self, String> {
        let md = fs::metadata(root).map_err(|e| {
            tx::fill(
                tx::MAPLIST_ERR_ROOT,
                &[("path", &root.display().to_string()), ("why", &e.to_string())],
            )
        })?;
        if !md.is_dir() {
            return Err(tx::fill(
                tx::MAPLIST_ERR_NOT_DIR,
                &[("path", &root.display().to_string())],
            ));
        }
        let first = fs::read_dir(root).map_err(|e| {
            tx::fill(
                tx::MAPLIST_ERR_ROOT,
                &[("path", &root.display().to_string()), ("why", &e.to_string())],
            )
        })?;
        let mut s = Self {
            root: root.to_path_buf(),
            stack: vec![first],
            entries: Vec::new(),
            stats: ScanStats::default(),
            errors: Vec::new(),
            done: false,
            elapsed: Duration::ZERO,
        };
        s.stats.dirs = 1;
        Ok(s)
    }

    /// 扫描根。
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 是否已经扫完。
    pub fn is_done(&self) -> bool {
        self.done
    }

    /// 目前为止找到的条数（扫描中也能看，用来显示进度）。
    pub fn found(&self) -> usize {
        self.entries.len()
    }

    /// 前几条错误原文。
    pub fn errors(&self) -> &[String] {
        &self.errors
    }

    /// 记一条错误（最多留 [`Self::MAX_ERRORS`] 条原文，其余只计数）。
    fn note_error(&mut self, text: String) {
        self.stats.errors += 1;
        if self.errors.len() < Self::MAX_ERRORS {
            self.errors.push(text);
        }
    }

    /// 处理一个目录项（**只读元数据**）。
    fn visit(&mut self, ent: &fs::DirEntry) {
        self.stats.seen += 1;
        let path = ent.path();
        let Ok(ft) = ent.file_type() else {
            self.stats.skipped += 1;
            return;
        };
        // 上游 `AttributesToSkip = FileAttributes.ReparsePoint` ⇒ 符号链接/联接点一律跳过
        // （否则 `Map\link` 指回父目录就会无限扫）。
        if ft.is_symlink() {
            self.stats.skipped += 1;
            return;
        }
        if ft.is_dir() {
            match fs::read_dir(&path) {
                Ok(rd) => {
                    self.stats.dirs += 1;
                    self.stack.push(rd);
                }
                // 读不了就跳过，但**记下来**（面板状态行会显示"跳过 N 项"）。
                Err(e) => self.note_error(tx::fill(
                    tx::MAPLIST_ERR_DIR,
                    &[("path", &path.display().to_string()), ("why", &e.to_string())],
                )),
            }
            return;
        }
        if !ft.is_file() || !is_map_file(&path) {
            return;
        }
        // ⚠ **这里就是"只读元数据"的全部**：`DirEntry::metadata()` 取的是目录项自带的
        // 文件属性（Windows 上来自 `FindFirstFile`/`FindNextFile` 的 `WIN32_FIND_DATA`，
        // 不打开文件、不读一个字节的内容）。
        let md = match ent.metadata() {
            Ok(v) => v,
            Err(e) => {
                self.note_error(tx::fill(
                    tx::MAPLIST_ERR_FILE,
                    &[("path", &path.display().to_string()), ("why", &e.to_string())],
                ));
                return;
            }
        };
        let rel = match path.strip_prefix(&self.root) {
            Ok(p) => p.to_string_lossy().to_string(),
            // strip 失败（理论上不会：path 一定在 root 下）⇒ 退回文件名，绝不留空串
            Err(_) => path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
        };
        self.entries.push(MapEntry {
            path,
            rel,
            bytes: md.len(),
            mtime: md.modified().ok(),
        });
    }

    /// 推进一步：**最多**花 `budget` 时间、处理 `max_items` 个目录项。
    ///
    /// 返回 `true` = 还没扫完（调用方应该下一帧接着调）。
    pub fn step(&mut self, budget: Duration, max_items: usize) -> bool {
        if self.done {
            return false;
        }
        let start = Instant::now();
        let mut items = 0usize;
        // 批量上限至少 1：调用方传 0 的话下面 `items >= max_items` 永远成立 ⇒ 活锁
        let cap = max_items.max(1);
        loop {
            // ⚠ 预算检查必须排在"至少干一项"之后：`items == 0` 时无条件放行一项。
            // 否则调用方给一个极小的 budget（如 `Duration::ZERO`）会让每次 `step`
            // 都在"还没干活"时就返回 `true` ⇒ 调用方 `while step() {}` 直接活锁。
            if items > 0 && (start.elapsed() >= budget || items >= cap) {
                self.elapsed += start.elapsed();
                self.stats.steps += 1;
                return true;
            }
            let Some(dir) = self.stack.last_mut() else {
                // 栈空 = 扫完了
                self.elapsed += start.elapsed();
                self.stats.steps += 1;
                self.finish();
                return false;
            };
            match dir.next() {
                None => {
                    self.stack.pop();
                }
                Some(Err(e)) => {
                    items += 1;
                    self.note_error(tx::fill(tx::MAPLIST_ERR_ENTRY, &[("why", &e.to_string())]));
                }
                Some(Ok(ent)) => {
                    items += 1;
                    self.visit(&ent);
                }
            }
        }
    }

    /// 收尾：排序 + 定格统计。
    fn finish(&mut self) {
        self.done = true;
        self.entries.sort_by(|a, b| natural_cmp(&a.rel, &b.rel));
        self.stats.entries = self.entries.len();
        self.stats.ms = self.elapsed.as_secs_f64() * 1000.0;
    }

    /// 取走结果（未扫完时先扫完 —— 只给测试/无窗路径用，**不要**在 `update` 里调）。
    pub fn into_parts(mut self) -> (Vec<MapEntry>, ScanStats, Vec<String>) {
        while !self.done {
            self.step(Duration::from_secs(3600), usize::MAX);
        }
        (self.entries, self.stats, self.errors)
    }
}

/// **一次性扫完**（阻塞；验收/无窗路径与单测用，GUI 走 [`Scanner::step`] 分帧）。
///
/// 语义与分帧版**完全相同**（同一个 `Scanner`，只是把预算放大到一次做完）。
pub fn scan_all(root: &Path) -> Result<(Vec<MapEntry>, ScanStats, Vec<String>), String> {
    let mut s = Scanner::new(root)?;
    while !s.is_done() {
        s.step(Duration::from_secs(3600), usize::MAX);
    }
    Ok(s.into_parts())
}

// ───────────────────────── 大小 / 时间格式化 ─────────────────────────

/// 把字节数格成「数字 + 单位」（1024 进制，KB/MB 1 位小数、GB 2 位）。
///
/// 单位串全部来自 [`crate::ui_text`]（`SIZE_UNIT_*`），不在代码里写字面量。
/// 判据见 `maplist_tests::大小格式化`：`0 → "0 B"`、`1023 → "1023 B"`、
/// `1024 → "1.0 KB"`。
pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    let (num, unit) = if bytes < KB {
        (bytes.to_string(), tx::SIZE_UNIT_B)
    } else if bytes < MB {
        (format!("{:.1}", bytes as f64 / KB as f64), tx::SIZE_UNIT_KB)
    } else if bytes < GB {
        (format!("{:.1}", bytes as f64 / MB as f64), tx::SIZE_UNIT_MB)
    } else {
        (format!("{:.2}", bytes as f64 / GB as f64), tx::SIZE_UNIT_GB)
    };
    tx::fill(tx::SIZE_VALUE_TMPL, &[("num", &num), ("unit", unit)])
}

/// 把「距 1970-01-01 的天数」换算成公历年月日（Howard Hinnant 的 `civil_from_days`）。
///
/// 为什么要自己算：本 crate **没有任何时间库**（依赖纪律：只用 egui/eframe + 同 workspace
/// 的库）。`SystemTime` 只能拿到"距 `UNIX_EPOCH` 的时长"，想显示 `2026-11-05 18:41`
/// 就得自己做历法换算。算法对 1970 前后都成立。
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let mth = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if mth <= 2 { y + 1 } else { y }, mth, d)
}

/// 把修改时间格成 `YYYY-MM-DD HH:MM`（**UTC**，不是本地时区）。
///
/// 为什么用 UTC：本 crate 没有时区库，拿本地时区要么再引依赖、要么调
/// `GetTimeZoneInformation`（unsafe）。列表里显示时间只是为了"这是哪一版"，
/// 差几个小时的 UTC 值不影响判断 —— 但**必须标清是 UTC**，否则就是假信息。
pub fn format_mtime(t: SystemTime) -> Option<String> {
    let d = t.duration_since(UNIX_EPOCH).ok()?;
    let secs = d.as_secs();
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (y, mo, da) = civil_from_days(days);
    Some(format!(
        "{y:04}-{mo:02}-{da:02} {:02}:{:02}",
        rem / 3600,
        (rem % 3600) / 60
    ))
}

// ───────────────────────── 面板状态 ─────────────────────────

/// 地图列表面板的**状态**（绘制与交互共用同一份，几何来自 [`Layout`]）。
#[derive(Default)]
pub struct MapListPanel {
    root: Option<PathBuf>,
    entries: Vec<MapEntry>,
    phase: Phase,
    scanner: Option<Scanner>,
    /// 列表顶部行号（滚动位置）。
    top: usize,
    /// 上一次自动滚动到的当前项（避免每帧抢滚动）。
    scrolled_to: Option<PathBuf>,
    /// 扫描中已找到的条数（状态行显示进度用）。
    scanning_found: usize,
    /// 最近一次扫描的统计（状态行与诊断行用）。
    stats: ScanStats,
    /// 上一次扫描的**墙钟**耗时（含分帧等待，用来对比"纯扫描耗时"）。
    wall_ms: f64,
    /// 上一次扫描开始时刻。
    started: Option<Instant>,
    /// 最近一次扫描的错误原文（最多 3 条，进 stderr 诊断）。
    errors: Vec<String>,
}

impl MapListPanel {
    /// 空面板（没有根、没有条目）。宿主启动时用这个。
    pub fn new() -> Self {
        Self::default()
    }

    /// 当前扫描根。
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// 当前阶段。
    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    /// 是否正在扫描（宿主据此每帧请求重绘）。
    pub fn is_scanning(&self) -> bool {
        matches!(self.phase, Phase::Scanning)
    }

    /// 已扫到的条数（扫描中是进度，扫完是总数）。
    pub fn len(&self) -> usize {
        if self.is_scanning() {
            self.scanning_found
        } else {
            self.entries.len()
        }
    }

    /// 是否一条都没有（`Ready` 且 0 条 = 空目录）。
    pub fn is_empty(&self) -> bool {
        !self.is_scanning() && self.entries.is_empty()
    }

    /// 全部条目（只读）。
    pub fn entries(&self) -> &[MapEntry] {
        &self.entries
    }

    /// 最近一次扫描的统计。
    pub fn stats(&self) -> &ScanStats {
        &self.stats
    }

    /// 最近一次扫描的墙钟耗时（毫秒）。
    pub fn wall_ms(&self) -> f64 {
        self.wall_ms
    }

    /// 最近一次扫描的错误原文。
    pub fn errors(&self) -> &[String] {
        &self.errors
    }

    /// 列表顶部行号。
    pub fn top(&self) -> usize {
        self.top
    }

    /// **开始扫一个根**（重复调同一个根会重新扫 —— 那是"刷新"的语义）。
    ///
    /// 根不可用（不存在/不是目录）⇒ 阶段直接进 [`Phase::Failed`]，条目清空，
    /// **不会**保留上一个根的结果（那会让人以为列表还是这个目录的）。
    pub fn start(&mut self, root: &Path) {
        self.root = Some(root.to_path_buf());
        self.entries.clear();
        self.scanner = None;
        self.top = 0;
        self.scrolled_to = None;
        self.scanning_found = 0;
        self.stats = ScanStats::default();
        self.wall_ms = 0.0;
        self.errors.clear();
        self.started = Some(Instant::now());
        match Scanner::new(root) {
            Ok(s) => {
                self.phase = Phase::Scanning;
                self.scanner = Some(s);
            }
            Err(why) => {
                eprintln!(
                    "{}",
                    tx::fill(
                        tx::DIAG_MAPLIST_FAIL,
                        &[("root", &root.display().to_string()), ("why", &why)]
                    )
                );
                self.phase = Phase::Failed(why);
            }
        }
    }

    /// **推进一步扫描**；返回 `true` = 还在扫（宿主应该下一帧再来 + 请求重绘）。
    ///
    /// 扫完的那一步会把结果收进面板并打一行诊断（`[地图列表] …`）。
    pub fn poll(&mut self) -> bool {
        let Some(mut sc) = self.scanner.take() else {
            return false;
        };
        let more = sc.step(
            Duration::from_millis(m::MAPS_SCAN_BUDGET_MS),
            m::MAPS_SCAN_MAX_ITEMS,
        );
        if more {
            self.scanning_found = sc.found();
            self.scanner = Some(sc);
            self.phase = Phase::Scanning;
            return true;
        }
        // 扫完了
        self.wall_ms = self
            .started
            .map(|t| t.elapsed().as_secs_f64() * 1000.0)
            .unwrap_or(0.0);
        let (entries, stats, errors) = sc.into_parts();
        self.entries = entries;
        self.stats = stats;
        self.errors = errors;
        self.phase = Phase::Ready;
        self.scanner = None;
        eprintln!(
            "{}",
            tx::fill(
                tx::DIAG_MAPLIST_SCAN,
                &[
                    (
                        "root",
                        &self
                            .root
                            .as_ref()
                            .map(|p| p.display().to_string())
                            .unwrap_or_default()
                    ),
                    ("n", &self.stats.entries.to_string()),
                    ("ms", &format!("{:.2}", self.stats.ms)),
                    ("wall", &format!("{:.2}", self.wall_ms)),
                    ("steps", &self.stats.steps.to_string()),
                    ("dirs", &self.stats.dirs.to_string()),
                    ("seen", &self.stats.seen.to_string()),
                    ("skipped", &self.stats.skipped.to_string()),
                    ("errors", &self.stats.errors.to_string()),
                ]
            )
        );
        for e in &self.errors {
            eprintln!("{e}");
        }
        false
    }

    /// 当前打开的地图在列表里的下标（路径比较大小写不敏感，Windows 语义）。
    pub fn index_of(&self, current: Option<&Path>) -> Option<usize> {
        let cur = current?;
        self.entries.iter().position(|e| same_path(&e.path, cur))
    }

    /// 把第 `idx` 行滚进可见区（只在必要时改 `top`）。
    pub fn scroll_to(&mut self, idx: usize, visible_rows: usize) {
        if visible_rows == 0 {
            return;
        }
        if idx < self.top {
            self.top = idx;
        } else if idx >= self.top + visible_rows {
            self.top = idx + 1 - visible_rows;
        }
        self.clamp_top(visible_rows);
    }

    /// 滚轮：按行滚动（正数 = 往下看后面的行）。
    pub fn scroll_rows(&mut self, rows: i64, visible_rows: usize) {
        let want = self.top as i64 + rows;
        self.top = want.max(0) as usize;
        self.clamp_top(visible_rows);
    }

    /// 夹住 `top`：不允许滚到"最后一行之上留白"。
    fn clamp_top(&mut self, visible_rows: usize) {
        let max_top = self.entries.len().saturating_sub(visible_rows);
        if self.top > max_top {
            self.top = max_top;
        }
    }

    /// **每帧的自动跟随**：当前地图变了（或刚扫完）就把那一行滚进可见区。
    ///
    /// 为什么"只跟一次"：用户手动滚走之后，每帧再把他拽回来就没法浏览别的图了。
    fn follow_current(&mut self, current: Option<&Path>, visible_rows: usize) {
        let Some(idx) = self.index_of(current) else {
            return;
        };
        let cur = current.map(Path::to_path_buf);
        if self.scrolled_to == cur {
            return;
        }
        self.scroll_to(idx, visible_rows);
        self.scrolled_to = cur;
    }

    /// 底部状态行的文案（**空目录/失败都给中文原因**）。
    pub fn status_text(&self) -> String {
        match &self.phase {
            Phase::Idle => tx::MAPLIST_NO_ROOT.to_owned(),
            Phase::Scanning => tx::fill(
                tx::MAPLIST_SCANNING_TMPL,
                &[("n", &self.scanning_found.to_string())],
            ),
            Phase::Failed(why) => tx::fill(tx::MAPLIST_FAIL_TMPL, &[("why", why)]),
            Phase::Ready if self.entries.is_empty() => tx::fill(
                tx::MAPLIST_EMPTY_TMPL,
                &[(
                    "path",
                    &self
                        .root
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default(),
                )],
            ),
            Phase::Ready => {
                let mut s = tx::fill(
                    tx::MAPLIST_STATUS_TMPL,
                    &[
                        ("n", &self.entries.len().to_string()),
                        ("ms", &format!("{:.1}", self.stats.ms)),
                    ],
                );
                if self.stats.skipped > 0 || self.stats.errors > 0 {
                    s.push_str(&tx::fill(
                        tx::MAPLIST_SKIPPED_TMPL,
                        &[
                            ("skipped", &self.stats.skipped.to_string()),
                            ("errors", &self.stats.errors.to_string()),
                        ],
                    ));
                }
                s
            }
        }
    }
}

/// 两个路径是否指向同一个文件（Windows 大小写不敏感，其它平台按字节比较）。
pub fn same_path(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    let (sa, sb) = (a.to_string_lossy(), b.to_string_lossy());
    if cfg!(windows) {
        sa.eq_ignore_ascii_case(&sb)
    } else {
        sa == sb
    }
}

// ───────────────────────── 几何 ─────────────────────────

/// 面板内工具条矩形（放「刷新」按钮）。
pub fn toolbar_rect(l: &Layout) -> Rect {
    let c = l.p_maps.content;
    Rect::from_min_max(c.min, pos2(c.right(), c.top() + m::MAPS_TOOLBAR_H))
}

/// 「刷新」按钮矩形。
pub fn refresh_rect(l: &Layout) -> Rect {
    let c = l.p_maps.content;
    Rect::from_min_size(
        pos2(
            c.left() + m::MAPS_PAD,
            c.top() + (m::MAPS_TOOLBAR_H - m::MAPS_BTN_H) * 0.5,
        ),
        vec2(m::MAPS_BTN_W, m::MAPS_BTN_H),
    )
}

/// 列表区矩形（工具条以下、状态行以上）。
pub fn list_rect(l: &Layout) -> Rect {
    let c = l.p_maps.content;
    Rect::from_min_max(
        pos2(c.left(), c.top() + m::MAPS_TOOLBAR_H),
        pos2(c.right(), c.bottom() - m::MAPS_STATUS_H),
    )
}

/// 底部状态行矩形。
pub fn status_rect(l: &Layout) -> Rect {
    let c = l.p_maps.content;
    Rect::from_min_max(pos2(c.left(), c.bottom() - m::MAPS_STATUS_H), c.max)
}

/// 列表区能显示几行（**面板关着时恒为 0**：零宽的面板一行都画不出来）。
pub fn visible_rows(l: &Layout) -> usize {
    if l.p_maps.whole.width() <= 0.0 {
        return 0;
    }
    let h = list_rect(l).height();
    if h <= 0.0 {
        return 0;
    }
    (h / m::MAPS_ROW_H).floor() as usize
}

/// 第 `row`（可见区内的 0 基序号）行的矩形。
pub fn row_rect(l: &Layout, row: usize) -> Rect {
    let list = list_rect(l);
    Rect::from_min_size(
        pos2(
            list.left() + m::MAPS_PAD,
            list.top() + row as f32 * m::MAPS_ROW_H,
        ),
        vec2(
            (list.width() - 2.0 * m::MAPS_PAD - m::MAPS_SCROLLBAR_W).max(0.0),
            m::MAPS_ROW_H,
        ),
    )
}

/// 滚动条轨道矩形。
pub fn scrollbar_rect(l: &Layout) -> Rect {
    let list = list_rect(l);
    Rect::from_min_max(
        pos2(list.right() - m::MAPS_PAD - m::MAPS_SCROLLBAR_W, list.top()),
        pos2(list.right() - m::MAPS_PAD, list.bottom()),
    )
}

// ───────────────────────── 绘制 ─────────────────────────

/// 画整个地图列表面板（标题条 + 工具条 + 列表 + 状态行）。
///
/// `current` = 当前打开的地图路径（用来高亮那一行）。
pub fn draw(p: &Painter, l: &Layout, st: &MapListPanel, current: Option<&Path>) {
    if l.p_maps.whole.width() <= 0.0 {
        return;
    }
    // 面板底 + 标题条（与左/右停靠区同一套骨架）
    panel::draw_panel(p, &l.p_maps, tx::TITLE_MAPS, "panel.maps", t::ui().panel_bg);
    // 与画布之间的 1px 分隔线（否则面板底色与画布底色在缩小视图下会糊在一起）
    pt::fill(
        p,
        Rect::from_min_size(
            pos2(l.p_maps.whole.right() - 1.0, l.p_maps.whole.top()),
            vec2(1.0, l.p_maps.whole.height()),
        ),
        t::ui().dock_divider,
    );

    // ── 工具条：「刷新」按钮 ──
    let btn = refresh_rect(l);
    pt::fill_round(p, btn, 3.0, t::ui().input_bg);
    pt::border(p, btn, t::ui().input_border);
    pt::text(
        p,
        btn.center(),
        Align2::CENTER_CENTER,
        tx::MAPLIST_REFRESH,
        m::MAPS_SIZE_FONT,
        t::ui().text,
    );

    // ── 列表 ──
    let list = list_rect(l);
    pt::fill(p, list, t::ui().input_bg);
    let n = visible_rows(l);
    let cur_idx = st.index_of(current);
    let top = st.top.min(st.entries.len());
    let pc = p.with_clip_rect(list);
    for row in 0..n {
        let idx = top + row;
        let Some(e) = st.entries.get(idx) else {
            break;
        };
        let r = row_rect(l, row);
        let is_cur = cur_idx == Some(idx);
        if is_cur {
            // 当前打开的那张：浅金底 + 金框 + 左侧金条（与图层行"当前编辑层"同一视觉语言）
            pt::fill(&pc, r, t::ui().accent_soft);
            pt::border(&pc, r, t::ui().accent_dark);
            pt::fill(
                &pc,
                Rect::from_min_size(r.min, vec2(m::MAPS_CURRENT_BAR_W, r.height())),
                t::ui().accent_dark,
            );
        }
        // 文件名（左列，按裁剪盒截断）
        let size_text = format_size(e.bytes);
        let size_w = pt::text_w(p, &size_text, m::MAPS_SIZE_FONT);
        let name_left = r.left() + m::MAPS_ROW_PAD;
        let name_right = (r.right() - m::MAPS_ROW_PAD - size_w - m::MAPS_COL_GAP).max(name_left);
        let mut name = DynLabel::new(
            "maplist.name",
            tx::MAPLIST_NAME_TMPL,
            e.rel.clone(),
            LabelStyle::new(
                m::MAPS_ROW_FONT,
                pos2(name_left, r.center().y),
                Align2::LEFT_CENTER,
                Rect::from_min_max(pos2(name_left, r.top()), pos2(name_right, r.bottom())),
                t::ui().text,
            ),
        );
        name.truncate_to_fit(p);
        name.draw(&pc);
        // 大小（右列，右对齐）
        pt::text(
            &pc,
            pos2(r.right() - m::MAPS_ROW_PAD, r.center().y),
            Align2::RIGHT_CENTER,
            &size_text,
            m::MAPS_SIZE_FONT,
            t::ui().text_muted,
        );
    }

    // ── 滚动条（只有超出一屏才画）──
    if st.entries.len() > n && n > 0 {
        let track = scrollbar_rect(l);
        let ratio = n as f32 / st.entries.len() as f32;
        let h = (track.height() * ratio).max(18.0);
        let max_top = st.entries.len().saturating_sub(n).max(1) as f32;
        let y = track.top() + (track.height() - h) * (top as f32 / max_top);
        pt::fill(
            p,
            Rect::from_min_size(pos2(track.left(), y), vec2(track.width(), h)),
            t::ui().scrollbar,
        );
    }

    // ── 状态行（空目录/失败都在这里给中文原因）──
    let sr = status_rect(l);
    pt::fill(p, sr, t::ui().panel_header);
    let mut lab = DynLabel::new(
        "maplist.status",
        tx::MAPLIST_STATUS_TMPL,
        st.status_text(),
        LabelStyle::new(
            m::MAPS_STATUS_FONT,
            pos2(sr.left() + m::MAPS_PAD, sr.center().y),
            Align2::LEFT_CENTER,
            Rect::from_min_max(
                pos2(sr.left() + m::MAPS_PAD, sr.top()),
                pos2(sr.right() - m::MAPS_PAD, sr.bottom()),
            ),
            if matches!(st.phase, Phase::Failed(_)) {
                t::ui().warn_banner_text
            } else {
                t::ui().text_muted
            },
        ),
    );
    lab.truncate_to_fit(p);
    lab.draw(p);
}

// ───────────────────────── 交互 ─────────────────────────

/// 一次列表交互的结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// 没点中任何东西。
    None,
    /// 点了第 `idx` 条（**调用方必须走 `open_path`**，不许另写加载旁路）。
    Open(usize),
    /// 点了「刷新」。
    Refresh,
}

/// 处理点击/滚轮。返回调用方该做什么。
///
/// 只在面板可见（[`Layout::p_maps`] 非零宽）时调用 —— 宿主那边由
/// `panels.get(PanelId::Maps)` 守着。
pub fn interact(ui: &mut egui::Ui, l: &Layout, st: &mut MapListPanel) -> Action {
    if l.p_maps.whole.width() <= 0.0 {
        return Action::None;
    }
    let n = visible_rows(l);
    // 滚轮（鼠标在列表上时）
    let list = list_rect(l);
    let resp = ui.interact(list, ui.id().with("maplist_list"), Sense::hover());
    if resp.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll.abs() > 0.5 {
            let mut rows = (scroll / m::MAPS_ROW_H).round() as i64;
            if rows == 0 {
                rows = if scroll > 0.0 { 1 } else { -1 };
            }
            // 滚轮向上（正 y）= 往回看 ⇒ 行号减小
            st.scroll_rows(-rows, n);
            ui.ctx().request_repaint();
        }
    }
    // 「刷新」
    if ui
        .interact(
            refresh_rect(l),
            ui.id().with("maplist_refresh"),
            Sense::click(),
        )
        .clicked()
    {
        return Action::Refresh;
    }
    // 行点击
    let top = st.top.min(st.entries.len());
    for row in 0..n {
        let idx = top + row;
        if idx >= st.entries.len() {
            break;
        }
        let r = row_rect(l, row);
        let mut resp = ui.interact(r, ui.id().with(("maplist_row", idx)), Sense::click());
        if let Some(e) = st.entries.get(idx) {
            let tip = tx::fill(
                tx::MAPLIST_TIP_TMPL,
                &[
                    ("path", &e.path.display().to_string()),
                    ("size", &format_size(e.bytes)),
                    (
                        "mtime",
                        &e.mtime
                            .and_then(format_mtime)
                            .unwrap_or_else(|| tx::VAL_UNKNOWN.to_owned()),
                    ),
                ],
            );
            resp = resp.on_hover_text(tip);
        }
        if resp.clicked() {
            return Action::Open(idx);
        }
    }
    Action::None
}

/// 每帧调用：把当前地图那一行滚进可见区（**只跟一次**，见 [`MapListPanel::follow_current`]）。
pub fn tick(l: &Layout, st: &mut MapListPanel, current: Option<&Path>) {
    if l.p_maps.whole.width() <= 0.0 {
        return;
    }
    st.follow_current(current, visible_rows(l));
}
