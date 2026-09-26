//! Same-device IOSurface import (implemented in `gpu-core`).
use crate::GpuContext;
use engine_api::EngineResult;

pub use gpu_core::SurfaceFormat;

/// Imports a retained RGBA8 or RGBA16F IOSurface as a writable texture on
/// this context's Metal device. The texture retains the surface storage until
/// GPU use ends.
pub fn write_to_iosurface(
    ctx: &GpuContext,
    id: u32,
) -> EngineResult<(wgpu::Texture, SurfaceFormat)> {
    gpu_core::write_to_iosurface(&ctx.device, id)
}
