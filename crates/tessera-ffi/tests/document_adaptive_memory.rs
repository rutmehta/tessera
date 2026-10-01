//! B5-20b review: measured memory of Adaptive Wide Angle on a layer at the
//! 100 MP limit (12240 × 8160, or `AWA_MEMORY_SIZE=WxH`) through the session: pixel-layer apply on U8
//! and F32 documents, and smart-object begin + apply on U8. Its own test
//! binary: the counting allocator is global, so run the ignored tests one at
//! a time:
//! `cargo test --release -p tessera-ffi --test document_adaptive_memory -- --ignored --nocapture --test-threads=1`.
#![cfg(target_os = "macos")]

use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use tessera_ffi::*;
use transform::adaptive::{Adaptive, CameraModel, LineConstraint, LineOrientation, Projection};

struct Counting;
static CURRENT: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            let now = CURRENT.fetch_add(l.size(), Ordering::SeqCst) + l.size();
            PEAK.fetch_max(now, Ordering::SeqCst);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) };
        CURRENT.fetch_sub(l.size(), Ordering::SeqCst);
    }
}

#[global_allocator]
static A: Counting = Counting;

/// 12240 × 8160 (the largest 3:2 layer under the limit) unless
/// `AWA_MEMORY_SIZE=WxH` overrides it.
fn size() -> (u32, u32) {
    std::env::var("AWA_MEMORY_SIZE")
        .ok()
        .and_then(|v| {
            let (w, h) = v.split_once('x')?;
            Some((w.parse().ok()?, h.parse().ok()?))
        })
        .unwrap_or((12240, 8160))
}

fn gb(b: usize) -> f64 {
    b as f64 / 1e9
}

/// A document at the limit with materialized pixels (a bar over most of the
/// layer, so its tiles are real, not uniform).
fn document(depth: DocDepth) -> (tempfile::TempDir, Arc<Engine>, Arc<DocumentSession>, u64) {
    let (w, h) = size();
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(dir.path().join("support").to_string_lossy().into_owned()).unwrap();
    let s = engine.clone().new_document(w, h, depth, None).unwrap();
    let layer = s.layers().unwrap()[0].id;
    s.set_selection_rect(37, 29, i64::from(w) - 80, i64::from(h) - 61, 0.0)
        .unwrap();
    s.fill_selection(
        layer,
        SelectionFill::Color {
            color: PaintColor {
                r: 0.1,
                g: 0.2,
                b: 0.3,
            },
        },
        1.0,
    )
    .unwrap();
    s.select_none().unwrap();
    s.wait_idle();
    (dir, engine, s, layer)
}

fn recipe(recipe_json: &str) -> String {
    let mut a: Adaptive = serde_json::from_str(recipe_json).unwrap();
    let (w, h) = size();
    let (wf, hf) = (f64::from(w), f64::from(h));
    a.camera = CameraModel::Manual {
        focal_px: 0.4 * wf,
        center: [wf / 2., hf / 2.],
        projection: Projection::Equidistant,
    };
    a.output_focal_px = 0.4 * wf;
    let json = serde_json::to_string(&a).unwrap();
    let c = adaptive_wide_angle_curve(json, vec![0.25 * wf, 0.2 * hf], vec![0.25 * wf, 0.8 * hf])
        .unwrap();
    a.lines.push(LineConstraint {
        points: c.chunks(2).map(|p| [p[0], p[1]]).collect(),
        orientation: LineOrientation::Vertical,
        weight: 1.,
    });
    serde_json::to_string(&a).unwrap()
}

/// Peak over `f`, absolute and above the allocation level before it.
fn measure(label: &str, f: impl FnOnce()) {
    let base = CURRENT.load(Ordering::SeqCst);
    PEAK.store(base, Ordering::SeqCst);
    let t = std::time::Instant::now();
    f();
    let peak = PEAK.load(Ordering::SeqCst);
    eprintln!(
        "{label}: peak {:.2} GB absolute, {:.2} GB above the {:.2} GB before it ({:.1} s)",
        gb(peak),
        gb(peak - base),
        gb(base),
        t.elapsed().as_secs_f64()
    );
}

fn pixel_apply(depth: DocDepth, label: &str) {
    let base = CURRENT.load(Ordering::SeqCst);
    let (_dir, _engine, s, layer) = document(depth);
    eprintln!(
        "{label}: document {:.2} GB",
        gb(CURRENT.load(Ordering::SeqCst) - base)
    );
    measure(&format!("{label} pixel begin + apply"), || {
        let info = s.begin_adaptive_wide_angle(layer, None).unwrap();
        s.commit_adaptive_wide_angle(info.token, recipe(&info.recipe_json))
            .unwrap();
        s.wait_idle();
    });
}

#[test]
#[ignore]
fn memory_of_a_pixel_apply_at_100_megapixels_u8() {
    pixel_apply(DocDepth::U8, "U8");
}

#[test]
#[ignore]
fn memory_of_a_pixel_apply_at_100_megapixels_f32() {
    pixel_apply(DocDepth::F32, "F32");
}

#[test]
#[ignore]
fn memory_of_a_smart_object_begin_and_apply_at_100_megapixels_u8() {
    let (_dir, _engine, s, layer) = document(DocDepth::U8);
    s.convert_for_smart_filters(layer).unwrap();
    s.wait_idle();
    let mut info = None;
    measure("U8 smart-object begin", || {
        info = Some(s.begin_adaptive_wide_angle(layer, None).unwrap());
    });
    let info = info.unwrap();
    let n = s.history_items().unwrap().len();
    measure("U8 smart-object apply", || {
        // Above ≈ 33.5 MP the compositor's CPU smart-filter pass limit
        // (`FilterPassLimits::retained_bytes`, 1 GiB) refuses the stage;
        // report it rather than fail the measurement.
        match s.commit_adaptive_wide_angle(info.token, recipe(&info.recipe_json)) {
            Ok(_) => s.wait_idle(),
            Err(e) => {
                eprintln!("U8 smart-object apply refused: {e}");
                assert_eq!(s.history_items().unwrap().len(), n, "no history on refusal");
            }
        }
    });
}
