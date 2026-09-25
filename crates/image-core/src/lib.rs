//! Image core: the raw pipeline graph, stage memoization and the progressive
//! tiled renderer (spec 04 §3, spec 08 §2).
//!
//! - [`graph`]: the fixed stage order, which stages are memoized, and the
//!   earliest dirty stage for a settings change.
//! - [`cache`]: an LRU, byte-budgeted tile cache keyed by
//!   [`engine_api::stage::MemoKey`]. Cached stage outputs are `F16Planar`.
//! - [`ops`]: the [`ops::StageOp`] backend trait. [`ops::CpuStageOp`] calls
//!   the `pipeline-cpu` reference operators; `pipeline-gpu` provides the Metal
//!   implementation with resident operator chains and batched submissions.
//! - [`render`]: [`render::Renderer`], which pulls output tiles through the
//!   graph, reusing memoized upstream tiles, and renders progressively.
//!
//! The [`render`] module documentation describes the tile frames, the cache
//! policy and the exactness guarantees relative to
//! `pipeline_cpu::render_scaled`.

#![forbid(unsafe_code)]

pub mod cache;
pub mod graph;
pub mod ops;
pub mod render;
mod resample;
mod source;

pub use cache::{CacheStats, TileCache};
pub use graph::{PipelineGraph, StageNode};
pub use ops::{CountingStageOp, CpuStageOp, Op, StageOp};
pub use render::{
    PixelRect, ProgressiveRenderJob, RenderOutput, Renderer, RendererConfig, Viewport,
};
pub use source::RawImage;
