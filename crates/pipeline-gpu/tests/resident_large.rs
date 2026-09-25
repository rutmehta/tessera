//! Hardware regression for a full sensor transaction, not just small tiles.
use std::sync::{Arc, Mutex};

use engine_api::{id::ImageId, recipe::DevelopSettings};
use image_core::{PixelRect, RawImage, Renderer, RendererConfig, TileCache};
use pipeline_gpu::{GpuContext, GpuStageOp};

struct Diagnostics;
impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Warn
    }
    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            eprintln!("{}: {}", record.target(), record.args());
        }
    }
    fn flush(&self) {}
}

#[test]
#[ignore = "requires the full Nikon NEF fixture and Metal"]
fn full_nef_transaction_keeps_device_alive() {
    let _ = log::set_logger(&Diagnostics);
    log::set_max_level(log::LevelFilter::Warn);
    let context = Arc::new(GpuContext::new().unwrap());
    let lost = Arc::new(Mutex::new(None));
    let record = lost.clone();
    context
        .device
        .set_device_lost_callback(move |reason, message| {
            eprintln!("device lost: {reason:?}: {message}");
            *record.lock().unwrap() = Some((reason, message));
        });
    let image = RawImage::open(
        ImageId(93),
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw/nikon-nef.NEF"),
    )
    .unwrap();
    let config = RendererConfig::default();
    let renderer = Renderer::with_ops(
        Arc::new(GpuStageOp::new(context)),
        Arc::new(TileCache::new(config.cache_budget_bytes)),
        config,
    );
    let result = renderer.render_region(
        &image,
        &DevelopSettings::default(),
        2,
        PixelRect::full(image.level_extent(2)),
    );
    assert!(
        result.is_ok(),
        "render: {result:?}; device loss: {:?}",
        lost.lock().unwrap()
    );
    assert!(lost.lock().unwrap().is_none());
}
