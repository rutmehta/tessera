use merge::{LinearImage, layers::*};
fn image(w: usize, h: usize) -> LinearImage {
    LinearImage {
        width: w,
        height: h,
        pixels: (0..w * h)
            .map(|i| {
                let x = (i % w) as f32;
                let y = (i / w) as f32;
                [0.4 + 0.15 * (x * 0.31).sin() + 0.12 * (y * 0.27).cos(); 3]
            })
            .collect(),
        color_matrix: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
        as_shot_neutral: [1.; 3],
    }
}
#[test]
fn translated_rotation_registration_is_subpixel_and_deterministic() {
    let a = image(96, 80);
    let mut b = a.clone();
    let angle = 1.2_f64.to_radians();
    let (s, c) = angle.sin_cos();
    for y in 0..80 {
        for x in 0..96 {
            let dx = x as f64 - 47.5 - 3.2;
            let dy = y as f64 - 39.5 + 2.4;
            b.pixels[y * 96 + x] = a
                .sample(c * dx + s * dy + 47.5, -s * dx + c * dy + 39.5)
                .unwrap_or([0.; 3]);
        }
    }
    let opts = AlignOptions {
        mode: AlignMode::Collage,
        ..Default::default()
    };
    let out = align_layers(&[a.clone(), b.clone()], &opts).unwrap();
    let again = align_layers(&[a, b], &opts).unwrap();
    assert_eq!(out.transforms, again.transforms);
    let transform::Operation::Free(t) = &out.transforms[1].operation else {
        panic!()
    };
    let mut error = 0.;
    for (x, y) in [(20., 20.), (70., 20.), (20., 60.), (70., 60.)] {
        let dx = x - 47.5;
        let dy = y - 39.5;
        let src = [
            c * dx - s * dy + 47.5 + 3.2 + 0.5,
            s * dx + c * dy + 39.5 - 2.4 + 0.5,
        ];
        let p = t.map(src).unwrap();
        error +=
            (p[0] + out.origin[0] - x - 0.5).powi(2) + (p[1] + out.origin[1] - y - 0.5).powi(2);
    }
    assert!((error / 4.).sqrt() < 0.5, "RMS {}", (error / 4.).sqrt());
}
#[test]
fn identity_has_exact_canvas_and_transform() {
    let im = image(32, 24);
    let out = align_layers(std::slice::from_ref(&im), &AlignOptions::default()).unwrap();
    assert_eq!((out.width, out.height), (32, 24));
    assert_eq!(out.origin, [0., 0.]);
    assert_eq!(out.images[0].pixels, im.pixels);
    assert!(out.coverage[0].iter().all(|v| *v));
}

#[test]
fn focus_selects_sharp_regions_and_corrections_reconstruct() {
    let (w, h) = (96, 64);
    let mut a = image(w, h);
    let mut b = a.clone();
    for y in 0..h {
        for x in 0..w {
            let sharp = if (x + y) % 2 == 0 { 0.2 } else { 0.8 };
            a.pixels[y * w + x] = [if x < w / 2 { sharp } else { 0.5 }; 3];
            b.pixels[y * w + x] = [if x >= w / 2 { sharp } else { 0.5 }; 3];
        }
    }
    let ims = vec![a, b];
    let cov = vec![vec![true; w * h]; 2];
    let out = blend_layers(
        &ims,
        &cov,
        &BlendOptions {
            mode: BlendMode::StackImages,
            ..Default::default()
        },
    )
    .unwrap();
    let mut correct = 0;
    for i in 0..w * h {
        let k = usize::from(i % w >= w / 2);
        correct += usize::from(out.masks[k][i] > 0.5);
        for c in 0..3 {
            let reconstructed = (0..2)
                .map(|l| out.masks[l][i] * (ims[l].pixels[i][c] + out.corrections[l][i][c]))
                .sum::<f32>();
            assert!((reconstructed - out.image.pixels[i][c]).abs() < 1e-5);
        }
    }
    assert!(correct as f64 / (w * h) as f64 >= 0.95);
}

#[test]
fn panorama_multiband_softens_seam_preserves_coverage_and_is_repeatable() {
    let (w, h) = (64, 32);
    let mut a = image(w, h);
    a.pixels.fill([0.2; 3]);
    let mut b = a.clone();
    b.pixels.fill([0.8; 3]);
    let ims = vec![a, b];
    let cov = vec![
        (0..w * h).map(|i| i % w < 44).collect(),
        (0..w * h).map(|i| i % w >= 20).collect(),
    ];
    let o = BlendOptions {
        seamless_tones: false,
        ..Default::default()
    };
    let out = blend_layers(&ims, &cov, &o).unwrap();
    let again = blend_layers(&ims, &cov, &o).unwrap();
    assert_eq!(out.masks, again.masks);
    assert_eq!(out.image.pixels, again.image.pixels);
    let jump = (1..w)
        .map(|x| (out.image.pixels[16 * w + x][0] - out.image.pixels[16 * w + x - 1][0]).abs())
        .fold(0f32, f32::max);
    assert!(jump < 0.3, "jump {jump}");
    for i in 0..w * h {
        assert_eq!(out.masks.iter().map(|m| m[i]).sum::<f32>(), 1.);
        for (k, covered) in cov.iter().enumerate() {
            if !covered[i] {
                assert_eq!(out.masks[k][i], 0.)
            }
        }
        for c in 0..3 {
            let p = (0..2)
                .map(|k| out.masks[k][i] * (ims[k].pixels[i][c] + out.corrections[k][i][c]))
                .sum::<f32>();
            assert!((p - out.image.pixels[i][c]).abs() < 1e-5);
        }
    }
}

#[test]
fn seamless_tones_removes_constant_exposure_step() {
    let mut a = image(64, 32);
    a.pixels.fill([0.2; 3]);
    let mut b = a.clone();
    b.pixels.fill([0.4; 3]);
    let coverage = vec![
        (0..2048).map(|i| i % 64 < 45).collect(),
        (0..2048).map(|i| i % 64 >= 20).collect(),
    ];
    let out = blend_layers(&[a, b], &coverage, &BlendOptions::default()).unwrap();
    assert!(out.image.pixels.iter().all(|p| (p[0] - 0.2).abs() < 1e-5));
}
#[test]
fn invalid_inputs_and_unsupported_options_error_and_fill_is_explicit() {
    let im = image(8, 8);
    assert!(align_layers(&[], &AlignOptions::default()).is_err());
    for mode in [AlignMode::Cylindrical, AlignMode::Spherical] {
        assert!(
            align_layers(
                std::slice::from_ref(&im),
                &AlignOptions {
                    mode,
                    ..Default::default()
                }
            )
            .is_ok()
        );
    }
    for (vignette_removal, geometric_distortion) in [(true, false), (false, true)] {
        assert!(
            align_layers(
                std::slice::from_ref(&im),
                &AlignOptions {
                    vignette_removal,
                    geometric_distortion,
                    ..Default::default()
                }
            )
            .is_err()
        );
    }
    assert!(
        blend_layers(
            std::slice::from_ref(&im),
            &[vec![true; 3]],
            &BlendOptions::default()
        )
        .is_err()
    );
    let mut cov = vec![true; 64];
    cov[27] = false;
    let out = blend_layers(
        &[im],
        &[cov],
        &BlendOptions {
            fill_transparent: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(out.fill_mask[27]);
    assert!(!out.coverage[27]);
    assert_eq!(out.image.pixels[27], [0.; 3]);
}
#[test]
#[ignore = "3x24 MP allocation/time benchmark; run release explicitly"]
fn benchmark_three_24mp_layers() {
    let mut ims = Vec::new();
    for k in 0..3 {
        let mut im = image(6000, 4000);
        for y in 0..4000 {
            for x in 0..6000 {
                let xx = (x as f64 + k as f64 * 40.) / 12.;
                let yy = y as f64 / 12.;
                let v = (0.5
                    + 0.15 * (xx * 0.17 + yy * 0.09).sin()
                    + 0.12 * ((xx * 0.039).sin() * 7. + (yy * 0.051).cos() * 9.).sin())
                    as f32;
                im.pixels[y * 6000 + x] = [v, 0.8 * v, 0.6 * v];
            }
        }
        ims.push(im);
    }
    let start = std::time::Instant::now();
    let aligned = align_layers(
        &ims,
        &AlignOptions {
            mode: AlignMode::Reposition,
            ..Default::default()
        },
    )
    .unwrap();
    let align_time = start.elapsed();
    drop(ims);
    eprintln!(
        "3x24 MP alignment+render: {align_time:?}, union {}x{}",
        aligned.width, aligned.height
    );
    let start = std::time::Instant::now();
    let out = blend_layers(&aligned.images, &aligned.coverage, &BlendOptions::default()).unwrap();
    let blend_time = start.elapsed();
    eprintln!(
        "3x24 MP panorama blend: {blend_time:?}; total {:?}",
        align_time + blend_time
    );
    assert!(out.image.pixels.len() >= 24_000_000);
    assert!(out.image.pixels.iter().flatten().all(|v| v.is_finite()));
    assert!(
        (align_time + blend_time).as_secs_f64() < 6.,
        "M5-24 CPU target <6s missed: {:?}",
        align_time + blend_time
    );
}

#[test]
fn untextured_registration_is_not_reported_as_identity_success() {
    let mut a = image(64, 64);
    a.pixels.fill([0.5; 3]);
    assert!(align_layers(&[a.clone(), a], &AlignOptions::default()).is_err());
}
#[test]
fn reposition_translation_has_subpixel_error_and_no_rotation() {
    let a = image(96, 80);
    let mut b = a.clone();
    for y in 0..80 {
        for x in 0..96 {
            b.pixels[y * 96 + x] = a.sample(x as f64 - 4.25, y as f64 + 2.3).unwrap_or([0.; 3]);
        }
    }
    let out = align_layers(
        &[a, b],
        &AlignOptions {
            mode: AlignMode::Reposition,
            seed: 0,
            ..Default::default()
        },
    )
    .unwrap();
    let transform::Operation::Free(t) = &out.transforms[1].operation else {
        panic!()
    };
    assert_eq!(t.matrix[0][0], 1.);
    assert_eq!(t.matrix[0][1], 0.);
    assert!((t.matrix[0][2] + out.origin[0] + 4.25).abs() < 0.5);
    assert!((t.matrix[1][2] + out.origin[1] - 2.3).abs() < 0.5);
}
