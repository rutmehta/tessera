use compositor::{Raster, Rect, raster::Depth};
use engine_api::tile::Extent;
use filters::caf::{ColourAdaptation, FillParams, SamplingArea, fill};
use std::sync::atomic::AtomicBool;

fn image(w: u32, h: u32, f: impl Fn(u32, u32) -> [f32; 4]) -> Raster {
    let mut r = Raster::new(Extent::new(w, h), 4, Depth::F32, 0.0);
    r.edit_region(Rect::of_extent(r.extent()), 1, |x, y, p| *p = f(x, y))
        .unwrap();
    r
}
fn mask(w: usize, h: usize, rect: [usize; 4]) -> Vec<f32> {
    (0..w * h)
        .map(|i| {
            if i % w >= rect[0] && i % w < rect[2] && i / w >= rect[1] && i / w < rect[3] {
                1.0
            } else {
                0.0
            }
        })
        .collect()
}
#[test]
fn invalid_inputs_cancel_noop_and_soft_coverage_are_transactional() {
    let r = image(24, 24, |_, _| [0.3, 0.4, 0.5, 1.0]);
    let cancel = AtomicBool::new(false);
    let p = FillParams::default();
    let zero = vec![0.0; 24 * 24];
    assert!(
        fill(&r, &zero, &p, &cancel)
            .unwrap()
            .composite
            .shares_all_tiles_with(&r)
    );
    for m in [
        vec![1.0; 24 * 24],
        vec![f32::NAN; 24 * 24],
        vec![-0.1; 24 * 24],
        vec![0.0; 3],
    ] {
        assert!(fill(&r, &m, &p, &cancel).is_err());
    }
    assert!(
        fill(&r, &zero, &p, &AtomicBool::new(true))
            .err()
            .unwrap()
            .is_cancelled()
    );
    let m = mask(24, 24, [14, 10, 18, 14])
        .into_iter()
        .map(|v| v * 0.25)
        .collect::<Vec<_>>();
    let r = image(24, 24, |x, _| {
        if x < 10 {
            [0.2, 0.2, 0.2, 1.0]
        } else {
            [0.8, 0.8, 0.8, 1.0]
        }
    });
    let p = FillParams {
        sampling: SamplingArea::Rect(Rect::new(0, 0, 10, 24)),
        colour_adaptation: ColourAdaptation::None,
        output_new_layer: true,
        ..p
    };
    let a = fill(&r, &m, &p, &cancel).unwrap();
    let b = fill(&r, &m, &p, &cancel).unwrap();
    assert!((a.composite.pixel(15, 11)[0] - 0.65).abs() < 1e-6);
    assert_eq!(a.new_layer.unwrap().pixel(15, 11), [0.2, 0.2, 0.2, 0.25]);
    for y in 0..24 {
        for x in 0..24 {
            assert_eq!(a.composite.pixel(x, y), b.composite.pixel(x, y));
        }
    }
}
#[test]
fn move_fills_hole_extend_keeps_source_and_paste_blends_seam() {
    use filters::caf::{MoveMode, move_or_extend};
    let m = mask(48, 28, [10, 10, 17, 17]);
    let r = image(48, 28, |x, y| {
        if m[y as usize * 48 + x as usize] > 0.0 {
            [0.9, 0.2, 0.1, 1.0]
        } else {
            [0.3, 0.3, 0.3, 1.0]
        }
    });
    let p = FillParams {
        colour_adaptation: ColourAdaptation::None,
        output_new_layer: true,
        ..Default::default()
    };
    for mode in [MoveMode::Move, MoveMode::Extend] {
        let out = move_or_extend(
            &r,
            &m,
            [20, 0],
            mode,
            &p,
            ColourAdaptation::None,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(out.composite.pixel(33, 13), r.pixel(13, 13));
        assert_eq!(
            out.composite.pixel(13, 13),
            if mode == MoveMode::Move {
                [0.3, 0.3, 0.3, 1.0]
            } else {
                r.pixel(13, 13)
            }
        );
        assert_eq!(out.composite.pixel(0, 0), r.pixel(0, 0));
        assert!(out.new_layer.is_some());
    }
    let ramp = image(48, 28, |x, y| {
        let v = if x < 24 { 0.2 } else { 0.7 };
        let bump = if (12..15).contains(&x) && (12..15).contains(&y) {
            0.1
        } else {
            0.0
        };
        [v + bump, v + bump, v + bump, 1.0]
    });
    let blended = move_or_extend(
        &ramp,
        &m,
        [20, 0],
        MoveMode::Extend,
        &p,
        ColourAdaptation::VeryHigh,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!((blended.composite.pixel(30, 13)[0] - 0.7).abs() < 0.005);
    assert!((blended.composite.pixel(33, 13)[0] - 0.8).abs() < 0.005);
}
#[test]
fn rotation_scale_and_mirror_adapt_source_texture() {
    let cancel = AtomicBool::new(false);
    for kind in 0..3 {
        let motif = [0.1, 0.2, 0.4, 0.9, 0.3, 0.7, 0.5];
        let texture = |x: u32, y: u32, donor: bool| {
            if kind == 0 {
                if (if donor { x } else { y }) / 2 % 2 == 0 {
                    0.2
                } else {
                    0.8
                }
            } else if kind == 1 {
                if (x / (if donor { 2 } else { 4 })).is_multiple_of(2) {
                    0.2
                } else {
                    0.8
                }
            } else {
                motif[if donor {
                    (x % 7) as usize
                } else {
                    ((7 - x % 7) % 7) as usize
                }]
            }
        };
        let m = mask(64, 32, [45, 12, 51, 18]);
        let r = image(64, 32, |x, y| {
            let v = if m[y as usize * 64 + x as usize] > 0.0 {
                1.0
            } else {
                texture(x, y, x < 24)
            };
            [v, v, v, 1.0]
        });
        let p = FillParams {
            sampling: SamplingArea::Rect(Rect::new(0, 0, 24, 32)),
            colour_adaptation: ColourAdaptation::None,
            rotation_radians: if kind == 0 {
                std::f32::consts::FRAC_PI_2
            } else {
                0.0
            },
            scale_range: if kind == 1 { [0.5, 1.0] } else { [1.0, 1.0] },
            mirror: kind == 2,
            ..Default::default()
        };
        let out = fill(&r, &m, &p, &cancel).unwrap();
        let mut error = 0.0;
        for y in 12..18 {
            for x in 45..51 {
                error += (out.composite.pixel(x, y)[0] - texture(x, y, false)).abs();
            }
        }
        assert!(error / 36.0 < 0.12, "transform {kind} MAE {}", error / 36.0);
    }
}
#[test]
fn colour_levels_reduce_boundary_error_without_touching_exterior() {
    let r = image(32, 24, |x, _| {
        let v = if x < 12 { 0.2 } else { 0.8 };
        [v, v, v, 1.0]
    });
    let m = mask(32, 24, [20, 9, 25, 14]);
    let mut errors = Vec::new();
    for level in [
        ColourAdaptation::None,
        ColourAdaptation::Default,
        ColourAdaptation::High,
        ColourAdaptation::VeryHigh,
    ] {
        let p = FillParams {
            sampling: SamplingArea::Rect(Rect::new(0, 0, 12, 24)),
            colour_adaptation: level,
            ..Default::default()
        };
        let out = fill(&r, &m, &p, &AtomicBool::new(false)).unwrap();
        errors.push((out.composite.pixel(20, 11)[0] - 0.8).abs());
        assert_eq!(out.composite.pixel(19, 11), r.pixel(19, 11));
    }
    assert!(
        errors.windows(2).all(|e| e[1] < e[0]),
        "boundary errors {errors:?}"
    );
    assert!(errors[3] < 0.005, "Poisson seam error {}", errors[3]);
}
#[test]
fn explicit_sampling_excludes_all_other_colours_and_returns_new_layer() {
    let r = image(32, 24, |x, _| {
        if x < 12 {
            [0.2, 0.3, 0.4, 1.0]
        } else {
            [0.9, 0.1, 0.1, 1.0]
        }
    });
    let m = mask(32, 24, [20, 9, 25, 14]);
    for sampling in [
        SamplingArea::Rect(Rect::new(0, 0, 12, 24)),
        SamplingArea::Custom(mask(32, 24, [0, 0, 12, 24])),
    ] {
        let p = FillParams {
            sampling,
            colour_adaptation: ColourAdaptation::None,
            output_new_layer: true,
            ..Default::default()
        };
        let out = fill(&r, &m, &p, &AtomicBool::new(false)).unwrap();
        assert_eq!(out.composite.pixel(22, 11), [0.2, 0.3, 0.4, 1.0]);
        let layer = out.new_layer.unwrap();
        assert_eq!(layer.pixel(0, 0), [0.0; 4]);
        assert_eq!(layer.pixel(22, 11), [0.2, 0.3, 0.4, 1.0]);
    }
}
#[test]
fn patchmatch_reconstructs_periodic_texture_not_flat_average() {
    let (w, h) = (40, 32);
    let m = mask(w, h, [15, 11, 25, 21]);
    let texture = |x: u32, y: u32| {
        let v = if (x / 2 + y / 2).is_multiple_of(2) {
            0.2
        } else {
            0.8
        };
        [v, v, v, 1.0]
    };
    let clean = image(w as u32, h as u32, texture);
    let broken = image(w as u32, h as u32, |x, y| {
        if m[y as usize * w + x as usize] > 0.0 {
            [1.0, 0.0, 0.0, 1.0]
        } else {
            texture(x, y)
        }
    });
    let out = fill(&broken, &m, &FillParams::default(), &AtomicBool::new(false)).unwrap();
    let mut error = 0.0f32;
    let mut sum = 0.0f32;
    let mut sq = 0.0f32;
    let mut n = 0.0f32;
    for y in 0..h {
        for x in 0..w {
            let p = out.composite.pixel(x as u32, y as u32);
            if m[y * w + x] > 0.0 {
                error += (p[0] - clean.pixel(x as u32, y as u32)[0]).abs();
                sum += p[0];
                sq += p[0] * p[0];
                n += 1.0;
            } else {
                assert_eq!(p, broken.pixel(x as u32, y as u32));
            }
        }
    }
    assert!(error / n < 0.10, "texture MAE {}", error / n);
    assert!((sum / n - 0.5).abs() < 0.12);
    assert!(
        sq / n - (sum / n).powi(2) > 0.045,
        "must retain texture variance"
    );
    assert_eq!(broken.pixel(16, 12), [1.0, 0.0, 0.0, 1.0]);
}

#[test]
fn distant_single_patch_donor_fills_disconnected_edge_holes_deterministically() {
    let (w, h) = (256, 64);
    let mut m = mask(w, h, [0, 0, 5, 5]);
    for y in 59..64 {
        for x in 0..5 {
            m[y * w + x] = 0.5;
        }
    }
    let r = image(w as u32, h as u32, |x, _| {
        if x >= 249 {
            [0.2, 0.3, 0.4, 1.0]
        } else {
            [0.9, 0.8, 0.7, 1.0]
        }
    });
    let p = FillParams {
        sampling: SamplingArea::Custom(mask(w, h, [249, 20, 256, 27])),
        colour_adaptation: ColourAdaptation::None,
        output_new_layer: true,
        ..Default::default()
    };
    let cancel = AtomicBool::new(false);
    let a = fill(&r, &m, &p, &cancel).unwrap();
    let b = fill(&r, &m, &p, &cancel).unwrap();
    for y in 0..h {
        for x in 0..w {
            let actual = a.composite.pixel(x as u32, y as u32);
            assert_eq!(actual, b.composite.pixel(x as u32, y as u32));
            let coverage = m[y * w + x];
            let original = r.pixel(x as u32, y as u32);
            for c in 0..4 {
                let expected = original[c] + coverage * ([0.2, 0.3, 0.4, 1.0][c] - original[c]);
                assert!((actual[c] - expected).abs() < 1e-6);
            }
        }
    }
    assert_eq!(a.new_layer.unwrap().pixel(0, 0), [0.2, 0.3, 0.4, 1.0]);
}

#[test]
fn source_patch_eligibility_checks_interior_not_just_corners() {
    let r = image(32, 24, |_, _| [0.3, 0.4, 0.5, 1.0]);
    let m = mask(32, 24, [24, 10, 28, 14]);
    let mut sampling = mask(32, 24, [2, 2, 9, 9]);
    let p = FillParams {
        sampling: SamplingArea::Custom(sampling.clone()),
        colour_adaptation: ColourAdaptation::None,
        ..Default::default()
    };
    let cancel = AtomicBool::new(false);
    assert!(fill(&r, &m, &p, &cancel).is_ok());
    sampling[5 * 32 + 5] = 0.0;
    assert!(
        fill(
            &r,
            &m,
            &FillParams {
                sampling: SamplingArea::Custom(sampling),
                ..p
            },
            &cancel
        )
        .is_err()
    );
}
