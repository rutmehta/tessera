//! Metal compute implementation of the M1 operators. See OPERATORS.md.
#![forbid(unsafe_code)]

mod batch;
mod context;
mod operator;
pub use batch::{GpuStageOp, GpuStats};
pub use context::{GpuCapabilities, GpuContext};
