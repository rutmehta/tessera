//! LR-8m (A-LR8 M8): an imported Smart Preview exports like an ordinary RAW.
//! Lens corrections, crop, Upright and masks run in the sensor frame; the
//! catalog orientation is applied once, at the end, as EXIF is for a RAW.
#[allow(dead_code)]
#[path = "../../raw-decode/tests/support/mod.rs"]
mod support;

use engine_api::{
    jobs::CancellationToken,
    recipe::{EditMeta, LocalAdjustment, LocalParams, MaskComponent, MaskKind, Recipe},
};
use export::*;
use pipeline_cpu::{CameraLinearProxy, RenderSource};

fn orient(rgb: image::Rgb32FImage, orientation: u16) -> image::Rgb32FImage {
    use image::imageops::*;
    match orientation {
        2 => flip_horizontal(&rgb),
        3 => rotate180(&rgb),
        4 => flip_vertical(&rgb),
        5 => rotate90(&flip_vertical(&rgb)),
        6 => rotate90(&rgb),
        7 => rotate90(&flip_horizontal(&rgb)),
        8 => rotate270(&rgb),
        _ => rgb,
    }
}

#[test]
fn lr8m_proxy_export_orients_once_after_sensor_frame_edits_and_lens() {
    let mut dng =
        raw_decode::lossy_dng::read(&mut std::io::Cursor::new(support::lossy_dng(false, false)))
            .unwrap()
            .unwrap();
    // A spatial gradient so any wrong rotation or reflection changes pixels.
    let (w, h) = (64usize, 48usize);
    dng.width = w;
    dng.height = h;
    dng.metadata.width = w as u32;
    dng.metadata.height = h as u32;
    dng.metadata.default_crop = [0, 0, w as u32, h as u32];
    dng.pixels = (0..h)
        .flat_map(|y| (0..w).map(move |x| [0.02 + x as f32 / 400., 0.03 + y as f32 / 300., 0.1]))
        .collect();
    let proxy = CameraLinearProxy::from_dng(dng).unwrap();
    let mut recipe = Recipe::default();
    recipe
        .edit(EditMeta::user("LR-8m frame edits", 1), |s| {
            s.detail.sharpening.amount = 0.;
            s.detail.noise_reduction.color = 0.;
            s.geometry.crop.rect.left = 0.125;
            s.geometry.crop.rect.right = 0.8125;
            s.geometry.crop.rect.top = 0.25;
            s.geometry.crop.rect.bottom = 0.9375;
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
        })
        .unwrap();
    let request = RenderRequest {
        color_space: ColorSpace::Srgb,
        resize: Resize::None,
        sharpen_for: SharpenFor::None,
        scale: 1,
    };
    let render = |proxy: &CameraLinearProxy| {
        let image = ExportImage {
            source: RenderSource::CameraLinear(proxy),
            name: "proxy",
            sequence: 1,
            date: "",
            metadata: None,
        };
        render_pixels(&image, &recipe, &request, &CancellationToken::new(), None).unwrap()
    };
    let sensor = render(&proxy.clone().with_catalog_orientation(1).unwrap());
    for o in 1..=8u16 {
        let actual = render(&proxy.clone().with_catalog_orientation(o).unwrap());
        let expected = orient(sensor.clone(), o);
        assert_eq!(
            actual.dimensions(),
            expected.dimensions(),
            "orientation {o}"
        );
        let error = actual
            .pixels()
            .zip(expected.pixels())
            .flat_map(|(a, b)| a.0.into_iter().zip(b.0).map(|(a, b)| (a - b).abs()))
            .fold(0f32, f32::max);
        assert!(error < 1e-5, "orientation {o}: {error}");
    }
}
