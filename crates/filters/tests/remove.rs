use compositor::{Raster, Rect, raster::Depth};
use engine_api::tile::Extent;
use filters::remove::{Backend, BackendUsed, RemoveParams, remove};
use std::sync::atomic::AtomicBool;
#[test]
fn empty_selection_bypasses_missing_weights_and_bad_inference_never_publishes() {
    use filters::remove::InpaintModel;
    use ml_runtime::Tensor;
    let mut input = Raster::new(Extent::new(16, 16), 4, Depth::F32, 0.0);
    input
        .edit_region(Rect::of_extent(input.extent()), 1, |_, _, p| {
            *p = [0.25, 0.25, 0.25, 1.0]
        })
        .unwrap();
    let p = RemoveParams {
        backend: Backend::Onnx,
        dilation: 0,
        ..Default::default()
    };
    let empty = remove(&input, &vec![0.0; 256], &p, None, &AtomicBool::new(false)).unwrap();
    assert!(empty.result.composite.shares_all_tiles_with(&input));
    assert_eq!(empty.backend, BackendUsed::Identity);
    struct Invalid;
    impl InpaintModel for Invalid {
        fn inpaint(
            &mut self,
            _: &Tensor,
            _: &Tensor,
            _: &AtomicBool,
        ) -> engine_api::EngineResult<Tensor> {
            Ok(Tensor::new(3, 16, 16, vec![f32::NAN; 768]).unwrap())
        }
    }
    let mut mask = vec![0.0; 256];
    mask[100] = 1.0;
    assert!(
        remove(
            &input,
            &mask,
            &p,
            Some(&mut Invalid),
            &AtomicBool::new(false)
        )
        .is_err()
    );
    assert!(remove(&input, &mask, &p, None, &AtomicBool::new(false)).is_err());
    struct Cancel;
    impl InpaintModel for Cancel {
        fn inpaint(
            &mut self,
            i: &Tensor,
            _: &Tensor,
            c: &AtomicBool,
        ) -> engine_api::EngineResult<Tensor> {
            c.store(true, std::sync::atomic::Ordering::Relaxed);
            Ok(i.clone())
        }
    }
    assert!(
        remove(
            &input,
            &mask,
            &p,
            Some(&mut Cancel),
            &AtomicBool::new(false)
        )
        .err()
        .unwrap()
        .is_cancelled()
    );
    assert_eq!(input.pixel(4, 6), [0.25, 0.25, 0.25, 1.0]);
    let auto = RemoveParams {
        backend: Backend::Auto,
        ..p
    };
    let out = remove(&input, &mask, &auto, None, &AtomicBool::new(false)).unwrap();
    assert_eq!(out.backend, BackendUsed::CpuPatchMatch);
    assert!(out.fallback_reason.is_some());
}
#[test]
fn actual_onnx_adapter_pads_and_crops_named_tensors() {
    use filters::remove::{InpaintModel, OnnxInpainter};
    use ml_runtime::{Session, SessionOptions, Tensor};
    // Genuine ONNX Identity graph with image+mask inputs, not removal weights.
    const FIXTURE: &[u8] = &[
        8, 8, 18, 20, 116, 101, 115, 115, 101, 114, 97, 45, 116, 101, 115, 116, 45, 102, 105, 120,
        116, 117, 114, 101, 58, 198, 1, 10, 43, 10, 5, 105, 109, 97, 103, 101, 18, 6, 111, 117,
        116, 112, 117, 116, 26, 16, 105, 100, 101, 110, 116, 105, 116, 121, 45, 102, 105, 120, 116,
        117, 114, 101, 34, 8, 73, 100, 101, 110, 116, 105, 116, 121, 18, 19, 114, 101, 109, 111,
        118, 101, 45, 97, 100, 97, 112, 116, 101, 114, 45, 116, 101, 115, 116, 90, 42, 10, 5, 105,
        109, 97, 103, 101, 18, 33, 10, 31, 8, 1, 18, 27, 10, 2, 8, 1, 10, 2, 8, 3, 10, 8, 18, 6,
        104, 101, 105, 103, 104, 116, 10, 7, 18, 5, 119, 105, 100, 116, 104, 90, 41, 10, 4, 109,
        97, 115, 107, 18, 33, 10, 31, 8, 1, 18, 27, 10, 2, 8, 1, 10, 2, 8, 1, 10, 8, 18, 6, 104,
        101, 105, 103, 104, 116, 10, 7, 18, 5, 119, 105, 100, 116, 104, 98, 43, 10, 6, 111, 117,
        116, 112, 117, 116, 18, 33, 10, 31, 8, 1, 18, 27, 10, 2, 8, 1, 10, 2, 8, 3, 10, 8, 18, 6,
        104, 101, 105, 103, 104, 116, 10, 7, 18, 5, 119, 105, 100, 116, 104, 66, 2, 16, 13,
    ];
    let path = std::path::PathBuf::from(std::env::var_os("CARGO_TARGET_DIR").unwrap())
        .join(format!("remove-fixture-{}.onnx", std::process::id()));
    std::fs::write(&path, FIXTURE).unwrap();
    let session = Session::load(&path, SessionOptions::cpu()).unwrap();
    let mut model = OnnxInpainter::from_session(session);
    let image = Tensor::new(
        3,
        11,
        13,
        (0..3 * 11 * 13).map(|i| (i % 127) as f32 / 127.0).collect(),
    )
    .unwrap();
    let mask = Tensor::new(1, 11, 13, vec![0.0; 11 * 13]).unwrap();
    let result = model
        .inpaint(&image, &mask, &AtomicBool::new(false))
        .unwrap();
    assert_eq!(result.shape(), image.shape());
    assert_eq!(result.data(), image.data());
    std::fs::remove_file(path).unwrap();
}
#[test]
fn uninstalled_onnx_slot_is_cleanly_unavailable_without_downloads() {
    use filters::remove::{OnnxInpainter, REMOVE_MODEL_ID, REMOVE_VERSION};
    use ml_runtime::{ModelRegistry, ModelSource, SessionOptions};
    let cache = std::path::PathBuf::from(std::env::var_os("CARGO_TARGET_DIR").unwrap())
        .join(format!("remove-missing-{}", std::process::id()));
    let manifest =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ml-runtime/models.toml");
    let registry = ModelRegistry::open(manifest, &cache).unwrap();
    let slot = registry
        .models()
        .iter()
        .find(|m| m.id == REMOVE_MODEL_ID && m.version == REMOVE_VERSION)
        .unwrap();
    assert_eq!(slot.source, ModelSource::Local);
    assert!(slot.download_url.is_empty());
    let error = match OnnxInpainter::load_local(&registry, SessionOptions::cpu()) {
        Ok(_) => panic!("unverified placeholder must not load"),
        Err(e) => e,
    };
    assert!(
        matches!(error, engine_api::EngineError::NotFound { .. }),
        "{error}"
    );
    assert_eq!(
        std::fs::read_dir(&cache).unwrap().count(),
        0,
        "must not download or cache placeholder weights"
    );
    std::fs::remove_dir_all(cache).unwrap();
}
#[test]
fn model_hook_receives_dilated_mask_and_display_rgb_then_blends_linear() {
    use filters::remove::InpaintModel;
    use ml_runtime::Tensor;
    struct Probe {
        called: bool,
    }
    impl InpaintModel for Probe {
        fn inpaint(
            &mut self,
            image: &Tensor,
            mask: &Tensor,
            _: &AtomicBool,
        ) -> engine_api::EngineResult<Tensor> {
            self.called = true;
            assert_eq!(image.shape(), [1, 3, 16, 16]);
            assert_eq!(mask.shape(), [1, 1, 16, 16]);
            assert_eq!(mask.data()[7 * 16 + 7], 1.0);
            assert_eq!(mask.data()[6 * 16 + 7], 1.0);
            assert_eq!(
                image.data()[7 * 16 + 7],
                0.0,
                "removed pixels must not leak into model"
            );
            assert!(
                (image.data()[0] - 0.5370987).abs() < 1e-5,
                "linear 0.25 encoded to sRGB"
            );
            Ok(Tensor::new(3, 16, 16, vec![0.7353569; 3 * 16 * 16]).unwrap())
        }
    }
    let mut input = Raster::new(Extent::new(16, 16), 4, Depth::F32, 0.0);
    input
        .edit_region(Rect::of_extent(input.extent()), 1, |_, _, p| {
            *p = [0.25, 0.25, 0.25, 0.7]
        })
        .unwrap();
    let mut mask = vec![0.0; 256];
    mask[7 * 16 + 7] = 0.5;
    let mut model = Probe { called: false };
    let params = RemoveParams {
        dilation: 1,
        backend: Backend::Onnx,
        ..Default::default()
    };
    let out = remove(
        &input,
        &mask,
        &params,
        Some(&mut model),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(model.called);
    assert_eq!(out.backend, BackendUsed::Onnx);
    assert!(out.fallback_reason.is_none());
    assert!((out.result.composite.pixel(7, 7)[0] - 0.375).abs() < 1e-5);
    assert_eq!(out.result.composite.pixel(7, 7)[3], 0.7);
    assert_eq!(out.result.composite.pixel(0, 0), input.pixel(0, 0));
}
#[test]
fn cpu_remove_dilates_object_mask_preserves_exterior_and_reports_backend() {
    let mut input = Raster::new(Extent::new(32, 24), 4, Depth::F32, 0.0);
    input
        .edit_region(Rect::of_extent(input.extent()), 1, |x, y, p| {
            *p = if (13..20).contains(&x) && (9..16).contains(&y) {
                [0.9, 0.0, 0.0, 1.0]
            } else {
                [0.4, 0.4, 0.4, 1.0]
            }
        })
        .unwrap();
    let mut mask = vec![0.0; 32 * 24];
    for y in 10..15 {
        for x in 14..19 {
            mask[y * 32 + x] = 1.0;
        }
    }
    let p = RemoveParams {
        dilation: 1,
        backend: Backend::Cpu,
        ..Default::default()
    };
    let out = remove(&input, &mask, &p, None, &AtomicBool::new(false)).unwrap();
    assert_eq!(out.backend, BackendUsed::CpuPatchMatch);
    for y in 9..16 {
        for x in 13..20 {
            assert!((out.result.composite.pixel(x, y)[0] - 0.4).abs() < 0.005);
        }
    }
    assert_eq!(out.result.composite.pixel(12, 8), input.pixel(12, 8));
    assert_eq!(input.pixel(15, 12), [0.9, 0.0, 0.0, 1.0]);
}
