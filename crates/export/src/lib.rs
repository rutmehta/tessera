//! Full-resolution image export.
mod batch;
mod codec;
pub use batch::{
    BatchReport, ExportItem, Progress, export_batch, export_batch_upscaled, export_batch_with_jobs,
};
mod filter;
use engine_api::{EngineError, EngineResult};
use engine_api::{jobs::CancellationToken, recipe::Recipe};
pub use filter::{Resize, SharpenFor};
use pipeline_cpu::RenderSource;
use sidecar::{MarkPreset, Sidecar, XmpPacket};
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
    Jpeg { quality: u8 },
    Png,
    Tiff { bits: u8 },
}
impl Format {
    fn extension(self) -> &'static str {
        match self {
            Self::Jpeg { .. } => "jpg",
            Self::Png => "png",
            Self::Tiff { .. } => "tif",
        }
    }
    fn validate(self) -> EngineResult<()> {
        match self {
            Self::Jpeg { quality: 1..=100 } | Self::Png | Self::Tiff { bits: 8 | 16 } => Ok(()),
            _ => Err(EngineError::invalid(
                "format",
                "JPEG quality must be 1..=100; TIFF bits must be 8 or 16",
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
    None,
}

#[derive(Clone, Debug)]
pub struct ExportSettings {
    pub format: Format,
    pub color_space: ColorSpace,
    pub metadata: Metadata,
    pub resize: Resize,
    pub sharpen_for: SharpenFor,
    pub naming: String,
    pub output_dir: PathBuf,
}
impl Default for ExportSettings {
    fn default() -> Self {
        Self {
            format: Format::Jpeg { quality: 90 },
            color_space: ColorSpace::Srgb,
            metadata: Metadata::All,
            resize: Resize::None,
            sharpen_for: SharpenFor::None,
            naming: "{name}-{seq}".into(),
            output_dir: ".".into(),
        }
    }
}

/// Render directly to the document profile in float, without an sRGB intermediate.
fn render_full(
    image: &ExportImage<'_>,
    recipe: &Recipe,
    space: ColorSpace,
) -> EngineResult<image::Rgb32FImage> {
    let mut registry = color_mgmt::Registry::new();
    let target = codec::profile(&mut registry, space)?;
    let mut settings = recipe.settings.clone();
    // Proofing is a display-only preview, never baked into a file export.
    settings.output.proof_profile = None;
    Ok(pipeline_cpu::render_managed_scaled(
        &settings,
        &image.source,
        1,
        &mut pipeline_cpu::OutputContext {
            registry: &mut registry,
            target: pipeline_cpu::OutputTarget::Export(&target),
            proof: None,
            options: color_mgmt::TransformOptions::default(),
        },
    )?
    .pixels)
}

/// Enhancement input is tone-mapped linear Rec.2020, never encoded sRGB.
fn render_full_float(image: &ExportImage<'_>, recipe: &Recipe) -> EngineResult<image::Rgb32FImage> {
    pipeline_cpu::render_output_linear_scaled(&recipe.settings, &image.source, 1)
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
    cancel.check()?;
    recipe.validate()?;
    settings.format.validate()?;
    let path = settings.output_dir.join(filename(
        &settings.naming,
        image.name,
        image.sequence,
        image.date,
        settings.format.extension(),
    )?);
    let side_path = Sidecar::paths(&path).xmp;
    if path
        .try_exists()
        .map_err(|e| EngineError::io_at(&path, &e))?
        || side_path
            .try_exists()
            .map_err(|e| EngineError::io_at(&side_path, &e))?
    {
        return Err(EngineError::invalid("output", "destination already exists"));
    }
    let rgb = if let Some(upscale) = upscale {
        let rgb = render_full_float(image, recipe)?;
        cancel.check()?;
        encode_output_profile(upscale_rgb(rgb, upscale)?, recipe, settings.color_space)?
    } else {
        render_full(image, recipe, settings.color_space)?
    };
    cancel.check()?;
    let rgb = filter::resize(rgb, settings.resize, cancel)?;
    let rgb = filter::sharpen(rgb, settings.sharpen_for, cancel)?;
    let packet = metadata_packet(image, recipe, settings.metadata)?;
    fs::create_dir_all(&settings.output_dir)
        .map_err(|e| EngineError::io_at(&settings.output_dir, &e))?;
    let mut temp = tempfile::NamedTempFile::new_in(&settings.output_dir).map_err(encode_error)?;
    codec::encode(
        temp.as_file_mut(),
        &rgb,
        settings.format,
        settings.color_space,
        packet.as_ref().map(XmpPacket::serialize),
        cancel,
    )?;
    temp.as_file().sync_all().map_err(encode_error)?;
    let side_temp = if let Some(packet) = &packet {
        let mut temp =
            tempfile::NamedTempFile::new_in(&settings.output_dir).map_err(encode_error)?;
        temp.write_all(packet.serialize().as_bytes())
            .map_err(encode_error)?;
        temp.as_file().sync_all().map_err(encode_error)?;
        Some(temp)
    } else {
        None
    };
    cancel.check()?;
    Ok(PreparedExport {
        temp,
        side_temp,
        path,
        side_path,
    })
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

struct PreparedExport {
    temp: tempfile::NamedTempFile,
    side_temp: Option<tempfile::NamedTempFile>,
    path: PathBuf,
    side_path: PathBuf,
}
impl PreparedExport {
    fn commit(self, cancel: &CancellationToken) -> EngineResult<PathBuf> {
        cancel.check()?;
        let Self {
            temp,
            side_temp,
            path,
            side_path,
        } = self;
        let has_sidecar = side_temp.is_some();
        // Deliberately non-cancellable commit. Publish the image last,
        // rolling back our sidecar on failure. Never overwrite user files.
        if let Some(temp) = side_temp {
            temp.persist_noclobber(&side_path).map_err(encode_error)?;
        }
        if let Err(e) = temp.persist_noclobber(&path) {
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
    policy: Metadata,
) -> EngineResult<Option<XmpPacket>> {
    use engine_api::recipe::Selection;
    let preset = MarkPreset::lightroom();
    let packet = match policy {
        Metadata::None => return Ok(None),
        Metadata::All => {
            let packet = image
                .metadata
                .cloned()
                .unwrap_or_else(|| XmpPacket::from_selection(&recipe.selection, &preset));
            packet.with_metadata(&recipe.selection, &packet.metadata()?, &preset)?
        }
        Metadata::CopyrightOnly => {
            let metadata = sidecar::Metadata {
                copyright: image
                    .metadata
                    .map(XmpPacket::metadata)
                    .transpose()?
                    .unwrap_or_default()
                    .copyright,
                ..Default::default()
            };
            XmpPacket::from_selection(&Selection::default(), &preset).with_metadata(
                &Selection::default(),
                &metadata,
                &preset,
            )?
        }
    };
    Ok(Some(packet))
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
