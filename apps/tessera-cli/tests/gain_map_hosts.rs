#[path = "../../../crates/export/tests/support/gain_map_fixture.rs"]
mod gain_map_fixture;
#[test]
fn cli_gain_map_exports_and_rejects_incompatible_options() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.png");
    image::RgbImage::from_pixel(80, 16, image::Rgb([255; 3]))
        .save(&source)
        .unwrap();
    gain_map_fixture::hdr_recipe(&source);
    let out = dir.path().join("out");
    let command = || {
        let mut c = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("tessera"));
        c.arg("--app-dir")
            .arg(dir.path().join("app"))
            .arg("export")
            .arg(&source);
        c
    };
    command()
        .arg("--out")
        .arg(&out)
        .args(["--format", "jpeg", "--gain-map"])
        .assert()
        .success();
    gain_map_fixture::check(&out.join("source-1.jpg"));
    for bad in [
        vec!["--format", "original"],
        vec!["--format", "png"],
        vec!["--format", "jpeg", "--hdr", "pq"],
        vec!["--format", "jpeg", "--color-space", "p3"],
        vec!["--format", "jpeg", "--upscale", "2"],
        vec!["--format", "jpeg", "--bit-depth", "16"],
    ] {
        let failed = dir.path().join("invalid");
        command()
            .arg("--out")
            .arg(&failed)
            .arg("--gain-map")
            .args(bad)
            .assert()
            .failure();
        assert!(!failed.exists());
    }
}
