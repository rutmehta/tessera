//! Procedural masks, local adjustments and retouch operations.
//!
//! Masks are stored procedurally (what to compute), never as pixels. AI
//! components record the model that produced them; cached rasters live in
//! `.edits/<image>/masks/` and are regenerable, keyed by the component hash.

use serde::{Deserialize, Serialize};

use super::settings::NormalizedRect;
use crate::id::{MaskId, ModelRef, PersonId, RetouchId};

/// Body/face parts selectable on a detected person.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersonPart {
    /// Whole person.
    Body,
    /// Face skin.
    FaceSkin,
    /// Body skin.
    BodySkin,
    /// Eyebrows.
    Eyebrows,
    /// Eye sclera.
    Sclera,
    /// Iris and pupil.
    Iris,
    /// Lips.
    Lips,
    /// Teeth.
    Teeth,
    /// Hair.
    Hair,
    /// Clothing.
    Clothing,
}

/// Landscape segmentation classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LandscapeClass {
    /// Sky.
    Sky,
    /// Water.
    Water,
    /// Vegetation.
    Vegetation,
    /// Mountains.
    Mountains,
    /// Architecture.
    Architecture,
    /// Natural ground.
    NaturalGround,
    /// Artificial ground.
    ArtificialGround,
}

/// What a mask component selects.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MaskKind {
    /// Main subject (AI).
    Subject {
        /// Model used.
        #[serde(default)]
        model: Option<ModelRef>,
    },
    /// Sky (AI).
    Sky {
        /// Model used.
        #[serde(default)]
        model: Option<ModelRef>,
    },
    /// Everything except the subject (AI).
    Background {
        /// Model used.
        #[serde(default)]
        model: Option<ModelRef>,
    },
    /// A person, or parts of them (AI).
    Person {
        /// Person (face cluster) id.
        person: PersonId,
        /// Parts; empty means the whole body.
        #[serde(default)]
        parts: Vec<PersonPart>,
        /// Model used.
        #[serde(default)]
        model: Option<ModelRef>,
    },
    /// An object selected by prompt, click or box (AI, promptable segmentation).
    Object {
        /// Text prompt, if any.
        #[serde(default)]
        prompt: Option<String>,
        /// Box prompt, if any.
        #[serde(default)]
        region: Option<NormalizedRect>,
        /// Positive click points `[x, y]`.
        #[serde(default)]
        points: Vec<[f32; 2]>,
        /// Model used.
        #[serde(default)]
        model: Option<ModelRef>,
    },
    /// A landscape class (AI).
    Landscape {
        /// Class.
        class: LandscapeClass,
        /// Model used.
        #[serde(default)]
        model: Option<ModelRef>,
    },
    /// A band of estimated depth, normalised `[near, far]` (AI depth).
    Depth {
        /// Range.
        range: [f32; 2],
        /// Feather, `0..=100`.
        #[serde(default)]
        feather: f32,
        /// Model used.
        #[serde(default)]
        model: Option<ModelRef>,
    },
    /// Linear gradient from fully on at `start` to off at `end`.
    Linear {
        /// Start `[x, y]`.
        start: [f32; 2],
        /// End `[x, y]`.
        end: [f32; 2],
    },
    /// Elliptical radial gradient.
    Radial {
        /// Centre `[x, y]`.
        center: [f32; 2],
        /// Radii `[rx, ry]`, normalised to image width/height.
        radii: [f32; 2],
        /// Rotation in degrees.
        #[serde(default)]
        angle: f32,
        /// Feather, `0..=100`.
        #[serde(default)]
        feather: f32,
    },
    /// Painted strokes.
    Brush {
        /// Strokes.
        strokes: Vec<BrushStroke>,
    },
    /// Luminance range.
    LuminanceRange {
        /// `[low, high]` in `0..=1`.
        range: [f32; 2],
        /// Smoothness, `0..=100`.
        #[serde(default)]
        smoothness: f32,
    },
    /// Colour range from sampled colours.
    ColorRange {
        /// Samples in OkLab `[L, a, b]`.
        samples: Vec<[f32; 3]>,
        /// Tolerance, `0..=100`.
        #[serde(default)]
        amount: f32,
    },
}

impl MaskKind {
    /// True for components computed by an ML model.
    pub fn is_ai(&self) -> bool {
        matches!(
            self,
            Self::Subject { .. }
                | Self::Sky { .. }
                | Self::Background { .. }
                | Self::Person { .. }
                | Self::Object { .. }
                | Self::Landscape { .. }
                | Self::Depth { .. }
        )
    }
}

/// One brush stroke.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BrushStroke {
    /// Points `[x, y, pressure]`.
    pub points: Vec<[f32; 3]>,
    /// Radius, normalised to image width.
    pub radius: f32,
    /// Feather, `0..=100`.
    pub feather: f32,
    /// Flow, `0..=100`.
    pub flow: f32,
    /// Erase instead of paint.
    pub erase: bool,
}

impl Default for BrushStroke {
    fn default() -> Self {
        Self {
            points: Vec::new(),
            radius: 0.02,
            feather: 50.0,
            flow: 100.0,
            erase: false,
        }
    }
}

/// How a component combines with the components before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaskCombine {
    /// Union.
    #[default]
    Add,
    /// Difference.
    Subtract,
    /// Intersection.
    Intersect,
}

/// One component of a composite mask.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaskComponent {
    /// What to select.
    #[serde(flatten)]
    pub kind: MaskKind,
    /// Combination with previous components (ignored for the first).
    #[serde(default)]
    pub combine: MaskCombine,
    /// Invert this component before combining.
    #[serde(default)]
    pub invert: bool,
}

impl MaskComponent {
    /// An additive, non-inverted component.
    pub fn new(kind: MaskKind) -> Self {
        Self {
            kind,
            combine: MaskCombine::Add,
            invert: false,
        }
    }
}

/// Parameters a local adjustment applies inside its mask. Same units as the
/// global controls; all default to neutral.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LocalParams {
    /// Exposure, EV.
    pub exposure: f32,
    /// Contrast.
    pub contrast: f32,
    /// Highlights.
    pub highlights: f32,
    /// Shadows.
    pub shadows: f32,
    /// Whites.
    pub whites: f32,
    /// Blacks.
    pub blacks: f32,
    /// Temperature shift.
    pub temperature: f32,
    /// Tint shift.
    pub tint: f32,
    /// Hue shift, degrees.
    pub hue: f32,
    /// Saturation.
    pub saturation: f32,
    /// Texture.
    pub texture: f32,
    /// Clarity.
    pub clarity: f32,
    /// Dehaze.
    pub dehaze: f32,
    /// Sharpness.
    pub sharpness: f32,
    /// Noise reduction.
    pub noise: f32,
    /// Moiré reduction.
    pub moire: f32,
    /// Defringe.
    pub defringe: f32,
    /// Colour overlay `[hue°, saturation]`, if any.
    pub color_overlay: Option<[f32; 2]>,
}

/// A mask plus the adjustment applied through it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LocalAdjustment {
    /// Stable id within the recipe.
    pub id: MaskId,
    /// User-visible name.
    pub name: String,
    /// Disabled adjustments are kept but not rendered.
    pub enabled: bool,
    /// Components, combined in order.
    pub components: Vec<MaskComponent>,
    /// Overall amount, `0..=200` (%), scaling every parameter.
    pub amount: f32,
    /// Invert the composite mask.
    pub invert: bool,
    /// Parameters.
    pub params: LocalParams,
}

impl Default for LocalAdjustment {
    fn default() -> Self {
        Self {
            id: MaskId(0),
            name: String::new(),
            enabled: true,
            components: Vec::new(),
            amount: 100.0,
            invert: false,
            params: LocalParams::default(),
        }
    }
}

/// Kind of retouch operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RetouchKind {
    /// Heal from a source offset.
    Heal {
        /// Source offset `[dx, dy]`, normalised.
        source_offset: [f32; 2],
    },
    /// Clone from a source offset.
    Clone {
        /// Source offset `[dx, dy]`, normalised.
        source_offset: [f32; 2],
    },
    /// Content-aware removal by local (non-generative) inpainting.
    Remove {
        /// Inpainting model used.
        #[serde(default)]
        model: Option<ModelRef>,
    },
    /// Frequency-separation skin smoothing on one person.
    Skin {
        /// Person.
        person: PersonId,
        /// Strength, `0..=100`.
        strength: f32,
    },
}

/// Where a retouch applies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RetouchTarget {
    /// The composite mask of a local adjustment.
    Mask {
        /// Mask id.
        mask: MaskId,
    },
    /// Explicit components (spot circles, brushed areas).
    Area {
        /// Components.
        components: Vec<MaskComponent>,
    },
    /// Whatever the kind implies (e.g. a person for skin retouch).
    Implicit,
}

/// A retouch operation. Pixel results of expensive kinds are cached under
/// `.edits/<image>/pixels/` keyed by `cache_key`; the cache is regenerable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetouchOperation {
    /// Stable id within the recipe.
    pub id: RetouchId,
    /// Kind.
    pub kind: RetouchKind,
    /// Target area.
    pub target: RetouchTarget,
    /// Opacity, `0..=100`.
    #[serde(default = "hundred")]
    pub opacity: f32,
    /// Feather, `0..=100`.
    #[serde(default)]
    pub feather: f32,
    /// Disabled operations are kept but not rendered.
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn hundred() -> f32 {
    100.0
}

fn yes() -> bool {
    true
}
