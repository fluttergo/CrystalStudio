//! 读客户端烘焙的图集 meta + PNG（只读，绝不写 `GodotClient/assets`）。

use std::path::Path;

use super::json_min;

/// meta 里的一帧（只取比对需要的字段）。
#[derive(Debug, Clone)]
pub struct MetaFrame {
    /// 0-based 帧号（与 [`mir2_assets::SpriteSource::frame`] 的 `index` 同义）。
    pub idx: u32,
    /// 帧宽。
    pub w: u32,
    /// 帧高。
    pub h: u32,
    /// 锚点偏移 `[px, py]`（对应帧头那对 i16；**可为负**）。
    ///
    /// 与 [`mir2_assets::Frame::anchor`] 是同一个值（上游图集烘焙工具 `wil2atlas` 的
    /// `meta.rs:27` 落盘；本仓只 vendor 了它的解码器，不含该文件；
    /// 本 crate 的 `WilSource` 从帧头搬）。meta 里缺该字段时为 `None`，
    /// 由调用方当「meta 不可用于锚点对账」处理，**不**当 `(0,0)`。
    pub anchor: Option<[i16; 2]>,
    /// 所在图集序号（无效帧为 `None`）。
    pub atlas: Option<usize>,
    /// 图集内矩形 `[x, y, w, h]`（无效帧为 `None`）。
    pub region: Option<[u32; 4]>,
}

/// 一个库的图集元数据。
#[derive(Debug, Clone)]
pub struct AtlasMeta {
    /// 烘焙时用的调色板来源（`file` / `client`）。
    pub palette: String,
    /// 文件调色板与客户端硬编码表的不一致项数（`palette=file` 时的诊断值）。
    pub palette_mismatch: u64,
    /// 图集 PNG 文件名（按 `atlas` 序号索引）。
    pub atlas_files: Vec<String>,
    /// 全部帧。
    pub frames: Vec<MetaFrame>,
}

/// 读并解析 `<库>_atlas_meta.json`。
pub fn load_atlas_meta(path: &Path) -> Result<AtlasMeta, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("读 meta 失败：{}（{e}）", path.display()))?;
    let root = json_min::parse(&text).map_err(|e| format!("{} 解析失败：{e}", path.display()))?;

    let palette = root
        .get("palette")
        .and_then(|v| v.as_str())
        .unwrap_or("<缺失>")
        .to_string();
    let palette_mismatch = root
        .get("palette_mismatch")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let mut atlas_files: Vec<String> = Vec::new();
    if let Some(list) = root
        .get("atlas")
        .and_then(|a| a.get("atlases"))
        .and_then(|a| a.as_arr())
    {
        for item in list {
            let file = item
                .get("file")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "atlas.atlases[i] 缺 file".to_string())?;
            atlas_files.push(file.to_string());
        }
    }

    let frames_json = root
        .get("frames")
        .and_then(|v| v.as_arr())
        .ok_or_else(|| format!("{} 缺 frames 数组", path.display()))?;

    let mut frames = Vec::with_capacity(frames_json.len());
    for f in frames_json {
        let idx = f
            .get("idx")
            .and_then(|v| v.as_u32())
            .ok_or_else(|| "frames[i] 缺 idx".to_string())?;
        let w = f
            .get("w")
            .and_then(|v| v.as_u32())
            .ok_or_else(|| format!("帧 {idx} 缺 w"))?;
        let h = f
            .get("h")
            .and_then(|v| v.as_u32())
            .ok_or_else(|| format!("帧 {idx} 缺 h"))?;
        let atlas = f.get("atlas").and_then(|v| v.as_usize());
        // `anchor: [px, py]`（上游 `wil2atlas` 的 `meta.rs:27` 写的字段，该文件未 vendor；可为负）。
        // 缺字段 / 不是 2 元数组 / 不是整数 ⇒ `None`（由调用方判「meta 不可用」）。
        let anchor = f.get("anchor").and_then(|v| v.as_arr()).and_then(|a| {
            if a.len() != 2 {
                return None;
            }
            Some([a[0].as_i16()?, a[1].as_i16()?])
        });
        let region = f.get("region").and_then(|v| v.as_arr()).and_then(|a| {
            if a.len() != 4 {
                return None;
            }
            Some([
                a[0].as_u32()?,
                a[1].as_u32()?,
                a[2].as_u32()?,
                a[3].as_u32()?,
            ])
        });
        frames.push(MetaFrame {
            idx,
            w,
            h,
            anchor,
            atlas,
            region,
        });
    }

    Ok(AtlasMeta {
        palette,
        palette_mismatch,
        atlas_files,
        frames,
    })
}

/// 读 PNG 并转成行优先 RGBA8（`image` crate 负责解码，只读不写）。
pub fn decode_png_rgba(path: &Path) -> Result<(u32, u32, Vec<u8>), String> {
    let img = image::open(path).map_err(|e| format!("解码 PNG 失败：{}（{e}）", path.display()))?;
    let rgba = img.into_rgba8();
    let (w, h) = rgba.dimensions();
    Ok((w, h, rgba.into_raw()))
}
