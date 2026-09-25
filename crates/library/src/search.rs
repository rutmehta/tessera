//! Saved searches shared by the library and Lightroom importer.
//!
//! Text syntax uses NOT > AND (including adjacency) > OR. Quoted values use
//! JSON string escaping. `@json:"..."` is a lossless, data-only escape for ASTs
//! that cannot be expressed natively (including unsupported imported rules).
//! Its payload is a preorder JSON array: Rule retains its fields; All/Any/None
//! carry child counts. The original nested JSON object is also accepted. This
//! envelope is text-only; `Serialize`/`Deserialize` retain the Lightroom shape.
//! Parsing/compilation is bounded to 64 levels, 4096 nodes and 64 KiB of
//! rule payloads. Encoded input allows 1 MiB for JSON/boolean syntax overhead.
//! `rating` aliases the native 0..=3 grade threshold, NOT Lightroom's 0..=5
//! stars. Imported thresholds outside that domain error; they are never clamped.
//! Semantic queries require `Index::search_with_semantic` and an implementation
//! of `index::SemanticSearch`; this module does not load embedding models.
use std::{fmt, str::FromStr};

use engine_api::error::{EngineError, EngineResult};
use index::{Comparison, Predicate, Query};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const MAX_BYTES: usize = 65_536;
const MAX_DEPTH: usize = 64;
const MAX_NODES: usize = 4096;

/// The serialized shape intentionally matches the original Lightroom AST.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SavedSearch {
    Rule {
        criteria: String,
        operation: String,
        value: Value,
    },
    All(Vec<SavedSearch>),
    Any(Vec<SavedSearch>),
    None(Vec<SavedSearch>),
}

// A preorder envelope keeps the lossless text escape shallow even when the
// AST reaches the supported depth. Normal serde serialization stays unchanged.
#[derive(Serialize, Deserialize)]
enum FlatNode {
    Rule {
        criteria: String,
        operation: String,
        value: Value,
    },
    All(usize),
    Any(usize),
    None(usize),
}

fn flatten(root: &SavedSearch) -> Vec<FlatNode> {
    let mut pending = vec![root];
    let mut out = Vec::new();
    while let Some(n) = pending.pop() {
        out.push(match n {
            SavedSearch::Rule {
                criteria,
                operation,
                value,
            } => FlatNode::Rule {
                criteria: criteria.clone(),
                operation: operation.clone(),
                value: value.clone(),
            },
            SavedSearch::All(v) | SavedSearch::Any(v) | SavedSearch::None(v) => {
                pending.extend(v.iter().rev());
                match n {
                    SavedSearch::All(_) => FlatNode::All(v.len()),
                    SavedSearch::Any(_) => FlatNode::Any(v.len()),
                    _ => FlatNode::None(v.len()),
                }
            }
        });
    }
    out
}

fn unflatten(nodes: Vec<FlatNode>) -> EngineResult<SavedSearch> {
    fn next(it: &mut std::vec::IntoIter<FlatNode>, depth: usize) -> EngineResult<SavedSearch> {
        if depth >= MAX_DEPTH {
            return Err(invalid("search exceeds depth limit"));
        }
        let node = it.next().ok_or_else(|| invalid("incomplete encoded AST"))?;
        Ok(match node {
            FlatNode::Rule {
                criteria,
                operation,
                value,
            } => SavedSearch::Rule {
                criteria,
                operation,
                value,
            },
            FlatNode::All(n) | FlatNode::Any(n) | FlatNode::None(n) => {
                if n > it.len() {
                    return Err(invalid("invalid encoded child count"));
                }
                let children = (0..n)
                    .map(|_| next(it, depth + 1))
                    .collect::<EngineResult<_>>()?;
                match node {
                    FlatNode::All(_) => SavedSearch::All(children),
                    FlatNode::Any(_) => SavedSearch::Any(children),
                    _ => SavedSearch::None(children),
                }
            }
        })
    }
    if nodes.len() > MAX_NODES {
        return Err(invalid("search exceeds node limit"));
    }
    let mut it = nodes.into_iter();
    let root = next(&mut it, 0)?;
    if it.next().is_some() {
        return Err(invalid("trailing encoded nodes"));
    }
    Ok(root)
}

fn error(pos: usize, message: impl fmt::Display) -> EngineError {
    EngineError::Decode {
        format: "saved-search".into(),
        message: format!("{message} at byte {pos}"),
    }
}
fn invalid(message: impl Into<String>) -> EngineError {
    EngineError::InvalidArgument {
        name: "saved search".into(),
        reason: message.into(),
    }
}

impl SavedSearch {
    /// Compile without executing the search. Unsupported imported rules fail
    /// explicitly. A semantic term must be a single positive conjunct.
    pub fn compile(&self) -> EngineResult<Query> {
        self.bounds()?;
        let mut semantic = None;
        let predicate = self.predicate(true, &mut semantic)?;
        Ok(Query {
            predicate: Some(predicate),
            semantic,
            ..Query::default()
        })
    }

    fn bounds(&self) -> EngineResult<()> {
        let mut pending = vec![(self, 0)];
        let mut count = 0;
        let mut bytes = 0usize;
        while let Some((node, depth)) = pending.pop() {
            count += 1;
            if count > MAX_NODES || depth >= MAX_DEPTH {
                return Err(invalid("search exceeds depth/node limit"));
            }
            match node {
                Self::Rule {
                    criteria,
                    operation,
                    value,
                } => {
                    bytes = bytes
                        .saturating_add(criteria.len())
                        .saturating_add(operation.len())
                        .saturating_add(value.to_string().len());
                    if bytes > MAX_BYTES {
                        return Err(invalid("search exceeds size limit"));
                    }
                }
                Self::All(v) | Self::Any(v) | Self::None(v) => {
                    if v.len() > MAX_NODES {
                        return Err(invalid("search exceeds node limit"));
                    }
                    pending.extend(v.iter().map(|n| (n, depth + 1)));
                }
            }
        }
        Ok(())
    }

    fn predicate(&self, positive: bool, semantic: &mut Option<String>) -> EngineResult<Predicate> {
        match self {
            Self::All(v) => Ok(Predicate::All(
                v.iter()
                    .map(|n| n.predicate(positive, semantic))
                    .collect::<EngineResult<_>>()?,
            )),
            Self::Any(v) => Ok(Predicate::Any(
                v.iter()
                    .map(|n| n.predicate(false, semantic))
                    .collect::<EngineResult<_>>()?,
            )),
            Self::None(v) => Ok(Predicate::Not(Box::new(Predicate::Any(
                v.iter()
                    .map(|n| n.predicate(false, semantic))
                    .collect::<EngineResult<_>>()?,
            )))),
            Self::Rule {
                criteria,
                operation,
                value,
            } => {
                if criteria == "semantic" {
                    let text = string_value(value)?;
                    if !matches!(operation.as_str(), ":" | "=") {
                        return Err(invalid("unsupported semantic operation"));
                    }
                    if !positive || semantic.is_some() {
                        return Err(invalid(
                            "semantic requires exactly one positive conjunct, never OR/NOT",
                        ));
                    }
                    *semantic = Some(text.into());
                    return Ok(Predicate::All(vec![]));
                }
                rule_predicate(criteria, operation, value)
            }
        }
    }
}

fn string_value(value: &Value) -> EngineResult<&str> {
    value
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| invalid("expected nonempty string value"))
}
fn comparison(op: &str) -> EngineResult<Comparison> {
    match op {
        ":" | "=" | "==" => Ok(Comparison::Eq),
        "!=" => Ok(Comparison::Ne),
        "<" => Ok(Comparison::Lt),
        "<=" => Ok(Comparison::Le),
        ">" => Ok(Comparison::Gt),
        ">=" => Ok(Comparison::Ge),
        _ => Err(invalid(format!("unsupported comparison {op:?}"))),
    }
}
fn rule_predicate(field: &str, op: &str, value: &Value) -> EngineResult<Predicate> {
    if matches!(field, "rating" | "grade" | "focus") {
        let cmp = comparison(op)?;
        let n = value
            .as_f64()
            .ok_or_else(|| invalid("expected numeric value"))?;
        let max = if field == "focus" { 1.0 } else { 3.0 };
        if !n.is_finite() || !(0.0..=max).contains(&n) || (field != "focus" && n.fract() != 0.0) {
            return Err(invalid(format!(
                "{field} must be {} in 0..={max}",
                if field == "focus" {
                    "a number"
                } else {
                    "an integer"
                }
            )));
        }
        return Ok(if field == "focus" {
            Predicate::Focus(cmp, n)
        } else {
            Predicate::Grade(cmp, n)
        });
    }
    if !matches!(
        field,
        "text" | "keyword" | "camera" | "lens" | "person" | "decision" | "mark" | "date"
    ) {
        return Err(invalid(format!("unsupported field {field:?}")));
    }
    if !matches!(op, ":" | "=" | "==" | "!=") || (field == "date" && op == "!=") {
        return Err(invalid(format!("unsupported operation {op:?} for {field}")));
    }
    let text = string_value(value)?;
    let p = match field {
        "text" => Predicate::Text(format!("\"{}\"", text.replace('"', "\"\""))),
        "keyword" => Predicate::Keyword(text.into()),
        "camera" => Predicate::Camera(text.into()),
        "lens" => Predicate::Lens(text.into()),
        "person" => Predicate::Person(text.into()),
        "mark" => Predicate::Mark(text.into()),
        "decision" => {
            if !matches!(text, "keep" | "reject" | "undecided") {
                return Err(invalid("decision must be keep, reject or undecided"));
            }
            Predicate::Decision(text.into())
        }
        "date" => {
            let (a, b) = text.split_once("..").unwrap_or((text, text));
            let (start, _) = date_bounds(a)?;
            let (_, end) = date_bounds(b)?;
            if start >= end {
                return Err(invalid("date range is reversed"));
            }
            Predicate::All(vec![Predicate::DateFrom(start), Predicate::DateBefore(end)])
        }
        _ => unreachable!(),
    };
    Ok(if op == "!=" {
        Predicate::Not(Box::new(p))
    } else {
        p
    })
}

/// ISO calendar prefixes denote inclusive years/months/days. The upper bound
/// advances by the precision of the supplied end, including leap days.
fn date_bounds(s: &str) -> EngineResult<(String, String)> {
    let parts: Vec<_> = s.split('-').collect();
    if parts.is_empty()
        || parts.len() > 3
        || parts[0].len() != 4
        || parts.iter().skip(1).any(|p| p.len() != 2)
        || parts.iter().any(|p| !p.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(invalid("date must be YYYY, YYYY-MM or YYYY-MM-DD"));
    }
    let y: u32 = parts[0].parse().map_err(|_| invalid("invalid year"))?;
    let m: u32 = if parts.len() >= 2 {
        parts[1].parse().map_err(|_| invalid("invalid month"))?
    } else {
        1
    };
    let d: u32 = if parts.len() == 3 {
        parts[2].parse().map_err(|_| invalid("invalid day"))?
    } else {
        1
    };
    let leap = y.is_multiple_of(4) && (!y.is_multiple_of(100) || y.is_multiple_of(400));
    let days = match m {
        2 => {
            if leap {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        _ => return Err(invalid("invalid month")),
    };
    if y == 0 || d == 0 || d > days {
        return Err(invalid("invalid calendar date"));
    }
    let (mut ey, mut em, mut ed) = (y, m, d);
    match parts.len() {
        1 => ey += 1,
        2 => em += 1,
        _ => {
            ed += 1;
            if ed > days {
                ed = 1;
                em += 1;
            }
        }
    }
    if em > 12 {
        em = 1;
        ey += 1;
    }
    if ey > 9999 {
        return Err(invalid("date exclusive boundary exceeds year 9999"));
    }
    Ok((
        format!("{y:04}-{m:02}-{d:02}"),
        format!("{ey:04}-{em:02}-{ed:02}"),
    ))
}

#[derive(Debug, Clone, PartialEq)]
enum Kind {
    Word(String),
    Quoted(String),
    Op(String),
    Left,
    Right,
}
#[derive(Debug, Clone)]
struct Token {
    kind: Kind,
    pos: usize,
}
fn tokens(s: &str) -> EngineResult<Vec<Token>> {
    let mut out = Vec::new();
    let mut p = 0;
    while p < s.len() {
        let c = s[p..].chars().next().unwrap();
        if c.is_whitespace() {
            p += c.len_utf8();
            continue;
        }
        let start = p;
        let kind = match c {
            '(' => {
                p += 1;
                Kind::Left
            }
            ')' => {
                p += 1;
                Kind::Right
            }
            '"' => {
                let mut stream = serde_json::Deserializer::from_str(&s[p..]).into_iter::<String>();
                let value = stream
                    .next()
                    .unwrap()
                    .map_err(|e| error(start, format!("invalid quoted string: {e}")))?;
                p += stream.byte_offset();
                Kind::Quoted(value)
            }
            ':' | '=' | '!' | '<' | '>' => {
                p += 1;
                while p < s.len() && matches!(s.as_bytes()[p], b':' | b'=' | b'!' | b'<' | b'>') {
                    p += 1;
                }
                Kind::Op(s[start..p].into())
            }
            _ => {
                while p < s.len() {
                    let ch = s[p..].chars().next().unwrap();
                    if ch.is_whitespace()
                        || matches!(ch, '(' | ')' | '"' | ':' | '=' | '!' | '<' | '>')
                    {
                        break;
                    }
                    p += ch.len_utf8();
                }
                Kind::Word(s[start..p].into())
            }
        };
        out.push(Token { kind, pos: start });
        if out.len() > MAX_NODES * 3 {
            return Err(error(start, "too many tokens"));
        }
    }
    Ok(out)
}
struct Parser {
    tokens: Vec<Token>,
    i: usize,
    end: usize,
}
impl Parser {
    fn pos(&self) -> usize {
        self.tokens.get(self.i).map_or(self.end, |t| t.pos)
    }
    fn word(&self, w: &str) -> bool {
        matches!(self.tokens.get(self.i), Some(Token { kind: Kind::Word(s), .. }) if s.eq_ignore_ascii_case(w))
    }
    fn or(&mut self, depth: usize) -> EngineResult<SavedSearch> {
        let mut v = vec![self.and(depth)?];
        while self.word("OR") {
            self.i += 1;
            v.push(self.and(depth)?);
        }
        Ok(if v.len() == 1 {
            v.pop().unwrap()
        } else {
            SavedSearch::Any(v)
        })
    }
    fn and(&mut self, depth: usize) -> EngineResult<SavedSearch> {
        let mut v = vec![self.atom(depth)?];
        loop {
            if self.word("AND") {
                self.i += 1;
            } else if self.i == self.tokens.len()
                || self.word("OR")
                || matches!(self.tokens[self.i].kind, Kind::Right)
            {
                break;
            }
            v.push(self.atom(depth)?);
        }
        Ok(if v.len() == 1 {
            v.pop().unwrap()
        } else {
            SavedSearch::All(v)
        })
    }
    fn atom(&mut self, depth: usize) -> EngineResult<SavedSearch> {
        if depth >= MAX_DEPTH {
            return Err(error(self.pos(), "maximum nesting depth exceeded"));
        }
        if self.word("NOT") {
            self.i += 1;
            return Ok(SavedSearch::None(vec![self.atom(depth + 1)?]));
        }
        if self.word("AND") || self.word("OR") {
            return Err(error(self.pos(), "expected expression"));
        }
        let t = self
            .tokens
            .get(self.i)
            .cloned()
            .ok_or_else(|| error(self.end, "expected expression"))?;
        self.i += 1;
        if t.kind == Kind::Left {
            let n = self.or(depth + 1)?;
            if !matches!(self.tokens.get(self.i).map(|t| &t.kind), Some(Kind::Right)) {
                return Err(error(self.pos(), "expected closing parenthesis"));
            }
            self.i += 1;
            return Ok(n);
        }
        let (text, quoted) = match t.kind {
            Kind::Word(s) => (s, false),
            Kind::Quoted(s) => (s, true),
            _ => return Err(error(t.pos, "expected expression")),
        };
        let (criteria, operation, value) =
            if !quoted && matches!(self.tokens.get(self.i).map(|t| &t.kind), Some(Kind::Op(_))) {
                let Kind::Op(op) = self.tokens[self.i].kind.clone() else {
                    unreachable!()
                };
                self.i += 1;
                let val = self
                    .tokens
                    .get(self.i)
                    .cloned()
                    .ok_or_else(|| error(self.end, "expected rule value"))?;
                self.i += 1;
                let raw = match val.kind {
                    Kind::Word(s) | Kind::Quoted(s) => s,
                    _ => return Err(error(val.pos, "expected rule value")),
                };
                let field = text.to_ascii_lowercase();
                let value = if matches!(field.as_str(), "rating" | "grade" | "focus") {
                    let n: serde_json::Number = raw
                        .parse()
                        .map_err(|_| error(val.pos, "expected finite number"))?;
                    Value::Number(n)
                } else {
                    Value::String(raw)
                };
                (field, op, value)
            } else {
                ("text".into(), ":".into(), Value::String(text))
            };
        let node = SavedSearch::Rule {
            criteria,
            operation,
            value,
        };
        // Validate leaves now so field/operator/value errors retain their source position.
        node.predicate(true, &mut None)
            .map_err(|e| error(t.pos, e))?;
        Ok(node)
    }
}
impl FromStr for SavedSearch {
    type Err = EngineError;
    fn from_str(s: &str) -> EngineResult<Self> {
        const MAX_INPUT_BYTES: usize = 1_048_576;
        if s.len() > MAX_INPUT_BYTES {
            return Err(error(MAX_INPUT_BYTES, "search exceeds size limit"));
        }
        let node = if let Some(json) = s.strip_prefix("@json:") {
            let encoded: String = serde_json::from_str(json).map_err(|e| error(6, e))?;
            if encoded.trim_start().starts_with('[') {
                unflatten(serde_json::from_str(&encoded).map_err(|e| error(6, e))?)
                    .map_err(|e| error(6, e))?
            } else {
                serde_json::from_str(&encoded).map_err(|e| error(6, e))?
            }
        } else {
            let mut p = Parser {
                tokens: tokens(s)?,
                i: 0,
                end: s.len(),
            };
            let n = p.or(0)?;
            if p.i != p.tokens.len() {
                return Err(error(p.pos(), "unexpected trailing token"));
            }
            n
        };
        node.bounds().map_err(|e| error(0, e))?;
        Ok(node)
    }
}

impl fmt::Display for SavedSearch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fn text(n: &SavedSearch) -> String {
            match n {
                SavedSearch::Rule {
                    criteria,
                    operation,
                    value,
                } => format!("{criteria}{operation}{value}"),
                SavedSearch::All(v) => {
                    format!("({})", v.iter().map(text).collect::<Vec<_>>().join(" AND "))
                }
                SavedSearch::Any(v) => {
                    format!("({})", v.iter().map(text).collect::<Vec<_>>().join(" OR "))
                }
                SavedSearch::None(v) => format!(
                    "NOT ({})",
                    v.iter().map(text).collect::<Vec<_>>().join(" OR ")
                ),
            }
        }
        let candidate = text(self);
        if candidate.parse::<SavedSearch>().as_ref() == Ok(self) {
            return f.write_str(&candidate);
        }
        // No meaning or serialized scalar type may be discarded for display.
        let json = serde_json::to_string(&flatten(self)).map_err(|_| fmt::Error)?;
        write!(
            f,
            "@json:{}",
            serde_json::to_string(&json).map_err(|_| fmt::Error)?
        )
    }
}

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;
