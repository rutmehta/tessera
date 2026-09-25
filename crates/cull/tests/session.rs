use cull::{CullSession, Decision};
use index::{Index, NoopMetadataProvider, NoopSidecarReader, Query};
use sidecar::Sidecar;

#[test]
fn filtered_individual_edit_rejects_same_stem_peer_without_mutation() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["image.jpg", "image.cr3"] {
        std::fs::write(dir.path().join(name), b"image").unwrap();
    }
    let mut index = Index::open(":memory:").unwrap();
    index
        .scan(dir.path(), &NoopSidecarReader, &NoopMetadataProvider)
        .unwrap();
    let ids = index.search(&Query::default()).unwrap();
    assert_eq!(ids.len(), 2);
    let mut session = CullSession::open(
        &index,
        Query {
            limit: 1,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(session.images().len(), 1);
    let before: Vec<_> = ids.iter().map(|id| index.selection(*id).unwrap()).collect();
    let error = session
        .grade(1)
        .expect_err("same-stem destinations must be rejected");
    assert!(error.to_string().contains("collision"), "{error}");
    assert!(!dir.path().join(".edits").exists());
    for (id, selection) in ids.iter().zip(before) {
        assert_eq!(index.selection(*id).unwrap(), selection);
        assert!(
            !Sidecar::paths(index.image_info(*id).unwrap().path)
                .xmp
                .exists()
        );
    }
    assert_eq!(session.position(), Some(0));
    assert!(!session.undo().unwrap());
}

#[test]
fn decision_history_restores_both_cursor_positions() {
    let (dir, index) = fixture(3);
    let mut session = CullSession::open(&index, dir.path()).unwrap();
    session.decide(Decision::Keep).unwrap();
    assert_eq!(session.position(), Some(1));
    session.set_position(2).unwrap();
    session.undo().unwrap();
    assert_eq!(session.position(), Some(0));
    session.set_auto_advance(false);
    session.redo().unwrap();
    assert_eq!(session.position(), Some(1));
    session.grade(2).unwrap();
    session.undo().unwrap();
    assert_eq!(session.position(), Some(1));
    session.redo().unwrap();
    assert_eq!(session.position(), Some(1));
    session.set_basket_target("Target").unwrap();
    session.toggle_basket().unwrap();
    session.undo().unwrap();
    session.redo().unwrap();
    assert_eq!(session.position(), Some(1));
    session
        .keep_best_reject_rest(session.current_group().unwrap())
        .unwrap();
    session.undo().unwrap();
    session.redo().unwrap();
    assert_eq!(session.position(), Some(1));
    session.decide(Decision::Reject).unwrap();
    session.undo().unwrap();
    session.redo().unwrap();
    assert_eq!(session.position(), Some(1));
    session.set_auto_advance(true);
    session.set_position(2).unwrap();
    session.decide(Decision::Reject).unwrap();
    session.undo().unwrap();
    session.redo().unwrap();
    assert_eq!(session.position(), Some(2));
}

fn fixture(count: usize) -> (tempfile::TempDir, Index) {
    let dir = tempfile::tempdir().unwrap();
    for n in 0..count {
        std::fs::write(dir.path().join(format!("{n:03}.jpg")), vec![0; n + 1]).unwrap();
    }
    let mut index = Index::open(":memory:").unwrap();
    index
        .scan(dir.path(), &NoopSidecarReader, &NoopMetadataProvider)
        .unwrap();
    (dir, index)
}

#[test]
fn keyboard_pass_persists_fifty_images_and_global_undo_redo() {
    let (dir, index) = fixture(50);
    let mut session = CullSession::open(&index, dir.path()).unwrap();
    let ids = session.images().to_vec();
    assert_eq!(ids.len(), 50);
    for (n, id) in ids.iter().enumerate() {
        assert_eq!(session.current(), Some(*id));
        session.set_auto_advance(false);
        session.grade((n % 3 + 1) as u8).unwrap();
        session.mark("client favourite").unwrap();
        session.set_auto_advance(true);
        session
            .decide(if n % 2 == 0 {
                Decision::Keep
            } else {
                Decision::Reject
            })
            .unwrap();
    }
    let last = ids[49];
    assert_eq!(
        index.selection(last).unwrap().unwrap().decision,
        Decision::Reject
    );
    assert!(session.undo().unwrap());
    assert_eq!(
        index.selection(last).unwrap().unwrap().decision,
        Decision::Keep
    );
    assert!(session.redo().unwrap());
    drop(session);
    // Clear the cache to prove open loads durable sidecars, not just SQLite.
    for id in &ids {
        index.set_selection(*id, &Default::default()).unwrap();
    }
    let session = CullSession::open(&index, Query::default()).unwrap();
    assert_eq!(session.images(), ids);
    for (n, id) in ids.iter().enumerate() {
        let info = index.image_info(*id).unwrap();
        let paths = Sidecar::paths(info.path);
        let recipe = Sidecar::read_recipe(paths.recipe).unwrap();
        let selection = index.selection(*id).unwrap().unwrap();
        assert_eq!(recipe.recipe.selection, selection);
        assert_eq!(
            Sidecar::read_xmp(paths.xmp).unwrap().selection().unwrap(),
            selection
        );
        assert_eq!(
            selection.decision,
            if n % 2 == 0 {
                Decision::Keep
            } else {
                Decision::Reject
            }
        );
        assert_eq!(selection.grade.is_some(), n % 2 == 0);
    }
}

#[test]
fn empty_invalid_and_branched_history() {
    let (_dir, index) = fixture(0);
    let mut empty = CullSession::open(&index, Query::default()).unwrap();
    assert_eq!(empty.current(), None);
    assert!(empty.decide(Decision::Keep).is_err());
    assert!(!empty.undo().unwrap());
    let (_dir, index) = fixture(2);
    let mut session = CullSession::open(&index, Query::default()).unwrap();
    assert!(session.grade(4).is_err());
    session.decide(Decision::Keep).unwrap();
    session.decide(Decision::Reject).unwrap();
    assert!(session.undo().unwrap());
    session.mark("retouch").unwrap();
    assert!(!session.redo().unwrap());
    assert!(session.undo().unwrap());
    assert!(session.undo().unwrap());
    assert!(!session.undo().unwrap());
}

#[test]
fn whole_folder_and_query_are_not_limited_to_index_default_page() {
    let (dir, index) = fixture(151);
    let session = CullSession::open(&index, dir.path()).unwrap();
    assert_eq!(session.images().len(), 151);
    let session = CullSession::open(&index, Query::default()).unwrap();
    assert_eq!(session.images().len(), 151);
    let page = CullSession::open(
        &index,
        Query {
            limit: 7,
            offset: 100,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(page.images(), &session.images()[100..107]);
}

#[test]
fn selection_query_reconciles_sidecars_before_filtering() {
    let (_dir, index) = fixture(2);
    let mut session = CullSession::open(&index, Query::default()).unwrap();
    let id = session.current().unwrap();
    session.decide(Decision::Keep).unwrap();
    drop(session);
    index.set_selection(id, &Default::default()).unwrap();
    let session = CullSession::open(
        &index,
        Query {
            decision: Some(Decision::Keep),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(session.images(), [id]);
}
