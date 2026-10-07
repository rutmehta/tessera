//! Full-resolution image export.
mod adobe_render;
mod ai_masks;
mod batch;
mod depth;
/// Shared preview/export segmentation implementation.
pub use mask_ai;
mod avif;
mod codec;
mod dng;
mod hdr;
pub use engine_api::tools::HdrTransfer;
mod jxl;
pub use avif::{AvifOptions, encode_avif};
mod watermark;
mod workflow;
pub use batch::{
    BatchReport, ExportItem, Progress, export_batch, export_batch_upscaled, export_batch_with_jobs,
};
pub use watermark::{Anchor, Watermark, apply_watermark};
pub use workflow::{AfterExportActions, AfterExportCommand, run_after_export};
mod filter;
mod gpu;
use engine_api::{EngineError, EngineResult};
use engine_api::{jobs::CancellationToken, recipe::Recipe};
pub use filter::{Resize, SharpenAmount, SharpenFor, sharpen_output};
use pipeline_cpu::RenderSource;
use sidecar::{MarkPreset, Sidecar, XmpPacket};
mod native;
mod original;
pub use original::export_original;
use std::{fs, io::Write, path::PathBuf};

/// Borrowed decoded pixels and stable naming context. Metadata is an optional
/// source XMP packet; the exporter never mutates the source or its sidecar.
pub struct ExportImage<'a> {
    pub source: RenderSource<'a>,
    pub name: &'a str,
    pub sequence: usize,
    pub date: &'a str,
    pub metadata: Option<&'a XmpPacket>,
}

#[derive(Clone, Copy, Debug)]
pub enum Format {
    Jpeg {
        quality: u8,
    },
    Png,
    /// Developed float32 LinearRaw DNG in linear Rec.2020 (D65).
    Dng,
    Tiff {
        bits: u8,
    },
    Avif(AvifOptions),
    /// Lossless sRGB JPEG XL, 8 or 16 bits per channel.
    JpegXl {
        bits: u8,
    },
}
impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Jpeg { .. } => "jpg",
            Self::Png => "png",
            Self::Dng => "dng",
            Self::Tiff { .. } => "tif",
            Self::Avif(_) => "avif",
            Self::JpegXl { .. } => "jxl",
        }
    }
    fn validate(self) -> EngineResult<()> {
        match self {
            Self::Avif(options) => options.validate(),
            Self::Jpeg { quality: 1..=100 }
            | Self::Png
            | Self::Dng
            | Self::Tiff { bits: 8 | 16 }
            | Self::JpegXl { bits: 8 | 16 } => Ok(()),
            _ => Err(EngineError::invalid(
                "format",
                "JPEG quality must be 1..=100; TIFF/JPEG XL bits must be 8 or 16",
            )),
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub enum ColorSpace {
    Srgb,
    DisplayP3,
    Rec2020,
    ProPhoto,
}
#[derive(Clone, Copy, Debug)]
pub enum Metadata {
    All,
    CopyrightOnly,
    CopyrightAndContact,
    AllExceptCamera,
    None,
}

#[derive(Clone, Debug)]
pub struct ExportSettings {
    /// Caller-owned Tessera support root for imported mask resources.
    pub mask_support: Option<PathBuf>,
    /// Caller-owned CPU retouch implementation; never serialized into a recipe.
    pub retouch: Option<std::sync::Arc<dyn pipeline_cpu::RetouchRenderer>>,
    pub format: Format,
    /// HDR PNG uses 16-bit samples; HDR AVIF requires 10/12 bits. None is SDR.
    pub hdr: Option<HdrTransfer>,
    pub color_space: ColorSpace,
    pub metadata: Metadata,
    /// Remove named regions and associated person keywords across native/XMP carriers.
    pub remove_person_info: bool,
    /// Remove GPS and IPTC image locations across native/XMP carriers.
    pub remove_location: bool,
    /// Preserve Lightroom keyword paths (flat keywords become single-level paths).
    pub keywords_as_hierarchy: bool,
    pub resize: Resize,
    pub sharpen_for: SharpenFor,
    pub sharpen_amount: SharpenAmount,
    pub naming: String,
    pub output_dir: PathBuf,
    /// Pixel density recorded in the file (JFIF, PNG pHYs, TIFF resolution
    /// tags). Also controls paper sharpening radius; never resamples.
    /// None records no density and uses 300 ppi for paper sharpening.
    pub dpi: Option<u32>,
    /// Rotate/flip RAW renders from sensor orientation into the EXIF
    /// orientation (what a viewer shows). Off by default: existing callers
    /// receive sensor-oriented pixels, as before.
    pub apply_orientation: bool,
    /// Render from a 1/2, 1/4 or 1/8 binned source (1, 2, 4 or 8) before
    /// `resize`, for outputs much smaller than the original. The caller picks
    /// a scale that still covers the output size. Ignored with AI masks or
    /// super-resolution (both need full resolution).
    pub render_scale: u32,
    /// JPEG byte budget including embedded metadata. An impossible budget
    /// fails without publishing output. Quality is an upper bound.
    pub max_file_bytes: Option<u64>,
    /// Composited in document-encoded RGB after output sharpening.
    pub watermark: Option<Watermark>,
    /// Embed this source's byte stream in a developed DNG. Requires unrestricted
    /// metadata because the original itself is not privacy-filtered.
    pub original_raw: Option<PathBuf>,
    /// Source files keyed by ExportImage sequence; read-only native metadata context.
    pub metadata_sources: std::collections::BTreeMap<usize, PathBuf>,
}
impl Default for ExportSettings {
    fn default() -> Self {
        Self {
            mask_support: None,
            retouch: None,
            format: Format::Jpeg { quality: 90 },
            hdr: None,
            color_space: ColorSpace::Srgb,
            metadata: Metadata::All,
            remove_person_info: false,
            remove_location: false,
            keywords_as_hierarchy: true,
            resize: Resize::None,
            sharpen_for: SharpenFor::None,
            sharpen_amount: SharpenAmount::Standard,
            naming: "{name}-{seq}".into(),
            output_dir: ".".into(),
            dpi: None,
            apply_orientation: false,
            render_scale: 1,
            max_file_bytes: None,
            watermark: None,
            original_raw: None,
            metadata_sources: Default::default(),
        }
    }
}

/// Render directly to the document profile in float, without an sRGB intermediate.
#[cfg(test)]
fn render_full(
    image: &ExportImage<'_>,
    recipe: &Recipe,
    space: ColorSpace,
) -> EngineResult<image::Rgb32FImage> {
    render_scaled(image, recipe, space, 1)
}

#[cfg(test)]
fn render_scaled(
    image: &ExportImage<'_>,
    recipe: &Recipe,
    space: ColorSpace,
    scale: u32,
) -> EngineResult<image::Rgb32FImage> {
    render_scaled_cancellable(image, recipe, space, scale, &CancellationToken::new())
}

fn render_scaled_cancellable(
    image: &ExportImage<'_>,
    recipe: &Recipe,
    space: ColorSpace,
    scale: u32,
    cancel: &CancellationToken,
) -> EngineResult<image::Rgb32FImage> {
    if std::env::var("TESSERA_EXPORT_BACKEND").as_deref() != Ok("cpu")
        && let Some(rgb) = gpu::render(image, recipe, space, scale, cancel, gpu::BUDGET)?
    {
        return Ok(rgb);
    }
    render_scaled_cpu(image, recipe, space, scale)
}

fn render_scaled_cpu(
    image: &ExportImage<'_>,
    recipe: &Recipe,
    space: ColorSpace,
    scale: u32,
) -> EngineResult<image::Rgb32FImage> {
    if is_adobe(recipe) {
        return encode_output_profile(adobe_float(image, recipe, scale)?, recipe, space);
    }
    if ai_masks::active(&recipe.settings) {
        return encode_output_profile(render_full_float(image, recipe)?, recipe, space);
    }
    let mut registry = color_mgmt::Registry::new();
    let target = codec::profile(&mut registry, space)?;
    let mut settings = recipe.settings.clone();
    // Proofing is a display-only preview, never baked into a file export.
    settings.output.proof_profile = None;
    Ok(pipeline_cpu::render_managed_scaled(
        &settings,
        &image.source,
        scale,
        &mut pipeline_cpu::OutputContext {
            registry: &mut registry,
            target: pipeline_cpu::OutputTarget::Export(&target),
            proof: None,
            options: color_mgmt::TransformOptions::default(),
        },
    )?
    .pixels)
}

/// Recorded with every HDR export of an Adobe-process recipe and shown in
/// Develop while its HDR toggle is on (REV-ENG-9 SF1): the compatibility
/// pipeline is display-referred, so the HDR file holds an SDR rendition.
pub const ADOBE_HDR_NOTICE: &str = "Lightroom-process edits render in standard dynamic range; \
     this HDR file has no highlights above SDR white.";

/// Develop draws every Adobe-process recipe (imported Lightroom edits) with
/// the compatibility pipeline, whatever the source: RAW and RGB originals and
/// Smart Preview proxies alike. Every output path follows it (ENG-9).
fn is_adobe(recipe: &Recipe) -> bool {
    recipe.process_version.family == engine_api::recipe::ProcessFamily::Adobe
}

/// Outputs rendered by [`ai_masks::render_develop`], the renderer with
/// Develop's local-mask, Lens Blur and retouch resources: every Adobe-process
/// recipe, and external Smart Previews that need those resources.
fn uses_develop_renderer(source: &RenderSource<'_>, recipe: &Recipe) -> bool {
    is_adobe(recipe)
        || (matches!(source, RenderSource::CameraLinear(p) if p.is_external_dng())
            && (ai_masks::active(&recipe.settings)
                || recipe.settings.effects.lens_blur.is_some()
                || !recipe.settings.locals.retouch.is_empty()))
}

/// Enhancement input is tone-mapped linear Rec.2020, never encoded sRGB.
fn adobe_float(
    image: &ExportImage<'_>,
    recipe: &Recipe,
    scale: u32,
) -> EngineResult<image::Rgb32FImage> {
    let rgb =
        image_core::pipeline_adobe::render_linear_scaled(&recipe.settings, &image.source, scale)?;
    Ok(image::Rgb32FImage::from_fn(
        rgb.width(),
        rgb.height(),
        |x, y| {
            let i = (y * rgb.width() + x) as usize;
            image::Rgb(std::array::from_fn(|c| rgb.planes()[c][i]))
        },
    ))
}

fn render_full_float(image: &ExportImage<'_>, recipe: &Recipe) -> EngineResult<image::Rgb32FImage> {
    if is_adobe(recipe) {
        return adobe_float(image, recipe, 1);
    }
    if ai_masks::active(&recipe.settings) {
        ai_masks::render(&image.source, &recipe.settings, None)
    } else {
        pipeline_cpu::render_output_linear_scaled(&recipe.settings, &image.source, 1)
    }
}

fn encode_output_profile(
    rgb: image::Rgb32FImage,
    recipe: &Recipe,
    space: ColorSpace,
) -> EngineResult<image::Rgb32FImage> {
    let mut registry = color_mgmt::Registry::new();
    let target = codec::profile(&mut registry, space)?;
    let mut settings = recipe.settings.clone();
    settings.output.proof_profile = None;
    Ok(pipeline_cpu::output_managed_linear(
        &settings,
        rgb,
        &mut pipeline_cpu::OutputContext {
            registry: &mut registry,
            target: pipeline_cpu::OutputTarget::Export(&target),
            proof: None,
            options: color_mgmt::TransformOptions::default(),
        },
    )?
    .pixels)
}

fn encode_error(e: impl std::fmt::Display) -> EngineError {
    EngineError::invalid("export", e.to_string())
}

pub fn export_one(
    image: &ExportImage<'_>,
    recipe: &Recipe,
    settings: &ExportSettings,
) -> EngineResult<PathBuf> {
    let cancel = CancellationToken::new();
    prepare(image, recipe, settings, &cancel)?.commit(&cancel)
}

/// One export with a caller-owned cancellation token and optional explicit
/// super-resolution model and segmentation backend (for recipes with AI
/// masks; see [`needs_segmenter`]). Streaming callers decode, export and drop
/// one image at a time instead of holding a whole batch of RAWs in memory.
pub fn export_one_cancellable(
    image: &ExportImage<'_>,
    recipe: &Recipe,
    settings: &ExportSettings,
    cancel: &CancellationToken,
    upscale: Option<&mut ml_enhance::SuperResolution>,
    segmenter: Option<&mut dyn mask_ai::MaskSegmenter>,
) -> EngineResult<PathBuf> {
    prepare_with_segmenter(image, recipe, settings, cancel, upscale, segmenter)?.commit(cancel)
}

/// Whether rendering `recipe` needs a segmentation backend (enabled AI masks).
pub fn needs_segmenter(recipe: &Recipe) -> bool {
    recipe
        .settings
        .locals
        .adjustments
        .iter()
        .filter(|g| g.enabled && g.amount != 0.)
        .flat_map(|g| &g.components)
        .flat_map(|c| c.active_leaves())
        .any(|c| {
            c.kind.is_ai()
                && !matches!(c.kind, engine_api::recipe::MaskKind::Depth { .. })
                && c.adobe_ai.as_ref().and_then(|s| s.mask_key).is_none()
        })
}

/// Rendered pixels without writing a file (print, contact sheets): the same
/// pipeline, orientation, resize and output sharpening as an export, in the
/// document colour space `space` (encoded floats, 0–1).
///
/// `scale` (1, 2, 4 or 8) renders from a binned source for small outputs; it
/// is ignored (1) when AI masks need the full-resolution segmentation input.
pub fn render_pixels(
    image: &ExportImage<'_>,
    recipe: &Recipe,
    render: &RenderRequest,
    cancel: &CancellationToken,
    segmenter: Option<&mut dyn mask_ai::MaskSegmenter>,
) -> EngineResult<image::Rgb32FImage> {
    render_pixels_with_retouch(image, recipe, render, cancel, segmenter, None)
}

pub fn render_pixels_with_mask_support(
    image: &ExportImage<'_>,
    recipe: &Recipe,
    render: &RenderRequest,
    cancel: &CancellationToken,
    segmenter: Option<&mut dyn mask_ai::MaskSegmenter>,
    support: Option<&std::path::Path>,
) -> EngineResult<image::Rgb32FImage> {
    render_pixels_with_resources(image, recipe, render, cancel, segmenter, support, None)
}

pub fn render_pixels_with_retouch(
    image: &ExportImage<'_>,
    recipe: &Recipe,
    render: &RenderRequest,
    cancel: &CancellationToken,
    segmenter: Option<&mut dyn mask_ai::MaskSegmenter>,
    retouch: Option<std::sync::Arc<dyn pipeline_cpu::RetouchRenderer>>,
) -> EngineResult<image::Rgb32FImage> {
    render_pixels_with_resources(image, recipe, render, cancel, segmenter, None, retouch)
}

fn retouch_float(
    image: &ExportImage<'_>,
    recipe: &Recipe,
    scale: u32,
    retouch: Option<std::sync::Arc<dyn pipeline_cpu::RetouchRenderer>>,
) -> EngineResult<image::Rgb32FImage> {
    if !matches!(scale, 1 | 2 | 4 | 8) {
        return Err(EngineError::invalid("scale", "must be 1, 2, 4 or 8"));
    }
    // These hooks need their own inference contexts; never drop them for retouch.
    if ai_masks::active(&recipe.settings) || depth::active(&image.source, &recipe.settings) {
        return Err(EngineError::invalid(
            "retouch",
            "retouch export with AI masks, denoise or depth is not supported",
        ));
    }
    let mut settings = recipe.settings.clone();
    settings.output.proof_profile = None;
    let context = pipeline_cpu::LensContext {
        retouch,
        ..Default::default()
    };
    let rgb =
        pipeline_cpu::render_linear_scaled_with_lens(&settings, &image.source, scale, &context)?;
    Ok(depth::tone_map(rgb))
}

/// Print/pixel export with a caller-owned retouch implementation.
pub fn render_pixels_with_resources(
    image: &ExportImage<'_>,
    recipe: &Recipe,
    render: &RenderRequest,
    cancel: &CancellationToken,
    segmenter: Option<&mut dyn mask_ai::MaskSegmenter>,
    support: Option<&std::path::Path>,
    retouch: Option<std::sync::Arc<dyn pipeline_cpu::RetouchRenderer>>,
) -> EngineResult<image::Rgb32FImage> {
    render_pixels_with_notes(image, recipe, render, cancel, segmenter, support, retouch)
        .map(|(rgb, _)| rgb)
}

/// [`render_pixels_with_resources`] plus the user-facing notes a file export
/// would record ([`proxy_notes`] and render warnings), for print and documents.
pub fn render_pixels_with_notes(
    image: &ExportImage<'_>,
    recipe: &Recipe,
    render: &RenderRequest,
    cancel: &CancellationToken,
    segmenter: Option<&mut dyn mask_ai::MaskSegmenter>,
    support: Option<&std::path::Path>,
    retouch: Option<std::sync::Arc<dyn pipeline_cpu::RetouchRenderer>>,
) -> EngineResult<(image::Rgb32FImage, Vec<String>)> {
    require_full_quality_source(&image.source)?;
    cancel.check()?;
    recipe.validate()?;
    let mut notes = proxy_notes(&image.source, recipe, support.is_some(), retouch.is_some());
    // Print and documents render; they do not export a file (REV2-SP N2).
    if let Some(first) = notes.first_mut()
        && first.starts_with("Exported from a Smart Preview")
    {
        *first = first.replacen("Exported from", "Rendered from", 1);
    }
    let planned = proxy_recipe(&image.source, recipe, support.is_some(), retouch.is_some());
    let recipe = planned.as_ref();
    if !matches!(render.scale, 1 | 2 | 4 | 8) {
        return Err(EngineError::invalid("scale", "must be 1, 2, 4 or 8"));
    }
    let rgb = if uses_develop_renderer(&image.source, recipe) {
        let rgb = ai_masks::render_develop(
            &image.source,
            recipe,
            render.scale,
            segmenter,
            &mut notes,
            support,
            retouch,
            cancel,
        )?;
        encode_output_profile(rgb, recipe, render.color_space)?
    } else if !recipe.settings.locals.retouch.is_empty() {
        let rgb = retouch_float(image, recipe, render.scale, retouch)?;
        encode_output_profile(rgb, recipe, render.color_space)?
    } else if ai_masks::active(&recipe.settings) {
        let rgb =
            ai_masks::render_with_support(&image.source, &recipe.settings, segmenter, support)?;
        encode_output_profile(rgb, recipe, render.color_space)?
    } else {
        render_scaled_cancellable(image, recipe, render.color_space, render.scale, cancel)?
    };
    cancel.check()?;
    let rgb = orient(rgb, source_orientation(&image.source));
    let rgb = filter::resize(rgb, render.resize, cancel)?;
    Ok((filter::sharpen(rgb, render.sharpen_for, cancel)?, notes))
}

/// Plain-sentence text for a Smart Preview render-plan field (shared with
/// Develop's loupe notices so every surface says the same thing).
pub fn proxy_notice_text(field: &str) -> &'static str {
    match field {
        "/decode" | "/linearize" | "/demosaic" | "/denoise" => {
            "Mosaic corrections are already baked into this Smart Preview."
        }
        "/white_balance/mode" => "Auto white balance unavailable; shown using As Shot.",
        "/camera_profile/look" => "Creative look unavailable; shown without it.",
        "/lens/profile" => "Lens profile unavailable; shown without it.",
        "/effects/lens_blur" => "Lens Blur is not rendered on Smart Preview yet.",
        "/locals/retouch" => "Retouch is not rendered on Smart Preview yet.",
        "/locals/adjustments" => "Some local masks are unavailable; shown without them.",
        "/output/hdr" => "Rendered using the available Smart Preview dynamic range.",
        _ => "An optional setting is unavailable for this Smart Preview.",
    }
}

/// What an output rendered from an external Smart Preview could not
/// reproduce, as sentences: the source note, every planned-away setting and
/// the embedded-profile substitution note (Adobe process). Empty otherwise.
pub fn proxy_notes(
    source: &RenderSource<'_>,
    recipe: &Recipe,
    mask_support: bool,
    retouch: bool,
) -> Vec<String> {
    let RenderSource::CameraLinear(proxy) = source else {
        return Vec::new();
    };
    if !proxy.is_external_dng() {
        return Vec::new();
    }
    let mut notes = vec![
        "Exported from a Smart Preview proxy at its available resolution; the original was not used."
            .to_owned(),
    ];
    let mut fields = proxy
        .render_plan_with_resources(&recipe.settings, true, mask_support, retouch)
        .1
        .into_iter()
        .map(proxy_notice_text)
        .collect::<Vec<_>>();
    fields.dedup();
    notes.extend(
        fields
            .into_iter()
            .map(|text| format!("Info: {text} Saved settings are unchanged.")),
    );
    if recipe.process_version.family == engine_api::recipe::ProcessFamily::Adobe
        && let (_, Some(note)) = image_core::pipeline_adobe::embedded_profile_fallback(
            proxy,
            &recipe.settings,
            proxy.embedded_profile(),
        )
    {
        notes.push(format!("Info: {note}"));
    }
    notes
}

/// See [`render_pixels`].
#[derive(Clone, Copy, Debug)]
pub struct RenderRequest {
    pub color_space: ColorSpace,
    pub resize: Resize,
    pub sharpen_for: SharpenFor,
    pub scale: u32,
}

/// Built-in document profile ICC bytes (for tagging rendered pixels).
pub fn color_space_icc(space: ColorSpace) -> EngineResult<Vec<u8>> {
    let mut registry = color_mgmt::Registry::new();
    Ok(codec::profile(&mut registry, space)?.icc_bytes().to_vec())
}

/// Adapt only external proxy pixels; keep the authoritative recipe for metadata.
fn proxy_recipe<'a>(
    source: &RenderSource<'_>,
    recipe: &'a Recipe,
    depth: bool,
    retouch: bool,
) -> std::borrow::Cow<'a, Recipe> {
    match source {
        RenderSource::CameraLinear(proxy) if proxy.is_external_dng() => {
            let mut drawn = recipe.clone();
            drawn.settings = proxy
                .render_plan_with_resources(&recipe.settings, true, depth, retouch)
                .0;
            std::borrow::Cow::Owned(drawn)
        }
        _ => std::borrow::Cow::Borrowed(recipe),
    }
}

/// Generated proxies cannot stand in for full-quality originals.
fn require_full_quality_source(source: &RenderSource<'_>) -> EngineResult<()> {
    if matches!(source, RenderSource::CameraLinear(proxy) if !proxy.is_external_dng()) {
        return Err(original_required());
    }
    Ok(())
}

fn original_required() -> EngineError {
    EngineError::Unsupported {
        what: "full-quality export: original required; Smart Preview pixels cannot be exported"
            .into(),
    }
}

fn source_orientation(source: &RenderSource<'_>) -> u16 {
    match source {
        RenderSource::Cfa { metadata, .. } => metadata.orientation,
        RenderSource::Rgb(_) => 1,
        RenderSource::StoredRgb { orientation, .. } => *orientation,
        RenderSource::CameraLinear(proxy) => proxy.original_metadata().orientation,
    }
}

/// Sensor → display orientation (EXIF 1–8).
fn orient(rgb: image::Rgb32FImage, orientation: u16) -> image::Rgb32FImage {
    use image::imageops::*;
    match orientation {
        2 => flip_horizontal(&rgb),
        3 => rotate180(&rgb),
        4 => flip_vertical(&rgb),
        5 => rotate90(&flip_vertical(&rgb)),
        6 => rotate90(&rgb),
        7 => rotate90(&flip_horizontal(&rgb)),
        8 => rotate270(&rgb),
        _ => rgb,
    }
}

fn prepare(
    image: &ExportImage<'_>,
    recipe: &Recipe,
    settings: &ExportSettings,
    cancel: &CancellationToken,
) -> EngineResult<PreparedExport> {
    prepare_enhanced(image, recipe, settings, cancel, None)
}

/// Export with an explicitly loaded x2/x4 model, before resize and output
/// sharpening. Existing export settings and the enhance-off path are unchanged.
/// Loading/downloading weights is the caller's responsibility, never an
/// implicit effect of an ordinary export.
pub fn export_one_upscaled(
    image: &ExportImage<'_>,
    recipe: &Recipe,
    settings: &ExportSettings,
    upscale: &mut ml_enhance::SuperResolution,
) -> EngineResult<PathBuf> {
    let cancel = CancellationToken::new();
    prepare_enhanced(image, recipe, settings, &cancel, Some(upscale))?.commit(&cancel)
}

fn prepare_enhanced(
    image: &ExportImage<'_>,
    recipe: &Recipe,
    settings: &ExportSettings,
    cancel: &CancellationToken,
    upscale: Option<&mut ml_enhance::SuperResolution>,
) -> EngineResult<PreparedExport> {
    prepare_with_segmenter(image, recipe, settings, cancel, upscale, None)
}

/// Export with a caller-owned segmentation backend. No global backend is installed.
pub fn export_one_with_segmenter(
    image: &ExportImage<'_>,
    recipe: &Recipe,
    settings: &ExportSettings,
    segmenter: &mut dyn mask_ai::MaskSegmenter,
) -> EngineResult<PathBuf> {
    let cancel = CancellationToken::new();
    prepare_with_segmenter(image, recipe, settings, &cancel, None, Some(segmenter))?.commit(&cancel)
}

/// Enhanced export using the same segmentation backend and pre-local rasters.
pub fn export_one_upscaled_with_segmenter(
    image: &ExportImage<'_>,
    recipe: &Recipe,
    settings: &ExportSettings,
    upscale: &mut ml_enhance::SuperResolution,
    segmenter: &mut dyn mask_ai::MaskSegmenter,
) -> EngineResult<PathBuf> {
    let cancel = CancellationToken::new();
    prepare_with_segmenter(
        image,
        recipe,
        settings,
        &cancel,
        Some(upscale),
        Some(segmenter),
    )?
    .commit(&cancel)
}

fn prepare_with_segmenter(
    image: &ExportImage<'_>,
    recipe: &Recipe,
    settings: &ExportSettings,
    cancel: &CancellationToken,
    upscale: Option<&mut ml_enhance::SuperResolution>,
    segmenter: Option<&mut dyn mask_ai::MaskSegmenter>,
) -> EngineResult<PreparedExport> {
    encode_rendered(
        render_one_cancellable(image, recipe, settings, cancel, upscale, segmenter)?,
        cancel,
    )
}

/// An owned output frame. Send it to an encoder thread while rendering the
/// next image. No source pixels, model sessions, or GPU allocations are held.
pub struct RenderedExport {
    warnings: Vec<String>,
    used_gpu: bool,
    rgb: image::Rgb32FImage,
    packet: Option<XmpPacket>,
    native: native::Native,
    settings: ExportSettings,
    path: PathBuf,
    side_path: PathBuf,
}

impl RenderedExport {
    /// Recoverable rendering omissions, also persisted beside a committed output.
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// True only when this frame actually completed resident GPU rendering.
    pub fn used_gpu(&self) -> bool {
        self.used_gpu
    }
    /// Encode and atomically publish. On cancellation temporary files are
    /// dropped; existing destinations are never overwritten.
    pub fn finish(self, cancel: &CancellationToken) -> EngineResult<PathBuf> {
        let started = std::time::Instant::now();
        let path = encode_rendered(self, cancel)?.commit(cancel);
        gpu::trace("encode + commit", started);
        path
    }
}

/// The render half of [`export_one_cancellable`], with no destination writes.
/// Model manifests and derived depth caches may be populated in app support.
/// Callers must bound admission (one encoder plus one renderer is sufficient).
pub fn render_one_cancellable(
    image: &ExportImage<'_>,
    recipe: &Recipe,
    settings: &ExportSettings,
    cancel: &CancellationToken,
    upscale: Option<&mut ml_enhance::SuperResolution>,
    segmenter: Option<&mut dyn mask_ai::MaskSegmenter>,
) -> EngineResult<RenderedExport> {
    require_full_quality_source(&image.source)?;
    cancel.check()?;
    Sidecar::ensure_destination(&settings.output_dir, "export")?;
    recipe.validate()?;
    let proxy_warnings = proxy_notes(
        &image.source,
        recipe,
        settings.mask_support.is_some(),
        settings.retouch.is_some(),
    );
    let metadata_recipe = recipe;
    let planned = proxy_recipe(
        &image.source,
        recipe,
        settings.mask_support.is_some(),
        settings.retouch.is_some(),
    );
    let recipe = planned.as_ref();
    settings.format.validate()?;
    dng::validate(settings)?;
    hdr::validate(settings)?;
    if settings.hdr.is_some()
        && (upscale.is_some()
            || ai_masks::active(&recipe.settings)
            || depth::active(&image.source, &recipe.settings))
    {
        return Err(encode_error(
            "HDR export does not support SDR enhancement hooks",
        ));
    }
    if matches!(settings.format, Format::Dng) && settings.watermark.is_some() {
        return Err(encode_error(
            "DNG watermark compositing in linear colour is not supported",
        ));
    }
    if let Some(mark) = &settings.watermark {
        mark.validate()?;
    }
    if settings.max_file_bytes.is_some()
        && (!matches!(settings.format, Format::Jpeg { .. }) || settings.max_file_bytes == Some(0))
    {
        return Err(EngineError::invalid(
            "max_file_bytes",
            "positive JPEG-only byte budget required",
        ));
    }
    let path = settings.output_dir.join(filename(
        &settings.naming,
        image.name,
        image.sequence,
        image.date,
        settings.format.extension(),
    )?);
    let side_path = Sidecar::paths(&path).xmp;
    if warning_path(&path).try_exists().map_err(encode_error)? {
        return Err(EngineError::invalid(
            "output",
            "destination warning report already exists",
        ));
    }
    if path
        .try_exists()
        .map_err(|e| EngineError::io_at(&path, &e))?
        || side_path
            .try_exists()
            .map_err(|e| EngineError::io_at(&side_path, &e))?
    {
        return Err(EngineError::invalid("output", "destination already exists"));
    }
    let mut native = settings
        .metadata_sources
        .get(&image.sequence)
        .map(|path| native::Native::read(path, cancel))
        .transpose()?
        .unwrap_or_default();
    let packet = metadata_packet(image, metadata_recipe, settings, &native)?;
    native.filter(settings, packet.as_ref())?;
    let needs_hooks = depth::active(&image.source, &recipe.settings);
    let mut warnings = proxy_warnings;
    // Develop-renderer recipes (every Adobe-process recipe) never take the
    // resident GPU path, which implements the current Native process only.
    let gpu_pixels = if settings.hdr.is_none()
        && !uses_develop_renderer(&image.source, recipe)
        && !matches!(settings.format, Format::Dng)
        && upscale.is_none()
        && !needs_hooks
        && recipe.settings.locals.retouch.is_empty()
        && !ai_masks::active(&recipe.settings)
        && std::env::var("TESSERA_EXPORT_BACKEND").as_deref() != Ok("cpu")
    {
        let resize = match settings.resize {
            Resize::Fit(w, h)
                if settings.apply_orientation && source_orientation(&image.source) >= 5 =>
            {
                Resize::Fit(h, w)
            }
            other => other,
        };
        gpu::render_resized(
            image,
            recipe,
            settings.color_space,
            settings.render_scale,
            cancel,
            gpu::BUDGET,
            resize,
        )?
    } else {
        None
    };
    let already_resized = gpu_pixels.is_some();
    let mut used_gpu = already_resized;
    let started = std::time::Instant::now();
    let rgb = if settings.hdr.is_some() {
        hdr::render(image, recipe, settings, cancel, &mut warnings)?
    } else if uses_develop_renderer(&image.source, recipe) {
        let rgb = ai_masks::render_develop(
            &image.source,
            recipe,
            if upscale.is_some() {
                1
            } else {
                settings.render_scale
            },
            segmenter,
            &mut warnings,
            settings.mask_support.as_deref(),
            settings.retouch.clone(),
            cancel,
        )?;
        let rgb = match upscale {
            Some(model) => upscale_rgb(rgb, model)?,
            None => rgb,
        };
        if matches!(settings.format, Format::Dng) {
            rgb
        } else {
            encode_output_profile(rgb, recipe, settings.color_space)?
        }
    } else if !recipe.settings.locals.retouch.is_empty() {
        let rgb = retouch_float(
            image,
            recipe,
            if upscale.is_some() {
                1
            } else {
                settings.render_scale
            },
            settings.retouch.clone(),
        )?;
        let rgb = match upscale {
            Some(model) => upscale_rgb(rgb, model)?,
            None => rgb,
        };
        if matches!(settings.format, Format::Dng) {
            rgb
        } else {
            encode_output_profile(rgb, recipe, settings.color_space)?
        }
    } else if matches!(settings.format, Format::Dng) {
        let rgb = if ai_masks::active(&recipe.settings) {
            ai_masks::render_with_hooks(
                &image.source,
                &recipe.settings,
                segmenter,
                None,
                None,
                &mut warnings,
                settings.mask_support.as_deref(),
            )?
        } else {
            render_full_float(image, recipe)?
        };
        match upscale {
            Some(model) => upscale_rgb(rgb, model)?,
            None => rgb,
        }
    } else if let Some(rgb) = gpu_pixels {
        rgb
    } else if needs_hooks {
        if !matches!(settings.render_scale, 1 | 2 | 4 | 8) {
            return Err(EngineError::invalid("render_scale", "must be 1, 2, 4 or 8"));
        }
        let scale = if upscale.is_some() || ai_masks::active(&recipe.settings) {
            1
        } else {
            settings.render_scale
        };
        let resident =
            if recipe.process_version == engine_api::recipe::ProcessVersion::NATIVE_CURRENT {
                depth::try_resident(&image.source, &recipe.settings, scale, cancel)?
            } else {
                None
            };
        let (rgb, notices) = match resident {
            Some(rgb) => {
                used_gpu = true;
                (rgb, Vec::new())
            }
            None => depth::render(
                &image.source,
                &recipe.settings,
                scale,
                &settings
                    .mask_support
                    .clone()
                    .map_or_else(depth::support, Ok)?,
                segmenter,
                None,
            )?,
        };
        warnings.extend(notices);
        cancel.check()?;
        let rgb = match upscale {
            Some(model) => upscale_rgb(rgb, model)?,
            None => rgb,
        };
        encode_output_profile(rgb, recipe, settings.color_space)?
    } else if ai_masks::active(&recipe.settings) {
        let rgb = ai_masks::render_with_hooks(
            &image.source,
            &recipe.settings,
            segmenter,
            None,
            None,
            &mut warnings,
            settings.mask_support.as_deref(),
        )?;
        cancel.check()?;
        let rgb = match upscale {
            Some(model) => upscale_rgb(rgb, model)?,
            None => rgb,
        };
        encode_output_profile(rgb, recipe, settings.color_space)?
    } else if let Some(upscale) = upscale {
        let rgb = render_full_float(image, recipe)?;
        cancel.check()?;
        encode_output_profile(upscale_rgb(rgb, upscale)?, recipe, settings.color_space)?
    } else {
        if !matches!(settings.render_scale, 1 | 2 | 4 | 8) {
            return Err(EngineError::invalid("render_scale", "must be 1, 2, 4 or 8"));
        }
        render_scaled_cpu(image, recipe, settings.color_space, settings.render_scale)?
    };
    cancel.check()?;
    let rgb = if settings.apply_orientation {
        orient(rgb, source_orientation(&image.source))
    } else {
        rgb
    };
    let rgb = if already_resized {
        rgb
    } else {
        filter::resize(rgb, settings.resize, cancel)?
    };
    let mut rgb = sharpen_output(
        rgb,
        settings.sharpen_for,
        settings.sharpen_amount,
        settings.dpi.unwrap_or(300),
        cancel,
    )?;
    if settings.hdr.is_some() {
        hdr::finalize(&mut rgb, recipe, settings, cancel)?;
    }
    if let Some(mark) = &settings.watermark {
        apply_watermark(&mut rgb, mark, cancel)?;
    }
    // Every rendered output already contains these adjustments. If CRS
    // instructions survive in embedded or adjacent XMP, reopening the output
    // as a new source applies them again. Keep descriptive metadata; original
    // source-copy export has its own path and retains editable instructions.
    let packet = packet.map(|p| p.without_development()).transpose()?;
    gpu::trace("CPU render/orient/resize/sharpen", started);
    // ENG-7b/7c: a named lens profile that is not available is a per-file
    // warning. "No lens profile available" for an Auto raw is not: with no
    // lens database it is the normal case for nearly every raw, so as a
    // per-file warning it would write a warnings file beside every export and
    // drown real omissions (REV2 N-B2). Develop omits it for the same reason.
    let lens_metadata = match &image.source {
        RenderSource::Cfa { metadata, .. } => Some(*metadata),
        RenderSource::CameraLinear(proxy) => Some(proxy.original_metadata()),
        _ => None,
    };
    warnings.extend(
        pipeline_cpu::lens_notice(&recipe.settings.lens, lens_metadata, &Default::default())
            .filter(|n| matches!(n, pipeline_cpu::LensNotice::ProfileUnavailable { .. }))
            .map(|n| n.to_string()),
    );
    Ok(RenderedExport {
        warnings,
        used_gpu,
        rgb,
        packet,
        native,
        settings: settings.clone(),
        path,
        side_path,
    })
}

fn encode_rendered(
    rendered: RenderedExport,
    cancel: &CancellationToken,
) -> EngineResult<PreparedExport> {
    cancel.check()?;
    let RenderedExport {
        warnings,
        rgb,
        packet,
        native,
        settings,
        path,
        side_path,
        ..
    } = rendered;
    Sidecar::ensure_destination(&path, "export")?;
    Sidecar::ensure_destination(&side_path, "export")?;
    fs::create_dir_all(&settings.output_dir)
        .map_err(|e| EngineError::io_at(&settings.output_dir, &e))?;
    let mut temp = new_output_temp(&settings.output_dir)?;
    if settings.hdr.is_some() {
        hdr::encode(
            temp.as_file_mut(),
            &rgb,
            &settings,
            &native,
            packet.as_ref().map(XmpPacket::serialize),
            cancel,
        )?;
    } else {
        codec::encode_limited(
            temp.as_file_mut(),
            &rgb,
            codec::Encoding {
                format: settings.format,
                space: settings.color_space,
                dpi: settings.dpi,
                native: Some(&native),
            },
            packet.as_ref().map(XmpPacket::serialize),
            cancel,
            settings.max_file_bytes,
        )?;
    }
    if matches!(settings.format, Format::Dng) {
        dng::finish(temp.as_file_mut(), settings.original_raw.as_deref(), cancel)?;
    }
    if matches!(settings.format, Format::Dng | Format::Tiff { .. }) {
        native::append_tiff(temp.as_file_mut(), &native)?;
    }
    temp.as_file().sync_all().map_err(encode_error)?;
    let side_temp = if let Some(packet) = &packet {
        let mut temp = new_output_temp(&settings.output_dir)?;
        temp.write_all(packet.serialize().as_bytes())
            .map_err(encode_error)?;
        temp.as_file().sync_all().map_err(encode_error)?;
        Some(temp)
    } else {
        None
    };
    let warning_temp = if warnings.is_empty() {
        None
    } else {
        let mut temp = new_output_temp(&settings.output_dir)?;
        for warning in &warnings {
            writeln!(temp, "{warning}").map_err(encode_error)?;
        }
        temp.as_file().sync_all().map_err(encode_error)?;
        Some(temp)
    };
    cancel.check()?;
    Ok(PreparedExport {
        temp,
        side_temp,
        warning_temp,
        warning_path: warning_path(&path),
        path,
        side_path,
    })
}

/// A temporary file that becomes an output: readable like any exported
/// document (0644), not the 0600 of a private temporary file.
fn new_output_temp(dir: &std::path::Path) -> EngineResult<tempfile::NamedTempFile> {
    Sidecar::ensure_destination(dir, "export")?;
    let temp = tempfile::NamedTempFile::new_in(dir).map_err(encode_error)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temp.as_file()
            .set_permissions(fs::Permissions::from_mode(0o644))
            .map_err(encode_error)?;
    }
    Ok(temp)
}

fn upscale_rgb(
    rgb: image::Rgb32FImage,
    model: &mut ml_enhance::SuperResolution,
) -> EngineResult<image::Rgb32FImage> {
    let (width, height) = rgb.dimensions();
    let factor = u32::try_from(model.factor()).map_err(encode_error)?;
    let out_width = width
        .checked_mul(factor)
        .ok_or_else(|| encode_error("upscale width overflow"))?;
    let out_height = height
        .checked_mul(factor)
        .ok_or_else(|| encode_error("upscale height overflow"))?;
    let mut planar = Vec::with_capacity(rgb.as_raw().len());
    for c in 0..3 {
        // The restoration model requires bounded input. Clip only at its
        // linear Rec.2020 boundary, not through an intermediate sRGB gamut.
        planar.extend(rgb.pixels().map(|p| p[c].clamp(0.0, 1.0)));
    }
    let input = ml_runtime::Tensor::new(3, height as usize, width as usize, planar)
        .map_err(encode_error)?;
    let output = model
        .super_resolution(&input, model.factor())
        .map_err(encode_error)?;
    let n = output.data().len() / 3;
    Ok(image::Rgb32FImage::from_fn(
        out_width,
        out_height,
        |x, y| {
            let i = y as usize * out_width as usize + x as usize;
            image::Rgb(std::array::from_fn(|c| {
                output.data()[c * n + i].clamp(0.0, 1.0)
            }))
        },
    ))
}

fn warning_path(path: &std::path::Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".tessera-warnings.txt");
    PathBuf::from(name)
}

impl BatchReport {
    /// Recoverable warnings for successfully committed results, in input order.
    /// Reports are stored beside each image as `<filename>.tessera-warnings.txt`.
    /// An unreadable existing report is an error, never an empty warning list.
    pub fn warnings(&self) -> EngineResult<Vec<(usize, Vec<String>)>> {
        let mut warnings = Vec::new();
        for (index, result) in self.results.iter().enumerate() {
            let Ok(path) = result else { continue };
            let report = warning_path(path);
            match fs::read_to_string(&report) {
                Ok(text) => warnings.push((index, text.lines().map(str::to_owned).collect())),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(EngineError::io_at(&report, &error)),
            }
        }
        Ok(warnings)
    }
}

struct PreparedExport {
    temp: tempfile::NamedTempFile,
    side_temp: Option<tempfile::NamedTempFile>,
    warning_temp: Option<tempfile::NamedTempFile>,
    path: PathBuf,
    side_path: PathBuf,
    warning_path: PathBuf,
}
impl PreparedExport {
    fn commit(self, cancel: &CancellationToken) -> EngineResult<PathBuf> {
        cancel.check()?;
        Sidecar::ensure_destination(&self.path, "export")?;
        Sidecar::ensure_destination(&self.side_path, "export")?;
        Sidecar::ensure_destination(&self.warning_path, "export")?;
        let Self {
            temp,
            side_temp,
            warning_temp,
            path,
            side_path,
            warning_path,
        } = self;
        let has_warning = warning_temp.is_some();
        let has_sidecar = side_temp.is_some();
        // Deliberately non-cancellable commit. Publish the image last,
        // rolling back our sidecar on failure. Never overwrite user files.
        if let Some(temp) = warning_temp {
            temp.persist_noclobber(&warning_path)
                .map_err(encode_error)?;
        }
        if let Some(temp) = side_temp
            && let Err(error) = temp.persist_noclobber(&side_path)
        {
            if has_warning {
                fs::remove_file(&warning_path).map_err(encode_error)?;
            }
            return Err(encode_error(error));
        }
        if let Err(e) = temp.persist_noclobber(&path) {
            if has_warning {
                fs::remove_file(&warning_path).map_err(encode_error)?;
            }
            if has_sidecar {
                fs::remove_file(&side_path).map_err(|e| EngineError::io_at(&side_path, &e))?;
            }
            return Err(encode_error(e));
        }
        Ok(path)
    }
}

fn metadata_packet(
    image: &ExportImage<'_>,
    recipe: &Recipe,
    settings: &ExportSettings,
    native: &native::Native,
) -> EngineResult<Option<XmpPacket>> {
    use sidecar::ExportMetadataPolicy as Policy;
    let policy = match settings.metadata {
        Metadata::None => return Ok(None),
        Metadata::All => Policy::All,
        Metadata::CopyrightOnly => Policy::CopyrightOnly,
        Metadata::CopyrightAndContact => Policy::CopyrightAndContact,
        Metadata::AllExceptCamera => Policy::AllExceptCamera,
    };
    let preset = MarkPreset::lightroom();
    // Native fields are source context. Reconcile them before applying edits,
    // so an explicit empty/replaced sidecar property remains authoritative.
    let keywords = native.keywords()?;
    let packet = match image.metadata {
        // With no native context, preserve the caller's packet structure.
        Some(sidecar) if native.xmp.is_none() && keywords.is_empty() => sidecar.clone(),
        sidecar => {
            let packet = native
                .xmp
                .clone()
                .unwrap_or_else(|| XmpPacket::from_selection(&recipe.selection, &preset))
                .with_native_keywords(&keywords)?;
            match sidecar {
                Some(sidecar) => packet.with_sidecar_overrides(sidecar)?,
                None => packet,
            }
        }
    };
    let packet = packet.with_selection(&recipe.selection, &preset)?;
    Ok(Some(packet.for_export_with_person_source(
        policy,
        settings.remove_person_info,
        settings.remove_location,
        settings.keywords_as_hierarchy,
        native.xmp.as_ref(),
    )?))
}

/// Expand a basename template. Sequence is caller supplied and one-based in batches.
/// Date is supplied capture-date text, making names stable across resumed runs.
pub fn filename(
    template: &str,
    name: &str,
    seq: usize,
    date: &str,
    extension: &str,
) -> EngineResult<String> {
    let mut result = String::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        result.push_str(&rest[..start]);
        let end = rest[start..]
            .find('}')
            .ok_or_else(|| EngineError::invalid("naming", "unclosed token"))?
            + start;
        match &rest[start..=end] {
            "{name}" => result.push_str(name),
            "{seq}" => result.push_str(&seq.to_string()),
            "{date}" => result.push_str(date),
            _ => return Err(EngineError::invalid("naming", "unknown token")),
        }
        rest = &rest[end + 1..];
    }
    result.push_str(rest);
    if result.is_empty()
        || result == "."
        || result == ".."
        || result
            .chars()
            .any(|c| c.is_control() || "/\\:{}".contains(c))
        || !extension.chars().all(|c| c.is_ascii_alphanumeric())
    {
        return Err(EngineError::invalid("naming", "not a safe filename"));
    }
    Ok(format!("{result}.{extension}"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn warning_publication_collision_never_overwrites_existing_report() {
        use super::*;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.png");
        let warnings = warning_path(&path);
        fs::write(&warnings, b"existing").unwrap();
        let mut temp = new_output_temp(dir.path()).unwrap();
        temp.write_all(b"image").unwrap();
        let mut warning_temp = new_output_temp(dir.path()).unwrap();
        warning_temp.write_all(b"new warning").unwrap();
        let prepared = PreparedExport {
            temp,
            side_temp: None,
            warning_temp: Some(warning_temp),
            path: path.clone(),
            side_path: dir.path().join("out.xmp"),
            warning_path: warnings.clone(),
        };
        assert!(prepared.commit(&CancellationToken::new()).is_err());
        assert!(!path.exists());
        assert_eq!(fs::read(warnings).unwrap(), b"existing");
    }

    #[test]
    fn warning_report_survives_commit_and_preserves_success() {
        use super::*;
        let pixels = pipeline_cpu::Image::new(8, 6, vec![vec![0.18; 48]; 3]).unwrap();
        let image = ExportImage {
            source: RenderSource::Rgb(&pixels),
            name: "warning",
            sequence: 1,
            date: "",
            metadata: None,
        };
        let dir = tempfile::tempdir().unwrap();
        let settings = ExportSettings {
            output_dir: dir.path().into(),
            format: Format::Png,
            metadata: Metadata::None,
            ..Default::default()
        };
        let token = CancellationToken::new();
        let mut rendered =
            render_one_cancellable(&image, &Recipe::default(), &settings, &token, None, None)
                .unwrap();
        rendered
            .warnings
            .push("Lens Blur skipped: depth model is not cached".into());
        assert_eq!(rendered.warnings().len(), 1);
        let path = rendered.finish(&token).unwrap();
        assert!(path.exists());
        let report = BatchReport {
            results: vec![Ok(path)],
        };
        assert_eq!(
            report.warnings().unwrap(),
            vec![(
                0,
                vec!["Lens Blur skipped: depth model is not cached".to_string()]
            )]
        );
        assert!(report.remaining().is_empty());
    }
    #[test]
    fn ai_subject_export_changes_pixels() {
        use super::*;
        use engine_api::recipe::mask::{LocalAdjustment, LocalParams, MaskComponent, MaskKind};
        let pixels = pipeline_cpu::Image::new(8, 6, vec![vec![0.18; 48]; 3]).unwrap();
        let image = ExportImage {
            source: RenderSource::Rgb(&pixels),
            name: "subject",
            sequence: 1,
            date: "",
            metadata: None,
        };
        let dir = tempfile::tempdir().unwrap();
        let settings = ExportSettings {
            output_dir: dir.path().into(),
            metadata: Metadata::None,
            ..Default::default()
        };
        let mut recipe = Recipe::default();
        recipe
            .edit(engine_api::recipe::EditMeta::user("subject", 0), |s| {
                s.locals.adjustments.push(LocalAdjustment {
                    components: vec![MaskComponent::new(MaskKind::Subject { model: None })],
                    params: LocalParams {
                        exposure: 1.0,
                        ..Default::default()
                    },
                    ..Default::default()
                })
            })
            .unwrap();
        struct Subject;
        impl mask_ai::MaskSegmenter for Subject {
            fn segment(
                &mut self,
                image: &image::RgbImage,
                request: &mask_ai::SegmentRequest,
            ) -> anyhow::Result<Vec<f32>> {
                assert_eq!(*request, mask_ai::SegmentRequest::Subject);
                Ok((0..image.width() * image.height())
                    .map(|i| {
                        if i % image.width() < image.width() / 2 {
                            1.0
                        } else {
                            0.0
                        }
                    })
                    .collect())
            }
        }
        let output = export_one_with_segmenter(&image, &recipe, &settings, &mut Subject);
        assert!(output.is_ok(), "AI mask export must render: {output:?}");
        let actual = image::open(output.unwrap()).unwrap().to_rgb8();
        assert!(actual.get_pixel(1, 2)[0] > actual.get_pixel(6, 2)[0] + 20);
        let baseline_settings = ExportSettings {
            naming: "baseline".into(),
            ..settings
        };
        let baseline = export_one(&image, &Recipe::default(), &baseline_settings).unwrap();
        let baseline = image::open(baseline).unwrap().to_rgb8();
        assert_ne!(actual, baseline);
        assert!((actual.get_pixel(6, 2)[0] as i16 - baseline.get_pixel(6, 2)[0] as i16).abs() < 3);
        let enhanced_input =
            ai_masks::render(&image.source, &recipe.settings, Some(&mut Subject)).unwrap();
        assert!(enhanced_input.get_pixel(1, 2)[0] > enhanced_input.get_pixel(6, 2)[0]);
    }

    #[test]
    fn enhancement_render_does_not_quantize_model_input() {
        let pixels = pipeline_cpu::Image::new(8, 6, vec![vec![0.18; 48]; 3]).unwrap();
        let image = super::ExportImage {
            source: pipeline_cpu::RenderSource::Rgb(&pixels),
            name: "float",
            sequence: 1,
            date: "20260925",
            metadata: None,
        };
        let output = super::render_full_float(&image, &Default::default()).unwrap();
        assert_eq!(output.dimensions(), (8, 6));
        assert!(
            output
                .as_raw()
                .iter()
                .any(|v| (v * 255.0 - (v * 255.0).round()).abs() > 0.01)
        );
        // No ordered dither should be injected ahead of the restoration net.
        assert!(output.pixels().all(|p| p == output.get_pixel(0, 0)));
        // Middle grey stays linear, rather than being sRGB-encoded before SR.
        assert!((output.get_pixel(0, 0)[0] - 0.18).abs() < 1e-5);
    }

    #[test]
    fn enhancement_output_matches_managed_render_for_every_profile() {
        use super::*;
        let pixels =
            pipeline_cpu::Image::new(8, 6, vec![vec![0.4; 48], vec![0.1; 48], vec![0.02; 48]])
                .unwrap();
        let image = ExportImage {
            source: RenderSource::Rgb(&pixels),
            name: "managed",
            sequence: 1,
            date: "",
            metadata: None,
        };
        let mut recipe = Recipe::default();
        // Both export paths ignore a display-only proof, even if unresolved.
        recipe.settings.output.proof_profile = Some(
            engine_api::color::IccProfileHandle::from_profile_bytes(b"display-only proof"),
        );
        let linear = render_full_float(&image, &recipe).unwrap();
        let mut outputs = Vec::new();
        for space in [
            ColorSpace::Srgb,
            ColorSpace::DisplayP3,
            ColorSpace::Rec2020,
            ColorSpace::ProPhoto,
        ] {
            let direct = render_full(&image, &recipe, space).unwrap();
            // Stand in for an identity enhancement to isolate the boundary:
            // no second tone curve, no intermediate sRGB, no double encoding.
            let enhanced = encode_output_profile(linear.clone(), &recipe, space).unwrap();
            assert_eq!(direct, enhanced);
            outputs.push(enhanced);
        }
        assert!(outputs.windows(2).all(|pair| pair[0] != pair[1]));
    }

    #[test]
    fn naming_tokens_and_path_safety() {
        assert_eq!(
            super::filename("{name}-{seq}-{date}", "DSC_001", 7, "20260925", "jpg").unwrap(),
            "DSC_001-7-20260925.jpg"
        );
        for template in ["../{name}", "{unknown}", "", "/absolute", "a\\b"] {
            assert!(super::filename(template, "photo", 1, "20260925", "jpg").is_err());
        }
    }
}

#[cfg(test)]
mod lightroom_safety_tests {
    use super::*;

    #[test]
    fn original_export_never_creates_files_in_lightroom_owned_directory() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.nef");
        fs::write(&source, b"original raw bytes").unwrap();
        let protected = root.path().join("X.lrdata");
        fs::create_dir(&protected).unwrap();
        let destination = protected.join("copy.nef");
        fs::create_dir_all(Sidecar::paths(&destination).xmp.parent().unwrap()).unwrap();
        assert!(
            export_original(&source, &destination, None, None, &CancellationToken::new()).is_err()
        );
        assert_eq!(fs::read_dir(protected).unwrap().count(), 0);
        assert_eq!(fs::read(source).unwrap(), b"original raw bytes");
    }

    #[test]
    fn rendered_export_rejects_lightroom_owned_directory_before_creation() {
        let root = tempfile::tempdir().unwrap();
        let protected = root.path().join("Foo.lrcat-data");
        let pixels = pipeline_cpu::Image::new(8, 6, vec![vec![0.18; 48]; 3]).unwrap();
        let image = ExportImage {
            source: RenderSource::Rgb(&pixels),
            name: "photo",
            sequence: 1,
            date: "",
            metadata: None,
        };
        let settings = ExportSettings {
            output_dir: protected.clone(),
            format: Format::Png,
            metadata: Metadata::None,
            ..Default::default()
        };
        assert!(export_one(&image, &Recipe::default(), &settings).is_err());
        assert!(!protected.exists());
    }
}
