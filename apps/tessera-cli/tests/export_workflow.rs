use assert_cmd::Command;

#[test]
#[cfg(unix)]
fn after_export_script_receives_literal_output_paths() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("photo.png");
    image::RgbImage::from_pixel(8, 6, image::Rgb([70, 110, 140]))
        .save(&input)
        .unwrap();
    let script = dir.path().join("post export.sh");
    std::fs::write(
        &script,
        "#!/bin/sh\nprintf '%s\\n' \"$1\" > \"$1.receipt\"\nprintf 'must not corrupt JSON'\n",
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let out = dir.path().join("space ; literal");
    let result = Command::new(assert_cmd::cargo::cargo_bin!("tessera"))
        .arg("--app-dir")
        .arg(dir.path().join("app"))
        .arg("--json")
        .arg("export")
        .arg(&input)
        .arg("--out")
        .arg(&out)
        .args(["--format", "png", "--after-export-script"])
        .arg(&script)
        .assert()
        .success()
        .get_output()
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(json["completed"], 1);
    assert!(json["workflow_errors"].as_array().unwrap().is_empty());
    assert_eq!(
        std::fs::read_to_string(out.join("photo-1.png.receipt")).unwrap(),
        format!("{}\n", out.join("photo-1.png").display())
    );
}
