# M3-16b implementation plan

User-approved scope: brief.md and the implementation request. Execute inline, no agents, commits, or board operations.

- Capture RED for CPU inference reuse across Amount and the new resident capability/handoff API.
- Add image-core CFA capability and validated packed host output. Separate full-strength memo identity from Amount/tone; include image, upstream, sensor pattern/extent, pinned model and adapter calibration/mask revision.
- Add GPU regional packed upload, inverse whole-image rotation and per-site mask unpack, and exact-endpoint Amount blend. Fresh queue-write buffers; f32 exact sensor cache pages.
- Wire tile and band schedulers after Highlights and before Demosaic; enable only supported CFA at both resident gates. Keep CPU fallback and direct adapter inference memoization.
- Add FFI caller-supplied calibrated CFA injection. Do not invent camera noise calibration or change host runtime.
- Verify GPU arithmetic/halos/rotations, cache counts/invalidation/cancellation/fallback, CPU equivalence, and an ignored real-fixture benchmark. Run required package tests, clippy, fmt; report actual evidence and limitations.
