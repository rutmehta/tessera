use engine_api::id::ImageId;
use library::{Album, Library};
#[test]
fn document_roundtrip_and_safe_delete() {
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("original.jpg");
    std::fs::write(&original, b"untouched").unwrap();
    let mut lib = Library::default();
    lib.roots.push(dir.path().into());
    lib.albums.insert(
        "Selects".into(),
        Album {
            id: 42,
            images: vec![ImageId(2), ImageId(1)],
            ..Default::default()
        },
    );
    lib.people.push("Anna".into());
    lib.marks_preset.insert("portfolio".into(), "purple".into());
    lib.publish_state
        .insert("future".into(), serde_json::json!({"v":1}));
    lib.unknown
        .insert("future_field".into(), serde_json::json!(true));
    let path = dir.path().join("library.json");
    lib.write(&path).unwrap();
    assert_eq!(Library::read(&path).unwrap(), lib);
    lib.remove_from_album(42, &[ImageId(2)]).unwrap();
    assert_eq!(lib.albums["Selects"].images, vec![ImageId(1)]);
    assert!(lib.in_album(ImageId(2)).is_empty());
    assert_eq!(lib.in_album(ImageId(1)), vec!["Selects"]);
    lib.delete_album(42).unwrap();
    assert!(lib.albums.is_empty());
    assert_eq!(std::fs::read(original).unwrap(), b"untouched");
}
