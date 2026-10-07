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
The import has no user-facing warning for it.

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

1. **Stale cached renders.** The previews disk cache (`previews::PreviewKey`)
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
