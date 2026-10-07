//! ENG-10: an Adobe-process export or print at render scale `s` (2, 4, 8)
//! is Develop's rendering at the matching pyramid level `log2(s)`.
//!
//! Develop draws a level by box-averaging the white-balanced frame first and
//! running Detail, Tone, Colour, locals, Effects and Geometry on level pixels
//! (`image-core` render.rs, "M2 controls"; ENG-6 for the Detail-before-
//! Geometry order). A scaled export is held to that rendering, with the
//! same quantisation-only bounds as ENG-9's full-resolution parity
//! (`eng9_develop_parity.rs`, which is unchanged):
//! - 0.5 level for Develop's 8-bit rounding (the Adobe Output stage has no
//!   dither);
//! - 0.002 for a 16-bit TIFF;
//! - 0.03 for float order (ICC transform against the Output-stage matrix,
//!   the 2^-18 Perceptual chroma search).
//!
//! So max <= 0.53 level and mean <= 0.35, in sRGB-encoded 8-bit levels.
use engine_api::{
    id::ImageId,
    jobs::CancellationToken,
    recipe::{EditMeta, ProcessVersion, Recipe, settings::GamutMapping},
};
use export::{ColorSpace, ExportImage, ExportSettings, Format, RenderRequest, Resize, SharpenFor};
use image_core::{PixelRect, RawImage, RenderOutput, Renderer, RendererConfig};
use pipeline_cpu::{CameraLinearProxy, RenderSource};
use std::sync::Arc;

const ADOBE_MAX: f32 = 0.53;
const MEAN: f32 = 0.35;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Raw,
    Rgb,
    Proxy,
}

struct Fixture {
    image: RawImage,
    rgb: Option<pipeline_cpu::Image>,
    proxy: Option<CameraLinearProxy>,
}

impl Fixture {
    fn source(&self) -> RenderSource<'_> {
        if let Some(rgb) = &self.rgb {
            RenderSource::Rgb(rgb)
        } else if let Some(proxy) = &self.proxy {
            RenderSource::CameraLinear(proxy)
        } else {
            RenderSource::Cfa {
                image: self.image.cfa(),
                metadata: self.image.metadata(),
            }
        }
    }
}

fn raw_metadata(w: u32, h: u32) -> raw_decode::RawMetadata {
    raw_decode::RawMetadata {
        make: "synthetic".into(),
        model: "camera".into(),
        lens: None,
        iso: 100.,
        shutter_s: 0.01,
        aperture: 4.,
        focal_mm: 50.,
        capture_time: 0,
        catalog_orientation: None,
        baseline_exposure: 0.,
        orientation: 1,
        width: w,
        height: h,
        cfa_layout: raw_decode::CfaLayout::Bayer([[0, 1], [1, 2]]),
        black_levels: [0.; 4],
        white_level: 65535,
        as_shot_wb: [2., 1., 1.5, 1.],
        camera_to_xyz: engine_api::color::ColorMatrix3::IDENTITY,
        cam_xyz: [[0.9, 0.2, -0.1], [-0.3, 1.2, 0.1], [0.0, 0.1, 0.8], [0.; 3]],
        rgb_cam: [[0.; 4]; 3],
        default_crop: [0, 0, w, h],
        has_gain_map: false,
        has_opcode_list: false,
        opcode_lists: [None, None, None],
    }
}

/// A saturated Bayer original with hard-edged blocks, so that a tone curve
/// before or after the box average gives different pixels.
fn raw() -> Fixture {
    let (w, h) = (128u32, 96u32);
    let cfa = raw_decode::CfaImage::from_linear(
        w,
        h,
        (0..w * h)
            .map(|i| {
                let (x, y) = (i % w, i / w);
                let block = ((x / 6 + y / 5) % 4) as f32;
                match (x % 2, y % 2) {
                    (0, 0) => 0.03 + 0.5 * (x as f32 / w as f32) + 0.1 * block,
                    (1, 1) => 0.02 + 0.4 * (y as f32 / h as f32),
                    _ => 0.04 + 0.12 * block,
                }
            })
            .collect(),
    )
    .unwrap();
    Fixture {
        image: RawImage::new(ImageId(10_000), Arc::new(cfa), Arc::new(raw_metadata(w, h))).unwrap(),
        rgb: None,
        proxy: None,
    }
}

/// Linear Rec.2020 with hard-edged colour blocks well outside sRGB.
fn rgb() -> Fixture {
    let (w, h) = (120u32, 88u32);
    let mut planes = vec![Vec::new(), Vec::new(), Vec::new()];
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
            let v = match (x / 5 + y / 7) % 4 {
                0 => [0.04 + 0.7 * fx, 0.02, 0.03 + 0.2 * fy],
                1 => [0.03, 0.05 + 0.6 * fy, 0.02 + 0.1 * fx],
                2 => [0.02 + 0.1 * fy, 0.03, 0.05 + 0.5 * fx],
                _ => [0.1 + 0.4 * fx, 0.1 + 0.4 * fx, 0.1 + 0.4 * fy],
            };
            for (plane, v) in planes.iter_mut().zip(v) {
                plane.push(v);
            }
        }
    }
    let pixels = pipeline_cpu::Image::new(w, h, planes).unwrap();
    Fixture {
        image: RawImage::from_rgb(
            ImageId(10_001),
            image_core::RgbSource::from_linear_rec2020(pixels.clone()).unwrap(),
        )
        .unwrap(),
        rgb: Some(pixels),
        proxy: None,
    }
}

/// The synthetic external Smart Preview fixture (orientation 1).
fn proxy() -> Fixture {
    let dng = raw_decode::lossy_dng::read(&mut std::io::Cursor::new(include_bytes!(
        "../../raw-decode/tests/fixtures/linear-gradient-jxl.dng"
    )))
    .unwrap()
    .unwrap();
    let proxy = CameraLinearProxy::from_dng(dng)
        .unwrap()
        .with_catalog_orientation(1)
        .unwrap();
    Fixture {
        image: RawImage::from_camera_linear_proxy(
            ImageId(10_002),
            ImageId(10_003),
            Arc::new(proxy.clone()),
        )
        .unwrap(),
        rgb: None,
        proxy: Some(proxy),
    }
}

fn fixture(kind: Kind) -> Fixture {
    match kind {
        Kind::Raw => raw(),
        Kind::Rgb => rgb(),
        Kind::Proxy => proxy(),
    }
}

/// Non-neutral tone and colour plus capture sharpening (Detail), so the
/// level order (downsample, then Detail and Tone on level pixels) matters.
fn recipe(mapping: GamutMapping) -> Recipe {
    let mut recipe = Recipe {
        process_version: ProcessVersion::adobe(6),
        ..Default::default()
    };
    recipe
        .edit(EditMeta::user("eng10", 1), |s| {
            s.output.gamut_mapping = mapping;
            s.color.saturation = 40.;
            s.tone.contrast = 35.;
            s.tone.highlights = -30.;
            s.tone.shadows = 25.;
            s.detail.sharpening.amount = 60.;
        })
        .unwrap();
    recipe
}

type Retouch = Option<Arc<dyn pipeline_cpu::RetouchRenderer>>;

/// Develop's SDR Output stage at `level`, assembled, in 8-bit levels.
fn develop(image: &RawImage, recipe: &Recipe, level: u8, retouch: Retouch) -> Frame {
    let mut renderer = Renderer::new(RendererConfig {
        process_version: recipe.process_version,
        ..Default::default()
    });
    if let Some(retouch) = retouch {
        renderer = renderer.with_retouch_renderer(retouch);
    }
    let extent = Renderer::output_extent(image, &recipe.settings, level).unwrap();
    let tiles = renderer
        .render_region_as(
            image,
            &recipe.settings,
            level,
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
    Frame {
        width: extent.width,
        height: extent.height,
        pixels: out,
    }
}

struct Frame {
    width: u32,
    height: u32,
    pixels: Vec<[f32; 3]>,
}

fn image(source: RenderSource<'_>) -> ExportImage<'_> {
    ExportImage {
        source,
        name: "eng10",
        sequence: 1,
        date: "",
        metadata: None,
    }
}

/// Print floats (sRGB document) at `scale`, as 8-bit levels.
fn print(fixture: &Fixture, recipe: &Recipe, scale: u32, retouch: Retouch) -> Frame {
    let (rgb, _) = export::render_pixels_with_notes(
        &image(fixture.source()),
        recipe,
        &RenderRequest {
            color_space: ColorSpace::Srgb,
            resize: Resize::None,
            sharpen_for: SharpenFor::None,
            scale,
        },
        &CancellationToken::new(),
        None,
        None,
        retouch,
    )
    .unwrap();
    Frame {
        width: rgb.width(),
        height: rgb.height(),
        pixels: rgb
            .pixels()
            .map(|p| p.0.map(|v| v.clamp(0., 1.) * 255.))
            .collect(),
    }
}

/// A 16-bit sRGB TIFF export at `render_scale`, decoded to 8-bit levels.
fn export_file(fixture: &Fixture, recipe: &Recipe, scale: u32, retouch: Retouch) -> Frame {
    let dir = tempfile::tempdir().unwrap();
    let path = export::export_one(
        &image(fixture.source()),
        recipe,
        &ExportSettings {
            format: Format::Tiff { bits: 16 },
            color_space: ColorSpace::Srgb,
            output_dir: dir.path().to_path_buf(),
            render_scale: scale,
            retouch,
            ..Default::default()
        },
    )
    .unwrap();
    let mut decoder = tiff::decoder::Decoder::new(std::fs::File::open(path).unwrap()).unwrap();
    let (width, height) = decoder.dimensions().unwrap();
    let samples = match decoder.read_image().unwrap() {
        tiff::decoder::DecodingResult::U16(v) => v,
        other => panic!("expected 16-bit samples, got {other:?}"),
    };
    Frame {
        width,
        height,
        pixels: samples
            .as_chunks::<3>()
            .0
            .iter()
            .map(|p| std::array::from_fn(|c| f32::from(p[c]) / 65535. * 255.))
            .collect(),
    }
}

/// Records one row; returns failures instead of panicking so the whole
/// table is reported.
fn check(label: &str, develop: &Frame, output: &Frame, failures: &mut Vec<String>) {
    if (develop.width, develop.height) != (output.width, output.height) {
        eprintln!(
            "ENG10 {label:<46} size {}x{} vs Develop {}x{} FAIL",
            output.width, output.height, develop.width, develop.height
        );
        failures.push(format!(
            "{label}: size {}x{}, Develop level is {}x{}",
            output.width, output.height, develop.width, develop.height
        ));
        return;
    }
    let (mut max, mut sum) = (0f32, 0f64);
    for (a, b) in develop.pixels.iter().zip(&output.pixels) {
        for c in 0..3 {
            let d = (a[c] - b[c]).abs();
            max = max.max(d);
            sum += f64::from(d);
        }
    }
    let mean = (sum / (3 * develop.pixels.len()) as f64) as f32;
    let ok = max <= ADOBE_MAX && mean <= MEAN;
    eprintln!(
        "ENG10 {label:<46} max {max:>7.3} mean {mean:>6.3} {}",
        if ok { "ok" } else { "FAIL" }
    );
    if !ok {
        failures.push(format!(
            "{label}: max {max} (limit {ADOBE_MAX}) mean {mean} (limit {MEAN})"
        ));
    }
}

fn run(kind: Kind) {
    let fixture = fixture(kind);
    let mut failures = Vec::new();
    for mapping in [GamutMapping::Perceptual, GamutMapping::Clip] {
        let r = recipe(mapping);
        for scale in [2u32, 4, 8] {
            let level = scale.trailing_zeros() as u8;
            let shown = develop(&fixture.image, &r, level, None);
            check(
                &format!("{kind:?} {mapping:?} scale {scale} export"),
                &shown,
                &export_file(&fixture, &r, scale, None),
                &mut failures,
            );
            check(
                &format!("{kind:?} {mapping:?} scale {scale} print"),
                &shown,
                &print(&fixture, &r, scale, None),
                &mut failures,
            );
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn eng10_raw_scaled_export_and_print_match_develop_level() {
    run(Kind::Raw);
}

#[test]
fn eng10_rgb_scaled_export_and_print_match_develop_level() {
    run(Kind::Rgb);
}

#[test]
fn eng10_smart_preview_scaled_export_and_print_match_develop_level() {
    run(Kind::Proxy);
}

/// Local adjustments, a crop and a retouch spot take Develop's level path:
/// a gradient mask with exposure and a crop on every source, plus a clone
/// spot on the RAW original (the test renderer clones a centred box from a
/// horizontal source offset, so a frame mismatch would move pixels).
#[test]
fn eng10_scaled_locals_crop_and_retouch_match_develop_level() {
    use engine_api::recipe::{
        LocalAdjustment, LocalParams, MaskComponent, MaskKind, RetouchOperation,
        mask::{BrushStroke, RetouchKind, RetouchTarget},
    };
    let clone_box = |w: u32, h: u32, planes: &mut [Vec<f32>], ops: &[RetouchOperation]| {
        for op in ops.iter().filter(|op| op.enabled) {
            let RetouchKind::Clone { source_offset } = op.kind else {
                continue;
            };
            let dx = (source_offset[0] * w as f32).round() as i64;
            for plane in planes.iter_mut() {
                let src = plane.clone();
                for y in h / 4..3 * h / 4 {
                    for x in w / 4..3 * w / 4 {
                        let sx = (i64::from(x) + dx).clamp(0, i64::from(w) - 1) as usize;
                        plane[(y * w + x) as usize] = src[y as usize * w as usize + sx];
                    }
                }
            }
        }
        Ok(())
    };
    let retouch: Arc<dyn pipeline_cpu::RetouchRenderer> = Arc::new(clone_box);
    let mut failures = Vec::new();
    for kind in [Kind::Raw, Kind::Rgb, Kind::Proxy] {
        let fixture = fixture(kind);
        let mut r = recipe(GamutMapping::Perceptual);
        r.edit(EditMeta::user("eng10", 2), |s| {
            s.locals.adjustments.push(LocalAdjustment {
                components: vec![MaskComponent::new(MaskKind::Linear {
                    start: [0., 0.],
                    end: [1., 0.],
                })],
                params: LocalParams {
                    exposure: 0.8,
                    ..Default::default()
                },
                ..Default::default()
            });
            s.geometry.crop.rect.left = 0.125;
            s.geometry.crop.rect.right = 0.875;
            s.geometry.crop.rect.top = 0.25;
            if kind == Kind::Raw {
                s.locals.retouch.push(RetouchOperation {
                    id: engine_api::id::RetouchId(1),
                    kind: RetouchKind::Clone {
                        source_offset: [0.25, 0.],
                    },
                    target: RetouchTarget::Area {
                        components: vec![MaskComponent::new(MaskKind::Brush {
                            strokes: vec![BrushStroke {
                                points: vec![[0.5, 0.5, 1.]],
                                radius: 0.2,
                                feather: 0.,
                                ..Default::default()
                            }],
                        })],
                    },
                    opacity: 100.,
                    feather: 0.,
                    enabled: true,
                });
            }
        })
        .unwrap();
        let retouch = (kind == Kind::Raw).then(|| retouch.clone());
        for scale in [2u32, 4] {
            let level = scale.trailing_zeros() as u8;
            let shown = develop(&fixture.image, &r, level, retouch.clone());
            check(
                &format!("{kind:?} locals+crop scale {scale} export"),
                &shown,
                &export_file(&fixture, &r, scale, retouch.clone()),
                &mut failures,
            );
            check(
                &format!("{kind:?} locals+crop scale {scale} print"),
                &shown,
                &print(&fixture, &r, scale, retouch.clone()),
                &mut failures,
            );
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
