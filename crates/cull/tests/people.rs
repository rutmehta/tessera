use cull::people::{NamePersonOptions, name_person};
use engine_api::id::ImageId;
use index::{FaceKey, FaceRecord, Index, NoopMetadataProvider, NoopSidecarReader, Query};
use library::Library;

fn catalog() -> (tempfile::TempDir, Index, Vec<ImageId>) {
    let dir = tempfile::tempdir().unwrap();
    for name in ["a.jpg", "b.jpg"] {
        std::fs::write(dir.path().join(name), b"jpeg").unwrap();
    }
    let mut index = Index::open(dir.path().join("index.sqlite")).unwrap();
    index
        .scan(dir.path(), &NoopSidecarReader, &NoopMetadataProvider)
        .unwrap();
    let ids = index.search(&Query::default()).unwrap();
    index.create_person("ada", None, None).unwrap();
    for id in &ids {
        index
            .replace_faces(
                *id,
                &[FaceRecord {
                    id: 0,
                    bbox: [20., 10., 40., 20.],
                    landmarks5: [[0.; 2]; 5],
                    confidence: 0.9,
                    embedding: None,
                    sharpness: 0.5,
                    eyes_open: None,
                }],
            )
            .unwrap();
        index
            .assign_face(
                FaceKey {
                    image_id: *id,
                    ordinal: 0,
                },
                "ada",
            )
            .unwrap();
    }
    (dir, index, ids)
}

#[test]
fn naming_propagates_to_two_images_and_library_without_sidecar_io() {
    let (dir, index, ids) = catalog();
    let library = dir.path().join("library.json");
    // Invalid XML must not even be read when sidecar export is disabled.
    let path = sidecar::Sidecar::paths(index.image_info(ids[0]).unwrap().path).xmp;
    std::fs::write(&path, "not XML").unwrap();
    name_person(
        &index,
        &library,
        "ada",
        Some("Ada"),
        &NamePersonOptions::default(),
    )
    .unwrap();
    for id in &ids {
        assert_eq!(
            index.face_assignments(*id).unwrap()[0]
                .person_name
                .as_deref(),
            Some("Ada")
        );
    }
    assert_eq!(Library::read(&library).unwrap().people, ["Ada"]);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "not XML");
    assert!(
        !sidecar::Sidecar::paths(index.image_info(ids[1]).unwrap().path)
            .xmp
            .exists()
    );
    name_person(
        &index,
        &library,
        "ada",
        Some("Grace"),
        &NamePersonOptions::default(),
    )
    .unwrap();
    assert_eq!(Library::read(&library).unwrap().people, ["Grace"]);
    name_person(&index, &library, "ada", None, &NamePersonOptions::default()).unwrap();
    assert!(Library::read(&library).unwrap().people.is_empty());
}

#[test]
fn opt_in_exports_normalized_faces_and_preserves_foreign_xml() {
    let (dir, index, ids) = catalog();
    let library = dir.path().join("library.json");
    let mut options = NamePersonOptions {
        write_sidecars: true,
        person_keywords: true,
        ..Default::default()
    };
    for id in &ids {
        options.dimensions.insert(*id, (200, 100));
    }
    let image = index.image_info(ids[0]).unwrap().path;
    let legacy = image.with_extension("xmp");
    let xml = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:custom="urn:custom"><custom:Keep value="yes">untouched</custom:Keep></rdf:Description></rdf:RDF></x:xmpmeta>"#;
    std::fs::write(&legacy, xml).unwrap();
    name_person(&index, &library, "ada", Some("Ada & Grace"), &options).unwrap();
    for (n, id) in ids.iter().enumerate() {
        let path = if n == 0 {
            legacy.clone()
        } else {
            sidecar::Sidecar::paths(index.image_info(*id).unwrap().path).xmp
        };
        let packet = sidecar::Sidecar::read_xmp(&path).unwrap();
        assert_eq!(
            packet.face_regions().unwrap(),
            vec![sidecar::FaceRegion {
                name: "Ada & Grace".into(),
                x: 0.2,
                y: 0.2,
                w: 0.2,
                h: 0.2
            }]
        );
        assert!(
            packet
                .metadata()
                .unwrap()
                .keywords
                .contains(&"Ada & Grace".into())
        );
    }
    assert!(
        std::fs::read_to_string(&legacy)
            .unwrap()
            .contains(r#"<custom:Keep value="yes">untouched</custom:Keep>"#)
    );
    assert!(!sidecar::Sidecar::paths(&image).xmp.exists());
    name_person(&index, &library, "ada", None, &options).unwrap();
    assert_eq!(
        sidecar::Sidecar::read_xmp(&legacy)
            .unwrap()
            .face_regions()
            .unwrap()[0]
            .name,
        ""
    );
}

#[test]
fn export_uses_analysis_scores_when_dimensions_not_explicit() {
    let (dir, index, ids) = catalog();
    for id in &ids {
        for (signal, value) in [("analysis_width", 200.), ("analysis_height", 100.)] {
            index
                .set_score(
                    *id,
                    &index::Score {
                        signal: signal.into(),
                        value,
                        model: "test".into(),
                    },
                )
                .unwrap();
        }
    }
    name_person(
        &index,
        dir.path().join("library.json"),
        "ada",
        Some("Ada"),
        &NamePersonOptions {
            write_sidecars: true,
            ..Default::default()
        },
    )
    .unwrap();
    for id in ids {
        let packet = sidecar::Sidecar::read_xmp(
            sidecar::Sidecar::paths(index.image_info(id).unwrap().path).xmp,
        )
        .unwrap();
        assert_eq!(packet.face_regions().unwrap()[0].x, 0.2);
        assert!(packet.metadata().unwrap().keywords.is_empty());
    }
}

#[test]
fn failed_library_write_restores_cluster_name() {
    let (dir, index, ids) = catalog();
    let path = dir.path().join("missing-parent/library.json");
    assert!(
        name_person(
            &index,
            &path,
            "ada",
            Some("Ada"),
            &NamePersonOptions::default()
        )
        .is_err()
    );
    assert_eq!(index.face_assignments(ids[0]).unwrap()[0].person_name, None);
    assert!(!path.exists());
}

#[test]
fn library_failure_compensates_sidecars_and_preserves_original_bytes() {
    let (dir, index, ids) = catalog();
    let existing = sidecar::Sidecar::paths(index.image_info(ids[0]).unwrap().path).xmp;
    let packet = sidecar::XmpPacket::from_selection(&Default::default(), &Default::default());
    sidecar::Sidecar::write_xmp(&existing, &packet).unwrap();
    let before = std::fs::read(&existing).unwrap();
    let options = NamePersonOptions {
        write_sidecars: true,
        dimensions: ids.iter().map(|id| (*id, (200, 100))).collect(),
        ..Default::default()
    };
    assert!(
        name_person(
            &index,
            dir.path().join("missing/library.json"),
            "ada",
            Some("Ada"),
            &options
        )
        .is_err()
    );
    assert_eq!(std::fs::read(existing).unwrap(), before);
    assert!(
        !sidecar::Sidecar::paths(index.image_info(ids[1]).unwrap().path)
            .xmp
            .exists()
    );
    assert_eq!(index.face_assignments(ids[0]).unwrap()[0].person_name, None);
}

#[test]
fn invalid_export_preflight_leaves_names_and_documents_untouched() {
    let (dir, index, ids) = catalog();
    let path = dir.path().join("library.json");
    let options = NamePersonOptions {
        write_sidecars: true,
        ..Default::default()
    };
    assert!(name_person(&index, &path, "ada", Some("Ada"), &options).is_err());
    assert_eq!(index.face_assignments(ids[0]).unwrap()[0].person_name, None);
    assert!(!path.exists());
    let mut options = options;
    for id in &ids {
        options.dimensions.insert(*id, (200, 100));
    }
    let xmp = sidecar::Sidecar::paths(index.image_info(ids[1]).unwrap().path).xmp;
    std::fs::write(&xmp, "invalid XML").unwrap();
    assert!(name_person(&index, &path, "ada", Some("Ada"), &options).is_err());
    assert_eq!(index.face_assignments(ids[0]).unwrap()[0].person_name, None);
    assert_eq!(std::fs::read_to_string(xmp).unwrap(), "invalid XML");
    assert!(!path.exists());
    assert!(
        !sidecar::Sidecar::paths(index.image_info(ids[0]).unwrap().path)
            .xmp
            .exists()
    );
}

#[test]
fn synchronization_deduplicates_display_names_without_losing_library_fields() {
    let (dir, index, _) = catalog();
    index.create_person("other-ada", Some("Ada"), None).unwrap();
    index.create_person("blank", Some("  "), None).unwrap();
    let mut library = Library {
        people: vec!["stale".into()],
        ..Default::default()
    };
    library
        .unknown
        .insert("future".into(), serde_json::json!({"keep": true}));
    library.sync_people_names(&index).unwrap();
    assert_eq!(library.people, ["Ada"]);
    let path = dir.path().join("library.json");
    library.write(&path).unwrap();
    name_person(
        &index,
        &path,
        "ada",
        Some("Ada"),
        &NamePersonOptions::default(),
    )
    .unwrap();
    assert_eq!(Library::read(&path).unwrap(), library);
    name_person(&index, &path, "ada", None, &NamePersonOptions::default()).unwrap();
    assert_eq!(Library::read(&path).unwrap().people, ["Ada"]);
}

#[test]
fn shared_legacy_sidecar_is_rejected_even_when_peer_is_not_in_cluster() {
    let (dir, mut index, ids) = catalog();
    let image = index.image_info(ids[0]).unwrap().path;
    std::fs::write(image.with_extension("cr3"), b"raw").unwrap();
    index
        .scan(dir.path(), &NoopSidecarReader, &NoopMetadataProvider)
        .unwrap();
    let legacy = image.with_extension("xmp");
    let packet = sidecar::XmpPacket::from_selection(&Default::default(), &Default::default());
    sidecar::Sidecar::write_xmp(&legacy, &packet).unwrap();
    let before = std::fs::read(&legacy).unwrap();
    let options = NamePersonOptions {
        write_sidecars: true,
        dimensions: ids.iter().map(|id| (*id, (200, 100))).collect(),
        ..Default::default()
    };
    let error = name_person(
        &index,
        dir.path().join("library.json"),
        "ada",
        Some("Ada"),
        &options,
    )
    .unwrap_err();
    assert!(error.to_string().contains("collision"));
    assert_eq!(std::fs::read(legacy).unwrap(), before);
    assert_eq!(index.face_assignments(ids[0]).unwrap()[0].person_name, None);
}
