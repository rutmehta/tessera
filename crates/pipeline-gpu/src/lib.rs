//! Metal compute implementation of M1 and selected M2 operators. See OPERATORS.md.
#![forbid(unsafe_code)]

mod batch;
mod color;
mod context;
mod curves;
mod detail;
mod effects;
mod geometry;
mod operator;
mod tone_local;
pub use batch::{GpuStageOp, GpuStats};
pub use context::{GpuCapabilities, GpuContext};
