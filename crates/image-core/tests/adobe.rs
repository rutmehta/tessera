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

#[test]
fn lr2b_pv2010_is_admitted_by_develop_renderer() {
    use engine_api::recipe::settings::{
        Curve, CurvePoint, LegacyPv2010, MonochromeSettings, ToneCurves,
    };
    let image = synthetic(202, 24, 20, RGGB, [0, 0, 24, 20]);
    let r = Renderer::new(RendererConfig {
        process_version: ProcessVersion::adobe(2),
        ..Default::default()
    });
    let mut s = DevelopSettings::default();
    s.tone.legacy_pv2010 = Some(LegacyPv2010 {
        exposure: Some(1.),
        ..Default::default()
    });
    s.tone.curves_extended = Some(ToneCurves {
        rgb: Curve(vec![
            CurvePoint { x: 0., y: 0. },
            CurvePoint { x: 2., y: 3. },
        ]),
        ..Default::default()
    });
    s.color.monochrome = Some(MonochromeSettings {
        enabled: true,
        ..Default::default()
    });
    let actual = r
        .render_region_as(
            &image,
            &s,
            0,
            PixelRect::full(image.level_extent(0)),
            RenderOutput::SceneLinear,
        )
        .unwrap();
    let expected = pipeline_adobe::render_linear_scaled(
        &s,
        &pipeline_cpu::RenderSource::Cfa {
            image: image.cfa(),
            metadata: image.metadata(),
        },
        1,
    )
    .unwrap();
    for (a, b) in assemble_f32(image.level_extent(0), &actual)
        .iter()
        .flatten()
        .zip(expected.planes().iter().flatten())
    {
        assert!((a - b).abs() < 0.0001, "{a} vs {b}");
    }
}

#[test]
fn lr2e_old_pv2010_requires_reimport_but_new_block_is_accepted() {
    let image = synthetic(203, 24, 20, RGGB, [0, 0, 24, 20]);
    for revision in [1, 2] {
        let r = Renderer::new(RendererConfig {
            process_version: ProcessVersion::adobe(revision),
            ..Default::default()
        });
        let mut s = DevelopSettings::default();
        s.tone.exposure = 2.;
        let error = r
            .render_region(&image, &s, 0, PixelRect::full(image.level_extent(0)))
            .unwrap_err();
        assert!(error.to_string().contains("re-import needed"), "{error}");
        s.tone.legacy_pv2010 = Some(Default::default());
        r.render_region(&image, &s, 0, PixelRect::full(image.level_extent(0)))
            .unwrap();
    }
}

/// ENG-9 gamut policy: the Adobe Output stage honours the recipe's gamut
/// mapping (as export does) instead of always hard-clipping, and the EDR
/// viewport (`DisplayLinear`) draws the same rendition unencoded.
#[test]
fn eng9_adobe_display_honours_gamut_mapping_and_draws_edr() {
    use engine_api::recipe::settings::GamutMapping;
    let image = synthetic(904, 40, 32, RGGB, [0, 0, 40, 32]);
    let rect = PixelRect::full(image.level_extent(0));
    let extent = image.level_extent(0);
    let r = Renderer::new(RendererConfig {
        process_version: ProcessVersion::adobe(6),
        ..Default::default()
    });
    let m = engine_api::color::WorkingSpace::LinearSrgb
        .to_xyz()
        .inverse()
        .unwrap()
        * engine_api::color::WorkingSpace::LinearRec2020.to_xyz();
    let mut shown = Vec::new();
    for mapping in [GamutMapping::Perceptual, GamutMapping::Clip] {
        let mut s = DevelopSettings::default();
        s.color.saturation = 80.;
        s.output.gamut_mapping = mapping;
        let linear = assemble_f32(
            extent,
            &r.render_region_as(&image, &s, 0, rect, RenderOutput::SceneLinear)
                .unwrap(),
        );
        let display = assemble_u8(extent, &r.render_region(&image, &s, 0, rect).unwrap());
        let edr = assemble_f32(
            extent,
            &r.render_region_as(
                &image,
                &s,
                0,
                rect,
                RenderOutput::DisplayLinear(image_core::Headroom::SDR),
            )
            .unwrap(),
        );
        let mut outside = 0;
        for i in 0..linear[0].len() {
            let v = m.apply([0, 1, 2].map(|c| f64::from(linear[c][i])));
            let v = v.map(|c| c as f32);
            outside += usize::from(v.iter().any(|c| !(0. ..=1.).contains(c)));
            let expected = if mapping == GamutMapping::Clip {
                v.map(|c| c.clamp(0., 1.))
            } else {
                // Grey point: working-space (Rec.2020) luminance, as export.
                let grey = (0.2627 * linear[0][i] + 0.6780 * linear[1][i] + 0.0593 * linear[2][i])
                    .clamp(0., 1.);
                let mut chroma = 1f32;
                for c in v {
                    if c < 0. {
                        chroma = chroma.min(-grey / (c - grey));
                    }
                    if c > 1. {
                        chroma = chroma.min((1. - grey) / (c - grey));
                    }
                }
                v.map(|c| (grey + chroma * (c - grey)).clamp(0., 1.))
            };
            for c in 0..3 {
                let level = pipeline_cpu::srgb_oetf(expected[c]) * 255.;
                let got = f32::from(display[i * 3 + c]);
                assert!(
                    (got - level).abs() <= 0.51,
                    "{mapping:?} SDR pixel {i}: {got} vs {level}"
                );
                // The EDR frame is the SDR rendition unencoded: the same
                // pre-Output tiles, so only the 8-bit rounding separates them.
                let encoded = pipeline_cpu::srgb_oetf(edr[c][i]) * 255.;
                assert!(
                    (encoded - got).abs() <= 0.5 + 1e-3,
                    "{mapping:?} EDR pixel {i}: {encoded} vs SDR {got}"
                );
                // The SceneLinear reference is read from the f16 tile memo
                // (2^-11 relative), amplified at most 4x by chroma mapping.
                assert!(
                    (edr[c][i] - expected[c]).abs() <= 2e-3,
                    "{mapping:?} EDR pixel {i}: {} vs {}",
                    edr[c][i],
                    expected[c]
                );
            }
        }
        assert!(outside > 0, "fixture must leave sRGB");
        shown.push(display);
    }
    assert_ne!(shown[0], shown[1], "Perceptual must differ from Clip");
}
