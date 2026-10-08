//! ENG-8b (REV-ENG-8 B1/B2): Native AI-mask exports of raws whose lens
//! correction warps the image (the Fujifilm RAF's built-in maker-note
//! correction applies in every lens mode) render through the full reference
//! pipeline with the masks before the warp, as Develop does, instead of
//! failing; and the masks land on the content they were segmented from.
use engine_api::{
    EngineResult,
    id::ImageId,
    recipe::{
        EditMeta, LocalAdjustment, Recipe,
        mask::{LocalParams, MaskComponent, MaskKind},
        settings::LensProfileSource,
    },
};
use export::{
    ColorSpace, ExportImage, ExportSettings, Format, Metadata, export_one_with_segmenter,
    mask_ai::{MaskSegmenter, SegmentRequest},
};
use image_core::{PixelRect, RawImage, RenderOutput, Renderer, RendererConfig};
use pipeline_cpu::{Image, RenderSource};
use std::sync::{Arc, Mutex};
use test_fixtures::raw as raw_fixtures;

/// A constant raster, so export and Develop use identical masks.
struct Constant(f32);
impl MaskSegmenter for Constant {
    fn segment(&mut self, image: &image::RgbImage, _: &SegmentRequest) -> anyhow::Result<Vec<f32>> {
        Ok(vec![self.0; (image.width() * image.height()) as usize])
    }
}
/// The same constant raster as Develop's mask cache hooks.
struct ConstantHooks(f32);
impl image_core::mask_cache::MaskHooks for ConstantHooks {
    fn revision(&self) -> u64 {
        1
    }
    fn rasterize(&self, input: &Image, _: &LocalAdjustment, _: u8) -> EngineResult<Vec<f32>> {
        Ok(vec![self.0; input.planes()[0].len()])
    }
}

fn recipe(profile: Option<LensProfileSource>, exposure: f32) -> Recipe {
    let mut recipe = Recipe::default();
    recipe
        .edit(EditMeta::user("mask", 0), |s| {
            if let Some(profile) = profile {
                s.lens.profile = profile;
            }
            s.locals.adjustments.push(LocalAdjustment {
                components: vec![MaskComponent::new(MaskKind::Subject { model: None })],
                params: LocalParams {
                    exposure,
                    ..Default::default()
                },
                ..Default::default()
            });
        })
        .unwrap();
    recipe
}

fn export_image(image: &RawImage) -> ExportImage<'_> {
    ExportImage {
        source: RenderSource::Cfa {
            image: image.cfa(),
            metadata: image.metadata(),
        },
        name: "eng8b",
        sequence: 1,
        date: "",
        metadata: None,
    }
}

/// 16-bit sRGB TIFF export, in 8-bit levels; and its size.
fn export(
    image: &RawImage,
    recipe: &Recipe,
    segmenter: &mut dyn MaskSegmenter,
) -> (Vec<[f32; 3]>, u32, u32) {
    let dir = tempfile::tempdir().unwrap();
    let path = export_one_with_segmenter(
        &export_image(image),
        recipe,
        &ExportSettings {
            format: Format::Tiff { bits: 16 },
            color_space: ColorSpace::Srgb,
            output_dir: dir.path().to_path_buf(),
            metadata: Metadata::None,
            ..Default::default()
        },
        segmenter,
    )
    .unwrap_or_else(|e| panic!("export failed: {e}"));
    let mut decoder = tiff::decoder::Decoder::new(std::fs::File::open(path).unwrap()).unwrap();
    let (w, h) = decoder.dimensions().unwrap();
    let samples = match decoder.read_image().unwrap() {
        tiff::decoder::DecodingResult::U16(v) => v,
        other => panic!("expected 16-bit samples, got {other:?}"),
    };
    let pixels = samples
        .as_chunks::<3>()
        .0
        .iter()
        .map(|p| std::array::from_fn(|c| f32::from(p[c]) / 65535. * 255.))
        .collect();
    (pixels, w, h)
}

/// Develop's renderer with the same mask raster, at full resolution.
fn develop(
    image: &RawImage,
    recipe: &Recipe,
    hooks: Arc<dyn image_core::mask_cache::MaskHooks>,
) -> (Vec<[f32; 3]>, u32, u32) {
    let renderer = Renderer::new(RendererConfig {
        process_version: recipe.process_version,
        ..Default::default()
    });
    renderer.mask_cache().set_hooks(Some(hooks));
    let extent = Renderer::output_extent(image, &recipe.settings, 0).unwrap();
    let tiles = renderer
        .render_region_as(
            image,
            &recipe.settings,
            0,
            PixelRect::full(extent),
            RenderOutput::Display,
        )
        .unwrap();
    let mut out = vec![[0f32; 3]; extent.area() as usize];
    for t in &tiles {
        let l = t.layout();
        let n = l.plane_len();
        let (ox, oy) = t.coord().pixel_origin(engine_api::tile::TILE_SIZE);
        let values = t.samples::<u8>().unwrap();
        for y in 0..l.extent.height {
            for x in 0..l.extent.width {
                let i = (y * l.extent.width + x) as usize;
                let o = ((oy + y) * extent.width + ox + x) as usize;
                for c in 0..3 {
                    out[o][c] = f32::from(values[c * n + i]);
                }
            }
        }
    }
    (out, extent.width, extent.height)
}

/// (max, mean) |a - b| in 8-bit levels.
fn parity(a: &[[f32; 3]], b: &[[f32; 3]]) -> (f32, f32) {
    assert_eq!(a.len(), b.len(), "frame sizes differ");
    let (mut max, mut sum) = (0f32, 0f64);
    for (a, b) in a.iter().zip(b) {
        for c in 0..3 {
            let d = (a[c] - b[c]).abs();
            max = max.max(d);
            sum += f64::from(d);
        }
    }
    (max, (sum / (3 * a.len()) as f64) as f32)
}

#[test]
fn ai_mask_exports_succeed_on_every_fixture_and_lens_mode_and_match_develop() {
    const TEST: &str = "ai_mask_exports_succeed_on_every_fixture_and_lens_mode_and_match_develop";
    let mut failures = Vec::new();
    for (i, path) in raw_fixtures::all(TEST).iter().enumerate() {
        let name = raw_fixtures::name(path);
        let image = RawImage::open(ImageId(8800 + i as u128), path).unwrap();
        for (mode, profile) in [
            ("Auto", Some(LensProfileSource::Auto)),
            ("None", Some(LensProfileSource::None)),
            ("recipe default", None),
        ] {
            let recipe = recipe(profile, 1.);
            let started = std::time::Instant::now();
            let (exported, w, h) = export(&image, &recipe, &mut Constant(0.5));
            let seconds = started.elapsed().as_secs_f64();
            let (developed, dw, dh) = develop(&image, &recipe, Arc::new(ConstantHooks(0.5)));
            assert_eq!((w, h), (dw, dh), "{name} {mode}: frame");
            let (max, mean) = parity(&developed, &exported);
            raw_fixtures::notice(
                TEST,
                &format!(
                    "{name} {mode}: max {max:.3} mean {mean:.4} levels, export {seconds:.1} s"
                ),
            );
            if max > NATIVE_MAX + NATIVE_GREY_POINT || mean > MEAN {
                failures.push(format!("{name} {mode}: max {max} mean {mean}"));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// The ENG-9 export/Develop parity bounds (`eng9_develop_parity.rs`): 1.0
/// level max for Native plus its documented 0.1-level grey-point exception
/// for saturated Perceptual pixels, 0.35 level mean. The NEF reaches 1.062
/// on the unchanged non-warp path, as on main.
const NATIVE_MAX: f32 = 1.0;
const NATIVE_GREY_POINT: f32 = 0.1;
const MEAN: f32 = 0.35;

/// Luma box-blurred with radius `r`.
fn smooth_luma(rgb: &[[f32; 3]], w: usize, h: usize, r: usize) -> Vec<f32> {
    let luma: Vec<f32> = rgb
        .iter()
        .map(|p| 0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2])
        .collect();
    let pass = |src: &[f32], horizontal: bool| -> Vec<f32> {
        let mut out = vec![0.; src.len()];
        for y in 0..h {
            for x in 0..w {
                let mut sum = 0.;
                for d in 0..=2 * r {
                    let (sx, sy) = if horizontal {
                        ((x + d).saturating_sub(r).min(w - 1), y)
                    } else {
                        (x, (y + d).saturating_sub(r).min(h - 1))
                    };
                    sum += src[sy * w + sx];
                }
                out[y * w + x] = sum / (2 * r + 1) as f32;
            }
        }
        out
    };
    pass(&pass(&luma, true), false)
}

/// A content-derived raster: smoothed luma above its median. Records the
/// median's rank so the target can be found in the export.
struct Threshold(Arc<Mutex<Vec<f32>>>);
impl MaskSegmenter for Threshold {
    fn segment(&mut self, image: &image::RgbImage, _: &SegmentRequest) -> anyhow::Result<Vec<f32>> {
        let (w, h) = (image.width() as usize, image.height() as usize);
        let rgb: Vec<[f32; 3]> = image.pixels().map(|p| p.0.map(f32::from)).collect();
        let luma = smooth_luma(&rgb, w, h, 4);
        let mut sorted = luma.clone();
        sorted.sort_by(f32::total_cmp);
        let t = sorted[sorted.len() / 2];
        let alpha: Vec<f32> = luma.iter().map(|&v| f32::from(u8::from(v > t))).collect();
        *self.0.lock().unwrap() = alpha.clone();
        Ok(alpha)
    }
}

fn iou(a: &[bool], b: &[bool], w: usize, h: usize) -> f64 {
    let (mx, my) = (w * 3 / 100, h * 3 / 100);
    let (mut inter, mut union) = (0u64, 0u64);
    for y in my..h - my {
        for x in mx..w - mx {
            let (p, q) = (a[y * w + x], b[y * w + x]);
            inter += u64::from(p && q);
            union += u64::from(p || q);
        }
    }
    inter as f64 / union.max(1) as f64
}

/// REV-ENG-8 B2 in export: the region a -3 EV AI mask darkens in the RAF
/// export is the region the segmenter selected, found in the export by the
/// same rule (smoothed luma above its median), as on the same raw without the
/// correction.
#[test]
fn raf_ai_mask_export_lands_on_its_content() {
    const TEST: &str = "raf_ai_mask_export_lands_on_its_content";
    let Some(path) = raw_fixtures::with_extension(TEST, "raf") else {
        return;
    };
    let image = RawImage::open(ImageId(8810), &path).unwrap();
    let mut stripped = image.metadata().clone();
    stripped.maker_lens = None;
    let plain = image
        .with_metadata(ImageId(8811), Arc::new(stripped))
        .unwrap();
    let mut results = Vec::new();
    for raw in [&image, &plain] {
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let (base, w, h) = export(raw, &recipe(None, 0.), &mut Threshold(recorded.clone()));
        let (masked, ..) = export(raw, &recipe(None, -3.), &mut Threshold(recorded.clone()));
        let (w, h) = (w as usize, h as usize);
        let luma = |v: &[[f32; 3]]| -> Vec<f32> {
            v.iter()
                .map(|p| 0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2])
                .collect()
        };
        let (lb, lm) = (luma(&base), luma(&masked));
        let darkened: Vec<bool> = lm.iter().zip(&lb).map(|(m, b)| *m < 0.5 * b).collect();
        // The segmenter smoothed over 4 px of its input, drawn at 1/scale
        // of the export (`ready_hooks`: long edge at most 2048).
        let scale = w.max(h).div_ceil(2048);
        let smooth = smooth_luma(&base, w, h, 4 * scale);
        let mut sorted = smooth.clone();
        sorted.sort_by(f32::total_cmp);
        let t = sorted[sorted.len() / 2];
        let target: Vec<bool> = smooth.iter().map(|&v| v > t).collect();
        results.push(iou(&darkened, &target, w, h));
    }
    let [corrected, uncorrected] = results[..] else {
        unreachable!()
    };
    raw_fixtures::notice(
        TEST,
        &format!("IoU with the built-in correction {corrected:.4}, without {uncorrected:.4}"),
    );
    // Measured 0.9873 with the correction, 0.9888 without: the raster is
    // segmented at 1/3 scale and resampled, so neither reaches 1. Misaligned
    // by the warp (Develop before ENG-8b, the pipeline-cpu A/B) costs ~0.03.
    assert!(
        corrected >= 0.98 && corrected >= uncorrected - 0.005,
        "{results:?}"
    );
}

/// REV-ENG-8 B1: print/documents (`render_pixels_with_notes`) and DNG export
/// share the AI-mask renderer; on the RAF they succeed and apply the mask.
#[test]
fn raf_ai_mask_print_and_dng_export_succeed() {
    const TEST: &str = "raf_ai_mask_print_and_dng_export_succeed";
    let Some(path) = raw_fixtures::with_extension(TEST, "raf") else {
        return;
    };
    let image = RawImage::open(ImageId(8812), &path).unwrap();
    let input = export_image(&image);
    let request = export::RenderRequest {
        color_space: ColorSpace::Srgb,
        resize: export::Resize::None,
        sharpen_for: export::SharpenFor::None,
        scale: 1,
    };
    let print = |exposure: f32| {
        export::render_pixels_with_notes(
            &input,
            &recipe(None, exposure),
            &request,
            &engine_api::jobs::CancellationToken::new(),
            Some(&mut Constant(1.)),
            None,
            None,
        )
        .unwrap_or_else(|e| panic!("print failed: {e}"))
        .0
    };
    let (bright, plain) = (print(1.), print(0.));
    let mean = |i: &image::Rgb32FImage| i.pixels().map(|p| f64::from(p.0[1])).sum::<f64>();
    assert!(mean(&bright) > mean(&plain) * 1.2, "print applies the mask");
    let dir = tempfile::tempdir().unwrap();
    let dng = export_one_with_segmenter(
        &input,
        &recipe(None, 1.),
        &ExportSettings {
            format: Format::Dng,
            output_dir: dir.path().to_path_buf(),
            metadata: Metadata::None,
            ..Default::default()
        },
        &mut Constant(1.),
    )
    .unwrap_or_else(|e| panic!("DNG export failed: {e}"));
    assert!(std::fs::metadata(dng).unwrap().len() > 0);
}
