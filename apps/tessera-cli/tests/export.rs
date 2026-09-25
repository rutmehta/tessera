use assert_cmd::Command;
use image::ImageDecoder;
use serde_json::Value;
use std::path::Path;

fn cli(app: &Path) -> Command {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("tessera"));
    cmd.arg("--app-dir").arg(app);
    cmd
}

#[test]
fn exports_two_images_with_long_edge_and_json_progress() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("photos");
    let output = temp.path().join("out");
    std::fs::create_dir(&input).unwrap();
    for name in ["one", "two"] {
        image::RgbImage::from_pixel(1200, 600, image::Rgb([80, 120, 160]))
            .save(input.join(format!("{name}.png")))
            .unwrap();
    }
    let result = cli(&temp.path().join("app"))
        .args([
            "--json",
            "export",
            input.to_str().unwrap(),
            "--out",
            output.to_str().unwrap(),
            "--format",
            "jpeg",
            "--long-edge",
            "800",
            "--jobs",
            "2",
            "--metadata",
            "none",
        ])
        .assert()
        .success()
        .get_output()
        .clone();
    let summary: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(summary["completed"], 2);
    assert_eq!(summary["total"], 2);
    assert!(String::from_utf8_lossy(&result.stderr).contains("1/2"));
    for (n, name) in ["one", "two"].iter().enumerate() {
        let path = output.join(format!("{name}-{}.jpg", n + 1));
        let mut decoder = image::codecs::jpeg::JpegDecoder::new(std::io::BufReader::new(
            std::fs::File::open(&path).unwrap(),
        ))
        .unwrap();
        assert_eq!(decoder.dimensions(), (800, 400));
        assert!(decoder.xmp_metadata().unwrap().is_none());
        assert!(!output.join(format!("{name}-{}.jpg.xmp", n + 1)).exists());
    }
}

#[test]
fn exports_index_query() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("photos");
    let output = temp.path().join("out");
    std::fs::create_dir(&input).unwrap();
    image::RgbImage::from_pixel(8, 4, image::Rgb([80, 120, 160]))
        .save(input.join("match.jpg"))
        .unwrap();
    let app = temp.path().join("app");
    cli(&app)
        .args(["index", input.to_str().unwrap()])
        .assert()
        .success();
    let result = cli(&app)
        .args([
            "--json",
            "export",
            "--query",
            "match",
            "--out",
            output.to_str().unwrap(),
            "--format",
            "png",
            "--metadata",
            "none",
        ])
        .assert()
        .success()
        .get_output()
        .clone();
    let summary: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(summary["completed"], 1);
    assert!(output.join("match-1.png").exists());
}
