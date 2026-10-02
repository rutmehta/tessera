//! Synthetic camera-channel fixture: no photo data.
#[allow(dead_code)]
#[path = "../../raw-decode/tests/support/mod.rs"]
mod support;
use zune_core::{bit_depth::BitDepth, colorspace::ColorSpace, options::EncoderOptions};
#[test]
fn jxl_linear_dng_preserves_16_bit_camera_codes() {
    let samples: Vec<u16> = (0..16).flat_map(|y| (0..16).flat_map(move |x| [1000+x*2000, 2000+y*1700, 3000+(x+y)*700])).collect();
    let data: Vec<u8> = samples.iter().flat_map(|v| v.to_ne_bytes()).collect();
    let mut tile = Vec::new();
    zune_jpegxl::JxlSimpleEncoder::new(&data, EncoderOptions::new(16,16,ColorSpace::RGB,BitDepth::Sixteen)).encode(&mut tile).unwrap();
    for strips in [false, true] {
        let mut dng = support::lossy_dng_with_jpeg(false, strips, &tile);
        let n = u16::from_le_bytes(dng[38..40].try_into().unwrap()) as usize;
        for i in 0..n {
            let p = 40+i*12;
            let tag = u16::from_le_bytes(dng[p..p+2].try_into().unwrap());
            if tag == 259 { dng[p+8..p+10].copy_from_slice(&52546_u16.to_le_bytes()); }
            if tag == 258 {
                let offset = u32::from_le_bytes(dng[p+8..p+12].try_into().unwrap()) as usize;
                for c in 0..3 { dng[offset+c*2..offset+c*2+2].copy_from_slice(&16_u16.to_le_bytes()); }
            }
            // Omit the classic 8-bit lookup table; camera codes are already linear.
            if tag == 50712 { dng[p..p+2].copy_from_slice(&65000_u16.to_le_bytes()); }
        }
        let decoded = raw_decode::lossy_dng::read(&mut std::io::Cursor::new(dng)).unwrap().unwrap();
        assert_eq!((decoded.width,decoded.height),(12,10));
        for y in 0..10 { for x in 0..12 { for c in 0..3 {
            let expected = (samples[((y+3)*16+x+2)*3+c] as f32-257.)/(65535.-257.);
            assert!((decoded.pixels[y*12+x][c]-expected).abs()<2e-6);
        }}}
    }
}
