//! Resident RAW export and bounded horizontal-band admission.

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
use std::sync::{Arc, Mutex, OnceLock};

pub(crate) const BUDGET: usize = 512 << 20;
// One GPU render at a time, across all export callers. Encoders do not hold
// this lock. A batch cannot multiply its resident allocation by CPU cores.
static DEVICE: OnceLock<EngineResult<Arc<GpuContext>>> = OnceLock::new();
static ADMISSION: Mutex<()> = Mutex::new(());

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
    // Auto can estimate a correction from image content even without embedded
    // opcodes. No resident lens stage exists, so never treat defaults as inert.
    if !image_core::resident_export_lens_supported(&recipe.settings.lens)
        || recipe.process_version != engine_api::recipe::ProcessVersion::NATIVE_CURRENT
        || !recipe.settings.locals.adjustments.is_empty()
        || recipe.settings.geometry != Default::default()
        || pipeline_cpu::denoise_active(&recipe.settings.denoise)
    {
        return Ok(None);
    }
    let _guard = ADMISSION
        .lock()
        .map_err(|_| EngineError::internal("export GPU lock poisoned"))?;
    cancel.check()?;
    let context = match DEVICE.get_or_init(|| GpuContext::new().map(Arc::new)) {
        Ok(context) => context.clone(),
        Err(_) => return Ok(None),
    };
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
    let frame = raw.level_extent(level);
    let (width, height) = resize.dimensions(frame.width, frame.height)?;
    // Resident resize is a downsampling path. Enlargements (up to the public
    // 100 MP limit) retain the bounded, row-parallel CPU resampler rather than
    // allocating an expanded GPU band that could exceed a device buffer limit.
    if width > frame.width || height > frame.height {
        return Ok(None);
    }
    let destination = engine_api::tile::Extent::new(width, height);
    let resizing = destination != frame;
    let row_bytes = (frame.width as usize)
        .saturating_mul(256)
        .saturating_mul((scale * scale) as usize);
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
    let mut settings = recipe.settings.clone();
    settings.output.proof_profile = None;
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
    let mut rgb = image::Rgb32FImage::new(width, height);
    for top in (0..height).step_by(output_band as usize) {
        cancel.check()?;
        let (renderer, rect) = if resizing {
            let request = pipeline_gpu::ExportResize {
                source: frame,
                destination,
                top,
                rows: output_band.min(height - top),
            };
            (
                ManagedRenderer::new_export_resized(output.clone(), config.clone(), request),
                request.source_rect()?,
            )
        } else {
            (
                ManagedRenderer::new_export(output.clone(), config.clone()),
                PixelRect::new(0, top, frame.width, band.min(frame.height - top)),
            )
        };
        let tiles = match renderer.render_export(&raw, &settings, level, rect, cancel) {
            Ok(Some(tiles)) => tiles,
            Ok(None) | Err(EngineError::Unsupported { .. }) => return Ok(None),
            Err(e) => return Err(e),
        };
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

    #[test]
    fn lens_corrections_require_reference_fallback() {
        let raw = common::synthetic(71, 32, 32, common::RGGB, [0, 0, 32, 32]);
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
        let mut manual = resident_recipe();
        manual.settings.lens.manual_distortion = 20.;
        for recipe in [Recipe::default(), manual] {
            assert!(
                render(
                    &image,
                    &recipe,
                    ColorSpace::Srgb,
                    1,
                    &CancellationToken::new(),
                    BUDGET
                )
                .unwrap()
                .is_none()
            );
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
            let recipe = resident_recipe();
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
            eprintln!("PRECISION {name} linear_max={linear} codes_max={codes}");
            assert!(
                linear <= 2e-3 && codes <= 1.0,
                "{name}: linear={linear}, codes={codes}"
            );
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
