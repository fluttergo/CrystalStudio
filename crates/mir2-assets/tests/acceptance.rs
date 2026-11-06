//! 验收门（派单 §五）—— 全部数据驱动，**目录/文件缺失时 `eprintln!` + 跳过，不 panic**。
//!
//! | 用例 | 覆盖的验收门 |
//! |---|---|
//! | [`抽样五十帧与客户端图集逐像素比对`] | 门 1（逐像素比对）+ 门 4（统计输出） |
//! | [`扫描客户端目录能解析出三个基库`] | 门 2（`LibPaths::discover`） |
//! | [`越界帧号与缺失库返回none`] | 门 3（越界返 `None`，不 panic） |
//! | [`帧锚点与图集meta逐帧一致`] | 门 5（`Frame::anchor` 与 meta `anchor` 逐帧对账） |
//!
//! 跑法：`cargo test -p mir2-assets -- --nocapture`（`--nocapture` 才能看到统计行）。

mod support;

use std::collections::BTreeMap;
use std::path::PathBuf;

use mir2_assets::{Family, Frame, LibPaths, LibRef, SkipReason, SpriteSource, WilSource};

use support::atlas::{decode_png_rgba, load_atlas_meta, MetaFrame};

/// 抽样库清单（派单 §五 指定这四个）。
///
/// 第二列是 `LibRef` 显式构造 —— 顺带把 §一 的 `N+1` 规则钉死在用例里：
/// `Tiles5` 对应地面 4 号、`Objects21` 对应表层 20 号。
const SAMPLE_LIBS: [(&str, LibRef); 4] = [
    ("Tiles", LibRef::ground(0)),
    ("Tiles5", LibRef::ground(4)),
    ("Objects", LibRef::surface(0)),
    ("Objects21", LibRef::surface(20)),
];
/// 每库抽样帧数（派单 §五 指定 50）。
const SAMPLES_PER_LIB: usize = 50;

/// 客户端图库根：读 `MIREDIT_CLIENT_ROOT`（**未设 / 不是目录 ⇒ `None`**）。
///
/// 公开仓不内置任何开发机路径 ⇒ 换机器只改环境变量，用例各自打印「跳过」并返回。
fn client_root() -> Option<PathBuf> {
    mir2_assets::client_root()
}

/// 客户端已烘焙图集根（只读）：读 `MIREDIT_CLIENT_ATLAS_ROOT`（未设 ⇒ `None`）。
fn atlas_root() -> Option<PathBuf> {
    mir2_assets::client_atlas_root()
}

/// 验收门 1 + 门 4：四个库各抽 50 帧，与客户端图集逐字节比对，并打印统计。
#[test]
fn 抽样五十帧与客户端图集逐像素比对() {
    let Some(client) = client_root() else {
        eprintln!("跳过：未设置 MIREDIT_CLIENT_ROOT（客户端图库根）");
        return;
    };
    let Some(assets) = atlas_root() else {
        eprintln!("跳过：未设置 MIREDIT_CLIENT_ATLAS_ROOT（客户端图集根）");
        return;
    };
    let paths = match LibPaths::discover(&client) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("跳过：扫描 {} 失败：{e}", client.display());
            return;
        }
    };
    if paths.available() == 0 {
        eprintln!("跳过：{} 下没登记到任何图库", client.display());
        return;
    }
    let src = match WilSource::open(&paths) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("跳过：打开图源失败：{e}");
            return;
        }
    };
    println!(
        "扫描根 {} → 登记 {} 个库（地面 {} / 中层 {} / 表层 {}）",
        client.display(),
        paths.available(),
        paths.available_in(Family::Ground),
        paths.available_in(Family::Middle),
        paths.available_in(Family::Surface)
    );

    let mut ran_libs = 0usize;
    let mut total_ok = 0usize;
    let mut total_bad = 0usize;
    let mut total_sampled = 0usize;
    let mut details: Vec<String> = Vec::new();

    for (name, lib) in SAMPLE_LIBS {
        let meta_path = assets.join(name).join(format!("{name}_atlas_meta.json"));
        if !meta_path.is_file() {
            eprintln!("跳过 {name}：meta 不存在 {}", meta_path.display());
            continue;
        }
        let meta = match load_atlas_meta(&meta_path) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("跳过 {name}：{e}");
                continue;
            }
        };
        // 用 `LibRef` 直接定位；顺带核「基名 ↔ LibRef」一致（防 N+1 规则悄悄跑偏）。
        if paths.find_stem(name) != Some(lib) {
            eprintln!("跳过 {name}：`discover` 未按 {lib} 登记该基名");
            continue;
        }

        // 帧池 = meta 里 `w>0 && h>0 && atlas 非空 && region 非空` 的帧（派单 §五 口径）。
        let pool: Vec<&MetaFrame> = meta
            .frames
            .iter()
            .filter(|f| f.w > 0 && f.h > 0 && f.atlas.is_some() && f.region.is_some())
            .collect();
        if pool.is_empty() {
            eprintln!("跳过 {name}：meta 里没有可比对的帧");
            continue;
        }

        let picks = pick_indices(pool.len(), SAMPLES_PER_LIB, seed_of(name));
        let mut by_atlas: BTreeMap<usize, Vec<&MetaFrame>> = BTreeMap::new();
        for &i in &picks {
            let f = pool[i];
            // 池的过滤条件已保证 atlas 非空。
            let atlas_idx = f.atlas.unwrap_or_default();
            by_atlas.entry(atlas_idx).or_default().push(f);
        }

        let mut ok = 0usize;
        let mut bad = 0usize;
        let mut sampled = 0usize;

        for (atlas_idx, frames) in &by_atlas {
            let Some(file) = meta.atlas_files.get(*atlas_idx) else {
                eprintln!("跳过 {name} 图集 {atlas_idx}：meta 的 atlas.atlases 未列该序号");
                continue;
            };
            let png = assets.join(name).join(file);
            if !png.is_file() {
                eprintln!("跳过 {name} 图集 {atlas_idx}：PNG 不存在 {}", png.display());
                continue;
            }
            let (pw, ph, px) = match decode_png_rgba(&png) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("跳过 {name} 图集 {atlas_idx}：{e}");
                    continue;
                }
            };
            for f in frames {
                sampled += 1;
                let region = f.region.unwrap_or_default();
                let Some(frame) = src.frame(lib, f.idx) else {
                    bad += 1;
                    println!(
                        "不一致：{name} 帧 {} —— 解码返回 None（库号 {lib}，越界/空帧/库不可得）",
                        f.idx
                    );
                    continue;
                };
                if frame.width != f.w || frame.height != f.h {
                    bad += 1;
                    println!(
                        "不一致：{name} 帧 {} —— 解码尺寸 {}x{} vs meta 声明 {}x{}",
                        f.idx, frame.width, frame.height, f.w, f.h
                    );
                    continue;
                }
                if region[2] != f.w || region[3] != f.h {
                    bad += 1;
                    println!(
                        "不一致：{name} 帧 {} —— region 尺寸 {}x{} vs 帧尺寸 {}x{}",
                        f.idx, region[2], region[3], f.w, f.h
                    );
                    continue;
                }
                match first_mismatch(&frame, &px, pw, ph, region) {
                    Err(msg) => {
                        bad += 1;
                        println!("不一致：{name} 帧 {} —— {msg}", f.idx);
                    }
                    Ok(None) => ok += 1,
                    Ok(Some((x, y, a, b))) => {
                        bad += 1;
                        println!(
                            "不一致：{name} 帧 {} 首个不一致像素 ({x},{y}) 图集={a:?} 解码={b:?}",
                            f.idx
                        );
                    }
                }
            }
        }

        let expect = SAMPLES_PER_LIB.min(pool.len());
        println!(
            "{name}：各库抽样 {expect} 帧，逐像素一致 {ok}/{sampled}，不一致 {bad}（palette={} mismatch={}）",
            meta.palette, meta.palette_mismatch
        );
        details.push(format!(
            "{name} ok={ok} bad={bad} sampled={sampled} expect={expect}"
        ));
        ran_libs += 1;
        total_ok += ok;
        total_bad += bad;
        total_sampled += sampled;
        assert_eq!(
            sampled, expect,
            "{name}：应比对 {expect} 帧，实际只比对了 {sampled} 帧（有图集 PNG / meta 条目缺失被跳过）"
        );
    }

    if ran_libs == 0 {
        eprintln!("跳过：四个抽样库均不可用（meta / PNG / 登记任一缺失）");
        return;
    }

    println!(
        "汇总：各库抽样 {SAMPLES_PER_LIB} 帧，逐像素一致 {total_ok}/{total_sampled}，不一致 {total_bad}"
    );
    println!("明细：{}", details.join(" | "));

    assert!(
        total_sampled > 0,
        "采样数为 0 —— 比对根本没跑起来，不能算通过"
    );
    assert_eq!(
        total_bad, 0,
        "存在逐像素不一致（逐条见上方打印）；不允许放宽成模糊比较"
    );
}

/// 验收门 2：`discover` 至少能解析出 `Tiles` / `SmTiles` / `Objects` 三个基库。
#[test]
fn 扫描客户端目录能解析出三个基库() {
    let Some(client) = client_root() else {
        eprintln!("跳过：未设置 MIREDIT_CLIENT_ROOT（客户端图库根）");
        return;
    };
    let paths = match LibPaths::discover(&client) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("跳过：扫描失败：{e}");
            return;
        }
    };
    if paths.available() == 0 {
        eprintln!("跳过：{} 下没登记到任何图库", client.display());
        return;
    }

    println!("登记库总数 = {}", paths.available());
    for family in Family::ALL {
        println!(
            "  {family}族 {} 个，族内索引 {:?}",
            paths.available_in(family),
            paths.indices(family)
        );
    }
    println!("文件名样例：");
    for info in paths.iter().take(8) {
        println!(
            "  {} {} → {}",
            info.lib,
            info.stem().unwrap_or("<无基名>"),
            info.file.data.display()
        );
    }
    println!(
        "跳过 {} 个文件、忽略 {} 个重复候选",
        paths.skipped().len(),
        paths.duplicates().len()
    );
    for s in paths.skipped().iter().take(5) {
        println!("  跳过 {}（{}）", s.path.display(), s.reason);
    }

    for (lib, stem) in [
        (LibRef::ground(0), "tiles"),
        (LibRef::middle(0), "smtiles"),
        (LibRef::surface(0), "objects"),
    ] {
        let path = paths
            .resolve(lib)
            .unwrap_or_else(|| panic!("{lib} 缺少基库（期望基名 `{stem}`）"));
        let got = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        assert_eq!(got, stem, "{lib} 解析到了 `{got}`");
        println!("{lib} = {}", path.display());
    }

    // 同索引跨族必须互不相通（防「把 btArea 当 btTiles 传」这类静默错页）。
    let ground0 = paths.resolve(LibRef::ground(0));
    let surface0 = paths.resolve(LibRef::surface(0));
    assert!(
        ground0.is_some() && surface0.is_some() && ground0 != surface0,
        "ground(0) 与 surface(0) 必须是两个不同文件"
    );
    println!(
        "跨族隔离：ground(0)={:?} / surface(0)={:?} 互不相同 ✅",
        ground0.map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())),
        surface0.map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
    );

    // §一 的规则锚点（基名 ↔ LibRef 的 N+1 映射）。
    for (stem, expect) in [
        ("Tiles", LibRef::ground(0)),
        ("Tiles5", LibRef::ground(4)),
        ("Objects", LibRef::surface(0)),
        ("Objects21", LibRef::surface(20)),
    ] {
        assert_eq!(paths.find_stem(stem), Some(expect), "{stem} 的定位键不对");
    }
    println!("基名 ↔ LibRef：Tiles/ground(0)、Tiles5/ground(4)、Objects/surface(0)、Objects21/surface(20) ✅");
}

/// 验收门 3：越界帧号（含 `u32::MAX`）/ 未登记库号返回 `None`，不 panic、不给假图。
#[test]
fn 越界帧号与缺失库返回none() {
    let Some(client) = client_root() else {
        eprintln!("跳过：未设置 MIREDIT_CLIENT_ROOT（客户端图库根）");
        return;
    };
    let paths = match LibPaths::discover(&client) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("跳过：扫描失败：{e}");
            return;
        }
    };
    if paths.available() == 0 {
        eprintln!("跳过：{} 下没登记到任何图库", client.display());
        return;
    }
    let src = match WilSource::open(&paths) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("跳过：打开图源失败：{e}");
            return;
        }
    };
    let tiles = LibRef::ground(0);
    if paths.resolve(tiles).is_none() {
        eprintln!("跳过：`discover` 未按 {tiles} 登记 Tiles");
        return;
    }

    assert!(
        src.frame(tiles, u32::MAX).is_none(),
        "u32::MAX 必须返回 None"
    );
    assert!(
        src.frame(tiles, u32::MAX - 1).is_none(),
        "u32::MAX-1 必须返回 None"
    );
    assert!(
        src.frame(tiles, 10_000_000).is_none(),
        "远超帧数必须返回 None"
    );
    // 未登记的「族 + 索引」组合（本机顶层族只有 0..=50 与 511..=517/5107）
    assert!(
        src.frame(LibRef::middle(65_535), 0).is_none(),
        "未登记的族内索引必须返回 None"
    );
    // 轨迹接口与 trait 接口必须同行为
    assert!(SpriteSource::frame(&src, LibRef::surface(65_535), 0).is_none());
    println!("越界/缺失库：4 次取帧全部 None ✅");

    // 空存根库（本机 `objects41.wzl` 只有 64 字节）→ None，而不是全透明假图。
    let stub = LibRef::surface(40); // objects41 ⇒ 表层 40 号
    if paths.resolve(stub).is_some() {
        let bytes = paths
            .resolve(stub)
            .and_then(|p| std::fs::metadata(p).ok())
            .map(|m| m.len())
            .unwrap_or(0);
        assert!(
            src.frame(stub, 0).is_none(),
            "空存根库 Objects41（{bytes} 字节）不应解出任何帧"
        );
        println!("Objects41（{bytes} 字节空存根）取帧 → None ✅");
    } else {
        println!("Objects41 未登记（本机无该库，跳过该子断言）");
    }
    println!("失败记录（应按需留痕，不静默）：{:?}", src.failures());
}

/// 补充校验：旧版 `.wil/.wix` 容器（`reference_full`）也能正确配对、正确记录不成对文件。
///
/// 这条不在派单的四道验收门里，用来堵一个盲区：四门只覆盖 `.wzl/.wzx`，
/// 而 `discover` 同时收 `.wil/.wix`，不验就等于没接。
#[test]
fn 扫描wil容器目录能正确配对() {
    let Some(root) = mir2_assets::client_wil_root() else {
        eprintln!("跳过：未设置 MIREDIT_CLIENT_WIL_ROOT（旧版 .wil/.wix 目录）");
        return;
    };
    let paths = match LibPaths::discover(&root) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("跳过：扫描 {} 失败：{e}", root.display());
            return;
        }
    };
    if paths.available() == 0 {
        eprintln!("跳过：{} 下没登记到 .wil/.wix 库", root.display());
        return;
    }
    println!(
        "`.wil/.wix` 目录 {} → 登记 {} 个库（地面 {} / 中层 {} / 表层 {}）",
        root.display(),
        paths.available(),
        paths.available_in(Family::Ground),
        paths.available_in(Family::Middle),
        paths.available_in(Family::Surface)
    );
    for info in paths.iter().take(6) {
        println!(
            "  {} {} → {}",
            info.lib,
            info.stem().unwrap_or("<无基名>"),
            info.file.data.display()
        );
    }

    // 基库必须落在 `.wil` 上、索引必须落在 `.wix` 上（不是 .wzl/.wzx）。
    for (lib, stem) in [
        (LibRef::ground(0), "tiles"),
        (LibRef::surface(0), "objects"),
    ] {
        let file = paths
            .resolve_lib(lib)
            .unwrap_or_else(|| panic!("{lib} 缺少基库"));
        assert_eq!(
            ext_lower(&file.data).as_deref(),
            Some("wil"),
            "{lib} 数据文件扩展名不对：{}",
            file.data.display()
        );
        assert_eq!(
            ext_lower(&file.index).as_deref(),
            Some("wix"),
            "{lib} 索引文件扩展名不对：{}",
            file.index.display()
        );
        assert_eq!(
            file.data
                .file_stem()
                .map(|s| s.to_string_lossy().to_ascii_lowercase())
                .as_deref(),
            Some(stem)
        );
    }

    let incomplete: Vec<String> = paths
        .skipped()
        .iter()
        .filter(|s| s.reason == SkipReason::IncompletePair)
        .filter_map(|s| s.path.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect();
    println!("不成对（记为跳过）{} 个：{incomplete:?}", incomplete.len());
    assert!(
        !incomplete.is_empty(),
        "本机 `reference_full` 里 `StateItem2.wil` 无配对 `.wix`，应被记为不成对"
    );
}

/// 验收门 5：`Frame::anchor` 必须与图集 meta 的 `frames[i].anchor` **逐帧相等**。
///
/// # 为什么这条能证明「最后一跳搬对了」
///
/// `anchor` 有两条独立的生产路径，落到同一个值：
///
/// | 路径 | 生产点 |
/// |---|---|
/// | ① 本 crate 取帧 | `WilSource::lookup_frame` 从 `DecodedFrame.info.px/py` 搬进 [`Frame::anchor`] |
/// | ② 离线烘焙图集 | 上游 `wil2atlas` 把同一对 `px/py` 写进 meta 的 `anchor`（`meta.rs:27`，未 vendor） |
///
/// 两条路共用同一个解码器，但**搬运的代码是两份**。只要有一份漏搬/搬错（例如把 `px` 和 `py`
/// 顺序搞反、或 WZL 容器取了 WIL 的字节位置），这里就会逐帧报出来。
/// 另外同时核 `SpriteSource::anchor()` 与 `SpriteSource::frame().anchor` 同值
/// （前者走「只读帧头」的快路径，后者走完整解码，两条实现必须一致）。
#[test]
fn 帧锚点与图集meta逐帧一致() {
    let Some(client) = client_root() else {
        eprintln!("跳过：未设置 MIREDIT_CLIENT_ROOT（客户端图库根）");
        return;
    };
    let Some(assets) = atlas_root() else {
        eprintln!("跳过：未设置 MIREDIT_CLIENT_ATLAS_ROOT（客户端图集根）");
        return;
    };
    let paths = match LibPaths::discover(&client) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("跳过：扫描 {} 失败：{e}", client.display());
            return;
        }
    };
    let src = match WilSource::open(&paths) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("跳过：打开图源失败：{e}");
            return;
        }
    };

    let mut 比对库数 = 0usize;
    let mut 比对帧数 = 0usize;
    let mut 非零锚点 = 0usize;
    let mut 不一致: Vec<String> = Vec::new();
    let mut 样例: Vec<String> = Vec::new();

    for (name, lib) in SAMPLE_LIBS {
        let meta_path = assets.join(name).join(format!("{name}_atlas_meta.json"));
        if !meta_path.is_file() {
            eprintln!("跳过 {name}：meta 不存在 {}", meta_path.display());
            continue;
        }
        let meta = match load_atlas_meta(&meta_path) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("跳过 {name}：{e}");
                continue;
            }
        };
        if paths.find_stem(name) != Some(lib) {
            eprintln!("跳过 {name}：`discover` 未按 {lib} 登记该基名");
            continue;
        }
        // 与门 1 同一个帧池口径：meta 说「可渲染」的帧，本 crate 也必须给得出 anchor。
        let pool: Vec<&MetaFrame> = meta
            .frames
            .iter()
            .filter(|f| f.w > 0 && f.h > 0 && f.atlas.is_some() && f.anchor.is_some())
            .collect();
        if pool.is_empty() {
            eprintln!("跳过 {name}：meta 里没有带 anchor 的可渲染帧");
            continue;
        }

        let mut 本库比对 = 0usize;
        let mut 本库非零 = 0usize;
        for f in &pool {
            let 期望 = f.anchor.unwrap_or_default();
            比对帧数 += 1;
            本库比对 += 1;
            if 期望 != [0, 0] {
                非零锚点 += 1;
                本库非零 += 1;
                if 样例.len() < 6 {
                    样例.push(format!("{name}#{} = ({}, {})", f.idx, 期望[0], 期望[1]));
                }
            }
            // ① 完整解码路径
            match src.frame(lib, f.idx) {
                Some(frame) => {
                    if [frame.anchor.0, frame.anchor.1] != 期望 {
                        不一致.push(format!(
                            "{name} 帧 {}：`Frame::anchor` = {:?} vs meta = {期望:?}",
                            f.idx, frame.anchor
                        ));
                    }
                }
                None => 不一致.push(format!("{name} 帧 {}：解码返回 None（meta 说是可渲染帧）", f.idx)),
            }
            // ② 只读帧头的快路径，必须与 ① 同值
            match src.anchor(lib, f.idx) {
                Some((ax, ay)) => {
                    if [ax, ay] != 期望 {
                        不一致.push(format!(
                            "{name} 帧 {}：`SpriteSource::anchor` = ({ax}, {ay}) vs meta = {期望:?}",
                            f.idx
                        ));
                    }
                }
                None => 不一致.push(format!(
                    "{name} 帧 {}：`SpriteSource::anchor` 返回 None（同帧解码却成功）",
                    f.idx
                )),
            }
        }
        println!(
            "{name}（{lib}）：逐帧比对 anchor {本库比对} 帧，其中非零锚点 {本库非零} 帧"
        );
        比对库数 += 1;
    }

    if 比对库数 == 0 {
        eprintln!("跳过：四个抽样库的 meta 均不可用");
        return;
    }
    println!("汇总：{比对库数} 个库共比对 {比对帧数} 帧，非零锚点 {非零锚点} 帧");
    println!("非零锚点样例：{}", 样例.join(" | "));
    if !不一致.is_empty() {
        for line in 不一致.iter().take(10) {
            println!("不一致：{line}");
        }
        panic!("anchor 与 meta 不一致 {} 处（上方列前 10 条）", 不一致.len());
    }
    assert_eq!(不一致.len(), 0);
    // 「全 0 也能通过」是假绿：必须真的比到非零锚点，否则说明 meta 里 anchor 恒 0 / 解析错了。
    assert!(
        非零锚点 > 0,
        "比对的 {比对帧数} 帧里没有一帧锚点非零 —— 这条用例等于没验（meta 解析或数据有问题）"
    );
}

// ---------------------------------------------------------------------------
// 以下为纯函数辅助（无 IO）
// ---------------------------------------------------------------------------
/// 取小写扩展名。
fn ext_lower(path: &std::path::Path) -> Option<String> {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
}

/// 确定性伪随机抽样：xorshift64 + 部分 Fisher–Yates，返回升序下标。
///
/// 「随机取 50 个」必须是**可复现**的，否则出问题无法复现场景；
/// 故用固定种子（由库名 FNV-1a 哈希得到）而非系统随机。
fn pick_indices(n: usize, k: usize, seed: u64) -> Vec<usize> {
    let k = k.min(n);
    let mut order: Vec<usize> = (0..n).collect();
    let mut s = seed | 1;
    for i in 0..k {
        // xorshift64
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        let span = (n - i) as u64;
        let j = i + (s % span) as usize;
        order.swap(i, j);
    }
    let mut out = order[..k].to_vec();
    out.sort_unstable();
    out
}

/// FNV-1a 64：把库名变成稳定种子。
fn seed_of(name: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in name.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 首个不一致像素：`(x, y, 图集 RGBA, 解码 RGBA)`。
type Mismatch = (u32, u32, [u8; 4], [u8; 4]);

/// 逐像素比对；返回首个不一致像素，全一致则 `Ok(None)`。
fn first_mismatch(
    frame: &Frame,
    atlas: &[u8],
    pw: u32,
    ph: u32,
    region: [u32; 4],
) -> Result<Option<Mismatch>, String> {
    let [rx, ry, rw, rh] = region;
    if rx + rw > pw || ry + rh > ph {
        return Err(format!(
            "region {region:?} 超出图集尺寸 {pw}x{ph}（图集损坏或 meta 与 PNG 不配套）"
        ));
    }
    let need = pw as usize * ph as usize * 4;
    if atlas.len() < need {
        return Err(format!(
            "图集像素字节数 {} < 期望 {need}（{pw}x{ph} RGBA8）",
            atlas.len()
        ));
    }
    if frame.rgba.len() != frame.width as usize * frame.height as usize * 4 {
        return Err(format!(
            "解码帧像素字节数 {} 与 {}x{} 不符",
            frame.rgba.len(),
            frame.width,
            frame.height
        ));
    }

    for y in 0..frame.height {
        let row = ((ry + y) as usize) * pw as usize + rx as usize;
        for x in 0..frame.width {
            let si = (row + x as usize) * 4;
            let di = (y as usize * frame.width as usize + x as usize) * 4;
            let a = [atlas[si], atlas[si + 1], atlas[si + 2], atlas[si + 3]];
            let b = [
                frame.rgba[di],
                frame.rgba[di + 1],
                frame.rgba[di + 2],
                frame.rgba[di + 3],
            ];
            if a != b {
                return Ok(Some((x, y, a, b)));
            }
        }
    }
    Ok(None)
}
