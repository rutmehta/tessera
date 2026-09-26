use compositor::{
    document::SmartFilter,
    geom::Rect,
    raster::{Depth, Raster},
    render::smart_filters::SmartFilterEvaluator,
};
use engine_api::tile::Extent;
use filters::{CompositorFilters, Effect, Filter, FilterParams};
use serde_json::json;
use std::sync::atomic::AtomicBool;

fn fixture() -> Raster {
    let mut r = Raster::new(Extent::new(259, 5), 4, Depth::F32, 0.0);
    r.edit_region(Rect::of_extent(r.extent()), 1, |x, _, p| {
        *p = [if x == 255 { 1.0 } else { 0.0 }, 0.2, 0.3, 1.0]
    })
    .unwrap();
    r
}
fn node(name: &str, params: serde_json::Value) -> SmartFilter {
    SmartFilter {
        name: name.into(),
        enabled: true,
        params,
        ..Default::default()
    }
}
#[cfg(not(feature = "camera-raw-filter"))]
#[test]
fn camera_raw_without_feature_is_unsupported() {
    assert!(matches!(
        CompositorFilters.evaluate(&fixture(), &node("camera_raw", json!({})), &context()),
        Err(engine_api::EngineError::Unsupported { .. })
    ));
}
#[cfg(feature = "camera-raw-filter")]
#[test]
fn camera_raw_tone_on_raster_preserves_alpha() {
    let input = fixture();
    let out = CompositorFilters
        .evaluate(
            &input,
            &node("camera_raw", json!({"settings":{"tone":{"exposure":1},"detail":{"sharpening":{"amount":0},"noise_reduction":{"color":0}}},"amount":0.5})),
         &context())
        .unwrap();
    for x in [0, 255, 256, 258] {
        for c in 0..3 {
            assert!((out.pixel(x, 2)[c] - input.pixel(x, 2)[c] * 1.5).abs() < 1e-6);
        }
        assert_eq!(out.pixel(x, 2)[3], input.pixel(x, 2)[3]);
    }
    for p in [
        json!({"exposure":11}),
        json!({"typo":1}),
        json!({"contrast":1e100}),
        json!({"amount":-1}),
    ] {
        assert!(
            CompositorFilters
                .evaluate(&input, &node("camera_raw", p), &context())
                .is_err()
        );
    }
}
#[test]
fn nested_gaussian_cache_and_typed_blend_mask() {
    use compositor::render::smart_filters::FilterBlend;
    use compositor::{Affine, Compositor, DocState, Document, Layer, LayerKind, Mask, SmartObject};
    use std::sync::Arc;
    let input = fixture();
    let e = input.extent();
    let mut child = DocState::new(e, Depth::F32);
    child.root.push(Arc::new(Layer::new(
        "source",
        LayerKind::Pixel(input.clone()),
    )));
    let mut inner = SmartObject::new(child, Affine::IDENTITY);
    let mut filter = node("gaussian_blur", json!({"radius":1.5}));
    filter.blend = FilterBlend {
        opacity: 0.5,
        ..Default::default()
    };
    inner.filters.push(filter);
    let mut mask = Mask::reveal_all(e, Depth::F32);
    mask.raster
        .edit_region(Rect::new(0, 0, 256, 5), 1, |_, _, p| p[0] = 0.0)
        .unwrap();
    inner.filter_mask = Some(mask);
    let mut middle = DocState::new(e, Depth::F32);
    middle.root.push(Arc::new(Layer::new(
        "filtered",
        LayerKind::SmartObject(inner),
    )));
    let outer = SmartObject::new(middle, Affine::IDENTITY);
    let mut state = DocState::new(e, Depth::F32);
    state.root.push(Arc::new(Layer::new(
        "nested",
        LayerKind::SmartObject(outer),
    )));
    let doc = Document::new(state);
    let mut compositor = Compositor::new(32 << 20);
    compositor.set_filter_evaluator(Arc::new(CompositorFilters));
    let first = compositor.render_level_rgba(&doc, 0).unwrap().1;
    let expected = Effect::Gaussian
        .apply(
            &input,
            &FilterParams {
                radius: 1.5,
                amount: 1.0,
                ..Default::default()
            },
            &AtomicBool::new(false),
        )
        .unwrap();
    for x in [254, 255, 256, 257, 258] {
        let want = if x < 256 {
            input.pixel(x, 2)[0]
        } else {
            0.5 * expected.pixel(x, 2)[0]
        };
        assert!((first[((2 * e.width + x) * 4) as usize] - want).abs() < 1e-5);
    }
    assert_eq!(compositor.filter_evaluations(), 1);
    let second = compositor.render_level_rgba(&doc, 0).unwrap().1;
    assert_eq!(first, second);
    assert_eq!(compositor.filter_evaluations(), 1);
}
#[test]
fn rejects_invalid_params_explicitly() {
    let input = fixture();
    for params in [
        json!(null),
        json!([]),
        json!({"typo":1}),
        json!({"amount":2}),
        json!({"seed":-1}),
        json!({"radius":"1"}),
        json!({"radius":1e100}),
        json!({"focus":[0,1e100]}),
        json!({"depth":[-1]}),
        json!({"adjust":{"exposure":{"stops":0,"offset":0,"gamma":0}}}),
    ] {
        assert!(
            CompositorFilters
                .evaluate(&input, &node("gaussian", params.clone()), &context())
                .is_err(),
            "{params}"
        );
    }
    assert!(matches!(
        CompositorFilters.evaluate(&input, &node("made_up", json!({})), &context()),
        Err(engine_api::EngineError::Unsupported { .. })
    ));
    let out = CompositorFilters
        .evaluate(&input, &node("gaussian", json!({"amount":0})), &context())
        .unwrap();
    assert!(out.shares_all_tiles_with(&input));
}
#[test]
fn inventory_names_map_to_existing_effects() {
    for name in [
        "box",
        "motion",
        "radial_spin",
        "radial_zoom",
        "lens_blur",
        "surface_blur",
        "unsharp_mask",
        "smart_sharpen",
        "high_pass",
        "add_noise",
        "reduce_noise",
        "median",
        "dust_scratches",
        "emboss",
        "find_edges",
        "solarize",
        "oil_paint",
        "clouds",
        "difference_clouds",
        "lens_flare",
        "adjust",
        "pinch",
        "spherize",
        "twirl",
        "wave",
        "ripple",
        "polar_to_rectangular",
        "rectangular_to_polar",
        "offset",
    ] {
        CompositorFilters
            .evaluate(&fixture(), &node(name, json!({"amount":0})), &context())
            .unwrap();
    }
    let out = CompositorFilters
        .evaluate(
            &fixture(),
            &node("adjust", json!({"adjust":"invert"})),
            &context(),
        )
        .unwrap();
    assert_eq!(out.pixel(255, 2)[0], 0.0);
}
#[test]
fn gaussian_aliases_default_active_and_cross_tile_halo() {
    let input = fixture();
    let expected = Effect::Gaussian
        .apply(
            &input,
            &FilterParams {
                radius: 1.5,
                amount: 1.0,
                ..Default::default()
            },
            &AtomicBool::new(false),
        )
        .unwrap();
    for name in ["gaussian", "gaussian_blur"] {
        let out = CompositorFilters
            .evaluate(&input, &node(name, json!({"radius":1.5})), &context())
            .unwrap();
        assert!(out.pixel(256, 2)[0] > 0.0);
        for x in 0..259 {
            for c in 0..4 {
                assert!((out.pixel(x, 2)[c] - expected.pixel(x, 2)[c]).abs() < 1e-6);
            }
        }
    }
    assert_eq!(input.pixel(255, 2)[0], 1.0);
}

fn context() -> compositor::render::smart_filters::FilterContext {
    compositor::render::smart_filters::FilterContext {
        profile: None,
        level: 0,
        canvas: engine_api::tile::Extent::new(259, 5),
    }
}
