//! Opt-in, aggregate-only app import profile. See the B5-51 handoff for semantics.
//! Never format a bridge error, recipe, issue reason, example, or source key.
use super::*;
use engine_api::recipe::CrsKey;
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Component, time::Instant};

type SafeResult<T> = std::result::Result<T, ()>;
fn safe<T, E>(value: std::result::Result<T, E>) -> SafeResult<T> {
    value.map_err(|_| ())
}

// Darwin's rusage: two timeval pairs followed by fourteen long counters.
// Kept local to this macOS-only test module; no dependency or production API change.
fn peak_rss() -> SafeResult<u64> {
    unsafe extern "C" {
        fn getrusage(who: i32, usage: *mut i64) -> i32;
    }
    let mut usage = [0i64; 18];
    // SAFETY: Darwin RUSAGE_SELF is zero, and the buffer matches struct rusage.
    if unsafe { getrusage(0, usage.as_mut_ptr()) } != 0 {
        return Err(());
    }
    Ok(usage[4] as u64)
}

fn bytes(dir: &Path) -> SafeResult<u64> {
    let mut total = 0;
    for entry in safe(std::fs::read_dir(dir))? {
        let entry = safe(entry)?;
        let meta = safe(entry.metadata())?;
        if safe(entry.file_type())?.is_symlink() {
            return Err(());
        }
        total += if meta.is_dir() {
            bytes(&entry.path())?
        } else {
            meta.len()
        };
    }
    Ok(total)
}

fn measure<T>(app: &Path, operation: impl FnOnce() -> SafeResult<T>) -> SafeResult<(T, Value)> {
    let before = bytes(app)?;
    let start = Instant::now();
    let result = operation()?;
    let seconds = start.elapsed().as_secs_f64();
    let peak = peak_rss()?;
    let after = bytes(app)?;
    Ok((
        result,
        json!({"wall_seconds": seconds, "peak_rss_bytes": peak,
        "app_bytes": after, "app_bytes_added": after.saturating_sub(before)}),
    ))
}

// Public Adobe property spellings absent from the legacy KEY_MAP. Values of
// these properties (including all names/digests/filenames) are never emitted.
const AUDIT_KEYS: &[&str] = &[
    "CropConstrainAspectRatio",
    "CustomIncrementalTemperature",
    "CustomIncrementalTint",
    "CustomLensProfileDigest",
    "CustomLensProfileDistortionScale",
    "CustomLensProfileFilename",
    "CustomLensProfileIsEmbedded",
    "CustomLensProfileName",
    "CustomLensProfileVignettingScale",
    "CustomTemperature",
    "CustomTint",
    "Preset",
    "RemoveAreas",
];

fn allowed_key(key: &str) -> Option<&'static str> {
    AUDIT_KEYS
        .iter()
        .copied()
        .find(|k| *k == key)
        .or_else(|| {
            import_lrcat::lua_develop::KEY_MAP
                .iter()
                .find_map(|(k, _)| (*k == key).then_some(*k))
        })
        .or_else(|| {
            import_lrcat::noop::RULES
                .iter()
                .find_map(|(k, _)| (*k == key).then_some(*k))
        })
}

fn warnings(issues: &[LrcatIssue]) -> Value {
    let mut counts = BTreeMap::<&'static str, u64>::new();
    for issue in issues {
        let category = match issue.category.as_str() {
            "Catalog" => "Catalog",
            "Develop settings" => "Develop settings",
            "Virtual copies" => "Virtual copies",
            "Stacks" => "Stacks",
            "Faces" => "Faces",
            "History" => "History",
            "Smart collections" => "Smart collections",
            "Keywords" => "Keywords",
            _ => "Other",
        };
        *counts.entry(category).or_default() += u64::from(issue.count);
    }
    json!(counts)
}

fn safe_options(import: &LrcatImport, app: &Path) -> SafeResult<LrcatOptions> {
    let mut options = safe(import.default_options())?;
    options.library_folder = app.join("library").to_string_lossy().into_owned();
    for (n, movement) in options.relocations.iter_mut().enumerate() {
        movement.to = app
            .join("missing")
            .join(n.to_string())
            .to_string_lossy()
            .into_owned();
    }
    // Validate lexically BEFORE plan/resolve performs any filesystem checks.
    let roots = root_paths(&import.plan);
    let moves = safe(relocations(&options))?;
    let check = |path: &Path| {
        let moved = relocate(path, &roots, &moves);
        moved.starts_with(app.join("missing"))
            && !moved
                .components()
                .any(|c| matches!(c, Component::ParentDir))
    };
    if roots.iter().any(|p| !check(p)) || import.plan.images.iter().any(|i| !check(&i.path)) {
        return Err(());
    }
    for folder in &import.plan.folders {
        if let Some(relative) = folder.get("pathFromRoot").and_then(Value::as_str)
            && (Path::new(relative).is_absolute()
                || Path::new(relative)
                    .components()
                    .any(|c| matches!(c, Component::ParentDir)))
        {
            return Err(());
        }
    }
    Ok(options)
}

fn aggregates(import: &LrcatImport, report: &LrcatReport) -> SafeResult<Value> {
    let mut keys = BTreeMap::<&'static str, [u64; 3]>::new();
    let mut unknown_retained = 0u64;
    let mut ignored = BTreeMap::<&str, u64>::new();
    let mut decoded = std::collections::BTreeSet::new();
    // One recipe at a time, from the app's spool, never a second import.
    for n in 0..import.plan.images.len() {
        let image = safe(import.read_image(n))?;
        if !image.recipe.history.entries.is_empty() {
            decoded.insert(image.catalog_id);
        }
        for (key, entries) in import_lrcat::diagnostics::entries(&image.recipe) {
            if entries.iter().any(|e| e.status == "ignored")
                && let Some(key) = allowed_key(&key)
            {
                *ignored.entry(key).or_default() += 1;
            }
            if entries.iter().any(|e| e.status == "approximate")
                && let Some(key) = allowed_key(&key)
            {
                keys.entry(key).or_default()[1] += 1;
            }
        }
        if let Some(properties) = image
            .recipe
            .unknown
            .get("lrcat_develop_source")
            .and_then(|v| v.get("properties"))
            .and_then(Value::as_object)
        {
            for key in properties.keys() {
                if let Some(key) = allowed_key(key) {
                    keys.entry(key).or_default()[2] += 1;
                } else {
                    unknown_retained += 1;
                }
            }
        }
    }
    // Read-only aggregate audit of source rows. No rows or errors escape this function.
    // The bounded projection avoids materializing oversized develop cells.
    let db = safe(rusqlite::Connection::open_with_flags(
        &import.catalog,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ))?;
    let develop_rows: i64 = safe(db.query_row(
        "SELECT count(*) FROM Adobe_imageDevelopSettings",
        [],
        |r| r.get(0),
    ))?;
    let mut statement = safe(db.prepare("SELECT image, processVersion, CASE WHEN octet_length(text) <= 4194304 THEN CAST(text AS TEXT) ELSE NULL END FROM Adobe_imageDevelopSettings WHERE rowid IN (SELECT max(rowid) FROM Adobe_imageDevelopSettings GROUP BY image) AND image IN (SELECT id_local FROM Adobe_images)"))?;
    let mut rows = safe(statement.query([]))?;
    let mut unaudited = 0u64;
    let mut unaudited_value_class_rows = 0u64;
    let mut warned_images = BTreeMap::<&str, u64>::new();
    let mut classes = BTreeMap::<String, BTreeMap<&str, u64>>::new();
    while let Some(row) = safe(rows.next())? {
        let id: i64 = safe(row.get(0))?;
        if !decoded.contains(&id) {
            continue;
        }
        let version: String = safe(row.get(1))?;
        let process_version = version.clone();
        let version = safe(engine_api::recipe::ProcessVersion::from_crs(&version))?;
        let source: Option<String> = row.get(2).ok().flatten();
        let Some(source) = source.filter(|s| !s.trim().is_empty()) else {
            unaudited += 1;
            continue;
        };
        let (_, source_warnings) = safe(import_lrcat::develop(id, &source, &process_version))?;
        let warned_keys: std::collections::BTreeSet<_> = source_warnings
            .iter()
            .map(|warning| {
                let warning = warning.strip_prefix("crs:").unwrap_or(warning);
                warning
                    .split_once(':')
                    .and_then(|(key, _)| allowed_key(key))
                    .unwrap_or("Other")
            })
            .collect();
        for key in warned_keys {
            *warned_images.entry(key).or_default() += 1;
        }
        let names: Vec<String> = if source.trim_start().starts_with('<') {
            unaudited_value_class_rows += 1;
            let source = if source.trim_start().starts_with("<rdf:Description") {
                format!(
                    "<rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">{source}</rdf:RDF>"
                )
            } else {
                source
            };
            match sidecar::XmpPacket::parse(source).and_then(|p| p.crs_values()) {
                Ok(values) => values.keys().map(|k| k.xmp_name().to_owned()).collect(),
                Err(_) => {
                    unaudited += 1;
                    continue;
                }
            }
        } else {
            match import_lrcat::lua_develop::read(&source) {
                Ok(import_lrcat::lua_develop::LuaValue::Table(table)) => {
                    for &(key, _) in import_lrcat::noop::RULES {
                        let value = table.fields.iter().find_map(|(k, v)| match k {
                            import_lrcat::lua_develop::LuaKey::Str(k) if k == key => Some(v),
                            _ => None,
                        });
                        if let Some(value) = value {
                            let class = if import_lrcat::noop::is_noop(key, &table, &version) {
                                "default_empty_inactive_provenance"
                            } else {
                                "non_default"
                            };
                            *classes
                                .entry(key.into())
                                .or_default()
                                .entry(class)
                                .or_default() += 1;
                            if key == "ToneCurveName2012" {
                                let class = if matches!(value, import_lrcat::lua_develop::LuaValue::String(s) if ["Linear", "Medium Contrast", "Strong Contrast", "Custom"].contains(&s.as_str()))
                                {
                                    "known_Adobe_name"
                                } else {
                                    "other_name"
                                };
                                *classes
                                    .entry(key.into())
                                    .or_default()
                                    .entry(class)
                                    .or_default() += 1;
                            }
                        }
                    }
                    table
                        .fields
                        .into_iter()
                        .filter_map(|(k, _)| match k {
                            import_lrcat::lua_develop::LuaKey::Str(s) => Some(s),
                            _ => None,
                        })
                        .collect()
                }
                _ => {
                    unaudited += 1;
                    continue;
                }
            }
        };
        for key in names.into_iter().collect::<std::collections::BTreeSet<_>>() {
            if CrsKey::from_xmp_name(&key).is_some()
                && let Some(key) = allowed_key(&key)
            {
                keys.entry(key).or_default()[0] += 1;
            }
        }
    }
    for (key, count) in ignored {
        if let Some(values) = keys.get_mut(key) {
            values[0] = values[0].saturating_sub(count);
        }
    }
    let mut warning_keys = BTreeMap::<&str, u64>::new();
    for issue in &report.unsupported {
        if issue.category != "Develop settings" {
            continue;
        }
        let reason = issue.reason.strip_prefix("crs:").unwrap_or(&issue.reason);
        let key = reason
            .split_once(':')
            .and_then(|(key, _)| allowed_key(key))
            .unwrap_or("Other");
        *warning_keys.entry(key).or_default() += u64::from(issue.count);
    }
    let approximate_groups_in_spool = keys.values().filter(|counts| counts[1] > 0).count();
    Ok(
        json!({"warned_images_by_key": warned_images, "warning_keys": warning_keys, "approximate_groups_in_spool": approximate_groups_in_spool, "value_classes": classes, "unaudited_value_class_rows": unaudited_value_class_rows, "develop_rows": develop_rows, "unaudited_develop_rows": unaudited,
        "keys_translated_approximate_retained": keys, "unlisted_retained_key_occurrences": unknown_retained,
        "warnings_by_category": warnings(&report.unsupported),
        "not_fully_supported_groups": report.unsupported.len(),
        "approximate_translation_groups": report.approximate.len(),
        "suppressed_examples": report.unsupported.iter().chain(&report.approximate).map(|i| i.examples.len()).sum::<usize>(),
        "imported": report.imported, "indexed": report.indexed, "skipped": report.skipped.len()}),
    )
}

fn profile(catalog: &Path, app: &Path) -> SafeResult<Value> {
    if cfg!(debug_assertions) {
        return Err(());
    }
    let app = safe(app.canonicalize())?;
    let temp = safe(std::env::temp_dir().canonicalize())?;
    let scratch = safe(Path::new("/tmp").canonicalize())?;
    if !(app.starts_with(&temp) || app.starts_with(&scratch))
        || safe(std::fs::read_dir(&app))?.next().is_some()
    {
        return Err(());
    }
    let catalog = safe(catalog.canonicalize())?;
    // A scratch copy is required; originals and user media directories are disallowed.
    if !(catalog.starts_with(&temp) || catalog.starts_with(&scratch)) || catalog.starts_with(&app) {
        return Err(());
    }
    let preview_dir = import_lrcat::previews::previews_dir(&catalog);
    if preview_dir.exists() {
        let preview_dir = safe(preview_dir.canonicalize())?;
        if !(preview_dir.starts_with(&temp) || preview_dir.starts_with(&scratch)) {
            return Err(());
        }
    }
    for suffix in ["-wal", "-shm"] {
        let mut sibling = catalog.as_os_str().to_os_string();
        sibling.push(suffix);
        let sibling = PathBuf::from(sibling);
        if sibling.exists() {
            let sibling = safe(sibling.canonicalize())?;
            if !(sibling.starts_with(&temp) || sibling.starts_with(&scratch)) {
                return Err(());
            }
        }
    }
    let (import, open) = measure(&app, || {
        let engine = safe(Engine::open(app.to_string_lossy().into_owned()))?;
        safe(engine.open_lrcat(catalog.to_string_lossy().into_owned()))
    })?;
    let options = safe_options(&import, &app)?;
    let ((summary, preview, fidelity), inspect) = measure(&app, || {
        Ok((
            import.summary(),
            safe(import.plan(options.clone()))?,
            safe(import.fidelity_sample(options.clone(), 12, 256))?,
        ))
    })?;
    let (report, apply) = measure(&app, || safe(import.apply(options, None)))?;
    let (counts, report_phase) = measure(&app, || aggregates(&import, &report))?;
    Ok(
        json!({"open": open, "inspect_preview": inspect, "apply": apply, "report": report_phase,
        "counts": counts, "images": summary.images, "collections": summary.collections,
        "collection_sets": summary.collection_sets, "smart_collections": summary.smart_collections,
        "keywords": summary.keywords, "edited": summary.edited, "missing": preview.missing,
        "fidelity_samples": fidelity.samples.len()}),
    )
}

#[test]
fn profile_synthetic_fixture() {
    let temp = tempfile::tempdir().unwrap();
    let fixture = import_lrcat::fixture::write(&temp.path().join("fixture")).unwrap();
    let app = temp.path().join("app");
    std::fs::create_dir(&app).unwrap();
    // The release-only profile is exercised by the required release gate.
    if cfg!(debug_assertions) {
        return;
    }
    let value = profile(&fixture.catalog, &app).expect("aggregate profile failed");
    assert_eq!(value["images"], 7);
    assert_eq!(value["collections"], 2);
    assert_eq!(value["keywords"], 7);
    assert_eq!(value["missing"], 6);
    assert!(value["counts"]["develop_rows"].as_u64().unwrap() > 0);
    assert!(
        value["counts"]["keys_translated_approximate_retained"]
            .as_object()
            .unwrap()
            .values()
            .any(|v| v[0].as_u64().unwrap() > 0)
    );
    assert_eq!(value["counts"]["imported"], 0);
    assert_eq!(value["counts"]["approximate_translation_groups"], 0);
    assert!(value["apply"]["app_bytes_added"].as_u64().unwrap() > 0);
    let encoded = value.to_string();
    for secret in [
        fixture.catalog.to_string_lossy().as_ref(),
        "Paris",
        "ceremony",
        "FutureKnob",
    ] {
        assert!(!encoded.contains(secret), "privacy regression");
    }
    let hostile = LrcatIssue {
        category: "private-category".into(),
        reason: "private-reason".into(),
        count: 3,
        examples: vec!["private-example".into()],
    };
    assert_eq!(warnings(&[hostile]), json!({"Other": 3}));
    assert_eq!(allowed_key("private-key"), None);
    assert!(profile(&fixture.catalog, &app).is_err());
}

// The opt-in test must run alone: redirect dependency output too, not just errors.
struct Quiet {
    saved: Vec<(i32, std::fs::File)>,
}
unsafe extern "C" {
    fn dup(fd: i32) -> i32;
    fn dup2(old: i32, new: i32) -> i32;
}
impl Quiet {
    fn new() -> SafeResult<Self> {
        use std::os::fd::{AsRawFd, FromRawFd};
        let null = safe(std::fs::OpenOptions::new().write(true).open("/dev/null"))?;
        let mut guard = Self { saved: vec![] };
        for fd in [1, 2] {
            // SAFETY: dup returns an owned descriptor, checked before adoption.
            let saved = unsafe { dup(fd) };
            if saved < 0 {
                return Err(());
            }
            guard
                .saved
                .push((fd, unsafe { std::fs::File::from_raw_fd(saved) }));
            // SAFETY: both descriptors are valid and remain open for this call.
            if unsafe { dup2(null.as_raw_fd(), fd) } < 0 {
                return Err(());
            }
        }
        Ok(guard)
    }
}
impl Drop for Quiet {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;
        for (fd, saved) in &self.saved {
            // SAFETY: the saved descriptor is owned by this guard and still open.
            if unsafe { dup2(saved.as_raw_fd(), *fd) } < 0 {
                std::process::exit(1);
            }
        }
    }
}

#[test]
#[ignore = "single isolated release profile; requires TESSERA_LRCAT_PROFILE and fresh TESSERA_APP_DIR"]
fn profile_from_env() {
    // Failures (including panic payloads from dependencies) are suppressed: a real
    // catalog must never reach Rust's default panic/error formatter.
    std::panic::set_hook(Box::new(|_| {}));
    let result = std::panic::catch_unwind(|| {
        let _quiet = Quiet::new()?;
        let catalog = safe(std::env::var("TESSERA_LRCAT_PROFILE"))?;
        let app = safe(std::env::var("TESSERA_APP_DIR"))?;
        profile(Path::new(&catalog), Path::new(&app))
    });
    match result {
        Ok(Ok(value)) => println!("{value}"),
        _ => {
            println!("{{\"profile_failed\":1}}");
            std::process::exit(1);
        }
    }
}

fn current_rss() -> SafeResult<u64> {
    let output = safe(
        std::process::Command::new("/bin/ps")
            .args(["-o", "rss=", "-p", &std::process::id().to_string()])
            .output(),
    )?;
    if !output.status.success() {
        return Err(());
    }
    Ok(safe(
        safe(std::str::from_utf8(&output.stdout))?
            .trim()
            .parse::<u64>(),
    )? * 1024)
}

fn measure_phase<T>(
    app: &Path,
    operation: impl FnOnce() -> SafeResult<T>,
) -> SafeResult<(T, Value)> {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    let before = bytes(app)?;
    let peak = AtomicU64::new(current_rss()?);
    let stop = AtomicBool::new(false);
    let start = Instant::now();
    let (result, seconds) = std::thread::scope(|scope| {
        scope.spawn(|| {
            while !stop.load(Ordering::Relaxed) {
                if let Ok(rss) = current_rss() {
                    peak.fetch_max(rss, Ordering::Relaxed);
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        });
        struct Stop<'a>(&'a AtomicBool);
        impl Drop for Stop<'_> {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Relaxed);
            }
        }
        let guard = Stop(&stop);
        let result = operation();
        let seconds = start.elapsed().as_secs_f64();
        drop(guard);
        stop.store(true, Ordering::Relaxed);
        (result, seconds)
    });
    peak.fetch_max(current_rss()?, Ordering::Relaxed);
    let after = bytes(app)?;
    Ok((
        result?,
        json!({"seconds":seconds,"sampled_peak_rss_bytes":peak.load(Ordering::Relaxed),
        "process_peak_rss_bytes":peak_rss()?,"bytes_written":after.saturating_sub(before),"app_bytes":after}),
    ))
}

fn scratch_directory(variable: &str, fresh: bool) -> SafeResult<PathBuf> {
    let path = PathBuf::from(safe(std::env::var_os(variable).ok_or(()))?);
    safe(std::fs::create_dir_all(&path))?;
    let path = safe(path.canonicalize())?;
    let scratch = safe(Path::new("/tmp").canonicalize())?;
    if !path.starts_with(scratch) || (fresh && safe(std::fs::read_dir(&path))?.next().is_some()) {
        return Err(());
    }
    Ok(path)
}

// Private scratch diagnostic only. Never include these errors in aggregate output:
// decoder and sidecar errors may contain source identities.
fn pair_checked<T, E: std::fmt::Debug>(
    app: &Path,
    stage: u32,
    value: std::result::Result<T, E>,
) -> SafeResult<T> {
    value.map_err(|error| {
        let _ = std::fs::write(
            app.join("pair-error.private"),
            format!("{stage}: {error:?}"),
        );
    })
}

fn pair_pixels(path: &Path, app: &Path) -> SafeResult<(image::RgbImage, bool)> {
    use image_core::{PixelRect, Renderer, RendererConfig};
    let doc = pair_checked(app, 1, Sidecar::read_recipe(Sidecar::paths(path).recipe))?;
    let id = app_image_id(path).ok_or(())?;
    let source = pair_checked(app, 2, crate::catalog::open_image(id, path))?;
    // This is exactly the viewport's CPU Develop settings admission. Retained
    // unsupported edits stay in the sidecar; no rewritten/default recipe is saved.
    let unavailable_lens = matches!(
        doc.recipe.settings.lens.profile,
        engine_api::recipe::settings::LensProfileSource::Database { .. }
    );
    let mut settings = crate::develop::session_renderable(&doc.recipe.settings, true, false);
    settings.output.hdr = false;
    settings.output.hdr_headroom_stops = 0.;
    let renderer = Renderer::new(RendererConfig {
        process_version: doc.recipe.process_version,
        ..Default::default()
    });
    let masks = crate::develop::masks::MaskShared::new(&source);
    pair_checked(app, 3, masks.load_imported(app, &settings))?;
    renderer
        .mask_cache()
        .set_hooks(Some(std::sync::Arc::new(crate::develop::masks::Hooks(
            masks,
        ))));
    let mut level = 0;
    while level < image_core::render::MAX_LEVEL {
        let e = pair_checked(
            app,
            4,
            Renderer::output_extent(&source, &settings, level + 1),
        )?;
        if e.width.max(e.height) < 1024 {
            break;
        }
        level += 1;
    }
    let extent = pair_checked(app, 5, Renderer::output_extent(&source, &settings, level))?;
    let tiles = pair_checked(
        app,
        6,
        renderer.render_region(&source, &settings, level, PixelRect::full(extent)),
    )?;
    let pixels = pair_checked(app, 7, crate::lrcat_fidelity::stitch(extent, &tiles))?;
    Ok((contact_thumbnail(&pixels), unavailable_lens))
}

fn contact_thumbnail(pixels: &image::RgbImage) -> image::RgbImage {
    let scale = 1024. / f64::from(pixels.width().max(pixels.height()).max(1));
    let width = (f64::from(pixels.width()) * scale).round().max(1.) as u32;
    let height = (f64::from(pixels.height()) * scale).round().max(1.) as u32;
    image::imageops::thumbnail(pixels, width, height)
}

fn pair_measurement(a: &image::RgbImage, b: &image::RgbImage) -> Value {
    use image::imageops::{FilterType, flip_horizontal, resize, rotate90, rotate180, rotate270};
    let luma = |p: &image::Rgb<u8>| {
        0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2])
    };
    let aspect = (f64::from(a.width()) * f64::from(b.height())
        / (f64::from(a.height()) * f64::from(b.width()))
        - 1.)
        .abs()
        < 0.02;
    let a = resize(a, 64, 64, FilterType::Triangle);
    let b = resize(b, 64, 64, FilterType::Triangle);
    let difference = |a: &image::RgbImage| {
        a.pixels()
            .zip(b.pixels())
            .map(|(x, y)| (luma(x) - luma(y)).abs())
            .sum::<f64>()
            / 4096.
    };
    let identity = difference(&a);
    let mirrored = flip_horizontal(&a);
    let alternatives = [
        rotate90(&a),
        rotate180(&a),
        rotate270(&a),
        mirrored.clone(),
        rotate90(&mirrored),
        rotate180(&mirrored),
        rotate270(&mirrored),
    ];
    let orientation = alternatives
        .iter()
        .all(|p| difference(p) + 0.25 >= identity);
    let mean_rgb_delta: [f64; 3] = std::array::from_fn(|c| {
        a.pixels()
            .zip(b.pixels())
            .map(|(a, b)| f64::from(a[c]) - f64::from(b[c]))
            .sum::<f64>()
            / 4096.
    });
    json!({"luminance_mad_8bit":identity,"mean_rgb_delta_8bit":mean_rgb_delta,"orientation_aspect_match":orientation&&aspect})
}

fn proxy_profile() -> std::result::Result<Value, u32> {
    if cfg!(debug_assertions) {
        return Err(1);
    }
    let contacts_only = std::env::var_os("TESSERA_LR8_CONTACT_ONLY").is_some();
    let app = scratch_directory("TESSERA_APP_DIR", !contacts_only).map_err(|_| 2u32)?;
    let contact = scratch_directory("TESSERA_LR8_CONTACT", !contacts_only).map_err(|_| 3u32)?;
    let catalog = PathBuf::from(std::env::var_os("TESSERA_LRCAT_PROFILE").ok_or(4u32)?);
    let catalog = catalog.canonicalize().map_err(|_| 4u32)?;
    if !catalog.starts_with(Path::new("/private/tmp")) {
        return Err(4);
    }
    let status = |phase: u32| -> SafeResult<()> {
        safe(std::fs::write(
            app.join("phase.json"),
            json!({"phase":phase}).to_string(),
        ))
    };
    status(1).map_err(|_| 5u32)?;
    let (import, open) = measure_phase(&app, || {
        let engine = safe(Engine::open(app.to_string_lossy().into_owned()))?;
        safe(engine.open_lrcat(catalog.to_string_lossy().into_owned()))
    })
    .map_err(|_| 10u32)?;
    let mut options = import.default_options().map_err(|_| 11u32)?;
    options.library_folder = app.join("library").to_string_lossy().into_owned();
    options.copy_proxies = false;
    // Every original and proxy is a read-only source for this dry run, including
    // the few originals outside Lightroom-owned bundles. Verify the actual
    // write destinations before calling either plan or apply.
    let resolved = resolve(&import.plan, &options).map_err(|_| 12u32)?;
    let profile = if contacts_only {
        serde_json::from_slice::<Value>(
            &std::fs::read(app.join("aggregate.json")).map_err(|_| 22u32)?,
        )
        .map_err(|_| 23u32)?
    } else {
        let mut all = [0u64; 3];
        for row in &resolved {
            all[if row.original_path.is_file() {
                0
            } else if row.smart_preview_available {
                1
            } else {
                2
            }] += 1;
        }
        status(2).map_err(|_| 14u32)?;
        let (plan, plan_phase) = measure_phase(&app, || {
            for row in &resolved {
                Sidecar::register_read_only_store(&row.path, &app);
                let paths = Sidecar::paths(&row.path);
                if !paths.recipe.starts_with(&app) || !paths.xmp.starts_with(&app) {
                    return Err(());
                }
            }
            safe(import.plan(options.clone()))
        })
        .map_err(|_| 20u32)?;
        status(3).map_err(|_| 21u32)?;
        let (report, apply) =
            measure_phase(&app, || safe(import.apply(options, None))).map_err(|_| 30u32)?;
        let value = json!({"images_total":import.summary.images,"all_images_online":all[0],"all_images_offline_proxy":all[1],"all_images_offline_without":all[2],
        "online_originals":plan.online_originals,"offline_proxy_masters":plan.offline_with_smart_preview,
        "offline_without_masters":plan.offline_without_smart_preview,"virtual_copies":plan.virtual_copies,
        "imported":report.imported,"resumed":report.resumed,"skipped":report.skipped.len(),"indexed":report.indexed,
        "cancelled":report.cancelled,"open":open,"plan":plan_phase,"apply":apply});
        safe(std::fs::write(
            app.join("aggregate.json"),
            value.to_string(),
        ))
        .map_err(|_| 31u32)?;
        value
    };
    status(4).map_err(|_| 32u32)?;
    let original_catalog =
        PathBuf::from(std::env::var_os("TESSERA_LRCAT_PREVIEW_CATALOG").ok_or(40u32)?);
    let previews = import_lrcat::previews::PreviewIndex::open(&original_catalog)
        .map_err(|_| 41u32)?
        .ok_or(42u32)?;
    let mut proxies = Vec::new();
    for row in resolved
        .iter()
        .filter(|r| r.outcome == Outcome::OfflineProxy)
    {
        if previews
            .has_preview(import.plan.images[row.index].catalog_id)
            .map_err(|_| 44u32)?
        {
            proxies.push(row);
        }
    }
    proxies.sort_by_key(|r| import.plan.images[r.index].catalog_id);
    if proxies.len() < 12 {
        return Err(43);
    }
    let step = (proxies.len() / 12).max(1);
    let mut pairs = Vec::new();
    let mut unavailable_lens_profiles = 0usize;
    for (n, row) in proxies.iter().step_by(step).take(12).enumerate() {
        let id = import.plan.images[row.index].catalog_id;
        let jpeg = previews
            .jpeg(id, u32::MAX)
            .map_err(|_| 50u32 + n as u32)?
            .ok_or(70u32 + n as u32)?;
        let reference =
            crate::lrcat_fidelity::decode_preview(&jpeg).map_err(|_| 90u32 + n as u32)?;
        let (rendered, unavailable_lens) =
            pair_pixels(&row.path, &app).map_err(|_| 110u32 + n as u32)?;
        unavailable_lens_profiles += usize::from(unavailable_lens);
        safe(std::fs::write(
            contact.join(format!("{:02}-lightroom.jpg", n + 1)),
            jpeg,
        ))
        .map_err(|_| 130u32 + n as u32)?;
        rendered
            .save(contact.join(format!("{:02}-tessera.png", n + 1)))
            .map_err(|_| 150u32 + n as u32)?;
        pairs.push(pair_measurement(&rendered, &reference));
        safe(std::fs::write(
            app.join("pairs-aggregate.json"),
            json!({"pairs":pairs,"unavailable_lens_profiles":unavailable_lens_profiles})
                .to_string(),
        ))
        .map_err(|_| 172u32)?;
        status(100 + n as u32).map_err(|_| 170u32)?;
    }
    status(5).map_err(|_| 171u32)?;
    Ok(
        json!({"profile":profile,"comparable_proxies":proxies.len(),"sample_step":step,"unavailable_lens_profiles":unavailable_lens_profiles,"pairs":pairs}),
    )
}

#[test]
#[ignore = "isolated release LR-8 dry run: fresh scratch app/contact, read-only source bundle"]
fn proxy_profile_from_env() {
    std::panic::set_hook(Box::new(|_| {}));
    let result = std::panic::catch_unwind(|| {
        let _quiet = Quiet::new().map_err(|_| 0u32)?;
        proxy_profile()
    });
    match result {
        Ok(Ok(value)) => println!("{value}"),
        Ok(Err(phase)) => {
            println!("{{\"profile_failed_phase\":{phase}}}");
            std::process::exit(1);
        }
        Err(_) => {
            println!("{{\"profile_panicked\":1}}");
            std::process::exit(1);
        }
    }
}

#[test]
#[ignore = "numeric-only PNG comparison; requires TESSERA_LR8_A and TESSERA_LR8_B"]
fn tone_comparison_from_env() {
    std::panic::set_hook(Box::new(|_| {}));
    let run = || -> SafeResult<Value> {
        let mut means = Vec::new();
        for variable in ["TESSERA_LR8_A", "TESSERA_LR8_B"] {
            let path = safe(std::env::var_os(variable).ok_or(()))?;
            let image = safe(image::open(path))?.into_rgb8();
            let image = image::imageops::thumbnail(&image, 256, 256);
            let mean: [f64; 3] = std::array::from_fn(|c| {
                image.pixels().map(|p| f64::from(p[c])).sum::<f64>()
                    / f64::from(image.width() * image.height())
            });
            means.push(mean);
        }
        Ok(json!({"downsampled_rgb_means":means}))
    };
    match run() {
        Ok(value) => println!("{value}"),
        Err(_) => panic!("numeric comparison failed"),
    }
}

#[test]
fn contact_thumbnail_preserves_non_square_aspect() {
    for (w, h, expected) in [(8, 4, (1024, 512)), (4, 8, (512, 1024))] {
        let pixels = image::RgbImage::new(w, h);
        assert_eq!(contact_thumbnail(&pixels).dimensions(), expected);
    }
}

#[test]
fn lr10_pair_metrics_report_signed_rgb_delta() {
    let a = image::RgbImage::from_pixel(8, 8, image::Rgb([30, 40, 50]));
    let b = image::RgbImage::from_pixel(8, 8, image::Rgb([10, 50, 40]));
    let value = pair_measurement(&a, &b);
    assert_eq!(value["mean_rgb_delta_8bit"], json!([20., -10., 10.]));
    assert!((value["luminance_mad_8bit"].as_f64().unwrap() - 2.178).abs() < 1e-8);
}

/// Recompute both metrics from already-captured contact pairs without rerendering.
#[test]
#[ignore = "numeric-only private contact metrics; requires TESSERA_LR10_MEASURE_DIR"]
fn lr10_saved_contact_metrics_from_env() {
    let directory = scratch_directory("TESSERA_LR10_MEASURE_DIR", false).unwrap();
    let pairs: Vec<_> = (1..=12)
        .map(|n| {
            let rendered = image::open(directory.join(format!("{n:02}-tessera.png")))
                .unwrap()
                .into_rgb8();
            let jpeg = std::fs::read(directory.join(format!("{n:02}-lightroom.jpg"))).unwrap();
            let reference = crate::lrcat_fidelity::decode_preview(&jpeg).unwrap();
            pair_measurement(&rendered, &reference)
        })
        .collect();
    std::fs::write(
        directory.join("metrics.json"),
        json!({"pairs":pairs}).to_string(),
    )
    .unwrap();
}

// Aggregate-only recipe audit. No source files are resolved or opened.
#[test]
#[ignore = "LR-12 private read-only recipe admission audit"]
fn lr12_recipe_audit_from_env() {
    std::panic::set_hook(Box::new(|_| {}));
    let run = || -> SafeResult<Value> {
        let _quiet = Quiet::new()?;
        let catalog = PathBuf::from(std::env::var_os("TESSERA_LRCAT_PROFILE").ok_or(())?);
        if !safe(catalog.canonicalize())?.starts_with("/private/tmp") {
            return Err(());
        }
        let mut counts = BTreeMap::<String, usize>::new();
        let defaults = safe(serde_json::to_value(
            engine_api::recipe::DevelopSettings::default(),
        ))?;
        safe(import_lrcat::import_each_with_storage(
            &catalog,
            None,
            |_| Ok(()),
            |image| {
                *counts.entry("images".into()).or_default() += 1;
                let mut settings = image.recipe.settings;
                if image.recipe.process_version.family == engine_api::recipe::ProcessFamily::Adobe {
                    settings.camera_profile.profile = Default::default();
                    settings.tone.display_transform = Default::default();
                }
                let fields = serde_json::to_value(&settings)?;
                for (stage, values) in fields.as_object().unwrap() {
                    for (field, value) in values.as_object().into_iter().flatten() {
                        if value == &defaults[stage][field] {
                            continue;
                        }
                        let mut isolated = defaults.clone();
                        isolated[stage][field] = value.clone();
                        let isolated = serde_json::from_value(isolated)?;
                        if pipeline_cpu::validate_settings(&isolated).is_err() {
                            *counts.entry(format!("{stage}/{field}")).or_default() += 1;
                        }
                    }
                }
                Ok(())
            },
        ))?;
        Ok(json!(counts))
    };
    match run() {
        Ok(value) => println!("{value}"),
        Err(()) => panic!("aggregate audit failed"),
    }
}
