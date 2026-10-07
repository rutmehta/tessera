//! ENG-9: file export and print render exactly what Develop shows, for every
//! process (Adobe and Native) and every source (raw originals, RGB originals,
//! external Smart Preview proxies), apart from deliberate output transforms
//! (output colour space, output sharpening, resizing), all disabled here.
//!
//! "Develop" is the image-core `Renderer` the Develop session draws with:
//! the SDR Output stage (`RenderOutput::Display`, 8-bit sRGB). Exports are
//! 16-bit sRGB TIFF files and print is `render_pixels_with_notes` floats.
//! Both are compared with Develop in sRGB-encoded 8-bit levels.
//!
//! Tolerances are quantisation only:
//! - Develop's 8-bit rounding: at most 0.5 level from the exact value.
//! - Native's Output stage adds a 4x4 ordered dither of at most 15.5/16 - 0.5
//!   = 0.47 level (`pipeline_cpu::display`); the Adobe Output stage has none.
//! - A 16-bit TIFF adds at most 0.5/65535 of full scale (0.002 level).
//! - Float arithmetic order (ICC transform against the Output-stage matrix;
//!   binary-searched against analytic Perceptual chroma, 2^-18 of chroma)
//!   is allowed 0.03 level.
//!
//! So max <= 0.53 level (Adobe) and max <= 1.0 level (Native); the mean
//! of uniformly distributed rounding error is 0.25 level, bounded at 0.35.
//!
//! One documented exception, Native only: Native's Output stage (CPU and
//! Metal, `pipeline_cpu::display`) takes its Perceptual grey point from the
//! 4-decimal sRGB luminance coefficients after conversion, the export from
//! the 4-decimal Rec.2020 ones before it. The two Y differ by coefficient
//! rounding only; on these fixtures that moves saturated Perceptual pixels
//! by at most 0.093 level beyond the quantisation bound (0.023 without
//! local adjustments), identical on main. `NATIVE_GREY_POINT` (0.1) covers
//! saturated Perceptual Native rows only. Aligning Native's Output stage
//! moves Native Develop pixels on every backend and is a separate lane
//! (ENG-9 HANDOFF); the Adobe Output stage uses the export's grey point.
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
const NATIVE_MAX: f32 = 1.0;
const MEAN: f32 = 0.35;
const NATIVE_GREY_POINT: f32 = 0.1;

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

/// A saturated synthetic Bayer original (strong red, varying blue).
fn raw() -> Fixture {
    let (w, h) = (48u32, 32u32);
    let cfa = raw_decode::CfaImage::from_linear(
        w,
        h,
        (0..w * h)
            .map(|i| {
                let (x, y) = (i % w, i / w);
                match (x % 2, y % 2) {
                    (0, 0) => 0.08 + 0.6 * (x as f32 / w as f32),
                    (1, 1) => 0.02 + 0.5 * (y as f32 / h as f32),
                    _ => 0.05 + 0.1 * ((x / 8 + y / 8) % 3) as f32,
                }
            })
            .collect(),
    )
    .unwrap();
    Fixture {
        image: RawImage::new(ImageId(9900), Arc::new(cfa), Arc::new(raw_metadata(w, h))).unwrap(),
        rgb: None,
        proxy: None,
    }
}

/// Linear Rec.2020 with colours well outside sRGB (Rec.2020 green/red).
fn rgb() -> Fixture {
    let (w, h) = (40u32, 28u32);
    let mut planes = vec![Vec::new(), Vec::new(), Vec::new()];
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
            let v = match (x / 10 + y / 7) % 4 {
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
            ImageId(9901),
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
            ImageId(9902),
            ImageId(9903),
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

fn recipe(process: ProcessVersion, mapping: GamutMapping, saturation: f32) -> Recipe {
    let mut recipe = Recipe {
        process_version: process,
        ..Default::default()
    };
    recipe
        .edit(EditMeta::user("eng9", 1), |s| {
            s.output.gamut_mapping = mapping;
            s.color.saturation = saturation;
            s.tone.contrast = 15.;
            s.detail.sharpening.amount = 0.;
        })
        .unwrap();
    recipe
}

/// Develop's SDR Output stage, assembled, in 8-bit levels.
fn develop(image: &RawImage, recipe: &Recipe) -> Vec<[f32; 3]> {
    develop_with(image, recipe, None)
}

/// Caller-owned retouch, as the Develop session installs it.
type Retouch = Option<Arc<dyn pipeline_cpu::RetouchRenderer>>;

fn develop_with(image: &RawImage, recipe: &Recipe, retouch: Retouch) -> Vec<[f32; 3]> {
    let mut renderer = Renderer::new(RendererConfig {
        process_version: recipe.process_version,
        ..Default::default()
    });
    if let Some(retouch) = retouch {
        renderer = renderer.with_retouch_renderer(retouch);
    }
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
    out
}

fn image(source: RenderSource<'_>) -> ExportImage<'_> {
    ExportImage {
        source,
        name: "eng9",
        sequence: 1,
        date: "",
        metadata: None,
    }
}

/// Print pixels converted to sRGB-encoded 8-bit levels.
fn print(fixture: &Fixture, recipe: &Recipe, space: ColorSpace) -> Vec<[f32; 3]> {
    print_with(fixture, recipe, space, None)
}

fn print_with(
    fixture: &Fixture,
    recipe: &Recipe,
    space: ColorSpace,
    retouch: Retouch,
) -> Vec<[f32; 3]> {
    let (rgb, _) = export::render_pixels_with_notes(
        &image(fixture.source()),
        recipe,
        &RenderRequest {
            color_space: space,
            resize: Resize::None,
            sharpen_for: SharpenFor::None,
            scale: 1,
        },
        &CancellationToken::new(),
        None,
        None,
        retouch,
    )
    .unwrap();
    rgb.pixels().map(|p| to_srgb_levels(p.0, space)).collect()
}

/// `space`-encoded floats -> sRGB-encoded 8-bit levels (unclipped in linear).
fn to_srgb_levels(v: [f32; 3], space: ColorSpace) -> [f32; 3] {
    use engine_api::color::{ChromaticAdaptation, WorkingSpace};
    match space {
        ColorSpace::Srgb => v.map(|v| v.clamp(0., 1.) * 255.),
        ColorSpace::DisplayP3 => {
            // Display P3 shares the sRGB transfer curve.
            let linear = v.map(|v| f64::from(srgb_eotf(v)));
            let m = WorkingSpace::LinearDisplayP3
                .conversion_to(WorkingSpace::LinearSrgb, ChromaticAdaptation::Bradford)
                .unwrap();
            m.apply(linear)
                .map(|v| pipeline_cpu::srgb_oetf(v as f32).clamp(0., 1.) * 255.)
        }
        other => panic!("unsupported comparison space {other:?}"),
    }
}

fn srgb_eotf(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// A 16-bit sRGB TIFF file export, decoded to 8-bit levels.
fn export_file(fixture: &Fixture, recipe: &Recipe) -> Vec<[f32; 3]> {
    export_file_with(fixture, recipe, None)
}

fn export_file_with(fixture: &Fixture, recipe: &Recipe, retouch: Retouch) -> Vec<[f32; 3]> {
    let dir = tempfile::tempdir().unwrap();
    let path = export::export_one(
        &image(fixture.source()),
        recipe,
        &ExportSettings {
            format: Format::Tiff { bits: 16 },
            color_space: ColorSpace::Srgb,
            output_dir: dir.path().to_path_buf(),
            retouch,
            ..Default::default()
        },
    )
    .unwrap();
    let mut decoder = tiff::decoder::Decoder::new(std::fs::File::open(path).unwrap()).unwrap();
    let samples = match decoder.read_image().unwrap() {
        tiff::decoder::DecodingResult::U16(v) => v,
        other => panic!("expected 16-bit samples, got {other:?}"),
    };
    samples
        .as_chunks::<3>()
        .0
        .iter()
        .map(|p| std::array::from_fn(|c| f32::from(p[c]) / 65535. * 255.))
        .collect()
}

/// (max, mean) |Develop - output| in 8-bit levels.
fn parity(develop: &[[f32; 3]], output: &[[f32; 3]]) -> (f32, f32) {
    assert_eq!(develop.len(), output.len(), "frame sizes differ");
    let (mut max, mut sum) = (0f32, 0f64);
    for (a, b) in develop.iter().zip(output) {
        for c in 0..3 {
            let d = (a[c] - b[c]).abs();
            max = max.max(d);
            sum += f64::from(d);
        }
    }
    (max, (sum / (3 * develop.len()) as f64) as f32)
}

/// How many pixels a recipe's linear Develop render puts outside sRGB.
fn out_of_srgb(image: &RawImage, recipe: &Recipe) -> usize {
    let renderer = Renderer::new(RendererConfig {
        process_version: recipe.process_version,
        ..Default::default()
    });
    let extent = Renderer::output_extent(image, &recipe.settings, 0).unwrap();
    let tiles = renderer
        .render_region_as(
            image,
            &recipe.settings,
            0,
            PixelRect::full(extent),
            RenderOutput::SceneLinear,
        )
        .unwrap();
    let m = engine_api::color::WorkingSpace::LinearRec2020
        .conversion_to(
            engine_api::color::WorkingSpace::LinearSrgb,
            engine_api::color::ChromaticAdaptation::Bradford,
        )
        .unwrap();
    let mut count = 0;
    for t in &tiles {
        let l = t.layout();
        let n = l.plane_len();
        let s = t.samples::<f32>().unwrap();
        for i in 0..l.extent.area() as usize {
            let v = m.apply([s[i], s[n + i], s[2 * n + i]].map(f64::from));
            count += usize::from(v.iter().any(|v| *v < -1e-3));
        }
    }
    count
}

fn processes() -> [ProcessVersion; 2] {
    [ProcessVersion::NATIVE_CURRENT, ProcessVersion::adobe(6)]
}

fn bound(process: ProcessVersion, saturated_perceptual: bool) -> f32 {
    if process.family == engine_api::recipe::ProcessFamily::Adobe {
        ADOBE_MAX
    } else if saturated_perceptual {
        NATIVE_MAX + NATIVE_GREY_POINT
    } else {
        NATIVE_MAX
    }
}

/// One row of the parity table; returns failures instead of panicking so
/// the whole table is reported.
fn check(
    label: &str,
    limit: f32,
    develop: &[[f32; 3]],
    output: &[[f32; 3]],
    failures: &mut Vec<String>,
) {
    let (max, mean) = parity(develop, output);
    let ok = max <= limit && mean <= MEAN;
    eprintln!(
        "ENG9 {label:<58} max {max:>7.3} mean {mean:>6.3} {}",
        if ok { "ok" } else { "FAIL" }
    );
    if !ok {
        failures.push(format!(
            "{label}: max {max} (limit {limit}) mean {mean} (limit {MEAN})"
        ));
    }
}

fn run(kind: Kind) {
    let fixture = fixture(kind);
    let mut failures = Vec::new();
    for process in processes() {
        let family = format!("{:?}{}", process.family, process.revision);
        // In gamut: saturation -100 makes every pixel neutral.
        for mapping in [GamutMapping::Perceptual, GamutMapping::Clip] {
            let r = recipe(process, mapping, -100.);
            let shown = develop(&fixture.image, &r);
            check(
                &format!("{kind:?} {family} in-gamut {mapping:?} export"),
                bound(process, false),
                &shown,
                &export_file(&fixture, &r),
                &mut failures,
            );
            check(
                &format!("{kind:?} {family} in-gamut {mapping:?} print sRGB"),
                bound(process, false),
                &shown,
                &print(&fixture, &r, ColorSpace::Srgb),
                &mut failures,
            );
            // Print's default document space (P3) is a deliberate output
            // transform; in gamut it converts back to the same sRGB values.
            check(
                &format!("{kind:?} {family} in-gamut {mapping:?} print P3"),
                bound(process, false),
                &shown,
                &print(&fixture, &r, ColorSpace::DisplayP3),
                &mut failures,
            );
        }
        // Saturated: the fixtures leave sRGB, so the gamut policy matters.
        for mapping in [GamutMapping::Perceptual, GamutMapping::Clip] {
            let r = recipe(process, mapping, 40.);
            assert!(
                out_of_srgb(&fixture.image, &r) > 0,
                "{kind:?} {family}: the saturated fixture must leave sRGB"
            );
            let shown = develop(&fixture.image, &r);
            check(
                &format!("{kind:?} {family} saturated {mapping:?} export"),
                bound(process, mapping == GamutMapping::Perceptual),
                &shown,
                &export_file(&fixture, &r),
                &mut failures,
            );
            check(
                &format!("{kind:?} {family} saturated {mapping:?} print sRGB"),
                bound(process, mapping == GamutMapping::Perceptual),
                &shown,
                &print(&fixture, &r, ColorSpace::Srgb),
                &mut failures,
            );
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn eng9_raw_original_export_and_print_match_develop() {
    run(Kind::Raw);
}

#[test]
fn eng9_rgb_original_export_and_print_match_develop() {
    run(Kind::Rgb);
}

#[test]
fn eng9_smart_preview_export_and_print_match_develop() {
    run(Kind::Proxy);
}

/// Adobe-process export is not the Native rendering of the same photo.
#[test]
fn eng9_adobe_export_is_not_the_native_rendering() {
    for kind in [Kind::Raw, Kind::Rgb] {
        let fixture = fixture(kind);
        let adobe = export_file(
            &fixture,
            &recipe(ProcessVersion::adobe(6), GamutMapping::Clip, 0.),
        );
        let native = export_file(
            &fixture,
            &recipe(ProcessVersion::NATIVE_CURRENT, GamutMapping::Clip, 0.),
        );
        let (max, _) = parity(&adobe, &native);
        assert!(max > 2., "{kind:?}: Adobe export equals Native ({max})");
    }
}

fn pq(v: f32) -> f32 {
    let l = (f64::from(v) * 203.0 / 10000.0)
        .clamp(0.0, 1.0)
        .powf(2610.0 / 16384.0);
    ((3424.0 / 4096.0 + 2413.0 / 128.0 * l) / (1.0 + 2392.0 / 128.0 * l)).powf(2523.0 / 32.0) as f32
}

/// A 16-bit Rec.2020 PQ PNG export, as code values.
fn hdr_export(fixture: &Fixture, recipe: &Recipe) -> Vec<f32> {
    let dir = tempfile::tempdir().unwrap();
    let path = export::export_one(
        &image(fixture.source()),
        recipe,
        &ExportSettings {
            format: Format::Png,
            hdr: Some(export::HdrTransfer::Pq),
            color_space: ColorSpace::Rec2020,
            output_dir: dir.path().to_path_buf(),
            ..Default::default()
        },
    )
    .unwrap();
    let decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap()));
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).unwrap();
    assert_eq!(info.bit_depth, png::BitDepth::Sixteen);
    buf[..info.buffer_size()]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| f32::from(u16::from_be_bytes(*b)))
        .collect()
}

/// HDR export of an Adobe-process recipe renders the Adobe pipeline (what
/// Develop's EDR viewport draws), not the Native one. The output colour
/// space (Rec.2020 PQ) is the deliberate output transform, so the comparison
/// is in PQ code values of Develop's EDR rendition: a neutral (in-gamut)
/// recipe within one 16-bit code of rounding plus float order.
#[test]
fn eng9_adobe_hdr_export_matches_develop_edr() {
    for kind in [Kind::Raw, Kind::Rgb, Kind::Proxy] {
        let fixture = fixture(kind);
        let mut r = recipe(ProcessVersion::adobe(6), GamutMapping::Perceptual, -100.);
        r.edit(EditMeta::user("eng9", 2), |s| {
            s.output.hdr = true;
            s.output.hdr_headroom_stops = 1.;
            s.tone.exposure = 0.7;
        })
        .unwrap();
        let headroom = 2f32;
        let renderer = Renderer::new(RendererConfig {
            process_version: r.process_version,
            ..Default::default()
        });
        // The host draws presentation (headroom) separately from pixel
        // settings (`develop::renderable`): the HDR toggle is not an operator.
        let mut drawn = r.settings.clone();
        drawn.output.hdr = false;
        drawn.output.hdr_headroom_stops = 0.;
        let extent = Renderer::output_extent(&fixture.image, &drawn, 0).unwrap();
        let tiles = renderer
            .render_region_as(
                &fixture.image,
                &drawn,
                0,
                PixelRect::full(extent),
                RenderOutput::DisplayLinear(image_core::Headroom::new(headroom)),
            )
            .unwrap();
        let mut edr = vec![[0f32; 3]; extent.area() as usize];
        let mut above_sdr = false;
        for t in &tiles {
            let l = t.layout();
            let n = l.plane_len();
            let (ox, oy) = t.coord().pixel_origin(engine_api::tile::TILE_SIZE);
            let s = t.samples::<f32>().unwrap();
            for y in 0..l.extent.height {
                for x in 0..l.extent.width {
                    let i = (y * l.extent.width + x) as usize;
                    let o = ((oy + y) * extent.width + ox + x) as usize;
                    edr[o] = std::array::from_fn(|c| s[c * n + i]);
                    above_sdr |= edr[o].iter().any(|v| *v > 1.0);
                }
            }
        }
        let m = engine_api::color::WorkingSpace::LinearSrgb
            .conversion_to(
                engine_api::color::WorkingSpace::LinearRec2020,
                engine_api::color::ChromaticAdaptation::Bradford,
            )
            .unwrap();
        let codes = hdr_export(&fixture, &r);
        assert_eq!(codes.len(), edr.len() * 3);
        let mut max = 0f32;
        for (i, v) in edr.iter().enumerate() {
            let rec2020 = m.apply(v.map(f64::from)).map(|c| c as f32);
            for c in 0..3 {
                let expected = pq(rec2020[c]) * 65535.;
                max = max.max((codes[i * 3 + c] - expected).abs());
            }
        }
        // Before ENG-9 the HDR export rendered the Native pipeline.
        let mut native = r.clone();
        native.process_version = ProcessVersion::NATIVE_CURRENT;
        let native_max = hdr_export(&fixture, &native)
            .iter()
            .zip(&codes)
            .map(|(a, b)| (a - b).abs())
            .fold(0f32, f32::max);
        // The Adobe pipeline is display-referred: its EDR rendition stays at
        // or below SDR white, and so does the HDR file.
        eprintln!(
            "ENG9 {kind:?} Adobe6 HDR PQ export vs Develop EDR: max {max:.3} codes; \
             Native rendering differs by {native_max:.0} codes; above SDR white: {above_sdr}"
        );
        assert!(
            native_max > 1000.,
            "{kind:?}: HDR export must not be Native"
        );
        assert!(
            max <= 1.0,
            "{kind:?}: HDR export differs from Develop EDR by {max} codes"
        );
    }
}

/// Local adjustments and a crop take the same path as Develop for both
/// processes: a gradient mask with exposure, on a saturated Perceptual recipe.
#[test]
fn eng9_local_adjustments_and_crop_export_and_print_match_develop() {
    use engine_api::recipe::{LocalAdjustment, LocalParams, MaskComponent, MaskKind};
    let mut failures = Vec::new();
    for kind in [Kind::Raw, Kind::Rgb, Kind::Proxy] {
        let fixture = fixture(kind);
        for process in processes() {
            let mut r = recipe(process, GamutMapping::Perceptual, 40.);
            r.edit(EditMeta::user("eng9", 3), |s| {
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
            })
            .unwrap();
            let shown = develop(&fixture.image, &r);
            let family = format!("{:?}{}", process.family, process.revision);
            let limit = bound(process, true);
            check(
                &format!("{kind:?} {family} gradient+crop export"),
                limit,
                &shown,
                &export_file(&fixture, &r),
                &mut failures,
            );
            check(
                &format!("{kind:?} {family} gradient+crop print sRGB"),
                limit,
                &shown,
                &print(&fixture, &r, ColorSpace::Srgb),
                &mut failures,
            );
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// ENG-9b (REV-ENG-9 SF3): a retouch spot on an Adobe RAW original exports
/// and prints exactly as Develop draws it. The test renderer clones a
/// centred box from a horizontal source offset, so a frame or ordering
/// mismatch between the paths would move pixels.
#[test]
fn eng9b_adobe_raw_retouch_export_and_print_match_develop() {
    use engine_api::recipe::{
        MaskComponent, MaskKind, RetouchOperation,
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
    let fixture = fixture(Kind::Raw);
    let mut failures = Vec::new();
    for mapping in [GamutMapping::Perceptual, GamutMapping::Clip] {
        let plain = recipe(ProcessVersion::adobe(6), mapping, 40.);
        let mut r = plain.clone();
        r.edit(EditMeta::user("eng9b", 1), |s| {
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
        })
        .unwrap();
        let shown = develop_with(&fixture.image, &r, Some(retouch.clone()));
        // The spot must actually change the frame.
        assert!(
            parity(&shown, &develop(&fixture.image, &plain)).0 > 2.,
            "{mapping:?}: retouch must change the frame"
        );
        let exported = export_file_with(&fixture, &r, Some(retouch.clone()));
        assert!(
            parity(&exported, &export_file(&fixture, &plain)).0 > 2.,
            "{mapping:?}: the export must apply the retouch"
        );
        check(
            &format!("Raw Adobe6 retouch {mapping:?} export"),
            ADOBE_MAX,
            &shown,
            &exported,
            &mut failures,
        );
        check(
            &format!("Raw Adobe6 retouch {mapping:?} print sRGB"),
            ADOBE_MAX,
            &shown,
            &print_with(&fixture, &r, ColorSpace::Srgb, Some(retouch.clone())),
            &mut failures,
        );
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// ENG-9b (REV-ENG-9 SF1): an Adobe-process HDR export tells the user that
/// the file holds a standard-dynamic-range rendition. Native HDR does not.
#[test]
fn eng9b_adobe_hdr_export_warns_that_it_is_sdr() {
    for kind in [Kind::Raw, Kind::Rgb, Kind::Proxy] {
        let fixture = fixture(kind);
        for process in processes() {
            let mut r = recipe(process, GamutMapping::Perceptual, 0.);
            r.edit(EditMeta::user("eng9b", 2), |s| {
                s.output.hdr = true;
                s.output.hdr_headroom_stops = 1.;
            })
            .unwrap();
            let dir = tempfile::tempdir().unwrap();
            let rendered = export::render_one_cancellable(
                &image(fixture.source()),
                &r,
                &ExportSettings {
                    format: Format::Png,
                    hdr: Some(export::HdrTransfer::Pq),
                    color_space: ColorSpace::Rec2020,
                    output_dir: dir.path().to_path_buf(),
                    ..Default::default()
                },
                &CancellationToken::new(),
                None,
                None,
            )
            .unwrap();
            let warned = rendered.warnings().iter().any(|w| {
                w.contains("Lightroom-process edits render in standard dynamic range")
                    && w.contains("no highlights above SDR white")
            });
            assert_eq!(
                warned,
                process.family == engine_api::recipe::ProcessFamily::Adobe,
                "{kind:?} {process:?}: {:?}",
                rendered.warnings()
            );
            // The warning reaches the report written beside the file.
            let path = rendered.finish(&CancellationToken::new()).unwrap();
            let report = std::fs::read_dir(dir.path())
                .unwrap()
                .map(|e| e.unwrap().path())
                .filter(|p| p != &path && p.extension().is_none_or(|e| e != "xmp"))
                .filter_map(|p| std::fs::read_to_string(p).ok())
                .any(|t| t.contains("standard dynamic range"));
            assert_eq!(
                report,
                process.family == engine_api::recipe::ProcessFamily::Adobe,
                "{kind:?} {process:?}: warning report"
            );
        }
    }
}
