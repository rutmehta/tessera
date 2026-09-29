//! Compositor bridge. Blending and shared masks belong to the compositor.
use crate::{Effect, FilterParams, distort::Distortion};
use compositor::{
    document::SmartFilter,
    raster::Raster,
    render::smart_filters::{FilterContext, SmartFilterEvaluator},
};
use engine_api::{EngineError, EngineResult, jobs::CancellationToken};
use std::sync::atomic::AtomicBool;
use std::sync::{Mutex, OnceLock};

// Explicit host installation only. Immutable model slots prevent replacement
// underneath compositor caches. Model sessions serialize their own inference.
static COLORIZE: OnceLock<ml_filters::Colorize> = OnceLock::new();
static JPEG: OnceLock<ml_filters::JpegArtifactRemoval> = OnceLock::new();
static RESTORATION: OnceLock<ml_filters::PhotoRestoration> = OnceLock::new();
static REMOVE: OnceLock<Mutex<crate::remove::OnnxInpainter>> = OnceLock::new();

fn missing_weights(name: &str) -> EngineError {
    EngineError::Unsupported {
        what: format!(
            "{name}: model weights are not loaded; document evaluation never downloads weights"
        ),
    }
}

/// Stateless CPU evaluator installed with `Compositor::set_filter_evaluator`.
/// Names are snake_case Effect names, plus `gaussian_blur` for `gaussian`.
/// Parameters are a strict JSON object of FilterParams fields; omitted amount
/// is 1 (unlike the standalone FilterParams default). Adjustments use externally
/// tagged snake_case variants. The compositor owns enabled/blend/mask handling.
#[derive(Clone, Copy, Debug, Default)]
pub struct CompositorFilters;
impl SmartFilterEvaluator for CompositorFilters {
    fn evaluate(
        &self,
        input: &Raster,
        node: &SmartFilter,
        context: &FilterContext,
    ) -> EngineResult<Raster> {
        if node.name.starts_with("neural/") {
            return neural(input, node);
        }
        #[cfg(not(feature = "camera-raw-filter"))]
        let _ = context;
        if matches!(
            node.name.as_str(),
            "content_aware_fill" | "content_aware_move" | "content_aware_extend" | "remove"
        ) {
            return retouch(input, node);
        }
        if node.name == "liquify" {
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Params {
                mesh: crate::liquify::Mesh,
                #[serde(default)]
                interpolation: crate::liquify::Interpolation,
            }
            let p: Params = serde_json::from_value(node.params.clone())
                .map_err(|e| EngineError::invalid("liquify", e.to_string()))?;
            return p
                .mesh
                .render(input, p.interpolation, &AtomicBool::new(false));
        }
        if node.name == "adaptive_wide_angle" {
            return adaptive_wide_angle(input, &node.params, &CancellationToken::new());
        }
        #[cfg(feature = "camera-raw-filter")]
        if node.name == "camera_raw" {
            return crate::camera_raw::evaluate(input, &node.params, context);
        }
        let (effect, params) = parse_filter(node, Some(input.extent()))?;
        effect.apply_tiled(input, &params, &AtomicBool::new(false))
    }

    /// B5-20b: Adaptive Wide Angle checks `cancel` inside its render; every
    /// other stage keeps the trait's boundary checks.
    fn evaluate_with_cancel(
        &self,
        input: &Raster,
        node: &SmartFilter,
        context: &FilterContext,
        cancel: &CancellationToken,
    ) -> EngineResult<Raster> {
        cancel.check()?;
        let result = if node.name == "adaptive_wide_angle" {
            adaptive_wide_angle(input, &node.params, cancel)?
        } else {
            self.evaluate(input, node, context)?
        };
        cancel.check()?;
        Ok(result)
    }
}

// B5-20 begin: Adaptive Wide Angle smart filter.

/// First key of `value` that `reference` (its typed re-serialization) lacks:
/// strict params at every depth without changing the transform crate.
fn unknown_key(value: &serde_json::Value, reference: &serde_json::Value) -> Option<String> {
    use serde_json::Value::{Array, Object};
    match (value, reference) {
        (Object(v), Object(r)) => v.iter().find_map(|(k, x)| match r.get(k) {
            None => Some(k.clone()),
            Some(y) => unknown_key(x, y),
        }),
        (Array(v), Array(r)) => v.iter().zip(r).find_map(|(x, y)| unknown_key(x, y)),
        _ => None,
    }
}

/// `adaptive_wide_angle`: params are a `transform::adaptive::Adaptive` recipe
/// whose source and output match the stage input. Solved per evaluation (the
/// compositor caches stage results) and rendered with the shared CPU
/// displacement renderer on premultiplied planes, like `transform` stages.
/// B5-20b: layers over the dense lattice solve on a coarse lattice and sample
/// at full resolution (`crate::adaptive_lattice`); layers within it keep the
/// dense path, unchanged.
fn adaptive_wide_angle(
    input: &Raster,
    params: &serde_json::Value,
    cancel: &CancellationToken,
) -> EngineResult<Raster> {
    use compositor::{geom::Rect, raster::Depth};
    let bad = |m: String| EngineError::invalid("adaptive_wide_angle", m);
    let recipe: transform::adaptive::Adaptive =
        serde_json::from_value(params.clone()).map_err(|e| bad(e.to_string()))?;
    let typed = serde_json::to_value(&recipe).map_err(|e| bad(e.to_string()))?;
    if let Some(k) = unknown_key(params, &typed) {
        return Err(bad(format!("unknown field `{k}`")));
    }
    let e = input.extent();
    let (w, h) = (e.width as usize, e.height as usize);
    if let Some(why) = crate::adaptive_lattice::size_refusal(w, h) {
        return Err(bad(why));
    }
    if [recipe.source_width, recipe.source_height] != [w, h]
        || [recipe.output_width, recipe.output_height] != [w, h]
    {
        return Err(bad(format!(
            "recipe size {}×{} → {}×{} does not match the {w}×{h} layer",
            recipe.source_width, recipe.source_height, recipe.output_width, recipe.output_height
        )));
    }
    if let Some(budget) = crate::adaptive_lattice::coarse_budget(w, h) {
        return crate::adaptive_lattice::evaluate(input, &recipe, budget, cancel);
    }
    cancel.check()?;
    let field = recipe.solve().map_err(|e| bad(e.to_string()))?;
    let mut planes: [Vec<f32>; 4] = std::array::from_fn(|_| Vec::with_capacity(w * h));
    for y in 0..e.height {
        for x in 0..e.width {
            let p = input.pixel(x, y);
            for (c, plane) in planes.iter_mut().enumerate() {
                plane.push(if c == 3 { p[3] } else { p[c] * p[3] });
            }
        }
    }
    let image = transform::Image::new(w, h, planes).map_err(|e| bad(e.to_string()))?;
    let op = transform::TransformOp {
        version: 1,
        operation: transform::Operation::Displacement(field),
        kernel: transform::Kernel::Automatic,
    };
    let out = op
        .apply_with_cancel(&image, w, h, 0, cancel)
        .map_err(|e| match e {
            transform::Error::Cancelled => EngineError::Cancelled,
            e => bad(e.to_string()),
        })?;
    let mut raster = Raster::new(e, 4, Depth::F32, 0.0);
    raster.edit_region(Rect::of_extent(e), 1, |x, y, p| {
        let i = y as usize * w + x as usize;
        let a = out.planes[3][i];
        for (c, v) in p.iter_mut().enumerate().take(3) {
            *v = if a > 0.0 { out.planes[c][i] / a } else { 0.0 };
        }
        p[3] = a;
    })?;
    Ok(raster)
}
// B5-20 end

/// Metadata for the neural menu; this never loads models or accesses the network.
pub fn neural_catalog() -> Vec<ml_filters::FilterInfo> {
    ml_filters::catalog()
}

fn neural_params(node: &SmartFilter) -> EngineResult<ml_filters::Params> {
    let catalog = neural_catalog();
    let index = match node.name.as_str() {
        "neural/skin_smoothing" => 0,
        "neural/colorize" => 1,
        "neural/jpeg_artifact_removal" => 2,
        "neural/photo_restoration" => 3,
        _ => {
            return Err(EngineError::Unsupported {
                what: node.name.clone(),
            });
        }
    };
    let object = node
        .params
        .as_object()
        .ok_or_else(|| EngineError::invalid("neural params", "expected object"))?;
    let mut p = ml_filters::Params::default();
    for (key, value) in object {
        if index == 0 && key == "faces" {
            p.faces = serde_json::from_value(value.clone())
                .map_err(|e| EngineError::invalid("faces", e.to_string()))?;
            if p.faces
                .iter()
                .any(|b| b.iter().any(|v| !v.is_finite()) || b[2] <= 0.0 || b[3] <= 0.0)
            {
                return Err(EngineError::invalid(
                    "faces",
                    "finite positive boxes required",
                ));
            }
            continue;
        }
        let schema = catalog[index]
            .params
            .iter()
            .find(|s| s.name.to_lowercase().replace(' ', "_") == *key)
            .ok_or_else(|| EngineError::invalid("neural params", format!("unknown field {key}")))?;
        let value = value
            .as_f64()
            .filter(|v| v.is_finite() && *v >= schema.min as f64 && *v <= schema.max as f64)
            .ok_or_else(|| {
                EngineError::invalid(key, format!("must be {}..{}", schema.min, schema.max))
            })? as f32;
        match key.as_str() {
            "blur" => p.blur = value,
            "smoothness" => p.smoothness = value,
            "artifact_reduction" => p.artifact_reduction = value,
            "saturation" => p.saturation = value,
            "strength" => p.strength = value,
            "photo_enhancement" => p.photo_enhancement = value,
            _ => unreachable!(),
        }
    }
    if index == 0 && p.faces.is_empty() {
        return Err(EngineError::invalid(
            "faces",
            "skin smoothing needs explicit face boxes; no implicit detector",
        ));
    }
    Ok(p)
}

fn neural(input: &Raster, node: &SmartFilter) -> EngineResult<Raster> {
    use ml_filters::NeuralFilter;
    let params = neural_params(node)?;
    let filter: &dyn NeuralFilter = match node.name.as_str() {
        "neural/skin_smoothing" => &ml_filters::SkinSmoothing,
        "neural/colorize" => COLORIZE.get().ok_or_else(|| missing_weights(&node.name))?,
        "neural/jpeg_artifact_removal" => JPEG.get().ok_or_else(|| missing_weights(&node.name))?,
        "neural/photo_restoration" => RESTORATION
            .get()
            .ok_or_else(|| missing_weights(&node.name))?,
        _ => {
            return Err(EngineError::Unsupported {
                what: node.name.clone(),
            });
        }
    };
    filter
        .apply(input, &params, &ml_filters::Cancel::new())
        .map_err(|e| EngineError::invalid("neural filter", e.to_string()))
}

/// Detect explicitly selected geometric distraction proxies, and freeze the
/// resulting masks in a replayable Remove node. Not semantic segmentation.
pub fn detect_distractions(
    input: &Raster,
    params: &serde_json::Value,
) -> EngineResult<(SmartFilter, serde_json::Value)> {
    use crate::distraction::DistractionDetector;
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Params {
        #[serde(default)]
        faces: Vec<[f32; 4]>,
        #[serde(default = "yes")]
        wires: bool,
        #[serde(default = "yes")]
        people: bool,
        #[serde(default)]
        remove: crate::remove::RemoveParams,
    }
    fn yes() -> bool {
        true
    }
    let p: Params = serde_json::from_value(params.clone())
        .map_err(|e| EngineError::invalid("distractions", e.to_string()))?;
    let faces = p
        .faces
        .into_iter()
        .map(|bbox| ml_faces::Face {
            bbox,
            score: 1.0,
            landmarks5: [[0.0; 2]; 5],
        })
        .collect::<Vec<_>>();
    let mut masks = crate::distraction::CpuDistractionDetector.detect(
        input,
        &faces,
        &AtomicBool::new(false),
    )?;
    if !p.wires {
        masks.wires.fill(0.0);
    }
    if !p.people {
        masks.people.fill(0.0);
    }
    let mask = masks.union(input.extent().area() as usize)?;
    let report = serde_json::json!({"wires": masks.wires, "people": masks.people, "mask": mask,
        "mask_space": "source pixels before removal dilation and selection/shared-mask clipping",
        "dilation": p.remove.dilation,
        "backend_requested": p.remove.backend,
        "limitation": "geometric wire and dilated face-box proxies, not semantic segmentation"});
    Ok((
        SmartFilter {
            name: "remove".into(),
            params: serde_json::json!({"mask":mask,"remove":p.remove}),
            ..Default::default()
        },
        report,
    ))
}

fn retouch(input: &Raster, node: &SmartFilter) -> EngineResult<Raster> {
    use crate::caf::{self, ColourAdaptation, FillParams, MoveMode};
    fn composite_only(fill: &FillParams) -> EngineResult<()> {
        if fill.output_new_layer {
            return Err(EngineError::Unsupported { what: "output_new_layer is not supported by single-raster document filters; duplicate the layer first".into() });
        }
        Ok(())
    }
    let cancel = AtomicBool::new(false);
    let decode = |e: serde_json::Error| EngineError::invalid("retouch params", e.to_string());
    match node.name.as_str() {
        "content_aware_fill" => {
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Params {
                mask: Vec<f32>,
                #[serde(default)]
                fill: FillParams,
            }
            let p: Params = serde_json::from_value(node.params.clone()).map_err(decode)?;
            composite_only(&p.fill)?;
            Ok(caf::fill(input, &p.mask, &p.fill, &cancel)?.composite)
        }
        "remove" => {
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Params {
                mask: Vec<f32>,
                #[serde(default)]
                remove: crate::remove::RemoveParams,
            }
            let p: Params = serde_json::from_value(node.params.clone()).map_err(decode)?;
            composite_only(&p.remove.fill)?;
            // A serialized document never authorizes installing a model. Only
            // an explicitly installed session is considered for Auto/Onnx.
            let mut loaded = if p.remove.backend == crate::remove::Backend::Cpu {
                None
            } else {
                REMOVE
                    .get()
                    .map(|m| {
                        m.lock()
                            .map_err(|_| EngineError::internal("remove model lock poisoned"))
                    })
                    .transpose()?
            };
            let model = loaded
                .as_deref_mut()
                .map(|m| m as &mut dyn crate::remove::InpaintModel);
            Ok(
                crate::remove::remove(input, &p.mask, &p.remove, model, &cancel)?
                    .result
                    .composite,
            )
        }
        _ => {
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Params {
                mask: Vec<f32>,
                offset: [i32; 2],
                #[serde(default)]
                fill: FillParams,
                #[serde(default)]
                seam: ColourAdaptation,
            }
            let p: Params = serde_json::from_value(node.params.clone()).map_err(decode)?;
            composite_only(&p.fill)?;
            let mode = if node.name == "content_aware_move" {
                MoveMode::Move
            } else {
                MoveMode::Extend
            };
            Ok(
                caf::move_or_extend(input, &p.mask, p.offset, mode, &p.fill, p.seam, &cancel)?
                    .composite,
            )
        }
    }
}

impl CompositorFilters {
    /// Explicit host opt-in to install weights, possibly downloading them using
    /// the supplied registry. Never called by evaluation, capability probes or
    /// document parsing. Sessions are pinned for the lifetime of this process.
    /// CPU execution is mandatory here; no resident GPU filter is advertised.
    pub fn load_model(
        name: &str,
        registry: Option<&ml_runtime::ModelRegistry>,
    ) -> EngineResult<()> {
        if !matches!(
            name,
            "remove"
                | "neural/colorize"
                | "neural/jpeg_artifact_removal"
                | "neural/photo_restoration"
        ) {
            return Err(EngineError::invalid("model", "unknown model-backed filter"));
        }
        let registry = registry.ok_or_else(|| missing_weights(name))?;
        let options = ml_runtime::SessionOptions::default()
            .with_execution_preference(ml_runtime::ExecutionPreference::CpuOnly);

        match name {
            "neural/colorize" if COLORIZE.get().is_none() => {
                let model = ml_filters::Colorize::load(registry, options).map_err(|e| {
                    EngineError::Unsupported {
                        what: format!("{name} weights/model unavailable: {e}"),
                    }
                })?;
                let _ = COLORIZE.set(model);
            }
            "neural/jpeg_artifact_removal" if JPEG.get().is_none() => {
                let model =
                    ml_filters::JpegArtifactRemoval::load(registry, options).map_err(|e| {
                        EngineError::Unsupported {
                            what: format!("{name} weights/model unavailable: {e}"),
                        }
                    })?;
                let _ = JPEG.set(model);
            }
            "neural/photo_restoration" if RESTORATION.get().is_none() => {
                let model = ml_filters::PhotoRestoration::load(registry, options).map_err(|e| {
                    EngineError::Unsupported {
                        what: format!("{name} weights/model unavailable: {e}"),
                    }
                })?;
                let _ = RESTORATION.set(model);
            }
            "remove" if REMOVE.get().is_none() => {
                let model = crate::remove::OnnxInpainter::load(registry, options)?;
                let _ = REMOVE.set(Mutex::new(model));
            }
            _ => {}
        }
        Ok(())
    }

    /// Capability probe without image allocation or device creation.
    pub fn supports(&self, node: &SmartFilter) -> EngineResult<bool> {
        if node.name.starts_with("neural/") {
            neural_params(node)?;
            return Ok(false);
        }
        resident_supports(node)
    }

    /// Legacy context-free entry point for filters that do not interpret colour.
    /// Camera Raw requires the context-aware evaluator trait instead.
    pub fn evaluate_resident(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        input: &wgpu::Buffer,
        extent: engine_api::tile::Extent,
        node: &SmartFilter,
    ) -> EngineResult<wgpu::Buffer> {
        if node.name == "camera_raw" {
            return Err(EngineError::invalid("camera_raw", "FilterContext required"));
        }
        self.evaluate_resident_with_context(
            device,
            queue,
            input,
            extent,
            node,
            &FilterContext {
                profile: None,
                level: 0,
                canvas: extent,
            },
        )
    }

    /// Evaluate on the compositor's device without crossing the pixel residency boundary.
    #[allow(clippy::too_many_arguments)]
    pub fn evaluate_resident_with_context(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        input: &wgpu::Buffer,
        extent: engine_api::tile::Extent,
        node: &SmartFilter,
        context: &FilterContext,
    ) -> EngineResult<wgpu::Buffer> {
        #[cfg(not(feature = "camera-raw-filter"))]
        let _ = context;
        #[cfg(feature = "camera-raw-filter")]
        if node.name == "camera_raw" {
            return crate::camera_raw_gpu::evaluate(
                device,
                queue,
                input,
                extent,
                &node.params,
                context,
            );
        }
        let (effect, params) = parse_filter(node, Some(extent))?;
        if !resident_supports(node)? {
            return Err(EngineError::Unsupported {
                what: format!("resident smart filter {}", node.name),
            });
        }
        crate::gpu::GpuFilters::from_device(device, queue)?.apply_buffer(
            effect,
            input,
            extent,
            &params,
            &AtomicBool::new(false),
        )
    }
}

impl compositor::render::smart_filters::ResidentFilterEvaluator for CompositorFilters {
    fn supports(&self, filter: &SmartFilter) -> EngineResult<bool> {
        CompositorFilters::supports(self, filter)
    }

    fn evaluate_resident(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        input: &wgpu::Buffer,
        extent: engine_api::tile::Extent,
        filter: &SmartFilter,
        context: &FilterContext,
    ) -> EngineResult<wgpu::Buffer> {
        CompositorFilters::evaluate_resident_with_context(
            self, device, queue, input, extent, filter, context,
        )
    }
}

// Keep CPU and resident decoding/validation identical; capability probes do not
// know the image extent, so size-dependent checks run during evaluation.
fn parse_filter(
    node: &SmartFilter,
    extent: Option<engine_api::tile::Extent>,
) -> EngineResult<(Effect, FilterParams)> {
    let effect = match node.name.as_str() {
        "gaussian" | "gaussian_blur" => Effect::Gaussian,
        "box" => Effect::Box,
        "motion" => Effect::Motion,
        "radial_spin" => Effect::RadialSpin,
        "radial_zoom" => Effect::RadialZoom,
        "lens_blur" => Effect::LensBlur,
        "surface_blur" => Effect::SurfaceBlur,
        "unsharp_mask" => Effect::UnsharpMask,
        "smart_sharpen" => Effect::SmartSharpen,
        "high_pass" => Effect::HighPass,
        "add_noise" => Effect::AddNoise,
        "reduce_noise" => Effect::ReduceNoise,
        "median" => Effect::Median,
        "dust_scratches" => Effect::DustScratches,
        "emboss" => Effect::Emboss,
        "find_edges" => Effect::FindEdges,
        "solarize" => Effect::Solarize,
        "oil_paint" => Effect::OilPaint,
        "clouds" => Effect::Clouds,
        "difference_clouds" => Effect::DifferenceClouds,
        "lens_flare" => Effect::LensFlare,
        "adjust" => Effect::Adjust,
        "pinch" => Effect::Distort(Distortion::Pinch),
        "spherize" => Effect::Distort(Distortion::Spherize),
        "twirl" => Effect::Distort(Distortion::Twirl),
        "wave" => Effect::Distort(Distortion::Wave),
        "ripple" => Effect::Distort(Distortion::Ripple),
        "polar_to_rectangular" => Effect::Distort(Distortion::PolarToRectangular),
        "rectangular_to_polar" => Effect::Distort(Distortion::RectangularToPolar),
        "offset" => Effect::Distort(Distortion::Offset),
        _ => {
            return Err(EngineError::Unsupported {
                what: format!("smart filter {}", node.name),
            });
        }
    };
    let object = node
        .params
        .as_object()
        .ok_or_else(|| EngineError::invalid("filter params", "object required"))?;
    let mut params: FilterParams = serde_json::from_value(node.params.clone())
        .map_err(|e| EngineError::invalid("filter params", e.to_string()))?;
    if !object.contains_key("amount") {
        params.amount = 1.0;
    }
    crate::validate(&params)?;
    params.adjust.validate()?;
    if params.focus.iter().any(|v| !v.is_finite())
        || params.depth.as_ref().is_some_and(|d| {
            extent.is_some_and(|e| d.len() != e.area() as usize)
                || d.iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        })
    {
        return Err(EngineError::invalid("filter params", "invalid focus/depth"));
    }
    // Validate nested geometry even for zero-opacity or unrelated effects;
    // invalid supplied controls must never silently become an identity.
    let kind = if let Effect::Distort(kind) = effect {
        kind
    } else {
        Distortion::Twirl
    };
    crate::distort::apply(
        kind,
        &params.distort,
        &[[0.0; 4]; 4],
        2,
        2,
        &AtomicBool::new(false),
    )?;
    Ok((effect, params))
}

fn resident_supports(node: &SmartFilter) -> EngineResult<bool> {
    #[cfg(feature = "camera-raw-filter")]
    if node.name == "camera_raw" {
        return crate::camera_raw_gpu::supports(&node.params);
    }
    let (effect, params) = match parse_filter(node, None) {
        Ok(decoded) => decoded,
        Err(EngineError::Unsupported { .. }) => return Ok(false),
        Err(error) => return Err(error),
    };
    // Lens depth is caller-supplied parameter data, not source-image pixels.
    if effect == Effect::LensBlur && (params.depth.is_none() || params.radius > 128.0) {
        return Ok(false);
    }
    Ok(crate::gpu::GpuFilters::supports_resident(effect, &params))
}

#[cfg(test)]
mod resident_tests {
    use super::*;

    #[test]
    fn capability_is_conservative_and_params_are_strict() {
        let mut node = SmartFilter {
            name: "gaussian_blur".into(),
            params: serde_json::json!({}),
            ..Default::default()
        };
        assert!(resident_supports(&node).unwrap());
        node.name = "adjust".into();
        node.params = serde_json::json!({"adjust": {"match_colour": {"target": [[0.2, 0.3, 0.4]], "amount": 1.0}}});
        assert!(!resident_supports(&node).unwrap());
        for name in ["smart_sharpen", "median", "transform", "unknown"] {
            node.name = name.into();
            node.params = serde_json::json!({});
            assert!(!resident_supports(&node).unwrap());
        }
        node.name = "gaussian".into();
        node.params = serde_json::json!({"radius": -1});
        assert!(resident_supports(&node).is_err());
        node.params = serde_json::json!({"typo": 1});
        assert!(resident_supports(&node).is_err());
    }
}
