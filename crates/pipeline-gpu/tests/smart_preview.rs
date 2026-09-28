//! Numerical and cache qualification for the camera-linear resident route.
#[path = "../../image-core/tests/common/mod.rs"]
mod common;
use engine_api::{
    id::ImageId,
    jobs::CancellationToken,
    recipe::{
        DevelopSettings, LocalAdjustment, MaskComponent, MaskKind, ProcessVersion,
        settings::{LensProfileSource, WhiteBalanceMode},
    },
    tile::Tile,
};
use image_core::{
    Headroom, PixelRect, RawImage, RenderOutput, Renderer, RendererConfig, TileCache,
};
use pipeline_cpu::{CameraLinearProxy, LensContext, ManualCaSettings, SmartPreviewEncoding};
use pipeline_gpu::{GpuContext, GpuStageOp};
use std::sync::Arc;

fn metal_context() -> Arc<GpuContext> {
    let context = GpuContext::new().expect("Smart Preview GPU qualification requires a Metal device; unavailable is a failure, not a skip");
    assert_eq!(context.adapter_info.backend, wgpu::Backend::Metal);
    eprintln!(
        "Smart Preview qualification adapter: {:?}",
        context.adapter_info
    );
    Arc::new(context)
}

fn fixture(id: u128, hdr: bool, embedded: bool) -> (RawImage, DevelopSettings) {
    fixture_size(id, hdr, embedded, 269, 67)
}
fn fixture_size(
    id: u128,
    hdr: bool,
    embedded: bool,
    w: u32,
    h: u32,
) -> (RawImage, DevelopSettings) {
    fixture_tier(
        id,
        hdr,
        embedded,
        w,
        h,
        pipeline_cpu::SmartPreviewTier::Detail2560,
    )
}
fn fixture_tier(
    id: u128,
    hdr: bool,
    embedded: bool,
    w: u32,
    h: u32,
    tier: pipeline_cpu::SmartPreviewTier,
) -> (RawImage, DevelopSettings) {
    fixture_options(id, hdr, embedded, w, h, tier, false)
}
fn unmapped_fixture(id: u128, hdr: bool) -> (RawImage, DevelopSettings) {
    fixture_options(
        id,
        hdr,
        false,
        269,
        67,
        pipeline_cpu::SmartPreviewTier::Detail2560,
        true,
    )
}
#[allow(clippy::too_many_arguments)]
fn fixture_options(
    id: u128,
    hdr: bool,
    embedded: bool,
    w: u32,
    h: u32,
    tier: pipeline_cpu::SmartPreviewTier,
    unmapped: bool,
) -> (RawImage, DevelopSettings) {
    let mut metadata = common::metadata(w, h, common::RGGB, [1, 1, w - 2, h - 2]);
    metadata.orientation = 6;
    let mut settings = DevelopSettings::default();
    if hdr || unmapped {
        settings.lens.profile = LensProfileSource::None;
        settings.lens.remove_chromatic_aberration = false;
    }
    if embedded {
        let mut opcode: Vec<u8> = [1_u32, 3, 0x01030000, 0, 56]
            .into_iter()
            .flat_map(u32::to_be_bytes)
            .collect();
        for value in [0.2_f64, 0., 0., 0., 0., 0.5, 0.5] {
            opcode.extend(value.to_be_bytes());
        }
        metadata.opcode_lists[1] = Some(opcode);
    }
    let samples = if hdr {
        (0..w * h)
            .map(|i| {
                if metadata.cfa_layout.channel_at(i % w, i / w) == 0 {
                    if (i % w).is_multiple_of(4) {
                        -500000.
                    } else {
                        -100000.
                    }
                } else {
                    0.3
                }
            })
            .collect()
    } else {
        common::samples(w, h, common::RGGB)
    };
    let cfa = raw_decode::CfaImage::from_linear(w, h, samples).unwrap();
    let profile = lens::Profile {
        model: "captured".into(),
        samples: vec![lens::CalibrationSample {
            vignette: [-0.13, 0.01, 0.],
            ca_red: [0.94, 0.002, 0.],
            ca_blue: [1.04, 0., 0.],
            distortion: lens::BrownConrady {
                k1: 0.018,
                ..Default::default()
            },
            ..Default::default()
        }],
        ..Default::default()
    };
    let context = if hdr || embedded || unmapped {
        LensContext::default()
    } else {
        LensContext {
            profile: Some(&profile),
            manual_ca: ManualCaSettings {
                red_cyan: 7.,
                blue_yellow: -4.,
            },
            ..Default::default()
        }
    };
    let proxy = CameraLinearProxy::generate_with_tier(
        &cfa,
        &metadata,
        &settings,
        ProcessVersion::NATIVE_CURRENT,
        [id as u8; 32],
        &context,
        tier,
    )
    .unwrap();
    let decoded =
        CameraLinearProxy::decode_persistent(&proxy.encode_persistent(1000).unwrap()).unwrap();
    if hdr {
        assert_eq!(decoded.encoding, SmartPreviewEncoding::F32);
        assert!(
            decoded
                .proxy
                .pixels()
                .planes()
                .iter()
                .flatten()
                .any(|v| *v < -65504.)
        );
        assert!(
            decoded
                .proxy
                .pixels()
                .planes()
                .iter()
                .flatten()
                .any(|v| *v > 65504.)
        );
    }
    (
        RawImage::from_camera_linear_proxy(
            ImageId(id),
            ImageId(id + 10000),
            Arc::new(decoded.proxy),
        )
        .unwrap(),
        settings,
    )
}
fn compare(actual: &[Tile], expected: &[Tile], display: bool) {
    assert_eq!(actual.len(), expected.len());
    for (a, b) in actual.iter().zip(expected) {
        assert_eq!(a.coord(), b.coord());
        assert_eq!(a.layout(), b.layout());
        if display {
            for (x, y) in a
                .samples::<u8>()
                .unwrap()
                .iter()
                .zip(b.samples::<u8>().unwrap())
            {
                assert!(x.abs_diff(*y) <= 4, "display {x} != {y}");
            }
        } else {
            for (index, (x, y)) in a
                .samples::<f32>()
                .unwrap()
                .iter()
                .zip(b.samples::<f32>().unwrap())
                .enumerate()
            {
                assert!(x.is_finite() && y.is_finite());
                assert!(
                    (x - y).abs() <= 0.0005 * y.abs() + 0.0002,
                    "linear {x} != {y}, tile {:?}, sample {index}",
                    a.coord()
                );
            }
        }
    }
}
#[test]
fn camera_linear_matches_scalar_with_captured_ca_vignette_geometry_wb_and_cache_edits() {
    let context = metal_context();
    for budget in [0, 1, 64 << 20] {
        let gpu = Arc::new(GpuStageOp::with_cache_budget(context.clone(), budget));
        let renderer = Renderer::with_ops(
            gpu.clone(),
            Arc::new(TileCache::new(budget)),
            RendererConfig {
                cache_budget_bytes: budget,
                ..Default::default()
            },
        );
        let cpu = Renderer::new(Default::default());
        let (image, mut settings) = fixture(200 + budget as u128, false, false);
        let tail = image
            .camera_linear_proxy()
            .unwrap()
            .resident_tail_plan(&settings)
            .unwrap()
            .unwrap();
        assert!(
            tail.ca.is_none(),
            "profile and manual CA are baked exactly once"
        );
        assert!(tail.vignette.is_some());
        assert!(!renderer.can_render_resident(&image, &settings).unwrap());
        for edit in 0..5 {
            if edit == 1 {
                settings.tone.exposure = 0.7;
            }
            if edit == 2 {
                settings.white_balance.mode = WhiteBalanceMode::Custom;
                settings.white_balance.temperature = 4200.;
                settings.white_balance.tint = 13.;
            }
            if edit == 3 {
                settings.geometry.crop.rect.right = 0.91;
                settings.geometry.crop.angle = 4.;
            }
            if edit == 4 {
                settings.detail.sharpening.amount = 30.;
            }
            let extent = Renderer::output_extent(&image, &settings, 0).unwrap();
            for output in [RenderOutput::SceneLinear, RenderOutput::Display] {
                let before = gpu.stats();
                let actual = renderer
                    .render_region_as(&image, &settings, 0, PixelRect::full(extent), output)
                    .unwrap();
                let after = gpu.stats();
                assert_eq!(after.submissions, before.submissions);
                if budget >= 64 << 20 && edit > 0 {
                    assert_eq!(
                        after.uploads, before.uploads,
                        "camera input must survive downstream edits"
                    );
                }
                let expected = cpu
                    .render_region_as(&image, &settings, 0, PixelRect::full(extent), output)
                    .unwrap();
                compare(&actual, &expected, output == RenderOutput::Display);
            }
        }
        // An independent snapshot sharing the recipe owner must never reuse its pixels.
        let (other, other_settings) = fixture(999, false, false);
        let other = RawImage::from_camera_linear_proxy(
            image.recipe_owner(),
            other.id(),
            Arc::new(other.camera_linear_proxy().unwrap().clone()),
        )
        .unwrap();
        let other_extent = Renderer::output_extent(&other, &other_settings, 0).unwrap();
        let before = gpu.stats();
        let actual = renderer
            .render_region_as(
                &other,
                &other_settings,
                0,
                PixelRect::full(other_extent),
                RenderOutput::SceneLinear,
            )
            .unwrap();
        assert_eq!(gpu.stats().uploads, before.uploads);
        let expected = cpu
            .render_region_as(
                &other,
                &other_settings,
                0,
                PixelRect::full(other_extent),
                RenderOutput::SceneLinear,
            )
            .unwrap();
        compare(&actual, &expected, false);
    }
}
#[test]
fn f32_negative_hdr_prefix_remains_finite_on_cold_warm_and_rejected_cache_paths() {
    let context = metal_context();
    let (image, mut settings) = fixture(51, true, false);
    let cpu = Renderer::new(Default::default());
    for budget in [0, 1, 64 << 20] {
        let gpu = Arc::new(GpuStageOp::with_cache_budget(context.clone(), budget));
        let renderer = Renderer::with_ops(
            gpu.clone(),
            Arc::new(TileCache::new(budget)),
            RendererConfig {
                cache_budget_bytes: budget,
                ..Default::default()
            },
        );
        for exposure in [0., -3., 0.7] {
            settings.tone.exposure = exposure;
            let extent = Renderer::output_extent(&image, &settings, 0).unwrap();
            let before = gpu.stats();
            let actual = renderer
                .render_region_as(
                    &image,
                    &settings,
                    0,
                    PixelRect::full(extent),
                    RenderOutput::SceneLinear,
                )
                .unwrap();
            assert!(gpu.stats().submissions > before.submissions);
            assert!(gpu.stats().last_resident_dispatches > 0);
            let expected = cpu
                .render_region_as(
                    &image,
                    &settings,
                    0,
                    PixelRect::full(extent),
                    RenderOutput::SceneLinear,
                )
                .unwrap();
            compare(&actual, &expected, false);
        }
    }
}
#[test]
fn unsupported_tail_locals_keep_scalar_fallback_and_export_guards() {
    let gpu = Arc::new(GpuStageOp::new(metal_context()));
    let renderer = Renderer::with_ops(
        gpu.clone(),
        Arc::new(TileCache::new(64 << 20)),
        Default::default(),
    );
    let cpu = Renderer::new(Default::default());
    for embedded in [false, true] {
        let (image, mut settings) = fixture(67 + u128::from(embedded), false, embedded);
        if embedded {
            assert!(
                image
                    .camera_linear_proxy()
                    .unwrap()
                    .resident_tail_plan(&settings)
                    .unwrap()
                    .is_none()
            );
        }
        for level in [1, 2] {
            let extent = Renderer::output_extent(&image, &settings, level).unwrap();
            let before = gpu.stats();
            let actual = renderer
                .render_region_as(
                    &image,
                    &settings,
                    level,
                    PixelRect::full(extent),
                    RenderOutput::Display,
                )
                .unwrap();
            let expected = cpu
                .render_region_as(
                    &image,
                    &settings,
                    level,
                    PixelRect::full(extent),
                    RenderOutput::Display,
                )
                .unwrap();
            compare(&actual, &expected, true);
            assert!(!renderer.can_render_resident(&image, &settings).unwrap());
            assert_eq!(gpu.stats().submissions, before.submissions);
        }
        settings.locals.adjustments.push(LocalAdjustment {
            components: vec![MaskComponent::new(MaskKind::Linear {
                start: [0., 0.],
                end: [1., 0.],
            })],
            ..Default::default()
        });
        assert!(!renderer.can_render_resident(&image, &settings).unwrap());
        let extent = Renderer::output_extent(&image, &settings, 0).unwrap();
        let before = gpu.stats();
        let actual = renderer
            .render_region_as(
                &image,
                &settings,
                0,
                PixelRect::full(extent),
                RenderOutput::Display,
            )
            .unwrap();
        let expected = cpu
            .render_region_as(
                &image,
                &settings,
                0,
                PixelRect::full(extent),
                RenderOutput::Display,
            )
            .unwrap();
        compare(&actual, &expected, true);
        assert_eq!(gpu.stats().submissions, before.submissions);
        let mut rows = vec![123.; extent.width as usize * 3];
        assert!(
            !renderer
                .render_export_rows(
                    &image,
                    &settings,
                    0,
                    0..1,
                    None,
                    &mut rows,
                    &CancellationToken::new()
                )
                .unwrap()
        );
        assert!(rows.iter().all(|v| *v == 123.));
    }
}

// These guide slopes straddle the original/proxy pixel-axis classification
// boundary because the odd short edge is rounded up during scale-two reduction.
#[test]
fn reduced_nonsquare_guided_geometry_uses_proxy_dimensions_and_matches_scalar() {
    use engine_api::recipe::settings::{GuideLine, LensProfileSource, UprightMode};
    let (width, height) = (2563, 67);
    let metadata = common::metadata(width, height, common::RGGB, [1, 1, 2561, 63]);
    let cfa = raw_decode::CfaImage::from_linear(
        width,
        height,
        common::samples(width, height, common::RGGB),
    )
    .unwrap();
    let mut settings = DevelopSettings::default();
    settings.lens.profile = LensProfileSource::None;
    settings.lens.remove_chromatic_aberration = false;
    let proxy = CameraLinearProxy::generate(
        &cfa,
        &metadata,
        &settings,
        ProcessVersion::NATIVE_CURRENT,
        [171; 32],
        &LensContext::default(),
    )
    .unwrap();
    let proxy = CameraLinearProxy::decode_persistent(&proxy.encode_persistent(1000).unwrap())
        .unwrap()
        .proxy;
    assert_eq!(proxy.scale(), 2);
    assert_eq!(
        (proxy.pixels().width(), proxy.pixels().height()),
        (1281, 32)
    );
    settings.geometry.upright.mode = UprightMode::Guided;
    settings.geometry.upright.guides = vec![
        GuideLine {
            start: [0.2, 0.09],
            end: [0.22, 0.9],
        },
        GuideLine {
            start: [0.8, 0.09],
            end: [0.78, 0.9],
        },
    ];
    let guides: Vec<_> = settings
        .geometry
        .upright
        .guides
        .iter()
        .map(|g| {
            let start = g.start.map(|v| 2. * f64::from(v) - 1.);
            let end = g.end.map(|v| 2. * f64::from(v) - 1.);
            let dx = (end[0] - start[0]).abs();
            let dy = (end[1] - start[1]).abs();
            assert!(
                dx * 2561. > dy * 63.,
                "original dimensions would infer Horizontal"
            );
            assert!(
                dx * 1281. < dy * 32.,
                "actual proxy dimensions must infer Vertical"
            );
            lens::Guide {
                start,
                end,
                axis: lens::GuideAxis::Vertical,
            }
        })
        .collect();
    let expected_upright = lens::guided_upright(&guides)
        .unwrap()
        .homography
        .inverse()
        .unwrap();
    assert_ne!(expected_upright, lens::Homography::IDENTITY);
    let plan = proxy.resident_tail_plan(&settings).unwrap().unwrap();
    assert!(plan.ca.is_none());
    for (actual, expected) in plan
        .map
        .as_ref()
        .unwrap()
        .upright
        .0
        .iter()
        .flatten()
        .zip(expected_upright.0.iter().flatten())
    {
        assert!(
            (actual - expected).abs() <= 1e-10,
            "wrong Guided frame: {actual} != {expected}"
        );
    }
    settings.geometry.crop.rect.left = 0.03;
    settings.geometry.crop.rect.right = 0.94;
    let image =
        RawImage::from_camera_linear_proxy(ImageId(171), ImageId(10171), Arc::new(proxy)).unwrap();
    let cpu = Renderer::new(Default::default());
    let context = metal_context();
    for budget in [0, 64 << 20] {
        let gpu = Arc::new(GpuStageOp::with_cache_budget(context.clone(), budget));
        let renderer = Renderer::with_ops(
            gpu.clone(),
            Arc::new(TileCache::new(budget)),
            RendererConfig {
                cache_budget_bytes: budget,
                ..Default::default()
            },
        );
        assert!(!renderer.can_render_resident(&image, &settings).unwrap());
        let rect = PixelRect::full(Renderer::output_extent(&image, &settings, 0).unwrap());
        for output in [
            RenderOutput::SceneLinear,
            RenderOutput::Display,
            RenderOutput::DisplayLinear(Headroom::new(4.)),
        ] {
            let expected = cpu
                .render_region_as(&image, &settings, 0, rect, output)
                .unwrap();
            for _ in 0..2 {
                let before = gpu.stats();
                let actual = renderer
                    .render_region_as(&image, &settings, 0, rect, output)
                    .unwrap();
                assert_eq!(gpu.stats().submissions, before.submissions);
                compare(&actual, &expected, output == RenderOutput::Display);
            }
        }
    }
}

#[test]
fn display_linear_tiles_keep_scene_signed_hdr_until_the_display_transform() {
    let context = metal_context();
    let cpu = Renderer::new(Default::default());
    for hdr in [false, true] {
        let (image, mut settings) = fixture(173 + u128::from(hdr), hdr, false);
        settings.tone.exposure = 2.;
        let rect = PixelRect::full(image.active_extent());
        for budget in [0, 64 << 20] {
            let gpu = Arc::new(GpuStageOp::with_cache_budget(context.clone(), budget));
            let renderer = Renderer::with_ops(
                gpu.clone(),
                Arc::new(TileCache::new(budget)),
                RendererConfig {
                    cache_budget_bytes: budget,
                    ..Default::default()
                },
            );
            let expected_scene = cpu
                .render_region_as(&image, &settings, 0, rect, RenderOutput::SceneLinear)
                .unwrap();
            let actual_scene = renderer
                .render_region_as(&image, &settings, 0, rect, RenderOutput::SceneLinear)
                .unwrap();
            compare(&actual_scene, &expected_scene, false);
            if hdr {
                assert!(
                    actual_scene.iter().any(|t| t
                        .samples::<f32>()
                        .unwrap()
                        .iter()
                        .any(|v| *v < 0.))
                );
                assert!(
                    actual_scene.iter().any(|t| t
                        .samples::<f32>()
                        .unwrap()
                        .iter()
                        .any(|v| *v > 1.))
                );
            }
            for headroom in [1., 4.] {
                let output = RenderOutput::DisplayLinear(Headroom::new(headroom));
                let expected = cpu
                    .render_region_as(&image, &settings, 0, rect, output)
                    .unwrap();
                for _ in 0..2 {
                    let before = gpu.stats();
                    let actual = renderer
                        .render_region_as(&image, &settings, 0, rect, output)
                        .unwrap();
                    assert_eq!(gpu.stats().submissions > before.submissions, hdr);
                    compare(&actual, &expected, false);
                    // DisplayLinear intentionally maps into [0, headroom]; negative
                    // preservation is checked above in SceneLinear, not after gamut mapping.
                    assert!(actual.iter().all(|t| {
                        t.samples::<f32>()
                            .unwrap()
                            .iter()
                            .all(|v| v.is_finite() && (0.0..=headroom).contains(v))
                    }));
                    if !hdr && headroom > 1. {
                        assert!(
                            actual.iter().any(|t| t
                                .samples::<f32>()
                                .unwrap()
                                .iter()
                                .any(|v| *v > 1.))
                        );
                    }
                }
            }
        }
    }
}

/// Simulates viewport/adaptive transitions without rebuilding the renderer:
/// reuse L0 checkpoints while changing coarse output level and downstream edits.
#[test]
fn coarse_proxy_post_geometry_linear_reduction_matches_scalar_across_edits() {
    let context = metal_context();
    for hdr in [false, true] {
        for budget in [0, 64 << 20] {
            let (image, mut settings) = fixture_size(991 + u128::from(hdr), hdr, false, 1029, 131);
            let cpu = Renderer::new(Default::default());
            let gpu = Arc::new(GpuStageOp::with_cache_budget(context.clone(), budget));
            let renderer = Renderer::with_ops(
                gpu.clone(),
                Arc::new(TileCache::new(budget)),
                RendererConfig {
                    cache_budget_bytes: budget,
                    ..Default::default()
                },
            );
            for (step, level) in [0, 1, 2, 3, 1, 0, 2].into_iter().enumerate() {
                if step == 2 {
                    settings.tone.exposure = 0.8;
                }
                if step == 3 {
                    settings.white_balance.mode = WhiteBalanceMode::Daylight;
                }
                if step == 4 {
                    settings.geometry.crop.rect.left = 0.07;
                    settings.geometry.crop.rect.right = 0.91;
                    settings.geometry.crop.angle = 3.;
                }
                if step == 5 {
                    settings.tone.texture = 17.;
                    settings.tone.clarity = 9.;
                    settings.tone.dehaze = 6.;
                }
                let extent = Renderer::output_extent(&image, &settings, level).unwrap();
                for output in [
                    RenderOutput::SceneLinear,
                    RenderOutput::Display,
                    RenderOutput::DisplayLinear(Headroom::new(4.)),
                ] {
                    let before = gpu.stats();
                    let got = renderer
                        .render_region_as(&image, &settings, level, PixelRect::full(extent), output)
                        .unwrap();
                    let after = gpu.stats();
                    let resident = hdr && step < 4;
                    assert_eq!(
                        renderer.can_render_resident(&image, &settings).unwrap(),
                        resident
                    );
                    assert_eq!(after.submissions > before.submissions, resident);
                    if resident {
                        assert!(after.last_resident_dispatches > 0);
                    }
                    if budget > 0 && step > 0 {
                        assert_eq!(
                            after.uploads, before.uploads,
                            "coarse output must reuse original proxy upload"
                        );
                    }
                    let expected = cpu
                        .render_region_as(&image, &settings, level, PixelRect::full(extent), output)
                        .unwrap();
                    eprintln!(
                        "coarse comparison hdr={hdr} budget={budget} step={step} level={level} output={output:?}"
                    );
                    compare(&got, &expected, output == RenderOutput::Display);
                    if level > 0 && extent.width > 256 {
                        let edge = PixelRect::new(256, 0, extent.width - 256, extent.height);
                        let part = renderer
                            .render_region_as(&image, &settings, level, edge, output)
                            .unwrap();
                        let reference = cpu
                            .render_region_as(&image, &settings, level, edge, output)
                            .unwrap();
                        compare(&part, &reference, output == RenderOutput::Display);
                    }
                }
            }
        }
    }
}

#[cfg(target_os = "macos")]
mod surface_checks {
    use super::*;
    use objc2_core_foundation::{CFDictionary, CFNumber, CFType};
    use objc2_io_surface::{
        IOSurfaceLockOptions, IOSurfaceRef, kIOSurfaceBytesPerElement, kIOSurfaceHeight,
        kIOSurfacePixelFormat, kIOSurfaceWidth,
    };
    fn surface(
        w: u32,
        h: u32,
        bytes: i64,
        format: [u8; 4],
    ) -> objc2_core_foundation::CFRetained<IOSurfaceRef> {
        let numbers = [
            CFNumber::new_i64(i64::from(w)),
            CFNumber::new_i64(i64::from(h)),
            CFNumber::new_i64(bytes),
            CFNumber::new_i64(i64::from(u32::from_be_bytes(format))),
        ];
        // SAFETY: immutable CFString keys, CFNumber values and retained Copy-rule surface.
        unsafe {
            let keys: [&CFType; 4] = [
                kIOSurfaceWidth.as_ref(),
                kIOSurfaceHeight.as_ref(),
                kIOSurfaceBytesPerElement.as_ref(),
                kIOSurfacePixelFormat.as_ref(),
            ];
            let values: Vec<&CFType> = numbers.iter().map(|n| (**n).as_ref()).collect();
            let dict = CFDictionary::from_slices(&keys, &values);
            IOSurfaceRef::new(dict.as_opaque()).unwrap()
        }
    }

    fn read(s: &IOSurfaceRef, w: u32, h: u32, bpe: usize) -> Vec<u8> {
        // SAFETY: rendering waited for completion; reads stay in the locked allocation.
        unsafe {
            s.lock(IOSurfaceLockOptions::ReadOnly, std::ptr::null_mut());
            let base = s.base_address().as_ptr() as *const u8;
            let stride = s.bytes_per_row();
            let mut out = Vec::new();
            for y in 0..h as usize {
                out.extend_from_slice(std::slice::from_raw_parts(
                    base.add(y * stride),
                    w as usize * bpe,
                ));
            }
            s.unlock(IOSurfaceLockOptions::ReadOnly, std::ptr::null_mut());
            out
        }
    }

    /// Actual surface dispatch and the level-independent Develop capability must
    /// agree as edits enter and leave mapped geometry. Fixtures with captured
    /// distortion remain covered separately; these are explicit no-map fixtures.
    #[test]
    fn unmapped_proxy_edits_restore_gpu_after_geometry_fallback_at_every_level() {
        let context = metal_context();
        for hdr in [false, true] {
            for budget in [0, 64 << 20] {
                let (image, mut settings) = fixture_options(
                    2300 + u128::from(hdr),
                    hdr,
                    false,
                    if hdr { 1029 } else { 4922 },
                    if hdr { 131 } else { 67 },
                    pipeline_cpu::SmartPreviewTier::Compact2048,
                    true,
                );
                // Keep extreme HDR overshoot before spatial averaging; the F16
                // branch independently covers odd Compact scale-three geometry.
                assert_eq!(
                    image.camera_linear_proxy().unwrap().scale(),
                    if hdr { 1 } else { 3 }
                );
                let gpu = Arc::new(GpuStageOp::with_cache_budget(context.clone(), budget));
                let renderer = Renderer::with_ops(
                    gpu.clone(),
                    Arc::new(TileCache::new(budget)),
                    RendererConfig {
                        cache_budget_bytes: budget,
                        ..Default::default()
                    },
                );
                let cpu = Renderer::new(Default::default());
                for edit in 0..5 {
                    match edit {
                        1 => settings.tone.exposure = 0.6,
                        2 => settings.white_balance.mode = WhiteBalanceMode::Daylight,
                        3 => {
                            settings.geometry.crop.rect.right = 0.93;
                            settings.geometry.crop.angle = 2.;
                        }
                        4 => settings.geometry = Default::default(),
                        _ => {}
                    }
                    let resident = edit != 3;
                    assert_eq!(
                        renderer.can_render_resident(&image, &settings).unwrap(),
                        resident
                    );
                    for level in [0, 1, 2, 3, 1] {
                        let extent = Renderer::output_extent(&image, &settings, level).unwrap();
                        for output in [
                            RenderOutput::Display,
                            RenderOutput::DisplayLinear(Headroom::new(4.)),
                        ] {
                            let float = output != RenderOutput::Display;
                            let target = surface(
                                extent.width,
                                extent.height,
                                if float { 8 } else { 4 },
                                if float { *b"RGhA" } else { *b"RGBA" },
                            );
                            let before = gpu.stats();
                            let receipt = renderer
                                .render_surface_as(
                                    &image,
                                    &settings,
                                    level,
                                    target.id(),
                                    output,
                                    &CancellationToken::new(),
                                )
                                .unwrap();
                            let after = gpu.stats();
                            assert_eq!(receipt.is_some(), resident);
                            assert_eq!(after.submissions > before.submissions, resident);
                            assert_eq!(after.pixel_readback_bytes, before.pixel_readback_bytes);
                            if resident {
                                assert!(after.last_resident_dispatches > 0);
                                if budget > 0 && edit > 0 {
                                    assert_eq!(after.uploads, before.uploads);
                                }
                            }
                            let before = gpu.stats();
                            let actual = renderer
                                .render_region_as(
                                    &image,
                                    &settings,
                                    level,
                                    PixelRect::full(extent),
                                    output,
                                )
                                .unwrap();
                            assert_eq!(gpu.stats().submissions > before.submissions, resident);
                            let expected = cpu
                                .render_region_as(
                                    &image,
                                    &settings,
                                    level,
                                    PixelRect::full(extent),
                                    output,
                                )
                                .unwrap();
                            compare(&actual, &expected, !float);
                            if !resident {
                                for (a, b) in actual.iter().zip(&expected) {
                                    if float {
                                        assert_eq!(
                                            a.samples::<f32>().unwrap(),
                                            b.samples::<f32>().unwrap()
                                        );
                                    } else {
                                        assert_eq!(
                                            a.samples::<u8>().unwrap(),
                                            b.samples::<u8>().unwrap()
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn camera_linear_surface_matches_scalar() {
        let (image, mut settings) = unmapped_fixture(801, false);
        settings.white_balance.mode = WhiteBalanceMode::Custom;
        settings.white_balance.temperature = 4200.;
        settings.white_balance.tint = 13.;
        settings.tone.exposure = 0.7;
        let extent = Renderer::output_extent(&image, &settings, 0).unwrap();
        let surface = surface(extent.width, extent.height, 4, *b"RGBA");
        let gpu = Arc::new(GpuStageOp::new(metal_context()));
        let renderer = Renderer::with_ops(
            gpu.clone(),
            Arc::new(TileCache::new(64 << 20)),
            Default::default(),
        );
        let before = gpu.stats();
        assert!(
            renderer
                .render_surface(
                    &image,
                    &settings,
                    0,
                    surface.id(),
                    &CancellationToken::new()
                )
                .unwrap()
                .is_some()
        );
        let after = gpu.stats();
        assert!(after.submissions > before.submissions);
        assert!(after.last_resident_dispatches > 0);
        assert_eq!(after.pixel_readback_bytes, before.pixel_readback_bytes);
        assert!(after.histogram_readbacks > before.histogram_readbacks);
        let pixels = read(&surface, extent.width, extent.height, 4);
        let cpu = Renderer::new(Default::default());
        let expected = common::assemble_u8(
            extent,
            &cpu.render_region(&image, &settings, 0, PixelRect::full(extent))
                .unwrap(),
        );
        for (rgba, rgb) in pixels
            .as_chunks::<4>()
            .0
            .iter()
            .zip(expected.as_chunks::<3>().0)
        {
            for c in 0..3 {
                assert!(rgba[c].abs_diff(rgb[c]) <= 4);
            }
            assert_eq!(rgba[3], 255);
        }
        let cancel = CancellationToken::new();
        cancel.cancel();
        assert!(
            renderer
                .render_surface(&image, &settings, 0, surface.id(), &cancel)
                .is_err()
        );
    }

    #[test]
    fn coarse_proxy_sdr_and_edr_surfaces_match_scalar_without_pixel_readback() {
        let context = metal_context();
        let cpu = Renderer::new(Default::default());
        for hdr in [false, true] {
            let (image, mut settings) = unmapped_fixture(1201 + u128::from(hdr), hdr);
            settings.tone.exposure = 1.25;
            let gpu = Arc::new(GpuStageOp::new(context.clone()));
            let renderer = Renderer::with_ops(
                gpu.clone(),
                Arc::new(TileCache::new(64 << 20)),
                Default::default(),
            );
            for level in [1, 2, 3, 0, 1] {
                let extent = Renderer::output_extent(&image, &settings, level).unwrap();
                for float in [false, true] {
                    let output = if float {
                        RenderOutput::DisplayLinear(Headroom::new(4.))
                    } else {
                        RenderOutput::Display
                    };
                    let bpe = if float { 8 } else { 4 };
                    let target = surface(
                        extent.width,
                        extent.height,
                        bpe as i64,
                        if float { *b"RGhA" } else { *b"RGBA" },
                    );
                    let before = gpu.stats();
                    let histogram = renderer
                        .render_surface_as(
                            &image,
                            &settings,
                            level,
                            target.id(),
                            output,
                            &CancellationToken::new(),
                        )
                        .unwrap()
                        .expect("resident coarse output");
                    let after = gpu.stats();
                    assert!(after.submissions > before.submissions);
                    assert_eq!(after.pixel_readback_bytes, before.pixel_readback_bytes);
                    for channel in histogram {
                        assert_eq!(channel.iter().sum::<u32>(), extent.width * extent.height);
                    }
                    let expected = cpu
                        .render_region_as(&image, &settings, level, PixelRect::full(extent), output)
                        .unwrap();
                    let bytes = read(&target, extent.width, extent.height, bpe);
                    if float {
                        let pixels: Vec<_> = bytes
                            .as_chunks::<2>()
                            .0
                            .iter()
                            .map(|v| half::f16::from_le_bytes(*v).to_f32())
                            .collect();
                        for t in expected {
                            let l = t.layout();
                            let samples = t.samples::<f32>().unwrap();
                            let (ox, oy) = t.coord().pixel_origin(engine_api::tile::TILE_SIZE);
                            for y in 0..l.extent.height as usize {
                                for x in 0..l.extent.width as usize {
                                    let src = y * l.extent.width as usize + x;
                                    let dst = ((oy as usize + y) * extent.width as usize
                                        + ox as usize
                                        + x)
                                        * 4;
                                    for c in 0..3 {
                                        let reference = samples[c * l.plane_len() + src];
                                        let error = 0.0005 * reference.abs() + 0.0002;
                                        assert!(
                                            (pixels[dst + c] - reference).abs()
                                                <= error
                                                    + (reference.abs() + error) / 2048.
                                                    + 1. / 33554432.
                                        );
                                    }
                                    assert_eq!(pixels[dst + 3], 1.);
                                }
                            }
                        }
                    } else {
                        let expected = common::assemble_u8(extent, &expected);
                        for (actual, reference) in bytes
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .zip(expected.as_chunks::<3>().0)
                        {
                            for c in 0..3 {
                                assert!(actual[c].abs_diff(reference[c]) <= 4);
                            }
                            assert_eq!(actual[3], 255);
                        }
                    }
                    let cancel = CancellationToken::new();
                    cancel.cancel();
                    assert!(
                        renderer
                            .render_surface_as(
                                &image,
                                &settings,
                                level,
                                target.id(),
                                output,
                                &cancel
                            )
                            .is_err()
                    );
                }
            }
        }
    }

    #[test]
    fn camera_linear_half_surface_matches_scalar_edr_with_no_pixel_readback() {
        let context = metal_context();
        let cpu = Renderer::new(Default::default());
        for hdr in [false, true] {
            let (image, mut settings) = unmapped_fixture(180 + u128::from(hdr), hdr);
            settings.tone.exposure = 2.;
            let extent = image.active_extent();
            let rect = PixelRect::full(extent);
            let headroom = 4.;
            let output = RenderOutput::DisplayLinear(Headroom::new(headroom));
            let expected = cpu
                .render_region_as(&image, &settings, 0, rect, output)
                .unwrap();
            for budget in [0, 64 << 20] {
                let gpu = Arc::new(GpuStageOp::with_cache_budget(context.clone(), budget));
                let renderer = Renderer::with_ops(
                    gpu.clone(),
                    Arc::new(TileCache::new(budget)),
                    RendererConfig {
                        cache_budget_bytes: budget,
                        ..Default::default()
                    },
                );
                let surface = surface(extent.width, extent.height, 8, *b"RGhA");
                for _ in 0..2 {
                    let before = gpu.stats();
                    let histogram = renderer
                        .render_surface_as(
                            &image,
                            &settings,
                            0,
                            surface.id(),
                            output,
                            &CancellationToken::new(),
                        )
                        .unwrap()
                        .expect("qualified proxy L0 surface");
                    let after = gpu.stats();
                    assert!(after.submissions > before.submissions);
                    assert!(after.last_resident_dispatches > 0);
                    assert_eq!(after.pixel_readback_bytes, before.pixel_readback_bytes);
                    assert!(after.histogram_readbacks > before.histogram_readbacks);
                    for channel in histogram {
                        assert_eq!(channel.iter().sum::<u32>(), extent.width * extent.height);
                    }
                    let bytes = read(&surface, extent.width, extent.height, 8);
                    assert_eq!(
                        bytes.len(),
                        extent.width as usize * extent.height as usize * 8
                    );
                    let pixels: Vec<_> = bytes
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|b| half::f16::from_le_bytes(*b).to_f32())
                        .collect();
                    let mut above_white = false;
                    for tile in &expected {
                        let layout = tile.layout();
                        assert_eq!(layout.halo, 0);
                        let samples = tile.samples::<f32>().unwrap();
                        let plane = layout.plane_len();
                        let (ox, oy) = tile.coord().pixel_origin(engine_api::tile::TILE_SIZE);
                        for y in 0..layout.extent.height as usize {
                            for x in 0..layout.extent.width as usize {
                                let i = y * layout.extent.width as usize + x;
                                let o =
                                    ((oy as usize + y) * extent.width as usize + ox as usize + x)
                                        * 4;
                                for c in 0..3 {
                                    let actual = pixels[o + c];
                                    let reference = samples[c * plane + i];
                                    assert!(
                                        actual.is_finite() && (0.0..=headroom).contains(&actual)
                                    );
                                    // Existing camera-linear tile tolerance plus IEEE binary16
                                    // nearest rounding: 2^-11 relative + 2^-25 absolute.
                                    let tile_error = 0.0005 * reference.abs() + 0.0002;
                                    let half_error =
                                        (reference.abs() + tile_error) / 2048. + 1. / 33554432.;
                                    assert!(
                                        (actual - reference).abs() <= tile_error + half_error,
                                        "half surface {actual} != scalar {reference}"
                                    );
                                    above_white |= actual > 1.;
                                }
                                assert_eq!(pixels[o + 3], 1.0);
                            }
                        }
                    }
                    if !hdr {
                        assert!(
                            above_white,
                            "EDR surface must preserve highlights above SDR white"
                        );
                    }
                    let cancel = CancellationToken::new();
                    cancel.cancel();
                    let frozen = read(&surface, extent.width, extent.height, 8);
                    let before = gpu.stats();
                    assert!(
                        renderer
                            .render_surface_as(&image, &settings, 0, surface.id(), output, &cancel)
                            .is_err()
                    );
                    assert_eq!(gpu.stats().submissions, before.submissions);
                    assert_eq!(read(&surface, extent.width, extent.height, 8), frozen);
                }
            }
        }
    }
}

#[test]
fn compact_scale_three_codec_reopen_keeps_captured_geometry_on_cpu() {
    let (image, mut settings) = fixture_tier(
        1337,
        false,
        false,
        4922,
        67,
        pipeline_cpu::SmartPreviewTier::Compact2048,
    );
    assert_eq!(
        image.camera_linear_proxy().unwrap().tier(),
        pipeline_cpu::SmartPreviewTier::Compact2048
    );
    assert_eq!(image.camera_linear_proxy().unwrap().scale(), 3);
    assert_eq!(image.active_extent().width, 1640);
    settings.white_balance.mode = WhiteBalanceMode::Daylight;
    settings.tone.exposure = 0.6;
    settings.geometry.crop.rect.right = 0.91;
    let cpu = Renderer::new(Default::default());
    let gpu = Arc::new(GpuStageOp::new(metal_context()));
    let renderer = Renderer::with_ops(
        gpu.clone(),
        Arc::new(TileCache::new(64 << 20)),
        Default::default(),
    );
    for level in [0, 1, 2, 0] {
        let extent = Renderer::output_extent(&image, &settings, level).unwrap();
        let mut gpu_scene = Vec::new();
        for output in [
            RenderOutput::SceneLinear,
            RenderOutput::Display,
            RenderOutput::DisplayLinear(Headroom::new(4.)),
        ] {
            let before = gpu.stats();
            let got = renderer
                .render_region_as(&image, &settings, level, PixelRect::full(extent), output)
                .unwrap();
            assert!(!renderer.can_render_resident(&image, &settings).unwrap());
            assert_eq!(gpu.stats().submissions, before.submissions);
            if output == RenderOutput::SceneLinear {
                gpu_scene = got.clone();
            }
            if output == RenderOutput::DisplayLinear(Headroom::new(4.)) {
                let isolated: Vec<_> = gpu_scene
                    .iter()
                    .map(|t| {
                        pipeline_cpu::display_linear(t, settings.output.gamut_mapping, 4.).unwrap()
                    })
                    .collect();
                eprintln!("Compact isolated display transform comparison L{level}");
                compare(&got, &isolated, false);
            }
            let expected = cpu
                .render_region_as(&image, &settings, level, PixelRect::full(extent), output)
                .unwrap();
            eprintln!("Compact comparison level={level} output={output:?}");
            compare(&got, &expected, output == RenderOutput::Display);
        }
    }
}
