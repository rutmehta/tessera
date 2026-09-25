//! Metal compute implementation of M1 and selected M2 operators. See OPERATORS.md.
#![deny(unsafe_code)]

mod batch;
mod iosurface;
mod resident;
pub use iosurface::write_to_iosurface;
mod color;
mod context;
mod curves;
mod detail;
mod effects;
mod geometry;
mod locals;
mod operator;
mod output_lut;
pub use output_lut::GpuOutputLut;
mod managed_output;
pub use managed_output::{GpuManagedOutput, ManagedRenderer, ManagedTile};
mod tone_local;
pub use batch::{GpuStageOp, GpuStats};
pub use context::{GpuCapabilities, GpuContext};
