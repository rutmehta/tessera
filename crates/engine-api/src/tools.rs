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
//!
//! Layered documents (spec 02) have their own call enum,
//! [`DocumentToolCall`], with the same conventions: tagged on `"tool"`,
//! snake_case names that are MCP tool names and are unique across both
//! enums (see [`crate::action::COMMANDS`]), and one
//! [`crate::document::DocumentHistoryEntry`] per mutating call.

use serde::{Deserialize, Serialize};

use crate::color::IccProfileHandle;
use crate::document::{
    AdjustmentSpec, AffineTransform, BrushParams, DocumentExportSettings, DocumentSummary,
    Interpolation, LayerInfo, LayerPropsUpdate, NewLayer, SelectionMode, SelectionShape,
    StrokePoint, StrokeTarget,
};
use crate::error::EngineError;
use crate::id::{
    DocumentId, HistoryEntryId, HistoryGroupId, ImageId, JobId, LayerId, MaskId, PersonId,
    SelectionId, SimilarityGroupId, StyleId,
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

/// A layered-document tool call. Variant names (snake_case) are the MCP tool
/// names; none collides with a [`ToolCall`] name. Layers are addressed by
/// [`LayerId`] within a [`DocumentId`]; strokes and selections are geometry
/// and parameters, never pixels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "tool", rename_all = "snake_case")]
pub enum DocumentToolCall {
    /// Open a PSD/PSB, `.tessera-doc` or image file as a layered document.
    OpenDocument {
        /// Absolute file path.
        path: String,
    },
    /// Add a layer.
    AddLayer {
        /// Document.
        document: DocumentId,
        /// Kind of layer.
        layer: NewLayer,
        /// Display name (engine default if absent).
        #[serde(default)]
        name: Option<String>,
        /// Enclosing group; `None` = the root.
        #[serde(default)]
        parent: Option<LayerId>,
        /// Sibling to insert directly above; `None` = top of `parent`.
        #[serde(default)]
        above: Option<LayerId>,
        /// Initial properties.
        #[serde(default)]
        props: LayerPropsUpdate,
    },
    /// Change a layer's common properties.
    SetLayerProps {
        /// Document.
        document: DocumentId,
        /// Layer.
        layer: LayerId,
        /// Properties to change.
        #[serde(flatten)]
        props: LayerPropsUpdate,
    },
    /// Paint one brush stroke on a layer (or its mask), limited by the
    /// current selection.
    PaintStroke {
        /// Document.
        document: DocumentId,
        /// Layer.
        layer: LayerId,
        /// Stroke path, in order; at least one point.
        points: Vec<StrokePoint>,
        /// Brush.
        #[serde(default)]
        brush: BrushParams,
        /// Pixels or mask.
        #[serde(default)]
        target: StrokeTarget,
    },
    /// Change the document's pixel selection.
    SetPixelSelection {
        /// Document.
        document: DocumentId,
        /// Region.
        #[serde(flatten)]
        shape: SelectionShape,
        /// Combination with the current selection.
        #[serde(default)]
        mode: SelectionMode,
        /// Feather radius, level-0 pixels.
        #[serde(default)]
        feather: f32,
        /// Also save the result as a named selection.
        #[serde(default)]
        save_as: Option<String>,
    },
    /// Add an adjustment layer (non-destructive).
    ApplyAdjustmentLayer {
        /// Document.
        document: DocumentId,
        /// Adjustment.
        adjustment: AdjustmentSpec,
        /// Display name (engine default if absent).
        #[serde(default)]
        name: Option<String>,
        /// Enclosing group; `None` = the root.
        #[serde(default)]
        parent: Option<LayerId>,
        /// Sibling to insert directly above; `None` = top of `parent`.
        #[serde(default)]
        above: Option<LayerId>,
        /// Clip to the layer below.
        #[serde(default)]
        clipped: bool,
        /// Use the current selection as the layer mask.
        #[serde(default = "yes")]
        mask_from_selection: bool,
    },
    /// Transform a layer's pixels (or a smart object's placement).
    TransformLayer {
        /// Document.
        document: DocumentId,
        /// Layer.
        layer: LayerId,
        /// Map from current to new level-0 canvas coordinates.
        transform: AffineTransform,
        /// Resampling filter (ignored by smart objects, which resample on render).
        #[serde(default)]
        interpolation: Interpolation,
    },
    /// Merge a layer into the one below it.
    MergeDown {
        /// Document.
        document: DocumentId,
        /// Upper layer.
        layer: LayerId,
    },
    /// Write the document to a file.
    ExportDocument {
        /// Document.
        document: DocumentId,
        /// Settings.
        settings: DocumentExportSettings,
    },
    /// Describe the layer tree.
    ListLayers {
        /// Document.
        document: DocumentId,
    },
}

impl DocumentToolCall {
    /// Every tool name, in declaration order.
    pub const NAMES: [&'static str; 10] = [
        "open_document",
        "add_layer",
        "set_layer_props",
        "paint_stroke",
        "set_pixel_selection",
        "apply_adjustment_layer",
        "transform_layer",
        "merge_down",
        "export_document",
        "list_layers",
    ];

    /// The tool (MCP) name.
    pub fn name(&self) -> &'static str {
        match self {
            Self::OpenDocument { .. } => "open_document",
            Self::AddLayer { .. } => "add_layer",
            Self::SetLayerProps { .. } => "set_layer_props",
            Self::PaintStroke { .. } => "paint_stroke",
            Self::SetPixelSelection { .. } => "set_pixel_selection",
            Self::ApplyAdjustmentLayer { .. } => "apply_adjustment_layer",
            Self::TransformLayer { .. } => "transform_layer",
            Self::MergeDown { .. } => "merge_down",
            Self::ExportDocument { .. } => "export_document",
            Self::ListLayers { .. } => "list_layers",
        }
    }

    /// The target document (`None` for `open_document`).
    pub fn document(&self) -> Option<DocumentId> {
        match self {
            Self::OpenDocument { .. } => None,
            Self::AddLayer { document, .. }
            | Self::SetLayerProps { document, .. }
            | Self::PaintStroke { document, .. }
            | Self::SetPixelSelection { document, .. }
            | Self::ApplyAdjustmentLayer { document, .. }
            | Self::TransformLayer { document, .. }
            | Self::MergeDown { document, .. }
            | Self::ExportDocument { document, .. }
            | Self::ListLayers { document } => Some(*document),
        }
    }

    /// True if the call changes a document (and therefore records exactly
    /// one document history entry).
    pub fn edits_document(&self) -> bool {
        !matches!(
            self,
            Self::OpenDocument { .. } | Self::ExportDocument { .. } | Self::ListLayers { .. }
        )
    }

    /// True if the call has no side effects.
    pub fn is_read_only(&self) -> bool {
        matches!(self, Self::ListLayers { .. })
    }
}

/// Envelope for a [`DocumentToolCall`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentToolRequest {
    /// The call.
    #[serde(flatten)]
    pub call: DocumentToolCall,
    /// One-line rationale recorded on the history entry.
    #[serde(default)]
    pub rationale: Option<String>,
    /// History group to record into.
    #[serde(default)]
    pub group: Option<HistoryGroupId>,
    /// Optimistic concurrency: fail with [`EngineError::Conflict`] unless the
    /// document's history head is this entry. Absent = no check; `null` =
    /// expect the state as opened.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub expect_head: Option<Option<HistoryEntryId>>,
}

fn present<'de, D, T>(d: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(d).map(Some)
}

/// Result of a [`DocumentToolCall`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "tool", rename_all = "snake_case")]
pub enum DocumentToolOutput {
    /// A document was opened.
    DocumentOpened {
        /// New session handle.
        document: DocumentId,
        /// Canvas, depth, layer count.
        #[serde(flatten)]
        summary: DocumentSummary,
    },
    /// A document edit was applied (or was a no-op, `entry: null`).
    DocumentEdited {
        /// Document.
        document: DocumentId,
        /// History entry recorded.
        entry: Option<HistoryEntryId>,
        /// Layer created or changed, if one.
        #[serde(default)]
        layer: Option<LayerId>,
        /// Selection saved by `save_as`, if any.
        #[serde(default)]
        selection: Option<SelectionId>,
    },
    /// The layer tree, bottom to top, every group before its children.
    LayerList {
        /// Document.
        document: DocumentId,
        /// Rows.
        layers: Vec<LayerInfo>,
    },
    /// Export queued.
    DocumentExportQueued {
        /// Document.
        document: DocumentId,
        /// Background job.
        job: JobId,
    },
}

/// Response to a [`DocumentToolRequest`]: `{"ok": …}` or `{"error": …}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentToolResponse {
    /// Success.
    Ok(DocumentToolOutput),
    /// Failure.
    Error(EngineError),
}

impl From<Result<DocumentToolOutput, EngineError>> for DocumentToolResponse {
    fn from(r: Result<DocumentToolOutput, EngineError>) -> Self {
        match r {
            Ok(o) => Self::Ok(o),
            Err(e) => Self::Error(e),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::document::{BlendMode, CanvasRect, DocumentDepth, DocumentFormat, GroupMode};
    use crate::recipe::mask::MaskKind;
    use crate::tile::Extent;
    use serde_json::json;

    fn img() -> ImageId {
        ImageId(0xabc)
    }

    pub(crate) fn all_document_calls() -> Vec<DocumentToolCall> {
        let doc = DocumentId(1);
        vec![
            DocumentToolCall::OpenDocument {
                path: "/work/poster.psd".into(),
            },
            DocumentToolCall::AddLayer {
                document: doc,
                layer: NewLayer::Group {
                    mode: GroupMode::Isolated,
                },
                name: Some("Retouch".into()),
                parent: None,
                above: Some(LayerId(2)),
                props: LayerPropsUpdate {
                    blend_mode: Some(BlendMode::Multiply),
                    ..Default::default()
                },
            },
            DocumentToolCall::SetLayerProps {
                document: doc,
                layer: LayerId(3),
                props: LayerPropsUpdate {
                    opacity: Some(0.5),
                    visible: Some(false),
                    color_tag: Some(FieldUpdate::Set("red".into())),
                    ..Default::default()
                },
            },
            DocumentToolCall::PaintStroke {
                document: doc,
                layer: LayerId(3),
                points: vec![
                    StrokePoint {
                        x: 10.0,
                        y: 10.0,
                        pressure: 0.5,
                    },
                    StrokePoint {
                        x: 40.0,
                        y: 12.0,
                        pressure: 1.0,
                    },
                ],
                brush: BrushParams {
                    size: 12.0,
                    color: [1.0, 0.0, 0.0],
                    ..Default::default()
                },
                target: StrokeTarget::Mask,
            },
            DocumentToolCall::SetPixelSelection {
                document: doc,
                shape: SelectionShape::Ellipse {
                    rect: CanvasRect {
                        x0: 0,
                        y0: 0,
                        x1: 64,
                        y1: 32,
                    },
                },
                mode: SelectionMode::Add,
                feather: 2.0,
                save_as: Some("face".into()),
            },
            DocumentToolCall::ApplyAdjustmentLayer {
                document: doc,
                adjustment: AdjustmentSpec::HueSaturation {
                    hue: 10.0,
                    saturation: -20.0,
                    lightness: 0.0,
                    colorize: false,
                },
                name: None,
                parent: Some(LayerId(4)),
                above: None,
                clipped: true,
                mask_from_selection: true,
            },
            DocumentToolCall::TransformLayer {
                document: doc,
                layer: LayerId(3),
                transform: AffineTransform([0.5, 0.0, 10.0, 0.0, 0.5, 20.0]),
                interpolation: Interpolation::Bilinear,
            },
            DocumentToolCall::MergeDown {
                document: doc,
                layer: LayerId(3),
            },
            DocumentToolCall::ExportDocument {
                document: doc,
                settings: DocumentExportSettings {
                    path: "/out/poster.psd".into(),
                    format: DocumentFormat::Psd {
                        maximize_compatibility: true,
                    },
                },
            },
            DocumentToolCall::ListLayers { document: doc },
        ]
    }

    pub(crate) fn all_calls() -> Vec<ToolCall> {
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
    fn every_document_call_round_trips_and_is_tagged_by_name() {
        let calls = all_document_calls();
        assert_eq!(calls.len(), DocumentToolCall::NAMES.len());
        for (call, name) in calls.iter().zip(DocumentToolCall::NAMES) {
            assert_eq!(call.name(), name);
            assert!(!ToolCall::NAMES.contains(&name), "{name} collides");
            let v = serde_json::to_value(call).unwrap();
            assert_eq!(v["tool"], name);
            assert_eq!(
                serde_json::from_value::<DocumentToolCall>(v).unwrap(),
                *call
            );
            assert_eq!(call.document().is_none(), name == "open_document");
            assert!(!(call.edits_document() && call.is_read_only()));
        }
    }

    #[test]
    fn document_call_wire_form() {
        // Flat JSON, as an MCP client sends it; defaults fill the rest.
        let c: DocumentToolCall = serde_json::from_value(json!({
            "tool": "set_pixel_selection",
            "document": 2,
            "shape": "rect",
            "rect": {"x0": 0, "y0": 0, "x1": 10, "y1": 10}
        }))
        .unwrap();
        assert_eq!(
            c,
            DocumentToolCall::SetPixelSelection {
                document: DocumentId(2),
                shape: SelectionShape::Rect {
                    rect: CanvasRect {
                        x0: 0,
                        y0: 0,
                        x1: 10,
                        y1: 10
                    }
                },
                mode: SelectionMode::Replace,
                feather: 0.0,
                save_as: None,
            }
        );
        let p: DocumentToolCall = serde_json::from_value(json!({
            "tool": "set_layer_props", "document": 2, "layer": 5, "opacity": 0.25,
            "blend_mode": "soft_light"
        }))
        .unwrap();
        let DocumentToolCall::SetLayerProps { props, .. } = &p else {
            panic!()
        };
        assert_eq!(props.opacity, Some(0.25));
        assert_eq!(props.blend_mode, Some(BlendMode::SoftLight));
        let s: DocumentToolCall = serde_json::from_value(json!({
            "tool": "paint_stroke", "document": 2, "layer": 5,
            "points": [{"x": 1, "y": 2}]
        }))
        .unwrap();
        let DocumentToolCall::PaintStroke { brush, target, .. } = &s else {
            panic!()
        };
        assert_eq!(*brush, BrushParams::default());
        assert_eq!(*target, StrokeTarget::Pixels);
        assert!(s.edits_document());
        let a: DocumentToolCall = serde_json::from_value(json!({
            "tool": "apply_adjustment_layer", "document": 2,
            "adjustment": {"kind": "invert"}
        }))
        .unwrap();
        let DocumentToolCall::ApplyAdjustmentLayer {
            mask_from_selection,
            ..
        } = a
        else {
            panic!()
        };
        assert!(mask_from_selection);
    }

    #[test]
    fn document_envelopes() {
        let req = DocumentToolRequest {
            call: DocumentToolCall::MergeDown {
                document: DocumentId(1),
                layer: LayerId(2),
            },
            rationale: Some("flatten retouch".into()),
            group: Some(HistoryGroupId(1)),
            expect_head: None,
        };
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["tool"], "merge_down");
        assert!(v.get("expect_head").is_none());
        assert_eq!(
            serde_json::from_value::<DocumentToolRequest>(v).unwrap(),
            req
        );
        for expect in [Some(None), Some(Some(HistoryEntryId(3)))] {
            let r = DocumentToolRequest {
                expect_head: expect,
                ..req.clone()
            };
            let v = serde_json::to_value(&r).unwrap();
            assert!(v.get("expect_head").is_some());
            assert_eq!(serde_json::from_value::<DocumentToolRequest>(v).unwrap(), r);
        }

        let outputs = [
            DocumentToolOutput::DocumentOpened {
                document: DocumentId(1),
                summary: DocumentSummary {
                    canvas: Extent::new(640, 480),
                    depth: DocumentDepth::U16,
                    layers: 3,
                },
            },
            DocumentToolOutput::DocumentEdited {
                document: DocumentId(1),
                entry: Some(HistoryEntryId(1)),
                layer: Some(LayerId(4)),
                selection: None,
            },
            DocumentToolOutput::LayerList {
                document: DocumentId(1),
                layers: vec![LayerInfo {
                    id: LayerId(1),
                    parent: None,
                    name: "Background".into(),
                    kind: crate::document::LayerKindTag::Pixel,
                    visible: true,
                    opacity: 1.0,
                    fill_opacity: 1.0,
                    blend_mode: BlendMode::Normal,
                    group_mode: None,
                    clipped: false,
                    has_mask: false,
                    bounds: Some(CanvasRect {
                        x0: 0,
                        y0: 0,
                        x1: 640,
                        y1: 480,
                    }),
                }],
            },
            DocumentToolOutput::DocumentExportQueued {
                document: DocumentId(1),
                job: JobId(7),
            },
        ];
        let tags = [
            "document_opened",
            "document_edited",
            "layer_list",
            "document_export_queued",
        ];
        for (o, tag) in outputs.into_iter().zip(tags) {
            let ok: DocumentToolResponse = Ok(o).into();
            let v = serde_json::to_value(&ok).unwrap();
            assert_eq!(v["ok"]["tool"], tag);
            assert_eq!(
                serde_json::from_value::<DocumentToolResponse>(v).unwrap(),
                ok
            );
        }
        let err: DocumentToolResponse = Err(EngineError::not_found("layer", LayerId(9))).into();
        let v = serde_json::to_value(&err).unwrap();
        assert_eq!(v["error"]["code"], "not_found");
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
