# M2-21 implementation and verification

Status: incomplete / FAIL acceptance. The latest required test command fails in the FFI preview suite; default-recipe GPU execution and performance acceptance also remain unresolved.

## Latest verification attempt

This attempt inspected the inherited implementation without changing its source code. It ran the exact required command with `CARGO_TARGET_DIR=/Users/rutmehta/.cache/tessera-target/M2-21`. The release export and image-core suites passed, but `tessera-ffi` test `slow_interactive_frames_are_not_starved` failed at `crates/tessera-ffi/tests/develop.rs:182`: `no frame; failures: []`. The chained command exited 101 before reaching clippy or fmt.

The same preview test was then run alone with `cargo test -p tessera-ffi --release --test develop slow_interactive_frames_are_not_starved -- --exact` and failed again after 121.25 seconds. Log: `current-preview-check.log`. This reproduces the failure independently of the export tests, but does not establish its root cause or prove it predates the implementation. The preview test and develop implementation are outside this work package's permitted paths and were not changed.

Clippy was run separately with the required arguments and exited 0 (`current-clippy.log`). `cargo fmt --check` and `git diff --check` also passed. Native LibRaw warnings are not the failing gate. Earlier successful runs and benchmark numbers below are historical evidence, not results rerun by this attempt. No new benchmark or performance success is claimed.

RESULT: FAIL required release suite times out in FFI preview; default-recipe GPU execution and performance acceptance remain incomplete.

## Implemented

- Resident float managed output through image-core Renderer and pipeline-gpu, retaining the target profile LUT instead of display quantization.
- Horizontal export regions with renderer halo dependencies, GPU separable Lanczos-3 downsampling, and one pixel readback per completed band.
- Global GPU render admission (one renderer at a time), scratch allocation guard, and per-band renderer lifetimes.
- Owned RenderedExport and bounded render/encode overlap in batch and FFI exports, preserving transactional publication and cancellation.
- Explicit TESSERA_EXPORT_BACKEND=cpu fallback. Other values attempt GPU and retain reference fallback for unsupported requests. RenderedExport::used_gpu reports the actual path.
- Synthetic CPU/GPU, whole/band, resampling, float/readback, lens fallback, cancellation and ICC/XMP tests, plus opt-in five-fixture benchmarks and precision gate.

## Retry fix: retire sensor uploads without a pixel readback

The initial real-fixture precision test failed because the transaction accumulated fresh queue-upload buffers across every sensor dependency. Queue writes cannot safely reuse storage inside an unsubmitted encoder, even when the planner has dropped that tile. A smaller horizontal output band alone did not solve that accumulation.

ResidentBatch now exposes a default no-op checkpoint. After a sensor dependency chunk retires, export transactions with at least 128 MiB of tracked scratch submit their pending commands, wait for completion, and release retired buffers. Live intermediate images stay resident. This does not add a pixel readback, does not publish unfinished cache entries, and leaves the 512 MiB scratch guard in place. Non-export backends retain their existing scheduling.

The previously failing five-fixture full-chain precision gate now passes. A non-ignored pipeline-gpu regression test exercises multiple submissions with one pixel readback and tracked scratch below 512 MiB.

## CPU delegation and remaining limitations

Lens corrections are not implemented by the resident graph. Default recipes enable Auto lens correction and CA removal; these fall back rather than silently omit effects. Disabling those settings is an explicit recipe edit, never an automatic export change. All default-recipe benchmark requests still use CPU fallback.

Local adjustments, AI mask rendering/segmentation hooks, geometry, denoise, Adobe processing, RGB sources and unsupported resident requests retain the existing CPU/reference path. AI hooks remain intact. Presence operations requiring whole-frame statistics also fall back when bands are necessary. Upscaling remains on the existing path. Output orientation, output sharpening, metadata construction and encoding remain CPU work.

The scratch guard accounts for resident payload buffers and final readback, not every driver allocation or parameter buffer. No claim of a measured total-process/GPU peak below 512 MiB is made. Arbitrary image dimensions and unsupported settings can still fall back.

## Verification from this retry

The exact required chain was run successfully after the changes:

    cargo test -p export -p image-core -p tessera-ffi --release && cargo clippy -p export -p image-core -p tessera-ffi --all-targets -- -D warnings && cargo fmt --check

Exit 0, log: retry-verification-3.log. CARGO_TARGET_DIR remained /Users/rutmehta/.cache/tessera-target/M2-21. LibRaw emitted native compiler warnings. Rust clippy passed with warnings denied.

Two earlier verification attempts failed intermittently in existing FFI tests: slow_interactive_frames_are_not_starved timed out awaiting a frame, then passed in isolation; print_renders_fit_the_box_in_the_chosen_colour_handling compared ICC bytes whose timestamp seconds differed (40 versus 41). Neither out-of-scope test was changed. Both passed in the final complete run. Logs: retry-verification.log, retry-develop.log, retry-verification-final.log.

Additional commands:

    cargo test -p pipeline-gpu --release --test export_float
    PIPELINE_RAW_FIXTURES="$PWD/tools/orchestrate/wp/M2-21/raw" cargo test -p export --release --lib five_fixture_full_chain_tolerance -- --ignored --nocapture

Both passed. Logs: retry-export-float.log, retry-precision.log. The precision command was also run before the fix and failed (retry-precision-red.log).

Full-chain precision with unsupported lens stages explicitly disabled:

| Fixture | Maximum linear error | Maximum 8-bit code error |
|---|---:|---:|
| Canon CR3 | 0.000001847744 | 1 |
| Sony ARW | 0.000001758337 | 1 |
| Nikon NEF | 0.000005722046 | 1 |
| Fuji RAF | 0.000003874302 | 1 |
| DNG | 0.00000846386 | 1 |

These satisfy the tested full-chain limits of 2e-3 linear and one 8-bit code. This is not a golden-suite DeltaE2000 measurement.

## Benchmarks

benchmark-results.json contains 40 newly measured records: five fixtures, Web/full JPEG, requested CPU/GPU, default/lens-disabled recipes. Times include RAW opening/decode, render and encode in isolated processes. These are single runs, not statistical or 100-image batch measurements. The benchmark prints used_gpu; requested=gpu alone does not demonstrate GPU use.

With lens correction explicitly disabled, all ten requested-GPU cases now complete on GPU:

| Fixture | Web CPU / GPU seconds | Full CPU / GPU seconds |
|---|---:|---:|
| Canon CR3 | 8.810 / 4.788 | 9.329 / 4.844 |
| Sony ARW | 3.298 / 2.054 | 9.285 / 3.546 |
| Nikon NEF | 15.092 / 4.924 | 50.095 / 8.286 |
| Fuji RAF | 3.742 / 2.400 | 8.776 / 5.297 |
| DNG | 4.446 / 2.964 | 14.084 / 5.880 |

Default full-size Nikon remained CPU fallback: CPU 65.930 s, requested GPU/fallback 51.335 s. The variance in these single runs is not evidence of GPU acceleration. No 45 MP under 1.5 s or 100-image under 40 s success is claimed.

Reproduce:

    PIPELINE_RAW_FIXTURES="$PWD/tools/orchestrate/wp/M2-21/raw" cargo test -p export --release --test gpu_bench five_fixture_export_benchmark -- --ignored --exact --nocapture
    PIPELINE_RAW_FIXTURES="$PWD/tools/orchestrate/wp/M2-21/raw" TESSERA_BENCH_LENS_OFF=1 cargo test -p export --release --test gpu_bench five_fixture_export_benchmark -- --ignored --exact --nocapture

Logs: retry-benchmark-default.log and retry-benchmark.log. RAW fixture files and logs are ignored. Engine-api was not modified. Changes are uncommitted and confined to allowed paths.
