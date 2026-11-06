//! 中文字体加载：运行时从系统字体目录读一份**单面 TTF**，注册成 egui 的首选字体。
//!
//! ## 为什么不用第三方字体库
//! `egui`/`epaint` 内部本来就用 [`ab_glyph`] 做字形解析与栅格化（`epaint-0.33.3/Cargo.toml`
//! → `ab_glyph = "0.2.32"`），本 crate 只是把**同一个** `ab_glyph` 直接声明成依赖来用它，
//! 构建图里不会多出任何新 crate（`cargo tree` 可核）。这样"校验能解析"和"egui 能渲染"
//! 用的是同一套解析器，不存在"我这里能读、egui 却读不了"的假绿灯。
//!
//! ## 拿不到字体怎么办
//! **绝不静默退回豆腐块**：打印 `找不到中文字体：<尝试过的路径与原因>` 后 `exit(2)`。
//! 宁可起不来，也不给用户一屏方块。

use std::path::PathBuf;
use std::sync::Arc;

use ab_glyph::FontArc;

/// 候选字体路径（按优先级）。
///
/// 1. `Deng.ttf` = 等线（Windows 10/11 自带，字形清爽，中英混排好看）
/// 2. `simhei.ttf` = 黑体（老系统/精简系统上也常在）
pub const FONT_CANDIDATES: [&str; 2] = [
    r"C:\Windows\Fonts\Deng.ttf",
    r"C:\Windows\Fonts\simhei.ttf",
];

/// 注册进 `FontDefinitions` 时用的字体名（内部键，不上屏）。
pub const FONT_KEY: &str = "miredit-zh";

/// 加载成功后的字体：路径 + 原始字节 + 已解析的 `ab_glyph` 字体（字形自测用）。
pub struct LoadedFont {
    /// 实际命中的字体文件路径。
    pub path: PathBuf,
    /// 字体原始字节（要交给 egui 再解析一次，所以留一份）。
    pub bytes: Vec<u8>,
    /// 已解析字体，供字形覆盖测试直接查 `glyph_id`。
    pub font: FontArc,
}

/// 按 [`FONT_CANDIDATES`] 顺序尝试加载；全部失败时返回"尝试过哪些路径、各自为什么失败"。
///
/// 运行时与单元测试**走同一条代码路径**，保证"测过的字体就是上屏的字体"。
pub fn load() -> Result<LoadedFont, String> {
    load_from(&FONT_CANDIDATES)
}

/// [`load`] 的实际实现（候选表作为参数，便于单测覆盖"全部失败"的报错文案）。
pub fn load_from(candidates: &[&str]) -> Result<LoadedFont, String> {
    let mut tried: Vec<String> = Vec::with_capacity(candidates.len());
    for path in candidates {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                tried.push(format!("{path}（读取失败：{e}）"));
                continue;
            }
        };
        // 只接受单面 TTF/OTF：TTC（字体集合）头部是 "ttcf"，egui 侧只能取 face 0，
        // 与"单面 TTF"的要求不符，直接跳过并记原因。
        if bytes.starts_with(b"ttcf") {
            tried.push(format!("{path}（是字体集合 TTC，要求单面 TTF）"));
            continue;
        }
        match FontArc::try_from_vec(bytes.clone()) {
            Ok(font) => {
                return Ok(LoadedFont {
                    path: PathBuf::from(path),
                    bytes,
                    font,
                })
            }
            Err(e) => tried.push(format!("{path}（解析失败：{e}）")),
        }
    }
    Err(tried.join("；"))
}

/// 把中文字体装进 egui：`Proportional` 与 `Monospace` 两个家族的**首位**都是它，
/// egui 内置字体退居其后作为回退（中文字体缺某个符号时仍能画出拉丁/符号字形）。
///
/// 失败即 `exit(2)`，见模块文档。
pub fn install(ctx: &egui::Context) {
    let loaded = match load() {
        Ok(f) => f,
        Err(tried) => {
            eprintln!("找不到中文字体：{tried}");
            std::process::exit(2);
        }
    };
    println!("中文字体：{}", loaded.path.display());
    ctx.set_fonts(definitions(loaded.bytes));
}

/// 由字体字节构造 `FontDefinitions`。
///
/// 运行时（[`install`]）与单元测试（[`measure_ctx`]）都走这里，
/// 保证"测过的字体布局"就是"上屏的字体布局"。
pub fn definitions(bytes: Vec<u8>) -> egui::FontDefinitions {
    let mut defs = egui::FontDefinitions::default();
    defs.font_data
        .insert(FONT_KEY.to_owned(), Arc::new(egui::FontData::from_owned(bytes)));
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        defs.families
            .entry(family)
            .or_default()
            .insert(0, FONT_KEY.to_owned());
    }
    defs
}

/// 造一个装了中文字体的 `Context`，供排版测量（无窗口、无 GPU，纯 CPU 排版）。
///
/// 注意：这里**不 exit**，加载失败直接 panic 并把"尝试过哪些路径"带出来 ——
/// 测试进程里 `exit(2)` 会静默杀掉整个测试二进制，看不到失败原因。
pub fn measure_ctx() -> egui::Context {
    let f = load().unwrap_or_else(|e| panic!("加载中文字体失败（测量上下文需要真实字体）：{e}"));
    let ctx = egui::Context::default();
    ctx.set_fonts(definitions(f.bytes));
    // egui 的字体要到**第一次 `Context::run`** 才真正装配（未装配时 `ctx.fonts()` 会 panic：
    // "No fonts available until first call to Context::run()"），这里空跑一帧完成装配。
    let _ = ctx.run(egui::RawInput::default(), |_| {});
    ctx
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_text;
    use ab_glyph::Font;
    use std::collections::BTreeMap;

    /// **字形齐全自测（验收门 §五-2）**
    ///
    /// 用与运行时**同一份**字体（同一条 [`load`] 路径），遍历 [`ui_text::ALL`] 里
    /// 每个字符串的每个字符，断言 `glyph_id != 0`（0 = `.notdef` = 豆腐块）。
    /// 缺字会把「字符 + 出现在哪条文案」全部列出来，一次修完。
    ///
    /// # 为什么还要遍历 `mir2_stamp::KIND_LABELS_ZH`
    ///
    /// 资源面板「分类」页签会把 19 个 kind 的中文名（地形 / 崖壁 / 墙体…）与「未分类」
    /// 直接画在分类下拉与右栏里。那 20 条**不在** `ui_text` 里 —— 它们的单一真源是
    /// `mir2-stamp/src/kind_labels.rs`（数据层对照表）。**抄一份进文案表就是两套真值**，
    /// 所以这里改成「直接遍历那张表」，字形覆盖照样不漏。
    #[test]
    fn 文案全部字符都有字形() {
        let f = load().unwrap_or_else(|e| {
            panic!("加载中文字体失败（字形自测要求本机装有 Deng.ttf 或 simhei.ttf）：{e}")
        });
        eprintln!("字形自测使用字体：{}", f.path.display());

        // 去重统计：字符 → 首次出现的文案键名（缺字时便于定位）
        let mut seen: BTreeMap<char, &'static str> = BTreeMap::new();
        let mut missing: BTreeMap<char, Vec<(&'static str, &'static str)>> = BTreeMap::new();

        // ① 界面文案总表
        let mut 全部: Vec<(&'static str, &'static str)> = ui_text::ALL.to_vec();
        // ② 印章分类中文名（显示在「分类」页签上，真源在 `mir2-stamp`）
        for (kind, zh) in mir2_stamp::KIND_LABELS_ZH {
            全部.push((kind, zh));
        }

        for (key, text) in 全部 {
            for c in text.chars() {
                // 换行不是字形，跳过（它在绘制里只用来断行）
                if c == '\n' {
                    continue;
                }
                seen.entry(c).or_insert(key);
                if f.font.glyph_id(c).0 == 0 {
                    let hits = missing.entry(c).or_default();
                    if !hits.iter().any(|(k, _)| *k == key) {
                        hits.push((key, text));
                    }
                }
            }
        }

        assert!(
            missing.is_empty(),
            "字体 {} 缺 {} 个字形：\n{}",
            f.path.display(),
            missing.len(),
            missing
                .iter()
                .map(|(c, hits)| {
                    let where_: Vec<String> = hits
                        .iter()
                        .map(|(k, t)| format!("{k} = {t:?}"))
                        .collect();
                    format!(
                        "  {:?} U+{:04X} ← {}",
                        c,
                        *c as u32,
                        where_.join("；")
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        );

        eprintln!(
            "共 {} 个不同字符，全部命中字形（字体 {}）",
            seen.len(),
            f.path.display()
        );
    }

    /// 加载结果必须是候选之一，且确实是单面 TTF（不是 TTC）。
    #[test]
    fn 命中的是候选单面字体() {
        let f = load().unwrap_or_else(|e| panic!("加载中文字体失败：{e}"));
        let p = f.path.to_string_lossy().to_string();
        assert!(FONT_CANDIDATES.contains(&p.as_str()), "命中了非候选路径：{p}");
        assert!(!f.bytes.starts_with(b"ttcf"), "命中字体是 TTC 集合，要求单面 TTF");
        // 至少能查到汉字"地"的字形，否则这个文件根本不含中文
        assert!(f.font.glyph_id('地').0 != 0, "字体 {p} 没有汉字字形");
    }

    /// 换行符不参与字形断言（防止有人"顺手"把 `\n` 也要求成字形）。
    #[test]
    fn 换行符不作字形要求() {
        let f = load().unwrap_or_else(|e| panic!("加载中文字体失败：{e}"));
        // 换行在 TTF 里通常没有字形（glyph_id 为 0 也正常），本测试只是把这个约定写下来
        let _ = f.font.glyph_id('\n');
    }

    /// 缺字体时**必须**把"尝试过哪些路径"原样报出来（运行时会带这句话 exit(2)）。
    ///
    /// `exit(2)` 本身在测试进程里没法跑（会杀掉整个测试二进制），
    /// 所以这里覆盖它的**前提**：错误文案必须列全所有尝试过的路径与原因。
    #[test]
    fn 缺字体时列出尝试过的路径() {
        let bogus = [
            r"C:\Windows\Fonts\__no_such_font_a__.ttf",
            r"C:\Windows\Fonts\__no_such_font_b__.ttf",
        ];
        let err = match load_from(&bogus) {
            Ok(_) => panic!("不存在的字体不该加载成功"),
            Err(e) => e,
        };
        for p in bogus {
            assert!(err.contains(p), "报错文案里漏了 {p}：{err}");
        }
        assert!(err.contains("读取失败"), "报错文案没说清失败原因：{err}");
    }
}
