//! B5-18b: Camera Raw Filter previews, the 1:1 detail pane and smart-filter
//! checks render the visible region at the view's pyramid level, not the
//! whole canvas at full resolution; apply still renders full resolution.
//! Work is asserted by the pixels the camera_raw adapter evaluated and by
//! peak heap allocation (a counting global allocator), never by timing.
#![cfg(target_os = "macos")]

use std::alloc::{GlobalAlloc, Layout, System};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use tessera_ffi::*;

// ───────────────────────────── allocation meter ─────────────────────────────

struct Counting;
static CURRENT: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            let now = CURRENT.fetch_add(l.size(), Ordering::Relaxed) + l.size();
            PEAK.fetch_max(now, Ordering::Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) };
        CURRENT.fetch_sub(l.size(), Ordering::Relaxed);
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        let q = unsafe { System.realloc(p, l, new) };
        if !q.is_null() {
            if new >= l.size() {
                let now = CURRENT.fetch_add(new - l.size(), Ordering::Relaxed) + new - l.size();
                PEAK.fetch_max(now, Ordering::Relaxed);
            } else {
                CURRENT.fetch_sub(l.size() - new, Ordering::Relaxed);
            }
        }
        q
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

/// Peak heap growth (bytes) while `f` runs.
fn peak_during<T>(f: impl FnOnce() -> T) -> (T, usize) {
    let base = CURRENT.load(Ordering::Relaxed);
    PEAK.store(base, Ordering::Relaxed);
    let out = f();
    (out, PEAK.load(Ordering::Relaxed).saturating_sub(base))
}

/// The meter is process-wide: tests in this file run one at a time.
fn serial() -> MutexGuard<'static, ()> {
    static M: Mutex<()> = Mutex::new(());
    M.lock().unwrap_or_else(|e| e.into_inner())
}

// ───────────────────────────── fixtures ─────────────────────────────

fn engine() -> (tempfile::TempDir, Arc<Engine>) {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(dir.path().join("support").to_string_lossy().into_owned()).unwrap();
    (dir, engine)
}

/// An opaque PNG with detail at every scale (edges, a checker, ramps).
fn opaque_png(dir: &Path, name: &str, w: u32, h: u32) -> PathBuf {
    let img = image::RgbaImage::from_fn(w, h, |x, y| {
        let checker = if (x / 7 + y / 5) % 2 == 0 { 200 } else { 40 };
        image::Rgba([
            (x * 255 / w) as u8,
            checker,
            if x > w / 2 && y > h / 3 {
                230
            } else {
                (y * 255 / h) as u8
            },
            255,
        ])
    });
    let path = dir.join(name);
    img.save(&path).unwrap();
    path
}

fn open(engine: &Arc<Engine>, path: &Path) -> Arc<DocumentSession> {
    engine
        .clone()
        .open_document(path.to_string_lossy().into_owned())
        .unwrap()
}

/// Camera Raw Filter JSON: `settings` over the sheet's neutral base (no
/// sharpening / colour NR, no lens corrections).
fn camera_raw(settings: serde_json::Value) -> String {
    let mut base = serde_json::json!({
        "detail": {"sharpening": {"amount": 0.0}, "noise_reduction": {"color": 0.0}},
        "lens": {"profile": {"kind": "none"}, "remove_chromatic_aberration": false},
    });
    merge(&mut base, settings);
    serde_json::json!({"id": "camera_raw", "params": {"settings": base, "amount": 1.0}}).to_string()
}

fn merge(a: &mut serde_json::Value, b: serde_json::Value) {
    match (a, b) {
        (serde_json::Value::Object(a), serde_json::Value::Object(b)) => {
            for (k, v) in b {
                merge(a.entry(k).or_insert(serde_json::Value::Null), v);
            }
        }
        (a, b) => *a = b,
    }
}

fn ms(t: std::time::Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn mb(b: usize) -> f64 {
    b as f64 / (1024.0 * 1024.0)
}

// ───────────────────────────── bench ─────────────────────────────

/// Before/after numbers for a 24 MP layer (6000 × 4000): preview at the fit
/// level, preview of a 100 % viewport, the 1:1 detail pane and apply.
/// `cargo test --release -p tessera-ffi --test document_camera_raw_preview
///  -- --ignored --nocapture bench_camera_raw_24mp`
#[test]
#[ignore]
fn bench_camera_raw_24mp() {
    let _g = serial();
    let (dir, engine) = engine();
    let (w, h) = (6000u32, 4000u32);
    let s = open(&engine, &opaque_png(dir.path(), "big.png", w, h));
    let id = s.layers().unwrap()[0].id;
    let local = |e: f64| camera_raw(serde_json::json!({"tone": {"exposure": e, "clarity": 20.0}}));
    let global = |e: f64| {
        camera_raw(
            serde_json::json!({"tone": {"exposure": e}, "effects": {"vignette": {"amount": -40.0}}}),
        )
    };
    let run = |label: &str, json: &dyn Fn(f64) -> String, region: Option<DocRect>| {
        let mut times = Vec::new();
        let mut peaks = Vec::new();
        for (i, e) in [0.3, 0.4, 0.5, 0.6].iter().enumerate() {
            let t = std::time::Instant::now();
            let ((), peak) = peak_during(|| {
                s.preview_filter(id, json(*e), region).unwrap();
                s.wait_filters_idle();
            });
            assert_eq!(s.filter_error(), None);
            if i > 0 {
                times.push(ms(t));
                peaks.push(peak);
            } else {
                println!("{label}: cold {:.0} ms, peak {:.0} MB", ms(t), mb(peak));
            }
        }
        times.sort_by(f64::total_cmp);
        peaks.sort();
        println!(
            "{label}: warm median {:.0} ms, peak {:.0} MB",
            times[times.len() / 2],
            mb(peaks[peaks.len() - 1])
        );
    };
    // Fit: level 2 (1500 × 1000) shows the whole canvas.
    s.set_viewport(2, 0, 0, 1500, 1000, 0.25).unwrap();
    run("fit L2 preview, local settings", &local, None);
    run("fit L2 preview, vignette (global)", &global, None);
    // 100 %: a 1600 × 1000 window in the middle of the canvas.
    s.set_viewport(0, 2200, 1500, 1600, 1000, 1.0).unwrap();
    let r = Some(DocRect {
        x: 2200,
        y: 1500,
        width: 1600,
        height: 1000,
    });
    run("100% preview, local settings", &local, r);
    run("100% preview, vignette (global)", &global, r);
    s.clear_preview().unwrap();
    for (label, json) in [
        ("detail 280x280, local", local(0.5)),
        ("detail 280x280, vignette", global(0.5)),
    ] {
        let t = std::time::Instant::now();
        let (d, peak) = peak_during(|| s.filter_detail(id, json, 2900, 1900, 280, 280).unwrap());
        println!(
            "{label}: {:.0} ms, peak {:.0} MB (level {})",
            ms(t),
            mb(peak),
            d.level
        );
    }
    let t = std::time::Instant::now();
    let (_, peak) = peak_during(|| s.apply_filter(id, local(0.5)).unwrap());
    println!(
        "apply at full resolution: {:.0} ms, peak {:.0} MB",
        ms(t),
        mb(peak)
    );
}

// ───────────────────────────── helpers ─────────────────────────────

/// A smooth opaque PNG (ramps only): level averages commute with smooth
/// operators, so a level-1 render compares with the level-0 one.
fn smooth_png(dir: &Path, name: &str, w: u32, h: u32) -> PathBuf {
    let img = image::RgbaImage::from_fn(w, h, |x, y| {
        image::Rgba([
            (40 + x * 180 / w) as u8,
            (60 + y * 150 / h) as u8,
            (90 + (x + y) * 100 / (w + h)) as u8,
            255,
        ])
    });
    let path = dir.join(name);
    img.save(&path).unwrap();
    path
}

/// Local operators only: tone, presence, detail, colour.
fn local_json(exposure: f64) -> String {
    camera_raw(serde_json::json!({
        "tone": {"exposure": exposure, "contrast": 20.0, "highlights": -30.0, "shadows": 30.0,
                 "clarity": 40.0, "texture": 30.0},
        "detail": {"sharpening": {"amount": 60.0}, "noise_reduction": {"luminance": 20.0}},
        "color": {"saturation": 15.0},
    }))
}

/// Settings whose result depends on the whole image (placed vignette).
fn vignette_json() -> String {
    camera_raw(
        serde_json::json!({"tone": {"exposure": 0.3}, "effects": {"vignette": {"amount": -40.0}}}),
    )
}

fn region(x: i64, y: i64, width: i64, height: i64) -> Option<DocRect> {
    Some(DocRect {
        x,
        y,
        width,
        height,
    })
}

/// `f`'s result and the pixels the Camera Raw adapter developed meanwhile.
fn developed<T>(s: &DocumentSession, f: impl FnOnce() -> T) -> (T, u64) {
    let before = s.camera_raw_pixels_developed();
    let out = f();
    (out, s.camera_raw_pixels_developed() - before)
}

/// `x0..x0+w × y0..y0+h` of an interleaved RGBA image `stride` pixels wide.
fn crop(px: &[f32], stride: usize, x0: usize, y0: usize, w: usize, h: usize) -> Vec<f32> {
    let mut out = Vec::with_capacity(w * h * 4);
    for y in y0..y0 + h {
        out.extend_from_slice(&px[(y * stride + x0) * 4..(y * stride + x0 + w) * 4]);
    }
    out
}

fn max_diff(a: &[f32], b: &[f32]) -> f32 {
    assert_eq!(a.len(), b.len());
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0, f32::max)
}

/// The layer's live pixels at `level` (no preview).
fn live(s: &DocumentSession, level: u8) -> Vec<f32> {
    s.clear_preview().unwrap();
    s.read_presented_level(level).unwrap().2
}

/// The pane's bytes as 0…1 straight RGBA. B5-27: like the canvas surfaces,
/// the pane holds the document's own samples (its encoding, its profile)
/// quantized to 8 bits, so these compare directly with the canvas samples.
fn pane(d: &FilterDetail) -> Vec<f32> {
    pane_bytes(d)
        .into_iter()
        .map(|v| f32::from(v) / 255.0)
        .collect()
}

fn pane_bytes(d: &FilterDetail) -> Vec<u8> {
    let surface = tessera_ffi::surface::Surface::lookup(d.surface_id, d.width, d.height).unwrap();
    let mut out = Vec::new();
    surface
        .with_pixels(|px, stride| {
            for y in 0..d.height as usize {
                out.extend_from_slice(&px[y * stride..y * stride + d.width as usize * 4]);
            }
        })
        .unwrap();
    out
}

/// What the canvas writes to its RGBA8 surfaces for a sample (CPU `quantize`,
/// GPU `textureStore` into `rgba8unorm`).
fn canvas_byte(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

/// Display light of one 8-bit colour channel: the host decodes canvas bytes
/// with `rgba8Unorm_srgb` into an extended linear sRGB layer, and pane bytes
/// through the colour space of the pane's CGImage (sRGB).
fn display_linear(v: u8) -> f32 {
    let e = f32::from(v) / 255.0;
    if e <= 0.04045 {
        e / 12.92
    } else {
        ((e + 0.055) / 1.055).powf(2.4)
    }
}

fn builtin_icc(b: color_mgmt::Builtin) -> Vec<u8> {
    color_mgmt::Registry::new()
        .builtin(b)
        .unwrap()
        .icc_bytes()
        .to_vec()
}

/// A 16-bit opaque PNG of saturated colour (outside sRGB when read as
/// Display P3) with an embedded `icc` profile.
fn tagged_png(dir: &Path, name: &str, w: u32, h: u32, icc: Vec<u8>) -> PathBuf {
    use image::ImageEncoder;
    let mut bytes = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let ramp = x as f32 / w as f32;
            let rgb = match (x / 16 + y / 16) % 3 {
                0 => [0.95, 0.05 + 0.3 * ramp, 0.08],
                1 => [0.1, 0.9 - 0.4 * ramp, 0.2],
                _ => [0.15 + 0.5 * ramp, 0.2, 0.92],
            };
            for v in rgb {
                bytes.extend_from_slice(&((v * 65535.0 + 0.5) as u16).to_ne_bytes());
            }
        }
    }
    let path = dir.join(name);
    let mut enc = image::codecs::png::PngEncoder::new(std::fs::File::create(&path).unwrap());
    enc.set_icc_profile(icc).unwrap();
    enc.write_image(&bytes, w, h, image::ExtendedColorType::Rgb16)
        .unwrap();
    path
}

// ───────────────────────────── tests ─────────────────────────────

#[test]
fn preview_work_scales_with_the_viewport_not_the_canvas() {
    let _g = serial();
    let (dir, engine) = engine();
    let mut seen = Vec::new();
    for (w, h) in [(768u32, 512u32), (2304, 1536)] {
        let s = open(&engine, &opaque_png(dir.path(), &format!("c{w}.png"), w, h));
        let id = s.layers().unwrap()[0].id;
        // 100 %: a 256 × 192 window.
        s.set_viewport(0, 200, 160, 256, 192, 1.0).unwrap();
        let (((), peak), pixels) = developed(&s, || {
            peak_during(|| {
                s.preview_filter(id, local_json(0.5), region(200, 160, 256, 192))
                    .unwrap();
                s.wait_filters_idle();
            })
        });
        assert_eq!(s.filter_error(), None);
        println!(
            "{w}x{h} 100% preview: {pixels} px developed, peak {:.1} MB",
            mb(peak)
        );
        // The window plus the local operators' halo, never the canvas.
        assert!(pixels > 0 && pixels <= 340 * 280, "{pixels} px");
        seen.push((pixels, peak));
        // Whole-image settings render the canvas of the view level.
        s.set_viewport(2, 0, 0, w / 4, h / 4, 0.25).unwrap();
        let ((), pixels) = developed(&s, || {
            s.preview_filter(id, vignette_json(), None).unwrap();
            s.wait_filters_idle();
        });
        assert_eq!(s.filter_error(), None);
        assert_eq!(
            pixels,
            u64::from(w / 4) * u64::from(h / 4),
            "fit level only"
        );
    }
    let ((small_px, small_peak), (big_px, big_peak)) = (seen[0], seen[1]);
    assert_eq!(small_px, big_px, "same window, same work on a 9× canvas");
    // A level-0 F32 copy of the big canvas alone would be 54 MB.
    assert!(
        big_peak < small_peak + (8 << 20) && big_peak < 24 << 20,
        "peak {:.1} MB vs {:.1} MB on the small canvas",
        mb(big_peak),
        mb(small_peak)
    );
}

#[test]
fn preview_matches_the_full_render_in_the_visible_region() {
    let _g = serial();
    let (dir, engine) = engine();
    let (w, h) = (512usize, 384usize);
    let s = open(
        &engine,
        &opaque_png(dir.path(), "m.png", w as u32, h as u32),
    );
    let id = s.layers().unwrap()[0].id;
    let (x0, y0, rw, rh) = (150usize, 100usize, 160usize, 120usize);
    s.set_viewport(0, x0 as u32, y0 as u32, rw as u32, rh as u32, 1.0)
        .unwrap();
    let ((), pixels) = developed(&s, || {
        s.preview_filter(
            id,
            local_json(0.5),
            region(x0 as i64, y0 as i64, rw as i64, rh as i64),
        )
        .unwrap();
        s.wait_filters_idle();
    });
    assert_eq!(s.filter_error(), None);
    assert!(
        pixels < (w * h / 2) as u64,
        "a crop was developed: {pixels}"
    );
    let shown = crop(&s.read_presented_level(0).unwrap().2, w, x0, y0, rw, rh);
    let before = crop(&live(&s, 0), w, x0, y0, rw, rh);
    assert!(
        max_diff(&shown, &before) > 0.05,
        "the preview changes pixels"
    );
    // Apply renders the whole canvas at full resolution.
    let (_, pixels) = developed(&s, || s.apply_filter(id, local_json(0.5)).unwrap());
    assert_eq!(pixels, (w * h) as u64, "apply is full resolution");
    let applied = crop(&live(&s, 0), w, x0, y0, rw, rh);
    let d = max_diff(&shown, &applied);
    assert!(d <= 2.5 / 255.0, "preview vs full render {d}");

    // Whole-image settings at the fit level match the full render's level.
    let s = open(
        &engine,
        &smooth_png(dir.path(), "g.png", w as u32, h as u32),
    );
    let id = s.layers().unwrap()[0].id;
    s.set_viewport(1, 0, 0, (w / 2) as u32, (h / 2) as u32, 0.5)
        .unwrap();
    s.preview_filter(id, vignette_json(), None).unwrap();
    s.wait_filters_idle();
    assert_eq!(s.filter_error(), None);
    let shown = s.read_presented_level(1).unwrap().2;
    assert!(max_diff(&shown, &live(&s, 1)) > 0.05);
    s.apply_filter(id, vignette_json()).unwrap();
    let d = max_diff(&shown, &live(&s, 1));
    assert!(
        d <= 4.0 / 255.0,
        "fit preview vs full render at level 1: {d}"
    );
}

#[test]
fn detail_pane_renders_the_tile_and_matches_the_canvas() {
    let _g = serial();
    let (dir, engine) = engine();
    let (w, h) = (1024usize, 768usize);
    let s = open(
        &engine,
        &opaque_png(dir.path(), "d.png", w as u32, h as u32),
    );
    let id = s.layers().unwrap()[0].id;
    let (x0, y0, dw, dh) = (300usize, 200usize, 128usize, 96usize);
    let gaussian = r#"{"id":"gaussian_blur","params":{"radius":3}}"#.to_owned();
    for json in [local_json(0.5), gaussian] {
        let (d, pixels) = developed(&s, || {
            s.filter_detail(id, json.clone(), x0 as i64, y0 as i64, dw as u32, dh as u32)
                .unwrap()
        });
        assert_eq!((d.width, d.height, d.level), (dw as u32, dh as u32, 0));
        assert!(pixels <= 220 * 180, "the tile and its halo: {pixels}");
        let pane = pane(&d);
        s.set_viewport(0, x0 as u32, y0 as u32, dw as u32, dh as u32, 1.0)
            .unwrap();
        s.preview_filter(id, json, region(x0 as i64, y0 as i64, dw as i64, dh as i64))
            .unwrap();
        s.wait_filters_idle();
        assert_eq!(s.filter_error(), None);
        let canvas = crop(&s.read_presented_level(0).unwrap().2, w, x0, y0, dw, dh);
        // B5-27: 8-bit quantization of the same samples: half a step.
        let diff = max_diff(&pane, &canvas);
        assert!(diff <= 0.01, "pane vs canvas {diff}");
        s.clear_preview().unwrap();
    }
}

/// B5-27: the pane shows what the canvas shows for the same region, in sRGB
/// and Display P3 documents alike. The canvas presents a document's samples
/// in the document's own encoding (no conversion), and the host decodes the
/// canvas surfaces and the pane's CGImage through the same sRGB curve, so the
/// pane's bytes must be the canvas's bytes. Tolerance: one 8-bit step
/// (rounding), and at most 0.005 in decoded display light.
#[test]
fn detail_pane_matches_the_canvas_in_srgb_and_display_p3_documents() {
    let _g = serial();
    let (dir, engine) = engine();
    let (w, h) = (256usize, 192usize);
    let (x0, y0, dw, dh) = (40usize, 30usize, 96usize, 64usize);
    for (file, icc, srgb) in [
        ("srgb.png", builtin_icc(color_mgmt::Builtin::Srgb), true),
        ("p3.png", builtin_icc(color_mgmt::Builtin::DisplayP3), false),
    ] {
        let s = open(
            &engine,
            &tagged_png(dir.path(), file, w as u32, h as u32, icc),
        );
        // The document keeps the embedded profile and its samples as stored.
        let name = s.info().unwrap().profile_name.unwrap_or_default();
        assert_eq!(
            name.contains("sRGB"),
            srgb,
            "{file}: document profile {name}"
        );
        let first = &s.read_presented_level(0).unwrap().2[..3];
        assert!(
            max_diff(first, &[0.95, 0.05, 0.08]) <= 1.0 / 255.0,
            "{file}: samples converted on open: {first:?}"
        );
        let id = s.layers().unwrap()[0].id;
        for json in [
            r#"{"id":"gaussian_blur","params":{"radius":2}}"#.to_owned(),
            camera_raw(
                serde_json::json!({"tone": {"exposure": 0.4}, "color": {"saturation": 20.0}}),
            ),
        ] {
            let d = s
                .filter_detail(id, json.clone(), x0 as i64, y0 as i64, dw as u32, dh as u32)
                .unwrap();
            assert_eq!((d.width, d.height, d.level), (dw as u32, dh as u32, 0));
            let pane = pane_bytes(&d);
            s.set_viewport(0, x0 as u32, y0 as u32, dw as u32, dh as u32, 1.0)
                .unwrap();
            s.preview_filter(
                id,
                json.clone(),
                region(x0 as i64, y0 as i64, dw as i64, dh as i64),
            )
            .unwrap();
            s.wait_filters_idle();
            assert_eq!(s.filter_error(), None);
            let canvas: Vec<u8> = crop(&s.read_presented_level(0).unwrap().2, w, x0, y0, dw, dh)
                .into_iter()
                .map(canvas_byte)
                .collect();
            let (mut worst_byte, mut worst_light) = (0u8, 0f32);
            for (i, (&p, &c)) in pane.iter().zip(&canvas).enumerate() {
                worst_byte = worst_byte.max(p.abs_diff(c));
                if i % 4 < 3 {
                    worst_light = worst_light.max((display_linear(p) - display_linear(c)).abs());
                }
            }
            assert!(
                worst_byte <= 1,
                "{file} {json}: pane vs canvas bytes differ by {worst_byte}"
            );
            assert!(
                worst_light <= 0.005,
                "{file} {json}: pane vs canvas display light differs by {worst_light}"
            );
            s.clear_preview().unwrap();
        }
    }
}

#[test]
fn re_edit_detail_replaces_the_saved_smart_filter() {
    let _g = serial();
    let (dir, engine) = engine();
    let s = open(&engine, &opaque_png(dir.path(), "r.png", 256, 192));
    let id = s.layers().unwrap()[0].id;
    let gaussian = r#"{"id":"gaussian_blur","params":{"radius":1.5}}"#.to_owned();
    let exposure = camera_raw(serde_json::json!({"tone": {"exposure": 1.0}}));
    let zero = serde_json::json!({"id": "camera_raw", "params": {"settings": {}, "amount": 0.0}})
        .to_string();
    let once_gaussian = pane(
        &s.filter_detail(id, gaussian.clone(), 40, 30, 64, 48)
            .unwrap(),
    );
    let once_exposure = pane(
        &s.filter_detail(id, exposure.clone(), 40, 30, 64, 48)
            .unwrap(),
    );
    let plain = pane(&s.filter_detail(id, zero, 40, 30, 64, 48).unwrap());
    s.convert_for_smart_filters(id).unwrap();
    for (json, once) in [(gaussian, once_gaussian), (exposure, once_exposure)] {
        s.apply_filter(id, json.clone()).unwrap();
        let edit = pane(
            &s.smart_filter_detail(id, 0, json.clone(), 40, 30, 64, 48)
                .unwrap(),
        );
        let d = max_diff(&edit, &once);
        assert!(d <= 0.01, "re-edit shows the filter once: {d}");
        let stacked = pane(&s.filter_detail(id, json.clone(), 40, 30, 64, 48).unwrap());
        let d = max_diff(&stacked, &once);
        assert!(
            d > 0.02,
            "a new filter stacks on the saved one: {d} ({json})"
        );
        s.remove_smart_filter(id, 0).unwrap();
        let d = max_diff(
            &pane(
                &s.filter_detail(
                    id,
                    r#"{"id":"camera_raw","params":{"settings":{},"amount":0}}"#.into(),
                    40,
                    30,
                    64,
                    48,
                )
                .unwrap(),
            ),
            &plain,
        );
        assert!(d <= 0.01, "back to the plain layer {d}");
    }
    assert!(
        s.smart_filter_detail(id, 3, local_json(0.1), 0, 0, 8, 8)
            .is_err()
    );
}

#[test]
fn engine_names_the_camera_raw_filter_and_checks_smart_filters_on_a_small_level() {
    let _g = serial();
    let (dir, engine) = engine();
    let s = open(&engine, &opaque_png(dir.path(), "n.png", 1536, 1024));
    let id = s.layers().unwrap()[0].id;
    s.apply_filter(id, local_json(0.2)).unwrap();
    assert_eq!(
        s.history_items().unwrap().last().unwrap().label,
        "Camera Raw Filter"
    );
    s.convert_for_smart_filters(id).unwrap();
    let history = s.history_items().unwrap().len();
    let bad = r#"{"id":"camera_raw","params":{"settings":{"tone":{"exposure":11}}}}"#;
    assert!(s.apply_filter(id, bad.into()).is_err());
    assert_eq!(
        s.history_items().unwrap().len(),
        history,
        "rejected, no history"
    );
    let (_, pixels) = developed(&s, || s.apply_filter(id, local_json(0.2)).unwrap());
    assert!(
        pixels <= 262_144,
        "checked on a small level, not 1.5 MP: {pixels}"
    );
    let rows = s.smart_filters(id).unwrap();
    assert_eq!(
        rows.iter()
            .map(|r| (r.filter_id.as_str(), r.name.as_str()))
            .collect::<Vec<_>>(),
        [("camera_raw", "Camera Raw Filter")]
    );
    assert_eq!(
        s.history_items().unwrap().last().unwrap().label,
        "Camera Raw Filter"
    );
}

// ─────────────── detail effects below 100 % (A's review of 117b44f8) ───────────────

/// `local_json` with the radius-dependent detail effects (sharpening, noise
/// reduction, Texture, Clarity) off: what a zoomed-out preview shows.
fn local_json_without_detail(exposure: f64) -> String {
    camera_raw(serde_json::json!({
        "tone": {"exposure": exposure, "contrast": 20.0, "highlights": -30.0, "shadows": 30.0,
                 "clarity": 0.0, "texture": 0.0},
        "detail": {"sharpening": {"amount": 0.0}, "noise_reduction": {"luminance": 0.0, "color": 0.0}},
        "color": {"saturation": 15.0},
    }))
}

/// The layer as presented at `level` with `json` previewed (`smart`: as a
/// re-edit of smart filter 0).
fn previewed(s: &DocumentSession, id: u64, json: String, level: u8, smart: bool) -> Vec<f32> {
    if smart {
        s.preview_smart_filter(id, 0, json, None).unwrap();
    } else {
        s.preview_filter(id, json, None).unwrap();
    }
    s.wait_filters_idle();
    assert_eq!(s.filter_error(), None);
    s.read_presented_level(level).unwrap().2
}

/// Below 100 % the preview renders a smaller pyramid level, where the detail
/// effects' full-resolution pixel radii would look 2-4x too wide: like Camera
/// Raw, the zoomed-out preview omits them (the sheet says so).
#[test]
fn zoomed_out_preview_omits_the_detail_effects() {
    let _g = serial();
    let (dir, engine) = engine();
    let (w, h) = (1024u32, 768u32);
    let s = open(&engine, &opaque_png(dir.path(), "z.png", w, h));
    let id = s.layers().unwrap()[0].id;
    for smart in [false, true] {
        if smart {
            s.convert_for_smart_filters(id).unwrap();
            s.apply_filter(
                id,
                camera_raw(serde_json::json!({"tone": {"exposure": 0.2}})),
            )
            .unwrap();
        }
        for level in [1u8, 2] {
            let (lw, lh) = (w >> level, h >> level);
            s.set_viewport(level, 0, 0, lw, lh, 1.0 / f64::from(1u8 << level))
                .unwrap();
            let with = previewed(&s, id, local_json(0.5), level, smart);
            let without = previewed(&s, id, local_json_without_detail(0.5), level, smart);
            let d = max_diff(&with, &without);
            assert!(
                d == 0.0,
                "level {level} (smart {smart}): preview includes detail effects, {d} from the preview without them"
            );
            assert!(
                max_diff(&with, &live(&s, level)) > 0.05,
                "the tone settings still preview"
            );
        }
    }
}

/// At 100 % the preview includes the detail effects and matches Apply in the
/// visible region; the 1:1 pane includes them whatever the zoom.
#[test]
fn preview_at_100_percent_and_the_detail_pane_keep_the_detail_effects() {
    let _g = serial();
    let (dir, engine) = engine();
    let (w, h) = (512usize, 384usize);
    let s = open(
        &engine,
        &opaque_png(dir.path(), "e.png", w as u32, h as u32),
    );
    let id = s.layers().unwrap()[0].id;
    let (x0, y0, rw, rh) = (150usize, 100usize, 160usize, 120usize);
    let r = region(x0 as i64, y0 as i64, rw as i64, rh as i64);
    s.set_viewport(0, x0 as u32, y0 as u32, rw as u32, rh as u32, 1.0)
        .unwrap();
    let at = |json: String| {
        s.preview_filter(id, json, r).unwrap();
        s.wait_filters_idle();
        assert_eq!(s.filter_error(), None);
        crop(&s.read_presented_level(0).unwrap().2, w, x0, y0, rw, rh)
    };
    let with = at(local_json(0.5));
    let without = at(local_json_without_detail(0.5));
    let d = max_diff(&with, &without);
    assert!(d > 0.02, "100 % preview shows the detail effects: {d}");

    // The 1:1 pane is level 0 and includes them even while zoomed out.
    s.set_viewport(2, 0, 0, (w / 4) as u32, (h / 4) as u32, 0.25)
        .unwrap();
    let pane_with = s
        .filter_detail(
            id,
            local_json(0.5),
            x0 as i64,
            y0 as i64,
            rw as u32,
            rh as u32,
        )
        .unwrap();
    assert_eq!(pane_with.level, 0);
    let d = max_diff(&pane(&pane_with), &with);
    assert!(d <= 0.01, "pane vs 100 % preview {d}");
    s.clear_preview().unwrap();

    // Apply (made while zoomed out) is exact: equal to the 100 % preview.
    s.apply_filter(id, local_json(0.5)).unwrap();
    let applied = crop(&live(&s, 0), w, x0, y0, rw, rh);
    let d = max_diff(&with, &applied);
    assert!(d <= 2.5 / 255.0, "100 % preview vs apply {d}");
    assert!(max_diff(&without, &applied) > 0.02, "apply includes detail");
}

// ──────── the note follows the submitted level (A's review of de753f5f) ────────

/// The viewport renders level `floor(log2(1 / zoom))`, so between 50 % and
/// 100 % it is still level 0: the preview includes the detail effects and
/// `filter_preview_level` (what the sheet's note reads) says 0. From 50 %
/// down (level >= 1) it says so and the effects are omitted.
#[test]
fn preview_level_decides_whether_the_detail_effects_are_omitted() {
    let _g = serial();
    let (dir, engine) = engine();
    let (w, h) = (512u32, 384u32);
    let s = open(&engine, &opaque_png(dir.path(), "b.png", w, h));
    let id = s.layers().unwrap()[0].id;
    for smart in [false, true] {
        if smart {
            s.convert_for_smart_filters(id).unwrap();
            s.apply_filter(
                id,
                camera_raw(serde_json::json!({"tone": {"exposure": 0.2}})),
            )
            .unwrap();
        }
        let index = smart.then_some(0u32);
        for (level, zoom) in [(0u8, 0.75), (0, 0.51), (1, 0.5), (1, 0.3), (2, 0.25)] {
            let (lw, lh) = (w >> level, h >> level);
            s.set_viewport(level, 0, 0, lw, lh, zoom).unwrap();
            let submitted = s.filter_preview_level(id, index, local_json(0.5)).unwrap();
            assert_eq!(
                submitted, level,
                "zoom {zoom} (smart {smart}): the preview renders level {level}"
            );
            let with = previewed(&s, id, local_json(0.5), level, smart);
            let without = previewed(&s, id, local_json_without_detail(0.5), level, smart);
            let d = max_diff(&with, &without);
            if level == 0 {
                assert!(
                    d > 0.02,
                    "zoom {zoom} (smart {smart}): level 0 preview includes the detail effects: {d}"
                );
            } else {
                assert!(
                    d == 0.0,
                    "zoom {zoom} (smart {smart}): level {level} preview omits the detail effects: {d}"
                );
            }
        }
        s.clear_preview().unwrap();
    }
}

/// B5-34: every saved stage and every edited stage follows the canvas level;
/// zoom transitions must not reuse a bake with the opposite detail policy.
#[test]
fn stacked_camera_raw_detail_follows_canvas_level() {
    let _g = serial();
    let (dir, engine) = engine();
    let path = opaque_png(dir.path(), "stack.png", 256, 192);
    let s = open(&engine, &path);
    let zero = open(&engine, &opaque_png(dir.path(), "stack-zero.png", 256, 192));
    let applied = open(
        &engine,
        &opaque_png(dir.path(), "stack-applied.png", 256, 192),
    );
    let id = s.layers().unwrap()[0].id;
    let zid = zero.layers().unwrap()[0].id;
    let aid = applied.layers().unwrap()[0].id;
    s.convert_for_smart_filters(id).unwrap();
    zero.convert_for_smart_filters(zid).unwrap();
    for exposure in [0.1, 0.2] {
        s.apply_filter(id, local_json(exposure)).unwrap();
        zero.apply_filter(zid, local_json_without_detail(exposure))
            .unwrap();
        applied.apply_filter(aid, local_json(exposure)).unwrap();
    }
    let saved = s
        .smart_filters(id)
        .unwrap()
        .iter()
        .map(|r| r.filter_json.clone())
        .collect::<Vec<_>>();
    for level in [2u8, 0, 1, 2, 0] {
        s.clear_preview().unwrap();
        s.set_viewport(
            level,
            0,
            0,
            256 >> level,
            192 >> level,
            1.0 / f64::from(1u8 << level),
        )
        .unwrap();
        let expected = zero.read_presented_level(level).unwrap().2;
        let canvas = s.read_presented_level(level).unwrap().2;
        let diff = max_diff(&canvas, &expected);
        println!("B5-34 L{level} saved vs zeroed: {diff}");
        if level > 0 {
            assert_eq!(
                diff, 0.0,
                "all saved Camera Raw stages omit detail at L{level}"
            );
        } else {
            assert!(diff > 0.02, "100% keeps detail");
            let d = max_diff(
                &crop(&canvas, 256, 64, 48, 128, 96),
                &crop(&live(&applied, 0), 256, 64, 48, 128, 96),
            );
            println!("B5-34 L0 vs Apply: {d}");
            assert!(d <= 2.5 / 255.0);
        }
        for index in [0u32, 1] {
            s.preview_smart_filter(
                id,
                index,
                local_json(if index == 0 { 0.1 } else { 0.2 }),
                None,
            )
            .unwrap();
            s.wait_filters_idle();
            assert_eq!(s.filter_error(), None);
            let preview = s.read_presented_level(level).unwrap().2;
            let d = max_diff(&preview, &canvas);
            println!("B5-34 L{level} edit {index} vs saved: {d}");
            assert!(d <= 2.5 / 255.0);
        }
    }
    s.clear_preview().unwrap();
    s.set_viewport(2, 0, 0, 64, 48, 0.25).unwrap();
    let detail = s
        .smart_filter_detail(id, 1, local_json(0.2), 64, 48, 128, 96)
        .unwrap();
    assert_eq!(detail.level, 0);
    let exact = crop(&live(&applied, 0), 256, 64, 48, 128, 96);
    let d = max_diff(&pane(&detail), &exact);
    println!("B5-34 zoomed-out 1:1 pane vs Apply: {d}");
    assert!(d <= 2.5 / 255.0);
    let out = dir.path().join("stack-export.png");
    s.export_flat(
        out.to_string_lossy().into_owned(),
        ExportFormat::Png,
        90,
        ExportColor::Document,
    )
    .unwrap();
    let exported: Vec<f32> = image::open(out)
        .unwrap()
        .to_rgba8()
        .as_raw()
        .iter()
        .map(|v| f32::from(*v) / 255.0)
        .collect();
    let d = max_diff(&crop(&exported, 256, 64, 48, 128, 96), &exact);
    println!("B5-34 zoomed-out export vs Apply: {d}");
    assert!(d <= 2.5 / 255.0);
    assert_eq!(
        saved,
        s.smart_filters(id)
            .unwrap()
            .iter()
            .map(|r| r.filter_json.clone())
            .collect::<Vec<_>>()
    );
}
