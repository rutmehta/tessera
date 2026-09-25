//! Surface used by long-lived hosts (the UniFFI bridge): owned sessions, batch
//! edits, reverse in-group navigation, batch basket and safe album removal.
use cull::{CullSession, Decision, Library};
use engine_api::EngineResult;
use index::{Index, Metadata, MetadataProvider, NoopMetadataProvider, NoopSidecarReader, Query};
use std::path::Path;

fn fixture(count: usize) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    for n in 0..count {
        std::fs::write(dir.path().join(format!("{n:03}.jpg")), vec![0; n + 1]).unwrap();
    }
    let db = dir.path().join("index.sqlite");
    let mut index = Index::open(&db).unwrap();
    index
        .scan(dir.path(), &NoopSidecarReader, &NoopMetadataProvider)
        .unwrap();
    (dir, db)
}

#[test]
fn owned_session_batches_are_single_undo_steps_and_report_touched_images() {
    let (dir, db) = fixture(4);
    let mut session = CullSession::open_owned(Index::open(&db).unwrap(), dir.path()).unwrap();
    let ids = session.images().to_vec();
    assert!(!session.can_undo());
    session
        .decide_images(&[ids[1], ids[3], ids[1]], Decision::Reject)
        .unwrap();
    assert_eq!(session.position(), Some(0), "batches never move the cursor");
    assert_eq!(session.undo_images(), [ids[1], ids[3]]);
    session.grade_images(&ids[..2], 3).unwrap();
    session.mark_images(&ids[2..], "retouch").unwrap();
    // A second connection sees the durable writes.
    let other = Index::open(&db).unwrap();
    assert_eq!(
        other.selection(ids[3]).unwrap().unwrap().decision,
        Decision::Reject
    );
    assert_eq!(
        other.selection(ids[1]).unwrap().unwrap().decision,
        Decision::Keep,
        "grade implies keep"
    );
    assert_eq!(
        session.selection(ids[2]).unwrap().mark.unwrap().0,
        "retouch"
    );
    assert!(session.undo().unwrap());
    assert!(session.selection(ids[2]).unwrap().mark.is_none());
    assert!(session.undo().unwrap());
    assert_eq!(
        session.selection(ids[1]).unwrap().decision,
        Decision::Reject
    );
    assert!(session.undo().unwrap());
    assert_eq!(
        session.selection(ids[1]).unwrap().decision,
        Decision::Undecided
    );
    assert!(!session.can_undo());
    assert!(session.can_redo());
    assert_eq!(session.redo_images(), [ids[1], ids[3]]);
    let foreign = engine_api::id::ImageId(42);
    assert!(session.decide_images(&[foreign], Decision::Keep).is_err());
    assert!(session.set_current(foreign).is_err());
    session.set_current(ids[2]).unwrap();
    assert_eq!(session.position(), Some(2));
}

#[test]
fn batch_basket_and_album_removal_are_undoable_and_never_touch_files() {
    let (dir, db) = fixture(3);
    let index = Index::open(&db).unwrap();
    let mut session = CullSession::open(&index, dir.path()).unwrap();
    let ids = session.images().to_vec();
    assert!(session.set_basket(&ids, true).is_err(), "needs a target");
    session.set_basket_target("Portfolio").unwrap();
    session.set_basket(&ids, true).unwrap();
    // Adding members that are already present records no history.
    session.set_basket(&ids[..1], true).unwrap();
    assert_eq!(session.undo_images(), ids);
    let path = dir.path().canonicalize().unwrap().join("library.json");
    assert_eq!(
        Library::read(&path).unwrap().albums["Portfolio"].images,
        ids
    );
    assert!(session.remove_from_album("Missing", &ids).is_err());
    session.remove_from_album("Portfolio", &ids[1..]).unwrap();
    assert_eq!(session.undo_images(), &ids[1..]);
    assert_eq!(
        Library::read(&path).unwrap().albums["Portfolio"].images,
        [ids[0]]
    );
    for id in &ids {
        assert!(index.image_info(*id).unwrap().path.exists());
    }
    let statuses = session.derived_statuses(&ids).unwrap();
    assert_eq!(statuses[0].in_album, ["Portfolio"]);
    assert!(statuses[1].in_album.is_empty());
    assert!(session.undo().unwrap());
    assert_eq!(
        Library::read(&path).unwrap().albums["Portfolio"].images,
        ids
    );
    assert!(session.undo().unwrap());
    assert!(
        !Library::read(&path)
            .unwrap()
            .albums
            .contains_key("Portfolio")
    );
    assert_eq!(session.library().unwrap().albums.len(), 0);
}

struct UnixSeconds;
impl MetadataProvider for UnixSeconds {
    fn read(&self, path: &Path) -> EngineResult<Metadata> {
        // RAW scanners store Unix seconds; 0 and 1 form a burst, 2 stands alone.
        let n: usize = path.file_stem().unwrap().to_str().unwrap().parse().unwrap();
        Ok(Metadata {
            capture_time: Some((1_700_000_000 + [0, 1, 60][n]).to_string()),
            ..Default::default()
        })
    }
}

#[test]
fn unix_second_capture_times_form_bursts_and_prev_in_group_stops_at_start() {
    let dir = tempfile::tempdir().unwrap();
    for n in 0..3 {
        std::fs::write(dir.path().join(format!("{n}.cr3")), b"x").unwrap();
    }
    let mut index = Index::open(":memory:").unwrap();
    index
        .scan(dir.path(), &NoopSidecarReader, &UnixSeconds)
        .unwrap();
    let mut session = CullSession::open(&index, Query::default()).unwrap();
    let sizes: Vec<_> = session.groups().iter().map(|g| g.images.len()).collect();
    assert_eq!(sizes, [2, 1]);
    let ids = session.images().to_vec();
    assert_eq!(session.group_of(ids[1]), Some(0));
    session.prev_in_group();
    assert_eq!(session.position(), Some(0));
    session.next_in_group();
    assert_eq!(session.position(), Some(1));
    session.prev_in_group();
    assert_eq!(session.position(), Some(0));
    session.next_group();
    session.prev_in_group();
    assert_eq!(session.position(), Some(2));
}
