use engine_api::{
    EngineError, EngineResult,
    id::ImageId,
    jobs::CancellationToken,
    recipe::{
        DevelopSettings, LocalAdjustment, LocalParams, MaskComponent, MaskKind, ProcessVersion,
    },
    tile::TileCoord,
};
use image_core::{RawImage, RenderOutput, Renderer, mask_cache::MaskHooks};
use std::sync::Arc;

struct BrokenRaster;
impl MaskHooks for BrokenRaster {
    fn revision(&self) -> u64 {
        1
    }
    fn rasterize(
        &self,
        _: &pipeline_cpu::Image,
        _: &LocalAdjustment,
        _: u8,
    ) -> EngineResult<Vec<f32>> {
        Err(EngineError::invalid("mask", "synthetic raster failed"))
    }
}

#[test]
fn proxy_raster_failure_aborts_each_process_and_thumbnail_level() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("synthetic.dng");
    std::fs::write(
        &path,
        include_bytes!("../../raw-decode/tests/fixtures/linear-gradient-jxl.dng"),
    )
    .unwrap();
    let image = RawImage::open(ImageId(1315), path).unwrap();
    let mut settings = DevelopSettings::default();
    settings.locals.adjustments.push(LocalAdjustment {
        components: vec![MaskComponent::new(MaskKind::Subject { model: None })],
        params: LocalParams {
            exposure: 1.,
            ..Default::default()
        },
        ..Default::default()
    });
    for version in [ProcessVersion::NATIVE_CURRENT, ProcessVersion::adobe(6)] {
        let renderer = Renderer::new(Default::default()).for_process_version(version);
        renderer
            .mask_cache()
            .set_hooks(Some(Arc::new(BrokenRaster)));
        for level in [0, 1, 2] {
            let mut emitted = 0;
            let result = renderer.render_tiles(
                &image,
                &settings,
                &[TileCoord::new(level, 0, 0)],
                RenderOutput::Display,
                &CancellationToken::new(),
                &mut |_| emitted += 1,
            );
            let error = result.expect_err("proxy render silently discarded a raster failure");
            assert!(error.to_string().contains("synthetic raster failed"));
            assert_eq!(emitted, 0, "failed frames must publish no partial pixels");
        }
    }
}
