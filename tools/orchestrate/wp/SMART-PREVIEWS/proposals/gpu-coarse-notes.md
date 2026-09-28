# Coarse resident Smart Preview candidate (source only)

## Artifacts and ordering

Apply `/tmp/tessera-smart-preview-gpu.patch`, then `/tmp/tessera-smart-preview-gpu-extra-tests.patch`, then `/tmp/tessera-smart-preview-gpu-coarse.patch`. Exact pre/post files for the final patch are `/tmp/tessera-smart-preview-coarse/{base,new}`. The coarse patch changes only image-core/src/resident_render.rs, image-core/src/smart_preview_render.rs, and pipeline-gpu/tests/smart_preview.rs.

FFI: apply prior `/tmp/tessera-smart-preview-gpu-ffi.patch`, then `/tmp/tessera-smart-preview-gpu-ffi-coarse.patch`. The second FFI patch changes the opt-in route's calibration level to the same <=2048 default screen rule as Develop, removes the intermediate L0-only drag residency condition, and updates its backend description. Original source preference, original backend OnceLock, export and journal guards remain unchanged. The opt-in TESSERA_SMART_PREVIEW_GPU=1 is retained pending qualification.

All changes are temporary files. No shared source mutation, compilation, tests or performance runs were performed. rustfmt parsed the files; core patch git apply --check passes against its exact stacked base. Source review is underway. Do not infer speed or numeric correctness until native tests run.

## Shared architecture

`develop_resident_level` is extracted from the existing `run_resident_level` body through its geometry/display stage and returns the resident tile plus actual post-geometry extent. A source equality check confirmed the extracted operations are byte-for-byte identical after replacing the owned Box's `&mut *batch` reborrow with the helper's borrowed `batch`. Existing original `run_resident_level` calls it with the same output and retains all metrics/cache/finish logic. Original tiled rendering is untouched.

`run_camera_linear_coarse` is called only for proxy levels >0. It admits only backends supporting the complete padded L0 frame; otherwise returns None to the existing scalar reference. It invokes the shared helper at L0 with SceneLinear output, so no display transform runs yet. After captured tail geometry, it crops the required regular L0 tiles (including odd edges) from the resident result, reuses ResidentBatch::resample box accumulation at the requested level, runs display_op on reduced linear pixels, and finishes directly to tiles or IOSurface/histogram. Cancellation is checked before each crop/reduction and finish. All numerical pixels remain resident until an explicit CPU tile consumer asks for readback.

L0 upload/WB/Detail keys retain their existing representation/render-ID/recipe domain separation. Coarse reduction introduces no memoized buffers: it cannot alias L0 or retain stale tone/geometry results. Adaptive level changes reuse the same L0 checkpoints and recompute the requested reduction. No input-domain downsampling and no encoded-pixel downsampling occur. Generic level-independent proxy residency now requires that the whole L0 tail fits, preventing a tile-only backend being advertised as supporting adaptive coarse frames.

## Candidate tests (all unrun)

- New 1029x131 source with odd cropped dimensions and multiple source/output tiles. Loop levels 0,1,2,3,1,0,2 with exposure, WB, geometry/crop, presence edits; compare SceneLinear, encoded Display and DisplayLinear to scalar with existing qualified tolerances; partial coarse right-region comparison; 0 and64MiB cache budgets; camera upload must persist across edits/level switches when cached.
- New matching-size RGBA8 and RGBA16F IOSurface checks at1,2,3,0,1 including captured geometry, signed/HDR fixture and exposure; pixel parity to scalar, histogram counts, actual GPU submissions, no pixel readback, cancellation.
- Preserved original L0 positive IOSurface comparison and extra Guided/HDR tests. Updated obsolete tests so supported coarse tails must submit GPU work; unsupported embedded tails/manual locals remain CPU and original/export guards remain tested.
- These sequences simulate adaptive levels at renderer level. They do NOT yet prove the FFI listener/adaptive controller uses the path. Actual public Engine attach_surfaces tests/benchmarks at L0/L1/L2 remain required.

## Risks and next gate

This is bounded to the whole-level supported proxy domain. It adds GPU crop copies of the linear frame before box reduction; no additional CPU pixel copy is introduced, but GPU transient memory/dispatch overhead must be measured. Small levels still pay full L0 nonlinear tail cost, as required for scalar-equivalent semantics. The expected improvement comes from reusable GPU source/WB/Detail checkpoints and fused downstream operations, not from changing rendering quality. Do not claim faster editing until matched originalMetal and proxyGPU viewport listener measurements pass.

The FFI qualification patch calibrates at its default screen level before the actual viewport is attached, just as original calibration uses a fixed level. Final UI acceptance must cover actual viewport sizes and adaptive transitions, HDR/SDR, CPU override, unsupported-tail fallback, cache pressure and original/proxy opening order. Original remains the default regardless of these results.
