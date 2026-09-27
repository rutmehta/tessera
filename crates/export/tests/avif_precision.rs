#![cfg(target_os = "macos")]
use engine_api::jobs::CancellationToken;
use export::{AvifOptions, ColorSpace, encode_avif};

#[test]
fn twelve_bit_encode_does_not_round_through_rgb8() {
    // Both colors round to 46/255 at 8 bits, but differ by five 12-bit codes.
    let pixels = image::Rgba32FImage::from_fn(64, 32, |x, _| {
        let v = if x < 32 { 0.18 } else { 0.1812 };
        image::Rgba([v, v, v, 1.0])
    });
    let data = encode_avif(
        &pixels,
        AvifOptions {
            bits: 12,
            quality: 100,
            speed: 6,
        },
        ColorSpace::Srgb,
        None,
        &CancellationToken::new(),
    )
    .unwrap();
    let tiff = color_mgmt::decode_to_tiff(&data).unwrap();
    let mut decoder = tiff::decoder::Decoder::new(std::io::Cursor::new(tiff)).unwrap();
    let tiff::decoder::DecodingResult::U16(samples) = decoder.read_image().unwrap() else {
        panic!("12-bit AVIF must decode above 8-bit precision");
    };
    let channels = samples.len() / (64 * 32);
    let left = samples[(16 * 64 + 8) * channels];
    let right = samples[(16 * 64 + 48) * channels];
    assert!(
        right > left + 32,
        "12-bit precision lost: {left} vs {right}"
    );
}

#[test]
fn tiny_and_odd_dimensions_roundtrip() {
    for (w, h) in [(1, 1), (3, 7), (33, 25)] {
        let pixels = image::Rgba32FImage::from_pixel(w, h, image::Rgba([0.2, 0.4, 0.6, 1.0]));
        let data = encode_avif(
            &pixels,
            AvifOptions {
                speed: 10,
                ..Default::default()
            },
            ColorSpace::Srgb,
            None,
            &CancellationToken::new(),
        )
        .unwrap();
        let tiff = color_mgmt::decode_to_tiff(&data).unwrap();
        let mut decoder = tiff::decoder::Decoder::new(std::io::Cursor::new(tiff)).unwrap();
        assert_eq!(decoder.dimensions().unwrap(), (w, h));
        decoder.read_image().unwrap();
    }
}
