//! Resident RAW export in horizontal bands that share the GPU with the viewport.
//!
//! Spec 08 §2 orders work UI > viewport > … > exports. There is no global
//! "one renderer at a time" lock: every export caller shares one device and
//! queue (wgpu queues are thread-safe) and only scratch memory is bounded.
//! The 512 MiB device scratch envelope is split into a viewport reserve and
//! an export budget; each band reserves its planned share of the export
//! budget, so concurrent exports interleave band by band. Before every band,
//! and at every sensor-chunk checkpoint inside one, export yields while any
//! interactive job is queued or running on a scheduler
//! ([`jobs::yield_to_interactive`]), so a slider drag never waits behind a
//! band that had not yet started.
//!
//! Bands are full-width row bands rendered with one dispatch per stage
//! (`Renderer::render_export_rows`), two in flight on their own threads,
//! each with half of the budget: one encodes and uploads while the other
//! executes and reads back. Recipes the band renderer declines (Texture,
//! Clarity, Dehaze) use the pyramid-tile renderer. Environment switches for
//! measurement: `TESSERA_EXPORT_TILES` (tile renderer only),
//! `TESSERA_EXPORT_IN_FLIGHT=n`, `TESSERA_EXPORT_WEB_LEVEL=1` (see
//! [`Options::web_level`]) and `TESSERA_EXPORT_TRACE=1`.

use crate::{ColorSpace, ExportImage, codec};
use engine_api::{
    EngineError, EngineResult,
    id::ImageId,
    jobs::CancellationToken,
    recipe::Recipe,
    tile::{Pyramid, TILE_SIZE},
};
use image_core::{PixelRect, RawImage, RendererConfig};
use pipeline_cpu::RenderSource;
use pipeline_gpu::{GpuContext, GpuManagedOutput, ManagedRenderer};
use std::sync::{Arc, Condvar, Mutex, OnceLock};

/// Device scratch envelope shared by interactive rendering and export.
pub(crate) const GPU_SCRATCH: usize = 512 << 20;
/// Always left to interactive (viewport) renders on the shared device.
pub(crate) const VIEWPORT_RESERVE: usize = 128 << 20;
/// Scratch all concurrent export bands may hold together.
pub(crate) const BUDGET: usize = GPU_SCRATCH - VIEWPORT_RESERVE;

static DEVICE: OnceLock<EngineResult<Arc<GpuContext>>> = OnceLock::new();
static RESERVED: Mutex<usize> = Mutex::new(0);
static RELEASED: Condvar = Condvar::new();

/// A band's share of [`BUDGET`], returned on drop.
struct Reservation(usize);

impl Reservation {
    fn acquire(bytes: usize, cancel: &CancellationToken) -> EngineResult<Self> {
        let bytes = bytes.clamp(1, BUDGET);
        let mut reserved = RESERVED.lock().unwrap_or_else(|e| e.into_inner());
        while *reserved + bytes > BUDGET {
            cancel.check()?;
            reserved = RELEASED
                .wait_timeout(reserved, std::time::Duration::from_millis(10))
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        *reserved += bytes;
        Ok(Self(bytes))
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        let mut reserved = RESERVED.lock().unwrap_or_else(|e| e.into_inner());
        *reserved -= self.0;
        drop(reserved);
        RELEASED.notify_all();
    }
}

/// Diagnostic: the Metal device's allocated bytes once the export device has
/// gone idle ([`GpuContext::idle_device_allocated_bytes`]), None without
/// Metal. Process-wide: callers must not overlap other GPU work.
#[doc(hidden)]
pub fn idle_device_allocated_bytes() -> Option<u64> {
    DEVICE
        .get_or_init(|| GpuContext::new().map(Arc::new))
        .as_ref()
        .ok()?
        .idle_device_allocated_bytes()
}

/// `TESSERA_EXPORT_TRACE=1` prints per-phase export timings to stderr.
pub(crate) fn trace(phase: &str, since: std::time::Instant) {
    static ON: OnceLock<bool> = OnceLock::new();
    if *ON.get_or_init(|| std::env::var_os("TESSERA_EXPORT_TRACE").is_some()) {
        eprintln!(
            "EXPORT_TRACE {phase} {:.1} ms",
            since.elapsed().as_secs_f64() * 1e3
        );
    }
}

fn trace_note(note: &str) {
    if std::env::var_os("TESSERA_EXPORT_TRACE").is_some() {
        eprintln!("EXPORT_TRACE {note}");
    }
}

pub(crate) fn render(
    image: &ExportImage<'_>,
    recipe: &Recipe,
    space: ColorSpace,
    scale: u32,
    cancel: &CancellationToken,
    budget: usize,
) -> EngineResult<Option<image::Rgb32FImage>> {
    render_resized(
        image,
        recipe,
        space,
        scale,
        cancel,
        budget,
        crate::Resize::None,
    )
}

pub(crate) fn render_resized(
    image: &ExportImage<'_>,
    recipe: &Recipe,
    space: ColorSpace,
    scale: u32,
    cancel: &CancellationToken,
    budget: usize,
    resize: crate::Resize,
) -> EngineResult<Option<image::Rgb32FImage>> {
    render_with_lens(image, recipe, space, scale, cancel, budget, resize, None)
}

// Band renderer scratch, fitted to the per-band allocations
// `TESSERA_EXPORT_TRACE=1` prints on the five fixtures (ENG-12b): the worst
// band of each camera, full chain at level 0, gives 25.1 B per sensor pixel
// and 95.6 B per developed pixel besides the readback; the CR3 (sensor 6288
// wide, developed 4000) separates the two. ENG-14 meters wgpu's staging
// copies and the parameter arena too: +4 B per sensor pixel and 2 MiB per
// band restore the largest actual/planned ratio to 0.99 (it reached 1.056).
// Planned bands keep [`BAND_MARGIN_DIVISOR`] of headroom below their share.
/// Per uploaded sensor pixel: the sensor-domain stages (raw upload and
/// gathers, highlights, demosaic, lateral CA), which run at the CFA width over
/// the band's sensor rows with all their halos, and wgpu's 4 B staging copy
/// of the raw upload (metered since ENG-14).
const SENSOR_BYTES_PER_PIXEL: usize = 30;
/// Per uploaded sensor pixel, when the lens plan has lateral CA (ENG-8: the
/// fit above had no CA): the CA resample's RGB output beside its input.
/// Fitted like the others on the RAF fixture with an estimated-CA sample:
/// its worst band (web pyramid level 1) needs 16.5 B for the 0.99
/// actual/planned ratio; 12 B left it at 1.041.
const CA_BYTES_PER_PIXEL: usize = 18;
/// The same for a maker-note built-in correction (ENG-8c), which resamples
/// all three channels in that stage: the RAF's worst band (web pyramid level
/// 1) needs 22.6 B for 0.99; 18 B left it at 1.045.
const MAKER_BYTES_PER_PIXEL: usize = 24;
/// Per band: the parameter arena and wgpu's staging copy of it (ENG-14).
const PARAMS_BYTES: usize = 2 << 20;
/// Per developed pixel (level-frame rows with the Detail halo): resample,
/// matrices, vignette, Detail, Tone/Color/Effects.
const DEVELOPED_BYTES_PER_PIXEL: usize = 96;
/// Per output pixel: the interleaved RGB readback buffer and its staging copy.
const READBACK_BYTES_PER_PIXEL: usize = 24;
/// Per output pixel with the export resize: its RGB output as well.
const RESIZED_READBACK_BYTES_PER_PIXEL: usize = 36;
/// Per developed pixel with the export resize: the copy of the developed band
/// and the resampler's intermediate rows (fitted: 25 to 31 B measured at full
/// resolution).
const RESIZE_SOURCE_BYTES_PER_PIXEL: usize = 32;
/// A band's planned scratch plus 1/20 (5%) must fit its share.
const BAND_MARGIN_DIVISOR: usize = 20;

/// A band's planned scratch (see the constants above), in bytes.
#[derive(Clone, Copy, Debug)]
struct BandCost {
    sensor: usize,
    developed: usize,
    mapped: usize,
    /// The readback, and the export resize when there is one.
    readback: usize,
    /// Parameters ([`PARAMS_BYTES`]).
    params: usize,
}

impl BandCost {
    fn total(&self) -> usize {
        self.sensor + self.developed + self.mapped + self.readback + self.params
    }

    fn fits(&self, share: usize) -> bool {
        let total = self.total();
        total.saturating_add(total / BAND_MARGIN_DIVISOR) <= share
    }
}
/// The map's mapped band and its display copy, per output pixel.
const MAP_BYTES_PER_PIXEL: usize = 48;
/// Bands in flight: one encodes and submits while the other executes.
const BANDS_IN_FLIGHT: usize = 2;
/// Upper bound on one band's developed pixels: bounds a single submission's
/// GPU time (viewport frames queue behind it) and the f32 plane-length limit.
const MAX_BAND_PIXELS: usize = 4 << 20;

/// The deepest pyramid level whose output frame still covers `destination`
/// (Web presets skip full resolution when the output is at most half size).
pub(crate) fn web_level(
    frame: impl Fn(u8) -> engine_api::tile::Extent,
    destination: (u32, u32),
) -> u8 {
    let mut level = 0;
    while level < 3 {
        let next = frame(level + 1);
        if next.width < destination.0 || next.height < destination.1 {
            break;
        }
        level += 1;
    }
    level
}

/// `resolved`: a precomputed lens correction (tests force calibrations);
/// None analyses the sensor like the reference renderer.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_with_lens(
    image: &ExportImage<'_>,
    recipe: &Recipe,
    space: ColorSpace,
    scale: u32,
    cancel: &CancellationToken,
    budget: usize,
    resize: crate::Resize,
    resolved: Option<pipeline_cpu::ResolvedLens>,
) -> EngineResult<Option<image::Rgb32FImage>> {
    render_with_options(
        image,
        recipe,
        space,
        scale,
        cancel,
        budget,
        resize,
        resolved,
        Options::default(),
    )
}

/// Scheduling choices (tests compare them; results must not depend on them).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Options {
    /// Full-width band renderer (else the M2-21b pyramid-tile path).
    pub bands: bool,
    /// Develop resized exports at the pyramid level covering the output
    /// (`TESSERA_EXPORT_WEB_LEVEL=1`). Off by default: it misses the docs/11
    /// §1.3 gate at Web scale (tone, Detail and output encoding do not
    /// commute with the box downsample; up to 56 codes on the fixtures, see
    /// `five_fixture_web_scale_tolerance`), while full-resolution development
    /// resized on the GPU meets it.
    pub web_level: bool,
    pub in_flight: usize,
    /// Test-only: build the effects constants map as interactive renders do
    /// (production exports use the inline path; ENG-14 compares the two).
    #[cfg(test)]
    pub effects_map: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            bands: std::env::var("TESSERA_EXPORT_TILES").is_err(),
            web_level: std::env::var("TESSERA_EXPORT_WEB_LEVEL").is_ok_and(|v| v == "1"),
            in_flight: std::env::var("TESSERA_EXPORT_IN_FLIGHT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(BANDS_IN_FLIGHT),
            #[cfg(test)]
            effects_map: false,
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_with_options(
    image: &ExportImage<'_>,
    recipe: &Recipe,
    space: ColorSpace,
    scale: u32,
    cancel: &CancellationToken,
    budget: usize,
    resize: crate::Resize,
    resolved: Option<pipeline_cpu::ResolvedLens>,
    options: Options,
) -> EngineResult<Option<image::Rgb32FImage>> {
    cancel.check()?;
    if !matches!(scale, 1 | 2 | 4 | 8) {
        return Err(EngineError::invalid("scale", "must be 1, 2, 4 or 8"));
    }
    let RenderSource::Cfa {
        image: cfa,
        metadata,
    } = &image.source
    else {
        return Ok(None);
    };
    if recipe.process_version != engine_api::recipe::ProcessVersion::NATIVE_CURRENT
        || !recipe.settings.locals.adjustments.is_empty()
        || pipeline_cpu::denoise_active(&recipe.settings.denoise)
    {
        return Ok(None);
    }
    let context = match DEVICE.get_or_init(|| GpuContext::new().map(Arc::new)) {
        Ok(context) => context.clone(),
        Err(_) => return Ok(None),
    };
    let started = std::time::Instant::now();
    // RenderSource borrows CFA storage whereas Renderer owns it. Copy only
    // the scalar sensor plane, never a full developed RGB intermediate.
    let pyramid = cfa.pyramid();
    let extent = pyramid.extent();
    let samples = pyramid.pixels().to_vec();
    trace("sensor copy", started);
    let started = std::time::Instant::now();
    let mut settings = recipe.settings.clone();
    settings.output.proof_profile = None;
    // Auto lens correction analyses the developed frame; the sparse sensor
    // analysis reproduces the reference's decisions without a CPU demosaic.
    let resolved = match resolved {
        Some(resolved) => resolved,
        None => {
            pipeline_cpu::resolve_lens_sensor(&samples, metadata, &settings, &Default::default())?
        }
    };
    let Some(lens) = resolved.plan(&settings, metadata)? else {
        return Ok(None);
    };
    trace("lens analysis", started);
    let raw = RawImage::new(
        ImageId(1),
        Arc::new(raw_decode::CfaImage::from_linear(
            extent.width,
            extent.height,
            samples,
        )?),
        Arc::new((*metadata).clone()),
    )?;
    let requested = scale.trailing_zeros() as u8;
    let output_frame = |level| image_core::Renderer::lens_output_extent(&raw, level, Some(&lens));
    // Output dimensions follow the requested level, whatever level renders.
    let requested_frame = output_frame(requested);
    let (width, height) = resize.dimensions(requested_frame.width, requested_frame.height)?;
    let level = if options.web_level && !matches!(resize, crate::Resize::None) {
        web_level(output_frame, (width, height)).max(requested)
    } else {
        requested
    };
    let frame = output_frame(level);
    // Resident resize is a downsampling path. Enlargements (up to the public
    // 100 MP limit) retain the bounded, row-parallel CPU resampler rather than
    // allocating an expanded GPU band that could exceed a device buffer limit.
    if width > frame.width || height > frame.height {
        return Ok(None);
    }
    let destination = engine_api::tile::Extent::new(width, height);
    // Texture, Clarity and Dehaze render only through the tile path, and only
    // when one tile band covers the frame. Decide that before the output
    // uploads its tables (ENG-15): the budget here bounds the job's, which
    // only loses the output's tables, so a frame declined here is declined
    // below too.
    if has_presence(&recipe.settings.tone)
        && tile_band_rows(
            raw.level_extent(level),
            frame,
            lens.map.is_some(),
            level,
            budget.min(BUDGET),
        ) < frame.height
    {
        trace_note("presence recipe over one tile band; CPU export");
        return Ok(None);
    }
    // Whatever path follows (bands, tiles, or a decline from either), flush
    // the uploads queued so far when the export ends: wgpu keeps their
    // staging until the next submission, which a CPU fallback never makes
    // (REV-ENG-14 SHOULD-FIX 2: 1.8 MiB per declined export, unbounded in a
    // headless batch).
    let _flush = FlushUploads(context.clone());
    let mut registry = color_mgmt::Registry::new();
    let target = codec::profile(&mut registry, space)?;
    let output = Arc::new(GpuManagedOutput::new(
        context,
        &settings,
        &mut pipeline_cpu::OutputContext {
            registry: &mut registry,
            target: pipeline_cpu::OutputTarget::Export(&target),
            proof: None,
            options: color_mgmt::TransformOptions::default(),
        },
    )?);
    let config = RendererConfig {
        cache_budget_bytes: 0,
        ..Default::default()
    };
    // The output's tables live for the whole export, shared by its band
    // workers: they come off the budget before it is split (ENG-14).
    let fixed = usize::try_from(output.device_bytes()).unwrap_or(usize::MAX);
    let scratch = BUDGET.saturating_sub(fixed).max(1);
    let budget = budget.min(scratch);
    let job = Job {
        raw: &raw,
        settings: &settings,
        lens: &lens,
        level,
        frame,
        destination,
        budget,
        scratch,
        fixed,
        cancel,
    };
    if options.bands && !has_presence(&recipe.settings.tone) {
        let in_flight = options.in_flight.max(1);
        // Pipelines compile once per export, not once per band.
        // Each band in flight may allocate its share of the device budget;
        // `budget` (tests pass tiny ones) only sizes the bands.
        let base = ManagedRenderer::new_export_budgeted(
            output.clone(),
            config.clone(),
            None,
            (scratch / in_flight) as u64,
        );
        #[cfg(test)]
        let base = if options.effects_map {
            base.with_export_effects_map()
        } else {
            base
        };
        match render_bands(&job, &base, in_flight) {
            Ok(Some(rgb)) => {
                LAST_PATH.set("bands");
                return Ok(Some(rgb));
            }
            Ok(None) => trace_note("band renderer declined; pyramid tiles"),
            Err(EngineError::Unsupported { what }) => trace_note(&format!(
                "band renderer unsupported ({what}); pyramid tiles"
            )),
            Err(e) => return Err(e),
        }
    }
    let base = ManagedRenderer::new_export_budgeted(output, config, None, scratch as u64);
    #[cfg(test)]
    let base = if options.effects_map {
        base.with_export_effects_map()
    } else {
        base
    };
    LAST_PATH.set("tiles");
    render_tiles(&job, &base)
}

thread_local! {
    /// The renderer the last GPU export on this thread used ("bands" or
    /// "tiles"): tests assert which path produced their pixels.
    pub(crate) static LAST_PATH: std::cell::Cell<&'static str> = const { std::cell::Cell::new("") };
}

/// One rendered band's device footprint (ENG-13), as the backend measured it.
#[cfg(test)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct BandFootprint {
    pub(crate) top: u32,
    pub(crate) rows: u32,
    /// Bytes the band's transaction held at readback (`recycled + fresh -
    /// released`), excluding the readback staging copy.
    pub(crate) live: u64,
    /// The largest footprint during the band.
    pub(crate) peak: u64,
    /// The readback staging copy, allocated after the scratch.
    pub(crate) readback: u64,
    /// What the budget counter (and the trace's "actual") reported.
    pub(crate) counted: u64,
    /// Recycled buffers the band took in from the worker's previous band.
    pub(crate) recycled: u64,
    /// The scratch the band renderer is given (`(BUDGET - fixed) / in_flight`).
    pub(crate) share: u64,
    /// Effects constants maps the band built (ENG-14: none in production).
    pub(crate) effects_maps: u64,
}

#[cfg(test)]
thread_local! {
    /// Footprints of the bands of GPU exports run on this thread.
    pub(crate) static BAND_FOOTPRINTS: std::cell::RefCell<Vec<BandFootprint>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn has_presence(tone: &engine_api::recipe::settings::ToneSettings) -> bool {
    tone.texture != 0. || tone.clarity != 0. || tone.dehaze != 0.
}

/// One export's resolved inputs, shared by its band workers.
struct Job<'a> {
    raw: &'a RawImage,
    settings: &'a engine_api::recipe::DevelopSettings,
    lens: &'a pipeline_cpu::LensPlan,
    level: u8,
    /// The rendered output frame at `level` (mapped with a lens map).
    frame: engine_api::tile::Extent,
    destination: engine_api::tile::Extent,
    /// Sizes bands (at most `scratch`; tests pass tiny ones).
    budget: usize,
    /// The export scratch band renderers share: [`BUDGET`] less `fixed`.
    scratch: usize,
    /// Device bytes the export holds outside every band (the output's
    /// tables), reserved alongside the bands' shares.
    fixed: usize,
    cancel: &'a CancellationToken,
}

/// Full-width bands, `in_flight` at a time (each on its own thread with its
/// share of the budget): one encodes and uploads while another executes and
/// reads back. Returns None when the recipe needs the tiled path.
fn render_bands(
    job: &Job<'_>,
    base: &ManagedRenderer,
    in_flight: usize,
) -> EngineResult<Option<image::Rgb32FImage>> {
    let started = std::time::Instant::now();
    let (frame, destination) = (job.frame, job.destination);
    let resizing = destination != frame;
    // Output (post-resize) rows per band: the largest count whose worst band
    // fits this band's share of the budget.
    let share = (job.budget / in_flight).max(1);
    let developed = job.raw.level_extent(job.level);
    let geometry = base.export_band_geometry(job.raw, job.settings, job.level, Some(job.lens))?;
    let sensor_width = geometry.sensor().width as usize;
    let readback_per_pixel = if resizing {
        RESIZED_READBACK_BYTES_PER_PIXEL
    } else {
        READBACK_BYTES_PER_PIXEL
    };
    let source_rows = |top: u32, rows: u32| -> EngineResult<std::ops::Range<u32>> {
        if resizing {
            let rect = pipeline_gpu::ExportResize {
                source: frame,
                destination,
                top,
                rows,
            }
            .support_rect()?;
            Ok(rect.y..rect.y + rect.height)
        } else {
            Ok(top..top + rows)
        }
    };
    // Developed rows each 16-row block of the output frame reads through the
    // map (evaluated once per block, in parallel): a band's are the union.
    const BLOCK: u32 = 16;
    let blocks: Option<Vec<(u32, u32)>> = job.lens.map.as_ref().map(|map| {
        use rayon::prelude::*;
        (0..frame.height.div_ceil(BLOCK))
            .into_par_iter()
            .map(|b| {
                let rows = b * BLOCK..((b + 1) * BLOCK).min(frame.height);
                map.source_rows(rows, developed.width, developed.height)
            })
            .collect()
    });
    let developed_rows = |rows: std::ops::Range<u32>| -> std::ops::Range<u32> {
        match &blocks {
            Some(blocks) => {
                let span = &blocks[(rows.start / BLOCK) as usize
                    ..(rows.end.div_ceil(BLOCK) as usize).min(blocks.len())];
                let first = span.iter().map(|b| b.0).min().unwrap_or(0);
                let end = span.iter().map(|b| b.1).max().unwrap_or(0);
                first..end.max(first)
            }
            None => rows,
        }
    };
    // Sensor stages are charged per sensor pixel (CFA width, halo rows), the
    // rest per developed and output pixel: a default crop much narrower than
    // the sensor (the CR3 fixture: 6288 wide, developed 4000) costs more than
    // its developed size suggests.
    let cost = |top: u32, rows: u32| -> EngineResult<(BandCost, usize)> {
        let source = source_rows(top, rows)?;
        let developed_rows = developed_rows(source.clone());
        let band = geometry.rows(developed_rows.clone());
        let cost = BandCost {
            sensor: band.sensor.len()
                * sensor_width
                * (SENSOR_BYTES_PER_PIXEL
                    + match &job.lens.ca {
                        Some(ca) if ca.maker.is_some() => MAKER_BYTES_PER_PIXEL,
                        Some(_) => CA_BYTES_PER_PIXEL,
                        None => 0,
                    }),
            developed: band.developed.len() * developed.width as usize * DEVELOPED_BYTES_PER_PIXEL,
            mapped: if blocks.is_some() {
                source.len() * frame.width as usize * MAP_BYTES_PER_PIXEL
            } else {
                0
            },
            readback: rows as usize * destination.width as usize * readback_per_pixel
                + if resizing {
                    band.developed.len() * developed.width as usize * RESIZE_SOURCE_BYTES_PER_PIXEL
                } else {
                    0
                },
            params: PARAMS_BYTES,
        };
        Ok((cost, developed_rows.len() * developed.width as usize))
    };
    let fits = |top: u32, rows: u32| -> EngineResult<bool> {
        let (cost, pixels) = cost(top, rows)?;
        Ok(cost.fits(share) && pixels <= MAX_BAND_PIXELS)
    };
    // Greedy bands: each takes the most output rows (a multiple of 16) that
    // still fits, so bands where the map spreads rows (edges of a distortion
    // correction) are shorter than those in the middle.
    let mut bands = Vec::new();
    let mut top = 0;
    while top < destination.height {
        let left = destination.height - top;
        let (mut lo, mut hi) = (1u32, left.div_ceil(16));
        if !fits(top, 16.min(left))? {
            // Decline at plan time, before any band renders, when even the
            // smallest band exceeds the scratch each band renderer is given
            // (`budget` only sizes bands: tests pass tiny ones).
            let (smallest, _) = cost(top, 16.min(left))?;
            if !smallest.fits(job.scratch / in_flight) {
                trace_note(&format!(
                    "band plan: rows {top}.. need {} B, over the {} B band scratch; pyramid tiles",
                    smallest.total(),
                    job.scratch / in_flight
                ));
                return Ok(None);
            }
            hi = 1;
        }
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            if fits(top, (mid * 16).min(left))? {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        let rows = (lo * 16).min(left);
        bands.push((top, rows));
        top += rows;
    }
    trace(&format!("band plan ({} bands)", bands.len()), started);
    let row_len = destination.width as usize * 3;
    let mut rgb = image::Rgb32FImage::new(destination.width, destination.height);
    let mut slices = Vec::with_capacity(bands.len());
    let mut rest: &mut [f32] = &mut rgb;
    for &(top, rows) in &bands {
        let (band, tail) = rest.split_at_mut(rows as usize * row_len);
        slices.push((top, band));
        rest = tail;
    }
    let queue = Mutex::new(slices.into_iter());
    let unsupported = std::sync::atomic::AtomicBool::new(false);
    // Any worker's failure stops the others at their next band.
    let stop = std::sync::atomic::AtomicBool::new(false);
    let waited = Mutex::new(std::time::Duration::ZERO);
    #[cfg(test)]
    let footprints = Mutex::new(Vec::new());
    let band_worker = || -> EngineResult<()> {
        // A worker's share of the budget covers its band in flight and the
        // idle buffers it retains for its next band, so it is held from the
        // worker's first band to its last (declared first: dropped last).
        let mut reservation = None;
        // Each worker recycles its own bands' buffers.
        let pool = base.export_band(None);
        loop {
            if stop.load(std::sync::atomic::Ordering::Relaxed) {
                return Ok(());
            }
            let Some((top, dst)) = queue.lock().unwrap_or_else(|e| e.into_inner()).next() else {
                return Ok(());
            };
            job.cancel.check()?;
            let rows = (dst.len() / row_len) as u32;
            // Export-priority: never start a band while interactive work waits.
            let yielded = jobs::yield_to_interactive(
                job.cancel,
                pipeline_gpu::EXPORT_QUIET,
                pipeline_gpu::EXPORT_MAX_YIELD,
            )?;
            *waited.lock().unwrap_or_else(|e| e.into_inner()) += yielded;
            let band_started = std::time::Instant::now();
            if reservation.is_none() {
                // With its part of the export's fixed tables.
                reservation = Some(Reservation::acquire(
                    share + job.fixed.div_ceil(in_flight),
                    job.cancel,
                )?);
            }
            let resize = resizing.then_some(pipeline_gpu::ExportResize {
                source: frame,
                destination,
                top,
                rows,
            });
            let source = source_rows(top, rows)?;
            let renderer = pool.export_band_recycling(resize);
            let supported = renderer.render_export_rows(
                job.raw,
                job.settings,
                job.level,
                source.clone(),
                Some(job.lens),
                dst,
                job.cancel,
            )?;
            if !supported {
                unsupported.store(true, std::sync::atomic::Ordering::Relaxed);
                stop.store(true, std::sync::atomic::Ordering::Relaxed);
                return Ok(());
            }
            #[cfg(test)]
            {
                let stats = renderer.stats();
                footprints
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(BandFootprint {
                        top,
                        rows,
                        live: stats.last_resident_live_bytes,
                        peak: stats.last_resident_peak_bytes,
                        readback: rows as u64 * u64::from(destination.width) * 12,
                        counted: stats.last_resident_allocated_bytes,
                        recycled: stats.last_resident_recycled_bytes,
                        share: (job.scratch / in_flight) as u64,
                        effects_maps: stats.effects_maps,
                    });
            }
            if std::env::var_os("TESSERA_EXPORT_TRACE").is_some() {
                let stats = renderer.stats();
                let (planned, _) = cost(top, rows)?;
                let band = geometry.rows(developed_rows(source.clone()));
                // The readback staging copy is allocated after the scratch.
                let staging = rows as u64 * u64::from(destination.width) * 12;
                let mib = |b: u64| b as f64 / (1 << 20) as f64;
                eprintln!(
                    "EXPORT_TRACE band rows={}..{} yielded={:.1} ms render={:.1} ms scratch={:.1} MiB ({:.0} B/px) dispatches={} \
                     actual={:.2} MiB planned={:.2} MiB (sensor {}x{} {:.2}, developed {}x{} {:.2}, mapped {:.2}, readback {:.2}) \
                     peak={:.2} MiB recycled_in={:.2} MiB fresh_buffers={}",
                    source.start,
                    source.end,
                    yielded.as_secs_f64() * 1e3,
                    band_started.elapsed().as_secs_f64() * 1e3,
                    mib(stats.last_resident_allocated_bytes),
                    stats.last_resident_allocated_bytes as f64
                        / (f64::from(frame.width) * f64::from(source.end - source.start)),
                    stats.last_resident_dispatches,
                    mib(stats.last_resident_allocated_bytes + staging),
                    mib(planned.total() as u64),
                    sensor_width,
                    band.sensor.len(),
                    mib(planned.sensor as u64),
                    developed.width,
                    band.developed.len(),
                    mib(planned.developed as u64),
                    mib(planned.mapped as u64),
                    mib(planned.readback as u64),
                    mib(stats.last_resident_peak_bytes),
                    mib(stats.last_resident_recycled_bytes),
                    stats.last_resident_buffers,
                );
            }
        }
    };
    let results: Vec<EngineResult<()>> = std::thread::scope(|scope| {
        let worker = || {
            let result = band_worker();
            if result.is_err() {
                stop.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            result
        };
        let handles: Vec<_> = (0..in_flight).map(|_| scope.spawn(worker)).collect();
        handles
            .into_iter()
            .map(|h| {
                h.join()
                    .unwrap_or_else(|_| Err(EngineError::internal("export band worker panicked")))
            })
            .collect()
    });
    #[cfg(test)]
    {
        let mut bands = footprints.into_inner().unwrap_or_else(|e| e.into_inner());
        bands.sort_by_key(|b| b.top);
        BAND_FOOTPRINTS.with(|f| f.borrow_mut().extend(bands));
    }
    // Cancellation first, then any other failure (e.g. an unsupported band).
    job.cancel.check()?;
    for result in results {
        result?;
    }
    if unsupported.into_inner() {
        return Ok(None);
    }
    trace("GPU bands (incl. yields)", started);
    let waited = waited.into_inner().unwrap_or_else(|e| e.into_inner());
    if !waited.is_zero() {
        trace(
            "  of which yielding before bands",
            std::time::Instant::now() - waited,
        );
    }
    Ok(Some(rgb))
}

/// Rows of `frame` one tile-path band renders within `budget`: whole tile
/// rows, at most the frame.
fn tile_band_rows(
    developed: engine_api::tile::Extent,
    frame: engine_api::tile::Extent,
    mapped: bool,
    level: u8,
    budget: usize,
) -> u32 {
    let scale = 1u32 << level;
    // Scratch per output row: the resident tile graph (~256 B/px), plus the
    // map's assembled input, mapped band and its encoded copy.
    let per_pixel = if mapped { 384 } else { 256 };
    let row_bytes = (developed.width.max(frame.width) as usize)
        .saturating_mul(per_pixel)
        .saturating_mul((scale * scale) as usize);
    let rows = (budget / row_bytes.max(1) / TILE_SIZE as usize).max(1);
    rows.saturating_mul(TILE_SIZE as usize)
        .min(frame.height as usize) as u32
}

/// Submits the uploads queued on the export device when dropped, so wgpu
/// can free their staging copies whether or not the export submitted GPU
/// work (a declined export otherwise leaves them until the next submission).
struct FlushUploads(Arc<GpuContext>);

impl Drop for FlushUploads {
    fn drop(&mut self) {
        self.0.flush_uploads();
    }
}

/// The M2-21b pyramid-tile path (Texture/Clarity/Dehaze in one band, and
/// the fallback when the band renderer declines).
fn render_tiles(job: &Job<'_>, base: &ManagedRenderer) -> EngineResult<Option<image::Rgb32FImage>> {
    let (frame, destination) = (job.frame, job.destination);
    let (width, height) = (destination.width, destination.height);
    let resizing = destination != frame;
    let level = job.level;
    let developed = job.raw.level_extent(level);
    let budget = job.budget;
    let band = tile_band_rows(developed, frame, job.lens.map.is_some(), level, budget);
    let tone = &job.settings.tone;
    // Global Dehaze statistics / local-tone barriers cannot be independently
    // evaluated per band. Preserve correctness via the scalar fallback.
    // (`render_with_options` declines these before building the output.)
    if band < frame.height && has_presence(tone) {
        return Ok(None);
    }
    let output_band = if resizing {
        (u64::from(band) * u64::from(height) / u64::from(frame.height))
            .max(1)
            .min(u64::from(height)) as u32
    } else {
        band
    };
    let started = std::time::Instant::now();
    let mut waited = std::time::Duration::ZERO;
    let mut rgb = image::Rgb32FImage::new(width, height);
    for top in (0..height).step_by(output_band as usize) {
        job.cancel.check()?;
        // Export-priority: never start a band while interactive work waits.
        let yielded = jobs::yield_to_interactive(
            job.cancel,
            pipeline_gpu::EXPORT_QUIET,
            pipeline_gpu::EXPORT_MAX_YIELD,
        )?;
        waited += yielded;
        let band_started = std::time::Instant::now();
        let reservation = Reservation::acquire(budget + job.fixed, job.cancel)?;
        let (renderer, rect) = if resizing {
            let request = pipeline_gpu::ExportResize {
                source: frame,
                destination,
                top,
                rows: output_band.min(height - top),
            };
            (base.export_band(Some(request)), request.source_rect()?)
        } else {
            (
                base.export_band(None),
                PixelRect::new(0, top, frame.width, band.min(frame.height - top)),
            )
        };
        let tiles = match renderer.render_export_lens(
            job.raw,
            job.settings,
            level,
            rect,
            job.lens,
            job.cancel,
        ) {
            Ok(Some(tiles)) => tiles,
            Ok(None) | Err(EngineError::Unsupported { .. }) => return Ok(None),
            Err(e) => return Err(e),
        };
        drop(reservation);
        if std::env::var_os("TESSERA_EXPORT_TRACE").is_some() {
            let stats = renderer.stats();
            eprintln!(
                "EXPORT_TRACE band top={top} yielded={:.1} ms render={:.1} ms scratch={:.1} MiB dispatches={}",
                yielded.as_secs_f64() * 1e3,
                band_started.elapsed().as_secs_f64() * 1e3,
                stats.last_resident_allocated_bytes as f64 / (1 << 20) as f64,
                stats.last_resident_dispatches
            );
        }
        for tile in tiles {
            let layout = tile.layout();
            let data = tile.samples::<f32>()?;
            let (ox, oy) = tile.coord().pixel_origin(TILE_SIZE);
            let oy = if resizing { top + oy } else { oy };
            for y in 0..layout.extent.height {
                for x in 0..layout.extent.width {
                    let i = (y * layout.extent.width + x) as usize;
                    rgb.put_pixel(
                        ox + x,
                        oy + y,
                        image::Rgb(std::array::from_fn(|c| data[c * layout.plane_len() + i])),
                    );
                }
            }
        }
    }
    job.cancel.check()?;
    trace("GPU tiles (incl. yields)", started);
    if !waited.is_zero() {
        trace(
            "  of which yielding before bands",
            std::time::Instant::now() - waited,
        );
    }
    Ok(Some(rgb))
}

#[cfg(test)]
#[path = "../../image-core/tests/common/mod.rs"]
mod common;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ColorSpace, ExportImage};
    use engine_api::{jobs::CancellationToken, recipe::Recipe};
    use pipeline_cpu::RenderSource;

    fn resident_recipe() -> Recipe {
        let mut recipe = Recipe::default();
        recipe
            .edit(
                engine_api::recipe::EditMeta::user("disable unsupported lens stages", 0),
                |s| {
                    s.lens.profile = engine_api::recipe::settings::LensProfileSource::None;
                    s.lens.remove_chromatic_aberration = false;
                },
            )
            .unwrap();
        recipe
    }

    fn compare(cpu: &image::Rgb32FImage, gpu: &image::Rgb32FImage) -> (f32, f32) {
        assert_eq!(cpu.dimensions(), gpu.dimensions());
        let decode = |v: f32| {
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        let linear = cpu
            .as_raw()
            .iter()
            .zip(gpu.as_raw())
            .map(|(a, b)| (decode(*a) - decode(*b)).abs())
            .fold(0.0f32, f32::max);
        let codes = cpu
            .as_raw()
            .iter()
            .zip(gpu.as_raw())
            .map(|(a, b)| {
                ((a.clamp(0., 1.) * 255.).round() - (b.clamp(0., 1.) * 255.).round()).abs()
            })
            .fold(0.0f32, f32::max);
        (linear, codes)
    }

    fn edited(edit: impl FnOnce(&mut engine_api::recipe::DevelopSettings)) -> Recipe {
        let mut recipe = Recipe::default();
        recipe
            .edit(engine_api::recipe::EditMeta::user("lens", 0), edit)
            .unwrap();
        recipe
    }

    /// Lens/geometry recipes stay resident and match the reference within
    /// the full-chain tolerance (docs/11 §1.3), whole and in bands.
    #[test]
    fn lens_and_geometry_recipes_render_on_gpu() {
        use engine_api::recipe::settings::NormalizedRect;
        let raw = common::synthetic(71, 300, 530, common::RGGB, [3, 5, 290, 520]);
        let image = ExportImage {
            source: RenderSource::Cfa {
                image: raw.cfa(),
                metadata: raw.metadata(),
            },
            name: "lens",
            sequence: 1,
            date: "",
            metadata: None,
        };
        let recipes = [
            ("default (Auto + CA)", Recipe::default()),
            (
                "manual distortion",
                edited(|s| s.lens.manual_distortion = 20.),
            ),
            (
                "manual vignetting",
                edited(|s| {
                    s.lens.manual_vignetting = -40.;
                    s.lens.manual_vignetting_midpoint = 30.;
                }),
            ),
            (
                "crop + straighten + distortion",
                edited(|s| {
                    s.lens.manual_distortion = -15.;
                    s.geometry.crop.rect = NormalizedRect {
                        left: 0.1,
                        top: 0.05,
                        right: 0.85,
                        bottom: 0.9,
                    };
                    s.geometry.crop.angle = 3.5;
                }),
            ),
            (
                "transform",
                edited(|s| {
                    s.geometry.transform.vertical = 20.;
                    s.geometry.transform.rotate = 2.;
                    s.geometry.transform.scale = 110.;
                }),
            ),
        ];
        let cancel = CancellationToken::new();
        for (name, recipe) in recipes {
            let cpu = crate::render_scaled_cpu(&image, &recipe, ColorSpace::Srgb, 1).unwrap();
            let whole = render(&image, &recipe, ColorSpace::Srgb, 1, &cancel, usize::MAX)
                .unwrap()
                .unwrap_or_else(|| panic!("{name}: must stay on GPU"));
            assert_eq!(LAST_PATH.get(), "bands", "{name}");
            let bands = render(&image, &recipe, ColorSpace::Srgb, 1, &cancel, 1)
                .unwrap()
                .unwrap();
            assert_eq!(LAST_PATH.get(), "bands", "{name}");
            let (linear, codes) = compare(&cpu, &whole);
            eprintln!("LENS {name}: linear={linear} codes={codes}");
            assert!(linear <= 2e-3 && codes <= 1.0, "{name}: {linear} {codes}");
            let (seam, _) = compare(&whole, &bands);
            assert!(seam < 1e-5, "{name}: band seam {seam}");
        }
        // Defringe is not ported: the reference path renders it.
        let defringe = edited(|s| s.lens.defringe_purple.amount = 5.);
        assert!(
            render(&image, &defringe, ColorSpace::Srgb, 1, &cancel, BUDGET)
                .unwrap()
                .is_none()
        );
    }

    /// Forced calibrations exercise lateral CA (sensor frame, before the
    /// matrices), profile vignetting and Brown-Conrady distortion together.
    #[test]
    fn forced_calibration_matches_reference() {
        let raw = common::synthetic(4242, 420, 300, common::RGGB, [2, 2, 416, 296]);
        let image = ExportImage {
            source: RenderSource::Cfa {
                image: raw.cfa(),
                metadata: raw.metadata(),
            },
            name: "forced",
            sequence: 1,
            date: "",
            metadata: None,
        };
        let sample = lens::CalibrationSample {
            distortion: lens::BrownConrady {
                k1: -0.08,
                k2: 0.01,
                cx: 0.02,
                cy: -0.01,
                ..Default::default()
            },
            ca_red: [1.003, 0.001, 0.],
            ca_blue: [0.997, -0.001, 0.],
            vignette: [-0.35, 0.05, 0.],
            ..Default::default()
        };
        let resolved = pipeline_cpu::ResolvedLens::from_calibration(sample);
        let recipe = Recipe::default();
        let mut registry = color_mgmt::Registry::new();
        let target = crate::codec::profile(&mut registry, ColorSpace::Srgb).unwrap();
        let cpu = pipeline_cpu::render_managed_scaled_resolved(
            &recipe.settings,
            &image.source,
            1,
            &mut pipeline_cpu::OutputContext {
                registry: &mut registry,
                target: pipeline_cpu::OutputTarget::Export(&target),
                proof: None,
                options: color_mgmt::TransformOptions::default(),
            },
            &resolved,
        )
        .unwrap()
        .pixels;
        let cancel = CancellationToken::new();
        for budget in [usize::MAX, 1] {
            let gpu = render_with_lens(
                &image,
                &recipe,
                ColorSpace::Srgb,
                1,
                &cancel,
                budget,
                crate::Resize::None,
                Some(resolved.clone()),
            )
            .unwrap()
            .expect("forced calibration stays on GPU");
            let (linear, codes) = compare(&cpu, &gpu);
            eprintln!("FORCED budget={budget}: linear={linear} codes={codes}");
            assert!(linear <= 2e-3 && codes <= 1.0, "{linear} {codes}");
        }
    }

    /// The five camera fixtures (`test_fixtures::raw::root()`), with their
    /// file names; `None` after a visible SKIPPED (a failure under
    /// `TESSERA_REQUIRE_RAW_FIXTURES`).
    fn five_fixtures() -> Option<Vec<(&'static str, std::path::PathBuf)>> {
        const NAMES: [&str; 5] = [
            "canon-cr3.CR3",
            "sony-arw.ARW",
            "nikon-nef.NEF",
            "fuji-raf.RAF",
            "sample.dng",
        ];
        let paths = test_fixtures::raw::files(&test_fixtures::current_test(), &NAMES)?;
        Some(NAMES.into_iter().zip(paths).collect())
    }

    /// Takes the band footprints of the exports run on this thread since the
    /// last call, prints one summary line, and describes every band whose
    /// true footprint (the larger of its peak, and its scratch at readback
    /// plus the readback staging copy) exceeds the scratch its renderer is
    /// given (`BUDGET / in_flight`).
    fn footprint_overruns(label: &str) -> Vec<String> {
        let bands = BAND_FOOTPRINTS.with(|f| std::mem::take(&mut *f.borrow_mut()));
        assert!(!bands.is_empty(), "{label}: no band footprints recorded");
        let mib = |b: u64| b as f64 / (1 << 20) as f64;
        let used = |b: &BandFootprint| (b.live + b.readback).max(b.peak);
        let max = |f: &dyn Fn(&BandFootprint) -> u64| bands.iter().map(f).max().unwrap_or(0);
        let over: Vec<String> = bands
            .iter()
            .filter(|b| used(b) > b.share)
            .map(|b| {
                format!(
                    "{label} rows {}+{}: live {:.1} + readback {:.1} MiB (peak {:.1}, recycled {:.1}) > {:.1} MiB",
                    b.top,
                    b.rows,
                    mib(b.live),
                    mib(b.readback),
                    mib(b.peak),
                    mib(b.recycled),
                    mib(b.share)
                )
            })
            .collect();
        eprintln!(
            "FOOTPRINT {label} bands={} over_share={} max_live_plus_readback={:.1} MiB max_peak={:.1} MiB max_counted_plus_readback={:.1} MiB share={:.1} MiB",
            bands.len(),
            over.len(),
            mib(max(&|b| b.live + b.readback)),
            mib(max(&|b| b.peak)),
            mib(max(&|b| b.counted + b.readback)),
            mib(max(&|b| b.share)),
        );
        over
    }

    #[test]
    fn five_fixture_full_chain_tolerance() {
        let Some(fixtures) = five_fixtures() else {
            return;
        };
        let mut overruns = Vec::new();
        for (name, path) in fixtures {
            let raw = RawImage::open(ImageId(1), &path).unwrap();
            let image = ExportImage {
                source: RenderSource::Cfa {
                    image: raw.cfa(),
                    metadata: raw.metadata(),
                },
                name,
                sequence: 1,
                date: "",
                metadata: None,
            };
            // Lens off, and the default recipe (Auto lens profile + CA).
            for (label, recipe) in [
                ("lens-off", resident_recipe()),
                ("default", Recipe::default()),
            ] {
                let cpu = crate::render_scaled_cpu(&image, &recipe, ColorSpace::Srgb, 1).unwrap();
                let gpu = render(
                    &image,
                    &recipe,
                    ColorSpace::Srgb,
                    1,
                    &CancellationToken::new(),
                    BUDGET,
                )
                .unwrap()
                .expect("fixture must use GPU");
                assert_eq!(LAST_PATH.get(), "bands", "{name} {label}");
                overruns.extend(footprint_overruns(&format!("full-chain {name} {label}")));
                let (linear, codes) = compare(&cpu, &gpu);
                eprintln!("PRECISION {name} {label} linear_max={linear} codes_max={codes}");
                assert!(
                    linear <= 2e-3 && codes <= 1.0,
                    "{name} {label}: linear={linear}, codes={codes}"
                );
            }
        }
        assert!(overruns.is_empty(), "{overruns:#?}");
    }

    #[test]
    fn raw_batch_cancel_preserves_icc_xmp_without_partial_files() {
        let raw = common::synthetic(811, 300, 270, common::RGGB, [0, 0, 300, 270]);
        let recipe = resident_recipe();
        let items: Vec<_> = (1..=3)
            .map(|sequence| crate::ExportItem {
                image: ExportImage {
                    source: RenderSource::Cfa {
                        image: raw.cfa(),
                        metadata: raw.metadata(),
                    },
                    name: "raw",
                    sequence,
                    date: "",
                    metadata: None,
                },
                recipe: &recipe,
            })
            .collect();
        let dir = tempfile::tempdir().unwrap();
        let token = CancellationToken::new();
        let report = crate::export_batch_with_jobs(
            &items,
            &crate::ExportSettings {
                output_dir: dir.path().into(),
                ..Default::default()
            },
            |p| {
                if p.completed == 1 {
                    token.cancel();
                }
            },
            &token,
            2,
        )
        .unwrap();
        assert_eq!(
            report.results.iter().filter(|r| r.is_ok()).count(),
            1,
            "{report:?}"
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
        // Parallel completion order is not input order: cancellation may
        // leave item zero cancelled while another item was committed first.
        let committed = report
            .results
            .iter()
            .find_map(|result| result.as_ref().ok())
            .unwrap();
        let bytes = std::fs::read(committed).unwrap();
        for marker in [
            b"ICC_PROFILE".as_slice(),
            b"http://ns.adobe.com/xap/1.0/".as_slice(),
        ] {
            assert!(bytes.windows(marker.len()).any(|w| w == marker));
        }
    }

    #[test]
    fn gpu_matches_cpu_and_bands_match_whole() {
        let raw = common::synthetic(991, 300, 530, common::RGGB, [0, 0, 300, 530]);
        let image = ExportImage {
            source: RenderSource::Cfa {
                image: raw.cfa(),
                metadata: raw.metadata(),
            },
            name: "gpu",
            sequence: 1,
            date: "",
            metadata: None,
        };
        let recipe = resident_recipe();
        let cancel = CancellationToken::new();
        for space in [
            ColorSpace::Srgb,
            ColorSpace::DisplayP3,
            ColorSpace::Rec2020,
            ColorSpace::ProPhoto,
        ] {
            let cpu = crate::render_scaled_cpu(&image, &recipe, space, 1).unwrap();
            let whole = render(&image, &recipe, space, 1, &cancel, usize::MAX)
                .unwrap()
                .unwrap();
            let bands = render(&image, &recipe, space, 1, &cancel, 1)
                .unwrap()
                .unwrap();
            let max = cpu
                .as_raw()
                .iter()
                .zip(whole.as_raw())
                .map(|(a, b)| (a - b).abs())
                .fold(0.0f32, f32::max);
            assert!(max <= 2e-3, "managed chain max error {max}");
            let codes = cpu
                .as_raw()
                .iter()
                .zip(whole.as_raw())
                .map(|(a, b)| {
                    ((a.clamp(0., 1.) * 255.).round() - (b.clamp(0., 1.) * 255.).round()).abs()
                })
                .fold(0.0f32, f32::max);
            assert!(codes <= 1.0, "8-bit codes {codes}");
            let seam = whole
                .as_raw()
                .iter()
                .zip(bands.as_raw())
                .map(|(a, b)| (a - b).abs())
                .fold(0.0f32, f32::max);
            assert!(seam < 1e-5, "band seam {seam}");
            let mode = crate::Resize::LongEdge(213);
            let reference = crate::filter::resize(cpu, mode, &cancel).unwrap();
            // Full-resolution development, resized on the GPU: the gate.
            let resized = render_opts(&image, &recipe, space, usize::MAX, mode, false);
            let banded = render_opts(&image, &recipe, space, 1, mode, false);
            assert_eq!(reference.dimensions(), resized.dimensions());
            let error = reference
                .as_raw()
                .iter()
                .zip(resized.as_raw())
                .map(|(a, b)| (a - b).abs())
                .fold(0.0f32, f32::max);
            assert!(error < 2e-3, "resized error {error}");
            let seam = banded
                .as_raw()
                .iter()
                .zip(resized.as_raw())
                .map(|(a, b)| (a - b).abs())
                .fold(0.0f32, f32::max);
            assert!(seam < 1e-5, "resized band seam {seam}");
            // The Web-scale path develops at the covering pyramid level: its
            // bands agree with each other; its distance from the reference
            // is a documented approximation (see five_fixture_web_scale).
            let web = render_opts(&image, &recipe, space, usize::MAX, mode, true);
            let web_bands = render_opts(&image, &recipe, space, 1, mode, true);
            assert_eq!(web.dimensions(), reference.dimensions());
            let seam = web
                .as_raw()
                .iter()
                .zip(web_bands.as_raw())
                .map(|(a, b)| (a - b).abs())
                .fold(0.0f32, f32::max);
            assert!(seam < 1e-5, "web-scale band seam {seam}");
        }
    }

    /// The row-band renderer reproduces the pyramid-tile renderer (same
    /// operators, different dispatch granularity) for Bayer and X-Trans,
    /// a cropped active area, colour-reconstructing highlights, position-
    /// dependent effects (vignette, grain) and a lens map, whole or in
    /// 16-row bands, at levels 0 and 1, resized or not.
    #[test]
    fn band_renderer_matches_tile_renderer() {
        let with = |edit: fn(&mut engine_api::recipe::DevelopSettings)| {
            let mut recipe = resident_recipe();
            recipe
                .edit(engine_api::recipe::EditMeta::user("bands", 0), edit)
                .unwrap();
            recipe
        };
        let recipes = [
            ("neutral", resident_recipe()),
            (
                "effects",
                with(|s| {
                    s.effects.vignette.amount = -30.;
                    s.effects.grain.amount = 40.;
                    s.tone.exposure = 0.7;
                    s.linearize.highlight_reconstruction =
                        engine_api::recipe::settings::HighlightReconstruction::ReconstructColor;
                }),
            ),
            ("map", with(|s| s.lens.manual_distortion = 25.)),
        ];
        let sources = [
            (
                "bayer",
                common::synthetic(501, 610, 452, common::RGGB, [5, 3, 598, 441]),
            ),
            (
                "xtrans",
                common::synthetic(502, 612, 450, common::xtrans(), [6, 0, 600, 444]),
            ),
        ];
        let cancel = CancellationToken::new();
        for (source, raw) in &sources {
            let image = ExportImage {
                source: RenderSource::Cfa {
                    image: raw.cfa(),
                    metadata: raw.metadata(),
                },
                name: "bands",
                sequence: 1,
                date: "",
                metadata: None,
            };
            for (name, recipe) in &recipes {
                for scale in [1, 2] {
                    for resize in [crate::Resize::None, crate::Resize::LongEdge(190)] {
                        let run = |bands: bool, budget: usize| {
                            let rgb = render_with_options(
                                &image,
                                recipe,
                                ColorSpace::Srgb,
                                scale,
                                &cancel,
                                budget,
                                resize,
                                None,
                                Options {
                                    bands,
                                    web_level: false,
                                    in_flight: BANDS_IN_FLIGHT,
                                    effects_map: false,
                                },
                            )
                            .unwrap()
                            .expect("stays on GPU");
                            assert_eq!(
                                LAST_PATH.get(),
                                if bands { "bands" } else { "tiles" },
                                "{source} {name}"
                            );
                            rgb
                        };
                        let tiles = run(false, usize::MAX);
                        for budget in [usize::MAX, 1] {
                            let bands = run(true, budget);
                            assert_eq!(tiles.dimensions(), bands.dimensions());
                            let error = tiles
                                .as_raw()
                                .iter()
                                .zip(bands.as_raw())
                                .map(|(a, b)| (a - b).abs())
                                .fold(0.0f32, f32::max);
                            assert!(
                                error < 1e-5,
                                "{source} {name} scale={scale} {resize:?} budget={budget}: {error}"
                            );
                        }
                    }
                }
            }
        }
    }

    fn render_opts(
        image: &ExportImage<'_>,
        recipe: &Recipe,
        space: ColorSpace,
        budget: usize,
        resize: crate::Resize,
        web_level: bool,
    ) -> image::Rgb32FImage {
        let rgb = render_with_options(
            image,
            recipe,
            space,
            1,
            &CancellationToken::new(),
            budget,
            resize,
            None,
            Options {
                bands: true,
                web_level,
                in_flight: BANDS_IN_FLIGHT,
                effects_map: false,
            },
        )
        .unwrap()
        .expect("stays on GPU");
        assert_eq!(LAST_PATH.get(), "bands");
        rgb
    }

    /// Error statistics against a reference: (max linear, max 8-bit codes,
    /// share of samples more than one code apart, 99.9th percentile codes).
    fn error_stats(cpu: &image::Rgb32FImage, gpu: &image::Rgb32FImage) -> (f32, f32, f64, f32) {
        let (linear, codes) = compare(cpu, gpu);
        let mut diffs: Vec<f32> = cpu
            .as_raw()
            .iter()
            .zip(gpu.as_raw())
            .map(|(a, b)| {
                ((a.clamp(0., 1.) * 255.).round() - (b.clamp(0., 1.) * 255.).round()).abs()
            })
            .collect();
        let over = diffs.iter().filter(|d| **d > 1.0).count() as f64 / diffs.len() as f64;
        let k = ((diffs.len() as f64 * 0.999) as usize).min(diffs.len() - 1);
        let (_, p999, _) = diffs.select_nth_unstable_by(k, f32::total_cmp);
        (linear, codes, over, *p999)
    }

    /// docs/11 §1.3 gate at Web scale (Resize::LongEdge(2048)): the GPU
    /// export against the CPU reference developed at full resolution and
    /// resized by the CPU exporter. Development at full resolution must meet
    /// the full-chain tolerance; the pyramid-level (Web-scale) development is
    /// measured and reported.
    #[test]
    fn five_fixture_web_scale_tolerance() {
        let Some(fixtures) = five_fixtures() else {
            return;
        };
        let mode = crate::Resize::LongEdge(2048);
        let cancel = CancellationToken::new();
        let mut failures = Vec::new();
        let mut overruns = Vec::new();
        for (name, path) in fixtures {
            let raw = RawImage::open(ImageId(1), &path).unwrap();
            let image = ExportImage {
                source: RenderSource::Cfa {
                    image: raw.cfa(),
                    metadata: raw.metadata(),
                },
                name,
                sequence: 1,
                date: "",
                metadata: None,
            };
            let recipe = Recipe::default();
            let cpu = crate::render_scaled_cpu(&image, &recipe, ColorSpace::Srgb, 1).unwrap();
            let reference = crate::filter::resize(cpu, mode, &cancel).unwrap();
            for web_level in [false, true] {
                let gpu = render_opts(&image, &recipe, ColorSpace::Srgb, BUDGET, mode, web_level);
                overruns.extend(footprint_overruns(&format!(
                    "web {name} {}",
                    if web_level {
                        "pyramid-level"
                    } else {
                        "full-res"
                    }
                )));
                assert_eq!(gpu.dimensions(), reference.dimensions());
                let (linear, codes, over, p999) = error_stats(&reference, &gpu);
                eprintln!(
                    "WEBGATE {name} {} linear_max={linear} codes_max={codes} over_1_code={:.4}% p99.9_codes={p999}",
                    if web_level {
                        "pyramid-level"
                    } else {
                        "full-res"
                    },
                    over * 100.
                );
                if !web_level && !(linear <= 2e-3 && codes <= 1.0) {
                    failures.push(format!("{name}: linear={linear} codes={codes}"));
                }
            }
        }
        assert!(failures.is_empty(), "{failures:?}");
        assert!(overruns.is_empty(), "{overruns:#?}");
    }

    /// REV3-ENG-8 NS3: the RAF with a wide zoom's built-in correction (-4 %,
    /// -6 % barrels: 72 and 104 px displacement) exports on the GPU band path
    /// within each band's scratch share, full chain and web sizes, and
    /// matches the CPU render like the fixtures do.
    #[test]
    fn raf_wide_zoom_barrels_export_on_bands_within_budget() {
        let Some(path) = test_fixtures::raw::with_extension(&test_fixtures::current_test(), "raf")
        else {
            return;
        };
        let raw = RawImage::open(ImageId(1), &path).unwrap();
        let cancel = CancellationToken::new();
        let mut failures = Vec::new();
        let mut overruns = Vec::new();
        for corner in [-4., -6.] {
            let mut m = raw.metadata().clone();
            m.maker_lens = Some(raw_decode::MakerLens::Fujifilm(raw_decode::FujifilmLens {
                knots: (0..=10).map(|i| i as f64 / 10.).collect(),
                distortion: (0..=10)
                    .map(|i| corner * (i as f64 / 10.).powi(2))
                    .collect(),
                ca_red: (0..=10).map(|i| 3e-4 * i as f64 / 10.).collect(),
                ca_blue: (0..=10).map(|i| -3e-4 * i as f64 / 10.).collect(),
                vignetting: (0..=10)
                    .map(|i| 100. - 20. * (i as f64 / 10.).powi(2))
                    .collect(),
                crop_factor: 1.,
            }));
            let image = ExportImage {
                source: RenderSource::Cfa {
                    image: raw.cfa(),
                    metadata: &m,
                },
                name: "raf-barrel",
                sequence: 1,
                date: "",
                metadata: None,
            };
            let recipe = Recipe::default();
            let cpu = crate::render_scaled_cpu(&image, &recipe, ColorSpace::Srgb, 1).unwrap();
            let full = render_opts(
                &image,
                &recipe,
                ColorSpace::Srgb,
                BUDGET,
                crate::Resize::None,
                false,
            );
            overruns.extend(footprint_overruns(&format!("{corner} % full")));
            let (linear, codes, _, _) = error_stats(&cpu, &full);
            eprintln!("BARREL {corner} % full-res linear_max={linear} codes_max={codes}");
            if !(linear <= 2e-3 && codes <= 1.0) {
                failures.push(format!("{corner} %: linear={linear} codes={codes}"));
            }
            let mode = crate::Resize::LongEdge(2048);
            for web_level in [false, true] {
                render_opts(&image, &recipe, ColorSpace::Srgb, BUDGET, mode, web_level);
                overruns.extend(footprint_overruns(&format!("{corner} % web {web_level}")));
            }
            let _ = &cancel;
        }
        assert!(failures.is_empty(), "{failures:?}");
        assert!(overruns.is_empty(), "{overruns:#?}");
    }

    /// A recipe with each effect the device-peak diagnostic measures.
    fn effects_variants(base: &Recipe) -> Vec<(&'static str, Recipe)> {
        let with = |edit: fn(&mut engine_api::recipe::DevelopSettings)| {
            let mut recipe = base.clone();
            recipe
                .edit(engine_api::recipe::EditMeta::user("effects", 0), edit)
                .unwrap();
            recipe
        };
        vec![
            ("none", base.clone()),
            ("vignette", with(|s| s.effects.vignette.amount = -40.)),
            ("grain", with(|s| s.effects.grain.amount = 40.)),
        ]
    }

    /// Runs `export` while another thread samples the Metal device's own
    /// allocation counter every 0.3 ms: (peak minus the idle baseline taken
    /// first, export time).
    fn device_peak(context: &GpuContext, export: impl FnOnce()) -> (u64, std::time::Duration) {
        use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
        let read = || {
            context
                .device_allocated_bytes()
                .expect("Metal allocation counter")
        };
        let baseline = context
            .idle_device_allocated_bytes()
            .expect("Metal allocation counter");
        let (done, peak) = (AtomicBool::new(false), AtomicU64::new(baseline));
        let elapsed = std::thread::scope(|scope| {
            scope.spawn(|| {
                while !done.load(Ordering::Acquire) {
                    peak.fetch_max(read(), Ordering::Relaxed);
                    std::thread::sleep(std::time::Duration::from_micros(300));
                }
                peak.fetch_max(read(), Ordering::Relaxed);
            });
            let started = std::time::Instant::now();
            export();
            let elapsed = started.elapsed();
            done.store(true, Ordering::Release);
            elapsed
        });
        (peak.into_inner().saturating_sub(baseline), elapsed)
    }

    /// ENG-14 diagnostic: true export device memory, measured as the
    /// reviewer of ENG-13 did. The Metal device's own allocation counter
    /// (`currentAllocatedSize`, which sees every buffer, pipeline and wgpu
    /// staging copy whether or not a budget counts it) is sampled during a
    /// real export, after a warm-up export of the same configuration. Five
    /// fixtures x {full chain, Web full-res, Web pyramid} x {no effects,
    /// vignette, grain}: every peak must be within [`BUDGET`]. Ignored
    /// because the counter is process-wide; run it alone:
    /// `cargo test --release -p export --lib five_fixture_device_peak -- --ignored --test-threads=1`
    #[test]
    #[ignore = "process-wide device counter: run alone"]
    fn five_fixture_device_peak_within_budget() {
        let Some(fixtures) = five_fixtures() else {
            return;
        };
        let context = match DEVICE.get_or_init(|| GpuContext::new().map(Arc::new)) {
            Ok(context) => context.clone(),
            Err(e) => panic!("Metal device required: {e}"),
        };
        let cancel = CancellationToken::new();
        let mib = |b: u64| b as f64 / (1 << 20) as f64;
        let mut over = Vec::new();
        for (name, path) in fixtures {
            let raw = RawImage::open(ImageId(1), &path).unwrap();
            let image = ExportImage {
                source: RenderSource::Cfa {
                    image: raw.cfa(),
                    metadata: raw.metadata(),
                },
                name,
                sequence: 1,
                date: "",
                metadata: None,
            };
            for (scale, resize, web_level) in [
                ("full-chain", crate::Resize::None, false),
                ("web-full-res", crate::Resize::LongEdge(2048), false),
                ("web-pyramid", crate::Resize::LongEdge(2048), true),
            ] {
                for (effect, recipe) in effects_variants(&Recipe::default()) {
                    let export = || {
                        render_with_options(
                            &image,
                            &recipe,
                            ColorSpace::Srgb,
                            1,
                            &cancel,
                            BUDGET,
                            resize,
                            None,
                            Options {
                                bands: true,
                                web_level,
                                in_flight: BANDS_IN_FLIGHT,
                                effects_map: false,
                            },
                        )
                        .unwrap()
                        .expect("fixture must use GPU");
                        assert_eq!(LAST_PATH.get(), "bands", "{name} {scale} {effect}");
                    };
                    export();
                    let (peak, elapsed) = device_peak(&context, export);
                    BAND_FOOTPRINTS.with(|f| f.borrow_mut().clear());
                    eprintln!(
                        "DEVICE_PEAK {name} {scale} {effect} peak={:.1} MiB export={:.1} ms",
                        mib(peak),
                        elapsed.as_secs_f64() * 1e3
                    );
                    if peak > BUDGET as u64 {
                        over.push(format!(
                            "{name} {scale} {effect}: {:.1} MiB > {:.1} MiB",
                            mib(peak),
                            mib(BUDGET as u64)
                        ));
                    }
                }
            }
        }
        assert!(over.is_empty(), "{over:#?}");
    }

    /// Exports `recipe` with production settings and with the effects map
    /// forced (test-only switch): describes any difference, any effects map
    /// the production export built, and a forced export that built none.
    #[allow(clippy::too_many_arguments)]
    fn effects_map_difference(
        label: &str,
        image: &ExportImage<'_>,
        recipe: &Recipe,
        scale: u32,
        resize: crate::Resize,
        web_level: bool,
        bands: bool,
        budget: usize,
    ) -> Vec<String> {
        let run = |effects_map: bool| {
            let rgb = render_with_options(
                image,
                recipe,
                ColorSpace::Srgb,
                scale,
                &CancellationToken::new(),
                budget,
                resize,
                None,
                Options {
                    bands,
                    web_level,
                    in_flight: BANDS_IN_FLIGHT,
                    effects_map,
                },
            )
            .unwrap()
            .expect("stays on GPU");
            assert_eq!(
                LAST_PATH.get(),
                if bands { "bands" } else { "tiles" },
                "{label}"
            );
            let maps: u64 = BAND_FOOTPRINTS
                .with(|f| std::mem::take(&mut *f.borrow_mut()))
                .iter()
                .map(|b| b.effects_maps)
                .sum();
            (rgb, maps)
        };
        let (inline, inline_maps) = run(false);
        let (mapped, mapped_maps) = run(true);
        let mut problems = Vec::new();
        if inline_maps != 0 {
            problems.push(format!("{label}: export built {inline_maps} effects maps"));
        }
        if bands && mapped_maps == 0 {
            problems.push(format!("{label}: the forced map was never built"));
        }
        let differing = inline
            .as_raw()
            .iter()
            .zip(mapped.as_raw())
            .filter(|(a, b)| a.to_bits() != b.to_bits())
            .count();
        eprintln!(
            "EFFECTS_MAP {label} maps_inline={inline_maps} maps_forced={mapped_maps} differing_samples={differing}"
        );
        if differing != 0 {
            problems.push(format!(
                "{label}: {differing} samples differ with the effects map"
            ));
        }
        problems
    }

    /// ENG-14: export never builds the effects constants map; the inline
    /// vignette/grain path is bit-identical to the map, on synthetic Bayer
    /// and X-Trans sources, bands (whole and 16-row) and tiles, levels 0
    /// and 1, resized or not.
    #[test]
    fn exports_use_the_inline_effects_path_bit_identical_to_the_map() {
        let sources = [
            (
                "bayer",
                common::synthetic(1411, 610, 452, common::RGGB, [5, 3, 598, 441]),
            ),
            (
                "xtrans",
                common::synthetic(1412, 612, 450, common::xtrans(), [6, 0, 600, 444]),
            ),
        ];
        let mut problems = Vec::new();
        for (source, raw) in &sources {
            let image = ExportImage {
                source: RenderSource::Cfa {
                    image: raw.cfa(),
                    metadata: raw.metadata(),
                },
                name: "effects",
                sequence: 1,
                date: "",
                metadata: None,
            };
            for (effect, recipe) in effects_variants(&resident_recipe()).into_iter().skip(1) {
                for scale in [1, 2] {
                    for resize in [crate::Resize::None, crate::Resize::LongEdge(190)] {
                        for (path, bands, budget) in [
                            ("bands", true, usize::MAX),
                            ("16-row bands", true, 1),
                            ("tiles", false, usize::MAX),
                        ] {
                            problems.extend(effects_map_difference(
                                &format!("{source} {effect} scale={scale} {resize:?} {path}"),
                                &image,
                                &recipe,
                                scale,
                                resize,
                                false,
                                bands,
                                budget,
                            ));
                        }
                    }
                }
            }
        }
        assert!(problems.is_empty(), "{problems:#?}");
    }

    /// ENG-14 on the five fixtures: full chain, Web full-res and Web pyramid,
    /// with vignette and with grain.
    #[test]
    fn five_fixture_exports_use_the_inline_effects_path_bit_identical_to_the_map() {
        let Some(fixtures) = five_fixtures() else {
            return;
        };
        let mut problems = Vec::new();
        for (name, path) in fixtures {
            let raw = RawImage::open(ImageId(1), &path).unwrap();
            let image = ExportImage {
                source: RenderSource::Cfa {
                    image: raw.cfa(),
                    metadata: raw.metadata(),
                },
                name,
                sequence: 1,
                date: "",
                metadata: None,
            };
            for (effect, recipe) in effects_variants(&Recipe::default()).into_iter().skip(1) {
                for (scale, resize, web_level) in [
                    ("full-chain", crate::Resize::None, false),
                    ("web-full-res", crate::Resize::LongEdge(2048), false),
                    ("web-pyramid", crate::Resize::LongEdge(2048), true),
                ] {
                    problems.extend(effects_map_difference(
                        &format!("{name} {scale} {effect}"),
                        &image,
                        &recipe,
                        1,
                        resize,
                        web_level,
                        true,
                        BUDGET,
                    ));
                }
            }
        }
        assert!(problems.is_empty(), "{problems:#?}");
    }
}
