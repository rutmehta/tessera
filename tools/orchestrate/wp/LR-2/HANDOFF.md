# LR-2f — Machine B handoff

Branch: `wp/LR-2-tone-curves`; local only. Step 0 fetched and rebased successfully
onto `origin/main` = `486d069f974ecd37cda2da36dbfd2dd007e06d18` (main had advanced
past the supplied `e37de957`). Git replayed 17 lane commits without conflicts;
rebased LR-2e HEAD was `031421f7`.

## Requested-item audit

1. **Already done by LR-2e:** the lane-local `check_approximate` is absent;
   LR-DIAG's `check_rows` is the only row gate. Field comparisons already use
   `n.field.as_deref() == Some(path)`.
2. **Done in LR-2f:** replaced the diagnostic writer's key match and
   `unreachable!` with `LR2_APPROXIMATE_FIELDS`, one explicit 21-entry key/path
   table exported for the matrix gate. The writer iterates that table and emits
   notes only for populated approximations; the matrix checks every table entry
   against its documented path.
3. **Done in LR-2f:** the production Lua matrix test asserts exactly 21 LR-2
   approximate rows, table length 21, all keys present, and matching paths. It
   continues to run the shared row gate against the actual importer.
4. **Already done by LR-2e:** the three requested documents contain no stale
   `required_schema_version_lr2` references.
5. **Already done by LR-2e:** monochrome requires v4 when enabled or when its
   disabled mixer is nonzero. The shared bumped-only-when-present test pins both
   cases to 4; disabled zero stays at 3. No obsolete schema-3 pin remains.
6. **Partly already done; reconciliation completed in LR-2f:** schema.rs is the
   union of all four LR-7 predicates and all three LR-2 predicates, with both
   lanes' tests. `sidecar/tests/roundtrip.rs` and `merge/tests/recipe.rs` are
   byte-identical to main's LR-7 versions. Restored the full schema fixture
   imports (`required_schema_version`, `v4_features_used`) and the fixture-wide
   `v4_features_used(recipe).is_empty()` assertion weakened by LR-7d. Kept
   LR-7's import feature test and added LR-2 import feature coverage including
   disabled B&W with a nonzero mixer.

## Tests first and commits

- `bd6242dc` — test(LR-2f): matrix mapping/count and restored schema fixture
  guard. RED release build failed with E0432 for the absent shared table.
- `0737d1f7` — fix(LR-2f): shared key/path table. Focused release tests passed:
  13 translation-matrix tests and 3 schema-version tests.
- A final docs(LR-2f) commit records the gate results and reproduction script.

All new commits carry the requested Claude Opus 5.5 co-author footer.
RED/GREEN logs: `/tmp/lr2f-red.log`, `/tmp/lr2f-green.log`.

## Final gates

All gates passed (exit 0):

- Requested package clean: 15,742 files / 3.3 GiB removed.
- Full eight-package `cargo test --release --locked ... --no-fail-fast -- --nocapture`:
  **1,409 passed, 0 failed, 49 ignored, 18 filtered instances**, across 217
  test/doc-test targets. The command supplied no test-name filters or exclusions;
  the filtered instances are reported by the existing harness. Release build:
  12m 00s. No ignored benchmarks were enabled.
- Broad-run Liquify 20 MP: **p95 7.3 ms**, median 6.2 ms, max 31.4 ms,
  full-resolution apply 57 ms. Passed the unchanged **p95 < 250 ms** bound.
- Serial release Liquify (`--exact --nocapture --test-threads=1`): **p95 4.7 ms**,
  median 4.3 ms, max 6.3 ms, full-resolution apply 48 ms. Passed; 1 test,
  14 filtered out. Both measurements are retained; no threshold changed.
- `cargo clippy --locked` for all eight packages, `--all-targets -- -D warnings`:
  **pass**.
- `cargo fmt --all --check`: **pass**.
- `git diff --check` and `bash -n tools/orchestrate/wp/LR-2/gates-f.sh`: **pass**.

Logs: `/tmp/tessera-lr2f-gates/{clean,test,liquify-serial,clippy,fmt}.log`
and `status.txt` (all four gate statuses zero). The host was loaded during
validation (observed one-minute load averages about 42–74); no causal or
cross-profile performance comparison is claimed.

Reproduce with `bash tools/orchestrate/wp/LR-2/gates-f.sh`.
The script uses the requested external target directory and three Cargo/Rayon
threads, cleans import-lrcat, engine-api, sidecar and merge before testing, and
runs all eight requested packages in RELEASE with no test-name exclusions.
It captures the broad run and serial Liquify measurement separately, followed
by all-target clippy with `-D warnings` and workspace fmt.

No Cargo.lock, board.json, Swift, or app changes. The supplied untracked
`LR-RULINGS-FROM-A.md` is untouched and unstaged. No push performed.

---

The following LR-2e handoff is historical; its hashes and gate results predate
this rebase and the release-profile LR-2f validation above.

# LR-2e — Machine B handoff

Branch: `wp/LR-2-tone-curves`. Local only, no squash or push.
This is the current handoff; HANDOFF-D.md and earlier evidence/scripts are historical.

## Rebase and ownership

Rebased the full LR-2..LR-2d chain from `dd7b5ddd` onto
`origin/wp/LR-7-upright` = `52eda5334078274445bcedee041319bc2d0e4e8a`
(main `427ab116`, LR-7 through 7e). All fourteen lane commits were replayed.
The LR-2e RED commit is `ada2d768`. The implementation commit is
`5a78ec051ca1489e1e2df06189a6d41785862ebc`. A separate `docs(LR-2e):` commit
records these results. Each new LR-2e commit ends with the requested
`Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` footer.

- Every base translation-matrix row is retained. Non-LR-2 rows are byte-identical;
  LR-2 changes its own rows and adds the Blacks/Recovery aliases. The shared matrix
  test only gains LR-2's synthetic legacy-version/HDR input context.
- LR-7 owns the LR-SCHEMA first-lane checklist edits and LR-DIAG approximation
  guard. LR-2's duplicate schema-version relaxations, roundtrip/merge changes,
  journal test and local guard were dropped. LR-2 retains its registry predicates
  and shared bumped-only-when-present tests, with no second sticky-schema test.
- Both import hooks are present. `xmp::parse_inner` runs geometry **inside** the
  `apply_lr2` guard. Generated Lua XMP uses `false`; the original Lua pass runs
  geometry and LR-2 once each, then the shared finish and validation.
- ENG-1's resident local-tone paths remain intact. B&W is inserted before curves
  in direct, local-tone, and banded chains. The CPU fallback remains exact-equality
  tested; no fallback tolerance was changed.

## Requested fixes

1. **Schema:** the lane-local `required_schema_version_lr2` function is gone.
   `monochrome`, `curves_extended`, and `legacy_pv2010` use `V4_FEATURE_PREDICATES`.
   `assert_bumped_only_when_present` exercises each; monochrome now requires v4
   for enabled B&W **or disabled B&W with a nonzero mixer**, preserving editable
   mixer data across saves. Disabled all-zero B&W stays inactive.
2. **Diagnostics:** LR-2d's shared writer/reader conversion is verified. LR-2 uses
   `diagnostics::push_approximate` only for populated fields and readers use
   `diagnostics::entries()`. Stale modern controls on legacy rows use
   `push_ignored` with status `ignored` and no field. There is no lane-local
   approximation guard or legacy diagnostics array writer.
3. **Old PV2010 recipes:** Adobe PV1/PV2 rendering requires a `legacy_pv2010`
   block. Missing blocks produce the explicit "re-import needed" error rather
   than silently rendering stale PV2012 values. Both revisions are tested with
   and without the block. The neutral sensor/WB prefix preserves an empty block
   for admission while neutralizing the actual tone values.
4. **Legacy stale controls:** every newly imported Adobe PV1/PV2 row gets a
   legacy block, including an empty block if no legacy slider was supplied.
   Modern tone, Clarity2012, Texture, Dehaze, parametric sliders and PV2012
   point curves (including extended curves) are cleared. Ignored source survives
   exactly in Lua/XMP retained properties; each ignored key gets a shared
   diagnostic. No default slider values are invented. Native revision 2 and
   modern Adobe versions keep their ordinary controls.
5. **HDR/parametric composition:** native CPU/GPU and both Adobe CPU paths always
   take parametric controls from ordinary `curves.parametric`, then apply the
   selected point/channel curves. Extended point curves never replace parametric
   sliders. Identity and nonidentity extended blocks are tested. Imported
   all-identity HDR curves leave the extended block absent, preserving ordinary
   points as well as parametric controls. The extended block's parametric member
   is unused. CPU composition, CPU/GPU parity and Adobe render effect are tested.

## Single Import history hazard resolved on the actual LR-7 base

LR-2 assigns the translated settings directly; it never calls `Recipe::edit`,
resets history, or constructs its own Import entry. LR-7's shared
`geometry::finish` records the final settings against the codec's `history.base`.
Both lanes therefore agree on the undo baseline. The Lua and XMP tests cover
LR-2 alone and a row combining B&W with LR-7 saved Upright: one `Author::Import`
entry, both settings present, one diagnostic per represented key, successful
validation, undo to `history.base`, and redo back to the imported settings.
A lower-level RED test proved that LR-2 previously created history before finish;
it now verifies that the mutation function leaves history untouched.
The combined test is complete here, not deferred to another chain rebase.

## Compatibility and fingerprints

No fingerprint re-baseline was needed. `upright_lr7_compat` retains all four
original tuples and passes, including global.lua `(15228, 14af73e31df40bc7)`.
The unrelated scalar row `(11420, 35b9bf7bfe270b28)`, inactive Upright row
`(11199, 01729e71086f5d4d)`, and structures.lua `(18101, beb156c96528413e)` also
remain unchanged. LR-2's disabled-zero B&W path and LR-7's shared finish preserve
those serialized bytes. Import `golden.rs` is verbatim; its synthetic retained
source and 2,000-row catalog checks pass. No golden or point-color compatibility
file differs from the LR-7 base. ENG-1's already-landed CPU golden instrumentation
is inherited unchanged from that base.

## Recorded approximations — deliberately not fixed

Legacy **Contrast pivots at linear 0.5**, not mid-grey. **Contrast 25 alone**:
**0.18 → 0.141**, **0.02 → 0.0097**, approximately **−1 EV in deep shadows**.
Stacked defaults (Brightness 50, Contrast 25, Blacks 5) produce a strong S-curve.
Brightness's largest **relative** lift is in deep shadows; at Brightness 50 its
rational-curve gain tends to `sqrt(2)` near black and to one at white. Both
operators remain **approximate**. These limitations are also recorded in
`crates/pipeline-cpu/LEGACY_PV2010.md`. The equations and operator goldens are
unchanged. A later pass may pivot at 0.18 or work in a gamma domain; LR-2e makes
no Adobe pixel-parity claim.

## Validation and boundaries

`cargo clean` removed 76,282 files (25.6 GiB); the clean test build took 2m 36s.
The broad `--no-fail-fast` run completed with **1,418 passed, 1 failed,
51 ignored, 41 filtered instances**, across 223 test/doc-test targets (exit 101).

| Crate | Passed | Failed | Ignored | Filtered |
| --- | ---: | ---: | ---: | ---: |
| engine-api | 120 | 0 | 0 | 0 |
| image-core | 116 | 0 | 2 | 4 |
| import-lrcat | 127 | 0 | 1 | 0 |
| merge | 53 | 0 | 1 | 0 |
| pipeline-adobe | 36 | 0 | 0 | 0 |
| pipeline-cpu | 176 | 0 | 3 | 3 |
| pipeline-gpu | 150 | 0 | 13 | 2 |
| sidecar | 75 | 0 | 0 | 18 |
| tessera-ffi | 565 | 1 | 31 | 14 |

The only broad failure was
`document_liquify_ui::brush_latency_on_a_20_megapixel_layer`: median **327.4 ms**,
p95 **790.6 ms**, maximum **1206.2 ms**, full-resolution apply **4874 ms**,
against the unchanged **p95 < 250 ms** assertion. Its other 14 tests passed.
The machine was heavily loaded during this run (observed one-minute load
averages around 52–53); this is context, not a base-versus-tip performance
comparison or proof that load caused the failure.

The requested **serial rerun** (`--exact --nocapture --test-threads=1`) also
failed (exit 101): median **306.6 ms**, p95 **672.8 ms**, maximum **1005.1 ms**,
full-resolution apply **3473 ms**, against the same **250 ms** limit. Both
results are retained; no threshold, assertion or test was weakened. This is
**not a fully green test gate**. No Liquify implementation changed in LR-2e.

`cargo clippy` for all nine packages, all targets, with the fixture feature and
`-D warnings`: **pass** (exit 0). `cargo fmt --all --check`: **pass** (exit 0).
`git diff --check` and `bash -n tools/orchestrate/wp/LR-2/gates-e.sh`: **pass**.
The script's final exit is 101, preserving the timing failures.

All LR-2e regressions passed in the broad run. It also passed ENG-1's resident
local-tone/presence cases, B&W toning with and without local tone, unchanged
exact-equality legacy GPU/CPU fallback, schema writers, shared diagnostics guards,
original import goldens, and all LR-7 compatibility fingerprints.

Raw logs: `/tmp/tessera-lr2e-gates/{clean,test,liquify-serial,clippy,fmt}.log`
and `status.txt`. RED evidence remains in `/tmp/lr2e-red-{schema,history,import,cpu,adobe}.log`
and `/tmp/lr2e-red.log` (old PV2010 admission).
Run `bash tools/orchestrate/wp/LR-2/gates-e.sh` to reproduce. It sets the required
external target directory, Cargo build jobs 3, and Rayon threads 3, cleans all
nine gate packages, and records broad tests, serial Liquify, clippy and fmt
separately under `/tmp/tessera-lr2e-gates` (override with `LR2E_LOG_DIR`).

The nine packages are import-lrcat, engine-api, pipeline-cpu, pipeline-gpu,
pipeline-adobe, image-core, sidecar, merge, and tessera-ffi. External-RAW tests
listed in the script are excluded; ignored benchmarks stay ignored. Fixtures
are synthetic only. No threshold changes, Cargo.lock/dependency/board changes,
Swift gate, app build, app installation, or real-catalog access. The supplied
untracked `LR-RULINGS-FROM-A.md` is unchanged and unstaged.
