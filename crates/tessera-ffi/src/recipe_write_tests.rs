//! Proposed contract tests for the internal recipe write gate. Intentionally RED until
//! `recipe_write` exists; this file does not enable a batch or public CAS API.
use crate::{Decision, Engine, ImageQuery, Selection};
use sidecar::Sidecar;
use std::{
    fs,
    path::Path,
    sync::{Arc, mpsc},
    thread,
    time::Duration,
};

use crate::recipe_write::gate_for;

fn tiny_jpeg(path: &Path) {
    image::RgbImage::new(2, 2).save(path).unwrap();
}

#[test]
fn raw_revision_includes_unknown_envelope_and_xmp_bytes_and_presence() {
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join("one.jpg");
    tiny_jpeg(&image);
    let paths = Sidecar::paths(&image);
    fs::create_dir_all(paths.recipe.parent().unwrap()).unwrap();
    fs::write(&paths.recipe, br#"{"recipe":{},"future_envelope":1}"#).unwrap();
    let gate = gate_for(&image).unwrap();
    let first = gate.capture_revision(&image).unwrap();
    assert!(gate.matches_revision(&first, &image).unwrap());

    // The engine's render hash excludes this future envelope field. The raw
    // write revision must still change, even though the field is not decoded.
    fs::write(&paths.recipe, br#"{"recipe":{},"future_envelope":2}"#).unwrap();
    assert!(!gate.matches_revision(&first, &image).unwrap());
    let second = gate.capture_revision(&image).unwrap();
    fs::write(&paths.xmp, b"<xmp>one</xmp>").unwrap();
    assert!(!gate.matches_revision(&second, &image).unwrap());
    let third = gate.capture_revision(&image).unwrap();
    fs::write(&paths.xmp, b"<xmp>two</xmp>").unwrap();
    assert!(!gate.matches_revision(&third, &image).unwrap());
    let fourth = gate.capture_revision(&image).unwrap();
    fs::remove_file(&paths.xmp).unwrap();
    assert!(!gate.matches_revision(&fourth, &image).unwrap());

    // Legacy `one.xmp` is the selected packet until appended `one.jpg.xmp`
    // appears. A path selection change is itself a revision change.
    let legacy = image.with_extension("xmp");
    fs::write(&legacy, b"<xmp>same bytes</xmp>").unwrap();
    let legacy_revision = gate.capture_revision(&image).unwrap();
    fs::write(&paths.xmp, b"<xmp>same bytes</xmp>").unwrap();
    assert!(!gate.matches_revision(&legacy_revision, &image).unwrap());
}

#[test]
fn retained_revision_prevents_weak_key_epoch_reset_aba() {
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join("one.jpg");
    tiny_jpeg(&image);
    let gate = gate_for(&image).unwrap();
    let old = gate.capture_revision(&image).unwrap();
    drop(gate);

    // A pruned weak-key table must recover the same live key while `old`
    // retains its strong state. The attempted write leaves bytes unchanged.
    for i in 0..8 {
        let other = dir.path().join(format!("other-{i}.jpg"));
        drop(gate_for(&other).unwrap());
    }
    let recreated = gate_for(&image).unwrap();
    {
        let attempt = recreated.begin_write().unwrap();
        assert!(attempt.matches_revision(&old, &image).unwrap());
    }
    assert!(!recreated.matches_revision(&old, &image).unwrap());
}

#[test]
fn same_stem_sources_share_the_existing_recipe_destination_gate() {
    let dir = tempfile::tempdir().unwrap();
    let jpg = dir.path().join("same.jpg");
    let png = dir.path().join("same.png");
    tiny_jpeg(&jpg);
    image::RgbImage::new(2, 2).save(&png).unwrap();
    assert_eq!(Sidecar::paths(&jpg).recipe, Sidecar::paths(&png).recipe);
    assert!(Arc::ptr_eq(
        &gate_for(&jpg).unwrap(),
        &gate_for(&png).unwrap()
    ));

    // Existing document identity rejects one image reading the other's
    // same-stem sidecar. The gate must serialize this destination, not invent
    // a new per-extension recipe filename in this slice.
    let mut doc = sidecar::RecipeDocument::default();
    doc.recipe.image_id = Some(engine_api::id::ImageId(1));
    Sidecar::write_recipe(Sidecar::paths(&jpg).recipe, &doc).unwrap();
    assert!(crate::catalog::document(&png, engine_api::id::ImageId(2)).is_err());
}

#[test]
fn two_engine_writers_wait_for_the_same_destination_gate() {
    let dir = tempfile::tempdir().unwrap();
    let photos = dir.path().join("photos");
    fs::create_dir(&photos).unwrap();
    let image = photos.join("one.jpg");
    tiny_jpeg(&image);
    let support = dir.path().join("support").to_string_lossy().into_owned();
    let first = Engine::open(support.clone()).unwrap();
    first
        .index_folder(photos.to_string_lossy().into_owned())
        .unwrap();
    let second = Engine::open(support).unwrap();
    let id = first.list_images(ImageQuery::default()).unwrap()[0]
        .id
        .clone();
    let gate = gate_for(&image).unwrap();
    let held = gate.begin_write().unwrap();
    let (started_tx, started_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        started_tx.send(()).unwrap();
        let result = second.set_selection(
            id,
            Selection {
                decision: Decision::Keep,
                grade: Some(1),
                mark: None,
            },
        );
        done_tx.send(result).unwrap();
    });
    started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(matches!(
        done_rx.recv_timeout(Duration::from_millis(100)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    drop(held);
    done_rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    worker.join().unwrap();
    assert!(Sidecar::paths(&image).recipe.exists());
}

#[test]
fn set_recipe_json_waits_for_the_same_destination_gate() {
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join("one.jpg");
    tiny_jpeg(&image);
    let support = dir.path().join("support").to_string_lossy().into_owned();
    let first = Engine::open(support.clone()).unwrap();
    first
        .index_folder(dir.path().to_string_lossy().into_owned())
        .unwrap();
    let second = Engine::open(support).unwrap();
    let id = first.list_images(ImageQuery::default()).unwrap()[0]
        .id
        .clone();
    let json = first.get_recipe(id.clone()).unwrap();
    let gate = gate_for(&image).unwrap();
    let held = gate.begin_write().unwrap();
    let (started_tx, started_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        started_tx.send(()).unwrap();
        done_tx.send(second.set_recipe_json(id, json)).unwrap();
    });
    started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(matches!(
        done_rx.recv_timeout(Duration::from_millis(100)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    drop(held);
    done_rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    worker.join().unwrap();
}
