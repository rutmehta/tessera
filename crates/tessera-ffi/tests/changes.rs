//! Library change feed over the bridge (M2-28): ordering and coalescing of
//! `changes_since`, `LibraryChanged` events (own writes and other writers),
//! and `CullSession::sync_changes` keeping history while frames arrive.
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tessera_ffi::{
    Decision, Engine, EngineEvent, EngineEventListener, ImageQuery, LibraryChangeKind, Selection,
};

#[derive(Default)]
struct Events(Mutex<Vec<u64>>);
impl EngineEventListener for Events {
    fn on_event(&self, event: EngineEvent) {
        if let EngineEvent::LibraryChanged { sequence } = event {
            self.0.lock().unwrap().push(sequence);
        }
    }
}
impl Events {
    fn wait_for(&self, sequence: u64) -> bool {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if self.0.lock().unwrap().iter().any(|s| *s >= sequence) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }
}

fn photo(path: &Path, shade: u8) {
    image::RgbImage::from_fn(48, 32, |x, y| {
        image::Rgb([shade.wrapping_add((x * 5) as u8), (y * 7) as u8, shade])
    })
    .save(path)
    .unwrap();
}

#[test]
fn feed_is_ordered_coalesced_and_announced() {
    let dir = tempfile::tempdir().unwrap();
    let photos = dir.path().join("photos");
    std::fs::create_dir(&photos).unwrap();
    photo(&photos.join("a.jpg"), 10);
    photo(&photos.join("b.jpg"), 90);
    let support = dir.path().join("support").to_string_lossy().into_owned();
    let engine = Engine::open(support.clone()).unwrap();
    let events = Arc::new(Events::default());
    engine.set_event_listener(Some(events.clone()));
    assert_eq!(engine.change_sequence().unwrap(), 0);

    engine
        .index_folder(photos.to_string_lossy().into_owned())
        .unwrap();
    let head = engine.change_sequence().unwrap();
    assert!(head > 0);
    assert_eq!(
        events.0.lock().unwrap().last(),
        Some(&head),
        "the scan announces its own writes at once"
    );
    let all = engine.changes_since(0).unwrap();
    assert_eq!((all.from, all.sequence, all.reset), (0, head, false));
    assert_eq!(all.changes.len(), 2);
    assert!(
        all.changes
            .iter()
            .all(|c| c.kind == LibraryChangeKind::Added)
    );
    assert!(all.changes[0].sequence < all.changes[1].sequence);

    // Two selection writes to one image coalesce into one update.
    let rows = engine.list_images(ImageQuery::default()).unwrap();
    for decision in [Decision::Keep, Decision::Reject] {
        engine
            .set_selection(
                rows[0].id.clone(),
                Selection {
                    decision,
                    grade: None,
                    mark: None,
                },
            )
            .unwrap();
    }
    let after = engine.changes_since(head).unwrap();
    assert_eq!(after.changes.len(), 1);
    assert_eq!(after.changes[0].image_id, rows[0].id);
    assert_eq!(after.changes[0].kind, LibraryChangeKind::Updated);
    assert!(after.changes[0].fields.selection && after.changes[0].fields.recipe);
    assert!(engine.changes_since(after.sequence + 5).unwrap().reset);

    // Another engine (a catalog import, the MCP server) writes the same catalog:
    // the watcher announces it without any command on this engine.
    photo(&photos.join("c.jpg"), 170);
    let other = Engine::open(support).unwrap();
    other
        .index_folder(photos.to_string_lossy().into_owned())
        .unwrap();
    let target = other.change_sequence().unwrap();
    assert!(events.wait_for(target), "watcher saw the other writer");
    let added = engine.changes_since(after.sequence).unwrap();
    assert_eq!(added.changes.len(), 1);
    assert_eq!(added.changes[0].kind, LibraryChangeKind::Added);

    // Delete from disk: only the images whose files are gone leave the catalog.
    std::fs::remove_file(&rows[1].path).unwrap();
    let ids = vec![rows[0].id.clone(), rows[1].id.clone()];
    assert_eq!(engine.forget_missing(ids).unwrap(), 1);
    let gone = engine.changes_since(added.sequence).unwrap();
    assert_eq!(gone.changes.len(), 1);
    assert_eq!(
        (gone.changes[0].image_id.as_str(), gone.changes[0].kind),
        (rows[1].id.as_str(), LibraryChangeKind::Removed)
    );
    assert_eq!(engine.list_images(ImageQuery::default()).unwrap().len(), 2);
}

#[test]
fn tethered_frames_join_the_open_session_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let shoot = dir.path().join("shoot");
    std::fs::create_dir(&shoot).unwrap();
    photo(&shoot.join("existing.jpg"), 40);
    let card = dir.path().join("card");
    std::fs::create_dir(&card).unwrap();
    for n in 0..3 {
        photo(&card.join(format!("F{n}.jpg")), 60 + n * 50);
    }
    let engine = Engine::open(dir.path().join("app").to_string_lossy().into_owned()).unwrap();
    let handle = engine
        .index_folder(shoot.to_string_lossy().into_owned())
        .unwrap();
    let session = engine.open_cull_session(handle.path.clone()).unwrap();
    session.groups().unwrap();
    let existing = session.images().unwrap()[0].id.clone();
    session.set_current(existing.clone()).unwrap();
    session.decide(Decision::Keep).unwrap();
    assert!(session.can_undo().unwrap());

    engine
        .tether_use_fake(Some(card.to_string_lossy().into_owned()), 0)
        .unwrap();
    engine
        .tether_start(
            shoot.join("Tether").to_string_lossy().into_owned(),
            "{sequence}_{original}.{ext}".into(),
        )
        .unwrap();
    let mut added = Vec::new();
    for _ in 0..3 {
        engine.tether_capture().unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut frames = Vec::new();
        while frames.is_empty() && Instant::now() < deadline {
            frames = engine.tether_poll().unwrap();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(frames.len(), 1);
        let delta = session.sync_changes().unwrap();
        assert!(!delta.reset);
        assert!(delta.removed.is_empty());
        added.extend(delta.added.iter().map(|i| i.id.clone()));
        assert_eq!(delta.current.as_deref(), Some(existing.as_str()));
        assert!(delta.groups.is_some() || delta.added.is_empty());
    }
    engine.tether_stop().unwrap();
    engine.tether_use_fake(None, 0).unwrap();
    assert_eq!(added.len(), 3, "every frame joined the queue");
    let images = session.images().unwrap();
    assert_eq!(images.len(), 4);
    assert!(
        images.iter().any(|i| i.path.ends_with("0002_F1.jpg")),
        "{:?}",
        images.iter().map(|i| &i.path).collect::<Vec<_>>()
    );
    // The review history from before the frames still undoes.
    assert!(session.can_undo().unwrap());
    let undone = session.undo().unwrap().unwrap();
    assert_eq!(undone.current.as_deref(), Some(existing.as_str()));
    assert_eq!(
        session.selection(existing).unwrap().decision,
        Decision::Undecided
    );
    assert!(session.change_sequence().unwrap() <= engine.change_sequence().unwrap());
    // Same queue and groups as a fresh session.
    let fresh = engine.open_cull_session(handle.path).unwrap();
    assert_eq!(
        fresh
            .images()
            .unwrap()
            .iter()
            .map(|i| &i.id)
            .collect::<Vec<_>>(),
        images.iter().map(|i| &i.id).collect::<Vec<_>>()
    );
    assert_eq!(
        fresh
            .groups()
            .unwrap()
            .iter()
            .map(|g| &g.images)
            .collect::<Vec<_>>(),
        session
            .groups()
            .unwrap()
            .iter()
            .map(|g| &g.images)
            .collect::<Vec<_>>()
    );
}
