#[path = "../../../crates/import-lrcat/tests/make_fixture.rs"]
mod make_fixture;
use assert_cmd::Command;
use serde_json::Value;
#[test]
fn inspect_and_apply_preserve_source_and_virtual_copies() {
    let temp = tempfile::tempdir().unwrap();
    let catalog = temp.path().join("test.lrcat");
    drop(make_fixture::make_fixture(&catalog));
    let before = std::fs::read(&catalog).unwrap();
    let app = temp.path().join("app");
    let dest = temp.path().join("imported");
    let run = |args: &[&str]| {
        let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("tessera"));
        cmd.arg("--app-dir")
            .arg(&app)
            .args(["import", "lrcat"])
            .arg(&catalog)
            .args(args)
            .arg("--json");
        cmd
    };
    let output = run(&["--inspect"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let summary: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(summary["images"], 3);
    assert!(!dest.exists());
    run(&["--apply", "--dest", dest.to_str().unwrap()])
        .assert()
        .success();
    let plan: Value =
        serde_json::from_slice(&std::fs::read(dest.join("import-plan.json")).unwrap()).unwrap();
    assert_eq!(plan["images"].as_array().unwrap().len(), 3);
    assert_eq!(std::fs::read_dir(dest.join("recipes")).unwrap().count(), 3);
    assert!(dest.join("library.json").is_file());
    assert_eq!(std::fs::read(&catalog).unwrap(), before);
    run(&["--apply", "--dest", dest.to_str().unwrap()])
        .assert()
        .code(1);
    run(&["--inspect", "--apply"]).assert().code(2);
    run(&["--apply"]).assert().code(2);
}
#[test]
fn model_commands_and_export_usage() {
    let temp = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| {
        let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("tessera"));
        cmd.arg("--app-dir").arg(temp.path()).args(args);
        cmd
    };
    let out = run(&["ml", "models", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(
        serde_json::from_slice::<Value>(&out).unwrap()["models"],
        serde_json::json!([])
    );
    run(&["ml", "check", "--json"]).assert().code(1);
    let output = run(&["export", "--help"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(String::from_utf8_lossy(&output).contains("--long-edge"));
    run(&["export"]).assert().code(2);
}
#[test]
fn make_fixture_writes_an_inspectable_catalog() {
    let temp = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| {
        let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("tessera"));
        cmd.arg("--app-dir")
            .arg(temp.path().join("app"))
            .args(["import", "lrcat"])
            .args(args)
            .arg("--json");
        cmd
    };
    let dir = temp.path().join("lr");
    let out = run(&["--make-fixture", dir.to_str().unwrap()])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let paths: Value = serde_json::from_slice(&out).unwrap();
    let catalog = paths["catalog"].as_str().unwrap();
    assert!(dir.join("Photos/2026/wedding/ceremony-01.jpg").is_file());
    assert!(
        dir.join("Catalog/Fixture Previews.lrdata/previews.db")
            .is_file()
    );
    let out = run(&[catalog, "--inspect"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let summary: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(summary["images"], 7);
    assert_eq!(summary["virtual_copies"], 1);
    let references = temp.path().join("references");
    std::fs::create_dir(&references).unwrap();
    let out = run(&[
        catalog,
        "--inspect",
        "--fidelity",
        "--reference-dir",
        references.to_str().unwrap(),
    ])
    .assert()
    .success()
    .get_output()
    .stdout
    .clone();
    let summary: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(summary["images"], 7);
    assert_eq!(summary["fidelity"]["skipped"], 7);
    assert!(!temp.path().join("app").exists());
    run(&[
        "--make-fixture",
        dir.to_str().unwrap(),
        "--fidelity",
        "--reference-dir",
        references.to_str().unwrap(),
    ])
    .assert()
    .code(2);
    run(&[catalog, "--make-fixture", dir.to_str().unwrap()])
        .assert()
        .code(2);
}

#[test]
fn duplicate_image_ids_apply_last_write_wins() {
    let temp = tempfile::tempdir().unwrap();
    let catalog = temp.path().join("duplicate.lrcat");
    let c = make_fixture::make_fixture(&catalog);
    c.execute_batch(
        "INSERT INTO Adobe_images SELECT * FROM Adobe_images WHERE id_local=30;
        UPDATE Adobe_images SET rating=1 WHERE rowid=(SELECT MAX(rowid) FROM Adobe_images);",
    )
    .unwrap();
    drop(c);
    let dest = temp.path().join("bundle");
    Command::new(assert_cmd::cargo::cargo_bin!("tessera"))
        .args(["import", "lrcat"])
        .arg(&catalog)
        .arg("--apply")
        .arg("--dest")
        .arg(&dest)
        .arg("--json")
        .assert()
        .success();
    let plan: Value =
        serde_json::from_slice(&std::fs::read(dest.join("import-plan.json")).unwrap()).unwrap();
    let images = plan["images"].as_array().unwrap();
    assert_eq!(images.len(), 3);
    assert_eq!(
        images.iter().find(|i| i["catalog_id"] == 30).unwrap()["rating"],
        1
    );
    for image in images {
        let path = dest
            .join("recipes")
            .join(format!("{}.json", image["catalog_id"]));
        let document: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(document["recipe"], image["recipe"]);
    }
    assert_eq!(std::fs::read_dir(dest.join("recipes")).unwrap().count(), 3);
    assert!(
        plan["report"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e.as_str().unwrap().contains("last-write-wins"))
    );
}

#[test]
fn oversized_cells_are_externalized_losslessly() {
    let temp = tempfile::tempdir().unwrap();
    let catalog = temp.path().join("large.lrcat");
    let c = make_fixture::make_fixture(&catalog);
    let huge = format!(
        "s = {{ CameraProfile = '{}' }}",
        "x".repeat(import_lrcat::MAX_CELL_BYTES)
    );
    c.execute(
        "UPDATE Adobe_imageDevelopSettings SET text=?1 WHERE image=30",
        [&huge],
    )
    .unwrap();
    let blob = vec![0xff_u8; import_lrcat::MAX_CELL_BYTES + 7];
    c.execute(
        "UPDATE Adobe_libraryImageDevelopHistoryStep SET text=?1 WHERE image=30",
        [&blob],
    )
    .unwrap();
    drop(c);
    let dest = temp.path().join("bundle");
    Command::new(assert_cmd::cargo::cargo_bin!("tessera"))
        .args(["import", "lrcat"])
        .arg(&catalog)
        .arg("--apply")
        .arg("--dest")
        .arg(&dest)
        .arg("--json")
        .assert()
        .success();
    let plan: Value =
        serde_json::from_slice(&std::fs::read(dest.join("import-plan.json")).unwrap()).unwrap();
    let image = plan["images"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["catalog_id"] == 30)
        .unwrap();
    let cell = &image["recipe"]["lrcat_develop_source"]["cell"];
    assert_eq!(cell["status"], "externalized");
    let relative = cell["path"].as_str().unwrap();
    assert_eq!(std::fs::read(dest.join(relative)).unwrap(), huge.as_bytes());
    assert!(plan["report"].as_array().unwrap().iter().any(|r| {
        let s = r.as_str().unwrap();
        s.contains("image 30") && s.contains(relative) && s.contains("imported as unedited")
    }));
    let history = &image["recipe"]["lrcat_history"][0]["text"];
    assert_eq!(history["status"], "externalized");
    assert_eq!(
        std::fs::read(dest.join(history["path"].as_str().unwrap())).unwrap(),
        blob
    );
}
