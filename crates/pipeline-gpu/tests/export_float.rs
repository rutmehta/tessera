use color_mgmt::{Builtin, Registry, TransformOptions};
use engine_api::{jobs::CancellationToken, recipe::DevelopSettings};
use pipeline_cpu::{OutputContext, OutputTarget};
use pipeline_gpu::{GpuContext, GpuManagedOutput, ManagedRenderer};
use std::sync::Arc;

#[path = "../../image-core/tests/common/mod.rs"]
mod common;

#[test]
fn export_retires_uploads_without_intermediate_readback() {
    let mut settings = DevelopSettings::default();
    settings.lens.profile = engine_api::recipe::settings::LensProfileSource::None;
    settings.lens.remove_chromatic_aberration = false;
    let mut registry = Registry::new();
    let target = registry.builtin(Builtin::Srgb).unwrap();
    let output = Arc::new(
        GpuManagedOutput::new(
            Arc::new(GpuContext::new().unwrap()),
            &settings,
            &mut OutputContext {
                registry: &mut registry,
                target: OutputTarget::Export(&target),
                proof: None,
                options: TransformOptions::default(),
            },
        )
        .unwrap(),
    );
    let renderer = ManagedRenderer::new_export(
        output,
        image_core::RendererConfig {
            cache_budget_bytes: 0,
            ..Default::default()
        },
    );
    let image = common::synthetic(272, 4096, 1024, common::RGGB, [0, 0, 4096, 1024]);
    let tiles = renderer
        .render_export(
            &image,
            &settings,
            0,
            image_core::PixelRect::new(0, 256, 4096, 512),
            &CancellationToken::new(),
        )
        .unwrap()
        .unwrap();
    let stats = renderer.stats();
    assert!(stats.submissions > 1, "must exercise scratch retirement");
    assert_eq!(stats.readbacks, 1);
    assert!(stats.last_resident_allocated_bytes < 512 << 20);
    assert_eq!(
        tiles.iter().map(|t| t.layout().extent.area()).sum::<u64>(),
        4096 * 512
    );
    assert!(
        tiles
            .iter()
            .all(|tile| tile.samples::<f32>().unwrap().iter().all(|v| v.is_finite()))
    );
}

#[test]
fn resident_export_is_float_and_reads_once() {
    let context = Arc::new(GpuContext::new().unwrap());
    let mut settings = DevelopSettings::default();
    let mut registry = Registry::new();
    let target = registry.builtin(Builtin::ProPhoto).unwrap();
    let output = Arc::new(
        GpuManagedOutput::new(
            context,
            &settings,
            &mut OutputContext {
                registry: &mut registry,
                target: OutputTarget::Export(&target),
                proof: None,
                options: TransformOptions::default(),
            },
        )
        .unwrap(),
    );
    let renderer = ManagedRenderer::new_export(output, image_core::RendererConfig::default());
    let image = common::synthetic(271, 300, 270, common::RGGB, [0, 0, 300, 270]);
    assert!(
        renderer
            .render_export(
                &image,
                &settings,
                0,
                image_core::PixelRect::full(image.active_extent()),
                &CancellationToken::new()
            )
            .unwrap()
            .is_none()
    );
    settings.lens.profile = engine_api::recipe::settings::LensProfileSource::None;
    settings.lens.remove_chromatic_aberration = false;
    let tiles = renderer
        .render_export(
            &image,
            &settings,
            0,
            image_core::PixelRect::full(image.active_extent()),
            &CancellationToken::new(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(renderer.stats().readbacks, 1);
    let resized = pipeline_gpu::ExportResize {
        source: image.active_extent(),
        destination: engine_api::tile::Extent::new(281, 111),
        top: 0,
        rows: 111,
    };
    let renderer = ManagedRenderer::new_export_resized(
        Arc::new(
            GpuManagedOutput::new(
                Arc::new(GpuContext::new().unwrap()),
                &settings,
                &mut OutputContext {
                    registry: &mut registry,
                    target: OutputTarget::Export(&target),
                    proof: None,
                    options: TransformOptions::default(),
                },
            )
            .unwrap(),
        ),
        image_core::RendererConfig::default(),
        resized,
    );
    let scaled = renderer
        .render_export(
            &image,
            &settings,
            0,
            resized.source_rect().unwrap(),
            &CancellationToken::new(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(renderer.stats().readbacks, 1);
    assert_eq!(scaled.len(), 2);
    assert_eq!(
        scaled.iter().map(|t| t.layout().extent.area()).sum::<u64>(),
        resized.destination.area()
    );
    assert_eq!(
        tiles.iter().map(|t| t.layout().extent.area()).sum::<u64>(),
        300 * 270
    );
    assert!(tiles.iter().all(|t| t.samples::<f32>().is_ok()));
    assert!(
        tiles
            .iter()
            .flat_map(|t| t.samples::<f32>().unwrap())
            .any(|v| (v * 255.0).fract().abs() > 0.01)
    );
}
