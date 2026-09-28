//! Checked Document Save As destination outcomes through the real session.
//! SOURCE ONLY / UNRUN until the exclusive native compiler lane is released.
#![cfg(target_os = "macos")]

use std::sync::Arc;
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
            "{ext} output should be parseable by its magic"
        );

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
        assert!(!session.info().unwrap().dirty);
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
}
