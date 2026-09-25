use super::*;

#[test]
fn shared_profiles_are_reused_and_tiff_quantizes_target_encoded_pixels() {
    use color_mgmt::{Builtin, Registry, Transform, TransformOptions};
    let mut registry = Registry::new();
    let source = registry.builtin(Builtin::Srgb).unwrap();
    let samples = [
        [0.0, 0.0, 0.0],
        [0.18, 0.4, 0.8],
        [1.0, 0.0, 0.0],
        [0.9, 0.9, 0.9],
    ];
    let image = image::Rgb32FImage::from_raw(4, 1, samples.as_flattened().to_vec()).unwrap();
    for (space, builtin) in [
        (ColorSpace::Srgb, Builtin::Srgb),
        (ColorSpace::DisplayP3, Builtin::DisplayP3),
        (ColorSpace::Rec2020, Builtin::Rec2020),
        (ColorSpace::ProPhoto, Builtin::ProPhoto),
    ] {
        let target = profile(&mut registry, space).unwrap();
        assert!(std::sync::Arc::ptr_eq(
            &target,
            &registry.builtin(builtin).unwrap()
        ));
        let transform = Transform::new(&source, &target, TransformOptions::default()).unwrap();
        let mut bytes = std::io::Cursor::new(Vec::new());
        encode(
            &mut bytes,
            &image,
            Format::Tiff { bits: 16 },
            space,
            None,
            &CancellationToken::new(),
        )
        .unwrap();
        bytes.set_position(0);
        let mut decoder = tiff::decoder::Decoder::new(bytes).unwrap();
        let icc = decoder
            .get_tag_u8_vec(tiff::tags::Tag::Unknown(34675))
            .unwrap();
        let embedded = registry.load_bytes(&icc).unwrap();
        let embedded_transform =
            Transform::new(&source, &embedded, TransformOptions::default()).unwrap();
        let tiff::decoder::DecodingResult::U16(actual) = decoder.read_image().unwrap() else {
            panic!("expected u16")
        };
        for (input, actual) in samples.into_iter().zip(actual.as_chunks::<3>().0) {
            let expected = input.map(|v| (v.clamp(0.0, 1.0) * 65535.0).round() as u16);
            assert_eq!(*actual, expected);
            for c in 0..3 {
                assert!(
                    (embedded_transform.apply(input)[c] - transform.apply(input)[c]).abs() < 1e-6
                );
            }
        }
    }
}

struct CancelOnWrite {
    inner: std::io::Cursor<Vec<u8>>,
    cancel: CancellationToken,
}
impl Write for CancelOnWrite {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.cancel.cancel();
        self.inner.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl Seek for CancelOnWrite {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(pos)
    }
}
#[test]
fn cancel_during_encoding_stops_writes() {
    for format in [
        Format::Jpeg { quality: 90 },
        Format::Png,
        Format::Tiff { bits: 16 },
    ] {
        let cancel = CancellationToken::new();
        let mut writer = CancelOnWrite {
            inner: std::io::Cursor::new(Vec::new()),
            cancel: cancel.clone(),
        };
        let image = image::Rgb32FImage::from_pixel(64, 64, image::Rgb([0.2, 0.3, 0.4]));
        assert_eq!(
            encode(&mut writer, &image, format, ColorSpace::Srgb, None, &cancel),
            Err(engine_api::EngineError::Cancelled)
        );
        assert!(writer.inner.get_ref().len() < 100);
    }
}
