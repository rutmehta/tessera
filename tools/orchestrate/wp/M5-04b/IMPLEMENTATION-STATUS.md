# M5-04b implementation status: partial (one performance target missed)

## Done and verified

- `gpu-core` crate: shared `GpuDevice` (device, queue, adapter info, capabilities, limits, device-loss), `read_buffer`, and the IOSurface import moved out of pipeline-gpu. `pipeline_gpu::GpuContext::{new, from_shared, shared}` and `GpuCompositor::{new, from_shared}` use it; pipeline-gpu's public API (fields, `write_to_iosurface`, `SurfaceFormat`, `GpuCapabilities`) is unchanged.
- `compositor::resident::ResidentRenderer` (replaces M5-04's flat `ResidentComposite`): content-addressed GPU page pool (level-0 tiles uploaded once per tile buffer, COW duplicates share pages), GPU mip pages hash-consed by child pages, program compiled from the document tree into persistent step/table/aux buffers, one dispatch per frame over the whole level or only damaged 16² blocks (page-table diff ∩ document damage log), all blend modes/groups/clipping/knockout/masks/Blend If/fills, all adjustment layers on the GPU, smart objects via uploaded CPU tiles, LRU eviction under a budget, `present` / `present_iosurface` (RGBA8) and explicit readback.
- Exact integer 8/16-bit mips on CPU and GPU (bit-identical).
- Premultiplied flag set on root/group/GPU/resident premultiplied tiles; `CompositePyramid::level_count` goes to 1×1 (capped at MAX_LEVEL).
- COMPOSITOR.md: engine-api version line, §5 mips, §6, §9, §10, §11 (engine-api 1.2), new §12 (design, damage, gate, bench).
- Gates (tests/gpu_resident.rs, 11 tests): per mode ≤ 2.4e-7 (Saturation 2.3e-6), each adjustment ≤ 3.6e-7, 8/16-bit mips ≤ 1e-6 levels 0–11, 50-node chain ≤ 1.4e-3 at float/16/8-bit and levels 0–9 (bound 2e-3), dirty-rect frames bit-identical to cold renders through paint/props/adjustment/undo/redo, determinism across renderers and devices, COW page sharing counts, eviction correctness, smart objects, presentation, shared device.

## Bench (M4, loaded machine; see COMPOSITOR.md §12.4)

- Cold open → first L2 frame: ~0.54–1.04 s (target < 1.5 s) — met.
- 64² dab → L2 recomposite: median 1.6 ms (target < 16 ms) — met.
- Full L0 composite 20 MP × 100 layers: 198 ms (target < 100 ms) — NOT met (CPU before: 1311 ms).

## Not done

- L0 < 100 ms. Shader interpretation overhead (per-step mode switch, generic loop) is ~2× a hand-written kernel; plan in COMPOSITOR.md §12.4 (per-structure specialized kernels with interpreter fallback).
- The app (apps/mac, tessera-ffi) is not yet switched to the shared `GpuDevice` or the resident renderer (out of scope).
- Presentation writes RGBA8 in document encoding only (no RGBA16F/EDR, no colour management); no viewport-restricted partial-level rendering; smart objects still CPU-resampled; nesting > 7 frames and mixed-depth rasters are `Unsupported` on the resident path.

## Verification

`CARGO_TARGET_DIR=~/.cache/tessera-target/M5-04b PIPELINE_RAW_FIXTURES=<repo>/fixtures/raw cargo test -p compositor -p psd -p pipeline-gpu -p gpu-core --release --no-fail-fast` (201 passed), `cargo clippy --release -p compositor -p psd -p pipeline-gpu -p gpu-core --all-targets -- -D warnings`, `cargo fmt --check`. Benches: `cargo test -p compositor --release --test bench -- --ignored --nocapture`. engine-api and apps/mac unchanged.
