mod common;
use common::*;
use compositor::*;
use engine_api::{jobs::CancellationToken, tile::Extent};

#[test]
fn inside_dashes_prepare_once_across_tiles_and_cache_each_level() {
    let extent = Extent::new(2133, 1177);
    let mut d = doc(extent, Depth::U8);
    let make = |dx: f64| vector::ShapeModel {
        path: vector::Path::polyline(
            &[
                vector::Point::new(90.25, 130.75),
                vector::Point::new(1310.5 + dx, 185.1),
                vector::Point::new(1500.3, 990.8),
            ],
            true,
        ),
        fill: Some(vector::Fill::Solid([0.7, 0.1, 0.8, 0.43])),
        stroke: Some((
            vector::Stroke {
                width: 24.,
                alignment: vector::Alignment::Inside,
                dashes: vec![32., 16.],
                ..Default::default()
            },
            vector::Fill::Solid([0.1, 0.9, 0.2, 0.7]),
        )),
        ..Default::default()
    };
    let id = add(
        &mut d,
        None,
        Layer::new(
            "dashes",
            LayerKind::Shape {
                model: make(0.),
                transform: Affine::IDENTITY,
            },
        ),
    );
    let comp = Compositor::new(64 << 20);
    for dx in [0., 27.5] {
        d.apply(DocOp::EditShape {
            id,
            model: make(dx),
            transform: Affine::IDENTITY,
        })
        .unwrap();
        comp.reset_stats();
        for level in [2, 3] {
            let actual = comp.render_level_rgba(&d, level).unwrap().1;
            assert_eq!(
                comp.stats().live_preparations,
                1,
                "not once per tile or level"
            );
            let before = comp.stats();
            let repeat = comp.render_level_rgba(&d, level).unwrap().1;
            assert_eq!(before.live_coverages, comp.stats().live_coverages);
            assert_eq!(before.live_tiles, comp.stats().live_tiles);
            let cold = Compositor::new(64 << 20)
                .render_level_rgba(&d, level)
                .unwrap()
                .1;
            for other in [repeat, cold] {
                assert_eq!(
                    actual.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                    other.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
                );
            }
        }
    }
}

#[test]
fn rgba_frame_matches_planar_reference_after_live_edits() {
    // Odd canvas/tile edges, partial alpha and HDR values exercise conversion,
    // rather than only the opaque-photo fast case.
    for depth in [Depth::U8, Depth::U16, Depth::F32] {
        let extent = Extent::new(533, 277);
        let mut d = doc(extent, depth);
        add(
            &mut d,
            None,
            layer_fn("pixels", extent, depth, |x, y| {
                [x as f32 / 211., y as f32 / 133., -0.2, (x % 7) as f32 / 6.]
            }),
        );
        let make = |dx: f64| vector::ShapeModel {
            path: vector::Path::polyline(
                &[
                    vector::Point::new(21.25 + dx, 18.75),
                    vector::Point::new(290.5, 45.1),
                    vector::Point::new(320.3, 266.8),
                ],
                true,
            ),
            fill: Some(vector::Fill::Solid([0.7, 0.1, 0.8, 0.43])),
            ..Default::default()
        };
        let id = add(
            &mut d,
            None,
            Layer::new(
                "live",
                LayerKind::Shape {
                    model: make(0.),
                    transform: Affine::IDENTITY,
                },
            ),
        );
        let comp = Compositor::new(32 << 20);
        for dx in [0., 11.5, -9.] {
            d.apply(DocOp::EditShape {
                id,
                model: make(dx),
                transform: Affine::IDENTITY,
            })
            .unwrap();
            for level in [0, 1, 2, 3] {
                let (e, actual) = comp.render_level_rgba(&d, level).unwrap();
                let tiles = comp
                    .render_level(&d, level, &CancellationToken::new())
                    .unwrap();
                let expected = render::interleave(e, &tiles).unwrap();
                assert_eq!(
                    actual.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                    expected.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                    "{depth:?} L{level}"
                );
            }
        }
    }
}
