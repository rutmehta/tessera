use cull::{CullSession, Decision};
use index::{Index, NoopMetadataProvider, NoopSidecarReader, Query};
use library::SavedSearch;

#[test]
fn saved_search_reconciles_sidecars_before_boolean_filters() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.jpg"), b"jpeg").unwrap();
    let mut index = Index::open(":memory:").unwrap();
    index
        .scan(dir.path(), &NoopSidecarReader, &NoopMetadataProvider)
        .unwrap();
    let mut session = CullSession::open(&index, dir.path()).unwrap();
    let id = session.current().unwrap();
    session.decide(Decision::Keep).unwrap();
    // Simulate a stale, rebuilt index while the sidecar remains authoritative.
    index.set_selection(id, &Default::default()).unwrap();
    let query = "decision:keep"
        .parse::<SavedSearch>()
        .unwrap()
        .compile()
        .unwrap();
    let session = CullSession::open(&index, query).unwrap();
    assert_eq!(session.images(), &[id]);
    let query = "NOT decision:keep"
        .parse::<SavedSearch>()
        .unwrap()
        .compile()
        .unwrap();
    assert!(
        CullSession::open(&index, query)
            .unwrap()
            .images()
            .is_empty()
    );
    assert_eq!(index.search(&Query::default()).unwrap(), vec![id]);
}
