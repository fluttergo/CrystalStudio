//! 印章目录 / 缩略图 / LRU 的**取证探针**（`--stamp-probe`，不开窗）。
//!
//! 一次跑出验收门要的全部数字，且**走的是面板同一条代码路径**
//! （[`crate::stamp_panel::render_thumb`] / [`ThumbCache`] 的同一套 LRU 规则）：
//!
//! | 段 | 内容 | 对应验收门 |
//! |---|---|---|
//! | ① | 目录装载（条数 / 跳过 / 未登记 / 耗时） | §四.2 的分母 |
//! | ② | 19 个 kind 的中文名 + 条数 + 「未分类」 | §四.2 |
//! | ③ | 未登记文件清单（只文件名） | §三.5 |
//! | ④ | 5 枚样本：面板路径 vs 直接 `render_stamp` 的逐字段对照 + 像素统计 | §四.3 |
//! | ⑤ | LRU 压测：前进 N 页再回滚，纹理峰值 / 真渲染 / 命中 / 淘汰 | §四.4 |
//! | ⑥ | 搜索对账（`market` / `mir_00000`） | §四.5 |
//!
//! **只读**：不写任何文件（截图那条路才写 PNG）。

use std::collections::BTreeMap;
use std::time::Instant;

use mir2_assets::{LibPaths, WilSource};
use mir2_stamp::{kind_label_zh, Stamp, StampCatalog};

use miredit_render::stamp::{render_stamp, StampRenderOptions};

use crate::metrics as m;
use crate::stamp_panel::{positioned_cells, render_thumb};
use crate::ui_text as tx;

/// 跑完整套探针；返回进程退出码（0 = 全部对账通过）。
///
/// `sweep` = 追加「全库逐枚渲染清点」（9,159 枚，release 十几秒）——
/// 验收要问"**实际渲染了多少枚、失败多少**"，只有真跑一遍才答得上来。
pub fn run(sweep: bool) -> i32 {
    let Some(path) = palette_path() else {
        // 公开仓没有内建索引 ⇒ 如实说「未设置哪个环境变量」，不猜路径、不报假成功
        eprintln!(
            "{}",
            tx::fill(
                tx::STAMP_CATALOG_MISSING_TMPL,
                &[("path", &format!("<未设置 {}>", crate::stamp_panel::PALETTE_ENV))]
            )
        );
        return 1;
    };
    println!("[印章探针] palette.json = {}", path.display());

    // ── ① 装载 ──
    let t0 = Instant::now();
    let mut catalog = match StampCatalog::load(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "{}",
                tx::fill(tx::STAMP_CATALOG_MISSING_TMPL, &[("path", &path.display().to_string())])
            );
            eprintln!("{}", tx::fill(tx::DIAG_STAMP_FAIL, &[("err", &e.to_string())]));
            return 1;
        }
    };
    let ms = t0.elapsed().as_secs_f64() * 1e3;
    let st = catalog.stats().clone();
    println!(
        "{}",
        tx::fill(
            tx::DIAG_STAMP_CATALOG,
            &[
                ("path", &path.display().to_string()),
                ("ms", &format!("{ms:.1}")),
                ("n", &catalog.len().to_string()),
                ("unlisted", &catalog.unlisted().len().to_string()),
                ("skipped", &st.skipped_entries.to_string()),
            ]
        )
    );
    println!(
        "[印章探针] objects[] 原始 {} 条 / 文件命中 {} / 缺失 {} / 扫描 .X {} 个",
        st.object_entries, st.files_present, st.files_missing, st.scanned_files
    );

    // ── ② 分类计数（19 kind + 未分类）──
    let mut 计数: BTreeMap<&str, usize> = BTreeMap::new();
    for e in catalog.all() {
        let key = e.kind.as_deref().unwrap_or("");
        if key.trim().is_empty() {
            *计数.entry("").or_insert(0) += 1;
        } else {
            *计数.entry(key).or_insert(0) += 1;
        }
    }
    let mut 排序: Vec<(&str, usize)> = 计数.into_iter().collect();
    排序.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    let mut 合计 = 0usize;
    for (kind, n) in &排序 {
        let zh = kind_label_zh(kind).unwrap_or(tx::VAL_UNKNOWN);
        println!(
            "{}",
            tx::fill(
                tx::DIAG_STAMP_KIND,
                &[("kind", kind), ("zh", zh), ("n", &n.to_string())]
            )
        );
        合计 += n;
    }
    println!(
        "[印章探针] 分类合计 {合计} 枚（= len() {} ⇒ {}）",
        catalog.len(),
        if 合计 == catalog.len() { "一致" } else { "不一致" }
    );

    // ── ③ 未登记 ──
    for (i, rel) in catalog.unlisted().iter().enumerate() {
        println!("[印章探针] 未登记 #{} {}", i + 1, rel);
    }

    // ── ④ 5 枚样本：面板路径 vs 直接调用 ──
    let src = match open_source() {
        Some(s) => s,
        None => {
            eprintln!("[印章探针] 图库打不开 ⇒ 缩略图与 LRU 两段跳过（不编数字）");
            return 1;
        }
    };
    let mut bad = 0usize;
    let total = catalog.len();
    let 样本 = [0usize, total / 4, total / 2, total * 3 / 4, total - 1];
    for idx in 样本 {
        let entry = catalog.all()[idx].clone();
        let Some(res) = catalog.resolve(&entry.id) else {
            println!("[印章探针] {} → resolve 失败（读不出/解析失败）", entry.id);
            bad += 1;
            continue;
        };
        // 面板路径
        let panel = render_thumb(&res.path, &src);
        // 直接调用（基准）
        let direct = std::fs::read(&res.path)
            .ok()
            .and_then(|b| Stamp::parse(&b).ok())
            .and_then(|s| positioned_cells(&s).map(|c| (s.cells.len(), c)))
            .and_then(|(n, cells)| {
                render_stamp(
                    &cells,
                    &src,
                    &StampRenderOptions::thumbnail(m::STAMP_THUMB_W as u32, m::STAMP_THUMB_H as u32),
                )
                .ok()
                .map(|img| (n, img))
            });
        match (panel, direct) {
            (Ok((pn, pimg)), Some((dn, dimg))) => {
                let same = pn == dn
                    && pimg.width == dimg.width
                    && pimg.height == dimg.height
                    && pimg.content == dimg.content
                    && pimg.cells_drawn == dimg.cells_drawn
                    && pimg.missing == dimg.missing
                    && pimg.rgba == dimg.rgba;
                if !same {
                    bad += 1;
                }
                println!(
                    "{}",
                    tx::fill(
                        tx::DIAG_STAMP_PROBE,
                        &[
                            ("id", &entry.id),
                            ("cells", &pn.to_string()),
                            ("bounds", &fmt_bounds(res.bounds)),
                            ("w", &pimg.width.to_string()),
                            ("h", &pimg.height.to_string()),
                            ("drawn", &pimg.cells_drawn.to_string()),
                            ("missing", &pimg.missing.to_string()),
                            ("same", if same { "是" } else { "否" }),
                        ]
                    )
                );
                // 每枚都报一次三族 (库,图号) + 物件第一帧的取帧结果（区分"库不对"与"帧缺"）
                println!(
                    "[印章探针]   层信息 {} {}",
                    entry.id,
                    layer_probe(&res.path, &src).unwrap_or_default()
                );
                // 第一枚额外报像素统计（非全透明像素占比）
                if idx == 0 {
                    let total_px = (pimg.width * pimg.height) as usize;
                    // 逐像素数 alpha：用索引走查（`chunks_exact(4)` 会触发 clippy 的 as_chunks 建议）
                    let opaque = (0..total_px).filter(|i| pimg.rgba[i * 4 + 3] != 0).count();
                    let pct = if total_px == 0 {
                        0.0
                    } else {
                        opaque as f64 * 100.0 / total_px as f64
                    };
                    println!(
                        "{}",
                        tx::fill(
                            tx::DIAG_STAMP_PIXELS,
                            &[
                                ("id", &entry.id),
                                ("w", &pimg.width.to_string()),
                                ("h", &pimg.height.to_string()),
                                ("total", &total_px.to_string()),
                                ("opaque", &opaque.to_string()),
                                ("pct", &format!("{pct:.1}")),
                            ]
                        )
                    );
                }
            }
            (Ok(_), None) => {
                println!("[印章探针] {} → 直接渲染失败（对照不了）", entry.id);
                bad += 1;
            }
            (Err(e), _) => {
                println!("[印章探针] {} → 面板渲染失败：{e}", entry.id);
                bad += 1;
            }
        }
    }

    // ── ⑤ LRU 压测（与面板同一套规则：上限 `m::STAMP_THUMB_CACHE_CAP`）──
    let drill = lru_drill(&mut catalog, &src);
    println!(
        "{}",
        tx::fill(
            tx::DIAG_STAMP_LRU,
            &[
                ("fwd", &drill.fwd.to_string()),
                ("back", &drill.back.to_string()),
                ("peak", &drill.peak.to_string()),
                ("cap", &m::STAMP_THUMB_CACHE_CAP.to_string()),
                ("decodes", &drill.decodes.to_string()),
                ("hits", &drill.hits.to_string()),
                ("evictions", &drill.evictions.to_string()),
            ]
        )
    );
    println!(
        "[印章 LRU] 峰值是否 ≤ 上限：{}；回滚页命中 {} 次（重用时命中缓存）",
        if drill.peak <= m::STAMP_THUMB_CACHE_CAP {
            "是"
        } else {
            "否"
        },
        drill.back_hits
    );

    // ── ⑥ 搜索对账 ──
    for needle in ["market", "mir_00000"] {
        let hits = catalog.search(needle);
        println!(
            "[印章探针] search({needle:?}) = {} 条，前 3 个 id = {:?}",
            hits.len(),
            hits.iter().take(3).map(|e| e.id.as_str()).collect::<Vec<_>>()
        );
    }

    // ── ⑦ 全库逐枚渲染清点（可选，`--stamp-sweep`）──
    if sweep {
        let t = Instant::now();
        let (mut ok, mut empty, mut partial, mut err) = (0usize, 0usize, 0usize, 0usize);
        // 「空图」再细分：`.X` 本来就 0 格（合法空印章）vs 有格但一帧都取不到
        let (mut zero_cells, mut no_frames) = (0usize, 0usize);
        // 前 5 个"有格但取不到帧"的样本：把它们的 (库,图号) 打出来，便于定位是缺库还是缺帧
        let mut 样本: Vec<String> = Vec::new();
        for i in 0..catalog.len() {
            let id = catalog.all()[i].id.clone();
            let Some(r) = catalog.resolve(&id) else {
                err += 1;
                continue;
            };
            match render_thumb(&r.path, &src) {
                Ok((cells, img)) => {
                    if img.cells_drawn == 0 || img.width == 0 {
                        empty += 1;
                        if cells == 0 {
                            zero_cells += 1;
                        } else {
                            no_frames += 1;
                            if 样本.len() < 5 {
                                样本.push(format!(
                                    "{id}（{cells} 格）{}",
                                    layer_probe(&r.path, &src).unwrap_or_default()
                                ));
                            }
                        }
                    } else if img.missing > 0 {
                        partial += 1;
                    } else {
                        ok += 1;
                    }
                }
                Err(_) => err += 1,
            }
        }
        println!(
            "{}",
            tx::fill(
                tx::DIAG_STAMP_SWEEP,
                &[
                    ("total", &catalog.len().to_string()),
                    ("ok", &ok.to_string()),
                    ("empty", &empty.to_string()),
                    ("partial", &partial.to_string()),
                    ("err", &err.to_string()),
                    ("ms", &format!("{:.0}", t.elapsed().as_secs_f64() * 1e3)),
                ]
            )
        );
        println!(
            "[印章清点] 空图细分：`.X` 本来 0 格 {zero_cells} 枚 / 有格但一帧都取不到 {no_frames} 枚"
        );
        for s in &样本 {
            println!("[印章清点]   取不到帧样本：{s}");
        }
        if ok + empty + partial + err != catalog.len() {
            println!("[印章探针] ⚠ 四类之和与总数不等（清点口径有漏）");
            return 1;
        }
    }

    if bad == 0 {
        println!("[印章探针] 结论：5 枚样本与直接渲染**逐字节一致**，0 失败");
        0
    } else {
        println!("[印章探针] 结论：{bad} 处不一致/失败（见上）");
        1
    }
}

/// `palette.json` 路径：环境变量 [`crate::stamp_panel::PALETTE_ENV`]；
/// **未设置 ⇒ `None`**（[`crate::stamp_panel::DEFAULT_PALETTE`] 恒为空串 = 没有内建缺省路径）。
fn palette_path() -> Option<std::path::PathBuf> {
    std::env::var_os(crate::stamp_panel::PALETTE_ENV)
        .map(std::path::PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// 打开图库（与 GUI 同一条 [`mir2_assets::client_root`] 路径）。
///
/// 未设置 `MIREDIT_CLIENT_ROOT`（或它不是目录）⇒ `None`，调用方据此打印跳过原因。
fn open_source() -> Option<WilSource> {
    let root = mir2_assets::client_root()?;
    let paths = LibPaths::discover(&root).ok()?;
    WilSource::open(&paths).ok()
}

/// 包围盒 → `(x0,y0,x1,y1)`（诊断行用；这里不经过 `ui_text`，因为它是纯数值元组，
/// 不是给人读的文案）。
fn fmt_bounds(b: (i32, i32, i32, i32)) -> String {
    format!("({},{},{},{})", b.0, b.1, b.2, b.3)
}

/// 把一枚 `.X` 的**三族 (库, 图号)** 列成一行（只用于"取不到帧"的样本定位）。
///
/// 目的：区分"库号越界（客户端没这个库）"与"库在、但图号超出该库帧数"——
/// 两者的处置完全不同（前者要补库，后者是数据错）。
///
/// 行尾额外报**第一对 (库,图号) 的直接取帧结果**（`Some(宽×高)` / `None`）：
/// 这样"渲染器报缺帧"与"图源本身取不到"能被分开验证，不用靠猜。
fn layer_probe(path: &std::path::Path, src: &WilSource) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let stamp = Stamp::parse(&bytes).ok()?;
    let mut bk = Vec::new();
    let mut mid = Vec::new();
    let mut fr = Vec::new();
    for c in &stamp.cells {
        if c.cell.back_image & 0x7FFF != 0 {
            bk.push(format!("{}/{}", c.cell.back_library, c.cell.back_image & 0x7FFF));
        }
        if c.cell.middle_image & 0x7FFF != 0 {
            mid.push(format!("{}/{}", c.cell.middle_library, c.cell.middle_image & 0x7FFF));
        }
        if let Some(f) = c.cell.foreground {
            if (f.image as u16) & 0x7FFF != 0 {
                fr.push(format!("{}/{}", f.library, (f.image as u16) & 0x7FFF));
            }
        }
    }
    let 头 = |v: &[String]| v.iter().take(3).cloned().collect::<Vec<_>>().join(",");
    // 直接问图源：物件族第一格那一帧到底取不取得到（0 基索引 = 图号 - 1）
    let 探针 = stamp
        .cells
        .iter()
        .find_map(|c| c.cell.foreground)
        .map(|f| {
            let lib = mir2_assets::LibRef::surface(u16::try_from(f.library).unwrap_or(0));
            let idx = u32::from((f.image as u16) & 0x7FFF).saturating_sub(1);
            match src.frame_of(lib, idx) {
                Some(fr) => format!("取帧 Some({}×{})", fr.width, fr.height),
                None => format!("取帧 None（库 {lib} 帧号 {idx}）"),
            }
        })
        .unwrap_or_else(|| "无物件层".to_owned());
    Some(format!(
        "地表库/图号 [{}] 中层 [{}] 物件 [{}] · {探针}",
        头(&bk),
        头(&mid),
        头(&fr)
    ))
}

/// LRU 压测结果。
struct Drill {
    /// 前进了几页。
    fwd: usize,
    /// 回滚了几页。
    back: usize,
    /// 纹理数量峰值。
    peak: usize,
    /// 真渲染次数。
    decodes: u64,
    /// 缓存命中次数（含回滚时命中的）。
    hits: u64,
    /// 淘汰次数。
    evictions: u64,
    /// **回滚阶段**的命中次数（"重用时命中缓存"的直接证据）。
    back_hits: u64,
}

/// 按面板的同一套规则（每页 96、可见 20、LRU 上限 256）走一遍：
/// 前进 `fwd` 页 → 回滚到第 1 页，统计峰值 / 命中 / 淘汰。
///
/// 用的是**面板那份 [`crate::stamp_panel::LruCache`]**（只把 payload 换成 `bool`）：
/// 探针不开窗、也不该为压测去申请显存，但**淘汰规则与容量必须是同一份代码** ——
/// 否则"探针报的峰值"与"面板真实行为"就是两回事。
fn lru_drill(catalog: &mut StampCatalog, src: &WilSource) -> Drill {
    let per_page = m::STAMP_PAGE_SIZE;
    let visible = 20usize; // 一屏 = 5 行 × 4 列（见 metrics::STAMP_CELL_H 的推导）
    let mut cache: crate::stamp_panel::LruCache<bool> = crate::stamp_panel::LruCache::new();
    let mut decodes = 0u64;
    let mut hits = 0u64;
    let mut back_hits = 0u64;

    let fwd = 50usize;
    let back = 50usize;
    let mut 遍历 = |page: usize, 计数命中: &mut u64| {
        let start = page * per_page;
        let end = (start + per_page).min(catalog.len());
        // 只渲染"可见"的那些（一页 96 枚，一屏只看得到 20 枚）
        for i in start..(start + visible).min(end) {
            let id = catalog.all()[i].id.clone();
            if cache.get(&id).is_some() {
                *计数命中 += 1;
                continue;
            }
            // 真渲染（与面板 `ensure` 同一条 `render_thumb`）
            let ok = catalog
                .resolve(&id)
                .map(|r| render_thumb(&r.path, src).is_ok())
                .unwrap_or(false);
            decodes += 1;
            cache.insert(id, ok);
        }
    };

    for page in 0..fwd {
        遍历(page, &mut hits);
    }
    for page in (0..back).rev() {
        遍历(page, &mut back_hits);
    }
    Drill {
        fwd,
        back,
        peak: cache.peak(),
        decodes,
        hits: hits + back_hits,
        evictions: cache.evictions(),
        back_hits,
    }
}
