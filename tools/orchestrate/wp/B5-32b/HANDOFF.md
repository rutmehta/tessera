# B5-32b — Camera Raw parity diagnostics on unchanged main numerics

Branch `wp/B5-32b`, based on `b07993a1125b49468e44e20a5aab3c718e86ccfe` (origin/main at package start). Reworks B5-32 according to Machine A's decision. Only test commit `c55a5e2c` was cherry-picked and then adapted; neither the pipeline fix `635f69d8` nor documentation commit `6fc9408c` was cherry-picked. This handoff and VALIDATION.txt replace the original branch's claims.

## Diagnosis and engine follow-up

Sharpening can produce signed RGB with nearly cancelling luminance. The subsequent texture/clarity gain divides by that signed luminance, amplifying ordinary f32 rounding. At the original failing pixel `(84, 41)`, the diagnostic detail-only round trip was approximately `[-0.006959494, -0.005868249, 0.097931265]` in linear Rec.2020, with `Y ≈ 3.924e-7` and a condition number around 30,000. These values include encode/decode and matrix round-trip rounding; they are not resident scratch-buffer captures.

B5-32 demonstrated that one input blue ulp moves CPU output by **0.084** (`17.687162 → 17.603212`, gap `0.08395004`) with its experimental tone fix. Its pre-fix measurement was even larger: `17.700655 → 17.600351`, gap `0.10030365`. Both are historical B5-32 measurements, not new B5-32b measurements. Dehaze was not the trigger: disabling it left the rich-chain failing maximum unchanged. Standalone sharpen/dehaze stages agree within 5e-6. This is not an f16 checkpoint, hardware texture filtering, or B5-28 profile-curve endpoint issue.

**Engine follow-up belongs to A/Codex:** define and implement a clamp/floor for the texture/clarity signed-luminance divisor, or evaluate texture/clarity before sharpening. Either changes rendering behavior and requires engine-side review and validation. Reassess the independent near-zero tone precision diagnostic as part of that work.

## Dropped changes and why

No product code is changed, including anything under `crates/pipeline-cpu/src` or `crates/pipeline-gpu/src`. The cancellation-safe softplus integral and WGSL log1p approximation from `635f69d8` are dropped. Although they improved the isolated tone reference error, they alter Develop output for every photo, leave the signed-luminance conditioning issue unresolved, and made the 24 MP worst full-frame error worse: **6.1408234 → 16.488846**. The historical small rich-chain error was **0.020483017** before that fix (approximately 0.0205).

## Live tests and diagnostics

- `resident_chain_matches_cpu_across_tile_edges_and_preserves_alpha` is exactly as on the branch base: scaled error below **0.002**, absolute ceiling **0.025**, exact alpha and zero-amount identity.
- `bright_value_stage_isolation_with_and_without_profile_curve` is live. Neutral, sharpen, dehaze, and sharpen+dehaze each assert **5e-6** absolute bounds with both the profile curve and its linear twin. Rich variants log maxima without asserting an unmet absolute bound. All cases check finite GPU RGB and exact alpha.
- `tone_signed_rgb_near_zero_luminance_matches_f64_reference` is an **ignored engine diagnostic**, retaining the independent reference and its unmet 1e-5 bound. Historical baseline errors: CPU **1.8405e-5**, GPU **0.00511713**. Its doc comment identifies the follow-up and measured limitations.
- `bench_24mp_cpu_gpu` retains main's **ignored timing benchmark** status. It preserves the original every-997th-pixel scaled **0.002** guard, adds all-pixel finite GPU RGB/exact alpha checks, and logs the full-frame absolute maximum. The unmet all-pixel 0.01 assertion is removed. A successful explicit run does not certify 0.01 full-frame parity.

## Validation and scope

See VALIDATION.txt for this branch's measured results. Gates use `PATH="$HOME/.cargo/bin:$PATH"` and `CARGO_TARGET_DIR=$HOME/.cache/tessera-target/B5-32` and run serially. Tests live only in filters and pipeline-gpu, so pipeline-cpu is not an additional gate target. No Swift changes; no Swift gate. No board.json or Cargo.lock changes, no push, and no GUI launch.
