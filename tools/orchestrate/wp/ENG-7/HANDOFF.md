# ENG-7 handoff: default lens correction matches Lightroom

Branch `wp/ENG-7` from main `fc3e757d`. Worker: Claude Opus 5.5.
Source: REV-ENG-6 SHOULD-FIX 1. Product decision (user, binding): "match Lightroom".

## Summary

The recipe default `LensProfileSource::Auto` used to estimate distortion and
vignetting from image content whenever a raw had no embedded correction and no
database profile, and it applied that estimate to preview and export. On the
repo fixtures it bent the Canon CR3 (k1 = -0.1008) and the Fuji RAF
(k1 = -0.1335). Lightroom never does this. Auto now resolves, in order:

1. the camera's embedded correction (DNG opcode lists), when the raw carries one;
2. a supplied or database profile matching the lens;
3. nothing.

Content estimation is still available, but only through the explicit
`LensProfileSource::AutoCalibrated` variant. That variant already existed and
is selectable through recipe JSON, MCP and import. The Mac UI has no lens-mode
picker, so no new UI was needed. A named `Database` profile that is not
available now applies no profile correction. Before, it failed the render with
"named profile not supplied", and since production never supplies a lens
database, every imported recipe that named an Adobe LCP failed to resolve.

## 1. Behaviour before ENG-7 (main `fc3e757d`)

- **engine-api** (`recipe/settings.rs`): `LensProfileSource` is
  `{None, Auto (default), Embedded, Database{LensProfileRef}, AutoCalibrated}`,
  serde-tagged `{"kind": "..."}` in snake_case. Every serialized recipe stores
  the tag, so saved default recipes contain `"kind":"auto"`.
  `LensSettings::default()` sets `profile: Auto`, all scales to 100 and
  `remove_chromatic_aberration: true`.
- **pipeline-cpu** `lens_resolve::resolve_with` is shared by `resolve_lens`
  (whole RGB or demosaiced frame) and `resolve_lens_sensor` (sparse CFA
  patches, bit-identical to the whole frame):
  - `Auto | Embedded`: use the embedded opcodes if present. `Embedded` errors
    if there are none.
  - `Auto | Database`: use `LensContext.profile`, otherwise a database lookup
    by name (`Database`) or by lens metadata (`Auto`). A named profile that is
    not found is an error.
  - `Auto | AutoCalibrated` with no sample yet: estimate from content.
    `estimate_k1` (lines from `detect_lines(gray, 0.02, 12)`, bounds ±0.3,
    confidence >= 0.8, |k1| > 1e-4) and `estimate_vignette` (confidence >= 0.9).
  - `remove_chromatic_aberration` estimates lateral CA in every mode,
    `None` included.
  - `render.rs` `camera_linear_prefix` applies the raw-domain embedded opcode
    stages for `Auto | Embedded`.
- **image-core**: `resolve_interactive_lens` (`resident_render.rs`) calls
  `resolve_lens` for RGB sources and `resolve_lens_sensor` for raw ones. It
  always passes an empty `LensContext` and caches the result by
  (lens, demosaic, linearize).
  - Every production `LensContext` is `Default`: previews, export, export/hdr,
    tessera-mcp and image-core. No production path ever supplies a profile or
    a database.
  - `resident_export_lens_supported` requires `None`.
- **sidecar** (`develop.rs::lens_profile`, decode):
  - A hash-verified `ts:LensProfileSource` companion restores
    `embedded` / `auto_calibrated`.
  - `crs:LensProfileEnable=0` gives `None`; disabled wins over named fields.
  - Any of name, filename or digest gives `Database{name, filename, digest, setup}`.
  - Enable or Setup present otherwise gives `Auto`.
  - No lens keys at all keeps the current value, which is the default `Auto`.
  - Encode: Enable is `kind != none`, Setup is the database setup or "Auto",
    and the identity fields round-trip.
- **import-lrcat**: Lua keys `LensProfileEnable/Setup/Name/Filename/Digest`
  pass through `KEY_MAP` into the sidecar decoder, so the mapping is the same
  as above. `LensProfileIsEmbedded` is provenance and warns when selected.
  There was no note when an enabled profile could not be supplied.
- **smart-preview codec**: `Auto` accepted any captured correction source,
  including an image estimate.

## 2. Behaviour after

| Lightroom / recipe | Before | After |
|---|---|---|
| default recipe (`auto`), raw without opcodes or profile | content-estimated k1/vignette applied (CR3 -0.1008, RAF -0.1335) | no profile correction: renders bit-identically to `None` |
| `auto`, raw with embedded opcodes | embedded | embedded (unchanged) |
| `auto`, database/supplied profile matches | profile | profile (unchanged) |
| `auto_calibrated` (explicit opt-in) | estimate | estimate (unchanged) |
| `none`, `embedded` | unchanged | unchanged |
| `database` named, profile available | profile | profile (unchanged) |
| `database` named, profile unavailable | **render error** | no profile correction |
| `LensProfileEnable=0` import | `None` | `None`, no note |
| `LensProfileEnable=1`, no identity | `Auto` | `Auto` plus one info note: automatic profile unavailable; embedded correction used when present, otherwise none; never an estimate |
| `LensProfileEnable=1`, named | `Database{..}` (failed to render) | `Database{..}` kept for export, plus one info note: profile unavailable, nothing applied, never an estimate |
| smart preview written under old `auto` with an image estimate | estimate applied on reopen | estimate dropped on reopen; baked image CA kept; exactly what generating it today resolves |

The info note is `import_lrcat::diagnostics::push_approximate` on key
`LensProfileEnable`, field `/settings/lens/profile`, lane `ENG-7`, level `info`.
It is not a warning, but it is visible. The Mac import sheet lists it under
"Approximate translations" (`tessera-ffi/src/lrcat.rs::note_diagnostics` →
`LightroomImportSheet.swift`). ENG-7b adds Develop and export notes.

**Recipe compatibility.** Saved recipes store `"kind":"auto"`. Nothing in the
app ever offered Auto as an explicit "estimate for me" choice: it is the serde
and recipe default, and the estimate mode has its own variant. So changing
what Auto means is the user decision applied to every recipe that never chose
a mode, and that is what this lane does. Every recipe that explicitly chose
`none`, `embedded`, `database` or `auto_calibrated` renders exactly as before,
except an unavailable named `database`, which used to fail. `recipe_hash`, the
schema, the serde shape and the history patches are unchanged.

## Item table (finding → code → test)

| Item | Code | Test |
|---|---|---|
| Auto never applies a content estimate | `pipeline-cpu/src/lens_resolve.rs` (`calibrate` only for `AutoCalibrated`) | `pipeline-cpu/tests/lens_default.rs::default_auto_never_applies_an_estimate_from_image_content` (synthetic vignette and k1 images, resolution and render equal `None`) |
| All five raw fixtures, default = no geometric correction | same | `lens_default.rs::raw_fixtures_default_applies_no_estimated_geometry` (resolve_lens_sensor and the L3 render equal `None` on CR3, RAF, NEF, DNG and ARW; CR3/RAF failed RED) |
| Explicit estimate opt-in still works | unchanged `AutoCalibrated` | `lens_default.rs::explicit_auto_calibrated_still_estimates` (vignette -0.2, k1 0.12 recovered, render differs from `None`); on fixtures the notice prints CR3 -0.1008 and RAF -0.1335 under AutoCalibrated |
| Embedded corrections unchanged | unchanged | `lens_default.rs::embedded_correction_is_unchanged_in_default_mode`; existing `lens_embedded.rs` |
| Database profile unchanged; no match applies nothing | unchanged / `lens_resolve.rs` | `lens_default.rs::database_profile_is_unchanged_in_default_mode` |
| Unavailable named profile applies nothing (no error) | `lens_resolve.rs` | `lens_default.rs::unavailable_named_profile_applies_nothing` (failed RED with "named profile not supplied") |
| Import mapping and info note | `import-lrcat/src/lens_profile.rs`, called from `xmp::parse` and `lua_develop::parse` | `import-lrcat/tests/eng7_lens_profile.rs` (Enable=0 → None with no note; Enable=1 → Auto or Database with an info note and no warning; absent → default with no note; Lua and XMP paths) |
| Stale smart-preview estimates | `pipeline-cpu/src/smart_preview_codec.rs` | `smart_preview_codec.rs::eng7_legacy_auto_estimate_is_not_applied_on_reopen` (failed RED) |
| Docs | `settings.rs`, `LENS_M2.md`, `STAGED_OPCODES_M2.md`, `filters/README.md`, `image-core/src/lib.rs` | — |

RED evidence (commit `4a3b140d`, plus `ce19b21c`, which fixed one synthetic
pattern that sat below the detector's thresholds, against the unchanged
implementation):

- lens_default: 4 of 7 failed. The 4 were default_auto (falloff estimated
  vignette -0.2), database (no match fell back to the estimate),
  unavailable_named ("named profile not supplied") and raw_fixtures (CR3
  k1 -0.1008, RAF k1 -0.1335).
- smart_preview_codec: the eng7 test failed (k1 -0.101 still applied).
- eng7_lens_profile: 2 of 4 failed (no note).

## 4. Golden and expectation audit

Raw goldens are unaffected: `fixtures/golden/*.png` and
`pipeline-cpu/tests/golden.rs` render with lens `None`. No fixture file
changed and no tolerance changed.

**Re-pinned, each named, with the reason "ENG-7 info note"**

I verified each one with a before/after JSON dump of all 2000 synthetic
images and the compat rows. Removing the single
`lrcat_translation_diagnostics.LensProfileEnable` entry restores every
image's bytes exactly. 1200 of 2000 images changed: the Lua rows built from
`lrc155/global.lua`, which has `LensProfileEnable = 1`.

| Expectation | Before | After |
|---|---|---|
| `import-lrcat/tests/golden.rs` GOLDEN | `87d28d71460e64ad1034fd0a5dc408a20a0452b7d37ccfd6f2a00ada8db3c0d5` | `2f065f67b60d22b8e2f8014c6fd420577c8d8770f3d8a57bd52931614604fec4` |
| `golden.rs` LR-6f digest | `141018bf3d1b53071354c60090993dc0799d7f7fa35c0c8685cbbb55349ef6e0` | `c375c986d2318b016a72bbbc09b3fb51ff19c904b0c3613050e854e5d120942b` |
| `tests/data/point-color-compat.txt` "global" | `15228 8aea11369452bc10e9857738e937d09cac441b96bf372d8a51dd6c28cbfca3be` | `15716 03eb4225da030d7775e92fd8e25561d84c212b017b230a21a3cce6ec83612c38` |
| `upright_lr7_compat.rs` global row | `(15228, 0x14af73e31df40bc7)` | `(15716, 0x76a536a0a0d32c5b)` |

**Test expectations changed (reason: ENG-7)**

- `pipeline-cpu/tests/smart_preview_codec.rs::authenticated_lens_mode_source_mismatches_are_rejected`:
  `(Database named, Manual, no sample)` is now valid, because an unavailable
  named profile resolves to no correction, and the test asserts that. The
  rejected case became `(Database named, Manual, with sample)`.
- These tests exercise image estimation, so they now select `AutoCalibrated`
  explicitly. Their assertions are unchanged.
  - `smart_preview_codec.rs` unit `image_estimated_sample_is_restored_without_reresolution`
  - `pipeline-cpu/tests/lens_profiles.rs::auto_calibration_recovers_synthetic_radial_falloff`
  - `image-core/tests/geometry_edr.rs::composed_optics_edr_matches_f32_scalar_reference`
    (it asserts "fixture must exercise calibrated distortion")
  - `image-core/tests/rgb_memo.rs::automatic_rgb_ca_and_vignette_are_real_corrections`
    (vignette half)
- `image-core/tests/fixture.rs` L3 and L0 engine-vs-reference tests (ENG-6)
  use `AutoCalibrated` through `geometry_settings()`. Under the new default no
  fixture would exercise Geometry, and the REV-ENG-6 mutation check depends on
  CR3/RAF Geometry. Tolerances are unchanged: linear `<= 1e-5`, display `== 0`,
  L0 exact.
- `image-core/tests/common/preview.rs`: doc comment only.

**Render output changed, test expectation unchanged**

To find these, I ran the whole workspace once with the old Auto semantics
plus a temporary log of every Auto resolution that applied an estimate. The
instrumentation was not committed. Every one of these tests passes before and
after, so its default render no longer carries the estimate:

- tessera-ffi `develop.rs`: `as_shot_sliders_round_trip_on_every_fixture_and_single_slider_touch` (CR3, RAF) and `panels_crop_masking_detail_and_history` (k1 -0.084)
- export `export_sharpening::sharpening_controls_reach_exported_chart_pixels` (k1 -0.142)
- image-core:
  - `fixture_level3_m2_extremes_are_finite` (CR3, RAF)
  - `hdr_surface::edr_tiles_match_cpu_reference_and_unit_headroom_is_sdr_linear` (k1 -0.031)
  - `local_tone_resident::preview_approximation_is_bounded_on_real_fixtures` (CR3, RAF)
  - `lr3_develop_retouch::library_preview_renders_retouch_and_cache_cannot_hide_missing_renderer` (k1 -0.156)
  - unit `resident_model::resident_graph_preserves_crop_phase_parity_and_edit_invalidation` (k1 -0.235, synthetic)
- tessera-cli `raw_workflow` (spawned `tessera` binary, CR3)

The size of the render change on the fixtures comes from REV-ENG-6. Auto
against None at L3 moved pixels by up to 0.04 on CR3 and 0.25 on RAF
(scene-linear). Under the new default that difference is 0
(`raw_fixtures_default_applies_no_estimated_geometry`).

**Not runnable here**: the ignored
`smart_preview_codec.rs::real_legacy_v1_asset_reopen_and_edit_parity` pins a
historical edited-render hash of a private v1 asset, rendered with default
(Auto) settings. If that asset captured an Auto image estimate, the hash
changes with this decision. It needs the private asset env vars.

## 5. Gates

All final gates ran on fix commit `6b5860b8`, after
`cargo clean -p engine-api -p pipeline-cpu -p image-core -p import-lrcat`.
The target was `~/.cache/tessera-target/ENG-7` and the fixtures were symlinked.

| Gate | Result |
|---|---|
| `cargo test --release --workspace --no-fail-fast` | pass, exit 0: 642 test binaries, **3303 passed, 0 failed, 99 ignored**, 0 SKIPPED lines. All five raw fixtures ran, including ENG-6's L0/L3 tests (now using `AutoCalibrated`) and `raw_fixtures_default_applies_no_estimated_geometry`. uptime load was 8.3 at the start and 40.2 at the end; there were no wall-clock failures and no reruns. |
| `cargo clippy --release --workspace --all-targets -- -D warnings` | pass (exit 0) |
| `cargo fmt --all -- --check` | pass (exit 0) |
| `cd apps/mac && ./build-ffi.sh` | pass (exit 0); worktree clean afterwards (0 changed paths, no bindings drift) |
| `tools/orchestrate/swift-gate.sh` | **SWIFT GATE OK**: XCTest executed 987 tests, 3 skipped, 0 failures; Swift Testing ran 5 tests in 2 suites, all passed |
| `swift build -c release --product Tessera -Xswiftc -strict-concurrency=complete -Xswiftc -warnings-as-errors` | pass ("Build of product 'Tessera' complete!") |

The HANDOFF commit after `6b5860b8` is documentation only.

## Not done / follow-ups

1. **Stale cached renders.** (Fixed in ENG-7b, render epoch 2.) The previews disk cache (`previews::PreviewKey`)
   and any app thumbnails key on `recipe_hash`, which is intentionally
   unchanged. Previews of Auto recipes rendered before ENG-7 keep the
   estimated distortion until they are re-rendered. A cache-epoch salt is a
   separate decision; it would invalidate every cache and hash pin.
2. **Automatic CA.** `remove_chromatic_aberration` defaults to `true` and still
   estimates lateral CA from content in every mode. That matches Lightroom's
   explicit "Remove Chromatic Aberration" checkbox, but not necessarily its
   default. It was out of scope for this distortion decision and needs a
   product call.
3. **Built-in corrections.** Lightroom applies a camera's built-in (embedded)
   correction even with `LensProfileEnable=0` for cameras that require it.
   Tessera's `None` and an unavailable named `Database` apply none. No fixture
   carries opcode lists, so this is untested on real data.
4. **Export of Auto.** Exporting Auto writes `LensProfileEnable=1` with
   `Setup=Auto`, as before. Lightroom would then apply a matching profile,
   where Tessera, having no database, applies none.
5. **Sidecar XMP import.** Plain sidecar import (`sidecar::XmpPacket::to_recipe`)
   only has user-facing warnings, so it records no note. The info note is on
   the Lightroom catalog import (import-lrcat) only.
6. Not checked: the real Adobe or lensfun profiles for the EOS M50 and X-E2S
   kit lenses (there is no profile database in the repo), and the
   estimator's false-positive rate. That rate is now relevant only to the
   AutoCalibrated opt-in.

---

# ENG-7b: review follow-up (REV-ENG-7 CHANGES-REQUIRED)

Same branch `wp/ENG-7`, on top of `c57b36c2`; `origin/main` is still
`fc3e757d`, so the rebase was a no-op. Worker: Claude Opus 5.5. The
coordinator's rulings apply the user's "match Lightroom" decision.

## Item table (finding → code → test)

| Item | Ruling | Code | Test |
|---|---|---|---|
| **B1** stale disk previews | render-epoch salt; `for_source` tag v2; `recipe_hash`, stage hashes and import pins unchanged by this item | `previews/src/lib.rs`: `RENDER_EPOCH = 2`, directory `e2-<file>-<orientation>-<recipe>` (path only, so key equality and every constructor are unchanged, including the struct-literal keys in tessera-ffi). `revision.rs`: `REVISION_DOMAIN` = `tessera-preview-revision-v2` | `previews` unit `previews_cached_under_an_earlier_render_epoch_miss` (an epoch-1 directory on disk misses at every level; a new put/get works; the `for_source` key uses the epoch and the v2 tag). The tessera-ffi thumbnail test that builds the path by hand was updated |
| **S1** automatic CA | `remove_chromatic_aberration` defaults to false; `AutoLateralCA=1/0` honoured; stored values kept | `engine-api` `LensSettings::default()`; doc; CONTRACTS.md entry | `pipeline-cpu/tests/lens_builtin.rs::remove_chromatic_aberration_defaults_off` (default false; stored `true` kept; omitted → false); `sidecar/tests/eng7b_ca.rs`; `import-lrcat/tests/eng7_lens_profile.rs::auto_lateral_ca_is_honoured_and_absence_is_off` (Lua and XMP) |
| Built-in corrections | apply embedded opcodes with `None` and an unavailable named profile; maker notes are a follow-up | `lens_resolve.rs`: `find_profile`, `uses_built_in`, `built_in_selected`, shared by `resolve_with`, `render::camera_linear_prefix` and `CameraLinearProxy::generate`. Codec `use_embedded` and mode checks follow the same rule. `image-core` `resident_lens_supported(lens, metadata)` keeps opcode raws off the resident fast path | `lens_builtin.rs::built_in_correction_applies_in_none_and_for_an_unavailable_profile` and `..._available_profile_and_auto_calibrated_keep_their_behaviour...`. `image-core/tests/builtin_lens.rs::profile_none_still_applies_built_in_opcodes` (engine L0 == reference, max diff 0, and differs from the same raw without opcodes). The input is synthetic DNG opcode metadata (OpcodeList3 FixVignetteRadial) as `raw_decode::dng::extract_dng_opcode_lists` yields it. The repo has no DNG writer for opcode tags, and no fixture carries opcode lists |
| Export ruling | keep `LensProfileEnable=1` for Auto; add an export-report note when no profile was available | `pipeline_cpu::lens_notice`; `export::render_one_cancellable` appends it to the per-file export warnings (`<file>.tessera-warnings.txt`), which the app lists in the completion toast (`ExportWarnings`). Sidecar export is unchanged | `export/tests/lens_notes.rs` (Auto raw → "No lens profile available — no profile correction applied"; unavailable named → "Lens profile '…' not available — …"; None and RGB Auto → none) |
| **S2** edits move | HANDOFF and a user-facing release note | `docs/RELEASE-NOTES.md` (new), plus this section | — |
| **S3** passive note | Develop and export, whatever the recipe's origin | export as above; `DevelopSession::lens_notes()` (FFI), `DevelopController.lensNotes`, and a status line in the Transform panel. Develop shows only the unavailable-named-profile note; the Auto/no-profile note would show on nearly every raw | `lens_builtin.rs::lens_notices` (all cases, including RGB). The FFI method is a thin filter of `lens_notice`. The Swift gate builds and runs the existing suite, but no XCTest drives the new status line |
| **S4** geometry guard | at least one fixture resolves a non-zero k1 under `geometry_settings()` | — | `image-core/tests/fixture.rs::geometry_settings_resolve_a_distortion_on_some_fixture`. It uses every fixture present, ignores the speed filter, and fails if none resolves `Image` with k1 ≠ 0. CR3 and RAF pass |
| **N1** codec comment | — | Comment made precise. I checked the review's premise: generation does apply image CA to the stored pixels (`render::camera_linear_prefix` runs `optics::lateral_ca` before the proxy is downsampled, and the tail never replays it). So "baked" was accurate; the comment now says exactly where | — |
| **N2** duplicate/string `LensProfileEnable` | comment | `import-lrcat/src/lens_profile.rs::enabled` | — |
| **N4** maker-note gap | named explicitly | see "Not done" | — |
| N3 (optional) | not done | `resident_model` test left on Auto; its purpose is intact and lens parity is covered elsewhere | — |

## A consequence the rulings did not spell out: built-in CA vs the CA switch

Before ENG-7b the Remove CA switch also gated the chromatic part of built-in
per-plane DNG warps. With the switch now off by default, default renders of
raws carrying 3-plane `WarpRectilinear` opcodes would have lost their built-in
CA correction. That contradicts the built-in ruling and Lightroom, which
always applies DNG opcodes in full. So built-in per-plane warps now apply at
the CA scale whatever the switch, and the switch governs only estimated and
profile CA. Internally, "common geometry" (the green-only map) now sets
`chromatic_aberration_scale = 0` as well as the switch. Default renders of
opcode raws are therefore unchanged.

The visible change is limited to a saved recipe that set `false` explicitly
on such a raw: its built-in CA is now applied. One test changed for this,
`lens_embedded::three_plane_embedded_warp_does_not_average_ca`, which now
removes the channel component with a zero CA scale. The audit below found no
other test with built-in CA and the switch off.

## Behaviour after ENG-7b (additions to the ENG-7 table)

| Case | ENG-7 | ENG-7b |
|---|---|---|
| raw with DNG opcode lists, profile `none` | no correction | built-in correction applied |
| raw with opcode lists, named profile unavailable | no correction | built-in correction applied, and a note |
| raw with opcode lists, named profile available / `auto_calibrated` | profile / estimate | unchanged |
| new or default recipe, Remove CA | on (content-estimated lateral CA) | **off** |
| stored recipe with `remove_chromatic_aberration` | stored value | stored value (unchanged) |
| stored JSON without the field (schema-1.2 documents without `lens`, partial or agent JSON, XMP without `AutoLateralCA`) | on | **off** |
| built-in per-plane CA with the switch off | not applied | applied (DNG opcodes always apply in full) |
| Develop, unavailable named profile | silent | status line "Lens profile '…' not available — no profile correction applied" |
| export, Auto raw without built-in data or a profile | silent | export warning "No lens profile available — no profile correction applied" |
| export, unavailable named profile | silent | export warning with the profile name |
| disk preview from before this change | served | not served (render epoch 2), re-rendered lazily |
| smart preview, profile `none` or unavailable named profile, raw with opcode lists | reopened as stored | rejected as inconsistent (the stored pixels lack the built-in stage-1/2 opcodes); it must be regenerated from the original. No fixture or known library asset is affected |

**S2, edits on photos that used to get the estimate.** Crop, masks, retouch
spots, Upright and Transform all sit after the lens stage. On photos where
Auto used to apply an image-estimated distortion, the same local edits now
land on slightly different content: up to a few percent of the half-diagonal
near the corners for k1 ≈ -0.10 to -0.13 (the CR3 and RAF samples here). The
test images in FFI and export reached k1 -0.235. Originals and Smart Previews
move the same way, so they stay consistent with each other.
`docs/RELEASE-NOTES.md` says this in user terms and points to the
`AutoCalibrated` opt-in.

## Golden and expectation audit (ENG-7b)

Every re-pin below has the same single cause, the Remove CA default, unless
stated otherwise. I verified it by running each generator twice, with the
default `true` and with `false`, dumping the decoded JSON, and removing every
`remove_chromatic_aberration` value and every history patch on that path:
nothing else differed. That covers 2000 catalog images plus the
`lr1`/`lr4`/`lr6b`/`lr6f` inputs. 600 of the 2000 catalog rows change their
effective setting (rows without `AutoLateralCA`). The 1200 rows with
`AutoLateralCA = 1` keep it on and now record it as an edit. Every history
base flips.

| Expectation | Before | After |
|---|---|---|
| `engine-api` `hash_is_stable_across_releases` (default recipe hash) | `b053649a…fd9e` | `41831fe4732daaeacbcb9500b91388cb2a5b03d45b35f680d3e9756d25760542` (CONTRACTS.md entry added) |
| `engine-api` `serialisation_bytes_are_pinned`: default, sample, sample compact, fixture 1.2 (no `lens` object), legacy in memory, partial | `3f57329d`, `91de48b0`, `283b4a80`, `69a56ca6`, `7c183437`, `a71da654` | `97ffc690`, `211d82d2`, `de83db39`, `ba0790a2`, `9558ba8f`, `a645492a` (full values in the code) |
| `import-lrcat` `golden.rs` GOLDEN | `2f065f67…fec4` | `ba15b969…d3e9` |
| `golden.rs` LR-6f digest | `c375c986…942b` | `1a0851f4…b70d` |
| `tests/data/lr6f-main-inactive.json` (images 1006, 1007) and `lr6b-untranslated-baseline.json` (8 cases) | recipe bytes with CA on | patched in place with the CA-off bytes; diff verified CA-only |
| `tests/data/point-color-compat.txt` (`lr1_compat`) | all 10 rows | re-pinned: empty/future/nil/empty-point/opaque-point/pending/xmp-opaque +2 bytes; global 15716 → 15850 (AutoLateralCA=1 is now an edit); structures 20442 → 20309 and legacy 11633 → 11500 (AutoLateralCA=0 is no longer an edit) |
| `lr4_compat` | 81809 / `aec3a2eb…43cb` | 81811 / `9eff2648…4aa7` |
| `lr4b_retained` (11 cases) | a88440a4-era sizes and digests | each +2 bytes, new digests (old values kept in the code comment) |
| `lr9b_translation` legacy | 11633 / `fe85a1a4…38ce` | 11500 / `38e7cca4…a95b` |
| `upright_lr7_compat` (4 rows) | `(11420,0x35b9…)`, `(11199,0x0172…)`, `(15716,0x76a5…)`, `(20442,0x0c5a…)` | `(11422,0x5298de2b98d75640)`, `(11201,0x2650e87b9393d40f)`, `(15850,0x80d6eda6bd3c1a60)`, `(20309,0x8bb392fd1c144607)` |
| `sidecar` `lr7e_acr_compat` original / extended; `review_lr7c` default packet | `bd82c6ac…0dbb` / `9b94b7df…1de9` | `c1d516e4…e22c` / `4ae81080…ed23` |

**Test setups changed, assertions unchanged.** Each of these tests exercises
automatic or profile CA, so it now enables the switch explicitly:

- `lens_profiles` `profile_render_applies_vignette_and_channel_maps` and `postdemosaic_auto_ca_reduces_channel_edge_error`
- `lens_stages` `exact_profile_ca_precedes_bayer_demosaic_and_channel_mixing`
- `rgb_memo` (CA half)
- `pipeline-gpu`:
  - `lateral_ca_batch`, through its `plan` helper, which covers 4 tests
  - `resident_rgb_optics` `resolved_ca_and_profile_gain_match_scalar_coordinates` and `unresolved_auto_and_ca_are_not_silently_identity`

**Changed expectations, built-in/CA decoupling.**

- `lens_embedded::three_plane_embedded_warp_does_not_average_ca` (see above).
- `smart_preview_codec`: the None and Database mode checks now admit `Embedded`.
  No existing test asserted the old rejection.

**Render output changed, test expectation unchanged.** I found these by a
temporary instrumented workspace run, not committed. It logged every place
where the old default would have estimated a lateral CA (an
over-approximation: it also logs recipes that set the switch off
explicitly), and every built-in correction newly applied under
`None`/unavailable named profiles. The tests that now skip a CA estimate
are:

- export `formats_profiles_and_privacy` and `jpeg_roundtrip_icc_and_selection_sidecar`
- pipeline-cpu:
  - `smart_preview` (3 tests) and `smart_preview_codec` (several container tests)
  - `lens_m2` (2), `manual_ca` `independent_manual_ca_works_without_auto_ca_or_profile`
  - `lr11b_local` s7
  - `lens_profiles` `zero_profile_strengths_are_bit_exact_to_off`
- tessera-ffi:
  - `smart_preview_thumbnail` tests (13)
  - `lrcat::combined_tests::lr6f_all_lanes_one_apply_both_resources_and_both_absent`

All of them pass before and after, and none is about CA. Built-in-under-None
hits came only from the two new ENG-7b tests. No existing test had a raw with
opcode lists under `None`. On the five repo fixtures, CA on and off render
identically (REV-ENG-7 measured this, and these fixtures carry no opcode
lists), so their defaults are unaffected by S1.

The raw goldens (`fixtures/golden`, lens-off renders) and the tolerances are
unchanged.

## Gates (ENG-7b)

All gates ran on `50d3a614` after `cargo clean -p engine-api -p pipeline-cpu
-p image-core -p import-lrcat -p previews -p export -p sidecar -p tessera-ffi
-p pipeline-gpu`, with target `~/.cache/tessera-target/ENG-7` and the fixtures
symlinked.

| Gate | Result |
|---|---|
| `cargo test --release --workspace --no-fail-fast` | pass: 646 test binaries, **3313 passed, 0 failed, 99 ignored**, 0 SKIPPED lines. All five raw fixtures ran, including the L0/L3 parity tests, the S4 guard and `raw_fixtures_default_applies_no_estimated_geometry`. Load was 6.8 at start and 28.1 at end; no wall-clock failures, no reruns |
| `cargo clippy --release --workspace --all-targets -- -D warnings` | pass |
| `cargo fmt --all -- --check` | pass |
| `apps/mac/build-ffi.sh` | pass; 0 changed paths afterwards (the regenerated bindings for `lensNotes` are committed in `c88ebcf5`) |
| `tools/orchestrate/swift-gate.sh` | **SWIFT GATE OK**: XCTest 987 tests, 3 skipped, 0 failures; Swift Testing 5 tests in 2 suites passed |
| strict release `swift build --product Tessera` (`-strict-concurrency=complete -warnings-as-errors`) | pass |

Earlier attempt on `82f177ce`, recorded for completeness:
- **clippy failed** on a constant `assert!` in the new previews test. Fixed
  with a `const` assert (`5866780f`).
- **The Swift suite crashed twice** (signal 11, at 13:4x and 13:52; load
  9.5–15) in `AgentReviewOwnershipTests.testConcurrentCloseWaitsForOneActualSessionFlush`.
  The DevelopSession test doubles subclass the FFI class with a null handle
  and did not forward the new `lensNotes()`. Fixed by forwarding it in all
  nine doubles (`50d3a614`). This was a real bug in the doubles, not a
  flake. No bound or assertion was changed.

## Not done / follow-ups (ENG-7b)

1. **Maker-note built-in corrections (N4).** Fujifilm, Panasonic, Olympus/OM,
   Sony and Leica bodies store built-in distortion, vignetting and CA in maker
   notes, not DNG opcodes. Lightroom applies them; Tessera does not parse
   them yet. The RAF fixture (X-E2S) therefore renders uncorrected, where
   Lightroom applies Fuji's built-in correction. None of the five fixtures
   carries DNG opcode lists either (`Embedded` errors "embedded calibration
   unavailable" on all five). This needs a separate lane.
2. **Per-camera Raw Defaults.** Lightroom turns on Remove CA (and profile
   corrections) by default for some newer mirrorless bodies. Tessera uses the
   Adobe Default (off) for all cameras.
3. **Original export notes.** The export note covers rendered exports.
   "Original" export (source copy plus sidecar XMP carrying
   `LensProfileEnable=1`) goes through `export_original`, which has no
   warnings channel. Sidecar export is unchanged, per the ruling.
4. **Swift UI test.** The Develop status line has no dedicated XCTest; it is
   a thin view over `lens_notice`, which is unit-tested.
5. **Uncorrected view.** "Show uncorrected" (`render_uncorrected`, profile
   None) still shows the built-in correction for opcode raws, as Lightroom
   cannot disable built-in corrections either.
