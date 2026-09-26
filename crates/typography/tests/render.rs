use typography::*;
fn renderer() -> TextRenderer {
    let mut r = TextRenderer::new();
    r.fonts_mut()
        .load_font_data(include_bytes!("fonts/NotoSans-Regular.ttf").to_vec());
    r
}
#[test]
fn unhinted_raster_has_premultiplied_coverage_and_vector_export() {
    let r = renderer();
    let mut text = TextModel::point("office", "Noto Sans", 24.0);
    text.runs[0].color = [200, 90, 30, 128];
    let rendered = r.render(&text, 1.0).unwrap();
    assert!(rendered.raster.width > 40);
    assert!(rendered.raster.height > 10);
    assert!(
        rendered
            .raster
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[3] > 0 && p[3] < 128)
    );
    assert!(
        rendered
            .raster
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| p[0] <= p[3] && p[1] <= p[3] && p[2] <= p[3])
    );
    let outlines = r.outlines(&text, &r.layout(&text).unwrap()).unwrap();
    assert_eq!(outlines.len(), 4);
    assert!(outlines.iter().all(|o| o.path.iter().count() > 4));
    let big = r.render(&text, 3.5).unwrap();
    assert!(big.raster.width > rendered.raster.width * 3);
    assert!(r.render(&text, f32::NAN).is_err());
    assert!(r.render(&text, 0.0).is_err());
}

#[test]
fn bundled_font_coverage_checksum() {
    let text = TextModel::point("office", "Noto Sans", 24.0);
    let image = renderer().render(&text, 1.0).unwrap();
    let hash = image
        .raster
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .fold(0xcbf29ce484222325u64, |hash, pixel| {
            (hash ^ u64::from(pixel[3])).wrapping_mul(0x100000001b3)
        });
    assert_eq!(
        hash, 9554203516772678598,
        "coverage fixture: {}x{} {:?}",
        image.raster.width, image.raster.height, image.bounds
    );
}

#[test]
fn variable_axes_affect_both_shaping_and_outlines() {
    let mut r = TextRenderer::new();
    r.fonts_mut()
        .load_font_data(include_bytes!("fonts/NotoSans-Variable.ttf").to_vec());
    let mut text = TextModel::point("Variable", "Noto Sans", 30.0);
    text.runs[0].axes.insert("wght".into(), 100.0);
    text.runs[0].axes.insert("wdth".into(), 100.0);
    let normal = r.render(&text, 1.0).unwrap();
    text.runs[0].axes.insert("wght".into(), 900.0);
    text.runs[0].axes.insert("wdth".into(), 62.5);
    let condensed = r.render(&text, 1.0).unwrap();
    assert_ne!(normal.raster, condensed.raster);
    assert!(condensed.layout.lines[0].width < normal.layout.lines[0].width);
}
