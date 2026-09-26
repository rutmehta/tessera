use merge::{LinearImage, layers::*};

#[test]
fn three_calibrated_sources_register_before_composing_lens_warps() {
    let (w, h) = (160, 120);
    let ks = [-0.035, 0.06, 0.025];
    let offsets = [0., 25., 50.];
    let images: Vec<_> = ks
        .iter()
        .zip(offsets)
        .map(|(&k, offset)| {
            let pixels = (0..w * h)
                .map(|i| {
                    let observed = [
                        2. * (i % w) as f64 / w as f64 + 1. / w as f64 - 1.,
                        2. * (i / w) as f64 / h as f64 + 1. / h as f64 - 1.,
                    ];
                    let radius = observed[0].hypot(observed[1]);
                    // Independent monotone scalar inversion for synthetic source data.
                    let (mut lo, mut hi) = (0., 2.);
                    for _ in 0..60 {
                        let mid = (lo + hi) / 2.;
                        if mid * (1. + k * mid * mid) < radius {
                            lo = mid;
                        } else {
                            hi = mid;
                        }
                    }
                    let scale = (lo + hi) / (2. * radius.max(1e-12));
                    let x = (observed[0] * scale + 1.) * w as f64 / 2. - 0.5 + offset;
                    let y = (observed[1] * scale + 1.) * h as f64 / 2. - 0.5;
                    let v = 0.5
                        + 0.12 * (x * 0.17 + y * 0.09).sin()
                        + 0.1 * (x * 0.07 - y * 0.21).cos()
                        + 0.12 * ((x * 0.039).sin() * 7. + (y * 0.051).cos() * 9.).sin();
                    [(v * (1. - 0.15 * radius * radius)) as f32; 3]
                })
                .collect();
            LinearImage {
                width: w,
                height: h,
                pixels,
                color_matrix: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
                as_shot_neutral: [1.; 3],
            }
        })
        .collect();
    let options = AlignOptions {
        mode: AlignMode::Collage,
        geometric_distortion: true,
        vignette_removal: true,
        lens_corrections: ks
            .iter()
            .map(|k| LensCorrection {
                distortion: [*k, 0., 0.],
                vignette: [-0.15, 0., 0.],
            })
            .collect(),
        ..Default::default()
    };
    let out = align_layers(&images, &options).unwrap();
    for (i, op) in out.transforms.iter().enumerate() {
        let transform::Operation::Warp(mesh) = &op.operation else {
            panic!()
        };
        let mut squared = 0.;
        for q in [[-0.5, -0.5], [0.5, -0.5], [-0.5, 0.5], [0.5, 0.5]] {
            let radial = 1. + ks[i] * (q[0] * q[0] + q[1] * q[1]);
            let actual = mesh.forward([(q[0] * radial + 1.) / 2., (q[1] * radial + 1.) / 2.]);
            let expected = [
                (q[0] + 1.) * w as f64 / 2. + offsets[i],
                (q[1] + 1.) * h as f64 / 2.,
            ];
            squared += (actual[0] + out.origin[0] - expected[0]).powi(2)
                + (actual[1] + out.origin[1] - expected[1]).powi(2);
        }
        let rms = (squared / 4.).sqrt();
        eprintln!("calibrated source {i} RMS {rms}");
        assert!(rms < 0.5);
    }
    let repeat = align_layers(&images, &options).unwrap();
    assert_eq!(out.transforms, repeat.transforms);
    assert_eq!(out.images[2].pixels, repeat.images[2].pixels);
}
