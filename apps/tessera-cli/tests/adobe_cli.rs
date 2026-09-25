#[path = "../../../crates/import-lrcat/tests/make_fixture.rs"]
mod make_fixture;
use assert_cmd::Command;
use serde_json::Value;

#[test]
fn dcp_is_an_explicit_adobe_only_input() {
    let temp = tempfile::tempdir().unwrap();
    let output = Command::new(assert_cmd::cargo::cargo_bin!("tessera"))
        .arg("--app-dir")
        .arg(temp.path())
        .args([
            "render",
            "missing.dng",
            "--out",
            "unused.png",
            "--process",
            "native",
            "--dcp",
            "profile.dcp",
        ])
        .assert()
        .code(1)
        .get_output()
        .clone();
    assert!(String::from_utf8_lossy(&output.stderr).contains("DCP requires Adobe"));
}

#[test]
fn render_process_is_a_validated_option() {
    let temp = tempfile::tempdir().unwrap();
    for mode in ["auto", "native", "adobe"] {
        let output = Command::new(assert_cmd::cargo::cargo_bin!("tessera"))
            .arg("--app-dir")
            .arg(temp.path())
            .args([
                "render",
                "missing.dng",
                "--out",
                "unused.png",
                "--process",
                mode,
            ])
            .assert()
            .code(1)
            .get_output()
            .clone();
        assert!(!String::from_utf8_lossy(&output.stderr).contains("unexpected argument"));
    }
    Command::new(assert_cmd::cargo::cargo_bin!("tessera"))
        .args([
            "render",
            "missing.dng",
            "--out",
            "unused.png",
            "--process",
            "bogus",
        ])
        .assert()
        .code(2);
}

#[test]
fn fidelity_standalone_uses_catalog_ids_and_reports_missing_inputs() {
    let temp = tempfile::tempdir().unwrap();
    let catalog = temp.path().join("test.lrcat");
    let conn = make_fixture::make_fixture(&catalog);
    conn.execute(
        "UPDATE AgLibraryRootFolder SET absolutePath=?1",
        [format!("{}/missing/", temp.path().display())],
    )
    .unwrap();
    drop(conn);
    let before = std::fs::read(&catalog).unwrap();
    let references = temp.path().join("references");
    std::fs::create_dir(&references).unwrap();
    image::RgbImage::new(8, 8)
        .save(references.join("30.jpg"))
        .unwrap();
    let output = Command::new(assert_cmd::cargo::cargo_bin!("tessera"))
        .arg("--app-dir")
        .arg(temp.path().join("app"))
        .args(["import", "lrcat"])
        .arg(&catalog)
        .arg("--fidelity")
        .arg("--reference-dir")
        .arg(&references)
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: Value = serde_json::from_slice(&output).unwrap();
    let rows = report["fidelity"]["images"].as_array().unwrap();
    assert_eq!(rows.len(), 3);
    for row in rows {
        let id = row["catalog_id"].as_i64().unwrap();
        assert_eq!(
            row["reference"],
            references.join(format!("{id}.jpg")).to_str().unwrap()
        );
        assert_eq!(row["status"], "skipped");
        assert!(row.get("mean").is_none());
        assert!(row["reason"].as_str().unwrap().contains(if id == 30 {
            "original"
        } else {
            "reference"
        }));
    }
    assert_eq!(report["fidelity"]["compared"], 0);
    assert_eq!(report["fidelity"]["skipped"], 3);
    assert!(!temp.path().join("app").exists());
    assert_eq!(std::fs::read(catalog).unwrap(), before);
}
