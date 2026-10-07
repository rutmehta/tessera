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

fn open_audit_catalog(catalog: &Path) -> SafeResult<rusqlite::Connection> {
    // This is a stable scratch copy. Immutable mode avoids locks and sidecar writes.
    // Encode bytes so URI delimiters and non-UTF-8 filenames remain literal paths.
    use std::fmt::Write;
    let mut uri = String::from("file:");
    for &byte in catalog.as_os_str().as_encoded_bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~".contains(&byte) {
            uri.push(char::from(byte));
        } else {
            safe(write!(&mut uri, "%{byte:02X}"))?;
        }
    }
    uri.push_str("?mode=ro&immutable=1");
    safe(rusqlite::Connection::open_with_flags(
        uri,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
            | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX
            | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    ))
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
    let db = open_audit_catalog(&import.catalog)?;
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

// Ask Darwin directly: std::env::temp_dir() trusts caller-controlled TMPDIR.
fn system_temp_dir() -> SafeResult<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    unsafe extern "C" {
        fn confstr(name: i32, buffer: *mut u8, len: usize) -> usize;
    }
    const CS_DARWIN_USER_TEMP_DIR: i32 = 65537;
    // SAFETY: a null buffer with zero length queries the required size.
    let len = unsafe { confstr(CS_DARWIN_USER_TEMP_DIR, std::ptr::null_mut(), 0) };
    if len == 0 {
        return Err(());
    }
    let mut buffer = vec![0u8; len];
    // SAFETY: buffer owns len writable bytes; confstr returns the required size.
    let written = unsafe { confstr(CS_DARWIN_USER_TEMP_DIR, buffer.as_mut_ptr(), len) };
    if written != len || buffer.pop() != Some(0) {
        return Err(());
    }
    safe(PathBuf::from(std::ffi::OsString::from_vec(buffer)).canonicalize())
}

fn profile(catalog: &Path, app: &Path) -> SafeResult<Value> {
    if cfg!(debug_assertions) {
        return Err(());
    }
    let app = safe(app.canonicalize())?;
    let temp = system_temp_dir()?;
    let scratch = safe(Path::new("/tmp").canonicalize())?;
    if !(app.starts_with(&temp) || app.starts_with(&scratch))
        || app.ancestors().any(|ancestor| {
            ancestor.extension().is_some_and(|ext| {
                ext.eq_ignore_ascii_case("lrdata") || ext.eq_ignore_ascii_case("lrcat")
            })
        })
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
#[cfg_attr(debug_assertions, ignore = "requires release profile")]
fn profile_synthetic_fixture() {
    // Use the guard's OS-provided scratch root, regardless of the gate's TMPDIR.
    let temp = tempfile::tempdir_in(system_temp_dir().unwrap()).unwrap();
    let fixture = import_lrcat::fixture::write(&temp.path().join("fixture")).unwrap();
    let app = temp.path().join("app");
    std::fs::create_dir(&app).unwrap();
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

#[test]
#[cfg_attr(debug_assertions, ignore = "requires release profile")]
fn profile_rejects_tmpdir_widening() {
    const CHILD: &str = "TESSERA_PROFILE_TMPDIR_TEST_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let root = std::env::temp_dir();
        let fixture = import_lrcat::fixture::write(&root.join("fixture")).unwrap();
        let app = root.join("app");
        std::fs::create_dir(&app).unwrap();
        assert!(profile(&fixture.catalog, &app).is_err());
        assert!(std::fs::read_dir(app).unwrap().next().is_none());
        return;
    }
    // A synthetic directory outside OS scratch roots; mutate only the child's
    // env. The first candidate not under /tmp or the system temp directory,
    // so a checkout that itself lives in a scratch root still tests a
    // widened TMPDIR (REV-SP-B N6).
    let temp = system_temp_dir().unwrap();
    let scratch = Path::new("/tmp").canonicalize().unwrap();
    let base = [
        std::env::current_dir().ok(),
        Some(PathBuf::from(env!("CARGO_MANIFEST_DIR"))),
        // The test binary's own directory (the target dir), usually elsewhere.
        std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(Path::to_path_buf)),
    ]
    .into_iter()
    .flatten()
    .filter_map(|p| p.canonicalize().ok())
    .find(|p| !p.starts_with(&temp) && !p.starts_with(&scratch))
    .expect("a synthetic directory outside the OS scratch roots");
    let root = tempfile::tempdir_in(base).unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "lrcat::lrcat_profile::profile_rejects_tmpdir_widening",
        ])
        .env(CHILD, "1")
        .env("TMPDIR", root.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "child rejected widened temp root test failed"
    );
}

#[test]
#[cfg_attr(debug_assertions, ignore = "requires release profile")]
fn profile_rejects_lightroom_bundle_app_dirs() {
    let root = tempfile::tempdir_in("/tmp").unwrap();
    let fixture = import_lrcat::fixture::write(&root.path().join("fixture")).unwrap();
    for bundle in [
        "synthetic.lrdata",
        "synthetic.lrcat",
        "synthetic.LRDATA",
        "synthetic.LRCAT",
    ] {
        let app = root.path().join(bundle).join("nested/app");
        std::fs::create_dir_all(&app).unwrap();
        assert!(profile(&fixture.catalog, &app).is_err());
        assert!(std::fs::read_dir(app).unwrap().next().is_none());
    }
}

#[test]
fn audit_catalog_is_immutable_and_read_only() {
    let root = tempfile::tempdir_in("/tmp").unwrap();
    let path = root.path().join("synthetic ?#%.lrcat");
    let writer = rusqlite::Connection::open(&path).unwrap();
    writer
        .execute_batch(
            "CREATE TABLE probe (value INTEGER); INSERT INTO probe VALUES (51); BEGIN EXCLUSIVE;",
        )
        .unwrap();
    let reader = open_audit_catalog(&path).unwrap();
    reader.busy_timeout(std::time::Duration::ZERO).unwrap();
    assert_eq!(
        reader
            .query_row("SELECT value FROM probe", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        51
    );
    assert!(reader.execute("INSERT INTO probe VALUES (52)", []).is_err());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
#[cfg(debug_assertions)]
fn debug_runner_lists_synthetic_profile_as_ignored() {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--list"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let listing = String::from_utf8(output.stdout).unwrap();
    assert!(
        listing
            .lines()
            .any(|line| line == "lrcat::lrcat_profile::profile_synthetic_fixture: test")
    );
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

/// Private comparison facts for one pair (numbers only).
struct PairFacts {
    unavailable_lens: bool,
    orientation: u16,
    cropped: bool,
}

fn pair_pixels(path: &Path, app: &Path) -> SafeResult<(image::RgbImage, PairFacts)> {
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
    })
    .with_host_ignored_native_profiles();
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
    // Develop renders the sensor frame and the host displays it oriented
    // (LR-8m); Lightroom's previews are displayed pixels.
    let orientation = source.metadata().orientation;
    let pixels = crate::assist::orient(pixels, orientation as u8);
    let crop = settings.geometry.crop;
    let cropped = crop.angle != 0.
        || [crop.rect.left, crop.rect.top] != [0., 0.]
        || [crop.rect.right, crop.rect.bottom] != [1., 1.];
    Ok((
        contact_thumbnail(&pixels),
        PairFacts {
            unavailable_lens,
            orientation,
            cropped,
        },
    ))
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
        let (rendered, facts) = pair_pixels(&row.path, &app).map_err(|_| 110u32 + n as u32)?;
        unavailable_lens_profiles += usize::from(facts.unavailable_lens);
        safe(std::fs::write(
            contact.join(format!("{:02}-lightroom.jpg", n + 1)),
            jpeg,
        ))
        .map_err(|_| 130u32 + n as u32)?;
        rendered
            .save(contact.join(format!("{:02}-tessera.png", n + 1)))
            .map_err(|_| 150u32 + n as u32)?;
        let mut measured = pair_measurement(&rendered, &reference);
        measured["display_orientation"] = json!(facts.orientation);
        measured["cropped"] = json!(facts.cropped);
        pairs.push(measured);
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

#[test]
#[ignore = "LR-13 aggregate read-only proxy admission measurement"]
fn lr13_proxy_admission_from_env() {
    std::panic::set_hook(Box::new(|_| {}));
    let run = || -> SafeResult<Value> {
        let _quiet = Quiet::new()?;
        let catalog = PathBuf::from(std::env::var_os("TESSERA_LRCAT_PROFILE").ok_or(())?);
        if !safe(catalog.canonicalize())?.starts_with("/private/tmp") {
            return Err(());
        }
        let index = import_lrcat::smart_previews::SmartPreviewIndex::from_bundle(
            std::env::var_os("TESSERA_LRCAT_SMART_PREVIEWS").ok_or(())?,
        );
        let baseline = std::env::var_os("TESSERA_LR13_BASELINE").is_some();
        let mut counts = BTreeMap::<String, usize>::new();
        safe(import_lrcat::import_each_with_storage(
            &catalog,
            None,
            |_| Ok(()),
            |image| {
                if image.master_image.is_some() {
                    return Ok(());
                }
                if image.path.is_file() {
                    *counts.entry("originals_resolve".into()).or_default() += 1;
                }
                let Some(path) = image.file_uuid.as_deref().and_then(|u| index.find(u)) else {
                    return Ok(());
                };
                *counts.entry("proxies".into()).or_default() += 1;
                let Some(metadata) =
                    raw_decode::lossy_dng::read_metadata(&mut std::fs::File::open(&path)?)?
                else {
                    *counts.entry("header_unrecognized".into()).or_default() += 1;
                    return Ok(());
                };
                let baseline_exposure = metadata.baseline_exposure;
                let proxy =
                    pipeline_cpu::CameraLinearProxy::from_dng(raw_decode::lossy_dng::LossyDng {
                        width: 2,
                        height: 2,
                        pixels: vec![[0.1; 3]; 4],
                        metadata,
                        color_matrices: [None; 2],
                        forward_matrices: [None; 2],
                        calibration_illuminants: [0; 2],
                        baseline_exposure,
                    });
                let Ok(proxy) = proxy else {
                    *counts.entry("source_admission_failed".into()).or_default() += 1;
                    return Ok(());
                };
                let embedded = image_core::pipeline_adobe::dcp::read_embedded_profile(
                    &mut std::fs::File::open(&path)?,
                )
                .map_err(|_| engine_api::EngineError::invalid("profile", "header invalid"))?;
                let mut proxy = proxy.with_embedded_profile(embedded);
                if let Some(orientation) = image
                    .orientation
                    .as_deref()
                    .and_then(import_lrcat::orientation::exif)
                {
                    proxy = proxy.with_catalog_orientation(orientation)?;
                }
                let raw = image_core::RawImage::from_camera_linear_proxy(
                    engine_api::id::ImageId(1),
                    engine_api::id::ImageId(2),
                    Arc::new(proxy.clone()),
                )?;
                let renderer = image_core::Renderer::new(Default::default())
                    .with_host_ignored_native_profiles()
                    .for_recipe(&image.recipe);
                let masks = crate::develop::masks::MaskShared::new(&raw);
                renderer
                    .mask_cache()
                    .set_hooks(Some(Arc::new(crate::develop::masks::Hooks(masks))));
                let drawn = crate::develop::session_renderable(&image.recipe.settings, true, false);
                let develop = renderer.render_tiles(
                    &raw,
                    &drawn,
                    &[],
                    image_core::RenderOutput::default(),
                    &engine_api::jobs::CancellationToken::new(),
                    &mut |_| {},
                );
                *counts
                    .entry(format!(
                        "develop/{}",
                        if develop.is_ok() {
                            "renderable"
                        } else {
                            "rejected"
                        }
                    ))
                    .or_default() += 1;
                let mut checked = image.recipe.settings.clone();
                if image.recipe.process_version.family == engine_api::recipe::ProcessFamily::Adobe {
                    checked.camera_profile.profile = Default::default();
                    checked.tone.display_transform = Default::default();
                }
                let original_checked = checked.clone();
                if !baseline {
                    checked = proxy.render_plan(&checked, false).0;
                }
                let validation = pipeline_cpu::validate_settings(&checked)
                    .and_then(|_| {
                        let metadata = proxy.original_metadata();
                        let camera = pipeline_cpu::camera_to_xyz(engine_api::color::ColorMatrix3(
                            std::array::from_fn(|r| metadata.cam_xyz[r].map(f64::from)),
                        ))?;
                        pipeline_cpu::white_balance_matrix(
                            &checked.white_balance,
                            camera,
                            metadata.as_shot_wb,
                        )
                        .map(|_| ())
                    })
                    .and_then(|_| proxy.validate_prefix(&checked))
                    .and_then(|_| {
                        pipeline_cpu::resolve_lens(
                            proxy.pixels(),
                            &checked.lens,
                            Some(proxy.original_metadata()),
                            &Default::default(),
                        )
                        .map(|_| ())
                    });
                let preview_renderer = image_core::Renderer::new(Default::default())
                    .with_host_ignored_native_profiles()
                    .for_recipe(&image.recipe);
                let preview_after = preview_renderer.render_tiles(
                    &raw,
                    &drawn,
                    &[],
                    image_core::RenderOutput::default(),
                    &engine_api::jobs::CancellationToken::new(),
                    &mut |_| {},
                );
                for route in ["preview", "export"] {
                    let admitted = if route == "preview" && !baseline {
                        preview_after.is_ok()
                    } else {
                        validation.is_ok()
                    };
                    *counts
                        .entry(format!(
                            "{route}/{}",
                            if admitted { "renderable" } else { "rejected" }
                        ))
                        .or_default() += 1;
                }
                let defaults =
                    serde_json::to_value(engine_api::recipe::DevelopSettings::default())?;
                let fields = serde_json::to_value(&original_checked)?;
                for (stage, values) in fields.as_object().unwrap() {
                    for (field, value) in values.as_object().into_iter().flatten() {
                        if value == &defaults[stage][field] {
                            continue;
                        }
                        let mut isolated = defaults.clone();
                        isolated[stage][field] = value.clone();
                        if pipeline_cpu::validate_settings(&serde_json::from_value(isolated)?)
                            .is_err()
                        {
                            *counts.entry(format!("reason/{stage}/{field}")).or_default() += 1;
                        }
                    }
                }
                if original_checked.white_balance.mode
                    == engine_api::recipe::settings::WhiteBalanceMode::Auto
                {
                    *counts
                        .entry("reason/white_balance/mode".into())
                        .or_default() += 1;
                }
                for field in proxy.render_plan(&image.recipe.settings, false).1 {
                    *counts.entry(format!("info{field}")).or_default() += 1;
                }
                Ok(())
            },
        ))?;
        Ok(json!(counts))
    };
    match run() {
        Ok(value) => println!("{value}"),
        Err(()) => panic!("aggregate proxy audit failed"),
    }
}

struct Lr13Frames(std::sync::mpsc::Sender<Option<crate::develop::FrameInfo>>);
impl crate::develop::DevelopListener for Lr13Frames {
    fn frame_ready(&self, frame: crate::develop::FrameInfo) {
        if frame.is_final {
            let _ = self.0.send(Some(frame));
        }
    }
    fn render_failed(&self, _: String) {
        let _ = self.0.send(None);
    }
    fn saved(&self, _: String) {}
}

#[test]
#[ignore = "LR-13 private real Develop/IOSurface 200 sample and 12 pairs"]
fn lr13_app_develop_sample_from_env() {
    std::panic::set_hook(Box::new(|_| {}));
    let run = || -> SafeResult<Value> {
        let _quiet = Quiet::new()?;
        let resume = std::env::var_os("TESSERA_LR13_RESUME").is_some();
        let app = scratch_directory("TESSERA_APP_DIR", !resume)?;
        let contact = scratch_directory("TESSERA_LR13_CONTACT", false)?;
        let catalog = PathBuf::from(std::env::var_os("TESSERA_LRCAT_PROFILE").ok_or(())?);
        if !safe(catalog.canonicalize())?.starts_with("/private/tmp") {
            return Err(());
        }
        let bundle = PathBuf::from(std::env::var_os("TESSERA_LR13_STANDARD_PREVIEWS").ok_or(())?);
        let db_dir = app.join("reference Previews.lrdata");
        if !resume {
            safe(std::fs::create_dir(&db_dir))?;
            safe(std::fs::copy(
                bundle.join("previews.db"),
                db_dir.join("previews.db"),
            ))?;
        } else if !db_dir.join("previews.db").is_file() || !app.join("index.sqlite").is_file() {
            return Err(());
        }
        let mut previews = safe(import_lrcat::previews::PreviewIndex::open(
            &app.join("reference.lrcat"),
        ))?
        .ok_or(())?;
        previews.dir = bundle;
        let engine = safe(Engine::open(app.to_string_lossy().into()))?;
        let import = safe(engine.clone().open_lrcat(catalog.to_string_lossy().into()))?;
        let mut options = safe(import.default_options())?;
        options.library_folder = app.join("library").to_string_lossy().into();
        options.import_smart_previews = true;
        options.copy_proxies = false;
        for (n, root) in options.relocations.iter_mut().enumerate() {
            root.to = app
                .join(format!("absent-originals-{n}"))
                .to_string_lossy()
                .into();
        }
        let resolved = safe(resolve(&import.plan, &options))?;
        for row in &resolved {
            Sidecar::register_read_only_store(&row.path, &app);
            let paths = Sidecar::paths(&row.path);
            if !paths.recipe.starts_with(&app) || !paths.xmp.starts_with(&app) {
                return Err(());
            }
        }
        safe(std::fs::write(
            contact.join("progress.json"),
            b"{\"phase\":\"import\"}",
        ))?;
        if !resume {
            safe(import.apply(options, None))?;
        }
        let rows = safe(engine.list_images(crate::ImageQuery {
            limit: u32::MAX,
            ..Default::default()
        }))?;
        let imported = rows.len();
        let by_path: BTreeMap<_, _> = rows
            .into_iter()
            .map(|r| (PathBuf::from(&r.path), r.id))
            .collect();
        let mut proxies: Vec<_> = resolved
            .iter()
            .filter(|r| r.outcome == Outcome::OfflineProxy && by_path.contains_key(&r.path))
            .collect();
        proxies.sort_by_key(|r| import.plan.images[r.index].catalog_id);
        let comparable: Vec<_> = proxies
            .iter()
            .copied()
            .filter(|r| {
                previews
                    .has_preview(import.plan.images[r.index].catalog_id)
                    .unwrap_or(false)
            })
            .collect();
        if proxies.len()
            != resolved
                .iter()
                .filter(|r| r.outcome == Outcome::OfflineProxy)
                .count()
            || proxies.len() < 200
            || comparable.len() < 12
        {
            return Err(());
        }
        let mut selected = BTreeMap::<i64, (usize, Option<usize>)>::new();
        for (n, row) in proxies
            .iter()
            .step_by(proxies.len() / 200)
            .take(200)
            .enumerate()
        {
            selected.insert(import.plan.images[row.index].catalog_id, (n, None));
        }
        let previous = PathBuf::from(std::env::var_os("TESSERA_LR13_PREVIOUS_CONTACT").ok_or(())?);
        for n in 0..12 {
            let reference = safe(std::fs::read(
                previous.join(format!("{:02}-lightroom.jpg", n + 1)),
            ))?;
            let center = n * (comparable.len() / 12);
            let row = comparable[center.saturating_sub(12)..(center + 13).min(comparable.len())]
                .iter()
                .find(|r| {
                    previews
                        .jpeg(import.plan.images[r.index].catalog_id, u32::MAX)
                        .ok()
                        .flatten()
                        .as_deref()
                        == Some(reference.as_slice())
                })
                .ok_or(())?;
            selected
                .entry(import.plan.images[row.index].catalog_id)
                .or_insert((200 + n, None))
                .1 = Some(n);
        }
        let mut successful = 0;
        let mut failed = 0;
        let mut pairs = 0;
        let mut routes = BTreeMap::<String, usize>::new();
        let only_sample = std::env::var("TESSERA_LR13_ONLY_SAMPLE")
            .ok()
            .and_then(|s| s.parse::<usize>().ok());
        let work: Vec<_> = proxies
            .iter()
            .filter_map(|row| {
                let catalog_id = import.plan.images[row.index].catalog_id;
                let &(n, pair) = selected.get(&catalog_id)?;
                if only_sample.is_some_and(|only| n != only) {
                    return None;
                }
                Some((catalog_id, n, pair, by_path[&row.path].clone()))
            })
            .collect();
        let selected_count = work.len();
        let work = std::sync::Mutex::new(work.into_iter());
        let workers = std::env::var("TESSERA_LR13_WORKERS")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(4)
            .clamp(1, 4);
        std::thread::scope(|scope| -> SafeResult<()> {
            let (send, receive) = std::sync::mpsc::channel();
            for _ in 0..workers {
                let send = send.clone();
                let work = &work;
                let engine = &engine;
                let previews = &previews;
                let contact = &contact;
                scope.spawn(move || {
                    loop {
                        let Some((catalog_id, n, pair, image_id)) = work.lock().unwrap().next()
                        else {
                            break;
                        };
                        let mut stage = "develop_open";
                        let mut pair_done = false;
                        let mut routes = BTreeMap::<String, usize>::new();
                        let result = (|| -> SafeResult<()> {
                            let session =
                                safe(engine.clone().open_develop_session(image_id.clone()))?;
                            let plan = session.plan_surface(8192, 8192);
                            let surface = safe(crate::surface::Surface::create_rgba8(
                                plan.width,
                                plan.height,
                            ))?;
                            let (send, receive) = std::sync::mpsc::channel();
                            session.set_listener(Some(Arc::new(Lr13Frames(send))));
                            safe(session.attach_surface(surface.id(), plan.width, plan.height))?;
                            stage = "develop_frame";
                            let rendered =
                                safe(receive.recv_timeout(std::time::Duration::from_secs(180)))?;
                            let Some(frame) = rendered else {
                                let _ = session.close();
                                return Err(());
                            };
                            stage = "histogram";
                            let histogram = safe(session.get_histogram())?;
                            if !histogram.red.iter().any(|v| *v != 0) {
                                let _ = session.close();
                                return Err(());
                            }
                            let pixels = safe(surface.with_pixels(|bytes, stride| {
                                image::RgbImage::from_fn(frame.width, frame.height, |x, y| {
                                    let offset = y as usize * stride + x as usize * 4;
                                    image::Rgb([
                                        bytes[offset],
                                        bytes[offset + 1],
                                        bytes[offset + 2],
                                    ])
                                })
                            }))?;
                            stage = "save_pixels";
                            safe(pixels.save(contact.join(format!("sample-{n:03}.png"))))?;
                            if let Some(pair) = pair {
                                let reference =
                                    safe(previews.jpeg(catalog_id, u32::MAX))?.ok_or(())?;
                                safe(std::fs::write(
                                    contact.join(format!("{:02}-lightroom.jpg", pair + 1)),
                                    reference,
                                ))?;
                                safe(
                                    pixels
                                        .save(contact.join(format!("{:02}-tessera.png", pair + 1))),
                                )?;
                                pair_done = true;
                            }
                            session.detach_surfaces();
                            session.set_listener(None);
                            safe(session.close())?;
                            *routes.entry("develop".into()).or_default() += 1;
                            if n < 200 {
                                for size in [256, 2048] {
                                    stage = if size == 256 { "thumbnail" } else { "loupe" };
                                    let started = std::time::Instant::now();
                                    loop {
                                        let response = safe(
                                            engine.clone().embedded_preview(image_id.clone(), size),
                                        )?;
                                        if response.bytes.is_some() {
                                            break;
                                        }
                                        if !response.pending || started.elapsed().as_secs() >= 180 {
                                            return Err(());
                                        }
                                        std::thread::sleep(std::time::Duration::from_millis(20));
                                    }
                                    *routes.entry(format!("preview_{size}")).or_default() += 1;
                                }
                                stage = "analysis";
                                safe(engine.analyze_image(
                                    image_id.clone(),
                                    crate::AnalysisOptions {
                                        quality: true,
                                        faces: false,
                                        force: true,
                                    },
                                ))?;
                                *routes.entry("analysis".into()).or_default() += 1;
                                stage = "export";
                                let options = json!({
                                    "destination": contact.join("exports"),
                                    "format": "png",
                                    "metadata": "none",
                                    "naming": format!("sample-{n:03}")
                                })
                                .to_string();
                                let target = crate::ExportTarget::Images {
                                    image_ids: vec![image_id.clone()],
                                };
                                let exported =
                                    safe(engine.export_batch(target, options, None, None))?;
                                if exported.exported != 1 || exported.failed != 0 {
                                    for error in
                                        exported.items.iter().filter_map(|i| i.error.as_deref())
                                    {
                                        let error = error.to_ascii_lowercase();
                                        for class in [
                                            "settings",
                                            "unsupported",
                                            "profile",
                                            "lens",
                                            "geometry",
                                            "homography",
                                            "tone",
                                            "white balance",
                                            "crop",
                                            "invalid argument",
                                            "not found",
                                            "permission",
                                            "overlap",
                                            "metadata",
                                            "lock",
                                            "busy",
                                            "retouch",
                                            "mask",
                                            "revision",
                                            "history",
                                            "memory",
                                            "upscale",
                                            "finite",
                                            "width",
                                            "height",
                                            "icc",
                                            "dimension",
                                            "active area",
                                            "skew",
                                            "cancel",
                                            "orientation",
                                            "output",
                                            "encode",
                                            "export",
                                            "dependency",
                                        ] {
                                            if error.contains(class) {
                                                *routes
                                                    .entry(format!("error_class/{class}"))
                                                    .or_default() += 1;
                                            }
                                        }
                                    }
                                    return Err(());
                                }
                                *routes.entry("export".into()).or_default() += 1;
                            }
                            Ok(())
                        })();

                        if send
                            .send((result.is_ok(), pair_done, routes, stage))
                            .is_err()
                        {
                            break;
                        }
                    }
                });
            }
            drop(send);
            for (ok, pair_done, completed, stage) in receive {
                if ok {
                    successful += 1;
                } else {
                    failed += 1;
                    *routes.entry(format!("failure/{stage}")).or_default() += 1;
                }
                pairs += usize::from(pair_done);
                for (route, count) in completed {
                    *routes.entry(route).or_default() += count;
                }
                safe(std::fs::write(
                    contact.join("progress.json"),
                    json!({"successful":successful,"failed":failed,"pairs":pairs,"routes":routes})
                        .to_string(),
                ))?;
            }
            Ok(())
        })?;
        drop(import);
        drop(engine);
        let result = json!({"imported":imported,"resumed_scratch":resume,"workers":workers,"selected":selected_count,"successful":successful,"failed":failed,"pairs":pairs,"routes":routes});
        safe(std::fs::write(
            contact.join("sample-aggregate.json"),
            result.to_string(),
        ))?;
        if failed == 0 && only_sample.is_none() {
            safe(std::fs::remove_dir_all(&app))?;
        }
        Ok(result)
    };
    match run() {
        Ok(value) => {
            println!("{value}");
            assert_eq!(value["failed"], 0);
        }
        Err(()) => panic!("private Develop sample failed"),
    }
}
