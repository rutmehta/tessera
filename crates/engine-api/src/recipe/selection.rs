//! The selection model (spec 06 §2): one decision, an optional grade on
//! keepers, and one user-defined mark. Derived status and AI signals are not
//! stored here.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::{EngineError, EngineResult};

/// The culling decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    /// Rejected (key X).
    Reject,
    /// Not yet decided (key U).
    #[default]
    Undecided,
    /// Kept (key P).
    Keep,
}

/// Optional grade on a kept image: keep → good → best.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub enum Grade {
    /// 1: good (client selects).
    One = 1,
    /// 2: better (portfolio).
    Two = 2,
    /// 3: best (hero).
    Three = 3,
}

impl TryFrom<u8> for Grade {
    type Error = EngineError;
    fn try_from(v: u8) -> EngineResult<Self> {
        match v {
            1 => Ok(Self::One),
            2 => Ok(Self::Two),
            3 => Ok(Self::Three),
            _ => Err(EngineError::invalid(
                "grade",
                format!("{v} is not 1, 2 or 3"),
            )),
        }
    }
}

impl From<Grade> for u8 {
    fn from(g: Grade) -> u8 {
        g as u8
    }
}

/// A user-defined mark, referenced by name so sidecars stay meaningful
/// outside the library that defined the mark set (the XMP `Label` carries
/// the same text).
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Mark(pub String);

impl Mark {
    /// Creates a mark.
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }
}

impl fmt::Debug for Mark {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Mark({:?})", self.0)
    }
}

/// Per-image selection state.
///
/// Invariant: `grade` is `None` unless `decision == Keep`. Constructors and
/// setters maintain it; [`Selection::normalized`] repairs deserialized input.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Selection {
    /// Decision.
    pub decision: Decision,
    /// Grade (keepers only).
    pub grade: Option<Grade>,
    /// Mark.
    pub mark: Option<Mark>,
}

impl Selection {
    /// A kept image with an optional grade.
    pub fn keep(grade: Option<Grade>) -> Self {
        Self {
            decision: Decision::Keep,
            grade,
            mark: None,
        }
    }

    /// Sets the decision; leaving Keep clears the grade.
    pub fn set_decision(&mut self, decision: Decision) {
        self.decision = decision;
        if decision != Decision::Keep {
            self.grade = None;
        }
    }

    /// Sets a grade, which implies Keep (spec 06 §2: grades are given "on a Keep").
    pub fn set_grade(&mut self, grade: Option<Grade>) {
        if grade.is_some() {
            self.decision = Decision::Keep;
        }
        self.grade = grade;
    }

    /// Returns a copy satisfying the invariant.
    pub fn normalized(mut self) -> Self {
        if self.decision != Decision::Keep {
            self.grade = None;
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grade_serializes_as_number() {
        assert_eq!(serde_json::to_string(&Grade::Two).unwrap(), "2");
        assert!(serde_json::from_str::<Grade>("4").is_err());
        let s = Selection {
            decision: Decision::Keep,
            grade: Some(Grade::Three),
            mark: Some(Mark::new("client favourite")),
        };
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(
            json,
            r#"{"decision":"keep","grade":3,"mark":"client favourite"}"#
        );
        assert_eq!(serde_json::from_str::<Selection>(&json).unwrap(), s);
    }

    #[test]
    fn invariant_is_kept() {
        let mut s = Selection::default();
        s.set_grade(Some(Grade::One));
        assert_eq!(s.decision, Decision::Keep);
        s.set_decision(Decision::Reject);
        assert_eq!(s.grade, None);
        let raw: Selection = serde_json::from_str(r#"{"decision":"reject","grade":2}"#).unwrap();
        assert_eq!(raw.normalized().grade, None);
        assert_eq!(
            serde_json::from_str::<Selection>("{}").unwrap(),
            Selection::default()
        );
    }
}
