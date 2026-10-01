# ENG-1 validation plan (source-only)

Reviewed main `b07993a1125b49468e44e20a5aab3c718e86ccfe`, `docs/coordination/CODEX-BRIEF-2026-09-30.md`, and `origin/wp/B5-32` handoff `6fc9408c8a3cc5b760af27c20d51f81de6a8a1fb`. No builds/tests/GPU run; do not take declined B5-32 numerical commits `635f69d8`.

## Existing CPU Develop goldens

`crates/pipeline-cpu/tests/golden.rs::raw_fixture_goldens` runs CPU Develop on five checked-in CC0 RAW fixtures (`fixtures/raw/{canon-cr3.CR3,fuji-raf.RAF,nikon-nef.NEF,sample.dng,sony-arw.ARW}`) and compares to `fixtures/golden/*.png`. It explicitly disables lens profile and CA, uses `DevelopSettings::default()`, asserts expected dimensions and RGB8/8-bit PNG, and requires exact byte parity (`max == 0`, i.e. 0/255). Default texture and clarity are zero, so this suite protects unrelated default rendering and is not itself a presence-conditioning fixture. Keep the exact oracle and do not loosen or replace goldens to accommodate ENG-1.

Command (from repo root; fixtures default to `fixtures/raw`):

```sh
cargo test --release -p pipeline-cpu --test golden -- --exact raw_fixture_goldens --nocapture
```

No golden regeneration is warranted unless evidence shows this edit changes default Develop output; if that occurs, treat it as a regression to investigate.

## Camera Raw resident filter chain

`crates/filters/tests/camera_raw_gpu.rs::resident_chain_matches_cpu_across_tile_edges_and_preserves_alpha` is the narrow acceptance for the brief's absolute 0.01 target. It uses deterministic 259x263 encoded RGBA samples (partial tile edges, RGB values spanning 0..1, varied alpha), `rich_settings()` (custom WB, sharpening 57, luminance NR 17, exposure/contrast/highlights/shadows/whites/blacks, texture 12, clarity 8, dehaze 9, curves, color, vignette, grain, linear local adjustment), and amounts 0 / 0.35 / 1. CPU scalar-reference Camera Raw and resident GPU outputs are compared at every pixel; alpha is bit exact, all RGB finite, amount=0 is RGB bit exact. Existing checks are `max_scaled < 0.002` (absolute below unit magnitude, relative above) plus `max_absolute < 0.025`. After conditioning, tighten only the absolute assertion to `< 0.01`; retain scaled guard, all-pixel scan, finite and exact-alpha checks, and zero-amount identity. This test's samples are documented by the B5-28 comments as untagged sRGB encoded; label 0.01 as absolute encoded sample error, not docs/11 scene-linear tolerance.

```sh
cargo test --release -p filters --test camera_raw_gpu resident_chain_matches_cpu_across_tile_edges_and_preserves_alpha -- --exact --nocapture
```

The existing B5-32 `bright_value_stage_isolation_with_and_without_profile_curve` is valuable diagnostic coverage (259x263, sRGB curve and linear twin; neutral/sharpen/dehaze/rich-no-sharpen/rich-no-dehaze/rich-no-presence/rich matrix; every RGB pixel and exact alpha; reports one-ULP CPU perturbation). It is present on `origin/wp/B5-32`, not necessarily this main checkout. If adapted, preserve its tight unaffected-stage bounds (5e-6, 1e-4), and use it to confirm the new behavior is localized to rich+presence rather than broad pipeline precision changes. Do not copy its declined numerics implementation.

## Shared Develop presence/render regression

The CPU presence implementation is `crates/pipeline-cpu/src/tone_extra.rs::presence`; the GPU renderer uses `crates/pipeline-gpu/src/resident_tone.rs` plus `presence.wgsl`. Run the existing scene-linear L0 renderer gate because it exercises CFA demosaic → sharpening → tone presence, not only the Camera Raw filter wrapper:

```sh
cargo test --release -p pipeline-gpu --test local_tone_resident level_zero_resident_presence_matches_cpu_renderer -- --exact --nocapture
```

It covers synthetic RGGB and X-Trans 530x301, texture/clarity sign combinations from `cases()`, curves/vibrance/vignette, SceneLinear and Display outputs. Preserve scene-linear max abs `<= 2e-3`, display max channel delta `<= 1` code, and one final readback assertion. Keep existing CPU shape/extrema/finite tests, especially `tone_extra::tests::negative_presence_reduces_contrast_and_extremes_remain_finite` and `presence_halo_matches_overlapping_reference`; run targeted CPU unit tests after the change:

```sh
cargo test --release -p pipeline-cpu tone_extra::tests:: -- --nocapture
```

## 24 MP claim boundary

B5-32 `bench_24mp_cpu_gpu` is an ignored release-only 6000x4000 synthetic encoded-sample rich chain. It scans all RGB for finite values, checks alpha bit-exact, reports full-frame max and worst coordinate, but asserts `<0.01` only over the old every-997th-pixel sample. The B5-32 report measured sampled 0.00609684 but full-frame max 16.488846 at `(798,852)`, blue; so sampled parity is not evidence of all-pixel HDR parity. Run only after the runtime hold releases:

```sh
cargo test --release -p filters --test camera_raw_gpu bench_24mp_cpu_gpu -- --ignored --exact --nocapture --test-threads=1
```

If ENG-1 claims a full-frame absolute 0.01 Camera Raw bound, require `max_absolute < 0.01` over **all** RGB pixels in this diagnostic as well; retain sampled metric, finite checks and exact alpha. Do not silently call the old sampled-only result all-pixel acceptance. If the target is limited to the 259x263 resident acceptance test, explicitly state the 24 MP full-scan limitation rather than weakening or hiding it.

## Tolerance context

`docs/11-execution-plan.md` §1.3 says per-operator max absolute `1e-4` scene-linear and full-chain `2e-3` scene-linear plus at most 1 8-bit display code; these are distinct from the Camera Raw wrapper's encoded-sample diagnostic. Keep each original oracle and report its native units. For ENG-1, only the rich Camera Raw resident-chain absolute check should change from 0.025 to 0.01 after fix; all-pixel/finite/alpha/identity and scaled checks remain. No blanket relaxation of exact Develop PNG goldens or scene-linear tests.
