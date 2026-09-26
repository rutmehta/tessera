#![cfg(feature = "camera-raw-filter")]
use color_mgmt::{Builtin, Registry, Transform, TransformOptions};
use compositor::{
    Rect,
    document::{ColorProfile, SmartFilter},
    raster::{Depth, Raster},
    render::smart_filters::{FilterContext, SmartFilterEvaluator},
};
use engine_api::{
    EngineError,
    id::ImageId,
    recipe::{DevelopSettings, LocalAdjustment, MaskComponent, MaskKind},
};
use filters::{CompositorFilters, camera_raw};
use image_core::{PixelRect, RawImage, RenderOutput, Renderer, RendererConfig};
use serde_json::json;
use std::sync::Arc;

fn node(settings: &DevelopSettings, amount: Option<f32>) -> SmartFilter {
    let mut params = json!({"settings": settings});
    if let Some(amount) = amount {
        params["amount"] = json!(amount);
    }
    SmartFilter {
        name: "camera_raw".into(),
        enabled: true,
        params,
        ..Default::default()
    }
}
fn settings() -> DevelopSettings {
    let mut s = DevelopSettings::default();
    s.tone.exposure = 0.65;
    s.tone.contrast = 17.;
    s.tone.highlights = -24.;
    s.tone.shadows = 11.;
    s.tone.texture = 13.;
    s.tone.clarity = 8.;
    s.tone.dehaze = 5.;
    s.color.vibrance = 18.;
    s.color.saturation = -6.;
    s.color.hsl.hue.red = 14.;
    s.effects.grain.amount = 7.;
    s.effects.vignette.amount = -12.;
    let mut local = LocalAdjustment::default();
    local.components.push(MaskComponent::new(MaskKind::Linear {
        start: [0.1, 0.2],
        end: [0.9, 0.7],
    }));
    local.params.exposure = 0.4;
    local.params.saturation = 9.;
    s.locals.adjustments.push(local);
    s
}

/// Actual PNG decoding through RgbSource, not the filter's in-memory constructor.
fn decoded_fixture(builtin: Builtin) -> (RawImage, Raster, FilterContext, Transform) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("golden.png");
    image::RgbImage::from_fn(259, 17, |x, y| {
        image::Rgb([
            (20 + (x * 13 + y * 7) % 210) as u8,
            (15 + (x * 3 + y * 19) % 225) as u8,
            (10 + (x * 11 + y * 5) % 230) as u8,
        ])
    })
    .save(&path)
    .unwrap();
    let source = RawImage::open(ImageId(17), path).unwrap();
    let mut registry = Registry::new();
    let profile = registry.builtin(builtin).unwrap();
    let linear = registry.linearized_rgb(&profile).unwrap().unwrap();
    let rec = registry.builtin(Builtin::LinearRec2020).unwrap();
    let to_doc = Transform::new(
        &rec,
        &linear,
        TransformOptions {
            black_point_compensation: false,
            ..Default::default()
        },
    )
    .unwrap();
    let e = source.active_extent();
    let mut raster = Raster::new(e, 4, Depth::F32, 0.);
    raster
        .edit_region(Rect::of_extent(e), 1, |x, y, p| {
            let i = (y * e.width + x) as usize;
            let rgb = to_doc.apply(std::array::from_fn(|c| {
                source.rgb().unwrap().pixels().planes()[c][i]
            }));
            *p = [rgb[0], rgb[1], rgb[2], (x % 5) as f32 / 4.];
        })
        .unwrap();
    let context = FilterContext {
        profile: Some(ColorProfile::from_icc(
            "working",
            profile.icc_bytes().to_vec(),
        )),
        level: 0,
        canvas: e,
    };
    (source, raster, context, to_doc)
}
fn assert_golden(builtin: Builtin) {
    let (source, input, context, to_doc) = decoded_fixture(builtin);
    let settings = settings();
    let renderer = Renderer::new(RendererConfig {
        threads: 1,
        cache_budget_bytes: 0,
        ..Default::default()
    });
    let e = input.extent();
    let mut golden =
        pipeline_cpu::Image::new(e.width, e.height, vec![vec![0.; e.area() as usize]; 3]).unwrap();
    for tile in renderer
        .render_region_as(
            &source,
            &settings,
            0,
            PixelRect::full(e),
            RenderOutput::SceneLinear,
        )
        .unwrap()
    {
        golden.put(&tile).unwrap();
    }
    for amount in [None, Some(0.35), Some(0.)] {
        let filter = node(&settings, amount);
        let roundtrip: SmartFilter =
            serde_json::from_slice(&serde_json::to_vec(&filter).unwrap()).unwrap();
        let output = CompositorFilters
            .evaluate(&input, &roundtrip, &context)
            .unwrap();
        if amount == Some(0.) {
            assert!(output.shares_all_tiles_with(&input));
        }
        for y in 0..e.height {
            for x in 0..e.width {
                let i = (y * e.width + x) as usize;
                let want = to_doc.apply(std::array::from_fn(|c| golden.planes()[c][i]));
                let before = input.pixel(x, y);
                let after = output.pixel(x, y);
                assert_eq!(after[3].to_bits(), before[3].to_bits());
                for c in 0..3 {
                    let expected = before[c] + amount.unwrap_or(1.) * (want[c] - before[c]);
                    assert!(
                        (after[c] - expected).abs() < 0.0003,
                        "{builtin:?} ({x},{y}) channel {c}: {} != {expected}",
                        after[c]
                    );
                }
            }
        }
        assert!(input.shares_all_tiles_with(&input.clone()));
    }
}
#[test]
fn full_develop_matches_rgb_decode_golden_srgb() {
    assert_golden(Builtin::Srgb);
}
#[test]
fn full_develop_matches_rgb_decode_golden_display_p3() {
    assert_golden(Builtin::DisplayP3);
}

#[test]
fn ai_masks_are_explicitly_unsupported_even_at_zero_amount() {
    let mut settings = DevelopSettings::default();
    settings.locals.adjustments.push(LocalAdjustment {
        components: vec![MaskComponent::new(MaskKind::Sky { model: None })],
        ..Default::default()
    });
    let error = camera_raw::parse(&node(&settings, Some(0.)).params).unwrap_err();
    assert!(matches!(error, EngineError::Unsupported { ref what } if what.contains("AI mask")));
}
#[test]
fn params_validate_engine_domains_and_outer_contract() {
    for params in [
        json!({}),
        json!({"settings":{},"amount":-0.1}),
        json!({"settings":{},"amount":1.1}),
        json!({"settings":{},"exposure":1}),
        json!({"settings":{"tone":{"exposure":11}}}),
        json!({"settings":{"color":{"saturation":101}}}),
    ] {
        assert!(camera_raw::parse(&params).is_err(), "{params}");
    }
    assert_eq!(
        camera_raw::parse(&json!({"settings":{}})).unwrap().amount,
        1.
    );
}
#[test]
fn profile_matrix_is_linear_preserves_hdr_and_rejects_missing_icc() {
    let (_, _, mut context, _) = decoded_fixture(Builtin::DisplayP3);
    let (forward, backward) = camera_raw::profile_matrices(&context).unwrap();
    let input = [-0.2, 1.5, 4.];
    let output = backward.apply(forward.apply(input));
    for c in 0..3 {
        assert!((input[c] - output[c]).abs() < 1e-10);
    }
    context.profile.as_mut().unwrap().icc = None;
    assert!(matches!(
        camera_raw::profile_matrices(&context),
        Err(EngineError::Color { .. })
    ));
}
#[test]
fn stack_parameter_edit_undo_and_native_roundtrip() {
    use compositor::{
        Affine, Compositor, DocOp, DocState, Document, Layer, LayerKind, SmartObject,
    };
    let (_, input, context, _) = decoded_fixture(Builtin::DisplayP3);
    let e = input.extent();
    let mut child = DocState::new(e, Depth::F32);
    child.profile = context.profile.clone();
    child
        .root
        .push(Arc::new(Layer::new("source", LayerKind::Pixel(input))));
    let mut doc = Document::new(DocState::new(e, Depth::F32));
    let id = doc
        .apply(DocOp::AddLayer {
            parent: None,
            index: 0,
            layer: Layer::new(
                "smart",
                LayerKind::SmartObject(SmartObject::new(child, Affine::IDENTITY)),
            ),
        })
        .unwrap()
        .created[0];
    let mut s = settings();
    doc.apply(DocOp::SetSmartFilters {
        id,
        filters: vec![node(&s, None)],
        mask: None,
    })
    .unwrap();
    let mut compositor = Compositor::new(32 << 20);
    compositor.set_filter_evaluator(Arc::new(CompositorFilters));
    let first = compositor.render_level_rgba(&doc, 0).unwrap().1;
    s.color.saturation = -65.;
    doc.apply(DocOp::SetSmartFilters {
        id,
        filters: vec![node(&s, None)],
        mask: None,
    })
    .unwrap();
    let edited = compositor.render_level_rgba(&doc, 0).unwrap().1;
    assert_ne!(first, edited);
    assert!(doc.undo());
    assert_eq!(compositor.render_level_rgba(&doc, 0).unwrap().1, first);
    let loaded =
        compositor::format::from_bytes(&compositor::format::to_bytes(doc.state()).unwrap())
            .unwrap();
    let mut fresh = Compositor::new(32 << 20);
    fresh.set_filter_evaluator(Arc::new(CompositorFilters));
    assert_eq!(
        fresh
            .render_level_rgba(&Document::new(loaded), 0)
            .unwrap()
            .1,
        first
    );
}

#[test]
fn in_memory_develop_slider_source_context_and_hdr_parity() {
    use engine_api::{jobs::CancellationToken, tile::Extent};
    let extent = Extent::new(29, 19);
    let mut input = Raster::new(extent, 4, Depth::F32, 0.);
    input
        .edit_region(Rect::of_extent(extent), 7, |x, y, p| {
            *p = [
                x as f32 / 9. - 0.3,
                y as f32 / 7.,
                ((x + y) % 11) as f32 / 5.,
                (x % 4) as f32 / 3.,
            ];
        })
        .unwrap();
    let mut s = settings();
    s.lens.manual_distortion = 9.;
    s.lens.manual_vignetting = 11.;
    s.geometry.crop.rect.right = 0.8;
    for builtin in [Builtin::Srgb, Builtin::DisplayP3] {
        let profile = Registry::new().builtin(builtin).unwrap();
        let context = FilterContext {
            profile: Some(ColorProfile::from_icc(
                "working",
                profile.icc_bytes().to_vec(),
            )),
            level: 2,
            canvas: Extent::new(116, 76),
        };
        for edit in 0..3 {
            s.tone.exposure = edit as f32 * 0.25;
            if edit == 2 {
                // Same revision, different pixels must not hit stale stages.
                input
                    .edit_region(Rect::of_extent(extent), 7, |_, _, p| p[0] += 0.05)
                    .unwrap();
            }
            let (forward, backward) = camera_raw::profile_matrices(&context).unwrap();
            let mut planes = vec![Vec::new(); 3];
            for y in 0..extent.height {
                for x in 0..extent.width {
                    let p = input.pixel(x, y);
                    let rgb = forward.apply([p[0] as f64, p[1] as f64, p[2] as f64]);
                    for c in 0..3 {
                        planes[c].push(rgb[c] as f32);
                    }
                }
            }
            let pixels = pipeline_cpu::Image::new(extent.width, extent.height, planes).unwrap();
            let src = RawImage::from_rgb(
                ImageId(88),
                image_core::RgbSource::from_linear_rec2020(pixels).unwrap(),
            )
            .unwrap();
            let renderer = Renderer::new(RendererConfig {
                cache_budget_bytes: 0,
                ..Default::default()
            });
            let golden = renderer
                .render_rgb_linear(&src, 0, &s, &CancellationToken::new())
                .unwrap();
            let params = node(&s, Some(0.63)).params;
            let cold = camera_raw::evaluate(&input, &params, &context).unwrap();
            let warm = camera_raw::evaluate(&input, &params, &context).unwrap();
            for y in 0..extent.height {
                for x in 0..extent.width {
                    let before = input.pixel(x, y);
                    let actual = cold.pixel(x, y);
                    assert_eq!(actual, warm.pixel(x, y));
                    assert_eq!(actual[3].to_bits(), before[3].to_bits());
                    let expected = if x < golden.width() && y < golden.height() {
                        let i = (y * golden.width() + x) as usize;
                        backward.apply(std::array::from_fn(|c| golden.planes()[c][i] as f64))
                    } else {
                        [0.; 3]
                    };
                    for c in 0..3 {
                        assert_eq!(
                            actual[c],
                            before[c] + 0.63 * (expected[c] as f32 - before[c])
                        );
                    }
                }
            }
        }
    }
}
