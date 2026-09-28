# Camera-linear GPU optimization boundary (source-only)

Audited after image-core commit 6c057641. No builds, source edits, format/tier changes or FFI changes performed for this audit.

## Recommendation

Implement a narrowly admitted proxy-L0 resident input path into the existing scheduler. Reuse current Metal kernels and exact resident caches. Do not merely reduce the proxy dimensions, reinterpret camera RGB as working RGB, delete source guards, or feed the original sensor crop to proxy pixel addressing.

The measured warm edit gap (proxy CPU 384.7 ms vs original Metal 33.1 ms including readback) is principally evidence that the proxy path bypasses downstream resident execution/cache. The source confirms it copies/develops the full proxy through CPU on each request. Existing original resident code retains WB/Detail whole-level checkpoints and fuses downstream point operations; it already has the right acceleration machinery.

## Smallest viable boundary

1. pipeline-cpu/src/smart_preview.rs and lens_plan.rs: expose a validated camera-linear render plan, not the mutable captured lens internals. It should return original-calibration CameraProfile and WB matrices plus a captured **tail-only** lens plan for a separately supplied proxy pixel frame. Keep immutable original metadata intact. Profile then WB order must match render.rs CameraLinear arm. Tail CA must always be absent: sensor reconstruction, demosaic, embedded stages 1/2, profile lateral CA and manual CA were already baked. Preserve sample vignetting and common geometric map after Effects. No resolve_lens/resolve_lens_sensor calls.

   Existing ResolvedLens::plan_impl is not directly usable: it rejects manual CA/embedded data that is already baked and can produce a fresh CA plan. Refactor its common geometry/vignette construction into a tail helper with explicit frame dimensions, retaining present portabilty checks (e.g. defringe, unsupported geometry/Upright). Do not clear arbitrary fields on a clone and hope equivalence follows. Initially decline unsupported tails back to the current CPU route. Auto/Image/Database source snapshots with supported tail math must remain supported; restricting to a neutral lens would miss the default use case.

2. image-core/src/render.rs: add explicit resolution/addressing of camera-linear input. Physical sensor metadata remains original; resident execution frame/crop is [0,0,proxy_width,proxy_height], period 1, no sensor halos/CFA chain. Derive matrices from retained cam_xyz/as_shot_wb, not fabricated neutral metadata. Prefer an explicit input-kind/frame on Resolved over making the proxy masquerade as RgbSource (which means linear Rec.2020).

3. image-core/src/resident_render.rs::balanced_tiles: add a camera-linear branch **before** sensor resample/source-gather paths. Obtain immutable proxy camera-RGB tiles, batch.upload and batch.cache_exact them under representation-separated prefix keys; then reuse existing profile matrix, WB matrix, lens_gain sequence. Never execute highlight/demosaic/CA again. Downstream run_resident_level/develop_tiles already implement real-neighbor Detail, tone/local-tone, color, Effects, geometry remap, output and finish. Their exact WB/Detail caches should invalidate normally on WB/tone/detail changes while source upload remains reusable.

   Important precision trap: upload_cached calls cache(), whose non-Decode GPU implementation rounds to f16 even on cold/cache-rejected paths. Use upload + cache_exact for this camera-linear checkpoint. Otherwise codec F32 fallback/HDR samples can overflow or incur an extra unqualified quantization. Domain-separate source/cache keys from CFA and working-RGB, bind to immutable render/container identity plus generator/prefix semantics; identical recipe owner does not mean identical render pixels.

4. image-core/src/smart_preview_render.rs and resident_render.rs: replace only the blanket refusal for a **qualified L0 resident route**; preserve existing prefix/process validation, unsupported-mask/depth/retouch checks, cancellation and fallback. Route tiles and IOSurface through the same plan and backend capability test. Keep export-row/original-quality export guards: GPU view support is not export-source authorization. RGB layer APIs must still decline camera-linear input.

5. Existing backend reuse: image-core/src/resident.rs already offers upload, cache_exact, matrix run_chain, lens_gain, remap, whole-level gather/crop and surface finish. pipeline-gpu/src/resident.rs implements them and exact f32 cache; pipeline-gpu/src/batch.rs exposes counters. No new WGSL is expected for the initial admitted L0 subset. Later new functionality must not be inferred from this statement.

## Critical order boundary: coarse levels

The current scalar CameraLinear contract renders detail/tone/effects/common geometry at proxy L0 and THEN uses integer area reduction. Original resident rendering instead reduces camera pixels before its downstream tail. Those are not interchangeable for nonlinear operations, grain, neighborhood detail, or odd crop/rotation. Therefore start resident acceleration at proxy level 0 only. Keep coarse progressive levels on CPU until an explicit post-tail reduction step (before display encoding), with exact partial-bin and geometry extent behavior, is tested. Reusing existing early resample for proxy coarse levels would silently change pixels. The proposed compact tier is independently versioned source generation and does not solve this runtime-order issue.

## Qualification and cost/risk

This is a moderate cross-crate integration: roughly 4-6 production files plus image-core resident-model and pipeline-gpu integration tests; it is smaller than new GPU kernels or a new proxy representation. Highest risks are double-applied CA, proxy-vs-sensor coordinates, calibration/WB order, early nonlinear reduction, cache identity/precision, and inadvertently enabling proxy exports. Local masks remain CPU until existing resident limitations change; auto Upright needs an explicit retained/proxy-domain analysis decision and should initially fall back when not portable.

Required discriminating checks before preference enable:
- Image-core resident CPU model vs scalar proxy at L0: negative/HDR F32 fallback, asymmetric channels/nonidentity calibration and WB, captured Auto/Image/Database corrections, manual CA baked once, profile vignette, odd original crop and geometry. Embedded unsupported tails must visibly fall back.
- Metal cold/warm/cache-budget-zero/rejected-cache fidelity; WB edit retains exact camera checkpoint but changes WB cache, tone edit reuses WB/Detail, source/container changes cannot reuse old checkpoints. Verify counters and finite HDR intermediates.
- Tile and IOSurface paths, cancellation/no later delivery, fallback parity, and unchanged original-only export guard.
- Repeat matched-output Sony cold/warm/edit benchmark with original Metal and proxy resident, including actual surface path before app preference decisions. FFI default backend is dynamically calibrated on surface presentation; current readback benchmark does not itself identify that runtime selection.

Continue original by default and proxy explicit/offline until these measurements succeed. A 2048 tier alone is unlikely to close an 11.6x warm-edit gap and would trade detail independently of the cache/backend fix.
