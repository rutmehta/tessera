//! Typed boolean catalog predicates, compiled using bound values only.
use rusqlite::types::ToSql;
use serde::{Deserialize, Serialize};

/// Numeric comparison; SQL operators are selected from this closed set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Comparison {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl Comparison {
    fn sql(self) -> &'static str {
        match self {
            Self::Eq => "=",
            Self::Ne => "!=",
            Self::Lt => "<",
            Self::Le => "<=",
            Self::Gt => ">",
            Self::Ge => ">=",
        }
    }
}

/// A two-valued boolean filter: absent/NULL leaf values do not match.
/// Empty `All` is true and empty `Any` is false.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Predicate {
    All(Vec<Predicate>),
    Any(Vec<Predicate>),
    Not(Box<Predicate>),
    /// FTS5 query syntax, as with `Query::text`.
    Text(String),
    /// Includes descendants in the keyword hierarchy.
    Keyword(String),
    Camera(String),
    Lens(String),
    Decision(String),
    Mark(String),
    DateFrom(String),
    /// Exclusive upper capture-time boundary.
    DateBefore(String),
    Grade(Comparison, f64),
    /// Latest score whose signal is `focus` (not per-face sharpness).
    Focus(Comparison, f64),
    /// Named person keyword, including descendants. The current schema has
    /// no persistent face/person identities; face ordinals are not people.
    Person(String),
    /// Explicit scope; empty scopes match nothing. IDs use catalog hex strings.
    Ids(Vec<engine_api::id::ImageId>),
}

impl Predicate {
    pub(crate) fn compile(&self, values: &mut Vec<Box<dyn ToSql>>) -> String {
        match self {
            Self::All(children) | Self::Any(children) => {
                let all = matches!(self, Self::All(_));
                if children.is_empty() {
                    return if all { "1" } else { "0" }.into();
                }
                let clauses: Vec<_> = children.iter().map(|p| p.compile(values)).collect();
                format!("({})", clauses.join(if all { " AND " } else { " OR " }))
            }
            Self::Not(child) => format!("NOT COALESCE(({}),0)", child.compile(values)),
            Self::Ids(ids) => {
                // One JSON parameter avoids SQLite's variable limit for large groups.
                let ids: Vec<_> = ids.iter().map(ToString::to_string).collect();
                values.push(Box::new(
                    serde_json::to_string(&ids).expect("string IDs serialize"),
                ));
                "i.id IN (SELECT value FROM json_each(?))".into()
            }
            Self::Camera(value)
            | Self::Lens(value)
            | Self::Decision(value)
            | Self::Mark(value)
            | Self::DateFrom(value)
            | Self::DateBefore(value) => {
                values.push(Box::new(value.clone()));
                let column = match self {
                    Self::Camera(_) => "i.camera",
                    Self::Lens(_) => "i.lens",
                    Self::Decision(_) => "s.decision",
                    Self::Mark(_) => "s.mark",
                    Self::DateFrom(_) | Self::DateBefore(_) => "i.capture_time",
                    _ => unreachable!(),
                };
                let op = match self {
                    Self::DateFrom(_) => ">=",
                    Self::DateBefore(_) => "<",
                    _ => "=",
                };
                format!("COALESCE({column}{op}?,0)")
            }
            Self::Text(value) => {
                values.push(Box::new(value.clone()));
                "i.rowid IN (SELECT rowid FROM fts WHERE fts MATCH ?)".into()
            }
            Self::Keyword(value) | Self::Person(value) => {
                values.push(Box::new(value.clone()));
                "i.id IN (SELECT ik.image_id FROM keyword k JOIN keyword_closure c ON c.ancestor_id=k.id JOIN image_keyword ik ON ik.keyword_id=c.descendant_id WHERE k.name=?)".into()
            }
            Self::Grade(comparison, value) => {
                values.push(Box::new(*value));
                format!("COALESCE(s.grade{}?,0)", comparison.sql())
            }
            Self::Focus(comparison, value) => {
                values.push(Box::new(*value));
                format!(
                    "EXISTS (SELECT 1 FROM score score_filter WHERE score_filter.image_id=i.id AND score_filter.signal='focus' AND score_filter.value{}?)",
                    comparison.sql()
                )
            }
        }
    }
}
