use engine_api::recipe::{DevelopSettings, settings::LensProfileSource};
use lens::{CalibrationSample, Profile};
use pipeline_cpu::{CorrectionSource, Image, LensContext, resolve_lens};
#[test]
fn auto_calibration_recovers_synthetic_radial_falloff() {
    let w = 65;
    let h = 65;
    let p = (0..w * h)
        .map(|i| {
            let x = 2. * (i % w) as f32 / (w - 1) as f32 - 1.;
            let y = 2. * (i / w) as f32 / (h - 1) as f32 - 1.;
            0.5 * (1. - 0.2 * (x * x + y * y))
        })
        .collect();
    let image = Image::new(w, h, vec![p; 3]).unwrap();
    let resolved = resolve_lens(
        &image,
        &DevelopSettings::default().lens,
        None,
        &LensContext::default(),
    )
    .unwrap();
    assert_eq!(resolved.source(), CorrectionSource::Image);
    assert!((resolved.sample().unwrap().vignette[0] + 0.2).abs() < 0.02);
}
#[test]
fn profile_render_applies_vignette_and_channel_maps() {
    let profile = Profile {
        camera: None,
        maker: "test".into(),
        model: "test".into(),
        samples: vec![CalibrationSample {
            vignette: [-0.1, 0., 0.],
            ca_red: [0.98, 0., 0.],
            ..Default::default()
        }],
    };
    let context = LensContext {
        profile: Some(&profile),
        ..Default::default()
    };
    let image = Image::new(
        32,
        24,
        vec![(0..768).map(|i| (i % 32) as f32 / 40. + 0.1).collect(); 3],
    )
    .unwrap();
    let mut s = DevelopSettings::default();
    s.detail.sharpening.amount = 0.;
    s.detail.noise_reduction.color = 0.;
    let out = pipeline_cpu::render_linear_scaled_with_lens(
        &s,
        &pipeline_cpu::RenderSource::Rgb(&image),
        1,
        &context,
    )
    .unwrap();
    assert!(out.planes()[1][0] > image.planes()[1][0]);
    assert!((out.planes()[0][20] - out.planes()[1][20]).abs() > 1e-4);
}
fn flat() -> Image {
    Image::new(32, 24, vec![vec![0.25; 768]; 3]).unwrap()
}
#[test]
fn supplied_profile_beats_image_and_none_bypasses_it() {
    let profile = Profile {
        camera: None,
        maker: "test".into(),
        model: "test".into(),
        samples: vec![CalibrationSample {
            vignette: [-0.2, 0., 0.],
            ..Default::default()
        }],
    };
    let context = LensContext {
        profile: Some(&profile),
        ..Default::default()
    };
    let mut s = DevelopSettings::default();
    let resolved = resolve_lens(&flat(), &s.lens, None, &context).unwrap();
    assert_eq!(resolved.source(), CorrectionSource::Database);
    assert_eq!(resolved.sample().unwrap().vignette, [-0.2, 0., 0.]);
    s.lens.profile = LensProfileSource::None;
    assert_eq!(
        resolve_lens(&flat(), &s.lens, None, &context)
            .unwrap()
            .source(),
        CorrectionSource::Manual
    );
}

#[test]
fn postdemosaic_auto_ca_reduces_channel_edge_error() {
    let n = 80_u32;
    let f = |x: f64, y: f64| (0.5 + 0.2 * (19. * x).sin() + 0.2 * (23. * y).sin()) as f32;
    let planes = (0..3)
        .map(|c| {
            (0..n * n)
                .map(|i| {
                    let x = 2. * (i % n) as f64 / (n - 1) as f64 - 1.;
                    let y = 2. * (i / n) as f64 / (n - 1) as f64 - 1.;
                    let scale = [1.012, 1., 0.989][c];
                    f(x / scale, y / scale)
                })
                .collect()
        })
        .collect();
    let image = Image::new(n, n, planes).unwrap();
    let mut s = DevelopSettings::default();
    s.lens.profile = LensProfileSource::None;
    s.detail.sharpening.amount = 0.;
    s.detail.noise_reduction.color = 0.;
    let error = |im: &Image| {
        (5..75)
            .flat_map(|y| (5..75).map(move |x| (y * n + x) as usize))
            .map(|i| {
                (im.planes()[0][i] - im.planes()[1][i]).abs()
                    + (im.planes()[2][i] - im.planes()[1][i]).abs()
            })
            .sum::<f32>()
    };
    let result =
        pipeline_cpu::render_linear_scaled(&s, &pipeline_cpu::RenderSource::Rgb(&image), 1)
            .unwrap();
    assert!(
        error(&result) < error(&image) * 0.5,
        "before {} after {}",
        error(&image),
        error(&result)
    );
}
#[test]
fn zero_profile_strengths_are_bit_exact_to_off() {
    let p = Profile {
        camera: None,
        maker: "test".into(),
        model: "test".into(),
        samples: vec![CalibrationSample {
            distortion: lens::BrownConrady {
                k1: 0.1,
                ..Default::default()
            },
            ca_red: [1.02, 0., 0.],
            vignette: [-0.2, 0., 0.],
            ..Default::default()
        }],
    };
    let context = LensContext {
        profile: Some(&p),
        ..Default::default()
    };
    let image = Image::new(
        32,
        24,
        vec![(0..768).map(|i| (i % 32) as f32 / 50.).collect(); 3],
    )
    .unwrap();
    let mut s = DevelopSettings::default();
    s.lens.distortion_scale = 0.;
    s.lens.vignetting_scale = 0.;
    s.lens.chromatic_aberration_scale = 0.;
    let a = pipeline_cpu::render_linear_scaled_with_lens(
        &s,
        &pipeline_cpu::RenderSource::Rgb(&image),
        1,
        &context,
    )
    .unwrap();
    s.lens.profile = LensProfileSource::None;
    s.lens.remove_chromatic_aberration = false;
    let b = pipeline_cpu::render_linear_scaled(&s, &pipeline_cpu::RenderSource::Rgb(&image), 1)
        .unwrap();
    assert_eq!(a.planes(), b.planes());
}
