//! Shared Develop renderer over straight document RGBA (never CFA).
//!
//! Document samples are ENCODED in the document profile's transfer function
//! (untagged means sRGB-encoded), like every other compositor sample. Develop
//! runs on scene-linear Rec.2020: decode with the profile TRC, apply the
//! profile matrix, develop, apply the inverse matrix and re-encode. Amount
//! (the filter's opacity) blends the encoded samples.
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

/// The document profile: embedded ICC bytes are authoritative, untagged
/// documents are sRGB (sRGB primaries and sRGB transfer curve).
fn document_profile(
    registry: &mut Registry,
    context: &FilterContext,
) -> EngineResult<std::sync::Arc<color_mgmt::Profile>> {
    match &context.profile {
        None => registry.builtin(Builtin::Srgb).map_err(color_error),
        Some(profile) => {
            let bytes = profile.icc.as_deref().ok_or_else(|| EngineError::Color {
                message: format!("camera_raw: unresolved ICC profile {}", profile.name),
            })?;
            registry.load_bytes(bytes).map_err(color_error)
        }
    }
}
fn linear_twin(
    registry: &mut Registry,
    profile: &color_mgmt::Profile,
) -> EngineResult<std::sync::Arc<color_mgmt::Profile>> {
    registry
        .linearized_rgb(profile)
        .map_err(color_error)?
        .ok_or_else(|| EngineError::Unsupported {
            what: "camera_raw requires a matrix-shaper RGB working ICC profile".into(),
        })
}
fn colorimetric() -> TransformOptions {
    TransformOptions {
        black_point_compensation: false,
        ..Default::default()
    }
}

/// Row-major linear document RGB -> LinearRec2020 and reverse matrices: the
/// profile's primaries with its TRCs removed (see [`profile_curves`] for the
/// transfer curve that is decoded before and re-encoded after them).
/// LUT-based or unresolved profiles are rejected, never silently treated as sRGB.
/// Sampling the linear ICC transform on basis vectors preserves signed/HDR values
/// when these matrices are applied on either CPU or GPU.
pub fn profile_matrices(context: &FilterContext) -> EngineResult<(ColorMatrix3, ColorMatrix3)> {
    let mut registry = Registry::new();
    let profile = document_profile(&mut registry, context)?;
    let working = linear_twin(&mut registry, &profile)?;
    let rec2020 = registry
        .builtin(Builtin::LinearRec2020)
        .map_err(color_error)?;
    let transform = Transform::new(&working, &rec2020, colorimetric()).map_err(color_error)?;
    let basis = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]].map(|v| transform.apply(v));
    let forward = ColorMatrix3(std::array::from_fn(|r| {
        std::array::from_fn(|c| f64::from(basis[c][r]))
    }));
    Ok((forward, forward.inverse()?))
}

/// Entries per channel of a sampled transfer curve.
pub const TRANSFER_LUT_SIZE: usize = 4096;

/// The document profile's per-channel transfer curves (encoded -> linear),
/// sampled once through the ICC engine so the CPU and GPU paths evaluate the
/// identical piecewise-linear curve.
///
/// - `decode` interpolates [`TRANSFER_LUT_SIZE`] uniform samples on [0,1].
/// - `encode` is the exact inverse of that piecewise-linear decode (a binary
///   search of the monotone table), so `encode(decode(x)) == x` up to f32.
/// - Float samples outside [0,1]: point-symmetric about the curve's value at
///   zero below 0 (odd-symmetric for every normal TRC, whose zero maps to
///   zero) and continued with the endpoint slope above 1.
/// - A linear-TRC profile (e.g. linear Rec.2020 float documents) is an exact
///   identity: [`TransferCurves::is_identity`] and no arithmetic at all.
#[derive(Clone, Debug, PartialEq)]
pub struct TransferCurves {
    /// Channel-major `3 * TRANSFER_LUT_SIZE` decode samples; empty = identity.
    table: Vec<f32>,
}
impl TransferCurves {
    pub fn identity() -> Self {
        Self { table: Vec::new() }
    }
    pub fn is_identity(&self) -> bool {
        self.table.is_empty()
    }
    fn channel(&self, c: usize) -> &[f32] {
        &self.table[c * TRANSFER_LUT_SIZE..(c + 1) * TRANSFER_LUT_SIZE]
    }
    /// Encoded document RGB -> linear document RGB.
    pub fn decode(&self, rgb: [f32; 3]) -> [f32; 3] {
        if self.is_identity() {
            return rgb;
        }
        std::array::from_fn(|c| decode_channel(self.channel(c), rgb[c]))
    }
    /// Linear document RGB -> encoded document RGB.
    pub fn encode(&self, rgb: [f32; 3]) -> [f32; 3] {
        if self.is_identity() {
            return rgb;
        }
        std::array::from_fn(|c| encode_channel(self.channel(c), rgb[c]))
    }
    /// The channel-major decode table the resident bridge binds (empty for
    /// the identity).
    pub fn table(&self) -> &[f32] {
        &self.table
    }
    fn digest_bytes(&self) -> Vec<u8> {
        self.table.iter().flat_map(|v| v.to_le_bytes()).collect()
    }
}

// Keep these two in lockstep with `decode` / `encode` in camera_raw_gpu's WGSL.
fn decode_channel(t: &[f32], v: f32) -> f32 {
    let last = t.len() - 1;
    if v < 0. {
        return 2. * t[0] - decode_channel(t, -v);
    }
    let x = v * last as f32;
    if x >= last as f32 {
        return t[last] + (x - last as f32) * (t[last] - t[last - 1]);
    }
    let i = x as usize;
    t[i] + (x - i as f32) * (t[i + 1] - t[i])
}
fn encode_channel(t: &[f32], y: f32) -> f32 {
    let last = t.len() - 1;
    if y < t[0] {
        return -encode_channel(t, 2. * t[0] - y);
    }
    if y >= t[last] {
        return 1. + (y - t[last]) / ((t[last] - t[last - 1]) * last as f32);
    }
    if y.is_nan() {
        return y;
    }
    // Largest i with t[i] <= y; t[0] <= y < t[last] bounds it to [0, last).
    let (mut lo, mut hi) = (0, last);
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if t[mid] <= y { lo = mid } else { hi = mid }
    }
    let span = t[lo + 1] - t[lo];
    let f = if span > 0. { (y - t[lo]) / span } else { 0. };
    (lo as f32 + f) / last as f32
}

/// The document profile's transfer curves (see [`TransferCurves`]). Untagged
/// documents use the sRGB curve. Curves that are not monotone nondecreasing,
/// or flat at white, are rejected rather than inverted approximately.
pub fn profile_curves(context: &FilterContext) -> EngineResult<TransferCurves> {
    let mut registry = Registry::new();
    let profile = document_profile(&mut registry, context)?;
    let digest = profile.digest();
    static CACHE: std::sync::Mutex<Vec<([u8; 32], TransferCurves)>> =
        std::sync::Mutex::new(Vec::new());
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, curves)) = cache.iter().find(|(d, _)| *d == digest) {
        return Ok(curves.clone());
    }
    let working = linear_twin(&mut registry, &profile)?;
    // Same primaries and white: the matrix part is the identity, so each output
    // channel is that channel's TRC.
    let decode = Transform::new(&profile, &working, colorimetric()).map_err(color_error)?;
    let n = TRANSFER_LUT_SIZE;
    let mut table = vec![0f32; 3 * n];
    for i in 0..n {
        let v = i as f32 / (n - 1) as f32;
        let linear = decode.apply([v; 3]);
        for c in 0..3 {
            table[c * n + i] = linear[c];
        }
    }
    let unsupported = |what: &str| EngineError::Unsupported {
        what: format!("camera_raw document transfer curve {what}"),
    };
    let mut identity = true;
    for c in 0..3 {
        let t = &mut table[c * n..(c + 1) * n];
        for i in 0..n {
            if !t[i].is_finite() {
                return Err(unsupported("is not finite"));
            }
            if i > 0 && t[i] < t[i - 1] {
                // LCMS float round-off only; a genuinely decreasing TRC is refused.
                if t[i - 1] - t[i] > 1e-6 {
                    return Err(unsupported("is not monotone"));
                }
                t[i] = t[i - 1];
            }
            identity &= (t[i] - i as f32 / (n - 1) as f32).abs() <= 1e-6;
        }
        if t[n - 1] <= t[n - 2] {
            return Err(unsupported("is flat at white"));
        }
    }
    let curves = if identity {
        TransferCurves::identity()
    } else {
        TransferCurves { table }
    };
    if cache.len() >= 16 {
        cache.remove(0);
    }
    cache.push((digest, curves.clone()));
    Ok(curves)
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
    let curves = profile_curves(context)?;
    if params.amount == 0.0 {
        return Ok(input.clone());
    }
    let mut planes: Vec<_> = (0..3)
        .map(|_| Vec::with_capacity(source.pixels.len()))
        .collect();
    for pixel in &source.pixels {
        let linear = curves.decode([pixel[0], pixel[1], pixel[2]]);
        let rgb = forward.apply(linear.map(f64::from));
        for c in 0..3 {
            planes[c].push(rgb[c] as f32);
        }
    }
    let extent = input.extent();
    let pixels = pipeline_cpu::Image::new(extent.width, extent.height, planes)?;
    // FilterContext has no layer identifier. Hash exact source bits, all tile
    // revisions and interpretation so independent layers/revisions cannot alias.
    // Hash one row at a time to avoid another full-frame key allocation.
    use engine_api::{
        id::{Digest, ImageId},
        jobs::CancellationToken,
        stage::{ParamHash, StageId},
    };
    let mut identity = ParamHash::of(
        StageId::Decode,
        &(
            extent.width,
            extent.height,
            context.level,
            context.canvas.width,
            context.canvas.height,
            forward.0,
            input
                .slots()
                .map(|(coord, slot)| (coord, slot.rev))
                .collect::<Vec<_>>(),
        ),
    );
    identity = ParamHash::chain(
        identity,
        ParamHash(Digest::derive(
            "camera raw transfer curves",
            &curves.digest_bytes(),
        )),
    );
    for row in source.pixels.chunks(source.w) {
        let bytes: Vec<u8> = row
            .iter()
            .flat_map(|p| p.iter().flat_map(|v| v.to_bits().to_le_bytes()))
            .collect();
        identity = ParamHash::chain(
            identity,
            ParamHash(Digest::derive("camera raw source row", &bytes)),
        );
    }
    let id = ImageId(u128::from_le_bytes(
        identity.0.0[..16].try_into().expect("digest prefix"),
    ));
    let rgb = image_core::RgbSource::from_linear_rec2020(pixels)?;
    let image = image_core::RawImage::from_rgb(id, rgb)?;
    static RENDERER: std::sync::OnceLock<image_core::Renderer> = std::sync::OnceLock::new();
    let renderer = RENDERER.get_or_init(|| {
        image_core::Renderer::new(image_core::RendererConfig {
            cache_budget_bytes: 256 << 20,
            ..Default::default()
        })
    });
    let developed = renderer.render_rgb_linear(
        &image,
        input.max_rev(),
        &params.settings,
        &CancellationToken::new(),
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
            // Re-encode, then blend the encoded samples (amount is opacity).
            let rgb = curves.encode(rgb.map(|v| v as f32));
            for (c, value) in rgb.iter().enumerate() {
                result.pixels[i][c] =
                    source.pixels[i][c] + params.amount * (*value - source.pixels[i][c]);
            }
        }
    }
    result.write(input, &cancel)
}
