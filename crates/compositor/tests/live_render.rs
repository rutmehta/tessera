mod common;
use common::*;
use compositor::*;
use engine_api::tile::Extent;

#[test]
fn vector_mask_is_rendered_with_density() {
    let mut d = doc(Extent::new(4, 2), Depth::F32);
    let mut l = Layer::new(
        "masked",
        LayerKind::Fill(Fill::Solid {
            color: [1., 0., 0.],
        }),
    );
    l.vector_mask = Some(
        serde_json::from_value(serde_json::json!({
            "enabled": true, "density": 0.5, "feather": 0.0,
            "path": {"fill_rule":"NonZero", "subpaths":[{"closed":true,"anchors":[
                {"point":{"x":0.,"y":0.},"incoming":{"x":0.,"y":0.},"outgoing":{"x":0.,"y":0.}},
                {"point":{"x":2.,"y":0.},"incoming":{"x":2.,"y":0.},"outgoing":{"x":2.,"y":0.}},
                {"point":{"x":2.,"y":2.},"incoming":{"x":2.,"y":2.},"outgoing":{"x":2.,"y":2.}},
                {"point":{"x":0.,"y":2.},"incoming":{"x":0.,"y":2.},"outgoing":{"x":0.,"y":2.}}
            ]}]}
        }))
        .unwrap(),
    );
    add(&mut d, None, l);
    let pixels = render(&d);
    assert_eq!(px(&pixels, 4, 0, 0)[3], 1.0);
    assert_eq!(px(&pixels, 4, 3, 0)[3], 0.5);
}

fn shape_doc(depth: Depth) -> Document {
    let mut d = doc(Extent::new(19, 13), depth);
    let path = vector::Shape::Rectangle {
        rect: vector::Rect::new(0.25, 0.25, 8.5, 7.5),
        radii: [0.; 4],
    }
    .path()
    .unwrap();
    add(
        &mut d,
        None,
        Layer::new(
            "live",
            LayerKind::Shape {
                model: vector::ShapeModel {
                    path,
                    fill: Some(vector::Fill::Solid([0.8, 0.2, 0.1, 0.7])),
                    ..Default::default()
                },
                transform: Affine::scale_translate(1., 1., 3., 2.),
            },
        ),
    );
    d
}

#[test]
fn live_shapes_render_at_each_level_and_model_changes_invalidate() {
    let mut d = shape_doc(Depth::F32);
    let comp = Compositor::new(1 << 20);
    for level in 0..3 {
        let (e, p) = comp.render_level_rgba(&d, level).unwrap();
        let expected = vector::VectorRenderer { tolerance: 0.02 }
            .coverage(
                &vector::Shape::Rectangle {
                    rect: vector::Rect::new(3.25, 2.25, 11.5, 9.5),
                    radii: [0.; 4],
                }
                .path()
                .unwrap(),
                vector::Viewport {
                    width: e.width,
                    height: e.height,
                    origin: vector::Point::ZERO,
                    level,
                },
            )
            .unwrap();
        for (rgba, coverage) in p.as_chunks::<4>().0.iter().zip(expected.data) {
            assert!((rgba[3] - coverage * 0.7).abs() < 1e-6);
        }
    }
    let id = d.state().root[0].id;
    let LayerKind::Shape {
        mut model,
        transform,
    } = d.state().find(id).unwrap().kind.clone()
    else {
        panic!()
    };
    model.fill = Some(vector::Fill::Solid([0., 1., 0., 1.]));
    d.apply(DocOp::EditShape {
        id,
        model,
        transform,
    })
    .unwrap();
    let p = comp.render_level_rgba(&d, 0).unwrap().1;
    assert_eq!(px(&p, 19, 5, 5), [0., 1., 0., 1.]);
    assert!(d.undo());
    assert_eq!(comp.render_level_rgba(&d, 0).unwrap().1, render(&d));
}

fn fonts() -> typography::TextRenderer {
    let mut fonts = typography::TextRenderer::new();
    fonts.fonts_mut().load_font_data(
        include_bytes!("../../typography/tests/fonts/NotoSans-Regular.ttf").to_vec(),
    );
    fonts
}

#[test]
fn text_uses_live_outlines_and_explicit_fonts() {
    let mut d = doc(Extent::new(80, 60), Depth::F32);
    add(
        &mut d,
        None,
        Layer::new(
            "type",
            LayerKind::Text {
                model: typography::TextModel::point("Hi", "Noto Sans", 18.),
                transform: Affine {
                    m: [0., -1., 50., 1., 0., 8.],
                },
            },
        ),
    );
    let comp = Compositor::new(1 << 20);
    comp.set_text_renderer(fonts());
    let (e, p) = comp.render_level_rgba(&d, 0).unwrap();
    assert!(p.as_chunks::<4>().0.iter().filter(|c| c[3] > 0.).count() > 70);
    assert_eq!(px(&p, e.width, 0, 0), [0.; 4]);
    let first = comp.render_level_rgba(&d, 1).unwrap().1;
    comp.clear_composites();
    assert_eq!(first, comp.render_level_rgba(&d, 1).unwrap().1);
}

#[test]
fn feather_has_no_tile_seam_and_multiplies_raster_mask() {
    let e = Extent::new(520, 4);
    let mut d = doc(e, Depth::F32);
    let mut l = Layer::new("mask", LayerKind::Fill(Fill::Solid { color: [1.; 3] }));
    l.vector_mask = Some(VectorMask {
        path: vector::Shape::Rectangle {
            rect: vector::Rect::new(254., -100., 258., 100.),
            radii: [0.; 4],
        }
        .path()
        .unwrap(),
        enabled: true,
        feather: 2.,
        density: 1.,
    });
    let mut m = Mask::reveal_all(e, Depth::F32);
    m.raster = Raster::new(e, 1, Depth::F32, 0.5);
    l.mask = Some(m);
    add(&mut d, None, l);
    let p = render(&d);
    assert!((px(&p, e.width, 255, 1)[3] - px(&p, e.width, 256, 1)[3]).abs() < 1e-7);
    assert!(px(&p, e.width, 250, 1)[3] > 0.);
    assert!(px(&p, e.width, 256, 1)[3] < 0.5);
}

#[test]
fn resident_live_geometry_and_masks_match_cpu() {
    let gpu = match compositor::gpu::GpuCompositor::new() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("GPU unavailable: {e}");
            return;
        }
    };
    for depth in [Depth::F32, Depth::U8] {
        let mut d = shape_doc(depth);
        let id = d.state().root[0].id;
        let mut raster_mask = Mask::reveal_all(d.state().canvas, depth);
        raster_mask.density = 0.6;
        raster_mask.raster = Raster::new(d.state().canvas, 1, depth, 0.0);
        d.apply(DocOp::SetMask {
            id,
            mask: Some(raster_mask),
        })
        .unwrap();
        d.apply(DocOp::SetVectorMask {
            id,
            mask: Some(VectorMask {
                path: vector::Shape::Rectangle {
                    rect: vector::Rect::new(4.25, 0., 10., 13.),
                    radii: [0.; 4],
                }
                .path()
                .unwrap(),
                enabled: true,
                density: 0.7,
                feather: 0.8,
            }),
        })
        .unwrap();
        let cpu = Compositor::new(1 << 20);
        let mut resident = compositor::resident::ResidentRenderer::new(&gpu).unwrap();
        for level in 0..3 {
            resident.render(&d, level).unwrap();
            for tile in resident.read_tiles(level).unwrap() {
                let expected = cpu.render_tile_premultiplied(&d, tile.coord()).unwrap();
                assert_eq!(
                    expected.samples::<f32>().unwrap(),
                    tile.samples::<f32>().unwrap()
                );
            }
        }
    }
}

#[test]
fn combined_mask_obeys_resident_effective_mask_rounding() {
    let e = Extent::new(1, 1);
    let mut d = doc(e, Depth::F32);
    let mut l = Layer::new("masked", LayerKind::Fill(Fill::Solid { color: [1.; 3] }));
    l.vector_mask = Some(VectorMask {
        path: vector::Path::default(),
        enabled: true,
        feather: 0.,
        density: 0.7,
    });
    let mut m = Mask::hide_all(e, Depth::F32);
    m.density = 0.6;
    l.mask = Some(m);
    add(&mut d, None, l);
    // The resident mask ABI applies 1-d*(1-m) to its uploaded samples.
    // Combined masks are uploaded with density=1; match that f32 arithmetic.
    let combined = (1f32 - 0.7) * (1f32 - 0.6);
    let expected = 1f32 - (1f32 - combined);
    assert_eq!(render(&d)[3].to_bits(), expected.to_bits());
}

#[test]
fn source_cache_reuses_model_tiles_after_composite_eviction_and_undo() {
    let mut d = shape_doc(Depth::F32);
    let comp = Compositor::new(1 << 20);
    comp.render_level_rgba(&d, 0).unwrap();
    assert_eq!(comp.stats().live_tiles, 1);
    comp.clear_composites();
    comp.render_level_rgba(&d, 0).unwrap();
    assert_eq!(comp.stats().live_tiles, 1);
    comp.render_level_rgba(&d, 1).unwrap();
    assert_eq!(comp.stats().live_tiles, 2);
    let id = d.state().root[0].id;
    let LayerKind::Shape { model, .. } = d.state().find(id).unwrap().kind.clone() else {
        panic!()
    };
    d.apply(DocOp::EditShape {
        id,
        model,
        transform: Affine::scale_translate(1., 1., 8., 3.),
    })
    .unwrap();
    comp.render_level_rgba(&d, 0).unwrap();
    assert_eq!(comp.stats().live_tiles, 3);
    d.undo();
    comp.clear_composites();
    comp.render_level_rgba(&d, 0).unwrap();
    assert_eq!(comp.stats().live_tiles, 3);
}
