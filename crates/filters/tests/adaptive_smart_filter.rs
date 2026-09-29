//! B5-20: Adaptive Wide Angle as the `adaptive_wide_angle` smart filter. The
//! stage stores the `transform::adaptive::Adaptive` recipe verbatim as its
//! params and renders `solve()` + the shared displacement renderer on the CPU.
use compositor::{
    Affine, Compositor, DocState, Document, Layer, LayerKind, SmartObject,
    document::SmartFilter,
    geom::Rect,
    raster::{Depth, Raster},
    render::smart_filters::{FilterContext, ResidentFilterEvaluator, SmartFilterEvaluator},
};
use engine_api::{EngineError, tile::Extent};
use filters::CompositorFilters;
use serde_json::json;
use std::sync::Arc;
use transform::adaptive::{Adaptive, CameraModel, LineConstraint, LineOrientation, Projection};

const W: u32 = 96;
const H: u32 = 72;

/// Checker with an alpha ramp in the lower quarter (premultiplication matters).
fn fixture(w: u32, h: u32) -> Raster {
    let mut r = Raster::new(Extent::new(w, h), 4, Depth::F32, 0.0);
    r.edit_region(Rect::of_extent(r.extent()), 1, |x, y, p| {
        let a = if y * 4 >= h * 3 {
            0.25 + 0.75 * x as f32 / w as f32
        } else {
            1.0
        };
        *p = if (x / 6 + y / 6) % 2 == 0 {
            [0.15, 0.25, 0.8, a]
        } else {
            [0.95, 0.9, 0.35, a]
        };
    })
    .unwrap();
    r
}

/// Equidistant fisheye with one vertical edge traced along its curved image.
fn recipe() -> Adaptive {
    let (cx, cy, f_true) = (48.0, 36.0, 52.0);
    let observed = |p: [f64; 2]| {
        let (x, y) = (p[0] - cx, p[1] - cy);
        let r = x.hypot(y);
        let k = if r < 1e-12 {
            1.
        } else {
            f_true * (r / f_true).atan() / r
        };
        [cx + k * x, cy + k * y]
    };
    let mut a = Adaptive::new(
        W as usize,
        H as usize,
        CameraModel::Manual {
            focal_px: 60.,
            center: [cx, cy],
            projection: Projection::Equidistant,
        },
    );
    a.lines.push(LineConstraint {
        points: (0..=8)
            .map(|i| observed([26.0, 14.0 + 5.5 * i as f64]))
            .collect(),
        orientation: LineOrientation::Vertical,
        weight: 1.,
    });
    a.scale = 0.9;
    a
}

fn node(params: serde_json::Value) -> SmartFilter {
    SmartFilter {
        name: "adaptive_wide_angle".into(),
        enabled: true,
        params,
        ..Default::default()
    }
}

fn context(w: u32, h: u32) -> FilterContext {
    FilterContext {
        profile: None,
        level: 0,
        canvas: Extent::new(w, h),
    }
}

/// Independent reference: solve, then the shared inverse-mapped renderer on
/// premultiplied planes, un-premultiplied back.
fn direct(input: &Raster, a: &Adaptive) -> Vec<[f32; 4]> {
    let e = input.extent();
    let (w, h) = (e.width as usize, e.height as usize);
    let mut planes: [Vec<f32>; 4] = Default::default();
    for y in 0..e.height {
        for x in 0..e.width {
            let p = input.pixel(x, y);
            for (c, plane) in planes.iter_mut().enumerate() {
                plane.push(if c == 3 { p[3] } else { p[c] * p[3] });
            }
        }
    }
    let image = transform::Image::new(w, h, planes).unwrap();
    let op = transform::TransformOp {
        version: 1,
        operation: transform::Operation::Displacement(a.solve().unwrap()),
        kernel: transform::Kernel::Automatic,
    };
    let out = op.apply(&image, w, h, 0).unwrap();
    (0..w * h)
        .map(|i| {
            let a = out.planes[3][i];
            let u = |c: usize| if a > 0.0 { out.planes[c][i] / a } else { 0.0 };
            [u(0), u(1), u(2), a]
        })
        .collect()
}

fn pixels(r: &Raster) -> Vec<[f32; 4]> {
    let e = r.extent();
    (0..e.height)
        .flat_map(|y| (0..e.width).map(move |x| (x, y)))
        .map(|(x, y)| r.pixel(x, y))
        .collect()
}

#[test]
fn stage_output_equals_direct_solve_and_apply_and_is_deterministic() {
    let input = fixture(W, H);
    let a = recipe();
    let params = serde_json::to_value(&a).unwrap();
    let first = CompositorFilters
        .evaluate(&input, &node(params.clone()), &context(W, H))
        .unwrap();
    assert_eq!(first.extent(), input.extent());
    let want = direct(&input, &a);
    let got = pixels(&first);
    let worst = got
        .iter()
        .zip(&want)
        .flat_map(|(g, w)| (0..4).map(move |c| (g[c] - w[c]).abs()))
        .fold(0.0f32, f32::max);
    assert!(worst <= 1e-6, "stage differs from direct solve+apply by {worst}");
    assert_ne!(got, pixels(&input), "the fisheye recipe must change pixels");
    let second = CompositorFilters
        .evaluate(&input, &node(params), &context(W, H))
        .unwrap();
    assert_eq!(pixels(&second), got, "re-evaluation is byte-identical");
}

fn invalid(result: Result<Raster, EngineError>) -> String {
    match result {
        Err(EngineError::Unsupported { what }) => panic!("unexpected Unsupported: {what}"),
        Err(e) => e.to_string(),
        Ok(_) => panic!("expected an error"),
    }
}

#[test]
fn strict_params_size_mismatch_and_conflicts_are_invalid() {
    let input = fixture(W, H);
    let eval = |p: serde_json::Value| CompositorFilters.evaluate(&input, &node(p), &context(W, H));
    let good = serde_json::to_value(recipe()).unwrap();

    let mut typo = good.clone();
    typo["focal"] = json!(1);
    assert!(invalid(eval(typo)).contains("focal"));
    let mut nested = good.clone();
    nested["lines"][0]["colour"] = json!("red");
    assert!(invalid(eval(nested)).contains("colour"));
    let mut missing = good.clone();
    missing.as_object_mut().unwrap().remove("lines");
    invalid(eval(missing));
    invalid(eval(json!(null)));

    let mut other = recipe();
    other.source_width = 95;
    other.output_width = 95;
    assert!(invalid(eval(serde_json::to_value(other).unwrap())).contains("size"));
    let mut cropped = recipe();
    cropped.output_height = 70;
    invalid(eval(serde_json::to_value(cropped).unwrap()));

    // A vertical source edge constrained horizontal cannot be satisfied.
    let mut conflict = Adaptive::new(
        W as usize,
        H as usize,
        CameraModel::Manual {
            focal_px: 100.,
            center: [48., 36.],
            projection: Projection::Rectilinear,
        },
    );
    conflict.lines.push(LineConstraint {
        points: vec![[48., 10.], [48., 62.]],
        orientation: LineOrientation::Horizontal,
        weight: 1.,
    });
    invalid(eval(serde_json::to_value(conflict).unwrap()));
    let mut degenerate = recipe();
    degenerate.lines[0].points = vec![[10., 10.], [10., 10.]];
    assert!(invalid(eval(serde_json::to_value(degenerate).unwrap())).contains("degenerate"));
}

#[test]
fn oversized_layer_reports_the_vertex_cap_before_any_work() {
    // 4096 × 4096 has 4097² > 16,777,216 lattice vertices.
    let big = Raster::new(Extent::new(4096, 4096), 4, Depth::F32, 0.0);
    let a = Adaptive::new(
        4096,
        4096,
        CameraModel::Manual {
            focal_px: 3000.,
            center: [2048., 2048.],
            projection: Projection::Rectilinear,
        },
    );
    let message = invalid(CompositorFilters.evaluate(
        &big,
        &node(serde_json::to_value(a).unwrap()),
        &context(4096, 4096),
    ));
    assert!(
        message.contains("4096 × 4096") && message.contains("16,777,216"),
        "{message}"
    );
}

#[test]
fn resident_renderer_leaves_awa_on_the_cpu() {
    let n = node(serde_json::to_value(recipe()).unwrap());
    assert!(!ResidentFilterEvaluator::supports(&CompositorFilters, &n).unwrap());
}

fn smart_doc(filter: SmartFilter) -> DocState {
    let input = fixture(W, H);
    let e = input.extent();
    let mut child = DocState::new(e, Depth::F32);
    child
        .root
        .push(Arc::new(Layer::new("photo", LayerKind::Pixel(input))));
    let mut so = SmartObject::new(child, Affine::IDENTITY);
    so.filters.push(filter);
    let mut state = DocState::new(e, Depth::F32);
    state
        .root
        .push(Arc::new(Layer::new("smart", LayerKind::SmartObject(so))));
    state
}

fn filters_of(state: &DocState) -> Vec<SmartFilter> {
    match &state.root[0].kind {
        LayerKind::SmartObject(so) => so.filters.clone(),
        _ => panic!("not a smart object"),
    }
}

#[test]
fn native_round_trip_keeps_params_exactly_without_a_format_bump() {
    assert_eq!(compositor::format::FORMAT_VERSION, 1);
    let mut a = recipe();
    a.lines.push(LineConstraint {
        points: vec![
            [12.25, 60.125],
            [30.1, 61.7],
            [48.000_000_000_000_01, 62.3],
            [66.9, 61.6],
            [84.3, 60.2],
        ],
        orientation: LineOrientation::Straight,
        weight: 0.75,
    });
    a.crop = [1.5, -0.3];
    let params = serde_json::to_value(&a).unwrap();
    let state = smart_doc(node(params.clone()));
    let back = compositor::format::from_bytes(&compositor::format::to_bytes(&state).unwrap())
        .unwrap();
    let stored = filters_of(&back);
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].name, "adaptive_wide_angle");
    assert_eq!(stored[0].params, params, "params are stored verbatim");
    let decoded: Adaptive = serde_json::from_value(stored[0].params.clone()).unwrap();
    assert_eq!(decoded, a, "f64 values round-trip bit-exactly");

    // The reopened document renders the same as the original.
    let mut c = Compositor::new(32 << 20);
    c.set_filter_evaluator(Arc::new(CompositorFilters));
    let before = c.render_level_rgba(&Document::new(state), 0).unwrap().1;
    let mut c = Compositor::new(32 << 20);
    c.set_filter_evaluator(Arc::new(CompositorFilters));
    let after = c.render_level_rgba(&Document::new(back), 0).unwrap().1;
    assert_eq!(before, after);
}

#[test]
fn unknown_filter_ids_degrade_the_same_way() {
    // An older build has no `adaptive_wide_angle` branch: to it the stage is
    // just another unknown id. The stage uses only the generic SmartFilter
    // fields, and an unknown id with the same params is `Unsupported`.
    let params = serde_json::to_value(recipe()).unwrap();
    let stage = serde_json::to_value(node(params.clone())).unwrap();
    let mut keys: Vec<_> = stage.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(keys, ["blend", "enabled", "name", "params"]);
    let mut future = node(params);
    future.name = "some_future_filter".into();
    assert!(matches!(
        CompositorFilters.evaluate(&fixture(W, H), &future, &context(W, H)),
        Err(EngineError::Unsupported { .. })
    ));
    // Loading keeps an unknown stage verbatim (no data loss).
    let state = smart_doc(future.clone());
    let back = compositor::format::from_bytes(&compositor::format::to_bytes(&state).unwrap())
        .unwrap();
    assert_eq!(filters_of(&back), vec![future]);
}
