//! The layered document model (spec 02 §1): canvas, depth, profile, a tree
//! of layers, masks and the selection. A [`DocState`] is an immutable,
//! structurally shared snapshot; [`crate::Document`] adds history.

use std::sync::Arc;

use engine_api::color::IccProfileHandle;
use engine_api::tile::Extent;
use engine_api::{EngineError, EngineResult};
use serde::{Deserialize, Serialize};

use crate::adjust::Adjustment;
use crate::blend::{BlendIf, BlendMode};
use crate::geom::{Affine, Rect, next_doc_key};
use crate::raster::{Depth, Raster};

/// Layer identifier, unique within one document (nested smart-object
/// documents have their own id space). The engine-api contract type
/// (serialized as a bare integer); `LayerId(0)` is `LayerId::ROOT`, the
/// document itself, and real layers are numbered from 1.
pub use engine_api::id::LayerId;

/// Knockout (Layer Style ▸ Advanced Blending).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Knockout {
    /// No knockout.
    #[default]
    None,
    /// Knock out to the backdrop at the start of the enclosing group or
    /// clipping group (to the Background layer at the document root).
    Shallow,
    /// Knock out to the Background layer (transparency if there is none).
    Deep,
}

/// Lock flags. Enforced by edit ops; the compositor ignores them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Locks {
    /// Painting keeps existing alpha.
    pub transparency: bool,
    /// No pixel edits.
    pub pixels: bool,
    /// No moves (translation is not modelled yet; stored for PSD round trip).
    pub position: bool,
    /// Everything locked.
    pub all: bool,
}

/// Properties every layer kind has. Changing them bumps `props_rev`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LayerProps {
    /// Non-destructive CPU layer effects; independent of fill opacity.
    pub styles: crate::render::styles::LayerStyles,
    /// Display name.
    pub name: String,
    /// Hidden layers (and layers clipped to them) do not render.
    pub visible: bool,
    /// Layer opacity in `[0, 1]`.
    pub opacity: f32,
    /// Fill opacity in `[0, 1]` (interior only; equals opacity without styles).
    pub fill_opacity: f32,
    /// Blend mode (ignored by pass-through groups).
    pub blend_mode: BlendMode,
    /// Blend If sliders.
    pub blend_if: BlendIf,
    /// Knockout.
    pub knockout: Knockout,
    /// Clipped to the nearest non-clipped layer below.
    pub clipped: bool,
    /// Lock flags.
    pub locks: Locks,
    /// The document Background layer (bottom of the root; deep knockout target).
    pub background: bool,
    /// Optional colour tag.
    pub color_tag: Option<String>,
}

impl Default for LayerProps {
    fn default() -> Self {
        Self {
            styles: Default::default(),
            name: String::new(),
            visible: true,
            opacity: 1.0,
            fill_opacity: 1.0,
            blend_mode: BlendMode::Normal,
            blend_if: BlendIf::default(),
            knockout: Knockout::None,
            clipped: false,
            locks: Locks::default(),
            background: false,
            color_tag: None,
        }
    }
}

/// A raster layer mask (single channel, document depth).
#[derive(Debug, Clone)]
pub struct Mask {
    /// Mask values; absent tiles read as the raster default (255 = reveal).
    pub raster: Raster,
    /// Density `d`: effective mask `1 − d·(1 − m)`.
    pub density: f32,
    /// Feather radius in pixels. Stored, not rendered yet (COMPOSITOR.md §9).
    pub feather: f32,
    /// Disabled masks are ignored.
    pub enabled: bool,
}

impl Mask {
    /// An all-reveal (white) mask.
    pub fn reveal_all(extent: Extent, depth: Depth) -> Self {
        Self {
            raster: Raster::new(extent, 1, depth, 1.0),
            density: 1.0,
            feather: 0.0,
            enabled: true,
        }
    }

    /// An all-hide (black) mask.
    pub fn hide_all(extent: Extent, depth: Depth) -> Self {
        Self {
            raster: Raster::new(extent, 1, depth, 0.0),
            ..Self::reveal_all(extent, depth)
        }
    }
}

/// Vector mask placeholder: the path data is preserved but not rasterized.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct VectorMask {
    /// Whether the mask is enabled.
    pub enabled: bool,
    /// Opaque path payload (e.g. PSD `vmsk` records) for round trip.
    pub path: serde_json::Value,
}

/// How a group composites its children.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GroupMode {
    /// Children blend directly into the backdrop; group opacity/mask fade
    /// the result against the backdrop at group entry.
    #[default]
    PassThrough,
    /// Children composite into a transparent buffer that is then blended
    /// with the group's own mode.
    Isolated,
}

/// Gradient geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GradientKind {
    /// Along `start → end`.
    #[default]
    Linear,
    /// Distance from `start`, `|end − start|` = 1.
    Radial,
}

/// A gradient colour stop (straight RGBA).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GradientStop {
    /// Position in `[0, 1]`.
    pub position: f32,
    /// Straight RGBA.
    pub color: [f32; 4],
}

/// Fill layer content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Fill {
    /// Opaque solid colour.
    Solid {
        /// Straight RGB.
        color: [f32; 3],
    },
    /// Gradient in canvas pixels.
    Gradient {
        /// Geometry.
        gradient: GradientKind,
        /// Canvas point at t = 0.
        start: [f32; 2],
        /// Canvas point at t = 1.
        end: [f32; 2],
        /// At least one stop, sorted by position.
        stops: Vec<GradientStop>,
    },
    /// A small straight-RGBA image repeated from `origin`.
    Pattern {
        /// Pattern width.
        width: u32,
        /// Pattern height.
        height: u32,
        /// Interleaved straight RGBA, `width·height·4` values.
        rgba: Vec<f32>,
        /// Canvas position of pattern pixel (0, 0).
        origin: [f32; 2],
    },
}

impl Fill {
    /// Straight RGBA at canvas point `(x, y)` (level-0 pixel units).
    pub fn sample(&self, x: f32, y: f32) -> [f32; 4] {
        match self {
            Fill::Solid { color } => [color[0], color[1], color[2], 1.0],
            Fill::Gradient {
                gradient,
                start,
                end,
                stops,
            } => {
                let (dx, dy) = (end[0] - start[0], end[1] - start[1]);
                let len2 = dx * dx + dy * dy;
                let t = if len2 <= 0.0 {
                    0.0
                } else {
                    match gradient {
                        GradientKind::Linear => ((x - start[0]) * dx + (y - start[1]) * dy) / len2,
                        GradientKind::Radial => {
                            ((x - start[0]).powi(2) + (y - start[1]).powi(2)).sqrt() / len2.sqrt()
                        }
                    }
                }
                .clamp(0.0, 1.0);
                gradient_at(stops, t)
            }
            Fill::Pattern {
                width,
                height,
                rgba,
                origin,
            } => {
                if *width == 0 || *height == 0 || rgba.len() < (*width * *height * 4) as usize {
                    return [0.0; 4];
                }
                let px = ((x - origin[0]).floor() as i64).rem_euclid(i64::from(*width)) as usize;
                let py = ((y - origin[1]).floor() as i64).rem_euclid(i64::from(*height)) as usize;
                let i = (py * *width as usize + px) * 4;
                [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
            }
        }
    }
}

fn gradient_at(stops: &[GradientStop], t: f32) -> [f32; 4] {
    let Some(first) = stops.first() else {
        return [0.0; 4];
    };
    if t <= first.position {
        return first.color;
    }
    for w in stops.windows(2) {
        if t <= w[1].position {
            let span = w[1].position - w[0].position;
            let f = if span > 0.0 {
                (t - w[0].position) / span
            } else {
                1.0
            };
            let mut o = [0.0; 4];
            for (c, v) in o.iter_mut().enumerate() {
                *v = w[0].color[c] + (w[1].color[c] - w[0].color[c]) * f;
            }
            return o;
        }
    }
    stops[stops.len() - 1].color
}

/// One non-destructive filter, evaluated in vector order in child coordinates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct SmartFilter {
    /// Filter-result blend mode and opacity.
    pub blend: crate::render::smart_filters::FilterBlend,
    /// Filter identifier.
    pub name: String,
    /// Enabled flag.
    pub enabled: bool,
    /// Parameters.
    pub params: serde_json::Value,
}

impl SmartFilter {
    /// Construct an enabled, validated non-destructive transform stage.
    pub fn transform(op: transform::TransformOp) -> EngineResult<Self> {
        op.validate()
            .map_err(|e| EngineError::invalid("transform", e.to_string()))?;
        Ok(Self {
            name: "transform".into(),
            enabled: true,
            params: serde_json::to_value(op)
                .map_err(|e| EngineError::invalid("transform", e.to_string()))?,
            ..Self::default()
        })
    }

    /// Decode reserved transform parameters, including disabled stages.
    pub fn transform_op(&self) -> EngineResult<Option<transform::TransformOp>> {
        if self.name != "transform" {
            return Ok(None);
        }
        let op: transform::TransformOp = serde_json::from_value(self.params.clone())
            .map_err(|e| EngineError::invalid("transform", e.to_string()))?;
        op.validate()
            .map_err(|e| EngineError::invalid("transform", e.to_string()))?;
        Ok(Some(op))
    }
}

/// An embedded document rendered through the compositor and mapped into
/// the parent with an affine transform.
#[derive(Debug, Clone)]
pub struct SmartObject {
    /// The child document state.
    pub state: Arc<DocState>,
    /// Maps child level-0 pixel coordinates to parent level-0 coordinates.
    pub transform: Affine,
    /// Smart filters, bottom/input first.
    pub filters: Vec<SmartFilter>,
    /// Shared mask, in child coordinates; fades the completed stack.
    pub filter_mask: Option<Mask>,
    /// Runtime cache namespace of the child (not serialized).
    pub(crate) key: u64,
}

impl SmartObject {
    /// Wraps a child document.
    pub fn new(state: DocState, transform: Affine) -> Self {
        Self {
            state: Arc::new(state),
            transform,
            filters: Vec::new(),
            filter_mask: None,
            key: next_doc_key(),
        }
    }

    /// Parent-canvas bounds of the transformed child canvas.
    pub fn bounds(&self) -> Rect {
        self.transform.map_rect(&Rect::of_extent(self.state.canvas))
    }
}

/// A text layer placeholder: the text model is stored; pixels come from a
/// rasterized proxy supplied by the (future) type engine.
#[derive(Debug, Clone)]
pub struct TextLayer {
    /// The text.
    pub text: String,
    /// Font name.
    pub font: String,
    /// Size in pixels.
    pub size: f32,
    /// Straight RGB.
    pub color: [f32; 3],
    /// Rasterized proxy (straight RGBA).
    pub proxy: Raster,
}

/// What a layer is.
#[derive(Debug, Clone)]
pub enum LayerKind {
    /// Tiled straight-RGBA pixels.
    Pixel(Raster),
    /// A function of the composite below.
    Adjustment(Adjustment),
    /// Procedural fill.
    Fill(Fill),
    /// A folder of layers (index 0 = bottom).
    Group {
        /// Pass-through or isolated.
        mode: GroupMode,
        /// Children, bottom first.
        children: Vec<Arc<Layer>>,
    },
    /// Smart object.
    SmartObject(SmartObject),
    /// Text placeholder.
    Text(TextLayer),
}

/// One node of the layer tree.
#[derive(Debug, Clone)]
pub struct Layer {
    /// Identifier (assigned by the document when zero).
    pub id: LayerId,
    /// Common properties.
    pub props: LayerProps,
    /// Revision of the last props change.
    pub props_rev: u64,
    /// Revision of the last non-tile content change (adjustment, fill,
    /// transform, mask parameters, group mode, smart-object contents).
    pub content_rev: u64,
    /// Raster mask.
    pub mask: Option<Mask>,
    /// Vector mask placeholder.
    pub vector_mask: Option<VectorMask>,
    /// Kind and payload.
    pub kind: LayerKind,
}

impl Layer {
    /// A new layer with default properties and the given name.
    pub fn new(name: impl Into<String>, kind: LayerKind) -> Self {
        Self {
            id: LayerId(0),
            props: LayerProps {
                name: name.into(),
                ..LayerProps::default()
            },
            props_rev: 0,
            content_rev: 0,
            mask: None,
            vector_mask: None,
            kind,
        }
    }

    /// A transparent pixel layer.
    pub fn pixel(name: impl Into<String>, extent: Extent, depth: Depth) -> Self {
        Self::new(name, LayerKind::Pixel(Raster::new(extent, 4, depth, 0.0)))
    }

    /// An empty group.
    pub fn group(name: impl Into<String>, mode: GroupMode) -> Self {
        Self::new(
            name,
            LayerKind::Group {
                mode,
                children: Vec::new(),
            },
        )
    }

    /// Builder: sets the blend mode.
    pub fn with_mode(mut self, mode: BlendMode) -> Self {
        self.props.blend_mode = mode;
        self
    }

    /// Builder: sets the opacity.
    pub fn with_opacity(mut self, opacity: f32) -> Self {
        self.props.opacity = opacity;
        self
    }

    /// Builder: adds a child (on top) to a group.
    pub fn with_child(mut self, child: Layer) -> Self {
        if let LayerKind::Group { children, .. } = &mut self.kind {
            children.push(Arc::new(child));
        }
        self
    }

    /// The raster holding this layer's own pixels, if any.
    pub fn raster(&self) -> Option<&Raster> {
        match &self.kind {
            LayerKind::Pixel(r) => Some(r),
            LayerKind::Text(t) => Some(&t.proxy),
            _ => None,
        }
    }

    /// Mutable raster, if any.
    pub fn raster_mut(&mut self) -> Option<&mut Raster> {
        match &mut self.kind {
            LayerKind::Pixel(r) => Some(r),
            LayerKind::Text(t) => Some(&mut t.proxy),
            _ => None,
        }
    }

    /// Children if this is a group.
    pub fn children(&self) -> Option<&[Arc<Layer>]> {
        match &self.kind {
            LayerKind::Group { children, .. } => Some(children),
            _ => None,
        }
    }

    /// True for groups.
    pub fn is_group(&self) -> bool {
        matches!(self.kind, LayerKind::Group { .. })
    }

    /// Cache stamp of this layer's rendered contribution to the level
    /// `level` tile `(tx, ty)`: the maximum revision over everything that
    /// can affect it (COMPOSITOR.md §5).
    pub fn stamp(&self, level: u8, tx: u32, ty: u32) -> u64 {
        let mut s = self.props_rev.max(self.content_rev);
        if let Some(m) = &self.mask {
            s = s.max(m.raster.footprint_rev(level, tx, ty));
        }
        match &self.kind {
            LayerKind::Pixel(r) => s.max(r.footprint_rev(level, tx, ty)),
            LayerKind::Text(t) => s.max(t.proxy.footprint_rev(level, tx, ty)),
            LayerKind::Group { children, .. } => children
                .iter()
                .fold(s, |a, c| a.max(c.stamp(level, tx, ty))),
            LayerKind::SmartObject(so) => s.max(so.state.rev),
            LayerKind::Adjustment(_) | LayerKind::Fill(_) => s,
        }
    }

    /// Canvas region this layer can change when composited, or `None` for
    /// "anywhere" (procedural layers, adjustments, smart objects).
    pub fn affected_bounds(&self) -> Option<Rect> {
        if !self.props.styles.effects.is_empty() {
            return None;
        }
        match &self.kind {
            LayerKind::Pixel(r) => Some(r.bounds().unwrap_or_default()),
            LayerKind::Text(t) => Some(t.proxy.bounds().unwrap_or_default()),
            LayerKind::Group { children, .. } => {
                let mut r = Rect::default();
                for c in children {
                    r = r.union(&c.affected_bounds()?);
                }
                Some(r)
            }
            LayerKind::SmartObject(_) | LayerKind::Adjustment(_) | LayerKind::Fill(_) => None,
        }
    }

    /// Visits this layer and every descendant.
    pub fn visit(&self, f: &mut impl FnMut(&Layer)) {
        f(self);
        if let Some(ch) = self.children() {
            for c in ch {
                c.visit(f);
            }
        }
    }
}

/// An embedded ICC profile.
#[derive(Debug, Clone, PartialEq)]
pub struct ColorProfile {
    /// Human-readable description.
    pub name: String,
    /// Digest of the profile bytes (resolved by color-mgmt's registry).
    pub handle: IccProfileHandle,
    /// The profile bytes, when embedded.
    pub icc: Option<Arc<Vec<u8>>>,
}

impl ColorProfile {
    /// Wraps profile bytes.
    pub fn from_icc(name: impl Into<String>, icc: Vec<u8>) -> Self {
        Self {
            name: name.into(),
            handle: IccProfileHandle::from_profile_bytes(&icc),
            icc: Some(Arc::new(icc)),
        }
    }
}

/// An immutable snapshot of a whole document.
#[derive(Debug, Clone)]
pub struct DocState {
    /// Named alpha masks and spot inks, in panel/PSD order.
    pub channels: Vec<crate::channels::DocumentChannel>,
    /// Next channel identity (separate from layer IDs).
    pub next_channel_id: u64,
    /// Shared direction for effects using global light.
    pub global_light: crate::render::styles::GlobalLight,
    /// Canvas size in pixels.
    pub canvas: Extent,
    /// Bits per channel.
    pub depth: Depth,
    /// Resolution in pixels per inch.
    pub ppi: f32,
    /// Document profile (`None` = untagged, treated as sRGB).
    pub profile: Option<ColorProfile>,
    /// Root layers, bottom first.
    pub root: Vec<Arc<Layer>>,
    /// Revision of the last root-structure change (add/remove/move at root,
    /// canvas properties).
    pub root_rev: u64,
    /// Revision of the op that produced this state.
    pub rev: u64,
    /// Next layer id to allocate.
    pub next_id: u64,
    /// Selection: float single-channel tiled mask (`None` = select all).
    pub selection: Option<Arc<Raster>>,
}

impl DocState {
    /// Whether this tree (including nested smart objects) contains styles.
    /// Such documents conservatively invalidate the whole CPU composite.
    pub fn has_layer_styles(&self) -> bool {
        fn styled(layer: &Layer) -> bool {
            !layer.props.styles.effects.is_empty()
                || layer
                    .children()
                    .is_some_and(|c| c.iter().any(|l| styled(l)))
                || matches!(&layer.kind, LayerKind::SmartObject(so) if so.state.has_layer_styles())
        }
        self.root.iter().any(|l| styled(l))
    }

    /// Host-side preflight for the resident backend, which does not evaluate
    /// CPU layer effects. Call before selecting the resident renderer; the
    /// per-tile GPU port independently rejects its styled source operations.
    pub fn check_resident_effects(&self) -> EngineResult<()> {
        fn filtered(layer: &Layer) -> bool {
            layer
                .children()
                .is_some_and(|c| c.iter().any(|l| filtered(l)))
                || matches!(&layer.kind, LayerKind::SmartObject(so) if so.filters.iter().any(|f| f.enabled) || so.state.check_resident_effects().is_err())
        }
        if self.has_layer_styles() || self.root.iter().any(|l| filtered(l)) {
            Err(EngineError::Unsupported {
                what: "layer styles and smart filters require CPU rendering".into(),
            })
        } else {
            Ok(())
        }
    }

    /// An empty RGB document.
    pub fn new(canvas: Extent, depth: Depth) -> Self {
        Self {
            global_light: Default::default(),
            canvas,
            depth,
            ppi: 72.0,
            profile: None,
            root: Vec::new(),
            root_rev: 0,
            rev: 0,
            next_id: 1,
            selection: None,
            channels: Vec::new(),
            next_channel_id: 1,
        }
    }

    /// Finds a layer anywhere in the tree.
    pub fn find(&self, id: LayerId) -> Option<&Layer> {
        fn go(v: &[Arc<Layer>], id: LayerId) -> Option<&Layer> {
            for l in v {
                if l.id == id {
                    return Some(l);
                }
                if let Some(c) = l.children()
                    && let Some(f) = go(c, id)
                {
                    return Some(f);
                }
            }
            None
        }
        go(&self.root, id)
    }

    /// `(parent, index)` of a layer; parent `None` is the root.
    pub fn locate(&self, id: LayerId) -> Option<(Option<LayerId>, usize)> {
        fn go(
            v: &[Arc<Layer>],
            parent: Option<LayerId>,
            id: LayerId,
        ) -> Option<(Option<LayerId>, usize)> {
            for (i, l) in v.iter().enumerate() {
                if l.id == id {
                    return Some((parent, i));
                }
                if let Some(c) = l.children()
                    && let Some(f) = go(c, Some(l.id), id)
                {
                    return Some(f);
                }
            }
            None
        }
        go(&self.root, None, id)
    }

    /// Every layer id in the tree, depth first, bottom first.
    pub fn layer_ids(&self) -> Vec<LayerId> {
        let mut out = Vec::new();
        for l in &self.root {
            l.visit(&mut |x| out.push(x.id));
        }
        out
    }

    /// Mutable access to one layer, copying the path from the root (COW).
    pub fn layer_mut<R>(&mut self, id: LayerId, f: impl FnOnce(&mut Layer) -> R) -> Option<R> {
        fn go<R>(
            v: &mut [Arc<Layer>],
            id: LayerId,
            f: &mut Option<impl FnOnce(&mut Layer) -> R>,
        ) -> Option<R> {
            for l in v.iter_mut() {
                if l.id == id {
                    let f = f.take()?;
                    return Some(f(Arc::make_mut(l)));
                }
                let contains = l.children().is_some_and(|c| contains_id(c, id));
                if contains && let LayerKind::Group { children, .. } = &mut Arc::make_mut(l).kind {
                    return go(children, id, f);
                }
            }
            None
        }
        let mut f = Some(f);
        go(&mut self.root, id, &mut f)
    }

    /// The child list of `parent` (`None` = root), mutable, path-copied.
    pub(crate) fn children_mut(
        &mut self,
        parent: Option<LayerId>,
    ) -> EngineResult<&mut Vec<Arc<Layer>>> {
        match parent {
            None => Ok(&mut self.root),
            Some(pid) => {
                // Two-phase to satisfy the borrow checker: path-copy first.
                self.layer_mut(pid, |_| ())
                    .ok_or_else(|| EngineError::not_found("layer", pid.0))?;
                fn go(v: &mut [Arc<Layer>], id: LayerId) -> Option<&mut Vec<Arc<Layer>>> {
                    for l in v.iter_mut() {
                        let hit = l.id == id;
                        let contains = hit || l.children().is_some_and(|c| contains_id(c, id));
                        if !contains {
                            continue;
                        }
                        let LayerKind::Group { children, .. } = &mut Arc::make_mut(l).kind else {
                            return None;
                        };
                        return if hit {
                            Some(children)
                        } else {
                            go(children, id)
                        };
                    }
                    None
                }
                go(&mut self.root, pid).ok_or_else(|| EngineError::invalid("parent", "not a group"))
            }
        }
    }

    /// Assigns fresh ids to `layer` and its descendants where they are zero
    /// (or everywhere with `force`).
    pub(crate) fn assign_ids(&mut self, layer: &mut Layer, force: bool) {
        if force || layer.id.0 == 0 {
            layer.id = LayerId(self.next_id);
            self.next_id += 1;
        } else {
            self.next_id = self.next_id.max(layer.id.0 + 1);
        }
        if let LayerKind::Group { children, .. } = &mut layer.kind {
            for c in children.iter_mut() {
                self.assign_ids(Arc::make_mut(c), force);
            }
        }
    }

    /// Stamp of the whole composite at a tile.
    pub fn root_stamp(&self, level: u8, tx: u32, ty: u32) -> u64 {
        self.root
            .iter()
            .fold(self.root_rev, |a, l| a.max(l.stamp(level, tx, ty)))
    }
}

pub(crate) fn contains_id(v: &[Arc<Layer>], id: LayerId) -> bool {
    v.iter()
        .any(|l| l.id == id || l.children().is_some_and(|c| contains_id(c, id)))
}

/// Selection helpers: selections are F32 single-channel rasters.
pub mod selection {
    use super::*;

    /// How a new selection combines with the existing one.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum Combine {
        /// Replace.
        Replace,
        /// Union: `max(a, b)`.
        Add,
        /// Difference: `min(a, 1 − b)`.
        Subtract,
        /// Intersection: `min(a, b)`.
        Intersect,
    }

    /// A hard-edged rectangular selection.
    pub fn rect(extent: Extent, r: Rect) -> EngineResult<Raster> {
        let mut s = Raster::new(extent, 1, Depth::F32, 0.0);
        s.edit_region(r, 0, |_, _, p| p[0] = 1.0)?;
        Ok(s)
    }

    /// Combines two selections of the same extent pixel by pixel.
    pub fn combine(a: &Raster, b: &Raster, op: Combine) -> EngineResult<Raster> {
        if a.extent() != b.extent() {
            return Err(EngineError::invalid("selection", "extent mismatch"));
        }
        let f = |x: f32, y: f32| match op {
            Combine::Replace => y,
            Combine::Add => x.max(y),
            Combine::Subtract => x.min(1.0 - y),
            Combine::Intersect => x.min(y),
        };
        let mut out = Raster::new(
            a.extent(),
            1,
            Depth::F32,
            f(a.default_value(), b.default_value()),
        );
        let (cols, rows) = a.grid();
        let (mut ba, mut bb) = (Vec::new(), Vec::new());
        for ty in 0..rows {
            for tx in 0..cols {
                if a.slot(tx, ty).is_none() && b.slot(tx, ty).is_none() {
                    continue;
                }
                a.read_tile(tx, ty, &mut ba)?;
                b.read_tile(tx, ty, &mut bb)?;
                let data: Vec<f32> = ba.iter().zip(&bb).map(|(x, y)| f(*x, *y)).collect();
                let t = engine_api::tile::Tile::from_samples(
                    engine_api::tile::TileCoord::new(0, tx, ty),
                    out.layout(tx, ty),
                    data,
                )?;
                out.set_slot(tx, ty, Some(t), 0)?;
            }
        }
        Ok(out)
    }
}
