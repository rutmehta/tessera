//! Gaussian Blur smart filters on the GPU-resident path (WP B5-15, perf
//! audit P19): where the route applies, the resident renderer evaluates the
//! stack (trace), the result matches the CPU bake within the operator
//! contract (docs/11 §1.3), a drag re-runs only the edited stage, and
//! everything else still bakes on the CPU.
#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use tessera_ffi::*;

fn engine() -> (tempfile::TempDir, Arc<Engine>) {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(dir.path().join("support").to_string_lossy().into_owned()).unwrap();
    (dir, engine)
}

/// A PNG with detail at every scale; `alpha` < 255 somewhere when `holes`.
fn png(dir: &Path, name: &str, w: u32, h: u32, sixteen: bool, holes: bool) -> PathBuf {
    let f = |x: u32, y: u32| -> [f64; 4] {
        let checker = if (x / 7 + y / 5).is_multiple_of(2) {
            0.8
        } else {
            0.15
        };
        let n = ((x.wrapping_mul(73856093) ^ y.wrapping_mul(19349663)) % 97) as f64 / 97.0;
        let a = if holes && x > w / 3 && x < w / 2 {
            0.3
        } else {
            1.0
        };
        [
            x as f64 / w as f64,
            checker,
            0.5 * n + 0.3 * (y as f64 / h as f64),
            a,
        ]
    };
    let path = dir.join(name);
    if sixteen {
        let img = image::ImageBuffer::<image::Rgba<u16>, Vec<u16>>::from_fn(w, h, |x, y| {
            image::Rgba(f(x, y).map(|v| (v * 65535.0).round() as u16))
        });
        img.save(&path).unwrap();
    } else {
        let img = image::RgbaImage::from_fn(w, h, |x, y| {
            image::Rgba(f(x, y).map(|v| (v * 255.0).round() as u8))
        });
        img.save(&path).unwrap();
    }
    path
}

fn open(engine: &Arc<Engine>, path: &Path) -> Arc<DocumentSession> {
    engine
        .clone()
        .open_document(path.to_string_lossy().into_owned())
        .unwrap()
}

fn gaussian(radius: f32) -> String {
    format!(r#"{{"id":"gaussian_blur","params":{{"radius":{radius}}}}}"#)
}

fn max_diff(a: &[f32], b: &[f32]) -> f32 {
    assert_eq!(a.len(), b.len());
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0, f32::max)
}

/// The largest difference in 8-bit display code values.
fn max_code_diff(a: &[f32], b: &[f32]) -> i32 {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as i32;
    a.iter()
        .zip(b)
        .map(|(x, y)| (q(*x) - q(*y)).abs())
        .max()
        .unwrap_or(0)
}

/// A 2×2 box reduction of interleaved RGBA (even width and height; the
/// images here are opaque, so straight and premultiplied means agree).
fn reduce(w: usize, v: &[f32]) -> Vec<f32> {
    let h = v.len() / 4 / w;
    let mut o = Vec::with_capacity(v.len() / 4);
    for y in (0..h).step_by(2) {
        for x in (0..w).step_by(2) {
            for c in 0..4 {
                let at = |xx: usize, yy: usize| v[(yy * w + xx) * 4 + c];
                o.push((at(x, y) + at(x + 1, y) + at(x, y + 1) + at(x + 1, y + 1)) / 4.0);
            }
        }
    }
    o
}

fn trace(s: &DocumentSession) -> SmartFilterTrace {
    s.smart_filter_trace().expect("Metal")
}

/// GPU route vs the CPU bake of the same stack, at level 0 and the fit
/// level, 16-bit (numeric tolerance) and 8-bit (one display code value).
#[test]
fn gpu_route_matches_the_cpu_bake_within_the_operator_contract() {
    let (dir, engine) = engine();
    for (sixteen, radii) in [
        (true, &[0.8f32, 2.0, 8.0, 32.0][..]),
        (false, &[3.0, 20.0][..]),
    ] {
        let s = open(
            &engine,
            &png(
                dir.path(),
                &format!("gpu{sixteen}.png"),
                640,
                448,
                sixteen,
                false,
            ),
        );
        let id = s.layers().unwrap()[0].id;
        s.convert_for_smart_filters(id).unwrap();
        for &r in radii {
            while !s.smart_filters(id).unwrap().is_empty() {
                s.remove_smart_filter(id, 0).unwrap();
            }
            s.apply_filter(id, gaussian(r)).unwrap();
            for level in [0u8, 1] {
                s.set_gpu_smart_filters(false);
                let before = trace(&s);
                // The reference is the CPU bake at level 0, reduced 2×2 for
                // level 1: the GPU stack runs at level 0 and reduces, where
                // the CPU bake of a coarser view filters that level with a
                // scaled radius (an approximation of the level-0 result).
                let (w0, _, cpu0) = s.read_presented_level(0).unwrap();
                assert!(
                    trace(&s).cpu_bakes > before.cpu_bakes,
                    "CPU reference baked"
                );
                let cpu = if level == 0 {
                    cpu0
                } else {
                    reduce(w0 as usize, &cpu0)
                };
                s.set_gpu_smart_filters(true);
                let before = trace(&s);
                let (_, _, gpu) = s.read_presented_level(level).unwrap();
                let after = trace(&s);
                if level == 0 {
                    assert!(after.gpu_stages > before.gpu_stages, "r {r}: GPU stage ran");
                } else {
                    // Stages run at level 0; other levels reduce the cached result.
                    assert_eq!(after.gpu_stages, before.gpu_stages, "r {r}: stage reused");
                }
                assert_eq!(after.cpu_bakes, before.cpu_bakes, "r {r}: no CPU bake");
                assert_eq!(after.cpu_fallbacks, before.cpu_fallbacks);
                let (d, c) = (max_diff(&gpu, &cpu), max_code_diff(&gpu, &cpu));
                eprintln!(
                    "16-bit {sixteen} radius {r} level {level}: max |Δ| {d:.2e}, {c} code values"
                );
                if sixteen && level == 0 {
                    // One operator plus the 16-bit quantization of the CPU bake.
                    assert!(d <= 1e-4 + 1.0 / 65535.0, "r {r}: {d}");
                } else if sixteen {
                    assert!(d <= 2e-3, "r {r} level {level}: {d}");
                } else {
                    assert!(c <= 1, "r {r} level {level}: {c} code values");
                }
            }
        }
    }
}

/// A two-filter stack with opacity: the chain agrees with the CPU bake, and
/// dragging the top filter's radius (previews) re-runs only that stage on
/// the GPU; the prefix and the child composite come from the cache.
#[test]
fn drag_of_the_top_filter_reruns_only_that_stage() {
    let (dir, engine) = engine();
    let s = open(
        &engine,
        &png(dir.path(), "stack.png", 512, 384, true, false),
    );
    let id = s.layers().unwrap()[0].id;
    s.convert_for_smart_filters(id).unwrap();
    s.apply_filter(id, gaussian(3.0)).unwrap();
    s.apply_filter(id, gaussian(6.0)).unwrap();
    s.set_smart_filter(
        id,
        1,
        SmartFilterEdit::Blending {
            mode: "normal".into(),
            opacity: 0.7,
        },
    )
    .unwrap();
    s.set_gpu_smart_filters(false);
    let (_, _, cpu) = s.read_presented_level(0).unwrap();
    s.set_gpu_smart_filters(true);
    let (_, _, gpu) = s.read_presented_level(0).unwrap();
    let d = max_diff(&gpu, &cpu);
    eprintln!("two-stage chain: max |Δ| {d:.2e}");
    assert!(d <= 2e-3, "chain {d}");

    let start = trace(&s);
    for (k, r) in [4.0f32, 5.0, 7.0, 9.0, 11.0].into_iter().enumerate() {
        s.preview_smart_filter(id, 1, gaussian(r), None).unwrap();
        let (_, _, shown) = s.read_presented_level(0).unwrap();
        let t = trace(&s);
        assert_eq!(
            t.gpu_stages - start.gpu_stages,
            k as u64 + 1,
            "one GPU stage per drag tick (the bottom filter is cached)"
        );
        assert_eq!(t.cpu_previews, start.cpu_previews, "no CPU preview job");
        assert!(shown.iter().all(|v| v.is_finite()));
    }
    // The preview matches the CPU preview of the same edit.
    s.set_gpu_smart_filters(false);
    s.preview_smart_filter(id, 1, gaussian(11.0), None).unwrap();
    s.wait_filters_idle();
    let (_, _, cpu_preview) = s.read_presented_level(0).unwrap();
    s.set_gpu_smart_filters(true);
    s.preview_smart_filter(id, 1, gaussian(11.0), None).unwrap();
    let (_, _, gpu_preview) = s.read_presented_level(0).unwrap();
    let d = max_diff(&gpu_preview, &cpu_preview);
    assert!(d <= 2e-3, "preview {d}");
    s.clear_preview().unwrap();
    // Committing re-evaluates the stack at most once (the committed layer is
    // a new smart object instance to the engine's cache: NEEDS.md), and
    // later frames reuse it.
    let before = trace(&s).gpu_stages;
    s.set_smart_filter(
        id,
        1,
        SmartFilterEdit::Params {
            filter_json: gaussian(11.0),
        },
    )
    .unwrap();
    s.read_presented_level(0).unwrap();
    let committed = trace(&s).gpu_stages;
    assert!(committed - before <= 2, "{before} → {committed}");
    s.read_presented_level(1).unwrap();
    s.read_presented_level(0).unwrap();
    assert_eq!(trace(&s).gpu_stages, committed, "committed stack cached");
}

/// Stacks and layers the GPU route does not cover keep the CPU bake:
/// another filter, a Gaussian above the exact radius, transparency in the
/// child, a transformed smart object is not tested here (the Rust API has no
/// transform call for smart objects).
#[test]
fn other_stacks_keep_the_cpu_bake() {
    let (dir, engine) = engine();
    let cases: [(&str, bool, String); 3] = [
        (
            "other.png",
            false,
            r#"{"id":"box_blur","params":{"radius":3}}"#.into(),
        ),
        ("large.png", false, gaussian(40.0)),
        ("holes.png", true, gaussian(4.0)),
    ];
    for (name, holes, json) in cases {
        let s = open(&engine, &png(dir.path(), name, 300, 200, false, holes));
        let id = s.layers().unwrap()[0].id;
        s.convert_for_smart_filters(id).unwrap();
        s.apply_filter(id, json.clone()).unwrap();
        let before = trace(&s);
        s.read_presented_level(0).unwrap();
        let after = trace(&s);
        assert_eq!(after.gpu_stages, before.gpu_stages, "{name}: no GPU stage");
        assert!(after.cpu_bakes > before.cpu_bakes, "{name}: CPU bake");
    }
}

/// Export keeps the CPU bake (full resolution, exact): a document whose
/// viewport uses the GPU route exports the same file as with the route off.
#[test]
fn export_is_unchanged_by_the_gpu_route() {
    let (dir, engine) = engine();
    let s = open(&engine, &png(dir.path(), "exp.png", 400, 300, false, false));
    let id = s.layers().unwrap()[0].id;
    s.convert_for_smart_filters(id).unwrap();
    s.apply_filter(id, gaussian(5.0)).unwrap();
    s.read_presented_level(0).unwrap();
    let a = dir.path().join("a.png");
    s.export_flat(
        a.to_string_lossy().into_owned(),
        ExportFormat::Png,
        90,
        ExportColor::Srgb,
    )
    .unwrap();
    s.set_gpu_smart_filters(false);
    let b = dir.path().join("b.png");
    s.export_flat(
        b.to_string_lossy().into_owned(),
        ExportFormat::Png,
        90,
        ExportColor::Srgb,
    )
    .unwrap();
    same_png(&a, &b);
}

/// Two PNG exports are the same: identical pixels and ICC profiles, except
/// the profile header's creation time (bytes 24..36), which the built-in
/// profiles stamp when they are created (NEEDS.md 7).
fn same_png(a: &std::path::Path, b: &std::path::Path) {
    fn read(p: &std::path::Path) -> (Vec<u8>, Vec<u8>) {
        let dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(p).unwrap()));
        let mut r = dec.read_info().unwrap();
        let mut icc = r
            .info()
            .icc_profile
            .as_ref()
            .map(|c| c.to_vec())
            .unwrap_or_default();
        if icc.len() >= 36 {
            icc[24..36].fill(0);
        }
        let mut buf = vec![0u8; r.output_buffer_size()];
        let n = r.next_frame(&mut buf).unwrap().buffer_size();
        buf.truncate(n);
        (buf, icc)
    }
    let (pa, ia) = read(a);
    let (pb, ib) = read(b);
    assert!(
        pa == pb,
        "pixels differ: {} vs {}",
        a.display(),
        b.display()
    );
    assert!(
        ia == ib,
        "ICC profiles differ: {} vs {}",
        a.display(),
        b.display()
    );
}
