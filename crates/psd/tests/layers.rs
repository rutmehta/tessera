use psd::PsdDocument;

fn minimal_layer_file() -> Vec<u8> {
    let mut info = 1i16.to_be_bytes().to_vec();
    for v in [0i32, 0, 1, 2] {
        info.extend_from_slice(&v.to_be_bytes());
    }
    info.extend_from_slice(&1u16.to_be_bytes());
    info.extend_from_slice(&0i16.to_be_bytes());
    info.extend_from_slice(&4u32.to_be_bytes());
    info.extend_from_slice(b"8BIMmul ");
    info.extend_from_slice(&[127, 1, 2, 0]);
    info.extend_from_slice(&12u32.to_be_bytes());
    info.extend_from_slice(&[0; 8]);
    info.extend_from_slice(&[1, b'L', 0, 0]);
    info.extend_from_slice(&[0, 0, 7, 42]);
    let mut lm = (info.len() as u32).to_be_bytes().to_vec();
    lm.extend(info);
    lm.extend_from_slice(&[0; 4]);
    [
        b"8BPS".as_slice(),
        &1u16.to_be_bytes(),
        &[0; 6],
        &1u16.to_be_bytes(),
        &1u32.to_be_bytes(),
        &2u32.to_be_bytes(),
        &8u16.to_be_bytes(),
        &1u16.to_be_bytes(),
        &[0; 8],
        &(lm.len() as u32).to_be_bytes(),
        &lm,
        &[0, 0, 7, 42],
    ]
    .concat()
}
#[test]
fn hand_built_layer_roundtrip_and_truncation() {
    let b = minimal_layer_file();
    let d = PsdDocument::read(&b).unwrap();
    assert_eq!(PsdDocument::read(&d.write().unwrap()).unwrap(), d);
    for n in 0..b.len() {
        assert!(PsdDocument::read(&b[..n]).is_err(), "{n}");
    }
}
