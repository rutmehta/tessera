//! Small independently encoded single-strip DNG fixture (camera XYZ channels).
pub fn fixture(bits: u16, orientation: u16, pixels: &[[f32; 3]]) -> Vec<u8> {
    calibrated_fixture(
        bits,
        orientation,
        pixels,
        [1., 0., 0., 0., 1., 0., 0., 0., 1.],
        [0.7, 1., 0.6],
    )
}

pub fn calibrated_fixture(
    bits: u16,
    orientation: u16,
    pixels: &[[f32; 3]],
    matrix: [f64; 9],
    neutral: [f64; 3],
) -> Vec<u8> {
    let short = |v: u16| v.to_le_bytes().to_vec();
    let long = |v: u32| v.to_le_bytes().to_vec();
    let rational = |v: f64| {
        [(v * 1_000_000.).round() as i32, 1_000_000]
            .into_iter()
            .flat_map(i32::to_le_bytes)
            .collect::<Vec<_>>()
    };
    let matrix: Vec<u8> = matrix.into_iter().flat_map(rational).collect();
    let neutral: Vec<u8> = neutral.into_iter().flat_map(rational).collect();
    let mut tags = vec![
        (256u16, 4u16, 1u32, long(2)),
        (257, 4, 1, long((pixels.len() / 2) as u32)),
        (258, 3, 3, short(bits).repeat(3)),
        (259, 3, 1, short(1)),
        (262, 3, 1, short(34892)),
        (273, 4, 1, long(0)),
        (274, 3, 1, short(orientation)),
        (277, 3, 1, short(3)),
        (278, 4, 1, long((pixels.len() / 2) as u32)),
        (
            279,
            4,
            1,
            long((pixels.len() * 3 * usize::from(bits / 8)) as u32),
        ),
        (284, 3, 1, short(1)),
        (339, 3, 3, short(if bits == 32 { 3 } else { 1 }).repeat(3)),
        (50706, 1, 4, vec![1, 4, 0, 0]),
        (
            50717,
            4,
            3,
            long(if bits == 32 { 1 } else { 65535 }).repeat(3),
        ),
        (50721, 10, 9, matrix),
        (50728, 5, 3, neutral),
        (50778, 3, 1, short(21)),
    ];
    tags.sort_by_key(|t| t.0);
    let end = 8 + 2 + tags.len() * 12 + 4;
    let mut payload: Vec<u8> = Vec::new();
    let mut entries = Vec::new();
    for (tag, typ, n, data) in &tags {
        entries.extend(tag.to_le_bytes());
        entries.extend(typ.to_le_bytes());
        entries.extend(n.to_le_bytes());
        if data.len() <= 4 {
            let mut v = [0; 4];
            v[..data.len()].copy_from_slice(data);
            entries.extend(v);
        } else {
            entries.extend(((end + payload.len()) as u32).to_le_bytes());
            payload.extend(data);
        }
    }
    let strip_entry = tags.iter().position(|t| t.0 == 273).unwrap() * 12 + 8;
    entries[strip_entry..strip_entry + 4]
        .copy_from_slice(&((end + payload.len()) as u32).to_le_bytes());
    let mut bytes = b"II\x2a\0\x08\0\0\0".to_vec();
    bytes.extend((tags.len() as u16).to_le_bytes());
    bytes.extend(entries);
    bytes.extend([0; 4]);
    bytes.extend(payload);
    for v in pixels.iter().flatten() {
        if bits == 32 {
            bytes.extend(v.to_le_bytes());
        } else {
            bytes.extend(((*v * 65535.).round() as u16).to_le_bytes());
        }
    }
    bytes
}
