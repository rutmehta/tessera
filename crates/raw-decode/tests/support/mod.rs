//! Synthetic only. JPEG generation source lives beside linear-gradient.jpg.
pub fn lossy_dng(be: bool, strips: bool) -> Vec<u8> {
    lossy_dng_with_jpeg(
        be,
        strips,
        include_bytes!("../fixtures/linear-gradient.jpg"),
    )
}

pub fn lossy_dng_with_jpeg(be: bool, strips: bool, jpeg: &[u8]) -> Vec<u8> {
    lossy_dng_with_opcodes(be, strips, jpeg, &[])
}

pub fn lossy_dng_with_opcodes(be: bool, strips: bool, jpeg: &[u8], opcodes: &[u8]) -> Vec<u8> {
    let short = |v: u16| if be { v.to_be_bytes() } else { v.to_le_bytes() };
    let long = |v: u32| if be { v.to_be_bytes() } else { v.to_le_bytes() };
    let shorts = |v: &[u16]| v.iter().flat_map(|&v| short(v)).collect::<Vec<_>>();
    let longs = |v: &[u32]| v.iter().flat_map(|&v| long(v)).collect::<Vec<_>>();
    let mut tags: Vec<(u16, u16, u32, Vec<u8>)> = vec![
        (254, 4, 1, longs(&[0])),
        (256, 4, 1, longs(&[16])),
        (257, 4, 1, longs(&[16])),
        (258, 3, 3, shorts(&[8; 3])),
        (259, 3, 1, shorts(&[34892])),
        (262, 3, 1, shorts(&[34892])),
        (271, 2, 10, b"Synthetic\0".to_vec()),
        (272, 2, 5, b"Test\0".to_vec()),
        (274, 3, 1, shorts(&[6])),
        (277, 3, 1, shorts(&[3])),
        (284, 3, 1, shorts(&[1])),
        (50706, 1, 4, vec![1, 4, 0, 0]),
        (
            50712,
            3,
            256,
            shorts(&(0..256).map(|x| x * 257).collect::<Vec<_>>()),
        ),
        (50714, 5, 1, longs(&[257, 1])),
        (50717, 4, 1, longs(&[65535])),
        (50719, 4, 2, longs(&[2, 3])),
        (50720, 4, 2, longs(&[12, 10])),
        (
            50721,
            10,
            9,
            longs(&[1, 1, 0, 1, 0, 1, 0, 1, 1, 1, 0, 1, 0, 1, 0, 1, 1, 1]),
        ),
        (50728, 5, 3, longs(&[1, 2, 1, 1, 2, 3])),
        (50778, 3, 1, shorts(&[21])),
    ];
    if !opcodes.is_empty() {
        tags.push((51009, 7, opcodes.len() as u32, opcodes.to_vec()));
    }
    tags.extend(if strips {
        vec![
            (273, 4, 1, longs(&[0])),
            (278, 4, 1, longs(&[16])),
            (279, 4, 1, longs(&[jpeg.len() as u32])),
        ]
    } else {
        vec![
            (322, 4, 1, longs(&[16])),
            (323, 4, 1, longs(&[16])),
            (324, 4, 1, longs(&[0])),
            (325, 4, 1, longs(&[jpeg.len() as u32])),
        ]
    });
    tags.sort_by_key(|t| t.0);
    // Thumbnail/root IFD points to the full image via SubIFDs.
    let image_ifd = 38;
    let mut out = if be { b"MM".to_vec() } else { b"II".to_vec() };
    out.extend(short(42));
    out.extend(long(8));
    out.extend(short(2));
    for (tag, value) in [(254, 1), (330, image_ifd)] {
        out.extend(short(tag));
        out.extend(short(4));
        out.extend(long(1));
        out.extend(long(value));
    }
    out.extend(long(0));
    out.extend(short(tags.len() as u16));
    let payload_start = out.len() + tags.len() * 12 + 4;
    let mut payload = Vec::new();
    let mut offset_slot = 0;
    for (tag, typ, n, data) in tags {
        out.extend(short(tag));
        out.extend(short(typ));
        out.extend(long(n));
        if tag == if strips { 273 } else { 324 } {
            offset_slot = out.len();
        }
        if data.len() <= 4 {
            out.extend(&data);
            out.resize(out.len() + 4 - data.len(), 0);
        } else {
            out.extend(long((payload_start + payload.len()) as u32));
            payload.extend(data);
        }
    }
    out.extend(long(0));
    out.extend(payload);
    let end = out.len() as u32;
    out[offset_slot..offset_slot + 4].copy_from_slice(&long(end));
    out.extend(jpeg);
    out
}

/// A 64x48 Bayer DNG with a spatial gradient (any wrong rotation or
/// reflection changes pixels) and the given EXIF orientation.
#[allow(dead_code)]
pub fn bayer_dng(orientation: u16) -> Vec<u8> {
    bayer_dng_sized(orientation, 64, 48, |x, y| {
        3000 + x as u16 * 260 + y as u16 * 90
    })
}

/// A `width`x`height` RGGB DNG (16-bit, white level 65535) with the given
/// EXIF orientation and per-photosite samples.
#[allow(dead_code)]
pub fn bayer_dng_sized(
    orientation: u16,
    width: u32,
    height: u32,
    sample: impl Fn(u32, u32) -> u16,
) -> Vec<u8> {
    let shorts = |v: &[u16]| v.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>();
    let longs = |v: &[u32]| v.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>();
    let mut tags = vec![
        (254u16, 4u16, 1u32, longs(&[0])),
        (256, 4, 1, longs(&[width])),
        (257, 4, 1, longs(&[height])),
        (258, 3, 1, shorts(&[16])),
        (259, 3, 1, shorts(&[1])),
        (262, 3, 1, shorts(&[32803])),
        (271, 2, 6, b"NIKON\0".to_vec()),
        (272, 2, 11, b"NIKON D850\0".to_vec()),
        (273, 4, 1, longs(&[0])),
        (274, 3, 1, shorts(&[orientation])),
        (277, 3, 1, shorts(&[1])),
        (278, 4, 1, longs(&[height])),
        (279, 4, 1, longs(&[width * height * 2])),
        (33421, 3, 2, shorts(&[2, 2])),
        (33422, 1, 4, vec![0, 1, 1, 2]),
        (50706, 1, 4, vec![1, 4, 0, 0]),
        (50707, 1, 4, vec![1, 1, 0, 0]),
        (50708, 2, 15, b"Synthetic Test\0".to_vec()),
        (50710, 1, 3, vec![0, 1, 2]),
        (50711, 3, 1, shorts(&[1])),
        (50714, 5, 1, longs(&[0, 1])),
        (50717, 4, 1, longs(&[65535])),
        (
            50721,
            10,
            9,
            longs(&[1, 1, 0, 1, 0, 1, 0, 1, 1, 1, 0, 1, 0, 1, 0, 1, 1, 1]),
        ),
        (50728, 5, 3, longs(&[1, 2, 1, 1, 2, 3])),
        (50778, 3, 1, shorts(&[21])),
    ];
    let mut second = tags.iter().find(|t| t.0 == 50721).unwrap().clone();
    second.0 = 50722;
    tags.push(second);
    tags.push((50779, 3, 1, shorts(&[17])));
    tags.sort_by_key(|t| t.0);
    let mut out = vec![0; 14 + tags.len() * 12];
    out[..8].copy_from_slice(&[73, 73, 42, 0, 8, 0, 0, 0]);
    out[8..10].copy_from_slice(&(tags.len() as u16).to_le_bytes());
    let mut strip_slot = 0;
    for (i, (tag, kind, count, data)) in tags.into_iter().enumerate() {
        let p = 10 + i * 12;
        out[p..p + 2].copy_from_slice(&tag.to_le_bytes());
        out[p + 2..p + 4].copy_from_slice(&kind.to_le_bytes());
        out[p + 4..p + 8].copy_from_slice(&count.to_le_bytes());
        if tag == 273 {
            strip_slot = p + 8;
        }
        if data.len() <= 4 {
            out[p + 8..p + 8 + data.len()].copy_from_slice(&data);
        } else {
            let offset = out.len() as u32;
            out[p + 8..p + 12].copy_from_slice(&offset.to_le_bytes());
            out.extend(data);
        }
    }
    let offset = out.len() as u32;
    out[strip_slot..strip_slot + 4].copy_from_slice(&offset.to_le_bytes());
    out.reserve(width as usize * height as usize * 2);
    for y in 0..height {
        for x in 0..width {
            out.extend(sample(x, y).to_le_bytes());
        }
    }
    out
}
