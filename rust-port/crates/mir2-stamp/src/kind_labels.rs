//! 印章分类（`palette.json` 的 `type`）→ **中文标签对照表**，只服务 UI 显示。
//!
//! # 一、这张表是干什么的（以及**不是**干什么的）
//!
//! GUI「印章」页签要按分类给用户看中文（「地形 / 崖壁 / 墙体…」），而数据源里只有英文 `type`。
//! 本模块就是那**一层**中英映射：**查表**，不推断、不分词、不做大小写归一化，查不到就如实说查不到
//! （见 §四）。它**不**参与 `.X` 解析、**不**参与 `palette.json` 读取、**不**改写任何字段 ——
//! [`crate::catalogue`] 照旧原样保留英文 `kind`，显示层要中文时来这里查。
//!
//! # 二、数据来源与实测口径（2026-11-04 21:5x，本机真实文件）
//!
//! - 来源：`<dataRoot>/catalogue/palette.json`（5,092,886 B）的 `objects[]`；
//! - 实测 **9,159** 条记录，`type` 字段 distinct 取值 **20** 个：
//!   Terrain 1,695 / Cliff 1,434 / Wall 1,152 / Rock 943 / Debris 759 / Furniture 534 /
//!   Decoration 518 / Water 469 / Plant 448 / Tree 323 / Building 271 / Fence 261 /
//!   Interior 145 / Bridge 57 / Effect 50 / Stairs 40 / Gate 26 / Boat 13 / Track 12 / `null` 9
//!   —— **合计 9,159，与 `objects[]` 条数逐条对齐**（`9,150 + 9`）；
//! - 实测 **没有**缺 `type` 字段的记录（0 条），`null` 共 **9** 条；
//! - `null` 在 Rust 侧（`catalogue::StampEntry::kind`）是 `None`，在本表里用**空串 `""`**
//!   当键、标签「未分类」—— 这是本表**唯一**一个非英文键。
//!
//! 上表**不在代码里写死**：`tests` 里的真实数据用例每次都会重算 distinct 取值、条数与漏译清单，
//! 本段只是「2026-11-04 这一天的读数」，用于人工核对。
//!
//! # 三、排序与「不编」两条纪律
//!
//! 1. **19 个英文键按 ASCII 字母序严格升序**（`Boat` → `Bridge` → `Building` → … → `Water`），
//!    便于人工逐行核对有没有漏、有没有插错位置；空串键（`null`）**排在最后** —— 它不是英文词，
//!    混进字母序里反而会让人以为漏了谁。该顺序由 `tests` 断言（升序 + 空串在末位），
//!    谁把表改乱了会当场红。
//! 2. **只译这 20 个**。`subtype`（实测 **447** 个不同取值，含 1,212 条 `null`，
//!    如 `Railway` / `Ship structure` / `Cave wall`）**一律保留英文**，本模块不给它任何映射：
//!    一没有官方中文名可依，二逐条编中文就是**编数据**。GUI 要显示 subtype 就显示英文原文；
//!    将来若真拿到权威译名，**另开一张表**（如 `subtype_labels.rs`），不要往这里塞。
//!
//! # 四、未知 kind 的回落策略（**不许编**）
//!
//! - [`kind_label_zh`] 查不到 ⇒ **`None`**，调用方自己决定怎么显示，本模块不猜；
//! - [`kind_label_zh_or`] 查不到 ⇒ **原样返回传入的 `kind`**：GUI 显示英文原文比显示「未知」更有
//!   信息量，也比显示空白强；
//! - 匹配**大小写敏感、精确相等**：`palette.json` 的 `type` 拼写是固定的，`"terrain"` 与 `"Terrain"`
//!   是两回事。本模块**不**做 `to_lowercase()` 兜底 —— 那会把「上游改名 / 改大小写」这种真问题
//!   悄悄吃掉（`catalogue::by_kind` 那种「查询键大小写不敏感」是查询便利，不是翻译兜底，两者别混）；
//! - 表里**没有**「Unknown / 未知 / 其他」这类兜底行：兜底是调用方的事，表里塞兜底行就等于
//!   给「漏译」打了个假绿灯，守卫测试也就废了。
//!
//! # 五、上游加了新 kind 会怎样（守卫）
//!
//! `tests::真实数据_全部kind都有中文标签_漏译即为红` 会读真实 `palette.json`、统计 `objects[].type`
//! 的全部 distinct 取值，**逐个**查表；只要有一个查不到就**失败**，并在失败消息里列出「漏掉的 kind
//! × 条数」与「共几条记录受影响」。
//! ⇒ 上游哪天加了 `Mountain` 之类的新分类，`cargo test -p mir2-stamp` **当场红**，
//! 而不是等到 GUI 上显示成英文才被发现。

/// 印章分类（`palette.json` 的 `type`）→ 中文标签。
///
/// 20 条 = `palette.json` 9,159 条记录的**实测全集**（19 个英文 kind + 1 条空串键，
/// 空串对应 JSON 的 `"type": null`，共 9 条记录，标签「未分类」）。
///
/// **顺序**：前 19 条按英文键 ASCII 升序，空串键排末位（理由见模块文档 §三）。
/// **匹配**：精确、大小写敏感（理由见模块文档 §四）。
/// **本表不译 `subtype`**（447 个取值，无权威译名，见模块文档 §三.2）。
///
/// 加新 kind 时**只改这一处**，`tests` 会校验长度 20 / 键唯一 / 值非空 / 字母序；
/// 若上游确实新增了分类，请同时更新模块文档 §二 的实测读数。
pub const KIND_LABELS_ZH: &[(&str, &str)] = &[
    ("Boat", "船只"),
    ("Bridge", "桥"),
    ("Building", "建筑"),
    ("Cliff", "崖壁"),
    ("Debris", "碎石"),
    ("Decoration", "装饰"),
    ("Effect", "特效"),
    ("Fence", "栅栏"),
    ("Furniture", "家具"),
    ("Gate", "城门"),
    ("Interior", "室内"),
    ("Plant", "草木"),
    ("Rock", "岩石"),
    ("Stairs", "楼梯"),
    ("Terrain", "地形"),
    ("Track", "铁轨"),
    ("Tree", "树木"),
    ("Wall", "墙体"),
    ("Water", "水面"),
    // 空串键 = JSON `"type": null`（实测 9 条）。它排在末位，不参与上面 19 条的字母序。
    ("", "未分类"),
];

/// 已知 kind 的中文标签；**未知返回 `None`**（调用方回落原始英文，不许编）。
///
/// 匹配**精确且大小写敏感**：`"Terrain"` 命中；`"terrain"`（大小写不同）、`"Terrain "`（尾随空格）、
/// `"Terrain\n"` 一律按**未知**处理 —— 不 trim、不 `to_lowercase`，免得把上游「改名 / 多空格」
/// 这种真问题悄悄吃掉。空串 `""` **是**表里的一条（对应 JSON `type: null`），返回 `Some("未分类")`。
///
/// # 示例
///
/// ```
/// use mir2_stamp::kind_label_zh;
///
/// assert_eq!(kind_label_zh("Terrain"), Some("地形"));
/// assert_eq!(kind_label_zh("terrain"), None);      // 不做大小写兜底
/// assert_eq!(kind_label_zh("Mountain"), None);     // 未知 ⇒ None，不编
/// assert_eq!(kind_label_zh(""), Some("未分类"));    // 空串 = JSON 的 null
/// ```
#[must_use]
pub fn kind_label_zh(kind: &str) -> Option<&'static str> {
    KIND_LABELS_ZH
        .iter()
        .find_map(|(key, label)| (*key == kind).then_some(*label))
}

/// 便捷版：未知时**原样返回 `kind`**（GUI 显示用）。
///
/// 已知 ⇒ 中文标签；未知 ⇒ 传入的英文原文（不返回空串、不返回「未知」）。
///
/// # 示例
///
/// ```
/// use mir2_stamp::kind_label_zh_or;
///
/// assert_eq!(kind_label_zh_or("Track"), "铁轨");
/// assert_eq!(kind_label_zh_or("NewThing"), "NewThing");  // 未知 ⇒ 原样返回
/// assert_eq!(kind_label_zh_or(""), "未分类");
/// ```
#[must_use]
pub fn kind_label_zh_or(kind: &str) -> &str {
    kind_label_zh(kind).unwrap_or(kind)
}

#[cfg(test)]
mod tests {
    //! 三条门禁：表自身形状 / **真实数据全覆盖（漏译即红）** / 未知回落。
    //!
    //! 纪律与 `catalogue_tests` 同款：真实资源不存在 ⇒ 打印原因并 skip（环境缺资源不算代码错）；
    //! 其余情况一律**失败**，不静默、不放宽。

    use std::collections::{BTreeMap, HashSet};
    use std::path::PathBuf;

    use super::{kind_label_zh, kind_label_zh_or, KIND_LABELS_ZH};

    /// 取证根缺省值：`palette.json` 的取证来源（**本模块里只出现这一处**）。
    /// 换机器用 `MIREDIT_UPSTREAM_ROOT`。
    const 缺省取证根: &str = r"I:\miredit\_upstream";

    /// 实测基线：distinct kind 数（19 个英文 + 空串/null）。
    const DISTINCT_KIND_BASELINE: usize = 20;

    /// 取证根：`MIREDIT_UPSTREAM_ROOT` → 缺省 [`缺省取证根`]；根不存在 ⇒ `None`。
    ///
    /// 只做 `var_os` + `PathBuf`，不引入任何第三方库。
    fn 取证根() -> Option<PathBuf> {
        let root = std::env::var_os("MIREDIT_UPSTREAM_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(缺省取证根));
        root.is_dir().then_some(root)
    }

    /// 真实 `palette.json` 路径（相对取证根；环境变量 `MIR2_STAMP_PALETTE` 优先）。
    fn 目录文件() -> Option<PathBuf> {
        match std::env::var_os("MIR2_STAMP_PALETTE") {
            Some(v) => Some(PathBuf::from(v)),
            None => Some(取证根()?.join("catalogue").join("palette.json")),
        }
    }

    /// 失败消息里怎么显示键：空串显示成 `<null>`，免得消息里出现一对空引号看不出是啥。
    fn 显示键(kind: &str) -> String {
        if kind.is_empty() {
            "<null>".to_string()
        } else {
            format!("{kind:?}")
        }
    }

    // -----------------------------------------------------------------------
    // 门禁 1：表自身形状（长度 / 键唯一 / 值非空 / 稳定顺序）
    // -----------------------------------------------------------------------

    #[test]
    fn 表_长度20_键无重复_值无空串_英文键按字母序() {
        assert_eq!(
            KIND_LABELS_ZH.len(),
            DISTINCT_KIND_BASELINE,
            "表必须是 {DISTINCT_KIND_BASELINE} 条 = palette.json 实测 distinct kind 全集（19 个英文 + 1 个空串/null）；\
             实际 {} 条。增删 kind 请连本断言与模块文档 §二 一起改，不许只改一边",
            KIND_LABELS_ZH.len()
        );

        // 键唯一：重复键会让 `kind_label_zh` 静默返回第一条，后面的行等于白写。
        let mut 已见: HashSet<&str> = HashSet::new();
        for (key, _) in KIND_LABELS_ZH {
            assert!(已见.insert(*key), "键 {key:?} 重复（表里一个 kind 只能有一行）");
        }
        assert_eq!(已见.len(), KIND_LABELS_ZH.len(), "去重后条数变少 ⇒ 有重复键");

        // 值不许空 / 不许纯空白（空标签等于没译，GUI 上就是一片空白）。
        for (key, label) in KIND_LABELS_ZH {
            assert!(
                !label.trim().is_empty(),
                "键 {key:?} 的中文标签是空串/纯空白 —— 空标签等于没译，不许占位"
            );
        }

        // 稳定顺序：空串键在末位，其余 19 个英文键 ASCII 严格升序（便于人工核对）。
        let (末位, 头部) = KIND_LABELS_ZH
            .split_last()
            .expect("长度已在上面断言为 20，不会为空");
        assert_eq!(
            末位.0, "",
            "空串键（对应 JSON `type: null`）必须排在末位，实际末位是 {:?}",
            末位.0
        );
        for 相邻 in 头部.windows(2) {
            let (前, 后) = (相邻[0].0, 相邻[1].0);
            assert!(
                前 < 后,
                "英文键必须按 ASCII 升序：{前:?} 应排在 {后:?} 之前（否则人工核对会漏行）"
            );
        }
    }

    // -----------------------------------------------------------------------
    // 门禁 2：真实数据全覆盖 —— 上游新增 kind 必须当场红
    // -----------------------------------------------------------------------

    #[test]
    fn 真实数据_全部kind都有中文标签_漏译即为红() {
        let Some(path) = 目录文件() else {
            eprintln!("[跳过] 真实数据覆盖：无取证根 MIREDIT_UPSTREAM_ROOT —— 本用例 skip（不 panic）");
            return;
        };
        if !path.is_file() {
            eprintln!(
                "[跳过] 真实数据覆盖：palette.json 不存在（{}）—— 本用例 skip（不 panic）",
                path.display()
            );
            return;
        }
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                eprintln!(
                    "[跳过] 真实数据覆盖：palette.json 读不出（{}：{e}）—— 本用例 skip（不 panic）",
                    path.display()
                );
                return;
            }
        };
        // 这里用 DOM（`serde_json::Value`）而不是生产路径的 `raw_value` 切片：本用例只读一次、
        // 只关心 `type` 一个字段，5 MB 解析 ~50 ms，换来「结构变了能给出明确报错」。
        let root: serde_json::Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(e) => {
                eprintln!(
                    "[跳过] 真实数据覆盖：palette.json 不是合法 JSON（{e}）—— 本用例 skip（不 panic）"
                );
                return;
            }
        };
        // 文件在、但结构变了 ⇒ **不许 skip**，否则守卫变成空转（假绿）。
        let objects = root
            .get("objects")
            .and_then(serde_json::Value::as_array)
            .expect("palette.json 存在但取不到 `objects[]` 数组：结构变了，本守卫不能空转");
        assert!(
            !objects.is_empty(),
            "palette.json 的 `objects[]` 为空 ⇒ 本守卫会空转，必须报错而不是通过"
        );

        // 统计 distinct 取值：`null`（以及极端情况下缺字段）视作空串；非字符串值单独标出来，
        // 它必然查不到标签，从而在失败消息里点名。
        let mut 条数: BTreeMap<String, usize> = BTreeMap::new();
        for obj in objects {
            let kind = match obj.get("type") {
                None | Some(serde_json::Value::Null) => String::new(),
                Some(serde_json::Value::String(s)) => s.clone(),
                Some(其它) => format!("<非字符串 type: {其它}>"),
            };
            *条数.entry(kind).or_insert(0) += 1;
        }

        // 逐 kind 查表：查不到就是漏译。
        let mut 漏译: Vec<(String, usize)> = 条数
            .iter()
            .filter(|(kind, _)| kind_label_zh(kind).is_none())
            .map(|(kind, n)| (kind.clone(), *n))
            .collect();
        漏译.sort();

        // 命中标签的也不许是空串（防止表里塞了空标签还「查得到」）。
        for kind in 条数.keys() {
            if let Some(label) = kind_label_zh(kind) {
                assert!(
                    !label.trim().is_empty(),
                    "kind {} 查到了空标签 —— 空标签等于没译",
                    显示键(kind)
                );
            }
        }

        eprintln!(
            "[真实数据] objects[] = {} 条；distinct kind = {}（基线 {DISTINCT_KIND_BASELINE}）",
            objects.len(),
            条数.len()
        );
        for (kind, n) in &条数 {
            eprintln!("[真实数据]   {:<12} {}", 显示键(kind), n);
        }

        assert!(
            漏译.is_empty(),
            "palette.json 里出现 {} 个没有中文标签的 kind（受影响 {} 条记录）：{}。\
             ⇒ 上游新增/改名了分类，请补进 KIND_LABELS_ZH（并在模块文档 §二 记下实测条数）；\
             本表不译 subtype，别顺手给 subtype 编中文。",
            漏译.len(),
            漏译.iter().map(|(_, n)| *n).sum::<usize>(),
            漏译
                .iter()
                .map(|(kind, n)| format!("{} × {n}", 显示键(kind)))
                .collect::<Vec<_>>()
                .join(" / ")
        );

        // distinct 数与基线不一致**不判失败**（少一个不算错、多一个上面已经红了），只提示复核。
        if 条数.len() != DISTINCT_KIND_BASELINE {
            eprintln!(
                "[注意] distinct kind = {} ≠ 基线 {DISTINCT_KIND_BASELINE} —— 上游增删了分类，\
                 本用例只保证「不遗漏」；请复核表是否需要同步增删，并更新模块文档 §二。",
                条数.len()
            );
        }
    }

    // -----------------------------------------------------------------------
    // 门禁 3：未知回落（已知取中文 / 未知原样返回 / 大小写敏感）
    // -----------------------------------------------------------------------

    #[test]
    fn 回落_已知取中文_未知原样返回() {
        // 已知：取中文标签。
        assert_eq!(kind_label_zh_or("Terrain"), "地形");
        assert_eq!(kind_label_zh("Terrain"), Some("地形"));

        // 未知：`kind_label_zh` 给 None，`kind_label_zh_or` **原样**返回英文（不编、不空白）。
        assert!(kind_label_zh("Unknown").is_none(), "未知 kind 必须返回 None");
        assert_eq!(kind_label_zh_or("NewThing"), "NewThing");

        // 空串 = JSON 的 `type: null`：表里有这一行，标签「未分类」。
        assert_eq!(kind_label_zh(""), Some("未分类"));
        assert_eq!(kind_label_zh_or(""), "未分类");

        // 大小写敏感：不做 `to_lowercase()` 兜底（理由见模块文档 §四）。
        assert!(kind_label_zh("terrain").is_none(), "大小写不同必须当未知");
        assert_eq!(kind_label_zh_or("terrain"), "terrain");
        // 尾随空格同理：不 trim 兜底，免得把上游「多了个空格」的真问题吃掉。
        assert!(kind_label_zh("Terrain ").is_none(), "尾随空格必须当未知");

        // 表与查表函数同源：每个键都能查回自己的标签。
        for (key, label) in KIND_LABELS_ZH {
            assert_eq!(
                kind_label_zh(key),
                Some(*label),
                "键 {key:?} 查不回自己的标签 {label:?}"
            );
            assert_eq!(kind_label_zh_or(key), *label, "键 {key:?} 的便捷查询不一致");
        }
    }
}
