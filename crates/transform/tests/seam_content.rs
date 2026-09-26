use transform::{
    Image,
    seam::{ContentAwareScale, apply},
};

fn image(w: usize, h: usize, row: &[f32]) -> Image {
    let values: Vec<_> = (0..h).flat_map(|_| row.iter().copied()).collect();
    Image::new(
        w,
        h,
        [values.clone(), values.clone(), values, vec![1.0; w * h]],
    )
    .unwrap()
}
fn params(w: usize, h: usize) -> ContentAwareScale {
    ContentAwareScale {
        target_width: w,
        target_height: h,
        amount: 1.0,
        protect: None,
    }
}

#[test]
fn seam_shrink_avoids_protected_columns() {
    let input = image(3, 3, &[0.1, 0.4, 0.9]);
    let mut p = params(2, 3);
    p.protect = Some([1.0, 0.0, 1.0].repeat(3));
    let output = apply(&input, &p).unwrap();
    assert_eq!((output.width, output.height), (2, 3));
    assert_eq!(output.planes[0], [0.1, 0.9].repeat(3));
    assert_eq!(output.planes, apply(&input, &p).unwrap().planes);
}

#[test]
fn seam_height_shrink_tracks_mask() {
    let input = Image::new(
        2,
        3,
        [
            vec![0.1, 0.1, 0.4, 0.4, 0.9, 0.9],
            vec![0.0; 6],
            vec![0.0; 6],
            vec![1.0; 6],
        ],
    )
    .unwrap();
    let mut p = params(2, 2);
    p.protect = Some(vec![1.0, 1.0, 0.0, 0.0, 1.0, 1.0]);
    let output = apply(&input, &p).unwrap();
    assert_eq!(output.planes[0], vec![0.1, 0.1, 0.9, 0.9]);
}

#[test]
fn seam_enlarges_both_axes_and_handles_single_pixel() {
    let input = image(3, 2, &[0.2, 0.4, 0.6]);
    let output = apply(&input, &params(5, 4)).unwrap();
    assert_eq!((output.width, output.height), (5, 4));
    assert!(output.planes[0].iter().all(|v| (0.2..=0.6).contains(v)));
    assert!(output.planes[3].iter().all(|&v| v == 1.0));
    let tiny = image(1, 1, &[0.25]);
    let expanded = apply(&tiny, &params(5, 7)).unwrap();
    assert_eq!(expanded.planes[0], vec![0.25; 35]);
    assert_eq!((expanded.width, expanded.height), (5, 7));
}

#[test]
fn seam_amount_zero_is_bilinear_and_partial_amount_keeps_target() {
    let input = image(2, 1, &[0.0, 1.0]);
    let mut p = params(3, 1);
    p.amount = 0.0;
    assert_eq!(apply(&input, &p).unwrap().planes[0], vec![0.0, 0.5, 1.0]);
    p.amount = 0.5;
    let out = apply(&input, &p).unwrap();
    assert_eq!((out.width, out.height), (3, 1));
}

#[test]
fn seam_rejects_invalid_parameters_and_publicly_mutated_images() {
    let input = image(2, 2, &[0.0, 1.0]);
    for amount in [-0.1, 1.1, f32::NAN, f32::INFINITY] {
        let mut p = params(1, 2);
        p.amount = amount;
        assert!(apply(&input, &p).is_err());
    }
    for (w, h) in [(0, 2), (2, 0), (usize::MAX, 2)] {
        assert!(apply(&input, &params(w, h)).is_err());
    }
    for mask in [vec![0.0], vec![f32::NAN; 4], vec![-1.0; 4], vec![1.1; 4]] {
        let mut p = params(1, 2);
        p.protect = Some(mask);
        assert!(apply(&input, &p).is_err());
    }
    let mut broken = input.clone();
    broken.planes[0].clear();
    assert!(apply(&broken, &params(1, 2)).is_err());
    broken = input.clone();
    broken.planes[1][0] = f32::NAN;
    assert!(apply(&broken, &params(1, 2)).is_err());
}

#[test]
fn seam_skin_hook_matches_explicit_mask_and_rejects_bad_scores() {
    use transform::seam::apply_with_skin_protection;
    let input = image(3, 2, &[0.1, 0.4, 0.9]);
    let p = params(2, 2);
    let mut calls = 0;
    let out = apply_with_skin_protection(&input, &p, |x, _y, rgba| {
        calls += 1;
        assert_eq!(rgba[3], 1.0);
        if x == 1 { 0.0 } else { 1.0 }
    })
    .unwrap();
    assert_eq!(calls, 6);
    assert_eq!(out.planes[0], [0.1, 0.9].repeat(2));
    assert!(apply_with_skin_protection(&input, &p, |_, _, _| f32::NAN).is_err());
}

#[test]
fn seam_partial_amount_rounds_seam_count_symmetrically() {
    let input = image(3, 1, &[0.1, 0.4, 0.9]);
    let mut p = params(2, 1);
    p.amount = 0.5;
    p.protect = Some(vec![1.0, 0.0, 1.0]);
    assert_eq!(apply(&input, &p).unwrap().planes[0], vec![0.1, 0.9]);
}

#[test]
fn seam_identity_is_exact() {
    let input = image(3, 2, &[0.0, 0.5, 1.0]);
    let output = apply(&input, &params(3, 2)).unwrap();
    assert_eq!(output.planes, input.planes);
    assert_eq!((output.width, output.height), (3, 2));
}
