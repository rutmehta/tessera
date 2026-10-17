# PL-1 — native slider response before calibration

Owner's report (FOLLOWUPS item 33): "Exposure is the only slider that has a
true large range between −100 and +100; the other sliders (Shadows,
Highlights, Contrast, Vibrance), compared to Lightroom, feel like ±20."

Measured 2026-10-16 on `fixtures/raw/sample.dng` through the native CPU
pipeline (`pipeline_cpu::render_linear_scaled`, scale 8, lens profile off,
CA off, everything else default) at commit `fd7116a8`. The harness is
`crates/pipeline-cpu/tests/pl1_response.rs`
(`PL1_TABLE=1 cargo test -p pipeline-cpu --release --test pl1_response -- --nocapture`).

## Method

* Baseline render at all-zero controls. Pixels are ranked by Rec.2020
  luminance Y; the **shadow quartile** is the darkest 25% (mean Y 0.0065,
  display L* 1.4), the **highlight quartile** the brightest 25% (mean Y 0.505,
  display L* 69.3). Baseline log2 Y standard deviation 2.326; mean OkLab chroma
  0.0522, mean chroma of the less-saturated half of the pixels 0.0299.
* Per control and amount: mean of log2(Y'/Y) over the quartile ("stops"),
  ratio of the log2-Y standard deviation to baseline ("log-contrast ratio"),
  ratio of mean OkLab chroma to baseline (all pixels and the muted half), and
  the mean change in CIE L* of the display render (`render_scaled`, sRGB
  8-bit) over each quartile.

## Table (before)

| control | amount | shadow quartile (stops) | highlight quartile (stops) | log-contrast ratio | mean chroma ratio | muted-half chroma ratio | shadow dL* | highlight dL* |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| shadows | -100 | -0.218 | -0.279 | 0.990 | 0.942 | 0.951 | -0.3 | -4.5 |
| shadows | -50 | -0.105 | -0.137 | 0.995 | 0.971 | 0.976 | -0.1 | -2.2 |
| shadows | +50 | +0.098 | +0.133 | 1.006 | 1.029 | 1.023 | +0.2 | +2.0 |
| shadows | +100 | +0.190 | +0.263 | 1.012 | 1.057 | 1.046 | +0.3 | +4.0 |
| highlights | -100 | -0.056 | -0.158 | 0.983 | 0.970 | 0.986 | -0.1 | -2.3 |
| highlights | -50 | -0.028 | -0.078 | 0.991 | 0.985 | 0.993 | -0.0 | -1.1 |
| highlights | +50 | +0.027 | +0.077 | 1.009 | 1.015 | 1.007 | +0.0 | +1.1 |
| highlights | +100 | +0.054 | +0.153 | 1.017 | 1.030 | 1.014 | +0.1 | +2.1 |
| contrast | -100 | +0.244 | -0.267 | 0.916 | 0.975 | 1.048 | +0.4 | -3.6 |
| contrast | -50 | +0.148 | -0.154 | 0.950 | 0.985 | 1.029 | +0.2 | -2.0 |
| contrast | +50 | -0.237 | +0.211 | 1.076 | 1.022 | 0.957 | -0.3 | +2.3 |
| contrast | +100 | -0.654 | +0.497 | 1.197 | 1.056 | 0.888 | -0.7 | +4.9 |
| vibrance | -100 | +0.014 | +0.018 | 1.001 | 0.681 | 0.689 | +0.0 | +0.2 |
| vibrance | -50 | +0.007 | +0.009 | 1.001 | 0.840 | 0.844 | +0.0 | +0.1 |
| vibrance | +50 | -0.007 | -0.010 | 0.999 | 1.159 | 1.157 | -0.0 | -0.1 |
| vibrance | +100 | -0.014 | -0.020 | 0.999 | 1.318 | 1.312 | -0.0 | -0.2 |
| saturation | -100 | +0.024 | +0.046 | 1.004 | 0.000 | 0.000 | +0.0 | +0.6 |
| saturation | -50 | +0.017 | +0.025 | 1.002 | 0.508 | 0.526 | +0.0 | +0.3 |
| saturation | +50 | +0.006 | -0.028 | 0.994 | 1.516 | 1.564 | +0.0 | -0.4 |
| saturation | +100 | +0.068 | -0.061 | 0.979 | 2.004 | 2.027 | +0.1 | -0.8 |
| whites | -100 | -0.023 | -0.074 | 0.991 | 0.986 | 0.994 | -0.0 | -1.0 |
| whites | -50 | -0.011 | -0.037 | 0.996 | 0.993 | 0.997 | -0.0 | -0.5 |
| whites | +50 | +0.011 | +0.037 | 1.004 | 1.007 | 1.003 | +0.0 | +0.5 |
| whites | +100 | +0.023 | +0.073 | 1.009 | 1.014 | 1.006 | +0.0 | +1.0 |
| blacks | -100 | -0.174 | -0.210 | 0.994 | 0.955 | 0.960 | -0.2 | -3.4 |
| blacks | -50 | -0.085 | -0.104 | 0.997 | 0.978 | 0.981 | -0.1 | -1.6 |
| blacks | +50 | +0.080 | +0.101 | 1.003 | 1.022 | 1.019 | +0.1 | +1.6 |
| blacks | +100 | +0.156 | +0.201 | 1.007 | 1.044 | 1.037 | +0.3 | +3.1 |
| clarity | ±50, ±100 | 0.000 | 0.000 | 1.000 | 1.000 | 1.000 | 0.0 | 0.0 |

(The 0 rows are identity by construction and are omitted.)

Single-colour vibrance probe (`vibrance_probe_table`; S is the relative
saturation `chroma / (0.25 L)` the operator uses):

| colour (linear Rec.2020) | OkLab L | S | vibrance +100 | vibrance +50 | vibrance -100 | saturation +100 |
|---|---:|---:|---:|---:|---:|---:|
| [0.19, 0.18, 0.17] warm grey | 0.57 | 0.06 | 1.30 | 1.15 | 0.70 | 2.00 |
| [0.21, 0.18, 0.15] pale warm | 0.57 | 0.17 | 1.30 | 1.15 | 0.70 | 2.00 |
| [0.16, 0.18, 0.22] pale blue | 0.56 | 0.17 | 1.97 | 1.49 | 0.03 | 2.00 |
| [0.12, 0.18, 0.28] blue | 0.55 | 0.47 | 1.82 | 1.41 | 0.18 | 2.00 |
| [0.3, 0.18, 0.1] skin-like | 0.60 | 0.54 | 1.23 | 1.12 | 0.77 | 2.00 |
| [0.1, 0.15, 0.4] vivid blue | 0.53 | 0.87 | 1.57 | 1.28 | 0.43 | 2.00 |
| [0.03, 0.02, 0.015] dark brown | 0.28 | 0.39 | 1.31 | 1.16 | 0.69 | 2.00 |
| [0.7, 0.6, 0.5] light warm | 0.86 | 0.17 | 1.30 | 1.15 | 0.70 | 2.00 |

## Reading

* **Shadows ±100 moved the shadow quartile 0.19–0.22 stop** (0.3 L*), and
  moved the *highlight* quartile more (0.26–0.28 stop, 4–4.5 L*). The old
  region term `.2·amount·L(z,.8)` integrates to a constant offset in
  `z = ln(1+Y/.18)` above its band, which is a constant gain in the
  highlights; below the band it is a slope change of only `.2·σ(.8) ≈ .14`.
  So the control was a weak global exposure with a slight shadow emphasis.
* **Highlights ±100 moved the highlight quartile 0.15 stop** (2.1–2.3 L*).
* **Contrast ±100 changed global log-contrast by ×1.20 / ×0.92.** The pivot
  slope was `c + (1−c)·ln 2` = 1.31 / 0.85, not the nominal 2 / 0.5, and the
  slope *grew* towards the highlights (power law `Y^c` for large Y): an
  inverted S.
* **Vibrance +100 raised mean chroma ×1.32** against ×2.00 for Saturation
  +100. The probe shows why: the skin-hue shield (`1 − .7·skin`) fired on
  hue alone, so every warm colour — a warm grey at S=0.06 included — took
  1.30× while the same-saturation blue took 1.97×. This frame is
  warm-dominated.
* Whites/Blacks were likewise 0.07 / 0.16–0.2 stop in their bands.
* Clarity is a local-contrast operator (`tone_extra`); these global metrics
  do not see it at scale 8 and it was not part of the complaint. Saturation
  was already at the reference (2× at +100, 0 at −100) and is untouched.
* Exposure is untouched (+1 EV doubles samples exactly).

## Reference

`crates/pipeline-adobe` (`basic_tone`) implements the same controls for the
Adobe process emulation: Highlights/Shadows ±100 are ±1 stop feathered log
gains, Whites ±100 a ±0.5 stop global gain, Blacks ±100 ±1 stop below Y=0.08,
Contrast ±100 a `Y^(2^±0.5)` power (×1.41 / ×0.71 log slope). Lightroom's
published behaviour (used as the target because the Adobe emulation is itself
an approximation): Shadows/Highlights ±100 move their band by about 1.5–2
stops at the extreme, Contrast ±100 about doubles / halves the mid-tone
slope, Vibrance +100 roughly doubles chroma in muted colours and protects
saturated skin. Native was 5–10× below either reference on every control the
owner named, which confirms the "±20" impression as a measurement, not a
display-level or HDR-headroom artefact (the display L* columns scale with the
scene-linear columns, and the exposure-only path is unchanged).
