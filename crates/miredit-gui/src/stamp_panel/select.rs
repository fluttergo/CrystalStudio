//! 印章面板的**过滤与选中**：分类下拉的行表、搜索、翻页、唯一选中入口。
//!
//! 本模块是 [`super`] 的子模块（同一份状态）。
//!
//! 两条纪律：
//!
//! 1. 搜索**走 `StampCatalog::search`**（只匹配 id/name 子串、大小写不敏感），
//!    本模块不自己写一套子串匹配；
//! 2. 选中只从 [`StampPanel::pick`] 走一条路（鼠标点击与 `--stamp-pick` 共用），
//!    格数/包围盒来自 `resolve`，取不到就如实 `None`（界面填 `—`）。

use mir2_stamp::kind_label_zh;

use crate::ui_text as tx;

use super::geom::page_range;
use super::{file_name, Filter, Item, Selected, StampAction, StampPanel};

impl StampPanel {
    /// 分类下拉的全部行：`全部分类` → 19 个 kind（条数降序）→ `未分类` → `未登记`。
    ///
    /// 条数全部来自实测（`kinds` / `unclassified` / `unlisted`），**没有一行是常量表**。
    pub fn kind_rows(&self) -> Vec<(Filter, usize)> {
        let mut out = Vec::with_capacity(self.kinds.len() + 3);
        let registered: usize = self.kinds.iter().map(|(_, n)| n).sum::<usize>() + self.unclassified;
        out.push((Filter::All, registered));
        for (k, n) in &self.kinds {
            out.push((Filter::Kind(k.clone()), *n));
        }
        if self.unclassified > 0 {
            out.push((Filter::Unclassified, self.unclassified));
        }
        out.push((Filter::Unlisted, self.unlisted));
        out
    }

    /// 切分类过滤（越界/未知键返回 `false`，**不静默换成「全部」**）。
    pub fn set_filter(&mut self, filter: Filter) -> bool {
        let ok = match &filter {
            Filter::All => true,
            Filter::Kind(k) => self.kinds.iter().any(|(kk, _)| kk == k),
            Filter::Unclassified => self.unclassified > 0,
            Filter::Unlisted => true,
        };
        if !ok {
            return false;
        }
        self.filter = filter;
        self.selected = None;
        self.combo_open = false;
        self.rebuild();
        true
    }

    /// 按下拉键切分类（`all` / `unclassified` / `unlisted` / 英文 kind）。
    pub fn set_filter_key(&mut self, key: &str) -> bool {
        let f = match key.trim() {
            "all" => Filter::All,
            "unclassified" => Filter::Unclassified,
            "unlisted" => Filter::Unlisted,
            other => Filter::Kind(other.to_owned()),
        };
        self.set_filter(f)
    }

    /// 设搜索词并立即重算列表（子串匹配走 `catalog.search`，大小写不敏感）。
    pub fn set_search(&mut self, q: &str) {
        self.search = q.to_owned();
        self.selected = None;
        self.rebuild();
    }

    /// 翻到某页（0 基；越界夹到 `0..=pages-1`），页内滚动归零。
    pub fn set_page(&mut self, page: usize) {
        self.page = page.min(self.pages().saturating_sub(1));
        self.row_off = 0;
    }

    /// 重算可见列表（分类 / 搜索变了就调）。
    pub(super) fn rebuild(&mut self) {
        self.page = 0;
        self.row_off = 0;
        let mut out: Vec<Item> = Vec::new();
        if let Some(cat) = self.catalog() {
            if self.filter == Filter::Unlisted {
                // 未登记项没有 name/id 可搜 ⇒ 按**文件名**子串过滤（大小写不敏感）
                let needle = self.search.trim().to_lowercase();
                for (i, rel) in cat.unlisted().iter().enumerate() {
                    if needle.is_empty() || rel.to_lowercase().contains(&needle) {
                        out.push(Item::Unlisted(i as u32));
                    }
                }
            } else {
                // 搜索非空 ⇒ **走 catalog.search**（只匹配 id/name 子串，大小写不敏感），
                // 再把结果映射回 `all()` 下标以保持 palette.json 顺序。
                let base: Vec<u32> = if self.search.trim().is_empty() {
                    (0..cat.len() as u32).collect()
                } else {
                    cat.search(&self.search)
                        .into_iter()
                        .filter_map(|e| self.by_id.get(&e.id.to_lowercase()).copied())
                        .collect()
                };
                for i in base {
                    let Some(e) = cat.all().get(i as usize) else {
                        continue;
                    };
                    if self.kind_matches(e.kind.as_deref()) {
                        out.push(Item::Entry(i));
                    }
                }
            }
        }
        self.visible = out;
    }

    /// 该 kind 是否落在当前过滤里（`None`/空白 = 未分类）。
    pub(super) fn kind_matches(&self, kind: Option<&str>) -> bool {
        match &self.filter {
            Filter::All => true,
            Filter::Kind(k) => kind == Some(k.as_str()),
            Filter::Unclassified => kind.map(|s| s.trim().is_empty()).unwrap_or(true),
            Filter::Unlisted => false,
        }
    }

    /// 列表第 `idx` 项的显示名与"分类中文名"（未登记项 = 文件名 + 「未登记」）。
    pub(super) fn item_text(&self, idx: usize) -> Option<(String, String)> {
        let cat = self.catalog()?;
        match self.visible.get(idx)? {
            Item::Entry(i) => {
                let e = cat.all().get(*i as usize)?;
                let zh = e
                    .kind
                    .as_deref()
                    .map(|k| kind_label_zh(k).unwrap_or(k).to_owned())
                    .unwrap_or_else(|| kind_label_zh("").unwrap_or(tx::VAL_UNKNOWN).to_owned());
                Some((e.name.clone(), zh))
            }
            Item::Unlisted(i) => {
                let rel = cat.unlisted().get(*i as usize)?;
                Some((file_name(rel), tx::STAMP_UNLISTED.to_owned()))
            }
        }
    }

    /// 选中列表第 `idx` 项（**唯一的选中入口**：鼠标点击与 `--stamp-pick` 都走它）。
    pub fn pick(&mut self, idx: usize) -> StampAction {
        let Some((name, kind_zh)) = self.item_text(idx) else {
            return StampAction::None;
        };
        let item = self.visible[idx];
        let (id, cells, bounds, path) = match item {
            Item::Entry(i) => {
                let id = self
                    .catalog()
                    .and_then(|c| c.all().get(i as usize))
                    .map(|e| e.id.clone())
                    .unwrap_or_default();
                // 格数/包围盒/绝对路径**全部**来自 `resolve`（真实读盘 + 解析；
                // 失败 ⇒ 三个都是 `None`，界面填 `—`、放置时报中文原因）
                let r = self.catalog_mut().and_then(|c| c.resolve(&id));
                match r {
                    Some(r) => (id, Some(r.cell_count), Some(r.bounds), Some(r.path)),
                    None => (id, None, None, None),
                }
            }
            Item::Unlisted(i) => {
                let rel = self
                    .catalog()
                    .and_then(|c| c.unlisted().get(i as usize))
                    .cloned()
                    .unwrap_or_default();
                // 未登记项没有 `resolve` 可言 ⇒ **不猜路径**（见 `Selected::path` 的说明）
                (rel, None, None, None)
            }
        };
        self.selected = Some(Selected {
            id,
            name,
            kind_zh,
            cells,
            bounds,
            path,
        });
        StampAction::Pick
    }

    /// 选中当前页的第 `slot` 格（`--stamp-pick` 用；越界返回 `false`）。
    pub fn pick_slot(&mut self, slot: usize) -> bool {
        let (start, end) = page_range(self.total(), self.page);
        let idx = start + slot;
        if idx >= end {
            return false;
        }
        self.pick(idx) == StampAction::Pick
    }

}
