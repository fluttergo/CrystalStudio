//! `render_maps` —— 抽样渲染三张真实地图并导出参考图（验收门 §五.3 + §五.5 的实跑入口）。
//!
//! 用法（在**仓库根**下）：
//!
//! ```text
//! $env:CARGO_INCREMENTAL=0
//! $env:CARGO_TARGET_DIR='<任意可写目录>'
//! $env:MIREDIT_ASSET_ROOT='<地图语料根>'          # 语料根，三张图挂在它下面
//! $env:MIREDIT_CLIENT_ROOT='<客户端根>'           # 图库根（.wzl/.wzx 等）
//! cargo run -p miredit-render --release --bin render_maps
//! cargo run -p miredit-render --release --bin render_maps -- --lighting=night
//! cargo run -p miredit-render --release --bin render_maps -- --lighting=night --viewport=240,250,64,48
//! ```
//!
//! - 每张图渲染 **一块 64×48 世界格**（不是整图；整图 700×700 会是 33600×22400 像素），
//!   视口可用 `--viewport=x,y,w,h` 改；
//! - 输出写到 `layout/refs/render-<图名>.png`（本 crate 允许写的路径）；
//!   带 `--lighting` 且**不是**白天档、或改了视口时，文件名加后缀
//!   （`render-<图名>-<档>.png` / `…-<档>_x240_y250.png`），
//!   **默认调用的输出名与内容都不变**（逐字节）；
//! - 语料根/图库根未设或不是目录、单张图缺失 ⇒ 打印 `[skip]` 后跳过，**不 panic、不造假图**；
//! - 每张图都会渲染两次做**确定性自检**（字节相等才打印 OK）；
//! - 计时口径：`冷` = 该图第一次渲染（含图库首次打开/解码），`热` = 同一进程内第二次。

use std::path::{Path, PathBuf};
use std::time::Instant;

use mir2_assets::{LibPaths, WilSource};
use mir2_map::Map;
use miredit_render::{
    try_render_region_with_options, CollisionOverlay, LayerMask, MapLighting, RenderOptions,
    Viewport, CELL_H, CELL_W,
};

/// 抽样地图：**相对 `MIREDIT_ASSET_ROOT` 的路径**（不是猜的；不存在就跳过）。
///
/// 为什么不再写绝对路径：公开仓不内置任何开发机路径（2026-11-06）。三张图的来源目录不同 ——
/// `0/41` 是客户端工程导出的 `GodotClient/maps`，`n0104` 是原版客户端的 `Map/` ——
/// 所以各自带相对段，统一挂在同一个资产根下。
const 抽样地图: [(&str, &str); 3] = [
    ("0", "GodotClient/maps/0.map"),
    ("41", "GodotClient/maps/41.map"),
    ("n0104", "client/热血传奇23周年客户端/Map/n0104.map"),
];

/// 地图语料根的环境变量名（与各测试里的 `MIREDIT_ASSET_ROOT` 同一个名字）。
const ASSET_ROOT_ENV: &str = "MIREDIT_ASSET_ROOT";

/// 地图语料根：读 `MIREDIT_ASSET_ROOT`；**未设 / 不是目录 ⇒ `None`**（不猜、不回落）。
fn 资产根() -> Option<PathBuf> {
    let raw = std::env::var_os(ASSET_ROOT_ENV)?;
    if raw.is_empty() {
        return None;
    }
    let p = PathBuf::from(raw);
    p.is_dir().then_some(p)
}

/// 抽样窗口：64×48 世界格，左上角 (0,0)。
fn 视口() -> Viewport {
    Viewport::new(0, 0, 64, 48)
}

/// 解析 `--lighting=<day|dawn-dusk|night>` 与 `--viewport=<x>,<y>,<w>,<h>`（也接受空格分隔）。
///
/// 缺省 = `day` + `(0,0,64,48)`（= `RenderOptions::default()` 的光照档 + 老视口）
/// ⇒ 输出与旧版**逐字节相同**。
/// 未知值 ⇒ 打印用法并返回 `None`（不静默回退，免得「以为换了档其实没换」）。
fn 解析参数(args: &[String]) -> Option<(MapLighting, Viewport)> {
    let mut 档 = MapLighting::Day;
    let mut vp = 视口();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        let (键, 值) = if let Some((k, v)) = a.split_once('=') {
            (k.to_string(), Some(v.to_string()))
        } else {
            i += 1;
            (a.clone(), args.get(i).cloned())
        };
        let Some(v) = 值 else {
            eprintln!("[用法] {键} 后面缺少取值");
            return None;
        };
        match 键.as_str() {
            "--lighting" => {
                档 = match v.as_str() {
                    "day" => MapLighting::Day,
                    "dawn-dusk" | "dawndusk" => MapLighting::DawnDusk,
                    "night" => MapLighting::Night,
                    _ => {
                        eprintln!("[用法] 未知光照档：{v}（只认 day / dawn-dusk / night）");
                        return None;
                    }
                };
            }
            "--viewport" => {
                // `x,y,w,h`：x/y 可为负（图外格不画），w/h 必须 > 0
                let 部分: Vec<&str> = v.split(',').collect();
                if 部分.len() != 4 {
                    eprintln!("[用法] --viewport 需要 x,y,w,h 四段，实得 {v}");
                    return None;
                }
                let 数: Option<Vec<i64>> = 部分.iter().map(|s| s.trim().parse::<i64>().ok()).collect();
                let Some(数) = 数 else {
                    eprintln!("[用法] --viewport 有非数字段：{v}");
                    return None;
                };
                let (Ok(w), Ok(h)) = (u32::try_from(数[2]), u32::try_from(数[3])) else {
                    eprintln!("[用法] --viewport 的宽高必须 > 0：{v}");
                    return None;
                };
                let (Ok(x), Ok(y)) = (i32::try_from(数[0]), i32::try_from(数[1])) else {
                    eprintln!("[用法] --viewport 的 x/y 超出 i32：{v}");
                    return None;
                };
                vp = Viewport::new(x, y, w, h);
            }
            _ => {
                eprintln!("[用法] 未知参数：{a}");
                eprintln!("[用法] render_maps [--lighting=day|dawn-dusk|night] [--viewport=x,y,w,h]");
                return None;
            }
        }
        i += 1;
    }
    Some((档, vp))
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((lighting, vp)) = 解析参数(&args) else {
        return std::process::ExitCode::FAILURE;
    };
    let options = RenderOptions::with_lighting(lighting);
    println!(
        "[光照] 档位 {}（遮罩 alpha {}）",
        lighting.name(),
        lighting.darkness()
    );

    let refs = refs_dir();
    if let Err(e) = std::fs::create_dir_all(&refs) {
        eprintln!("[错误] 无法创建输出目录 {}：{e}", refs.display());
        return std::process::ExitCode::FAILURE;
    }

    // 地图语料根：**未设 / 不是目录 ⇒ 直接跳过**（不猜任何缺省目录，也不 panic）
    let Some(assets_root) = 资产根() else {
        eprintln!(
            "[skip] 未设置 {ASSET_ROOT_ENV}（地图语料根）或它不是目录 ⇒ 无地图可出图，退出（不造假图）"
        );
        return std::process::ExitCode::FAILURE;
    };
    println!("[资产根] {}", assets_root.display());

    // 图库根：未设 / 不是目录 ⇒ `None`（公开仓不内置缺省根，见 `mir2_assets::client_root`）
    let Some(root) = mir2_assets::client_root() else {
        eprintln!(
            "[skip] 未设置 {}（图库扫描根）或它不是目录 ⇒ 跳过全部地图，不造假图",
            mir2_assets::CLIENT_ROOT_ENV
        );
        return std::process::ExitCode::FAILURE;
    };
    println!("[图库根] {}", root.display());
    let paths = match LibPaths::discover(&root) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[错误] 图库扫描失败：{e}（跳过全部地图，不造假图）");
            return std::process::ExitCode::FAILURE;
        }
    };
    println!(
        "[图库] 已登记 {} 个（地面 {} / 中层 {} / 表层 {}）",
        paths.available(),
        paths.available_in(mir2_assets::Family::Ground),
        paths.available_in(mir2_assets::Family::Middle),
        paths.available_in(mir2_assets::Family::Surface)
    );
    let src = match WilSource::open(&paths) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[错误] 图源打开失败：{e}（跳过全部地图）");
            return std::process::ExitCode::FAILURE;
        }
    };

    let (pw, ph) = vp.pixel_size().expect("64×48 视口必然合法");
    println!(
        "[视口] origin=({},{}) cells={}×{} scale={} ⇒ 输出 {}×{} px（世界格 {}×{}）",
        vp.origin_x, vp.origin_y, vp.cells_w, vp.cells_h, vp.scale, pw, ph, CELL_W, CELL_H
    );

    let mut 失败 = 0u32;
    for (name, rel) in 抽样地图 {
        // 相对段挂在资产根下（不再有绝对路径）
        let p = assets_root.join(rel);
        if !p.is_file() {
            eprintln!("[skip] {name}：地图不存在 {}", p.display());
            continue;
        }
        let bytes = match std::fs::read(&p) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("[skip] {name}：读文件失败 {}：{e}", p.display());
                失败 += 1;
                continue;
            }
        };
        let map = match Map::parse(&bytes) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("[skip] {name}：解析失败 {}：{e}", p.display());
                失败 += 1;
                continue;
            }
        };
        println!(
            "---- {name}.map：{}×{} 格，档位 {:?}，{} 字节 ----",
            map.width,
            map.height,
            map.format,
            bytes.len()
        );

        let t0 = Instant::now();
        let a = try_render_region_with_options(
            &map,
            &src,
            &vp,
            &LayerMask::all(),
            CollisionOverlay::none(),
            options,
        )
        .expect("64×48 视口应能渲染");
        let 冷 = t0.elapsed();
        let t1 = Instant::now();
        let b = try_render_region_with_options(
            &map,
            &src,
            &vp,
            &LayerMask::all(),
            CollisionOverlay::none(),
            options,
        )
        .expect("64×48 视口应能渲染");
        let 热 = t1.elapsed();

        let 一致 = a.rgba == b.rgba && a.stats == b.stats;
        println!(
            "  渲染 {}×{} px  冷 {:.1} ms / 热 {:.1} ms  确定性 {}",
            a.width,
            a.height,
            冷.as_secs_f64() * 1000.0,
            热.as_secs_f64() * 1000.0,
            if 一致 {
                "OK（两次字节相等）"
            } else {
                "FAIL（两次不一致！）"
            }
        );
        println!("  stats: {}", a.stats);
        if 热.as_secs_f64() > 0.300 {
            println!("  ⚠ 热渲染超过 300 ms 目标");
        }

        // 默认口径（白天档 + 老视口）保持旧文件名 `render-<图名>.png`；
        // 其它组合加后缀，互不覆盖。
        let 默认视口 = vp == 视口();
        let out = if lighting == MapLighting::Day && 默认视口 {
            refs.join(format!("render-{name}.png"))
        } else {
            let 视口后缀 = if 默认视口 {
                String::new()
            } else {
                format!(
                    "_x{}_y{}",
                    vp.origin_x, vp.origin_y
                )
            };
            refs.join(format!(
                "render-{name}-{}{}.png",
                lighting.name(),
                视口后缀
            ))
        };
        match a.save_png(&out) {
            Ok(()) => println!("  已导出 {}", out.display()),
            Err(e) => {
                eprintln!("  [错误] 导出失败 {}：{e}", out.display());
                失败 += 1;
            }
        }
    }

    let fails = src.failures();
    if !fails.is_empty() {
        println!("[图源失败记录] {} 条（前 5 条）：", fails.len());
        for f in fails.iter().take(5) {
            println!("  - {f}");
        }
    }

    if 失败 > 0 {
        eprintln!("[结果] {失败} 项失败");
        return std::process::ExitCode::FAILURE;
    }
    println!("[结果] 全部完成");
    std::process::ExitCode::SUCCESS
}

/// `layout/refs`（相对本 crate 清单目录）。
fn refs_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("layout")
        .join("refs")
}
