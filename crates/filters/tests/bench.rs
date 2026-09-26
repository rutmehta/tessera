//! Run: cargo test -p filters --release --test bench -- --ignored --nocapture
use compositor::{
    geom::Rect,
    raster::{Depth, Raster},
};
use engine_api::tile::Extent;
use filters::{Effect, Filter, FilterParams, adjust::Adjustment, distort::DistortParams};
use std::{sync::atomic::AtomicBool, time::Instant};
#[test]
#[ignore = "20 MP CPU throughput benchmark, not a timing assertion"]
fn filters_20mp_l0_l2() {
    let cancel = AtomicBool::new(false);
    for level in [0, 2] {
        let extent = Extent::new(5000, 4000).at_level(level);
        let mut input = Raster::new(extent, 4, Depth::F32, 0.0);
        input
            .edit_region(Rect::of_extent(extent), 1, |x, y, p| {
                *p = [
                    x as f32 / extent.width as f32,
                    y as f32 / extent.height as f32,
                    ((x * 13 + y * 7) % 31) as f32 / 31.0,
                    1.0,
                ]
            })
            .unwrap();
        let params = FilterParams {
            amount: 1.0,
            radius: 1.0,
            angle: 0.1,
            strength: 0.5,
            depth: Some(vec![1.0; extent.area() as usize]),
            adjust: Adjustment::Exposure {
                stops: 0.5,
                offset: 0.0,
                gamma: 1.0,
            },
            distort: DistortParams {
                amount: 0.2,
                offset: [3.0, 2.0],
                ..Default::default()
            },
            ..Default::default()
        };
        for effect in Effect::inventory() {
            let start = Instant::now();
            let result = effect.apply(&input, &params, &cancel);
            let ms = start.elapsed().as_secs_f64() * 1000.0;
            match result {
                Ok(out) => {
                    std::hint::black_box(out.pixel(extent.width / 2, extent.height / 2));
                    println!(
                        "L{level} {}x{} {effect:?}: {ms:.3} ms",
                        extent.width, extent.height
                    );
                }
                Err(error)
                    if matches!(
                        effect,
                        Effect::OilPaint | Effect::LensFlare | Effect::CameraRaw
                    ) =>
                {
                    println!("L{level} {effect:?}: unavailable ({error}), {ms:.3} ms dispatch")
                }
                Err(error) => panic!("{effect:?}: {error}"),
            }
        }
    }
}
