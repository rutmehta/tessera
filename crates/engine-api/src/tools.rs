//! The typed engine tool API (spec 10 §2).
//!
//! Every operation an agent, script or MCP client can perform is a
//! [`ToolCall`] variant; every result is a [`ToolOutput`] variant. Both are
//! internally tagged on `"tool"` with snake_case names, so one JSON object is
//! one call and the variant name is the MCP tool name:
//!
//! ```json
//! {"tool": "set_tone", "image": "…", "exposure": 0.5, "shadows": 25}
//! ```
//!
//! Mutating calls become ordinary recipe history entries; the
//! [`ToolRequest`] envelope carries the rationale and history group that the
//! entry records.

use serde::{Deserialize, Serialize};

use crate::color::IccProfileHandle;
use crate::error::EngineError;
use crate::id::{
    HistoryEntryId, HistoryGroupId, ImageId, JobId, MaskId, PersonId, SimilarityGroupId, StyleId,
};
use crate::recipe::mask::{LocalParams, MaskComponent};
use crate::recipe::selection::{Decision, Grade, Mark};
use crate::recipe::settings::NormalizedRect;
use crate::recipe::RecipeHash;

/// Partial update of global tone and colour controls. `None` leaves a
/// control unchanged. Units as in [`crate::recipe::settings`].
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ToneUpdate {
    /// Exposure, EV.
    pub exposure: Option<f32>,
    /// Contrast.
    pub contrast: Option<f32>,
    /// Highlights.
    pub highlights: Option<f32>,
    /// Shadows.
    pub shadows: Option<f32>,
    /// Whites.
    pub whites: Option<f32>,
    /// Blacks.
    pub blacks: Option<f32>,
    /// Texture.
    pub texture: Option<f32>,
    /// Clarity.
    pub clarity: Option<f32>,
    /// Dehaze.
    pub dehaze: Option<f32>,
    /// Vibrance.
    pub vibrance: Option<f32>,
    /// Saturation.
    pub saturation: Option<f32>,
    /// White balance temperature, kelvin (switches WB to custom).
    pub temperature: Option<f32>,
    /// White balance tint (switches WB to custom).
    pub tint: Option<f32>,
}

/// How `remove_object` fills the removed area. Generative fill is
/// deliberately absent: base edits are non-generative (spec 10 §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemovalMethod {
    /// Engine choice between heal and inpaint.
    #[default]
    Auto,
    /// Texture-synthesis heal.
    Heal,
    /// Local inpainting model.
    Inpaint,
}

/// Change to an optional selection field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", content = "value", rename_all = "snake_case")]
pub enum FieldUpdate<T> {
    /// Set the field.
    Set(T),
    /// Clear the field.
    Clear,
}

/// Rendered-image histogram request space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistogramSpace {
    /// Display-referred output of the current recipe.
    #[default]
    Display,
    /// Scene-linear working space before the display transform.
    SceneLinear,
}

/// Image comparison metric.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompareMetric {
    /// Mean/percentile CIEDE2000 on the rendered outputs.
    #[default]
    DeltaE2000,
    /// Parameter-level diff of the two recipes.
    RecipeDiff,
    /// Quality-score comparison (focus, eyes, aesthetics).
    Scores,
}

/// Encoded output format.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "format", rename_all = "snake_case")]
pub enum ExportFormat {
    /// JPEG.
    Jpeg {
        /// Quality `1..=100`.
        quality: u8,
    },
    /// TIFF.
    Tiff {
        /// 8, 16 or 32 bits per channel.
        bit_depth: u8,
    },
    /// PNG.
    Png {
        /// 8 or 16 bits per channel.
        bit_depth: u8,
    },
    /// JPEG XL.
    JpegXl {
        /// Quality `1..=100`; 100 is lossless.
        quality: u8,
    },
    /// AVIF.
    Avif {
        /// Quality `1..=100`.
        quality: u8,
    },
    /// DNG (linear or original raw).
    Dng,
}

/// Output resize.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "fit", rename_all = "snake_case")]
pub enum Resize {
    /// Fit the long edge.
    LongEdge {
        /// Pixels.
        pixels: u32,
    },
    /// Fit within a box.
    Within {
        /// Width.
        width: u32,
        /// Height.
        height: u32,
    },
    /// Target megapixels.
    Megapixels {
        /// Megapixels.
        megapixels: f32,
    },
}

/// Export settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExportSettings {
    /// Destination directory.
    pub destination: String,
    /// File-name template, e.g. `"{name}-{seq:3}"`.
    #[serde(default = "default_template")]
    pub name_template: String,
    /// Format.
    pub format: ExportFormat,
    /// Output colour profile; `None` means sRGB.
    #[serde(default)]
    pub profile: Option<IccProfileHandle>,
    /// Resize, if any.
    #[serde(default)]
    pub resize: Option<Resize>,
    /// Embed XMP metadata (ratings, keywords, IPTC).
    #[serde(default = "yes")]
    pub embed_metadata: bool,
    /// Write HDR (gain map or PQ/HLG, format permitting).
    #[serde(default)]
    pub hdr: bool,
}

fn default_template() -> String {
    "{name}".into()
}

fn yes() -> bool {
    true
}

/// A tool call. Variant names (snake_case) are the MCP tool names.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "tool", rename_all = "snake_case")]
pub enum ToolCall {
    /// Adjust global tone/colour controls.
    SetTone {
        /// Target image.
        image: ImageId,
        /// Controls to change.
        #[serde(flatten)]
        update: ToneUpdate,
    },
    /// Create a local adjustment from mask components.
    CreateMask {
        /// Target image.
        image: ImageId,
        /// Display name.
        #[serde(default)]
        name: Option<String>,
        /// Components, combined in order.
        components: Vec<MaskComponent>,
        /// Initial parameters.
        #[serde(default)]
        params: LocalParams,
    },
    /// Change a local adjustment.
    AdjustMask {
        /// Target image.
        image: ImageId,
        /// Mask.
        mask: MaskId,
        /// Replacement parameters, if changing.
        #[serde(default)]
        params: Option<LocalParams>,
        /// Components to append.
        #[serde(default)]
        add_components: Vec<MaskComponent>,
        /// Amount `0..=200`, if changing.
        #[serde(default)]
        amount: Option<f32>,
        /// Invert, if changing.
        #[serde(default)]
        invert: Option<bool>,
        /// Enable/disable, if changing.
        #[serde(default)]
        enabled: Option<bool>,
    },
    /// Remove the content under a mask by non-generative fill.
    RemoveObject {
        /// Target image.
        image: ImageId,
        /// Mask selecting the object.
        mask: MaskId,
        /// Fill method.
        #[serde(default)]
        method: RemovalMethod,
    },
    /// Skin retouch for one person.
    RetouchSkin {
        /// Target image.
        image: ImageId,
        /// Person.
        person: PersonId,
        /// Strength `0..=100`.
        strength: f32,
    },
    /// Apply a preset, profile, LUT or learned style.
    ApplyStyle {
        /// Target image.
        image: ImageId,
        /// Style.
        style: StyleId,
        /// Amount `0..=200` (%).
        #[serde(default = "hundred")]
        amount: f32,
    },
    /// Set the crop.
    Crop {
        /// Target image.
        image: ImageId,
        /// Rectangle before rotation.
        rect: NormalizedRect,
        /// Straighten angle, degrees.
        #[serde(default)]
        angle: f32,
    },
    /// Compare two images.
    Compare {
        /// First image.
        image_a: ImageId,
        /// Second image.
        image_b: ImageId,
        /// Metric.
        #[serde(default)]
        metric: CompareMetric,
    },
    /// Histogram and clipping statistics of the current render.
    GetHistogram {
        /// Target image.
        image: ImageId,
        /// Space.
        #[serde(default)]
        space: HistogramSpace,
        /// Bin count (default 256).
        #[serde(default = "default_bins")]
        bins: u16,
    },
    /// Quality scores and AI signals.
    GetScores {
        /// Target image.
        image: ImageId,
    },
    /// Index a folder (spec 05 §2.1: open = index, no import).
    IndexFolder {
        /// Absolute folder path.
        path: String,
        /// Recurse into subfolders.
        #[serde(default = "yes")]
        recursive: bool,
    },
    /// Change selection state on one or more images.
    SetSelection {
        /// Images.
        images: Vec<ImageId>,
        /// New decision, if changing.
        #[serde(default)]
        decision: Option<Decision>,
        /// Grade change, if any (setting a grade implies Keep).
        #[serde(default)]
        grade: Option<FieldUpdate<Grade>>,
        /// Mark change, if any.
        #[serde(default)]
        mark: Option<FieldUpdate<Mark>>,
    },
    /// Render and encode images.
    Export {
        /// Images.
        images: Vec<ImageId>,
        /// Settings.
        settings: ExportSettings,
    },
}

fn hundred() -> f32 {
    100.0
}

fn default_bins() -> u16 {
    256
}

impl ToolCall {
    /// Every tool name, in declaration order.
    pub const NAMES: [&'static str; 13] = [
        "set_tone",
        "create_mask",
        "adjust_mask",
        "remove_object",
        "retouch_skin",
        "apply_style",
        "crop",
        "compare",
        "get_histogram",
        "get_scores",
        "index_folder",
        "set_selection",
        "export",
    ];

    /// The tool (MCP) name.
    pub fn name(&self) -> &'static str {
        match self {
            Self::SetTone { .. } => "set_tone",
            Self::CreateMask { .. } => "create_mask",
            Self::AdjustMask { .. } => "adjust_mask",
            Self::RemoveObject { .. } => "remove_object",
            Self::RetouchSkin { .. } => "retouch_skin",
            Self::ApplyStyle { .. } => "apply_style",
            Self::Crop { .. } => "crop",
            Self::Compare { .. } => "compare",
            Self::GetHistogram { .. } => "get_histogram",
            Self::GetScores { .. } => "get_scores",
            Self::IndexFolder { .. } => "index_folder",
            Self::SetSelection { .. } => "set_selection",
            Self::Export { .. } => "export",
        }
    }

    /// True if the call changes a recipe (and therefore records history).
    pub fn edits_recipe(&self) -> bool {
        matches!(
            self,
            Self::SetTone { .. }
                | Self::CreateMask { .. }
                | Self::AdjustMask { .. }
                | Self::RemoveObject { .. }
                | Self::RetouchSkin { .. }
                | Self::ApplyStyle { .. }
                | Self::Crop { .. }
        )
    }

    /// True if the call has no side effects.
    pub fn is_read_only(&self) -> bool {
        matches!(
            self,
            Self::Compare { .. } | Self::GetHistogram { .. } | Self::GetScores { .. }
        )
    }
}

/// Envelope for a call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolRequest {
    /// The call.
    #[serde(flatten)]
    pub call: ToolCall,
    /// One-line rationale recorded on the history entry.
    #[serde(default)]
    pub rationale: Option<String>,
    /// History group to record into (e.g. "Agent base edit").
    #[serde(default)]
    pub group: Option<HistoryGroupId>,
    /// Optimistic concurrency: fail with [`EngineError::Conflict`] if the
    /// image's recipe hash no longer matches.
    #[serde(default)]
    pub expect_recipe: Option<RecipeHash>,
}

/// Per-channel histogram.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Histogram {
    /// Red counts.
    pub red: Vec<u32>,
    /// Green counts.
    pub green: Vec<u32>,
    /// Blue counts.
    pub blue: Vec<u32>,
    /// Luminance counts.
    pub luminance: Vec<u32>,
    /// Fraction of pixels clipped to black in any channel.
    pub clipped_shadows: f32,
    /// Fraction of pixels clipped to white in any channel.
    pub clipped_highlights: f32,
}

/// Per-face signals.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FaceScore {
    /// Person, if identified.
    pub person: Option<PersonId>,
    /// Face box.
    pub region: NormalizedRect,
    /// Focus `0..=1`.
    pub focus: f32,
    /// Probability eyes are open `0..=1`.
    pub eyes_open: f32,
}

/// AI quality signals (spec 06 §2). Read-only; never changes a decision.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Scores {
    /// Global focus `0..=1`.
    pub focus: f32,
    /// Faces.
    pub faces: Vec<FaceScore>,
    /// Motion blur `0..=1` (higher = blurrier).
    pub motion_blur: f32,
    /// Exposure quality `0..=1`.
    pub exposure: f32,
    /// Noise level `0..=1`.
    pub noise: f32,
    /// Aesthetic score `0..=1`.
    pub aesthetic: f32,
    /// Burst / near-duplicate group.
    pub group: Option<SimilarityGroupId>,
    /// Suggested best frame of its group.
    pub best_of_group: bool,
}

/// Result of a call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "tool", rename_all = "snake_case")]
pub enum ToolOutput {
    /// A recipe edit was applied (or was a no-op, `entry: null`).
    Edited {
        /// Image.
        image: ImageId,
        /// History entry recorded.
        entry: Option<HistoryEntryId>,
        /// New recipe hash.
        recipe: RecipeHash,
    },
    /// A mask was created.
    MaskCreated {
        /// Image.
        image: ImageId,
        /// New mask.
        mask: MaskId,
        /// History entry recorded.
        entry: HistoryEntryId,
        /// Fraction of the image covered `0..=1`.
        coverage: f32,
        /// New recipe hash.
        recipe: RecipeHash,
    },
    /// Comparison result.
    Comparison {
        /// Metric used.
        metric: CompareMetric,
        /// Scalar distance (ΔE mean, changed-parameter count, or score delta).
        distance: f32,
        /// Human-readable summary.
        summary: String,
    },
    /// Histogram.
    Histogram {
        /// Image.
        image: ImageId,
        /// Data.
        histogram: Histogram,
    },
    /// Scores.
    Scores {
        /// Image.
        image: ImageId,
        /// Data.
        scores: Scores,
    },
    /// Folder indexing started.
    Indexing {
        /// Background job.
        job: JobId,
        /// Files found by the initial scan.
        files_found: u64,
    },
    /// Selection updated.
    SelectionUpdated {
        /// Number of images changed.
        changed: u32,
    },
    /// Export queued.
    ExportQueued {
        /// Background job.
        job: JobId,
        /// Number of images.
        images: u32,
    },
}

/// Response to a [`ToolRequest`]: `{"ok": …}` or `{"error": …}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolResponse {
    /// Success.
    Ok(ToolOutput),
    /// Failure.
    Error(EngineError),
}

impl From<Result<ToolOutput, EngineError>> for ToolResponse {
    fn from(r: Result<ToolOutput, EngineError>) -> Self {
        match r {
            Ok(o) => Self::Ok(o),
            Err(e) => Self::Error(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe::mask::MaskKind;
    use serde_json::json;

    fn img() -> ImageId {
        ImageId(0xabc)
    }

    fn all_calls() -> Vec<ToolCall> {
        vec![
            ToolCall::SetTone {
                image: img(),
                update: ToneUpdate {
                    exposure: Some(0.5),
                    ..Default::default()
                },
            },
            ToolCall::CreateMask {
                image: img(),
                name: Some("Sky".into()),
                components: vec![MaskComponent::new(MaskKind::Sky { model: None })],
                params: LocalParams {
                    exposure: -0.5,
                    ..Default::default()
                },
            },
            ToolCall::AdjustMask {
                image: img(),
                mask: MaskId(0),
                params: None,
                add_components: vec![],
                amount: Some(80.0),
                invert: None,
                enabled: None,
            },
            ToolCall::RemoveObject {
                image: img(),
                mask: MaskId(1),
                method: RemovalMethod::Inpaint,
            },
            ToolCall::RetouchSkin {
                image: img(),
                person: PersonId(4),
                strength: 30.0,
            },
            ToolCall::ApplyStyle {
                image: img(),
                style: StyleId::new("user/wedding-warm"),
                amount: 100.0,
            },
            ToolCall::Crop {
                image: img(),
                rect: NormalizedRect {
                    left: 0.1,
                    top: 0.0,
                    right: 0.9,
                    bottom: 1.0,
                },
                angle: 1.5,
            },
            ToolCall::Compare {
                image_a: img(),
                image_b: ImageId(1),
                metric: CompareMetric::DeltaE2000,
            },
            ToolCall::GetHistogram {
                image: img(),
                space: HistogramSpace::Display,
                bins: 256,
            },
            ToolCall::GetScores { image: img() },
            ToolCall::IndexFolder {
                path: "/photos/2026".into(),
                recursive: true,
            },
            ToolCall::SetSelection {
                images: vec![img()],
                decision: None,
                grade: Some(FieldUpdate::Set(Grade::Three)),
                mark: Some(FieldUpdate::Clear),
            },
            ToolCall::Export {
                images: vec![img()],
                settings: ExportSettings {
                    destination: "/out".into(),
                    name_template: "{name}".into(),
                    format: ExportFormat::Jpeg { quality: 90 },
                    profile: None,
                    resize: Some(Resize::LongEdge { pixels: 2048 }),
                    embed_metadata: true,
                    hdr: false,
                },
            },
        ]
    }

    #[test]
    fn every_call_round_trips_and_is_tagged_by_name() {
        let calls = all_calls();
        assert_eq!(calls.len(), ToolCall::NAMES.len());
        for (call, name) in calls.iter().zip(ToolCall::NAMES) {
            assert_eq!(call.name(), name);
            let v = serde_json::to_value(call).unwrap();
            assert_eq!(v["tool"], name);
            assert_eq!(serde_json::from_value::<ToolCall>(v).unwrap(), *call);
        }
    }

    #[test]
    fn minimal_json_uses_defaults() {
        let call: ToolCall = serde_json::from_value(json!({
            "tool": "set_tone",
            "image": "00000000000000000000000000000abc",
            "exposure": 0.25,
            "shadows": 20
        }))
        .unwrap();
        let ToolCall::SetTone { update, .. } = &call else {
            panic!()
        };
        assert_eq!(update.exposure, Some(0.25));
        assert_eq!(update.shadows, Some(20.0));
        assert_eq!(update.contrast, None);
        assert!(call.edits_recipe());

        let h: ToolCall = serde_json::from_value(
            json!({"tool": "get_histogram", "image": "00000000000000000000000000000abc"}),
        )
        .unwrap();
        assert_eq!(
            h,
            ToolCall::GetHistogram {
                image: img(),
                space: HistogramSpace::Display,
                bins: 256
            }
        );
        assert!(h.is_read_only());
    }

    #[test]
    fn request_and_response_envelopes() {
        let req = ToolRequest {
            call: ToolCall::GetScores { image: img() },
            rationale: Some("check focus".into()),
            group: Some(HistoryGroupId(1)),
            expect_recipe: None,
        };
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["tool"], "get_scores");
        assert_eq!(v["rationale"], "check focus");
        assert_eq!(serde_json::from_value::<ToolRequest>(v).unwrap(), req);

        let ok: ToolResponse = Ok(ToolOutput::SelectionUpdated { changed: 3 }).into();
        let v = serde_json::to_value(&ok).unwrap();
        assert_eq!(v["ok"]["tool"], "selection_updated");
        assert_eq!(serde_json::from_value::<ToolResponse>(v).unwrap(), ok);

        let err: ToolResponse = Err(EngineError::not_found("mask", MaskId(9))).into();
        let v = serde_json::to_value(&err).unwrap();
        assert_eq!(v["error"]["code"], "not_found");
        assert_eq!(serde_json::from_value::<ToolResponse>(v).unwrap(), err);
    }
}
