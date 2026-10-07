# LR-CLEAN handoff

Base: `gate/b51`, `392c2156` on `wp/LR-CLEAN`. Synthetic inputs only.
Status: implementation and verification in progress; this is not a gate claim.

## Finding → code → test

| Finding | Code / disposition | Test / evidence |
| --- | --- | --- |
| LR-1: native point colors must not require v4 | `engine-api/src/recipe/schema.rs`: require Adobe `selection` | Native points RED (4 instead of 3), then schema suite 95 passed |
| LR-2: restore sticky schema test / comment | `schema.rs` documents retention of stored version; restored `lr2d_schema_bump_is_sticky_after_feature_removal` | Serialize, reload, remove feature, reserialize remains 4 |
| LR-2: stale source retention | `import-lrcat/tests/lr2e.rs` | Assert each stale key has retained nonempty source in Lua and XMP |
| LR-4: non-Paint MaskValue != 1 | `mask_source.rs`, `sidecar/src/masks.rs`: unsupported, source retained, warning; no rendered approximation | `lr4e_shape_mask_value_is_explicitly_diagnosed` RED; neutral value and Paint blend tests preserved |
| LR-4: restore schema regression | `import-lrcat/tests/schema_version.rs` | `lr4c_imported_mask_features_write_schema_four`: nested, disabled, display-range cases |
| LR-4: XMP luminance scale | Translation matrix | Explicit Lightroom perceptual-scale interpretation of exported native linear masks |
| LR-3: avoid discarded admission Upright solve | `image-core/src/render.rs`: shared CPU-chain predicate before public admission | Public `render_tiles` regression RED `[2;5]`, GREEN unchanged `[1;5]`; 46 image-core unit tests passed |
| LR-3: restore vec![1;5] | Already present (`881212e8`); test previously bypassed public admission | Retained exact assertion, now exercises public API |
| LR-3: base Lua hook order | `lua_develop.rs`: geometry → LR-2 → retouch → finish | Existing combined Lua/XMP one-history-entry regression |
| LR-3: exact feature list | `schema_version.rs` | `assert_eq!` full `v4_features_used`, not membership only |
| LR-3: dual memo memory | See memory note below | Source audit; no cache bound changed |
| LR-3: MCP scale-switch owner note | `crates/tessera-mcp/README.md` | Documentation of no-spot preview behavior |
| LR-6: complete regeneration reason | `lr6_lens_blur.rs` | Assert entire regeneration reason, count, field and level |
| LR-6: error kind | `tessera-ffi/src/lrcat_depth_tests.rs` | Match bridge Failure and I/O prefix (bridge erases engine variant) |
| LR-9c: cloud legend and rows | Translation matrix | Cloud status, warning level and retained source documented |
| LR-9c: mixed heal + generative source | `lr9c.rs` | Exact retained RetouchAreas text assertion |
| LR-9c: circle CenterWeight assumption/conflict | `retouch.rs`, matrix | Agreement accepted; conflicting inherited Feather fails closed with warning and retained source; RED observed |
| LR-9c: empty DepthBasedCorrections | `noop.rs` | Empty no-op vs nonempty warning regression RED observed |
| LR-9c: pre/post rebase hash mapping | Already done (`121fa5a0`, ancestor of base) | `tools/orchestrate/wp/LR-9/HANDOFF.md`, Hash mapping table |
| Identifier pins, including id:/identifier: | `LibraryDevelopAccessibilityTests.swift`: old pins retained; 202 additional current-base literal stems | Historical review count 102 differs from this base; gate scanner guards all pins, including listed examples |
| Legacy-prefix acceptance | Same file: anchored full dynamic templates | RED unknown row suffix accepted; six isolated Swift helper checks GREEN |
| Keyword tree name collision | Product decision, not changed | Library explicitly enforces global name uniqueness; index rejects different parents; see decision below |
| B5-49: skip focus claim outside document / grid leave test | Dependency-blocked on requested base | B5-49 focus-claim/checklist implementation is absent; see dependency note |
| B5-49: anchor address normalizer | Dependency-blocked on requested base | Normalizer is absent; no unrelated replacement invented |
| B5-49c L1 stop-on-repeat | Fixed on later lane `a6cd0ae8`, not in base | Commit inspected; not claimed already merged |
| B5-49c L2 Shift+Tab History minus | Fixed on later lane `cd281635`, not in base | Commit and B5-49d handoff inspected; tests use six focus arrangements |
| B5-49c L3 thread-safe FKA getter | Fixed on later lane `867adc0d`, not in base | Commit inspected; requested base lacks `KeyboardAccessHarness.swift` |
| B5-49c L4 SwiftUI $hex normalization | Fixed on later lane `af23afbf`, not in base | Commit adds `$[0-9a-f]+`; normalization remains too broad for the separate address-anchor finding |
| Stale layer row/cell identifiers after insert | Done in `LayersOutline.swift` | Stable-source RED: 1 test / 1,200 assertion failures; GREEN: 1 test / 0 failures, after collapse and insertion |
| ENG-4 independent f64 denominator | `pipeline-cpu/src/tone_extra.rs` | Independent `(1.0_f64/0.18).ln_1p()` oracle; exact zero and overflow branches |
| ENG-4 host log constants | Done | Both tests RED before fix; GPU unit suite 27 passed, 2 preexisting ignored |
| ENG-4 identical WGSL helpers | `pipeline-gpu/tests/local_tone_resident.rs` | Compare actual log/exp function bodies across all three shaders |
| ENG-4 overflow literal | Done | All three shaders use 6.1250826e37 |
| ENG-4 near-black presence | Same integration test | Non-ignored resident signed near-black CPU parity, 1e-6 bound |
| Export wall-clock test | `export/tests/workflow.rs` | Release-only; configured timeout stays 1 s; running cancellation waits for child ready then asserts cancelled outcome, with 30 s hang guards |
| Preview wall-clock test | `previews/src/lib.rs` | Release-only; existing 3.0 s assertion retained |
| ml-embed HNSW flake | No test bound/assertion/exclusion changes | Full workspace gate will exercise it; any failure/serialized retry recorded below |

## Production behavior changes

Native point colors without Adobe selection no longer force schema 4. Unsupported
non-Paint mask values no longer render as unit masks. Conflicting circle feather
encodings retain source and warn. Empty depth corrections no longer produce an
unsupported warning. CPU-required rendering selects its chain before admission,
avoiding a discarded Upright analysis. The large-exponent CPU decoder now uses compensated f32 arithmetic to retain
its low exponent bits; the new overflow regression first failed the unchanged
3e-7 relative bound and now passes. The normal-range branch is unchanged. Layer accessibility queries refresh retained cell/control identifiers after row shifts. Both
presence shader paths consume the same host-computed log denominator as curves.
An existing direct-dispatch test fixture needed its parameter arrays updated to
the new ABI; its numeric assertions and limits are unchanged.

The single baseline warning for empty DepthBasedCorrections is deliberately
removed from `lr6b-untranslated-baseline.json` under the explicit LR-9c ruling.
No recipe bytes or other golden values are changed. This is an announced semantic
expectation update, not a repin to suppress a failure.

## Memory and preview notes

A session can retain both its original backend RGB/Upright memo and its lazy CPU
fallback memo. Selecting CPU before admission avoids populating the original memo
for a CPU-required request, but does not free earlier no-spot memo contents. Both
memo lifetimes remain tied to the session; no memory bound is raised. No-spot
preview scale switches retain their preexisting backend/level behavior. MCP owners
must not extrapolate the retouch CPU-chain guarantee to those recipes.

## Dependency and product decisions

The prescribed base does not contain B5-49d: `867adc0d` and `a6cd0ae8` are not
ancestors, and the checklist/FKA harness files are absent. The user was asked
whether to retain the base or integrate an approved tip. Until a tip is specified,
this lane retains the prescribed base and does not silently import another lane.

Keyword names are a global identity in both library operations and the index,
not merely a SwiftUI identity choice. `Library::add_keyword` rejects an existing
name; index insertion rejects an existing keyword with a different parent.
Supporting same-named children beneath different parents needs a decision about
path/ID-based storage, sidecar tags, search, import conflict handling and API
compatibility. Changing only ForEach identity would not resolve ambiguous edits.

## Gates / attempts

Pre-gate attempts:

- Schema: 1 expected RED failure, then 95 unit tests passed.
- Import LR-4/LR-9 regressions: expected RED failures, including a separate
  circle-local Feather agreement failure; final focused import run: 61 passed.
- Public Upright admission: expected RED `[2;5]`; fixed image-core unit run:
  46 passed, 0 failed.
- First broad Rust run: 633 passed, 3 failed, 5 ignored. Failures were the two
  matrix guards (new cloud status and missing no-op annotation) and the new CPU
  overflow oracle. Fixed without reducing bounds; focused matrix suite 18 passed,
  overflow regression 1 passed. The streaming import memory/time test passed.
- GPU ABI regressions: 2 expected RED failures. Resident near-black/helper tests:
  2 passed. First full focused GPU run: 36 passed, 1 failed, 3 ignored: the old
  direct-dispatch fixture omitted the newly required denominator. Updated only
  its ABI inputs; retry: 27 unit tests passed, 2 preexisting ignored. Resident
  integration suite: 10 passed, 1 preexisting ignored.
- Identifier helper: expected RED accepting an unpinned suffix; 6 isolated Swift
  checks passed after anchored templates.
- First Swift layer-probe build exited 1 because the test source was corrected
  during compilation. No RED claim. Stable-source retry: 1 test failed with
  1,200 stale-identifier assertions. A first GREEN command ran from the repository
  root and exited 1 (no Package.swift), before any build/test; correct-directory
  retry: 1 test passed with 0 failures, 2.834 seconds.
- Plain `cargo clean -p …` removed 0 files (default debug artifacts). The first
  workspace gate build was deliberately interrupted (exit 130, no test verdict).
  Corrected `cargo clean --release -p …` removed 1,039 files / 2.1 GiB before the
  final workspace attempt.

- The first post-release-clean workspace build exited 101 before tests: the
  new FFI assertion needed `crate::BridgeError`. Fixed its qualification, cleaned
  tessera-ffi release artifacts again (25 files / 476.7 MiB), and restarted the
  full workspace command.
- Final formatting gate: exit 0. No lockfile, board or generated binding drift.

Remaining final gates pending. Detailed transient logs are kept outside the repository;
no private paths, pixel data or catalog-derived strings are committed as evidence.

## Commits / publication

Pending final commit list, hash and push verification.
