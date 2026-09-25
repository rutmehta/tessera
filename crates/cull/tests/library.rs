use cull::{CullSession, Decision, Library, Status};
use index::{Index, NoopMetadataProvider, NoopSidecarReader, Query};
use sidecar::Sidecar;

#[test]
fn status_transitions_and_basket_share_global_history() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("one.jpg"), b"jpeg").unwrap();
    let library_path = dir.path().join("library.json");
    std::fs::write(
        &library_path,
        r#"{"albums":{},"roots":["preserve"],"future":{"v":1}}"#,
    )
    .unwrap();
    let mut index = Index::open(":memory:").unwrap();
    index
        .scan(dir.path(), &NoopSidecarReader, &NoopMetadataProvider)
        .unwrap();
    let mut session = CullSession::open(&index, dir.path()).unwrap();
    let id = session.current().unwrap();
    assert_eq!(session.derived_status(id).unwrap().status, Status::Unedited);
    assert!(session.toggle_basket().is_err());
    session.set_basket_target("Portfolio").unwrap();
    assert!(session.toggle_basket().unwrap());
    assert_eq!(session.derived_status(id).unwrap().in_album, ["Portfolio"]);
    session.decide(Decision::Keep).unwrap();
    assert!(session.undo().unwrap());
    assert_eq!(session.derived_status(id).unwrap().in_album, ["Portfolio"]);
    assert!(session.undo().unwrap());
    assert!(session.derived_status(id).unwrap().in_album.is_empty());
    assert!(
        !Library::read(&library_path)
            .unwrap()
            .albums
            .contains_key("Portfolio")
    );
    assert!(session.redo().unwrap());
    assert!(session.redo().unwrap());
    assert!(!session.toggle_basket().unwrap());
    session.undo().unwrap();
    let lib = Library::read(&library_path).unwrap();
    assert_eq!(lib.albums["Portfolio"].images, [id]);
    assert_eq!(lib.roots, vec![std::path::PathBuf::from("preserve")]);
    assert_eq!(lib.unknown["future"], serde_json::json!({"v":1}));
    let paths = Sidecar::paths(index.image_info(id).unwrap().path);
    let mut doc = Sidecar::read_recipe(&paths.recipe).unwrap();
    doc.recipe
        .edit(Default::default(), |s| s.tone.exposure = 1.0)
        .unwrap();
    Sidecar::write_recipe(&paths.recipe, &doc).unwrap();
    assert_eq!(session.derived_status(id).unwrap().status, Status::Edited);
    index.record_export(id, "one-output.jpg", false).unwrap();
    assert_eq!(session.derived_status(id).unwrap().status, Status::Exported);
    index.record_export(id, "gallery", true).unwrap();
    assert_eq!(
        session.derived_status(id).unwrap().status,
        Status::Published
    );
    let mut reopened = CullSession::open(&index, Query::default()).unwrap();
    reopened.set_library(&library_path).unwrap();
    assert_eq!(reopened.derived_status(id).unwrap().in_album, ["Portfolio"]);
}
