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

/// The document profile's own transfer curve, evaluated by LCMS independently
/// of the filter: linear twin -> profile encodes, profile -> twin decodes.
/// Outside [0,1] it extends odd-symmetrically below zero and with the endpoint
/// slope above one (the filter's documented float-document contract).
struct Trc {
    encode: Transform,
    decode: Transform,
}
impl Trc {
    fn new(builtin: Builtin) -> Self {
        let mut registry = Registry::new();
        let profile = registry.builtin(builtin).unwrap();
        let linear = registry.linearized_rgb(&profile).unwrap().unwrap();
        let options = TransformOptions {
            black_point_compensation: false,
            ..Default::default()
        };
        Self {
            encode: Transform::new(&linear, &profile, options).unwrap(),
            decode: Transform::new(&profile, &linear, options).unwrap(),
        }
    }
    fn curve(t: &Transform, c: usize, v: f32) -> f32 {
        let at = |v: f32| t.apply([v; 3])[c];
        let a = v.abs();
        let e = if a <= 1. {
            at(a)
        } else {
            let h = 1. / 4095.;
            at(1.) + (a - 1.) * (at(1.) - at(1. - h)) / h
        };
        e.copysign(v)
    }
    fn encode(&self, rgb: [f32; 3]) -> [f32; 3] {
        std::array::from_fn(|c| Self::curve(&self.encode, c, rgb[c]))
    }
    fn decode(&self, rgb: [f32; 3]) -> [f32; 3] {
        std::array::from_fn(|c| Self::curve(&self.decode, c, rgb[c]))
    }
}

/// Actual PNG decoding through RgbSource, not the filter's in-memory constructor.
/// The layer holds the PNG converted into the document profile's ENCODED
/// samples, exactly as document import stores pixels (never linear).
fn decoded_fixture(builtin: Builtin) -> (RawImage, Raster, FilterContext, Transform, Trc) {
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
    let trc = Trc::new(builtin);
    let e = source.active_extent();
    let mut raster = Raster::new(e, 4, Depth::F32, 0.);
    raster
        .edit_region(Rect::of_extent(e), 1, |x, y, p| {
            let i = (y * e.width + x) as usize;
            let rgb = trc.encode(to_doc.apply(std::array::from_fn(|c| {
                source.rgb().unwrap().pixels().planes()[c][i]
            })));
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
    (source, raster, context, to_doc, trc)
}

/// Encoded-sample tolerance of the Develop parity golden. The filter's
/// 4096-entry per-channel LUT (linear interpolation, exact PL inverse) vs the
/// analytic LCMS curve, plus f32 render noise, stays well below 1/4096.
const GOLDEN_TOLERANCE: f32 = 0.0004;

/// The library Develop render of the same PNG (scene-linear Rec.2020, then the
/// document profile's matrix and TRC) is what the filter must produce.
fn assert_golden(builtin: Builtin) {
    let (source, input, context, to_doc, trc) = decoded_fixture(builtin);
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
        let mut max_error = 0f32;
        for y in 0..e.height {
            for x in 0..e.width {
                let i = (y * e.width + x) as usize;
                let want = trc.encode(to_doc.apply(std::array::from_fn(|c| golden.planes()[c][i])));
                let before = input.pixel(x, y);
                let after = output.pixel(x, y);
                assert_eq!(after[3].to_bits(), before[3].to_bits());
                for c in 0..3 {
                    // Amount is the filter's opacity: it blends ENCODED samples.
                    let expected = before[c] + amount.unwrap_or(1.) * (want[c] - before[c]);
                    max_error = max_error.max((after[c] - expected).abs());
                    assert!(
                        (after[c] - expected).abs() < GOLDEN_TOLERANCE,
                        "{builtin:?} ({x},{y}) channel {c}: {} != {expected}",
                        after[c]
                    );
                }
            }
        }
        eprintln!("{builtin:?} amount {amount:?}: max encoded error {max_error}");
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
fn full_develop_matches_rgb_decode_golden_adobe_rgb() {
    assert_golden(Builtin::AdobeRgb);
}

/// Camera Raw's own neutral draft: raw-only detail defaults and Auto lens off.
fn neutral() -> DevelopSettings {
    let mut s = DevelopSettings::default();
    s.detail.sharpening.amount = 0.;
    s.detail.noise_reduction.color = 0.;
    s.lens.profile = engine_api::recipe::settings::LensProfileSource::None;
    s.lens.remove_chromatic_aberration = false;
    s
}
fn flat(extent: engine_api::tile::Extent, value: [f32; 4]) -> Raster {
    let mut raster = Raster::new(extent, 4, Depth::F32, 0.);
    raster
        .edit_region(Rect::of_extent(extent), 1, |_, _, p| *p = value)
        .unwrap();
    raster
}
fn context(builtin: Option<Builtin>, extent: engine_api::tile::Extent) -> FilterContext {
    FilterContext {
        profile: builtin.map(|b| {
            ColorProfile::from_icc(
                "working",
                Registry::new().builtin(b).unwrap().icc_bytes().to_vec(),
            )
        }),
        level: 0,
        canvas: extent,
    }
}

/// B5-28: document samples are encoded. An untagged document is sRGB-ENCODED
/// (not linear sRGB), so +1 EV on encoded mid grey 0.5 doubles decoded light:
/// 0.5 -> 0.21404 linear -> 0.42809 -> 0.68579 encoded.
#[test]
fn plus_one_ev_doubles_decoded_light_of_srgb_encoded_mid_grey() {
    let extent = engine_api::tile::Extent::new(8, 8);
    let input = flat(extent, [0.5, 0.5, 0.5, 1.]);
    let mut s = neutral();
    s.tone.exposure = 1.;
    let trc = Trc::new(Builtin::Srgb);
    for profile in [None, Some(Builtin::Srgb)] {
        let output =
            camera_raw::evaluate(&input, &node(&s, None).params, &context(profile, extent))
                .unwrap();
        let p = output.pixel(3, 3);
        let ratio = trc.decode([p[0], p[1], p[2]])[1] / trc.decode([0.5; 3])[1];
        assert!(
            (ratio - 2.).abs() < 0.01,
            "{profile:?}: encoded {p:?}, decoded ratio {ratio} (want 2)"
        );
        assert!(
            (p[1] - 0.6858).abs() < 0.002,
            "{profile:?}: {p:?} != 0.6858"
        );
    }
}

/// Neutral settings are an identity on encoded samples in every profile,
/// including out-of-range float samples (odd-symmetric / endpoint-slope TRC).
#[test]
fn neutral_settings_are_identity_on_encoded_samples() {
    let extent = engine_api::tile::Extent::new(23, 7);
    let mut input = Raster::new(extent, 4, Depth::F32, 0.);
    input
        .edit_region(Rect::of_extent(extent), 1, |x, y, p| {
            let v = x as f32 / 22.;
            *p = [
                v,
                (v * 0.7 + y as f32 * 0.05).min(1.),
                1. - v,
                (y % 3) as f32 / 2.,
            ];
        })
        .unwrap();
    let params = node(&neutral(), None).params;
    for profile in [
        None,
        Some(Builtin::Srgb),
        Some(Builtin::DisplayP3),
        Some(Builtin::AdobeRgb),
        Some(Builtin::LinearRec2020),
    ] {
        let output = camera_raw::evaluate(&input, &params, &context(profile, extent)).unwrap();
        for y in 0..extent.height {
            for x in 0..extent.width {
                let (a, b) = (input.pixel(x, y), output.pixel(x, y));
                assert_eq!(a[3].to_bits(), b[3].to_bits());
                for c in 0..3 {
                    assert!(
                        (a[c] - b[c]).abs() < 2e-4,
                        "{profile:?} ({x},{y})/{c}: {a:?} -> {b:?}"
                    );
                }
            }
        }
    }
}

/// Amount is the filter's opacity and blends in ENCODED space, like every
/// other document filter's fade: half of +1 EV on 0.5 is (0.5 + 0.6858) / 2.
#[test]
fn amount_blends_encoded_samples() {
    let extent = engine_api::tile::Extent::new(4, 4);
    let input = flat(extent, [0.5, 0.5, 0.5, 1.]);
    let mut s = neutral();
    s.tone.exposure = 1.;
    let context = context(None, extent);
    let full = camera_raw::evaluate(&input, &node(&s, None).params, &context)
        .unwrap()
        .pixel(1, 1);
    let half = camera_raw::evaluate(&input, &node(&s, Some(0.5)).params, &context)
        .unwrap()
        .pixel(1, 1);
    for c in 0..3 {
        assert!(
            (half[c] - (0.5 + full[c]) / 2.).abs() < 1e-6,
            "{half:?} vs {full:?}"
        );
    }
    assert!((half[1] - (0.5 + 0.6858) / 2.).abs() < 0.002, "{half:?}");
}

/// A linear-TRC profile (a float document's LinearRec2020) keeps the exact
/// matrix-only pipeline: its transfer curve is an exact identity.
#[test]
fn linear_profile_transfer_is_exact_identity() {
    use engine_api::jobs::CancellationToken;
    let extent = engine_api::tile::Extent::new(13, 5);
    let mut input = Raster::new(extent, 4, Depth::F32, 0.);
    input
        .edit_region(Rect::of_extent(extent), 1, |x, y, p| {
            *p = [x as f32 / 5. - 0.4, 1.7 - y as f32 * 0.3, 0.25, 1.];
        })
        .unwrap();
    let mut s = neutral();
    s.tone.exposure = 0.5;
    s.color.saturation = 12.;
    let context = context(Some(Builtin::LinearRec2020), extent);
    let (forward, backward) = camera_raw::profile_matrices(&context).unwrap();
    let planes = (0..3)
        .map(|c| {
            (0..extent.area())
                .map(|i| {
                    let p = input.pixel(i as u32 % extent.width, i as u32 / extent.width);
                    forward.apply([p[0] as f64, p[1] as f64, p[2] as f64])[c] as f32
                })
                .collect()
        })
        .collect();
    let pixels = pipeline_cpu::Image::new(extent.width, extent.height, planes).unwrap();
    let src = RawImage::from_rgb(
        ImageId(5),
        image_core::RgbSource::from_linear_rec2020(pixels).unwrap(),
    )
    .unwrap();
    let golden = Renderer::new(RendererConfig {
        cache_budget_bytes: 0,
        ..Default::default()
    })
    .render_rgb_linear(&src, 0, &s, &CancellationToken::new())
    .unwrap();
    let output = camera_raw::evaluate(&input, &node(&s, None).params, &context).unwrap();
    for y in 0..extent.height {
        for x in 0..extent.width {
            let i = (y * extent.width + x) as usize;
            let want = backward.apply(std::array::from_fn(|c| golden.planes()[c][i] as f64));
            let (before, got) = (input.pixel(x, y), output.pixel(x, y));
            for c in 0..3 {
                let expected = before[c] + 1. * (want[c] as f32 - before[c]);
                assert_eq!(got[c], expected, "({x},{y})/{c}");
            }
        }
    }
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
    let (_, _, mut context, _, _) = decoded_fixture(Builtin::DisplayP3);
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
    let (_, input, context, _, _) = decoded_fixture(Builtin::DisplayP3);
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
            // B5-28: samples are encoded; decode with the profile's curve.
            let curves = camera_raw::profile_curves(&context).unwrap();
            let mut planes = vec![Vec::new(); 3];
            for y in 0..extent.height {
                for x in 0..extent.width {
                    let p = input.pixel(x, y);
                    let rgb = forward.apply(curves.decode([p[0], p[1], p[2]]).map(f64::from));
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
                    let expected = curves.encode(expected.map(|v| v as f32));
                    for c in 0..3 {
                        assert_eq!(actual[c], before[c] + 0.63 * (expected[c] - before[c]));
                    }
                }
            }
        }
    }
}
