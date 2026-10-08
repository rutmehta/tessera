//! The real RAW fixtures under `fixtures/raw` (see `fixtures/fetch.sh`).
//!
//! Tests never pass silently when the fixtures are absent. Without them a test
//! prints an uncaptured `SKIPPED` line naming itself (libtest has no runtime
//! ignore), and with `TESSERA_REQUIRE_RAW_FIXTURES` set it fails instead.
//!
//! The default location is resolved from this crate's manifest, so a worktree
//! whose `fixtures/raw` is a symlink to another checkout's fixtures works.

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
    let files = scan(&root);
    if files.is_empty() {
        skipped(test, &format!("no RAW fixtures in {}", root.display()));
    }
    files
}

/// Every RAW file directly in `root` (symlinks followed), sorted by path.
fn scan(root: &Path) -> Vec<PathBuf> {
    let mut files: Vec<_> = match std::fs::read_dir(root) {
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

/// The fixture named `file` (for example `"sample.dng"`). `None` (after
/// [`skipped`]) when it is absent.
pub fn file(test: &str, file: &str) -> Option<PathBuf> {
    let path = root().join(file);
    if path.is_file() {
        Some(path)
    } else {
        skipped(test, &format!("no {file} fixture in {}", root().display()));
        None
    }
}

/// Every fixture in `names`, in that order. `None` (after [`skipped`]) when
/// any is absent.
pub fn files(test: &str, names: &[&str]) -> Option<Vec<PathBuf>> {
    let root = root();
    let missing: Vec<_> = names
        .iter()
        .filter(|n| !root.join(n).is_file())
        .copied()
        .collect();
    if missing.is_empty() {
        Some(names.iter().map(|n| root.join(n)).collect())
    } else {
        skipped(
            test,
            &format!("{} missing from {}", missing.join(", "), root.display()),
        );
        None
    }
}

/// The first fixture (by path) whose extension is `ext`, case insensitive.
/// `None` (after [`skipped`]) when there is none.
pub fn with_extension(test: &str, ext: &str) -> Option<PathBuf> {
    let root = root();
    let found = scan(&root)
        .into_iter()
        .find(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext)));
    if found.is_none() {
        skipped(test, &format!("no .{ext} fixture in {}", root.display()));
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_follows_a_symlinked_fixture_directory() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        std::fs::create_dir(&real).unwrap();
        for name in ["b.NEF", "a.dng", "notes.txt"] {
            std::fs::write(real.join(name), b"x").unwrap();
        }
        let link = dir.path().join("raw");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let names: Vec<_> = scan(&link).iter().map(|p| name(p)).collect();
        assert_eq!(names, ["a.dng", "b.NEF"]);
        assert!(scan(&dir.path().join("absent")).is_empty());
    }

    /// The default location is this repository's `fixtures/raw`. In a
    /// worktree that is usually a symlink to the main checkout's fixtures;
    /// this checks the path, not whether the fixtures are present.
    #[test]
    fn default_root_is_the_repository_fixtures_directory() {
        let expected = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw");
        assert!(expected.parent().unwrap().join("fetch.sh").is_file());
        if std::env::var_os("PIPELINE_RAW_FIXTURES").is_none()
            && std::env::var_os("RAW_DECODE_FIXTURES").is_none()
        {
            assert_eq!(root(), expected);
        }
    }

    #[test]
    fn current_test_is_the_libtest_thread_name() {
        assert_eq!(
            crate::current_test(),
            "raw::tests::current_test_is_the_libtest_thread_name"
        );
    }
}
