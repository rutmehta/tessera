//! Standalone neural filter adapters. No compositor registration.
//!
//! Raster RGB is bounded display-sRGB, with straight alpha. Model errors and
//! cancellation are returned rather than silently replacing a failed edit.
use anyhow::{Result, ensure};
use compositor::{geom::Rect, raster::Raster};
pub use engine_api::jobs::CancellationToken as Cancel;

mod skin;
pub use skin::SkinSmoothing;
mod color;
mod colorize;
pub use colorize::{ColorHint, Colorize};
mod jpeg;
pub use jpeg::{JpegArtifactRemoval, estimate_jpeg_quality};
mod restoration;
pub use restoration::PhotoRestoration;
mod catalog;
pub use catalog::{FilterInfo, catalog};

#[derive(Clone, Debug)]
pub struct Params {
    /// Pixel-coordinate x/y/width/height boxes, e.g. from YuNet. No detection
    /// occurs implicitly: skin smoothing itself requires no weights.
    pub faces: Vec<[f32; 4]>,
    pub blur: f32,
    pub smoothness: f32,
    pub artifact_reduction: f32,
    pub saturation: f32,
    pub hints: Vec<ColorHint>,
    pub strength: f32,
    pub photo_enhancement: f32,
    pub enhance_face: f32,
    pub scratch_reduction: f32,
}
impl Default for Params {
    fn default() -> Self {
        Self {
            faces: vec![],
            blur: 4.0,
            smoothness: 0.5,
            artifact_reduction: 0.0,
            saturation: 1.0,
            hints: vec![],
            strength: 0.5,
            photo_enhancement: 0.5,
            enhance_face: 0.0,
            scratch_reduction: 0.0,
        }
    }
}
impl Params {
    pub fn with_faces(mut self, faces: &[ml_faces::Face]) -> Self {
        self.faces = faces.iter().map(|f| f.bbox).collect();
        self
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ParamSchema {
    pub name: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
}

pub trait NeuralFilter {
    fn name(&self) -> &'static str;
    fn params_schema(&self) -> &'static [ParamSchema];
    fn requires_weights(&self) -> bool;
    fn apply(&self, input: &Raster, params: &Params, cancel: &Cancel) -> Result<Raster>;
}

pub(crate) fn validate(input: &Raster, cancel: &Cancel) -> Result<()> {
    cancel.check()?;
    ensure!(
        (3..=4).contains(&input.channels()),
        "RGB or straight RGBA required"
    );
    ensure!(
        input.extent().width > 0 && input.extent().height > 0,
        "empty raster"
    );
    for y in 0..input.extent().height {
        cancel.check()?;
        for x in 0..input.extent().width {
            ensure!(
                input
                    .pixel(x, y)
                    .iter()
                    .take(input.channels() as usize)
                    .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
                "filters require bounded display-sRGB; convert HDR first"
            );
        }
    }
    Ok(())
}

pub(crate) fn unit(v: f32) -> Result<()> {
    ensure!(
        v.is_finite() && (0.0..=1.0).contains(&v),
        "parameter must be 0..1"
    );
    Ok(())
}

/// Publish tiles only after the whole operation succeeds. Check cancellation
/// between rows and again before returning; input is never modified.
pub(crate) fn render(
    input: &Raster,
    cancel: &Cancel,
    mut f: impl FnMut(u32, u32, &mut [f32; 4]),
) -> Result<Raster> {
    let mut output = input.clone();
    let rev = input
        .max_rev()
        .checked_add(1)
        .ok_or_else(|| anyhow::anyhow!("revision overflow"))?;
    for y in (0..input.extent().height).step_by(256) {
        cancel.check()?;
        output.edit_region(
            Rect::new(
                0,
                y as i64,
                input.extent().width as i64,
                (y as i64 + 256).min(input.extent().height as i64),
            ),
            rev,
            |x, y, p| {
                if !cancel.is_cancelled() {
                    f(x, y, p);
                }
            },
        )?;
    }
    cancel.check()?;
    Ok(output)
}
