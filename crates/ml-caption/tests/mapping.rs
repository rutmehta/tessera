use library::{Keyword, Library};
use ml_caption::{WritePolicy, accept_keywords, map_keyword};

#[test]
fn maps_names_before_synonyms_and_proposes_without_mutation() {
    let library = Library {
        keywords: vec![
            Keyword {
                id: 1,
                name: "Nature".into(),
                synonyms: vec![],
                children: vec![Keyword {
                    id: 2,
                    name: "Bird".into(),
                    synonyms: vec!["avian".into(), "Feather".into()],
                    children: vec![],
                }],
            },
            Keyword {
                id: 3,
                name: "Feather".into(),
                synonyms: vec![],
                children: vec![],
            },
        ],
        ..Default::default()
    };
    assert_eq!(
        map_keyword(&library, " AVIAN ").unwrap().path,
        ["Nature", "Bird"]
    );
    assert!(!map_keyword(&library, "avian").unwrap().proposed);
    assert_eq!(map_keyword(&library, "feather").unwrap().path, ["Feather"]);
    assert_eq!(
        map_keyword(&library, "Sunset").unwrap().path,
        ["Suggested", "Sunset"]
    );
    assert!(map_keyword(&library, " ").is_err());
    assert!(map_keyword(&library, "a|b").is_err());
    assert_eq!(library.keywords.len(), 2);
}

#[test]
fn bulk_accept_defaults_to_index_only_and_xmp_is_explicit() {
    let tmp = tempfile::tempdir().unwrap();
    let photo = tmp.path().join("photo.jpg");
    image::RgbImage::new(8, 8).save(&photo).unwrap();
    let before = std::fs::read(&photo).unwrap();
    let mut idx = index::Index::open(":memory:").unwrap();
    idx.scan(
        tmp.path(),
        &index::NoopSidecarReader,
        &index::NoopMetadataProvider,
    )
    .unwrap();
    let id = idx.search(&index::Query::default()).unwrap()[0];
    let mut lib = Library::default();
    let names = vec!["sunset".into(), "red".into()];
    accept_keywords(&idx, &mut lib, &[id], &names, WritePolicy::IndexOnly).unwrap();
    assert!(!sidecar::Sidecar::paths(&photo).xmp.exists());
    assert_eq!(idx.images_with_keyword("Suggested").unwrap(), [id]);
    std::fs::write(sidecar::Sidecar::paths(&photo).xmp, "changed sidecar stamp").unwrap();
    idx.scan(
        tmp.path(),
        &index::NoopSidecarReader,
        &index::NoopMetadataProvider,
    )
    .unwrap();
    assert_eq!(idx.images_with_keyword("sunset").unwrap(), [id]);
    std::fs::remove_file(sidecar::Sidecar::paths(&photo).xmp).unwrap();
    accept_keywords(&idx, &mut lib, &[id], &names, WritePolicy::Xmp).unwrap();
    let xmp = sidecar::Sidecar::read_xmp(sidecar::Sidecar::paths(&photo).xmp).unwrap();
    assert_eq!(
        xmp.metadata().unwrap().hierarchical_keywords,
        ["Suggested|sunset", "Suggested|red"]
    );
    assert_eq!(std::fs::read(photo).unwrap(), before);
    assert_eq!(lib.keywords[0].children.len(), 2);
}
