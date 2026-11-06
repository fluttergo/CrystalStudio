//! 取证与自动化：截图落盘、与客户端同视点分块比对、单元格探针。
//!
//! 这三件事都需要"窗口已经在跑"或"图已经在手"，放在 app 里会让 `update()` 长到没法读，
//! 单独成模块后每一件都能被单独调用与测试。
//!
//! # 一、截图怎么来的
//! 走 egui 自己的通道：`ViewportCommand::Screenshot` 请求一帧 → 下一帧从
//! `Event::Screenshot { image, .. }` 拿到 [`egui::ColorImage`]（1500×950，与窗口客户区一致）。
//! **不用系统截屏 API**：那样会带上窗口边框/被其它窗口遮挡，尺寸也不再是 1500×950。
//!
//! # 二、同视点比对的口径（任务书 §五.2）
//! 客户端那张图是**相机取景**，我们是**视口取景**，两者世界像素矩形不同 ⇒
//! 先各自算出"世界像素矩形"，取**交集**，再从各自图里裁出同一块世界像素做 16×16 分块比对。
//!
//! 客户端的相机算式（与 `GodotClient/tools/map_preview.gd:82,103-104` 完全一致）：
//!
//! ```text
//! center_tile = (at.x / 2, at.y / 2)        # 整数除（GDScript 的 int/int）
//! cam.position = (center_tile.x*96 + 48, center_tile.y*64 + 32)   # 世界像素
//! 屏幕左上角世界像素 = cam.position - 视口尺寸/2                    # 1280×720
//! ```
//!
//! 我们这边：`视口左上角世界像素 = (pan_x, pan_y)`，尺寸 = `画布矩形 / scale`。
//! 只有 `scale == 1` 时才 1:1 可比（否则要缩放我们的图，会把"地形结构对不对"
//! 和"缩放插值糊不糊"混在一起），故本函数**只接受 1×**。

use std::path::Path;

use egui::Rect;
use mir2_assets::SpriteSource;
use mir2_map::CellFormat;
use miredit_render::compare::{compare_blocks, CompareOptions};
use miredit_render::png_io::{read_png, write_rgba_png, PngImage};

use crate::canvas_view::CanvasView;
use crate::cellinfo;
use crate::mapdoc::DocState;
use crate::ui_text as tx;

/// 一次同视点比对的结果。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gate2 {
    /// 命中块数。
    pub hit: u32,
    /// 总块数。
    pub total: u32,
    /// 命中率（0..=1）。
    pub rate: f64,
}

/// 把 egui 的截图写成 PNG。
///
/// `ColorImage.pixels` 是 **premultiplied** Color32；不透明像素（窗口截图恒为不透明）
/// 直接取分量即可，但这里仍按 alpha 反预乘一次，避免将来带透明背景时写出偏暗的图。
pub fn save_screenshot(img: &egui::ColorImage, path: &Path) -> Result<(), String> {
    let (w, h) = (img.size[0] as u32, img.size[1] as u32);
    let mut rgba = Vec::with_capacity(img.pixels.len() * 4);
    for px in &img.pixels {
        let c = px.to_array();
        let a = c[3];
        if a == 0 || a == 255 {
            rgba.extend_from_slice(&c);
        } else {
            let un = |v: u8| -> u8 { ((u32::from(v) * 255 + u32::from(a) / 2) / u32::from(a)) as u8 };
            rgba.extend_from_slice(&[un(c[0]), un(c[1]), un(c[2]), a]);
        }
    }
    write_rgba_png(path, w, h, &rgba).map_err(|e| e.to_string())?;
    eprintln!(
        "{}",
        tx::fill(
            tx::DIAG_SHOT_OK,
            &[
                ("path", &path.display().to_string()),
                ("w", &w.to_string()),
                ("h", &h.to_string()),
            ]
        )
    );
    Ok(())
}

/// 同视点分块比对：`shot` 是本机窗口截图，`canvas` 是画布矩形，`view` 是当前视口。
///
/// 成功时打印两行诊断（几何 + 命中率）并把两张对齐后的裁剪图写到截图旁边
/// （`<shot>.ours.png` / `<shot>.client.png`），供人眼复核。
pub fn gate2(
    shot: &PngImage,
    canvas: Rect,
    overlay: Rect,
    view: &CanvasView,
    client_path: &Path,
    at: (i32, i32),
    shot_path: &Path,
) -> Result<Gate2, String> {
    if (view.scale - 1.0).abs() > 1e-6 {
        return Err(format!("同视点比对只支持 1×（当前 {}）", view.scale));
    }
    let client = read_png(client_path).map_err(|e| e.to_string())?;

    // ① 客户端的世界像素矩形（照抄 map_preview.gd 的相机算式）
    let fx = at.0 / 2; // 整数除
    let fy = at.1 / 2;
    let cam_x = fx * 96 + 48;
    let cam_y = fy * 64 + 32;
    let cox = cam_x - client.width as i32 / 2;
    let coy = cam_y - client.height as i32 / 2;

    // ② 我们的世界像素矩形
    let oox = view.pan_x.round() as i32;
    let ooy = view.pan_y.round() as i32;
    let ow = (canvas.width() / view.scale).round() as i32;
    let oh = (canvas.height() / view.scale).round() as i32;

    // ③ 交集
    let ix0 = cox.max(oox);
    let iy0 = coy.max(ooy);
    let ix1 = (cox + client.width as i32).min(oox + ow);
    let iy1 = (coy + client.height as i32).min(ooy + oh);
    if ix1 <= ix0 || iy1 <= iy0 {
        return Err(format!(
            "两者世界像素矩形不相交：客户端 x[{cox}..{}) y[{coy}..{})，本机 x[{oox}..{}) y[{ooy}..{})",
            cox + client.width as i32,
            coy + client.height as i32,
            oox + ow,
            ooy + oh
        ));
    }
    let iw = (ix1 - ix0) as u32;
    let ih = (iy1 - iy0) as u32;

    // ④ 各自裁同一块世界像素
    let sx = canvas.left() + (ix0 as f32 - view.pan_x) * view.scale;
    let sy = canvas.top() + (iy0 as f32 - view.pan_y) * view.scale;
    let ours = shot.crop(sx.round() as u32, sy.round() as u32, iw, ih);
    let theirs = client.crop((ix0 - cox) as u32, (iy0 - coy) as u32, iw, ih);

    let bg = corner_bg(&client);
    eprintln!(
        "{}",
        tx::fill(
            tx::DIAG_GATE2,
            &[
                ("w", &ours.width.to_string()),
                ("h", &ours.height.to_string()),
                ("cw", &theirs.width.to_string()),
                ("ch", &theirs.height.to_string()),
                ("iw", &iw.to_string()),
                ("ih", &ih.to_string()),
                ("r", &bg[0].to_string()),
                ("g", &bg[1].to_string()),
                ("b", &bg[2].to_string()),
            ]
        )
    );

    let opts = CompareOptions {
        block: 16,
        threshold: 32,
        background: bg,
    };
    let rep = compare_blocks(&ours, &theirs, &opts);
    eprintln!(
        "{}",
        tx::fill(
            tx::DIAG_GATE2_RESULT,
            &[
                ("hit", &rep.hit.to_string()),
                ("total", &rep.total.to_string()),
                ("rate", &format!("{:.2}", rep.hit_rate * 100.0)),
                ("threshold", &opts.threshold.to_string()),
                ("block", &opts.block.to_string()),
            ]
        )
    );
    eprintln!(
        "        块色差 平均 {:.1} / 中位 {} / 最大 {}",
        rep.mean_delta, rep.median_delta, rep.max_delta
    );

    // ④b 补充口径：把画布浮层（缩放浮层）整条排除后再比一次
    //
    // 任务书 §五.2 的口径是"画布里只多了页签条和浮层，取纯地图区域比"。
    // 缩放浮层固定压在画布左下角，所以"排除浮层"= 只比它上沿以上那一段。
    let crop_y = sy.round();
    let keep_h = (overlay.top() - crop_y).clamp(0.0, ih as f32) as u32;
    if keep_h >= 16 {
        let ours2 = shot.crop(sx.round() as u32, crop_y as u32, iw, keep_h);
        let theirs2 = client.crop((ix0 - cox) as u32, (iy0 - coy) as u32, iw, keep_h);
        let rep2 = compare_blocks(&ours2, &theirs2, &opts);
        eprintln!(
            "{}",
            tx::fill(
                tx::DIAG_GATE2_OVERLAY,
                &[
                    ("ph", &(ih - keep_h).to_string()),
                    ("hit", &rep2.hit.to_string()),
                    ("total", &rep2.total.to_string()),
                    ("rate", &format!("{:.2}", rep2.hit_rate * 100.0)),
                ]
            )
        );
    }

    // ⑤ 证据落盘（截图旁边，便于人眼复核"是不是同一块地")
    let ours_p = with_suffix(shot_path, "-crop-ours");
    let theirs_p = with_suffix(shot_path, "-crop-client");
    if let Err(e) = ours.save_png(&ours_p) {
        eprintln!("[门2] 对齐裁剪图（本机）导出失败：{e}");
    }
    if let Err(e) = theirs.save_png(&theirs_p) {
        eprintln!("[门2] 对齐裁剪图（客户端）导出失败：{e}");
    }

    Ok(Gate2 {
        hit: rep.hit,
        total: rep.total,
        rate: rep.hit_rate,
    })
}

/// `a.png` + `-x` ⇒ `a-x.png`
fn with_suffix(p: &Path, suffix: &str) -> std::path::PathBuf {
    let stem = p
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "shot".to_string());
    let dir = p.parent().map(Path::to_path_buf).unwrap_or_default();
    dir.join(format!("{stem}{suffix}.png"))
}

/// 四角像素取众数当"客户端清屏底色"（与 `miredit-render` 验收门 4 的口径一致）。
fn corner_bg(img: &PngImage) -> [u8; 3] {
    let pts = [
        (0u32, 0u32),
        (img.width.saturating_sub(1), 0),
        (0, img.height.saturating_sub(1)),
        (
            img.width.saturating_sub(1),
            img.height.saturating_sub(1),
        ),
    ];
    let mut seen: Vec<([u8; 3], u32)> = Vec::new();
    for (x, y) in pts {
        if let Some(p) = img.pixel(x, y) {
            let c = [p[0], p[1], p[2]];
            match seen.iter_mut().find(|(k, _)| *k == c) {
                Some((_, n)) => *n += 1,
                None => seen.push((c, 1)),
            }
        }
    }
    seen.into_iter()
        .max_by_key(|(_, n)| *n)
        .map(|(c, _)| c)
        .unwrap_or([0, 0, 0])
}

/// 渲染转储：**不开窗**，直接用 `miredit_render::try_render_region_with_options`
/// 渲一块窗口并写 PNG。
///
/// 用途：把「渲染库的锅」和「界面（贴图/裁剪/平移）的锅」分开 ——
/// 同一块世界格窗口，本函数出的图若与客户端一致、而界面截图不一致，
/// 那就一定是贴图环节的问题，不用去怀疑合成规则。
///
/// `overlay` = 阻挡标记通道（缺省 [`miredit_render::CollisionOverlay::none`]）。
/// 验收"开通道 → 出现标记 / 关通道 → 逐字节回到原图"就是拿它跑两遍、比 PNG 字节。
///
/// `options` = **整份渲染选项**（`--open-doors` + `--lighting`，判据只有一处
/// [`crate::cli::Cli::export_options`]）。缺省 [`miredit_render::RenderOptions::default()`]
/// = 闭门 + 白天 ⇒ 输出与接线前**逐字节相同**；夜晚档叠全屏黑遮罩、开门档改前景层帧号。
/// 这也是"两个 option 同时生效"的**离屏取证口**：同一条命令跑多遍、比 PNG 像素与 sha256。
pub fn dump_render(
    doc: &DocState,
    origin: (i32, i32),
    cells: (u32, u32),
    mask: &miredit_render::LayerMask,
    overlay: miredit_render::CollisionOverlay,
    options: miredit_render::RenderOptions,
    path: &Path,
) -> Result<(), String> {
    let d = doc.doc.as_ref().ok_or_else(|| "没有打开地图".to_string())?;
    let src = doc.assets.source().ok_or_else(|| "没有图库".to_string())?;
    let vp = miredit_render::Viewport::new(origin.0, origin.1, cells.0, cells.1);
    let out = miredit_render::try_render_region_with_options(
        &d.map,
        src as &dyn SpriteSource,
        &vp,
        mask,
        overlay,
        options,
    )
    .map_err(|e| e.to_string())?;
    write_rgba_png(path, out.width, out.height, &out.rgba).map_err(|e| e.to_string())?;
    eprintln!(
        "{}",
        tx::fill(
            tx::DIAG_DUMP,
            &[
                ("path", &path.display().to_string()),
                ("cw", &cells.0.to_string()),
                ("ch", &cells.1.to_string()),
                ("pw", &out.width.to_string()),
                ("ph", &out.height.to_string()),
                ("ox", &origin.0.to_string()),
                ("oy", &origin.1.to_string()),
                ("doors", if options.doors_open { "1" } else { "0" }),
                ("lighting", &format!("{:?}", options.lighting)),
                ("stats", &out.stats.to_string()),
            ]
        )
    );
    Ok(())
}

/// 单元格探针：**不开窗**，把该格的六行信息打到 stderr（与面板同一套取数）。
///
/// 额外打印 `walkable=`（`mir2_map::Map::walkable` 的**真实返回**）：阻挡位改得对不对，
/// 最终判据是客户端 `CanWalk`（`TMap.cpp:339`）的等价实现，而不是"看着像"。
pub fn probe(doc: &DocState, x: u16, y: u16) {
    let (cell, fmt, walk) = match doc.doc.as_ref() {
        Some(d) => (
            d.map.cell(x, y),
            d.map.format,
            // 越界格 `walkable` 返回 false；但那种情况下 `cell` 也是 None，两处口径一致
            d.map.walkable(x, y),
        ),
        None => (None, CellFormat::Old12, false),
    };
    let src = doc.assets.source().map(|s| s as &dyn SpriteSource);
    let lines = cellinfo::cell_lines(x, y, cell, fmt, src);
    eprintln!(
        "{}",
        tx::fill(
            tx::DIAG_PROBE,
            &[
                ("head", &lines.head),
                ("collision", &lines.values[0].replace('\n', " ")),
                ("walk", if walk { "1" } else { "0" }),
                ("ground", &lines.values[1].replace('\n', " ")),
                ("middle", &lines.values[2].replace('\n', " ")),
                ("objects", &lines.values[3].replace('\n', " ")),
                ("meta", &lines.values[4].replace('\n', " ")),
                ("anim", &lines.values[5].replace('\n', " ")),
            ]
        )
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 截图保存：造一张 2×2 的 ColorImage，写出来的 PNG 必须能被读回（同尺寸同像素）。
    #[test]
    fn 截图往返() {
        let img = egui::ColorImage {
            size: [2, 2],
            source_size: egui::vec2(2.0, 2.0),
            pixels: vec![
                egui::Color32::from_rgb(1, 2, 3),
                egui::Color32::from_rgb(4, 5, 6),
                egui::Color32::from_rgb(7, 8, 9),
                egui::Color32::from_rgb(10, 11, 12),
            ],
        };
        let dir = std::env::temp_dir();
        let p = dir.join("miredit_gui_unit_shot.png");
        save_screenshot(&img, &p).expect("写 PNG 不该失败");
        let back = read_png(&p).expect("读回 PNG");
        assert_eq!((back.width, back.height), (2, 2));
        assert_eq!(&back.rgba[0..4], &[1, 2, 3, 255]);
        assert_eq!(&back.rgba[12..16], &[10, 11, 12, 255]);
        let _ = std::fs::remove_file(&p);
    }

    /// 四角众数：四角同色时就是它；不同色时取出现次数最多的。
    #[test]
    fn 四角底色取众数() {
        let mut rgba = vec![0u8; 3 * 2 * 4];
        // 四角：三个 (9,9,9)，一个 (200,1,1)
        let set = |rgba: &mut Vec<u8>, x: usize, y: usize, c: [u8; 4]| {
            let i = (y * 3 + x) * 4;
            rgba[i..i + 4].copy_from_slice(&c);
        };
        set(&mut rgba, 0, 0, [9, 9, 9, 255]);
        set(&mut rgba, 2, 0, [9, 9, 9, 255]);
        set(&mut rgba, 0, 1, [200, 1, 1, 255]);
        set(&mut rgba, 2, 1, [9, 9, 9, 255]);
        let img = PngImage {
            width: 3,
            height: 2,
            rgba,
        };
        assert_eq!(corner_bg(&img), [9, 9, 9]);
    }

    /// 后缀拼接：`a.png` → `a-x.png`。
    #[test]
    fn 后缀拼接() {
        let p = Path::new(r"I:\tmp\shot.png");
        assert_eq!(
            with_suffix(p, "-crop-ours").to_string_lossy(),
            r"I:\tmp\shot-crop-ours.png"
        );
    }

    /// 非 1× 必须拒绝（否则会把缩放插值和地形正确性混在一起）。
    #[test]
    fn 非一倍缩放被拒绝() {
        let shot = PngImage {
            width: 4,
            height: 4,
            rgba: vec![0; 64],
        };
        let mut v = CanvasView::default();
        v.scale = 2.0;
        let e = gate2(
            &shot,
            Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(4.0, 4.0)),
            Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1.0, 1.0)),
            &v,
            Path::new(r"I:\__no_such_client__.png"),
            (0, 0),
            Path::new(r"I:\__no_such_shot__.png"),
        )
        .unwrap_err();
        assert!(e.contains("1×"), "{e}");
    }
}
