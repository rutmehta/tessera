mod support;
use merge::{
    LinearImage,
    pano::{PanoramaOptions, Projection, panorama, panorama_with_fill},
};

fn image(w: usize, h: usize) -> LinearImage {
    LinearImage {
        width: w,
        height: h,
        pixels: (0..w * h)
            .map(|i| [i as f32 / (w * h) as f32, (i % w) as f32 / w as f32, 2.])
            .collect(),
        color_matrix: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
        as_shot_neutral: [1.; 3],
    }
}

#[test]
fn boundary_warp_expands_mesh_without_cropping() {
    let im = image(121, 81);
    let mut results = Vec::new();
    for amount in [0, 50, 100] {
        results.push(
            panorama(
                std::slice::from_ref(&im),
                &PanoramaOptions {
                    projection: Projection::Spherical,
                    focal_pixels: 50.,
                    auto_crop: false,
                    boundary_warp: amount,
                    ..Default::default()
                },
            )
            .unwrap(),
        );
    }
    let sizes: Vec<_> = results
        .iter()
        .map(|r| (r.image.width, r.image.height, r.origin))
        .collect();
    assert_eq!(sizes[0], sizes[1]);
    assert_eq!(sizes[1], sizes[2]);
    let counts: Vec<_> = results
        .iter()
        .map(|r| r.coverage.iter().filter(|v| **v).count())
        .collect();
    assert!(counts[0] < counts[1] && counts[1] < counts[2], "{counts:?}");
    assert!(results[2].coverage.iter().all(|v| *v));
    let moved = results[0]
        .coverage
        .iter()
        .enumerate()
        .filter(|(i, c)| {
            **c && (results[0].image.pixels[*i][1] - results[2].image.pixels[*i][1]).abs() > 0.01
        })
        .count();
    assert!(
        moved > 100,
        "interior content must move, not just extend borders"
    );
    assert_eq!(results[2].image.color_matrix, im.color_matrix);
    assert!(
        results[2]
            .image
            .pixels
            .iter()
            .all(|p| (p[2] - 2.).abs() < 1e-5)
    );
    assert!(
        panorama(
            &[im],
            &PanoramaOptions {
                boundary_warp: 101,
                ..Default::default()
            }
        )
        .is_err()
    );
}

#[test]
fn edge_fill_matches_real_patchmatch_on_linear_hdr_samples() {
    use compositor::{Depth, Raster, Rect};
    use engine_api::tile::Extent;
    use filters::caf::{FillParams, fill};
    use std::sync::atomic::AtomicBool;
    let im = image(61, 41);
    let options = PanoramaOptions {
        projection: Projection::Spherical,
        focal_pixels: 25.,
        auto_crop: false,
        ..Default::default()
    };
    let empty = panorama(std::slice::from_ref(&im), &options).unwrap();
    let (w, h) = (empty.image.width, empty.image.height);
    let mut raster = Raster::new(
        Extent {
            width: w as u32,
            height: h as u32,
        },
        4,
        Depth::F32,
        0.,
    );
    raster
        .edit_region(Rect::new(0, 0, w as i64, h as i64), 1, |x, y, p| {
            let c = empty.image.pixels[y as usize * w + x as usize];
            *p = [c[0], c[1], c[2], 1.];
        })
        .unwrap();
    let mask: Vec<_> = empty
        .coverage
        .iter()
        .map(|v| if *v { 0. } else { 1. })
        .collect();
    let expected = fill(
        &raster,
        &mask,
        &FillParams::default(),
        &AtomicBool::new(false),
    )
    .unwrap();
    let actual = panorama_with_fill(
        &[im],
        &PanoramaOptions {
            fill_edges: true,
            ..options
        },
        support::caf,
    )
    .unwrap();
    assert_eq!(actual.coverage, empty.coverage);
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            assert_eq!(
                actual.image.pixels[i],
                expected.composite.pixel(x as u32, y as u32)[..3],
                "pixel {x},{y}"
            );
        }
    }
}

#[test]
fn auto_thresholds_are_angular_not_aspect_ratio() {
    for (horizontal, vertical, expected) in [
        (99.99_f64, 30.0_f64, Projection::Perspective),
        (100.01, 30., Projection::Cylindrical),
        (40., 79.99, Projection::Perspective),
        (40., 80.01, Projection::Spherical),
        (110., 90., Projection::Spherical),
    ] {
        // Keep the tested span exact by varying focal length rather than
        // rounding dimensions. Odd dimensions include the central ray.
        let (w, h, f) = if vertical == 30. {
            (201, 21, 100. / (horizontal.to_radians() / 2.).tan())
        } else if horizontal > 100. {
            (301, 201, 100. / (vertical.to_radians() / 2.).tan())
        } else {
            (21, 201, 100. / (vertical.to_radians() / 2.).tan())
        };
        let im = image(w, h);
        let result = panorama(
            &[im],
            &PanoramaOptions {
                projection: Projection::Auto,
                focal_pixels: f,
                auto_crop: false,
                pyramid_levels: 1,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(result.projection, expected, "{horizontal} / {vertical}");
    }
}

#[test]
fn fill_adapter_contract_and_crop_precedence() {
    let im = image(61, 41);
    let options = PanoramaOptions {
        projection: Projection::Spherical,
        focal_pixels: 25.,
        auto_crop: false,
        fill_edges: true,
        ..Default::default()
    };
    assert!(
        panorama(std::slice::from_ref(&im), &options)
            .unwrap_err()
            .contains("adapter")
    );
    for bad in [vec![], vec![[f32::NAN; 3]; 100]] {
        assert!(panorama_with_fill(std::slice::from_ref(&im), &options, |_, _| Ok(bad)).is_err());
    }
    let err = panorama_with_fill(std::slice::from_ref(&im), &options, |_, _| {
        Err("CAF donor error".into())
    })
    .unwrap_err();
    assert_eq!(err, "CAF donor error");
    for opts in [
        PanoramaOptions {
            auto_crop: true,
            ..options.clone()
        },
        PanoramaOptions {
            boundary_warp: 100,
            ..options
        },
    ] {
        let result = panorama_with_fill(std::slice::from_ref(&im), &opts, |_, _| {
            panic!("no holes need synthesis")
        })
        .unwrap();
        assert!(result.coverage.iter().all(|v| *v));
    }
}

#[test]
fn rectangular_mesh_is_identity_at_all_strengths() {
    let im = image(31, 21);
    let reference = panorama(std::slice::from_ref(&im), &PanoramaOptions::default()).unwrap();
    for amount in [0, 25, 50, 100] {
        let result = panorama(
            std::slice::from_ref(&im),
            &PanoramaOptions {
                boundary_warp: amount,
                ..Default::default()
            },
        )
        .unwrap();
        for (a, b) in reference.image.pixels.iter().zip(result.image.pixels) {
            assert!(a.iter().zip(b).all(|(a, b)| (*a - b).abs() < 1e-6));
        }
    }
}

#[test]
fn auto_projection_uses_both_angular_spans() {
    for (w, h, f, expected) in [
        (101, 61, 100., Projection::Perspective),
        (301, 61, 100., Projection::Cylindrical),
        (101, 301, 100., Projection::Spherical),
    ] {
        let im = image(w, h);
        let options = PanoramaOptions {
            projection: Projection::Auto,
            focal_pixels: f,
            auto_crop: false,
            ..Default::default()
        };
        let auto = panorama(std::slice::from_ref(&im), &options).unwrap();
        let explicit = panorama(
            &[im],
            &PanoramaOptions {
                projection: expected,
                ..options
            },
        )
        .unwrap();
        assert_eq!(auto.projection, expected);
        assert_eq!(auto.image.pixels, explicit.image.pixels);
        assert_eq!(auto.coverage, explicit.coverage);
    }
}
