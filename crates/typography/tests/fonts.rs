use typography::*;
#[test]
fn user_font_directory_is_loaded_and_postscript_names_resolve() {
    let mut r = TextRenderer::new();
    assert_eq!(r.fonts().faces().count(), 0);
    r.load_font_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fonts"));
    assert!(r.fonts().faces().count() >= 2);
    let text = TextModel::point("abc", "NotoSans-Regular", 20.0);
    assert_eq!(r.layout(&text).unwrap().glyphs.len(), 3);
}
#[cfg(target_os = "macos")]
#[test]
fn macos_system_discovery_finds_fonts() {
    let mut r = TextRenderer::new();
    r.discover_system_fonts();
    assert!(r.fonts().faces().count() > 0);
}
#[test]
fn kerning_toggle_changes_pair_advance() {
    let mut r = TextRenderer::new();
    r.fonts_mut()
        .load_font_data(include_bytes!("fonts/NotoSans-Regular.ttf").to_vec());
    let mut text = TextModel::point("AV", "Noto Sans", 50.0);
    let kerned = r.layout(&text).unwrap().lines[0].width;
    text.runs[0].kerning = false;
    assert!(r.layout(&text).unwrap().lines[0].width > kerned);
}
