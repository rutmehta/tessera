//! Only generated SQLite and radial pixels; no external catalog/RAW fixtures.
#[test]
fn synthetic_catalog_ca_reaches_cpu_render() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = import_lrcat::fixture::write(dir.path()).unwrap();
    let db = rusqlite::Connection::open(&fixture.catalog).unwrap();
    db.execute(
        "UPDATE Adobe_imageDevelopSettings SET text=?1, processVersion='5.7'",
        ["s = { ChromaticAberrationR = 100, ChromaticAberrationB = -100, AutoLateralCA = 0 }"],
    )
    .unwrap();
    drop(db);
    let plan = import_lrcat::import(&fixture.catalog).unwrap();
    let recipe = &plan
        .images
        .iter()
        .find(|i| serde_json::to_value(&i.recipe.settings.lens).unwrap()["legacy_ca_red"] == 100.)
        .expect("catalog coefficients must translate")
        .recipe;
    let mut settings = recipe.settings.clone();
    settings.lens.profile = engine_api::recipe::settings::LensProfileSource::None;
    settings.detail.sharpening.amount = 0.;
    settings.detail.noise_reduction.color = 0.;
    let n = 129usize;
    let radial: Vec<f32> = (0..n * n)
        .map(|i| ((i % n) as f64 - 64.).hypot((i / n) as f64 - 64.) as f32 / 128.)
        .collect();
    let image = pipeline_cpu::Image::new(n as u32, n as u32, vec![radial; 3]).unwrap();
    let actual =
        pipeline_cpu::render_linear_scaled(&settings, &pipeline_cpu::RenderSource::Rgb(&image), 1)
            .unwrap();
    // Independently construct the analytically scaled radial input, then run
    // the same downstream colour/tone pipeline with the CA fields absent.
    let expected_planes = [1.01, 1., 0.99]
        .into_iter()
        .map(|scale| {
            (0..n * n)
                .map(|i| (((i % n) as f64 - 64.).hypot((i / n) as f64 - 64.) * scale / 128.) as f32)
                .collect()
        })
        .collect();
    let expected_input = pipeline_cpu::Image::new(n as u32, n as u32, expected_planes).unwrap();
    let mut value = serde_json::to_value(&settings).unwrap();
    value["lens"]
        .as_object_mut()
        .unwrap()
        .remove("legacy_ca_red");
    value["lens"]
        .as_object_mut()
        .unwrap()
        .remove("legacy_ca_blue");
    let baseline = serde_json::from_value(value).unwrap();
    let expected = pipeline_cpu::render_linear_scaled(
        &baseline,
        &pipeline_cpu::RenderSource::Rgb(&expected_input),
        1,
    )
    .unwrap();
    let mut max_error = 0f32;
    for c in 0..3 {
        for y in 16..113 {
            for x in 16..113 {
                if (x as f64 - 64.).hypot(y as f64 - 64.) < 8. {
                    continue;
                }
                max_error = max_error
                    .max((actual.planes()[c][y * n + x] - expected.planes()[c][y * n + x]).abs());
            }
        }
    }
    eprintln!("LR-7b import/render maximum error {max_error:.8}; tolerance 0.001");
    assert!(max_error < 0.001, "{max_error}");
}

#[test]
fn lr7d_ffi_mode_change_clears_saved_matrix() {
    use tessera_ffi::{Engine, ImageQuery};
    let dir = tempfile::tempdir().unwrap();
    let photos = dir.path().join("photos");
    std::fs::create_dir(&photos).unwrap();
    image::RgbImage::new(16, 16)
        .save(photos.join("synthetic.png"))
        .unwrap();
    let engine = Engine::open(dir.path().join("db").to_string_lossy().into_owned()).unwrap();
    engine
        .index_folder(photos.to_string_lossy().into_owned())
        .unwrap();
    let row = engine.list_images(ImageQuery::default()).unwrap().remove(0);
    let session = engine.open_develop_session(row.id).unwrap();
    session.set_settings(r#"{"geometry":{"upright":{"mode":"auto","homography_mode":"auto","homography":[[1,0,0],[0,1,0],[0.2,0,1]]}}}"#.into(), false).unwrap();
    session
        .set_settings(r#"{"geometry":{"upright":{"mode":"level"}}}"#.into(), false)
        .unwrap();
    let r: serde_json::Value = serde_json::from_str(&session.get_settings_json().unwrap()).unwrap();
    assert!(r["geometry"]["upright"]["homography"].is_null());
}
