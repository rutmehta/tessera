//! B5-20b: the coarse-lattice solve against the dense solve on a layer under
//! the 16,777,216-vertex cap (4000 × 3000), with the coarse path forced
//! through the smart filter by `adaptive_lattice::with_coarse_budget`.
//!
//! Stated tolerances (the default coarse budget, 2,097,152 vertices):
//! - inverse map (output pixel centre -> source position), where both are
//!   defined: max difference ≤ 0.02 source px;
//! - pixels (straight RGBA, all channels): mean |Δ| ≤ 1e-3 over the whole
//!   layer, max |Δ| ≤ 0.01 away from the coverage boundary (the
//!   transparent-hole edge moves by at most one coarse cell);
//! - coverage: the defined/undefined boundary differs on ≤ 0.1 % of pixels.
mod awa_scene;

use awa_scene::{Scene, context, node, rows};
use compositor::render::smart_filters::SmartFilterEvaluator;
use filters::{CompositorFilters, adaptive_lattice};

const MAP_PX: f64 = 0.02;
const MEAN_ABS: f64 = 1e-3;
const MAX_INTERIOR: f32 = 0.01;
const COVERAGE: f64 = 1e-3;

#[test]
fn coarse_matches_dense_within_the_stated_tolerance() {
    let (w, h) = (4000u32, 3000u32);
    let scene = Scene::new(w, h);
    let input = scene.raster();
    let recipe = scene.recipe();
    let budget = adaptive_lattice::COARSE_VERTICES;

    // Maps: the dense field against the coarse lattice, every 7th pixel.
    let dense_field = recipe.solve().unwrap();
    let lattice = adaptive_lattice::Lattice::solve(&recipe, budget).unwrap();
    assert!(lattice.vertices() <= budget);
    assert!(lattice.factor() < 1.0);
    let (mut worst, mut both, mut differ) = (0.0f64, 0usize, 0usize);
    for y in (0..h).step_by(7) {
        for x in (0..w).step_by(7) {
            let p = [f64::from(x) + 0.5, f64::from(y) + 0.5];
            match (dense_field.inverse(p), lattice.inverse(p)) {
                (Some(a), Some(b)) => {
                    both += 1;
                    worst = worst.max((a[0] - b[0]).hypot(a[1] - b[1]));
                }
                (None, None) => {}
                _ => differ += 1,
            }
        }
    }
    let sampled = (w as usize).div_ceil(7) * (h as usize).div_ceil(7);
    eprintln!(
        "factor {:.4}, {} vertices; map max Δ {worst:.5} px over {both} samples; coverage differs on {differ}/{sampled}",
        lattice.factor(),
        lattice.vertices()
    );
    assert!(both * 2 > sampled, "most of the output is covered");
    assert!(worst <= MAP_PX, "coarse map differs by {worst} px");
    assert!((differ as f64) <= COVERAGE * sampled as f64);

    // Pixels: the dense stage (fine path, under the cap) against the stage
    // with the coarse path forced.
    let dense = CompositorFilters
        .evaluate(&input, &node(&recipe), &context(w, h))
        .unwrap();
    let coarse = adaptive_lattice::with_coarse_budget(budget, || {
        CompositorFilters.evaluate(&input, &node(&recipe), &context(w, h))
    })
    .unwrap();
    assert_eq!(coarse.extent(), dense.extent());
    let (d, c) = (rows(&dense, 3), rows(&coarse, 3));
    let mut sum = 0.0f64;
    let mut interior = 0.0f32;
    for (i, (a, b)) in d.iter().zip(&c).enumerate() {
        let diff = (0..4).map(|k| (a[k] - b[k]).abs()).fold(0.0f32, f32::max);
        sum += (0..4).map(|k| f64::from((a[k] - b[k]).abs())).sum::<f64>() / 4.0;
        // Away from the coverage edge: both fully opaque here and at the
        // neighbours two pixels away on the same row.
        let x = i % w as usize;
        let opaque = |j: usize| d[j][3] >= 1.0 - 1e-6 && c[j][3] >= 1.0 - 1e-6;
        if x >= 2 && x + 2 < w as usize && opaque(i - 2) && opaque(i) && opaque(i + 2) {
            interior = interior.max(diff);
        }
    }
    let mean = sum / d.len() as f64;
    eprintln!("pixels: mean |Δ| {mean:.2e}, interior max |Δ| {interior:.2e}");
    assert!(mean <= MEAN_ABS, "mean abs pixel difference {mean}");
    assert!(interior <= MAX_INTERIOR, "interior max pixel difference {interior}");
    let (v, hz) = scene.straightness(&coarse);
    assert!(v <= 0.5 && hz <= 0.5, "coarse straightness {v} / {hz}");
}

#[test]
fn the_coarse_path_is_deterministic_and_leaves_small_layers_on_the_fine_path() {
    let (w, h) = (1200u32, 900u32);
    let scene = Scene::new(w, h);
    let input = scene.raster();
    let recipe = scene.recipe();
    let run = || {
        adaptive_lattice::with_coarse_budget(200_000, || {
            CompositorFilters.evaluate(&input, &node(&recipe), &context(w, h))
        })
        .unwrap()
    };
    let (a, b) = (run(), run());
    assert_eq!(rows(&a, 1), rows(&b, 1), "byte-identical re-render");
    let l1 = adaptive_lattice::Lattice::solve(&recipe, 200_000).unwrap();
    let l2 = adaptive_lattice::Lattice::solve(&recipe, 200_000).unwrap();
    assert_eq!(l1, l2, "identical lattices");
    assert!(l1.vertices() <= 200_000);
    // Without the hook a layer under the cap renders on the fine path, which
    // differs (slightly) from the coarse one.
    let fine = CompositorFilters
        .evaluate(&input, &node(&recipe), &context(w, h))
        .unwrap();
    assert_ne!(rows(&fine, 1), rows(&a, 1));
    // The hook is scoped: afterwards the fine path is back.
    let again = CompositorFilters
        .evaluate(&input, &node(&recipe), &context(w, h))
        .unwrap();
    assert_eq!(rows(&again, 1), rows(&fine, 1));
}

#[test]
fn lattice_budget_and_factor_follow_the_layer_size() {
    for (w, h) in [(6000usize, 4000usize), (5212, 3468), (4096, 4096), (12001, 997)] {
        let scene = Scene::new(w as u32, h as u32);
        let l = adaptive_lattice::Lattice::solve(&scene.recipe(), adaptive_lattice::COARSE_VERTICES)
            .unwrap();
        assert!(l.vertices() <= adaptive_lattice::COARSE_VERTICES, "{w}×{h}");
        assert!(l.vertices() * 2 > adaptive_lattice::COARSE_VERTICES / 2, "{w}×{h} uses the budget");
        assert!(l.factor() > 0.0 && l.factor() < 1.0);
    }
}
