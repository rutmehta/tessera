//! Incremental queue updates (M2-28): catalog changes reach an open session
//! in place, keeping undo/redo, the cursor and unaffected groups.
use cull::{Decision, ImageId, OwnedCullSession};
use engine_api::EngineResult;
use index::{Index, Metadata, MetadataProvider, NoopSidecarReader};
use std::path::{Path, PathBuf};

/// Capture time by file stem: 0 and 1 are a burst, 2 and 3 stand alone; 4 lands
/// inside the first burst, 5 after everything, 6 bridges the burst and 2.
struct Times;
impl MetadataProvider for Times {
    fn read(&self, path: &Path) -> EngineResult<Metadata> {
        let n: usize = path.file_stem().unwrap().to_str().unwrap().parse().unwrap();
        let times = [
            "2026-09-01T23:59:59.000",
            "2026-09-02T00:00:01.000",
            "2026-09-02T00:00:03.500",
            "2026-09-02T00:01:00.000",
            "2026-09-02T00:00:00.000",
            "2026-09-02T00:05:00.000",
            "2026-09-02T00:00:02.200",
        ];
        Ok(Metadata {
            capture_time: Some(times[n].into()),
            ..Default::default()
        })
    }
}

struct Shoot {
    _dir: tempfile::TempDir,
    folder: PathBuf,
    db: PathBuf,
    writer: Index,
}
impl Shoot {
    fn new(names: &[usize]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("shoot");
        std::fs::create_dir(&folder).unwrap();
        let db = dir.path().join("index.sqlite");
        let mut shoot = Self {
            writer: Index::open(&db).unwrap(),
            _dir: dir,
            folder,
            db,
        };
        shoot.add(names);
        shoot
    }
    fn add(&mut self, names: &[usize]) {
        for n in names {
            std::fs::write(self.folder.join(format!("{n}.jpg")), vec![0; n + 10]).unwrap();
        }
        self.writer
            .scan(&self.folder, &NoopSidecarReader, &Times)
            .unwrap();
    }
    fn remove(&mut self, n: usize) {
        std::fs::remove_file(self.folder.join(format!("{n}.jpg"))).unwrap();
        self.writer.prune_missing(false).unwrap();
    }
    fn open(&self) -> OwnedCullSession {
        let mut session =
            OwnedCullSession::open_owned(Index::open(&self.db).unwrap(), self.folder.clone())
                .unwrap();
        session.set_auto_advance(false);
        session
    }
    fn id(&self, n: usize) -> ImageId {
        self.writer
            .image_at(&self.folder.canonicalize().unwrap().join(format!("{n}.jpg")))
            .unwrap()
            .unwrap()
    }
    fn names(&self, ids: &[ImageId]) -> Vec<usize> {
        ids.iter()
            .map(|id| {
                (0..7)
                    .find(|n| {
                        self.writer
                            .image_at(&self.folder.canonicalize().unwrap().join(format!("{n}.jpg")))
                            .unwrap()
                            == Some(*id)
                    })
                    .unwrap()
            })
            .collect()
    }
    fn groups(&self, session: &OwnedCullSession) -> Vec<Vec<usize>> {
        session
            .groups()
            .iter()
            .map(|g| self.names(&g.images))
            .collect()
    }
}

#[test]
fn inserts_regroup_the_affected_burst_and_keep_history() {
    let mut shoot = Shoot::new(&[0, 1, 2, 3]);
    let mut session = shoot.open();
    assert_eq!(shoot.groups(&session), [vec![0, 1], vec![2], vec![3]]);
    // One undo step on 2, cursor on 3.
    session.set_current(shoot.id(2)).unwrap();
    session.decide(Decision::Keep).unwrap();
    session.set_current(shoot.id(3)).unwrap();
    assert!(session.can_undo());

    shoot.add(&[4, 5]);
    let change = session.sync_catalog().unwrap();
    assert_eq!(shoot.names(&change.inserted), [4, 5], "queue order");
    assert!(change.removed.is_empty() && change.regrouped && !change.reset);
    assert_eq!(shoot.names(session.images()), [0, 4, 1, 2, 3, 5]);
    assert_eq!(
        shoot.groups(&session),
        [vec![0, 4, 1], vec![2], vec![3], vec![5]]
    );
    assert_eq!(
        session.current(),
        Some(shoot.id(3)),
        "cursor follows its image"
    );
    assert!(session.can_undo(), "history survives");
    assert_eq!(
        session.change_sequence(),
        shoot.writer.change_head().unwrap()
    );
    assert!(session.sync_catalog().unwrap().is_empty(), "nothing new");

    // A frame between the burst and 2 bridges them; unrelated 3 and 5 stay.
    shoot.add(&[6]);
    session.sync_catalog().unwrap();
    assert_eq!(
        shoot.groups(&session),
        [vec![0, 4, 1, 6, 2], vec![3], vec![5]]
    );

    // Undo still addresses image 2 and restores the cursor onto it.
    assert!(session.undo().unwrap());
    assert_eq!(session.current(), Some(shoot.id(2)));
    assert_eq!(
        shoot
            .writer
            .selection(shoot.id(2))
            .unwrap()
            .unwrap()
            .decision,
        Decision::Undecided
    );
    assert!(session.redo().unwrap());

    // Incremental result equals a fresh session over the same catalog.
    let fresh = shoot.open();
    assert_eq!(fresh.images(), session.images());
    assert_eq!(fresh.groups(), session.groups());
}

#[test]
fn removals_split_bursts_and_drop_history_of_gone_images() {
    let mut shoot = Shoot::new(&[0, 1, 2, 3, 6]);
    let mut session = shoot.open();
    assert_eq!(shoot.groups(&session), [vec![0, 1, 6, 2], vec![3]]);
    session.set_current(shoot.id(6)).unwrap();
    session.decide(Decision::Reject).unwrap();
    session.set_current(shoot.id(0)).unwrap();
    session.decide(Decision::Keep).unwrap();
    session.set_current(shoot.id(6)).unwrap();

    let six = shoot.id(6);
    shoot.remove(6);
    let change = session.sync_catalog().unwrap();
    assert_eq!(change.removed, [six]);
    assert_eq!(shoot.groups(&session), [vec![0, 1], vec![2], vec![3]]);
    assert_eq!(
        session.current(),
        Some(shoot.id(2)),
        "a removed cursor image hands over to its successor"
    );
    // The step on 6 is gone; the step on 0 remains and still undoes.
    assert!(session.undo().unwrap());
    assert_eq!(
        shoot
            .writer
            .selection(shoot.id(0))
            .unwrap()
            .unwrap()
            .decision,
        Decision::Undecided
    );
    assert!(!session.can_undo());
    let fresh = shoot.open();
    assert_eq!(fresh.groups(), session.groups());
}

#[test]
fn updates_from_other_connections_are_reported_without_regrouping() {
    let shoot = Shoot::new(&[0, 1, 3]);
    let mut session = shoot.open();
    let other = shoot.open();
    drop(other);
    let mut writer =
        OwnedCullSession::open_owned(Index::open(&shoot.db).unwrap(), shoot.folder.clone())
            .unwrap();
    writer.set_current(shoot.id(3)).unwrap();
    writer.decide(Decision::Reject).unwrap();
    let change = session.sync_catalog().unwrap();
    assert!(change.inserted.is_empty() && change.removed.is_empty() && !change.regrouped);
    assert_eq!(change.updated.len(), 1);
    assert_eq!(change.updated[0].0, shoot.id(3));
    assert!(change.updated[0].1.contains(index::ChangeFields::SELECTION));
    assert_eq!(
        session.selection(shoot.id(3)).unwrap().decision,
        Decision::Reject
    );
}
