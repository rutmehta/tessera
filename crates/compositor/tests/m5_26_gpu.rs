//! M5-26: exact resident pointwise adjustments, interpreter and specialization.
mod common;
use common::*;
use compositor::{gpu::GpuCompositor, resident::ResidentRenderer, *};
use engine_api::tile::Extent;

fn exact(a: Adjustment) {
    exact_fixture(a, false);
}

fn exact_fixture(a: Adjustment, hdr: bool) {
    let gpu = GpuCompositor::new().expect("M5-26 exactness requires a real GPU");
    for depth in [Depth::F32, Depth::U8, Depth::U16] {
        let e = Extent::new(31, 19);
        let mut d = doc(e, depth);
        add(
            &mut d,
            None,
            layer_fn("pixels", e, depth, |x, y| {
                let mut p = [
                    ((x * 7 + y * 3) % 97) as f32 / 96.0,
                    ((x * 13 + y * 11) % 89) as f32 / 88.0,
                    ((x * 3 + y * 17) % 79) as f32 / 78.0,
                    if x < 5 {
                        0.0
                    } else {
                        ((x + y) % 9) as f32 / 8.0
                    },
                ];
                if hdr && depth == Depth::F32 {
                    for v in &mut p[..3] {
                        *v = *v * 2.5 - 0.25;
                    }
                }
                p
            }),
        );
        add(
            &mut d,
            None,
            Layer::new("adjustment", LayerKind::Adjustment(a.clone())),
        );
        let cpu = Compositor::new(32 << 20);
        let mut r = ResidentRenderer::new(&gpu).unwrap();
        r.set_specialization(false);
        for specialized in [false, true] {
            r.set_specialization(specialized);
            if specialized {
                r.invalidate();
                r.render(&d, 0).unwrap();
                r.wait_for_specializations();
                assert_eq!(r.specialized_pipeline_count(), 1);
            }
            for level in [0, 2] {
                r.invalidate();
                r.render(&d, level).unwrap();
                for g in r.read_tiles(level).unwrap() {
                    let want = cpu.render_tile_premultiplied(&d, g.coord()).unwrap();
                    for (i, (p, q)) in want
                        .samples::<f32>()
                        .unwrap()
                        .iter()
                        .zip(g.samples::<f32>().unwrap())
                        .enumerate()
                    {
                        assert_eq!(
                            p.to_bits(),
                            q.to_bits(),
                            "{a:?} {depth:?} L{level} specialized={specialized} sample={i} cpu={p} gpu={q}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn pointwise_adjustments_exact() {
    use serde_json::json;
    let cases = vec![
        json!({"kind":"desaturate"}),
        json!({"kind":"vibrance","vibrance":37.0,"saturation":-13.0}),
        json!({"kind":"color_balance","shadows":[20,-10,5],"midtones":[-5,15,7],"highlights":[12,-8,-20],"preserve_luminosity":true}),
        json!({"kind":"black_white","sliders":[35,70,45,55,25,85],"tint":[0.8,0.5,0.2]}),
        json!({"kind":"photo_filter","color":[1,0.5,0.1],"density":35,"preserve_luminosity":true}),
        json!({"kind":"gradient_map","stops":[[0,0.1,0.2,0.4],[0.4,0.7,0.3,0.2],[1,0.9,0.8,0.5]],"dither":true,"reverse":true,"method":"classic"}),
        json!({"kind":"gradient_map","stops":[[0,0.1,0.2,0.4],[1,0.9,0.8,0.5]],"dither":false,"reverse":false,"method":"perceptual"}),
        json!({"kind":"gradient_map","stops":[[0,0.1,0.2,0.4],[1,0.9,0.8,0.5]],"dither":false,"reverse":false,"method":"linear"}),
        json!({"kind":"selective_color","colors":[[10,-5,3,8],[-12,7,8,-5],[8,9,-7,10],[0,4,2,1],[3,4,5,6],[12,-6,3,8],[5,7,3,1],[3,-5,4,8],[-5,2,7,-6]],"absolute":true}),
        json!({"kind":"equalize","maps":[[0,0.1,0.7,1],[0.1,0.3,0.5,0.9],[0,0.4,0.6,1]]}),
        json!({"kind":"auto","mode":"color","black":[0.02,0.05,0.03],"white":[0.93,0.97,0.9],"gamma":[1.2,0.8,1.1]}),
        json!({"kind":"match_color","source_layer":42,"source_mean":[0.4,0.5,0.6],"source_std":[0.2,0.3,0.4],"target_mean":[0.3,0.45,0.5],"target_std":[0.3,0.25,0.3],"luminance":110,"color_intensity":85,"fade":15}),
        json!({"kind":"replace_color","color":[0.6,0.3,0.4],"fuzziness":160,"hue":32,"saturation":17,"lightness":-8}),
        json!({"kind":"color_lookup","size":2,"data":[[0.1,0.2,0],[0.9,0.1,0.2],[0,0.8,0.1],[0.8,0.9,0],[0.2,0.1,0.9],[0.9,0,0.8],[0,0.9,0.8],[0.9,0.8,0.9]]}),
    ];
    for case in cases {
        exact(serde_json::from_value(case).unwrap());
    }
}

#[test]
fn shadows_highlights_resident_positive_radius() {
    let gpu = GpuCompositor::new().expect("requires GPU");
    let e = Extent::new(8, 8);
    let mut d = doc(e, Depth::F32);
    add(
        &mut d,
        None,
        layer_fn("pixels", e, Depth::F32, |_, _| [0.2, 0.3, 0.4, 1.0]),
    );
    add(
        &mut d,
        None,
        Layer::new(
            "local",
            LayerKind::Adjustment(Adjustment::ShadowsHighlights {
                settings: compositor::adjust::shadows::ShadowsHighlights {
                    shadows_amount: 0.5,
                    ..Default::default()
                },
            }),
        ),
    );
    let mut r = ResidentRenderer::new(&gpu).unwrap();
    r.render(&d, 0).unwrap();
    assert!(!r.read_tiles(0).unwrap().is_empty());
}

#[test]
fn perceptual_hdr_exact() {
    exact_fixture(
        Adjustment::Vibrance {
            vibrance: 50.0,
            saturation: -30.0,
        },
        true,
    );
    exact_fixture(
        Adjustment::MatchColor {
            source_layer: 42,
            source_mean: [60.0, 5.0, -10.0],
            source_std: [15.0; 3],
            target_mean: [50.0, 0.0, 0.0],
            target_std: [20.0; 3],
            luminance: 110.0,
            color_intensity: 90.0,
            fade: 25.0,
        },
        true,
    );
}

#[test]
fn shadows_highlights_zero_radius_exact() {
    use compositor::adjust::shadows::ShadowsHighlights;
    for settings in [
        ShadowsHighlights::default(),
        ShadowsHighlights {
            shadows_amount: 0.6,
            shadows_radius: 0.0,
            highlights_amount: 0.4,
            highlights_radius: 0.0,
            shadows_tone: 0.7,
            highlights_tone: 0.3,
            color: 0.15,
            midtone: -0.2,
            black_clip: 0.03,
            white_clip: 0.05,
        },
    ] {
        exact(Adjustment::ShadowsHighlights { settings });
    }
}

#[test]
fn pointwise_edge_cases_exact() {
    use compositor::adjust::{AutoMode, GradientMethod};
    for a in [
        Adjustment::Vibrance {
            vibrance: 0.0,
            saturation: 0.0,
        },
        Adjustment::Vibrance {
            vibrance: -100.0,
            saturation: 75.0,
        },
        Adjustment::Vibrance {
            vibrance: 100.0,
            saturation: -100.0,
        },
        Adjustment::PhotoFilter {
            color: [0.2, 0.6, 1.0],
            density: 100.0,
            preserve_luminosity: false,
        },
        Adjustment::ColorBalance {
            shadows: [-100.0, 0.0, 30.0],
            midtones: [10.0; 3],
            highlights: [100.0, -100.0, 0.0],
            preserve_luminosity: false,
        },
        Adjustment::BlackWhite {
            sliders: [0.0, 100.0, 200.0, -50.0, 50.0, 75.0],
            tint: None,
        },
        Adjustment::SelectiveColor {
            colors: [[13.0, -27.0, 19.0, -7.0]; 9],
            absolute: false,
        },
        Adjustment::ReplaceColor {
            color: [0.0; 3],
            fuzziness: 0.0,
            hue: -173.0,
            saturation: -50.0,
            lightness: 25.0,
        },
        Adjustment::Equalize {
            maps: [vec![], vec![0.3], vec![0.0, 0.2, 1.0]],
        },
        Adjustment::Auto {
            mode: AutoMode::Tone,
            black: [0.2, 0.4, 0.8],
            white: [0.2, 0.9, 0.1],
            gamma: [1.0, 0.0, 2.2],
        },
        Adjustment::Auto {
            mode: AutoMode::Contrast,
            black: [0.0; 3],
            white: [1.0; 3],
            gamma: [1.0; 3],
        },
        Adjustment::MatchColor {
            source_layer: 42,
            source_mean: [55.0, 8.0, -12.0],
            source_std: [18.0, 12.0, 9.0],
            target_mean: [45.0, -4.0, 6.0],
            target_std: [20.0, 15.0, 11.0],
            luminance: 95.0,
            color_intensity: 120.0,
            fade: 20.0,
        },
        Adjustment::MatchColor {
            source_layer: 42,
            source_mean: [0.0; 3],
            source_std: [0.0; 3],
            target_mean: [0.0; 3],
            target_std: [0.0; 3],
            luminance: 100.0,
            color_intensity: 100.0,
            fade: 100.0,
        },
    ] {
        exact(a);
    }
    for method in [
        GradientMethod::Classic,
        GradientMethod::Linear,
        GradientMethod::Perceptual,
    ] {
        for stops in [
            vec![],
            vec![[0.4, 0.3, 0.6, 0.1]],
            vec![
                [0.8, 0.9, 0.3, 0.5],
                [0.2, 0.1, 0.6, 0.3],
                [0.2, 0.2, 0.3, 0.4],
            ],
        ] {
            exact(Adjustment::GradientMap {
                stops,
                dither: true,
                reverse: false,
                method,
            });
        }
    }
    let n = 3;
    let data = (0..n * n * n)
        .map(|i| {
            [
                (i % 7) as f32 / 6.0,
                (i % 11) as f32 / 10.0,
                (i % 13) as f32 / 12.0,
            ]
        })
        .collect();
    exact(Adjustment::ColorLookup { size: n, data });
}

#[test]
fn gradient_spatial_dither_multitile_exact() {
    use compositor::adjust::GradientMethod;
    let gpu = GpuCompositor::new().expect("spatial dither requires a real GPU");
    // Both L0 and L2 cross a 256-pixel tile boundary.
    let e = Extent::new(1040, 16);
    for depth in [Depth::F32, Depth::U8, Depth::U16] {
        let mut d = doc(e, depth);
        add(
            &mut d,
            None,
            layer_fn("flat", e, depth, |_, y| {
                [
                    0.5,
                    0.5,
                    0.5,
                    if y < 4 {
                        1.0
                    } else if y < 8 {
                        0.0
                    } else {
                        0.5
                    },
                ]
            }),
        );
        add(
            &mut d,
            None,
            Layer::new(
                "dither",
                LayerKind::Adjustment(Adjustment::GradientMap {
                    stops: vec![],
                    dither: true,
                    reverse: false,
                    method: GradientMethod::Classic,
                }),
            ),
        );
        let cpu = Compositor::new(32 << 20);
        let mut r = ResidentRenderer::new(&gpu).unwrap();
        for specialized in [false, true] {
            r.set_specialization(specialized);
            if specialized {
                r.invalidate();
                r.render(&d, 0).unwrap();
                r.wait_for_specializations();
                assert_eq!(r.specialized_pipeline_count(), 1);
            }
            for level in [0, 2] {
                let (extent, image) = cpu.render_level_rgba(&d, level).unwrap();
                let row: Vec<_> = image[..extent.width as usize * 4]
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|p| p[0].to_bits())
                    .collect();
                assert!(
                    row.windows(2).any(|p| p[0] != p[1]),
                    "flat field must spatially dither"
                );
                assert_ne!(
                    &row[..4],
                    &row[256..260],
                    "dither must not reset at tile boundaries"
                );
                r.invalidate();
                r.render(&d, level).unwrap();
                let tiles = r.read_tiles(level).unwrap();
                assert!(tiles.len() > 1);
                for g in tiles {
                    let want = cpu.render_tile_premultiplied(&d, g.coord()).unwrap();
                    for (i, (p, q)) in want
                        .samples::<f32>()
                        .unwrap()
                        .iter()
                        .zip(g.samples::<f32>().unwrap())
                        .enumerate()
                    {
                        assert_eq!(
                            p.to_bits(),
                            q.to_bits(),
                            "spatial dither {depth:?} L{level} specialized={specialized} tile={:?} sample={i}",
                            g.coord()
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn brightness_contrast_exact() {
    for legacy in [false, true] {
        exact(Adjustment::BrightnessContrast {
            brightness: 37.0,
            contrast: 42.0,
            legacy,
        });
    }
}
