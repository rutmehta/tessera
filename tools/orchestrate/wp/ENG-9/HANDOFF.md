# ENG-9: export and print render what Develop shows

Base: `origin/main` `f77aae5d`. Synthetic fixtures only. No board.json or
Cargo.lock change. Source finding: REV2-SP NS1 and the SP-INT HANDOFF NS1 row
("Adobe-process originals export through the Native pipeline").

## What was wrong on main

1. Export, print and documents had no process dispatch for originals.
   Adobe-process recipes (imported Lightroom edits) on RAW and RGB originals
   were rendered by the **Native** pipeline. Only external Smart Preview
   proxies went through `pipeline_adobe`.
2. The Adobe Output stage in Develop (`AdobeStageOp` →
   `CpuStageOp::display_linear`) always hard-clipped to sRGB. Export honours
   `output.gamut_mapping` (Perceptual by default).
3. The Develop EDR viewport (`RenderOutput::DisplayLinear`, float ring) failed
   for every Adobe-process original: the Adobe stage returned U8 tiles to a
   float output ("tile is U8, requested F32Planar"). Proxies had their own
   clamp-only EDR path.
4. HDR export of an Adobe-process recipe (original or proxy) silently
   rendered the Native pipeline with the Native HDR sigmoid.

## Gamut policy (decision)

Develop honours the recipe's `output.gamut_mapping` exactly as export does
("show what you will get"). This is possible on the display path: the
mapping is a per-pixel point operation, so nothing prevents it.

- One implementation, `pipeline_cpu::map_gamut(v, y, gamut, peak)`.
  - `Clip` clamps each channel. This is bit-identical to the old Adobe
    Output stage.
  - `Perceptual` keeps hue and luminance and compresses chroma toward the
    grey point until every channel fits. The grey point is the
    **working-space (Rec.2020) luminance**, computed before conversion, the
    same one the managed export transform uses.
- Callers:
  - `CpuStageOp::adobe_display`: the Adobe Output stage for originals and
    proxies, both SDR (8-bit, no dither) and EDR (`[0, headroom]`).
  - `pipeline_adobe::render_scaled*`: CLI `render` and the lrcat fidelity
    tool. This keeps `lrcat_linear`'s Develop-equals-`render_scaled_with_profile`
    contract.
- Native Develop is **unchanged**. It already gamut-maps, but its Output
  stage (CPU `pipeline_cpu::display*` and the Metal `display` kernels) takes
  the grey point from 4-decimal sRGB coefficients after conversion. That
  coefficient rounding moves saturated Perceptual pixels by up to 0.093
  level beyond the quantisation bound on these fixtures, identical on main.
  Aligning it would move Native Develop pixels on every backend (and Native
  goldens), so it is left as a follow-up lane (see "Not done").
- The superseded SP-INT2 ruling ("proxies keep main's hard clip") is replaced
  for originals and proxies alike. `sp_int2_gamut.rs` now asserts the new
  policy: Clip is still a hard clip, and Perceptual Develop equals print.

## Entry points changed

All file, print and MCP outputs funnel into two export-crate functions. Both
now dispatch on the recipe's process.

| Entry point | Path | Change |
| --- | --- | --- |
| File export: FFI `Engine::export` (`render_one_cancellable`), `export_one*`, `export_batch*` (CLI `tessera export`), MCP `export` tool (`export_one`) | `export::render_one_cancellable` | `uses_develop_renderer`: every Adobe recipe (any source) renders through `ai_masks::render_develop` (formerly `render_proxy`), then the managed output transform. DNG gets the display-referred linear float, and upscale applies before output, as for proxies |
| Print (FFI `print_image`), documents (`document/io.rs`), mask previews (`render_pixels_with_mask_support`) | `export::render_pixels_with_notes` | Same dispatch |
| HDR export (PNG16 / AVIF 10/12 PQ/HLG) | `export::hdr::render` | Adobe: `render_develop` output with no Native sigmoid, gamut-mapped into `[0, headroom]` (Develop's EDR rendition), then the existing transfer encode |
| CPU helpers `render_scaled_cpu`, `render_full_float` | `export/src/lib.rs` | Adobe on any source (was external proxies only) |
| Resident GPU export | `export/src/gpu.rs` | Unchanged: it already declines everything except `NATIVE_CURRENT`, so Adobe never reaches it |
| Develop Output stage, originals | `image-core/src/adobe.rs` | `Op::Display { gamut, headroom }` → `CpuStageOp::adobe_display` (gamut policy, EDR fixed) |
| Develop Output stage, proxies | `image-core/src/smart_preview_render.rs` | Same function (was hard clip SDR, clamp-only EDR) |
| `pipeline_adobe::render_scaled*` | `pipeline-adobe/src/render.rs` | `encode` uses `map_gamut` with the recipe's mapping |

`render_develop` carries Develop's resources for originals too:
- the local-mask hook (AI and imported rasters through `ready_masks`);
- Lens Blur depth (support root, or `depth::support()` for originals);
- retouch;
- neural post-demosaic denoise. This uses the new
  `pipeline_adobe::render_linear_scaled_with_denoiser`, and
  `depth::denoiser` is factored out of `depth::render`. Before, the Adobe
  pipeline had no denoiser input, so Adobe RAW + Denoise would have failed
  with "no post-demosaic denoiser injected".

## Parity numbers (tests: `crates/export/tests/eng9_develop_parity.rs`)

How they are measured:
- Develop is the `Renderer` SDR Output stage (8-bit).
- Export is a 16-bit sRGB TIFF file through `export_one`.
- Print is `render_pixels_with_notes` floats, in sRGB and in Display P3.
  P3 is the FFI print default; it is converted back to sRGB for comparison.
- Values are |Develop − output| in 8-bit sRGB levels, at full resolution,
  orientation 1, no resize or sharpening.

Fixtures, all synthetic:
- a 48×32 saturated Bayer RAW;
- a 40×28 linear Rec.2020 RGB image with colours outside sRGB;
- the repo's `linear-gradient-jxl.dng` Smart Preview.

Recipes:
- In gamut: saturation −100.
- Saturated: saturation +40, out of sRGB (asserted).

Tolerance (quantisation only):
- 0.5 level for 8-bit rounding;
- 0.47 for Native's ordered dither;
- 0.002 for TIFF16;
- 0.03 for float order, ICC against matrix, and the 2^-18 chroma search.

That gives max 0.53 (Adobe) and 1.0 (Native), mean ≤ 0.35. One exception,
Native saturated Perceptual only, allows +0.1 (`NATIVE_GREY_POINT`, grey
point above).

| Case (max / mean levels) | Before (main) | After |
| --- | --- | --- |
| RAW Adobe in gamut, export / print sRGB / print P3 | 32.83 / 10.01 (all three) | 0.502 / 0.253 (P3 0.504) |
| RAW Adobe saturated, Perceptual | 255 / 43.93 | 0.529 / 0.177 |
| RAW Adobe saturated, Clip | 255 / 10.51 | 0.518 / 0.079 |
| RGB Adobe in gamut | 18.63 / 9.45 | 0.502 / 0.244 |
| RGB Adobe saturated, Perceptual | 134.48 / 27.70 | 0.511 / 0.189 |
| RGB Adobe saturated, Clip | 79.88 / 14.76 | 0.502 / 0.149 |
| Proxy Adobe in gamut | 0.502 / 0.251 | 0.502 / 0.251 |
| Proxy Adobe saturated, Perceptual | 187.94 / 26.27 | 0.502 / 0.179 |
| Proxy Adobe saturated, Clip | 0.502 / 0.143 | 0.502 / 0.143 |
| Native, all sources, in gamut and saturated Clip | ≤ 0.967 | unchanged |
| Native RAW saturated Perceptual | 1.023 / 0.231 | unchanged (grey point) |
| Gradient mask + crop, Adobe RAW / RGB / proxy | (added after the fix) | 0.518 / 0.504 / 0.498 |
| Gradient mask + crop, Native RAW / RGB / proxy | (added after the fix) | 1.094 / 0.965 / 0.954 |
| HDR PQ export vs Develop EDR (headroom 2), Adobe RAW / RGB / proxy, 16-bit codes | EDR errored; export rendered Native (differs by 5453 / 1775 / 1766 codes) | 0.502 / 0.502 / 0.504 |

The Adobe-process HDR file holds the Adobe rendition, as Develop's EDR
viewport does. The Adobe pipeline is display-referred, so nothing exceeds
SDR white. This is reported, not a silent fallback.

## Item table (finding → code → test)

| Finding | Code | Test |
| --- | --- | --- |
| Adobe originals export/print through Native | `export/src/lib.rs` `uses_develop_renderer`, `render_one_cancellable`, `render_pixels_with_notes`, `render_scaled_cpu`, `render_full_float`; `ai_masks::render_develop` | `eng9_raw_original_…`, `eng9_rgb_original_…`, `eng9_adobe_export_is_not_the_native_rendering`, `eng9_local_adjustments_and_crop_…` (RED → GREEN, except the last, which was added after) |
| Develop hard-clips while export gamut-maps | `pipeline_cpu::map_gamut`, `CpuStageOp::adobe_display`, `pipeline_adobe` `encode` | `image-core/tests/adobe.rs::eng9_adobe_display_honours_gamut_mapping_and_draws_edr` (RED → GREEN), the saturated rows above, `sp_int2_gamut.rs` (policy updated) |
| Adobe EDR viewport errors | `adobe.rs` `Op::Display { headroom: Some }` | same image-core test (RED: format mismatch) |
| HDR export of Adobe recipes silently Native | `export/src/hdr.rs` | `eng9_adobe_hdr_export_matches_develop_edr` (RED → GREEN) |
| Adobe + neural denoise on RAW | `pipeline_adobe::render_linear_scaled_with_denoiser`, `depth::denoiser` | `image-core/tests/adobe.rs::eng9_adobe_denoise_matches_develop_renderer` |

## Golden audit

No golden or fingerprint changed and none was re-pinned. The full workspace
run is green with no golden edits (`git diff f77aae5d` touches no golden or
fingerprint file).

- Expected candidates were export goldens for Adobe-process recipes. There
  are none in the repo: existing Adobe export tests (`lr13b_proxy_masks`,
  `sp_int2_*`, `smart_preview_admission`, `lr8m_*`) already exported
  proxies through `pipeline_adobe`.
- Clip outputs of the Adobe Output stage are bit-identical to main.

Behaviour changes without a golden:
- Adobe originals' export, print and HDR pixels (the fix).
- Adobe Develop, proxy and `pipeline_adobe::render_scaled` pixels for
  out-of-sRGB colours under Perceptual. This is the default mapping, so
  Lightroom-imported photos with saturated colours now display chroma-
  compressed instead of clipped, in Develop as in export.
- Adobe EDR viewport: works instead of erroring.

One existing test changed its asserted policy:
`sp_int2_gamut.rs::sp_int2_saturated_proxy_develop_clips_like_an_adobe_original`
is renamed `…_develop_uses_the_adobe_original_output_stage`. Its Clip
assertion is unchanged. Its Perceptual assertion moves from "hard clip" to
"chroma compression with the Rec.2020 grey point", and the print test gains
a Perceptual Develop-equals-print check (max ≤ 2, mean ≤ 0.6, the file's
existing bound).

## Gates

Run on the final code tip after `cargo clean --release -p export -p
image-core -p pipeline-adobe -p pipeline-cpu`. Load average was 30.7 at the
start and 22.3 at the end.

| Gate | Result |
| --- | --- |
| `cargo test --release --workspace --no-fail-fast` | 3504 passed, 0 failed, 108 ignored (exit 0). The only later edit is clippy's `as_chunks` in two test helpers of `eng9_develop_parity.rs`; that file was rerun: 6/6 |
| `cargo clippy --release --workspace --all-targets -- -D warnings` | clean (after the `as_chunks` fix) |
| `cargo fmt --all -- --check` | clean |
| `apps/mac/build-ffi.sh` | OK, worktree clean (no bindings drift) |
| `tools/orchestrate/swift-gate.sh` | SWIFT GATE OK (996 XCTest, 3 skipped, 0 failures; 5 Swift Testing tests passed) |
| `swift build -c release --product Tessera -Xswiftc -strict-concurrency=complete -Xswiftc -warnings-as-errors` | Build complete |

## Not done / follow-ups

- **Native grey point (follow-up lane).** Native's Output stage (CPU
  `display`, `display_float`, `display_linear` and the Metal
  `display`/`display_linear` in `operators.wgsl`) should take its
  Perceptual grey point from the Rec.2020 luminance, as export and the Adobe
  stage now do. The difference is ≤ 0.1 level today, and the change moves
  Native goldens.
- **Performance.** Adobe-process exports render on the CPU compatibility
  pipeline. This was also true on main (the resident GPU export declined
  them), but it now develops the base at full resolution even for
  `render_scale` 2–8 (print), as Smart Previews already did. Not measured on
  large RAWs.
- **Not exercised end-to-end.** Neural denoise for Adobe RAW exports is
  covered only at the renderer level (a fake denoiser); exports load the
  real model from the support directory. AI-mask originals on Adobe go
  through `ready_masks`, which is the same code that already serves proxies.
