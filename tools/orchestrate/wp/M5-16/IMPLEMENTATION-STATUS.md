# M5-16 implementation and verification

Implemented without engine-api changes or commits. All changed paths match the work-package allowlist.

## Presentation

- Legacy `present` / `present_iosurface` retain the pinned document-encoded RGBA8 behavior.
- `present_profiled` / `present_iosurface_profiled` resolve the rendered document ICC profile and convert to an encoded display profile or RGBA16F linear extended sRGB/P3. Untagged documents use sRGB. Unresolved embedded-profile data and stale/foreign document revisions error.
- Device-resident LUT buffers are retained by profile/options and content. Raw mutable LUTs are content-hashed to avoid address-based stale caching. Headroom changes do not upload another LUT.
- EDR range follows min(2^stops, display headroom), HDR off = 1, capped to finite f16. Encoded ICC input is explicitly [0,1]. Extended HDR requires explicitly already-linear matrix-profile input, relative-colorimetric intent, and float output. This is color conversion and range limiting, not the develop scene tone curve. The host must tag the surface/layer appropriately.
- Cache lifetime is the shared pipeline lifetime; no automatic eviction. GPU comparisons against LCMS and direct LUT references cover alpha, offsets, headroom, profile/intent changes and invalid contracts.

## Viewports and smart objects

- Output buffers cover only the block-aligned viewport plus margin. Pans GPU-copy overlapping rows and preserve valid overlap. Full-level readback still requires full rendering.
- Smart-object child rendering is limited to the required sampling window, including filter support. Pending parent tiles retain their own child buffer handles as windows move.
- Opt-in `SmartQuality::Lanczos3` uses normalized 6x6 Lanczos-3 at output level zero and bilinear above it. Legacy bilinear remains default to preserve pinned results. Quality changes invalidate affected caches, including nested children.
- Independent CPU Lanczos references, repeatability, adjacent-tile halo, nested quality switching, huge sparse child windows, nonzero-origin presentation, panning and specialization are tested.
- Review reproduced a failed oversized full render poisoning unsubmitted mip pages. Output bounds are now checked before page materialization. A failure-to-compact-viewport regression was observed failing with transparent pixels and passing after the fix.

## Executed verification

With CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M5-16:

`cargo test -p compositor -p gpu-core -p color-mgmt --release && cargo clippy -p compositor -p gpu-core -p color-mgmt --all-targets -- -D warnings && cargo fmt --check`

Exit 0. 136 passed, 0 failed, 8 ignored. Full final output: `verification.log`. `git diff --check` passes.

Explicit ignored benchmark:

`cargo test -p compositor --release --lib benchmark_present_before_after_resident_lut -- --ignored --nocapture`

Apple M4, 1920x1080, 60 presents with wait/frame: forced-upload baseline 1.176 ms/frame, resident 0.751 ms/frame, zero redraw LUT uploads. This is a same-build upload-every-frame baseline (including content hashing), not a historical-binary measurement. Timing is informational, not a flaky assertion.

The older M5-08b full 4K recomposite performance target is not re-claimed here. This package changes presentation and memory locality, not that layer-blending throughput target.

RESULT: PASS
