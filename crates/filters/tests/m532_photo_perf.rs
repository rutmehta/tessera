//! Real-photo timing fixture, explicitly supplied so normal tests stay offline.
use compositor::{Raster, Rect, raster::Depth};
use engine_api::tile::Extent;
use filters::remove::{Backend, BackendUsed, RemoveParams, remove};
use std::{sync::atomic::AtomicBool, time::Instant};

#[test]
#[ignore = "18MP real-photo CPU gate; set M532_PHOTO, run release explicitly"]
fn real_photo_18mp_300px_stroke() {
    assert!(!std::hint::black_box(cfg!(debug_assertions)));
    let path = std::env::var("M532_PHOTO").expect("M532_PHOTO must point to a real photograph");
    let photo = image::open(path)
        .unwrap()
        .resize_exact(6000, 3000, image::imageops::FilterType::Triangle)
        .to_rgb8();
    let extent = Extent::new(6000, 3000);
    let mut input = Raster::new(extent, 4, Depth::F32, 0.0);
    input
        .edit_region(Rect::of_extent(extent), 1, |x, y, p| {
            let rgb = photo.get_pixel(x, y).0;
            *p = [
                rgb[0] as f32 / 255.0,
                rgb[1] as f32 / 255.0,
                rgb[2] as f32 / 255.0,
                1.0,
            ];
        })
        .unwrap();
    let mut mask = vec![0.0; extent.area() as usize];
    for y in 1350..1650 {
        mask[y * 6000 + 2850..y * 6000 + 3150].fill(1.0);
    }
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
        "M5-32 real photograph 6000x3000 / 300x300 stroke / default CAF: {:.3} ms",
        elapsed.as_secs_f64() * 1000.0
    );
    assert_eq!(out.backend, BackendUsed::CpuPatchMatch);
    assert_eq!(out.result.composite.pixel(0, 0), input.pixel(0, 0));
    assert!(
        out.result
            .composite
            .pixel(3000, 1500)
            .iter()
            .all(|v| v.is_finite())
    );
    assert!(
        elapsed.as_secs_f64() < 1.0,
        "real-photo CPU Remove exceeds 1s: {elapsed:?}"
    );
}
