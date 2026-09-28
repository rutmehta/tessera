# Smart Preview Calibration Decision Reuse Implementation Plan

> For implementation after coordinator approval: use the executing-plans skill task by task; no delegation or runtime is authorized by this document.

**Goal:** Test whether bounded Engine-local calibration decision reuse reduces validated reopen-to-first-frame latency.

**Architecture:** Cache only fixed-size selection metadata after every current source validation succeeds. Reconstruct fresh session render resources on a hit; never retain heavy image/backend ownership.

**Tech stack:** Rust, existing Engine/IOSurface qualification harness, existing Metal and CPU operators.

**Spec:** `docs/coordination/SMART-PREVIEW-LATENCY-NEXT-STEP.md`. Source inspected at main `3a395c69b6946660daf2d30a595149d5ef26e144`; accepted historical performance reference remains `fbd0f266`.

SOURCE-ONLY proposal. No implementation, builds, benchmarks, UI or GPU work performed. Root review required before coding; runtime remains assigned elsewhere. Original remains default and existing fidelity/geometry CPU fallback are immutable constraints. No speedup promised.

## Concrete current path and findings

`Engine::open_smart_preview_develop_session` → `open_develop_source` reserves ImageId admission, calls `load_smart_preview`, reconciles acknowledged journal intent, validates document, online sidecar baseline and proxy-compatible recipe, then `develop_render_resources` → `Engine::develop_renderer` → `backend::select_proxy`. The proxy branch repeats selection on every open; Original uses `Engine.renderer: OnceLock<Backend>` and is outside this change. `select_proxy` chooses first level0…4 with max image extent<=2048, clones settings only to clear HDR/headroom for calibration, then `select_at` constructs CPU/Metal operators and performs three CPU and three Metal surface renders. The resulting Backend retains warmed caches for that session. No first-ever-open shortcut is justified by current evidence.

`load_smart_preview` already validates full decoded container/source identity and, if original exists, hashes original bytes. That cost MUST remain on every reopen. It currently truncates container_digest to128bits for render ImageId and discards the full digest from its return tuple: **do not use render ImageId alone as the decision key**. JournalSnapshot has generation/recipe_digest but not incarnation; the open SmartPreviewJournal privately retains incarnation. Acquire both from the same validated load, without a second journal read or path lookup. Engine already owns one shared GpuDevice in OnceLock, with device_failure(); adapter name alone is not device identity. RendererConfig includes cache budget, threads, process version, graph and preview approximations; all influence calibration.

Existing evidence is bounded AppleM4/Sony: autoSDR625ms open versus CPU42ms/forcedGPU74ms; autoEDR791ms. First-surface after open differs because calibration warmed winner caches. Removing calibration also removes that warmup: first-surface cost may rise and offset saved open cost. Measure combined open-through-delivery, not open alone. No claim of physical display latency.

## Narrow implementation slice

1. Add crate-private `backend::ProxyDecisionCache` and test module, either in backend.rs or backend/proxy_decision_cache.rs. Engine owns `Mutex<ProxyDecisionCache>` initialized empty. Cache stores fixed-size keys/decision/timing/expiry only; never Backend, Renderer, RawImage, proxy Arc, GpuContext/StageOp, surface, recipe, PathBuf or String. No global/static cache or persistence.
2. Refactor internal validated-load result to carry a small copied `ProxyCalibrationIdentity` from the existing decode/open: owner ImageId, full256bit container digest, captured original digest+length, journal incarnation+generation+recipe_digest, dimensions/tier/container format version. This may be an internal result struct replacing the four-tuple at its existing callers. Add only crate-private copied identity getter on the already-open journal; no extra store read. Preserve actual validation order and all current open errors.
3. Extend only internal proxy resource selection arguments to pass that identity. Original calls remain unchanged. Form key only AFTER current validation/reconciliation/recipe gates succeed and after final recipe normalization used by resource creation. No new public FFI/Swift API.
4. Split backend selection into preparation, measurement and decision materialization while preserving select/select_at existing outcome policy. `SelectionOutcome` distinguishes SuccessfullyMeasured(Cpu|Metal, samples) from ExplicitOverride and FallbackError. Only successful finite positive measurements eligible for gpu_is_faster's existing comparison may enter cache; invalid timings, failed CPU/GPU calibration or unavailable operators/device never cache a successful decision.
5. On auto lookup hit, recheck current GPU device health and current renderer capability, then construct a FRESH matching selected backend/config. A Metal construction failure removes the entry and uses existing honest CPU fallback; do not invoke retained stale ops. A CPU decision hit also checks device health before reuse. Original, explicit overrides and unsupported/dynamic-dependency cases take existing path unchanged. On miss retain original six-frame measurement order and winner behavior, then insert only the small successful result.

## Exact key / dependency contract

Use one domain-separated256bit fingerprint, computed by streaming a versioned structured encoding into blake3 (implement bounded `io::Write` adapter; do not serde_json::to_vec or clone variable-sized recipe payloads). Fixed-size fields use specified endian/tag framing; settings serialized directly from validated existing reference. Cache key includes:

- domain/schema `tessera proxy calibration decision v1` and selector-policy version1;
- owner ImageId; full asset container digest; original digest+length; tier/format version; source proxy width/height;
- journal incarnation, generation and exact recipe document digest (conservative: even unknown-field/history-only updates cause miss);
- full typed DevelopSettings including HDR/headroom/output, every stage setting and vector payload, plus recipe process version. Do not substitute truncated stage hashes or omit currently unsupported controls. Presentation policy remains in key even though calibration uses normalized SDR clone;
- computed calibration level AND output extent for these settings; sink policy `SDR rgba8 real IOSurface v1`;
- effective RendererConfig: cache_budget_bytes, threads, process_version, entire graph descriptor/flags, preview_approximations. PipelineGraph::nodes() exposes the ordered fixed array: fingerprint each node’s stage, cacheable, implemented and frame using explicit stable tags; include node count. It has no serialization derive, so do not hash Debug text. Key-sensitivity tests must cover graph memoization flags and frame/stage encoding. No field guessed from defaults;
- Engine-local device generation token (initial1, never reused for another device) and fingerprint of adapter backend/vendor/device/driver info plus enabled GpuCapabilities. SharedGpu OnceLock currently cannot replace its device, so generation remains1 during this Engine; loss clears entries and bypasses. If reset/replacement is added later, increment generation before reuse.

External mutable render inputs are a correctness dependency: full settings can contain references such as LUT StyleId or camera-profile names. Cache only the already-supported self-contained proxy resident route. If current eligibility resolves any external mutable asset without a content/version identity included in the key, BYPASS decision reuse and retain ordinary calibration. Do not assume hashing a path/name hashes its contents. This slice should first support the existing ordinary unmasked/noLUT eligible proxy recipes; broaden only with enumerated dependency identities and review. Mapped geometry and unsupported tails can simply bypass reuse; their existing CPU behavior is not altered. This conservative cache has intentionally limited hit rate.

## Fixed ownership and memory bounds

Use a fixed `[Option<Entry>;16]` array, not HashMap/Vec with allocator spare capacity. Entry contains32byte fingerprint, small Cpu/Metal enum, insertion Instant, last-used monotonic ordinal and optional fixed six-f64 sample array. No variable allocations in entries. Enforce `size_of::<ProxyDecisionCache>() <= 8192` with compile-time/test assertion; size_of includes Option padding, entry array and bookkeeping. Engine mutex adds fixed bounded overhead separately; no claim of a process memory cap. Hashing streams through a fixed adapter without accumulating bytes; serialization failure/nonfinite unsupported values produce bypass, not an open failure introduced by optimization.

Maximum16 entries; expired entry removed on lookup/insert. TTL30seconds from successful measurement completion, NOT sliding on hit. Evict least recently used among nonexpired entries when full, update access ordinal with checked overflow/reset-safe ordering. Fake clock in pure tests; monotonic Instant in product. The TTL reduces stale-performance choices but cannot guarantee thermally optimal selection. Cache stores no error strings or variable-size provenance.

Never hold cache mutex while hashing settings, loading assets, creating device/operators, measuring/rendering, callbacks or session construction. Lookup returns copied decision; insertion accepts copied key/sample record. No wait queue/singleflight in this slice: duplicate concurrent misses may measure independently, then safely replace exact key. Cache is advisory, so poison/lock error should clear/bypass safely, not fail image admission. Engine drop releases fixed cache; existing Shared/Renderer/GpuStageOp/ring Weak lifecycle proof must remain valid.

## Invalidation and bypass matrix

- Every open validates original/local source, journal and sidecars BEFORE lookup; any corruption/staleness/dirty Original conflict retains exact current error and performs no cache-authorized render.
- Different asset/tier/dimensions/container/incarnation/generation/recipe/settings/config/device/level/sink identity: miss by key. Rebuild same bytes still changes incarnation. Save/sync changes generation/digest: miss. No cross-photo reuse.
- Time>=30s, capacity eviction, Engine restart/drop: miss. No disk cache; process restart cannot reuse.
- `TESSERA_RENDER_BACKEND=cpu|gpu`: bypass read/write; preserve current explicit route. Override absence/unknown value keeps existing auto interpretation. Tests must clean env serially; do not add a product rollout environment flag.
- Device lost/unavailable, GPU context creation error, failed/nonpositive/nonfinite calibration: bypass/no insertion. On device loss clear cache; keep current source validation and CPU error/fallback behavior.
- Mapped geometry/unsupported tails/external dependency without identity: bypass. Per-edit `can_render_resident` checks remain authoritative even after successful cached Metal selection; later crop/transform still CPU, resetting can restore resident.
- In-flight source mutation is governed by existing stableID admission/validation, not decision cache; do not claim external-process locking.

## Tests-first sequence and seams

A. Pure tests before product: fixed storage/count/bytes, TTL boundary/non-sliding age/LRU, fakeclock expiry, duplicate insertion/concurrent misses, key changes for each dependency family, no insertion on any failure outcome, override bypass. Expose test-only scalar hit/miss/calibration-run counters, no strong backend/image references. Tests for document/profile dependency bypass and original validator call ordering are mandatory.
B. Preserve `backend::gpu_is_faster` and current surface measurement tests. Meaningful selector tests inject cheap preparation/measurement results only, while real lifecycle/validation integration uses actual Engine methods. Prove warm cache cannot bypass corrupted/deletedproxy, changed original digest or sidecar conflict; no stale recipe returned.
C. Lifecycle: extend existing preview_qualification owned-resource test; fill entries, close/drop sessions, clear instrumentation strong references, bounded5s eventual WeakShared/Renderer/GpuStageOp/ring release, Engine drop. Cache entries survive close but retain zero heavy ownership. Dirty recipe edit/save/reopen creates miss; unchanged clean close/reopen hits. Preserve CPU override and geometry transition counter assertions.
D. Independently review before runtime, then assigned serialized native focused/strict/full checks. No archive/UI gate needed until code actually changes; final FFI integration/Swift workflows remain required because this is public open behavior. Root controls gates and main merge.

## Measurement protocol and predeclared acceptance

Do not benchmark now. First freeze exact accepted baseline + harness-only extension; same copied Sony fixture/original hashes, same viewport/settings/output and normal auto mode. Use separate fresh processes for baseline/candidate, five unchanged-key reopen cycles in the SAME Engine after one normal initial open, for SDR and EDR. Keep five paired trials per mode with predetermined ABBA alternation across baseline/candidate processes; no competing GUI/GPU/builds. Fresh process is not cold filesystem. Test-only cache-disabled candidate control may additionally isolate cache effect but does not replace exact baseline artifact.

Time from immediately before public open through first matching-generation FINAL frame for the fixed requested viewport delivered to actual host IOSurface. Record open-return, post-open frame and sum; normalization/pixel readback after timer. Record actual settings/output, requested+actual level/dimensions, route, submissions, timed readback, calibration counts/hitmiss and key fingerprints. Do not compare ratios for different output dimensions; no source-size advantage over Original claim. Do not save recipe during hit series; separate changed-recipe functional case proves miss.

Freeze these thresholds BEFORE collecting candidate timings (proposal for root approval):
- Benefit: both SDR and EDR exact-key reopen median end-to-end delivery decreases by at least20% AND at least20ms relative to matched uncached auto median.
- First-open miss regression: median no worse than baseline +max(10ms,5%). Warm exposure/WB-edit median after reopen no worse than baseline +max(2ms,10%). Five samples establish bounded median only, never p95/tail guarantee.
- Baseline stability preflight: if baseline repeated-cycle coefficient of variation exceeds20%, mark performance qualification inconclusive; investigate environment or collect a separately preregistered larger baseline before ANY candidate timing, not widen tolerances after candidate data. Record all attempts.
- Fidelity unchanged: exact-dimension CPU/GPU comparisons must pass existing accepted numerical gates (SDR and EDR bounds unchanged), float EDR retains above-white content/recipe policy, GPUeligible frames prove Metal submissions/resident receipt and zero timed pixel readback; mappedCPU proves no new Metal submissions. No silent fallback reported asGPU speed.
- Mandatory hits: every intended unexpired unchanged-key reopen has zero calibration frames; changed-key/expired cases demonstrably calibrate or take documented bypass. If cold fresh operators erase aggregate benefit, reject cache optimization despite lower open-return time.

Current625/791ms figures motivate testing only. No predicted74ms result, first-ever-open speedup, crosscamera generality, physical screen/input-to-present latency or global memory bound. Keep accepted behavior if any correctness/fidelity/lifecycle gate fails or measured benefit is absent.

## Execution units and exact source boundaries

### Task 1: Pure bounded decision storage and key encoding

Files: create `crates/tessera-ffi/src/backend/proxy_decision_cache.rs`; declare privately from `crates/tessera-ffi/src/backend.rs`. Read `crates/image-core/src/graph.rs::PipelineGraph::nodes`, `render.rs::RendererConfig`, and `gpu-core/src/lib.rs::GpuCapabilities`. No graph or GPU public API expansion is needed.

- [ ] Add failing unit cases `fixed_storage_is_bounded`, `expiry_is_not_sliding`, `lru_evicts_seventeenth`, `key_changes_with_each_dependency`, `invalid_measurement_is_not_inserted`, and `overrides_bypass_read_and_write`. Use fake monotonic time and fixed sample arrays, not sleeps/GPU.
- [ ] Observe RED under assigned lane: `cargo test -p tessera-ffi --release proxy_decision_cache -- --test-threads=1`.
- [ ] Implement private fixed array, explicit key encoder and copied selection result. Unit test exact boundary at 30 seconds, all 16 slots, ordinal rollover, and encoded setting/graph differences.
- [ ] Observe GREEN with same command; review and commit only this bounded unit.

### Task 2: Validated identity and selection integration

Files: `crates/tessera-ffi/src/{lib.rs,backend.rs,develop.rs,smart_preview.rs,smart_preview_store.rs}`. Consume full copied identity from the already validated decode/journal snapshot; produce only a fresh `backend::Backend` through `Engine::develop_renderer`. Cache lookup precedes neither `load_smart_preview` nor the document/source compatibility checks.

- [ ] Add failing integration cases with prepopulated successful decisions: corrupt container must still error; changed online original must still error; changed sidecar must still follow conflict behavior; unchanged reopen hits; changed journal recipe misses; explicit CPU/GPU bypass; mapped/external-asset cases bypass. Assert actual validation/measurement counters, not merely returned labels.
- [ ] Observe RED before integrating.
- [ ] Add Engine-local fixed cache and copied validated identity, split measured outcome from fallback, reconstruct fresh backend on hits. Preserve current six-frame ordering and winner backend on misses.
- [ ] Observe GREEN; run existing smart-preview workflow and full FFI unit/strict checks under assigned lane; independent source review before checkpoint.

### Task 3: First-frame and ownership qualification

File: existing `crates/tessera-ffi/src/develop/preview_qualification.rs` and its external runner. Add no public telemetry API.

- [ ] Extend baseline harness with same-Engine unchanged reopen, initial/miss/hit timestamps, scalar decision counters, and bounded owned-resource release checks. Validate this harness against unchanged baseline before candidate measurement.
- [ ] Freeze exact numerical fidelity thresholds from the accepted runner into the run manifest (not only a reference to “existing gates”); abort if they cannot be located. Freeze the performance thresholds above before candidate data.
- [ ] Run the preregistered baseline/candidate ordering only after coordinator lane grant. Every command uses `MACOSX_DEPLOYMENT_TARGET=15.0`, `CARGO_BUILD_JOBS=2`, and `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/depth-histogram-readonly-77eb68d0-relocated`. Capture exact commands, direct exits, source/fixture/artifact before/after hashes and all failures.
- [ ] Review matching geometry/fidelity, misses, hits, device/capability counters and Weak lifecycle evidence. Reject optimization if combined first-frame benefit fails; do not salvage it by reporting open-return alone.
- [ ] Qualify generated archive consistency and actual Swift offline/edit/reopen/sync workflows on final immutable candidate before any merge. Original default and export source guards remain unchanged.

## Review focus / remaining deliberate limitations

1. A journal mutation or source corruption must never become renderable because a cache entry exists (Task 2 validation-order tests).
2. A named LUT/profile may change bytes without changing a recipe (Task 2 explicit bypass until content identity exists).
3. A calibrated CPU decision can outlive thermal/load changes (Task 1 non-sliding TTL; no optimal-backend claim).
4. A removed calibration warmup may delay first real pixels despite a faster open return (Task 3 combined timing gate).
5. A closed session must release heavy operators despite retained decisions (Task 3 Weak ownership tests).

This deliberately targets unchanged recipe reopen only. First-ever open and most edit/save/reopen paths still calibrate because generation/recipe identity changes. That narrow hit rate is a design cost to measure, not a reason to weaken source or settings identity.
