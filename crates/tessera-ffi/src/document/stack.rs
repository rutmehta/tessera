//! Edit ▸ Auto-Align Layers, Edit ▸ Auto-Blend Layers and File ▸ Automate ▸
//! Photomerge in document mode (WP B5-19) over the compositor's
//! `DocOp::AutoAlignLayers`, `DocOp::AutoBlendLayers` and `DocOp::Photomerge`.
//!
//! Every call is one history node. Selections are validated here first, in
//! words the sheets can show (two or more top-level, unlocked, unclipped
//! layers; pixel layers for alignment, pixel or aligned layers for blending),
//! so nothing changes on error; the engine validates again atomically.
//!
//! Memory: every stack is limited to [`stack_max_megapixels`] in total (half
//! of physical memory at ~50 B per pixel, at most [`MAX_STACK_MEGAPIXELS`]),
//! checked before anything is decoded or copied (Photomerge reads each
//! file's size from its header). Photomerge takes JPEG / PNG / TIFF files
//! and library photos only, and converts each photo into the target
//! document's colour profile (a new document takes the first photo's).
//!
//! Cancellation: Photomerge checks its `CancelFlag` before and while
//! decoding each photo and once more before the edit. Alignment and blending
//! themselves are not cancellable yet (`merge::layers` takes no token).
//! Lens corrections (vignette removal, geometric distortion) need one
//! explicit calibration per layer; library lens profiles are not mapped.
//! Reposition is not offered: `merge::layers` mis-registers it (see
//! `reposition_is_withheld_while_the_engine_misregisters_it`).

use super::{DocumentSession, DocumentUpdate, Opened, io};
use crate::{Engine, Result, export::CancelFlag, failure};
use compositor::{ColorProfile, DocOp, DocState, Document, LayerId, LayerKind, Raster};
use engine_api::{EngineResult, jobs::CancellationToken, tile::Extent};
use merge::{
    LinearImage,
    layers::{AlignMode, AlignOptions, BlendMode, BlendOptions, LensCorrection},
};
use std::{path::Path, sync::Arc, sync::atomic::AtomicBool};

/// Most layers or photos one stack takes (`merge::layers`).
const MAX_STACK: usize = 128;

/// Most pixels one stack takes on any machine, in megapixels (all layers or
/// photos together): e.g. eight 24 MP or four 48 MP photos.
pub const MAX_STACK_MEGAPIXELS: u64 = 200;

/// Alignment and blending keep several full-resolution float copies: about
/// 50 bytes per source pixel at peak (200 MP ⇒ ~10 GB).
const STACK_BYTES_PER_PIXEL: u64 = 50;

/// A stack may use up to this share of physical memory (1 / n).
const STACK_MEMORY_SHARE: u64 = 2;

/// The error a cancelled Photomerge returns (nothing changed).
pub const PHOTOMERGE_CANCELLED: &str = "Photomerge was cancelled";

/// The stack budget, in whole megapixels, for a machine with
/// `physical_memory` bytes of RAM (`None`: unknown): half of it at
/// [`STACK_BYTES_PER_PIXEL`], capped at [`MAX_STACK_MEGAPIXELS`], at least 1.
pub fn stack_megapixels_for_memory(physical_memory: Option<u64>) -> u64 {
    physical_memory.map_or(MAX_STACK_MEGAPIXELS, |bytes| {
        (bytes / STACK_MEMORY_SHARE / STACK_BYTES_PER_PIXEL / 1_000_000)
            .clamp(1, MAX_STACK_MEGAPIXELS)
    })
}

/// Installed RAM in bytes, when the OS says.
#[cfg(target_os = "macos")]
fn physical_memory() -> Option<u64> {
    use std::ffi::{c_char, c_int, c_void};
    unsafe extern "C" {
        fn sysctlbyname(
            name: *const c_char,
            oldp: *mut c_void,
            oldlenp: *mut usize,
            newp: *mut c_void,
            newlen: usize,
        ) -> c_int;
    }
    let mut bytes = 0u64;
    let mut len = std::mem::size_of::<u64>();
    // SAFETY: `hw.memsize` is a u64; `bytes` / `len` describe a writable u64
    // and nothing is set (`newp` null).
    let rc = unsafe {
        sysctlbyname(
            c"hw.memsize".as_ptr(),
            (&raw mut bytes).cast(),
            &raw mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    (rc == 0 && len == std::mem::size_of::<u64>() && bytes > 0).then_some(bytes)
}

/// Installed RAM in bytes, from `/proc/meminfo` where there is one.
#[cfg(not(target_os = "macos"))]
fn physical_memory() -> Option<u64> {
    let info = std::fs::read_to_string("/proc/meminfo").ok()?;
    let kb = info
        .lines()
        .find_map(|l| l.strip_prefix("MemTotal:"))?
        .trim()
        .strip_suffix("kB")?
        .trim()
        .parse::<u64>()
        .ok()?;
    Some(kb * 1024)
}

/// This machine's stack budget in megapixels (read once).
fn budget_megapixels() -> u64 {
    static BUDGET: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *BUDGET.get_or_init(|| stack_megapixels_for_memory(physical_memory()))
}

/// This machine's stack budget in megapixels (all layers or photos of one
/// stack): [`stack_megapixels_for_memory`] of its physical memory.
#[uniffi::export]
pub fn stack_max_megapixels() -> u64 {
    budget_megapixels()
}

fn megapixels(pixels: u64) -> u64 {
    pixels.div_ceil(1_000_000)
}

fn over_budget(what: &str, pixels: u64) -> Option<String> {
    over(what, pixels, budget_megapixels())
}

fn over(what: &str, pixels: u64, limit_megapixels: u64) -> Option<String> {
    (pixels > limit_megapixels.saturating_mul(1_000_000)).then(|| {
        format!(
            "{what} is limited to {limit_megapixels} megapixels in total on this computer; these have \
             {} megapixels. Use fewer or smaller images.",
            megapixels(pixels)
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_error_names_the_actual_limit() {
        let msg = over("Photomerge", 90_000_001, 85).unwrap();
        assert!(msg.contains("limited to 85 megapixels"), "{msg}");
        assert!(msg.contains("these have 91 megapixels"), "{msg}");
        assert_eq!(over("Photomerge", 85_000_000, 85), None);
    }

    #[test]
    fn profiles_match_by_handle_or_by_bytes_up_to_the_creation_date() {
        let srgb = io::profile(None).unwrap().unwrap();
        let bytes = srgb.icc.as_deref().unwrap().clone();
        let unembedded = ColorProfile {
            icc: None,
            ..srgb.clone()
        };
        assert!(same_profile(&srgb, &unembedded));
        let mut later = bytes.clone();
        later[35] ^= 1; // creation time, seconds
        let later = ColorProfile::from_icc("sRGB", later);
        assert_ne!(later.handle, srgb.handle);
        assert!(same_profile(&srgb, &later));
        assert!(!same_profile(&unembedded, &later));
        let mut other = bytes;
        other[200] ^= 1;
        assert!(!same_profile(&srgb, &ColorProfile::from_icc("x", other)));
        let p3 = io::profile(Some("Display P3")).unwrap().unwrap();
        assert!(!same_profile(&srgb, &p3));
    }
}

/// Auto-Align / Photomerge projection ("Layout" in Photoshop). Photoshop's
/// Reposition is withheld until `merge::layers` registers it correctly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum StackAlignMode {
    Auto,
    Perspective,
    Cylindrical,
    Spherical,
    Collage,
}

/// Auto-Blend method.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum StackBlendMode {
    /// Seams between overlapping images.
    Panorama,
    /// Focus stacking: each region from its sharpest layer.
    StackImages,
}

/// One layer's radial lens calibration (`merge::layers::LensCorrection`):
/// distortion maps ideal to observed radius by `r·(1 + k1·r² + k2·r⁴ +
/// k3·r⁶)`; vignette coefficients describe observed illumination.
#[derive(Clone, Copy, Debug, Default, PartialEq, uniffi::Record)]
pub struct StackLensCorrection {
    pub k1: f64,
    pub k2: f64,
    pub k3: f64,
    pub v1: f64,
    pub v2: f64,
    pub v3: f64,
}

/// Auto-Align Layers options.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct StackAlignOptions {
    pub mode: StackAlignMode,
    /// Index into the ids (or sources) of the layer that stays put.
    pub reference_index: u32,
    pub vignette_removal: bool,
    pub geometric_distortion: bool,
    /// One per layer, in order; required by either lens correction.
    pub lens_corrections: Vec<StackLensCorrection>,
    pub seed: u64,
}

/// Auto-Blend Layers options.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct StackBlendOptions {
    pub mode: StackBlendMode,
    pub seamless_tones: bool,
    /// Content-Aware Fill Transparent Areas: one new layer filling the
    /// pixels no layer covers.
    pub content_aware_fill: bool,
    pub seed: u64,
}

/// Whether the given layers can be aligned / blended, and why not.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct StackEligibility {
    pub can_align: bool,
    pub can_blend: bool,
    /// The alignment problem, else the blend problem.
    pub reason: Option<String>,
}

/// Photoshop's defaults: Auto layout, no lens corrections.
#[uniffi::export]
pub fn default_stack_align_options() -> StackAlignOptions {
    StackAlignOptions {
        mode: StackAlignMode::Auto,
        reference_index: 0,
        vignette_removal: false,
        geometric_distortion: false,
        lens_corrections: Vec::new(),
        seed: AlignOptions::default().seed,
    }
}

/// Photoshop's defaults: Panorama with seamless tones, no fill.
#[uniffi::export]
pub fn default_stack_blend_options() -> StackBlendOptions {
    let d = BlendOptions::default();
    StackBlendOptions {
        mode: StackBlendMode::Panorama,
        seamless_tones: d.seamless_tones,
        content_aware_fill: d.fill_transparent,
        seed: d.seed,
    }
}

impl From<StackAlignMode> for AlignMode {
    fn from(m: StackAlignMode) -> Self {
        match m {
            StackAlignMode::Auto => AlignMode::Auto,
            StackAlignMode::Perspective => AlignMode::Perspective,
            StackAlignMode::Cylindrical => AlignMode::Cylindrical,
            StackAlignMode::Spherical => AlignMode::Spherical,
            StackAlignMode::Collage => AlignMode::Collage,
        }
    }
}

impl From<StackBlendMode> for BlendMode {
    fn from(m: StackBlendMode) -> Self {
        match m {
            StackBlendMode::Panorama => BlendMode::Panorama,
            StackBlendMode::StackImages => BlendMode::StackImages,
        }
    }
}

/// The engine's options for `count` layers, after the checks the engine
/// would report less clearly.
fn align_options(o: &StackAlignOptions, count: usize) -> Result<AlignOptions> {
    if o.reference_index as usize >= count {
        return Err(failure(format!(
            "the reference layer must be one of the {count} layers"
        )));
    }
    if (o.vignette_removal || o.geometric_distortion) && o.lens_corrections.len() != count {
        let what = if o.vignette_removal {
            "Vignette removal"
        } else {
            "Geometric distortion correction"
        };
        return Err(failure(format!(
            "{what} needs a lens calibration for every layer ({} given for {count})",
            o.lens_corrections.len()
        )));
    }
    Ok(AlignOptions {
        mode: o.mode.into(),
        reference: o.reference_index as usize,
        seed: o.seed,
        vignette_removal: o.vignette_removal,
        geometric_distortion: o.geometric_distortion,
        lens_corrections: o
            .lens_corrections
            .iter()
            .map(|c| LensCorrection {
                distortion: [c.k1, c.k2, c.k3],
                vignette: [c.v1, c.v2, c.v3],
            })
            .collect(),
    })
}

fn blend_options(o: &StackBlendOptions) -> BlendOptions {
    BlendOptions {
        mode: o.mode.into(),
        seamless_tones: o.seamless_tones,
        fill_transparent: o.content_aware_fill,
        seed: o.seed,
        ..BlendOptions::default()
    }
}

/// `DocOp`'s Content-Aware Fill hook: `filters::caf::fill` (non-capturing,
/// so it coerces to `compositor::edit::ContentAwareFill`).
fn content_aware_fill(input: &Raster, holes: &[f32], seed: u64) -> EngineResult<Raster> {
    let params = filters::caf::FillParams {
        seed,
        ..Default::default()
    };
    Ok(filters::caf::fill(input, holes, &params, &AtomicBool::new(false))?.composite)
}

fn fill_for(o: &StackBlendOptions) -> Option<compositor::edit::ContentAwareFill> {
    o.content_aware_fill
        .then_some(content_aware_fill as compositor::edit::ContentAwareFill)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Align,
    Blend,
}

/// Why `ids` cannot be aligned (`Stage::Align`) or blended, in the sheets'
/// words; `None` when they can.
fn problem(s: &DocState, ids: &[u64], stage: Stage) -> Option<String> {
    let (verb, title) = match stage {
        Stage::Align => ("align", "Auto-Align"),
        Stage::Blend => ("blend", "Auto-Blend"),
    };
    if ids.len() < 2 {
        return Some(format!("Select two or more layers to {verb}"));
    }
    if ids.len() > MAX_STACK {
        return Some(format!("{title} takes at most {MAX_STACK} layers"));
    }
    let mut pixels = 0u64;
    for (i, id) in ids.iter().enumerate() {
        if ids[..i].contains(id) {
            return Some(format!("layer {id} is selected twice"));
        }
        let Some(l) = s.root.iter().find(|l| l.id.0 == *id) else {
            return Some(match s.find(LayerId(*id)) {
                Some(l) => format!(
                    "{title} works on top-level layers; “{}” is inside a group",
                    l.props.name
                ),
                None => format!("layer {id} not found"),
            });
        };
        let name = &l.props.name;
        let locks = l.props.locks;
        if locks.all || locks.pixels || locks.position {
            return Some(format!("“{name}” is locked"));
        }
        if l.props.clipped {
            return Some(format!("“{name}” is part of a clipping mask"));
        }
        let ok = match (&l.kind, stage) {
            (LayerKind::Pixel(r), _) => r.channels() == 4,
            (LayerKind::SmartObject(_), Stage::Blend) => true,
            _ => false,
        };
        if !ok {
            return Some(format!(
                "{title} needs pixel layers; “{name}” is not a pixel layer"
            ));
        }
        let extent = match &l.kind {
            LayerKind::Pixel(r) => r.extent(),
            LayerKind::SmartObject(so) => so.state.canvas,
            _ => Extent::new(0, 0),
        };
        pixels = pixels.saturating_add(u64::from(extent.width) * u64::from(extent.height));
    }
    over_budget(title, pixels)
}

fn is_image_id(s: &str) -> bool {
    s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn ext(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// Pixel count from the file header only (no decode); `None` when the
/// header cannot say (HEIC, linear DNG): those count after decoding.
fn header_pixels(path: &Path) -> Option<u64> {
    let (w, h) = if matches!(ext(path).as_str(), "tif" | "tiff") {
        let file = std::io::BufReader::new(std::fs::File::open(path).ok()?);
        tiff::decoder::Decoder::new(file).ok()?.dimensions().ok()?
    } else if image_core::RgbSource::recognizes(path) {
        image::ImageReader::open(path)
            .ok()?
            .with_guessed_format()
            .ok()?
            .into_dimensions()
            .ok()?
    } else {
        let m = raw_decode::RawSource::open(path).ok()?.metadata();
        (m.width, m.height)
    };
    Some(u64::from(w) * u64::from(h))
}

/// One Photomerge source, resolved but not decoded.
enum Source {
    Image { id: String, path: String },
    File(String),
}

impl Source {
    fn resolve(engine: &Arc<Engine>, s: &str) -> Result<Self> {
        if is_image_id(s) {
            let c = engine.lock()?;
            let path =
                Engine::path(&c, s).map_err(|_| failure("photo not found in the library"))?;
            return Ok(Self::Image {
                id: s.to_owned(),
                path,
            });
        }
        match ext(Path::new(s)).as_str() {
            "jpg" | "jpeg" | "png" | "tif" | "tiff" => Ok(Self::File(s.to_owned())),
            _ => Err(failure(
                "Photomerge takes JPEG, PNG or TIFF files or library photos",
            )),
        }
    }

    fn path(&self) -> &Path {
        match self {
            Self::Image { path, .. } | Self::File(path) => Path::new(path),
        }
    }

    /// Decodes into its display name, the composite's RGB converted into
    /// `target` (`None`: keep the photo's own profile) and that profile.
    fn load(
        &self,
        engine: &Arc<Engine>,
        target: Option<&ColorProfile>,
        cancel: &CancellationToken,
    ) -> Result<(String, LinearImage, ColorProfile)> {
        let (opened, name) = match self {
            Self::Image { id, .. } => {
                let o = io::open_image(engine, id, true, cancel)?;
                let name = o.title.clone();
                (o, name)
            }
            Self::File(path) => {
                let name = Path::new(path)
                    .file_stem()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Layer".into());
                (io::open_path(Path::new(path))?, name)
            }
        };
        let source = profile_or_srgb(opened.doc.state().profile.as_ref())?;
        let (extent, mut rgba) = compositor::Compositor::new(64 << 20)
            .render_level_rgba_with_cancel(&opened.doc, 0, cancel)?;
        drop(opened);
        let target = target.cloned().unwrap_or_else(|| source.clone());
        // The same profile needs no conversion, embedded or not.
        if !same_profile(&source, &target) {
            io::convert(icc(&source)?, icc(&target)?, &mut rgba)?;
        }
        let image = LinearImage {
            width: extent.width as usize,
            height: extent.height as usize,
            pixels: rgba
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| [p[0], p[1], p[2]])
                .collect(),
            color_matrix: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
            as_shot_neutral: [1.; 3],
        };
        Ok((name, image, target))
    }
}

/// A document's profile, untagged meaning sRGB.
fn profile_or_srgb(p: Option<&ColorProfile>) -> Result<ColorProfile> {
    match p {
        Some(p) => Ok(p.clone()),
        None => Ok(io::profile(None)?.expect("sRGB")),
    }
}

/// Whether `a` and `b` are one profile: the same handle (digest of the
/// bytes; enough when either is not embedded), or embedded bytes that differ
/// only in the header's creation date and profile ID (built-in profiles are
/// generated with the current time).
pub(super) fn same_profile(a: &ColorProfile, b: &ColorProfile) -> bool {
    if a.handle == b.handle {
        return true;
    }
    let (Some(x), Some(y)) = (a.icc.as_deref(), b.icc.as_deref()) else {
        return false;
    };
    const DATE: std::ops::Range<usize> = 24..36;
    const ID: std::ops::Range<usize> = 84..100;
    x.len() == y.len()
        && x.len() >= ID.end
        && x.iter()
            .zip(y.iter())
            .enumerate()
            .all(|(i, (p, q))| p == q || DATE.contains(&i) || ID.contains(&i))
}

fn icc(p: &ColorProfile) -> Result<&[u8]> {
    p.icc.as_deref().map(Vec::as_slice).ok_or_else(|| {
        failure(format!(
            "the colour profile “{}” is not embedded, so photos cannot be converted to it",
            p.name
        ))
    })
}

/// Checks the count and the pixel budget (from headers), then decodes every
/// source in order, converting each into `target` (`None`: the first
/// photo's profile). Returns the images and the profile they are in.
fn load_sources(
    engine: &Arc<Engine>,
    sources: &[String],
    target: Option<ColorProfile>,
    cancel: &CancellationToken,
) -> Result<(Vec<(String, LinearImage)>, ColorProfile)> {
    if sources.len() < 2 {
        return Err(failure("Photomerge needs two or more photos"));
    }
    if sources.len() > MAX_STACK {
        return Err(failure(format!(
            "Photomerge takes at most {MAX_STACK} photos"
        )));
    }
    let named = |s: &str, e: crate::BridgeError| failure(format!("{s}: {e}"));
    let resolved = sources
        .iter()
        .map(|s| Source::resolve(engine, s).map_err(|e| named(s, e)))
        .collect::<Result<Vec<_>>>()?;
    let headers: Vec<_> = resolved.iter().map(|s| header_pixels(s.path())).collect();
    let known = headers
        .iter()
        .flatten()
        .fold(0u64, |a, b| a.saturating_add(*b));
    if let Some(p) = over_budget("Photomerge", known) {
        return Err(failure(p));
    }
    let mut target = target;
    let mut total = known;
    let mut images = Vec::with_capacity(sources.len());
    for ((source, s), header) in resolved.iter().zip(sources).zip(&headers) {
        if cancel.is_cancelled() {
            return Err(failure(PHOTOMERGE_CANCELLED));
        }
        let loaded = source.load(engine, target.as_ref(), cancel);
        if cancel.is_cancelled() {
            return Err(failure(PHOTOMERGE_CANCELLED));
        }
        let (name, image, profile) = loaded.map_err(|e| named(s, e))?;
        if header.is_none() {
            total = total.saturating_add((image.width * image.height) as u64);
            if let Some(p) = over_budget("Photomerge", total) {
                return Err(failure(p));
            }
        }
        target.get_or_insert(profile);
        images.push((name, image));
    }
    if cancel.is_cancelled() {
        return Err(failure(PHOTOMERGE_CANCELLED));
    }
    Ok((images, target.expect("two or more sources")))
}

fn photomerge_op(
    images: Vec<(String, LinearImage)>,
    align: &StackAlignOptions,
    blend: &StackBlendOptions,
) -> Result<DocOp> {
    Ok(DocOp::Photomerge {
        align: align_options(align, images.len())?,
        images,
        blend: blend_options(blend),
        fill: fill_for(blend),
    })
}

#[uniffi::export]
impl DocumentSession {
    /// Whether `ids` (Layers panel selection) can be aligned and blended.
    pub fn stack_eligibility(&self, ids: Vec<u64>) -> Result<StackEligibility> {
        let st = self.shared.read()?;
        let s = st.live().state();
        let align = problem(s, &ids, Stage::Align);
        let blend = problem(s, &ids, Stage::Blend);
        Ok(StackEligibility {
            can_align: align.is_none(),
            can_blend: blend.is_none(),
            reason: align.or(blend),
        })
    }

    /// Edit ▸ Auto-Align Layers: registers the root pixel layers `ids` (the
    /// reference stays put), extends the canvas to their union and keeps each
    /// source with its transform editable. One history node. Blocking.
    pub fn auto_align_layers(
        &self,
        ids: Vec<u64>,
        options: StackAlignOptions,
    ) -> Result<DocumentUpdate> {
        {
            let st = self.shared.lock()?;
            if let Some(p) = problem(st.live().state(), &ids, Stage::Align) {
                return Err(failure(p));
            }
        }
        let options = align_options(&options, ids.len())?;
        self.edit(
            DocOp::AutoAlignLayers {
                ids: ids.into_iter().map(LayerId).collect(),
                options,
            },
            Some("Auto-Align Layers"),
        )
    }

    /// Edit ▸ Auto-Blend Layers: editable masks on each layer (panorama
    /// seams or focus stacking), optional seamless-tone correction layers
    /// and, when asked, one Content-Aware Fill layer for uncovered pixels.
    /// One history node. Blocking.
    pub fn auto_blend_layers(
        &self,
        ids: Vec<u64>,
        options: StackBlendOptions,
    ) -> Result<DocumentUpdate> {
        {
            let st = self.shared.lock()?;
            if let Some(p) = problem(st.live().state(), &ids, Stage::Blend) {
                return Err(failure(p));
            }
        }
        self.edit(
            DocOp::AutoBlendLayers {
                ids: ids.into_iter().map(LayerId).collect(),
                options: blend_options(&options),
                fill: fill_for(&options),
            },
            Some("Auto-Blend Layers"),
        )
    }

    /// Photomerge into this document: each source (library image id or
    /// JPEG / PNG / TIFF path) becomes a named top-level layer, converted to
    /// the document's profile, aligned and blended, as one history node.
    /// Blocking (decodes every source first); `cancel` stops it before the
    /// edit with [`PHOTOMERGE_CANCELLED`] and nothing changed.
    pub fn photomerge_into_layers(
        &self,
        sources: Vec<String>,
        align: StackAlignOptions,
        blend: StackBlendOptions,
        cancel: Arc<CancelFlag>,
    ) -> Result<DocumentUpdate> {
        let engine = self
            .shared
            .engine
            .upgrade()
            .ok_or_else(|| failure("engine is closed"))?;
        align_options(&align, sources.len().max(1))?;
        let profile = {
            let st = self.shared.lock()?;
            profile_or_srgb(st.live().state().profile.as_ref())?
        };
        let (images, _) = load_sources(&engine, &sources, Some(profile), cancel.token())?;
        self.edit(photomerge_op(images, &align, &blend)?, Some("Photomerge"))
    }
}

#[uniffi::export]
impl Engine {
    /// File ▸ Automate ▸ Photomerge: a new Untitled document, in the first
    /// photo's colour profile, whose one history node after "New Document"
    /// merges `sources` (library image ids or JPEG / PNG / TIFF paths) into
    /// named, aligned and blended layers. Blocking; `cancel` as for
    /// `photomerge_into_layers`.
    pub fn photomerge_document(
        self: Arc<Self>,
        sources: Vec<String>,
        align: StackAlignOptions,
        blend: StackBlendOptions,
        cancel: Arc<CancelFlag>,
    ) -> Result<Arc<DocumentSession>> {
        align_options(&align, sources.len().max(1))?;
        let (images, profile) = load_sources(&self, &sources, None, cancel.token())?;
        let op = photomerge_op(images, &align, &blend)?;
        let depth = compositor::Depth::F32;
        let mut state = DocState::new(Extent::new(1, 1), depth);
        state.profile = Some(profile);
        let session = self.register_document(
            None,
            Opened {
                doc: Document::new(state),
                title: "Untitled".into(),
                path: None,
                source_image_id: None,
                origin: "New Document",
                unsaved: true,
            },
        );
        if let Err(e) = session.edit(op, Some("Photomerge")) {
            session.close();
            return Err(e);
        }
        Ok(session)
    }
}
