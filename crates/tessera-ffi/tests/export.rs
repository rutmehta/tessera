//! Export over the bridge (WP M2-20): settings JSON, presets, streaming batch
//! export with progress/cancel/conflicts, print renders and the soft-proof
//! LUT. Scratch folders only; the RAW case skips without fixtures.
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tessera_ffi::*;

#[test]
fn original_export_keeps_embedded_only_dng_edits() {
    let dir = tempfile::tempdir().unwrap();
    let photos = dir.path().join("photos");
    std::fs::create_dir(&photos).unwrap();
    let source = photos.join("edited.dng");
    let image = merge::LinearImage {
        width: 32,
        height: 32,
        pixels: vec![[0.2; 3]; 32 * 32],
        color_matrix: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        as_shot_neutral: [1.0; 3],
    };
    let packet = r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" xmlns:xmp="http://ns.adobe.com/xap/1.0/" crs:Exposure2012="1.25" xmp:Rating="4"/></rdf:RDF>"#;
    merge::dng::write(&mut std::fs::File::create(&source).unwrap(), &image, packet).unwrap();
    let engine = Engine::open(dir.path().join("support").to_string_lossy().into_owned()).unwrap();
    engine
        .index_folder(photos.to_string_lossy().into_owned())
        .unwrap();
    let report = engine
        .export_batch(
            ExportTarget::Query {
                query: ImageQuery::default(),
            },
            settings(
                &dir.path().join("out"),
                serde_json::json!({"format":"original","resize":{"mode":"none"}}),
            ),
            None,
            None,
        )
        .unwrap();
    assert_eq!((report.exported, report.failed), (1, 0), "{report:?}");
    let output = raw_decode::linear_dng::read(
        &mut std::fs::File::open(report.items[0].output_path.as_ref().unwrap()).unwrap(),
    )
    .unwrap();
    let imported = sidecar::XmpPacket::parse(output.xmp)
        .unwrap()
        .to_recipe()
        .unwrap()
        .recipe;
    assert_eq!(imported.settings.tone.exposure, 1.25);
    assert_eq!(
        imported.selection,
        sidecar::XmpPacket::parse(packet)
            .unwrap()
            .selection()
            .unwrap()
    );
}

#[test]
fn original_export_copies_bytes_and_is_remembered() {
    let f = fixture();
    let out = f.dir.path().join("originals");
    let options = settings(
        &out,
        serde_json::json!({"format":"original", "resize":{"mode":"none"}}),
    );
    let report = f
        .engine
        .export_batch(
            ExportTarget::Images {
                image_ids: vec![f.ids[0].clone()],
            },
            options,
            None,
            None,
        )
        .unwrap();
    assert_eq!((report.exported, report.failed), (1, 0), "{report:?}");
    assert_eq!(
        std::fs::read(out.join("a.jpg")).unwrap(),
        std::fs::read(f.dir.path().join("photos/a.jpg")).unwrap()
    );
    assert!(sidecar::Sidecar::paths(out.join("a.jpg")).xmp.exists());
    let again = f
        .engine
        .export_with_previous(
            ExportTarget::Images {
                image_ids: vec![f.ids[1].clone()],
            },
            None,
            None,
        )
        .unwrap();
    assert_eq!((again[0].exported, again[0].failed), (1, 0));
    assert_eq!(
        std::fs::read(out.join("b.jpg")).unwrap(),
        std::fs::read(f.dir.path().join("photos/b.jpg")).unwrap()
    );
    for extra in [
        serde_json::json!({"remove_location":true}),
        serde_json::json!({"metadata":"none"}),
        serde_json::json!({"resize":{"mode":"long_edge"}}),
        serde_json::json!({"sharpening":"screen"}),
    ] {
        let mut value = extra;
        value["format"] = "original".into();
        assert!(normalize_export_settings(value.to_string()).is_err());
    }
}

#[test]
fn sharpening_amount_json_defaults_and_roundtrip() {
    let defaults: serde_json::Value =
        serde_json::from_str(&normalize_export_settings("{}".into()).unwrap()).unwrap();
    assert_eq!(defaults["sharpening_amount"], "standard");
    for amount in ["low", "standard", "high"] {
        let json =
            serde_json::json!({"sharpening": "matte", "sharpening_amount": amount, "dpi": 240});
        let normalized: serde_json::Value =
            serde_json::from_str(&normalize_export_settings(json.to_string()).unwrap()).unwrap();
        assert_eq!(normalized["sharpening_amount"], amount);
        assert_eq!(normalized["dpi"], 240);
    }
    assert!(normalize_export_settings(r#"{"sharpening_amount":"extreme"}"#.into()).is_err());
}

#[test]
fn workflow_previous_survives_reopen_and_uses_the_new_selection() {
    let f = fixture();
    let target = |id: &String| ExportTarget::Images {
        image_ids: vec![id.clone()],
    };
    assert!(
        f.engine
            .export_with_previous(target(&f.ids[0]), None, None)
            .is_err()
    );
    let out = f.dir.path().join("previous");
    let options = settings(&out, serde_json::json!({"format":"png"}));
    let first = f
        .engine
        .export_batch(target(&f.ids[0]), options, None, None)
        .unwrap();
    assert_eq!((first.exported, first.failed), (1, 0));
    let reopened = Engine::open(f.support.clone()).unwrap();
    let reports = reopened
        .export_with_previous(target(&f.ids[1]), None, None)
        .unwrap();
    assert_eq!(reports.len(), 1);
    assert_eq!((reports[0].exported, reports[0].failed), (1, 0));
    assert_eq!(image::open(out.join("b.png")).unwrap().width(), 16);
    assert!(!out.join("a-2.png").exists());
}

#[test]
fn workflow_multiple_settings_are_preflighted_and_remembered_together() {
    let f = fixture();
    let target = || ExportTarget::Images {
        image_ids: vec![f.ids[0].clone()],
    };
    let out = f.dir.path().join("multiple");
    let jpeg = settings(&out, serde_json::json!({}));
    let png = settings(&out, serde_json::json!({"format":"png"}));
    assert!(
        f.engine
            .export_multiple(
                target(),
                vec![jpeg.clone(), "{\"quality\":0}".into()],
                None,
                None
            )
            .is_err()
    );
    assert!(
        !out.exists(),
        "later invalid settings must prevent earlier exports"
    );
    let reports = f
        .engine
        .export_multiple(target(), vec![jpeg, png], None, None)
        .unwrap();
    assert_eq!(reports.len(), 2);
    assert!(reports.iter().all(|r| r.exported == 1 && r.failed == 0));
    assert!(out.join("a.jpg").is_file());
    assert!(out.join("a.png").is_file());
    let saved = std::fs::read(Path::new(&f.support).join("LastExport.json")).unwrap();
    let saved_json: serde_json::Value = serde_json::from_slice(&saved).unwrap();
    assert_eq!(saved_json["settings"].as_array().unwrap().len(), 2);
    let cancel = CancelFlag::new();
    cancel.cancel();
    let cancelled = f
        .engine
        .export_with_previous(target(), None, Some(cancel))
        .unwrap();
    assert!(cancelled[0].cancelled);
    assert_eq!(
        std::fs::read(Path::new(&f.support).join("LastExport.json")).unwrap(),
        saved
    );
    let reopened = Engine::open(f.support.clone()).unwrap();
    let again = reopened
        .export_with_previous(
            ExportTarget::Images {
                image_ids: vec![f.ids[1].clone()],
            },
            None,
            None,
        )
        .unwrap();
    assert_eq!(again.len(), 2);
    assert!(out.join("b.jpg").is_file());
    assert!(out.join("b.png").is_file());
}

#[test]
#[cfg(unix)]
fn workflow_script_runs_in_host_only_after_successful_export() {
    use std::os::unix::fs::PermissionsExt;
    let f = fixture();
    let out = f.dir.path().join("with spaces;not-shell");
    let script = f.dir.path().join("post export.sh");
    std::fs::write(&script, "#!/bin/sh\nfor file do\n  test -f \"$file\" || exit 42\n  printf '%s\\n' \"$file\" > \"$file.receipt\"\ndone\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let options = settings(
        &out,
        serde_json::json!({"after_export":{"run_script":script}}),
    );
    let report = f
        .engine
        .export_batch(
            ExportTarget::Images {
                image_ids: vec![f.ids[0].clone()],
            },
            options.clone(),
            None,
            None,
        )
        .unwrap();
    assert_eq!((report.exported, report.failed), (1, 0));
    assert!(report.workflow_errors.is_empty(), "{report:?}");
    assert_eq!(
        std::fs::read_to_string(out.join("a.jpg.receipt")).unwrap(),
        format!("{}\n", out.join("a.jpg").display())
    );
    let previous = std::fs::read(Path::new(&f.support).join("LastExport.json")).unwrap();
    // A host failure must retain the output and previous settings, not report
    // the already-published image as a failed render or discard its path.
    std::fs::write(&script, "#!/bin/sh\nexit 23\n").unwrap();
    let report = f
        .engine
        .export_batch(
            ExportTarget::Images {
                image_ids: vec![f.ids[1].clone()],
            },
            options,
            None,
            None,
        )
        .unwrap();
    assert_eq!((report.exported, report.failed), (1, 0));
    assert_eq!(report.workflow_errors.len(), 1);
    assert!(report.workflow_errors[0].contains("23"));
    assert!(out.join("b.jpg").exists());
    assert_eq!(
        std::fs::read(Path::new(&f.support).join("LastExport.json")).unwrap(),
        previous
    );
    assert!(
        normalize_export_settings(r#"{"after_export":{"run_script":"relative.sh"}}"#.into())
            .is_err()
    );
}

#[test]
fn workflow_failures_preserve_previous_and_earlier_preset_reports() {
    let f = fixture();
    let target = || ExportTarget::Images {
        image_ids: vec![f.ids[0].clone()],
    };
    let out = f.dir.path().join("good");
    let options = settings(&out, serde_json::json!({}));
    f.engine
        .export_batch(target(), options.clone(), None, None)
        .unwrap();
    let last = Path::new(&f.support).join("LastExport.json");
    let saved = std::fs::read(&last).unwrap();
    let blocked = f.dir.path().join("not-a-directory");
    std::fs::write(&blocked, "not a directory").unwrap();
    let reports = f
        .engine
        .export_multiple(
            target(),
            vec![options, settings(&blocked, serde_json::json!({}))],
            None,
            None,
        )
        .unwrap();
    assert_eq!(reports.len(), 2);
    assert_eq!((reports[0].exported, reports[1].failed), (1, 1));
    assert!(Path::new(reports[0].items[0].output_path.as_ref().unwrap()).is_file());
    assert!(reports[1].items[0].error.is_some());
    assert_eq!(std::fs::read(&last).unwrap(), saved);
    let invalid = settings(&out, serde_json::json!({"on_conflict":"skip"}));
    assert_eq!(
        f.engine
            .export_batch(target(), invalid, None, None)
            .unwrap()
            .failed,
        1
    );
    assert_eq!(std::fs::read(&last).unwrap(), saved);
    for corrupt in [
        "not json",
        r#"{"version":2,"settings":[]}"#,
        r#"{"version":1,"settings":[]}"#,
    ] {
        std::fs::write(&last, corrupt).unwrap();
        assert!(f.engine.export_with_previous(target(), None, None).is_err());
        assert_eq!(std::fs::read_to_string(&last).unwrap(), corrupt);
    }
}

#[test]
#[cfg(unix)]
fn workflow_does_not_run_scripts_after_cancel_or_partial_failure() {
    use std::os::unix::fs::PermissionsExt;
    let f = fixture();
    let script = f.dir.path().join("mark.sh");
    std::fs::write(
        &script,
        "#!/bin/sh\nfor file do touch \"$file.receipt\"; done\n",
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let out = f.dir.path().join("partial");
    std::fs::create_dir(&out).unwrap();
    std::fs::write(out.join("a.jpg"), "existing").unwrap();
    let options = settings(
        &out,
        serde_json::json!({"on_conflict":"skip","after_export":{"run_script":script}}),
    );
    let report = f
        .engine
        .export_batch(
            ExportTarget::Images {
                image_ids: f.ids.clone(),
            },
            options.clone(),
            None,
            None,
        )
        .unwrap();
    assert_eq!((report.exported, report.failed), (2, 1));
    assert!(!out.join("b.jpg.receipt").exists());
    assert!(!Path::new(&f.support).join("LastExport.json").exists());
    let cancel = CancelFlag::new();
    cancel.cancel();
    let report = f
        .engine
        .export_batch(
            ExportTarget::Images {
                image_ids: f.ids.clone(),
            },
            options,
            None,
            Some(cancel),
        )
        .unwrap();
    assert!(report.cancelled);
    assert!(!out.join("b.jpg.receipt").exists());
}

struct Fixture {
    dir: tempfile::TempDir,
    support: String,
    folder: String,
    engine: Arc<Engine>,
    /// a.jpg (48×32), b.jpg (32×48), c.jpg (40×40), in path order.
    ids: Vec<String>,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let photos = dir.path().join("photos");
    std::fs::create_dir(&photos).unwrap();
    for (name, w, h) in [("a", 48, 32), ("b", 32, 48), ("c", 40, 40)] {
        image::RgbImage::from_fn(w, h, |x, y| image::Rgb([(x * 5) as u8, (y * 5) as u8, 90]))
            .save(photos.join(format!("{name}.jpg")))
            .unwrap();
    }
    let support = dir.path().join("support").to_string_lossy().into_owned();
    let engine = Engine::open(support.clone()).unwrap();
    let folder = engine
        .index_folder(photos.to_string_lossy().into_owned())
        .unwrap()
        .path;
    let mut rows = engine.list_images(ImageQuery::default()).unwrap();
    rows.sort_by(|a, b| a.path.cmp(&b.path));
    Fixture {
        support,
        folder,
        engine,
        ids: rows.into_iter().map(|r| r.id).collect(),
        dir,
    }
}

fn settings(destination: &Path, extra: serde_json::Value) -> String {
    let mut json = serde_json::json!({
        "format": "jpeg",
        "quality": 85,
        "resize": {"mode": "long_edge", "long_edge": 24},
        "naming": "{name}",
        "dpi": 240,
        "destination": destination.to_string_lossy(),
    });
    for (k, v) in extra.as_object().unwrap() {
        json[k] = v.clone();
    }
    json.to_string()
}

#[derive(Default)]
struct Recorder(Mutex<Vec<ExportProgress>>);
impl ExportProgressListener for Recorder {
    fn on_progress(&self, progress: ExportProgress) {
        self.0.lock().unwrap().push(progress);
    }
}

fn files(dir: &Path) -> Vec<String> {
    let mut names: Vec<_> = std::fs::read_dir(dir)
        .map(|d| {
            d.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[test]
fn dng_settings_select_float_linear_export() {
    let json = normalize_export_settings(r#"{"format":"dng","bit_depth":32}"#.into()).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["format"], "dng");
    assert_eq!(value["bit_depth"], 32);
    assert!(normalize_export_settings(r#"{"format":"dng","bit_depth":16}"#.into()).is_err());
}

#[test]
fn jpeg_xl_settings_support_lossless_depths_and_reject_false_profiles() {
    for bits in [8, 16] {
        let json = format!(r#"{{"format":"jpeg_xl","bit_depth":{bits}}}"#);
        let value: serde_json::Value =
            serde_json::from_str(&normalize_export_settings(json).unwrap()).unwrap();
        assert_eq!(value["format"], "jpeg_xl");
        assert_eq!(value["bit_depth"], bits);
    }
    for extra in [r#""bit_depth":12"#, r#""color_space":"display_p3""#] {
        assert!(normalize_export_settings(format!(r#"{{"format":"jpeg_xl",{extra}}}"#)).is_err());
    }
}

#[test]
fn avif_settings_are_backward_compatible_and_validate_depth_and_speed() {
    for bits in [8, 10, 12] {
        let json = format!(r#"{{"format":"avif","bit_depth":{bits},"avif_speed":8}}"#);
        let value: serde_json::Value =
            serde_json::from_str(&normalize_export_settings(json).unwrap()).unwrap();
        assert_eq!(value["bit_depth"], bits);
        assert_eq!(value["avif_speed"], 8);
    }
    for json in [
        r#"{"format":"avif","bit_depth":16}"#,
        r#"{"format":"avif","avif_speed":0}"#,
        r#"{"format":"avif","avif_speed":11}"#,
        r#"{"format":"avif","max_file_bytes":1000}"#,
    ] {
        assert!(normalize_export_settings(json.into()).is_err(), "{json}");
    }
    assert!(normalize_export_settings("{}".into()).is_ok());
}

#[test]
fn jpeg_xl_batch_encodes_each_depth_and_keeps_metadata_sidecar() {
    let f = fixture();
    for bits in [8, 16] {
        let out = f.dir.path().join(format!("jxl-{bits}"));
        let report = f
            .engine
            .export_batch(
                ExportTarget::Images {
                    image_ids: vec![f.ids[0].clone()],
                },
                settings(
                    &out,
                    serde_json::json!({"format":"jpeg_xl", "bit_depth":bits}),
                ),
                None,
                None,
            )
            .unwrap();
        assert_eq!((report.exported, report.failed), (1, 0), "{report:?}");
        let data = std::fs::read(out.join("a.jxl")).unwrap();
        assert_eq!(&data[4..12], b"JXL \r\n\x87\n");
        assert!(out.join("a.jxl.xmp").is_file());
    }
}

#[test]
fn avif_batch_encodes_each_depth_and_keeps_metadata_sidecar() {
    let f = fixture();
    for bits in [8, 10, 12] {
        let out = f.dir.path().join(format!("avif-{bits}"));
        let report = f
            .engine
            .export_batch(
                ExportTarget::Images {
                    image_ids: vec![f.ids[0].clone()],
                },
                settings(
                    &out,
                    serde_json::json!({"format":"avif", "bit_depth":bits, "avif_speed":10}),
                ),
                None,
                None,
            )
            .unwrap();
        assert_eq!((report.exported, report.failed), (1, 0), "{report:?}");
        let data = std::fs::read(out.join("a.avif")).unwrap();
        assert_eq!(&data[4..12], b"ftypavif");
        assert!(out.join("a.avif.xmp").is_file());
    }
}

#[test]
fn settings_json_is_validated_and_normalized() {
    let normalized =
        normalize_export_settings(r#"{"format":"tiff","bit_depth":16}"#.into()).unwrap();
    let value: serde_json::Value = serde_json::from_str(&normalized).unwrap();
    assert_eq!(value["quality"], 90);
    assert_eq!(value["color_space"], "srgb");
    assert_eq!(value["resize"]["mode"], "none");
    assert_eq!(value["on_conflict"], "unique");
    assert!(value["max_file_bytes"].is_null());
    assert!(value["watermark"].is_null());
    let options = ExportOptions::from_json(r#"{"max_file_bytes":4096,"watermark":{"kind":"text","text":"Copyright","font":"font.ttf","size":0.05,"color":[1,1,1],"opacity":0.5,"anchor":"bottom_right","inset":0.02,"rotation":0}}"#).unwrap();
    assert_eq!(options.max_file_bytes, Some(4096));
    assert_eq!(
        ExportOptions::from_json(&options.to_json()).unwrap(),
        options
    );
    for bad in [
        r#"{"quality":0}"#,
        r#"{"format":"jpeg","bit_depth":16}"#,
        r#"{"upscale":3}"#,
        r#"{"naming":"../{name}"}"#,
        r#"{"naming":"{unknown}"}"#,
        r#"{"resize":{"mode":"long_edge","long_edge":0}}"#,
        r#"{"surprise":true}"#,
        r#"{"max_file_bytes":0}"#,
        r#"{"format":"png","max_file_bytes":4096}"#,
        r#"{"watermark":{"kind":"graphic","path":"a.png","scale":0,"opacity":1,"anchor":"center","inset":0}}"#,
    ] {
        assert!(normalize_export_settings(bad.into()).is_err(), "{bad}");
    }
    assert_eq!(
        export_filename(
            "{date}_{name}-{seq}".into(),
            "IMG_1".into(),
            3,
            "2026-09-25".into(),
            "jpg".into()
        )
        .unwrap(),
        "2026-09-25_IMG_1-3.jpg"
    );
    assert!(export_filename("a/b".into(), "x".into(), 1, "".into(), "jpg".into()).is_err());
}

#[test]
fn presets_ship_defaults_and_persist_crud() {
    let f = fixture();
    let names = |e: &Engine| {
        e.export_presets()
            .unwrap()
            .into_iter()
            .map(|p| p.name)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        names(&f.engine),
        [
            "Web 2048 sRGB",
            "Full-size JPEG",
            "16-bit TIFF ProPhoto",
            "Print 300 dpi"
        ]
    );
    let web = &f.engine.export_presets().unwrap()[0];
    let web: serde_json::Value = serde_json::from_str(&web.settings_json).unwrap();
    assert_eq!(
        (web["quality"].as_u64(), web["color_space"].as_str()),
        (Some(85), Some("srgb"))
    );
    assert_eq!(web["resize"]["long_edge"], 2048.0);
    let print: serde_json::Value =
        serde_json::from_str(&f.engine.export_presets().unwrap()[3].settings_json).unwrap();
    assert_eq!(
        (print["dpi"].as_u64(), print["resize"]["unit"].as_str()),
        (Some(300), Some("in"))
    );

    let mine = settings(Path::new("/tmp/out"), serde_json::json!({"quality": 70}));
    f.engine
        .save_export_preset("Client / proofs".into(), mine.clone())
        .unwrap();
    assert!(
        f.engine
            .save_export_preset("  ".into(), mine.clone())
            .is_err()
    );
    assert!(
        f.engine
            .save_export_preset("Bad".into(), "{\"quality\":0}".into())
            .is_err()
    );
    // Saving again under the same name replaces it.
    f.engine
        .save_export_preset(
            "Client / proofs".into(),
            settings(Path::new("/tmp/out"), serde_json::json!({"quality": 60})),
        )
        .unwrap();
    f.engine
        .delete_export_preset("Full-size JPEG".into())
        .unwrap();
    f.engine
        .rename_export_preset("Print 300 dpi".into(), "Lab prints".into())
        .unwrap();
    assert!(
        f.engine
            .rename_export_preset("Lab prints".into(), "Client / proofs".into())
            .is_err()
    );
    assert!(f.engine.delete_export_preset("Nope".into()).is_err());
    drop(f.engine);

    // Files under <app-dir>/ExportPresets survive a relaunch; deleted defaults stay deleted.
    let engine = Engine::open(f.support.clone()).unwrap();
    assert_eq!(
        names(&engine),
        [
            "Web 2048 sRGB",
            "16-bit TIFF ProPhoto",
            "Client / proofs",
            "Lab prints"
        ]
    );
    let client = engine
        .export_presets()
        .unwrap()
        .into_iter()
        .find(|p| p.name == "Client / proofs")
        .unwrap();
    assert!(client.settings_json.contains("\"quality\": 60"));
    assert!(Path::new(&f.support).join("ExportPresets").is_dir());
    engine.restore_default_export_presets().unwrap();
    assert_eq!(names(&engine).len(), 6);
    let _ = &f.dir;
}

#[test]
fn sharpening_amount_reaches_exported_pixels_and_preset() {
    let f = fixture();
    let mut outputs = Vec::new();
    for amount in ["low", "standard", "high"] {
        let out = f.dir.path().join(amount);
        let json = settings(
            &out,
            serde_json::json!({
                "format": "tiff", "bit_depth": 16, "metadata": "none",
                "sharpening": "matte", "sharpening_amount": amount, "dpi": 300
            }),
        );
        f.engine
            .save_export_preset(amount.into(), json.clone())
            .unwrap();
        let preset = f
            .engine
            .export_presets()
            .unwrap()
            .into_iter()
            .find(|p| p.name == amount)
            .unwrap();
        let saved: serde_json::Value = serde_json::from_str(&preset.settings_json).unwrap();
        assert_eq!(saved["sharpening_amount"], amount);
        let report = f
            .engine
            .export_batch(
                ExportTarget::Images {
                    image_ids: vec![f.ids[0].clone()],
                },
                json,
                None,
                None,
            )
            .unwrap();
        assert_eq!((report.exported, report.failed), (1, 0));
        outputs.push(
            image::open(report.items[0].output_path.as_ref().unwrap())
                .unwrap()
                .to_rgb16(),
        );
    }
    assert!(outputs.windows(2).all(|p| p[0] != p[1]));
}

#[test]
fn batch_exports_resized_oriented_files_with_progress_and_status() {
    let f = fixture();
    let out = f.dir.path().join("out");
    let recorder = Arc::new(Recorder::default());
    let report = f
        .engine
        .export_batch(
            ExportTarget::Images {
                image_ids: f.ids.clone(),
            },
            settings(&out, serde_json::json!({"metadata": "none"})),
            Some(recorder.clone()),
            None,
        )
        .unwrap();
    assert_eq!(
        (report.exported, report.failed, report.cancelled),
        (3, 0, false)
    );
    assert_eq!(files(&out), ["a.jpg", "b.jpg", "c.jpg"]);
    for (name, dims) in [
        ("a.jpg", (24, 16)),
        ("b.jpg", (16, 24)),
        ("c.jpg", (24, 24)),
    ] {
        assert_eq!(
            image::image_dimensions(out.join(name)).unwrap(),
            dims,
            "{name}"
        );
    }
    let jpeg = std::fs::read(out.join("a.jpg")).unwrap();
    let at = jpeg.windows(5).position(|w| w == b"JFIF\0").unwrap();
    assert_eq!(
        &jpeg[at + 7..at + 12],
        &[1, 0, 240, 0, 240],
        "240 dpi recorded"
    );
    let progress = recorder.0.lock().unwrap().clone();
    assert_eq!(progress.first().unwrap().current, "a.jpg");
    assert_eq!(progress.last().unwrap().done, 3);
    assert_eq!(progress.last().unwrap().exported, 3);
    assert!(progress.last().unwrap().current.is_empty());
    // The export log feeds the derived "Exported" status.
    let session = f.engine.open_cull_session(f.folder.clone()).unwrap();
    let statuses = session.derived_statuses(f.ids.clone()).unwrap();
    assert!(
        statuses.iter().all(|s| s.phase == StatusPhase::Exported),
        "{statuses:?}"
    );

    // Existing names: unique (default) never overwrites, skip reports each image.
    let again = f
        .engine
        .export_batch(
            ExportTarget::Images {
                image_ids: f.ids[..1].to_vec(),
            },
            settings(&out, serde_json::json!({"metadata": "none"})),
            None,
            None,
        )
        .unwrap();
    assert!(
        again.items[0]
            .output_path
            .as_ref()
            .unwrap()
            .ends_with("a-2.jpg")
    );
    let skipped = f
        .engine
        .export_batch(
            ExportTarget::Images {
                image_ids: f.ids.clone(),
            },
            settings(
                &out,
                serde_json::json!({"metadata": "none", "on_conflict": "skip"}),
            ),
            None,
            None,
        )
        .unwrap();
    assert_eq!((skipped.exported, skipped.failed), (0, 3));
    assert!(
        skipped.items[1]
            .error
            .as_ref()
            .unwrap()
            .contains("b.jpg already exists")
    );
    // A template that maps every photo to one name fails before writing.
    let clash = f.engine.export_batch(
        ExportTarget::Images {
            image_ids: f.ids.clone(),
        },
        settings(
            &f.dir.path().join("clash"),
            serde_json::json!({"naming": "same", "on_conflict": "skip"}),
        ),
        None,
        None,
    );
    assert!(clash.unwrap_err().to_string().contains("{seq}"));
    assert!(files(&f.dir.path().join("clash")).is_empty());
}

#[test]
fn batch_targets_cancel_and_destination_rules() {
    let f = fixture();
    // Query target, sequence numbers and dates in names, TIFF 16-bit.
    let out = f.dir.path().join("query");
    let report = f
        .engine
        .export_batch(
            ExportTarget::Query {
                query: ImageQuery {
                    folder: Some(f.folder.clone()),
                    ..Default::default()
                },
            },
            settings(
                &out,
                serde_json::json!({"format": "tiff", "bit_depth": 16, "color_space": "prophoto",
                                   "naming": "{seq}-{name}", "resize": {"mode": "percent", "percent": 50}}),
            ),
            None,
            None,
        )
        .unwrap();
    assert_eq!(report.exported, 3);
    assert_eq!(
        files(&out).iter().filter(|n| n.ends_with(".tif")).count(),
        3
    );
    assert!(files(&out).iter().any(|n| n.starts_with("1-")));

    // Album target, in album order.
    let store = f
        .engine
        .clone()
        .open_library(format!("{}/library.json", f.folder))
        .unwrap();
    let album = store.create_album("Picks".into(), None).unwrap();
    store
        .add_to_album(album, vec![f.ids[2].clone(), f.ids[0].clone()])
        .unwrap();
    let out = f.dir.path().join("album");
    let report = f
        .engine
        .export_batch(
            ExportTarget::Album {
                library_path: store.path(),
                album_id: album,
            },
            settings(
                &out,
                serde_json::json!({"naming": "{seq}", "metadata": "none"}),
            ),
            None,
            None,
        )
        .unwrap();
    assert_eq!(
        report
            .items
            .iter()
            .map(|i| i.name.as_str())
            .collect::<Vec<_>>(),
        ["c.jpg", "a.jpg"]
    );
    assert_eq!(files(&out), ["1.jpg", "2.jpg"]);
    assert_eq!(
        image::image_dimensions(out.join("1.jpg")).unwrap(),
        (24, 24)
    );

    // Cancelled before the first image: nothing written, report says so.
    let cancel = CancelFlag::new();
    cancel.cancel();
    let out = f.dir.path().join("cancelled");
    let report = f
        .engine
        .export_batch(
            ExportTarget::Images {
                image_ids: f.ids.clone(),
            },
            settings(&out, serde_json::json!({})),
            None,
            Some(cancel),
        )
        .unwrap();
    assert!(report.cancelled);
    assert_eq!(report.exported, 0);
    assert!(files(&out).is_empty());

    // Destination must be absolute; an empty target is refused.
    let relative = settings(Path::new("relative/out"), serde_json::json!({}));
    assert!(
        f.engine
            .export_batch(
                ExportTarget::Images {
                    image_ids: f.ids.clone()
                },
                relative,
                None,
                None
            )
            .is_err()
    );
    assert!(
        f.engine
            .export_batch(
                ExportTarget::Images { image_ids: vec![] },
                settings(&f.dir.path().join("x"), serde_json::json!({})),
                None,
                None
            )
            .is_err()
    );
}

/// A small-gamut RGB output-class profile with a warm, dim paper white (the
/// colour-mgmt proof tests use the same shape).
fn output_profile() -> Vec<u8> {
    use lcms2::*;
    let xy = |x, y| CIExyY { x, y, Y: 1.0 };
    let curve = ToneCurve::new(2.2);
    let mut p = lcms2::Profile::new_rgb(
        &xy(0.3457, 0.3585),
        &CIExyYTRIPLE {
            Red: xy(0.48, 0.34),
            Green: xy(0.30, 0.48),
            Blue: xy(0.23, 0.20),
        },
        &[&curve, &curve, &curve],
    )
    .unwrap();
    p.set_version(2.4);
    p.set_device_class(ProfileClassSignature::OutputClass);
    p.write_tag(
        TagSignature::MediaWhitePointTag,
        Tag::CIEXYZ(&CIEXYZ {
            X: 0.80,
            Y: 0.85,
            Z: 0.55,
        }),
    );
    let mut mlu = MLU::new(1);
    mlu.set_text("Tessera test printer", Locale::none());
    p.write_tag(TagSignature::ProfileDescriptionTag, Tag::MLU(&mlu));
    p.icc().unwrap()
}

#[test]
fn print_renders_fit_the_box_in_the_chosen_colour_handling() {
    let f = fixture();
    let a = &f.ids[0];
    let managed = f
        .engine
        .render_for_print(
            PrintRenderRequest {
                image_id: a.clone(),
                max_width: 30,
                max_height: 30,
                sharpening: PrintSharpening::Matte,
                profile: None,
            },
            None,
        )
        .unwrap();
    assert_eq!(
        (managed.width, managed.height, managed.channels),
        (30, 20, 3)
    );
    assert_eq!(managed.data.len(), 30 * 20 * 3);
    assert!(managed.icc.len() > 100);

    let profile_path = f.dir.path().join("printer.icc");
    let profile_bytes = output_profile();
    std::fs::write(&profile_path, &profile_bytes).unwrap();
    let described = describe_printer_profile(profile_path.to_string_lossy().into_owned()).unwrap();
    assert_eq!(described.color_space, "RGB");
    let app = f
        .engine
        .render_for_print(
            PrintRenderRequest {
                image_id: f.ids[1].clone(),
                max_width: 100,
                max_height: 60,
                sharpening: PrintSharpening::Glossy,
                profile: Some(PrintProfile {
                    path: described.path.clone(),
                    intent: RenderingIntent::Perceptual,
                    black_point_compensation: true,
                }),
            },
            None,
        )
        .unwrap();
    assert_eq!((app.width, app.height, app.channels), (40, 60, 3));
    // Creating another profile can cross a second boundary and change its
    // ICC creation timestamp. Require the exact bytes of the input fixture.
    assert_eq!(app.icc, profile_bytes);

    let cmyk = PathBuf::from("/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc");
    if cmyk.exists() {
        let out = f
            .engine
            .render_for_print(
                PrintRenderRequest {
                    image_id: f.ids[2].clone(),
                    max_width: 10,
                    max_height: 10,
                    sharpening: PrintSharpening::None,
                    profile: Some(PrintProfile {
                        path: cmyk.to_string_lossy().into_owned(),
                        intent: RenderingIntent::RelativeColorimetric,
                        black_point_compensation: true,
                    }),
                },
                None,
            )
            .unwrap();
        assert_eq!((out.channels, out.data.len()), (4, 400));
        assert!(printer_profiles().iter().any(|p| p.color_space == "CMYK"));
    }
    assert!(describe_printer_profile("/nonexistent.icc".into()).is_err());
}

fn export_resize(long_edge: u32, percent: u32) -> ::export::Resize {
    match (long_edge, percent) {
        (0, 0) => ::export::Resize::None,
        (0, p) => ::export::Resize::Percent(f64::from(p)),
        (n, _) => ::export::Resize::LongEdge(n),
    }
}

#[test]
fn print_scale_bins_only_while_the_box_stays_covered() {
    let full = [0.0, 0.0, 1.0, 1.0];
    assert_eq!(print_scale((6000, 4000), full, (6000, 4000)), 1);
    assert_eq!(print_scale((6000, 4000), full, (3000, 3000)), 2);
    assert_eq!(print_scale((6000, 4000), full, (1000, 1000)), 4);
    assert_eq!(print_scale((6000, 4000), full, (100, 100)), 8);
    // A half-width crop needs twice the pixels.
    assert_eq!(
        print_scale((6000, 4000), [0.25, 0.0, 0.75, 1.0], (600, 600)),
        4
    );
    assert_eq!(
        print_scale((6000, 4000), [0.25, 0.0, 0.75, 1.0], (1600, 1600)),
        2
    );
    assert_eq!(
        print_scale((6000, 4000), [0.25, 0.0, 0.75, 1.0], (3000, 4000)),
        1
    );
    // Exports: the Web preset on a 36 MP frame renders at half size, full size never bins.
    use export_scale as scale;
    let (d, r) = ((7360, 4912), full);
    assert_eq!(scale(d, r, export_resize(2048, 0)), 2);
    assert_eq!(scale(d, r, export_resize(0, 0)), 1);
    assert_eq!(scale(d, r, export_resize(0, 25)), 4);
}

#[test]
fn soft_proof_lut_flags_out_of_gamut_and_simulates_paper() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("printer.icc");
    std::fs::write(&path, output_profile()).unwrap();
    let options = SoftProofOptions {
        profile_path: path.to_string_lossy().into_owned(),
        intent: RenderingIntent::RelativeColorimetric,
        black_point_compensation: true,
        simulate_paper: false,
    };
    let lut = soft_proof_lut(&options).unwrap();
    let n = lut.size as usize;
    assert_eq!(lut.rgba.len(), n * n * n * 4);
    let node = |r: usize, g: usize, b: usize| {
        let i = ((b * n + g) * n + r) * 4;
        &lut.rgba[i..i + 4]
    };
    assert_eq!(node(n - 1, 0, 0)[3], u16::MAX, "pure red is out of gamut");
    assert_eq!(node(n / 2, n / 2, n / 2)[3], 0, "mid grey prints");
    assert!(
        node(n - 1, n - 1, n - 1)[..3].iter().all(|v| *v > 64_000),
        "relative: white stays white"
    );
    assert!(lut.out_of_gamut > 0.05 && lut.out_of_gamut < 0.95);
    assert_eq!(lut.profile_name, "Tessera test printer");
    // Cached: the same options return the same allocation.
    assert!(Arc::ptr_eq(&lut, &soft_proof_lut(&options).unwrap()));
    let paper = soft_proof_lut(&SoftProofOptions {
        simulate_paper: true,
        ..options.clone()
    })
    .unwrap();
    let white = node(n - 1, n - 1, n - 1).to_vec();
    let i = (n * n * n - 1) * 4;
    assert!(
        paper.rgba[i + 2] + 3000 < white[2],
        "paper white is dimmer and warmer"
    );
    assert!(
        soft_proof_lut(&SoftProofOptions {
            profile_path: "/nonexistent.icc".into(),
            ..options
        })
        .is_err()
    );
}

#[test]
fn raw_export_with_the_web_preset_and_a_binned_print_render() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw");
    let Some(raw) = std::fs::read_dir(&root).ok().and_then(|d| {
        d.flatten()
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("nef")))
    }) else {
        eprintln!("skipping: no NEF fixture in {}", root.display());
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let photos = dir.path().join("raw");
    std::fs::create_dir(&photos).unwrap();
    std::fs::copy(&raw, photos.join(raw.file_name().unwrap())).unwrap();
    let engine = Engine::open(dir.path().join("support").to_string_lossy().into_owned()).unwrap();
    engine
        .index_folder(photos.to_string_lossy().into_owned())
        .unwrap();
    let row = engine.list_images(ImageQuery::default()).unwrap().remove(0);
    let web = engine.export_presets().unwrap().remove(0);
    let mut json: serde_json::Value = serde_json::from_str(&web.settings_json).unwrap();
    let out = dir.path().join("web");
    json["destination"] = out.to_string_lossy().into();
    let t = std::time::Instant::now();
    let report = engine
        .export_batch(
            ExportTarget::Images {
                image_ids: vec![row.id.clone()],
            },
            json.to_string(),
            None,
            None,
        )
        .unwrap();
    assert_eq!(report.exported, 1, "{:?}", report.items);
    let path = report.items[0].output_path.clone().unwrap();
    let (w, h) = image::image_dimensions(&path).unwrap();
    assert_eq!(w.max(h), 2048);
    assert_eq!(h > w, row.orientation >= 5, "display orientation");
    eprintln!("RAW web export: {:.1} s", t.elapsed().as_secs_f64());
    let t = std::time::Instant::now();
    let print = engine
        .render_for_print(
            PrintRenderRequest {
                image_id: row.id,
                max_width: 300,
                max_height: 300,
                sharpening: PrintSharpening::Matte,
                profile: None,
            },
            None,
        )
        .unwrap();
    eprintln!(
        "RAW print render (binned): {:.1} s",
        t.elapsed().as_secs_f64()
    );
    assert_eq!(print.width.max(print.height), 300);
    assert_eq!(print.height > print.width, row.orientation >= 5);
}
