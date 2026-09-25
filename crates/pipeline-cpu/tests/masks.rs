use engine_api::recipe::mask::*;
use pipeline_cpu::Image;
use pipeline_cpu::masks::{GuidedRefinement, MaskOptions, rasterize};
#[test]
fn composition_and_inversion() {
    let i = image(2, 1);
    for (op, expected) in [
        (MaskCombine::Add, vec![0.75, 0.75]),
        (MaskCombine::Subtract, vec![0.5625, 0.0625]),
        (MaskCombine::Intersect, vec![0.1875, 0.1875]),
    ] {
        let mut g = group(linear());
        g.components[0].combine = MaskCombine::Subtract;
        g.components.push(MaskComponent {
            kind: linear(),
            combine: op,
            invert: true,
        });
        close(
            &rasterize(&i, &g, MaskOptions::default()).unwrap(),
            &expected,
        );
        g.invert = true;
        close(
            &rasterize(&i, &g, MaskOptions::default()).unwrap(),
            &expected.iter().map(|v| 1. - v).collect::<Vec<_>>(),
        );
    }
}
#[test]
fn radial_feather_and_rotation() {
    let i = image(5, 5);
    let g = group(MaskKind::Radial {
        center: [0.5, 0.5],
        radii: [0.4, 0.2],
        angle: 0.,
        feather: 100.,
    });
    let m = rasterize(&i, &g, MaskOptions::default()).unwrap();
    close(&[m[12], m[13], m[14], m[17]], &[1., 0.5, 0., 0.]);
    let g = group(MaskKind::Radial {
        center: [0.5, 0.5],
        radii: [0.4, 0.2],
        angle: 90.,
        feather: 100.,
    });
    let m = rasterize(&i, &g, MaskOptions::default()).unwrap();
    close(&[m[12], m[13], m[17]], &[1., 0., 0.5]);
}
#[test]
fn brush_stamps_accumulate_pressure_and_erase() {
    let i = image(5, 5);
    let s = BrushStroke {
        points: vec![[0.5, 0.5, 0.5]],
        radius: 0.4,
        feather: 100.,
        flow: 50.,
        erase: false,
    };
    let m = rasterize(
        &i,
        &group(MaskKind::Brush {
            strokes: vec![s.clone(), s.clone()],
        }),
        MaskOptions::default(),
    )
    .unwrap();
    close(&[m[12], m[13], m[14]], &[0.4375, 0.234375, 0.]);
    let mut eraser = s.clone();
    eraser.erase = true;
    let m = rasterize(
        &i,
        &group(MaskKind::Brush {
            strokes: vec![s, eraser],
        }),
        MaskOptions::default(),
    )
    .unwrap();
    close(&[m[12]], &[0.1875]);
}
#[test]
fn brush_interpolates_path_in_pixel_metric() {
    let i = image(21, 5);
    let s = BrushStroke {
        points: vec![[0.1, 0.5, 1.], [0.9, 0.5, 1.]],
        radius: 0.06,
        feather: 0.,
        flow: 100.,
        erase: false,
    };
    let m = rasterize(
        &i,
        &group(MaskKind::Brush { strokes: vec![s] }),
        MaskOptions::default(),
    )
    .unwrap();
    assert!(m[44..61].iter().all(|&v| v == 1.));
    assert_eq!(m[10], 0.);
}
#[test]
fn luminance_and_depth_ranges() {
    let values = vec![0., 0.25, 0.5, 0.75, 1.];
    let i = Image::new(5, 1, vec![values.clone(); 3]).unwrap();
    let g = group(MaskKind::LuminanceRange {
        range: [0.5, 0.75],
        smoothness: 100.,
    });
    close(
        &rasterize(&i, &g, MaskOptions::default()).unwrap(),
        &[0., 0.5, 1., 1., 0.5],
    );
    let g = group(MaskKind::Depth {
        range: [0.25, 0.75],
        feather: 0.,
        model: None,
    });
    close(
        &rasterize(
            &i,
            &g,
            MaskOptions {
                depth: Some(&values),
                ..Default::default()
            },
        )
        .unwrap(),
        &[0., 1., 1., 1., 0.],
    );
    assert!(rasterize(&i, &g, MaskOptions::default()).is_err());
}
#[test]
fn color_range_oklab_distance_and_smoothness() {
    let i = Image::new(3, 1, vec![vec![0., 0.125, 1.]; 3]).unwrap();
    let g = group(MaskKind::ColorRange {
        samples: vec![[0., 0., 0.]],
        amount: 100.,
    });
    close(
        &rasterize(
            &i,
            &g,
            MaskOptions {
                color_smoothness: 100.,
                ..Default::default()
            },
        )
        .unwrap(),
        &[1., 0.5, 0.],
    );
    close(
        &rasterize(
            &i,
            &g,
            MaskOptions {
                color_smoothness: 0.,
                ..Default::default()
            },
        )
        .unwrap()[..2],
        &[1., 1.],
    );
    let g = group(MaskKind::ColorRange {
        samples: vec![[0., 0., 0.], [1., 0., 0.]],
        amount: 10.,
    });
    close(
        &rasterize(&i, &g, MaskOptions::default()).unwrap(),
        &[1., 0., 1.],
    );
    close(
        &rasterize(
            &i,
            &group(MaskKind::ColorRange {
                samples: vec![],
                amount: 100.,
            }),
            MaskOptions::default(),
        )
        .unwrap(),
        &[0.; 3],
    );
}
#[test]
fn guided_refinement_smooths_flat_guide_but_preserves_edges() {
    let flat = image(9, 1);
    let values = vec![0., 0., 0., 0., 1., 1., 1., 1., 1.];
    let edge = Image::new(9, 1, vec![values.clone(); 3]).unwrap();
    let g = group(MaskKind::Depth {
        range: [0.5, 1.],
        feather: 0.,
        model: None,
    });
    let options = MaskOptions {
        depth: Some(&values),
        refinement: Some(GuidedRefinement {
            radius: 2,
            epsilon: 1e-6,
        }),
        ..Default::default()
    };
    let smoothed = rasterize(&flat, &g, options).unwrap();
    let preserved = rasterize(&edge, &g, options).unwrap();
    assert!(smoothed[3] > 0.2 && smoothed[4] < 0.8);
    assert!(preserved[3] < 0.001 && preserved[4] > 0.999);
    let unchanged = rasterize(
        &edge,
        &g,
        MaskOptions {
            refinement: Some(GuidedRefinement {
                radius: 0,
                epsilon: 0.1,
            }),
            ..options
        },
    )
    .unwrap();
    close(&unchanged, &values);
    let constant = rasterize(
        &flat,
        &group(MaskKind::LuminanceRange {
            range: [0., 1.],
            smoothness: 0.,
        }),
        options,
    )
    .unwrap();
    close(&constant, &[1.; 9]);
}
#[test]
fn rejects_invalid_options_and_geometry() {
    let i = image(2, 2);
    let g = group(linear());
    for options in [
        MaskOptions {
            color_smoothness: f32::NAN,
            ..Default::default()
        },
        MaskOptions {
            color_smoothness: 101.,
            ..Default::default()
        },
        MaskOptions {
            depth: Some(&[0.]),
            ..Default::default()
        },
        MaskOptions {
            depth: Some(&[0., 0., f32::NAN, 0.]),
            ..Default::default()
        },
        MaskOptions {
            depth: Some(&[0., 0., 2., 0.]),
            ..Default::default()
        },
        MaskOptions {
            refinement: Some(GuidedRefinement {
                radius: 1,
                epsilon: 0.,
            }),
            ..Default::default()
        },
    ] {
        assert!(rasterize(&i, &g, options).is_err(), "{options:?}");
    }
    assert!(
        rasterize(
            &Image::new(2, 2, vec![vec![0.; 4]]).unwrap(),
            &g,
            MaskOptions::default()
        )
        .is_err()
    );
    for kind in [
        MaskKind::Linear {
            start: [0., 0.],
            end: [0., 0.],
        },
        MaskKind::Linear {
            start: [f32::NAN, 0.],
            end: [1., 0.],
        },
        MaskKind::Radial {
            center: [0.5, 0.5],
            radii: [0., 1.],
            angle: 0.,
            feather: 0.,
        },
        MaskKind::Radial {
            center: [0.5, 0.5],
            radii: [1., 1.],
            angle: f32::INFINITY,
            feather: 0.,
        },
        MaskKind::LuminanceRange {
            range: [0.8, 0.2],
            smoothness: 0.,
        },
        MaskKind::ColorRange {
            samples: vec![[f32::NAN, 0., 0.]],
            amount: 50.,
        },
        MaskKind::Depth {
            range: [-1., 1.],
            feather: 0.,
            model: None,
        },
    ] {
        assert!(rasterize(&i, &group(kind), MaskOptions::default()).is_err());
    }
}
#[test]
fn rejects_unsafe_brush_and_all_unsupported_ai() {
    let i = image(3, 3);
    for s in [
        BrushStroke {
            points: vec![[1e30, 0., 1.]],
            ..Default::default()
        },
        BrushStroke {
            points: vec![[0., 0., -1.]],
            ..Default::default()
        },
        BrushStroke {
            radius: 0.,
            ..Default::default()
        },
        BrushStroke {
            flow: f32::NAN,
            ..Default::default()
        },
        BrushStroke {
            feather: 101.,
            ..Default::default()
        },
    ] {
        assert!(
            rasterize(
                &i,
                &group(MaskKind::Brush { strokes: vec![s] }),
                MaskOptions::default()
            )
            .is_err()
        );
    }
    for kind in [
        MaskKind::Subject { model: None },
        MaskKind::Sky { model: None },
        MaskKind::Background { model: None },
        MaskKind::Object {
            prompt: None,
            region: None,
            points: vec![],
            model: None,
        },
        MaskKind::Landscape {
            class: LandscapeClass::Sky,
            model: None,
        },
        MaskKind::Person {
            person: engine_api::id::PersonId(0),
            parts: vec![],
            model: None,
        },
    ] {
        assert!(rasterize(&i, &group(kind), MaskOptions::default()).is_err());
    }
}
#[test]
fn empty_group_never_selects_even_if_inverted() {
    let g = LocalAdjustment {
        invert: true,
        ..Default::default()
    };
    close(
        &rasterize(&image(2, 1), &g, MaskOptions::default()).unwrap(),
        &[0., 0.],
    );
}
#[test]
fn stamp_budget_is_checked_before_rasterising() {
    let s = BrushStroke {
        points: vec![[-16., 0., 1.], [16., 0., 1.]],
        radius: 1e-6,
        ..Default::default()
    };
    let i = image(8192, 1);
    assert!(
        rasterize(
            &i,
            &group(MaskKind::Brush { strokes: vec![s] }),
            MaskOptions::default()
        )
        .is_err()
    );
}
#[test]
fn rejects_nonfinite_rgb_inserted_by_tile() {
    use engine_api::tile::{Extent, Tile, TileCoord, TileLayout};
    let mut i = image(1, 1);
    let tile = Tile::from_samples(
        TileCoord::new(0, 0, 0),
        TileLayout {
            extent: Extent::new(1, 1),
            channels: 3,
            halo: 0,
        },
        vec![f32::NAN, 0., 0.],
    )
    .unwrap();
    i.put(&tile).unwrap();
    assert!(rasterize(&i, &group(linear()), MaskOptions::default()).is_err());
}
#[test]
fn diagonal_gradient_clamps_and_is_level_relative() {
    let g = group(MaskKind::Linear {
        start: [0.25, 0.25],
        end: [0.75, 0.75],
    });
    let m = rasterize(&image(4, 4), &g, MaskOptions::default()).unwrap();
    close(&[m[0], m[5], m[10], m[15]], &[1., 0.75, 0.25, 0.]);
    let small = rasterize(&image(2, 2), &group(linear()), MaskOptions::default()).unwrap();
    close(&small, &[0.75, 0.25, 0.75, 0.25]);
}
#[test]
fn brush_zero_pressure_flow_and_erase_paths() {
    let i = image(21, 5);
    let path = BrushStroke {
        points: vec![[0.1, 0.5, 1.], [0.9, 0.5, 1.]],
        radius: 0.06,
        feather: 0.,
        flow: 100.,
        erase: false,
    };
    let mut eraser = path.clone();
    eraser.erase = true;
    close(
        &rasterize(
            &i,
            &group(MaskKind::Brush {
                strokes: vec![path.clone(), eraser],
            }),
            MaskOptions::default(),
        )
        .unwrap(),
        &[0.; 105],
    );
    for s in [
        BrushStroke {
            flow: 0.,
            ..path.clone()
        },
        BrushStroke {
            points: vec![[0.1, 0.5, 0.], [0.9, 0.5, 0.]],
            ..path.clone()
        },
    ] {
        close(
            &rasterize(
                &i,
                &group(MaskKind::Brush { strokes: vec![s] }),
                MaskOptions::default(),
            )
            .unwrap(),
            &[0.; 105],
        );
    }
    let s = BrushStroke {
        points: vec![[0.1, 0.5, 0.], [0.9, 0.5, 1.]],
        flow: 10.,
        ..path
    };
    let m = rasterize(
        &i,
        &group(MaskKind::Brush { strokes: vec![s] }),
        MaskOptions::default(),
    )
    .unwrap();
    assert!(m[47] < m[52] && m[52] < m[57]);
}
#[test]
fn depth_feather_and_hdr_luminance_are_not_clipped() {
    let i = Image::new(5, 1, vec![vec![-1., 0., 0.5, 1., 2.]; 3]).unwrap();
    close(
        &rasterize(
            &i,
            &group(MaskKind::LuminanceRange {
                range: [0., 1.],
                smoothness: 0.,
            }),
            MaskOptions::default(),
        )
        .unwrap(),
        &[0., 1., 1., 1., 0.],
    );
    let g = group(MaskKind::Depth {
        range: [0.5, 0.75],
        feather: 100.,
        model: None,
    });
    close(
        &rasterize(
            &i,
            &g,
            MaskOptions {
                depth: Some(&[0., 0.25, 0.5, 0.75, 1.]),
                ..Default::default()
            },
        )
        .unwrap(),
        &[0., 0.5, 1., 1., 0.5],
    );
}
#[test]
fn guided_two_dimensional_flat_reference_and_large_radius() {
    let i = image(3, 3);
    let d = [0., 0., 0., 0., 1., 0., 0., 0., 0.];
    let g = group(MaskKind::Depth {
        range: [0.5, 1.],
        feather: 0.,
        model: None,
    });
    let options = MaskOptions {
        depth: Some(&d),
        refinement: Some(GuidedRefinement {
            radius: 1,
            epsilon: 0.01,
        }),
        ..Default::default()
    };
    let m = rasterize(&i, &g, options).unwrap();
    // Two truncated 3x3 means of a centre impulse.
    let first = [
        0.25,
        1. / 6.,
        0.25,
        1. / 6.,
        1. / 9.,
        1. / 6.,
        0.25,
        1. / 6.,
        0.25,
    ];
    for y in 0usize..3 {
        for x in 0usize..3 {
            let mut sum = 0.;
            let mut count = 0.;
            for yy in y.saturating_sub(1)..=(y + 1).min(2) {
                for xx in x.saturating_sub(1)..=(x + 1).min(2) {
                    sum += first[yy * 3 + xx];
                    count += 1.;
                }
            }
            close(&[m[y * 3 + x]], &[sum / count]);
        }
    }
    let m = rasterize(
        &i,
        &g,
        MaskOptions {
            refinement: Some(GuidedRefinement {
                radius: u32::MAX,
                epsilon: 0.01,
            }),
            ..options
        },
    )
    .unwrap();
    close(&m, &[1. / 9.; 9]);
}
fn image(w: u32, h: u32) -> Image {
    Image::new(w, h, vec![vec![0.5; (w * h) as usize]; 3]).unwrap()
}
fn group(k: MaskKind) -> LocalAdjustment {
    LocalAdjustment {
        components: vec![MaskComponent::new(k)],
        ..Default::default()
    }
}
fn linear() -> MaskKind {
    MaskKind::Linear {
        start: [0., 0.],
        end: [1., 0.],
    }
}
fn close(a: &[f32], b: &[f32]) {
    assert_eq!(a.len(), b.len());
    for (a, b) in a.iter().zip(b) {
        assert!((a - b).abs() < 1e-5, "{a} != {b}");
    }
}
#[test]
fn linear_centres_and_empty() {
    let i = image(4, 1);
    close(
        &rasterize(&i, &group(linear()), MaskOptions::default()).unwrap(),
        &[0.875, 0.625, 0.375, 0.125],
    );
    close(
        &rasterize(&i, &LocalAdjustment::default(), MaskOptions::default()).unwrap(),
        &[0.; 4],
    );
}
