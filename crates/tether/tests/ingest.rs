use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tether::{Device, Result, Session, TetherBackend};

#[test]
fn stop_closes_backend_even_if_poll_reports_disconnect() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    struct Disconnected(Arc<AtomicBool>);
    impl TetherBackend for Disconnected {
        fn devices(&mut self) -> Result<Vec<Device>> {
            Ok(vec![])
        }
        fn start(&mut self, _: &Path) -> Result<()> {
            Ok(())
        }
        fn capture(&mut self) -> Result<()> {
            Err(tether::Error::Unsupported)
        }
        fn poll(&mut self) -> Result<Vec<PathBuf>> {
            Err(tether::Error::Message("disconnected".into()))
        }
        fn stop(&mut self) -> Result<()> {
            self.0.store(true, Ordering::SeqCst);
            Ok(())
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let stopped = Arc::new(AtomicBool::new(false));
    let mut session = Session::start(
        Disconnected(stopped.clone()),
        &dir.path().join("shoot"),
        "{sequence}.{ext}",
        &dir.path().join("db"),
        None,
    )
    .unwrap();
    assert!(session.stop().is_err());
    assert!(stopped.load(Ordering::SeqCst));
    assert!(session.stop().is_ok());
}

struct Fake {
    folder: PathBuf,
}
impl TetherBackend for Fake {
    fn devices(&mut self) -> Result<Vec<Device>> {
        Ok(vec![])
    }
    fn start(&mut self, folder: &Path) -> Result<()> {
        self.folder = folder.into();
        Ok(())
    }
    fn capture(&mut self) -> Result<()> {
        Ok(())
    }
    fn poll(&mut self) -> Result<Vec<PathBuf>> {
        if self.folder.as_os_str().is_empty() {
            return Ok(vec![]);
        }
        let paths = ["B.jpg", "A.jpg"].map(|n| self.folder.join(n));
        for p in &paths {
            image::RgbImage::new(32, 32).save(p).unwrap();
        }
        self.folder = PathBuf::new();
        Ok(paths.into())
    }
    fn stop(&mut self) -> Result<()> {
        Ok(())
    }
}
#[test]
fn ingest_indexes_scores_then_publishes_in_arrival_order() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("shoot");
    let db = dir.path().join("index.sqlite");
    let mut session = Session::start(
        Fake {
            folder: PathBuf::new(),
        },
        &folder,
        "{sequence}_{original}.{ext}",
        &db,
        None,
    )
    .unwrap();
    session.poll().unwrap();
    let first = session
        .events()
        .recv_timeout(Duration::from_secs(15))
        .unwrap();
    let second = session
        .events()
        .recv_timeout(Duration::from_secs(15))
        .unwrap();
    assert_eq!(first.sequence, 1);
    assert_eq!(second.sequence, 2);
    assert!(first.path.ends_with("0001_B.jpg"));
    assert!(second.path.ends_with("0002_A.jpg"));
    assert!(first.error.is_none(), "{:?}", first.error);
    assert!(first.preview.as_ref().unwrap().is_file());
    assert!(first.face_warning.is_some()); // Missing weights must not masquerade as no faces.
    let index = index::Index::open(&db).unwrap();
    for event in [first, second] {
        let id = event.image_id.unwrap();
        assert_eq!(index.image_info(id).unwrap().path, event.path);
        assert!(
            index
                .scores(id)
                .unwrap()
                .iter()
                .any(|s| s.signal == "quality")
        );
        assert_eq!(
            index.selection(id).unwrap().unwrap().decision,
            engine_api::recipe::Decision::Undecided
        );
    }
}
