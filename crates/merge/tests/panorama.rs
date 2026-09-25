use merge::{
    LinearImage,
    pano::{PanoramaOptions, Projection, panorama},
};
fn texture(x: f64, y: f64) -> [f32; 3] {
    let v = 0.45
        + 0.13 * (x * 0.17 + y * 0.13).sin()
        + 0.12 * (x * 0.31 - y * 0.23).cos()
        + 0.10 * (x * 0.067 + y * 0.091).sin()
        + 0.08 * ((x * 0.11).sin() * 7. + y * 0.19).cos();
    [
        v as f32,
        (v * 0.8 + 0.06 * (x * 0.08).sin()) as f32,
        (v * 0.7 + 0.07 * (y * 0.09).cos()) as f32,
    ]
}
fn image(offset: f64, p: f64) -> LinearImage {
    let (w, h) = (240, 160);
    let mut pixels = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let d = 1. + p * x as f64 + 0.00003 * y as f64;
            pixels.push(texture(
                (x as f64 + offset) / d,
                (y as f64 + 3. * offset / 100.) / d,
            ));
        }
    }
    LinearImage {
        width: w,
        height: h,
        pixels,
        color_matrix: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
        as_shot_neutral: [1.; 3],
    }
}
#[test]
fn three_projective_views_recover_scene() {
    let images = vec![image(0., 0.), image(85., 0.00012), image(170., 0.00021)];
    let result = panorama(
        &images,
        &PanoramaOptions {
            auto_crop: false,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(result.transforms.len(), 3);
    let mut geometry_error = 0.0_f64;
    for (i, (offset, perspective)) in [(85., 0.00012), (170., 0.00021)].into_iter().enumerate() {
        let h = result.transforms[i + 1];
        assert!(h[2][0].abs() > 0.00001);
        for (x, y) in [(20., 20.), (120., 80.), (220., 140.)] {
            let d = 1. + perspective * x + 0.00003 * y;
            let world_x = (x + offset) / d;
            let world_y = (y + 3. * offset / 100.) / d;
            let first_d = 1. - 0.00003 * world_y;
            let estimated_d = h[2][0] * x + h[2][1] * y + h[2][2];
            let estimated_x = (h[0][0] * x + h[0][1] * y + h[0][2]) / estimated_d;
            let estimated_y = (h[1][0] * x + h[1][1] * y + h[1][2]) / estimated_d;
            geometry_error = geometry_error
                .max((estimated_x - world_x / first_d).hypot(estimated_y - world_y / first_d));
        }
    }
    eprintln!("panorama maximum sampled transform error: {geometry_error:.4} pixels");
    assert!(geometry_error < 0.2);
    let mut mse = 0.;
    let mut n = 0;
    for y in 8..result.image.height.saturating_sub(8) {
        for x in 8..result.image.width.saturating_sub(8) {
            let gx = x as f64 + result.origin[0];
            let gy = y as f64 + result.origin[1];
            if !(100. ..310.).contains(&gx)
                || !(15. ..135.).contains(&gy)
                || !result.coverage[y * result.image.width + x]
            {
                continue;
            }
            // First view defines the coordinate system and has a nonzero y projective term.
            let d = 1. + 0.00003 * gy;
            let truth = texture(gx / d, gy / d);
            for (a, b) in result.image.pixels[y * result.image.width + x]
                .iter()
                .zip(truth)
            {
                mse += (*a as f64 - b as f64).powi(2);
                n += 1;
            }
        }
    }
    assert!(n > 10000);
    let psnr = -10. * (mse / n as f64).log10();
    eprintln!("panorama overlap PSNR: {psnr:.3} dB");
    assert!(psnr > 30., "PSNR {psnr}");
    assert!(result.transforms[1][2][0].abs() > 0.00001);
}
#[test]
fn rejects_bad_inputs() {
    let o = PanoramaOptions::default();
    assert!(panorama(&[], &o).is_err());
    let mut a = image(0., 0.);
    let mut b = a.clone();
    b.as_shot_neutral[0] = 2.;
    assert!(panorama(&[a.clone(), b], &o).is_err());
    a.pixels.fill([0.5; 3]);
    assert!(panorama(&[a.clone(), a], &o).is_err());
    let a = image(0., 0.);
    assert!(
        panorama(
            &[a],
            &PanoramaOptions {
                focal_pixels: 0.,
                ..o
            }
        )
        .is_err()
    );
}
#[test]
fn projections_and_crop_are_real() {
    let a = image(0., 0.);
    let mut outputs = Vec::new();
    for projection in [
        Projection::Perspective,
        Projection::Cylindrical,
        Projection::Spherical,
    ] {
        let r = panorama(
            std::slice::from_ref(&a),
            &PanoramaOptions {
                projection,
                focal_pixels: 100.,
                auto_crop: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(r.coverage.iter().all(|v| *v));
        outputs.push((r.image.width, r.image.height));
    }
    assert_ne!(outputs[0], outputs[1]);
    assert_ne!(outputs[1], outputs[2]);
}

#[test]
fn fill_preserves_measured_coverage_and_metadata() {
    let a = image(0., 0.);
    let o = PanoramaOptions {
        projection: Projection::Spherical,
        focal_pixels: 90.,
        auto_crop: false,
        ..Default::default()
    };
    let empty = panorama(std::slice::from_ref(&a), &o).unwrap();
    let filled = panorama(
        std::slice::from_ref(&a),
        &PanoramaOptions {
            fill_edges: true,
            ..o
        },
    )
    .unwrap();
    assert_eq!(empty.coverage, filled.coverage);
    assert!(empty.coverage.iter().any(|v| !*v));
    assert_eq!(filled.image.color_matrix, a.color_matrix);
    assert_eq!(filled.image.as_shot_neutral, a.as_shot_neutral);
    for (i, covered) in empty.coverage.iter().enumerate() {
        if !covered {
            assert_eq!(empty.image.pixels[i], [0.; 3]);
            assert!(filled.image.pixels[i][0] > 0.);
        } else {
            assert_eq!(empty.image.pixels[i], filled.image.pixels[i]);
        }
    }
}

#[test]
fn malformed_and_disconnected_images_are_rejected() {
    let o = PanoramaOptions::default();
    let a = image(0., 0.);
    let mut b = a.clone();
    b.pixels.pop();
    assert!(panorama(&[b], &o).is_err());
    let mut b = a.clone();
    b.pixels[0][0] = f32::NAN;
    assert!(panorama(&[b], &o).is_err());
    let mut b = a.clone();
    let mut seed = 571u64;
    for p in &mut b.pixels {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        *p = [(seed % 1000) as f32 / 1000.; 3];
    }
    assert!(panorama(&[a, b], &o).is_err());
}
