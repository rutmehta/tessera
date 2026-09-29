//! Photo Restoration in the app's Neural Filters (WP B5-17a, UI for M5-32):
//! the FFI catalogue lists every engine neural filter (no truncation), the
//! restoration kind dispatches to `neural/photo_restoration`, and a missing
//! DRUNet or out-of-range value changes nothing for any destination.
#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use tessera_ffi::*;

fn engine() -> (tempfile::TempDir, Arc<Engine>) {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(dir.path().join("support").to_string_lossy().into_owned()).unwrap();
    (dir, engine)
}

fn png(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    image::RgbaImage::from_fn(24, 20, |x, y| {
        image::Rgba([(x * 9) as u8, (y * 11) as u8, 90, 255])
    })
    .save(&path)
    .unwrap();
    path
}

fn open(engine: &Arc<Engine>, path: &Path) -> Arc<DocumentSession> {
    engine
        .clone()
        .open_document(path.to_string_lossy().into_owned())
        .unwrap()
}

/// Everything an apply could change.
fn snapshot(
    s: &DocumentSession,
    layer: u64,
) -> (
    Vec<DocHistoryItem>,
    usize,
    Vec<f32>,
    Vec<SmartFilterRecord>,
    bool,
) {
    s.wait_idle();
    let smart = s.smart_filters(layer).unwrap_or_default();
    (
        s.history_items().unwrap(),
        s.layers().unwrap().len(),
        s.read_presented_level(0).unwrap().2,
        smart,
        s.info().unwrap().dirty,
    )
}

#[test]
fn catalogue_lists_every_engine_neural_filter_including_photo_restoration() {
    let cat = neural_filters();
    assert_eq!(
        cat.len(),
        filters::neural_catalog().len(),
        "no catalogue entry may be dropped"
    );
    assert_eq!(
        cat.iter().map(|f| f.kind).collect::<Vec<_>>(),
        [
            NeuralFilterKind::SkinSmoothing,
            NeuralFilterKind::Colorize,
            NeuralFilterKind::JpegArtifactRemoval,
            NeuralFilterKind::PhotoRestoration,
        ]
    );
    for f in &cat {
        assert!(f.filter_id.starts_with("neural/"), "{}", f.filter_id);
    }
    let r = &cat[3];
    assert_eq!(r.filter_id, "neural/photo_restoration");
    assert_eq!(r.name, "Photo Restoration");
    assert!(r.requires_weights);
    assert_eq!(r.params.len(), 1);
    let p = &r.params[0];
    assert_eq!(p.key, "photo_enhancement");
    assert_eq!((p.min, p.max, p.default_value), (0.0, 1.0, 0.5));
    let lim = r.limitation.as_deref().unwrap_or_default();
    assert!(lim.contains("Denoise only"), "{lim}");
}

#[test]
fn photo_restoration_without_weights_changes_nothing_for_any_destination() {
    let (dir, engine) = engine();
    for smart in [false, true] {
        let s = open(&engine, &png(dir.path(), &format!("r{smart}.png")));
        let layer = s.layers().unwrap()[0].id;
        if smart {
            s.convert_for_smart_filters(layer).unwrap();
        }
        let dests: &[NeuralDestination] = if smart {
            &[NeuralDestination::CurrentLayer, NeuralDestination::SmartFilter]
        } else {
            &[
                NeuralDestination::CurrentLayer,
                NeuralDestination::NewLayer,
                NeuralDestination::SmartFilter,
            ]
        };
        for &dest in dests {
            let before = snapshot(&s, layer);
            let e = s
                .neural_filter(
                    layer,
                    NeuralFilterKind::PhotoRestoration,
                    r#"{"photo_enhancement":0.5}"#.into(),
                    dest,
                )
                .unwrap_err()
                .to_string();
            assert!(e.contains("weights"), "{dest:?}: {e}");
            assert!(e.contains("enhance/drunet-color"), "{dest:?}: {e}");
            assert!(e.contains("Photo Restoration"), "{dest:?}: {e}");
            assert_eq!(snapshot(&s, layer), before, "smart={smart} {dest:?}");
        }
        s.close();
    }
}

#[test]
fn photo_restoration_rejects_values_outside_its_range_before_anything_else() {
    let (dir, engine) = engine();
    let s = open(&engine, &png(dir.path(), "v.png"));
    let layer = s.layers().unwrap()[0].id;
    for bad in [
        r#"{"photo_enhancement":1.5}"#,
        r#"{"photo_enhancement":-0.1}"#,
        r#"{"photo_enhancement":"high"}"#,
        r#"{"scratch_reduction":0.5}"#,
    ] {
        let before = snapshot(&s, layer);
        let e = s
            .neural_filter(
                layer,
                NeuralFilterKind::PhotoRestoration,
                bad.into(),
                NeuralDestination::CurrentLayer,
            )
            .unwrap_err()
            .to_string();
        assert!(!e.contains("not installed"), "{bad}: validated first: {e}");
        assert!(
            e.contains("photo_enhancement") || e.contains("scratch_reduction"),
            "{bad}: {e}"
        );
        assert_eq!(snapshot(&s, layer), before, "{bad}");
    }
    s.close();
}

#[test]
fn drunet_model_row_names_photo_restoration() {
    let (dir, engine) = engine();
    let s = open(&engine, &png(dir.path(), "m.png"));
    let models = s.retouch_models().unwrap();
    let drunet = models
        .iter()
        .find(|m| m.model_id == "enhance/drunet-color")
        .unwrap();
    assert_eq!(
        drunet.used_by,
        "JPEG Artifact Removal, Photo Restoration (DRUNet)"
    );
    s.close();
}
