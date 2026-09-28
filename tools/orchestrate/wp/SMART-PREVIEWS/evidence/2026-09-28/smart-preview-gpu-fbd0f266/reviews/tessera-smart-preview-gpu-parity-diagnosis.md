# Independent source diagnosis: first Metal parity failures

Read-only inspection; no build, source mutation, or tolerance change. Failure evidence: /Volumes/betterSSD/tessera-validation/smart-previews/gpu-integration/01-smart-preview-gpu.log. Runtime diagnostics are owned by FFI.

## Concrete defect

crates/pipeline-gpu/src/lens.rs remap_band constructs flags with unconditional bit 8 (upright). In crates/pipeline-gpu/src/lens.wgsl remap, flags & 11 forces conversion from pixel coordinates into normalized f32 coordinates and back, including an identity homography operation. This occurs even for crop/straighten without active lens or transform. CPU geometry_effects.rs only performs normalized mapping for active transform/lens/upright; MapPlan::source in lens_plan.rs has the same conditional rule. Therefore the GPU adds a precision-losing roundtrip absent from the reference.

Proposed narrow repair: set upright flag iff plan.upright != lens::Homography::IDENTITY. Retain all existing numerical tolerances and add a crop-only signed-HDR regression. The existing failing coarse test supplies such coverage. This is a source-proven semantic difference; whether it fully explains the recorded numeric failure needs FFI runtime qualification.

FFI run02 context: first coarse failure hdr=true, budget0, step4, level1, SceneLinear, immediately after crop left .07/right .91 and angle3. Earlier HDR steps0–3 and all non-HDR steps passed. Difference -565.2051 versus -566.3008 is consistent with high-gradient/cancellation sensitivity to coordinate error.

## Compact remains unresolved

FFI run03 context: first Compact failure is level0 DisplayLinear(headroom4); SceneLinear and SDR comparisons pass. It is not evidence of coarse reduction failure. Compact has actual captured lens distortion and cannot simply skip normalized mapping. Shader lens map is f32; scalar lens map is f64 before final f32 coordinates. Shader header explicitly documents this approximation. Display transform may amplify an upstream sub-tolerance difference.

Discriminating next check sent to FFI: apply pipeline_cpu::display_linear to the exact GPU SceneLinear tiles, compare that result with GPU DisplayLinear, then inspect CPU/GPU scene RGB at the failed display sample. This separates display-stage error from amplification of upstream error without loosening tolerance.

If exact-coordinate mismatch is established, existing public MapPlan::source(x,y,iw,ih) is the reference mapping API. A bounded cached coordinate upload using this API is a possible precise GPU-sampling boundary (pixel processing remains GPU), but requires further design and measurement; do not implement blindly. Lanczos accumulation/contraction still requires independent check if equal coordinates fail.

## Coarse ordering check

image-core resident_render.rs run_camera_linear_coarse calls develop_resident_level(SceneLinear), including remap, then batch.resample, then display output. This matches scalar render.rs geometry followed by downsample_crop. For tested levels1–3, zero crop origin and 256px source-tile boundaries align with power-of-two box bins, so the existing per-tile partial sum path does not split these bins. There is no source evidence that changing reduction order is the correct fix.

## After runtime06: bounded repair assessment

FFI reports identity-bit repair improves HDR difference to20 versus19.980469 but does not fully pass. Compact CPU display transform applied to GPU SceneLinear passes GPU DisplayLinear comparison, establishing that its failure is upstream error amplified by display (not display implementation itself). Exact coordinates are therefore a justified next experiment, not yet a proven complete fix.

Recommended additive proxy-only boundary: ResidentBatch exact-map capability/method; image-core selects it only for camera-linear geometry; GPU uploads two F32 coordinate planes from MapPlan::source and samples those in a dedicated kernel entry. Existing Original/export remap remains intact. Default unsupported capability must cause honest scalar fallback before resident work rather than claiming support then failing mid-render.

Coordinate allocation:8 bytes per output pixel, SonyCompact1640x1092=14,327,040 bytes (~13.66MiB), worst Compact2048 square32MiB, Detail2560 square50MiB. Temporary host coordinates coexist with GPU copy; this is not zero-cost. Build with checked dimensions/device binding limits and row-level cancellation. Represent None as[-1,-1], outside valid>=-0.5, and test validity before integer/index conversion.

Use existing GPU LRU via exact two-channel ResidentTile: upload supports arbitrary channel counts, cache_exact retains F32, actual storage bytes are already charged by cache_payload_bytes. Never use cache(), which packs F16. Key uses dedicated domain, immutable proxy/render owner identity, geometry/lens settings and frame/output sizes. Exclude exposure/WB/output mode/level so ordinary edits reuse coordinates. Do not use chained Geometry stage hash, which incorporates all upstream edits. Cache-budget0/oversize rejection must regenerate exact same map and preserve parity. No unbounded secondary map cache.

Qualification required: exact map cold/warm/budget0/eviction, crop/lens/transform invalidation, exposure/WB mapreuse, invalid-edge sentinel, signedHDR and Compact original failing samples, no pixel readback on surfaces. Measure generation/upload cost and warmed edit latency; MapPlan::source recomputes per-pixel crop trigonometry and optional lens operations, so do not assume cheap cold builds. Compiler may hoist but that is not evidence.

Residual risk: even exact coordinates do not guarantee same Lanczos weights/sums. WGSL sin and contraction differ from scalar f32. First run existing unchanged tolerances with exactcoordinates. If HDR residual remains, isolate sampling with identical input/coordinates, then use explicit rounded products/additions and test known cancellation cases. Do not hide remaining error by broad tolerance relaxation or quantizing HDR.

## Source review of explicit mapped-geometry fallback candidate

Reviewed applied source-only after author diagnosis09/10, before runtime qualification. Both camera_linear_resident_supported and try_camera_linear_resident reject tail.map.is_some() before batch creation, preserving CPU geometry at every level. Develop recomputes can_render_resident each edit, and surface dispatch funnels through the same gate. Original/export route guards remain separate. Captured-map fixtures and original numerical tolerances remain in the suite; explicit CPU submission assertions replace their unsupported GPU claim. New positively named unmapped fixtures separately exercise GPU rendering, cache reuse, SDR/EDR surfaces and geometry edit/reset transitions across levels0–3. No skipped numerical regression is presented as GPU qualification. The removed research diagnostic remains preserved externally.

One requested correction: the static backend label currently appends “GPU with CPU fallback” even when backend selection returns CPU. Author notified to use a backend-neutral qualifier such as “mapped geometry uses CPU” or conditional label. This avoids a CPU-selected session falsely claiming GPU selection. No other actionable source defect found in the fallback change. Runtime suite and actual Engine route measurements remain required. New transition test proves surface receipt/submissions/no readback and CPU exact tile parity; existing dedicated positive surface tests supply actual surface-byte parity coverage.

Final label follow-up inspected: lib.rs now appends GPU-with-CPU-fallback only for the existing backend.rs Metal (...) name; CPU selections remain CPU-labelled. The disclosure finding is resolved. Source review clear; no runtime acceptance inferred.
