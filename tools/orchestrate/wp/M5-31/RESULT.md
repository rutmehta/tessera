# M5-31 result

RESULT: PASS

Implemented every effect represented by the M5-14 CPU reference in the resident GPU path, with backdrop-aware style composition, native-resolution source halos, independently mipped effect planes, bounded revision/style/light/level/region caching, and GPU gradient/pattern sampling. Direct style rejection is removed; styled smart-object children no longer force CPU fallback.

## Verification

Executed with CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M5-31:

```
cargo test -p compositor --release && cargo clippy -p compositor --all-targets -- -D warnings && cargo fmt --check
```

Exit status 0. Aggregated test results: 253 passed, 0 failed, 11 ignored. The ignored 4K style benchmark was separately executed successfully. Logs: `gate.log`, `benchmark.log` in this directory. Existing vendor LibRaw C++ warnings remain; Rust Clippy passes with warnings denied.

New coverage includes exact effect-plane comparisons at four scales, L0/L2 full and cropped tile-boundary parity, interpreter/specialized paths, native PSD lfx2 import, F32/U8/U16, blend-if, fill/whole opacity, knockout, clipping, nested groups, global-light/source/mask edits, undo, unrelated-layer cache reuse, and oversized-cache non-retention. Invalid geometry is regression-tested in debug as well as release.

Independent review found a validation-order overflow for nonfinite geometry. Validation now precedes halo arithmetic; the debug regression failed before the fix and passed afterward. Final targeted review passed.

## Benchmark

3840x2160 document, 20 sparse styled layers, 256x256 L2 viewport, synchronized GPU submission:

- CPU cold tile: 48.005758583 s
- CPU unchanged warm tile: 185.875 us
- Resident cold viewport: 435.410 ms
- Resident unchanged warm viewport: 1.875 us
- Cold/warm dispatched blocks: 256 / 0

Single-run cold and idle results, not a full-L2 viewport or interactive edit-throughput claim. Pixel parity was checked by the benchmark.

## Explicit boundaries

- All M5-14 effect variants have GPU implementations. No effect-specific CPU style fallback is needed.
- Contour/jitter, bevel texture, and Adobe controls not evaluated by M5-14 remain unevaluated, matching CPU semantics.
- Styles directly on adjustment/pass-through layers remain Unsupported on both CPU and GPU. Isolate the group.
- Legacy `DocState::check_resident_effects()` was outside the allowed edit scope and remains conservative; hosts should call resident render/render_viewport directly.
- Existing unrelated unsupported smart-filter fallback remains. Resource/binding limits and the padded-alpha cap can require smaller viewports.
- No document.rs/edit.rs/format.rs/psd.rs edits, no commits, and no build output under the repository.

Implementation and numerical tolerance documentation: `crates/compositor/COMPOSITOR.md` section 9.2.
