use psd::{
    metadata, AdditionalInfo, Channel, Compression, ImageResource, Layer, PsdDocument, Rect,
    Resolution, Version,
};
fn base() -> PsdDocument {
    PsdDocument::read(include_bytes!("fixtures/green-1x1.psd")).unwrap()
}
fn tag(key: &[u8; 4], data: Vec<u8>) -> AdditionalInfo {
    AdditionalInfo {
        signature: *b"8BIM",
        key: *key,
        data,
    }
}
fn unicode(text: &str) -> Vec<u8> {
    let u: Vec<u16> = text.encode_utf16().collect();
    [
        (u.len() as u32).to_be_bytes().as_slice(),
        &u.iter().flat_map(|v| v.to_be_bytes()).collect::<Vec<_>>(),
    ]
    .concat()
}
fn descriptor() -> Vec<u8> {
    [0, 0, 0, 0, 0, 0, 0, 0, b'n', b'u', b'l', b'l', 0, 0, 0, 0].to_vec()
}
fn versioned() -> Vec<u8> {
    [16u32.to_be_bytes().as_slice(), &descriptor()].concat()
}
#[test]
fn names_ids_groups_have_typed_views() {
    for block in [
        tag(b"luni", unicode("Layer 日本 🦀")),
        tag(b"lyid", 42u32.to_be_bytes().to_vec()),
        tag(
            b"lsct",
            [
                1u32.to_be_bytes().as_slice(),
                b"8BIMpass",
                &0u32.to_be_bytes(),
            ]
            .concat(),
        ),
    ] {
        assert!(
            metadata::parse(&block).unwrap().is_some(),
            "{:?}",
            block.key
        );
    }
}
#[test]
fn layers_depths_compressions_masks_and_resources() {
    for version in [Version::Psd, Version::Psb] {
        for depth in [8, 16, 32] {
            for compression in [
                Compression::Raw,
                Compression::Rle,
                Compression::Zip,
                Compression::ZipPrediction,
            ] {
                let mut d = base();
                d.version = version;
                d.width = 3;
                d.height = 2;
                d.depth = depth;
                d.compression = compression;
                d.composite = (0..3 * 2 * d.channels as usize * depth as usize / 8)
                    .map(|n| n as u8)
                    .collect();
                d.layer_section = Default::default();
                let mut mask = Vec::new();
                for v in [0i32, 0, 1, 2] {
                    mask.extend_from_slice(&v.to_be_bytes());
                }
                mask.extend_from_slice(&[255, 0, 0, 0]);
                d.layer_section.layers.push(Layer {
                    bounds: Rect {
                        top: -1,
                        left: -2,
                        bottom: 1,
                        right: 1,
                    },
                    channels: vec![
                        Channel {
                            id: 0,
                            compression,
                            data: vec![77; 6 * depth as usize / 8],
                        },
                        Channel {
                            id: -2,
                            compression,
                            data: vec![255; 2 * depth as usize / 8],
                        },
                    ],
                    blend_mode: *b"hMix",
                    opacity: 123,
                    clipping: 1,
                    flags: 1,
                    name: b"layer".to_vec(),
                    mask_data: mask,
                    blending_ranges: vec![[1, 2, 253, 254, 3, 4, 251, 252]],
                    additional: vec![
                        tag(b"luni", unicode("日本")),
                        tag(b"lyid", 42u32.to_be_bytes().to_vec()),
                        tag(b"????", vec![9, 1, 7]),
                    ],
                    ..Layer::default()
                });
                d.layer_section.merged_alpha = true;
                d.layer_section
                    .additional
                    .push(tag(b"unkn", vec![4, 3, 2, 1, 0]));
                let resolution = Resolution {
                    horizontal: 300 << 16,
                    vertical: 144 << 16,
                    horizontal_unit: 1,
                    vertical_unit: 1,
                    width_unit: 1,
                    height_unit: 1,
                };
                d.resources = vec![
                    ImageResource::new(1039, vec![0, 1, 2, 3]),
                    resolution.to_resource(),
                    ImageResource::new(
                        1069,
                        [1u16.to_be_bytes().as_slice(), &42u32.to_be_bytes()].concat(),
                    ),
                    ImageResource::new(1050, vec![7, 8, 9]),
                ];
                let b = d.write().unwrap();
                let e = PsdDocument::read(&b).unwrap();
                assert_eq!(e, d);
                assert_eq!(e.icc_profile(), Some([0, 1, 2, 3].as_slice()));
                assert_eq!(e.resolution().unwrap(), Some(resolution));
                assert_eq!(e.selected_layer_ids().unwrap(), vec![42]);
                assert_eq!(
                    e.layer_section.layers[0]
                        .mask()
                        .unwrap()
                        .unwrap()
                        .bounds
                        .right,
                    2
                );
            }
        }
    }
}
#[test]
fn psb_large_dimension_and_64_bit_keys() {
    let mut d = base();
    d.version = Version::Psb;
    d.width = 30_001;
    d.composite = vec![17; d.width as usize * d.channels as usize];
    d.layer_section = Default::default();
    d.layer_section.additional = vec![
        tag(b"LMsk", vec![0; 13]),
        AdditionalInfo {
            signature: *b"8B64",
            key: *b"futr",
            data: vec![1, 2, 3],
        },
    ];
    let b = d.write().unwrap();
    assert_eq!(PsdDocument::read(&b).unwrap(), d);
    d.version = Version::Psd;
    assert!(d.write().is_err());
}
#[test]
fn every_supported_key_and_unknown_payload_roundtrips() {
    let mut d = base();
    d.layer_section = Default::default();
    let mut blocks = vec![
        tag(b"lsct", 1u32.to_be_bytes().to_vec()),
        tag(b"luni", unicode("unicode")),
        tag(b"lyid", 7u32.to_be_bytes().to_vec()),
        tag(
            b"lfx2",
            [0u32.to_be_bytes().as_slice(), &versioned()].concat(),
        ),
        tag(b"lrFX", vec![0; 4]),
    ];
    for key in [b"SoCo", b"GdFl", b"PtFl"] {
        blocks.push(tag(key, versioned()));
    }
    // Real documented fixed layouts; opaque payload fidelity is independent of interpretation.
    blocks.push(tag(
        b"levl",
        [2u16.to_be_bytes().as_slice(), &vec![0; 290]].concat(),
    ));
    blocks.push(tag(b"curv", vec![0, 0, 1, 0, 0, 0, 0]));
    blocks.push(tag(b"brit", vec![0; 8]));
    blocks.push(tag(
        b"hue2",
        [2u16.to_be_bytes().as_slice(), &[0; 98]].concat(),
    ));
    blocks.push(tag(b"blnc", vec![0; 20]));
    blocks.push(tag(
        b"expA",
        [1u16.to_be_bytes().as_slice(), &[0; 8], &1f32.to_be_bytes()].concat(),
    ));
    for key in [b"vibA", b"blwh"] {
        blocks.push(tag(key, versioned()));
    }
    blocks.push(tag(
        b"phfl",
        [2u16.to_be_bytes().as_slice(), &[0; 15]].concat(),
    ));
    blocks.push(tag(
        b"mixr",
        [1u16.to_be_bytes().as_slice(), &[0; 22]].concat(),
    ));
    blocks.push(tag(
        b"selc",
        [1u16.to_be_bytes().as_slice(), &[0; 82]].concat(),
    ));
    blocks.push(tag(b"thrs", 128u16.to_be_bytes().to_vec()));
    blocks.push(tag(b"post", 4u16.to_be_bytes().to_vec()));
    blocks.push(tag(b"nvrt", vec![]));
    for key in [b"SoLd", b"PlLd"] {
        blocks.push(tag(
            key,
            [b"soLD".as_slice(), &4u32.to_be_bytes(), &versioned()].concat(),
        ));
    }
    let original = b"embedded original bytes";
    let linked = [
        b"liFD".as_slice(),
        &1u32.to_be_bytes(),
        &[2, b'i', b'd'],
        &unicode("photo.bin"),
        b"8BPS8BIM",
        &(original.len() as u64).to_be_bytes(),
        &[0],
        original,
    ]
    .concat();
    let mut linked_block = (linked.len() as u64).to_be_bytes().to_vec();
    linked_block.extend(&linked);
    linked_block.resize(linked_block.len() + (4 - linked.len() % 4) % 4, 0);
    blocks.push(tag(b"lnkD", linked_block));
    let text = [
        1u16.to_be_bytes().as_slice(),
        &[0; 48],
        &50u16.to_be_bytes(),
        &versioned(),
        &1u16.to_be_bytes(),
        &versioned(),
        &[0; 32],
    ]
    .concat();
    blocks.push(tag(b"TySh", text));
    for key in [b"vmsk", b"vsms"] {
        blocks.push(tag(
            key,
            [
                3u32.to_be_bytes().as_slice(),
                &[0; 4],
                &6u16.to_be_bytes(),
                &[0; 24],
            ]
            .concat(),
        ));
    }
    blocks.push(tag(b"Patt", vec![]));
    blocks.push(tag(b"what", vec![0, 255, 7]));
    d.layer_section.layers = vec![Layer {
        additional: blocks.clone(),
        ..Layer::default()
    }];
    for version in [Version::Psd, Version::Psb] {
        d.version = version;
        let e = PsdDocument::read(&d.write().unwrap()).unwrap();
        assert_eq!(e, d);
        assert_eq!(e.layer_section.layers[0].additional, blocks);
        for block in &blocks {
            if block.key != *b"what" {
                assert!(metadata::parse(block).unwrap().is_some(), "{:?}", block.key);
            }
        }
    }
}
