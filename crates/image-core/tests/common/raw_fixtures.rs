//! The real RAW fixtures under `fixtures/raw` (see `fixtures/fetch.sh`).
//!
//! Tests never pass silently when the fixtures are absent. Without them a test
//! prints an uncaptured `SKIPPED` line naming itself (libtest has no runtime
//! ignore), and with `TESSERA_REQUIRE_RAW_FIXTURES` set it fails instead.
//! A copy of this file lives in `crates/pipeline-cpu/tests/common/`.
#![allow(dead_code)]

use std::io::Write;
use std::path::{Path, PathBuf};

/// Every fixture extension `fixtures/fetch.sh` provides.
pub const EXTENSIONS: [&str; 5] = ["arw", "cr3", "dng", "nef", "raf"];

/// Set (to any value) to turn an absent fixture set into a failure.
pub const REQUIRE_ENV: &str = "TESSERA_REQUIRE_RAW_FIXTURES";

/// `PIPELINE_RAW_FIXTURES`, then the older `RAW_DECODE_FIXTURES`, then the
/// repository's `fixtures/raw`.
pub fn root() -> PathBuf {
    std::env::var_os("PIPELINE_RAW_FIXTURES")
        .or_else(|| std::env::var_os("RAW_DECODE_FIXTURES"))
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw"))
}

/// Reports that `test` checked nothing. Written to the real stderr so the
/// line shows in plain `cargo test` output, not only on failure.
pub fn skipped(test: &str, reason: &str) {
    assert!(
        std::env::var_os(REQUIRE_ENV).is_none(),
        "{test}: {reason} ({REQUIRE_ENV} is set; run fixtures/fetch.sh)"
    );
    let _ = writeln!(
        std::io::stderr().lock(),
        "test {test} ... SKIPPED: {reason}; it checked nothing (run fixtures/fetch.sh, \
         or set {REQUIRE_ENV}=1 to make this a failure)"
    );
}

/// An uncaptured note about coverage a test could not exercise with the
/// fixtures present (not an absence, so never a failure).
pub fn notice(test: &str, message: &str) {
    let _ = writeln!(std::io::stderr().lock(), "test {test}: {message}");
}

/// Every RAW fixture present, sorted by path. Empty (after [`skipped`]) when
/// the directory is missing or holds no RAW file.
pub fn all(test: &str) -> Vec<PathBuf> {
    let root = root();
    let mut files: Vec<_> = match std::fs::read_dir(&root) {
        Ok(entries) => entries
            .map(|e| e.unwrap().path())
            .filter(|p| {
                p.is_file()
                    && p.extension().is_some_and(|e| {
                        EXTENSIONS.contains(&e.to_string_lossy().to_ascii_lowercase().as_str())
                    })
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    files.sort();
    if files.is_empty() {
        skipped(test, &format!("no RAW fixtures in {}", root.display()));
    }
    files
}

/// [`all`], narrowed for speed by a comma-separated environment variable whose
/// entries match a fixture's extension or a substring of its file name (case
/// insensitive). Unset or empty selects every fixture. A filter matching
/// nothing fails rather than checking nothing.
pub fn selected(test: &str, env: &str) -> Vec<PathBuf> {
    let files = all(test);
    let Some(filter) = std::env::var(env).ok().filter(|f| !f.trim().is_empty()) else {
        return files;
    };
    let wanted: Vec<String> = filter
        .split(',')
        .map(|w| w.trim().to_ascii_lowercase())
        .filter(|w| !w.is_empty())
        .collect();
    let chosen: Vec<_> = files
        .iter()
        .filter(|p| {
            let name = p
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_ascii_lowercase();
            let ext = p
                .extension()
                .unwrap()
                .to_string_lossy()
                .to_ascii_lowercase();
            wanted
                .iter()
                .any(|w| *w == ext || name.contains(w.as_str()))
        })
        .cloned()
        .collect();
    assert!(
        files.is_empty() || !chosen.is_empty(),
        "{test}: {env}={filter} matches none of {files:?}"
    );
    if chosen.len() < files.len() {
        let _ = writeln!(
            std::io::stderr().lock(),
            "test {test}: {env}={filter} selects {} of {} RAW fixtures",
            chosen.len(),
            files.len()
        );
    }
    chosen
}

/// The fixture's file name, for messages without absolute paths.
pub fn name(path: &Path) -> String {
    path.file_name().unwrap().to_string_lossy().into_owned()
}
