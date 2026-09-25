use image::{Rgb, RgbImage};
use ml_faces::{Face, FaceChip, face_signals, face_strip};

#[test]
fn catalog_strips_and_person_filters_use_identity_not_detection_ordinals() {
    let dir = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    for n in 0..3 {
        std::fs::write(dir.path().join(format!("{n}.jpg")), format!("image {n}")).unwrap();
    }
    let mut index = index::Index::open(":memory:").unwrap();
    index
        .scan(
            dir.path(),
            &index::NoopSidecarReader,
            &index::NoopMetadataProvider,
        )
        .unwrap();
    let ids = index.search(&index::Query::default()).unwrap();
    for (id, eye) in ids.iter().zip([Some(0.1), Some(0.9), None]) {
        index
            .replace_faces(
                *id,
                &[index::FaceRecord {
                    id: 0,
                    bbox: [2., 3., 10., 12.],
                    landmarks5: [[4., 5.]; 5],
                    confidence: 1.,
                    embedding: None,
                    sharpness: 0.7,
                    eyes_open: eye,
                }],
            )
            .unwrap();
    }
    let identities = |image, _: &index::FaceRecord| {
        Some(if image == ids[1] { "other" } else { "bride" }.to_string())
    };
    let chips = ml_faces::face_strip_from_index(&index, ids[0], (20, 20), identities).unwrap();
    assert_eq!(chips[0].person_id.as_deref(), Some("bride"));
    assert_eq!(chips[0].focus_score, 0.7);
    assert_eq!(chips[0].crop_rect, [2, 3, 10, 12]);
    let result = ml_faces::frames_with_person_eyes_closed(
        &index,
        &[ids[0], ids[0], ids[1], ids[2]],
        "bride",
        0.3,
        identities,
    )
    .unwrap();
    assert_eq!(result, vec![ids[0]]);
    assert!(
        ml_faces::frames_with_person_eyes_closed(&index, &ids, "other", 0.3, identities)
            .unwrap()
            .is_empty()
    );
    assert!(
        ml_faces::frames_with_person_eyes_closed(&index, &ids, "bride", f64::NAN, identities)
            .is_err()
    );
    assert!(ml_faces::face_strip_from_index(&index, ids[0], (0, 20), identities).is_err());
}

// Explicit supplied detection, not a claim that YuNet detects this drawing.
fn fixture() -> (RgbImage, Face) {
    let image = RgbImage::from_fn(112, 112, |x, y| {
        let (x, y) = (x as i32, y as i32);
        let eye = [(38, 52), (74, 52)]
            .iter()
            .any(|&(ex, ey)| (x - ex).pow(2) + (y - ey).pow(2) < 16);
        let mouth = (40..73).contains(&x) && (90..94).contains(&y);
        if eye || mouth {
            Rgb([30; 3])
        } else if (x - 56).pow(2) * 2 + (y - 65).pow(2) < 3500 {
            Rgb([210, 170, 140])
        } else {
            Rgb([60, 100, 140])
        }
    });
    let face = Face {
        bbox: [8., 5., 96., 106.],
        landmarks5: [[38., 52.], [74., 52.], [56., 72.], [42., 92.], [70., 92.]],
        score: 1.,
    };
    (image, face)
}

#[test]
fn crops_are_clipped_and_rounded_outward_in_input_order() {
    let (image, mut face) = fixture();
    face.bbox = [-5.5, -2.25, 25.75, 20.5];
    let mut other = face.clone();
    other.bbox = [100.25, 101.75, 30., 40.];
    let chips = face_strip(&image, &[face, other]).unwrap();
    assert_eq!(
        chips.iter().map(|c| c.crop_rect).collect::<Vec<_>>(),
        vec![[0, 0, 21, 19], [100, 101, 12, 11]]
    );
    for chip in chips {
        let [x, y, w, h] = chip.crop_rect;
        let crop = image::imageops::crop_imm(&image, x, y, w, h).to_image();
        assert_eq!(
            chip.focus_score,
            ml_quality::analyze(&crop).unwrap().sharpness
        );
    }
}

#[test]
fn empty_detections_are_valid_but_empty_images_are_not() {
    let (image, face) = fixture();
    assert!(face_strip(&image, &[]).unwrap().is_empty());
    for image in [
        RgbImage::new(0, 0),
        RgbImage::new(0, 10),
        RgbImage::new(10, 0),
    ] {
        assert!(face_strip(&image, &[]).is_err());
        assert!(face_strip(&image, std::slice::from_ref(&face)).is_err());
    }
}

#[test]
fn invalid_detections_fail_the_whole_strip() {
    let (image, face) = fixture();
    for bbox in [
        [0., 0., 0., 2.],
        [0., 0., 2., -1.],
        [f32::NAN, 0., 2., 2.],
        [0., 0., f32::INFINITY, 2.],
        [f32::MAX, 0., f32::MAX, 2.],
        [112., 0., 2., 2.],
        [-5., 0., 2., 2.],
        [0., 112., 2., 2.],
        [0., -5., 2., 2.],
    ] {
        let mut invalid = face.clone();
        invalid.bbox = bbox;
        assert!(
            face_strip(&image, &[face.clone(), invalid]).is_err(),
            "{bbox:?}"
        );
    }
    let mut invalid = face.clone();
    invalid.landmarks5[0][0] = f32::NAN;
    assert!(face_strip(&image, &[invalid]).is_err());
    for score in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
        let mut invalid = face.clone();
        invalid.score = score;
        assert!(face_strip(&image, &[invalid]).is_err());
    }
}

#[test]
fn degenerate_landmarks_preserve_unknown_eyes_and_tiny_crops_are_safe() {
    let (image, mut face) = fixture();
    face.landmarks5 = [[0.; 2]; 5];
    face.bbox = [111.5, 111.5, 1., 1.];
    let chips = face_strip(&image, &[face]).unwrap();
    assert_eq!(chips[0].crop_rect, [111, 111, 1, 1]);
    assert_eq!(chips[0].eyes_open, None);
    assert_eq!(chips[0].person_id, None);
    assert!(chips[0].focus_score.is_finite());
}

#[test]
fn supplied_detection_produces_ui_chip_without_models() {
    let (image, face) = fixture();
    let signals = face_signals(&image, &face).unwrap();
    let chips: Vec<FaceChip> = face_strip(&image, &[face]).unwrap();
    assert_eq!(chips.len(), 1);
    let chip = &chips[0];
    assert_eq!(chip.crop_rect, [8, 5, 96, 106]);
    assert_eq!(chip.focus_score, signals.sharpness);
    assert!(chip.focus_score > 0. && chip.focus_score <= 1.);
    assert_eq!(chip.eyes_open, signals.eyes_open);
    assert!(chip.eyes_open.is_some());
    assert_eq!(chip.person_id, None);
}
