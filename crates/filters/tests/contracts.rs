use compositor::{
    geom::Rect,
    raster::{Depth, Raster},
};
use engine_api::tile::Extent;
use filters::{Effect, Filter, FilterParams, Halo};
use std::sync::atomic::AtomicBool;
#[test]
fn gaussian_independent_f64_scalar_oracle() {
    let mut input = Raster::new(Extent::new(11, 9), 4, Depth::F32, 0.0);
    input
        .edit_region(Rect::of_extent(input.extent()), 1, |x, y, p| {
            *p = [((x * 13 + y * 7) % 31) as f32 / 31.0, 0.2, 0.4, 0.8]
        })
        .unwrap();
    let cancel = AtomicBool::new(false);
    for sigma in [0.1_f64, 0.5, 1.25, 3.0] {
        let out = Effect::Gaussian
            .apply(
                &input,
                &FilterParams {
                    amount: 1.0,
                    radius: sigma as f32,
                    ..Default::default()
                },
                &cancel,
            )
            .unwrap();
        let radius = (3.0 * sigma).ceil() as i32;
        for y in 0_i32..9 {
            for x in 0_i32..11 {
                let mut sum = 0.0_f64;
                let mut total = 0.0_f64;
                for dy in -radius..=radius {
                    for dx in -radius..=radius {
                        let weight = (-(dx * dx + dy * dy) as f64 / (2.0 * sigma * sigma)).exp();
                        sum += weight
                            * f64::from(
                                input.pixel(
                                    (x + dx).clamp(0, 10) as u32,
                                    (y + dy).clamp(0, 8) as u32,
                                )[0],
                            );
                        total += weight;
                    }
                }
                assert!((f64::from(out.pixel(x as u32, y as u32)[0]) - sum / total).abs() < 1e-5);
            }
        }
    }
}
#[test]
fn independent_large_halos_and_nonfloat_depths() {
    let cancel = AtomicBool::new(false);
    for depth in [Depth::U8, Depth::U16, Depth::F32] {
        for channels in [1, 3, 4] {
            let mut r = Raster::new(Extent::new(273, 9), channels, depth, 0.25);
            r.edit_region(Rect::new(250, 1, 260, 8), 5, |x, _, p| {
                *p = [(x % 5) as f32 / 4.0, 0.3, 0.7, 0.8]
            })
            .unwrap();
            let p = FilterParams {
                amount: 1.0,
                radius: 12.0,
                ..Default::default()
            };
            assert_eq!(Effect::Gaussian.halo(&p), Halo::Radius(36));
            let a = Effect::Gaussian.apply(&r, &p, &cancel).unwrap();
            let b = Effect::Gaussian.apply_tiled(&r, &p, &cancel).unwrap();
            for y in 0..9 {
                for x in 0..273 {
                    assert_eq!(a.pixel(x, y), b.pixel(x, y));
                }
            }
            assert_eq!(a.depth(), depth);
            assert_eq!(a.channels(), channels);
            assert_eq!(a.max_rev(), 6);
            assert_eq!(r.max_rev(), 5);
        }
    }
}
#[test]
fn zero_effect_controls_are_pixel_identities_at_full_opacity() {
    let mut r = Raster::new(Extent::new(8, 7), 4, Depth::F32, 0.0);
    r.edit_region(Rect::of_extent(r.extent()), 1, |x, y, p| {
        *p = [x as f32 / 8.0, y as f32 / 7.0, 1.2, 0.5]
    })
    .unwrap();
    let c = AtomicBool::new(false);
    let p = FilterParams {
        amount: 1.0,
        radius: 0.0,
        strength: 0.0,
        angle: 0.0,
        ..Default::default()
    };
    for e in [
        Effect::Gaussian,
        Effect::Box,
        Effect::Motion,
        Effect::RadialSpin,
        Effect::RadialZoom,
        Effect::SurfaceBlur,
        Effect::UnsharpMask,
        Effect::SmartSharpen,
        Effect::AddNoise,
        Effect::ReduceNoise,
        Effect::Median,
        Effect::DustScratches,
        Effect::Adjust,
    ] {
        let out = e.apply(&r, &p, &c).unwrap();
        for y in 0..7 {
            for x in 0..8 {
                assert_eq!(out.pixel(x, y), r.pixel(x, y), "{e:?}");
            }
        }
    }
}
