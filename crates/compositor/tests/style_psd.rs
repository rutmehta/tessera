use compositor::psd::{ImportedPsd, from_psd, to_psd};
use compositor::render::styles::*;
use compositor::{Depth, DocState, Layer, LayerKind, Raster};
use engine_api::tile::Extent;
use std::sync::Arc;

fn styled(styles: LayerStyles) -> ImportedPsd {
    let canvas = Extent::new(4, 4);
    let mut state = DocState::new(canvas, Depth::U8);
    let mut layer = Layer::new(
        "styled",
        LayerKind::Pixel(Raster::new(canvas, 4, Depth::U8, 1.0)),
    );
    layer.props.styles = styles;

    state.root.push(Arc::new(layer));
    ImportedPsd::from_state(state).unwrap()
}

fn id(key: &[u8]) -> Vec<u8> {
    [
        ((if key.len() == 4 { 0 } else { key.len() }) as u32)
            .to_be_bytes()
            .as_slice(),
        key,
    ]
    .concat()
}
fn descriptor(class: &[u8], entries: Vec<(&[u8], &[u8; 4], Vec<u8>)>) -> Vec<u8> {
    let mut out = [
        0u32.to_be_bytes().to_vec(),
        id(class),
        (entries.len() as u32).to_be_bytes().to_vec(),
    ]
    .concat();
    for (key, ty, data) in entries {
        out.extend(id(key));
        out.extend(ty);
        out.extend(data);
    }
    out
}
fn unit(unit: &[u8; 4], value: f64) -> Vec<u8> {
    [unit.as_slice(), &value.to_be_bytes()].concat()
}
fn tag(key: &[u8; 4], data: Vec<u8>) -> psd::AdditionalInfo {
    psd::AdditionalInfo {
        signature: *b"8BIM",
        key: *key,
        data,
    }
}
// Independent native descriptor fixture: no adapter encoder involved.
fn shadow_fixture(size_unit: &[u8; 4]) -> psd::PsdDocument {
    let color = descriptor(
        b"RGBC",
        vec![
            (b"Rd  ", b"doub", 255f64.to_be_bytes().to_vec()),
            (b"Grn ", b"doub", 127.5f64.to_be_bytes().to_vec()),
            (b"Bl  ", b"doub", 0f64.to_be_bytes().to_vec()),
        ],
    );
    let shadow = descriptor(
        b"DrSh",
        vec![
            (b"enab", b"bool", vec![1]),
            (b"Md  ", b"enum", [id(b"BlnM"), id(b"Mltp")].concat()),
            (b"Clr ", b"Objc", color),
            (b"Opct", b"UntF", unit(b"#Prc", 40.0)),
            (b"blur", b"UntF", unit(size_unit, 12.0)),
            (b"Ckmt", b"UntF", unit(b"#Prc", 25.0)),
            (b"vendorShadowData", b"tdta", vec![0, 0, 0, 3, 7, 8, 9]),
        ],
    );
    let data = [
        0u32.to_be_bytes().to_vec(),
        16u32.to_be_bytes().to_vec(),
        descriptor(
            b"Lefx",
            vec![
                (b"Scl ", b"UntF", unit(b"#Prc", 150.0)),
                (b"DrSh", b"Objc", shadow),
                (b"vendorEffect", b"tdta", vec![0, 0, 0, 2, 42, 43]),
            ],
        ),
    ]
    .concat();
    let mut psd = to_psd(&styled(LayerStyles::default())).unwrap();
    psd.layer_section.layers[0]
        .additional
        .push(tag(b"lfx2", data));
    psd.layer_section.layers[0]
        .additional
        .push(tag(b"SoLE", vec![1, 7, 3, 8]));
    psd.layer_section.layers[0]
        .additional
        .push(tag(b"zzzz", vec![5, 4, 3, 2, 1]));
    psd
}

#[test]
fn independent_descriptor_import_edit_and_opaque_preservation() {
    let source = shadow_fixture(b"#Pxl");
    let source = psd::PsdDocument::read(&source.write().unwrap()).unwrap();
    let mut imported = from_psd(&source).unwrap();
    let StyleEffect::DropShadow(shadow) = &imported.root[0].props.styles.effects[0] else {
        panic!("shadow")
    };
    assert_eq!(shadow.color, [1.0, 0.5, 0.0, 1.0]);
    assert_eq!(shadow.spread, 3.0);
    assert_eq!(shadow.size, 12.0);
    assert_eq!(shadow.opacity, 0.4);
    assert_eq!(imported.root[0].props.styles.scale, 1.5);
    assert_eq!(
        to_psd(&imported).unwrap().layer_section.layers[0].additional,
        source.layer_section.layers[0].additional
    );
    let StyleEffect::DropShadow(shadow) =
        &mut Arc::make_mut(&mut imported.root[0]).props.styles.effects[0]
    else {
        panic!("shadow")
    };
    shadow.distance = 17.0;
    let out = to_psd(&imported).unwrap();
    let out = psd::PsdDocument::read(&out.write().unwrap()).unwrap();
    for key in [b"SoLE", b"zzzz"] {
        assert_eq!(
            out.layer_section.layers[0].info(key),
            source.layer_section.layers[0].info(key)
        );
    }
    let parsed =
        psd::metadata::parse_styles(&out.layer_section.layers[0].info(b"lfx2").unwrap().data)
            .unwrap();
    assert_eq!(
        parsed.descriptor.get(b"vendorEffect"),
        Some(&psd::metadata::Value::Raw(&[42, 43]))
    );
    assert_eq!(
        parsed
            .drop_shadow()
            .unwrap()
            .descriptor
            .get(b"vendorShadowData"),
        Some(&psd::metadata::Value::Raw(&[7, 8, 9]))
    );
    assert_eq!(
        parsed.drop_shadow().unwrap().distance().unwrap().number(),
        Some(17.0)
    );
    Arc::make_mut(&mut imported.root[0])
        .props
        .styles
        .effects
        .clear();
    let removed = to_psd(&imported).unwrap();
    let parsed =
        psd::metadata::parse_styles(&removed.layer_section.layers[0].info(b"lfx2").unwrap().data)
            .unwrap();
    assert!(parsed.drop_shadow().is_none());
    assert!(parsed.descriptor.get(b"vendorEffect").is_some());
}

#[test]
fn unsupported_units_remain_opaque_instead_of_becoming_pixels() {
    let source = shadow_fixture(b"#Pnt");
    let imported = from_psd(&source).unwrap();
    assert!(imported.root[0].props.styles.effects.is_empty());
    assert_eq!(
        to_psd(&imported).unwrap().layer_section.layers[0].info(b"lfx2"),
        source.layer_section.layers[0].info(b"lfx2")
    );
}

#[test]
fn editing_disabled_styles_does_not_enable_opaque_effects() {
    let mut source = shadow_fixture(b"#Pxl");
    let data = &mut source.layer_section.layers[0]
        .additional
        .iter_mut()
        .find(|b| b.key == *b"lfx2")
        .unwrap()
        .data;
    // Empty descriptor name + four-byte class ID places its item count at 20.
    data[20..24].copy_from_slice(&4u32.to_be_bytes());
    data.extend(id(b"masterFXSwitch"));
    data.extend(b"bool");
    data.push(0);
    let mut imported = from_psd(&source).unwrap();
    let StyleEffect::DropShadow(s) =
        &mut Arc::make_mut(&mut imported.root[0]).props.styles.effects[0]
    else {
        panic!("shadow")
    };
    assert!(!s.enabled);
    s.distance = 22.0;
    let out = to_psd(&imported).unwrap();
    let parsed =
        psd::metadata::parse_styles(&out.layer_section.layers[0].info(b"lfx2").unwrap().data)
            .unwrap();
    assert_eq!(
        parsed.descriptor.get(b"masterFXSwitch"),
        Some(&psd::metadata::Value::Bool(false))
    );
}

#[test]
fn unrepresentable_styles_fail_export_instead_of_disappearing() {
    for effects in [
        vec![StyleEffect::Bevel(Bevel::default())],
        vec![
            StyleEffect::DropShadow(Shadow::default()),
            StyleEffect::DropShadow(Shadow::default()),
        ],
        vec![StyleEffect::OuterGlow(Glow {
            shape: EffectShape {
                jitter: 0.2,
                ..Default::default()
            },
            ..Default::default()
        })],
    ] {
        assert!(
            to_psd(&styled(LayerStyles {
                effects,
                ..Default::default()
            }))
            .is_err()
        );
    }
}

#[test]
fn native_lfx2_style_set_roundtrip() {
    let styles = LayerStyles {
        scale: 1.25,
        effects: vec![
            StyleEffect::DropShadow(Shadow {
                angle: 45.0,
                distance: 3.0,
                size: 4.0,
                spread: 1.0,
                opacity: 0.5,
                use_global_light: false,
                ..Default::default()
            }),
            StyleEffect::InnerShadow(Shadow {
                enabled: false,
                ..Default::default()
            }),
            StyleEffect::OuterGlow(Glow {
                size: 8.0,
                spread: 2.0,
                ..Default::default()
            }),
            StyleEffect::InnerGlow(Glow {
                center: true,
                ..Default::default()
            }),
            StyleEffect::Stroke(Stroke {
                position: StrokePosition::Inside,
                ..Default::default()
            }),
            StyleEffect::ColorOverlay(Overlay::default()),
        ],
    };
    let mut doc = styled(styles.clone());
    doc.state.global_light = GlobalLight {
        angle: 135.0,
        elevation: 40.0,
    };
    let psd = to_psd(&doc).unwrap();
    let block = psd.layer_section.layers[0]
        .info(b"lfx2")
        .expect("native lfx2, not private metadata");
    let parsed = psd::metadata::parse_styles(&block.data).unwrap();
    let shadow = parsed.drop_shadow().unwrap();
    assert_eq!(shadow.distance().unwrap().number(), Some(3.0));
    assert_eq!(shadow.blend_mode(), Some(b"Mltp".as_slice()));
    let bytes = psd.write().unwrap();
    let reread = psd::PsdDocument::read(&bytes).unwrap();
    let imported = from_psd(&reread).unwrap();
    assert_eq!(imported.root[0].props.styles, styles);
    assert_eq!(imported.state.global_light, doc.state.global_light);
}
