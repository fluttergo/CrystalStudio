//! 九格式验收：**门 2**（全仓 1,158 张开图）、**门 3**（未改即存 0 字节差异）、
//! **门 4**（Type5 走真装载路径 + 地表落笔被前端拦住）、**门 5**（67 张带尾块图的黄条）。
//!
//! # 一、为什么这几门必须走 `DocState` / `MapDoc`，不能走 `mir2_map::Map::parse`
//!
//! 2026-11-05 之前 GUI 的装载路径是 `mir2_map::Map::parse` —— 它只认 12 / 14 / 36 三档，
//! Type5 / Type1 / Type4 / Mir3 / CrystalCs 一律 `LengthMismatch`，67 张带尾块的真实图
//! 也一律拒绝。本模块验的就是"换成 `miredit_core::Document::load`（内部
//! `mir2_format::read_any_tolerant`）之后，九种格式真的能开、真的能原样存回去"。
//!
//! # 二、门 2 / 门 3 是 `#[ignore]`（默认不跑）
//!
//! 1,158 张图 × (解析 + 出字节 + 逐字节比对) 在本机约 13 秒（release 档），
//! debug 档约 1~2 分钟；它属于**交付期一次**的全量重活，所以显式跑：
//!
//! ```text
//! cargo test -p miredit-gui --lib --release -- --ignored --nocapture 门2门3
//! ```
//!
//! 它们**不是"跳过"**：`#[ignore]` 的用例在交付时是显式跑的，跑完打印完整统计表。
//!
//! # 二·五、门 2 只覆盖 3 种格式 —— 九种靠合成样本补齐
//!
//! 本机 1,158 张真实图实测只有 **`Old12` 856 / `Shanda14` 107 / `Shanda2012` 195**，
//! Type1 / Type4 / Mir3 / CrystalCs **一张都没有**。所以另有一条
//! [`九格式合成样本_全部能开且原样存回`]：九种各造一张（构造口径逐字对齐
//! `mir2-format/src/formats/*.rs` 各自的 `造图`），逐张走真装载路径打开 + 出字节比对。
//!
//! # 三、门 4 为什么不再是"注入验证"
//!
//! 上一版（`selection.rs` 的门 6）用 `EditSession::inject_document` 把 Type5 档位**注入**
//! 会话，因为 GUI 根本打不开 Type5 文件。现在 Type5 能开了 ⇒ 本模块改为
//! **真的写一个 Type5 文件到磁盘、走 [`crate::mapdoc::DocState::load`] 打开**，
//! 再验 ① 格式名是 `Mir3Wemade` ② `ground_paint_supported == false`
//! ③ 地表落笔在**前端**就被拦住（[`crate::app::ground_block_reason`]，一条命令都不发）。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::acceptance_tests::common::{资产根, 临时目录};
use crate::app::ground_block_reason;
use crate::edit::EditSession;
use crate::mapdoc::DocState;
use crate::screen_text;

// ───────────────────────── 全仓语料 ─────────────────────────

/// 语料目录一：客户端工程自带的 61 张（12 / 14 / 36 三档）：`<资产根>/GodotClient/maps`。
pub(super) fn 语料_a() -> Option<PathBuf> {
    Some(资产根()?.join("GodotClient").join("maps"))
}

/// 语料目录二：23 周年客户端的 1,097 张（含 36B 高清图与 67 张带尾块的图）。
pub(super) fn 语料_b() -> Option<PathBuf> {
    Some(资产根()?.join("client").join("热血传奇23周年客户端").join("Map"))
}

/// 带未解尾块的真实样本（头部声明 30×35 ⇒ `52 + 12×1050 = 12,652`，实际 20,692）。
pub(super) fn 尾块样本() -> Option<PathBuf> {
    let p = 语料_b()?.join("EM001.map");
    p.is_file().then_some(p)
}
/// 该样本的未解尾块字节数（`20,692 − 12,652`）。
pub(super) const 尾块字节数: usize = 8040;

/// 把两个语料目录里的 `.map` 全收进来（按路径排序，保证两次跑的顺序一致）。
fn 全部语料() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for dir in [语料_a(), 语料_b()].into_iter().flatten() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            eprintln!("[跳过] 语料目录不存在：{}", dir.display());
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) == Some("map") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// 一份 `DocState`（地图已开、图库固定失败态：逐字节比对与图库一个字节都不相干）。
fn 只有地图(path: &Path) -> Option<DocState> {
    let (doc, err) = DocState::load_map(path);
    doc.map(|d| DocState {
        doc: Some(d),
        assets: crate::mapdoc::Assets::Failed("验收不装载图库".to_string()),
        map_error: err,
        map_gen: 1,
    })
}

// ───────────────────────── 门 2 + 门 3 ─────────────────────────

/// **门 2 + 门 3**：1,158 张全部能开（失败 0），且「未改即存」逐字节差异 0。
///
/// 口径：
/// - 装载走 [`DocState::load_map`]（= GUI 的地图装载路径，内部 `Document::load`）；
/// - 出字节走 [`crate::mapdoc::MapDoc::to_bytes`]（= `Document::try_to_bytes`，
///   九格式各自 writer + 未解尾块原样带上），**不是** `map.to_bytes()`；
/// - 比对是**全文件逐字节** `memcmp`（长度不等也算差异）。
///
/// 打印：按格式统计张数 + 总字节 + 带尾块张数；失败时逐个列文件名与原因。
#[test]
#[ignore = "1,158 张全仓扫描：release 约 13 秒 / debug 约 1~2 分钟，交付期显式跑（--ignored）"]
fn 门2门3_全仓一千一百五十八张_开图零失败_未改即存零差异() {
    let files = 全部语料();
    if files.is_empty() {
        eprintln!("[跳过] 两个语料目录都不存在，本机没有可扫的语料");
        return;
    }

    let mut 格式统计: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut 总字节 = 0usize;
    let mut 带尾块 = 0usize;
    let mut 尾块字节合计 = 0usize;
    let mut 开图失败: Vec<(String, String)> = Vec::new();
    let mut 存回差异: Vec<(String, usize, usize, usize)> = Vec::new(); // (文件, 原长, 新长, 差异字节)

    for p in &files {
        let name = p
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let Some(mut st) = 只有地图(p) else {
            开图失败.push((
                name,
                st_err(p),
            ));
            continue;
        };
        let d = st.doc.as_mut().expect("刚构造的 DocState 必有文档");
        *格式统计.entry(d.format_label()).or_default() += 1;
        总字节 += d.bytes;
        if d.tolerant_trailer() > 0 {
            带尾块 += 1;
            尾块字节合计 += d.tolerant_trailer();
        }
        let 原 = std::fs::read(p).expect("读原文件");
        let 新 = match d.to_bytes() {
            Ok(b) => b,
            Err(e) => {
                eprintln!("[存回失败] {name}：{e}");
                存回差异.push((name, 原.len(), 0, usize::MAX));
                continue;
            }
        };
        let 差异 = 逐字节差异(&原, &新);
        if 差异 > 0 {
            存回差异.push((name, 原.len(), 新.len(), 差异));
        }
    }

    eprintln!("═══ 门 2 / 门 3 全仓统计 ═══");
    eprintln!(
        "语料目录：{} + {}",
        语料_a().map(|p| p.display().to_string()).unwrap_or_else(|| "<无资产根>".to_string()),
        语料_b().map(|p| p.display().to_string()).unwrap_or_else(|| "<无资产根>".to_string())
    );
    eprintln!("文件总数：{}（开图失败 {}）", files.len(), 开图失败.len());
    eprintln!("文件字节合计：{总字节}");
    eprintln!("带未解尾块：{带尾块} 张，尾块字节合计 {尾块字节合计}");
    eprintln!("按格式统计（张数）：");
    for (fmt, n) in &格式统计 {
        eprintln!("  {n:>5}  {fmt}");
    }
    eprintln!(
        "未改即存：有差异 {} 张（要求 0）",
        存回差异.len()
    );
    for (name, a, b, n) in &存回差异 {
        eprintln!("  [差异] {name}：原 {a} 字节 / 存 {b} 字节 / 不同 {n} 字节");
    }
    for (name, err) in &开图失败 {
        eprintln!("  [开图失败] {name}：{err}");
    }

    assert!(
        开图失败.is_empty(),
        "门 2：{} 张开图失败（明细见上）",
        开图失败.len()
    );
    assert!(
        存回差异.is_empty(),
        "门 3：{} 张未改即存出现字节差异（明细见上）",
        存回差异.len()
    );
    assert_eq!(
        格式统计.values().sum::<usize>(),
        files.len(),
        "每张图都必须被归到某个格式"
    );
}

/// 开图失败的具体原因（`DocState::load_map` 把原因放在第二个返回值里）。
fn st_err(p: &Path) -> String {
    match DocState::load_map(p) {
        (_, Some(e)) => e,
        _ => "未知（第二次装载却成功了，说明有不确定性）".to_string(),
    }
}

/// 两个字节串的**不同字节数**（长度不等时多出来的字节一律计差异）。
fn 逐字节差异(a: &[u8], b: &[u8]) -> usize {
    let mut n = a.len().abs_diff(b.len());
    for i in 0..a.len().min(b.len()) {
        if a[i] != b[i] {
            n += 1;
        }
    }
    n
}

// ───────────────────────── 门 4：Type5 走真装载路径 ─────────────────────────

/// 按 `mir3_wemade` 的格式规格造一张 `w×h` 的 Type5 字节流（**无真实样本**）。
///
/// 构造手法与 `mir2-format/src/formats/mir3_wemade.rs` 的 `造图` 测试同源：
/// 头部 28 字节（`bytes[0] == 0` 是无标题签名、`w` 在 22、`h` 在 24）+ 半分辨率背景块
/// `3·ceil(w/2)·floor(h/2)` + 每格 14 字节。
///
/// 全零字节的其它副作用：`Shanda14`/`Shanda2012` 要求 `bytes[4] == 0x0F`、
/// `CrystalCs` 要求 `bytes[2..4]` 是魔数、`Old12` 要求长度正好 `52 + 12·w·h` ——
/// 对本样本**全都不成立**，所以判别唯一命中 `Mir3Wemade`（测试里会断言这一点）。
fn 造type5(w: u16, h: u16) -> Vec<u8> {
    // Type5 的布局（`mir2-format/src/formats/mir3_wemade.rs:137-146` 逐字对齐）：
    //   头部 28 字节（`bytes[0] == 0` 是无标题签名、`w` 在偏移 22、`h` 在偏移 24）
    //   + 半分辨率背景块 `3 × ceil(w/2) × floor(h/2)`
    //   + 格数组 `14 × w × h`
    //
    // ⚠ 这里**不引 `mir2-format` 依赖**（宿主 crate 不直接依赖它，保持依赖图不变）：
    //    算式只有两行，抄错会在下面的 `format_label` 断言上当场红 —— 那比"悄悄多一条依赖边"好。
    let half_w = w as usize / 2 + w as usize % 2;
    let half_h = h as usize / 2;
    let cells_off = 28 + 3 * half_w * half_h;
    let n = cells_off + 14 * w as usize * h as usize;
    let mut b = vec![0u8; n];
    b[0] = 0; // 无标题
    b[22..24].copy_from_slice(&(w as i16).to_le_bytes());
    b[24..26].copy_from_slice(&(h as i16).to_le_bytes());
    b
}

/// **门 4**：合成 Type5 文件 → 真装载路径打开 → 格式名 / 地表能力 / 前端拦截。
///
/// 四条断言：
/// 1. 打开成功，`format_label()` 含 `Mir3Wemade`（判别真的命中 Type5，不是被别的格式抢走）；
/// 2. `ground_paint_supported() == false`；
/// 3. 地表层的落笔**在前端就被拦住**（[`ground_block_reason`] 给出原因），
///    且此时撤销栈 0 步、脏标记假、地图字节一字节未变（= **一条命令都没发**）；
/// 4. 反向对照：同一张 Type5 图，切到**中层** ⇒ 放行（拦的是"地表逐格"，不是整张图）。
///
/// 附加：把同一件事交给内核（直接 `touch`）时内核也拒绝 —— 证明前端拦的理由与内核一致，
/// 只是**位置更早**。
#[test]
fn 门4_type5真装载路径_格式名与地表能力_前端拦截() {
    let dir = 临时目录("type5");
    let p = dir.join("type5_4x2.map");
    let bytes = 造type5(4, 2);
    std::fs::write(&p, &bytes).expect("写 Type5 合成样本");

    // ① 真装载路径（与 GUI 打开文件同一个函数）
    let (doc, err) = DocState::load_map(&p);
    let doc = doc.unwrap_or_else(|| panic!("Type5 必须能打开，实际失败：{err:?}"));
    println!("GATE4 装载成功：{} 字节 / {}×{}", doc.bytes, doc.map.width, doc.map.height);
    println!("GATE4 format_label = {}", doc.format_label());
    assert!(
        doc.format_label().contains("Mir3Wemade"),
        "判别必须命中 Type5，实际 = {}",
        doc.format_label()
    );
    assert_eq!(doc.size(), (4, 2), "尺寸要按 Type5 头部的 22/24 偏移读");
    assert_eq!(doc.bytes, bytes.len(), "读到的字节数必须等于文件长度");

    // ② 地表能力位
    assert!(
        !doc.ground_paint_supported(),
        "Type5 的 ground_paint_supported 必须为 false"
    );

    // ③ 前端拦截：判据 + 文案（与 `app.rs` 共用同一个函数）
    let st = DocState {
        doc: Some(doc),
        assets: crate::mapdoc::Assets::Failed("验收不装载图库".to_string()),
        map_error: None,
        map_gen: 1,
    };
    let mut ed = EditSession::default();
    let d = st.doc.as_ref().expect("文档");
    assert!(
        ed.bind_document(&d.path, d.orig_bytes()),
        "真文档必须能绑进编辑会话（否则内核按占位 Old12 判定）"
    );

    让地表层落笔(&mut ed);
    let why = ground_block_reason(&st, ed.layer()).expect("地表层在 Type5 上必须被前端拦住");
    println!("GATE4 前端拦截原因：{why}");
    assert!(why.contains("Mir3Wemade"), "原因要点名格式：{why}");
    assert!(
        why.contains("bk_img"),
        "原因要点明无效的字段（bk_img）：{why}"
    );

    // 「不发命令」的证据：撤销栈 0 步、不脏、字节不变
    let 拦截前 = st.doc.as_ref().expect("文档").orig_bytes().to_vec();
    assert_eq!(ed.undo_len(), 0, "拦截时不许产生撤销步（= 一条命令都没发）");
    assert!(!ed.dirty(), "拦截不许把文档弄脏");
    let mut st2 = 只有地图(&p).expect("再开一次");
    assert_eq!(
        st2.doc.as_mut().expect("文档").to_bytes().expect("出字节"),
        拦截前,
        "拦截后地图必须逐字节不变"
    );

    // ④ 反向对照：切到中层 ⇒ 放行（Type5 的中层照常可写）
    让中层落笔(&mut ed);
    assert!(
        ground_block_reason(&st, ed.layer()).is_none(),
        "Type5 的中层不该被拦（拦的是地表逐格写 bk_img）"
    );

    // ⑤ 内核侧同口径：走一次完整的"开始一笔 → 落一格 → 结束"，内核也拒绝。
    //
    //    ⚠ 必须 `begin()`：`EditSession::touch` 的第一句是 `if !undo.stroke_active() { return 0 }`，
    //    没有笔画在手时它连地表闸门都不会问（返回 0 但 `note` 是 `None`）。
    //    这也正是**前端拦截的价值**：它连 `begin()` 都不做，用户不必先"开始一笔"才被告知。
    let mut st3 = 只有地图(&p).expect("再开一次");
    let mut ed3 = EditSession::default();
    {
        let d3 = st3.doc.as_ref().expect("文档");
        assert!(ed3.bind_document(&d3.path, d3.orig_bytes()));
    }
    让地表层落笔(&mut ed3);
    ed3.begin();
    let n = ed3.touch(&mut st3.doc.as_mut().expect("文档").map, 0, 0);
    println!(
        "GATE4 内核直调 touch 改格数 = {n}，内核原因 = {:?}",
        ed3.note()
    );
    assert_eq!(n, 0, "内核也必须拒绝 Type5 的地表落笔");
    assert!(
        ed3.note().is_some_and(|s| s.contains("Mir3Wemade")),
        "内核原因要点名格式，实际 = {:?}",
        ed3.note()
    );
    ed3.end();

    let _ = std::fs::remove_dir_all(&dir);
}

/// 把当前编辑层切到**地表**（`EditLayer::Ground` 是缺省值，这里显式设一次防将来改缺省）。
fn 让地表层落笔(ed: &mut EditSession) {
    ed.set_layer(crate::edit::EditLayer::Ground);
}

/// 把当前编辑层切到**中层**（Type5 上中层是 `mid_img`，逐格可写）。
fn 让中层落笔(ed: &mut EditSession) {
    ed.set_layer(crate::edit::EditLayer::Middle);
}

// ───────────────────────── 九格式合成样本 ─────────────────────────

/// **九种格式全部能开、且原样存回**（本单标题那句"真正能打开 9 种地图格式"的直接证据）。
///
/// # 为什么必须补这一条
///
/// 全仓 1,158 张真实图**只覆盖 3 种格式**（`Old12` 856 / `Shanda14` 107 / `Shanda2012` 195）——
/// 门 2 / 门 3 再绿也只能证明这 3 种。Type1 / Type4 / Mir3 / CrystalCs 在本机语料里
/// **一张都没有**，只能靠合成样本验。本用例把九种各造一张（构造手法逐字对齐
/// `mir2-format/src/formats/*.rs` 各自的 `造图` 测试），逐张走
/// [`DocState::load_map`]（= GUI 装载路径）打开并出字节比对。
///
/// # 每条断言
///
/// 1. 打开成功；
/// 2. `format_label()` 含**该格式自己的名字**（判别真的命中它，没被判别链前面的人抢走）；
/// 3. 尺寸对得上（宽高按该格式自己的头部偏移读）；
/// 4. `to_bytes()` 与输入**逐字节相同**（= 未改即存 0 差异，九种都成立）。
#[test]
fn 九格式合成样本_全部能开且原样存回() {
    // (格式名（要出现在 format_label 里）, 期望宽, 期望高, 字节)
    let cases: Vec<(&str, u16, u16, Vec<u8>)> = vec![
        ("Old12", 4, 3, 造_old12(4, 3)),
        ("Shanda14", 4, 3, 造_shanda14(4, 3)),
        ("Shanda2012", 4, 3, 造_shanda2012(4, 3)),
        ("Mir3Wemade", 4, 2, 造type5(4, 2)),
        ("Mir3Shanda", 4, 3, 造_mir3_shanda(4, 3)),
        ("AntiHack", 5, 4, 造_antihack(5, 4, 0x0303)),
        ("Wemade2010", 4, 3, 造_wemade2010(4, 3, 0x0F0F)),
        ("Heroes34", 3, 5, 造_heroes34(3, 5)),
        ("CrystalCs", 3, 2, 造_crystal_cs(5, 3, 2, true)),
    ];

    let dir = 临时目录("nine");
    let mut seen: Vec<&str> = Vec::new();
    for (want, w, h, bytes) in &cases {
        let p = dir.join(format!("{want}.map"));
        std::fs::write(&p, bytes).expect("写合成样本");
        let (doc, err) = DocState::load_map(&p);
        let doc = doc.unwrap_or_else(|| panic!("{want} 必须能打开，实际失败：{err:?}"));
        println!(
            "NINE {want:<12} → {:<52} {}×{} {} 字节",
            doc.format_label(),
            doc.map.width,
            doc.map.height,
            doc.bytes
        );
        assert!(
            doc.format_label().contains(want),
            "{want} 的判别必须命中它自己，实际 = {}",
            doc.format_label()
        );
        assert_eq!((doc.map.width, doc.map.height), (*w, *h), "{want} 的宽高");
        assert_eq!(doc.bytes, bytes.len(), "{want} 的字节数");
        let mut doc = doc;
        let 出 = doc.to_bytes().expect("出字节");
        assert_eq!(出.len(), bytes.len(), "{want} 出字节长度必须不变");
        assert_eq!(&出, bytes, "{want} 未改即存必须逐字节相同");
        seen.push(want);
    }
    assert_eq!(seen.len(), 9, "九种格式一个都不能少：{seen:?}");
    println!("NINE 九种格式全部通过：{seen:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `Old12`（52B 头 + 12B/格）。构造口径：`mir2-format/src/formats/old12.rs` 的 `造图`。
fn 造_old12(w: u16, h: u16) -> Vec<u8> {
    let mut b = vec![0u8; 52 + 12 * w as usize * h as usize];
    for (i, x) in b.iter_mut().enumerate() {
        *x = (i as u8).wrapping_mul(11).wrapping_add(5);
    }
    b[0..2].copy_from_slice(&w.to_le_bytes());
    b[2..4].copy_from_slice(&h.to_le_bytes());
    b
}

/// `Shanda14`（52B 头 + 14B/格，签名 `b[4]=0x0F / b[18]=0x0D / b[19]=0x0A`）。
fn 造_shanda14(w: u16, h: u16) -> Vec<u8> {
    let mut b = vec![0u8; 52 + 14 * w as usize * h as usize];
    for (i, x) in b.iter_mut().enumerate() {
        *x = (i as u8).wrapping_mul(13).wrapping_add(7);
    }
    b[0..2].copy_from_slice(&w.to_le_bytes());
    b[2..4].copy_from_slice(&h.to_le_bytes());
    b[4] = 0x0F;
    b[18] = 0x0D;
    b[19] = 0x0A;
    b
}

/// `Shanda2012`（36B/格 = 14B 字段 + 22B 未建模尾区，与 `Shanda14` 同签名）。
fn 造_shanda2012(w: u16, h: u16) -> Vec<u8> {
    let mut b = vec![0u8; 52 + 36 * w as usize * h as usize];
    for (i, x) in b.iter_mut().enumerate() {
        *x = (i as u8).wrapping_mul(17).wrapping_add(3);
    }
    b[0..2].copy_from_slice(&w.to_le_bytes());
    b[2..4].copy_from_slice(&h.to_le_bytes());
    b[4] = 0x0F;
    b[18] = 0x0D;
    b[19] = 0x0A;
    b
}

/// `Mir3Shanda`（40B 头 + 20B/格，签名 `b[0]=0x0F / b[5]=0x53 / b[14]=0x33`；宽在 16、高在 18）。
fn 造_mir3_shanda(w: u16, h: u16) -> Vec<u8> {
    let mut b = vec![0u8; 40 + 20 * w as usize * h as usize];
    for (i, x) in b.iter_mut().enumerate() {
        *x = (i as u8).wrapping_mul(19).wrapping_add(3);
    }
    b[0] = 0x0F;
    b[5] = 0x53;
    b[14] = 0x33;
    b[16..18].copy_from_slice(&(w as i16).to_le_bytes());
    b[18..20].copy_from_slice(&(h as i16).to_le_bytes());
    b
}

/// `AntiHack`（64B 头 + 12B/格；宽高在头里是**异或后**的值：`w^xor` 在 31、`xor` 在 33、`h^xor` 在 35）。
fn 造_antihack(w: i16, h: i16, xor: i16) -> Vec<u8> {
    let mut b = vec![0u8; 64 + 12 * (w as usize) * (h as usize)];
    for (i, x) in b.iter_mut().enumerate() {
        *x = (i as u8).wrapping_mul(23).wrapping_add(9);
    }
    b[0] = 0x15;
    b[4] = 0x32;
    b[6] = 0x41;
    b[19] = 0x31;
    b[31..33].copy_from_slice(&(w ^ xor).to_le_bytes());
    b[33..35].copy_from_slice(&xor.to_le_bytes());
    b[35..37].copy_from_slice(&(h ^ xor).to_le_bytes());
    b
}

/// `Wemade2010`（54B 头 + 15B/格；`w^xor` 在 21、`xor` 在 23、`h^xor` 在 25）。
fn 造_wemade2010(w: i16, h: i16, xor: i16) -> Vec<u8> {
    let mut b = vec![0u8; 54 + 15 * (w as usize) * (h as usize)];
    for (i, x) in b.iter_mut().enumerate() {
        *x = (i as u8).wrapping_mul(41).wrapping_add(13);
    }
    b[0] = 0x10;
    b[2] = 0x61;
    b[7] = 0x31;
    b[14] = 0x31;
    b[21..23].copy_from_slice(&(w ^ xor).to_le_bytes());
    b[23..25].copy_from_slice(&xor.to_le_bytes());
    b[25..27].copy_from_slice(&(h ^ xor).to_le_bytes());
    b
}

/// `Heroes34`（54B 头 + 15B/格；宽在 21、高在 25，**不异或**）。
fn 造_heroes34(w: u16, h: u16) -> Vec<u8> {
    let mut b = vec![0u8; 54 + 15 * w as usize * h as usize];
    for (i, x) in b.iter_mut().enumerate() {
        *x = (i as u8).wrapping_mul(31).wrapping_add(1);
    }
    b[0] = 0x0D;
    b[1] = 0x4C;
    b[7] = 0x20;
    b[11] = 0x6D;
    b[21..23].copy_from_slice(&(w as i16).to_le_bytes());
    b[25..27].copy_from_slice(&(h as i16).to_le_bytes());
    b
}

/// `CrystalCs`（**变长格**：8B 头 + 逐格可变，魔数 `b[2..4] = 0x43 0x23`）。
///
/// 构造口径逐字对齐 `mir2-format/src/formats/crystal_cs.rs` 的 `造图`（`ver=5` 且带前景格）。
fn 造_crystal_cs(ver: i16, w: u16, h: u16, with_front: bool) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&ver.to_le_bytes());
    out.extend_from_slice(&[0x43, 0x23]); // MAGIC
    out.extend_from_slice(&(w as i16).to_le_bytes());
    out.extend_from_slice(&(h as i16).to_le_bytes());
    for i in 0..(w as usize * h as usize) {
        out.extend_from_slice(&(i as i16).to_le_bytes()); // 背景库号
        out.extend_from_slice(&(0x1000u32 + i as u32).to_le_bytes()); // 背景图号
        out.extend_from_slice(&(2i16).to_le_bytes()); // 中间层库号
        out.extend_from_slice(&(0x2000u16 + i as u16).to_le_bytes()); // 中间图号
        if ver > 2 {
            out.push(u8::from(with_front));
            if with_front {
                out.extend_from_slice(&(7i16).to_le_bytes());
                out.extend_from_slice(&(0x3000u16).to_le_bytes());
                if ver > 3 {
                    out.push(0x11);
                    out.push(0x22);
                    if ver > 4 {
                        out.push(0x33);
                    }
                }
            }
        } else {
            out.extend_from_slice(&(if with_front { 7i16 } else { 0i16 }).to_le_bytes());
            out.extend_from_slice(&(if with_front { 0x3000u16 } else { 0u16 }).to_le_bytes());
        }
        out.push(0x81); // 门索引
        out.push(0x02); // 门偏移
        if ver < 4 {
            if with_front || ver <= 2 {
                out.push(0x44);
                out.push(0x55);
            } else {
                out.push(0x66);
                out.push(0x77);
            }
        }
        out.push(0x08); // 中间层动画帧
        out.push(0x09); // 中间层动画节拍
        out.extend_from_slice(&(0x1234i16).to_le_bytes()); // TileAnimationImage
        out.extend_from_slice(&(0x5678i16).to_le_bytes()); // TileAnimationOffset
        out.push(0x0A); // TileAnimationFrames
        out.push(0x0B); // 光照
        if ver >= 2 {
            for k in 0..12u8 {
                out.push(0xC0 + k);
            }
        }
    }
    out
}

// ───────────────────────── 门 5：67 张带尾块图 ─────────────────────────

/// **门 5**：`EM001.map` ⇒ 打开成功、`tolerant_trailer == 8040`、`edit_warning` 非空、
/// 黄条（[`DocText::banner`]）逐字等于 core 原文。
///
/// 为什么验"黄条字段"而不只是验数字：黄条是**用户唯一能看见**的那条告警。
/// 字段非空但界面没画，等于没告警 —— 所以这里断言
/// [`crate::screen_text::build`] 真的把它装进了 `DocText::banner`，
/// 而 `banner::dyn_labels` 又真的会为它产生一条文字（`banner` 模块自测已覆盖）。
#[test]
fn 门5_带尾块样本_8040字节_黄条出现() {
    let p = match 尾块样本() {
        Some(p) => p,
        None => {
            eprintln!(
                "[跳过] 无资产根 MIREDIT_ASSET_ROOT（或缺 client/热血传奇23周年客户端/Map/EM001.map）"
            );
            return;
        }
    };
    let st = 只有地图(&p).expect("EM001.map 必须能打开（宽松判别）");
    let d = st.doc.as_ref().expect("文档");

    println!(
        "GATE5 {} / {}×{} / {} 字节 / 格式 {}",
        d.file_name,
        d.map.width,
        d.map.height,
        d.bytes,
        d.format_label()
    );
    assert_eq!(
        d.tolerant_trailer(),
        尾块字节数,
        "未解尾块字节数必须等于 文件长度 − (52 + 12·w·h)"
    );
    let warn = d.edit_warning().expect("带尾块的图必须有常驻告警");
    println!("GATE5 edit_warning = {warn}");
    assert!(
        warn.contains(&尾块字节数.to_string()),
        "告警原文必须带上真实字节数 {尾块字节数}：{warn}"
    );

    // 黄条真的被装配进本帧文案（不是只有 API 拿得到）
    let text = screen_text::build(
        &st,
        &crate::canvas_view::CanvasView::default(),
        None,
        &crate::cellinfo::CellLines::unknown(),
        &EditSession::default(),
    );
    assert_eq!(
        text.banner.as_deref(),
        Some(warn.as_str()),
        "黄条文案必须是 core 原文（一个字符都不许加工）"
    );
    // 格式段也接上了（门 4 的"格式名上屏"在状态栏这一侧的对应证据）
    assert!(
        text.status_format.contains("格式："),
        "状态栏必须有格式段：{:?}",
        text.status_format
    );
    println!("GATE5 状态栏格式段 = {}", text.status_format);
}
