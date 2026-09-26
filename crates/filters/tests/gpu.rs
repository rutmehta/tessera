use compositor::{
    geom::Rect,
    raster::{Depth, Raster},
};
use engine_api::tile::Extent;
use filters::{Effect, Filter, FilterParams, gpu::GpuFilters};
use std::sync::atomic::AtomicBool;
fn fixture() -> Raster {
    fixture_size(19, 13)
}
#[test]
fn large_gaussian_downsample_pipeline_parity() {
    let gpu = GpuFilters::new().expect("real Metal adapter required");
    for radius in [32.01, 64.0, 128.0, 250.0] {
        for effect in [Effect::Gaussian, Effect::UnsharpMask, Effect::HighPass] {
            parity(
                &gpu,
                effect,
                FilterParams {
                    radius,
                    amount: 0.75,
                    strength: 0.8,
                    threshold: 0.0,
                    ..Default::default()
                },
            );
        }
    }
}
fn fixture_size(w: u32, h: u32) -> Raster {
    let mut r = Raster::new(Extent::new(w, h), 4, Depth::F32, 0.0);
    r.edit_region(Rect::of_extent(r.extent()), 1, |x, y, p| {
        *p = [
            ((x * 13 + y * 7) % 31) as f32 / 23.0 - 0.12,
            ((x * 3 + y * 17) % 29) as f32 / 29.0,
            ((x * 19 + y * 5) % 37) as f32 / 31.0,
            ((x + y) % 11) as f32 / 11.0,
        ];
    })
    .unwrap();
    r
}
fn parity(gpu: &GpuFilters, effect: Effect, p: FilterParams) {
    parity_input(gpu, effect, p, fixture());
}
fn parity_input(gpu: &GpuFilters, effect: Effect, p: FilterParams, r: Raster) {
    let cancel = AtomicBool::new(false);
    let cpu = effect.apply(&r, &p, &cancel).unwrap();
    let out = gpu.apply(effect, &r, &p, &cancel).unwrap();
    let mut max = 0.0_f32;
    for y in 0..r.extent().height {
        for x in 0..r.extent().width {
            for c in 0..4 {
                if c == 3
                    && matches!(
                        effect,
                        Effect::Adjust
                            | Effect::AddNoise
                            | Effect::HighPass
                            | Effect::UnsharpMask
                            | Effect::LensBlur
                    )
                {
                    assert_eq!(
                        out.pixel(x, y)[3].to_bits(),
                        r.pixel(x, y)[3].to_bits(),
                        "alpha changed: {effect:?}"
                    );
                }
                let a = cpu.pixel(x, y)[c];
                let b = out.pixel(x, y)[c];
                assert!(b.is_finite());
                max = max.max((a - b).abs());
            }
        }
    }
    assert!(max <= 1e-4, "{effect:?} {:?}: max abs {max}", p.adjust);
}
#[test]
fn match_colour_constant_and_dark_inputs() {
    let gpu = GpuFilters::new().expect("real Metal adapter required");
    for rgb in [
        [0.0; 3],
        [-0.3, -0.2, -0.1],
        [0.4; 3],
        [0.00001, 0.00002, 0.00003],
    ] {
        let mut r = fixture();
        r.edit_region(Rect::of_extent(r.extent()), 2, |_, _, p| {
            p[..3].copy_from_slice(&rgb)
        })
        .unwrap();
        parity_input(
            &gpu,
            Effect::Adjust,
            FilterParams {
                amount: 1.0,
                adjust: filters::adjust::Adjustment::MatchColour {
                    target: vec![[0.2, 0.5, 0.7], [0.6, 0.3, 0.8]],
                    amount: 0.75,
                },
                ..Default::default()
            },
            r,
        );
    }
}
#[test]
fn neutral_and_max_exact_sigma_parity() {
    let gpu = GpuFilters::new().expect("real Metal adapter required");
    for effect in [
        Effect::Gaussian,
        Effect::Box,
        Effect::HighPass,
        Effect::UnsharpMask,
        Effect::Motion,
        Effect::SurfaceBlur,
    ] {
        parity(
            &gpu,
            effect,
            FilterParams {
                amount: 1.0,
                radius: 0.0,
                ..Default::default()
            },
        );
    }
    parity(
        &gpu,
        Effect::Gaussian,
        FilterParams {
            amount: 1.0,
            radius: 32.0,
            ..Default::default()
        },
    );
}
#[test]
fn tiled_and_degenerate_metal_parity() {
    use filters::distort::{DistortParams, Distortion::*};
    let gpu = GpuFilters::new().expect("real Metal adapter required");
    for (w, h) in [(263, 17), (1, 9), (9, 1)] {
        for effect in [
            Effect::Gaussian,
            Effect::Box,
            Effect::Motion,
            Effect::RadialSpin,
            Effect::RadialZoom,
            Effect::SurfaceBlur,
            Effect::UnsharpMask,
            Effect::HighPass,
            Effect::AddNoise,
        ] {
            parity_input(
                &gpu,
                effect,
                FilterParams {
                    radius: 1.7,
                    amount: 1.0,
                    angle: 0.19,
                    strength: 0.43,
                    threshold: 0.13,
                    gaussian_noise: true,
                    seed: 42,
                    ..Default::default()
                },
                fixture_size(w, h),
            );
        }
        for kind in [Pinch, Spherize, Twirl, Wave, Ripple, Offset] {
            parity_input(
                &gpu,
                Effect::Distort(kind),
                FilterParams {
                    amount: 1.0,
                    distort: DistortParams {
                        amount: -0.7,
                        wavelength: 19.0,
                        phase: 1.3,
                        offset: [-1.2, 3.7],
                        center: [0.43, 0.57],
                    },
                    ..Default::default()
                },
                fixture_size(w, h),
            );
        }
    }
}
#[test]
fn gpu_validation_cancellation_and_identity() {
    use engine_api::EngineError;
    let gpu = GpuFilters::new().expect("real Metal adapter required");
    let r = fixture();
    let cancel = AtomicBool::new(false);
    let p = FilterParams::default();
    assert!(
        gpu.apply(Effect::Gaussian, &r, &p, &cancel)
            .unwrap()
            .shares_all_tiles_with(&r)
    );
    assert!(matches!(
        gpu.apply(Effect::Gaussian, &r, &p, &AtomicBool::new(true)),
        Err(EngineError::Cancelled)
    ));
    for effect in [
        Effect::SmartSharpen,
        Effect::ReduceNoise,
        Effect::Median,
        Effect::DustScratches,
        Effect::CameraRaw,
        Effect::OilPaint,
    ] {
        assert!(
            gpu.apply(
                effect,
                &r,
                &FilterParams {
                    amount: 1.0,
                    ..Default::default()
                },
                &cancel
            )
            .is_err()
        );
    }
    for p in [
        FilterParams {
            amount: 1.0,
            radius: 251.0,
            ..Default::default()
        },
        FilterParams {
            amount: 1.0,
            radius: f32::NAN,
            ..Default::default()
        },
    ] {
        assert!(gpu.apply(Effect::Gaussian, &r, &p, &cancel).is_err());
    }
    assert!(
        gpu.apply(
            Effect::LensBlur,
            &r,
            &FilterParams {
                amount: 1.0,
                ..Default::default()
            },
            &cancel
        )
        .is_err()
    );
    assert!(
        gpu.apply(
            Effect::Adjust,
            &r,
            &FilterParams {
                amount: 1.0,
                adjust: filters::adjust::Adjustment::Exposure {
                    stops: 0.0,
                    offset: 0.0,
                    gamma: 0.0
                },
                ..Default::default()
            },
            &cancel
        )
        .is_err()
    );
}
#[test]
fn lens_metal_parity() {
    let gpu = GpuFilters::new().expect("real Metal adapter required");
    parity(
        &gpu,
        Effect::LensBlur,
        FilterParams {
            radius: 4.7,
            amount: 0.86,
            focus: [0.32, 0.46],
            depth: Some((0..247).map(|i| ((i * 7) % 29) as f32 / 28.0).collect()),
            ..Default::default()
        },
    );
}
#[test]
fn adjustments_metal_parity() {
    use filters::adjust::Adjustment::*;
    let gpu = GpuFilters::new().expect("real Metal adapter required");
    let mut corrections = [[0.0; 4]; 9];
    for (i, c) in corrections.iter_mut().enumerate() {
        *c = [0.02 * i as f32, -0.04, 0.07, -0.015];
    }
    for adjust in [
        Levels {
            input_black: [0.03, 0.07, 0.1],
            input_white: [0.91, 0.87, 0.95],
            gamma: [0.8, 1.3, 1.7],
            output_black: [0.01; 3],
            output_white: [0.97; 3],
        },
        Curves {
            points: std::array::from_fn(|c| {
                vec![
                    [0.0, 0.03],
                    [0.3, 0.4 + c as f32 * 0.06],
                    [0.7, 0.8],
                    [1.0, 0.98],
                ]
            }),
        },
        BrightnessContrast {
            brightness: 32.0,
            contrast: -24.0,
            legacy: false,
        },
        BrightnessContrast {
            brightness: -18.0,
            contrast: 34.0,
            legacy: true,
        },
        Exposure {
            stops: 0.7,
            offset: -0.08,
            gamma: 1.3,
        },
        Threshold { level: 0.43 },
        Posterize { levels: 7 },
        Hsl {
            hue_degrees: -47.0,
            saturation: 0.27,
            lightness: -0.13,
        },
        Vibrance {
            vibrance: 0.45,
            saturation: -0.17,
        },
        PhotoFilter {
            colour: [0.7, 0.3, 0.9],
            density: 0.4,
            preserve_luminosity: true,
        },
        PhotoFilter {
            colour: [0.7, 0.3, 0.9],
            density: 0.4,
            preserve_luminosity: false,
        },
        ChannelMixer {
            matrix: [[0.8, 0.3, -0.1], [0.1, 0.7, 0.2], [-0.2, 0.1, 1.1]],
            constant: [0.03, -0.02, 0.05],
        },
        GradientMap {
            stops: vec![
                (0.0, [0.1, 0.0, 0.3]),
                (0.4, [0.3, 0.7, 0.2]),
                (1.0, [0.9, 0.4, 1.0]),
            ],
            reverse: true,
        },
        SelectiveColour {
            corrections,
            relative: true,
        },
        SelectiveColour {
            corrections,
            relative: false,
        },
        BlackWhite {
            weights: [0.7, 1.2, 0.9, 1.3, 0.6, 1.1],
            tint: Some([0.8, 0.6, 0.4]),
        },
        MatchColour {
            target: vec![[0.2, 0.1, 0.7], [0.7, 0.8, 0.3], [0.4, 0.2, 0.1]],
            amount: 0.63,
        },
        Invert,
        Desaturate,
    ] {
        parity(
            &gpu,
            Effect::Adjust,
            FilterParams {
                amount: 0.79,
                adjust,
                ..Default::default()
            },
        );
    }
}
#[test]
fn distortions_metal_parity() {
    use filters::distort::{DistortParams, Distortion::*};
    let gpu = GpuFilters::new().expect("real Metal adapter required");
    for kind in [
        Pinch,
        Spherize,
        Twirl,
        Wave,
        Ripple,
        PolarToRectangular,
        RectangularToPolar,
        Offset,
    ] {
        parity(
            &gpu,
            Effect::Distort(kind),
            FilterParams {
                amount: 0.87,
                distort: DistortParams {
                    amount: 0.65,
                    wavelength: 17.0,
                    phase: 0.7,
                    offset: [2.35, -1.7],
                    center: [0.43, 0.57],
                },
                ..Default::default()
            },
        );
    }
}
#[test]
fn spatial_metal_parity() {
    let gpu = GpuFilters::new().expect("real Metal adapter required");
    for effect in [
        Effect::Box,
        Effect::UnsharpMask,
        Effect::HighPass,
        Effect::Motion,
        Effect::RadialSpin,
        Effect::RadialZoom,
        Effect::SurfaceBlur,
        Effect::AddNoise,
    ] {
        for gaussian_noise in [false, true] {
            for monochrome in [false, true] {
                parity(
                    &gpu,
                    effect,
                    FilterParams {
                        radius: 2.3,
                        amount: 0.81,
                        angle: 0.27,
                        threshold: 0.24,
                        strength: 0.36,
                        seed: 0xfedcba98,
                        gaussian_noise,
                        monochrome,
                        ..Default::default()
                    },
                );
            }
        }
    }
}
#[test]
fn gaussian_metal_parity() {
    let gpu = GpuFilters::new().expect("real Metal adapter required");
    parity(
        &gpu,
        Effect::Gaussian,
        FilterParams {
            radius: 1.7,
            amount: 0.73,
            ..Default::default()
        },
    );
}
