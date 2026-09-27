#[path = "../../../crates/export/tests/support/native_fixture.rs"]
mod native_fixture;
use native_fixture::{stamp, tags};
#[test]
fn cli_extracts_each_sources_native_metadata_and_filters() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input");
    std::fs::create_dir(&input).unwrap();
    for name in ["a", "b"] {
        let path = input.join(format!("{name}.jpg"));
        image::RgbImage::from_pixel(16, 16, image::Rgb([90; 3]))
            .save(&path)
            .unwrap();
        stamp(&path, name);
    }
    for policy in ["all", "copyright"] {
        let out = dir.path().join(policy);
        assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("tessera"))
            .arg("--app-dir")
            .arg(dir.path().join("app"))
            .arg("export")
            .arg(&input)
            .arg("--out")
            .arg(&out)
            .args([
                "--format",
                "png",
                "--name",
                "{name}",
                "--metadata",
                policy,
                "--jobs",
                "2",
            ])
            .assert()
            .success();
        for name in ["a", "b"] {
            let path = out.join(format!("{name}.png"));
            assert_eq!(tags(&path, &["-s3", "-EXIF:Copyright"]).trim(), name);
            assert_eq!(
                tags(&path, &["-s3", "-EXIF:Make"]).contains("Private Camera"),
                policy == "all"
            );
        }
    }
}
