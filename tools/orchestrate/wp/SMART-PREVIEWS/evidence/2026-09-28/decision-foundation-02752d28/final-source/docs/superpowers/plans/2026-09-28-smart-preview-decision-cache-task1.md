# Decision reuse Task 1 — pure contract preparation

Approved parent plan: `2026-09-28-smart-preview-decision-reuse.md`. This checkpoint contains only tests and explicit stubs, not a working cache. No Engine integration, product selector change, source-validation change, compilation, formatting or runtime is authorized/performed in this source-preparation phase.

Base is immutable baseline3614e21bee508563432a9d235964b76e3c43a02f, preserving the same product implementation for future A/B. Prior captured-CFA branch32d792c2 remains preserved separately. Frozen proxy-reopen-baseline checkout is not changed. Current worktree reused only after clean/no checkout-associated compiler/process verification; other unrelated long-lived app processes were not terminated or considered owned.

## Private source boundaries

`backend.rs` adds only cfg(test) module declaration. `backend/proxy_decision_cache.rs` has private fixed-array types and empty/miss/Unsupported stubs. `backend/proxy_decision_cache/tests.rs` has18 deterministic pure contracts. No public API/dependency/environment mutation, device initialization, filesystem source or actual calibration path is involved. Stubs deliberately return no hits/no insertion and key Unsupported; RED counts must be observed rather than guessed. The fixed-layout/empty-state control can already pass and is not falsely labeled behavioral RED.

Fixed storage is16 Option entries, <=8192bytes including padding/bookkeeping, no Drop/heap-owning entry fields. Entry fields are key32bytes, Cpu/Metal decision, completion timestamp, recency ordinal and fixed six measured times. Pure time input is u64 monotonic nanoseconds from a single Engine-local epoch: TTL30seconds by elapsed subtraction, not saturating absolute expiry. Caller clock adaptation and device health discovery are Task2. Ordinal rollover must preserve relative LRU order rather than silently turn a touched entry oldest.

A successful measuredCPU outcome is reusable just like measuredMetal, only if all six CPU/GPU samples are finite and strictlypositive. Existing selector rule remains toneGPU<toneCPU AND wbGPU<wbCPU; initialGPUneednotbefaster. Equality choosesCPU. Unavailable device/operator or failed calibration is never represented as successful measuredCPU. Explicitoverride outcome and override policy bypass read/write. DeviceUnhealthy clears both decision kinds. HDR is NOT an unsupported recipe by itself: full hdr/headroom remains key identity even though actual calibration later uses its existing temporary SDR clone. The pure key test proves those fields differ without mutating input; Task2 eligibility must preserve qualified HDR support and actual-source validation.

Keys consume copied validated asset identity, complete settings, recipe process, effective renderer config/ordered graph, device generation/descriptor+capabilities and calibration sink/geometry/policy. Asset metadata includes full256bit container digest, full original digest+length, owner16bytes, incarnation, generation, exactdocumentdigest, both dimensions/tier/format. Device capability bits explicitly name all four current GpuCapabilities; any new capability must update version/key review. Adapter fingerprint must later cover actual backend/vendor/device/driver descriptor, never just name. Key generation errors mean advisory cache bypass at later integration, not an image-open error. Full typed-settings streaming and explicit graph encoding remain required; vector-tail and truncated-container collision regressions pin those dependencies.

## Contract list and later execution

1. Fixed16/8KiB/no heapownership.
2. Successfully measured Metal and CPU reusable.
3. CPU selection preserves strict existing tone/WB rule.
4. Exact TTL boundary; hits do not slide measurement completion age.
5. Seventeenth entry evicts true LRU.
6. Ordinal rollover preserves recency/capacity.
7. Duplicate successful publication replaces value+completion time, single slot.
8. Invalid samples/all failure outcomes cannot insert or replace successful entry.
9. Explicit overrides/mapped geometry/unsupported tails/unversioned assets bypass reads+writes.
10. Device loss clearsCPU+Metal.
11. Explicitclear/Engine restart have no entries.
12. Every copied asset/journal/device/calibration dependency changes key.
13. Full HDR/headroom/exposure/WB settings distinguish key without mutation.
14. Renderer budget/threads/process/approximation/graph flags distinguish key.
15. Key deterministic; nonfinite settings return explicit key error/bypass.
16. Expiry pruning oninsert; timestamp arithmetic near u64MAX safe.
17. Concurrent independently completed misses publish one slot under caller mutex.
18. Deep curve vector tail and last128bits of containerdigest affect key.

Before any implementation: independent testcontract review; then assigned runtime lane and observed compiled behavioralRED via `cargo test -p tessera-ffi --lib --release proxy_decision_cache -- --test-threads=1`, using exactBetterSSD target/deployment15/jobs2. Preserve compileerrors separately if any; don't equate compilation failure with behavioralRED. Only then separately authorized minimum Task1 implementation, GREEN/strict/fmt and review. No Task2 Engine integration before the pure unit is accepted. No candidate timing before the full accepted integration/measurement plan and coordinator grant.

Remaining later seams: actual validated-load fullcontainer/incarnation transport; externalasset eligibility; real device-health invalidation; current renderer/config fingerprint; lock poisoning/bypass and no callbacks/calibration under lock; source-corruption/dirtyjournal guards BEFORE lookup; fresh operator construction; lifecycle proof; cachehit/calibration counters; exact current SDR/EDR hostviewport A/B. These are not falsely covered by pure tests. Private cache bytes bound does not cap serializer work, decodedimage/native memory or the overall process.

Nonfinite clarification: serde_json can serialize NaN/Infinity as null without error. Implementation must explicitly reject nonfinite settings before or during fingerprint encoding; successful streaming JSON serialization alone is insufficient. Pure contracts cover NaN/+Infinity/-Infinity in exposure, HDR headroom and nested curve-vector entries, requiring key error/cache bypass rather than a usable null-aliased key. Do not reuse broad legacy SDR validation that rejects otherwise-qualified HDR presentation; preserve finite HDR support. This checkpoint still only has Unsupported key stub, so none of this behavior is claimed implemented or passed.
