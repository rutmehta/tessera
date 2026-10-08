//! Synthetic RAF blobs: the RAF directory, an embedded JPEG whose EXIF maker
//! note carries CropMode, and a CFA section TIFF whose IFD0 points to the
//! FujiIFD with the three correction tags (signed rationals).
use super::*;
use std::io::Cursor;

const KNOTS_11: [f64; 11] = [0., 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.];
const DIST_11: [f64; 11] = [
    0., 0.102, 0.205, 0.307, 0.408, 0.517, 0.66, 0.879, 1.184, 1.598, 2.158,
];
const CAR_10: [f64; 10] = [
    0.000103, 0.000188, 0.000238, 0.000235, 0.000183, 0.000102, 7e-06, -0.000102, -0.000227,
    -0.000366,
];
const CAB_10: [f64; 10] = [
    -3.8e-05, -6.6e-05, -7.1e-05, -4.2e-05, 3e-05, 0.00014, 0.000235, 0.000376, 0.00054, 0.000793,
];
const VIG_11: [f64; 11] = [
    100., 99.92, 99.77, 99.46, 98.94, 98.43, 98.11, 97.29, 96.78, 95.88, 94.9,
];

/// Values of the three tags in the X-Trans I/II/III layout (23, 31, 23).
fn layout_23() -> [Vec<f64>; 3] {
    let mut d = vec![267.4545455];
    d.extend(KNOTS_11);
    d.extend(DIST_11);
    let mut c = vec![294.2];
    c.extend(&KNOTS_11[1..]);
    c.extend(CAR_10);
    c.extend(CAB_10);
    let mut v = vec![267.4545455];
    v.extend(KNOTS_11);
    v.extend(VIG_11);
    [d, c, v]
}

/// The X-Trans IV/V layout (19, 29, 19): 9 knots, CA knots included.
fn layout_19() -> [Vec<f64>; 3] {
    let knots: Vec<f64> = (0..9).map(|i| i as f64 / 8.).collect();
    let mut d = vec![300.];
    d.extend(&knots);
    d.extend((0..9).map(|i| -0.25 * i as f64));
    let mut c = vec![300.];
    c.extend(&knots);
    c.extend((0..9).map(|i| 1e-5 * i as f64));
    c.extend((0..9).map(|i| -2e-5 * i as f64));
    c.push(0.);
    let mut v = vec![300.];
    v.extend(&knots);
    v.extend((0..9).map(|i| 100. - i as f64));
    [d, c, v]
}

fn u16le(v: u16) -> [u8; 2] {
    v.to_le_bytes()
}
fn u32le(v: u32) -> [u8; 4] {
    v.to_le_bytes()
}

/// A little-endian IFD at `at` (absolute offset within `out`); out-of-line
/// values are appended after it. `base` is subtracted from stored offsets.
fn ifd(out: &mut Vec<u8>, base: usize, entries: &[(u16, u16, u32, Vec<u8>)]) {
    let start = out.len();
    let mut data = start + 2 + entries.len() * 12 + 4;
    let mut tail: Vec<u8> = Vec::new();
    out.extend(u16le(entries.len() as u16));
    for (tag, kind, count, bytes) in entries {
        out.extend(u16le(*tag));
        out.extend(u16le(*kind));
        out.extend(u32le(*count));
        if bytes.len() <= 4 {
            let mut v = bytes.clone();
            v.resize(4, 0);
            out.extend(v);
        } else {
            out.extend(u32le((data - base) as u32));
            tail.extend(bytes);
            data += bytes.len();
        }
    }
    out.extend(u32le(0));
    out.extend(tail);
}

fn srational(values: &[f64]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|v| {
            let n = (v * 1e6).round() as i32;
            [n.to_le_bytes(), 1_000_000i32.to_le_bytes()].concat()
        })
        .collect()
}

/// The CFA section: a TIFF whose IFD0 has tag 0xF000 → FujiIFD.
fn cfa_tiff(tags: &[(u16, u16, Vec<f64>)]) -> Vec<u8> {
    let mut t = b"II*\0".to_vec();
    t.extend(u32le(8));
    // IFD0 at 8 with one entry, FujiIFD right after (8 + 2 + 12 + 4 = 26).
    ifd(&mut t, 0, &[(0xf000, 13, 1, u32le(26).to_vec())]);
    assert_eq!(t.len(), 26);
    let entries: Vec<_> = tags
        .iter()
        .map(|(tag, kind, values)| (*tag, *kind, values.len() as u32, srational(values)))
        .collect();
    let mut fuji = vec![(0xf001, 4, 1, u32le(4992).to_vec())];
    fuji.extend(entries);
    ifd(&mut t, 0, &fuji);
    t.resize(t.len() + 64, 0xAB); // raw samples follow
    t
}

/// A JPEG with APP1 EXIF: IFD0 → ExifIFD → Fujifilm maker note (CropMode).
fn jpeg(crop_mode: Option<u16>) -> Vec<u8> {
    let mut tiff = b"II*\0".to_vec();
    tiff.extend(u32le(8));
    ifd(&mut tiff, 0, &[(0x8769, 4, 1, u32le(26).to_vec())]);
    let mut note = b"FUJIFILM".to_vec();
    note.extend(u32le(12));
    let mut entries = vec![(0x1000, 2, 4, b"NOR\0".to_vec())];
    if let Some(mode) = crop_mode {
        entries.push((0x104d, 3, 1, u16le(mode).to_vec()));
    }
    ifd(&mut note, 0, &entries);
    ifd(&mut tiff, 0, &[(0x927c, 7, note.len() as u32, note)]);
    let mut app1 = b"Exif\0\0".to_vec();
    app1.extend(tiff);
    let mut j = vec![0xff, 0xd8, 0xff, 0xe1];
    j.extend(((app1.len() + 2) as u16).to_be_bytes());
    j.extend(app1);
    j.extend([0xff, 0xd9]);
    j
}

fn raf(jpeg: &[u8], cfa: &[u8]) -> Vec<u8> {
    let mut f = MAGIC.to_vec();
    f.extend(b"0201FA119507X-E2S");
    f.resize(0x3c, 0);
    f.extend(b"0102");
    f.resize(84, 0);
    let jpeg_at = 0x200u32;
    let cfa_at = jpeg_at + jpeg.len() as u32;
    for v in [jpeg_at, jpeg.len() as u32, 0, 0, cfa_at, cfa.len() as u32] {
        f.extend(v.to_be_bytes());
    }
    f.resize(jpeg_at as usize, 0);
    f.extend(jpeg);
    f.extend(cfa);
    f
}

fn tags([d, c, v]: [Vec<f64>; 3]) -> Vec<(u16, u16, Vec<f64>)> {
    vec![(0xf00b, 10, d), (0xf00f, 10, c), (0xf010, 10, v)]
}

fn extract(file: Vec<u8>) -> Option<FujifilmLens> {
    extract_raf_lens(&mut Cursor::new(file))
        .unwrap()
        .map(|MakerLens::Fujifilm(f)| f)
}

fn close(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
}

#[test]
fn xtrans_i_to_iii_layout_is_read() {
    let f = extract(raf(&jpeg(None), &cfa_tiff(&tags(layout_23())))).expect("correction");
    assert!(close(&f.knots, &KNOTS_11), "{:?}", f.knots);
    assert!(close(&f.distortion, &DIST_11));
    // The CA tag has no first (zero) knot: it is zero there.
    let mut red = vec![0.];
    red.extend(CAR_10);
    let mut blue = vec![0.];
    blue.extend(CAB_10);
    assert!(close(&f.ca_red, &red), "{:?}", f.ca_red);
    assert!(close(&f.ca_blue, &blue));
    assert!(close(&f.vignetting, &VIG_11));
    assert_eq!(f.crop_factor, 1.);
}

#[test]
fn xtrans_iv_v_layout_is_read() {
    let [d, c, v] = layout_19();
    let f = extract(raf(
        &jpeg(Some(0)),
        &cfa_tiff(&tags([d.clone(), c.clone(), v.clone()])),
    ))
    .expect("correction");
    assert!(close(&f.knots, &d[1..10]));
    assert!(close(&f.distortion, &d[10..19]));
    assert!(close(&f.ca_red, &c[10..19]));
    assert!(close(&f.ca_blue, &c[19..28]));
    assert!(close(&f.vignetting, &v[10..19]));
    assert_eq!(f.crop_factor, 1.);
}

#[test]
fn crop_mode_1_25x_scales_the_knots() {
    for (mode, factor) in [
        (None, 1.),
        (Some(1), 1.),
        (Some(2), 1.25),
        (Some(4), 1.25),
        (Some(8), 1.),
    ] {
        let f = extract(raf(&jpeg(mode), &cfa_tiff(&tags(layout_23())))).unwrap();
        assert_eq!(f.crop_factor, factor, "CropMode {mode:?}");
    }
    // A JPEG without EXIF still yields the correction, at crop factor 1.
    let f = extract(raf(
        &[0xff, 0xd8, 0xff, 0xd9],
        &cfa_tiff(&tags(layout_23())),
    ))
    .unwrap();
    assert_eq!(f.crop_factor, 1.);
}

#[test]
fn big_endian_tiffs_are_read() {
    // Swap the CFA TIFF to Motorola order by re-encoding it.
    let le = cfa_tiff(&tags(layout_23()));
    let be = to_big_endian(&le);
    let f = extract(raf(&jpeg(None), &be)).expect("big-endian FujiIFD");
    assert!(close(&f.distortion, &DIST_11));
}

/// Re-encode the little-endian TIFF built by `cfa_tiff` in Motorola order.
fn to_big_endian(le: &[u8]) -> Vec<u8> {
    let mut b = le.to_vec();
    b[..4].copy_from_slice(b"MM\0*");
    let sw32 = |b: &mut Vec<u8>, at: usize| {
        let v = u32::from_le_bytes(b[at..at + 4].try_into().unwrap());
        b[at..at + 4].copy_from_slice(&v.to_be_bytes());
    };
    let sw16 = |b: &mut Vec<u8>, at: usize| {
        let v = u16::from_le_bytes(b[at..at + 2].try_into().unwrap());
        b[at..at + 2].copy_from_slice(&v.to_be_bytes());
    };
    sw32(&mut b, 4);
    let mut pending = vec![8usize];
    while let Some(at) = pending.pop() {
        let n = u16::from_le_bytes(b[at..at + 2].try_into().unwrap()) as usize;
        sw16(&mut b, at);
        for e in 0..n {
            let p = at + 2 + 12 * e;
            let tag = u16::from_le_bytes(b[p..p + 2].try_into().unwrap());
            let kind = u16::from_le_bytes(b[p + 2..p + 4].try_into().unwrap());
            let count = u32::from_le_bytes(b[p + 4..p + 8].try_into().unwrap()) as usize;
            let value = u32::from_le_bytes(b[p + 8..p + 12].try_into().unwrap()) as usize;
            sw16(&mut b, p);
            sw16(&mut b, p + 2);
            sw32(&mut b, p + 4);
            sw32(&mut b, p + 8);
            if tag == 0xf000 {
                pending.push(value);
            } else if kind == 10 {
                for i in 0..2 * count {
                    sw32(&mut b, value + 4 * i);
                }
            }
        }
        sw32(&mut b, at + 2 + 12 * n);
    }
    b
}

#[test]
fn malformed_or_inconsistent_data_applies_nothing() {
    let good = || tags(layout_23());
    let mut cases: Vec<(&str, Vec<u8>)> = Vec::new();
    let mut t = good();
    t[1].2[3] = 0.25; // CA knot differs from the distortion knot
    cases.push(("knot mismatch", raf(&jpeg(None), &cfa_tiff(&t))));
    let mut t = good();
    t[0].2.pop();
    cases.push(("unknown count", raf(&jpeg(None), &cfa_tiff(&t))));
    let mut t = good();
    t.remove(1);
    cases.push(("missing CA tag", raf(&jpeg(None), &cfa_tiff(&t))));
    let mut t = good();
    t[2].2[12] = 0.;
    cases.push(("zero illumination", raf(&jpeg(None), &cfa_tiff(&t))));
    let mut t = good();
    for k in [2, 3] {
        t[0].2[k] = 0.5;
        t[1].2[k - 1] = 0.5;
        t[2].2[k] = 0.5;
    }
    cases.push(("non-increasing knots", raf(&jpeg(None), &cfa_tiff(&t))));
    let mut t = good();
    t[0].2[20] = 80.;
    cases.push(("implausible distortion", raf(&jpeg(None), &cfa_tiff(&t))));
    let mut t = good();
    t[0].1 = 2; // ASCII, not a rational
    cases.push(("wrong type", raf(&jpeg(None), &cfa_tiff(&t))));
    let mut file = raf(&jpeg(None), &cfa_tiff(&good()));
    let at = file.len() - 64 - 23 * 8; // zero the last CA denominator
    file[at - 4..at].copy_from_slice(&0i32.to_le_bytes());
    cases.push(("zero denominator", file));
    let file = raf(&jpeg(None), &cfa_tiff(&good()));
    cases.push(("truncated", file[..file.len() - 300].to_vec()));
    let mut file = raf(&jpeg(None), &cfa_tiff(&good()));
    file[0] = b'X';
    cases.push(("not a RAF", file));
    let mut file = raf(&jpeg(None), &cfa_tiff(&good()));
    let at = file.len() - cfa_tiff(&good()).len();
    file[at..at + 2].copy_from_slice(b"XX");
    cases.push(("CFA section without TIFF", file));
    cases.push(("tiny", b"FUJIFILMCCD-RAW ".to_vec()));
    for (name, file) in cases {
        let got = extract_raf_lens(&mut Cursor::new(file));
        assert!(matches!(got, Ok(None)), "{name}: {got:?}");
    }
}
