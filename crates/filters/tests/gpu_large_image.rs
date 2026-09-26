use compositor::raster::{Depth, Raster};
use engine_api::tile::Extent;
use filters::{Effect, FilterParams, adjust::Adjustment, gpu::GpuFilters};
use std::{sync::atomic::AtomicBool, time::Instant};
#[test]
#[ignore = "20 MP real-GPU memory/throughput exercise"]
fn gpu_processes_20mp_without_default_storage_limit() {
    let gpu = GpuFilters::new().expect("real Metal adapter required");
    let input = Raster::new(Extent::new(5000, 4000), 4, Depth::F32, 0.25);
    let p = FilterParams {
        amount: 1.0,
        adjust: Adjustment::Exposure {
            stops: 1.0,
            offset: 0.0,
            gamma: 1.0,
        },
        ..Default::default()
    };
    let start = Instant::now();
    let out = gpu
        .apply(Effect::Adjust, &input, &p, &AtomicBool::new(false))
        .unwrap();
    assert_eq!(out.pixel(4999, 3999), [0.5, 0.5, 0.5, 0.25]);
    assert_eq!(input.tile_count(), 0);
    println!(
        "GPU L0 5000x4000 exposure (upload/dispatch/readback/Raster): {:.3} ms",
        start.elapsed().as_secs_f64() * 1000.0
    );
}
