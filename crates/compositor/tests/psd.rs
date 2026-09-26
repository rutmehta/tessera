use compositor::psd::{from_psd, to_psd};
use compositor::{Depth, LayerKind};
use psd::{Channel, ColorMode, Compression, Layer, LayerSection, PsdDocument, Rect, Version};

fn fixture(depth: u16) -> PsdDocument {
    let sample = |v: u8| match depth {
        8 => vec![v],
        16 => (u16::from(v) * 257).to_be_bytes().to_vec(),
        _ => (f32::from(v) / 255.0).to_be_bytes().to_vec(),
    };
    PsdDocument {
        version: Version::Psd,
        width: 2,
        height: 1,
        depth,
        channels: 3,
        color_mode: ColorMode::Rgb,
        color_data: vec![],
        resources: vec![],
        layer_section: LayerSection {
            layers: vec![Layer {
                name: b"pixel".to_vec(),
                bounds: Rect {
                    top: 0,
                    left: 1,
                    bottom: 1,
                    right: 2,
                },
                channels: [(0, 128), (1, 64), (2, 255), (-1, 192)]
                    .into_iter()
                    .map(|(id, v)| Channel {
                        id,
                        compression: Compression::Raw,
                        data: sample(v),
                    })
                    .collect(),
                ..Layer::default()
            }],
            ..LayerSection::default()
        },
        composite: vec![0; 6 * usize::from(depth / 8)],
        compression: Compression::Raw,
    }
}

fn tag(key: &[u8; 4], data: Vec<u8>) -> psd::AdditionalInfo {
    psd::AdditionalInfo {
        signature: *b"8BIM",
        key: *key,
        data,
    }
}

#[test]
fn hue_saturation_master_is_editable_without_losing_extension_data() {
    use compositor::Adjustment;
    for colorize in [false, true] {
        let mut source = fixture(8);
        let mut data = vec![0, 2, u8::from(colorize), 0];
        for v in [120i16, 40, -10, -30, 25, 15] {
            data.extend_from_slice(&v.to_be_bytes());
        }
        data.resize(100, 0);
        data.extend_from_slice(&[9, 8, 7, 6]);
        source.layer_section.layers[0].additional = vec![tag(b"hue2", data.clone())];
        let mut imported = from_psd(&source).unwrap();
        let expected = if colorize {
            [120.0, 40.0, -10.0]
        } else {
            [-30.0, 25.0, 15.0]
        };
        let LayerKind::Adjustment(Adjustment::HueSaturation {
            hue,
            saturation,
            lightness,
            colorize: actual,
        }) = &imported.root[0].kind
        else {
            panic!("hue2 must import as an editable adjustment");
        };
        assert_eq!([*hue, *saturation, *lightness], expected);
        assert_eq!(*actual, colorize);
        assert_eq!(
            to_psd(&imported).unwrap().layer_section.layers,
            source.layer_section.layers
        );
        std::sync::Arc::make_mut(&mut imported.root[0]).kind =
            LayerKind::Adjustment(Adjustment::HueSaturation {
                hue: 45.0,
                saturation: 60.0,
                lightness: -20.0,
                colorize,
            });
        let out = to_psd(&imported).unwrap();
        let edited = &out.layer_section.layers[0].info(b"hue2").unwrap().data;
        assert_eq!(&edited[16..], &data[16..]);
        let again = from_psd(&out).unwrap();
        let LayerKind::Adjustment(a) = &again.root[0].kind else {
            panic!("adjustment")
        };
        let LayerKind::Adjustment(b) = &imported.root[0].kind else {
            panic!("adjustment")
        };
        assert_eq!(a, b);
    }
}

#[test]
fn channel_mixer_imports_signed_rgb_and_monochrome_records() {
    use compositor::Adjustment;
    for monochrome in [false, true] {
        let mut source = fixture(8);
        let mut data = vec![0, 1, 0, u8::from(monochrome)];
        let rows = [
            [120i16, -30, 10, 0, -5],
            [0, 80, 20, 0, 10],
            [20, 0, 80, 0, 0],
            [30, 60, 10, 0, 0],
        ];
        for row in rows {
            for value in row {
                data.extend_from_slice(&value.to_be_bytes());
            }
        }
        data.extend_from_slice(&[9, 8, 7, 6]);
        source.layer_section.layers[0].additional = vec![tag(b"mixr", data.clone())];
        let mut imported = from_psd(&source).unwrap();
        let LayerKind::Adjustment(Adjustment::ChannelMixer {
            matrix,
            constant,
            monochrome: actual,
        }) = &imported.root[0].kind
        else {
            panic!("mixr must be editable")
        };
        assert_eq!(*actual, monochrome);
        assert_eq!(matrix[0], [1.2, -0.3, 0.1]);
        assert_eq!(constant[0], -0.05);
        if !monochrome {
            assert_eq!(matrix[1], [0.0, 0.8, 0.2]);
            assert_eq!(constant[1], 0.1);
        }
        assert_eq!(
            to_psd(&imported).unwrap().layer_section.layers,
            source.layer_section.layers
        );
        let LayerKind::Adjustment(Adjustment::ChannelMixer { matrix, .. }) =
            &mut std::sync::Arc::make_mut(&mut imported.root[0]).kind
        else {
            unreachable!()
        };
        matrix[0][1] = -0.4;
        let exported = to_psd(&imported).unwrap();
        let block = &exported.layer_section.layers[0].info(b"mixr").unwrap().data;
        assert_eq!(
            &block[14..],
            &data[14..],
            "inactive channels and extension bytes must survive"
        );
        let again = from_psd(&PsdDocument::read(&exported.write().unwrap()).unwrap()).unwrap();
        let LayerKind::Adjustment(Adjustment::ChannelMixer { matrix, .. }) = &again.root[0].kind
        else {
            panic!("mixr")
        };
        assert_eq!(matrix[0][1], -0.4);
    }
}

#[test]
fn curves_import_channel_selection_and_editable_export() {
    use compositor::{Adjustment, Curve};
    for version in [1u16, 4] {
        let mut source = fixture(8);
        let mut data = vec![0]; // Control points, not a sampled map.
        data.extend_from_slice(&version.to_be_bytes());
        data.extend_from_slice(&(if version == 1 { 5u32 } else { 2 }).to_be_bytes());
        for points in [
            vec![(0u16, 0u16), (180, 128), (255, 255)],
            vec![(10, 0), (240, 255)],
        ] {
            data.extend_from_slice(&(points.len() as u16).to_be_bytes());
            for (output, input) in points {
                data.extend_from_slice(&output.to_be_bytes());
                data.extend_from_slice(&input.to_be_bytes());
            }
        }
        source.layer_section.layers[0].additional = vec![tag(b"curv", data)];
        let mut imported = from_psd(&source).unwrap();
        let LayerKind::Adjustment(Adjustment::Curves { master, rgb }) = &imported.root[0].kind
        else {
            panic!("curv must import as an editable adjustment")
        };
        assert_eq!(master.0[1], [128.0 / 255.0, 180.0 / 255.0]);
        assert_eq!(
            rgb[usize::from(version == 1)].0,
            [[0.0, 10.0 / 255.0], [1.0, 240.0 / 255.0]]
        );
        assert_eq!(
            to_psd(&imported).unwrap().layer_section.layers,
            source.layer_section.layers
        );
        let edited = Adjustment::Curves {
            master: Curve(vec![[0.0, 0.0], [1.0, 1.0]]),
            rgb: [
                Curve(vec![[0.0, 1.0], [1.0, 0.0]]),
                Curve::default(),
                Curve::default(),
            ],
        };
        std::sync::Arc::make_mut(&mut imported.root[0]).kind = LayerKind::Adjustment(edited);
        let out = to_psd(&imported).unwrap();
        let again = from_psd(&PsdDocument::read(&out.write().unwrap()).unwrap()).unwrap();
        let LayerKind::Adjustment(Adjustment::Curves { master, rgb }) = &again.root[0].kind else {
            panic!("curves")
        };
        assert!(master.is_identity());
        assert_eq!(rgb[0].0, [[0.0, 1.0], [1.0, 0.0]]);
        assert!(rgb[1].is_identity() && rgb[2].is_identity());
    }
}

#[test]
fn properties_blend_keys_and_unknown_blocks_survive_edits() {
    for mode in compositor::BlendMode::ALL {
        let mut source = fixture(8);
        let l = &mut source.layer_section.layers[0];
        l.blend_mode = mode.psd_key();
        l.opacity = 123;
        l.flags = 2;
        l.clipping = 1;
        l.additional = vec![tag(b"iOpa", vec![77]), tag(b"test", vec![1, 2, 3, 4])];
        l.blending_ranges = vec![[1, 20, 220, 250, 2, 30, 210, 240]];
        let mut imported = from_psd(&source).unwrap();
        let l = &imported.root[0];
        assert_eq!(l.props.blend_mode, mode);
        assert_eq!(l.props.opacity, 123.0 / 255.0);
        assert_eq!(l.props.fill_opacity, 77.0 / 255.0);
        assert!(!l.props.visible);
        assert!(l.props.clipped);
        assert_eq!(
            l.props.blend_if.gray.this_layer,
            [1.0 / 255.0, 20.0 / 255.0, 220.0 / 255.0, 250.0 / 255.0]
        );
        assert_eq!(
            to_psd(&imported).unwrap().layer_section.layers,
            source.layer_section.layers
        );
        let l = std::sync::Arc::make_mut(&mut imported.root[0]);
        l.props.opacity = 0.5;
        l.props.name = "renamed λ".into();
        l.props.visible = true;
        l.props.clipped = false;
        let out = to_psd(&imported).unwrap();
        assert_eq!(out.layer_section.layers[0].opacity, 128);
        assert_eq!(out.layer_section.layers[0].clipping, 0);
        assert!(out.layer_section.layers[0].visible());
        assert_eq!(
            out.layer_section.layers[0].info(b"test").unwrap().data,
            [1, 2, 3, 4]
        );
        assert_eq!(from_psd(&out).unwrap().root[0].props.name, "renamed λ");
    }
}

fn divider(kind: u32, blend: &[u8; 4], label: &str) -> Layer {
    Layer {
        name: label.as_bytes().to_vec(),
        additional: vec![tag(
            b"lsct",
            [kind.to_be_bytes().as_slice(), b"8BIM", blend].concat(),
        )],
        ..Layer::default()
    }
}
#[test]
fn nested_groups_preserve_file_order_and_boundary_metadata() {
    let mut source = fixture(8);
    let pixel = source.layer_section.layers.remove(0);
    source.layer_section.layers = vec![
        divider(1, b"pass", "outer"),
        divider(2, b"mul ", "inner"),
        pixel,
        divider(3, b"norm", "inner end"),
        divider(3, b"norm", "outer end"),
    ];
    let imported = from_psd(&source).unwrap();
    assert_eq!(imported.root.len(), 1);
    let LayerKind::Group { mode, children } = &imported.root[0].kind else {
        panic!("group")
    };
    assert_eq!(*mode, compositor::GroupMode::PassThrough);
    assert_eq!(children.len(), 1);
    let LayerKind::Group {
        mode,
        children: nested,
    } = &children[0].kind
    else {
        panic!("nested group")
    };
    assert_eq!(*mode, compositor::GroupMode::Isolated);
    assert_eq!(
        children[0].props.blend_mode,
        compositor::BlendMode::Multiply
    );
    assert_eq!(nested[0].props.name, "pixel");
    assert_eq!(
        to_psd(&imported).unwrap().layer_section.layers,
        source.layer_section.layers
    );
    source.layer_section.layers.pop();
    assert!(from_psd(&source).is_err(), "unbalanced groups must fail");
}

#[test]
fn masks_import_density_feather_flags_and_preserve_payloads() {
    for bits in [8, 16, 32] {
        let mut source = fixture(bits);
        let l = &mut source.layer_section.layers[0];
        l.mask_data = [
            0i32.to_be_bytes(),
            0i32.to_be_bytes(),
            1i32.to_be_bytes(),
            1i32.to_be_bytes(),
        ]
        .concat();
        l.mask_data.extend_from_slice(&[255, 16, 3, 128]);
        l.mask_data.extend_from_slice(&2.5f64.to_be_bytes());
        let data = match bits {
            8 => vec![64],
            16 => (64u16 * 257).to_be_bytes().to_vec(),
            _ => (64.0f32 / 255.0).to_be_bytes().to_vec(),
        };
        l.channels.push(Channel {
            id: -2,
            compression: Compression::Raw,
            data,
        });
        let mut imported = from_psd(&source).unwrap();
        let mask = imported.root[0].mask.as_ref().expect("mask");
        assert!((mask.raster.pixel(0, 0)[0] - 64.0 / 255.0).abs() < 0.00002);
        assert_eq!(mask.raster.pixel(1, 0)[0], 1.0);
        assert_eq!(mask.density, 128.0 / 255.0);
        assert_eq!(mask.feather, 2.5);
        assert!(mask.enabled);
        assert_eq!(
            to_psd(&imported).unwrap().layer_section.layers,
            source.layer_section.layers
        );
        std::sync::Arc::make_mut(&mut imported.root[0])
            .mask
            .as_mut()
            .unwrap()
            .enabled = false;
        assert!(
            !from_psd(&to_psd(&imported).unwrap()).unwrap().root[0]
                .mask
                .as_ref()
                .unwrap()
                .enabled
        );
    }
}

#[test]
fn adjustment_parameters_are_editable_and_unknown_adjustments_stay_opaque() {
    let mut source = fixture(8);
    let mut levels = 2u16.to_be_bytes().to_vec();
    for i in 0..29 {
        for v in [if i == 0 { 12u16 } else { 0 }, 255, 0, 255, 100] {
            levels.extend_from_slice(&v.to_be_bytes());
        }
    }
    let exposure = [
        1u16.to_be_bytes().as_slice(),
        &1.5f32.to_be_bytes(),
        &0.125f32.to_be_bytes(),
        &0.8f32.to_be_bytes(),
    ]
    .concat();
    for (key, data, expected) in [
        (*b"nvrt", vec![], compositor::Adjustment::Invert),
        (
            *b"post",
            vec![0, 8],
            compositor::Adjustment::Posterize { levels: 8 },
        ),
        (
            *b"thrs",
            vec![0, 111],
            compositor::Adjustment::Threshold {
                level: 111.0 / 255.0,
            },
        ),
        (
            *b"expA",
            exposure,
            compositor::Adjustment::Exposure {
                exposure: 1.5,
                offset: 0.125,
                gamma: 0.8,
            },
        ),
        (
            *b"levl",
            levels,
            compositor::Adjustment::Levels {
                master: compositor::LevelsChannel {
                    in_black: 12.0 / 255.0,
                    ..Default::default()
                },
                rgb: [Default::default(); 3],
            },
        ),
    ] {
        source.layer_section.layers[0].additional = vec![tag(&key, data)];
        let mut imported = from_psd(&source).unwrap();
        let LayerKind::Adjustment(a) = &imported.root[0].kind else {
            panic!("adjustment")
        };
        assert_eq!(*a, expected);
        assert_eq!(
            to_psd(&imported).unwrap().layer_section.layers,
            source.layer_section.layers
        );
        std::sync::Arc::make_mut(&mut imported.root[0]).kind =
            LayerKind::Adjustment(compositor::Adjustment::Invert);
        let out = to_psd(&imported).unwrap();
        assert!(out.layer_section.layers[0].info(b"nvrt").is_some());
        assert!(matches!(
            from_psd(&out).unwrap().root[0].kind,
            LayerKind::Adjustment(compositor::Adjustment::Invert)
        ));
    }
}
#[test]
fn text_and_smart_originals_remain_available_with_raster_proxies() {
    for key in [*b"TySh", *b"SoLd"] {
        let mut source = fixture(8);
        source.layer_section.layers[0].additional =
            vec![tag(&key, vec![1, 2, 3, 4]), tag(b"lnk2", vec![9, 8, 7])];
        let imported = from_psd(&source).unwrap();
        if key == *b"TySh" {
            assert!(matches!(imported.root[0].kind, LayerKind::Text(_)));
        } else {
            assert!(matches!(imported.root[0].kind, LayerKind::SmartObject(_)));
        }
        assert_eq!(
            to_psd(&imported).unwrap().layer_section.layers,
            source.layer_section.layers
        );
    }
}

#[test]
fn edited_pixels_expand_bounds_and_refresh_merged_composite() {
    let mut imported = from_psd(&fixture(8)).unwrap();
    std::sync::Arc::make_mut(&mut imported.root[0])
        .raster_mut()
        .unwrap()
        .edit_region(compositor::Rect::new(0, 0, 1, 1), 1, |_, _, p| {
            *p = [1.0, 0.0, 0.0, 1.0]
        })
        .unwrap();
    let out = to_psd(&imported).unwrap();
    assert_eq!(out.layer_section.layers[0].bounds.left, 0);
    assert_eq!(out.composite[0], 255);
    let again = from_psd(&PsdDocument::read(&out.write().unwrap()).unwrap()).unwrap();
    assert_eq!(
        again.root[0].raster().unwrap().pixel(0, 0),
        [1.0, 0.0, 0.0, 1.0]
    );
}
#[test]
fn document_resources_and_flattened_rgb_import() {
    let mut source = fixture(8);
    source.resources = vec![
        psd::ImageResource::new(1039, vec![1, 2, 3]),
        psd::Resolution {
            horizontal: 300 << 16,
            vertical: 300 << 16,
            horizontal_unit: 1,
            vertical_unit: 1,
            width_unit: 1,
            height_unit: 1,
        }
        .to_resource(),
        psd::ImageResource::new(4000, vec![7, 8, 9]),
    ];
    source.layer_section.layers.clear();
    source.composite = vec![255, 0, 0, 255, 0, 0];
    let imported = from_psd(&source).unwrap();
    assert_eq!(imported.ppi, 300.0);
    assert_eq!(
        imported
            .profile
            .as_ref()
            .unwrap()
            .icc
            .as_deref()
            .unwrap()
            .as_slice(),
        [1, 2, 3]
    );
    assert_eq!(imported.root.len(), 1);
    assert_eq!(
        imported.root[0].raster().unwrap().pixel(0, 0),
        [1.0, 0.0, 0.0, 1.0]
    );
    assert_eq!(to_psd(&imported).unwrap().resources, source.resources);
}

#[test]
fn unsupported_features_are_reported_without_losing_opaque_data() {
    let mut source = fixture(8);
    let layer = &mut source.layer_section.layers[0];
    layer.blend_mode = *b"????";
    layer.additional = vec![tag(b"brit", vec![0, 5, 0, 10]), tag(b"vmsk", vec![1, 2, 3])];
    let imported = from_psd(&source).unwrap();
    assert!(imported.warnings.iter().any(|w| w.contains("blend")));
    assert!(imported.warnings.iter().any(|w| w.contains("adjustment")));
    assert!(imported.warnings.iter().any(|w| w.contains("vector")));
    assert_eq!(
        imported.original_layer(imported.root[0].id).unwrap(),
        &source.layer_section.layers[0]
    );
    assert_eq!(
        to_psd(&imported).unwrap().layer_section.layers,
        source.layer_section.layers
    );
}
#[test]
fn unsupported_text_edits_and_invalid_inputs_fail_explicitly() {
    let mut source = fixture(8);
    source.layer_section.layers[0]
        .additional
        .push(tag(b"TySh", vec![1, 2, 3]));
    let mut imported = from_psd(&source).unwrap();
    let LayerKind::Text(text) = &mut std::sync::Arc::make_mut(&mut imported.root[0]).kind else {
        panic!("text")
    };
    text.text = "new text".into();
    assert!(
        to_psd(&imported).is_err(),
        "do not silently export obsolete text descriptors"
    );
    source.color_mode = ColorMode::Cmyk;
    assert!(from_psd(&source).is_err());
    source.color_mode = ColorMode::Rgb;
    source.layer_section.layers[0].channels[0].data.clear();
    assert!(from_psd(&source).is_err());
}

#[test]
fn native_tree_export_import_renders_identically_at_all_depths() {
    use compositor::{Compositor, DocState, Document, GroupMode, Layer as Node, Mask, Raster};
    use std::sync::Arc;
    let extent = engine_api::tile::Extent::new(2, 1);
    for depth in [Depth::U8, Depth::U16, Depth::F32] {
        for blend in compositor::BlendMode::ALL {
            if blend == compositor::BlendMode::Dissolve {
                continue;
            } // IDs seed the dissolve hash.
            let mut state = DocState::new(extent, depth);
            let mut base = Node::pixel("base", extent, depth);
            base.id = compositor::LayerId(1);
            base.raster_mut()
                .unwrap()
                .edit_region(compositor::Rect::of_extent(extent), 1, |_, _, p| {
                    *p = [0.7, 0.3, 0.4, 1.0]
                })
                .unwrap();
            let mut top = Node::pixel("top", extent, depth);
            top.id = compositor::LayerId(3);
            top.props.blend_mode = blend;
            top.props.opacity = 128.0 / 255.0;
            top.props.fill_opacity = 192.0 / 255.0;
            top.props.clipped = true;
            top.raster_mut()
                .unwrap()
                .edit_region(compositor::Rect::of_extent(extent), 1, |_, _, p| {
                    *p = [0.2, 0.8, 0.5, 0.6]
                })
                .unwrap();
            let mut mask = Mask::reveal_all(extent, depth);
            mask.raster = Raster::new(extent, 1, depth, 1.0);
            mask.raster
                .edit_region(compositor::Rect::of_extent(extent), 1, |x, _, p| {
                    p[0] = if x == 0 { 0.5 } else { 1.0 }
                })
                .unwrap();
            top.mask = Some(mask);
            let mut group = Node::group("group", GroupMode::PassThrough)
                .with_child(base)
                .with_child(top);
            group.id = compositor::LayerId(2);
            state.root = vec![Arc::new(group)];
            state.next_id = 4;
            let before = Compositor::new(1 << 20)
                .render_level_rgba(&Document::new(state.clone()), 0)
                .unwrap()
                .1;
            let imported = compositor::psd::ImportedPsd::from_state(state).unwrap();
            let bytes = to_psd(&imported).unwrap().write().unwrap();
            let again = from_psd(&PsdDocument::read(&bytes).unwrap()).unwrap();
            let after = Compositor::new(1 << 20)
                .render_level_rgba(&Document::new(again.state), 0)
                .unwrap()
                .1;
            for (a, b) in before.iter().zip(after) {
                assert!((a - b).abs() < 0.0001, "{depth:?} {blend:?}: {a} != {b}");
            }
        }
    }
}

#[test]
fn rgb_pixels_all_depths_are_imported_and_roundtrip() {
    for (bits, depth) in [(8, Depth::U8), (16, Depth::U16), (32, Depth::F32)] {
        let source = fixture(bits);
        let imported = from_psd(&source).unwrap();
        assert_eq!(imported.depth, depth);
        let LayerKind::Pixel(raster) = &imported.root[0].kind else {
            panic!("pixel")
        };
        assert_eq!(raster.pixel(0, 0), [0.0; 4]);
        for (actual, expected) in
            raster
                .pixel(1, 0)
                .into_iter()
                .zip([128.0 / 255.0, 64.0 / 255.0, 1.0, 192.0 / 255.0])
        {
            assert!((actual - expected).abs() < 0.00002);
        }
        let exported = to_psd(&imported).unwrap();
        assert_eq!(exported.layer_section.layers, source.layer_section.layers);
        assert_eq!(exported.depth, bits);
        let reread = PsdDocument::read(&exported.write().unwrap()).unwrap();
        assert_eq!(reread.layer_section.layers, source.layer_section.layers);
    }
}

#[test]
fn new_adjustments_match_native_render_after_psd_serialization() {
    use compositor::{Adjustment, Compositor, Curve, Document, Layer as Node};
    use std::sync::Arc;
    let adjustments = [
        Adjustment::ChannelMixer {
            matrix: [[1.2, -0.3, 0.1], [0.0, 0.8, 0.2], [0.2, 0.0, 0.8]],
            constant: [-0.05, 0.1, 0.0],
            monochrome: false,
        },
        Adjustment::ChannelMixer {
            matrix: [[0.3, 0.6, 0.1]; 3],
            constant: [0.05; 3],
            monochrome: true,
        },
        Adjustment::Curves {
            master: Curve(vec![[0.0, 0.0], [128.0 / 255.0, 180.0 / 255.0], [1.0, 1.0]]),
            rgb: Default::default(),
        },
        Adjustment::HueSaturation {
            hue: -60.0,
            saturation: 25.0,
            lightness: -10.0,
            colorize: false,
        },
        Adjustment::HueSaturation {
            hue: 180.0,
            saturation: 45.0,
            lightness: 20.0,
            colorize: true,
        },
    ];
    for depth in [8, 16, 32] {
        for adjustment in &adjustments {
            let mut imported = from_psd(&fixture(depth)).unwrap();
            let mut layer = Node::new("adjustment", LayerKind::Adjustment(adjustment.clone()));
            layer.id = compositor::LayerId(imported.next_id);
            imported.next_id += 1;
            imported.root.push(Arc::new(layer));
            let before = Compositor::new(1 << 20)
                .render_level_rgba(&Document::new(imported.state.clone()), 0)
                .unwrap()
                .1;
            let serialized = to_psd(&imported).unwrap().write().unwrap();
            let again = Document::from_psd(PsdDocument::read(&serialized).unwrap()).unwrap();
            let after = Compositor::new(1 << 20)
                .render_level_rgba(&again, 0)
                .unwrap()
                .1;
            for (a, b) in before.iter().zip(after) {
                assert!((a - b).abs() < 1e-6, "{depth}: {adjustment:?}: {a} != {b}");
            }
        }
    }
}

#[test]
fn selective_hue_bands_remain_opaque_until_replaced() {
    use compositor::Adjustment;
    let mut source = fixture(8);
    let mut data = vec![0; 100];
    data[1] = 2;
    data[25] = 30;
    source.layer_section.layers[0].additional = vec![tag(b"hue2", data)];
    let mut imported = from_psd(&source).unwrap();
    assert!(matches!(imported.root[0].kind, LayerKind::Pixel(_)));
    assert!(!imported.warnings.is_empty());
    assert_eq!(
        to_psd(&imported).unwrap().layer_section.layers,
        source.layer_section.layers
    );
    std::sync::Arc::make_mut(&mut imported.root[0]).kind =
        LayerKind::Adjustment(Adjustment::HueSaturation {
            hue: 0.0,
            saturation: 50.0,
            lightness: 0.0,
            colorize: false,
        });
    let again = from_psd(&to_psd(&imported).unwrap()).unwrap();
    assert!(matches!(
        again.root[0].kind,
        LayerKind::Adjustment(Adjustment::HueSaturation { .. })
    ));
}
