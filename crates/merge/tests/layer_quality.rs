use merge::{LinearImage, layers::*};
fn scene(x: f64, y: f64) -> [f32; 3] {
    let v = 0.5
        + 0.12 * (x * 0.17 + y * 0.09).sin()
        + 0.1 * (x * 0.07 - y * 0.21).cos()
        + 0.12 * ((x * 0.039).sin() * 7. + (y * 0.051).cos() * 9.).sin();
    [v as f32, (v * 0.8) as f32, (v * 0.6) as f32]
}
fn crop(offset: f64, angle: f64, scale: f64) -> LinearImage {
    let (s, c) = angle.sin_cos();
    LinearImage {
        width: 240,
        height: 180,
        pixels: (0..240 * 180)
            .map(|i| {
                let x = (i % 240) as f64;
                let y = (i / 240) as f64;
                scene(scale * (c * x - s * y) + offset, scale * (s * x + c * y))
            })
            .collect(),
        color_matrix: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
        as_shot_neutral: [1.; 3],
    }
}
#[test]
fn three_sequential_similarity_crops_and_seam() {
    let truth = [(0., 0., 1.), (140., 0.018, 1.025), (280., -0.012, 0.98)];
    let ims: Vec<_> = truth.iter().map(|&(t, a, s)| crop(t, a, s)).collect();
    let automatic = align_layers(&ims, &AlignOptions::default()).unwrap();
    let transform::Operation::Free(last) = &automatic.transforms[2].operation else {
        panic!()
    };
    let point = last.map([120.5, 90.5]).unwrap();
    let (sin, cos) = truth[2].1.sin_cos();
    assert!(
        (point[0] + automatic.origin[0]
            - 0.5
            - (truth[2].2 * (cos * 120. - sin * 90.) + truth[2].0))
            .hypot(point[1] + automatic.origin[1] - 0.5 - truth[2].2 * (sin * 120. + cos * 90.))
            < 0.5
    );
    let out = align_layers(
        &ims,
        &AlignOptions {
            mode: AlignMode::Collage,
            ..Default::default()
        },
    )
    .unwrap();
    for (k, &(t, a, scale)) in truth.iter().enumerate() {
        let transform::Operation::Free(h) = &out.transforms[k].operation else {
            panic!()
        };
        let mut e = 0.;
        let (s, c) = a.sin_cos();
        for (x, y) in [(30., 30.), (200., 30.), (30., 150.), (200., 150.)] {
            let p = h.map([x + 0.5, y + 0.5]).unwrap();
            e += (p[0] + out.origin[0] - 0.5 - scale * (c * x - s * y) - t).powi(2)
                + (p[1] + out.origin[1] - 0.5 - scale * (s * x + c * y)).powi(2);
        }
        eprintln!("source {k} RMS {}", (e / 4.).sqrt());
        assert!((e / 4.).sqrt() < 0.5, "source {k} RMS {}", (e / 4.).sqrt());
        assert!((h.matrix[0][0] - h.matrix[1][1]).abs() < 1e-9);
        assert!((h.matrix[0][1] + h.matrix[1][0]).abs() < 1e-9);
    }
    let reverse = align_layers(
        &ims,
        &AlignOptions {
            mode: AlignMode::Collage,
            reference: 2,
            ..Default::default()
        },
    )
    .unwrap();
    let transform::Operation::Free(t) = &reverse.transforms[0].operation else {
        panic!()
    };
    let p = t.map([120.5, 90.5]).unwrap();
    let (s, c) = truth[2].1.sin_cos();
    let x = p[0] + reverse.origin[0] - 0.5;
    let y = p[1] + reverse.origin[1] - 0.5;
    assert!(
        (truth[2].2 * (c * x - s * y) + truth[2].0 - 120.)
            .hypot(truth[2].2 * (s * x + c * y) - 90.)
            < 0.5,
        "nonzero reference chain"
    );
    let blend = blend_layers(&out.images, &out.coverage, &BlendOptions::default()).unwrap();
    let mut error = 0.;
    let mut n = 0;
    for y in 15..out.height - 15 {
        for x in 15..out.width - 15 {
            let i = y * out.width + x;
            if out.coverage.iter().filter(|c| c[i]).count() > 1 {
                let p = scene(x as f64 + out.origin[0], y as f64 + out.origin[1]);
                error += (p[0] - blend.image.pixels[i][0]).abs() as f64;
                n += 1;
            }
        }
    }
    let mut seam_error = 0.;
    let mut seam_count = 0;
    for y in 15..out.height - 15 {
        for x in 16..out.width - 15 {
            let i = y * out.width + x;
            if blend.coverage[i]
                && blend.coverage[i - 1]
                && blend.masks.iter().any(|m| m[i] != m[i - 1])
            {
                let a = scene(x as f64 + out.origin[0], y as f64 + out.origin[1])[0];
                let b = scene(x as f64 - 1. + out.origin[0], y as f64 + out.origin[1])[0];
                seam_error += ((blend.image.pixels[i][0] - blend.image.pixels[i - 1][0]) - (a - b))
                    .abs() as f64;
                seam_count += 1;
            }
        }
    }
    assert!(seam_count > 0);
    eprintln!(
        "ownership-boundary gradient MAE {}",
        seam_error / seam_count as f64
    );
    assert!(seam_error / (seam_count as f64) < 0.025);
    eprintln!("overlap MAE {}", error / n as f64);
    assert!(n > 100);
    assert!(
        error / (n as f64) < 0.025,
        "overlap MAE {}",
        error / n as f64
    );
}
#[test]
fn three_source_multiscale_focus_rejects_fine_noise() {
    let (w, h) = (300, 120);
    let mut ims = vec![crop(0., 0., 1.); 3];
    for (k, im) in ims.iter_mut().enumerate() {
        im.width = w;
        im.height = h;
        im.pixels = (0..w * h)
            .map(|i| {
                let x = i % w;
                let y = i / w;
                let sharp = x / 100 == k;
                let v = 0.5
                    + if sharp {
                        0.25 * ((x as f64 * 0.11).sin() + (y as f64 * 0.13).cos())
                    } else {
                        0.004 * if (x + y) % 2 == 0 { 1. } else { -1. }
                    };
                [v as f32; 3]
            })
            .collect();
    }
    let out = blend_layers(
        &ims,
        &vec![vec![true; w * h]; 3],
        &BlendOptions {
            mode: BlendMode::StackImages,
            seamless_tones: false,
            ..Default::default()
        },
    )
    .unwrap();
    let correct = (0..w * h)
        .filter(|i| out.masks[(i % w) / 100][*i] > 0.5)
        .count();
    eprintln!(
        "noise-resistant multiscale focus accuracy {}",
        correct as f64 / (w * h) as f64
    );
    assert!(
        correct as f64 / (w * h) as f64 >= 0.95,
        "accuracy {}",
        correct as f64 / (w * h) as f64
    );
}
#[test]
fn three_gaussian_blurred_regions_select_sharpest_source() {
    let (w, h) = (600, 120);
    let mut ims = vec![crop(0., 0., 1.); 3];
    for (k, im) in ims.iter_mut().enumerate() {
        im.width = w;
        im.height = h;
        // Gaussian convolution of a sinusoid is analytic: exp(-sigma² omega²/2).
        im.pixels = (0..w * h)
            .map(|i| {
                let x = i % w;
                let y = i / w;
                let region = x / 200;
                let sigma = if k == region {
                    0.
                } else if (k + 1) % 3 == region {
                    2.
                } else {
                    4.
                };
                let v = 0.5
                    + 0.2 * (-0.5_f64 * sigma * sigma * 0.7 * 0.7).exp() * (x as f64 * 0.7).sin()
                    + 0.2
                        * (-0.5_f64 * sigma * sigma * 0.55 * 0.55).exp()
                        * (y as f64 * 0.55).cos();
                [v as f32; 3]
            })
            .collect();
    }
    let out = blend_layers(
        &ims,
        &vec![vec![true; w * h]; 3],
        &BlendOptions {
            mode: BlendMode::StackImages,
            seamless_tones: false,
            ..Default::default()
        },
    )
    .unwrap();
    let correct = (0..w * h)
        .filter(|i| out.masks[(i % w) / 200][*i] > 0.5)
        .count();
    eprintln!(
        "three Gaussian-blurred focus accuracy {}",
        correct as f64 / (w * h) as f64
    );
    assert!(correct as f64 / (w * h) as f64 >= 0.95);
}
#[test]
fn padded_smart_object_canvas_does_not_rescale_mesh_source() {
    let mut im = crop(0., 0., 1.);
    im.width = 32;
    im.height = 24;
    im.pixels.truncate(32 * 24);
    let aligned = align_layers(
        std::slice::from_ref(&im),
        &AlignOptions {
            mode: AlignMode::Spherical,
            ..Default::default()
        },
    )
    .unwrap();
    let input = transform::Image::new(
        48,
        40,
        std::array::from_fn(|c| {
            (0..48 * 40)
                .map(|i| {
                    let x = i % 48;
                    let y = i / 48;
                    if x < 32 && y < 24 {
                        if c < 3 { im.pixels[y * 32 + x][c] } else { 1. }
                    } else {
                        0.
                    }
                })
                .collect()
        }),
    )
    .unwrap();
    let rendered = aligned.transforms[0]
        .apply(&input, aligned.width, aligned.height, 0)
        .unwrap();
    for i in 0..aligned.width * aligned.height {
        if aligned.coverage[0][i] {
            for c in 0..3 {
                assert!(
                    (rendered.planes[c][i] / rendered.planes[3][i]
                        - aligned.images[0].pixels[i][c])
                        .abs()
                        < 1e-5
                );
            }
        }
    }
}
#[test]
fn nonlinear_mesh_accuracy_union_and_original_source_extent() {
    for mode in [AlignMode::Cylindrical, AlignMode::Spherical] {
        let im = crop(0., 0., 1.);
        let out = align_layers(
            &[im],
            &AlignOptions {
                mode,
                ..Default::default()
            },
        )
        .unwrap();
        let transform::Operation::Warp(m) = &out.transforms[0].operation else {
            panic!()
        };
        assert_eq!((m.width, m.height), (240., 180.));
        for y in 0..=20 {
            for x in 0..=20 {
                let u = x as f64 / 20.;
                let v = y as f64 / 20.;
                let xx = u - 0.5;
                let yy = (v * 180. - 90.) / 240.;
                let r = (1. + xx * xx).sqrt();
                let expected = [
                    120. + 240. * xx.atan(),
                    90. + 240.
                        * if mode == AlignMode::Spherical {
                            (yy / r).atan()
                        } else {
                            yy / r
                        },
                ];
                let p = m.forward([u, v]);
                assert!(
                    (p[0] + out.origin[0] - expected[0]).hypot(p[1] + out.origin[1] - expected[1])
                        < 0.06
                );
                assert!(
                    p[0] >= 0.
                        && p[1] >= 0.
                        && p[0] <= out.width as f64
                        && p[1] <= out.height as f64
                );
            }
        }
    }
}
