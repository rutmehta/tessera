use std::path::{Path, PathBuf};
use tether::{Device, Result, Session, TetherBackend};

#[test]
fn stop_ingests_downloads_completed_during_camera_close() {
    struct Closing {
        folder: PathBuf,
        closed: bool,
    }
    impl TetherBackend for Closing {
        fn devices(&mut self) -> Result<Vec<Device>> {
            Ok(vec![])
        }
        fn start(&mut self, p: &Path) -> Result<()> {
            self.folder = p.into();
            Ok(())
        }
        fn capture(&mut self) -> Result<()> {
            Ok(())
        }
        fn poll(&mut self) -> Result<Vec<PathBuf>> {
            if !self.closed {
                return Ok(vec![]);
            }
            self.closed = false;
            let path = self.folder.join("late.jpg");
            image::RgbImage::new(32, 32).save(&path).unwrap();
            Ok(vec![path])
        }
        fn stop(&mut self) -> Result<()> {
            self.closed = true;
            Ok(())
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::start(
        Closing {
            folder: PathBuf::new(),
            closed: false,
        },
        &dir.path().join("shoot"),
        "{sequence}.{ext}",
        &dir.path().join("db"),
        None,
    )
    .unwrap();
    s.stop().unwrap();
    let frame = s
        .events()
        .try_recv()
        .expect("final download must be scored before stop returns");
    assert!(frame.error.is_none());
}
