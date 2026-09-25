//! Append-only edit history with branching undo and named snapshots.
//!
//! Each [`HistoryEntry`] stores the *changes* it made to the develop
//! settings as JSON-pointer patches, plus a pointer to its parent entry. The
//! entries form a tree rooted at [`History::base`]; `head` names the entry
//! the current settings correspond to. Undo moves `head` to the parent, redo
//! to the newest child, and a new edit after an undo starts a branch — no
//! entry is ever modified or removed, so every past state stays reachable.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::settings::DevelopSettings;
use crate::error::{EngineError, EngineResult};
use crate::id::{HistoryEntryId, HistoryGroupId, ImageId, StyleId};

/// Who made an edit.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Author {
    /// The user, interactively.
    #[default]
    User,
    /// An agent acting through the tool API.
    Agent {
        /// Agent name / model.
        name: String,
    },
    /// Imported from another application.
    Import {
        /// Source, e.g. `"lightroom-classic"`.
        source: String,
    },
    /// Applied from a preset or style.
    Preset {
        /// Style id.
        style: StyleId,
    },
    /// Copied from another image (sync / paste settings).
    Sync {
        /// Source image.
        from: ImageId,
    },
}

/// One patch operation on the serialized [`DevelopSettings`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ParamChange {
    /// Set the value at `path`, creating intermediate objects.
    Set {
        /// RFC 6901 JSON pointer.
        path: String,
        /// New value.
        value: Value,
    },
    /// Remove the object member at `path`.
    Remove {
        /// RFC 6901 JSON pointer.
        path: String,
    },
}

impl ParamChange {
    /// The pointer this change targets.
    pub fn path(&self) -> &str {
        match self {
            Self::Set { path, .. } | Self::Remove { path } => path,
        }
    }
}

/// Descriptive metadata supplied with an edit.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct EditMeta {
    /// Short label shown in the history panel, e.g. `"Exposure +0.50"`.
    pub label: String,
    /// Who made it.
    pub author: Author,
    /// Unix time in milliseconds.
    pub timestamp_ms: i64,
    /// Group (e.g. "Agent base edit"), if any.
    pub group: Option<HistoryGroupId>,
    /// One-line rationale (agents must supply one; spec 10 §2).
    pub rationale: Option<String>,
}

impl EditMeta {
    /// A user edit with the given label.
    pub fn user(label: impl Into<String>, timestamp_ms: i64) -> Self {
        Self {
            label: label.into(),
            timestamp_ms,
            ..Self::default()
        }
    }
}

/// An immutable history entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// Id; equals the entry's 1-based position in [`History::entries`].
    pub id: HistoryEntryId,
    /// Parent entry, or `None` for a child of the base state.
    #[serde(default)]
    pub parent: Option<HistoryEntryId>,
    /// Metadata.
    #[serde(flatten)]
    pub meta: EditMeta,
    /// Changes relative to the parent state.
    #[serde(default)]
    pub changes: Vec<ParamChange>,
}

/// A named pointer to a history state.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Snapshot {
    /// Unique name within the recipe.
    pub name: String,
    /// Entry, or `None` for the base state.
    pub entry: Option<HistoryEntryId>,
    /// Unix time in milliseconds.
    pub created_ms: i64,
}

/// A named group of entries (e.g. one agent run).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct HistoryGroup {
    /// Id.
    pub id: HistoryGroupId,
    /// Display name.
    pub name: String,
}

/// The history tree.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct History {
    /// State before the first entry (usually defaults, or the imported state).
    pub base: DevelopSettings,
    /// All entries ever recorded, in creation order.
    pub entries: Vec<HistoryEntry>,
    /// Entry the current settings correspond to (`None` = base).
    pub head: Option<HistoryEntryId>,
    /// Named snapshots.
    pub snapshots: Vec<Snapshot>,
    /// Named groups.
    pub groups: Vec<HistoryGroup>,
}

impl History {
    /// Looks up an entry.
    pub fn entry(&self, id: HistoryEntryId) -> Option<&HistoryEntry> {
        let idx = usize::try_from(id.0).ok()?.checked_sub(1)?;
        self.entries.get(idx).filter(|e| e.id == id)
    }

    fn require(&self, id: HistoryEntryId) -> EngineResult<&HistoryEntry> {
        self.entry(id)
            .ok_or_else(|| EngineError::not_found("history entry", id))
    }

    /// Entries from the base to `target`, oldest first.
    pub fn lineage(&self, target: Option<HistoryEntryId>) -> EngineResult<Vec<&HistoryEntry>> {
        let mut chain = Vec::new();
        let mut cur = target;
        while let Some(id) = cur {
            let e = self.require(id)?;
            if chain.len() > self.entries.len() {
                return Err(EngineError::internal("history parent cycle"));
            }
            chain.push(e);
            cur = e.parent;
        }
        chain.reverse();
        Ok(chain)
    }

    /// Materializes the settings at `target` by replaying its lineage.
    pub fn state_at(&self, target: Option<HistoryEntryId>) -> EngineResult<DevelopSettings> {
        let mut value = serde_json::to_value(&self.base)?;
        for e in self.lineage(target)? {
            for change in &e.changes {
                apply_change(&mut value, change)?;
            }
        }
        Ok(serde_json::from_value(value)?)
    }

    /// Records the transition `before → after` as a child of `head` and
    /// advances `head`. Returns `None` (recording nothing) if nothing changed.
    pub fn record(
        &mut self,
        before: &DevelopSettings,
        after: &DevelopSettings,
        meta: EditMeta,
    ) -> EngineResult<Option<HistoryEntryId>> {
        let changes = diff(
            &serde_json::to_value(before)?,
            &serde_json::to_value(after)?,
        );
        if changes.is_empty() {
            return Ok(None);
        }
        let id = HistoryEntryId(self.entries.len() as u64 + 1);
        self.entries.push(HistoryEntry {
            id,
            parent: self.head,
            meta,
            changes,
        });
        self.head = Some(id);
        Ok(Some(id))
    }

    /// Parent of `head` (the undo target), if `head` is not the base.
    pub fn undo_target(&self) -> Option<Option<HistoryEntryId>> {
        let head = self.head?;
        Some(self.entry(head).and_then(|e| e.parent))
    }

    /// Newest child of `head` (the redo target), if any.
    pub fn redo_target(&self) -> Option<HistoryEntryId> {
        self.entries
            .iter()
            .rev()
            .find(|e| e.parent == self.head)
            .map(|e| e.id)
    }

    /// Snapshot by name.
    pub fn snapshot(&self, name: &str) -> Option<&Snapshot> {
        self.snapshots.iter().find(|s| s.name == name)
    }

    /// Adds a snapshot of `entry`. Names must be unique.
    pub fn add_snapshot(
        &mut self,
        name: impl Into<String>,
        entry: Option<HistoryEntryId>,
        created_ms: i64,
    ) -> EngineResult<()> {
        let name = name.into();
        if name.is_empty() {
            return Err(EngineError::invalid(
                "name",
                "snapshot name must not be empty",
            ));
        }
        if self.snapshot(&name).is_some() {
            return Err(EngineError::invalid(
                "name",
                format!("snapshot `{name}` already exists"),
            ));
        }
        if let Some(id) = entry {
            self.require(id)?;
        }
        self.snapshots.push(Snapshot {
            name,
            entry,
            created_ms,
        });
        Ok(())
    }

    /// Adds a group, returning its id.
    pub fn add_group(&mut self, name: impl Into<String>) -> HistoryGroupId {
        let id = HistoryGroupId(self.groups.iter().map(|g| g.id.0 + 1).max().unwrap_or(1));
        self.groups.push(HistoryGroup {
            id,
            name: name.into(),
        });
        id
    }

    /// Checks structural invariants (ids sequential, parents precede children).
    pub fn validate(&self) -> EngineResult<()> {
        for (i, e) in self.entries.iter().enumerate() {
            if e.id.0 != i as u64 + 1 {
                return Err(EngineError::internal(format!(
                    "history entry {} at position {}",
                    e.id,
                    i + 1
                )));
            }
            if let Some(p) = e.parent {
                if p.0 >= e.id.0 {
                    return Err(EngineError::internal(format!(
                        "{} has non-preceding parent {p}",
                        e.id
                    )));
                }
            }
        }
        if let Some(h) = self.head {
            self.require(h)?;
        }
        Ok(())
    }
}

/// Minimal patch turning `before` into `after`: objects are diffed member by
/// member; arrays and scalars are replaced whole.
pub fn diff(before: &Value, after: &Value) -> Vec<ParamChange> {
    let mut out = Vec::new();
    diff_into(before, after, &mut String::new(), &mut out);
    out
}

fn diff_into(before: &Value, after: &Value, path: &mut String, out: &mut Vec<ParamChange>) {
    match (before, after) {
        (Value::Object(a), Value::Object(b)) => {
            for (k, va) in a {
                let len = path.len();
                push_token(path, k);
                match b.get(k) {
                    Some(vb) => diff_into(va, vb, path, out),
                    None => out.push(ParamChange::Remove { path: path.clone() }),
                }
                path.truncate(len);
            }
            for (k, vb) in b {
                if !a.contains_key(k) {
                    let len = path.len();
                    push_token(path, k);
                    out.push(ParamChange::Set {
                        path: path.clone(),
                        value: vb.clone(),
                    });
                    path.truncate(len);
                }
            }
        }
        (a, b) if a == b => {}
        (_, b) => out.push(ParamChange::Set {
            path: path.clone(),
            value: b.clone(),
        }),
    }
}

fn push_token(path: &mut String, key: &str) {
    path.push('/');
    path.push_str(&key.replace('~', "~0").replace('/', "~1"));
}

fn split_pointer(path: &str) -> EngineResult<Vec<String>> {
    if path.is_empty() {
        return Ok(Vec::new());
    }
    let rest = path
        .strip_prefix('/')
        .ok_or_else(|| EngineError::invalid("path", format!("`{path}` is not a JSON pointer")))?;
    Ok(rest
        .split('/')
        .map(|t| t.replace("~1", "/").replace("~0", "~"))
        .collect())
}

/// Applies one change to a JSON value.
pub fn apply_change(root: &mut Value, change: &ParamChange) -> EngineResult<()> {
    let tokens = split_pointer(change.path())?;
    let Some((last, parents)) = tokens.split_last() else {
        return match change {
            ParamChange::Set { value, .. } => {
                *root = value.clone();
                Ok(())
            }
            ParamChange::Remove { .. } => {
                Err(EngineError::invalid("path", "cannot remove the root"))
            }
        };
    };
    let mut cur = root;
    for t in parents {
        cur = match cur {
            Value::Object(map) => map
                .entry(t.clone())
                .or_insert_with(|| Value::Object(Map::new())),
            Value::Array(items) => {
                let i: usize = t
                    .parse()
                    .map_err(|_| EngineError::invalid("path", format!("bad index `{t}`")))?;
                items.get_mut(i).ok_or_else(|| {
                    EngineError::invalid("path", format!("index {i} out of range"))
                })?
            }
            _ => {
                return Err(EngineError::invalid(
                    "path",
                    format!("`{}` traverses a scalar", change.path()),
                ))
            }
        };
    }
    match (cur, change) {
        (Value::Object(map), ParamChange::Set { value, .. }) => {
            map.insert(last.clone(), value.clone());
        }
        (Value::Object(map), ParamChange::Remove { .. }) => {
            map.remove(last);
        }
        (Value::Array(items), ParamChange::Set { value, .. }) => {
            let i: usize = last
                .parse()
                .map_err(|_| EngineError::invalid("path", format!("bad index `{last}`")))?;
            let slot = items
                .get_mut(i)
                .ok_or_else(|| EngineError::invalid("path", format!("index {i} out of range")))?;
            *slot = value.clone();
        }
        _ => {
            return Err(EngineError::invalid(
                "path",
                format!("cannot apply {} here", change.path()),
            ))
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn diff_apply_round_trip() {
        let a = json!({"x": 1, "o": {"k": [1, 2], "gone": true, "a/b": 0}});
        let b = json!({"x": 2, "o": {"k": [1, 3], "new": "v", "a/b": 5}});
        let patch = diff(&a, &b);
        assert!(patch
            .iter()
            .any(|c| matches!(c, ParamChange::Remove { path } if path == "/o/gone")));
        assert!(patch.iter().any(|c| c.path() == "/o/a~1b"));
        let mut v = a.clone();
        for c in &patch {
            apply_change(&mut v, c).unwrap();
        }
        assert_eq!(v, b);
        assert!(diff(&b, &b).is_empty());
    }

    #[test]
    fn replay_matches_recorded_states() {
        let mut h = History::default();
        let s0 = DevelopSettings::default();
        let mut s1 = s0.clone();
        s1.tone.exposure = 0.7;
        let mut s2 = s1.clone();
        s2.color.vibrance = 20.0;
        let e1 = h
            .record(&s0, &s1, EditMeta::user("Exposure", 1))
            .unwrap()
            .unwrap();
        let e2 = h
            .record(&s1, &s2, EditMeta::user("Vibrance", 2))
            .unwrap()
            .unwrap();
        assert_eq!(h.state_at(Some(e1)).unwrap(), s1);
        assert_eq!(h.state_at(Some(e2)).unwrap(), s2);
        assert_eq!(h.state_at(None).unwrap(), s0);
        assert_eq!(h.record(&s2, &s2, EditMeta::default()).unwrap(), None);
        h.validate().unwrap();
        assert!(h.state_at(Some(HistoryEntryId(9))).is_err());
    }
}
