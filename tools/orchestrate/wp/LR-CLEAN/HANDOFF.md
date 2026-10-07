# LR-CLEAN handoff

Lane: LR-CLEAN2 on `wp/LR-CLEAN`. Synthetic inputs only.
Base: `origin/main` at `a94b0288`. All 20 lane commits from the old base
`392c2156` were rebased with `git rebase --onto origin/main 392c2156`.
Rebase conflicts: none. Main's keyboard, AI-mask and local-adjustment work is retained.
Status: GATE-CLEAN on the final source tip; results appear in the LR-CLEAN2 section below.
The original LR-CLEAN attempts are retained as historical evidence.

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
| Transform sliders broken by anchoring (found in LR-CLEAN2 Swift gate) | Same file: `transformSliders` exact pins (`2c0dca28` RED, `2259f0fe` fix) | Concatenated `"transform-" + path` ids were accepted on main by prefix; anchored pins rejected them (14 window-walk failures). Seven exact pins, equality-checked against `TransformControls.all`; `transform-unpinned` stays rejected |
| Keyword tree name collision | Product decision, not changed | Library explicitly enforces global name uniqueness; index rejects different parents; see decision below |
| B5-49: skip focus claim outside document / grid leave test | Fixed in LR-CLEAN2 (`DocumentWorkspace.setPanelsHidden`; `d31866eb` RED, `b441ec8c` fix) | Restoration claims focus only in document mode; leave test enters `.grid` and preserves the prior responder |
| B5-49: anchor address normalizer | Fixed in LR-CLEAN2 (`DocumentKeyboardChecklistTests.normalizeAddresses`; `d31866eb` RED, `b441ec8c` fix) | Anchor hex addresses to complete `ObjectIdentifier(0x…)`; diagnostic hex values remain unchanged; existing SwiftUI `$hex` support retained |
| B5-49c L1 stop-on-repeat | already done (commit `a6cd0ae8`) | Main ancestry verified; row-exit walk stops when a view repeats |
| B5-49c L2 Shift+Tab History minus | already done (commit `cd281635`) | Main ancestry verified; identity-checked History minus destination and six focus arrangements retained |
| B5-49c L3 thread-safe FKA getter | already done (commit `867adc0d`) | Main ancestry verified; FKA getter reads lock-protected state; off-main-thread regression retained |
| B5-49c L4 SwiftUI $hex normalization | already done (commit `af23afbf`) | Main ancestry verified; `$[0-9a-f]+` normalization and its existing assertions retained |
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

The LR-CLEAN2 rebase resolves the previous keyboard dependency block. B5-49d
L1–L4 are present on main, as are the B5-49e review fixes. Only the document-mode
focus guard and ObjectIdentifier address anchor required additional changes.

Keyword names are a global identity in both library operations and the index,
not merely a SwiftUI identity choice. `Library::add_keyword` rejects an existing
name; index insertion rejects an existing keyword with a different parent.
Supporting same-named children beneath different parents needs a decision about
path/ID-based storage, sidecar tags, search, import conflict handling and API
compatibility. Changing only ForEach identity would not resolve ambiguous edits.

## Historical LR-CLEAN gates / attempts (before rebase)

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

## Pending product decision

Keyword-name collisions remain intentionally unimplemented. Decide whether names
remain globally unique or become path/ID-based identities before changing storage,
sidecars, search, import conflict handling or API behavior.

## LR-CLEAN2 verification

- RED: both focused regressions failed as expected: grid exit claimed the canvas;
  broad normalization erased diagnostic hex and partially rewrote an invalid identifier.
- RED: transform pins: 7 acceptance failures before the fix; GREEN: class 11 tests, 0 failures.
- All gate logs remain outside the repository. No live library inputs are used.
- `cargo clean -p` of all ten touched crates before the gates.

| Gate (source tip) | Result | 1/5/15-min load |
| --- | --- | --- |
| `cargo test --release --workspace --no-fail-fast` (`b441ec8c`) | PASS: 3287 passed, 0 failed, 99 ignored (640 result lines); `raw_without_jpeg_is_rendered` ok first try, no serialized rerun needed | start 17.5/23.3/25.7; end 24.5/37.0/38.9 |
| `cargo clippy --release --workspace --all-targets -- -D warnings` | PASS | 16.8/32.0/36.9 |
| `cargo fmt --all -- --check` | PASS | n/a |
| `apps/mac/build-ffi.sh` | PASS, no bindings drift | n/a |
| `tools/orchestrate/swift-gate.sh` (`b441ec8c`) | FAILED: `testLibraryAndDevelopWindows` (14 transform slider failures) | 41.7/33.0/36.0 |
| `tools/orchestrate/swift-gate.sh` (`2259f0fe`) | SWIFT GATE OK: 986 executed, 3 skipped, 0 failures; Swift Testing 5 passed | start 9.9/16.2/25.0 |
| strict release `swift build --product Tessera` | PASS (only the existing ld min-version warning from the FFI archive) | end 45.7/25.9/24.6 |

The transform commits change only a Swift test file, so the Rust gates on
`b441ec8c` apply unchanged to the final source tip.

## Commits / publication

The list below records original pre-rebase commit IDs for historical traceability.
Current publication hash is reported to the user after pushing the final handoff.
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

LR-CLEAN2 commits after the rebase:

- `d31866eb` test(LR-CLEAN2): expose grid-exit focus claim and broad hex normalization
- `b441ec8c` fix(LR-CLEAN2): preserve grid focus and anchor object address normalization
- `2c0dca28` test(LR-CLEAN2): require pinned Transform slider identifiers
- `2259f0fe` fix(LR-CLEAN2): pin Transform slider identifiers exactly

## Rebase mapping

All 20 entries are patch-equivalent (`git range-diff` reports `=` for each).

| Original | Rebased |
| --- | --- |
| `c95a0d64` | `0be255fc` |
| `53b4ab49` | `cd77b00b` |
| `67a70145` | `4019fe21` |
| `1615aa9d` | `93379972` |
| `8edd9e6d` | `79ba0a63` |
| `3659aa16` | `919a7ea0` |
| `dd227299` | `0d69574d` |
| `85e459fa` | `68594a78` |
| `f0151a5d` | `b7f2a083` |
| `0e8f6071` | `3adda07b` |
| `d7002dd7` | `933e16dc` |
| `049ed0fa` | `8a835ea2` |
| `985d66cd` | `e6ca823d` |
| `5f006db3` | `d83c470d` |
| `dddb544d` | `3965c044` |
| `25c56b58` | `b8ade1b3` |
| `a6218ea3` | `40bdc98c` |
| `fe808744` | `bdeaa26c` |
| `c8bff729` | `48715091` |
| `df01e5ef` | `e469f2b0` |
