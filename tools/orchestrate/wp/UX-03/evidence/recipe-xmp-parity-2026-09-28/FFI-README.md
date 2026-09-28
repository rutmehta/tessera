# Isolated FFI regeneration receipt

After the original native receipt, `bash apps/mac/build-ffi.sh` ran in the export-integration checkout against the same frozen Rust/Cargo bytes (`3bd2e341`; evidence-only HEAD `8f884101`). `CARGO_BUILD_JOBS=2`, `RAYON_NUM_THREADS=2`, and `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/main` were set. The script exited 0 directly, with no watchdog expiry, and `lipo` reported arm64. Raw output and exact command/timing are in `combined-3bd2e341/build-ffi.log` and `build-ffi.json`.

The new `libtessera_ffi.a` SHA-256 is `4a45f8235a8c6382d0dad9be735d5cd336a86c20eb50c15051b1c55654444596`. The generated Swift, C header, and modulemap are byte-identical to the tracked files, so this behavior change did not alter the FFI ABI or checked-in bindings. `combined-3bd2e341/ffi-before.json`, `ffi-after.json`, and `ffi-archive-provenance.json` record exact before/after hashes and postbuild source identity. All 15 inputs in the original combined source manifest still match, and the checkout was clean after build.

The old export-worktree archive (SHA-256 `19f9f5487752e2588c6febe99d126905cad7d9986285d2aebf633dfaeba2b427`) was copied byte-identically to `/Volumes/betterSSD/tessera-validation/recipe-xmp-parity/historical-export-ffi-19f9/ffi` before building. Other worktrees' historical `8ab43f64...` archives were untouched. A coherent new archive/bindings copy is stored at `/Volumes/betterSSD/tessera-validation/recipe-xmp-parity/ffi-4a45f823/ffi` with its own `provenance.json`; the large archive is intentionally not committed to Git. No Swift app or GUI gate is claimed.

The prior 31-payload native receipt and its SHA manifest remain unchanged. `FFI-SHA256SUMS` covers only the new FFI receipt files.
