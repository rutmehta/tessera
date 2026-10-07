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
    let path = root.join("alias/photo.xmp");
    assert!(Sidecar::write_xmp(&path, &packet).is_err());
    assert!(!path.exists());
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
    doc.recipe
        .edit(
            engine_api::recipe::EditMeta::user("Exposure", 1),
            |settings| {
                settings.tone.exposure = 1.25;
            },
        )
        .unwrap();
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
    doc.recipe
        .edit(
            engine_api::recipe::EditMeta::user("Exposure", 1),
            |settings| {
                settings.tone.exposure = 1.75;
            },
        )
        .unwrap();
    let paths = Sidecar::paths(&photo);
    Sidecar::write_recipe(&paths.recipe, &doc).unwrap();
    // A pre-B5-38c string alias remains readable after a process restart.
    let store = paths
        .recipe
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let path_key = blake3::hash(photo.canonicalize().unwrap().as_os_str().as_encoded_bytes())
        .to_hex()
        .to_string();
    std::fs::write(
        store.join("paths").join(format!("{path_key}.json")),
        serde_json::to_vec(paths.recipe.file_stem().unwrap().to_str().unwrap()).unwrap(),
    )
    .unwrap();
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

#[test]
fn protected_rewrite_preserves_recipe_and_realiases_new_content() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/.scratch-rewrite");
    let _ = std::fs::remove_dir_all(&root);
    let photo = root.join("X.lrdata/image.dng");
    let support = root.join("support");
    std::fs::create_dir_all(photo.parent().unwrap()).unwrap();
    Sidecar::register_store(&photo.parent().unwrap().canonicalize().unwrap(), &support);
    std::fs::write(&photo, b"original preview").unwrap();
    let original = Sidecar::paths(&photo);
    let mut doc = RecipeDocument::default();
    doc.recipe
        .edit(engine_api::recipe::EditMeta::user("Exposure", 1), |s| {
            s.tone.exposure = 1.25
        })
        .unwrap();
    Sidecar::write_recipe(&original.recipe, &doc).unwrap();
    std::fs::write(&photo, b"regenerated preview with metadata").unwrap();
    let rewritten = Sidecar::paths(&photo);
    assert_eq!(Sidecar::read_recipe(&rewritten.recipe).unwrap(), doc);
    assert_eq!(
        rewritten, original,
        "keep the authoritative recipe destination stable"
    );
    Sidecar::write_recipe(&rewritten.recipe, &doc).unwrap();
    let hash = blake3::hash(b"regenerated preview with metadata")
        .to_hex()
        .to_string();
    let alias = support
        .join(".edits/lightroom/content")
        .join(format!("{hash}.json"));
    let key: String = serde_json::from_slice(&std::fs::read(alias).unwrap()).unwrap();
    assert_eq!(key, original.recipe.file_stem().unwrap().to_str().unwrap());
    let path_key = blake3::hash(photo.canonicalize().unwrap().as_os_str().as_encoded_bytes())
        .to_hex()
        .to_string();
    let path_alias = support
        .join(".edits/lightroom/paths")
        .join(format!("{path_key}.json"));
    let stored_alias: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path_alias).unwrap()).unwrap();
    assert_eq!(stored_alias["content_hash"], hash);
    assert_eq!(stored_alias["recipe_key"], key);
    let moved = photo.with_file_name("renamed.dng");
    std::fs::rename(&photo, &moved).unwrap();
    assert_eq!(
        Sidecar::read_recipe(Sidecar::paths(&moved).recipe).unwrap(),
        doc
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn protected_rewrite_does_not_redirect_another_paths_existing_recipe() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/.scratch-rewrite-collision");
    let folder = root.join("X.lrdata");
    let first = folder.join("first.dng");
    let second = folder.join("second.dng");
    if std::env::var_os("TESSERA_TEST_REWRITE_CHILD").is_some() {
        Sidecar::register_store(&folder.canonicalize().unwrap(), &root.join("support"));
        for (path, exposure) in [(&first, 1.25), (&second, -0.75)] {
            assert_eq!(
                Sidecar::read_recipe(Sidecar::paths(path).recipe)
                    .unwrap()
                    .recipe
                    .settings
                    .tone
                    .exposure,
                exposure
            );
        }
        return;
    }
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&folder).unwrap();
    Sidecar::register_store(&folder.canonicalize().unwrap(), &root.join("support"));
    for (path, bytes, exposure) in [
        (&first, b"first".as_slice(), 1.25),
        (&second, b"second".as_slice(), -0.75),
    ] {
        std::fs::write(path, bytes).unwrap();
        let mut doc = RecipeDocument::default();
        doc.recipe
            .edit(engine_api::recipe::EditMeta::user("Exposure", 1), |s| {
                s.tone.exposure = exposure
            })
            .unwrap();
        Sidecar::write_recipe(Sidecar::paths(path).recipe, &doc).unwrap();
    }
    std::fs::write(&first, b"second").unwrap();
    let paths = Sidecar::paths(&first);
    let doc = Sidecar::read_recipe(&paths.recipe).unwrap();
    assert_eq!(doc.recipe.settings.tone.exposure, 1.25);
    Sidecar::write_recipe(&paths.recipe, &doc).unwrap();
    assert_eq!(
        Sidecar::read_recipe(Sidecar::paths(&second).recipe)
            .unwrap()
            .recipe
            .settings
            .tone
            .exposure,
        -0.75
    );
    assert!(
        std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "protected_rewrite_does_not_redirect_another_paths_existing_recipe"
            ])
            .env("TESSERA_TEST_REWRITE_CHILD", "1")
            .status()
            .unwrap()
            .success()
    );
    std::fs::remove_dir_all(root).unwrap();
}

/// SP-INT2 (REV-SP-A S5): in-place Smart Previews pinned to their catalog
/// image keep separate recipes even when byte-identical, across restarts.
#[test]
fn pinned_protected_identities_keep_identical_sources_separate() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/.scratch-pinned");
    let bundle = root.join("Catalog Smart Previews.lrdata/A/A1B2");
    let support = root.join("support");
    let photos = [bundle.join("A1B2-one.dng"), bundle.join("A1B2-two.dng")];
    let register = || {
        Sidecar::register_store(&bundle.canonicalize().unwrap(), &support);
        let pins: Vec<_> = photos
            .iter()
            .enumerate()
            .map(|(i, photo)| (photo.clone(), format!("catalog image {i}").into_bytes()))
            .collect();
        Sidecar::pin_protected_identities(&pins).unwrap();
    };
    if std::env::var_os("TESSERA_TEST_PINNED_CHILD").is_some() {
        Sidecar::register_store(&bundle.canonicalize().unwrap(), &support);
        for (i, photo) in photos.iter().enumerate() {
            let doc = Sidecar::read_recipe(Sidecar::paths(photo).recipe).unwrap();
            assert_eq!(doc.recipe.settings.tone.exposure, i as f32 + 0.5);
        }
        return;
    }
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&bundle).unwrap();
    for photo in &photos {
        std::fs::write(photo, b"byte-identical smart preview").unwrap();
    }
    register();
    for (i, photo) in photos.iter().enumerate() {
        let mut doc = RecipeDocument::default();
        doc.recipe
            .edit(engine_api::recipe::EditMeta::user("Exposure", 1), |s| {
                s.tone.exposure = i as f32 + 0.5;
            })
            .unwrap();
        Sidecar::write_recipe(Sidecar::paths(photo).recipe, &doc).unwrap();
    }
    assert_ne!(
        Sidecar::paths(&photos[0]).recipe,
        Sidecar::paths(&photos[1]).recipe
    );
    assert!(
        std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "pinned_protected_identities_keep_identical_sources_separate"
            ])
            .env("TESSERA_TEST_PINNED_CHILD", "1")
            .status()
            .unwrap()
            .success()
    );
    std::fs::remove_dir_all(root).unwrap();
}
