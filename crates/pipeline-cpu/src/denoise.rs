//! RGB denoise is the tail of Demosaic, never the reserved raw Denoise stage.
use crate::Image;
use engine_api::{
    EngineError, EngineResult,
    color::{ColorMatrix3, WorkingSpace},
    recipe::settings::{DenoiseMethod, DenoiseSettings},
};

/// Pinned contract, checked against ml-enhance by image-core's adapter tests.
pub const POST_DENOISE_MODEL_ID: &str = "enhance/drunet-color";
pub const POST_DENOISE_VERSION: &str = "a2b9fccfa27b197f44a3876c567f5e48970c44a7";
pub const POST_DENOISE_ADAPTER: &str = "linear-srgb-v1-sigma25/camera-residual-v1";

/// Injectable full-image inference. Input/output are bounded linear sRGB,
/// never camera RGB or Rec.2020. Implementations own runtime/session policy.
pub trait PostDemosaicDenoise: Send + Sync {
    /// Must change whenever inference semantics change (part of cache identity).
    fn adapter_revision(&self) -> &str;
    fn denoise(&self, bounded_linear_srgb: &Image, amount: f32) -> EngineResult<Image>;
}

pub fn validate_denoise(s: &DenoiseSettings) -> EngineResult<()> {
    if !s.amount.is_finite() || !(0.0..=100.0).contains(&s.amount) || s.chroma_only {
        return Err(EngineError::invalid(
            "denoise",
            "amount must be 0..=100; chroma-only is unsupported",
        ));
    }
    if let DenoiseMethod::Neural {
        model,
        joint_demosaic,
    } = &s.method
        && (*joint_demosaic
            || model.id != POST_DENOISE_MODEL_ID.into()
            || model.version != POST_DENOISE_VERSION)
    {
        return Err(EngineError::invalid(
            "denoise",
            "requires pinned RGB DRUNet; joint CFA denoise/demosaic is unsupported",
        ));
    }
    Ok(())
}

pub fn denoise_active(s: &DenoiseSettings) -> bool {
    !matches!(s.method, DenoiseMethod::Off) && s.amount != 0.0
}

/// Full-image colour barrier. Preserve unbounded scene residual in linear sRGB
/// and add it back before the inverse matrix. Off/zero return without any colour
/// round trip or runtime access. `rgb_to_xyz` describes the input's primaries.
pub fn post_demosaic_denoise(
    input: Image,
    rgb_to_xyz: ColorMatrix3,
    settings: &DenoiseSettings,
    backend: Option<&dyn PostDemosaicDenoise>,
) -> EngineResult<Image> {
    validate_denoise(settings)?;
    if !denoise_active(settings) {
        return Ok(input);
    }
    let backend = backend
        .ok_or_else(|| EngineError::invalid("denoise", "no post-demosaic denoiser injected"))?;
    if input.planes().len() != 3 {
        return Err(EngineError::invalid("denoise", "RGB required"));
    }
    let forward = WorkingSpace::LinearSrgb.to_xyz().inverse()? * rgb_to_xyz;
    let inverse = forward.inverse()?;
    let n = input.width() as usize * input.height() as usize;
    let mut bounded = vec![vec![0.0; n]; 3];
    let mut residual = vec![vec![0.0f64; n]; 3];
    for i in 0..n {
        let rgb = forward.apply(std::array::from_fn(|c| f64::from(input.planes()[c][i])));
        for c in 0..3 {
            bounded[c][i] = rgb[c].clamp(0.0, 1.0) as f32;
            residual[c][i] = rgb[c] - f64::from(bounded[c][i]);
        }
    }
    let bounded = Image::new(input.width(), input.height(), bounded)?;
    let restored = backend.denoise(&bounded, settings.amount)?;
    if restored.width() != input.width()
        || restored.height() != input.height()
        || restored.planes().len() != 3
        || restored
            .planes()
            .iter()
            .flatten()
            .any(|v| !(0.0..=1.0).contains(v))
    {
        return Err(EngineError::invalid(
            "denoise",
            "backend must return same-shape bounded linear sRGB",
        ));
    }
    let mut planes = vec![vec![0.0; n]; 3];
    for i in 0..n {
        let rgb = inverse.apply(std::array::from_fn(|c| {
            f64::from(restored.planes()[c][i]) + residual[c][i]
        }));
        for c in 0..3 {
            planes[c][i] = rgb[c] as f32;
        }
    }
    Image::new(input.width(), input.height(), planes)
}
