# Revision 2 review mapping

- Smaller first proof adopted: image-core actual Detail/padded-WB/tile-WB keys and Some/None/error; cache_exact recorded REQUEST only. No source lookup or pipeline-gpu edits needed.
- Publication semantics corrected: no request/admission/persistence claim, no pending-versus-persistent inference. Later actual Some is the observation.
- Scope reduced: default-off image-core feature + FFI phase/harness plumbing, two crates only. Broader GPU counters explicitly deferred.
- Identity corrected: copied Backend/operator identity shared with calibration/session Renderer; local render ordinal is not falsely called an internal GPU transaction. Losing-operator scalar records retained without Arc ownership.
- Missing events: not-reached requires complete enclosing-hit evidence; overflow/error/missing phase means inconclusive.
- Bounds:256/32KiB provisional cap requires actual35-tile and multi-render phase inventory, compile-time size check and explicit overflow refusal; no arbitrary sampling.
- Actual route/TTL: automatic Metal per session and real second-session decision hit with zero added calibration required; CPU/expiry stops as inapplicable/inconclusive.
- Semantic discriminator: resolved Custom6500/+10 matrix must differ from Daylight; exact failed firstDaylight preserved, repeats/custom never replace it.
- Existing fidelity, hashes, generation/final matching and fresh-resource release remain. No performance acceptance, implementation or runtime.

Reviewed source: /tmp/tessera-proxy-cache-wb-diagnostic-plan-review.md, especially Root scope clarification. v1 retained as /tmp/tessera-proxy-cache-wb-diagnostic-plan-v1.md. Current proposal: /tmp/tessera-proxy-cache-wb-diagnostic-plan.md.
