//! The per-image edit document (`.edits/<image>.json`, spec 05 §2).
//!
//! A [`Recipe`] holds the current [`DevelopSettings`] (ordered by pipeline
//! stage), the [`Selection`], and the append-only [`History`] that produced
//! the settings. [`Recipe::recipe_hash`] is a deterministic digest of the
//! render-affecting state only and is the render/preview cache key.
//!
//! Forward compatibility: every struct is `#[serde(default)]`, so older
//! documents load into newer builds; unknown top-level members are preserved
//! verbatim in [`Recipe::unknown`]. A build refuses to *write* a document
//! whose `schema_version` is newer than [`RECIPE_SCHEMA_VERSION`], so it can
//! never silently drop fields it does not understand.

pub mod crs;
pub mod history;
pub mod mask;
pub mod selection;
pub mod settings;

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use crs::{CrsKey, CrsValueType};
pub use history::{Author, EditMeta, History, HistoryEntry, ParamChange, Snapshot};
pub use mask::{LocalAdjustment, LocalParams, MaskComponent, MaskKind, RetouchOperation};
pub use selection::{Decision, Grade, Mark, Selection};
pub use settings::DevelopSettings;

use crate::error::{EngineError, EngineResult};
use crate::id::{Digest, HistoryEntryId, ImageId, MaskId, RetouchId};
use crate::stage::{canonical_json, ParamHash, StageId};

/// Schema version this build reads and writes.
pub const RECIPE_SCHEMA_VERSION: u32 = 1;

/// Which pipeline math a recipe is rendered with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessFamily {
    /// The engine's own pipeline.
    Native,
    /// Adobe Process Version compatibility (imported edits).
    Adobe,
}

/// Versioned rendering semantics. Changing how any operator renders a given
/// parameter set requires a new native revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct ProcessVersion {
    /// Family.
    pub family: ProcessFamily,
    /// Revision within the family (Adobe: PV 1–6).
    pub revision: u32,
}

impl Default for ProcessVersion {
    fn default() -> Self {
        Self::NATIVE_CURRENT
    }
}

impl ProcessVersion {
    /// Current native process.
    pub const NATIVE_CURRENT: Self = Self {
        family: ProcessFamily::Native,
        revision: 1,
    };

    /// Adobe process version `pv` (1–6).
    pub const fn adobe(pv: u32) -> Self {
        Self {
            family: ProcessFamily::Adobe,
            revision: pv,
        }
    }

    /// Parses `crs:ProcessVersion` (e.g. `"15.4"` → Adobe PV6).
    pub fn from_crs(value: &str) -> EngineResult<Self> {
        let pv = match value.trim() {
            "5.0" => 1,
            "5.7" => 2,
            "6.7" => 3,
            "10.0" => 4,
            "11.0" => 5,
            "15.4" => 6,
            other => {
                return Err(EngineError::Unsupported {
                    what: format!("crs:ProcessVersion {other}"),
                })
            }
        };
        Ok(Self::adobe(pv))
    }

    /// `crs:ProcessVersion` value for Adobe processes.
    pub fn crs_value(self) -> Option<&'static str> {
        if self.family != ProcessFamily::Adobe {
            return None;
        }
        Some(match self.revision {
            1 => "5.0",
            2 => "5.7",
            3 => "6.7",
            4 => "10.0",
            5 => "11.0",
            6 => "15.4",
            _ => return None,
        })
    }

    /// Seed of the stage hash chain; distinct processes never share cache entries.
    pub fn chain_seed(self) -> ParamHash {
        ParamHash(Digest::derive(
            "engine-api 2026 process-version v1",
            &canonical_json(&self),
        ))
    }
}

/// Digest of a recipe's render-affecting state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RecipeHash(pub Digest);

impl fmt::Display for RecipeHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// Monotonic id counters; ids are never reused, even after undo.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct IdCounters {
    /// Next [`MaskId`].
    pub next_mask: u32,
    /// Next [`RetouchId`].
    pub next_retouch: u32,
}

/// The per-image edit document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Recipe {
    /// Schema version of this document.
    pub schema_version: u32,
    /// Image this recipe belongs to.
    pub image_id: Option<ImageId>,
    /// Rendering semantics.
    pub process_version: ProcessVersion,
    /// Current settings. Invariant: equals `history.state_at(history.head)`.
    pub settings: DevelopSettings,
    /// Culling state (not render-affecting).
    pub selection: Selection,
    /// Edit history.
    pub history: History,
    /// Id allocation.
    pub ids: IdCounters,
    /// Unknown top-level members from newer writers, preserved on round trip.
    #[serde(flatten)]
    pub unknown: BTreeMap<String, Value>,
}

impl Default for Recipe {
    fn default() -> Self {
        Self {
            schema_version: RECIPE_SCHEMA_VERSION,
            image_id: None,
            process_version: ProcessVersion::default(),
            settings: DevelopSettings::default(),
            selection: Selection::default(),
            history: History::default(),
            ids: IdCounters::default(),
            unknown: BTreeMap::new(),
        }
    }
}

#[derive(Serialize)]
struct HashedState<'a> {
    process_version: &'a ProcessVersion,
    settings: &'a DevelopSettings,
}

impl Recipe {
    /// Fresh recipe for an image, with default settings.
    pub fn new(image_id: ImageId) -> Self {
        Self {
            image_id: Some(image_id),
            ..Self::default()
        }
    }

    /// Deterministic digest of the render-affecting state (process version +
    /// settings). Excludes selection, history, ids and unknown members, and is
    /// independent of field declaration order.
    pub fn recipe_hash(&self) -> RecipeHash {
        let state = HashedState {
            process_version: &self.process_version,
            settings: &self.settings,
        };
        RecipeHash(Digest::derive(
            "engine-api 2026 recipe v1",
            &canonical_json(&state),
        ))
    }

    /// Cumulative per-stage hashes for [`crate::stage::MemoKey`]s.
    pub fn stage_chain(&self) -> [(StageId, ParamHash); StageId::COUNT] {
        self.settings.stage_chain(self.process_version.chain_seed())
    }

    /// Applies an edit through `f`, recording it in history. Returns the new
    /// entry, or `None` if `f` changed nothing.
    pub fn edit(
        &mut self,
        meta: EditMeta,
        f: impl FnOnce(&mut DevelopSettings),
    ) -> EngineResult<Option<HistoryEntryId>> {
        let mut next = self.settings.clone();
        f(&mut next);
        let id = self.history.record(&self.settings, &next, meta)?;
        if id.is_some() {
            self.settings = next;
        }
        Ok(id)
    }

    /// Moves to `entry` (`None` = base) and rematerializes the settings.
    pub fn checkout(&mut self, entry: Option<HistoryEntryId>) -> EngineResult<()> {
        self.settings = self.history.state_at(entry)?;
        self.history.head = entry;
        Ok(())
    }

    /// Steps back one entry. Returns `false` at the base.
    pub fn undo(&mut self) -> EngineResult<bool> {
        match self.history.undo_target() {
            Some(target) => self.checkout(target).map(|()| true),
            None => Ok(false),
        }
    }

    /// Steps forward to the newest child of the head. Returns `false` if none.
    pub fn redo(&mut self) -> EngineResult<bool> {
        match self.history.redo_target() {
            Some(target) => self.checkout(Some(target)).map(|()| true),
            None => Ok(false),
        }
    }

    /// Snapshots the current head under `name`.
    pub fn create_snapshot(&mut self, name: impl Into<String>, now_ms: i64) -> EngineResult<()> {
        let head = self.history.head;
        self.history.add_snapshot(name, head, now_ms)
    }

    /// Moves to the state recorded by snapshot `name`.
    pub fn restore_snapshot(&mut self, name: &str) -> EngineResult<()> {
        let entry = self
            .history
            .snapshot(name)
            .ok_or_else(|| EngineError::not_found("snapshot", name))?
            .entry;
        self.checkout(entry)
    }

    /// Allocates a fresh mask id.
    pub fn allocate_mask_id(&mut self) -> MaskId {
        let floor = self
            .settings
            .locals
            .adjustments
            .iter()
            .map(|a| a.id.0 + 1)
            .max()
            .unwrap_or(0);
        let id = self.ids.next_mask.max(floor);
        self.ids.next_mask = id + 1;
        MaskId(id)
    }

    /// Allocates a fresh retouch id.
    pub fn allocate_retouch_id(&mut self) -> RetouchId {
        let floor = self
            .settings
            .locals
            .retouch
            .iter()
            .map(|r| r.id.0 + 1)
            .max()
            .unwrap_or(0);
        let id = self.ids.next_retouch.max(floor);
        self.ids.next_retouch = id + 1;
        RetouchId(id)
    }

    /// Checks every invariant: history structure, settings = replay(head),
    /// selection normalized.
    pub fn validate(&self) -> EngineResult<()> {
        self.history.validate()?;
        if self.history.state_at(self.history.head)? != self.settings {
            return Err(EngineError::Conflict {
                message: "settings do not match history head".into(),
            });
        }
        if self.selection.clone().normalized() != self.selection {
            return Err(EngineError::invalid(
                "selection",
                "grade set on a non-kept image",
            ));
        }
        Ok(())
    }

    /// Parses a recipe document (any schema version; newer ones load best-effort).
    pub fn from_json(bytes: &[u8]) -> EngineResult<Self> {
        let mut recipe: Recipe =
            serde_json::from_slice(bytes).map_err(|e| EngineError::Decode {
                format: "recipe-json".into(),
                message: e.to_string(),
            })?;
        recipe.selection = recipe.selection.normalized();
        Ok(recipe)
    }

    /// Serializes the document (pretty, stable member order). Fails with
    /// [`EngineError::SchemaVersion`] for documents from a newer schema.
    pub fn to_json(&self) -> EngineResult<Vec<u8>> {
        if self.schema_version > RECIPE_SCHEMA_VERSION {
            return Err(EngineError::SchemaVersion {
                document: "recipe".into(),
                found: self.schema_version,
                supported: RECIPE_SCHEMA_VERSION,
            });
        }
        Ok(serde_json::to_vec_pretty(self)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe::mask::{LocalAdjustment, MaskComponent, MaskKind};
    use crate::recipe::selection::Grade;

    fn sample() -> Recipe {
        let mut r = Recipe::new(ImageId(42));
        r.edit(EditMeta::user("Exposure", 1), |s| s.tone.exposure = 0.5)
            .unwrap();
        let mask = r.allocate_mask_id();
        r.edit(EditMeta::user("Sky mask", 2), |s| {
            s.locals.adjustments.push(LocalAdjustment {
                id: mask,
                name: "Sky".into(),
                components: vec![MaskComponent::new(MaskKind::Sky { model: None })],
                ..LocalAdjustment::default()
            });
            s.locals.adjustments[0].params.exposure = -0.3;
        })
        .unwrap();
        r.create_snapshot("first pass", 3).unwrap();
        r.selection.set_grade(Some(Grade::Two));
        r
    }

    #[test]
    fn json_round_trip() {
        let r = sample();
        let bytes = r.to_json().unwrap();
        let back = Recipe::from_json(&bytes).unwrap();
        assert_eq!(back, r);
        back.validate().unwrap();
        assert_eq!(back.recipe_hash(), r.recipe_hash());
    }

    #[test]
    fn empty_and_partial_documents_load() {
        assert_eq!(Recipe::from_json(b"{}").unwrap(), Recipe::default());
        let r = Recipe::from_json(
            br#"{"settings":{"tone":{"exposure":1.0,"future_knob":3}},"future_section":{"a":1}}"#,
        )
        .unwrap();
        assert_eq!(r.settings.tone.exposure, 1.0);
        assert_eq!(r.unknown["future_section"]["a"], 1);
        let again = Recipe::from_json(&r.to_json().unwrap()).unwrap();
        assert_eq!(again.unknown, r.unknown);
    }

    #[test]
    fn newer_schema_is_read_only() {
        let r = Recipe::from_json(br#"{"schema_version":99}"#).unwrap();
        assert!(matches!(
            r.to_json(),
            Err(EngineError::SchemaVersion { found: 99, .. })
        ));
    }

    #[test]
    fn hash_covers_render_state_only() {
        let r = sample();
        let h = r.recipe_hash();
        let mut other = r.clone();
        other.selection.set_decision(Decision::Reject);
        other.history.snapshots.clear();
        other.unknown.insert("x".into(), Value::Bool(true));
        assert_eq!(other.recipe_hash(), h);
        other.settings.tone.exposure = 0.51;
        assert_ne!(other.recipe_hash(), h);
        let mut adobe = r.clone();
        adobe.process_version = ProcessVersion::adobe(6);
        assert_ne!(adobe.recipe_hash(), h);
        let mut neg = Recipe::default();
        neg.settings.tone.exposure = -0.0;
        assert_eq!(neg.recipe_hash(), Recipe::default().recipe_hash());
    }

    #[test]
    fn hash_is_stable_across_releases() {
        // Golden value: if this changes, every render cache is invalidated.
        // Update deliberately, together with a note in CONTRACTS.md.
        let h = Recipe::default().recipe_hash().to_string();
        assert_eq!(h.len(), 64);
        assert_eq!(
            h,
            "f0c302becbf2879b9c41f1bef7efd94eb4a45e925c37cd4989bee4da8d370fcd"
        );
    }

    #[test]
    fn undo_redo_branch_and_snapshots() {
        let mut r = sample();
        let tip = r.history.head;
        assert!(r.undo().unwrap());
        assert_eq!(r.settings.locals.adjustments.len(), 0);
        assert_eq!(r.settings.tone.exposure, 0.5);
        assert!(r.redo().unwrap());
        assert_eq!(r.history.head, tip);
        assert!(r.undo().unwrap());
        assert!(r.undo().unwrap());
        assert!(!r.undo().unwrap());
        assert_eq!(r.settings, DevelopSettings::default());

        // New edit at the base forks a branch; the old tip is still reachable.
        r.edit(EditMeta::user("Contrast", 4), |s| s.tone.contrast = 10.0)
            .unwrap();
        assert_eq!(r.history.entries.len(), 3);
        assert_eq!(r.settings.tone.exposure, 0.0);
        r.restore_snapshot("first pass").unwrap();
        assert_eq!(r.history.head, tip);
        assert_eq!(r.settings.tone.exposure, 0.5);
        assert_eq!(r.settings.tone.contrast, 0.0);
        r.validate().unwrap();
        assert!(r.restore_snapshot("nope").is_err());
        assert!(r.create_snapshot("first pass", 5).is_err());
    }

    #[test]
    fn ids_are_never_reused() {
        let mut r = sample();
        r.undo().unwrap();
        let m = r.allocate_mask_id();
        assert_eq!(m, MaskId(1));
        assert_eq!(r.allocate_retouch_id(), RetouchId(0));
        assert_eq!(r.allocate_retouch_id(), RetouchId(1));
    }

    #[test]
    fn stage_chain_seeded_by_process() {
        let a = Recipe::default();
        let b = Recipe {
            process_version: ProcessVersion::adobe(6),
            ..Recipe::default()
        };
        assert_ne!(a.stage_chain()[0], b.stage_chain()[0]);
        assert_eq!(
            ProcessVersion::from_crs("15.4").unwrap(),
            ProcessVersion::adobe(6)
        );
        assert_eq!(ProcessVersion::adobe(3).crs_value(), Some("6.7"));
        assert!(ProcessVersion::from_crs("99").is_err());
    }
}
