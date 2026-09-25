mod common;
use common::*;
use engine_api::recipe::{DevelopSettings, ProcessVersion};
use engine_api::stage::StageId;
use image_core::{
    CountingStageOp, CpuStageOp, PixelRect, RenderOutput, Renderer, RendererConfig, TileCache,
};
use std::sync::Arc;

#[test]
fn adobe_dispatch_changes_pixels_but_reuses_demosaic() {
    let image = synthetic(18, 48, 40, RGGB, [0, 0, 48, 40]);
    let ops = Arc::new(CountingStageOp::new(CpuStageOp));
    let cache = Arc::new(TileCache::new(16 << 20));
    let mut settings = DevelopSettings::default();
    settings.tone.contrast = 35.;
    let rect = PixelRect::full(image.level_extent(0));
    let native = Renderer::with_ops(ops.clone(), cache.clone(), RendererConfig::default());
    let a = native.render_region(&image, &settings, 0, rect).unwrap();
    ops.reset();
    let compat = Renderer::with_ops(
        ops.clone(),
        cache,
        RendererConfig {
            process_version: ProcessVersion::adobe(6),
            ..Default::default()
        },
    );
    let b = compat.render_region(&image, &settings, 0, rect).unwrap();
    for stage in [
        StageId::CameraProfile,
        StageId::WhiteBalance,
        StageId::Detail,
        StageId::Tone,
        StageId::Color,
        StageId::Effects,
    ] {
        assert!(compat.adobe_invocations(stage) > 0, "{stage:?}");
        assert_eq!(native.adobe_invocations(stage), 0);
    }
    assert_eq!(
        ops.count(StageId::Demosaic),
        0,
        "process switch must reuse demosaic"
    );
    assert_ne!(
        assemble_u8(image.level_extent(0), &a),
        assemble_u8(image.level_extent(0), &b)
    );
    for r in [&native, &compat] {
        let tiles = r
            .render_region_as(&image, &settings, 0, rect, RenderOutput::SceneLinear)
            .unwrap();
        assert!(
            tiles
                .iter()
                .all(|t| t.samples::<f32>().unwrap().iter().all(|v| v.is_finite()))
        );
    }
}

#[test]
fn op_set_hash_starts_at_camera_profile() {
    let s = DevelopSettings::default();
    let a = image_core::PipelineGraph::stage_chain(
        &s,
        ProcessVersion::NATIVE_CURRENT.chain_seed(),
        pipeline_cpu::POST_DENOISE_ADAPTER,
    );
    let b = image_core::PipelineGraph::stage_chain(
        &s,
        ProcessVersion::adobe(6).chain_seed(),
        pipeline_cpu::POST_DENOISE_ADAPTER,
    );
    for ((stage, a), (_, b)) in a.into_iter().zip(b) {
        assert_eq!(a == b, stage < StageId::CameraProfile, "{stage:?}");
    }
}

// Minimal real TIFF DCP, explicit bytes rather than treating profile names as paths.
fn dcp_bytes() -> Vec<u8> {
    let mut b = vec![0u8; 38];
    b[..8].copy_from_slice(&[73, 73, 82, 67, 8, 0, 0, 0]);
    b[8..10].copy_from_slice(&2u16.to_le_bytes());
    b[10..12].copy_from_slice(&50721u16.to_le_bytes());
    b[12..14].copy_from_slice(&10u16.to_le_bytes());
    b[14..18].copy_from_slice(&9u32.to_le_bytes());
    b[18..22].copy_from_slice(&38u32.to_le_bytes());
    b[22..24].copy_from_slice(&50778u16.to_le_bytes());
    b[24..26].copy_from_slice(&3u16.to_le_bytes());
    b[26..30].copy_from_slice(&1u32.to_le_bytes());
    b[30..32].copy_from_slice(&21u16.to_le_bytes());
    for i in 0..9 {
        b.extend(if i % 4 == 0 { 1i32 } else { 0 }.to_le_bytes());
        b.extend(1i32.to_le_bytes());
    }
    b
}

#[test]
fn compat_matches_standalone_with_and_without_dcp() {
    let image = synthetic(19, 40, 32, RGGB, [0, 0, 40, 32]);
    let mut s = DevelopSettings::default();
    s.tone.exposure = 0.3;
    s.white_balance.mode = engine_api::recipe::settings::WhiteBalanceMode::Custom;
    s.white_balance.temperature = 6504.;
    s.white_balance.tint = 5.;
    s.tone.contrast = 23.;
    s.tone.clarity = 10.;
    s.color.vibrance = 12.;
    let source = pipeline_cpu::RenderSource::Cfa {
        image: image.cfa(),
        metadata: image.metadata(),
    };
    for dcp in [false, true] {
        let bytes = dcp_bytes();
        let profile =
            dcp.then(|| image_core::pipeline_adobe::dcp::DcpProfile::parse(&bytes).unwrap());
        let expected = image_core::pipeline_adobe::render_linear_scaled_with_profile(
            &s,
            &source,
            1,
            profile.as_ref(),
        )
        .unwrap();
        let mut r = Renderer::new(RendererConfig {
            process_version: ProcessVersion::adobe(6),
            ..Default::default()
        });
        if dcp {
            r = r.with_dcp_profile(&bytes).unwrap();
        }
        let tiles = r
            .render_region_as(
                &image,
                &s,
                0,
                PixelRect::full(image.level_extent(0)),
                RenderOutput::SceneLinear,
            )
            .unwrap();
        let got = assemble_f32(image.level_extent(0), &tiles);
        for (a, b) in got.iter().flatten().zip(expected.planes().iter().flatten()) {
            assert!(a.is_finite());
            assert!((a - b).abs() < 0.0001, "DCP={dcp}: {a} != {b}");
        }
    }
}
