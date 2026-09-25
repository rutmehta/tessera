# M2-15 implementation and verification

Implemented shared ICC registry, generated built-ins, macOS display-profile discovery, intent/BPC transforms, soft proofing and gamut warnings, shared 33-cubed LUT caching, explicit managed CPU rendering, shared export conversion, and actual GPU LUT compute with managed region rendering.

Engine API unchanged. Required schema fields and context-only configuration are documented in crates/pipeline-cpu/MISSING_FIELDS.md and OUTPUT_M2.md.

## Retry changes and verification

- Reproduced the geometry failure. The test predated CPU Upright/manual-transform support. Updated the assertions to preserve CPU success coverage for Auto/scale, CPU invalid-guide validation, and the crop-only GPU kernel's explicit unsupported errors. No geometry production behavior changed.
- Replaced file export's sRGB8 intermediate with the managed float render directly into the selected document profile. Codec now quantizes already-converted pixels, avoiding double conversion. A TIFF16 regression failed before this change and passed afterward, checking saturated wide-gamut pixels and sub-8-bit differences against the CPU float reference.
- Added display-id lookup and required sRGB fallback. Switched CoreGraphics/CoreFoundation objects to objc2 types with retained ownership. Kept a nullable binding for CopyColorSpace because the generated wrapper assumes non-null. Verified unavailable display fallback; active-list membership is necessary because this OS can substitute the main-monitor profile for sentinel IDs.

Ran the exact required chain after the final code changes with CARGO_TARGET_DIR=/Users/rutmehta/.cache/tessera-target/M2-15:

`cargo test -p color-mgmt -p pipeline-cpu -p pipeline-gpu -p export --release && cargo clippy -p color-mgmt -p pipeline-cpu -p pipeline-gpu -p export --all-targets -- -D warnings && cargo fmt --check`

Exit 0. Full output is in `tools/orchestrate/wp/M2-15/verification.log`. Vendor LibRaw C++ warnings remain, but Rust clippy with -D warnings passed. Existing ignored tests remain ignored. `git diff --check` also passed. All modifications are inside the allowed paths; engine-api is unchanged. No commit was made.

## GPU integration completed in this retry

- Added `GpuManagedOutput`, using the shared CPU output resolver to validate proof identity and display/export policy. WGSL now applies the same luminance sigmoid and gamut policy, the cached ICC/proof LUT, and destination transfer encoding. The old `GpuOutputLut` remains explicitly documented as a raw primitive, not the managed output stage.
- Added `Registry::linearized_rgb` for matrix/shaper displays. Signed linear-destination LUTs plus sampled transfer curves avoid clipping negative ICC node channels and interpolating across transfer knees. CLUT profiles are not stripped or treated as matrix profiles.
- Added separate monitor/proof GPU mask buffers, based on round-trip DeltaE before gamut mapping. The warning LUT has an extended working-RGB domain; tetrahedral interpolation avoids false warnings on neutral shadows.
- Added `ManagedRenderer` with immutable output selection and isolated caches. It replaces batched and resident Output, including the IOSurface presentation path, without modifying image-core or engine-api. A real IOSurface test verifies exact tile/surface pixel agreement, one submission, and no extra pixel readbacks.
- Added CPU-reference gates for five built-in destinations, both gamut modes, proof on/off, saturated/negative/highlight colors, a whole Bayer frame, and a synthetic small-gamut printer with paper simulation. A debug regression reproduced and fixed oversized-layout arithmetic overflow.

Final required command returned exit 0: 190 tests passed, 0 failed, 7 existing tests ignored; clippy `-D warnings` and workspace `cargo fmt --check` passed. `CARGO_TARGET_DIR` remained `/Users/rutmehta/.cache/tessera-target/M2-15`. Full output is in `verification.log` alongside this file.

## Precision and API boundaries

Legacy default-sRGB entry points remain unchanged; callers select the explicit managed APIs. GPU preview pixels and warning boundaries approximate the CMM; the gates use 0.025 absolute RGB for managed samples and 0.03 for the complete resident/U8 frame. Exact float export and exact warning masks use the CPU CMM. The preview warning lattice clamps outside [-0.5,3.5], and these tests do not establish a universal error bound over arbitrary CLUT profiles or extreme signed RGB.

Resident presentation retains the existing scene-operator eligibility rules and does not paint gamut-warning overlays. The float/mask convenience region API still performs readbacks; use `ManagedRenderer::render_to_surface` or `GpuManagedOutput::encode` for resident output. HDR, calibration UI, proof-copy history and independent ink simulation controls are outside this SDR engine implementation. Required engine-api fields are reported in `crates/pipeline-cpu/MISSING_FIELDS.md`; engine-api itself is unchanged.

RESULT: PASS
