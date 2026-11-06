//! 格式注册表 + 判别 + 「随便给我一个 `.map`」的统一入口。
//!
//! # 一、注册表顺序 = C# 判别链顺序（只用于**遍历**，不代表判别优先级）
//!
//! 顺序逐条对应 CrystalStudio `LegacyMapReader.cs:55-117` 的 `if-else` 链：
//!
//! | # | 注册项 | C# 源码 | 头部签名 |
//! |---|--------|---------|----------|
//! | 1 | [`crystal_cs::CrystalCs`] | `:60-64` | `Bytes[2]==0x43 && Bytes[3]==0x23`（`"C#"`） |
//! | 2 | [`mir3_wemade::Mir3Wemade`] | `:67-71` | `Bytes[0]==0`（无标题） |
//! | 3 | [`mir3_shanda::Mir3Shanda`] | `:73-77` | `Bytes[0]==0x0F && Bytes[5]==0x53 && Bytes[14]==0x33` |
//! | 4 | [`antihack::AntiHack`] | `:79-83` | `Bytes[0]==0x15 && Bytes[4]==0x32 && Bytes[6]==0x41 && Bytes[19]==0x31` |
//! | 5 | [`wemade2010::Wemade2010`] | `:85-89` | `Bytes[0]==0x10 && Bytes[2]==0x61 && Bytes[7]==0x31 && Bytes[14]==0x31` |
//! | 6 | [`shanda2012::Shanda2012`] | `:91-99` | `Bytes[4]==0x0F && Bytes[18]==0x0D && Bytes[19]==0x0A` |
//! | 7 | [`shanda14::Shanda14`] | `:91-104` | 同上（两者共用，C# 靠长度分流） |
//! | 8 | [`heroes34::Heroes34`] | `:108-112` | `Bytes[0]==0x0D && Bytes[1]==0x4C && Bytes[7]==0x20 && Bytes[11]==0x6D` |
//! | 9 | [`old12::Old12`] | `:114-115` | **兜底**（无签名） |
//!
//! # 二、判别策略：**长度优先 + 签名辅助**，不能只照 C# 签名链
//!
//! ## 2.1 C# 的判别只看标题字节，已被真实样本证伪（两个方向都错）
//!
//! | 现象 | 实测样本 | C# 的后果 |
//! |------|----------|-----------|
//! | 14 B/格 但 `Bytes[4]` 不是 `0x0F` | `GodotClient/maps` **15 个** + `<客户端工程>/需要的资料` 同名 15 个（`10.map` `B[4]=0x07`、`33.map`/`4.map` `0x03`、`42.map`/`58.map`/`64.map` `0x0B`、`14.map` `0x0D`、`20.map` `0xFF`） | 落到兜底 `LoadMapType0` 按 12 B/格读 ⇒ **读成花图** |
//! | 12 B/格 但标题恰好 `0x0F…0x0D0A` | `client/热血传奇23周年客户端/Map/hell107.map` 等 **4 个** | 按 14 B/格读 ⇒ **越界读穿** |
//!
//! ⇒ 标题字节**不是**可靠判据；文件长度才是硬事实。
//!
//! ## 2.2 本 crate 的判别顺序
//!
//! ```text
//! ① 长度优先：52 字节 TMapHeader 家族（12 / 14 / 36 字节格，外加一个无实证的 40 字节形态），标题不参与
//!     len == 52 + 40*w*h  →  None（无实证，仅防御；read_any 报 UnsupportedVariant）
//!     len == 52 + 36*w*h  →  Shanda2012
//!     len == 52 + 14*w*h  →  Shanda14
//!     len == 52 + 12*w*h  →  12 B/格家族：有 AntiHack 签名（且长度对得上它自己的 64 字节头）⇒ AntiHack
//!                                             否则 ⇒ Old12
//! ② 其余格式：签名链（C# 顺序）+ 各自精确长度（Type1/4/5/6/7/100 头长各不相同）
//! ③ 容忍尾随：仍按 C# 顺序，只要求「签名成立 + 文件不短于该格式所需长度」，多出的进 trailer
//! ④ 全不中 ⇒ None
//! ```
//!
//! 为什么 ① 排在 ② 前面：12/14/36 三档在**同一组 `w/h`** 下长度互斥（`w*h == 0` 已被
//! `validate_dims` 拦掉），所以「长度整除」本身就是唯一解，比任何标题字节都可靠。
//! （`52 + 40·w·h` 那一档**真实语料里 0 个**，保留只为防御未来怪文件；它与 12/14/36 三档
//! 在同一 `w/h` 下同样互斥，见 [`csharp_40b_variant`]。）
//! ② 里的六种格式头长各不相同（8 / 28 / 40 / 54 / 64），与 52 字节 `TMapHeader` 的
//! `52 + k·w·h` 不会撞车，所以 ① 先跑不会抢走它们。
//!
//! ## 2.3 关于 AntiHack 与 Old12 的「12 B/格家族」
//!
//! 两者格长都是 12，但**头部长度不同**：`Old12` 是 52（`52 + 12·w·h`），`AntiHack` 是 64
//! （`64 + 12·w·h`）。同一个 `w*h` 下两式不可能同时成立 ⇒ 长度已经把二者分开。
//! ①里仍按「先看 AntiHack 签名」的顺序写，是为了与规格口径一致；实际由长度兜住。
//!
//! # 三、`detect_format` 与 `detect_format_csharp_order` 的分工
//!
//! - [`detect_format`]：**本 crate 的实现口径**（上面 2.2）；
//! - [`detect_format_csharp_order`]：**纯 C# 口径**（只按注册表顺序问 `header_signature`），
//!   用于对照「C# 会怎么判」，`tests/synthetic.rs` 用它证明两套口径在已知样本上的分歧。

use crate::error::FormatError;
use crate::formats::{
    antihack, crystal_cs, heroes34, mir3_shanda, mir3_wemade, old12, shanda14, shanda2012,
    wemade2010,
};
use crate::model::{tmap_dims, FormatId, MapFormat, RawMap};

// 九个格式的常驻单例（零尺寸类型，无状态）。
static CRYSTAL_CS: crystal_cs::CrystalCs = crystal_cs::CrystalCs;
static MIR3_WEMADE: mir3_wemade::Mir3Wemade = mir3_wemade::Mir3Wemade;
static MIR3_SHANDA: mir3_shanda::Mir3Shanda = mir3_shanda::Mir3Shanda;
static ANTIHACK: antihack::AntiHack = antihack::AntiHack;
static WEMADE2010: wemade2010::Wemade2010 = wemade2010::Wemade2010;
static SHANDA2012: shanda2012::Shanda2012 = shanda2012::Shanda2012;
static SHANDA14: shanda14::Shanda14 = shanda14::Shanda14;
static HEROES34: heroes34::Heroes34 = heroes34::Heroes34;
static OLD12: old12::Old12 = old12::Old12;

/// 注册表本体，顺序见模块文档第一节。
static FORMATS: [&'static dyn MapFormat; 9] = [
    &CRYSTAL_CS,
    &MIR3_WEMADE,
    &MIR3_SHANDA,
    &ANTIHACK,
    &WEMADE2010,
    &SHANDA2012,
    &SHANDA14,
    &HEROES34,
    &OLD12,
];

/// 全部已注册格式，**按 C# 判别链顺序**排列。
pub fn registry() -> &'static [&'static dyn MapFormat] {
    &FORMATS
}

/// 按 [`FormatId`] 取格式实现。
pub fn format_by_id(id: FormatId) -> &'static dyn MapFormat {
    match id {
        FormatId::CrystalCs => &CRYSTAL_CS,
        FormatId::Mir3Wemade => &MIR3_WEMADE,
        FormatId::Mir3Shanda => &MIR3_SHANDA,
        FormatId::AntiHack => &ANTIHACK,
        FormatId::Wemade2010 => &WEMADE2010,
        FormatId::Shanda2012 => &SHANDA2012,
        FormatId::Shanda14 => &SHANDA14,
        FormatId::Heroes34 => &HEROES34,
        FormatId::Old12 => &OLD12,
    }
}

/// 是不是「`52 + 40·w·h`」这个长度形态（命中返回宽高）。
///
/// **只看长度** `52 + 40*w*h`（不看标题，理由见
/// [`crate::formats::shanda2012::Shanda2012::csharp_40b_variant`]）。
///
/// # ⚠ **无实证，仅防御**（2026-11-05 订正）
///
/// - **真实语料 0 个**：本机 1,158 个真实 `.map` 里，长度精确等于 `52 + 40·w·h` 的
///   文件 **0 个**（`52 + 36·w·h` 的有 195 个）⇒ 本函数在真实数据上**永不命中**。
/// - **它不是「C# 的格长」**：C# `LoadMapType3` 就是 **36 B/格**，
///   `LegacyMapReader.cs:280` 的 `ValidateDimensions(52, 36)` 与实现**完全自洽**
///   （`:300`/`:303` 的 `ToInt16` 不推进 `offset`，其 2 字节落在 `:301 +=7` 与 `:304 +=14`
///   的跳过区内部）。旧文档写「C# 每格推进 40、与 `:280` 自相矛盾」是**误读，已删除**。
/// - **保留理由**：万一未来出现长度恰好凑成 `52 + 40·w·h` 的怪文件，宁可显式报
///   [`FormatError::UnsupportedVariant`]，也不许按 36 B 硬读。
/// - **不会误伤 36 B/格文件**：命中是精确等式，`52+40·w·h == 52+36·w·h` 只在 `w·h == 0` 时成立。
pub fn csharp_40b_variant(bytes: &[u8]) -> Option<(u16, u16)> {
    shanda2012::Shanda2012::csharp_40b_variant(bytes)
}

/// 判别一个 `.map` 字节流属于哪种格式（长度优先 + 签名辅助，见模块文档 2.2）。
///
/// 「`52 + 40·w·h`」这个**无实证**的长度形态**不返回任何格式**（交给 [`read_any`] 报
/// [`FormatError::UnsupportedVariant`]，或用 [`csharp_40b_variant`] 单独判定）。
pub fn detect_format(bytes: &[u8]) -> Option<&'static dyn MapFormat> {
    // ── ① 长度优先：52 字节 TMapHeader 家族（12 / 14 / 36，外加无实证的 40 形态），标题不参与 ──
    if let Some((w, h)) = tmap_dims(bytes) {
        let cells = w as u64 * h as u64;
        let len = bytes.len() as u64;
        let head = old12::HEADER_LEN as u64;

        // ①-a 40 B/格形态 ⇒ 无实证，仅防御；不认领（不许按 36 B 硬读）。
        if len == head + cells * shanda2012::CSHARP_CELL_LEN as u64 {
            return None;
        }
        // ①-b 12 B/格家族：先看 AntiHack 签名（它头长 64，长度自会分家），否则 Old12。
        if len == head + cells * old12::CELL_LEN as u64 {
            if ANTIHACK.header_signature(bytes)
                && len == antihack::HEADER_LEN as u64 + cells * antihack::CELL_LEN as u64
            {
                return Some(&ANTIHACK);
            }
            return Some(&OLD12);
        }
        // ①-c 14 B/格
        if len == head + cells * shanda14::CELL_LEN as u64 {
            return Some(&SHANDA14);
        }
        // ①-d 36 B/格（真实 36B 口径）
        if len == head + cells * shanda2012::CELL_LEN as u64 {
            return Some(&SHANDA2012);
        }
    }

    // ── ② 其余格式：签名链（C# 顺序）+ 各自精确长度 ──
    FORMATS.into_iter().find(|f| f.detect(bytes))
}

/// 宽松判别：在 [`detect_format`] 之后再补一遍「签名成立 + 文件不短于该格式所需长度」。
///
/// ⚠ 与 [`detect_format`] 的区别**只在最后这一遍**：它允许「格数组之后还有多出来的字节」，
/// 那些字节会被 [`RawMap::trailer`] 原样保留。**这不是给正常文件用的** —— 正常文件请用
/// [`detect_format`]；本函数只服务于「长度不自洽、但确实是某个格式的头 + 格数据」的
/// 取证/迁移场景（真实样本里 `client/热血传奇23周年客户端/Map/EM001.map` 等 67 个就是这种：
/// 头部声明 30×35 = 1050 格，`52 + 12*1050 = 12652`，而文件是 20692 字节）。
///
/// # ⚠ 走这条路必然产生非空 `trailer` ⇒ 调用方**必须告警**
///
/// 本函数之所以能「救活」这些文件，靠的就是把解释不了的字节塞进 [`RawMap::trailer`]。
/// 那 N 字节**不是垃圾**：`20,692 = 52 + 12 × 1720`，`1720 = 40 × 43`，而头部写的是 30×35
/// ⇒ 「`1720 = 40×43`」与「头部 30×35」二者必有一错（本机无第三方证据可判，审查报告 §四 P1-1）。
/// 若真实尺寸是 40×43，则本工具用 `h = 35` 做的**列优先展开会让整图错位**，不只是缺尾。
///
/// ⇒ 拿到结果后**必须**检查 [`RawMap::trailer_len`]，非 0 就提示用户：
/// 「**本工具按头部 `w·h` 解释，剩余 N 字节未解释**」。
pub fn detect_format_tolerant(bytes: &[u8]) -> Option<&'static dyn MapFormat> {
    if let Some(f) = detect_format(bytes) {
        return Some(f);
    }
    for f in FORMATS {
        if !f.header_signature(bytes) {
            continue;
        }
        let Some((w, h)) = f.dims(bytes) else {
            continue;
        };
        if let Some(need) = f.expected_len(w, h) {
            if (bytes.len() as u64) < need {
                continue;
            }
        }
        return Some(f);
    }
    None
}

/// **纯 C# 判别链**（只看头部签名，不看长度），用于与 C# 行为做对照。
///
/// ⚠ 与 C# 的差异仅一处：C# 在 14/36 字节格那一支内部还会用长度分流（`:95`），
/// 本函数不做分流 —— 两者共用同一条签名，先命中者（[`shanda2012::Shanda2012`]）返回。
/// 想要「按长度定档」请用 [`detect_format`]。
pub fn detect_format_csharp_order(bytes: &[u8]) -> Option<&'static dyn MapFormat> {
    FORMATS.iter().copied().find(|f| f.header_signature(bytes))
}

/// 读任意格式的 `.map`。
///
/// 成功即保证 [`MapFormat::write`] 能逐字节写回。
pub fn read_any(bytes: &[u8]) -> Result<RawMap, FormatError> {
    if let Some((w, h)) = csharp_40b_variant(bytes) {
        return Err(FormatError::unsupported(
            FormatId::Shanda2012,
            format!(
                "文件长度 {} = 52 + 40*{w}*{h}，命中「40 B/格」长度形态。\
                 该形态**无实证**（本机 1158 个真实样本里 0 个命中；36 B/格有 195 个），\
                 且 C# LoadMapType3 本身就是 36 B/格（:280 ValidateDimensions(52, 36)），\
                 本 crate 拒绝按 36B 硬读",
                bytes.len()
            ),
        ));
    }
    match detect_format(bytes) {
        Some(f) => f.read(bytes),
        None => {
            // 诊断一遍：给出「是哪个格式的签名、期望多少字节」而不是含糊的 UnknownFormat。
            // 这不改变任何成功路径，只让失败信息可定位。
            for f in FORMATS {
                if !f.header_signature(bytes) {
                    continue;
                }
                let Some((w, h)) = f.dims(bytes) else {
                    continue;
                };
                let Some(need) = f.expected_len(w, h) else {
                    continue;
                };
                if (bytes.len() as u64) < need {
                    return Err(FormatError::Truncated {
                        format: f.id(),
                        len: bytes.len(),
                        need,
                    });
                }
                return Err(FormatError::LengthMismatch {
                    format: f.id(),
                    width: w,
                    height: h,
                    len: bytes.len(),
                    expected: need,
                });
            }
            Err(FormatError::UnknownFormat { len: bytes.len() })
        }
    }
}

/// 宽松读取：允许「格数组之后还有多出来的字节」，多出的进 [`RawMap::trailer`]。
///
/// 只在 [`detect_format`] 失败后才走宽松判别；能严格判别的文件，结果与 [`read_any`] 一致。
/// 用途见 [`detect_format_tolerant`]。
///
/// # ⚠ 本函数「吞掉」了多少字节 —— 调用方必须查 [`RawMap::trailer_len`]
///
/// 走这条路时，**解释不了的剩余字节一律进 [`RawMap::trailer`]，本函数不报错、不告警**。
/// 实测：67 张真实文件（`EM001..EM060`/`EM100`/`EM200`/`EM300`、`RYZL`、`T118/T218/T318`）
/// 全部 `20,692` 字节、头部声明 `30×35`，`52 + 12×1050 = 12,652`
/// ⇒ **`trailer.len() = 8,040`（= 670 格，占 39%）**。
///
/// **契约（必须遵守）**：拿到 `Ok(map)` 之后，
///
/// ```text
/// if map.trailer_len() != 0 {
///     // 必须告警：本工具按头部 w·h 解释，剩余 N 字节未解释
/// }
/// ```
///
/// 理由：若真实尺寸是 `40×43` 而不是头部的 `30×35`，列优先展开用错 `h` 会让**整图错位**
/// （不只是缺尾）。静默当成正常文件 = 用错数据。详见 [`detect_format_tolerant`] 的 ⚠ 段。
pub fn read_any_tolerant(bytes: &[u8]) -> Result<RawMap, FormatError> {
    if let Some((w, h)) = csharp_40b_variant(bytes) {
        return Err(FormatError::unsupported(
            FormatId::Shanda2012,
            format!(
                "文件长度 {} = 52 + 40*{w}*{h}，命中「40 B/格」长度形态（无实证，仅防御）；\
                 拒绝按 36B 硬读",
                bytes.len()
            ),
        ));
    }
    match detect_format_tolerant(bytes) {
        Some(f) => f.read(bytes),
        None => Err(FormatError::UnknownFormat { len: bytes.len() }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 注册表九项且顺序对齐csharp判别链() {
        let ids: Vec<FormatId> = registry().iter().map(|f| f.id()).collect();
        assert_eq!(
            ids,
            vec![
                FormatId::CrystalCs,
                FormatId::Mir3Wemade,
                FormatId::Mir3Shanda,
                FormatId::AntiHack,
                FormatId::Wemade2010,
                FormatId::Shanda2012,
                FormatId::Shanda14,
                FormatId::Heroes34,
                FormatId::Old12,
            ]
        );
        assert_eq!(registry().len(), 9);
    }

    #[test]
    fn 每个格式都有唯一实现且caps自洽() {
        for f in registry() {
            let caps = f.caps();
            assert_eq!(
                format_by_id(f.id()).id(),
                f.id(),
                "format_by_id 应能按 id 找回同一实现"
            );
            assert!(caps.can_write, "{} 应支持写回", f.name());
            assert!(!caps.source.is_empty(), "{} 应标注源码位置", f.name());
            assert_eq!(
                caps.fixed_cell_len.is_some(),
                f.expected_len(1, 1).is_some(),
                "{} 的 fixed_cell_len 与 expected_len 应一致",
                f.name()
            );
        }
        // B 档口径（`editable`）：**`Old12` 与 `Shanda14`** 承诺「改格后语义仍自洽」。
        //
        // 2026-11-05（P1-9）把 `Old12` 从 `false` 改 `true`。旧值**不是按判据推的**，
        // 是按「敢不敢承诺」定的，后果是 `miredit-core` 给 1,051/1,158 张真实图（90.8%）
        // 挂常驻黄条，文案「未解区语义未知」对 `Old12` 是**事实错误**：
        //   ① 12 字节全部有名字（`old12.rs:17-27` 的字段表逐条带源码行号）；
        //   ② `TAIL_LEN = 0`、`read` 里 `tail: Vec::new()`（`old12.rs:70`、`:136`）；
        //   ③ `miredit-core` 的 `unmodeled_fields(Old12)` 为空（`convert.rs`）；
        //   ④ 856 张真实样本往返 0 字节差异（`tests/real_samples.rs`）。
        // ⇒ 判据 1（无未建模区）与判据 2（有真实样本）都满足 ⇒ 必须 `true`。
        //
        // 仍为 `false` 的 7 种（逐条依据见各 `formats/*.rs` 的 `caps()`）：
        //   `Shanda2012` 22 B 尾区未建模且疑与背图索引/光照联动（有样本但判据 1 不满足）；
        //   `AntiHack` 无未建模区但**零真实样本**（判据 2 不满足，与 `CrystalCs` 同一处置）；
        //   `Wemade2010` / `Heroes34` / `Mir3Wemade` / `Mir3Shanda` / `CrystalCs` 两条都不满足。
        // `CrystalCs` 的收紧史见 `crystal_cs.rs:81-100`（要改回 `true` 需真实 Type100 样本
        // + C# 双向对读验证字段顺序）。
        let editable: Vec<FormatId> = registry()
            .iter()
            .filter(|f| f.caps().editable)
            .map(|f| f.id())
            .collect();
        // 顺序 = 注册表顺序（`Shanda14` 在 `Old12` 之前，见上方 `注册表九项` 测试）。
        assert_eq!(editable, vec![FormatId::Shanda14, FormatId::Old12]);
    }

    /// **`editable` 与 `editable_caveat` 必须一致**，且「不承诺」必须说得出具体原因。
    ///
    /// 三条不变量（P1-9 / P1-15 的落地）：
    /// 1. `editable == true` ⟺ `editable_caveat` 是空串 ——
    ///    否则会出现「标了可编辑却留着告警」或「标了不可承诺却说不清为什么」的自相矛盾旗标；
    /// 2. 零真实样本 ⇒ 必须 `editable == false`（布局未验证，改格后无从判断自洽）；
    /// 3. 有格内未建模尾区（`has_tail`）⇒ 必须 `editable == false`，且文案里要出现该格式的
    ///    尾区字节数（证明它说清了「哪一段、多少字节」，而不是一句套话）。
    #[test]
    fn editable与caveat必须一致() {
        for f in registry() {
            let caps = f.caps();
            assert_eq!(
                caps.editable,
                caps.editable_caveat.is_empty(),
                "{}：`editable` 与 `editable_caveat` 必须互为反相（true ⟺ 空串），实际 editable={} / caveat={:?}",
                f.name(),
                caps.editable,
                caps.editable_caveat
            );
            if !caps.has_real_samples {
                assert!(
                    !caps.editable,
                    "{} 零真实样本却承诺 editable —— 布局未经真实文件验证，不许承诺",
                    f.name()
                );
            }
            if caps.has_tail {
                assert!(
                    !caps.editable,
                    "{} 有格内未建模尾区却承诺 editable —— 尾区不会随编辑字段更新",
                    f.name()
                );
                // 文案里必须点名该格式的尾区规模（防"一句话套九种"）
                let 期望片段: &[&str] = match f.id() {
                    FormatId::Shanda2012 => &["22 字节未建模"],
                    FormatId::Heroes34 => &["1 字节未建模"],
                    FormatId::Mir3Wemade => &["4 字节未建模"],
                    FormatId::Mir3Shanda => &["7 字节未建模"],
                    // 只有 15 B/格一档：格内 rel 14 共 1 字节（自造的 17 B 档已删除，
                    // 见 `formats/wemade2010.rs` 模块文档 2.2）
                    FormatId::Wemade2010 => &["1 字节未建模"],
                    FormatId::CrystalCs => &["12 字节"],
                    other => panic!(
                        "{other:?} 的 `has_tail` 为真但本测试没列它 —— 新增格式必须补上期望文案"
                    ),
                };
                for 片段 in 期望片段 {
                    assert!(
                        caps.editable_caveat.contains(片段),
                        "{} 的 caveat 必须写明「{}」：{}",
                        f.name(),
                        片段,
                        caps.editable_caveat
                    );
                }
            }
            // 旧文案的两个词是 P1-9 的病灶，不许复活
            for 禁词 in ["未解区语义未知", "编辑仅保证字节保真"] {
                assert!(
                    !caps.editable_caveat.contains(禁词),
                    "{} 的 caveat 不许用旧套话「{}」",
                    f.name(),
                    禁词
                );
            }
        }
    }

    /// `ground_paint_supported`：**只有 `Mir3Wemade`（Type5）为 `false`**，其余 8 种为 `true`。
    ///
    /// 依据：Type5 的背图在**块级**（3 B / 2×2 组，C# `LegacyMapReader.cs:399-408`），
    /// `Cell::bk_img` 恒 0 且 `write` 不写它 ⇒ 逐格写 `bk_img` 会被**静默丢弃**。
    ///
    /// `CrystalCs` 定为 `true` 的理由：它的背图是**格内 `u32`**（C# `:580`
    /// `ToInt32` 读 `BackImage`），`write` 真的把 `cell.bk_img` 写回格内
    /// ⇒ 逐格落笔**有效**，与「能不能改格后语义自洽」（`editable`）是两件事。
    #[test]
    fn 地表落笔能力只有type5为false() {
        let 逐格落笔: Vec<(FormatId, bool)> = registry()
            .iter()
            .map(|f| (f.id(), f.caps().ground_paint_supported))
            .collect();
        assert_eq!(
            逐格落笔,
            vec![
                (FormatId::CrystalCs, true),
                (FormatId::Mir3Wemade, false),
                (FormatId::Mir3Shanda, true),
                (FormatId::AntiHack, true),
                (FormatId::Wemade2010, true),
                (FormatId::Shanda2012, true),
                (FormatId::Shanda14, true),
                (FormatId::Heroes34, true),
                (FormatId::Old12, true),
            ],
            "只有 Mir3Wemade（Type5）的地表落笔无效（背图在块级）"
        );
        assert_eq!(
            逐格落笔.iter().filter(|(_, v)| !v).count(),
            1,
            "应当且只应当有一种格式为 false"
        );
        // 交叉校验：false 的那一种必须真的是 Mir3Wemade
        assert!(
            !format_by_id(FormatId::Mir3Wemade)
                .caps()
                .ground_paint_supported,
            "Mir3Wemade 的地表落笔必须是 false"
        );
    }
}
