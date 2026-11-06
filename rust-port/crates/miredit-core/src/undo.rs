//! 撤销 / 重做栈：**整格快照**、一次拖动 = 一步、上限 100 步。
//!
//! 抽取自 `miredit-gui/src/edit.rs:554-579`（`Change` / `Step` / `UNDO_LIMIT`）与
//! `edit.rs:851-960`（`begin` / `end`）、`edit.rs:1288-1323`（`undo` / `redo`）。
//!
//! # 一、为什么快照是**整格**
//!
//! 撤销要能逐字节还原**任何一层**的改动（`edit.rs:563`）。只记图号的话，橡皮/阻挡层的位翻转
//! 就还原不了；只记"改了哪一位"的话，`Old12`/`New14`/`Ext36` 三档的差异又得再分一次。
//! 一格 `Cell` 定长 36 B（`mir2-map/src/cell.rs:176-202`），一步最多 200,000 格
//! （[`crate::FILL_MAX_CELLS`]）≈ 15 MB，仍在可接受范围（`metrics.rs:209-210` 同款估算）。
//!
//! # 二、一次拖动 = 一步（`edit.rs:89-92`）
//!
//! `begin` → 若干次落笔（每次经 [`UndoStack::push`] 记一格）→ `end` 压成一个 [`Step`]，
//! 于是一次拖动只占一层撤销。**同一格在一次拖动里只记第一次的 `before`**
//! （否则撤销会退到中间态）—— 去重由 [`UndoStack::mark_seen`] 用 `HashSet` 做 O(1) 判定：
//! 笔刷尺寸上到 31×31 后一笔就要落 961 格，线性扫描会让一次长拖动退化成 O(n²)
//! （`edit.rs:609-614` 实测：31×31 拖过 100 个点 = 9.6 万格 ⇒ 几十亿次比较）。
//!
//! # 三、撤销后必须**逐字节**回到操作前
//!
//! `undo` 把 `changes` 逆序写回 `before`、`redo` 正序写回 `after`（`edit.rs:1294-1300`、
//! `:1313-1318`）。因为 `Map::to_bytes()` 是逐格序列化（`mir2-map/src/map.rs:117-125`），
//! 格值回到原样 ⇒ 字节必然回到原样 —— 这是"撤销逐字节"的机制性保证，有集成测试钉住。
//!
//! # 四、上限与保存点
//!
//! - [`UNDO_LIMIT`] = 100 步（`edit.rs:579`）；第 101 步把最老的一步挤掉。
//!   挤掉后 `saved_pos` 可能变得不可达 ⇒ `dirty` 恒为真（保守但不会骗人，`edit.rs:956`）。
//! - 保存点记的是**步号**（`saved_pos`）：撤销回到保存点后 `dirty` 必须自动变回 `false`
//!   （`edit.rs:719-722`），所以脏判据不是"栈非空"而是"当前位置 ≠ 保存点"。

use std::collections::HashSet;

use mir2_map::{Cell, Map};

use crate::document::cell_mut;

/// 一格的变更（撤销用它回到 `before`，重做用它回到 `after`）。来源：`edit.rs:556-567`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Change {
    /// 世界格 X。
    pub x: u16,
    /// 世界格 Y。
    pub y: u16,
    /// 改前整格（**整格**而不是只记图号：撤销要能逐字节还原任何一层的改动）。
    pub before: Cell,
    /// 改后整格。
    pub after: Cell,
}

/// 一步 = 一次拖动（按下到松开）或一次填充产生的全部变更。来源：`edit.rs:569-576`。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    /// 单调递增的步号（`0` 保留给"文件刚打开时的状态"）。
    pub seq: u64,
    /// 本步改过的格（同格只出现一次，按经过顺序）。
    pub changes: Vec<Change>,
}

/// 撤销栈上限 100 步。来源：`edit.rs:578-579`（任务书 §三.2）。
pub const UNDO_LIMIT: usize = 100;

/// 撤销 / 重做栈 + 当前笔画的累积缓冲。抽取自 `edit.rs:604-623` 的五个字段。
#[derive(Debug, Default)]
pub struct UndoStack {
    /// 撤销栈（顶 = 最近一步）。
    undo: Vec<Step>,
    /// 重做栈（撤销时压入，新的一步会清空）。
    redo: Vec<Step>,
    /// 当前拖动累积的变更。
    stroke: Vec<Change>,
    /// 本次拖动**已经改过的格**（O(1) 去重，见模块文档第二节）。
    stroke_seen: HashSet<(u16, u16)>,
    /// 是否正在一次拖动中。
    stroking: bool,
    /// 步号发生器（只增，不复用 ⇒ 步号可以当"位置"用）。
    seq: u64,
    /// 保存点所在的步号（`0` = 就是刚打开文件时的状态）。
    saved_pos: u64,
}

impl UndoStack {
    /// 空栈（= 文件刚打开时的状态）。
    pub fn new() -> Self {
        Self::default()
    }

    // ── 只读 ──

    /// 撤销栈深度。来源：`edit.rs:700-702`。
    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    /// 重做栈深度。来源：`edit.rs:705-707`。
    pub fn redo_len(&self) -> usize {
        self.redo.len()
    }

    /// 当前所处步号（`0` = 文件刚打开时）。来源：`edit.rs:710-712`。
    pub fn pos(&self) -> u64 {
        self.undo.last().map(|s| s.seq).unwrap_or(0)
    }

    /// 保存点步号。来源：`edit.rs:715-717`。
    pub fn saved_pos(&self) -> u64 {
        self.saved_pos
    }

    /// 是否正在一次拖动中。来源：`edit.rs:745-747`。
    pub fn stroke_active(&self) -> bool {
        self.stroking
    }

    /// 当前拖动里**已有未结算的变更**（脏判据要用，见 [`Self::dirty`]）。
    pub fn stroke_pending(&self) -> bool {
        !self.stroke.is_empty()
    }

    /// 当前拖动最近落笔的格（插值起点）。来源：`edit.rs:750-752`。
    pub fn last_cell(&self) -> Option<(u16, u16)> {
        self.stroke.last().map(|c| (c.x, c.y))
    }

    /// 是否有未保存的修改。来源：`edit.rs:719-729`。
    ///
    /// 判据不是"栈非空"，而是**当前位置 ≠ 保存点**；并且**笔画没结算时也算脏**
    /// （窗口失焦/拖动被打断时可能收不到"松开"事件，此时字节已写进内存却没有对应的撤销步
    /// —— 若脏标记仍是假，关窗就不会提示，那些字节会静默丢掉）。
    pub fn dirty(&self) -> bool {
        self.pos() != self.saved_pos || !self.stroke.is_empty()
    }

    // ── 状态变更 ──

    /// 标记「已保存到磁盘」：把保存点挪到当前位置。来源：`edit.rs:837-839`。
    pub fn mark_saved(&mut self) {
        self.saved_pos = self.pos();
    }

    /// 开始一次拖动。来源：`edit.rs:855-862`。
    ///
    /// 上一笔若还没结算（窗口失焦、系统吞了"松开"事件），**先把它结算成一步**再开新的一笔 ——
    /// 否则那批字节既撤销不了、也不会出现在撤销栈里，成了孤儿。
    pub fn begin(&mut self) {
        if !self.stroke.is_empty() {
            self.end();
        }
        self.stroking = true;
        self.stroke.clear();
        self.stroke_seen.clear();
    }

    /// 标记"这一格本次拖动已经写过了"；返回 `true` 表示**第一次**（可以继续落笔）。
    /// 来源：`edit.rs:878-880`。
    ///
    /// ⚠ 顺序有讲究：判重必须在 `paint` **之前**做（与 GUI 一致）—— 这样"被拒绝的格"
    /// 也算"本笔已经试过"，同一笔里不会对同一格反复报同一条拒绝原因。
    pub fn mark_seen(&mut self, x: u16, y: u16) -> bool {
        self.stroke_seen.insert((x, y))
    }

    /// 在当前笔画里记一格变更（调用方已确认 `before != after` 且已写进地图）。
    /// 来源：`edit.rs:911-916`。
    pub fn push(&mut self, c: Change) {
        self.stroke.push(c);
    }

    /// 结束一次拖动：把累积变更压成**一步**；返回本步改了几格。来源：`edit.rs:940-960`。
    pub fn end(&mut self) -> usize {
        self.stroking = false;
        self.stroke_seen.clear();
        let changes = std::mem::take(&mut self.stroke);
        if changes.is_empty() {
            return 0;
        }
        let n = changes.len();
        self.seq += 1;
        // 新的一步会让"可重做"作废（否则重做会把另一条分支的内容贴回来）
        self.redo.clear();
        self.undo.push(Step {
            seq: self.seq,
            changes,
        });
        while self.undo.len() > UNDO_LIMIT {
            // 丢最老的一步；`saved_pos` 可能因此变得不可达 ⇒ `dirty` 恒为真（保守但不会骗人）
            self.undo.remove(0);
        }
        n
    }

    /// 把**一整批**变更压成一步（填充用；"整次填充 = 一步撤销"）。
    ///
    /// 与 GUI 的 `begin(); stroke = changes; end();` 等价（`edit.rs:1197-1199`）：
    /// 先结算上一笔（若有），再一次性入栈。
    pub fn commit(&mut self, changes: Vec<Change>) -> usize {
        if changes.is_empty() {
            return 0;
        }
        self.begin();
        self.stroke = changes;
        self.end()
    }

    /// 撤销一步；返回还原了几格（0 = 没有可撤销的步骤）。来源：`edit.rs:1289-1305`。
    ///
    /// 本方法**只管地图**（旧测试与纯地图调用方用它）；要连带维护派生量（五层计数）请用
    /// [`UndoStack::undo_with`]。
    pub fn undo(&mut self, map: &mut Map) -> usize {
        self.undo_with(map, |_, _, _, _| {})
    }

    /// 撤销一步，并在**每写回一格**之后回调 `on_write(x, y, 写前格, 写后格)`。
    ///
    /// 为什么要回调：五层计数（[`crate::layer_counts`]）是「格的派生量」，
    /// 撤销不经过 [`crate::Document::write_view_cell`]（那里要 `&mut Document`，
    /// 与这里要的 `&mut Map` 是同一块内存）⇒ 用「写一格报一格」的方式让调用方
    /// 在**同一处**把计数改回去。回调在**写完之后**调用，`写后格 == 该步的 before`。
    ///
    /// 返回还原了几格（越界格跳过、不回调 —— 与 `undo` 的原行为一致）。
    pub fn undo_with<F>(&mut self, map: &mut Map, mut on_write: F) -> usize
    where
        F: FnMut(u16, u16, Cell, Cell),
    {
        let Some(step) = self.undo.pop() else {
            return 0;
        };
        let mut n = 0;
        // 逆序还原：同一格在一步里只出现一次，这里的顺序只是为了让"后写的先退"
        for c in step.changes.iter().rev() {
            if let Some(slot) = cell_mut(map, c.x, c.y) {
                let prev = *slot;
                *slot = c.before;
                on_write(c.x, c.y, prev, c.before);
                n += 1;
            }
        }
        self.redo.push(step);
        n
    }

    /// 重做一步；返回重放了几格（0 = 没有可重做的步骤）。来源：`edit.rs:1308-1323`。
    ///
    /// 同 [`UndoStack::undo`]：只管地图；要连带更新计数请用 [`UndoStack::redo_with`]。
    pub fn redo(&mut self, map: &mut Map) -> usize {
        self.redo_with(map, |_, _, _, _| {})
    }

    /// 重做一步，并在每写回一格之后回调（语义同 [`UndoStack::undo_with`]）。
    pub fn redo_with<F>(&mut self, map: &mut Map, mut on_write: F) -> usize
    where
        F: FnMut(u16, u16, Cell, Cell),
    {
        let Some(step) = self.redo.pop() else {
            return 0;
        };
        let mut n = 0;
        for c in step.changes.iter() {
            if let Some(slot) = cell_mut(map, c.x, c.y) {
                let prev = *slot;
                *slot = c.after;
                on_write(c.x, c.y, prev, c.after);
                n += 1;
            }
        }
        self.undo.push(step);
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::造图;
    use mir2_map::CellFormat;

    fn 改一格(map: &mut Map, x: u16, y: u16, img: u16) -> Change {
        let before = *map.cell(x, y).unwrap();
        let mut after = before;
        after.bk_img = img;
        *cell_mut(map, x, y).unwrap() = after;
        Change {
            x,
            y,
            before,
            after,
        }
    }

    /// 一次拖动只占一层撤销；同格重复经过不再记第二次。来源：`edit.rs:2319-2341`。
    #[test]
    fn 一次拖动算一步() {
        let mut map = 造图(4, 4, CellFormat::New14);
        let orig = map.to_bytes();
        let mut u = UndoStack::new();
        u.begin();
        for (x, y) in [(1u16, 1u16), (2, 1), (3, 1)] {
            assert!(u.mark_seen(x, y), "第一次应放行");
            u.push(改一格(&mut map, x, y, 42));
        }
        // 同一格重复经过
        assert!(!u.mark_seen(3, 1), "同笔第二次必须被去重");
        assert_eq!(u.end(), 3, "本步应记 3 格");
        assert_eq!(u.undo_len(), 1, "一次拖动只占一层撤销");
        assert!(u.dirty());
        assert_eq!(u.undo(&mut map), 3);
        assert_eq!(map.to_bytes(), orig, "撤销一次应逐字节回到原样");
        assert_eq!(u.redo_len(), 1);
        assert!(!u.dirty(), "撤销回保存点后不该脏");
    }

    /// 新的一步清空重做栈。来源：`edit.rs:2427-2439`。
    #[test]
    fn 新的一步清空重做栈() {
        let mut map = 造图(3, 3, CellFormat::Old12);
        let mut u = UndoStack::new();
        u.commit(vec![改一格(&mut map, 1, 0, 11)]);
        u.undo(&mut map);
        assert_eq!(u.redo_len(), 1);
        u.commit(vec![改一格(&mut map, 2, 0, 22)]);
        assert_eq!(u.redo_len(), 0, "新的一步应让可重做作废");
    }

    /// 撤销栈上限 100 步：第 101 步把最老的一步挤掉。来源：`edit.rs:2410-2423`。
    #[test]
    fn 撤销栈上限一百步() {
        let mut map = 造图(2, 2, CellFormat::Old12);
        let mut u = UndoStack::new();
        for i in 0..(UNDO_LIMIT + 20) {
            let (x, y) = if i % 2 == 0 { (0u16, 1u16) } else { (1, 1) };
            let img = (i as u16 % 7) + 2;
            u.commit(vec![改一格(&mut map, x, y, img)]);
        }
        assert_eq!(u.undo_len(), UNDO_LIMIT, "撤销栈只保留最近 100 步");
    }

    /// 空的一步不产生撤销层（"空刷一遍"不许污染栈）。来源：`edit.rs:2443-2453`。
    #[test]
    fn 空步不入栈() {
        let mut u = UndoStack::new();
        u.begin();
        assert_eq!(u.end(), 0);
        assert_eq!(u.undo_len(), 0);
        assert!(!u.dirty());
        assert_eq!(u.commit(Vec::new()), 0);
        assert_eq!(u.undo_len(), 0);
    }

    /// 笔画没结算时也算脏；结算后才有撤销步。来源：`edit.rs:2458-2470`。
    #[test]
    fn 未结算的笔画也算脏() {
        let mut map = 造图(3, 3, CellFormat::New14);
        let mut u = UndoStack::new();
        assert!(!u.dirty());
        u.begin();
        assert!(u.mark_seen(2, 2));
        u.push(改一格(&mut map, 2, 2, 77));
        assert!(u.dirty(), "笔画还没结算也必须算脏");
        assert_eq!(u.undo_len(), 0, "还没结算就没有撤销步");
        assert!(u.stroke_active() && u.stroke_pending());
        assert_eq!(u.end(), 1, "结算后才算一步");
        assert_eq!(u.undo_len(), 1);
        assert!(!u.stroke_active() && !u.stroke_pending());
    }

    /// 重开一笔会先把上一笔结算掉（那批字节不会变成孤儿）。来源：`edit.rs:2474-2493`。
    #[test]
    fn 重开一笔先结算上一笔() {
        let mut map = 造图(3, 3, CellFormat::New14);
        let mut u = UndoStack::new();
        u.begin();
        u.mark_seen(1, 1);
        u.push(改一格(&mut map, 1, 1, 88));
        // 没调 end 就开新的一笔
        u.begin();
        assert_eq!(u.undo_len(), 1, "上一笔必须被自动结算");
        u.mark_seen(2, 2);
        u.push(改一格(&mut map, 2, 2, 99));
        assert_eq!(u.end(), 1);
        assert_eq!(u.undo_len(), 2);
        assert_eq!(u.undo(&mut map), 1);
        assert_eq!(u.undo(&mut map), 1);
        assert!(!u.dirty(), "撤销回原样后不该脏");
    }

    /// 保存点：`mark_saved` 后干净，再撤销又脏，重做回来又干净。来源：`edit.rs:2376-2394`。
    #[test]
    fn 保存点随撤销重做变化() {
        let mut map = 造图(2, 2, CellFormat::Old12);
        let mut u = UndoStack::new();
        u.commit(vec![改一格(&mut map, 1, 1, 3)]);
        assert!(u.dirty());
        u.mark_saved();
        assert_eq!(u.saved_pos(), u.pos());
        assert!(!u.dirty(), "保存后必须变干净");
        u.undo(&mut map);
        assert!(u.dirty(), "撤销到保存点之前又该脏");
        u.redo(&mut map);
        assert!(!u.dirty(), "重做回保存点应重新变干净");
    }

    /// `undo_with` / `redo_with`：每写回一格回调一次，且「写前 / 写后」两个格值必须是**真值**。
    ///
    /// 五层计数（[`crate::layer_counts`]）就是靠这条回调同步的：撤销要把计数按
    /// 「写前格 → 写后格」的掩码差改回去。回调值一旦错位，计数会静默跑偏。
    #[test]
    fn 撤销回调逐格报告前后值() {
        let mut map = 造图(2, 2, CellFormat::New14);
        let before = *map.cell(1, 0).unwrap();
        let mut after = before;
        after.bk_img = 0x8005;
        // 先把地图写成「改后值」（撤销要把它退回 before ⇒ 回调的写前值 = after）
        *cell_mut(&mut map, 1, 0).unwrap() = after;
        let mut u = UndoStack::new();
        u.commit(vec![Change {
            x: 1,
            y: 0,
            before,
            after,
        }]);

        let mut 撤销轨迹 = Vec::new();
        assert_eq!(
            u.undo_with(&mut map, |x, y, prev, next| {
                撤销轨迹.push((x, y, prev, next));
            }),
            1
        );
        assert_eq!(
            撤销轨迹,
            vec![(1u16, 0u16, after, before)],
            "撤销：写前 = 改后值，写后 = 改前值"
        );

        let mut 重做轨迹 = Vec::new();
        assert_eq!(
            u.redo_with(&mut map, |x, y, prev, next| {
                重做轨迹.push((x, y, prev, next));
            }),
            1
        );
        assert_eq!(
            重做轨迹,
            vec![(1u16, 0u16, before, after)],
            "重做：写前 = 改前值，写后 = 改后值"
        );
    }

    /// 整格快照：撤销能还原**任何一层**（图号 + 库号 + 位）。
    #[test]
    fn 整格快照能还原所有字段() {
        let mut map = 造图(2, 2, CellFormat::Ext36);
        let before = *map.cell(1, 1).unwrap();
        let mut u = UndoStack::new();
        let mut after = before;
        after.bk_img = 0x8123;
        after.mid_img = 0x4567;
        after.fr_img = 0x89AB;
        after.tiles = 0xEE;
        after.sm_tiles = 0xDD;
        after.area = 0xCC;
        after.light = 0xBB;
        after.tail[5] = 0xAA;
        *cell_mut(&mut map, 1, 1).unwrap() = after;
        u.commit(vec![Change {
            x: 1,
            y: 1,
            before,
            after,
        }]);
        assert_eq!(u.undo(&mut map), 1);
        assert_eq!(*map.cell(1, 1).unwrap(), before, "整格必须逐字段还原");
    }
}
