//! 帧缓存：把「同一 (库, 帧号) 的重复取帧」收敛成一次解码。
//!
//! # 为什么必须有
//!
//! `mir2_assets::SpriteSource::frame` 的契约是**每次都返回一份新解码的像素**
//! （`wil_source.rs:154-173` 直接 `slot.lib.decode_frame(...)`，帧级无缓存）。
//! 而一张 64×48 格的窗口里，地表只有 768 个偶偶格、中层/表层各最多 3072 格，
//! 其中大量格引用的是**同一个帧号**（0.map 地表实际只用少数几个帧号）。
//! 不缓存的话同一帧会被重复解码上千次 —— 纯浪费，且把「渲染」的耗时算到解码头上。
//!
//! # 只做缓存，不改变事实
//!
//! - 缓存键是 `(LibRef, 帧号)`，值可能是 `None`（**缺帧也要缓存**，避免对同一个缺帧
//!   反复打库）—— `None` 就是 `None`，绝不用缓存把「取不到」变成「有图」；
//! - 缓存**不影响输出字节**：命中与否只影响 `frames_decoded` 计数与耗时；
//! - 超出条目上限（[`MAX_CACHE_ENTRIES`]）后**停止新增**（已有条目继续用）。
//!   这是为了防「大窗口 × 变尺寸表层」把内存顶爆；停缓存只损失速度，不损失正确性。

use std::collections::HashMap;
use std::rc::Rc;

use mir2_assets::{Frame, LibRef, SpriteSource};

/// 缓存的 (库, 帧号) 条目上限。
///
/// 一帧 96×64 RGBA = 24 KB；8192 条最坏约 200 MB。选这个量级是为了覆盖
/// 「64×48 格窗口 / 160×120 格窗口」的实际去重规模（地表帧号种类通常只有几十个）。
pub(crate) const MAX_CACHE_ENTRIES: usize = 8192;

/// 帧缓存（持有一个 `&dyn SpriteSource`）。
pub(crate) struct FrameCache<'a> {
    src: &'a dyn SpriteSource,
    slots: HashMap<(LibRef, u32), Option<Rc<Frame>>>,
    decoded: u32,
}

impl<'a> FrameCache<'a> {
    /// 包一层缓存（本身不预取任何帧）。
    pub(crate) fn new(src: &'a dyn SpriteSource) -> Self {
        FrameCache {
            src,
            slots: HashMap::new(),
            decoded: 0,
        }
    }

    /// 取帧（0-based 帧号）；不可得返回 `None`。
    ///
    /// 命中缓存直接返回；未命中才真正问图库，并在取到时把 `frames_decoded` +1。
    pub(crate) fn get(&mut self, lib: LibRef, index: u32) -> Option<Rc<Frame>> {
        if let Some(hit) = self.slots.get(&(lib, index)) {
            return hit.clone();
        }
        let got = self.src.frame(lib, index).map(Rc::new);
        if got.is_some() {
            self.decoded += 1;
        }
        if self.slots.len() < MAX_CACHE_ENTRIES {
            self.slots.insert((lib, index), got.clone());
        }
        got
    }

    /// 实际发生过的成功解码次数（= 缓存未命中且取到帧的次数）。
    pub(crate) fn decoded(&self) -> u32 {
        self.decoded
    }
}
