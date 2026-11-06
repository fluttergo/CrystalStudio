//! 编辑器主程序入口。
//!
//! 实际实现在 `lib.rs` 的各个模块里（lib/bin 分离，便于单元测试）。
//!
//! `windows_subsystem = "windows"`：release 构建不**新建**控制台窗口。
//! 理由：本程序是 GUI，若保留控制台，Windows 会额外弹一个黑色控制台窗，
//! 它会**盖在应用窗口上**（实测控制台 rect (130,130)-(1123,649) 与应用窗口
//! (156,156)-(1656,1106) 重叠），既影响观感也让自动截图抓不到干净画面。
//! debug 构建保留控制台，方便 `cargo run` 看日志。
//!
//! > ⚠ 从**已有控制台**（`cargo run`、pwsh）拉起时，标准句柄会被子进程继承，
//! > 所以诊断输出照样看得到；`windows_subsystem` 只影响"要不要新开一个控制台窗"。
//!
//! 命令行开关见 [`miredit_gui::ui_text::DIAG_USAGE`]：
//! - 无开关：用 [`miredit_gui::mapdoc::DEFAULT_MAP_PATH`]（**恒为空 = 未指定地图**：
//!   公开仓没有内建缺省图，界面会如实说「未指定地图」并留空画布，不猜路径）；
//! - `--map <路径>`：打开哪张图（**唯一**指定地图的入口）。
//! - `--probe x,y`：**不开窗**，只把该格的单元格信息打到 stderr（与面板同一套取数）；
//! - `--edit-* [--save]`：**不开窗**跑编辑脚本并按需原子写回（见 [`miredit_gui::editrun`]）；
//! - `--shot` / `--autodrag` / `--gate2-*`：取证用（截图、拖动实测、同视点比对）。

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use miredit_gui::cli::Cli;
use miredit_gui::editrun;
use miredit_gui::mapdoc::DocState;
use miredit_gui::selfcheck;
use miredit_gui::ui_text as tx;

fn main() -> eframe::Result {
    let cli = Cli::parse(std::env::args().skip(1));
    if cli.help {
        println!("{}", tx::DIAG_USAGE);
        return Ok(());
    }
    println!("{}", tx::DIAG_START);

    // 无窗编辑驱动器：跑 `--edit-*` 脚本、按需 `--save`，然后退出（不进窗口）
    if cli.headless() {
        std::process::exit(editrun::run(&cli));
    }

    // 探针模式：不开窗，只报某格的真实字段（验收门 5 的"与 mir2-map 直读对照"靠它）
    if let Some((x, y)) = cli.probe {
        let doc = DocState::load(cli.map.as_deref());
        selfcheck::probe(&doc, x, y);
        return Ok(());
    }

    // 印章探针：不开窗，跑目录对账 + 缩略图渲染对照 + LRU 压测（验收门 §四.2~5 靠它）
    if cli.stamp_probe {
        std::process::exit(miredit_gui::stamp_probe::run(cli.stamp_sweep));
    }

    // 渲染转储模式：不开窗，直接出 miredit-render 的 1:1 位图（隔离"渲染库 vs 界面"）
    if let Some(path) = cli.dump_render.clone() {
        let doc = DocState::load(cli.map.as_deref());
        let (ox, oy) = cli.dump_origin.unwrap_or((0, 0));
        let (cw, ch) = cli.dump_cells.unwrap_or((64, 48));
        let mask = match cli.layers {
            Some([g, mid, f]) => miredit_render::LayerMask {
                ground: g,
                middle: mid,
                front: f,
            },
            None => miredit_render::LayerMask::all(),
        };
        // `--collision` 也作用于转储：验收"开通道 → 出现标记 / 关通道 → 逐字节回到原图"
        // 就是同一条命令跑两遍（带/不带这个开关）比 PNG 字节
        let overlay = cli
            .collision
            .unwrap_or_else(miredit_render::CollisionOverlay::none);
        // `--open-doors` / `--lighting` 同理：不带 ⇒ 闭门 + 白天（与接线前逐字节相同），
        // 带 ⇒ 前景层按门开帧偏移 / 叠对应档位遮罩。两者打包成同一份 `RenderOptions`，
        // 判据只有一处（[`miredit_gui::cli::Cli::export_options`]）。
        if let Err(e) = selfcheck::dump_render(
            &doc,
            (ox, oy),
            (cw, ch),
            &mask,
            overlay,
            cli.export_options(),
            &path,
        ) {
            eprintln!("[渲染转储] 失败：{e}");
        }
        return Ok(());
    }

    // 菜单清单转储：不开窗，把 5 个菜单 39 项逐条打到 stderr 后退出（验收门 §四.2 的证据来源）
    if cli.menu_dump {
        let _ = miredit_gui::menu::dump(
            cli.map.is_some(),
            cli.dark || cli.theme == Some(true),
            &miredit_gui::menu::panels::PanelVis::default(),
        );
        return Ok(());
    }

    let r = miredit_gui::window::run(&cli);
    match &r {
        Ok(()) => println!("{}", tx::fill(tx::DIAG_EXIT, &[("code", "0")])),
        Err(e) => eprintln!(
            "{}",
            tx::fill(tx::DIAG_EXIT_ERR, &[("err", &e.to_string())])
        ),
    }
    r
}
