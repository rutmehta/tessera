# M3-14b retry verification

Re-ran the required command chain on the existing implementation with `CARGO_TARGET_DIR=/Users/rutmehta/.cache/tessera-target/M3-14b` retained:

    cargo test -p agent -p tessera-mcp -p image-core -p pipeline-gpu --release && cargo clippy -p agent -p tessera-mcp -p image-core -p pipeline-gpu --all-targets -- -D warnings && cargo fmt --check

Release results: 178 passed, 0 failed, 11 ignored. Clippy passed. Workspace formatting failed solely on module ordering at `apps/tessera-cli/src/main.rs:10`: rustfmt requires `mod tether;` before `mod understanding;`. Verified that file is byte-identical to HEAD. It is outside the allowed paths and was not changed. Scoped formatting for all four packages and `git diff --check` passed. No modified/untracked paths outside the allowlist.

Explicitly re-ran the ignored five-fixture benchmark. It passed, including exact full-output histogram and clipping counts, mean luminance error below 1e-4, and native face crop parity. Evidence: `retry-bench.log`.

| Fixture | Median cached reduction | Amortized complete step |
| --- | ---: | ---: |
| CR3 | 4.082750 ms | 72.260000 ms |
| ARW | 3.969958 ms | 46.516264 ms |
| NEF | 8.837833 ms | 99.895958 ms |
| RAF | 3.910959 ms | 40.995402 ms |
| DNG | 4.521834 ms | 59.932805 ms |

The NEF reduction meets the 20 ms target; the complete step does not. No source changes were needed or made during this retry. Resolving the mandatory workspace formatting gate requires permission to change the out-of-scope CLI file or an upstream formatting fix.

RESULT: FAIL required workspace cargo fmt --check fails on unchanged, out-of-scope apps/tessera-cli/src/main.rs module ordering.
