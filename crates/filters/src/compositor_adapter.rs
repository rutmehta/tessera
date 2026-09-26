//! Compositor bridge. Blending and shared masks belong to the compositor.
use crate::{Effect, FilterParams, distort::Distortion};
use compositor::{
    document::SmartFilter,
    raster::Raster,
    render::smart_filters::{FilterContext, SmartFilterEvaluator},
};
use engine_api::{EngineError, EngineResult};
use std::sync::atomic::AtomicBool;

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
        #[cfg(feature = "camera-raw-filter")]
        if node.name == "camera_raw" {
            return crate::camera_raw::evaluate(input, &node.params, context);
        }
        let (effect, params) = parse_filter(node, Some(input.extent()))?;
        effect.apply_tiled(input, &params, &AtomicBool::new(false))
    }
}

fn retouch(input: &Raster, node: &SmartFilter) -> EngineResult<Raster> {
    use crate::caf::{self, ColourAdaptation, FillParams, MoveMode};
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
            // A serialized document never authorizes downloading/loading a model.
            Ok(
                crate::remove::remove(input, &p.mask, &p.remove, None, &cancel)?
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
    /// Capability probe without image allocation or device creation.
    pub fn supports(&self, node: &SmartFilter) -> EngineResult<bool> {
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
