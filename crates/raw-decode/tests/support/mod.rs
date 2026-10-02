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
    if !opcodes.is_empty() { tags.push((51009, 7, opcodes.len() as u32, opcodes.to_vec())); }
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
