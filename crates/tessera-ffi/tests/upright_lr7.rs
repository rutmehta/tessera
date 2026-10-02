//! Synthetic catalog -> recipe -> CPU geometry, using existing dependencies only.
#[test]
fn catalog_upright_renders_known_projective_corners_and_reports_cloud_features() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = import_lrcat::fixture::write(dir.path()).unwrap();
    let db = rusqlite::Connection::open(&fixture.catalog).unwrap();
    let changed_rows = db.execute("UPDATE Adobe_imageDevelopSettings SET text=?1, processVersion='15.4'", ["s = { PerspectiveUpright = 1, UprightTransform_1 = '1,0,0,0,1,0,0.2,0,1', EnableDistractionRemoval = true, FilterList={{What='synthetic-filter'}} }"]).unwrap();
    drop(db);
    let plan = import_lrcat::import(&fixture.catalog).unwrap();
    assert!(changed_rows > 0);
    assert_eq!(
        plan.images
            .iter()
            .filter(|image| image.recipe.settings.geometry.upright.mode
                == engine_api::recipe::settings::UprightMode::Auto)
            .count(),
        changed_rows
    );
    // Images with no Develop row must not gain a cloud note.
    assert!(plan.images.iter().all(|image| {
        import_lrcat::diagnostics::entries(&image.recipe)
            .values()
            .flatten()
            .filter(|note| {
                note.status == "ignored"
                    && note
                        .reason
                        .contains("requires Adobe cloud; not translatable")
            })
            .count()
            == usize::from(
                image.recipe.settings.geometry.upright.mode
                    == engine_api::recipe::settings::UprightMode::Auto,
            )
    }));
    let recipe = &plan
        .images
        .iter()
        .find(|i| {
            i.recipe.settings.geometry.upright.mode
                == engine_api::recipe::settings::UprightMode::Auto
        })
        .unwrap()
        .recipe;
    // Coordinate ramp measures inverse-mapped source locations. The four
    // interior corners form a rectangle at x/y = .25/.75 in the output.
    let n = 128usize;
    let x: Vec<f32> = (0..n * n).map(|i| (i % n) as f32).collect();
    let y: Vec<f32> = (0..n * n).map(|i| (i / n) as f32).collect();
    let input = pipeline_cpu::Image::new(n as u32, n as u32, vec![x, y, vec![1.; n * n]]).unwrap();
    let output = pipeline_cpu::geometry(&input, &recipe.settings.geometry).unwrap();
    let mut max_error = 0f64;
    for (x, y) in [(32, 32), (96, 32), (96, 96), (32, 96)] {
        // Independent inverse of u=x/(1+.2*x), v=y/(1+.2*x).
        let u = (x as f64 + 0.5) / 128.;
        let v = (y as f64 + 0.5) / 128.;
        let expected = [
            128. * u / (1. - 0.2 * u) - 0.5,
            128. * v / (1. - 0.2 * u) - 0.5,
        ];
        for (c, want) in expected.into_iter().enumerate() {
            max_error = max_error.max((output.planes()[c][y * n + x] as f64 - want).abs());
        }
    }
    eprintln!("LR-7 maximum corner coordinate error: {max_error:.6} px (tolerance 0.08 px)");
    assert!(max_error < 0.08, "{max_error}");
}
