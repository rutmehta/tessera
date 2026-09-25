//! Bounded classic-TIFF DNG opcode extraction, independent of LibRaw.
use std::collections::HashSet;
use std::io::{self, Read, Seek, SeekFrom};

/// Raw bytes grouped by source IFD, never merged across preview/raw images.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DngOpcodeLists {
    pub ifd_offset: u32,
    /// OpcodeList1, OpcodeList2, OpcodeList3, respectively. Payloads are big-endian.
    pub lists: [Option<Vec<u8>>; 3],
}
fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn read_at<R: Read + Seek>(r: &mut R, offset: u64, n: usize, size: u64) -> io::Result<Vec<u8>> {
    if offset.checked_add(n as u64).is_none_or(|end| end > size) {
        return Err(invalid("TIFF range outside file"));
    }
    r.seek(SeekFrom::Start(offset))?;
    let mut b = vec![0; n];
    r.read_exact(&mut b)?;
    Ok(b)
}
/// Reads classic TIFF (II/MM, magic 42) IFD and SubIFD graphs from stream start.
/// Requires a DNGVersion tag. Non-TIFF/non-DNG returns empty; BigTIFF is unsupported.
/// Limits: 256 IFDs, 4096 entries/IFD, 4 MiB/list, 12 MiB total opcode data.
/// Malformed data returns InvalidData/UnexpectedEof; no partial lists escape.
/// Leaves the stream position unspecified. Does not interpret opcode payloads.
pub fn extract_dng_opcode_lists<R: Read + Seek>(r: &mut R) -> io::Result<Vec<DngOpcodeLists>> {
    let size = r.seek(SeekFrom::End(0))?;
    if size < 2 {
        return Ok(Vec::new());
    }
    let sig = read_at(r, 0, 2, size)?;
    let le = match sig.as_slice() {
        b"II" => true,
        b"MM" => false,
        _ => return Ok(Vec::new()),
    };
    let h = read_at(r, 0, 8, size)?;
    let u16v = |b: &[u8]| {
        let a = [b[0], b[1]];
        if le {
            u16::from_le_bytes(a)
        } else {
            u16::from_be_bytes(a)
        }
    };
    let u32v = |b: &[u8]| {
        let a = [b[0], b[1], b[2], b[3]];
        if le {
            u32::from_le_bytes(a)
        } else {
            u32::from_be_bytes(a)
        }
    };
    if u16v(&h[2..]) != 42 {
        return Err(invalid("unsupported TIFF magic (BigTIFF not supported)"));
    }
    let mut pending = vec![u32v(&h[4..])];
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let mut dng = false;
    let mut total = 0usize;
    while let Some(offset) = pending.pop() {
        if offset == 0 {
            continue;
        }
        if !seen.insert(offset) {
            return Err(invalid("cyclic or aliased IFD graph"));
        }
        if seen.len() > 256 {
            return Err(invalid("too many IFDs"));
        }
        let n = u16v(&read_at(r, offset as u64, 2, size)?) as usize;
        if n > 4096 {
            return Err(invalid("too many TIFF entries"));
        }
        let table = read_at(r, offset as u64 + 2, n * 12 + 4, size)?;
        let mut lists = [None, None, None];
        for e in table[..n * 12].as_chunks::<12>().0 {
            let tag = u16v(e);
            let typ = u16v(&e[2..]);
            let count = u32v(&e[4..]) as usize;
            if tag == 50706 && typ == 1 && count == 4 {
                dng = true;
            }
            let index = match tag {
                51008 => Some(0),
                51009 => Some(1),
                51022 => Some(2),
                _ => None,
            };
            if let Some(i) = index {
                if typ != 7 || !(4..=4 * 1024 * 1024).contains(&count) || lists[i].is_some() {
                    return Err(invalid("invalid/duplicate opcode tag"));
                }
                total += count;
                if total > 12 * 1024 * 1024 {
                    return Err(invalid("opcode byte budget exceeded"));
                }
                lists[i] = Some(if count <= 4 {
                    e[8..8 + count].to_vec()
                } else {
                    read_at(r, u32v(&e[8..]) as u64, count, size)?
                });
            } else if tag == 330 {
                if !matches!(typ, 4 | 13) || count > 256 || pending.len() + count > 256 {
                    return Err(invalid("invalid SubIFDs"));
                }
                let b = if count <= 1 {
                    e[8..8 + count * 4].to_vec()
                } else {
                    read_at(r, u32v(&e[8..]) as u64, count * 4, size)?
                };
                pending.extend(b.as_chunks::<4>().0.iter().map(|v| u32v(v)));
            }
        }
        if lists.iter().any(Option::is_some) {
            out.push(DngOpcodeLists {
                ifd_offset: offset,
                lists,
            });
        }
        pending.push(u32v(&table[n * 12..]));
    }
    if dng { Ok(out) } else { Ok(Vec::new()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    #[test]
    fn rejects_malformed_tiff_without_partial_results() {
        let b = fixture(true);
        for n in 2..b.len() {
            assert!(
                extract_dng_opcode_lists(&mut Cursor::new(&b[..n])).is_err(),
                "prefix {n}"
            );
        }
        for (at, value) in [(26, 0u32), (38, u32::MAX), (42, u32::MAX), (58, 8)] {
            let mut bad = b.clone();
            bad[at..at + 4].copy_from_slice(&value.to_le_bytes());
            assert!(
                extract_dng_opcode_lists(&mut Cursor::new(bad)).is_err(),
                "offset {at}"
            );
        }
        let mut bad = b.clone();
        bad[24..26].copy_from_slice(&4u16.to_le_bytes());
        assert!(extract_dng_opcode_lists(&mut Cursor::new(bad)).is_err());
        let mut non_dng = b;
        non_dng[10..12].copy_from_slice(&1u16.to_le_bytes());
        assert!(
            extract_dng_opcode_lists(&mut Cursor::new(non_dng))
                .unwrap()
                .is_empty()
        );
        assert!(
            extract_dng_opcode_lists(&mut Cursor::new(b"not a tiff"))
                .unwrap()
                .is_empty()
        );
    }
    #[test]
    fn traverses_subifds_without_merging_image_lists() {
        let mut b = fixture(true);
        // Replace first list with a SubIFD pointing at a second IFD.
        b[22..24].copy_from_slice(&330u16.to_le_bytes());
        b[24..26].copy_from_slice(&4u16.to_le_bytes());
        b[26..30].copy_from_slice(&1u32.to_le_bytes());
        b[30..34].copy_from_slice(&68u32.to_le_bytes());
        b.push(0);
        b.extend(1u16.to_le_bytes());
        b.extend(51008u16.to_le_bytes());
        b.extend(7u16.to_le_bytes());
        b.extend(4u32.to_le_bytes());
        b.extend([0; 8]);
        let lists = extract_dng_opcode_lists(&mut Cursor::new(b)).unwrap();
        assert_eq!(lists.len(), 2);
        assert_eq!(lists[0].ifd_offset, 8);
        assert_eq!(lists[0].lists[0], None);
        assert_eq!(lists[1].ifd_offset, 68);
        assert_eq!(lists[1].lists[0], Some(vec![0; 4]));
    }
    fn fixture(le: bool) -> Vec<u8> {
        let u16b = |v: u16| if le { v.to_le_bytes() } else { v.to_be_bytes() };
        let u32b = |v: u32| if le { v.to_le_bytes() } else { v.to_be_bytes() };
        let mut b = if le { b"II".to_vec() } else { b"MM".to_vec() };
        b.extend(u16b(42));
        b.extend(u32b(8));
        b.extend(u16b(4));
        for (tag, typ, count, value) in [
            (50706, 1, 4, 0x00000401),
            (51008, 7, 4, 0),
            (51009, 7, 5, 62),
            (51022, 7, 4, 0),
        ] {
            b.extend(u16b(tag));
            b.extend(u16b(typ));
            b.extend(u32b(count));
            b.extend(u32b(value));
        }
        b.extend(u32b(0));
        b.extend([0, 0, 0, 0, 99]);
        b
    }
    #[test]
    fn extracts_three_lists_in_both_endian_tiffs() {
        for le in [true, false] {
            let lists = extract_dng_opcode_lists(&mut Cursor::new(fixture(le))).unwrap();
            assert_eq!(lists.len(), 1);
            assert_eq!(lists[0].ifd_offset, 8);
            assert_eq!(lists[0].lists[0], Some(vec![0; 4]));
            assert_eq!(lists[0].lists[1], Some(vec![0, 0, 0, 0, 99]));
            assert_eq!(lists[0].lists[2], Some(vec![0; 4]));
        }
    }
}
