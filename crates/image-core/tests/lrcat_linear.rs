#[allow(dead_code)]
#[path = "../../raw-decode/tests/support/mod.rs"]
mod support;
use engine_api::{
    id::ImageId,
    jobs::CancellationToken,
    recipe::{DevelopSettings, ProcessVersion},
    tile::TileCoord,
};
use image_core::{RawImage, RenderOutput, Renderer};
#[test]
fn classic_linear_dng_enters_camera_develop_for_native_and_adobe() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("synthetic.dng");
    std::fs::write(&path, support::lossy_dng(false, false)).unwrap();
    let image = RawImage::open(ImageId(818), &path).unwrap();
    assert_eq!(image.metadata().orientation, 6);
    assert_eq!(image.metadata().as_shot_wb[..3], [2., 1., 1.5]);
    let settings = DevelopSettings::default();
    for process in [
        ProcessVersion::NATIVE_CURRENT,
        ProcessVersion {
            family: engine_api::recipe::ProcessFamily::Adobe,
            revision: 6,
        },
    ] {
        let renderer = Renderer::new(Default::default()).for_process_version(process);
        let mut tiles = Vec::new();
        renderer
            .render_tiles(
                &image,
                &settings,
                &[TileCoord::new(0, 0, 0)],
                RenderOutput::SceneLinear,
                &CancellationToken::new(),
                &mut |tile| tiles.push(tile),
            )
            .unwrap();
        assert!(!tiles.is_empty());
    }
}

#[test]
fn crop_radial_and_saved_upright_land_in_same_normalized_region() {
    use engine_api::recipe::{
        LocalAdjustment, LocalParams, MaskComponent, MaskKind, settings::UprightMode,
    };
    let source = |w: usize, h: usize| {
        let mut dng = raw_decode::lossy_dng::read(&mut std::io::Cursor::new(support::lossy_dng(
            false, false,
        )))
        .unwrap()
        .unwrap();
        dng.width = w;
        dng.height = h;
        dng.pixels = vec![[0.12; 3]; w * h];
        dng.metadata.width = w as u32;
        dng.metadata.height = h as u32;
        dng.metadata.default_crop = [0, 0, w as u32, h as u32];
        pipeline_cpu::CameraLinearProxy::from_dng(dng).unwrap()
    };
    let original = source(512, 384);
    let proxy = source(128, 96);
    let mut s = DevelopSettings::default();
    s.detail.sharpening.amount = 0.;
    s.geometry.crop.rect.left = 0.125;
    s.geometry.crop.rect.right = 0.875;
    s.geometry.crop.rect.top = 0.125;
    s.geometry.crop.rect.bottom = 0.875;
    s.geometry.upright.mode = UprightMode::Full;
    s.geometry.upright.homography = Some([[1., 0.02, 0.01], [0., 1., 0.02], [0.015, 0., 1.]]);
    s.locals.adjustments.push(LocalAdjustment {
        components: vec![MaskComponent::new(MaskKind::Radial {
            center: [0.6, 0.4],
            radii: [0.25, 0.3],
            angle: 20.,
            feather: 60.,
        })],
        params: LocalParams {
            exposure: 1.,
            ..Default::default()
        },
        ..Default::default()
    });
    let render = |p: &pipeline_cpu::CameraLinearProxy, scale| {
        pipeline_cpu::render_linear_scaled(&s, &pipeline_cpu::RenderSource::CameraLinear(p), scale)
            .unwrap()
    };
    let full = render(&original, 4);
    let reduced = render(&proxy, 1);
    assert_eq!((full.width(), full.height()), (96, 72));
    assert_eq!(
        (full.width(), full.height()),
        (reduced.width(), reduced.height())
    );
    let max_error = full
        .planes()
        .iter()
        .flatten()
        .zip(reduced.planes().iter().flatten())
        .map(|(a, b)| (a - b).abs())
        .fold(0.0_f32, f32::max);
    assert!(max_error < 0.02, "normalized geometry error {max_error}");
}
