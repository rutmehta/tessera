//! Dev-only helpers for tests that depend on files the repository does not
//! carry: the real RAW fixtures under `fixtures/raw` (see `fixtures/fetch.sh`)
//! and cached ML model weights.
//!
//! Such a test never passes silently. When what it needs is absent it prints
//! an uncaptured `test <name> ... SKIPPED: <reason>` line (libtest has no
//! runtime ignore), and it fails instead when the matching variable is set:
//! [`raw::REQUIRE_ENV`] for RAW fixtures, [`models::REQUIRE_ENV`] for weights.
//!
//! Use this crate only as a `[dev-dependencies]` entry.

use std::io::Write;

pub mod models;
pub mod raw;

/// The running test's name. libtest names each test's thread after the test
/// (also with `--test-threads=1`); `"<unknown test>"` otherwise.
pub fn current_test() -> String {
    std::thread::current()
        .name()
        .filter(|n| *n != "main")
        .unwrap_or("<unknown test>")
        .to_owned()
}

/// A visible note that `test` did not run an opt-in check whose input (a
/// private sample, a training environment, ...) is never provided by the
/// repository or CI. Never a failure: no `TESSERA_REQUIRE_*` variable applies.
pub fn opt_in_skipped(test: &str, reason: &str) {
    let _ = writeln!(
        std::io::stderr().lock(),
        "test {test} ... SKIPPED (opt-in): {reason}"
    );
}
