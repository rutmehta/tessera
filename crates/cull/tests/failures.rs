use cull::{CullSession, Decision};
use index::{Index, NoopMetadataProvider, NoopSidecarReader};
use previews::{Codec, Jpeg};
use sidecar::Sidecar;

#[test]
fn failed_group_write_rolls_back_sidecars_index_and_history() {
    let dir = tempfile::tempdir().unwrap();
    let jpeg = Jpeg.encode(&image::RgbImage::new(18, 16)).unwrap();
    for n in 0..3 {
        std::fs::write(dir.path().join(format!("{n}.jpg")), &jpeg).unwrap();
    }
    let db = dir.path().join("index.sqlite");
    let mut index = Index::open(&db).unwrap();
    index
        .scan(dir.path(), &NoopSidecarReader, &NoopMetadataProvider)
        .unwrap();
    let mut session = CullSession::open(&index, dir.path()).unwrap();
    assert_eq!(session.groups()[0].images.len(), 3);
    let ids = session.images().to_vec();
    // Fail the last index write after earlier image writes have succeeded.
    let connection = rusqlite::Connection::open(db).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE fail_image(id TEXT);
        CREATE TRIGGER reject_selection BEFORE UPDATE ON selection
        WHEN new.image_id IN (SELECT id FROM fail_image) AND new.decision!='undecided'
        BEGIN SELECT RAISE(ABORT,'injected failure'); END;",
        )
        .unwrap();
    connection
        .execute("INSERT INTO fail_image VALUES(?)", [ids[2].to_string()])
        .unwrap();
    assert!(session.keep_best_reject_rest(0).is_err());
    assert!(!session.undo().unwrap());
    for id in &ids {
        assert_eq!(
            index.selection(*id).unwrap().unwrap().decision,
            Decision::Undecided
        );
        let paths = Sidecar::paths(index.image_info(*id).unwrap().path);
        assert!(!paths.recipe.exists());
        assert!(!paths.xmp.exists());
    }
    connection
        .execute_batch("DROP TRIGGER reject_selection")
        .unwrap();
    session.keep_best_reject_rest(0).unwrap();
    assert!(session.undo().unwrap());
}

#[test]
fn malformed_xmp_is_not_overwritten_and_does_not_advance() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("one.jpg");
    std::fs::write(&path, b"jpeg").unwrap();
    let mut index = Index::open(":memory:").unwrap();
    index
        .scan(dir.path(), &NoopSidecarReader, &NoopMetadataProvider)
        .unwrap();
    let mut session = CullSession::open(&index, dir.path()).unwrap();
    let paths = Sidecar::paths(&path);
    std::fs::write(&paths.xmp, b"<broken>").unwrap();
    assert!(session.decide(Decision::Keep).is_err());
    assert_eq!(session.position(), Some(0));
    assert!(!session.undo().unwrap());
    assert_eq!(std::fs::read(paths.xmp).unwrap(), b"<broken>");
    assert!(!paths.recipe.exists());
}
