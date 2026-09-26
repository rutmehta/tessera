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

mod adobe;
pub mod cache;
pub use adobe::AdobeStageOp;
pub use pipeline_adobe;
pub mod graph;
pub mod mask_cache;
pub use mask_cache::MaskRasterCache;
#[cfg(feature = "ml-denoise")]
mod ml_denoise;
pub mod ops;
pub mod render;
mod resample;
/// Opaque backend handles and whole-render transactions.
pub mod resident;
mod source;
#[cfg(feature = "ml-denoise")]
pub use ml_denoise::MlPostDemosaicDenoise;
#[cfg(feature = "ml-denoise")]
mod ml_cfa;
#[cfg(feature = "ml-denoise")]
pub use ml_cfa::MlCfaDenoise;

pub use cache::{CacheStats, TileCache};
pub use graph::{PipelineGraph, StageNode};
pub use ops::{CountingStageOp, CpuStageOp, Op, StageOp};
pub use render::{
    Headroom, PixelRect, ProgressiveRenderJob, RenderOutput, Renderer, RendererConfig, Viewport,
};
pub use source::RawImage;

/// The resident graph does not implement lens correction/auto-calibration.
/// Auto is not inert even when RAW metadata has no embedded lens opcodes.
pub fn resident_export_lens_supported(lens: &engine_api::recipe::settings::LensSettings) -> bool {
    lens.profile == engine_api::recipe::settings::LensProfileSource::None
        && !lens.remove_chromatic_aberration
        && lens.manual_distortion == 0.
        && lens.manual_vignetting == 0.
        && lens.defringe_purple.amount == 0.
        && lens.defringe_green.amount == 0.
        && lens.softness_correction == 0.
}

#[cfg(test)]
extern crate self as image_core;
#[cfg(test)]
mod resident_model;
