use merge::{LinearImage, layers::*};

#[test]
fn radial_distortion_is_composed_into_editable_geometry() {
    for k1 in [-0.04, 0.08] {
        for mode in [
            AlignMode::Perspective,
            AlignMode::Cylindrical,
            AlignMode::Spherical,
        ] {
            let options = AlignOptions {
                mode,
                geometric_distortion: true,
                lens_corrections: vec![LensCorrection {
                    distortion: [k1, 0., 0.],
                    ..Default::default()
                }],
                ..Default::default()
            };
            let out = align_layers(&[flat(48, 40)], &options).unwrap();
            let transform::Operation::Warp(mesh) = &out.transforms[0].operation else {
                panic!()
            };
            for y in 0..=12 {
                for x in 0..=12 {
                    let q = [x as f64 / 10. - 0.6, y as f64 / 10. - 0.6];
                    let s = 1. + k1 * (q[0] * q[0] + q[1] * q[1]);
                    let actual = mesh.forward([(q[0] * s + 1.) / 2., (q[1] * s + 1.) / 2.]);
                    let mut expected = [(q[0] + 1.) * 24., (q[1] + 1.) * 20.];
                    if mode != AlignMode::Perspective {
                        let nx = (expected[0] - 24.) / 48.;
                        let ny = (expected[1] - 20.) / 48.;
                        let r = (1. + nx * nx).sqrt();
                        expected = [
                            24. + 48. * nx.atan(),
                            20. + 48.
                                * if mode == AlignMode::Spherical {
                                    (ny / r).atan()
                                } else {
                                    ny / r
                                },
                        ];
                    }
                    assert!(
                        (actual[0] + out.origin[0] - expected[0])
                            .hypot(actual[1] + out.origin[1] - expected[1])
                            < 0.06
                    );
                }
            }
            let repeat = align_layers(&[flat(48, 40)], &options).unwrap();
            assert_eq!(out.transforms, repeat.transforms);
        }
    }
}

fn flat(w: usize, h: usize) -> LinearImage {
    LinearImage {
        width: w,
        height: h,
        pixels: vec![[0.4, 0.6, 0.8]; w * h],
        color_matrix: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
        as_shot_neutral: [1.; 3],
    }
}

#[test]
fn calibrated_vignette_removal_restores_linear_rgb() {
    let mut image = flat(48, 40);
    for y in 0..40 {
        for x in 0..48 {
            let r2 = (2. * (x as f64 + 0.5) / 48. - 1.).powi(2)
                + (2. * (y as f64 + 0.5) / 40. - 1.).powi(2);
            let illumination = (1. - 0.2 * r2 + 0.02 * r2 * r2) as f32;
            for c in &mut image.pixels[y * 48 + x] {
                *c *= illumination;
            }
        }
    }
    let original = image.clone();
    let options = AlignOptions {
        vignette_removal: true,
        lens_corrections: vec![LensCorrection {
            vignette: [-0.2, 0.02, 0.],
            ..Default::default()
        }],
        ..Default::default()
    };
    let output = align_layers(std::slice::from_ref(&image), &options).unwrap();
    assert_eq!(image.pixels, original.pixels);
    for pixel in &output.images[0].pixels {
        for (actual, expected) in pixel.iter().zip([0.4, 0.6, 0.8]) {
            assert!((actual - expected).abs() < 2e-6);
        }
    }
}
