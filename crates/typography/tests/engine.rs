use typography::*;

fn renderer() -> TextRenderer {
    let mut renderer = TextRenderer::new();
    renderer
        .fonts_mut()
        .load_font_data(include_bytes!("fonts/NotoSans-Regular.ttf").to_vec());
    renderer
}

#[test]
fn layout_is_deterministic_and_ligatures_are_live() {
    let r = renderer();
    let mut text = TextModel::point("office", "Noto Sans", 24.0);
    let a = r.layout(&text).unwrap();
    assert_eq!(a, r.layout(&text).unwrap());
    assert_eq!(a.glyphs.len(), 4);
    text.runs[0].features.insert("liga".into(), 0);
    assert_eq!(r.layout(&text).unwrap().glyphs.len(), 6);
    assert!(a.lines[0].width > 0.0);
}

#[test]
fn engine_data_roundtrip_and_version_guard() {
    let mut text = TextModel::point("office", "Noto Sans", 24.0);
    text.runs[0].features.insert("liga".into(), 0);
    text.runs[0].axes.insert("wght".into(), 550.0);
    text.runs[0].baseline_shift = 2.0;
    let json = text.to_engine_data().unwrap();
    assert_eq!(TextModel::from_engine_data(&json).unwrap(), text);
    assert!(TextModel::from_engine_data(&json.replace("\"version\":1", "\"version\":99")).is_err());
    text.runs[0].size = f32::NAN;
    assert!(text.to_engine_data().is_err());
}
