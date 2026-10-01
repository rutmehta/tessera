# LR-2 — Machine B handoff

Status: locally committed and verified within the synthetic-only gate scope.
Requested catalog mappings implemented locally, with explicit fidelity
limits. **Not a claim of full Adobe fidelity, nor completion of the later Machine A
rulings for exact legacy operators / HDR-domain curves.** Coordinator review required
before merge. No push, mailbox, board edit, Swift gate, or app launch.

## Commits and ownership

- Branch: `wp/LR-2-tone-curves`.
- Starting HEAD: `87ff1ff173e4d6d2053a534b7cfd9343aef906e1` (brief-only successor of `e6c3e5da`).
- RED: `3aac4cb67deab186c0ba00afbf780c21549d2dc9` — `test(LR-2): specify extended curves grayscale and PV2010 translation`.
- Implementation/GREEN: `20b0fe10d69145493eb9639300775b386c4c784e`.
- This handoff is committed separately with `docs(LR-2):`.
- Every lane commit ends with the requested Claude Fable 5.1 co-author trailer.
- The externally supplied, untracked `LR-RULINGS-FROM-A.md` was read but is not owned,
  edited, staged, or removed by this lane.

## What landed

1. Small additive parser hooks call `import-lrcat/src/lr2.rs`. Lua uses the parsed
   table, XMP reuses the parsed XML document. Unrelated structures are not cloned
   into the mapping table; the existing-only path avoids cloning settings/masks.
2. Four extended curve keys map to existing rgb/red/green/blue curves, dividing
   coordinates by 255. Validity matches the CPU spline: finite domain 0..255,
   strictly increasing x, nondecreasing y. Extended keys override ordinary keys
   for that channel when both exist. Unsupported HDR/nonmonotone curves retain
   the exact source and named diagnostic, rather than creating an unrenderable
   recipe or silently clamping.
3. Optional `/settings/color/monochrome {enabled,mixer}` reuses `HueBands`, with
   all eight GrayMixer channels, independent enabled state, JSON/history round
   trip, and no schema bump. The missing field has no serialized representation
   or render effect. The document-layer 3x3 channel mixer was not sufficient.
4. CPU B&W conversion uses linear Rec.2020 Y and interpolated hue-band luminance
   gains, before ordinary color adjustments. Explicit diagnostics label Adobe
   profile-dependent fidelity as approximate. Disabled mixers do not alter CPU
   pixels or GPU parameters. Enabled B&W declines GPU parameter construction;
   the caller must use CPU fallback, never silently accept a color GPU render.
5. PV1/2-only legacy slider heuristics fill existing tone fields. Modern `*2012`
   keys win, regardless of source order. Revision 3+ ignores stale legacy sliders.
   Exact source remains retained because the mapping is lossy. The README has the
   equations, accepted ranges, aliases, precedence, and Adobe semantic references.
6. AutoToneDigest* remains exact retained metadata with an explicit non-rendering
   diagnostic. A digest cannot supply Auto Tone results. DepthMapInfo also stays
   exact retained metadata; helper-depth lookup/regeneration is LR-5/LR-6 work.

## RED / GREEN evidence

`EVIDENCE.md` records command results and measured output. Raw local logs are
retained under `$CARGO_TARGET_DIR/lr2-evidence/`.

- Initial RED: 2 passed, **6 failed** across mapping and CPU tests. Missing curve
  points, absent B&W field, zero legacy settings, and color pixels instead of gray
  were observed at runtime. The standalone synthetic SQLite import also failed
  at the required source-removal assertion.
- GPU guard RED was separately observed as an assertion failure before adding
  the guard. The first attempted GPU test used unavailable serde_json; it was
  corrected to a typed setting (no dependency added) before recording that RED.
- Additional RED regressions caught nonmonotone curve import, missing B&W fidelity
  diagnostic, and disabled-B&W GPU parameters losing the identity fast path.
- Tests were not relaxed to pass: old “all extended curves unsupported” cases now
  use out-of-domain curves, while new tests require in-range translation. The
  coordinator-provided `monochrome` field name replaced the provisional `grayscale`
  name. An absence assertion was corrected to avoid indexing a missing map entry.
- The 2,000-row synthetic import golden was intentionally re-pinned from
  `d42640939d17a76668916260b58d77a568c5979f84d23c285480f7c1fd7441b8` to
  `ad814642116dda65cf0a49494eee8a5cf034c2d3151cb8c35c69975f8cfd661a`:
  1,200 rows contain newly translated `ConvertToGrayscale=false`. The unchanged
  source-only behavior is checked separately against the lane-base importer.

## Gates and measurements

Environment used throughout builds:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR=$HOME/.cache/tessera-target/LR-2-tone-curves
export CARGO_BUILD_JOBS=3
export RAYON_NUM_THREADS=3
```

Final results: broad synthetic gate **470 passed, 0 failed, 17 ignored, 5 filtered**;
final focused mapping/CPU **11 passed** and GPU guards **2 passed**; clippy, fmt,
end-to-end and byte-compatibility probes all passed. The focused reruns followed
the final source-map prefilter and disabled-GPU-identity correction.

Broad synthetic Rust gate:

```sh
cargo test --locked -p import-lrcat -p engine-api -p pipeline-cpu -p pipeline-gpu --no-fail-fast -- \
  --skip raw_fixture_goldens \
  --skip real_opcode_fixtures_when_available \
  --skip fixture_as_shot_roundtrip_and_slider_directions \
  --skip fixture_level3_tolerance_per_operator_and_output \
  --skip preview_approximation_is_bounded_on_real_fixtures
cargo clippy --locked -p import-lrcat -p engine-api -p pipeline-cpu -p pipeline-gpu --all-targets -- -D warnings
cargo fmt --all --check
bash tools/orchestrate/wp/LR-2/e2e.sh
bash tools/orchestrate/wp/LR-2/compat.sh
```

The five non-synthetic tests above are deliberately excluded; existing ignored
performance/real-RAW tests remain ignored. An initial unfiltered run reached the
repository real-RAW golden and was interrupted; it is not claimed as a passing
unfiltered gate. No real catalog was accessed. All newly authored fixtures are
synthetic. C++ warnings from the existing LibRaw build occur, but Rust clippy
`-D warnings` passes. No Swift or live-app validation is claimed.

- End-to-end: synthetic SQLite catalog -> import -> validated history-backed
  recipe -> CPU tone/curve/color operators. Four B&W/exposure swatches (12 channel
  comparisons): max absolute error **0** against the declared luminance reference.
- Four additional catalog imports exercise master/red/green/blue curve control
  points: max absolute error **0.000000030** over 12 channel comparisons.
- Both end-to-end bounds: **2e-6** absolute, in scene-linear RGB.
- CPU unit swatches: **1e-6** absolute. These are reference-formula tests, **not
  Adobe-rendered goldens**.
- Five unrelated synthetic recipes: **56,666 serialized bytes identical** to
  `87ff1ff1` importer output. This includes modern controls, unknown nested Lua,
  retained metadata/legacy keys under modern PV, and namespace-resolved XMP.
  The comparison compiles the base importer against the same engine dependencies;
  existing engine-api fingerprint tests also pass.

## 29c compatibility / merge notes

- `lua_develop.rs` and `xmp.rs` each have one additive call at the end of source
  capture; Lua also has a small explanatory doc update. Existing normalization,
  key tables, bounds, SQLite query paths, and streaming code are unchanged.
- Successfully represented curve and monochrome keys leave pending source and
  redundant per-property unknown entries. Inactive mixer values are represented,
  not discarded. Partial/malformed values remain exact retained source.
- PV2010 approximation keys remain retained deliberately. Source removal is not
  used to disguise a lossy mapping. Depth/digest metadata stays byte-identical in
  recipes; its report wording is more explicit.
- Optional monochrome uses serde default + skip-if-None. No FORMAT or process
  contract version change; older engines ignore the member and will not render
  its effect. Round-trip tests cover settings/history with a disabled mixer.
- No Cargo.toml dependency addition and **no Cargo.lock change**. The standalone
  end-to-end/compatibility tools link already-built workspace rlibs with rustc,
  avoiding a new dev-dependency/lockfile edge.
- Expected merge overlap: ColorSettings with other color lanes; shared parser
  one-line hooks; CPU color operator; GPU color parameter guard. Keep this pass
  after retained-source construction. Preserve the updated synthetic golden only
  until integration with other translating lanes requires a combined re-pin.

## Blocked / unrepresentable / later ruling differences

- Exact Adobe PV2010 is **not implemented**. The task explicitly requested PV2012
  equivalents with documented differences; that bounded mapping is delivered.
  The later ruling requests `legacy_pv2010` and exact operators instead. No legacy
  render specification or calibrated synthetic Adobe reference was available to
  derive Adobe's image-adaptive brightness, recovery, contrast/toe or black
  clipping. Inventing exact operators would be a false fidelity claim. Legacy
  source is retained so that a future exact branch can replace these heuristics.
- HDR-domain extended curves outside 0..255 remain unrepresented. This lane maps
  into the existing normalized curves as requested; it does not add the later
  ruling's `curves_extended` field or HDR-domain renderer.
- B&W is a tested CPU approximation, not verified Adobe profile-dependent parity.
  GPU B&W, UI controls, live CPU-fallback integration, and sidecar CRS export of
  the new monochrome field are not implemented/validated here. Native recipe
  JSON round trips are supported. Intentional color grading may tint B&W output.
- AutoToneDigest diagnostics are explicit but the UI “Not fully supported”
  categorization was not changed; suppressing harmless metadata in that UI is
  separate host work under the later ruling.
- DepthMapInfo is retained for LR-5/LR-6; no helper raster is read or regenerated
  by this lane. No real-catalog calibration or real-image parity is claimed.
