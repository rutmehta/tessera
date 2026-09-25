use assert_cmd::Command;
use serde_json::Value;
use std::path::Path;
#[test]
fn develop_round_trip_preserves_other_settings() {
    let temp = tempfile::tempdir().unwrap();
    let photo = temp.path().join("one.jpg");
    image::RgbImage::new(2, 2).save(&photo).unwrap();
    let app = temp.path().join("app");
    cli(&app)
        .args([
            "develop",
            "set",
            photo.to_str().unwrap(),
            "--exposure",
            "0.5",
            "--contrast",
            "10",
            "--temperature",
            "5500",
            "--tint",
            "-2",
            "--vibrance",
            "15",
        ])
        .assert()
        .success();
    let settings = json(&app, &["develop", "show", photo.to_str().unwrap()]);
    assert_eq!(settings["tone"]["exposure"], 0.5);
    assert_eq!(settings["tone"]["contrast"], 10.0);
    cli(&app)
        .args(["develop", "set", photo.to_str().unwrap(), "--shadows", "20"])
        .assert()
        .success();
    let updated = json(&app, &["develop", "show", photo.to_str().unwrap()]);
    assert_eq!(updated["tone"]["exposure"], settings["tone"]["exposure"]);
    assert_eq!(updated["tone"]["shadows"], 20.0);
    cli(&app)
        .args([
            "develop",
            "set",
            photo.to_str().unwrap(),
            "--exposure",
            "100",
        ])
        .assert()
        .failure();
    assert_eq!(
        json(&app, &["develop", "show", photo.to_str().unwrap()]),
        updated
    );
}
fn cli(app: &Path) -> Command {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("tessera"));
    cmd.arg("--app-dir").arg(app);
    cmd
}
fn json(app: &Path, args: &[&str]) -> Value {
    serde_json::from_slice(
        &cli(app)
            .args(args)
            .arg("--json")
            .assert()
            .success()
            .get_output()
            .stdout,
    )
    .unwrap()
}
#[test]
fn index_is_incremental_and_lists_every_image() {
    let temp = tempfile::tempdir().unwrap();
    let photos = temp.path().join("photos");
    std::fs::create_dir(&photos).unwrap();
    for n in 0..105 {
        image::RgbImage::new(2, 2)
            .save(photos.join(format!("image{n}.jpg")))
            .unwrap();
    }
    let app = temp.path().join("app");
    assert_eq!(
        json(&app, &["index", photos.to_str().unwrap()])["changed"],
        105
    );
    assert_eq!(
        json(&app, &["index", photos.to_str().unwrap()])["changed"],
        0
    );
    assert_eq!(json(&app, &["ls"]).as_array().unwrap().len(), 105);
}

#[test]
fn prune_reports_and_removes_only_missing_files() {
    let temp = tempfile::tempdir().unwrap();
    let photos = temp.path().join("photos");
    std::fs::create_dir(&photos).unwrap();
    let gone = photos.join("gone.jpg");
    let live = photos.join("live.jpg");
    for photo in [&gone, &live] {
        image::RgbImage::new(2, 2).save(photo).unwrap();
    }
    let app = temp.path().join("app");
    json(&app, &["index", photos.to_str().unwrap()]);
    std::fs::remove_file(&gone).unwrap();
    assert_eq!(
        json(&app, &["index", "prune", "--dry-run"]),
        serde_json::json!({"images":1,"files":1,"dry_run":true})
    );
    assert_eq!(json(&app, &["ls"]).as_array().unwrap().len(), 2);
    assert_eq!(
        json(&app, &["index", "prune"]),
        serde_json::json!({"images":1,"files":1,"dry_run":false})
    );
    assert_eq!(json(&app, &["ls"]).as_array().unwrap().len(), 1);
    assert_eq!(json(&app, &["index", "prune"])["files"], 0);
    assert_eq!(
        json(&app, &["index", photos.to_str().unwrap()])["changed"],
        0
    );
}

#[test]
fn app_dir_flag_overrides_environment() {
    let temp = tempfile::tempdir().unwrap();
    let env_dir = temp.path().join("env");
    let flag_dir = temp.path().join("flag");
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("tessera"));
    cmd.env("TESSERA_APP_DIR", &env_dir)
        .args(["index", "prune"])
        .assert()
        .success();
    assert!(env_dir.join("index.sqlite").exists());
    cli(&flag_dir)
        .env("TESSERA_APP_DIR", &env_dir)
        .args(["index", "prune"])
        .assert()
        .success();
    assert!(flag_dir.join("index.sqlite").exists());
}

#[test]
fn cull_round_trip_and_reindex_sidecars() {
    let temp = tempfile::tempdir().unwrap();
    let photos = temp.path().join("photos");
    std::fs::create_dir(&photos).unwrap();
    let photo = photos.join("one.jpg");
    image::RgbImage::new(2, 2).save(&photo).unwrap();
    let app = temp.path().join("app");
    json(&app, &["index", photos.to_str().unwrap()]);
    cli(&app)
        .args([
            "cull",
            "set",
            photo.to_str().unwrap(),
            "--decision",
            "P",
            "--grade",
            "3",
            "--mark",
            "client",
        ])
        .assert()
        .success();
    let rows = json(&app, &["ls", "--decision", "keep"]);
    assert_eq!(rows.as_array().unwrap().len(), 1);
    assert_eq!(rows[0]["selection"]["grade"], 3);
    let rebuilt = temp.path().join("rebuilt");
    json(&rebuilt, &["index", photos.to_str().unwrap()]);
    assert_eq!(json(&rebuilt, &["ls", "--decision", "keep"]), rows);
    assert_eq!(
        json(&app, &["cull", "groups", photos.to_str().unwrap()])["groups"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(
        json(&app, &["cull", "sweep", photos.to_str().unwrap()])["defects"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
