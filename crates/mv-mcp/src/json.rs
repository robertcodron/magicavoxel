//! Self-contained JSON parser, serializer, and Base64 encoder for MCP JSON-RPC 2.0.
//! Keeps `mv-mcp` dependency-free beyond workspace crates (`rustc 1.87.0` compatible).

use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq)]
pub enum JsonValue {
    Null,
    Bool(bool),
    Number(f64),
    Str(String),
    Array(Vec<JsonValue>),
    Object(Vec<(String, JsonValue)>),
}

impl JsonValue {
    pub fn null() -> Self {
        JsonValue::Null
    }

    pub fn bool(b: bool) -> Self {
        JsonValue::Bool(b)
    }

    pub fn int(n: i64) -> Self {
        JsonValue::Number(n as f64)
    }

    pub fn uint(n: usize) -> Self {
        JsonValue::Number(n as f64)
    }

    pub fn float(n: f64) -> Self {
        JsonValue::Number(if n.is_finite() { n } else { 0.0 })
    }

    pub fn str(s: impl Into<String>) -> Self {
        JsonValue::Str(s.into())
    }

    pub fn array(items: Vec<JsonValue>) -> Self {
        JsonValue::Array(items)
    }

    pub fn object(fields: Vec<(impl Into<String>, JsonValue)>) -> Self {
        JsonValue::Object(fields.into_iter().map(|(k, v)| (k.into(), v)).collect())
    }

    pub fn get(&self, key: &str) -> Option<&JsonValue> {
        match self {
            JsonValue::Object(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            JsonValue::Str(s) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            JsonValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            JsonValue::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            JsonValue::Number(n) => Some(*n as i64),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self {
            JsonValue::Number(n) if *n >= 0.0 => Some(*n as u64),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[JsonValue]> {
        match self {
            JsonValue::Array(a) => Some(a.as_slice()),
            _ => None,
        }
    }

    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(JsonValue::as_str)
    }

    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.get(key).and_then(JsonValue::as_bool)
    }

    pub fn get_i64(&self, key: &str) -> Option<i64> {
        self.get(key).and_then(JsonValue::as_i64)
    }

    pub fn get_i32(&self, key: &str) -> Option<i32> {
        self.get_i64(key).map(|v| v as i32)
    }

    pub fn get_u32(&self, key: &str) -> Option<u32> {
        self.get(key).and_then(JsonValue::as_u64).map(|v| v as u32)
    }

    pub fn get_usize(&self, key: &str) -> Option<usize> {
        self.get(key)
            .and_then(JsonValue::as_u64)
            .map(|v| v as usize)
    }

    pub fn get_u8(&self, key: &str) -> Option<u8> {
        self.get(key)
            .and_then(JsonValue::as_u64)
            .map(|v| v.min(255) as u8)
    }

    pub fn get_f32(&self, key: &str) -> Option<f32> {
        self.get(key).and_then(JsonValue::as_f64).map(|v| v as f32)
    }

    pub fn get_array(&self, key: &str) -> Option<&[JsonValue]> {
        self.get(key).and_then(JsonValue::as_array)
    }

    pub fn get_ivec3(&self, key: &str) -> Option<[i32; 3]> {
        let arr = self.get_array(key)?;
        if arr.len() < 3 {
            return None;
        }
        Some([
            arr[0].as_i64()? as i32,
            arr[1].as_i64()? as i32,
            arr[2].as_i64()? as i32,
        ])
    }

    pub fn get_vec3(&self, key: &str) -> Option<[f32; 3]> {
        let arr = self.get_array(key)?;
        if arr.len() < 3 {
            return None;
        }
        Some([
            arr[0].as_f64()? as f32,
            arr[1].as_f64()? as f32,
            arr[2].as_f64()? as f32,
        ])
    }

    pub fn to_json_string(&self) -> String {
        let mut out = String::new();
        self.write_to(&mut out);
        out
    }

    fn write_to(&self, out: &mut String) {
        match self {
            JsonValue::Null => out.push_str("null"),
            JsonValue::Bool(true) => out.push_str("true"),
            JsonValue::Bool(false) => out.push_str("false"),
            JsonValue::Number(n) => {
                if !n.is_finite() {
                    out.push('0');
                } else if n.fract() == 0.0 && n.abs() < (i64::MAX as f64) {
                    let _ = write!(out, "{}", *n as i64);
                } else {
                    let _ = write!(out, "{n}");
                }
            }
            JsonValue::Str(s) => write_escaped_str(out, s),
            JsonValue::Array(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    item.write_to(out);
                }
                out.push(']');
            }
            JsonValue::Object(fields) => {
                out.push('{');
                for (i, (k, v)) in fields.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_escaped_str(out, k);
                    out.push(':');
                    v.write_to(out);
                }
                out.push('}');
            }
        }
    }

    pub fn parse(input: &str) -> Result<JsonValue, String> {
        let mut parser = Parser {
            bytes: input.as_bytes(),
            pos: 0,
        };
        let value = parser.parse_value()?;
        parser.skip_ws();
        if parser.pos < parser.bytes.len() {
            return Err(format!(
                "Unexpected trailing characters at byte {}",
                parser.pos
            ));
        }
        Ok(value)
    }
}

fn write_escaped_str(out: &mut String, s: &str) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len() {
            match self.bytes[self.pos] {
                b' ' | b'\t' | b'\n' | b'\r' => self.pos += 1,
                _ => break,
            }
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn parse_value(&mut self) -> Result<JsonValue, String> {
        self.skip_ws();
        let b = self
            .peek()
            .ok_or_else(|| "Unexpected end of JSON".to_string())?;
        match b {
            b'n' => self.parse_literal(b"null", JsonValue::Null),
            b't' => self.parse_literal(b"true", JsonValue::Bool(true)),
            b'f' => self.parse_literal(b"false", JsonValue::Bool(false)),
            b'"' => self.parse_string().map(JsonValue::Str),
            b'[' => self.parse_array(),
            b'{' => self.parse_object(),
            b'-' | b'0'..=b'9' => self.parse_number(),
            _ => Err(format!("Unexpected byte '{}' at {}", b as char, self.pos)),
        }
    }

    fn parse_literal(&mut self, lit: &[u8], val: JsonValue) -> Result<JsonValue, String> {
        if self.bytes.get(self.pos..self.pos + lit.len()) == Some(lit) {
            self.pos += lit.len();
            Ok(val)
        } else {
            Err(format!("Invalid literal at {}", self.pos))
        }
    }

    fn parse_string(&mut self) -> Result<String, String> {
        if self.peek() != Some(b'"') {
            return Err(format!("Expected '\"' at {}", self.pos));
        }
        self.pos += 1;
        let mut out = String::new();
        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            self.pos += 1;
            match b {
                b'"' => return Ok(out),
                b'\\' => {
                    let esc = *self
                        .bytes
                        .get(self.pos)
                        .ok_or_else(|| "Unterminated escape".to_string())?;
                    self.pos += 1;
                    match esc {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{08}'),
                        b'f' => out.push('\u{0C}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hex = self
                                .bytes
                                .get(self.pos..self.pos + 4)
                                .ok_or_else(|| "Invalid \\u escape".to_string())?;
                            let hex_str = std::str::from_utf8(hex)
                                .map_err(|_| "Invalid \\u hex".to_string())?;
                            let code = u32::from_str_radix(hex_str, 16)
                                .map_err(|_| "Invalid \\u hex digits".to_string())?;
                            self.pos += 4;
                            let mut code = code;
                            if (0xD800..0xDC00).contains(&code)
                                && self.bytes.get(self.pos..self.pos + 2) == Some(b"\\u")
                            {
                                let low = self
                                    .bytes
                                    .get(self.pos + 2..self.pos + 6)
                                    .and_then(|h| std::str::from_utf8(h).ok())
                                    .and_then(|h| u32::from_str_radix(h, 16).ok());
                                if let Some(low) = low.filter(|l| (0xDC00..0xE000).contains(l)) {
                                    code = 0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00);
                                    self.pos += 6;
                                }
                            }
                            if let Some(ch) = char::from_u32(code) {
                                out.push(ch);
                            } else {
                                out.push('\u{FFFD}');
                            }
                        }
                        other => out.push(other as char),
                    }
                }
                _ => {
                    self.pos -= 1;
                    let rest = &self.bytes[self.pos..];
                    let s = std::str::from_utf8(rest)
                        .map_err(|_| format!("Invalid UTF-8 at {}", self.pos))?;
                    let ch = s.chars().next().unwrap();
                    out.push(ch);
                    self.pos += ch.len_utf8();
                }
            }
        }
        Err("Unterminated JSON string".to_string())
    }

    fn parse_array(&mut self) -> Result<JsonValue, String> {
        self.pos += 1; // '['
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(JsonValue::Array(items));
        }
        loop {
            items.push(self.parse_value()?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                }
                Some(b']') => {
                    self.pos += 1;
                    return Ok(JsonValue::Array(items));
                }
                _ => return Err(format!("Expected ',' or ']' at {}", self.pos)),
            }
        }
    }

    fn parse_object(&mut self) -> Result<JsonValue, String> {
        self.pos += 1; // '{'
        let mut fields = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(JsonValue::Object(fields));
        }
        loop {
            self.skip_ws();
            let key = self.parse_string()?;
            self.skip_ws();
            if self.peek() != Some(b':') {
                return Err(format!("Expected ':' at {}", self.pos));
            }
            self.pos += 1;
            let val = self.parse_value()?;
            fields.push((key, val));
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                }
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(JsonValue::Object(fields));
                }
                _ => return Err(format!("Expected ',' or '}}' at {}", self.pos)),
            }
        }
    }

    fn parse_number(&mut self) -> Result<JsonValue, String> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        let s = std::str::from_utf8(&self.bytes[start..self.pos])
            .map_err(|_| "Invalid number UTF-8".to_string())?;
        let n: f64 = s
            .parse()
            .map_err(|_| format!("Invalid number '{s}' at {start}"))?;
        Ok(JsonValue::Number(n))
    }
}

/// Standard RFC 4648 Base64 encoding (used for PNG image payloads in MCP responses).
pub fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_nested() {
        let src = r#"{"a":[1,2,{"b":null,"c":[true,false,"x"]}],"d":{"e":-3.5,"f":""},"g":[]}"#;
        let v = JsonValue::parse(src).unwrap();
        let out = v.to_json_string();
        assert_eq!(out, src);
        assert_eq!(JsonValue::parse(&out).unwrap(), v);
        assert_eq!(v.get("a").unwrap().as_array().unwrap().len(), 3);
    }

    #[test]
    fn whitespace_is_tolerated() {
        let v = JsonValue::parse(" \n\t{ \"a\" : [ 1 , 2 ] }\r\n ").unwrap();
        assert_eq!(v.get_array("a").unwrap().len(), 2);
    }

    #[test]
    fn string_escapes_roundtrip() {
        let s = "q\" b\\ n\n t\t r\r bs\u{08} ff\u{0C} ctl\u{01}\u{1f} é 日本";
        let json = JsonValue::str(s).to_json_string();
        assert!(!json.contains('\n'));
        assert!(json.contains("\\u0001"));
        assert_eq!(JsonValue::parse(&json).unwrap().as_str(), Some(s));
    }

    #[test]
    fn parse_escape_sequences() {
        let v = JsonValue::parse(r#""a\"b\\c\/d\n\t\u0041\u00e9""#).unwrap();
        assert_eq!(v.as_str(), Some("a\"b\\c/d\n\tAé"));
    }

    #[test]
    fn unicode_surrogate_pair() {
        let v = JsonValue::parse(r#""\ud83d\ude00""#).unwrap();
        assert_eq!(v.as_str(), Some("😀"));
        let v = JsonValue::parse("\"😀\"").unwrap();
        assert_eq!(v.as_str(), Some("😀"));
        // Lone surrogate degrades to the replacement character instead of failing.
        let v = JsonValue::parse(r#""\ud83dx""#).unwrap();
        assert_eq!(v.as_str(), Some("\u{FFFD}x"));
    }

    #[test]
    fn numbers() {
        assert_eq!(JsonValue::parse("42").unwrap().as_i64(), Some(42));
        assert_eq!(JsonValue::parse("-7").unwrap().as_i64(), Some(-7));
        assert_eq!(JsonValue::parse("0").unwrap().as_i64(), Some(0));
        assert_eq!(JsonValue::parse("3.25").unwrap().as_f64(), Some(3.25));
        assert_eq!(JsonValue::parse("-0.5").unwrap().as_f64(), Some(-0.5));
        assert_eq!(JsonValue::parse("1e3").unwrap().as_f64(), Some(1000.0));
        assert_eq!(JsonValue::parse("2.5E-2").unwrap().as_f64(), Some(0.025));
        assert_eq!(JsonValue::parse("1e+2").unwrap().as_f64(), Some(100.0));
        assert_eq!(JsonValue::int(5).to_json_string(), "5");
        assert_eq!(JsonValue::int(-5).to_json_string(), "-5");
        assert_eq!(JsonValue::uint(7).to_json_string(), "7");
        assert_eq!(JsonValue::Number(3.0).to_json_string(), "3");
        assert_eq!(JsonValue::float(1.5).to_json_string(), "1.5");
        assert_eq!(JsonValue::float(f64::NAN).to_json_string(), "0");
        assert_eq!(JsonValue::Number(f64::INFINITY).to_json_string(), "0");
        assert_eq!(JsonValue::as_u64(&JsonValue::Number(-1.0)), None);
    }

    #[test]
    fn invalid_inputs() {
        for bad in [
            "",
            "   ",
            "{} x",
            "[1,2]]",
            "\"abc",
            "\"abc\\",
            "tru",
            "nul",
            "falsey",
            "[1,",
            "[1 2]",
            "{\"a\"}",
            "{\"a\":1,}",
            "{a:1}",
            "-",
            "[,]",
            "\"\\u12\"",
            "\"\\uZZZZ\"",
            "@",
        ] {
            assert!(JsonValue::parse(bad).is_err(), "should fail: {bad:?}");
        }
    }

    #[test]
    fn getters() {
        let v = JsonValue::parse(
            r#"{"s":"hi","b":true,"i":-3,"u":300,"neg":-1,"f":1.5,"p":[1,-2,3],"short":[1,2],"bad":[1,"x",3],"fp":[0.5,1,2]}"#,
        )
        .unwrap();
        assert_eq!(v.get_str("s"), Some("hi"));
        assert_eq!(v.get_bool("b"), Some(true));
        assert_eq!(v.get_i64("i"), Some(-3));
        assert_eq!(v.get_i32("i"), Some(-3));
        assert_eq!(v.get_u32("u"), Some(300));
        assert_eq!(v.get_usize("u"), Some(300));
        assert_eq!(v.get_f32("f"), Some(1.5));
        assert_eq!(v.get_u8("u"), Some(255)); // clamped
        assert_eq!(v.get_u8("neg"), None);
        assert_eq!(v.get_u8("missing"), None);
        assert_eq!(v.get_u8("i"), None);
        assert_eq!(v.get_ivec3("p"), Some([1, -2, 3]));
        assert_eq!(v.get_ivec3("short"), None);
        assert_eq!(v.get_ivec3("bad"), None);
        assert_eq!(v.get_ivec3("s"), None);
        assert_eq!(v.get_vec3("fp"), Some([0.5, 1.0, 2.0]));
        assert!(v.get("nope").is_none());
        assert!(JsonValue::Null.get("a").is_none());
        assert!(v.get_str("b").is_none());
    }

    #[test]
    fn constructors() {
        let o = JsonValue::object(vec![
            ("k", JsonValue::null()),
            ("b", JsonValue::bool(false)),
        ]);
        assert_eq!(o.to_json_string(), r#"{"k":null,"b":false}"#);
        assert_eq!(JsonValue::array(vec![]).to_json_string(), "[]");
    }

    #[test]
    fn base64_rfc4648_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64_encode(&[0xff, 0xfb, 0xff]), "//v/");
    }
}
