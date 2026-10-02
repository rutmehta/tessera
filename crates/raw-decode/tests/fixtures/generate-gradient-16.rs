use zune_core::{bit_depth::BitDepth, colorspace::ColorSpace, options::EncoderOptions};
fn main() {
    let data: Vec<u8> = (0..16u16)
        .flat_map(|y| {
            (0..16u16).flat_map(move |x| [1000 + x * 2000, 2000 + y * 1700, 3000 + (x + y) * 700])
        })
        .flat_map(u16::to_ne_bytes)
        .collect();
    let mut tile = Vec::new();
    zune_jpegxl::JxlSimpleEncoder::new(
        &data,
        EncoderOptions::new(16, 16, ColorSpace::RGB, BitDepth::Sixteen),
    )
    .encode(&mut tile)
    .unwrap();
    std::fs::write(
        "crates/raw-decode/tests/fixtures/linear-gradient-16.jxl",
        tile,
    )
    .unwrap();
}
