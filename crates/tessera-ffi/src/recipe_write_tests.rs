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
    let original = sidecar::RecipeDocument {
        recipe: engine_api::recipe::Recipe::new(engine_api::id::ImageId(1)),
        ..Default::default()
    };
    let render_hash = original.recipe.recipe_hash();
    Sidecar::write_recipe(&paths.recipe, &original).unwrap();
    let gate = gate_for(&image).unwrap();
    let first = gate.capture_revision(&image).unwrap();
    assert!(gate.matches_revision(&first, &image).unwrap());

    // Both documents are valid and have equal actual render hashes. Selection,
    // history, sync metadata, and a future envelope field still alter the
    // write revision. Current RecipeDocument reads but does not retain that
    // unknown envelope member; this test does not claim lossless persistence.
    let mut changed = original.clone();
    changed.recipe.selection = engine_api::recipe::Selection::keep(None);
    changed.recipe.create_snapshot("checkpoint", 1).unwrap();
    changed.record_write("test", 1).unwrap();
    assert_eq!(render_hash, changed.recipe.recipe_hash());
    let mut changed_json = serde_json::to_value(&changed).unwrap();
    changed_json["future_envelope"] = serde_json::json!({"unknown": true});
    fs::write(&paths.recipe, serde_json::to_vec(&changed_json).unwrap()).unwrap();
    let decoded = Sidecar::read_recipe(&paths.recipe).unwrap();
    assert!(!gate.matches_revision(&first, &image).unwrap());
    let with_unknown = gate.capture_revision(&image).unwrap();
    changed_json["future_envelope"] = serde_json::json!({"unknown": false});
    fs::write(&paths.recipe, serde_json::to_vec(&changed_json).unwrap()).unwrap();
    assert_eq!(decoded, Sidecar::read_recipe(&paths.recipe).unwrap());
    assert!(!gate.matches_revision(&with_unknown, &image).unwrap());
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
fn capture_revision_waits_for_a_participating_write() {
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join("one.jpg");
    tiny_jpeg(&image);
    let gate = gate_for(&image).unwrap();
    let held = gate.begin_write().unwrap();
    let contended = gate.observe_next_contended_gate_attempt();
    let (done_tx, done_rx) = mpsc::channel();
    let capturing_gate = gate.clone();
    let capturing_image = image.clone();
    let worker = thread::spawn(move || {
        done_tx
            .send(capturing_gate.capture_revision(&capturing_image))
            .unwrap();
    });
    contended.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(matches!(done_rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
    drop(held);
    let revision = done_rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    worker.join().unwrap();
    assert!(gate.matches_revision(&revision, &image).unwrap());
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
    let other = photos.join("two.jpg");
    tiny_jpeg(&image);
    tiny_jpeg(&other);
    let support = dir.path().join("support").to_string_lossy().into_owned();
    let first = Engine::open(support.clone()).unwrap();
    first
        .index_folder(photos.to_string_lossy().into_owned())
        .unwrap();
    let second = Engine::open(support).unwrap();
    let rows = first.list_images(ImageQuery::default()).unwrap();
    let id = rows
        .iter()
        .find(|r| r.path.ends_with("one.jpg"))
        .unwrap()
        .id
        .clone();
    let other_id = rows
        .iter()
        .find(|r| r.path.ends_with("two.jpg"))
        .unwrap()
        .id
        .clone();
    let gate = gate_for(&image).unwrap();
    let held = gate.begin_write().unwrap();
    // Installed after `held`, so this can only be signaled after the Engine
    // actually reaches this gate and observes its mutex is contended. A
    // test-only try_lock probe in begin_write supplies that handshake.
    let contended = gate.observe_next_contended_gate_attempt();
    let (done_tx, done_rx) = mpsc::channel();
    let blocked_id = id.clone();
    let unrelated_engine = second.clone();
    let worker = thread::spawn(move || {
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
    contended.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(matches!(done_rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
    // A different recipe destination can make progress while `one.jpg` is
    // held. This also catches an Engine that waits while holding its catalog.
    let (other_tx, other_rx) = mpsc::channel();
    let unrelated = thread::spawn(move || {
        other_tx
            .send(unrelated_engine.set_selection(
                other_id,
                Selection {
                    decision: Decision::Keep,
                    grade: None,
                    mark: None,
                },
            ))
            .unwrap();
    });
    other_rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    unrelated.join().unwrap();
    drop(held);
    done_rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    worker.join().unwrap();
    assert_keep_persisted(&first, &image, &blocked_id, Some(1));
    let unrelated_id = rows
        .iter()
        .find(|r| r.path.ends_with("two.jpg"))
        .unwrap()
        .id
        .clone();
    assert_keep_persisted(&first, &other, &unrelated_id, None);
}

fn assert_keep_persisted(engine: &Arc<Engine>, image: &Path, id: &str, grade: Option<u8>) {
    let paths = Sidecar::paths(image);
    let doc = Sidecar::read_recipe(&paths.recipe).unwrap();
    let xmp = Sidecar::read_xmp(&paths.xmp).unwrap().selection().unwrap();
    let indexed = engine
        .list_images(ImageQuery::default())
        .unwrap()
        .into_iter()
        .find(|row| row.id == id)
        .unwrap();
    assert_eq!(
        doc.recipe.selection.decision,
        engine_api::recipe::Decision::Keep
    );
    assert_eq!(doc.recipe.selection.grade.map(u8::from), grade);
    assert_eq!(xmp.decision, engine_api::recipe::Decision::Keep);
    assert_eq!(xmp.grade.map(u8::from), grade);
    assert_eq!(indexed.selection.decision, Decision::Keep);
    assert_eq!(indexed.selection.grade, grade);
    assert_eq!(indexed.recipe_hash, doc.recipe.recipe_hash().to_string());
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
    let written_json = json.clone();
    let gate = gate_for(&image).unwrap();
    let held = gate.begin_write().unwrap();
    let contended = gate.observe_next_contended_gate_attempt();
    let (done_tx, done_rx) = mpsc::channel();
    let written_id = id.clone();
    let worker = thread::spawn(move || {
        done_tx.send(second.set_recipe_json(id, json)).unwrap();
    });
    contended.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(matches!(done_rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
    drop(held);
    done_rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    worker.join().unwrap();
    let paths = Sidecar::paths(&image);
    let doc = Sidecar::read_recipe(&paths.recipe).unwrap();
    let xmp = Sidecar::read_xmp(&paths.xmp).unwrap().selection().unwrap();
    let indexed = first
        .list_images(ImageQuery::default())
        .unwrap()
        .into_iter()
        .find(|row| row.id == written_id)
        .unwrap();
    assert_eq!(first.get_recipe(written_id).unwrap(), written_json);
    assert_eq!(doc.vector_clock.get("tessera-mac"), Some(&1));
    assert_eq!(xmp, doc.recipe.selection);
    assert_eq!(indexed.recipe_hash, doc.recipe.recipe_hash().to_string());
}
