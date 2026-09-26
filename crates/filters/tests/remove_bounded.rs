use compositor::{Raster, Rect, raster::Depth};
use engine_api::tile::Extent;
use filters::remove::{BackendUsed, InpaintModel, Remove, RemoveParams, remove};
use ml_runtime::{ModelRegistry, SessionOptions, Tensor};
use std::sync::atomic::AtomicBool;

#[test]
fn auto_missing_is_offline_cpu_and_explicit_onnx_still_errors() {
    let cache = std::path::PathBuf::from(std::env::var_os("CARGO_TARGET_DIR").unwrap())
        .join(format!("remove-auto-{}", std::process::id()));
    let registry = ModelRegistry::open(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../ml-runtime/models.toml"),
        &cache,
    )
    .unwrap();
    let mut auto = <dyn Remove>::auto(&registry, SessionOptions::cpu()).unwrap();
    let mut input = Raster::new(Extent::new(16, 16), 4, Depth::F32, 0.0);
    input
        .edit_region(Rect::of_extent(input.extent()), 1, |_, _, p| {
            *p = [0.2, 0.2, 0.2, 1.0]
        })
        .unwrap();
    let mut mask = vec![0.0; 256];
    mask[100] = 1.0;
    let out = auto
        .apply(
            &input,
            &mask,
            &RemoveParams::default(),
            &AtomicBool::new(false),
        )
        .unwrap();
    assert_eq!(out.backend, BackendUsed::CpuPatchMatch);
    assert!(out.fallback_reason.is_some());
    let params = RemoveParams {
        backend: filters::remove::Backend::Onnx,
        ..Default::default()
    };
    assert!(
        auto.apply(&input, &mask, &params, &AtomicBool::new(false))
            .is_err()
    );
    assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 0);
    std::fs::remove_dir_all(cache).unwrap();
}

#[test]
fn working_resolution_preserves_thin_mask_and_exterior() {
    struct Bounded;
    impl InpaintModel for Bounded {
        fn inpaint(
            &mut self,
            image: &Tensor,
            mask: &Tensor,
            _: &AtomicBool,
        ) -> engine_api::EngineResult<Tensor> {
            assert_eq!(image.shape(), [1, 3, 256, 512]);
            // A one-pixel feature between sample centers must not disappear.
            assert_eq!(mask.data()[127 * 512 + 255], 1.0);
            assert_eq!(image.data()[127 * 512 + 255], 0.0);
            Ok(Tensor::new(3, 256, 512, vec![0.5; 3 * 256 * 512]).unwrap())
        }
    }
    let mut input = Raster::new(Extent::new(1024, 512), 4, Depth::F32, 0.0);
    input
        .edit_region(Rect::of_extent(input.extent()), 1, |_, _, p| {
            *p = [0.2, 0.2, 0.2, 0.6]
        })
        .unwrap();
    let mut mask = vec![0.0; 1024 * 512];
    mask[255 * 1024 + 511] = 1.0;
    let out = remove(
        &input,
        &mask,
        &RemoveParams {
            dilation: 0,
            ..Default::default()
        },
        Some(&mut Bounded),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(out.backend, BackendUsed::Onnx);
    assert_eq!(out.result.composite.pixel(511, 255)[3], 0.6);
    assert_eq!(out.result.composite.pixel(510, 255), input.pixel(510, 255));
}

#[test]
fn auto_rejects_corrupt_cached_weights_instead_of_silently_falling_back() {
    let cache = std::path::PathBuf::from(std::env::var_os("CARGO_TARGET_DIR").unwrap())
        .join(format!("remove-corrupt-{}", std::process::id()));
    let registry = ModelRegistry::open(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../ml-runtime/models.toml"),
        &cache,
    )
    .unwrap();
    let path = cache.join(format!("{}.onnx", filters::remove::REMOVE_SHA256));
    std::fs::write(&path, b"not the pinned weights").unwrap();
    assert!(<dyn Remove>::auto(&registry, SessionOptions::cpu()).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"not the pinned weights");
    assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 1);
    std::fs::remove_dir_all(cache).unwrap();
}
