//! Legacy entry controls and a real helper-entry observation. No delegation yet.
use super::*;
use std::cell::Cell;

#[derive(Clone, Copy, Default)]
struct Observation {
    calls: usize,
    input_ptr: usize,
    input_capacity: usize,
    cancelled: bool,
}
thread_local! {
    static ACTIVE: Cell<Option<Observation>> = const { Cell::new(None) };
}
struct Scope;
impl Scope {
    fn new() -> Self {
        ACTIVE.with(|s| {
            assert!(s.get().is_none(), "nested legacy observation");
            s.set(Some(Observation::default()));
        });
        Self
    }
    fn snapshot(&self) -> Observation {
        ACTIVE.with(|s| s.get().unwrap())
    }
}
impl Drop for Scope {
    fn drop(&mut self) {
        ACTIVE.with(|s| s.set(None));
    }
}
// Called only at the actual helper entry. Thread-local scoped values retain no
// buffers, images, native handles or callbacks and cannot influence the result.
pub(super) fn record_entry(plane: &PackedPlane, cancel: &CancellationToken) {
    ACTIVE.with(|s| {
        if let Some(mut seen) = s.get() {
            seen.calls += 1;
            seen.input_ptr = plane.samples.as_ptr() as usize;
            seen.input_capacity = plane.samples.capacity();
            seen.cancelled = cancel.check().is_err();
            s.set(Some(seen));
        }
    });
}
fn packed(layout: CfaLayout) -> libraw_ffi::CfaImage {
    libraw_ffi::CfaImage {
        width: 13,
        height: 7,
        data: (0..91)
            .map(|i| [0, 32, 256, 1024, 2048, u16::MAX][i % 6])
            .collect(),
        cfa_layout: layout,
        black: [64., 32., 16., 8.],
        white: 1024,
        wb_coeffs: [1.; 4],
        color_matrix: [[0.; 3]; 3],
        cam_xyz: [[0.; 3]; 4],
        rgb_cam: [[0.; 4]; 3],
        // Arithmetic must address full sensor coordinates, not this crop.
        crop: [1, 1, 9, 5],
    }
}
fn expected(p: &libraw_ffi::CfaImage) -> Vec<u32> {
    p.data
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let x = i % p.width as usize;
            let y = i / p.width as usize;
            let c = match p.cfa_layout {
                CfaLayout::Bayer(m) => m[y % 2][x % 2],
                CfaLayout::XTrans(m) => m[y % 6][x % 6],
                CfaLayout::Unsupported => panic!("oracle requires layout"),
            } as usize;
            ((v as f32 - p.black[c]) / (p.white as f32 - p.black[c]))
                .clamp(0., 1.2)
                .to_bits()
        })
        .collect()
}
fn assert_pixels(p: libraw_ffi::CfaImage) {
    let want = expected(&p);
    let (width, height) = (p.width, p.height);
    let got = crate::linearize(p).unwrap();
    assert_eq!(
        (got.pyramid.extent.width, got.pyramid.extent.height),
        (width, height)
    );
    assert_eq!(
        got.pyramid
            .pixels
            .iter()
            .map(|v| v.to_bits())
            .collect::<Vec<_>>(),
        want
    );
}
#[test]
fn legacy_entry_delegates_once_with_owned_input_and_uncancelled_token() {
    let mut p = packed(CfaLayout::Bayer([[0, 1], [3, 2]]));
    p.data.reserve_exact(47);
    let ptr = p.data.as_ptr() as usize;
    let capacity = p.data.capacity();
    let want = expected(&p);
    let scope = Scope::new();
    let got = crate::linearize(p).unwrap();
    // Numerical equality alone is an existing control, not proof of delegation.
    assert_eq!(
        got.pyramid
            .pixels
            .iter()
            .map(|v| v.to_bits())
            .collect::<Vec<_>>(),
        want
    );
    let seen = scope.snapshot();
    assert_eq!(
        seen.calls, 1,
        "legacy entry must actually call the shared helper"
    );
    assert_eq!(seen.input_ptr, ptr);
    assert_eq!(seen.input_capacity, capacity);
    assert!(!seen.cancelled);
}
#[test]
fn legacy_bayer_xtrans_full_sensor_bits_are_preserved() {
    assert_pixels(packed(CfaLayout::Bayer([[2, 3], [1, 0]])));
    assert_pixels(packed(CfaLayout::XTrans([
        [1, 0, 1, 1, 2, 1],
        [2, 1, 2, 0, 1, 0],
        [1, 0, 1, 1, 2, 1],
        [1, 2, 1, 1, 0, 1],
        [0, 1, 0, 2, 1, 2],
        [1, 2, 1, 1, 0, 1],
    ])));
}
#[test]
fn legacy_permissive_layout_and_negative_black_controls() {
    let mut p = packed(CfaLayout::Bayer([[0; 2]; 2]));
    p.black = [-2.; 4];
    p.white = 0;
    assert_pixels(p);
    assert_pixels(packed(CfaLayout::XTrans([[2; 6]; 6])));
}
#[test]
fn legacy_invalid_input_keeps_raw_decode_error_category() {
    let mut cases = Vec::new();
    let mut p = packed(CfaLayout::Bayer([[0, 1], [3, 2]]));
    p.width = 0;
    cases.push(p);
    let mut p = packed(CfaLayout::Bayer([[0, 1], [3, 2]]));
    p.data.pop();
    cases.push(p);
    cases.push(packed(CfaLayout::Unsupported));
    cases.push(packed(CfaLayout::Bayer([[4; 2]; 2])));
    cases.push(packed(CfaLayout::XTrans([[3; 6]; 6])));
    for value in [f32::NAN, f32::INFINITY, 1024.] {
        let mut p = packed(CfaLayout::Bayer([[0, 1], [3, 2]]));
        p.black[3] = value;
        cases.push(p);
    }
    for p in cases {
        match crate::linearize(p) {
            Err(EngineError::Decode { format, .. }) => assert_eq!(format, "raw"),
            Err(e) => panic!("changed error category: {e:?}"),
            Ok(_) => panic!("invalid input admitted"),
        }
    }
}
