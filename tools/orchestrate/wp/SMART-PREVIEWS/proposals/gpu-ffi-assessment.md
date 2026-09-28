# Source-only GPU interaction integration assessment

Prepared `/tmp/tessera-smart-preview-gpu-ffi.patch` from main's accepted FFI sources, with temporary copies under `/tmp/tessera-proxy-gpu-ffi`. Three files: backend.rs, lib.rs, develop.rs. `git apply --check` and rustfmt succeeded. No shared source writes, builds, GPU tests, or runtime claims. Depends on qualification/application of `/tmp/tessera-smart-preview-gpu.patch` first.

## Actual app path

Swift DevelopController.attachSurfaces passes actual device-pixel viewport dimensions to plan_surface (swapped for EXIF orientation). Rust chooses the coarsest pyramid level covering both dimensions. A 2560x1706 proxy uses L0 for a 1600x1000 viewport, L1 for 1000x700, and L2 for 600x400. Before surfaces attach, default_level chooses <=2048, hence L1 for this proxy. Adaptive interactive rendering can further drop up to two levels. Thus L0-only GPU support is an intermediate improvement, not complete fast-proxy interaction.

Current FFI explicitly returns a CPU renderer for every proxy. Original backend calibration also uses default settings and L2, so simply removing that guard would measure scalar proxy fallback twice. It would additionally let the first proxy poison the original's engine-wide OnceLock choice.

## Bounded qualification patch

The separate opt-in environment variable TESSERA_SMART_PREVIEW_GPU=1 permits a per-session proxy backend decision, using the shared Metal device but never the original renderer OnceLock. Explicit TESSERA_RENDER_BACKEND=cpu remains authoritative. Original source default, export guard, journal and ownership paths are untouched. Proxy calibration uses the current recipe's prefix-compatible settings at L0, measuring the actual IOSurface/histogram sink for first frame, tone edit and a real changed white-balance mode. It preserves CPU fallback on unsupported settings/failure. Both recurring edit timings must beat CPU to select GPU automatically; explicit gpu override still skips calibration.

Backend description identifies L0 capability and coarse CPU fallback. Resident drag admission is false for coarse proxy screen levels rather than assuming the no-level capability applies to L1/L2. The patch does NOT force oversize L0 surfaces or disable adaptive downshift. It will accelerate supported L0 interactions only after native GPU capability and this FFI path are qualified. Every proxy open recalibrates instead of caching a recipe-dependent unsupported decision for all proxy photos; startup calibration cost is an explicit intermediate limitation.

## Required qualification before enabling by default

1. Apply underlying GPU patch and run its numeric signed/HDR/captured-lens/geometry/WB tests and RGBA8 IOSurface tests; inspect actual resident submissions and zero pixel-readback evidence.
2. Run public Engine proxy open and attach_surfaces at both L0 and L1 viewport sizes with the opt-in. Test CPU override, auto and GPU override, valid captured nondefault prefix, unsupported local adjustment fallback, original/proxy open-order independence, dirty journal/export gates.
3. Measure actual listener frame end-to-end and render_ms for warm tone, WB, presence, crop and drag bursts; record frame levels. Include SDR and RGBA16F/EDR surface qualification. Calibration currently uses SDR only, matching existing original calibration; HDR cannot be inferred from its result.
4. Compare against Original Metal at matched visible dimensions. A synthetic L0 tile benchmark does not establish app speed.

## Coarse GPU follow-on: reusable primitive exists, but integration needs a deliberate boundary

`ResidentBatch::resample` and pipeline-gpu/src/resident.rs already provide exact box accumulation from multiple resident tiles using crop+TileCoord level. They can perform the needed GPU reduction. Existing export_resize uses Lanczos and is NOT the reference proxy coarse algorithm.

The scalar proxy path calls render_linear_scaled: full proxy development and geometry first, box reduction in scene-linear space second, Output/display encoding last. Therefore reducing uploaded camera pixels before tone/detail/geometry, or reducing encoded L0 pixels, changes semantics.

The current run_resident/run_resident_level consume the batch and finish internally, with Output sometimes fused before remap or finish and whole-level cache shortcuts. Safe follow-on requires extracting a reusable resident linear-tail result BEFORE display conversion and batch.finish, shared by tiled and whole-level paths. For a coarse request: render required L0 linear tail with captured optics/geometry and current output extent; resample those GPU tiles using crop=[0,0,output_width,output_height] into requested coarse coords; apply display_op afterward; finish directly into the coarse IOSurface and histogram. Use representation, recipe, post-geometry extent and level in cache keys; cancellation throughout; never use original sensor extent for proxy addressing.

This is not a safe one-line removal of level checks: both run_resident branches and scene-linear/RGBA8/RGBA16F output contracts need the same ordering. I have not produced a speculative second renderer implementation in this task. Tests must cover odd sizes/edge denominators, geometry crop/distortion, signed HDR, WB/tone/presence, partial tile vs full surface, histogram parity, adaptation L0->L1->L0 and stale cache invalidation. Once qualified, make capability level-aware and calibrate at actual viewport levels (or a small per-representation level matrix), then remove the L0-only FFI qualification switch.
