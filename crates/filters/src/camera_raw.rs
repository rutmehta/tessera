//! Shared Develop renderer over straight, document-linear RGBA (never CFA).
use color_mgmt::{Builtin, Registry, Transform, TransformOptions};
use compositor::{
    raster::{Depth, Raster},
    render::smart_filters::FilterContext,
};
use engine_api::{
    EngineError, EngineResult,
    color::ColorMatrix3,
    recipe::{CrsKey, CrsTarget, CrsValueType, DevelopSettings},
};

use std::sync::atomic::AtomicBool;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Params {
    pub settings: DevelopSettings,
    #[serde(default = "one")]
    pub amount: f32,
}
fn one() -> f32 {
    1.0
}

/// Decode the complete engine settings schema; outer filter fields are strict.
pub fn parse(value: &serde_json::Value) -> EngineResult<Params> {
    let p: Params = serde_json::from_value(value.clone())
        .map_err(|e| EngineError::invalid("camera_raw", e.to_string()))?;
    if !p.amount.is_finite() || !(0.0..=1.0).contains(&p.amount) {
        return Err(EngineError::invalid(
            "camera_raw.amount",
            "finite [0,1] required",
        ));
    }
    // Use the engine API's slider domains, not another independent range table.
    // Integer XMP controls are continuous float sliders in the native engine.
    let settings = serde_json::to_value(&p.settings)?;
    for key in CrsKey::ALL {
        let CrsTarget::Field(path) = key.target() else {
            continue;
        };
        let Some(path) = path.strip_prefix("/settings") else {
            continue;
        };
        if !["/tone/", "/color/", "/detail/", "/white_balance/"]
            .iter()
            .any(|prefix| path.starts_with(prefix))
        {
            continue;
        }
        let Some(v) = settings.pointer(path).and_then(|v| v.as_f64()) else {
            continue;
        };
        let valid = match key.value_type() {
            CrsValueType::Integer { min, max } => (min as f64..=max as f64).contains(&v),
            ty => ty.accepts(v),
        };
        if !valid {
            return Err(EngineError::invalid(
                format!("camera_raw.settings{path}"),
                "outside engine settings domain",
            ));
        }
    }
    for group in &p.settings.locals.adjustments {
        if group.components.iter().any(|c| c.kind.is_ai()) {
            return Err(EngineError::Unsupported { what: "camera_raw AI mask requires a host-supplied segmentation/depth raster; no AI mask provider is installed".into() });
        }
    }
    pipeline_cpu::validate_settings(&p.settings)?;
    Ok(p)
}
fn color_error(error: color_mgmt::Error) -> EngineError {
    EngineError::Color {
        message: format!("camera_raw: {error}"),
    }
}

/// Row-major document-linear -> LinearRec2020 and reverse matrices.
/// Embedded ICC bytes are authoritative. Untagged documents mean linear sRGB.
/// Matrix-shaper TRCs are removed because compositor samples are already linear.
/// LUT-based or unresolved profiles are rejected, never silently treated as sRGB.
/// Sampling the linear ICC transform on basis vectors preserves signed/HDR values
/// when these matrices are applied on either CPU or GPU.
pub fn profile_matrices(context: &FilterContext) -> EngineResult<(ColorMatrix3, ColorMatrix3)> {
    let mut registry = Registry::new();
    let profile = match &context.profile {
        None => registry.builtin(Builtin::Srgb).map_err(color_error)?,
        Some(profile) => {
            let bytes = profile.icc.as_deref().ok_or_else(|| EngineError::Color {
                message: format!("camera_raw: unresolved ICC profile {}", profile.name),
            })?;
            registry.load_bytes(bytes).map_err(color_error)?
        }
    };
    let working = registry
        .linearized_rgb(&profile)
        .map_err(color_error)?
        .ok_or_else(|| EngineError::Unsupported {
            what: "camera_raw requires a matrix-shaper RGB working ICC profile".into(),
        })?;
    let rec2020 = registry
        .builtin(Builtin::LinearRec2020)
        .map_err(color_error)?;
    let options = TransformOptions {
        black_point_compensation: false,
        ..Default::default()
    };
    let transform = Transform::new(&working, &rec2020, options).map_err(color_error)?;
    let basis = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]].map(|v| transform.apply(v));
    let forward = ColorMatrix3(std::array::from_fn(|r| {
        std::array::from_fn(|c| f64::from(basis[c][r]))
    }));
    Ok((forward, forward.inverse()?))
}

/// The input is the actual filter-level image. Render it at level zero rather
/// than downsampling it a second time using the presentation mip level.
/// Crop output is placed at the canvas origin and padded black, retaining the
/// input alpha and extent as required by the smart-filter contract.
pub fn evaluate(
    input: &Raster,
    value: &serde_json::Value,
    context: &FilterContext,
) -> EngineResult<Raster> {
    let params = parse(value)?;
    if input.channels() != 4 || input.depth() != Depth::F32 || input.extent().area() == 0 {
        return Err(EngineError::invalid(
            "camera_raw",
            "nonempty F32 RGBA required",
        ));
    }
    let cancel = AtomicBool::new(false);
    let source = crate::Buffer::read(input, &cancel)?;
    let (forward, backward) = profile_matrices(context)?;
    if params.amount == 0.0 {
        return Ok(input.clone());
    }
    let mut planes: Vec<_> = (0..3)
        .map(|_| Vec::with_capacity(source.pixels.len()))
        .collect();
    for pixel in &source.pixels {
        let rgb = forward.apply([pixel[0] as f64, pixel[1] as f64, pixel[2] as f64]);
        for c in 0..3 {
            planes[c].push(rgb[c] as f32);
        }
    }
    let extent = input.extent();
    let pixels = pipeline_cpu::Image::new(extent.width, extent.height, planes)?;
    // The scalar RGB entry point shares the Develop operators and procedural
    // mask hooks with image-core, without a file round-trip or CFA fabrication.
    let developed = pipeline_cpu::render_linear_scaled(
        &params.settings,
        &pipeline_cpu::RenderSource::Rgb(&pixels),
        1,
    )?;
    let output_extent = engine_api::tile::Extent::new(developed.width(), developed.height());
    let mut result = source.clone();
    for y in 0..extent.height {
        for x in 0..extent.width {
            let i = y as usize * source.w + x as usize;
            let rgb = if x < output_extent.width && y < output_extent.height {
                let j = (y * output_extent.width + x) as usize;
                backward.apply(std::array::from_fn(|c| developed.planes()[c][j] as f64))
            } else {
                [0.0; 3]
            };
            for (c, value) in rgb.iter().enumerate() {
                result.pixels[i][c] =
                    source.pixels[i][c] + params.amount * (*value as f32 - source.pixels[i][c]);
            }
        }
    }
    result.write(input, &cancel)
}
