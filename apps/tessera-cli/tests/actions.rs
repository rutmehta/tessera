use assert_cmd::Command;
use serde_json::{Value, json};

#[test]
fn actions_play_batches_inputs_into_out_dir() {
    let dir = tempfile::tempdir().unwrap();
    let action = dir.path().join("invert.tessera-action");
    std::fs::write(
        &action,
        json!({
            "format": "tessera-action",
            "version": 1,
            "name": "Invert half",
            "inputs": 1,
            "steps": [
                {"command": "apply_adjustment_layer",
                 "params": {"document": {"$input": 0}, "adjustment": {"kind": "invert"}},
                 "rationale": "negative look"},
                {"command": "set_layer_props",
                 "params": {"document": {"$input": 0}, "layer": {"$layer": 0}, "opacity": 1.0}},
                {"command": "merge_down",
                 "params": {"document": {"$input": 0}, "layer": {"$layer": 0}},
                 "enabled": false}
            ]
        })
        .to_string(),
    )
    .unwrap();
    let mut inputs = Vec::new();
    for (i, name) in ["one.png", "two.jpg"].iter().enumerate() {
        let path = dir.path().join(name);
        image::RgbImage::from_fn(20, 10, |x, y| {
            image::Rgb([(x * 10) as u8, (y * 20) as u8, 40 * i as u8])
        })
        .save(&path)
        .unwrap();
        inputs.push(path);
    }
    let out = dir.path().join("out");
    let output = Command::new(assert_cmd::cargo::cargo_bin!("tessera"))
        .arg("--app-dir")
        .arg(dir.path().join("app"))
        .args(["--json", "actions", "play"])
        .arg(&action)
        .args(&inputs)
        .arg("--out-dir")
        .arg(&out)
        .args(["--format", "png"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(report["runs"].as_array().unwrap().len(), 2);
    assert_eq!(report["runs"][0]["steps"], 3);
    let src = image::open(&inputs[0]).unwrap().to_rgb8();
    let got = image::open(out.join("one.png")).unwrap().to_rgba8();
    for (s, g) in src.pixels().zip(got.pixels()) {
        for c in 0..3 {
            assert_eq!(g[c], 255 - s[c]);
        }
    }
    assert!(out.join("two.png").exists());
    // Show validates; a malformed action fails with a message.
    Command::new(assert_cmd::cargo::cargo_bin!("tessera"))
        .args(["actions", "show"])
        .arg(&action)
        .assert()
        .success();
    let bad = dir.path().join("bad.tessera-action");
    std::fs::write(&bad, r#"{"format":"tessera-action","version":1,"name":"x","steps":[{"command":"nope","params":{}}]}"#).unwrap();
    let failed = Command::new(assert_cmd::cargo::cargo_bin!("tessera"))
        .arg("--app-dir")
        .arg(dir.path().join("app"))
        .args(["actions", "play"])
        .arg(&bad)
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();
    assert!(String::from_utf8_lossy(&failed).contains("unknown command"));
    // Inputs without --out-dir are refused.
    Command::new(assert_cmd::cargo::cargo_bin!("tessera"))
        .arg("--app-dir")
        .arg(dir.path().join("app"))
        .args(["actions", "play"])
        .arg(&action)
        .arg(&inputs[0])
        .assert()
        .failure();
}
