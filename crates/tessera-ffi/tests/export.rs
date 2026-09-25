//! Export over the bridge (WP M2-20): settings JSON, presets, streaming batch
//! export with progress/cancel/conflicts, print renders and the soft-proof
//! LUT. Scratch folders only; the RAW case skips without fixtures.
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tessera_ffi::*;

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
fn settings_json_is_validated_and_normalized() {
    let normalized =
        normalize_export_settings(r#"{"format":"tiff","bit_depth":16}"#.into()).unwrap();
    let value: serde_json::Value = serde_json::from_str(&normalized).unwrap();
    assert_eq!(value["quality"], 90);
    assert_eq!(value["color_space"], "srgb");
    assert_eq!(value["resize"]["mode"], "none");
    assert_eq!(value["on_conflict"], "unique");
    for bad in [
        r#"{"quality":0}"#,
        r#"{"format":"jpeg","bit_depth":16}"#,
        r#"{"upscale":3}"#,
        r#"{"naming":"../{name}"}"#,
        r#"{"naming":"{unknown}"}"#,
        r#"{"resize":{"mode":"long_edge","long_edge":0}}"#,
        r#"{"surprise":true}"#,
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
    std::fs::write(&profile_path, output_profile()).unwrap();
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
    assert_eq!(app.icc, output_profile());

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
