use import_lrcat::smart_previews::SmartPreviewIndex;

#[test]
fn derives_only_file_uuid_paths_and_never_writes_to_bundle() {
    let temp = tempfile::tempdir().unwrap();
    let catalog = temp.path().join("Synthetic.lrcat");
    let index = SmartPreviewIndex::new(&catalog);
    let uuid = "ABCD1234-5678-90AB-CDEF-1234567890AB";
    let path = temp.path().join("Synthetic Smart Previews.lrdata/A/ABCD").join(format!("{uuid}.dng"));
    assert_eq!(index.expected_path(uuid), Some(path.clone()));
    assert_eq!(index.find(uuid), None);
    assert!(!temp.path().join("Synthetic Smart Previews.lrdata").exists());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"synthetic path lookup only").unwrap();
    assert_eq!(index.find(uuid), Some(path));
    for bad in ["", "A", "../outside", "ABCD/../../escape", "éABC", "abcd\\outside"] {
        assert_eq!(index.expected_path(bad), None);
    }
}
