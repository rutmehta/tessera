use cull::{Album, CullSession, Library};
use index::{Index, NoopMetadataProvider, NoopSidecarReader};

fn fixture() -> (tempfile::TempDir, Index) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("one.jpg"), b"image").unwrap();
    let mut index = Index::open(":memory:").unwrap();
    index
        .scan(dir.path(), &NoopSidecarReader, &NoopMetadataProvider)
        .unwrap();
    (dir, index)
}

#[test]
fn basket_history_preserves_existing_album_and_unrelated_external_fields() {
    let (dir, index) = fixture();
    let path = dir.path().join("library.json");
    std::fs::write(
        &path,
        r#"{"albums":{"Target":{"images":[],"description":"original"}},"roots":["original"]}"#,
    )
    .unwrap();
    let mut session = CullSession::open(&index, dir.path()).unwrap();
    session.set_basket_target("Target").unwrap();
    session.toggle_basket().unwrap();
    let mut library = Library::read(&path).unwrap();
    library
        .unknown
        .insert("roots".into(), serde_json::json!(["external"]));
    library.albums.insert("Other".into(), Album::default());
    library.write(&path).unwrap();
    session.undo().unwrap();
    let library = Library::read(&path).unwrap();
    assert!(library.albums["Target"].images.is_empty());
    assert_eq!(library.albums["Target"].unknown["description"], "original");
    assert_eq!(library.unknown["roots"], serde_json::json!(["external"]));
    assert!(library.albums.contains_key("Other"));
    session.redo().unwrap();
    let library = Library::read(&path).unwrap();
    assert_eq!(
        library.albums["Target"].images,
        [session.current().unwrap()]
    );
    assert_eq!(library.albums["Target"].unknown["description"], "original");
}

#[test]
fn basket_history_rejects_external_album_edits_without_writes() {
    let (dir, index) = fixture();
    let path = dir.path().join("library.json");
    let mut session = CullSession::open(&index, dir.path()).unwrap();
    session.set_basket_target("Target").unwrap();
    session.toggle_basket().unwrap();
    let original = Library::read(&path).unwrap();
    let mut external = original.clone();
    external
        .albums
        .get_mut("Target")
        .unwrap()
        .unknown
        .insert("note".into(), serde_json::json!("outside edit"));
    external.write(&path).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(session.undo().is_err());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    original.write(&path).unwrap();
    session.undo().unwrap();
    assert!(!Library::read(&path).unwrap().albums.contains_key("Target"));
    let mut external = Library::read(&path).unwrap();
    external.albums.insert("Target".into(), Album::default());
    external.write(&path).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(session.redo().is_err());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}
