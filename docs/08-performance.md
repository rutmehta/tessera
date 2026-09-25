# Performance Requirements

Goal: noticeably faster than Lightroom Classic and Photoshop on the same hardware, and never blocking the user.

## 1. Targets (45 MP raw, Apple M3 Pro / RTX 4070 laptop class)
| Operation | Target | LrC/Ps today (typical) |
|---|---|---|
| Open folder of 2,000 raws to scrollable grid | < 1 s (embedded previews) | 30–120 s to build previews |
| Loupe first paint after grid click | < 50 ms (embedded/preview), < 300 ms to full render | 0.5–3 s "Loading…" |
| Slider drag latency (screen res) | < 16 ms per frame | 30–200 ms |
| 1:1 region render after zoom | < 100 ms | 0.5–2 s |
| AI mask (subject/people) | < 300 ms | 1–4 s |
| AI denoise | < 3 s | 5–20 s |
| Export 100 JPEGs | < 40 s (parallel) | 2–4 min |
| Library search over 1M images | < 100 ms | seconds, or unusable |
| App launch to usable grid | < 1 s | 5–20 s |
| Memory | Bounded tile cache, configurable; no unbounded growth | Frequent multi-GB growth |

## 2. Architecture levers
- **GPU-resident pipeline**: raw tiles uploaded once; every stage is a compute kernel; no CPU round trips until encode. Zero-copy texture sharing with the UI (IOSurface / DXGI shared handles).
- **Progressive rendering**: render at 1/8 → 1/4 → 1/1 as the viewport settles; sliders always update the on-screen resolution first.
- **Stage memoization**: cache post-demosaic and post-denoise buffers (the expensive stages) so tone/colour edits never re-run them; recipe diff decides the earliest dirty stage.
- **Embedded preview fast path** for culling; full render only on demand.
- **Background job scheduler** with priority classes (UI > viewport render > prefetch neighbours > previews > AI scores > exports) and cancellation; prefetch next/previous images in the filmstrip.
- **Index, not database**: read-mostly SQLite with covering indexes; grid virtualization; metadata facets computed incrementally; FTS5 for text; vector index for similarity.
- **Model execution**: ONNX Runtime with CoreML/DirectML/CUDA/TensorRT providers; models quantized (int8/fp16) where quality allows; batched inference for culling scores; warm model cache.
- **Startup**: no catalog "open" step; lazy index load; UI shell first, data streams in.
- **Export**: tile-parallel render + parallel encoders; reuse cached stage buffers when exporting the currently open image.
- **No modal blocking**: previews, AI scores, imports, exports and sync all run concurrently with editing.
- **Layered editor**: tiled copy-on-write layers, per-layer mip caches, dirty-rect compositing; brush strokes rendered incrementally.

## 3. Engineering practice
- Continuous performance benchmarks on fixed hardware in CI with regression gates for the table above.
- Golden-image and bit-exactness tests to prevent GPU/CPU divergence.
- Telemetry (opt-in) of stage timings to drive optimization priorities.
