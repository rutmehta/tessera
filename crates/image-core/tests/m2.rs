mod common;
use common::*;
use engine_api::recipe::DevelopSettings;
use engine_api::recipe::settings::NormalizedRect;
use engine_api::tile::Extent;
use image_core::{PixelRect, RenderOutput, Renderer, RendererConfig};

#[test]
fn image_stage_fallbacks_match_reference_and_count_global_barriers() {
    use engine_api::{jobs::CancellationToken, stage::StageId};
    use image_core::{CountingStageOp, CpuStageOp, Op, StageOp};
    let image = pipeline_cpu::Image::new(
        520,
        270,
        vec![
            vec![0.2; 520 * 270],
            vec![0.3; 520 * 270],
            vec![0.4; 520 * 270],
        ],
    )
    .unwrap();
    let mut s = DevelopSettings::default();
    s.tone.dehaze = 35.;
    s.tone.clarity = 20.;
    s.geometry.crop.rect.right = 0.8;
    s.detail.sharpening.amount = 80.;
    let ops = CountingStageOp::new(CpuStageOp);
    let cancel = CancellationToken::new();
    let got = ops
        .run_image(
            StageId::Tone,
            &Op::ToneExtra(&s.tone),
            image.clone(),
            &cancel,
        )
        .unwrap();
    let expected = pipeline_cpu::tone_extra_image(&image, &s.tone).unwrap();
    assert_eq!(got.planes(), expected.planes());
    assert_eq!(ops.count(StageId::Tone), 1);
    let got = ops
        .run_image(StageId::Geometry, &Op::Geometry(&s.geometry), got, &cancel)
        .unwrap();
    let expected = pipeline_cpu::geometry(&expected, &s.geometry).unwrap();
    assert_eq!(got.planes(), expected.planes());
    assert_eq!(ops.count(StageId::Geometry), 1);
    let got = ops
        .run_image(
            StageId::Detail,
            &Op::Detail(&s.detail),
            image.clone(),
            &cancel,
        )
        .unwrap();
    let mut expected = image.clone();
    for coord in image.coords() {
        let mut t = image
            .tile(coord, pipeline_cpu::detail_halo(&s.detail), 1)
            .unwrap();
        pipeline_cpu::detail(&mut t, &s.detail).unwrap();
        expected.put(&t).unwrap();
    }
    assert_eq!(got.planes(), expected.planes());
    assert_eq!(ops.count(StageId::Detail), 6);
    ops.reset();
    cancel.cancel();
    assert!(
        ops.run_image(StageId::Tone, &Op::ToneExtra(&s.tone), image, &cancel)
            .is_err()
    );
    assert_eq!(ops.count(StageId::Tone), 0);
}

#[test]
fn m2_cache_reuses_wb_but_dispatches_changed_controls() {
    use engine_api::stage::StageId;
    use image_core::{CountingStageOp, CpuStageOp, TileCache};
    use std::sync::Arc;
    let raw = synthetic(902, 300, 280, RGGB, [0, 0, 300, 280]);
    let ops = Arc::new(CountingStageOp::new(CpuStageOp));
    let r = Renderer::with_ops(
        ops.clone(),
        Arc::new(TileCache::new(64 << 20)),
        RendererConfig::default(),
    );
    let mut s = DevelopSettings::default();
    s.color.vibrance = 10.;
    let rect = PixelRect::full(raw.level_extent(0));
    r.render_region(&raw, &s, 0, rect).unwrap();
    for stage in [
        StageId::Detail,
        StageId::Color,
        StageId::Effects,
        StageId::Geometry,
    ] {
        assert!(ops.count(stage) > 0, "{stage}");
    }
    for i in 0..5 {
        match i {
            0 => s.detail.sharpening.amount = 75.,
            1 => s.tone.dehaze = 20.,
            2 => s.color.vibrance = 40.,
            3 => s.geometry.crop.angle = 3.,
            _ => s.effects.grain.amount = 25.,
        }
        ops.reset();
        let warm = r.render_region(&raw, &s, 0, rect).unwrap();
        assert_eq!(ops.count(StageId::Demosaic), 0);
        assert_eq!(ops.count(StageId::WhiteBalance), 0);
        assert_eq!(ops.count(StageId::Detail), 4);
        assert_eq!(ops.count(StageId::Tone), 9); // neutral WB-path tone + basic tone + one global barrier
        assert_eq!(ops.count(StageId::Color), 4);
        assert_eq!(ops.count(StageId::Geometry), 1);
        assert_eq!(ops.count(StageId::Effects), 4);
        let cold = Renderer::new(RendererConfig::default())
            .render_region(&raw, &s, 0, rect)
            .unwrap();
        assert!(
            max_u8_diff(
                &assemble_u8(raw.active_extent(), &warm),
                &assemble_u8(raw.active_extent(), &cold)
            ) <= 2
        );
    }
}

#[test]
fn m2_cropped_regions_levels_and_cancellation() {
    use engine_api::{EngineError, jobs::CancellationToken, tile::TileCoord};
    let raw = synthetic(903, 300, 280, RGGB, [0, 0, 300, 280]);
    let mut s = DevelopSettings::default();
    s.geometry.crop.rect.right = 0.8; // 240 wide, inside the first raw tile.
    let r = Renderer::new(RendererConfig::default());
    assert!(
        r.render_region(&raw, &s, 0, PixelRect::new(245, 0, 10, 10))
            .unwrap()
            .is_empty()
    );
    assert!(
        r.render_region(&raw, &s, 255, PixelRect::new(0, 0, 1, 1))
            .is_err()
    );
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert_eq!(
        r.render_tiles(
            &raw,
            &s,
            &[],
            RenderOutput::Display,
            &cancel,
            &mut |_| panic!()
        ),
        Err(EngineError::Cancelled)
    );
    for coords in [
        vec![TileCoord::new(255, 0, 0)],
        vec![TileCoord::new(0, 0, 0), TileCoord::new(1, 0, 0)],
        vec![TileCoord::new(0, 1, 0)],
    ] {
        assert!(
            r.render_tiles(
                &raw,
                &s,
                &coords,
                RenderOutput::Display,
                &CancellationToken::new(),
                &mut |_| panic!()
            )
            .is_err()
        );
    }
    let cancel = CancellationToken::new();
    let mut delivered = 0;
    assert_eq!(
        r.render_tiles(
            &raw,
            &s,
            &[TileCoord::new(0, 0, 0), TileCoord::new(0, 0, 1)],
            RenderOutput::SceneLinear,
            &cancel,
            &mut |_| {
                delivered += 1;
                cancel.cancel();
            }
        ),
        Err(EngineError::Cancelled)
    );
    assert_eq!(delivered, 1);
    let tiles = r
        .render_region_as(
            &raw,
            &s,
            3,
            PixelRect::full(raw.level_extent(3)),
            RenderOutput::SceneLinear,
        )
        .unwrap();
    let e = Renderer::output_extent(&raw, &s, 3).unwrap();
    assert_eq!(
        tiles.iter().map(|t| t.layout().extent.area()).sum::<u64>(),
        e.area()
    );
    assert!(tiles.iter().all(|t| t.coord().level == 3));
}

#[test]
fn m2_matches_whole_image_reference_across_seams_and_crop() {
    let raw = synthetic(901, 700, 533, RGGB, [3, 5, 690, 521]);
    let mut s = DevelopSettings::default();
    s.detail.sharpening.amount = 70.;
    s.tone.exposure = 0.4;
    s.tone.dehaze = 30.;
    s.tone.clarity = 20.;
    s.color.vibrance = 30.;
    s.effects.grain.amount = 15.;
    s.geometry.crop.rect = NormalizedRect {
        left: 0.1,
        top: 0.1,
        right: 0.9,
        bottom: 0.9,
    };
    s.geometry.crop.angle = 2.;
    let source = pipeline_cpu::RenderSource::Cfa {
        image: raw.cfa(),
        metadata: raw.metadata(),
    };
    let mut base_settings = DevelopSettings::default();
    base_settings.detail.sharpening.amount = 0.0;
    base_settings.detail.noise_reduction.color = 0.0;
    let base = pipeline_cpu::render_linear_scaled(&base_settings, &source, 1).unwrap();
    let mut expected = base.clone();
    for c in base.coords() {
        let mut t = base
            .tile(c, pipeline_cpu::detail_halo(&s.detail), 1)
            .unwrap();
        pipeline_cpu::detail(&mut t, &s.detail).unwrap();
        expected.put(&t).unwrap();
    }
    for c in expected.coords() {
        let mut t = expected.tile(c, 0, 1).unwrap();
        pipeline_cpu::tone(&mut t, &s.tone).unwrap();
        expected.put(&t).unwrap();
    }
    expected = pipeline_cpu::tone_extra_image(&expected, &s.tone).unwrap();
    for c in expected.coords() {
        let mut t = expected.tile(c, 0, 1).unwrap();
        pipeline_cpu::color(&mut t, &s.color).unwrap();
        expected.put(&t).unwrap();
    }
    let extent = Extent::new(expected.width(), expected.height());
    for c in expected.coords() {
        let mut t = expected.tile(c, 0, 1).unwrap();
        pipeline_cpu::effects_in_crop(&mut t, &s.effects, extent, &s.geometry.crop).unwrap();
        expected.put(&t).unwrap();
    }
    expected = pipeline_cpu::geometry(&expected, &s.geometry).unwrap();
    let extent = Extent::new(expected.width(), expected.height());
    let reference = pipeline_cpu::render_linear_scaled(&s, &source, 1).unwrap();
    assert_eq!(reference.planes(), expected.planes());
    let r = Renderer::new(RendererConfig::default());
    let tiles = r
        .render_region_as(
            &raw,
            &s,
            0,
            PixelRect::full(raw.level_extent(0)),
            RenderOutput::SceneLinear,
        )
        .unwrap();
    assert_eq!(
        tiles.iter().map(|t| t.layout().extent.area()).sum::<u64>(),
        extent.area()
    );
    assert!(max_f32_diff(&assemble_f32(extent, &tiles), expected.planes()) < 1e-5);
}
