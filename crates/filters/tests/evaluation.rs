use compositor::{
    geom::Rect,
    raster::{Depth, Raster},
};
use engine_api::tile::Extent;
use filters::{
    CameraRawFilter, CameraRawProcessor, Effect, Filter, FilterParams, SmartFilter, SmartFilters,
};
use std::sync::atomic::AtomicBool;
fn fixture() -> Raster {
    let mut r = Raster::new(Extent::new(273, 261), 4, Depth::F32, 0.0);
    r.edit_region(Rect::of_extent(r.extent()), 1, |x, y, p| {
        *p = [((x * 13 + y * 7) % 31) as f32 / 31.0, 0.2, 0.4, 0.8]
    })
    .unwrap();
    r
}
#[test]
fn actual_halo_tiles_equal_whole_image() {
    let r = fixture();
    let cancel = AtomicBool::new(false);
    let p = FilterParams {
        radius: 1.25,
        amount: 0.7,
        angle: 0.7,
        ..Default::default()
    };
    for e in [
        Effect::Gaussian,
        Effect::Box,
        Effect::Motion,
        Effect::SurfaceBlur,
        Effect::UnsharpMask,
        Effect::SmartSharpen,
        Effect::HighPass,
        Effect::ReduceNoise,
        Effect::Median,
        Effect::DustScratches,
        Effect::Emboss,
        Effect::FindEdges,
        Effect::Solarize,
        Effect::Adjust,
    ] {
        let a = e.apply(&r, &p, &cancel).unwrap();
        let b = e.apply_tiled(&r, &p, &cancel).unwrap();
        for y in 0..261 {
            for x in 0..273 {
                for c in 0..4 {
                    assert!(
                        (a.pixel(x, y)[c] - b.pixel(x, y)[c]).abs() < 1e-5,
                        "{e:?} {x} {y} {c}"
                    );
                }
            }
        }
    }
}
#[test]
fn smart_filters_mask_disable_and_source_retention() {
    let r = fixture();
    let original = r.clone();
    let cancel = AtomicBool::new(false);
    let mut mask = Raster::new(r.extent(), 1, Depth::F32, 0.0);
    mask.edit_region(Rect::new(0, 0, 2, 2), 1, |_, _, p| p[0] = 0.5)
        .unwrap();
    let node = SmartFilter {
        enabled: true,
        effect: Effect::Adjust,
        params: FilterParams {
            amount: 1.0,
            adjust: filters::adjust::Adjustment::Invert,
            ..Default::default()
        },
        mask: Some(mask),
    };
    let stack = SmartFilters {
        filters: vec![node.clone()],
    };
    let out = stack.apply(&r, &cancel).unwrap();
    assert!((out.pixel(0, 0)[0] - 0.5).abs() < 1e-6);
    assert_eq!(out.pixel(3, 3), r.pixel(3, 3));
    assert!(r.shares_all_tiles_with(&original));
    let off = SmartFilters {
        filters: vec![SmartFilter {
            enabled: false,
            ..node
        }],
    };
    assert!(off.apply(&r, &cancel).unwrap().shares_all_tiles_with(&r));
}
struct Exposure;
impl CameraRawProcessor for Exposure {
    fn process(&self, input: &Raster, cancel: &AtomicBool) -> engine_api::EngineResult<Raster> {
        Effect::Adjust.apply(
            input,
            &FilterParams {
                amount: 1.0,
                adjust: filters::adjust::Adjustment::Exposure {
                    stops: 1.0,
                    offset: 0.0,
                    gamma: 1.0,
                },
                ..Default::default()
            },
            cancel,
        )
    }
}
#[test]
fn camera_raw_is_explicit_injected_full_image_barrier() {
    let r = fixture();
    let c = AtomicBool::new(false);
    let p = FilterParams {
        amount: 0.5,
        ..Default::default()
    };
    let f = CameraRawFilter {
        processor: &Exposure,
    };
    let out = f.apply(&r, &p, &c).unwrap();
    assert_eq!(f.halo(&p), filters::Halo::WholeImage);
    assert!((out.pixel(3, 3)[0] - 1.5 * r.pixel(3, 3)[0]).abs() < 1e-6);
}
