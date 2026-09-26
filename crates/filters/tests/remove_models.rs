//! Real pinned weights only; no network unless TESSERA_REMOVE_MODELS=1.
use compositor::{Raster, Rect, raster::Depth};
use engine_api::tile::Extent;
use filters::remove::{
    BackendUsed, CpuPatchMatch, OnnxInpainter, REMOVE_SHA256, Remove, RemoveParams,
};
use ml_runtime::{ModelRegistry, SessionOptions};
use std::{path::PathBuf, sync::atomic::AtomicBool, time::Instant};

// Do not benchmark CoreML while a second LaMa session saturates the CPU.
static MODEL_TEST: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn load(registry: &ModelRegistry, options: SessionOptions) -> OnnxInpainter {
    if std::env::var("TESSERA_REMOVE_MODELS").as_deref() == Ok("1") {
        OnnxInpainter::load(registry, options).unwrap()
    } else {
        OnnxInpainter::load_local(registry, options).unwrap()
    }
}

fn registry() -> Option<ModelRegistry> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let cache = std::env::var_os("TESSERA_REMOVE_MODEL_CACHE")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("tools/orchestrate/wp/M3-20/.cache"));
    let fetch = std::env::var("TESSERA_REMOVE_MODELS").as_deref() == Ok("1");
    if !fetch && !cache.join(format!("{REMOVE_SHA256}.onnx")).is_file() {
        eprintln!(
            "SKIP: pinned LaMa weights absent; set TESSERA_REMOVE_MODELS=1 to fetch or TESSERA_REMOVE_MODEL_CACHE to an existing cache"
        );
        return None;
    }
    Some(ModelRegistry::open(root.join("crates/ml-runtime/models.toml"), cache).unwrap())
}

fn scene(size: u32) -> (Raster, Vec<f32>) {
    let mut image = Raster::new(Extent::new(size, size), 4, Depth::F32, 0.0);
    image
        .edit_region(Rect::of_extent(image.extent()), 1, |x, y, p| {
            let u = x as f32 / size as f32;
            let v = y as f32 / size as f32;
            let texture = 0.035 * (u * 120.0).sin() * (v * 90.0).cos();
            let base = 0.18 + 0.28 * u + 0.10 * v + texture;
            *p = [base, base * 0.85, base * 0.65, 0.8];
        })
        .unwrap();
    let mut mask = vec![0.0; (size * size) as usize];
    let lo = size * 3 / 8;
    let hi = size * 5 / 8;
    image
        .edit_region(
            Rect::new(lo as i64, lo as i64, hi as i64, hi as i64),
            2,
            |x, y, p| {
                *p = [0.95, 0.01, 0.02, 0.8];
                mask[y as usize * size as usize + x as usize] = 1.0;
            },
        )
        .unwrap();
    (image, mask)
}

fn stats(image: &Raster, points: &[(u32, u32)]) -> [f64; 6] {
    let mut out = [0.0; 6];
    for &(x, y) in points {
        let p = image.pixel(x, y);
        for c in 0..3 {
            out[c] += p[c] as f64;
            out[c + 3] += (p[c] as f64).powi(2);
        }
    }
    for c in 0..3 {
        out[c] /= points.len() as f64;
        out[c + 3] = (out[c + 3] / points.len() as f64 - out[c].powi(2))
            .max(0.0)
            .sqrt();
    }
    out
}
fn distance(a: [f64; 6], b: [f64; 6]) -> f64 {
    a.iter().zip(b).map(|(a, b)| (a - b).abs()).sum()
}

#[test]
fn cached_lama_removes_object_and_beats_patchmatch_statistics() {
    let _exclusive = MODEL_TEST.lock().unwrap_or_else(|e| e.into_inner());
    let Some(registry) = registry() else {
        return;
    };
    let mut model = load(&registry, SessionOptions::cpu());
    let (image, mask) = scene(256);
    let params = RemoveParams {
        dilation: 2,
        ..Default::default()
    };
    let cancel = AtomicBool::new(false);
    let start = Instant::now();
    let neural = model.apply(&image, &mask, &params, &cancel).unwrap();
    println!("LaMa CPU pipeline {:?}", start.elapsed());
    assert_eq!(neural.backend, BackendUsed::Onnx);
    let cpu = CpuPatchMatch
        .apply(&image, &mask, &params, &cancel)
        .unwrap();
    let mut inside = Vec::new();
    let mut surroundings = Vec::new();
    for y in 78..178 {
        for x in 78..178 {
            if (98..158).contains(&x) && (98..158).contains(&y) {
                inside.push((x, y));
            } else if !(94..162).contains(&x) || !(94..162).contains(&y) {
                surroundings.push((x, y));
            }
        }
    }
    let target = stats(&image, &surroundings);
    let neural_error = distance(stats(&neural.result.composite, &inside), target);
    let cpu_error = distance(stats(&cpu.result.composite, &inside), target);
    let mut seam = 0.0;
    let mut count = 0;
    for t in 94..162 {
        for ((x, y), (nx, ny)) in [
            ((94, t), (93, t)),
            ((161, t), (162, t)),
            ((t, 94), (t, 93)),
            ((t, 161), (t, 162)),
        ] {
            for c in 0..3 {
                seam += (neural.result.composite.pixel(x, y)[c] - image.pixel(nx, ny)[c]).abs();
                count += 1;
            }
        }
    }
    seam /= count as f32;
    println!(
        "texture stats L1: LaMa={neural_error:.6} PatchMatch={cpu_error:.6}; boundary MAE={seam:.6}"
    );
    assert!(seam < 0.015, "boundary seam {seam}");
    assert!(
        neural_error < cpu_error,
        "LaMa stats {neural_error} must beat PatchMatch {cpu_error}"
    );
    for y in 0..256 {
        for x in 0..256 {
            assert_eq!(neural.result.composite.pixel(x, y)[3], image.pixel(x, y)[3]);
            if !(94..162).contains(&x) || !(94..162).contains(&y) {
                assert_eq!(neural.result.composite.pixel(x, y), image.pixel(x, y));
            }
        }
    }
    // Actual present-cache selection, not an explicitly injected model.
    drop(model);
    let mut auto = <dyn Remove>::auto(&registry, SessionOptions::cpu()).unwrap();
    assert_eq!(
        auto.apply(&image, &mask, &params, &cancel).unwrap().backend,
        BackendUsed::Onnx
    );
}

#[cfg(target_os = "macos")]
#[test]
fn cached_lama_1024_coreml_under_two_seconds() {
    let _exclusive = MODEL_TEST.lock().unwrap_or_else(|e| e.into_inner());
    let Some(registry) = registry() else {
        return;
    };
    let mut model = load(&registry, SessionOptions::default());
    let (image, mask) = scene(1024);
    let cancel = AtomicBool::new(false);
    let params = RemoveParams::default();
    // Session compilation and one warm-up are not interactive latency.
    model.apply(&image, &mask, &params, &cancel).unwrap();
    let start = Instant::now();
    let result = model.apply(&image, &mask, &params, &cancel).unwrap();
    let elapsed = start.elapsed();
    let report = model.partition_report().unwrap();
    let coreml = report
        .nodes
        .iter()
        .filter(|n| n.provider == "CoreMLExecutionProvider")
        .count();
    println!(
        "LaMa 1024 pipeline {elapsed:?}; {coreml}/{} executed nodes CoreML; fallback={:?}",
        report.nodes.len(),
        model.fallback_reason()
    );
    assert_eq!(result.backend, BackendUsed::Onnx);
    assert!(
        coreml > 0,
        "CPU-only execution is not a CoreML performance pass"
    );
    // Preserve the strict all-CoreML guard: mixed execution must not pass it.
    if coreml != report.nodes.len() {
        assert!(report.require_coreml().is_err());
    }
    assert!(
        elapsed.as_secs_f32() < 2.0,
        "1024 pipeline took {elapsed:?}"
    );
}
