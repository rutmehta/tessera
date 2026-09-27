use compositor::{Raster, Rect, raster::Depth};
use engine_api::tile::Extent;
use filters::remove::{Backend, BackendUsed, RemoveParams, remove};
use std::{sync::atomic::AtomicBool, time::Instant};

#[test]
#[ignore = "18MP CPU timing gate; run with --release --ignored --nocapture"]
fn remove_18mp_300px_stroke_under_one_second() {
    assert!(
        !std::hint::black_box(cfg!(debug_assertions)),
        "benchmark requires --release"
    );
    let extent = Extent::new(6000, 3000);
    let mut input = Raster::new(extent, 4, Depth::F32, 0.0);
    input
        .edit_region(Rect::of_extent(extent), 1, |x, y, p| {
            let v = 0.35 + ((x % 8 + y % 8) as f32) * 0.015;
            *p = [v, v * 0.8, v * 0.6, 1.0];
        })
        .unwrap();
    let mut mask = vec![0.0; extent.area() as usize];
    // A 300px square brush footprint (more coverage than a 300px disc).
    for y in 1350..1650 {
        mask[y * 6000 + 2850..y * 6000 + 3150].fill(1.0);
    }
    input
        .edit_region(Rect::new(2850, 1350, 3150, 1650), 2, |_, _, p| {
            *p = [1.0, 0.0, 1.0, 1.0];
        })
        .unwrap();
    let start = Instant::now();
    let out = remove(
        &input,
        &mask,
        &RemoveParams {
            backend: Backend::Cpu,
            ..Default::default()
        },
        None,
        &AtomicBool::new(false),
    )
    .unwrap();
    let elapsed = start.elapsed();
    eprintln!(
        "M5-32 CPU Remove 6000x3000 / 300x300 stroke / dilation=2 / default CAF: {:.3} ms",
        elapsed.as_secs_f64() * 1000.0
    );
    assert_eq!(out.backend, BackendUsed::CpuPatchMatch);
    assert_eq!(out.result.composite.pixel(0, 0), input.pixel(0, 0));
    let mut error = 0.0;
    for y in 1350..1650 {
        for x in 2850..3150 {
            let expected = 0.35 + ((x % 8 + y % 8) as f32) * 0.015;
            error += (out.result.composite.pixel(x, y)[0] - expected).abs();
        }
    }
    assert!(error / 90_000.0 < 0.01, "texture MAE {}", error / 90_000.0);
    assert!(
        elapsed.as_secs_f64() < 1.0,
        "CPU Remove exceeds 1s: {elapsed:?}"
    );
}
