#[test]
fn catalog_orientation_codes_match_exif_including_reflections() {
    for (code, expected) in [
        ("AB", 1),
        ("BA", 2),
        ("CD", 3),
        ("DC", 4),
        ("CB", 5),
        ("BC", 6),
        ("AD", 7),
        ("DA", 8),
    ] {
        assert_eq!(import_lrcat::orientation::exif(code), Some(expected));
    }
    for code in ["", "AA", "ab", "private-invalid"] {
        assert_eq!(import_lrcat::orientation::exif(code), None);
    }
}

#[test]
fn catalog_orientation_is_saved_without_an_extra_import_history_entry() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = import_lrcat::fixture::write(dir.path()).unwrap();
    let db = rusqlite::Connection::open(&fixture.catalog).unwrap();
    db.execute("UPDATE Adobe_images SET orientation='BC'", [])
        .unwrap();
    drop(db);
    let imported = import_lrcat::import(&fixture.catalog).unwrap();
    for image in imported.images {
        assert_eq!(
            image
                .recipe
                .unknown
                .get("lightroom_orientation")
                .and_then(serde_json::Value::as_u64),
            Some(6)
        );
        assert!(image.recipe.history.entries.len() <= 1);
    }
}
