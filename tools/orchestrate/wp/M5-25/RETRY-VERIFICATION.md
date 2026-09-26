# M5-25 retry verification

The existing implementation was inspected, not represented as newly completed work. No production code was changed in this retry, and no commits were made.

## Executed in this retry

Retained `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M5-25` and ran:

    cargo test -p filters -p compositor -p pipeline-gpu --release && cargo clippy -p filters -p compositor -p pipeline-gpu --all-targets -- -D warnings && cargo fmt --check

Exit status: 0. Parsed test summaries: 388 passed, 0 failed, 26 ignored. Full output is in `retry-gate.log`. Existing LibRaw C++ compiler warnings are not the acceptance failure. `git diff --check` also exited 0.

## Confirmed acceptance gaps

- `crates/filters/src/camera_raw_gpu.rs:28` checks a restricted resident capability set. Default automatic lens/CA settings cannot take the resident path. Lens blur is rejected.
- `crates/pipeline-cpu/src/lens_plan.rs:294` rejects defringe, Upright, orientation, constrain-crop and other nonportable cases. The current resident RGB optics helper delegates to that planner rather than adding those capabilities.
- `crates/filters/src/camera_raw.rs:153` invokes `pipeline_cpu::render_linear_scaled`, not image-core's memoised Renderer. The compositor output cache does not substitute for Develop-stage memoisation.
- `crates/image-core/src/rgb.rs:11` has private pixels and only a file-opening constructor. `RawImage::from_rgb` in `source.rs:75` requires an already constructed RgbSource. A direct, safe, no-file-roundtrip integration needs permission to add a validated in-memory constructor in `crates/image-core/src/rgb.rs`, plus corresponding image-core tests. These paths are outside the current allow-list.

The constructor permission would unblock direct renderer integration, not by itself close the GPU work. Full resident lens/geometry implementation and full-chain 24MP coverage still remain. No guards were removed, and no CPU fallback is being claimed as GPU completion.

`kanban_show()` returned that no task ID/environment binding exists, so there was no board task available to block. This file records the precise unresolved state instead.

RESULT: FAIL full resident Develop coverage and internal stage memoisation remain incomplete despite the passing gate.
