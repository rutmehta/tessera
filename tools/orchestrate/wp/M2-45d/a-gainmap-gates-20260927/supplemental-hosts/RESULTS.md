# A supplemental and host gain-map gates

Source and command manifests are stored alongside the full logs. Every run used `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M2-45d`, `CARGO_BUILD_JOBS=2`, and `MACOSX_DEPLOYMENT_TARGET=15.0`. The original ImageIO four-stop gate remains 4 passed / 1 failed and unchanged; this report does not claim full acceptance.

| Run | Exit | Outcome |
|---|---:|---|
| Initial supplemental | 101 | Objective-C `NSNumber` serialized `gain_present`/`valid` as JSON `1`; Rust expected JSON `true`, so the test stopped at the first 2-stop readback. |
| Initial combined hosts | 101 | CLI 1/1 passed; FFI 0/1 failed before encode because pre-index sidecar had no image ID; Cargo stopped before MCP. |
| Repaired supplemental | 0 | 1/1 passed. Default and explicit Core Image software readback peaks were 2.0000005, 4.000001, and 16.000004 for 1/2/4 stops, with dark/bright and missing-association controls. |
| Repaired FFI | 0 | 1/1 passed after setting the recipe through FFI following catalog indexing. |
| Independent MCP | 0 | 1/1 passed, including incompatible request rejection. |

## Exact commands and provenance

- `supplemental` exit 101: `cargo test --locked -p export --release --test gain_map supplemental_coreimage_software_reconstructs_1_2_4_stops_and_requires_association -- --exact --nocapture --test-threads=1`. Source: `/tmp/tessera-gainmap-a-headroom/pre-host-source-manifest.json`. Log: `/tmp/tessera-gainmap-a-supplemental-hosts-20260927/supplemental.log`.
- `hosts` exit 101: `cargo test --locked -p tessera-cli -p tessera-ffi -p tessera-mcp --release --test gain_map_hosts -- --nocapture --test-threads=1`. Source: `/tmp/tessera-gainmap-a-headroom/pre-host-source-manifest.json`. Log: `/tmp/tessera-gainmap-a-supplemental-hosts-20260927/hosts.log`.
- `supplemental-repair` exit 0: `cargo test --locked -p export --release --test gain_map supplemental_coreimage_software_reconstructs_1_2_4_stops_and_requires_association -- --exact --nocapture --test-threads=1`. Source: `/tmp/tessera-gainmap-a-supplemental-hosts-20260927/repair-source-manifest.json`. Log: `/tmp/tessera-gainmap-a-supplemental-hosts-20260927/supplemental-repair.log`.
- `ffi-repair` exit 0: `cargo test --locked -p tessera-ffi --release --test gain_map_hosts -- --nocapture --test-threads=1`. Source: `/tmp/tessera-gainmap-a-supplemental-hosts-20260927/repair-source-manifest.json`. Log: `/tmp/tessera-gainmap-a-supplemental-hosts-20260927/ffi-repair.log`.
- `mcp` exit 0: `cargo test --locked -p tessera-mcp --release --test gain_map_hosts -- --nocapture --test-threads=1`. Source: `/tmp/tessera-gainmap-a-supplemental-hosts-20260927/final-targeted-source-manifest.json`. Log: `/tmp/tessera-gainmap-a-supplemental-hosts-20260927/mcp.log`.

## Source changes made during these gates

- Import ordering only in `crates/engine-api/src/tools.rs`, before the first run; its old and new SHA-256 values are recorded in `/tmp/tessera-gainmap-a-headroom/pre-host-source-manifest.json`.
- Test-only Objective-C helper now emits JSON booleans. No pixel value, acceptance bound, or native ImageIO assertion changed.
- FFI host test now reads and sets its HDR recipe through public FFI after catalog ID assignment. A scoped `dead_code` allowance on the shared test helper avoids an unused-function warning in this integration target.
- Final source SHA-256 values are in `final-targeted-source-manifest.json`. `cargo fmt --all -- --check` and `git diff --check` both exited 0 after the test-only changes.
