# M2-47 current verification: round 2 PASS

All four round-two decisions are implemented. The required gate was run in this worktree and exited 0. No commits or pushes were made.

## Delivered

1. Native float32 and uint16 LinearRaw DNGs enter calibrated linear Rec.2020 RGB at the Demosaic boundary. CFA DNG remains on the raw path. Develop, grid previews, and export work. The FFI regression actually merges a bracket, receives a grid JPEG, opens a develop session, renders non-black, changes exposure, flushes/reopens the persisted settings, and exports a non-black JPEG.
2. Boundary Warp 0..100 deforms a separable ruled boundary mesh without substituting a crop. Auto projection samples angular horizontal/vertical FOV (spherical above 80 degrees vertical, otherwise cylindrical above 100 degrees horizontal, otherwise perspective). Fill Edges uses actual filters::caf through an FFI callback: a direct merge -> filters dependency would cycle through compositor. Preview warnings expose the selected projection and any uncalibrated Auto FOV estimate.
3. Raw Details remains an explicitly rejected compatibility field, with the missing Apache/MIT learned-demosaic model reason documented in ml-enhance/README.md.
4. Merge/enhance consumes EXIF orientation before processing and writes upright DNGs. All eight orientation permutations are tested through real enhancement publication and through RGB ingestion.

## Required gate: PASS

Working directory: `/Users/rutmehta/Developer/tessera/.worktrees/M2-47`

`CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M2-47` stayed set for every Cargo command. No CI or test-thread override was set for the gate.

    cargo test -p tessera-ffi -p merge -p ml-enhance -p image-core --release && cargo clippy -p tessera-ffi -p merge -p ml-enhance -p image-core --all-targets -- -D warnings && cargo fmt --check && (cd apps/mac && ./build-ffi.sh && swift build)

Evidence: `round2-gate.log`, ending `GATE_EXIT=0`. Background process `proc_550adbfc1d07` exited 0. Parsed Rust totals: 296 passed, 0 failed, 11 ignored. Model-dependent cases can explicitly skip without weights; these totals do not establish real production-weight inference. Swift/C bindings were regenerated and `swift build` completed in 23.29 seconds. Generated Swift was read back for photoMerge, mergePreview, enhance, MergeOptions, EnhanceOptions, PhotoJob, and PhotoJobListener.

Additional parent-run verification: `cargo test -p raw-decode -p previews --test linear_dng --release` passed 4 tests (`round2-ingestion.log`, `INGESTION_EXIT=0`). Focused FFI tests passed, including observed red failures for the prior orientation rejection, missing warp wiring, and LibRaw export failure before the corresponding fixes (`round2-ffi-red.log`, `round2-merge-red.log`, `round2-ffi-green.log`). The full gate also exercises the strengthened all-orientation test.

The existing native LibRaw C++ warnings and blake3 macOS 26.5-vs-15.0 linker warning remain nonfatal. `git diff --check` reports two trailing-whitespace lines emitted by UniFFI for the new Raw Details documentation; the requested formatting/build gate passes.

## Scope and caveats

Changed paths remain within the provided allow-list. No export.rs, document modules, render/resident files, or UI were edited. Image-core production changes are only source.rs/rgb.rs. Native LinearRaw parsing is bounded interchange support, not an arbitrary third-party DNG decoder. Merge preview remains an explicitly warned approximate camera-channel rendition. Real enhancement models retain their existing SDR/gamut limits. See HANDOFF.md for API contracts, Auto calibration fallback, cancellation/publication behavior, and coordinator integration hotspots.

No Kanban task ID was injected; the initial kanban_show returned the missing-task-ID error. No board lifecycle transition was possible or claimed.

RESULT: PASS
