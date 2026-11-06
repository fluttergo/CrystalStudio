//! 文档：**双表示**（`raw` 字节真值 + `view` 编辑视图）+ 路径 + **脏标记** + `to_bytes()`。
//!
//! 抽取自 `miredit-gui/src/mapdoc.rs:22-44`（`MapDoc`）与
//! `miredit-gui/src/edit.rs:1326-1346`（`index_of` / `cell_at` / `cell_mut`）、
//! `edit.rs:1669-1720`（`ByteDiff` / `byte_diff`）。
//!
//! # 一、双表示：`raw` 是字节真值，`view` 是编辑/渲染视图
//!
//! ```text
//! raw : mir2_format::RawMap  ← 打开时 read_any_tolerant(bytes)；保存时由它出字节
//! view: mir2_map::Map        ← 编辑/渲染只认它（miredit-render 的入口签名就是 &mir2_map::Map）
//! ```
//!
//! - **打开**：`mir2_format::read_any_tolerant` → `RawMap` → [`crate::convert::view_from_raw`] → `Map`；
//! - **编辑**：只改 `view`（`paint` / `fill` / `undo` 一行都没改）；
//! - **保存**：[`Document::try_to_bytes`] 里先 [`crate::convert::sync_view_into_raw`] 把 `view`
//!   的格写回 `raw.cells`，再让 `raw.format` 对应的 writer 出字节，
//!   `header_raw` + `trailer` 原样带上。
//!
//! 为什么必须这样：`mir2_map::Map` 只认 12/14/36 三档（`mir2-map/src/cell.rs:96-104`），
//! 而九种格式的头部长度 / 格长 / 尾区各不相同（见 [`crate::convert`] 模块文档）。
//! 若强行让 `mir2_map::Map` 承载九种格式，就要改渲染层与 103 条既有测试 —— 那是另一件事。
//!
//! # 二、为什么"脏标记"存在这里，而不是再存一份
//!
//! 脏标记的真值只有一个（`Document::dirty`）。**谁能算它**：会话（[`crate::EditSession`]）
//! 拿 `pos != saved_pos || 笔画未结算` 推出来，然后调 [`Document::set_dirty`] 写回这一个字段
//! （`miredit-gui/src/edit.rs:727-729` 的判据原样搬过来）。**绝不两处各存一份**——
//! 两套真值迟早会出现"标题栏有 `*`、保存却什么都不写"这类自相矛盾。
//!
//! # 三、`orig_bytes` 为什么留着
//!
//! 验收要能回答"撤销之后是不是和磁盘上那份**逐字节**相同"（任务书 §六.4），
//! 以及"这一笔在全文件里差了几个字节"（§六.6）。`orig_bytes` 就是打开时读进来的那份字节，
//! **只读、只在 [`Document::diff`] 里被比对**，从不参与写回（写回永远走
//! [`Document::try_to_bytes`] 现算）。代价是一份地图大小的内存（`0.map` 5.6 MB），
//! 换来的是任何时刻都能做字节级自证。
//!
//! # 四、就地改 `view.cells`，不做覆盖层
//!
//! `mir2_map::Map::cells` 是 `pub Vec<Cell>`，**列优先**：`cells[x * height + y]` 对应 `(x, y)`
//! （`mir2-map/src/map.rs` §存储顺序）。本模块就地改 `cells[idx]`：
//!
//! - **不用覆盖层**（`HashMap<(u16,u16), Cell>`）：那会让"内存里的地图"和"文件里的字节"变成
//!   两套真值，保存时要再套回去，漏一个字段就会出现"看着改了、存下去没改"
//!   （`edit.rs:83-87` 原话）；
//! - 就地改的副作用可控：`sync_view_into_raw` 是逐格写回，改哪格就只影响哪格的字节区间。
//!
//! # 五、`view` 与 `raw` 谁可以被外部改
//!
//! `view` 是**编辑面**（`view_mut()` 拿得到），`raw` 是**只读真值**（只有 `raw()`）。
//! 唯一会写 `raw.cells` 的地方是 [`Document::try_to_bytes`]（写在一份 clone 上，
//! 不改文档本身）。这样"文档里的 `raw`"永远等于"磁盘上那份字节"，可以直接做逐字节自证。
//!
//! # 六、五层计数（2026-11-06 新增，见 [`crate::layer_counts`]）
//!
//! 文档缓存五个官方层计数（`Document.LayerCounts.cs:5@acfbbf8a` 的等价物）：
//!
//! - **单格写**走 [`Document::write_view_cell`]（写视图 + O(1) 增量更新计数）；
//!   它是编辑路径的**唯一**逐格写入口（`paint` / `fill` / `stamp` / 选区 / 撤销）。
//! - **整块换视图**（`std::mem::swap(document.view_mut(), &mut map)`，GUI 的
//!   `mapdoc.rs:154` / `:194`、`edit.rs:322` / `:324`）绕过了上一个入口 ⇒
//!   换完**必须**调 [`Document::rebuild_layer_counts`]，否则计数停在旧视图上
//!   （旧值不会报错，只会显示错数）。**直接改 `view_mut().cells[…]` 的调用方同理。**
//! - 打开文件（[`Document::from_bytes`]）构造完立刻 `rebuild_layer_counts()` 一次。

use std::path::{Path, PathBuf};

use mir2_format::{format_by_id, FormatCaps, FormatError, FormatId, RawMap};
use mir2_map::{Cell, Map};

use crate::convert::{ground_paint_supported, sync_view_into_raw, view_from_raw};
use crate::error::CoreError;
use crate::layer_counts::{cell_mask_ext, count_map, CountKeeper, LayerCounts, MaskContext};

/// 一张已解析的地图 + 它的出处信息。抽取自 `mapdoc.rs:22-44`。
#[derive(Debug)]
pub struct Document {
    /// **字节层真值**：`format` / `header_raw` / `trailer` / `cells`（`mir2-format/src/model.rs:305-344`）。
    ///
    /// 保存时由它出字节；它永远是"打开时那份字节 + 视图改过的格"。
    raw: RawMap,
    /// **编辑/渲染视图**：`mir2_map::Map`（`mir2-map/src/map.rs:28-41`）。
    ///
    /// 编辑只改它。它的 `format` 是**语义投影**（见 [`crate::convert::view_cell_format`]），
    /// 不是磁盘格长；它的 `header_raw` 是**视图头**，永不写回。
    view: Map,
    /// 打开时的文件路径（原样保留，用于保存与诊断）。
    pub path: PathBuf,
    /// 文件名（含扩展名，如 `0.map`）——标题栏、页签、状态栏都用它。
    pub file_name: String,
    /// 打开时的**磁盘原文件字节**（只读；见模块文档第三节）。
    orig_bytes: Vec<u8>,
    /// 脏标记（唯一真值；由 [`crate::EditSession`] 在每次改动后写回）。
    dirty: bool,
    /// `raw.trailer` 的字节数（= `raw.trailer_len()`）—— **「本工具没解释的剩余字节」**。
    ///
    /// 非 0 就必须告警（见 [`Document::edit_warning`]）：`mir2-format/src/model.rs:330-342`
    /// 明确要求「上层拿到 `trailer_len() != 0` 时必须提示用户：本工具按头部 `w·h` 解释，
    /// 剩余 N 字节未解释」。实测 67 张真实文件（`EM001.map` 等）全部 `8040` 字节。
    tolerant_trailer: usize,
    /// **五层计数缓存**（见模块文档第六节与 [`crate::layer_counts`]）。
    ///
    /// 唯一维护路径：[`Document::write_view_cell`]（增量）与
    /// [`Document::rebuild_layer_counts`]（全量）；**没有第三处**。
    counts: LayerCounts,
}

impl Document {
    /// 从磁盘读并解析一张 `.map`。
    ///
    /// 失败分两类，都带路径：读不出来 ⇒ [`CoreError::Io`]；读出来但九种格式都判不出来 ⇒
    /// [`CoreError::Map`]（判据全在 `mir2-format` 里，这里不重写）。
    pub fn load(path: &Path) -> Result<Document, CoreError> {
        let bytes = std::fs::read(path).map_err(|e| CoreError::Io {
            path: path.to_path_buf(),
            detail: e.to_string(),
        })?;
        Document::from_bytes(path, bytes)
    }

    /// 从已有字节构造（网页版上传一份字节、或测试用内存图）。
    ///
    /// `bytes` 同时成为 [`Document::orig_bytes`]：构造那一刻"磁盘上是什么"就是它。
    ///
    /// # 为什么用 `read_any_tolerant` 而不是 `read_any`
    ///
    /// 真实语料里有 **67 张**「长度多一截」的文件（`client/热血传奇23周年客户端/Map/EM001.map`
    /// 等：头部声明 30×35 = 1050 格、`52 + 12×1050 = 12,652`，实际 20,692 字节）。
    /// 严格入口会按规格拒绝它们（`FormatError::LengthMismatch`），而编辑器**必须能打开**
    /// （验收门 1：1,158 张全部能 open）。宽松入口把那 8,040 字节原样放进 `raw.trailer`，
    /// **一个字节都不丢**，同时把「没解释的字节数」暴露在 [`Document::tolerant_trailer`] 上。
    pub fn from_bytes(path: &Path, bytes: Vec<u8>) -> Result<Document, CoreError> {
        let raw = mir2_format::read_any_tolerant(&bytes).map_err(|err: FormatError| CoreError::Map {
            path: path.to_path_buf(),
            err,
        })?;
        let view = view_from_raw(&raw);
        let tolerant_trailer = raw.trailer_len();
        let file_name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());
        Ok(Document {
            raw,
            view,
            path: path.to_path_buf(),
            file_name,
            orig_bytes: bytes,
            dirty: false,
            tolerant_trailer,
            counts: LayerCounts::zero(),
        })
        // 构造完立刻全量数一次（`Document.LayerCounts.cs:5@acfbbf8a` 的五个计数在
        // `MapDocument.Load` 的逐格 `Set` 里被累计，等价于这里的一次 O(n) 重建）。
        .map(|mut doc| {
            doc.rebuild_layer_counts();
            doc
        })
    }

    // ── 双表示的两侧 ──

    /// **字节层真值**（只读）。`format` / `header_raw` / `trailer` / `cells` 全在这里。
    pub fn raw(&self) -> &RawMap {
        &self.raw
    }

    /// **编辑/渲染视图**（只读）。渲染层要的就是它。
    pub fn view(&self) -> &Map {
        &self.view
    }

    /// **编辑/渲染视图**（可变）—— 编辑路径（`paint` / `fill` / `undo`）走这条。
    ///
    /// ⚠ 改 `view.header_raw` / `view.format` / `view.width` **不会**影响保存结果：
    /// 保存走 `raw`，视图头与视图档位都是投影（见模块文档第一节）。
    /// 唯一有意义的改动是 `view.cells`。
    pub fn view_mut(&mut self) -> &mut Map {
        &mut self.view
    }

    // ── 五层计数（2026-11-06 新增；口径见 [`crate::layer_counts`]） ──

    /// 当前五层计数（官方 `LayerCount(MapLayer)` × 5 的缓存）。
    ///
    /// 由 [`Document::write_view_cell`] / [`Document::rebuild_layer_counts`] 维护；
    /// 直接改 `view_mut()` 的调用方必须自己调 `rebuild_layer_counts()`（模块文档第六节）。
    pub fn layer_counts(&self) -> LayerCounts {
        self.counts
    }

    /// **整块重建**五层计数（O(n)），返回重建后的结果。
    ///
    /// 两个调用点（**只有这两个**）：
    /// 1. [`Document::from_bytes`] 构造完之后（自动）；
    /// 2. 外部**整块换视图**之后 —— GUI 用
    ///    `std::mem::swap(document.view_mut(), &mut map)`（`mapdoc.rs:154` / `:194`、
    ///    `edit.rs:322` / `:324`）或直接改 `view_mut().cells[…]` 时，
    ///    绕过了 [`Document::write_view_cell`] ⇒ 必须调本方法把计数追平新视图。
    pub fn rebuild_layer_counts(&mut self) -> LayerCounts {
        self.counts = count_map(&self.raw, &self.view);
        self.counts
    }

    /// **写一格视图 + 增量更新计数**（编辑路径的**唯一**逐格写入口）。
    ///
    /// 返回 `true` = 真的写了（格值变了）；`false` = 没写、计数也没动，三种来源：
    /// 1. `(x, y)` 越界；
    /// 2. `after` 与当前格**逐字段相同**（no-op 不许动计数，也不许产生撤销步）；
    /// 3. 字节层与视图的格号对不上（[`crate::convert::ConvertError::Shape`] 类不变量被破坏）
    ///    —— 此时**宁可一格不写**，也不写一个「计数说不清」的格。
    pub fn write_view_cell(&mut self, x: u16, y: u16, after: Cell) -> bool {
        let Some(i) = index_of(&self.view, x, y) else {
            return false;
        };
        let before = self.view.cells[i];
        if before == after {
            return false;
        }
        let Some(raw_cell) = self.raw.cells.get(i) else {
            return false;
        };
        let ctx = MaskContext::from_raw(&self.raw);
        let before_mask = cell_mask_ext(raw_cell, &before, ctx, x, y);
        let after_mask = cell_mask_ext(raw_cell, &after, ctx, x, y);
        self.view.cells[i] = after;
        self.counts.apply(before_mask, after_mask);
        true
    }

    /// 借出「视图（可写）+ 计数更新器（可写）+ 字节层（只读）」三件**不重叠**的借用。
    ///
    /// 给撤销 / 重做用：`UndoStack::undo_with` 需要 `&mut Map` 写回旧格，同时要按
    /// 「每写一格」更新计数；若只借 `&mut Document`，两件事会撞在同一个 `&mut self` 上。
    pub(crate) fn view_and_keeper(&mut self) -> (&mut Map, CountKeeper<'_>) {
        let Document {
            raw,
            view,
            counts,
            ..
        } = self;
        (view, CountKeeper::new(raw, counts))
    }

    /// 格数组之前的**真头部字节**（九种格式各自长度不同，`mir2-format/src/model.rs:312-316`）。
    ///
    /// ⚠ 与 [`Document::view`]`().header_raw` 不同：那是定长 52 的**视图头**（永不写回）。
    pub fn raw_header(&self) -> &[u8] {
        &self.raw.header_raw
    }

    /// 格数组之后**没被解释**的字节（原样保留、原样写回）。
    pub fn trailer(&self) -> &[u8] {
        &self.raw.trailer
    }

    // ── 格式信息 ──

    /// 九种格式的标识（真值来自 `mir2-format` 的判别，不是猜的）。
    pub fn format_id(&self) -> FormatId {
        self.raw.format
    }

    /// 格式人读名（带格长与出处，例如 `"Shanda2012（36B/格，14B 字段 + 22B 未建模尾区）"`）。
    pub fn format_label(&self) -> &'static str {
        format_by_id(self.raw.format).name()
    }

    /// 该格式的能力声明（能不能写、能不能编辑、地表逐格落笔有没有效、碰撞极性…）。
    ///
    /// 依据 `mir2-format/src/model.rs:419-461`。
    pub fn caps(&self) -> FormatCaps {
        format_by_id(self.raw.format).caps()
    }

    /// 宽 × 高（世界格）。来源：`mapdoc.rs:36-38`。
    pub fn size(&self) -> (u16, u16) {
        (self.raw.width, self.raw.height)
    }

    /// **视图档位名**（`Old12` / `New14` / `Ext36`）—— 注意这是 `mir2_map::CellFormat` 的
    /// 三档语义投影，**不是**磁盘格长（见 [`crate::convert::view_cell_format`]）。
    ///
    /// 真格式名请用 [`Document::format_label`]。
    pub fn format_name(&self) -> &'static str {
        self.view.format.name()
    }

    /// 磁盘单格字节数（变长格式如 `CrystalCs` 返回 `None`）。
    ///
    /// 与 [`Document::format_name`] 的区别：这个才是**文件里一格几字节**。
    pub fn disk_cell_len(&self) -> Option<usize> {
        self.caps().fixed_cell_len
    }

    // ── 未解释字节 / 告警 ──

    /// `raw.trailer` 的字节数 —— **「本工具没解释的剩余字节」**。
    ///
    /// 非 0 ⇒ 必须告警（见 [`Document::edit_warning`]）。实测那 67 张真实文件都是 `8040`。
    pub fn tolerant_trailer(&self) -> usize {
        self.tolerant_trailer
    }

    /// **常驻告警**（`None` = 没有需要提醒的事）。GUI/网页应当把它常驻显示。
    ///
    /// 两种来源，可能**同时**成立（用「；」连接）：
    ///
    /// 1. **未解尾块**（[`Document::tolerant_trailer`] `> 0`）：
    ///    「本图有 N 字节未解释尾块（按头部 w×h 解释）」
    ///    —— 依据 `mir2-format/src/model.rs` 对 [`mir2_format::RawMap::trailer`] 的强制要求。
    ///    **这条是真警告，任何改动都不许削弱它**（67 张 `EM001.map` 类文件尾块 8,040 字节，
    ///    若真实尺寸不是头部声明值，列优先展开会让整图错位）。
    /// 2. **格式不承诺「改格后语义自洽」**（`caps().editable == false`）：
    ///    文案**逐格式具体**取自 [`mir2_format::FormatCaps::editable_caveat`]，
    ///    说清「哪一段、多少字节、编辑时怎么处理」。
    ///
    /// # ⚠ 2026-11-05 订正（P1-9）：这里曾是一句**假话**
    ///
    /// 旧实现不管哪种格式都追加「本格式（X）未解区语义未知，编辑仅保证字节保真，
    /// 不保证动画/联动正确」。对 `Old12` 那是**事实错误** ——
    /// 它的 12 字节全部有名字（`mir2-format/src/formats/old12.rs` 字段表）、`tail` 恒空、
    /// [`crate::convert::unmodeled_fields`]`(Old12)` 为空、856 张真实样本往返 0 字节差异
    /// ⇒ 它**没有"未解区"**。
    /// 后果：1,051 / 1,158 张真实图（90.8%，含缺省打开的 `0.map`）一打开就挂一条常驻黄条，
    /// 用户很快学会无视它，而真正要看的第 1 条（尾块可能整图错位）就排在同一条黄条里被淹没。
    ///
    /// ⇒ 现在 `editable == true` 的格式（`Old12` / `Shanda14`）在来源 2 上**一个字都不出**；
    /// 其余 7 种各说各的具体原因（见 `mir2-format/src/formats/*.rs` 的 `caps()`）。
    ///
    /// ⚠ `editable == false` **不等于不能编辑**：本 crate 一律允许编辑，只是不承诺语义自洽。
    pub fn edit_warning(&self) -> Option<String> {
        let mut parts: Vec<String> = Vec::new();
        if self.tolerant_trailer > 0 {
            parts.push(format!(
                "本图有 {} 字节未解释尾块（按头部 {}×{} 解释）——若真实尺寸不是头部声明值，\
                 列优先展开会让整图错位",
                self.tolerant_trailer, self.raw.width, self.raw.height
            ));
        }
        let caps = self.caps();
        if !caps.editable {
            // 契约（`mir2-format/src/registry.rs` 的 `editable与caveat必须一致` 钉住）：
            // `editable == false` ⟺ `editable_caveat` 非空。走到空串分支说明契约被破坏 ——
            // 此时宁可说「未给出具体原因」，也**不许**再编一句「未解区语义未知」那种套话。
            let 原因 = if caps.editable_caveat.is_empty() {
                "改格后语义自洽性未经确认（本格式未给出具体原因）"
            } else {
                caps.editable_caveat
            };
            parts.push(format!("本格式「{}」{}", self.format_label(), 原因));
        }
        if parts.is_empty() {
            None
        } else {
            Some(parts.join("；"))
        }
    }

    // ── 字节 ──

    /// 打开时的文件字节数（诊断用；也是"真的读了这么多字节"的凭据）。
    pub fn orig_len(&self) -> usize {
        self.orig_bytes.len()
    }

    /// 打开时的磁盘原文件字节（只读视图）。见模块文档第三节。
    pub fn orig_bytes(&self) -> &[u8] {
        &self.orig_bytes
    }

    /// **保存写回的就是它**：`view → raw` 同步后由 `raw.format` 的 writer 出字节。
    ///
    /// # Panics
    ///
    /// 只在 [`Document::try_to_bytes`] 失败时 panic（会带上原因）。失败意味着
    /// 「`raw` 不再是它自己那次 `read` 的产物」或「有人把 `view` 的宽高改坏了」——
    /// 那是本 crate 的 bug，**宁可炸也不许静默回退到原字节**（回退＝假保存）。
    /// 生产路径（`save_session`）走的是 [`Document::try_to_bytes`]，失败会变成
    /// [`CoreError::Map`]（payload = [`FormatError::Inconsistent`]）。
    pub fn to_bytes(&self) -> Vec<u8> {
        self.try_to_bytes()
            .unwrap_or_else(|e| panic!("文档字节生成失败（这是本 crate 的 bug，不许静默回退）：{e}"))
    }

    /// 同 [`Document::to_bytes`]，但把失败如实报出来。
    ///
    /// 两条失败来源，都带明确原因，且都折成 [`CoreError::Map`]（payload 是
    /// [`FormatError`]，理由见 `error.rs` 模块文档最后一节）：
    /// 1. [`crate::convert::ConvertError::Shape`] —— 视图与 `raw` 的宽高/格数对不上
    ///    （包装成 [`FormatError::Inconsistent`]）；
    /// 2. [`FormatError`] —— writer 拒绝了某个值（例如该格式的图号只有 16 位）。
    ///
    /// ⚠ **绝不**回退到"原字节"：回退＝用户以为存上了，实际磁盘上还是旧内容。
    pub fn try_to_bytes(&self) -> Result<Vec<u8>, CoreError> {
        let mut raw = self.raw.clone();
        sync_view_into_raw(&self.view, &mut raw).map_err(|e| CoreError::Map {
            path: self.path.clone(),
            err: FormatError::Inconsistent {
                detail: e.to_string(),
            },
        })?;
        format_by_id(raw.format)
            .write(&raw)
            .map_err(|err| CoreError::Map {
                path: self.path.clone(),
                err,
            })
    }

    // ── 脏标记 ──

    /// 是否有未保存的修改。判据由会话算、存在这一个字段里（见模块文档第二节）。
    pub fn dirty(&self) -> bool {
        self.dirty
    }

    /// 写脏标记（**会话是唯一写入点**；见模块文档第二节）。
    pub fn set_dirty(&mut self, v: bool) {
        self.dirty = v;
    }

    /// 标脏（改过一格就调一次）。
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// 标干净（保存成功后调）。
    pub fn mark_clean(&mut self) {
        self.dirty = false;
    }

    /// 当前内存地图 vs 磁盘原文件的逐字节差异。来源：`edit.rs:1690-1720`。
    ///
    /// 用的是 [`Document::to_bytes`]（= 真保存字节），不是 `view.to_bytes()`
    /// —— 后者对九种里的七种根本不是文件字节。
    pub fn diff(&self) -> ByteDiff {
        byte_diff_bytes(
            &self.to_bytes(),
            &self.orig_bytes,
            self.raw.header_raw.len(),
            self.disk_cell_len().unwrap_or(1).max(1),
        )
    }

    /// 该格式的「地表逐格落笔」是否有效（`false` 时地表笔刷必须被拒）。
    ///
    /// 九种里只有 `Mir3Wemade`（Type5）为 `false`；判据在
    /// [`crate::convert::ground_paint_supported`]。
    pub fn ground_paint_supported(&self) -> bool {
        ground_paint_supported(self.raw.format)
    }
}

// ───────────────────────── 格访问 ─────────────────────────

/// `(x, y)` → `cells` 下标（列优先：`x * height + y`）；越界 `None`。来源：`edit.rs:1329-1334`。
pub fn index_of(map: &Map, x: u16, y: u16) -> Option<usize> {
    if x >= map.width || y >= map.height {
        return None;
    }
    Some(x as usize * map.height as usize + y as usize)
}

/// 只读取格（与 `Map::cell` 同语义，单独包一层便于本 crate 自证口径一致）。
/// 来源：`edit.rs:1337-1339`。
pub fn cell_at(map: &Map, x: u16, y: u16) -> Option<&Cell> {
    map.cell(x, y)
}

/// 可变取格 —— **本 crate 唯一的"可变访问器"**。来源：`edit.rs:1343-1346`。
///
/// 走 `Map::cells` 这个公开字段，**不需要**给 `mir2-map` 加 `cell_mut`
/// （那个 crate 正在别路验收，本单一个字都不许改）。
///
/// ⚠ **它绕过五层计数**：改完必须自己对 [`Document::rebuild_layer_counts`] 负责
/// （编辑路径请改用 [`Document::write_view_cell`]，那才是「写一格 + 计数增量」的入口）。
pub fn cell_mut(map: &mut Map, x: u16, y: u16) -> Option<&mut Cell> {
    let i = index_of(map, x, y)?;
    map.cells.get_mut(i)
}

// ───────────────────────── 逐字节差异（验收取证用） ─────────────────────────

/// 当前地图序列化后与"磁盘原文件"的差异统计。来源：`edit.rs:1669-1687`。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ByteDiff {
    /// 不同的字节数。
    pub bytes: usize,
    /// 涉及的格数（按格长折算）。
    pub cells: usize,
    /// 首个差异的文件偏移。
    pub first: Option<usize>,
    /// 头部（格数组之前的字节）里不同的字节数。
    pub header: usize,
}

impl ByteDiff {
    /// 是否逐字节相同。
    pub fn same(&self) -> bool {
        self.bytes == 0
    }
}

/// 逐字节比较 `map.to_bytes()` 与 `orig`；差异按格归并。来源：`edit.rs:1690-1720`。
///
/// ⚠ 只对 `mir2_map` 能完整表示的三档（`Old12` / `New14` / `Ext36`）成立 ——
/// 它的头部固定按 52 字节、格长按 `map.format.cell_size()` 折算。九种格式的通用版本是
/// [`byte_diff_bytes`]（[`Document::diff`] 走的是后者）。
///
/// 长度不等时按较长的一侧扫（多出的字节一律计差异），所以"截断/多写"也会被抓到。
pub fn byte_diff(map: &Map, orig: &[u8]) -> ByteDiff {
    byte_diff_bytes(
        &map.to_bytes(),
        orig,
        mir2_map::HEADER_LEN,
        map.format.cell_size().max(1),
    )
}

/// 通用版逐字节差异：给定**当前字节**与**原始字节**，按 `header_len` / `cell_size` 归并。
///
/// `cell_size` 传 0 时按 1 处理（变长格式没有固定格长，此时只统计字节数与首个偏移，
/// 格数一栏不可信 —— 调用方须自己知道这一点）。
pub fn byte_diff_bytes(now: &[u8], orig: &[u8], header_len: usize, cell_size: usize) -> ByteDiff {
    let size = cell_size.max(1);
    let total_cells = now.len().saturating_sub(header_len) / size + 1;
    let mut seen = vec![false; total_cells];
    let mut d = ByteDiff::default();
    let n = now.len().max(orig.len());
    for i in 0..n {
        let a = now.get(i).copied();
        let b = orig.get(i).copied();
        if a == b {
            continue;
        }
        d.bytes += 1;
        if d.first.is_none() {
            d.first = Some(i);
        }
        match i.checked_sub(header_len) {
            // 落在头部里的差异（正常编辑不会发生）
            None => d.header += 1,
            Some(rel) => {
                let cell = rel / size;
                if cell < total_cells && !seen[cell] {
                    seen[cell] = true;
                    d.cells += 1;
                }
            }
        }
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::造图;
    use mir2_map::CellFormat;

    /// 列优先下标必须与 `Map::cell` 同口径（否则改格会改到别的格上）。来源：`edit.rs:2123-2134`。
    #[test]
    fn 下标与取格同源() {
        let map = 造图(4, 3, CellFormat::New14);
        for y in 0..3u16 {
            for x in 0..4u16 {
                let idx = index_of(&map, x, y).expect("图内");
                assert_eq!(idx, x as usize * 3 + y as usize);
                assert_eq!(&map.cells[idx], map.cell(x, y).unwrap());
                // cell_mut 必须指到同一格
                let mut m = map.clone();
                assert_eq!(cell_mut(&mut m, x, y).copied(), Some(map.cells[idx]));
            }
        }
        assert_eq!(index_of(&map, 4, 0), None);
        assert_eq!(index_of(&map, 0, 3), None);
        let mut m = map.clone();
        assert!(cell_mut(&mut m, 4, 0).is_none(), "越界不许给可变引用");
    }

    /// 文档构造后必须**不脏**，`to_bytes()` 与输入逐字节相等（往返保真）。
    #[test]
    fn 构造后不脏且往返一致() {
        let map = 造图(3, 3, CellFormat::Ext36);
        let bytes = map.to_bytes();
        let doc = Document::from_bytes(Path::new(r"I:\x\3x3.map"), bytes.clone()).unwrap();
        assert!(!doc.dirty());
        assert_eq!(doc.to_bytes(), bytes);
        assert_eq!(doc.orig_len(), bytes.len());
        assert!(doc.diff().same(), "刚打开时 0 差异");
        assert_eq!(doc.file_name, "3x3.map");
        assert_eq!(doc.size(), (3, 3));
        assert_eq!(doc.format_name(), "Ext36");
        // 双表示两侧都能取到，且真值侧是 Shanda2012（36 B/格）
        assert_eq!(doc.format_id(), FormatId::Shanda2012);
        assert_eq!(doc.disk_cell_len(), Some(36));
        assert_eq!(doc.tolerant_trailer(), 0);
        assert_eq!(doc.raw_header().len(), 52, "Shanda2012 的真头部是 52 字节");
        assert_eq!(doc.trailer().len(), 0);
        // 视图档位 = 语义投影
        assert_eq!(doc.view().format, CellFormat::Ext36);
        assert_eq!(doc.view().cells.len(), 9);
    }

    /// 脏标记：`set_dirty` 写的就是那一个字段，`mark_clean` 清掉。来源：模块文档第二节。
    #[test]
    fn 脏标记只有一份() {
        let map = 造图(2, 2, CellFormat::Old12);
        let mut doc = Document::from_bytes(Path::new(r"I:\x\a.map"), map.to_bytes()).unwrap();
        assert!(!doc.dirty());
        doc.mark_dirty();
        assert!(doc.dirty());
        doc.mark_clean();
        assert!(!doc.dirty());
        doc.set_dirty(true);
        assert!(doc.dirty());
    }

    /// 逐字节差异统计：只改 2 格 ⇒ 差异落在 2 个格区间内，头部 0 字节差异。
    /// 来源：`edit.rs:2511-2526`。
    #[test]
    fn 差异按格统计() {
        let mut map = 造图(4, 4, CellFormat::Ext36);
        let orig = map.to_bytes();
        assert!(byte_diff(&map, &orig).same());

        let a = index_of(&map, 1, 2).unwrap();
        let b = index_of(&map, 3, 0).unwrap();
        map.cells[a].bk_img = 0x1234;
        map.cells[b].bk_img = 0x5678;
        let d = byte_diff(&map, &orig);
        assert_eq!(d.cells, 2, "应只涉及 2 个格");
        assert_eq!(d.header, 0, "52 字节头不许变");
        assert_eq!(d.bytes, 4, "每格只改 bk_img 的 2 字节");
        assert!(d.first.unwrap() >= mir2_map::HEADER_LEN);
    }

    /// 长度不等（多写 / 截断）也必须被抓到，不许静默判"相同"。
    #[test]
    fn 长度不等算差异() {
        let map = 造图(2, 2, CellFormat::Old12);
        let mut orig = map.to_bytes();
        orig.push(0);
        let d = byte_diff(&map, &orig);
        assert!(!d.same());
        assert_eq!(d.bytes, 1);
        assert_eq!(d.header, 0);
        assert_eq!(d.first, Some(map.to_bytes().len()), "差异就在多出来的那个字节");
    }

    /// 解析失败必须是 `Err(Map)`，带路径与具体原因（禁静默降级）。来源：`mapdoc.rs:176-190`。
    #[test]
    fn 解析失败带路径与原因() {
        let e = Document::from_bytes(Path::new(r"I:\x\bad.map"), vec![0u8; 51]).unwrap_err();
        match e {
            CoreError::Map { path, err } => {
                assert_eq!(path, Path::new(r"I:\x\bad.map"));
                assert!(err.to_string().contains("51"), "{err}");
            }
            other => panic!("应报 Map，实际 {other:?}"),
        }
    }

    /// 读不到文件 ⇒ `Err(Io)`，原因带系统原文。
    #[test]
    fn 读不到文件如实报错() {
        let p = Path::new(r"I:\__no_such_dir_miredit_core__\x.map");
        match Document::load(p) {
            Err(CoreError::Io { path, detail }) => {
                assert_eq!(path, p);
                assert!(!detail.is_empty(), "系统报错原文不许为空");
            }
            other => panic!("应报 Io，实际 {:?}", other.map(|d| d.file_name)),
        }
    }

    /// **出字节失败必须如实报，且绝不回退到原字节**（假保存比报错更糟）。
    ///
    /// 手法：把视图的宽高改坏（模拟"有人绕过 `Document` 动了 `view`"），
    /// 让 `sync_view_into_raw` 的形状校验失败。
    #[test]
    fn 出字节失败报格式层错误不回退() {
        let map = 造图(2, 2, CellFormat::Old12);
        let mut doc = Document::from_bytes(Path::new(r"I:\x\bad_shape.map"), map.to_bytes()).unwrap();
        doc.view_mut().width = 3;
        let e = doc.try_to_bytes().expect_err("形状不符必须报错");
        match e {
            CoreError::Map { path, err } => {
                assert_eq!(path, Path::new(r"I:\x\bad_shape.map"));
                assert!(
                    matches!(err, mir2_format::FormatError::Inconsistent { .. }),
                    "形状不符要折成 Inconsistent，实际 {err:?}"
                );
                let msg = err.to_string();
                assert!(msg.contains("形状不一致"), "{msg}");
                assert!(msg.contains('3') && msg.contains('2'), "信息要带两边尺寸：{msg}");
            }
            other => panic!("应报 Map（写方向），实际 {other:?}"),
        }
    }
}
