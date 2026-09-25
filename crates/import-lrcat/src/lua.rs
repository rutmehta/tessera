//! A data-only reader for Lightroom saved-search tables; never executes Lua.
use std::collections::BTreeMap;

use engine_api::error::{EngineError, EngineResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A saved search, preserving its nested boolean structure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SavedSearch {
    /// One Lightroom criterion, with its original operation and scalar value.
    Rule {
        criteria: String,
        operation: String,
        value: Value,
    },
    /// All children must match.
    All(Vec<SavedSearch>),
    /// At least one child must match.
    Any(Vec<SavedSearch>),
    /// No child may match.
    None(Vec<SavedSearch>),
}

/// Parse a table or `s = { ... }`, with an optional final semicolon.
///
/// Only search fields, nested tables, positive integer indices, quoted UTF-8
/// strings, decimal numbers and booleans are accepted. Expressions, calls,
/// comments, and additional statements are rejected, not evaluated. Duplicate
/// fields/indices and mixed rule/group tables are errors rather than silently
/// discarded data. Recursion is bounded to 64 tables.
pub fn parse(text: &str) -> EngineResult<SavedSearch> {
    let mut p = Parser { text, pos: 0 };
    p.ws();
    if p.peek() != Some(b'{') {
        if p.ident()? != "s" {
            return Err(p.error("only the s assignment wrapper is supported"));
        }
        p.expect(b'=')?;
    }
    let result = p.table(0)?;
    p.take(b';');
    p.ws();
    if p.pos != text.len() {
        return Err(p.error("trailing content"));
    }
    Ok(result)
}

struct Parser<'a> {
    text: &'a str,
    pos: usize,
}

impl Parser<'_> {
    fn error(&self, message: &str) -> EngineError {
        EngineError::Decode {
            format: "lightroom-saved-search".into(),
            message: format!("{message} at byte {}", self.pos),
        }
    }

    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.pos).copied()
    }

    fn ws(&mut self) {
        while self.peek().is_some_and(|b| b.is_ascii_whitespace()) {
            self.pos += 1;
        }
    }

    fn take(&mut self, byte: u8) -> bool {
        self.ws();
        if self.peek() == Some(byte) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, byte: u8) -> EngineResult<()> {
        if self.take(byte) {
            Ok(())
        } else {
            Err(self.error("unexpected token"))
        }
    }

    fn ident(&mut self) -> EngineResult<String> {
        self.ws();
        let start = self.pos;
        if !self
            .peek()
            .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        {
            return Err(self.error("expected identifier"));
        }
        self.pos += 1;
        while self
            .peek()
            .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            self.pos += 1;
        }
        Ok(self.text[start..self.pos].into())
    }

    fn string(&mut self) -> EngineResult<String> {
        self.ws();
        let quote = self.peek().ok_or_else(|| self.error("expected string"))?;
        if quote != b'\'' && quote != b'"' {
            return Err(self.error("expected quoted string"));
        }
        self.pos += 1;
        // Lua decimal escapes denote bytes, not Unicode code points.
        let mut bytes = Vec::new();
        while let Some(b) = self.peek() {
            self.pos += 1;
            if b == quote {
                return String::from_utf8(bytes).map_err(|_| self.error("string is not UTF-8"));
            }
            if matches!(b, b'\n' | b'\r') {
                return Err(self.error("unescaped newline in string"));
            }
            if b != b'\\' {
                bytes.push(b);
                continue;
            }
            let escaped = self.peek().ok_or_else(|| self.error("unfinished escape"))?;
            self.pos += 1;
            bytes.push(match escaped {
                b'\\' | b'\'' | b'"' => escaped,
                b'a' => 7,
                b'b' => 8,
                b'f' => 12,
                b'n' => b'\n',
                b'r' => b'\r',
                b't' => b'\t',
                b'v' => 11,
                b'\n' => b'\n',
                b'\r' => {
                    if self.peek() == Some(b'\n') {
                        self.pos += 1;
                    }
                    b'\n'
                }
                b'0'..=b'9' => {
                    let mut n = u16::from(escaped - b'0');
                    for _ in 0..2 {
                        if let Some(d @ b'0'..=b'9') = self.peek() {
                            n = n * 10 + u16::from(d - b'0');
                            self.pos += 1;
                        } else {
                            break;
                        }
                    }
                    u8::try_from(n).map_err(|_| self.error("decimal escape exceeds 255"))?
                }
                _ => return Err(self.error("unsupported string escape")),
            });
        }
        Err(self.error("unterminated string"))
    }

    fn digits(&mut self) -> usize {
        let start = self.pos;
        while self.peek().is_some_and(|b| b.is_ascii_digit()) {
            self.pos += 1;
        }
        self.pos - start
    }

    fn scalar(&mut self) -> EngineResult<Value> {
        self.ws();
        if matches!(self.peek(), Some(b'\'' | b'"')) {
            return self.string().map(Value::String);
        }
        if self.peek().is_some_and(|b| b.is_ascii_alphabetic()) {
            return match self.ident()?.as_str() {
                "true" => Ok(Value::Bool(true)),
                "false" => Ok(Value::Bool(false)),
                _ => Err(self.error("only literal scalar values are supported")),
            };
        }
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        let mut digits = self.digits();
        let mut fractional = false;
        if self.peek() == Some(b'.') {
            fractional = true;
            self.pos += 1;
            digits += self.digits();
        }
        if digits == 0 {
            return Err(self.error("expected decimal number"));
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            fractional = true;
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if self.digits() == 0 {
                return Err(self.error("missing exponent"));
            }
        }
        let number = &self.text[start..self.pos];
        if !fractional {
            if let Ok(n) = number.parse::<i64>() {
                return Ok(n.into());
            }
            if let Ok(n) = number.parse::<u64>() {
                return Ok(n.into());
            }
            return Err(self.error("integer out of range"));
        }
        number
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .ok_or_else(|| self.error("non-finite or invalid number"))
    }

    fn table(&mut self, depth: usize) -> EngineResult<SavedSearch> {
        if depth >= 64 {
            return Err(self.error("table nesting exceeds 64"));
        }
        self.expect(b'{')?;
        let mut fields = BTreeMap::new();
        let mut children = BTreeMap::new();
        let mut implicit_index = 1_u64;
        while !self.take(b'}') {
            if self.peek() == Some(b'{') || self.peek() == Some(b'[') {
                let index = if self.take(b'[') {
                    self.ws();
                    let start = self.pos;
                    self.digits();
                    let n = self.text[start..self.pos]
                        .parse::<u64>()
                        .map_err(|_| self.error("expected positive integer index"))?;
                    if n == 0 {
                        return Err(self.error("indices start at 1"));
                    }
                    self.expect(b']')?;
                    self.expect(b'=')?;
                    n
                } else {
                    let n = implicit_index;
                    implicit_index += 1;
                    n
                };
                let child = self.table(depth + 1)?;
                if children.insert(index, child).is_some() {
                    return Err(self.error("duplicate child index"));
                }
            } else {
                let key = self.ident()?;
                self.expect(b'=')?;
                let value = self.scalar()?;
                if fields.insert(key, value).is_some() {
                    return Err(self.error("duplicate field"));
                }
            }
            if self.take(b'}') {
                break;
            }
            if !self.take(b',') && !self.take(b';') {
                return Err(self.error("expected table field separator"));
            }
        }
        if let Some(combine) = fields.remove("combine") {
            if !fields.is_empty() {
                return Err(self.error("group contains rule or unknown fields"));
            }
            let children = children.into_values().collect();
            return match combine.as_str() {
                Some("intersect" | "and") => Ok(SavedSearch::All(children)),
                Some("union" | "or") => Ok(SavedSearch::Any(children)),
                Some("exclude" | "none") => Ok(SavedSearch::None(children)),
                _ => Err(self.error("unknown combine operation")),
            };
        }
        if !children.is_empty() {
            return Err(self.error("children require a combine operation"));
        }
        let criteria = fields
            .remove("criteria")
            .and_then(|v| v.as_str().map(str::to_owned))
            .ok_or_else(|| self.error("missing or non-string criteria"))?;
        let operation = fields
            .remove("operation")
            .and_then(|v| v.as_str().map(str::to_owned))
            .ok_or_else(|| self.error("missing or non-string operation"))?;
        let value = fields
            .remove("value")
            .ok_or_else(|| self.error("missing value"))?;
        if !fields.is_empty() {
            return Err(self.error("unknown rule field"));
        }
        Ok(SavedSearch::Rule {
            criteria,
            operation,
            value,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rule(value: Value) -> SavedSearch {
        SavedSearch::Rule {
            criteria: "rating".into(),
            operation: ">=".into(),
            value,
        }
    }

    #[test]
    fn parses_nested_lightroom_data() {
        let text = r#"s = {combine='intersect', [2]={combine='exclude', {criteria='rating',operation='>=',value=false}}, [1]={combine='union', {criteria='rating',operation='>=',value=-1.5e2}, {criteria='rating',operation='>=',value='café\n\"\\\097'}},};"#;
        let expected = SavedSearch::All(vec![
            SavedSearch::Any(vec![rule(json!(-150.0)), rule(json!("café\n\"\\a"))]),
            SavedSearch::None(vec![rule(json!(false))]),
        ]);
        assert_eq!(parse(text).unwrap(), expected);
        assert_eq!(
            serde_json::from_str::<SavedSearch>(&serde_json::to_string(&expected).unwrap())
                .unwrap(),
            expected
        );
        for (alias, expected) in [
            ("and", SavedSearch::All(vec![])),
            ("or", SavedSearch::Any(vec![])),
            ("none", SavedSearch::None(vec![])),
        ] {
            assert_eq!(parse(&format!("{{combine='{alias}'}}")).unwrap(), expected);
        }
    }

    #[test]
    fn scalar_forms_preserve_types() {
        for (source, expected) in [
            ("true", json!(true)),
            ("false", json!(false)),
            ("-42", json!(-42)),
            ("18446744073709551615", json!(u64::MAX)),
            (".5", json!(0.5)),
            ("1.", json!(1.0)),
            ("2E+3", json!(2000.0)),
            (r#""it\'s\t雪\195\169""#, json!("it's\t雪é")),
        ] {
            let text = format!("{{criteria='rating';operation='>=';value={source};}}");
            assert_eq!(parse(&text).unwrap(), rule(expected), "{source}");
        }
    }

    #[test]
    fn rejects_malformed_or_executable_input() {
        for text in [
            "",
            "{}",
            "s =",
            "return {combine='and'}",
            "x = {combine='and'}",
            "{combine='and'}; os.execute('touch /tmp/unsafe')",
            "{combine='and'} {}",
            "{combine='and'};;",
            "{combine='and'}()",
            "{combine='xor'}",
            "{combine=true}",
            "{combine='and',combine='or'}",
            "{combine='and',criteria='rating'}",
            "{combine='and',foo=3}",
            "{combine='and' {combine='or'}}",
            "{combine='and',,}",
            "{combine='and',[0]={combine='or'}}",
            "{combine='and',[-1]={combine='or'}}",
            "{combine='and',[1.5]={combine='or'}}",
            "{combine='and',[1+1]={combine='or'}}",
            "{combine='and',[1]={combine='or'},[1]={combine='or'}}",
            "{combine='and',[1]={combine='or'},{combine='or'}}",
            "{{combine='and'}}",
            "{criteria='rating',operation='>='}",
            "{criteria=false,operation='>=',value=3}",
            "{criteria='rating',operation='>=',value=3,extra=1}",
        ] {
            assert!(
                matches!(parse(text), Err(EngineError::Decode { .. })),
                "accepted {text}"
            );
        }
        for value in [
            "os.execute('boom')",
            "function() end",
            "(3)",
            "3+4",
            "3..4",
            "true or false",
            "nil",
            "NaN",
            "1e9999",
            "1e",
            "--1",
            "18446744073709551616",
            "'unterminated",
            "'line\nbreak'",
            r"'\q'",
            r"'\256'",
            r"'\255'",
            "{}",
        ] {
            let text = format!("{{criteria='rating',operation='>=',value={value}}}");
            assert!(parse(&text).is_err(), "accepted {text}");
        }
    }

    #[test]
    fn rejects_excessive_nesting_and_truncation_without_panicking() {
        let deep = format!(
            "{}{}{}",
            "{combine='and',".repeat(64),
            "{combine='or'}",
            "}".repeat(64)
        );
        assert!(parse(&deep).is_err());
        let text = "s = {combine='and', {criteria='rating',operation='>=',value='雪'}}";
        for (end, _) in text.char_indices() {
            assert!(
                parse(&text[..end]).is_err(),
                "accepted prefix ending at {end}"
            );
        }
        assert!(parse(text).is_ok());
    }

    #[test]
    fn parses_rule() {
        assert_eq!(
            parse("{criteria='rating',operation='>=',value=3}").unwrap(),
            rule(json!(3))
        );
    }
}
