use engine_api::id::ImageId;
use index::Index;
use library::{Library, NodeKind, SavedSearch};
use rusqlite::{Connection, params};

fn names(lib: &Library) -> Vec<(String, usize)> {
    lib.sidebar()
        .into_iter()
        .map(|n| (n.name, n.depth))
        .collect()
}

#[test]
fn sidebar_order_nesting_and_safe_group_delete() {
    let mut lib = Library::default();
    let trips = lib.create_group("Trips", None).unwrap();
    let b = lib.create_album("B", None).unwrap();
    let a = lib.create_album("A", Some(trips)).unwrap();
    let smart = lib
        .create_smart_album("Best", "rating>=2".parse().unwrap(), Some(trips), true)
        .unwrap();
    // Unordered: groups first, then albums, then smart albums, by name.
    assert_eq!(
        names(&lib),
        [
            ("Trips".into(), 0),
            ("A".into(), 1),
            ("Best".into(), 1),
            ("B".into(), 0)
        ]
    );
    // Reorder at the root and nest B inside Trips after A.
    lib.move_node(b, None, 0).unwrap();
    assert_eq!(lib.sidebar()[0].name, "B");
    lib.move_node(b, Some(trips), 1).unwrap();
    assert_eq!(
        names(&lib),
        [
            ("Trips".into(), 0),
            ("A".into(), 1),
            ("B".into(), 1),
            ("Best".into(), 1)
        ]
    );
    assert_eq!(lib.albums["B"].parent, Some(trips));
    // A group cannot contain itself (directly or through a descendant).
    let inner = lib.create_group("Inner", Some(trips)).unwrap();
    assert!(lib.move_node(trips, Some(inner), 0).is_err());
    assert!(lib.move_node(trips, Some(trips), 0).is_err());
    assert!(
        lib.move_node(a, Some(b), 0).is_err(),
        "albums are not containers"
    );
    // Deleting a group lifts its contents to the root in its place; albums keep members.
    lib.add_to_album(a, &[ImageId(1)]).unwrap();
    lib.delete_node(trips).unwrap();
    assert_eq!(
        names(&lib),
        [
            ("A".into(), 0),
            ("B".into(), 0),
            ("Best".into(), 0),
            ("Inner".into(), 0)
        ]
    );
    assert_eq!(lib.albums["A"].images, vec![ImageId(1)]);
    assert_eq!(lib.smart_albums[0].parent, None);
    // Round trip through library.json keeps the order and the scope flag.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.json");
    lib.write(&path).unwrap();
    let back = Library::read(&path).unwrap();
    assert_eq!(back, lib);
    let kinds: Vec<_> = back.sidebar().iter().map(|n| n.kind).collect();
    assert_eq!(
        kinds,
        [
            NodeKind::Album,
            NodeKind::Album,
            NodeKind::SmartAlbum,
            NodeKind::Group
        ]
    );
    lib.rename_node(smart, "Top").unwrap();
    lib.rename_node(a, "Z").unwrap();
    assert!(
        lib.rename_node(a, "B").is_err(),
        "album names are unique handles"
    );
    assert!(lib.albums.contains_key("Z"));
    lib.delete_node(smart).unwrap();
    assert!(lib.smart_albums.is_empty());
}

#[test]
fn older_documents_default_to_scoped_smart_albums() {
    let json = r#"{"schema_version":1,"smart_albums":[{"id":3,"name":"S","parent":1,"search":{"Rule":{"criteria":"rating","operation":">=","value":2}}}]}"#;
    let lib: Library = serde_json::from_str(json).unwrap();
    assert!(lib.smart_albums[0].scoped);
}

fn catalog() -> (tempfile::TempDir, Index) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.sqlite");
    let index = Index::open(&path).unwrap();
    let db = Connection::open(path).unwrap();
    db.execute_batch("INSERT INTO root VALUES(1,'/p'); INSERT INTO folder VALUES(1,1,'/p');")
        .unwrap();
    for n in 1..=4 {
        let id = ImageId(n).to_string();
        db.execute(
            "INSERT INTO file(id,folder_id,path,name,size,mtime) VALUES(?,1,?,?,1,1)",
            params![n as i64, format!("/p/{n}.jpg"), format!("{n}.jpg")],
        )
        .unwrap();
        // Image 4 stores Unix seconds like RAW scans do (2024-06-15).
        let time = if n == 4 {
            "1718445600".to_string()
        } else {
            format!("2024-0{n}-10T12:00:00")
        };
        db.execute(
            "INSERT INTO image(id,file_id,capture_time) VALUES(?,?,?)",
            params![id, n as i64, time],
        )
        .unwrap();
        db.execute(
            "INSERT INTO selection VALUES(?,'keep',?,NULL)",
            params![id, n.min(3) as i64],
        )
        .unwrap();
    }
    (dir, index)
}

#[test]
fn album_rules_and_scope_toggle_resolve_against_the_library() {
    let (_dir, index) = catalog();
    let mut lib = Library::default();
    let group = lib.create_group("Project", None).unwrap();
    let a = lib.create_album("Picks", Some(group)).unwrap();
    lib.add_to_album(a, &[ImageId(1), ImageId(2)]).unwrap();
    let run = |lib: &Library, text: &str| {
        let mut ids = index
            .search(&lib.compile_search(&text.parse().unwrap()).unwrap())
            .unwrap();
        ids.sort();
        ids
    };
    assert_eq!(run(&lib, "album:none"), [ImageId(3), ImageId(4)]);
    assert_eq!(run(&lib, "album:any"), [ImageId(1), ImageId(2)]);
    assert_eq!(run(&lib, "album:Picks rating>=2"), [ImageId(2)]);
    assert_eq!(run(&lib, "album!=Picks"), [ImageId(3), ImageId(4)]);
    // Date ranges also match RAW rows that store Unix seconds.
    assert_eq!(run(&lib, "date:2024-06"), [ImageId(4)]);
    assert!(lib.compile_search(&"album:Nope".parse().unwrap()).is_err());
    let plain: SavedSearch = "album:Picks".parse().unwrap();
    assert!(plain.compile().is_err(), "album rules need the library");
    assert!(
        lib.create_smart_album("Bad", "album:Nope".parse().unwrap(), None, false)
            .is_err()
    );

    let s = lib
        .create_smart_album("Graded", "rating>=2".parse().unwrap(), Some(group), true)
        .unwrap();
    assert_eq!(
        index.search(&lib.smart_query(s).unwrap()).unwrap(),
        [ImageId(2)]
    );
    lib.update_smart_album(s, None, Some(false)).unwrap();
    assert_eq!(index.search(&lib.smart_query(s).unwrap()).unwrap().len(), 3);
}

#[test]
fn diagnostics_locate_the_offending_rule() {
    let diag = |t: &str| SavedSearch::parse_diagnostic(t).unwrap_err();
    let d = diag("rating>=2 wat:x");
    assert_eq!((d.start, d.end), (10, 15));
    assert!(d.message.starts_with("Unsupported field"), "{}", d.message);
    assert!(!d.message.contains("at byte"), "{}", d.message);
    let d = diag(r#"keep camera:"EOS 5D" rating:9"#);
    assert_eq!(
        &r#"keep camera:"EOS 5D" rating:9"#[d.start..d.end],
        "rating:9"
    );
    let d = diag("(a OR b");
    assert_eq!((d.start, d.end), (7, 7));
    assert!(d.message.starts_with("Expected closing parenthesis"));
    let text = "éclair AND";
    let d = diag(text);
    assert_eq!(d.start, text.len());
    let d = diag("海 keyword:");
    assert_eq!(d.start, "海 keyword:".len());
    assert!(SavedSearch::parse_diagnostic("album:any").is_ok());
}

#[test]
fn keyword_tree_is_unique_and_safely_editable() {
    let mut lib = Library::default();
    lib.add_keyword("Places", None).unwrap();
    lib.add_keyword("France", Some("Places")).unwrap();
    lib.add_keyword("Paris", Some("France")).unwrap();
    lib.add_keyword("Animals", None).unwrap();
    assert!(lib.add_keyword("Paris", None).is_err());
    assert!(lib.add_keyword("a|b", None).is_err());
    assert!(lib.add_keyword("x", Some("Missing")).is_err());
    assert_eq!(
        lib.keyword_path("Paris").unwrap(),
        ["Places", "France", "Paris"]
    );
    assert!(lib.move_keyword("Places", Some("Paris")).is_err());
    lib.move_keyword("France", None).unwrap();
    assert_eq!(lib.keyword_path("Paris").unwrap(), ["France", "Paris"]);
    lib.delete_keyword("France").unwrap();
    assert_eq!(lib.keyword_path("Paris").unwrap(), ["Paris"]);
    assert_eq!(
        lib.keyword_pairs(),
        [
            ("Animals".into(), None),
            ("Paris".into(), None),
            ("Places".into(), None)
        ]
    );
}
