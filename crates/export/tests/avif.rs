use engine_api::jobs::CancellationToken;
use export::{AvifOptions, ColorSpace, encode_avif};

fn boxes(mut input: &[u8]) -> Vec<(&[u8], &[u8])> {
    let mut result = Vec::new();
    while !input.is_empty() {
        assert!(input.len() >= 8);
        let len = u32::from_be_bytes(input[..4].try_into().unwrap()) as usize;
        assert!((8..=input.len()).contains(&len));
        result.push((&input[4..8], &input[8..len]));
        input = &input[len..];
    }
    result
}

fn child<'a>(input: &'a [u8], kind: &[u8]) -> &'a [u8] {
    boxes(input)
        .into_iter()
        .find(|(k, _)| *k == kind)
        .unwrap()
        .1
}

#[test]
fn avif_depths_profiles_alpha_and_xmp_roundtrip() {
    let pixels = image::Rgba32FImage::from_fn(32, 24, |x, _| {
        image::Rgba([0.2, 0.4, 0.7, if x < 16 { 0.25 } else { 1.0 }])
    });
    for bits in [8, 10, 12] {
        for space in [
            ColorSpace::Srgb,
            ColorSpace::DisplayP3,
            ColorSpace::Rec2020,
            ColorSpace::ProPhoto,
        ] {
            let bytes = encode_avif(
                &pixels,
                AvifOptions {
                    bits,
                    quality: 100,
                    speed: 10,
                },
                space,
                Some("<xmp>copyright test</xmp>"),
                &CancellationToken::new(),
            )
            .unwrap();
            assert_eq!(&bytes[4..8], b"ftyp");
            assert!(bytes.windows(4).any(|v| v == b"prof"));
            assert!(bytes.windows(4).any(|v| v == b"nclx"));
            assert!(bytes.windows(14).any(|v| v == b"copyright test"));
            let meta = &child(&bytes, b"meta")[4..];
            let ipco = child(child(meta, b"iprp"), b"ipco");
            let props = boxes(ipco);
            assert_eq!(child(ipco, b"pixi"), &[0, 0, 0, 0, 3, bits, bits, bits]);
            let colr: Vec<_> = props
                .iter()
                .filter(|(k, _)| *k == b"colr")
                .map(|(_, v)| *v)
                .collect();
            let (primaries, transfer) = match space {
                ColorSpace::Srgb => (1u16, 13u16),
                ColorSpace::DisplayP3 => (12, 13),
                ColorSpace::Rec2020 => (9, 2),
                ColorSpace::ProPhoto => (2, 2),
            };
            assert_eq!(
                colr[0],
                &[
                    b"nclx".as_slice(),
                    &primaries.to_be_bytes(),
                    &transfer.to_be_bytes(),
                    &[0, 0, 128]
                ]
                .concat()
            );
            assert_eq!(&colr[1][..4], b"prof");
            let mut registry = color_mgmt::Registry::new();
            let embedded = registry.load_bytes(&colr[1][4..]).unwrap();
            let expected = registry
                .load_bytes(&export::color_space_icc(space).unwrap())
                .unwrap();
            let source = registry.builtin(color_mgmt::Builtin::Srgb).unwrap();
            let a = color_mgmt::Transform::new(&source, &embedded, Default::default()).unwrap();
            let b = color_mgmt::Transform::new(&source, &expected, Default::default()).unwrap();
            assert_eq!(a.apply([0.1, 0.3, 0.9]), b.apply([0.1, 0.3, 0.9]));
            assert_eq!(props.iter().filter(|(k, _)| *k == b"av1C").count(), 2);
            assert_eq!(
                &child(child(meta, b"iprp"), b"ipma")[4..8],
                &2u32.to_be_bytes()
            );
            #[cfg(target_os = "macos")]
            {
                let tiff = color_mgmt::decode_to_tiff(&bytes).unwrap();
                let mut decoder = tiff::decoder::Decoder::new(std::io::Cursor::new(tiff)).unwrap();
                assert_eq!(decoder.dimensions().unwrap(), (32, 24));
                let premultiplied = decoder
                    .get_tag_u16_vec(tiff::tags::Tag::ExtraSamples)
                    .unwrap()[0]
                    == 1;
                let decoded = match decoder.read_image().unwrap() {
                    tiff::decoder::DecodingResult::U8(v) => {
                        v.into_iter().map(|s| s as f32 / 255.0).collect::<Vec<_>>()
                    }
                    tiff::decoder::DecodingResult::U16(v) => {
                        v.into_iter().map(|s| s as f32 / 65535.0).collect()
                    }
                    other => panic!("unexpected {other:?}"),
                };
                assert_eq!(decoded.len(), pixels.as_raw().len());
                // Interpret TIFF's alpha convention rather than ignoring RGB
                // under partial transparency.
                for (i, p) in decoded.as_chunks::<4>().0.iter().enumerate() {
                    let expected = pixels.as_raw()[i * 4 + 3];
                    assert!((p[3] - expected).abs() < 0.02, "alpha bits={bits}: {p:?}");
                    for c in 0..3 {
                        let actual = if premultiplied { p[c] / p[3] } else { p[c] };
                        assert!(
                            (actual - pixels.as_raw()[i * 4 + c]).abs() < 0.025,
                            "RGB bits={bits} space={space:?}: {p:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn avif_rejects_invalid_options_and_honors_cancellation() {
    let image = image::Rgba32FImage::new(16, 16);
    for options in [
        AvifOptions {
            bits: 16,
            ..Default::default()
        },
        AvifOptions {
            quality: 0,
            ..Default::default()
        },
        AvifOptions {
            speed: 11,
            ..Default::default()
        },
    ] {
        assert!(
            encode_avif(
                &image,
                options,
                ColorSpace::Srgb,
                None,
                &CancellationToken::new()
            )
            .is_err()
        );
    }
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(matches!(
        encode_avif(
            &image,
            AvifOptions::default(),
            ColorSpace::Srgb,
            None,
            &cancel
        ),
        Err(engine_api::EngineError::Cancelled)
    ));
}
