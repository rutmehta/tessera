# B5-28 handoff — Camera Raw Filter decodes/re-encodes the document transfer curve

Branch `wp/B5-28` from `origin/main` `031eaa56`. Changes only in `crates/filters` (src, tests, README) and the
Swift `DocumentCameraRawTests`. No FFI, compositor or format change; no version bump; Cargo.lock and board.json
untouched; no new dependencies.

## Behaviour change (read this first)
**Existing Camera Raw smart filters now re-render correctly, i.e. differently.** Document samples are encoded in the
document profile's transfer curve (untagged = sRGB-encoded); the filter used to treat them as linear (untagged as
linear sRGB). Example: +1 EV on encoded mid grey 0.5 was 1.0, is now 0.6858 (decoded light ×2). Stored filter
params are unchanged; only their rendering changes.

## Approach
- `camera_raw::profile_curves(&FilterContext) -> TransferCurves` (new, public). Curve source: color-mgmt/LCMS
  transform document profile -> its `linearized_rgb` twin (relative colorimetric, no BPC; same primaries so the
  matrix is identity and each channel is its TRC), sampled at **4096** uniform points per channel on [0,1].
  Untagged = `Builtin::Srgb`. Cached per ICC digest (16 entries).
- `decode` interpolates the table linearly; `encode` is the **exact inverse of that piecewise-linear decode**
  (binary search), so CPU and GPU use the identical curve and `encode(decode(x)) == x` up to f32.
- Float extension: below 0 point-symmetric about decode(0) (odd-symmetric for any TRC with 0 -> 0); above 1 the
  endpoint (last-interval) slope. Linear-TRC profiles (every sample within 1e-6 of identity, e.g. linear Rec.2020
  float documents) become `TransferCurves::identity()`: no arithmetic, bit-identical to the old matrix-only path.
  Non-finite, non-monotone (> 1e-6 decrease) or flat-at-white TRCs are rejected (`Unsupported`).
- CPU (`camera_raw.rs::evaluate`): decode before the forward matrix, encode after the inverse matrix. The curve
  table joins the Develop source identity hash (two profiles with equal primaries but different TRCs cannot alias).
- GPU (`camera_raw_gpu.rs`): table bound at bridge binding 4, flag + size in the existing uniform pad words;
  `unpack` decodes, `pack` encodes, with a WGSL mirror of `decode_channel` / `encode_channel`.
- `profile_matrices` keeps its signature (doc now says it is the linear-twin matrix).
- Cost: ~50 ns/px CPU for decode+encode (≈1.2 s single-threaded at 24 MP vs ≈22 s Develop). Not optimized.

## Amount
**Encoded space.** Amount is the filter's opacity and blends original vs developed-and-re-encoded samples, matching
the other document filters' fade semantics. Tested (`amount_blends_encoded_samples`).

## Tests
RED `8c46a9f6` (fails on main): `plus_one_ev_doubles_decoded_light_of_srgb_encoded_mid_grey` (got 1.0, ratio 4.67),
`amount_blends_encoded_samples` (0.75), `full_develop_matches_rgb_decode_golden_{srgb,display_p3,adobe_rgb}`
(e.g. sRGB (0,0) 0.1298 != 0.1112), GPU `resident_plus_one_ev_doubles_decoded_light_of_srgb_encoded_mid_grey`
(1.0). Also added, passing on main and after: `neutral_settings_are_identity_on_encoded_samples` (untagged, sRGB,
P3, Adobe RGB, linear Rec.2020; < 2e-4), `linear_profile_transfer_is_exact_identity` (bit-exact),
`resident_matches_cpu_evaluator_on_encoded_samples_across_profiles` (signed and > 1 samples).

Develop parity: the golden fixture now stores the PNG as ENCODED document samples (`RawImage::open` PNG ->
linear Rec.2020 -> document matrix -> document TRC via LCMS, independent of the filter's LUT) and compares the
filter against `Renderer::render_region_as(.., RenderOutput::SceneLinear)` of the same PNG, encoded the same way.
Tolerance 4e-4 encoded; measured max 4.9e-5 (sRGB), 4.7e-5 (P3), 5.3e-5 (Adobe RGB).
GPU vs CPU evaluator on encoded input: tolerance 0.002; measured ≤ 3.7e-5 (linear Rec.2020 3.3e-6).

Updated with justification (GREEN `dbf73f25` + Swift follow-up):
- `camera_raw.rs::in_memory_develop_slider_source_context_and_hdr_parity`: reference decodes/encodes with
  `profile_curves` (still bit-exact `assert_eq!`).
- `camera_raw_gpu.rs::cpu_reference`: decodes/encodes with `profile_curves`.
- `resident_chain_matches_cpu_across_tile_edges_and_preserves_alpha` and ignored `bench_24mp_cpu_gpu`: compare
  encoded samples; the 0.002 bound is unchanged in [-1,1] and **relative above |1|**. Decoded (darker) input drives
  a few rich-chain outliers (sharpening 57 + dehaze on a noise pattern) to ≈6.4 encoded, where CPU/GPU agree to
  ≈0.11 % relative (absolute 0.0072). Measured scaled max 0.00116. The bench passes (CPU 22 s, GPU 1.3 s).
- `compositor_adapter.rs::camera_raw_tone_on_raster_preserves_alpha`: expectation `x × 1.5` (linear) ->
  `x + 0.5·(enc(2·dec(x)) − x)` with sRGB curve and endpoint-slope extension above 1; < 2e-4.
- Swift `testNeutralIsIdentityAndExposureDoublesLinear` and `testReEditDetailPaneShowsTheFilterOnce`: ratios
  asserted on decoded samples (×2 once, ×4 stacked; the stacked filter no longer clips at 255).
- tessera-ffi Camera Raw tests (B5-18/18b/27): **no change needed**, all pass.

## Follow-ups (out of scope, not changed)
- `crates/tessera-ffi/src/document/filters.rs` ~3364 (B5-18b detail pane) sRGB-encodes samples via `srgb_u8` as if
  they were linear, while the canvas (`render.rs` `quantize`) shows them directly. The pane is therefore brighter
  than the canvas for 8/16-bit docs. Swift/ffi tests decode the pane to recover samples, which is why they still hold.
- `document_adaptive_ui::cancel_during_a_real_size_commit_stops_the_render_without_history` fails in `--release`
  on main as well (commit finishes in ≈0.31 s, before the 300 ms cancel); unrelated to this package.

## Gates
- `cargo test --release -p filters --no-fail-fast`: all pass (28 binaries).
- `cargo test --release -p tessera-ffi --no-fail-fast`: all pass except the pre-existing release-only AWA cancel test above.
- `cargo clippy --release -p filters --all-targets -- -D warnings`: clean. `cargo fmt --all --check`: clean.
- `apps/mac/build-ffi.sh`: OK, bindings unchanged. `tools/orchestrate/swift-gate.sh`: **SWIFT GATE OK** (855 XCTest, 3 skipped, 0 failures).
