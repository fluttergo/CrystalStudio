//! 编辑会话：文档 + 工具 + 笔刷 + 当前层 + 笔刷尺寸 + 撤销栈 + 提示。
//!
//! 抽取自 `miredit-gui/src/edit.rs:581-1324`（`EditSession` 全块，含 `FillRecord`）、
//! `edit.rs:1612-1665`（`layer_note` / `line_cells`）。
//!
//! # 一、与 GUI 的结构差异（唯一一处，且是被 `cmd::apply` 的签名逼出来的）
//!
//! GUI 把「文档」与「会话」拆成两个对象（`DocState` + `EditSession`，`app.rs` 里成对传参）。
//! 本 crate 的 [`crate::apply`] 签名是 `apply(session: &mut EditSession, cmd: Command)`
//! （任务书 §五：网页版只拿得到一个会话句柄）⇒ **会话必须持有文档**。
//! 于是 [`EditSession`] = `Document` + 工具状态 + [`UndoStack`]。
//!
//! 落笔语义、记账方式、拒绝原因**一条都没改**，只是把 `&mut Map` 参数换成了自己的字段。
//!
//! # 二、N×N 由这里展开（`paint` 只吃一格）
//!
//! 形状 = **居中方形**（官方 `MapViewport.cs:476-478` 的双层循环
//! `for iy in -radius..=radius { for ix in -radius..=radius { … if (!Document.Contains(sx, sy)) continue; } }`；
//! 官方工具提示 `CentredBrushFootprintAndtoResize_55DDE2E3` 也写的是「以中心定位笔刷范围」）。
//! 边缘裁剪 = **逐格跳过越界格**（与官方 `continue` 同一判据），不是"把方块整体挪回界内"。
//!
//! ⚠ **与官方的一处差异（有意，已记档）**：官方地表层的 `step = 2`
//! （`MapViewport.cs:474-475`：`int step = Layer == MapLayer.Ground ? 2 : 1; if (step == 2) { x &= ~1; y &= ~1; }`，
//! 即它的地表按 2×2 对齐落笔）；本 crate 的 `paint()` 对三层都是**逐格**写，
//! 所以这里 `step = 1`。理由：笔刷必须与既有落笔语义（以及 `mir2-map` 的逐格字节）一致，
//! 否则同一格会出现"笔刷写得到、填充写不到"的两套网格（`edit.rs:806-811` 原话）。
//!
//! # 三、脏标记只有一份
//!
//! 存在 [`Document::dirty`] 里（见 `document.rs` 模块文档第一节）。本模块每次改完地图都调
//! [`EditSession::sync_dirty`] 把它按 `pos != saved_pos || 笔画未结算` 重算一遍
//! （判据原样来自 `edit.rs:719-729`）。**绝不另存一份**。
//!
//! # 三之二、吸管在地表层按 2×2 对齐（与上游一致）
//!
//! 上游 `CrystalStudio/MapViewport.cs:405`：`if (Layer == MapLayer.Ground) { x &= ~1; y &= ~1; }`
//! —— `PickTile` 在读格（`:406`）之前先把地表点击吸附到偶偶格。地表格按 2×2 图块拼贴，
//! 奇格通常 `bk_img == 0`，不吸附就会把空图号吸成笔刷（随后落笔把可见地表写成 0）。
//! 本 crate 的 [`EditSession::pick_anchor`] 逐条复刻这一行，只作用于两条取笔刷入口；
//! [`EditSession::pick_cell`]（读整格信息）与 `fill` 的对齐各自独立成条，互不串味。
//!
//! # 四、测试为什么在单独文件里
//!
//! 实现 + 测试写在同一个文件会超过 800 行（任务书 §三 的硬上限）⇒ 测试段拆到
//! `session_tests.rs`，用 `#[cfg(test)] #[path]` 挂回来，两边仍属同一个模块。
//! 拆的时候**一行断言都没改**（只去掉整体缩进 4 空格）。

use std::collections::HashMap;
use std::path::Path;

use mir2_map::{Cell, CellFormat, Map};

use crate::brush::{
    clamp_brush_size, lib_ref_of, palette_index_to_sprite, Brush, LibRef, PickedCell, BRUSH_DEFAULT,
    BRUSH_MAX, BRUSH_MIN,
};
use crate::document::{cell_at, Document};
use crate::fill::{align_start, collect_region, fill_key, FillOutcome, RegionOutcome};
use crate::layer::EditLayer;
use crate::layer_counts::LayerCounts;
use crate::paint::{paint, PaintOutcome, Tool};
use crate::selection::{clear_plan, Clipboard, Selection, SelectionState};
use crate::stamp::{
    plan_stamp, PlaceOutcome, PlanMode, StampCellInput, STAMP_MAX_CELLS, STAMP_REGISTRY_MAX,
};
use crate::undo::{Change, UndoStack};

/// 一次成功填充的回执（状态栏 `填充 N 格` 的数据来源）。来源：`edit.rs:625-632`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FillRecord {
    /// 真正改动的格数（= 那一步撤销里的格数）。
    pub cells: usize,
    /// 填充时所在的编辑层（阻挡层用另一句文案）。
    pub layer: EditLayer,
}

/// 编辑会话：文档 + 工具 + 笔刷 + **当前编辑层** + **笔刷尺寸** + 撤销/重做栈 + 提示。
/// 来源：`edit.rs:583-623`。
pub struct EditSession {
    /// 文档（地图 + 路径 + 脏标记）。GUI 侧对应 `mapdoc::DocState.doc`。
    doc: Document,
    /// 当前工具。
    tool: Tool,
    /// 当前笔刷内容（`None` = 还没用吸管取过）。
    brush: Option<Brush>,
    /// **当前编辑层**（缺省 = 地表层）：落笔写哪个字段由它决定。
    layer: EditLayer,
    /// **笔刷尺寸**（3..=31 的奇数，缺省 3）：落笔范围 = 以鼠标格为中心的 N×N。
    ///
    /// 只影响"落笔范围"，**不影响任何已渲出的像素**（见 [`Self::set_brush_size`]）。
    brush_size: usize,
    /// 最近一次「拒绝落笔」的原因（`None` = 没有被拒）。状态栏的中段显示它。
    note: Option<String>,
    /// 最近一次填充的结果（`None` = 本次运行还没填过）。
    ///
    /// 为什么不复用 [`Self::note`]：`note` 只装**拒绝**原因（配警示色），
    /// 而"填了 N 格"是**成功回执**，两者语义相反（`edit.rs:598-602`）。
    last_fill: Option<FillRecord>,
    /// 最近一次**清空选区**改了几格（`None` = 还没清过）。
    ///
    /// 为什么要单独存一份：[`Self::cut_selection`] 的返回值按派单 §三 是 `(宽, 高)`，
    /// **装不下"清了几格"**，而 `Command::CutSelection` 的 `Outcome.changed` 必须报真实格数
    /// （网页层靠它决定要不要重绘）。
    ///
    /// ⚠ 与 [`Self::last_fill`] 不同：它**不随撤销/切层清空** —— 它只是"上一次清空改了几格"
    /// 这个**历史回执**，不是"当前状态"（`last_fill` 会被状态栏当现状显示，所以必须失效）。
    last_clear: Option<usize>,
    /// 撤销 / 重做栈（含当前笔画的累积缓冲）。
    undo: UndoStack,
    /// 最近一次保存的结果（`None` = 本次运行还没按过保存）。
    last_save: Option<Result<(), String>>,
    /// **印章注册表**：印章 id → 格表。
    ///
    /// 为什么要它：`Command::PlaceStamp { stamp: String }` 只带一个 **id**（本 crate 不读盘、
    /// 也不许依赖 `mir2-stamp`，见 `Cargo.toml`），所以"id → 格表"的解析结果必须有个地方放。
    /// 由**上层**（GUI/网页：它们才读 `.X`）调 [`Self::register_stamp`] 填进来，
    /// 再由 [`Self::place_stamp_by_id`] 取出来放置 —— 两边共享同一份格表，不各存一份。
    ///
    /// ⚠ 它不是"缓存"：注册是**显式**动作，id 不存在就明确拒绝（[`PlaceOutcome::Refused`]），
    /// 绝不"猜一个最像的印章"或静默放置空内容。
    stamps: HashMap<String, Vec<StampCellInput>>,
    /// **矩形选区 + 剪贴板**（2026-11-05 新增，P0-4）。
    ///
    /// 单独装成一个 [`SelectionState`]，与 C# 的 `MapEditorState`（`MapViewport.cs:147`/`:152`
    /// 把 `Selection` 与 `History` **并列**存取）同构：**选区不进撤销栈、也不置脏** ——
    /// 它只是"用户框了哪一块"这个 UI 状态，改它不产生任何字节变化。
    sel: SelectionState,
    /// **三层可见性** `[地表, 中层, 物体]`（缺省全开）。来源：`RegionCopy.cs:106-107` 的 `VisibleLayers`。
    ///
    /// 为什么放在会话上：C# 把可见性当**复制那一刻**的判据 ——
    /// `RegionCopy.cs:3` 的原注释是「layer choices are captured when copying, not when pasting」，
    /// `:107` 把 `VisibleLayers[0..2]` 直接传给 `RegionCopy` 构造函数 ⇒
    /// 本字段**只被 [`Self::copy_selection`] 读**；[`Self::paste_clipboard`] **不看它**
    /// （粘贴用的是剪贴板里已经滤好的格表，见 [`crate::selection::Clipboard::capture`]）。
    ///
    /// 纯 UI 状态：改它不改地图、不进撤销栈、不置脏。
    visible: [bool; 3],
}

impl EditSession {
    /// 新建会话（工具 = 画笔、当前层 = 地表层、尺寸 = 3、无笔刷、无撤销步）。
    /// 来源：`edit.rs:634-656` 的 `Default`。
    pub fn new(doc: Document) -> Self {
        Self {
            doc,
            // 缺省工具 = 画笔（与界面缺省选中的第 0 个图标一致，由调用方再按需覆盖）
            tool: Tool::Brush,
            brush: None,
            // 缺省当前层 = 地表层（任务书 A：默认当前层 = 地表层）
            layer: EditLayer::Ground,
            // 缺省尺寸 3（官方 `MainWindow.axaml:6` 的 `Value="3"`）
            brush_size: BRUSH_DEFAULT,
            note: None,
            last_fill: None,
            last_clear: None,
            undo: UndoStack::new(),
            last_save: None,
            stamps: HashMap::new(),
            sel: SelectionState::default(),
            // 缺省三层全开（C# 的 `VisibleLayers` 初值就是三个 true；只有全关才拒绝复制）
            visible: [true; 3],
        }
    }

    /// 从磁盘读一张图并新建会话（读不到 / 解析不了就 `Err`，绝不造一个空文档）。
    pub fn load(path: &Path) -> Result<Self, crate::CoreError> {
        Ok(Self::new(Document::load(path)?))
    }

    // ── 只读 ──

    /// 文档（地图 + 路径 + 脏标记）。
    pub fn document(&self) -> &Document {
        &self.doc
    }

    /// 可变文档 —— **仅供测试/工具直接改地图**。
    ///
    /// ⚠ 走这条路改地图**不会**自动更新脏标记、**不会**进撤销栈、**也不会**更新五层计数
    /// （它绕过了整个记账链路）。生产路径一律用 [`Self::touch`] / [`Self::fill`] /
    /// [`Self::undo`] 等方法。
    ///
    /// ⚠ 若确实要整块换视图（GUI 的 `mem::swap(document.view_mut(), &mut map)`），
    /// 换完**必须**调 [`Document::rebuild_layer_counts`]（`document.rs` 模块文档第六节）。
    pub fn document_mut(&mut self) -> &mut Document {
        &mut self.doc
    }

    /// 当前地图（**编辑/渲染视图**，只读）。
    ///
    /// ⚠ 返回的是 `mir2_map::Map`（渲染层的入口类型），它的 `format` 是**语义投影**
    /// （见 [`crate::convert::view_cell_format`]）、`header_raw` 是**视图头**（永不写回）。
    /// 字节层真值请用 [`Self::document`]`().raw()`。
    pub fn map(&self) -> &Map {
        self.doc.view()
    }

    /// **常驻告警**（`None` = 没事）。GUI/网页应当把它常驻显示。
    ///
    /// 直接转发 [`Document::edit_warning`]：未解尾块 / 本格式不承诺编辑语义，
    /// 两种来源可能同时成立（用「；」连接）。
    pub fn edit_warning(&self) -> Option<String> {
        self.doc.edit_warning()
    }

    /// 该格式的「地表逐格落笔」是否有效（`false` ⇒ 地表笔刷一律被拒）。
    ///
    /// 九种里只有 `Mir3Wemade`（Type5）为 `false`：它的背图在**块级**，逐格写 `bk_img`
    /// 会被 writer 静默丢弃（`mir2-format/src/model.rs:430-440`）。
    pub fn ground_paint_supported(&self) -> bool {
        self.doc.ground_paint_supported()
    }

    /// **五层计数**（地表 / 中层 / 物体 / 低位阻挡 / 高位阻挡）的当前值。
    ///
    /// 口径见 [`crate::layer_counts`]：官方掩码按格式归一化（legacy 的 `bit15`、
    /// Mir3 的 `flag`、u32 族的 bit29/30），**不是**直接读磁盘 bit29/30。
    ///
    /// 本会话的所有改格路径（落笔 / 填充 / 印章 / 粘贴 / 删除 / 撤销 / 重做）都会
    /// **增量**维护它（O(1)/格），所以这里可以直接读缓存。
    pub fn layer_counts(&self) -> LayerCounts {
        self.doc.layer_counts()
    }

    /// 当前工具。来源：`edit.rs:662-664`。
    pub fn tool(&self) -> Tool {
        self.tool
    }

    /// 当前笔刷内容。来源：`edit.rs:667-669`。
    pub fn brush(&self) -> Option<Brush> {
        self.brush
    }

    /// **当前编辑层**（落笔写哪一层）。来源：`edit.rs:672-674`。
    pub fn layer(&self) -> EditLayer {
        self.layer
    }

    /// 最近一次「拒绝落笔」的原因（`None` = 没被拒）。来源：`edit.rs:676-682`。
    ///
    /// 只装**拒绝**原因（族不匹配 / 图库笔刷落到阻挡层 / 填充被拒），不装"落笔字段提示"。
    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    /// **笔刷尺寸**（3..=31 的奇数）——落笔范围 = 以鼠标格为中心的 N×N。来源：`edit.rs:685-687`。
    pub fn brush_size(&self) -> usize {
        self.brush_size
    }

    /// 最近一次填充的回执（`None` = 还没填过，或填完被撤销/重做/新笔画清掉了）。
    /// 来源：`edit.rs:690-692`。
    pub fn last_fill(&self) -> Option<FillRecord> {
        self.last_fill
    }

    /// 最近一次**清空选区**（[`Self::delete_selection`] / [`Self::cut_selection`]）改了几格。
    ///
    /// `None` = 本次运行还没清过。存在理由见字段 [`Self::last_clear`] 的文档。
    pub fn last_clear(&self) -> Option<usize> {
        self.last_clear
    }

    /// 最近一次保存的结果。来源：`edit.rs:732-734`。
    pub fn last_save(&self) -> Option<&Result<(), String>> {
        self.last_save.as_ref()
    }

    /// 最近一次保存的失败原因（成功/没按过 = `None`）。来源：`edit.rs:737-742`。
    pub fn save_error(&self) -> Option<&str> {
        match self.last_save.as_ref() {
            Some(Err(e)) => Some(e.as_str()),
            _ => None,
        }
    }

    /// 撤销栈深度。来源：`edit.rs:700-702`。
    pub fn undo_len(&self) -> usize {
        self.undo.undo_len()
    }

    /// 重做栈深度。来源：`edit.rs:705-707`。
    pub fn redo_len(&self) -> usize {
        self.undo.redo_len()
    }

    /// 当前所处步号（`0` = 文件刚打开时）。来源：`edit.rs:710-712`。
    pub fn pos(&self) -> u64 {
        self.undo.pos()
    }

    /// 保存点步号。来源：`edit.rs:715-717`。
    pub fn saved_pos(&self) -> u64 {
        self.undo.saved_pos()
    }

    /// 是否有未保存的修改（真值 = [`Document::dirty`]，见模块文档第三节）。
    /// 判据来源：`edit.rs:727-729`。
    pub fn dirty(&self) -> bool {
        self.doc.dirty()
    }

    /// 是否正在一次拖动中。来源：`edit.rs:745-747`。
    pub fn stroke_active(&self) -> bool {
        self.undo.stroke_active()
    }

    /// 当前拖动最近落笔的格（插值起点）。来源：`edit.rs:750-752`。
    pub fn last_cell(&self) -> Option<(u16, u16)> {
        self.undo.last_cell()
    }

    /// 清掉「拒绝落笔」提示（用户切层/换笔刷后由内部自动调用）。来源：`edit.rs:695-697`。
    pub fn clear_note(&mut self) {
        self.note = None;
    }

    // ── 状态变更 ──

    /// 选工具；返回该工具是否已接线。来源：`edit.rs:757-760`（GUI 收的是图标下标，这里收工具本身）。
    pub fn set_tool(&mut self, tool: Tool) -> bool {
        self.tool = tool;
        self.tool.is_wired()
    }

    /// 选**当前编辑层**（写哪一层由它决定）。来源：`edit.rs:766-773`。
    ///
    /// 切层会立刻重算「族不匹配」提示：上一层的笔刷很可能配不上新层，
    /// 那种情况下状态栏应当**马上**说出来，而不是等用户点下去才发现没反应。
    pub fn set_layer(&mut self, layer: EditLayer) {
        self.layer = layer;
        // 上一次填充的回执说的是**另一层**的格数，留着会张冠李戴
        if self.last_fill.map(|f| f.layer) != Some(layer) {
            self.last_fill = None;
        }
        self.refresh_layer_note();
    }

    /// 设**笔刷尺寸**；返回真正生效的尺寸（夹进 3..=31 的奇数）。来源：`edit.rs:784-787`。
    ///
    /// ## ⚠ 改尺寸**不重渲**（也不改地图、不进撤销栈、不动脏标记）
    ///
    /// 尺寸只决定"下一次落笔覆盖多大一块"，已写进地图的字节一个都不变。
    /// 官方 `MainWindow.axaml.cs:609` 的 `BrushChanged` 同样只改 `Viewport.BrushSize`
    /// 与标签文字（+ 重画笔刷预览），"不碰地图数据"。
    pub fn set_brush_size(&mut self, n: usize) -> usize {
        self.brush_size = clamp_brush_size(n);
        self.brush_size
    }

    /// 笔刷尺寸按 `delta` 走（官方 `[` / `]` = ∓2）；返回真正生效的尺寸。来源：`edit.rs:790-797`。
    pub fn grow_brush_size(&mut self, delta: isize) -> usize {
        let cur = self.brush_size as isize;
        let next = (cur + delta).clamp(BRUSH_MIN as isize, BRUSH_MAX as isize) as usize;
        self.set_brush_size(next)
    }

    /// 标记「已保存到磁盘」：清脏标记并把保存点挪到当前位置。来源：`edit.rs:837-839`。
    pub fn mark_saved(&mut self) {
        self.undo.mark_saved();
        self.doc.mark_clean();
    }

    /// 记一次保存成功。来源：`edit.rs:842-844`。
    pub fn note_save_ok(&mut self) {
        self.last_save = Some(Ok(()));
    }

    /// 记一次保存失败（原因原样保留，状态栏要显示它）。来源：`edit.rs:847-849`。
    pub fn note_save_err(&mut self, err: String) {
        self.last_save = Some(Err(err));
    }

    /// 以 `(x, y)` 为中心的 **N×N 落笔格**（已按地图边界裁剪）。来源：`edit.rs:812-827`。
    ///
    /// 形状/裁剪判据全在 [`crate::footprint_of`] 里（单一实现）；本方法只负责
    /// "用当前笔刷尺寸 + 自己文档的宽高"去调它。
    pub fn footprint(&self, x: u16, y: u16) -> Vec<(u16, u16)> {
        crate::footprint_of(
            self.brush_size,
            x,
            y,
            self.doc.view().width,
            self.doc.view().height,
        )
    }

    /// 按「笔刷 vs 当前编辑层」重算拒绝提示：不匹配就写原因，匹配就清掉。来源：`edit.rs:832-834`。
    fn refresh_layer_note(&mut self) {
        self.note = layer_note(self.brush, self.layer);
    }

    /// 把脏标记按判据重算并写回 [`Document`]（**唯一写入点**，见模块文档第三节）。
    fn sync_dirty(&mut self) {
        let v = self.undo.dirty();
        debug_assert_eq!(
            v,
            self.undo.pos() != self.undo.saved_pos() || self.undo.stroke_pending(),
            "脏判据两条写法必须等价"
        );
        self.doc.set_dirty(v);
    }

    // ── 核心：改格 ──

    /// 开始一次拖动。来源：`edit.rs:855-862`（未结算的上一笔由 [`UndoStack::begin`] 先结算）。
    pub fn begin(&mut self) {
        self.undo.begin();
        // `begin` 可能结算了上一笔 ⇒ 脏标记要重算（否则"刚结算完就不脏"会被漏掉）
        self.sync_dirty();
    }

    /// 在当前拖动里把工具作用到 `(x, y)`；返回是否真的改了这一格（0 或 1）。
    /// 来源：`edit.rs:871-918`。
    ///
    /// 同一格在一次拖动里重复经过只记第一次：返回 0（值已经写过了）。
    /// **族不匹配时返回 0 且一字节不动**，并把原因记进 [`Self::note`]。
    ///
    /// # 两道「格式能力」闸门（2026-11-05 接入 `mir2-format` 后新增）
    ///
    /// 1. **地表闸门**（[`Self::ground_refusal`]）：格式声明 `ground_paint_supported=false`
    ///    （九种里只有 Type5）⇒ **落笔前**就拒绝，连 `mark_seen` 都不做
    ///    （否则同一笔里再经过这格会被当成"已改过"而静默放过）；
    /// 2. **库号字节闸门**：`paint` 只按 `CellFormat::has_lib_bytes()` 决定写不写库号字节，
    ///    不知道具体格式的 writer 写不写那个字节 ⇒ 写不进去的必须**还原成改前值**
    ///    （见 [`crate::convert`] 模块文档第六节）。
    pub fn touch(&mut self, x: u16, y: u16) -> usize {
        if !self.undo.stroke_active() {
            return 0;
        }
        // ① 地表闸门：必须在**任何写入之前**（连 mark_seen 都不做）
        if let Some(why) = self.ground_refusal() {
            self.note = Some(why);
            return 0;
        }
        let Some(before) = cell_at(self.doc.view(), x, y).copied() else {
            return 0; // 越界
        };
        if !self.undo.mark_seen(x, y) {
            return 0; // 本次拖动已经改过这一格
        }
        let fmt = self.doc.view().format;
        let mut after = match paint(fmt, before, self.tool, self.brush, self.layer) {
            PaintOutcome::Write(after) => after,
            // 未接线工具 / 画笔还没取内容：**不是错误**，什么都不改
            PaintOutcome::NoOp => return 0,
            PaintOutcome::LayerMismatch { brush, layer } => {
                // 拒绝：一格都不许写，然后把原因交给状态栏（绝不静默）
                self.note = Some(format!(
                    "笔刷族（{}）与当前图层（{}）不一致",
                    brush.label(),
                    layer.name()
                ));
                return 0;
            }
            PaintOutcome::PaletteBrushOnCollision { brush, layer } => {
                // 阻挡层不接受图库笔刷：不写位、也不写图号，把原因交给状态栏
                self.note = Some(format!(
                    "笔刷来自图库（{}），{}只改 0x8000 位、不写图号",
                    brush.label(),
                    layer.name()
                ));
                return 0;
            }
        };
        // ② 库号字节闸门：写不进去的还原成改前值（只动该层那一个字节）
        let dropped_lib = crate::convert::restore_unwritten_lib_byte(
            self.doc.format_id(),
            self.layer,
            &before,
            &mut after,
        );
        if dropped_lib {
            self.note = Some(self.lib_byte_drop_note());
        }
        if after == before {
            return 0; // 值没变，不记步（否则"空刷一遍也算一步"）
        }
        // 真的写进去了 ⇒ 上一次的拒绝提示自动失效（提示必须跟状态一致）。
        // ⚠ 例外：本次刚发生的「库号字节写不进去」是**当前**事实，不许被清掉。
        if !dropped_lib {
            self.note = None;
        }
        // 写视图 + 增量更新五层计数（**唯一**逐格写入口，见 `document.rs` 模块文档第六节）。
        // 返回 `false` = 形状不变量被破坏（前面 `cell_at` 已确认在界内，理论上不可达）
        // ⇒ 如实不记步，绝不写一个「计数说不清」的格。
        if !self.doc.write_view_cell(x, y, after) {
            return 0;
        }
        self.undo.push(Change {
            x,
            y,
            before,
            after,
        });
        self.sync_dirty();
        1
    }

    /// 「地表层落笔被格式能力拒绝」的原因（`None` = 允许落笔）。
    ///
    /// 判据：`Document::ground_paint_supported()` 为 `false` **且**当前层写的是 `bk_img`
    /// （地表 [`EditLayer::Ground`] 与低位阻挡 [`EditLayer::LowCollision`] 都写 `bk_img`，
    /// 见 `layer.rs:242-244`）⇒ 逐格改 `bk_img` 一定被 writer 丢掉。
    ///
    /// 文案必须带**中文原因**（任务书 §七「拒绝必须返回明确 outcome」），
    /// 且点明「Type5 背图在块级，逐格写 bk_img 无效」。
    fn ground_refusal(&self) -> Option<String> {
        if self.doc.ground_paint_supported() {
            return None;
        }
        if !matches!(self.layer, EditLayer::Ground | EditLayer::LowCollision) {
            return None;
        }
        Some(format!(
            "Type5 背图在块级，逐格写 bk_img 无效 —— 本格式（{}）的背景图号来自半分辨率 \
             3 B / 2×2 块，Cell::bk_img 恒 0 且写回时不参与编码；已拒绝落笔，地图一个字节都没动",
            self.doc.format_label()
        ))
    }

    /// 「库号字节写不进去」的提示（由 [`crate::convert::writes_layer_lib_byte`] 判定）。
    ///
    /// ⚠ **旧文案是假陈述，已订正（2026-11-05）**：原文写「该格式的写回路径不写它」，
    /// 但 `Old12`/`AntiHack` 的物体层（`area`）**是写的** ——
    /// `mir2-format/src/formats/old12.rs:158` 的 `out.push(cell.area)`。
    /// 现在按 `convert.rs` 的权威表逐格判，文案只说**准确**的那部分：
    /// 「该层在本格式里没有独立的库号字节」。
    fn lib_byte_drop_note(&self) -> String {
        format!(
            "{} 在 {} 里没有独立的库号字节（该格式的格布局里这一层不带库号）—— \
             库号字节未改，只改图号",
            self.layer.lib_field().unwrap_or("库号"),
            self.doc.format_label()
        )
    }

    /// 在当前拖动里把工具作用到**以 `(x, y)` 为中心的 N×N**；返回真的改了几格。
    /// 来源：`edit.rs:926-937`。
    ///
    /// 这是**画笔 / 橡皮**的落笔入口（官方 `MapViewport.cs:473-482`）：
    /// 尺寸只对这两个工具生效，铅笔与填充恒为 1 格（[`Tool::uses_brush_size`]）。
    /// 吸管/未接线工具**不吃尺寸**，直接落到单格（它们本来就不改地图）。
    pub fn touch_footprint(&mut self, x: u16, y: u16) -> usize {
        if !self.tool.uses_brush_size() {
            return self.touch(x, y);
        }
        let cells = self.footprint(x, y);
        let mut n = 0;
        for (cx, cy) in cells {
            n += self.touch(cx, cy);
        }
        n
    }

    /// 结束一次拖动：把累积变更压成**一步**；返回本步改了几格。来源：`edit.rs:940-960`。
    pub fn end(&mut self) -> usize {
        let n = self.undo.end();
        self.sync_dirty();
        n
    }

    /// 单格一步（按下即松开、或脚本驱动时用）：`begin + touch + end`。来源：`edit.rs:966-971`。
    ///
    /// ⚠ **恒为 1 格**（不吃笔刷尺寸）：这是"精确落一格"的底层原语，
    /// 鼠标落笔走的是 [`Self::paint_stroke_once`]（吃尺寸）。
    pub fn apply_once(&mut self, x: u16, y: u16) -> usize {
        self.begin();
        let n = self.touch(x, y);
        self.end();
        n
    }

    /// 以 `(x, y)` 为中心按**当前笔刷尺寸**落一笔（= `begin + touch_footprint + end`）。
    /// 来源：`edit.rs:977-982`。
    ///
    /// 鼠标"按下即松开"与脚本 `--edit-brush` 走的都是这条（吃尺寸）。
    pub fn paint_stroke_once(&mut self, x: u16, y: u16) -> usize {
        self.begin();
        let n = self.touch_footprint(x, y);
        self.end();
        n
    }

    /// 从 `from` 拖到 `to`（Bresenham 插值，**整条轨迹都按 N×N 落**），结算成**一步**。
    /// 来源：`edit.rs:988-1002`。
    ///
    /// 这就是鼠标拖动的完整语义（GUI 的 `paint_to` 与脚本 `--edit-stroke` 共用）：
    /// ① 插值补格（快速拖动时指针会跳格）；② 每个插值点落 N×N；③ 全部并进同一步撤销。
    pub fn paint_stroke(&mut self, from: Option<(u16, u16)>, to: (u16, u16)) -> usize {
        self.begin();
        let start = from.unwrap_or(to);
        let mut n = 0;
        for (cx, cy) in line_cells(start, to) {
            n += self.touch_footprint(cx, cy);
        }
        self.end();
        n
    }

    // ── 填充（Bucket） ──

    /// 填充：从 `(x, y)` 出发，BFS 扩散到「**同层且该层图号相同**」的连通块，用当前笔刷填满。
    /// 来源：`edit.rs:1036-1206`（判据表见 [`crate::fill`] 模块文档）。
    ///
    /// 顺序与 GUI 逐条一致：① 层闸门 → ①b 地表对齐偶偶格 → ② 越界 → ③ 笔刷闸门 →
    /// ④ **先算** `clicked_after`（拒绝必须发生在动笔之前）→ ⑤ 已是目标值早退 →
    /// ⑥ 只读收块（超限即停）→ ⑦ 算 `after`、写盘、**整批压成一步**。
    ///
    /// **拒绝时地图一个字节都没动**，且 [`FillOutcome::note`] 一定给得出原因。
    pub fn fill(&mut self, x: u16, y: u16) -> FillOutcome {
        // ① 层闸门（官方 MapViewport.cs:488-492：只支持地表 / 中层 / 低高位阻挡）
        if !matches!(
            self.layer,
            EditLayer::Ground | EditLayer::Middle | EditLayer::LowCollision | EditLayer::HighCollision
        ) {
            return self.refuse_fill(FillOutcome::UnsupportedLayer);
        }
        // ①b 地表的网格步长 = 2（官方 :496-497），对齐到偶偶格
        let (x, y) = align_start(self.layer, x, y);
        // ①c 地表闸门（2026-11-05 接入 `mir2-format` 后新增）：格式声明地表逐格落笔无效
        //     （九种里只有 Type5）⇒ 在**动笔之前**明确拒绝，一格都不写。
        if let Some(why) = self.ground_refusal() {
            return self.refuse_fill_with(FillOutcome::FormatCannotPaintGround, why);
        }
        // ② 越界闸门（官方 :493）
        let Some(clicked) = cell_at(self.doc.view(), x, y).copied() else {
            return self.refuse_fill_with(
                FillOutcome::OutOfMap,
                format!("点击格 ({x},{y}) 在地图外"),
            );
        };
        // ③ 笔刷闸门：图号层必须有笔刷；阻挡层不用笔刷（官方 :494）
        let brush = if self.layer.is_collision() {
            None
        } else {
            match self.brush {
                Some(b) => Some(b),
                None => return self.refuse_fill(FillOutcome::NoBrush),
            }
        };
        let fmt = self.doc.view().format;
        // ④ 先把"点下去会改成什么"算出来：拒绝（族不匹配等）必须发生在**动笔之前**
        let mut clicked_after = match paint(fmt, clicked, Tool::Brush, brush, self.layer) {
            PaintOutcome::Write(c) => c,
            PaintOutcome::NoOp => return self.refuse_fill(FillOutcome::NoBrush),
            // 阻挡层收到图库笔刷也归到 LayerMismatch（edit.rs:1102-1110 同一归类）
            PaintOutcome::LayerMismatch { .. } | PaintOutcome::PaletteBrushOnCollision { .. } => {
                return self.refuse_fill(FillOutcome::LayerMismatch)
            }
        };
        // ④b 库号字节闸门：写不进去的还原成改前值（见 convert.rs 模块文档第六节）
        let dropped_lib = crate::convert::restore_unwritten_lib_byte(
            self.doc.format_id(),
            self.layer,
            &clicked,
            &mut clicked_after,
        );
        let key = fill_key(self.layer, &clicked);
        // ⑤ 已是目标值 ⇒ 官方 :501 `if (source == (…Library, image)) return 0;`
        if key == fill_key(self.layer, &clicked_after) {
            self.note = if dropped_lib {
                Some(self.lib_byte_drop_note())
            } else {
                None
            };
            return FillOutcome::AlreadyFilled;
        }

        // ⑥ BFS 收连通块（**只读**；超上限立刻停手，一格都不写）
        let region = match collect_region(self.doc.view(), self.layer, x, y) {
            RegionOutcome::Region(r) => r,
            RegionOutcome::TooLarge { cells } => {
                return self.refuse_fill(FillOutcome::TooLarge { cells })
            }
            RegionOutcome::OutOfMap { x, y } => {
                return self.refuse_fill_with(
                    FillOutcome::OutOfMap,
                    format!("点击格 ({x},{y}) 在地图外"),
                )
            }
        };

        // ⑦ 逐格算出 after（值没变的格不记步），再一次性写进地图 + 压成**一步**
        let mut changes: Vec<Change> = Vec::with_capacity(region.len());
        let mut after_cells: Vec<(u16, u16, mir2_map::Cell)> = Vec::with_capacity(region.len());
        for (cx, cy) in region {
            let Some(before) = cell_at(self.doc.view(), cx, cy).copied() else {
                continue;
            };
            let mut after = match paint(fmt, before, Tool::Brush, brush, self.layer) {
                PaintOutcome::Write(c) => c,
                // 走到这里说明 BFS 期间判据变了（理论上不会）：保守地跳过这一格
                _ => continue,
            };
            // ⑦b 库号字节闸门（逐格同一口径）
            crate::convert::restore_unwritten_lib_byte(
                self.doc.format_id(),
                self.layer,
                &before,
                &mut after,
            );
            if after == before {
                continue;
            }
            changes.push(Change {
                x: cx,
                y: cy,
                before,
                after,
            });
            after_cells.push((cx, cy, after));
        }
        if changes.is_empty() {
            self.note = None;
            return FillOutcome::AlreadyFilled;
        }
        let n = changes.len();
        for (cx, cy, after) in after_cells {
            // 写视图 + 增量更新计数（与 `touch()` 同一入口）
            self.doc.write_view_cell(cx, cy, after);
        }
        // 复用拖动那套记账：一次填充 = 撤销栈里的**一步**（edit.rs:1195-1199）
        self.undo.commit(changes);
        self.note = None;
        self.last_fill = Some(FillRecord {
            cells: n,
            layer: self.layer,
        });
        self.sync_dirty();
        FillOutcome::Filled(n)
    }

    /// 记下"填充被拒"的原因（调用方靠它上屏），并把上一次的回执清掉。来源：`edit.rs:1209-1213`。
    fn refuse_fill(&mut self, why: FillOutcome) -> FillOutcome {
        let note = why.note(self.layer, self.brush.map(|b| b.family));
        self.refuse_fill_with(why, note.unwrap_or_else(|| "填充被拒绝".to_string()))
    }

    /// 同上，但原因由调用方给（越界那种要带具体坐标的）。
    fn refuse_fill_with(&mut self, why: FillOutcome, note: String) -> FillOutcome {
        self.last_fill = None;
        self.note = Some(note);
        why
    }

    // ── 印章（放置） ──

    /// **注册**一枚印章的格表（供 [`Self::place_stamp_by_id`] / `Command::PlaceStamp` 用）。
    ///
    /// 成功返回该 id 下**现在**有多少格。**同一个 id 再注册 = 覆盖**（不是追加）——
    /// 选"覆盖"而不是"幂等丢弃"：上层改完一枚 `.X` 再注册时，要的必然是**新内容**；
    /// 若选"丢弃"，用户会看到"改了印章、贴出来还是旧的"。覆盖也**不占**新槽位
    /// （注册表满时仍可覆盖已有 id）。
    ///
    /// # 为什么由上层注册
    ///
    /// `.X` 的解析在 `mir2-stamp`（本 crate 只在 dev-dependencies 里依赖它），
    /// 本 crate **不读盘、零 IO**（任务书 §二：`stamp = 印章 id`，core 侧只存 id 供上层解释）。
    /// 上层（GUI/网页：它们才持有 `StampCatalog`）解析出格表后交进来，
    /// `Command::PlaceStamp { stamp }` 才有真实现可用。
    ///
    /// # 三道护栏（都是**明确拒绝**，绝不静默截断）
    ///
    /// 1. **空格表** ⇒ `Err`（空印章没有任何可放置的内容，注册它只会让后面"放了 0 格"难以解释）；
    /// 2. **单枚超过 [`STAMP_MAX_CELLS`]**（1,000,000 格）⇒ `Err`（同 C# `Document.cs:140`
    ///    的 `if (list.Count > 1_000_000) throw`）；
    /// 3. **注册表满**（[`STAMP_REGISTRY_MAX`] 个 id）⇒ `Err`，原因里给出"先 [`Self::remove_stamp`] 腾位"。
    ///    为什么要有上限：注册表按 id 存整份格表，一个 1e6 格的印章 ≈ 20 MB，
    ///    没有上限时"每次点素材面板都注册一遍"会把内存吃穿。
    pub fn register_stamp(
        &mut self,
        id: impl Into<String>,
        cells: Vec<StampCellInput>,
    ) -> Result<usize, String> {
        let id = id.into();
        if cells.is_empty() {
            return Err(format!(
                "印章 {id:?} 的格表是空的 —— 空印章没有可放置的内容，未注册（地图未改动）"
            ));
        }
        if cells.len() > STAMP_MAX_CELLS {
            return Err(format!(
                "印章 {id:?} 有 {} 格，超过单枚上限 {STAMP_MAX_CELLS} 格（同 C# Document.cs:140 的 \
                 1,000,000）—— 未注册（地图未改动）",
                cells.len()
            ));
        }
        // 覆盖已有 id 不占新槽位 ⇒ 只有在"新增一个 id"时才判满
        if !self.stamps.contains_key(&id) && self.stamps.len() >= STAMP_REGISTRY_MAX {
            return Err(format!(
                "印章注册表已满（{STAMP_REGISTRY_MAX} 个 id，本仓护栏，不是原版限制）：{id:?} 未注册。\
                 先调 remove_stamp 腾位，或复用已有 id（再注册同一个 id 是覆盖、不受此限）"
            ));
        }
        let n = cells.len();
        self.stamps.insert(id, cells);
        Ok(n)
    }

    /// 注销一枚印章；返回是否真的删掉了（未注册 ⇒ `false`）。
    ///
    /// 注册表满时用它腾位（见 [`Self::register_stamp`] 的第 3 条护栏）。
    pub fn remove_stamp(&mut self, id: &str) -> bool {
        self.stamps.remove(id).is_some()
    }

    /// 已注册的印章 id（**按字典序**，便于日志/界面稳定复现）。
    pub fn stamp_ids(&self) -> Vec<&str> {
        let mut ids: Vec<&str> = self.stamps.keys().map(String::as_str).collect();
        ids.sort_unstable();
        ids
    }

    /// 取某个 id 的格表（未注册 ⇒ `None`）。
    pub fn stamp_cells(&self, id: &str) -> Option<&[StampCellInput]> {
        self.stamps.get(id).map(Vec::as_slice)
    }

    /// **以 `(x,y)` 为锚点放置一枚印章**；`dx,dy` 为相对偏移（可为负）。
    ///
    /// 语义全在 [`crate::stamp`] 模块文档第一节（逐条带来源），这里只做三件事：
    /// ① 调 [`plan_stamp`] 算计划（**先算后写**：拒绝时一格不写）；
    /// ② 把计划里的 `after` 写进视图；
    /// ③ 整批压成**一步**撤销（[`UndoStack::commit`]，内部就是 `begin(); stroke = changes; end();`）。
    ///
    /// 拒绝时把中文原因同时写进 [`Self::note`]（状态栏能直接显示），
    /// 并把**一格都没写**这一事实如实返回（[`PlaceOutcome::Refused`]）。
    pub fn place_stamp(&mut self, x: u16, y: u16, cells: &[StampCellInput]) -> PlaceOutcome {
        // 口径 = C# `PlaceStamp`（`PlacedStamps.cs:114-121`，内部先调 `CanPlaceStamp`）
        // ⇒ 走 [`PlanMode::Stamp`]：**占用闸门**（落点已有物件 ⇒ 整枚拒绝）与
        // **坐标重复闸门**（`(dx,dy)` 重复 ⇒ 整枚拒绝）都生效。
        self.place_stamp_with_mode(x, y, cells, PlanMode::Stamp)
    }

    /// [`Self::place_stamp`] 与 [`Self::paste_clipboard`] 的**共同实现**，差别只在 `mode`。
    ///
    /// 为什么要分口径：C# 里 `PlaceStamp`（`PlacedStamps.cs:114`）先过 `CanPlaceStamp`
    /// （含占用/坐标重复两道闸门），而 `RegionCopy.Paste`（`RegionCopy.cs:72`）只有
    /// `CanPaste`（纯边界判断）。两者前置条件不同 ⇒ 闸门也必须不同。
    fn place_stamp_with_mode(
        &mut self,
        x: u16,
        y: u16,
        cells: &[StampCellInput],
        mode: PlanMode,
    ) -> PlaceOutcome {
        let plan = match plan_stamp(self.doc.view(), self.doc.format_id(), x, y, cells, mode) {
            Ok(plan) => plan,
            Err(reason) => {
                self.note = Some(reason.clone());
                return PlaceOutcome::Refused { reason };
            }
        };
        // 写视图（`changes` 已去重：同一格只有一条，`after` 是最终值）+ 增量更新计数
        for c in &plan.changes {
            self.doc.write_view_cell(c.x, c.y, c.after);
        }
        let n = self.undo.commit(plan.changes);
        // 成功也要能说出"哪一层的库号字节被忽略"（那是当前事实，不许被清成 None）
        self.note = if plan.notes.is_empty() {
            None
        } else {
            Some(plan.notes.join("；"))
        };
        self.sync_dirty();
        PlaceOutcome::Placed {
            cells: n,
            skipped: plan.skipped,
        }
    }

    /// 按 **id** 放置一枚**已注册**的印章（`Command::PlaceStamp` 的唯一实现路径）。
    ///
    /// 未注册的 id ⇒ [`PlaceOutcome::Refused`]，原因里带上"已注册哪些 id"，
    /// 让人一眼看出是**没注册**而不是"印章坏了"。
    pub fn place_stamp_by_id(&mut self, x: u16, y: u16, id: &str) -> PlaceOutcome {
        let Some(cells) = self.stamps.get(id).cloned() else {
            let 已注册 = self.stamp_ids();
            let 清单 = if 已注册.is_empty() {
                "（当前一个都没注册）".to_string()
            } else {
                format!("（已注册：{}）", 已注册.join(" / "))
            };
            let reason = format!(
                "未注册的印章 id：{id:?}{清单} —— 本 crate 不读盘，印章格表必须由上层先调 \
                 register_stamp 注册，地图一个字节都没动"
            );
            self.note = Some(reason.clone());
            return PlaceOutcome::Refused { reason };
        };
        self.place_stamp(x, y, &cells)
    }

    // ── 三层可见性（2026-11-05 新增，复制按可见性过滤） ──

    /// 三层可见性 `[地表, 中层, 物体]`（缺省全开）。来源：`RegionCopy.cs:106-107` 的 `VisibleLayers`。
    ///
    /// 顺序与 C# 一致：`VisibleLayers[0]` = 地表、`[1]` = 中层、`[2]` = 物体
    /// （`RegionCopy.cs:107` 就是按这个顺序传给 `RegionCopy` 构造函数的）。
    pub fn visible_layers(&self) -> [bool; 3] {
        self.visible
    }

    /// 设三层可见性 `[地表, 中层, 物体]`。来源：`RegionCopy.cs:107`。
    ///
    /// 只影响**之后**的 [`Self::copy_selection`]：C# 在复制瞬间把可见性拍进快照
    /// （`RegionCopy.cs:3`、`:107`）⇒ 已经复制进剪贴板的内容**不会被重滤**，
    /// 之后再改可见性也不影响粘贴结果（[`Self::paste_clipboard`] 不读本状态）。
    ///
    /// 纯 UI 状态：不改地图、不进撤销栈、不置脏。
    pub fn set_visible_layers(&mut self, vis: [bool; 3]) {
        self.visible = vis;
    }

    // ── 矩形选区 / 剪贴板（2026-11-05 新增，审计 P0-4） ──

    /// 当前矩形选区（`None` = 没有选区）。来源：`MapViewport.cs:102`（`Rect? Selection`）。
    ///
    /// 选区是 **UI 状态**：它不改地图、不进撤销栈、不动脏标记
    /// （C# 把 `Selection` 与 `History` **并列**存进 `MapEditorState`，`MapViewport.cs:147`/`:152`）。
    pub fn selection(&self) -> Option<Selection> {
        self.sel.sel
    }

    /// 设选区（**越界裁剪**；返回裁剪后是否非空）。来源：`MapViewport.cs:390`、`:399`、`:400`。
    ///
    /// 裁剪判据全在 [`Selection::clamp_to`]（单一实现）。本方法只做三件事：
    /// ① 裁剪；② 整块在图外 ⇒ **取消选区**并给中文原因；③ 成功 ⇒ 清掉上一次的拒绝提示。
    ///
    /// 复刻的两条 C# 不变量：起点必须图内（`:390` `if (!Document.Contains(x, y)) return false;`）、
    /// 终点夹到 `0..=Width-1`（`:399` `Math.Clamp`）⇒ **存下来的选区永远整块在图内**，
    /// 后面所有迭代都不必再判越界。
    pub fn set_selection(&mut self, sel: Selection) -> bool {
        let (w, h) = self.doc.size();
        match sel.clamp_to(w, h) {
            Some(s) => {
                self.sel.sel = Some(s);
                self.note = None;
                true
            }
            None => {
                self.sel.sel = None;
                self.note = Some(format!(
                    "选区整块在地图外（地图 {w}×{h}，选区 ({},{})..({},{})），已取消选区 —— \
                     地图一个字节都没动",
                    sel.x0, sel.y0, sel.x1, sel.y1
                ));
                false
            }
        }
    }

    /// 取消选区（= C# 的 `Deselect()`）。来源：`MapViewport.cs:103-108`。
    ///
    /// **不清剪贴板**：剪贴板是独立的一份快照（C# 的 `copiedRegion` 也不随 `Deselect` 清空，
    /// `RegionCopy.cs:96`、`:114`）。
    pub fn clear_selection(&mut self) {
        self.sel.sel = None;
    }

    /// 当前剪贴板（`None` = 还没复制过）。来源：`RegionCopy.cs:96`（`private RegionCopy? copiedRegion;`）。
    pub fn clipboard(&self) -> Option<&Clipboard> {
        self.sel.clip.as_ref()
    }

    /// 把选区**复制**进剪贴板；返回 `(宽, 高)`。来源：`RegionCopy.cs:101`、`:103-110`、`:21-27`。
    ///
    /// 没有选区 / 选区为空 / **三层全关** ⇒ `None`，且**剪贴板保持原样**（不静默清空 —— 与
    /// [`Self::pick_at`] 越界不清笔刷同一条纪律），原因写进 [`Self::note`]。
    ///
    /// 抓取口径（整块都抓、含空格、**整格**：三族图号 + 三个库号 + 门/光照/前景动画、
    /// 图号只存低 15 位）见
    /// [`Clipboard::capture`] 与 `selection.rs` 模块文档 §三。
    ///
    /// **按可见性过滤**（`RegionCopy.cs:106-107` + `:78-82`）：隐藏层的族一律不抓，
    /// 对应字段填 0 ⇒ 粘贴时那一族自然不写（`paste_clipboard` 不读当前可见性，
    /// 只吃这份已滤好的格表）。
    pub fn copy_selection(&mut self) -> Option<(u16, u16)> {
        let Some(sel) = self.sel.sel else {
            self.note = Some(
                "没有选区：先框一个矩形选区（C# 里是 M 工具拖拽，MapViewport.cs:412-415），剪贴板未变"
                    .to_string(),
            );
            return None;
        };
        // 三层全关 ⇒ 拒绝。判据来源：`RegionCopy.cs:106`
        // `if (!VisibleLayers.Any(v => v) && !CollisionVisible) { Message?.Invoke("Show at least one layer to copy"); return; }`
        //
        // ⚠ 与 C# 的**唯一口径差**：那边还要求「碰撞层也不可见」才拒；本 crate 的剪贴板
        //    **不抓阻挡位**（`Clipboard::capture` 只取低 15 位，见该函数文档）⇒ 三层全关就是
        //    "一格可抓的东西都没有"，不需要碰撞层这个第二条件。
        // 拒绝时：剪贴板保持原样、地图一格不动、不产生撤销步（本函数本来就不碰 `undo`）。
        if !self.visible.iter().any(|v| *v) {
            self.note = Some(
                "三层（地表 / 中层 / 物体）全部隐藏：没有可复制的内容 —— 至少显示一层再复制\
                 （C# 同一判据 RegionCopy.cs:106）；剪贴板未变，地图一个字节都没动"
                    .to_string(),
            );
            return None;
        }
        let clip = Clipboard::capture(self.doc.view(), sel, self.visible);
        if clip.is_empty() {
            self.note = Some(format!(
                "选区 ({},{})..({},{}) 是空的（宽或高为 0），剪贴板未变",
                sel.x0, sel.y0, sel.x1, sel.y1
            ));
            return None;
        }
        let (w, h) = (clip.width(), clip.height());
        self.sel.clip = Some(clip);
        self.note = None;
        Some((w, h))
    }

    /// **剪切** = 复制 + 清空（**整次 = 一步撤销**）；返回 `(宽, 高)`。
    /// 来源：`RegionCopy.cs:102`（`CutSelection`）、`:108`（`if (cut) … CutSource(…)`）、`:45-69`。
    ///
    /// 复制失败（没有选区）⇒ `None` 且**一格不动**（`copy_selection` 的 `?` 提前返回）。
    ///
    /// # 回执只出**一句**（2026-11-05 实测修正）
    ///
    /// 曾经的写法是"复制段 + 删除段"两段拼接 ⇒ 同一条 note 里既说「N 格被清空」
    /// 又说「没有可清的内容，未改动」，自相矛盾。现在改成一句里带两个数：
    /// `剪切 W×H 格（已复制 N 格进剪贴板；清空 M 格）`，`M == 0` 时明写"选区内容本来就是空的"。
    ///
    /// **清空也按可见性**（`RegionCopy.cs:108` → `:52/57/61` 的三个 `if (ground/middle/objects && …)`）：
    /// 隐藏的层既没进剪贴板、也**不会被清掉** —— 否则"复制滤掉、剪切却删掉"会造成不对称的数据丢失。
    pub fn cut_selection(&mut self) -> Option<(u16, u16)> {
        let (w, h) = self.copy_selection()?;
        // 复制到的格数 = 宽 × 高（整块都抓，含空格；`selection.rs` §三 第 1 条）
        let 已复制 = usize::from(w) * usize::from(h);
        // ⚠ 这里传**当前可见性**（不是 `[true; 3]`）：C# 的 `CutSource` 就是按复制时定格的
        // `ground/middle/objects` 逐族清（`RegionCopy.cs:52/57/61`），隐藏的层不动。
        let (m, 告警) = match self.clear_selection_now(self.visible) {
            Ok(r) => r,
            // 刚复制成功 ⇒ 选区一定还在，这条不可达；真到了也不 panic，如实拒绝
            Err(reason) => {
                self.last_clear = Some(0);
                self.note = Some(reason);
                return None;
            }
        };
        self.last_clear = Some(m);
        let mut s = if m == 0 {
            format!(
                "剪切 {w}×{h} 格（已复制 {已复制} 格进剪贴板；选区内容本来就是空的，清空 0 格）"
            )
        } else {
            format!("剪切 {w}×{h} 格（已复制 {已复制} 格进剪贴板；清空 {m} 格）")
        };
        for w2 in 告警 {
            s.push_str("；另：");
            s.push_str(&w2);
        }
        self.note = Some(s);
        Some((w, h))
    }

    /// 以 `(x, y)` 为**鼠标/点击格**粘贴剪贴板；返回放置结果。
    ///
    /// 来源：`RegionCopy.cs:72-91`（`Paste`）+ `:70`（`Anchor`）+ `:121-122`（`PlaceRegion`）。
    /// 语义与 [`Self::place_stamp`] **同一份实现**：
    /// 逐格越界跳过（`skipped` 如实报数）、三族图号为 0 的族不写、保留目标格 `bit15`、
    /// Type5 闸门、整次 = 一步撤销（`selection.rs` 模块文档 §三/§四）。
    ///
    /// # 锚点规则（2026-11-06 对齐上游，**粘贴落点 ≠ 点击点**）
    ///
    /// 上游 `PlaceRegion`（`RegionCopy.cs:121-122`）是**先** `Anchor(hover.X, hover.Y)` **再**
    /// `CanPaste`：`Anchor`（`:70`）在地表层可见时按**源的奇偶**把左上角往左上对齐
    /// （`x - ((x - sourceX) & 1)`），负锚点 ⇒ `CanPaste` 为 false ⇒ **整块拒绝**。
    /// 本方法逐条照此执行（公式与边界全在 [`Clipboard::anchor`]）：
    ///
    /// 1. 先 `clip.anchor(x, y)`，拿到 `Some((ax, ay))` 才进 `plan_stamp`（用 **`(ax, ay)`** 当左上角）；
    /// 2. `anchor` 返回 `None`（负锚点）⇒ **整块拒绝**，剪贴板放回、地图一个字节不动、
    ///    原因里点名"锚点为负"与 `CanPaste`（**不许**夹到 `(0, y)` 硬贴，也不许与"剪贴板为空"混为一谈）；
    /// 3. 无源剪贴板（[`Clipboard::from_cells`] 造的）与"复制时地表不可见"⇒ 不重算，
    ///    `(x, y)` 就是左上角（= 2026-11-06 之前的旧行为）。
    ///
    /// 剪贴板为空 ⇒ [`PlaceOutcome::Refused`] + 中文原因，地图一个字节不动。
    pub fn paste_clipboard(&mut self, x: u16, y: u16) -> PlaceOutcome {
        // `place_stamp` 要 `&mut self`，而剪贴板是 `self` 的字段 ⇒ 先 take 出来、粘完放回。
        // 为什么不用 `clone()`：整图选区能有 49 万格（≈12 MB），每次粘贴复制一遍纯属浪费。
        let Some(clip) = self.sel.clip.take() else {
            let reason = "剪贴板是空的：先框一个选区再复制（C# 里是 Ctrl+C，RegionCopy.cs:101-110），\
                          地图一个字节都没动"
                .to_string();
            self.note = Some(reason.clone());
            return PlaceOutcome::Refused { reason };
        };
        // ── ① 锚点（`RegionCopy.cs:70`，先 Anchor 再 CanPaste：`:121-122`）──
        let Some((ax, ay)) = clip.anchor(x, y) else {
            // 负锚点：上游 `CanPaste` 的 `document.Contains(x, y)` 必然 false ⇒ `Paste` 返回
            // false、`PlaceRegion` 只弹一句 "Region extends outside the map"，**一格不写**。
            // 这里等价地整块拒绝，并且**先把剪贴板放回**（拒绝不许吃掉剪贴板）。
            let 源 = clip.source().unwrap_or((x, y));
            self.sel.clip = Some(clip);
            let reason = format!(
                "按源奇偶重算后的粘贴锚点为负（点击 ({x},{y})，剪贴板源左上角 ({},{})，\
                 复制时地表可见 ⇒ 左上角要往左上对齐一格），整块拒绝（对应 C# `CanPaste`，\
                 RegionCopy.cs:70-71）—— 地图一个字节都没动",
                源.0, 源.1
            );
            self.note = Some(reason.clone());
            return PlaceOutcome::Refused { reason };
        };
        // 口径 = C# `RegionCopy.Paste`（`RegionCopy.cs:72-91`）：前置只有 `CanPaste`（纯边界），
        // **没有**占用闸门、也没有坐标重复概念 ⇒ 走 [`PlanMode::Paste`]。
        // ⚠ 这条差异必须留着：印章的"落点已有物件 ⇒ 整枚拒绝"（`PlacedStamps.cs:63`）
        //    在粘贴路径上会把「往有物件的区域贴一块地表」也一起毙掉，而 C# 是允许的。
        let out = self.place_stamp_with_mode(ax, ay, clip.cells(), PlanMode::Paste);
        // 放回：剪贴板要能反复粘贴（C# 粘完提示"Region pasted · Ctrl+V to paste again"，`:123`）
        self.sel.clip = Some(clip);
        // 拒绝原因的文案要说得清是"粘贴"被拒（`plan_stamp` 的原文是给印章写的，带"印章"字样）
        match out {
            PlaceOutcome::Placed { cells, skipped } => PlaceOutcome::Placed { cells, skipped },
            PlaceOutcome::Refused { reason } => {
                let reason = format!("粘贴被拒绝：{reason}");
                self.note = Some(reason.clone());
                PlaceOutcome::Refused { reason }
            }
        }
    }

    /// **删除选区内容**（三族按层清，**整次 = 一步撤销**）；返回真的改了几格。
    ///
    /// 来源：`RegionCopy.cs:45-69` 的 `CutSource` —— 逐族判据、保留 `bit15`、库号字节写 0，
    /// 逐条落点见 `selection.rs` 模块文档 §二（含与 `MapViewport.cs:546` `ClearSelection`
    /// 的**有意差异**：那边不清库号字节，本仓统一按 `CutSource` 清）。
    ///
    /// 没有选区 / 选区内三族图号全 0 ⇒ 返回 0 并给中文原因，**地图一个字节不动**。
    /// 回执只出**一句**（成功回执或"没有可清的内容"，二者互斥，不拼接）。
    ///
    /// ⚠ **不按可见性**（传 `[true; 3]`）：它的 C# 对照是 `MapViewport.cs:546` 的 `ClearSelection`
    /// （那边既不判可见性、也不清库号字节；本仓只按 `CutSource` 清库号，见模块文档 §二）。
    /// 按可见性清的只有 [`Self::cut_selection`]（`RegionCopy.cs:108` 的 `CutSource` 支）。
    pub fn delete_selection(&mut self) -> usize {
        let 选区 = self.sel.sel;
        match self.clear_selection_now([true; 3]) {
            Ok((n, 告警)) => {
                self.last_clear = Some(n);
                let mut s = match 选区 {
                    Some(sel) if n == 0 => format!(
                        "选区 ({},{})..({},{}) 里没有可清的内容（三族图号都是 0），未改动",
                        sel.x0, sel.y0, sel.x1, sel.y1
                    ),
                    _ => format!("删除选区内容 {n} 格（三族按层清）"),
                };
                for w in 告警 {
                    s.push_str("；另：");
                    s.push_str(&w);
                }
                self.note = Some(s);
                n
            }
            Err(reason) => {
                self.last_clear = Some(0);
                self.note = Some(reason);
                0
            }
        }
    }

    /// **清空选区的唯一实现**：算计划 → 写视图 → 压成一步撤销。**不拼任何文案**（回执由调用方组装，
    /// 见 [`Self::delete_selection`] / [`Self::cut_selection`] 的"只出一句"约定）。
    ///
    /// `vis` = 逐族清空的闸门（`[地表, 中层, 物体]`）：`[true; 3]` = 三族全清（删除），
    /// 传当前可见性 = 只清可见层（剪切，`RegionCopy.cs:52/57/61`）。
    ///
    /// 返回 `(真清掉几格, 告警)`；`Err(中文原因)` = 前置不满足（没有选区），**一格未写**。
    fn clear_selection_now(&mut self, vis: [bool; 3]) -> Result<(usize, Vec<String>), String> {
        let Some(sel) = self.sel.sel else {
            return Err(
                "没有选区：先框一个矩形选区（C# 里是 M 工具拖拽，MapViewport.cs:412-415），未改动"
                    .to_string(),
            );
        };
        // **先算后写**：拒绝/空操作必须发生在动笔之前（同 stamp/fill 的口径）
        let plan = clear_plan(self.doc.view(), self.doc.format_id(), sel, vis);
        if plan.changes.is_empty() {
            return Ok((0, plan.notes));
        }
        for c in &plan.changes {
            // 写视图 + 增量更新计数（与 `touch()` 同一入口）
            self.doc.write_view_cell(c.x, c.y, c.after);
        }
        let n = self.undo.commit(plan.changes);
        self.sync_dirty();
        Ok((n, plan.notes))
    }

    /// 用**当前笔刷**填满选区（**整次 = 一步撤销**）；返回真的改了几格。
    ///
    /// 来源：`MapViewport.cs:540` 的 `FillSelection`（逐格调 `Apply`）。
    ///
    /// # 地表步长：本 crate 取 **step=1（严格填满选区）**
    ///
    /// C# 的 `Apply` 对地表带 `step = 2` 且 `x &= ~1; y &= ~1`（`:474-475`）⇒
    /// 3×3 选区在奇数原点会**溢出到选区外**（覆盖 4×4）。本 crate 不学这条：
    /// ① 溢出违背用户预期；② 与本 crate 既有笔刷口径一致（`session.rs:22-26` 已记档）。
    /// 代价是地表奇格写进去不显示（渲染只铺偶偶格，`miredit-render/src/compose.rs:107-116`）——
    /// 属无害。完整论证见 `selection.rs` 模块文档 §五。
    ///
    /// # 闸门顺序（与 [`Self::fill`] 同口径）
    ///
    /// ① 无选区 → ② 地表格式闸门（Type5）→ ③ 笔刷闸门（阻挡层不用笔刷）→
    /// ④ 逐格算 `after`（族不匹配 ⇒ **一格不写**，因为还没动过地图）→ ⑤ 写盘 + 压成一步。
    pub fn fill_selection(&mut self) -> usize {
        let Some(sel) = self.sel.sel else {
            self.note = Some(
                "没有选区：先框一个矩形选区（C# 里是 M 工具拖拽，MapViewport.cs:412-415），未改动"
                    .to_string(),
            );
            return 0;
        };
        // ① 地表闸门（Type5）：与 `touch()` / `fill()` 同一条
        if let Some(why) = self.ground_refusal() {
            self.note = Some(why);
            return 0;
        }
        // ② 笔刷闸门：图号层必须有笔刷；阻挡层不用笔刷（官方 :494 `!collision && SpriteIndex < 0`）
        let brush = if self.layer.is_collision() {
            None
        } else {
            match self.brush {
                Some(b) => Some(b),
                None => {
                    self.note =
                        Some("没有笔刷：先用吸管取一格，或在资源面板选一帧".to_string());
                    return 0;
                }
            }
        };
        let fmt = self.doc.view().format;
        let id = self.doc.format_id();
        let mut changes: Vec<Change> = Vec::new();
        let mut after_cells: Vec<(u16, u16, mir2_map::Cell)> = Vec::new();
        let mut dropped_lib = false;
        for (x, y) in sel.iter() {
            let Some(before) = cell_at(self.doc.view(), x, y).copied() else {
                continue;
            };
            let mut after = match paint(fmt, before, Tool::Brush, brush, self.layer) {
                PaintOutcome::Write(c) => c,
                // 未接线工具 / 没笔刷（阻挡层以外已拦）：不改这一格
                PaintOutcome::NoOp => continue,
                // 族不匹配 / 图库笔刷落阻挡层：**一格都不许写**（此刻地图还没动过）
                PaintOutcome::LayerMismatch { brush: b, layer } => {
                    self.note = Some(format!(
                        "笔刷族（{}）与当前图层（{}）不一致，选区填充被拒绝，一格未写",
                        b.label(),
                        layer.name()
                    ));
                    return 0;
                }
                PaintOutcome::PaletteBrushOnCollision { brush: b, layer } => {
                    self.note = Some(format!(
                        "笔刷来自图库（{}），{}只改 0x8000 位、不写图号，选区填充被拒绝，一格未写",
                        b.label(),
                        layer.name()
                    ));
                    return 0;
                }
            };
            // ③ 库号字节闸门（同 `touch()` / `fill()`）：写不进去的还原成改前值
            if crate::convert::restore_unwritten_lib_byte(id, self.layer, &before, &mut after) {
                dropped_lib = true;
            }
            if after == before {
                continue; // 这一格本来就是笔刷值
            }
            changes.push(Change {
                x,
                y,
                before,
                after,
            });
            after_cells.push((x, y, after));
        }
        if changes.is_empty() {
            self.note = if dropped_lib {
                Some(self.lib_byte_drop_note())
            } else {
                None
            };
            return 0;
        }
        let n = changes.len();
        for (cx, cy, after) in after_cells {
            // 写视图 + 增量更新计数（与 `touch()` 同一入口）
            self.doc.write_view_cell(cx, cy, after);
        }
        // 复用拖动那套记账：一次选区填充 = 撤销栈里的**一步**
        self.undo.commit(changes);
        self.note = if dropped_lib {
            Some(self.lib_byte_drop_note())
        } else {
            None
        };
        self.last_fill = Some(FillRecord {
            cells: n,
            layer: self.layer,
        });
        self.sync_dirty();
        n
    }

    // ── 吸管 ──
    //
    // 三条入口的分工（**别合并**）：
    // - `pick_cell`：读整格三族的原始信息，**不吸附**（没有"再画回去"的语义）；
    // - `pick_at` / `pick_at_from`：取成笔刷，地表按 2×2 对齐到偶偶格（`pick_anchor`）。
    //
    // ⚠ **层门控比上游宽，是既有的能力差异，不在本单收紧**：上游 `MapViewport.cs:404`
    // 只允许 `Ground` / `Middle`（其余层 `PickTile` 直接 `return false`）；
    // 本 crate 允许地表 / 中层 / 物件 / 低位阻挡 / 高位阻挡五层吸取
    // （`brush_from_cell` 用 `layer.pick_family()` / `layer.img_of()`，五层都取得到内容）。
    // 保留更宽能力的理由：阻挡层"吸同一格内容"已是既有用例
    // （`吸管在阻挡层取同一格内容`、`阻挡层接受地图吸来的笔刷`），收紧会砍功能。

    /// 吸管读**整格三族**（三个图号字段原样）；越界 = `None`。来源：`edit.rs:1218-1227`。
    ///
    /// 只读不写：它不改地图、不进撤销栈。判据在 [`crate::pick_cell_of`]（单一实现），
    /// 本方法只是"用自己文档的地图"去调它。
    ///
    /// ⚠ **不吸附**（与两条取笔刷入口有意不同）：本方法返回的是"这一格上到底有什么"，
    /// 是给人看的整格信息，没有"取成笔刷再画回去"的语义 ⇒ 地表也不需要按 2×2 对齐。
    /// 需要吸附的是 [`Self::pick_at`] / [`Self::pick_at_from`]（见 [`Self::pick_anchor`]）。
    pub fn pick_cell(&self, x: u16, y: u16) -> Option<PickedCell> {
        crate::pick_cell_of(self.doc.view(), x, y)
    }

    /// 吸管：把 `(x, y)` **当前编辑层**的内容取成笔刷；越界 = `None`（笔刷不变）。
    /// 来源：`edit.rs:1234-1252`。
    ///
    /// 「按当前层取」的含义：当前层是中间层，取的就是 `mid_img` + `btsmTiles`，
    /// 笔刷族 = [`crate::Family::Middle`]；吸完立刻能往中间层画（族自然匹配）。
    /// 整格的另外两族**不会被塞进笔刷** —— 那正是"跨族乱写"的来源。
    ///
    /// 地表层先按 2×2 对齐到**偶偶格**再读（[`Self::pick_anchor`]）；笔刷的
    /// [`Brush::from`] 记的是**锚点**而不是点击格 —— 吸到的内容与"从哪格吸的"必须指同一格，
    /// 否则落笔时会拿奇格坐标当参照。
    pub fn pick_at(&mut self, x: u16, y: u16) -> Option<Brush> {
        let (ax, ay) = self.pick_anchor(x, y);
        let c = *cell_at(self.doc.view(), ax, ay)?;
        let fmt = self.doc.view().format;
        Some(self.brush_from_cell(c, ax, ay, fmt))
    }

    /// 同上，但**从外部借来的地图**取内容（宿主把地图放在自己手里时用）。
    ///
    /// 为什么要它：GUI 的 `EditSession::pick_at(&mut self, map: &Map, …)` 收的是
    /// **不可变** `&Map`，没法把地图换进本会话（`mem::swap` 要 `&mut`）——
    /// 没有这条通路就只能把「按当前层取笔刷」的判据再抄一份到宿主里。
    ///
    /// 吸附语义与 [`Self::pick_at`] **逐字节相同**（同一个 [`Self::pick_anchor`]）。
    pub fn pick_at_from(&mut self, map: &Map, x: u16, y: u16) -> Option<Brush> {
        let (ax, ay) = self.pick_anchor(x, y);
        let c = *cell_at(map, ax, ay)?;
        let fmt = map.format;
        Some(self.brush_from_cell(c, ax, ay, fmt))
    }

    /// 吸管读格的**锚点**：当前层是地表 ⇒ 对齐到 2×2 图块的偶偶格 `(x & !1, y & !1)`；
    /// 其余层（中层 / 物件 / 低位阻挡 / 高位阻挡）**原样返回**。
    ///
    /// # 依据（逐字对应上游）
    ///
    /// `CrystalStudio/MapViewport.cs:402-414` 的 `PickTile`：
    /// - `:404` 先过 `Document.Contains` 与 `Layer is not (Ground or Middle)`；
    /// - **`:405` `if (Layer == MapLayer.Ground) { x &= ~1; y &= ~1; }`** ← 本条实现的判据；
    /// - `:406` `var cell = Document.Get(x, y);`（对齐**之后**才读格）；
    /// - `:408` 地表取 `(cell.BackImage & 0x1FFFFFFF) - 1`。
    ///
    /// 为什么必须吸附：地表格在编辑器里按 2×2 图块拼贴，一张可见图块只落在偶偶格上，
    /// 奇格通常 `bk_img == 0`（或只有 bit15）。不吸附就会把 `img = 0` 吸成笔刷，
    /// 随后落笔（笔刷尺寸 ≥ 3 的 N×N 覆盖到相邻偶格）把**看得见的那格地表图号写成 0**
    /// —— 用户视角就是"吸一下地表就画不出来了 / 地表被擦掉了"。
    ///
    /// # 与填充那一侧是同一条判据
    ///
    /// [`crate::fill::align_start`]（`fill.rs:129-135`，来源 `MapViewport.cs:496-499`）按
    /// `step_of(layer) == 2` 判定、用的也是 `x & !1`。两处必须给出**同一张网格**，
    /// 否则会出现"吸管吸偶格、填充从奇格起"的两套网格（`session_tests.rs` 里有对拍用例）。
    fn pick_anchor(&self, x: u16, y: u16) -> (u16, u16) {
        if self.layer == EditLayer::Ground {
            (x & !1, y & !1)
        } else {
            (x, y)
        }
    }

    /// 「一格内容 + 当前层 → 笔刷」的**唯一实现**（`pick_at` / `pick_at_from` 共用）。
    ///
    /// `(x, y)` 是**锚点**（地表已对齐偶偶格，见 [`Self::pick_anchor`]），原样记进
    /// [`Brush::from`]，不做任何再换算。
    fn brush_from_cell(&mut self, c: Cell, x: u16, y: u16, fmt: CellFormat) -> Brush {
        let layer = self.layer;
        let b = Brush {
            family: layer.pick_family(),
            img: layer.img_of(&c),
            lib_byte: layer.lib_byte_of(&c),
            // 只有 14/36 档的地图格里有库号字节；`Old12` 档如实记「不知道库号」
            lib: if fmt.has_lib_bytes() {
                Some(lib_ref_of(layer, u16::from(layer.lib_byte_of(&c))))
            } else {
                None
            },
            from: Some((x, y)),
        };
        self.brush = Some(b);
        self.refresh_layer_note();
        b
    }

    /// 素材面板点选一帧 → 设成笔刷（**三族都允许**，笔刷带族）。来源：`edit.rs:1273-1286`。
    ///
    /// 不受限的是"从哪个库选"，**受限的是落笔**：写哪个字段由当前编辑层决定，
    /// 族与层不一致时 [`Self::touch`] 会拒绝并把原因写进 [`Self::note`]。
    ///
    /// `img` 必须是**0 基帧索引**且换算后的图号 ≤ [`crate::SPRITE_MAX`]，否则返回 `None`
    /// 且**不动**现有笔刷（图号 0x8000 会撞上最高位，那是错数据，宁可拒绝）。
    pub fn set_brush_from_palette(&mut self, lib: LibRef, img: u32) -> Option<Brush> {
        let sprite = palette_index_to_sprite(img)?;
        let lib_byte = u8::try_from(lib.index).ok()?;
        let b = Brush {
            family: lib.family,
            img: sprite,
            lib_byte,
            lib: Some(lib),
            from: None,
        };
        self.brush = Some(b);
        self.refresh_layer_note();
        Some(b)
    }

    // ── 撤销 / 重做 ──

    /// 撤销一步；返回还原了几格（0 = 没有可撤销的步骤）。来源：`edit.rs:1289-1305`。
    ///
    /// 撤销会**同时**把五层计数改回去：拆出「视图 + 计数更新器」两笔不重叠的借用，
    /// 由 [`UndoStack::undo_with`] 每写回一格回调一次（`document.rs` 模块文档第六节）。
    pub fn undo(&mut self) -> usize {
        let (view, mut keeper) = self.doc.view_and_keeper();
        let n = self
            .undo
            .undo_with(view, |x, y, before, after| keeper.record(x, y, &before, &after));
        // 撤销之后，"刚才填了 N 格"这句回执就不成立了（那批格已经退回去了）
        self.last_fill = None;
        self.sync_dirty();
        n
    }

    /// 重做一步；返回重放了几格（0 = 没有可重做的步骤）。来源：`edit.rs:1308-1323`。
    ///
    /// 与 [`Self::undo`] 对称：每写回一格都把计数增量改回去。
    pub fn redo(&mut self) -> usize {
        let (view, mut keeper) = self.doc.view_and_keeper();
        let n = self
            .undo
            .redo_with(view, |x, y, before, after| keeper.record(x, y, &before, &after));
        // 重做回到的是"某一笔"，不一定是那次填充 ⇒ 回执同样作废（宁可不说，不说错）
        self.last_fill = None;
        self.sync_dirty();
        n
    }

    // ── 给 cmd 用的说明 ──

    /// 落笔 **0 格**时给出**一句明确原因**（任务书 §七：拒绝必须返回明确 outcome，不许静默不改）。
    ///
    /// 判据顺序与 [`Self::touch`] 的早退顺序一致：
    /// ① 已有拒绝提示（族不匹配 / 图库笔刷落阻挡层）⇒ 原样给出；
    /// ② 目标格在地图外；③ 图号层没有笔刷；④ 工具未接线；⑤ 其余 = 值本来就是要写的值。
    pub fn paint_note(&self, x: u16, y: u16) -> String {
        if let Some(n) = &self.note {
            return n.clone();
        }
        if cell_at(self.doc.view(), x, y).is_none() {
            return format!("点击格 ({x},{y}) 在地图外，未改动");
        }
        if !self.layer.is_collision() && self.brush.is_none() {
            return "没有笔刷：先用吸管取一格，或在资源面板选一帧".to_string();
        }
        if !self.tool.is_wired() {
            return format!("工具 {} 未接线，不改地图", self.tool.key());
        }
        "目标格与笔刷内容相同，未改动".to_string()
    }

    /// 把当前状态装成一个 [`crate::Outcome`]（`cmd::apply` 的统一出口）。
    pub fn outcome(&self, changed: usize, note: Option<String>) -> crate::Outcome {
        crate::Outcome {
            changed,
            note,
            dirty: self.dirty(),
            can_undo: self.undo_len() > 0,
            can_redo: self.redo_len() > 0,
        }
    }
}

/// 「笔刷 vs 当前层」的拒绝原因（`None` = 这一笔允许落）。来源：`edit.rs:1612-1636`。
///
/// 一处实现、三处共用（切层 / 吸管后 / 落笔时），避免三份判断各说各话：
/// - 图号层：族不一致 ⇒ 族/层不一致；
/// - 阻挡层 + 图库笔刷 ⇒ 只改 0x8000 位、不写图号；
/// - 其余（没笔刷 / 地图吸来的笔刷 / 图号层且族匹配）⇒ `None`。
fn layer_note(brush: Option<Brush>, layer: EditLayer) -> Option<String> {
    let b = brush?;
    if layer.is_collision() {
        // 只有"来自图库"的笔刷才拒绝：地图吸来的笔刷不携带"盖图号"的意图
        if b.from.is_none() {
            return Some(format!(
                "笔刷来自图库（{}），{}只改 0x8000 位、不写图号",
                b.family_name(),
                layer.name()
            ));
        }
        return None;
    }
    match layer.family() {
        Some(fam) if b.family != fam => Some(format!(
            "笔刷族（{}）与当前图层（{}）不一致",
            b.family_name(),
            layer.name()
        )),
        _ => None,
    }
}

/// 两格之间要经过的格（Bresenham，含首尾）—— 快速拖动时指针会跳格，
/// 不插值的话画出来的线是断的。来源：`edit.rs:1638-1665`。
pub fn line_cells(from: (u16, u16), to: (u16, u16)) -> Vec<(u16, u16)> {
    let (x0, y0) = (i64::from(from.0), i64::from(from.1));
    let (x1, y1) = (i64::from(to.0), i64::from(to.1));
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let (mut x, mut y, mut err) = (x0, y0, dx + dy);
    let mut out = Vec::new();
    loop {
        out.push((x as u16, y as u16));
        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
    out
}

/// 会话的单元测试（拆到单独文件：`session.rs` 本体只放实现，见模块文档第一节末尾）。
#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
