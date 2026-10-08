//! Cached ML model weights. No weights are checked in and CI does not fetch
//! them, so tests that run real models are skipped by default.
//!
//! A skip is never silent: it prints an uncaptured `SKIPPED` line naming the
//! test, and with `TESSERA_REQUIRE_MODEL_WEIGHTS` set it fails instead (set it
//! where the weight caches are populated, never in CI unless CI fetches them).

use std::io::Write;

/// Set (to any value) to turn absent model weights into a failure.
pub const REQUIRE_ENV: &str = "TESSERA_REQUIRE_MODEL_WEIGHTS";

/// The variable the face-model tests honoured before [`REQUIRE_ENV`]; still
/// accepted with the same meaning.
pub const LEGACY_REQUIRE_ENV: &str = "TESSERA_REQUIRE_MODELS";

/// Whether absent weights must fail rather than skip.
pub fn required() -> bool {
    std::env::var_os(REQUIRE_ENV).is_some() || std::env::var_os(LEGACY_REQUIRE_ENV).is_some()
}

/// Reports that `test` (or the part `reason` names) did not run because model
/// weights are absent. Written to the real stderr so the line shows in plain
/// `cargo test` output. Panics when [`required`].
pub fn skipped(test: &str, reason: &str) {
    assert!(
        !required(),
        "{test}: {reason} ({REQUIRE_ENV} is set; populate the model cache)"
    );
    let _ = writeln!(
        std::io::stderr().lock(),
        "test {test} ... SKIPPED: {reason}; model weights absent, not checked \
         (set {REQUIRE_ENV}=1 to make this a failure)"
    );
}
