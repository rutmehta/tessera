//! SP-INT2 (REV-SP-A S1, coordinator ruling): Smart Previews use exactly the
//! gamut policy ordinary originals use, in Develop and in export.
//!
//! Before ENG-9 an Adobe-process original was drawn in Develop with a hard
//! clip in sRGB whatever `output.gamut_mapping` said, while export honoured
//! it. ENG-9 makes the Adobe Output stage honour the recipe's gamut mapping
//! the way export does (`CpuStageOp::adobe_display`), for originals and
//! proxies alike: `Clip` is still a hard clip, `Perceptual` compresses
//! chroma toward the working-space luminance.
use engine_api::{
    id::ImageId,
    jobs::CancellationToken,
    recipe::{EditMeta, ProcessVersion, Recipe, settings::GamutMapping},
};
use export::{ColorSpace, ExportImage, RenderRequest, Resize, SharpenFor};
use image_core::{PixelRect, RawImage, RenderOutput, Renderer, RendererConfig};
use pipeline_cpu::{CameraLinearProxy, RenderSource};

fn saturated_proxy() -> CameraLinearProxy {
    let dng = raw_decode::lossy_dng::read(&mut std::io::Cursor::new(include_bytes!(
        "../../raw-decode/tests/fixtures/linear-gradient-jxl.dng"
    )))
    .unwrap()
    .unwrap();
    // Display orientation 1: Develop's sensor frame is the printed frame.
    CameraLinearProxy::from_dng(dng)
        .unwrap()
        .with_catalog_orientation(1)
        .unwrap()
}

fn recipe(mapping: GamutMapping, saturation: f32) -> Recipe {
    let mut recipe = Recipe {
        process_version: ProcessVersion::adobe(6),
        ..Default::default()
    };
    recipe
        .edit(EditMeta::user("gamut", 1), |s| {
            s.output.gamut_mapping = mapping;
            s.color.saturation = saturation;
            s.detail.sharpening.amount = 0.;
        })
        .unwrap();
    recipe
}

fn develop(image: &RawImage, recipe: &Recipe, output: RenderOutput) -> Vec<Vec<f32>> {
    let renderer = Renderer::new(RendererConfig {
        process_version: recipe.process_version,
        ..Default::default()
    });
    let extent = Renderer::output_extent(image, &recipe.settings, 0).unwrap();
    let tiles = renderer
        .render_region_as(image, &recipe.settings, 0, PixelRect::full(extent), output)
        .unwrap();
    let mut planes = vec![vec![0f32; extent.area() as usize]; 3];
    for t in &tiles {
        let l = t.layout();
        let n = l.plane_len();
        let (ox, oy) = t.coord().pixel_origin(engine_api::tile::TILE_SIZE);
        let values: Vec<f32> = match output {
            RenderOutput::Display => t
                .samples::<u8>()
                .unwrap()
                .iter()
                .map(|v| f32::from(*v))
                .collect(),
            _ => t.samples::<f32>().unwrap().to_vec(),
        };
        for y in 0..l.extent.height {
            for x in 0..l.extent.width {
                let i = (y * l.extent.width + x) as usize;
                let o = ((oy + y) * extent.width + ox + x) as usize;
                for (c, plane) in planes.iter_mut().enumerate() {
                    plane[o] = values[c * n + i];
                }
            }
        }
    }
    planes
}

fn print(proxy: &CameraLinearProxy, recipe: &Recipe) -> image::Rgb32FImage {
    export::render_pixels(
        &ExportImage {
            source: RenderSource::CameraLinear(proxy),
            name: "print",
            sequence: 1,
            date: "",
            metadata: None,
        },
        recipe,
        &RenderRequest {
            color_space: ColorSpace::Srgb,
            resize: Resize::None,
            sharpen_for: SharpenFor::None,
            scale: 1,
        },
        &CancellationToken::new(),
        None,
    )
    .unwrap()
}

/// (max, mean) |Develop - print| in 8-bit levels.
fn parity(develop: &[Vec<f32>], print: &image::Rgb32FImage) -> (f32, f32) {
    let n = develop[0].len();
    assert_eq!(n, (print.width() * print.height()) as usize);
    let mut max = 0f32;
    let mut sum = 0f32;
    for (i, p) in print.pixels().enumerate() {
        for (plane, v) in develop.iter().zip(p.0) {
            let d = (plane[i] - (v.clamp(0., 1.) * 255.)).abs();
            max = max.max(d);
            sum += d;
        }
    }
    (max, sum / (3 * n) as f32)
}

/// A saturated synthetic Bayer original (strong red/blue photosites).
fn adobe_original() -> RawImage {
    let (w, h) = (32u32, 24u32);
    let metadata = raw_decode::RawMetadata {
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
        maker_lens: None,
    };
    let cfa = raw_decode::CfaImage::from_linear(
        w,
        h,
        (0..w * h)
            .map(|i| {
                let (x, y) = (i % w, i / w);
                match (x % 2, y % 2) {
                    (0, 0) => 0.6,
                    (1, 1) => 0.02 + x as f32 / 400.,
                    _ => 0.05,
                }
            })
            .collect(),
    )
    .unwrap();
    RawImage::new(
        ImageId(9100),
        std::sync::Arc::new(cfa),
        std::sync::Arc::new(metadata),
    )
    .unwrap()
}

#[test]
fn sp_int2_saturated_proxy_develop_uses_the_adobe_original_output_stage() {
    let proxy = saturated_proxy();
    let image = RawImage::from_camera_linear_proxy(
        ImageId(9101),
        ImageId(9102),
        std::sync::Arc::new(proxy),
    )
    .unwrap();
    for (image, needs_clipping) in [(image, true), (adobe_original(), false)] {
        for mapping in [GamutMapping::Perceptual, GamutMapping::Clip] {
            let r = recipe(mapping, 0.);
            let shown = develop(&image, &r, RenderOutput::Display);
            let linear = develop(&image, &r, RenderOutput::SceneLinear);
            // The Adobe Output stage (originals and proxies): Rec.2020 ->
            // linear sRGB, the recipe's gamut mapping, sRGB OETF. Clip is a
            // hard clip; Perceptual compresses chroma toward the Rec.2020
            // luminance, as the managed export transform does (ENG-9).
            let m = engine_api::color::WorkingSpace::LinearSrgb
                .to_xyz()
                .inverse()
                .unwrap()
                * engine_api::color::WorkingSpace::LinearRec2020.to_xyz();
            let mut clipped = 0usize;
            for i in 0..linear[0].len() {
                let v = [linear[0][i], linear[1][i], linear[2][i]];
                let s: [f32; 3] = std::array::from_fn(|r| {
                    let row = m.0[r];
                    row[0] as f32 * v[0] + row[1] as f32 * v[1] + row[2] as f32 * v[2]
                });
                clipped += s.iter().filter(|s| !(0. ..=1.).contains(*s)).count();
                let mapped = if mapping == GamutMapping::Clip {
                    s
                } else {
                    let grey = (0.2627 * v[0] + 0.6780 * v[1] + 0.0593 * v[2]).clamp(0., 1.);
                    let mut chroma = 1f32;
                    for c in s {
                        if c < 0. {
                            chroma = chroma.min(-grey / (c - grey));
                        }
                        if c > 1. {
                            chroma = chroma.min((1. - grey) / (c - grey));
                        }
                    }
                    s.map(|c| grey + chroma * (c - grey))
                };
                for (r, s) in mapped.into_iter().enumerate() {
                    let expected = (pipeline_cpu::srgb_oetf(s.clamp(0., 1.)) * 255.).round();
                    assert!(
                        (shown[r][i] - expected).abs() <= 1.,
                        "{mapping:?}: Develop must use the Adobe original's Output stage"
                    );
                }
            }
            assert!(
                clipped > 0 || !needs_clipping,
                "the fixture must be out of gamut"
            );
        }
    }
}

#[test]
fn sp_int2_saturated_proxy_print_matches_develop_under_the_shared_policy() {
    let proxy = saturated_proxy();
    let image = RawImage::from_camera_linear_proxy(
        ImageId(9103),
        ImageId(9104),
        std::sync::Arc::new(proxy.clone()),
    )
    .unwrap();
    // In gamut (saturation -100): Develop and print agree whatever the policy.
    for mapping in [GamutMapping::Perceptual, GamutMapping::Clip] {
        let r = recipe(mapping, -100.);
        let (max, mean) = parity(
            &develop(&image, &r, RenderOutput::Display),
            &print(&proxy, &r),
        );
        assert!(
            max <= 2. && mean <= 0.6,
            "{mapping:?} in gamut: max {max} mean {mean}"
        );
    }
    // Saturated with Clip: both paths hard-clip, so they agree too.
    let r = recipe(GamutMapping::Clip, 0.);
    let (max, mean) = parity(
        &develop(&image, &r, RenderOutput::Display),
        &print(&proxy, &r),
    );
    assert!(
        max <= 2. && mean <= 0.6,
        "Clip saturated: max {max} mean {mean}"
    );
    // Saturated with Perceptual: print honours the recipe's gamut mapping,
    // and since ENG-9 so does Develop, so they agree here too.
    let r = recipe(GamutMapping::Perceptual, 0.);
    let perceptual = print(&proxy, &r);
    let (max, mean) = parity(&develop(&image, &r, RenderOutput::Display), &perceptual);
    assert!(
        max <= 2. && mean <= 0.6,
        "Perceptual saturated: max {max} mean {mean}"
    );
    let clip = print(&proxy, &recipe(GamutMapping::Clip, 0.));
    assert!(
        perceptual.pixels().zip(clip.pixels()).any(|(a, b)| a
            .0
            .iter()
            .zip(b.0)
            .any(|(a, b)| (a - b).abs() > 2. / 255.)),
        "print must apply the recipe's gamut mapping, as for originals"
    );
}
