# LR-4e review follow-up

Local branch `wp/LR-4-parametric-masks`, rebased without squashing onto
`486d069f` (`origin/main` fetched 2026-10-01). This includes `02ae8196`, LR-DIAG,
and LR-7. The untracked `LR-RULINGS-FROM-A.md` belongs to the coordinator and is
not included in lane commits.

## Reconciliation

Main owns the schema first-lane checklist and approximate matrix guard. The
lane-local schema-version relaxation, sidecar/merge normalization and journal
v4 test were dropped. Main's versions survive. Every base matrix row survives;
only LR-4 rows change. LR-4 registers disabled components, nested groups,
four-bound luminance, and display-domain luminance predicates and corresponding
bumped-only-when-present tests. `RECIPE_SCHEMA_VERSION` remains 3.

The foreign-mask decoder audit is required for atomic retention of unsupported
parents, not merely for the schema checklist. It now uses the catalog transaction
API, with LR-7 geometry apply/finish owning the single Import history record.
A combined mask + homography regression checks that exact one-entry invariant.
Nested matrix fixture input handling was retained on top of the main guard;
LR-7 context and the shared approximate/ignored checks remain intact.

## Review items

- `MaskKind::LuminanceRange.luminance_domain`: default `linear`, omitted when
  linear; Adobe range imports explicitly use `display`. CPU and resident GPU
  select the matching luminance coordinate. The FFI eyedropper uses the shared
  domain conversion and creates linear masks. Native XMP preserves the display
  extension. Existing linear mask JSON bytes and original
  `luminance_and_depth_ranges` input/output assertions are preserved.
- Swift handles `.group`, supplies `enabled` to regenerated component records,
  and only chooses enabled additive non-inverted brushes. Tests include a
  disabled brush followed by a group and another brush.
- The eight-component-level / 65536-component cap is enforced for settings,
  history base, retouch areas, recipe edits/writes/validation, FFI settings and
  component setters, and MCP create/adjust. Disabled nodes count toward depth.
  Rejected setter calls are checked before live state/storage changes.
- Composition remains depth-bounded, not two-buffer. At cap, budget roughly ten
  full `f32` alpha planes (~960 MB at 24 MP), excluding RGB, cached rasters and
  guided refinement. The older LR-4 handoff has been corrected too.
- Gradient/radial `MaskValue` remains approximate: exact source survives and a
  shared diagnostic explicitly says the value is not reproduced and unit
  selection is used. Paint diagnostic bytes remain pinned. Explicit Paint blend
  modes override zero-value implicit subtraction.
- The exact fixture schema assertions from main are retained. The matrix's
  retained-key rejection asserts its exact error string.
- EXIF 6/8 regression uses an imported crop and gradient and checks both in the
  sensor frame. Nested GPU fallback has a no-group control.
- All six requested reader files use `diagnostics::entries()`. A source scan of
  `crates/**/*.rs` finds no `lrcat_develop_diagnostics` writer or reader.

## Validation

RED: `red-schema.log`: display domain was dropped and Recipe::validate accepted
nine levels (2 failed, 3 passed). First import run exposed an accidental Paint
diagnostic byte change; restored the original Paint reason and pointer. A new
zero-Paint test initially had an unbalanced synthetic Lua literal; corrected it.

Initial full required-package Rust run: exit 0, 2007 passed, 68 ignored, 330
summaries, including doctests (`gate-tests-initial.log`). The prescribed initial
package clean removed 40.6 GiB; an additional explicit release clean removed
6.6 GiB before the final rebuild (`gate-clean-release.log`).

Additional focused corrections: generated MCP schemas now mirror
`LuminanceDomain` and its serde omission helper. The new resident no-group
control uses the existing filter inventory's 1e-4 GPU arithmetic tolerance;
its measured difference was 1.35e-5. The nested fallback retains exact equality.
The older nested cache test now explicitly declares its display domain and
also proves domain changes invalidate its alpha cache.

All Cargo commands used `PATH="$HOME/.cargo/bin:$PATH"`,
`CARGO_TARGET_DIR=$HOME/.cache/tessera-target/LR-4-parametric-masks`,
`CARGO_BUILD_JOBS=3`, and `RAYON_NUM_THREADS=3`.

| Gate | Result | Evidence |
| --- | --- | --- |
| Clean then `cargo test --release --locked --no-fail-fast -p import-lrcat -p engine-api -p pipeline-cpu -p pipeline-gpu -p compositor -p filters -p sidecar -p image-core -p merge -p tessera-ffi -p tessera-mcp` | PASS: 2007 passed, 68 ignored, zero failures across 330 summaries including doctests | `gate-tests.log`, `gate-tests.exit` |
| Feature-unified external mask tests (`--test lr4b_bounds --test lr4_nested`, including `-p mask-ai`) | PASS: 5 tests | `gate-external-masks.log` |
| `cargo clippy --locked --workspace --all-targets --release -- -D warnings` | PASS | `gate-clippy.log` |
| `cargo fmt --all -- --check` | PASS | `gate-fmt.log` |
| `cd apps/mac && ./build-ffi.sh` | PASS: regenerated Swift bindings and arm64 static library | `gate-build-ffi.log` |
| `tools/orchestrate/swift-gate.sh` | SWIFT GATE OK: 920 XCTest tests, 3 skipped, zero failures; 5 Swift Testing tests passed | `gate-swift.log` |
| `cd apps/mac && swift build -c release --product Tessera -Xswiftc -strict-concurrency=complete -Xswiftc -warnings-as-errors` | PASS: product Tessera built in 136.54 s | `gate-swift-release.log` |

The strict Swift build exited 0 with no Swift compiler warnings. The linker
emitted a deployment-target warning: `blake3_neon.o` in the FFI static archive
was built for macOS 26.2 while the app links for macOS 15.0. Runtime compatibility
on macOS 15 was not verified by these build gates.

No Liquify failure occurred in either full Rust run; no serial rerun or threshold
change was needed. Initial Clippy found one redundant borrow in the new MCP test;
that was fixed, Clippy passed, and the affected MCP regression passed again
(`gate-mcp-post-lint.log`). Initial failure evidence is retained.

## Commits and boundaries

- `b9c56308` — test-first LR-4e regressions.
- `7c2e46f3` — luminance compatibility, validation, import diagnostics and Swift fixes.
- `fcc08d27` — external composition domain coverage and MCP lint correction.
- `0e17c7a4` — regenerated Swift FFI bindings.

All new commit bodies end with the requested co-author trailer. Rebase preserved
individual commits; nothing was pushed. No dependency, `Cargo.lock`, or board
changes. No GUI was opened and no real user Lightroom catalog was read. The
only pre-existing untracked file is the coordinator's `LR-RULINGS-FROM-A.md`.
Generated UniFFI Swift retains the generator's existing trailing-space style;
non-generated changes pass `git diff --check`.


## LR-4f predecessor integration (2026-10-02)

Rebased all 31 LR-4 commits without squashing from `4ecf521d` onto fetched
`origin/wp/LR-1-point-color` at `095de74aa3687143739abd7eda15c3ff95a37d3b`
(main + LR-2 through 2f + LR-1 through 1c). This is a local pre-stack; no push.

Every textual conflict (original replayed commit IDs):

| Replayed commit | File | Resolution |
| --- | --- | --- |
| `943a38a6` | `crates/import-lrcat/src/lua_develop.rs` | Combined LR-4's mask retention exclusion with LR-2's legacy/stale-modern-control retention. The final parser uses predecessor `parse_inner(..., false)` so Lua hooks run once and LR-7 finish owns history. |
| `943a38a6` | `crates/import-lrcat/src/xmp.rs` | Combined mask exclusion and legacy retention; preserved LR-1 PointColors diagnostics/hooks. |
| `8af368d6` | `crates/engine-api/src/recipe/schema.rs` | Kept all LR-2 feature tests and appended all LR-4 mask feature tests. |
| `bff98c61` | `crates/engine-api/src/recipe/schema.rs` | Union of monochrome, extended curves, legacy tone and mask predicates. |
| `bff98c61` | `crates/import-lrcat/src/lua_develop.rs` | Applied LR-4c's deliberate removal of the translated-mask retention exclusion, preserving LR-2 stale-modern controls and retained source. |
| `bff98c61` | `crates/import-lrcat/src/xmp.rs` | Same LR-4c retention evolution, preserving LR-1 diagnostics and LR-2 legacy retention/hooks. |
| `15434970` | `docs/coordination/LR-TRANSLATION-MATRIX.md` | Kept all predecessor rows, including Blacks and Recovery; added LR-4 nested range/Paint rows and applied LR-4 row updates. |
| `7c2e46f3` | `crates/engine-api/src/recipe/schema.rs` | Kept PointColors predicate and added display-luminance mask predicate; all prior predicates/tests survive. |
| `7c2e46f3` | `crates/import-lrcat/tests/translation_matrix.rs` | Combined LR-2 HDR context with LR-4 slash-qualified root-key import handling; retained main's approximate/ignored and exact-retention guards. |
| `7c2e46f3` | `crates/tessera-mcp/tests/console.rs` | Kept both independent tests: LR-1c Point Color before monochrome, and LR-4e rejection of a ninth mask level without saving. |

`pipeline-gpu/src/batch.rs` had no textual conflict and is identical to the
predecessor: legacy-tone, PointColors, ToneExtra and geometry CPU fallbacks all
survive. Main's first-lane schema checklist remains intact. LR-7's shared finish
records history; no new lane history writer was added. Point Color remains
before B&W, and LR-4 masks remain before Upright.

The single new integration regression is
`crates/tessera-ffi/tests/lr4f_combined_import.rs`. It updates a row in a generated
fixture catalog with PointColors, monochrome/mixer, a nested luminance mask and
Upright homography. It verifies one Import author entry, required and serialized
schema 4, fields for every lane, diagnostics read only through
`diagnostics::entries()`, and finite nonzero full CPU output that changes when
the imported local mask is removed. No dependencies or real catalogs are used.

### LR-4f gates

Environment for every Cargo/Swift gate: `PATH="$HOME/.cargo/bin:$PATH"`,
`CARGO_TARGET_DIR=$HOME/.cache/tessera-target/LR-4-parametric-masks`,
`CARGO_BUILD_JOBS=4`, `RAYON_NUM_THREADS=4`. Explicit release clean of all LR-4
touched crates removed 8.0 GiB; predecessor-only `pipeline-adobe` was cleaned
before the full run too. Existing repo `fixtures/raw` was used.

| Gate | Result | Evidence in `../LR-4f/` |
| --- | --- | --- |
| Focused combined import regression, release | PASS: 1 test | `combined.log` |
| `cargo test --release --locked --no-fail-fast -p import-lrcat -p engine-api -p pipeline-cpu -p pipeline-gpu -p compositor -p filters -p sidecar -p image-core -p merge -p export -p tessera-ffi -p tessera-mcp` | PASS: 2193 passed, 0 failed, 75 ignored; 374 summaries including doctests; no command-level exclusions | `tests.log`, `tests.exit` |
| `cargo clippy --release --locked --workspace --all-targets -- -D warnings` | PASS | `clippy.log`, `clippy.exit` |
| `cargo fmt --all -- --check` | PASS | `fmt.log`, `fmt.exit` |
| `cd apps/mac && ./build-ffi.sh` | PASS: arm64 archive; regenerated bindings match tracked files | `build-ffi.log`, `build-ffi.exit` |
| `tools/orchestrate/swift-gate.sh` | SWIFT GATE OK: 920 XCTest tests, 3 skipped, 0 failures; 5 Swift Testing tests passed | `swift.log`, `swift.exit` |
| `cd apps/mac && swift build -c release --product Tessera -Xswiftc -strict-concurrency=complete -Xswiftc -warnings-as-errors` | PASS: 143.15 s | `swift-release.log`, `swift-release.exit` |

The strict build had no Swift compiler warnings. The linker repeated the known
`blake3_neon.o` warning (object built for macOS 26.2, app links for 15.0).
The FFI rebuild also printed LibRaw C deprecation warnings. Neither failed a
requested gate; macOS 15 runtime compatibility is not established by these builds.

All requested gates passed on the first full run. The matrix audit retained all
111 predecessor rows unchanged outside LR-4 and added seven LR-4 rows. No
`Cargo.lock`, dependency manifests, or `board.json` changed, no GUI was opened,
and no user's real catalog was read. Only generated catalogs and repository
fixtures were used. The pre-existing untracked coordinator rulings file remains
untouched. Regenerated FFI files had no tracked differences.

New commits: `2b3685dd` is the single LR-4f test/integration commit; the following
`docs(LR-4f)` commit records this handoff and gate evidence. Both end with the
requested co-author trailer. All commits remain local for the coordinator to
force-push; all 31 original LR-4 commits were preserved individually.
