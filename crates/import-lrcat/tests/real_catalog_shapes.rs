//! Shapes seen in real Lightroom Classic 15.5 catalogs that the synthetic
//! fixture did not originally cover (B5-29): the unnamed keyword root, NULL
//! names, unknown collection kinds and empty develop rows. The importer
//! degrades with report entries instead of aborting.
use import_lrcat::{fixture, import, inspect};
use rusqlite::Connection;

fn setup() -> (tempfile::TempDir, fixture::Fixture) {
    let dir = tempfile::tempdir().unwrap();
    let f = fixture::write(dir.path()).unwrap();
    (dir, f)
}

fn names(ks: &[import_lrcat::Keyword]) -> Vec<&str> {
    ks.iter().map(|k| k.name.as_str()).collect()
}

#[test]
fn unnamed_root_keyword_is_the_tree_root() {
    let (_dir, f) = setup();
    let plan = import(&f.catalog).unwrap();
    assert_eq!(names(&plan.library.keywords), ["Places", "People", "Trips"]);
    assert_eq!(names(&plan.library.keywords[0].children), ["NYC", "Paris"]);
    assert!(
        !plan.report.iter().any(|r| r.contains("keyword")),
        "{:?}",
        plan.report
    );
    assert_eq!(inspect(&f.catalog).unwrap().keywords, 7);
}

#[test]
fn unnamed_non_root_keyword_is_skipped_with_a_diagnostic() {
    let (_dir, f) = setup();
    let c = Connection::open(&f.catalog).unwrap();
    c.execute_batch("INSERT INTO AgLibraryKeyword VALUES(200,NULL,1),(201,'Brooklyn',200);")
        .unwrap();
    drop(c);
    let plan = import(&f.catalog).unwrap();
    let places = &plan.library.keywords[0];
    // The unnamed keyword is dropped; its child keeps its place under the parent.
    assert_eq!(names(&places.children), ["NYC", "Paris", "Brooklyn"]);
    assert!(
        plan.report
            .iter()
            .any(|r| r.contains("keyword 200") && r.contains("no name")),
        "{:?}",
        plan.report
    );
}

#[test]
fn second_unnamed_root_keyword_is_reported() {
    let (_dir, f) = setup();
    let c = Connection::open(&f.catalog).unwrap();
    c.execute_batch("INSERT INTO AgLibraryKeyword VALUES(300,NULL,NULL),(301,'Orphans',300);")
        .unwrap();
    drop(c);
    let plan = import(&f.catalog).unwrap();
    assert_eq!(
        names(&plan.library.keywords),
        ["Places", "People", "Trips", "Orphans"]
    );
    assert!(
        plan.report
            .iter()
            .any(|r| r.contains("keyword 300") && r.contains("second unnamed root")),
        "{:?}",
        plan.report
    );
    assert!(
        !plan
            .report
            .iter()
            .any(|r| r.contains(&format!("keyword {}", fixture::KEYWORD_ROOT)))
    );
}

#[test]
fn collection_rows_with_missing_or_unknown_fields_degrade() {
    let (_dir, f) = setup();
    let c = Connection::open(&f.catalog).unwrap();
    c.execute_batch(
        "INSERT INTO AgLibraryCollection VALUES
          (20,NULL,NULL,'com.adobe.ag.library.collection'),
          (21,'No kind',NULL,NULL),
          (22,'Slideshow',NULL,'com.adobe.ag.slideshow.unsaved'),
          (23,'Group',NULL,'com.adobe.ag.library.group');
         INSERT INTO AgLibraryCollectionImage VALUES(20,31,1);",
    )
    .unwrap();
    drop(c);
    let plan = import(&f.catalog).unwrap();
    let lib = &plan.library;
    // A NULL-name collection keeps its membership under a placeholder name.
    let untitled = lib.albums.values().find(|a| a.id == 20).unwrap();
    assert_eq!(untitled.images.len(), 1);
    assert!(
        plan.report
            .iter()
            .any(|r| r.contains("collection 20") && r.contains("no name"))
    );
    // Unknown or missing kinds are skipped, not guessed.
    assert!(lib.albums.values().all(|a| a.id != 21 && a.id != 22));
    assert!(plan.report.iter().any(|r| r.contains("collection 21")));
    assert!(
        plan.report
            .iter()
            .any(|r| r.contains("collection 22") && r.contains("slideshow"))
    );
    // `library.group` is Lightroom's collection set.
    assert!(
        lib.album_groups
            .iter()
            .any(|g| g.id == 23 && g.name == "Group")
    );
}

#[test]
fn empty_develop_row_imports_as_unedited() {
    let (_dir, f) = setup();
    let c = Connection::open(&f.catalog).unwrap();
    c.execute_batch(
        "UPDATE Adobe_imageDevelopSettings SET text='', processVersion=NULL WHERE image=32;",
    )
    .unwrap();
    drop(c);
    let plan = import(&f.catalog).unwrap();
    let img = plan.images.iter().find(|i| i.catalog_id == 32).unwrap();
    assert_eq!(img.recipe.settings.tone.exposure, 0.0);
}

#[test]
fn untranslatable_smart_collection_is_reported_not_fatal() {
    let (_dir, f) = setup();
    let c = Connection::open(&f.catalog).unwrap();
    c.execute_batch(
        r#"INSERT INTO AgLibraryCollection VALUES
            (30,'Recent',NULL,'com.adobe.ag.library.smart_collection'),
            (31,'No rules',NULL,'com.adobe.ag.library.smart_collection');
           INSERT INTO AgLibraryCollectionContent VALUES
            (30,'s = { { criteria = "captureTime", operation = "inLast", value = 1, value_units = "months", }, combine = "intersect", }');"#,
    )
    .unwrap();
    drop(c);
    let plan = import(&f.catalog).unwrap();
    let ids: Vec<i64> = plan.library.smart_albums.iter().map(|s| s.id).collect();
    assert_eq!(ids, [3, 5]);
    assert!(
        plan.report
            .iter()
            .any(|r| r.contains("smart collection 30") && r.contains("unknown rule field"))
    );
    assert!(
        plan.report
            .iter()
            .any(|r| r.contains("smart collection 31") && r.contains("no rules"))
    );
}
