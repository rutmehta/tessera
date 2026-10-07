//! LR-8m (A-LR8 M4): a relinked original (catalog orientation from an
//! imported Smart Preview) uses the normal RAW path, including the resident
//! GPU chain, exactly like an ordinary import with the same EXIF orientation.
#[path = "../../image-core/tests/common/mod.rs"]
#[allow(dead_code)]
mod common;
#[allow(dead_code)]
#[path = "../../raw-decode/tests/support/mod.rs"]
mod support;

use engine_api::{id::ImageId, jobs::CancellationToken, recipe::DevelopSettings};
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
    let rect = PixelRect::full(relinked.level_extent(0));
    let cancel = CancellationToken::new();
    let resident = gpu
        .render_resident_region(&relinked, &s, 0, rect, &cancel)
        .unwrap()
        .expect("relinked original renders resident");
    let expected = gpu
        .render_resident_region(&ordinary, &s, 0, rect, &cancel)
        .unwrap()
        .expect("ordinary import renders resident");
    let extent = relinked.level_extent(0);
    assert_eq!(extent, ordinary.level_extent(0));
    assert_eq!(
        common::assemble_u8(extent, &resident),
        common::assemble_u8(extent, &expected)
    );
}
