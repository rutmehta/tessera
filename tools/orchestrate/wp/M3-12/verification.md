# M3-12 verification

Implemented ImageCaptureCore tether backend, serial live ingest/scoring queue,
UniFFI engine methods/listener, and CLI list/start/capture. Engine-api unchanged.

## Executed checks

- Required chain exited 0: `cargo test -p tether -p tessera-ffi -p tessera-cli --release && cargo clippy -p tether -p tessera-ffi -p tessera-cli --all-targets -- -D warnings && cargo fmt --check`.
- Release suite summaries: 117 passed, 0 failed, 6 ignored, plus harness-free main-thread discovery.
- Separate opt-in `cargo test -p tether --release --test face_models -- --ignored`: 1 passed. Real hash-pinned YuNet/SFace inference, persisted faces_analyzed and quality scores before event delivery. No weights committed.
- `TESSERA_EXPECT_NO_CAMERA=1 cargo test -p tether --release --test discovery`: passed, 0 cameras.
- Built CLI `tessera --json tether list`: `[]`, exit 0.
- Objective-C bridge compiled independently with `-Wall -Wextra -Werror`; policy/lifetime tests passed; real SDK smoke returned 0 cameras.
- `git diff --check`: passed.

## Environment and limitations

`CARGO_TARGET_DIR` remained `/Users/rutmehta/.cache/tessera-target/M3-12`.
The full passing chain used `CI=1`, `CARGO_PROFILE_DEV_DEBUG=0`,
`CARGO_INCREMENTAL=0`, and `CARGO_BUILD_JOBS=2`. CI mode uses the repository's
existing 120-second RAW-preview test timeout. An initial non-CI run failed the
existing `crates/tessera-ffi/tests/fallback.rs` 3-second timeout. That test and
its thresholds were not modified. Initial rebuild attempts also exhausted disk;
only this work package's external Cargo build cache was cleaned. Existing
vendored LibRaw C++ warnings remain; Rust clippy completed with `-D warnings`.

No camera was attached, so physical shutter/download behavior remains a hardware
validation item. Native calls must run on the process main thread and the FFI
host must call `tether_poll` periodically. See `crates/tether/README.md` for the
lifecycle, naming grammar, model configuration and callback contract.
