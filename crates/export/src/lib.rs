//! Full-resolution image export.
mod batch;
mod codec;
pub use batch::{BatchReport, ExportItem, Progress, export_batch, export_batch_with_jobs};
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
    let rgb = render_full(image, recipe, settings.color_space)?;
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
