use merge::{LinearImage, auto_recipe, recipe_xmp};
#[test]
fn recipe_is_editable_and_embedded_in_float_dng() {
    let im = LinearImage {
        width: 2,
        height: 2,
        pixels: vec![[2.; 3]; 4],
        color_matrix: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
        as_shot_neutral: [0.5, 1., 0.75],
    };
    let mut recipe = auto_recipe(&im).unwrap();
    assert!(recipe.settings.tone.exposure < 0.);
    assert!(recipe.settings.tone.highlights < 0.);
    recipe
        .unknown
        .insert("note".into(), serde_json::json!("<keep & editable>"));
    let xmp = recipe_xmp(&recipe).unwrap();
    assert!(xmp.contains("&lt;keep &amp; editable&gt;"));
    let mut b = Vec::new();
    merge::dng::write(&mut b, &im, &xmp).unwrap();
    let decoded = raw_decode::linear_dng::read(&mut std::io::Cursor::new(b)).unwrap();
    assert_eq!(decoded.pixels, im.pixels);
    assert_eq!(decoded.xmp, xmp);
    let text = xmp
        .split("<ts:Recipe>")
        .nth(1)
        .unwrap()
        .split("</ts:Recipe>")
        .next()
        .unwrap()
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&");
    let parsed: engine_api::recipe::Recipe = serde_json::from_str(&text).unwrap();
    assert_eq!(parsed, recipe);
}
