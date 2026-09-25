use cull::{CullSession, Decision, Threshold};
use index::{Index, NoopMetadataProvider, NoopSidecarReader, Query, Score};

#[test]
fn sweep_is_review_only_and_checks_both_directions() {
    let dir = tempfile::tempdir().unwrap();
    for n in 0..3 {
        std::fs::write(dir.path().join(format!("{n}.jpg")), b"jpeg").unwrap();
    }
    let mut index = Index::open(":memory:").unwrap();
    index
        .scan(dir.path(), &NoopSidecarReader, &NoopMetadataProvider)
        .unwrap();
    let mut session = CullSession::open(&index, Query::default()).unwrap();
    let thresholds = vec![
        Threshold::below("focus", 0.4),
        Threshold::above("closed_eyes", 0.8),
        Threshold::above("clipping", 0.1),
    ];
    assert!(session.defect_sweep(&thresholds).unwrap().is_empty());
    let ids = session.images();
    for (id, signal, value) in [
        (ids[0], "focus", 0.2),
        (ids[0], "clipping", 0.5),
        (ids[1], "closed_eyes", 0.9),
        (ids[2], "focus", 0.4),
    ] {
        index
            .set_score(
                id,
                &Score {
                    signal: signal.into(),
                    value,
                    model: "synthetic".into(),
                },
            )
            .unwrap();
    }
    let result = session.defect_sweep(&thresholds).unwrap();
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].0, ids[0]);
    assert_eq!(result[0].1.len(), 2);
    assert_eq!(result[1].1[0].signal, "closed_eyes");
    for id in ids {
        assert_eq!(
            index.selection(*id).unwrap().unwrap().decision,
            Decision::Undecided
        );
    }
    assert!(!session.undo().unwrap());
    assert!(
        session
            .defect_sweep(&[Threshold::below("focus", f64::NAN)])
            .is_err()
    );
}
