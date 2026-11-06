//! 无窗编辑驱动器：`--edit-*` 脚本 + `--save` / `--save-as`，**不开窗**就把
//! 「改格 → 原子写回」跑完。
//!
//! # `--save-as`（2026-11-05 新增）
//!
//! 「文件 → 另存为…」/ `Ctrl+Shift+S` 要先弹模态敲路径，无人值守运行里敲不了字。
//! 这个开关调的就是模态回车时调的 [`crate::save::save_as_session`]：
//! **写新路径 → 成功后再把文档路径改成它**（标题/状态栏跟着变），原文件一字节不动。
//!
//! 成功时会额外打两行 `[另存为] 标题：…` / `[另存为] 状态栏：…` ——
//! 那两段字来自 GUI 每帧调的同一个 [`crate::screen_text::build`]，
//! 所以"退出后标题显示新文件名"这件事**在没有窗口时也能取证**。
//!
//! # 为什么需要它（不是为了绕过 GUI，而是为了让验收可复现）
//!
//! 验收要求「改 3 个格 → `Ctrl+S` → 用 Python 逐字节比对」。窗口关掉之后，
//! 用鼠标做同样的事**无法复现**（点偏一格就换了一个字节区间），于是把"
//! 同一串操作"做成命令行脚本：
//!
//! - 走的是与鼠标**完全相同**的代码：`edit::EditSession`（吸管/笔刷/橡皮擦/撤销/重做）
//!   与 `save::save_session`（`dirty` 判定 + 原子替换），**没有第二套实现**；
//! - GUI 模式下同一份脚本也会在开窗前执行一遍（见 `app::MireditApp::new`），
//!   所以截图里的"改动痕迹"与这里改的是同一批格。
//!
//! 输出全部走 [`crate::ui_text`] 模板（`[编辑]` / `[比对]` / `[保存]` 前缀），
//! 每一条都是**可机器解析**的：验收脚本直接 grep 这些行。

use crate::cli::Cli;
use crate::edit::{self, EditSession};
use crate::mapdoc::DocState;
use crate::palette::PaletteState;
use crate::save;
use crate::ui_text as tx;

/// 驱动器退出码：
/// - `0` = 脚本跑完（是否真写了盘看 `saved=` / `saveas=` 字段：`ok` 写了 / `skip` 没改动没写）；
/// - `1` = 根本没打开地图（参数没给地图或文件读不了）；
/// - `3` = **保存失败**（`--save` 或 `--save-as` 任一失败；原文件保持原样，
///   失败原因在 stderr 的 `[保存] 失败：…` / `[另存为] 失败：…` 里）。
pub fn run(cli: &Cli) -> i32 {
    let mut doc = DocState::load(cli.map.as_deref());
    if doc.doc.is_none() {
        eprintln!("{}", tx::DIAG_EDIT_NO_MAP);
        return 1;
    }
    let mut ed = EditSession::default();
    // ⓪ **把真文档绑进会话**（2026-11-05 九格式收口）：内核的库号字节还原 / Type5 地表闸门 /
    //    选区入口裁剪读的都是 `raw`（字节层）。不绑的话 `raw` 停在 1×1 `Old12` 占位，
    //    九种格式里除 `Old12` 以外全按错档位判定（Type5 的地表闸门在脚本路径上直接不可达）。
    if let Some(d) = doc.doc.as_ref() {
        if !ed.bind_document(&d.path, d.orig_bytes()) {
            eprintln!(
                "{}",
                tx::fill(
                    tx::DIAG_EDIT_BIND_FAIL,
                    &[("path", &d.path.display().to_string())]
                )
            );
        }
    }
    ed.select(cli.tool.unwrap_or(0).min(crate::icons::TOOLS.len() - 1));
    // ⓪a `--layer`：先定**当前编辑层**（缺省地表层）—— 必须在选笔刷之前，
    //     因为"族不匹配"的提示是按层算的。
    if let Some(layer) = cli.layer {
        ed.set_layer(layer);
    }
    // ⓪a2 `--brush-size`：笔刷尺寸（缺省 3，夹进 3..=31 的奇数）—— 必须在任何落笔之前
    if let Some(n) = cli.brush_size {
        let got = ed.set_brush_size(n);
        eprintln!("{}", tx::fill(tx::DIAG_EDIT_SIZE, &[("n", &got.to_string())]));
    }

    // ⓪a3 `--select`：矩形选区（2026-11-05 起在无窗模式也生效 —— `--export-selection`
    //      要它）。走的是与 GUI 启动时**完全相同**的 `EditSession::set_selection`
    //      （内核在入口处裁剪一次），不是另写一条"直接赋值"的捷径。
    if let Some((x0, y0, x1, y1)) = cli.select {
        if let Some(d) = doc.doc.as_mut() {
            ed.set_selection(
                &mut d.map,
                miredit_core::Selection::new(x0, y0, x1, y1),
            );
        }
    }

    // ⓪ 素材面板（与 GUI 同一份 `apply_palette_cli`）：`--palette-*` 切库/搜索/点选，
    //    点选那一帧会成为"当前笔刷"，紧随其后的 `--edit-brush` 就用它落格 ——
    //    这条链路是"从素材选一帧 → 画一格 → 保存 → 逐字节核对"的闭环。
    let mut pal = PaletteState::new(match &doc.assets {
        crate::mapdoc::Assets::Ready { paths, .. } => Some(paths),
        crate::mapdoc::Assets::Failed(_) => None,
    });
    crate::app::apply_palette_cli(&mut pal, &mut ed, cli, doc.assets.source());

    // ① 编辑脚本（按命令行出现顺序）
    let rep = edit::apply_script(&mut doc, &mut ed, &cli.edits);

    // ①b 被拒绝的落笔（族不匹配）必须出现在**验收可抓**的通道里（状态栏只在窗口里看得见）
    if let Some(why) = ed.note() {
        eprintln!("{}", tx::fill(tx::DIAG_EDIT_REJECT, &[("why", why)]));
    }

    // ② 保存（只在有修改时真写；失败时原文件保持原样）
    let saved = if cli.save {
        Some(save::save_session(&mut doc, &mut ed))
    } else {
        None
    };

    // ②b **另存为**（2026-11-05）：写到新路径并把文档路径改成它。
    //     放在 `--save` 之后：两个开关同时给时"先存原路径、再另存一份"，顺序不歧义。
    //     走的是菜单模态回车时调的**同一个** `save_as_session`（不另写写盘捷径）。
    let saved_as = cli
        .save_as
        .as_ref()
        .map(|target| save::save_as_session(&mut doc, &mut ed, target));

    // ②c 另存为成功后，把**与窗口同一套装配**的标题/状态栏原文打出来。
    //
    //     为什么必须打：验收要的是"退出后标题/状态栏显示新文件名"。无窗模式没有窗口，
    //     所以这里调的是 GUI 每帧调的同一个 `screen_text::build`（不是自己拼字符串），
    //     打出来的就是窗口里会显示的那两段字。
    if matches!(&saved_as, Some(save::SaveOutcome::Written { .. })) {
        let text = crate::screen_text::build(
            &doc,
            &crate::canvas_view::CanvasView::default(),
            None,
            &crate::cellinfo::CellLines::unknown(),
            &ed,
        );
        eprintln!(
            "{}",
            tx::fill(tx::DIAG_SAVE_AS_TITLE, &[("title", &text.title)])
        );
        eprintln!(
            "{}",
            tx::fill(tx::DIAG_SAVE_AS_STATUS, &[("status", &text.status_left)])
        );
    }

    // ②c2 **另存为成功 ⇒ 记进最近地图**（2026-11-05 补）。
    //
    //     官方 `MainWindow.axaml.cs:501` 另存成功后也 `RememberMap(target)`，与打开地图
    //     同一套。菜单路径走 [`crate::menu::action::save_as_path`]（它内部调
    //     `remember_recent`），而**无窗路径走的是本函数的 `save::save_as_session`**
    //     ⇒ 这里必须显式补一次**同源**的记录调用（`action::remember_recent_into`），
    //     否则"命令行另存"与"菜单另存"在最近列表上会变成两种行为。
    //
    //     设置文件：`--settings` 给了就用它，否则用默认路径（与 GUI 启动时同一判据）。
    //     读失败 ⇒ 回落"浅色 + 空列表"（读不到旧历史，也就没有旧历史可覆盖）。
    if let Some(save::SaveOutcome::Written { path, .. }) = &saved_as {
        let settings_path = cli
            .settings
            .clone()
            .unwrap_or_else(crate::menu::settings::default_path);
        let (dark, mut recent) = match crate::menu::settings::load_quiet(&settings_path) {
            Ok(s) => (s.dark, s.recent),
            Err(_) => (false, Vec::new()),
        };
        let _ = crate::menu::action::remember_recent_into(&settings_path, dark, &mut recent, path);
    }

    // ②d **导出 PNG**（2026-11-05）：`--export-selection` / `--export-minimap`。
    //
    //     放在保存之后：导出**只读**地图（不改一个字节），先存再导语义不歧义。
    //     走的是与菜单模态回车**同一个** `crate::export`（不是另写一条渲染/编码旁路）：
    //     分块渲染 → 拼整幅 → `write_rgba_png` → 现有原子写（tmp → 核对 → rename）。
    //     失败时目标路径上**一个文件都不会多**，原因走 `[导出] 失败：…`。
    //
    //     **门态 + 光照档位**（2026-11-05 补）：无窗模式没有窗口也没有菜单，但导出的
    //     必须是"用户在同一张图上看到的那副样子" ⇒ 缺省取 [`Cli::export_options`]
    //     （= `--open-doors` + `--lighting`，与宿主字段 `app.doors_open` / `app.lighting`
    //      的初值同源），`--export-doors` 可显式要开门态。回执里的 `开门=` / `光照=`
    //     字段就是这件事的自证。
    let 导出选项 = cli.export_options();
    let mut export_failed = false;
    if let Some(target) = &cli.export_selection {
        let sel = ed.selection();
        match crate::export::export_selection(&doc, sel, target, 导出选项) {
            Ok(rep) => eprintln!(
                "{}",
                tx::fill(
                    tx::DIAG_EXPORT_OK,
                    &[
                        ("path", &rep.path.display().to_string()),
                        ("w", &rep.width.to_string()),
                        ("h", &rep.height.to_string()),
                        ("bytes", &rep.bytes.to_string()),
                        ("scale", rep.scale.label()),
                        ("tiles", &rep.tiles.to_string()),
                        ("margin", &rep.margin.to_string()),
                        ("twostage", if rep.two_stage { "yes" } else { "no" }),
                        ("doors", if rep.doors_open { "1" } else { "0" }),
                        ("lighting", &format!("{:?}", rep.lighting)),
                    ]
                )
            ),
            Err(e) => {
                eprintln!("{}", tx::fill(tx::DIAG_EXPORT_FAIL, &[("err", &e)]));
                export_failed = true;
            }
        }
    }
    if let Some(target) = &cli.export_minimap {
        // @acfbbf8a：官方缺省 W*12 × H*8；只有旧 --minimap-max-side 显式给了才走兼容链。
        let 导出结果 = if let Some(max_side) = cli.minimap_max_side {
            if cli.minimap_preset.is_some()
                || cli.minimap_width.is_some()
                || cli.minimap_height.is_some()
            {
                Err("小地图尺寸冲突：--minimap-max-side 是本仓兼容链，不能与官方 --minimap-preset / --minimap-width / --minimap-height 同时给".to_owned())
            } else {
                crate::export::export_minimap(&doc, target, max_side, 导出选项)
            }
        } else {
            cli.minimap_size().and_then(|size| {
                crate::export::export_minimap_sized(&doc, target, size, 导出选项)
            })
        };
        match 导出结果 {
            Ok(rep) => eprintln!(
                "{}",
                tx::fill(
                    tx::DIAG_EXPORT_OK,
                    &[
                        ("path", &rep.path.display().to_string()),
                        ("w", &rep.width.to_string()),
                        ("h", &rep.height.to_string()),
                        ("bytes", &rep.bytes.to_string()),
                        ("scale", rep.scale.label()),
                        ("tiles", &rep.tiles.to_string()),
                        ("margin", &rep.margin.to_string()),
                        ("twostage", if rep.two_stage { "yes" } else { "no" }),
                        ("doors", if rep.doors_open { "1" } else { "0" }),
                        ("lighting", &format!("{:?}", rep.lighting)),
                    ]
                )
            ),
            Err(e) => {
                eprintln!("{}", tx::fill(tx::DIAG_EXPORT_FAIL, &[("err", &e)]));
                export_failed = true;
            }
        }
    }

    // ③ 收尾一行摘要（验收脚本 grep 这一行即可判定整轮结果）
    eprintln!(
        "EDIT_RESULT edited={} steps={} undo={} redo={} dirty={} layer={} saved={} saveas={}",
        rep.edited_cells,
        rep.steps,
        ed.undo_len(),
        ed.redo_len(),
        u8::from(ed.dirty()),
        ed.layer().key(),
        match &saved {
            None => "none",
            Some(save::SaveOutcome::NoMap) => "nomap",
            Some(save::SaveOutcome::Clean) => "skip",
            Some(save::SaveOutcome::Written { .. }) => "ok",
            Some(save::SaveOutcome::Failed(_)) => "fail",
        },
        match &saved_as {
            None => "none",
            Some(save::SaveOutcome::NoMap) => "nomap",
            Some(save::SaveOutcome::Clean) => "skip",
            Some(save::SaveOutcome::Written { .. }) => "ok",
            Some(save::SaveOutcome::Failed(_)) => "fail",
        }
    );
    let failed = matches!(&saved, Some(save::SaveOutcome::Failed(_)))
        || matches!(&saved_as, Some(save::SaveOutcome::Failed(_)))
        || export_failed;
    if failed {
        3
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::EditCmd;
    use mir2_map::HEADER_LEN;
    use std::path::PathBuf;

    fn 造图文件(tag: &str, w: u16, h: u16, cell: usize) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("miredit_gui_editrun_{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(format!("{tag}.map"));
        let mut bytes = vec![0u8; HEADER_LEN + cell * (w as usize * h as usize)];
        bytes[0..2].copy_from_slice(&w.to_le_bytes());
        bytes[2..4].copy_from_slice(&h.to_le_bytes());
        for i in 0..(w as usize * h as usize) {
            let base = HEADER_LEN + i * cell;
            bytes[base..base + 2].copy_from_slice(&((i as u16) + 1).to_le_bytes());
            if cell >= 14 {
                bytes[base + 12] = 4;
            }
        }
        std::fs::write(&p, &bytes).unwrap();
        p
    }

    /// 造图时写进第 `i` 格的 `bk_img` = `i + 1`（`i` = **文件里的格下标**）。
    ///
    /// `mir2_map::Map::cell(x, y)` 的下标是**列优先**：`idx = x * h + y`
    /// （`mir2-map/src/map.rs` 的 `Map::cell`，:132），所以 `(x, y)` 这一格原始 `bk_img`
    /// 就是 `x * h + y + 1`。
    ///
    /// ⚠ 为什么要写成函数、而不是在断言里手写 `0 * 6 + 0 + 1`：
    /// 那个 `6` 是**图高**，与 `造图文件("drv", 6, 6, 12)` 的实参重复。手写版里
    /// `x` 恒为 0 时 `0 * 6` 恒为 0（clippy `erasing_op` 正是这么报出来的）——
    /// 算式看着"通用"，实际把图尺寸抄了两遍：改了尺寸只改一处，断言会静默算错。
    /// 收进一个函数后尺寸只有一个来源，算式也不会再出现"恒为 0 的因子"。
    fn 原始图号(h: u16, x: u16, y: u16) -> u16 {
        x * h + y + 1
    }

    /// 无窗驱动器跑通：改一片 → `--save` → 磁盘上只有那一片格区间变了。
    ///
    /// ⚠ **本版语义**：`--edit-brush` 吃当前笔刷尺寸（缺省 3 ⇒ 3×3，见
    /// [`crate::edit::EditSession::touch_footprint`]），所以 (2,2) 一笔改 9 格；
    /// 断言里既钉"3×3 全中"，也钉"3×3 之外一格不动"（边缘裁剪与不越界）。
    #[test]
    fn 无窗改格并保存() {
        // 图的宽/高只在这里出现一次（断言按它算期望值，见 `原始图号`）
        const W: u16 = 6;
        const H: u16 = 6;
        let p = 造图文件("drv", W, H, 12);
        let orig = std::fs::read(&p).unwrap();
        let cli = Cli {
            map: Some(p.clone()),
            edits: vec![EditCmd::Pickup(0, 0), EditCmd::Brush(2, 2)],
            save: true,
            ..Default::default()
        };
        assert_eq!(run(&cli), 0);
        let after = std::fs::read(&p).unwrap();
        assert_eq!(after.len(), orig.len(), "保存不许改文件长度");
        let d = mir2_map::Map::parse(&after).unwrap();
        // 3×3 覆盖 (1..3, 1..3)：全部刷成 0 号格内容（图号 1）
        for y in 1..=3u16 {
            for x in 1..=3u16 {
                assert_eq!(
                    d.cell(x, y).unwrap().bk_img,
                    1,
                    "({x},{y}) 应在 3×3 落笔范围内"
                );
            }
        }
        // 3×3 之外一格都不许动：(0,0) / (4,4) / (1,4) 的原始图号 = 列优先下标 + 1
        assert_eq!(d.cell(0, 0).unwrap().bk_img, 原始图号(H, 0, 0), "左上角不许动");
        assert_eq!(d.cell(4, 4).unwrap().bk_img, 原始图号(H, 4, 4), "右下角不许动");
        assert_eq!(d.cell(1, 4).unwrap().bk_img, 原始图号(H, 1, 4), "边界外不许动");
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    /// **不按保存就不动文件**：同一个脚本去掉 `--save`，磁盘字节一个都不变。
    #[test]
    fn 不保存时磁盘不变() {
        let p = 造图文件("nosave", 4, 4, 12);
        let orig = std::fs::read(&p).unwrap();
        let cli = Cli {
            map: Some(p.clone()),
            edits: vec![
                EditCmd::Pickup(0, 0),
                EditCmd::Brush(1, 1),
                EditCmd::Check,
            ],
            save: false,
            no_window: true,
            ..Default::default()
        };
        assert_eq!(run(&cli), 0);
        assert_eq!(std::fs::read(&p).unwrap(), orig, "没按保存，文件必须一字节不动");
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    /// 撤销回原样后再保存：**不该写盘**（`dirty` 已假），文件保持原样。
    #[test]
    fn 撤销回原样后保存被跳过() {
        let p = 造图文件("undoall", 3, 3, 12);
        let orig = std::fs::read(&p).unwrap();
        let cli = Cli {
            map: Some(p.clone()),
            edits: vec![
                EditCmd::Pickup(0, 0),
                EditCmd::Brush(1, 1),
                EditCmd::Undo(1),
            ],
            save: true,
            ..Default::default()
        };
        assert_eq!(run(&cli), 0);
        assert_eq!(std::fs::read(&p).unwrap(), orig, "撤销回原样后不该写盘");
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    /// 打不开地图时驱动器如实退出 1（不会假装改过）。
    #[test]
    fn 打不开地图返回一() {
        let cli = Cli {
            map: Some(PathBuf::from(r"E:\__no_such_map__.map")),
            edits: vec![EditCmd::Brush(0, 0)],
            save: true,
            ..Default::default()
        };
        assert_eq!(run(&cli), 1);
    }

    /// 保存失败时退出码 3，且**原文件一字节不动**（与 [`crate::save`] 的保证一致）。
    #[test]
    fn 保存失败返回三且原文件不变() {
        let p = 造图文件("savefail", 3, 3, 12);
        let orig = std::fs::read(&p).unwrap();
        // 用目录占住临时路径 ⇒ 写临时文件必失败
        let tmp = save::temp_path_for(&p);
        std::fs::create_dir_all(&tmp).unwrap();
        let cli = Cli {
            map: Some(p.clone()),
            edits: vec![EditCmd::Pickup(0, 0), EditCmd::Brush(1, 1)],
            save: true,
            ..Default::default()
        };
        assert_eq!(run(&cli), 3, "保存失败必须以非 0 退出码报出来");
        assert_eq!(std::fs::read(&p).unwrap(), orig, "原文件必须一字节不动");
        assert!(!tmp.is_file(), "失败后不许留下临时文件");
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    /// **`--save-as` 端到端**（2026-11-05）：改一笔 → 另存为到新路径 ⇒
    /// ① 新文件在且**大小与原图相同** ② 原文件**一字节不动** ③ 没有临时文件残留。
    ///
    /// 这条是验收口径 ①② 的机器版；③（标题/状态栏显示新文件名）由
    /// `另存为后标题与状态栏都换成新文件名` 覆盖。
    #[test]
    fn 另存为写到新路径且原文件不动() {
        let p = 造图文件("saveas", 5, 5, 12);
        let orig = std::fs::read(&p).unwrap();
        let b = p.parent().unwrap().join("b.map");
        let cli = Cli {
            map: Some(p.clone()),
            edits: vec![EditCmd::Pickup(0, 0), EditCmd::Brush(1, 1)],
            save_as: Some(b.clone()),
            ..Default::default()
        };
        assert_eq!(run(&cli), 0);

        let new = std::fs::read(&b).expect("新文件必须真的在盘上");
        assert_eq!(new.len(), orig.len(), "另存为不许改文件长度");
        assert_ne!(new, orig, "改过一笔 ⇒ 新文件内容必须与原文件不同");
        assert_eq!(std::fs::read(&p).unwrap(), orig, "原文件必须一字节不动");
        assert!(!save::temp_path_for(&b).exists(), "不许留临时文件残渣");
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    /// **另存为成功后标题与状态栏都换成新文件名**（验收口径 ③ 的机器版）。
    ///
    /// 断言的是 `screen_text::build`（GUI 每帧调的就是它）输出的那两段原文 ——
    /// 不是自己拼一句"应该长这样"。
    #[test]
    fn 另存为后标题与状态栏都换成新文件名() {
        let p = 造图文件("saveastitle", 4, 4, 12);
        let b = p.parent().unwrap().join("b.map");
        let cli = Cli {
            map: Some(p.clone()),
            edits: vec![EditCmd::Pickup(0, 0), EditCmd::Brush(1, 1)],
            save_as: Some(b.clone()),
            ..Default::default()
        };
        assert_eq!(run(&cli), 0);

        // 用与驱动器同一条装配路径复算一遍（驱动器打的就是这份字）。
        // ⚠ 这里必须用 `DocState::load`（连资源库一起装）：`screen_text::build` 的状态栏左段
        // 有优先级（地图错 → 保存错 → 图库错 → 文件名），图库失败会**盖住**文件名那一段。
        let st = crate::mapdoc::DocState::load(Some(&b));
        let doc = st.doc.as_ref().expect("新文件必须能被重新打开（不是半截文件）");
        assert_eq!(doc.file_name, "b.map", "重新打开时文件名必须是新名字");
        assert_eq!(doc.path, b, "重新打开时路径必须是新路径");

        let mut ed = crate::edit::EditSession::default();
        let _ = ed.bind_document(&doc.path, doc.orig_bytes());
        let text = crate::screen_text::build(
            &st,
            &crate::canvas_view::CanvasView::default(),
            None,
            &crate::cellinfo::CellLines::unknown(),
            &ed,
        );
        assert!(
            text.title.contains("b.map"),
            "标题里必须是新文件名：{}",
            text.title
        );
        // 状态栏左段**有优先级**（地图错 → 保存错 → 图库错 → 文件名）：图库不可用时，
        // 左段会被「资源库装载失败」那句盖住 —— 那是既定设计（见 `screen_text` 模块文档），
        // 不是本用例要验的东西。所以这里**按两种模式各断言自己那一档**：
        //   有图库 ⇒ 左段必须是新文件名；
        //   无图库（公开仓 / CI / 缺资产）⇒ 左段必须是图库失败原文，且**绝不能**还留着旧文件名。
        // 2026-11-06 补齐：此前这条只在本机有真图库时才成立（缺资产模式会红）。
        match st.assets.error() {
            None => assert!(
                text.status_left.contains("b.map"),
                "状态栏左段里必须是新文件名：{}",
                text.status_left
            ),
            Some(err) => {
                assert!(
                    text.status_left.contains(err),
                    "图库不可用时左段应如实报图库失败：{}",
                    text.status_left
                );
                assert!(
                    !text.status_left.contains("saveastitle"),
                    "左段里不许还留着旧文件名：{}",
                    text.status_left
                );
            }
        }
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    /// **`--save-as` 指向非空目录 ⇒ 退出码 3**：原文件不动、目录里的东西不动、
    /// 没有新文件被创建。
    #[test]
    fn 另存为目标是目录时失败返回三() {
        let p = 造图文件("saveasdir", 3, 3, 12);
        let orig = std::fs::read(&p).unwrap();
        let target = p.parent().unwrap().join("b.map");
        std::fs::create_dir_all(&target).unwrap();
        let marker = target.join("keep.txt");
        std::fs::write(&marker, b"KEEP").unwrap();

        let cli = Cli {
            map: Some(p.clone()),
            edits: vec![EditCmd::Pickup(0, 0), EditCmd::Brush(1, 1)],
            save_as: Some(target.clone()),
            ..Default::default()
        };
        assert_eq!(run(&cli), 3, "另存为失败必须以非 0 退出码报出来");
        assert_eq!(std::fs::read(&p).unwrap(), orig, "原文件必须一字节不动");
        assert!(target.is_dir(), "目标目录必须还在（没被当成文件覆盖）");
        assert_eq!(std::fs::read(&marker).unwrap(), b"KEEP", "目录里的东西不许动");
        assert!(!save::temp_path_for(&target).exists(), "不许留临时文件");
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }
}
