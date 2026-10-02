//! LR-11b: synthetic catalog/XMP import -> CPU pixels. Scene-linear tolerance 2e-6.
use engine_api::recipe::Recipe;
use pipeline_cpu::{Image, RenderSource, render_linear_scaled};

fn fixture(name: &str, extension: &str) -> String {
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../import-lrcat/tests/data");
    ["lr11", "lr11b"]
        .iter()
        .find_map(|dir| {
            std::fs::read_to_string(data.join(format!("{dir}/{name}.{extension}"))).ok()
        })
        .unwrap()
}
fn catalog_recipe(name: &str) -> Recipe {
    let dir = tempfile::tempdir().unwrap();
    let f = import_lrcat::fixture::write(dir.path()).unwrap();
    let db = rusqlite::Connection::open(&f.catalog).unwrap();
    let id: i64 = db
        .query_row(
            "SELECT image FROM Adobe_imageDevelopSettings ORDER BY image LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    db.execute(
        "UPDATE Adobe_imageDevelopSettings SET text=?1, processVersion='15.4' WHERE image=?2",
        rusqlite::params![fixture(name, "lua"), id],
    )
    .unwrap();
    drop(db);
    let plan = import_lrcat::import(&f.catalog).unwrap();
    let r = plan
        .images
        .iter()
        .find(|i| i.catalog_id == id)
        .unwrap()
        .recipe
        .clone();
    assert_eq!(r.history.entries.len(), 1);
    r
}
fn xmp_recipe(name: &str) -> Recipe {
    let (r, w) = import_lrcat::xmp::parse(&fixture(name, "xmp"), "15.4").unwrap();
    assert!(w.is_empty(), "{w:?}");
    r
}
fn image(pixels: &[[f32; 3]]) -> Image {
    Image::new(
        pixels.len() as u32,
        1,
        (0..3)
            .map(|c| pixels.iter().map(|p| p[c]).collect())
            .collect(),
    )
    .unwrap()
}
fn max_difference(actual: &Image, expected: &[[f32; 3]]) -> f32 {
    let mut worst = 0f32;
    for (i, p) in expected.iter().enumerate() {
        for (c, v) in p.iter().enumerate() {
            worst = worst.max((actual.planes()[c][i] - v).abs());
        }
    }
    worst
}

/// B3: on an SDR image the extended local curve must not replace the ordinary
/// one. The reference is the ordinary curve (halved log1p(v/0.18) coordinate)
/// blended through the 0.75/0.25 gradient alpha.
#[test]
fn b3_sdr_extended_local_curve_renders_the_ordinary_curve() {
    let pixels = [[0.2f32, 0.3, 0.4]; 2];
    let input = image(&pixels);
    let mut expected = pixels;
    for (i, alpha) in [0.75f32, 0.25].into_iter().enumerate() {
        expected[i] = pixels[i].map(|v| v + alpha * (0.18 * ((1. + v / 0.18).sqrt() - 1.) - v));
    }
    for r in [
        catalog_recipe("curve-extended-sdr"),
        xmp_recipe("curve-extended-sdr"),
    ] {
        let local =
            pipeline_cpu::locals_image(&input, &r.settings.locals.adjustments, Default::default())
                .unwrap();
        assert!(max_difference(&local, &expected) < 2e-6);
        let full = render_linear_scaled(&r.settings, &RenderSource::Rgb(&input), 1).unwrap();
        assert!(max_difference(&full, &expected) < 2e-6);
        // The ordinary-only fixture renders the same pixels.
        let ordinary = catalog_recipe("curve");
        assert_eq!(
            pipeline_cpu::locals_image(
                &input,
                &ordinary.settings.locals.adjustments,
                Default::default()
            )
            .unwrap()
            .planes(),
            local.planes()
        );
    }
    // Control: with HDR output the extended curve is honoured and differs.
    for r in [
        catalog_recipe("curve-extended-hdr"),
        xmp_recipe("curve-extended-hdr"),
    ] {
        let local =
            pipeline_cpu::locals_image(&input, &r.settings.locals.adjustments, Default::default())
                .unwrap();
        assert!(max_difference(&local, &expected) > 1e-3);
    }
}
