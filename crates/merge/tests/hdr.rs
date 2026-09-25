use merge::{
    LinearImage,
    hdr::{BracketFrame, Deghost, Exposure, HdrOptions, hdr},
};

#[test]
fn hdr_uses_registration_and_skips_missing_border_samples() {
    let mut truth = scene(160, 120);
    for (i, p) in truth.pixels.iter_mut().enumerate() {
        let x = (i % 160) as f32;
        let y = (i / 160) as f32;
        let v = 0.35
            + 0.12 * (x * 0.23 + y * 0.17).sin()
            + 0.1 * (x * 0.41 - y * 0.13).cos()
            + 0.07 * ((x * 0.07).sin() * 7. + y * 0.31).cos();
        *p = [v, v * 0.8, v * 0.6];
    }
    let mut frames = bracket(&truth);
    for (j, dx, dy) in [(0, 4., -3.), (2, -2., 2.)] {
        let original = frames[j].image.clone();
        for y in 0..120 {
            for x in 0..160 {
                frames[j].image.pixels[y * 160 + x] = original
                    .sample(x as f64 - dx, y as f64 - dy)
                    .unwrap_or([0.; 3]);
            }
        }
    }
    let out = hdr(
        &frames,
        &HdrOptions {
            reference: 1,
            deghost: Deghost::None,
            ..Default::default()
        },
    )
    .unwrap();
    let mse = out
        .image
        .pixels
        .iter()
        .zip(&truth.pixels)
        .map(|(a, b)| (a[0] - b[0]).powi(2) as f64)
        .sum::<f64>()
        / truth.pixels.len() as f64;
    eprintln!("registered HDR MSE {mse}");
    assert!(mse < 0.00002);
    assert!((out.image.pixels[0][0] - truth.pixels[0][0]).abs() < 0.002);
}

#[test]
fn moving_object_is_masked_and_reference_is_preserved() {
    let truth = scene(96, 64);
    let mut frames = bracket(&truth);
    for y in 20..35 {
        for x in 15..30 {
            frames[0].image.pixels[y * 96 + x] = [0.8, 0.65, 0.5];
        }
    }
    let opts = HdrOptions {
        reference: 1,
        auto_align: false,
        deghost: Deghost::High,
        ..Default::default()
    };
    let out = hdr(&frames, &opts).unwrap();
    for y in 20..35 {
        for x in 15..30 {
            let i = y * 96 + x;
            assert!(out.deghost_mask[i]);
            assert_eq!(out.image.pixels[i], frames[1].image.pixels[i]);
        }
    }
    let unmasked = hdr(
        &frames,
        &HdrOptions {
            deghost: Deghost::None,
            ..opts
        },
    )
    .unwrap();
    assert!(unmasked.deghost_mask.iter().all(|v| !v));
    assert!(
        (unmasked.image.pixels[25 * 96 + 20][0] - out.image.pixels[25 * 96 + 20][0]).abs() > 0.1
    );
    assert!(out.deghost_mask.iter().filter(|v| **v).count() < 400);
}

#[test]
fn histogram_refines_bad_exif_and_strength_is_monotone() {
    let mut truth = scene(64, 48);
    for p in &mut truth.pixels {
        for v in p {
            *v *= 0.4;
        }
    }
    let mut frames = bracket(&truth);
    frames[0].exposure.shutter_s *= 1.1;
    let opts = HdrOptions {
        reference: 1,
        auto_align: false,
        deghost: Deghost::None,
        ..Default::default()
    };
    let out = hdr(&frames, &opts).unwrap();
    assert!((out.exposure_ratios[0] - 0.25).abs() < 0.001);
    for y in 10..20 {
        for x in 10..20 {
            for v in &mut frames[0].image.pixels[y * 64 + x] {
                *v *= 1.2;
            }
        }
    }
    let mut counts = Vec::new();
    for strength in [Deghost::None, Deghost::Low, Deghost::Medium, Deghost::High] {
        counts.push(
            hdr(
                &frames,
                &HdrOptions {
                    deghost: strength,
                    ..opts.clone()
                },
            )
            .unwrap()
            .deghost_mask
            .iter()
            .filter(|v| **v)
            .count(),
        );
    }
    assert_eq!(counts[0], 0);
    assert!(counts.windows(2).all(|v| v[0] <= v[1]));
    assert!(counts[3] >= 100);
}
fn scene(w: usize, h: usize) -> LinearImage {
    LinearImage {
        width: w,
        height: h,
        pixels: (0..w * h)
            .map(|i| {
                let x = (i % w) as f32;
                let y = (i / w) as f32;
                let v = 0.02 + 1.6 * x / w as f32 + 0.15 * (x * 0.21 + y * 0.31).sin().abs();
                [v, v * 0.8, v * 0.65]
            })
            .collect(),
        color_matrix: [[0.8, -0.1, 0.2], [0.1, 1., -0.1], [0., 0.1, 0.9]],
        as_shot_neutral: [0.5, 1., 0.75],
    }
}
fn bracket(im: &LinearImage) -> Vec<BracketFrame> {
    [0.25, 1., 4.]
        .into_iter()
        .enumerate()
        .map(|(k, gain)| {
            let mut image = im.clone();
            for (i, p) in image.pixels.iter_mut().enumerate() {
                for (c, v) in p.iter_mut().enumerate() {
                    let noise = ((i * 37 + c * 17 + k * 71) % 101) as f32 / 100. - 0.5;
                    *v = (*v * gain + noise * 0.0005).clamp(0., 1.);
                }
            }
            BracketFrame {
                image,
                exposure: Exposure {
                    shutter_s: gain as f64,
                    iso: 100.,
                    aperture: 1.,
                },
            }
        })
        .collect()
}
#[test]
fn noisy_two_ev_bracket_recovers_linear_scene() {
    let truth = scene(96, 64);
    let frames = bracket(&truth);
    let out = hdr(
        &frames,
        &HdrOptions {
            reference: 1,
            auto_align: false,
            deghost: Deghost::None,
            ..Default::default()
        },
    )
    .unwrap();
    let mut worst = 0.0_f32;
    let mut sum = 0.;
    for (a, b) in out.image.pixels.iter().zip(&truth.pixels) {
        for c in 0..3 {
            let e = (a[c] - b[c]).abs() / b[c].max(0.02);
            worst = worst.max(e);
            sum += e;
        }
    }
    let mean = sum / (truth.pixels.len() * 3) as f32;
    eprintln!("HDR relative error: mean {mean:.6}, worst {worst:.6}");
    assert!(worst < 0.02, "{worst}");
    assert!(out.image.pixels.iter().any(|p| p[0] > 1.));
    assert!(out.deghost_mask.iter().all(|v| !v));
    assert_eq!(out.image.color_matrix, truth.color_matrix);
}
