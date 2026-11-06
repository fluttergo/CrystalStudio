//! 印章**放置**：把资源面板里选中的那一枚 `.X` 真的贴到地图上。
//!
//! # 一、本模块只做「上层该做的事」，编辑语义一行都不重写
//!
//! 判据（锚点偏移 / 逐格越界裁剪 / 三族图号 / `bit15` 保留矩阵 / Type5 闸门 /
//! 整次放置 = 一步撤销）**全部**在 `miredit_core::stamp` 里，本模块只负责四件事：
//!
//! 1. **读盘**：`.X` 字节 → [`mir2_stamp::Stamp::parse`] → [`StampCellInput`] 数组；
//! 2. **注册**：`EditSession::register_stamp(id, cells)`（内核**不读盘**，格表必须由上层喂）；
//! 3. **落位**：`EditSession::place_stamp_by_id(id, x, y)`；
//! 4. **把结果折成一句中文**（成功回执 / 拒绝原因）与**待放置态**（[`Armed`]）。
//!
//! # 二、`.X → StampCellInput` 的适配**只有这一份**
//!
//! 面板缩略图那条路（`stamp_panel::thumbs::positioned_cells`）原先自己写了一遍
//! 字段搬运；现在它改为**调用本模块的 [`cells_of`]** 再掩掉 `bit15` 交给渲染库。
//! 两边各写一份的话，"面板里看到的图"与"贴到地上的图"迟早会不一样。
//!
//! 适配口径（与 `miredit-render/tests/stamp.rs` 的验收基准逐字段相同）：
//!
//! | 本模块字段 | `.X` 侧 | 处理 |
//! |---|---|---|
//! | `dx` / `dy` | `StampCell.x` / `.y`（`i32`，**可负**） | 原样 |
//! | `bk` | `back_image`（`i32`） | **取低 16 位**（`bk_img` 同宽；高位是 C# 的阻挡标记，本仓不解释） |
//! | `mid` | `middle_image`（`u16`） | 原样 |
//! | `fr` | `foreground.image`（`i16`） | `as u16`（负值按补码，随后由内核掩 `0x7FFF`） |
//! | `bk_lib` / `mid_lib` / `fr_lib` | 三个 `i16` 库号 | `u8::try_from`，**越界 ⇒ 整枚放弃**（绝不截断成别的库） |
//!
//! # 三、失败一律"地图一个字节都没动"
//!
//! 三道闸门的顺序是**读盘 → 注册 → 落位**，前两道失败时根本没有进入内核的写路径：
//!
//! | 失败 | 谁给原因 | 结果 |
//! |---|---|---|
//! | 文件读不出 / `.X` 解析失败 / 库号越界 | 本模块 [`read`] | 中文原因，一格未写 |
//! | 空格表 / 超过 100 万格 / 注册表满 | 内核 `register_stamp` | 中文原因，一格未写 |
//! | Type5 有地表 / 全越界 / 未注册 | 内核 `plan_stamp` | 中文原因，一格未写 |
//!
//! 第三道里"部分越界"**不是失败**：内核逐格跳过并在 `PlaceOutcome::Placed.skipped`
//! 里报数，本模块把它写进回执（[`tx::STAMP_PLACE_SKIP_TMPL`]）。
//!
//! # 四、`Alt` 悬停预览只画轮廓
//!
//! [`outline_rect`] 把「锚点 + 印章包围盒」算成屏幕矩形，画的时候只描边、**不填任何像素**
//! （任务书 §三.5）。所以它只吃**包围盒**这一项真实数据（来自
//! `StampCatalog::resolve` 的 `bounds`，与 `Stamp::bounds()` 同源），不碰图库、不渲染缩略图 ——
//! 没有帧也照样能画出正确的框，绝不会出现"框的位置对、里面是假的图"。

use std::path::Path;

use egui::Rect;
use mir2_map::Map;
use mir2_stamp::Stamp;

use miredit_core::{PlaceOutcome, StampCellInput};

use crate::canvas_view::CanvasView;
use crate::edit::EditSession;
use crate::ui_text as tx;

/// 一枚 `.X` 读盘后的**全部**结果：格表 + 包围盒。
///
/// 包围盒也一起返回，是因为待放置态的轮廓预览要用它（[`Armed::bounds`]），
/// 而 `resolve` 的缓存与本次读盘读的是同一个文件 —— 不必让调用方再查一次目录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StampFile {
    /// 格表（相对锚点的偏移 + 三族图号/库号）。
    pub cells: Vec<StampCellInput>,
    /// 包围盒 `(min_x, min_y, max_x, max_y)`，**闭区间**，与 `Stamp::bounds()` 同源。
    pub bounds: (i32, i32, i32, i32),
}

/// 印章**待放置轮廓**的输入：`(锚点, 包围盒)`。
///
/// 单列一个类型别名是为了让 [`crate::canvas::CanvasCtx`] 的字段与
/// [`crate::app::MireditApp`] 的取值函数共用同一个名字 —— 两处各写一遍
/// `((u16, u16), (i32, i32, i32, i32))` 既难读，也会让 clippy 的 `type_complexity` 各报一条。
pub type Outline = ((u16, u16), (i32, i32, i32, i32));

/// 待放置态：**已经选中一枚印章，正等着用户点画布落位**。
///
/// 为什么要有这个态（而不是"选中即放置"）：选中是**浏览**动作（面板里一枚枚看过去），
/// 放置是**破坏性**动作（写地图 + 占一步撤销）。两者必须分开，否则用户翻页看缩略图
/// 就会在图上到处留印章。所以：选中 → 按 `Enter` 进入本态 → 点画布才落位。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Armed {
    /// 印章 id（`register_stamp` 的键，也是 `palette.json` 的主键）。
    pub id: String,
    /// 显示名（回执里用；数据，不是文案）。
    pub name: String,
    /// 包围盒（`None` = 目录里 `resolve` 失败 ⇒ 预览不画框，落位时会报读盘失败）。
    pub bounds: Option<(i32, i32, i32, i32)>,
}

/// 放置成功的结果（内核 `PlaceOutcome::Placed` 的宿主侧镜像，只留两个数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placed {
    /// **真正改动**的格数（= 那一步撤销里的格数）。
    pub cells: usize,
    /// 越界被跳过的格数。
    pub skipped: usize,
}

/// `.X` 的格表 → 内核的 [`StampCellInput`] 数组（**全仓唯一一份适配**）。
///
/// 库号越界（超出 `u8`）⇒ `None`：**绝不截断**成另一个库号（那会取到别的图 = 假数据）。
/// 真实 9,159 枚里没有这种条目（`--stamp-probe` 的全库清点会报 `lib_overflow`）。
///
/// # 2026-11-05 补齐：门 / 光照 / 动画六项
///
/// `miredit-core` 的 `StampCellInput` 新增了 10 个字段（`door` / `door_offset` / `light` /
/// `ani_frame` / `ani_tick` / `mid_ani_frame` / `mid_ani_tick` / `tile_animation` /
/// `tile_offset` / `tile_frames`）。它们**在 `.X` 里全都有**（`mir2-stamp` 的
/// `StampCellData` 与 C# `Cell`（`Document.cs:7-32`）一一对应），所以这里**逐字段照搬**，
/// 不是填 0 占位 —— 填 0 会让"这枚印章带门/带光照"这件事在落位时静默丢失。
///
/// ⚠ 两处类型口径：
/// * `tile_animation` / `tile_offset` 在 `.X` 里是 `i16`（`short`），内核要 `u16`
///   ⇒ 用 `as u16` **保位**转换（高位 `0x8000` 是编辑器标记位，不能丢）；
/// * 前景动画的 `frames` / `tick` 只在 `foreground` 为 `Some` 时有值
///   （C# `ForegroundImages.Count` 的判据是"有没有前景块"，与 `image == 0` 无关，
///   见 `mir2-stamp/src/cell.rs:95-99`）⇒ `None` 时这两项如实给 0。
pub fn cells_of(stamp: &Stamp) -> Option<Vec<StampCellInput>> {
    let mut out = Vec::with_capacity(stamp.cells.len());
    for c in &stamp.cells {
        let front = c.cell.foreground;
        out.push(StampCellInput {
            dx: c.x,
            dy: c.y,
            // `back_image` 是 i32：本仓只承载低 16 位（与 `mir2_map::Cell::bk_img` 同宽）。
            // 高位（C# 的 `0x60000000` 阻挡标记）**不解释** —— 语义未确认，不猜。
            bk: (c.cell.back_image as u32 & 0xFFFF) as u16,
            mid: c.cell.middle_image,
            fr: front.map(|f| f.image as u16).unwrap_or(0),
            bk_lib: u8::try_from(c.cell.back_library).ok()?,
            mid_lib: u8::try_from(c.cell.middle_library).ok()?,
            fr_lib: u8::try_from(front.map(|f| f.library).unwrap_or(0)).ok()?,
            door: c.cell.door,
            door_offset: c.cell.door_offset,
            light: c.cell.light,
            ani_frame: front.map(|f| f.frames).unwrap_or(0),
            ani_tick: front.map(|f| f.tick).unwrap_or(0),
            mid_ani_frame: c.cell.middle_frames,
            mid_ani_tick: c.cell.middle_tick,
            tile_animation: c.cell.tile_animation as u16,
            tile_offset: c.cell.tile_offset as u16,
            tile_frames: c.cell.tile_frames,
        });
    }
    Some(out)
}

/// 读一枚 `.X` 并换成内核要的格表；失败 ⇒ **带路径与原文的中文原因**。
///
/// 三种失败都走 [`tx::STAMP_READ_FAIL_TMPL`]（读不出 / 解析失败）或库号越界那一条，
/// 后半句都是"地图一个字节都没动"——这是有保证的事实：本函数只读，不碰会话。
pub fn read(path: &Path) -> Result<StampFile, String> {
    let bytes = std::fs::read(path).map_err(|e| {
        tx::fill(
            tx::STAMP_READ_FAIL_TMPL,
            &[("path", &path.display().to_string()), ("err", &e.to_string())],
        )
    })?;
    let stamp = Stamp::parse(&bytes).map_err(|e| {
        tx::fill(
            tx::STAMP_READ_FAIL_TMPL,
            &[("path", &path.display().to_string()), ("err", &e.to_string())],
        )
    })?;
    let cells = cells_of(&stamp).ok_or_else(|| {
        tx::fill(
            tx::STAMP_READ_FAIL_TMPL,
            &[
                ("path", &path.display().to_string()),
                ("err", "库号超出 u8 表达范围（拒绝截断成别的库）"),
            ],
        )
    })?;
    Ok(StampFile {
        cells,
        bounds: stamp.bounds(),
    })
}

/// **读盘 → 注册 → 落位**的完整链路（GUI 与无窗验收共用这一条）。
///
/// 返回 `Ok(Placed)` = 真改了 `cells` 格；`Err(中文原因)` = **一格未写**。
///
/// 为什么每次都重新读盘：`.X` 在磁盘上可能被用户改过（CrystalStudio 也是每次放置前
/// 现读，`PlacedStamps.cs:114-121`）。缓存格表会让"改了印章、贴出来还是旧的"。
pub fn place(
    ed: &mut EditSession,
    map: &mut Map,
    armed: &Armed,
    path: &Path,
    x: u16,
    y: u16,
) -> Result<Placed, String> {
    let file = read(path)?;
    // 覆盖式注册：同一个 id 再注册 = 覆盖（内核的契约），正是"改完印章再贴"要的语义。
    ed.register_stamp(armed.id.clone(), file.cells)?;
    match ed.place_stamp_by_id(map, x, y, &armed.id) {
        PlaceOutcome::Placed { cells, skipped } => Ok(Placed { cells, skipped }),
        PlaceOutcome::Refused { reason } => Err(reason),
    }
}

/// 放置成功的回执（有越界跳过时换另一条模板，见 [`tx::STAMP_PLACE_SKIP_TMPL`] 的理由）。
pub fn receipt(name: &str, p: Placed) -> String {
    let n = p.cells.to_string();
    if p.skipped == 0 {
        tx::fill(tx::STAMP_PLACE_OK_TMPL, &[("name", name), ("n", &n)])
    } else {
        tx::fill(
            tx::STAMP_PLACE_SKIP_TMPL,
            &[("name", name), ("n", &n), ("k", &p.skipped.to_string())],
        )
    }
}

/// 放置失败的回执（原因来自内核或 [`read`]）。
pub fn fail_text(why: &str) -> String {
    tx::fill(tx::STAMP_PLACE_FAIL_TMPL, &[("why", why)])
}

/// 待放置态的提示行。
pub fn armed_text(name: &str) -> String {
    tx::fill(tx::STAMP_ARMED_TMPL, &[("name", name)])
}

/// 「锚点 + 印章包围盒」→ 屏幕矩形（**只用于画轮廓**，不产生任何像素）。
///
/// 包围盒是**闭区间**的格坐标（`mir2_stamp::Stamp::bounds` 的口径）：
/// 覆盖的格是 `anchor.x + min_x ..= anchor.x + max_x`（`y` 同理），可负 ⇒ 可能落在图外。
/// 本函数**不裁剪**到地图：越界部分照画（用户要能看见"这一角会掉出去"），
/// 绘制方靠 painter 的 clip 裁到画布矩形。
///
/// 反解：`cell_rect` 的 `min` 是格左上角，`max` 是右下角 ⇒ 左上取第一格的 `min`、
/// 右下取最后一格的 `max`，正好覆盖整块。
pub fn outline_rect(
    view: &CanvasView,
    canvas: Rect,
    anchor: (u16, u16),
    bounds: (i32, i32, i32, i32),
) -> Rect {
    let (min_x, min_y, max_x, max_y) = bounds;
    let ax = i32::from(anchor.0);
    let ay = i32::from(anchor.1);
    let tl = view.cell_rect(canvas, ax + min_x, ay + min_y);
    let br = view.cell_rect(canvas, ax + max_x, ay + max_y);
    Rect::from_min_max(tl.min, br.max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{pos2, vec2};
    use miredit_core::EditLayer;
    use std::path::PathBuf;

    /// 取证根缺省值：`.X` 的取证来源（**整个测试模块里只出现这一处**）。
    /// 换机器用 `MIREDIT_UPSTREAM_ROOT`。
    const 缺省取证根: &str = r"I:\miredit\_upstream";

    /// 取证根：`MIREDIT_UPSTREAM_ROOT` → 缺省 [`缺省取证根`]；根不存在 ⇒ `None`。
    ///
    /// 只做 `var_os` + `PathBuf`，不引入任何第三方库。
    fn 取证根() -> Option<PathBuf> {
        let root = std::env::var_os("MIREDIT_UPSTREAM_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(缺省取证根));
        root.is_dir().then_some(root)
    }

    /// 本机真实印章（CrystalStudio 自带范例；core 的验收门 2 用的也是它）。
    /// 缺取证根/缺文件 ⇒ `None`（公开仓/PR 的机器上没有 `.X` 素材，不算代码错）。
    fn 印章路径() -> Option<PathBuf> {
        let p = 取证根()?.join("Objects").join("mir").join("mir_00000.X");
        p.is_file().then_some(p)
    }

    /// 合成 8×8 `New14` 图：每格地表图号 = 序号 + 1（可区分）。
    fn 造图(w: u16, h: u16) -> Map {
        let mut bytes = vec![0u8; mir2_map::HEADER_LEN + 14 * (w as usize * h as usize)];
        bytes[0..2].copy_from_slice(&w.to_le_bytes());
        bytes[2..4].copy_from_slice(&h.to_le_bytes());
        for i in 0..(w as usize * h as usize) {
            let base = mir2_map::HEADER_LEN + i * 14;
            bytes[base..base + 2].copy_from_slice(&((i as u16) + 1).to_le_bytes());
            bytes[base + 12] = 4; // btTiles
        }
        Map::parse(&bytes).expect("合成图应能解析")
    }

    /// 一枚手工格表（不读盘）：2×2，三族都给非 0 值。
    fn 手印章() -> StampFile {
        StampFile {
            cells: vec![
                StampCellInput { dx: 0, dy: 0, bk: 11, mid: 12, fr: 13, bk_lib: 21, mid_lib: 22, fr_lib: 23, ..Default::default() },
                StampCellInput { dx: 1, dy: 0, bk: 14, mid: 15, fr: 16, bk_lib: 24, mid_lib: 25, fr_lib: 26, ..Default::default() },
                StampCellInput { dx: 0, dy: 1, bk: 17, mid: 18, fr: 19, bk_lib: 27, mid_lib: 28, fr_lib: 29, ..Default::default() },
                StampCellInput { dx: 1, dy: 1, bk: 20, mid: 21, fr: 22, bk_lib: 30, mid_lib: 31, fr_lib: 32, ..Default::default() },
            ],
            bounds: (0, 0, 1, 1),
        }
    }

    /// **注册 → 落位**这条链走通：格表进得去、`place_stamp_by_id` 能按 id 放到锚点上。
    #[test]
    fn 注册后按id落位() {
        let mut map = 造图(8, 8);
        let mut ed = EditSession::default();
        let f = 手印章();
        assert_eq!(ed.register_stamp("t", f.cells).expect("注册应成功"), 4);
        let out = ed.place_stamp_by_id(&mut map, 3, 3, "t");
        assert_eq!(out, PlaceOutcome::Placed { cells: 4, skipped: 0 });
        assert_eq!(map.cell(3, 3).unwrap().bk_img, 11);
        assert_eq!(map.cell(4, 4).unwrap().bk_img, 20);
        assert_eq!(ed.undo_len(), 1, "整次放置 = 一步撤销");
        assert!(ed.dirty(), "放置必须置脏");
    }

    /// **未注册的 id** ⇒ 拒绝 + 中文原因，地图一字节不动。
    #[test]
    fn 未注册被拒且不动地图() {
        let mut map = 造图(4, 4);
        let before = map.to_bytes();
        let mut ed = EditSession::default();
        let out = ed.place_stamp_by_id(&mut map, 0, 0, "nope");
        let PlaceOutcome::Refused { reason } = out else {
            panic!("未注册必须拒绝");
        };
        assert!(reason.contains("未注册"), "{reason}");
        assert_eq!(map.to_bytes(), before, "拒绝时地图必须逐字节不变");
        assert_eq!(ed.undo_len(), 0, "拒绝不该产生撤销步");
    }

    /// **读盘失败**给出中文原因（含路径），且一格未写。
    #[test]
    fn 读盘失败带中文原因() {
        let err = read(Path::new(r"E:\__no_such_stamp__.X")).expect_err("不存在必须失败");
        assert!(err.contains("读不出印章文件"), "{err}");
        assert!(err.contains("__no_such_stamp__.X"), "要点名路径：{err}");
        assert!(err.contains("地图一个字节都没动"), "{err}");
    }

    /// **真实印章**：读得出 25 格、包围盒 `(-1,-2,4,3)`（与 core 门 2 的三个数一致）。
    ///
    /// 这条同时钉住"适配口径没漂"：格数/包围盒变了就说明字段搬运改了。
    #[test]
    fn 真实印章格数与包围盒() {
        let p = match 印章路径() {
            Some(p) => p,
            None => {
                eprintln!("[跳过] 无取证根 MIREDIT_UPSTREAM_ROOT（或缺 Objects/mir/mir_00000.X）");
                return;
            }
        };
        let Ok(f) = read(&p) else {
            eprintln!("[跳过] 本机没有印章样本 {}", p.display());
            return;
        };
        assert_eq!(f.cells.len(), 25, "mir_00000.X 是 25 格");
        assert_eq!(f.bounds, (-1, -2, 4, 3), "包围盒必须是 (-1,-2,4,3)");
        assert_eq!(f.cells.iter().filter(|c| c.fr != 0).count(), 8, "8 格带前景");
        assert!(
            f.cells.iter().all(|c| c.bk == 0 && c.mid == 0),
            "这枚是纯物件印章：地表/中层图号全 0"
        );
    }

    /// **`place()` 全链路**（读盘 → 注册 → 落位）在真实印章上跑通，回执含名称与格数。
    #[test]
    fn 全链路放置真实印章() {
        let p = match 印章路径() {
            Some(p) => p,
            None => {
                eprintln!("[跳过] 无取证根 MIREDIT_UPSTREAM_ROOT（或缺 Objects/mir/mir_00000.X）");
                return;
            }
        };
        let mut map = 造图(16, 16);
        let mut ed = EditSession::default();
        let armed = Armed {
            id: "mir/mir_00000".to_string(),
            name: "mir_00000".to_string(),
            bounds: Some((-1, -2, 4, 3)),
        };
        // 锚点 (5,5) + 包围盒 (-1,-2,4,3) ⇒ x 4..=9 / y 3..=8，全在 16×16 内
        let out = place(&mut ed, &mut map, &armed, &p, 5, 5).expect("应能放置");
        assert_eq!(out.skipped, 0, "锚点在图内，不该有越界格");
        assert!(out.cells > 0, "必须真的改了格");
        assert_eq!(ed.undo_len(), 1);
        let text = receipt(&armed.name, out);
        assert_eq!(text, format!("放置印章「mir_00000」{} 格", out.cells));
        // 带跳过的回执换一条模板（两个数都在）
        let t2 = receipt("x", Placed { cells: 3, skipped: 7 });
        assert_eq!(t2, "放置印章「x」3 格（7 格在地图外，已跳过）");
    }

    /// **全越界 ⇒ 拒绝**，且地图逐字节不变（门 2 的反面）。
    #[test]
    fn 全越界拒绝且不动地图() {
        let p = match 印章路径() {
            Some(p) => p,
            None => {
                eprintln!("[跳过] 无取证根 MIREDIT_UPSTREAM_ROOT（或缺 Objects/mir/mir_00000.X）");
                return;
            }
        };
        let mut map = 造图(4, 4);
        let before = map.to_bytes();
        let mut ed = EditSession::default();
        let armed = Armed { id: "mir/mir_00000".into(), name: "mir_00000".into(), bounds: None };
        let err = place(&mut ed, &mut map, &armed, &p, 200, 200).expect_err("必须拒绝");
        assert!(err.contains("地图外"), "{err}");
        assert_eq!(map.to_bytes(), before, "拒绝时地图必须逐字节不变");
        assert!(!ed.dirty(), "拒绝不该置脏");
    }

    /// **轮廓矩形**：包围盒 `(-1,-2,4,3)` 在锚点 `(100,100)` ⇒ 覆盖 6×6 格。
    ///
    /// 6 列 = `100-1 ..= 100+4`、6 行 = `100-2 ..= 100+3`；再按 1× 缩放 × 48×32 像素。
    #[test]
    fn 轮廓矩形按包围盒算() {
        let view = CanvasView::default();
        let canvas = Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 800.0));
        let r = outline_rect(&view, canvas, (100, 100), (-1, -2, 4, 3));
        // pan = (0,0)、scale = 1 ⇒ 屏幕坐标 = 世界像素
        assert!((r.left() - 99.0 * 48.0).abs() < 0.01, "左边 {}", r.left());
        assert!((r.top() - 98.0 * 32.0).abs() < 0.01, "上边 {}", r.top());
        assert!((r.right() - 105.0 * 48.0).abs() < 0.01, "右边 {}", r.right());
        assert!((r.bottom() - 104.0 * 32.0).abs() < 0.01, "下边 {}", r.bottom());
        assert!((r.width() - 6.0 * 48.0).abs() < 0.01, "6 列宽");
        assert!((r.height() - 6.0 * 32.0).abs() < 0.01, "6 行高");
    }

    /// 轮廓矩形**不裁剪**：锚点贴着左上角时负偏移会画到画布外（用户要能看见"要掉出去"）。
    #[test]
    fn 轮廓不裁剪到地图() {
        let view = CanvasView::default();
        let canvas = Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 800.0));
        let r = outline_rect(&view, canvas, (0, 0), (-1, -2, 4, 3));
        assert!(r.left() < 0.0, "负偏移必须画到画布外：{}", r.left());
        assert!(r.top() < 0.0, "负偏移必须画到画布外：{}", r.top());
    }

    /// 放置后**当前编辑层与笔刷都不受影响**（印章写的是三族图号，不是"落笔"）。
    #[test]
    fn 放置不改当前层() {
        let mut map = 造图(8, 8);
        let mut ed = EditSession::default();
        ed.set_layer(EditLayer::Middle);
        let f = 手印章();
        ed.register_stamp("t", f.cells).unwrap();
        let _ = ed.place_stamp_by_id(&mut map, 2, 2, "t");
        assert_eq!(ed.layer(), EditLayer::Middle, "放置不该改当前编辑层");
    }
}
