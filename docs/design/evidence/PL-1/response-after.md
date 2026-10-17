# PL-1 — native slider response after calibration

Same fixture, harness and metrics as [response-before.md](response-before.md)
(`sample.dng`, scale 8; shadow quartile mean Y 0.0065 / L* 1.4, highlight
quartile mean Y 0.505 / L* 69.3). The table is asserted by
`native_slider_response_table` in `crates/pipeline-cpu/tests/pl1_response.rs`
(ranges, not exact values, so a fixture-decoder change does not trip it); the
single-luminance pins are asserted by `tone_gain_pins_the_stop_responses`
without any fixture.

## What changed

Basic tone (`pipeline_cpu::tone`, `tone_math::tone_gain`, and the twin
`tone_gain` in `pipeline-gpu/src/operators.wgsl`) is now a pure luminance
gain `G = exp(shift + stops·ln 2)` on the log axis `u = ln(Y/.18)`:

* **Contrast** `u1 = u + (c−1)·K·tanh(u/K)`, `c = 2^(contrast/100)`,
  `K = 1.5·ln 2`. Pivot log-slope is exactly `c` (×2 at +100, ×0.5 at −100),
  the slope returns to 1 away from grey, and the far shadows/highlights shift
  by at most ∓1.5 stops at +100 (±0.75 at −100). Black stays black. The old
  form put the strongest slope in the highlights and only ×1.31 at the pivot.
* **Blacks / Shadows / Highlights / Whites** each add
  `amount/100 · range · σ(±(u1 − centre·ln 2)/width)` stops: a logistic
  feather that saturates to a constant gain inside the band and to nothing
  outside it. Bands (range in stops at ±100, centre in stops from grey,
  width in natural-log units): Blacks 1.25 @ −3.5 (w .55); Shadows 1.5 @ −2.0
  (w .75); Highlights 1.6 @ +1.25 (w .55); Whites 1.0 @ +2.75 (w .6). The old
  lower regions saturated to a constant offset *above* their band, which is
  why Shadows moved highlights more than shadows.
* Response is linear in the amount by construction, so +20 is a fifth of
  +100 (Shadows +20 lifts Y=0.0065 by 0.28 stop; Highlights +20 lifts Y=1 by
  0.26 stop). Nothing is "multiplied by 5": the curve shapes are new, the
  gains were chosen against the reference, and the first 20 points are the
  same gentle slope as before but in the right band.
* Monotone: every feather derivative is bounded by `range/(4·width)`, the
  bands are spaced, and the combined log-slope stays ≥ 0.24 at the worst of
  the 48 slider-extreme combinations (Blacks/Shadows +100, Highlights/Whites
  −100, Contrast −100, at u ≈ 0.15 stop). `tone_extremes_remain_finite_and_monotone`
  (all 32 sign masks at +10 EV over 0..4 linear) still passes. No cancellation:
  the gain is an exponential of bounded terms, so the ENG-4 near-zero signed
  luminance test passes against an f64 reference of the new model at 1e-5.
* **Vibrance**: `muted = 1/(1+S²)` (Lorentzian in the relative saturation
  `S = C/(.25·L)`) instead of `1/(1+S)`, so colours below S≈0.3 take the
  whole boost and vivid ones little; and the skin-hue shield now ramps in
  with saturation, `.7·skin·(S/.3)²/(1+(S/.3)²)`, so a muted warm colour is
  no longer mistaken for skin. Saturation and Exposure are untouched.
* `previews::RENDER_EPOCH` 3 → 4 so on-disk previews rendered by the old
  engine are not served for edited recipes (the same mechanism ENG-7/7b and
  ENG-8 used).
* Approved SDR fingerprints in `pipeline-gpu/tests/hdr_surface.rs` were
  re-recorded for the two non-neutral cases on the same adapter (Apple M4
  Max); the neutral case is unchanged.

## Table (after)

| control | amount | shadow quartile (stops) | highlight quartile (stops) | log-contrast ratio | mean chroma ratio | muted-half chroma ratio | shadow dL* | highlight dL* |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| shadows | -100 | -1.379 | -0.096 | 1.214 | 0.912 | 0.796 | -1.1 | -1.7 |
| shadows | -50 | -0.689 | -0.048 | 1.107 | 0.953 | 0.890 | -0.7 | -0.9 |
| shadows | +50 | +0.688 | +0.048 | 0.894 | 1.052 | 1.127 | +1.5 | +0.9 |
| shadows | +100 | +1.375 | +0.096 | 0.788 | 1.111 | 1.273 | +4.3 | +1.7 |
| highlights | -100 | -0.001 | -0.744 | 0.883 | 0.898 | 0.995 | -0.0 | -11.9 |
| highlights | -50 | -0.001 | -0.372 | 0.940 | 0.946 | 0.998 | -0.0 | -5.5 |
| highlights | +50 | +0.001 | +0.372 | 1.062 | 1.062 | 1.002 | +0.0 | +4.7 |
| highlights | +100 | +0.001 | +0.745 | 1.127 | 1.132 | 1.005 | +0.0 | +8.7 |
| contrast | -100 | +0.748 | -0.405 | 0.820 | 1.016 | 1.166 | +1.7 | -6.1 |
| contrast | -50 | +0.438 | -0.237 | 0.893 | 1.007 | 1.094 | +0.8 | -3.5 |
| contrast | +50 | -0.621 | +0.336 | 1.155 | 1.000 | 0.882 | -0.7 | +4.3 |
| contrast | +100 | -1.501 | +0.811 | 1.382 | 1.022 | 0.740 | -1.1 | +9.3 |
| vibrance | -100 | +0.017 | +0.025 | 1.002 | 0.526 | 0.570 | +0.0 | +0.3 |
| vibrance | -50 | +0.009 | +0.013 | 1.001 | 0.763 | 0.785 | +0.0 | +0.2 |
| vibrance | +50 | -0.010 | -0.014 | 0.999 | 1.236 | 1.214 | -0.0 | -0.2 |
| vibrance | +100 | -0.020 | -0.028 | 0.998 | 1.472 | 1.426 | -0.0 | -0.4 |
| saturation | -100 | +0.024 | +0.046 | 1.004 | 0.000 | 0.000 | +0.0 | +0.6 |
| saturation | -50 | +0.017 | +0.025 | 1.002 | 0.508 | 0.526 | +0.0 | +0.3 |
| saturation | +50 | +0.006 | -0.028 | 0.994 | 1.516 | 1.564 | +0.0 | -0.4 |
| saturation | +100 | +0.068 | -0.061 | 0.979 | 2.004 | 2.027 | +0.1 | -0.8 |
| whites | -100 | -0.000 | -0.185 | 0.968 | 0.968 | 0.999 | -0.0 | -2.4 |
| whites | -50 | -0.000 | -0.092 | 0.984 | 0.983 | 1.000 | -0.0 | -1.1 |
| whites | +50 | +0.000 | +0.092 | 1.016 | 1.017 | 1.000 | +0.0 | +1.1 |
| whites | +100 | +0.000 | +0.185 | 1.033 | 1.036 | 1.001 | +0.0 | +2.0 |
| blacks | -100 | -0.994 | -0.005 | 1.165 | 0.962 | 0.882 | -0.9 | -0.1 |
| blacks | -50 | -0.497 | -0.003 | 1.082 | 0.980 | 0.937 | -0.5 | -0.0 |
| blacks | +50 | +0.499 | +0.003 | 0.919 | 1.022 | 1.071 | +0.9 | +0.0 |
| blacks | +100 | +0.999 | +0.005 | 0.840 | 1.048 | 1.151 | +2.4 | +0.1 |
| clarity | ±50, ±100 | 0.000 | 0.000 | 1.000 | 1.000 | 1.000 | 0.0 | 0.0 |

Single-colour vibrance probe (after):

| colour (linear Rec.2020) | OkLab L | S | vibrance +100 | vibrance +50 | vibrance -100 | saturation +100 |
|---|---:|---:|---:|---:|---:|---:|
| [0.19, 0.18, 0.17] warm grey | 0.57 | 0.06 | 1.97 | 1.49 | 0.03 | 2.00 |
| [0.21, 0.18, 0.15] pale warm | 0.57 | 0.17 | 1.80 | 1.40 | 0.20 | 2.00 |
| [0.16, 0.18, 0.22] pale blue | 0.56 | 0.17 | 1.97 | 1.49 | 0.03 | 2.00 |
| [0.12, 0.18, 0.28] blue | 0.55 | 0.47 | 1.82 | 1.41 | 0.18 | 2.00 |
| [0.3, 0.18, 0.1] skin-like | 0.60 | 0.54 | 1.36 | 1.18 | 0.64 | 2.00 |
| [0.1, 0.15, 0.4] vivid blue | 0.53 | 0.87 | 1.57 | 1.28 | 0.43 | 2.00 |
| [0.03, 0.02, 0.015] dark brown | 0.28 | 0.39 | 1.52 | 1.26 | 0.48 | 2.00 |
| [0.7, 0.6, 0.5] light warm | 0.86 | 0.17 | 1.80 | 1.40 | 0.20 | 2.00 |

Single-luminance pins of the tone model (stops of change at the given scene
luminance; 0.18 is mid grey):

| control, amount | Y=0.001 | 0.0065 | 0.02 | 0.05 | 0.18 | 0.5 | 1.0 | 4.0 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| Shadows +100 | +1.49 | +1.39 | +1.12 | +0.70 | +0.20 | +0.06 | +0.02 | 0.00 |
| Shadows +20 | +0.30 | +0.28 | +0.22 | +0.14 | +0.04 | +0.01 | 0.00 | 0.00 |
| Blacks −100 | −1.24 | −1.04 | −0.50 | −0.14 | −0.01 | 0.00 | 0.00 | 0.00 |
| Highlights −100 | 0.00 | 0.00 | −0.01 | −0.03 | −0.27 | −0.91 | −1.32 | −1.57 |
| Highlights +20 | 0.00 | 0.00 | 0.00 | +0.01 | +0.05 | +0.18 | +0.26 | +0.31 |
| Whites +100 | 0.00 | 0.00 | 0.00 | 0.00 | +0.04 | +0.19 | +0.42 | +0.88 |
| Contrast +100 | −1.50 | −1.49 | −1.46 | −1.26 | 0.00 | +1.13 | +1.39 | +1.49 |
| Contrast −100 | +0.75 | +0.75 | +0.73 | +0.63 | 0.00 | −0.57 | −0.70 | −0.75 |

## Before → after, in brief

| control, ±100 | before | after | reference |
|---|---|---|---|
| Shadows, shadow quartile | 0.19–0.22 stop (and 0.26 in highlights) | 1.38 stop (0.10 in highlights) | 1.5–2 stops in the band (1.4–1.5 at Y≤0.0065 here) |
| Highlights, highlight quartile | 0.15 stop | 0.74 stop (1.32 at Y=1, 1.57 at Y=4) | 1.5–2 stops in the band |
| Contrast, global log-contrast | ×1.20 / ×0.92 (pivot slope 1.31 / 0.85) | ×1.38 / ×0.82 (pivot slope 2.00 / 0.50) | ~2× mid-tone slope |
| Vibrance +100, mean chroma | ×1.32 (warm grey 1.30, blue 1.97) | ×1.47 (warm grey 1.97, skin 1.36, blue 1.97) | ~2× in muted colours, skin protected |
| Blacks, shadow quartile | 0.16–0.17 stop | 1.0 stop | — |
| Whites, highlight quartile | 0.07 stop | 0.19 stop (0.88 at Y=4) | — |
| Saturation, Exposure | unchanged | unchanged | at reference already |

The highlight quartile of this frame spans Y≈0.15…1 (mean 0.5, L* 69), so
its mean sits on the feather; the band proper (Y≥1) moves 1.3–1.6 stops.
Display L* changes are smaller than scene stops because the display sigmoid
(contrast 1.5) compresses both ends; that is the native look, unchanged here.

## Process-version decision: fix inside native revision 2

`CONTRACTS.md` §4 says a rendering change needs a new native revision. This
change stays in revision 2, with `previews::RENDER_EPOCH` bumped, because:

1. **A bump would not preserve old renderings.** When revision 1 became 2
   (`271c4232`) no revision-1 renderer was kept: "the operator entry points
   take settings, not a revision". The same would hold here — old recipes
   would still render with the new math — so the only effect of a bump is
   cache-key separation, which `RENDER_EPOCH` already provides for disk
   previews (ENG-7/7b and ENG-8 changed every default lens rendering this
   way, without a revision bump). In-process memo keys are per session.
2. **Blast radius.** "Native revision 2" is hard-coded as an admission gate
   in the pinned-RAW descriptor (`pinned_raw.rs`), smart previews
   (`smart_preview.rs`), the FFI's `set_process_version`, and ~70 references
   across crates, docs and tests. A bump would have to migrate or re-admit
   every existing recipe through those gates to avoid refusing them — far
   beyond a tone calibration, and outside this item's engine-only scope.
3. **It is a defect, not a look.** The old gains were set by the
   monotonicity bound ("each derivative contributes at most 0.2"), not
   calibrated; the OPERATORS.md section itself said "this is not Adobe's
   tone model"; and Shadows moved highlights more than shadows. The owner's
   report is a bug report about the control, and recipes that recorded
   Shadows +40 asked for a shadow lift they never got. Keeping their numbers
   and making them mean what Lightroom users expect is the stable outcome;
   freezing the broken response under a new number is not.
4. **Pre-release.** The product is in the 2026-10 redesign with owner
   test shoots; there is no shipped catalogue whose look must be frozen.

If the owner prefers a revision bump anyway, the operator change is
self-contained (`tone_math::tone_gain`, the WGSL twin, and the vibrance
lines), so the old code could be restored behind a revision switch; the
admission gates above would then need to accept both revisions.

## Tests

* `cargo test -p pipeline-cpu` (release, `--no-fail-fast`): green, including
  the new `pl1_response` (fixture response table, tone pins, vibrance probe)
  and the pre-existing `tone_extremes_remain_finite_and_monotone`,
  `neutral_tone_is_bit_exact_and_copy_on_write`, and the five RAW goldens
  (all-zero controls; unchanged).
* `cargo test -p pipeline-gpu`: green; CPU/GPU parity tests
  (`operators`, `rgb`, `renderer`, fusion, resident, export) pass unchanged
  at their 1e-4 / 1-LSB tolerances; the f64 near-zero reference was updated
  to the new model; SDR fingerprints re-recorded for the two non-neutral cases.
* `cargo test -p image-core`: green. `cargo test -p previews`: green.
* FFI archive untouched; `apps/mac` untouched.
