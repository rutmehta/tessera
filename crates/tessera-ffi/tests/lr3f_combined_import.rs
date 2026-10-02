//! Pre-stack integration: only a generated catalog and in-memory RGB pixels.
use engine_api::recipe::{history::Author, required_schema_version};
use import_lrcat::diagnostics;
use pipeline_cpu::{Image, LensContext, RenderSource, render_linear_scaled_with_lens};
use std::sync::Arc;

#[test]
fn lr3f_combined_import_preserves_history_diagnostics_and_spot_exterior() {
    let points = include_str!("../../import-lrcat/tests/data/point-color.lua");
    let row = points.replacen(
        "s = {",
        r#"s = {
        Sharpness = 0, LuminanceSmoothing = 0, ColorNoiseReduction = 0,
        RetouchInfo = {
            {centerX=0.25,centerY=0.3333333333,radius=0.04,sourceX=0.75,sourceY=0.3333333333,spotType='heal',opacity=1,feather=0},
            {centerX=0.25,centerY=0.6666666667,radius=0.04,sourceX=0.75,sourceY=0.6666666667,spotType='clone',opacity=1,feather=0}
        },
        ConvertToGrayscale = true, GrayMixerRed = 20,
        PerspectiveUpright = 1,
        UprightTransform_1 = "1,0,0,0,1,0,0,0,1",
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
        rusqlite::params![&row, id],
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
    assert_eq!(recipe.history.entries[0].meta.label, "Import XMP");
    assert_eq!(
        recipe.history.entries[0].meta.author,
        Author::Import {
            source: "xmp".into()
        }
    );
    assert_eq!(recipe.settings.locals.retouch.len(), 2);
    assert!(matches!(
        recipe.settings.locals.retouch[0].kind,
        engine_api::recipe::mask::RetouchKind::Heal { .. }
    ));
    assert!(matches!(
        recipe.settings.locals.retouch[1].kind,
        engine_api::recipe::mask::RetouchKind::Clone { .. }
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
        ("RetouchInfo", "LR-3"),
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
        if let Some(field) = entry.field.as_deref() {
            assert!(
                json.pointer(field).is_some_and(|v| !v.is_null()),
                "{entry:?}"
            );
        }
    }
    assert!(!recipe.unknown.contains_key("lrcat_develop_diagnostics"));
    // Spatial operators are disabled in the row so the exact spot footprints
    // are the only changed pixels; all imported color/mask stages still run.
    let (w, h) = (128usize, 96usize);
    let planes = (0..3)
        .map(|c| {
            (0..w * h)
                .map(|i| 0.05 + ((i * 17 + (i / w) * 13 + c * 29) % 97) as f32 / 140.)
                .collect()
        })
        .collect();
    let source = Image::new(w as u32, h as u32, planes).unwrap();
    let context = LensContext {
        retouch: Some(Arc::new(brush::render_retouch)),
        ..Default::default()
    };
    let rendered =
        render_linear_scaled_with_lens(&recipe.settings, &RenderSource::Rgb(&source), 1, &context)
            .unwrap();
    let spot_start = row.find("        RetouchInfo = ").unwrap();
    let spot_end = row.find("        ConvertToGrayscale").unwrap();
    let mut row_without_spots = row.clone();
    row_without_spots.replace_range(spot_start..spot_end, "");
    let (baseline_recipe, _) =
        import_lrcat::lua_develop::parse(&row_without_spots, "15.4").unwrap();
    let mut expected = recipe.settings.clone();
    expected.locals.retouch.clear();
    assert_eq!(baseline_recipe.settings, expected);
    let baseline = render_linear_scaled_with_lens(
        &baseline_recipe.settings,
        &RenderSource::Rgb(&source),
        1,
        &context,
    )
    .unwrap();
    assert_eq!(rendered.width(), baseline.width());
    assert_eq!(rendered.height(), baseline.height());
    let mut changed = [0usize; 2];
    let mut exterior = 0;
    for y in 0..h {
        for x in 0..w {
            // Radius .04 of width plus the rasterizer's half-pixel edge.
            let spot = [32., 64.].iter().position(|cy| {
                ((x as f32 + 0.5 - 32.).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt() <= 6.
            });
            for (c, (after, before)) in rendered.planes().iter().zip(baseline.planes()).enumerate()
            {
                let a = after[y * w + x];
                let b = before[y * w + x];
                assert!(a.is_finite());
                if let Some(s) = spot {
                    changed[s] += usize::from(a.to_bits() != b.to_bits());
                } else {
                    assert_eq!(a.to_bits(), b.to_bits(), "exterior ({x},{y}) channel {c}");
                    exterior += 1;
                }
            }
        }
    }
    eprintln!(
        "LR-3f exterior samples identical: {exterior}; changed heal/clone samples: {changed:?}"
    );
    assert!(exterior > w * h * 2);
    assert!(
        changed.into_iter().all(|n| n > 0),
        "both heal and clone must change pixels"
    );
}
