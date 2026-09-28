# Stage C native lease validation receipt

Final source checkpoint: `593730668d6bcc5f65b8ea88b2febc999393e1f1` on `codex/develop-exclusive-admission`, derived from current integration base `f627c4a071b7e4a35c2a114917cb726ab793d458` plus already integrated UI tests/docs. This receipt packages test evidence only; root owns main integration.

## Final result

- Original tests-first REDs at `d61512ed`: second editor opened instead of conflicting (intended assertion, direct exit 101); direct recipe replacement succeeded during Develop (intended assertion, direct exit 101). Foreign-disk OwnerBaseline control passed (direct exit 0). All three logs and full tracked Rust/Cargo input freezes are under `red-second-editor-d61512ed/`, `red-direct-setter-d61512ed/`, and `control-foreign-disk-d61512ed/`.
- Final targeted admission/lifetime/authority groups on `59373066` all passed: second editor; direct setter; callback-final-Arc worker Drop; close-in-flight; failed snapshot/decode; failed-close repair retention; stale/wrong-gate/wrong-key authority; monotonic ID exhaustion; and `recipe_write_tests::` (10 tests). Individual logs and per-run 1,177-input before/after hashes are in `runtime-59373066/`.
- Final adjacent controls passed for failed close retention, retained Arc after successful close, save-listener reentrancy, concurrent close retry, selection during edit, legacy RGB, foreign sidecar baseline, nested unknown-owner fail-closed behavior, temporary/read-only histogram coexistence, cached process validation, and missing-model cache behavior.
- `cargo fmt --all -- --check` passed. `cargo clippy -p tessera-ffi --lib --tests -- -D warnings` passed after the single let-chain lint correction in `59373066`.
- Full serial `cargo test -p tessera-ffi --lib -- --test-threads=1` passed 137/137.

## Preserved nonfinal attempts

`compile-harness-failure-4f26ef95/` preserves a compile-only missing `std::thread` import error (direct 101; zero tests ran). It is not a behavioral failure. `runtime-096873b2/` retains the pre-lint-correction focused/adjacent logs, the command that selected zero tests, and the initial strict-clippy `collapsible_if` failure. These do not replace the final 59373066 results. The move/use test issue at `59210c1a` was caught in source preflight and never run.

## Environment and frozen inputs

- Host: macOS 15.6, Apple Silicon (`aarch64-apple-darwin`); `rustc 1.98.1`, `cargo 1.98.1`.
- Target: `/Volumes/betterSSD/tessera-cache/target/depth-histogram-readonly-77eb68d0-relocated`; `CARGO_BUILD_JOBS=2`; Rust tests serialized with `--test-threads=1`.
- Each recorded runtime invocation includes its exact command, raw stdout/stderr, direct process exit, before/after HEAD, and before/after SHA-256 manifest. Each manifest has 1,177 tracked Rust/Cargo/toolchain inputs; all final-run source hash comparisons exit 0. The baseline manifest path list is preserved in each `source-hashes-before.txt`.
- `reviews/` preserves the independent plan, source checkpoint, test-impact, and UI readiness reviews. `reviews/review-source-hashes.txt` records the original `/tmp` note SHA-256 values before copying them here.

## Scope boundary

The lease protects participating in-process Engine recipe replacement, selection policy, and Develop save/repair for the existing exact `destination_key` behavior. It does not normalize case-variant recipe filenames. Root confirmed this macOS volume resolves `.edits/photo.json` and `.edits/PHOTO.json` to the same physical file while the current key preserves different spellings; therefore this candidate does not claim physical-file exclusivity across case aliases. Agent/Cull/import/merge or external filesystem writers also remain outside the contract. There is no filesystem CAS or multi-file transaction. Unsupported nested Develop owner members still fail closed. The read-only histogram path acquires no writer lease.
