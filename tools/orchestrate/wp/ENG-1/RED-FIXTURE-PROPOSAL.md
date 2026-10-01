# ENG-1 proposed synthetic RED fixtures (UNAUTHORED / UNRUN)

2026-10-01. Read tracked ENG-1 SOURCE-PLAN.md and VALIDATION-PLAN.md in `codex-perf-1-styles`, plus current CPU presence source. No repository edits, execution, compilation, GPU, or declined numerical commits. The snippets below are a concrete proposal for insertion into the existing private `tone_extra.rs::tests` module after review, not a claim that tests compile or fail.

## Fixture and assertion contract

Use a 27x27 planar RGB image: deterministic positive grayscale spatial texture, with one center pixel replaced by substantial signed channels whose weighted luminance cancels. Center `(13,13)` is away from radius8 direct boundaries; the two guided means can still reach boundaries, and CPU/GPU must use identical whole-image geometry. Do not crop one GPU version differently.

Start with `[0.678_f32, -0.2627_f32, 0.0]`. Under the existing multiply/add order, its first two weighted products have identical magnitudes and opposite signs; their sum is exactly zero. Adjacent representable G values yield positive/negative cancellation probes without relying on a guessed decimal luminance. Assert measured classification using the current luma helper; if an architecture/compiler changes the classification, fail fixture setup explicitly rather than silently skip. GPU classification must be observed separately because contraction/reassociation can matter.

These are substantial signed channels (~0.7), not tiny grayscale values that hide a large RGB gain. A dark center surrounded by lighter structured pixels, combined with negative Texture, should request positive luminance at center; verify that request explicitly before evaluating output. Run Texture-only, Clarity-only and combined settings, but do not claim every sign/configuration must be a witness: assert one known-active configuration first and report the rest as distinct cases once validated.

Proposed source-level continuity ceiling is **1e-4 absolute scene-linear RGB** for an adjacent-input-ulp pair or an almost-no-op adjustment. This is a new sensitivity assertion, not the Camera Raw encoded 0.01 parity threshold and not automatically mandated by existing per-operator numeric accuracy tolerances. Parent must approve this ceiling before making it a product gate. Print actual input/output maximum deltas and worst channel. It does not prescribe epsilon or any gain formula; it limits a visibly large response to an infinitesimal perturbation. If physics/contract demands a different response near signed cancellation, resolve that semantic conflict rather than tune a floor to pass mechanically.

## Proposed helpers

```rust
const SIDE: usize = 27;
const N: usize = SIDE * SIDE;
const CENTER: usize = 13 * SIDE + 13;
const CONTINUITY_LIMIT: f32 = 1e-4; // Proposed policy bound; review before adoption.

fn cancellation_scene(center: [f32; 3]) -> Vec<f32> {
    let mut out = vec![0.0; 3 * N];
    for y in 0..SIDE {
        for x in 0..SIDE {
            // Four dyadic levels; genuine fine/mid-scale spatial variation.
            let level = 0.125 + ((3 * x + 5 * y) % 4) as f32 * 0.0625;
            for c in 0..3 {
                out[c * N + y * SIDE + x] = level;
            }
        }
    }
    for c in 0..3 { out[c * N + CENTER] = center[c]; }
    out
}
fn center_rgb(data: &[f32]) -> [f32; 3] {
    [data[CENTER], data[N + CENTER], data[2 * N + CENTER]]
}
fn render_presence(input: &[f32], texture: f32, clarity: f32) -> Vec<f32> {
    let mut out = input.to_vec();
    presence(&mut out, SIDE, SIDE, &ToneSettings {
        texture, clarity, ..ToneSettings::default()
    });
    assert!(out.iter().all(|x| x.is_finite()));
    out
}
fn cancellation_triplet() -> [[f32; 3]; 3] {
    let zero = [0.678_f32, -0.2627_f32, 0.0];
    let below = [zero[0], zero[1].next_down(), zero[2]];
    let above = [zero[0], zero[1].next_up(), zero[2]];
    assert_eq!(luma(zero), 0.0, "fixture must cancel in existing luma order");
    assert!(luma(below) < 0.0, "negative fixture classification");
    assert!(luma(above) > 0.0, "positive fixture classification");
    [below, zero, above]
}
fn assert_center_close(a: &[f32], b: &[f32], context: &str) {
    let aa = center_rgb(a);
    let bb = center_rgb(b);
    for c in 0..3 {
        let d = (aa[c] - bb[c]).abs();
        assert!(d <= CONTINUITY_LIMIT,
            "{context}: channel={c}, a={}, b={}, delta={d}", aa[c], bb[c]);
    }
}
```

Fixture RGB storage and center extraction match presence's channel-planar layout. No alpha channel is passed to this private RGB helper; exact alpha belongs in the existing image/Camera Raw wrapper tests, not a fictitious fourth presence plane.

## Separate target witness from output oracle

To avoid a vacuous test where range clipping keeps `adjusted==z`, compute a **branch witness only**, never expected RGB, from the existing neighborhood helpers before applying presence. This intentionally reuses guided/range/encode to observe the unchanged upstream request and is NOT an independent oracle for those filters. It contains no candidate denominator floor or gain expression.

```rust
fn center_request(input: &[f32], texture: f32, clarity: f32) -> (f32, f32) {
    let z: Vec<f32> = (0..N).map(|i| {
        encode(luma([input[i], input[N+i], input[2*N+i]]).max(0.0))
    }).collect();
    let fine = guided(&z, &z, SIDE, SIDE, 1, 0.001);
    let mid = guided(&z, &z, SIDE, SIDE, 3, 0.001);
    let wide = if clarity != 0.0 {
        guided(&z, &z, SIDE, SIDE, 8, 0.001)
    } else { mid.clone() };
    let (lo, hi) = range(&z, SIDE, SIDE, 1);
    let v = z[CENTER];
    let t = v.clamp(0.0, 1.0);
    let weight = 4.0 * t * (1.0 - t);
    let delta = texture.clamp(-100.0, 100.0) / 100.0 * (fine[CENTER] - mid[CENTER])
        + clarity.clamp(-100.0, 100.0) / 100.0 * weight * (mid[CENTER] - wide[CENTER]);
    (v, (v + delta).clamp(lo[CENTER], hi[CENTER]))
}
```

Keep the witness helper frozen for this investigation; do not weaken `adjusted != z` or change settings silently if an assertion fails. A failing fixture-validity assertion is **not** observed RED evidence for conditioning.

## Test A: crossing the current zero/sign branch

```rust
#[test]
fn presence_is_stable_across_one_ulp_luminance_sign_crossing() {
    let [negative, zero, positive] = cancellation_triplet();
    let neg_input = cancellation_scene(negative);
    let zero_input = cancellation_scene(zero);
    let pos_input = cancellation_scene(positive);
    let (z, requested) = center_request(&pos_input, -100.0, 0.0);
    assert!(requested > z, "fixture must request a non-neutral positive adjustment");
    let neg = render_presence(&neg_input, -100.0, 0.0);
    let neutral = render_presence(&zero_input, -100.0, 0.0);
    let pos = render_presence(&pos_input, -100.0, 0.0);
    // Preserve the current nonpositive-luma behavior as its own explicit contract.
    assert_eq!(center_rgb(&neg).map(f32::to_bits), negative.map(f32::to_bits));
    assert_eq!(center_rgb(&neutral).map(f32::to_bits), zero.map(f32::to_bits));
    assert_center_close(&neg, &neutral, "negative to exact-zero input");
    assert_center_close(&neutral, &pos, "zero to next-up positive input");
}
```

This can expose that preserving the nonpositive skip plus simple correction-gain floor still leaves a jump when target remains nonzero. It deliberately does not assume either proposed formula solves the boundary. If rejected as too strong a rendering contract, retain measured diagnostic output and make the explicit policy decision before implementation.

## Test B: almost-no-op versus exact no-op (positive near-cancellation)

Use `positive` from the triplet. Compare zero Texture to a negative Texture magnitude selected solely by its encoded request being 1–4 ulps above z. Search a fixed bounded set of control magnitudes, not a selected output that happens to pass. This accommodates the guided upstream scale without adopting any conditioning epsilon.

```rust
#[test]
fn almost_noop_presence_does_not_rescale_signed_rgb_abruptly() {
    let positive = cancellation_triplet()[2];
    let input = cancellation_scene(positive);
    let neutral = render_presence(&input, 0.0, 0.0);
    assert_eq!(neutral, input, "zero controls must retain exact RGB");
    let (z, _) = center_request(&input, 0.0, 0.0);
    assert!(z > 0.0);
    let ulp = z.next_up() - z;
    let texture = (0..=96).find_map(|k| {
        let t = -100.0 * 2.0_f32.powi(-k);
        let (_, adjusted) = center_request(&input, t, 0.0);
        let delta = adjusted - z;
        (delta > 0.0 && delta <= 4.0 * ulp).then_some(t)
    }).expect("fixture must yield a changed request within four encoded ulps");
    let changed = render_presence(&input, texture, 0.0);
    assert_center_close(&neutral, &changed, "exact versus almost no-op adjustment");
}
```

The bounded search should be optimized in authored tests by computing center `fine-mid`, z, lo and hi once, then selecting the magnitude with the same operation order. As written it is concrete and bounded but redundantly computes guided fields up to97 times. The current implementation might pass this particular almost-no-op control; its role is to block the **new** discontinuity introduced by total-target flooring. Do not insist every useful regression test fails on the baseline. Pair it with Test A/adjacent-input sensitivity evidence for observed RED.

Also add a nearby exactly-no-change witness (one or more halvings of the chosen magnitude until `adjusted==z`) and assert exact center bit identity. Test positive control sign separately using a positive near-cancellation center that is not the local minimum, otherwise no-new-extrema clipping trivially suppresses the requested darkening. Do not call a clipped case evidence for both signs.

## Test C: strictly positive adjacent input, plus historical diagnostic input

After fixture validation, add positive pairs `positive` and `[positive[0], positive[1].next_up(), positive[2]]` under the same spatial neighborhood/settings and require both measured lumas>0. Compare their center output delta against the reviewed continuity ceiling, recording the actual input delta. This isolates divide conditioning from the Y<=0 branch. Repeat with `[-0.006959494,-0.005868249,0.097931265]` and each channel's next-up/down neighbors only when both measured lumas stay positive; assert at least one eligible pair was exercised. The historical center has Y around3.924e-7 according to B5-32, but recompute it and never encode that report value as expected output.

Do not cherry-pick only successful neighboring perturbations: scan the fixed R/G/B six-neighbor set, report/compare every eligible pair, and retain the worst coordinate/channel. The full image may have propagated neighborhood differences as well; add an all-pixel maximum report/check after the center-focused test is established, using the same approved scene-linear bound.

## Remaining required coverage

- These direct-presence fixtures establish sensitivity controls, not a detail→presence reproduction. Add a separate deterministically sharpened spatial image and capture the actual signed/cancellation pixel before presence; do not claim manual signed input proves sharpening integration.
- Both GPU shaders (`presence.wgsl`, `tone_local.wgsl` mode6) must receive identical fixtures after the runtime lane opens. Exact CPU zero cancellation may classify differently on GPU: record the values and treat that as relevant evidence, not an instruction to change precision or revive declined commits.
- Retain all existing Develop goldens and scene-linear operator/render bounds; require all-pixel resident Camera Raw encoded <=0.01 acceptance separately. No inferred conversion between this proposal's scene-linear sensitivity ceiling and encoded sample tolerance.
- For implementation acceptance, record fixture-validity outcomes separately from behavior assertion failures, exact revision/command, and baseline/candidate data. No RED, GREEN, or chosen conditioning policy is claimed in this proposal.
