//! Profile dispatch is explicit in the recipe; native pixels remain independent.
use engine_api::{
    id::ImageId,
    recipe::{DevelopSettings, ProcessVersion},
};
use image_core::{PixelRect, RawImage, Renderer, RendererConfig};
#[allow(dead_code)]
#[path = "../../raw-decode/tests/support/mod.rs"]
mod support;

fn pixels(renderer: &Renderer, image: &RawImage, settings: &DevelopSettings) -> Vec<u8> {
    let extent = Renderer::output_extent(image, settings, 0).unwrap();
    renderer
        .render_region(image, settings, 0, PixelRect::full(extent))
        .unwrap()
        .iter()
        .flat_map(|t| t.samples::<u8>().unwrap().to_vec())
        .collect()
}

#[test]
fn lr10_embedded_linear_raw_matches_explicit_profile_and_named_adobe_dispatch() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("synthetic.dng");
    let bytes = support::lossy_dng(false, false);
    std::fs::write(&path, &bytes).unwrap();
    let image = RawImage::open(ImageId(810), &path).unwrap();
    let mut settings = DevelopSettings::default();
    settings.detail.sharpening.amount = 0.;
    settings.detail.noise_reduction.color = 0.;
    let config = RendererConfig {
        process_version: ProcessVersion::adobe(6),
        ..Default::default()
    };
    let explicit = Renderer::new(config.clone())
        .with_dcp_profile(&bytes)
        .unwrap();
    let expected = pixels(&explicit, &image, &settings);
    assert_eq!(pixels(&Renderer::new(config), &image, &settings), expected);
    settings.camera_profile.profile.name = "Adobe Color".into();
    assert_eq!(
        pixels(&Renderer::new(Default::default()), &image, &settings),
        expected
    );
    settings.camera_profile.profile.name.0.clear();
    let native = Renderer::new(Default::default());
    let before = pixels(&native, &image, &settings);
    let supplied = native.with_dcp_profile(&bytes).unwrap();
    assert_eq!(pixels(&supplied, &image, &settings), before);
    assert_ne!(before, expected);
}
