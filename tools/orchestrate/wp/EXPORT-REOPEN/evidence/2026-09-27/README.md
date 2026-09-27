# Rendered-export XMP reopen regression evidence

Product/test commit: `0473a87e4b9e3d4da36dd7e3c9a451673ad33af5`, based on `origin/main` `be48c00da5b817e9037b31b49427eec36187777a`. Worktree: `/Users/rutmehta/.codex/worktrees/render-resource-bounds/tessera`. This was a scratch-generated 16×16 RGB/JPEG/PNG test only; no user photo, catalog, app launch or GUI was used.

`red.patch` and `*.red.rs` preserve the exact pre-fix regression source. `red-source.sha256` verifies those snapshots. The focused RED command was:

```sh
CARGO_BUILD_JOBS=2 RAYON_NUM_THREADS=2 CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target cargo test -p export --test metadata_policy baked_jpeg_metadata_does_not_reapply_source_development -- --exact --nocapture
```

It exited **101**: the baked-vs-neutral JPEG pixel assertion passed, then embedded XMP `to_recipe()` returned exposure `1.0` rather than neutral `0.0` (`red.log`, `red-result.json`). The initial compiler build took 108.96 s. This is an expected regression failure, not a passing gate.

`final-source.sha256` hashes the committed product/test files and Cargo.lock. `final-results.json` records the exact subsequent commands/exits. The final frozen source passed:

| Gate | Result |
| --- | --- |
| `cargo test -p export --test metadata_policy` | 3 passed, 0 failed |
| `cargo test -p export --test hdr_output` | 1 passed, 0 failed |
| `cargo test -p export --test original` | 3 passed, 0 failed |
| `cargo test -p export --test dng` | 1 passed, 0 failed |
| `cargo test -p sidecar --test selection` | 1 passed, 0 failed |

All final commands used `CARGO_BUILD_JOBS=2`, `RAYON_NUM_THREADS=2` and the external Cargo target above, with per-command 300 s watchdogs; raw logs are alongside this file. `cargo fmt --check -p export -p sidecar` and `git diff --check` also passed. The product fix strips development XMP from the shared final packet for every baked output while retaining exact custom-mark `ts:Mark`/`ts:MarkLabel`; Original-copy export stays separate. These tests do not claim a full app build, a GUI reopen pixel comparison, or coverage of arbitrary external editors.
