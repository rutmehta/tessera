//! Pre-stack integration: only a generated catalog and in-memory RGB pixels.
use engine_api::recipe::{history::Author, required_schema_version};
use import_lrcat::diagnostics;
use pipeline_cpu::{Image, RenderSource, render_linear_scaled};

#[test]
fn lr4f_combined_import_has_one_history_entry_all_diagnostics_and_cpu_pixels() {
    let points = include_str!("../../import-lrcat/tests/data/point-color.lua");
    let row = points.replacen(
        "s = {",
        r#"s = {
        ConvertToGrayscale = true, GrayMixerRed = 20,
        PerspectiveUpright = 1,
        UprightTransform_1 = "1,0,0,0,1,0,0.2,0,1",
        MaskGroupBasedCorrections = {{ LocalExposure2012 = 1,
            CorrectionMasks = {{ What = "Mask/Group", Masks = {
                { What = "Mask/RangeMask", CorrectionRangeMask = {
                    Type = 2, LumRange = "0.1 0.2 0.8 0.9"
                }}
            }}}
        }},"#,
        1,
    );
    let dir = tempfile::tempdir().unwrap();
    let fixture = import_lrcat::fixture::write(dir.path()).unwrap();
    let db = rusqlite::Connection::open(&fixture.catalog).unwrap();
    let id: i64 = db
        .query_row(
            "SELECT image FROM Adobe_imageDevelopSettings ORDER BY image LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    db.execute(
        "UPDATE Adobe_imageDevelopSettings SET text=?1, processVersion='15.4' WHERE image=?2",
        rusqlite::params![row, id],
    )
    .unwrap();
    drop(db);
    let plan = import_lrcat::import(&fixture.catalog).unwrap();
    let recipe = &plan
        .images
        .iter()
        .find(|i| i.catalog_id == id)
        .unwrap()
        .recipe;
    recipe.validate().unwrap();
    assert_eq!(recipe.history.entries.len(), 1);
    assert!(matches!(
        recipe.history.entries[0].meta.author,
        Author::Import { .. }
    ));
    assert_eq!(required_schema_version(recipe), 4);
    assert_eq!(recipe.settings.color.point_colors.len(), 1);
    assert!(recipe.settings.color.monochrome.as_ref().unwrap().enabled);
    assert!(recipe.settings.geometry.upright.homography.is_some());
    let json = serde_json::to_value(recipe).unwrap();
    assert_eq!(json["schema_version"], 4);
    let nested = &json["settings"]["locals"]["adjustments"][0]["components"][0]["group"][0];
    assert_eq!(nested["kind"], "luminance_range");
    assert_eq!(nested["luminance_domain"], "display");
    assert_eq!(nested["luminance_bounds"].as_array().unwrap().len(), 4);

    let notes = diagnostics::entries(recipe);
    for (key, lane) in [
        ("PointColors", "LR-1"),
        ("ConvertToGrayscale", "LR-2"),
        ("GrayMixerRed", "LR-2"),
        ("MaskGroupBasedCorrections", "LR-4"),
        (
            "MaskGroupBasedCorrections/CorrectionRangeMask/LumRange",
            "LR-4",
        ),
        ("UprightTransform_1", "LR-7"),
    ] {
        assert!(
            notes.get(key).is_some_and(|entries| entries
                .iter()
                .any(|e| e.lane == lane && e.status == "approximate" && e.level == "info")),
            "missing {lane} diagnostic for {key}: {notes:?}"
        );
    }
    for entry in notes.values().flatten() {
        if let Some(field) = &entry.field {
            assert!(
                json.pointer(field).is_some_and(|v| !v.is_null()),
                "{entry:?}"
            );
        }
    }
    assert!(!recipe.unknown.contains_key("lrcat_develop_diagnostics"));
    let source = Image::new(
        16,
        12,
        [0.52, 0.05, 0.05].map(|v| vec![v; 16 * 12]).to_vec(),
    )
    .unwrap();
    let rendered = render_linear_scaled(&recipe.settings, &RenderSource::Rgb(&source), 1).unwrap();
    assert!(rendered.planes().iter().flatten().all(|v| v.is_finite()));
    assert!(rendered.planes()[0].iter().any(|v| *v > 0.));
    let mut without_masks = recipe.settings.clone();
    without_masks.locals.adjustments.clear();
    let baseline = render_linear_scaled(&without_masks, &RenderSource::Rgb(&source), 1).unwrap();
    assert!(
        rendered.planes()[0]
            .iter()
            .zip(&baseline.planes()[0])
            .any(|(a, b)| (a - b).abs() > 1e-3)
    );
}
