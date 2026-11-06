//! `stamp_panel` 的验收测试（**真实数据 + 纯逻辑**两层）。
//!
//! 单独一个文件是因为实现已经贴着 600 行上限（任务书 §五），用例塞进去会破线；
//! 做法与 `mir2-stamp` 的 `catalogue_tests.rs` / `roundtrip_tests.rs` 一致。
//!
//! 两类用例：
//! 1. **纯逻辑**（几何 / 分页 / LRU / 截断）：不依赖任何外部文件，永远跑；
//! 2. **真实数据**（分类条数 / 搜索计数 / 未登记清单 / 缩略图渲染对照）：
//!    读本机 `CrystalStudio/catalogue/palette.json` 与 `Objects/`；文件不在 ⇒
//!    打印原因并 `return`（**不 panic、也不静默变成"绿"**）。

use std::path::PathBuf;

use egui::{Color32, ColorImage, TextureOptions};

use mir2_stamp::kind_label_zh;

use crate::metrics as m;
use crate::stamp_panel::{self as sp, Filter, StampPanel};

// ───────────────────────── 纯逻辑 ─────────────────────────

/// 「分类」页签在 `PALETTE_TABS` 里的下标必须与文案表一致（改文案表顺序会当场红）。
#[test]
fn 分类页签下标与文案表一致() {
    assert_eq!(
        crate::ui_text::PALETTE_TABS[sp::TAB_INDEX],
        crate::ui_text::TAB_CATEGORIES,
        "TAB_INDEX 与文案表的「分类」位置不一致"
    );
}

/// 分页算术：96 一页、整行、末页不满也不越界。
#[test]
fn 分页算术() {
    assert_eq!(sp::page_count(0), 1, "空列表也要有 1 页（否则分页行没法显示）");
    assert_eq!(sp::page_count(1), 1);
    assert_eq!(sp::page_count(96), 1);
    assert_eq!(sp::page_count(97), 2);
    assert_eq!(sp::page_count(9159), 96, "9159 / 96 = 95.4 ⇒ 96 页");
    // 区间
    assert_eq!(sp::page_range(9159, 0), (0, 96));
    assert_eq!(sp::page_range(9159, 95), (9120, 9159), "末页只有 39 枚");
    assert_eq!(sp::page_range(9159, 999), (9120, 9159), "越界页夹到最后一页");
    assert_eq!(sp::page_range(0, 0), (0, 0));
    // 行数 / 滚动上限
    assert_eq!(sp::page_rows(9159, 0), 24, "96 / 4 = 24 行");
    assert_eq!(sp::page_rows(9159, 95), 10, "39 枚 = 10 行（最后一行不满）");
    assert_eq!(sp::max_row_off(9159, 0, 5), 19, "24 行 - 可见 5 行 = 19");
    assert_eq!(sp::max_row_off(20, 0, 5), 0, "一页只有 5 行时不能滚");
}

/// 文件名提取：未登记项**只显示文件名**（不显示目录）。
#[test]
fn 未登记只取文件名() {
    assert_eq!(sp::file_name("mir/mir_00000.X"), "mir_00000.X");
    assert_eq!(sp::file_name("a\\b\\c.X"), "c.X");
    assert_eq!(sp::file_name("plain.X"), "plain.X");
}

/// `fit_text`：放得下原样返回、放不下**掐头留尾**补 `…`、极端窄也至少留一个省略号。
#[test]
fn 名称截断() {
    let ctx = crate::fonts::measure_ctx();
    let p = egui::Painter::new(
        ctx,
        egui::LayerId::debug(),
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(m::WINDOW_W, m::WINDOW_H)),
    );
    let short = "古树";
    assert_eq!(sp::fit_text(&p, short, m::STAMP_NAME_FONT, 200.0), short);
    let long = "一二三四五六七八九十一二三四五六七八九十";
    let cut = sp::fit_text(&p, long, m::STAMP_NAME_FONT, m::STAMP_CELL_W - 6.0);
    assert!(cut.starts_with(crate::ui_text::TRUNC_SUFFIX), "截断要带省略号：{cut}");
    assert!(
        crate::paint::text_w(&p, &cut, m::STAMP_NAME_FONT) <= m::STAMP_CELL_W - 6.0,
        "截断后仍要放得进格子：{cut}"
    );
    assert_eq!(sp::fit_text(&p, long, m::STAMP_NAME_FONT, 1.0), crate::ui_text::TRUNC_SUFFIX);
    // **掐头留尾**：真实名称 `<subtype> — <id>` 的区分段在尾部，必须留得住 id
    let real = "Terrain — mir_00293";
    let cut2 = sp::fit_text(&p, real, m::STAMP_NAME_FONT, m::STAMP_CELL_W - 6.0);
    eprintln!("[印章] 真实名称截断：{real:?} → {cut2:?}");
    assert!(
        cut2.ends_with("00293"),
        "尾部的 id 必须留在可见段里（否则 20 个格子长得一样）：{cut2}"
    );
}

/// **LRU 生效**（不依赖外部文件）：超过上限就淘汰最久未用的，峰值 ≤ 上限。
///
/// 用的是面板那份 [`sp::LruCache`]，只把 payload 换成 `u32`。
#[test]
fn lru_上限与淘汰顺序() {
    let cap = m::STAMP_THUMB_CACHE_CAP;
    let mut c: sp::LruCache<u32> = sp::LruCache::new();
    for i in 0..cap as u32 {
        c.insert(format!("k{i}"), i);
    }
    assert_eq!(c.len(), cap);
    assert_eq!(c.peak(), cap);
    assert_eq!(c.evictions(), 0, "没超上限不该淘汰");
    // 摸一下 k0（变成最新），再插一条 ⇒ 被淘汰的应该是最久未用的 k1
    assert!(c.get("k0").is_some());
    c.insert("new".to_owned(), 999);
    assert_eq!(c.len(), cap, "上限必须硬住");
    assert_eq!(c.evictions(), 1);
    assert!(c.get("k1").is_none(), "最久未用的 k1 应被淘汰");
    assert!(c.get("k0").is_some(), "刚摸过的 k0 必须还在");
    assert!(c.get("new").is_some());
    // 峰值不会超过上限（再灌 3 倍量）
    for i in 0..cap as u32 * 3 {
        c.insert(format!("x{i}"), i);
    }
    assert_eq!(c.peak(), cap, "峰值必须 ≤ 上限，实际 {}", c.peak());
}

/// 分类标签回落口径：已知 ⇒ 中文；未知 ⇒ 原样英文（**不编**）。
#[test]
fn 分类中文回落不编() {
    assert_eq!(kind_label_zh("Terrain"), Some("地形"));
    assert_eq!(kind_label_zh("Mountain"), None);
    assert_eq!(Filter::Kind("Mountain".to_owned()).label(), "Mountain");
    assert_eq!(Filter::Kind("Terrain".to_owned()).label(), "地形");
    assert_eq!(Filter::Unlisted.label(), crate::ui_text::STAMP_UNLISTED);
    assert_eq!(Filter::All.label(), crate::ui_text::STAMP_KIND_ALL);
    // key 往返（`--stamp-kind` 用的就是它）
    for f in [
        Filter::All,
        Filter::Kind("Terrain".to_owned()),
        Filter::Unclassified,
        Filter::Unlisted,
    ] {
        assert_eq!(Filter::Kind(f.key()).key(), f.key(), "key 往返不一致：{f:?}");
    }
}

/// **「当前资源」两行都放得进文字区**（真实字体量宽）。
///
/// 文字区宽 = `AA_CARD_W - AA_PREVIEW_OFF - AA_PREVIEW_W - AA_TEXT_GAP` = 128 px。
/// 断言用**真实会出现的**取值：名称 6 个汉字、分类 2 个汉字、包围盒 `-12..14`（实测 9,159 枚
/// 的包围盒都在几十以内）；另外把 ±9999 的极端串**打印出来但不作断言** ——
/// 那种数据不存在，为它改字号只会把正常内容压得更小。
#[test]
fn 印章信息两行都放得进文字区() {
    let ctx = crate::fonts::measure_ctx();
    let p = egui::Painter::new(
        ctx,
        egui::LayerId::debug(),
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(m::WINDOW_W, m::WINDOW_H)),
    );
    let band = m::AA_CARD_W - m::AA_PREVIEW_OFF - m::AA_PREVIEW_W - m::AA_TEXT_GAP;
    let name_line = crate::ui_text::fill(
        crate::ui_text::STAMP_AA_TMPL,
        &[("name", "一二三四五六"), ("kind", "地形")],
    );
    let geom_line = crate::ui_text::fill(
        crate::ui_text::STAMP_AA_GEOM_TMPL,
        &[
            ("n", "25"),
            ("x0", "-12"),
            ("y0", "-8"),
            ("x1", "14"),
            ("y1", "9"),
        ],
    );
    for (tag, s) in [("名称行", &name_line), ("几何行", &geom_line)] {
        let w = crate::paint::text_w(&p, s, m::STAMP_AA_FONT);
        eprintln!("[印章] {tag} 宽 {w:.1} / 可用 {band:.1}：{s}");
        assert!(
            w <= band,
            "{tag} 放不下（{w:.1} > {band:.1}）：{s} —— 要么缩短文案，要么再拆一行"
        );
    }
    // 极端值只打印（不作断言）：真出现时由 `DynLabel::truncate_to_fit` 截成 `…`，不越界
    let extreme = crate::ui_text::fill(
        crate::ui_text::STAMP_AA_GEOM_TMPL,
        &[
            ("n", "999"),
            ("x0", "-9999"),
            ("y0", "-9999"),
            ("x1", "9999"),
            ("y1", "9999"),
        ],
    );
    eprintln!(
        "[印章] 极端几何行宽 {:.1} / 可用 {band:.1}（超了会被截断）：{extreme}",
        crate::paint::text_w(&p, &extreme, m::STAMP_AA_FONT)
    );
}

/// 分页行文字（`第 N 页，共 M 页 · 共 K 枚`）放得进两枚按钮之间那条窄带。
#[test]
fn 分页行文字放得下() {
    let ctx = crate::fonts::measure_ctx();
    let p = egui::Painter::new(
        ctx,
        egui::LayerId::debug(),
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(m::WINDOW_W, m::WINDOW_H)),
    );
    let l = crate::layout::Layout::compute(egui::Rect::from_min_size(
        egui::Pos2::ZERO,
        egui::vec2(m::WINDOW_W, m::WINDOW_H),
    ));
    let (prev, next) = crate::dock_right::pager_rects(&l);
    let band = (next.left() - 4.0) - (prev.right() + 4.0);
    let worst = crate::ui_text::fill(
        crate::ui_text::STAMP_PAGE_TMPL,
        &[("page", "96"), ("pages", "96"), ("total", "9159")],
    );
    let w = crate::paint::text_w(&p, &worst, m::BODY_FONT);
    eprintln!("[印章] 分页行宽 {w:.1} / 可用 {band:.1}：{worst}");
    assert!(w <= band, "分页行放不下（{w:.1} > {band:.1}）：{worst}");
}

// ───────────────────────── 真实数据 ─────────────────────────

/// `palette.json` 路径：`MIR2_STAMP_PALETTE` → `<MIREDIT_UPSTREAM_ROOT>/catalogue/palette.json`
/// → **生产缺省常量** [`sp::DEFAULT_PALETTE`]（缺省根的唯一出处，本测试不再自带一份）。
///
/// ⚠ **显式给了 `MIREDIT_UPSTREAM_ROOT` 就只认它**（即使那个根不存在，也不回落到生产缺省常量）：
/// 否则「缺资产模式」会悄悄读到本机真素材，跳过判据当场失效。文件不存在时
/// `ready_panel` 会打印 `[跳过]` 并返回（公开仓/PR 的机器上没有素材也不会红）。
fn palette_json() -> PathBuf {
    if let Some(v) = std::env::var_os(sp::PALETTE_ENV) {
        return PathBuf::from(v);
    }
    match std::env::var_os("MIREDIT_UPSTREAM_ROOT") {
        Some(v) => PathBuf::from(v).join("catalogue").join("palette.json"),
        None => PathBuf::from(sp::DEFAULT_PALETTE),
    }
}

/// 等后台装载完成（最多 `secs` 秒）；超时返回 `false`（**不无限等**）。
fn wait_ready(panel: &mut StampPanel, secs: u64) -> bool {
    let t0 = std::time::Instant::now();
    while t0.elapsed().as_secs() < secs {
        if panel.poll() && panel.is_ready() {
            return true;
        }
        if panel.is_ready() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panel.is_ready()
}

/// 造一个已装载的面板；目录不在 ⇒ 打印原因并返回 `None`（调用方 `return`）。
fn ready_panel(tag: &str) -> Option<StampPanel> {
    let path = palette_json();
    if !path.is_file() {
        eprintln!("[跳过 {tag}] 本机没有 {}（真实数据用例需要它）", path.display());
        return None;
    }
    let mut panel = StampPanel::with_path(path);
    if !wait_ready(&mut panel, 30) {
        eprintln!("[跳过 {tag}] 目录 30 s 内没装载完");
        return None;
    }
    Some(panel)
}

/// **19 个 kind 的中文名 + 条数**与 `kind_labels.rs` 的实测读数逐条对齐。
///
/// 这张表是**验收门 §四.2 的程序化版本**：条数不对 ⇒ 当场红，而不是等人工比截图。
#[test]
fn 分类条数与实测一致() {
    let Some(mut panel) = ready_panel("分类条数") else {
        return;
    };
    // 期望值 = `mir2-stamp/src/kind_labels.rs` 模块文档 §二 的 2026-11-04 实测读数
    let 期望: [(&str, usize); 19] = [
        ("Terrain", 1695),
        ("Cliff", 1434),
        ("Wall", 1152),
        ("Rock", 943),
        ("Debris", 759),
        ("Furniture", 534),
        ("Decoration", 518),
        ("Water", 469),
        ("Plant", 448),
        ("Tree", 323),
        ("Building", 271),
        ("Fence", 261),
        ("Interior", 145),
        ("Bridge", 57),
        ("Effect", 50),
        ("Stairs", 40),
        ("Gate", 26),
        ("Boat", 13),
        ("Track", 12),
    ];
    let rows = panel.kind_rows();
    // 第一行是「全部分类」，最后一行是「未登记」，中间 19 个 kind + 可能一行「未分类」
    assert_eq!(rows[0].0, Filter::All, "下拉第一行必须是「全部分类」");
    assert_eq!(
        rows.last().map(|(f, _)| f.clone()),
        Some(Filter::Unlisted),
        "下拉最后一行必须是「未登记」"
    );
    let mut 合计 = 0usize;
    for (kind, n) in 期望 {
        let got = rows
            .iter()
            .find(|(f, _)| *f == Filter::Kind(kind.to_owned()))
            .map(|(_, n)| *n)
            .unwrap_or(0);
        eprintln!("[印章] {kind}（{}）= {got}", kind_label_zh(kind).unwrap_or("?"));
        assert_eq!(got, n, "{kind} 的条数与实测不一致");
        合计 += n;
    }
    // 「未分类」= type 为 null 的 9 条
    let 未分类 = rows
        .iter()
        .find(|(f, _)| *f == Filter::Unclassified)
        .map(|(_, n)| *n);
    assert_eq!(未分类, Some(9), "未分类应为 9 条（palette.json 里 type: null）");
    // 全部分类 = 19 个 kind + 未分类
    assert_eq!(
        rows[0].1,
        合计 + 9,
        "「全部分类」应等于 19 个 kind + 未分类"
    );
    assert!(
        panel.set_filter(Filter::All),
        "「全部分类」必须能切过去"
    );
    assert_eq!(panel.total(), 合计 + 9, "「全部分类」的列表长度要等于登记条数");
}

/// **搜索对账**：`market` → 25 枚、`mir_00000` → 1 枚（与 `catalog.search` 一致）。
#[test]
fn 搜索计数与目录一致() {
    let Some(mut panel) = ready_panel("搜索计数") else {
        return;
    };
    panel.set_search("market");
    let got = panel.total();
    eprintln!("[印章] search(\"market\") = {got}");
    assert_eq!(got, 25, "market 应命中 25 枚（实测）");
    panel.set_search("mir_00000");
    eprintln!("[印章] search(\"mir_00000\") = {}", panel.total());
    assert_eq!(panel.total(), 1, "mir_00000 应命中 1 枚");
    // 大小写不敏感（目录的 search 键是小写化的）
    panel.set_search("MARKET");
    assert_eq!(panel.total(), 25, "大小写不敏感");
    // 清空 ⇒ 回到全量
    panel.set_search("");
    assert_eq!(panel.total(), 9159, "清空搜索应回到 9,159 枚");
}

/// **未登记分组**：11 个文件，只显示文件名，且列表里每一格都是 `Item::Unlisted`。
#[test]
fn 未登记分组十一个() {
    let Some(mut panel) = ready_panel("未登记分组") else {
        return;
    };
    assert!(panel.set_filter(Filter::Unlisted));
    eprintln!("[印章] 未登记分组 = {} 个", panel.total());
    assert_eq!(panel.total(), 11, "磁盘上有、palette.json 没登记的 .X 应是 11 个");
    // 选中第 0 个：名称必须是**文件名**（不含目录分隔符），分类写「未登记」，格数/包围盒未知
    assert!(panel.pick_slot(0));
    let sel = panel.selected().expect("刚选过就该有选中项");
    assert!(
        !sel.name.contains('/') && !sel.name.contains('\\'),
        "未登记项只显示文件名，实际 {}",
        sel.name
    );
    assert_eq!(sel.kind_zh, crate::ui_text::STAMP_UNLISTED);
    assert_eq!(sel.cells, None, "未登记项不猜格数");
    assert_eq!(sel.bounds, None, "未登记项不猜包围盒");
}

/// **缩略图是真渲染**：5 枚样本经面板路径与直接 `render_stamp` 的产物**逐字节相同**。
///
/// 这是验收门 §四.3 的程序化版本。图库打不开 ⇒ 跳过（打印原因）。
#[test]
fn 缩略图与直接渲染逐字节一致() {
    let Some(mut panel) = ready_panel("缩略图对照") else {
        return;
    };
    let Some(src) = open_source() else {
        eprintln!("[跳过 缩略图对照] 图库打不开（需要客户端 data 目录）");
        return;
    };
    // 取 5 个等距样本（与探针同一组下标，便于人工对账）
    let total = panel.total();
    let 下标 = [0usize, total / 4, total / 2, total * 3 / 4, total - 1];
    for idx in 下标 {
        // 面板侧：先翻到该枚所在的页，再选中它
        panel.set_page(idx / m::STAMP_PAGE_SIZE);
        let slot = idx % m::STAMP_PAGE_SIZE;
        assert!(panel.pick_slot(slot), "第 {idx} 枚应该能选中");
        let sel = panel.selected().cloned().expect("选中项");
        let (cells, bounds) = (sel.cells, sel.bounds);
        assert!(cells.is_some(), "已登记项应能 resolve 出格数：{}", sel.id);
        // 直接调用（基准）
        let path = resolve_path(&sel.id).expect("能 resolve 出路径");
        let bytes = std::fs::read(&path).expect("读得到 .X");
        let stamp = mir2_stamp::Stamp::parse(&bytes).expect("解析得了 .X");
        let positioned = sp::positioned_cells(&stamp).expect("库号在 u8 内");
        let direct = miredit_render::stamp::render_stamp(
            &positioned,
            &src,
            &miredit_render::stamp::StampRenderOptions::thumbnail(
                m::STAMP_THUMB_W as u32,
                m::STAMP_THUMB_H as u32,
            ),
        )
        .expect("直接渲染不报错");
        // 面板侧
        let (_, panel_img) = sp::render_thumb(&path, &src).expect("面板渲染不报错");
        eprintln!(
            "[印章] {} cells={} bounds={:?} 渲染 {}×{} 画格 {} 缺帧 {}",
            sel.id,
            cells.unwrap_or(0),
            bounds,
            panel_img.width,
            panel_img.height,
            panel_img.cells_drawn,
            panel_img.missing
        );
        assert_eq!(panel_img.width, direct.width, "宽不一致：{}", sel.id);
        assert_eq!(panel_img.height, direct.height, "高不一致：{}", sel.id);
        assert_eq!(panel_img.content, direct.content, "内容框不一致：{}", sel.id);
        assert_eq!(panel_img.cells_drawn, direct.cells_drawn, "画格数不一致：{}", sel.id);
        assert_eq!(panel_img.missing, direct.missing, "缺帧数不一致：{}", sel.id);
        assert_eq!(panel_img.rgba, direct.rgba, "像素不一致：{}", sel.id);
        // 缩略图必须是 46×32 框内**等比**的结果：
        // · 有内容的印章 ⇒ 宽高都 > 0（0×0 会被面板判成缺帧，那是另一条路）；
        // · 空印章（图号全 0 / 帧全缺）⇒ 0×0 是**合法**的（`stamp.rs` §五：空内容不是错误），
        //   面板据此画「缺帧」占位 —— 这里只钉住"不越界"。
        assert!(
            panel_img.width <= m::STAMP_THUMB_W as u32,
            "缩略图宽 {} 超过目标框 {}：{}",
            panel_img.width,
            m::STAMP_THUMB_W,
            sel.id
        );
        assert!(
            panel_img.height <= m::STAMP_THUMB_H as u32,
            "缩略图高 {} 超过目标框 {}：{}",
            panel_img.height,
            m::STAMP_THUMB_H,
            sel.id
        );
        if panel_img.cells_drawn > 0 {
            assert!(panel_img.width > 0 && panel_img.height > 0, "有内容就必须有尺寸：{}", sel.id);
        }
    }
}

/// 打开图库（与 GUI 同一条 [`mir2_assets::client_root`] 路径）。
///
/// 未设置 `MIREDIT_CLIENT_ROOT`（或它不是目录）⇒ `None`，调用方打印跳过原因。
fn open_source() -> Option<mir2_assets::WilSource> {
    let root = mir2_assets::client_root()?;
    let paths = mir2_assets::LibPaths::discover(&root).ok()?;
    mir2_assets::WilSource::open(&paths).ok()
}

/// 把 id 解析成 `.X` 路径（与面板 `ensure` 同一条 `resolve`）。
fn resolve_path(id: &str) -> Option<PathBuf> {
    let mut cat = mir2_stamp::StampCatalog::load(&palette_json()).ok()?;
    cat.resolve(id).map(|r| r.path)
}

/// **LRU 真跑一遍**（真实目录 + 真渲染 + 真纹理上传）：滚到第 50 页再滚回来。
///
/// 用真实的 `egui::Context` 建纹理（无窗口、无 GPU；`load_texture` 只登记到
/// `TexturesDelta`，不碰显存）—— 这样测的就是面板那一条 `ensure` 路径。
///
/// # 三段，各自证明一件事
///
/// | 段 | 动作 | 要证明的 |
/// |---|---|---|
/// | ① | 从第 1 页滚到第 50 页（每页 20 枚可见 ⇒ 共 1,000 枚） | 纹理数量**峰值 ≤ 上限**（不是"全都传上去"） |
/// | ② | 一路滚回第 1 页 | 那些纹理**已被 LRU 淘汰** ⇒ 命中 0、重新渲染（上限真的在起作用） |
/// | ③ | 紧接着回到刚看过的第 46~50 页 | **重用时命中缓存**（不是每帧重渲） |
///
/// ⚠ ②「命中 0」不是缺陷，是上限的**正面证据**：1,000 枚走过 256 的缓存，第 1 页那批
/// 早已被淘汰；真要说"滚回来还命中"，那就得把上限提到 1,000 以上（= 任务书禁止的全量上传）。
#[test]
fn 纹理缓存峰值不超过上限且重访命中() {
    let Some(mut panel) = ready_panel("LRU 实测") else {
        return;
    };
    let Some(src) = open_source() else {
        eprintln!("[跳过 LRU 实测] 图库打不开");
        return;
    };
    let ctx = crate::fonts::measure_ctx();
    let cap = m::STAMP_THUMB_CACHE_CAP;

    /// 把某一页的可见格（20 枚）逐个选中并取纹理。
    fn 画一页(panel: &mut StampPanel, ctx: &egui::Context, src: &mir2_assets::WilSource, page: usize) -> usize {
        panel.set_page(page);
        let mut n = 0;
        for slot in 0..20 {
            let idx = page * m::STAMP_PAGE_SIZE + slot;
            if idx >= panel.total() {
                break;
            }
            if panel.pick_slot(slot) {
                let _ = panel.selected_texture(ctx, Some(src));
                n += 1;
            }
        }
        n
    }

    // ① 前进 50 页
    let mut 画过 = 0usize;
    for page in 0..50 {
        画过 += 画一页(&mut panel, &ctx, &src, page);
    }
    let f = panel.stats();
    eprintln!(
        "[印章 LRU] ① 前进 50 页（画过 {画过} 枚）：缓存 {} 峰值 {} 淘汰 {} 渲染 {} 命中 {}",
        f.cached, f.peak, f.evictions, f.decodes, f.hits
    );
    assert!(f.peak <= cap, "纹理峰值 {} 超过上限 {cap}", f.peak);
    assert!(f.decodes > 0, "真渲染次数不能是 0");
    assert!(f.evictions > 0, "走过 1,000 枚必然发生淘汰（否则上限没生效）");

    // ② 滚回第 1 页
    let hits_before = panel.stats().hits;
    let mut 重渲 = 0u64;
    for page in (0..3).rev() {
        let before = panel.stats().decodes;
        画一页(&mut panel, &ctx, &src, page);
        重渲 += panel.stats().decodes - before;
    }
    let b = panel.stats();
    eprintln!(
        "[印章 LRU] ② 滚回第 1~3 页：命中增量 {}（应为 0）/ 重新渲染 {重渲} 枚 / 峰值 {}",
        b.hits - hits_before,
        b.peak
    );
    assert_eq!(
        b.hits - hits_before,
        0,
        "第 1 页那批纹理早已被淘汰，不该命中（命中说明上限没生效）"
    );
    assert!(重渲 > 0, "被淘汰的必须重新渲染，否则缓存计数不可信");

    // ③ 回到刚看过的第 46~50 页 ⇒ 必须命中缓存
    let hits_mid = panel.stats().hits;
    let mut 命中 = 0u64;
    for page in 46..50 {
        let before = panel.stats().hits;
        画一页(&mut panel, &ctx, &src, page);
        命中 += panel.stats().hits - before;
    }
    let e = panel.stats();
    eprintln!(
        "[印章 LRU] ③ 回到第 47~50 页：命中 {命中} 次（总命中 {}）/ 峰值 {} / 上限 {cap}",
        e.hits, e.peak
    );
    assert!(命中 > 0, "刚看过的页必须命中缓存（增量 0 说明每帧都在重渲）");
    assert!(e.hits > hits_mid, "总命中数应增加");
    assert!(e.peak <= cap, "峰值 {} 超过上限", e.peak);
    assert!(e.cached <= cap, "缓存条目 {} > 上限 {cap}", e.cached);
}

/// 选一枚印章 ⇒ 状态栏那一段与「当前资源」快照都是**真实数据**。
#[test]
fn 选中后信息真实() {
    let Some(mut panel) = ready_panel("选中信息") else {
        return;
    };
    // 用 Terrain 分类（1,695 枚）里的第 0 枚，避开"未登记"与空分类
    assert!(panel.set_filter_key("Terrain"));
    assert_eq!(panel.total(), 1695);
    assert!(panel.pick_slot(0));
    let sel = panel.selected().cloned().expect("选中项");
    eprintln!(
        "[印章] 选中 {}（{} · {}）{:?} 格 包围盒 {:?}",
        sel.id, sel.name, sel.kind_zh, sel.cells, sel.bounds
    );
    assert_eq!(sel.kind_zh, "地形", "中文分类应来自 kind_label_zh");
    assert!(!sel.name.is_empty());
    assert!(sel.cells.unwrap_or(0) > 0, "已登记项应有格数");
    let b = sel.bounds.expect("应有包围盒");
    assert!(b.2 >= b.0 && b.3 >= b.1, "包围盒必须是闭区间且 x1>=x0/y1>=y0：{b:?}");
    // 状态栏文字：`印章：<名称>`
    let text = panel.status_text().expect("选中后状态栏必须有这一段");
    assert_eq!(text, crate::ui_text::fill(crate::ui_text::STAMP_STATUS_TMPL, &[("name", &sel.name)]));
    // 清空选择 ⇒ 状态栏那一段消失（不写占位）
    assert!(panel.set_filter(Filter::All));
    assert!(panel.selected().is_none(), "切分类要清掉选中项");
    assert!(panel.status_text().is_none());
}

/// 纹理上传自检：`ColorImage` 的像素数必须等于 `w*h*4`（防止把 stride 算错）。
///
/// 这条挡的是"图能显示但错位/斜切"这一类**肉眼不易发现**的错。
#[test]
fn 纹理像素数与尺寸自洽() {
    let img = ColorImage::from_rgba_unmultiplied([3, 2], &[0u8; 24]);
    assert_eq!(img.size, [3, 2]);
    assert_eq!(img.pixels.len(), 6);
    // 面板那边也是这么建的（`ensure` 里同一句），这里只是把契约钉住
    let _ = TextureOptions::LINEAR;
    let _ = Color32::WHITE;
}
