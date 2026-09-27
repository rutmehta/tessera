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

#[test]
fn live_edits_reuse_distant_composites() {
    let mut d = doc(Extent::new(2048, 512), Depth::F32);
    let model = typography::TextModel::point("Hello", "Noto Sans", 80.);
    let id = add(
        &mut d,
        None,
        Layer::new(
            "text",
            LayerKind::Text {
                model: model.clone(),
                transform: Affine::IDENTITY,
            },
        ),
    );
    let comp = Compositor::new(64 << 20);
    comp.set_text_renderer(fonts());
    comp.render_level_rgba(&d, 0).unwrap();
    comp.reset_stats();
    let mut edited = model;
    edited.runs[0].text.push('!');
    d.apply(DocOp::EditText {
        id,
        model: edited,
        transform: Affine::IDENTITY,
    })
    .unwrap();
    let actual = comp.render_level_rgba(&d, 0).unwrap();
    let stats = comp.stats();
    assert!(stats.root_full + stats.root_partial <= 2, "{stats:?}");
    assert!(stats.live_tiles <= 2, "{stats:?}");
    let cold = Compositor::new(64 << 20);
    cold.set_text_renderer(fonts());
    assert_eq!(actual, cold.render_level_rgba(&d, 0).unwrap());
}

#[test]
fn moving_shape_erases_old_tiles_and_preserves_distant_tiles() {
    let mut d = doc(Extent::new(1536, 256), Depth::U8);
    let model = vector::ShapeModel {
        path: vector::Shape::Ellipse {
            center: vector::Point::new(80., 90.),
            radii: vector::Vec2::new(50., 35.),
        }
        .path()
        .unwrap(),
        fill: Some(vector::Fill::Solid([0.7, 0.3, 0.2, 0.6])),
        stroke: Some((
            vector::Stroke {
                width: 8.,
                ..Default::default()
            },
            vector::Fill::Solid([1., 1., 1., 0.8]),
        )),
        ..Default::default()
    };
    let id = add(
        &mut d,
        None,
        Layer::new(
            "shape",
            LayerKind::Shape {
                model: model.clone(),
                transform: Affine::IDENTITY,
            },
        ),
    );
    let comp = Compositor::new(64 << 20);
    let first = comp.render_level_rgba(&d, 0).unwrap();
    comp.reset_stats();
    d.apply(DocOp::EditShape {
        id,
        model,
        transform: Affine::scale_translate(1., 1., 520., 0.),
    })
    .unwrap();
    let actual = comp.render_level_rgba(&d, 0).unwrap();
    let stats = comp.stats();
    assert!(stats.root_full + stats.root_partial <= 2, "{stats:?}");
    assert_eq!(actual, Compositor::new(0).render_level_rgba(&d, 0).unwrap());
    assert_eq!(px(&actual.1, 1536, 80, 90), [0.; 4]);
    assert!(d.undo());
    assert_eq!(comp.render_level_rgba(&d, 0).unwrap(), first);
    assert!(d.redo());
    assert_eq!(comp.render_level_rgba(&d, 0).unwrap(), actual);
}

#[test]
fn appending_only_rasterizes_new_glyph_coverage() {
    let mut d = doc(Extent::new(1800, 400), Depth::F32);
    let mut model = typography::TextModel::point("MMMMMMMM", "Noto Sans", 80.);
    model.runs[0].kerning = false;
    let id = add(
        &mut d,
        None,
        Layer::new(
            "text",
            LayerKind::Text {
                model: model.clone(),
                transform: Affine::IDENTITY,
            },
        ),
    );
    let comp = Compositor::new(64 << 20);
    comp.set_text_renderer(fonts());
    comp.render_level_rgba(&d, 1).unwrap();
    comp.reset_stats();
    model.runs[0].text.push('M');
    d.apply(DocOp::EditText {
        id,
        model,
        transform: Affine::IDENTITY,
    })
    .unwrap();
    comp.render_level_rgba(&d, 1).unwrap();
    let s = comp.stats();
    assert_eq!(s.live_preparations, 1, "{s:?}");
    assert_eq!(s.live_coverages, 1, "{s:?}");
    assert_eq!(s.root_partial, 1, "{s:?}");
    comp.clear_composites();
    comp.render_level_rgba(&d, 1).unwrap();
    assert_eq!(comp.stats().live_coverages, 1);
}

#[test]
fn paragraph_edits_deletion_transform_and_reflow_match_cold() {
    let mut d = doc(Extent::new(1025, 769), Depth::F32);
    let mut model = typography::TextModel::point(
        "First paragraph.\nSecond ffi paragraph e\u{301}.",
        "Noto Sans",
        47.,
    );
    model.text_box = typography::TextBox::Paragraph {
        width: 470.,
        height: 700.,
    };
    let mut transform = Affine::scale_translate(1., 1., 230.25, 12.5);
    let id = add(
        &mut d,
        None,
        Layer::new(
            "paragraph",
            LayerKind::Text {
                model: model.clone(),
                transform,
            },
        ),
    );
    let comp = Compositor::new(64 << 20);
    comp.set_text_renderer(fonts());
    for step in 0..8 {
        match step {
            1 => model.runs[0].text.insert_str(5, " very long changed text"),
            2 => model.runs[0].text = "Short.\nSecond ffi paragraph e\u{301}.".into(),
            3 => model.paragraph.alignment = typography::Alignment::Center,
            4 => model.runs[0].color = [80, 180, 30, 170],
            5 => {
                transform = Affine {
                    m: [0.8, -0.25, 420.5, 0.2, 0.9, 60.25],
                }
            }
            6 => model.warp.amount = 0.15,
            7 => model.runs.clear(),
            _ => {}
        }
        d.apply(DocOp::EditText {
            id,
            model: model.clone(),
            transform,
        })
        .unwrap();
        for level in [0, 2, 3] {
            let actual = comp.render_level_rgba(&d, level).unwrap();
            let cold = Compositor::new(0);
            cold.set_text_renderer(fonts());
            assert_eq!(
                actual,
                cold.render_level_rgba(&d, level).unwrap(),
                "step {step} level {level}"
            );
        }
    }
}

#[test]
fn live_masks_group_properties_and_style_halos_remain_conservative() {
    use compositor::render::styles::{Shadow, StyleEffect};
    for styled in [false, true] {
        let mut d = doc(Extent::new(520, 100), Depth::F32);
        let group = add(&mut d, None, Layer::group("group", GroupMode::Isolated));
        let model = vector::ShapeModel {
            path: vector::Shape::Rectangle {
                rect: vector::Rect::new(235., 20., 250., 65.),
                radii: [0.; 4],
            }
            .path()
            .unwrap(),
            fill: Some(vector::Fill::Solid([0.8, 0.2, 0.1, 0.8])),
            ..Default::default()
        };
        let mut layer = Layer::new(
            "shape",
            LayerKind::Shape {
                model: model.clone(),
                transform: Affine::IDENTITY,
            },
        );
        if styled {
            layer
                .props
                .styles
                .effects
                .push(StyleEffect::DropShadow(Shadow {
                    distance: 35.,
                    size: 7.,
                    opacity: 0.7,
                    ..Default::default()
                }));
        }
        let id = add(&mut d, Some(group), layer);
        let c = Compositor::new(32 << 20);
        for step in 0..5 {
            match step {
                1 => {
                    d.apply(DocOp::EditShape {
                        id,
                        model: model.clone(),
                        transform: Affine::scale_translate(1., 1., 25., 0.),
                    })
                    .unwrap();
                }
                2 => {
                    d.apply(DocOp::SetVectorMask {
                        id,
                        mask: Some(VectorMask {
                            path: model.path.clone(),
                            enabled: true,
                            density: 0.5,
                            feather: 2.,
                        }),
                    })
                    .unwrap();
                }
                3 => {
                    d.apply(DocOp::SetMask {
                        id,
                        mask: Some(Mask::hide_all(d.state().canvas, Depth::F32)),
                    })
                    .unwrap();
                }
                4 => set_props(&mut d, group, |p| p.opacity = 0.3),
                _ => {}
            }
            let actual = c.render_level_rgba(&d, 1).unwrap();
            assert_eq!(
                actual,
                Compositor::new(0).render_level_rgba(&d, 1).unwrap(),
                "styled={styled} step={step}"
            );
        }
    }
}

#[test]
fn tiny_cache_and_font_replacement_do_not_reuse_stale_live_pixels() {
    let mut d = doc(Extent::new(520, 140), Depth::F32);
    let model = typography::TextModel::point("Cache eviction", "Noto Sans", 43.);
    let id = add(
        &mut d,
        None,
        Layer::new(
            "text",
            LayerKind::Text {
                model: model.clone(),
                transform: Affine::IDENTITY,
            },
        ),
    );
    let c = Compositor::new(1024);
    c.set_text_renderer(fonts());
    let first = c.render_level_rgba(&d, 0).unwrap();
    for x in [260., -35., 0.] {
        d.apply(DocOp::EditText {
            id,
            model: model.clone(),
            transform: Affine::scale_translate(1., 1., x, 0.),
        })
        .unwrap();
        let cold = Compositor::new(0);
        cold.set_text_renderer(fonts());
        assert_eq!(
            c.render_level_rgba(&d, 0).unwrap(),
            cold.render_level_rgba(&d, 0).unwrap()
        );
    }
    assert_eq!(c.render_level_rgba(&d, 0).unwrap(), first);
    c.set_text_renderer(typography::TextRenderer::new());
    assert!(c.render_level_rgba(&d, 0).is_err());
    c.set_text_renderer(fonts());
    assert_eq!(c.render_level_rgba(&d, 0).unwrap(), first);
}

#[test]
fn hidden_live_content_never_resolves_fonts() {
    for mode in 0..3 {
        let mut d = doc(Extent::new(10, 10), Depth::F32);
        let mut text = Layer::new(
            "missing",
            LayerKind::Text {
                model: typography::TextModel::point("unavailable", "Missing Test Font", 24.),
                transform: Affine::IDENTITY,
            },
        );
        let parent = if mode == 1 {
            let mut group = Layer::group("hidden", GroupMode::Isolated);
            group.props.visible = false;
            Some(add(&mut d, None, group))
        } else {
            None
        };
        if mode == 0 {
            text.props.visible = false;
        }
        if mode == 2 {
            let mut base = Layer::pixel("hidden clipping base", d.state().canvas, Depth::F32);
            base.props.visible = false;
            add(&mut d, None, base);
            text.props.clipped = true;
        }
        add(&mut d, parent, text);
        let c = Compositor::new(1 << 20);
        c.set_text_renderer(typography::TextRenderer::new());
        assert_eq!(
            c.render_level_rgba(&d, 0).unwrap().1,
            vec![0.; 400],
            "mode {mode}"
        );
    }
}

#[test]
fn invalid_live_tile_coordinates_return_errors_before_bound_scaling() {
    use engine_api::tile::TileCoord;
    let d = shape_doc(Depth::F32);
    let c = Compositor::new(1 << 20);
    for coord in [
        TileCoord::new(255, 0, 0),
        TileCoord::new(0, u32::MAX, 0),
        TileCoord::new(compositor::render::MAX_LEVEL, 0, 0),
    ] {
        assert!(c.render_tile(&d, coord).is_err());
    }
}

#[test]
fn very_distant_finite_live_geometry_is_transparent_without_integer_overflow() {
    let mut d = shape_doc(Depth::F32);
    let id = d.state().root[0].id;
    let LayerKind::Shape { model, .. } = d.state().find(id).unwrap().kind.clone() else {
        panic!()
    };
    let c = Compositor::new(1 << 20);
    for offset in [1e100, -1e100] {
        d.apply(DocOp::EditShape {
            id,
            model: model.clone(),
            transform: Affine::scale_translate(1., 1., offset, 0.),
        })
        .unwrap();
        for level in [0, 2, 3] {
            assert!(
                c.render_level_rgba(&d, level)
                    .unwrap()
                    .1
                    .iter()
                    .all(|p| *p == 0.)
            );
        }
    }
}
