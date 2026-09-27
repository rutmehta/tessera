# Transform cancellation kernel evidence — 2026-09-27

Scope: product commit `610ca001abfdac65887237f67e208a8fcc9b12e9` on branch `codex/transform-cancellation`, based on `cf675cba66036bc4463f543edcb2721db6f47946` (`origin/main` when this isolated branch began). Product changes are limited to `crates/transform` and its `Cargo.lock` dependency entry. This does **not** include the compositor cancellation bridge or B-owned Document/FFI call sites.

`source-snapshots/initial` holds exact test-first RED source and the relevant original `origin/main` files. `source-snapshots/red2` holds exact second RED source. `source-snapshots/final` holds the six changed source files exactly as tested in the final gate. `MANIFEST.json` records SHA-256 and byte length for every source and evidence file; the source bytes remained identical before and after the final GREEN run.

| Gate | Exact command | Environment | Exit | Result |
|---|---|---|---:|---|
| Initial public RED | `cargo test -p transform --test cancellation` | `CARGO_BUILD_JOBS=2`; `RAYON_NUM_THREADS` unset | 101 | Expected compile failure: absent public cancellable methods and `Error::Cancelled`; no test executed, so Rayon workers were unused. |
| Initial inner RED | `cargo test -p transform --lib seam_search_checks_cancellation_during_its_inner_work` | `CARGO_BUILD_JOBS=2 RAYON_NUM_THREADS=2` | 101 | Expected compile failure: absent private seam checkpoint and `Error::Cancelled`. |
| Initial focused GREEN | `cargo test -p transform --test cancellation`; then `--lib seam_search_checks_cancellation_during_its_inner_work` | Both worker variables set to 2 | 0, 0 | Four public and one private test passed. |
| Initial crate GREEN | `cargo test -p transform` | Both worker variables set to 2 | 0 | 64 tests passed; zero doc tests. |
| RED2 after additional tests | `cargo test -p transform --lib` | Both worker variables set to 2 | 101 | Expected compile failure: missing private `TransformOp::apply_checked` and phase-aware seam checkpoint. |
| Final crate GREEN | `cargo test -p transform` | Both worker variables set to 2 | 0 | 66 tests passed; zero doc tests, no warnings or timeout. |

Every command ran with a process-group timeout (180–240 seconds); no timeout occurred. Raw combined stdout/stderr and per-command JSON exit metadata are alongside this file. `cargo fmt -p transform -- --check` and `git diff --check` passed after the final source changes. Existing seam pixel fixtures still pass. The two additional tests use 4×4 and 1025×1 inputs to observe cancellation after the width seam and after 1,024 non-seam pixels respectively, without sleeps or background-thread timing.

The implementation adds `TransformOp::apply_with_cancel`, `seam::apply_with_cancel`, and `seam::apply_with_skin_protection_and_cancel`, retaining all old entrypoints through fresh noncancelled tokens. It adds `transform::Error::Cancelled` and checks the supplied `engine_api::jobs::CancellationToken` within seam search/copy/resample loops and non-seam Rayon mapping. Validation, transform preparation, and compositor/FFI propagation remain separate follow-up work; this gate does not claim those paths are cancellable during their inner work.
