use engine_api::recipe::{Decision, Recipe};
use sidecar::{MarkPreset, RecipeDocument, Sidecar, WriteStamp, XmpPacket};
use std::collections::BTreeMap;

#[test]
fn lightroom_owned_destinations_reject_direct_writes() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/.scratch-lightroom-write");
    let _ = std::fs::remove_dir_all(&root);
    let packet = XmpPacket::from_selection(&Default::default(), &MarkPreset::default());
    for folder in [
        "X.lrdata",
        "Foo.lrcat-data",
        "Lightroom Catalog Previews.lrdata",
        "Catalog Smart Previews.lrdata",
    ] {
        let dir = root.join(folder);
        std::fs::create_dir_all(&dir).unwrap();
        let xmp = dir.join("photo.dng.xmp");
        let result = Sidecar::write_xmp(&xmp, &packet);
        assert!(result.is_err(), "accepted protected destination: {folder}");
        assert!(!xmp.exists());
        assert!(
            Sidecar::write_recipe(dir.join(".edits/photo.json"), &RecipeDocument::default())
                .is_err()
        );
        assert!(!dir.join(".edits").exists());
    }
    let normal = root.join("normal/photo.dng.xmp");
    Sidecar::write_xmp(&normal, &packet).unwrap();
    assert!(normal.exists());
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn lww_uses_write_time_not_vector_sum_and_converges() {
    let a = RecipeDocument {
        recipe: Recipe::default(),
        vector_clock: BTreeMap::from([("a".into(), 100)]),
        last_writer: WriteStamp {
            timestamp_ms: 10,
            machine_id: "a".into(),
            counter: 100,
        },
    };
    let mut b = RecipeDocument {
        recipe: Recipe::default(),
        vector_clock: BTreeMap::from([("b".into(), 1)]),
        last_writer: WriteStamp {
            timestamp_ms: 20,
            machine_id: "b".into(),
            counter: 1,
        },
    };
    b.recipe.selection.decision = Decision::Keep;
    let merged = Sidecar::merge_last_writer_wins(&a, &b).unwrap();
    assert_eq!(merged.recipe, b.recipe);
    assert_eq!(
        merged.vector_clock,
        BTreeMap::from([("a".into(), 100), ("b".into(), 1)])
    );
    assert_eq!(merged, Sidecar::merge_last_writer_wins(&b, &a).unwrap());
    assert_eq!(
        merged,
        Sidecar::merge_last_writer_wins(&merged, &a).unwrap()
    );
    assert_eq!(
        merged,
        Sidecar::merge_last_writer_wins(&merged, &merged).unwrap()
    );
    b.record_write("b", 15).unwrap();
    assert!(b.last_writer.timestamp_ms > 20);
    assert_eq!(b.vector_clock["b"], 2);
}
#[test]
fn xmp_disk_write_is_atomic_and_readable() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/.scratch-xmp");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("photo.ARW.xmp");
    let packet = XmpPacket::from_selection(&Default::default(), &MarkPreset::lightroom());
    Sidecar::write_xmp(&path, &packet).unwrap();
    assert_eq!(Sidecar::read_xmp(&path).unwrap(), packet);
    let bad = XmpPacket {
        xml: "<broken>".into(),
    };
    assert!(Sidecar::write_xmp(&path, &bad).is_err());
    assert_eq!(Sidecar::read_xmp(&path).unwrap(), packet);
    let blocked = dir.join("blocked");
    std::fs::create_dir(&blocked).unwrap();
    assert!(Sidecar::write_xmp(&blocked, &packet).is_err());
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
    std::fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn lightroom_owned_symlink_rejects_writes() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/.scratch-lightroom-links");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("X.lrdata")).unwrap();
    std::os::unix::fs::symlink(root.join("X.lrdata"), root.join("alias")).unwrap();
    std::fs::create_dir_all(root.join("catalog/photos")).unwrap();
    std::fs::write(root.join("catalog/X.lrcat"), b"fixture").unwrap();
    let packet = XmpPacket::from_selection(&Default::default(), &MarkPreset::default());
    for path in [root.join("alias/photo.xmp")] {
        assert!(Sidecar::write_xmp(&path, &packet).is_err());
        assert!(!path.exists());
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn protected_store_path_survives_original_going_offline() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/.scratch-lightroom-offline");
    let _ = std::fs::remove_dir_all(&root);
    let photo = root.join("X.lrdata/photo.dng");
    std::fs::create_dir_all(photo.parent().unwrap()).unwrap();
    std::fs::write(&photo, b"fixture").unwrap();
    let online = Sidecar::paths(&photo);
    std::fs::remove_file(&photo).unwrap();
    assert_eq!(Sidecar::paths(&photo), online);
    assert!(!online.recipe.starts_with(root.join("X.lrdata")));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn catalog_beside_photos_and_preview_substrings_are_writable() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/.scratch-narrow");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("Catalog.lrcat"), b"fixture").unwrap();
    for name in [
        "photo.xmp",
        "Smart Previews for client/photo.xmp",
        "catalog.lrdata.backup/photo.xmp",
    ] {
        let path = root.join(name);
        assert!(!Sidecar::is_lightroom_owned(&path));
        Sidecar::write_xmp(
            &path,
            &XmpPacket::from_selection(&Default::default(), &Default::default()),
        )
        .unwrap();
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn protected_edits_survive_parent_rename_in_app_store() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/.scratch-rename");
    let _ = std::fs::remove_dir_all(&root);
    let photo = root.join("Photos/X.lrdata/image.dng");
    std::fs::create_dir_all(photo.parent().unwrap()).unwrap();
    std::fs::write(&photo, root.to_string_lossy().as_bytes()).unwrap();
    let paths = Sidecar::paths(&photo);
    assert!(!paths.recipe.starts_with(&root), "store must be app-owned");
    let mut doc = RecipeDocument::default();
    doc.recipe.settings.tone.exposure = 1.25;
    Sidecar::write_recipe(&paths.recipe, &doc).unwrap();
    std::fs::rename(root.join("Photos"), root.join("Photos 1")).unwrap();
    let moved = Sidecar::paths(root.join("Photos 1/X.lrdata/image.dng"));
    assert_eq!(Sidecar::read_recipe(&moved.recipe).unwrap(), doc);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn protected_offline_alias_survives_process_restart() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/.scratch-offline-restart");
    let photo = root.join("X.lrdata/image.dng");
    if std::env::var_os("TESSERA_TEST_OFFLINE_CHILD").is_some() {
        let doc = Sidecar::read_recipe(Sidecar::paths(&photo).recipe).unwrap();
        assert_eq!(doc.recipe.settings.tone.exposure, 1.75);
        return;
    }
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(photo.parent().unwrap()).unwrap();
    std::fs::write(&photo, root.to_string_lossy().as_bytes()).unwrap();
    let mut doc = RecipeDocument::default();
    doc.recipe.settings.tone.exposure = 1.75;
    Sidecar::write_recipe(Sidecar::paths(&photo).recipe, &doc).unwrap();
    std::fs::remove_file(&photo).unwrap();
    assert!(
        std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "protected_offline_alias_survives_process_restart"
            ])
            .env("TESSERA_TEST_OFFLINE_CHILD", "1")
            .status()
            .unwrap()
            .success()
    );
    std::fs::remove_dir_all(root).unwrap();
}
