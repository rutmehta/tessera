use compositor::{
    geom::Rect,
    raster::{Depth, Raster},
};
use engine_api::tile::Extent;
use filters::{Effect, Filter, FilterParams, distort::Distortion};
use std::sync::atomic::AtomicBool;
fn image() -> Raster {
    let mut r = Raster::new(Extent::new(17, 13), 4, Depth::F32, 0.0);
    r.edit_region(Rect::of_extent(r.extent()), 1, |x, y, p| {
        *p = [((x * 13 + y * 7) % 31) as f32 / 31.0, 0.2, 0.4, 0.8]
    })
    .unwrap();
    r
}
#[test]
fn inventory_identity_and_nontrivial_finite() {
    let r = image();
    let c = AtomicBool::new(false);
    for e in Effect::inventory() {
        assert!(
            e.apply(&r, &FilterParams::default(), &c)
                .unwrap()
                .shares_all_tiles_with(&r),
            "{e:?}"
        );
        let p = FilterParams {
            amount: 0.8,
            radius: 1.5,
            angle: 0.5,
            depth: Some(vec![1.0; 17 * 13]),
            ..Default::default()
        };
        let out = e.apply(&r, &p, &c);
        if matches!(e, Effect::OilPaint | Effect::LensFlare | Effect::CameraRaw) {
            assert!(out.is_err());
            continue;
        }
        let out = out.unwrap();
        assert_eq!(out.extent(), r.extent());
        for y in 0..13 {
            for x in 0..17 {
                assert!(out.pixel(x, y).iter().all(|v| v.is_finite()), "{e:?}");
            }
        }
    }
}
#[test]
fn median_removes_impulse_and_dust_threshold_protects_detail() {
    let mut r = Raster::new(Extent::new(9, 9), 4, Depth::F32, 0.0);
    r.edit_region(Rect::new(4, 4, 5, 5), 1, |_, _, p| *p = [1.0; 4])
        .unwrap();
    let c = AtomicBool::new(false);
    let p = FilterParams {
        amount: 1.0,
        radius: 1.0,
        ..Default::default()
    };
    assert_eq!(
        Effect::Median.apply(&r, &p, &c).unwrap().pixel(4, 4)[0],
        0.0
    );
    assert_eq!(
        Effect::DustScratches
            .apply(
                &r,
                &FilterParams {
                    threshold: 2.0,
                    ..p.clone()
                },
                &c
            )
            .unwrap()
            .pixel(4, 4)[0],
        1.0
    );
    assert_eq!(
        Effect::DustScratches.apply(&r, &p, &c).unwrap().pixel(4, 4)[0],
        0.0
    );
}
#[test]
fn noise_is_seeded_and_mono_and_source_immutable() {
    let r = Raster::new(Extent::new(32, 32), 4, Depth::F32, 0.5);
    let copy = r.clone();
    let c = AtomicBool::new(false);
    let p = FilterParams {
        amount: 1.0,
        monochrome: true,
        seed: 12,
        ..Default::default()
    };
    let a = Effect::AddNoise.apply(&r, &p, &c).unwrap();
    let b = Effect::AddNoise.apply(&r, &p, &c).unwrap();
    assert!(r.shares_all_tiles_with(&copy));
    for y in 0..32 {
        for x in 0..32 {
            let v = a.pixel(x, y);
            assert_eq!(v, b.pixel(x, y));
            assert_eq!(v[0], v[1]);
            assert_eq!(v[3], 0.5);
        }
    }
    assert_ne!(a.pixel(2, 3)[0], a.pixel(3, 3)[0]);
}
#[test]
fn point_adjust_and_distort_dispatch() {
    let r = image();
    let c = AtomicBool::new(false);
    let p = FilterParams {
        amount: 1.0,
        adjust: filters::adjust::Adjustment::Invert,
        ..Default::default()
    };
    let out = Effect::Adjust.apply(&r, &p, &c).unwrap();
    assert!((out.pixel(2, 3)[0] + r.pixel(2, 3)[0] - 1.0).abs() < 1e-6);
    let p = FilterParams {
        amount: 1.0,
        distort: filters::distort::DistortParams {
            offset: [2.0, 0.0],
            ..Default::default()
        },
        ..Default::default()
    };
    let out = Effect::Distort(Distortion::Offset)
        .apply(&r, &p, &c)
        .unwrap();
    assert_eq!(out.pixel(4, 3), r.pixel(2, 3));
}
#[test]
fn cancelled_and_invalid_are_errors() {
    let r = image();
    let p = FilterParams {
        amount: 1.0,
        ..Default::default()
    };
    assert!(
        Effect::Gaussian
            .apply(&r, &p, &AtomicBool::new(true))
            .is_err()
    );
    for radius in [-1.0, 251.0, f32::NAN] {
        assert!(
            Effect::Gaussian
                .apply(
                    &r,
                    &FilterParams {
                        radius,
                        ..p.clone()
                    },
                    &AtomicBool::new(false)
                )
                .is_err()
        );
    }
}
