mod common;

use common::*;
use compositor::*;
use engine_api::tile::Extent;

fn fonts() -> typography::TextRenderer {
    let mut renderer = typography::TextRenderer::new();
    renderer.fonts_mut().load_font_data(
        include_bytes!("../../typography/tests/fonts/NotoSans-Regular.ttf").to_vec(),
    );
    renderer
}

#[test]
fn resident_text_matches_cpu_after_run_edit_undo_and_font_reset() {
    // This verification deliberately requires a real adapter, not an early-return pass.
    let gpu = compositor::gpu::GpuCompositor::new().expect("Metal adapter for live text parity");
    for depth in [Depth::U8, Depth::U16, Depth::F32] {
        let mut document = doc(Extent::new(96, 64), depth);
        let id = add(
            &mut document,
            None,
            Layer::new(
                "live text",
                LayerKind::Text {
                    model: typography::TextModel::point("office", "Noto Sans", 18.),
                    transform: Affine {
                        m: [1., 0.15, 6.25, -0.1, 1., 9.5],
                    },
                },
            ),
        );
        document
            .apply(DocOp::SetVectorMask {
                id,
                mask: Some(VectorMask {
                    path: vector::Shape::Ellipse {
                        center: vector::Point::new(42., 25.),
                        radii: vector::Vec2::new(35., 20.),
                    }
                    .path()
                    .unwrap(),
                    enabled: true,
                    density: 0.65,
                    feather: 1.25,
                }),
            })
            .unwrap();
        let cpu = Compositor::new(4 << 20);
        cpu.set_text_renderer(fonts());
        let mut resident = compositor::resident::ResidentRenderer::new(&gpu).unwrap();
        resident.set_text_renderer(fonts());
        for phase in 0..4 {
            if phase == 1 {
                document
                    .apply(DocOp::EditTextRuns {
                        id,
                        range: 0..1,
                        runs: vec![typography::TextRun {
                            text: "Hi".into(),
                            family: "Noto Sans".into(),
                            size: 26.,
                            color: [35, 120, 210, 180],
                            ..Default::default()
                        }],
                    })
                    .unwrap();
            } else if phase == 2 {
                assert!(document.undo());
            } else if phase == 3 {
                cpu.set_text_renderer(fonts());
                resident.set_text_renderer(fonts());
            }
            for level in 0..3 {
                resident.render(&document, level).unwrap();
                let tiles = resident.read_tiles(level).unwrap();
                assert!(!tiles.is_empty());
                for tile in tiles {
                    let expected = cpu
                        .render_tile_premultiplied(&document, tile.coord())
                        .unwrap();
                    assert!(expected.samples::<f32>().unwrap().iter().any(|v| *v > 0.));
                    assert_eq!(
                        expected.samples::<f32>().unwrap(),
                        tile.samples::<f32>().unwrap(),
                        "depth={depth:?}, phase={phase}, level={level}"
                    );
                }
            }
        }
    }
}
