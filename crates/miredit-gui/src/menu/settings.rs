//! **设置持久化** —— 记住用户的深色模式选择 + **最近打开的地图列表**。
//!
//! # 为什么写在 `%LOCALAPPDATA%\miredit\settings.json`
//!
//! 本仓**没有** `settings.json`（仓库根下全仓 `grep` 过，只有 `Cargo.toml` /
//! `layout/` 等），所以不能"复用已有的设置文件"。官方 CrystalStudio 用的是
//! `EditorPaths.ProfileDirectory` 下的 `appearance.json`（`ThemeService.Avalonia.cs:11`），
//! 那是**它的**用户配置目录；本仓是独立程序，按 Windows 惯例落到
//! `%LOCALAPPDATA%\miredit\settings.json`（首次写入时创建目录）。
//!
//! # 两个字段各来自官方哪一处
//!
//! | 字段 | 官方出处 | 本仓落点 |
//! |---|---|---|
//! | `dark` | `ThemeService.Avalonia.cs:11` 的 `appearance.json` | 本文件 |
//! | `recent` | `RecentMapStore.cs:13-14`：`%LOCALAPPDATA%\CrystalStudio\recent-maps.json` | **并入本文件** |
//!
//! 官方把最近地图**单独存一个文件**（`recent-maps.json`，`RecentMapStore.cs:13-14`）；
//! 本仓只有一份用户配置，为免多一个文件，把列表并进 `settings.json` 的 `recent` 数组。
//! **这是有意偏离**（不是抄漏），理由：本仓只有一个设置文件，两个字段同生共死，
//! 分两处写反而会出现"深色写成功、最近列表写失败"的半截状态。
//!
//! # 三条纪律
//!
//! 1. **读失败不崩**：文件不存在 / 权限不足 / JSON 坏了 —— 一律返回 `Err(原因)`，
//!    调用方**回落浅色 + 空列表**并把这句原因打到 stderr（[`tx::DIAG_SETTINGS_FALLBACK`]）。
//! 2. **不引新依赖**：文件里只有 `{"dark": bool, "recent": [..]}`，手写 JSON 读写足够，
//!    不为了它引入 `serde`（本 crate 的依赖纪律：只用 egui/eframe + workspace 内库）。
//! 3. **写失败要说**：保存失败返回 `Err`，调用方打到 stderr（不静默吞掉）。
//!
//! # `recent` 的语义（照官方 `RecentMapStore.cs`）
//!
//! * **最近优先**：`recent[0]` 是最后打开的那张（官方 `maps.Insert(0, path)`，`:36`）；
//! * **去重**：同一路径只留一条；Windows 路径大小写不敏感，官方用
//!   `StringComparer.OrdinalIgnoreCase`（`:26` / `:35`），本仓同口径（[`same_path`]）；
//! * **上限**：官方 `Limit = 20`（`RecentMapStore.cs:8`），**本仓取 10**
//!   （[`RECENT_LIMIT`]）—— 2026-11-05 的单子明确要求 10 条，属**有意偏离**，不是抄漏；
//! * **只记成功打开的**：只有真把文档换掉之后才记（[`crate::menu::action::open_path`]），
//!   打开失败一个字节都不写（官方 `RememberMap` 也只在 `OpenMap` / 保存成功后调，
//!   `MainWindow.axaml.cs:458/462/501`）。
//!
//! ⚠ 与官方的**第二处**有意偏离：官方 `Load()` 会丢掉扩展名不是 `.map` 的条目
//! （`RecentMapStore.cs:23`）；本仓**不按扩展名过滤** —— 本仓的 `Document::load` 是
//! **九格式内容判别**（不看扩展名），按扩展名过滤会把"内容真的是地图但名字不叫 .map"
//! 的文件从历史里剔掉，那是假信息。见 [`clean_recent`]。

use std::path::{Path, PathBuf};

use crate::ui_text as tx;

/// 最近地图列表的上限。
///
/// 官方 `RecentMapStore.cs:8` 是 `Limit = 20`；**本仓取 10**（2026-11-05 单子要求）。
/// 这是一处**有意偏离**：改这个数只影响"留几条历史"，不影响任何地图数据。
pub const RECENT_LIMIT: usize = 10;

/// 设置文件的字段集（深色偏好 + 最近打开的地图）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    /// 深色模式（`false` = 浅色，默认）。
    pub dark: bool,
    /// 最近打开的地图路径，**最近优先**（`[0]` = 最后打开的那张）。
    ///
    /// 不变量（[`push_recent`] / [`clean_recent`] 维护）：无空串、无重复（大小写不敏感）、
    /// 长度 ≤ [`RECENT_LIMIT`]。
    pub recent: Vec<String>,
}

/// 默认设置文件路径：`%LOCALAPPDATA%\miredit\settings.json`。
///
/// `LOCALAPPDATA` 取不到（极少见：某些服务账户）时回落到**当前目录**下的
/// `miredit-settings.json`，并在调用方打出的诊断行里带上真实路径 ——
/// 绝不"假装写成功"。
pub fn default_path() -> PathBuf {
    match std::env::var_os("LOCALAPPDATA") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir).join("miredit").join("settings.json"),
        _ => PathBuf::from("miredit-settings.json"),
    }
}

/// JSON 字符串转义（只处理 `"`、`\` 与 < 0x20 的控制字符 —— 其余原样输出）。
///
/// Windows 路径里的 `\` 必须转义成 `\\`，否则写出来的文件不是合法 JSON。
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// 序列化成 JSON 文本（缩进 2 空格 + 结尾换行，便于人眼核对）。
///
/// 空列表写成 `"recent": []`（一行），非空才展开成多行 —— 这样"没有历史"的文件
/// 仍然只有 3 行，`git diff` 不会因为加一个字段就整片重排。
pub fn to_json(s: &Settings) -> String {
    let mut out = String::from("{\n  \"dark\": ");
    out.push_str(if s.dark { "true" } else { "false" });
    if s.recent.is_empty() {
        out.push_str(",\n  \"recent\": []\n}\n");
        return out;
    }
    out.push_str(",\n  \"recent\": [\n");
    for (i, p) in s.recent.iter().enumerate() {
        out.push_str("    \"");
        out.push_str(&escape(p));
        out.push('"');
        if i + 1 < s.recent.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str("  ]\n}\n");
    out
}

/// 单行 JSON —— **只给诊断行用**。
///
/// `DIAG_SETTINGS_SAVED` 的 `{json}` 是一格，塞多行会把一行诊断拆成好几行
/// （验收脚本按行 grep 就抓不全了）。所以诊断打这个紧凑版，文件里仍是 [`to_json`]。
fn to_json_compact(s: &Settings) -> String {
    let items: Vec<String> = s
        .recent
        .iter()
        .map(|p| format!("\"{}\"", escape(p)))
        .collect();
    format!(
        "{{\"dark\":{},\"recent\":[{}]}}",
        if s.dark { "true" } else { "false" },
        items.join(",")
    )
}

/// 跳空白（JSON 的 ` \t\r\n`）。
fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\r' | b'\n') {
        i += 1;
    }
    i
}

/// 解析一个 JSON 字符串（`*i` 必须指向开引号；返回后 `*i` 指向闭引号之后）。
fn parse_string(b: &[u8], i: &mut usize) -> Result<String, String> {
    if b.get(*i) != Some(&b'"') {
        return Err("这里应该是字符串（缺开引号）".to_string());
    }
    *i += 1;
    let mut out: Vec<u8> = Vec::new();
    while *i < b.len() {
        match b[*i] {
            b'"' => {
                *i += 1;
                return String::from_utf8(out).map_err(|e| format!("字符串不是合法 UTF-8：{e}"));
            }
            b'\\' => {
                *i += 1;
                let Some(&c) = b.get(*i) else {
                    return Err("字符串在转义符后结束".to_string());
                };
                *i += 1;
                match c {
                    b'\\' => out.push(b'\\'),
                    b'"' => out.push(b'"'),
                    b'/' => out.push(b'/'),
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    b'b' => out.push(0x08),
                    b'f' => out.push(0x0c),
                    b'u' => {
                        let end = *i + 4;
                        let hex = b
                            .get(*i..end)
                            .and_then(|h| std::str::from_utf8(h).ok())
                            .ok_or_else(|| "\\u 后面要跟 4 位十六进制".to_string())?;
                        let cp = u32::from_str_radix(hex, 16)
                            .map_err(|_| format!("\\u{hex} 不是十六进制"))?;
                        let ch = char::from_u32(cp)
                            .ok_or_else(|| format!("\\u{hex} 不是合法码点"))?;
                        let mut buf = [0u8; 4];
                        out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                        *i = end;
                    }
                    other => {
                        return Err(format!("不认识的转义 \\{}", other as char));
                    }
                }
            }
            c => {
                out.push(c);
                *i += 1;
            }
        }
    }
    Err("字符串没有结束引号".to_string())
}

/// 解析字符串数组（`i` 必须指向 `[`；返回后 `i` 指向 `]` 之后）。
fn parse_string_array(b: &[u8], mut i: usize) -> Result<(Vec<String>, usize), String> {
    if b.get(i) != Some(&b'[') {
        return Err("`recent` 后面不是数组（缺 [）".to_string());
    }
    i += 1;
    let mut out = Vec::new();
    loop {
        i = skip_ws(b, i);
        match b.get(i) {
            None => return Err("数组没有结束的 ]".to_string()),
            Some(b']') => {
                i += 1;
                return Ok((out, i));
            }
            Some(b'"') => {
                out.push(parse_string(b, &mut i)?);
                i = skip_ws(b, i);
                match b.get(i) {
                    Some(b',') => i += 1,
                    Some(b']') => {
                        i += 1;
                        return Ok((out, i));
                    }
                    Some(_) => return Err("数组项之间缺少逗号".to_string()),
                    None => return Err("数组没有结束的 ]".to_string()),
                }
            }
            Some(_) => {
                return Err(format!("数组第 {} 项不是字符串", out.len() + 1));
            }
        }
    }
}

/// 找 `key` 的**真键位**（`key` 之后除空格外紧跟 `:`），返回指向 key 开引号的字节下标。
///
/// # 为什么要"逐个试"，不能 `text.find(key)`
///
/// `recent` 的**值**里可能含有 `"dark"` 这种字样（某个目录/文件就叫 `dark`），
/// 直接取第一个出现会把"值里的字"认成"键"。逐个试 + 要求紧跟冒号即可排除。
///
/// 返回 `None` = 没有这样的键（**不区分**"完全没出现"与"出现了但后面不是冒号"，
/// 调用方用 [`find_key_error`] 决定报哪句原因）。
fn find_key(text: &str, key: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(rel) = text[from..].find(key) {
        let at = from + rel;
        let after = &text[at + key.len()..];
        let head = after.trim_start();
        if head.starts_with(':') {
            return Some(at);
        }
        from = at + key.len();
    }
    None
}

/// [`find_key`] 失败时的原因：区分"根本没这个键"与"有键但后面不是冒号"。
fn find_key_error(text: &str, key: &str) -> String {
    if text.contains(key) {
        format!("{key} 字段后缺少冒号")
    } else {
        format!("缺少 {key} 字段")
    }
}

/// 从 JSON 文本解析。
///
/// * `"dark"` **必需**（缺了就是坏文件，报原因 —— 不许静默返回默认值：
///   "文件坏了但装作读到了浅色"会让用户以为自己的选择丢了）；
/// * `"recent"` **可选**：老文件只有 `{"dark": false}`（2026-11-05 之前写的），
///   读出来 `recent` 是空列表 —— **向后兼容**，不许因为缺字段就判坏文件；
/// * 其它字段一律忽略（未来加字段不会打坏旧版本）。
pub fn from_json(text: &str) -> Result<Settings, String> {
    let key = "\"dark\"";
    let at = find_key(text, key).ok_or_else(|| find_key_error(text, key))?;
    let rest = &text[at + key.len()..];
    let colon = rest.find(':').ok_or_else(|| "字段后缺少冒号".to_string())?;
    let tail = rest[colon + 1..].trim_start();
    let dark = if tail.starts_with("true") {
        true
    } else if tail.starts_with("false") {
        false
    } else {
        return Err(format!(
            "字段值不是 true/false：{}",
            tail.chars().take(12).collect::<String>()
        ));
    };

    let recent = match find_key(text, "\"recent\"") {
        None => {
            // 出现了却没紧跟冒号 ⇒ 这是坏文件，要说出来（不能当"没有这个字段"）
            if text.contains("\"recent\"") {
                return Err(find_key_error(text, "\"recent\""));
            }
            Vec::new()
        }
        Some(rk) => {
            let after = &text[rk + "\"recent\"".len()..];
            let c = after
                .find(':')
                .ok_or_else(|| "`recent` 字段后缺少冒号".to_string())?;
            // 数组起点 = 冒号之后第一个 `[`（中间允许空白）
            let arr_at = rk + "\"recent\"".len() + c + 1;
            let b = text.as_bytes();
            let start = skip_ws(b, arr_at);
            let (items, _) = parse_string_array(b, start)?;
            items
        }
    };
    Ok(Settings {
        dark,
        recent: clean_recent(recent),
    })
}

/// 把一条路径统一成"绝对化 + `\` 分隔 + 去尾分隔符"的写法（**纯字面，不碰磁盘**）。
///
/// 为什么不 `canonicalize`：最近列表里的路径**可能已经被删了**
/// （那正是"文件已不存在 ⇒ 从列表移除"那条路径要处理的），
/// `canonicalize` 对不存在的路径直接失败，用它做比较就永远匹配不上。
/// `std::path::absolute` 只做字面绝对化（Windows 下走 `GetFullPathName`，
/// 与官方 `Path.GetFullPath` 同口径），不查文件是否存在。
pub fn normalize_path(p: &str) -> String {
    let abs = std::path::absolute(Path::new(p)).unwrap_or_else(|_| PathBuf::from(p));
    let mut s = abs.to_string_lossy().replace('/', "\\");
    while s.len() > 3 && s.ends_with('\\') {
        s.pop();
    }
    s
}

/// 两个路径是不是**同一个文件**（Windows 口径：大小写不敏感）。
///
/// 官方用 `string.Equals(p, path, StringComparison.OrdinalIgnoreCase)`
/// （`RecentMapStore.cs:35`），本仓先字面绝对化再比，语义更严一点（相对/绝对写法也能对上）。
pub fn same_path(a: &str, b: &str) -> bool {
    normalize_path(a).eq_ignore_ascii_case(&normalize_path(b))
}

/// 清洗列表：去空串、去重（大小写不敏感）、截到 [`RECENT_LIMIT`]，**保持原顺序**。
///
/// ⚠ **不按扩展名过滤**（与官方 `RecentMapStore.cs:23` 有意不同）：
/// 本仓的 `Document::load` 是九格式**内容**判别，扩展名不是判据；
/// 按扩展名剔条目会把"内容真是地图、名字不叫 .map"的历史项悄悄删掉 —— 那是假信息。
pub fn clean_recent(items: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(items.len().min(RECENT_LIMIT));
    for raw in items {
        let p = raw.trim();
        if p.is_empty() {
            continue;
        }
        if out.iter().any(|e| same_path(e, p)) {
            continue;
        }
        out.push(p.to_owned());
        if out.len() == RECENT_LIMIT {
            break;
        }
    }
    out
}

/// 把一条路径**提升/追加**进列表（最近优先）：返回列表是否真的变了。
///
/// 照官方 `RecentMapStore.Remember`（`:31-39`）：先删同一条，再插到最前面，再截上限。
/// 存的是 [`normalize_path`] 之后的写法（与官方 `Path.GetFullPath(path)` 同口径）。
pub fn push_recent(list: &mut Vec<String>, path: &str) -> bool {
    let p = path.trim();
    if p.is_empty() {
        return false;
    }
    let norm = normalize_path(p);
    let before = list.clone();
    list.retain(|e| !same_path(e, &norm));
    list.insert(0, norm);
    list.truncate(RECENT_LIMIT);
    *list != before
}

/// 从列表里**移除**一条（路径比较见 [`same_path`]）：返回是否真的删掉了。
///
/// 用在哪：「点了一条最近地图，但文件已经不在了」⇒ 状态栏给原因 + 把它从列表里去掉
/// （[`crate::menu::action::open_recent_index`]）。
pub fn remove_recent(list: &mut Vec<String>, path: &str) -> bool {
    let before = list.len();
    list.retain(|e| !same_path(e, path));
    list.len() != before
}

/// 读设置；文件不存在 / 读失败 / 解析失败都返回原因（调用方回落浅色 + 空列表）。
pub fn load(path: &Path) -> Result<Settings, String> {
    let s = load_quiet(path)?;
    eprintln!(
        "{}",
        tx::fill(
            tx::DIAG_SETTINGS_LOADED,
            &[
                ("path", &path.display().to_string()),
                ("dark", if s.dark { "1" } else { "0" }),
            ]
        )
    );
    Ok(s)
}

/// 同 [`load`]，但**不打诊断行**。
///
/// 谁要它：`--menu-dump`（它自己那一屏输出是给脚本 grep 的，多一行"设置读到"会干扰），
/// 以及"只是想拿一份当前值、失败也无所谓"的调用点。
pub fn load_quiet(path: &Path) -> Result<Settings, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    from_json(&text)
}

/// 写设置（父目录不存在就创建；**原子性不保证**，这是一份偏好文件，写坏了大不了回落浅色）。
pub fn save(path: &Path, s: &Settings) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir).map_err(|e| format!("建目录 {} 失败：{e}", dir.display()))?;
        }
    }
    let json = to_json(s);
    std::fs::write(path, json.as_bytes()).map_err(|e| e.to_string())?;
    eprintln!(
        "{}",
        tx::fill(
            tx::DIAG_SETTINGS_SAVED,
            &[
                ("path", &path.display().to_string()),
                ("json", &to_json_compact(s)),
            ]
        )
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 序列化 → 反序列化往返（深色两态 × 最近列表 0/1/3 条都要过）。
    #[test]
    fn json往返() {
        let cases = vec![
            Settings {
                dark: false,
                recent: vec![],
            },
            Settings {
                dark: true,
                recent: vec![],
            },
            Settings {
                dark: true,
                recent: vec![r"maps\41.map".to_owned()],
            },
            Settings {
                dark: false,
                recent: vec![
                    r"maps\41.map".to_owned(),
                    r"maps\10.map".to_owned(),
                    r"临时 目录\带空格\3.map".to_owned(),
                ],
            },
        ];
        for s in cases {
            let text = to_json(&s);
            assert!(text.contains("\"dark\""), "缺 dark：{text}");
            assert!(text.contains("\"recent\""), "缺 recent：{text}");
            // 反斜杠必须转义（否则写出来的不是合法 JSON）
            if s.recent.iter().any(|p| p.contains('\\')) {
                assert!(
                    text.contains("\\\\"),
                    "路径里的反斜杠没转义，写出去的不是合法 JSON：{text}"
                );
            }
            assert_eq!(from_json(&text).expect("应能解析"), s, "往返不一致：{text}");
        }
    }

    /// **老文件（2026-11-05 之前只有 `dark`）必须照读**，`recent` 得空列表。
    ///
    /// 这条挡的是"升级后把用户的深色偏好判成坏文件"——那会让用户以为设置丢了。
    #[test]
    fn 老文件没有recent也能读() {
        let s = from_json("{\n  \"dark\": true\n}\n").expect("老文件必须能读");
        assert!(s.dark);
        assert!(s.recent.is_empty(), "缺 recent 字段 ⇒ 空列表");
        // 官方 `RecentMapStore` 那种"别的字段"也不许打坏解析
        let s = from_json("{\"dark\": false, \"other\": 1}").expect("多字段也要能读");
        assert!(!s.dark);
        assert!(s.recent.is_empty());
    }

    /// **键的顺序不许影响解析**（人手编辑时会把 `recent` 挪到 `dark` 前面）。
    ///
    /// 这条挡的是"按第一个出现的键名硬认"——那样 `recent` 挪到前面就会把值当键。
    #[test]
    fn 键顺序无关() {
        let s = from_json("{\"recent\": [\"E:\\\\a\\\\1.map\"], \"dark\": true}").expect("应能解析");
        assert!(s.dark, "真键在数组后面，必须认对");
        assert_eq!(s.recent, vec![r"E:\a\1.map".to_owned()]);
    }

    /// 坏文件必须**带原因**失败（调用方据此回落浅色并打诊断）。
    #[test]
    fn 坏文件带原因失败() {
        let e = from_json("{}").unwrap_err();
        assert!(e.contains("dark"), "原因里要说明缺什么：{e}");
        let e = from_json("{\"dark\": maybe}").unwrap_err();
        assert!(e.contains("true/false"), "原因要说明取值非法：{e}");
        let e = from_json("{\"dark\" true}").unwrap_err();
        assert!(e.contains("冒号"), "原因要说明缺冒号：{e}");
        // recent 坏了也要报原因（不许静默当空列表 —— 那等于偷偷删了用户的历史）
        let e = from_json("{\"dark\": false, \"recent\": 3}").unwrap_err();
        assert!(e.contains("数组"), "原因要说明 recent 不是数组：{e}");
        let e = from_json("{\"dark\": false, \"recent\": [\"a\" \"b\"]}").unwrap_err();
        assert!(e.contains("逗号"), "原因要说明缺逗号：{e}");
        let e = from_json("{\"dark\": false, \"recent\": [\"a\"").unwrap_err();
        assert!(e.contains(']'), "原因要说明数组没结束：{e}");
    }

    /// 转义/反转义：反斜杠、引号、换行、`\u` 都要能原样回来。
    #[test]
    fn 转义往返() {
        for raw in [
            r"E:\a\b.map",
            "带\"引号\".map",
            "换\n行.map",
            "制表\t符.map",
            "反斜杠\\两个.map",
        ] {
            let s = Settings {
                dark: false,
                recent: vec![raw.to_owned()],
            };
            let text = to_json(&s);
            let got = from_json(&text).expect("应能解析");
            assert_eq!(got.recent, vec![raw.to_owned()], "转义往返丢了字符：{text}");
        }
        // `\u` 码点（手写 JSON，模拟别的工具写出来的文件）
        let s = from_json("{\"dark\": false, \"recent\": [\"\\u4e2d\\u6587.map\"]}").expect("解析");
        assert_eq!(s.recent, vec!["中文.map".to_owned()]);
    }

    /// **最近优先 / 去重 / 上限 10**（官方 `RecentMapStore.Remember` 的三条语义）。
    #[test]
    fn 提升去重与上限() {
        let mut list: Vec<String> = Vec::new();
        assert!(push_recent(&mut list, r"E:\m\10.map"));
        assert!(push_recent(&mut list, r"E:\m\41.map"));
        assert_eq!(
            list,
            vec![r"E:\m\41.map".to_owned(), r"E:\m\10.map".to_owned()],
            "最近优先"
        );
        // 再打开 10.map ⇒ 提升到第一位，且**不重复**
        assert!(push_recent(&mut list, r"E:\m\10.map"));
        assert_eq!(
            list,
            vec![r"E:\m\10.map".to_owned(), r"E:\m\41.map".to_owned()],
            "提升后不许出现两份 10.map"
        );
        // 大小写/分隔符不同 ⇒ 仍是同一条（Windows 口径）
        assert!(push_recent(&mut list, r"e:\m\10.MAP"));
        assert_eq!(list.len(), 2, "大小写不同不许算两条：{list:?}");
        assert!(push_recent(&mut list, "E:/m/10.map"));
        assert_eq!(list.len(), 2, "斜杠写法不同不许算两条：{list:?}");
        // 空串不动列表
        let snapshot = list.clone();
        assert!(!push_recent(&mut list, "   "));
        assert_eq!(list, snapshot, "空路径不该改列表");
        // 上限
        for i in 0..20 {
            push_recent(&mut list, &format!(r"E:\m\{i}.map"));
        }
        assert_eq!(list.len(), RECENT_LIMIT, "上限 {RECENT_LIMIT} 条");
        assert_eq!(list[0], r"E:\m\19.map", "最后打开的排第一");
        // 移除
        assert!(remove_recent(&mut list, r"E:\m\19.map"));
        assert_eq!(list.len(), RECENT_LIMIT - 1);
        assert!(!remove_recent(&mut list, r"E:\m\19.map"), "删过的再删返回 false");
    }

    /// 磁盘往返：临时目录里真写一次、真读一次；不存在的文件读失败。
    #[test]
    fn 磁盘往返() {
        let dir = std::env::temp_dir().join("miredit_settings_test");
        let p = dir.join("settings.json");
        let _ = std::fs::remove_file(&p);
        // 不存在的文件：读失败（不是 panic、不是默认值）
        assert!(load(&p).is_err());
        let s = Settings {
            dark: true,
            recent: vec![r"E:\m\41.map".to_owned(), r"E:\m\10.map".to_owned()],
        };
        save(&p, &s).expect("写设置");
        let got = load(&p).expect("读设置");
        assert_eq!(got, s, "磁盘往返丢了字段");
        let text = std::fs::read_to_string(&p).expect("读回文件");
        assert!(text.contains("true"));
        assert!(text.contains("41.map") && text.contains("10.map"));
        // 换一份写：旧内容必须被整份替换（不许残留上一条）
        save(
            &p,
            &Settings {
                dark: false,
                recent: vec![],
            },
        )
        .expect("再写");
        let got = load(&p).expect("再读");
        assert!(!got.dark);
        assert!(got.recent.is_empty(), "空列表要真写空：{got:?}");
        let text = std::fs::read_to_string(&p).expect("读回文件");
        assert!(text.contains("\"recent\": []"), "空列表写法：{text}");
        let _ = std::fs::remove_file(&p);
        let _ = std::fs::remove_dir(&dir);
    }

    /// 默认路径必须是 `.../miredit/settings.json` 这种形状（不是仓库里的相对路径）。
    #[test]
    fn 默认路径形状() {
        let p = default_path();
        let s = p.to_string_lossy().replace('\\', "/");
        assert!(s.ends_with("miredit/settings.json"), "默认路径不对：{s}");
    }
}
