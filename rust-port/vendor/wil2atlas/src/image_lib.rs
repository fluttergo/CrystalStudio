//! 图库统一接口：让 WIL/WIX 与 WZL/WZX 两种格式走**同一条**图集生成管线。
//!
//! `main.rs` 只面向 `dyn ImageLib`，格式差异全部封装在各自解码器内。

use anyhow::Result;

use crate::wil_decoder::{
    DecodedFrame, DecodedIndexFrame, FrameInfo, Palette, WilHeader, WilLib,
};
use crate::wzl_decoder::WzlLib;

/// 图库的公共读取接口（元信息 / 单帧解码）。
///
/// `Send + Sync` 是图集模式 `rayon::par_iter` 并行解码所必需（两个实现都只持有 `Vec<u8>`）。
pub trait ImageLib: Send + Sync {
    /// 文件头（WZL 为合成头，仅用于诊断打印）。
    fn header(&self) -> &WilHeader;
    /// 生效口令（WZL 恒 0）。
    fn effective_password(&self) -> i32;
    /// 是否老格式（WZL 恒 false）。
    fn old_format(&self) -> bool;
    /// 内嵌调色板（WZL 恒 None）。
    fn file_palette(&self) -> Option<&Palette>;
    /// 帧总数（= 索引表长度）。
    fn frame_count(&self) -> usize;
    /// 第 `index` 帧元信息（越界返回 None）。
    fn frame_info(&self, index: usize) -> Option<FrameInfo>;
    /// 解码第 `index` 帧为 RGBA8888。
    fn decode_frame(&self, index: usize, palette: &Palette) -> Result<DecodedFrame>;
    /// 尝试取第 `index` 帧的 8bit 索引（非 8bit 帧返回 Ok(None)）。
    fn decode_frame_indices_opt(&self, index: usize) -> Result<Option<DecodedIndexFrame>>;
}

impl ImageLib for WilLib {
    fn header(&self) -> &WilHeader {
        &self.header
    }
    fn effective_password(&self) -> i32 {
        self.effective_password
    }
    fn old_format(&self) -> bool {
        self.old_format
    }
    fn file_palette(&self) -> Option<&Palette> {
        self.file_palette.as_ref()
    }
    fn frame_count(&self) -> usize {
        self.offsets.len()
    }
    fn frame_info(&self, index: usize) -> Option<FrameInfo> {
        WilLib::frame_info(self, index)
    }
    fn decode_frame(&self, index: usize, palette: &Palette) -> Result<DecodedFrame> {
        WilLib::decode_frame(self, index, palette)
    }
    fn decode_frame_indices_opt(&self, index: usize) -> Result<Option<DecodedIndexFrame>> {
        WilLib::decode_frame_indices_opt(self, index)
    }
}

impl ImageLib for WzlLib {
    fn header(&self) -> &WilHeader {
        &self.header
    }
    fn effective_password(&self) -> i32 {
        0
    }
    fn old_format(&self) -> bool {
        false
    }
    fn file_palette(&self) -> Option<&Palette> {
        self.file_palette.as_ref()
    }
    fn frame_count(&self) -> usize {
        self.offsets.len()
    }
    fn frame_info(&self, index: usize) -> Option<FrameInfo> {
        WzlLib::frame_info(self, index)
    }
    fn decode_frame(&self, index: usize, palette: &Palette) -> Result<DecodedFrame> {
        WzlLib::decode_frame(self, index, palette)
    }
    fn decode_frame_indices_opt(&self, index: usize) -> Result<Option<DecodedIndexFrame>> {
        WzlLib::decode_frame_indices_opt(self, index)
    }
}
