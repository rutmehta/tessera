//! Library-scoped people identities and host-driven clustering contracts.
//! These types do not run models, schedule jobs, or write sidecars.

use serde::{Deserialize, Serialize};

use crate::id::{ImageId, PersonId};
use crate::tile::Extent;

/// Composite face identity. Detector ordinals are local to an image; face
/// re-detection invalidates old references rather than silently reusing them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FaceRef {
    /// Stable image identity.
    pub image_id: ImageId,
    /// Zero-based detector ordinal within that image's current detections.
    pub ordinal: u32,
}

/// Stable library identity, not a queue-local cluster label. Empty identities
/// may persist after faces move elsewhere.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PersonSummary {
    /// Catalog-stable person ID (the existing numeric PersonId contract).
    pub id: PersonId,
    /// Current user-provided name, if any.
    pub name: Option<String>,
    /// Number of explicitly confirmed face assignments.
    pub confirmed_count: u64,
    /// Membership includes approximate clustering, not an exact hierarchy.
    pub approximate: bool,
}

/// MWG-compatible normalized centre/size, NOT edge coordinates. Coordinates
/// refer to the oriented analysis preview described by `coordinate_space`.
/// Hosts must require finite normalized geometry with positive dimensions.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FaceRegion {
    /// Normalized horizontal centre.
    pub center_x: f64,
    /// Normalized vertical centre.
    pub center_y: f64,
    /// Normalized width.
    pub width: f64,
    /// Normalized height.
    pub height: f64,
    /// Pixel dimensions of the coordinate space (MWG AppliedToDimensions).
    pub coordinate_space: Extent,
}

/// Read-only identity suggestion. Does not assign, confirm, or name a face.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NameSuggestion {
    /// Suggested stable identity, including unnamed identities.
    pub person_id: PersonId,
    /// Current name, absent for an unnamed identity.
    #[serde(default)]
    pub name: Option<String>,
    /// Cosine similarity in [-1, 1], NOT a probability or calibrated confidence.
    pub similarity: f32,
}

/// Conservative eligibility thresholds for training/automatic medoids.
/// Rejected or descriptor-less faces remain manually assignable.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct QualityGate {
    /// Minimum detector confidence, finite [0, 1].
    pub min_confidence: f32,
    /// Minimum BOTH face sides, in oriented preview pixels, finite >= 0.
    pub min_size: f32,
    /// Minimum sharpness, finite >= 0.
    pub min_sharpness: f64,
}

impl Default for QualityGate {
    fn default() -> Self {
        Self {
            min_confidence: 0.9,
            min_size: 32.0,
            min_sharpness: 0.1,
        }
    }
}

/// Host-driven incremental clustering configuration. Hosts validate before
/// starting work; automatic jobs protect named identities and confirmed faces.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClusterOptions {
    /// Minimum cosine similarity for assignment, finite [-1, 1].
    pub cosine_threshold: f32,
    /// Training eligibility gate.
    pub quality_gate: QualityGate,
    /// Refit after this many newly observed faces; must be positive.
    pub refit_interval: u64,
}

impl Default for ClusterOptions {
    fn default() -> Self {
        Self {
            cosine_threshold: 0.363,
            quality_gate: QualityGate::default(),
            refit_interval: 1000,
        }
    }
}

/// Outcome of one host-driven incremental/refit invocation. No promise of
/// equivalence to a future batch fit or of a full exact library hierarchy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PeopleJobResult {
    /// Number of face assignments written by the job.
    pub assigned: u64,
    /// Whether this invocation refitted clusters.
    pub refit: bool,
    /// Whether this invocation used approximate clustering.
    pub approximate: bool,
}

/// Explicit write opt-ins. Defaults perform catalog edits only. Sidecar/XML
/// and catalog transactions are not crash-atomic together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PeopleWriteOptions {
    /// Write normalized MWG face regions/names to sidecars.
    pub write_sidecars: bool,
    /// Add person names as keywords; never removes old keywords on rename.
    /// Sidecar keyword writes additionally require `write_sidecars`.
    pub person_keywords: bool,
}
