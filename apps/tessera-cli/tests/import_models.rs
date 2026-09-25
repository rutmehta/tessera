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
    run(&[catalog, "--make-fixture", dir.to_str().unwrap()])
        .assert()
        .code(2);
}
