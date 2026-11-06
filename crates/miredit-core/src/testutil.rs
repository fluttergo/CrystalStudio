//! **仅测试用**的合成夹具（`#[cfg(test)]`，不进产物）。
//!
//! 为什么单独一个模块：`document` / `paint` / `fill` / `session` / `undo` 的单元测试都要
//! "一张每格可区分的合成图"，各写一份会出现 5 个略有差异的夹具（然后断言就不可信了）。
//! 这里只有一份，口径与 `miredit-gui/src/edit.rs:2094-2119` 的测试夹具**逐字段相同**：
//! 图号 = 格序 + 1、中层 +100、物件 +200、门索引 7、光照 9、14/36 档库号 3/4、尾区 `0xA0+j`。

use std::path::Path;

use mir2_map::{CellFormat, Map};

use crate::document::Document;

/// 造一张 `w*h` 的合成图（每格内容按列优先格序可区分）。
pub(crate) fn 造图(w: u16, h: u16, fmt: CellFormat) -> Map {
    let size = fmt.cell_size();
    let mut bytes = vec![0u8; mir2_map::HEADER_LEN + size * (w as usize * h as usize)];
    bytes[0..2].copy_from_slice(&w.to_le_bytes());
    bytes[2..4].copy_from_slice(&h.to_le_bytes());
    for i in 0..(w as usize * h as usize) {
        let base = mir2_map::HEADER_LEN + i * size;
        // 大图上 i 会超过 u16（如 500×500 = 250,000 格）⇒ 用 wrapping 算术：
        // 小图上取值与 GUI 夹具完全一致，大图上只求"每格可区分、不溢出"。
        let img = (i as u16).wrapping_add(1);
        bytes[base..base + 2].copy_from_slice(&img.to_le_bytes());
        bytes[base + 2..base + 4].copy_from_slice(&img.wrapping_add(100).to_le_bytes());
        bytes[base + 4..base + 6].copy_from_slice(&img.wrapping_add(200).to_le_bytes());
        bytes[base + 6] = 7; // 门索引
        bytes[base + 11] = 9; // 光照
        if fmt.has_lib_bytes() {
            bytes[base + 12] = 3; // btTiles
            bytes[base + 13] = 4; // btsmTiles
        }
        if fmt.has_tail() {
            for (j, b) in bytes[base + 14..base + 36].iter_mut().enumerate() {
                *b = 0xA0 + j as u8;
            }
        }
    }
    Map::parse(&bytes).expect("合成地图应能解析")
}

/// 把一张合成图包成 [`Document`]（路径只是诊断用的假路径，不落盘）。
pub(crate) fn 造文档(w: u16, h: u16, fmt: CellFormat) -> Document {
    let map = 造图(w, h, fmt);
    Document::from_bytes(Path::new(r"I:\__miredit_core_synth__\synth.map"), map.to_bytes())
        .expect("合成图应能构造成文档")
}
