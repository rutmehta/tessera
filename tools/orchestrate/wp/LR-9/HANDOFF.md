# LR-9 — default/inactive Lightroom import diagnostics

Local Machine B lane, `wp/LR-9-default-noise`, based on `43a508e3`.
No catalog row contents, paths, preset names, resource IDs or literal source values are reproduced
here. Catalog measurements below are aggregate counts only. All committed tests
and payloads are synthetic.

## Implementation and evidence

`crates/import-lrcat/src/noop.rs::RULES` is the single policy table used by both
Lua/XMP adapters, the matrix guard and the ignored aggregate audit. It runs after
translation, so non-default mappings and their approximation diagnostics stay in
place. It filters only proven key-scoped no-op warnings and no-op report diagnostics.
It never edits the unconditional `lrcat_develop_source`, Lua source/ordered-entry
containers, XMP fragments, or settings/history.

- Metadata/provenance rules accept Version, CompatibleVersion, UprightVersion,
  UprightPreview, UprightTransformCount, AutoToneDigest*, LensProfileIsEmbedded.
- Control constants are the LR-9 brief's values: false flags, zero counts and
  incremental WB, 0/0.5/35 Upright defaults when Off, 100 curve saturation,
  modern-PV legacy 50/25/0/5, zero SDR controls or finite SDR controls with HDR Off.
  Zero incremental WB is also neutral on rendered images; the adapter need not
  infer raw status from a filename. Nonzero values still warn.
- ToneCurveName2012 is metadata only when an accompanying valid modern curve is
  supplied. Custom names are not logged; the name audit emits only
  known-Adobe-name/other counts. Standalone names still warn.
- Empty RetouchInfo/RedEyeInfo, inactive LensBlur, and empty/all-placeholder
  PointColors (including contiguous explicit Lua indices) are no-ops.
- A second residual audit found empty AILook and FilterList, and enabled empty
  Distraction Removal panels. These also use the shared table. The enabled
  panel is silent only when FilterList, RemoveAreas, GenerativeRemove and
  GenerativeFill are absent/empty. Nonempty payloads retain the existing warnings.
  Malformed/duplicate context fails closed.

The constants were not inferred from the modal catalog value. Empty structures
and zero adjustments are structural no-ops. The additional panel rule follows
[the original SDK bug report acknowledged by Adobe](https://community.adobe.com/t5/lightroom-classic-bugs/sdk-photo-getdevelopsettings-doesn-t-return-the-state-of-the-distraction-removal-panel-s-toggle/idi-p/15433594):
EnableDistractionRemoval is a panel toggle. The conclusion that the toggle alone
has no pixel work is an inference; active resource payloads remain unsupported.
[The SDK change investigation](https://community.adobe.com/questions-675/changes-to-the-lr-14-3-sdk-981935)
identifies AILook and FilterList, without establishing nonempty-payload rendering
parity. This lane makes no such parity claim.

The matrix has a `no-op when default` suffix without changing the non-default
translation status. Its guard exercises every rule, exact source retention,
zero warnings/diagnostics for the default, and a key-specific warning or
translation diagnostic for a non-default control. Provenance has no effectful
non-default case. Extra tests cover both syntax adapters, malformed/duplicate
controls, context gates, indexed placeholders, saved B&W mixers, and cloud payloads.

## Approximation report investigation

The B5-51 harness intentionally relocates every original into a nonexistent
scratch directory. Its apply phase imports zero images. `LrcatReport.approximate`
counts successfully written/resumed photos, so zero report groups on that run is
correct; it does not mean the spool lost its approximation diagnostics.

`lr9_real_decoder_approximate_apply_and_resume` updates synthetic catalog rows
with PointColors, RetouchInfo, active LensBlur and DepthMapInfo. All four keys
appear after real apply and resume, without injecting fake spool diagnostics.
The pre-existing LR-DIAG test remains intact.

A separate real bug was fixed: `note_approximate` previously included `ignored`
diagnostics and selected the first arbitrary entry's reason. It now selects only
info/approximate entries. The ignored-only regression is RED at `bd916dfd` and GREEN
after `25692160`.

## Byte and golden justification

No import golden digest was re-pinned. Never-developed rows now remain silent; failed edit imports still warn individually. Warnings are returned separately from
`ImportedImage.recipe`; suppressing them does not change serialized recipe state.
Existing `crs:*` source/diagnostic payloads are retained, too.

| Fixture / row | Recipe byte change | Removed serialized warning / diagnostic | Justification |
| --- | --- | --- | --- |
| 2,000-row original import fixture, all rows | None | None | Full retained-source digest remains `87d28d71460e64ad1034fd0a5dc408a20a0452b7d37ccfd6f2a00ada8db3c0d5` |
| LR-6f active/standalone-depth fixture, including 1004–1007 | None | None | Digest remains `141018bf3d1b53071354c60090993dc0799d7f7fa35c0c8685cbbb55349ef6e0`; inactive rows still match saved bytes |
| All four `upright_lr7_compat` rows | None | None | Existing length/hash fingerprints unchanged, including the standalone enabled-panel row |
| New synthetic `lr9_false_grayscale_with_saved_mixer_has_no_grayscale_report_entry`: false ConvertToGrayscale plus GrayMixerRed=20 | Only the ConvertToGrayscale info/approximate entry is removed | Key ConvertToGrayscale; reason `profile-dependent B&W response and ordering lack Adobe-rendered calibration` | False flag requests no conversion. GrayMixerRed diagnostic, settings/history and exact source remain. No existing fixture/golden is changed |
| New native `lr9_zero_native_color_overlay_keeps_optional_presence`, synthetic local ID 7 | Decoded native `color_overlay` now preserves `[0,0]` instead of losing it to null | No warning strings changed | Existing typed `ts:LocalId` distinguishes native optional state; exported XMP and foreign Adobe zero-control decoding are unchanged |

Three cloud-warning tests (including the FFI CPU geometry fixture) now include a synthetic nonempty FilterList alongside
EnableDistractionRemoval=true. Their original empty panel was not a cloud effect.
The exact cloud-warning wording assertions are preserved; no golden was updated.

## Commits and RED/GREEN

- `2c109154`: initial RED matrix test plus aggregate-only ignored profile, adapted
  from B5-51 `8750280f`. RED fails on the Version unsupported warning.
- `bd916dfd`: real decoder apply/resume test and RED ignored-report regression.
- `25692160`: shared no-op handling, matrix policy guard, diagnostic/report fix.
- `f5853df1`: RED empty FilterList/AILook and empty panel regression.
- `d17d31c6` / `a1313734`: RED structured-placeholder classification, then GREEN
  placeholder/empty-payload rules and expanded aggregate key inventory.
- `d30b5ba3` / `d54c5d85`: RED silent never-developed rows, then GREEN; failed
  imports retain individual warnings. Three cloud tests now carry actual payloads.
- `2bbb45cd` / `720dfec5`: foreign-XMP review fix. The initial regression asserted
  warnings; retouch decoding can already be quiet independently of LR-9, so the
  final regression directly asserts the no-op classifier fails closed for foreign
  payloads. `720dfec5` completes the no-op projection changes. The final production-code
  commit, including the gate-discovered native round-trip correction below, is `950485f3`.
  The following FFI test-only commit aligns the expected failed-row count; the
  full unfiltered gate is rerun after that expectation change.

## Final measurements and gates


Read-only scratch catalog: 21,656 images, 21,615 edited.
The opt-in profile used a fresh TESSERA_APP_DIR under the authorized scratch
location; originals were relocated to missing scratch paths. No original image
was opened or written. Both source-row and value-class audits have zero unaudited
rows. Retained-source image counts are identical for every baseline key.

| Measurement | Before | After |
| --- | ---: | ---: |
| Develop-settings warning occurrences | 572,811 | 7,658 |
| Not fully supported groups | 137 | 78 |
| Approximate translations in apply report | 0 | 0 |
| Approximate key groups in parsed spool | 19 | 19 |
| Applied images | 0 | 0 |

Reduction: 565,153 Develop-settings warnings (98.66%).
The intermediate run after only the initially enumerated policies was 35,447
warnings / 121 groups; the aggregate residual audit justified the empty AI
payload/panel rules. Ordinary never-developed rows now contribute zero warnings
(previously 41). Failed edit decodes remain individually reported.

### Per-key value classes

The no-op column combines documented/default, empty, inactive and provenance
classes. Non-default does **not** mean unsupported: successfully approximated
non-defaults appear separately in diagnostics. Counts are images, not number of
nested fields or swatches. Warning-image counts deduplicate a key within each row.

| Key | No-op | Non-default | Images still warning for key |
| --- | ---: | ---: | ---: |
| `AILook` | 615 | 0 | 0 |
| `AutoToneDigest` | 1,477 | 0 | 0 |
| `AutoToneDigestNoSat` | 1,477 | 0 | 0 |
| `Brightness` | 19,343 | 1,896 | 1,896 |
| `CompatibleVersion` | 1,245 | 0 | 0 |
| `Contrast` | 19,343 | 1,896 | 1,896 |
| `ConvertToGrayscale` | 21,613 | 2 | 0 |
| `CurveRefineSaturation` | 18,248 | 0 | 0 |
| `EnableDistractionRemoval` | 9,552 | 1 | 1 |
| `Exposure` | 21,212 | 27 | 27 |
| `FilterList` | 8,029 | 0 | 0 |
| `IncrementalTemperature` | 1,944 | 22 | 22 |
| `IncrementalTint` | 1,946 | 20 | 20 |
| `LensBlur` | 16,716 | 417 | 0 |
| `LensProfileIsEmbedded` | 12,042 | 0 | 0 |
| `OverrideLookVignette` | 21,605 | 10 | 10 |
| `PointColors` | 16,855 | 277 | 1 |
| `RedEyeInfo` | 21,615 | 0 | 0 |
| `RetouchInfo` | 11,517 | 26 | 3 |
| `SDRBlend` | 17,114 | 19 | 19 |
| `SDRBrightness` | 17,113 | 20 | 20 |
| `SDRClarity` | 17,115 | 18 | 18 |
| `SDRContrast` | 17,115 | 18 | 18 |
| `SDRHighlights` | 17,113 | 20 | 20 |
| `SDRShadows` | 17,114 | 19 | 19 |
| `SDRWhites` | 17,121 | 12 | 12 |
| `Shadows` | 19,343 | 1,896 | 1,896 |
| `ToneCurveName2012` | 21,615 | 0 | 0 |
| `UprightCenterMode` | 21,614 | 1 | 0 |
| `UprightCenterNormX` | 21,614 | 1 | 0 |
| `UprightCenterNormY` | 21,614 | 1 | 0 |
| `UprightFocalLength35mm` | 21,614 | 1 | 0 |
| `UprightFocalMode` | 21,614 | 1 | 0 |
| `UprightFourSegmentsCount` | 21,615 | 0 | 0 |
| `UprightPreview` | 21,615 | 0 | 0 |
| `UprightTransformCount` | 21,615 | 0 | 0 |
| `UprightVersion` | 21,615 | 0 | 0 |
| `Version` | 21,615 | 0 | 0 |

ToneCurveName2012 names: known Adobe names 21,615; other names 0. No name values were emitted or retained in this handoff.

### Remaining work

These are the non-no-op residual keys, counted per image. This lane retains their
existing non-default handling. In particular, the large legacy Brightness,
Contrast and Shadows remainder is deliberately not broadened beyond the brief's
50/25/5 default rule on modern PV rows. A warning is not proof of a visible pixel
difference: unclassified metadata/cache-looking residuals still need semantic
review before suppression. Nonempty masks/retouch/filter payloads need decoding
or explicit limitations, not blanket suppression.

| Key | Images | Warning occurrences |
| --- | ---: | ---: |
| `AutoTone` | 27 | 27 |
| `AutoWhiteVersion` | 52 | 52 |
| `Blacks2012` | 27 | 27 |
| `Brightness` | 1,896 | 1,896 |
| `Contrast` | 1,896 | 1,896 |
| `Contrast2012` | 27 | 27 |
| `CropConstrainAspectRatio` | 47 | 47 |
| `CustomIncrementalTemperature` | 2 | 2 |
| `CustomIncrementalTint` | 2 | 2 |
| `CustomLensProfileDigest` | 2 | 2 |
| `CustomLensProfileDistortionScale` | 2 | 2 |
| `CustomLensProfileFilename` | 2 | 2 |
| `CustomLensProfileIsEmbedded` | 2 | 2 |
| `CustomLensProfileName` | 2 | 2 |
| `CustomLensProfileVignettingScale` | 2 | 2 |
| `CustomTemperature` | 42 | 42 |
| `CustomTint` | 42 | 42 |
| `DepthBasedCorrections` | 16 | 16 |
| `EnableDistractionRemoval` | 1 | 2 |
| `Exposure` | 27 | 27 |
| `Exposure2012` | 27 | 27 |
| `FillLight` | 27 | 27 |
| `GrainSeed` | 10 | 10 |
| `HighlightRecovery` | 27 | 27 |
| `Highlights2012` | 27 | 27 |
| `IncrementalTemperature` | 22 | 22 |
| `IncrementalTint` | 20 | 20 |
| `MaskGroupBasedCorrections` | 697 | 697 |
| `OverrideLookVignette` | 10 | 10 |
| `PointColors` | 1 | 1 |
| `Preset` | 146 | 146 |
| `RangeMaskMapInfo` | 8 | 8 |
| `RemoveAreas` | 42 | 42 |
| `RetouchAreas` | 330 | 330 |
| `RetouchInfo` | 3 | 3 |
| `SDRBlend` | 19 | 19 |
| `SDRBrightness` | 20 | 20 |
| `SDRClarity` | 18 | 18 |
| `SDRContrast` | 18 | 18 |
| `SDRHighlights` | 20 | 20 |
| `SDRShadows` | 19 | 19 |
| `SDRWhites` | 12 | 12 |
| `Saturation` | 27 | 27 |
| `Shadows` | 1,896 | 1,896 |
| `Shadows2012` | 27 | 27 |
| `ToggleStyleAmount` | 4 | 4 |
| `ToggleStyleDigest` | 4 | 4 |
| `UprightDependentDigest` | 1 | 1 |
| `UprightTransform_0` | 1 | 1 |
| `UprightTransform_2` | 1 | 1 |
| `UprightTransform_3` | 1 | 1 |
| `UprightTransform_4` | 1 | 1 |
| `UprightTransform_5` | 1 | 1 |
| `Vibrance` | 27 | 27 |
| `Whites2012` | 27 | 27 |

Approximate diagnostics preserved in the spool:

| Key | Images |
| --- | ---: |
| `ConvertToGrayscale` | 2 |
| `DepthMapInfo` | 417 |
| `GrayMixerAqua` | 1 |
| `GrayMixerBlue` | 1 |
| `GrayMixerGreen` | 1 |
| `GrayMixerMagenta` | 1 |
| `GrayMixerOrange` | 1 |
| `GrayMixerPurple` | 1 |
| `GrayMixerRed` | 1 |
| `GrayMixerYellow` | 1 |
| `LensBlur` | 417 |
| `PointColors` | 276 |
| `RetouchInfo` | 23 |
| `UprightCenterMode` | 1 |
| `UprightCenterNormX` | 1 |
| `UprightCenterNormY` | 1 |
| `UprightFocalLength35mm` | 1 |
| `UprightFocalMode` | 1 |
| `UprightTransform_1` | 1 |

### Reproduction

Use the environment in the lane request (warm LR-4 target, 4 Cargo jobs and 4
Rayon threads). `cargo clean --release -p import-lrcat -p sidecar -p tessera-ffi` ran before
the final full release gate. Profile only the supplied read-only scratch copy:

```sh
cargo test --release -p tessera-ffi --lib   lrcat::lrcat_profile::profile_from_env -- --exact --ignored --nocapture
```

Set TESSERA_LRCAT_PROFILE to the authorized copy and TESSERA_APP_DIR to a fresh
empty scratch directory. The test fails closed outside scratch paths, masks
errors/panics/dependency output, and emits only allowlisted keys, class labels and
aggregate numeric data. It audits one spooled recipe at a time and opens SQLite
with SQLITE_OPEN_READ_ONLY. The value-class audit is explicitly Lua-only; XMP
rows are counted as unaudited rather than silently misclassified (zero here).


## Gate-discovered native sidecar correction

The full unfiltered gate exposed `mapped_properties::all_mapped_fields_round_trip`
when its random generator selected a zero local color overlay. The pre-existing
condition from `d0904c39d` (already in the base stack) collapsed native
`Some([0,0])` to `None` whenever foreign mask extensions were enabled. This was
not caused by the LR-9 warning filter.

- RED `f987afe5` adds a deterministic native round-trip assertion and a foreign
  zero-control assertion. The unchanged randomized property test remains enabled.
- GREEN `950485f3` recognizes the already-exported typed native `LocalId` and
  preserves optional overlay presence for that native group only. No new XMP
  syntax, source removal, dependency or recipe schema field is introduced.
- All three import golden tests pass unchanged after this fix. The native mask
  suite and the original randomized test pass. The final clean/gate includes
  `sidecar` as a touched crate.


The catalog profile was rerun on final code `950485f3`; its entire aggregate
`counts` object equals the preceding run (7,658 warnings, 78 unsupported groups,
19 spool approximation groups, zero applied groups). All four owned profiling
scratch directories have been deleted. The supplied catalog copy remains intact.

## Final gates

| Gate | Result |
| --- | --- |
| `cargo test --release -p import-lrcat -p engine-api -p sidecar -p merge -p tessera-ffi` | PASS; no command-level filters or exclusions |
| `cargo clippy --workspace --all-targets --release -- -D warnings` | PASS |
| `cargo fmt --all -- --check` | PASS |
| Final-code ignored aggregate profile | PASS; counts verified identical after the native sidecar correction |
| Original import goldens and LR-7 byte fingerprints | PASS unchanged; no re-pins |

Full release test totals: 1,117 passed, 0 failed, 32 ignored by their existing test annotations. The explicit opt-in catalog profile was run separately.

The final release clean removed 1,588 artifacts (3.2 GiB) for import-lrcat,
sidecar and tessera-ffi before the final full gate. No Cargo.lock, Cargo.toml,
dependency, board.json or apps/mac changes. No Swift gate was needed, no app was
launched, and no original/user-media writes were performed. Local commits only;
nothing was pushed. Every lane commit carries the requested co-author trailer.
The docs commit containing this handoff follows `950485f3`.

## LR-9c — LR-5-free restack and review corrections

This section supersedes the LR-9/9b claims above about ignored cloud effects,
unconditional embedded-profile metadata, AI-mask promotion, and the legacy
compatibility re-pin. Branch: `wp/LR-9-restack`. It was built on `wp/LR-6-restack` at `f84aebdb` (main
`270f0169` plus approved LR-3/LR-6) and finally rebased onto its replacement
`4dba1640`, which has the identical tree (commit messages only).

### Restack range-diff

Replayed all 56 commits in `43a508e3..46b1bf54` onto `f84aebdb`, in order,
without squash or reorder. `git range-diff` reports 53 identical patches and
three adjusted patches:

- `f987afe5`: kept the native zero-overlay regression, excluded adjacent LR-5
  Adobe AI state round-trip context.
- `fc6bd47a`: kept actionable mask reasons, excluded AI raster provenance
  acceptance dependent on LR-5.
- `38490249`: kept retouch authority and LR-9b integration behavior, excluded
  the same LR-5 provenance acceptance block.

The LR-9b AI-raster promotion test is updated to require unsupported diagnostics
and exact source retention on this stack. No LR-5 regeneration behavior is added.

The range-diff was re-run for this handoff (`git range-diff 43a508e3..46b1bf54
f84aebdb..83fe6be4`): 56 commits on each side, 53 `=`, 3 `!` (the three above;
the adjustments only remove LR-5-dependent lines).

The lane was started by a Codex worker and completed by Claude Opus 5.5 after
the first worker ran out of quota; the last four LR-9c commits say so.

### A's findings, item by item

| Item | Status | How | Test |
| --- | --- | --- | --- |
| B1 residual.rs cloud switches | done | `EnableDistractionRemoval`, `GenerativeRemove`, `GenerativeFill` keep their warning and get a `status: cloud`, `level: warning` diagnostic per key (`diagnostics::push_cloud`); nothing is pushed as `ignored` | `lr9c::cloud_effects_are_visible_and_filter_list_survives`, `lr9b_translation::cloud_switches_keep_each_feature_visible` |
| B1 FilterList not dropped | done | the `retain` that removed `FilterList` is gone | same two tests; FFI `lr9c_cloud_report_apply_resume_counts_examples_and_keeps_filter_list` |
| B1 retouch.rs generative areas | done | generative `RetouchAreas`/`RemoveAreas` push a cloud diagnostic and a `crs:GenerativeRemove` warning | `lr9c::generative_only_retouch_areas_are_cloud_not_ignored`, `lr9b_translation::generative_removal_has_one_cloud_note_per_image`, `mixed_cloud_and_patch_removal_keeps_both_dispositions` |
| B1 report group with counts and examples | done | new `LrcatReport.cloud` (one entry per Adobe feature, one count per photo, up to five example paths), filled for written AND resumed photos; cloud reasons are removed from the report's `unsupported` list so they are not listed twice | FFI `lr9c_cloud_report_apply_resume_counts_examples_and_keeps_filter_list` (apply, then second apply = resume, equal cloud groups), `lr9c_cloud_counts_photos_once_caps_examples_and_omits_ignored` |
| B1 `ignored` never in the report | done | `note_diagnostics` only admits `approximate`/`info` and `cloud`/`warning` | `lr9c_cloud_counts_photos_once_caps_examples_and_omits_ignored`, `lr9_ignored_diagnostics_are_not_approximate_report_entries` |
| B1 summary / plan preview | done (found in this pass) | the previous worker also filtered cloud reasons out of the plan preview, which has no cloud group; the filter is removed so they stay in "Not fully supported" before the import | FFI `lr9c_plan_preview_keeps_cloud_effects_visible` (RED `c002bd6f`, GREEN `3b47161e`) |
| B1 Swift sheet + Markdown | done | "Requires Adobe cloud (not rendered)" group in `ReportStep` with identifier `document.import.report.cloud` (existing identifiers untouched) and a section in `import-report.md`; a cloud-only report no longer says "No warnings." / "Everything in the catalog has a Tessera equivalent." | Swift `testCloudReportIncludesCountsAndExamples`, `testEmptyUnsupportedTextDoesNotClaimFullSupportWhenCloudContentExists`, `testReportExposesCountsWarningsAndReadOnlyMarkdown`, `testCloudOnlyReportDoesNotClaimNoWarnings` |
| Restore tests/upright_lr7.rs | done | asserts the warning names the key, says "cannot render" and carries the verbatim cloud wording (original two clauses plus the ruling-9 wording) | `cloud_only_and_invalid_geometry_explain_missing_rendering` |
| Restore tests/legacy_ca_lr7b.rs | done | asserts on warnings again, now also requiring the key name | `cloud_wording_is_verbatim` |
| Restore tessera-ffi/tests/upright_lr7.rs | done | `plan.report` assertion restored with "cannot render"; the per-image check counts `cloud`, not `ignored` | `catalog_upright_renders_known_projective_corners_and_reports_cloud_features` |
| Restore generative case in tests/retouch.rs | done, changed shape | the original case was a clone spot plus a generative spot retained atomically. A's ruling makes that mixed list translate the heal, so the restored case is a generative-only spot (still retained atomically, no operation); the mixed list is covered by the next row | `lr3_malformed_or_unrepresentable_key_is_retained_atomically` |
| Mixed heal + generative RetouchAreas | done | heal translated with an `approximate` note, generative item adds a cloud diagnostic and warning | `lr9c::mixed_heal_and_generative_keeps_both_dispositions` |
| S1 no-op rules | done | `Rule::Legacy` for FillLight=0, HighlightRecovery=0, Recovery=0, Blacks=5 on PV2012+; non-default values still report | `lr9c::legacy_defaults_leave_no_diagnostics`, `non_default_legacy_tone_values_are_still_reported`, `lr9b_translation::legacy_fixture_bytes_remain_unchanged` |
| S1 no golden re-pinned | done | `tests/data/point-color-compat.txt` is byte-identical to the base (`git diff 4dba1640 -- crates/import-lrcat/tests/data/point-color-compat.txt` is empty) | `lr1_compat::untranslated_recipe_bytes_match_pre_lr1` |
| S2 LensProfileIsEmbedded | done | silent only when the flag is off or `LensProfileEnable` is off/absent | `lr9c::embedded_profile_selected_is_not_silent`, `embedded_profile_flag_is_silent_only_when_not_selected` |
| S3 mask `Version` | done | accepted only on `Mask/CircularGradient`; elsewhere the mask stays unsupported with source retained | `lr9c::version_on_undocumented_mask_kind_is_retained` |
| S4 SourceY vs OffsetY | done | conflicting values reject the translation: `RetouchAreas` warning, no operation, source retained; equal values translate | `lr9c::conflicting_source_y_aliases_warn`, `source_y_aliases_conflict_names_the_key_and_agreement_translates` |
| S4 CenterWeight on circles | done (implemented) | feather = 100 - CenterWeight*100, as for paint masks | `lr9c::circle_centerweight_controls_feather` |
| S5 per-key range label | done | `crs:<key>: <key> is outside its supported numeric range`; no auto-tone wording | `lr9c::unrelated_out_of_range_control_does_not_claim_auto_tone` |
| Negative ToneCurveName2012 | done | missing, duplicate-x and out-of-range points keep the warning | `lr9c::curve_name_without_valid_points_is_not_suppressed` |

Goldens: against the base, the only file under `crates/import-lrcat/tests/data`
that differs is `lr6b-untranslated-baseline.json`, one warning string
(`DepthBasedCorrections` wording) changed by LR-9b commit `905d85b0`; its
`recipe_bytes` are unchanged. A did not object to it in the LR-9b review, so it
is left as is and named here so it is not a surprise. No other golden or pinned
hash differs from the base.

### Real-catalog measurement (aggregate counts only)

`lr9c_aggregate` (ignored by default) on the read-only scratch copy, 21,615
decoded develop-settings rows, 0 decode failures on both sides.

| | origin/main `b7a28a9b` | this tip |
| --- | ---: | ---: |
| Develop-settings warning occurrences | 573,627 | 1,289 |
| Distinct warning keys | 61 | 46 |
| Images in the cloud group | n/a | 10 |

Top residual keys at this tip: MaskGroupBasedCorrections 633,
CustomTemperature 42, CustomTint 42, RetouchAreas 38, RemoveAreas 36, then 27
each for AutoTone, Blacks2012, Contrast2012, Exposure2012, Highlights2012,
Saturation, Shadows2012, Vibrance and Whites2012 (out-of-range values),
IncrementalTemperature 22, IncrementalTint 20, SDRBrightness 20,
SDRHighlights 20, SDRBlend 19, SDRShadows 19, SDRClarity 18, SDRContrast 18,
DepthBasedCorrections 16, SDRWhites 12, GenerativeRemove 10, GrainSeed 10,
OverrideLookVignette 10, RangeMaskMapInfo 8. The cloud group is 10 images:
GenerativeRemove 10, of which 2 also carry EnableDistractionRemoval.
MaskGroupBasedCorrections is higher than LR-9b reported because AI-mask
promotion left with LR-5.

### Gates (run on `066d63e9`, tree-identical to `373f5aa5` after the final rebase; docs-only afterwards)

`cargo clean --release` of the touched crates first (360 files removed). One
attempt each; nothing was rerun, relaxed or excluded.

| Gate | Result |
| --- | --- |
| `cargo test --release -p import-lrcat` | 247 passed, 0 failed, 2 ignored |
| `-p sidecar` | 81 passed, 0 failed |
| `-p engine-api` | 136 passed, 0 failed |
| `-p image-core` | 131 passed, 0 failed, 3 ignored |
| `-p pipeline-cpu` | 210 passed, 0 failed, 3 ignored |
| `-p pipeline-gpu` | 158 passed, 0 failed, 13 ignored |
| `-p filters` | 154 passed, 0 failed, 8 ignored |
| `-p previews` | 28 passed, 0 failed, 3 ignored |
| `-p export` | 103 passed, 0 failed, 7 ignored |
| `-p tessera-ffi` | 635 passed, 0 failed, 31 ignored |
| `-p tessera-mcp` | 89 passed, 0 failed |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo fmt --all -- --check` | clean |
| `apps/mac/build-ffi.sh` | ok |
| `tools/orchestrate/swift-gate.sh` | SWIFT GATE OK: 923 XCTest executed, 3 skipped, 0 failures; 5 Swift Testing tests passed |
| strict release build of `Tessera` | complete, no warnings |

An earlier import-lrcat run in this pass failed to compile against stale
artifacts left in the shared target directory by the previous worker's
baseline measurement; all workspace crates were cleaned and it was rerun
(243 passed before the four tests added in this pass). No test failed at any
point other than the intended RED runs.

Worktree clean after the gates; no Cargo.lock, board.json or generated-binding
drift. The app was never launched.

After the gates the lane's 66 commits were rebased with `git rebase --onto
4dba1640 f84aebdb` (no conflicts); `git diff` between the pre- and post-rebase
tips is empty. Hashes quoted in the restack range-diff section above
(`f84aebdb..83fe6be4`) are the pre-rebase ones.
