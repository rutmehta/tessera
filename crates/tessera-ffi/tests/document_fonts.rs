//! One font snapshot for every document compositor (WP B5-10b): a text layer
//! renders the same (docs/11 §1.3: at most one 8-bit code value) through the
//! viewport, export_flat, the composite thumbnail, the eyedropper, a Gaussian
//! blur preview / apply on a smart object, merge down, the styled-document CPU
//! fallback and the PSD composite, and a missing-font text layer fails every
//! one of those paths with the documented error instead of falling back.
//!
//! The bundled OFL Noto Sans fixture is added to the shared snapshot only (it
//! is not installed), so a compositor built without the snapshot reports a
//! missing font: that is what makes these cases fail before B5-10b.
#![cfg(target_os = "macos")]

use std::sync::{Arc, Once};
use tessera_ffi::*;
use typography::{TextModel, TextRun};

const W: u32 = 240;
const H: u32 = 120;
const NOTO: &str = "Noto Sans";
const SYSTEM: &str = "Helvetica";
const MISSING: &str = "No Such Family 12345";
/// The documented missing-font error (typography `Error::MissingFont`).
const MISSING_ERR: &str = "font unavailable: No Such Family 12345";
const BG: PaintColor = PaintColor {
    r: 0.2,
    g: 0.4,
    b: 0.6,
};

fn fixture_fonts() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        load_text_fonts_for_tests(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../typography/tests/fonts"
        ))
    });
}

fn engine() -> (tempfile::TempDir, Arc<Engine>) {
    fixture_fonts();
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(dir.path().join("support").to_string_lossy().into_owned()).unwrap();
    (dir, engine)
}

fn text(family: &str, s: &str) -> String {
    serde_json::to_string(&TextModel {
        runs: vec![TextRun {
            text: s.into(),
            family: family.into(),
            size: 40.0,
            color: [220, 30, 30, 255],
            ..TextRun::default()
        }],
        ..TextModel::default()
    })
    .unwrap()
}

fn at(x: f64, y: f64) -> TransformMatrix {
    TransformMatrix {
        a: 1.0,
        b: 0.0,
        c: x,
        d: 0.0,
        e: 1.0,
        f: y,
    }
}

/// An opaque background layer (filled with `BG`) under one text layer in
/// `family`; returns `(session, background id, text id)`.
fn document(e: &Arc<Engine>, family: &str) -> (Arc<DocumentSession>, u64, u64) {
    let s = e.clone().new_document(W, H, DocDepth::U8, None).unwrap();
    let bg = s.layers().unwrap()[0].id;
    s.select_all().unwrap();
    s.fill_selection(bg, SelectionFill::Color { color: BG }, 1.0)
        .unwrap();
    s.select_none().unwrap();
    let id = s
        .add_text_layer(
            String::new(),
            None,
            None,
            text(family, "Tessa"),
            at(12.0, 70.0),
            false,
        )
        .unwrap()
        .created[0];
    s.wait_idle();
    (s, bg, id)
}

fn q8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

fn quantize(rgba: &[f32]) -> Vec<u8> {
    rgba.iter().map(|v| q8(*v)).collect()
}

/// The viewport path (session renderer: resident on Metal, else CPU).
#[track_caller]
fn viewport(s: &DocumentSession) -> Vec<u8> {
    let (w, h, px) = s.read_level(0).unwrap();
    assert_eq!((w, h), (W, H));
    quantize(&px)
}

/// Largest per-sample difference.
fn worst(a: &[u8], b: &[u8]) -> u8 {
    assert_eq!(a.len(), b.len());
    a.iter().zip(b).map(|(x, y)| x.abs_diff(*y)).max().unwrap()
}

/// Samples that differ from the plain background (the text's ink).
fn ink(rgba: &[u8]) -> usize {
    let bg = [q8(BG.r), q8(BG.g), q8(BG.b)];
    rgba.chunks(4)
        .filter(|p| (0..3).any(|c| p[c].abs_diff(bg[c]) > 8))
        .count()
}

fn export_png(
    s: &DocumentSession,
    dir: &std::path::Path,
    name: &str,
) -> Result<Vec<u8>, BridgeError> {
    let path = dir.join(format!("{name}.png"));
    s.export_flat(
        path.to_string_lossy().into_owned(),
        ExportFormat::Png,
        100,
        ExportColor::Document,
    )?;
    Ok(image::open(&path).unwrap().to_rgba8().into_raw())
}

fn surface_rgba(sid: u32, w: u32, h: u32) -> Vec<u8> {
    let surface = tessera_ffi::surface::Surface::lookup(sid, w, h).unwrap();
    surface
        .with_pixels(|px, stride| {
            let mut out = Vec::with_capacity((w * h * 4) as usize);
            for y in 0..h as usize {
                out.extend_from_slice(&px[y * stride..y * stride + w as usize * 4]);
            }
            out
        })
        .unwrap()
}

fn gaussian(radius: f32) -> String {
    format!(r#"{{"id":"gaussian_blur","params":{{"radius":{radius}}}}}"#)
}

fn presented(s: &DocumentSession) -> Vec<u8> {
    let (w, h, px) = s.read_presented_level(0).unwrap();
    assert_eq!((w, h), (W, H));
    quantize(&px)
}

fn assert_missing<T: std::fmt::Debug>(r: Result<T, BridgeError>, path: &str) {
    match r {
        Ok(v) => panic!("{path}: rendered a missing font ({v:?})"),
        Err(e) => assert!(
            e.to_string().contains(MISSING_ERR),
            "{path}: expected the missing-font error, got {e}"
        ),
    }
}

#[test]
fn text_renders_identically_through_every_document_compositor() {
    let (dir, e) = engine();
    let (s, _bg, id) = document(&e, NOTO);
    let view = viewport(&s);
    assert!(ink(&view) > 200, "the text is visible ({})", ink(&view));

    // export_flat (io.rs).
    let png = export_png(&s, dir.path(), "flat").expect("export_flat");
    assert!(
        worst(&png, &view) <= 1,
        "export_flat {}",
        worst(&png, &view)
    );

    // Composite thumbnail at full size (Channels panel RGB row).
    let sid = s.composite_thumbnail(W).unwrap();
    let thumb = surface_rgba(sid, W, H);
    assert!(
        worst(&thumb, &view) <= 1,
        "thumbnail {}",
        worst(&thumb, &view)
    );

    // Eyedropper over the composite (tools.rs), on the reddest ink pixel.
    let (i, _) = view
        .chunks(4)
        .enumerate()
        .max_by_key(|(_, p)| i32::from(p[0]) - i32::from(p[2]))
        .unwrap();
    let (x, y) = ((i as u32 % W) as f32, (i as u32 / W) as f32);
    let c = s.sample_color(x + 0.5, y + 0.5, true, None, 0).unwrap();
    let got = [q8(c.r), q8(c.g), q8(c.b)];
    let want = [view[i * 4], view[i * 4 + 1], view[i * 4 + 2]];
    assert!(
        got.iter().zip(&want).all(|(a, b)| a.abs_diff(*b) <= 1),
        "eyedropper {got:?} vs viewport {want:?}"
    );

    // Merge down (document/render.rs composite_raster).
    s.merge_down(id).unwrap();
    s.wait_idle();
    let merged = viewport(&s);
    assert!(
        worst(&merged, &view) <= 1,
        "merge down {}",
        worst(&merged, &view)
    );
    s.undo().unwrap();
    s.wait_idle();
    assert!(worst(&viewport(&s), &view) <= 1, "undo merge");

    // Gaussian blur on the text as a smart object: preview (filter worker
    // compositor), apply (smart filter bake), export (output bake).
    // (The unfiltered smart object itself is not read back here: the
    // resident renderer's nested child renderers do not inherit the snapshot,
    // NEEDS.md 2; with a filter the presented document holds baked pixels.)
    s.convert_for_smart_filters(id).unwrap();
    s.preview_filter(id, gaussian(2.0), None).unwrap();
    let preview = presented(&s);
    assert_eq!(s.filter_error(), None);
    assert!(ink(&preview) > ink(&view), "the preview is blurred");
    // A blur moves ink without adding any: the mean colour stays.
    let mean = |v: &[u8], c: usize| v.chunks(4).map(|p| f64::from(p[c])).sum::<f64>();
    for c in 0..3 {
        let (a, b) = (mean(&preview, c), mean(&view, c));
        assert!((a - b).abs() / b < 0.01, "channel {c}: {a} vs {b}");
    }
    s.apply_filter(id, gaussian(2.0)).unwrap();
    s.clear_preview().unwrap();
    let applied = presented(&s);
    assert!(
        worst(&applied, &preview) <= 1,
        "apply = preview {}",
        worst(&applied, &preview)
    );
    let png = export_png(&s, dir.path(), "blurred").expect("export smart filter");
    assert!(
        worst(&png, &applied) <= 1,
        "export = viewport {}",
        worst(&png, &applied)
    );
    // Flatten bakes the same smart filter through composite_raster.
    s.flatten().unwrap();
    s.wait_idle();
    let flat = viewport(&s);
    assert!(
        worst(&flat, &applied) <= 1,
        "flatten {}",
        worst(&flat, &applied)
    );
}

#[test]
fn styled_text_uses_the_shared_fonts_on_the_cpu_fallback() {
    let (_dir, e) = engine();
    let (s, _bg, id) = document(&e, NOTO);
    let plain = viewport(&s);
    let styles = r#"{"effects":[{"kind":"drop_shadow","settings":{"distance":4.0,"size":2.0}}],"scale":1.0}"#;
    s.set_layer_styles_json(id, styles.into(), false).unwrap();
    s.wait_idle();
    // The resident program refuses styles; the CPU fallback renders them.
    let styled = viewport(&s);
    assert!(ink(&styled) > ink(&plain), "the shadow is drawn");
    let mut fonts = typography::TextRenderer::new();
    fonts.load_font_dir(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../typography/tests/fonts"
    ));
    let cpu = compositor::Compositor::new(64 << 20);
    cpu.set_text_renderer(fonts);
    let (_, reference) = cpu
        .render_level_rgba(
            &compositor::Document::new((*s.document_state().unwrap()).clone()),
            0,
        )
        .unwrap();
    assert!(worst(&styled, &quantize(&reference)) <= 1, "styled = CPU");
    let sid = s.layer_thumbnail(id, 64).unwrap();
    assert_ne!(sid, 0);
}

/// The PSD writer composites in the engine (crates/compositor psd.rs) with a
/// system-font renderer of its own (NEEDS.md 1), so this case uses an
/// installed family: the stored composite equals the viewport, before and
/// after reopening.
#[test]
fn psd_composite_matches_the_viewport() {
    let (dir, e) = engine();
    let (s, _bg, _id) = document(&e, SYSTEM);
    let view = viewport(&s);
    assert!(ink(&view) > 200);
    let path = dir.path().join("text.psd");
    s.save_as(path.to_string_lossy().into_owned()).unwrap();
    let psd = ::psd::PsdDocument::read(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!((psd.width, psd.height, psd.depth), (W, H, 8));
    let plane = (W * H) as usize;
    let mut composite = vec![255u8; plane * 4];
    let colours = if psd.layer_section.merged_alpha { 4 } else { 3 };
    for c in 0..colours {
        for i in 0..plane {
            composite[i * 4 + c] = psd.composite[c * plane + i];
        }
    }
    assert!(
        worst(&composite, &view) <= 1,
        "PSD composite {}",
        worst(&composite, &view)
    );
    s.close();
    let r = e
        .clone()
        .open_document(path.to_string_lossy().into_owned())
        .unwrap();
    r.wait_idle();
    assert!(worst(&viewport(&r), &view) <= 1, "reopened PSD");
    let sid = r.composite_thumbnail(W).unwrap();
    assert!(
        worst(&surface_rgba(sid, W, H), &view) <= 1,
        "reopened thumbnail"
    );
}

#[test]
fn a_missing_font_fails_every_path_with_the_documented_error() {
    let (dir, e) = engine();
    let (s, bg, id) = document(&e, MISSING);
    assert_missing(s.read_level(0), "viewport");
    assert_missing(export_png(&s, dir.path(), "missing"), "export_flat");
    assert_missing(s.composite_thumbnail(64), "composite thumbnail");
    assert_missing(s.layer_thumbnail(id, 64), "layer thumbnail");
    assert_missing(s.sample_color(20.0, 60.0, true, None, 1), "eyedropper");
    assert_missing(s.merge_down(id), "merge down");
    assert_missing(s.flatten(), "flatten");
    let psd = dir.path().join("missing.psd");
    assert_missing(s.save_as(psd.to_string_lossy().into_owned()), "PSD save");
    // The background alone still renders and exports.
    s.set_visible(id, false).unwrap();
    s.wait_idle();
    assert!(s.read_level(0).is_ok());
    s.set_visible(id, true).unwrap();
    // A filter preview of the text as a smart object reports the error.
    s.convert_for_smart_filters(id).unwrap();
    s.preview_filter(id, gaussian(2.0), None).unwrap();
    s.wait_filters_idle();
    let err = s.filter_error().expect("preview error");
    assert!(err.contains(MISSING_ERR), "filter preview: {err}");
    let _ = bg;
}

/// B5-10's black Channels thumbnails (step 359): after a PSD with text, an
/// alpha and a spot channel was reopened, the RGB / Red / Green / Blue rows
/// went black once a missing-font text layer was added. The PSD reopen is not
/// the cause: the composite thumbnail (the source of those four rows) fails
/// with the documented missing-font error, which the host's `try?` shows as an
/// empty (black) image. Saved channels are unaffected.
#[test]
fn channels_thumbnails_after_psd_reopen_and_a_missing_font() {
    let (dir, e) = engine();
    let (s, _bg, _id) = document(&e, SYSTEM);
    s.set_selection_rect(0, 0, 120, i64::from(H), 0.0).unwrap();
    s.save_selection_channel("Mask".into(), None, SelectionOp::Replace)
        .unwrap();
    let ink_color = PaintColor {
        r: 1.0,
        g: 0.8,
        b: 0.0,
    };
    s.new_spot_channel("Varnish".into(), ink_color, 0.4, true)
        .unwrap();
    s.select_none().unwrap();
    s.wait_idle();
    let view = viewport(&s);
    let path = dir.path().join("channels.psd");
    s.save_as(path.to_string_lossy().into_owned()).unwrap();
    s.close();
    let r = e
        .clone()
        .open_document(path.to_string_lossy().into_owned())
        .unwrap();
    r.wait_idle();
    let composite = |r: &DocumentSession| -> Result<Vec<u8>, BridgeError> {
        Ok(surface_rgba(r.composite_thumbnail(W)?, W, H))
    };
    assert!(worst(&composite(&r).unwrap(), &view) <= 1, "RGB rows");
    let channel_rows = |r: &DocumentSession| -> Vec<(u8, u8)> {
        r.document_channels()
            .unwrap()
            .iter()
            .map(|c| {
                let t = surface_rgba(r.channel_thumbnail(c.id, W).unwrap(), W, H);
                (t[(60 * 4) as usize], t[(200 * 4) as usize])
            })
            .collect()
    };
    assert_eq!(channel_rows(&r), vec![(255, 0), (255, 0)], "Mask, Varnish");
    // The step-359 edit: a text layer in a family that is not installed.
    let missing = r
        .add_text_layer(
            "Missing".into(),
            None,
            None,
            text(MISSING, "?"),
            at(10.0, 100.0),
            false,
        )
        .unwrap()
        .created[0];
    assert_missing(composite(&r), "composite thumbnail (RGB rows)");
    assert_eq!(channel_rows(&r), vec![(255, 0), (255, 0)], "saved channels");
    // Hiding the layer brings the composite back.
    r.set_visible(missing, false).unwrap();
    assert!(
        worst(&composite(&r).unwrap(), &view) <= 1,
        "hidden missing layer"
    );
}
