//! Opening (native, PSD/PSB, flat images, library images), saving, flat
//! export, and the raster-producing edits (merge down, flatten, selections).

use super::{Opened, find, render::composite_raster, tile_from_f32};
use crate::{Engine, Result, catalog, failure, parse_id};
use compositor::{
    BlendMode, ColorProfile, DocOp, DocState, Document, Knockout, Layer, LayerId, LayerKind,
    Raster, Rect, document::selection,
};
use engine_api::tile::{Extent, TileCoord};
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

/// Flat export container.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ExportFormat {
    Png,
    Jpeg,
    Tiff,
}

/// Colour space of a flat export.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ExportColor {
    /// The document's own profile (no conversion).
    Document,
    Srgb,
    DisplayP3,
    AdobeRgb,
    ProPhoto,
    Rec2020,
}

// ─────────────────────────────── profiles ───────────────────────────────

fn builtin(b: color_mgmt::Builtin) -> Result<Vec<u8>> {
    Ok(color_mgmt::Registry::new()
        .builtin(b)
        .map_err(failure)?
        .icc_bytes()
        .to_vec())
}

/// A named or file profile (`None` → sRGB).
pub(crate) fn profile(name: Option<&str>) -> Result<Option<ColorProfile>> {
    use color_mgmt::Builtin::*;
    let name = name
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .unwrap_or("sRGB");
    let key: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    let known = match key.as_str() {
        "srgb" | "srgbiec6196621" => Some((Srgb, "sRGB IEC61966-2.1")),
        "displayp3" | "p3" => Some((DisplayP3, "Display P3")),
        "adobergb" | "adobergb1998" => Some((AdobeRgb, "Adobe RGB (1998)")),
        "prophoto" | "prophotorgb" => Some((ProPhoto, "ProPhoto RGB")),
        "rec2020" | "rec2020rgb" | "bt2020" => Some((Rec2020, "Rec. 2020")),
        _ => None,
    };
    if let Some((b, label)) = known {
        return Ok(Some(ColorProfile::from_icc(label, builtin(b)?)));
    }
    let path = Path::new(name);
    let bytes = std::fs::read(path).map_err(|e| failure(format!("profile {name}: {e}")))?;
    color_mgmt::Registry::new()
        .load_bytes(&bytes)
        .map_err(|e| failure(format!("profile {name}: {e}")))?;
    let label = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| name.to_owned());
    Ok(Some(ColorProfile::from_icc(label, bytes)))
}

// ─────────────────────────────── opening ───────────────────────────────

fn ext(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

fn file_title(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Untitled".into())
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Layer".into())
}

pub(crate) fn open_path(path: &Path) -> Result<Opened> {
    let (doc, saved_path) = match ext(path).as_str() {
        "tessera-doc" => (compositor::format::load(path)?, true),
        "psd" | "psb" => {
            let bytes = std::fs::read(path)?;
            let psd = ::psd::PsdDocument::read(&bytes)
                .map_err(|e| failure(format!("{}: {e}", path.display())))?;
            (Document::from_psd(psd)?, true)
        }
        "jpg" | "jpeg" | "png" | "tif" | "tiff" => (flat_image(path)?, false),
        other => return Err(failure(format!("cannot open .{other} files as documents"))),
    };
    Ok(Opened {
        doc,
        title: file_title(path),
        path: saved_path.then(|| path.to_path_buf()),
        source_image_id: None,
        origin: "Open",
        unsaved: false,
    })
}

/// A decoded flat image: extent, straight RGBA f32, depth, embedded ICC.
type Decoded = (Extent, Vec<f32>, compositor::Depth, Option<Vec<u8>>);

/// Straight RGBA f32, depth and optional ICC of a flat image file.
fn decode_flat(path: &Path) -> Result<Decoded> {
    if matches!(ext(path).as_str(), "tif" | "tiff") {
        return decode_tiff(path);
    }
    use image::ImageDecoder;
    let mut decoder = image::ImageReader::open(path)?
        .with_guessed_format()?
        .into_decoder()
        .map_err(failure)?;
    let icc = decoder.icc_profile().ok().flatten();
    let orientation = decoder.orientation().ok();
    let deep = decoder.color_type().bytes_per_pixel() / decoder.color_type().channel_count() > 1;
    let mut img = image::DynamicImage::from_decoder(decoder).map_err(failure)?;
    if let Some(o) = orientation {
        img.apply_orientation(o);
    }
    let rgba = img.to_rgba32f();
    let e = Extent::new(rgba.width(), rgba.height());
    let depth = if deep {
        compositor::Depth::U16
    } else {
        compositor::Depth::U8
    };
    Ok((e, rgba.into_raw(), depth, icc))
}

fn decode_tiff(path: &Path) -> Result<Decoded> {
    use tiff::{ColorType, decoder::DecodingResult, tags::Tag};
    let bad = |e: tiff::TiffError| failure(format!("{}: {e}", path.display()));
    let mut d = tiff::decoder::Decoder::new(std::io::BufReader::new(std::fs::File::open(path)?))
        .map_err(bad)?;
    let (w, h) = d.dimensions().map_err(bad)?;
    let channels = match d.colortype().map_err(bad)? {
        ColorType::Gray(_) => 1,
        ColorType::GrayA(_) => 2,
        ColorType::RGB(_) => 3,
        ColorType::RGBA(_) => 4,
        other => return Err(failure(format!("unsupported TIFF colour type {other:?}"))),
    };
    let icc = d.get_tag_u8_vec(Tag::Unknown(34675)).ok();
    let (samples, depth): (Vec<f32>, _) = match d.read_image().map_err(bad)? {
        DecodingResult::U8(v) => (
            v.iter().map(|&x| f32::from(x) / 255.0).collect(),
            compositor::Depth::U8,
        ),
        DecodingResult::U16(v) => (
            v.iter().map(|&x| f32::from(x) / 65535.0).collect(),
            compositor::Depth::U16,
        ),
        DecodingResult::F32(v) => (v, compositor::Depth::F32),
        _ => return Err(failure("unsupported TIFF sample format")),
    };
    let n = w as usize * h as usize;
    if samples.len() < n * channels {
        return Err(failure("truncated TIFF"));
    }
    let mut rgba = vec![0.0f32; n * 4];
    for (i, p) in rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let s = &samples[i * channels..(i + 1) * channels];
        *p = match channels {
            1 => [s[0], s[0], s[0], 1.0],
            2 => [s[0], s[0], s[0], s[1]],
            3 => [s[0], s[1], s[2], 1.0],
            _ => [s[0], s[1], s[2], s[3]],
        };
    }
    Ok((Extent::new(w, h), rgba, depth, icc))
}

fn flat_image(path: &Path) -> Result<Document> {
    let (extent, rgba, depth, icc) = decode_flat(path)?;
    let profile = match icc {
        Some(bytes) if color_mgmt::Registry::new().load_bytes(&bytes).is_ok() => {
            Some(ColorProfile::from_icc(icc_description(&bytes), bytes))
        }
        _ => profile(None)?,
    };
    Ok(Document::new(single_layer_state(
        extent,
        depth,
        profile,
        &stem(path),
        &rgba,
    )?))
}

/// The profile description tag (`desc`), or "Embedded Profile".
fn icc_description(bytes: &[u8]) -> String {
    lcms2::Profile::new_icc(bytes)
        .ok()
        .and_then(|p| p.info(lcms2::InfoType::Description, lcms2::Locale::none()))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Embedded Profile".into())
}

fn single_layer_state(
    extent: Extent,
    depth: compositor::Depth,
    profile: Option<ColorProfile>,
    name: &str,
    rgba: &[f32],
) -> Result<DocState> {
    let raster = super::raster_from_rgba(extent, depth, rgba, true)?;
    let mut state = DocState::new(extent, depth);
    state.profile = profile;
    let mut layer = Layer::new(name, LayerKind::Pixel(raster));
    layer.id = LayerId(1);
    state.next_id = 2;
    state.root.push(Arc::new(layer));
    Ok(state)
}

/// A library image rendered through the export path into one pixel layer.
pub(crate) fn open_image(engine: &Arc<Engine>, image_id: &str, developed: bool) -> Result<Opened> {
    let id = parse_id(image_id)?;
    let (path, orientation) = {
        let c = engine.lock()?;
        let path = Engine::path(&c, image_id)?;
        let orientation: String = c.reader.query_row(
            "SELECT COALESCE((SELECT value FROM metadata WHERE image_id=? AND key='orientation'),'1')",
            [image_id],
            |r| r.get(0),
        )?;
        (PathBuf::from(path), orientation.parse::<u16>().unwrap_or(1))
    };
    let recipe = if developed {
        let _c = engine.lock()?;
        catalog::document(&path, id)?.recipe
    } else {
        engine_api::recipe::Recipe::new(id)
    };
    let source = crate::export::Source::open(&path, orientation)?;
    let name = stem(&path);
    let image = export::ExportImage {
        source: source.render_source(),
        name: &name,
        sequence: 1,
        date: "",
        metadata: None,
    };
    let mut segmenter = if export::needs_segmenter(&recipe) {
        Some(
            export::mask_ai::load_segmenter(engine.support_dir()?)
                .map_err(|e| failure(format!("AI masks: {e}")))?,
        )
    } else {
        None
    };
    let rgb = export::render_pixels(
        &image,
        &recipe,
        &export::RenderRequest {
            color_space: export::ColorSpace::Srgb,
            resize: export::Resize::None,
            sharpen_for: export::SharpenFor::None,
            scale: 1,
        },
        &engine_api::jobs::CancellationToken::new(),
        match segmenter.as_mut() {
            Some(s) => Some(s.as_mut()),
            None => None,
        },
    )?;
    let extent = Extent::new(rgb.width(), rgb.height());
    let mut rgba = Vec::with_capacity(extent.area() as usize * 4);
    for p in rgb.as_raw().as_chunks::<3>().0 {
        rgba.extend_from_slice(&[p[0], p[1], p[2], 1.0]);
    }
    let profile = Some(ColorProfile::from_icc(
        "sRGB IEC61966-2.1",
        export::color_space_icc(export::ColorSpace::Srgb)?,
    ));
    let state = single_layer_state(extent, compositor::Depth::U16, profile, &name, &rgba)?;
    Ok(Opened {
        doc: Document::new(state),
        title: name,
        path: None,
        source_image_id: Some(image_id.to_owned()),
        origin: if developed {
            "Open Developed Image"
        } else {
            "Open Image"
        },
        unsaved: true,
    })
}

// ─────────────────────────────── saving ───────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SaveKind {
    Native,
    Psd,
    Psb,
}

pub(crate) fn save_kind(path: &Path) -> Result<SaveKind> {
    match ext(path).as_str() {
        "tessera-doc" => Ok(SaveKind::Native),
        "psd" => Ok(SaveKind::Psd),
        "psb" => Ok(SaveKind::Psb),
        other => Err(failure(format!(
            "documents are saved as .tessera-doc, .psd or .psb, not .{other}"
        ))),
    }
}

/// Writes `bytes` next to `path` and renames it into place.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(bytes)?;
    tmp.as_file().sync_all()?;
    tmp.persist(path).map_err(|e| failure(e.error))?;
    Ok(())
}

pub(crate) fn save(doc: &Document, path: &Path) -> Result<()> {
    match save_kind(path)? {
        SaveKind::Native => Ok(compositor::format::save(doc, path)?),
        kind => {
            let mut psd = compositor::psd::to_psd(doc)?;
            if kind == SaveKind::Psb {
                psd.version = ::psd::Version::Psb;
            } else if psd.width > 30_000 || psd.height > 30_000 {
                return Err(failure("PSD is limited to 30000 pixels: save as .psb"));
            } else {
                psd.version = ::psd::Version::Psd;
            }
            let bytes = psd.write().map_err(failure)?;
            write_atomic(path, &bytes)
        }
    }
}

// ─────────────────────────────── export ───────────────────────────────

pub(crate) fn export_flat(
    doc: &Document,
    path: &Path,
    format: ExportFormat,
    quality: u8,
    color: ExportColor,
) -> Result<()> {
    if format == ExportFormat::Jpeg && !(1..=100).contains(&quality) {
        return Err(failure("JPEG quality must be 1–100"));
    }
    let state = doc.state();
    let (e, mut rgba) = compositor::Compositor::new(256 << 20).render_level_rgba(doc, 0)?;
    // Colour: document profile (untagged = sRGB) → target.
    let source = match &state.profile {
        Some(ColorProfile { icc: Some(b), .. }) => b.as_ref().clone(),
        _ => builtin(color_mgmt::Builtin::Srgb)?,
    };
    let target = match color {
        ExportColor::Document => source.clone(),
        ExportColor::Srgb => builtin(color_mgmt::Builtin::Srgb)?,
        ExportColor::DisplayP3 => builtin(color_mgmt::Builtin::DisplayP3)?,
        ExportColor::AdobeRgb => builtin(color_mgmt::Builtin::AdobeRgb)?,
        ExportColor::ProPhoto => builtin(color_mgmt::Builtin::ProPhoto)?,
        ExportColor::Rec2020 => builtin(color_mgmt::Builtin::Rec2020)?,
    };
    if source != target {
        convert(&source, &target, &mut rgba)?;
    }
    let (w, h) = (e.width, e.height);
    let wide = state.depth != compositor::Depth::U8;
    let q8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    let q16 = |v: f32| (v.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16;
    let mut out: Vec<u8> = Vec::new();
    let bad = |e: &dyn std::fmt::Display| failure(format!("export: {e}"));
    match format {
        ExportFormat::Jpeg => {
            let (w16, h16) = (
                u16::try_from(w).map_err(|_| failure("JPEG is limited to 65535 pixels"))?,
                u16::try_from(h).map_err(|_| failure("JPEG is limited to 65535 pixels"))?,
            );
            let rgb: Vec<u8> = rgba
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| {
                    let a = p[3].clamp(0.0, 1.0);
                    [0, 1, 2].map(|c| q8(p[c] * a + (1.0 - a)))
                })
                .collect();
            let mut enc = jpeg_encoder::Encoder::new(&mut out, quality);
            enc.add_icc_profile(&target).map_err(|e| bad(&e))?;
            enc.encode(&rgb, w16, h16, jpeg_encoder::ColorType::Rgb)
                .map_err(|e| bad(&e))?;
        }
        ExportFormat::Png => {
            let mut info = png::Info::with_size(w, h);
            info.color_type = png::ColorType::Rgba;
            info.bit_depth = if wide {
                png::BitDepth::Sixteen
            } else {
                png::BitDepth::Eight
            };
            info.icc_profile = Some(target.clone().into());
            let enc = png::Encoder::with_info(&mut out, info).map_err(|e| bad(&e))?;
            let mut writer = enc.write_header().map_err(|e| bad(&e))?;
            let data: Vec<u8> = if wide {
                rgba.iter().flat_map(|v| q16(*v).to_be_bytes()).collect()
            } else {
                rgba.iter().map(|v| q8(*v)).collect()
            };
            writer.write_image_data(&data).map_err(|e| bad(&e))?;
            writer.finish().map_err(|e| bad(&e))?;
        }
        ExportFormat::Tiff => {
            use tiff::{encoder::colortype, tags::Tag};
            let mut cursor = std::io::Cursor::new(&mut out);
            let mut enc = tiff::encoder::TiffEncoder::new(&mut cursor).map_err(|e| bad(&e))?;
            macro_rules! write_tiff {
                ($color:ty, $data:expr) => {{
                    let mut image = enc.new_image::<$color>(w, h).map_err(|e| bad(&e))?;
                    image
                        .encoder()
                        .write_tag(Tag::Unknown(34675), target.as_slice())
                        .map_err(|e| bad(&e))?;
                    image.write_data($data).map_err(|e| bad(&e))?;
                }};
            }
            if wide {
                let data: Vec<u16> = rgba.iter().map(|v| q16(*v)).collect();
                write_tiff!(colortype::RGBA16, &data);
            } else {
                let data: Vec<u8> = rgba.iter().map(|v| q8(*v)).collect();
                write_tiff!(colortype::RGBA8, &data);
            }
        }
    }
    write_atomic(path, &out)
}

/// Converts straight RGB (alpha untouched) between two ICC profiles.
fn convert(source: &[u8], target: &[u8], rgba: &mut [f32]) -> Result<()> {
    let src = lcms2::Profile::new_icc(source).map_err(failure)?;
    let dst = lcms2::Profile::new_icc(target).map_err(failure)?;
    let t: lcms2::Transform<[f32; 3], [f32; 3]> = lcms2::Transform::new_flags(
        &src,
        lcms2::PixelFormat::RGB_FLT,
        &dst,
        lcms2::PixelFormat::RGB_FLT,
        lcms2::Intent::RelativeColorimetric,
        lcms2::Flags::BLACKPOINT_COMPENSATION,
    )
    .map_err(failure)?;
    let mut rgb: Vec<[f32; 3]> = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| [p[0], p[1], p[2]])
        .collect();
    for chunk in rgb.chunks_mut(1 << 16) {
        t.transform_in_place(chunk);
    }
    for (p, c) in rgba.as_chunks_mut::<4>().0.iter_mut().zip(rgb) {
        p[..3].copy_from_slice(&c);
    }
    Ok(())
}

// ─────────────────────────────── edits ───────────────────────────────

/// Pixel-exact bounds of a selection raster: the pixels with any
/// selection (`None`: nothing selected). The marching ants and the status
/// bar show them, so stored-tile granularity is not enough (WP M5-10b).
/// Scans the stored tiles; `info` caches the result per selection.
pub(crate) fn selection_bounds(r: &Raster) -> Option<Rect> {
    if r.default_value() > 0.0 {
        return Some(Rect::of_extent(r.extent()));
    }
    let mut out = Rect::default();
    let mut buf = Vec::new();
    for ((tx, ty), slot) in r.slots() {
        if slot.tile.is_none() {
            continue;
        }
        let l = r.layout(tx, ty);
        let tile = engine_api::tile::TILE_SIZE;
        let (ox, oy) = (i64::from(tx * tile), i64::from(ty * tile));
        if r.read_tile(tx, ty, &mut buf).is_err() {
            // Unreadable tile: fall back to its whole extent.
            out = out.union(&Rect::new(
                ox,
                oy,
                ox + i64::from(l.extent.width),
                oy + i64::from(l.extent.height),
            ));
            continue;
        }
        let (w, h, stride) = (
            l.extent.width as usize,
            l.extent.height as usize,
            l.stride(),
        );
        let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
        for y in 0..h {
            let row = &buf[y * stride..y * stride + w];
            if let Some(first) = row.iter().position(|v| *v > 0.0) {
                let last = row.iter().rposition(|v| *v > 0.0).unwrap_or(first);
                x0 = x0.min(first);
                x1 = x1.max(last + 1);
                y0 = y0.min(y);
                y1 = y + 1;
            }
        }
        if x1 > x0 && y1 > y0 {
            out = out.union(&Rect::new(
                ox + x0 as i64,
                oy + y0 as i64,
                ox + x1 as i64,
                oy + y1 as i64,
            ));
        }
    }
    (!out.is_empty()).then_some(out)
}

/// A one-channel raster in `depth` from a (float) selection raster.
pub(crate) fn convert_raster(sel: &Raster, depth: compositor::Depth) -> Result<Raster> {
    let mut out = Raster::new(sel.extent(), 1, depth, sel.default_value());
    let mut buf = Vec::new();
    for ((tx, ty), slot) in sel.slots() {
        if slot.tile.is_none() {
            continue;
        }
        sel.read_tile(tx, ty, &mut buf)?;
        let tile = tile_from_f32(
            TileCoord::new(0, tx, ty),
            out.layout(tx, ty),
            depth,
            buf.clone(),
        )?;
        out.set_slot(tx, ty, Some(tile), 0)?;
    }
    Ok(out)
}

/// A rectangular selection with edges ramped over `feather` pixels.
pub(crate) fn rect_selection(canvas: Extent, rect: Rect, feather: f32) -> Result<Raster> {
    let full = Rect::of_extent(canvas);
    if rect.intersect(&full).is_empty() {
        return Err(failure("selection rectangle is outside the canvas"));
    }
    if !feather.is_finite() || feather < 0.0 {
        return Err(failure("feather must be ≥ 0"));
    }
    if feather < 0.5 {
        return Ok(selection::rect(canvas, rect.intersect(&full))?);
    }
    let f = feather;
    let reach = f.ceil() as i64;
    let ramp = |c: f32, lo: f32, hi: f32| {
        ((c - lo) / f + 0.5).clamp(0.0, 1.0) * ((hi - c) / f + 0.5).clamp(0.0, 1.0)
    };
    let (x0, y0, x1, y1) = (
        rect.x0 as f32,
        rect.y0 as f32,
        rect.x1 as f32,
        rect.y1 as f32,
    );
    let mut s = Raster::new(canvas, 1, compositor::Depth::F32, 0.0);
    s.edit_region(rect.inflate(reach).intersect(&full), 0, |x, y, p| {
        p[0] = ramp(x as f32 + 0.5, x0, x1) * ramp(y as f32 + 0.5, y0, y1);
    })?;
    Ok(s)
}

/// The op merging `id` into the layer below it (see `merge_down`).
pub(crate) fn merge_down_op(s: &DocState, id: LayerId) -> Result<DocOp> {
    let (parent, i) = s
        .locate(id)
        .ok_or_else(|| failure(format!("layer {} not found", id.0)))?;
    if i == 0 {
        return Err(failure("there is no layer below to merge into"));
    }
    let siblings = match parent {
        None => &s.root[..],
        Some(p) => find(s, p.0)?.children().expect("parent is a group"),
    };
    let upper = siblings[i].clone();
    let below = siblings[i - 1].clone();
    if matches!(
        below.kind,
        LayerKind::Adjustment(_) | LayerKind::Group { .. }
    ) {
        return Err(failure(
            "merge down needs a pixel, fill, text or smart object layer below",
        ));
    }
    let mut base = (*below).clone();
    base.props.visible = true;
    base.props.opacity = 1.0;
    base.props.fill_opacity = 1.0;
    base.props.blend_mode = BlendMode::Normal;
    base.props.clipped = false;
    base.props.knockout = Knockout::None;
    base.props.background = false;
    base.props.blend_if = Default::default();
    let mut mini = DocState::new(s.canvas, s.depth);
    mini.next_id = s.next_id;
    mini.root = vec![Arc::new(base), upper.clone()];
    let raster = composite_raster(&Document::new(mini), None, true)?;
    let mut merged = Layer::new(below.props.name.clone(), LayerKind::Pixel(raster));
    merged.props = below.props.clone();
    merged.id = below.id;
    Ok(DocOp::Batch(vec![
        DocOp::RemoveLayer { id: upper.id },
        DocOp::RemoveLayer { id: below.id },
        DocOp::AddLayer {
            parent,
            index: i - 1,
            layer: merged,
        },
    ]))
}

/// The op replacing every layer by one opaque Background layer.
pub(crate) fn flatten_op(s: &DocState) -> Result<DocOp> {
    if s.root.is_empty() {
        return Err(failure("the document has no layers"));
    }
    let raster = composite_raster(&Document::new(s.clone()), Some([1.0; 3]), false)?;
    let mut bg = Layer::new("Background", LayerKind::Pixel(raster));
    bg.props.background = true;
    let mut ops: Vec<DocOp> = s
        .root
        .iter()
        .map(|l| DocOp::RemoveLayer { id: l.id })
        .collect();
    ops.push(DocOp::AddLayer {
        parent: None,
        index: 0,
        layer: bg,
    });
    Ok(DocOp::Batch(ops))
}
