use compositor::{Raster, Rect, raster::Depth};
use engine_api::tile::Extent;
use filters::{
    distraction::{CpuDistractionDetector, DistractionDetector},
    remove::{BackendUsed, CpuPatchMatch, RemoveParams},
};
use std::sync::atomic::AtomicBool;

fn canvas() -> Raster {
    let mut image = Raster::new(Extent::new(64, 48), 4, Depth::F32, 0.0);
    image
        .edit_region(Rect::of_extent(image.extent()), 1, |_, _, p| {
            *p = [0.4, 0.4, 0.4, 0.8]
        })
        .unwrap();
    image
}

#[test]
fn stub_detects_wire_then_remove_consumes_mask_end_to_end() {
    let mut image = canvas();
    image
        .edit_region(Rect::new(8, 20, 56, 21), 2, |_, _, p| {
            *p = [0.02, 0.02, 0.02, 0.8]
        })
        .unwrap();
    let cancel = AtomicBool::new(false);
    let mut detector = CpuDistractionDetector;
    let masks = detector.detect(&image, &[], &cancel).unwrap();
    assert_eq!(masks.wires[20 * 64 + 30], 1.0);
    assert_eq!(masks.wires[10 * 64 + 30], 0.0);
    assert!(masks.people.iter().all(|&v| v == 0.0));
    let out = detector
        .remove(
            &image,
            &[],
            &mut CpuPatchMatch,
            &RemoveParams::default(),
            &cancel,
        )
        .unwrap();
    assert_eq!(out.backend, BackendUsed::CpuPatchMatch);
    assert!((out.result.composite.pixel(30, 20)[0] - 0.4).abs() < 0.01);
    assert_eq!(out.result.composite.pixel(30, 20)[3], 0.8);
    assert_eq!(out.result.composite.pixel(0, 0), image.pixel(0, 0));
}

#[test]
fn face_boxes_are_dilated_clipped_and_validated() {
    let image = canvas();
    let cancel = AtomicBool::new(false);
    let mut face = ml_faces::Face {
        bbox: [20.0, 10.0, 8.0, 8.0],
        landmarks5: [[0.0; 2]; 5],
        score: 0.9,
    };
    let mut detector = CpuDistractionDetector;
    let masks = detector
        .detect(&image, std::slice::from_ref(&face), &cancel)
        .unwrap();
    assert_eq!(masks.people[8 * 64 + 18], 1.0, "dilated face box");
    assert_eq!(masks.people[0], 0.0);
    assert!(masks.wires.iter().all(|&v| v == 0.0));
    face.bbox = [-2.0, -2.0, 8.0, 8.0];
    assert_eq!(
        detector
            .detect(&image, std::slice::from_ref(&face), &cancel)
            .unwrap()
            .people[0],
        1.0
    );
    face.bbox[0] = f32::NAN;
    assert!(detector.detect(&image, &[face], &cancel).is_err());
    assert!(
        detector
            .detect(&image, &[], &AtomicBool::new(true))
            .err()
            .unwrap()
            .is_cancelled()
    );
}

#[test]
fn flat_and_broad_objects_are_not_wires() {
    let mut image = canvas();
    image
        .edit_region(Rect::new(20, 10, 40, 30), 2, |_, _, p| {
            *p = [0.01, 0.01, 0.01, 1.0]
        })
        .unwrap();
    let masks = CpuDistractionDetector
        .detect(&image, &[], &AtomicBool::new(false))
        .unwrap();
    assert!(masks.wires.iter().all(|&v| v == 0.0));
}

#[test]
fn diagonal_and_vertical_structures_are_suggestions_and_invalid_masks_fail() {
    for diagonal in [false, true] {
        let mut image = canvas();
        image
            .edit_region(Rect::of_extent(image.extent()), 2, |x, y, p| {
                if (6..40).contains(&y) && x == if diagonal { y } else { 20 } {
                    *p = [0.9, 0.9, 0.9, 0.8];
                }
            })
            .unwrap();
        let masks = CpuDistractionDetector
            .detect(&image, &[], &AtomicBool::new(false))
            .unwrap();
        assert_eq!(masks.wires[20 * 64 + 20], 1.0);
        assert_eq!(masks.union(64 * 48).unwrap(), masks.wires);
    }
    let mut invalid = filters::distraction::DistractionMasks {
        wires: vec![1.0],
        people: vec![0.0; 2],
    };
    assert!(invalid.union(2).is_err());
    invalid.wires = vec![f32::NAN, 0.0];
    assert!(invalid.union(2).is_err());
}
