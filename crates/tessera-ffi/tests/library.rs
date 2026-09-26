//! Library surface: albums/groups/smart albums, faceted search with located
//! diagnostics, keywords and IPTC written to XMP. Scratch folders only.
use std::sync::Arc;
use tessera_ffi::*;

struct Fixture {
    _dir: tempfile::TempDir,
    support: String,
    folder: String,
    engine: Arc<Engine>,
    store: Arc<LibraryStore>,
    /// Image ids for a.jpg..d.jpg.
    ids: Vec<String>,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let photos = dir.path().join("photos");
    std::fs::create_dir(&photos).unwrap();
    for (n, name) in ["a", "b", "c", "d"].iter().enumerate() {
        image::RgbImage::from_fn(48, 32, |x, y| {
            image::Rgb([(x * 5) as u8, (y * 7) as u8, n as u8 * 60])
        })
        .save(photos.join(format!("{name}.jpg")))
        .unwrap();
    }
    let support = dir.path().join("support").to_string_lossy().into_owned();
    let engine = Engine::open(support.clone()).unwrap();
    let folder = engine
        .index_folder(photos.to_string_lossy().into_owned())
        .unwrap()
        .path;
    let mut rows = engine.list_images(ImageQuery::default()).unwrap();
    rows.sort_by(|a, b| a.path.cmp(&b.path));
    let ids = rows.into_iter().map(|r| r.id).collect();
    let store = engine
        .clone()
        .open_library(format!("{folder}/library.json"))
        .unwrap();
    Fixture {
        _dir: dir,
        support,
        folder,
        engine,
        store,
        ids,
    }
}

fn request(
    text: &str,
    filters: Vec<FacetFilter>,
    scope: SearchScope,
    folder: &str,
) -> SearchRequest {
    SearchRequest {
        text: text.into(),
        filters,
        scope,
        folder: Some(folder.into()),
    }
}

fn count(list: &[FacetCount], value: &str) -> u32 {
    list.iter()
        .find(|c| c.value == value)
        .map_or(0, |c| c.count)
}

#[test]
fn albums_groups_and_smart_albums_are_ordered_nested_and_safely_deleted() {
    let f = fixture();
    let s = &f.store;
    let trip = s.create_group("Trip".into(), None).unwrap();
    let picks = s.create_album("Picks".into(), None).unwrap();
    let day1 = s.create_album("Day 1".into(), Some(trip)).unwrap();
    s.add_to_album(
        picks,
        vec![f.ids[2].clone(), f.ids[0].clone(), f.ids[2].clone()],
    )
    .unwrap();
    assert_eq!(
        s.album_images(picks).unwrap(),
        [f.ids[2].clone(), f.ids[0].clone()]
    );
    s.reorder_album(picks, vec![f.ids[0].clone(), f.ids[2].clone()])
        .unwrap();
    assert_eq!(
        s.album_images(picks).unwrap(),
        [f.ids[0].clone(), f.ids[2].clone()]
    );
    assert!(s.reorder_album(picks, vec![f.ids[0].clone()]).is_err());

    // Drag Picks into Trip above Day 1, then rename.
    s.move_node(picks, Some(trip), 0).unwrap();
    s.rename(picks, "Best".into()).unwrap();
    let nodes = s.nodes().unwrap();
    let outline: Vec<_> = nodes
        .iter()
        .map(|n| (n.name.as_str(), n.depth, n.kind))
        .collect();
    assert_eq!(
        outline,
        [
            ("Trip", 0, LibraryNodeKind::Group),
            ("Best", 1, LibraryNodeKind::Album),
            ("Day 1", 1, LibraryNodeKind::Album),
        ]
    );
    assert_eq!(nodes[1].handle.as_deref(), Some("Best"));
    assert_eq!(nodes[1].image_count, 2);
    assert!(s.move_node(trip, Some(trip), 0).is_err());

    // A rule that does not parse cannot be saved; a valid one can, scoped to the group.
    let err = s
        .create_smart_album("Bad".into(), "rating>=".into(), Some(trip), true)
        .unwrap_err();
    assert!(err.to_string().contains("Expected rule value"), "{err}");
    let smart = s
        .create_smart_album(
            "Undecided here".into(),
            "decision:undecided".into(),
            Some(trip),
            true,
        )
        .unwrap();
    let node = s
        .nodes()
        .unwrap()
        .into_iter()
        .find(|n| n.id == smart)
        .unwrap();
    assert_eq!(node.rule.as_deref(), Some("decision:undecided"));
    assert!(node.scoped);
    let found = |scope| {
        let mut ids = s
            .search(request("", vec![], scope, &f.folder))
            .unwrap()
            .image_ids;
        ids.sort();
        ids
    };
    let mut in_best = vec![f.ids[0].clone(), f.ids[2].clone()];
    in_best.sort();
    assert_eq!(found(SearchScope::SmartAlbum { id: smart }), in_best);
    assert_eq!(found(SearchScope::Group { id: trip }), in_best);
    s.update_smart_album(smart, None, Some(false)).unwrap();
    assert_eq!(found(SearchScope::SmartAlbum { id: smart }).len(), 4);

    // Safe delete: the group goes, its contents move up; files are untouched.
    s.delete_node(trip).unwrap();
    let names: Vec<_> = s
        .nodes()
        .unwrap()
        .into_iter()
        .map(|n| (n.name, n.depth))
        .collect();
    assert_eq!(
        names,
        [
            ("Best".into(), 0),
            ("Day 1".into(), 0),
            ("Undecided here".into(), 0)
        ]
    );
    s.delete_node(picks).unwrap();
    s.delete_node(day1).unwrap();
    assert_eq!(
        std::fs::read_dir(&f.folder)
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|x| x == "jpg")
            })
            .count(),
        4
    );
}

#[test]
fn faceted_search_counts_other_filters_and_locates_text_errors() {
    let f = fixture();
    let s = &f.store;
    let keep = |grade| Selection {
        decision: Decision::Keep,
        grade,
        mark: None,
    };
    f.engine
        .set_selection(f.ids[0].clone(), keep(Some(2)))
        .unwrap();
    f.engine
        .set_selection(f.ids[1].clone(), keep(Some(3)))
        .unwrap();
    f.engine
        .set_selection(
            f.ids[2].clone(),
            Selection {
                decision: Decision::Reject,
                grade: None,
                mark: Some("Review".into()),
            },
        )
        .unwrap();
    let album = s.create_album("Portfolio".into(), None).unwrap();
    s.add_to_album(album, vec![f.ids[1].clone()]).unwrap();

    let all = s
        .search(request("", vec![], SearchScope::All, &f.folder))
        .unwrap();
    assert_eq!(all.image_ids.len(), 4);
    assert_eq!(all.rule, "");
    assert_eq!(count(&all.facets.decisions, "keep"), 2);
    assert_eq!(count(&all.facets.grades, "3"), 1);
    assert_eq!(count(&all.facets.marks, "Review"), 1);
    assert_eq!((all.facets.in_any_album, all.facets.in_no_album), (1, 3));

    // Decision filter: the decision facet still shows the alternatives (other
    // filters only), while the grade facet narrows to keeps.
    let decision = FacetFilter {
        field: FacetField::Decision,
        values: vec!["keep".into()],
    };
    let r = s
        .search(request(
            "",
            vec![decision.clone()],
            SearchScope::All,
            &f.folder,
        ))
        .unwrap();
    assert_eq!(r.image_ids.len(), 2);
    assert_eq!(count(&r.facets.decisions, "reject"), 1);
    assert_eq!(count(&r.facets.grades, "2"), 1);
    assert_eq!((r.facets.in_any_album, r.facets.in_no_album), (1, 1));
    assert_eq!(r.rule, "decision:keep");

    // "Not in any album" is a derived status filter.
    let unfiled = FacetFilter {
        field: FacetField::Album,
        values: vec!["none".into()],
    };
    let r = s
        .search(request(
            "rating>=2",
            vec![decision, unfiled],
            SearchScope::All,
            &f.folder,
        ))
        .unwrap();
    assert_eq!(r.image_ids, [f.ids[0].clone()]);
    assert_eq!(r.rule, "rating>=2 AND decision:keep AND album:none");
    // The composed rule saves as a smart album with the same result.
    let smart = s
        .create_smart_album("Unfiled keeps".into(), r.rule.clone(), None, false)
        .unwrap();
    let saved = s
        .search(request(
            "",
            vec![],
            SearchScope::SmartAlbum { id: smart },
            &f.folder,
        ))
        .unwrap();
    assert_eq!(saved.image_ids, r.image_ids);

    // Invalid text: located diagnostic; filters still apply.
    let text = "rating>=2 wat:x";
    let r = s
        .search(request(
            text,
            vec![FacetFilter {
                field: FacetField::Mark,
                values: vec!["Review".into()],
            }],
            SearchScope::All,
            &f.folder,
        ))
        .unwrap();
    let d = r.diagnostic.unwrap();
    assert_eq!(&text[d.start as usize..d.end as usize], "wat:x");
    assert_eq!(r.image_ids, [f.ids[2].clone()]);
    // Unknown album names are reported, never an empty match.
    let r = s
        .search(request("album:Nope", vec![], SearchScope::All, &f.folder))
        .unwrap();
    assert!(r.diagnostic.unwrap().message.contains("Nope"));
    assert_eq!(r.image_ids.len(), 4);

    // Album scope keeps manual order.
    s.add_to_album(album, vec![f.ids[3].clone(), f.ids[0].clone()])
        .unwrap();
    let r = s
        .search(request(
            "",
            vec![],
            SearchScope::Album { id: album },
            &f.folder,
        ))
        .unwrap();
    assert_eq!(
        r.image_ids,
        [f.ids[1].clone(), f.ids[3].clone(), f.ids[0].clone()]
    );
    assert_eq!(r.rule, "album:Portfolio");
}

#[test]
fn rule_checks_report_positions_and_items_round_trip() {
    let f = fixture();
    let s = &f.store;
    let check = s
        .check_rule("rating>=2 AND (camera:\"X-T5\" OR NOT keyword:beach)".into())
        .unwrap();
    assert!(check.diagnostic.is_none());
    let kinds: Vec<_> = check.items.iter().map(|i| (i.kind, i.children)).collect();
    assert_eq!(
        kinds,
        [
            (RuleItemKind::All, 2),
            (RuleItemKind::Rule, 0),
            (RuleItemKind::Any, 2),
            (RuleItemKind::Rule, 0),
            (RuleItemKind::Not, 1),
            (RuleItemKind::Rule, 0),
        ]
    );
    let formatted = s.format_rule(check.items.clone()).unwrap();
    assert_eq!(
        formatted.text,
        "rating>=2 AND (camera:X-T5 OR NOT keyword:beach)"
    );
    assert!(formatted.diagnostic.is_none());
    // A bad leaf from the editor is located in the rendered text.
    let mut items = check.items;
    items[1].value = "7".into();
    let bad = s.format_rule(items).unwrap();
    let d = bad.diagnostic.unwrap();
    assert_eq!(&bad.text[d.start as usize..d.end as usize], "rating>=7");
    let d = s.check_rule("(beach".into()).unwrap().diagnostic.unwrap();
    assert_eq!((d.start, d.end), (6, 6));
    assert!(s.check_rule("   ".into()).unwrap().diagnostic.is_some());
}

#[test]
fn keywords_and_iptc_persist_to_xmp_and_search() {
    let f = fixture();
    let s = &f.store;
    s.add_keyword("Places".into(), None).unwrap();
    s.add_keyword("France".into(), Some("Places".into()))
        .unwrap();
    s.apply_keywords(
        f.ids[..2].to_vec(),
        vec!["France".into(), "beach".into()],
        true,
    )
    .unwrap();
    let tree = s.keywords(Some(f.folder.clone())).unwrap();
    let row = |name: &str| tree.iter().find(|k| k.name == name).unwrap().clone();
    assert_eq!(row("France").parent.as_deref(), Some("Places"));
    assert_eq!(row("France").count, 2);
    assert!(row("beach").in_tree, "applied keywords join the tree");
    // Hierarchy: searching the parent finds images tagged with the child.
    let r = s
        .search(request(
            "keyword:Places",
            vec![],
            SearchScope::All,
            &f.folder,
        ))
        .unwrap();
    assert_eq!(r.image_ids.len(), 2);
    assert_eq!(count(&r.facets.keywords, "France"), 2);
    s.apply_keywords(vec![f.ids[1].clone()], vec!["France".into()], false)
        .unwrap();

    s.set_iptc(
        vec![f.ids[0].clone()],
        IptcEdit {
            title: Some("Harbour at dawn".into()),
            caption: Some("Boats \"leaving\" & returning".into()),
            copyright: Some("© 2026 R. Mehta".into()),
            creator: Some("R. Mehta; Assistant".into()),
            keywords: None,
            alt_text: None,
        },
    )
    .unwrap();
    let path = f.engine.list_images(ImageQuery::default()).unwrap();
    let a = path.iter().find(|r| r.id == f.ids[0]).unwrap();
    let xmp = std::fs::read_to_string(format!("{}.xmp", a.path)).unwrap();
    assert!(xmp.contains("Harbour at dawn"), "{xmp}");
    assert!(xmp.contains("Places|France"), "{xmp}");
    // Reopen: the edit is read back from the sidecar; decisions are untouched.
    drop(f.store);
    drop(f.engine);
    let engine = Engine::open(f.support.clone()).unwrap();
    let store = engine
        .clone()
        .open_library(format!("{}/library.json", f.folder))
        .unwrap();
    let meta = store.metadata(f.ids[0].clone()).unwrap();
    assert_eq!(meta.title, "Harbour at dawn");
    assert_eq!(meta.caption, "Boats \"leaving\" & returning");
    assert_eq!(meta.creator, "R. Mehta; Assistant");
    assert_eq!(meta.copyright, "© 2026 R. Mehta");
    assert_eq!(meta.keywords, ["France", "beach"]);
    assert!(
        meta.fields
            .iter()
            .any(|m| m.name == "Name" && m.value == "a.jpg")
    );
    let b = store.metadata(f.ids[1].clone()).unwrap();
    assert_eq!(b.keywords, ["beach"]);
    assert!(
        b.hierarchical_keywords
            .iter()
            .all(|h| !h.contains("France"))
    );
    // Caption text is searchable.
    let r = store
        .search(request("harbour", vec![], SearchScope::All, &f.folder))
        .unwrap();
    assert!(r.image_ids.is_empty(), "title is not indexed; caption is");
    let r = store
        .search(request("boats", vec![], SearchScope::All, &f.folder))
        .unwrap();
    assert_eq!(r.image_ids, [f.ids[0].clone()]);
    // Safe keyword delete: the list entry goes, the photo keeps its tag.
    store.delete_keyword("beach".into()).unwrap();
    let tree = store.keywords(Some(f.folder.clone())).unwrap();
    let beach = tree.iter().find(|k| k.name == "beach").unwrap();
    assert!(!beach.in_tree);
    assert_eq!(beach.count, 2);
}
