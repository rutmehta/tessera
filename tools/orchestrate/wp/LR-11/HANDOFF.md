# LR-11 — per-mask local adjustments

Branch `wp/LR-11-local-adjustments`, based on `46b1bf540dd1061438725d3cbdad652ee79f2938`
(the LR-9/9b predecessor). Local only. No dependency, lockfile or board changes.
All fixtures are invented Lua/XMP and generated catalogs. The original raw gate
logs exposed private build paths; LR-11c replaces them with sanitized summaries.
The current lane files contain no private paths or account identifiers.

## Implementation

- Local point curves: optional `params.curves` and `params.curves_extended`, both
  use global `ToneCurves`. Decode Main/Red/Green/Blue and their Extended spellings
  with the global normalization by 255. Extended channels inherit ordinary
  channels when not supplied. The CPU global spline is reused inside the mask.
  Curve amount interpolates/extrapolates the scene-linear curve delta.
- Local Point Color: optional `params.point_colors`, sharing LR-1's 19-number and
  SDK-resource decoder (including indexed Lua arrays) and HSL operator. With B&W,
  only local Point Color moves before monochrome; later local processing removes
  it to avoid double application. Tone cache keys include those points and mask
  geometry. Other local operators retain their post-global position, so overlay
  color survives B&W. Absent local Point Color adds no image-copy pass.
- Local Color tint: existing `params.color_overlay` now renders. A unit-value hue
  is scaled to the original Rec.2020 luminance, then mixed by saturation and
  group amount. Mask alpha limits the resulting delta. Hue 0..360 and saturation
  0..100 are accepted; zero foreign saturation stays absent.
- Local defringe: existing `params.defringe` now reuses the global edge-selective
  purple/green operator. Local 0..100 maps to global 0..20; negative or oversized
  values are retained with a named warning. Existing global hue bands and edge
  threshold remain unchanged. Only the masked delta is applied.
- Individual object instances: optional `adobe_ai.instance_hint` retains typed
  numeric IDs/bounds as informational JSON. The existing object/subject mask seam
  regenerates the selection. The info diagnostic explicitly says **per-instance
  segmentation is unavailable**; no model interface was added. Non-object,
  malformed and unknown hint structures still fail closed; nested range masks
  preserve the object seed and its hint.
- Radial conflicts: retain the entire unsupported parent and emit
  `radial mask inversion flags conflict`. No precedence was found in the reviewed
  [Adobe masking documentation](https://helpx.adobe.com/lightroom-classic/desktop/process-and-develop-photos/masking.html)
  or [Radial Filter documentation](https://helpx.adobe.com/lightroom-classic/desktop/process-and-develop-photos/lightroom-radial-filter.html).
  They describe UI inversion, not precedence between serialized `Flipped` and
  `MaskInverted`. No heuristic precedence was introduced.

These are Tessera rendering approximations, not Adobe pixel-parity claims.
All newly populated fields keep exact source and use
`import_lrcat::diagnostics::push_approximate`, with matrix-matching recipe paths.
The shared finish still owns the single Import history entry. No history writer
was added.

The three new local optional fields and the instance hint omit absent values.
Each has a schema-4 predicate and bumped-only-when-present test. Newly renderable
nonzero defringe and present overlay also require schema 4. Schema predicates
inspect history base and disabled groups as well as live settings. The existing
recipe schema constant remains 3. No old fixture golden was re-pinned.

GPU admission rejects CPU-only local fields before resident dispatch. The
compositor regression requires exact equality (absolute tolerance 0) with CPU,
and has a no-feature GPU-admission control. The raw/RGB engine paths explicitly
select CPU, and RGB memo keys isolate CPU arithmetic from earlier GPU frames.
FFI preview no longer clears overlay/defringe. MCP schema mirrors include the
new curve/Point Color types. No exported UniFFI signature or Swift UI changed,
so binding/UI and Swift build gates are not required for this lane.

## Synthetic evidence

- `crates/import-lrcat/tests/data/lr11/`: six matched Lua/XMP fixtures, including
  the retained radial conflict. Both codecs map to identical local settings;
  exact source, info diagnostics, recipe round-trip and one Import entry checked.
- `crates/tessera-ffi/tests/lr11_local.rs`: generated catalog and XMP imports to
  analytic masked reference pixels, absolute scene-linear tolerance **2e-6**.
  Includes disabled/zero-amount/outside-mask identity, Point Color before B&W,
  tone-cache invalidation, and deterministic existing host-mask seam for instances.
- `crates/pipeline-cpu/tests/lr11_local.rs`: default-byte omission and curve amount
  scaling, including 200% without producing invalid scaled control knots.
- `crates/sidecar/tests/lr11_local.rs`: native XMP round-trip of all new fields.
- Schema predicates and matrix guard cover every new field/spelling.
- Malformed curves/Point Color/overlay/defringe/hints retain the parent atomically.
  Empty controls produce no LR-11 diagnostic. Native and prior absent-field bytes
  stay unchanged. Four LR-9b tests that expected these features to be unsupported
  now assert populated fields and explicit approximation; unrelated goldens stay.

Test-first commits: `484ea506` (RED: two failures, radial named-note control passed)
and `2617ee61` (expanded fixtures/pixels/fallback coverage). Further test corrections
are included in implementation commit `4d9a8cf2`.
Initial test setup corrections: a nonexistent schema helper method was changed
into the existing free function before recording RED; global sharpening/NR were
explicitly neutralized in the pixel fixtures; one Lua literal had an extra brace;
XMP round-trip setup now uses Recipe::edit so its history matches its settings.
The analytic references and tolerances were not weakened. Preflight Clippy found
small syntax/style issues that were corrected before the clean gate.

## Aggregate measurements

The final read-only aggregate audit confirms Develop warnings **715 → 645** and
mask-warning images **71 → 1**: **70 of 71** newly translate with zero mask warnings.
Per-item counts overlap where an image has multiple local controls:

| Item | Before | After |
|---|---:|---:|
| Local tone curve | 25 | 0 |
| Local Point Color | 39 | 0 |
| Local color overlay | 11 | 0 |
| Local defringe | 2 | 0 |
| Individual AI object instance | 17 | 0 |
| Conflicting radial inversion flags | 1 | 1 |

The exclusive baseline partition is Point Color only 17; curve+Point Color 22;
curve only 2; curve+overlay 1; overlay only 9; overlay+defringe 1; defringe only 1;
AI instance 17; radial conflict 1. All classes except the last are now warning-free.
These are translation dispositions, including explicitly diagnosed approximations.
The AI result does not imply per-instance isolation.

The final isolated FFI profile passed: 21,656 images, 21,615 edited, all 21,656
originals intentionally resolved as missing, 0 imported, 21,656 skipped, and
0 fidelity samples. It confirms 645 Develop-settings warnings. The fresh
`lr11-appdir` was removed in a finally block and verified absent. No real-image
fidelity claim is made. Only curated aggregate counts are committed; raw profile
output remains outside the repository.

## Final gates — passed

Environment: PATH includes `$HOME/.cargo/bin`,
`CARGO_TARGET_DIR=$HOME/.cache/tessera-target/LR-4-parametric-masks`,
`CARGO_BUILD_JOBS=4`, `RAYON_NUM_THREADS=4`.
The explicit touched-crate release clean removed **4,781 files / 7.1 GiB**.

All eleven user-requested packages passed: **2,241 passed, 0 failed, 71 ignored**
across 357 test binaries/doc-test suites. There were no command-level exclusions;
ignored tests retain their declared status. Both real-catalog opt-in tests were
run separately and passed. Matrix guard, schema predicates, masked reference
pixels (2e-6), and GPU-selected fallback equality (0) passed in this clean gate.

```sh
cargo test --release --locked --no-fail-fast -p import-lrcat -p engine-api -p pipeline-cpu -p pipeline-gpu -p compositor -p filters -p sidecar -p image-core -p merge -p tessera-ffi -p tessera-mcp
cargo clippy --release --locked --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

All three exit codes are 0. See `gate-tests.log`, `gate-test-counts.json`,
`gate-clippy.log`, `gate-fmt.log` and their exit files. Vendor LibRaw C++ build
warnings are present; Rust workspace Clippy with warnings denied passed.
No bindings/UI changed, so Swift gates are not applicable. No GUI was opened;
no writes were made under Pictures. Dependency manifests, Cargo.lock and board.json
are unchanged. All commits are local, with the requested co-author trailer.

## Commit sequence

- `484ea506`: initial test-first RED.
- `2617ee61`: expanded synthetic fixtures, pixels and fallback tests.
- `4d9a8cf2`: complete implementation and test setup corrections.
- The final `docs(LR-11)` commit contains this handoff, translation matrix and
  final gate/aggregate evidence; its hash is reported in the delivery message.


---

# LR-11b — restack without LR-5 and Machine A round-2 findings

Branch `wp/LR-11-restack`, base `wp/LR-9-restack` at `3be064d2` (main lineage +
LR-3 + LR-6 + LR-9/9b/9c, **without** the rejected LR-5 AI-masks lane).
Everything above this line describes the original LR-11 on the LR-5 stack
(`46b1bf54..6a4cedcc`) and is kept as history: its counts (715 → 645, 70 of 71),
its instance-hint item and the `gate-*`/`aggregate-counts.json` files next to this
document belong to that stack and are superseded by this section.

No dependency, `Cargo.lock` or `board.json` change. Synthetic fixtures only.
No golden or pinned fixture was modified; the only data files in the diff are new
synthetic fixtures under `crates/import-lrcat/tests/data/lr11/` and `lr11b/`.

## Restack

`git rebase --onto 3be064d2 46b1bf54` of the four LR-11 commits, not squashed,
not reordered, messages and trailers untouched.

| Original | Restacked | range-diff | What changed |
|---|---|---|---|
| `484ea506` test | `193edeba` | `=` | identical |
| `2617ee61` test | `6e2bf2c2` | `!` | context only: a neighbouring LR-9c test was renamed on the new base; the commit's own 21-file diff is the same size |
| `4d9a8cf2` feat | `5730e3a4` | `!` | LR-5 dependencies dropped (below) |
| `6a4cedcc` docs | `032e188c` | `!` | matrix prose merged with LR-9c wording; the two instance rows became `unsupported-diagnostic` |

Dropped from `feat` because it only existed on top of LR-5:

- `AdobeAiMask::instance_hint`, the `mask_instance_hint` and `adobe_ai_mask`
  schema-4 predicates and their two tests (the type does not exist on this base).
- The instance clause in `LocalAdjustment::requires_cpu` (now `params.requires_cpu()`).
- AI-kind admission, instance-hint validation and the LR-5 category notes in
  `mask_source.rs`; the instance-hint note loop in `record_approximation_diagnostics`.
- `lr5_imported_tests` in `tessera-ffi/src/masks.rs`.
- Tests that asserted AI masks translate: three import tests and one FFI test for
  instance hints, the `instance` entry of the Lua/XMP parity loop, the `adobe_ai`
  payload in the sidecar round-trip test, and LR-11's rewrites of three LR-9b tests
  (those three keep the base text in the restacked commit).

Kept unchanged: local curves, local Point Color, overlay, defringe, radial conflict
note, GPU admission before dispatch with exact CPU fallback, the five `local_*`
schema-4 predicates and tests, MCP schema mirrors.

## Findings

| Finding | Status | Code | Tests |
|---|---|---|---|
| B2 AI-instance masks must not render as the whole object | done | `sidecar/src/masks.rs` `import_component` rejects `InstanceIDs`/`InstanceBounds` on every mask kind before the kind is read; the instance-hint decoder is deleted. `instance_hint`, its predicate and the `requires_cpu` clause were removed by the restack. `import-lrcat/src/mask_source.rs` names the reason (`individual AI instance selection is not implemented`) | `sidecar lr11b::b2_instance_keys_reject_the_mask_group_on_every_kind` (RED: a native object selection with instance keys was accepted), `import-lrcat lr11b::b2_ai_instance_selection_is_unsupported_and_retained_in_both_codecs`, `b2_instance_keys_block_the_parent_with_other_translatable_content` (both pass from the restack onward and pin it) |
| B3 extended local curves override SDR curves | done | `sidecar/src/masks.rs` `import_local_curves` reads `HDREditMode` from the packet and fills `curves_extended` only for HDR output and non-identity points; `import-lrcat/src/xmp.rs` drops it for a legacy process. Same three clauses as the global rule in `lr2.rs`. A malformed extended curve is still reported on SDR. Native `ts:curves_extended` still round-trips | `sidecar lr11b::b3_extended_local_curves_follow_the_global_hdr_rule`, `b3_native_extended_local_curve_still_round_trips`; `import-lrcat lr11b::b3_sdr_images_ignore_the_extended_local_curve`, `b3_hdr_images_translate_the_extended_local_curve`, `b3_identity_legacy_and_malformed_extended_local_curves`; SDR pixel test `tessera-ffi lr11b_local::b3_sdr_extended_local_curve_renders_the_ordinary_curve` (2e-6, with an HDR control) |
| S7 local Point Color at one stage | done | The stage is: after basic Tone, before monochrome conversion and before the global point curves, with B&W on or off. `pipeline_cpu::split_local_point_colors` (documented there) is now called unconditionally by `pipeline-cpu/src/render.rs`, `image-core/src/render.rs` and `image-core/src/rgb_render.rs`. `DevelopSettings::stage_hashes` puts local Point Color in the Tone hash in both modes | `tessera-ffi lr11b_local::s7_rgb_local_point_color_is_one_stage_before_curves_with_and_without_bw`, `s7_raw_local_point_color_is_one_stage_before_curves_with_and_without_bw`, `s7_tone_cache_tracks_local_point_color_with_and_without_bw`. Each compares against a hand-written composition of the documented order; RGB 2e-6, raw path 1e-5 (the existing raw-path bound), GPU-selected renderer exactly equal to CPU |
| S8 match groups by stable id | done | `mask_source.rs` `source_group_ids` + `record_approximation_diagnostics` look the source group up by the codec's group id. The id rule moved into one function, `sidecar::assign_mask_group_ids`, used by the decoder and the adapter. Decoded ids are unchanged | `import-lrcat` lib `mask_source::lr11b_tests::s8_groups_match_their_source_by_stable_id_not_index`, `s8_group_without_a_source_id_gets_no_source_keyed_note`, `s8_foreign_group_ids_skip_native_ids` |
| S9 local defringe −100..100 | done | Range widened in `sidecar/src/masks.rs`, `mask_source.rs` `correction`, `pipeline-cpu/src/locals.rs` and the FFI preview sanitizer. Positive renders as before. Negative is kept with its source and adds no local defringe (see open point 1) | `import-lrcat lr11b::s9_local_defringe_accepts_the_signed_adobe_range`, `s9_local_defringe_outside_the_adobe_range_is_retained`; `sidecar lr11b::s9_local_defringe_accepts_the_signed_adobe_range`; `pipeline-cpu lr11_local::negative_local_defringe_is_valid_and_adds_no_defringe`; compositor `lr11_local_operators_choose_cpu_fallback_with_identical_pixels` (new `defringe: -50` case); FFI `sanitizing_drops_what_cannot_render_and_clamps_the_rest` |
| Restack consequence: wrong blocker named | done | Without LR-5 an AI selection blocks its group. `unsupported_reason` no longer names a local operator just because its key is present; `decoder_reason` names it only when the decoder failed on it. The radial conflict is named only when `Flipped` and `MaskInverted` disagree (the codec now uses a separate message for that case) | `import-lrcat lr11b::decodable_local_operators_are_not_blamed_for_an_unsupported_selection`, `undecodable_local_operators_keep_their_named_reason`, `radial_conflict_is_named_only_when_the_flags_disagree`; `lr9b_translation::ai_group_rejection_does_not_blame_a_decodable_local_curve`, `neutral_color_variance_is_not_named_as_a_curve_blocker` |

Kept as praised: operators apply with mask weight and amount; GPU admission happens
before dispatch with exact CPU-fallback equality; the v4 tests; no golden changed.

### Existing tests whose inputs changed because of a ruling

Assertions were not weakened or removed. Inputs changed in five places:

- `lr11_local::ordinary_and_extended_channel_curves_keep_channel_fallbacks` adds
  `HDREditMode=1` (B3: extended curves exist only for HDR output).
- `translation_matrix.rs` evaluates `MaskGroupBasedCorrections/Extended*` rows in the
  same HDR context it already uses for `ExtendedToneCurvePV2012` (B3).
- `lr11_local::malformed_local_payloads_retain_parent_atomically` uses
  `LocalDefringe=-101` instead of `-1` (S9: −1 is valid).
- `locals_invariants::invalid_controls_and_blend_layouts_reject` uses `-100.5` and
  `100.5` instead of `-1` (S9).
- `lr9b_translation` AI-group tests: two tests that required the warning to name
  the local tone curve now require that it does not (curves render; the AI
  selection is the blocker). The restacked `feat` commit carries the base text of
  these two tests; the change is in `53d50338`.

Two corrections to my own new tests were made in the fix commits and are named in
their messages: the RGB GPU-equality check needed a renderer without the f16 tile
cache, and one sidecar anchor string was `50` where the exporter writes `50.0`.

## Commits

| Commit | Purpose | RED evidence |
|---|---|---|
| `53d50338` test | B2 + blocker naming | sidecar lr11b 1 failed; import-lrcat lr11b 2 of 5 failed; lr9b_translation 2 of 27 failed |
| `8d825417` fix | B2 + blocker naming | |
| `1ab1d1f0` test | B3 | import-lrcat lr11b 2 of 8; sidecar lr11b 1 of 3; tessera-ffi lr11b_local 1 of 1 |
| `4b81d0bf` fix | B3 | |
| `a75103d2` test | S7 | tessera-ffi lr11b_local 3 of 4 (all on B&W off) |
| `0ae129d9` fix | S7 | |
| `e56b8264` test | S8 | import-lrcat lib 2 of 3 |
| `1edb6574` fix | S8 | |
| `d5a8401a` test | S9 | import-lrcat lr11b 1 of 10; sidecar lr11b 1 of 4; pipeline-cpu lr11_local 1 of 4; compositor 1 of 10; tessera-ffi lib 1 |
| `630efbb3` fix | S9 | |
| `b33931ae` docs | translation matrix prose | |

## Real-catalog measurement (aggregate counts only)

`lr9c_aggregate::aggregate_only` (LR-9c's opt-in test) on the read-only scratch
copy, opened immutable. 21,615 images decoded, 0 decode failures, at every point.

| Point | Develop-settings warnings | `MaskGroupBasedCorrections` | Distinct warning keys |
|---|---:|---:|---:|
| Base `3be064d2` | 1,289 | 633 | 46 |
| Restacked LR-11 `032e188c` | 1,286 | 630 | 46 |
| LR-11b tip | 1,286 | 630 | 46 |

So on the LR-5-free stack LR-11 clears 3 mask images, not the 70 it cleared on the
LR-5 stack: nearly every image whose mask carries a local curve, Point Color,
overlay or defringe also selects with an AI mask, and those stay retained until
LR-5b lands. The five findings change no count; they change what is rendered and
what is reported. Of the 630 remaining mask warnings at the tip, 613 carry the
generic selection reason and 17 name the individual AI instance selection. Before
the blocker-naming fix the same 630 were split 561 generic, 50 naming a local
operator that had in fact decoded, 14 naming a radial conflict and 5 naming the
instance selection.

## Gates

Environment: `CARGO_TARGET_DIR=$HOME/.cache/tessera-target/LR-11b`,
`CARGO_BUILD_JOBS=5`, `RAYON_NUM_THREADS=5`. `cargo clean --release -p` for
engine-api, sidecar, import-lrcat, pipeline-cpu, pipeline-gpu, image-core,
tessera-ffi, compositor, filters, tessera-mcp, previews and export ran first.

All gates ran once, at `b33931ae` (the last commit that touches code or the matrix);
the only later commit adds this section. No rerun was needed and nothing was
serialized or excluded.

| Gate | Result |
|---|---|
| `cargo test --release --locked --no-fail-fast -p import-lrcat -p sidecar -p engine-api -p image-core -p pipeline-cpu -p pipeline-gpu -p filters -p previews -p export -p tessera-ffi -p tessera-mcp -p compositor` | exit 0; **2,344 passed, 0 failed, 81 ignored** across 383 test binaries/doc-test suites (compositor is extra: its LR-11 fallback test was extended) |
| `cargo clippy --release --locked --workspace --all-targets -- -D warnings` | exit 0, clean |
| `cargo fmt --all -- --check` | exit 0 |
| `apps/mac/build-ffi.sh` | exit 0 |
| `tools/orchestrate/swift-gate.sh` | **SWIFT GATE OK**; 923 XCTest tests, 3 skipped, 0 failures; 5 Swift Testing tests passed |
| `swift build -c release --product Tessera -Xswiftc -strict-concurrency=complete -Xswiftc -warnings-as-errors` | complete, exit 0 (one linker note about the deployment target of a vendored object, not a compiler warning) |
| Worktree after the Swift gates | clean; generated bindings unchanged |
| Import goldens | no golden or pinned fixture file is modified in `3be064d2..HEAD`; `golden.rs`, `lr1_compat`, `lr4_compat` and the other compat suites pass unchanged |

Ignored tests keep their declared status. The opt-in aggregate test was run
separately for the measurement above.

## Open points for Machine A

1. **Negative local defringe renders as no change.** In Adobe a negative value
   protects the area from global defringe. Global defringe runs in the lens stage,
   before Detail and Tone, so it cannot be undone at the locals stage. The value
   is accepted, kept in the recipe with its source, and reported as `approximate`
   with a note saying the protection is not rendered. Rendering it properly needs
   the mask at the lens stage. No translated mask group in the measured catalog
   carries a negative local defringe.
2. **The HDR rule is enforced at import, like the global one.** A recipe that
   carries a native `curves_extended` on a non-HDR image still renders it, exactly
   as the global `tone.curves_extended` does. If the rule should also hold at
   render time it has to be added for the global and local paths together.
3. **Most of LR-11's catalog benefit now depends on LR-5b** (see measurement).
   When LR-5b adds AI kinds back, the instance rejection in `import_component`
   runs before the kind is read, so instance selections stay unsupported.
4. `sidecar` gained one public Rust function, `assign_mask_group_ids`. No UniFFI
   signature or Swift source changed.

---

## LR-11b — Rebased onto LR-5b final

Machine A accepted the three LR-11b open points (negative local defringe as
`approximate` with its note; native `curves_extended` on non-HDR keeps rendering;
the ruling-driven test-input changes). The lane was then rebased with
`git rebase --onto 4393cac4 3be064d2`. `4393cac4` is `origin/wp/LR-5b-ai-masks`,
which is main `ef376831` (LR-9 stack merged) plus the 35 LR-5/5b/5c commits.
No squash or reorder, and no trailers were touched. The pre-rebase tip is kept
locally as `backup/LR-11-restack-pre-5b` (`80a9de08`).

### Conflicts and how they were resolved

| Commit | File | Resolution |
|---|---|---|
| feat(LR-11) `9fe8fd85` | `engine-api/src/recipe/schema.rs` | Kept LR-5b's `adobe_ai_mask` predicate test and LR-11's five `local_*` tests. |
| feat(LR-11) | `import-lrcat/src/mask_source.rs` `audited_approximation` | Kept LR-5b's AI-kind admission, `MaskType` and `MaskDigest` in the audit key list, and added LR-11's curve keys. |
| feat(LR-11) | `import-lrcat/src/mask_source.rs` `record_approximation_diagnostics` | LR-11's per-operator notes come first, then LR-5b's AI category notes, unchanged. |
| docs(LR-11) `fb6a7d09` | translation matrix prose | LR-5b's AI raster-provenance sentence plus LR-11's operator sentence. |

All LR-11b commits applied cleanly. The B2 instance rejection in
`sidecar::masks::import_component` runs before the mask kind is read, so it also
covers the AI kinds that LR-5b translates. LR-5b's own person/part rejection is
unchanged.

range-diff `3be064d2..80a9de08` → `4393cac4..HEAD`:

- `=` for commits 1 and 5–14, and for the HANDOFF commit 16.
- `!` for 2–4: context changes, plus the conflict resolutions above.
- `!` for 15: one matrix line, whose context now carries LR-5b's wording.
- Two commits are new.

New commits:

- `b9e3a8c0` test(LR-11b): with LR-5b back, a subject AI mask regenerates. A group
  with a decodable local curve, Point Color, overlay or defringe on a subject mask
  therefore translates with no warning. The tests that check the real blocker is
  named now use an LR-5b person mask (`MaskSubType=3`, still unsupported). The
  warning must name "AI person, part or instance selection" and must not name the
  decodable operator. The radial-conflict test uses the same person mask, and each
  test also checks that the subject variant translates.
  - Affected tests: `lr11b::decodable_local_operators_are_not_blamed_for_an_unsupported_selection`,
    `lr11b::radial_conflict_is_named_only_when_the_flags_disagree`,
    `lr9b_translation::ai_group_with_a_decodable_local_curve_translates_and_a_person_mask_names_itself`
    and `neutral_color_variance_is_not_named_as_a_curve_blocker`.
  - This is a test-input change only. No code change was needed: LR-5b's
    `decoder_reason` needle already names person/part masks, and LR-11b's
    operator needles run only when the decoder failed on that operator.
- `07de76e4` docs(LR-11b): matrix wording for the LR-5b stack.

### Real-catalog measurement (aggregate counts only)

Run with `lr9c_aggregate::aggregate_only` on the read-only scratch copy. At both
points: 21,615 images decoded, 0 decode failures, 46 distinct warning keys.

| Point | Develop-settings warnings | `MaskGroupBasedCorrections` |
|---|---:|---:|
| LR-5b final `4393cac4` | 769 | 113 |
| LR-11b tip | **738** | **82** |

This is where LR-11's benefit shows: 31 mask images now translate with no warning.

Remaining mask warnings at the tip (static reason labels, counted outside the repo):

| Reason | Images |
|---|---:|
| AI person, part or instance selection (LR-5b ruling) | 64 |
| Individual AI instance selection (B2) | 17 |
| Generic reason | 1 |

At `4393cac4`, 54 images carried a "local … is not implemented" reason. 31 of them
now translate. The other 22 also contain a person/part mask, so they now report that
mask as the reason (42 → 64). The one radial "inversion flags conflict" at
`4393cac4` was a mislabel: its radial carries `Flipped=true` with no disagreeing
`MaskInverted`, and the group fails the audit for another reason. It now carries the
generic reason.

### Gates on the rebased tip (`07de76e4`; this commit adds only this section)

`cargo clean --release -p` ran first for the same twelve crates as before.

| Gate | Result |
|---|---|
| `cargo test --release --locked --no-fail-fast` for import-lrcat, sidecar, engine-api, image-core, pipeline-cpu, pipeline-gpu, filters, previews, export, tessera-ffi, tessera-mcp, compositor | exit 0; **2,406 passed, 0 failed, 80 ignored**, 386 suites |
| `TMPDIR=<scratch dir> cargo test --release --locked --no-fail-fast -p tessera-ffi` | exit 0; **670 passed, 0 failed, 31 ignored**, 63 suites |
| `cargo clippy --release --locked --workspace --all-targets -- -D warnings` | exit 0 |
| `cargo fmt --all -- --check` | exit 0 |
| `apps/mac/build-ffi.sh` | exit 0 |
| `tools/orchestrate/swift-gate.sh` | **SWIFT GATE OK**: 934 XCTest tests (3 skipped, 0 failures) and 5 Swift Testing tests |
| strict release build of `Tessera` | complete, exit 0 |
| Worktree after the Swift gates | clean |
| Goldens | none modified or re-pinned in `4393cac4..HEAD`; the only data files added are the synthetic `lr11`/`lr11b` fixtures |

Nothing was rerun or serialized, and no command-level exclusions were used.
