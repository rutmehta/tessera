#[path = "support/linear_dng.rs"]
mod support;
use engine_api::{
    color::{ChromaticAdaptation, WhitePoint, WorkingSpace},
    id::ImageId,
};
use image_core::RawImage;

#[test]
fn d65_cross_channel_calibration_is_inverted_without_row_normalization() {
    use engine_api::color::ColorMatrix3;
    let matrix = ColorMatrix3([[0.9, -0.15, 0.08], [0.05, 0.8, 0.03], [-0.1, 0.2, 0.7]]);
    let working_xyz = WorkingSpace::LinearRec2020.to_xyz();
    let working = [[0.2, 0.4, 0.8], [-0.1, 2.5, 0.3]];
    let pixels = working.map(|p| matrix.apply(working_xyz.apply(p)).map(|v| v as f32));
    let neutral = matrix.apply(working_xyz.apply([1.; 3]));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("calibrated.dng");
    std::fs::write(
        &path,
        support::calibrated_fixture(
            32,
            1,
            &pixels,
            matrix
                .0
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .try_into()
                .unwrap(),
            neutral,
        ),
    )
    .unwrap();
    let source = RawImage::open(ImageId(6), &path).unwrap();
    for (i, expected) in working.into_iter().enumerate() {
        for (c, value) in expected.into_iter().enumerate() {
            assert!(
                (f64::from(source.rgb().unwrap().pixels().planes()[c][i]) - value).abs() < 1e-4
            );
        }
    }
}

#[test]
fn linear_dng_boundary_rejects_inconsistent_dimensions_without_panicking() {
    let mut dng = raw_decode::linear_dng::read(&mut std::io::Cursor::new(support::fixture(
        32,
        1,
        &[[0.7, 1., 0.6]; 2],
    )))
    .unwrap();
    dng.pixels.pop();
    assert!(image_core::RgbSource::from_linear_dng(dng).is_err());
}

#[test]
fn cfa_dng_stays_raw_and_corrupt_linear_dng_does_not_fall_back() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw/sample.dng");
    let source = RawImage::open(ImageId(4), &path).unwrap();
    assert_eq!(source.source_kind(), "raw");
    assert!(source.rgb().is_none());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("broken.dng");
    let mut bytes = support::fixture(32, 1, &[[0.7, 1., 0.6]; 2]);
    bytes.pop();
    std::fs::write(&path, bytes).unwrap();
    assert!(
        matches!(RawImage::open(ImageId(5), &path), Err(engine_api::EngineError::Decode {format,..}) if format == "LinearRaw DNG")
    );
}

#[test]
fn linear_dng_consumes_all_orientations_once() {
    let dir = tempfile::tempdir().unwrap();
    let pixels: Vec<_> = (1..=6)
        .map(|i| [0.7 * i as f32, i as f32, 0.6 * i as f32])
        .collect();
    let mappings = [
        [0, 1, 2, 3, 4, 5],
        [1, 0, 3, 2, 5, 4],
        [5, 4, 3, 2, 1, 0],
        [4, 5, 2, 3, 0, 1],
        [0, 2, 4, 1, 3, 5],
        [4, 2, 0, 5, 3, 1],
        [5, 3, 1, 4, 2, 0],
        [1, 3, 5, 0, 2, 4],
    ];
    for (i, mapping) in mappings.iter().enumerate() {
        let path = dir.path().join(format!("oriented-{i}.dng"));
        std::fs::write(&path, support::fixture(32, (i + 1) as u16, &pixels)).unwrap();
        let source = RawImage::open(ImageId(3), &path).unwrap();
        let image = source.rgb().unwrap().pixels();
        assert_eq!(
            (image.width(), image.height()),
            if i < 4 { (2, 3) } else { (3, 2) }
        );
        assert_eq!(source.metadata().orientation, 1);
        for (actual, original) in image.planes()[1].iter().zip(mapping) {
            assert!(
                (actual - (*original + 1) as f32).abs() < 1e-4,
                "orientation {}",
                i + 1
            );
        }
    }
}

#[test]
fn integer_linear_dng_normalizes_white_level_without_byte_quantization() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("merged16.dng");
    let pixels = [[0.14, 0.2, 0.12], [0.1401, 0.2001, 0.1201]];
    std::fs::write(&path, support::fixture(16, 1, &pixels)).unwrap();
    let source = RawImage::open(ImageId(2), &path).unwrap();
    let decoded = raw_decode::linear_dng::read(&mut std::fs::File::open(&path).unwrap()).unwrap();
    for (actual, expected) in decoded.pixels.iter().flatten().zip(pixels.iter().flatten()) {
        assert!((actual - expected).abs() < 1. / 65535.);
    }
    assert_eq!(source.source_kind(), "rgb");
    let plane = &source.rgb().unwrap().pixels().planes()[1];
    assert!((plane[0] - 0.2).abs() < 5e-5);
    assert_ne!(plane[0], plane[1]);
}

#[test]
fn float_linear_dng_uses_calibrated_working_rgb_without_clipping() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("merged.dng");
    let pixels = [
        [0.7, 1., 0.6],
        [2.8, 4., 2.4],
        [-0.1, 0.3, 0.8],
        [0., 0., 0.],
    ];
    std::fs::write(&path, support::fixture(32, 1, &pixels)).unwrap();
    let source = RawImage::open(ImageId(1), &path).unwrap();
    assert_eq!(source.source_kind(), "rgb");
    assert_eq!(source.metadata().orientation, 1);
    let transform = WorkingSpace::LinearRec2020.to_xyz().inverse().unwrap()
        * ChromaticAdaptation::Bradford
            .matrix(WhitePoint::new(0.7 / 2.3, 1. / 2.3), WhitePoint::D65)
            .unwrap();
    for (i, pixel) in pixels.iter().enumerate() {
        let expected = transform.apply(pixel.map(f64::from));
        for (plane, value) in source.rgb().unwrap().pixels().planes().iter().zip(expected) {
            assert!(
                (f64::from(plane[i]) - value).abs() < 1e-5,
                "{} != {value}",
                plane[i]
            );
        }
    }
    for plane in source.rgb().unwrap().pixels().planes() {
        assert!((plane[0] - 1.).abs() < 1e-4);
        assert!(plane[1] > 3.99);
    }
}
