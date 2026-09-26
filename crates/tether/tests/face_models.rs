//! Opt-in real, hash-pinned YuNet/SFace pipeline test (downloads weights).
use std::path::{Path, PathBuf};
use tether::{Device, Result, Session, TetherBackend};
struct Camera(Option<PathBuf>);
impl TetherBackend for Camera {
    fn devices(&mut self) -> Result<Vec<Device>> {
        Ok(vec![])
    }
    fn start(&mut self, folder: &Path) -> Result<()> {
        self.0 = Some(folder.into());
        Ok(())
    }
    fn capture(&mut self) -> Result<()> {
        Ok(())
    }
    fn poll(&mut self) -> Result<Vec<PathBuf>> {
        let Some(folder) = self.0.take() else {
            return Ok(vec![]);
        };
        let path = folder.join("frame.jpg");
        image::RgbImage::new(320, 320).save(&path).unwrap();
        Ok(vec![path])
    }
    fn stop(&mut self) -> Result<()> {
        Ok(())
    }
}
#[test]
#[ignore = "downloads hash-pinned YuNet/SFace weights; run explicitly with network"]
fn real_face_inference_is_persisted_before_frame_event() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::copy(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../ml-runtime/models.toml"),
        dir.path().join("models.toml"),
    )
    .unwrap();
    let db = dir.path().join("index.sqlite");
    let mut session = Session::start(
        Camera(None),
        &dir.path().join("shoot"),
        "{sequence}.{ext}",
        &db,
        Some(dir.path().into()),
    )
    .unwrap();
    session.poll().unwrap();
    session.stop().unwrap();
    let frame = session.events().try_recv().unwrap();
    assert!(frame.error.is_none(), "{:?}", frame.error);
    assert!(frame.face_warning.is_none(), "{:?}", frame.face_warning);
    let index = index::Index::open(db).unwrap();
    let id = frame.image_id.unwrap();
    assert!(
        index
            .scores(id)
            .unwrap()
            .iter()
            .any(|s| s.signal == "faces_analyzed" && s.value == 1.0)
    );
    assert!(
        index
            .scores(id)
            .unwrap()
            .iter()
            .any(|s| s.signal == "quality")
    );
    assert!(index.faces(id).unwrap().is_empty());
}
