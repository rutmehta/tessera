//! Caller-owned retouch bridge; implementations may live above the CPU dependency graph.
use engine_api::{EngineError, EngineResult, recipe::mask::RetouchOperation};

/// Render ordered spots into scene-linear planar RGB, before Detail, Tone and local adjustments.
pub trait RetouchRenderer: Send + Sync {
    /// Preserve dimensions and plane lengths; return errors for unsupported operations.
    fn render(
        &self,
        width: u32,
        height: u32,
        planes: &mut [Vec<f32>],
        spots: &[RetouchOperation],
    ) -> EngineResult<()>;
}

impl<F> RetouchRenderer for F
where
    F: Fn(u32, u32, &mut [Vec<f32>], &[RetouchOperation]) -> EngineResult<()> + Send + Sync,
{
    fn render(
        &self,
        width: u32,
        height: u32,
        planes: &mut [Vec<f32>],
        spots: &[RetouchOperation],
    ) -> EngineResult<()> {
        self(width, height, planes, spots)
    }
}

/// Execute a registered renderer, failing closed even for an all-disabled spot list.
pub fn apply_retouch(
    image: crate::Image,
    spots: &[RetouchOperation],
    renderer: Option<&dyn RetouchRenderer>,
) -> EngineResult<crate::Image> {
    if spots.is_empty() {
        return Ok(image);
    }
    let renderer = renderer.ok_or_else(|| {
        EngineError::invalid(
            "retouch",
            "recipe has retouch spots but no renderer is registered",
        )
    })?;
    let (width, height) = (image.width(), image.height());
    let mut planes = image.into_planes();
    renderer.render(width, height, &mut planes, spots)?;
    crate::Image::new(width, height, planes)
}

/// Validate settings with the supplied capability, without executing any pixel work.
pub fn validate_settings_with_retouch(
    settings: &engine_api::recipe::DevelopSettings,
    renderer: Option<&dyn RetouchRenderer>,
) -> EngineResult<()> {
    if !settings.locals.retouch.is_empty() && renderer.is_none() {
        return Err(EngineError::invalid(
            "retouch",
            "recipe has retouch spots but no renderer is registered",
        ));
    }
    let mut checked = settings.clone();
    checked.locals.retouch.clear();
    crate::validate_settings(&checked)
}

impl std::fmt::Debug for dyn RetouchRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RetouchRenderer")
    }
}

/// Solve on target-level pixels and lift only changed cells. Zero deltas never
/// touch the original bits (including signed zero). The box footprint matches
/// Image::downsample_crop, including partial cells at odd image boundaries.
pub(crate) fn apply_retouch_scaled(
    image: crate::Image,
    spots: &[RetouchOperation],
    renderer: Option<&dyn RetouchRenderer>,
    scale: u32,
) -> EngineResult<crate::Image> {
    if scale <= 1 || !spots.iter().any(|op| op.enabled && op.opacity > 0.0) {
        return apply_retouch(image, spots, renderer);
    }
    let (width, height) = (image.width(), image.height());
    let reduced = image.downsample_crop([0, 0, width, height], scale)?;
    let edited = apply_retouch(reduced.clone(), spots, renderer)?;
    let mut planes = image.into_planes();
    for ((out, before), after) in planes.iter_mut().zip(reduced.planes()).zip(edited.planes()) {
        for y in 0..height {
            for x in 0..width {
                let cell = ((y / scale) * reduced.width() + x / scale) as usize;
                if before[cell].to_bits() != after[cell].to_bits() {
                    let delta = after[cell] - before[cell];
                    if delta != 0.0 {
                        out[(y * width + x) as usize] += delta;
                    }
                }
            }
        }
    }
    crate::Image::new(width, height, planes)
}
