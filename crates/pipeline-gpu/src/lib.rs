//! Metal compute implementation of M1 and selected M2 operators. See OPERATORS.md.
#![deny(unsafe_code)]

mod batch;
mod iosurface;
mod resident;
pub use iosurface::{SurfaceFormat, write_to_iosurface};
pub use resident::export_resize::ExportResize;
pub use resident::{EXPORT_MAX_YIELD, EXPORT_QUIET};
mod color;
mod context;
mod curves;
mod detail;
mod effects;
mod fused;
mod geometry;
mod locals;
mod operator;
mod output_lut;
pub use output_lut::GpuOutputLut;
mod managed_output;
pub use managed_output::{GpuManagedOutput, ManagedRenderer, ManagedTile};
mod tone_local;
#[cfg(test)]
mod tone_local_compute_tests;
pub use batch::{GpuStageOp, GpuStats};
pub use context::{GpuCapabilities, GpuContext};
