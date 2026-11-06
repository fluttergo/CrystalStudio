//! 印章缩略图：**LRU 缓存** / `.X` → `PositionedCell` 适配 / 渲染 / 文本截断。
//!
//! 本模块是 [`super`] 的子模块：字段与函数的可见性沿用父模块的私有约定
//! （子模块能看见父模块的私有项，所以**不必**把字段全改成 `pub`）。
//!
//! 三条纪律（与父模块文档 §四 同一份）：
//!
//! 1. 缓存上限 [`crate::metrics::STAMP_THUMB_CACHE_CAP`]，LRU 淘汰（纹理随 `TextureHandle::drop` 释放）；
//! 2. **负缓存**：缺帧 / 读不出 / 解析失败 / 库号越界 / 渲染报错，一律记 `Missing`，不重复读盘；
//! 3. 适配口径与 `miredit-render/tests/stamp.rs` **逐字段相同**（图号取低 15 位、库号 `i16→u8`）。

use std::collections::{HashMap, VecDeque};

use egui::{Painter, TextureHandle, TextureId};
use mir2_stamp::Stamp;

use miredit_render::stamp::{render_stamp, PositionedCell, StampRenderOptions};

use crate::metrics as m;
use crate::paint as pt;
use crate::ui_text as tx;

/// 一张已上传的缩略图，或"这枚取不到帧"的负缓存。
pub(super) enum Thumb {
    /// 真渲染出来的 RGBA 纹理（`w`/`h` 是纹理像素尺寸，用于等比缩进格子）。
    Ready {
        /// egui 纹理句柄（`Drop` 时自动释放）。
        tex: TextureHandle,
        /// 纹理宽。
        w: u32,
        /// 纹理高。
        h: u32,
    },
    /// 缺帧 / 读不出 / 渲染失败（**负缓存**，同一枚不再重复读盘）。
    Missing,
}

/// 一次查缓存的结论（**只返回 `Copy` 数据**，避免把 `&mut self` 的借用带出去）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Hit {
    /// 命中一张纹理。
    Ready(TextureId, u32, u32),
    /// 命中负缓存（缺帧）。
    Missing,
    /// 没命中，需要现渲染。
    Miss,
}

/// LRU 缩略图缓存：`印章 id → 纹理 / 缺帧`。
///
/// **泛型**是为了让验收探针（`--stamp-probe`，不开窗）能用**同一份淘汰代码**压测，
/// 只把 payload 换成 `bool`（不为压测去申请显存）—— 这样"探针报的峰值/命中/淘汰"
/// 与"面板真实行为"是同一套规则，不是两份各自实现的 LRU。
pub(crate) struct LruCache<V> {
    map: HashMap<String, V>,
    /// 队首 = 最久未用。
    order: VecDeque<String>,
    /// 历史峰值（验收要报的数，见任务书 §四.4）。
    peak: usize,
    /// 淘汰次数。
    evictions: u64,
}

impl<V> LruCache<V> {
    pub(crate) fn new() -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
            peak: 0,
            evictions: 0,
        }
    }

    /// 查一条并刷新 LRU（命中负缓存也刷新 —— 它同样"被用到"了）。
    pub(crate) fn get(&mut self, key: &str) -> Option<&V> {
        if !self.map.contains_key(key) {
            return None;
        }
        if let Some(pos) = self.order.iter().position(|x| x == key) {
            self.order.remove(pos);
        }
        self.order.push_back(key.to_owned());
        self.map.get(key)
    }

    /// 放一条；超过上限按 LRU 淘汰最久未用的（**纹理随 `TextureHandle::drop` 一起释放**）。
    pub(crate) fn insert(&mut self, key: String, value: V) {
        if self.map.insert(key.clone(), value).is_some() {
            if let Some(pos) = self.order.iter().position(|x| *x == key) {
                self.order.remove(pos);
            }
        }
        self.order.push_back(key);
        while self.map.len() > m::STAMP_THUMB_CACHE_CAP {
            match self.order.pop_front() {
                Some(old) => {
                    self.map.remove(&old);
                    self.evictions += 1;
                }
                None => break,
            }
        }
        self.peak = self.peak.max(self.map.len());
    }

    pub(crate) fn len(&self) -> usize {
        self.map.len()
    }

    /// 历史峰值（验收要报的数）。
    pub(crate) fn peak(&self) -> usize {
        self.peak
    }

    /// 淘汰次数（验收要报的数）。
    pub(crate) fn evictions(&self) -> u64 {
        self.evictions
    }
}


/// 把一段**数据驱动**文字按宽度截断成 `…`。
///
/// 为什么不走 [`crate::text_fit::DynLabel`]：`DynLabel::tmpl` 要求是
/// [`crate::ui_text::ALL`] 里登记的**界面文案**，而印章名称是**数据**（9,159 条里什么都有），
/// 硬塞进文案表就等于把数据当文案管。截断口径与 `DynLabel` 一致（同一测量函数、同一省略号）。
///
/// # 掐头留尾（不是掐尾留头）
///
/// 真实名称的形态是 `<subtype> — <id>`（如 `Terrain — mir_00293`）：**区分段在尾部**。
/// 若按常规掐尾，格子里 20 个标签会全变成 `Terrain — mir_…`（实测第一版就是这样，
/// 肉眼完全分不出是哪一枚）。所以这里保留**尾段**、把省略号放前面 —— 得到 `…mir_00293`，
/// 一眼能对上 id。
pub fn fit_text(p: &Painter, s: &str, font: f32, max_w: f32) -> String {
    if pt::text_w(p, s, font) <= max_w {
        return s.to_owned();
    }
    let chars: Vec<char> = s.chars().collect();
    // 从尾部往前留字符：留得越多越完整，找到"最长且放得下"的那一段。
    // 再把这段的**开头标点/空格**去掉 —— 否则会得到 `… — mir_00293` 这种以破折号开头的串。
    let mut keep = chars.len();
    while keep > 0 {
        keep -= 1;
        let tail: String = chars[chars.len() - keep..].iter().collect();
        let tail = tail.trim_start_matches(|c: char| !c.is_alphanumeric());
        let t = format!("{}{}", tx::TRUNC_SUFFIX, tail);
        if pt::text_w(p, &t, font) <= max_w {
            return t;
        }
    }
    tx::TRUNC_SUFFIX.to_owned()
}

/// `mir2_stamp::Stamp` → 渲染库的 [`PositionedCell`]（**适配在本 crate，渲染库不认识 `.X`**）。
///
/// # 字段搬运只有一份：先走 [`crate::stamp_place::cells_of`]
///
/// 2026-11-05 起，`.X → 三族图号/库号` 的适配**唯一实现**在
/// [`crate::stamp_place::cells_of`]（印章放置也用它）。本函数只在它的结果上再做
/// **渲染侧的一步**：把图号掩成低 15 位（`& 0x7FFF`）—— 因为渲染库只认纯图号，
/// 高位是编辑器的阻挡/标记位，带进去会取到别的帧。
///
/// 为什么必须共用：两边各写一份的话，"面板里看到的缩略图"与"贴到地上的格"迟早不一致
/// （哪怕只差一个掩码），而那种不一致只能靠逐字节比对才发现。
///
/// 库号越界（真实 9,159 枚里没有）⇒ **返回 `None`**：绝不截断成另一个库号
/// （那会取到别的图 = 假数据）。
pub fn positioned_cells(stamp: &Stamp) -> Option<Vec<PositionedCell>> {
    let cells = crate::stamp_place::cells_of(stamp)?;
    Some(
        cells
            .into_iter()
            .map(|c| PositionedCell {
                dx: c.dx,
                dy: c.dy,
                bk: c.bk & 0x7FFF,
                mid: c.mid & 0x7FFF,
                fr: c.fr & 0x7FFF,
                bk_lib: c.bk_lib,
                mid_lib: c.mid_lib,
                fr_lib: c.fr_lib,
            })
            .collect(),
    )
}

/// 渲染一枚 `.X` 的缩略图（**面板与探针共用这一条路径**）。
///
/// 返回 `(格, 图)`；`.X` 读不出 / 解析失败 / 库号越界 ⇒ `Err(原因)`（原因只进诊断，不上屏）。
pub fn render_thumb(
    path: &std::path::Path,
    src: &dyn mir2_assets::SpriteSource,
) -> Result<(usize, miredit_render::stamp::StampImage), String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let stamp = Stamp::parse(&bytes).map_err(|e| e.to_string())?;
    let cells = positioned_cells(&stamp).ok_or_else(|| "库号超出 u8".to_owned())?;
    let img = render_stamp(
        &cells,
        src,
        &StampRenderOptions::thumbnail(
            m::STAMP_THUMB_W as u32,
            m::STAMP_THUMB_H as u32,
        ),
    )
    .map_err(|e| e.to_string())?;
    Ok((stamp.cells.len(), img))
}

