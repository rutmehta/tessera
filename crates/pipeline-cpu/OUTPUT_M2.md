# M2-15 managed CPU Output

`render_managed_scaled(settings, source, scale, &mut OutputContext)` is an
explicit, synchronous end-to-end CPU render entry point. It runs the same
scene-linear stages through Geometry/downsampling as `render_linear_scaled`,
then the default luminance sigmoid and `color_mgmt::Transform` from
`Builtin::LinearRec2020` directly to the selected RGB ICC profile. It returns
`ManagedOutput { pixels: Rgb32FImage, gamut_warnings: Vec<GamutWarning> }`.
Pixels are target-encoded SDR [0,1]; warning flags are row-major, computed
before gamut mapping, and never painted into pixels.

The caller owns `color_mgmt::Registry` and retains profile Arcs from `builtin`,
`load_file`, `load_bytes`, or `display_profiles`. `OutputContext` borrows that
registry and the target profile (`OutputTarget::Display` or `::Export`). No
monitor, file or machine-global profile is silently selected. For example:

```rust,ignore
let mut registry = color_mgmt::Registry::new();
let target = registry.builtin(color_mgmt::Builtin::DisplayP3)?;
let output = pipeline_cpu::render_managed_scaled(
    &settings, &source, 1,
    &mut pipeline_cpu::OutputContext {
        registry: &mut registry,
        target: pipeline_cpu::OutputTarget::Display(&target),
        proof: None,
        options: color_mgmt::TransformOptions::default(),
    },
)?;
```

## Proofs and settings

Set `settings.output.proof_profile` with
`IccProfileHandle::from_profile_bytes(proof.icc_bytes())` and supply the resolved
profile as `context.proof`. Missing, mismatched or unexpected proof profiles
are errors. Engine handles use domain-separated BLAKE3; they are **not** the
raw BLAKE3 `color_mgmt::Profile::digest()`. The adapter validates actual bytes
using the engine constructor rather than conflating these identifiers.

The shared `Transform::proof` receives context intent, black-point compensation,
paper simulation and gamut threshold. Proof copies/history remain caller-owned.
Export targets reject active proof state to avoid accidentally baking monitor
simulation into a document; clear both proof fields to render an actual export.
Legacy `render`, `render_scaled`, `display`, and their validation/golden behavior
are unchanged. They still reject proof settings; select the managed API explicitly.

`GamutMapping::Clip` bounds target channels. `Perceptual` searches a constant
linear-working-luminance chroma ray toward neutral until the ICC target RGB
is in bounds. This is a pragmatic RGB compression, **not** an Oklab/JzAzBz
hue-preserving mapper or a guarantee for non-monotonic printer LUT boundaries.
The CMM rendering intent is independently selected through context options.

## Deliberate limits

- SDR RGB destinations only; HDR/headroom controls remain errors, not silently
  tone-mapped HDR. No EDR/PQ/HLG swapchain, OS transform bypass, 10-bit surface,
  calibration hardware or UI is implemented here.
- Output context is not serialized or included in engine stage hashes. A caller
  caching managed output must include target/proof ICC identity and every
  transform option in its own cache key.
- Engine OutputSettings lacks target/display identity, intent, BPC, paper/ink
  simulation, warning controls and threshold. These are explicit context options
  rather than invented recipe fields. See MISSING_FIELDS.md.
- Managed output is floating point; presentation/encoding and any quantization
  dither are caller-owned. The old 8-bit ordered-dither path is preserved.
- The file exporter consumes this managed float entry point directly, then
  resizes/sharpens destination-encoded floats and quantizes only in the codec.
- Scalar CMM warnings and boundary searches favor reference correctness over
  throughput; no transform cache or managed-output benchmark is claimed.

Tests compare managed render pixels and flags against shared direct/proof CMM
transforms, exercise partial-edge downsampling, profile-handle failures,
export-proof rejection, HDR rejection and clipping/compression. Existing raw
fixture goldens remain unchanged.
