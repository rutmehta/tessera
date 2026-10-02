# LR-2b evidence (synthetic inputs only)

Code commit: `bb4b3e6e347cdc28761488c11b14c33b3a798ac0`.
Base: `26e5cb8a`. Branch: `wp/LR-2-tone-curves`; no rebase/push.

## RED

1. `cargo test --locked -p import-lrcat -p pipeline-cpu -p pipeline-gpu -p sidecar lr2b --no-fail-fast`
   - Seven failures across five targets: absent optional fields/CPU effects,
     monochrome GPU guard, sidecar round-trip, and digest test.
   - Committed as `aaa68ad459096ffa0d87e104b4b5408a6a4cb998`.
   - The digest test's first failure was the expected source quote spelling;
     source preserves `'opaque'`, not a rewritten double-quoted string. The
     corrected test verifies both exact source and warning suppression.
2. `cargo test --locked -p image-core --test adobe lr2b -- --nocapture`
   - Failed with the renderer's `Adobe PV3–6 required` admission error.
   - Committed as `03d0f5b94824a9bd10cb7ff9a44f2ed205ef2694` before the routing fix.
     The schema/operator work was present in the worktree for this runtime probe.
3. Additional RED checks before their fixes:
   - `cargo test --locked -p pipeline-cpu --lib lr2b_black_toe`: black-toe sample
     `.01` rendered `0` rather than `.0012626263`; after the public SDK scale/toe
     change, an independent-channel test caught the earlier luminance-only black
     mapping (`[.0063405354,.012681071,.11412964]`).
   - `cargo test --locked -p import-lrcat --lib lr2b_native`: native revision 2
     incorrectly populated the legacy Adobe field. Adding a ProcessFamily check
     fixes this without changing Adobe PV1/2 admission.

The former heuristic test now requires zero PV2012 sliders and original legacy
values in the dedicated block. HDR examples formerly used as unsupported
fixtures are now valid translations; malformed/nonmonotone examples test source
retention instead. No tolerance was increased.

## Final gates

Reproduction: `bash tools/orchestrate/wp/LR-2/gates.sh`.

Environment:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/LR-2-tone-curves"
export CARGO_BUILD_JOBS=3
export RAYON_NUM_THREADS=3
```

The script runs full tests for `import-lrcat`, `engine-api`, `pipeline-cpu`,
`pipeline-gpu`, `pipeline-adobe`, `image-core`, and `sidecar`, with the existing
`import-lrcat/fixture` feature enabled; then all-target clippy with `-D warnings`,
workspace fmt, E2E and compatibility probes. Eight external-RAW tests are excluded:

- `raw_fixture_goldens`
- `real_opcode_fixtures_when_available`
- `fixture_as_shot_roundtrip_and_slider_directions`
- `fixture_level3_tolerance_per_operator_and_output`
- `preview_approximation_is_bounded_on_real_fixtures`
- `fixture_level3_matches_pipeline_cpu`
- `fixture_level3_m2_extremes_are_finite`
- `cfa_dng_stays_raw_and_corrupt_linear_dng_does_not_fall_back`

Existing ignored benchmarks/fixture tests remain ignored. No external RAW
acceptance, Swift gate, app launch or real-catalog access is claimed. Existing
LibRaw C++ build warnings are separate from the Rust clippy gate.

Final test coverage: **702 passed, 0 unresolved failures, 19 ignored, 26 filtered
instances** (eight named external-RAW exclusions across targets). The full run
reported 699 passed / three stale assertions in `import-lrcat --test followups`;
all other targets passed. After a test-only correction for ruling 3, that entire
16-test target passed (replacing its earlier 13 passed / three failed result).
The implementation was unchanged; unaffected targets were not rerun.
All-target clippy `-D warnings`, workspace fmt, synthetic E2E and the baseline
byte-identity/golden audit passed.

An earlier broad run was deliberately superseded after the public black-toe
refinement and golden audit; it is not counted as the final gate. The preceding
focused suite passed **19**, including actual Metal monochrome/HDR/fused tests;
the final gate additionally covers the native/adobe-family regression.

The full run exposed three LR-2-era expectations that valid HDR curves were
unsupported. Commit `0de9fc9b` updates those tests to assert exact normalized HDR
knots, including an identity master with an edited HDR red channel. Malformed,
nonmonotone curves retain exact source and the grouped named warning; the XMP
test now covers identity, valid HDR and malformed inputs separately. No renderer
or importer implementation changed after `bb4b3e6e`.

## Numerical evidence

- Legacy goldens: 36 neutral-ramp samples / 108 channels, four quadratic toe
  samples, and a colored patch proving independent black-channel mapping.
  Scalar-reference absolute bounds: `1e-6`, toe/colored patch `1e-7`.
- Each legacy operator also passes a 2,001-point monotonic ramp.
- Native CPU/GPU monochrome, signed HDR curves in all five channels, and their
  fused path: existing absolute operator bound **1e-4**, unchanged. Standalone
  comparisons also check deterministic repeated GPU output.
- Native GPU legacy Tone and fused construction explicitly return an error;
  these are guards, not legacy GPU parity claims.
- Synthetic SQLite E2E, four monochrome/exposure swatches: max error **0** over
  12 channels, bound **2e-6**.
- Four SDR extended-curve channel imports: max error **0.000000030** over
  12 channels, bound **2e-6**.
- New legacy exposure + brightness SQLite import/JSON round-trip/render:
  max error **0**, bound **1e-4**.
- New HDR SQLite import/JSON round-trip/render: max error **0.000015259** at
  the HDR control point (~50.53 linear output), bound **1e-4**.
- Full PV2010 Develop dispatch with both new tone fields and monochrome agrees
  with the standalone Adobe compatibility renderer within **1e-4**.

These are analytical/reference-engine goldens, not Adobe-generated reference
renders. Public behavior and approximation limits are in LEGACY_PV2010.md.

## Serialization and golden audit

`compat.sh` builds both importer and sidecar source from `26e5cb8a`, links them
against the current dependency artifacts without modifying any manifest/lockfile,
and compares synthetic inputs only.

- Five untranslated recipes: **56,666 bytes identical** to baseline.
- The old 2,000-row hash is reproduced:
  `ad814642116dda65cf0a49494eee8a5cf034c2d3151cb8c35c69975f8cfd661a`.
- **1,200** already-translated monochrome rows change **history only** because
  sidecar now populates monochrome in the initial XMP import edit. All settings,
  retained source and other members are asserted identical.
- **800** remaining rows are byte-identical.
- The intentional new hash is
  `174e43107e23125fb0477cad9376a144a35c28a7966de355ebe87314a98c8fee`.
- Recipe default serialization/fingerprint gates and new field JSON/history
  round-trips also pass. Sidecar independently round-trips monochrome.

Raw local logs are retained at `$CARGO_TARGET_DIR/lr2b-evidence/`; the final
full run is `lr2b-final-gates.log`; its corrected target is
`lr2b-followups-final.log`. `lr2b-final-remaining-gates.log` records clippy,
fmt, E2E and compatibility completion; `lr2b-final-clippy.log` repeats clippy
after the last test-only assertion edit. They are not committed fixture data.
