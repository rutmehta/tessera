# WP M0-07 — GPU spike: wgpu 30 vs native Metal for tile kernels (escalated to Opus)

Standalone crate at `spikes/gpu-bench/` (own Cargo.toml; do not touch the root workspace). A previous attempt left a placeholder that measured wall-clock dispatch of simplified proxies and skipped the native comparison; replace it entirely. This spike is the go/no-go for using wgpu compute in the pipeline on Apple silicon, so the measurements have to be real.

Implement three kernels on a 4096×4096 image, each in (a) WGSL via wgpu 30 compute and (b) MSL via `objc2-metal` (compile the MSL at runtime from a string):
1. Bilinear Bayer demosaic: input u16 RGGB CFA (synthesised from a known RGB image so error is measurable), output RGBA f32.
2. Guided filter, self-guided, radius 8, eps 1e-3, using separable box sums (two passes) on RGBA f32.
3. 3D LUT (33³, trilinear) applied in Oklab: linear RGB → Oklab → LUT → Oklab → linear RGB.
Timings: GPU-only time via wgpu `TIMESTAMP_QUERY` (`Features::TIMESTAMP_QUERY` + `write_timestamp`/`resolve_query_set`) and Metal `MTLCommandBuffer` `GPUStartTime`/`GPUEndTime`; median of 20 runs after 3 warmups. Also report wall-clock including readback for context.
Accuracy: scalar f32 CPU reference for each kernel; report max abs error and mean abs error for both backends versus the reference, in linear units.
HDR surface: with `winit` create a hidden or tiny window, try `SurfaceConfiguration` with `Rgba16Float` and each of `ExtendedSrgbLinear`, `ExtendedDisplayP3`; record success/failure and the result of `display_hdr_info` (or whatever wgpu 30 exposes, name it exactly).
Output: `cargo run --release` writes `REPORT.md` **and** `report.json` with fields `{kernels:[{name, backend:"wgpu"|"metal", gpu_ms, wall_ms, max_abs_err, mean_abs_err}], hdr:{extended_srgb_linear: bool, extended_display_p3: bool, notes}, recommendation:"go"|"no-go"|"go-with-msl-passthrough", rationale}`; every kernel must have both backends and finite numbers. End REPORT.md with one paragraph recommending whether wgpu is acceptable (rule of thumb: within 1.5× of native on all three, errors ≤ 1e-4) and what to do if not.
Build with `CARGO_TARGET_DIR` outside the repo if the repo path contains a colon.

Test command: `cd spikes/gpu-bench && cargo build --release && cargo run --release && python3 -c "import json;d=json.load(open('report.json'));ks=d['kernels'];assert len(ks)==6 and all(k['gpu_ms']>0 and k['max_abs_err']>=0 for k in ks) and {k['backend'] for k in ks}=={'wgpu','metal'};print('ok',d['recommendation'])"`
