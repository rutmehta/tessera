# M2-04b final verification

RESULT: PASS

The existing worktree commit 271c423 already contains the native revision-2 colour science, active default sharpening/chroma NR, process-version cache separation, five updated goldens and enabled Swift Basic controls. This run completed the newly authorized sidecar/FFI integration. No commit or push was made in this run. The pre-existing brief edit and previous logs were preserved.

## Changes in this run

- Sidecar export regression now expects NativeRevision 2 and still verifies roundtrip and Adobe companion removal.
- FFI slider initialization calls pipeline_cpu::as_shot_temperature_tint with the calibrated camera inverse. It no longer uses McCamy, vertical-v tint, clamping or rounding for valid fixture whites. Existing fallback (5500, 0) remains for invalid/unrepresentable metadata.
- A first temperature-only or tint-only patch from AsShot seeds the omitted coordinate from the same inverse, avoiding the informational recipe-default coordinate.
- FFI renderable passes Texture, Clarity, Dehaze, Vibrance and Saturation through with finite-value validation and bounds. Unsupported controls remain ignored and reported.
- Regression tests cover the five Basic values and every RAW fixture's unrounded slider coordinates and first single-slider touch, including matrix identity within 1e-4.
- OPERATORS.md now describes the completed FFI integration rather than an out-of-scope blocker.

## Verification actually run

Exact requested chain, exit 0 (terminal process proc_5788d8da94d6):

    cargo test --workspace --release && cargo clippy -p pipeline-cpu -p pipeline-gpu -p image-core -p engine-api -p sidecar -p tessera-ffi --all-targets -- -D warnings && cargo fmt --check && (cd apps/mac && ./build-ffi.sh && swift build && swift test)

This includes the CPU/GPU reference tests, WB/default-detail regressions, full workspace tests, all six requested Clippy packages, formatting, rebuilt FFI, Swift build and Swift tests. Swift reported 14 XCTest tests and 5 Swift Testing tests, all passing. Vendored LibRaw C++ compiler warnings remain; they did not fail the requested chain.

Additional verification:

- Observed the Basic pass-through regression fail before its fix, then pass (basic-green.log).
- Observed the old slider inverse fail on ARW: (4650, 18) versus (4632.284, 3.4482048), then observed the first temperature-only touch still fail before seeding the missing coordinate (ffi-wb-red.log, ffi-wb-partial-red.log).
- The five-fixture FFI regression passes after both fixes (ffi-wb-green.log), also in the full chain.
- Regenerated all five revision-2 PNGs in this run (goldens-final.log). They are byte-identical to the existing committed revision-2 goldens; git reports no additional golden diff.
- Re-ran cargo test -p pipeline-cpu --release --test golden after regeneration: PASS, 1 passed (goldens-verify-final.log).
- git diff --check: PASS. All source/document changes stay in the allowlist. FFI generation introduced no Swift source diff.

CARGO_TARGET_DIR stayed /Users/rutmehta/.cache/tessera-target/M2-04b throughout. No repository target directory was used.
