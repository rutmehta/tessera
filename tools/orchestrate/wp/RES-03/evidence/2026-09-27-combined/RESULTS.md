# Combined masked pass and cancellation bridge — 2026-09-27

This isolated candidate is merge commit `1ef74c577e45d88e4801923475984bcf82dc8675` on `codex/resource-integration`, with parents main `0d62702381e2286b902de5d914f8a28ba554603b` (the accepted compositor cancellation bridge) and masked-pass branch `4f5f0f496a4bc658d7abcf48fed4c79615a7f91b`. It is not a main merge. RES-03's original RED/GREEN evidence remains in the sibling `2026-09-27` directory; this directory records the combined source and gates.

The only merge conflict was `crates/compositor/src/render/smart_filters.rs`: imports, the private transform-cancellation mapping test, and entry/exit of the mask blend section. The resolution retains the semantic masked key, frame-scoped masked result and separate charge under the existing combined pass allowance, while retaining the caller token through stage evaluation and exact `EngineError::Cancelled` mapping. Digest scans check cancellation between stored tiles. Mask composition checks cancellation between output tiles and before masked-result publication; its reservation releases on error. Digest-byte and mask-pixel counters record completed tile work if a later tile is canceled. No B Document/FFI or resident GPU behavior was edited.

The masked key contains the unmasked cache key (child identity/revision, filter parameters, input context) and a digest of mask settings, raster dimensions/depth/default, stored tile coordinates/layout/format, and sample bytes. It uses semantic content rather than layer IDs or temporary object addresses. Invalid mask geometry, density, feather, and samples still return errors; invalid results are never committed. A dense mask is still scanned on every pass lookup, even when the masked result is reused, so this integration does not claim that all repeated mask work is eliminated. The pass allowance charges retained unmasked source/result and masked output separately under one `retained_bytes`/`entries` limit; it is not a global working-memory or GPU cap.

`candidate-source` and `candidate-manifest.json` preserve exact relevant source bytes, including `Cargo.lock`. All five hashes matched the checkout before gates and after strict lint. Every command below has raw stdout/stderr and JSON exit metadata alongside this report. Every gate used `CARGO_BUILD_JOBS=2`, `RAYON_NUM_THREADS=2`, and `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/main`, with bounded subprocess timeouts; none timed out.

| Test command | Result |
|---|---:|
| `cargo test -p compositor --test smart_filter_deadlock` | 10 passed, 0 failed |
| `cargo test -p compositor --test transform_cancellation` | 4 passed, 0 failed |
| `cargo test -p compositor --lib transform_cancelled_is_not_reported_as_invalid_input` | 1 passed, 0 failed |
| `cargo test -p compositor --test smart_filters` | 3 passed, 0 failed |
| `cargo test -p compositor --test transform_content` | 1 passed, 0 failed |
| `cargo test -p compositor --test transforms` | 5 passed, 0 failed |
| `cargo test -p compositor --test layer_styles` | 5 passed, 0 failed |
| `cargo test -p compositor --test live_style_damage` | 2 passed, 0 failed |
| `cargo test -p compositor --test live_frame` | 2 passed, 0 failed |
| `cargo test -p compositor --test live_render -- --skip resident_live_geometry_and_masks_match_cpu` | 15 passed, 0 failed; 1 GPU-requesting test filtered |

Total: 48 passed, 0 failed. `cargo clippy -p compositor --all-targets -- -D warnings` exited 0. Vendor LibRaw C/C++ warnings appear in raw logs; they are unrelated to strict Rust lint. `cargo fmt -p compositor -- --check` and `git diff --check` passed. This gate does not include a GPU runtime test, large stress test, or mid-mask cancellation timing assertion; the new cancellation boundaries are source-reviewed and the separate focused cancellation tests passed on the integrated source.
