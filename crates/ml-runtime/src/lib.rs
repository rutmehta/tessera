//! On-device machine-learning model runtime.
mod registry;
pub use registry::{Dtype, ModelHandle, ModelRegistry, ModelSpec, TensorSpec};
mod session;
mod tensor;
pub use session::{
    ComputeUnits, ModelFormat, NodeAssignment, PartitionReport, Session, SessionOptions,
};
pub use tensor::Tensor;
