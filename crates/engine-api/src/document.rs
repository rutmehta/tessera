//! Layered-document contracts (spec 02 §1, §11): the value types that the
//! layered-document tool calls ([`crate::tools::DocumentToolCall`]) carry,
//! and the document history ([`DocumentHistory`]).
//!
//! The document model itself (layer tree, rasters, compositing) lives in the
//! compositor crate. Everything here is a serializable description that an
//! agent, script or recorded [`crate::action::Action`] can hold without
//! touching pixels. Serialized forms match the compositor's own serde forms
//! where the compositor has one (blend modes, group modes, adjustments,
//! depth, rectangles), so converting is a JSON round trip; the compositor's
//! `tests/contracts.rs` checks this. [`AffineTransform`] uses the same
//! coefficient order as the compositor's `Affine::m` but a bare-array wire form.
//!
//! Coordinates are level-0 canvas pixels unless stated otherwise; colours
//! are straight, normalized `[0, 1]` in the document's encoding.

use serde::{Deserialize, Serialize};

use crate::action::Action;
use crate::color::IccProfileHandle;
use crate::error::{EngineError, EngineResult};
use crate::id::{ChannelId, Digest, HistoryEntryId, HistoryGroupId, LayerId, SelectionId};
use crate::recipe::history::{EditMeta, HistoryGroup, Snapshot};
use crate::tile::Extent;
use crate::tools::{ExportFormat, FieldUpdate, Resize};

/// Bits per channel of a layered document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentDepth {
    /// 8-bit unsigned, display-referred.
    #[default]
    U8,
    /// 16-bit unsigned, display-referred.
    U16,
    /// 32-bit float, scene-referred, unbounded.
    F32,
}

/// The 27 Photoshop layer blend modes, in Photoshop's menu order. Groups use
/// [`GroupMode`] for pass-through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(missing_docs)]
pub enum BlendMode {
    #[default]
    Normal,
    Dissolve,
    Darken,
    Multiply,
    ColorBurn,
    LinearBurn,
    DarkerColor,
    Lighten,
    Screen,
    ColorDodge,
    /// Add.
    LinearDodge,
    LighterColor,
    Overlay,
    SoftLight,
    HardLight,
    VividLight,
    LinearLight,
    PinLight,
    HardMix,
    Difference,
    Exclusion,
    Subtract,
    Divide,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

impl BlendMode {
    /// Every mode, in Photoshop's menu order.
    pub const ALL: [BlendMode; 27] = [
        Self::Normal,
        Self::Dissolve,
        Self::Darken,
        Self::Multiply,
        Self::ColorBurn,
        Self::LinearBurn,
        Self::DarkerColor,
        Self::Lighten,
        Self::Screen,
        Self::ColorDodge,
        Self::LinearDodge,
        Self::LighterColor,
        Self::Overlay,
        Self::SoftLight,
        Self::HardLight,
        Self::VividLight,
        Self::LinearLight,
        Self::PinLight,
        Self::HardMix,
        Self::Difference,
        Self::Exclusion,
        Self::Subtract,
        Self::Divide,
        Self::Hue,
        Self::Saturation,
        Self::Color,
        Self::Luminosity,
    ];
}

/// How a group composites its children.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupMode {
    /// Children blend straight into the backdrop.
    #[default]
    PassThrough,
    /// Children composite into their own buffer, blended with the group's mode.
    Isolated,
}

/// A half-open rectangle in level-0 canvas pixels, `[x0, x1) × [y0, y1)`.
/// May extend past the canvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct CanvasRect {
    /// Left edge (inclusive).
    pub x0: i64,
    /// Top edge (inclusive).
    pub y0: i64,
    /// Right edge (exclusive).
    pub x1: i64,
    /// Bottom edge (exclusive).
    pub y1: i64,
}

impl CanvasRect {
    /// True if the rectangle covers no pixel.
    pub const fn is_empty(&self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }
}

/// What kind of layer a [`LayerInfo`] describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayerKindTag {
    /// Pixel layer.
    Pixel,
    /// Adjustment layer.
    Adjustment,
    /// Fill layer (solid, gradient, pattern).
    Fill,
    /// Group.
    Group,
    /// Smart object.
    SmartObject,
    /// Text layer.
    Text,
}

/// The kind of layer `add_layer` creates. Adjustment layers are created by
/// `apply_adjustment_layer`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NewLayer {
    /// An empty (transparent) pixel layer covering the canvas.
    Pixel,
    /// An empty group.
    Group {
        /// Group mode.
        #[serde(default)]
        mode: GroupMode,
    },
    /// A solid-colour fill layer.
    SolidColor {
        /// Straight RGB.
        color: [f32; 3],
    },
}

/// Partial update of a layer's common properties. `None` leaves a property
/// unchanged.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LayerPropsUpdate {
    /// Display name.
    pub name: Option<String>,
    /// Visibility.
    pub visible: Option<bool>,
    /// Layer opacity `0..=1`.
    pub opacity: Option<f32>,
    /// Fill opacity `0..=1`.
    pub fill_opacity: Option<f32>,
    /// Blend mode.
    pub blend_mode: Option<BlendMode>,
    /// Clip to the layer below.
    pub clipped: Option<bool>,
    /// Colour tag.
    pub color_tag: Option<FieldUpdate<String>>,
}

impl LayerPropsUpdate {
    /// True if the update changes nothing.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// One sampled point of a brush stroke, level-0 canvas pixels.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StrokePoint {
    /// X.
    pub x: f32,
    /// Y.
    pub y: f32,
    /// Pen pressure `0..=1` (1 without a tablet).
    #[serde(default = "one")]
    pub pressure: f32,
}

/// Whether a stroke deposits or removes paint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrushMode {
    /// Paint `color` (Normal blending).
    #[default]
    Paint,
    /// Erase to transparency (on a mask: paint black).
    Erase,
}

/// Round-brush parameters (spec 02 §4). The engine rasterizes the stroke; the
/// caller never supplies pixels.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BrushParams {
    /// Tip diameter in level-0 pixels.
    pub size: f32,
    /// Edge hardness `0..=1`.
    pub hardness: f32,
    /// Dab spacing as a fraction of `size`.
    pub spacing: f32,
    /// Per-dab flow `0..=1`.
    pub flow: f32,
    /// Stroke opacity cap `0..=1`.
    pub opacity: f32,
    /// Straight RGB (ignored when erasing; on a mask only the luminance counts).
    pub color: [f32; 3],
    /// Paint or erase.
    pub mode: BrushMode,
    /// Pressure scales the tip size.
    pub pressure_size: bool,
    /// Pressure scales the flow.
    pub pressure_flow: bool,
}

impl Default for BrushParams {
    fn default() -> Self {
        Self {
            size: 20.0,
            hardness: 1.0,
            spacing: 0.25,
            flow: 1.0,
            opacity: 1.0,
            color: [0.0; 3],
            mode: BrushMode::Paint,
            pressure_size: false,
            pressure_flow: false,
        }
    }
}

/// What a stroke paints on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StrokeTarget {
    /// The layer's pixels.
    #[default]
    Pixels,
    /// The layer's raster mask (created reveal-all if absent).
    Mask,
}

/// The region a selection change describes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "shape", rename_all = "snake_case")]
pub enum SelectionShape {
    /// Select the whole canvas.
    All,
    /// Deselect (`mode` is ignored).
    None,
    /// Invert the current selection (`mode` is ignored).
    Inverse,
    /// Rectangle.
    Rect {
        /// Rectangle.
        rect: CanvasRect,
    },
    /// Ellipse inscribed in `rect`.
    Ellipse {
        /// Bounding rectangle.
        rect: CanvasRect,
    },
    /// Closed polygon (lasso), even-odd fill.
    Polygon {
        /// Vertices `[x, y]`.
        points: Vec<[f32; 2]>,
    },
    /// A layer's transparency (Ctrl-click a thumbnail).
    LayerAlpha {
        /// Layer.
        layer: LayerId,
    },
    /// A saved selection.
    Saved {
        /// Saved selection.
        selection: SelectionId,
    },
}

/// How a selection shape combines with the current selection
/// (per-pixel replace / max / `a·(1 − b)` / min).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionMode {
    /// Replace.
    #[default]
    Replace,
    /// Add (union).
    Add,
    /// Subtract.
    Subtract,
    /// Intersect.
    Intersect,
}

/// Levels for one channel (input black/white, gamma, output black/white),
/// normalized `[0, 1]`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LevelsChannel {
    /// Input black point.
    pub in_black: f32,
    /// Input white point.
    pub in_white: f32,
    /// Midtone gamma (> 0, 1 = neutral).
    pub gamma: f32,
    /// Output black.
    pub out_black: f32,
    /// Output white.
    pub out_white: f32,
}

impl Default for LevelsChannel {
    fn default() -> Self {
        Self {
            in_black: 0.0,
            in_white: 1.0,
            gamma: 1.0,
            out_black: 0.0,
            out_white: 1.0,
        }
    }
}

/// An adjustment layer's function (spec 02 §7). Field names and the `kind`
/// tag match the compositor's `Adjustment`. Omitted fields are neutral.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AdjustmentSpec {
    /// Levels: per channel, then master.
    Levels {
        /// Master.
        #[serde(default)]
        master: LevelsChannel,
        /// R, G, B.
        #[serde(default)]
        rgb: [LevelsChannel; 3],
    },
    /// Curves through `(input, output)` points; empty = identity.
    Curves {
        /// Master.
        #[serde(default)]
        master: Vec<[f32; 2]>,
        /// R, G, B.
        #[serde(default)]
        rgb: [Vec<[f32; 2]>; 3],
    },
    /// Hue/Saturation.
    HueSaturation {
        /// Hue shift, degrees `-180..=180`.
        #[serde(default)]
        hue: f32,
        /// Saturation `-100..=100`.
        #[serde(default)]
        saturation: f32,
        /// Lightness `-100..=100`.
        #[serde(default)]
        lightness: f32,
        /// Colorize.
        #[serde(default)]
        colorize: bool,
    },
    /// Exposure.
    Exposure {
        /// EV.
        #[serde(default)]
        exposure: f32,
        /// Offset.
        #[serde(default)]
        offset: f32,
        /// Gamma correction (1 = neutral).
        #[serde(default = "one")]
        gamma: f32,
    },
    /// Invert.
    Invert,
    /// Posterize.
    Posterize {
        /// Levels per channel (≥ 2).
        levels: u32,
    },
    /// Threshold on Rec.601 luma.
    Threshold {
        /// Level `0..=1`.
        level: f32,
    },
    /// Channel mixer.
    ChannelMixer {
        /// Row-major 3×3; row i is output channel i.
        #[serde(default = "identity3")]
        matrix: [[f32; 3]; 3],
        /// Constant per output channel.
        #[serde(default)]
        constant: [f32; 3],
        /// Monochrome (row 0 to all channels).
        #[serde(default)]
        monochrome: bool,
    },
}

/// Resampling filter for transforms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Interpolation {
    /// Nearest neighbour.
    Nearest,
    /// Bilinear.
    Bilinear,
    /// Bicubic.
    #[default]
    Bicubic,
}

/// A 2-D affine map `x' = a·x + b·y + c`, `y' = d·x + e·y + f` in level-0
/// canvas pixels, row-major `[a, b, c, d, e, f]` (the compositor's `Affine`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AffineTransform(pub [f64; 6]);

impl AffineTransform {
    /// The identity.
    pub const IDENTITY: Self = Self([1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
}

impl Default for AffineTransform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// Container written by `export_document`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "container", rename_all = "snake_case")]
pub enum DocumentFormat {
    /// The native `.tessera-doc` (layers, masks, smart objects; no history).
    TesseraDoc,
    /// Photoshop document.
    Psd {
        /// Also write the flattened composite.
        #[serde(default = "yes")]
        maximize_compatibility: bool,
    },
    /// Large Document Format.
    Psb {
        /// Also write the flattened composite.
        #[serde(default = "yes")]
        maximize_compatibility: bool,
    },
    /// Flattened image.
    Image {
        /// Encoding.
        encoding: ExportFormat,
        /// Resize, if any.
        #[serde(default)]
        resize: Option<Resize>,
        /// Output profile; `None` keeps the document profile.
        #[serde(default)]
        profile: Option<IccProfileHandle>,
    },
}

/// `export_document` settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentExportSettings {
    /// Absolute output file path.
    pub path: String,
    /// Container and encoding.
    pub format: DocumentFormat,
}

/// One row of `list_layers`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayerInfo {
    /// Layer.
    pub id: LayerId,
    /// Enclosing group, or `None` at the root.
    #[serde(default)]
    pub parent: Option<LayerId>,
    /// Display name.
    pub name: String,
    /// Kind.
    pub kind: LayerKindTag,
    /// Visibility.
    pub visible: bool,
    /// Opacity `0..=1`.
    pub opacity: f32,
    /// Fill opacity `0..=1`.
    pub fill_opacity: f32,
    /// Blend mode.
    pub blend_mode: BlendMode,
    /// Group mode, for groups.
    #[serde(default)]
    pub group_mode: Option<GroupMode>,
    /// Clipped to the layer below.
    #[serde(default)]
    pub clipped: bool,
    /// Has a raster mask.
    #[serde(default)]
    pub has_mask: bool,
    /// Bounds of non-transparent content, if known and non-empty.
    #[serde(default)]
    pub bounds: Option<CanvasRect>,
}

/// An immutable document history entry: the [`Action`] that produced the
/// state plus the same metadata a recipe history entry carries (author,
/// label, timestamp, group, rationale).
///
/// Unlike recipe history the entry does not store the state change: the
/// document's states are copy-on-write snapshots held by the compositor,
/// one per entry. The action is the replayable, human-readable record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentHistoryEntry {
    /// Id; equals the entry's 1-based position in [`DocumentHistory::entries`].
    pub id: HistoryEntryId,
    /// Parent entry, or `None` for a child of the opened state.
    #[serde(default)]
    pub parent: Option<HistoryEntryId>,
    /// Metadata.
    #[serde(flatten)]
    pub meta: EditMeta,
    /// The command that produced this state.
    pub action: Action,
}

/// The document history tree: append-only, branching undo, named snapshots
/// and groups, with the recipe history's invariants (CONTRACTS.md
/// invariants 6 and 16).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DocumentHistory {
    /// All entries ever recorded, in creation order.
    pub entries: Vec<DocumentHistoryEntry>,
    /// Entry the current state corresponds to (`None` = as opened).
    pub head: Option<HistoryEntryId>,
    /// Named snapshots.
    pub snapshots: Vec<Snapshot>,
    /// Named groups.
    pub groups: Vec<HistoryGroup>,
}

impl DocumentHistory {
    /// Looks up an entry.
    pub fn entry(&self, id: HistoryEntryId) -> Option<&DocumentHistoryEntry> {
        let idx = usize::try_from(id.0).ok()?.checked_sub(1)?;
        self.entries.get(idx).filter(|e| e.id == id)
    }

    fn require(&self, id: HistoryEntryId) -> EngineResult<&DocumentHistoryEntry> {
        self.entry(id)
            .ok_or_else(|| EngineError::not_found("history entry", id))
    }

    /// Entries from the opened state to `target`, oldest first.
    pub fn lineage(
        &self,
        target: Option<HistoryEntryId>,
    ) -> EngineResult<Vec<&DocumentHistoryEntry>> {
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

    /// Records `action` as a child of `head` and advances `head`.
    pub fn record(&mut self, action: Action, meta: EditMeta) -> HistoryEntryId {
        let id = HistoryEntryId(self.entries.len() as u64 + 1);
        self.entries.push(DocumentHistoryEntry {
            id,
            parent: self.head,
            meta,
            action,
        });
        self.head = Some(id);
        id
    }

    /// Moves `head` to `target` (undo, redo and history jumps).
    pub fn checkout(&mut self, target: Option<HistoryEntryId>) -> EngineResult<()> {
        if let Some(id) = target {
            self.require(id)?;
        }
        self.head = target;
        Ok(())
    }

    /// Parent of `head` (the undo target), if `head` is not the opened state.
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

    /// Adds a snapshot of `entry`. Names must be unique and non-empty.
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

    /// Checks structural invariants: ids sequential, parents precede
    /// children, `head` and snapshot targets exist.
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
        for s in &self.snapshots {
            if let Some(e) = s.entry {
                self.require(e)?;
            }
        }
        Ok(())
    }
}

/// Persistent channel interpretation. Spot metadata is display-only, not a
/// spectral ink model, and currently does not change RGB compositing.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChannelKind {
    /// Saved selection mask.
    #[default]
    Alpha,
    /// Spot ink mask.
    Spot {
        /// Finite normalized RGB display colour.
        display_rgb: [f32; 3],
        /// Finite normalized ink solidity.
        solidity: f32,
    },
}

/// One document-owned channel. Duplicate names are allowed; address by ID.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ChannelSummary {
    /// Stable ID within this document.
    pub id: ChannelId,
    /// Display name.
    pub name: String,
    /// Alpha or spot interpretation.
    pub kind: ChannelKind,
    /// Stored raster depth, independent of document depth.
    pub depth: DocumentDepth,
    /// Single-channel raster extent (must match the canvas).
    pub extent: Extent,
}

/// Immutable single-channel raster staged by the host, not an inline pixel
/// buffer or URL. The host resolves the content digest and validates depth,
/// extent and finite normalized samples before applying the atomic edit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelRasterRef {
    /// BLAKE3 content identity in the host's raster store; retained for replay.
    pub digest: Digest,
    /// Stored depth.
    pub depth: DocumentDepth,
    /// Canvas-sized raster extent.
    pub extent: Extent,
}

/// Canvas size, depth and ordered persistent channels reported on open.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct DocumentSummary {
    /// Canvas size.
    pub canvas: Extent,
    /// Depth.
    pub depth: DocumentDepth,
    /// Number of layers (all depths).
    pub layers: u32,
    /// Persistent channels in document order, independent of active selection.
    /// Missing in 1.2 documents means no reported channels.
    #[serde(default)]
    pub channels: Vec<ChannelSummary>,
}

fn one() -> f32 {
    1.0
}

fn yes() -> bool {
    true
}

fn identity3() -> [[f32; 3]; 3] {
    [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe::history::Author;
    use serde_json::json;

    #[test]
    fn blend_mode_names_are_stable() {
        let names: Vec<String> = BlendMode::ALL
            .iter()
            .map(|m| {
                serde_json::to_value(m)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect();
        assert_eq!(names.len(), 27);
        assert_eq!(names[0], "normal");
        assert_eq!(names[10], "linear_dodge");
        assert_eq!(names[26], "luminosity");
        let mut sorted = names.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), 27);
    }

    #[test]
    fn adjustment_minimal_json_is_neutral() {
        let a: AdjustmentSpec = serde_json::from_value(json!({"kind": "exposure"})).unwrap();
        assert_eq!(
            a,
            AdjustmentSpec::Exposure {
                exposure: 0.0,
                offset: 0.0,
                gamma: 1.0
            }
        );
        let m: AdjustmentSpec = serde_json::from_value(json!({"kind": "channel_mixer"})).unwrap();
        let AdjustmentSpec::ChannelMixer { matrix, .. } = m else {
            panic!()
        };
        assert_eq!(matrix, identity3());
        let l: AdjustmentSpec =
            serde_json::from_value(json!({"kind": "levels", "master": {"gamma": 1.2}})).unwrap();
        let v = serde_json::to_value(&l).unwrap();
        assert_eq!(v["master"]["in_white"], 1.0);
        assert_eq!(v["rgb"].as_array().unwrap().len(), 3);
        assert_eq!(serde_json::from_value::<AdjustmentSpec>(v).unwrap(), l);
    }

    #[test]
    fn value_types_round_trip() {
        let shapes = [
            SelectionShape::All,
            SelectionShape::None,
            SelectionShape::Inverse,
            SelectionShape::Rect {
                rect: CanvasRect {
                    x0: -4,
                    y0: 0,
                    x1: 100,
                    y1: 50,
                },
            },
            SelectionShape::Polygon {
                points: vec![[0.0, 0.0], [10.0, 0.0], [5.0, 8.0]],
            },
            SelectionShape::LayerAlpha { layer: LayerId(3) },
            SelectionShape::Saved {
                selection: SelectionId(1),
            },
        ];
        for s in shapes {
            let v = serde_json::to_value(&s).unwrap();
            assert!(v["shape"].is_string());
            assert_eq!(serde_json::from_value::<SelectionShape>(v).unwrap(), s);
        }
        let b: BrushParams = serde_json::from_value(json!({"size": 40})).unwrap();
        assert_eq!(b.size, 40.0);
        assert_eq!(b.hardness, 1.0);
        let p: StrokePoint = serde_json::from_value(json!({"x": 1, "y": 2})).unwrap();
        assert_eq!(p.pressure, 1.0);
        let f: DocumentFormat = serde_json::from_value(json!({"container": "psd"})).unwrap();
        assert_eq!(
            f,
            DocumentFormat::Psd {
                maximize_compatibility: true
            }
        );
        let img = DocumentFormat::Image {
            encoding: ExportFormat::Png { bit_depth: 16 },
            resize: None,
            profile: None,
        };
        let v = serde_json::to_value(&img).unwrap();
        assert_eq!(v["container"], "image");
        assert_eq!(v["encoding"]["format"], "png");
        assert_eq!(serde_json::from_value::<DocumentFormat>(v).unwrap(), img);
        assert_eq!(
            serde_json::to_value(AffineTransform::IDENTITY).unwrap(),
            json!([1.0, 0.0, 0.0, 0.0, 1.0, 0.0])
        );
        assert!(LayerPropsUpdate::default().is_empty());
        assert!(CanvasRect::default().is_empty());
    }

    fn agent(rationale: &str) -> EditMeta {
        EditMeta {
            label: "step".into(),
            author: Author::Agent {
                name: "planner".into(),
            },
            timestamp_ms: 1,
            group: None,
            rationale: Some(rationale.into()),
        }
    }

    #[test]
    fn history_is_append_only_and_branches() {
        let mut h = DocumentHistory::default();
        let a = Action::new("merge_down", [("document", json!(1)), ("layer", json!(2))]);
        let e1 = h.record(a.clone(), agent("one"));
        let e2 = h.record(a.clone(), agent("two"));
        assert_eq!((e1, e2), (HistoryEntryId(1), HistoryEntryId(2)));
        assert_eq!(h.undo_target(), Some(Some(e1)));
        h.checkout(Some(e1)).unwrap();
        assert_eq!(h.redo_target(), Some(e2));
        let e3 = h.record(a.clone(), agent("branch"));
        assert_eq!(h.entry(e3).unwrap().parent, Some(e1));
        assert_eq!(h.entries.len(), 3, "undo never removes entries");
        assert_eq!(
            h.lineage(Some(e3))
                .unwrap()
                .iter()
                .map(|e| e.id)
                .collect::<Vec<_>>(),
            [e1, e3]
        );
        h.add_snapshot("before", Some(e1), 5).unwrap();
        assert!(h.add_snapshot("before", None, 6).is_err());
        assert!(h.add_snapshot("ghost", Some(HistoryEntryId(9)), 6).is_err());
        assert!(h.checkout(Some(HistoryEntryId(9))).is_err());
        let g = h.add_group("Agent base edit");
        assert_eq!(g, HistoryGroupId(1));
        h.validate().unwrap();

        let v = serde_json::to_value(&h).unwrap();
        assert_eq!(v["entries"][0]["author"]["kind"], "agent");
        assert_eq!(v["entries"][0]["rationale"], "one");
        assert_eq!(v["entries"][0]["action"]["command"], "merge_down");
        let back: DocumentHistory = serde_json::from_value(v).unwrap();
        assert_eq!(back, h);

        let mut bad = h.clone();
        bad.entries.swap(0, 1);
        assert!(bad.validate().is_err());
    }
}
