use compositor::BlendMode;
use compositor::gpu::GpuCompositor;

#[test]
fn resident_revision_reuse_and_whole_level_blend() {
    let gpu = GpuCompositor::new().expect("Metal required for resident gate");
    let mut scene = gpu.resident(5, 3).unwrap();
    let mut pixels = vec![0.25f32; 5 * 3 * 4];
    pixels[45..].fill(1.0);
    assert!(scene.upload(&gpu, 7, 1, &pixels).unwrap());
    assert!(!scene.upload(&gpu, 7, 1, &pixels).unwrap());
    scene
        .composite(&gpu, 0, &[(7, BlendMode::Normal, 1.0)], None)
        .unwrap();
    let result = scene.readback(&gpu, 0).unwrap();
    assert_eq!(result, pixels);
    assert_eq!(scene.upload_count(), 1);
}

#[test]
fn resident_mip_damage_is_local_and_survives_partial_level_requests() {
    let gpu = GpuCompositor::new().unwrap();
    let mut scene = gpu.resident(17, 13).unwrap();
    let mut reference = gpu.resident(17, 13).unwrap();
    let n = 17 * 13;
    let mut samples = vec![0.2; n * 4];
    samples[3 * n..].fill(1.0);
    let steps = [(1, BlendMode::Normal, 1.0)];
    scene.upload(&gpu, 1, 0, &samples).unwrap();
    scene.composite(&gpu, 3, &steps, None).unwrap();
    let cold_texels = scene.mip_texel_count();
    assert_eq!(cold_texels, 9 * 7 + 5 * 4 + 3 * 2);
    // Separate revisions before rendering must union their damage.
    for (revision, x, y, pixel) in [
        (1, 1usize, 1usize, [0.8, 0.4, 0.1, 0.5]),
        (2, 2, 1, [0.1, 0.9, 0.7, 0.3]),
    ] {
        scene
            .upload_region(&gpu, 1, revision, [x as u32, y as u32, 1, 1], &pixel)
            .unwrap();
        for c in 0..4 {
            samples[c * n + y * 17 + x] = pixel[c];
        }
    }
    scene.composite(&gpu, 1, &steps, None).unwrap();
    assert_eq!(scene.mip_texel_count() - cold_texels, 2);
    scene.composite(&gpu, 3, &steps, None).unwrap();
    assert_eq!(scene.mip_texel_count() - cold_texels, 4);
    // Odd bottom-right edge and nontrivial alpha must match cold mips.
    let pixel = [0.9, 0.2, 0.6, 0.4];
    scene
        .upload_region(&gpu, 1, 3, [16, 12, 1, 1], &pixel)
        .unwrap();
    for c in 0..4 {
        samples[c * n + 12 * 17 + 16] = pixel[c];
    }
    scene.composite(&gpu, 3, &steps, None).unwrap();
    assert_eq!(scene.mip_texel_count() - cold_texels, 7);
    reference.upload(&gpu, 1, 3, &samples).unwrap();
    for level in 0..=3 {
        scene.composite(&gpu, level, &steps, None).unwrap();
        reference.composite(&gpu, level, &steps, None).unwrap();
        assert_eq!(
            scene.readback(&gpu, level).unwrap(),
            reference.readback(&gpu, level).unwrap()
        );
    }
    assert_eq!(scene.mip_texel_count() - cold_texels, 7);
}

#[test]
fn resident_mips_preserve_odd_edge_and_revision_updates() {
    let gpu = GpuCompositor::new().unwrap();
    let mut scene = gpu.resident(5, 3).unwrap();
    let mut pixels = vec![0.25f32; 60];
    pixels[45..].fill(1.0);
    scene.upload(&gpu, 1, 0, &pixels).unwrap();
    scene
        .composite(&gpu, 2, &[(1, BlendMode::Normal, 1.0)], None)
        .unwrap();
    assert_eq!(
        scene.readback(&gpu, 2).unwrap(),
        vec![0.25, 0.25, 0.25, 0.25, 0.25, 0.25, 1.0, 1.0]
    );
    pixels[..15].fill(0.75);
    scene.upload(&gpu, 1, 1, &pixels).unwrap();
    scene
        .composite(&gpu, 2, &[(1, BlendMode::Normal, 1.0)], None)
        .unwrap();
    assert_eq!(&scene.readback(&gpu, 2).unwrap()[..2], &[0.75, 0.75]);
}

#[test]
fn resident_adjustment_preserves_alpha_and_dirty_exterior() {
    let gpu = GpuCompositor::new().unwrap();
    let mut scene = gpu.resident(5, 3).unwrap();
    let mut pixels = vec![0.25f32; 60];
    pixels[45..].fill(0.5);
    scene.upload(&gpu, 1, 0, &pixels).unwrap();
    scene
        .composite(&gpu, 0, &[(1, BlendMode::Normal, 1.0)], None)
        .unwrap();
    scene
        .adjust(
            &gpu,
            0,
            &compositor::Adjustment::Invert,
            1.0,
            Some([1, 1, 2, 1]),
        )
        .unwrap();
    let got = scene.readback(&gpu, 0).unwrap();
    for c in 0..4 {
        for i in 0..15 {
            let expected = if c == 3 {
                0.5
            } else if i == 6 || i == 7 {
                0.375
            } else {
                0.125
            };
            assert_eq!(got[c * 15 + i], expected);
        }
    }
}

mod common;
#[test]
fn resident_adjustments_match_cpu() {
    use compositor::*;
    use engine_api::tile::{Extent, TileCoord};
    let gpu = GpuCompositor::new().unwrap();
    let e = Extent::new(17, 13);
    let n = e.area() as usize;
    let pixel = |x: u32, y: u32| [(x as f32) / 16.0, (y as f32) / 12.0, 0.3, 0.6];
    let mut samples = vec![0.0; n * 4];
    for y in 0..13 {
        for x in 0..17 {
            for c in 0..4 {
                samples[c * n + (y * 17 + x) as usize] = pixel(x, y)[c];
            }
        }
    }
    let mut scene = gpu.resident(17, 13).unwrap();
    scene.upload(&gpu, 1, 1, &samples).unwrap();
    for adj in [
        Adjustment::Invert,
        Adjustment::Exposure {
            exposure: 0.7,
            offset: -0.1,
            gamma: 1.3,
        },
        Adjustment::Threshold { level: 0.45 },
        Adjustment::Posterize { levels: 7 },
        Adjustment::Levels {
            master: LevelsChannel {
                gamma: 0.7,
                ..Default::default()
            },
            rgb: [LevelsChannel::default(); 3],
        },
        Adjustment::Curves {
            master: Curve(vec![[0.0, 0.1], [0.4, 0.6], [1.0, 0.9]]),
            rgb: std::array::from_fn(|_| Curve::default()),
        },
        Adjustment::ChannelMixer {
            matrix: [[0.8, 0.2, 0.0], [0.1, 0.7, 0.2], [0.0, 0.3, 0.7]],
            constant: [0.1, 0.0, -0.1],
            monochrome: false,
        },
        Adjustment::HueSaturation {
            hue: -70.0,
            saturation: 30.0,
            lightness: -20.0,
            colorize: false,
        },
        Adjustment::HueSaturation {
            hue: 80.0,
            saturation: 50.0,
            lightness: 20.0,
            colorize: true,
        },
    ] {
        let mut doc = common::doc(e, Depth::F32);
        common::add(
            &mut doc,
            None,
            common::layer_fn("base", e, Depth::F32, pixel),
        );
        common::add(
            &mut doc,
            None,
            Layer::new("adjust", LayerKind::Adjustment(adj.clone())).with_opacity(0.7),
        );
        let want = Compositor::new(1 << 20)
            .render_tile_premultiplied(&doc, TileCoord::new(0, 0, 0))
            .unwrap();
        scene
            .composite(&gpu, 0, &[(1, BlendMode::Normal, 1.0)], None)
            .unwrap();
        scene.adjust(&gpu, 0, &adj, 0.7, None).unwrap();
        let got = scene.readback(&gpu, 0).unwrap();
        let worst = got
            .iter()
            .zip(want.samples::<f32>().unwrap())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(worst <= 1e-4, "{adj:?}: {worst}");
    }
}

#[test]
fn resident_blends_and_mips_match_cpu() {
    use compositor::*;
    use engine_api::tile::{Extent, TileCoord};
    let gpu = GpuCompositor::new().unwrap();
    let e = Extent::new(17, 13);
    let n = e.area() as usize;
    for mode in BlendMode::ALL {
        let mut doc = common::doc(e, Depth::F32);
        let mut scene = gpu.resident(17, 13).unwrap();
        let mut steps = Vec::new();
        for j in 0..2 {
            let pixel = |x: u32, y: u32| {
                [
                    ((x * 7 + y * 13 + j * 11) % 37) as f32 / 36.0,
                    ((x * 3 + y * 7 + j * 5) % 31) as f32 / 30.0,
                    0.3 + j as f32 * 0.4,
                    0.2 + ((x + y + j) % 7) as f32 / 10.0,
                ]
            };
            let m = if j == 0 { BlendMode::Normal } else { mode };
            let id = common::add(
                &mut doc,
                None,
                common::layer_fn("layer", e, Depth::F32, pixel)
                    .with_mode(m)
                    .with_opacity(0.7),
            );
            let mut samples = vec![0.0; n * 4];
            for y in 0..13 {
                for x in 0..17 {
                    for c in 0..4 {
                        samples[c * n + (y * 17 + x) as usize] = pixel(x, y)[c];
                    }
                }
            }
            scene.upload(&gpu, id.0, 1, &samples).unwrap();
            steps.push((id.0, m, 0.7));
        }
        for level in 0..3 {
            scene.composite(&gpu, level, &steps, None).unwrap();
            let got = scene.readback(&gpu, level).unwrap();
            let want = Compositor::new(1 << 20)
                .render_tile_premultiplied(&doc, TileCoord::new(level, 0, 0))
                .unwrap();
            let worst = got
                .iter()
                .zip(want.samples::<f32>().unwrap())
                .map(|(a, b)| (a - b).abs())
                .fold(0.0f32, f32::max);
            assert!(worst <= 1e-4, "{mode:?} L{level}: {worst}");
        }
    }
}

#[test]
fn resident_dab_updates_only_dirty_rectangle() {
    let gpu = GpuCompositor::new().unwrap();
    let mut scene = gpu.resident(17, 13).unwrap();
    let n = 17 * 13;
    let mut samples = vec![0.2; n * 4];
    samples[3 * n..].fill(1.0);
    scene.upload(&gpu, 1, 0, &samples).unwrap();
    scene
        .composite(&gpu, 0, &[(1, BlendMode::Normal, 1.0)], None)
        .unwrap();
    let patch = [0.8, 0.7, 0.6, 0.5, 0.4, 0.3, 1.0, 1.0];
    assert!(
        scene
            .upload_region(&gpu, 1, 1, [3, 4, 2, 1], &patch)
            .unwrap()
    );
    assert!(
        !scene
            .upload_region(&gpu, 1, 1, [3, 4, 2, 1], &patch)
            .unwrap()
    );
    scene
        .composite(&gpu, 0, &[(1, BlendMode::Normal, 1.0)], Some([3, 4, 2, 1]))
        .unwrap();
    let partial = scene.readback(&gpu, 0).unwrap();
    for c in 0..4 {
        for i in 0..n {
            let want = if i == 71 || i == 72 {
                patch[c * 2 + i - 71]
            } else {
                samples[c * n + i]
            };
            assert!((partial[c * n + i] - want).abs() < 1e-6);
        }
    }
    scene
        .composite(&gpu, 0, &[(1, BlendMode::Normal, 1.0)], None)
        .unwrap();
    assert_eq!(partial, scene.readback(&gpu, 0).unwrap());
}
#[test]
fn resident_accepts_twenty_megapixel_extent() {
    let gpu = GpuCompositor::new().unwrap();
    assert!(gpu.resident(5000, 4000).is_ok());
}
