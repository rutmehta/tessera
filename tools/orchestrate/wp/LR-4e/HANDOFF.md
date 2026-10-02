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

