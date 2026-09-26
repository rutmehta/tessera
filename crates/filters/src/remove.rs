//! Remove dispatch: local PatchMatch fallback or an explicitly supplied model.
//! No operation here implicitly downloads weights or pretends CPU is neural.
use crate::{
    Buffer,
    caf::{self, FillParams, FillResult},
    checkpoint,
};
use compositor::Raster;
use engine_api::{EngineError, EngineResult};
use ml_runtime::{ModelRegistry, Session, SessionOptions, Tensor};
use std::sync::atomic::AtomicBool;

pub const REMOVE_MODEL_ID: &str = "remove/lama";
pub const REMOVE_VERSION: &str = "c3c0c9e468934d62e79c329e35d82dd09ff8c444";
pub const REMOVE_SHA256: &str = "1faef5301d78db7dda502fe59966957ec4b79dd64e16f03ed96913c7a4eb68d6";
pub const WORKING_EDGE: usize = 512;

pub struct OnnxInpainter {
    session: Session,
    fixed_lama: bool,
}
impl OnnxInpainter {
    /// Adapts an explicitly loaded session. Caller owns provenance verification;
    /// its graph must obey the image/mask -> output contract documented below.
    pub fn from_session(session: Session) -> Self {
        Self {
            session,
            fixed_lama: false,
        }
    }
    /// Load only an already cached, hash-verified artifact. No downloads.
    pub fn load_local(registry: &ModelRegistry, options: SessionOptions) -> EngineResult<Self> {
        let handle = registry
            .resolve_cached_ref(&model_ref())
            .map_err(model_error)?
            .ok_or_else(|| EngineError::not_found("model weights", REMOVE_MODEL_ID))?;
        Self::load_handle(handle, options)
    }
    /// Explicit user-initiated installation/load may download missing weights.
    pub fn load(registry: &ModelRegistry, options: SessionOptions) -> EngineResult<Self> {
        Self::load_handle(
            registry.resolve_ref(&model_ref()).map_err(model_error)?,
            options,
        )
    }
    fn load_handle(handle: ml_runtime::ModelHandle, options: SessionOptions) -> EngineResult<Self> {
        if handle.spec().sha256 != REMOVE_SHA256 {
            return Err(model_error("unexpected LaMa weights"));
        }
        Ok(Self {
            session: Session::load_with_dimensions_and_threads(
                handle.path(),
                options,
                &[("batch", 1)],
                6,
            )
            .map_err(model_error)?,
            fixed_lama: true,
        })
    }
    /// Executed provider assignments, not merely requested EP configuration.
    pub fn partition_report(&mut self) -> EngineResult<ml_runtime::PartitionReport> {
        self.session.partition_report().map_err(model_error)
    }
    pub fn fallback_reason(&self) -> Option<&str> {
        self.session.fallback_reason.as_deref()
    }
}
fn model_ref() -> engine_api::id::ModelRef {
    engine_api::id::ModelRef {
        id: REMOVE_MODEL_ID.into(),
        version: REMOVE_VERSION.into(),
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    Cpu,
    Onnx,
    #[default]
    Auto,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendUsed {
    Identity,
    CpuPatchMatch,
    Onnx,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RemoveParams {
    /// Square/Chebyshev dilation in canvas pixels, 0..=64.
    pub dilation: u32,
    pub backend: Backend,
    pub fill: FillParams,
}
impl Default for RemoveParams {
    fn default() -> Self {
        Self {
            dilation: 2,
            backend: Backend::Auto,
            fill: FillParams::default(),
        }
    }
}
pub struct RemoveResult {
    pub result: FillResult,
    pub backend: BackendUsed,
    pub fallback_reason: Option<String>,
}

/// Object-safe raster removal interface with explicit backend implementations.
pub trait Remove {
    fn apply(
        &mut self,
        input: &Raster,
        mask: &[f32],
        params: &RemoveParams,
        cancel: &AtomicBool,
    ) -> EngineResult<RemoveResult>;
}
#[derive(Default)]
pub struct CpuPatchMatch;
/// Reusable automatic backend: resolves only once, never downloads implicitly.
pub struct AutoRemove {
    model: Option<OnnxInpainter>,
}
impl dyn Remove {
    pub fn auto(registry: &ModelRegistry, options: SessionOptions) -> EngineResult<AutoRemove> {
        let model = match OnnxInpainter::load_local(registry, options) {
            Ok(model) => Some(model),
            Err(EngineError::NotFound { .. }) => None,
            Err(e) => return Err(e),
        };
        Ok(AutoRemove { model })
    }
}
impl Remove for AutoRemove {
    fn apply(
        &mut self,
        input: &Raster,
        mask: &[f32],
        params: &RemoveParams,
        cancel: &AtomicBool,
    ) -> EngineResult<RemoveResult> {
        let model = self.model.as_mut().map(|m| m as &mut dyn InpaintModel);
        remove(input, mask, params, model, cancel)
    }
}
impl Remove for CpuPatchMatch {
    fn apply(
        &mut self,
        input: &Raster,
        mask: &[f32],
        params: &RemoveParams,
        cancel: &AtomicBool,
    ) -> EngineResult<RemoveResult> {
        let mut params = params.clone();
        params.backend = Backend::Cpu;
        remove(input, mask, &params, None, cancel)
    }
}
impl Remove for OnnxInpainter {
    fn apply(
        &mut self,
        input: &Raster,
        mask: &[f32],
        params: &RemoveParams,
        cancel: &AtomicBool,
    ) -> EngineResult<RemoveResult> {
        let mut params = params.clone();
        params.backend = Backend::Onnx;
        remove(input, mask, &params, Some(self), cancel)
    }
}
/// Hook contract: NCHW bounded display-sRGB, masked RGB zeroed, binary mask
/// (1 = remove). Return same-sized display-sRGB. The dispatcher handles linear
/// colour conversion, coverage blending and preserving original alpha.
pub trait InpaintModel {
    fn inpaint(
        &mut self,
        image: &Tensor,
        mask: &Tensor,
        cancel: &AtomicBool,
    ) -> EngineResult<Tensor>;
}
impl InpaintModel for OnnxInpainter {
    fn inpaint(
        &mut self,
        image: &Tensor,
        mask: &Tensor,
        cancel: &AtomicBool,
    ) -> EngineResult<Tensor> {
        checkpoint(cancel)?;
        let [n, c, h, w] = image.shape();
        if n != 1
            || c != 3
            || mask.shape() != [1, 1, h, w]
            || image
                .data()
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
            || mask.data().iter().any(|v| *v != 0.0 && *v != 1.0)
        {
            return Err(model_error(
                "expected NCHW RGB in [0,1] and same-sized binary mask",
            ));
        }
        let ph = h
            .checked_add(7)
            .map(|v| v / 8 * 8)
            .ok_or_else(|| model_error("shape overflow"))?;
        let pw = w
            .checked_add(7)
            .map(|v| v / 8 * 8)
            .ok_or_else(|| model_error("shape overflow"))?;
        let (ph, pw) = if self.fixed_lama {
            if h > WORKING_EDGE || w > WORKING_EDGE {
                return Err(model_error(
                    "LaMa input must be bounded to 512; use Remove::apply",
                ));
            }
            (WORKING_EDGE, WORKING_EDGE)
        } else {
            (ph, pw)
        };
        let area = ph
            .checked_mul(pw)
            .filter(|&n| n <= 16_777_216)
            .ok_or_else(|| EngineError::ResourceExhausted {
                resource: "Remove ONNX full-frame limit is 16M padded pixels".into(),
            })?;
        let pad = |tensor: &Tensor, channels: usize| -> EngineResult<ml_runtime::TensorInput> {
            let mut data = vec![0.0; channels * area];
            for c in 0..channels {
                for y in 0..ph {
                    checkpoint(cancel)?;
                    for x in 0..pw {
                        data[c * area + y * pw + x] =
                            tensor.data()[c * h * w + y.min(h - 1) * w + x.min(w - 1)];
                    }
                }
            }
            Ok(ml_runtime::TensorInput::F32 {
                shape: vec![1, channels, ph, pw],
                data,
            })
        };
        let inputs = [("image", pad(image, 3)?), ("mask", pad(mask, 1)?)];
        checkpoint(cancel)?;
        let output = self.session.run_tensors(&inputs).map_err(model_error)?;
        checkpoint(cancel)?;
        if output.len() != 1
            || output[0].name != "output"
            || output[0].shape != [1, 3, ph, pw]
            || output[0].data.len() != 3 * area
            || output[0].data.iter().any(|v| !v.is_finite())
        {
            return Err(model_error(
                "expected one finite output tensor named output with shape [1,3,H,W]",
            ));
        }
        let mut data = vec![0.0; 3 * h * w];
        for c in 0..3 {
            for y in 0..h {
                data[c * h * w + y * w..c * h * w + (y + 1) * w]
                    .copy_from_slice(&output[0].data[c * area + y * pw..c * area + y * pw + w]);
            }
        }
        if self.fixed_lama {
            for v in &mut data {
                *v /= 255.0;
            }
        }
        Tensor::new(3, h, w, data).map_err(model_error)
    }
}
/// Explicit ONNX requests never silently fall back; Auto falls back only when
/// no model was supplied. Inference failures propagate rather than hiding bugs.
pub fn remove(
    input: &Raster,
    mask: &[f32],
    params: &RemoveParams,
    model: Option<&mut dyn InpaintModel>,
    cancel: &AtomicBool,
) -> EngineResult<RemoveResult> {
    checkpoint(cancel)?;
    caf::validate_mask(mask, input.extent().area() as usize)?;
    if params.dilation > 64 {
        return Err(EngineError::invalid("remove.dilation", "must be 0..64"));
    }
    let mask = dilate(
        mask,
        input.extent().width as usize,
        input.extent().height as usize,
        params.dilation,
        cancel,
    )?;
    if mask.iter().all(|&m| m == 0.0) {
        return Ok(RemoveResult {
            result: caf::fill(input, &mask, &params.fill, cancel)?,
            backend: BackendUsed::Identity,
            fallback_reason: None,
        });
    }
    if params.backend != Backend::Cpu {
        if let Some(model) = model {
            let src = Buffer::read(input, cancel)?;
            if src
                .pixels
                .iter()
                .any(|p| p[..3].iter().any(|v| !(0.0..=1.0).contains(v)))
            {
                return Err(EngineError::invalid(
                    "remove.image",
                    "ONNX adapter requires bounded linear sRGB, not HDR/camera RGB",
                ));
            }
            let (image, tensor_mask) = working_inputs(&src, &mask, cancel)?;
            checkpoint(cancel)?;
            let output = model.inpaint(&image, &tensor_mask, cancel)?;
            checkpoint(cancel)?;
            if output.shape() != image.shape() || output.data().iter().any(|v| !v.is_finite()) {
                return Err(model_error("invalid output shape or nonfinite samples"));
            }
            let mut paint = src.clone();
            for y in 0..src.h {
                checkpoint(cancel)?;
                for x in 0..src.w {
                    // Only covered predictions enter harmonization or the
                    // boundary solve. Leave the exterior in its original
                    // linear representation instead of resampling/decoding
                    // pixels that finish() will never publish.
                    if mask[y * src.w + x] == 0.0 {
                        continue;
                    }
                    for c in 0..3 {
                        paint.pixels[y * src.w + x][c] =
                            decode(sample_output(&output, c, x, y, src.w, src.h).clamp(0.0, 1.0));
                    }
                }
            }
            if !matches!(params.fill.colour_adaptation, caf::ColourAdaptation::None) {
                harmonize(&src, &mut paint, &mask, cancel)?;
            }
            blend_boundary(&src, &mut paint, &mask, cancel)?;
            return Ok(RemoveResult {
                result: caf::finish(
                    input,
                    &src,
                    &paint,
                    &mask,
                    params.fill.output_new_layer,
                    cancel,
                )?,
                backend: BackendUsed::Onnx,
                fallback_reason: None,
            });
        }
        if params.backend == Backend::Onnx {
            return Err(EngineError::not_found("model weights", "remove/lama"));
        }
    }
    Ok(RemoveResult {
        result: caf::fill(input, &mask, &params.fill, cancel)?,
        backend: BackendUsed::CpuPatchMatch,
        fallback_reason: (params.backend == Backend::Auto)
            .then(|| "No inpainting model supplied; CPU PatchMatch used".into()),
    })
}
/// Match local first/second moments before the boundary solve, analogous to
/// PatchMatch colour adaptation. Bounded affine correction retains the model's
/// structure, but avoids dull/shifted fills. No source pixels inside the hole
/// participate. Constant predictions cannot manufacture texture this way.
fn harmonize(
    src: &Buffer,
    paint: &mut Buffer,
    mask: &[f32],
    cancel: &AtomicBool,
) -> EngineResult<()> {
    let ring = dilate(mask, src.w, src.h, 16, cancel)?;
    let mut sums = [[0.0f64; 6]; 2];
    let mut counts = [0.0f64; 2];
    for (i, &m) in mask.iter().enumerate() {
        if i % src.w == 0 {
            checkpoint(cancel)?;
        }
        let (group, pixel) = if m > 0.0 {
            (0, paint.pixels[i])
        } else if ring[i] > 0.0 {
            (1, src.pixels[i])
        } else {
            continue;
        };
        counts[group] += 1.0;
        for c in 0..3 {
            let v = f64::from(pixel[c]);
            sums[group][c] += v;
            sums[group][c + 3] += v * v;
        }
    }
    if counts.iter().any(|&n| n < 16.0) {
        return Ok(());
    }
    let mut mean = [[0.0; 3]; 2];
    let mut sigma = [[0.0; 3]; 2];
    for g in 0..2 {
        for c in 0..3 {
            mean[g][c] = sums[g][c] / counts[g];
            sigma[g][c] = (sums[g][c + 3] / counts[g] - mean[g][c].powi(2))
                .max(0.0)
                .sqrt();
        }
    }
    for (i, &m) in mask.iter().enumerate() {
        if i % src.w == 0 {
            checkpoint(cancel)?;
        }
        if m == 0.0 {
            continue;
        }
        for c in 0..3 {
            let gain = if sigma[0][c] > 1e-4 {
                (sigma[1][c] / sigma[0][c]).clamp(0.5, 2.0)
            } else {
                1.0
            };
            let shift = (mean[1][c] - mean[0][c]).clamp(-0.1, 0.1);
            paint.pixels[i][c] =
                ((f64::from(paint.pixels[i][c]) - mean[0][c]) * gain + mean[0][c] + shift)
                    .clamp(0.0, 1.0) as f32;
        }
    }
    Ok(())
}
/// Solve a discrete gradient-domain problem on an eight-pixel inner band.
/// Exterior pixels are Dirichlet constraints from the untouched source; deep
/// interior is fixed to the model. Preserve model gradients within the hole,
/// use zero normal gradient across its boundary (never the erased object's
/// gradient). 64 deterministic Gauss-Seidel sweeps bound work and cancellation.
fn blend_boundary(
    src: &Buffer,
    paint: &mut Buffer,
    mask: &[f32],
    cancel: &AtomicBool,
) -> EngineResult<()> {
    let mut distance: Vec<u8> = mask.iter().map(|&m| if m > 0.0 { 9 } else { 0 }).collect();
    for y in 0..src.h {
        checkpoint(cancel)?;
        for x in 0..src.w {
            let i = y * src.w + x;
            if x > 0 {
                distance[i] = distance[i].min(distance[i - 1] + 1);
            }
            if y > 0 {
                distance[i] = distance[i].min(distance[i - src.w] + 1);
            }
        }
    }
    for y in (0..src.h).rev() {
        checkpoint(cancel)?;
        for x in (0..src.w).rev() {
            let i = y * src.w + x;
            if x + 1 < src.w {
                distance[i] = distance[i].min(distance[i + 1] + 1);
            }
            if y + 1 < src.h {
                distance[i] = distance[i].min(distance[i + src.w] + 1);
            }
        }
    }
    let mut band = Vec::new();
    for (i, &d) in distance.iter().enumerate() {
        if i % src.w == 0 {
            checkpoint(cancel)?;
        }
        if d == 0 {
            paint.pixels[i] = src.pixels[i];
        } else if d <= 8 {
            let adjacent = neighbours(i, src.w, src.h);
            let mut gradient = [0.0; 3];
            let mut count = 0;
            for &j in &adjacent {
                if j == i {
                    continue;
                }
                count += 1;
                if mask[j] > 0.0 {
                    for (c, g) in gradient.iter_mut().enumerate() {
                        *g += paint.pixels[i][c] - paint.pixels[j][c];
                    }
                }
            }
            band.push((i, adjacent, gradient, count as f32));
        }
    }
    for _ in 0..64 {
        checkpoint(cancel)?;
        for (k, &(i, adjacent, gradient, count)) in band.iter().enumerate() {
            if k % 4096 == 0 {
                checkpoint(cancel)?;
            }
            for (c, &g) in gradient.iter().enumerate() {
                let sum = adjacent
                    .iter()
                    .filter(|&&j| j != i)
                    .map(|&j| paint.pixels[j][c])
                    .sum::<f32>();
                paint.pixels[i][c] = ((sum + g) / count).clamp(0.0, 1.0);
            }
        }
    }
    Ok(())
}
fn neighbours(i: usize, w: usize, h: usize) -> [usize; 4] {
    let (x, y) = (i % w, i / w);
    [
        if x > 0 { i - 1 } else { i },
        if x + 1 < w { i + 1 } else { i },
        if y > 0 { i - w } else { i },
        if y + 1 < h { i + w } else { i },
    ]
}
/// Area pooling for image and conservative max pooling for coverage. Every
/// contributing source pixel is considered, so thin wires cannot disappear.
fn working_inputs(
    src: &Buffer,
    mask: &[f32],
    cancel: &AtomicBool,
) -> EngineResult<(Tensor, Tensor)> {
    let edge = src.w.max(src.h).max(WORKING_EDGE);
    let w = (src.w * WORKING_EDGE / edge).max(1);
    let h = (src.h * WORKING_EDGE / edge).max(1);
    let mut rgb = vec![0.0; 3 * w * h];
    let mut binary = vec![0.0; w * h];
    for y in 0..h {
        checkpoint(cancel)?;
        for x in 0..w {
            let (x0, x1) = (x * src.w / w, ((x + 1) * src.w).div_ceil(w));
            let (y0, y1) = (y * src.h / h, ((y + 1) * src.h).div_ceil(h));
            let mut sum = [0.0; 3];
            let mut hole = false;
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let i = sy * src.w + sx;
                    hole |= mask[i] > 0.0;
                    for (c, channel) in sum.iter_mut().enumerate() {
                        *channel += encode(src.pixels[i][c]);
                    }
                }
            }
            binary[y * w + x] = f32::from(hole);
            if !hole {
                for c in 0..3 {
                    rgb[c * w * h + y * w + x] = sum[c] / ((x1 - x0) * (y1 - y0)) as f32;
                }
            }
        }
    }
    Ok((
        Tensor::new(3, h, w, rgb).map_err(model_error)?,
        Tensor::new(1, h, w, binary).map_err(model_error)?,
    ))
}
fn sample_output(t: &Tensor, c: usize, x: usize, y: usize, w: usize, h: usize) -> f32 {
    let [_, _, th, tw] = t.shape();
    let fx = ((x as f32 + 0.5) * tw as f32 / w as f32 - 0.5).max(0.0);
    let fy = ((y as f32 + 0.5) * th as f32 / h as f32 - 0.5).max(0.0);
    let (x0, y0) = (fx as usize, fy as usize);
    let (x1, y1) = ((x0 + 1).min(tw - 1), (y0 + 1).min(th - 1));
    let (dx, dy) = (fx - x0 as f32, fy - y0 as f32);
    let at = |xx, yy| t.data()[c * tw * th + yy * tw + xx];
    (at(x0, y0) * (1.0 - dx) + at(x1, y0) * dx) * (1.0 - dy)
        + (at(x0, y1) * (1.0 - dx) + at(x1, y1) * dx) * dy
}
fn model_error(e: impl std::fmt::Display) -> EngineError {
    EngineError::Model {
        model: "remove/lama".into(),
        message: e.to_string(),
    }
}
fn encode(v: f32) -> f32 {
    if v <= 0.0031308 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}
fn decode(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}
fn dilate(mask: &[f32], w: usize, h: usize, r: u32, cancel: &AtomicBool) -> EngineResult<Vec<f32>> {
    let r = r as usize;
    let mut horizontal = vec![0.0f32; mask.len()];
    let mut out = horizontal.clone();
    for y in 0..h {
        checkpoint(cancel)?;
        for x in 0..w {
            horizontal[y * w + x] = (x.saturating_sub(r)..=(x + r).min(w - 1))
                .fold(0.0f32, |v, s| v.max(mask[y * w + s]));
        }
    }
    for y in 0..h {
        checkpoint(cancel)?;
        for x in 0..w {
            out[y * w + x] = (y.saturating_sub(r)..=(y + r).min(h - 1))
                .fold(0.0f32, |v, s| v.max(horizontal[s * w + x]));
        }
    }
    Ok(out)
}
