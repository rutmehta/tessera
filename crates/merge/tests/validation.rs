use merge::{
    LinearImage,
    alignment::align,
    hdr::{BracketFrame, Exposure, HdrOptions, hdr},
};
fn frame() -> BracketFrame {
    BracketFrame {
        image: LinearImage {
            width: 32,
            height: 32,
            pixels: vec![[0.3; 3]; 1024],
            color_matrix: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
            as_shot_neutral: [1.; 3],
        },
        exposure: Exposure {
            shutter_s: 0.01,
            iso: 100.,
            aperture: 2.,
        },
    }
}
#[test]
fn invalid_hdr_inputs_return_errors() {
    let f = frame();
    let o = HdrOptions::default();
    assert!(hdr(&[], &o).is_err());
    assert!(hdr(std::slice::from_ref(&f), &o).is_err());
    assert!(
        hdr(
            &[f.clone(), f.clone()],
            &HdrOptions {
                reference: 2,
                ..o.clone()
            }
        )
        .is_err()
    );
    for bad in [0., -1., f64::NAN, f64::INFINITY] {
        let mut b = f.clone();
        b.exposure.iso = bad;
        assert!(hdr(&[f.clone(), b], &o).is_err());
    }
    let mut b = f.clone();
    b.image.color_matrix[0][0] = 2.;
    assert!(hdr(&[f.clone(), b], &o).is_err());
    let mut b = f.clone();
    b.image.pixels.pop();
    assert!(hdr(&[f.clone(), b], &o).is_err());
    let mut b = f.clone();
    b.image.pixels[0][0] = f32::NAN;
    assert!(hdr(&[f.clone(), b], &o).is_err());
    let mut b = f.clone();
    b.image.width = usize::MAX;
    assert!(hdr(&[f.clone(), b], &o).is_err());
    let out = hdr(&[f.clone(), f], &o).unwrap();
    assert!(out.deghost_mask.iter().all(|v| !*v));
    assert!(out.image.pixels.iter().all(|p| *p == [0.3; 3]));
}
#[test]
fn extremely_thin_alignment_is_rejected_without_fft_panic() {
    let mut f = frame();
    f.image.width = 8192;
    f.image.height = 16;
    f.image.pixels = (0..8192 * 16)
        .map(|i| [0.1 + (i % 101) as f32 / 200.; 3])
        .collect();
    assert!(align(&f.image, &f.image, 1.).is_err());
}
