//! LR-8m (A-LR8 M4): a relinked original (catalog orientation from an
//! imported Smart Preview) uses the normal RAW path, including the resident
//! GPU chain, exactly like an ordinary import with the same EXIF orientation.
#[path = "../../image-core/tests/common/mod.rs"]
#[allow(dead_code)]
mod common;
#[allow(dead_code)]
#[path = "../../raw-decode/tests/support/mod.rs"]
mod support;

use engine_api::{id::ImageId, recipe::DevelopSettings};
use image_core::{PixelRect, RawImage, Renderer, RendererConfig, TileCache};
use pipeline_gpu::{GpuContext, GpuStageOp};
use std::sync::Arc;

#[test]
fn lr8m_relinked_original_renders_resident_like_an_ordinary_import() {
    let dir = tempfile::tempdir().unwrap();
    let relinked_path = dir.path().join("relinked.dng");
    std::fs::write(&relinked_path, support::bayer_dng(1)).unwrap();
    let ordinary_path = dir.path().join("ordinary.dng");
    std::fs::write(&ordinary_path, support::bayer_dng(6)).unwrap();
    let gpu = Renderer::with_ops(
        Arc::new(GpuStageOp::new(Arc::new(GpuContext::new().unwrap()))),
        Arc::new(TileCache::new(64 << 20)),
        RendererConfig::default(),
    );
    let ordinary = RawImage::open(ImageId(7401), &ordinary_path).unwrap();
    let relinked =
        RawImage::open_with_catalog_orientation(ImageId(7402), &relinked_path, Some(6)).unwrap();
    let mut s = DevelopSettings::default();
    s.tone.exposure = 0.4;
    s.lens.manual_vignetting = 25.;
    assert!(gpu.can_render_resident(&ordinary, &s).unwrap());
    assert!(
        gpu.can_render_resident(&relinked, &s).unwrap(),
        "a relinked original must not be forced onto the scalar CPU route"
    );
    // Develop's GPU renderer draws both through the same resident-capable
    // RAW route (lens plan included): identical frames at every level.
    for level in [0u8, 1, 2] {
        let extent = Renderer::output_extent(&relinked, &s, level).unwrap();
        assert_eq!(
            extent,
            Renderer::output_extent(&ordinary, &s, level).unwrap()
        );
        let rect = PixelRect::full(extent);
        let actual = gpu.render_region(&relinked, &s, level, rect).unwrap();
        let expected = gpu.render_region(&ordinary, &s, level, rect).unwrap();
        assert_eq!(
            common::assemble_u8(extent, &actual),
            common::assemble_u8(extent, &expected),
            "level {level}"
        );
    }
}
