use engine_api::{
    recipe::mask::{LocalAdjustment, LocalParams, MaskComponent, MaskKind},
    tile::Extent,
};
use pipeline_gpu::GpuContext;
use std::sync::Arc;
use wgpu::util::DeviceExt;

fn check(groups: &[LocalAdjustment], tolerance: f32) {
    check_at(groups, tolerance, Extent::new(19, 13));
}
fn check_at(groups: &[LocalAdjustment], tolerance: f32, extent: Extent) {
    let ctx = Arc::new(GpuContext::new().unwrap());
    let n = (extent.width * extent.height) as usize;
    let planes: Vec<Vec<f32>> = (0..3)
        .map(|c| {
            (0..n)
                .map(|i| ((i * 37 + c * 113) % 401) as f32 / 300. - 0.03)
                .collect()
        })
        .collect();
    let input = pipeline_cpu::Image::new(extent.width, extent.height, planes.clone()).unwrap();
    let expected = pipeline_cpu::locals_image(&input, groups, Default::default()).unwrap();
    let values: Vec<f32> = planes.into_iter().flatten().collect();
    let source = ctx
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&values),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        });
    let output = ctx.locals_resident(&source, extent, groups).unwrap();
    assert_ne!(
        output.buffer(),
        &source,
        "output must own independent storage"
    );
    assert!(!output.packed());
    let bytes = gpu_core::read_buffer(
        &ctx.device,
        &ctx.queue,
        output.buffer(),
        0,
        (values.len() * 4) as u64,
    )
    .unwrap();
    let actual: &[f32] = bytemuck::cast_slice(&bytes);
    assert!(
        actual.iter().all(|v| v.is_finite()),
        "GPU output must be finite"
    );
    let error = actual
        .iter()
        .zip(expected.planes().iter().flatten())
        .map(|(a, b)| (a - b).abs())
        .fold(0f32, f32::max);
    assert!(
        error <= tolerance,
        "max error {error}, tolerance {tolerance}"
    );
    let before = gpu_core::read_buffer(
        &ctx.device,
        &ctx.queue,
        &source,
        0,
        (values.len() * 4) as u64,
    )
    .unwrap();
    assert_eq!(bytemuck::cast_slice::<u8, f32>(&before), values);
}
fn group(params: LocalParams) -> LocalAdjustment {
    LocalAdjustment {
        components: vec![MaskComponent::new(MaskKind::Linear {
            start: [0.1, 0.2],
            end: [0.9, 0.8],
        })],
        params,
        ..Default::default()
    }
}
#[test]
fn procedural_masks_and_composition_match_cpu() {
    use engine_api::recipe::mask::{BrushStroke, MaskCombine};
    let masks = vec![
        MaskKind::Radial {
            center: [0.4, 0.6],
            radii: [0.35, 0.7],
            angle: 32.,
            feather: 57.,
        },
        MaskKind::LuminanceRange {
            range: [0.2, 0.6],
            smoothness: 49.,
        },
        MaskKind::ColorRange {
            samples: vec![[0.7, 0.1, -0.05], [0.4, -0.1, 0.1]],
            amount: 40.,
        },
        MaskKind::Brush {
            strokes: vec![
                BrushStroke {
                    points: vec![[0.1, 0.1, 0.2], [0.8, 0.7, 0.9]],
                    radius: 0.23,
                    feather: 50.,
                    flow: 67.,
                    erase: false,
                },
                BrushStroke {
                    points: vec![[0.5, 0.5, 0.7]],
                    radius: 0.18,
                    feather: 0.,
                    flow: 80.,
                    erase: true,
                },
            ],
        },
    ];
    for mask in &masks {
        let mut g = group(LocalParams {
            exposure: 0.75,
            ..Default::default()
        });
        g.components = vec![MaskComponent::new(mask.clone())];
        check(&[g], 1e-4);
    }
    for combine in [
        MaskCombine::Add,
        MaskCombine::Subtract,
        MaskCombine::Intersect,
    ] {
        let mut g = group(LocalParams {
            exposure: 0.75,
            ..Default::default()
        });
        g.amount = 137.;
        g.invert = true;
        g.components.push(MaskComponent {
            kind: masks[0].clone(),
            combine,
            invert: true,
        });
        let mut second = g.clone();
        second.params.exposure = -0.25;
        second.invert = false;
        check(&[g, second], 1e-4);
    }
}
#[test]
fn local_spatial_operators_match_cpu() {
    for sign in [-1., 1.] {
        for (name, p) in [
            (
                "texture",
                LocalParams {
                    texture: sign * 43.,
                    ..Default::default()
                },
            ),
            (
                "clarity",
                LocalParams {
                    clarity: sign * 37.,
                    ..Default::default()
                },
            ),
            (
                "dehaze",
                LocalParams {
                    dehaze: sign * 35.,
                    ..Default::default()
                },
            ),
            (
                "sharpness",
                LocalParams {
                    sharpness: sign * 55.,
                    ..Default::default()
                },
            ),
            (
                "noise",
                LocalParams {
                    noise: sign * 47.,
                    ..Default::default()
                },
            ),
        ] {
            eprintln!("{name} {sign}");
            check(&[group(p)], 1e-4);
        }
    }
}
#[test]
fn complete_chain_and_tile_boundaries() {
    let mut g = group(LocalParams {
        exposure: 0.35,
        contrast: 21.,
        highlights: -25.,
        shadows: 13.,
        whites: 7.,
        blacks: -9.,
        temperature: 12.,
        tint: -8.,
        saturation: 23.,
        hue: 27.,
        texture: 31.,
        clarity: -19.,
        dehaze: 17.,
        sharpness: 27.,
        noise: -15.,
        moire: 20.,
        ..Default::default()
    });
    g.amount = 143.;
    let mut other = group(LocalParams {
        exposure: -0.25,
        saturation: -12.,
        ..Default::default()
    });
    other.invert = true;
    for extent in [Extent::new(1, 1), Extent::new(1, 17), Extent::new(521, 259)] {
        check_at(&[g.clone(), other.clone()], 2e-3, extent);
    }
}
#[test]
fn invalid_parameters_and_external_masks_are_rejected() {
    let ctx = Arc::new(GpuContext::new().unwrap());
    let source = ctx
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&[0.5f32; 3]),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        });
    for params in [
        LocalParams {
            defringe: 1.,
            ..Default::default()
        },
        LocalParams {
            color_overlay: Some([30., 50.]),
            ..Default::default()
        },
        LocalParams {
            hue: f32::NAN,
            ..Default::default()
        },
        LocalParams {
            noise: f32::INFINITY,
            ..Default::default()
        },
    ] {
        assert!(
            ctx.locals_resident(&source, Extent::new(1, 1), &[group(params)])
                .is_err()
        );
    }
    for amount in [-1., 201., f32::NAN] {
        let mut g = group(Default::default());
        g.amount = amount;
        assert!(
            ctx.locals_resident(&source, Extent::new(1, 1), &[g])
                .is_err()
        );
    }
    for kind in [
        MaskKind::Subject { model: None },
        MaskKind::Depth {
            range: [0., 1.],
            feather: 0.,
            model: None,
        },
        MaskKind::Linear {
            start: [0., 0.],
            end: [0., 0.],
        },
        MaskKind::Radial {
            center: [0.5, 0.5],
            radii: [0., 1.],
            angle: 0.,
            feather: 50.,
        },
    ] {
        let mut g = group(Default::default());
        g.components = vec![MaskComponent::new(kind)];
        assert!(
            ctx.locals_resident(&source, Extent::new(1, 1), &[g])
                .is_err()
        );
    }
    assert!(
        ctx.locals_resident(&source, Extent::new(2, 1), &[])
            .is_err()
    );
    assert!(
        ctx.locals_resident(&source, Extent::new(0, 1), &[])
            .is_err()
    );
}
#[test]
fn local_point_operators_match_cpu() {
    for (name, p) in [
        (
            "contrast",
            LocalParams {
                contrast: 37.,
                ..Default::default()
            },
        ),
        (
            "highlights",
            LocalParams {
                highlights: -61.,
                ..Default::default()
            },
        ),
        (
            "shadows",
            LocalParams {
                shadows: 43.,
                ..Default::default()
            },
        ),
        (
            "whites",
            LocalParams {
                whites: 31.,
                ..Default::default()
            },
        ),
        (
            "blacks",
            LocalParams {
                blacks: -25.,
                ..Default::default()
            },
        ),
        (
            "temperature",
            LocalParams {
                temperature: 25.,
                ..Default::default()
            },
        ),
        (
            "tint",
            LocalParams {
                tint: -30.,
                ..Default::default()
            },
        ),
        (
            "saturation",
            LocalParams {
                saturation: 41.,
                ..Default::default()
            },
        ),
        (
            "hue",
            LocalParams {
                hue: -63.,
                ..Default::default()
            },
        ),
    ] {
        eprintln!("operator {name}");
        check(&[group(p)], 1e-4);
    }
}
#[test]
fn neutral_skipped_groups_and_parameter_scaling() {
    check(&[group(Default::default())], 0.);
    let mut disabled = group(LocalParams {
        exposure: f32::NAN,
        ..Default::default()
    });
    disabled.enabled = false;
    let mut zero = disabled.clone();
    zero.enabled = true;
    zero.amount = 0.;
    let mut empty = disabled.clone();
    empty.enabled = true;
    empty.components.clear();
    empty.invert = true;
    check(&[disabled, zero, empty], 0.);
    for amount in [0.01, 50., 200.] {
        let mut g = group(LocalParams {
            exposure: -1.7,
            contrast: -43.,
            highlights: 59.,
            shadows: -49.,
            whites: -31.,
            blacks: 28.,
            temperature: -23.,
            tint: 18.,
            hue: 721.,
            saturation: -54.,
            ..Default::default()
        });
        g.amount = amount;
        check(&[g], 2e-3);
    }
    check(
        &[group(LocalParams {
            moire: 99.,
            ..Default::default()
        })],
        0.,
    );
}
#[test]
#[ignore = "24MP GPU/CPU regression; run explicitly"]
fn locals_24mp() {
    check_at(
        &[group(LocalParams {
            exposure: 0.35,
            saturation: 23.,
            temperature: 12.,
            hue: 27.,
            texture: 31.,
            clarity: -19.,
            dehaze: 17.,
            sharpness: 27.,
            noise: -15.,
            ..Default::default()
        })],
        2e-3,
        Extent::new(6000, 4000),
    );
}
#[test]
fn exposure_and_independent_identity() {
    check(&[], 0.);
    check(
        &[group(LocalParams {
            exposure: 1.2,
            ..Default::default()
        })],
        1e-4,
    );
}
