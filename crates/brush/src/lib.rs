//! Stamp-based brush engine (spec 02 §4) and healing/clone basics (§5).
//!
//! Data flow of one stroke:
//!
//! ```text
//! InputPoint ─► Smoother (pulled string) ─► Symmetry copies ─► Spacer (spacing × size)
//!            ─► Dynamics (seeded jitter / pressure / velocity) ─► Dab
//!            ─► tip coverage × dual brush × texture × wet edges
//!            ─► stroke buffer (alpha-darken: flow builds up, opacity caps)
//!            ─► blend over the base snapshot with a compositor BlendMode
//! ```
//!
//! Each call that adds dabs returns the dirty [`Rect`](compositor::Rect); the
//! caller composites exactly that rect with [`Stroke::apply`] (in place) or
//! [`Stroke::render_tiles`] (tile deltas for a `DocOp::PaintTiles`). Pixels
//! are always recomputed from the stroke-start snapshot, so incremental
//! rendering never double-applies paint.
//!
//! [`gpu::GpuDabRenderer`] rasterizes round and sampled dabs into the stroke
//! buffer in WGSL; it is optional and verified against the CPU path.
#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![allow(clippy::needless_range_loop, clippy::too_many_arguments)]

pub mod abr;
pub mod api;
pub mod dynamics;
pub mod engine;
pub mod gpu;
pub mod heal;
pub mod pixels;
pub mod planner;
pub mod rng;
mod serde_raster;
pub mod sparse;
pub mod stroke;
pub mod symmetry;
pub mod tip;

pub use dynamics::{AngleControl, Control, Dynamics, Jitter};
pub use engine::{Brush, CloneSource, PaintMode, Stroke, composite};
pub use planner::{Dab, Planner};
pub use rng::Rng;
pub use stroke::{InputPoint, Smoothing};
pub use symmetry::Symmetry;
pub use tip::{DualBrush, Pose, SampledTip, Texture, TextureMode, Tip, TipShape};
