#[path = "../../image-core/tests/common/mod.rs"]
mod common;
use color_mgmt::{Builtin, Registry};
use engine_api::{
    color::IccProfileHandle,
    recipe::{DevelopSettings, settings::GamutMapping},
};
use image_core::{PixelRect, RendererConfig};
use pipeline_cpu::{OutputContext, OutputTarget, RenderSource};
use pipeline_gpu::{GpuContext, GpuManagedOutput, ManagedRenderer};
use std::sync::Arc;

#[test]
fn resident_managed_display_matches_cpu_across_proof_and_gamut_modes() {
    let device = Arc::new(GpuContext::new().unwrap());
    let mut registry = Registry::new();
    let target = registry.builtin(Builtin::DisplayP3).unwrap();
    let proof = registry.builtin(Builtin::Srgb).unwrap();
    let image = common::synthetic(915, 67, 49, common::RGGB, [0, 0, 67, 49]);
    for proof_enabled in [false, true] {
        for gamut in [GamutMapping::Clip, GamutMapping::Perceptual] {
            let mut settings = DevelopSettings::default();
            settings.tone.exposure = 1.0;
            settings.output.gamut_mapping = gamut;
            settings.output.proof_profile =
                proof_enabled.then(|| IccProfileHandle::from_profile_bytes(proof.icc_bytes()));
            let mut context = OutputContext {
                registry: &mut registry,
                target: OutputTarget::Display(&target),
                proof: proof_enabled.then_some(proof.as_ref()),
                options: Default::default(),
            };
            let output =
                Arc::new(GpuManagedOutput::new(device.clone(), &settings, &mut context).unwrap());
            let renderer = ManagedRenderer::new(output, RendererConfig::default());
            let actual = renderer
                .render_region(&image, &settings, 0, PixelRect::full(image.level_extent(0)))
                .unwrap();
            let expected = pipeline_cpu::render_managed_scaled(
                &settings,
                &RenderSource::Cfa {
                    image: image.cfa(),
                    metadata: image.metadata(),
                },
                1,
                &mut context,
            )
            .unwrap();
            for tile in &actual {
                let (ox, oy) = tile.coord().pixel_origin(256);
                let n = tile.layout().plane_len();
                let samples = tile.samples::<u8>().unwrap();
                for y in 0..tile.layout().extent.height {
                    for x in 0..tile.layout().extent.width {
                        let i = (y * tile.layout().extent.width + x) as usize;
                        for c in 0..3 {
                            let actual = f32::from(samples[c * n + i]) / 255.0;
                            let expected = expected.pixels.get_pixel(ox + x, oy + y)[c];
                            assert!(
                                (actual - expected).abs() < 0.03,
                                "proof={proof_enabled} gamut={gamut:?} ({x},{y}) {c}: {actual} vs {expected}"
                            );
                        }
                    }
                }
            }
            assert!(renderer.stats().last_resident_dispatches > 0);
            let mut wrong = settings.clone();
            wrong.output.proof_profile =
                Some(IccProfileHandle::from_profile_bytes(target.icc_bytes()));
            assert!(
                renderer
                    .render_region(&image, &wrong, 0, PixelRect::full(image.level_extent(0)))
                    .is_err()
            );
        }
    }
}
