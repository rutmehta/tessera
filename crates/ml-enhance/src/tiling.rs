use anyhow::{Context, Result, ensure};
use ml_runtime::Tensor;

/// A model's proven finite spatial support. Do not use this for global
/// attention or global pooling (including stock NAFNet).
#[derive(Clone, Copy, Debug)]
pub struct SpatialContract {
    pub scale: usize,
    /// Conservative receptive radius measured in input pixels.
    pub radius: usize,
    /// Sampling phase period (e.g. 2 for x2 RRDB pixel-unshuffle).
    pub alignment: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct Tiling {
    /// Interior input pixels, not including halo.
    pub tile_size: usize,
    pub halo: usize,
}

/// Run a local model on phase-aligned patches, then copy only their interiors.
/// At the image boundary, only the padding required by alignment is added
/// (edge replication). Other boundaries are defined by the model's own padding.
/// Memory includes the complete input/output plus one patch's activations.
pub fn run_tiled(
    input: &Tensor,
    contract: SpatialContract,
    tiling: Tiling,
    mut infer: impl FnMut(&Tensor) -> Result<Tensor>,
) -> Result<Tensor> {
    let channels = input.shape()[1];
    ensure!(matches!(channels, 3 | 4), "RGB or packed CFA required");
    ensure!(
        input.data().iter().all(|v| v.is_finite()),
        "nonfinite input"
    );
    let SpatialContract {
        scale,
        radius,
        alignment,
    } = contract;
    let Tiling { tile_size, halo } = tiling;
    ensure!(matches!(scale, 1 | 2 | 4), "unsupported spatial scale");
    ensure!(
        alignment > 0
            && tile_size > 0
            && tile_size.is_multiple_of(alignment)
            && halo.is_multiple_of(alignment)
            && halo >= radius,
        "invalid tile size, phase alignment or receptive halo"
    );
    let [_, _, h, w] = input.shape();
    let ph = h.checked_add(alignment - 1).context("height overflow")? / alignment * alignment;
    let pw = w.checked_add(alignment - 1).context("width overflow")? / alignment * alignment;
    let oh = h.checked_mul(scale).context("output height overflow")?;
    let ow = w.checked_mul(scale).context("output width overflow")?;
    let len = oh
        .checked_mul(ow)
        .and_then(|v| v.checked_mul(channels))
        .context("output size overflow")?;
    let mut result = Vec::new();
    result.try_reserve_exact(len)?;
    result.resize(len, 0.0);
    for y in (0..h).step_by(tile_size) {
        for x in (0..w).step_by(tile_size) {
            let end_y = y.saturating_add(tile_size).min(h);
            let end_x = x.saturating_add(tile_size).min(w);
            let y0 = y.saturating_sub(halo);
            let x0 = x.saturating_sub(halo);
            let y1 = y.saturating_add(tile_size).saturating_add(halo).min(ph);
            let x1 = x.saturating_add(tile_size).saturating_add(halo).min(pw);
            let ch = y1 - y0;
            let cw = x1 - x0;
            let patch_len = ch
                .checked_mul(cw)
                .and_then(|v| v.checked_mul(channels))
                .context("patch size overflow")?;
            let mut patch = Vec::new();
            patch.try_reserve_exact(patch_len)?;
            for c in 0..channels {
                for row in y0..y1 {
                    for col in x0..x1 {
                        patch.push(input.data()[c * h * w + row.min(h - 1) * w + col.min(w - 1)]);
                    }
                }
            }
            let output = infer(&Tensor::new(channels, ch, cw, patch)?)?;
            let sh = ch.checked_mul(scale).context("patch height overflow")?;
            let sw = cw.checked_mul(scale).context("patch width overflow")?;
            ensure!(
                output.shape() == [1, channels, sh, sw],
                "model output shape does not match spatial contract"
            );
            ensure!(
                output.data().iter().all(|v| v.is_finite()),
                "nonfinite output"
            );
            for c in 0..channels {
                for row in y * scale..end_y * scale {
                    let src = c * sh * sw + (row - y0 * scale) * sw + (x - x0) * scale;
                    let dst = c * oh * ow + row * ow + x * scale;
                    let count = (end_x - x) * scale;
                    result[dst..dst + count].copy_from_slice(&output.data()[src..src + count]);
                }
            }
        }
    }
    Tensor::new(channels, oh, ow, result)
}
