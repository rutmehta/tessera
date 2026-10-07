mod common;
use engine_api::{
    id::ImageId,
    recipe::{DevelopSettings, ProcessVersion},
};
use image_core::{PixelRect, RawImage, Renderer, RendererConfig};
use std::sync::Arc;

fn render(baseline: f32, exposure: f32, version: ProcessVersion) -> Vec<u8> {
    let mut m = common::metadata(32, 24, common::RGGB, [0, 0, 32, 24]);
    m.baseline_exposure = baseline;
    let raw = raw_decode::CfaImage::from_linear(32, 24, vec![0.08; 32 * 24]).unwrap();
    let image = RawImage::new(ImageId(84), Arc::new(raw), Arc::new(m)).unwrap();
    let renderer = Renderer::new(RendererConfig {
        process_version: version,
        ..Default::default()
    });
    let mut s = DevelopSettings::default();
    s.detail.sharpening.amount = 0.;
    s.detail.noise_reduction.color = 0.;
    s.tone.exposure = exposure;
    let extent = Renderer::output_extent(&image, &s, 0).unwrap();
    common::assemble_u8(
        extent,
        &renderer
            .render_region(&image, &s, 0, PixelRect::full(extent))
            .unwrap(),
    )
}
#[test]
fn native_baseline_metadata_does_not_change_pixels() {
    assert_eq!(
        render(0., 0., ProcessVersion::NATIVE_CURRENT),
        render(0.75, 0., ProcessVersion::NATIVE_CURRENT)
    );
}
#[test]
fn adobe_adds_baseline_and_user_exposure_once() {
    let version = ProcessVersion::adobe(6);
    assert_eq!(render(0.75, -0.25, version), render(0., 0.5, version));
    assert_ne!(render(0.75, -0.25, version), render(0., 0., version));
}

#[test]
fn adobe_rejects_underflowing_baseline_gain_without_a_profile() {
    let mut m = common::metadata(32, 24, common::RGGB, [0, 0, 32, 24]);
    m.baseline_exposure = -150.;
    let raw = raw_decode::CfaImage::from_linear(32, 24, vec![0.08; 32 * 24]).unwrap();
    let image = RawImage::new(ImageId(85), Arc::new(raw), Arc::new(m)).unwrap();
    let renderer = Renderer::new(RendererConfig {
        process_version: ProcessVersion::adobe(6),
        ..Default::default()
    });
    assert!(
        renderer
            .render_region(
                &image,
                &DevelopSettings::default(),
                0,
                PixelRect::full(image.active_extent())
            )
            .is_err()
    );
}
