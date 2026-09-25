use engine_api::{
    jobs::CancellationToken,
    recipe::settings::{ColorSettings, EffectsSettings, ToneSettings},
    stage::StageId,
    tile::{Extent, Tile, TileCoord, TileLayout},
};
use image_core::{CpuStageOp, Op, StageOp};
use pipeline_gpu::{GpuContext, GpuStageOp};
use std::sync::Arc;
#[path = "../../image-core/tests/common/mod.rs"]
mod common;

#[test]
fn resident_capability_matches_extended_settings_and_backend() {
    use engine_api::recipe::DevelopSettings;
    use image_core::{Renderer, RendererConfig, TileCache};
    let gpu = Arc::new(GpuStageOp::new(Arc::new(GpuContext::new().unwrap())));
    let renderer = Renderer::with_ops(gpu, Arc::new(TileCache::new(0)), RendererConfig::default());
    let cpu = Renderer::new(RendererConfig::default());
    let image = common::synthetic(1719, 19, 17, common::xtrans(), [0, 0, 19, 17]);
    let mut s = DevelopSettings::default();
    s.color.vibrance = 20.;
    s.tone.curves.parametric.darks = 10.;
    s.effects.vignette.amount = -20.;
    assert!(renderer.can_render_resident(&image, &s).unwrap());
    assert!(!cpu.can_render_resident(&image, &s).unwrap());
    // M2-17b: Texture/Clarity/Dehaze run as a resident whole-level barrier.
    s.tone.clarity = 5.;
    assert!(renderer.can_render_resident(&image, &s).unwrap());
    assert!(!cpu.can_render_resident(&image, &s).unwrap());
    s.geometry.crop.angle = 2.;
    assert!(!renderer.can_render_resident(&image, &s).unwrap());
}

#[test]
fn xtrans_renderer_fuses_edits_and_reuses_resident_cache() {
    renderer_parity(common::xtrans());
}

#[test]
fn bayer_renderer_fuses_edits_and_reuses_resident_cache() {
    renderer_parity(common::RGGB);
}

fn renderer_parity(cfa: raw_decode::CfaLayout) {
    use engine_api::recipe::DevelopSettings;
    use image_core::{PixelRect, RenderOutput, Renderer, RendererConfig, TileCache};
    let gpu = Arc::new(GpuStageOp::new(Arc::new(GpuContext::new().unwrap())));
    let renderer = Renderer::with_ops(
        gpu.clone(),
        Arc::new(TileCache::new(0)),
        RendererConfig::default(),
    );
    let cpu = Renderer::new(RendererConfig::default());
    let image = common::synthetic(1717, 269, 83, cfa, [3, 5, 263, 77]);
    let mut settings = DevelopSettings::default();
    settings.tone.exposure = 0.2;
    settings.tone.curves.parametric.lights = 15.;
    settings.color.vibrance = 20.;
    settings.effects.grain.amount = 15.;
    settings.effects.vignette.amount = -25.;
    for level in [0, 2, 5] {
        for output in [RenderOutput::Display, RenderOutput::SceneLinear] {
            let rect = PixelRect::full(image.level_extent(level));
            let expected = cpu
                .render_region_as(&image, &settings, level, rect, output)
                .unwrap();
            let before = gpu.stats();
            let actual = renderer
                .render_region_as(&image, &settings, level, rect, output)
                .unwrap();
            let cold = gpu.stats();
            assert_eq!(
                cold.submissions - before.submissions,
                1,
                "resident transaction required"
            );
            // Whole-level requests run as one level-sized tile: one fused pass.
            assert_eq!(cold.fused_dispatches - before.fused_dispatches, 1);
            let again = renderer
                .render_region_as(&image, &settings, level, rect, output)
                .unwrap();
            assert_eq!(
                gpu.stats().uploads,
                cold.uploads,
                "warm render must not upload pixels"
            );
            for ((a, b), warm) in actual.iter().zip(&expected).zip(&again) {
                assert_eq!(a.coord(), b.coord());
                assert_eq!(a.layout(), b.layout());
                if output == RenderOutput::Display {
                    assert_eq!(a.samples::<u8>().unwrap(), warm.samples::<u8>().unwrap());
                    let max =
                        common::max_u8_diff(a.samples::<u8>().unwrap(), b.samples::<u8>().unwrap());
                    assert!(max <= 1, "level {level}: {max}");
                } else {
                    assert_eq!(a.samples::<f32>().unwrap(), warm.samples::<f32>().unwrap());
                    let max = a
                        .samples::<f32>()
                        .unwrap()
                        .iter()
                        .zip(b.samples::<f32>().unwrap())
                        .map(|(a, b)| (a - b).abs())
                        .fold(0., f32::max);
                    assert!(max <= 0.002, "level {level}: {max}");
                }
            }
            settings.color.saturation += 3.;
        }
    }
}

#[test]
fn geometry_and_locals_keep_surface_fallback() {
    use engine_api::recipe::{DevelopSettings, LocalAdjustment};
    use image_core::{Renderer, RendererConfig, TileCache};
    let gpu = Arc::new(GpuStageOp::new(Arc::new(GpuContext::new().unwrap())));
    let renderer = Renderer::with_ops(
        gpu.clone(),
        Arc::new(TileCache::new(0)),
        RendererConfig::default(),
    );
    for cfa in [common::RGGB, common::xtrans()] {
        let image = common::synthetic(1718, 19, 17, cfa, [0, 0, 19, 17]);
        for mode in 0..2 {
            let mut s = DevelopSettings::default();
            match mode {
                0 => s.geometry.crop.angle = 5.,
                _ => s.locals.adjustments.push(LocalAdjustment::default()),
            }
            assert!(
                !renderer
                    .render_to_surface(&image, &s, 0, 0, &CancellationToken::new())
                    .unwrap()
            );
        }
    }
    assert_eq!(gpu.stats().submissions, 0);
}

#[test]
fn resident_point_chain_is_one_dispatch_and_matches_cpu() {
    let gpu = GpuStageOp::new(Arc::new(GpuContext::new().unwrap()));
    let mut tone = ToneSettings {
        exposure: 0.3,
        ..Default::default()
    };
    tone.curves.parametric.lights = 25.;
    let color = ColorSettings {
        vibrance: 30.,
        saturation: -10.,
        ..Default::default()
    };
    let mut effects = EffectsSettings::default();
    effects.vignette.amount = -30.;
    effects.grain.amount = 20.;
    let extent = Extent::new(1031, 539);
    for display in [false, true] {
        let layout = TileLayout {
            extent: Extent::new(17, 13),
            halo: 2,
            channels: 3,
        };
        let input = Tile::from_samples(
            TileCoord::new(1, 1, 0),
            layout,
            (0..layout.len())
                .map(|i| ((i * 31) % 900) as f32 / 500. - 0.08)
                .collect(),
        )
        .unwrap();
        let mut chain = vec![
            Op::Tone(&tone),
            Op::ToneExtra(&tone),
            Op::Color(&color),
            Op::Effects(&effects, extent),
        ];
        if display {
            chain.push(Op::Display {
                gamut: Default::default(),
                headroom: None,
            });
        }
        let mut expected = input.clone();
        for op in &chain {
            expected = CpuStageOp.run(StageId::Tone, op, expected).unwrap();
        }
        let mut batch = gpu.begin_resident().unwrap();
        let uploaded = batch.upload(&input).unwrap();
        let output = batch.run_chain(&chain, &uploaded).unwrap();
        let result = batch
            .finish(vec![output], display, None, &CancellationToken::new())
            .unwrap();
        // The first chain also builds the GPU-resident vignette/grain map;
        // the second reuses it (same effects geometry, different Display).
        assert_eq!(
            gpu.stats().last_resident_dispatches,
            if display { 1 } else { 2 }
        );
        let actual = &result.tiles[0];
        assert_eq!(actual.layout(), expected.layout());
        if display {
            assert!(
                actual
                    .samples::<u8>()
                    .unwrap()
                    .iter()
                    .zip(expected.samples::<u8>().unwrap())
                    .all(|(a, b)| a.abs_diff(*b) <= 1)
            );
        } else {
            assert!(
                actual
                    .samples::<f32>()
                    .unwrap()
                    .iter()
                    .zip(expected.samples::<f32>().unwrap())
                    .all(|(a, b)| (a - b).abs() <= 0.002)
            );
        }
    }
    assert_eq!(gpu.stats().fused_dispatches, 2);
    assert_eq!(gpu.stats().submissions, 2);
    assert_eq!(gpu.stats().readbacks, 2);
}
