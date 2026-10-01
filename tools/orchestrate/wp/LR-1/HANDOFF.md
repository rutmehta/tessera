# LR-1b Point Color — Machine A review follow-up

Branch `wp/LR-1-point-color`, commits **on top of `95d5a40f`**, no rebase or push.
The coordinator owns integration after LR-2. **All three Point Color blockers are
addressed; the overall test gate is not green because the existing liquify
latency test failed both the broad run and its serial retry.** All new regression fixtures for this
follow-up are synthetic; external RAW tests are excluded by `LR-1b/gates.sh`.

## Blockers

- **a — fixed:** saturation/luminance limits are sample-relative, centered at .5
  like hue. HSL now uses gamma-encoded working RGB (sRGB transfer on Rec.2020
  primaries), with inverse transfer after adjustment. The source sample gets full
  weight for the default ranges at S/L .9/.5, .95/.85, .1/.2 and .5/.5. This fixes
  the encoding mismatch; Adobe's exact internal primaries/transfer remain unknown.
- **b — fixed:** approximation information is in non-warning
  `recipe.unknown.tessera_import_info.PointColors`. It no longer contributes to
  unsupported-import warnings. PointColors is `translated` in the matrix, with a
  synthetic SDK Lua swatch. This branch lacked the matrix and guard: both were
  brought from local `main`, then only the PointColors row/documentation changed.
  `translation_matrix.rs` is byte-identical to local main (SHA1
  `9886aad364b950f0cc8d1ff8fa9a59399d4e0a58`); its negative control remains strict.
- **c — fixed:** GPU batch tile/image/mixed-chain execution selects CPU for the
  color stage when points exist; fused-chain capability no longer accepts that
  stage. Renderer resident capability returns false, selecting the established
  nonresident route for preview/export, including output-depth consumers.
  Shader parameter validation stays strict. Tests assert exact CPU equality for
  the color fallback, and GPU-backed display/scene-linear renderer parity within
  the existing surrounding GPU tolerances. No live app/MCP interaction was run.

## Other requested fixes

- All point memberships, including native OkLCh points, use the original pixel
  entering the Point Color stage. Weighted adjustments still compose in recipe
  order; a first point cannot recruit new pixels into a second selection.
- No hard switch at channel 1.0: selection/adjustment use clamped coordinates and
  add signed/HDR residuals back afterwards. Regression compares .999 and 1.001.
- Complete 19-number all-−1 placeholders are skipped within a swatch list; real
  swatches in that list translate. Other malformed/unknown entries still reject
  atomically and retain source. An all-placeholder/empty list retains the existing
  empty-property source policy.
- Missing SDK range tables default independently to `[0,.25,.75,1]`. Partially
  supplied tables remain errors. The SDK calls these tables optional, but does
  not publish their defaults: the chosen numbers are Tessera reference defaults.
- PV1/2 catalog imports retain PointColors and report that PV3+ is required.
  Rejection happens before recipe/history construction. Verified native packets
  retain their existing native round-trip semantics.

Implementation math, encoding caveats and fixtures: `crates/pipeline-cpu/POINT_COLOR.md`.

## Tests-first evidence and gates

`57d58a70` is the tests-first commit, before implementation.
`7df2e882` is the implementation commit. Both have the requested Claude Opus 5.5
co-author trailer. RED logs in
`LR-1b/` show the requested selection/seam/overlap, import, matrix, GPU tile and
resident-capability failures. Prior LR-1 arithmetic tests and the synthetic
catalog-to-render test were updated to feed linearized gamma-domain swatches;
the numeric channel tolerance remains 2e-6. No byte-compatibility goldens were
re-pinned and no test thresholds were relaxed.

- Focused regressions: **25 passed** across importer (11), matrix (2), CPU (9)
  and GPU fallback (3). CPU/import/matrix are in the combined focused run; GPU
  was rerun after fixing fused-chain eligibility.
- Clippy: **PASS**, exit 0, all seven crates / all targets / `-D warnings`, 95 s
  (`LR-1b/gates-clippy.log`). Existing LibRaw C++ build-script warnings are not
  Rust clippy findings.
- Formatting: **PASS**, exit 0 (`LR-1b/gates-fmt.log`); `git diff --check` passes.
- Broad seven-crate test gate: **1205 passed, 1 failed, 49 ignored, 23 filtered**,
  exit 101, approximately 4153 s wall (`LR-1b/gates-test.log`). Counts use the last
  summary of each top-level harness, excluding nested child-test summaries.
  The 23 filtered tests require external RAW files; exclusions are explicit in
  `LR-1b/gates.sh`. The unchanged byte-compatibility golden and the synthetic
  catalog-to-full-render test both passed.
- The sole failure was existing
  `document_liquify_ui::brush_latency_on_a_20_megapixel_layer`: p95 **297.0 ms**
  versus **<250 ms**, median 231.1 ms. The serial retry of the **same binary** also
  **failed**, exit 101: p95 **595.2 ms**, median **330.3 ms**, 67.45 s wall
  (`LR-1b/retry-liquify.log`). The machine's load averages immediately after that
  retry were **74.88 / 52.39 / 58.22 on 16 logical CPUs**. This is shared-load
  context, not proof of causality or a Point Color regression diagnosis.
  No test threshold, unrelated source, RAW golden or byte-compatibility pin was
  changed. Both failed runs remain recorded as failed; **timing acceptance is
  open and needs a controlled-workload rerun**. No overall-green test claim.

Serial retry, from `crates/tessera-ffi`, with the same four environment variables:

```sh
/usr/bin/time -p "$CARGO_TARGET_DIR/debug/deps/document_liquify_ui-de51b843db61eba9" brush_latency_on_a_20_megapixel_layer --exact --nocapture --test-threads=1
```

After compilation and clippy finished, this lane's 7.2 GiB disposable incremental
compiler cache was removed to relieve shared-disk pressure. Test binaries and
logs were retained; the running gate was not interrupted.

The repeatable broad gate is `sh tools/orchestrate/wp/LR-1/LR-1b/gates.sh`.
It sets the four requested environment variables, uses `--locked`, tests
import-lrcat, engine-api, pipeline-cpu, pipeline-gpu, image-core, sidecar and
tessera-ffi, and lists each excluded external-RAW test explicitly. Ignored
benchmarks remain ignored. Clippy covers the same seven crates with all targets
and `-D warnings`; formatting is workspace-wide:

```sh
cargo clippy --locked -p import-lrcat -p engine-api -p pipeline-cpu -p pipeline-gpu -p image-core -p sidecar -p tessera-ffi --all-targets -- -D warnings
cargo fmt --all -- --check
```

No Swift gate, app build or app launch. No Cargo.lock, dependency or board.json changes. The coordinator's
untracked `LR-RULINGS-FROM-A.md` is untouched.

## Non-blocking fidelity / compatibility record

1. **HSL is approximate.** Machine A reported saturated-color error of a nominal
   +30° producing 19°–58° perceptual rotation and up to −41% chroma. Those are the
   review's prior-operator measurements, not a new Adobe-oracle measurement of
   this revision. Gamma-domain selection fixes do not establish perceptual or
   Adobe pixel parity. No Adobe render oracle was available.
2. **Older builds ignore `selection`.** They see `source_lch: [0,0,0]` on imported
   points and cannot render the intended selection. The field is additive and
   optional; there is no format/contract bump. Byte preservation for native points
   without `selection` remains covered.
3. **XMP export is still Tessera's native format**, not Adobe's 19-number form.
   Imports support the Adobe sequence; generated native RDF does not imply
   Lightroom interoperability. `crates/sidecar/UNMAPPED.md` is updated.
4. **Variance remains retained/unsupported**, as do unknown source layouts. It is
   not silently dropped or claimed as translated.

## LR-2 stage order — coordinator decision at rebase

Putting B&W **before** Point Color means the imported points do nothing when B&W
is on: their input is achromatic and the point operator returns it unchanged.
Do not infer from this that imported points are missing. Color grading can still
color a monochrome image if it runs afterwards.

Best documented Adobe behavior: the [Adobe tone/color documentation](https://helpx.adobe.com/lightroom-classic/desktop/process-and-develop-photos/image-tone-color.html)
describes B&W Mix converting source colors to gray and Color Grading subsequently
toning grayscale. The [Point Color documentation](https://helpx.adobe.com/lightroom-classic/desktop/process-and-develop-photos/color-mixer.html)
places Point Color in Color Mixer, but **does not publish the internal evaluation
order relative to B&W**. A [firsthand Adobe Community report from October 2023](https://community.adobe.com/feature-requests-676/p-point-color-available-in-black-and-white-665677)
observed Color Mixer/Point Color being replaced by the B&W panel; that is historical
UI evidence, not a current-version pixel oracle or proof of internal ordering.
There is no verified Adobe basis here for claiming that Point Color must run
before B&W. (SDK PV3+ and optional-range requirements are
in [LrDevelopController.addPointColorSwatch](https://lrc.mcor.dev/modules/LrDevelopController.html#LrDevelopController.addPointColorSwatch).)

Proposal for Adobe-oriented integration: keep B&W conversion ahead of global
Point Color and grading, make global Point Color explicitly inactive under B&W,
and preserve its settings for switching back to Color. This makes the inertness
intentional, while keeping grading after B&W as Adobe documents. This is a
compatibility proposal, **not a measured Adobe pipeline-order claim**. If the
product instead requires Point Color to affect monochrome tones, use Point Color
on the color input **before** B&W; that is a distinct behavior requiring an Adobe
oracle/explicit coordinator decision. No LR-2 code is changed here.

At rebase, preserve newer main/LR-2 translation-matrix rows; apply only this
lane's PointColors row and representation note if the matrix addition conflicts.
The imported guard is unchanged and must remain strict.

Resolve the README.md / lua_develop.rs integration conflicts and re-pin
`point-color-compat.txt` **only at coordinator rebase time after LR-2**. This lane
leaves those files and the golden untouched.
