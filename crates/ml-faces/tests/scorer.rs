use cull::Scorer;
use index::{FaceRecord, Index, NoopMetadataProvider, NoopSidecarReader, Query};
#[test]
fn face_scorer_reads_minimum_face_focus_and_handles_absence() {
    let dir = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    std::fs::write(dir.path().join("a.jpg"), b"synthetic").unwrap();
    let mut index = Index::open(":memory:").unwrap();
    index
        .scan(dir.path(), &NoopSidecarReader, &NoopMetadataProvider)
        .unwrap();
    let id = index.search(&Query::default()).unwrap()[0];
    let face = FaceRecord {
        id: 0,
        bbox: [0., 0., 10., 10.],
        landmarks5: [[1., 1.]; 5],
        confidence: 1.,
        embedding: None,
        sharpness: 0.6,
        eyes_open: Some(0.2),
    };
    index.replace_faces(id, &[face]).unwrap();
    let scorer = ml_faces::FaceScorer::from_index(&index, &[id]).unwrap();
    assert_eq!(scorer.score(&index.image_info(id).unwrap()), 0.6);
    index.replace_faces(id, &[]).unwrap();
    let scorer = ml_faces::FaceScorer::from_index(&index, &[id]).unwrap();
    assert_eq!(scorer.score(&index.image_info(id).unwrap()), 0.0);
}
