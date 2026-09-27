use std::collections::BTreeMap;

/// Independent classic little-endian TIFF directory reader. Does not decode
/// pixels (the generic TIFF decoder rejects LinearRaw photometric 34892).
pub fn read(path: &std::path::Path) -> BTreeMap<u16, (u16, u32, Vec<u8>)> {
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(&bytes[..4], b"II\x2a\0");
    let short = |at: usize| u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap());
    let long = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let start = long(4) as usize;
    let count = short(start) as usize;
    let mut tags = BTreeMap::new();
    for i in 0..count {
        let entry = start + 2 + 12 * i;
        let tag = short(entry);
        let kind = short(entry + 2);
        let count = long(entry + 4);
        let size = count as usize
            * match kind {
                1 | 2 | 7 => 1,
                3 => 2,
                4 | 9 | 11 => 4,
                5 | 10 | 12 => 8,
                _ => panic!("unsupported TIFF type {kind}"),
            };
        let offset = if size <= 4 {
            entry + 8
        } else {
            long(entry + 8) as usize
        };
        assert!(
            tags.insert(tag, (kind, count, bytes[offset..offset + size].to_vec()))
                .is_none()
        );
    }
    assert_eq!(long(start + 2 + 12 * count), 0);
    tags
}
