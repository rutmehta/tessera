mod common;
use common::*;
use compositor::{gpu::GpuCompositor, render::styles::*, resident::ResidentRenderer, *};
use engine_api::tile::Extent;

#[test]
fn effects_l0_l2_cross_tile() {
    let gpu = GpuCompositor::new().expect("M5-31 requires a GPU");
    let e = Extent::new(1040, 12);
    let effects = vec![
        StyleEffect::DropShadow(Shadow::default()),
        StyleEffect::InnerShadow(Shadow::default()),
        StyleEffect::OuterGlow(Glow::default()),
        StyleEffect::InnerGlow(Glow::default()),
        StyleEffect::Bevel(Bevel::default()),
        StyleEffect::Satin(Satin::default()),
        StyleEffect::ColorOverlay(Overlay::default()),
        StyleEffect::GradientOverlay(Overlay {
            fill: Fill::Gradient {
                gradient: GradientKind::Linear,
                start: [0.0, 0.0],
                end: [1040.0, 12.0],
                stops: vec![
                    GradientStop {
                        position: 0.0,
                        color: [0.2, 0.4, 0.1, 0.7],
                    },
                    GradientStop {
                        position: 1.0,
                        color: [0.8, 0.1, 0.6, 0.3],
                    },
                ],
            },
            ..Default::default()
        }),
        StyleEffect::PatternOverlay(Overlay {
            fill: Fill::Pattern {
                width: 2,
                height: 1,
                rgba: vec![0.2, 0.3, 0.6, 0.5, 0.8, 0.6, 0.3, 0.9],
                origin: [1.0, -1.0],
            },
            ..Default::default()
        }),
        StyleEffect::Stroke(Stroke::default()),
        StyleEffect::Stroke(Stroke {
            position: StrokePosition::Inside,
            ..Default::default()
        }),
        StyleEffect::Stroke(Stroke {
            position: StrokePosition::Center,
            ..Default::default()
        }),
    ];
    for (index, effect) in effects.into_iter().enumerate() {
        let mut d = doc(e, Depth::F32);
        add(
            &mut d,
            None,
            layer_fn("back", e, Depth::F32, |_, _| [0.2, 0.3, 0.1, 0.6]),
        );
        let id = add(
            &mut d,
            None,
            layer_fn("styled", e, Depth::F32, |x, y| {
                [
                    0.7,
                    0.2,
                    0.5,
                    if x % 23 < 15 && y > 2 && y < 10 {
                        0.6
                    } else {
                        0.0
                    },
                ]
            }),
        );
        set_props(&mut d, id, |p| {
            p.styles.effects = vec![effect];
            p.fill_opacity = 0.4;
            p.opacity = 0.7;
        });
        let cpu = Compositor::new(64 << 20);
        let mut resident = ResidentRenderer::new(&gpu).unwrap();
        for level in [0, 2] {
            let (_, expected) = cpu.render_level_rgba(&d, level).unwrap();
            resident.render(&d, level).unwrap();
            let (_, actual) = resident.read_level(level, false).unwrap();
            for (i, (a, b)) in actual.iter().zip(&expected).enumerate() {
                assert!(
                    (a - b).abs() <= 1e-4,
                    "effect {index}, L{level}, component {i}: {a} != {b}"
                );
            }
            let count = resident.style_evaluations();
            assert_eq!(resident.render(&d, level).unwrap().blocks, 0);
            assert_eq!(resident.style_evaluations(), count);
            resident.wait_for_specializations();
        }
    }
}
