//! Synthetic TIFF/DNG profile admission; no camera/vendor profile files.
use pipeline_adobe::dcp::DcpProfile;
#[allow(dead_code)]
#[path = "../../raw-decode/tests/support/mod.rs"]
mod support;

#[test]
fn lr10_reads_embedded_profile_from_linear_raw_subifd_in_both_byte_orders() {
    for be in [false, true] {
        let bytes = support::lossy_dng(be, false);
        let profile = DcpProfile::parse_embedded(&bytes).expect("embedded DNG profile");
        let rgb = profile.apply_tone([0.25; 3]);
        for channel in rgb {
            assert!((channel - 0.52069).abs() < 0.0001);
        }
    }
}

#[test]
fn lr10_embedded_reader_never_reads_pixel_payload_and_rejects_cycles() {
    use std::io::{Read, Seek, SeekFrom};
    struct MetadataOnly {
        inner: std::io::Cursor<Vec<u8>>,
        limit: u64,
    }
    impl Read for MetadataOnly {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            assert!(
                self.inner.position() + buffer.len() as u64 <= self.limit,
                "read pixel payload"
            );
            self.inner.read(buffer)
        }
    }
    impl Seek for MetadataOnly {
        fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
            self.inner.seek(pos)
        }
    }
    let bytes = support::lossy_dng(false, false);
    let count = u16::from_le_bytes(bytes[38..40].try_into().unwrap()) as usize;
    let entry = bytes[40..40 + count * 12]
        .as_chunks::<12>()
        .0
        .iter()
        .find(|e| u16::from_le_bytes(e[..2].try_into().unwrap()) == 324)
        .unwrap();
    let limit = u32::from_le_bytes(entry[8..12].try_into().unwrap()) as u64;
    let mut reader = MetadataOnly {
        inner: std::io::Cursor::new(bytes.clone()),
        limit,
    };
    let extracted = pipeline_adobe::dcp::read_embedded_profile(&mut reader)
        .unwrap()
        .unwrap();
    DcpProfile::parse_embedded(&extracted).unwrap();
    let mut cycle = bytes;
    cycle[34..38].copy_from_slice(&8u32.to_le_bytes());
    assert!(DcpProfile::parse_embedded(&cycle).is_err());
}

#[test]
fn lr10_raw_subifd_inherits_root_tone_and_exposure_tags() {
    let mut bytes = support::lossy_dng(false, false);
    let root = bytes.len() as u32;
    bytes[4..8].copy_from_slice(&root.to_le_bytes());
    // New root keeps the existing raw SubIFD; profile defaults live in IFD0.
    let entries = [
        (254u16, 4u16, 1u32, 1u32),
        (330, 4, 1, 38),
        (50940, 11, 4, root + 54),
        (51110, 4, 1, 1),
    ];
    bytes.extend(4u16.to_le_bytes());
    for (tag, kind, count, value) in entries {
        bytes.extend(tag.to_le_bytes());
        bytes.extend(kind.to_le_bytes());
        bytes.extend(count.to_le_bytes());
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(0u32.to_le_bytes());
    for v in [0f32, 0., 1., 1.] {
        bytes.extend(v.to_le_bytes());
    }
    let profile = DcpProfile::parse_embedded(&bytes).unwrap();
    for v in profile.apply_tone([0.25; 3]) {
        assert!((v - 0.25).abs() < 0.00002);
    }
}
