# WB exact-key priming diagnostic — narrowed revision 2

Proposal only: no code, runtime, benchmark rerun or acceptance change. Supersedes v1's broader instrumentation proposal; v1 retained separately. This revision adopts the root scope clarification in /tmp/tessera-proxy-cache-wb-diagnostic-plan-review.md. Frozen baseline5f31f148, candidate7a3ec9bf, 03-performance failure and every preregistered threshold remain unchanged. The decision cache is NOT performance-qualified.

## Question and evidence standard

Determine whether actual baseline calibration requests WB-dependent intermediates subsequently returned by an exact-key lookup during the first Daylight edit, while a fresh candidate hit session has to compute those intermediates. A same-operator calibration request plus later same-key Some is evidence of reusable primed work; it does not prove cache admission history or GPU cost. Some means an actual value was returned. None means actual lookup miss. Error remains error. Never call a cache_exact request "published", "admitted" or "retained".

Calibration clones the real initial settings, adds0.25exposure for its second render, then toggles AsShot/Daylight for the third. With the preserved fixture recipe, those become exposure0.25/AsShot,0.5/AsShot,0.5/Daylight. The exact failed probe is0.5/Daylight. Preserve all full settings and normalized render settings; do not generalize these fixture values to every calibration.

## Scope: two crates and harness only

Modify only a separately reviewed diagnostic composition of image-core and tessera-ffi/harness. One explicit default-off image-core observation feature is propagated by the FFI diagnostic feature. Dependencies do not inherit caller cfg(test). Default builds must contain no observation storage/behavior. No pipeline-gpu changes, public FFI/API, renderer arithmetic, cache policy, eligibility, selection, image validation or resource-lifetime change.

At image-core/resident_render.rs, observe these existing calls exactly once:

- `develop_resident_level`: full-level Detail lookup at approximately1003; padded-WB lookup at1007, with distinct bucket labels.
- `balanced_tiles`: camera-linear WB-tile lookup at805. Do not require nested tile lookup execution when a successful enclosing hit skips it.
- Corresponding existing cache_exact calls: label REQUEST before calling, then returned success/error if needed; success is not persistent publication.

Copy actual MemoKey fields (image numeric identity, stage, coordinate, full parameter fingerprint), not independently recomputed keys. Record actual returned Some/None/error without repeating a lookup, perturbing LRU or inspecting a returned tile's ownership beyond existing code. Capture actual resolved WB matrix fingerprint and full recipe fingerprint so Daylight/custom differences are verified semantically. Do not add source-cache observation to this first slice: Detail/WB keys suffice, and nested source work may not be reached.

## Phase and operator identity

FFI assigns a copied numeric identity to each newly constructed Backend/operator. Propagate the same scalar ID into its temporary calibration Renderer and session Renderer; no observer retains Backend, operator, renderer or image Arc. Baseline and candidate reopen must have fresh operator IDs, while each session's selected calibration and display share one ID as applicable.

Record explicit phase begin/end/error around each real CPU/GPU calibration render and each harness request/final-frame completion. Capture losing-backend scalar calibration records before it drops, without retaining it. Associate records with operator ID, phase ordinal, render transaction ordinal, generation where applicable, level and output. Multiple renders within a phase must have distinct transaction ordinals; do not invent equivalence to an internal GPU transaction unavailable at this layer. Every aborted scope closes through RAII, retaining partial observations. Phase output is written before assertions.

A missing nested event means **not reached** only when its successful enclosing hit/control-flow record is present in the same complete phase/transaction and there was no overflow/error. Otherwise missing means unobserved/inconclusive. A Detail hit proves the WB-dependent Detail chain was reused; it does not manufacture a WB-tile or source hit. No pending-versus-persistent provenance can be inferred here.

## Fixed storage and overflow

Use a bounded per-phase scalar ring, no paths/strings/pixels/tiles/Arc or unbounded allocation. Before implementation settles its size, enumerate the actual accepted fixture's tile/phase inventory. Current35 Compact L0 tiles give at most35 WB lookup events plus35 request events, plus padded-WB and Detail lookup/request and phase boundaries per ordinary single render (less when enclosing hits skip them). If request completion is separate, include those extra37 events. Account explicitly for every calibration render and any progressive/final render transaction; drain at each calibration-render completion and harness final-frame boundary. Do not merely assume256 entries is sufficient for a multi-render phase.

Provisional cap256 fixed records and32KiB total storage is acceptable only with a compile-time layout bound and checked inventory. Overflow increments a scalar, retains the prefix and ends interpretation as inconclusive. Never sample, silently truncate, allocate more or rerun with a larger limit without separate review. Counter snapshots/export occur outside cache locks and after existing calls; no callbacks or formatting in cache operations. Tests cover Some/None/error once-only observation, nested not-reached versus missing, phase abort, overflow refusal, default-disabled behavior and no retained strong references.

Existing GPU submissions, last-resident-dispatches and readback counters remain reported with current semantics. Last-transaction dispatches are not a cumulative counter and cannot be summed across a phase as total work. This slice does not attribute dispatch categories or pipeline creation costs.

## Four-process diagnostic protocol

Exactly baseline/candidate × SDR/EDR, separate diagnostic binaries and evidence. Same disposable Sony copy, Compact, Native2, denoiseOff, initial recipe,640x426 viewport, expected comparable L1/820x546 output. No forced winner or controlled calibration samples. Each process opens once, closes/releases, then opens unchanged in the same Engine within the existing30s TTL.

Assert actual automatic Metal on both sessions. CPU selection is legitimate product behavior but diagnostic-inapplicable for this GPU priming question: stop/report inconclusive, never force Metal. Candidate second session must prove one real lookup hit and zero additional measurement/publication, plus unchanged full key and live TTL. A miss/expiry is inconclusive, not fresh-hit evidence. Baseline retains actual recalibration records. Do not alter clocks or TTL to obtain the hit.

After second-session initial matching-generation final frame:

1. Exposure0.5/AsShot: exact existing exposure probe.
2. Exposure0.5/Daylight: exact first WB probe that failed acceptance. Preserve its first-use event sequence unchanged.
3. Restore exposure0.5/AsShot and render final, then revisit Daylight. Additional diagnostic only; cannot replace step2.
4. Exposure0.5/Custom6500K/tint+10. Require actual resolved WB matrix fingerprint differs from Daylight before interpreting this previously uncalibrated state. If equal, report discriminator inconclusive; no silent substitute.
5. Restore exact original recipe, matching final frame, close/drop/drain.

Record every render including calibration, restore and repeated WB. Same frame generation/final matching, actual Metal receipts, unchanged SDR/EDR numerical bounds, post-observation pixel comparisons and zero timed pixel-readback expectations. Preserve original/proxy/journal/recipe hashes, WeakShared/Renderer/GPU, surfaces and finalEngine release. Source/fixture/runner/artifact freezes before/after. Observation timing is diagnostic and cannot be pooled with uninstrumented acceptance data or used to claim performance benefit.

## Interpretation and deferred work

Expected supporting result: baseline firstDaylight receives same-key Some for a WB-dependent entry requested in its selected operator's calibration; candidate hit firstDaylight sees None at that corresponding reached bucket; repeat becomes Some; distinct custom first-use misses. Compare the highest reached bucket first (Detail before paddedWB before tileWB). A higher-level hit can establish priming while lower buckets correctly remain not reached. Different key/operator, incomplete records, errors, overflow, CPU route or TTL miss limit attribution; do not assert causality for unobserved buckets.

This can establish exact-key priming, not quantify which GPU kernels cost9ms. Defer all pipeline-gpu pending/persistent admission, eviction/rejection, packed conversion, monotonic category dispatch and lazy pipeline creation instrumentation unless a separately reviewed follow-up is required after inconclusive first-stage results or to quantify removed work.

No product remedy is part of this plan. Do not pre-render Daylight, hide extra work as setup, retain heavy objects, broaden key eligibility or relabel calibration. An explicit future warm-up design would have to account for every render and combined-open/edit/cancellation/release cost; renderer optimization requires independent arithmetic/fidelity/accounting review. Neither can waive or replace the original failed acceptance gate. If no bounded follow-up meets unchanged gates, leave decision reuse unmerged.
