# WP M2-07b — Make GitHub CI pass

Run https://github.com/rutmehta/tessera/actions run 36138209848 failed after 27 minutes: `crates/tessera-ffi/tests/fallback.rs:48` (`missing_jpeg_returns_pending_then_callback_and_cached_bytes`) panicked, because `ci.sh` runs `cargo test --workspace` in the debug profile and the rendered preview of `sample.dng` exceeds the test's 3 s limit on a GitHub macOS runner. Locally everything is verified with `--release`.
Fix:
1. `ci.sh`: run `cargo test --workspace --release` (keep clippy/fmt/deny as they are); keep `CARGO_TARGET_DIR` untouched.
2. `.github/workflows/ci.yml`: add `Swatinem/rust-cache@v2` (keyed on Cargo.lock), cache `fixtures/raw` with `actions/cache` keyed on `fixtures/fetch.sh`, set a 60-minute job timeout, and make the Swift job build FFI in release. Keep `actionlint` clean.
3. The fallback test: keep the 3 s assertion in release but skip the timing assertion (not the correctness assertions) when `cfg!(debug_assertions)`; add a short comment.
4. Any other test that is timing-based must follow the same rule; grep for `Duration::from_secs` / `elapsed()` in tests and apply.
Verify locally: `bash ci.sh` passes; `actionlint .github/workflows/*.yml` passes (install via brew if missing). Only touch ci.sh, .github/**, crates/tessera-ffi/tests/**, other `tests/` files with timing asserts, tools/orchestrate/wp/M2-07b/**.
