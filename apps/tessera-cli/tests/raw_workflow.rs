use assert_cmd::Command;
use serde_json::Value;
use std::path::Path;
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
fn raw_workflow() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw");
    let names = [
        "canon-cr3.CR3",
        "nikon-nef.NEF",
        "sony-arw.ARW",
        "fuji-raf.RAF",
        "sample.dng",
    ];
    if names.iter().any(|name| !fixtures.join(name).is_file()) {
        eprintln!("SKIP: raw fixture set absent");
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let photos = temp.path().join("photos");
    std::fs::create_dir(&photos).unwrap();
    for name in names {
        std::fs::copy(fixtures.join(name), photos.join(name)).unwrap();
    }
    let app = temp.path().join("app");
    assert_eq!(
        json(&app, &["index", photos.to_str().unwrap()])["changed"],
        5
    );
    assert_eq!(json(&app, &["ls"]).as_array().unwrap().len(), 5);
    let catalog = index::Index::open(app.join("index.sqlite")).unwrap();
    let ids = catalog.search(&index::Query::default()).unwrap();
    let canon_id = ids
        .into_iter()
        .find(|id| {
            catalog
                .image_info(*id)
                .unwrap()
                .path
                .ends_with("canon-cr3.CR3")
        })
        .unwrap();
    assert!(
        catalog
            .image_info(canon_id)
            .unwrap()
            .capture_seconds
            .is_some()
    );
    let canon = photos.join("canon-cr3.CR3");
    let path = canon.to_str().unwrap();
    cli(&app)
        .args(["cull", "set", path, "--decision", "P", "--grade", "2"])
        .assert()
        .success();
    assert_eq!(
        json(&app, &["ls", "--decision", "keep"])
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        json(&app, &["ls", "--query", "canon"])
            .as_array()
            .unwrap()
            .len(),
        1
    );
    cli(&app)
        .args([
            "develop",
            "set",
            path,
            "--exposure",
            "0.5",
            "--contrast",
            "10",
        ])
        .assert()
        .success();
    assert_eq!(
        json(&app, &["develop", "show", path])["tone"]["exposure"],
        0.5
    );
    let png = temp.path().join("render.png");
    cli(&app)
        .args([
            "render",
            path,
            "--out",
            png.to_str().unwrap(),
            "--scale",
            "1/8",
        ])
        .assert()
        .success();
    let meta = raw_decode::RawSource::open(&canon).unwrap().metadata();
    let mut dimensions = (
        meta.default_crop[2].div_ceil(8),
        meta.default_crop[3].div_ceil(8),
    );
    if (5..=8).contains(&meta.orientation) {
        dimensions = (dimensions.1, dimensions.0);
    }
    assert_eq!(image::image_dimensions(&png).unwrap(), dimensions);
    assert_eq!(&std::fs::read(png).unwrap()[..8], b"\x89PNG\r\n\x1a\n");
    let jpg = temp.path().join("preview.jpg");
    cli(&app)
        .args([
            "preview",
            path,
            "--out",
            jpg.to_str().unwrap(),
            "--max",
            "1024",
        ])
        .assert()
        .success();
    let (w, h) = image::image_dimensions(&jpg).unwrap();
    assert_eq!(w.max(h), 1024);
    assert_eq!(&std::fs::read(jpg).unwrap()[..2], &[255, 216]);
}
