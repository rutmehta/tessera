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

use crate::{ColorSpace, ExportImage, codec};
use engine_api::{
    EngineError, EngineResult,
    id::ImageId,
    jobs::CancellationToken,
    recipe::Recipe,
    tile::{Pyramid, TILE_SIZE, TileCoord},
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
    let mut samples = vec![0.; extent.area() as usize];
    for y in 0..extent.height.div_ceil(TILE_SIZE) {
        for x in 0..extent.width.div_ceil(TILE_SIZE) {
            cancel.check()?;
            let tile = pyramid.tile(TileCoord::new(0, x, y))?;
            let l = tile.layout();
            let data = tile.samples::<f32>()?;
            for row in 0..l.extent.height {
                let from = (row * l.extent.width) as usize;
                let to = ((y * TILE_SIZE + row) * extent.width + x * TILE_SIZE) as usize;
                samples[to..to + l.extent.width as usize]
                    .copy_from_slice(&data[from..from + l.extent.width as usize]);
            }
        }
    }
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
    let level = scale.trailing_zeros() as u8;
    let developed = raw.level_extent(level);
    let frame = image_core::Renderer::lens_output_extent(&raw, level, Some(&lens));
    let (width, height) = resize.dimensions(frame.width, frame.height)?;
    // Resident resize is a downsampling path. Enlargements (up to the public
    // 100 MP limit) retain the bounded, row-parallel CPU resampler rather than
    // allocating an expanded GPU band that could exceed a device buffer limit.
    if width > frame.width || height > frame.height {
        return Ok(None);
    }
    let destination = engine_api::tile::Extent::new(width, height);
    let resizing = destination != frame;
    // Scratch per output row: the resident tile graph (~256 B/px), plus the
    // map's assembled input, mapped band and its encoded copy.
    let per_pixel = if lens.map.is_some() { 384 } else { 256 };
    let row_bytes = (developed.width.max(frame.width) as usize)
        .saturating_mul(per_pixel)
        .saturating_mul((scale * scale) as usize);
    let budget = budget.min(BUDGET);
    let rows = (budget / row_bytes.max(1) / TILE_SIZE as usize).max(1);
    let band = rows
        .saturating_mul(TILE_SIZE as usize)
        .min(frame.height as usize) as u32;
    let tone = &recipe.settings.tone;
    // Global Dehaze statistics / local-tone barriers cannot be independently
    // evaluated per band. Preserve correctness via the scalar fallback.
    if band < frame.height && (tone.texture != 0. || tone.clarity != 0. || tone.dehaze != 0.) {
        return Ok(None);
    }
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
    let output_band = if resizing {
        (u64::from(band) * u64::from(height) / u64::from(frame.height))
            .max(1)
            .min(u64::from(height)) as u32
    } else {
        band
    };
    let started = std::time::Instant::now();
    let mut waited = std::time::Duration::ZERO;
    // Pipelines compile once per export, not once per band.
    let base = ManagedRenderer::new_export_budgeted(output, config, None, BUDGET as u64);
    let mut rgb = image::Rgb32FImage::new(width, height);
    for top in (0..height).step_by(output_band as usize) {
        cancel.check()?;
        // Export-priority: never start a band while interactive work waits.
        let yielded = jobs::yield_to_interactive(
            cancel,
            pipeline_gpu::EXPORT_QUIET,
            pipeline_gpu::EXPORT_MAX_YIELD,
        )?;
        waited += yielded;
        let band_started = std::time::Instant::now();
        let reservation = Reservation::acquire(budget, cancel)?;
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
        let tiles = match renderer.render_export_lens(&raw, &settings, level, rect, &lens, cancel) {
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
    cancel.check()?;
    trace("GPU bands (incl. yields)", started);
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
            let bands = render(&image, &recipe, ColorSpace::Srgb, 1, &cancel, 1)
                .unwrap()
                .unwrap();
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
        let bytes = std::fs::read(report.results[0].as_ref().unwrap()).unwrap();
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
            let resized = render_resized(&image, &recipe, space, 1, &cancel, usize::MAX, mode)
                .unwrap()
                .unwrap();
            let banded = render_resized(&image, &recipe, space, 1, &cancel, 1, mode)
                .unwrap()
                .unwrap();
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
        }
    }
}
