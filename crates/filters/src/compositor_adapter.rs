//! Compositor bridge. Blending and shared masks belong to the compositor.
use crate::{Effect, FilterParams, distort::Distortion};
use compositor::{
    document::SmartFilter, raster::Raster, render::smart_filters::SmartFilterEvaluator,
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
    fn evaluate(&self, input: &Raster, node: &SmartFilter) -> EngineResult<Raster> {
        #[cfg(feature = "camera-raw-filter")]
        if node.name == "camera_raw" {
            return camera_raw(input, &node.params);
        }
        let (effect, params) = parse_filter(node, Some(input.extent()))?;
        effect.apply_tiled(input, &params, &AtomicBool::new(false))
    }
}

impl CompositorFilters {
    /// Capability probe without image allocation or device creation.
    pub fn supports(&self, node: &SmartFilter) -> EngineResult<bool> {
        resident_supports(node)
    }

    /// Evaluate on the compositor's device without crossing the pixel residency boundary.
    pub fn evaluate_resident(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        input: &wgpu::Buffer,
        extent: engine_api::tile::Extent,
        node: &SmartFilter,
    ) -> EngineResult<wgpu::Buffer> {
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
    ) -> EngineResult<wgpu::Buffer> {
        CompositorFilters::evaluate_resident(self, device, queue, input, extent, filter)
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
        for name in [
            "smart_sharpen",
            "median",
            "camera_raw",
            "transform",
            "unknown",
        ] {
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

/// Deliberately tone-only raster stub, not a RAW decoder/develop pipeline.
#[cfg(feature = "camera-raw-filter")]
fn camera_raw(input: &Raster, value: &serde_json::Value) -> EngineResult<Raster> {
    use compositor::geom::Rect;
    use engine_api::{
        recipe::settings::ToneSettings,
        tile::{Tile, TileCoord, TileLayout},
    };
    let object = value
        .as_object()
        .ok_or_else(|| EngineError::invalid("camera_raw", "object required"))?;
    let mut settings = ToneSettings::default();
    let mut amount = 1.0;
    for (key, value) in object {
        let (dst, bound) = match key.as_str() {
            "exposure" => (&mut settings.exposure, 10.0),
            "contrast" => (&mut settings.contrast, 100.0),
            "highlights" => (&mut settings.highlights, 100.0),
            "shadows" => (&mut settings.shadows, 100.0),
            "whites" => (&mut settings.whites, 100.0),
            "blacks" => (&mut settings.blacks, 100.0),
            "amount" => (&mut amount, 1.0),
            _ => {
                return Err(EngineError::invalid(
                    "camera_raw",
                    format!("unknown parameter {key}"),
                ));
            }
        };
        let v = value
            .as_f64()
            .ok_or_else(|| EngineError::invalid("camera_raw", "number required"))?;
        if !v.is_finite() || v.abs() > bound || (key == "amount" && v < 0.0) {
            return Err(EngineError::invalid(
                "camera_raw",
                "parameter outside tone domain",
            ));
        }
        *dst = v as f32;
    }
    if input.channels() != 4
        || input.depth() != compositor::raster::Depth::F32
        || input.extent().area() == 0
    {
        return Err(EngineError::invalid(
            "camera_raw",
            "nonempty F32 RGBA required",
        ));
    }
    if amount == 0.0 {
        return Ok(input.clone());
    }
    let rev = input
        .max_rev()
        .checked_add(1)
        .ok_or_else(|| EngineError::invalid("camera_raw", "revision overflow"))?;
    let mut out = input.clone();
    let (nx, ny) = input.grid();
    let mut samples = Vec::new();
    for ty in 0..ny {
        for tx in 0..nx {
            input.read_tile(tx, ty, &mut samples)?;
            if samples.iter().any(|v| !v.is_finite()) {
                return Err(EngineError::invalid("camera_raw", "finite pixels required"));
            }
            let layout = TileLayout {
                channels: 3,
                ..input.layout(tx, ty)
            };
            let n = layout.plane_len();
            let mut tile =
                Tile::from_samples(TileCoord::new(0, tx, ty), layout, samples[..3 * n].to_vec())?;
            pipeline_cpu::tone(&mut tile, &settings)?;
            let data = tile.samples::<f32>()?;
            if data.iter().any(|v| !v.is_finite()) {
                return Err(EngineError::invalid("camera_raw", "nonfinite tone result"));
            }
            out.edit_region(
                Rect::new(
                    i64::from(tx) * 256,
                    i64::from(ty) * 256,
                    i64::from(tx + 1) * 256,
                    i64::from(ty + 1) * 256,
                ),
                rev,
                |x, y, p| {
                    let i = (y - ty * 256) as usize * layout.stride() + (x - tx * 256) as usize;
                    for c in 0..3 {
                        p[c] += amount * (data[c * n + i] - p[c]);
                    }
                },
            )?;
        }
    }
    Ok(out)
}
