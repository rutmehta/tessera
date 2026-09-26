mod common;
use common::*;
use compositor::{
    adjust::shadows::ShadowsHighlights, gpu::GpuCompositor, resident::ResidentRenderer, *,
};
use engine_api::tile::Extent;

#[test]
fn spatial_shadows_gpu_l0_l2_cross_tile_alpha_groups() {
    let gpu = GpuCompositor::new().expect("M5-28 requires a real GPU");
    let e = Extent::new(1040, 12);
    for (mode, clipped) in [
        (GroupMode::Isolated, false),
        (GroupMode::PassThrough, false),
        (GroupMode::Isolated, true),
    ] {
        let mut d = doc(e, Depth::F32);
        add(
            &mut d,
            None,
            layer_fn("outside", e, Depth::F32, |_, _| [0.3, 0.1, 0.2, 0.3]),
        );
        let group = add(&mut d, None, Layer::group("group", mode));
        set_props(&mut d, group, |p| p.opacity = 0.65);
        let target = add(
            &mut d,
            Some(group),
            layer_fn("pixels", e, Depth::F32, |x, y| {
                let v = if x % 37 < 18 { 0.08 } else { 0.8 };
                [
                    v,
                    v * 0.7,
                    v * 0.9,
                    if x % 29 == 0 {
                        0.0
                    } else {
                        0.3 + (y % 5) as f32 * 0.14
                    },
                ]
            }),
        );
        for radius in [5.0, 3.0] {
            let adj = add(
                &mut d,
                Some(group),
                Layer::new(
                    "local",
                    LayerKind::Adjustment(Adjustment::ShadowsHighlights {
                        settings: ShadowsHighlights {
                            shadows_amount: 0.7,
                            highlights_amount: 0.5,
                            shadows_radius: radius,
                            highlights_radius: radius + 2.0,
                            ..Default::default()
                        },
                    }),
                ),
            );
            set_props(&mut d, adj, |p| {
                p.opacity = 0.8;
                p.clipped = clipped;
                p.blend_mode = BlendMode::SoftLight;
            });
            let mut mask = Mask::reveal_all(e, Depth::F32);
            mask.raster
                .edit_region(Rect::of_extent(e), 1, |x, _, p| {
                    p[0] = 0.2 + (x % 13) as f32 / 16.0
                })
                .unwrap();
            d.apply(DocOp::SetMask {
                id: adj,
                mask: Some(mask),
            })
            .unwrap();
        }
        let cpu = Compositor::new(64 << 20);
        let mut r = ResidentRenderer::new(&gpu).unwrap();
        for revision in 0..2 {
            if revision == 1 {
                let op = paint_op(
                    d.state(),
                    target,
                    PaintTarget::Content,
                    Rect::new(255, 0, 258, 12),
                    |_, _, p| *p = [0.02, 0.01, 0.01, 0.95],
                )
                .unwrap();
                d.apply(op).unwrap();
            }
            for level in [0, 2] {
                r.render_viewport(&d, level, Rect::new(250, 0, 260, 2), 0)
                    .unwrap();
                r.render(&d, level).unwrap();
                for got in r.read_tiles(level).unwrap() {
                    let want = cpu.render_tile_with_neighbourhood(&d, got.coord()).unwrap();
                    let n = got.samples::<f32>().unwrap().len() / 4;
                    let a = want.samples::<f32>().unwrap();
                    let b = got.samples::<f32>().unwrap();
                    for i in 0..n {
                        for c in 0..4 {
                            let expected = if c < 3 {
                                a[c * n + i] * a[3 * n + i]
                            } else {
                                a[3 * n + i]
                            };
                            assert!(
                                (expected - b[c * n + i]).abs() < 1e-4,
                                "{mode:?} L{level} {i}/{c}: {expected} {}",
                                b[c * n + i]
                            );
                        }
                    }
                }
                let (_, expected) = cpu.render_level_rgba(&d, level).unwrap();
                let (_, actual) = r.read_level(level, false).unwrap();
                for (a, b) in expected.iter().zip(actual.iter()) {
                    assert!((a - b).abs() <= 1e-4, "straight alpha parity: {a} != {b}");
                }
                assert_eq!(r.render(&d, level).unwrap().blocks, 0);
            }
        }
    }
}

#[test]
fn hdr_methods_gpu_l0_l2_alpha_and_seams() {
    use compositor::adjust::hdr::{HdrMethod, HdrToning};
    let gpu = GpuCompositor::new().expect("M5-28 requires a real GPU");
    let e = Extent::new(1040, 8);
    for method in [
        HdrMethod::LocalAdaptation,
        HdrMethod::EqualizeHistogram,
        HdrMethod::ExposureGamma,
        HdrMethod::HighlightCompression,
    ] {
        for mode in [GroupMode::Isolated, GroupMode::PassThrough] {
            let mut d = doc(e, Depth::F32);
            add(
                &mut d,
                None,
                layer_fn("outside", e, Depth::F32, |_, _| [0.1, 0.2, 0.15, 0.2]),
            );
            let group = add(&mut d, None, Layer::group("HDR group", mode));
            set_props(&mut d, group, |p| p.opacity = 0.6);
            add(
                &mut d,
                Some(group),
                layer_fn("HDR", e, Depth::F32, |x, y| {
                    let v = (x % 41) as f32 / 8.0;
                    [
                        v,
                        v * 0.6,
                        v * 1.2,
                        if x % 17 == 0 {
                            0.0
                        } else {
                            0.2 + (y % 5) as f32 * 0.17
                        },
                    ]
                }),
            );
            let settings = HdrToning {
                method,
                radius: 7.0,
                strength: 0.7,
                gamma: 1.3,
                exposure: 0.25,
                detail: 0.2,
                shadows: 0.1,
                highlights: 0.2,
                vibrance: 0.25,
                saturation: -0.1,
                curve: compositor::adjust::Curve(vec![[0.0, 0.0], [0.4, 0.5], [1.0, 1.0]]),
                equalize_map: vec![0.0, 0.15, 0.6, 0.85, 1.0],
                equalize_max: 6.0,
            };
            let id = add(
                &mut d,
                Some(group),
                Layer::new(
                    "HDR toning",
                    LayerKind::Adjustment(Adjustment::HdrToning { settings }),
                ),
            );
            set_props(&mut d, id, |p| p.opacity = 0.85);
            let cpu = Compositor::new(64 << 20);
            let mut resident = ResidentRenderer::new(&gpu).unwrap();
            for level in [0, 2] {
                resident.render(&d, level).unwrap();
                for got in resident.read_tiles(level).unwrap() {
                    let expected = cpu.render_tile_premultiplied(&d, got.coord()).unwrap();
                    for (i, (&a, &b)) in expected
                        .samples::<f32>()
                        .unwrap()
                        .iter()
                        .zip(got.samples::<f32>().unwrap())
                        .enumerate()
                    {
                        assert!(
                            (a - b).abs() <= 1e-4,
                            "{method:?} L{level} sample {i}: {a} {b}"
                        );
                    }
                }
            }
        }
    }
}
