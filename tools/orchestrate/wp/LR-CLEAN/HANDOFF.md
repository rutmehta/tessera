# LR-CLEAN handoff

Base: `gate/b51`, `392c2156` on `wp/LR-CLEAN`. Synthetic inputs only.
Status: STOPPED / NOT GATE-CLEAN. The unchanged preview latency bound failed in
the workspace run and the required serialized retry. No merge approval is claimed.

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
| LR-3: preserve admission validation | `render.rs`: resolve full settings before entering the selected CPU chain | Additional RED invalid NaN settings invoked retouch; GREEN rejects before callback (1 passed) |
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
| ml-embed HNSW flake | No test bound/assertion/exclusion changes | Both HNSW tests passed in the workspace run |

## Production behavior changes

Native point colors without Adobe selection no longer force schema 4. Unsupported
non-Paint mask values no longer render as unit masks. Conflicting circle feather
encodings retain source and warn. Empty depth corrections no longer produce an
unsupported warning. CPU-required rendering selects its chain before admission,
avoiding a discarded Upright analysis while retaining full settings validation before retouch callbacks. A follow-up RED/GREEN pair caught and fixed that validation-order regression. The large-exponent CPU decoder now uses compensated f32 arithmetic to retain
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
- Admission-validation follow-up: expected RED callback count 2 instead of 1
  when invalid NaN settings reached retouch; GREEN 1 passed, 0 failed, 0.07 s.
  Commits `fe808744` / `c8bff729` were made after the workspace run had built its
  test binaries, so that workspace result does not verify the final source tip.
- Completed workspace attempt: **3,275 passed, 1 failed, 99 ignored** across
  640 result summaries; exit 101. Only failing target: `previews --lib`, test
  `tests::raw_without_jpeg_is_rendered`, **3.221841292 s** versus unchanged
  **< 3.0 s**. That suite: 22 passed, 1 failed, 3 ignored. Both HNSW tests passed.
- Required serialized retry: `cargo test --release -p previews --lib --
  --test-threads=1 --nocapture`; **22 passed, 1 failed, 3 ignored**, exit 101,
  suite duration 4.73 s. Same preview test took **3.276844792 s**. No `CI`
  override, test exclusion, assertion edit or bound change was used.
- Stopped further gate attempts after the serialized failure under the user's
  explicit stop-and-report rule. No unrelated preview performance change was
  invented to hide the failure.

| Required final gate | Recorded result |
| --- | --- |
| Release clean of touched crates | Done before workspace attempt: 1,039 files / 2.1 GiB; FFI reclean 25 files / 476.7 MiB. Late admission follow-up still needs a new clean/final gate cycle. |
| `cargo test --release --workspace --no-fail-fast` | FAILED: 3,275 passed / 1 failed / 99 ignored; serialized failing-suite retry also failed. Latest admission follow-up has focused GREEN only. |
| `cargo clippy --release --workspace --all-targets -- -D warnings` | Not run after stop; pending. |
| `cargo fmt --all -- --check` | Earlier check exit 0; latest follow-up was formatted, but final-tip check pending. |
| `cd apps/mac && ./build-ffi.sh` | Preparation build passed with no bindings drift; final-tip gate pending. |
| `tools/orchestrate/swift-gate.sh` | Not run; no SWIFT GATE OK claim. Focused native layer probe: 1 passed / 0 failed. |
| Strict release Tessera build | Not run after stop; pending. |

Detailed transient logs are kept outside the repository; no private paths, pixel
data or catalog-derived strings are committed as evidence. No Cargo.lock, board,
or generated binding changes are present in the committed diff.

## Outstanding work

1. Resolve the preview latency failure without changing the 3.0 s bound, then
   complete all final gates on the final source tip. The serialized retry alone
   does not establish whether the cause is shared-machine load or a performance
   issue, and this handoff makes no such diagnosis.
2. Obtain an approved B5-49d integration base/tip before applying the outside-
   document focus/grid-leave and address-normalizer follow-ups. Later-lane L1–L4
   fixes were inspected, but are not ancestors of this lane. No answer to the
   base/integration clarification was received before stopping.
3. Decide whether keyword names remain globally unique or become path/ID-based
   identities. The existing invariant prevents the hypothesized duplicate-name
   tree; changing that invariant is a product/API decision, not a view-only fix.

## Commits / publication

Implementation tip: `c8bff729`. The final documentation commit records this
blocked handoff; its own hash is reported to the user after publication.
Publication target: `origin/wp/LR-CLEAN`; push verification is reported with the
final hash rather than embedding a self-referential hash here.

- `c95a0d64` test(LR-CLEAN): distinguish native point colors and pin sticky schema writes
- `53b4ab49` test(LR-CLEAN): pin unsupported masks, circle conflicts and empty depth corrections
- `67a70145` test(LR-CLEAN): exercise spot Upright memo through public admission
- `1615aa9d` fix(LR-CLEAN): narrow schema predicate and select CPU chain before admission
- `8edd9e6d` test(LR-CLEAN): reject unpinned legacy accessibility suffixes
- `3659aa16` test(LR-CLEAN): pin host GPU denominators and resident near-black parity
- `dd227299` test(LR-CLEAN): use independent axis oracle and cover zero and overflow
- `85e459fa` test(LR-CLEAN): cover circle-local Feather agreement as well as parent Feather
- `f0151a5d` fix(LR-CLEAN): reject unsupported mask values and conflicting circle feather encodings
- `0e8f6071` test(LR-CLEAN): restore import assertions and separate cancellation from scheduler latency
- `d7002dd7` fix(LR-CLEAN): share host GPU axis constants and compensate CPU overflow rounding
- `049ed0fa` test(LR-CLEAN): regenerate identifier pins and anchor legacy templates
- `985d66cd` docs(LR-CLEAN): clarify cloud, circle and cross-application luminance semantics
- `5f006db3` test(LR-CLEAN): reproduce stale layer control identifiers after row shifts
- `dddb544d` test(LR-CLEAN): wait for complete readiness marker and qualify bridge error
- `25c56b58` fix(LR-CLEAN): refresh layer cell identifiers when realized rows move
- `a6218ea3` docs(LR-CLEAN): map all rulings, production changes and dependency limits
- `fe808744` test(LR-CLEAN): preserve validation before CPU retouch admission
- `c8bff729` fix(LR-CLEAN): retain full settings admission before the selected CPU chain
