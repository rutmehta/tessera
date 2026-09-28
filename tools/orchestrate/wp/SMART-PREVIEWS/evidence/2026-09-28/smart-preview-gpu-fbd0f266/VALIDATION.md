# Calibrated Smart Preview Metal candidate

Candidate **fbd0f266443091af45ca973f06a470b84c988383**, based on c1f9d4e0. Fourteen explicit Rust implementation/test files; no Swift source or public UniFFI signature changes. Original remains the editing-source default. Explicitly selected Smart Previews now use their own measured CPU/Metal calibration, with the existing CPU override and device fallback. The experimental environment gate is removed in this candidate.

All candidate gates passed: native qualification, actual Swift focused69, full Swift39 and strict product40. Source and archive inputs are immutable and verified; main integration remains with the root task.

## Scope and limitations

GPU support is limited to the qualified camera-linear tail with no geometry map. Captured distortion, crop/rotation, transform/upright maps, and other unsupported resident tails use the existing CPU reference path on every edit. Both capability selection and dispatch check this before a GPU batch. A recipe can return to GPU after its map is removed. The backend label describes selection/capability, not per-frame execution. Original full-quality export and dirty-journal synchronization guards remain in force.

A small shared Original/proxy remap fix enables the upright flag only for a nonidentity homography, matching the scalar predicate. Existing Original geometry/lens tests pass. No exact-coordinate map cache or divergent renderer was added; no numerical tolerance was loosened. Earlier accelerated mapped-geometry failures are retained, and those exact fixtures now prove explicit CPU fallback.

HDR recipe presentation fields are preserved. Only temporary copies used for immutable-prefix validation, SDR thumbnail rendering and pre-display SDR calibration clear hdr/headroom. General scalar/legacy HDR rejection contracts are unchanged. Actual EDR float-surface delivery, above-SDR-white highlights and saved/reopened HDR policy are tested. Calibration still uses its SDR sink before host headroom is known; EDR route capability and output were measured separately on this Apple M4.

## Actual Engine/IOSurface qualification

Run28 is the final gate-absent source. All13 fresh processes passed: two SDR runs per forced OriginalGPU/proxyCPU/proxyGPU route, SDR auto for both sources, and five EDR forced/auto routes. Total312 measured frames. ForcedGPU and automatically selected Metal produce actual resident receipts, new submissions and zero timed GPU pixel readback. CPU override produces no resident receipts. Both auto routes selected Metal on this fixture/device.

Post-run pixel comparison passed63/72 same-proxy-dimension pairs (SDR42, EDR21). Nine adaptive-resolution pairs are explicitly excluded from equivalent-output ratios. Maximum normalized CPU/GPU proxy pixel difference: SDR0.003860294, EDR0.001953125. These meet unchanged numerical gates; they are not a broad perceptual-equivalence claim. Original and proxy source dimensions differ and their spatial loss is recorded separately.

Warm edits, same proxy render dimensions; eight SDR samples and four EDR samples per requested viewport:

| Viewport | SDR CPU/GPU median delivery ms | SDR paired speedup median | EDR CPU/GPU median delivery ms | EDR paired speedup median |
|---|---:|---:|---:|---:|
|640×426|134.20 /10.93|16.77×|131.22 /14.46|11.25×|
|1280×852|167.57 /2.32|71.70×|153.95 /2.55|60.72×|
|320×213|127.79 /5.13|24.74×|128.62 /4.90|26.27×|

These are small bounded samples of listener/surface delivery latency, not physical input-to-screen presentation or latency-tail statistics. Pixel inspection and viewport normalization occur after timing. Fresh process does not imply cold filesystem/driver caches. Original Metal often remains faster while rendering different/higher resolution, so no equivalent-quality speedup over Original is claimed.

Opening cost is material: final SDR proxy-auto625ms, proxyCPU42ms, forcedproxyGPU74ms; first-surface delivery4.2/138.3/26.8ms respectively. AutoEDR open791ms. Calibration repeats per proxy session; no speculative decision-cache optimization is included.

Fifteen bounded repeated open/edit/close cycles cover CPU override and auto routes. Adding geometry changes to CPU with no GPU submissions; resetting it restores GPU for Metal selection. After clearing instrumentation ownership and dropping sessions/listeners/rings, Weak Shared/Renderer/GPU-operator references and owned IOSurfaces must release within5seconds. Observed checks completed about0.011–0.02ms. This proves scoped object release, not a global-memory/RSS bound.

## Gates and provenance

-24: pipeline-cpu generation + invariants/output16passed, including unchanged legacyHDR rejection and exact prefix/presentation-policy separation.
-25: thumbnail14passed, including actual offline HDR edit/flush/close, unchanged SDR thumbnail, exact journal/history preservation and unsupported proofing rejection.
-28: actual default-route SDR/EDR13processes/312frames plus15releasecycles; original hashes/source freezes unchanged.
-29: GPU/proxy11 + sharedOriginalgeometry/lens10passed, no GPU skips.
-30: image-core18passed, including legacyHDR rejection.
-31: FFI189passed, one explicit qualification test ignored here and executed separately in28.
-32: gate-absent real RAW public workflow passed: clean+dirty offline restart/edit/save, cached Library, reconnect/conflict, unchanged original-copy export and edited JPEG4920×3276. Fixture BLAKE3 unchanged.
-33: strict release Clippy pipeline-cpu/image-core/pipeline-gpu/tessera-ffi alltargets passed.
-34: formatting failure only the testfixture is_multiple_of layout; preserved.35 formatting passed;36 affectedGPU11passed on finalbytes.
-37: archive/bindings regeneration passed. Only ignored nativearchive changed; generated Swift/header/modulemap are byte-identical. Archive SHA256456758e564b4d73cbd8a9dbd79cbc54eacbe1000e2536f1e659138306ee9fe38.
-38: actual Swift offline/thumbnail plus focusedUI/routing/queue69passed, no skips/failures, matching rebuilt archive and frozen inputs.
-39: full Swift689XCTest/1existingopt-in20kfixture skip/0failures plus5SwiftTesting passed; all tracked inputs and matching archive frozen.
-40: strict Swift product build with complete concurrency checking and warnings-as-errors passed; source/archive freeze equal. Final executable/testbinary/archive/fixture hashes recorded.

CANDIDATE-SOURCES.json verifies all14 source bytes against immutable Git blobs. Final36 matches all14. Earlier28/31/32/33 differ only in final test whitespace; product bytes are identical. Independent root verification additionally compares the full tracked input set. Run37 source_equal=false is expected and precisely limited to the rebuilt archive, documented in37-generation-changes.json.

Earlier opt-in evidence17(SDR) and26(EDR) is retained separately from final gate-absent28. Failures01–07,09–11,16,19,22–23 and34 remain preserved with commands/logs/input hashes. Runs22/23 are intended HDR regression RED checks. Diagnostic10 shader source is retained in evidence outside shipped tests. No ignored diagnostic is counted as a GPU success. LibRaw's existing vendor C++ deprecation warnings remain logged; Rust strictness is unrelaxed.

All builds use CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/depth-histogram-readonly-77eb68d0-relocated, MACOSX_DEPLOYMENT_TARGET=15.0 and CARGO_BUILD_JOBS=2, with exclusive runtime ownership. Only disposable copies of the Sony fixture are edited. Immutable source SHA256bf4c6d21136aa4fd626212fe72b962b6404e3fca45cdc3b6afbed8e73fee2cf8.

Portable evidence includes final source snapshots/patch, reviews, commands, failures, before/after source manifests, output hashes and derived comparisons. Roughly8GB of raw rendered pixels remain at the BetterSSD evidence root with a per-file SHA256 manifest; they are not duplicated into the portable metadata archive. No main merge or new GUI activation is performed by this lane.
