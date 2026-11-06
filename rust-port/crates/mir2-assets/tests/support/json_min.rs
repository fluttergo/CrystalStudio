//! 极简 JSON 递归下降解析器（仅测试用，纯 std）。
//!
//! 为什么手写：验收比对要读客户端烘焙的 `*_atlas_meta.json`（最大 6.3 MB），
//! 而本 crate 的依赖白名单只允许 `image`，不能引 `serde_json`。
//! 这里实现的是**完整**解析（字符串转义 / `\uXXXX` 与代理对 / 数字 / 嵌套数组与对象 /
//! 字面量），不是「按行扫字段」那种脆招 —— 文件格式一变就当报错，而不是读出半个结果。

/// 解析后的 JSON 值。
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    /// `null`
    Null,
    /// `true` / `false`
    Bool(bool),
    /// 数字（统一按 f64 存；整数取用时校验小数部分为 0）
    Num(f64),
    /// 字符串
    Str(String),
    /// 数组
    Arr(Vec<Json>),
    /// 对象（保留原始键序）
    Obj(Vec<(String, Json)>),
}

impl Json {
    /// 按键取对象成员。
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(items) => items.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// 当数组用。
    pub fn as_arr(&self) -> Option<&[Json]> {
        match self {
            Json::Arr(v) => Some(v),
            _ => None,
        }
    }

    /// 当字符串用。
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    /// 当非负整数用（小数部分必须为 0）。
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Json::Num(n) if *n >= 0.0 && n.fract() == 0.0 && *n <= u64::MAX as f64 => {
                Some(*n as u64)
            }
            _ => None,
        }
    }

    /// 当 `u32` 用。
    pub fn as_u32(&self) -> Option<u32> {
        self.as_u64().and_then(|v| u32::try_from(v).ok())
    }

    /// 当 `usize` 用。
    pub fn as_usize(&self) -> Option<usize> {
        self.as_u64().and_then(|v| usize::try_from(v).ok())
    }

    /// 当 `i16` 用（**允许负数**，小数部分必须为 0）。
    ///
    /// 为读图集 meta 的 `anchor: [px, py]` 而加：那两个值是有符号的（实测 `0` / `7` / `-44` /
    /// `-296` 都出现过），用 `as_u64` 会把负值读成 `None`，从而把「锚点是负数」误判成「字段缺失」。
    pub fn as_i16(&self) -> Option<i16> {
        match self {
            Json::Num(n)
                if n.fract() == 0.0
                    && *n >= f64::from(i16::MIN)
                    && *n <= f64::from(i16::MAX) =>
            {
                Some(*n as i16)
            }
            _ => None,
        }
    }
}

/// 解析整段文本；尾部有多余内容也算错。
pub fn parse(text: &str) -> Result<Json, String> {
    let mut p = Parser {
        src: text.as_bytes(),
        pos: 0,
    };
    p.skip_ws();
    let v = p.value()?;
    p.skip_ws();
    if p.pos != p.src.len() {
        return Err(format!("JSON 尾部有多余内容（偏移 {}）", p.pos));
    }
    Ok(v)
}

struct Parser<'a> {
    src: &'a [u8],
    pos: usize,
}

impl Parser<'_> {
    fn skip_ws(&mut self) {
        while let Some(b) = self.src.get(self.pos) {
            if matches!(b, b' ' | b'\t' | b'\n' | b'\r') {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn value(&mut self) -> Result<Json, String> {
        match self.peek() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b't') => self.literal("true", Json::Bool(true)),
            Some(b'f') => self.literal("false", Json::Bool(false)),
            Some(b'n') => self.literal("null", Json::Null),
            Some(_) => self.number(),
            None => Err("JSON 意外结束".to_string()),
        }
    }

    fn literal(&mut self, lit: &str, value: Json) -> Result<Json, String> {
        let end = self.pos + lit.len();
        if self.src.get(self.pos..end) == Some(lit.as_bytes()) {
            self.pos = end;
            Ok(value)
        } else {
            Err(format!("偏移 {} 处的字面量不是 `{lit}`", self.pos))
        }
    }

    fn object(&mut self) -> Result<Json, String> {
        self.pos += 1; // 跳过 `{`
        let mut items: Vec<(String, Json)> = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Json::Obj(items));
        }
        loop {
            self.skip_ws();
            let key = self.string()?;
            self.skip_ws();
            self.expect(b':')?;
            self.skip_ws();
            let v = self.value()?;
            items.push((key, v));
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Json::Obj(items));
                }
                other => {
                    return Err(format!(
                        "对象内偏移 {} 处期望 `,` 或 `}}`，实际 {:?}",
                        self.pos,
                        other.map(|b| b as char)
                    ))
                }
            }
        }
    }

    fn array(&mut self) -> Result<Json, String> {
        self.pos += 1; // 跳过 `[`
        let mut items: Vec<Json> = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Json::Arr(items));
        }
        loop {
            self.skip_ws();
            items.push(self.value()?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Json::Arr(items));
                }
                other => {
                    return Err(format!(
                        "数组内偏移 {} 处期望 `,` 或 `]`，实际 {:?}",
                        self.pos,
                        other.map(|b| b as char)
                    ))
                }
            }
        }
    }

    fn expect(&mut self, b: u8) -> Result<(), String> {
        if self.peek() == Some(b) {
            self.pos += 1;
            Ok(())
        } else {
            Err(format!(
                "偏移 {} 处期望 `{}`，实际 {:?}",
                self.pos,
                b as char,
                self.peek().map(|c| c as char)
            ))
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let Some(b) = self.peek() else {
                return Err("字符串未闭合".to_string());
            };
            match b {
                b'"' => {
                    self.pos += 1;
                    return String::from_utf8(out)
                        .map_err(|e| format!("字符串不是合法 UTF-8：{e}"));
                }
                b'\\' => {
                    self.pos += 1;
                    self.escape(&mut out)?;
                }
                0x00..=0x1f => {
                    return Err(format!("字符串里出现未转义控制字符 0x{b:02X}"));
                }
                _ => {
                    out.push(b);
                    self.pos += 1;
                }
            }
        }
    }

    fn escape(&mut self, out: &mut Vec<u8>) -> Result<(), String> {
        let Some(e) = self.peek() else {
            return Err("转义序列未完成".to_string());
        };
        self.pos += 1;
        match e {
            b'"' => out.push(b'"'),
            b'\\' => out.push(b'\\'),
            b'/' => out.push(b'/'),
            b'b' => out.push(0x08),
            b'f' => out.push(0x0c),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'u' => {
                let mut cp = self.hex4()?;
                // 代理对：高代理后必须紧跟 `\uXXXX` 低代理
                if (0xD800..=0xDBFF).contains(&cp) {
                    self.expect(b'\\')?;
                    self.expect(b'u')?;
                    let low = self.hex4()?;
                    if !(0xDC00..=0xDFFF).contains(&low) {
                        return Err(format!("非法低代理 U+{low:04X}"));
                    }
                    cp = 0x10000 + ((cp - 0xD800) << 10) + (low - 0xDC00);
                }
                let ch = char::from_u32(cp).ok_or_else(|| format!("非法码点 U+{cp:04X}"))?;
                let mut buf = [0u8; 4];
                out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
            }
            other => return Err(format!("未知转义 `\\{}`", other as char)),
        }
        Ok(())
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let end = self.pos + 4;
        let slice = self
            .src
            .get(self.pos..end)
            .ok_or_else(|| "`\\u` 后不足 4 位十六进制".to_string())?;
        let text = std::str::from_utf8(slice).map_err(|e| format!("`\\u` 后不是 ASCII：{e}"))?;
        let v = u32::from_str_radix(text, 16).map_err(|e| format!("`\\u{text}` 非法：{e}"))?;
        self.pos = end;
        Ok(v)
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.pos;
        while let Some(b) = self.peek() {
            if matches!(b, b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9') {
                self.pos += 1;
            } else {
                break;
            }
        }
        if start == self.pos {
            return Err(format!("偏移 {start} 处不是合法 JSON 值"));
        }
        let text = std::str::from_utf8(&self.src[start..self.pos])
            .map_err(|e| format!("数字区不是 ASCII：{e}"))?;
        let v: f64 = text
            .parse()
            .map_err(|e| format!("数字 `{text}` 解析失败：{e}"))?;
        Ok(Json::Num(v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 解析嵌套与转义() {
        let v = parse(r#"{"a":[1,2,{"b":"x\ny"}],"c":null,"d":true}"#).expect("应能解析");
        assert_eq!(
            v.get("a").and_then(|a| a.as_arr()).map(|a| a.len()),
            Some(3)
        );
        let b = v
            .get("a")
            .and_then(|a| a.as_arr())
            .and_then(|a| a.get(2))
            .and_then(|o| o.get("b"))
            .and_then(|s| s.as_str());
        assert_eq!(b, Some("x\ny"));
        assert_eq!(v.get("c"), Some(&Json::Null));
        assert_eq!(v.get("d"), Some(&Json::Bool(true)));
        assert_eq!(
            v.get("a")
                .and_then(|a| a.as_arr())
                .and_then(|a| a.get(1))
                .and_then(|n| n.as_u32()),
            Some(2)
        );
    }

    #[test]
    fn 拒绝尾部垃圾与坏结构() {
        assert!(parse(r#"{"a":1} x"#).is_err());
        assert!(parse("{").is_err());
        assert!(parse("[1,]").is_err());
    }

    #[test]
    fn 小数不当整数用() {
        let v = parse("[92.0, 92.5]").expect("应能解析");
        let arr = v.as_arr().expect("是数组");
        assert_eq!(arr[0].as_u32(), Some(92));
        assert_eq!(arr[1].as_u32(), None);
    }

    #[test]
    fn 代理对解码() {
        let v = parse(r#""\uD83D\uDE00""#).expect("应能解析");
        assert_eq!(v.as_str(), Some("\u{1F600}"));
    }
}
