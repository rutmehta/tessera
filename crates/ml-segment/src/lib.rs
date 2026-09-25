//! On-device AI masks.
mod job;
mod model;
pub use job::{PrecomputeJob, PrecomputeKind, Precomputer};
mod refinement;
pub use model::{
    Click, Prompts, SAM_VERSION, SUBJECT_VERSION, Segmenter, cache_key, image_hash, person_box,
    sky_prior,
};
pub use previews::masks::{MaskRaster, MaskStore};
pub use refinement::refine;
