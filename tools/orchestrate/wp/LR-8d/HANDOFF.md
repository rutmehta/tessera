# LR-8d / LR-8e2 handoff (BaselineExposure, embedded-profile scope, M2, M11, M12)

Branch `wp/LR-8d`, based on `wp/LR-8R` `2ee8bfc6`. Binding review:
Machine A `A-LR8-REVIEW` (B3, B4 with its 14-item LR-8d hunk list, M2, M11, M12).
Started by a Codex worker (first six commits plus most of the uncommitted
LR-8e2 work); completed, committed and gated by Claude Opus 5.5.
No board.json or Cargo.lock change. Only synthetic fixtures. No golden re-pinned.

## Commits (oldest first)

| Commit | Group | Summary |
|---|---|---|
| `c2ad67b7` | test(LR-8d) RED | Native BaselineExposure regression; deletes the zeroing line in `pipeline-cpu/tests/golden.rs` |
| `0ac66088` | fix(LR-8d) | Native applies no BaselineExposure; Adobe applies 2^(baseline+user) once at Tone |
| `c936e53c` | test(LR-8e2) RED | Restore main's DCP fixtures and 0.2 expectation; fallback scope and HueSatMap headroom tests |
| `0f324c94` | test(LR-8e2) RED | Absent second illuminant defaults to unknown without masking malformed matrices |
| `1417184b` | test(LR-8d) RED | Adobe boundary rejects non-finite / non-positive baseline gain |
| `88bd0b0e` | fix(LR-8d) | Validate baseline gain at Adobe boundaries only |
| `9369c0c8` | test(LR-8e2) RED | Visible substitution notes; installed profile wins over embedded data |
| `1adee24e` | fix(LR-8e2) | B3 scope, non-fatal parse, illuminants, notes; M11 installed-profile compatibility; M2 headroom |
| `60cb8f8d` | docs(LR-8e2) | M12 Adobe DNG SDK licence attribution |
| (this commit) | docs(LR-8d) | Handoff |

## B4 hunk list (review numbering)

| # | Hunk | Disposition | Code | Test |
|---|---|---|---|---|
| 1 | libraw-ffi `RawFile::baseline_exposure()` | Kept (data; -999 sentinel -> 0) | `libraw-ffi/src/lib.rs` | via raw-decode metadata |
| 2 | raw-decode `RawMetadata.baseline_exposure` | Kept as data | `raw-decode/src/lib.rs` | — |
| 3 | pipeline-cpu `camera_profile_matrix` + re-export | Removed; `color.rs` identical to origin/main | `pipeline-cpu/src/color.rs`, `lib.rs` | `pipeline-cpu/tests/golden.rs` untouched |
| 4 | pipeline-cpu render.rs profile replacements | Reverted to main's matrix | `pipeline-cpu/src/render.rs` | native goldens, `lrcat_dng.rs` |
| 5 | image-core render.rs profile matrix (GPU feed) | Reverted to `LinearRec2020^-1 * camera_xyz` | `image-core/src/render.rs` | `baseline_exposure.rs::native_baseline_metadata_does_not_change_pixels` |
| 6 | image-core raw_admission gain check | Removed; identical to main | `image-core/src/raw_admission.rs` | raw_admission tests |
| 7 | image-core denoise_render calibration | Reverted; identical to main | `image-core/src/denoise_render.rs` | denoise tests |
| 8 | image-core adobe.rs baseline -> apply_exposure | Kept: Tone applies `baseline + user` once (with or without DCP); prefix assembly skips Tone | `image-core/src/adobe.rs`, `render.rs` | `adobe_adds_baseline_and_user_exposure_once`, `adobe_rejects_underflowing_baseline_gain_without_a_profile` |
| 9 | pipeline-adobe render.rs | Kept: undo native WB*profile (no gain), then `baseline + user` once | `pipeline-adobe/src/render.rs` | `dcp_render.rs::adobe_rejects_invalid_baseline_gain_with_and_without_installed_profile` |
| 10 | pipeline-adobe dcp.rs exposure/offset | Kept (embedded profiles) | `pipeline-adobe/src/dcp.rs` | `lr10_exposure_runs_after_hue_and_before_value_dependent_look` |
| 11 | smart_preview.rs GENERATOR_REVISION 2->3, no baked gain | Kept | `pipeline-cpu/src/smart_preview.rs` | `smart_preview.rs`, `lrcat_dng.rs` |
| 12 | smart_preview_codec serialized baseline + check | Kept | `pipeline-cpu/src/smart_preview_codec.rs` | `smart_preview_codec.rs` |
| 13 | `baseline_exposure: 0.,` literals | Kept (field remains) | tests | — |
| 14 | golden.rs zeroing line | DELETED; `pipeline-cpu/tests/golden.rs` is byte-identical to origin/main | — | goldens pass untouched |

`examples/regenerate_goldens.rs` now renders with the same settings as
`tests/golden.rs` (lens profile None, CA off, default otherwise; no
BaselineExposure in Native), so regeneration reproduces the committed PNGs.

## B3 / M11 / M2 / M12

| Finding | Code | Test |
|---|---|---|
| B3 fallback only for external LinearRaw smart-preview proxies, Adobe process, recipe naming an Adobe profile, no installed DCP | `pipeline-adobe/src/embedded_profile.rs` (`embedded_profile_fallback`); `image-core/src/render.rs::prepare_dcp`; `pipeline-adobe/src/render.rs` | `embedded_adobe.rs::only_adobe_named_proxy_recipes_use_embedded_profile`, `ordinary_cfa_dng_does_not_use_embedded_fallback` |
| B3 "Adobe*"-named Native recipes do not flip pipeline | `prepare_dcp` requires Adobe family; `names_adobe_profile` no longer selects a family; Native `validate_settings` treats an Adobe name as inert metadata | `native_adobe_named_recipe_does_not_switch_pipeline` (pixels identical to default-name Native) |
| B3 parse problem never fails a render | `embedded_profile_fallback` returns no profile + info note; `RawImage` snapshot and export `.ok().flatten()` | `malformed_embedded_profile_does_not_fail_a_proxy_render`, `installed_profile_wins_over_valid_or_malformed_embedded_data` |
| B3 CalibrationIlluminant default 0; all EXIF illuminants | `dcp.rs` `illuminant()` (fluorescent 2/14,12,13,15,16 -> SDK interval midpoints, 15 = 3525 K since LR-8e3; 255 Other and, since LR-8e3, undefined codes -> single matrix); absent tag -> 0 | `absent_calibration_illuminant_defaults_to_unknown`, absent-second-illuminant test |
| B3 ForwardMatrix tolerance | Unchanged (0.002, as on main); a rejection now takes the non-fatal fallback | existing DCP tests |
| B3 visible note | `SUBSTITUTED_PROFILE_NOTICE` = "profile substituted (embedded DNG profile)"; `UNAVAILABLE_PROFILE_NOTICE` for fallback; `Renderer::profile_notice`; surfaced in Develop per-photo notes (`tessera-ffi/src/develop.rs`) | notice assertions in `embedded_adobe.rs` |
| M11 restore edited DCP fixtures and 0.2 expectation; installed profiles keep main's behaviour within f32 rounding (max 7.2e-7, reviewed): exposure is applied in f64 by `apply_exposure` before basic tone instead of as an f32 gain inside it | `DcpProfile::parse` = installed (main's unit-Y calibration, legacy D65, tint residual, pre-tone Look, identity without curve); `parse_embedded` opts into SDK defaults | `dcp_render.rs` and `dcp.rs` tests restored to origin/main text (only additions remain, apart from M2 below) |
| M2 no clamp at 1.0 in HueSatMap before exposure | `Table::apply_domain`: V output `max(0)` only; post-exposure LookTable keeps SDR clamp | `huesat_preserves_scaled_value_until_exposure` |
| M12 Adobe DNG SDK licence attribution | root `NOTICE` (agreement text verified byte-identical to `LICENSE.source_code` at revision `de700ad4`), `docs/13-licensing.md`, `dcp_acr3.rs` header | — |

Integration fixture note: `lrcat_combined_tests::int1_offline_proxy_nested_locals_adobe_render_and_orientation`
now names `CameraProfile='Adobe Color'` in its synthetic row, because the ruling
limits substitution to Adobe-named recipes; it compares against an explicitly
parsed embedded profile instead of `with_dcp_profile` (installed semantics).

## Golden audit (versus origin/main)

- Golden PNGs (`fixtures/golden/*.png`): none changed. Native goldens pass with
  `tests/golden.rs` byte-identical to origin/main (zeroing line gone).
- `import-lrcat` catalog golden digests: unchanged (one new test re-asserts the same `GOLDEN`).
- No 64-hex digest or numeric pixel expectation in a pre-existing test changed,
  except the one ruling-mandated M2 expectation:
  `dcp.rs::clips_table_value_as_required_by_dng` -> `huesat_preserves_scaled_value_until_exposure`.
  Before: `p.apply(c*0.75) == a.apply(c)` (value x2 then clipped to 1). After:
  `p.apply_without_tone(c*0.75) == a.apply_without_tone(c*1.5)` (value x2 keeps headroom). Ruling: M2.
- M11: the CFA expectation is `0.2` again (LR-8R had `0.21781155`); fixtures restored.
- Cache/fingerprint identities (intentional invalidation, Adobe/proxy only):
  Adobe CameraProfile seed `adobe-compat-v1` -> `adobe-compat-lr8e2-v1`;
  embedded profile bytes keyed under `tessera embedded DCP v1`;
  Smart Preview `GENERATOR_REVISION` 2 -> 3 (proxy no longer bakes BaselineExposure).
  Native keys unchanged.
- Pixel effect by path: Native originals — none (bit-identical; goldens). Adobe
  process, original with BaselineExposure B — scene-linear gain 2^B before tone
  versus main (main ignored B). Among fixtures only `sample.dng` has B = -0.5
  (x0.7071); no test pins Adobe pixels for it. Proxies — generated proxy carries
  B as data and Adobe applies it once (previously baked into the pixels).

## Gates

Run at code tip `60cb8f8d` (2026-10-07), env `PATH=$HOME/.cargo/bin:$PATH`,
`CARGO_BUILD_JOBS=5`, `RAYON_NUM_THREADS=5`, lane-isolated `CARGO_TARGET_DIR`.
Preceded by `cargo clean --release -p libraw-ffi -p raw-decode -p pipeline-cpu -p pipeline-adobe -p image-core -p tessera-ffi` (removed 886 files).
Logs stay outside the repository.

| Gate | Result |
|---|---|
| `cargo test --release --workspace --no-fail-fast` | 3369 passed, 1 failed, 106 ignored (1629 s; load average 28.5 at start). Only failure: `export/tests/workflow.rs::script_timeout_cancellation_empty_and_spawn_failure` (wall-clock: the 1 s script timeout elapsed before the child wrote its marker; file untouched by this lane). |
| Serialized rerun `cargo test --release -p export --test workflow -- --test-threads=1` | Attempt 1 (load 21.0): same failure. Attempt 2 (load 14.8): 3 passed, 0 failed. No bound changed. |
| `cargo clippy --release --workspace --all-targets -- -D warnings` | Exit 0 |
| `cargo fmt --all -- --check` | Exit 0 |
| `cd apps/mac && ./build-ffi.sh` | Exit 0; `git status --porcelain` empty afterwards (no bindings drift) |
| `tools/orchestrate/swift-gate.sh` | Exit 0; XCTest 935 executed, 3 skipped, 0 failures; Swift Testing 5 tests in 2 suites passed; printed **SWIFT GATE OK** |
| Strict release `swift build -c release --product Tessera -Xswiftc -strict-concurrency=complete -Xswiftc -warnings-as-errors` | Exit 0 (198.37 s build). Retains the known BLAKE3 neon macOS 26.2/15.0 linker warning. |

Final worktree status after all gates: clean.

## LR-8e3 (review REV-LR-8d: APPROVE-WITH-NITS; coordinator ruling)

Commits on top of `7f5b9637`: `d7c6ec49` test(LR-8e3) RED, `8d257d90` fix(LR-8e3),
`5732bdcc` docs(LR-8e3), then this handoff update.

| Item | Code | Test |
|---|---|---|
| SF1 Native draws only "Adobe Standard" / "Adobe Color" (plain matrix) with a visible note; every other non-default profile (Adobe Monochrome, Vivid, Landscape, Portrait, Neutral, creative profiles, Camera Standard) refused as on main | `pipeline_cpu::native_approximates_profile`, `NATIVE_APPROXIMATED_PROFILE_NOTICE`, Native `validate_settings`; `Renderer::profile_notice` (Native branch); Develop `render_notices` now also covers originals | `image-core/tests/native_adobe_profiles.rs`: `native_refuses_profiles_it_cannot_reproduce_as_on_main` (original + proxy), `native_approximates_adobe_standard_and_color_with_a_note` (pixels byte-identical to the default profile) |
| SF1 host compatibility: the Develop host keeps imported Adobe identities in drawn settings (needed for Adobe-process proxy substitution). In Native, the unreproducible ones are drawn without the identity and listed under ignored settings, exactly as main's host did (main stripped every profile and listed it) | `Renderer::with_host_ignored_native_profiles` / `native_ignores_profile` (validation and the two Native pipeline-cpu call sites); set on the FFI backend, preview and lrcat_profile renderers; `DevelopSession::ignored_settings` adds `/camera_profile/profile` | `host_listed_ignored_profiles_render_like_main`; FFI `tests/develop.rs::native_adobe_profile_identities_render_like_main_and_are_visible` (real NEF: Monochrome/Vivid frames byte-identical and listed ignored; Standard/Color byte-identical with the note) |
| SF1 ruling-required expectation change | `embedded_adobe.rs::native_adobe_named_recipe_does_not_switch_pipeline`: Native "Adobe Color" notice `None` -> `Some(NATIVE_APPROXIMATED_PROFILE_NOTICE)`; pixel assertion unchanged | — |
| SF2 Auto WB on Adobe-named proxies through `render_tiles` / `render_progressive` | `prepare_dcp`, `render_tiles`, `render_progressive` plan proxy settings like `render_region_as` | `adobe_named_proxy_with_auto_white_balance_renders_through_every_entry_point` (region, tiles and progressive level 0 identical). Verified RED: without the planning `render_progressive` returned "Auto is not implemented". |
| Nit: WhiteFluorescent 15 -> 3525 K | `dcp.rs::illuminant`, `DCP.md` | `white_fluorescent_uses_sdk_interval_midpoint` |
| Nit: undefined illuminant codes -> 0 (single matrix) | `dcp.rs::illuminant` | `undefined_calibration_illuminants_use_first_matrix_like_sdk` (5–8, 25, 254, 256, 65535; installed and embedded) |
| Nit: installed-DCP Adobe renders within f32 rounding (max 7.2e-7) | wording above and `DCP.md` | reviewer probe |
| Nit: exposure now precedes the legacy PV2010 step; installed-profile LookTables still clip at 1.0 before exposure | documented in `DCP.md` (no code change) | — |
| Nit: proxies with no CameraProfile get no substitution and no note (per ruling) | documented in `DCP.md` (no code change) | `only_adobe_named_proxy_recipes_use_embedded_profile` |

Investigation, `image-core` `fixture_level3_matches_pipeline_cpu` on canon-cr3:
`PIPELINE_RAW_FIXTURES` only overrides the fixture directory (default
`fixtures/raw`, a git-ignored directory filled by `fixtures/fetch.sh`). The test
runs only the Sony ARW unless `IMAGE_CORE_ALL_FIXTURES` is set. Reproduced on
this branch: default run passes the ARW (max linear diff 0); with
`IMAGE_CORE_ALL_FIXTURES=1` it fails on canon-cr3 (scene-linear max diff
0.04327532). So the main gate does not run that case at all: a silent narrowing
by default selection, and a silent skip of the whole test when `fixtures/raw` is
absent (clean clone). The reviewer saw the same failure on origin/main; this lane
does not touch it and leaves it unchanged.

### LR-8e3 gates (code tip `5732bdcc`)

Preceded by `cargo clean --release -p pipeline-cpu -p pipeline-adobe -p image-core -p export -p tessera-ffi`.

| Gate | Result |
|---|---|
| `cargo test --release -p pipeline-cpu -p pipeline-adobe -p image-core -p export -p tessera-ffi --no-fail-fast` | Exit 0: 1224 passed, 0 failed, 51 ignored (785 s; load average 27.7 at start) |
| `cargo clippy --release --workspace --all-targets -- -D warnings` | Exit 0 |
| `cargo fmt --all -- --check` | Exit 0 |
| `cd apps/mac && ./build-ffi.sh` | Exit 0; worktree clean afterwards (no bindings drift) |
| `tools/orchestrate/swift-gate.sh` | Exit 0; XCTest 935 executed, 3 skipped, 0 failures; Swift Testing 5 tests in 2 suites passed; **SWIFT GATE OK** |
| Strict release build | Exit 0 (189.86 s); known BLAKE3 neon linker warning only |

Final worktree status after all gates: clean.
