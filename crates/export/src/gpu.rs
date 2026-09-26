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

/// Band renderer scratch per developed pixel of a band at level L: the
/// level-L stages plus the full-resolution sensor stages (raw, highlights,
/// demosaic, lateral CA) of its 4^L sensor pixels, and the readback. Fresh
/// allocations measured 135–140 B/px at level 0 and about 245 B/px at level 1
/// on the five fixtures (`TESSERA_EXPORT_TRACE=1` prints them per band).
fn band_bytes_per_pixel(level: u8) -> usize {
    150 + 40 * ((1usize << (2 * level)) - 1)
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
    let budget = budget.min(BUDGET);
    let job = Job {
        raw: &raw,
        settings: &settings,
        lens: &lens,
        level,
        frame,
        destination,
        budget,
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
            (BUDGET / in_flight) as u64,
        );
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
    let base = ManagedRenderer::new_export_budgeted(output, config, None, BUDGET as u64);
    LAST_PATH.set("tiles");
    render_tiles(&job, &base)
}

thread_local! {
    /// The renderer the last GPU export on this thread used ("bands" or
    /// "tiles"): tests assert which path produced their pixels.
    pub(crate) static LAST_PATH: std::cell::Cell<&'static str> = const { std::cell::Cell::new("") };
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
    budget: usize,
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
    let per_pixel = band_bytes_per_pixel(job.level);
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
    let developed_rows = |rows: std::ops::Range<u32>| -> usize {
        match &blocks {
            Some(blocks) => {
                let span = &blocks[(rows.start / BLOCK) as usize
                    ..(rows.end.div_ceil(BLOCK) as usize).min(blocks.len())];
                let first = span.iter().map(|b| b.0).min().unwrap_or(0);
                let end = span.iter().map(|b| b.1).max().unwrap_or(0);
                end.saturating_sub(first) as usize
            }
            None => rows.len(),
        }
    };
    let fits = |top: u32, rows: u32| -> EngineResult<bool> {
        let source = source_rows(top, rows)?;
        let developed_rows = developed_rows(source.clone());
        let mapped = if blocks.is_some() {
            source.len() * frame.width as usize * MAP_BYTES_PER_PIXEL
        } else {
            0
        };
        Ok(
            developed_rows * developed.width as usize * per_pixel + mapped <= share
                && developed_rows * developed.width as usize <= MAX_BAND_PIXELS,
        )
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
                reservation = Some(Reservation::acquire(share, job.cancel)?);
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
            if std::env::var_os("TESSERA_EXPORT_TRACE").is_some() {
                let stats = renderer.stats();
                eprintln!(
                    "EXPORT_TRACE band rows={}..{} yielded={:.1} ms render={:.1} ms scratch={:.1} MiB ({:.0} B/px) dispatches={}",
                    source.start,
                    source.end,
                    yielded.as_secs_f64() * 1e3,
                    band_started.elapsed().as_secs_f64() * 1e3,
                    stats.last_resident_allocated_bytes as f64 / (1 << 20) as f64,
                    stats.last_resident_allocated_bytes as f64
                        / (f64::from(frame.width) * f64::from(source.end - source.start)),
                    stats.last_resident_dispatches
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

/// The M2-21b pyramid-tile path (Texture/Clarity/Dehaze in one band, and
/// the fallback when the band renderer declines).
fn render_tiles(job: &Job<'_>, base: &ManagedRenderer) -> EngineResult<Option<image::Rgb32FImage>> {
    let (frame, destination) = (job.frame, job.destination);
    let (width, height) = (destination.width, destination.height);
    let resizing = destination != frame;
    let level = job.level;
    let developed = job.raw.level_extent(level);
    let scale = 1u32 << level;
    // Scratch per output row: the resident tile graph (~256 B/px), plus the
    // map's assembled input, mapped band and its encoded copy.
    let per_pixel = if job.lens.map.is_some() { 384 } else { 256 };
    let row_bytes = (developed.width.max(frame.width) as usize)
        .saturating_mul(per_pixel)
        .saturating_mul((scale * scale) as usize);
    let budget = job.budget;
    let rows = (budget / row_bytes.max(1) / TILE_SIZE as usize).max(1);
    let band = rows
        .saturating_mul(TILE_SIZE as usize)
        .min(frame.height as usize) as u32;
    let tone = &job.settings.tone;
    // Global Dehaze statistics / local-tone barriers cannot be independently
    // evaluated per band. Preserve correctness via the scalar fallback.
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
        let reservation = Reservation::acquire(budget, job.cancel)?;
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

    #[test]
    #[ignore = "full-chain precision on all five real RAW fixtures"]
    fn five_fixture_full_chain_tolerance() {
        let root = std::path::PathBuf::from(
            std::env::var_os("PIPELINE_RAW_FIXTURES").expect("fixture directory required"),
        );
        for name in [
            "canon-cr3.CR3",
            "sony-arw.ARW",
            "nikon-nef.NEF",
            "fuji-raf.RAF",
            "sample.dng",
        ] {
            let raw = RawImage::open(ImageId(1), root.join(name)).unwrap();
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
                let (linear, codes) = compare(&cpu, &gpu);
                eprintln!("PRECISION {name} {label} linear_max={linear} codes_max={codes}");
                assert!(
                    linear <= 2e-3 && codes <= 1.0,
                    "{name} {label}: linear={linear}, codes={codes}"
                );
            }
        }
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
    #[ignore = "Web-scale precision on all five real RAW fixtures"]
    fn five_fixture_web_scale_tolerance() {
        let root = std::path::PathBuf::from(
            std::env::var_os("PIPELINE_RAW_FIXTURES").expect("fixture directory required"),
        );
        let mode = crate::Resize::LongEdge(2048);
        let cancel = CancellationToken::new();
        let mut failures = Vec::new();
        for name in [
            "canon-cr3.CR3",
            "sony-arw.ARW",
            "nikon-nef.NEF",
            "fuji-raf.RAF",
            "sample.dng",
        ] {
            let raw = RawImage::open(ImageId(1), root.join(name)).unwrap();
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
    }
}
