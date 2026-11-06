//! 素材面板的真实图块树：**库枚举 / 按需解码缩略图 / 虚拟滚动 / 图号搜索 / 点选设笔刷**。
//!
//! # 一、为什么单独一层
//! 面板上的东西全都能在无窗口测试里核对：库列表来自 [`mir2_assets::LibPaths`]（真实文件），
//! 帧数来自 [`mir2_assets::WilSource::frame_count`]（真实索引表长度），
//! 缩略图来自 [`mir2_assets::WilSource::frame_of`]（真解码）。所以"库里到底有几帧、
//! 现在解了几帧、跳到了第几行"这些数必须能在 `cargo test --lib` 里断言，
//! 不能只靠肉眼看截图 —— 于是状态、几何、缓存都放在本模块，绘制只做排版。
//!
//! # 二、虚拟滚动（**绝不解码全库**）
//! `Tiles` 有 31775 帧、`Objects21` 有 16372 帧，一次性解码既慢又吃内存
//! （一帧 96×64 RGBA = 24 KB，31775 帧 ≈ 763 MB）。因此：
//!
//! 1. 只遍历**可见行 + 1 行预取**（每行 [`m::PALETTE_GRID_COLS`] = 8 格）；
//! 2. 解出来的帧按 [`FrameKey`] 进 [`Cache`]（上限 [`THUMB_CACHE_CAP`]），
//!    滚动回来时直接命中，**不重复解码**；
//! 3. 打不开/空帧（`.wzx` 偏移为 0）**做负缓存**：同一格不会每帧都去敲一次解码器，
//!    也不会画洋红假图 —— 格子留底色，计数进 [`PaletteState::empty_count`]。
//!
//! # 三、几何全部来自 [`crate::metrics`]
//! 网格矩形、单格边长、可见行数都是 [`crate::metrics`] 里带推导注释的常量算出来的，
//! 本模块**没有一个"看起来差不多"的字面量**。
//!
//! # 四、本单**不做**的事（如实说明，不假装）
//! 收藏 / 最近使用 / 分类三个页签点了只显示"未实现"。
//!
//! # 五、三族都能设为笔刷（2026-11-04 改）
//! 上一版只允许地面库（`Tiles`）设笔刷，理由当时是"画笔只写地表层"。
//! 现在画笔写哪一层由**当前编辑层**决定（见 [`crate::edit::EditLayer`]），
//! 所以中层（`SmTiles`）/ 物件（`Objects`）选出来的帧同样可以设成笔刷 ——
//! 笔刷带自己的族（[`crate::edit::Brush::family`]），与当前层不一致时**落笔那一刻**被拒，
//! 而不是在面板上假装"这个库不能选"。图号越界（> 32767）仍然在面板这里就拒掉。

use std::collections::{HashMap, VecDeque};

use egui::{
    pos2, vec2, Color32, ColorImage, Context, Painter, Rect, Sense, TextureHandle, TextureId,
    TextureOptions, Ui,
};

use mir2_assets::{Family, LibPaths, LibRef, WilSource};

use crate::layout::Layout;
use crate::metrics as m;
use crate::paint as pt;
use crate::theme as t;
use crate::ui_text as tx;

/// 缩略图纹理缓存上限 **512** 张。
///
/// 取值依据：最坏 96×64 RGBA = 24 KB/张 ⇒ 512 张 ≈ 12 MB 显存 + 12 MB 纹理对象开销；
/// 而一屏最多 `6 行 × 8 列 = 48` 格，512 足够覆盖 10 屏的滚动历史，
/// 再往回滚也基本不会重解码（真滚回去时由 [`Cache`] 的 LRU 决定丢谁）。
pub const THUMB_CACHE_CAP: usize = 512;

/// 帧定位键：**库 + 帧号**（三族的索引空间互不相通，键里必须带族）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FrameKey {
    /// 图库（族 + 族内索引）。
    pub lib: LibRef,
    /// 0-based 帧号。
    pub index: u32,
}

/// 一个已登记的图库（文件基名 + 定位键）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibEntry {
    /// 定位键。
    pub lib: LibRef,
    /// 文件基名（如 `Tiles5`、`Objects21`），**来自真实文件名**。
    pub stem: String,
}

impl LibEntry {
    /// 所属层的中文名（文案取自 [`crate::ui_text`]，不在这里写汉字）。
    pub fn family_label(&self) -> &'static str {
        family_label(self.lib.family)
    }
}

/// 族 → 中文层名（复用左侧图层面板的同一批文案，避免第二套叫法）。
pub fn family_label(family: Family) -> &'static str {
    match family {
        Family::Ground => tx::LAYER_GROUND,
        Family::Middle => tx::LAYER_MIDDLE,
        Family::Surface => tx::LAYER_OBJECTS,
    }
}

/// 一次网格交互的结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteAction {
    /// 什么都没发生。
    None,
    /// 用户点选了一帧，应把它设为笔刷（调用方负责切工具与落格）。
    SetBrush(FrameKey),
}

/// 一张已上传的缩略图（纹理 + 原始帧尺寸，用于等比缩进格子）。
struct Thumb {
    tex: TextureHandle,
    w: u32,
    h: u32,
}

/// 缩略图缓存：`FrameKey → Option<Thumb>`，`None` = 已知取不到（空帧/解码失败）的负缓存。
struct Cache {
    map: HashMap<FrameKey, Option<Thumb>>,
    /// LRU 顺序：队首最久未用。
    order: VecDeque<FrameKey>,
}

impl Cache {
    fn new() -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    /// 命中则刷新 LRU 并返回内部值（`Some(None)` = 负缓存命中）。
    fn get(&mut self, k: FrameKey) -> Option<Option<&Thumb>> {
        if !self.map.contains_key(&k) {
            return None;
        }
        if let Some(pos) = self.order.iter().position(|x| *x == k) {
            self.order.remove(pos);
        }
        self.order.push_back(k);
        Some(self.map.get(&k).and_then(|v| v.as_ref()))
    }

    /// 放入缓存；超过上限时按 LRU 淘汰最久未用的一张。
    fn insert(&mut self, k: FrameKey, v: Option<Thumb>) {
        let existed = self.map.insert(k, v).is_some();
        if existed {
            if let Some(pos) = self.order.iter().position(|x| *x == k) {
                self.order.remove(pos);
            }
        }
        self.order.push_back(k);
        while self.map.len() > THUMB_CACHE_CAP {
            match self.order.pop_front() {
                Some(old) => {
                    self.map.remove(&old);
                }
                None => break,
            }
        }
    }

    /// 已缓存的条目数。
    fn len(&self) -> usize {
        self.map.len()
    }

    /// 其中"已知取不到"的条目数（空帧 + 解码失败）。
    fn empty_len(&self) -> usize {
        self.map.values().filter(|v| v.is_none()).count()
    }
}

/// 素材面板的全部状态（**不含绘制**）。
pub struct PaletteState {
    /// 已登记的图库（族按 `Family::ALL`、族内按索引升序，与 `LibPaths::iter` 一致）。
    libs: Vec<LibEntry>,
    /// 当前库在 `libs` 里的下标。
    current: usize,
    /// `frame_count` 的缓存：库打不开 / 不存在就是 `None`（**不猜 0**）。
    counts: HashMap<LibRef, Option<u32>>,
    /// 顶部可见行号（虚拟滚动的唯一滚动量，单位=行）。
    scroll: usize,
    /// 当前高亮的帧（金色描边）；点选与搜索都会设置它。
    selected: Option<FrameKey>,
    /// 搜索框里的文字。
    search: String,
    /// 搜索框是否有焦点（有焦点才吃键盘）。
    focus: bool,
    /// 面板底部的一行提示（搜索失败/拒绝设笔刷等，文案全部来自 [`crate::ui_text`]）。
    note: Option<String>,
    /// 当前页签（0 = 图库）。
    tab: usize,
    /// 库下拉是否展开。
    combo_open: bool,
    /// 库下拉的滚动行（弹层最多显示 [`m::PALETTE_COMBO_ROWS`] 行）。
    combo_scroll: usize,
    /// 缩略图缓存。
    cache: Cache,
    /// 真解码次数（**只在这里 +1**，滚动性能报告要用）。
    decodes: u64,
    /// 缓存命中次数。
    hits: u64,
    /// 最近一次绘制覆盖的槽位数（可见格 + 1 行预取）。
    slots: usize,
    /// 最近一次绘制的可见行数。
    rows_drawn: usize,
}

impl PaletteState {
    /// 从真实登记表建状态（`None` = 图库装载失败 ⇒ 面板走空态）。
    ///
    /// 缺省选中 **地面 0 号库（`Tiles`）**：它是地表层的主库，也是本单画笔唯一能写的族。
    pub fn new(paths: Option<&LibPaths>) -> Self {
        let libs: Vec<LibEntry> = paths
            .map(|p| {
                p.iter()
                    .map(|i| LibEntry {
                        lib: i.lib,
                        stem: i.stem().unwrap_or(tx::VAL_UNKNOWN).to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let current = libs
            .iter()
            .position(|e| e.lib.family == Family::Ground && e.lib.index == 0)
            .unwrap_or(0);
        Self {
            libs,
            current,
            counts: HashMap::new(),
            scroll: 0,
            selected: None,
            search: String::new(),
            focus: false,
            note: None,
            tab: 0,
            combo_open: false,
            combo_scroll: 0,
            cache: Cache::new(),
            decodes: 0,
            hits: 0,
            slots: 0,
            rows_drawn: 0,
        }
    }

    // ── 只读 ──

    /// 全部已登记图库。
    pub fn libs(&self) -> &[LibEntry] {
        &self.libs
    }

    /// 当前库下标。
    pub fn current_index(&self) -> usize {
        self.current
    }

    /// 当前库（没登记任何库时 `None`）。
    pub fn current(&self) -> Option<&LibEntry> {
        self.libs.get(self.current)
    }

    /// 当前库的帧总数（没查过 / 打不开 = `None`，**不是 0**）。
    pub fn current_count(&self) -> Option<u32> {
        self.current()
            .and_then(|e| self.counts.get(&e.lib).copied().flatten())
    }

    /// 顶部可见行号。
    pub fn scroll(&self) -> usize {
        self.scroll
    }

    /// 当前高亮的帧。
    pub fn selected(&self) -> Option<FrameKey> {
        self.selected
    }

    /// 搜索框文字。
    pub fn search_text(&self) -> &str {
        &self.search
    }

    /// 搜索框是否有焦点。
    pub fn focused(&self) -> bool {
        self.focus
    }

    /// 底部提示（没有就 `None`）。
    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    /// 当前页签下标。
    pub fn tab(&self) -> usize {
        self.tab
    }

    /// 库下拉是否展开。
    pub fn combo_open(&self) -> bool {
        self.combo_open
    }

    /// 展开/收起库下拉（`--palette-combo` 与点击都走这里）。
    pub fn set_combo_open(&mut self, open: bool) {
        self.combo_open = open;
        if open {
            self.combo_scroll = self.current.saturating_sub(1);
        }
    }

    /// 库下拉的滚动行。
    pub fn combo_scroll(&self) -> usize {
        self.combo_scroll
    }

    /// 真解码次数。
    pub fn decodes(&self) -> u64 {
        self.decodes
    }

    /// 缓存命中次数。
    pub fn hits(&self) -> u64 {
        self.hits
    }

    /// 已知取不到的帧数（空帧 + 解码失败）。
    pub fn empty_count(&self) -> usize {
        self.cache.empty_len()
    }

    /// 缓存条目总数。
    pub fn cached(&self) -> usize {
        self.cache.len()
    }

    /// 最近一次绘制覆盖的槽位数。
    pub fn slots(&self) -> usize {
        self.slots
    }

    /// 最近一次绘制的可见行数。
    pub fn rows_drawn(&self) -> usize {
        self.rows_drawn
    }

    /// 可见行数（按当前布局算，与滚动位置无关）。
    pub fn rows_visible(&self, l: &Layout) -> usize {
        rows_visible(l)
    }

    /// 总行数（当前库；帧数未知时 0）。
    pub fn total_rows(&self) -> usize {
        match self.current_count() {
            Some(n) if n > 0 => (n as usize).div_ceil(m::PALETTE_GRID_COLS),
            _ => 0,
        }
    }

    // ── 状态变更 ──

    /// 切库：重置滚动、清掉高亮（高亮是"某个库里的某一帧"，跨库无效）。
    pub fn set_current(&mut self, i: usize) {
        if i >= self.libs.len() || i == self.current {
            return;
        }
        self.current = i;
        self.scroll = 0;
        self.selected = None;
        self.note = None;
        self.combo_scroll = i.saturating_sub(1);
    }

    /// 按文件基名（大小写不敏感）切库；返回是否找到。
    pub fn select_lib_by_stem(&mut self, stem: &str) -> bool {
        match self
            .libs
            .iter()
            .position(|e| e.stem.eq_ignore_ascii_case(stem))
        {
            Some(i) => {
                self.set_current(i);
                self.current = i;
                true
            }
            None => false,
        }
    }

    /// 直接把滚动放到某一行（越界由绘制侧夹住）。
    pub fn set_scroll(&mut self, row: usize) {
        self.scroll = row;
        self.selected = None;
    }

    /// 滚动若干行（负 = 向上）。
    pub fn scroll_by(&mut self, rows: i64) {
        let total = self.total_rows();
        let max = total.saturating_sub(1);
        let next = (self.scroll as i64 + rows).clamp(0, max as i64);
        self.scroll = next as usize;
    }

    /// 切页签；非「图库」页**不伪造内容**（说明由网格区中央那句静态文案承担）。
    pub fn set_tab(&mut self, i: usize) {
        self.tab = i.min(crate::ui_text::PALETTE_TABS.len().saturating_sub(1));
        self.note = None;
    }

    /// 记一条提示（文案必须是 [`crate::ui_text`] 里的常量或模板填出来的）。
    pub fn set_note(&mut self, s: Option<String>) {
        self.note = s;
    }

    /// 刷新当前库的帧数缓存（库打开过一次之后就是纯哈希查表）。
    ///
    /// 没有图源时**什么都不做**：不能把"暂时不知道"缓存成 `None`，
    /// 否则图源就位后再也查不出真数字了。
    pub fn refresh_count(&mut self, src: Option<&WilSource>) {
        let Some(e) = self.libs.get(self.current) else {
            return;
        };
        if self.counts.contains_key(&e.lib) {
            return;
        }
        let Some(src) = src else {
            return;
        };
        self.counts.insert(e.lib, src.frame_count(e.lib));
    }

    /// 搜索框里追加文字（**不做字符过滤**：中文也要能打进来，好在回车时如实提示"输入图号"）。
    pub fn push_text(&mut self, s: &str) {
        for c in s.chars() {
            if !c.is_control() {
                self.search.push(c);
            }
        }
    }

    /// 退格。
    pub fn backspace(&mut self) {
        self.search.pop();
    }

    /// 清空搜索框。
    pub fn clear_search(&mut self) {
        self.search.clear();
        self.note = None;
    }

    /// 回车：**只认纯数字图号**，其余一律提示"输入图号"，不猜语义。
    ///
    /// ⚠ 输入的是**图号（1 基）**，不是 0 基帧索引（审计 E1 全仓统一口径）：
    /// 该库帧总数 `total` 就是最大图号，合法区间 `1..=total`；
    /// 命中后高亮的 `FrameKey::index = 图号 - 1`（面板内部仍按 0 基索引取帧）。
    ///
    /// 返回给 stderr 用的诊断文本（同时会把面板提示设置好）。
    pub fn submit_search(&mut self) -> String {
        let q = self.search.trim().to_string();
        let count = self.current_count();

        let msg = if q.is_empty() {
            None
        } else if !q.chars().all(|c| c.is_ascii_digit()) {
            Some(tx::PALETTE_SEARCH_NEED_DIGITS.to_owned())
        } else {
            match (q.parse::<u32>(), count) {
                (Ok(n), Some(total)) if n >= 1 && n <= total => {
                    let index = n - 1;
                    let row = index as usize / m::PALETTE_GRID_COLS;
                    // 命中行放到顶部往上 1 行，方便看到上下文
                    self.scroll = row.saturating_sub(1);
                    self.selected = self.current().map(|e| FrameKey {
                        lib: e.lib,
                        index,
                    });
                    Some(tx::fill(tx::PALETTE_SEARCH_JUMP_TMPL, &[("n", &n.to_string())]))
                }
                (Ok(_), Some(total)) => Some(tx::fill(
                    tx::PALETTE_SEARCH_OUT_OF_RANGE_TMPL,
                    &[("max", &total.to_string())],
                )),
                // 帧数未知（库打不开）——如实说范围未知，不编一个上限
                (Ok(_), None) => Some(tx::PALETTE_LIB_EMPTY.to_owned()),
                (Err(_), _) => Some(tx::PALETTE_SEARCH_NEED_DIGITS.to_owned()),
            }
        };
        let text = msg.clone().unwrap_or_else(|| tx::VAL_UNKNOWN.to_owned());
        self.note = msg;
        tx::fill(
            tx::DIAG_PALETTE_SEARCH,
            &[("q", &self.search), ("msg", &text)],
        )
    }

    // ── 几何（绘制与命中测试共用同一份） ──

    /// 网格区域（Palette 内容区里，页签/标签/下拉/搜索之下、提示行与分页行之上）。
    pub fn grid_rect(l: &Layout) -> Rect {
        let c = l.p_palette.content;
        let top = c.top() + m::PALETTE_GRID_OFF;
        let bottom = c.bottom()
            - m::PALETTE_BOTTOM_PAD
            - m::PAGER_ROW_H
            - m::PALETTE_GRID_GAP
            - m::PALETTE_NOTE_H;
        Rect::from_min_max(
            pos2(c.left() + m::RIGHT_PAD_X, top),
            pos2(c.right() - m::RIGHT_PAD_X, bottom),
        )
    }

    /// 提示行矩形（网格与分页行之间的一条 16px 窄带）。
    pub fn note_rect(l: &Layout) -> Rect {
        let c = l.p_palette.content;
        let bottom = c.bottom() - m::PALETTE_BOTTOM_PAD - m::PAGER_ROW_H - m::PALETTE_GRID_GAP;
        Rect::from_min_max(
            pos2(c.left() + m::RIGHT_PAD_X, bottom - m::PALETTE_NOTE_H),
            pos2(c.right() - m::RIGHT_PAD_X, bottom),
        )
    }

    /// **真正可见**的网格矩形 = 整行数 × 格边长（把底部那截"半行"切掉）。
    ///
    /// 为什么需要它：`grid_rect` 的高度是 261，而 6 行只占 252 —— 剩下 9 px。
    /// 如果按 `grid_rect` 裁剪，预取的那一行会露出 9 px 的一条，看起来像渲染错位；
    /// 按本矩形裁剪则**预取照旧解码**（滚动立刻出图），但**一个像素都不画**。
    pub fn visible_rect(l: &Layout) -> Rect {
        let g = Self::grid_rect(l);
        let rows = rows_visible(l);
        Rect::from_min_max(
            g.min,
            pos2(g.right(), g.top() + rows as f32 * m::PALETTE_CELL),
        )
    }

    /// 某一格的屏幕矩形（`row` 是**绝对行号**，按当前滚动位置换算）。
    pub fn cell_rect(l: &Layout, scroll: usize, row: usize, col: usize) -> Rect {
        let g = Self::grid_rect(l);
        let dy = (row as f32 - scroll as f32) * m::PALETTE_CELL;
        Rect::from_min_size(
            pos2(g.left() + col as f32 * m::PALETTE_CELL, g.top() + dy),
            vec2(m::PALETTE_CELL, m::PALETTE_CELL),
        )
    }

    /// 库下拉弹层矩形（向下展开，最多 [`m::PALETTE_COMBO_ROWS`] 行）。
    pub fn combo_popup_rect(&self, l: &Layout) -> Rect {
        let combo = crate::dock_right::combo_rect(l);
        let rows = self.popup_rows();
        Rect::from_min_size(
            pos2(combo.left(), combo.bottom() + 2.0),
            vec2(
                combo.width(),
                rows as f32 * m::PALETTE_COMBO_ROW_H + 2.0 * m::TEXT_CLIP_SLACK,
            ),
        )
    }

    /// 弹层一次显示几行。
    pub fn popup_rows(&self) -> usize {
        self.libs.len().min(m::PALETTE_COMBO_ROWS)
    }

    /// 弹层第 `i` 行（0 = 弹层第一行）的矩形。
    pub fn combo_row_rect(&self, l: &Layout, i: usize) -> Rect {
        let pop = self.combo_popup_rect(l);
        Rect::from_min_size(
            pos2(
                pop.left() + 1.0,
                pop.top() + m::TEXT_CLIP_SLACK + i as f32 * m::PALETTE_COMBO_ROW_H,
            ),
            vec2(pop.width() - 2.0, m::PALETTE_COMBO_ROW_H),
        )
    }

    /// 把屏幕点换算成帧（不在网格内 / 越界 → `None`）。
    ///
    /// 用的是 [`PaletteState::visible_rect`]：点在底部那截"半行"上不算命中，
    /// 否则会选中一个根本看不见的帧。
    pub fn hit_test(&self, l: &Layout, pos: egui::Pos2) -> Option<FrameKey> {
        let g = Self::visible_rect(l);
        if !g.contains(pos) {
            return None;
        }
        let total = self.current_count()?;
        let col = ((pos.x - g.left()) / m::PALETTE_CELL).floor();
        let row = ((pos.y - g.top()) / m::PALETTE_CELL).floor();
        if col < 0.0 || row < 0.0 {
            return None;
        }
        let idx = (self.scroll + row as usize) * m::PALETTE_GRID_COLS + col as usize;
        if idx as u32 >= total {
            return None;
        }
        Some(FrameKey {
            lib: self.current()?.lib,
            index: idx as u32,
        })
    }

    // ── 交互 ──

    /// **只处理页签点击**（页签行由本结构拥有，两个页签共用）。
    ///
    /// 为什么单独拆出来：「分类」页签（印章浏览器）的其余交互在
    /// [`crate::stamp_panel::StampPanel::interact`] 里，但"切页签"这件事只有这里知道
    /// `tab` 字段 ⇒ 那边每帧只调本函数，不重复实现一份页签几何。
    pub fn interact_tabs(&mut self, ui: &mut Ui, l: &Layout) {
        for (i, r) in crate::dock_right::tab_rects(ui.painter(), l)
            .into_iter()
            .enumerate()
        {
            if ui
                .interact(r, ui.id().with(("palette_tab", i)), Sense::click())
                .clicked()
            {
                self.set_tab(i);
            }
        }
    }

    /// 处理素材面板这一帧的全部点击/滚轮/键盘，返回需要调用方执行的动作。
    pub fn interact(
        &mut self,
        ui: &mut Ui,
        l: &Layout,
        src: Option<&WilSource>,
    ) -> PaletteAction {
        self.refresh_count(src);
        let mut action = PaletteAction::None;

        // ① 页签
        self.interact_tabs(ui, l);

        // ② 库下拉：点框开合；展开时先吃掉弹层内的点击（否则会被下面的网格抢走）
        let combo = crate::dock_right::combo_rect(l);
        if ui
            .interact(combo, ui.id().with("palette_combo"), Sense::click())
            .clicked()
        {
            let open = !self.combo_open;
            self.set_combo_open(open);
        }
        if self.combo_open {
            let pop = self.combo_popup_rect(l);
            if ui
                .ctx()
                .pointer_hover_pos()
                .map(|p| pop.expand(2.0).contains(p))
                .unwrap_or(false)
            {
                let dy = ui.input(|i| i.smooth_scroll_delta.y);
                if dy.abs() > 1.0 {
                    let step = ((dy.abs() / m::PALETTE_COMBO_ROW_H).round() as usize).max(1);
                    let max = self.libs.len().saturating_sub(self.popup_rows());
                    self.combo_scroll = if dy > 0.0 {
                        self.combo_scroll.saturating_sub(step)
                    } else {
                        (self.combo_scroll + step).min(max)
                    };
                }
            }
            let rows = self.popup_rows();
            for i in 0..rows {
                let idx = self.combo_scroll + i;
                if idx >= self.libs.len() {
                    break;
                }
                if ui
                    .interact(
                        self.combo_row_rect(l, i),
                        ui.id().with(("palette_combo_row", i)),
                        Sense::click(),
                    )
                    .clicked()
                {
                    self.set_current(idx);
                    self.combo_open = false;
                }
            }
            // 点在弹层与框之外 ⇒ 收起
            if ui.input(|i| i.pointer.any_click()) {
                let inside = ui
                    .ctx()
                    .pointer_hover_pos()
                    .map(|p| pop.expand(2.0).contains(p) || combo.contains(p))
                    .unwrap_or(false);
                if !inside {
                    self.combo_open = false;
                }
            }
            return action; // 弹层展开时不响应面板其它控件（避免误触）
        }

        // ③ 搜索框：点一下获得焦点；有焦点就吃键盘
        let search = crate::dock_right::search_rect(l);
        if ui
            .interact(search, ui.id().with("palette_search"), Sense::click())
            .clicked()
        {
            self.focus = true;
        } else if ui.input(|i| i.pointer.any_click()) && self.focus {
            // 点别处（不是搜索框）就失焦
            let in_search = ui
                .ctx()
                .pointer_hover_pos()
                .map(|p| search.contains(p))
                .unwrap_or(false);
            if !in_search {
                self.focus = false;
            }
        }
        if self.focus {
            let (texts, backspace, enter) = ui.input(|i| {
                let mut texts = Vec::new();
                for e in &i.events {
                    if let egui::Event::Text(t) = e {
                        texts.push(t.clone());
                    }
                }
                (
                    texts,
                    i.key_pressed(egui::Key::Backspace),
                    i.key_pressed(egui::Key::Enter),
                )
            });
            for t in texts {
                self.push_text(&t);
            }
            if backspace {
                self.backspace();
            }
            if enter {
                let msg = self.submit_search();
                eprintln!("{msg}");
            }
        }

        // ④ 网格：滚轮翻行 + 点选
        let vis = Self::visible_rect(l);
        let hover_in_grid = ui
            .ctx()
            .pointer_hover_pos()
            .map(|p| vis.contains(p))
            .unwrap_or(false);
        if hover_in_grid && self.tab == 0 {
            let dy = ui.input(|i| i.smooth_scroll_delta.y);
            if dy.abs() > 1.0 {
                let step = ((dy.abs() / m::PALETTE_CELL).round() as i64).max(1);
                self.scroll_by(if dy > 0.0 { -step } else { step });
            }
        }
        if self.tab == 0 {
            let resp = ui.interact(vis, ui.id().with("palette_grid"), Sense::click());
            if resp.clicked() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    if let Some(key) = self.hit_test(l, pos) {
                        action = self.pick(key);
                    }
                }
            }
        }

        // ⑤ 分页行：上/下翻一屏（**只有图库页有网格可翻**）
        if self.tab == 0 {
            let (prev, next) = crate::dock_right::pager_rects(l);
            let page = rows_visible(l).max(1) as i64;
            if ui
                .interact(prev, ui.id().with("palette_page_prev"), Sense::click())
                .clicked()
            {
                self.scroll_by(-page);
            }
            if ui
                .interact(next, ui.id().with("palette_page_next"), Sense::click())
                .clicked()
            {
                self.scroll_by(page);
            }
        }

        action
    }

    /// 选中当前库的第 `index` 帧（`--palette-pick` 用）。
    ///
    /// 走的是与鼠标点击**完全相同**的 [`PaletteState::pick`]，只是入口由"屏幕坐标"换成"帧号"，
    /// 好让闭环验收能无人值守复现。
    pub fn pick_frame(&mut self, index: u32) -> PaletteAction {
        let Some(e) = self.current().cloned() else {
            return PaletteAction::None;
        };
        self.pick(FrameKey {
            lib: e.lib,
            index,
        })
    }

    /// 点选一帧：设置高亮；**三族都允许设为笔刷**（写哪个字段由当前编辑层决定）。
    ///
    /// 本版**取消了**「只允许地面库」的限制（原来的 `PALETTE_NEED_GROUND` 那句已删）：
    /// 中层（`SmTiles`）/ 物件（`Objects`）选出来的笔刷带自己的族，
    /// 与当前编辑层不一致时由 [`crate::edit::EditSession::touch`] 在**落笔那一刻**拒绝，
    /// 而不是在素材面板上假装"这个库不能选"。
    fn pick(&mut self, key: FrameKey) -> PaletteAction {
        self.selected = Some(key);
        // 闸门按**图号**判（`index + 1`），不是按 0 基索引判：索引 0x7FFF 落格会变成
        // 0x8000，撞上图号字段的最高位（审计 E1 第 3 条）。
        if crate::edit::palette_index_to_sprite(key.index).is_none() {
            self.note = Some(tx::PALETTE_FRAME_TOO_BIG.to_owned());
            return PaletteAction::None;
        }
        self.note = None;
        PaletteAction::SetBrush(key)
    }

    // ── 绘制 ──

    /// 画网格本体（含缩略图与选中金框）。**只解码可见行 + 1 行预取**。
    pub fn draw(&mut self, p: &Painter, l: &Layout, src: Option<&WilSource>) {
        let g = Self::grid_rect(l);
        let vis = Self::visible_rect(l);
        // 底色铺满整块网格区，裁剪盒只到"整行"为止（预取行不许露半行）
        pt::fill(p, g, t::ui().input_search_bg);
        let clip = p.with_clip_rect(vis);

        self.rows_drawn = rows_visible(l);
        self.slots = 0;
        if self.tab != 0 {
            return; // 占位文案由 `dock_right` 画（它是静态标签，进得了文案自检）
        }
        let (Some(entry), Some(count)) = (self.current().cloned(), self.current_count()) else {
            return; // 库打不开/没登记：`dock_right` 画"该库没有可用帧"
        };
        if count == 0 {
            return;
        }

        let rows = self.rows_drawn;
        let total_rows = (count as usize).div_ceil(m::PALETTE_GRID_COLS);
        let first = self.scroll.min(total_rows.saturating_sub(1));
        self.scroll = first;
        // +1 行预取：滚一格就能立刻出图，不必等下一帧再解码
        let last = (first + rows + 1).min(total_rows);

        for row in first..last {
            for col in 0..m::PALETTE_GRID_COLS {
                let idx = (row * m::PALETTE_GRID_COLS + col) as u32;
                if idx >= count {
                    break;
                }
                self.slots += 1;
                let cell = Self::cell_rect(l, first, row, col);
                let key = FrameKey {
                    lib: entry.lib,
                    index: idx,
                };
                pt::fill(&clip, cell, t::ui().panel_bg);
                pt::border(&clip, cell, t::ui().separator);
                if let Some((id, w, h)) = self.thumb(p.ctx(), src, key) {
                    let inner = cell.shrink(m::PALETTE_THUMB_PAD);
                    let dst = fit_rect(inner, w, h);
                    clip.image(
                        id,
                        dst,
                        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
                if self.selected == Some(key) {
                    pt::border_w(&clip, cell, m::PALETTE_SELECT_STROKE, t::ui().accent_dark);
                }
            }
        }
    }

    /// 画库下拉的弹层（**必须最后画**，它要压在网格之上）。
    pub fn draw_combo_popup(&self, p: &Painter, l: &Layout) {
        if !self.combo_open {
            return;
        }
        let pop = self.combo_popup_rect(l);
        pt::fill(p, pop, t::ui().input_bg);
        pt::border(p, pop, t::ui().input_border);
        let rows = self.popup_rows();
        for i in 0..rows {
            let idx = self.combo_scroll + i;
            let Some(e) = self.libs.get(idx) else {
                break;
            };
            let r = self.combo_row_rect(l, i);
            if idx == self.current {
                pt::fill(p, r, t::ui().accent_soft);
            }
            let text = tx::fill(
                tx::PALETTE_LIB_ROW_TMPL,
                &[("stem", &e.stem), ("fam", e.family_label())],
            );
            let pc = p.with_clip_rect(r.shrink2(vec2(4.0, 0.0)));
            pt::text(
                &pc,
                pos2(r.left() + 4.0, r.center().y),
                egui::Align2::LEFT_CENTER,
                &text,
                m::PALETTE_LABEL_FONT,
                t::ui().text,
            );
        }
    }

    /// 当前高亮帧的缩略图（给「当前素材」面板的预览框用；走同一份缓存与计数）。
    pub fn selected_texture(
        &mut self,
        ctx: &Context,
        src: Option<&WilSource>,
    ) -> Option<(TextureId, u32, u32)> {
        let key = self.selected?;
        self.thumb(ctx, src, key)
    }

    /// 取（必要时解码并上传）某帧的缩略图纹理。
    fn thumb(
        &mut self,
        ctx: &Context,
        src: Option<&WilSource>,
        key: FrameKey,
    ) -> Option<(TextureId, u32, u32)> {
        if let Some(cached) = self.cache.get(key) {
            self.hits += 1;
            return cached.map(|t| (t.tex.id(), t.w, t.h));
        }
        let src = src?;
        match src.frame_of(key.lib, key.index) {
            Some(f) => {
                self.decodes += 1;
                let img = ColorImage::from_rgba_unmultiplied(
                    [f.width as usize, f.height as usize],
                    &f.rgba,
                );
                let tex = ctx.load_texture(
                    format!("miredit-thumb-{}-{}", u32::from(key.lib.index), key.index),
                    img,
                    TextureOptions::LINEAR,
                );
                let id = tex.id();
                let (w, h) = (f.width, f.height);
                self.cache.insert(
                    key,
                    Some(Thumb {
                        tex,
                        w,
                        h,
                    }),
                );
                Some((id, w, h))
            }
            None => {
                // 负缓存：空帧/解码失败只问一次，避免每帧重复敲解码器
                self.cache.insert(key, None);
                None
            }
        }
    }
}

/// 可见行数 = 网格高 / 格边长（向下取整，不铺半行）。
pub fn rows_visible(l: &Layout) -> usize {
    let h = PaletteState::grid_rect(l).height();
    if h <= 0.0 {
        return 0;
    }
    (h / m::PALETTE_CELL).floor().max(0.0) as usize
}

/// 把 `w×h` 等比缩进 `area` 并居中（缩略图口径：**不拉伸**）。
///
/// 96×64 的地表帧缩进 38×38 的格内区 ⇒ 38×25.3；「当前素材」的预览框（188×88）同理。
pub fn fit_rect(area: Rect, w: u32, h: u32) -> Rect {
    if w == 0 || h == 0 {
        return area;
    }
    let s = (area.width() / w as f32).min(area.height() / h as f32);
    Rect::from_center_size(area.center(), vec2(w as f32 * s, h as f32 * s))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fonts;
    use mir2_assets::LibPaths;

    fn window() -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), vec2(m::WINDOW_W, m::WINDOW_H))
    }

    fn layout() -> Layout {
        Layout::compute(window())
    }

    /// 本机图库（未设 `MIREDIT_CLIENT_ROOT` 或装不上就 `None`，让相关测试打印跳过 —— 换机器不该变红）。
    fn 本机路径() -> Option<LibPaths> {
        let Some(root) = mir2_assets::client_root() else {
            eprintln!(
                "[跳过] 未设置 {}（图库扫描根）",
                mir2_assets::CLIENT_ROOT_ENV
            );
            return None;
        };
        match LibPaths::discover(&root) {
            Ok(p) if !p.is_empty() => Some(p),
            _ => {
                eprintln!("[跳过] 本机没有图库 {}", root.display());
                None
            }
        }
    }

    fn 本机源(paths: &LibPaths) -> Option<WilSource> {
        match WilSource::open(paths) {
            Ok(s) => Some(s),
            Err(e) => {
                eprintln!("[跳过] 本机图库打不开：{e}");
                None
            }
        }
    }

    /// 库枚举必须来自真实登记表：`Tiles / Tiles5 / Objects / Objects21` 都在，
    /// 且**缺省选中的是地面 0 号库**（画笔唯一能写的族）。
    #[test]
    fn 库枚举来自真实登记表且缺省选地面库() {
        let Some(paths) = 本机路径() else { return };
        let st = PaletteState::new(Some(&paths));
        assert_eq!(st.libs().len(), paths.available(), "库数必须与登记表一致");
        for stem in ["Tiles", "Tiles5", "Objects", "Objects21"] {
            assert!(
                st.libs().iter().any(|e| e.stem == stem),
                "登记表里有 {stem}，面板却没枚举出来"
            );
        }
        let cur = st.current().expect("缺省必须有当前库");
        assert_eq!(cur.lib, LibRef::ground(0));
        assert_eq!(cur.stem, "Tiles");
        assert_eq!(cur.family_label(), tx::LAYER_GROUND);
    }

    /// 图库装载失败 ⇒ 一个库都没有（面板走空态，不伪造库）。
    #[test]
    fn 无图库时列表为空() {
        let st = PaletteState::new(None);
        assert!(st.libs().is_empty());
        assert!(st.current().is_none());
        assert_eq!(st.current_count(), None);
        assert_eq!(st.total_rows(), 0);
    }

    /// **虚拟滚动：只解码可见行 + 1 行预取**，且总帧数远大于解码数。
    ///
    /// 这条是"禁止一次性解码全库"的机器闸门：`Tiles` 有 31775 帧，
    /// 顶部一屏最多 `6 行 × 8 列 = 48` 格 + 8 预取 = 56 次解码。
    #[test]
    fn 只解码可见行() {
        let Some(paths) = 本机路径() else { return };
        let Some(src) = 本机源(&paths) else { return };
        let l = layout();
        let ctx = fonts::measure_ctx();
        let painter = Painter::new(
            ctx,
            egui::LayerId::debug(),
            window(),
        );
        let mut st = PaletteState::new(Some(&paths));
        st.refresh_count(Some(&src));
        let total = st.current_count().expect("Tiles 要有帧数");
        assert!(total > 30000, "本机 Tiles 应有三万多帧，实际 {total}");

        st.draw(&painter, &l, Some(&src));
        let rows = rows_visible(&l);
        assert_eq!(rows, 6, "1500×950 下网格应可见 6 行（可见行数变了要同步改注释）");
        let slots = st.slots();
        assert_eq!(slots, (rows + 1) * m::PALETTE_GRID_COLS, "槽位必须 = (可见行+1)×8");
        assert!(
            (st.decodes() as usize) <= slots,
            "解码 {} 次不该超过槽位 {}",
            st.decodes(),
            slots
        );
        assert!(
            st.decodes() * 100 < u64::from(total),
            "解码 {} 次 / 总帧 {total} —— 必须远小于总帧数",
            st.decodes()
        );
        eprintln!(
            "[顶部] 总帧 {total} / 槽位 {slots} / 解码 {} / 空帧 {}",
            st.decodes(),
            st.empty_count()
        );

        // 再画一次同一屏：应当**全是缓存命中**，解码次数不涨
        let before = st.decodes();
        st.draw(&painter, &l, Some(&src));
        assert_eq!(st.decodes(), before, "同一屏重绘不许再解码（缓存必须生效）");
        assert!(st.hits() > 0, "重绘应当有缓存命中");

        // 滚到中部：解码的是新的一屏，总量仍远小于总帧数
        let mid = st.total_rows() / 2;
        st.set_scroll(mid);
        st.draw(&painter, &l, Some(&src));
        let mid_decodes = st.decodes();
        eprintln!(
            "[中部] 顶部行 {mid} / 累计解码 {mid_decodes} / 缓存命中 {} / 空帧 {}",
            st.hits(),
            st.empty_count()
        );
        assert!(
            mid_decodes * 100 < u64::from(total),
            "滚到中部累计解码 {mid_decodes} 次 / 总帧 {total} —— 必须远小于总帧数"
        );
    }

    /// 滚回去时**不许重复解码**：滚到中部再滚回顶部，解码数只增一屏左右。
    #[test]
    fn 滚回已看过的行不重复解码() {
        let Some(paths) = 本机路径() else { return };
        let Some(src) = 本机源(&paths) else { return };
        let l = layout();
        let ctx = fonts::measure_ctx();
        let painter = Painter::new(ctx, egui::LayerId::debug(), window());
        let mut st = PaletteState::new(Some(&paths));
        st.refresh_count(Some(&src));
        st.draw(&painter, &l, Some(&src));
        let first = st.decodes();

        st.set_scroll(400);
        st.draw(&painter, &l, Some(&src));
        let mid = st.decodes();
        assert!(mid > first, "换一屏应当有新的解码");

        st.set_scroll(0);
        st.draw(&painter, &l, Some(&src));
        assert_eq!(
            st.decodes(),
            mid,
            "滚回已看过的行不该再解码（缓存没生效）"
        );
    }

    /// 搜索：纯数字**图号（1 基）**命中并跳到含该帧的行；非数字/越界如实提示，**不猜语义**。
    ///
    /// 审计 E1 起搜索框收的是「图号」= 0 基索引 + 1：`FrameKey::index = 图号 - 1`，
    /// 合法区间 `1..=total`（`0` 现在是越界值，不是"第一帧"）。
    #[test]
    fn 搜索只认纯数字() {
        let Some(paths) = 本机路径() else { return };
        let Some(src) = 本机源(&paths) else { return };
        let mut st = PaletteState::new(Some(&paths));
        st.refresh_count(Some(&src));
        let total = st.current_count().expect("Tiles 要有帧数");

        // 打的是图号，落的是索引
        let want_img = 12000u32.min(total);
        st.push_text(&want_img.to_string());
        let diag = st.submit_search();
        let want_idx = want_img - 1;
        assert_eq!(
            st.scroll(),
            (want_idx as usize / m::PALETTE_GRID_COLS).saturating_sub(1),
            "应滚到含该图号的行"
        );
        assert_eq!(
            st.selected(),
            Some(FrameKey {
                lib: LibRef::ground(0),
                index: want_idx
            }),
            "图号 {want_img} 命中的是索引 {want_idx}"
        );
        assert!(st.note().is_some_and(|s| s.contains(&want_img.to_string())));
        assert!(diag.contains(&want_img.to_string()), "{diag}");

        // 中文/字母：只说"输入图号"
        st.clear_search();
        st.push_text("地表");
        st.submit_search();
        assert_eq!(st.note(), Some(tx::PALETTE_SEARCH_NEED_DIGITS));

        // `0` 现在是越界值（图号从 1 起）：不许再被当成"第一帧"静默接受
        st.clear_search();
        st.push_text("0");
        st.submit_search();
        assert!(
            st.note()
                .is_some_and(|s| s.contains("1-") && s.contains(&total.to_string())),
            "图号 0 必须报越界、并给出真实区间 1-{total}：{:?}",
            st.note()
        );

        // 越界：如实报范围
        st.clear_search();
        st.push_text(&(total + 5).to_string());
        st.submit_search();
        assert!(
            st.note().is_some_and(|s| s.contains(&total.to_string())),
            "越界提示里要带真实上限（= 帧总数 {total}）：{:?}",
            st.note()
        );

        // 空输入不提示（不影响任何状态）
        st.clear_search();
        st.submit_search();
        assert_eq!(st.note(), None);
    }

    /// 点选**三族**的帧都返回 `SetBrush`（写哪个字段交给当前编辑层决定）；
    /// 只有图号越界时才拒绝并给出原因。
    #[test]
    fn 点选三族都能设笔刷() {
        let Some(paths) = 本机路径() else { return };
        let l = layout();
        let Some(src) = 本机源(&paths) else { return };
        let mut st = PaletteState::new(Some(&paths));
        st.refresh_count(Some(&src));
        // 第 2 行第 3 列 = 帧 11
        let pos = PaletteState::cell_rect(&l, 0, 2, 3).center();
        let key = st.hit_test(&l, pos).expect("格内应命中一帧");
        assert_eq!(key.index, 2 * m::PALETTE_GRID_COLS as u32 + 3);
        assert_eq!(st.pick(key), PaletteAction::SetBrush(key));
        assert_eq!(st.selected(), Some(key));

        // 物件库与中层库同样允许（笔刷带族，落笔由当前编辑层把关）
        for stem in ["Objects", "SmTiles"] {
            let i = st
                .libs()
                .iter()
                .position(|e| e.stem == stem)
                .unwrap_or_else(|| panic!("本机应有 {stem}"));
            st.set_current(i);
            st.refresh_count(Some(&src));
            let key = st.hit_test(&l, pos).expect("格内应命中一帧");
            assert_eq!(
                st.pick(key),
                PaletteAction::SetBrush(key),
                "{stem} 的帧也必须能设为笔刷（写哪个字段由当前编辑层决定）"
            );
            assert_eq!(st.note(), None, "{stem} 点选不该再报「只能用地面库」");
            assert_eq!(st.selected(), Some(key));
        }
    }

    /// 命中测试：网格外的点、超出帧数的格子都返回 `None`（不许点到"不存在的帧"）。
    #[test]
    fn 命中测试不出网格() {
        let Some(paths) = 本机路径() else { return };
        let l = layout();
        let mut st = PaletteState::new(Some(&paths));
        // 没查帧数 ⇒ 还没法命中（不猜）
        assert_eq!(st.hit_test(&l, PaletteState::grid_rect(&l).center()), None);
        st.counts.insert(LibRef::ground(0), Some(20));
        let g = PaletteState::grid_rect(&l);
        assert!(st.hit_test(&l, pos2(g.left() - 1.0, g.center().y)).is_none());
        assert!(st.hit_test(&l, pos2(g.center().x, g.bottom() + 1.0)).is_none());
        // 帧数 20 ⇒ 第 3 行第 5 列（= 61）越界
        assert_eq!(
            st.hit_test(&l, PaletteState::cell_rect(&l, 0, 3, 5).center()),
            None
        );
        // 第 1 行第 5 列 = 13，合法
        assert_eq!(
            st.hit_test(&l, PaletteState::cell_rect(&l, 0, 1, 5).center()),
            Some(FrameKey {
                lib: LibRef::ground(0),
                index: 13
            })
        );
    }

    /// 几何自洽：8 列刚好铺满可用宽、6 行整、格子互不重叠且在网格内。
    #[test]
    fn 网格几何自洽() {
        let l = layout();
        let g = PaletteState::grid_rect(&l);
        assert!((g.width() - m::PALETTE_CELL * m::PALETTE_GRID_COLS as f32).abs() < 0.01);
        assert_eq!(rows_visible(&l), 6);
        let rows = rows_visible(&l) + 1; // 连预取那一行一起验
        for row in 0..rows {
            for col in 0..m::PALETTE_GRID_COLS {
                let c = PaletteState::cell_rect(&l, 0, row, col);
                assert!((c.width() - m::PALETTE_CELL).abs() < 0.01);
                // 水平方向必须整格落在网格里
                assert!(c.left() >= g.left() - 0.01 && c.right() <= g.right() + 0.01);
                // 可见行必须在网格里；预取那一行允许在网格下方（它就是给滚动准备的）
                if row < rows_visible(&l) {
                    assert!(c.top() >= g.top() - 0.01 && c.bottom() <= g.bottom() + 0.01);
                }
            }
        }
        // 提示行在网格与分页行之间，不与网格重叠
        let n = PaletteState::note_rect(&l);
        assert!(n.top() >= g.bottom() - 0.01, "提示行压在网格上");
        assert!(n.bottom() <= l.p_palette.content.bottom() - m::PAGER_ROW_H);
    }

    /// 库下拉弹层几何：行矩形必须落在弹层内、行高一致、且不以 `current` 为基准漂移。
    #[test]
    fn 库下拉弹层几何自洽() {
        let Some(paths) = 本机路径() else { return };
        let l = layout();
        let mut st = PaletteState::new(Some(&paths));
        st.set_combo_open(true);
        assert!(st.combo_open(), "set_combo_open(true) 应真的展开");
        let pop = st.combo_popup_rect(&l);
        let combo = crate::dock_right::combo_rect(&l);
        assert!((pop.left() - combo.left()).abs() < 0.01, "弹层要与组合框左对齐");
        assert!(pop.top() > combo.bottom(), "弹层在组合框下方");
        assert_eq!(st.popup_rows(), m::PALETTE_COMBO_ROWS.min(st.libs().len()));
        for i in 0..st.popup_rows() {
            let r = st.combo_row_rect(&l, i);
            assert!(r.left() >= pop.left() && r.right() <= pop.right());
            assert!((r.height() - m::PALETTE_COMBO_ROW_H).abs() < 0.01);
            if i > 0 {
                let prev = st.combo_row_rect(&l, i - 1);
                assert!((r.top() - prev.bottom() - 0.0).abs() < 0.01, "行间不许有缝/重叠");
            }
        }
        // 弹层底端不许压到分页行
        let last = st.combo_row_rect(&l, st.popup_rows() - 1);
        let (prev_btn, _) = crate::dock_right::pager_rects(&l);
        assert!(last.bottom() <= prev_btn.top(), "弹层压到分页行了");
        st.set_combo_open(false);
        assert!(!st.combo_open());
    }

    /// 面板里的动态文字（帧数 / 区间 / 库名 / 提示）都放得进各自的裁剪盒。
    ///
    /// 用与绘制相同的测量函数（[`crate::paint::text_w`]）与真实数据，
    /// 这条挡的是"三万多帧的数字把 '帧 N' 撑到溢出到隔壁列"。
    #[test]
    fn 面板动态文字放得下() {
        let l = layout();
        let ctx = fonts::measure_ctx();
        let painter = Painter::new(ctx, egui::LayerId::debug(), window());
        let g = PaletteState::grid_rect(&l);
        let note = PaletteState::note_rect(&l);
        // 最坏情况：最大帧数（Tiles 31775）、最长区间串、最长库名
        let worst = [
            (
                tx::fill(tx::PALETTE_FRAMES_TMPL, &[("n", "31775")]),
                g.width(),
                m::PALETTE_LABEL_FONT,
            ),
            (
                tx::fill(
                    tx::PALETTE_RANGE_TMPL,
                    // 1 基图号的**最坏宽度**：`1-48` 短于 `1-31760`，取后者
                    &[("a", "1"), ("b", "31760"), ("n", "31775")],
                ),
                // 分页行中间那段的可用宽 = 总宽 - 两个按钮 - 两个内缩
                g.width() - 2.0 * (m::PAGER_BTN_W + m::PAGER_BTN_INSET) - 8.0,
                m::BODY_FONT,
            ),
            (
                tx::fill(
                    tx::PALETTE_LIB_ROW_TMPL,
                    &[("stem", "Objects5107"), ("fam", tx::LAYER_OBJECTS)],
                ),
                g.width() - 8.0,
                m::PALETTE_LABEL_FONT,
            ),
            (tx::PALETTE_FRAME_TOO_BIG.to_owned(), note.width(), m::PALETTE_LABEL_FONT),
            (
                tx::fill(
                    tx::PALETTE_SEARCH_OUT_OF_RANGE_TMPL,
                    &[("max", "31775")],
                ),
                note.width(),
                m::PALETTE_LABEL_FONT,
            ),
        ];
        for (text, cap, font) in worst {
            let w = pt::text_w(&painter, &text, font);
            eprintln!("[宽度] {text:?} w={w:.1} cap={cap:.1}");
            assert!(w <= cap, "{text:?} 宽 {w:.1} 放不进 {cap:.1}");
        }
    }
}
