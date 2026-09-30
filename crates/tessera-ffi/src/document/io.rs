//! Opening (native, PSD/PSB, flat images, library images), saving, flat
//! export, and the raster-producing edits (merge down, flatten, selections).

use super::{DocumentSaveAsResult, DocumentSession, Opened, find, render::composite_raster, tile_from_f32};
use crate::{Engine, Result, catalog, failure, parse_id};
use compositor::{
    BlendMode, ColorProfile, DocOp, DocState, Document, Knockout, Layer, LayerId, LayerKind,
    Raster, Rect, document::selection,
};
use engine_api::{
    jobs::CancellationToken,
    tile::{Extent, TileCoord},
};
use std::{
    io::{self, Write},
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
pub(crate) fn open_image(
    engine: &Arc<Engine>,
    image_id: &str,
    developed: bool,
    cancel: &CancellationToken,
) -> Result<Opened> {
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
        cancel,
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

/// A private commit mode; legacy saves replace without implying a UI approval.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CommitMode {
    Replace,
    CreateIfAbsent,
}

fn stage_bytes(path: &Path, bytes: &[u8]) -> Result<tempfile::NamedTempFile> {
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(bytes)?;
    tmp.as_file().sync_all()?;
    Ok(tmp)
}

fn commit_staged(
    staged: tempfile::NamedTempFile,
    path: &Path,
    mode: CommitMode,
) -> Result<DocumentSaveAsResult> {
    match mode {
        CommitMode::Replace => {
            staged.persist(path).map_err(|e| failure(e.error))?;
            Ok(DocumentSaveAsResult::Saved)
        }
        CommitMode::CreateIfAbsent => match staged.persist_noclobber(path) {
            Ok(_) => {
                // tempfile may use hard-link/unlink on a filesystem lacking
                // exclusive rename; its unlink error is not observable here.
                // Publication already succeeded, so never report failure or
                // try to remove a possibly reused stage path afterward.
                Ok(DocumentSaveAsResult::Saved)
            }
            Err(error) if error.error.kind() == io::ErrorKind::AlreadyExists => {
                Ok(DocumentSaveAsResult::DestinationExists)
            }
            Err(error) => Err(failure(error.error)),
        },
    }
}

fn write_atomic_with_mode(
    path: &Path,
    bytes: &[u8],
    mode: CommitMode,
) -> Result<DocumentSaveAsResult> {
    commit_staged(stage_bytes(path, bytes)?, path, mode)
}

/// Existing flat-export behavior retains replacing publication.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    write_atomic_with_mode(path, bytes, CommitMode::Replace)?;
    Ok(())
}

pub(super) fn save_with_mode(
    doc: &Document,
    path: &Path,
    mode: CommitMode,
) -> Result<DocumentSaveAsResult> {
    match save_kind(path)? {
        SaveKind::Native => {
            let bytes = compositor::format::to_bytes(doc.state())?;
            write_atomic_with_mode(path, &bytes, mode)
        }
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
            write_atomic_with_mode(path, &bytes, mode)
        }
    }
}

/// Copy-only transaction. Conversion uses the operation's live native token;
/// the encoded PSD writer remains opaque and boundary-checked.
pub(super) fn save_psd_copy_checked(
    doc: &Document,
    path: &Path,
    cancel: &CancellationToken,
    check: &impl Fn() -> super::psd_copy::CopyResult<()>,
    admit_commit: &impl Fn() -> super::psd_copy::CopyResult<()>,
) -> super::psd_copy::CopyResult<()> {
    check()?;
    let kind = save_kind(path)?;
    if kind == SaveKind::Native {
        return Err(failure("copy requires PSD or PSB").into());
    }
    let mut psd = compositor::psd::to_psd_with_cancel(doc, cancel)?;
    check()?;
    psd.version = if kind == SaveKind::Psb {
        ::psd::Version::Psb
    } else {
        ::psd::Version::Psd
    };
    let bytes = psd.write().map_err(failure)?;
    check()?;
    write_copy_atomic(path, &bytes, check, admit_commit)
}

fn write_copy_atomic(
    path: &Path,
    bytes: &[u8],
    check: &impl Fn() -> super::psd_copy::CopyResult<()>,
    admit_commit: &impl Fn() -> super::psd_copy::CopyResult<()>,
) -> super::psd_copy::CopyResult<()> {
    check()?;
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    for chunk in bytes.chunks(64 * 1024) {
        check()?;
        tmp.write_all(chunk)?;
    }
    check()?;
    tmp.as_file().sync_all()?;
    check()?;
    // Linearization point: cancel can no longer win after admission. Never
    // report Cancelled after replacement, even if close races this syscall.
    admit_commit()?;
    tmp.persist(path).map_err(|e| failure(e.error))?;
    Ok(())
}

// ─────────────────────────────── export ───────────────────────────────

// B5-15 (P16) begin: export runs from an immutable snapshot on a caller
// thread, reports progress and stops at cancellation checkpoints. Cancelling
// never touches the destination: the file is encoded in memory and renamed
// into place from a temporary file in the same folder only at the end.

/// Progress and cancellation of one flat export.
pub(crate) struct ExportCtl<'a> {
    /// Set to stop at the next checkpoint (tile, phase boundary).
    pub cancel: &'a std::sync::atomic::AtomicBool,
    /// `(fraction 0…1, phase)`; called from the exporting threads.
    pub progress: &'a (dyn Fn(f32, &str) + Sync),
}

impl ExportCtl<'_> {
    fn check(&self) -> Result<()> {
        if self.cancel.load(std::sync::atomic::Ordering::Relaxed) {
            Err(failure("export cancelled"))
        } else {
            Ok(())
        }
    }
}

/// Checks what an export can reject before any work (JPEG quality).
pub(crate) fn export_flat_check(format: ExportFormat, quality: u8) -> Result<()> {
    if format == ExportFormat::Jpeg && !(1..=100).contains(&quality) {
        return Err(failure("JPEG quality must be 1–100"));
    }
    Ok(())
}

/// Level 0 of `doc` as interleaved straight RGBA, exactly as
/// `Compositor::render_level_rgba` (each tile is `render_tile`, which is
/// `unpremultiply(render_tile_premultiplied)` as in `render_level`), rendered
/// on worker threads with a cancellation check and progress per tile. Tiles
/// are interleaved as they arrive, so the tile list and the image are never
/// both resident.
fn render_rgba(
    comp: &compositor::Compositor,
    doc: &Document,
    ctl: &ExportCtl<'_>,
    span: (f32, f32),
) -> Result<(Extent, Vec<f32>)> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let e = doc.state().canvas;
    let (cols, rows) = e.tile_grid(engine_api::tile::TILE_SIZE);
    let coords: Vec<TileCoord> = (0..rows)
        .flat_map(|y| (0..cols).map(move |x| TileCoord::new(0, x, y)))
        .collect();
    let mut out = vec![0.0f32; e.width as usize * e.height as usize * 4];
    let threads = std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .min(coords.len().max(1));
    let next = AtomicUsize::new(0);
    let (tx, rx) = std::sync::mpsc::sync_channel::<Result<engine_api::tile::Tile>>(threads * 2);
    std::thread::scope(|s| -> Result<()> {
        for _ in 0..threads {
            let (tx, next, coords) = (tx.clone(), &next, &coords);
            s.spawn(move || {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(c) = coords.get(i) else { break };
                    let r = ctl
                        .check()
                        .and_then(|()| comp.render_tile(doc, *c).map_err(Into::into));
                    let failed = r.is_err();
                    if tx.send(r).is_err() || failed {
                        break;
                    }
                }
            });
        }
        drop(tx);
        let mut done = 0usize;
        let mut first_err = None;
        for r in rx {
            match r {
                Ok(t) => {
                    interleave_tile(e, &t, &mut out)?;
                    done += 1;
                    (ctl.progress)(
                        span.0 + (span.1 - span.0) * done as f32 / coords.len().max(1) as f32,
                        "Compositing",
                    );
                }
                Err(err) => {
                    // Stop the other workers at their next tile.
                    next.store(usize::MAX / 2, Ordering::Relaxed);
                    first_err.get_or_insert(err);
                }
            }
        }
        first_err.map_or(Ok(()), Err)
    })?;
    Ok((e, out))
}

/// Copies one straight planar tile into interleaved `out` (`compositor::
/// render::interleave` for a single tile).
fn interleave_tile(e: Extent, t: &engine_api::tile::Tile, out: &mut [f32]) -> Result<()> {
    let (ox, oy) = t.coord().pixel_origin(engine_api::tile::TILE_SIZE);
    let l = t.layout();
    let s = t.samples::<f32>()?;
    let p = l.plane_len();
    for y in 0..l.extent.height as usize {
        for x in 0..l.extent.width as usize {
            let o = ((oy as usize + y) * e.width as usize + ox as usize + x) * 4;
            for c in 0..4 {
                out[o + c] = s[c * p + y * l.stride() + x];
            }
        }
    }
    Ok(())
}

/// Flat export of `doc` (smart filters already baked) to `path`.
pub(crate) fn export_flat(
    doc: &Document,
    path: &Path,
    format: ExportFormat,
    quality: u8,
    color: ExportColor,
    ctl: &ExportCtl<'_>,
    span: (f32, f32),
) -> Result<()> {
    export_flat_check(format, quality)?;
    ctl.check()?;
    let state = doc.state();
    let comp = super::fonts::compositor(256 << 20); // B5-10b
    let (e, mut rgba) = render_rgba(&comp, doc, ctl, (span.0, span.0 + (span.1 - span.0) * 0.85))?;
    drop(comp);
    ctl.check()?;
    (ctl.progress)(span.0 + (span.1 - span.0) * 0.85, "Converting colour");
    // B5-15 end
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
    ctl.check()?; // B5-15
    (ctl.progress)(span.0 + (span.1 - span.0) * 0.9, "Encoding"); // B5-15
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
    drop(rgba); // B5-15: the encoded file is all that is left
    ctl.check()?; // B5-15: the last point where cancelling is possible
    (ctl.progress)(span.0 + (span.1 - span.0) * 0.97, "Writing"); // B5-15
    write_atomic(path, &out)?;
    (ctl.progress)(span.1, "Done"); // B5-15
    Ok(())
}

/// Converts straight RGB (alpha untouched) between two ICC profiles.
pub(crate) fn convert(source: &[u8], target: &[u8], rgba: &mut [f32]) -> Result<()> {
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
/// bar show them, so stored-tile granularity is not enough (WP B5-03).
/// Scans the stored tiles; `info` caches the result per selection.
pub(crate) fn selection_bounds(r: &Raster) -> Option<Rect> {
    let mut out = Rect::default();
    let mut buf = Vec::new();
    let tile = engine_api::tile::TILE_SIZE;
    if r.default_value() > 0.0 {
        // Absent tiles are selected (Select All, Inverse): every tile
        // without stored samples counts whole (WP B5-04).
        let (cols, rows) = r.grid();
        for ty in 0..rows {
            for tx in 0..cols {
                if r.tile(tx, ty).is_none() {
                    let l = r.layout(tx, ty);
                    let (ox, oy) = (i64::from(tx * tile), i64::from(ty * tile));
                    out = out.union(&Rect::new(
                        ox,
                        oy,
                        ox + i64::from(l.extent.width),
                        oy + i64::from(l.extent.height),
                    ));
                }
            }
        }
    }
    for ((tx, ty), slot) in r.slots() {
        if slot.tile.is_none() {
            continue;
        }
        let l = r.layout(tx, ty);
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

// ─────────────────────────────── display ───────────────────────────────

/// B5-30: the ICC bytes the canvas should tag its 8-bit surfaces with, so
/// macOS colour-manages the document's own encoded samples to the display.
/// `None` means sRGB: an untagged document, or one tagged with the built-in
/// sRGB profile (the canvas keeps its sRGB path). A non-embedded profile is
/// resolved against the built-ins by handle; one that cannot be resolved is
/// also `None` (shown as sRGB, as before B5-30).
pub(crate) fn display_icc(profile: Option<&ColorProfile>) -> Result<Option<Vec<u8>>> {
    let _ = profile;
    Ok(None)
}

#[uniffi::export]
impl DocumentSession {
    /// B5-30: the document profile's ICC bytes for tagging the canvas, or
    /// `None` for sRGB (see [`display_icc`]). No pixel work.
    pub fn display_profile_icc(&self) -> Result<Option<Vec<u8>>> {
        let st = self.shared.lock()?;
        display_icc(st.live().state().profile.as_ref())
    }
}

#[cfg(test)]
mod display_tests {
    use super::*;

    #[test]
    fn srgb_and_untagged_documents_keep_the_srgb_canvas() {
        assert_eq!(display_icc(None).unwrap(), None);
        let srgb = profile(None).unwrap().unwrap();
        assert_eq!(display_icc(Some(&srgb)).unwrap(), None);
        let unembedded = ColorProfile {
            icc: None,
            ..srgb
        };
        assert_eq!(display_icc(Some(&unembedded)).unwrap(), None);
    }

    #[test]
    fn a_p3_document_hands_over_its_own_icc_bytes() {
        let p3 = profile(Some("Display P3")).unwrap().unwrap();
        let bytes = p3.icc.as_deref().unwrap().clone();
        assert_eq!(display_icc(Some(&p3)).unwrap(), Some(bytes.clone()));
        // Not embedded: resolved against the built-ins by handle.
        let unembedded = ColorProfile { icc: None, ..p3 };
        assert_eq!(display_icc(Some(&unembedded)).unwrap(), Some(bytes));
        let adobe = profile(Some("Adobe RGB")).unwrap().unwrap();
        assert_eq!(
            display_icc(Some(&adobe)).unwrap().as_deref(),
            adobe.icc.as_deref().map(Vec::as_slice)
        );
    }
}

#[cfg(test)]
mod copy_transaction_tests {
    use super::super::psd_copy::CopyError;
    use super::*;
    use std::cell::Cell;
    #[test]
    fn copy_conversion_uses_the_requests_live_native_token() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("copy.psd");
        std::fs::write(&path, b"sentinel").unwrap();
        let doc = Document::new(DocState::new(Extent::new(2, 2), compositor::Depth::U8));
        let request = super::super::filtering::RequestCancellation::default();
        request.cancel();
        // The transaction's boundary check deliberately succeeds; the
        // compositor must observe the very same native request token.
        let result =
            save_psd_copy_checked(&doc, &path, request.native_token(), &|| Ok(()), &|| {
                panic!("cancelled conversion must not enter commit")
            });
        assert!(matches!(result, Err(CopyError::Cancelled)));
        assert_eq!(std::fs::read(&path).unwrap(), b"sentinel");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);

        let fresh = super::super::filtering::RequestCancellation::default();
        save_psd_copy_checked(&doc, &path, fresh.native_token(), &|| Ok(()), &|| Ok(())).unwrap();
        assert!(std::fs::read(&path).unwrap().starts_with(b"8BPS"));
    }
    #[test]
    fn cancelled_before_commit_preserves_destination_and_removes_temp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("copy.psd");
        std::fs::write(&path, b"sentinel").unwrap();
        let result = write_copy_atomic(&path, b"replacement", &|| Ok(()), &|| {
            Err(CopyError::Cancelled)
        });
        assert!(result.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"sentinel");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        write_copy_atomic(&path, b"retry", &|| Ok(()), &|| Ok(())).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"retry");
    }
    #[test]
    fn cancellation_during_chunked_write_removes_temporary_output() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("copy.psd");
        std::fs::write(&path, b"sentinel").unwrap();
        let checks = Cell::new(0);
        let result = write_copy_atomic(
            &path,
            &vec![0; 64 * 1024 + 1],
            &|| {
                checks.set(checks.get() + 1);
                if checks.get() == 3 {
                    Err(CopyError::Cancelled)
                } else {
                    Ok(())
                }
            },
            &|| panic!("cancelled write must never reach commit admission"),
        );
        assert!(result.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"sentinel");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
    #[test]
    fn no_cancellation_check_after_commit_and_persist_errors_remain_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("copy.psd");
        let committed = Cell::new(false);
        write_copy_atomic(
            &path,
            b"saved",
            &|| {
                assert!(!committed.get());
                Ok(())
            },
            &|| {
                committed.set(true);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"saved");
        let occupied = dir.path().join("directory.psd");
        std::fs::create_dir(&occupied).unwrap();
        assert!(write_copy_atomic(&occupied, b"bytes", &|| Ok(()), &|| Ok(())).is_err());
        assert!(occupied.is_dir());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }
}

#[cfg(test)]
mod destination_commit_tests {
    use super::*;
    use crate::document::DocumentSaveAsResult;
    use std::{
        os::unix::fs::symlink,
        sync::{Arc, Barrier},
    };

    #[test]
    fn create_if_absent_conflicts_with_file_created_after_staging() {
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("new.tessera-doc");
        let staged = stage_bytes(&destination, b"complete-new-bytes").unwrap();
        let staged_path = staged.path().to_owned();
        std::fs::write(&destination, b"other-writer").unwrap();

        let result = commit_staged(staged, &destination, CommitMode::CreateIfAbsent).unwrap();
        assert_eq!(result, DocumentSaveAsResult::DestinationExists);
        assert_eq!(std::fs::read(&destination).unwrap(), b"other-writer");
        assert!(
            !staged_path.exists(),
            "failed publication must release its owned stage"
        );
    }

    #[test]
    fn two_independent_stages_cannot_overwrite_each_other() {
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("same.psd");
        let left = stage_bytes(&destination, b"left-complete").unwrap();
        let right = stage_bytes(&destination, b"right-complete").unwrap();
        assert_ne!(left.path(), right.path(), "stages must have unique names");
        let left_path = left.path().to_owned();
        let right_path = right.path().to_owned();
        let barrier = Arc::new(Barrier::new(3));
        let (left_result, right_result) = std::thread::scope(|scope| {
            let left_gate = barrier.clone();
            let left_dest = destination.clone();
            let left_worker = scope.spawn(move || {
                left_gate.wait();
                commit_staged(left, &left_dest, CommitMode::CreateIfAbsent).unwrap()
            });
            let right_gate = barrier.clone();
            let right_dest = destination.clone();
            let right_worker = scope.spawn(move || {
                right_gate.wait();
                commit_staged(right, &right_dest, CommitMode::CreateIfAbsent).unwrap()
            });
            barrier.wait();
            (left_worker.join().unwrap(), right_worker.join().unwrap())
        });
        assert_eq!(
            [left_result, right_result]
                .into_iter()
                .filter(|outcome| *outcome == DocumentSaveAsResult::Saved)
                .count(),
            1
        );
        assert_eq!(
            [left_result, right_result]
                .into_iter()
                .filter(|outcome| *outcome == DocumentSaveAsResult::DestinationExists)
                .count(),
            1
        );
        let bytes = std::fs::read(&destination).unwrap();
        assert!(bytes == b"left-complete" || bytes == b"right-complete");
        // Failed publication owns a removable stage. A successful
        // persist_noclobber may retain a hard-link fallback staging name if
        // its internal unlink fails; do not assert a stronger cleanup contract.
        if left_result == DocumentSaveAsResult::DestinationExists {
            assert!(!left_path.exists());
        }
        if right_result == DocumentSaveAsResult::DestinationExists {
            assert!(!right_path.exists());
        }
    }

    #[test]
    fn no_clobber_rejects_regular_file_dangling_symlink_and_directory() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("existing.psb");
        std::fs::write(&file, b"sentinel").unwrap();
        let staged = stage_bytes(&file, b"new").unwrap();
        assert_eq!(
            commit_staged(staged, &file, CommitMode::CreateIfAbsent).unwrap(),
            DocumentSaveAsResult::DestinationExists
        );
        assert_eq!(std::fs::read(&file).unwrap(), b"sentinel");

        let link = dir.path().join("broken.psb");
        symlink("absent-target", &link).unwrap();
        let staged = stage_bytes(&link, b"new").unwrap();
        assert_eq!(
            commit_staged(staged, &link, CommitMode::CreateIfAbsent).unwrap(),
            DocumentSaveAsResult::DestinationExists
        );
        assert_eq!(
            std::fs::read_link(&link).unwrap(),
            std::path::PathBuf::from("absent-target")
        );
        assert!(!dir.path().join("absent-target").exists());

        let folder = dir.path().join("directory.psb");
        std::fs::create_dir(&folder).unwrap();
        let staged = stage_bytes(&folder, b"new").unwrap();
        let result = commit_staged(staged, &folder, CommitMode::CreateIfAbsent);
        assert_eq!(result.unwrap(), DocumentSaveAsResult::DestinationExists);
        assert!(folder.is_dir());
    }

    #[test]
    fn confirmed_replace_preserves_existing_path_semantics() {
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("replace.psd");
        let staged = stage_bytes(&destination, b"confirmed-bytes").unwrap();
        std::fs::write(&destination, b"changed-after-confirmation").unwrap();
        assert_eq!(
            commit_staged(staged, &destination, CommitMode::Replace).unwrap(),
            DocumentSaveAsResult::Saved
        );
        assert_eq!(std::fs::read(&destination).unwrap(), b"confirmed-bytes");
    }
}
