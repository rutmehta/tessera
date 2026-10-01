# ENG-1 conditioning plan: independent source review

Reviewed 2026-10-01 against main `b07993a1`, the ENG-1 brief and `SOURCE-PLAN.md`. Read-only source/algebra review; no repository edits, builds, tests, GPU, or implementation. Declined numerical commit `635f69d8` was not used.

## Blocking issue: flooring the total-gain denominator breaks near-identity continuity

The proposed `gain = target / max(Y, epsilon)` should NOT proceed unchanged. CPU `tone_extra.rs:227-240` and GPU `presence.wgsl:105-115` explicitly skip multiplication when the encoded adjusted value equals the original z. Therefore exact no-change still preserves RGB, but for any positive Y below epsilon an arbitrarily small nonzero presence change suddenly scales the ENTIRE RGB by approximately Y/epsilon.

Example in ideal arithmetic: Y=epsilon/10, target=Y+delta with delta approaching zero. Exact no-change takes identity gain1; an almost-no-change pixel takes gain approximately0.1. Signed cancelling channels can be substantial while Y is tiny, so this is a large RGB discontinuity, directly contrary to the brief's intent of tiny changes restricted to ill-conditioned behavior. CPU/GPU rounding can place one path on adjusted==z and the other on adjusted!=z; the proposal introduces a new amplification mechanism precisely at that branch. Ordinary constant-color/no-change tests alone will miss it because they take the skip branch.

At Y=epsilon itself, the max denominator is mathematically continuous, with a derivative kink; do not describe that floor threshold as a value discontinuity. The serious new discontinuity is around zero adjustment with 0<Y<epsilon. The existing Y<=0 skip boundary needs separate examination.

## Identity-preserving alternative to evaluate, not an approved semantic contract

`gain = 1 + (target - Y) / max(Y, epsilon)` conditions the *requested luminance change*, preserving the mathematical identity gain when target=Y. For Y>=epsilon it is algebraically target/Y, but a globally rewritten expression changes f32 arithmetic/order. To protect unaffected goldens, consider retaining the existing division for Y>=epsilon and testing a separate conditioned below-floor branch, with explicit CPU/GPU agreement at the boundary. This is a candidate for reviewed experiments, not proof of Adobe semantics or acceptance.

For 0<Y<epsilon in ideal arithmetic, output luminance becomes `Y + (Y/epsilon)*(target-Y)`: a blend toward target rather than an unrelated attenuation of source. With nonnegative target, gain is nonnegative, preserving relative channel signs unless floating saturation intervenes. This also attenuates intended low-luminance presence changes; that is a semantic choice requiring goldens and explicit approval. Encode/decode roundtrip error means target-Y may not be exactly zero numerically; keep the existing exact no-change skip and test one-ulp departures from it.

Neither formulation is a universal absolute gain bound: target can remain larger than epsilon, and signed RGB channel magnitudes are not bounded by small Y. The plan should say it bounds denominator sensitivity for positive near-zero Y, not that it caps output or gain to a specified constant. Choose and justify epsilon using observed failures and perceptual/golden evidence; the existing guided-filter variance epsilon is unrelated in units.

## Signed branch and remaining boundary risk

Preserving `Y<=0 -> unchanged` is consistent with the existing implementation and is narrower than introducing abs(Y) or processing negative luminance. However even the correction-gain candidate does not prove continuity across Y=0. The texture term can derive a nonzero target from neighboring structure as positive Y tends to zero, while exactly zero/negative Y still skips; limit gain can approach `1+target/epsilon`. Whether this produces a material signed-RGB jump must be tested with a spatial fixture and matching +/- one-ulp cancellation inputs. No claim of solved sign-boundary stability follows from merely replacing the divide.

## Missing GPU path in the narrow implementation list

There is another actual gain divide in `crates/pipeline-gpu/src/tone_local.wgsl:80-95`, mode6: `decode(adjusted)/lum`. `tone_local.rs:64` compiles that shader. The current plan's general request to verify all paths should explicitly list it alongside `presence.wgsl`. Updating only the resident shader plus CPU would leave tile/local GPU behavior numerically different. Keep independent parametric/luminance-curve divides out of this narrowly scoped fix unless separately justified.

## Required RED/acceptance corrections

- Before tests encode any chosen formula as expected output, add policy-independent no-op continuity checks: 0<Y<epsilon, substantial signed channels, and spatial texture/clarity fields causing adjusted z to move by just one representable step. Compare exact no-change and neighboring changed results. A test that copies the proposed denominator floor can approve the regression above.
- Test below/at/above epsilon, positive/zero/negative near-cancellation, both adjustment signs, neighborhood-range clipping, and a sharpening-to-presence chain. Include the historical vector only as diagnostic input, not a desired-pixel oracle.
- Retain existing above-floor arithmetic where possible; enumerate any deliberate behavior change. GPU encode/decode uses log/exp while CPU uses ln_1p/exp_m1, so matching source formulas alone does not establish matched output. ENG-1 asks for conditioning, not adoption of the declined precision changes.
- Exercise both GPU presence implementations and complete resident-chain all-pixel <=0.01 acceptance, plus every Develop golden. No result is claimed here. Candidate floor1e-3 remains unvalidated.

Recommendation: revise the conditioning choice and RED invariants before implementation. The direct total-gain denominator floor has an algebraically demonstrated near-identity regression; the correction-gain alternative merits controlled validation but is not yet an approved solution.
