use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tether::{Device, Result, Session, TetherBackend};
struct Files {
    folder: PathBuf,
    names: Vec<(&'static str, bool)>,
}
impl TetherBackend for Files {
    fn devices(&mut self) -> Result<Vec<Device>> {
        Ok(vec![])
    }
    fn start(&mut self, p: &Path) -> Result<()> {
        self.folder = p.into();
        Ok(())
    }
    fn capture(&mut self) -> Result<()> {
        Err(tether::Error::Unsupported)
    }
    fn poll(&mut self) -> Result<Vec<PathBuf>> {
        let mut paths = Vec::new();
        for (name, valid) in self.names.drain(..) {
            let path = self.folder.join(name);
            if valid {
                image::RgbImage::new(32, 32).save(&path).unwrap();
            } else {
                std::fs::write(&path, b"not a jpeg").unwrap();
            }
            paths.push(path.clone());
            paths.push(path); // Duplicate backend notification must not duplicate queue.
        }
        Ok(paths)
    }
    fn stop(&mut self) -> Result<()> {
        Ok(())
    }
}
fn backend(names: Vec<(&'static str, bool)>) -> Files {
    Files {
        folder: PathBuf::new(),
        names,
    }
}
#[test]
fn collision_preserves_existing_original_and_ingests_with_suffix() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("shoot");
    std::fs::create_dir(&folder).unwrap();
    let old = folder.join("0001_A.jpg");
    image::RgbImage::new(8, 8).save(&old).unwrap();
    let bytes = std::fs::read(&old).unwrap();
    let mut s = Session::start(
        backend(vec![("A.jpg", true)]),
        &folder,
        "{sequence}_{original}.{ext}",
        &dir.path().join("db"),
        None,
    )
    .unwrap();
    s.poll().unwrap();
    let event = s.events().recv_timeout(Duration::from_secs(15)).unwrap();
    assert!(event.error.is_none(), "{:?}", event.error);
    assert!(event.path.ends_with("0001_A-1.jpg"));
    assert_eq!(std::fs::read(old).unwrap(), bytes);
    s.stop().unwrap();
    assert!(s.events().try_recv().is_err());
}
#[test]
fn bad_frame_does_not_block_later_frame_and_live_view_is_unsupported() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::start(
        backend(vec![("bad.jpg", false), ("good.jpg", true)]),
        &dir.path().join("shoot"),
        "{sequence}.{ext}",
        &dir.path().join("db"),
        None,
    )
    .unwrap();
    assert!(matches!(s.live_view(), Err(tether::Error::Unsupported)));
    assert!(matches!(s.capture(), Err(tether::Error::Unsupported)));
    s.poll().unwrap();
    s.stop().unwrap();
    let events: Vec<_> = s.events().try_iter().collect();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].sequence, 1);
    assert!(events[0].error.is_some());
    assert!(events[1].error.is_none());
    assert_eq!(events[1].sequence, 2);
    assert!(s.capture().is_err());
    assert!(s.poll().is_err());
}
#[test]
fn rejects_path_traversal_unknown_tokens_and_extension_changes() {
    let dir = tempfile::tempdir().unwrap();
    for naming in [
        "../{original}.{ext}",
        "{unknown}.{ext}",
        "{original}.png",
        "",
        "/tmp/{original}.{ext}",
    ] {
        assert!(
            Session::start(
                backend(vec![]),
                &dir.path().join("shoot"),
                naming,
                &dir.path().join("db"),
                None
            )
            .is_err()
        );
    }
    assert!(!dir.path().join("shoot").exists());
}
