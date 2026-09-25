use engine_api::id::ImageId;
use index::{Index, Query};
use library::{Album, AlbumGroup, Library, SavedSearch, SmartAlbum};
use rusqlite::{Connection, params};

#[test]
fn compiled_filters_and_nested_project_scope_use_real_sql() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.sqlite");
    let index = Index::open(&path).unwrap();
    let db = Connection::open(path).unwrap();
    db.execute_batch("INSERT INTO root VALUES(1,'/photos'); INSERT INTO folder VALUES(1,1,'/photos'); INSERT INTO keyword VALUES(1,'Anna',NULL); INSERT INTO keyword_closure VALUES(1,1,0);").unwrap();
    for n in 1..=3 {
        let id = ImageId(n).to_string();
        db.execute(
            "INSERT INTO file(id,folder_id,path,name,size,mtime) VALUES(?,1,?,?,1,1)",
            params![n as i64, format!("/photos/{n}.jpg"), format!("{n}.jpg")],
        )
        .unwrap();
        db.execute("INSERT INTO image(id,file_id,capture_time,camera,lens) VALUES(?,?,'2024-06-30T12:00:00','Canon','85mm')", params![id, n as i64]).unwrap();
        db.execute("INSERT INTO selection VALUES(?,'keep',3,NULL)", [&id])
            .unwrap();
        db.execute("INSERT INTO score VALUES(?,'focus',0.8,'test')", [&id])
            .unwrap();
        db.execute("INSERT INTO image_keyword VALUES(?,1)", [&id])
            .unwrap();
        db.execute(
            "INSERT INTO fts(rowid,image_id,caption) VALUES(?,?,'beach')",
            params![n as i64, id],
        )
        .unwrap();
    }
    let search: SavedSearch = "rating>=3 AND (text:beach OR camera:Sony) NOT decision:reject date:2024-01..2024-06 lens:85mm focus>0.6 person:Anna".parse().unwrap();
    let q = search.compile().unwrap();
    assert_eq!(index.search(&q).unwrap().len(), 3);
    let mut lib = Library {
        album_groups: vec![
            AlbumGroup {
                id: 1,
                name: "Project".into(),
                parent: None,
            },
            AlbumGroup {
                id: 2,
                name: "Nested".into(),
                parent: Some(1),
            },
        ],
        ..Default::default()
    };
    for (name, id, parent, image) in [
        ("A", 10, Some(1), 1),
        ("B", 11, Some(2), 2),
        ("Elsewhere", 12, None, 3),
    ] {
        lib.albums.insert(
            name.into(),
            Album {
                id,
                parent,
                images: vec![ImageId(image)],
                ..Default::default()
            },
        );
    }
    lib.smart_albums.push(SmartAlbum {
        id: 20,
        name: "Best".into(),
        parent: Some(1),
        search,
    });
    let q = lib.smart_query(20).unwrap();
    assert_eq!(index.search(&q).unwrap(), vec![ImageId(1), ImageId(2)]);
    struct Embeddings;
    impl library::SemanticSearch for Embeddings {
        fn search_text(
            &mut self,
            text: &str,
            k: usize,
        ) -> engine_api::EngineResult<Vec<(ImageId, f32)>> {
            assert_eq!(text, "laughing");
            assert_eq!(k, usize::MAX);
            Ok(vec![
                (ImageId(3), 1.0),
                (ImageId(2), 0.9),
                (ImageId(1), 0.8),
            ])
        }
    }
    lib.smart_albums[0].search = "rating>=3 semantic:laughing".parse().unwrap();
    let semantic = lib.smart_query(20).unwrap();
    assert!(index.search(&semantic).is_err());
    assert_eq!(
        index
            .search_with_semantic(&semantic, &mut Embeddings)
            .unwrap(),
        vec![ImageId(2), ImageId(1)]
    );
    assert_eq!(
        index
            .facets(&Query {
                limit: 1,
                offset: 1,
                ..q.clone()
            })
            .unwrap()
            .cameras,
        vec![("Canon".into(), 2)]
    );
    lib.albums.clear();
    lib.smart_albums[0].search = "rating>=3".parse().unwrap();
    assert!(
        index
            .search(&lib.smart_query(20).unwrap())
            .unwrap()
            .is_empty()
    );
    lib.album_groups[0].parent = Some(2);
    assert!(lib.smart_query(20).is_err());
}

#[test]
fn ordered_album_operations_keep_identity() {
    let mut lib = Library::default();
    let id = lib.create_album("Basket", None).unwrap();
    lib.add_to_album(id, &[ImageId(2), ImageId(1), ImageId(2)])
        .unwrap();
    assert_eq!(lib.albums["Basket"].images, vec![ImageId(2), ImageId(1)]);
    lib.rename_album(id, "Final").unwrap();
    assert_eq!(lib.albums["Final"].id, id);
    assert!(lib.reorder_album(id, &[ImageId(2)]).is_err());
    lib.reorder_album(id, &[ImageId(1), ImageId(2)]).unwrap();
    assert_eq!(lib.albums["Final"].images, vec![ImageId(1), ImageId(2)]);
    assert!(lib.create_album("Final", None).is_err());
    assert!(lib.create_album("Missing parent", Some(99)).is_err());
}
