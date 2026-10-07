//! Catalog orientation is a display orientation (LR-8m, A-LR8 M8): edits are
//! normalized in the sensor frame for proxies, relinked and ordinary RAWs.
#[allow(dead_code)]
#[path = "../../raw-decode/tests/support/mod.rs"]
mod support;
use engine_api::recipe::{DevelopSettings, LocalAdjustment, LocalParams, MaskComponent, MaskKind};
use pipeline_cpu::{CameraLinearProxy, RenderSource};

#[test]
fn all_eight_catalog_orientations_keep_crop_masks_upright_and_lens_in_the_sensor_frame() {
    let mut dng =
        raw_decode::lossy_dng::read(&mut std::io::Cursor::new(support::lossy_dng(false, false)))
            .unwrap()
            .unwrap();
    // A spatial gradient prevents an incorrect reflection/rotation from passing.
    dng.width = 64;
    dng.height = 48;
    dng.metadata.width = 64;
    dng.metadata.height = 48;
    dng.metadata.default_crop = [0, 0, 64, 48];
    dng.metadata.orientation = 1;
    dng.pixels = (0..48)
        .flat_map(|y| (0..64).map(move |x| [0.02 + x as f32 / 400., 0.03 + y as f32 / 300., 0.1]))
        .collect();
    let proxy = CameraLinearProxy::from_dng(dng).unwrap();
    let mut neutral = DevelopSettings::default();
    neutral.detail.sharpening.amount = 0.;
    neutral.detail.noise_reduction.color = 0.;
    for o in 1..=8 {
        let oriented = proxy.clone().with_catalog_orientation(o).unwrap();
        assert_eq!(
            oriented.original_metadata().orientation,
            o,
            "the catalog orientation replaces EXIF as the display orientation"
        );
        let mut edits = neutral.clone();
        edits.geometry.crop.rect.left = 0.125;
        edits.geometry.crop.rect.right = 0.875;
        edits.geometry.crop.rect.top = 0.25;
        edits.geometry.crop.rect.bottom = 0.875;
        edits.geometry.upright.mode = engine_api::recipe::settings::UprightMode::Full;
        edits.geometry.upright.homography =
            Some([[1., 0.02, 0.01], [0., 1., 0.02], [0.015, 0., 1.]]);
        edits.lens.manual_distortion = 12.;
        edits.lens.manual_vignetting = 30.;
        edits.locals.adjustments.push(LocalAdjustment {
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
        let actual =
            pipeline_cpu::render_linear_scaled(&edits, &RenderSource::CameraLinear(&oriented), 1)
                .unwrap();
        // The same edits on the unrotated proxy: orientation never moves them.
        let expected =
            pipeline_cpu::render_linear_scaled(&edits, &RenderSource::CameraLinear(&proxy), 1)
                .unwrap();
        assert_eq!(
            (actual.width(), actual.height()),
            (expected.width(), expected.height()),
            "orientation {o}"
        );
        let error = actual
            .planes()
            .iter()
            .flatten()
            .zip(expected.planes().iter().flatten())
            .map(|(a, b)| (a - b).abs())
            .fold(0f32, f32::max);
        assert!(error < 0.0001, "orientation {o}: {error}");
    }
}

#[test]
fn cfa_and_generated_proxy_share_the_oriented_edit_frame() {
    use engine_api::recipe::ProcessVersion;
    let dng =
        raw_decode::lossy_dng::read(&mut std::io::Cursor::new(support::lossy_dng(false, false)))
            .unwrap()
            .unwrap();
    let mut metadata = dng.metadata;
    metadata.width = 64;
    metadata.height = 48;
    metadata.default_crop = [0, 0, 64, 48];
    metadata.cfa_layout = raw_decode::CfaLayout::Bayer([[0, 1], [1, 2]]);
    let cfa = raw_decode::CfaImage::from_linear(
        64,
        48,
        (0..64 * 48)
            .map(|i| 0.05 + (i % 64) as f32 / 600. + (i / 64) as f32 / 800.)
            .collect(),
    )
    .unwrap();
    let mut settings = DevelopSettings::default();
    settings.detail.sharpening.amount = 0.;
    settings.detail.noise_reduction.color = 0.;
    let proxy = CameraLinearProxy::generate(
        &cfa,
        &metadata,
        &settings,
        ProcessVersion::NATIVE_CURRENT,
        [1; 32],
        &Default::default(),
    )
    .unwrap();
    settings.geometry.crop.rect.left = 0.25;
    settings.geometry.crop.rect.bottom = 0.75;
    settings.geometry.upright.mode = engine_api::recipe::settings::UprightMode::Full;
    settings.geometry.upright.homography =
        Some([[1., 0.03, 0.01], [0.01, 1., 0.02], [0.01, 0., 1.]]);
    settings.locals.adjustments.push(LocalAdjustment {
        components: vec![MaskComponent::new(MaskKind::Linear {
            start: [0.1, 0.2],
            end: [0.9, 0.7],
        })],
        params: LocalParams {
            exposure: 0.5,
            ..Default::default()
        },
        ..Default::default()
    });
    for o in 1..=8 {
        metadata.catalog_orientation = Some(o);
        metadata.orientation = 1;
        let proxy = proxy.clone().with_catalog_orientation(o).unwrap();
        let a = pipeline_cpu::render_linear_scaled(
            &settings,
            &RenderSource::Cfa {
                image: &cfa,
                metadata: &metadata,
            },
            1,
        )
        .unwrap();
        let b =
            pipeline_cpu::render_linear_scaled(&settings, &RenderSource::CameraLinear(&proxy), 1)
                .unwrap();
        assert_eq!((a.width(), a.height()), (b.width(), b.height()));
        let error = a
            .planes()
            .iter()
            .flatten()
            .zip(b.planes().iter().flatten())
            .map(|(a, b)| (a - b).abs())
            .fold(0f32, f32::max);
        assert!(error < 2e-5, "orientation {o}: {error}");
    }
}
