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

fn allowed_key(key: &str) -> Option<&'static str> {
    import_lrcat::lua_develop::KEY_MAP
        .iter()
        .find_map(|(k, _)| (*k == key).then_some(*k))
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
    let mut statement = safe(db.prepare("SELECT image, CASE WHEN octet_length(text) <= 4194304 THEN CAST(text AS TEXT) ELSE NULL END FROM Adobe_imageDevelopSettings WHERE rowid IN (SELECT max(rowid) FROM Adobe_imageDevelopSettings GROUP BY image) AND image IN (SELECT id_local FROM Adobe_images)"))?;
    let mut rows = safe(statement.query([]))?;
    let mut unaudited = 0u64;
    while let Some(row) = safe(rows.next())? {
        let id: i64 = safe(row.get(0))?;
        if !decoded.contains(&id) {
            continue;
        }
        let source: Option<String> = row.get(1).ok().flatten();
        let Some(source) = source.filter(|s| !s.trim().is_empty()) else {
            unaudited += 1;
            continue;
        };
        let names: Vec<String> = if source.trim_start().starts_with('<') {
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
                Ok(import_lrcat::lua_develop::LuaValue::Table(table)) => table
                    .fields
                    .into_iter()
                    .filter_map(|(k, _)| match k {
                        import_lrcat::lua_develop::LuaKey::Str(s) => Some(s),
                        _ => None,
                    })
                    .collect(),
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
    Ok(
        json!({"develop_rows": develop_rows, "unaudited_develop_rows": unaudited,
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
