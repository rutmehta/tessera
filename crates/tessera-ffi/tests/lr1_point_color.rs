//! Synthetic Lua develop row -> shared importer -> recipe -> CPU color stage.
use engine_api::tile::TileCoord;
#[test]
fn synthetic_lua_point_color_renders_expected_pixels() {
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
        rusqlite::params![
            include_str!("../../import-lrcat/tests/data/point-color.lua"),
            id
        ],
    )
    .unwrap();
    drop(db);
    let plan = import_lrcat::import(&fixture.catalog).unwrap();
    let recipe = &plan
        .images
        .iter()
        .find(|image| image.catalog_id == id)
        .unwrap()
        .recipe;
    assert_eq!(recipe.settings.color.point_colors.len(), 1);
    recipe.validate().unwrap();
    assert!(!recipe.unknown.contains_key("lrcat_develop_source"));
    pipeline_cpu::validate_settings(&recipe.settings).unwrap();
    let mut tile = pipeline_cpu::Image::new(1, 1, vec![vec![0.75], vec![0.25], vec![0.25]])
        .unwrap()
        .tile(TileCoord::new(0, 0, 0), 0, 1)
        .unwrap();
    pipeline_cpu::color(&mut tile, &recipe.settings.color).unwrap();
    let source = pipeline_cpu::Image::new(1, 1, vec![vec![0.75], vec![0.25], vec![0.25]]).unwrap();
    let rendered = pipeline_cpu::render_linear_scaled(
        &recipe.settings,
        &pipeline_cpu::RenderSource::Rgb(&source),
        1,
    )
    .unwrap();
    for (plane, expected) in rendered.planes().iter().zip([0.75, 0.5, 0.25]) {
        assert!((plane[0] - expected).abs() < 2e-6);
    }
    for (a, b) in tile.samples::<f32>().unwrap().iter().zip([0.75, 0.5, 0.25]) {
        assert!((a - b).abs() < 2e-6);
    }
}
