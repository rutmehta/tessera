//! Export Flat off the caller's thread (WP B5-15, perf audit P16):
//! `begin_export_flat` snapshots, `DocFlatExport::run` renders and writes with
//! progress, `cancel` stops it without touching the destination; the pixels
//! are the CPU compositor's level 0, as before.
#![cfg(target_os = "macos")]

use compositor::{Compositor, DocOp, DocState, Document, Layer, raster::Depth};
use engine_api::tile::{Extent, Tile, TileCoord};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tessera_ffi::*;

fn engine() -> (tempfile::TempDir, Arc<Engine>) {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(dir.path().join("support").to_string_lossy().into_owned()).unwrap();
    (dir, engine)
}

fn noise(seed: u32, coord: TileCoord, layout: engine_api::tile::TileLayout, alpha: u8) -> Tile {
    let n = layout.plane_len();
    let mut v = vec![0u8; 4 * n];
    let mut x = seed.wrapping_mul(2654435761) ^ (coord.x << 16) ^ coord.y ^ 0x9e37;
    for i in 0..n {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        v[i] = x as u8;
        v[n + i] = (x >> 8) as u8;
        v[2 * n + i] = (x >> 16) as u8;
        v[3 * n + i] = if alpha == 0 { (x >> 24) as u8 } else { alpha };
    }
    Tile::from_samples(coord, layout, v).unwrap()
}

/// An 8-bit document: an opaque noise layer and a semi-transparent one above
/// in Multiply at 60 %.
fn document(w: u32, h: u32) -> Document {
    let e = Extent::new(w, h);
    let mut d = Document::new(DocState::new(e, Depth::U8));
    for (s, alpha) in [(1u32, 255u8), (2, 0)] {
        let mut l = Layer::pixel(format!("layer {s}"), e, Depth::U8);
        let r = l.raster_mut().unwrap();
        let (cols, rows) = r.grid();
        for ty in 0..rows {
            for tx in 0..cols {
                let t = noise(s, TileCoord::new(0, tx, ty), r.layout(tx, ty), alpha);
                r.set_slot(tx, ty, Some(t), 1).unwrap();
            }
        }
        if s == 2 {
            l.props.opacity = 0.6;
            l.props.blend_mode = compositor::BlendMode::Multiply;
        }
        d.apply(DocOp::AddLayer {
            parent: None,
            index: usize::MAX,
            layer: l,
        })
        .unwrap();
    }
    d
}

#[derive(Default)]
struct Progress(Mutex<Vec<(f32, String)>>);

impl DocExportListener for Progress {
    fn on_progress(&self, fraction: f32, phase: String) {
        self.0.lock().unwrap().push((fraction, phase));
    }
}

fn png_pixels(path: &Path) -> (u32, u32, Vec<u8>) {
    let img = image::open(path).unwrap().into_rgba8();
    (img.width(), img.height(), img.into_raw())
}

fn leftovers(dir: &Path, keep: &[&Path]) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| !keep.contains(&p.as_path()))
        .collect()
}

/// The exported pixels are the CPU compositor's level 0, quantized as
/// before, and the progress callbacks are monotonic and end at "Done".
#[test]
fn background_export_is_the_cpu_composite_with_monotonic_progress() {
    let (dir, engine) = engine();
    let doc = document(700, 530);
    let (e, rgba) = Compositor::new(64 << 20)
        .render_level_rgba(&doc, 0)
        .unwrap();
    let s = engine.adopt_document(doc, "flat".into());
    let out = dir.path().join("flat.png");
    let job = s
        .begin_export_flat(
            out.to_string_lossy().into_owned(),
            ExportFormat::Png,
            90,
            ExportColor::Document,
        )
        .unwrap();
    let progress = Arc::new(Progress::default());
    job.run(Some(progress.clone())).unwrap();
    let (w, h, px) = png_pixels(&out);
    assert_eq!((w, h), (e.width, e.height));
    let q8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    let expected: Vec<u8> = rgba.iter().map(|v| q8(*v)).collect();
    assert!(px == expected, "exported pixels differ from the CPU composite");
    let p = progress.0.lock().unwrap();
    assert!(p.windows(2).all(|w| w[0].0 <= w[1].0), "{p:?}");
    assert_eq!(p.last().unwrap(), &(1.0, "Done".to_owned()));
    assert!(p.iter().any(|(_, ph)| ph == "Compositing"));
    // A job runs once.
    assert!(job.run(None).is_err());
    // The synchronous call writes the same file.
    let sync = dir.path().join("sync.png");
    s.export_flat(
        sync.to_string_lossy().into_owned(),
        ExportFormat::Png,
        90,
        ExportColor::Document,
    )
    .unwrap();
    assert_eq!(std::fs::read(&sync).unwrap(), std::fs::read(&out).unwrap());
}

/// Cancelling mid-composite stops the export with an error, leaves the file
/// already at the destination byte for byte, and leaves no temporary file.
#[test]
fn cancel_mid_export_leaves_the_destination_untouched() {
    let (dir, engine) = engine();
    let s = engine.adopt_document(document(3000, 2000), "big".into());
    let out_dir = dir.path().join("out");
    std::fs::create_dir(&out_dir).unwrap();
    for (format, name) in [
        (ExportFormat::Png, "x.png"),
        (ExportFormat::Tiff, "x.tif"),
        (ExportFormat::Jpeg, "x.jpg"),
    ] {
        let out = out_dir.join(name);
        std::fs::write(&out, b"previous contents").unwrap();
        let job = s
            .begin_export_flat(
                out.to_string_lossy().into_owned(),
                format,
                80,
                ExportColor::DisplayP3,
            )
            .unwrap();
        struct CancelAt(Arc<DocFlatExport>, Mutex<Vec<f32>>);
        impl DocExportListener for CancelAt {
            fn on_progress(&self, fraction: f32, _: String) {
                self.1.lock().unwrap().push(fraction);
                if fraction >= 0.2 {
                    self.0.cancel();
                }
            }
        }
        let l = Arc::new(CancelAt(job.clone(), Mutex::new(Vec::new())));
        let t = Instant::now();
        let r = job.run(Some(l.clone()));
        let err = r.expect_err("cancelled export must fail").to_string();
        assert!(err.contains("cancelled"), "{err}");
        assert!(job.is_cancelled());
        let seen = l.1.lock().unwrap();
        assert!(
            seen.iter().all(|f| *f < 0.9),
            "cancelled after encoding started: {seen:?}"
        );
        eprintln!(
            "{name}: cancelled at {:.2} after {:.0} ms",
            seen.last().unwrap(),
            t.elapsed().as_secs_f64() * 1000.0
        );
        assert_eq!(std::fs::read(&out).unwrap(), b"previous contents");
    }
    let keep: Vec<PathBuf> = ["x.png", "x.tif", "x.jpg"]
        .iter()
        .map(|n| out_dir.join(n))
        .collect();
    let keep: Vec<&Path> = keep.iter().map(PathBuf::as_path).collect();
    assert_eq!(leftovers(&out_dir, &keep), Vec::<PathBuf>::new());
    // Cancelled before it runs: nothing is written.
    let fresh = out_dir.join("never.png");
    let job = s
        .begin_export_flat(
            fresh.to_string_lossy().into_owned(),
            ExportFormat::Png,
            90,
            ExportColor::Srgb,
        )
        .unwrap();
    job.cancel();
    assert!(job.run(None).is_err());
    assert!(!fresh.exists());
}

/// The export shows the document as it was when it began: later edits,
/// undo and closing the document do not change or stop it.
#[test]
fn export_is_a_snapshot_that_outlives_edits_and_close() {
    let (dir, engine) = engine();
    let s = engine.adopt_document(document(900, 600), "snap".into());
    let top = s.layers().unwrap()[0].id;
    let before = dir.path().join("before.png");
    s.export_flat(
        before.to_string_lossy().into_owned(),
        ExportFormat::Png,
        90,
        ExportColor::Srgb,
    )
    .unwrap();
    let out = dir.path().join("snap.png");
    let job = s
        .begin_export_flat(
            out.to_string_lossy().into_owned(),
            ExportFormat::Png,
            90,
            ExportColor::Srgb,
        )
        .unwrap();
    s.set_opacity(top, 0.1, false).unwrap();
    s.set_visible(top, false).unwrap();
    s.close();
    job.run(None).unwrap();
    assert_eq!(png_pixels(&out), png_pixels(&before));
    // Invalid settings fail when the export begins, on the caller.
    assert!(
        engine
            .adopt_document(document(64, 64), "q".into())
            .begin_export_flat(
                dir.path().join("q.jpg").to_string_lossy().into_owned(),
                ExportFormat::Jpeg,
                0,
                ExportColor::Srgb,
            )
            .is_err()
    );
}

/// `begin_export_flat` (what the main thread calls) only snapshots; the
/// work is in `run`. Frames and edits continue while an export runs on
/// another thread.
#[test]
fn begin_is_cheap_and_edits_continue_during_run() {
    let (dir, engine) = engine();
    let s = engine.adopt_document(document(4000, 3000), "cheap".into());
    let top = s.layers().unwrap()[0].id;
    let out = dir.path().join("cheap.tif");
    let t = Instant::now();
    let job = s
        .begin_export_flat(
            out.to_string_lossy().into_owned(),
            ExportFormat::Tiff,
            90,
            ExportColor::Srgb,
        )
        .unwrap();
    let begin = t.elapsed();
    assert!(begin < Duration::from_millis(50), "begin took {begin:?}");
    let runner = std::thread::spawn(move || job.run(None));
    let mut worst = Duration::ZERO;
    for i in 0..40 {
        let t = Instant::now();
        s.set_opacity(top, 0.2 + (i % 5) as f32 * 0.1, true).unwrap();
        worst = worst.max(t.elapsed());
        std::thread::sleep(Duration::from_millis(5));
    }
    s.commit("Opacity".into()).unwrap();
    runner.join().unwrap().unwrap();
    eprintln!("begin {begin:?}; slowest edit during the export {worst:?}");
    assert!(worst < Duration::from_millis(50), "edit took {worst:?}");
    assert!(out.exists());
}
