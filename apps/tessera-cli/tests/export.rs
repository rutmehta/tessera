use assert_cmd::Command;
use image::ImageDecoder;
use serde_json::Value;
use std::path::Path;

#[test]
fn cached_models_cli_upscale_smoke() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let cache = std::env::var_os("TESSERA_ENHANCE_MODEL_CACHE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("tools/orchestrate/wp/M3-05/.cache"));
    for (factor, sha) in [(2, ml_enhance::SR_X2_SHA256), (4, ml_enhance::SR_X4_SHA256)] {
        let model = cache.join(format!("{sha}.onnx"));
        if !model.is_file() {
            eprintln!("SKIP: Real-ESRGAN x{factor} not cached");
            continue;
        }
        let temp = tempfile::tempdir().unwrap();
        let app = temp.path().join("app");
        std::fs::create_dir_all(app.join("models")).unwrap();
        std::fs::copy(
            root.join("crates/ml-runtime/models.toml"),
            app.join("models.toml"),
        )
        .unwrap();
        std::fs::copy(model, app.join("models").join(format!("{sha}.onnx"))).unwrap();
        let input = temp.path().join("photos");
        std::fs::create_dir(&input).unwrap();
        for name in ["one", "two"] {
            image::RgbImage::from_pixel(8, 6, image::Rgb([80, 120, 160]))
                .save(input.join(format!("{name}.png")))
                .unwrap();
        }
        let out = temp.path().join("out");
        let result = cli(&app)
            .arg("--json")
            .arg("export")
            .arg(&input)
            .arg("--out")
            .arg(&out)
            .args([
                "--format",
                "png",
                "--upscale",
                &factor.to_string(),
                "--jobs",
                "4",
            ])
            .assert()
            .success()
            .get_output()
            .clone();
        let summary: Value = serde_json::from_slice(&result.stdout).unwrap_or_else(|error| {
            panic!(
                "invalid CLI JSON: {error}; stdout={}",
                String::from_utf8_lossy(&result.stdout)
            )
        });
        assert_eq!(summary["completed"], 2);
        assert_eq!(summary["outputs"].as_array().unwrap().len(), 2);
        for (i, name) in ["one", "two"].iter().enumerate() {
            assert_eq!(
                image::image_dimensions(out.join(format!("{name}-{}.png", i + 1))).unwrap(),
                (8 * factor, 6 * factor)
            );
        }
    }
}

fn cli(app: &Path) -> Command {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("tessera"));
    cmd.arg("--app-dir").arg(app);
    cmd
}

#[test]
fn upscale_requires_app_local_registry_but_plain_export_does_not() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("input.png");
    image::RgbImage::from_pixel(8, 6, image::Rgb([80, 120, 160]))
        .save(&input)
        .unwrap();
    let out = temp.path().join("out");
    let app = temp.path().join("app");
    let result = cli(&app)
        .arg("export")
        .arg(&input)
        .arg("--out")
        .arg(&out)
        .args(["--format", "png", "--upscale", "2"])
        .assert()
        .failure()
        .get_output()
        .clone();
    assert!(String::from_utf8_lossy(&result.stderr).contains("models.toml"));
    assert!(!out.exists());
    cli(&app)
        .arg("export")
        .arg(&input)
        .arg("--out")
        .arg(&out)
        .args(["--format", "png"])
        .assert()
        .success();
    assert!(!app.join("models").exists());
}

#[test]
fn upscale_preflights_duplicate_names_across_serial_waves() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("photos");
    std::fs::create_dir(&input).unwrap();
    for name in ["one", "two"] {
        image::RgbImage::from_pixel(8, 6, image::Rgb([80, 120, 160]))
            .save(input.join(format!("{name}.png")))
            .unwrap();
    }
    let out = temp.path().join("out");
    let result = cli(&temp.path().join("app"))
        .arg("export")
        .arg(&input)
        .arg("--out")
        .arg(&out)
        .args([
            "--format",
            "png",
            "--upscale",
            "2",
            "--name",
            "same",
            "--jobs",
            "1",
        ])
        .assert()
        .failure()
        .get_output()
        .clone();
    assert!(String::from_utf8_lossy(&result.stderr).contains("duplicate output names"));
    assert!(!out.exists());
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
