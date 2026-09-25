use assert_cmd::Command;
use serde_json::Value;
#[test]
fn agent_style_only_dry_run_then_redo() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("photo.jpg");
    image::RgbImage::from_pixel(32, 32, image::Rgb([110, 120, 130]))
        .save(&path)
        .unwrap();
    let run = |dry: bool| {
        let mut c = Command::new(assert_cmd::cargo::cargo_bin!("tessera"));
        c.args([
            "--app-dir",
            dir.path().join("app").to_str().unwrap(),
            "agent",
            "edit",
            path.to_str().unwrap(),
            "--provider",
            "none",
            "--redo",
            "warmer, keep the sky",
        ]);
        if dry {
            c.arg("--dry-run");
        }
        let out = c.assert().success().get_output().stdout.clone();
        serde_json::from_slice::<Value>(&out).unwrap()
    };
    let result = run(true);
    assert!(
        result["review_queue"][0]["plans"][0][0]["rationale"]
            .as_str()
            .unwrap()
            .contains("warmer")
    );
    assert!(!sidecar::Sidecar::paths(&path).recipe.exists());
    run(false);
    assert!(sidecar::Sidecar::paths(&path).recipe.exists());
}
