use engine_api::{
    error::EngineResult,
    recipe::{Decision, Grade, Mark, Selection},
};
use index::{
    Index, Metadata, MetadataProvider, NoopMetadataProvider, NoopSidecarReader, Query, Scanner,
};
use std::path::Path;

#[test]
fn png_embedded_camera_is_searchable() {
    let dir = tempfile::tempdir().unwrap();
    // Synthetic 1x1 RGB PNG with an eXIf Model tag and valid chunk CRCs.
    std::fs::write(
        dir.path().join("camera.png"),
        include_bytes!("fixtures/camera.png"),
    )
    .unwrap();
    let mut index = Index::open(":memory:").unwrap();
    assert_eq!(
        index
            .scan(dir.path(), &NoopSidecarReader, &NoopMetadataProvider)
            .unwrap(),
        1
    );
    assert_eq!(
        index
            .search(&Query {
                camera: Some("CameraX".into()),
                ..Default::default()
            })
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn rendered_formats_reach_metadata_and_scan_incrementally() {
    struct RenderedMetadata;
    impl MetadataProvider for RenderedMetadata {
        fn read(&self, path: &Path) -> EngineResult<Metadata> {
            Ok(Metadata {
                camera: Some(path.file_name().unwrap().to_str().unwrap().into()),
                ..Default::default()
            })
        }
    }
    for extension in [
        "jpg", "jpeg", "png", "heic", "heif", "tif", "tiff", "PNG", "HEIC", "HEIF",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let filename = format!("rendered.{extension}");
        // Admission is independent of decoding; metadata is supplied by the host.
        std::fs::write(dir.path().join(&filename), b"rendered").unwrap();
        std::fs::write(dir.path().join("ignored.txt"), b"text").unwrap();
        let mut index = Index::open(":memory:").unwrap();
        let scanner = Scanner::new(&NoopSidecarReader, &RenderedMetadata);
        assert_eq!(
            scanner.scan(&mut index, dir.path()).unwrap(),
            1,
            "{extension}"
        );
        assert_eq!(
            index
                .search(&Query {
                    camera: Some(filename),
                    ..Default::default()
                })
                .unwrap()
                .len(),
            1,
            "{extension}"
        );
        assert_eq!(
            scanner.scan(&mut index, dir.path()).unwrap(),
            0,
            "{extension}"
        );
    }
}

#[test]
fn raw_fixtures_scan_incrementally_when_available() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw");
    if !root.is_dir() {
        eprintln!("skipping raw fixture scan: fixtures/raw is absent");
        return;
    }
    let mut i = Index::open(":memory:").unwrap();
    let count = i
        .scan(&root, &NoopSidecarReader, &NoopMetadataProvider)
        .unwrap();
    assert!(count >= 5, "expected the five RAW fixtures");
    assert_eq!(
        i.scan(&root, &NoopSidecarReader, &NoopMetadataProvider)
            .unwrap(),
        0
    );
}

#[test]
fn folder_boundaries_keyword_text_and_paging() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    for folder in ["a_%", "a_%other"] {
        std::fs::create_dir(root.join(folder)).unwrap();
        std::fs::write(root.join(folder).join("photo.jpg"), b"jpeg").unwrap();
    }
    let mut i = Index::open(":memory:").unwrap();
    Scanner::new(&NoopSidecarReader, &NoopMetadataProvider)
        .scan(&mut i, &root)
        .unwrap();
    let q = Query {
        folder: Some(root.join("a_%").display().to_string()),
        ..Default::default()
    };
    let ids = i.search(&q).unwrap();
    assert_eq!(ids.len(), 1);
    i.add_keyword("People", None).unwrap();
    i.add_keyword("Family", Some("People")).unwrap();
    i.add_keyword("Children", Some("Family")).unwrap();
    i.tag(ids[0], "Children").unwrap();
    assert_eq!(
        i.search(&Query {
            text: Some("Children".into()),
            ..Default::default()
        })
        .unwrap(),
        ids
    );
    assert_eq!(
        i.search(&Query {
            keyword: Some("People".into()),
            ..Default::default()
        })
        .unwrap(),
        ids
    );
    assert_eq!(i.images_with_keyword("People").unwrap(), ids);
    let first = i
        .search(&Query {
            limit: 1,
            ..Default::default()
        })
        .unwrap();
    let second = i
        .search(&Query {
            limit: 1,
            offset: 1,
            ..Default::default()
        })
        .unwrap();
    assert_ne!(first, second);
    assert!(i.add_keyword("People", Some("Children")).is_err());
    assert!(i.add_keyword("Orphan", Some("missing")).is_err());
}

#[test]
fn selection_and_all_filters() {
    struct Provider;
    impl MetadataProvider for Provider {
        fn read(&self, _: &Path) -> EngineResult<Metadata> {
            Ok(Metadata {
                camera: Some("Sony".into()),
                lens: Some("35mm".into()),
                capture_time: Some("2026-09-01T12:00:00".into()),
                ..Default::default()
            })
        }
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.arw"), b"raw").unwrap();
    let mut i = Index::open(":memory:").unwrap();
    i.scan(dir.path(), &NoopSidecarReader, &Provider).unwrap();
    let id = i.search(&Query::default()).unwrap()[0];
    let s = Selection {
        decision: Decision::Keep,
        grade: Some(Grade::Two),
        mark: Some(Mark::new("hero")),
    };
    i.set_selection(id, &s).unwrap();
    assert_eq!(i.selection(id).unwrap(), Some(s));
    let q = Query {
        camera: Some("Sony".into()),
        lens: Some("35mm".into()),
        date_from: Some("2026-09-01".into()),
        date_to: Some("2026-09-02".into()),
        decision: Some(Decision::Keep),
        grade: Some(Grade::Two),
        mark: Some("hero".into()),
        ..Default::default()
    };
    assert_eq!(i.search(&q).unwrap(), vec![id]);
    assert_eq!(i.facets(&q).unwrap().decisions, vec![("keep".into(), 1)]);
    i.set_selection(
        id,
        &Selection {
            decision: Decision::Reject,
            grade: Some(Grade::Three),
            mark: None,
        },
    )
    .unwrap();
    assert_eq!(i.selection(id).unwrap().unwrap().grade, None);
    assert!(i.search(&q).unwrap().is_empty());
}

#[test]
fn dotted_filename_recipe_changes_invalidate_scan() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("photo.v1.cr3"), b"raw").unwrap();
    std::fs::create_dir(dir.path().join(".edits")).unwrap();
    let recipe = dir.path().join(".edits/photo.v1.json");
    std::fs::write(&recipe, b"{}").unwrap();
    let mut index = Index::open(":memory:").unwrap();
    let scanner = Scanner::new(&NoopSidecarReader, &NoopMetadataProvider);
    assert_eq!(scanner.scan(&mut index, dir.path()).unwrap(), 1);
    assert_eq!(scanner.scan(&mut index, dir.path()).unwrap(), 0);
    std::fs::write(&recipe, b"{\"schema_version\":1}").unwrap();
    assert_eq!(scanner.scan(&mut index, dir.path()).unwrap(), 1);
    assert_eq!(scanner.scan(&mut index, dir.path()).unwrap(), 0);
}

#[test]
fn missing_root_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let mut i = Index::open(":memory:").unwrap();
    assert!(
        i.scan(
            dir.path().join("missing"),
            &NoopSidecarReader,
            &NoopMetadataProvider
        )
        .is_err()
    );
}
