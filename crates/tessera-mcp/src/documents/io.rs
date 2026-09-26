//! Opening documents (`.tessera-doc`, PSD/PSB, flat images) and
//! `export_document`.
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use color_mgmt::{Builtin, Registry};
use compositor::{Compositor, Depth, DocState, Document, Layer, Rect};
use engine_api::document::{DocumentExportSettings, DocumentFormat};
use engine_api::id::{DocumentId, JobId};
use engine_api::tile::Extent;
use engine_api::tools::{DocumentToolOutput, ExportFormat, Resize};
use engine_api::{EngineError, EngineResult};
use image::{ImageDecoder, ImageEncoder};

use super::{DocumentSession, Documents, summary};

static NEXT_JOB: AtomicU64 = AtomicU64::new(1);

/// Export runs synchronously; the id is bookkeeping (the output variant is
/// named for a queued job).
pub(super) fn next_job() -> JobId {
    JobId(NEXT_JOB.fetch_add(1, Ordering::Relaxed))
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn decode(format: &str, e: impl std::fmt::Display) -> EngineError {
    EngineError::Decode {
        format: format.into(),
        message: e.to_string(),
    }
}

fn encode(format: &str, e: impl std::fmt::Display) -> EngineError {
    EngineError::Encode {
        format: format.into(),
        message: e.to_string(),
    }
}

fn absolute(path: &str, name: &str) -> EngineResult<PathBuf> {
    let p = PathBuf::from(path);
    if !p.is_absolute() {
        return Err(EngineError::invalid(name, "absolute path required"));
    }
    Ok(p)
}

impl Documents {
    pub(super) fn open(&mut self, path: &str) -> EngineResult<DocumentToolOutput> {
        let path = absolute(path, "path")?.canonicalize()?;
        let (doc, warnings) = open_file(&path)?;
        let id = DocumentId(self.next);
        self.next += 1;
        let summary = summary(doc.state())?;
        self.sessions
            .insert(id, DocumentSession::new(path, doc, warnings));
        Ok(DocumentToolOutput::DocumentOpened {
            document: id,
            summary,
        })
    }
}

/// Reads a document file: `.tessera-doc`, `.psd`/`.psb`, or a flat JPEG or
/// PNG image (one Background pixel layer; 16-bit PNGs open as 16-bit).
pub(crate) fn open_file(path: &Path) -> EngineResult<(Document, Vec<String>)> {
    match extension(path).as_str() {
        "tessera-doc" => Ok((compositor::format::load(path)?, Vec::new())),
        "psd" | "psb" => {
            let bytes = std::fs::read(path).map_err(|e| EngineError::io_at(path, &e))?;
            let source = ::psd::PsdDocument::read(&bytes).map_err(|e| decode("psd", e))?;
            // The preserving constructor retains opaque PSD records. The public
            // importer also exposes warnings; keep these on the session rather
            // than replacing the preserving document with a bare DocState.
            let warnings = compositor::psd::from_psd(&source)?.warnings;
            Ok((Document::from_psd(source)?, warnings))
        }
        "jpg" | "jpeg" | "png" => Ok((flat_image(path)?, Vec::new())),
        other => Err(crate::unsupported(format!(
            "open_document reads .tessera-doc, .psd, .psb, .jpg and .png (got `.{other}`)"
        ))),
    }
}

fn flat_image(path: &Path) -> EngineResult<Document> {
    let mut decoder = image::ImageReader::open(path)
        .map_err(|e| EngineError::io_at(path, &e))?
        .with_guessed_format()
        .map_err(|e| EngineError::io_at(path, &e))?
        .into_decoder()
        .map_err(|e| decode("image", e))?;
    // Decoding does not color-convert samples. Retain their profile so a later
    // export cannot silently relabel a tagged wide-gamut image as sRGB.
    let icc = decoder.icc_profile().map_err(|e| decode("icc", e))?;
    let img = image::DynamicImage::from_decoder(decoder).map_err(|e| decode("image", e))?;
    let (w, h) = (img.width(), img.height());
    let sixteen = matches!(
        img.color(),
        image::ColorType::L16
            | image::ColorType::La16
            | image::ColorType::Rgb16
            | image::ColorType::Rgba16
    );
    let (depth, px): (Depth, Vec<f32>) = if sixteen {
        let v = img.into_rgba16();
        (
            Depth::U16,
            v.into_raw()
                .into_iter()
                .map(|s| f32::from(s) / 65535.0)
                .collect(),
        )
    } else {
        let v = img.into_rgba8();
        (
            Depth::U8,
            v.into_raw()
                .into_iter()
                .map(|s| f32::from(s) / 255.0)
                .collect(),
        )
    };
    let extent = Extent::new(w, h);
    let mut state = DocState::new(extent, depth);
    state.profile = icc.map(|bytes| compositor::ColorProfile::from_icc("Embedded ICC", bytes));
    let mut layer = Layer::pixel("Background", extent, depth);
    layer.props.background = true;
    layer.id = engine_api::id::LayerId(1);
    state.next_id = 2;
    if let Some(r) = layer.raster_mut() {
        r.edit_region(Rect::of_extent(extent), 1, |x, y, p| {
            let i = (y as usize * w as usize + x as usize) * 4;
            p.copy_from_slice(&px[i..i + 4]);
        })?;
    }
    state.root.push(std::sync::Arc::new(layer));
    Ok(Document::new(state))
}

/// Writes a document's current state.
pub(super) fn export(
    session: &DocumentSession,
    settings: &DocumentExportSettings,
) -> EngineResult<()> {
    let path = absolute(&settings.path, "path")?;
    if let Some(parent) = path.parent()
        && !parent.is_dir()
    {
        return Err(EngineError::invalid(
            "path",
            "parent directory does not exist",
        ));
    }
    let doc = &session.doc;
    match &settings.format {
        DocumentFormat::TesseraDoc => compositor::format::save(doc, &path),
        DocumentFormat::Psd { .. } | DocumentFormat::Psb { .. } => {
            let mut psd = compositor::psd::to_psd(doc)?;
            if matches!(settings.format, DocumentFormat::Psb { .. }) {
                psd.version = ::psd::Version::Psb;
            }
            let bytes = psd.write().map_err(|e| encode("psd", e))?;
            write_atomic(&path, &bytes)
        }
        DocumentFormat::Image {
            encoding,
            resize,
            profile,
        } => {
            if profile.is_some() {
                return Err(crate::unsupported(
                    "output ICC profile handles are not resolved for document exports; omit profile to keep the document color space",
                ));
            }
            let (e, rgba) = Compositor::new(256 << 20).render_level_rgba(doc, 0)?;
            let mut float = doc.state().depth == Depth::F32;
            let (e, mut rgba) = resized(e, rgba, resize.as_ref())?;
            let mut registry = Registry::new();
            let icc = match &doc.state().profile {
                Some(profile) => {
                    let bytes = profile.icc.as_ref().ok_or_else(|| {
                        crate::unsupported("document ICC profile bytes are unavailable")
                    })?;
                    let profile = registry.load_bytes(bytes).map_err(|e| encode("icc", e))?;
                    if profile.icc_bytes().get(16..20) != Some(b"RGB ") {
                        return Err(crate::unsupported(
                            "document image export needs an RGB ICC profile",
                        ));
                    }
                    profile
                }
                None => registry
                    .builtin(Builtin::Srgb)
                    .map_err(|e| encode("icc", e))?,
            };
            if float && doc.state().profile.is_some() {
                // F32 documents are scene-linear in their profile's primaries.
                // Encode into that profile, never apply an sRGB transfer curve
                // then attach an unrelated ICC. LUT profiles cannot safely be
                // linearized; fail rather than guess their scene encoding.
                let linear = registry
                    .linearized_rgb(&icc)
                    .map_err(|e| encode("icc", e))?
                    .ok_or_else(|| {
                        crate::unsupported("float export needs a matrix RGB ICC profile")
                    })?;
                let transform = color_mgmt::Transform::new(
                    &linear,
                    &icc,
                    color_mgmt::TransformOptions::default(),
                )
                .map_err(|e| encode("icc", e))?;
                for p in rgba.as_chunks_mut::<4>().0 {
                    let rgb = transform.apply([p[0], p[1], p[2]].map(|v| v.clamp(0.0, 1.0)));
                    p[..3].copy_from_slice(&rgb);
                }
                float = false;
            }
            write_image(&path, e, &rgba, float, encoding, icc.icc_bytes())
        }
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> EngineResult<()> {
    let tmp = path.with_extension("tmp-export");
    std::fs::write(&tmp, bytes).map_err(|e| EngineError::io_at(&tmp, &e))?;
    std::fs::rename(&tmp, path).map_err(|e| EngineError::io_at(path, &e))
}

/// Linear to sRGB-encoded (scene-referred f32 documents only).
pub(crate) fn encode_srgb(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

fn resized(e: Extent, rgba: Vec<f32>, resize: Option<&Resize>) -> EngineResult<(Extent, Vec<f32>)> {
    let Some(resize) = resize else {
        return Ok((e, rgba));
    };
    let (w, h) = (f64::from(e.width), f64::from(e.height));
    let scale = match resize {
        Resize::LongEdge { pixels } => f64::from(*pixels) / w.max(h),
        Resize::Within { width, height } => (f64::from(*width) / w).min(f64::from(*height) / h),
        Resize::Megapixels { megapixels } => (f64::from(*megapixels) * 1e6 / (w * h)).sqrt(),
    };
    if !(scale.is_finite() && scale > 0.0) {
        return Err(EngineError::invalid("resize", "must be positive"));
    }
    if scale >= 1.0 {
        return Ok((e, rgba));
    }
    let (nw, nh) = (
        ((w * scale).round() as u32).max(1),
        ((h * scale).round() as u32).max(1),
    );
    // Resample premultiplied so transparent pixels do not bleed colour.
    let pre: Vec<f32> = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| [p[0] * p[3], p[1] * p[3], p[2] * p[3], p[3]])
        .collect();
    let buf = image::ImageBuffer::<image::Rgba<f32>, _>::from_raw(e.width, e.height, pre)
        .ok_or_else(|| EngineError::internal("composite size"))?;
    let out = image::imageops::resize(&buf, nw, nh, image::imageops::FilterType::Lanczos3);
    let px = out
        .into_raw()
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| {
            let a = p[3].clamp(0.0, 1.0);
            if a <= 1e-7 {
                [0.0; 4]
            } else {
                [p[0] / a, p[1] / a, p[2] / a, a]
            }
        })
        .collect();
    Ok((Extent::new(nw, nh), px))
}

fn write_image(
    path: &Path,
    e: Extent,
    rgba: &[f32],
    float: bool,
    encoding: &ExportFormat,
    icc: &[u8],
) -> EngineResult<()> {
    let enc = |v: f32| {
        if float {
            encode_srgb(v)
        } else {
            v.clamp(0.0, 1.0)
        }
    };
    let q8 = |v: f32| (enc(v) * 255.0).round() as u8;
    let q16 = |v: f32| (enc(v) * 65535.0).round() as u16;
    let alpha8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let alpha16 = |v: f32| (v.clamp(0.0, 1.0) * 65535.0).round() as u16;
    let mut bytes = std::io::Cursor::new(Vec::new());
    match encoding {
        ExportFormat::Jpeg { quality } => {
            if !(1..=100).contains(quality) {
                return Err(EngineError::invalid("quality", "must be within 1..=100"));
            }
            // Flatten onto white, as Photoshop does for formats without alpha.
            let rgb: Vec<u8> = rgba
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| {
                    let a = p[3].clamp(0.0, 1.0);
                    [0, 1, 2].map(|c| {
                        let v = enc(p[c]) * a + (1.0 - a);
                        (v.clamp(0.0, 1.0) * 255.0).round() as u8
                    })
                })
                .collect();
            let mut encoder =
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, *quality);
            encoder
                .set_icc_profile(icc.to_vec())
                .map_err(|e| encode("jpeg", e))?;
            encoder
                .encode(&rgb, e.width, e.height, image::ExtendedColorType::Rgb8)
                .map_err(|e| encode("jpeg", e))?;
        }
        ExportFormat::Png { bit_depth: 8 } => {
            let px: Vec<u8> = rgba
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| [q8(p[0]), q8(p[1]), q8(p[2]), alpha8(p[3])])
                .collect();
            let mut encoder = image::codecs::png::PngEncoder::new(&mut bytes);
            encoder
                .set_icc_profile(icc.to_vec())
                .map_err(|e| encode("png", e))?;
            encoder
                .write_image(&px, e.width, e.height, image::ExtendedColorType::Rgba8)
                .map_err(|e| encode("png", e))?;
        }
        ExportFormat::Png { bit_depth: 16 } => {
            let px: Vec<u16> = rgba
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| [q16(p[0]), q16(p[1]), q16(p[2]), alpha16(p[3])])
                .collect();
            // ImageEncoder accepts native-endian u16 samples, unlike the PNG wire format.
            let px: Vec<u8> = px.into_iter().flat_map(u16::to_ne_bytes).collect();
            let mut encoder = image::codecs::png::PngEncoder::new(&mut bytes);
            encoder
                .set_icc_profile(icc.to_vec())
                .map_err(|e| encode("png", e))?;
            encoder
                .write_image(&px, e.width, e.height, image::ExtendedColorType::Rgba16)
                .map_err(|e| encode("png", e))?;
        }
        ExportFormat::Tiff {
            bit_depth: bits @ (8 | 16),
        } => {
            use tiff::encoder::{TiffEncoder, colortype};
            let mut encoder = TiffEncoder::new(&mut bytes).map_err(|e| encode("tiff", e))?;
            if *bits == 16 {
                let px: Vec<u16> = rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .flat_map(|p| [q16(p[0]), q16(p[1]), q16(p[2]), alpha16(p[3])])
                    .collect();
                let mut image = encoder
                    .new_image::<colortype::RGBA16>(e.width, e.height)
                    .map_err(|e| encode("tiff", e))?;
                image
                    .encoder()
                    .write_tag(tiff::tags::Tag::Unknown(34675), icc)
                    .map_err(|e| encode("tiff", e))?;
                image.write_data(&px).map_err(|e| encode("tiff", e))?;
            } else {
                let px: Vec<u8> = rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .flat_map(|p| [q8(p[0]), q8(p[1]), q8(p[2]), alpha8(p[3])])
                    .collect();
                let mut image = encoder
                    .new_image::<colortype::RGBA8>(e.width, e.height)
                    .map_err(|e| encode("tiff", e))?;
                image
                    .encoder()
                    .write_tag(tiff::tags::Tag::Unknown(34675), icc)
                    .map_err(|e| encode("tiff", e))?;
                image.write_data(&px).map_err(|e| encode("tiff", e))?;
            }
        }
        _ => {
            return Err(crate::unsupported(
                "document image export supports JPEG, PNG 8/16-bit and TIFF 8/16-bit",
            ));
        }
    }
    write_atomic(path, &bytes.into_inner())
}
