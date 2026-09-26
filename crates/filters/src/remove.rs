//! Remove dispatch: local PatchMatch fallback or an explicitly supplied model.
//! No operation here implicitly downloads weights or pretends CPU is neural.
use crate::{
    Buffer,
    caf::{self, FillParams, FillResult},
    checkpoint,
};
use compositor::Raster;
use engine_api::{EngineError, EngineResult};
use ml_runtime::{ModelRegistry, ModelSource, Session, SessionOptions, Tensor};
use std::sync::atomic::AtomicBool;

pub const REMOVE_MODEL_ID: &str = "remove/lama";
pub const REMOVE_VERSION: &str = "local-slot-v1";
/// Registry placeholder, NOT a digest of any actual artifact. Never resolve it.
pub const REMOVE_UNINSTALLED_SHA256: &str =
    "0000000000000000000000000000000000000000000000000000000000000000";

pub struct OnnxInpainter {
    session: Session,
}
impl OnnxInpainter {
    /// Adapts an explicitly loaded session. Caller owns provenance verification;
    /// its graph must obey the image/mask -> output contract documented below.
    pub fn from_session(session: Session) -> Self {
        Self { session }
    }
    /// Only local, hash-verified registry artifacts may be loaded here. No URL
    /// resolution, even for Auto mode. A TODO hash is unavailable, not a model.
    pub fn load_local(registry: &ModelRegistry, options: SessionOptions) -> EngineResult<Self> {
        let spec = registry
            .models()
            .iter()
            .find(|m| m.id == REMOVE_MODEL_ID && m.version == REMOVE_VERSION)
            .ok_or_else(|| EngineError::not_found("model weights", REMOVE_MODEL_ID))?;
        if spec.sha256 == REMOVE_UNINSTALLED_SHA256 {
            return Err(EngineError::not_found(
                "model weights",
                format!("{REMOVE_MODEL_ID}: TODO verified local weights/hash"),
            ));
        }
        if spec.source != ModelSource::Local {
            return Err(model_error(
                "Remove only accepts local registry weights; downloads are disabled",
            ));
        }
        let handle = registry
            .resolve_ref(&engine_api::id::ModelRef {
                id: REMOVE_MODEL_ID.into(),
                version: REMOVE_VERSION.into(),
            })
            .map_err(model_error)?;
        Ok(Self {
            session: Session::load(handle.path(), options).map_err(model_error)?,
        })
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
            let binary: Vec<f32> = mask
                .iter()
                .map(|&v| if v > 0.0 { 1.0 } else { 0.0 })
                .collect();
            let data: Vec<f32> = (0..3)
                .flat_map(|c| {
                    src.pixels.iter().enumerate().map({
                        let binary = &binary;
                        move |(i, p)| if binary[i] > 0.0 { 0.0 } else { encode(p[c]) }
                    })
                })
                .collect();
            let image = Tensor::new(3, src.h, src.w, data).map_err(model_error)?;
            let tensor_mask = Tensor::new(1, src.h, src.w, binary).map_err(model_error)?;
            checkpoint(cancel)?;
            let output = model.inpaint(&image, &tensor_mask, cancel)?;
            checkpoint(cancel)?;
            if output.shape() != image.shape() || output.data().iter().any(|v| !v.is_finite()) {
                return Err(model_error("invalid output shape or nonfinite samples"));
            }
            let mut paint = src.clone();
            for i in 0..mask.len() {
                for c in 0..3 {
                    paint.pixels[i][c] = decode(output.data()[c * mask.len() + i].clamp(0.0, 1.0));
                }
            }
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
