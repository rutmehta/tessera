use super::*;

fn handle() -> RawFile {
    let raw = unsafe { bindings::libraw_init(0) };
    assert!(!raw.is_null());
    RawFile {
        raw,
        unpacked: false,
    }
}

#[test]
fn dng_crop_is_relative_to_active_origin() {
    let raw = handle();
    unsafe {
        let d = &mut *raw.raw;
        d.sizes.raw_width = 100;
        d.sizes.raw_height = 80;
        d.sizes.width = 90;
        d.sizes.height = 70;
        d.sizes.left_margin = 3;
        d.sizes.top_margin = 5;
        d.idata.dng_version = 0x01040000;
        d.color.dng_levels.default_crop = [2, 4, 60, 50];
    }
    assert_eq!(raw.sensor_info().crop, [5, 9, 60, 50]);
}

#[test]
fn orientation_converts_all_libraw_flags() {
    for (flip, expected) in [1, 2, 4, 3, 5, 8, 6, 7].into_iter().enumerate() {
        assert_eq!(sensor::exif_orientation(flip as i32), expected);
    }
    assert_eq!(sensor::exif_orientation(-1), 1);
}

#[test]
fn metadata_uses_lens_black_wb_matrix_and_absolute_xtrans() {
    let raw = handle();
    let pattern = std::array::from_fn(|y| std::array::from_fn(|x| ((x + y) % 3) as u8));
    unsafe {
        let d = &mut *raw.raw;
        d.idata.filters = 9;
        d.idata.xtrans_abs = pattern.map(|r| r.map(|v| v as std::ffi::c_char));
        d.color.black = 100;
        d.color.cblack[..6].copy_from_slice(&[1, 2, 3, 4, 1, 1]);
        d.color.cblack[6] = 10;
        d.color.maximum = 4095;
        d.color.cam_mul = [2.0, 1.0, 1.5, 1.0];
        d.color.rgb_cam = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ];
        d.lens.Lens[..4].copy_from_slice(&[76, 101, 110, 115]);
    }
    let m = raw.sensor_info();
    assert_eq!(m.cfa_layout, CfaLayout::XTrans(pattern));
    assert_eq!(m.black, [111.0, 112.0, 113.0, 104.0]);
    assert_eq!(m.white, 4095);
    assert_eq!(m.wb_coeffs, [2.0, 1.0, 1.5, 1.0]);
    assert_eq!(m.color_matrix[0], [0.412453, 0.357580, 0.180423]);
    assert_eq!(raw.metadata().lens.as_deref(), Some("Lens"));
    assert!(m.data.is_empty());
    assert_eq!(m.cfa_layout.channel_at(7, 8), pattern[2][1] as usize);
}

#[test]
fn gain_map_parser_respects_lengths_and_big_endian() {
    let words: [u32; 9] = [2, 1, 0, 0, 0, 9, 0, 0, 0];
    let bytes: Vec<u8> = words.into_iter().flat_map(u32::to_be_bytes).collect();
    assert!(sensor::opcode_has_gain_map(&bytes));
    for len in 0..bytes.len() {
        assert!(!sensor::opcode_has_gain_map(&bytes[..len]));
    }
    assert!(!sensor::opcode_has_gain_map(&[0; 4]));
    assert!(!sensor::opcode_has_gain_map(&[255; 20]));
}

#[test]
fn lens_falls_back_to_makernotes() {
    let raw = handle();
    unsafe {
        (&mut (*raw.raw).lens.makernotes.Lens)[..4].copy_from_slice(&[76, 101, 110, 115]);
    }
    assert_eq!(raw.metadata().lens.as_deref(), Some("Lens"));
}

#[test]
fn fixture_previews_select_largest_jpeg_not_bitmap() {
    let root = std::env::var_os("RAW_DECODE_FIXTURES")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw"));
    if !root.exists() {
        eprintln!("skipping preview fixtures: fixtures/raw is absent");
        return;
    }
    for entry in std::fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if !path.is_file() {
            continue;
        }
        let mut raw = RawFile::open(&path).unwrap();
        let largest = unsafe {
            let list = &(*raw.raw).thumbs_list;
            list.thumblist.iter().take(list.thumbcount as usize)
                .filter(|t| t.tformat == bindings::LibRaw_internal_thumbnail_formats_LIBRAW_INTERNAL_THUMBNAIL_JPEG)
                .map(|t| (u64::from(t.twidth) * u64::from(t.theight), t.tlength))
                .max()
        };
        let preview = raw.embedded_preview();
        match largest {
            Some((area, _)) => {
                let jpeg = preview.expect("JPEG listed by LibRaw must decode");
                assert!(jpeg.starts_with(&[255, 216]) && jpeg.ends_with(&[255, 217]));
                let thumb = unsafe { &(*raw.raw).thumbnail };
                assert_eq!(u64::from(thumb.twidth) * u64::from(thumb.theight), area);
            }
            None => {
                assert!(preview.is_none());
                eprintln!("{}: LibRaw lists no JPEG thumbnail", path.display());
            }
        }
    }
}
