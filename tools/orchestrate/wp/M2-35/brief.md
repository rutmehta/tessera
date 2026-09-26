# WP M2-35 — Three findings from the regression pass

Reproduce and fix, with a Swift or Rust test each:
1. **Mask thumbnail**: in the Masks panel, a Subject mask's thumbnail shows a tiny white spot instead of the subject-sized blob (verified on the tomato CR3). Read crates/tessera-ffi/src/masks.rs (thumbnail generation), apps/mac Masks panel thumbnail rendering. Likely a level/scale mismatch or the AI raster sampled at the wrong resolution; the thumbnail must show the mask at the thumbnail's own scale.
2. **Tether incoming strip**: a single click on an Incoming tile must select it (move the loupe/grid selection and key focus) — currently only double-click works and the decision chip on the tile doesn't refresh until later. Read apps/mac tether panel and the incremental change feed (M2-28); apply decision updates to the strip immediately from the change feed.
3. **Interval stepper**: the frame-count stepper/field keeps 10 when 5 is typed; make the field commit on Return/blur and clamp.
Run the relevant ACCEPTANCE steps (57, 123, 125) yourself with the test camera and scratch data (`--app-dir`, `--fake-tether`), capturing only the app window. Theme lint must stay green.
`cargo test -p tessera-ffi --release && cargo clippy -p tessera-ffi --all-targets -- -D warnings && cargo fmt --check && (cd apps/mac && ./build-ffi.sh && swift build && swift test -c release -Xswiftc -enable-testing)`. Allowed: crates/tessera-ffi/src/masks.rs, crates/tessera-ffi/src/tether.rs, crates/tessera-ffi/tests/**, apps/mac/**, tools/orchestrate/wp/M2-35/**.
