use anyhow::{Result, ensure};
use engine_api::id::ModelRef;
use ml_runtime::{ModelRegistry, PartitionReport, Session, SessionOptions, Tensor};

use crate::{SpatialContract, Tiling, run_tiled};

pub const SR_VERSION: &str = "09f741bac80a246b407da3ee902bf5f3291b602f";
pub const SR_X2_SHA256: &str = "7115ba92e8a1bfa63d68558ef006ef3d91273a068d321b1439f8bb1c9179002c";
pub const SR_X4_SHA256: &str = "5c586662929cbc686c1a5c38d9c060dbdb4ea5863a1f7672b8c0761e6b89c033";

/// Pinned BSD-3-Clause Real-ESRGAN RRDBNet. Input is display-encoded RGB
/// in [0,1], not scene-linear camera RGB. Loading is explicitly allowed to
/// download; inference never discovers or silently switches model versions.
pub struct SuperResolution {
    session: Session,
    factor: usize,
}

impl SuperResolution {
    pub fn load(registry: &ModelRegistry, factor: usize, options: SessionOptions) -> Result<Self> {
        let (id, sha) = match factor {
            2 => ("enhance/realesrgan-x2", SR_X2_SHA256),
            4 => ("enhance/realesrgan-x4", SR_X4_SHA256),
            _ => anyhow::bail!("upscale must be 2 or 4"),
        };
        let handle = registry.resolve_ref(&ModelRef {
            id: id.into(),
            version: SR_VERSION.into(),
        })?;
        ensure!(
            handle.spec().sha256 == sha,
            "unexpected Real-ESRGAN weights"
        );
        Ok(Self {
            session: Session::load(handle.path(), options)?,
            factor,
        })
    }

    pub fn factor(&self) -> usize {
        self.factor
    }

    pub fn super_resolution(&mut self, rgb: &Tensor, factor: usize) -> Result<Tensor> {
        ensure!(factor == self.factor, "factor does not match loaded model");
        ensure!(
            rgb.data().iter().all(|v| (0.0..=1.0).contains(v)),
            "SR input must be display RGB in [0,1]"
        );
        // 23 RRDBs * 3 dense blocks * 5 3x3 convolutions, plus a
        // conservative allowance for stem/body/upsampling/output convolutions.
        // x2 uses pixel-unshuffle, doubling the support in input coordinates.
        let alignment = if factor == 2 { 2 } else { 1 };
        let radius = (23 * 3 * 5 + 7) * alignment;
        run_tiled(
            rgb,
            SpatialContract {
                scale: factor,
                radius,
                alignment,
            },
            Tiling {
                tile_size: 128,
                halo: radius,
            },
            |patch| self.session.run(patch),
        )
    }

    /// Executed provider assignments, including CPU fallbacks.
    pub fn partition_report(&mut self) -> Result<PartitionReport> {
        self.session.partition_report()
    }
}
