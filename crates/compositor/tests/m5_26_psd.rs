use compositor::psd::{ImportedPsd, from_psd, to_psd};
use compositor::{Adjustment as A, Depth, DocState, Layer, LayerKind};
use engine_api::tile::Extent;
use std::sync::Arc;

fn export(a: A) -> psd::PsdDocument {
    let mut state = DocState::new(Extent::new(1, 1), Depth::U8);
    state
        .root
        .push(Arc::new(Layer::new("adjustment", LayerKind::Adjustment(a))));
    to_psd(&ImportedPsd::from_state(state).unwrap()).unwrap()
}
fn roundtrip(a: A, key: &[u8; 4]) -> psd::PsdDocument {
    let source = export(a.clone());
    let source = psd::PsdDocument::read(&source.write().unwrap()).unwrap();
    assert!(source.layer_section.layers[0].info(key).is_some());
    let imported = from_psd(&source).unwrap();
    let LayerKind::Adjustment(actual) = &imported.root[0].kind else {
        panic!("not adjustment")
    };
    assert_eq!(actual, &a);
    assert_eq!(
        to_psd(&imported).unwrap().layer_section.layers,
        source.layer_section.layers
    );
    source
}
fn malformed(key: &[u8; 4], data: Vec<u8>) -> psd::PsdDocument {
    let mut source = export(A::Invert);
    source.layer_section.layers[0].additional = vec![psd::AdditionalInfo {
        signature: *b"8BIM",
        key: *key,
        data,
    }];
    source
}
#[test]
fn rejects_truncated_and_invalid_layouts() {
    for (key, data, minimum) in [
        (
            b"brit",
            export(A::BrightnessContrast {
                brightness: 5.0,
                contrast: 6.0,
                legacy: true,
            }),
            7,
        ),
        (
            b"blnc",
            export(A::ColorBalance {
                shadows: [0.0; 3],
                midtones: [0.0; 3],
                highlights: [0.0; 3],
                preserve_luminosity: true,
            }),
            19,
        ),
        (
            b"selc",
            export(A::SelectiveColor {
                colors: [[0.0; 4]; 9],
                absolute: false,
            }),
            84,
        ),
        (
            b"phfl",
            export(A::PhotoFilter {
                color: [1.0; 3],
                density: 25.0,
                preserve_luminosity: true,
            }),
            17,
        ),
    ] {
        let bytes = &data.layer_section.layers[0].info(key).unwrap().data;
        for length in 0..minimum {
            assert!(
                from_psd(&malformed(key, bytes[..length].to_vec())).is_err(),
                "{key:?} length {length}"
            );
        }
    }
    for key in [b"vibA", b"blwh", b"CgEd"] {
        for bytes in [
            vec![],
            vec![0, 0, 0, 15],
            vec![0, 0, 0, 16],
            b"{\"kind\":\"vibrance\"}".to_vec(),
        ] {
            assert!(from_psd(&malformed(key, bytes)).is_err());
        }
    }
    let mut bytes = vec![0; 84];
    bytes[1] = 2;
    assert!(from_psd(&malformed(b"selc", bytes)).is_err());
    let mut bytes = vec![0; 20];
    bytes[18] = 2;
    assert!(from_psd(&malformed(b"blnc", bytes)).is_err());
    let mut bytes = vec![0; 20];
    bytes[1] = 4;
    assert!(from_psd(&malformed(b"phfl", bytes)).is_err());
    // Out-of-range signed percentages must not silently clamp during rendering.
    let mut bytes = vec![0; 20];
    bytes[0..2].copy_from_slice(&101i16.to_be_bytes());
    assert!(from_psd(&malformed(b"blnc", bytes)).is_err());
}
#[test]
fn gradient_rejects_truncation_and_retains_unsupported_features() {
    let a = A::GradientMap {
        stops: vec![[0.0, 0.0, 0.0, 0.0], [1.0, 1.0, 1.0, 1.0]],
        dither: false,
        reverse: false,
        method: compositor::adjust::GradientMethod::Classic,
    };
    let source = export(a.clone());
    let bytes = &source.layer_section.layers[0].info(b"grdm").unwrap().data;
    for length in 0..bytes.len() - 4 {
        assert!(
            from_psd(&malformed(b"grdm", bytes[..length].to_vec())).is_err(),
            "length {length}"
        );
    }
    let mut v1 = bytes.clone();
    v1[1] = 1;
    v1.drain(4..8);
    let imported = from_psd(&malformed(b"grdm", v1)).unwrap();
    assert!(matches!(&imported.root[0].kind, LayerKind::Adjustment(actual) if actual == &a));
    // Non-central midpoint and nonopaque alpha are outside native model.
    for offset in [20usize, 66] {
        let mut unsupported = bytes.clone();
        unsupported[offset..offset + 4].copy_from_slice(&25u32.to_be_bytes());
        let src = malformed(b"grdm", unsupported);
        let imported = from_psd(&src).unwrap();
        assert!(!matches!(imported.root[0].kind, LayerKind::Adjustment(_)));
        assert_eq!(
            to_psd(&imported).unwrap().layer_section.layers[0].info(b"grdm"),
            src.layer_section.layers[0].info(b"grdm")
        );
    }
}
#[test]
fn soco_is_not_an_adjustment_and_native_only_errors_are_explicit() {
    assert!(psd::metadata::adjustment(*b"SoCo", b"{}").is_none());
    let source = malformed(b"SoCo", vec![0, 0, 0, 16]);
    assert!(!matches!(
        from_psd(&source).unwrap().root[0].kind,
        LayerKind::Adjustment(_)
    ));
    for a in [
        A::Desaturate,
        A::Equalize {
            maps: Default::default(),
        },
        A::Auto {
            mode: compositor::adjust::AutoMode::Tone,
            black: [0.0; 3],
            white: [1.0; 3],
            gamma: [1.0; 3],
        },
        A::ReplaceColor {
            color: [0.0; 3],
            fuzziness: 10.0,
            hue: 0.0,
            saturation: 0.0,
            lightness: 0.0,
        },
        A::MatchColor {
            source_layer: 1,
            source_mean: [0.0; 3],
            source_std: [1.0; 3],
            target_mean: [0.0; 3],
            target_std: [1.0; 3],
            luminance: 100.0,
            color_intensity: 100.0,
            fade: 0.0,
        },
        A::ShadowsHighlights {
            settings: Default::default(),
        },
    ] {
        let mut state = DocState::new(Extent::new(1, 1), Depth::U8);
        state
            .root
            .push(Arc::new(Layer::new("native", LayerKind::Adjustment(a))));
        let err = to_psd(&ImportedPsd::from_state(state).unwrap()).unwrap_err();
        assert!(err.to_string().contains("native-only"));
    }
}
#[test]
fn modern_brightness_precedence_and_type_changes() {
    let original = A::BrightnessContrast {
        brightness: 35.0,
        contrast: -12.0,
        legacy: false,
    };
    let mut source = export(original.clone());
    // An intentionally different legacy fallback must not win in either order.
    let legacy = source.layer_section.layers[0]
        .additional
        .iter_mut()
        .find(|b| b.key == *b"brit")
        .unwrap();
    legacy.data[0..2].copy_from_slice(&5i16.to_be_bytes());
    for _ in 0..2 {
        source.layer_section.layers[0].additional.reverse();
        let imported = from_psd(&source).unwrap();
        assert!(matches!(&imported.root[0].kind,LayerKind::Adjustment(a) if a == &original));
    }
    for replacement in [
        A::BrightnessContrast {
            brightness: 3.0,
            contrast: 4.0,
            legacy: true,
        },
        A::Vibrance {
            vibrance: 5.0,
            saturation: 6.0,
        },
    ] {
        let mut imported = from_psd(&source).unwrap();
        Arc::make_mut(&mut imported.root[0]).kind = LayerKind::Adjustment(replacement.clone());
        let out = to_psd(&imported).unwrap();
        assert!(out.layer_section.layers[0].info(b"CgEd").is_none());
        let again = from_psd(&out).unwrap();
        assert!(matches!(&again.root[0].kind,LayerKind::Adjustment(a) if a == &replacement));
    }
}
#[test]
fn binary_payloads_match_independently_constructed_adobe_records() {
    let mut blnc = Vec::new();
    for value in [-10i16, 20, 30, 40, -50, 60, 70, 80, -90] {
        blnc.extend_from_slice(&value.to_be_bytes());
    }
    blnc.extend_from_slice(&[1, 0]);
    let src = malformed(b"blnc", blnc);
    let imported = from_psd(&src).unwrap();
    assert!(
        matches!(&imported.root[0].kind,LayerKind::Adjustment(A::ColorBalance { shadows,midtones,highlights,preserve_luminosity:true }) if *shadows==[-10.0,20.0,30.0] && *midtones==[40.0,-50.0,60.0] && *highlights==[70.0,80.0,-90.0])
    );
    let src = export(A::PhotoFilter {
        color: [1.0, 0.0, 1.0],
        density: 25.5,
        preserve_luminosity: true,
    });
    assert_eq!(
        src.layer_section.layers[0].info(b"phfl").unwrap().data,
        [
            0, 2, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 0, 0, 9, 246, 1, 0, 0, 0
        ]
    );
}
#[test]
fn gradient_map_adobe_binary_roundtrip() {
    for method in [
        compositor::adjust::GradientMethod::Classic,
        compositor::adjust::GradientMethod::Perceptual,
        compositor::adjust::GradientMethod::Linear,
    ] {
        let source = roundtrip(
            A::GradientMap {
                stops: vec![
                    [0.0, 1.0, 0.0, 0.0],
                    [0.5, 0.0, 1.0, 0.0],
                    [1.0, 0.0, 0.0, 1.0],
                ],
                dither: true,
                reverse: true,
                method,
            },
            b"grdm",
        );
        let d = &source.layer_section.layers[0].info(b"grdm").unwrap().data;
        assert_eq!(&d[..4], &[0, 3, 1, 1]);
        assert!(
            [b"Gcls", b"Perc", b"Lnr "]
                .iter()
                .any(|key| d[4..8] == **key)
        );
    }
}
#[test]
fn color_descriptors_use_adobe_fields() {
    for (a, key, field) in [
        (
            A::Vibrance {
                vibrance: -25.0,
                saturation: 45.0,
            },
            b"vibA",
            b"vibrance".as_slice(),
        ),
        (
            A::BlackWhite {
                sliders: [40.0, 60.0, 40.0, 60.0, 20.0, 80.0],
                tint: Some([1.0, 0.25, 0.0]),
            },
            b"blwh",
            b"useTint".as_slice(),
        ),
        (
            A::BlackWhite {
                sliders: [40.0, 60.0, 40.0, 60.0, 20.0, 80.0],
                tint: None,
            },
            b"blwh",
            b"useTint".as_slice(),
        ),
    ] {
        let source = roundtrip(a, key);
        let d = &source.layer_section.layers[0].info(key).unwrap().data;
        assert_eq!(&d[..4], &16u32.to_be_bytes());
        let (descriptor, _) = psd::metadata::parse_descriptor(&d[4..]).unwrap();
        assert_eq!(descriptor.class_id, b"null");
        assert!(descriptor.get(field).is_some());
    }
}
#[test]
fn color_binary_keys_roundtrip() {
    let source = roundtrip(
        A::ColorBalance {
            shadows: [-10.0, 20.0, 30.0],
            midtones: [40.0, -50.0, 60.0],
            highlights: [70.0, 80.0, -90.0],
            preserve_luminosity: true,
        },
        b"blnc",
    );
    assert_eq!(
        source.layer_section.layers[0]
            .info(b"blnc")
            .unwrap()
            .data
            .len(),
        20
    );
    let colors = std::array::from_fn(|i| [i as f32, -20.0, 30.0, -40.0]);
    for absolute in [false, true] {
        let source = roundtrip(A::SelectiveColor { colors, absolute }, b"selc");
        let d = &source.layer_section.layers[0].info(b"selc").unwrap().data;
        assert_eq!(d.len(), 84);
        assert_eq!(&d[4..12], &[0; 8]);
    }
    roundtrip(
        A::PhotoFilter {
            color: [1.0, 0.0, 1.0],
            density: 25.5,
            preserve_luminosity: true,
        },
        b"phfl",
    );
}
// Independent Action Descriptor fixture builder, not the production encoder.
fn lookup_fixture(format: &[u8], cube: &[u8]) -> Vec<u8> {
    fn id(out: &mut Vec<u8>, value: &[u8]) {
        out.extend_from_slice(&(value.len() as u32).to_be_bytes());
        out.extend_from_slice(value);
    }
    let mut out = vec![0, 1, 0, 0, 0, 16];
    out.extend_from_slice(&0u32.to_be_bytes()); // empty Unicode name
    id(&mut out, b"null");
    out.extend_from_slice(&3u32.to_be_bytes());
    for (key, ty, value) in [
        (
            b"lookupType".as_slice(),
            b"colorLookupType".as_slice(),
            b"3DLUT".as_slice(),
        ),
        (b"LUTFormat", b"LUTFormatType", format),
    ] {
        id(&mut out, key);
        out.extend_from_slice(b"enum");
        id(&mut out, ty);
        id(&mut out, value);
    }
    id(&mut out, b"LUT3DFileData");
    out.extend_from_slice(b"tdta");
    out.extend_from_slice(&(cube.len() as u32).to_be_bytes());
    out.extend_from_slice(cube);
    out
}

#[test]
fn color_lookup_malformed_errors_and_unsupported_stays_opaque() {
    let cube = b"LUT_3D_SIZE 2\n1 0 0\n0 0 0\n1 1 0\n0 1 0\n1 0 1\n0 0 1\n1 1 1\n0 1 1\n";
    let bytes = lookup_fixture(b"LUTFormatCUBE", cube);
    let source = malformed(b"clrL", bytes.clone());
    let imported = from_psd(&source).unwrap();
    assert!(
        matches!(&imported.root[0].kind, LayerKind::Adjustment(A::ColorLookup { size: 2, data }) if data[0] == [1.0, 0.0, 0.0] && data[1] == [0.0; 3])
    );
    for length in 0..bytes.len() {
        assert!(
            from_psd(&malformed(b"clrL", bytes[..length].to_vec())).is_err(),
            "length {length}"
        );
    }
    for bad in [
        b"LUT_3D_SIZE 2\n0 0 0".as_slice(),
        b"LUT_3D_SIZE 2\nNaN 0 0",
        b"\xff",
    ] {
        assert!(from_psd(&malformed(b"clrL", lookup_fixture(b"LUTFormatCUBE", bad))).is_err());
    }
    for bytes in [
        lookup_fixture(b"LUTFormatLOOK", b"opaque look"),
        lookup_fixture(b"LUTFormatCUBE", b"LUT_1D_SIZE 2\n0 0 0\n1 1 1"),
        lookup_fixture(
            b"LUTFormatCUBE",
            &[b"DOMAIN_MIN -1 -1 -1\n".as_slice(), cube].concat(),
        ),
    ] {
        let source = malformed(b"clrL", bytes);
        let imported = from_psd(&source).unwrap();
        assert!(!matches!(&imported.root[0].kind, LayerKind::Adjustment(_)));
        assert_eq!(
            to_psd(&imported).unwrap().layer_section.layers[0].info(b"clrL"),
            source.layer_section.layers[0].info(b"clrL")
        );
    }
}

#[test]
fn color_lookup_cube_descriptor_roundtrips_psd_and_psb() {
    // Red varies fastest; asymmetry catches channel/table transposition.
    let data: Vec<_> = (0..8)
        .map(|i| {
            [
                1.0 - (i & 1) as f32,
                ((i >> 1) & 1) as f32 * 0.7,
                ((i >> 2) & 1) as f32 * 0.3,
            ]
        })
        .collect();
    let expected = A::ColorLookup {
        size: 2,
        data: data.clone(),
    };
    for version in [psd::Version::Psd, psd::Version::Psb] {
        let mut source = export(expected.clone());
        source.version = version;
        let source = psd::PsdDocument::read(&source.write().unwrap()).unwrap();
        let bytes = &source.layer_section.layers[0].info(b"clrL").unwrap().data;
        assert_eq!(&bytes[..6], &[0, 1, 0, 0, 0, 16]);
        let (d, _) = psd::metadata::parse_descriptor(&bytes[6..]).unwrap();
        use psd::metadata::Value as V;
        assert_eq!(
            d.get(b"lookupType"),
            Some(&V::Enum {
                type_id: b"colorLookupType",
                value: b"3DLUT"
            })
        );
        assert_eq!(
            d.get(b"LUTFormat"),
            Some(&V::Enum {
                type_id: b"LUTFormatType",
                value: b"LUTFormatCUBE"
            })
        );
        let Some(V::Raw(cube)) = d.get(b"LUT3DFileData") else {
            panic!("missing raw CUBE")
        };
        assert_eq!(
            A::color_lookup_from_cube(std::str::from_utf8(cube).unwrap()).unwrap(),
            expected
        );
        let imported = from_psd(&source).unwrap();
        assert!(matches!(&imported.root[0].kind, LayerKind::Adjustment(a) if a == &expected));
        assert_eq!(
            to_psd(&imported).unwrap().layer_section.layers,
            source.layer_section.layers
        );
    }
}

#[test]
fn brightness_has_legacy_binary_and_modern_descriptor() {
    for legacy in [true, false] {
        let source = roundtrip(
            A::BrightnessContrast {
                brightness: -25.0,
                contrast: 42.0,
                legacy,
            },
            b"brit",
        );
        let layer = &source.layer_section.layers[0];
        assert_eq!(
            layer.info(b"brit").unwrap().data,
            vec![255, 231, 0, 42, 0, 127, 0, 0]
        );
        assert_eq!(layer.info(b"CgEd").is_some(), !legacy);
    }
}
