# M2-15 color-mgmt API

Crate `color-mgmt` / Rust `color_mgmt`.
- `Builtin::{Srgb, DisplayP3, AdobeRgb, ProPhoto, Rec2020, LinearRec2020}`.
- `Registry::new()`; `builtin(&mut self, Builtin) -> Result<Arc<Profile>>`; `load_bytes(&mut self, &[u8]) -> Result<Arc<Profile>>`; `load_file(&mut self, impl AsRef<Path>) -> Result<Arc<Profile>>`. Profiles are immutable ICC bytes identified by BLAKE3 digest; same bytes reuse Arc.
- `Profile::digest() -> [u8;32]`, `icc_bytes() -> &[u8]`.
- `Transform::new(working: &Profile, display: &Profile, options: TransformOptions) -> Result<Transform>`.
- `Transform::proof(working: &Profile, display: &Profile, proof: &Profile, options: TransformOptions) -> Result<Transform>`.
- `TransformOptions { intent: Intent, black_point_compensation: bool, simulate_paper: bool, gamut_threshold: f32 }` (Default relative, BPC true, paper false, threshold 2.0 DeltaE76).
- `Intent::{Perceptual, RelativeColorimetric, Saturation, AbsoluteColorimetric}`.
- `Transform::apply(&self, rgb: [f32;3]) -> [f32;3]`; `gamut_warning(&self, rgb) -> GamutWarning { monitor: bool, proof: bool }`.
- `Transform::lut33(&self) -> Arc<Lut3d>` cached in the source registry by source/destination/proof digests and all options; `Lut3d { size: usize, values: Vec<[f32;3]> }`, red fastest index `(b*size+g)*size+r`, normalized [0,1] input; `sample([f32;3])` trilinear/clamped.
- `Registry::display_profiles(&mut self) -> Result<Vec<DisplayProfile>>`, macOS CoreGraphics active displays with copied ICC; other platforms return unsupported. `DisplayProfile { display_id: u32, profile: Arc<Profile> }`.
- `Registry::display_profile(display_id) -> Result<DisplayProfile>` refreshes one active monitor, with registry sRGB fallback for unavailable/unusable profiles. Uses objc2 CoreGraphics/CoreFoundation types and retained ownership. An explicit nullable CopyColorSpace binding avoids the generated non-null wrapper's panic during hot unplug. Active-list membership is checked because CoreGraphics may substitute a main-monitor profile for invalid IDs.

No image-core coupling: callers supply RGB float pixels. LUT is normalized SDR only; direct transform does not pre-clamp working float input. Gamut warnings use clipped destination round-trip DeltaE76 and do not overwrite output pixels. Proof copies/history are caller-owned.

Additional output integration:
- `Registry::linearized_rgb(profile)` returns a registry-owned matrix/shaper variant with linear TRCs, or None for CLUT/non-RGB profiles. It never strips CLUT tags.
- `Transform::gamut_delta(rgb)` returns monitor/proof DeltaE76 before thresholding (proof zero when absent).
- `pipeline_cpu::OutputContext::resolve(settings)` validates the resolved proof handle and display/export policy for both backends.
- `pipeline_gpu::GpuManagedOutput` prepares shared ICC LUTs, transfer curves and preview warning distances, then executes tone/gamut/proof Output on GPU. `apply` returns `ManagedTile { pixels, gamut_warnings }`; `encode` returns resident RGB/mask buffers. `scene_settings` validates and removes only consumed proof state.
- `pipeline_gpu::ManagedRenderer` wires that immutable output selection into batched and resident Output and provides `render_region` and zero-pixel-readback `render_to_surface`. Isolated caches prevent cross-profile output collisions. See `crates/pipeline-gpu/OPERATORS.md` for precision and residency limits.
