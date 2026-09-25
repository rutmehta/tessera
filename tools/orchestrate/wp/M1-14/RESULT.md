# M1-14 verification

Implemented missing/small embedded-JPEG fallback in `previews`, with `PreviewSource::Rendered`, default settings, bilinear demosaic, and the CPU renderer's `render_scaled` entry point. Scaling happens after demosaic in linear light, not by decimating CFA phases. The JPEG pyramid key includes raw content, requested size, orientation and the default recipe hash. Edited recipe application remains future work as scoped.

FFI RAW misses return `PreviewResponse { bytes: None, pending: true }` without opening/decoding RAW. A deduplicated `Priority::Preview` job publishes the cache key before the existing `PreviewReady` callback. Ready requests return cached bytes without another callback. Failed jobs also wake waiting clients so the next request surfaces the error. One RAW worker bounds peak full-sensor memory use.

The app previously did not register an engine listener, so more than a one-line refresh was necessary: a shared listener, buffered callback subscriptions, cancellation-aware asynchronous retry, and regenerated UniFFI bindings. Swift does not render RAW or poll.

Validation run personally in this worktree with CARGO_TARGET_DIR=/Users/rutmehta/.cache/tessera-target/M1-14:

- `cargo test -p previews -p tessera-ffi --release && cargo clippy -p previews -p tessera-ffi --all-targets -- -D warnings && cargo fmt --check && (cd apps/mac && swift build)` returned exit 0.
- Rust: 8 previews tests passed, 1 pre-existing synthetic performance test ignored. FFI unit/integration tests passed, including pending/deduplication/off-thread callback/cache retrieval.
- Initial measured cold sample.dng preview: 868.6 ms, mean luminance 0.26317, standard deviation 0.24348. Release test enforces <3 s, >0.02 mean and >0.01 standard deviation.
- Sony ARW embedded-JPEG path asserted zero pipeline renders. Eighth-resolution boundary and recipe-key separation tested.
- All eight EXIF transforms tested. Corrected existing swapped orientation 5/7 mappings.
- `apps/mac/build-ffi.sh` regenerated bindings and built the arm64 archive.
- `swift test --package-path apps/mac` passed 8 XCTest tests and 5 Swift Testing tests. The real bridge test explicitly uses sample.dng and completed in 1.02 s, including cold callback-driven loading.

Non-fatal existing native build warnings remain in vendored LibRaw. Swift linking reports the cached blake3 NEON object targets macOS 26.5 while the app targets 15.0; build/tests pass on this machine, but this run does not prove compatibility on macOS 15.

No commits made. Only allowed paths changed. RESULT: PASS
