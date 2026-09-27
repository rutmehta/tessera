use compositor::{Raster, Rect, raster::Depth};
use engine_api::tile::Extent;
use filters::{
    caf::{ColourAdaptation, FillParams},
    remove::{Backend, RemoveParams, remove},
};
use std::{sync::atomic::AtomicBool, time::Instant};

#[test]
#[ignore = "18MP release performance gate"]
fn remove_18mp_300px_stroke() {
    let e = Extent::new(6000, 3000);
    let mut input = Raster::new(e, 4, Depth::F32, 0.0);
    input
        .edit_region(Rect::of_extent(e), 1, |x, y, p| {
            let v = 0.35 + ((x % 8 + y % 8) as f32) * 0.015;
            *p = [v, v * 0.8, v * 0.6, 1.0];
        })
        .unwrap();
    let mut mask = vec![0.0; e.area() as usize];
    for y in 1350..1650 {
        for x in 2850..3150 {
            mask[y * 6000 + x] = 1.0;
        }
    }
    input
        .edit_region(Rect::new(2850, 1350, 3150, 1650), 2, |_, _, p| {
            *p = [1.0, 0.0, 1.0, 1.0]
        })
        .unwrap();
    let start = Instant::now();
    let out = remove(
        &input,
        &mask,
        &RemoveParams::default(),
        None,
        &AtomicBool::new(false),
    )
    .unwrap();
    let elapsed = start.elapsed();
    let mut error = 0.0f64;
    for y in 1350..1650 {
        for x in 2850..3150 {
            let expected = 0.35 + ((x % 8 + y % 8) as f32) * 0.015;
            error += f64::from((out.result.composite.pixel(x, y)[0] - expected).abs());
        }
    }
    eprintln!(
        "REMOVE 6000x3000 300x300 hole default+dilation: {:.3} ms, MAE {:.6}",
        elapsed.as_secs_f64() * 1000.0,
        error / 90000.0
    );
    assert!(error / 90000.0 < 0.01);
    assert_eq!(out.result.composite.pixel(0, 0), input.pixel(0, 0));
    assert!(
        elapsed.as_secs_f64() < 1.0,
        "18MP Remove exceeds 1s: {elapsed:?}"
    );
}

#[test]
fn distant_sampling_soft_coverage_and_partial_edge_tiles_survive_roi() {
    use filters::caf::{SamplingArea, fill};
    let e = Extent::new(1801, 321);
    let mut input = Raster::new(e, 4, Depth::F32, 0.0);
    input
        .edit_region(Rect::of_extent(e), 9, |x, _, p| {
            *p = if x < 1100 {
                [0.2, 0.3, 0.4, 0.8]
            } else {
                [0.9, 0.7, 0.5, 1.0]
            };
        })
        .unwrap();
    let mut mask = vec![0.0; e.area() as usize];
    for y in 315..321 {
        for x in 1795..1801 {
            mask[y * 1801 + x] = 0.25;
        }
    }
    let mut custom = vec![0.0; mask.len()];
    for y in 50..57 {
        for x in 900..907 {
            custom[y * 1801 + x] = 1.0;
        }
    }
    for sampling in [
        SamplingArea::Rect(Rect::new(900, 50, 907, 57)),
        SamplingArea::Custom(custom),
    ] {
        let params = RemoveParams {
            backend: Backend::Cpu,
            dilation: 0,
            fill: FillParams {
                sampling,
                colour_adaptation: ColourAdaptation::None,
                output_new_layer: true,
                ..Default::default()
            },
        };
        let out = remove(&input, &mask, &params, None, &AtomicBool::new(false)).unwrap();
        let reference = fill(&input, &mask, &params.fill, &AtomicBool::new(false)).unwrap();
        assert_eq!(out.result.composite.extent(), e);
        assert_eq!(out.result.composite.slot(0, 0).unwrap().rev, 9);
        assert_eq!(out.result.composite.slot(7, 1).unwrap().rev, 10);
        assert_eq!(
            out.result.composite.tile(7, 1).unwrap().coord(),
            engine_api::tile::TileCoord::new(0, 7, 1)
        );
        for (x, y) in [(1800, 320), (1795, 315), (1794, 315), (900, 50), (0, 0)] {
            assert_eq!(
                out.result.composite.pixel(x, y),
                reference.composite.pixel(x, y)
            );
            assert_eq!(
                out.result.new_layer.as_ref().unwrap().pixel(x, y),
                reference.new_layer.as_ref().unwrap().pixel(x, y)
            );
        }
        assert_eq!(input.pixel(1800, 320), [0.9, 0.7, 0.5, 1.0]);
    }
}

#[test]
fn outside_sampling_rect_is_an_error_even_at_coordinate_limits() {
    let input = Raster::new(Extent::new(1024, 32), 4, Depth::F32, 0.2);
    let mut mask = vec![0.0; 1024 * 32];
    mask[16 * 1024 + 800] = 1.0;
    let params = RemoveParams {
        backend: Backend::Cpu,
        fill: FillParams {
            sampling: filters::caf::SamplingArea::Rect(Rect::new(i64::MIN, 0, i64::MIN + 1, 32)),
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(remove(&input, &mask, &params, None, &AtomicBool::new(false)).is_err());
}

#[test]
fn auto_sampling_is_local_to_stroke() {
    // Identical local neighborhoods must not change with unrelated distant paint.
    let e = Extent::new(1000, 64);
    let make = |far: f32| {
        let mut r = Raster::new(e, 4, Depth::F32, 0.0);
        r.edit_region(Rect::of_extent(e), 1, |x, y, p| {
            let v = if x < 700 {
                0.2 + ((x * 17 + y * 13) % 31) as f32 / 100.0
            } else {
                far
            };
            *p = [v, v, v, 1.0];
        })
        .unwrap();
        r
    };
    let mut mask = vec![0.0; e.area() as usize];
    for y in 24..40 {
        for x in 100..116 {
            mask[y * 1000 + x] = 1.0;
        }
    }
    let p = RemoveParams {
        dilation: 0,
        backend: Backend::Cpu,
        fill: FillParams {
            colour_adaptation: ColourAdaptation::None,
            ..Default::default()
        },
    };
    let a = remove(&make(0.0), &mask, &p, None, &AtomicBool::new(false)).unwrap();
    let b = remove(&make(0.35), &mask, &p, None, &AtomicBool::new(false)).unwrap();
    for y in 24..40 {
        for x in 100..116 {
            assert_eq!(
                a.result.composite.pixel(x, y),
                b.result.composite.pixel(x, y)
            );
        }
    }
}
