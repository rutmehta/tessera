# ENG-1 CPU RED scaffolding — UNRUN

Base: `b1af2436`. Branch: `codex/eng-1-conditioning`. Date: 2026-10-01. Claude retains merge ownership.

Authored private cfg(test) CPU tests in `tone_extra.rs::eng1_conditioning_tests`; no product fix or shader/numerical changes. All tests are **UNRUN**, with no claimed RED failure, successful compilation, or GREEN result. Declined commit `635f69d8` was not used.

The coordinator explicitly selected **1e-4 absolute scene-linear RGB** as the adjacent-input-ULP/almost-no-op sensitivity target. This is distinct from the separate encoded Camera Raw resident CPU/GPU absolute 0.01 acceptance criterion. No epsilon or conditioning gain formula has been selected.

Coverage authored:

- Spatial 27x27 dyadic grayscale texture with a signed cancellation center, exact-zero and adjacent representable negative/positive G probes. Classification and non-neutral requested adjustment are asserted before behavior. Nonpositive center skip remains bit exact; sign-pair sensitivity scans the entire RGB image.
- Almost-no-op Texture control chosen by a bounded 1–4 encoded-ulp request witness, plus exact no-op and nonzero-but-rounded-no-change controls. Center response uses the chosen sensitivity ceiling. Guided witness fields are computed once per fixture, not once per search candidate.
- Every eligible positive-Y member of the fixed six-neighbor next-up/down R/G/B set for both a constructed cancellation center and the historical diagnostic vector. All eligible outputs are scanned before asserting the worst all-pixel delta. Historical input is not an Adobe output oracle.

The request witness intentionally reuses unchanged encode/guided/range helpers solely to prove branch activity; expected RGB never copies a proposed gain formula. Fixture-validity failure is not evidence of the intended conditioning RED. These tests first need compilation and baseline execution after lane release. Almost-no-op control may already pass baseline and guards against a future floor-induced regression.

Plans/review provenance: `origin/codex/perf-1-styles` at `eda37ee1`, files `tools/orchestrate/wp/ENG-1/{SOURCE-PLAN,INDEPENDENT-REVIEW,VALIDATION-PLAN}.md`; proposed fixture text was `/Volumes/betterSSD/tmp/eng1-red-fixture-proposal.md`. Current handoff records only authored source, not results from those plans.

Remaining: independent source review; observed RED/fixture evidence; policy selection; positive/darkening and Clarity-specific branch witnesses; deterministic sharpening→presence chain; both GPU shaders (presence.wgsl and tone_local.wgsl mode6); all Develop goldens and existing scene-linear tolerances; full resident encoded all-pixel <=0.01, exact alpha/neutral controls, and explicitly scoped 24MP validation. No GPU, golden, integration, performance or acceptance evidence exists for this branch.

Compiler HOLD: external batch17 parent PID72750 owns the runtime/Clippy lane. No Cargo/compiler/tests/GPU/GUI commands were run. Direct file rustfmt and git diff --check only. Do not begin runtime until coordinator release; no product implementation before observed RED and policy review.
