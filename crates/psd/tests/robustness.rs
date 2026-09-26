use psd::{
    metadata, AdditionalInfo, Channel, Compression, Layer, LayerLocation, PsdDocument, Rect,
    Version,
};
fn minimal(compression: u16, data: &[u8]) -> Vec<u8> {
    [
        b"8BPS".as_slice(),
        &1u16.to_be_bytes(),
        &[0; 6],
        &1u16.to_be_bytes(),
        &1u32.to_be_bytes(),
        &3u32.to_be_bytes(),
        &8u16.to_be_bytes(),
        &1u16.to_be_bytes(),
        &[0; 12],
        &compression.to_be_bytes(),
        data,
    ]
    .concat()
}
#[test]
fn hand_built_all_compressions() {
    // Independently specified PackBits and zlib streams for [7,7,7].
    let cases: &[(u16, &[u8])] = &[
        (0, &[7, 7, 7]),
        (1, &[0, 2, 254, 7]),
        (2, &[120, 156, 99, 103, 103, 7, 0, 0, 45, 0, 22]),
        (3, &[120, 156, 99, 103, 96, 0, 0, 0, 24, 0, 8]),
    ];
    for &(compression, data) in cases {
        let bytes = minimal(compression, data);
        assert_eq!(PsdDocument::read(&bytes).unwrap().composite, vec![7; 3]);
        for n in 0..bytes.len() {
            assert!(
                PsdDocument::read(&bytes[..n]).is_err(),
                "mode {compression}, prefix {n}"
            );
        }
    }
}
#[test]
fn alternate_high_depth_layer_storage_edits() {
    for version in [Version::Psd, Version::Psb] {
        for (depth, key) in [(16, b"Lr16"), (32, b"Lr32")] {
            let mut d = PsdDocument::read(&minimal(0, &[7, 7, 7])).unwrap();
            d.version = version;
            d.depth = depth;
            d.composite = vec![3; 3 * depth as usize / 8];
            d.layer_section.location = LayerLocation::Additional(0);
            d.layer_section.additional.push(AdditionalInfo {
                signature: *b"8BIM",
                key: *key,
                data: vec![],
            });
            d.layer_section.layers.push(Layer {
                bounds: Rect {
                    top: 0,
                    left: 0,
                    bottom: 1,
                    right: 3,
                },
                channels: vec![Channel {
                    id: 0,
                    compression: Compression::ZipPrediction,
                    data: d.composite.clone(),
                }],
                ..Layer::default()
            });
            let mut e = PsdDocument::read(&d.write().unwrap()).unwrap();
            assert_eq!(e.layer_section.layers, d.layer_section.layers);
            assert_eq!(e.layer_section.location, LayerLocation::Additional(0));
            e.layer_section.layers[0].opacity = 97;
            let f = PsdDocument::read(&e.write().unwrap()).unwrap();
            assert_eq!(f.layer_section.layers[0].opacity, 97);
            assert_eq!(f.layer_section.layers, e.layer_section.layers);
        }
    }
}
#[test]
fn corrupted_length_and_random_file_inputs_never_panic() {
    let source = include_bytes!("fixtures/green-1x1.psd");
    for i in 0..source.len() {
        let mut b = source.to_vec();
        b[i] ^= 255;
        let _ = PsdDocument::read(&b);
    }
    for n in 0..source.len() {
        assert!(PsdDocument::read(&source[..n]).is_err(), "{n}");
    }
    let mut state = 417u64;
    for n in 0..512 {
        let bytes: Vec<u8> = (0..n)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state as u8
            })
            .collect();
        let _ = PsdDocument::read(&bytes);
        for key in [
            b"lfx2", b"TySh", b"SoCo", b"lsct", b"luni", b"vmsk", b"lnkD", b"SoLd", b"Patt",
            b"lrFX",
        ] {
            let _ = metadata::parse_block(*key, &bytes);
        }
    }
}
#[test]
fn nonempty_pattern_header_and_payload() {
    let pattern = [
        1u32.to_be_bytes().as_slice(),
        &3u32.to_be_bytes(),
        &[0, 2, 0, 2],
        &1u32.to_be_bytes(),
        &[0, b'P'],
        &[2, b'i', b'd'],
        &3u32.to_be_bytes(),
        &28u32.to_be_bytes(),
        &[0; 28],
    ]
    .concat();
    let mut b = (pattern.len() as u32).to_be_bytes().to_vec();
    b.extend(&pattern);
    b.resize(b.len() + (4 - pattern.len() % 4) % 4, 0);
    let patterns = metadata::parse_patterns(&b).unwrap();
    assert_eq!(patterns[0].name, "P");
    assert_eq!(patterns[0].id, b"id");
    assert_eq!(patterns[0].virtual_memory.len(), 36);
    for n in 1..b.len() {
        assert!(metadata::parse_patterns(&b[..n]).is_err());
    }
}

#[test]
fn alternate_layers_preserve_primary_fallback_bytes() {
    // Main layer info has an explicit zero count; Lr16 overrides it.
    let section = [
        2u32.to_be_bytes().as_slice(),
        &[0, 0],
        &[0; 4],
        b"8BIMLr16",
        &2u32.to_be_bytes(),
        &[0; 4],
    ]
    .concat();
    let bytes = [
        b"8BPS".as_slice(),
        &1u16.to_be_bytes(),
        &[0; 6],
        &1u16.to_be_bytes(),
        &1u32.to_be_bytes(),
        &1u32.to_be_bytes(),
        &16u16.to_be_bytes(),
        &1u16.to_be_bytes(),
        &[0; 8],
        &(section.len() as u32).to_be_bytes(),
        &section,
        &[0; 4],
    ]
    .concat();
    let d = PsdDocument::read(&bytes).unwrap();
    let written = d.write().unwrap();
    assert_eq!(&written[38..44], &[0, 0, 0, 2, 0, 0]);
}
