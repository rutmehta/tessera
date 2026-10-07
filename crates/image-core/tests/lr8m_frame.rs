//! LR-8m (A-LR8 M8, M4): one edit frame for Smart Previews, relinked
//! originals and ordinary imports. Crop, masks, Upright and lens corrections
//! are normalized in the sensor (active-area) frame, exactly as for an
//! ordinary RAW. The absolute catalog orientation replaces EXIF as the display
//! orientation and is applied by the caller (host, thumbnail store, export),
//! the same way as an ordinary import's EXIF orientation.
#[allow(dead_code)]
mod common;
#[allow(dead_code)]
#[path = "../../raw-decode/tests/support/mod.rs"]
mod support;

use engine_api::{
    id::ImageId,
    recipe::{
        DevelopSettings, LocalAdjustment, LocalParams, MaskComponent, MaskKind,
        settings::UprightMode,
    },
};
use image_core::{PixelRect, RawImage, Renderer};

/// Crop, Upright, a radial mask and manual lens corrections: every geometric
/// edit that depends on the frame it is normalized in.
fn frame_edits() -> DevelopSettings {
    let mut s = DevelopSettings::default();
    s.detail.sharpening.amount = 0.;
    s.detail.noise_reduction.color = 0.;
    s.geometry.crop.rect.left = 0.125;
    s.geometry.crop.rect.right = 0.8125;
    s.geometry.crop.rect.top = 0.25;
    s.geometry.crop.rect.bottom = 0.9375;
    s.geometry.upright.mode = UprightMode::Full;
    s.geometry.upright.homography = Some([[1., 0.02, 0.01], [0., 1., 0.02], [0.015, 0., 1.]]);
    s.lens.manual_distortion = 12.;
    s.lens.manual_vignetting = 30.;
    s.locals.adjustments.push(LocalAdjustment {
        components: vec![MaskComponent::new(MaskKind::Radial {
            center: [0.7, 0.35],
            radii: [0.22, 0.3],
            angle: 20.,
            feather: 50.,
        })],
        params: LocalParams {
            exposure: 1.,
            ..Default::default()
        },
        ..Default::default()
    });
    s
}

fn pixels(renderer: &Renderer, image: &RawImage, s: &DevelopSettings, level: u8) -> Vec<u8> {
    let extent = Renderer::output_extent(image, s, level).unwrap();
    let tiles = renderer
        .render_region(image, s, level, PixelRect::full(extent))
        .unwrap();
    common::assemble_u8(extent, &tiles)
}

#[test]
fn lr8m_relinked_original_uses_the_ordinary_import_frame_for_every_orientation() {
    let dir = tempfile::tempdir().unwrap();
    // The relinked file's own EXIF disagrees with the catalog: the catalog
    // orientation replaces it and is never composed on top of it.
    let relinked_path = dir.path().join("relinked.dng");
    std::fs::write(&relinked_path, support::bayer_dng(3)).unwrap();
    let renderer = Renderer::new(Default::default());
    let s = frame_edits();
    let upright =
        RawImage::open_with_catalog_orientation(ImageId(7200), &relinked_path, Some(1)).unwrap();
    let sensor_frame = pixels(&renderer, &upright, &s, 0);
    for o in 1..=8u16 {
        let ordinary_path = dir.path().join(format!("ordinary-{o}.dng"));
        std::fs::write(&ordinary_path, support::bayer_dng(o)).unwrap();
        let ordinary = RawImage::open(ImageId(7100 + u128::from(o)), &ordinary_path).unwrap();
        let relinked = RawImage::open_with_catalog_orientation(
            ImageId(7200 + u128::from(o)),
            &relinked_path,
            Some(o),
        )
        .unwrap();
        assert_eq!(ordinary.metadata().orientation, o);
        assert_eq!(
            relinked.metadata().orientation,
            o,
            "catalog orientation is the display orientation, as EXIF is for an ordinary import"
        );
        assert_eq!(
            relinked.active_extent(),
            ordinary.active_extent(),
            "orientation {o}: edits stay in the sensor frame"
        );
        assert_eq!(
            Renderer::output_extent(&relinked, &s, 0).unwrap(),
            Renderer::output_extent(&ordinary, &s, 0).unwrap(),
            "orientation {o}"
        );
        assert_eq!(
            pixels(&renderer, &relinked, &s, 0),
            sensor_frame,
            "orientation {o}: crop, mask, Upright and lens land in the sensor frame"
        );
    }
}

#[test]
fn lr8m_proxy_catalog_orientation_is_display_only() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("proxy.dng");
    // The synthetic LinearRaw Smart Preview carries EXIF orientation 6.
    std::fs::write(&path, support::lossy_dng(false, false)).unwrap();
    let plain = RawImage::open(ImageId(7300), &path).unwrap();
    assert!(plain.camera_linear_proxy().is_some());
    assert_eq!(plain.metadata().orientation, 6);
    let renderer = Renderer::new(Default::default());
    let s = frame_edits();
    let expected = pixels(&renderer, &plain, &s, 0);
    for o in 1..=8u16 {
        let proxy =
            RawImage::open_with_catalog_orientation(ImageId(7300 + u128::from(o)), &path, Some(o))
                .unwrap();
        assert_eq!(proxy.metadata().orientation, o, "orientation {o}");
        assert_eq!(
            proxy
                .camera_linear_proxy()
                .unwrap()
                .original_metadata()
                .orientation,
            o
        );
        assert_eq!(
            proxy.active_extent(),
            plain.active_extent(),
            "orientation {o}"
        );
        assert_eq!(
            Renderer::output_extent(&proxy, &s, 0).unwrap(),
            Renderer::output_extent(&plain, &s, 0).unwrap()
        );
        // Lens corrections resolve against the proxy's sensor frame and run
        // before crop/Upright; orientation never reorders them.
        assert_eq!(
            pixels(&renderer, &proxy, &s, 0),
            expected,
            "orientation {o}: Smart Preview edits stay in the sensor frame"
        );
    }
}
