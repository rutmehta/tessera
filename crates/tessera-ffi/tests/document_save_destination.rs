//! Checked Document Save As destination outcomes through the real session.
#![cfg(target_os = "macos")]

use std::sync::{Arc, Barrier};
use tessera_ffi::{DocDepth, DocumentSaveAsResult, DocumentSaveDestinationIntent, Engine};

fn new_document() -> (
    tempfile::TempDir,
    Arc<Engine>,
    Arc<tessera_ffi::DocumentSession>,
) {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(dir.path().join("support").to_string_lossy().into_owned()).unwrap();
    let session = engine
        .clone()
        .new_document(4, 4, DocDepth::U8, None)
        .unwrap();
    (dir, engine, session)
}

fn assert_reopens_with_tiny_extent(dir: &std::path::Path, path: &std::path::Path, label: &str) {
    let support = dir.join(format!("reopen-{label}"));
    let engine = Engine::open(support.to_string_lossy().into_owned()).unwrap();
    let reopened = engine
        .clone()
        .open_document(path.to_string_lossy().into_owned())
        .unwrap();
    let info = reopened.info().unwrap();
    assert_eq!((info.width, info.height), (4, 4));
}

#[test]
fn checked_collision_keeps_destination_and_unsaved_session_then_new_name_saves() {
    let (dir, engine, session) = new_document();
    let occupied = dir.path().join("occupied.tessera-doc");
    std::fs::write(&occupied, b"external-sentinel").unwrap();
    let before = session.info().unwrap();
    assert!(before.dirty);
    assert!(before.path.is_none());

    let outcome = session
        .save_as_checked(
            occupied.to_string_lossy().into_owned(),
            DocumentSaveDestinationIntent::CreateIfAbsent,
        )
        .unwrap();
    assert_eq!(outcome, DocumentSaveAsResult::DestinationExists);
    assert_eq!(std::fs::read(&occupied).unwrap(), b"external-sentinel");
    let after = session.info().unwrap();
    assert_eq!(after.path, before.path);
    assert_eq!(after.title, before.title);
    assert_eq!(after.dirty, before.dirty);
    assert_eq!(after.history_head, before.history_head);

    let fresh = dir.path().join("fresh.tessera-doc");
    assert_eq!(
        session
            .save_as_checked(
                fresh.to_string_lossy().into_owned(),
                DocumentSaveDestinationIntent::CreateIfAbsent
            )
            .unwrap(),
        DocumentSaveAsResult::Saved
    );
    let saved = session.info().unwrap();
    let fresh_path = fresh.to_string_lossy().into_owned();
    assert_eq!(saved.path.as_deref(), Some(fresh_path.as_str()));
    assert_eq!(saved.title, "fresh.tessera-doc");
    assert!(!saved.dirty);
    assert!(std::fs::read(&fresh).unwrap().starts_with(b"TSRDOC\0\x01"));
    assert_reopens_with_tiny_extent(dir.path(), &fresh, "fresh-native");
    session.close();
    let reopened = engine
        .clone()
        .open_document(fresh.to_string_lossy().into_owned())
        .unwrap();
    assert_ne!(reopened.id(), session.id());
    assert_eq!(reopened.info().unwrap().width, 4);
}

#[test]
fn checked_native_psd_and_psb_create_and_confirmed_replace() {
    for ext in ["tessera-doc", "psd", "psb"] {
        let (dir, _engine, session) = new_document();
        let destination = dir.path().join(format!("document.{ext}"));
        assert_eq!(
            session
                .save_as_checked(
                    destination.to_string_lossy().into_owned(),
                    DocumentSaveDestinationIntent::CreateIfAbsent
                )
                .unwrap(),
            DocumentSaveAsResult::Saved
        );
        let first = std::fs::read(&destination).unwrap();
        let expected_magic: &[u8] = if ext == "tessera-doc" {
            b"TSRDOC\0\x01"
        } else {
            b"8BPS"
        };
        assert!(
            first.starts_with(expected_magic),
            "{ext} output should have its expected format header"
        );
        assert_reopens_with_tiny_extent(dir.path(), &destination, &format!("created-{ext}"));

        std::fs::write(&destination, b"changed-after-approval").unwrap();
        assert_eq!(
            session
                .save_as_checked(
                    destination.to_string_lossy().into_owned(),
                    DocumentSaveDestinationIntent::ReplaceConfirmed
                )
                .unwrap(),
            DocumentSaveAsResult::Saved
        );
        assert!(
            std::fs::read(&destination)
                .unwrap()
                .starts_with(expected_magic)
        );
        assert_reopens_with_tiny_extent(dir.path(), &destination, &format!("replaced-{ext}"));
        assert!(!session.info().unwrap().dirty);
    }
}

#[test]
fn two_independent_sessions_race_same_absent_name_for_each_format() {
    for ext in ["tessera-doc", "psd", "psb"] {
        let destination_dir = tempfile::tempdir().unwrap();
        let destination = destination_dir.path().join(format!("shared.{ext}"));
        let (_left_dir, _left_engine, left) = new_document();
        let (_right_dir, _right_engine, right) = new_document();
        let barrier = Arc::new(Barrier::new(3));
        let (left_outcome, right_outcome) = std::thread::scope(|scope| {
            let left_barrier = barrier.clone();
            let left_session = left.clone();
            let left_path = destination.clone();
            let left_worker = scope.spawn(move || {
                left_barrier.wait();
                left_session
                    .save_as_checked(
                        left_path.to_string_lossy().into_owned(),
                        DocumentSaveDestinationIntent::CreateIfAbsent,
                    )
                    .unwrap()
            });
            let right_barrier = barrier.clone();
            let right_session = right.clone();
            let right_path = destination.clone();
            let right_worker = scope.spawn(move || {
                right_barrier.wait();
                right_session
                    .save_as_checked(
                        right_path.to_string_lossy().into_owned(),
                        DocumentSaveDestinationIntent::CreateIfAbsent,
                    )
                    .unwrap()
            });
            barrier.wait();
            (left_worker.join().unwrap(), right_worker.join().unwrap())
        });
        assert_eq!(
            [left_outcome, right_outcome]
                .into_iter()
                .filter(|outcome| *outcome == DocumentSaveAsResult::Saved)
                .count(),
            1
        );
        assert_eq!(
            [left_outcome, right_outcome]
                .into_iter()
                .filter(|outcome| *outcome == DocumentSaveAsResult::DestinationExists)
                .count(),
            1
        );
        let destination_path = destination.to_string_lossy().into_owned();
        for (outcome, session) in [(left_outcome, &left), (right_outcome, &right)] {
            let info = session.info().unwrap();
            if outcome == DocumentSaveAsResult::Saved {
                assert_eq!(info.path.as_deref(), Some(destination_path.as_str()));
                assert!(!info.dirty);
            } else {
                assert!(info.path.is_none());
                assert!(info.dirty);
            }
        }
        assert_reopens_with_tiny_extent(destination_dir.path(), &destination, ext);
    }
}

#[test]
fn checked_native_psd_and_psb_collisions_preserve_session_markers() {
    for ext in ["tessera-doc", "psd", "psb"] {
        let (dir, _engine, session) = new_document();
        let destination = dir.path().join(format!("occupied.{ext}"));
        std::fs::write(&destination, b"another-writer").unwrap();
        let before = session.info().unwrap();
        assert_eq!(
            session
                .save_as_checked(
                    destination.to_string_lossy().into_owned(),
                    DocumentSaveDestinationIntent::CreateIfAbsent
                )
                .unwrap(),
            DocumentSaveAsResult::DestinationExists
        );
        assert_eq!(std::fs::read(&destination).unwrap(), b"another-writer");
        let after = session.info().unwrap();
        assert_eq!(after.path, before.path);
        assert_eq!(after.title, before.title);
        assert_eq!(after.dirty, before.dirty);
        assert_eq!(after.history_head, before.history_head);
    }
}

#[test]
fn legacy_save_as_still_replaces_existing_destination() {
    let (dir, _engine, session) = new_document();
    let path = dir.path().join("legacy.tessera-doc");
    std::fs::write(&path, b"old").unwrap();
    session
        .save_as(path.to_string_lossy().into_owned())
        .unwrap();
    assert!(std::fs::read(&path).unwrap().starts_with(b"TSRDOC\0\x01"));
    assert!(!session.info().unwrap().dirty);

    std::fs::write(&path, b"changed-after-save-as").unwrap();
    session.save().unwrap();
    assert!(std::fs::read(&path).unwrap().starts_with(b"TSRDOC\0\x01"));
}
