//! B5-20b: Adaptive Wide Angle on real photo sizes. Layers over the dense
//! 16,777,216-vertex lattice (4095 × 4095) render through the coarse-lattice
//! solve, transparently to the smart filter: same stored recipe, full-size
//! output, straightened constraint lines, deterministic.
mod awa_scene;

use awa_scene::{Scene, context, node, rows};
use compositor::{
    raster::{Depth, Raster},
    render::smart_filters::SmartFilterEvaluator,
};
use engine_api::tile::Extent;
use filters::CompositorFilters;
use transform::adaptive::{Adaptive, CameraModel, Projection};

/// Straightened constraint lines: max − min of the stripe centroid along the
/// traced range, in output pixels (uncorrected tilt is ≈ 0.008 · 0.55 · H).
const STRAIGHT_PX: f64 = 0.5;

fn real_size(w: u32, h: u32) {
    let scene = Scene::new(w, h);
    let input = scene.raster();
    let recipe = scene.recipe();
    let t = std::time::Instant::now();
    let out = CompositorFilters
        .evaluate(&input, &node(&recipe), &context(w, h))
        .unwrap_or_else(|e| panic!("{w} × {h} must render: {e}"));
    eprintln!("{w} × {h}: {:.2} s", t.elapsed().as_secs_f64());
    assert_eq!(out.extent(), Extent::new(w, h));
    let (v, hz) = scene.straightness(&out);
    eprintln!("{w} × {h}: vertical spread {v:.3} px, horizontal spread {hz:.3} px");
    assert!(v <= STRAIGHT_PX, "vertical constraint bends by {v} px");
    assert!(hz <= STRAIGHT_PX, "horizontal constraint bends by {hz} px");
    // Deterministic: a second evaluation is byte-identical.
    let again = CompositorFilters
        .evaluate(&input, &node(&recipe), &context(w, h))
        .unwrap();
    assert_eq!(rows(&again, 97), rows(&out, 97));
}

#[test]
fn a_24_megapixel_layer_renders_with_straight_constraints() {
    real_size(6000, 4000);
}

#[test]
fn a_sample_dng_sized_layer_renders_with_straight_constraints() {
    // fixtures/raw/sample.dng develops to 5212 × 3468.
    real_size(5212, 3468);
}

#[test]
fn layers_over_100_megapixels_are_still_refused_before_any_work() {
    let big = Raster::new(Extent::new(12000, 9000), 4, Depth::F32, 0.0);
    let a = Adaptive::new(
        12000,
        9000,
        CameraModel::Manual {
            focal_px: 5000.,
            center: [6000., 4500.],
            projection: Projection::Rectilinear,
        },
    );
    let message = CompositorFilters
        .evaluate(&big, &node(&a), &context(12000, 9000))
        .expect_err("over 100 MP is refused")
        .to_string();
    assert!(
        message.contains("100 megapixels") && message.contains("12000 × 9000"),
        "{message}"
    );
}

#[test]
fn a_cancel_stops_the_coarse_render() {
    use engine_api::{EngineError, jobs::CancellationToken};
    let (w, h) = (6000u32, 4000u32);
    let recipe = Scene::new(w, h).recipe();
    let input = Raster::new(Extent::new(w, h), 4, Depth::F32, 0.5);
    let token = CancellationToken::new();
    let later = token.clone();
    let canceller = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(200));
        later.cancel();
    });
    let t = std::time::Instant::now();
    let result =
        CompositorFilters.evaluate_with_cancel(&input, &node(&recipe), &context(w, h), &token);
    eprintln!("cancelled after {:.2} s", t.elapsed().as_secs_f64());
    canceller.join().unwrap();
    assert!(
        matches!(result, Err(EngineError::Cancelled)),
        "{:?}",
        result.map(|r| r.extent())
    );
}
