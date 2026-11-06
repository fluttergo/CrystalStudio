//! **真实原版 `.lib` 素材**验收（E1b）：path-backed 打开 + 按帧 `seek` 解码。
//!
//! # 为什么单独一个文件
//!
//! `tests/crystal_lib.rs` 全是合成样本（逐字段按 `Assets.cs` 拼出来），它证明的是
//! 「字段顺序读对了」；本文件证明的是「原版客户端真实素材能读、能解、像素长度自洽」。
//! 两者不能互相替代：合成样本不会暴露「整文件读入 595 MB」这种量级问题。
//!
//! # 怎么跑
//!
//! ```text
//! # Windows PowerShell（路径含空格，用单引号）
//! $env:MIREDIT_LIB_ROOT='G:\Crystal Mir 2\Data'; cargo test -p mir2-assets --test crystal_lib_real -- --nocapture
//! # 不设该环境变量 / 路径不是目录 ⇒ 每个用例打印 [skip] 并直接 return
//! ```
//!
//! `MIREDIT_LIB_ROOT` 指向**客户端 `Data` 目录**（`Data\Map\WemadeMir2\Tiles.Lib` 那层）
//! 或直接指向 `Data\Map` 都能用 —— [`CrystalLibPaths::discover`] 自己判 `root/Map` 是否存在。
//!
//! 公开仓 CI 里没有这套素材（也不该有：原版客户端是商业素材），所以**绝不硬编码盘符**、
//! 绝不把「没有素材」当失败。素材在、但帧/库不符合预期 ⇒ 直接 `panic!` 并说「素材变了」，
//! 因为那时静默跳过就变成了假绿。

use std::path::{Path, PathBuf};
use std::time::Instant;

use mir2_assets::{
    CrystalAssetFormat, CrystalLibPaths, CrystalLibSource, CrystalLibrary, Family, LibRef,
    SpriteSource,
};

/// 真实素材根的环境变量名。
const ROOT_ENV: &str = "MIREDIT_LIB_ROOT";

/// 小库扫帧上限：找第一个「帧头有效 / 能解出像素」的帧，最多看这么多帧
/// （真实库里第一帧基本都有效，给足余量又不至于把 595 MB 的库扫一遍）。
const SCAN_LIMIT: u32 = 4096;

// ===================== 素材根与路径解析 =====================

/// 取真实素材根；未设置 / 不是目录 → 打印 `[skip]` 并返回 `None`。
fn lib_root() -> Option<PathBuf> {
    let Ok(raw) = std::env::var(ROOT_ENV) else {
        eprintln!("[skip] 未设置 {ROOT_ENV}（真实素材验收需要本机原版客户端，CI 不跑）");
        return None;
    };
    let raw = raw.trim();
    if raw.is_empty() {
        eprintln!("[skip] {ROOT_ENV} 是空字符串");
        return None;
    }
    let root = PathBuf::from(raw);
    if !root.is_dir() {
        eprintln!("[skip] {ROOT_ENV}={} 不是目录", root.display());
        return None;
    }
    Some(root)
}

/// 在素材根下找 `rel`（如 `WemadeMir2/Tiles.Lib`），`root` 与 `root/Map` 两种根都认。
fn resolve_lib(root: &Path, rel: &str) -> Option<PathBuf> {
    for base in [root.to_path_buf(), root.join("Map")] {
        let path = base.join(rel);
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

/// 素材根下的某个库必须存在（不存在说明素材被换了，不许静默跳过）。
fn require_lib(root: &Path, rel: &str) -> PathBuf {
    resolve_lib(root, rel)
        .unwrap_or_else(|| panic!("素材里找不到 {rel}（{ROOT_ENV} 指向的客户端被换过？）"))
}

/// 目录里 `.lib` 文件数（扩展名大小写不敏感，与路径表的匹配口径一致）。
fn count_lib_files(dir: &Path) -> std::io::Result<usize> {
    let mut n = 0usize;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let is_lib = name.rsplit_once('.').is_some_and(|(stem, ext)| {
            !stem.is_empty() && ext.eq_ignore_ascii_case("lib")
        });
        if is_lib {
            n += 1;
        }
    }
    Ok(n)
}

/// 在前 `limit` 帧里找第一个 `read_header` 返回 `Some` 的帧号（附帧头）。
fn first_header(lib: &CrystalLibrary, limit: u32) -> Option<(u32, (u32, u32, i16, i16))> {
    (0..lib.count().min(limit)).find_map(|i| lib.read_header(i).map(|h| (i, h)))
}

/// 在前 `limit` 帧里找第一个 `frame()` 返回 `Some` 的帧号。
fn first_frame(src: &CrystalLibSource, lib: LibRef, limit: u32) -> Option<u32> {
    let end = src.frame_count(lib)?.min(limit);
    (0..end).find(|&i| src.frame_of(lib, i).is_some())
}

// ===================== 1. 路径表 =====================

/// **路径表**：真实 `Data\Map\WemadeMir2` 下三族都登记得到，且**没有文件被静默丢掉**。
///
/// 素材实测（2026-11-06）：`WemadeMir2` 28 个 `.Lib` = `Tiles` + `SmTiles` +
/// `Objects` + `Objects2..26`，全部落在三族规则内；扫不到的必须带原因进 `skipped`。
#[test]
fn 路径表_三族都能登记且不静默丢文件() {
    let Some(root) = lib_root() else {
        return;
    };
    let paths = CrystalLibPaths::discover(&root, CrystalAssetFormat::WemadeMir2)
        .expect("扫描真实素材根应成功");
    assert_eq!(
        paths.resolved_format(),
        Some(CrystalAssetFormat::WemadeMir2),
        "显式要求 WemadeMir2 就必须解析成它"
    );

    for family in Family::ALL {
        let n = paths.available_in(family);
        assert!(
            n > 0,
            "{} 族在 {} 下一个库都没登记到",
            family.label(),
            paths.scan_dir().display()
        );
        println!("{} 族登记 {n} 个库", family.label());
    }

    for lib in [LibRef::ground(0), LibRef::middle(0), LibRef::surface(0)] {
        let path = paths.resolve(lib).unwrap_or_else(|| {
            panic!(
                "{lib} 必须能解析到文件（扫描目录 {}，素材与上游规则不符？）",
                paths.scan_dir().display()
            )
        });
        assert!(path.is_file(), "{lib} → {} 不是文件", path.display());
        println!("{lib} → {}", path.display());
    }

    // 不静默丢：目录里每个 `.lib` 要么登记、要么带原因进 skipped、要么记进 duplicates。
    let on_disk = count_lib_files(paths.scan_dir()).expect("列扫描目录");
    let accounted = paths.available() + paths.skipped().len() + paths.duplicates().len();
    assert_eq!(
        accounted,
        on_disk,
        "{} 个 `.lib` 里只有 {accounted} 个落了账（登记 {} + 跳过 {} + 重复 {}）",
        on_disk,
        paths.available(),
        paths.skipped().len(),
        paths.duplicates().len()
    );
    println!(
        "{on_disk} 个 `.lib` 全部落账：登记 {} / 跳过 {} / 重复 {}",
        paths.available(),
        paths.skipped().len(),
        paths.duplicates().len()
    );
    for s in paths.skipped() {
        println!("[规则外] {} → {}", s.path.display(), s.reason);
    }

    // 真实 `WemadeMir2` 28 个库全部落在三族规则内（`skipped` 实际为空），所以上面
    // 用「登记 + 跳过 + 重复 == 目录里 .lib 总数」来钉「不静默丢」。而 `ShandaMir2`
    // 里确实有规则外文件（`AniTiles1.Lib` = 上游扁平 ID 190 等），用它证明
    // `skipped` 在真实素材上真的会记账、不是永远空着的摆设。
    let shanda = CrystalLibPaths::discover(&root, CrystalAssetFormat::ShandaMir2)
        .expect("扫描真实素材根下的 ShandaMir2 应成功");
    assert_eq!(
        shanda.resolved_format(),
        Some(CrystalAssetFormat::ShandaMir2),
        "ShandaMir2 目录存在且非空"
    );
    assert!(
        !shanda.skipped().is_empty(),
        "ShandaMir2 里应当有规则外文件（AniTiles1.Lib 等），实际一个都没有 —— 素材或分类规则变了？"
    );
    let shanda_on_disk = count_lib_files(shanda.scan_dir()).expect("列 ShandaMir2 目录");
    let shanda_accounted =
        shanda.available() + shanda.skipped().len() + shanda.duplicates().len();
    assert_eq!(
        shanda_accounted,
        shanda_on_disk,
        "ShandaMir2：{} 个 `.lib` 里只有 {shanda_accounted} 个落了账",
        shanda_on_disk
    );
    println!(
        "ShandaMir2：{shanda_on_disk} 个 `.lib` 落账 —— 登记 {} / 跳过 {} / 重复 {}",
        shanda.available(),
        shanda.skipped().len(),
        shanda.duplicates().len()
    );
    for s in shanda.skipped() {
        println!("[ShandaMir2 规则外] {} → {}", s.path.display(), s.reason);
    }
}

// ===================== 2. 三个小库：帧头 + 像素 =====================

/// **三个小库**：`open` → 找第一个有效帧 → `read_header` 与 `decode` 必须自洽。
///
/// 覆盖三本不同来源的库（WemadeMir2 中层 1.29 MB、WemadeMir2 表层 31.7 MB、
/// ShandaMir2 中层 372 KB），每本都用**真实 gzip 帧**走一遍
/// 「偏移 → 17 字节帧头 → `length` 字节 gzip → `w×h×4` 字节 BGRA → RGBA」。
#[test]
fn 三个小库_帧头与像素自洽() {
    let Some(root) = lib_root() else {
        return;
    };
    let cases = [
        "WemadeMir2/SmTiles.Lib",
        "WemadeMir2/Objects15.Lib",
        "ShandaMir2/SmTiles.Lib",
    ];
    for rel in cases {
        let path = require_lib(&root, rel);
        let lib = CrystalLibrary::open(&path)
            .unwrap_or_else(|e| panic!("打开 {} 失败：{e}", path.display()));
        assert!(lib.count() > 0, "{rel} 的帧数必须 > 0");
        assert!(
            matches!(lib.version(), 2 | 3 | 5 | 6),
            "{rel} 版本号 {} 不在 v2/v3/v5/v6 内",
            lib.version()
        );

        let (index, (w, h, x, y)) = first_header(&lib, SCAN_LIMIT).unwrap_or_else(|| {
            panic!(
                "{rel} 前 {SCAN_LIMIT} 帧里没有一帧帧头有效（素材变了？共 {} 帧）",
                lib.count()
            )
        });
        assert!(w > 0 && h > 0, "{rel} 第 {index} 帧 {w}×{h} 必须为正");

        let frame = lib
            .decode(index)
            .unwrap_or_else(|| panic!("{rel} 第 {index} 帧解不出像素（素材变了？）"));
        assert!(
            frame.width > 0 && frame.height > 0,
            "{rel} 第 {index} 帧解出来的尺寸 {}×{} 必须为正",
            frame.width,
            frame.height
        );
        assert_eq!(
            frame.rgba.len(),
            frame.width as usize * frame.height as usize * 4,
            "{rel} 第 {index} 帧：RGBA 必须正好 w×h×4 字节"
        );
        assert_eq!(
            frame.anchor, (x, y),
            "{rel} 第 {index} 帧：decode 的锚点必须等于 read_header 的锚点"
        );
        // v2/v3 的像素尺寸只能来自帧头；Sanxiu `fileType 1` 才允许取内嵌图片自身尺寸。
        if !lib.is_sanxiu() {
            assert_eq!(
                (frame.width, frame.height),
                (w, h),
                "{rel} 第 {index} 帧：v2/v3 的像素尺寸必须等于帧头"
            );
        }
        println!(
            "{rel}: v{}，共 {} 帧；第 {index} 帧 {w}×{h}，锚点 ({x}, {y})，RGBA {} 字节",
            lib.version(),
            lib.count(),
            frame.rgba.len()
        );
    }
}

// ===================== 3. 595 MB 大库：只读头部 =====================

/// **595 MB 的 `Tiles.Lib`**：`open` + `read_header(0)` 必须立刻返回，**不解码**。
///
/// 这条用例的意义在量级：整文件读入 595,097,241 字节会同时表现为「慢」和「吃内存」，
/// 而 path-backed 只读文件头 + 偏移表（几 MB）与 17 字节帧头。
///
/// ⚠ 这里的时间断言是**极宽松兜底**（10 s），不是性能断言：真机上 `open` 是毫秒级，
/// 它只把「明显退化成整文件读入 + 冷盘/忙盘」这种数量级差异变成红灯。
/// 精确的结构钉在 `src/crystal_lib.rs` 的单测 `open_走path_backed只读元数据区`
/// （断言后端是 `Backing::File` 而不是内存字节）。
#[test]
fn 大库tiles_只读头部不解码() {
    let Some(root) = lib_root() else {
        return;
    };
    let path = require_lib(&root, "WemadeMir2/Tiles.Lib");
    let file_len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    assert!(
        file_len > 500 * 1024 * 1024,
        "WemadeMir2/Tiles.Lib 只有 {file_len} 字节，不是那本 595 MB 的大库（素材变了？）"
    );

    let t_open = Instant::now();
    let lib = CrystalLibrary::open(&path).expect("595 MB 的 Tiles.Lib 必须能打开");
    let open_elapsed = t_open.elapsed();

    let t_header = Instant::now();
    let header = lib.read_header(0);
    let header_elapsed = t_header.elapsed();

    assert!(lib.count() > 0, "帧数必须 > 0");
    if let Some((w, h, x, y)) = header {
        assert!(w > 0 && h > 0, "第 0 帧 {w}×{h} 必须为正");
        println!("第 0 帧 {w}×{h}，锚点 ({x}, {y})");
    } else {
        println!("第 0 帧是空帧（w/h 非正）—— 帧号 0 不保证非空，如实用例不依赖它");
    }
    println!(
        "Tiles.Lib {file_len} 字节 / {} 帧：open {open_elapsed:?}，read_header(0) {header_elapsed:?}（未 decode）",
        lib.count()
    );
    assert!(
        open_elapsed.as_secs() < 10,
        "open 用了 {open_elapsed:?}：这不是「只读文件头 + 偏移表」该有的量级（疑似退回整文件读入）"
    );
    assert!(
        header_elapsed.as_secs() < 10,
        "read_header(0) 用了 {header_elapsed:?}：疑似读了整帧数据"
    );
}

// ===================== 4. 图源：同一路径取帧与锚点 =====================

/// **`CrystalLibSource`**：`open(&paths)` 后按 `LibRef` 取帧，`anchor()` 与 `frame().anchor`
/// 必须同值（走的是两条不同代码路径：只读帧头 vs 读帧头 + 解像素）。
#[test]
fn 图源_取帧与锚点走同一路径且同值() {
    let Some(root) = lib_root() else {
        return;
    };
    let paths = CrystalLibPaths::discover(&root, CrystalAssetFormat::WemadeMir2)
        .expect("扫描真实素材根应成功");
    let src = CrystalLibSource::open(&paths).expect("登记表非空就该能打开图源");

    for lib in [LibRef::middle(0), LibRef::surface(0)] {
        let count = src
            .frame_count(lib)
            .unwrap_or_else(|| panic!("{lib} 应该能打开（登记表里有它）"));
        assert!(count > 0, "{lib} 的帧数必须 > 0");
        let index = first_frame(&src, lib, SCAN_LIMIT).unwrap_or_else(|| {
            panic!("{lib} 前 {SCAN_LIMIT} 帧里没有一帧能解出像素（素材变了？共 {count} 帧）")
        });
        let frame = src
            .frame_of(lib, index)
            .unwrap_or_else(|| panic!("{lib} 第 {index} 帧刚试过能取到"));
        assert_eq!(
            SpriteSource::anchor(&src, lib, index),
            Some(frame.anchor),
            "{lib} 第 {index} 帧：锚点快路径必须与 frame().anchor 同值"
        );
        assert_eq!(
            frame.rgba.len(),
            frame.width as usize * frame.height as usize * 4,
            "{lib} 第 {index} 帧：RGBA 必须正好 w×h×4 字节"
        );
        println!(
            "{lib} 第 {index} 帧 {}×{}，锚点 {:?}，RGBA {} 字节",
            frame.width,
            frame.height,
            frame.anchor,
            frame.rgba.len()
        );
    }
    assert!(
        src.failures().is_empty(),
        "正常素材路径不该留失败记录：{:?}",
        src.failures()
    );
}
