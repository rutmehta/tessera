use cull::{CullSession, Decision, Threshold};
use image::{Rgb, RgbImage};
use index::{Index, NoopMetadataProvider, NoopSidecarReader, Query};

#[test]
fn computed_signals_drive_default_best_and_defect_review() {
    let dir = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    let sharp = RgbImage::from_fn(128, 128, |x, y| {
        Rgb([if (x / 8 + y / 8) % 2 == 0 { 40 } else { 210 }; 3])
    });
    let blur = image::imageops::blur(&sharp, 2.);
    for (name, img) in [("a.jpg", &sharp), ("b.jpg", &blur)] {
        image::DynamicImage::ImageRgb8(img.clone())
            .save(dir.path().join(name))
            .unwrap();
    }
    // Ensure the worse image is larger, to catch LargestFile fallback.
    use std::io::Write;
    std::fs::OpenOptions::new()
        .append(true)
        .open(dir.path().join("b.jpg"))
        .unwrap()
        .write_all(&[0; 100000])
        .unwrap();
    let mut index = Index::open(dir.path().join("index.sqlite")).unwrap();
    index
        .scan(dir.path(), &NoopSidecarReader, &NoopMetadataProvider)
        .unwrap();
    let ids = index.search(&Query::default()).unwrap();
    let a = ids
        .iter()
        .copied()
        .find(|id| index.image_info(*id).unwrap().path.ends_with("a.jpg"))
        .unwrap();
    let b = ids.iter().copied().find(|id| *id != a).unwrap();
    let high = ml_quality::analyze_and_store(&index, a, &sharp)
        .unwrap()
        .sharpness;
    let low = ml_quality::analyze_and_store(&index, b, &blur)
        .unwrap()
        .sharpness;
    let mut session = CullSession::open(&index, Query::default()).unwrap();
    assert_eq!(session.groups().len(), 1);
    let defects = session
        .defect_sweep(&[Threshold::below("sharpness", (low + high) / 2.)])
        .unwrap();
    assert_eq!(defects.len(), 1);
    assert_eq!(defects[0].0, b);
    assert_eq!(
        index.selection(b).unwrap().unwrap().decision,
        Decision::Undecided
    );
    assert_eq!(session.keep_best_reject_rest(0).unwrap(), a);
    assert_eq!(
        index.selection(b).unwrap().unwrap().decision,
        Decision::Reject
    );
    session.undo().unwrap();
    assert_eq!(
        index.selection(b).unwrap().unwrap().decision,
        Decision::Undecided
    );
    session.set_scorer(Box::new(
        ml_quality::QualityScorer::from_index(&index, &ids).unwrap(),
    ));
    assert_eq!(session.best_in_group(0).unwrap(), a);
}
