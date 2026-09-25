# M2-14b implementation and verification

## Implemented

- Reproduced the missing-cache-at-delivery failure deterministically by constraining the thumbnail cache to one byte. With NSCache, both the identity assertion inside the callback and the non-nil assertion after delivery failed. Replaced NSCache with an explicit cost-bounded LRU keyed by full PhotoItem identity. It retains the newest image (even if that single image exceeds the budget) and evicts older entries explicitly, not asynchronously under system pressure.
- Moved cache publication onto MainActor immediately before callback delivery. Previously storage happened on the detached decode task before the actor hop, leaving a window for eviction while the callback waited. Added concurrent delivery/eviction coverage with sixteen distinct images sharing a dense ID and a one-byte budget.
- ThumbnailLoader invalidation cancels pending requests for both tiers of the exact PhotoItem before clearing cache entries. Previously an already-running decode could repopulate an invalidated cache and deliver stale pixels. A new regression failed before this change and passed afterward.
- Preview cancellation tests drain the specific requests rather than sleeping. BridgeTests explicitly subscribes to the matching engine/image/tier PreviewReady callback. Dense-ID tests assert both independent deliveries and cached image identity. Existing assertions were not relaxed.
- Added shared mask-ai segmentation backend, model loader, orientation/request mapping, raster resampling and composition. Export uses the crate; FFI shares its source module because editing tessera-ffi/Cargo.toml is outside the allowed paths.
- Ordinary and enhanced exports install per-export image-core MaskHooks. Subject + local exposure is covered with a fake segmenter and assertions on decoded exported pixels versus an unmasked export. Additional tests cover orientation, mixed composition and inference failures.

## Verification actually run by parent

The exact required command exited 0:

    cargo test -p export -p tessera-ffi -p image-core --release && cargo clippy -p export -p tessera-ffi -p image-core --all-targets -- -D warnings && cargo fmt --check && (cd apps/mac && ./build-ffi.sh && swift build && swift test)

Full output: verification.log. Existing third-party C compiler warnings are present; clippy completed successfully.

This retry reran the exact command above successfully, then reran stress.py: ten complete swift test suites under RUST_TEST_THREADS=1 cargo test --workspace --release. All ten Swift runs and the Rust workspace suite exited 0. Each Swift run passed 37 XCTest tests and 5 Swift Testing tests. stress-results.json records process start/end times; verified programmatically that every Swift run was fully contained within the Rust workspace command's lifetime. Logs: swift-load-1.log through swift-load-10.log and rust-load-1.log. The harness now waits for the Rust subprocess to start before starting Swift.

CARGO_TARGET_DIR remained /Users/rutmehta/.cache/tessera-target/M2-14b. MACOSX_DEPLOYMENT_TARGET=15.0 and CARGO_INCREMENTAL=0 were used for final verification. git diff --check passed. No commit was made, and tracked edits/new source files are within the user's allowed paths.

## Unresolved / limitations

The historical failures were not replayed under their original machine conditions. The new cache-pressure regression does reproduce the same cache-miss symptom with the previous NSCache implementation and passes with the explicit cache. The original tests retain their assertions and await the matching callbacks. Stress success is evidence of stability, not a proof against every possible interleaving.

AI masks combined with lens warps explicitly return an error. The public pipeline-cpu entry point exposes neither a pre-lens local-mask callback nor its private warp operator, and changing pipeline-cpu is outside the allowed paths. Export does not silently ignore those corrections or publish a misaligned mask. Unsupported AI kinds also return explicit errors. Real segmentation/SR weights were not required for the fake-backend export regression.

RESULT: PASS
