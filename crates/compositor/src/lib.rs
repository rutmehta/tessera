//! Layered document model and tiled compositor (spec 02 §1–2, spec 04 §4).
//!
//! - [`DocState`]/[`Layer`]: the document tree — pixel, adjustment, fill,
//!   group (pass-through or isolated), smart object and text-placeholder
//!   layers with opacity, fill, blend mode, Blend If, knockout, masks,
//!   clipping, visibility and locks; selections are float tiled rasters.
//! - [`Raster`]: tiled copy-on-write storage with per-tile revisions.
//! - [`Document`]: ops ([`DocOp`]) with non-linear history and a damage log.
//! - [`Compositor`]: per-tile scene-graph traversal at any pyramid level,
//!   per-layer tile caches keyed by `(layer, stamp, tile)`, dirty-rect
//!   recompositing; CPU reference maths in [`blend`].
//! - [`gpu::GpuCompositor`]: WGSL port of the blend/group/knockout pipeline.
//! - [`format`]: the `.tessera-doc` container.
//!
//! COMPOSITOR.md documents the maths and invariants.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod adjust;
pub mod blend;
pub mod document;
pub mod edit;
pub mod format;
pub mod geom;
pub mod gpu;
pub mod psd;
pub mod raster;
pub mod render;

pub use adjust::{Adjustment, Curve, LevelsChannel};
pub use blend::{BlendIf, BlendIfChannel, BlendMode};
pub use document::{
    ColorProfile, DocState, Fill, GradientKind, GradientStop, GroupMode, Knockout, Layer, LayerId,
    LayerKind, LayerProps, Locks, Mask, SmartFilter, SmartObject, TextLayer, VectorMask,
};
pub use edit::{Applied, DocOp, Document, History, HistoryNode, PaintTarget, TileDelta, paint_op};
pub use geom::{Affine, Rect};
pub use raster::{Depth, Raster};
pub use render::{CompositePyramid, Compositor, CompositorStats};
