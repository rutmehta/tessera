//! Exercise IFD relocation without relying on the exporter's TIFF helpers.
use engine_api::jobs::CancellationToken;
use export::export_original;
use sidecar::Sidecar;

fn tiff(little: bool) -> Vec<u8> {
    let short = |n: u16| {
        if little {
            n.to_le_bytes()
        } else {
            n.to_be_bytes()
        }
    };
    let long = |n: u32| {
        if little {
            n.to_le_bytes()
        } else {
            n.to_be_bytes()
        }
    };
    // Deliberately opaque payload and next-IFD bytes. Copying must not rewrite them.
    let mut bytes = if little {
        b"II".to_vec()
    } else {
        b"MM".to_vec()
    };
    bytes.extend(short(42));
    bytes.extend(long(8));
    bytes.extend(short(2));
    bytes.extend(short(50706));
    bytes.extend(short(1));
    bytes.extend(long(4));
    bytes.extend([1, 6, 0, 0]);
    bytes.extend(short(65000));
    bytes.extend(short(1));
    bytes.extend(long(9));
    bytes.extend(long(38));
    bytes.extend(long(48));
    bytes.extend(b"untouched");
    bytes.push(0);
    bytes.extend(short(0));
    bytes.extend(long(0));
    bytes
}

#[test]
fn both_byte_orders_preserve_opaque_payloads_and_next_ifd() {
    for little in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.dng");
        let output = dir.path().join("output.dng");
        let bytes = tiff(little);
        std::fs::write(&source, &bytes).unwrap();
        export_original(&source, &output, None, None, &CancellationToken::new()).unwrap();
        let out = std::fs::read(&output).unwrap();
        assert_eq!(&out[..4], &bytes[..4]);
        assert_eq!(&out[8..bytes.len()], &bytes[8..]);
        let short = |b: &[u8]| {
            let b = b.try_into().unwrap();
            if little {
                u16::from_le_bytes(b)
            } else {
                u16::from_be_bytes(b)
            }
        };
        let long = |b: &[u8]| {
            let b = b.try_into().unwrap();
            if little {
                u32::from_le_bytes(b)
            } else {
                u32::from_be_bytes(b)
            }
        };
        let root = long(&out[4..8]) as usize;
        assert_eq!(short(&out[root..root + 2]), 3);
        let mut found = false;
        for entry in out[root + 2..root + 38].as_chunks::<12>().0 {
            match short(&entry[..2]) {
                700 => {
                    let at = long(&entry[8..12]) as usize;
                    let len = long(&entry[4..8]) as usize;
                    sidecar::XmpPacket::parse(std::str::from_utf8(&out[at..at + len]).unwrap())
                        .unwrap();
                    found = true;
                }
                50706 => assert_eq!(entry, &bytes[10..22]),
                65000 => assert_eq!(entry, &bytes[22..34]),
                tag => panic!("unexpected tag {tag}"),
            }
        }
        assert!(found);
        assert_eq!(long(&out[root + 38..root + 42]), 48);
        assert!(!Sidecar::paths(&output).xmp.exists());
        assert_eq!(std::fs::read(source).unwrap(), bytes);
    }
}

#[test]
fn malformed_dng_and_sidecar_collision_do_not_publish() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.dng");
    let output = dir.path().join("out.dng");
    let valid = tiff(true);
    for len in 0..38 {
        std::fs::write(&source, &valid[..len]).unwrap();
        assert!(export_original(&source, &output, None, None, &CancellationToken::new()).is_err());
        assert!(!output.exists());
    }
    let mut big_tiff = valid.clone();
    big_tiff[2] = 43;
    std::fs::write(&source, big_tiff).unwrap();
    assert!(export_original(&source, &output, None, None, &CancellationToken::new()).is_err());
    std::fs::write(&source, valid).unwrap();
    let side = Sidecar::paths(&output).xmp;
    std::fs::write(&side, b"existing sidecar").unwrap();
    assert!(export_original(&source, &output, None, None, &CancellationToken::new()).is_err());
    assert!(!output.exists());
    assert_eq!(std::fs::read(side).unwrap(), b"existing sidecar");
}
