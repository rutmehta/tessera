use compositor::{Raster, Rect, raster::Depth};
use engine_api::tile::Extent;
use filters::caf::{FillParams, fill};
use std::{sync::atomic::AtomicBool, time::Instant};

#[test]
#[ignore = "24MP end-to-end benchmark; run release explicitly"]
fn caf_24mp_512_hole() {
    let e = Extent::new(6000, 4000);
    let mut input = Raster::new(e, 4, Depth::F32, 0.0);
    input
        .edit_region(Rect::of_extent(e), 1, |x, y, p| {
            let v = 0.35 + ((x % 8 + y % 8) as f32) * 0.015;
            *p = [v, v * 0.8, v * 0.6, 1.];
        })
        .unwrap();
    let mut mask = vec![0.; e.area() as usize];
    for y in 1744..2256 {
        for x in 2744..3256 {
            mask[y * 6000 + x] = 1.;
        }
    }
    input
        .edit_region(Rect::new(2744, 1744, 3256, 2256), 2, |_, _, p| {
            *p = [1., 0., 1., 1.]
        })
        .unwrap();
    let start = Instant::now();
    let result = fill(
        &input,
        &mask,
        &FillParams::default(),
        &AtomicBool::new(false),
    )
    .unwrap();
    let elapsed = start.elapsed();
    std::hint::black_box(result.composite.pixel(3000, 2000));
    eprintln!(
        "CAF 6000x4000, 512x512 hole, default controls: {:.3} ms",
        elapsed.as_secs_f64() * 1000.
    );
    assert!(
        elapsed.as_secs_f64() < 2.0,
        "CAF exceeds 2s end-to-end budget: {elapsed:?}"
    );
}
