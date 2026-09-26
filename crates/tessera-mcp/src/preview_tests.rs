use super::*;
use engine_api::{color::ColorMatrix3, stage::StageId};
use image_core::{CountingStageOp, CpuStageOp};
use raw_decode::{CfaImage, CfaLayout, RawMetadata};

#[test]
fn raw_steps_reuse_upstream_operator_results() {
    let id = ImageId(1);
    let metadata = RawMetadata {
        make: "Synthetic".into(),
        model: "Test".into(),
        lens: None,
        iso: 100.,
        shutter_s: 0.01,
        aperture: 4.,
        focal_mm: 50.,
        capture_time: 0,
        orientation: 1,
        opcode_lists: [None, None, None],
        width: 64,
        height: 64,
        cfa_layout: CfaLayout::Bayer([[0, 1], [3, 2]]),
        black_levels: [0.; 4],
        white_level: 16383,
        as_shot_wb: [2., 1., 1.6, 1.],
        camera_to_xyz: ColorMatrix3([[0.; 3]; 3]),
        cam_xyz: [[0.9, 0.2, -0.1], [-0.3, 1.2, 0.1], [0., 0.1, 0.8], [0.; 3]],
        rgb_cam: [[0.; 4]; 3],
        default_crop: [0, 0, 64, 64],
        has_gain_map: false,
        has_opcode_list: false,
    };
    let raw = RawImage::new(
        id,
        Arc::new(CfaImage::from_linear(64, 64, vec![0.2; 64 * 64]).unwrap()),
        Arc::new(metadata),
    )
    .unwrap();
    let cache = PreviewCache::default();
    cache
        .sources
        .lock()
        .unwrap()
        .insert(id, Arc::new(Decoded::Raw(raw)));
    let ops = Arc::new(CountingStageOp::new(CpuStageOp));
    assert!(
        cache
            .renderer
            .set(Renderer::with_ops(
                ops.clone(),
                Arc::new(TileCache::new(32 << 20)),
                RendererConfig::default()
            ))
            .is_ok()
    );
    let mut recipe = Recipe::default();
    let path = Path::new("not-opened.NEF");
    let before = cache.display(id, path, &recipe, Some(1024)).unwrap();
    let counts = ops.counts();
    assert!(ops.count(StageId::Demosaic) > 0);
    for exposure in [0.1, 0.2, 0.3] {
        recipe.settings.tone.exposure = exposure;
        let after = cache.display(id, path, &recipe, Some(1024)).unwrap();
        assert_ne!(before, after);
        for stage in [
            StageId::Demosaic,
            StageId::CameraProfile,
            StageId::WhiteBalance,
        ] {
            assert_eq!(ops.count(stage), counts[stage.index()], "{stage:?}");
        }
    }
    assert!(ops.count(StageId::Tone) > counts[StageId::Tone.index()]);
    assert!(ops.count(StageId::Output) > counts[StageId::Output.index()]);
}

#[test]
fn rgb_decode_is_shared_by_preview_histogram_and_final() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.jpg");
    image::RgbImage::from_pixel(2050, 10, image::Rgb([64; 3]))
        .save(&path)
        .unwrap();
    let cache = PreviewCache::default();
    let id = ImageId(2);
    let recipe = Recipe::default();
    assert_eq!(
        cache
            .display(id, &path, &recipe, Some(1024))
            .unwrap()
            .width(),
        513
    );
    std::fs::remove_file(&path).unwrap();
    assert_eq!(cache.linear(id, &path, &recipe).unwrap().width(), 513);
    assert_eq!(
        cache.display(id, &path, &recipe, None).unwrap().width(),
        2050
    );
    assert_eq!(cache.sources.lock().unwrap().len(), 1);
}
