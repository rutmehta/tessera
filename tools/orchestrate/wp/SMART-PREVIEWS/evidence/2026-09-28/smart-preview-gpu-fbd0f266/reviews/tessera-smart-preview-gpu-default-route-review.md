# Default explicit-proxy backend route review

Source-only, no builds or source edits by reviewer.

Product gate removal in Engine::develop_renderer is scoped to camera_linear_proxy().is_some(). Original path and renderer OnceLock branch remain unchanged; no Swift source-preference default edit. select_proxy retains CPU override before GPU creation, unavailable-device/context fallback and measured Auto selection. Both mapped-geometry capability and actual dispatch remain CPU fallback. Backend label remains selection information, not frame telemetry. No product correctness finding in this final delta.

Lifecycle test covers3 repeated actual proxy sessions forAuto andCPU, owned SDR rings, mapped CPU transition/reset resident, actual receipts/submission counters/no pixel readback, explicit listener/session/ring/instrumentation release. Measured EDR workload is separate; SDR lifecycle ring is intentional.

Actionable test-only finding sent author: immediate GPUWeak/IOSurface release assertion races worker completion. ViewportJob.run delivers final frame inside finish_surface at develop.rs2348, then reports progress/returns/drops. Public close flushes saveworker and cancels renderworker but does not join its completion (3145–3179). Receiver may close/drop/assert while a legitimate finishing job still owns renderer/surface references. Use a bounded eventual-release wait or actual terminal-job synchronization; do not diagnose a leak from immediate callback return. Optional Weak Shared/Renderer assertion would extend lifetime coverage toCPU; current CPUring release alone does not prove CPUcache release.

Final source review awaiting this lifecycle oracle correction. No runtime outcome asserted.

Final follow-up reviewed: author replaced immediate assertions with5ms polling bounded to5seconds after all explicit ownership drops, recording release_ms/deadline_ms. Added WeakShared and WeakRenderer forCPU as well as optionalGPU Weak; owned-ring lookup retained. Finding resolved. Final source review approved; runtime remains pending.

Final reviewed SHA256:
- f962502ac9bc6eabd287849fae56bc5e13362b47f8e79f4f5a4331b145be3456  crates/tessera-ffi/src/lib.rs
- ab6d7b51a0033bf1927670cbc7dc53e6e298563d049998ecd8f9b44b543b228d  crates/tessera-ffi/src/backend.rs
- 42e7795ec723457fed06a2a73174095ad3e25265f9d8fc732d8cd3080ca820a2  crates/tessera-ffi/src/develop/preview_qualification.rs
