use compositor::psd::{from_psd, to_psd};
use compositor::{Depth, LayerKind};
use psd::{
    AdditionalInfo, Channel, ColorMode, Compression, Layer, LayerSection, PsdDocument, Rect,
    Version,
};
fn tag(key: [u8; 4], data: Vec<u8>) -> AdditionalInfo {
    AdditionalInfo {
        signature: *b"8BIM",
        key,
        data,
    }
}
fn id(out: &mut Vec<u8>, s: &[u8]) {
    out.extend((if s.len() == 4 { 0 } else { s.len() } as u32).to_be_bytes());
    out.extend(s);
}
fn string(out: &mut Vec<u8>, s: &str) {
    let u: Vec<_> = s.encode_utf16().collect();
    out.extend((u.len() as u32).to_be_bytes());
    for c in u {
        out.extend(c.to_be_bytes());
    }
}
fn desc(items: Vec<(&[u8], [u8; 4], Vec<u8>)>) -> Vec<u8> {
    let mut out = vec![0; 4];
    id(&mut out, b"null");
    out.extend((items.len() as u32).to_be_bytes());
    for (k, t, v) in items {
        id(&mut out, k);
        out.extend(t);
        out.extend(v);
    }
    out
}
fn number(n: f64) -> Vec<u8> {
    n.to_be_bytes().to_vec()
}
fn versioned(d: Vec<u8>) -> Vec<u8> {
    [16u32.to_be_bytes().to_vec(), d].concat()
}
fn solid() -> Vec<u8> {
    versioned(desc(vec![
        (
            b"Clr ",
            *b"Objc",
            desc(vec![
                (b"Rd  ", *b"doub", number(255.)),
                (b"Grn ", *b"doub", number(0.)),
                (b"Bl  ", *b"doub", number(0.)),
            ]),
        ),
        (b"opaqueFuture", *b"long", 77i32.to_be_bytes().to_vec()),
    ]))
}
fn path() -> Vec<u8> {
    let mut out = [3u32.to_be_bytes(), 0u32.to_be_bytes()].concat();
    out.extend(0u16.to_be_bytes());
    out.extend(4u16.to_be_bytes());
    out.extend([0u8; 22]);
    for (y, x) in [(0.25f64, 0.25f64), (0.25, 0.75), (0.75, 0.75), (0.75, 0.25)] {
        out.extend(2u16.to_be_bytes());
        for _ in 0..3 {
            out.extend(((y * 16777216.) as i32).to_be_bytes());
            out.extend(((x * 16777216.) as i32).to_be_bytes());
        }
    }
    out
}
fn fixture(tags: Vec<AdditionalInfo>) -> PsdDocument {
    PsdDocument {
        version: Version::Psd,
        width: 32,
        height: 32,
        depth: 8,
        channels: 3,
        color_mode: ColorMode::Rgb,
        color_data: vec![],
        resources: vec![],
        layer_section: LayerSection {
            layers: vec![Layer {
                name: b"Editable".to_vec(),
                bounds: Rect {
                    top: 0,
                    left: 0,
                    bottom: 32,
                    right: 32,
                },
                channels: [0, 1, 2, -1]
                    .into_iter()
                    .map(|id| Channel {
                        id,
                        compression: Compression::Raw,
                        data: vec![0; 1024],
                    })
                    .collect(),
                additional: tags,
                ..Default::default()
            }],
            ..Default::default()
        },
        composite: vec![0; 3072],
        compression: Compression::Raw,
    }
}
fn tysh() -> Vec<u8> {
    let mut out = 1u16.to_be_bytes().to_vec();
    for n in [1f64, 0., 0., 1., 3., 20.] {
        out.extend(n.to_be_bytes());
    }
    out.extend(50u16.to_be_bytes());
    let mut txt = Vec::new();
    string(&mut txt, "Hi");
    let engine=b"<< /EngineDict << /StyleRun << /RunLengthArray [ 2 ] /RunArray [ << /StyleSheet << /StyleSheetData << /Font 0 /FontSize 12 /Tracking 0 /Future 42 >> >> >> ] >> >> /ResourceDict << /FontSet [ << /Name (Arial) >> ] >> /Opaque (keep me) >>";
    let raw = [
        (engine.len() as u32).to_be_bytes().to_vec(),
        engine.to_vec(),
    ]
    .concat();
    out.extend(versioned(desc(vec![
        (b"Txt ", *b"TEXT", txt),
        (b"EngineData", *b"tdta", raw),
    ])));
    out.extend(1u16.to_be_bytes());
    out.extend(versioned(desc(vec![])));
    for n in [0f64, 0., 30., 20.] {
        out.extend(n.to_be_bytes());
    }
    out
}
#[test]
fn byte_built_tysh_edits_write_engine_and_cached_pixels() {
    let source = fixture(vec![tag(*b"TySh", tysh())]);
    let mut doc = from_psd(&source).unwrap();
    let LayerKind::Text { model, transform } = &mut std::sync::Arc::make_mut(&mut doc.root[0]).kind
    else {
        panic!("editable text expected")
    };
    assert_eq!(transform.m, [1., 0., 3., 0., 1., 20.]);
    model.runs[0].text = "Edit".into();
    model.runs[0].size = 14.;
    model.runs[0].color = [0, 255, 0, 255];
    model.text_box = typography::TextBox::Paragraph {
        width: 29.,
        height: 28.,
    };
    model.paragraph.alignment = typography::Alignment::Center;
    let out = to_psd(&doc).unwrap();
    let text = psd::metadata::parse_text(&out.layer_section.layers[0].info(b"TySh").unwrap().data)
        .unwrap();
    assert_eq!(text.text(), Some("Edit"));
    assert!(String::from_utf8_lossy(text.engine_data().unwrap()).contains("/Future 42"));
    let again = reopen_standard(&out);
    let LayerKind::Text { model, .. } = &again.root[0].kind else {
        panic!()
    };
    assert_eq!(model.runs[0].color, [0, 255, 0, 255]);
    assert_eq!(model.paragraph.alignment, typography::Alignment::Center);
    assert!(
        out.layer_section.layers[0]
            .channels
            .iter()
            .find(|c| c.id == -1)
            .unwrap()
            .data
            .iter()
            .any(|v| *v > 0)
    );
}
#[test]
fn byte_built_shape_edits_retain_opaque_fill_and_refresh_path() {
    let source = fixture(vec![tag(*b"vmsk", path()), tag(*b"SoCo", solid())]);
    let mut doc = from_psd(&source).unwrap();
    let LayerKind::Shape { model, .. } = &mut std::sync::Arc::make_mut(&mut doc.root[0]).kind
    else {
        panic!("shape expected")
    };
    assert_eq!(
        model.path.subpaths[0].anchors[0].point,
        vector::Point::new(8., 8.)
    );
    model.fill = Some(vector::Fill::Solid([0., 0., 1., 1.]));
    model.stroke = Some((
        vector::Stroke {
            width: 2.,
            alignment: vector::Alignment::Inside,
            dashes: vec![4., 2.],
            cap: vector::LineCap::Round,
            ..Default::default()
        },
        vector::Fill::Solid([1., 1., 0., 1.]),
    ));
    let out = to_psd(&doc).unwrap();
    let layer = &out.layer_section.layers[0];
    let (d, _) = psd::metadata::parse_descriptor(&layer.info(b"SoCo").unwrap().data[4..]).unwrap();
    assert_eq!(d.get(b"opaqueFuture").unwrap().number(), Some(77.));
    assert!(layer.info(b"vstk").is_some());
    let again = reopen_standard(&out);
    let LayerKind::Shape { model, .. } = &again.root[0].kind else {
        panic!()
    };
    assert_eq!(model.fill, Some(vector::Fill::Solid([0., 0., 1., 1.])));
    let stroke = &model.stroke.as_ref().unwrap().0;
    assert_eq!(stroke.alignment, vector::Alignment::Inside);
    assert_eq!(stroke.dashes, vec![4., 2.]);
}
#[test]
fn pixel_vector_mask_density_feather_survive_file_roundtrip() {
    let mut source = fixture(vec![tag(*b"vsms", path())]);
    for c in &mut source.layer_section.layers[0].channels {
        c.data.fill(255);
    }
    let mut doc = from_psd(&source).unwrap();
    let mask = std::sync::Arc::make_mut(&mut doc.root[0])
        .vector_mask
        .as_mut()
        .unwrap();
    mask.density = 128. / 255.;
    mask.feather = 1.25;
    let out = to_psd(&doc).unwrap();
    let again = from_psd(&PsdDocument::read(&out.write().unwrap()).unwrap()).unwrap();
    let mask = again.root[0].vector_mask.as_ref().unwrap();
    assert_eq!(mask.density, 128. / 255.);
    assert_eq!(mask.feather, 1.25);
    assert_eq!(again.depth, Depth::U8);
}
#[test]
fn shape_independent_vector_mask_has_standard_raster_bridge_and_live_roundtrip() {
    let source = fixture(vec![tag(*b"vmsk", path()), tag(*b"SoCo", solid())]);
    let mut doc = from_psd(&source).unwrap();
    let node = std::sync::Arc::make_mut(&mut doc.root[0]);
    node.vector_mask = Some(compositor::VectorMask {
        path: vector::Shape::Rectangle {
            rect: vector::Rect::new(0., 0., 16., 32.),
            radii: [0.; 4],
        }
        .path()
        .unwrap(),
        ..Default::default()
    });
    let out = to_psd(&doc).unwrap();
    let record = &out.layer_section.layers[0];
    assert!(record.info(b"tvMk").is_some());
    let mask = record.channels.iter().find(|c| c.id == -2).unwrap();
    assert_eq!(mask.data[12 * 32 + 12], 255);
    assert_eq!(mask.data[12 * 32 + 20], 0);
    let again = from_psd(&PsdDocument::read(&out.write().unwrap()).unwrap()).unwrap();
    assert_eq!(again.root[0].vector_mask, doc.root[0].vector_mask);
    assert!(again.root[0].mask.is_none());
    assert!(matches!(again.root[0].kind, LayerKind::Shape { .. }));
}
#[test]
fn native_gradient_live_rectangle_and_warped_text_export_as_adobe_records() {
    use engine_api::tile::Extent;
    let shape = vector::Shape::Rectangle {
        rect: vector::Rect::new(4., 5., 28., 27.),
        radii: [3.; 4],
    };
    let fill = vector::Fill::Gradient(
        vector::Gradient::new(
            vector::GradientKind::Linear,
            vector::Point::new(4., 16.),
            vector::Point::new(28., 16.),
            vec![
                vector::Stop {
                    position: 0.,
                    color: [1., 0., 0., 1.],
                },
                vector::Stop {
                    position: 1.,
                    color: [0., 0., 1., 0.5],
                },
            ],
            false,
        )
        .unwrap(),
    );
    let model = vector::ShapeModel::from_shape(shape.clone(), Some(fill), None).unwrap();
    let mut state = compositor::DocState::new(Extent::new(32, 32), Depth::U8);
    state.root.push(std::sync::Arc::new(compositor::Layer::new(
        "Gradient",
        LayerKind::Shape {
            model,
            transform: Default::default(),
        },
    )));
    let imported = compositor::psd::ImportedPsd::from_state(state).unwrap();
    let out = to_psd(&imported).unwrap();
    let record = &out.layer_section.layers[0];
    assert!(record.info(b"GdFl").is_some());
    assert_eq!(
        &record.info(b"vogk").unwrap().data[..8],
        &[0, 0, 0, 1, 0, 0, 0, 16]
    );
    let again = reopen_standard(&out);
    let LayerKind::Shape { model, .. } = &again.root[0].kind else {
        panic!()
    };
    assert_eq!(model.live_shape, Some(shape));
    let vector::Fill::Gradient(g) = model.fill.as_ref().unwrap() else {
        panic!()
    };
    assert_eq!(g.start, vector::Point::new(4., 16.));
    assert_eq!(g.end, vector::Point::new(28., 16.));
    assert_eq!(g.stops[1].color[3], 0.5);
}
#[test]
fn native_shape_controls_and_affine_survive_without_overriding_external_edits() {
    let shape = vector::Shape::Polygon {
        center: vector::Point::new(12., 12.),
        radius: 8.,
        sides: 5,
        rotation: 0.2,
        inner_radius: Some(4.),
    };
    let model =
        vector::ShapeModel::from_shape(shape, Some(vector::Fill::Solid([1., 0., 0., 1.])), None)
            .unwrap();
    let transform = compositor::Affine::scale_translate(1.2, 0.8, 2., 3.);
    let mut state = compositor::DocState::new(engine_api::tile::Extent::new(32, 32), Depth::U8);
    state.root.push(std::sync::Arc::new(compositor::Layer::new(
        "Star",
        LayerKind::Shape {
            model: model.clone(),
            transform,
        },
    )));
    let out = to_psd(&compositor::psd::ImportedPsd::from_state(state).unwrap()).unwrap();
    let again = from_psd(&out).unwrap();
    let LayerKind::Shape {
        model: actual,
        transform: affine,
    } = &again.root[0].kind
    else {
        panic!()
    };
    assert_eq!(actual, &model);
    assert_eq!(affine, &transform);
    let mut edited = out.clone();
    let b = edited.layer_section.layers[0]
        .additional
        .iter_mut()
        .find(|b| b.key == *b"SoCo")
        .unwrap();
    b.data = solid(); // Different descriptor bytes invalidate the native source bridge.
    let again = from_psd(&edited).unwrap();
    let LayerKind::Shape {
        transform: affine, ..
    } = &again.root[0].kind
    else {
        panic!()
    };
    assert_eq!(*affine, compositor::Affine::default());
}

fn reopen_standard(out: &PsdDocument) -> compositor::psd::ImportedPsd {
    let mut standard = out.clone();
    for layer in &mut standard.layer_section.layers {
        layer
            .additional
            .retain(|b| !matches!(&b.key, b"tvSh" | b"tvTx"));
    }
    from_psd(&PsdDocument::read(&standard.write().unwrap()).unwrap()).unwrap()
}
#[test]
fn text_native_axis_features_and_warp_keep_real_adobe_engine_data() {
    let mut source = fixture(vec![tag(*b"TySh", tysh())]);
    source.width = 32;
    let mut doc = from_psd(&source).unwrap();
    let LayerKind::Text { model, .. } = &mut std::sync::Arc::make_mut(&mut doc.root[0]).kind else {
        panic!()
    };
    model.runs[0].features.insert("liga".into(), 0);
    model.runs[0].axes.insert("wght".into(), 600.);
    model.runs[0].weight = 500;
    model.warp = typography::Warp {
        kind: typography::WarpKind::Wave,
        amount: 0.1,
    };
    let expected = model.clone();
    let out = to_psd(&doc).unwrap();
    let text = psd::metadata::parse_text(&out.layer_section.layers[0].info(b"TySh").unwrap().data)
        .unwrap();
    assert!(text.engine_data().unwrap().starts_with(b"<<"));
    assert_eq!(
        text.warp.get(b"warpValue").unwrap().number(),
        Some(f64::from(0.1f32) * 100.)
    );
    let again = from_psd(&out).unwrap();
    let LayerKind::Text { model, .. } = &again.root[0].kind else {
        panic!()
    };
    assert_eq!(model, &expected);
}
#[test]
fn converted_pixels_do_not_resurrect_source_type_descriptors() {
    for tags in [
        vec![tag(*b"TySh", tysh())],
        vec![tag(*b"vmsk", path()), tag(*b"SoCo", solid())],
    ] {
        let mut doc = from_psd(&fixture(tags)).unwrap();
        let raster = compositor::rasterize_layer(&doc.root[0], doc.canvas, 0).unwrap();
        std::sync::Arc::make_mut(&mut doc.root[0]).kind = LayerKind::Pixel(raster);
        let again = from_psd(&to_psd(&doc).unwrap()).unwrap();
        assert!(
            matches!(again.root[0].kind, LayerKind::Pixel(_)),
            "conversion must discard old editable type descriptors"
        );
    }
}
fn typed_list(values: Vec<Vec<u8>>) -> Vec<u8> {
    let mut b = (values.len() as u32).to_be_bytes().to_vec();
    for v in values {
        b.extend(b"Objc");
        b.extend(v);
    }
    b
}
#[test]
fn adobe_gradient_independent_transparency_stops_are_not_lost() {
    let colors = typed_list(vec![
        desc(vec![
            (b"Lctn", *b"long", 0i32.to_be_bytes().to_vec()),
            (
                b"Clr ",
                *b"Objc",
                desc(vec![
                    (b"Rd  ", *b"doub", number(255.)),
                    (b"Grn ", *b"doub", number(0.)),
                    (b"Bl  ", *b"doub", number(0.)),
                ]),
            ),
        ]),
        desc(vec![
            (b"Lctn", *b"long", 4096i32.to_be_bytes().to_vec()),
            (
                b"Clr ",
                *b"Objc",
                desc(vec![
                    (b"Rd  ", *b"doub", number(0.)),
                    (b"Grn ", *b"doub", number(0.)),
                    (b"Bl  ", *b"doub", number(255.)),
                ]),
            ),
        ]),
    ]);
    let alpha = typed_list(
        [(0i32, 100.), (2048, 0.), (4096, 100.)]
            .into_iter()
            .map(|(pos, opacity)| {
                desc(vec![
                    (b"Lctn", *b"long", pos.to_be_bytes().to_vec()),
                    (b"Opct", *b"doub", number(opacity)),
                ])
            })
            .collect(),
    );
    let gradient = versioned(desc(vec![(
        b"Grad",
        *b"Objc",
        desc(vec![
            (b"Clrs", *b"VlLs", colors),
            (b"Trns", *b"VlLs", alpha),
        ]),
    )]));
    let doc = from_psd(&fixture(vec![
        tag(*b"vmsk", path()),
        tag(*b"GdFl", gradient),
    ]))
    .unwrap();
    let LayerKind::Shape { model, .. } = &doc.root[0].kind else {
        panic!()
    };
    assert_eq!(
        model
            .fill
            .as_ref()
            .unwrap()
            .sample(vector::Point::new(16., 16.))[3],
        0.
    );
}
#[test]
fn adobe_shape_vector_mask_disabled_and_density_affect_fill() {
    for disabled in [false, true] {
        let mut p = path();
        if disabled {
            p[4..8].copy_from_slice(&4u32.to_be_bytes());
        }
        let mut source = fixture(vec![tag(*b"vmsk", p), tag(*b"SoCo", solid())]);
        if !disabled {
            let mut params = vec![0; 18];
            params[16] = 255;
            params[17] = 16;
            params.extend([12, 128]);
            params.extend(1.25f64.to_be_bytes());
            source.layer_section.layers[0].mask_data = params;
        }
        let doc = from_psd(&source).unwrap();
        let (_, pixels) = compositor::Compositor::new(0)
            .render_level_rgba(&compositor::Document::new(doc.state.clone()), 0)
            .unwrap();
        let alpha = pixels[(32 + 1) * 4 + 3];
        if disabled {
            assert_eq!(alpha, 1.);
        } else {
            assert_eq!(doc.root[0].vector_mask.as_ref().unwrap().feather, 1.25);
            let boundary = pixels[(16 * 32 + 8) * 4 + 3];
            assert!(
                boundary > 0.6 && boundary < 0.95,
                "feathered edge: {boundary}"
            );
            assert!(
                (alpha - 127. / 255.).abs() < 0.01,
                "density outside path: {alpha}"
            );
        }
    }
}
