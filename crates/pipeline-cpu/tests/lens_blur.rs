use engine_api::recipe::settings::LensBlur;
use pipeline_cpu::{Image, LensBlurOptions, lens_blur};

#[test]
fn normalized_layers_do_not_darken_edges_or_bleed_sharp_foreground_into_background() {
    let w = 33;
    let depth: Vec<_> = (0..w * 9)
        .map(|i| if i % w < 16 { 0.2 } else { 1. })
        .collect();
    let values: Vec<_> = (0..w * 9)
        .map(|i| if i % w < 16 { -10. } else { 2. })
        .collect();
    let image = Image::new(w as u32, 9, vec![values; 3]).unwrap();
    let s = LensBlur {
        amount: 100.,
        focus_range: [0.2, 0.2],
        ..Default::default()
    };
    for shape in ["circle", "hexagon", "octagon"] {
        let settings = LensBlur {
            bokeh: shape.into(),
            ..s.clone()
        };
        let out = lens_blur(&image, &depth, &settings, Default::default()).unwrap();
        assert_eq!(image.planes(), out.planes());
    }
    // No black padding or unnormalized partial layers, even for extreme HDR.
    for value in [-f32::MAX, -2., 0.5, f32::MAX] {
        let image = Image::new(w as u32, 9, vec![vec![value; w * 9]; 3]).unwrap();
        let depth: Vec<_> = (0..w * 9)
            .map(|i| (i % w) as f32 / (w - 1) as f32)
            .collect();
        let out = lens_blur(&image, &depth, &s, Default::default()).unwrap();
        assert!(out.planes().iter().flatten().all(|&v| v == value));
    }
}

#[test]
fn nearer_layers_composite_over_farther_layers() {
    let image = Image::new(2, 1, vec![vec![1., 0.]; 3]).unwrap();
    let settings = LensBlur {
        amount: 100.,
        focus_range: [0.5, 0.5],
        ..Default::default()
    };
    let options = LensBlurOptions {
        max_radius: 4.,
        ..Default::default()
    };
    let out = lens_blur(&image, &[0., 1.], &settings, options).unwrap();
    // Each layer covers half the two-pixel footprint: near alpha=.5 over far=.5,
    // normalized by total coverage .75 yields 2/3, not the 1/2 of a flat gather.
    assert!((out.planes()[0][0] - 2. / 3.).abs() < 1e-6);
    let reverse = lens_blur(&image, &[1., 0.], &settings, options).unwrap();
    assert!((reverse.planes()[0][0] - 1. / 3.).abs() < 1e-6);
}

#[test]
fn inclusive_focus_boundaries_tiny_images_and_cross_tile_support() {
    let image = Image::new(4, 1, vec![vec![-0., 7., 2., 1.]; 3]).unwrap();
    let settings = LensBlur {
        focus_range: [0.2, 0.4],
        ..Default::default()
    };
    let out = lens_blur(&image, &[0.2, 0.3, 0.4, 1.], &settings, Default::default()).unwrap();
    for i in 0..3 {
        assert_eq!(out.planes()[0][i].to_bits(), image.planes()[0][i].to_bits());
    }
    let tiny = Image::new(1, 1, vec![vec![-3.]; 3]).unwrap();
    assert_eq!(
        lens_blur(&tiny, &[1.], &settings, Default::default())
            .unwrap()
            .planes(),
        tiny.planes()
    );
    let mut impulse = vec![0.; 520];
    impulse[256] = 1.;
    let image = Image::new(520, 1, vec![impulse; 3]).unwrap();
    let out = lens_blur(&image, &[1.; 520], &settings, Default::default()).unwrap();
    assert!(out.planes()[0][255] > 0.);
    assert_eq!(out.planes()[0][255], out.planes()[0][257]);
}

#[test]
fn depth_render_runs_blur_before_effects_geometry_and_downsample() {
    use engine_api::recipe::DevelopSettings;
    use pipeline_cpu::{
        LensContext, RenderSource, render_linear_scaled, render_linear_scaled_with_depth,
    };
    let image = Image::new(
        32,
        16,
        vec![(0..512).map(|i| (i % 7) as f32 / 6.).collect(); 3],
    )
    .unwrap();
    let source = RenderSource::Rgb(&image);
    let mut settings = DevelopSettings::default();
    let base = render_linear_scaled(&settings, &source, 1).unwrap();
    settings.effects.lens_blur = Some(LensBlur {
        amount: 100.,
        ..Default::default()
    });
    settings.effects.grain.amount = 25.;
    settings.effects.vignette.amount = -30.;
    settings.geometry.crop.rect.left = 0.25;
    let depth = vec![1.; 512];
    let context = LensContext::default();
    let options = LensBlurOptions::default();
    let actual =
        render_linear_scaled_with_depth(&settings, &source, 2, &context, &depth, options).unwrap();
    let mut expected = lens_blur(
        &base,
        &depth,
        settings.effects.lens_blur.as_ref().unwrap(),
        options,
    )
    .unwrap();
    let mut effects = settings.effects.clone();
    effects.lens_blur = None;
    for coord in expected.coords() {
        let mut tile = expected.tile(coord, 0, 1).unwrap();
        pipeline_cpu::effects_in_crop(
            &mut tile,
            &effects,
            engine_api::tile::Extent::new(32, 16),
            &settings.geometry.crop,
        )
        .unwrap();
        expected.put(&tile).unwrap();
    }
    expected = pipeline_cpu::geometry(&expected, &settings.geometry).unwrap();
    expected = expected
        .downsample_crop([0, 0, expected.width(), expected.height()], 2)
        .unwrap();
    assert_eq!(actual.planes(), expected.planes());
    assert!(
        render_linear_scaled(&settings, &source, 1).is_err(),
        "legacy render must not silently drop depth blur"
    );
    assert!(
        render_linear_scaled_with_depth(&settings, &source, 1, &context, &depth[..511], options)
            .is_err()
    );
    settings.effects.lens_blur = None;
    let off =
        render_linear_scaled_with_depth(&settings, &source, 2, &context, &depth, options).unwrap();
    assert_eq!(
        off.planes(),
        render_linear_scaled(&settings, &source, 2)
            .unwrap()
            .planes()
    );
}

#[test]
fn boost_increases_only_out_of_focus_highlights_and_cat_eye_is_explicitly_reserved() {
    let image = Image::new(3, 1, vec![vec![2., 0.5, 4.]; 3]).unwrap();
    let s = LensBlur {
        focus_range: [0., 0.2],
        ..Default::default()
    };
    let options = LensBlurOptions {
        max_radius: 0.5,
        boost: 100.,
        ..Default::default()
    };
    let out = lens_blur(&image, &[1., 1., 0.1], &s, options).unwrap();
    assert!(out.planes()[0][0] > 2.);
    assert_eq!(out.planes()[0][1], 0.5);
    assert_eq!(out.planes()[0][2], 4.);
    for options in [
        LensBlurOptions {
            boost: -1.,
            ..Default::default()
        },
        LensBlurOptions {
            boost: f32::NAN,
            ..Default::default()
        },
        LensBlurOptions {
            boost: 101.,
            ..Default::default()
        },
        LensBlurOptions {
            cat_eye: 0.5,
            ..Default::default()
        },
        LensBlurOptions {
            cat_eye: f32::NAN,
            ..Default::default()
        },
    ] {
        assert!(lens_blur(&image, &[1.; 3], &s, options).is_err());
    }
    let hdr = Image::new(2, 1, vec![vec![f32::MAX; 2]; 3]).unwrap();
    assert!(
        lens_blur(&hdr, &[1.; 2], &s, options)
            .unwrap()
            .planes()
            .iter()
            .flatten()
            .all(|v| v.is_finite())
    );
}

#[test]
fn aperture_shapes_have_distinct_normalized_impulse_footprints() {
    let mut impulse = vec![0.; 41 * 41];
    impulse[20 * 41 + 20] = 1.;
    let image = Image::new(41, 41, vec![impulse; 3]).unwrap();
    let mut footprints = Vec::new();
    for shape in ["disc", "hexagon", "octagon"] {
        let settings = LensBlur {
            amount: 100.,
            focus_range: [0., 0.],
            bokeh: shape.into(),
            ..Default::default()
        };
        let out = lens_blur(
            &image,
            &vec![1.; 41 * 41],
            &settings,
            LensBlurOptions {
                max_radius: 8.,
                ..Default::default()
            },
        )
        .unwrap();
        let p = &out.planes()[0];
        assert!((p.iter().sum::<f32>() - 1.).abs() < 1e-5);
        assert_eq!(p[20 * 41 + 12], p[20 * 41 + 28]);
        footprints.push(p.clone());
    }
    assert_ne!(footprints[0], footprints[1]);
    assert_ne!(footprints[0], footprints[2]);
    assert_ne!(footprints[1], footprints[2]);
}

#[test]
fn invalid_inputs_are_rejected_even_when_off() {
    let image = Image::new(2, 1, vec![vec![0.; 2]; 3]).unwrap();
    let off = LensBlur {
        amount: 0.,
        ..Default::default()
    };
    for d in [
        vec![],
        vec![0.],
        vec![0., f32::NAN],
        vec![0., f32::INFINITY],
        vec![0., -0.1],
        vec![0., 1.1],
    ] {
        assert!(lens_blur(&image, &d, &off, Default::default()).is_err());
    }
    for amount in [-1., 101., f32::NAN, f32::INFINITY] {
        let s = LensBlur {
            amount,
            ..Default::default()
        };
        assert!(lens_blur(&image, &[0., 1.], &s, Default::default()).is_err());
    }
    for focus_range in [[0.8, 0.2], [-0.1, 1.], [0., 1.1], [f32::NAN, 1.]] {
        let s = LensBlur {
            focus_range,
            ..Default::default()
        };
        assert!(lens_blur(&image, &[0., 1.], &s, Default::default()).is_err());
    }
    for options in [
        LensBlurOptions {
            layers: 0,
            ..Default::default()
        },
        LensBlurOptions {
            layers: 65,
            ..Default::default()
        },
        LensBlurOptions {
            max_radius: f32::NAN,
            ..Default::default()
        },
        LensBlurOptions {
            max_radius: 129.,
            ..Default::default()
        },
    ] {
        assert!(lens_blur(&image, &[0., 1.], &off, options).is_err());
    }
    let mono = Image::new(2, 1, vec![vec![0.; 2]]).unwrap();
    assert!(lens_blur(&mono, &[0., 1.], &off, Default::default()).is_err());
    let unknown = LensBlur {
        bokeh: "unknown".into(),
        ..off
    };
    assert!(lens_blur(&image, &[0., 1.], &unknown, Default::default()).is_err());
}

#[test]
fn disabled_blur_preserves_signed_zero_and_hdr_bits() {
    let image = Image::new(3, 1, vec![vec![-0., -2., f32::MAX]; 3]).unwrap();
    let s = LensBlur {
        amount: 0.,
        ..Default::default()
    };
    let out = lens_blur(&image, &[0.8; 3], &s, Default::default()).unwrap();
    for (a, b) in image
        .planes()
        .iter()
        .flatten()
        .zip(out.planes().iter().flatten())
    {
        assert_eq!(a.to_bits(), b.to_bits());
    }
}

#[test]
fn two_planes_keep_focus_exact_and_reduce_background_contrast() {
    let (w, h) = (64, 32);
    let values: Vec<f32> = (0..w * h)
        .map(|i| if (i % w + i / w) % 2 == 0 { 1. } else { 0. })
        .collect();
    let depth: Vec<f32> = (0..w * h)
        .map(|i| if i % w < 24 { 0.2 } else { 0.9 })
        .collect();
    let image = Image::new(w as u32, h as u32, vec![values.clone(); 3]).unwrap();
    let settings = LensBlur {
        amount: 100.,
        focus_range: [0.1, 0.3],
        ..Default::default()
    };
    let out = lens_blur(&image, &depth, &settings, LensBlurOptions::default()).unwrap();
    for i in 0..w * h {
        if depth[i] == 0.2 {
            assert_eq!(out.planes()[0][i].to_bits(), values[i].to_bits());
        }
    }
    let background: Vec<_> = (8..24)
        .flat_map(|y| (40..56).map(move |x| y * w + x))
        .collect();
    let contrast = background
        .iter()
        .map(|&i| (out.planes()[0][i] - 0.5).abs())
        .sum::<f32>()
        / background.len() as f32;
    assert!(
        contrast < 0.25,
        "background contrast {contrast} must drop >50%"
    );
}
